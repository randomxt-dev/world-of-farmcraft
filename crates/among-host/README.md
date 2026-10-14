# Play with friends

One person hosts. Friends join in a browser with no install, or with the native launcher and
their own WoW 1.12.1 client.

## Host

You need an English 1.12.1 (build 5875) WoW install. The browser players are streamed its game
files, one at a time, from your PC. Nothing from it is ever offered as a download.

From a release zip:

```powershell
.\among-host.exe
```

From the repo (`WoW` junction at the root, browser build made once with
`crates\among-web\web-setup.ps1` then `crates\among-web\web-build.ps1`):

```powershell
cargo run --release -p among-host
```

It starts the game server and the web page, then prints a **lobby code** and the invite links.
Everyone logs in with any account name and the lobby code as the password. The first player in
is the lobby host and starts the game with the Emergency button once 4 are in
(`--min-players`, `--impostors` and the rest are `among-server`'s options).

The page and its game files need the lobby code: the invite link carries it, and anyone without
it gets a 403.

## Friends in the browser (Chrome or Edge)

The world renders with WebGPU, which browsers allow only over https or on localhost. The easy
https is [Tailscale](https://tailscale.com), free for this:

- Your tailnet only: invite your friends to it, then `tailscale serve --bg 8090`.
- Anyone with the link: `tailscale funnel --bg 8090`.

Then send them the `https://<your-pc>.<tailnet>.ts.net/?lobby=CODE` link `among-host` prints.

## Friends with the native launcher

Download the release zip and run `among-azeroth.exe`. The first run asks for your WoW 1.12.1
folder, checks it holds the game's MPQs, and remembers it; nothing is copied or changed there.
At the login screen, set the Realmlist to the host:

- Same network: the LAN address `among-host` prints.
- Tailscale: the host's `100.x.y.z` address (`tailscale ip -4` on the host).
- Over the internet with port forwarding: forward TCP 3724 and 8085 to the host, and start it
  with `--advertise <public IP or name>` so the realm list hands out the public address.

## Options

```text
--web 0.0.0.0:8090     where the page listens
--bind 0.0.0.0         where the game server listens for native clients
--lobby-code CODE      a fixed code instead of a random one
--data PATH            the WoW Data folder to stream (default: found like the launcher finds it)
--www PATH             the browser build (default: www beside the exe)
```
