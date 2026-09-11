# Runs the godot-e2e visual/behavioural test suite, compares screenshots
# against baselines with ImageMagick, writes per-shot AE metrics to the run
# directory, and prunes old or empty artefact runs.
#
# Usage:
#   powershell -File scripts\run_visual_tests.ps1 [-KeepRuns 5] [-UpdateBaselines] [-SkipMovie]
#
# -KeepRuns N       how many recent artefact run dirs to retain (default 5)
# -UpdateBaselines  copy the latest run's screenshots over the baselines
# -SkipMovie        skip the deterministic movie-frame capture step
#
# Each compared run gets a metrics.txt next to its screenshots recording the
# per-shot AE metric (pixel count differing beyond the 5% fuzz), so visual
# regressions can be quantified after the fact, not just localized.

param(
    [int]$KeepRuns = 5,
    [switch]$UpdateBaselines,
    [switch]$SkipMovie
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $repoRoot

# --- 1. Build the Rust extension -------------------------------------------
cargo build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# --- 2. Run the E2E suite ---------------------------------------------------
$env:GODOT_PATH = if ($env:GODOT_PATH) { $env:GODOT_PATH } else { 'C:\Godot\Godot_v4.7-stable_win64.exe' }
python -m pytest tests/e2e -v
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# --- 3. Locate the newest artefact run -------------------------------------
$artifactsRoot = Join-Path $repoRoot 'artifacts\e2e'
$runDir = Get-ChildItem $artifactsRoot -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending | Select-Object -First 1
if (-not $runDir) {
    Write-Host 'No artefact run directory found.'
    exit 1
}

# --- 4. Compare screenshots against baselines ------------------------------
$magick = (Get-Command magick -ErrorAction SilentlyContinue).Source
if (-not $magick) {
    $magick = Get-ChildItem "$env:ProgramFiles\ImageMagick-*\magick.exe" |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
$baselines = Join-Path $repoRoot 'tests\e2e\baselines'
New-Item -ItemType Directory -Path $baselines -Force | Out-Null

$failures = @()
$metrics = New-Object System.Collections.Generic.List[string]
$metrics.Add("run: $($runDir.Name)")
$metrics.Add("fuzz: 5%")
$metrics.Add("metric: AE (pixel count differing beyond fuzz)")
$metrics.Add("")
# Only canonical (cmp_*) screenshots participate in regression comparison;
# insp_*.png are inspection-only captures (extra angles/zooms) and are never
# gated, so the deterministic comparison set stays small and stable.
foreach ($png in Get-ChildItem $runDir.FullName -Filter "cmp_*.png") {
    $baseline = Join-Path $baselines $png.Name
    if (-not (Test-Path $baseline) -or $UpdateBaselines) {
        # Baselines may be locked by a viewer; try to clear and delete first,
        # then copy. Never fail the run on a locked baseline.
        try {
            if (Test-Path $baseline) {
                (Get-Item $baseline).Attributes = 'Archive'
                Remove-Item $baseline -Force -ErrorAction SilentlyContinue
            }
            Copy-Item $png.FullName $baseline -Force -ErrorAction Stop
            Write-Host "Baseline updated: $($png.Name)"
            $metrics.Add("BASELINE  $($png.Name) (seeded, not compared)")
        } catch {
            Write-Host "Could not update baseline (locked): $($png.Name)"
            $metrics.Add("BASELINE-LOCKED $($png.Name)")
        }
        continue
    }
    $diffPath = Join-Path $runDir.FullName ("diff_" + $png.Name)
    # ImageMagick writes its metric to stderr; route through cmd so PowerShell
    # does not promote the stderr redirect into a terminating error.
    $metric = (cmd /c "`"$magick`" compare -metric AE -fuzz 5% `"$baseline`" `"$($png.FullName)`" `"$diffPath`" 2>&1" |
        Out-String).Trim()
    # compare exits 0 = identical within fuzz, 1 = differences, >=2 = error
    # (e.g. dimension mismatch). Only exit 1 is a visual regression.
    if ($LASTEXITCODE -ge 2) {
        $failures += $png.Name
        Write-Warning "Comparison error: $($png.Name) metric=$metric"
        $metrics.Add("ERROR     $($png.Name) ($metric)")
    } elseif ($LASTEXITCODE -eq 1) {
        $failures += $png.Name
        Write-Warning "Visual regression: $($png.Name) metric=$metric"
        $metrics.Add("FAIL      $($png.Name) AE=$metric")
    } else {
        Write-Host "OK $($png.Name) metric=$metric"
        # IM7 prints the metric as "<AE> (<normalized>)", e.g. "0 (0)".
        # AE=0 means pixel-identical within fuzz: the diff image carries no
        # information, so auto-delete it and keep only actionable artefacts.
        $aeIsZero = $metric -match '^(\d+(?:\.\d+)?)' -and [double]$Matches[1] -eq 0
        if ($aeIsZero -and (Test-Path $diffPath)) {
            Remove-Item $diffPath -Force -ErrorAction SilentlyContinue
            $metrics.Add("OK        $($png.Name) AE=$metric (diff omitted)")
        } else {
            $metrics.Add("OK        $($png.Name) AE=$metric")
        }
    }
}
$metrics.Add("")
$metrics.Add("failures: $($failures.Count)")
$metricsPath = Join-Path $runDir.FullName 'metrics.txt'
$metrics | Set-Content -Path $metricsPath -Encoding UTF8
Write-Host "Metrics written: $metricsPath"

# --- 4b. Deterministic movie capture (dynamic-perception channel) -----------
# Records a short, seeded, time-stepped frame sequence of the animated app so
# particle motion can be reviewed after any change. Non-fatal on failure: the
# screenshot diff gate above remains the authoritative regression check.
if (-not $SkipMovie) {
    Write-Host 'Capturing deterministic movie frames...'
    powershell -File (Join-Path $PSScriptRoot 'capture_movie.ps1') -Seconds 2
    if ($LASTEXITCODE -ne 0) {
        Write-Warning 'Movie capture failed (non-fatal); see output above.'
    }
}

# --- 5. Prune old artefact runs --------------------------------------------
foreach ($artifactsRoot in @(
        (Join-Path $repoRoot 'artifacts\e2e'),
        (Join-Path $repoRoot 'artifacts\movies'))) {
    if (-not (Test-Path $artifactsRoot)) { continue }
    $runs = Get-ChildItem $artifactsRoot -Directory | Sort-Object Name -Descending
    if ($runs.Count -gt $KeepRuns) {
        $runs | Select-Object -Skip $KeepRuns | ForEach-Object {
            # Screenshots may still be held open by a viewer; pruning is
            # best-effort and must never fail the run.
            try {
                Get-ChildItem $_.FullName -File -ErrorAction SilentlyContinue |
                    ForEach-Object { $_.Attributes = 'Archive' }
                Remove-Item $_.FullName -Recurse -Force -ErrorAction SilentlyContinue
            } catch {
                # Ignore - the next run will retry.
            }
            if (Test-Path $_.FullName) {
                Write-Host "Could not prune (locked): $($_.Name)"
            } else {
                Write-Host "Pruned old run: $($_.Name)"
            }
        }
    }
    # Auto-delete empty run dirs: aborted sessions that created their
    # timestamped directory but never produced any artefact. The current
    # run is exempt because metrics.txt is written into it above.
    Get-ChildItem $artifactsRoot -Directory | ForEach-Object {
        $hasFiles = @(Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue)
        if ($hasFiles.Count -eq 0) {
            try {
                Remove-Item $_.FullName -Recurse -Force -ErrorAction SilentlyContinue
            } catch {
                # Ignore - the next run will retry.
            }
            if (-not (Test-Path $_.FullName)) {
                Write-Host "Pruned empty run: $($_.Name)"
            }
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host "Visual regression failures: $($failures -join ', ')"
    exit 1
}
Write-Host 'Visual tests passed.'
