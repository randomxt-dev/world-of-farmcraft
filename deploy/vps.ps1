<#
.SYNOPSIS
Deploy an Among Azeroth lobby to an Ubuntu VPS over ssh, logged in as root.

.DESCRIPTION
Cross-compiles a static Linux binary (rust-lld, no WSL), uploads it with the layout and the survey
marks, and runs deploy/setup.sh there, which installs a systemd service that restarts on crash and
reboot. Native players set their realmlist to the VPS.

-Browser runs among-host instead: it also uploads the browser build and the game data (the first
run sends all 5 GB; later runs send only MPQs whose size changed) and puts Caddy in front for the
https WebGPU needs. Run again to update; the lobby code is kept unless -LobbyCode is given.

.EXAMPLE
deploy\vps.ps1 -Server root@203.0.113.7
.EXAMPLE
deploy\vps.ps1 -Server root@203.0.113.7 -Browser -Layout crates\among-server\layouts\northshire.example.toml
#>
param(
    [Parameter(Mandatory)] [string] $Server,
    [switch] $Browser,
    [string] $LobbyCode,
    [string] $Layout,
    # NAME:PASSWORD of an operator account; with one, only operators may use survey commands.
    [string] $Owner,
    # The https name browser players open (default: <ip>.sslip.io, which needs no domain).
    [string] $Domain,
    [string] $Marks,
    [string] $Data,
    [string] $Www
)
$ErrorActionPreference = 'Continue'
function Assert-Ok($what) { if ($LASTEXITCODE -ne 0) { throw "$what failed ($LASTEXITCODE)" } }
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $root
if (-not $Marks) { $Marks = Join-Path $root 'benilla-config\among\marks.toml' }
if (-not $Data) { $Data = Join-Path $root 'WoW\Data' }
if (-not $Www) { $Www = Join-Path $root 'target\among-web\dist' }
$mode = if ($Browser) { 'host' } else { 'server' }
$lf = [string][char]10

if ($Browser -and -not (Test-Path (Join-Path $Www 'index.html'))) {
    throw "no browser build at ${Www}: run crates\among-web\web-build.ps1 first"
}
if ($Layout -and -not (Test-Path $Marks)) { throw "the layout names marks, but there are none at $Marks" }

# Pure-Rust dependencies, so Rust's own linker builds a static musl binary any distro runs.
$target = 'x86_64-unknown-linux-musl'
rustup target add $target | Out-Null
Assert-Ok 'rustup target add'
$env:CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = 'rust-lld'
cargo build --release --target $target -p "among-$mode"
Assert-Ok 'cargo build'

$stage = Join-Path $root 'target\deploy\among-deploy'
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
try {
    Copy-Item (Join-Path $root "target\$target\release\among-$mode") $stage
    # bash and systemd need LF; a Windows checkout has CRLF.
    foreach ($f in 'setup.sh', "among-$mode.service") {
        $text = (Get-Content -Raw (Join-Path $PSScriptRoot $f)) -replace '\r\n', $lf
        [IO.File]::WriteAllText((Join-Path $stage $f), $text)
    }
    if (Test-Path $Marks) { Copy-Item $Marks (Join-Path $stage 'marks.toml') }
    if ($Layout) { Copy-Item $Layout (Join-Path $stage 'layout.toml') }
    if ($Browser) {
        tar -czf (Join-Path $stage 'www.tar.gz') -C $Www .
        Assert-Ok 'packing the browser build'
    }
    $params = @("MODE=$mode")
    foreach ($p in @(@('LOBBY_CODE', $LobbyCode), @('OWNER', $Owner), @('DOMAIN', $Domain))) {
        if (-not $p[1]) { continue }
        # They land in a sourced file and a systemd command line, which split on spaces.
        if ($p[1] -match "['\s]") { throw "$($p[0]) cannot contain spaces or single quotes" }
        $params += "$($p[0])='$($p[1])'"
    }
    [IO.File]::WriteAllText((Join-Path $stage 'params.env'), ($params -join $lf) + $lf)

    Write-Host "Uploading to $Server..."
    ssh $Server 'rm -rf /tmp/among-deploy'
    Assert-Ok 'ssh'
    scp -q -r $stage "${Server}:/tmp/"
    Assert-Ok 'scp'

    if ($Browser) {
        # Only MPQs the VPS lacks, or holds at another size (an interrupted upload), are sent.
        $remote = @{}
        $listing = ssh $Server 'mkdir -p /opt/among-azeroth/Data && cd /opt/among-azeroth/Data && (stat -c %n:%s *.MPQ 2>/dev/null || true)'
        Assert-Ok 'listing the game data on the VPS'
        foreach ($line in $listing) {
            $name, $size = $line -split ':', 2
            if ($size) { $remote[$name] = [long]$size }
        }
        foreach ($mpq in Get-ChildItem $Data -Filter *.MPQ) {
            if ($remote[$mpq.Name] -eq $mpq.Length) { continue }
            Write-Host ('Uploading {0} ({1:N0} MB)' -f $mpq.Name, ($mpq.Length / 1MB))
            scp $mpq.FullName "${Server}:/opt/among-azeroth/Data/"
            Assert-Ok "uploading $($mpq.Name)"
        }
    }

    ssh $Server 'bash /tmp/among-deploy/setup.sh'
    Assert-Ok 'setup on the VPS'
} finally {
    # params.env can hold the owner's password.
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
}
