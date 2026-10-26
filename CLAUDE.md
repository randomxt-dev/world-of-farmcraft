# World of Farmcraft

A cozy farming game set in Elwynn Forest, WoW 1.12.1: plant Azeroth's herbs and garden crops on
Stonefield Farm, water them, sleep, harvest, ship or sell, mine copper in Fargodeep, with a wolf
pup at your heel. Built on **benilla**, a from-scratch 1.12.1 client in Rust + Bevy, through a fork
of **Among Azeroth**'s tiny 1.12.1 server. Private fan project: non-commercial, not affiliated with
Blizzard. The plan and the verified asset ids: `docs/PLAN.md`.

Upstream's rules for the client crates are `docs/METHOD.md`, `docs/MAP.md` and
`docs/CONTRIBUTING.md`: read them before touching `crates/benilla-*`. `docs/AMONG-AZEROTH.md` is
the Among Azeroth README this repo was forked from.

## Architecture

| crate | what |
|---|---|
| `crates/farm-rules` | The game as a pure state machine: clock and weather, crops, plot tiles, ore veins, the bag, the store, saves. No Bevy, no network, no game data; unit-tested. |
| `crates/farm-server` | A minimal 1.12.1 realmd + world server (protocol code from `among-server`) that shows a farmer's farm through WoW's own objects: soil and crops are gameobjects, the store is Brog Hamfist's stock merchant window, saves are RON. `game.rs` is one farmer's session; `layout.rs` is where everything stands. |
| `crates/farmcraft` | The launcher: starts `farm-server` on a background thread, points the stock client at it (`WOW_HOST`) and logs in (`WOW_USER`/`WOW_PASS`). |

The stock client is unchanged: movement, camera, UI are regular WoW. Game rules go in
`farm-rules` first with tests, then are shown by `farm-server`.

`crates/among-*` are the Among Azeroth crates this repo was forked from, kept for reference until
their pieces are no longer needed; `crates/benilla-*` and `crates/wenilla*` are upstream.

## Remotes

- `origin`: github.com/randomxt-dev/world-of-farmcraft (private).
- `upstream`: ballxxxxx/among-azeroth. Merged once as the base; later syncs are real merges.

## Hard rules

- **Never commit anything from a WoW install**: art, models, sounds, maps, DBC, MPQ contents.
  Game data is read at runtime from `WOW_DATA` or the gitignored `WoW` junction at the repo root
  (here it points at `your own 1.12.1 install`).
- **The install is read-only.** Nothing we run writes into it.
- **Our features live in our crates.** A change inside an upstream file only when unavoidable,
  the smallest possible, with a one-line comment saying why.
- **No vmangos (GPL) data copied in bulk.** Single facts (a spawn's coordinates, a display id)
  are fine; tables are not.
- **Code that can run in the browser**: no threads, no `std::time::Instant`, `SystemTime` or
  `env::vars_os` in code meant for wasm.
- **Commit small and often, on a branch.** Never say "done" without saying what was verified and
  how.

## Gates

- `cargo fmt --all -- --check`
- `cargo clippy -p farm-rules -p farm-server -p farmcraft --all-targets -- -D warnings`
- `cargo test -p farm-rules -p farm-server` (needs no game data; every server packet is parsed
  back through the client's own reader)

## Running it

- Play: `play.bat`, or `cargo build --release -p farmcraft` then `target\release\farmcraft.exe`.
  The farm saves to `target\release\benilla-config\farmcraft\save.ron` (`BENILLA_HOME` moves it).
- Scripted run with a screenshot: make a save with
  `target\release\examples\new_farmer.exe <dir>\farmcraft\save.ron Hoggle [demo]`, then run with
  `BENILLA_HOME=<dir> WOW_CHAR=Hoggle WOW_UNATTENDED=1 WOW_NOSOUND=1 WOW_WIN=1600x900
  WOW_LIVE_SHOT=<png> WOW_LIVE_SHOT_AT=22`. `WOW_PROBE_CHEST=<x>,<y>,<z>,0` makes the client
  teleport (`.go xyz`) and right-click the nearest lockable object, e.g. a ripe crop.
- In-game commands: `.time`, `.home`, and for testing `.sleep`, `.rain`, `.hour`, `.gold`.
- Ground survey for placing things: `target\release\examples\survey.exe <x> <y> [radius]` with
  `WOW_DATA` set.
