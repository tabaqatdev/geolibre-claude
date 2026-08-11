#Requires -Version 5
# GeoLibre-Claude — stop services (Windows). Use -Purge to also remove the plugin drop-in.
param([switch]$Purge)
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root
$RunDir = Join-Path $Root '.run'

function Info($m) { Write-Host "> $m" -ForegroundColor Cyan }
function Ok($m)   { Write-Host "* $m" -ForegroundColor Green }
function Warn($m) { Write-Warning $m }

$cfg = @{}
if (Test-Path "$Root/.env") {
  Get-Content "$Root/.env" | ForEach-Object {
    $line = $_.Trim()
    if ($line -and -not $line.StartsWith('#') -and $line.Contains('=')) {
      $i = $line.IndexOf('='); $cfg[$line.Substring(0, $i).Trim()] = $line.Substring($i + 1).Trim()
    }
  }
}
$PluginsDir = $HOME + '/.geolibre/plugins'
if ($cfg.ContainsKey('GEOLIBRE_PLUGINS_DIR') -and $cfg['GEOLIBRE_PLUGINS_DIR']) {
  $PluginsDir = $cfg['GEOLIBRE_PLUGINS_DIR'].Replace('~', $HOME)
}

# ── stop the HTTP daemon, if any ─────────────────────────────────────────────
if (Test-Path "$RunDir/mcp.pid") {
  $procId = (Get-Content "$RunDir/mcp.pid").Trim()
  $proc = Get-Process -Id $procId -ErrorAction SilentlyContinue
  if ($proc) { Stop-Process -Id $procId -Force; Ok "Stopped MCP server (pid $procId)." }
  else { Warn "No live process for pid $procId." }
  Remove-Item "$RunDir/mcp.pid" -ErrorAction SilentlyContinue
} else {
  Info "No HTTP MCP server running (stdio servers are spawned by the client)."
}

# ── optionally remove the plugin drop-in ─────────────────────────────────────
if ($Purge) {
  $Dest = Join-Path $PluginsDir 'geolibre-claude-bridge'
  if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest; Ok "Removed plugin drop-in $Dest" }
  else { Info "No plugin drop-in to remove." }
}

Ok "Done."
