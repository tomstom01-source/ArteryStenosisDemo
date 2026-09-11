# Captures a deterministic, time-stepped PNG frame sequence ("movie") of the
# artery simulation using Godot's Movie Maker mode, for visual review of
# particle motion that single screenshots cannot convey.
#
# Usage:
#   powershell -File scripts\capture_movie.ps1 [-Plaque 80] [-Lifestyle 2] `
#               [-Seconds 4] [-Fps 30] [-Width 1280] [-Height 720] `
#               [-Seed 1337] [-OutDir path] [-Gif] [-GifSampleEvery 2]
#
# Determinism: DEVIN_PARTICLE_SEED fixes cell placement, Movie Maker's
# --fixed-fps fixes the timestep, and DEVIN_INITIAL_PLAQUE / LIFESTYLE fix
# the starting slider state (applied by scripts/ui.gd). Particle motion is
# ENABLED here - unlike the screenshot suite - so the frames show flow.
#
# Exclusive-source note: only the parameters you actually pass are forwarded
# to the UI, and the last one passed becomes the exclusive plaque source.
#   .\capture_movie.ps1 -Plaque 100            -> direct plaque source at 100%
#   .\capture_movie.ps1 -Smoking 20 -SmokingYears 10   -> smoking source
# Passing several parameters makes the LAST one the active source.

param(
    [double]$Plaque = 30.0,
    [double]$Lifestyle = 0.0,
    [double]$Smoking = 0.0,
    [double]$SmokingYears = 10.0,
    [double]$Seconds = 4.0,
    [int]$Fps = 30,
    [int]$Width = 1280,
    [int]$Height = 720,
    [string]$Seed = "1337",
    [string]$OutDir = "",
    [switch]$Gif,
    [int]$GifSampleEvery = 2
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $repoRoot

# --- 1. Build the Rust extension -------------------------------------------
cargo build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# --- 2. Locate Godot ---------------------------------------------------------
$godot = if ($env:GODOT_PATH) { $env:GODOT_PATH } else { 'C:\Godot\Godot_v4.7-stable_win64.exe' }
if (-not (Test-Path $godot)) {
    Write-Error "Godot not found at '$godot'. Set GODOT_PATH or install Godot 4.x."
    exit 1
}

# --- 3. Prepare output directory ---------------------------------------------
if ($OutDir -eq "") {
    $OutDir = Join-Path $repoRoot ("artifacts\movies\" + (Get-Date -Format 'yyyyMMdd_HHmmss'))
}
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
$frames = [int][math]::Ceiling($Seconds * $Fps)

# --- 4. Run Godot in Movie Maker mode ----------------------------------------
# Env vars are scoped to this run and always cleaned up, so a subsequent
# pytest run in the same shell is not affected by leaked state.
#
# Only parameters explicitly passed on the command line are exported. The UI
# (`scripts/ui.gd`) makes the *last* valid `DEVIN_INITIAL_*` variable the
# exclusive plaque source, so exporting unset defaults (e.g. SmokingYears=10)
# would silently override a requested -Plaque/-Lifestyle state with the
# smoking source (which contributes 0 when Smoking=0).
$env:DEVIN_PARTICLE_SEED = $Seed
foreach ($bound in $PSBoundParameters.GetEnumerator()) {
    switch ($bound.Key) {
        'Plaque'       { $env:DEVIN_INITIAL_PLAQUE = "$Plaque" }
        'Lifestyle'    { $env:DEVIN_INITIAL_LIFESTYLE = "$Lifestyle" }
        'Smoking'      { $env:DEVIN_INITIAL_SMOKING = "$Smoking" }
        'SmokingYears' { $env:DEVIN_INITIAL_SMOKING_YEARS = "$SmokingYears" }
    }
}
Remove-Item Env:\DEVIN_FREEZE_PARTICLES -ErrorAction SilentlyContinue

$movieBase = Join-Path $OutDir 'frame.png'
Write-Host "Capturing $frames frames at ${Width}x${Height} @ ${Fps}fps (plaque=$Plaque, lifestyle=$Lifestyle, smoking=$Smoking, smoking-years=$SmokingYears, seed=$Seed)..."
try {
    # Start-Process (not the call operator) so PowerShell actually WAITS for
    # the Godot executable: Godot's win64.exe is a GUI-subsystem binary and
    # the call operator returns immediately, racing the frame-count check.
    $godotArgs = @(
        '--path', (Join-Path $repoRoot 'godot'),
        '--windowed',
        '--resolution', "${Width}x${Height}",
        '--write-movie', $movieBase,
        '--fixed-fps', "$Fps",
        '--quit-after', "$frames"
    )
    $proc = Start-Process -FilePath $godot -ArgumentList $godotArgs -Wait -PassThru -NoNewWindow
    if ($proc.ExitCode -ne 0) { exit $proc.ExitCode }
}
finally {
    Remove-Item Env:DEVIN_PARTICLE_SEED -ErrorAction SilentlyContinue
    Remove-Item Env:DEVIN_INITIAL_PLAQUE -ErrorAction SilentlyContinue
    Remove-Item Env:DEVIN_INITIAL_LIFESTYLE -ErrorAction SilentlyContinue
    Remove-Item Env:DEVIN_INITIAL_SMOKING -ErrorAction SilentlyContinue
    Remove-Item Env:DEVIN_INITIAL_SMOKING_YEARS -ErrorAction SilentlyContinue
}

# --- 5. Verify and summarize --------------------------------------------------
$pngs = @(Get-ChildItem $OutDir -Filter 'frame*.png' | Sort-Object Name)
Write-Host "Captured $($pngs.Count) frames (expected $frames) -> $OutDir"
if ($pngs.Count -eq 0) {
    Write-Error 'No frames were written; check the Godot output above.'
    exit 1
}

if ($Gif) {
    $magick = (Get-Command magick -ErrorAction SilentlyContinue).Source
    if (-not $magick) {
        $magick = Get-ChildItem "$env:ProgramFiles\ImageMagick-*\magick.exe" -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending |
            Select-Object -First 1 -ExpandProperty FullName
    }
    if ($magick) {
        $delay = [math]::Max(1, [math]::Round(100.0 / $Fps * $GifSampleEvery))
        $gifPath = Join-Path $OutDir 'artery_movie.gif'
        cmd /c "`"$magick`" -delay $delay -loop 0 `"$OutDir\frame*.png`" `"$OutDir\artery_movie.gif`"" | Out-Null
        if (Test-Path (Join-Path $OutDir 'artery_movie.gif')) {
            Write-Host "GIF assembled (every $GifSampleEvery frame(s)): artery_movie.gif"
        } else {
            Write-Warning 'GIF assembly failed; PNG frames remain available.'
        }
    } else {
        Write-Host 'ImageMagick not found; skipping GIF assembly (PNG frames are still usable).'
    }
}

Write-Host "Movie capture complete: $OutDir"
