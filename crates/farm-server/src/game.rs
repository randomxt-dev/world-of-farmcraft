//! One farmer's session in the world: the farm's rules shown through WoW's own objects. Soil and
//! crops are gameobjects on the plot, ripe crops carry Herbalism's lock and veins Mining's, the
//! hay bed sleeps, the shipping crate buys the harvest, Brog Hamfist's stock UI sells seeds, and a
//! wolf pup follows the farmer around.

use std::sync::Arc;
use std::time::Duration;

use farm_rules::save::Character;
use farm_rules::{Crop, DayReport, Farm, Item, Reject, Weather};

use crate::layout::{Layout, Role, PETS};
use crate::saves::Saves;
use crate::world::creatures::{self, Kind, Look, Shown};
use crate::world::items::{self, Held};
use crate::world::objects::{self, Template, Use};
use crate::world::packets::{self, Ware};
use crate::world::player;
use crate::world::socket::{Outbox, Packet};
use crate::world::spells::{self, CastRequest, Target};

// Gameobject entries: soil, each crop's growing stages and its ripe plant, and the fixtures.
const SOIL_DRY: u32 = 50_000;
const SOIL_WET: u32 = 50_001;
const CROP_BASE: u32 = 51_000;
/// Within a crop's hundred entries, its ripe plant.
const RIPE: u32 = 99;
const VEIN: u32 = 52_000;
/// Wild herbs, one entry per crop from here.
const WILD_BASE: u32 = 53_000;
const BED: u32 = 52_001;
const SHIPPING: u32 = 52_002;
// Creature entries: villagers by layout index, and the farm dog.
const NPC_BASE: u32 = 60_000;
/// The companion, one entry per pet so the client's name cache never mixes them up.
const COMPANION_BASE: u32 = 61_000;

/// `HIGHGUID_GAMEOBJECT` and `HIGHGUID_UNIT`.
const HIGH_GAMEOBJECT: u64 = 0xF110;
const HIGH_UNIT: u64 = 0xF130;

// `GameObjectDisplayInfo.dbc` looks.
/// `World\Azeroth\Duskwood\PassiveDoodads\Tombs\DirtMound01.mdx`: turned earth.
const DIRT: u32 = 20;
/// `World\Lordaeron\TirisfalGlade\PassiveDoodads\Graves\TirisfallGraveDirtMound02.mdx`: dark,
/// damp earth.
const DAMP_DIRT: u32 = 498;
const COPPER_VEIN: u32 = 310;
/// `World\Azeroth\Elwynn\PassiveDoodads\Haystacks\haystack01.mdx`.
const HAYSTACK: u32 = 251;
/// `World\Generic\Human\Passive Doodads\Crates\CrateGrain01.mdx`.
const GRAIN_CRATE: u32 = 347;

/// The dirt mounds are grave-sized; a tile's is smaller, and the damp mound is the bigger model.
const SOIL_SCALE: f32 = 0.42;
const WET_SOIL_SCALE: f32 = 0.34;
/// How close the farmer must stand to work a tile or talk to someone.
const REACH: f32 = 6.0;
/// How long each job's cast bar runs.
const PLANT_TIME: Duration = Duration::from_millis(1500);
const HARVEST_TIME: Duration = Duration::from_millis(1500);
const MINE_TIME: Duration = Duration::from_millis(2500);
/// How long the farmer lies in the hay before morning comes.
const SLEEP_TIME: Duration = Duration::from_millis(3000);
/// How often the game is written to disk while playing.
const AUTOSAVE: Duration = Duration::from_secs(60);
/// The dog trots this far behind and runs to catch up beyond `DOG_RUN`.
const DOG_HEEL: f32 = 2.5;
const DOG_RUN: f32 = 10.0;

/// A crop's look on the plot.
fn crop_display(crop: Crop) -> u32 {
    match crop {
        Crop::Silverleaf => 270,
        Crop::Peacebloom => 269,
        Crop::Earthroot => 414,
        Crop::Mageroyal => 268,
        Crop::Briarthorn => 271,
        Crop::Pumpkin => 60,
        Crop::Watermelon => 332,
    }
}

fn crop_index(crop: Crop) -> u32 {
    Crop::ALL.iter().position(|&c| c == crop).unwrap_or(0) as u32
}

/// The entry of a crop `age` days grown, watered or not, or ripe.
fn crop_entry(crop: Crop, age: u32, watered: bool, ripe: bool) -> u32 {
    let base = CROP_BASE + crop_index(crop) * 100;
    if ripe {
        base + RIPE
    } else {
        base + age.min(40) * 2 + u32::from(watered)
    }
}

/// The template behind a gameobject entry.
fn object_template(entry: u32) -> Option<Template> {
    Some(match entry {
        SOIL_DRY => Template::new("Tilled Soil", DIRT, Use::Goober),
        SOIL_WET => Template::new("Watered Soil", DAMP_DIRT, Use::Goober),
        VEIN => Template::new("Copper Vein", COPPER_VEIN, Use::Mine),
        BED => Template::new("Hay Bed", HAYSTACK, Use::Goober),
        SHIPPING => Template::new("Shipping Crate", GRAIN_CRATE, Use::Goober),
        e if (WILD_BASE..WILD_BASE + Crop::ALL.len() as u32).contains(&e) => {
            let crop = Crop::ALL[(e - WILD_BASE) as usize];
            Template::new(crop.name(), crop_display(crop), Use::Herb)
        }
        e if (CROP_BASE..CROP_BASE + 100 * Crop::ALL.len() as u32).contains(&e) => {
            let crop = Crop::ALL[((e - CROP_BASE) / 100) as usize];
            let n = (e - CROP_BASE) % 100;
            if n == RIPE {
                Template::new(
                    format!("Ripe {}", crop.name()),
                    crop_display(crop),
                    Use::Herb,
                )
            } else {
                let (age, watered) = (n / 2, n % 2 == 1);
                let state = if watered { "watered" } else { "needs water" };
                let name = format!(
                    "{} (day {} of {}, {state})",
                    crop.name(),
                    age + 1,
                    crop.def().days
                );
                Template::new(name, crop_display(crop), Use::Goober)
            }
        }
        _ => return None,
    })
}

/// The size a crop of `crop` grown `age` days shows at: herb nodes are bush-sized at full scale,
/// so a ripe herb stays within its tile; the garden crops are already small.
fn crop_scale(crop: Crop, age: u32) -> f32 {
    let full = match crop {
        Crop::Pumpkin | Crop::Watermelon => 1.0,
        _ => 0.5,
    };
    full * (0.3 + 0.7 * crop.growth(age))
}

