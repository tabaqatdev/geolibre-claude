#!/usr/bin/env bash
# GeoLibre-Claude — stop services. Use --purge to also remove the plugin drop-in.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
RUN_DIR="$ROOT/.run"

if [ -t 1 ]; then B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; N=$'\033[0m'; else B=; G=; Y=; N=; fi
info() { printf '%s▸%s %s\n' "$B" "$N" "$*"; }
ok()   { printf '%s✓%s %s\n' "$G" "$N" "$*"; }
warn() { printf '%s!%s %s\n' "$Y" "$N" "$*" >&2; }
expand_tilde() { case "$1" in "~"|"~/"*) printf '%s' "${1/#\~/$HOME}";; *) printf '%s' "$1";; esac; }

PURGE=0
[ "${1:-}" = "--purge" ] && PURGE=1

if [ -f "$ROOT/.env" ]; then set -a; # shellcheck disable=SC1091
  source "$ROOT/.env"; set +a; fi
PLUGINS_DIR="$(expand_tilde "${GEOLIBRE_PLUGINS_DIR:-$HOME/.geolibre/plugins}")"

# ── stop the HTTP daemon, if any ─────────────────────────────────────────────
if [ -f "$RUN_DIR/mcp.pid" ]; then
  PID="$(cat "$RUN_DIR/mcp.pid")"
  if kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
    sleep 1
    kill -9 "$PID" 2>/dev/null || true
    ok "Stopped MCP server (pid $PID)."
  else
    warn "No live process for pid $PID."
  fi
  rm -f "$RUN_DIR/mcp.pid"
else
  info "No HTTP MCP server running (stdio servers are spawned by the client)."
fi

# ── optionally remove the plugin drop-in ─────────────────────────────────────
if [ "$PURGE" = "1" ]; then
  DEST="$PLUGINS_DIR/geolibre-claude-bridge"
  if [ -e "$DEST" ] || [ -L "$DEST" ]; then
    rm -rf "$DEST" && ok "Removed plugin drop-in $DEST"
  else
    info "No plugin drop-in to remove."
  fi
fi

ok "Done."
