# Cut the world-entry read set out of an among-host request log into northshire-manifest.json,
# which web-build.ps1 merges into the page's world-manifest.json so the glue screens prefetch it.
# Capture: start among-host with --request-log <file> on a fresh port (a cold browser cache),
# enter the world in the browser, stop the host once the loading screen clears.
# Usage: read-set.ps1 -Log <file> [-From <regex of the first world-entry name>]
param(
    [Parameter(Mandatory)] [string] $Log,
    [string] $From = '\.wdt$'
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')

# What the page already prefetches stays out.
$known = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
foreach ($manifest in 'web\boot-manifest.json', 'web\world-manifest.json') {
    foreach ($name in (Get-Content -Raw (Join-Path $root $manifest) | ConvertFrom-Json).names) {
        [void]$known.Add($name)
    }
}

# Lines are `ms <TAB> status <TAB> ms taken <TAB> method <TAB> name`; served GETs only, first
# request order.
$names = New-Object 'System.Collections.Generic.List[string]'
$entered = $false
foreach ($line in Get-Content $Log) {
    $cols = $line -split "`t", 5
    if ($cols.Count -lt 5) { continue }
    $name = $cols[4]
    if (-not $entered) { $entered = $name -match $From }
    if (-not $entered -or $cols[1] -ne '200' -or $cols[3] -ne 'GET' -or $name -eq '__index') { continue }
    if ($known.Add($name)) { $names.Add($name) }
}
if (-not $entered) { throw "no request in $Log matches -From '$From'" }

$out = Join-Path $PSScriptRoot 'northshire-manifest.json'
# The manifests' own layout: one name per line.
$lines = $names | ForEach-Object { $_ | ConvertTo-Json }
$json = "{`n`"version`": 1,`n`"names`": [`n" + ($lines -join ",`n") + "`n]`n}`n"
[IO.File]::WriteAllText($out, $json, (New-Object Text.UTF8Encoding($false)))
"$($names.Count) names -> $out"