/// "1g 2s 3c", leaving out what is zero.
pub fn coins(copper: u32) -> String {
    let (g, s, c) = (copper / 10_000, copper / 100 % 100, copper % 100);
    let mut parts = Vec::new();
    if g > 0 {
        parts.push(format!("{g}g"));
    }
    if s > 0 {
        parts.push(format!("{s}s"));
    }
    if c > 0 || parts.is_empty() {
        parts.push(format!("{c}c"));
    }
    parts.join(" ")
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// What a finished cast does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Plant(usize, Crop),
    Harvest(usize),
    Mine(usize),
    Forage(usize),
}

#[derive(Clone, Copy, Debug)]
struct Cast {
    spell: u32,
    target: Target,
    left: Duration,
    job: Job,
}

/// What an open gossip menu's options do.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Menu {
    /// Take the pet pen's animal, by its index in [`PETS`], as the companion.
    Adopt(usize),
}

/// The greeting texts the gossip windows show.
const TEXT_MA: u32 = 2;
const TEXT_PET: u32 = 3;

const MA_STONEFIELD: &str = "Well, look who's here! That field's been fallow since the \
kobolds took Fargodeep. It's yours to work now, dear.$B$BTurn the soil, plant your seeds, \
and water them every day - rain does it for you. Your seeds sit on your bar: scroll the \
mouse wheel or press a number to pick which ones you sow, and hold Shift to zoom instead. When they're ripe, gather them up and drop \
them in the shipping crate by the house, or sell them to Brog Hamfist at the Goldshire inn. \
He sells seeds too.$B$BAnd mind you get to bed in that hay before two in the morning!";

/// One farmer's world.
pub struct Game {
    me: Character,
    farm: Farm,
    layout: Arc<Layout>,
    saves: Saves,
    out: Outbox,
    /// Where the farmer stands and faces.
    at: [f32; 3],
    facing: f32,
    /// The backpack's farm-goods slots, 24 to 38.
    bag: Vec<Option<Held>>,
    /// The next item guid's number.
    next_item: u32,
    /// Each tile's soil and crop objects, and each vein's, as shown: (guid, entry).
    soil: Vec<(u64, u32)>,
    crops: Vec<Option<(u64, u32)>>,
    veins: Vec<Option<u64>>,
    wild: Vec<Option<u64>>,
    /// The item entries on the action bar, slot by slot, and the slot the farmer has picked.
    bar: Vec<u32>,
    selected: usize,
    /// The hay bed's and the shipping crate's guids.
    fixtures: [u64; 2],
    /// Bumped to give every recreated object a fresh guid.
    generation: u64,
    cast: Option<Cast>,
    menu: Option<Menu>,
    /// The time left lying in the hay.
    asleep: Option<Duration>,
    dog: [f32; 3],
    since_dog: Duration,
    since_save: Duration,
    /// The last hour the clock announced.
    told_hour: u32,
    /// The forced-move counter the teleport ack echoes.
    move_counter: u32,
}

impl Game {
    pub fn new(me: Character, farm: Farm, layout: Arc<Layout>, saves: Saves, out: Outbox) -> Self {
        let mut farm = farm;
        farm.fit(layout.tiles.len(), layout.veins.len(), layout.wild.len());
        let tiles = layout.tiles.len();
        let veins = layout.veins.len();
        let wild = layout.wild.len();
        let at = layout.home.position;
        let told_hour = farm.clock.hour_minute().0;
        Self {
            me,
            farm,
            layout,
            saves,
            out,
            at,
            facing: 0.0,
            bag: vec![None; farm_rules::bag::SLOTS],
            next_item: 1000,
            soil: vec![(0, 0); tiles],
            crops: vec![None; tiles],
            veins: vec![None; veins],
            wild: vec![None; wild],
            bar: Vec::new(),
            selected: 0,
            fixtures: [0; 2],
            generation: 0,
            cast: None,
            menu: None,
            asleep: None,
            dog: [at[0] - 2.0, at[1], at[2]],
            since_dog: Duration::ZERO,
            since_save: Duration::ZERO,
            told_hour,
            move_counter: 0,
        }
    }

    fn send(&self, p: Packet) {
        let _ = self.out.send(p);
    }

    fn say(&self, text: &str) {
        self.send(packets::system_message(text));
    }

    fn notify(&self, text: &str) {
        self.send(spells::notification(text));
    }

    fn guid(&self) -> u64 {
        self.me.guid
    }

    fn clock_packet(&self) -> Packet {
        let (h, m) = self.farm.clock.hour_minute();
        packets::time_speed(
            h,
            m,
            self.farm.clock.day,
            farm_rules::clock::GAME_MINUTES_PER_SECOND,
        )
    }

    fn weather_packet(&self, instant: bool) -> Packet {
        match self.farm.weather {
            Weather::Sunny => packets::weather(0, 0.0, instant),
            Weather::Rain => packets::weather(1, 0.6, instant),
            Weather::Snow => packets::weather(2, 0.6, instant),
        }
    }

    /// Seat the farmer at home and show them the farm.
    pub fn enter(&mut self) {
        let carried = vec![player::pick(self.guid())];
        for p in player::login_packets(
            &self.me,
            self.layout.home,
            self.clock_packet(),
            &carried,
            self.farm.money,
        ) {
            self.send(p);
        }
        self.at = self.layout.home.position;
        self.sync_bag(None, false);
        self.show_world();
        self.send(self.weather_packet(true));
        self.say(&format!(
            "Welcome to Stonefield Farm! It's {}, {} and {}.",
            self.farm.clock.date(),
            self.farm.clock.face(),
            self.farm.weather.name()
        ));
        if self.farm.harvested == 0 {
            self.say("Ma Stonefield by the farmhouse can tell you how farming works.");
        }
    }

    /// A new gameobject guid for template `entry`. The client reads the entry back out of the
    /// guid (bits 24-47, as vmangos packs it) to ask for the template, so it must be there.
    fn fresh_guid(&mut self, entry: u32) -> u64 {
        self.generation += 1;
        HIGH_GAMEOBJECT << 48 | u64::from(entry & 0xFF_FFFF) << 24 | (self.generation & 0xFF_FFFF)
    }

