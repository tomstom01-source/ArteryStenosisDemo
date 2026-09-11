# Builds the web (Wasm) export of the artery visualization.
#
# Prerequisites (one-time):
#   - Emscripten SDK 3.1.74 installed (default: %USERPROFILE%\emsdk; set EMSDK_PATH
#     to override) with emcc available.
#   - Rust nightly-2026-05-01 with rust-src and the wasm32-unknown-emscripten target.
#   - Godot 4.7 (GODOT_PATH env var or 'C:\Godot\Godot_v4.7-stable_win64.exe').
#   - Godot 4.7 web export templates in %APPDATA%\Godot\export_templates\4.7.stable.
#
# Usage:
#   powershell -File scripts\build_web.ps1          # build + export to build\web\
#
# The output is a static folder (build\web) hostable on any static web server
# (no Cross-Origin Isolation headers needed - the build is single-threaded).

param(
    [string]$GodotPath = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $repoRoot

$godot = if ($env:GODOT_PATH) { $env:GODOT_PATH } elseif ($GodotPath) { $GodotPath } else { 'C:\Godot\Godot_v4.7-stable_win64.exe' }

# --- 1. Toolchain environment -------------------------------------------------
$emsdkPath = if ($env:EMSDK_PATH) { $env:EMSDK_PATH } else { Join-Path $env:USERPROFILE 'emsdk' }
$emcc = Join-Path $emsdkPath 'upstream\emscripten\emcc.bat'
if (-not (Test-Path $emcc)) {
    Write-Error "emcc not found under $emsdkPath; install emsdk 3.1.74 or set EMSDK_PATH."
    exit 1
}
$env:PATH = (Join-Path $emsdkPath 'upstream\emscripten') + ";$env:PATH"

# --- 2. Compile the Rust GDExtension to Wasm (nothreads side module) ----------
# Pinned nightly: -Zemscripten-wasm-eh (needed to opt out of the Godot-
# incompatible wasm EH ABI) was removed in newer nightlies (rust#156928).
$toolchain = 'nightly-2026-05-01'
Push-Location "$repoRoot\rust"
try {
    $env:GODOT4_BIN = $godot
    cargo +$toolchain build --release --features web -Zbuild-std --target wasm32-unknown-emscripten
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Remove-Item Env:\GODOT4_BIN -ErrorAction SilentlyContinue
}
Pop-Location

# --- 3. Export the Godot project ----------------------------------------------
New-Item -ItemType Directory -Force -Path "$repoRoot\build\web" | Out-Null
& $godot --headless --path (Join-Path $repoRoot 'godot') --export-release "Web" ../build/web/index.html
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Output "Web build complete: build\web\index.html (serve the folder over HTTP to play)."
