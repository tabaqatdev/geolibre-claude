---
description: Check and guide GeoLibre-Claude configuration.
---

Help the user get GeoLibre-Claude working. Check the essentials and guide them through anything
missing — don't change settings for them, explain what to set.

1. **Data source** — layers come from the map (`get_map_state`). `GEOLIBRE_CATALOG_URL` is an optional
   base for relative ArcGIS refs; `GEOLIBRE_PARQUET_ROOT` / `GEOLIBRE_PARQUET_URL_PREFIX` enable
   GeoParquet. Confirm at least one path to data is configured, or that the plugin sends map context.
2. **Transport** — `stdio` (default, zero-config) vs `http` (durable; needs mkcert TLS on the
   configured port + OAuth). Confirm which they want and that `start.sh` / `start.ps1` ran.
3. **Live map (optional)** — for map control, is the `geolibre-claude-bridge` plugin installed into
   `~/.geolibre/plugins/` and is GeoLibre running? Note the plugin needs `GEOLIBRE_CATALOG_URL` and
   `GEOLIBRE_CLAUDE_PROJECT_URL` in its runtime env (the live-integration step to verify).
4. **Language** — confirm the default locale; all 16 GeoLibre locales are supported with English
   fallback.

Report what's ready and what's missing, with the exact next step for each gap. Reply in the user's
language.
