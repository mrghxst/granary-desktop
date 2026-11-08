<#
.SYNOPSIS
  Pixel check that the video is really visible under the transparent overlay (0.1.0 user report: with the overlay
  shown, the window showed the desktop behind it instead of the picture).
.DESCRIPTION
  Captures the composed screen (DWM output, like a screenshot) inside the client area of the running Granary
  window, samples one row at the given height and counts saturated colour families (6 hue buckets of 60 degrees).
  The test clips are colour bars, so a visible picture gives several families; the desktop or a black window gives
  0-1. Prints one JSON line: { buckets, samples, saturated, width, height }. Exit 0 always; the caller judges.
  The window must not be covered by another window (it is brought to the front, but topmost windows stay above).
  Runs on Windows PowerShell 5.1 and PowerShell 7.
.PARAMETER Row
  Sample row as a fraction of the client height (default 0.45, between the overlay's top and bottom bars).
.PARAMETER Out
  Optional PNG path for the captured client area.
#>
[CmdletBinding()]
param([double]$Row = 0.45, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class GranaryPix {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
[void][GranaryPix]::SetProcessDPIAware()
$proc = Get-Process granary -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { throw 'no Granary window' }
$h = $proc.MainWindowHandle
[void][GranaryPix]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 400
$rc = New-Object GranaryPix+RECT
[void][GranaryPix]::GetClientRect($h, [ref]$rc)
$pt = New-Object GranaryPix+POINT
[void][GranaryPix]::ClientToScreen($h, [ref]$pt)
$w = $rc.Right - $rc.Left
$ht = $rc.Bottom - $rc.Top
$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($pt.X, $pt.Y, 0, 0, (New-Object System.Drawing.Size $w, $ht))
$g.Dispose()
if ($Out) { $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png) }
$y = [int]($ht * $Row)
$buckets = @{}
$samples = 0
$saturated = 0
for ($x = [int]($w * 0.02); $x -lt [int]($w * 0.98); $x += 4) {
  $c = $bmp.GetPixel($x, $y)
  $samples++
  if ($c.GetSaturation() -ge 0.5 -and $c.GetBrightness() -ge 0.2 -and $c.GetBrightness() -le 0.85) {
    $saturated++
    $buckets[[int][math]::Floor((($c.GetHue() + 30) % 360) / 60)] = $true
  }
}
$bmp.Dispose()
[pscustomobject]@{ buckets = $buckets.Count; samples = $samples; saturated = $saturated; width = $w; height = $ht } |
  ConvertTo-Json -Compress
exit 0
