<#
.SYNOPSIS
  Pre-push secret scan. Uses gitleaks if installed; otherwise checks the content that would be pushed
  (commits not yet on origin, plus staged changes) for the values in .env.local and for common token
  patterns. Never prints secret values, only key names and file paths.
.OUTPUTS
  Exit 0 = clean, 1 = possible secret found.
#>
[CmdletBinding()]
param(
  [string]$Range = ''
)
$ErrorActionPreference = 'Stop'
$root = (git rev-parse --show-toplevel).Trim()
Set-Location $root

if (Get-Command gitleaks -ErrorAction SilentlyContinue) {
  gitleaks git --no-banner --redact .
  exit $LASTEXITCODE
}

# Determine the range to scan: everything not yet on origin/main.
if (-not $Range) {
  $null = git rev-parse --verify --quiet origin/main 2>$null
  if ($LASTEXITCODE -eq 0) { $Range = 'origin/main..HEAD' } else { $Range = 'HEAD' }
}

$patches = @()
$patches += (git log -p --no-color $Range 2>$null) -join "`n"
$patches += (git diff --cached --no-color) -join "`n"
$text = $patches -join "`n"

$found = $false

# 1. Exact .env.local values (length >= 6 to avoid trivial matches).
$envFile = Join-Path $root '.env.local'
if (Test-Path $envFile) {
  foreach ($line in Get-Content $envFile) {
    if ($line -match '^\s*#' -or $line -notmatch '=') { continue }
    $k, $v = $line -split '=', 2
    $v = $v.Trim().Trim('"').Trim("'")
    if ($k.Trim() -in @('TEST_MEDIA_DIR')) { continue }
    if ($v.Length -ge 6 -and $text.Contains($v)) {
      Write-Host "SECRET-SCAN: value of $($k.Trim()) from .env.local appears in content to be pushed" -ForegroundColor Red
      $found = $true
    }
  }
}

# 2. Generic token patterns in added lines only.
$added = ($text -split "`n") | Where-Object { $_ -match '^\+' -and $_ -notmatch '^\+\+\+' }
$patterns = @{
  'GitHub token'    = 'gh[pousr]_[A-Za-z0-9]{36,}'
  'AWS key'         = 'AKIA[0-9A-Z]{16}'
  'Private key'     = '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  'Bearer literal'  = '(?i)authorization:\s*bearer\s+[A-Za-z0-9\-_\.]{30,}'
  'JWT'             = 'eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}'
}
foreach ($name in $patterns.Keys) {
  $hits = $added | Where-Object { $_ -match $patterns[$name] }
  if ($hits) {
    Write-Host "SECRET-SCAN: pattern '$name' matched $(@($hits).Count) added line(s)" -ForegroundColor Red
    $found = $true
  }
}

# 3. Forbidden files.
$files = (git diff --cached --name-only) + (git log --name-only --pretty=format: $Range 2>$null)
$bad = $files | Where-Object { $_ -match '(^|/)\.env(\.|$)' -and $_ -notmatch '\.env\.example$' }
if ($bad) { Write-Host "SECRET-SCAN: env file staged/committed: $($bad -join ', ')" -ForegroundColor Red; $found = $true }

if ($found) { Write-Host 'SECRET-SCAN: FAILED'; exit 1 }
Write-Host "SECRET-SCAN: clean (range $Range + staged; gitleaks not installed, built-in scan used)"
exit 0
