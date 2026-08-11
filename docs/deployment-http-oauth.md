# Deploying with HTTPS + OAuth (durable, team scale)

The default **stdio** transport is zero-config and right for a single user on one machine — the MCP
client spawns the binary, no ports, no certs. For a **durable Claude Desktop connection at team
scale**, use the **Streamable-HTTP + OAuth** path documented here.

> **Status:** the server's `--transport http` path is **implemented and working** — it serves MCP over
> TLS via `rmcp`'s `transport-streamable-http-server`, gates `/mcp` (plus the bridge's `/project` and
> `/context` endpoints) behind a bearer token, and advertises RFC 9728 protected-resource metadata. The
> bearer token is the OAuth resource-server half; wiring a full IdP (below) so Claude obtains tokens
> interactively is the remaining production step. The infrastructure (TLS, IdP, proxy) below is real.

## Why HTTPS is required

Claude's HTTP connector only accepts **`https`** URLs. So any HTTP deployment needs TLS — locally
trusted for dev, CA-signed for a real host.

## Local dev — mkcert on a defined port

`start.sh` / `start.ps1` already handle this when `GEOLIBRE_CLAUDE_TRANSPORT=http`:

```bash
# .env
GEOLIBRE_CLAUDE_TRANSPORT=http
GEOLIBRE_CLAUDE_HTTP_HOST=127.0.0.1
GEOLIBRE_CLAUDE_HTTP_PORT=8443
GEOLIBRE_CLAUDE_TLS_CERT=certs/localhost.pem
GEOLIBRE_CLAUDE_TLS_KEY=certs/localhost-key.pem
```

`start.sh` runs `mkcert -install` once (adds a locally-trusted CA) and issues
`certs/localhost.pem` for `https://127.0.0.1:8443`. No browser warnings, no prompts.

## Team deployment — IdP + reverse proxy

The MCP server acts as an **OAuth 2.0 Resource Server**; an **Identity Provider** (Keycloak, Auth0,
Cognito, or your SSO) issues and validates tokens. Standards the flow relies on:

- **PKCE** (RFC 7636) for the auth-code exchange,
- **Dynamic Client Registration** (RFC 7591) so Claude can register itself,
- **Protected Resource Metadata** (RFC 9728) so the client discovers the auth server,
- **refresh tokens + silent re-auth** (`prompt=none`) so app restarts don't re-prompt.

Configure the server side via `.env`:

```bash
GEOLIBRE_CLAUDE_OAUTH_ISSUER=https://idp.example.com/realms/tabaqat
GEOLIBRE_CLAUDE_OAUTH_AUDIENCE=geolibre-claude
GEOLIBRE_CLAUDE_OAUTH_JWKS_URL=https://idp.example.com/realms/tabaqat/protocol/openid-connect/certs
```

A minimal shape (TLS terminated at a proxy, IdP alongside):

```yaml
# docker-compose.yml (sketch)
services:
  geolibre-claude:
    build: .
    command: ["--transport", "http"]
    environment:
      GEOLIBRE_CATALOG_URL: ${GEOLIBRE_CATALOG_URL}
      GEOLIBRE_CLAUDE_TRANSPORT: http
      GEOLIBRE_CLAUDE_HTTP_HOST: 0.0.0.0
      GEOLIBRE_CLAUDE_HTTP_PORT: 8443
      GEOLIBRE_CLAUDE_OAUTH_ISSUER: ${OAUTH_ISSUER}
      GEOLIBRE_CLAUDE_OAUTH_JWKS_URL: ${OAUTH_JWKS_URL}
    expose: ["8443"]

  proxy:            # terminates CA-signed TLS, forwards to the server
    image: caddy:2  # or nginx/traefik
    ports: ["443:443"]
    # route https://mcp.example.com → geolibre-claude:8443

  idp:              # Keycloak / your SSO
    image: quay.io/keycloak/keycloak:latest
    # realm with DCR enabled, an audience mapper for `geolibre-claude`
```

Point the client at `https://mcp.example.com`. Use Claude Code's `/mcp` for the dev/auth loop.

## Known caveat (not something you can fully fix server-side)

Some token-persistence failures for **custom / DCR OAuth connectors** are an Anthropic-side
regression around **Claude Code 2.1.117 → 2.1.118 (April 2026)** — see anthropics/claude-code issues
#52565 and #54710; 2.1.118 was the fix-heavy release. Managed connectors were unaffected. Track your
Claude version; the HTTP+OAuth path mitigates but can't fully paper over a client-side bug.
