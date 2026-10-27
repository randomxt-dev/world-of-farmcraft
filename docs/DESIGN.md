# World of Farmcraft: game design

**Interactive version, phone-friendly: [design.html](design.html)** (timeline, quest cards, tool belt).

A cozy farming game in the Stardew Valley mould, set on Stonefield Farm. This is the target; what
is built today is marked **(built)**. Prices are in copper (100c = 1s, 100s = 1g), as the bag and
the merchant window show them.

## The feel

- **No fail state.** Nothing dies and nothing is lost. A missed day only slows a crop down.
- **Every day has a small plan.** Water, harvest, ship, then pick one bigger job: chop, mine,
  build, fish, or visit town.
- **Every ability is a quest.** You start as a plain farmer with a shovel. Each new tool, animal
  or building is a WoW quest with a clear reward, so the next goal is always on screen.
- **The world is the reward.** Quests open up more of Elwynn: Goldshire, Fargodeep, the lake,
  Eastvale. Each place has a reason to go back.

## Progression: the quest chain

Start: **Shovel, 8 Silverleaf seeds, 2 silver, day 1.** Quest givers show a yellow **!** when they
have a quest and a yellow **?** when you can turn one in.

| NPC | Where | Role |
|---|---|---|
| Ma Stonefield | The farm | Farm quests; later land, buildings, animals |
| Innkeeper Farley | Lion's Pride Inn, Goldshire | **Buys** your goods |
| Brog Hamfist | General Supplies, same inn | **Sells** seeds and tools |
| Smith Argus | Goldshire | Tool upgrades (planned) |

### Built

| # | Phase | Quest | Giver → turn-in | Objective | Reward |
|---|---|---|---|---|---|
| 1 | Farmer (days 1–2) | Breaking Ground | Ma → Ma | Plant your 8 Silverleaf seeds | Watering Can |
| 2 | | Thirsty Work | Ma → Ma | Water 8 crops | 10c; Ma explains rain waters for you |
| 3 | | Early to Bed | Ma → Ma | Sleep in your bed in the farmhouse | Farm Horse (hotbar slot 5, 2× walking speed) |
| 4 | Market (days 2–3) | Market Day | Ma → Farley | Ride to Goldshire, tell Farley the farm is growing again | 25c; Farley buys your produce |
| 5 | | Seeds for Spring | Farley → Brog | Buy 10 seeds from Brog | 20c |
| 6 | | The First Harvest | Brog → Farley | Bring Farley 10 Silverleaf (water daily, ripe in 2 days) | 80c and the Shipping Crate at the farm |
| 7 | Woodcutter & Builder | Timber! | Brog → Ma | Buy a Woodcutter's Axe (1s), chop Westfall oaks, bring 4 Simple Wood | Build Fence |
| 8 | | Good Fences | Ma → Ma | Build 8 fence sections (2 wood each) | A pair of hens |
| 9 | Rancher | Eggs for Breakfast | Ma → Ma | Gather 3 eggs | The cow and the sheep: milk and wool daily |
| 10 | Miner | Into Fargodeep | Brog → Brog | Buy a Mining Pick (2s), mine 5 Copper Ore | 1s |

Farley's cart collects the Shipping Crate each morning.

### Planned

| Phase | Quests | Unlocks |
|---|---|---|
| Smith & tool upgrades (weeks 2–4) | A Fire by the Barn, Argus's Apprentice, Forged in Fargodeep, All That Glitters | Furnace, copper / iron / gold tools |
| Land (end of spring) | A Deed in Hand (25s), Rain on Demand, Glass and Green (2g) | East field (96 plots), sprinklers, greenhouse |
| Breeding & selling animals (summer) | Hay There, Five Hearts Full, Little Lamb Lost, A Fair Price | Hay, breeding, selling, the pig, coop/barn upgrades |
| House upgrades (summer–autumn) | Hearth and Home, Up the Stairs, Down in the Cellar | Kitchen, second floor, cellar |
| Swift Brown Steed (autumn) | Saddle Up, Swift as the Wind (Katie Hunter, Eastvale) | Saddlebags (20s), Swift Brown Steed (5g) |
| Seasons | Turn of the Season, Winter's Veil Pantry | 28-day seasons, seasonal seed lists |
| Stonefield Restoration (end of year 1) | Six ledger pages, then A Harvest Long Remembered | The harvest festival |

