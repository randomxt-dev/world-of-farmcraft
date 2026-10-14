# Live check of the action-bar names and tooltips: builds and hosts among-server, then runs the
# among-azeroth client against it in this window. Close the client to stop the server.
#
#   .\crates\among-azeroth\scripts\ability-text-check.ps1
#   .\crates\among-azeroth\scripts\ability-text-check.ps1 -Account TESTUSER:TESTPASS -NoBuild
param(
    # A test account as NAME:PASSWORD; default: WOW_USER and WOW_PASS from .probe-identity.
    [string]$Account = '',
    # Skip the release build (about 6.5 min from clean).
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
# A linked worktree keeps no WoW link or .probe-identity of its own: the main checkout's serve.
$main = Split-Path -Parent (Resolve-Path (git -C $root rev-parse --git-common-dir)).Path

function Find-First([string[]]$paths) {
    foreach ($p in $paths) {
        if ($p -and (Test-Path $p)) { return $p }
    }
    return $null
}

if (-not $Account) {
    $identity = Find-First @((Join-Path $root '.probe-identity'), (Join-Path $main '.probe-identity'))
    if (-not $identity) { throw 'No .probe-identity found: pass -Account NAME:PASSWORD (a test account).' }
    $vars = @{}
    foreach ($line in Get-Content $identity) {
        if ($line -match '^\s*(\w+)\s*=\s*(.*)$') { $vars[$Matches[1]] = $Matches[2].Trim() }
    }
    $Account = $vars['WOW_USER'] + ':' + $vars['WOW_PASS']
}

if (-not $env:WOW_DATA) {
    $data = Find-First @((Join-Path $root 'WoW\Data'), (Join-Path $main 'WoW\Data'))
    if (-not $data) { throw 'No WoW\Data found: set WOW_DATA to the 1.12.1 Data folder.' }
    $env:WOW_DATA = $data
}

$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
if (-not $NoBuild) {
    cargo build --release -p among-server -p among-azeroth --manifest-path (Join-Path $root 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}

$serverExe = Join-Path $target 'release\among-server.exe'
$server = Start-Process -FilePath $serverExe -ArgumentList @('--account', $Account) -WorkingDirectory $root -PassThru
Write-Host ''
Write-Host 'among-server is up in its own window. Log in with the account above, then:'
Write-Host '  1. Hover action-bar buttons 3 to 9. Each shows our name (Use, Report, Kill, Sabotage,'
Write-Host '     Fix, Emergency Meeting, Skip Vote), "Instant" with any cooldown on the right, and a'
Write-Host '     gold description. Button 1 (Attack) keeps the stock tooltip.'
Write-Host '  2. Hover button 2, your class spell: its real name and rank, and'
Write-Host '     "Deals N damage to a monster within N yards. It cannot harm players."'
Write-Host '  3. Press P: the spellbook lists our names, with no rank under the buttons.'
Write-Host '  4. Type /reload, then hover button 5 again: still "Kill".'
Write-Host 'This window prints "among-azeroth: ability text hooked on VM session N" at login and at'
Write-Host 'each /reload.'
Write-Host ''
try {
    & (Join-Path $target 'release\among-azeroth.exe')
}
finally {
    if (-not $server.HasExited) { Stop-Process -Id $server.Id }
}
