# GeoLibre-Claude — Setup Guide (step by step)

Do the steps in order. Every command here has been run and verified on a machine like yours
(macOS 26, **GeoLibre Desktop 2.5.0**, **Claude Code 2.1.201**). Where a value or environment is
already in place, it's noted **“already set — shown for reference”**; you only change it if you want to.

**What this guide sets up, in order:**

1. **A built server** — the `geolibre-claude` binary that talks to your GIS data (ArcGIS catalog + DuckDB).
2. **A background HTTPS service** — the server running as an auto-restarting service, secured with a token you generate (Mode A, the default).
3. **The plugin installed in Claude** — this one install gives Claude the **commands + skills + agents**, *and* wires up the **tool connector** to the service (the connector is bundled inside the plugin — you don't add it separately).
4. **(Optional) the GeoLibre Desktop plugin** — so Claude can draw on a live map.

The end state: you open Claude, type `/geolibre-claude:ask …`, and it answers from your live catalog.

> **Fastest path — just run `./install.sh`.** It does Steps 1–6 for you (creates `.env`, asks for your
> catalog URL, generates + saves your token, issues the certificate, builds), and when run in a
> terminal it also offers to do Steps 4–6 (start the service, register the plugin). The steps below are
> the same actions spelled out — follow them if you prefer to do it by hand or something needs fixing.

---

## Read this first — the three pieces (this is the confusing part)

Three things are easy to mix up. **The short version: you install the *Claude plugin*; the connector
comes bundled inside it — you don't add a connector separately.**

| Piece | What it is | Gives you | Where |
|---|---|---|---|
| **Claude plugin** — `geolibre-claude` | a Claude **Code** plugin you install | **skills + agents + commands**, *and* it declares the MCP connector | Steps 5–6 |
| **MCP connector** | the tool link the plugin declares in its `.mcp.json` — stdio **or** HTTPS+OAuth | the **tools** (`list_services`, `spatial_sql`, `add_layer`, …) | comes with the plugin (Steps 4–6) |
| **GeoLibre app plugin** — `geolibre-claude-bridge` | a TypeScript plugin you drop into **GeoLibre Desktop** | lets Claude drive a **live map** | Step 8 (optional) |

**Why install a plugin instead of just adding a connector?** A bare connector gives Claude only the
*tools* — none of the guidance (how to write spatial SQL, choose a layer, reproject before measuring,
design symbology), and none of the `/geolibre-claude:*` commands or the agents. That intelligence
lives entirely in **skills/agents/commands**, which come only from the plugin. And the plugin's
bundled `.mcp.json` *is* the connector — so **one install gives you everything**. You'd add a
standalone `claude mcp add` connector only on Claude **Desktop**, which doesn't run Code plugins
(there you get tools, but not the commands/agents).

**The connector can run two ways (the “modes”):**
- **Mode A — HTTPS + OAuth (default).** The server runs as a background service; the plugin connects
  over TLS with your saved token. Durable; also works for Claude Desktop. *(Steps 1–7)*
- **Mode B — stdio (optional).** No service, cert, or token — the plugin launches the server itself.
  Simplest, one machine. *(end of guide)*

---

## Prerequisites — already installed here, shown for reference

| Tool | Check |
|---|---|
| Rust ≥ 1.82 | `rustc --version` |
| Node ≥ 18 | `node --version` |
| mkcert | `mkcert -version` |
| GeoLibre Desktop | already at `/Applications/GeoLibre Desktop.app` (2.5.0) |
| Claude Code | `claude --version` |

Install any that are missing (`brew install mkcert`, `rustup`, `nodejs`).

## Configuration — already set in `.env`, shown for reference

The essentials are already in your `.env`. Change these only if you want to; everything below assumes
the defaults.

| Variable | Default | What it is |
|---|---|---|
| `GEOLIBRE_CATALOG_URL` | *(optional)* | base for *relative* ArcGIS refs; layers normally arrive from the map as full URLs |
| `GEOLIBRE_PARQUET_ROOT` | *(optional)* | local dir GeoParquet files must sit under (enables the GeoParquet backend) |
| `GEOLIBRE_PARQUET_URL_PREFIX` | *(optional)* | cloud prefix (https/s3) GeoParquet URLs must start with |
| `GEOLIBRE_CLAUDE_HTTP_PORT` | `8443` | HTTPS port |
| `GEOLIBRE_CLAUDE_TLS_CERT` / `_KEY` | `certs/localhost.pem` / `-key.pem` | TLS files |
| `GEOLIBRE_CLAUDE_AUTH_TOKEN` | *(you generate in Step 3)* | your login credential |
| `GEOLIBRE_CLAUDE_OAUTH_ISSUER` | `https://127.0.0.1:8443` | advertised for OAuth discovery |

---

# STANDARD SETUP — Mode A (Default: HTTPS + OAuth)

### Step 1 — Run the installer (does Steps 1–6)

On a fresh machine with nothing set up, this one command creates `.env`, sets your catalog URL,
generates + saves your token, issues the certificate, and builds. It's safe to re-run:

```bash
cd /Users/ahmedosman/Apps/geolibre-claude
./install.sh
```

Run in a terminal, it then *asks* whether to also do Steps 4–6 (export the token, start the service,
register the plugin) — answer `y` and you're done; answer `n` (or run it non-interactively) and it
prints the remaining commands. **If you use `./install.sh`, skip to [Step 7](#step-7--use-it).** The
rest of Steps 1–6 below spell out exactly what it does, for doing it by hand or fixing something.

*Manual build only:* `cp .env.example .env` then `./start.sh` (or `cargo build --release`). First
build compiles DuckDB — a few minutes. *Already built here.*

### Step 2 — Generate the TLS certificate

```bash
mkcert -install                                   # trust the local CA once (lets Claude accept the cert)
mkdir -p certs
mkcert -cert-file certs/localhost.pem -key-file certs/localhost-key.pem 127.0.0.1 localhost
```
*Already generated here — the files are in `certs/`.* For a real server, use a CA-signed cert and
point `GEOLIBRE_CLAUDE_TLS_CERT/KEY` at it.

### Step 3 — Create and save your login token

This token is your credential (the server is an OAuth **resource server** — it advertises discovery
per RFC 9728 and requires this bearer token). Mint it with the built-in command and save it to `.env`
in one step:

```bash
./target/release/geolibre-claude apikey --save    # generates + writes GEOLIBRE_CLAUDE_AUTH_TOKEN to .env
# (equivalent by hand: openssl rand -hex 32, then paste into .env)
```

1. In `.env`:
   ```bash
   GEOLIBRE_CLAUDE_AUTH_TOKEN=<paste the token>
   ```
2. In your shell profile, so Claude can read it when it connects (this is what “saves the login”):
   ```bash
   echo 'export GEOLIBRE_CLAUDE_AUTH_TOKEN=<paste the token>' >> ~/.zshrc
   echo 'export GEOLIBRE_CATALOG_URL=<your catalog url>'       >> ~/.zshrc
   source ~/.zshrc
   ```

### Step 4 — Run the server as an auto-restarting background service

Use the provided launchd template — `KeepAlive` restarts it automatically if it ever exits.

```bash
# 1. edit the template: replace the REPLACE_ME paths, paste the same token, set HOME
open deploy/com.tabaqat.geolibre-claude.plist
# 2. install and start it
cp deploy/com.tabaqat.geolibre-claude.plist ~/Library/LaunchAgents/
launchctl load  ~/Library/LaunchAgents/com.tabaqat.geolibre-claude.plist
launchctl start com.tabaqat.geolibre-claude
```

Verify it's up (should print `ok`):
```bash
curl -s https://127.0.0.1:8443/health && echo
```
Logs are in `.run/service.err.log`. To stop it: `launchctl unload ~/Library/LaunchAgents/com.tabaqat.geolibre-claude.plist`.

### Step 5 — Point the plugin at the HTTPS server

Set the plugin's `.mcp.json` to the HTTP form so a single install gives skills/agents/commands **and**
the tools over HTTPS. Write exactly this into `.mcp.json` at the repo root:

```json
{
  "mcpServers": {
    "geolibre-claude": {
      "type": "http",
      "url": "https://127.0.0.1:8443/mcp",
      "headers": { "Authorization": "Bearer ${GEOLIBRE_CLAUDE_AUTH_TOKEN}" }
    }
  }
}
```
The `${GEOLIBRE_CLAUDE_AUTH_TOKEN}` is filled from the environment variable you exported in Step 3.

### Step 6 — Register the plugin with Claude (skills + agents + commands + tools)

First tell Claude where the plugin's marketplace is. **Pick one source:**

```bash
# a) from your local clone (use the ABSOLUTE path, not ".")
claude plugin marketplace add /Users/ahmedosman/Apps/geolibre-claude

# b) from a GitHub repo (owner/repo) — for when it's published
claude plugin marketplace add tabaqat/geolibre-claude

# c) from any git URL
claude plugin marketplace add https://github.com/tabaqat/geolibre-claude
```

Then install it:
```bash
claude plugin install geolibre-claude@geolibre-claude
```

**Open Claude and confirm the connector.** Start Claude Code, then check the connection:
```bash
claude              # opens Claude Code; inside, type /mcp to see connector status
# or from the shell:
claude plugin list  | grep -A2 geolibre    # → Status: ✔ enabled
claude mcp list     | grep geolibre        # → https://127.0.0.1:8443/mcp (HTTP) - ✔ Connected
```

**On Claude Desktop** (instead of the CLI), define the connector in the UI: **Settings → Connectors →
Add custom connector →** URL `https://127.0.0.1:8443/mcp`. With `GEOLIBRE_CLAUDE_OAUTH_ISSUER` set,
Desktop runs the OAuth discovery; otherwise paste the header `Authorization: Bearer <your token>`.
(Skills/agents/commands still come from the plugin install above — Desktop connectors carry only the
tools.)

**Or add the HTTPS server as a standalone connector (tools only, no plugin).** If you *only* want the
tools — no skills/agents/commands — register the connector directly instead of installing the plugin:

```bash
claude mcp add --transport http geolibre-claude https://127.0.0.1:8443/mcp \
  --header "Authorization: Bearer $GEOLIBRE_CLAUDE_AUTH_TOKEN"
claude mcp list | grep geolibre        # → (HTTP) - ✔ Connected
```
This is exactly what your OAuth/HTTPS server exposes as a connector; it just leaves out the
intelligence layer, so the full plugin (above) is recommended.

> **Tip:** run `claude` from *outside* this repo folder for normal use. Inside the repo, Claude also
> sees the project `.mcp.json` and will show a second “pending approval” copy — harmless, but the
> plugin copy is the one that matters.

### Step 7 — Use it

Open Claude Code (anywhere) and try each capability:
```
/geolibre-claude:ask   how many world cities have population over 5 million?
/geolibre-claude:map    add the world cities coloured by population and zoom to them
```
- **Tools/connector:** the ask returns a real table → the connector works.
- **Commands:** the `/geolibre-claude:*` entries exist → commands work.
- **Skills:** ask a plain geo question (no command) — it routes through the skills automatically.
- **Agents:** *“use catalog-scout to find layers about hospitals”* → it delegates to the agent.

---

# Step 8 (optional) — Install the GeoLibre app plugin (`geolibre-claude-bridge`) for live maps

This is the **third** piece from the table up top — a TypeScript plugin that goes **into GeoLibre
Desktop** (not into Claude). Only needed if you want Claude to drive a live GeoLibre map.

```bash
# build + install the bridge plugin into GeoLibre's plugin folder
cd plugins/geolibre-claude-bridge && npm install && npm run build && cd ../..
mkdir -p ~/.geolibre/plugins
ln -sfn "$(pwd)/plugins/geolibre-claude-bridge" ~/.geolibre/plugins/geolibre-claude-bridge

# launch GeoLibre with the runtime env it needs, from a terminal
export GEOLIBRE_CLAUDE_PROJECT_URL="file://$HOME/Apps/geolibre-claude/claude.geolibre.json"   # server → plugin (apply)
export GEOLIBRE_CLAUDE_CONTEXT_URL="file://$HOME/Apps/geolibre-claude/map-context.json"       # plugin → server (map context)
open -a "GeoLibre Desktop"
```

The plugin **reads** the project document (`PROJECT_URL`) to apply Claude's changes, and **writes** the
map context (`CONTEXT_URL`) — the layers on the map with their sources (an ArcGIS Feature Service URL +
token, or a GeoParquet path) — so `describe_layer` / `query_data` know what's on the map. The token for
a secured ArcGIS service travels in that context and is resolved by the server; it's redacted from
`get_map_state`, never exposed to the model. (Both file paths are the live-integration point to verify
under GeoLibre's CSP — see the contract in the `geolibre-project-file` skill.)

In GeoLibre: open the **Claude** toolbar menu → **Bridge panel**; it should say *“Bridge active”*.
Prove the apply path by seeding a map and watching it load:
```bash
cp examples/claude.geolibre.json ./claude.geolibre.json
```
Then drive it from Claude with `/geolibre-claude:map`.

> This is the one step not yet verified end-to-end here (it needs the running app + its CSP). If the
> layer doesn't appear, open GeoLibre's dev console for a CSP/fetch error; seeding the file directly
> isolates it. See [REVIEW.md](REVIEW.md).

---

# OPTIONAL — Mode B (stdio, simplest, no service)

If you don't want the HTTPS service, skip Steps 2–5 entirely. Keep the shipped `.mcp.json` (stdio),
export your catalog URL, and install the plugin — Claude launches the server itself.

```bash
export GEOLIBRE_CATALOG_URL=<your catalog url>
claude plugin marketplace add /Users/ahmedosman/Apps/geolibre-claude
claude plugin install geolibre-claude@geolibre-claude
claude mcp list | grep geolibre        # → (stdio) - ✔ Connected
```
You get the same skills/agents/commands/tools, just spawned locally instead of over HTTPS.

---

## The `start.sh` / `stop.sh` scripts

- **`start.sh`** — builds the Rust workspace + the plugin, generates the mkcert cert (in HTTP mode),
  and installs the GeoLibre bridge drop-in. `start.ps1` is the Windows equivalent.
- **`stop.sh`** — stops a running HTTP daemon; `stop.sh --purge` also removes the bridge drop-in.
- Runtime state (PID, logs) lives in `.run/`.

## Troubleshooting (verified gotchas)

| Symptom | Fix |
|---|---|
| `claude plugin marketplace add` → “Invalid source format” | use an **absolute path** (or `./path`), not `.` |
| connector shows disconnected / 401 | the service isn't running, or the token in `.mcp.json`'s env doesn't match the service's `GEOLIBRE_CLAUDE_AUTH_TOKEN` |
| `${GEOLIBRE_CLAUDE_AUTH_TOKEN}` not filled | you didn't `export` it in the shell that launches Claude (Step 3) — `source ~/.zshrc` |
| a second “pending approval” geolibre-claude | you're running `claude` inside the repo; run it elsewhere, or approve/ignore it |
| cert error when connecting | run `mkcert -install` (it's what makes Claude trust the local cert) |
| catalog tools error | `GEOLIBRE_CATALOG_URL` unset or wrong — try `list_services` first |
| skills/agents/commands missing | you connected a bare MCP connector but didn't **install the plugin** — do Step 6 |
| GeoLibre map layer doesn't appear | see Step 8 note — CSP; seed `claude.geolibre.json` directly to isolate |

## Note on the login

Today the credential is a **bearer token** you generate and save (Steps 3/6) — the server is a proper
OAuth resource server (discovery + `WWW-Authenticate`), and Claude saves and reuses the token. A full
**browser login page** (a built-in authorization server, so Claude does the interactive “log in and
remember” OAuth flow) is the planned upgrade; the token path above is what's built and verified.
