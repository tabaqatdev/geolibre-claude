#Requires -Version 5
# GeoLibre-Claude — build all code, prepare TLS/plugin, and start services (Windows).
# macOS / Linux / WSL / Git Bash users: use ./start.sh instead.
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root
$RunDir = Join-Path $Root '.run'
New-Item -ItemType Directory -Force -Path $RunDir | Out-Null

function Info($m) { Write-Host "> $m" -ForegroundColor Cyan }
function Ok($m)   { Write-Host "* $m" -ForegroundColor Green }
function Warn($m) { Write-Warning $m }
function Have($c) { $null -ne (Get-Command $c -ErrorAction SilentlyContinue) }

# ── config ───────────────────────────────────────────────────────────────────
if (-not (Test-Path "$Root/.env")) {
  Warn ".env not found - creating from .env.example (set GEOLIBRE_CATALOG_URL)."
  Copy-Item "$Root/.env.example" "$Root/.env"
}
$cfg = @{}
Get-Content "$Root/.env" | ForEach-Object {
  $line = $_.Trim()
  if ($line -and -not $line.StartsWith('#') -and $line.Contains('=')) {
    $i = $line.IndexOf('='); $cfg[$line.Substring(0, $i).Trim()] = $line.Substring($i + 1).Trim()
  }
}
function Cfg($k, $d) { if ($cfg.ContainsKey($k) -and $cfg[$k]) { $cfg[$k] } else { $d } }

$Transport  = Cfg 'GEOLIBRE_CLAUDE_TRANSPORT' 'stdio'
$HttpHost   = Cfg 'GEOLIBRE_CLAUDE_HTTP_HOST' '127.0.0.1'
$HttpPort   = Cfg 'GEOLIBRE_CLAUDE_HTTP_PORT' '8443'
$TlsCert    = Cfg 'GEOLIBRE_CLAUDE_TLS_CERT' 'certs/localhost.pem'
$TlsKey     = Cfg 'GEOLIBRE_CLAUDE_TLS_KEY' 'certs/localhost-key.pem'
$PluginsDir = (Cfg 'GEOLIBRE_PLUGINS_DIR' "$HOME/.geolibre/plugins").Replace('~', $HOME)
$Bin        = Join-Path $Root 'target/release/geolibre-claude.exe'

Info "GeoLibre-Claude - transport=$Transport"

# ── 1. build Rust workspace ──────────────────────────────────────────────────
if (-not (Have 'cargo')) { throw "cargo not found. Install Rust from https://rustup.rs" }
Info "Building Rust workspace (cargo build --release)..."
cargo build --release
Ok "Built $Bin"

# ── 2. build the GeoLibre plugin (TypeScript) ────────────────────────────────
$PluginSrc = Join-Path $Root 'plugins/geolibre-claude-bridge'
if ((Have 'npm') -and (Test-Path "$PluginSrc/package.json")) {
  Info "Building GeoLibre plugin..."
  try {
    Push-Location $PluginSrc
    if (-not (Test-Path 'node_modules')) { npm install --silent }
    npm run --silent build
    Ok "Built plugin -> $PluginSrc/dist"
  } catch { Warn "Plugin build failed - skipping (live map control is optional)." }
  finally { Pop-Location }
} else { Warn "npm not found or no plugin package.json - skipping plugin build." }

# ── 3. TLS certs (http transport only) ───────────────────────────────────────
if ($Transport -eq 'http') {
  if (Have 'mkcert') {
    New-Item -ItemType Directory -Force -Path (Join-Path $Root 'certs') | Out-Null
    if (-not (Test-Path "$Root/$TlsCert") -or -not (Test-Path "$Root/$TlsKey")) {
      Info "Issuing locally-trusted TLS cert with mkcert..."
      try { mkcert -install | Out-Null } catch { Warn "mkcert -install needed elevated rights; continuing." }
      Push-Location $Root
      mkcert -cert-file $TlsCert -key-file $TlsKey $HttpHost localhost 127.0.0.1 ::1 | Out-Null
      Pop-Location
      Ok "Cert ready: $TlsCert"
    } else { Ok "TLS cert already present." }
  } else { Warn "mkcert not found - HTTPS needs it. Install: https://github.com/FiloSottile/mkcert" }
}

# ── 4. install the plugin drop-in so GeoLibre discovers it ───────────────────
if (Test-Path "$PluginSrc/plugin.json") {
  New-Item -ItemType Directory -Force -Path $PluginsDir | Out-Null
  $Dest = Join-Path $PluginsDir 'geolibre-claude-bridge'
  try {
    if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
    New-Item -ItemType SymbolicLink -Path $Dest -Target $PluginSrc | Out-Null
    Ok "Linked plugin into $Dest"
  } catch {
    Copy-Item -Recurse -Force $PluginSrc $Dest
    Ok "Copied plugin into $Dest (symlink needs Developer Mode/admin)"
  }
}

# ── 5. start service ─────────────────────────────────────────────────────────
if ($Transport -eq 'http') {
  Info "Starting HTTPS MCP server on https://${HttpHost}:${HttpPort} ..."
  $p = Start-Process -FilePath $Bin -ArgumentList '--transport', 'http' -PassThru `
        -RedirectStandardOutput "$RunDir/mcp.out.log" -RedirectStandardError "$RunDir/mcp.err.log"
  $p.Id | Out-File -Encoding ascii "$RunDir/mcp.pid"
  Start-Sleep -Seconds 1
  if (-not $p.HasExited) {
    Ok "Server running (pid $($p.Id)). Logs: .run/mcp.*.log"
  } else {
    Remove-Item "$RunDir/mcp.pid" -ErrorAction SilentlyContinue
    Warn "Server exited immediately. HTTPS+OAuth transport lands in Phase 5 - see .run/mcp.err.log"
  }
} else {
  Ok "stdio transport - the MCP client spawns the binary. Register it with:"
  Write-Host "`n    claude mcp add geolibre-claude -- `"$Bin`" --transport stdio`n"
}

Ok "Done."