    fn show_world(&mut self) {
        for i in 0..self.layout.tiles.len() {
            self.show_tile(i);
        }
        for v in 0..self.layout.veins.len() {
            self.show_vein(v);
        }
        for w in 0..self.layout.wild.len() {
            self.show_wild(w);
        }
        let fixtures = [
            (BED, self.layout.bed, 0.6),
            (SHIPPING, self.layout.shipping, 1.0),
        ];
        for (i, (entry, at, facing)) in fixtures.into_iter().enumerate() {
            let guid = self.fresh_guid(entry);
            self.fixtures[i] = guid;
            let t = object_template(entry).expect("a fixture");
            self.send(objects::create(guid, entry, &t, at, facing, 1.0));
        }
        for (i, npc) in self.layout.npcs.iter().enumerate() {
            let entry = NPC_BASE + i as u32;
            let shown = Shown {
                guid: HIGH_UNIT << 48 | u64::from(entry) << 24 | i as u64,
                entry,
                position: npc.at,
                orientation: npc.facing,
                owner: None,
            };
            self.send(creatures::create(&shown, &npc.look));
        }
        self.show_companion();
    }

    /// The companion's index in [`PETS`]: the farm dog unless the farmer took another.
    fn companion(&self) -> usize {
        (self.farm.companion.as_deref())
            .and_then(|name| PETS.iter().position(|p| p.0 == name))
            .unwrap_or(0)
    }

    fn dog_guid(&self) -> u64 {
        HIGH_UNIT << 48 | u64::from(COMPANION_BASE + self.companion() as u32) << 24
    }

    fn show_companion(&self) {
        let pet = self.companion();
        let dog = Shown {
            guid: self.dog_guid(),
            entry: COMPANION_BASE + pet as u32,
            position: self.dog,
            orientation: 0.0,
            owner: None,
        };
        self.send(creatures::create(&dog, &companion_look(pet)));
    }

    fn show_wild(&mut self, w: usize) {
        let picked = self.farm.foraged[w];
        match (self.wild[w], picked) {
            (Some(guid), true) => {
                self.send(crate::world::destroy(guid));
                self.wild[w] = None;
            }
            (None, false) => {
                let (crop, at) = self.layout.wild[w];
                let entry = WILD_BASE + crop_index(crop);
                let guid = self.fresh_guid(entry);
                let t = object_template(entry).expect("a wild herb");
                self.send(objects::create(guid, entry, &t, at, w as f32, 0.8));
                self.wild[w] = Some(guid);
            }
            _ => {}
        }
    }

    /// Show tile `i` as the farm has it: dry or watered soil, and its crop at its size.
    fn show_tile(&mut self, i: usize) {
        let tile = self.farm.tiles[i];
        let at = self.layout.tiles[i];
        let soil = if tile.watered { SOIL_WET } else { SOIL_DRY };
        if self.soil[i].1 != soil {
            if self.soil[i].0 != 0 {
                self.send(crate::world::destroy(self.soil[i].0));
            }
            let guid = self.fresh_guid(soil);
            let t = object_template(soil).expect("soil");
            let scale = if tile.watered {
                WET_SOIL_SCALE
            } else {
                SOIL_SCALE
            };
            self.send(objects::create(guid, soil, &t, at, 0.0, scale));
            self.soil[i] = (guid, soil);
        }
        let wanted = tile.crop.map(|(crop, age)| {
            let ripe = tile.ripe().is_some();
            (crop_entry(crop, age, tile.watered, ripe), crop, age)
        });
        match (self.crops[i], wanted) {
            (Some((_, shown)), Some((entry, _, _))) if shown == entry => {}
            (old, wanted) => {
                if let Some((guid, _)) = old {
                    self.send(crate::world::destroy(guid));
                }
                self.crops[i] = wanted.map(|(entry, crop, age)| {
                    let guid = self.fresh_guid(entry);
                    let t = object_template(entry).expect("a crop");
                    let scale = crop_scale(crop, age);
                    self.send(objects::create(guid, entry, &t, at, i as f32, scale));
                    (guid, entry)
                });
            }
        }
    }

    fn show_vein(&mut self, v: usize) {
        let mined = self.farm.mined[v];
        match (self.veins[v], mined) {
            (Some(guid), true) => {
                self.send(crate::world::destroy(guid));
                self.veins[v] = None;
            }
            (None, false) => {
                let guid = self.fresh_guid(VEIN);
                let (at, facing) = self.layout.veins[v];
                let t = object_template(VEIN).expect("a vein");
                self.send(objects::create(guid, VEIN, &t, at, facing, 1.0));
                self.veins[v] = Some(guid);
            }
            _ => {}
        }
    }

    /// Lay the bag out in the backpack: each kind keeps the slots it had, stacks change count in
    /// place, new stacks take the first free slot. `gained` names an item to announce in chat.
    fn sync_bag(&mut self, gained: Option<(Item, u32)>, bought: bool) {
        let owner = self.guid();
        let mut want = self.farm.bag.stacks();
        let mut kept: Vec<Option<Held>> = vec![None; self.bag.len()];
        for (slot, held) in self.bag.iter().enumerate() {
            let Some(h) = *held else {
                continue;
            };
            if let Some(k) = want.iter().position(|&(i, _)| items::entry(i) == h.entry) {
                let (_, count) = want.remove(k);
                if count != h.count {
                    self.send(items::count_change(h.guid, count));
                }
                kept[slot] = Some(Held { count, ..h });
            } else {
                self.send(player::slot_change(owner, h.slot, 0));
                self.send(crate::world::destroy(h.guid));
            }
        }
        for (item, count) in want {
            let Some(slot) = kept.iter().position(Option::is_none) else {
                break;
            };
            self.next_item += 1;
            let held = Held {
                slot: items::BACKPACK + 1 + slot as u8,
                guid: items::item_guid(owner, self.next_item),
                entry: items::entry(item),
                count,
            };
            self.send(Packet::new(
                benilla_protocol::messages::opcode::SMSG_UPDATE_OBJECT,
                items::create_block(&held, owner),
            ));
            self.send(player::slot_change(owner, held.slot, held.guid));
            kept[slot] = Some(held);
        }
        self.bag = kept;
        self.sync_bar();
        if let Some((item, n)) = gained {
            let entry = items::entry(item);
            let slot = (self.bag.iter().flatten())
                .find(|h| h.entry == entry)
                .map_or(items::BACKPACK + 1, |h| h.slot);
            self.send(items::push_result(owner, entry, slot, n, bought));
        }
    }

    /// The action bar as a hotbar: each crop's seeds in their own slot, in crop order, empty while
    /// none are carried, so a slot never changes under the farmer's pick. Sent again only when it
    /// changes; the counts are the client's own, read from the bags.
    fn sync_bar(&mut self) {
        let bar: Vec<u32> = (Crop::ALL.iter())
            .map(|&c| {
                let carried = self.farm.bag.count(Item::Seeds(c)) > 0;
                if carried {
                    items::entry(Item::Seeds(c))
                } else {
                    0
                }
            })
            .collect();
        if bar != self.bar {
            self.send(spells::item_bar(&bar));
            self.bar = bar;
        }
    }

