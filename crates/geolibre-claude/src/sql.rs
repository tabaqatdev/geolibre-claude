//! SELECT-only guard + DuckDB Spatial execution for the `spatial_sql` tool.
//!
//! The guard is a safety boundary, not a hint: the user's query must be a single
//! read-only statement, and the sanctioned way to get data in is the server-loaded
//! `attach` tables — so file/URL-reading functions are refused too, closing the
//! local-file / exfiltration hole. Extension loading and table creation happen in
//! trusted server-built SQL, never in the user's string.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

/// Reject absurdly long SQL before it reaches the parser.
const MAX_SQL_LEN: usize = 20_000;
/// In-engine cancel deadline (best-effort; the tool also has an outer wall-clock bound).
const QUERY_INTERRUPT_SECS: u64 = 28;

/// A catalog layer the server has fetched and written to a local GeoJSON file,
/// to be exposed to the query as a DuckDB table named `name`.
pub struct LoadedTable {
    pub name: String,
    pub geojson_path: String,
}

/// Tokens that must never appear in a read-only query. Covers DDL/DML, transaction
/// and session control, extension management, and every file/URL reader (data comes
/// in via `attach`, so the query never needs to touch the filesystem or network).
const DENY: &[&str] = &[
    "attach", "detach", "install", "load", "copy", "pragma", "call", "export", "import", "set",
    "reset", "checkpoint", "vacuum", "insert", "update", "delete", "create", "drop", "alter",
    "replace", "truncate", "read_csv", "read_csv_auto", "read_parquet", "read_json",
    "read_json_auto", "read_ndjson", "read_text", "read_blob", "parquet_scan", "csv_scan", "glob",
    "st_read", "st_readosm", "st_read_meta", "getenv", "sniff_csv",
];

/// Validate that `sql` is a single SELECT/WITH statement with no forbidden tokens.
pub fn ensure_select_only(sql: &str) -> Result<(), String> {
    if sql.len() > MAX_SQL_LEN {
        return Err(format!("query too long ({} bytes; max {MAX_SQL_LEN})", sql.len()));
    }
    let stripped = strip_comments(sql);
    let body = stripped.trim().trim_end_matches(';').trim();
    if body.is_empty() {
        return Err("empty query".into());
    }
    if body.contains(';') {
        return Err("multiple statements are not allowed — send one SELECT".into());
    }
    let lower = body.to_ascii_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    if first != "select" && first != "with" {
        return Err(format!("only SELECT / WITH queries are allowed (got `{first}`)"));
    }
    for tok in tokenize(&lower) {
        if DENY.contains(&tok.as_str()) {
            return Err(format!(
                "`{tok}` is not allowed in a read-only query — load data via `attach`, not file readers"
            ));
        }
    }
    Ok(())
}

/// Validate a free-text SQL fragment (a WHERE clause) is side-effect-free: no
/// statement terminator and none of the denied tokens (file/URL readers, DDL, …).
/// Used for the GeoParquet backend, where the connection must keep file access on
/// to read the parquet — so guarding the user's WHERE is the sandbox.
pub fn ensure_fragment_safe(fragment: &str) -> Result<(), String> {
    let stripped = strip_comments(fragment);
    if stripped.contains(';') {
        return Err("`;` is not allowed in a filter".into());
    }
    let lower = stripped.to_ascii_lowercase();
    for tok in tokenize(&lower) {
        if DENY.contains(&tok.as_str()) {
            return Err(format!("`{tok}` is not allowed in a filter — reference fields only"));
        }
    }
    Ok(())
}

/// A comma-separated list of field references / order terms — letters, digits, `_`,
/// `.`, spaces, commas, `*`. No parentheses (so no function calls) or other
/// punctuation, which stops a subquery or reader being smuggled into a field list.
pub fn is_ident_list(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ',' | ' ' | '*'))
}

