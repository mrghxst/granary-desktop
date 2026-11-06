<#
.SYNOPSIS
  Build the Granary NSIS installer (per-user install) with the pinned libmpv bundled next to granary.exe.
  Writes target/release/bundle/nsis/<installer>.exe plus a .sha256 file and prints both. Idempotent.
  Runs on Windows PowerShell 5.1 and PowerShell 7; CI release.yml calls the same script.
.PARAMETER Install
  Run `pnpm install --frozen-lockfile` first.
#>
[CmdletBinding()]
param([switch]$Install)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
foreach ($dir in @((Join-Path $env:USERPROFILE '.cargo\bin'), (Join-Path $env:APPDATA 'npm'))) {
  if ($dir -and (Test-Path $dir) -and -not (($env:PATH -split ';') -contains $dir)) { $env:PATH = "$dir;$env:PATH" }
}

function Run([string]$Exe, [string[]]$CmdArgs) {
  Write-Host "==> $Exe $($CmdArgs -join ' ')" -ForegroundColor Cyan
  $ErrorActionPreference = 'Continue'
  & $Exe @CmdArgs
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  Write-Host "<== exit $code"
  if ($code -ne 0) { throw "$Exe failed (exit $code)" }
}

$ps = (Get-Process -Id $PID).Path
Run $ps @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/fetch-libmpv.ps1')
if ($Install) { Run 'pnpm' @('install', '--frozen-lockfile') }
Run 'pnpm' @('--filter', 'desktop', 'tauri', 'build', '--bundles', 'nsis')

$out = Join-Path $root 'target\release\bundle\nsis'
$installer = Get-ChildItem $out -Filter '*-setup.exe' | Sort-Object LastWriteTime | Select-Object -Last 1
if (-not $installer) { throw "no installer found in $out" }
$hash = (Get-FileHash -Algorithm SHA256 $installer.FullName).Hash.ToLowerInvariant()
$shaFile = "$($installer.FullName).sha256"
Set-Content -Encoding ascii -NoNewline $shaFile "$hash  $($installer.Name)"
Write-Host ""
Write-Host "installer : $($installer.FullName) ($([math]::Round($installer.Length / 1MB, 1)) MB)" -ForegroundColor Green
Write-Host "sha256    : $hash"
exit 0
