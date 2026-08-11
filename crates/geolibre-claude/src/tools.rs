//! The MCP tool surface. Layers are identified by a **reference** the GeoLibre
//! plugin passes as map context — an ArcGIS Feature Service layer URL or a
//! GeoParquet path/URL. Each tool does one deterministic thing and returns JSON;
//! the reasoning (which layer, which filter) lives in the skills.

use std::path::PathBuf;
use std::sync::Arc;

use crate::app;
use crate::catalog::{Catalog, QueryDataArgs};
use crate::context::MapContext;
use crate::geoparquet::{self, GeoParquetConfig};
use crate::sql::{ensure_select_only, is_valid_ident, run_query, LoadedTable};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use serde_json::Value;

fn default_where() -> String {
    "1=1".to_string()
}
fn default_out_fields() -> String {
    "*".to_string()
}
fn default_attach_max() -> u32 {
    2000
}
fn default_page_size() -> u32 {
    100
}
fn default_spatial_rel() -> String {
    "esriSpatialRelIntersects".to_string()
}

/// A layer reference from the map: an ArcGIS layer URL (`…/MapServer/0`) or a
/// GeoParquet path/URL. Relative refs resolve against `GEOLIBRE_CATALOG_URL`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LayerRefParams {
    pub layer: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StatSpec {
    /// count | min | max | avg | sum | stddev | var
    pub statistic_type: String,
    /// field to aggregate
    pub field: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryDataParams {
    /// Layer reference: an ArcGIS layer URL (`…/MapServer/0`) or a GeoParquet path/URL.
    pub layer: String,
    /// SQL-92 WHERE clause (`1=1` for all). Use field names from describe_layer.
    #[serde(default = "default_where", rename = "where")]
    pub where_clause: String,
    /// Comma-separated fields, or `*`.
    #[serde(default = "default_out_fields")]
    pub out_fields: String,
    /// Esri geometry JSON for a spatial filter.
    #[serde(default)]
    pub geometry: Option<Value>,
    /// esriGeometryPoint | esriGeometryPolyline | esriGeometryPolygon | esriGeometryEnvelope …
    #[serde(default)]
    pub geometry_type: Option<String>,
    /// Spatial relationship (default esriSpatialRelIntersects).
    #[serde(default = "default_spatial_rel")]
    pub spatial_rel: String,
    /// Buffer the filter geometry before applying it.
    #[serde(default)]
    pub buffer_distance: Option<f64>,
    /// esriSRUnit_Meter | esriSRUnit_Kilometer | esriSRUnit_Foot …
    #[serde(default)]
    pub buffer_units: Option<String>,
    /// Aggregations; when present, returns statistics instead of rows.
    #[serde(default)]
    pub statistics: Vec<StatSpec>,
    /// Field to group statistics by (only with `statistics`).
    #[serde(default)]
    pub group_by: Option<String>,
    /// Sort fields, or stat names as `<statistic_type>_<field>`.
    #[serde(default)]
    pub order_by_fields: Option<String>,
    /// Page size (default 100, max 1000).
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    /// Pagination offset.
    #[serde(default)]
    pub record_offset: u32,
    /// Include geometry in the results (default false).
    #[serde(default)]
    pub return_geometry: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AttachSpec {
    /// Table name to expose in the query (a valid SQL identifier).
    pub table: String,
    /// Layer reference (ArcGIS layer URL or GeoParquet path/URL) to load.
    pub layer: String,
    /// WHERE clause selecting which features to load (`1=1` for all).
    #[serde(default = "default_where", rename = "where")]
    pub where_clause: String,
    /// Max features to load into the table.
    #[serde(default = "default_attach_max")]
    pub max_records: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SpatialSqlParams {
    /// A single SELECT / WITH…SELECT DuckDB Spatial query. Read-only: no DDL/DML,
    /// no PRAGMA/ATTACH/COPY/INSTALL/LOAD, no file/URL readers. Reference the
    /// tables you list in `attach` by name. Wrap geometry with ST_AsGeoJSON /
    /// ST_AsText in the SELECT.
    pub sql: String,
    /// Layers to load as tables before the query runs. The geometry column is `geom`.
    #[serde(default)]
    pub attach: Vec<AttachSpec>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddLayerParams {
    /// Stable layer id (used to update or remove it later).
    pub id: String,
    /// Human-readable layer name.
    pub name: String,
    /// Source layer reference (ArcGIS layer URL or GeoParquet path/URL), OR `geojson`.
    #[serde(default)]
    pub layer: Option<String>,
    /// WHERE clause for a referenced source.
    #[serde(default = "default_where", rename = "where")]
    pub where_clause: String,
    /// Inline GeoJSON FeatureCollection, as an alternative to a layer reference.
    #[serde(default)]
    pub geojson: Option<Value>,
    /// Optional symbology spec (see the geolibre-symbology skill).
    #[serde(default)]
    pub style: Option<Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LayerIdParams {
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetStyleParams {
    pub id: String,
    /// Symbology spec to apply to the layer.
    pub style: Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ZoomToParams {
    /// Bounding box [west, south, east, north] in EPSG:4326.
    #[serde(default)]
    pub bounds: Option<[f64; 4]>,
    /// Center [lon, lat] in EPSG:4326 (with `zoom`), as an alternative to `bounds`.
    #[serde(default)]
    pub center: Option<[f64; 2]>,
    #[serde(default)]
    pub zoom: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EmptyParams {}

#[derive(Clone)]
pub struct GeolibreServer {
    catalog: Catalog,
    /// Allowlist for GeoParquet reads (local root + cloud prefix).
    parquet: GeoParquetConfig,
    /// The layers the GeoLibre plugin reports on the map (+ their sources/tokens).
    context: MapContext,
    /// The project document the app-bridge tools read/write; the GeoLibre plugin applies it.
    project_path: PathBuf,
    /// Serializes the project load→mutate→save cycle so concurrent tool calls
    /// can't lose each other's writes (the file is the shared source of truth).
    project_lock: Arc<std::sync::Mutex<()>>,
}

/// Render a result as pretty JSON text, or a clear error string. Errors are
/// returned as content (not a protocol error) so the model can read and recover.
fn render(result: anyhow::Result<serde_json::Value>) -> String {
    match result {
        Ok(v) => serde_json::to_string_pretty(&v)
            .unwrap_or_else(|e| format!("(could not serialize result: {e})")),
        Err(e) => format!("ERROR: {e:#}"),
    }
}

#[tool_router]
impl GeolibreServer {
    pub fn new(catalog: Catalog, parquet: GeoParquetConfig, project_path: PathBuf) -> Self {
        let context = MapContext::new(&project_path);
        Self {
            catalog,
            parquet,
            context,
            project_path,
            project_lock: Arc::new(std::sync::Mutex::new(())),
        }
    }

    /// Load the project, apply a mutation, save it, and report. Central to the
    /// app-bridge tools so the load→mutate→save cycle stays consistent. Held under
    /// `project_lock` so concurrent tool calls serialize instead of clobbering.
    fn mutate_project<F>(&self, f: F) -> String
    where
        F: FnOnce(&mut app::Project) -> anyhow::Result<Value>,
    {
        let _guard = self.project_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut project = match app::load(&self.project_path) {
            Ok(p) => p,
            Err(e) => return format!("ERROR: reading project: {e:#}"),
        };
        let result = match f(&mut project) {
            Ok(v) => v,
            Err(e) => return format!("ERROR: {e:#}"),
        };
        if let Err(e) = app::save(&self.project_path, &project) {
            return format!("ERROR: saving project: {e:#}");
        }
        render(Ok(serde_json::json!({
            "ok": true,
            "result": result,
            "project_path": self.project_path.display().to_string(),
        })))
    }

    // ── Data tools ───────────────────────────────────────────────────────────

    #[tool(description = "Describe a layer before querying it: fields (name, type, alias, sample \
        values, and a semantic hint), geometry type, extent, and CRS. `layer` is the reference the \
        map gives you (an ArcGIS layer URL or a GeoParquet path/URL). Always call this first — the \
        sample values help you build a correct WHERE clause. Never guess field names.")]
    async fn describe_layer(&self, Parameters(p): Parameters<LayerRefParams>) -> String {
        // Resolve the reference (a layer id or source url/path) against the map
        // context, picking up the ArcGIS token if the plugin sent one.
        let resolved = self.context.resolve(&p.layer);
        let layer_ref = resolved.reference;

        if geoparquet::is_geoparquet_ref(&layer_ref) {
            let cfg = self.parquet.clone();
            let res = tokio::task::spawn_blocking(move || {
                let target = cfg.resolve(&layer_ref)?;
                geoparquet::describe(&target)
            })
            .await;
            return match res {
                Ok(inner) => render(inner),
                Err(e) => format!("ERROR: task failed: {e}"),
            };
        }
        render(self.catalog.describe(&layer_ref, resolved.token.as_deref()).await)
    }

    #[tool(description = "Query a layer with structured, ESRI-compatible parameters (not raw SQL): \
        `where`; a spatial filter via `geometry` + `geometry_type` + `spatial_rel` (+ optional \
        `buffer_distance`/`buffer_units`); `statistics` + `group_by`; `order_by_fields`; \
        `page_size`/`record_offset`; `return_geometry`. Works for both ArcGIS and GeoParquet layers. \
        Returns `resultRecords` (or `statistics`) plus `totalMatchingRecords`. Call describe_layer first.")]
    async fn query_data(&self, Parameters(p): Parameters<QueryDataParams>) -> String {
        let stats: Vec<(String, String)> =
            p.statistics.iter().map(|s| (s.statistic_type.clone(), s.field.clone())).collect();

        let resolved = self.context.resolve(&p.layer);
        let layer_ref = resolved.reference;
        let token = resolved.token;

        if geoparquet::is_geoparquet_ref(&layer_ref) {
            let cfg = self.parquet.clone();
            let res = tokio::task::spawn_blocking(move || {
                let target = cfg.resolve(&layer_ref)?;
                let args = QueryDataArgs {
                    where_clause: &p.where_clause,
                    out_fields: &p.out_fields,
                    geometry: p.geometry.as_ref(),
                    geometry_type: p.geometry_type.as_deref(),
                    spatial_rel: &p.spatial_rel,
                    buffer_distance: p.buffer_distance,
                    buffer_units: p.buffer_units.as_deref(),
                    statistics: &stats,
                    group_by: p.group_by.as_deref(),
                    order_by: p.order_by_fields.as_deref(),
                    page_size: p.page_size,
                    record_offset: p.record_offset,
                    return_geometry: p.return_geometry,
                };
                geoparquet::query(&target, &args)
            })
            .await;
            return match res {
                Ok(inner) => render(inner),
                Err(e) => format!("ERROR: task failed: {e}"),
            };
        }

        let args = QueryDataArgs {
            where_clause: &p.where_clause,
            out_fields: &p.out_fields,
            geometry: p.geometry.as_ref(),
            geometry_type: p.geometry_type.as_deref(),
            spatial_rel: &p.spatial_rel,
            buffer_distance: p.buffer_distance,
            buffer_units: p.buffer_units.as_deref(),
            statistics: &stats,
            group_by: p.group_by.as_deref(),
            order_by: p.order_by_fields.as_deref(),
            page_size: p.page_size,
            record_offset: p.record_offset,
            return_geometry: p.return_geometry,
        };
        render(self.catalog.query_data(&layer_ref, &args, token.as_deref()).await)
    }

    #[tool(description = "Run a SELECT-only DuckDB Spatial query for relational/geometric analysis \
        (joins across layers, distances, areas, within/intersects). Load layers as named tables via \
        `attach` (each `layer` is a map reference), then reference them in `sql`. The geometry column \
        is `geom`; reproject before measuring and wrap geometry with ST_AsGeoJSON in the SELECT.")]
    async fn spatial_sql(&self, Parameters(p): Parameters<SpatialSqlParams>) -> String {
        if let Err(e) = ensure_select_only(&p.sql) {
            return format!("ERROR: query rejected — {e}");
        }

        // Materialize each attached layer to a local GeoJSON file (trusted, server-side).
        let tmp = match tempfile::tempdir() {
            Ok(t) => t,
            Err(e) => return format!("ERROR: could not create temp dir: {e}"),
        };
        let mut tables = Vec::new();
        for a in &p.attach {
            if !is_valid_ident(&a.table) {
                return format!("ERROR: `{}` is not a valid table name (letters, digits, _)", a.table);
            }
            let resolved = self.context.resolve(&a.layer);
            let value = match self
                .catalog
                .query_geojson(&resolved.reference, &a.where_clause, a.max_records, resolved.token.as_deref())
                .await
            {
                Ok(v) => v,
                Err(e) => return format!("ERROR: loading `{}`: {e:#}", a.table),
            };
            let path = tmp.path().join(format!("{}.geojson", a.table));
            if let Err(e) = std::fs::write(&path, value.to_string()) {
                return format!("ERROR: writing `{}`: {e}", a.table);
            }
            tables.push(LoadedTable {
                name: a.table.clone(),
                geojson_path: path.to_string_lossy().into_owned(),
            });
        }

        // DuckDB is blocking; run it off the async runtime with a wall-clock bound.
        let sql = p.sql.clone();
        let handle = tokio::task::spawn_blocking(move || run_query(&sql, &tables, 1000));
        match tokio::time::timeout(std::time::Duration::from_secs(35), handle).await {
            Ok(Ok(inner)) => render(inner),
            Ok(Err(e)) => format!("ERROR: query task failed: {e}"),
            Err(_) => {
                "ERROR: query timed out after 30s — simplify it or narrow the filter/attach".to_string()
            }
        }
    }

    // ── App-bridge tools ─────────────────────────────────────────────────────
    // These maintain the project document at `project_path`; the GeoLibre
    // `geolibre-claude-bridge` plugin applies it to the running map.

    #[tool(description = "Add (or replace, by id) a layer on the GeoLibre map. Source is either a \
        layer reference (`layer` = ArcGIS layer URL or GeoParquet path/URL [+ `where`]) or inline \
        `geojson`. Optionally include a `style` (symbology spec). Applied by the GeoLibre plugin.")]
    async fn add_layer(&self, Parameters(p): Parameters<AddLayerParams>) -> String {
        let source = if let Some(layer) = &p.layer {
            serde_json::json!({ "type": "arcgis", "layer": layer, "where": p.where_clause })
        } else if let Some(gj) = p.geojson {
            serde_json::json!({ "type": "geojson", "data": gj })
        } else {
            return "ERROR: provide a layer reference (`layer`) or inline `geojson`".to_string();
        };

        let mut layer = serde_json::json!({
            "id": p.id, "name": p.name, "source": source, "visible": true
        });
        if let Some(style) = p.style {
            layer["style"] = style;
        }

        self.mutate_project(|proj| {
            app::upsert_layer(proj, layer.clone());
            Ok(serde_json::json!({ "added": p.id, "layers": proj.layers.len() }))
        })
    }

    #[tool(description = "Remove a layer from the GeoLibre map by id.")]
    async fn remove_layer(&self, Parameters(p): Parameters<LayerIdParams>) -> String {
        self.mutate_project(|proj| {
            let removed = app::remove_layer(proj, &p.id);
            Ok(serde_json::json!({ "removed": removed, "id": p.id }))
        })
    }

    #[tool(description = "Set a layer's symbology on the GeoLibre map (see geolibre-symbology for \
        the spec).")]
    async fn set_style(&self, Parameters(p): Parameters<SetStyleParams>) -> String {
        self.mutate_project(|proj| {
            if app::set_style(proj, &p.id, p.style.clone()) {
                Ok(serde_json::json!({ "styled": p.id }))
            } else {
                anyhow::bail!("no layer with id `{}` — add it first", p.id)
            }
        })
    }

    #[tool(description = "Set the GeoLibre map camera — either `bounds` [w,s,e,n] or `center` \
        [lon,lat] + `zoom` (EPSG:4326).")]
    async fn zoom_to(&self, Parameters(p): Parameters<ZoomToParams>) -> String {
        let view = if let Some(b) = p.bounds {
            serde_json::json!({ "bounds": b })
        } else if let Some(c) = p.center {
            serde_json::json!({ "center": c, "zoom": p.zoom.unwrap_or(10.0) })
        } else {
            return "ERROR: provide `bounds` or `center` (+ `zoom`)".to_string();
        };
        self.mutate_project(|proj| {
            proj.view = Some(view.clone());
            Ok(serde_json::json!({ "view": view }))
        })
    }

    #[tool(description = "Read the current GeoLibre map: the layers the plugin reports (with their \
        sources — the references you pass to describe_layer / query_data; tokens redacted) plus any \
        `pending_changes` this server has queued. Start here to see what's on the map.")]
    async fn get_map_state(&self, Parameters(_): Parameters<EmptyParams>) -> String {
        let map = self.context.redacted();
        let _guard = self.project_lock.lock().unwrap_or_else(|e| e.into_inner());
        let pending = app::load(&self.project_path)
            .ok()
            .and_then(|p| serde_json::to_value(&p).ok())
            .unwrap_or(Value::Null);
        render(Ok(serde_json::json!({ "map": map, "pending_changes": pending })))
    }
}

#[tool_handler]
impl ServerHandler for GeolibreServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        // ServerInfo is #[non_exhaustive]; build from default, then set fields.
        let mut info = rmcp::model::ServerInfo::default();
        info.instructions = Some(
            "Geospatial layer access for GeoLibre-Claude. Layers come from the map (get_map_state) \
             as references. describe_layer(layer) for the schema, then query_data(layer, …) with \
             structured params (ArcGIS + GeoParquet), or spatial_sql for cross-layer analysis. \
             add_layer / set_style / zoom_to drive the live map. See the `geolibre` skills."
                .to_string(),
        );
        info.capabilities = rmcp::model::ServerCapabilities::builder().enable_tools().build();
        info.server_info.name = "geolibre-claude".to_string();
        info.server_info.version = env!("CARGO_PKG_VERSION").to_string();
        info
    }
}
