# Build Among Azeroth for the browser into target\among-web\dist, the steps of upstream's
# scripts/web-build.sh for our entry crate: the wasm build, wasm-bindgen named `wenilla` so
# upstream's unchanged web\index.html loads it, the page files, wasm-opt -O3, and .gz siblings.
# Setup first: crates\among-web\web-setup.ps1. Usage: web-build.ps1 [-Backend webgpu|webgl2] [-DebugSymbols]
param(
    [ValidateSet('webgpu', 'webgl2')] [string] $Backend = 'webgpu',
    [switch] $DebugSymbols
)
$ErrorActionPreference = 'Continue'
function Assert-Ok($what) { if ($LASTEXITCODE -ne 0) { throw "$what failed ($LASTEXITCODE)" } }
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
Set-Location $root

$env:WASI_SDK = Join-Path $root 'tools\wasi-sdk'
if (-not (Test-Path (Join-Path $env:WASI_SDK 'share\wasi-sysroot\lib\wasm32-wasip1\libsetjmp.a'))) {
    throw 'wasi-sdk missing: run crates\among-web\web-setup.ps1 first'
}
$dist = Join-Path $root 'target\among-web\dist'
$wasm = Join-Path $root 'target\wasm32-unknown-unknown\release\among_web.wasm'

cargo build --release --target wasm32-unknown-unknown -p among-web --features $Backend
Assert-Ok 'cargo build'

New-Item -ItemType Directory -Force $dist | Out-Null
# The name section is half the file and only feeds stack-trace symbols.
$strip = @('--remove-name-section', '--remove-producers-section')
if ($DebugSymbols) { $strip = @() }
wasm-bindgen --target web --no-typescript @strip --out-name wenilla --out-dir $dist $wasm
Assert-Ok 'wasm-bindgen'

foreach ($f in 'index.html', 'wasi_stubs.js', 'boot.js', 'platform.js', 'bridge.js', 'boot-manifest.json', 'world-manifest.json') {
    $src = Join-Path 'web' $f
    if (Test-Path $src) { Copy-Item $src $dist }
}
# The world-entry prefetch also warms Northshire (read-set.ps1's cut of a request log): its
# terrain, buildings, doodads and textures, which the client otherwise fetches one at a time
# under the loading screen.
$readSet = Join-Path $PSScriptRoot 'northshire-manifest.json'
$worldManifest = Join-Path $dist 'world-manifest.json'
if ((Test-Path $readSet) -and (Test-Path $worldManifest)) {
    $world = Get-Content -Raw $worldManifest | ConvertFrom-Json
    $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    $names = New-Object 'System.Collections.Generic.List[string]'
    foreach ($name in @($world.names) + @((Get-Content -Raw $readSet | ConvertFrom-Json).names)) {
        if ($seen.Add($name)) { $names.Add($name) }
    }
    $lines = $names | ForEach-Object { $_ | ConvertTo-Json }
    $json = "{`n`"version`": $($world.version),`n`"names`": [`n" + ($lines -join ",`n") + "`n]`n}`n"
    [IO.File]::WriteAllText($worldManifest, $json, (New-Object Text.UTF8Encoding($false)))
    "world-manifest.json: $($names.Count) names"
}
New-Item -ItemType Directory -Force (Join-Path $dist 'examples') | Out-Null
Copy-Item 'web\examples\*.js' (Join-Path $dist 'examples')

# wasm-opt -O3 is a speed pass (the client is CPU-bound on one wasm thread); the feature flags
# match what rustc emitted, or wasm-opt refuses the module.
$opt = Join-Path $root 'tools\binaryen\bin\wasm-opt.exe'
$bg = Join-Path $dist 'wenilla_bg.wasm'
if ((Test-Path $opt) -and -not $DebugSymbols) {
    & $opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue $bg -o "$bg.opt"
    Assert-Ok 'wasm-opt'
    Move-Item -Force "$bg.opt" $bg
}

# Precompressed .gz siblings, which wenilla-host serves without per-request CPU.
foreach ($file in Get-ChildItem $dist -File | Where-Object { $_.Extension -in '.wasm', '.js' }) {
    $in = [IO.File]::OpenRead($file.FullName)
    $out = [IO.File]::Create($file.FullName + '.gz')
    $gz = New-Object IO.Compression.GZipStream($out, [IO.Compression.CompressionLevel]::Optimal)
    $in.CopyTo($gz)
    $gz.Dispose(); $out.Dispose(); $in.Dispose()
}
Get-ChildItem $dist | Format-Table Name, Length
