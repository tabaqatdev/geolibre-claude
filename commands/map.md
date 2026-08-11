---
description: Build or update the GeoLibre map from a natural-language request.
argument-hint: <what to show on the map>
---

Put this on the GeoLibre map: **$ARGUMENTS**

Use the `geolibre` skills. Work through it end to end:

1. **Find the data** — `get_map_state` / `describe_layer` to locate and confirm the right layer(s)
   and fields (delegate to `catalog-scout` if it's involved).
2. **Prepare geometry** if needed — `spatial_sql` for any buffer/clip/dissolve/filter
   (`geolibre-geoprocessing`); reproject before measuring.
3. **Design the style** from the data distribution (`geolibre-symbology` or the `symbology-designer`
   agent) if the request implies colouring/classifying.
4. **Apply it** with the app-bridge tools (`geolibre-map-control`): `add_layer` (with a stable id and
   the style), then `zoom_to` the extent.
5. **Report** what you added and, if GeoLibre isn't open, that it's prepared in the project document
   and will appear when the app opens with the bridge plugin.

Reply in the user's language. Confirm before removing or wholesale-restyling existing layers.
