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
| `crates/farmcraft` | The launcher: starts `farm-server` on a background thread, points the stock client at it (`WOW_HOST`) and logs in (`WOW_USER`/`WOW_PASS`). Its `hotbar.lua` runs in the stock UI's VM: the action bar holds a button for every action (wheel or number keys pick, Shift+wheel zooms) and tells the server the pick with `.sel <n>`. Slots 1-7 are tools (Shovel, which casts Till Soil once earned, Watering Can, Sickle, Axe, Mining Pick, Build Fence, Horse) wearing borrowed free spells, renamed and re-iconed in Lua; the server lights the tool a job takes (`FarmcraftTool` addon line). Slots 8-12 are the seeds carried. |

The stock client is unchanged: movement, camera, UI are regular WoW. Game rules go in
`farm-rules` first with tests, then are shown by `farm-server`.

`crates/among-*` are the Among Azeroth crates this repo was forked from, kept for reference until
their pieces are no longer needed; `crates/benilla-*` and `crates/wenilla*` are upstream.

## Remotes

- `origin`: github.com/randomxt-dev/world-of-farmcraft (public since 2026-10-28: anything committed is published).
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
- Gameobject guids carry their entry in bits 24-47: the client reads the entry from the guid
  to ask for the template, and without it no lock, cursor or name works.
- `FARMCRAFT_SELFTEST=1` turns the wheel twice in a live run and logs the hotbar's state;
  `=ride` presses the horse button, `=map` opens the world map.
- `.go xyz` needs a z above the ground (70 is safe around the farm): below it you fall forever.
- The game design (progression, economy, goals) is `docs/DESIGN.md`.
- Progression is a quest chain (`farm-rules/src/quests.rs`, shown by `farm-server/src/game/quests.rs`
  through WoW's own quest windows, log and `!`/`?`): tools, the crate and the animals are locked
  until a quest or Brog's shop gives them. Quests pay money only for goods handed over.
- Skills (`farm-rules/src/skills.rs`) wear borrowed `SkillLine.dbc` lines: Farming on Tailoring,
  Lumberjacking on Skinning, Woodworking on Engineering, Shepherding on Leatherworking (Riding is
  itself); the experience bar shows the picked bar slot's skill; `hotbar.lua` renames them
  in the Skills tab and the skill-up line. Watering's cast bar is a borrowed spell renamed the same way.
- `.preview [day]` (F8 in game, `game/preview.rs` + `Farm::preview`) rebuilds the world as day 10,
  25, 50 or 100 might look; it never saves.
- `HIDDEN_DOODADS` (set in `farmcraft/src/main.rs`, read by benilla-world's streamer) leaves the
  map's own melon props out of the field.
- `markers.lua` draws the villagers' !/? on the minimap and world map from an addon line the
  server sends (`FarmcraftMarks`), placing them with Elwynn's `WorldMapArea.dbc` bounds.
- In-game commands: `.time`, `.home`, `.gps` (where you stand), and for testing `.sleep`, `.rain`, `.hour`, `.gold`, `.chop`,
  `.quest <n>` (skip to quest n with what the earlier ones unlock), `.talk ma|farley|brog` (open their window)
  (the nearest grove tree). `WOW_PROBE_CHAT=".go xyz x y z 0;.chop"` types them from a scripted run,
  `WOW_PROBE_CAM="<yaw>,<pitch>,<dist>@<s>"` parks the camera (negative pitch looks down).
- `FARMCRAFT_TREE_LINE=1` lines the grove's tree kinds up in order, to look at.
- Ground survey for placing things: `target\release\examples\survey.exe <x> <y> [radius]` with
  `WOW_DATA` set.
