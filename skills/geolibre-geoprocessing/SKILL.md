---
name: geolibre-geoprocessing
description: >-
  Run vector geoprocessing — buffer, clip, dissolve, union, intersect, difference, simplify,
  centroid, convex hull, and H3 binning — over catalog layers. Use whenever the user wants to create
  new geometry from existing features: "buffer the schools by 500 m", "clip roads to the district",
  "dissolve parcels by zone", "merge these polygons", "generalize the coastline". Most vector
  operations run through DuckDB Spatial via the `spatial_sql` tool; raster/terrain/hydrology tools
  are a separate path (see the note at the end).
---

# GeoLibre Geoprocessing

Geoprocessing = producing *new* geometry from existing features. Two paths, by complexity:

- **Simple: `query_data`.** A single layer's **buffer** (`buffer_distance` + `buffer_units`) or a
  **spatial filter** (`geometry` + `spatial_rel`, incl. bbox) needs no SQL — and works on **both**
  ArcGIS and GeoParquet layers. Reach for this first.
- **Heavier: `spatial_sql`.** New geometry across features or layers — clip, dissolve, union,
  intersect, simplify, spatial joins, distances between features — is DuckDB Spatial via `spatial_sql`.

For the SQL path the engine you already have — DuckDB Spatial — does the core operations, so it's
mostly about choosing the right function and getting the CRS right. Pairs with **`geolibre-spatial-sql`**
(the dialect, the SELECT-only contract, `attach`); read that for the mechanics.

## The one rule that governs correctness: CRS

Every operation that involves a **distance or an area** — buffer, `ST_DWithin`, area of a clip —
must run in a **projected CRS measured in metres**, not EPSG:4326 (degrees). Reproject with
`ST_Transform(geom, 'EPSG:4326', 'EPSG:<projected>')` before the operation, and transform back to
4326 if you're handing geometry to the map. A "500 m buffer" computed in degrees is silently wrong.

## Operations → SQL

Load the layers with `attach` (geometry column is `geom`), then:

| Operation | Pattern |
|---|---|
| **Buffer** | `ST_Buffer(ST_Transform(geom,'EPSG:4326','EPSG:32638'), 500)` — radius in metres |
| **Clip** (to a mask) | `ST_Intersection(a.geom, b.geom)` with `WHERE ST_Intersects(a.geom, b.geom)` |
| **Dissolve** | `ST_Union_Agg(geom) … GROUP BY <field>` — merge features sharing an attribute |
| **Union** (two sets) | `ST_Union(a.geom, b.geom)` |
| **Intersect** (overlay) | `ST_Intersection(a.geom, b.geom)` |
| **Difference** (erase) | `ST_Difference(a.geom, b.geom)` |
| **Simplify** | `ST_Simplify(geom, tolerance)` — tolerance in CRS units |
| **Centroid / hull** | `ST_Centroid(geom)`, `ST_ConvexHull(geom)` |

Return geometry with `ST_AsGeoJSON(...)` when it's going to the map. Full function signatures are in
the `geolibre-spatial-sql` [function reference](../geolibre-spatial-sql/references/functions.md).

**Example — "buffer the schools by 500 m, keep those within a district":**
```sql
WITH b AS (
  SELECT id, ST_Buffer(ST_Transform(geom,'EPSG:4326','EPSG:32638'), 500) AS buf
  FROM schools
)
SELECT b.id, ST_AsGeoJSON(ST_Transform(b.buf,'EPSG:32638','EPSG:4326')) AS geojson
FROM b JOIN districts d
  ON ST_Intersects(b.buf, ST_Transform(d.geom,'EPSG:4326','EPSG:32638'));
```

## H3 hexagon binning

True H3 binning needs DuckDB's community `h3` extension, which **cannot** be installed in the
read-only sandbox (extension install and community extensions are disabled for safety). So H3 isn't
available. When asked for it, **approximate** with a square grid via `ST_SnapToGrid` in `spatial_sql`
and say plainly that it's a grid approximation, not true H3 — don't route it to a tool that can't run.

## Method

1. Confirm geometry column + CRS with `describe_layer`.
2. Reproject to a metric CRS appropriate to the region before any distance/area op.
3. Do the operation in `spatial_sql`; return `ST_AsGeoJSON` (in 4326) if it's for the map.
4. Sanity-check magnitudes — a buffer area near πr² tells you the units are right.

## When it's *not* a DuckDB job

DuckDB covers vector geometry. **Raster, terrain, and hydrology** — slope/aspect, flow accumulation,
viewshed, cost distance, zonal stats over rasters, the broader Whitebox toolbox — are **not** SQL
operations. Those belong to the `run_geoprocessing` tool, which runs GeoLibre's own `geolibre-rust`
(WASM) toolkit via wasmtime. That tool is **not wired yet** (it needs the upstream `geolibre-cli.wasm`
artifact); until it lands, do the vector parts here in SQL and tell the user plainly that the
raster/terrain step is pending rather than improvising it.
