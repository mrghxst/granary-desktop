<#
.SYNOPSIS
  Full local check suite (the quality bar before every gate): cargo fmt --check, clippy -D warnings, cargo test,
  TS typecheck, eslint, prettier --check, vitest, frontend build and the secret scan.
  Prints every command with its exit code and exits non-zero if any step failed. Idempotent; runs on Windows
  PowerShell 5.1 and PowerShell 7. CI runs the same script.
.PARAMETER Install
  Run `pnpm install --frozen-lockfile` first (CI, fresh clones).
.PARAMETER SkipFetch
  Do not call scripts/fetch-libmpv.ps1 (the DLL/import library must then already be in vendor-bin/libmpv).
#>
[CmdletBinding()]
param([switch]$Install, [switch]$SkipFetch)
$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Tool shims that are not on PATH in every shell on this machine (rustup, corepack pnpm).
foreach ($dir in @((Join-Path $env:USERPROFILE '.cargo\bin'), (Join-Path $env:APPDATA 'npm'))) {
  if ($dir -and (Test-Path $dir) -and -not (($env:PATH -split ';') -contains $dir)) { $env:PATH = "$dir;$env:PATH" }
}

$results = New-Object System.Collections.Generic.List[object]
function Step([string]$Name, [string]$Exe, [string[]]$CmdArgs) {
  $line = "$Exe $($CmdArgs -join ' ')"
  Write-Host ""
  Write-Host "==> $Name : $line" -ForegroundColor Cyan
  $sw = [Diagnostics.Stopwatch]::StartNew()
  & $Exe @CmdArgs
  $code = $LASTEXITCODE
  if ($null -eq $code) { $code = 0 }
  $sw.Stop()
  $color = if ($code -eq 0) { 'Green' } else { 'Red' }
  Write-Host ("<== {0} -> exit {1} ({2:N1}s)" -f $line, $code, $sw.Elapsed.TotalSeconds) -ForegroundColor $color
  $results.Add([pscustomobject]@{ Step = $Name; Command = $line; Exit = $code })
}

$ps = (Get-Process -Id $PID).Path
if (-not $SkipFetch) { Step 'libmpv' $ps @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/fetch-libmpv.ps1') }
if ($Install) { Step 'pnpm install' 'pnpm' @('install', '--frozen-lockfile') }

Step 'rustfmt' 'cargo' @('fmt', '--all', '--', '--check')
Step 'clippy' 'cargo' @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
Step 'cargo test' 'cargo' @('test', '--workspace', '--locked')
Step 'typecheck' 'pnpm' @('-r', '--if-present', 'typecheck')
Step 'eslint' 'pnpm' @('-r', '--if-present', 'lint')
Step 'prettier' 'pnpm' @('-r', '--if-present', 'format:check')
Step 'vitest' 'pnpm' @('-r', '--if-present', 'test')
Step 'frontend build' 'pnpm' @('--filter', 'desktop', 'build')
Step 'secret scan' $ps @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/secret-scan.ps1')

Write-Host ""
Write-Host "verify summary" -ForegroundColor Cyan
$results | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
$failed = @($results | Where-Object { $_.Exit -ne 0 })
if ($failed.Count -gt 0) {
  Write-Host "VERIFY FAILED: $(@($failed | ForEach-Object { $_.Step }) -join ', ')" -ForegroundColor Red
  exit 1
}
Write-Host "VERIFY OK" -ForegroundColor Green
exit 0
