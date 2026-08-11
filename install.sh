#!/usr/bin/env bash
# GeoLibre-Claude — first-time setup.
#
# Run this once after cloning:   ./install.sh
# It is idempotent (safe to re-run) and does the tedious first-time parts:
#   1. checks prerequisites
#   2. creates .env from the template
#   3. asks for your catalog URL (or uses the public sample server)
#   4. generates + saves your bearer token
#   5. issues a locally-trusted TLS certificate (mkcert)
#   6. builds the server
# Then it OFFERS (only when run interactively) to finish Mode A:
#   7. export the token in ~/.zshrc, 8. install the launchd service, 9. register the plugin.
# Windows users: see docs/SETUP.md (the .ps1 scripts + manual steps).
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

if [ -t 1 ]; then B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; R=$'\033[31m'; N=$'\033[0m'; else B=; G=; Y=; R=; N=; fi
info(){ printf '%s▸%s %s\n' "$B" "$N" "$*"; }
ok(){   printf '%s✓%s %s\n' "$G" "$N" "$*"; }
warn(){ printf '%s!%s %s\n' "$Y" "$N" "$*" >&2; }
die(){  printf '%s✗%s %s\n' "$R" "$N" "$*" >&2; exit 1; }
have(){ command -v "$1" >/dev/null 2>&1; }
interactive(){ [ -t 0 ]; }
ask(){ local p="$1" d="${2:-}" a=""; interactive && read -r -p "$p " a || a=""; printf '%s' "${a:-$d}"; }
confirm(){ interactive || return 1; local a=""; read -r -p "$1 [y/N] " a || a=""; case "$a" in y|Y|yes|YES) return 0;; *) return 1;; esac; }
get_env(){ grep -E "^$1=" .env 2>/dev/null | head -1 | cut -d= -f2-; }
set_env(){ # set_env KEY VALUE — update in place or append
  local k="$1" v="$2" tmp; tmp="$(mktemp)"
  awk -v k="$k" -v v="$v" 'BEGIN{d=0} $0 ~ "^"k"=" {print k"="v; d=1; next} {print} END{if(!d) print k"="v}' .env > "$tmp" && mv "$tmp" .env
}

echo; echo "${B}GeoLibre-Claude — first-time setup${N}"; echo

# 1 ─ prerequisites
info "Checking prerequisites…"
have cargo   || die "Rust/cargo not found. Install: https://rustup.rs"
have openssl || die "openssl not found (needed to generate the token)."
have mkcert  || warn "mkcert not found — needed for HTTPS (Mode A). Install: brew install mkcert"
have npm     || warn "npm not found — needed only for the GeoLibre live-map plugin."
ok "Prerequisites ok."

# 2 ─ .env
if [ -f .env ]; then ok ".env exists — keeping it (values updated below)."; else cp .env.example .env && ok "Created .env from .env.example."; fi

# 3 ─ catalog URL
cur="$(get_env GEOLIBRE_CATALOG_URL)"
sample="https://sampleserver6.arcgisonline.com/arcgis/rest/services"
def="$cur"; { [ -z "$def" ] || printf '%s' "$def" | grep -q "example.com"; } && def="$sample"
if interactive; then
  info "Optional base ArcGIS-REST URL (layers normally come from the map as full URLs; GeoParquet"
  info "roots are set in .env). Leave as the sample server to try it out."
  url="$(ask "  URL [Enter = $def]:" "$def")"
  set_env GEOLIBRE_CATALOG_URL "$url"; ok "Catalog: $url"
elif [ "$cur" != "$def" ]; then
  set_env GEOLIBRE_CATALOG_URL "$def"; ok "Catalog: $def (sample — edit .env to use yours)"
else ok "Catalog already set: $cur"; fi

# 4 ─ token
tok="$(get_env GEOLIBRE_CLAUDE_AUTH_TOKEN)"
if [ -z "$tok" ]; then tok="$(openssl rand -hex 32)"; set_env GEOLIBRE_CLAUDE_AUTH_TOKEN "$tok"; ok "Generated + saved a bearer token."
else ok "Auth token already set."; fi

# 5 ─ certificate
if have mkcert; then
  mkdir -p certs
  if [ -f certs/localhost.pem ] && [ -f certs/localhost-key.pem ]; then ok "TLS cert already present."
  else
    info "Issuing a locally-trusted TLS cert…"
    mkcert -install >/dev/null 2>&1 || warn "mkcert -install may need elevated rights; continuing."
    mkcert -cert-file certs/localhost.pem -key-file certs/localhost-key.pem 127.0.0.1 localhost >/dev/null 2>&1 \
      && ok "Cert ready (certs/localhost.pem)." || warn "mkcert failed — generate the cert manually (docs/SETUP.md Step 2)."
  fi
fi

