//! One farmer's farm: the plot's tiles, what grows on them, the ore veins in the mine, the bag,
//! the purse and the clock. Every action either changes the farm or says why not.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::bag::{Bag, Item};
use crate::clock::{Clock, Weather};
use crate::crops::Crop;
use crate::quests::{Deed, Goal, Marker, Quests, Unlock, Villager, QUESTS};

/// Copper a new farmer starts with: 2 silver.
pub const START_MONEY: u32 = 200;

/// Days a felled tree takes to grow back.
pub const TREE_REGROW_DAYS: u32 = 3;

/// A grove tree's stage after felling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeStage {
    Tree,
    /// The day it was felled.
    Stump,
    /// Sprouting again until it is full grown.
    Sapling,
}

/// One plot tile.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tile {
    /// What grows here and for how many watered days.
    pub crop: Option<(Crop, u32)>,
    /// Watered today: tonight the crop grows a day.
    pub watered: bool,
}

impl Tile {
    /// A crop that has grown all its days.
    pub fn ripe(&self) -> Option<Crop> {
        self.crop
            .filter(|&(c, age)| age >= c.def().days)
            .map(|(c, _)| c)
    }
}

/// Why an action was refused, as the player is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    NoSuchTile,
    Occupied,
    NoSeeds(Crop),
    NothingPlanted,
    AlreadyWatered,
    NotRipe,
    BagFull,
    NotEnoughMoney,
    NotSold,
    NotCarried,
    Mined,
    NoSuchVein,
    Picked,
    NoSuchTree,
    Felled,
    NoSuchAnimal,
    Collected,
    NoWood,
    NoSuchFence,
    /// The tool or unlock it takes is not the farmer's yet.
    Locked(Unlock),
    /// The quest is not in the log, or not done yet.
    QuestNotDone,
    AlreadyOwned,
}

impl Reject {
    pub fn text(&self) -> String {
        match self {
            Reject::NoSuchTile => "There is no soil there.".into(),
            Reject::Occupied => "Something is already growing there.".into(),
            Reject::NoSeeds(c) => format!("You have no {} Seeds.", c.name()),
            Reject::NothingPlanted => "Nothing is planted there.".into(),
            Reject::AlreadyWatered => "That's already watered today.".into(),
            Reject::NotRipe => "It isn't ripe yet.".into(),
            Reject::BagFull => "Inventory is full.".into(),
            Reject::NotEnoughMoney => "You don't have enough money.".into(),
            Reject::NotSold => "The store doesn't sell that.".into(),
            Reject::NotCarried => "You don't have that.".into(),
            Reject::Mined => "That vein is spent until tomorrow.".into(),
            Reject::NoSuchVein => "There is no vein there.".into(),
            Reject::NoSuchTree => "There is no tree there.".into(),
            Reject::NoSuchAnimal => "There is no animal there.".into(),
            Reject::Collected => "Come back tomorrow.".into(),
            Reject::NoWood => format!("You need {FENCE_WOOD} Simple Wood. Chop an oak!"),
            Reject::NoSuchFence => "There is no fence there.".into(),
            Reject::Felled => "It hasn't grown back yet.".into(),
            Reject::Picked => "Someone picked that already. It grows back tomorrow.".into(),
            Reject::Locked(u) => match u {
                Unlock::WateringCan => "You need a watering can. Ma Stonefield has one.".into(),
                Unlock::Horse => "You don't have a horse yet.".into(),
                Unlock::Axe => "You need an axe. Brog Hamfist in Goldshire sells them.".into(),
                Unlock::Fence => "You don't know how to build fences yet.".into(),
                Unlock::MiningPick => {
                    "You need a Mining Pick. Brog Hamfist in Goldshire sells them.".into()
                }
                _ => "You can't do that yet.".into(),
            },
            Reject::QuestNotDone => "That isn't done yet.".into(),
            Reject::AlreadyOwned => "You already have one.".into(),
        }
    }
}

