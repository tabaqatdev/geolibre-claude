# Honest review — code, tests, roadmap

A critical pass over GeoLibre-Claude as if reviewing someone else's work. Written 2026-08-11 at v0.4.

## Verdict

Coherent, well-structured, and — unusually — **verified against real systems** at each step (live
ArcGIS catalog, real DuckDB, a real TLS handshake). The thin-tools/thick-skills split is honoured:
the Rust is small and the intelligence lives in skills. But it is **not v1.0** — it's a strong
v0.4/v0.5 with three unproven capabilities and thin automated-test coverage. Below, specifics.

## Code review by area

### Strengths
- Pure, dependency-free logic (`geolibre-core`: normalization, BM25) is isolated and unit-tested —
  the right seam. Bugs there (the Arabic article-stripping gap) were caught by tests, not users.
- `spatial_sql` is genuinely defended: SELECT-only guard **plus** DuckDB `enable_external_access`
  lockdown (see Security). The `attach` design keeps the user query read-only while the server does
  trusted loading.
- Concurrency-safe project document (mutex around load→mutate→save) — found and fixed by testing
  interleaved calls, which a sequential client would have hidden.
- Compiles against the real `rmcp` 3.1.2 and the real DuckDB; the HTTPS path was proven with an
  actual mkcert TLS handshake, not just documented.

### Issues (by severity)

**HIGH — fixed during this review**
- *`spatial_sql` replacement-scan bypass.* The token denylist blocks reader *functions*
  (`read_csv`, `ST_Read`, …) but not DuckDB's bare-string replacement scan
  `SELECT * FROM '/path/secret.parquet'`. **Fixed:** `SET enable_external_access=false` after the
  trusted attach-load; verified it blocks file reads while normal spatial ops still work.

**MEDIUM — open**
- *Errors are returned as success text.* Tools return `"ERROR: …"` strings, not MCP protocol errors
  (`isError`). The model can read them, but a strict client can't distinguish failure. Consider
  proper `CallToolResult` error results.
- *No DuckDB statement timeout.* A pathological query (accidental cross join) can peg a blocking
  worker. Add `SET statement_timeout='30s'` (or wrap `spawn_blocking` with a timeout).
- *Catalog path segments are interpolated unvalidated.* `service`/`folder` go straight into the URL
  path; a value like `../../x` could reach other paths on the *same* catalog host (mild traversal,
  not cross-host — base host is fixed). Validate segments against `[\w./-]` and reject `..`.
- *Silent truncation.* `attach` caps at `max_records` (default 2000) and the crawl caps at 200
  services — neither surfaces "I truncated" in the result, so an analysis over a large layer can be
  quietly wrong. Flag truncation in the tool output.

**LOW — open**
- `search_catalog` indexes only layer titles + service paths, not field names/aliases — the biggest
  multilingual signal is left on the table. Field enrichment is a real next step, not just polish.
- `app::load` treats a corrupt project file as empty (documented, but a hand-edit could lose work).
- No cross-process lock on the project file (fine for one server; not for two).
- `eprintln!` logging only; no request logging or structured logs on the HTTP path.
- No graceful shutdown / signal handling on the HTTP server.

## Test coverage — **the weakest area**

- **Unit tests (14, all passing):** normalization, BM25 index, the SELECT-only guard (incl. injection
  and reader-bypass cases), and the project-doc mutations. Good coverage of the *pure* and
  *security-critical* logic.
- **Gaps:**
  - **No committed integration tests.** The catalog client, the tools layer, `spatial_sql` execution,
    and the HTTP transport were all verified with **ad-hoc scripts that don't live in the repo**. A
    fresh clone can't prove they work.
  - **No CI.** No automated `cargo test` / `clippy` / `fmt` / `tsc` / `claude plugin validate` on push.
  - **No tests for the app-bridge over the wire** or the plugin's `applyProject` logic.
- **Recommendation (highest-value next work):** add a `tests/` integration suite (spawn the binary,
  drive MCP over stdio against the public sample server) and a GitHub Actions workflow running
  `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`, the plugin `tsc`/build, and
  `claude plugin validate`.

## Roadmap critique

- **"Full parity" (Phase 4) overclaims.** Live map control is *prepared, not proven* — the plugin's
  file-load path under GeoLibre's CSP is untested. Until it runs in the app, "parity" is aspirational.
