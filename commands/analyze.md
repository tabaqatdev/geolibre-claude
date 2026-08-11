---
description: Run a geospatial analysis over the catalog and report the result.
argument-hint: <the analysis question>
---

Analyze: **$ARGUMENTS**

Use the `geolibre` skills. Lead with data, not the map:

1. **Locate + confirm** the layer(s) and fields (`get_map_state`, `describe_layer`; `catalog-scout`
   for heavier discovery).
2. **Size it** (a `query_data` count / statistics) before pulling geometry.
3. **Compute** the answer with `spatial_sql` when it's relational or geometric — joins, distance,
   area, within/intersects (`geolibre-spatial-sql`; delegate to `spatial-sql-writer`). Reproject
   before measuring; push work into SQL.
4. **Present** a clear table or direct answer, note the CRS/units and any filter you applied, and
   offer to put the result on the map (`/geolibre-claude:map`) if that would help.

Reply in the user's language (RTL for Arabic).