### Built outside the chain

Crops and growth stages, rain, the day clock, sleeping, wild Peacebloom and Silverleaf to forage,
choppable Westfall oaks that regrow, fence lines with planned posts when short on wood, digging
new plots with the shovel, the pet pen (~58 companions), wandering critters, copper veins in
Fargodeep that refill each morning, the farmhouse icon on the map.

## The day

| Time | |
|---|---|
| 06:00 | Wake in the farmhouse; the weather for the day **(built)** |
| Morning | Water and harvest. Drop goods in the Shipping Crate **(built)** |
| Afternoon | One bigger job: chop, mine, build fences, gather wild herbs, ride to Goldshire **(built)** |
| Evening | Tend the animals **(built)**; cook, craft, talk |
| 02:00 | Pass out wherever you are and lose a little gold; sleeping in bed is free **(built)** |
| Overnight | Crops grow, the crate pays out, trees regrow **(built)** |

A day lasts about 17 real minutes **(built)**. A season is 28 days and a year is 4 seasons.

## Land: how many plots

| Stage | Plots | How |
|---|---|---|
| Start | 24 (4 rows × 6) on the leaf rows | Given **(built)** |
| Spring | Up to 48 | Dig with the shovel on the field's furrows **(built, no cap yet)** |
| Copper shovel | Till 3 in a row | Smith upgrade |
| Field deed, 25s | The east field: up to 96 plots | Quest: A Deed in Hand |
| Sprinklers | Water 4 or 8 plots each morning | Quest: Rain on Demand; copper and tin bars |
| Greenhouse, 2g | Grows all year | Quest: Glass and Green; the barn's second floor |

Watering, not land, is the early limit, until sprinklers arrive.

## Crops by season

| Season | Crops |
|---|---|
| Spring | Silverleaf, Peacebloom, Earthroot **(built)**, Wheat |
| Summer | Mageroyal, Briarthorn **(built)**, Corn, Watermelon **(built)** |
| Autumn | Pumpkin **(built)**, Fadeleaf, Wild Steelbloom |
| Winter | Nothing outdoors. Greenhouse, mining, crafting, animals |

Today every crop grows all year. Seasons switch on after the greenhouse exists, so winter never
feels empty. The rare herbs (Kingsblood, Liferoot) only come from seeds found foraging and mining.

## Money

| Selling | Buying |
|---|---|
| Innkeeper Farley, Lion's Pride Inn: paid now (after Market Day) | Brog Hamfist: seeds, Woodcutter's Axe (1s), Mining Pick (2s) |
| Shipping Crate: collected each morning by Farley's cart (after The First Harvest) | Ma's starter pouch: 8 Silverleaf seeds **(built)** |
| Weekly orders on the Goldshire board | Smith Argus: tool upgrades, bars |
| Special orders from townsfolk, which also earn friendship | Ma Stonefield: land, buildings, animals |

Rough earnings: week 1 about 1s a day (Silverleaf, Peacebloom); end of spring 10–20s a day
(Mageroyal, pumpkins, animal goods); year 2, 1g or more a day.

## Tools and upgrades

Every tool is a button on the hotbar, unlocked by its quest **(built: shovel, watering can,
horse, axe, fences, mining pick)**.

| Tool | Copper (Smith Argus) | Iron | Gold |
|---|---|---|---|
| Watering Can | Waters 3 in a line | 5 | 3×3 |
| Shovel | Tills 3 in a row | 5 | 3×3 |
| Axe | Fells in 2 swings | 1 swing; clears stumps and logs | Splits Heartwood |
| Mining Pick | Copper and tin | Iron and silver | Gold, mithril, gems |

