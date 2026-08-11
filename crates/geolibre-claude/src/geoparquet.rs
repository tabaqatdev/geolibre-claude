//! GeoParquet backend for `describe_layer` / `query_data` — the second data format.
//!
//! Same structured query shape as the ArcGIS backend, translated to DuckDB SQL over
//! `read_parquet`. Because reading the parquet needs file/URL access on, the sandbox
//! here is different from `spatial_sql`: the read path is **server-controlled**
//! (validated against an allowlist), and the model-supplied WHERE / field lists are
//! **guarded** (no readers, no subqueries) so they can only reference fields.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::catalog::{field_value_type, QueryDataArgs};
use crate::sql::{ensure_fragment_safe, is_ident_list};

/// Where GeoParquet may be read from. Everything else is refused.
#[derive(Clone, Default)]
pub struct GeoParquetConfig {
    /// Local files must resolve to a path under this directory.
    pub local_root: Option<PathBuf>,
    /// Remote files must start with this https/s3 prefix.
    pub url_prefix: Option<String>,
}

/// Is this layer reference a GeoParquet source (vs an ArcGIS URL)?
pub fn is_geoparquet_ref(layer_ref: &str) -> bool {
    layer_ref.trim().to_ascii_lowercase().ends_with(".parquet")
}

impl GeoParquetConfig {
    /// Validate a parquet reference and return the read target DuckDB should open,
    /// or an error if it's outside the allowlist.
    pub fn resolve(&self, layer_ref: &str) -> Result<String> {
        let r = layer_ref.trim();
        if r.starts_with("http://") || r.starts_with("https://") || r.starts_with("s3://") {
            match &self.url_prefix {
                Some(prefix) if r.starts_with(prefix.as_str()) => Ok(r.to_string()),
                Some(_) => anyhow::bail!("remote parquet `{r}` is outside GEOLIBRE_PARQUET_URL_PREFIX"),
                None => anyhow::bail!("remote parquet is disabled (set GEOLIBRE_PARQUET_URL_PREFIX)"),
            }
        } else {
            let root = self
                .local_root
                .as_ref()
                .context("local parquet is disabled (set GEOLIBRE_PARQUET_ROOT)")?;
            let root = root.canonicalize().context("GEOLIBRE_PARQUET_ROOT does not exist")?;
            // Resolve relative to the root, then confirm it stays inside it.
            let candidate = root.join(r);
            let resolved = candidate
                .canonicalize()
                .with_context(|| format!("parquet `{r}` not found under the allowed root"))?;
            if !resolved.starts_with(&root) {
                anyhow::bail!("parquet `{r}` escapes GEOLIBRE_PARQUET_ROOT");
            }
            Ok(resolved.to_string_lossy().into_owned())
        }
    }
}

fn open_conn(read_target: &str) -> Result<duckdb::Connection> {
    let config = duckdb::Config::default()
        .max_memory("1GB")
        .and_then(|c| c.threads(2))
        .context("configure DuckDB limits")?;
    let conn =
        duckdb::Connection::open_in_memory_with_flags(config).context("open in-memory DuckDB")?;

    let home = std::env::temp_dir().join("geolibre-claude-duckdb");
    std::fs::create_dir_all(&home).ok();
    conn.execute_batch(&format!("SET home_directory='{}';", home.to_string_lossy().replace('\'', "''")))
        .context("set DuckDB home_directory")?;

    // parquet (read_parquet) + spatial (ST_ funcs); httpfs only when reading remotely.
    // Loaded explicitly because we disable autoload below for hardening.
    let mut load =
        String::from("INSTALL parquet; LOAD parquet; INSTALL spatial; LOAD spatial; INSTALL json; LOAD json;");
    if read_target.starts_with("http") || read_target.starts_with("s3://") {
        load.push_str(" INSTALL httpfs; LOAD httpfs;");
    }
    conn.execute_batch(&load).context("load DuckDB extensions")?;

    // Lock the sandbox down as far as we can while still allowing the parquet read:
    // no new/community extensions, no config changes. (external_access must stay on.)
    conn.execute_batch(
        "SET autoinstall_known_extensions=false; SET autoload_known_extensions=false; \
         SET allow_community_extensions=false; SET max_expression_depth=1000; \
         SET lock_configuration=true;",
    )
    .context("harden DuckDB")?;
    Ok(conn)
}