    /// `.sel <n>`: the farmer picked bar slot `n` (from 1) with the wheel or a number key.
    fn select(&mut self, slot: usize) {
        self.selected = slot.saturating_sub(1);
        tracing::debug!("farm: {} picks bar slot {slot}", self.me.name);
    }

    /// The seed in the picked bar slot, if it holds one the farmer still has.
    fn selected_seed(&self) -> Option<Crop> {
        match items::item_of(*self.bar.get(self.selected)?) {
            Some(Item::Seeds(c)) if self.farm.bag.count(Item::Seeds(c)) > 0 => Some(c),
            _ => None,
        }
    }

    fn sync_money(&self) {
        self.send(player::money_change(self.guid(), self.farm.money));
    }

    fn tile_of(&self, object: u64) -> Option<usize> {
        (self.soil.iter().position(|&(g, _)| g == object)).or_else(|| {
            self.crops
                .iter()
                .position(|c| c.is_some_and(|(g, _)| g == object))
        })
    }

    fn near(&self, at: [f32; 3]) -> bool {
        distance(self.at, at) <= REACH
    }

    fn start(&mut self, spell: u32, target: Target, time: Duration, job: Job) {
        self.menu = None;
        let ms = time.as_millis() as u32;
        self.send(spells::spell_start(self.guid(), spell, ms, target));
        self.cast = Some(Cast {
            spell,
            target,
            left: time,
            job,
        });
    }

    fn refuse(&self, r: &Reject) {
        self.notify(&r.text());
    }

    /// A right-click on a lockless object: soil or a growing crop, the hay bed, the crate.
    pub fn use_object(&mut self, object: u64) {
        tracing::info!("farm: {} uses {object:#x}", self.me.name);
        if self.cast.is_some() || self.asleep.is_some() {
            return;
        }
        if let Some(i) = self.tile_of(object) {
            self.work_tile(i, object);
        } else if let Some(v) = self.veins.iter().position(|&g| g == Some(object)) {
            // A client that sends a plain use for the vein still swings the pick.
            if !self.near(self.layout.veins[v].0) {
                return self.notify("You need to be closer.");
            }
            self.start(
                spells::MINING,
                Target::GameObject(object),
                MINE_TIME,
                Job::Mine(v),
            );
        } else if let Some(w) = self.wild.iter().position(|&g| g == Some(object)) {
            self.forage(w, object);
        } else if object == self.fixtures[0] {
            self.go_to_bed();
        } else if object == self.fixtures[1] {
            self.ship();
        }
    }

    fn work_tile(&mut self, i: usize, object: u64) {
        let at = self.layout.tiles[i];
        if !self.near(at) {
            return self.notify("You need to be closer.");
        }
        let tile = self.farm.tiles[i];
        match tile.crop {
            None => match self.selected_seed() {
                Some(crop) => self.start(
                    spells::PLANT_SEEDS,
                    Target::GameObject(object),
                    PLANT_TIME,
                    Job::Plant(i, crop),
                ),
                None if self.bar.iter().all(|&e| e == 0) => {
                    self.notify("You have no seeds. Brog Hamfist in Goldshire sells them.")
                }
                None => self.notify("Pick seeds on your bar: scroll the mouse wheel or press 1-9."),
            },
            Some((crop, age)) => {
                if tile.ripe().is_some() {
                    return self.start(
                        spells::HERB_GATHERING,
                        Target::GameObject(object),
                        HARVEST_TIME,
                        Job::Harvest(i),
                    );
                }
                if tile.watered {
                    let left = crop.def().days - age;
                    let days = if left == 1 { "day" } else { "days" };
                    return self.notify(&format!(
                        "{} is watered. Ripe in {left} more {days}.",
                        crop.name()
                    ));
                }
                // Watering is instant: the soil darkening is the feedback.
                match self.farm.water(i) {
                    Ok(()) => self.show_tile(i),
                    Err(r) => self.refuse(&r),
                }
            }
        }
    }

    /// The client's own cast: Herb Gathering on a ripe crop, Mining on a vein.
    pub fn cast(&mut self, cast: CastRequest) {
        let refuse = |g: &Game| g.send(spells::cast_failed(cast.spell, spells::FAILED_SILENT));
        if self.cast.is_some() || self.asleep.is_some() {
            return refuse(self);
        }
        let Target::GameObject(object) = cast.target else {
            return refuse(self);
        };
        tracing::info!("farm: {} casts {} at {object:#x}", self.me.name, cast.spell);
        let job = match cast.spell {
            spells::HERB_GATHERING => (self.tile_of(object).map(Job::Harvest)).or_else(|| {
                (self.wild.iter())
                    .position(|&g| g == Some(object))
                    .map(Job::Forage)
            }),
            spells::MINING => self
                .veins
                .iter()
                .position(|&v| v == Some(object))
                .map(Job::Mine),
            _ => None,
        };
        let (Some(job), time) = (
            job,
            if cast.spell == spells::MINING {
                MINE_TIME
            } else {
                HARVEST_TIME
            },
        ) else {
            return refuse(self);
        };
        let at = match job {
            Job::Mine(v) => self.layout.veins[v].0,
            Job::Harvest(i) => self.layout.tiles[i],
            Job::Forage(w) => self.layout.wild[w].1,
            _ => self.at,
        };
        if !self.near(at) {
            refuse(self);
            return self.notify("You need to be closer.");
        }
        self.start(cast.spell, cast.target, time, job);
    }

    /// The farmer moved: a cast breaks when they walk away from it.
    pub fn moved(&mut self, at: [f32; 3], facing: f32) {
        let walked = distance(at, self.at) > 0.3;
        self.at = at;
        self.facing = facing;
        if walked {
            self.interrupt();
            if self.asleep.take().is_some() {
                self.send(player::stand_change(self.guid(), 0));
            }
        }
    }

    pub fn interrupt(&mut self) {
        if let Some(c) = self.cast.take() {
            self.send(spells::cast_failed(c.spell, spells::FAILED_INTERRUPTED));
            self.send(spells::spell_failed_other(self.guid(), c.spell));
        }
    }

