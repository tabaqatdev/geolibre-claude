---
name: geolibre-earth-observation
description: >-
  Guide earth-observation and satellite-imagery workflows — finding and using Sentinel-2, Landsat,
  NAIP and similar via STAC catalogs (e.g. Microsoft Planetary Computer). Use whenever the user asks
  about satellite imagery, remote sensing, a spectral index (NDVI, NDWI), cloud-free scenes over an
  area and date range, or pulling raster tiles/COGs for a place. Explains the STAC search → asset →
  analysis flow and how it connects to the rest of GeoLibre-Claude.
---

# GeoLibre Earth Observation

Earth-observation work follows a consistent shape regardless of sensor: **define where and when →
search a STAC catalog → pick assets (bands) → compute or visualize**. This skill teaches that flow
so you can direct it correctly; note the boundaries below on what's tooled today.

## STAC in one screen

**STAC** (SpatioTemporal Asset Catalog) is the common index for imagery. You query it with:

- a **bounding box** or geometry (the area of interest),
- a **datetime range**,
- a **collection** (e.g. `sentinel-2-l2a`, `landsat-c2-l2`, `naip`),
- optional **filters** (e.g. `eo:cloud_cover < 10`).

It returns **items** (scenes), each exposing **assets** — per-band **COGs** (Cloud-Optimized
GeoTIFFs) you can read windowed over HTTP. The **Microsoft Planetary Computer** is a large free STAC
endpoint (its assets need URL signing before download).

## Common workflows

- **Cloud-free scene over an AOI:** search the collection for the date range, sort by `eo:cloud_cover`,
  take the least-cloudy item, use its band assets.
- **Spectral index:** NDVI = (NIR − Red)/(NIR + Red); NDWI = (Green − NIR)/(Green + NIR). Read the
  two band COGs windowed to the AOI and compute per-pixel.
- **Time series:** same AOI across many dates → one value (e.g. mean NDVI) per scene → a trend.

## How it connects here

- The **AOI usually comes from a map layer** — pick it from `get_map_state`, then get a
  district/parcel geometry via the data tools and use its extent as the STAC bbox. `describe_layer`
  gives you the extent; `query_data` (or `spatial_sql`) can produce a precise geometry or its
  `ST_Extent`.
- **Vector overlays** on imagery (e.g. clip an index to a boundary) are `geolibre-geoprocessing`.
- **Styling** a raster (colour ramp for NDVI) follows the same ideas as `geolibre-symbology`.
- **Displaying** a COG/tile layer on a live map is `geolibre-map-control` (the GeoLibre plugin's
  `addCogLayer` / tile support).

## Boundaries (be honest about tooling)

There is **no dedicated STAC/raster MCP tool yet** in this server — the DATA tools (`describe_layer`,
`query_data`) speak both ArcGIS-REST and GeoParquet, and `spatial_sql` is vector DuckDB. So today this
skill's role is to **plan the workflow correctly** and use what exists (a map layer's geometry for the
AOI, DuckDB for vector steps, the plugin for display).
For the actual STAC search and per-pixel raster math, either drive an external STAC client/notebook,
or wait for the earth-observation tooling on the roadmap. When a request needs that missing piece,
say so and hand back the precise STAC query (collection, bbox, datetime, filters) you would run,
rather than pretending a scene was fetched.
