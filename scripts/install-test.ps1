<#
.SYNOPSIS
  Phase 9 local install test: silent per-user install (/S), launch, wait for the "ready" log line, close, silent
  uninstall, then check that nothing but user data is left behind. Prints each check and exits 1 on any failure.
  Runs on Windows PowerShell 5.1 and PowerShell 7. Needs no admin rights (per-user NSIS install).
.PARAMETER Installer
  Path to the NSIS setup exe. Default: the newest target/release/bundle/nsis/*-setup.exe.
.PARAMETER Report
  Optional path of a text file that receives the same check lines.
#>
[CmdletBinding()]
param([string]$Installer, [string]$Report)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$lines = New-Object System.Collections.Generic.List[string]
$failed = 0
function Say([string]$Text) { Write-Host $Text; $lines.Add($Text) }
function Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
  $status = if ($Ok) { 'PASS' } else { 'FAIL' }
  $suffix = if ($Detail) { " ($Detail)" } else { '' }
  Say "$status $Name$suffix"
  if (-not $Ok) { $script:failed++ }
}
function UninstallEntry {
  $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
  if (-not (Test-Path $key)) { return $null }
  Get-ChildItem $key | ForEach-Object { Get-ItemProperty $_.PSPath } |
    Where-Object { $_.PSObject.Properties['DisplayName'] -and $_.DisplayName -eq 'Granary' } | Select-Object -First 1
}
function WaitFor([scriptblock]$Condition, [int]$Seconds) {
  $end = (Get-Date).AddSeconds($Seconds)
  while ((Get-Date) -lt $end) {
    if (& $Condition) { return $true }
    Start-Sleep -Milliseconds 500
  }
  return [bool](& $Condition)
}

if (-not $Installer) {
  $found = Get-ChildItem (Join-Path $root 'target\release\bundle\nsis') -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime | Select-Object -Last 1
  if (-not $found) { throw 'no installer found; run scripts/release-build.ps1 first' }
  $Installer = $found.FullName
}
if (Get-Process granary -ErrorAction SilentlyContinue) { throw 'granary.exe is running; close it first' }
if (UninstallEntry) { throw 'Granary is already installed for this user; uninstall it first so the test starts clean' }
if (Test-Path 'HKCU:\Software\Granary contributors') { throw 'HKCU:\Software\Granary contributors exists from an earlier install; remove it so the test starts clean' }

Say "installer: $Installer"
Say "sha256   : $((Get-FileHash -Algorithm SHA256 $Installer).Hash.ToLowerInvariant())"
$userData = @((Join-Path $env:APPDATA 'Granary'), (Join-Path $env:LOCALAPPDATA 'Granary'))
$before = @{}
foreach ($d in $userData) {
  $before[$d] = @(if (Test-Path $d) { Get-ChildItem $d -Recurse -Force | ForEach-Object FullName })
}

# ---- install ----
$p = Start-Process -FilePath $Installer -ArgumentList '/S' -PassThru -Wait
Check 'silent install exits 0' ($p.ExitCode -eq 0) "exit $($p.ExitCode)"
$entry = UninstallEntry
Check 'uninstall entry registered under HKCU' ($null -ne $entry)
# The installer writes this key; seeing it here proves the "left behind" check after uninstall can fail.
Check 'install-location key written under HKCU\Software\Granary contributors' (Test-Path 'HKCU:\Software\Granary contributors\Granary')
if (-not $entry) { throw 'install failed; stopping' }
$uninstaller = ($entry.UninstallString -replace '"', '').Trim()
$installDir = Split-Path -Parent $uninstaller
Say "install dir: $installDir"
Check 'per-user install location (no admin)' ($installDir.StartsWith($env:LOCALAPPDATA, [StringComparison]::OrdinalIgnoreCase)) $installDir
$expected = @('granary.exe', 'libmpv-2.dll', 'mpv.conf', 'LICENSE.txt', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_LICENSES.txt')
foreach ($f in $expected) { Check "installed $f" (Test-Path (Join-Path $installDir $f)) }
# Only what the installer added: the per-user install dir is also the app's local data folder (logs, caches).
$preexisting = @($before.Values | ForEach-Object { $_ })
$installed = @(Get-ChildItem $installDir -Recurse -Force -File | ForEach-Object FullName |
    Where-Object { $preexisting -notcontains $_ })
Say "installed files: $($installed.Count) ($((($installed | ForEach-Object { Split-Path -Leaf $_ }) -join ', ')))"
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Granary.lnk'
Check 'start menu shortcut' (Test-Path $shortcut)

# ---- launch ----
$logDir = Join-Path $env:LOCALAPPDATA 'Granary\logs'
$started = Get-Date
$app = Start-Process -FilePath (Join-Path $installDir 'granary.exe') -PassThru
$readyLine = $null
$ready = WaitFor {
  # No LastWriteTime filter: Windows updates it lazily while the app holds the log open; the line timestamps decide.
  $files = Get-ChildItem $logDir -Filter '*.log*' -ErrorAction SilentlyContinue
  foreach ($f in $files) {
    $hit = Select-String -Path $f.FullName -Pattern 'ready cold_start_ms=\d+' -ErrorAction SilentlyContinue |
      Where-Object {
        $t = [datetime]::MinValue
        $_.Line.Length -ge 27 -and [datetime]::TryParse($_.Line.Substring(0, 27), [ref]$t) -and $t.ToLocalTime() -ge $started.AddSeconds(-1)
      } |
      Select-Object -Last 1
    if ($hit) { $script:readyLine = $hit.Line; return $true }
  }
  return $false
} 30
Check 'app starts and logs ready' $ready $(if ($readyLine) { $readyLine.Trim() } else { 'no ready line within 30 s' })
$title = (Get-Process -Id $app.Id -ErrorAction SilentlyContinue).MainWindowTitle
Check 'window title has no trademark' ($title -notmatch 'Silo') "title '$title'"
$null = $app.CloseMainWindow()
$exited = WaitFor { $app.HasExited } 15
if (-not $exited) { Stop-Process -Id $app.Id -Force }
Check 'app closes on window close' $exited

# ---- uninstall ----
$u = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru -Wait
Check 'silent uninstall starts' ($u.ExitCode -eq 0) "exit $($u.ExitCode)"
# The NSIS uninstaller copies itself to %TEMP% and returns at once; wait for the real work to finish.
$gone = WaitFor { -not (Test-Path (Join-Path $installDir 'granary.exe')) -and -not (UninstallEntry) } 60
Check 'uninstall entry and program removed' $gone
Start-Sleep -Seconds 2
$left = @($installed | Where-Object { Test-Path $_ })
Check 'no installed file left behind' ($left.Count -eq 0) $(if ($left.Count) { $left -join ', ' } else { "$($installed.Count) files removed" })
Check 'start menu shortcut removed' (-not (Test-Path $shortcut))
Check 'no registry key left under HKCU\Software\Granary contributors' (-not (Test-Path 'HKCU:\Software\Granary contributors'))
Check 'desktop shortcut absent' (-not (Test-Path (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Granary.lnk')))
foreach ($d in $userData) {
  $now = @(if (Test-Path $d) { Get-ChildItem $d -Recurse -Force | ForEach-Object FullName })
  $kept = @($before[$d] | Where-Object { Test-Path $_ }).Count
  Say "user data kept: $d ($kept of $($before[$d].Count) earlier entries, $($now.Count) now)"
}

Say ''
if ($failed -eq 0) { Say 'INSTALL TEST PASSED' } else { Say "INSTALL TEST FAILED ($failed checks)" }
if ($Report) { [IO.File]::WriteAllLines($Report, $lines) }
if ($failed -eq 0) { exit 0 } else { exit 1 }