    fn finish(&mut self, c: Cast) {
        self.send(spells::spell_go(self.guid(), c.spell, c.target));
        match c.job {
            Job::Plant(i, crop) => match self.farm.plant(i, crop) {
                Ok(()) => {
                    self.sync_bag(None, false);
                    self.show_tile(i);
                    if !self.farm.tiles[i].watered {
                        self.say(&format!(
                            "You planted {}. Right-click it to water it.",
                            crop.name()
                        ));
                    }
                }
                Err(r) => self.refuse(&r),
            },
            Job::Forage(w) => {
                let crop = self.layout.wild[w].0;
                match self.farm.forage(w, crop) {
                    Ok((n, seed)) => {
                        self.sync_bag(Some((Item::Produce(crop), n)), false);
                        if seed {
                            self.say(&format!(
                                "You found a {} seed! Plant it at home.",
                                crop.name()
                            ));
                        }
                        self.show_wild(w);
                    }
                    Err(r) => self.refuse(&r),
                }
            }
            Job::Harvest(i) => match self.farm.harvest(i) {
                Ok((crop, n)) => {
                    tracing::info!(
                        "farm: {} harvested {n} {} from tile {i}",
                        self.me.name,
                        crop.name()
                    );
                    self.sync_bag(Some((Item::Produce(crop), n)), false);
                    self.show_tile(i);
                }
                Err(r) => self.refuse(&r),
            },
            Job::Mine(v) => match self.farm.mine(v) {
                Ok(n) => {
                    self.sync_bag(Some((Item::CopperOre, n)), false);
                    self.show_vein(v);
                }
                Err(r) => self.refuse(&r),
            },
        }
    }

    fn forage(&mut self, w: usize, object: u64) {
        if !self.near(self.layout.wild[w].1) {
            return self.notify("You need to be closer.");
        }
        self.start(
            spells::HERB_GATHERING,
            Target::GameObject(object),
            HARVEST_TIME,
            Job::Forage(w),
        );
    }

    fn go_to_bed(&mut self) {
        if !self.near(self.layout.bed) {
            return self.notify("You need to be closer.");
        }
        self.say("You settle into the hay...");
        self.send(player::stand_change(self.guid(), 3));
        self.asleep = Some(SLEEP_TIME);
    }

    fn ship(&mut self) {
        if !self.near(self.layout.shipping) {
            return self.notify("You need to be closer.");
        }
        let goods: Vec<(Item, u32)> = (self.farm.bag.stacks().into_iter())
            .filter(|(i, _)| !matches!(i, Item::Seeds(_)))
            .collect();
        if goods.is_empty() {
            return self.notify("You have no crops or ore to ship.");
        }
        let mut earned = 0;
        let mut n = 0;
        for (item, count) in goods {
            if let Ok(paid) = self.farm.sell(item, count) {
                earned += paid;
                n += count;
            }
        }
        self.sync_bag(None, false);
        self.sync_money();
        self.say(&format!("You shipped {n} goods for {}.", coins(earned)));
    }

    /// A new morning: tell the farmer what the night did, reset the clock and the weather.
    fn morning(&mut self, report: DayReport) {
        if report.passed_out {
            self.say("You collapsed from exhaustion at 2:00 and woke up at home.");
            self.move_counter += 1;
            let home = self.layout.home;
            self.send(packets::teleport(
                self.guid(),
                self.move_counter,
                home.position,
                home.orientation,
            ));
            self.at = home.position;
        }
        self.send(player::stand_change(self.guid(), 0));
        self.send(self.clock_packet());
        self.send(self.weather_packet(false));
        self.told_hour = self.farm.clock.hour_minute().0;
        for i in 0..self.layout.tiles.len() {
            self.show_tile(i);
        }
        for v in 0..self.layout.veins.len() {
            self.show_vein(v);
        }
        for w in 0..self.layout.wild.len() {
            self.show_wild(w);
        }
        self.send(packets::headline(&format!("Good morning! {}", report.date)));
        let sky = match report.weather {
            Weather::Rain => "It's raining: your crops are watered.",
            Weather::Snow => "It's snowing.",
            Weather::Sunny => "It's sunny: don't forget to water.",
        };
        let ripe = match report.ripe {
            0 => String::new(),
            1 => " One crop is ripe.".into(),
            n => format!(" {n} crops are ripe."),
        };
        self.say(&format!(
            "{sky} {} crops grew overnight.{ripe}",
            report.grew
        ));
        self.save();
    }

    pub fn save(&mut self) {
        self.saves.store(self.guid(), &self.farm);
        self.since_save = Duration::ZERO;
    }

    /// Run `dt` of the game: casts finish, the day runs and the dog follows.
    pub fn tick(&mut self, dt: Duration) {
        if let Some(mut c) = self.cast.take() {
            if c.left <= dt {
                self.finish(c);
            } else {
                c.left -= dt;
                self.cast = Some(c);
            }
        }
        if let Some(left) = self.asleep {
            if left <= dt {
                self.asleep = None;
                let report = self.farm.sleep();
                self.morning(report);
            } else {
                self.asleep = Some(left - dt);
            }
        } else if let Some(report) = self.farm.tick(dt.as_secs_f32()) {
            self.morning(report);
        }
        let hour = self.farm.clock.hour_minute().0;
        if hour != self.told_hour {
            self.told_hour = hour;
            match hour {
                22 => self.say("It's getting late. Your hay bed is by the farmhouse."),
                1 => self.notify("You're exhausted! Get to bed before 2:00."),
                _ => {}
            }
        }
        self.since_dog += dt;
        if self.since_dog >= Duration::from_millis(400) {
            self.since_dog = Duration::ZERO;
            self.walk_dog();
        }
        self.since_save += dt;
        if self.since_save >= AUTOSAVE {
            self.save();
        }
    }

    fn walk_dog(&mut self) {
        let gap = distance(self.dog, self.at);
        if gap <= DOG_HEEL + 1.0 {
            return;
        }
        // A spot at heel, behind the farmer's left shoulder.
        let behind = self.facing + std::f32::consts::PI * 0.8;
        let to = [
            self.at[0] + DOG_HEEL * behind.cos(),
            self.at[1] + DOG_HEEL * behind.sin(),
            self.at[2],
        ];
        let run = gap > DOG_RUN;
        let speed = if run { 8.0 } else { 3.0 };
        let ms = (distance(self.dog, to) / speed * 1000.0) as u32;
        self.send(creatures::move_to(
            self.dog_guid(),
            self.dog,
            to,
            ms.max(1),
            run,
        ));
        self.dog = to;
    }

