# Live check of what the client shows: builds and hosts among-server, then runs the among-azeroth
# client against it in this window. Close the client to stop the server.
#
#   .\crates\among-azeroth\scripts\live-check.ps1
#   .\crates\among-azeroth\scripts\live-check.ps1 -Account TESTUSER:TESTPASS -NoBuild -MinPlayers 3
param(
    # A test account as NAME:PASSWORD; default: WOW_USER and WOW_PASS from .probe-identity.
    [string]$Account = '',
    # Fewest players a game starts with; the rules never allow fewer than 3.
    [int]$MinPlayers = 4,
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

# The server takes the stock realmd and world ports, which a running one already holds.
if (Get-Process among-server -ErrorAction SilentlyContinue) {
    throw 'An among-server is already running: close it (and its clients) first.'
}
$serverExe = Join-Path $target 'release\among-server.exe'
$server = Start-Process -FilePath $serverExe -ArgumentList @('--account', $Account, '--min-players', $MinPlayers) -WorkingDirectory $root -PassThru
Write-Host ''
Write-Host 'among-server is up in its own window. Log in with the account above, then:'
Write-Host '  1. In the lobby the bar holds 4 buttons. Hover 3 and 4: Report and Skip Vote, each with'
Write-Host '     "Instant" and a gold description. Button 1 (Attack) keeps the stock tooltip; 5 and 6'
Write-Host '     are empty. There is no Use, Fix or Emergency button: those are right-clicks.'
Write-Host '  2. Hover button 2, your class spell: its real name and rank, and'
Write-Host '     "Deals N damage to a monster within N yards. It cannot harm players."'
Write-Host '  3. Press P: the spellbook lists every button, Kill and Sabotage included, with no rank,'
Write-Host '     and Herb Gathering and Mining. Bag 1 holds a Mining Pick.'
Write-Host '  4. Type /reload, then hover button 3 again: still "Report".'
Write-Host '  5. A big red Emergency Button stands at the spawn, and the sabotage stations around it: a'
Write-Host '     lamp post, a gnome terminal, two metal levers and two gnome machines. Hover each for'
Write-Host '     its name.'
Write-Host '  6. Chat: /s reaches players within 25 yd and /p everyone in the lobby; /y and /w say'
Write-Host '     they are off.'
Write-Host 'With a started game (-MinPlayers players logged in, then the host right-clicks the button):'
Write-Host '  7. The quest tracker under the minimap lists Crew Tasks and your tasks at once; an'
Write-Host '     impostor sees theirs marked (fake).'
Write-Host '  8. Impostors alone get Kill on button 5 and Sabotage on 6.'
Write-Host '  9. An impostor presses Sabotage: a tall purple column rises over each station to fix.'
Write-Host '     Right-click the station (or the column) within 5 yd: the column goes.'
Write-Host ' 10. Right-clicking the button within 5 yd calls a meeting (15 s after the start); before'
Write-Host '     that it answers "Ready in N s". /p works during the meeting, not in play.'
Write-Host ' 11. Right-click a copper vein: the mining cursor, then a pickaxe swing for 5 s. Its tooltip'
Write-Host '     says "Requires Mining" in red for every player; that line is cosmetic.'
Write-Host ' 12. A picked herb or vein vanishes for every player, and the quest line counts your share'
Write-Host '     ("Pick Peacebloom (2)"). Back in the lobby every node grows again.'
Write-Host 'This window prints "among-azeroth: ability text hooked" and "quest tracker hooked" at'
Write-Host 'login and at each /reload.'
Write-Host ''
try {
    & (Join-Path $target 'release\among-azeroth.exe')
}
finally {
    if (-not $server.HasExited) { Stop-Process -Id $server.Id }
}
