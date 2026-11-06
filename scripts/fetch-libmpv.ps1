<#
.SYNOPSIS
  Download the pinned libmpv dev build (shinchiro/mpv-winbuild-cmake), verify its SHA-256, extract it to
  vendor-bin/libmpv/ and generate an MSVC import library (mpv.lib) from the bundled .def file.
  Idempotent: does nothing if vendor-bin/libmpv/.pinned matches the pinned asset and hash.
.NOTES
  Pin values live in scripts/libmpv.pin.json (single source of truth, also read by CI and THIRD_PARTY_NOTICES).
#>
[CmdletBinding()]
param([switch]$Force)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = Split-Path -Parent $PSScriptRoot
$pin = Get-Content (Join-Path $PSScriptRoot 'libmpv.pin.json') -Raw | ConvertFrom-Json
$dest = Join-Path $root 'vendor-bin\libmpv'
$marker = Join-Path $dest '.pinned'
$expectedMarker = "$($pin.asset) $($pin.sha256)"

if (-not $Force -and (Test-Path $marker) -and ((Get-Content $marker -Raw).Trim() -eq $expectedMarker) -and
    (Test-Path (Join-Path $dest 'libmpv-2.dll')) -and (Test-Path (Join-Path $dest 'mpv.lib'))) {
  Write-Host "libmpv already present: $($pin.asset)"
  exit 0
}

$dl = Join-Path $root 'vendor-bin\download'
New-Item -ItemType Directory -Force $dl | Out-Null
$archive = Join-Path $dl $pin.asset
$url = "https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/$($pin.tag)/$($pin.asset)"

if (-not (Test-Path $archive) -or $Force) {
  Write-Host "Downloading $url"
  Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing
}

$actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
if ($actual -ne $pin.sha256.ToLowerInvariant()) {
  Remove-Item $archive -Force
  throw "SHA-256 mismatch for $($pin.asset): expected $($pin.sha256), got $actual (archive deleted)"
}
Write-Host "SHA-256 OK: $actual"

# Locate 7-Zip.
$sevenZip = (Get-Command 7z -ErrorAction SilentlyContinue).Source
if (-not $sevenZip) {
  foreach ($c in @("$env:ProgramFiles\7-Zip\7z.exe", "${env:ProgramFiles(x86)}\7-Zip\7z.exe")) { if (Test-Path $c) { $sevenZip = $c; break } }
}
if (-not $sevenZip) { throw '7-Zip (7z.exe) not found' }

if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
New-Item -ItemType Directory -Force $dest | Out-Null
& $sevenZip x -y "-o$dest" $archive | Out-Null
if ($LASTEXITCODE -ne 0) { throw "7z extraction failed (exit $LASTEXITCODE)" }

$dll = Get-ChildItem $dest -Recurse -Filter 'libmpv-2.dll' | Select-Object -First 1
if (-not $dll) { throw 'libmpv-2.dll not found in archive' }
$def = Get-ChildItem $dest -Recurse -Include 'mpv.def', 'libmpv.def' | Select-Object -First 1

# Generate MSVC import library from the .def file (or from DLL exports if no .def is shipped).
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$libExe = Get-ChildItem (Join-Path $vsPath 'VC\Tools\MSVC') -Recurse -Filter lib.exe |
  Where-Object { $_.FullName -match '\\bin\\Hostx64\\x64\\' } | Select-Object -First 1
if (-not $libExe) { throw 'lib.exe (MSVC x64) not found' }

if (-not $def) {
  $dumpbin = Join-Path $libExe.DirectoryName 'dumpbin.exe'
  $exports = & $dumpbin /nologo /exports $dll.FullName | ForEach-Object {
    if ($_ -match '^\s+\d+\s+[0-9A-F]+\s+[0-9A-F]+\s+(mpv_\w+)') { $Matches[1] }
  }
  $def = Join-Path $dest 'mpv.def'
  (@('EXPORTS') + $exports) | Set-Content -Encoding ascii $def
  $def = Get-Item $def
}
$lib = Join-Path $dest 'mpv.lib'
& $libExe.FullName /nologo /machine:x64 "/def:$($def.FullName)" "/name:libmpv-2.dll" "/out:$lib" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "lib.exe failed (exit $LASTEXITCODE)" }

# Flatten: keep DLL + lib + headers at the top of vendor-bin/libmpv.
if ($dll.DirectoryName -ne $dest) { Copy-Item $dll.FullName $dest -Force }
Set-Content -Encoding ascii $marker $expectedMarker
Write-Host "libmpv ready: $dest (libmpv-2.dll, mpv.lib, include/)"
exit 0