- **"16 languages" is half true.** Only Arabic has real normalization depth. The other 15 rely on the
  catalog carrying that language's text, or on Claude translating intent to search English. On a
  monolingual English catalog, cross-language search is weak without the deferred glossary/embeddings.
  Honest framing: *Arabic-deep; 15 others via catalog text + translation.*
- **`geolibre-rust` reuse is unproven.** Raster geoprocessing depends on an external WASM artifact
  that isn't built or wired. It's treated as a "reuse win" but is really an unstarted integration —
  either build `geolibre-cli.wasm` + wasmtime, or drop the raster claim to match reality.
- **Correctness leans on the model following skills.** The thin-tools thesis means nothing *enforces*
  (e.g.) reprojecting before measuring — a model that ignores `geolibre-spatial-sql` gets wrong areas
  with no guardrail. Reasonable, but a stated risk.
- **No release engineering.** Not a git repo yet; no CI, changelog, versioning, security policy, or
  CONTRIBUTING. "v1.0" needs these.
- **Timelines are planning-optimistic** (each phase "1–2 weeks" compresses a lot). Fine as a map, not
  a commitment.

## Hardening applied (2026-08-11 follow-up pass)

Grounded in a research pass over the DuckDB "Securing DuckDB" guidance and the MCP 2025-11-25 security
best-practices, plus an independent review of the skills/agents.

**Query builder (the critical path) — done & verified:**
- Full DuckDB lockdown after the trusted attach-load, `lock_configuration=true` last:
  `enable_external_access=false`, `autoinstall/autoload/community extensions=false`,
  `disabled_filesystems='LocalFileSystem'`, `max_expression_depth=1000`. Replacement-scan file read
  is blocked (verified); normal spatial ops unaffected (verified).
- Connection bounded at creation: `max_memory=1GB`, `threads=2`.
- Real cancel: an `interrupt_handle()` watchdog interrupts a query past ~28s, with a 35s outer
  wall-clock bound in the tool. (DuckDB has no `statement_timeout`.)
- SQL length cap (20 KB) + the existing 1000-row output cap.
- Reframed correctly: the **subquery-wrapping is the read-only guarantee** (the parser rejects
  non-SELECT inside `FROM (…)`); the token denylist is only a cheap pre-filter. Added guard unit tests.
- Catalog client: bounded redirect chains (SSRF hygiene) on top of the earlier timeouts.

**Skills/agents — done:** fixed a **critical** bug where the router advertised a phantom
`run_geoprocessing` tool for buffer/clip/dissolve (would hard-fail "buffer the schools"); corrected
every vector-vs-raster reference; documented the `attach`/output caps that could silently truncate;
promoted `search_catalog`/`normalize_query` in `catalog-scout`; added the agent-delegation map to the
router; added read-only `tools:` allowlists to the four read-only agents; corrected the H3, geometry-
column, file-reader, and whole-layer-`statistics` guidance. *(One caveat: the `tools:` allowlists use
the `mcp__geolibre-claude__*` naming convention — confirm on first run that agents still see their
tools; if an agent shows none, adjust the prefix.)*

**Still open (not done here):**
- **Engine-enforced READ_ONLY connection** — stronger than the SET lockdown, but needs a refactor to
  load the catalog into an attached/file DB opened read-only. The SET battery + subquery-wrapping is
  strong today; this is the next increment.
- **OS-level sandbox** (separate process / cgroup / container with a hard SIGKILL) — DuckDB's own docs
  say the in-engine settings are defense-in-depth, not a substitute for isolation.
- **CI + committed integration tests** — still the biggest gap (a `tests/https_smoke.sh` exists; a
  stdio/tool integration suite and a GitHub Actions workflow do not).
- **HTTP:** per-token rate limiting; Origin-header validation (rmcp validates Host, not Origin);
  JWT/JWKS + audience binding (RFC 8707) for real IdP tokens (today: static bearer).
- **MCP protocol conformance** via MCP Inspector CLI in CI.
- Proper MCP error results instead of `"ERROR:"` strings; `statistics`/catalog path-segment validation.

## Prioritized recommendations

1. **CI + committed integration tests** — the single biggest gap; without it, "verified" doesn't
   survive a clone.
2. **Verify the plugin live-load in real GeoLibre** — resolves the one existential unknown (CSP).
3. **`statement_timeout`, path-segment validation, surface truncation** — cheap correctness/robustness.
4. **Decide `run_geoprocessing`** — build the WASM path or drop the raster promise.
5. **Right-size the multilingual claim** — or invest in the field-alias index + glossary.
6. **Proper MCP error results** instead of `"ERROR:"` strings.
