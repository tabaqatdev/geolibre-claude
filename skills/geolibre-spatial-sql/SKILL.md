---
name: geolibre-spatial-sql
description: >-
  Query GeoLibre map layers — filtering, statistics, spatial filters, and cross-layer analysis. Use
  `query_data` (structured, ESRI-compatible) for a single layer's attribute/spatial/statistics query
  on either format (ArcGIS or GeoParquet), and `spatial_sql` (SELECT-only DuckDB Spatial) when you
  must relate multiple layers — joins, distances, areas, within/intersects. Reach here the moment a
  geospatial question needs more than reading a field. Pairs with the global `duckdb` skill.
---

# Querying layers

Two tools, different jobs. Reach for the smaller one first.

## `query_data` — the everyday query (both formats)

Structured, ESRI-compatible **parameters** (not raw SQL), against **one** layer reference. Works the
same over an ArcGIS Feature Service and a GeoParquet file:

| Param | Purpose |
|---|---|
| `where` | SQL-92 attribute filter (`1=1` for all) — use field names from `describe_layer` |
| `geometry` + `geometry_type` + `spatial_rel` | spatial filter (`esriGeometryEnvelope` bbox works on both formats; other geometry types are ArcGIS-side) |
| `buffer_distance` + `buffer_units` | buffer the filter geometry first |
| `statistics` (`[{statistic_type, field}]`) + `group_by` | aggregate (count/min/max/avg/sum/stddev/var) instead of rows |
| `order_by_fields`, `page_size` (≤1000), `record_offset` | sort + paginate |
| `return_geometry` | include geometry in the results (default false) |

Returns `resultRecords` (or `statistics`) plus `totalMatchingRecords`. **Always `describe_layer`
first** for the exact field names and sample values. Prefer `query_data` for anything single-layer —
it's cheaper and format-agnostic.

**Example** — cities over 5M, largest first:
```
query_data(layer=<ref>, where="POP>5000000", out_fields="CITY_NAME,POP",
           order_by_fields="POP DESC", page_size=10)
```

## `spatial_sql` — cross-layer DuckDB Spatial

When the question spans **multiple layers** or needs geometry math `query_data` can't express (joins,
distance between features, dissolve), use `spatial_sql`: one **SELECT-only** DuckDB Spatial query.

- **Load layers with `attach`** — each entry is `{ table, layer: <reference>, where?, max_records? }`.
  The server fetches the layer and exposes it as a table; the geometry column is **`geom`**. `attach`
  caps at `max_records` (**default 2000**) — raise it or tighten `where` for a bigger layer.
- **Read-only:** only `SELECT` / `WITH … SELECT`; no DDL/DML, no `PRAGMA`/`ATTACH`/`COPY`/`INSTALL`/
  `LOAD`, no file/URL readers — the engine blocks them. Output caps at 1000 rows (`truncated: true`).
- This is where the **`duckdb`** global skill helps for base SQL.

**Example** — schools within 2 km of a hospital (two layers):
```sql
SELECT s.name FROM schools s JOIN hospitals h
  ON ST_DWithin(ST_Transform(s.geom,'EPSG:4326','EPSG:32638'),
                ST_Transform(h.geom,'EPSG:4326','EPSG:32638'), 2000);
```

## The rule that governs correctness: CRS

Any **distance or area** must be computed in a **projected CRS measured in metres**, not EPSG:4326
(degrees). In `spatial_sql`, `ST_Transform(geom,'EPSG:4326','EPSG:<projected>')` before measuring, and
transform back to 4326 for the map. A "500 m" computed in degrees is silently wrong.

## Working method

1. `describe_layer` for field names + sample values (+ CRS).
2. Single layer? → `query_data`. Multiple layers / geometry math? → `spatial_sql`.
3. Reproject before any metric; push filtering/aggregation into the query.
4. Return `ST_AsGeoJSON(...)` (in 4326) when the result goes to the map.

The `ST_*` function catalog is in [references/functions.md](references/functions.md) — read it when
you need a function you're unsure of.
