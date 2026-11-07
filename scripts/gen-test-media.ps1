<#
.SYNOPSIS
  Generate synthetic test fixtures into test-media/ (gitignored) with ffmpeg, -Duration seconds long (default 60:
  long enough for periodic progress samples and seeking). Idempotent: existing files are
  kept unless -Force. If TEST_MEDIA_DIR is set (env or .env.local), its files are probed (never copied).
#>
[CmdletBinding()]
param([switch]$Force, [int]$Duration = 60)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root 'test-media'
New-Item -ItemType Directory -Force $out | Out-Null

if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw 'ffmpeg not found on PATH' }

function Invoke-FF([string]$name, [string[]]$ffargs) {
  $dest = Join-Path $out $name
  if ((Test-Path $dest) -and -not $Force) { Write-Host "skip  $name (exists)"; return }
  Write-Host "make  $name"
  & ffmpeg -hide_banner -loglevel error -y @ffargs $dest
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $name (exit $LASTEXITCODE)" }
}

# ASS subtitle (external file, also muxed below).
$ass = Join-Path $out 'subs.en.ass'
if (-not (Test-Path $ass) -or $Force) {
  @'
[Script Info]
Title: Granary test subtitles
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,64,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,3,1,2,40,40,60,1
Style: Sign,Arial,48,&H0000FFFF,&H000000FF,&H00000000,&H64000000,1,0,0,0,100,100,0,0,1,2,0,8,40,40,40,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.50,0:00:03.00,Default,,0,0,0,,Granary test subtitle one
Dialogue: 0,0:00:03.50,0:00:06.00,Default,,0,0,0,,{\i1}Italic line two{\i0}
Dialogue: 0,0:00:06.50,0:00:09.50,Sign,,0,0,0,,{\pos(960,120)}Positioned sign (ASS styling)
'@ | Set-Content -Encoding utf8 $ass
  Write-Host 'make  subs.en.ass'
}

$dur = [string]$Duration

# 1. SDR H.264 1080p at 25 fps (23.976 is a separate clip).
#    Audio: AAC stereo + AC3 5.1 + E-AC3 5.1; ASS muxed.
Invoke-FF 'sdr_h264_1080p.mkv' @(
  '-f','lavfi','-i',"testsrc2=size=1920x1080:rate=25:duration=$dur",
  '-f','lavfi','-i',"sine=frequency=440:duration=$dur",
  '-f','lavfi','-i',"sine=frequency=330:duration=$dur",
  '-f','lavfi','-i',"sine=frequency=550:duration=$dur",
  '-i',$ass,
  '-map','0:v','-map','1:a','-map','2:a','-map','3:a','-map','4:s',
  '-vf','setparams=range=tv:color_primaries=bt709:color_trc=bt709:colorspace=bt709','-c:v','libx264','-preset','veryfast','-pix_fmt','yuv420p','-color_primaries','bt709','-color_trc','bt709','-colorspace','bt709',
  '-c:a:0','aac','-b:a:0','160k','-ac:a:0','2',
  '-c:a:1','ac3','-b:a:1','448k','-ac:a:1','6',
  '-c:a:2','eac3','-b:a:2','640k','-ac:a:2','6',
  '-c:s','ass',
  '-metadata:s:a:0','language=eng','-metadata:s:a:0','title=Stereo AAC',
  '-metadata:s:a:1','language=eng','-metadata:s:a:1','title=5.1 AC3',
  '-metadata:s:a:2','language=deu','-metadata:s:a:2','title=5.1 E-AC3',
  '-metadata:s:s:0','language=eng'
)

# 2. HEVC Main10 HDR10 2160p with mastering display + MaxCLL/MaxFALL metadata.
#    Bright moving pattern so highlights exceed SDR range after PQ encoding.
$x265 = 'colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:range=limited:hdr10=1:hdr10-opt=1:repeat-headers=1:' +
        'master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,50):max-cll=1000,400:log-level=error'
