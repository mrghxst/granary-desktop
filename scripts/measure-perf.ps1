<#
.SYNOPSIS
  Measure Granary cold start and idle memory (Phase 8, docs/perf.md).

.DESCRIPTION
  Launches the given granary.exe -Runs times. For each run it:
  - records the wall-clock time from process start until the app logs `ready` (the first screen is interactive);
  - reads the host's own `cold_start_ms` from the same line;
  - waits -IdleSeconds, then sums the working set and private bytes of granary.exe and its WebView2 child processes;
  - closes the window and waits for a clean exit.
  The first run is a "cold" run (OS file cache may be warm from the build); all runs are reported.
  Results go to docs/evidence/phase-8/perf.json and are printed as a table. Idempotent; runs on PowerShell 5.1 and 7.

.EXAMPLE
  ./scripts/measure-perf.ps1 -Exe target/release/granary.exe -Runs 5
#>
param(
  [string]$Exe = "target/release/granary.exe",
  [int]$Runs = 5,
  [int]$IdleSeconds = 30,
  [string]$Out = "docs/evidence/phase-8/perf.json"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$exePath = (Resolve-Path $Exe).Path
$logDir = Join-Path $env:LOCALAPPDATA "Granary\logs"

if (Get-Process granary -ErrorAction SilentlyContinue) {
  throw "Granary is already running; close it first (single instance)."
}

function Get-Tree([int]$rootPid) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name
  $ids = New-Object System.Collections.Generic.List[int]
  $ids.Add($rootPid)
  $i = 0
  while ($i -lt $ids.Count) {
    $p = $ids[$i]
    foreach ($c in $all) { if ($c.ParentProcessId -eq $p -and -not $ids.Contains([int]$c.ProcessId)) { $ids.Add([int]$c.ProcessId) } }
    $i++
  }
  return $ids
}

function Get-LogLength {
  $f = Get-ChildItem $logDir -Filter "granary*.log" -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
  # The directory entry's size lags while the app holds the file open; read the length from an open handle.
  if ($f) {
    $fs = [System.IO.File]::Open($f.FullName, 'Open', 'Read', 'ReadWrite')
    try { return @{ Path = $f.FullName; Length = $fs.Length } } finally { $fs.Dispose() }
  }
  return @{ Path = $null; Length = 0 }
}

$results = @()
for ($run = 1; $run -le $Runs; $run++) {
  $before = Get-LogLength
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $proc = Start-Process -FilePath $exePath -PassThru
  $ready = $null
  $coldMs = $null
  while ($sw.Elapsed.TotalSeconds -lt 30 -and -not $ready) {
    Start-Sleep -Milliseconds 50
    $now = Get-LogLength
    if (-not $now.Path) { continue }
    $start = if ($now.Path -eq $before.Path) { $before.Length } else { 0 }
    if ($now.Length -le $start) { continue }
    $fs = [System.IO.File]::Open($now.Path, 'Open', 'Read', 'ReadWrite')
    try {
      $null = $fs.Seek($start, 'Begin')
      $text = (New-Object System.IO.StreamReader($fs)).ReadToEnd()
    } finally { $fs.Dispose() }
    $m = [regex]::Match($text, 'ready cold_start_ms=(\d+)')
    if ($m.Success) { $ready = $sw.ElapsedMilliseconds; $coldMs = [int]$m.Groups[1].Value }
  }
  if (-not $ready) {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    throw "run ${run}: no 'ready' log line within 30 s"
  }
  Start-Sleep -Seconds $IdleSeconds
  $tree = Get-Tree $proc.Id
  $ps = Get-Process -Id $tree -ErrorAction SilentlyContinue
  $ws = ($ps | Measure-Object WorkingSet64 -Sum).Sum
  $priv = ($ps | Measure-Object PrivateMemorySize64 -Sum).Sum
  $hostWs = (Get-Process -Id $proc.Id).WorkingSet64
  $null = $proc.CloseMainWindow()
  if (-not $proc.WaitForExit(15000)) { Stop-Process -Id $proc.Id -Force; throw "run ${run}: did not exit after close" }
  Start-Sleep -Seconds 2
  $r = [ordered]@{
    run = $run
    readyWallMs = $ready
    coldStartMs = $coldMs
    processes = $ps.Count
    workingSetMiB = [math]::Round($ws / 1MB, 1)
    privateMiB = [math]::Round($priv / 1MB, 1)
    hostWorkingSetMiB = [math]::Round($hostWs / 1MB, 1)
  }
  $results += New-Object PSObject -Property $r
  Write-Host ("run {0}: ready {1} ms (host {2} ms), idle {3} MiB working set / {4} MiB private over {5} processes (granary.exe {6} MiB)" -f $run, $ready, $coldMs, $r.workingSetMiB, $r.privateMiB, $r.processes, $r.hostWorkingSetMiB)
}

$median = { param($xs) $s = @($xs | Sort-Object); $s[[int][math]::Floor(($s.Count - 1) / 2)] }
$summary = [ordered]@{
  exe = $Exe
  measuredAt = (Get-Date).ToString("s")
  runs = $results
  medianReadyWallMs = & $median ($results | ForEach-Object { $_.readyWallMs })
  medianColdStartMs = & $median ($results | ForEach-Object { $_.coldStartMs })
  medianWorkingSetMiB = & $median ($results | ForEach-Object { $_.workingSetMiB })
  medianPrivateMiB = & $median ($results | ForEach-Object { $_.privateMiB })
}
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
($summary | ConvertTo-Json -Depth 4) | Out-File -Encoding utf8 $Out
Write-Host ("PERF: median ready {0} ms wall ({1} ms host), idle {2} MiB working set, {3} MiB private -> {4}" -f $summary.medianReadyWallMs, $summary.medianColdStartMs, $summary.medianWorkingSetMiB, $summary.medianPrivateMiB, $Out)
