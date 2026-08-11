//! Map context the GeoLibre plugin sends back — the layers currently on the map,
//! each with its data source. The server reads it to (a) answer `get_map_state`
//! and (b) resolve the ArcGIS **token** for a secured layer WITHOUT ever exposing
//! that token to the model.
//!
//! The plugin writes `<project-dir>/map-context.json`:
//! ```json
//! {
//!   "layers": [
//!     { "id": "roads",   "name": "Roads",   "source": { "type": "arcgis", "url": "https://…/FeatureServer/0", "token": "…" } },
//!     { "id": "parcels", "name": "Parcels", "source": { "type": "geoparquet", "path": "s3://…/parcels.parquet" } }
//!   ],
//!   "view": { … }
//! }
//! ```

use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct MapContext {
    path: PathBuf,
}

/// A layer reference resolved against the map context.
pub struct Resolved {
    /// The actual URL/path to hit (an ArcGIS layer URL or a GeoParquet path/URL).
    pub reference: String,
    /// The ArcGIS token for this layer, if the map context carries one.
    pub token: Option<String>,
}

impl MapContext {
    pub fn new(project_path: &Path) -> Self {
        let path = project_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("map-context.json");
        Self { path }
    }

    fn load(&self) -> Value {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({ "layers": [] }))
    }

    /// The current map context with any tokens redacted — safe to return to the model.
    pub fn redacted(&self) -> Value {
        let mut ctx = self.load();
        if let Some(layers) = ctx.get_mut("layers").and_then(|l| l.as_array_mut()) {
            for l in layers {
                if let Some(src) = l.get_mut("source").and_then(|s| s.as_object_mut()) {
                    if src.contains_key("token") {
                        src.insert("token".into(), Value::String("<redacted>".into()));
                    }
                }
            }
        }
        ctx
    }

    /// Resolve a layer reference the model passed — a layer `id`, or a source
    /// `url`/`path` — to the actual reference plus any token from the context.
    pub fn resolve(&self, layer_ref: &str) -> Resolved {
        let ctx = self.load();
        if let Some(layers) = ctx.get("layers").and_then(|l| l.as_array()) {
            for l in layers {
                let id = l.get("id").and_then(|v| v.as_str());
                let src = l.get("source");
                let url = src.and_then(|s| s.get("url")).and_then(|v| v.as_str());
                let path = src.and_then(|s| s.get("path")).and_then(|v| v.as_str());
                let source_ref = url.or(path);
                if id == Some(layer_ref) || source_ref == Some(layer_ref) {
                    if let Some(sr) = source_ref {
                        let token = src
                            .and_then(|s| s.get("token"))
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        return Resolved { reference: sr.to_string(), token };
                    }
                }
            }
        }
        // Not in the context — use the reference as given, with no token.
        Resolved { reference: layer_ref.to_string(), token: None }
    }
}