/// A valid DuckDB identifier for an attached table name (guards the CREATE TABLE).
pub fn is_valid_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false)
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn strip_comments(sql: &str) -> String {
    // Remove /* block */ and -- line comments so tokens can't hide in them.
    let mut out = String::with_capacity(sql.len());
    let b = sql.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'-' && i + 1 < b.len() && b[i + 1] == b'-' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

fn tokenize(sql_lower: &str) -> Vec<String> {
    sql_lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// Run a validated read-only query in a fresh in-memory DuckDB, after loading the
/// spatial + json extensions and creating one table per `attach` entry. Blocking —
/// call from `spawn_blocking`. Rows are returned as JSON via DuckDB's `to_json`,
/// which keeps numbers/text/nesting typed without hand-mapping every DuckDB value.
pub fn run_query(sql: &str, tables: &[LoadedTable], max_rows: usize) -> Result<Value> {
    // Bound the sandbox at creation: cap memory and threads so a runaway or
    // adversarial query (e.g. an accidental cross join) can't exhaust the host.
    // Read-only access mode isn't usable here — we must create the attach tables
    // first — so we lock the connection down at runtime instead (below).
    let config = duckdb::Config::default()
        .max_memory("1GB")
        .and_then(|c| c.threads(2))
        .context("configure DuckDB limits")?;
    let conn =
        duckdb::Connection::open_in_memory_with_flags(config).context("open in-memory DuckDB")?;

    // Give DuckDB a stable extension cache dir so it doesn't depend on $HOME being
    // set in whatever process spawns the server, and so the spatial/json downloads
    // are fetched once and reused across calls.
    let home = std::env::temp_dir().join("geolibre-claude-duckdb");
    std::fs::create_dir_all(&home).ok();
    conn.execute_batch(&format!(
        "SET home_directory='{}';",
        home.to_string_lossy().replace('\'', "''")
    ))
    .context("set DuckDB home_directory")?;

    conn.execute_batch("INSTALL spatial; LOAD spatial; INSTALL json; LOAD json;")
        .context("load DuckDB spatial/json extensions (needs network on first use)")?;

    for t in tables {
        // name is pre-validated as an identifier; path is server-generated.
        let ddl = format!(
            "CREATE TABLE {} AS SELECT * FROM ST_Read('{}');",
            t.name,
            t.geojson_path.replace('\'', "''")
        );
        conn.execute_batch(&ddl)
            .with_context(|| format!("load attached table `{}`", t.name))?;
    }

    // Defence in depth: the token denylist can't catch DuckDB's *replacement scan*
    // (`SELECT * FROM '/path/secret.parquet'` reads a file with no function name).
    // All trusted file access (extensions, ST_Read of attach files) is done above;
    // lock external access off before running the user's query so it can't touch the
    // filesystem or network at all.
    // DuckDB "Securing DuckDB" lockdown, applied only after trusted setup and with
    // lock_configuration LAST so the user query can't turn any of it back on:
    //   - enable_external_access=false → no file/URL access (incl. replacement scans)
    //   - autoinstall/autoload/community/unsigned extensions off → no arbitrary native code
    //   - disabled_filesystems → belt-and-suspenders FS block
    //   - max_expression_depth → bound pathological nesting
    conn.execute_batch(
        "SET enable_external_access=false;\
         SET autoinstall_known_extensions=false;\
         SET autoload_known_extensions=false;\
         SET allow_community_extensions=false;\
         SET disabled_filesystems='LocalFileSystem';\
         SET max_expression_depth=1000;\
         SET lock_configuration=true;",
    )
    .context("apply DuckDB security lockdown before running the query")?;

    // Wrap the user's SELECT as a subquery. This is the real read-only guarantee:
    // the parser rejects anything that isn't a table-producing SELECT (a DROP/INSERT
    // can't sit inside `FROM (…)`), so the token denylist is only a cheap pre-filter.
    let wrapped = format!(
        "SELECT to_json(sub) AS j FROM ({}) AS sub",
        sql.trim().trim_end_matches(';')
    );

    // Best-effort in-engine cancel: a watchdog thread interrupts a query that runs
    // past the deadline (DuckDB has no statement_timeout). memory/thread caps from the
    // Config bound the blast radius meanwhile.
    let interrupt = conn.interrupt_handle();
    let done = Arc::new(AtomicBool::new(false));
    let done_w = done.clone();
    let watchdog = std::thread::spawn(move || {
        let mut waited = 0u64;
        while waited < QUERY_INTERRUPT_SECS * 1000 {
            std::thread::sleep(Duration::from_millis(100));
            if done_w.load(Ordering::Relaxed) {
                return;
            }
            waited += 100;
        }
        interrupt.interrupt();
    });

    let result = (|| -> Result<(Vec<Value>, bool)> {
        let mut stmt = conn.prepare(&wrapped).context("prepare query")?;
        let mut rows = stmt.query([]).context("execute query")?;
        let mut out = Vec::new();
        let mut truncated = false;
        while let Some(row) = rows.next().context("read row")? {
            if out.len() >= max_rows {
                truncated = true;
                break;
            }
            let s: String = row.get(0).context("read row json")?;
            out.push(serde_json::from_str(&s).unwrap_or(Value::String(s)));
        }
        Ok((out, truncated))
    })();

    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let (out, truncated) = result?;

    Ok(serde_json::json!({
        "rows": out,
        "row_count": out.len(),
        "truncated": truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_select_and_cte() {
        assert!(ensure_select_only("SELECT 1").is_ok());
        assert!(ensure_select_only("  select * from cities where pop > 5  ").is_ok());
        assert!(ensure_select_only("WITH x AS (SELECT 1 AS a) SELECT a FROM x").is_ok());
        assert!(ensure_select_only("SELECT 1;").is_ok()); // single trailing semicolon
    }

    #[test]
    fn rejects_non_select_statements() {
        for q in [
            "DROP TABLE cities",
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a=1",
            "DELETE FROM t",
            "CREATE TABLE t (a int)",
            "PRAGMA database_list",
            "ATTACH 'x.db'",
        ] {
            assert!(ensure_select_only(q).is_err(), "should reject: {q}");
        }
    }

    #[test]
    fn rejects_multiple_statements_and_injection() {
        assert!(ensure_select_only("SELECT 1; DROP TABLE cities").is_err());
        assert!(ensure_select_only("SELECT 1; SELECT 2").is_err());
        // comment-hidden second statement
        assert!(ensure_select_only("SELECT 1 -- ok\n; DROP TABLE t").is_err());
    }

    #[test]
    fn rejects_file_and_url_readers() {
        for q in [
            "SELECT * FROM read_csv('/etc/passwd')",
            "SELECT * FROM read_parquet('s3://x/y.parquet')",
            "SELECT * FROM ST_Read('/etc/hosts')",
            "SELECT * FROM read_json_auto('http://x')",
            "SELECT * FROM glob('/**')",
        ] {
            assert!(ensure_select_only(q).is_err(), "should reject reader: {q}");
        }
    }

    #[test]
    fn rejects_overlong_sql() {
        let long = format!("SELECT {}", "1,".repeat(20_000));
        assert!(ensure_select_only(&long).is_err());
    }

    #[test]
    fn valid_ident_rules() {
        assert!(is_valid_ident("cities"));
        assert!(is_valid_ident("_tmp2"));
        assert!(!is_valid_ident("2cities")); // leading digit
        assert!(!is_valid_ident("a b")); // space
        assert!(!is_valid_ident("a;b")); // punctuation
        assert!(!is_valid_ident("")); // empty
    }
}

