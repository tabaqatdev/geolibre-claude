#!/usr/bin/env bash
# GeoLibre-Claude — build all code, prepare TLS/plugin, and start services.
# Works on macOS, Linux, WSL, and Git Bash on Windows.
# Windows-native users: use ./start.ps1 instead.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
RUN_DIR="$ROOT/.run"
mkdir -p "$RUN_DIR"

# ── pretty logging ───────────────────────────────────────────────────────────
if [ -t 1 ]; then B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; R=$'\033[31m'; N=$'\033[0m'; else B=; G=; Y=; R=; N=; fi
info()  { printf '%s▸%s %s\n' "$B" "$N" "$*"; }
ok()    { printf '%s✓%s %s\n' "$G" "$N" "$*"; }
warn()  { printf '%s!%s %s\n' "$Y" "$N" "$*" >&2; }
die()   { printf '%s✗%s %s\n' "$R" "$N" "$*" >&2; exit 1; }
have()  { command -v "$1" >/dev/null 2>&1; }
expand_tilde() { case "$1" in "~"|"~/"*) printf '%s' "${1/#\~/$HOME}";; *) printf '%s' "$1";; esac; }

# ── config ───────────────────────────────────────────────────────────────────
if [ ! -f "$ROOT/.env" ]; then
  warn ".env not found — creating it from .env.example (set GEOLIBRE_CATALOG_URL)."
  cp "$ROOT/.env.example" "$ROOT/.env"
fi
set -a; # shellcheck disable=SC1091
source "$ROOT/.env"; set +a

TRANSPORT="${GEOLIBRE_CLAUDE_TRANSPORT:-stdio}"
HTTP_HOST="${GEOLIBRE_CLAUDE_HTTP_HOST:-127.0.0.1}"
HTTP_PORT="${GEOLIBRE_CLAUDE_HTTP_PORT:-8443}"
TLS_CERT="${GEOLIBRE_CLAUDE_TLS_CERT:-certs/localhost.pem}"
TLS_KEY="${GEOLIBRE_CLAUDE_TLS_KEY:-certs/localhost-key.pem}"
PLUGINS_DIR="$(expand_tilde "${GEOLIBRE_PLUGINS_DIR:-$HOME/.geolibre/plugins}")"
BIN="$ROOT/target/release/geolibre-claude"

info "GeoLibre-Claude — transport=$TRANSPORT"

# ── 1. build Rust workspace ──────────────────────────────────────────────────
have cargo || die "cargo not found. Install Rust from https://rustup.rs"
info "Building Rust workspace (cargo build --release)…"
cargo build --release
ok "Built $BIN"

# ── 2. build the GeoLibre plugin (TypeScript) ────────────────────────────────
PLUGIN_SRC="$ROOT/plugins/geolibre-claude-bridge"
if have npm && [ -f "$PLUGIN_SRC/package.json" ]; then
  info "Building GeoLibre plugin…"
  ( cd "$PLUGIN_SRC" && { [ -d node_modules ] || npm install --silent; } && npm run --silent build ) \
    && ok "Built plugin → $PLUGIN_SRC/dist" \
    || warn "Plugin build failed — skipping (live map control is optional)."
else
  warn "npm not found or no plugin package.json — skipping plugin build."
fi

# ── 3. TLS certs (http transport only) ───────────────────────────────────────
if [ "$TRANSPORT" = "http" ]; then
  if have mkcert; then
    mkdir -p "$ROOT/certs"
    if [ ! -f "$ROOT/$TLS_CERT" ] || [ ! -f "$ROOT/$TLS_KEY" ]; then
      info "Issuing locally-trusted TLS cert with mkcert…"
      mkcert -install >/dev/null 2>&1 || warn "mkcert -install needed elevated rights; continuing."
      ( cd "$ROOT" && mkcert -cert-file "$TLS_CERT" -key-file "$TLS_KEY" "$HTTP_HOST" localhost 127.0.0.1 ::1 >/dev/null 2>&1 )
      ok "Cert ready: $TLS_CERT"
    else
      ok "TLS cert already present."
    fi
  else
    warn "mkcert not found — HTTPS needs it. Install: https://github.com/FiloSottile/mkcert"
  fi
fi

# ── 4. install the plugin drop-in so GeoLibre discovers it ───────────────────
if [ -f "$PLUGIN_SRC/plugin.json" ]; then
  mkdir -p "$PLUGINS_DIR"
  DEST="$PLUGINS_DIR/geolibre-claude-bridge"
  if ln -sfn "$PLUGIN_SRC" "$DEST" 2>/dev/null; then
    ok "Linked plugin into $DEST"
  else
    cp -R "$PLUGIN_SRC" "$DEST" && ok "Copied plugin into $DEST"
  fi
fi

# ── 5. start service ─────────────────────────────────────────────────────────
if [ "$TRANSPORT" = "http" ]; then
  info "Starting HTTPS MCP server on https://$HTTP_HOST:$HTTP_PORT …"
  nohup "$BIN" --transport http >"$RUN_DIR/mcp.log" 2>&1 &
  echo $! > "$RUN_DIR/mcp.pid"
  sleep 1
  if kill -0 "$(cat "$RUN_DIR/mcp.pid")" 2>/dev/null; then
    ok "Server running (pid $(cat "$RUN_DIR/mcp.pid")). Logs: .run/mcp.log"
  else
    rm -f "$RUN_DIR/mcp.pid"
    warn "Server exited immediately — check TLS certs and the bearer token. See .run/mcp.log:"
    tail -n 5 "$RUN_DIR/mcp.log" >&2 || true
  fi
else
  ok "stdio transport — the MCP client spawns the binary. Register it with:"
  printf '\n    claude mcp add geolibre-claude -- "%s" --transport stdio\n\n' "$BIN"
fi

ok "Done."