fn quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Format one coordinate pair as WKT `x y` (numeric only — never model text, so
/// there is nothing to escape and no injection surface).
fn wkt_xy(pt: &Value) -> Option<String> {
    let arr = pt.as_array()?;
    let x = arr.first()?.as_f64()?;
    let y = arr.get(1)?.as_f64()?;
    Some(format!("{x} {y}"))
}

/// Join an array of `[x,y]` positions into a WKT coordinate list `x0 y0, x1 y1, …`.
fn wkt_ring(positions: &Value) -> Option<String> {
    let pts: Vec<String> = positions.as_array()?.iter().filter_map(wkt_xy).collect();
    if pts.len() < 2 {
        return None;
    }
    Some(pts.join(", "))
}

/// Convert an ESRI geometry JSON into a WKT literal DuckDB Spatial can parse.
///
/// Inferred from the JSON shape (robust to a missing/loose `geometryType`):
/// `{x,y}`→POINT, `{points}`→MULTIPOINT, `{paths}`→(MULTI)LINESTRING,
/// `{rings}`→POLYGON (first ring = shell, the rest = holes),
/// `{xmin,ymin,xmax,ymax}`→POLYGON envelope. All values are numbers.
fn esri_to_wkt(geometry: &Value) -> Option<String> {
    let n = |k: &str| geometry.get(k).and_then(|v| v.as_f64());

    // Envelope
    if let (Some(xmin), Some(ymin), Some(xmax), Some(ymax)) =
        (n("xmin"), n("ymin"), n("xmax"), n("ymax"))
    {
        return Some(format!(
            "POLYGON(({xmin} {ymin}, {xmax} {ymin}, {xmax} {ymax}, {xmin} {ymax}, {xmin} {ymin}))"
        ));
    }
    // Point
    if let (Some(x), Some(y)) = (n("x"), n("y")) {
        return Some(format!("POINT({x} {y})"));
    }
    // Multipoint
    if let Some(points) = geometry.get("points") {
        let pts: Vec<String> = points.as_array()?.iter().filter_map(wkt_xy).collect();
        if pts.is_empty() {
            return None;
        }
        return Some(format!("MULTIPOINT({})", pts.join(", ")));
    }
    // Polyline
    if let Some(paths) = geometry.get("paths").and_then(|p| p.as_array()) {
        let mut parts: Vec<String> = paths.iter().filter_map(wkt_ring).collect();
        match parts.len() {
            0 => return None,
            1 => return Some(format!("LINESTRING({})", parts.remove(0))),
            _ => {
                let inner: Vec<String> = parts.into_iter().map(|p| format!("({p})")).collect();
                return Some(format!("MULTILINESTRING({})", inner.join(", ")));
            }
        }
    }
    // Polygon — first ring is the shell, remaining rings are holes.
    if let Some(rings) = geometry.get("rings").and_then(|r| r.as_array()) {
        let inner: Vec<String> = rings.iter().filter_map(wkt_ring).map(|r| format!("({r})")).collect();
        if inner.is_empty() {
            return None;
        }
        return Some(format!("POLYGON({})", inner.join(", ")));
    }
    None
}

/// Map an ESRI spatial relationship to the DuckDB Spatial predicate.
fn spatial_func(spatial_rel: &str) -> &'static str {
    match spatial_rel {
        "esriSpatialRelContains" => "ST_Contains",
        "esriSpatialRelWithin" => "ST_Within",
        "esriSpatialRelCrosses" => "ST_Crosses",
        "esriSpatialRelOverlaps" => "ST_Overlaps",
        "esriSpatialRelTouches" => "ST_Touches",
        _ => "ST_Intersects",
    }
}

