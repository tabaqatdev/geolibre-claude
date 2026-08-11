# GeoLibre-Claude — Roadmap

> A public, MIT-licensed Claude Code plugin that lets **Claude Desktop / Claude Code act as the AI brain** for [GeoLibre](https://geolibre.app)-style geospatial work — multilingual (all 16 GeoLibre locales), skill-driven, with zero changes to GeoLibre core.

Status: **planning** · Owner: Tabaqat (tabaqat.net) · Last updated: 2026-08-10

---

## 1. Vision

GeoLibre's built-in AI Assistant embeds an LLM *inside* the app (Strands Agents SDK) and plugs in a provider (Gemini/Claude/OpenAI/…). **We flip the topology:** Claude (Desktop/Code) becomes the host and reasoning engine, and GeoLibre's capabilities + your data catalog are exposed to it as **MCP tools** plus a rich set of **skills**.

The release is a **single public GitHub repo that *is* a Claude Code plugin**. A user clones it, runs one build, and gets:
- **Skills** that teach Claude every component of GeoLibre (catalog, spatial SQL, symbology, geoprocessing, map control, project files, earth observation, i18n across 16 locales).
- **Agents** that orchestrate multi-step geospatial work.
- **An MCP server** (thin, safe primitives) for data access + live app control.
- **A GeoLibre plugin** that bridges Claude to the running map.

**Guiding principle — thin tools, thick skills.** The MCP server holds only dumb, safe primitives. *All* intelligence — query building, styling, geoprocessing, multilingual handling, workflows — lives in skills. This maximizes Claude's own reasoning, keeps the code small and auditable, and makes the whole thing portable.

---

## 2. Locked decisions

| Decision | Choice | Rationale |
|---|---|---|
| Topology | Claude is host; our MCP server + skills hold the tools | Offloads LLM cost to the Claude subscription; no GeoLibre core changes |
| Scope | Chat-with-data **and** control the live GeoLibre app | Full parity with the built-in assistant |
| Existing MCP server | Build fresh; use the connected `8cf4a5ff` server as reference only | Clean, endpoint-agnostic, open-source-friendly |
| **Language & stack** | **Rust where GeoLibre is Rust** (server + geoprocessing); **TypeScript** for the in-app plugin — *not everything is Rust* | Cargo workspace; unified with GeoLibre's own toolchain and conventions |
| **MCP framework** | Official **Rust MCP SDK (`rmcp`)**; stdio default, Streamable-HTTP + `auth` for the durable path | Native Rust; no Node runtime to ship |
| **Geoprocessing** | Reuse **`geolibre-rust`** by running its `geolibre-cli.wasm` via **wasmtime** (it's WASM/WASI-only — *not* a native crate) | Same 1000+ tools as the app, bit-identical; no reimplementation |
| **Data engine** | **DuckDB via `duckdb-rs`** (native) + spatial extension | Same engine as GeoLibre's DuckDB-WASM; runs with the app closed |
| **In-app plugin** | Standard **GeoLibre TS/ESM plugin** (`plugin.json` + `GeoLibrePlugin` API, from the upstream template) | Follows GeoLibre's plugin conventions; loads like any other plugin — the correct, non-Rust surface |
| **Coexistence** | Binary **`geolibre-claude`** *complements* upstream **`geolibre-mcp`** (which authors `.geolibre.json`) — never clashes | We own catalog data + geoprocessing + live control; they own project-file authoring |
| **HTTPS + OAuth** | **mkcert**-issued local cert on a **defined port** (`:8443`, configurable); MCP = OAuth Resource Server | Claude's HTTP connector requires `https`; mkcert makes it locally trusted with no prompts |
| **Languages** | All **16 GeoLibre locales** (en, zh, es, fr, de, pt, it, nl, ja, ko, ru, tr, id, hi, th, ar); missing keys fall back to `en` | Parity with GeoLibre's react-i18next catalogs |
| Distribution | Public GitHub repo that is a Claude Code plugin (+ marketplace manifest) | `git clone` → `./start.sh` → install |
| License | MIT | Open release |
| Endpoint | **Generic** — configured via `GEOLIBRE_CATALOG_URL`; not hardwired to Tabaqat | Reusable by anyone |
| Intelligence split | **Max skills**; MCP server = primitives only | "Stretch Claude to the max" |
| Default transport | **stdio** (zero-config); HTTPS + OAuth is a documented opt-in | Frictionless clone-and-go |
| RAG | **No document-RAG.** Lightweight index over layer/field *metadata* only | NL→query needs tool-calling, not RAG |
| Catalog index | **BM25 + multilingual alias** default; **embeddings optional** | Keeps "just build" true; scales to large multilingual catalogs |

---

## 3. Architecture

### 3.1 Runtime topology (default, local)

```
Claude Desktop / Claude Code
        │  MCP · stdio (default, /reload-plugins to recover)  |  HTTPS + OAuth on :8443 (durable)
        ▼
   geolibre-claude  (Rust · rmcp · single binary)
     ├── DATA tools ───────────────▶ ArcGIS-REST catalog + DuckDB (duckdb-rs, SELECT-only) + catalog index
     ├── GEO tools  ───────────────▶ geolibre-cli.wasm via wasmtime (geolibre-rust) — same toolkit as the app
     └── APP tools  ── localhost ──▶ GeoLibre plugin (TypeScript/ESM) ─▶ MapLibre map · DuckDB-WASM · layers
```

- **Data tools work even when GeoLibre isn't open.** App tools require the plugin + a running GeoLibre instance.
- **Fallback control channel:** if the plugin API can't hold a live socket, Claude edits a `.geolibre.json` project file and the plugin hot-reloads it.

### 3.2 Division of labor

| Layer | Holds | Examples |
|---|---|---|
| **MCP tools** (thin) | Deterministic, safe primitives | `list_services`, `describe_layer`, `query_features`, `spatial_sql` (SELECT-only), `statistics`, `search_catalog`, `add_layer`, `set_style`, `zoom_to` |
| **Skills** (thick) | All reasoning + GeoLibre knowledge | how to choose a layer, write DuckDB spatial SQL, design a color ramp, chain geoprocessing, normalize Arabic, run a workflow |
| **Agents** | Orchestration + context isolation | `geospatial-analyst`, `catalog-scout`, `spatial-sql-writer`, `symbology-designer`, `language-liaison` |
| **Commands** | User entry points | `/geolibre-claude:map`, `:ask`, `:analyze`, `:setup` |

### 3.3 Repo layout

```
geolibre-claude/                       ← repo root = plugin root (MIT)
├── .claude-plugin/     plugin.json   (marketplace.json added in Phase 5)
├── .mcp.json           launches ${CLAUDE_PLUGIN_ROOT}/target/release/geolibre-claude
├── skills/             geolibre · geolibre-catalog · geolibre-spatial-sql · geolibre-symbology
│                       geolibre-geoprocessing · geolibre-map-control · geolibre-project-file
│                       geolibre-earth-observation · geolibre-i18n
├── agents/             geospatial-analyst · catalog-scout · spatial-sql-writer
│                       symbology-designer · language-liaison
├── commands/           map · ask · analyze · setup
├── crates/
│   ├── geolibre-claude/  (rmcp server binary: data · geo · app-bridge tools)
│   └── geolibre-core/    (shared tool contracts + catalog index)
├── plugins/
│   └── geolibre-claude-bridge/  GeoLibre plugin (TypeScript/ESM, from the template)
├── Cargo.toml          [workspace] members = crates/*
├── start.sh · stop.sh          build + run  (macOS / Linux / WSL / Git Bash)
├── start.ps1 · stop.ps1        build + run  (Windows PowerShell)
├── .env.example        config (catalog, transport, TLS port, locales)
└── examples/ · docs/(localized) · LICENSE · README.md
```

---

## 4. Component inventory

### 4.1 Skills (the intelligence)

| Skill | Teaches Claude… |
|---|---|
| `geolibre` | Router/overview: what the system is, when to reach for each sub-skill |
| `geolibre-catalog` | Service/layer/field discovery; folder taxonomy; multilingual `search_catalog` usage |
| `geolibre-spatial-sql` | DuckDB Spatial dialect, SELECT-only rules, spatial functions, query patterns (pairs with the global `duckdb` skill) |
| `geolibre-symbology` | Graduated & categorized ramps, class breaks, color choices (pairs with the global `dataviz` skill) |
| `geolibre-geoprocessing` | buffer / clip / dissolve / union / intersection / spatial-join / simplify / H3 — when and how |
| `geolibre-map-control` | Driving the live app: add/remove/order layers, camera, popups, basemaps |
| `geolibre-project-file` | `.geolibre.json` schema; generate/edit projects; the fallback control channel |
| `geolibre-earth-observation` | STAC / Microsoft Planetary Computer; Sentinel-2 / Landsat / NAIP workflows |
| `geolibre-i18n` | All 16 GeoLibre locales: term→layer glossaries, reply in the user's language; **Arabic** gets extra normalization (hamza/alef آأإ→ا, tashkeel, ta-marbuta ة/ه) + RTL |

### 4.2 Agents

- **`geospatial-analyst`** — orchestrator: discover → query → analyze → visualize.
- **`catalog-scout`** — read-only multilingual catalog search; isolates the large catalog context from the main thread.
- **`spatial-sql-writer`** — writes + self-validates DuckDB spatial SQL; tight toolset.
- **`symbology-designer`** — designs map styles from data distributions.
- **`language-liaison`** — interprets queries in any of the 16 locales, applies per-language normalization (Arabic especially), maps terms to layers.

### 4.3 MCP tools (primitives only)

Data: `list_services`, `list_layers`, `describe_layer`, `query_features`, `statistics`, `count_features`, `spatial_sql` (SELECT-only, guarded), `search_catalog` (locale-aware), `normalize_query` (per-locale, Arabic-aware).
App-bridge: `add_layer`, `remove_layer`, `set_style`, `zoom_to`, `run_geoprocessing`, `get_map_state`.

### 4.4 Buildable units

- **`crates/geolibre-claude`** (Rust) — the MCP server (`rmcp`); stdio by default, Streamable-HTTP + OAuth optional. Hosts DATA tools (`duckdb-rs`), GEO tools (`geolibre-rust`), and app-bridge tools. Named to avoid clashing with upstream `geolibre-mcp`.
- **`crates/geolibre-core`** (Rust) — shared tool-contract types (one source of truth for server ↔ plugin) + the catalog index builder.
- **`plugins/geolibre-claude-bridge`** (TypeScript/ESM) — a standard GeoLibre plugin (`plugin.json` + `GeoLibrePlugin`), scaffolded from the [upstream template](https://github.com/opengeos/geolibre-plugin-template). Uses `GeoLibreAppAPI` (`getMap`, `addGeoJsonLayer`, `registerRightPanel`, `getProjectState`…) to drive the live map. Installed as a drop-in under `~/.geolibre/plugins/`.
- **Reused, not rebuilt:** `geolibre-rust` is **WASM/WASI-only** (crates `geolibre-cli`, `geolibre-tools`, `geolibre-wasm`, `geolibre-pmtiles`), so we run its `geolibre-cli.wasm` through **wasmtime** rather than linking it as a native crate — geoprocessing stays bit-identical to the app.
- **Build & run:** `start.sh` / `start.ps1` build all of the above, issue mkcert TLS (for the HTTPS path), and install the plugin drop-in; `stop.sh` / `stop.ps1` tear down.

---

## 5. Cross-cutting stances

**Multilingual (16 locales).** We mirror GeoLibre's full locale set — en, zh, es, fr, de, pt, it, nl, ja, ko, ru, tr, id, hi, th, ar — with English fallback, like its react-i18next catalogs. Claude handles conversation/reasoning natively; the work is at the data boundary — (1) multilingual catalog matching (a query in any locale → the often English-coded layer) via the catalog index + per-language glossaries, (2) locale-aware attribute search (normalize before `WHERE`/FTS; **Arabic** needs the most: hamza/alef, tashkeel, ta-marbuta), (3) localized tool/skill descriptions so tool selection is language-agnostic and replies respect the user's language + direction (RTL for Arabic).

**HTTPS, OAuth / reconnect.** Default stdio path needs no OAuth; a dropped plugin MCP server recovers with **`/reload-plugins`** (no full app restart). The durable **Claude Desktop** path is **Streamable-HTTP + OAuth**, and Claude's HTTP connector **requires `https`** — so `start.sh` provisions a locally-trusted **mkcert** cert on a **defined port** (`https://127.0.0.1:8443`, configurable) with the MCP server as OAuth **Resource Server** behind an IdP (Keycloak/Auth0/Cognito/Tabaqat SSO), with refresh tokens + silent re-auth so restarts don't re-prompt. Note: some token-persistence failures are an Anthropic-side bug (regressed ~Claude Code 2.1.117–2.1.118, Apr 2026) and can't be fully fixed server-side. Use **Claude Code `/mcp`** for the dev loop.

**Safety.** `spatial_sql` is SELECT-only and guarded. Catalog endpoint is user-configured. No secrets in the repo; `.env.example` documents required config.

---

## 6. Milestones

Each milestone is independently useful and shippable.

### Phase 0 — Foundations & verification (2–3 days)
- ✅ **Confirmed** the GeoLibre plugin API: TS/ESM plugins, `plugin.json` manifest, `GeoLibrePlugin` interface, rich `GeoLibreAppAPI` (`getMap`, `addGeoJsonLayer`, `registerRightPanel`, `getProjectState`…), drop-ins under `~/.geolibre/plugins/`. Remaining Phase-4 question: can the in-app (browser-context) plugin reach a `localhost` MCP endpoint under GeoLibre's CSP, or does control go through `.geolibre.json`? **Critical path.**
- ✅ **Scaffold built & green:** Cargo workspace (`geolibre-claude` + `geolibre-core`), TS plugin (`geolibre-claude-bridge`), `.mcp.json`, `.env.example`, cross-platform `start.sh`/`stop.sh` + `.ps1`, README.
- **Size the catalog** (layers/fields) → confirm BM25-alias vs embeddings.
- **Acceptance:** `cargo build --release` produces `geolibre-claude`; `claude --plugin-dir .` loads the plugin; namespaced skills appear in `/help`. *(scaffold done; skills land Phase 1.)*

### Phase 1 — Data MCP + core skills (1–2 weeks)
- ✅ `crates/geolibre-claude` ArcGIS-REST data tools (stdio, `rmcp` 3.1.2): `list_services`, `list_layers`, `describe_layer`, `query_features` (GeoJSON), `count_features`, `statistics`. Verified end-to-end against a live server through a full MCP handshake.
- ✅ `spatial_sql` (DuckDB Spatial via `duckdb-rs` bundled): SELECT-only guard (rejects DDL/DML, file readers, multi-statement), server-side `attach` of catalog layers as DuckDB tables, `to_json` row output. Verified: aggregate + `ST_Transform`/`ST_Distance` over attached live-catalog data; guard rejects `read_csv`/`DROP`/`;`.
- ✅ Skills: `geolibre`, `geolibre-catalog`, `geolibre-spatial-sql`.
- ✅ Agents `catalog-scout` (read-only catalog locator; context isolation) and `spatial-sql-writer` (writes + self-validates DuckDB Spatial SQL).
- **Acceptance:** ✅ ask a data question in English → correct answer + table from the live catalog. **First usable release (v0.1) COMPLETE — 7 data tools, 3 core skills, 2 agents.**

### Phase 2 — Multilingual + semantic catalog (1 week)
- ✅ BM25 catalog index + locale-aware normalization in `geolibre-core` (pure, unit-tested); tools `search_catalog` (lazy concurrent crawl, cached) and `normalize_query`. Verified: indexed 369 live layers, ranked EN queries correctly; Arabic normalization folds alef/hamza/tashkeel/ta-marbuta/digits; light Arabic stemming (strip ال) in the index only.
- ✅ Skill `geolibre-i18n` (16 locales; Arabic depth; RTL); agent `language-liaison`.
- ⏳ Localized tool/skill descriptions; optional embeddings + curated term→layer glossary for monolingual-catalog cross-language matching.
- **Acceptance:** ✅ core met — Arabic query normalizes and matches; `search_catalog` ranks live layers. **v0.2** (glossary/embeddings are optional hardening).

### Phase 3 — Visualization skills (1 week)
- ✅ Skills: `geolibre-symbology` (classification + ramp + a `set_style` spec contract), `geolibre-geoprocessing`, `geolibre-earth-observation`; agent `symbology-designer`.
- ✅ **Vector geoprocessing runs through the existing `spatial_sql` tool** (DuckDB Spatial: buffer/clip/dissolve/union/intersect/difference/simplify/centroid/hull). Verified live: 50 km buffer → correct πr² area; `ST_Union_Agg` dissolve.
- ⏳ `run_geoprocessing` for **raster/terrain/hydrology** (Whitebox via `geolibre-rust` WASM + wasmtime) — deferred: needs the upstream `geolibre-cli.wasm` artifact; not shipped unverified. H3 binning needs the DuckDB `h3` community extension.
- **Acceptance:** ✅ "buffer the schools by 500 m and color by capacity" → correct buffer SQL + a symbology spec. **v0.3** (raster geoprocessing pending).

### Phase 4 — Live app control (1–2 weeks)
- ✅ App-bridge MCP tools (`add_layer`, `remove_layer`, `set_style`, `zoom_to`, `get_map_state`) maintaining a `claude.geolibre.json` project document (load→mutate→save, serialized under a lock). Verified server-side: add/style/zoom/read/remove round-trips correctly.
- ✅ `plugins/geolibre-claude-bridge` (GeoLibre TS/ESM plugin): `applyProject()` turns the document into `GeoLibreAppAPI` calls (catalog fetch, GeoJSON layers, symbology→paint, camera). Typechecks + bundles.
- ✅ Skills `geolibre-map-control`, `geolibre-project-file`; commands `map`, `analyze`, `ask`, `setup`.
- ⏳ **Needs live GeoLibre test:** the plugin's file-load loop (reads `GEOLIBRE_CLAUDE_PROJECT_URL`) depends on the app's CSP/runtime — the flagged open risk. Apply logic is built; the load path is the thing to verify in a running app.
- **Acceptance:** ✅ server prepares a styled layer + camera in the project document (verified); ⏳ end-to-end onto a running map pending a hands-on GeoLibre run. **v0.4.**

### Phase 5 — Release hardening (ongoing)
- ✅ `.claude-plugin/marketplace.json` (schema-correct) + **`claude plugin validate` passes** (caught and fixed a `plugin.json` author-shape error).
- ✅ **HTTPS transport implemented + tested** (`--transport http`, rmcp `StreamableHttpService` + axum + rustls TLS). Verified end-to-end over mkcert TLS: `/health` open, RFC 9728 discovery, bearer-token gate (401 without / 200 with), MCP handshake succeeds. `docs/SETUP.md` is the step-by-step guide.
- ✅ **Query-builder hardening** (research-grounded — DuckDB "Securing DuckDB" + MCP security best-practices): full DuckDB lockdown after the trusted attach-load (`enable_external_access`, extension install/autoload/community off, `disabled_filesystems`, `max_expression_depth`, `lock_configuration` last), connection bounded (`max_memory=1GB`, `threads=2`), a real `interrupt_handle` timeout, and a SQL length cap. The **subquery-wrap is the read-only guarantee** (parser rejects non-SELECT); the denylist is a pre-filter. Replacement-scan file read verified blocked; normal ops unaffected. Guard unit tests + catalog HTTP timeouts/bounded redirects.
- ✅ **Skills/agents quality pass** (independent review): fixed a critical phantom-`run_geoprocessing` bug in the router (would hard-fail "buffer the schools"), made vector-vs-raster consistent everywhere, documented the truncation caps, added read-only `tools:` allowlists to the four read-only agents, and added the agent-delegation map to the router. Details in `docs/REVIEW.md`.
- ✅ Runnable `examples/`; `docs/deployment-http-oauth.md`; localized `docs/README.ar.md`; `docs/SETUP.md`; `docs/REVIEW.md` (honest code + roadmap review).
- ⏳ Remaining: full **OAuth JWT/JWKS validation** (today the resource server enforces a bearer token + advertises the issuer via RFC 9728; interactive IdP flow is documented); `run_geoprocessing` (WASM/wasmtime); verify the plugin live-load path in a running GeoLibre.
- **Acceptance:** a fresh clone reaches a working v0.1 in <10 min following `docs/SETUP.md`. **v1.0** (data + query + analyze + map-authoring + HTTPS all verified; three capabilities remain, each flagged).

---

## 7. Open questions / risks

1. **Live-control channel (highest remaining risk).** The GeoLibre plugin runs in the app's browser context; whether it can reach a `localhost` MCP endpoint under GeoLibre's CSP/CORS is unconfirmed. Mitigation: `.geolibre.json` project-file channel via `getProjectState`/`applyProjectState`. *Resolve in Phase 4.*
2. **Catalog scale → index choice.** BM25-alias vs embeddings. *Resolve in Phase 0 sizing.*
3. **Desktop reconnect durability.** Partly an Anthropic-side bug; the HTTPS + OAuth path mitigates but doesn't fully fix. Track Claude version.
4. **`spatial_sql` safety surface.** SELECT-only guard must be robust (no CTE/`pragma`/attach escapes).
5. **Multilingual coverage.** 16 locales' term→layer glossaries need curation; Arabic normalization (dialect, transliteration) is the deepest.
6. **Don't duplicate upstream.** Keep scope on catalog/geo/live-control; leave `.geolibre.json` *authoring* to upstream `geolibre-mcp` (we only *edit* it as a fallback).

---

## 8. Distribution & licensing

- **License:** MIT (`LICENSE` at root).
- **Install (dev):** `git clone && cp .env.example .env && ./start.sh` (macOS/Linux/WSL) or `./start.ps1` (Windows). Stop with `./stop.sh` / `./stop.ps1`.
- **Install (users):** `/plugin marketplace add tabaqat/geolibre-claude` → `/plugin install`.
- **Config:** `cp .env.example .env`, set `GEOLIBRE_CATALOG_URL` (+ optional transport/TLS/embeddings).
- **GeoLibre plugin:** `start.sh` builds `plugins/geolibre-claude-bridge` and installs it into `~/.geolibre/plugins/` for live map control (optional).

---

_Phase 0 scaffold is built and green (Rust workspace, TS plugin, cross-platform scripts, README). Next: run Phase 1 (data tools + core skills), and size the catalog._