    /// The gossip window: Ma Stonefield's advice, or a pet pen animal offering to come along;
    /// Brog's is a store, opened by the client.
    pub fn gossip_hello(&mut self, npc: u64) {
        let Some(i) = self.npc_index(npc) else {
            return;
        };
        match self.layout.npcs[i].role {
            Role::Advice => self.send(packets::gossip_menu(npc, TEXT_MA, &[])),
            Role::Pet(pet) => {
                self.menu = Some(Menu::Adopt(pet));
                let mut options = Vec::new();
                if pet != self.companion() {
                    options.push(format!("Make {} your companion.", PETS[pet].0));
                }
                if self.companion() != 0 {
                    options.push(format!("Bring back {}.", PETS[0].0));
                }
                self.send(packets::gossip_menu(npc, TEXT_PET, &options));
            }
            _ => {}
        }
    }

    pub fn gossip_select(&mut self, option: u32) {
        let Some(Menu::Adopt(pet)) = self.menu.take() else {
            return;
        };
        self.send(packets::gossip_complete());
        let chosen = if option == 0 && pet != self.companion() {
            pet
        } else {
            0
        };
        self.send(crate::world::destroy(self.dog_guid()));
        self.farm.companion = (chosen != 0).then(|| PETS[chosen].0.to_owned());
        self.dog = [self.at[0] - 1.5, self.at[1], self.at[2]];
        self.show_companion();
        self.say(&format!("{} is following you now.", PETS[chosen].0));
        self.save();
    }

    pub fn npc_text(&self, id: u32) {
        match id {
            TEXT_PET => self.send(packets::npc_text(
                id,
                "It looks up at you hopefully. Any animal in the pen will follow you around.",
            )),
            TEXT_MA => self.send(packets::npc_text(id, MA_STONEFIELD)),
            _ => {}
        }
    }

    fn npc_index(&self, guid: u64) -> Option<usize> {
        let entry = ((guid >> 24) & 0xFF_FFFF) as u32;
        let i = entry.checked_sub(NPC_BASE)? as usize;
        (guid >> 48 == HIGH_UNIT && i < self.layout.npcs.len()).then_some(i)
    }

    fn is_store(&self, guid: u64) -> bool {
        self.npc_index(guid)
            .is_some_and(|i| self.layout.npcs[i].role == Role::Store)
    }

    fn wares() -> Vec<Ware> {
        (Crop::ALL.iter())
            .map(|&c| {
                let entry = items::entry(Item::Seeds(c));
                Ware {
                    entry,
                    display: items::template(entry).map_or(0, |t| t.display_id),
                    price: c.def().seed_price,
                }
            })
            .collect()
    }

    pub fn list_inventory(&self, vendor: u64) {
        if self.is_store(vendor) {
            self.send(packets::list_inventory(vendor, &Self::wares()));
        }
    }

    pub fn buy(&mut self, vendor: u64, entry: u32, count: u8) {
        use benilla_protocol::messages::buy_result;
        if !self.is_store(vendor) {
            return;
        }
        let Some(item) = items::item_of(entry) else {
            return self.send(packets::buy_failed(
                vendor,
                entry,
                buy_result::CANT_FIND_ITEM,
            ));
        };
        let n = u32::from(count.max(1));
        match self.farm.buy(item, n) {
            Ok(()) => {
                let row = Self::wares()
                    .iter()
                    .position(|w| w.entry == entry)
                    .unwrap_or(0) as u32;
                self.send(packets::buy_item(vendor, row + 1, n));
                self.sync_money();
                self.sync_bag(Some((item, n)), true);
            }
            Err(Reject::NotEnoughMoney) => self.send(packets::buy_failed(
                vendor,
                entry,
                buy_result::NOT_ENOUGH_MONEY,
            )),
            Err(Reject::BagFull) => self.send(packets::buy_failed(
                vendor,
                entry,
                buy_result::CANT_CARRY_MORE,
            )),
            Err(_) => self.send(packets::buy_failed(
                vendor,
                entry,
                buy_result::CANT_FIND_ITEM,
            )),
        }
    }

    pub fn sell(&mut self, vendor: u64, item_guid: u64, count: u8) {
        use benilla_protocol::messages::sell_result;
        if !self.is_store(vendor) {
            return;
        }
        let held = self
            .bag
            .iter()
            .flatten()
            .find(|h| h.guid == item_guid)
            .copied();
        let Some((held, item)) = held.and_then(|h| Some((h, items::item_of(h.entry)?))) else {
            return self.send(packets::sell_failed(
                vendor,
                item_guid,
                sell_result::CANT_SELL_ITEM,
            ));
        };
        let n = if count == 0 {
            held.count
        } else {
            u32::from(count).min(held.count)
        };
        match self.farm.sell(item, n) {
            Ok(_) => {
                self.sync_bag(None, false);
                self.sync_money();
            }
            Err(_) => self.send(packets::sell_failed(
                vendor,
                item_guid,
                sell_result::CANT_FIND_ITEM,
            )),
        }
    }

    /// `CMSG_DESTROYITEM` on a backpack slot: the whole stack is thrown away.
    pub fn destroy_item(&mut self, slot: u8) {
        let held = self.bag.iter().flatten().find(|h| h.slot == slot).copied();
        if let Some((h, item)) = held.and_then(|h| Some((h, items::item_of(h.entry)?))) {
            self.farm.bag.take(item, h.count);
            self.sync_bag(None, false);
        }
    }

    pub fn creature_query(&self, entry: u32) -> Packet {
        let pet = entry.checked_sub(COMPANION_BASE).map(|i| i as usize);
        if let Some(pet) = pet.filter(|&i| i < PETS.len()) {
            return creatures::query_response(entry, Some(&companion_look(pet)));
        }
        let npc = (entry.checked_sub(NPC_BASE)).and_then(|i| self.layout.npcs.get(i as usize));
        creatures::query_response(entry, npc.map(|n| &n.look))
    }

    pub fn gameobject_query(&self, entry: u32) -> Packet {
        objects::query_response(entry, object_template(entry).as_ref())
    }

