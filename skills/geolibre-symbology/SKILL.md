---
name: geolibre-symbology
description: >-
  Design a map symbology spec from a data distribution — graduated (choropleth) or categorized
  styling, class breaks, and a colour ramp. Use whenever the user wants to colour, shade, classify,
  or style a layer by a field ("colour districts by population", "show schools graduated by
  capacity", "style roads by class"), or asks how a layer should look on the map. Produces a
  structured spec the map (or the GeoLibre plugin) can apply. Pairs with the global `dataviz` skill
  for colour choices.
---

# GeoLibre Symbology

Turning a field into a map style is two decisions — **how to break the data into classes** and
**which colours to assign** — grounded in the data's actual distribution, not a guess. If the global
**`dataviz`** skill is available, use it for the colour theory (sequential/diverging/qualitative
ramps, contrast, colour-blind safety); this skill covers the *spatial* side and the output contract.

## First, look at the distribution

You cannot classify well without seeing the numbers. Get them with `query_data` (statistics: min/max/avg) or a
`spatial_sql` query (percentiles, histogram) **before** choosing breaks. A field ranging 0–10 with
one value at 9,000 needs a different scheme than an evenly spread one.

## Choose the classification

| Method | Use when | Note |
|---|---|---|
| **Quantile** | skewed data; you want equal-sized groups | each class has ~the same count; break values look irregular |
| **Equal interval** | evenly spread data; intuitive legend | outliers leave classes empty |
| **Natural breaks (Jenks)** | clustered data; minimise within-class variance | the default "just make it look right" |
| **Manual** | meaningful thresholds exist (e.g. capacity 250/500/1000) | prefer this when the domain has real cut points |
| **Standard deviation** | showing distance from the mean | pairs with a diverging ramp |

Use **3–7 classes** — fewer loses signal, more is unreadable. For **categorized** styling (discrete
values like road class or land use), one colour per distinct value, not ranges.

## Choose the ramp

- **Sequential** (light→dark of one hue) for low→high magnitude — population, capacity, density.
- **Diverging** (two hues around a neutral midpoint) for above/below a reference — change, anomaly,
  standard deviations.
- **Qualitative** (distinct hues, similar lightness) for categories with no order.

Keep it colour-blind safe (avoid red/green as the only distinction) and give polygons a thin,
low-opacity stroke so boundaries read without dominating.

## Output: the symbology spec

Emit this structure — it's the contract the map / GeoLibre plugin applies (Phase 4 `set_style`):

```json
{
  "type": "graduated",
  "field": "capacity",
  "method": "quantile",
  "ramp": "sequential",
  "classes": [
    { "min": 0,   "max": 200,  "color": "#fee5d9", "label": "0–200" },
    { "min": 200, "max": 500,  "color": "#fcae91", "label": "200–500" },
    { "min": 500, "max": 1000, "color": "#fb6a4a", "label": "500–1,000" },
    { "min": 1000,"max": null, "color": "#cb181d", "label": "1,000+" }
  ],
  "opacity": 0.85,
  "stroke": { "color": "#00000033", "width": 0.5 }
}
```

For **categorized**, replace each class's `min`/`max` with `"value": "<category>"`. For a single
uniform style, `"type": "simple"` with one `color`. Always include real break values from the data
and human-readable `label`s in the user's language and number format.

## Working method

1. Pull the distribution (`query_data` statistics / `spatial_sql` percentiles).
2. Pick method + class count to match the distribution's shape (skewed → quantile/Jenks).
3. Pick a ramp that matches the data's meaning (magnitude → sequential; divergence → diverging).
4. Compute the breaks and assign colours; write the spec with real values and clear labels.
5. If the map is live, hand the spec to `set_style` (see `geolibre-map-control`); otherwise return
   it for the user to apply.
