# World of Farmcraft

A cozy farming game set in Elwynn Forest, World of Warcraft 1.12.1. Restore a run-down farm near
Goldshire, grow herbs and crops, raise animals with your wolf pup at your side, mine copper in
Fargodeep, and sell your harvest in town. Working title.

Built on [benilla](https://github.com/samwhosung/benilla), a from-scratch WoW 1.12.1 client in Rust
and Bevy, through a fork of [Among Azeroth](https://github.com/ballxxxxx/among-azeroth)'s tiny 1.12.1
server, so the stock WoW interface (bags, vendors, quests, cast bars) works unchanged.

> Unofficial fan project, not affiliated with Blizzard Entertainment. No game files are included:
> it reads them from your own English 1.12.1 client (build 5875).

Status: playable prototype on Windows. See [docs/PLAN.md](docs/PLAN.md).

## Play

1. Link your 1.12.1 client's folder as `WoW` at the repo root (PowerShell, no admin needed):
   `New-Item -ItemType Junction -Path WoW -Target C:\path\to\WoW`
2. Double-click `play.bat` (the first build takes a few minutes).
3. Create a character, then enter the world at Stonefield Farm.

| Do | How |
|---|---|
| Plant | Right-click a dirt mound (pick the seed if you carry several kinds) |
| Water | Right-click a planted crop once a day; rain does it for you |
| Harvest | Right-click a ripe crop |
| Sleep | Right-click the hay pile by the farmhouse; past 2:00 you pass out |
| Sell | Drop goods in the Shipping Crate, or sell to Brog Hamfist in the Goldshire inn |
| Buy seeds | Brog Hamfist, <General Supplies> |
| Mine | Copper veins in Fargodeep Mine, south-west of the farm |
| Help | Talk to Ma Stonefield; type `.time` or `.home` in chat |