/// What a night did, for the morning's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayReport {
    /// The new day's date line.
    pub date: String,
    pub weather: Weather,
    /// Crops that grew overnight, and how many of them are now ripe.
    pub grew: usize,
    pub ripe: usize,
    /// The farmer was still up at 2:00 and passed out.
    pub passed_out: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Farm {
    pub clock: Clock,
    pub weather: Weather,
    /// Copper.
    pub money: u32,
    pub bag: Bag,
    pub tiles: Vec<Tile>,
    /// Veins mined today, by index.
    pub mined: Vec<bool>,
    /// Draws the weather.
    pub seed: u64,
    /// Crops harvested, ever.
    pub harvested: u32,
    /// Wild herbs picked today, by index.
    #[serde(default)]
    pub foraged: Vec<bool>,
    /// The companion that follows the farmer, by name; `None` is the farm dog.
    #[serde(default)]
    pub companion: Option<String>,
    /// The day each grove tree was felled, while it grows back.
    #[serde(default)]
    pub felled: Vec<Option<u32>>,
    /// Plot cells the farmer dug beyond the starting plot, as (furrow, step) on the field's grid;
    /// their tiles follow the starting ones in `tiles`.
    #[serde(default)]
    pub dug: Vec<(i32, i32)>,
    /// Each farm animal's product collected today, by index.
    #[serde(default)]
    pub collected: Vec<bool>,
    /// Fences the farmer built.
    #[serde(default)]
    pub fences: Vec<Fence>,
    #[serde(default)]
    pub quests: Quests,
    /// Tools and the like, won from quests or bought; the shovel is always there.
    #[serde(default)]
    pub unlocked: BTreeSet<Unlock>,
}

/// A fence the farmer built: where it stands, which way it runs, and which look it has.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Fence {
    pub at: [f32; 3],
    pub facing: f32,
    pub style: u32,
    /// Put up; `false` is a planned fence, marked out and waiting for wood.
    #[serde(default = "built")]
    pub built: bool,
}

fn built() -> bool {
    true
}

/// Simple Wood one fence takes, and gives back when it is pulled up.
pub const FENCE_WOOD: u32 = 2;

impl Farm {
    /// A new farm of `tiles` empty tiles and `veins` ore veins: 2 silver and a few seeds.
    pub fn new(tiles: usize, veins: usize, seed: u64) -> Self {
        let mut bag = Bag::default();
        bag.add(Item::Seeds(Crop::Silverleaf), 8);
        Self {
            clock: Clock::default(),
            weather: Weather::Sunny,
            money: START_MONEY,
            bag,
            tiles: vec![Tile::default(); tiles],
            mined: vec![false; veins],
            seed,
            harvested: 0,
            foraged: Vec::new(),
            companion: None,
            felled: Vec::new(),
            dug: Vec::new(),
            collected: Vec::new(),
            fences: Vec::new(),
            quests: Quests::default(),
            unlocked: BTreeSet::new(),
        }
    }

    pub fn has(&self, u: Unlock) -> bool {
        self.unlocked.contains(&u)
    }

    /// Counts `deed` for the quests in the log: the quests it moved on, with their new counts.
    pub fn note(&mut self, deed: Deed) -> Vec<(usize, u32)> {
        self.quests.note(deed)
    }

    /// Whether quest `q`, in the log, can be turned in.
    pub fn quest_ready(&self, q: usize) -> bool {
        if !self.quests.is_active(q) {
            return false;
        }
        match QUESTS[q].goal {
            Goal::Do(_, need, _) => self.quests.count(q) >= need,
            Goal::Bring(item, n) => self.bag.count(item) >= n,
            Goal::Visit => true,
        }
    }

    /// What `who` shows over their head.
    pub fn marker(&self, who: Villager) -> Marker {
        let ending = (self.quests.active.iter()).filter(|&&(q, _)| QUESTS[q].ender == who);
        let mut waiting = false;
        for &(q, _) in ending {
            if self.quest_ready(q) {
                return Marker::TurnIn;
            }
            waiting = true;
        }
        if self.quests.offer(who).is_some() {
            Marker::Offer
        } else if waiting {
            Marker::Waiting
        } else {
            Marker::None
        }
    }