    /// A `.` command: the farmer's shortcuts and a few for testing.
    pub fn command(&mut self, text: &str) {
        let mut words = text.split_whitespace();
        match words.next().unwrap_or("") {
            ".time" => self.say(&format!(
                "{}, {} - {}. You have {}.",
                self.farm.clock.date(),
                self.farm.clock.face(),
                self.farm.weather.name(),
                coins(self.farm.money)
            )),
            ".go" if words.next() == Some("xyz") => {
                let n: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
                if let [x, y, z, ..] = n[..] {
                    self.move_counter += 1;
                    self.send(packets::teleport(
                        self.guid(),
                        self.move_counter,
                        [x, y, z],
                        0.0,
                    ));
                    self.at = [x, y, z];
                }
            }
            ".home" => {
                self.move_counter += 1;
                let home = self.layout.home;
                self.send(packets::teleport(
                    self.guid(),
                    self.move_counter,
                    home.position,
                    home.orientation,
                ));
                self.at = home.position;
            }
            ".sleep" => {
                let report = self.farm.sleep();
                self.morning(report);
            }
            ".rain" => {
                self.farm.weather = Weather::Rain;
                for t in &mut self.farm.tiles {
                    t.watered = t.crop.is_some() && t.ripe().is_none();
                }
                self.send(self.weather_packet(false));
                for i in 0..self.layout.tiles.len() {
                    self.show_tile(i);
                }
            }
            ".hour" => {
                self.farm.tick(50.0);
                self.send(self.clock_packet());
                self.say(&format!("It's {}.", self.farm.clock.face()));
            }
            ".sel" => {
                if let Some(n) = words.next().and_then(|w| w.parse().ok()) {
                    self.select(n);
                }
            }
            ".gold" => {
                self.farm.money += 10_000;
                self.sync_money();
            }
            _ => self.say(
                "Commands: .time, .home (back to the farm), and for testing: .sleep, .rain, \
                 .hour, .gold",
            ),
        }
    }

    pub fn leave(&mut self) {
        self.save();
    }
}

