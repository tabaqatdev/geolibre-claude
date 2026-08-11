<div align="center">

# 🗺️ GeoLibre-Claude

**Turn Claude into the AI brain for [GeoLibre](https://geolibre.app)-style geospatial work.**
*A public, MIT-licensed Claude Code plugin — catalog data, spatial SQL, symbology, geoprocessing, and live map control, in 16 languages.*

<sub>حوِّل Claude إلى العقل المُدبِّر لأعمالك الجغرافية المكانية على نمط GeoLibre — إضافة عامة مفتوحة المصدر.</sub>

`MIT` · `Rust + TypeScript` · `stdio (zero-config) or HTTPS + OAuth` · Status: **working — ESRI-compatible `describe_layer` + `query_data` over ArcGIS *and* GeoParquet; layers from the map; HTTPS/OAuth verified**

</div>

---

## What this is

GeoLibre embeds an LLM *inside* the app (Strands Agents SDK) and plugs in a provider. **GeoLibre-Claude flips the topology:** Claude (Desktop / Code) becomes the host and reasoning engine, and GeoLibre's capabilities + your data catalog are exposed to it as **MCP tools** plus a set of **skills**.

**Guiding principle — thin tools, thick skills.** The MCP server holds only small, safe primitives. *All* intelligence — query building, styling, geoprocessing, multilingual handling, workflows — lives in skills, so Claude does the reasoning and the code stays small and auditable.

## What actually gets set up (plain terms)

You install **one plugin into Claude**. That plugin gives Claude four things:

- **commands** — `/geolibre-claude:ask`, `:map`, `:analyze`, `:setup`
- **skills** — the know-how (how to find a layer, write spatial SQL, reproject, style, geoprocess)
- **agents** — helpers that run multi-step work on their own
- **a connector to a small MCP server** — the program that reads the **layers on your map** (each an ArcGIS Feature Service *or* a GeoParquet file) and queries them

```
You, in Claude ──ask──▶  GeoLibre-Claude PLUGIN  ──connector──▶  MCP SERVER  ──▶  your data
                         commands · skills · agents · tools       (small binary)    (ArcGIS + DuckDB)
                                                                                         │ optional
                                                                                         ▼
                                                                     GeoLibre Desktop + bridge → live map
```

**The connector is bundled *inside* the plugin — you don't add it separately.** Installing the plugin
is one step that gives you the commands, skills, agents **and** the tool connection. (A bare MCP
connector on its own would give only the tools, none of the intelligence — that's the Claude Desktop /
tools-only case.)

**Running the setup produces, in order:**

1. **a built server** — the `geolibre-claude` binary;
2. **(default) a background HTTPS service** it runs as, secured with a token you generate (the “Mode A” standard);
3. **the plugin installed in Claude** — bundling the tool connector, so this one step wires up commands + skills + agents + tools;
4. **(optional) a GeoLibre Desktop plugin** so Claude can draw on a live map.

`./install.sh` does 1–3 for you. Full step-by-step: **[docs/SETUP.md](docs/SETUP.md)**.

### How it relates to GeoLibre's own tooling

GeoLibre already ships two AI surfaces; we deliberately **complement** them rather than duplicate:

| Surface | Owner | Job |
|---|---|---|
| Built-in AI Assistant | GeoLibre | In-app assistant (Strands SDK), live control |
| `geolibre-mcp` | GeoLibre | Authors `.geolibre.json` **project files** over stdio |
| **`geolibre-claude`** (this repo) | Tabaqat | **Query the map's layers** (ArcGIS Feature Service **or** GeoParquet) with one ESRI-compatible tool, **geoprocessing**, **skills/agents**, and **live control** via a GeoLibre plugin |

Our server is named `geolibre-claude` precisely so it never clashes with upstream `geolibre-mcp`.

---

## Architecture at a glance

```
Claude Desktop / Claude Code
        │  MCP  ·  stdio (default)  |  HTTPS + OAuth on :8443 (durable)
        ▼
   geolibre-claude   (Rust · rmcp · single binary)
     ├── describe_layer / query_data ─▶ ArcGIS Feature Service (REST)  OR  GeoParquet (DuckDB)
     ├── spatial_sql ────────▶ DuckDB Spatial (SELECT-only) for cross-layer analysis
     ├── GEO tools  ─────────▶ geolibre-cli.wasm via wasmtime (geolibre-rust) — same toolkit as the app
     └── APP tools  ── local ▶ GeoLibre plugin (TypeScript) ─▶ MapLibre map · DuckDB-WASM
```

- **Rust** where GeoLibre uses Rust (the server); geoprocessing **reuses GeoLibre's own WASM toolkit** via wasmtime. **TypeScript** for the in-app plugin, following GeoLibre's plugin conventions — *not everything is Rust.*
- **Data tools work with GeoLibre closed.** App tools need the plugin + a running instance.
- Full design: **[ROADMAP.md](ROADMAP.md)** · visual schematic: **[docs/architecture-schematic.html](docs/architecture-schematic.html)**.
- **[Setup guide (step by step)](docs/SETUP.md)** — build, run, HTTPS/OAuth, background service, plugin, Claude connector.
- More: runnable **[examples](examples/README.md)** · **[HTTPS + OAuth deployment](docs/deployment-http-oauth.md)** · **[honest code/roadmap review](docs/REVIEW.md)** · **[دليل عربي](docs/README.ar.md)**.

---

## Quick start

### Prerequisites

- **Rust** (stable ≥ 1.82) — <https://rustup.rs>
- **Node.js** ≥ 18 (for the GeoLibre plugin) — <https://nodejs.org>
- **mkcert** (only for the HTTPS + OAuth transport) — <https://github.com/FiloSottile/mkcert>
- **GeoLibre desktop** (only for live map control) — <https://geolibre.app>

### Build & run

```bash
git clone https://github.com/tabaqatdev/geolibre-claude
cd geolibre-claude
./install.sh                 # first-time setup: creates .env, generates the token, issues the cert, builds
```

`./install.sh` is the one command to run after cloning — it creates your `.env`, asks for your catalog
URL (or uses the public sample server), generates + saves your auth token, issues the mkcert
certificate, and builds. Run interactively and it will also offer to start the HTTPS service and
register the plugin with Claude. Full walkthrough: **[docs/SETUP.md](docs/SETUP.md)**.

On Windows (PowerShell):

```powershell
Copy-Item .env.example .env
./start.ps1
```

`start.sh` / `start.ps1` build the Rust workspace **and** the plugin, generate locally-trusted TLS certs with mkcert (when `GEOLIBRE_CLAUDE_TRANSPORT=http`), and install the plugin drop-in into `~/.geolibre/plugins/`. `stop.sh` / `stop.ps1` stop any running service and can remove the drop-in with `--purge`.

### Point Claude at it

Stdio (zero-config dev loop):

```bash
claude mcp add geolibre-claude -- "$(pwd)/target/release/geolibre-claude" --transport stdio
```

Or install the whole repo as a Claude Code plugin:

```bash
claude --plugin-dir .
```

---

## Transports

| | **stdio** (default) | **HTTPS + OAuth** |
|---|---|---|
| Setup | zero-config | mkcert cert + a defined port |
| Who spawns it | the MCP client | runs as a daemon |
| Use for | dev loop, single user | durable Claude Desktop at team scale |
| Recovery | `/reload-plugins` | refresh tokens + silent re-auth |

Claude's HTTP connector **requires `https`**, so the durable path needs TLS. `start.sh` runs `mkcert -install` once and issues `certs/localhost.pem` for `https://127.0.0.1:8443` (host/port configurable in `.env`). OAuth settings (issuer, audience, JWKS) live in `.env`; the MCP server acts as an OAuth **Resource Server**.

---

## Languages

GeoLibre-Claude mirrors GeoLibre's **16 UI locales**; any missing key falls back to English, exactly like GeoLibre's react-i18next catalogs:

`en` English · `zh` 中文 · `es` Español · `fr` Français · `de` Deutsch · `pt` Português · `it` Italiano · `nl` Nederlands · `ja` 日本語 · `ko` 한국어 · `ru` Русский · `tr` Türkçe · `id` Indonesia · `hi` हिन्दी · `th` ไทย · `ar` العربية (RTL)

Beyond UI strings, the multilingual work happens at the data boundary: matching a query in any of these languages to the right (often English-coded) catalog layer, and normalizing attribute search (Arabic hamza/alef, tashkeel, ta-marbuta) before it hits `WHERE`/FTS.

---

## Repository layout

```
geolibre-claude/                  ← repo root = Claude Code plugin root (MIT)
├── .claude-plugin/plugin.json    plugin manifest
├── .mcp.json                     launches target/release/geolibre-claude
├── skills/  agents/  commands/   the intelligence (Phase 1+)
├── crates/
│   ├── geolibre-claude/          the MCP server binary (rmcp)
│   └── geolibre-core/            shared contracts + catalog index
├── plugins/
│   └── geolibre-claude-bridge/   GeoLibre plugin (TypeScript/ESM)
├── Cargo.toml                    Rust workspace
├── start.sh · stop.sh            build + run (macOS / Linux / WSL / Git Bash)
├── start.ps1 · stop.ps1          build + run (Windows PowerShell)
├── .env.example                  configuration template
└── docs/  ROADMAP.md  LICENSE
```

---

## Status

**Working and verified live.** The `rmcp` server (stdio + HTTPS/OAuth) exposes **8 tools**:

- **`get_map_state`** — the layers on the map (from the plugin) + their source references.
- **`describe_layer`** — ESRI-shaped schema (fields, sample values, `fieldValueType`), for ArcGIS **and** GeoParquet.
- **`query_data`** — one ESRI-compatible **structured** query (where / spatial filter / statistics / paging), dual-backend: ArcGIS REST **or** DuckDB-over-GeoParquet, verified to return the same shape on both.
- **`spatial_sql`** — SELECT-only DuckDB Spatial for cross-layer analysis (guarded sandbox).
- **`add_layer` · `remove_layer` · `set_style` · `zoom_to`** — drive the live map.

Layers come from the map (an ArcGIS Feature Service **or** a GeoParquet file, local or cloud);
**tokens for secured ArcGIS services are resolved server-side and never exposed to the model.** Ships
with 9 skills, 5 agents, 4 commands; `geolibre-claude apikey` mints the connector token; HTTPS+OAuth,
plugin install, and both data backends are tested. Full history + open items:
**[ROADMAP.md](ROADMAP.md)** / **[REVIEW.md](docs/REVIEW.md)**.

**Known follow-ups:** GeoParquet point/polygon spatial filters (bbox works); the plugin's live
context/apply loop needs verifying in a running GeoLibre; `run_geoprocessing` (Whitebox/WASM) and full
OAuth JWT/JWKS remain.

## License

[MIT](LICENSE) © Tabaqat. Built on and alongside [GeoLibre](https://github.com/opengeos/GeoLibre) and [geolibre-rust](https://github.com/opengeos/geolibre-rust).