    /// Turn in quest `q`: hand over what it asks for, take the reward.
    pub fn turn_in(&mut self, q: usize) -> Result<(), Reject> {
        if !self.quest_ready(q) {
            return Err(Reject::QuestNotDone);
        }
        let quest = &QUESTS[q];
        if let Goal::Bring(item, n) = quest.goal {
            self.bag.take(item, n);
        }
        self.money += quest.money;
        if let Some(u) = quest.unlocks {
            self.unlocked.insert(u);
        }
        self.quests.abandon(q);
        self.quests.done = self.quests.done.max(q + 1);
        Ok(())
    }

    /// Buy a tool for `price` copper.
    pub fn buy_tool(&mut self, u: Unlock, price: u32) -> Result<(), Reject> {
        if self.has(u) {
            return Err(Reject::AlreadyOwned);
        }
        if price > self.money {
            return Err(Reject::NotEnoughMoney);
        }
        self.money -= price;
        self.unlocked.insert(u);
        Ok(())
    }

    /// Fit a loaded farm to a layout of `tiles` tiles, `veins` veins and `wild` wild herbs.
    pub fn fit(&mut self, tiles: usize, veins: usize, wild: usize) {
        self.tiles.resize(tiles + self.dug.len(), Tile::default());
        self.mined.resize(veins, false);
        self.foraged.resize(wild, false);
    }

    /// Fit a loaded farm to `animals` farm animals.
    pub fn fit_animals(&mut self, animals: usize) {
        self.collected.resize(animals, false);
    }

    /// Dig a new plot cell `cell`; its tile is the new last one. `false` if it is already dug.
    pub fn till(&mut self, cell: (i32, i32)) -> bool {
        if self.dug.contains(&cell) {
            return false;
        }
        self.dug.push(cell);
        self.tiles.push(Tile::default());
        true
    }

    /// Collect farm animal `a`'s `item` for the day: an egg, a pail of milk, a fleece.
    pub fn collect(&mut self, a: usize, item: Item) -> Result<(), Reject> {
        match self.collected.get(a) {
            None => return Err(Reject::NoSuchAnimal),
            Some(true) => return Err(Reject::Collected),
            Some(false) => {}
        }
        if !self.bag.add(item, 1) {
            return Err(Reject::BagFull);
        }
        self.collected[a] = true;
        Ok(())
    }

    /// Build a fence for [`FENCE_WOOD`] Simple Wood, or, without the wood, mark it out as planned.
    /// Whether it went up.
    pub fn build(&mut self, mut fence: Fence) -> bool {
        fence.built = self.bag.take(Item::Wood, FENCE_WOOD);
        self.fences.push(fence);
        fence.built
    }

    /// Put up planned fence `f` now there is wood for it.
    pub fn finish_fence(&mut self, f: usize) -> Result<(), Reject> {
        match self.fences.get(f) {
            None => return Err(Reject::NoSuchFence),
            Some(fence) if fence.built => return Ok(()),
            Some(_) => {}
        }
        if !self.bag.take(Item::Wood, FENCE_WOOD) {
            return Err(Reject::NoWood);
        }
        self.fences[f].built = true;
        Ok(())
    }

    /// Pull up fence `f`, getting its wood back if it was built.
    pub fn unbuild(&mut self, f: usize) -> Result<(), Reject> {
        let Some(fence) = self.fences.get(f) else {
            return Err(Reject::NoSuchFence);
        };
        if fence.built && !self.bag.add(Item::Wood, FENCE_WOOD) {
            return Err(Reject::BagFull);
        }
        self.fences.remove(f);
        Ok(())
    }

    /// Fit a loaded farm to a grove of `trees` trees.
    pub fn fit_grove(&mut self, trees: usize) {
        self.felled.resize(trees, None);
    }

    /// Grove tree `t` as it stands today.
    pub fn tree(&self, t: usize) -> TreeStage {
        match self.felled.get(t).copied().flatten() {
            None => TreeStage::Tree,
            Some(day) if self.clock.day >= day + TREE_REGROW_DAYS => TreeStage::Tree,
            Some(day) if self.clock.day > day => TreeStage::Sapling,
            Some(_) => TreeStage::Stump,
        }
    }

