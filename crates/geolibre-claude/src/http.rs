//! HTTPS + OAuth (Streamable-HTTP) transport — the durable path for Claude Desktop.
//!
//! Claude's HTTP connector requires `https`, so this serves the MCP over TLS
//! (mkcert locally, a CA cert in production) and gates `/mcp` behind a bearer token.
//! The token check is the OAuth **resource-server** half — a static token makes it
//! testable without an IdP; for production, `authorization_servers` in the RFC 9728
//! metadata points Claude at your IdP, which mints the tokens.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};

use crate::tools::GeolibreServer;

pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub auth_token: Option<String>,
    pub issuer: Option<String>,
}

#[derive(Clone)]
struct AuthState {
    token: String,
    metadata_url: String,
}

pub async fn serve(server: GeolibreServer, cfg: HttpConfig) -> Result<()> {
    // rustls 0.23 needs a process-default crypto provider before building a ServerConfig.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let base = format!("{}:{}", cfg.host, cfg.port);
    let metadata_url = format!("https://{base}/.well-known/oauth-protected-resource");

    // StreamableHttpServerConfig is #[non_exhaustive]; build from default, then set fields.
    let mut http_config = StreamableHttpServerConfig::default();
    http_config.allowed_hosts = vec![
        cfg.host.clone(),
        base.clone(),
        "localhost".to_string(),
        format!("localhost:{}", cfg.port),
    ];

    let mcp = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        http_config,
    );

    let prm = protected_resource_metadata(&base, cfg.issuer.as_deref());

    let mut app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/.well-known/oauth-protected-resource",
            get(move || {
                let prm = prm.clone();
                async move { Json(prm) }
            }),
        )
        .nest_service("/mcp", mcp);

    // Gate everything except health + discovery behind the bearer token, if set.
    match &cfg.auth_token {
        Some(token) => {
            let auth = AuthState { token: token.clone(), metadata_url };
            app = app.layer(middleware::from_fn_with_state(auth, require_bearer));
            eprintln!("  auth: bearer token required (GEOLIBRE_CLAUDE_AUTH_TOKEN)");
        }
        None => {
            eprintln!("  WARNING: no GEOLIBRE_CLAUDE_AUTH_TOKEN set — /mcp is UNAUTHENTICATED.");
            eprintln!("           set a token (or an OAuth issuer) before exposing this beyond localhost.");
        }
    }

    let addr: SocketAddr = base.parse().with_context(|| format!("invalid address {base}"))?;
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cfg.cert, &cfg.key)
        .await
        .with_context(|| {
            format!("load TLS cert/key ({} / {})", cfg.cert.display(), cfg.key.display())
        })?;

    eprintln!("  listening on https://{base}/mcp");
    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service())
        .await
        .context("HTTPS server error")?;
    Ok(())
}

fn protected_resource_metadata(base: &str, issuer: Option<&str>) -> serde_json::Value {
    // RFC 9728 — lets a client discover the auth server for this resource.
    let mut m = serde_json::json!({
        "resource": format!("https://{base}/mcp"),
        "bearer_methods_supported": ["header"],
    });
    if let Some(iss) = issuer {
        m["authorization_servers"] = serde_json::json!([iss]);
    }
    m
}

async fn require_bearer(State(auth): State<AuthState>, req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path();
    // Health and OAuth discovery are intentionally open.
    if path == "/health" || path.starts_with("/.well-known/") {
        return next.run(req).await;
    }

    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    if let Some(got) = provided {
        if constant_time_eq(got.as_bytes(), auth.token.as_bytes()) {
            return next.run(req).await;
        }
    }

    let mut res = (StatusCode::UNAUTHORIZED, "unauthorized\n").into_response();
    if let Ok(v) = HeaderValue::from_str(&format!(
        "Bearer resource_metadata=\"{}\"",
        auth.metadata_url
    )) {
        res.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    res
}

/// Length-aware, difference-accumulating comparison so a wrong token doesn't leak
/// its correct length or prefix through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
