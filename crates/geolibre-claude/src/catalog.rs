//! ArcGIS-REST backend for the data tools.
//!
//! Works with **absolute layer references** — the layer URLs the GeoLibre plugin
//! passes back as map context (e.g. `https://host/arcgis/rest/services/Svc/MapServer/0`).
//! An optional `GEOLIBRE_CATALOG_URL` base still resolves relative refs for convenience.
//! Deliberately dumb: it builds the documented ArcGIS query URLs and returns JSON;
//! all judgment (which layer, which WHERE) lives in the skills.

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Clone)]
pub struct Catalog {
    base: Option<String>,
    http: reqwest::Client,
}

/// Structured, ESRI-compatible query args — backend-agnostic so the same shape
/// drives ArcGIS REST here and DuckDB-over-GeoParquet in the other backend.
pub struct QueryDataArgs<'a> {
    pub where_clause: &'a str,
    pub out_fields: &'a str,
    pub geometry: Option<&'a Value>,
    pub geometry_type: Option<&'a str>,
    pub spatial_rel: &'a str,
    pub buffer_distance: Option<f64>,
    pub buffer_units: Option<&'a str>,
    pub statistics: &'a [(String, String)],
    pub group_by: Option<&'a str>,
    pub order_by: Option<&'a str>,
    pub page_size: u32,
    pub record_offset: u32,
    pub return_geometry: bool,
}

/// Semantic hint for a field (ESRI's `fieldValueType`), inferred from type + name.
pub fn field_value_type(name: &str, type_hint: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let t = type_hint.to_ascii_lowercase();
    if t.contains("date") || t.contains("time") {
        "dateAndTime"
    } else if n.contains("name") || n.contains("title") || n.contains("city") || n.contains("label") {
        "nameOrTitle"
    } else if t.contains("int") || t.contains("double") || t.contains("single")
        || t.contains("float") || t.contains("oid") || t.contains("decimal")
    {
        "countOrAmount"
    } else {
        "text"
    }
}