# 6 ─ build
info "Building the server (first build compiles DuckDB — a few minutes)…"
if cargo build --release >/dev/null 2>&1; then ok "Built target/release/geolibre-claude"
else die "Build failed — run 'cargo build --release' to see the error."; fi

cat_url="$(get_env GEOLIBRE_CATALOG_URL)"
echo; ok "Prepared. Your settings live in ${B}.env${N} (git-ignored)."

# ── Optional finishing steps (interactive only) ──────────────────────────────
if ! interactive; then
  echo
  echo "${B}Next (Mode A — HTTPS + OAuth), run these — or re-run ./install.sh in a terminal to do them for you:${N}"
  echo "  export GEOLIBRE_CLAUDE_AUTH_TOKEN=$tok"
  echo "  export GEOLIBRE_CATALOG_URL=$cat_url"
  echo "  # then: docs/SETUP.md Steps 4–6 (service + plugin)."
  exit 0
fi

echo
# Finish Mode A in one coherent action — the three parts depend on each other
# (the HTTPS connector needs the token persisted in your shell), so they're not
# offered separately. Everything here is idempotent.
if have mkcert && have claude && confirm "Finish Mode A now — start the HTTPS service + register the plugin with Claude?"; then

  # a) persist the exports so Claude has the token in every new shell (no duplicates on re-run)
  if grep -q "GEOLIBRE_CLAUDE_AUTH_TOKEN=" "$HOME/.zshrc" 2>/dev/null; then
    ok "~/.zshrc already exports the token (leaving it)."
  else
    { echo "export GEOLIBRE_CLAUDE_AUTH_TOKEN=$tok"; echo "export GEOLIBRE_CATALOG_URL=$cat_url"; } >> "$HOME/.zshrc"
    ok "Added token + catalog exports to ~/.zshrc."
  fi
  export GEOLIBRE_CLAUDE_AUTH_TOKEN="$tok" GEOLIBRE_CATALOG_URL="$cat_url"

  # b) HTTPS background service (auto-restart via launchd; re-runs cleanly)
  plist="$HOME/Library/LaunchAgents/com.tabaqat.geolibre-claude.plist"
  sed -e "s|REPLACE_ME/geolibre-claude|$ROOT|g" \
      -e "s|REPLACE_ME_HOME|$HOME|g" \
      -e "s|REPLACE_ME_WITH_openssl_rand_hex_32|$tok|g" \
      -e "s|https://your-host/arcgis/rest/services|$cat_url|g" \
      deploy/com.tabaqat.geolibre-claude.plist > "$plist"
  mkdir -p .run
  launchctl unload "$plist" 2>/dev/null || true
  launchctl load "$plist" 2>/dev/null && launchctl start com.tabaqat.geolibre-claude 2>/dev/null && sleep 2
  if curl -sk https://127.0.0.1:8443/health 2>/dev/null | grep -q ok; then ok "Service up: https://127.0.0.1:8443/mcp"
  else warn "Service didn't answer /health yet — check .run/service.err.log"; fi

  # c) point the plugin at the HTTPS server, then register it with Claude
  cat > .mcp.json <<JSON
{
  "mcpServers": {
    "geolibre-claude": {
      "type": "http",
      "url": "https://127.0.0.1:8443/mcp",
      "headers": { "Authorization": "Bearer \${GEOLIBRE_CLAUDE_AUTH_TOKEN}" }
    }
  }
}
JSON
  claude plugin marketplace add "$ROOT" >/dev/null 2>&1 || claude plugin marketplace update geolibre-claude >/dev/null 2>&1 || true
  if claude plugin install geolibre-claude@geolibre-claude >/dev/null 2>&1; then ok "Plugin registered (skills + agents + commands + HTTPS tools)."
  else warn "Plugin install reported an issue — run 'claude plugin list' to check."; fi

  echo
  ok "Mode A is set up."
  echo "  ${B}Open a NEW terminal${N} (so the exports load) and run: ${B}claude${N}"
  echo "  Inside Claude, type ${B}/mcp${N} — or from a shell: ${B}claude mcp list${N} → geolibre-claude (HTTP) - ✔ Connected"
  echo "  Try it: ${B}/geolibre-claude:ask how many cities have population over 5 million?${N}"
else
  echo
  echo "${B}To finish Mode A by hand (docs/SETUP.md Steps 3–6):${N}"
  echo "  1. echo 'export GEOLIBRE_CLAUDE_AUTH_TOKEN=$tok' >> ~/.zshrc"
  echo "     echo 'export GEOLIBRE_CATALOG_URL=$cat_url'   >> ~/.zshrc  &&  source ~/.zshrc"
  echo "  2. start the service:  docs/SETUP.md Step 4 (launchd)"
  echo "  3. register the plugin: docs/SETUP.md Steps 5–6"
fi

echo; ok "Done. Full walkthrough + troubleshooting: ${B}docs/SETUP.md${N}"