    /// Fell grove tree `t`: two to four Simple Wood, and a stump that sprouts tomorrow and is a
    /// tree again in [`TREE_REGROW_DAYS`] days.
    pub fn chop(&mut self, t: usize) -> Result<u32, Reject> {
        if t >= self.felled.len() {
            return Err(Reject::NoSuchTree);
        }
        if self.tree(t) != TreeStage::Tree {
            return Err(Reject::Felled);
        }
        let salt = u64::from(self.clock.day) << 20 | 2 << 16 | t as u64;
        let n = 2 + crate::rng::below(self.seed, salt, 3) as u32;
        if !self.bag.add(Item::Wood, n) {
            return Err(Reject::BagFull);
        }
        self.felled[t] = Some(self.clock.day);
        Ok(n)
    }

    /// Pick wild herb `w`, a `crop` plant: one of it, and half the time a seed to plant at home.
    /// It grows back tomorrow.
    pub fn forage(&mut self, w: usize, crop: Crop) -> Result<(u32, bool), Reject> {
        match self.foraged.get(w) {
            None => return Err(Reject::NoSuchVein),
            Some(true) => return Err(Reject::Picked),
            Some(false) => {}
        }
        let salt = u64::from(self.clock.day) << 20 | 1 << 16 | w as u64;
        let seed = crate::rng::below(self.seed, salt, 2) == 0;
        if !self.bag.fits(Item::Produce(crop), 1) || (seed && !self.bag.fits(Item::Seeds(crop), 1))
        {
            return Err(Reject::BagFull);
        }
        self.bag.add(Item::Produce(crop), 1);
        if seed {
            self.bag.add(Item::Seeds(crop), 1);
        }
        self.foraged[w] = true;
        Ok((1, seed))
    }

    fn tile(&mut self, i: usize) -> Result<&mut Tile, Reject> {
        self.tiles.get_mut(i).ok_or(Reject::NoSuchTile)
    }

    /// Sow a `crop` seed in empty tile `i`. Rain waters it at once.
    pub fn plant(&mut self, i: usize, crop: Crop) -> Result<(), Reject> {
        let raining = self.weather.waters();
        if self.tile(i)?.crop.is_some() {
            return Err(Reject::Occupied);
        }
        if !self.bag.take(Item::Seeds(crop), 1) {
            return Err(Reject::NoSeeds(crop));
        }
        let tile = self.tile(i)?;
        tile.crop = Some((crop, 0));
        tile.watered = raining;
        Ok(())
    }

    /// Water tile `i`'s crop for today.
    pub fn water(&mut self, i: usize) -> Result<(), Reject> {
        let tile = self.tile(i)?;
        if tile.crop.is_none() {
            return Err(Reject::NothingPlanted);
        }
        if tile.ripe().is_some() || tile.watered {
            return Err(Reject::AlreadyWatered);
        }
        tile.watered = true;
        Ok(())
    }

    /// Pick tile `i`'s ripe crop into the bag; the tile is empty again.
    pub fn harvest(&mut self, i: usize) -> Result<(Crop, u32), Reject> {
        let tile = *self.tile(i)?;
        let crop = match (tile.crop, tile.ripe()) {
            (None, _) => return Err(Reject::NothingPlanted),
            (Some(_), None) => return Err(Reject::NotRipe),
            (_, Some(c)) => c,
        };
        let n = crop.def().harvest;
        if !self.bag.add(Item::Produce(crop), n) {
            return Err(Reject::BagFull);
        }
        *self.tile(i)? = Tile::default();
        self.harvested += n;
        Ok((crop, n))
    }

    /// Mine vein `v`: one to three Copper Ore, then it is spent until tomorrow.
    pub fn mine(&mut self, v: usize) -> Result<u32, Reject> {
        match self.mined.get(v) {
            None => return Err(Reject::NoSuchVein),
            Some(true) => return Err(Reject::Mined),
            Some(false) => {}
        }
        let n = 1 + crate::rng::below(self.seed, u64::from(self.clock.day) << 16 | v as u64, 3);
        let n = n as u32;
        if !self.bag.add(Item::CopperOre, n) {
            return Err(Reject::BagFull);
        }
        self.mined[v] = true;
        Ok(n)
    }