/// Column name + type for each column in the parquet.
fn columns(conn: &duckdb::Connection, from: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(&format!("DESCRIBE SELECT * FROM {from}"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Best-effort geometry column: GeoParquet's standard `geometry`, or a GEOMETRY/BLOB column.
fn geom_col(cols: &[(String, String)]) -> Option<String> {
    cols.iter()
        .find(|(n, t)| {
            n.eq_ignore_ascii_case("geometry")
                || n.eq_ignore_ascii_case("geom")
                || t.to_ascii_uppercase().contains("GEOMETRY")
                || t.eq_ignore_ascii_case("BLOB")
        })
        .map(|(n, _)| n.clone())
}

/// Run a SELECT and return its rows as JSON (via `to_json`, so no hand-mapping).
fn json_rows(conn: &duckdb::Connection, select_sql: &str, max_rows: usize) -> Result<Vec<Value>> {
    let interrupt = conn.interrupt_handle();
    let done = Arc::new(AtomicBool::new(false));
    let done_w = done.clone();
    let watchdog = std::thread::spawn(move || {
        let mut waited = 0u64;
        while waited < 28_000 {
            std::thread::sleep(Duration::from_millis(100));
            if done_w.load(Ordering::Relaxed) {
                return;
            }
            waited += 100;
        }
        interrupt.interrupt();
    });

    let result = (|| -> Result<Vec<Value>> {
        let wrapped = format!("SELECT to_json(sub) FROM ({select_sql}) AS sub");
        let mut stmt = conn.prepare(&wrapped).context("prepare query")?;
        let mut rows = stmt.query([]).context("execute query")?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("read row")? {
            if out.len() >= max_rows {
                break;
            }
            let s: String = row.get(0).context("read row json")?;
            out.push(serde_json::from_str(&s).unwrap_or(Value::String(s)));
        }
        Ok(out)
    })();

    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    result
}

/// ESRI-shaped description of a GeoParquet layer (fields + sample values + hint).
pub fn describe(read_target: &str) -> Result<Value> {
    let conn = open_conn(read_target)?;
    let from = format!("read_parquet('{}')", quote(read_target));
    let cols = columns(&conn, &from)?;
    let geom = geom_col(&cols);

    // Sample non-geometry columns (geometry is a WKB blob and doesn't JSON-encode).
    let sample_select = match &geom {
        Some(g) => format!("SELECT * EXCLUDE(\"{}\") FROM {from} LIMIT 5", g.replace('"', "")),
        None => format!("SELECT * FROM {from} LIMIT 5"),
    };
    let samples = json_rows(&conn, &sample_select, 5).unwrap_or_default();

    let fields: Vec<Value> = cols
        .iter()
        .map(|(name, ty)| {
            let sample_values: Vec<Value> = samples
                .iter()
                .filter_map(|row| row.get(name).cloned())
                .filter(|v| !v.is_null())
                .take(3)
                .collect();
            serde_json::json!({
                "name": name,
                "type": ty,
                "alias": name,
                "fieldValueType": field_value_type(name, ty),
                "sampleValues": sample_values,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "source": "geoparquet",
        "path": read_target,
        "geometryColumn": geom,
        "fields": fields,
    }))
}

/// ESRI-compatible structured query over a GeoParquet layer: attribute (`where`),
/// statistics, and spatial-geometry filters (point/multipoint/polyline/polygon/
/// envelope, with optional buffer) — full parity with the ArcGIS backend.
pub fn query(read_target: &str, p: &QueryDataArgs<'_>) -> Result<Value> {
    // Guard everything the model supplied.
    ensure_fragment_safe(p.where_clause).map_err(|e| anyhow::anyhow!("where rejected — {e}"))?;
    if !is_ident_list(p.out_fields) {
        anyhow::bail!("out_fields must be a simple field list");
    }
    if let Some(g) = p.group_by {
        if !is_ident_list(g) {
            anyhow::bail!("group_by must be a field name");
        }
    }
    if let Some(o) = p.order_by {
        if !is_ident_list(o) {
            anyhow::bail!("order_by_fields must be a field list");
        }
    }
    for (_, f) in p.statistics {
        if !is_ident_list(f) {
            anyhow::bail!("statistics field must be a field name");
        }
    }

    let conn = open_conn(read_target)?;
    let from = format!("read_parquet('{}')", quote(read_target));
    let cols = columns(&conn, &from)?;
    let geom = geom_col(&cols);

    // Attribute filter, plus an optional spatial filter. All ESRI geometry types
    // (envelope, point, multipoint, polyline, polygon) are translated to WKT and
    // applied with the requested predicate. `buffer_distance` grows the query
    // geometry with ST_Buffer — in the layer's own CRS units (degrees for
    // EPSG:4326), mirroring the "reproject for metric buffers" rule in the
    // geoprocessing skill; ArcGIS buffers server-side in the requested `units`.
    let mut clauses = vec![format!("({})", p.where_clause)];
    if let (Some(g), Some(geometry)) = (&geom, p.geometry) {
        if let Some(wkt) = esri_to_wkt(geometry) {
            let func = spatial_func(p.spatial_rel);
            let mut query_geom = format!("ST_GeomFromText('{}')", quote(&wkt));
            if let Some(d) = p.buffer_distance {
                if d != 0.0 {
                    query_geom = format!("ST_Buffer({query_geom}, {d})");
                }
            }
            clauses.push(format!("{func}(ST_GeomFromWKB(\"{g}\"), {query_geom})"));
        }
    }
    let where_sql = format!("WHERE {}", clauses.join(" AND "));

    if !p.statistics.is_empty() {
        let mut select: Vec<String> = Vec::new();
        if let Some(g) = p.group_by {
            select.push(g.to_string());
        }
        for (kind, field) in p.statistics {
            let func = match kind.to_ascii_lowercase().as_str() {
                "avg" | "mean" => "avg",
                "min" => "min",
                "max" => "max",
                "sum" => "sum",
                "count" => "count",
                "stddev" | "std" => "stddev",
                "var" | "variance" => "variance",
                other => anyhow::bail!("unknown statistic `{other}`"),
            };
            select.push(format!("{func}({field}) AS {kind}_{field}"));
        }
        let mut sql = format!("SELECT {} FROM {from} {where_sql}", select.join(", "));
        if let Some(g) = p.group_by {
            sql.push_str(&format!(" GROUP BY {g}"));
        }
        let stats = json_rows(&conn, &sql, 1000)?;
        return Ok(serde_json::json!({ "source": "geoparquet", "statistics": stats }));
    }

    // Record path. Exclude the geometry blob unless the caller wants it as GeoJSON.
    let projection = if p.out_fields.trim() == "*" {
        match &geom {
            Some(g) if p.return_geometry => {
                format!("* EXCLUDE(\"{g}\"), ST_AsGeoJSON(\"{g}\") AS geometry")
            }
            Some(g) => format!("* EXCLUDE(\"{g}\")"),
            None => "*".to_string(),
        }
    } else {
        p.out_fields.to_string()
    };

    let mut sql = format!("SELECT {projection} FROM {from} {where_sql}");
    if let Some(o) = p.order_by {
        sql.push_str(&format!(" ORDER BY {o}"));
    }
    sql.push_str(&format!(" LIMIT {} OFFSET {}", p.page_size.min(1000), p.record_offset));

    let records: Vec<Value> = json_rows(&conn, &sql, 1000)?
        .into_iter()
        .map(|attrs| serde_json::json!({ "attributes": attrs }))
        .collect();

    let total: Option<i64> = json_rows(&conn, &format!("SELECT count(*) AS n FROM {from} {where_sql}"), 1)?
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|n| n.as_i64());

    Ok(serde_json::json!({
        "source": "geoparquet",
        "resultRecords": records,
        "totalMatchingRecords": total,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esri_to_wkt_covers_all_geometry_types() {
        assert_eq!(
            esri_to_wkt(&serde_json::json!({"x": 46.7, "y": 24.6})).unwrap(),
            "POINT(46.7 24.6)"
        );
        assert_eq!(
            esri_to_wkt(&serde_json::json!({"xmin": 0, "ymin": 0, "xmax": 2, "ymax": 2})).unwrap(),
            "POLYGON((0 0, 2 0, 2 2, 0 2, 0 0))"
        );
        assert_eq!(
            esri_to_wkt(&serde_json::json!({"points": [[0, 0], [1, 1]]})).unwrap(),
            "MULTIPOINT(0 0, 1 1)"
        );
        assert_eq!(
            esri_to_wkt(&serde_json::json!({"paths": [[[0, 0], [1, 1]]]})).unwrap(),
            "LINESTRING(0 0, 1 1)"
        );
        assert_eq!(
            esri_to_wkt(&serde_json::json!({"paths": [[[0, 0], [1, 1]], [[2, 2], [3, 3]]]})).unwrap(),
            "MULTILINESTRING((0 0, 1 1), (2 2, 3 3))"
        );
        assert_eq!(
            esri_to_wkt(&serde_json::json!({"rings": [[[0, 0], [2, 0], [2, 2], [0, 0]]]})).unwrap(),
            "POLYGON((0 0, 2 0, 2 2, 0 0))"
        );
        // Polygon with a hole (shell + inner ring).
        assert_eq!(
            esri_to_wkt(&serde_json::json!({"rings": [
                [[0, 0], [9, 0], [9, 9], [0, 0]],
                [[3, 3], [4, 3], [4, 4], [3, 3]]
            ]}))
            .unwrap(),
            "POLYGON((0 0, 9 0, 9 9, 0 0), (3 3, 4 3, 4 4, 3 3))"
        );
        // Unrecognized / empty → None (no spatial clause applied).
        assert!(esri_to_wkt(&serde_json::json!({"foo": 1})).is_none());
        assert!(esri_to_wkt(&serde_json::json!({"rings": []})).is_none());
    }
}
