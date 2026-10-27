# World of Farmcraft

A cozy farming game set in Elwynn Forest, World of Warcraft 1.12.1. Restore a run-down farm near
Goldshire, grow herbs and crops, raise animals with your wolf pup at your side, mine copper in
Fargodeep, and sell your harvest in town. Working title.

Built on [benilla](https://github.com/samwhosung/benilla), a from-scratch WoW 1.12.1 client in Rust
and Bevy, through a fork of [Among Azeroth](https://github.com/ballxxxxx/among-azeroth)'s tiny 1.12.1
server, so the stock WoW interface (bags, vendors, quests, cast bars) works unchanged.

> Unofficial fan project, not affiliated with Blizzard Entertainment. No game files are included:
> it reads them from your own English 1.12.1 client (build 5875).

Status: playable prototype on Windows. See [docs/PLAN.md](docs/PLAN.md) and the full game's
design in [docs/DESIGN.md](docs/DESIGN.md), or open
[docs/design.html](docs/design.html) for the interactive timeline.

## Play

1. Link your 1.12.1 client's folder as `WoW` at the repo root (PowerShell, no admin needed):
   `New-Item -ItemType Junction -Path WoW -Target C:\path\to\WoW`
2. Double-click `play.bat` (the first build takes a few minutes).
3. Create a character, then enter the world at Stonefield Farm.

You start with a shovel and 8 Silverleaf seeds. Everything else comes from quests: follow the
yellow `!` and `?` over Ma Stonefield at the farm, and Innkeeper Farley and Brog Hamfist at the
Lion's Pride Inn in Goldshire.

| Quest | Giver → turn in | Unlocks |
|---|---|---|
| 1 Breaking Ground: plant your 8 seeds | Ma → Ma | Watering Can |
| 2 Thirsty Work: water 8 crops | Ma → Ma | 10c |
| 3 Early to Bed: sleep in your bed | Ma → Ma | Farm Horse |
| 4 Market Day: ride to Goldshire | Ma → Farley | Farley buys your produce |
| 5 Seeds for Spring: buy 10 seeds | Farley → Brog | 20c |
| 6 The First Harvest: bring 10 Silverleaf | Brog → Farley | Shipping Crate at the farm |
| 7 Timber!: buy an axe, bring 4 wood | Brog → Ma | Build Fence |
| 8 Good Fences: build 8 sections | Ma → Ma | A pair of hens |
| 9 Eggs for Breakfast: gather 3 eggs | Ma → Ma | The cow and the sheep |
| 10 Into Fargodeep: buy a pick, mine 5 copper | Brog → Brog | 1s |

| Do | How |
|---|---|
| Hotbar | 1 Till Soil, 2 Watering Can, 3 Axe, 4 Build Fence, 5 Horse (each once you have it), 6-12 your seeds. Scroll the mouse wheel or press a number to pick (Shift+wheel zooms) |
| Plant | Pick a seed, then right-click a dirt mound |
| Water | Pick the watering can, then right-click a planted crop once a day; rain does it for you |
| Harvest | Right-click a ripe crop |
| Sleep | Right-click the bed inside the farmhouse; past 2:00 you pass out |
| Sell | To Innkeeper Farley in the Goldshire inn, or drop goods in the Shipping Crate once you have it |
| Buy | Seeds, the axe and the pick from Brog Hamfist in the Goldshire inn |
| Dig more plots | Press 1 (Till Soil), then click a furrow in the field |
| Fences | Press 4, click where the fence starts, then where it goes: Elwynn split rail goes up along the line, 2 Simple Wood a section, and carries on from the end. Click the end again to stop. Short of wood, the rest is marked with posts: right-click one to build it later. Right-click a fence to take it down |
| Horse | Press 5 to mount or step down: twice as fast, for trips to Goldshire |
| Animals | Right-click the cows, sheep and chickens once a day for milk, wool and eggs |
| Mine | Copper veins in Fargodeep Mine, south-west of the farm |
| Wild herbs | Peacebloom and Silverleaf grow around the farm; pick them for herbs and sometimes seeds |
| Chop trees | Right-click one of the Westfall oaks around the property: three axe swings and it falls for Simple Wood. A sapling comes up the next day and is a tree again in 3 |
| Tools | The shovel comes out for the soil and the axe for the trees, by themselves |
| Pets | Visit the pet pen south of the field and right-click an animal to make it your companion |
| Wildlife | Rabbits, squirrels, deer, hares, prairie dogs and toads wander the property |
| Map | The farm is a house on the world map and minimap whenever you are away |
| Help | Talk to Ma Stonefield; type `.time`, `.home` or `.gps` in chat. `.quest <n>` skips to quest n, for testing |