Each upgrade costs gold plus bars, and the tool is away at the smith for one day. Bars come from
Fargodeep ore, smelted in a furnace by the barn.

## Fences and animals

- **Fences** **(built)**: draw a line with Build Fence. Each 4 yd section of Elwynn split rail
  costs 2 Simple Wood. Without enough wood, the rest is marked with posts to build later.
- **Gates**: a fence section that opens. Animals stay inside a closed pen.
- **The first animals come from quests** (hens from Good Fences, cow and sheep from Eggs for
  Breakfast). More are bought from Ma Stonefield once you have a coop or barn:

| Animal | Price | Needs | Gives |
|---|---|---|---|
| Chicken | 8s | Coop | Small Egg daily **(built)** |
| Sheep | 15s | Barn | Wool every 3 days **(built daily)** |
| Cow | 20s | Barn | Ice Cold Milk daily **(built)** |
| Pig | 25s | Barn | Truffles in summer and autumn |

- **Care**: pet it each day and keep it fed (hay from tall grass, cut with the shovel). More
  hearts give better goods: Large Egg, Gold Milk, Fine Wool.
- **Breeding**: two adults of the same kind with 5 hearts and room in the barn. A baby arrives in
  7 days and grows up in 7 more. You name it.
- **Selling animals** to Ma: half the buy price, plus a bonus for hearts.
- **Coop and barn upgrades** add space: 4, then 8, then 12 animals.

## Mining

Fargodeep Mine, south-west of the farm **(built: copper veins that refill each morning)**.

- Floors 1–10: copper and tin. Floors 11–20: iron and silver; kobolds are scared off with a
  lantern, not fought. Floors 21–30: gold and gems (Malachite, Tigerseye, Shadowgem).
- Every 5th floor has a lift. Gems go to the Goldshire jeweller, or into furniture and gifts.

## Getting around

| Step | How | |
|---|---|---|
| On foot | Start | 7 yd/s |
| Farm Horse | Quest: Early to Bed | 14 yd/s **(built, hotbar slot 5)** |
| Saddlebags | 20s, Eastvale stables | 4 more bag slots while riding |
| Swift Brown Steed | 5g, Eastvale stables | Faster; carries a full harvest in one trip |

Goldshire is about 600 yd from the farm: 40 s on the horse instead of 85 s on foot.

## The house

| Stage | Cost | |
|---|---|---|
| Stonefield farmhouse | Start | A bed **(built)**, a fireplace, one room |
| Kitchen | 10s + 50 wood | Cook buff foods |
| Second floor | 1g + 150 wood | Bigger bed, a chest, room for furniture |
| Cellar | 3g | Barrels to age cheese and mead |

## Goals: the Stonefield Restoration

Ma Stonefield's ledger has 6 pages, each a bundle. Finishing a page fixes something:

| Page | Bundle | Reward |
|---|---|---|
| The Field | 5 of each spring crop | Sprinklers recipe |
| The Pen | Egg, Milk, Wool | Free barn upgrade |
| The Mine | 20 copper bars, 5 tin | The mine lift |
| The Woods | 100 wood, 10 Heartwood | The bridge to Eastvale reopens |
| The Town | 5 gifts to Goldshire folk | Goldshire Harvest Fair each autumn |
| The Lake | 5 fish from Crystal Lake | The fishing rod upgrade |

A finished ledger (around the end of year 1) brings back the Stonefields' harvest festival. After
it the game is open-ended: the best farm, every recipe, every animal at 10 hearts.

## Order to build

1. **The quest chain 1–10** and the ! / ? markers (this round).
2. **Energy and a day plan**: an energy bar from tool use, refilled by sleep and food.
3. **Smith and bars**: furnace, tool upgrades.
4. **Land**: field deed, sprinklers, greenhouse.
5. **Animals**: buy, pet for hearts, feed hay, breed, sell.
6. **House upgrades and furniture.**
7. **Saddlebags and the Swift Brown Steed.**
8. **Seasons and seed lists that change with them**, plus Wheat and Corn.
9. **Mine floors**, the lift, **the ledger** and its rewards.
