# Examples

These use the public ArcGIS sample server so anyone can run them without their own catalog:

```bash
export GEOLIBRE_CATALOG_URL="https://sampleserver6.arcgisonline.com/arcgis/rest/services"
```

Register the server (see the root README), then try the prompts below in Claude Code. The skills do
the routing; you don't call tools by hand.

## 1. Ask a data question — `/geolibre-claude:ask`

> How many world cities have a population over 5 million, and which are the largest three?

What happens: `search_catalog` / `describe_layer` locate `SampleWorldCities/MapServer` layer 0 and
its `POP` field → `count_features` (POP > 5,000,000) → `query_features` sorted by POP. You get a
count (30) and a small table (Buenos Aires, São Paulo, …).

## 2. Analyze with spatial SQL — `/geolibre-claude:analyze`

> What's the straight-line distance from Lima to Buenos Aires?

What happens: `spatial_sql` loads the two cities via `attach`, reprojects, and measures with
`ST_Distance(ST_Transform(...))`. Note the CRS matters — the skill will pick an appropriate
projection rather than measuring in degrees.

## 3. Build a map — `/geolibre-claude:map`

> Add the world cities and colour them graduated by population, then frame the whole world.

What happens: `add_layer` (catalog source + a graduated symbology spec from `geolibre-symbology`) →
`zoom_to` world bounds. The result is written to `claude.geolibre.json` (see
[`claude.geolibre.json`](claude.geolibre.json) for the shape) and applied by the GeoLibre bridge
plugin when the app is open.

## Multilingual

> كم عدد المدن التي يزيد عدد سكانها عن ٥ ملايين؟

The same question in Arabic: `normalize_query` folds the digits/diacritics, `search_catalog` matches,
and the reply comes back in Arabic (right-to-left).
