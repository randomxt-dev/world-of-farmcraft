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
design in [docs/DESIGN.md](docs/DESIGN.md).

## Play

1. Link your 1.12.1 client's folder as `WoW` at the repo root (PowerShell, no admin needed):
   `New-Item -ItemType Junction -Path WoW -Target C:\path\to\WoW`
2. Double-click `play.bat` (the first build takes a few minutes).
3. Create a character, then enter the world at Stonefield Farm.

| Do | How |
|---|---|
| Hotbar | 1 Till Soil, 2 Watering Can, 3 Axe, 4 Build Fence, 5 Horse, 6-12 your seeds. Scroll the mouse wheel or press a number to pick (Shift+wheel zooms) |
| Plant | Pick a seed, then right-click a dirt mound |
| Water | Pick the watering can, then right-click a planted crop once a day; rain does it for you |
| Harvest | Right-click a ripe crop |
| Sleep | Right-click the bed inside the farmhouse; past 2:00 you pass out |
| Sell | Drop goods in the Shipping Crate, or sell to Brog Hamfist in the Goldshire inn |
| Buy seeds | Auntie Bernice by the farmhouse, or Brog Hamfist in the Goldshire inn |
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
| Help | Talk to Ma Stonefield; type `.time`, `.home` or `.gps` in chat |
