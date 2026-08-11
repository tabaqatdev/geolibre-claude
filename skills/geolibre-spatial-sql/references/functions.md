# DuckDB Spatial — function reference

Load-free reference for the `ST_*` functions available inside `spatial_sql` (the `spatial`
extension is pre-loaded). Read the section you need; you don't need the whole file in context.

Contents: [Constructors](#constructors) · [Accessors](#accessors) · [Measurement](#measurement) ·
[Predicates](#predicates) · [Transformations](#transformations) · [Aggregates](#aggregates) ·
[Output](#output) · [Reading files](#reading-files) · [Gotchas](#gotchas)

## Constructors

| Function | Returns | Notes |
|---|---|---|
| `ST_Point(x, y)` | POINT | x = longitude, y = latitude |
| `ST_GeomFromText(wkt)` | GEOMETRY | parse WKT, e.g. `'POLYGON((...))'` |
| `ST_GeomFromWKB(blob)` | GEOMETRY | parse WKB |
| `ST_GeomFromGeoJSON(json)` | GEOMETRY | parse a GeoJSON geometry |
| `ST_MakeLine(geom, …)` | LINESTRING | build a line from points |
| `ST_MakeEnvelope(xmin, ymin, xmax, ymax)` | POLYGON | axis-aligned box |

## Accessors

| Function | Returns | Notes |
|---|---|---|
| `ST_X(pt)` / `ST_Y(pt)` | DOUBLE | point coordinates |
| `ST_GeometryType(geom)` | VARCHAR | e.g. `'POLYGON'` |
| `ST_IsValid(geom)` | BOOLEAN | topological validity |
| `ST_NPoints(geom)` | INTEGER | vertex count |
| `ST_StartPoint` / `ST_EndPoint` | POINT | line endpoints |
| `ST_Centroid(geom)` | POINT | geometric centroid |
| `ST_Envelope(geom)` | POLYGON | bounding box of one geometry |

## Measurement

Always in the CRS's units — **reproject to a projected CRS first for metres**.

| Function | Returns | Notes |
|---|---|---|
| `ST_Area(geom)` | DOUBLE | m² in a projected CRS; **degrees²** in 4326 |
| `ST_Length(geom)` | DOUBLE | line length |
| `ST_Perimeter(geom)` | DOUBLE | polygon perimeter |
| `ST_Distance(a, b)` | DOUBLE | shortest distance between geometries |
| `ST_DWithin(a, b, d)` | BOOLEAN | true if within distance `d` (CRS units) |

## Predicates

Spatial relationships — use in `JOIN … ON` and `WHERE`. All boolean.

| Function | True when… |
|---|---|
| `ST_Intersects(a, b)` | they share any point |
| `ST_Contains(a, b)` | `a` fully contains `b` |
| `ST_Within(a, b)` | `a` is fully inside `b` |
| `ST_Covers(a, b)` / `ST_CoveredBy(a, b)` | like contains/within, boundary-inclusive |
| `ST_Touches(a, b)` | boundaries meet, interiors don't |
| `ST_Crosses(a, b)` | interiors cross |
| `ST_Overlaps(a, b)` | same-dimension partial overlap |
| `ST_Equals(a, b)` | spatially equal |

## Transformations

Return new geometry. These run in `spatial_sql` (vector geoprocessing is SQL — see
`geolibre-geoprocessing`), whole-layer or not; only raster/terrain is a separate, not-yet-wired path.

| Function | Notes |
|---|---|
| `ST_Transform(geom, from_srid, to_srid)` | reproject, e.g. `ST_Transform(geom, 'EPSG:4326', 'EPSG:32638')`; PROJ is bundled |
| `ST_Buffer(geom, radius)` | radius in CRS units — reproject to metres first |
| `ST_Intersection(a, b)` | geometric AND |
| `ST_Union(a, b)` | geometric OR (see aggregate `ST_Union_Agg` for many) |
| `ST_Difference(a, b)` | `a` minus `b` |
| `ST_ConvexHull(geom)` | tightest enclosing convex polygon |
| `ST_Simplify(geom, tol)` | Douglas–Peucker; `tol` in CRS units |
| `ST_MakeValid(geom)` | repair invalid geometry |
| `ST_FlipCoordinates(geom)` | swap X/Y — the fix when data loads lat/lon reversed |

## Aggregates

| Function | Notes |
|---|---|
| `ST_Union_Agg(geom)` | dissolve a group into one geometry |
| `ST_Collect(geom)` | gather into a GEOMETRYCOLLECTION/multi |
| `ST_Extent(geom)` | bounding box over a set — handy for `zoom_to` |

## Output

| Function | Returns | Use |
|---|---|---|
| `ST_AsText(geom)` | WKT | debugging / display |
| `ST_AsGeoJSON(geom)` | JSON | hand geometry to the map or `add_layer` |
| `ST_AsWKB(geom)` | BLOB | binary interchange |

## Reading files — blocked

File and URL readers — `ST_Read`, `ST_ReadOSM`, `read_csv`/`read_parquet`/`read_json`, `glob`,
`getenv` — are **refused**: the guard denies them, and the engine runs with
`enable_external_access=false`, so even a bare replacement scan (`FROM '/path.parquet'`) is blocked.
**Data enters only via the `attach` mechanism** of the `spatial_sql` tool (the server fetches the
catalog layer and loads it as a table named after your `attach` entry, geometry column `geom`).

## Gotchas

- **Degrees vs metres.** The single most common error. `ST_Area`/`ST_Distance`/`ST_Buffer` in
  EPSG:4326 are in degrees. Reproject first.
- **Axis order.** If points plot in the wrong hemisphere, the source is lat/lon not lon/lat —
  `ST_FlipCoordinates`.
- **Invalid geometry.** Self-intersecting polygons make predicates throw or lie; wrap suspect inputs
  in `ST_MakeValid`.
- **SRID is not carried implicitly.** DuckDB Spatial treats geometries as coordinates without an
  attached SRID; you must know the layer's CRS (from `describe_layer`) and pass it explicitly to
  `ST_Transform`.
- **Read-only.** No `LOAD`/`INSTALL`/`COPY`/`CREATE`. One `SELECT`/`WITH … SELECT` per call.
