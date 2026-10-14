# Play with friends

One person hosts. Friends join in a browser with no install, or with the native launcher and
their own WoW 1.12.1 client.

## Host

You need an English 1.12.1 (build 5875) WoW install. The browser players are streamed its game
files, one at a time, from your PC. Nothing from it is ever offered as a download.

From a release zip, in the folder you unzipped it to:

```powershell
cd C:\path\to\AmongAzeroth
.\among-host.exe
```

From the repo (`WoW` junction at the root; the browser build made once with
`crates\among-web\web-setup.ps1`, then `crates\among-web\web-build.ps1` after each change):

```powershell
cd D:\AmongAzeroth
cargo run --release -p among-host
```

It starts the game server and the web page, then prints a **lobby code** and the invite links.
Everyone logs in with any account name and the lobby code as the password. The first player in
is the lobby host: once 4 are in (`--min-players`), they right-click the emergency button in the
abbey hall to start. The rest of `among-server`'s options (`--impostors`, `--layout`, ...) work
here too.

The page and its game files need the lobby code: the invite link carries it, and anyone without
it gets a 403.

## Friends in the browser (Chrome or Edge)

The world renders with WebGPU, which browsers allow only over https or on localhost, so friends
need an https link to your PC. Either:

- **Cloudflare** (no account): install `cloudflared` once
  (`winget install Cloudflare.cloudflared`), then start the host with `--tunnel`:
  ```powershell
  cd D:\AmongAzeroth
  cargo run --release -p among-host -- --tunnel
  ```
  It prints `Friends: https://<random words>.trycloudflare.com/?lobby=CODE`; send that. The
  address changes each time, and the tunnel closes with the host. If your internet provider
  filters `trycloudflare.com` (GlobalConnect's SafeSurf does), set Windows' DNS to Cloudflare's
  `1.1.1.1` first.
- **Tailscale**: `tailscale funnel --bg 8090`, then send the
  `https://<your-pc>.<tailnet>.ts.net/?lobby=CODE` link `among-host` prints.

## Friends with the native launcher

Download the release zip and run `among-azeroth.exe`. The first run asks for your WoW 1.12.1
folder, checks it holds the game's MPQs, and remembers it; nothing is copied or changed there.
At the login screen, set the Realmlist to the host:

- Same network: the LAN address `among-host` prints.
- Over the internet: forward TCP 3724 and 8085 to the host, and start it with
  `--advertise <public IP or name>` so the realm list hands out the public address. A
  Cloudflare quick tunnel carries only the web page, not these two ports.

## Playing

Tasks, sabotage fixes and meetings are right-clicks on the world's own objects: a task's herb,
vein or station, a sabotage's station (or the purple column over it), and the emergency button.
The action bar holds only Attack, your class spell, Report and Skip Vote, plus Kill and Sabotage
for the impostors.

## Options

```text
--web 0.0.0.0:8090     where the page listens
--bind 0.0.0.0         where the game server listens for native clients
--lobby-code CODE      a fixed code instead of a random one
--data PATH            the WoW Data folder to stream (default: found like the launcher finds it)
--www PATH             the browser build (default: www beside the exe)
```
