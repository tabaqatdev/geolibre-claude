//! `geolibre-claude` — the MCP server binary.
//!
//! Named to avoid colliding with GeoLibre's own upstream `geolibre-mcp`
//! (which authors `.geolibre.json` projects). This server's job is different:
//! DATA tools over a geospatial catalog (Phase 1, here), then GEO tools
//! (geolibre-rust via wasmtime) and app-bridge tools in later phases.

mod app;
mod catalog;
mod context;
mod geoparquet;
mod http;
mod sql;
mod tools;

use std::env;
use std::path::PathBuf;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};

use crate::catalog::Catalog;
use crate::tools::GeolibreServer;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Config {
    transport: String,
    http_host: String,
    http_port: u16,
    catalog_url: String,
    default_locale: String,
    root: Option<String>,
}

impl Config {
    fn from_env_and_args(args: &[String]) -> Self {
        let mut transport = env::var("GEOLIBRE_CLAUDE_TRANSPORT").unwrap_or_else(|_| "stdio".into());
        let http_host = env::var("GEOLIBRE_CLAUDE_HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let http_port = env::var("GEOLIBRE_CLAUDE_HTTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8443);
        let catalog_url = env::var("GEOLIBRE_CATALOG_URL").unwrap_or_default();
        let default_locale =
            env::var("GEOLIBRE_CLAUDE_DEFAULT_LOCALE").unwrap_or_else(|_| "en".into());

        // --transport <stdio|http> and --root <dir> override env values.
        let mut root = env::var("GEOLIBRE_CLAUDE_ROOT").ok();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--transport" => {
                    if let Some(v) = it.next() {
                        transport = v.clone();
                    }
                }
                "--root" => {
                    if let Some(v) = it.next() {
                        root = Some(v.clone());
                    }
                }
                _ => {}
            }
        }

        Config { transport, http_host, http_port, catalog_url, default_locale, root }
    }
}

fn print_help() {
    println!(
        "geolibre-claude {VERSION}\n\
         MCP server: geospatial catalog + live GeoLibre control for Claude.\n\n\
         USAGE:\n    geolibre-claude [--transport <stdio|http>] [--root <dir>]\n    geolibre-claude apikey [--save]      mint a bearer token for the HTTPS connector\n\n\
         FLAGS:\n    --transport   stdio (default) or http (Streamable-HTTP + OAuth, needs TLS)\n\
         \x20   --root        base directory for project files\n    -h, --help    show this help\n    -V, --version show version\n"
    );
}

/// The project document the app-bridge tools maintain: `<root>/claude.geolibre.json`,
/// where root is `--root` / `GEOLIBRE_CLAUDE_ROOT` (with `~` expanded), else the cwd.
/// A distinct filename so it never clobbers a user's own `.geolibre.json`.
fn project_path(cfg: &Config) -> PathBuf {
    let root = match &cfg.root {
        Some(r) if r == "~" || r.starts_with("~/") => {
            match env::var("HOME") {
                Ok(home) => PathBuf::from(r.replacen('~', &home, 1)),
                Err(_) => PathBuf::from(r),
            }
        }
        Some(r) => PathBuf::from(r),
        None => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    root.join("claude.geolibre.json")
}

/// `geolibre-claude apikey [--save]` — mint a bearer token (the HTTPS connector
/// credential). Uses the OS CSPRNG. `--save` writes it into ./.env.
fn gen_apikey(args: &[String]) -> Result<()> {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).map_err(|e| anyhow::anyhow!("OS RNG failed: {e}"))?;
    let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();

    if args.iter().any(|a| a == "--save") {
        let path = std::path::Path::new(".env");
        let key = "GEOLIBRE_CLAUDE_AUTH_TOKEN";
        let mut lines: Vec<String> = if path.exists() {
            std::fs::read_to_string(path)?.lines().map(str::to_string).collect()
        } else {
            Vec::new()
        };
        let entry = format!("{key}={token}");
        match lines.iter_mut().find(|l| l.starts_with(&format!("{key}="))) {
            Some(l) => *l = entry,
            None => lines.push(entry),
        }
        std::fs::write(path, lines.join("\n") + "\n")?;
        eprintln!("Saved {key} to .env");
    }

    eprintln!("API key (use as: Authorization: Bearer <token>):");
    println!("{token}"); // the token itself goes to stdout so it's pipeable
    eprintln!("Tip: `geolibre-claude apikey --save` writes it into .env for you.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("apikey") {
        return gen_apikey(&args);
    }

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("geolibre-claude {VERSION}");
        return Ok(());
    }

    let cfg = Config::from_env_and_args(&args);

    // Banner to stderr so it never corrupts an stdio JSON-RPC stream.
    eprintln!("geolibre-claude {VERSION} — transport={}", cfg.transport);
    eprintln!(
        "  catalog: {}",
        if cfg.catalog_url.is_empty() { "(unset — set GEOLIBRE_CATALOG_URL)" } else { &cfg.catalog_url }
    );
    eprintln!("  default locale: {} (16 GeoLibre locales, fallback en)", cfg.default_locale);
    if let Some(root) = &cfg.root {
        eprintln!("  root: {root}");
    }

    if cfg.catalog_url.is_empty() {
        eprintln!("  note: GEOLIBRE_CATALOG_URL unset — layers use absolute references from the map.");
    }

    match cfg.transport.as_str() {
        "stdio" => {
            let project_path = project_path(&cfg);
            eprintln!("  project: {}", project_path.display());
            let base = (!cfg.catalog_url.is_empty()).then_some(cfg.catalog_url.as_str());
            let parquet = geoparquet::GeoParquetConfig {
                local_root: env::var("GEOLIBRE_PARQUET_ROOT")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
                url_prefix: env::var("GEOLIBRE_PARQUET_URL_PREFIX").ok().filter(|s| !s.is_empty()),
            };
            let server = GeolibreServer::new(Catalog::new(base), parquet, project_path);
            let service = server.serve(stdio()).await?;
            service.waiting().await?;
            Ok(())
        }
        "http" => {
            let project_path = project_path(&cfg);
            eprintln!("  project: {}", project_path.display());
            // The bridge's `PUT /context` target — same directory as the project doc.
            let context_path = project_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("map-context.json");
            let base = (!cfg.catalog_url.is_empty()).then_some(cfg.catalog_url.as_str());
            let parquet = geoparquet::GeoParquetConfig {
                local_root: env::var("GEOLIBRE_PARQUET_ROOT")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
                url_prefix: env::var("GEOLIBRE_PARQUET_URL_PREFIX").ok().filter(|s| !s.is_empty()),
            };
            let server = GeolibreServer::new(Catalog::new(base), parquet, project_path.clone());
            let http_cfg = http::HttpConfig {
                host: cfg.http_host.clone(),
                port: cfg.http_port,
                cert: PathBuf::from(
                    env::var("GEOLIBRE_CLAUDE_TLS_CERT").unwrap_or_else(|_| "certs/localhost.pem".into()),
                ),
                key: PathBuf::from(
                    env::var("GEOLIBRE_CLAUDE_TLS_KEY").unwrap_or_else(|_| "certs/localhost-key.pem".into()),
                ),
                auth_token: env::var("GEOLIBRE_CLAUDE_AUTH_TOKEN").ok().filter(|t| !t.is_empty()),
                issuer: env::var("GEOLIBRE_CLAUDE_OAUTH_ISSUER").ok().filter(|s| !s.is_empty()),
                project_path,
                context_path,
            };
            http::serve(server, http_cfg).await
        }
        other => anyhow::bail!("unknown transport '{other}' (expected stdio or http)"),
    }
}