    /// Buy `n` of `item` from the general store.
    pub fn buy(&mut self, item: Item, n: u32) -> Result<(), Reject> {
        let price = item.buy_price().ok_or(Reject::NotSold)? * n;
        if price > self.money {
            return Err(Reject::NotEnoughMoney);
        }
        if !self.bag.add(item, n) {
            return Err(Reject::BagFull);
        }
        self.money -= price;
        Ok(())
    }

    /// Sell `n` of `item` to the general store; the copper it fetched.
    pub fn sell(&mut self, item: Item, n: u32) -> Result<u32, Reject> {
        if !self.bag.take(item, n) {
            return Err(Reject::NotCarried);
        }
        let paid = item.sell_price() * n;
        self.money += paid;
        Ok(paid)
    }

    /// Run `seconds` of the day; past 2:00 the farmer passes out and wakes tomorrow.
    pub fn tick(&mut self, seconds: f32) -> Option<DayReport> {
        if self.clock.advance(seconds) {
            let mut report = self.sleep();
            report.passed_out = true;
            Some(report)
        } else {
            None
        }
    }

    /// Sleep until morning: watered crops grow a day, veins refill, a new day's weather rolls in,
    /// and rain waters everything.
    pub fn sleep(&mut self) -> DayReport {
        let mut grew = 0;
        for tile in &mut self.tiles {
            if let (Some((crop, age)), true) = (tile.crop, tile.watered) {
                if age < crop.def().days {
                    tile.crop = Some((crop, age + 1));
                    grew += 1;
                }
            }
            tile.watered = false;
        }
        self.mined.iter_mut().for_each(|m| *m = false);
        self.foraged.iter_mut().for_each(|f| *f = false);
        self.collected.iter_mut().for_each(|c| *c = false);
        self.clock.next_day();
        self.weather = Weather::roll(self.seed, self.clock.day, self.clock.season());
        if self.weather.waters() {
            for tile in &mut self.tiles {
                tile.watered = tile.crop.is_some() && tile.ripe().is_none();
            }
        }
        DayReport {
            date: self.clock.date(),
            weather: self.weather,
            grew,
            ripe: self.tiles.iter().filter(|t| t.ripe().is_some()).count(),
            passed_out: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A farm with a few Peacebloom seeds too.
    fn farm() -> Farm {
        let mut f = Farm::new(4, 2, 1);
        f.bag.add(Item::Seeds(Crop::Peacebloom), 4);
        f
    }

    /// Sleep until a dry morning, so watering is up to the farmer.
    fn sleep_dry(f: &mut Farm) {
        loop {
            f.sleep();
            if !f.weather.waters() {
                return;
            }
        }
    }

    #[test]
    fn a_watered_crop_ripens_and_is_harvested_into_the_bag() {
        let mut f = farm();
        f.plant(0, Crop::Silverleaf).unwrap();
        assert_eq!(f.bag.count(Item::Seeds(Crop::Silverleaf)), 7);
        for _ in 0..Crop::Silverleaf.def().days {
            assert_eq!(f.harvest(0), Err(Reject::NotRipe));
            let _ = f.water(0);
            sleep_dry(&mut f);
        }
        assert_eq!(f.tiles[0].ripe(), Some(Crop::Silverleaf));
        assert_eq!(f.water(0), Err(Reject::AlreadyWatered));
        assert_eq!(f.harvest(0), Ok((Crop::Silverleaf, 2)));
        assert_eq!(f.bag.count(Item::Produce(Crop::Silverleaf)), 2);
        assert_eq!(f.tiles[0], Tile::default());
        assert_eq!(f.harvest(0), Err(Reject::NothingPlanted));
    }

    #[test]
    fn a_dry_crop_does_not_grow() {
        let mut f = farm();
        f.plant(1, Crop::Peacebloom).unwrap();
        f.tiles[1].watered = false;
        sleep_dry(&mut f);
        assert_eq!(f.tiles[1].crop, Some((Crop::Peacebloom, 0)));
    }

    #[test]
    fn rain_waters_every_growing_crop() {
        let mut f = farm();
        f.plant(0, Crop::Peacebloom).unwrap();
        for _ in 0..40 {
            f.sleep();
            if f.weather.waters() {
                assert!(f.tiles[0].watered || f.tiles[0].ripe().is_some());
                return;
            }
        }
        panic!("no rain in 40 days");
    }

    #[test]
    fn planting_needs_an_empty_tile_and_a_seed() {
        let mut f = farm();
        assert_eq!(f.plant(9, Crop::Peacebloom), Err(Reject::NoSuchTile));
        assert_eq!(
            f.plant(0, Crop::Pumpkin),
            Err(Reject::NoSeeds(Crop::Pumpkin))
        );
        f.plant(0, Crop::Peacebloom).unwrap();
        assert_eq!(f.plant(0, Crop::Peacebloom), Err(Reject::Occupied));
    }

    #[test]
    fn the_store_takes_money_for_seeds_and_pays_for_crops() {
        let mut f = farm();
        assert_eq!(f.buy(Item::Produce(Crop::Pumpkin), 1), Err(Reject::NotSold));
        f.buy(Item::Seeds(Crop::Pumpkin), 1).unwrap();
        assert_eq!(f.money, START_MONEY - 150);
        assert_eq!(
            f.buy(Item::Seeds(Crop::Pumpkin), 1),
            Err(Reject::NotEnoughMoney)
        );
        f.bag.add(Item::Produce(Crop::Peacebloom), 3);
        assert_eq!(f.sell(Item::Produce(Crop::Peacebloom), 3), Ok(36));
        assert_eq!(f.sell(Item::CopperOre, 1), Err(Reject::NotCarried));
    }

    #[test]
    fn a_vein_gives_ore_once_a_day() {
        let mut f = farm();
        let n = f.mine(0).unwrap();
        assert!((1..=3).contains(&n));
        assert_eq!(f.bag.count(Item::CopperOre), n);
        assert_eq!(f.mine(0), Err(Reject::Mined));
        assert_eq!(f.mine(5), Err(Reject::NoSuchVein));
        f.sleep();
        assert!(f.mine(0).is_ok());
    }

    #[test]
    fn a_wild_herb_is_picked_once_a_day_and_sometimes_gives_a_seed() {
        let mut f = farm();
        f.fit(4, 2, 3);
        let mut seeds = 0;
        for _ in 0..40 {
            let (n, seed) = f.forage(1, Crop::Peacebloom).unwrap();
            assert_eq!(n, 1);
            seeds += u32::from(seed);
            assert_eq!(f.forage(1, Crop::Peacebloom), Err(Reject::Picked));
            f.sleep();
            f.bag = Bag::default();
        }
        assert!((5..35).contains(&seeds), "about half give a seed: {seeds}");
        assert_eq!(f.forage(7, Crop::Peacebloom), Err(Reject::NoSuchVein));
    }

    #[test]
    fn a_tree_falls_to_a_stump_sprouts_and_grows_back() {
        let mut f = farm();
        f.fit_grove(2);
        let n = f.chop(1).unwrap();
        assert!((2..=4).contains(&n));
        assert_eq!(f.bag.count(Item::Wood), n);
        assert_eq!(f.tree(1), TreeStage::Stump);
        assert_eq!(f.chop(1), Err(Reject::Felled));
        assert_eq!(f.tree(0), TreeStage::Tree);
        f.sleep();
        assert_eq!(f.tree(1), TreeStage::Sapling);
        for _ in 1..TREE_REGROW_DAYS {
            f.sleep();
        }
        assert_eq!(f.tree(1), TreeStage::Tree);
        assert!(f.chop(1).is_ok());
        assert_eq!(f.chop(5), Err(Reject::NoSuchTree));
    }

    #[test]
    fn animals_give_once_a_day_dug_cells_add_tiles_and_fences_cost_wood() {
        let mut f = farm();
        f.fit_animals(2);
        f.collect(0, Item::Egg).unwrap();
        assert_eq!(f.collect(0, Item::Egg), Err(Reject::Collected));
        assert_eq!(f.collect(3, Item::Egg), Err(Reject::NoSuchAnimal));
        f.sleep();
        assert!(f.collect(0, Item::Egg).is_ok());
        assert_eq!(f.bag.count(Item::Egg), 2);

        let tiles = f.tiles.len();
        assert!(f.till((5, 2)));
        assert!(!f.till((5, 2)));
        assert_eq!(f.tiles.len(), tiles + 1);
        f.fit(tiles, 2, 0);
        assert_eq!(
            f.tiles.len(),
            tiles + 1,
            "a loaded farm keeps its dug tiles"
        );

        let fence = Fence {
            at: [1.0, 2.0, 3.0],
            facing: 0.5,
            style: 6151,
            built: true,
        };
        assert!(!f.build(fence), "no wood: planned");
        assert!(!f.fences[0].built);
        assert_eq!(f.finish_fence(0), Err(Reject::NoWood));
        f.bag.add(Item::Wood, 3);
        f.finish_fence(0).unwrap();
        assert!(f.fences[0].built);
        assert_eq!(f.bag.count(Item::Wood), 1);
        f.unbuild(0).unwrap();
        assert_eq!(f.bag.count(Item::Wood), 3);
        assert!(f.fences.is_empty());
    }

    #[test]
    fn staying_up_past_two_passes_out_into_the_next_day() {
        let mut f = farm();
        assert!(f.tick(100.0).is_none());
        let report = f.tick(10_000.0).expect("passed out");
        assert!(report.passed_out);
        assert_eq!(f.clock.day, 2);
        assert_eq!(f.clock.face(), "06:00");
    }

    #[test]
    fn the_first_quest_takes_planting_and_gives_the_watering_can() {
        let mut f = Farm::new(8, 1, 1);
        assert!(!f.has(Unlock::WateringCan));
        assert_eq!(f.marker(Villager::Ma), Marker::Offer);
        assert!(f.quests.accept(0));
        assert_eq!(f.marker(Villager::Ma), Marker::Waiting);
        assert_eq!(f.turn_in(0), Err(Reject::QuestNotDone));
        for i in 0..8 {
            f.plant(i, Crop::Silverleaf).unwrap();
            f.note(Deed::Plant);
        }
        assert_eq!(f.bag.count(Item::Seeds(Crop::Silverleaf)), 0);
        assert_eq!(f.marker(Villager::Ma), Marker::TurnIn);
        f.turn_in(0).unwrap();
        assert!(f.has(Unlock::WateringCan));
        assert_eq!(f.marker(Villager::Ma), Marker::Offer);
    }

    #[test]
    fn a_bring_quest_takes_the_goods_at_the_turn_in() {
        let mut f = Farm::new(1, 1, 1);
        f.quests.done = 5;
        assert_eq!(f.marker(Villager::Brog), Marker::Offer);
        f.quests.accept(5);
        assert_eq!(f.marker(Villager::Farley), Marker::Waiting);
        f.bag.add(Item::Produce(Crop::Silverleaf), 12);
        assert_eq!(f.marker(Villager::Farley), Marker::TurnIn);
        let money = f.money;
        f.turn_in(5).unwrap();
        assert_eq!(f.bag.count(Item::Produce(Crop::Silverleaf)), 2);
        assert_eq!(f.money, money + 80);
        assert!(f.has(Unlock::ShippingCrate));
        assert_eq!(f.marker(Villager::Brog), Marker::Offer);
    }

    #[test]
    fn a_tool_is_bought_once() {
        let mut f = Farm::new(1, 1, 1);
        f.buy_tool(Unlock::Axe, 100).unwrap();
        assert_eq!(f.money, START_MONEY - 100);
        assert_eq!(f.buy_tool(Unlock::Axe, 100), Err(Reject::AlreadyOwned));
        assert_eq!(
            f.buy_tool(Unlock::MiningPick, 500),
            Err(Reject::NotEnoughMoney)
        );
    }
}