impl Catalog {
    pub fn new(base: Option<&str>) -> Self {
        let base = base
            .filter(|b| !b.is_empty())
            .map(|b| b.trim_end_matches('/').to_string());
        let http = reqwest::Client::builder()
            .user_agent(concat!("geolibre-claude/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .expect("failed to build HTTP client");
        Self { base, http }
    }

    /// Resolve a layer reference to a full URL: absolute passes through; a relative
    /// ref uses the optional base.
    fn resolve(&self, layer_ref: &str) -> Result<String> {
        let r = layer_ref.trim();
        if r.starts_with("http://") || r.starts_with("https://") {
            Ok(r.trim_end_matches('/').to_string())
        } else if let Some(base) = &self.base {
            Ok(format!("{base}/{}", r.trim_start_matches('/').trim_end_matches('/')))
        } else {
            anyhow::bail!("layer `{r}` is not a full URL and no GEOLIBRE_CATALOG_URL base is set")
        }
    }

    /// GET `url` with `f=json` (unless the caller overrode `f`) and parse JSON.
    async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<Value> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if !query.iter().any(|(k, _)| *k == "f") {
            params.push(("f", "json".to_string()));
        }
        params.extend(query.iter().cloned());

        let resp = self
            .http
            .get(url)
            .query(&params)
            .send()
            .await
            .with_context(|| format!("request to {url} failed"))?
            .error_for_status()
            .with_context(|| format!("{url} returned an error status"))?;

        let value: Value = resp
            .json()
            .await
            .with_context(|| format!("{url} did not return valid JSON"))?;

        // ArcGIS reports logical errors in a 200 body under `error`.
        if let Some(err) = value.get("error") {
            anyhow::bail!("ArcGIS error from {url}: {err}");
        }
        Ok(value)
    }

    /// ESRI-style layer description: fields with type/alias/sample values/semantic
    /// hint, plus geometry type, extent, and spatial reference.
    pub async fn describe(&self, layer_ref: &str, token: Option<&str>) -> Result<Value> {
        let url = self.resolve(layer_ref)?;
        let mut meta_q: Vec<(&str, String)> = Vec::new();
        if let Some(t) = token {
            meta_q.push(("token", t.to_string()));
        }
        let meta = self.get(&url, &meta_q).await?;
        let mut sample_q = vec![
            ("where", "1=1".to_string()),
            ("outFields", "*".to_string()),
            ("resultRecordCount", "5".to_string()),
            ("returnGeometry", "false".to_string()),
        ];
        if let Some(t) = token {
            sample_q.push(("token", t.to_string()));
        }
        let samples = self.get(&format!("{url}/query"), &sample_q).await.ok();
        let sample_feats = samples
            .as_ref()
            .and_then(|s| s.get("features"))
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();

        let fields: Vec<Value> = meta
            .get("fields")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|fld| {
                        let name = fld.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let ftype = fld.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        let alias = fld.get("alias").and_then(|a| a.as_str()).unwrap_or(name);
                        let sample_values: Vec<Value> = sample_feats
                            .iter()
                            .filter_map(|f| f.get("attributes").and_then(|a| a.get(name)).cloned())
                            .filter(|v| !v.is_null())
                            .take(3)
                            .collect();
                        serde_json::json!({
                            "name": name,
                            "type": ftype,
                            "alias": alias,
                            "fieldValueType": field_value_type(name, ftype),
                            "sampleValues": sample_values,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(serde_json::json!({
            "source": "arcgis",
            "name": meta.get("name").cloned().unwrap_or(Value::Null),
            "type": meta.get("type").cloned().unwrap_or(Value::Null),
            "description": meta.get("description").cloned().unwrap_or(Value::Null),
            "geometryType": meta.get("geometryType").cloned().unwrap_or(Value::Null),
            "extent": meta.get("extent").cloned().unwrap_or(Value::Null),
            "spatialReference": meta.get("extent").and_then(|e| e.get("spatialReference")).cloned().unwrap_or(Value::Null),
            "fields": fields,
        }))
    }

    /// ESRI-compatible structured query against an ArcGIS Feature Service layer.
    pub async fn query_data(&self, layer_ref: &str, p: &QueryDataArgs<'_>, token: Option<&str>) -> Result<Value> {
        let url = self.resolve(layer_ref)?;
        let path = format!("{url}/query");
        let mut q: Vec<(&str, String)> =
            vec![("where", p.where_clause.to_string()), ("f", "json".to_string())];
        if let Some(t) = token {
            q.push(("token", t.to_string()));
        }

        let aggregating = !p.statistics.is_empty();
        if aggregating {
            let stats: Vec<Value> = p
                .statistics
                .iter()
                .map(|(kind, field)| {
                    serde_json::json!({
                        "statisticType": kind,
                        "onStatisticField": field,
                        "outStatisticFieldName": format!("{kind}_{field}"),
                    })
                })
                .collect();
            q.push(("outStatistics", serde_json::to_string(&stats)?));
            if let Some(g) = p.group_by {
                q.push(("groupByFieldsForStatistics", g.to_string()));
            }
        } else {
            q.push(("outFields", p.out_fields.to_string()));
            q.push(("resultRecordCount", p.page_size.min(1000).to_string()));
            q.push(("resultOffset", p.record_offset.to_string()));
            q.push(("returnGeometry", p.return_geometry.to_string()));
        }
        if let Some(geom) = p.geometry {
            q.push(("geometry", geom.to_string()));
            q.push(("spatialRel", p.spatial_rel.to_string()));
        }
        if let Some(gt) = p.geometry_type {
            q.push(("geometryType", gt.to_string()));
        }
        if let Some(d) = p.buffer_distance {
            q.push(("distance", d.to_string()));
            if let Some(u) = p.buffer_units {
                q.push(("units", u.to_string()));
            }
        }
        if let Some(o) = p.order_by {
            q.push(("orderByFields", o.to_string()));
        }

        let resp = self.get(&path, &q).await?;
        let features = resp.get("features").and_then(|f| f.as_array()).cloned().unwrap_or_default();

        let mut out = serde_json::json!({
            "source": "arcgis",
            "geometryType": resp.get("geometryType").cloned().unwrap_or(Value::Null),
            "spatialReference": resp.get("spatialReference").cloned().unwrap_or(Value::Null),
        });
        if aggregating {
            out["statistics"] = Value::Array(
                features
                    .iter()
                    .map(|f| f.get("attributes").cloned().unwrap_or_else(|| serde_json::json!({})))
                    .collect(),
            );
        } else {
            out["resultRecords"] = Value::Array(
                features
                    .iter()
                    .map(|f| {
                        let mut r = serde_json::json!({
                            "attributes": f.get("attributes").cloned().unwrap_or_else(|| serde_json::json!({})),
                        });
                        if p.return_geometry {
                            if let Some(g) = f.get("geometry") {
                                r["geometry"] = g.clone();
                            }
                        }
                        r
                    })
                    .collect(),
            );
            let mut count_q = vec![
                ("where", p.where_clause.to_string()),
                ("returnCountOnly", "true".to_string()),
            ];
            if let Some(t) = token {
                count_q.push(("token", t.to_string()));
            }
            if let Ok(c) = self.get(&path, &count_q).await {
                if let Some(n) = c.get("count") {
                    out["totalMatchingRecords"] = n.clone();
                }
            }
        }
        Ok(out)
    }

    /// Fetch a layer as GeoJSON, for loading into DuckDB via `spatial_sql`'s attach.
    pub async fn query_geojson(
        &self,
        layer_ref: &str,
        where_clause: &str,
        max_records: u32,
        token: Option<&str>,
    ) -> Result<Value> {
        let url = self.resolve(layer_ref)?;
        let mut q = vec![
            ("where", where_clause.to_string()),
            ("outFields", "*".to_string()),
            ("resultRecordCount", max_records.to_string()),
            ("returnGeometry", "true".to_string()),
            ("f", "geojson".to_string()),
        ];
        if let Some(t) = token {
            q.push(("token", t.to_string()));
        }
        self.get(&format!("{url}/query"), &q).await
    }
}
