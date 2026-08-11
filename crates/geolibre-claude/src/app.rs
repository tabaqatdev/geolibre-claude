//! App-bridge state: a GeoLibre-style project document the map tools read and write.
//!
//! The live channel to a running GeoLibre is uncertain under the app's CSP, so the
//! reliable bridge is a file both sides agree on: the app-bridge tools maintain this
//! project document on disk, and the `geolibre-claude-bridge` plugin applies it to
//! the live map (and can write map state back). Deterministic and testable without
//! the app running.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

fn default_version() -> String {
    "0.1".to_string()
}

/// The desired map state. Layers are kept as JSON objects (id, name, source, style,
/// visible) so the shape can grow without churn; `view`/`basemap` are opaque too.
#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basemap: Option<String>,
    #[serde(default)]
    pub layers: Vec<Value>,
}

impl Default for Project {
    fn default() -> Self {
        Project { version: default_version(), view: None, basemap: None, layers: Vec::new() }
    }
}

pub fn load(path: &Path) -> Result<Project> {
    if !path.exists() {
        return Ok(Project::default());
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    // A hand-edited/corrupt file shouldn't brick the tools — start fresh but keep going.
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

pub fn save(path: &Path, project: &Project) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let text = serde_json::to_string_pretty(project).context("serialize project")?;
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn layer_id(layer: &Value) -> Option<&str> {
    layer.get("id").and_then(|v| v.as_str())
}

/// Insert or replace a layer by id (last write wins for the same id).
pub fn upsert_layer(project: &mut Project, layer: Value) {
    let id = layer_id(&layer).map(|s| s.to_string());
    if let Some(id) = id {
        project.layers.retain(|l| layer_id(l) != Some(id.as_str()));
    }
    project.layers.push(layer);
}

/// Remove a layer by id; returns whether anything was removed.
pub fn remove_layer(project: &mut Project, id: &str) -> bool {
    let before = project.layers.len();
    project.layers.retain(|l| layer_id(l) != Some(id));
    project.layers.len() != before
}

/// Set an existing layer's style; returns whether the layer was found.
pub fn set_style(project: &mut Project, id: &str, style: Value) -> bool {
    for l in &mut project.layers {
        if layer_id(l) == Some(id) {
            if let Some(obj) = l.as_object_mut() {
                obj.insert("style".to_string(), style);
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_replaces_same_id() {
        let mut p = Project::default();
        upsert_layer(&mut p, serde_json::json!({"id":"a","name":"first"}));
        upsert_layer(&mut p, serde_json::json!({"id":"a","name":"second"}));
        assert_eq!(p.layers.len(), 1);
        assert_eq!(p.layers[0]["name"], "second");
    }

    #[test]
    fn set_style_and_remove() {
        let mut p = Project::default();
        upsert_layer(&mut p, serde_json::json!({"id":"a","name":"A"}));
        assert!(set_style(&mut p, "a", serde_json::json!({"type":"simple","color":"#f00"})));
        assert_eq!(p.layers[0]["style"]["color"], "#f00");
        assert!(remove_layer(&mut p, "a"));
        assert!(p.layers.is_empty());
    }
}
