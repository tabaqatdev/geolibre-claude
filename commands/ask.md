---
description: Answer a data question from the geospatial catalog.
argument-hint: <your question>
---

Answer from the catalog: **$ARGUMENTS**

Use the `geolibre` skills. Keep it tight: `get_map_state` / `describe_layer` to find and confirm the
right layer and fields, then the smallest tool that answers it — `query_data` (counts, statistics, or
a filtered page) or `spatial_sql` for cross-layer work. Confirm the layer before trusting a fuzzy
match; never invent a field. Reply with the answer (a short table if it's rows) in the user's language.
