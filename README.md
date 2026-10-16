<div align="center">
  <h1>Among Azeroth</h1>
  <p><b>Among Us, played in World of Warcraft 1.12.1</b></p>
  <p>
    <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue?style=for-the-badge" alt="License"></a>
  </p>
</div>

Crewmates walk a real WoW map doing tasks: picking herbs, mining veins, killing wolves. Impostors
fake those tasks and kill. Bodies get reported, everyone meets at the emergency button, talks it
over in chat and votes someone out. Crew wins by finishing every task or ejecting every impostor;
impostors win by sabotage or by outnumbering the crew.

It runs on [benilla](https://github.com/samwhosung/benilla), a 1.12.1 client written from scratch
in Rust and [Bevy](https://bevy.org), through its browser fork
[wenilla](https://github.com/Arnesen/wenilla). Movement, camera and controls are regular WoW. The
game itself lives in a small server of ours that speaks the original 1.12.1 protocol, so the
client logs in to it unchanged.

## Playing

Everyone needs their own English 1.12.1 client (build 5875) for the game data, or a host that
streams it to the browser.

- **In the browser** (Chrome or Edge, WebGPU): open the invite link, `https://<host>/?lobby=CODE`.
  The landing page at `/` also creates a new lobby or joins one by code.
- **Native**: set your realmlist to the host and log in with any account name and the lobby code
  as the password. With several lobbies on one server, log in as `NAME-CODE`.

The emergency button starts the round. Each player gets a role, a class and a task list in the
quest log.

- **Tasks** are real WoW work: gather herbs and ore, slay creatures. Everyone can see you do them,
  and monsters fight back, so a crewmate can die to a wolf and leave a body like any other.
- **Your action bar** holds Attack, your class spell, Report, Vote and Skip Vote; impostors also
  get Kill and Sabotage. Task nodes, sabotage stations and the emergency button are right-clicks
  in the world.
- **Every class has one ability**: Rogue stealth, Mage Polymorph, Priest shield, Warlock Fear,
  Warrior Hamstring, Shaman Ghost Wolf, Hunter Freezing Trap, Paladin Divine Shield, Druid
  Entangling Roots.
- **Sabotage** cuts the crew's vision (Lights) or hides their task lists (Comms) until the
  stations are fixed. The critical ones, Oxygen and Reactor, win the game for the impostors if
  the crew doesn't fix them before the deadline.
- **The dead** rise as ghosts beside their corpse. Ghosts can still do tasks, and only other
  ghosts hear them.

## Hosting

`among-host` runs everything for one machine: the game server for native players, the browser
page, its game data and the WebSocket relay, all behind lobby codes.

```powershell
crates\among-web\web-setup.ps1      # once: wasm target, wasm-bindgen, wasi-sdk
crates\among-web\web-build.ps1      # the browser build, into target\among-web\dist
cargo run --release -p among-host -- --layout crates\among-server\layouts\northshire.example.toml --tunnel
```

`--tunnel` opens a Cloudflare quick tunnel and prints an https link for friends; without it the
invite names your LAN or Tailscale address. Other useful flags: `--min-players`, `--impostors`,
`--lobby-code`, `--countdown`, `--max-lobbies`. To put a lobby on an Ubuntu VPS behind Caddy:

```powershell
deploy\vps.ps1 -Server root@<ip> -Browser -Layout crates\among-server\layouts\northshire.example.toml
```

The native client is `cargo run --release -p among-azeroth`: the stock benilla client plus our
plugins. Point it at a server with `WOW_HOST` or the Realmlist button on the login screen.

### Game data

Game data is read at runtime from `WOW_DATA`, or from a `WoW` link at the repo root (on Windows a
junction: `New-Item -ItemType Junction -Path WoW -Target C:\path\to\WoW`). Nothing is ever written
into the install. Map layouts (task spots, creatures, stations) are TOML; the server's `.mark`
chat commands survey new spots in game.

## How it's built

| crate | what |
|---|---|
| `among-rules` | The game as a pure state machine: lobby, roles, tasks, kills, bodies, meetings, votes, ejections, sabotage, class abilities, win check. No Bevy, no network; unit-tested. |
| `among-server` | A minimal 1.12.1 realmd and world server. Hosts many lobbies, drives `among-rules`, and shows it all with WoW's own pieces: death and ghosts, teleports for meetings, chat, quests, the action bar. |
| `among-spells` | Which existing 1.12 spells our buttons are, shared by the server's bar and the client's tooltips. |
| `among-host` | One command to host: the server, the browser page and game data, the landing page and lobby creation. |
| `among-azeroth` | The native launcher: benilla with our Bevy and Lua plugins (ability tooltips, the task tracker, sabotage effects). |
| `among-web` | The same, compiled for the browser on wenilla's wasm entry. |

Everything under `crates/benilla-*` and `crates/wenilla*` is upstream, merged in from
[Arnesen/wenilla](https://github.com/Arnesen/wenilla) and changed only where unavoidable.
[`CLAUDE.md`](CLAUDE.md) has the project rules and gates; [`docs/MAP.md`](docs/MAP.md) maps the
client.

```powershell
cargo fmt --all -- --check
cargo clippy -p among-rules -p among-server -p among-azeroth --all-targets -- -D warnings
cargo test -p among-rules -p among-server
```

The tests need no game data.

---

Built on [benilla](https://github.com/samwhosung/benilla) by Sam Sung and
[wenilla](https://github.com/Arnesen/wenilla) by Alexander Arnesen and contributors.

Among Azeroth is a non-commercial fan project, not affiliated with or endorsed by Blizzard
Entertainment or Innersloth. It ships **no Blizzard content**: no art, models, sounds, maps, MPQ
contents or FrameXML. You provide your own legally obtained 1.12.1 client. World of Warcraft is a
trademark of Blizzard Entertainment, Inc.; Among Us is a trademark of Innersloth LLC.

Our own code is licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your
option. The vendored components under `third_party/`, the kira audio engine and a Lua 5.1 patched
to the 1.12 client's dialect, keep their own licenses, alongside each.
