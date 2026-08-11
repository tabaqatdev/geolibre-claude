---
name: geolibre-catalog
description: >-
  Find the right layer on the GeoLibre map and read its schema before querying or styling. Use
  whenever a task refers to data you haven't pinned yet — "the schools layer", "which layer has
  population?", "roads in the north" — or when you need a layer's fields, geometry, extent, or CRS.
  Covers both ArcGIS Feature Service and GeoParquet layers through one `describe_layer` tool. This is
  step one of almost every geospatial task.
---

# GeoLibre Catalog — layers from the map

There is no separate catalog to crawl. **The layers are whatever is on the map**, reported by the
GeoLibre plugin. Your job is to pick the right one and learn its schema before you build a query — a
wrong layer or a mis-typed field name produces confident, empty, wrong answers.

## 1. See what's on the map

Call **`get_map_state`**. It returns the current layers, each with:
- an **`id`** and **`name`**,
- a **`source`** — `arcgis` (a Feature Service layer URL) or `geoparquet` (a path/URL). Tokens are
  redacted; the server applies them for you.

Pick the layer that matches the request by name/theme. You pass its **`id`** *or* its source
**reference** to the next tools — either works.

## 2. Describe it

Call **`describe_layer`** with that reference. It returns, the same way for both formats:
- **fields**: `name`, `type`, `alias`, a `fieldValueType` **semantic hint** (`nameOrTitle`,
  `countOrAmount`, `dateAndTime`), and **`sampleValues`** — real values from the data,
- **geometry type**, **extent**, and **CRS** (ArcGIS) / geometry column (GeoParquet).

The sample values are the point: they show you the real spelling, casing, and range, so you can write
a WHERE clause that actually matches. **Never guess field names** — read them here.

## Working method

1. `get_map_state` → choose the layer by name/theme.
2. `describe_layer` → confirm it's the right geometry and read exact field names + sample values.
3. Hand off: **`geolibre-spatial-sql`** to query it (`query_data`), or **`geolibre-map-control`** to
   restyle/frame it.

## Judgment

- **Prefer aliases when talking to the user**, machine names when writing a query — tell them "the
  *School Capacity* field", write `where capacity > 500`.
- **Don't invent fields.** If a field the user implies isn't in `describe_layer`'s output, say so and
  offer the closest real fields (the sample values help you spot the right one).
- **If several layers on the map could match, ask or show** the candidates (name + geometry) rather
  than silently picking one.
- **Adding a *new* layer that isn't on the map yet?** That's `add_layer` (`geolibre-map-control`) — you
  supply the source reference (an ArcGIS layer URL or a GeoParquet path); then describe/query it.
