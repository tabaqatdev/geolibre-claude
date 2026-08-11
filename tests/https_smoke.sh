#!/usr/bin/env bash
# HTTPS + bearer-auth smoke test for the geolibre-claude server.
# Starts the server over TLS (mkcert), runs four checks, stops it.
#
#   bash tests/https_smoke.sh
#
# Requires: a release build (./start.sh or cargo build --release), mkcert, curl, openssl.
set -uo pipefail
cd "$(dirname "$0")/.."
BIN=./target/release/geolibre-claude
[ -x "$BIN" ] || { echo "build first: cargo build --release"; exit 1; }
command -v mkcert >/dev/null || { echo "mkcert required (brew install mkcert)"; exit 1; }

D=$(mktemp -d "${TMPDIR:-/tmp}/glc-https.XXXX")
PORT="${PORT:-8443}"
TOKEN=$(openssl rand -hex 16)
trap '[ -n "${SRV:-}" ] && kill "$SRV" 2>/dev/null; rm -rf "$D"' EXIT

mkcert -cert-file "$D/cert.pem" -key-file "$D/key.pem" 127.0.0.1 localhost >/dev/null 2>&1

GEOLIBRE_CATALOG_URL="https://sampleserver6.arcgisonline.com/arcgis/rest/services" \
GEOLIBRE_CLAUDE_TRANSPORT=http GEOLIBRE_CLAUDE_HTTP_HOST=127.0.0.1 GEOLIBRE_CLAUDE_HTTP_PORT="$PORT" \
GEOLIBRE_CLAUDE_TLS_CERT="$D/cert.pem" GEOLIBRE_CLAUDE_TLS_KEY="$D/key.pem" \
GEOLIBRE_CLAUDE_AUTH_TOKEN="$TOKEN" GEOLIBRE_CLAUDE_OAUTH_ISSUER="https://idp.example.com/realms/tabaqat" \
GEOLIBRE_CLAUDE_ROOT="$D" \
"$BIN" --transport http >"$D/server.log" 2>&1 &
SRV=$!
sleep 3
kill -0 "$SRV" 2>/dev/null || { echo "FAIL: server did not start"; cat "$D/server.log"; exit 1; }

B="https://127.0.0.1:$PORT"
INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}'
fail=0

check() { if eval "$2"; then echo "  PASS: $1"; else echo "  FAIL: $1"; fail=1; fi; }

check "health is open"           "[ \"\$(curl -sk $B/health)\" = ok ]"
check "RFC9728 discovery"        "curl -sk $B/.well-known/oauth-protected-resource | grep -q authorization_servers"
check "no-token POST is 401"     "[ \"\$(curl -sk -o /dev/null -w '%{http_code}' -X POST $B/mcp -H 'Accept: application/json, text/event-stream' -d '$INIT')\" = 401 ]"
check "401 has WWW-Authenticate" "curl -sk -D - -o /dev/null -X POST $B/mcp -d '{}' | grep -qi 'www-authenticate: Bearer'"
check "token POST returns result" "curl -sk -X POST $B/mcp -H 'Authorization: Bearer $TOKEN' -H 'Accept: application/json, text/event-stream' -H 'Content-Type: application/json' -d '$INIT' | grep -q '\"serverInfo\"'"

echo
[ "$fail" = 0 ] && echo "ALL PASS" || echo "SOME CHECKS FAILED"
exit "$fail"
