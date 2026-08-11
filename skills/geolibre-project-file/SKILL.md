---
name: geolibre-project-file
description: >-
  The two files that carry map state between the GeoLibre plugin and the MCP server — the inbound
  map context (what layers are on the map) and the outbound project document (desired changes the
  bridge applies). Use when you need to understand, hand-edit, or debug how map state and layer
  sources (ArcGIS Feature Service + token, or GeoParquet) flow, or why a layer didn't appear.
---

# The plugin ↔ server contract

Two files in the project root carry state, one per direction. Neither depends on a live socket — they
work with the app closed and survive restarts.

## Inbound — `map-context.json` (plugin → server)

The GeoLibre bridge plugin writes the **layers currently on the map**, each with its data source. The
server reads it to answer `get_map_state` and to resolve the ArcGIS token for a secured layer
**without ever exposing the token to the model**.

```json
{
  "layers": [
    { "id": "roads", "name": "Roads",
      "source": { "type": "arcgis", "url": "https://host/arcgis/rest/services/Svc/FeatureServer/0", "token": "…" } },
    { "id": "parcels", "name": "Parcels",
      "source": { "type": "geoparquet", "path": "s3://bucket/parcels.parquet" } }
  ],
  "view": { "bounds": [w, s, e, n] }
}
```

- **`source.type`** is `arcgis` (with `url`, optional `token`) or `geoparquet` (with `path`).
- `get_map_state` returns this with tokens **redacted**. When you call `describe_layer` / `query_data`
  with a layer's `id` **or** its `url`/`path`, the server injects the token itself.
- GeoParquet paths must fall inside the server's allowlist (`GEOLIBRE_PARQUET_ROOT` /
  `GEOLIBRE_PARQUET_URL_PREFIX`) or the read is refused.

## Outbound — `claude.geolibre.json` (server → plugin)

The app-bridge tools (`add_layer`, `set_style`, `zoom_to`, `remove_layer`) write the **desired** map
state here; the plugin applies it to the live map. This is the "response" the bridge handles.

```json
{
  "version": "0.1",
  "view": { "bounds": [w, s, e, n] },
  "basemap": "https://…/style.json",
  "layers": [
    {
      "id": "hospitals", "name": "Hospitals", "visible": true,
      "source": { "type": "arcgis", "layer": "https://…/FeatureServer/1", "where": "1=1" },
      "style": { "type": "graduated", "field": "beds", "classes": [ … ] }
    },
    { "id": "buffer", "name": "500 m buffer",
      "source": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [ … ] } } }
  ]
}
```

**What a layer response can contain, and how the bridge applies it:**

| `source.type` | Fields | Bridge action |
|---|---|---|
| `arcgis` | `layer` (URL), `where?` | fetch the Feature Service as GeoJSON and add it |
| `geoparquet` | `path` | read the parquet (client-side) and add it |
| `geojson` | `data` (FeatureCollection) | add the inline features directly |

Plus `style` (a symbology spec → MapLibre paint), `visible`, `view` (`fitBounds` or center/zoom), and
`basemap`. Reusing an `id` replaces that layer; `remove_layer` deletes it.

## Debugging "my layer didn't show up"

- **A `geoparquet` path outside the allowlist** → the server refused it (`describe_layer`/`query_data`
  return an error).
- **A secured `arcgis` layer with no token in `map-context.json`** → ArcGIS returns an auth error;
  the plugin must include the token in the context it writes.
- **`visible: false`** or a `where` returning zero features → an empty-looking map.
- **The plugin isn't reading the outbound file** → verify the live-load path (see `geolibre-map-control`
  and the setup guide); read back with `get_map_state`.
