---
name: geolibre-map-control
description: >-
  Drive the live GeoLibre map — add, remove, style, and frame layers on a running app. Use whenever
  the user wants something to happen on their map: "add the schools layer", "put hospitals on the map
  and colour them by capacity", "zoom to the northern region", "remove that layer", "what's on the
  map right now?". These actions change what the user sees, so confirm before destructive ones. If
  GeoLibre isn't running, the changes still land in the project document and apply when it opens.
---

# GeoLibre Map Control

This is the one place you *act on the world*: the app-bridge tools change what the user sees on their
map. Everything else in GeoLibre-Claude is read-only; here, be deliberate.

The tools maintain a **project document** (`claude.geolibre.json`) that the `geolibre-claude-bridge`
plugin applies to the live MapLibre map. So a change works whether or not GeoLibre is open right now —
it's recorded, and applied when the plugin next reads it. See `geolibre-project-file` for the schema.

## The tools

| Tool | Does |
|---|---|
| `add_layer` | Add/replace a layer by `id`. Source = a catalog layer (`service` + `layer` [+ `where`]) **or** inline `geojson`. Optional `style` (a symbology spec). |
| `set_style` | Apply a symbology spec to an existing layer. |
| `zoom_to` | Set the camera — `bounds` [w,s,e,n], or `center` [lon,lat] + `zoom`. |
| `remove_layer` | Remove a layer by `id`. |
| `get_map_state` | Read the current project document (layers, view, basemap). |

## How to work

1. **Get the data first.** Find and confirm the layer with `get_map_state` + `describe_layer` before
   adding it. Prefer a **source reference** (`layer` = an ArcGIS layer URL or a GeoParquet path) so the
   plugin fetches live data; use inline `geojson` for a computed result (e.g. a `spatial_sql` buffer).
2. **Give layers stable ids.** The `id` is how you restyle or remove a layer later. Reuse the same id
   to update; `add_layer` replaces a layer with the same id rather than duplicating it.
3. **Style from the data.** Design the spec with `geolibre-symbology` (or the `symbology-designer`
   agent), then pass it as `add_layer`'s `style` or via `set_style`. The spec is the same contract the
   symbology skill defines.
4. **Frame the result.** After adding, `zoom_to` the layer's extent (get it from `describe_layer` or a
   `spatial_sql` `ST_Extent`) so the user sees what changed.
5. **Confirm before destructive actions.** Adding a layer is low-stakes. `remove_layer` and a
   wholesale restyle change what the user has — say what you're about to do first, then do it.

## Example

"Add hospitals and colour them by bed capacity, then zoom to them":
1. `get_map_state` → find the hospitals layer → confirm with `describe_layer` (find the capacity field).
2. `symbology-designer` (or `geolibre-symbology`) → a graduated spec on the capacity field.
3. `add_layer { id: "hospitals", name: "Hospitals", service, layer, style: <spec> }`.
4. `zoom_to { bounds: <layer extent> }`.

If GeoLibre isn't open, tell the user the map is prepared and will appear when they open the app with
the bridge plugin active.
