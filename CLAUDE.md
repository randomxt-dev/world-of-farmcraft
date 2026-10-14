# Among Azeroth

Among Us played in World of Warcraft 1.12.1: crewmates do tasks around a real WoW map, impostors
kill, bodies get reported, everyone meets and votes someone out. It is built on **benilla**, a
from-scratch 1.12.1 client in Rust + Bevy, through its browser fork **wenilla**. Private fan
project: non-commercial, no Blizzard branding, not affiliated with Blizzard.

This file is authoritative for our crates and process. Upstream's rules for the client crates are
`docs/METHOD.md`, `docs/MAP.md` and `docs/CONTRIBUTING.md`: read them before touching
`crates/benilla-*`. `AGENTS.md` is wenilla's own fork process (PRs to `Arnesen/wenilla`, its
realm deploy); only its wasm invariants apply to us.

## Non-negotiables

- **Movement, camera and controls are regular WoW.** benilla's own player controller, follow
  camera, movement packets and animation, untouched. No top-down camera, no custom hero
  controller.
- **1.12.1 only** (build 5875). benilla supports no other client version.
- **Multiplayer is server-authoritative.** Roles, kills, bodies, meetings, votes, sabotage and win
  conditions are decided by the server; a client only displays and requests.

## Architecture (the plan)

Modelled on Aethus's Azeroth Survivors native build, whose game crate `wenilla-rogue` ships its
own `realmd.rs` + `server.rs` + `proto.rs`: a small 1.12.1 server the stock benilla client logs
in to, so the client's real movement and world work unchanged. We do the same, multiplayer.

| crate | what |
|---|---|
| `crates/among-rules` | The game as a pure state machine: lobby, roles, tasks, kills, bodies, meetings, votes, ejections, sabotage, win check. No Bevy, no network, no game data; fully unit-tested. |
| `crates/among-server` | A minimal 1.12.1 realmd + world server that speaks the original protocol, hosts the lobby and drives `among-rules`. Uses WoW's own death/ghost states, teleports for meetings, chat for discussion. |
| `crates/among-azeroth` | The launcher: benilla through `benilla_app::run_with` plus our Bevy plugins (role reveal, task list, Use / Report / Kill / Sabotage, voting screen). Host mode starts `among-server` in-process. |

`benilla-protocol` and `benilla-srp` are the client half of the wire; the server half is ours.
They are the reference for every packet we must answer.

## Remotes

- `wenilla`: the upstream we merge (benilla + browser build). `main` is based on `wenilla/main`.
- `benilla`: wenilla's upstream; reference only.
- `survivors`: OutThisLife/azeroth-survivors-re. Unrelated history: **never merge it.** It is a
  top-down, server-less re-implementation that disables the player controller. Read it
  (`git show survivors/main:<path>`) for HUD, menu and local-entity ideas only.

## Hard rules (from benilla's METHOD / wenilla's AGENTS, adapted)

- **Never commit anything from a WoW install**: art, models, sounds, maps, DBC, MPQ contents,
  FrameXML. Game data is read at runtime from `WOW_DATA` or a gitignored `WoW` junction at the
  repo root. Never copy anything out of `D:\Games\AzerothSurvivors` either: its MPQs are not ours.
- **The install is read-only.** Nothing we run writes into the WoW folder.
- **Our features live in our crates.** A change inside an upstream file only when unavoidable:
  the smallest possible, behind our own flag or `#[cfg]`, with a one-line comment saying why.
  Every upstream line we touch conflicts at the next merge, so touch few.
- **Upstream is merged, never copied.** A sync is a real merge commit with `wenilla/main` as the
  second parent. Never GitHub's "Sync fork" button.
- **Our settings are our own.** Never add rows to benilla's CVar table; our files go under
  `benilla_app::config_dir()` (`benilla-config/`, gitignored).
- **Code that can run in the browser**: no threads, no `std::time::Instant`, `SystemTime` or
  `env::vars_os` (they panic on wasm); use `bevy::platform::time::Instant` or `web-time`.
- **Our code is MIT OR Apache-2.0**, a claim about provenance: no original client code, no
  bundled assets.
- **Test accounts only.** A scripted login uses the account in `.probe-identity` (never
  committed), never a real person's. Agent runs set `WOW_UNATTENDED=1` and `WOW_NOSOUND=1`.

## Method

1. **Reference first.** For anything WoW-defining (a packet, movement, death and ghost, visibility)
   find what 1.12.1 does before building: benilla's protocol code, `reference/`,
   [benilla-re](https://github.com/samwhosung/benilla-re), a packet trace
   (`WOW_MOVE_TRACE=<file>` with `WOW_MOVE_TRACE_TAGS="in,out"`).
2. **Build idiomatically.** Modern Rust and Bevy. Game rules go in `among-rules` first, with
   tests, then get wired to the server and the HUD.
3. **Measure, never eyeball.** Traces, logs and numbers settle timing and behaviour. Take one
   simple screenshot at most; if it does not settle the question, hand the user a precise script
   ("host a lobby, join with a second client, kill, does the body show?").
4. **Close the loop on the symptom.** Reproduce a reported bug first, fix it, and show the same
   check clean afterwards.
5. **Commit small and often, on a branch.** Never say "done" without saying what was verified
   and how.
6. **Comments**: one line, what the code does and the fact behind it. No history, no changelogs
   or status docs; git is the history.

## Gates

- `cargo fmt --all -- --check`
- `cargo clippy -p among-rules -p among-server -p among-azeroth --all-targets -- -D warnings`
- `cargo test -p among-rules -p among-server` (must pass with no game data)
- A change to a `benilla-*` crate also runs upstream's `scripts/check.sh` (bash: Git Bash or WSL).
- Platform seams (`cfg(target_os)`, `#[link]`) are invisible to the other platform's gates: say
  in the commit which platforms were built.

## This machine

- Windows 11, Ryzen 7 9800X3D, 32 GB, RTX 5070 Ti. Native builds use MSVC (Visual Studio
  Community 2026). The repo lives on `D:`; `C:` is short on space, so keep `target/` here.
- Rust is pinned by `rust-toolchain.toml`; rustup installs that toolchain on first use.
- WSL Ubuntu 26.04 is available for upstream's bash scripts and the later browser build.
- Game data: `WoW/Data` at the repo root (gitignored), the 14 MPQs of the English 1.12.1
  (build 5875) client, MD5-verified against archive.org's `1.12.1-5875_enUS.zip`. benilla finds it
  with no `WOW_DATA` set.
- Run the stock client: `cargo run --release -p benilla` (release build ~6.5 min from clean). With
  no server it parks at the login screen; it opens borderless fullscreen at 3440x1440.