/// The companion's look: the pet pen animal at its size.
fn companion_look(pet: usize) -> Look {
    let (name, display, scale) = PETS[pet];
    let mut look = Look::new(name, display, Kind::Beast);
    look.subname = "Companion".into();
    look.scale = scale;
    look.level = 1;
    look
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    fn game() -> (Game, UnboundedReceiver<Packet>) {
        let layout = Arc::new(Layout::stonefield(None));
        let saves = Saves::open(
            None,
            layout.tiles.len(),
            layout.veins.len(),
            layout.wild.len(),
        )
        .unwrap();
        let me = Character {
            guid: 1,
            name: "Hoggle".into(),
            race: 1,
            class: 1,
            gender: 0,
            skin: 0,
            face: 0,
            hair_style: 0,
            hair_color: 0,
            facial_hair: 0,
        };
        saves.create(me.clone()).unwrap();
        let farm = saves.farm(1).unwrap();
        let (tx, rx) = unbounded_channel();
        let mut g = Game::new(me, farm, layout, saves, tx);
        g.enter();
        (g, rx)
    }

    /// Every packet sent so far, each one checked to read back whole in the client.
    fn drain(rx: &mut UnboundedReceiver<Packet>) -> Vec<ServerPacket> {
        let mut out = Vec::new();
        while let Ok(p) = rx.try_recv() {
            let (parsed, tail) = parse_server_with_tail(p.opcode, &p.body)
                .unwrap_or_else(|e| panic!("{:#06x} does not parse: {e}", p.opcode));
            assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
            out.push(parsed);
        }
        out
    }

    fn stand_at(g: &mut Game, at: [f32; 3]) {
        g.moved(at, 0.0);
    }

    fn run(g: &mut Game, secs: f32) {
        for _ in 0..(secs * 10.0) as u32 {
            g.tick(Duration::from_millis(100));
        }
    }

    #[test]
    fn the_world_shows_the_plot_veins_fixtures_villagers_and_the_dog() {
        let (g, mut rx) = game();
        let seen = drain(&mut rx);
        let creates = seen
            .iter()
            .filter_map(|p| match p {
                ServerPacket::UpdateObject { objects } => Some(objects.len()),
                _ => None,
            })
            .sum::<usize>();
        let l = &g.layout;
        let expected = l.tiles.len() + l.veins.len() + l.wild.len() + 2 + l.npcs.len() + 1;
        assert!(
            creates >= expected,
            "{creates} objects for {expected} things"
        );
    }

    #[test]
    fn a_day_on_the_farm_plants_waters_grows_harvests_and_ships() {
        let (mut g, mut rx) = game();
        drain(&mut rx);
        let tile = g.layout.tiles[0];
        stand_at(&mut g, tile);
        g.farm.weather = Weather::Sunny;
        // The bar holds both kinds of seed; the first is picked, so the soil gets Silverleaf.
        assert_eq!(g.bar.iter().filter(|&&e| e != 0).count(), 2);
        g.use_object(g.soil[0].0);
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles[0].crop, Some((Crop::Silverleaf, 0)));
        assert_eq!(g.farm.bag.count(Item::Seeds(Crop::Silverleaf)), 5);
        let crop = g.crops[0].expect("the crop stands").0;

        for _ in 0..Crop::Silverleaf.def().days {
            if !g.farm.tiles[0].watered {
                let crop = g.crops[0].expect("growing").0;
                g.use_object(crop);
                assert!(g.farm.tiles[0].watered, "watering is instant");
                assert_eq!(g.soil[0].1, SOIL_WET);
            }
            let bed = g.layout.bed;
            stand_at(&mut g, bed);
            g.use_object(g.fixtures[0]);
            run(&mut g, 4.0);
            stand_at(&mut g, tile);
        }
        assert_eq!(g.farm.tiles[0].ripe(), Some(Crop::Silverleaf));
        assert_ne!(
            g.crops[0].unwrap().0,
            crop,
            "the ripe plant is a new object"
        );
        drain(&mut rx);

        let ripe = g.crops[0].unwrap().0;
        g.cast(CastRequest {
            spell: spells::HERB_GATHERING,
            target: Target::GameObject(ripe),
        });
        run(&mut g, 2.0);
        assert_eq!(g.farm.bag.count(Item::Produce(Crop::Silverleaf)), 2);
        assert!(g.crops[0].is_none());
        let seen = drain(&mut rx);
        assert!(seen
            .iter()
            .any(|p| matches!(p, ServerPacket::ItemPushResult(_))));

        let shipping = g.layout.shipping;
        stand_at(&mut g, shipping);
        let money = g.farm.money;
        g.use_object(g.fixtures[1]);
        assert_eq!(g.farm.money, money + 2 * Crop::Silverleaf.def().sell_price);
        assert_eq!(g.farm.bag.count(Item::Produce(Crop::Silverleaf)), 0);
        drain(&mut rx);
    }

    #[test]
    fn the_picked_bar_slot_is_what_gets_planted() {
        let (mut g, mut rx) = game();
        let tile = g.layout.tiles[1];
        stand_at(&mut g, tile);
        let seen = drain(&mut rx);
        assert!(
            seen.iter()
                .any(|p| matches!(p, ServerPacket::ActionButtons { .. })),
            "the seeds go on the bar"
        );
        g.command(".sel 2");
        g.use_object(g.soil[1].0);
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles[1].crop.map(|c| c.0), Some(Crop::Peacebloom));
        // An empty slot plants nothing.
        g.command(".sel 9");
        let tile = g.layout.tiles[2];
        stand_at(&mut g, tile);
        g.use_object(g.soil[2].0);
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles[2].crop, None);
        // When the last Peacebloom seed goes, its slot empties and the others stay put.
        g.command(".sel 1");
        g.farm.bag.take(Item::Seeds(Crop::Peacebloom), 3);
        g.sync_bag(None, false);
        assert_eq!(g.bar[1], 0);
        assert_eq!(g.selected_seed(), Some(Crop::Silverleaf));
        drain(&mut rx);
    }

    #[test]
    fn a_wild_herb_is_picked_and_grows_back_tomorrow() {
        let (mut g, mut rx) = game();
        let (crop, at) = g.layout.wild[0];
        stand_at(&mut g, at);
        let herb = g.wild[0].expect("the herb stands");
        g.cast(CastRequest {
            spell: spells::HERB_GATHERING,
            target: Target::GameObject(herb),
        });
        run(&mut g, 2.0);
        assert_eq!(g.farm.bag.count(Item::Produce(crop)), 1);
        assert!(g.wild[0].is_none());
        g.command(".sleep");
        assert!(g.wild[0].is_some());
        drain(&mut rx);
    }

    #[test]
    fn a_pet_pen_animal_becomes_the_companion() {
        let (mut g, mut rx) = game();
        let (i, pet) = (g.layout.npcs.iter().enumerate())
            .find_map(|(i, n)| match n.role {
                Role::Pet(p) if PETS[p].0 == "Panda Cub" => Some((i, p)),
                _ => None,
            })
            .expect("the panda is in the pen");
        let entry = NPC_BASE + i as u32;
        let guid = HIGH_UNIT << 48 | u64::from(entry) << 24 | i as u64;
        drain(&mut rx);
        g.gossip_hello(guid);
        assert_eq!(g.menu, Some(Menu::Adopt(pet)));
        g.gossip_select(0);
        assert_eq!(g.companion(), pet);
        assert_eq!(g.farm.companion.as_deref(), Some("Panda Cub"));
        let seen = drain(&mut rx);
        assert!(seen
            .iter()
            .any(|p| matches!(p, ServerPacket::UpdateObject { .. })));
        // Option 1 sends it back for the farm dog.
        g.gossip_hello(guid);
        g.gossip_select(1);
        assert_eq!(g.companion(), 0);
        drain(&mut rx);
    }

    #[test]
    fn walking_away_breaks_a_cast() {
        let (mut g, mut rx) = game();
        let tile = g.layout.tiles[3];
        stand_at(&mut g, tile);
        g.use_object(g.soil[3].0);
        assert!(g.cast.is_some(), "the picked seed plants");
        stand_at(&mut g, [tile[0] + 3.0, tile[1], tile[2]]);
        assert!(g.cast.is_none());
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles[3].crop, None);
        drain(&mut rx);
    }

    #[test]
    fn brog_sells_seeds_and_buys_the_harvest() {
        let (mut g, mut rx) = game();
        let brog = (0..g.layout.npcs.len())
            .find(|&i| g.layout.npcs[i].role == Role::Store)
            .unwrap();
        let entry = NPC_BASE + brog as u32;
        let guid = HIGH_UNIT << 48 | u64::from(entry) << 24 | brog as u64;
        drain(&mut rx);
        g.list_inventory(guid);
        assert!(drain(&mut rx)
            .iter()
            .any(|p| matches!(p, ServerPacket::VendorList { .. })));
        let pumpkin = items::entry(Item::Seeds(Crop::Pumpkin));
        g.buy(guid, pumpkin, 1);
        assert_eq!(g.farm.bag.count(Item::Seeds(Crop::Pumpkin)), 1);
        assert_eq!(g.farm.money, 50);
        g.buy(guid, pumpkin, 1);
        assert_eq!(g.farm.bag.count(Item::Seeds(Crop::Pumpkin)), 1, "too poor");
        let held = *g.bag.iter().flatten().find(|h| h.entry == pumpkin).unwrap();
        g.sell(guid, held.guid, 0);
        assert_eq!(g.farm.bag.count(Item::Seeds(Crop::Pumpkin)), 0);
        assert_eq!(g.farm.money, 50 + 75);
        assert!(g.bag.iter().flatten().all(|h| h.entry != pumpkin));
        drain(&mut rx);
    }

    #[test]
    fn a_vein_is_mined_once_a_day() {
        let (mut g, mut rx) = game();
        let (at, _) = g.layout.veins[0];
        stand_at(&mut g, at);
        let vein = g.veins[0].unwrap();
        g.cast(CastRequest {
            spell: spells::MINING,
            target: Target::GameObject(vein),
        });
        run(&mut g, 3.0);
        assert!(g.farm.bag.count(Item::CopperOre) >= 1);
        assert!(g.veins[0].is_none());
        g.command(".sleep");
        assert!(g.veins[0].is_some());
        drain(&mut rx);
    }

    #[test]
    fn staying_up_too_late_sends_the_farmer_home() {
        let (mut g, mut rx) = game();
        let vein = g.layout.veins[0].0;
        stand_at(&mut g, vein);
        run(&mut g, 1001.0);
        assert_eq!(g.farm.clock.day, 2);
        assert_eq!(g.at, g.layout.home.position);
        drain(&mut rx);
    }

    #[test]
    fn a_gameobject_guid_carries_its_entry_where_the_client_reads_it() {
        let (mut g, _rx) = game();
        let guid = g.fresh_guid(VEIN);
        assert_eq!(benilla_protocol::guid::entry(guid), Some(VEIN));
    }

    #[test]
    fn coins_read_like_the_game_writes_them() {
        assert_eq!(coins(0), "0c");
        assert_eq!(coins(205), "2s 5c");
        assert_eq!(coins(10_000), "1g");
        assert_eq!(coins(12_345), "1g 23s 45c");
    }

    #[test]
    fn every_crop_stage_has_its_own_entry_and_template() {
        let mut seen = std::collections::HashSet::new();
        for crop in Crop::ALL {
            for age in 0..crop.def().days {
                for watered in [false, true] {
                    let e = crop_entry(crop, age, watered, false);
                    assert!(seen.insert(e));
                    let t = object_template(e).unwrap();
                    assert_eq!(t.use_, Use::Goober);
                    assert!(t.name.starts_with(crop.name()), "{}", t.name);
                }
            }
            let ripe = object_template(crop_entry(crop, 0, false, true)).unwrap();
            assert_eq!(ripe.use_, Use::Herb);
        }
        for e in [SOIL_DRY, SOIL_WET, VEIN, BED, SHIPPING] {
            assert!(object_template(e).is_some());
        }
    }
}
