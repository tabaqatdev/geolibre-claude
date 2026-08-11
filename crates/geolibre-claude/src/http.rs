//! HTTPS + OAuth (Streamable-HTTP) transport — the durable path for Claude Desktop.
//!
//! Claude's HTTP connector requires `https`, so this serves the MCP over TLS
//! (mkcert locally, a CA cert in production) and gates `/mcp` behind a bearer token.
//! The token check is the OAuth **resource-server** half — a static token makes it
//! testable without an IdP; for production, `authorization_servers` in the RFC 9728
//! metadata points Claude at your IdP, which mints the tokens.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, put},
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
    /// The project document the GeoLibre bridge reads via `GET /project`.
    pub project_path: PathBuf,
    /// Where the bridge writes the live map context via `PUT /context`.
    pub context_path: PathBuf,
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

    // Bridge file-exchange endpoints. GeoLibre's webview CSP forbids `file:` in
    // connect-src (and `fetch` can't read/write `file://` anyway), but it allows
    // `https:` and `http://localhost:*` — so the plugin reaches these over the same
    // HTTPS listener instead of touching the filesystem directly. Both sit behind
    // the bearer gate below.
    let project_path = cfg.project_path.clone();
    let context_path = cfg.context_path.clone();

    let mut app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/.well-known/oauth-protected-resource",
            get(move || {
                let prm = prm.clone();
                async move { Json(prm) }
            }),
        )
        .route(
            "/project",
            get(move || {
                let p = project_path.clone();
                async move { serve_project(&p).await }
            }),
        )
        .route(
            "/context",
            put(move |body: Bytes| {
                let p = context_path.clone();
                async move { write_context(&p, body).await }
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

/// `GET /project` — serve the project document (`claude.geolibre.json`) the
/// app-bridge tools maintain, so the plugin can apply Claude's map. 404 before the
/// first tool creates it; the bridge treats that as "nothing to apply yet".
async fn serve_project(path: &Path) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            ([(header::CONTENT_TYPE, "application/json")], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "no project document yet\n").into_response(),
    }
}

/// `PUT /context` — the plugin reports the live map (`map-context.json`) so
/// describe_layer / query_data see the on-map layers and resolve tokens. The body
/// must be JSON; anything else is rejected before it can corrupt the file the
/// server later reads. Body size is bounded by axum's default request limit.
async fn write_context(path: &Path, body: Bytes) -> Response {
    if serde_json::from_slice::<serde_json::Value>(&body).is_err() {
        return (StatusCode::BAD_REQUEST, "body must be JSON\n").into_response();
    }
    match tokio::fs::write(path, &body).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("write failed: {e}\n")).into_response()
        }
    }
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
