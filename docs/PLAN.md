# Plan

## Decisions (2026-10-26)
- Game: cozy farming (Stardew-like) in Elwynn Forest. Name: World of Farmcraft (for now).
- Base: fork ballxxxxx/among-azeroth (MIT OR Apache-2.0). It is a tiny 1.12.1 realmd and world
  server, and the stock benilla client logs into it unchanged, so we get the stock UI for free.
  We borrow from Detroph/eternal-survivors (same license): `terrain_height_at` ground lookups,
  `SMSG_WEATHER` and time-speed packets, RON content and serde saves.
- Native Windows first. Browser (wenilla) later, either with a small hosted server or with the
  server compiled to wasm in a Web Worker. Nobody has tried the wasm route yet.
- Hosting later: a trimmed, streamed data pack (~150-300 MB) recorded from real play sessions.
- Replace any data copied from vmangos (GPL), e.g. among-azeroth's `world/items.rs`.

## Assets verified in the 1.12.1 data
- Crops: herb nodes `World/SkillActivated/TradeskillNodes/Bush_*` (GO displays 268-271, 414, ...
  up to Black Lotus 4636). Corn growth stages `World/Azeroth/Elwynn/PassiveDoodads/CornCrop/`
  `corncrop1-3` and `corncropdead`. ElwynnWheat01, WestFallCabbage, WestFallMandrake, ElwynnMelon01
  (GO 1927), pumpkins (GO 60, 6325), watermelon (GO 332).
- Corn, wheat and cabbage have no GameObjectDisplayInfo row. Add rows in our data pack, or spawn
  them on the client.
- Dog: there is no real dog model. Worg Pup and the "Dog" critter use DireWolf.mdx; Young Wolf uses
  Wolf.mdx. DogHouse is GO display 154.
- Animals: swine/Princess (Boar), chicken, cow, sheep, cat, rabbit, squirrel, prairie dog, frog,
  owl. Pets: panda cub, Murky, tiny dragon, turtle, Mechanical Squirrel.
- Tools held in hand: `Misc_1H_Bucket_A_01`, `Misc_2H_Shovel_A_01`. Decor: scarecrow, plow, hay
  wagon, haystacks, windmills, fences, troughs, grain crates, herb sacks.
- Animations: KneelLoop (planting), Loot (harvest), EmoteWork, ChannelCastDirected (watering),
  FishingLoop, Sleep, SitGround.

## Places (world x, y, z)
| Place | Coordinates |
|---|---|
| Stonefield Farm (start) | -9890, 330, 37 |
| Goldshire | -9466, 50, 57 |
| Maclure Vineyards | -10010, 50, 35 |
| Brackwell Pumpkin Patch | -9780, -880, 40 |
| Fargodeep Mine | -9800, 130 |
| Jasperlode Mine | -9120, -590 |

## Prototype (about 4 weeks)
| Week | Goal |
|---|---|
| 1 | Fork; log in to our server at Stonefield Farm; strip the Among Us rules; 20-minute day cycle and weather |
| 2 | 4x4 plot: plant, water (bucket), sleep, growth stages (herbs + corn), harvest |
| 3 | Bags, Goldshire seed/crop vendor, copper in Fargodeep, saving |
| 4 | Wolf pup that follows you, doghouse, chicken coop, polish |

## Setup
- Game data: `your 1.12.1 client's Data folder`. Link it as `WoW` at the repo root (a junction;
  this path is gitignored).
- Toolchain: Rust (the version is pinned by the fork), wasm-bindgen-cli 0.2.122 and wasi-sdk 27
  for the browser build later.