Invoke-FF 'hdr10_hevc_main10_2160p.mkv' @(
  '-f','lavfi','-i',"testsrc2=size=3840x2160:rate=24000/1001:duration=$dur",
  '-f','lavfi','-i',"sine=frequency=440:duration=$dur",
  '-map','0:v','-map','1:a',
  '-vf','format=yuv420p10le,setparams=range=tv:color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc',
  '-c:v','libx265','-preset','ultrafast','-x265-params',$x265,
  '-color_primaries','bt2020','-color_trc','smpte2084','-colorspace','bt2020nc','-color_range','tv',
  '-c:a','eac3','-b:a','640k','-ac','6'
)

# 3. 23.976 fps clip (refresh-rate matching tests).
Invoke-FF 'sdr_h264_23976.mkv' @(
  '-f','lavfi','-i',"testsrc2=size=1920x1080:rate=24000/1001:duration=$dur",
  '-f','lavfi','-i',"sine=frequency=440:duration=$dur",
  '-map','0:v','-map','1:a',
  '-vf','setparams=range=tv:color_primaries=bt709:color_trc=bt709:colorspace=bt709','-c:v','libx264','-preset','veryfast','-pix_fmt','yuv420p',
  '-c:a','ac3','-b:a','448k','-ac','6'
)

# 4./5. Near-black ramps (MANUAL_TEST_CHECKLIST A.14): left = black, right ~10 nits (PQ) / ~10 % (SDR); smooth top
#    half, 32 steps in the bottom half. Raised blacks show as a grey left edge, banding as visible steps in the top half.
Invoke-FF 'hdr10_near_black_ramp.mkv' @(
  '-f','lavfi','-i','color=c=black:size=1920x1080:rate=24:duration=20',
  '-vf',"format=yuv420p10le,geq=lum='64+if(lt(Y,H/2),X/W*276,floor(X/W*32)*276/32)':cb=512:cr=512,setparams=range=tv:color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc",
  '-c:v','libx265','-preset','ultrafast','-x265-params',$x265,
  '-color_primaries','bt2020','-color_trc','smpte2084','-colorspace','bt2020nc','-color_range','tv'
)
Invoke-FF 'sdr_near_black_ramp.mkv' @(
  '-f','lavfi','-i','color=c=black:size=1920x1080:rate=24:duration=20',
  '-vf',"format=yuv420p10le,geq=lum='64+if(lt(Y,H/2),X/W*88,floor(X/W*32)*88/32)':cb=512:cr=512,setparams=range=tv:color_primaries=bt709:color_trc=bt709:colorspace=bt709",
  '-c:v','libx265','-preset','ultrafast','-x265-params','log-level=error',
  '-color_primaries','bt709','-color_trc','bt709','-colorspace','bt709','-color_range','tv'
)

# Probe summary.
Get-ChildItem $out -Filter *.mkv | ForEach-Object {
  $p = & ffprobe -v error -show_entries 'stream=index,codec_type,codec_name,profile,width,height,r_frame_rate,pix_fmt,color_transfer,color_primaries,channels' -of compact=p=0:nk=0 $_.FullName
  Write-Host "--- $($_.Name)"; $p | ForEach-Object { Write-Host "    $_" }
}

# Optional: probe user's TEST_MEDIA_DIR (never copied).
$tmd = $env:TEST_MEDIA_DIR
$envFile = Join-Path $root '.env.local'
if (-not $tmd -and (Test-Path $envFile)) {
  $line = Get-Content $envFile | Where-Object { $_ -match '^\s*TEST_MEDIA_DIR\s*=' } | Select-Object -First 1
  if ($line) { $tmd = ($line -split '=', 2)[1].Trim().Trim('"') }
}
if ($tmd -and (Test-Path $tmd)) {
  Write-Host "Probing TEST_MEDIA_DIR files (not copied):"
  Get-ChildItem $tmd -File | ForEach-Object {
    $p = & ffprobe -v error -show_entries 'stream=codec_type,codec_name,profile,color_transfer:stream_side_data=dv_profile' -of compact=p=0:nk=0 $_.FullName
    Write-Host "--- $($_.Name)"; $p | ForEach-Object { Write-Host "    $_" }
  }
} else { Write-Host 'TEST_MEDIA_DIR not set or missing; only synthetic fixtures available.' }
exit 0
