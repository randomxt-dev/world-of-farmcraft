# One-time setup for the browser build on Windows, the same tools as upstream's scripts/web-setup.sh:
# the wasm32 target, the wasm-bindgen CLI pinned to Cargo.lock, wasi-sdk (a C sysroot for the Lua
# VM) in tools/wasi-sdk, and binaryen's wasm-opt in tools/binaryen. Re-runnable; done steps skip.
# Native tools write progress to stderr, which 'Stop' would take for a failure: check exit codes.
$ErrorActionPreference = 'Continue'
function Assert-Ok($what) { if ($LASTEXITCODE -ne 0) { throw "$what failed ($LASTEXITCODE)" } }
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
Set-Location $root

Write-Host '== rustup target'
rustup target add wasm32-unknown-unknown
Assert-Ok 'rustup target add'

Write-Host '== wasm-bindgen-cli (pinned to Cargo.lock)'
$lock = Get-Content Cargo.lock -Raw
$want = [regex]::Match($lock, 'name = "wasm-bindgen"\r?\nversion = "([^"]+)"').Groups[1].Value
$have = ''
if (Get-Command wasm-bindgen -ErrorAction SilentlyContinue) { $have = (wasm-bindgen --version).Split(' ')[1] }
if ($have -ne $want) {
    cargo install wasm-bindgen-cli --version $want --locked --force
    Assert-Ok 'cargo install wasm-bindgen-cli'
} else {
    Write-Host "wasm-bindgen $want already installed"
}

function Get-Release($url, $dir) {
    New-Item -ItemType Directory -Force $dir | Out-Null
    $archive = Join-Path $env:TEMP ([IO.Path]::GetFileName($url))
    curl.exe -sSfL $url -o $archive
    Assert-Ok "download $url"
    tar.exe -xzf $archive --strip-components=1 -C $dir
    Assert-Ok "unpack $archive"
    Remove-Item $archive
}

Write-Host '== wasi-sdk 27'
if (Test-Path 'tools\wasi-sdk\share\wasi-sysroot\lib\wasm32-wasip1\libsetjmp.a') {
    Write-Host 'already at tools\wasi-sdk'
} else {
    Get-Release 'https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-27/wasi-sdk-27.0-x86_64-windows.tar.gz' 'tools\wasi-sdk'
}

Write-Host '== binaryen 130 (wasm-opt -O3)'
if (Test-Path 'tools\binaryen\bin\wasm-opt.exe') {
    Write-Host 'already at tools\binaryen'
} else {
    Get-Release 'https://github.com/WebAssembly/binaryen/releases/download/version_130/binaryen-version_130-x86_64-windows.tar.gz' 'tools\binaryen'
}

Write-Host 'ready: crates\among-web\web-build.ps1'
