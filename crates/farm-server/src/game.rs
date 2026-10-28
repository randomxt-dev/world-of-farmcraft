//! One farmer's session in the world: the farm's rules shown through WoW's own objects. Soil and
//! crops are gameobjects on the plot, ripe crops carry Herbalism's lock and veins Mining's, the
//! hay bed sleeps, the shipping crate buys the harvest, Brog Hamfist's stock UI sells seeds, and a
//! wolf pup follows the farmer around.

use std::sync::Arc;
use std::time::Duration;

use farm_rules::save::Character;
use farm_rules::{
    Crop, DayReport, Deed, Farm, Fence, Item, Reject, Skill, TreeStage, Unlock, Weather,
};

use crate::layout::{
    Layout, Role, FENCE_DISPLAY, FENCE_LENGTH, HORSE_DISPLAY, PETS, PLOT_COLUMNS, PLOT_ROWS, TREES,
};
use crate::saves::Saves;
use crate::world::creatures::{self, Kind, Look, Shown};
use crate::world::items::{self, Held};
use crate::world::objects::{self, Template, Use};
use crate::world::packets::{self, Ware};
use crate::world::player;
use crate::world::socket::{Outbox, Packet};
use crate::world::spells::{self, CastRequest, Target};

mod preview;
mod quests;

use quests::{AXE_PRICE, PICK_PRICE, TEXT_BROG, TEXT_FARLEY, TEXT_KHELDEN};

// Gameobject entries: soil, each crop's growing stages and its ripe plant, and the fixtures.
const SOIL_DRY: u32 = 50_000;
const SOIL_WET: u32 = 50_001;
const CROP_BASE: u32 = 51_000;
/// Within a crop's hundred entries, its ripe plant.
const RIPE: u32 = 99;
const VEIN: u32 = 52_000;
/// Grove trees, one entry per kind in [`TREES`] from here, then a stump and a sapling.
const TREE_BASE: u32 = 54_000;
const STUMP: u32 = 54_100;
const SAPLING: u32 = 54_101;
/// `World\Lordaeron\AeriePeaks\PassiveDoodads\Trees\AeriePeaksStump01.mdx`.
const STUMP_DISPLAY: u32 = 6158;
/// `World\Kalimdor\StoneTalon\ActiveDoodads\Tree\g_sapling01.mdx`.
const SAPLING_DISPLAY: u32 = 4951;
/// `Emotes.dbc` 36, `ONESHOT_ATTACK1H`: one swing of the one-hand weapon in hand.
const SWING: u32 = 36;
/// The time between axe swings, and the pause before the tree falls; the swings are the skill's.
const SWING_EVERY: Duration = Duration::from_millis(900);
/// How close the farmer must stand to a tree's trunk.
const TREE_REACH: f32 = 7.0;
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
/// `WORLD\GENERIC\HUMAN\PASSIVE DOODADS\BEDS\DUSKWOODBED.MDX`: a plain farmhouse bed.
const INN_BED: u32 = 3471;
/// A fence the farmer built, a planned one waiting for wood, and the start of a fence line.
const FENCE: u32 = 55_000;
const FENCE_PLAN: u32 = 55_001;
const FENCE_START: u32 = 55_002;
/// `World\Generic\Human\Passive Doodads\SignPosts\WoodSignPointerWorn01.mdx`: a marker post.
const MARKER_POST: u32 = 22;
/// The horse the farmer rides: Riding Horse's look, and the run speed in the saddle.
const RIDE_SPEED: f32 = 14.0;
/// On foot: the 1.12 run speed.
const WALK_SPEED: f32 = 7.0;
/// The map's house icon (`POIIcons.blp` cell 4), flagging the farm.
const HOUSE_ICON: u32 = 4;
/// The client drops the map marker 480 s after it comes, or once the farmer is inside 10 yd of
/// it; it is sent again a little sooner, and only from farther away.
const MARKER_REFRESH: Duration = Duration::from_secs(420);
const MARKER_NEAR: f32 = 12.0;
/// A wild critter wanders every this often, one of them at a time.
const WANDER_EVERY: Duration = Duration::from_millis(700);
/// `SoundEntries.dbc`: an axe biting wood, its heavier last blow, a tree hitting the ground, wood
/// picked up, and wood set down.
const SOUND_CHOP: u32 = 3202;
const SOUND_CHOP_LAST: u32 = 3203;
const SOUND_TREE_FALL: u32 = 921;
const SOUND_WOOD_UP: u32 = 1199;
const SOUND_WOOD_DOWN: u32 = 1216;
/// `World\Generic\Human\Passive Doodads\Crates\CrateGrain01.mdx`.
const GRAIN_CRATE: u32 = 347;

/// The dirt mounds are grave-sized; a tile's is smaller, and the damp mound is the bigger model.
const SOIL_SCALE: f32 = 0.42;
const WET_SOIL_SCALE: f32 = 0.34;
/// The dirt mound model is long along its own facing, so it faces down the furrow.
const MOUND_TURN: f32 = 0.0;
/// How close the farmer must stand to work a tile or talk to someone.
const REACH: f32 = 6.0;
/// How long each job's cast bar runs.
const PLANT_TIME: Duration = Duration::from_millis(1500);
const WATER_TIME: Duration = Duration::from_millis(1200);
/// How far from the bed `/sleep` still counts as in the farmhouse.
const HOUSE_REACH: f32 = 10.0;
/// `EmotesText.dbc`'s SLEEP.
const TEXT_EMOTE_SLEEP: u32 = 87;
/// The `SkillLine.dbc` lines the farmer's skills wear: Herbalism, Mining and Riding as
/// themselves; Farming, Lumberjacking, Woodworking and Shepherding on Tailoring, Skinning,
/// Engineering and Leatherworking, which the hotbar's Lua renames.
const SKILL_LINES: [(Skill, u32); 7] = [
    (Skill::Farming, 197),
    (Skill::Herbalism, 182),
    (Skill::Mining, 186),
    (Skill::Lumberjacking, 393),
    (Skill::Woodworking, 202),
    (Skill::Shepherding, 165),
    (Skill::Riding, 762),
];
/// Yards ridden for each try at Riding.
const RIDE_PRACTICE: f32 = 40.0;

/// How hard a crop is to gather, as Herbalism reckons it: the herb's own skill in the game.
fn herb_difficulty(crop: Crop) -> u32 {
    match crop {
        Crop::Silverleaf | Crop::Peacebloom => 1,
        Crop::Earthroot => 15,
        Crop::Pumpkin => 25,
        Crop::Mageroyal | Crop::Watermelon => 50,
        Crop::Briarthorn => 70,
    }
}
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
        BED => Template::new("Bed", INN_BED, Use::Goober),
        FENCE => Template::new("Elwynn Fence", FENCE_DISPLAY, Use::Goober),
        FENCE_PLAN => Template::new("Planned Fence (2 Simple Wood)", MARKER_POST, Use::Goober),
        FENCE_START => Template::new("Fence Line Start", MARKER_POST, Use::Goober),
        SHIPPING => Template::new("Shipping Crate", GRAIN_CRATE, Use::Goober),
        STUMP => Template::new("Tree Stump", STUMP_DISPLAY, Use::None),
        SAPLING => Template::new("Sapling", SAPLING_DISPLAY, Use::None),
        e if (TREE_BASE..TREE_BASE + TREES.len() as u32).contains(&e) => {
            let (name, display, _) = TREES[(e - TREE_BASE) as usize];
            Template::new(name, display, Use::Goober)
        }
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

/// Villager `i`'s guid: its entry in bits 24-47, as the client reads it.
fn npc_guid(i: usize) -> u64 {
    HIGH_UNIT << 48 | u64::from(NPC_BASE + i as u32) << 24 | i as u64
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
    Water(usize),
    /// Dig a new plot tile at a field cell.
    Till((i32, i32)),
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
    /// Open this villager's merchant window.
    Shop(u64),
}

/// A tree being chopped: which, the swings still to come, and the time to the next.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Chop {
    tree: usize,
    swings: u32,
    next: Duration,
}

/// The greeting texts the gossip windows show.
const TEXT_MA: u32 = 2;
const TEXT_PET: u32 = 3;

const MA_STONEFIELD: &str = "Well, look who's here! That field's been fallow since the \
kobolds took Fargodeep. It's yours to work now, dear.$B$BEverything you carry sits on your \
bar: scroll the mouse wheel or press a number to pick, and hold Shift to zoom instead. Sell \
your harvest to Innkeeper Farley at the Lion's Pride Inn in Goldshire; Brog Hamfist there \
sells seeds and tools.$B$BAnd mind you get to bed before two in the morning!";
const FARLEY: &str = "Welcome to the Lion's Pride, friend! Fresh from the farm? I'll take \
whatever you grow for the kitchen, and pay fair coin for it.";
const BROG: &str = "Seeds, tools, a good stout axe: if a farmer needs it, Brog has it.";
const KHELDEN: &str = "Welcome to the abbey library. Every trade under the Light has a book here, and most of them are overdue.";

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
    /// Each grove tree's object as shown: (guid, entry).
    trees: Vec<(u64, u32)>,
    chop: Option<Chop>,
    /// The farmer's fences as shown, in the farm's order.
    fences: Vec<u64>,
    /// The fence line being drawn: where it starts, and its marker.
    fence_start: Option<([f32; 3], u64)>,
    /// In the saddle.
    mounted: bool,
    /// Where each wild critter stands now, and the time to the next one's stroll.
    critter_at: Vec<[f32; 3]>,
    since_wander: Duration,
    /// How long the farm's map marker has been up, while the client still shows it.
    marker_age: Option<Duration>,
    /// Yards ridden since the last try at Riding.
    ridden: f32,
    /// Showing a look-ahead preview: nothing is saved.
    previewing: bool,
    /// The preview's herd as shown.
    herd: Vec<u64>,
    /// The tool in hand: the shovel or the axe.
    tool: u32,
    /// The quest log as last sent: (quest id, counter word) per slot.
    quest_log: Vec<(u32, u32)>,
    /// Ma's, Farley's, Brog's and Khelden's markers as last sent.
    markers: [u32; 4],
    /// The tool slots on the bar as last sent: till, can, sickle, axe, pick, fence and horse.
    bar_tools: [bool; 7],
    /// Whether the shovel's slot casts Till Soil, as last sent.
    bar_till: bool,
    /// The bar slot lit as the tool in use, and the seed slot last picked (from the first).
    lit: usize,
    seed_pick: usize,
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
        farm.fit_grove(layout.trees.len());
        let animals = (layout.npcs.iter())
            .filter(|n| matches!(n.role, Role::Animal(..)))
            .count();
        farm.fit_animals(animals);
        let tiles = farm.tiles.len();
        let veins = layout.veins.len();
        let wild = layout.wild.len();
        let grove = layout.trees.len();
        let critter_at: Vec<[f32; 3]> = layout.critters.iter().map(|(_, spots)| spots[0]).collect();
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
            trees: vec![(0, 0); grove],
            chop: None,
            fences: Vec::new(),
            fence_start: None,
            mounted: false,
            critter_at,
            since_wander: Duration::ZERO,
            marker_age: None,
            ridden: 0.0,
            previewing: false,
            herd: Vec::new(),
            tool: items::SHOVEL,
            quest_log: Vec::new(),
            markers: [u32::MAX; 4],
            bar_tools: [false; 7],
            bar_till: false,
            lit: spells::SEED_SLOT0,
            seed_pick: 0,
            bar: Vec::new(),
            selected: spells::SEED_SLOT0,
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
        let mut carried = vec![
            player::spare_tool(self.guid()),
            player::spare_sickle(self.guid()),
        ];
        if self.farm.has(Unlock::MiningPick) {
            carried.push(player::pick(self.guid()));
        }
        for p in player::login_packets(
            &self.me,
            self.layout.home,
            self.clock_packet(),
            &carried,
            self.farm.money,
            &self.skill_lines(),
        ) {
            self.send(p);
        }
        self.at = self.layout.home.position;
        self.sync_bag(None, false);
        self.show_world();
        self.send(self.weather_packet(true));
        self.marker_age = None;
        self.mark_home();
        self.send(player::quest_log(self.guid(), &[]));
        self.sync_quest_log();
        self.refresh_markers();
        self.say(&format!(
            "Welcome to Stonefield Farm! It's {}, {} and {}.",
            self.farm.clock.date(),
            self.farm.clock.face(),
            self.farm.weather.name()
        ));
        if self.farm.quests.done == 0 && self.farm.quests.active.is_empty() {
            self.say("Ma Stonefield by the farmhouse has a job for you: look for the yellow !");
        }
    }

    /// A new gameobject guid for template `entry`. The client reads the entry back out of the
    /// guid (bits 24-47, as vmangos packs it) to ask for the template, so it must be there.
    fn fresh_guid(&mut self, entry: u32) -> u64 {
        self.generation += 1;
        HIGH_GAMEOBJECT << 48 | u64::from(entry & 0xFF_FFFF) << 24 | (self.generation & 0xFF_FFFF)
    }

    fn show_world(&mut self) {
        for i in 0..self.farm.tiles.len() {
            self.show_tile(i);
        }
        for v in 0..self.layout.veins.len() {
            self.show_vein(v);
        }
        for w in 0..self.layout.wild.len() {
            self.show_wild(w);
        }
        for t in 0..self.layout.trees.len() {
            self.show_tree(t);
        }
        for f in 0..self.farm.fences.len() {
            let guid = self.show_fence(f);
            self.fences.push(guid);
        }
        self.show_fixture(0);
        if self.farm.has(Unlock::ShippingCrate) {
            self.show_fixture(1);
        }
        for i in 0..self.layout.npcs.len() {
            let waits = match self.layout.npcs[i].role {
                Role::Horse => Some(Unlock::Horse),
                _ => self.animal_unlock(i),
            };
            if waits.is_none_or(|u| self.farm.has(u)) {
                self.show_npc(i);
            }
        }
        self.show_companion();
    }

    /// Fixture `i`: the bed, or the shipping crate.
    fn show_fixture(&mut self, i: usize) {
        let (entry, at, facing) = match i {
            0 => (BED, self.layout.bed, self.layout.bed_facing),
            _ => (SHIPPING, self.layout.shipping, 1.0),
        };
        let guid = self.fresh_guid(entry);
        self.fixtures[i] = guid;
        let t = object_template(entry).expect("a fixture");
        self.send(objects::create(guid, entry, &t, at, facing, 1.0));
    }

    /// What farm animal `i` waits on: the hens, or the cow and sheep. `None` for anyone else.
    fn animal_unlock(&self, i: usize) -> Option<Unlock> {
        match self.layout.npcs[i].role {
            Role::Animal(Item::Egg, _) => Some(Unlock::Hens),
            Role::Animal(..) => Some(Unlock::Herd),
            _ => None,
        }
    }

    fn show_npc(&self, i: usize) {
        let npc = &self.layout.npcs[i];
        let shown = Shown {
            guid: npc_guid(i),
            entry: NPC_BASE + i as u32,
            position: npc.at,
            orientation: npc.facing,
            owner: None,
        };
        self.send(creatures::create(&shown, &npc.look));
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

    /// Where tile `i` lies: the starting plot's, then the cells the farmer dug.
    fn tile_pos(&self, i: usize) -> [f32; 3] {
        let base = self.layout.tiles.len();
        match i.checked_sub(base) {
            None => self.layout.tiles[i],
            Some(d) => (self.farm.dug.get(d))
                .and_then(|&cell| self.layout.cell_pos(cell))
                .unwrap_or(self.at),
        }
    }

    /// Whether field cell `cell` already holds a tile.
    fn is_dug(&self, cell: (i32, i32)) -> bool {
        let start =
            (0..PLOT_ROWS as i32).contains(&cell.0) && (0..PLOT_COLUMNS as i32).contains(&cell.1);
        start || self.farm.dug.contains(&cell)
    }

    /// Put up fence `f` of the farm's, or its marker while it waits for wood; its guid.
    fn show_fence(&mut self, f: usize) -> u64 {
        let fence = self.farm.fences[f];
        let entry = if fence.built { FENCE } else { FENCE_PLAN };
        let guid = self.fresh_guid(entry);
        let t = object_template(entry).expect("a fence");
        self.send(objects::create(
            guid,
            entry,
            &t,
            fence.at,
            fence.facing,
            1.0,
        ));
        guid
    }

    /// The ends of every fence, built or planned, that a new line can join.
    fn fence_ends(&self) -> Vec<[f32; 3]> {
        let mut ends = Vec::new();
        for f in &self.farm.fences {
            ends.push(f.at);
            ends.push([
                f.at[0] - FENCE_LENGTH * f.facing.cos(),
                f.at[1] - FENCE_LENGTH * f.facing.sin(),
                f.at[2],
            ]);
        }
        ends
    }

    /// Till Soil at a spot: the nearest field cell, if it is free, close by and in the field.
    fn till_at(&mut self, p: [f32; 3]) {
        if !self.farm.has(Unlock::Tilling) {
            return self.refuse(&Reject::Locked(Unlock::Tilling));
        }
        let Some(cell) = self.layout.cell_at(p[0], p[1]) else {
            return self.notify("Dig in the field: the furrowed ground by the farmhouse.");
        };
        if self.is_dug(cell) {
            return self.notify("That ground is already dug.");
        }
        let at = self.layout.cell_pos(cell).unwrap_or(p);
        if !self.near(at) {
            return self.notify("You need to be closer.");
        }
        self.start(
            spells::PLANT_SEEDS,
            Target::Caster,
            self.quick(Skill::Farming, PLANT_TIME),
            Job::Till(cell),
        );
    }

    /// Build Fence at a spot. The first click starts a line, each next one runs fence sections
    /// from there to the click, section by section, and carries the line on from its end; a
    /// click on the line's start ends it. A click near a fence's end joins it. Sections the wood
    /// runs out for are marked out with a post, to finish later.
    fn build_at(&mut self, p: [f32; 3]) {
        if !self.farm.has(Unlock::Fence) {
            return self.refuse(&Reject::Locked(Unlock::Fence));
        }
        self.take_tool(spells::FENCE_SLOT, None);
        if distance(self.at, p) > 30.0 {
            return self.notify("You need to be closer.");
        }
        let ends = self.fence_ends();
        let p = (ends.iter())
            .copied()
            .filter(|e| distance(*e, p) < 2.0)
            .min_by(|a, b| distance(*a, p).total_cmp(&distance(*b, p)))
            .unwrap_or(p);
        let Some((start, marker)) = self.fence_start.take() else {
            let guid = self.fresh_guid(FENCE_START);
            let t = object_template(FENCE_START).expect("a marker");
            self.send(objects::create(guid, FENCE_START, &t, p, 0.0, 1.0));
            self.fence_start = Some((p, guid));
            return self.say(
                "The fence starts here. Click where it should run to; click here again to stop.",
            );
        };
        self.send(crate::world::destroy(marker));
        let length = distance(start, p);
        if length < 1.5 {
            return self.say("Fence line finished.");
        }
        let sections = (length / FENCE_LENGTH).round().max(1.0) as usize;
        let dir = [(p[0] - start[0]) / length, (p[1] - start[1]) / length];
        let facing = dir[1].atan2(dir[0]) + std::f32::consts::PI;
        let (mut built, mut planned) = (0, 0);
        let mut end = start;
        for k in 0..sections {
            let t = k as f32 * FENCE_LENGTH;
            let at = [
                start[0] + dir[0] * t,
                start[1] + dir[1] * t,
                start[2] + (p[2] - start[2]) * (t / length).min(1.0),
            ];
            let fence = Fence {
                at,
                facing,
                style: FENCE_DISPLAY,
                built: true,
            };
            if self.farm.build(fence) {
                built += 1;
            } else {
                planned += 1;
            }
            let guid = self.show_fence(self.farm.fences.len() - 1);
            self.fences.push(guid);
            let t = t + FENCE_LENGTH;
            end = [
                start[0] + dir[0] * t,
                start[1] + dir[1] * t,
                start[2] + (p[2] - start[2]) * (t / length).min(1.0),
            ];
        }
        if built > 0 {
            self.send(packets::play_sound(SOUND_WOOD_DOWN));
            self.sync_bag(None, false);
            for _ in 0..built {
                self.noted(Deed::BuildFence);
                self.practice(Skill::Woodworking, 1);
                if self.farm.lucky(Skill::Woodworking) {
                    self.farm.bag.add(Item::Wood, farm_rules::FENCE_WOOD);
                    self.say("Your Woodworking saved the wood on that section.");
                    self.sync_bag(None, false);
                }
            }
        }
        let mut line = format!("{built} fence sections built");
        if planned > 0 {
            line += &format!(
                ", {planned} marked out: right-click a post to build it once you have {} Simple Wood",
                farm_rules::FENCE_WOOD
            );
        }
        self.say(&format!("{line}."));
        // Carry the line on from where it ended.
        let guid = self.fresh_guid(FENCE_START);
        let t = object_template(FENCE_START).expect("a marker");
        self.send(objects::create(guid, FENCE_START, &t, end, 0.0, 1.0));
        self.fence_start = Some((end, guid));
    }

    /// Mount or dismount the farm horse.
    fn ride(&mut self, mount: bool) {
        if self.mounted == mount {
            return;
        }
        self.mounted = mount;
        if mount {
            self.take_tool(spells::RIDE_SLOT, None);
        }
        if let Some(h) = self.horse() {
            if mount {
                self.send(crate::world::destroy(npc_guid(h)));
            } else {
                self.show_npc(h);
            }
        }
        let display = if mount { HORSE_DISPLAY } else { 0 };
        self.send(player::mount_change(self.guid(), display));
        self.move_counter += 1;
        let pace = farm_rules::skills::pace(self.farm.skills.get(Skill::Riding).0);
        let speed = if mount { RIDE_SPEED * pace } else { WALK_SPEED };
        self.send(packets::force_run_speed(
            self.guid(),
            self.move_counter,
            speed,
        ));
    }

    /// The farm horse's index among the villagers, once it is the farmer's.
    fn horse(&self) -> Option<usize> {
        if !self.farm.has(Unlock::Horse) {
            return None;
        }
        (self.layout.npcs.iter()).position(|n| n.role == Role::Horse)
    }

    /// `.ride`, the hotbar's horse button: up if the farmer has a horse, or down.
    fn toggle_ride(&mut self) {
        if !self.farm.has(Unlock::Horse) {
            return self.refuse(&Reject::Locked(Unlock::Horse));
        }
        let mount = !self.mounted;
        self.ride(mount);
    }

    /// One try at `skill` on something of `difficulty`; a point shows in the skills.
    fn practice(&mut self, skill: Skill, difficulty: u32) {
        let Some(level) = self.farm.practice(skill, difficulty) else {
            return;
        };
        self.send(player::skill_change(self.guid(), &self.skill_lines()));
        if level % farm_rules::skills::TIER != 0 {
            return;
        }
        let perk = match skill {
            Skill::Farming => "you dig, plant and water quicker",
            Skill::Herbalism => "you gather quicker, and now and then find an extra herb",
            Skill::Mining => "you mine quicker, and now and then find extra ore",
            Skill::Lumberjacking if level == 50 => "two swings fell a tree now",
            Skill::Lumberjacking if level == 150 => "one swing fells a tree now",
            Skill::Lumberjacking => "you chop quicker, and now and then split an extra log",
            Skill::Woodworking => "now and then a fence section costs no wood",
            Skill::Shepherding => "now and then an animal gives twice",
            Skill::Riding => "Biscuit runs a little faster",
        };
        self.say(&format!("{} {level}: {perk}.", skill.name()));
        if skill == Skill::Riding && self.mounted {
            self.mounted = false;
            self.ride(true);
        }
    }

    /// A lucky extra of `item`, by `skill`'s level: 1 if it came, into the bag already.
    fn bonus(&mut self, skill: Skill, item: Item) -> u32 {
        if self.farm.lucky(skill) && self.farm.bag.add(item, 1) {
            1
        } else {
            0
        }
    }

    /// The farmer's skills as the player's fields carry them.
    fn skill_lines(&self) -> Vec<player::SkillLine> {
        (SKILL_LINES.iter())
            .map(|&(skill, id)| {
                let (level, cap) = self.farm.skills.get(skill);
                (id, level, cap)
            })
            .collect()
    }

    /// Show grove tree `t` as it stands    /// Show grove tree `t` as it stands: a tree, a stump the day it fell, then a sapling.
    fn show_tree(&mut self, t: usize) {
        let (kind, at) = self.layout.trees[t];
        let (entry, scale) = match self.farm.tree(t) {
            TreeStage::Tree => (TREE_BASE + kind as u32, TREES[kind].2),
            // A felled tree leaves nothing until its sapling comes up.
            TreeStage::Stump => (0, 0.0),
            TreeStage::Sapling => (SAPLING, 1.0),
        };
        if self.trees[t].1 == entry {
            return;
        }
        if self.trees[t].0 != 0 {
            self.send(crate::world::destroy(self.trees[t].0));
        }
        if entry == 0 {
            self.trees[t] = (0, 0);
            return;
        }
        let guid = self.fresh_guid(entry);
        let tmpl = object_template(entry).expect("a grove object");
        self.send(objects::create(
            guid,
            entry,
            &tmpl,
            at,
            t as f32 * 1.7,
            scale,
        ));
        self.trees[t] = (guid, entry);
    }

    /// Start chopping tree `t`: the axe comes out and swings until it falls.
    fn start_chop(&mut self, t: usize) {
        if !self.farm.has(Unlock::Axe) {
            return self.refuse(&Reject::Locked(Unlock::Axe));
        }
        self.ride(false);
        if distance(self.at, self.layout.trees[t].1) > TREE_REACH {
            return self.notify("You need to be closer.");
        }
        if self.farm.tree(t) != TreeStage::Tree {
            return self.notify(&Reject::Felled.text());
        }
        self.take_tool(spells::AXE_SLOT, Some(items::LUMBERJACK_AXE));
        self.send(player::sheath_change(self.guid(), true));
        self.chop = Some(Chop {
            tree: t,
            swings: farm_rules::skills::swings(self.farm.skills.get(Skill::Lumberjacking).0),
            next: Duration::ZERO,
        });
    }

    /// Run the chop on: a swing every [`SWING_EVERY`], and after the last the tree falls.
    fn tick_chop(&mut self, dt: Duration) {
        let Some(mut chop) = self.chop.take() else {
            return;
        };
        if chop.next > dt {
            chop.next -= dt;
            self.chop = Some(chop);
            return;
        }
        if chop.swings > 0 {
            self.send(packets::emote(self.guid(), SWING));
            let sound = if chop.swings == 1 {
                SOUND_CHOP_LAST
            } else {
                SOUND_CHOP
            };
            self.send(packets::play_sound(sound));
            chop.swings -= 1;
            chop.next = self.quick(Skill::Lumberjacking, SWING_EVERY);
            self.chop = Some(chop);
            return;
        }
        self.send(player::sheath_change(self.guid(), false));
        match self.farm.chop(chop.tree) {
            Ok(n) => {
                let n = n + self.bonus(Skill::Lumberjacking, Item::Wood);
                self.sync_bag(Some((Item::Wood, n)), false);
                self.practice(Skill::Lumberjacking, 1);
                self.show_tree(chop.tree);
                self.send(packets::play_sound(SOUND_TREE_FALL));
                self.say("Timber!");
            }
            Err(r) => self.refuse(&r),
        }
    }

    fn stop_chop(&mut self) {
        if self.chop.take().is_some() {
            self.send(player::sheath_change(self.guid(), false));
        }
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
        let at = self.tile_pos(i);
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
            let facing = crate::layout::FURROW_FACING + MOUND_TURN;
            self.send(objects::create(guid, soil, &t, at, facing, scale));
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
        let till = self.farm.has(Unlock::Tilling);
        let tools = [
            true,
            self.farm.has(Unlock::WateringCan),
            self.farm.has(Unlock::Sickle),
            self.farm.has(Unlock::Axe),
            self.farm.has(Unlock::MiningPick),
            self.farm.has(Unlock::Fence),
            self.farm.has(Unlock::Horse),
        ];
        // The seeds carried, in crop order, as many as the bar has slots after the tools.
        let bar: Vec<u32> = (Crop::ALL.iter())
            .filter(|&&c| self.farm.bag.count(Item::Seeds(c)) > 0)
            .map(|&c| items::entry(Item::Seeds(c)))
            .take(12 - spells::SEED_SLOT0)
            .collect();
        if bar != self.bar || tools != self.bar_tools || till != self.bar_till {
            self.send(spells::item_bar(&bar, &tools, till));
            self.bar = bar;
            self.bar_tools = tools;
            self.bar_till = till;
            self.send_ui();
        }
        self.sync_quest_log();
        self.refresh_markers();
    }

    /// `.sel <n>`: the farmer picked bar slot `n` (from 1) with the wheel or a number key.
    fn select(&mut self, slot: usize) {
        self.selected = slot.saturating_sub(1);
        self.lit = self.selected;
        if let Some(k) = self.selected.checked_sub(spells::SEED_SLOT0) {
            self.seed_pick = k;
        }
        tracing::debug!("farm: {} picks bar slot {slot}", self.me.name);
    }

    /// The seed to sow: the one last picked on the bar, or the first carried.
    fn selected_seed(&self) -> Option<Crop> {
        let entry = self.bar.get(self.seed_pick).or(self.bar.first())?;
        match items::item_of(*entry) {
            Some(Item::Seeds(c)) if self.farm.bag.count(Item::Seeds(c)) > 0 => Some(c),
            _ => None,
        }
    }

    /// What the hotbar's Lua keeps: whether the shovel digs, and the lit slot.
    fn send_ui(&self) {
        let till = if self.farm.has(Unlock::Tilling) {
            "1"
        } else {
            "0"
        };
        self.send(packets::addon(self.guid(), "FarmcraftTill", till));
        let lit = (self.lit + 1).to_string();
        self.send(packets::addon(self.guid(), "FarmcraftTool", &lit));
    }

    /// The right tool for the job, in hand and lit on the bar.
    fn take_tool(&mut self, slot: usize, hold: Option<u32>) {
        if let Some(entry) = hold {
            self.wield(entry);
        }
        if self.lit != slot {
            self.lit = slot;
            let line = (slot + 1).to_string();
            self.send(packets::addon(self.guid(), "FarmcraftTool", &line));
        }
    }

    /// How long a job of `skill` takes the farmer, `base` for the unskilled.
    fn quick(&self, skill: Skill, base: Duration) -> Duration {
        base.mul_f32(farm_rules::skills::quickness(self.farm.skills.get(skill).0))
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

    /// Put tool `entry` in hand: the shovel for the soil, the axe for the trees.
    fn wield(&mut self, entry: u32) {
        if self.tool != entry {
            self.tool = entry;
            self.send(player::wield_change(self.guid(), entry));
        }
    }

    fn start(&mut self, spell: u32, target: Target, time: Duration, job: Job) {
        self.menu = None;
        self.ride(false);
        let (slot, hold) = match job {
            Job::Plant(..) => (0, items::SHOVEL),
            Job::Till(_) => (0, items::SHOVEL),
            Job::Water(_) => (spells::CAN_SLOT, items::SHOVEL),
            Job::Harvest(_) | Job::Forage(_) => (spells::SICKLE_SLOT, items::SICKLE),
            Job::Mine(_) => (spells::PICK_SLOT, items::MINING_PICK),
        };
        self.take_tool(slot, Some(hold));
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
        if self.cast.is_some() || self.asleep.is_some() || self.chop.is_some() {
            return;
        }
        if let Some(i) = self.tile_of(object) {
            self.work_tile(i, object);
        } else if let Some(v) = self.veins.iter().position(|&g| g == Some(object)) {
            // A client that sends a plain use for the vein still swings the pick.
            if !self.near(self.layout.veins[v].0) {
                return self.notify("You need to be closer.");
            }
            if !self.farm.has(Unlock::MiningPick) {
                return self.refuse(&Reject::Locked(Unlock::MiningPick));
            }
            self.start(
                spells::MINING,
                Target::GameObject(object),
                self.quick(Skill::Mining, MINE_TIME),
                Job::Mine(v),
            );
        } else if self.fence_start.is_some_and(|(_, g)| g == object) {
            if let Some((_, marker)) = self.fence_start.take() {
                self.send(crate::world::destroy(marker));
            }
            self.say("Fence line finished.");
        } else if let Some(f) = self.fences.iter().position(|&g| g == object) {
            if self.farm.fences[f].built {
                match self.farm.unbuild(f) {
                    Ok(()) => {
                        self.send(crate::world::destroy(object));
                        self.fences.remove(f);
                        self.send(packets::play_sound(SOUND_WOOD_UP));
                        self.sync_bag(Some((Item::Wood, farm_rules::FENCE_WOOD)), false);
                    }
                    Err(r) => self.refuse(&r),
                }
            } else {
                match self.farm.finish_fence(f) {
                    Ok(()) => {
                        self.send(crate::world::destroy(object));
                        self.fences[f] = self.show_fence(f);
                        self.send(packets::play_sound(SOUND_WOOD_DOWN));
                        self.sync_bag(None, false);
                        self.noted(Deed::BuildFence);
                        self.practice(Skill::Woodworking, 1);
                    }
                    Err(r) => self.refuse(&r),
                }
            }
        } else if let Some(t) = self.trees.iter().position(|&(g, _)| g == object) {
            self.start_chop(t);
        } else if let Some(w) = self.wild.iter().position(|&g| g == Some(object)) {
            self.forage(w, object);
        } else if object == self.fixtures[0] {
            self.go_to_bed();
        } else if object == self.fixtures[1] && object != 0 {
            self.ship();
        }
    }

    fn work_tile(&mut self, i: usize, object: u64) {
        let at = self.tile_pos(i);
        if !self.near(at) {
            return self.notify("You need to be closer.");
        }
        let tile = self.farm.tiles[i];
        match tile.crop {
            None => match self.selected_seed() {
                Some(crop) => self.start(
                    spells::PLANT_SEEDS,
                    Target::GameObject(object),
                    self.quick(Skill::Farming, PLANT_TIME),
                    Job::Plant(i, crop),
                ),
                None => self.notify("You have no seeds. Brog Hamfist in Goldshire sells them."),
            },
            Some((crop, age)) => {
                if tile.ripe().is_some() {
                    if !self.farm.has(Unlock::Sickle) {
                        return self.refuse(&Reject::Locked(Unlock::Sickle));
                    }
                    return self.start(
                        spells::HERB_GATHERING,
                        Target::GameObject(object),
                        self.quick(Skill::Herbalism, HARVEST_TIME),
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
                if !self.farm.has(Unlock::WateringCan) {
                    return self.refuse(&Reject::Locked(Unlock::WateringCan));
                }
                self.start(
                    spells::WATERING,
                    Target::GameObject(object),
                    self.quick(Skill::Farming, WATER_TIME),
                    Job::Water(i),
                );
            }
        }
    }

    /// The client's own cast: Herb Gathering on a ripe crop, Mining on a vein.
    pub fn cast(&mut self, cast: CastRequest) {
        let refuse = |g: &Game| g.send(spells::cast_failed(cast.spell, spells::FAILED_SILENT));
        if (self.cast.is_some() || self.asleep.is_some()) && cast.spell != spells::RIDE {
            return refuse(self);
        }
        if let Target::Ground(p) = cast.target {
            // The press is shown as another spell or none: free the client's button.
            self.send(spells::cast_ok(cast.spell));
            return match cast.spell {
                spells::TILL => self.till_at(p),
                spells::BUILD_FENCE => self.build_at(p),
                _ => {}
            };
        }
        if cast.spell == spells::RIDE {
            self.send(spells::cast_ok(cast.spell));
            if !self.farm.has(Unlock::Horse) {
                return self.refuse(&Reject::Locked(Unlock::Horse));
            }
            let mount = !self.mounted;
            return self.ride(mount);
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
        if job.is_some_and(|j| matches!(j, Job::Mine(_))) && !self.farm.has(Unlock::MiningPick) {
            refuse(self);
            return self.refuse(&Reject::Locked(Unlock::MiningPick));
        }
        let gathers = job.is_some_and(|j| matches!(j, Job::Harvest(_) | Job::Forage(_)));
        if gathers && !self.farm.has(Unlock::Sickle) {
            refuse(self);
            return self.refuse(&Reject::Locked(Unlock::Sickle));
        }
        let (Some(job), time) = (
            job,
            if cast.spell == spells::MINING {
                self.quick(Skill::Mining, MINE_TIME)
            } else {
                self.quick(Skill::Herbalism, HARVEST_TIME)
            },
        ) else {
            return refuse(self);
        };
        let at = match job {
            Job::Mine(v) => self.layout.veins[v].0,
            Job::Harvest(i) => self.tile_pos(i),
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
        let step = distance(at, self.at);
        let walked = step > 0.3;
        if self.mounted && step < 30.0 {
            self.ridden += step;
            if self.ridden >= RIDE_PRACTICE {
                self.ridden = 0.0;
                self.practice(Skill::Riding, 1);
            }
        }
        self.at = at;
        self.facing = facing;
        self.mark_home();
        if walked {
            self.interrupt();
            self.stop_chop();
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
                    self.noted(Deed::Plant);
                    self.practice(Skill::Farming, herb_difficulty(crop));
                    if !self.farm.tiles[i].watered {
                        self.say(&format!(
                            "You planted {}. Right-click it to water it.",
                            crop.name()
                        ));
                    }
                }
                Err(r) => self.refuse(&r),
            },
            Job::Till(cell) => {
                if self.farm.till(cell) {
                    self.practice(Skill::Farming, 1);
                    self.soil.push((0, 0));
                    self.crops.push(None);
                    let i = self.farm.tiles.len() - 1;
                    self.show_tile(i);
                }
            }
            Job::Water(i) => match self.farm.water(i) {
                Ok(()) => {
                    self.show_tile(i);
                    self.noted(Deed::Water);
                    if let Some((crop, _)) = self.farm.tiles[i].crop {
                        self.practice(Skill::Farming, herb_difficulty(crop));
                    }
                }
                Err(r) => self.refuse(&r),
            },
            Job::Forage(w) => {
                let crop = self.layout.wild[w].0;
                match self.farm.forage(w, crop) {
                    Ok((n, seed)) => {
                        let n = n + self.bonus(Skill::Herbalism, Item::Produce(crop));
                        self.sync_bag(Some((Item::Produce(crop), n)), false);
                        self.practice(Skill::Herbalism, herb_difficulty(crop));
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
                    let n = n + self.bonus(Skill::Herbalism, Item::Produce(crop));
                    self.sync_bag(Some((Item::Produce(crop), n)), false);
                    self.show_tile(i);
                    self.practice(Skill::Herbalism, herb_difficulty(crop));
                }
                Err(r) => self.refuse(&r),
            },
            Job::Mine(v) => match self.farm.mine(v) {
                Ok(n) => {
                    let n = n + self.bonus(Skill::Mining, Item::CopperOre);
                    self.sync_bag(Some((Item::CopperOre, n)), false);
                    self.show_vein(v);
                    self.practice(Skill::Mining, 1);
                    for _ in 0..n {
                        self.noted(Deed::Mine);
                    }
                }
                Err(r) => self.refuse(&r),
            },
        }
    }

    fn forage(&mut self, w: usize, object: u64) {
        if !self.near(self.layout.wild[w].1) {
            return self.notify("You need to be closer.");
        }
        if !self.farm.has(Unlock::Sickle) {
            return self.refuse(&Reject::Locked(Unlock::Sickle));
        }
        self.start(
            spells::HERB_GATHERING,
            Target::GameObject(object),
            self.quick(Skill::Herbalism, HARVEST_TIME),
            Job::Forage(w),
        );
    }

    fn go_to_bed(&mut self) {
        if !self.near(self.layout.bed) {
            return self.notify("You need to be closer.");
        }
        self.lie_down();
    }

    fn lie_down(&mut self) {
        self.ride(false);
        self.say("You settle into bed...");
        self.send(player::stand_change(self.guid(), 3));
        self.asleep = Some(SLEEP_TIME);
    }

    /// `CMSG_TEXT_EMOTE`: the emote plays for the farmer, and `/sleep` in the farmhouse ends
    /// the day.
    pub fn text_emote(&mut self, id: u32) {
        self.send(packets::text_emote(self.guid(), id));
        if id != TEXT_EMOTE_SLEEP || self.asleep.is_some() || self.cast.is_some() {
            return;
        }
        if distance(self.at, self.layout.bed) <= HOUSE_REACH {
            self.lie_down();
        } else {
            self.notify("Sleep inside the farmhouse to end the day.");
        }
    }

    /// `CMSG_CANCEL_AURA`: the farmer clicked off a buff; any of them steps down off the horse.
    pub fn cancel_aura(&mut self) {
        self.ride(false);
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
        for i in 0..self.farm.tiles.len() {
            self.show_tile(i);
        }
        for v in 0..self.layout.veins.len() {
            self.show_vein(v);
        }
        for w in 0..self.layout.wild.len() {
            self.show_wild(w);
        }
        for t in 0..self.layout.trees.len() {
            self.show_tree(t);
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
        if self.previewing {
            return;
        }
        self.saves.store(self.guid(), &self.farm);
        self.since_save = Duration::ZERO;
    }

    /// Run `dt` of the game: casts finish, the day runs and the dog follows.
    /// Keeps a house on the map where the farm is, whenever the farmer is away from it.
    fn mark_home(&mut self) {
        let home = self.layout.home.position;
        if (self.at[0] - home[0]).hypot(self.at[1] - home[1]) < MARKER_NEAR {
            self.marker_age = None;
            return;
        }
        if self.marker_age.is_some_and(|age| age < MARKER_REFRESH) {
            return;
        }
        self.marker_age = Some(Duration::ZERO);
        self.send(packets::gossip_poi(
            home[0],
            home[1],
            HOUSE_ICON,
            "Stonefield Farm",
        ));
    }

    pub fn tick(&mut self, dt: Duration) {
        self.tick_chop(dt);
        if let Some(age) = &mut self.marker_age {
            *age += dt;
        }
        self.since_wander += dt;
        if self.since_wander >= WANDER_EVERY && !self.critter_at.is_empty() {
            self.since_wander = Duration::ZERO;
            self.wander();
        }
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
                self.noted(Deed::Sleep);
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
                22 => self.say("It's getting late. Type /sleep in the farmhouse to go to bed."),
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

    /// One wild critter strolls to another of its spots.
    fn wander(&mut self) {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let c = rng.gen_range(0..self.critter_at.len());
        let (npc, spots) = &self.layout.critters[c];
        let to = spots[rng.gen_range(0..spots.len())];
        let from = self.critter_at[c];
        if distance(from, to) < 0.5 {
            return;
        }
        let entry = NPC_BASE + *npc as u32;
        let guid = HIGH_UNIT << 48 | u64::from(entry) << 24 | *npc as u64;
        let ms = (distance(from, to) / 2.0 * 1000.0) as u32;
        self.send(creatures::move_to(guid, from, to, ms, false));
        self.critter_at[c] = to;
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
            Role::Quest(v) => self.villager_hello(npc, v),
            Role::Horse => {
                if distance(self.at, self.layout.npcs[i].at) > REACH {
                    return self.notify("You need to be closer.");
                }
                if self.farm.has(Unlock::Horse) {
                    self.ride(true);
                }
            }
            Role::Animal(item, a) => {
                if distance(self.at, self.layout.npcs[i].at) > REACH {
                    return self.notify("You need to be closer.");
                }
                match self.farm.collect(a, item) {
                    Ok(()) => {
                        let n = 1 + self.bonus(Skill::Shepherding, item);
                        self.sync_bag(Some((item, n)), false);
                        if item == Item::Egg {
                            self.noted(Deed::GatherEgg);
                        }
                        let difficulty = match item {
                            Item::Egg => 1,
                            Item::Milk => 25,
                            _ => 50,
                        };
                        self.practice(Skill::Shepherding, difficulty);
                    }
                    Err(r) => self.refuse(&r),
                }
            }
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
        let pet = match self.menu.take() {
            Some(Menu::Adopt(pet)) => pet,
            Some(Menu::Shop(npc)) => {
                self.send(packets::gossip_complete());
                return self.list_inventory(npc);
            }
            None => return,
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
            TEXT_FARLEY => self.send(packets::npc_text(id, FARLEY)),
            TEXT_BROG => self.send(packets::npc_text(id, BROG)),
            TEXT_KHELDEN => self.send(packets::npc_text(id, KHELDEN)),
            _ => {}
        }
    }

    fn npc_index(&self, guid: u64) -> Option<usize> {
        let entry = ((guid >> 24) & 0xFF_FFFF) as u32;
        let i = entry.checked_sub(NPC_BASE)? as usize;
        (guid >> 48 == HIGH_UNIT && i < self.layout.npcs.len()).then_some(i)
    }

    /// Whether `guid` is the villager `v`.
    fn is(&self, guid: u64, v: farm_rules::Villager) -> bool {
        self.npc_index(guid)
            .is_some_and(|i| self.layout.npcs[i].role == Role::Quest(v))
    }

    /// Brog's shelf: every seed, and the axe and the pick once a quest sends the farmer for them.
    fn wares(&self) -> Vec<Ware> {
        let ware = |entry: u32, price: u32| Ware {
            entry,
            display: items::template(entry).map_or(0, |t| t.display_id),
            price,
        };
        // While Seeds for Spring is on, only seeds the farmer can still buy enough of: the quest
        // can't be spent out of reach.
        let seeds_left = (self.farm.quests.is_active(4))
            .then(|| 10u32.saturating_sub(self.farm.quests.count(4)))
            .filter(|&n| n > 0);
        let mut wares: Vec<Ware> = (Crop::ALL.iter())
            .filter(|c| seeds_left.is_none_or(|n| c.def().seed_price * n <= self.farm.money))
            .map(|&c| ware(items::entry(Item::Seeds(c)), c.def().seed_price))
            .collect();
        let sent_for = |q: usize| self.farm.quests.done > q || self.farm.quests.is_active(q);
        if sent_for(6) && !self.farm.has(Unlock::Axe) {
            wares.push(ware(items::LUMBERJACK_AXE, AXE_PRICE));
        }
        if sent_for(9) && !self.farm.has(Unlock::MiningPick) {
            wares.push(ware(items::MINING_PICK, PICK_PRICE));
        }
        wares
    }

    /// The merchant window: Brog's shelf, or Farley's empty one to sell produce into.
    pub fn list_inventory(&self, vendor: u64) {
        if self.is(vendor, farm_rules::Villager::Brog) && self.farm.has(Unlock::Market) {
            self.send(packets::list_inventory(vendor, &self.wares()));
        } else if self.is(vendor, farm_rules::Villager::Farley) && self.farm.has(Unlock::Market) {
            self.send(packets::list_inventory(vendor, &[]));
        }
    }

    pub fn buy(&mut self, vendor: u64, entry: u32, count: u8) {
        use benilla_protocol::messages::buy_result;
        if !self.is(vendor, farm_rules::Villager::Brog) {
            return;
        }
        let wares = self.wares();
        let Some(row) = wares.iter().position(|w| w.entry == entry) else {
            return self.send(packets::buy_failed(
                vendor,
                entry,
                buy_result::CANT_FIND_ITEM,
            ));
        };
        let tool = match entry {
            items::LUMBERJACK_AXE => Some(Unlock::Axe),
            items::MINING_PICK => Some(Unlock::MiningPick),
            _ => None,
        };
        if let Some(u) = tool {
            return match self.farm.buy_tool(u, wares[row].price) {
                Ok(()) => {
                    self.send(packets::buy_item(vendor, row as u32 + 1, 1));
                    self.sync_money();
                    self.unlocked(u);
                    self.save();
                }
                Err(_) => self.send(packets::buy_failed(
                    vendor,
                    entry,
                    buy_result::NOT_ENOUGH_MONEY,
                )),
            };
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
                self.send(packets::buy_item(vendor, row as u32 + 1, n));
                self.sync_money();
                self.sync_bag(Some((item, n)), true);
                if matches!(item, Item::Seeds(_)) {
                    let shelf = self.wares();
                    for _ in 0..n {
                        self.noted(Deed::BuySeeds);
                    }
                    if self.wares() != shelf {
                        self.list_inventory(vendor);
                    }
                }
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
        let buyer = self.is(vendor, farm_rules::Villager::Farley) && self.farm.has(Unlock::Market);
        if !buyer {
            if self.is(vendor, farm_rules::Villager::Brog) {
                self.say("Brog Hamfist says: I sell, friend. Innkeeper Farley buys produce.");
            }
            return self.send(packets::sell_failed(
                vendor,
                item_guid,
                sell_result::CANT_SELL_ITEM,
            ));
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
                self.noted(Deed::Sleep);
            }
            ".rain" => {
                self.farm.weather = Weather::Rain;
                for t in &mut self.farm.tiles {
                    t.watered = t.crop.is_some() && t.ripe().is_none();
                }
                self.send(self.weather_packet(false));
                for i in 0..self.farm.tiles.len() {
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
            ".chop" => {
                let nearest = (0..self.layout.trees.len()).min_by(|&a, &b| {
                    let d = |t: usize| distance(self.at, self.layout.trees[t].1);
                    d(a).total_cmp(&d(b))
                });
                if let Some(t) = nearest {
                    self.start_chop(t);
                }
            }
            ".gps" => self.say(&format!(
                "You stand at {:.1}, {:.1}, {:.1}, facing {:.2}.",
                self.at[0], self.at[1], self.at[2], self.facing
            )),
            // Opens a villager's window as a right-click would, for scripted runs.
            ".talk" => {
                let who = match words.next() {
                    Some("farley") => farm_rules::Villager::Farley,
                    Some("brog") => farm_rules::Villager::Brog,
                    _ => farm_rules::Villager::Ma,
                };
                let npc = self.villager_guid(who);
                self.gossip_hello(npc);
            }
            ".ride" => self.toggle_ride(),
            ".markers" => self.send_markers(),
            ".ui" => self.send_ui(),
            ".preview" => self.preview(words.next().and_then(|w| w.parse().ok())),
            ".quest" => {
                if let Some(n) = words.next().and_then(|w| w.parse::<usize>().ok()) {
                    self.skip_to_quest(n.saturating_sub(1));
                }
            }
            ".gold" => {
                self.farm.money += 10_000;
                self.sync_money();
            }
            _ => self.say(
                "Commands: .time, .home (back to the farm), and for testing: .sleep, .rain, \
                 .hour, .gold, .quest <n>",
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

    fn new_game(unlocked: bool) -> (Game, UnboundedReceiver<Packet>) {
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
        let mut farm = saves.farm(1).unwrap();
        if unlocked {
            farm.unlocked = [
                Unlock::WateringCan,
                Unlock::Horse,
                Unlock::Market,
                Unlock::ShippingCrate,
                Unlock::Axe,
                Unlock::Fence,
                Unlock::Hens,
                Unlock::Herd,
                Unlock::MiningPick,
                Unlock::Tilling,
                Unlock::Sickle,
            ]
            .into_iter()
            .collect();
            farm.bag.add(Item::Seeds(Crop::Peacebloom), 4);
        }
        let (tx, rx) = unbounded_channel();
        let mut g = Game::new(me, farm, layout, saves, tx);
        g.enter();
        (g, rx)
    }

    /// A farmer who has everything the quests unlock.
    fn game() -> (Game, UnboundedReceiver<Packet>) {
        new_game(true)
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
        assert_eq!(g.selected, spells::SEED_SLOT0);
        g.use_object(g.soil[0].0);
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles[0].crop, Some((Crop::Silverleaf, 0)));
        assert_eq!(g.farm.bag.count(Item::Seeds(Crop::Silverleaf)), 7);
        let crop = g.crops[0].expect("the crop stands").0;

        for _ in 0..Crop::Silverleaf.def().days {
            if !g.farm.tiles[0].watered {
                let crop = g.crops[0].expect("growing").0;
                g.command(".sel 2");
                g.use_object(crop);
                assert!(g.cast.is_some(), "watering takes a moment");
                run(&mut g, 1.5);
                g.command(".sel 6");
                assert!(g.farm.tiles[0].watered);
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
        // Slot 8 holds the Silverleaf, slot 9 the Peacebloom.
        g.command(".sel 9");
        g.use_object(g.soil[1].0);
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles[1].crop.map(|c| c.0), Some(Crop::Peacebloom));
        assert_eq!(g.lit, 0, "planting lights the shovel");
        // A tool slot picked keeps the seed: the next empty mound gets Peacebloom too.
        g.command(".sel 4");
        let tile = g.layout.tiles[2];
        stand_at(&mut g, tile);
        g.use_object(g.soil[2].0);
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles[2].crop.map(|c| c.0), Some(Crop::Peacebloom));
        // When the last Peacebloom seed goes, the bar closes up and the first seed is sown.
        g.farm.bag.take(Item::Seeds(Crop::Peacebloom), 2);
        g.sync_bag(None, false);
        assert_eq!(g.bar.len(), 1);
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
    fn three_swings_fell_a_tree_into_wood_and_a_stump_that_regrows() {
        let (mut g, mut rx) = game();
        let at = g.layout.trees[0].1;
        stand_at(&mut g, [at[0] + 2.0, at[1], at[2]]);
        let tree = g.trees[0].0;
        g.use_object(tree);
        assert!(g.chop.is_some());
        run(&mut g, 4.0);
        let seen = drain(&mut rx);
        let swings = (seen.iter())
            .filter(|p| matches!(p, ServerPacket::Emote { .. }))
            .count();
        assert_eq!(swings, farm_rules::skills::swings(1) as usize);
        assert!(g.farm.bag.count(Item::Wood) >= 2);
        assert_eq!(g.trees[0], (0, 0), "a felled tree leaves nothing");
        assert_eq!(g.tool, items::LUMBERJACK_AXE);
        g.command(".sleep");
        assert_eq!(g.trees[0].1, SAPLING);
        g.command(".sleep");
        g.command(".sleep");
        assert_eq!(g.trees[0].1, TREE_BASE + g.layout.trees[0].0 as u32);
        drain(&mut rx);
    }

    #[test]
    fn till_soil_digs_a_new_tile_in_the_field_only() {
        let (mut g, mut rx) = game();
        let before = g.farm.tiles.len();
        let cell = (1, 7);
        let p = g
            .layout
            .cell_pos(cell)
            .expect("a field cell beside the plot");
        g.moved(p, 0.0);
        // The very bytes the client sends for a ground-targeted press.
        let body = benilla_protocol::messages::cast_spell_at_dest(spells::TILL, p);
        g.cast(CastRequest::read(&body).unwrap());
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles.len(), before + 1);
        assert_eq!(g.farm.dug, vec![cell]);
        assert_eq!(g.tile_pos(before), p);
        assert_ne!(g.soil[before].0, 0, "the new mound stands");
        // The same cell again, and a spot off the field, dig nothing.
        g.cast(CastRequest {
            spell: spells::TILL,
            target: Target::Ground(p),
        });
        run(&mut g, 2.0);
        g.cast(CastRequest {
            spell: spells::TILL,
            target: Target::Ground([p[0] + 500.0, p[1], p[2]]),
        });
        run(&mut g, 2.0);
        assert_eq!(g.farm.tiles.len(), before + 1);
        drain(&mut rx);
    }

    #[test]
    fn a_fence_line_runs_sections_and_marks_out_what_the_wood_cannot_cover() {
        let (mut g, mut rx) = game();
        let a = [g.at[0] + 2.0, g.at[1], g.at[2]];
        let b = [a[0] + FENCE_LENGTH * 3.0, a[1], a[2]];
        g.farm.bag.add(Item::Wood, 4);
        let click = |g: &mut Game, p: [f32; 3]| {
            let body = benilla_protocol::messages::cast_spell_at_dest(spells::BUILD_FENCE, p);
            g.cast(CastRequest::read(&body).unwrap());
        };
        click(&mut g, a);
        assert!(g.fence_start.is_some());
        click(&mut g, b);
        assert_eq!(g.farm.fences.len(), 3);
        assert_eq!(g.farm.fences.iter().filter(|f| f.built).count(), 2);
        assert_eq!(g.farm.bag.count(Item::Wood), 0);
        // Sections join end to end along the line.
        let ends = g.fence_ends();
        assert!(ends.iter().any(|e| distance(*e, b) < 0.01));
        // The line carries on from its end; a click there finishes it.
        assert!(g.fence_start.is_some_and(|(p, _)| distance(p, b) < 0.01));
        click(&mut g, b);
        assert!(g.fence_start.is_none());
        // A marked-out section goes up once there is wood.
        g.farm.bag.add(Item::Wood, 2);
        let planned = g.farm.fences.iter().position(|f| !f.built).unwrap();
        let marker = g.fences[planned];
        g.use_object(marker);
        assert!(g.farm.fences[planned].built);
        drain(&mut rx);
    }

    #[test]
    fn the_horse_speeds_the_farmer_up_and_work_brings_them_down() {
        let (mut g, mut rx) = game();
        g.cast(CastRequest {
            spell: spells::RIDE,
            target: Target::Caster,
        });
        assert!(g.mounted);
        let tile = g.layout.tiles[0];
        g.moved(tile, 0.0);
        g.use_object(g.soil[0].0);
        assert!(!g.mounted, "planting dismounts");
        drain(&mut rx);
    }

    #[test]
    fn critters_wander_between_their_spots() {
        let (mut g, mut rx) = game();
        assert!(!g.layout.critters.is_empty());
        drain(&mut rx);
        run(&mut g, 10.0);
        let moves = (drain(&mut rx).iter())
            .filter(|p| matches!(p, ServerPacket::MonsterMove { .. }))
            .count();
        assert!(moves > 3, "{moves} moves");
    }

    #[test]
    fn a_cow_gives_milk_once_a_day() {
        let (mut g, mut rx) = game();
        let i = (g.layout.npcs.iter())
            .position(|n| matches!(n.role, Role::Animal(Item::Milk, _)))
            .expect("a cow");
        let at = g.layout.npcs[i].at;
        g.moved(at, 0.0);
        let guid = HIGH_UNIT << 48 | u64::from(NPC_BASE + i as u32) << 24 | i as u64;
        g.gossip_hello(guid);
        g.gossip_hello(guid);
        assert_eq!(g.farm.bag.count(Item::Milk), 1);
        g.command(".sleep");
        g.gossip_hello(guid);
        assert_eq!(g.farm.bag.count(Item::Milk), 2);
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
    fn brog_sells_seeds_and_farley_buys_the_harvest() {
        let (mut g, mut rx) = game();
        let guid = g.villager_guid(farm_rules::Villager::Brog);
        let farley = g.villager_guid(farm_rules::Villager::Farley);
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
        assert_eq!(
            g.farm.bag.count(Item::Seeds(Crop::Pumpkin)),
            1,
            "Brog only sells"
        );
        g.sell(farley, held.guid, 0);
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
    fn the_farm_is_on_the_map_while_the_farmer_is_away() {
        let (mut g, mut rx) = game();
        let markers = |rx: &mut UnboundedReceiver<Packet>| {
            (drain(rx).iter())
                .filter(|p| matches!(p, ServerPacket::GossipPoi(_)))
                .count()
        };
        markers(&mut rx);
        let home = g.layout.home.position;
        stand_at(&mut g, home);
        assert_eq!(markers(&mut rx), 0);
        let away = [home[0] + 40.0, home[1], home[2]];
        stand_at(&mut g, away);
        stand_at(&mut g, [away[0] + 1.0, away[1], away[2]]);
        assert_eq!(markers(&mut rx), 1);
        run(&mut g, 421.0);
        stand_at(&mut g, away);
        assert_eq!(markers(&mut rx), 1);
        stand_at(&mut g, home);
        stand_at(&mut g, away);
        assert_eq!(markers(&mut rx), 1);
    }

    #[test]
    fn the_quests_walk_a_new_farmer_from_the_shovel_to_market() {
        use farm_rules::Villager;
        let (mut g, mut rx) = new_game(false);
        let ma = g.villager_guid(Villager::Ma);
        let farley = g.villager_guid(Villager::Farley);
        let brog = g.villager_guid(Villager::Brog);
        let statuses = |rx: &mut UnboundedReceiver<Packet>| {
            (drain(rx).into_iter())
                .filter_map(|p| match p {
                    ServerPacket::QuestGiverStatus { npc, status } => Some((npc, status)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(statuses(&mut rx).contains(&(ma, 5)), "Ma has a yellow !");
        // Only the shovel and the seeds are on the bar; there is no can to water with.
        assert_eq!(
            g.bar_tools,
            [true, false, false, false, false, false, false]
        );
        assert!(
            !g.bar_till,
            "the shovel plants; it digs once Brog gives the hoe"
        );

        // Breaking Ground: plant all eight seeds.
        g.gossip_hello(ma);
        assert!(drain(&mut rx)
            .iter()
            .any(|p| matches!(p, ServerPacket::QuestGiverDetails(_))));
        g.quest_accept(ma, 900);
        assert!(g.farm.quests.is_active(0));
        for i in 0..8 {
            let tile = g.tile_pos(i);
            stand_at(&mut g, tile);
            g.use_object(g.soil[i].0);
            run(&mut g, 2.0);
        }
        assert_eq!(g.farm.quests.count(0), 8);
        assert!(statuses(&mut rx).contains(&(ma, 7)), "Ma has a yellow ?");
        g.quest_complete(ma, 900);
        g.quest_choose(ma, 900);
        assert!(g.farm.has(Unlock::WateringCan));
        assert!(g.bar_tools[1], "the can is on the bar");
        let next = drain(&mut rx);
        assert!(next
            .iter()
            .any(|p| matches!(p, ServerPacket::QuestGiverComplete(_))));
        assert!(
            next.iter()
                .any(|p| matches!(p, ServerPacket::QuestGiverDetails(_))),
            "Ma offers the next quest at once"
        );

        // Thirsty Work, Early to Bed.
        g.quest_accept(ma, 901);
        g.select(spells::CAN_SLOT + 1);
        for i in 0..8 {
            let tile = g.tile_pos(i);
            stand_at(&mut g, tile);
            g.use_object(g.crops[i].unwrap().0);
            run(&mut g, 1.5);
        }
        g.quest_choose(ma, 901);
        g.quest_accept(ma, 902);
        g.command(".sleep");
        g.quest_choose(ma, 902);
        assert!(g.farm.has(Unlock::Horse));

        // Market Day ends with Farley in Goldshire, who sends the farmer to Brog for seeds.
        g.quest_accept(ma, 903);
        drain(&mut rx);
        g.quest_status(farley);
        assert!(statuses(&mut rx).contains(&(farley, 7)));
        g.quest_choose(farley, 903);
        assert!(g.farm.has(Unlock::Market));
        g.quest_accept(farley, 904);
        g.farm.money = 1000;
        g.buy(brog, items::entry(Item::Seeds(Crop::Peacebloom)), 10);
        assert_eq!(g.farm.quests.count(4), 10);
        g.quest_choose(brog, 904);
        assert_eq!(g.farm.quests.done, 5);
        assert!(g.bar_till, "Brog's hoe: the shovel digs");
        drain(&mut rx);
    }

    #[test]
    fn sleep_in_the_house_the_horse_by_it_and_skills_that_grow() {
        let (mut g, mut rx) = game();
        // /sleep away from the house only plays the emote; in it, the day ends.
        g.text_emote(TEXT_EMOTE_SLEEP);
        assert!(g.asleep.is_none());
        let bed = g.layout.bed;
        stand_at(&mut g, [bed[0] + 3.0, bed[1], bed[2]]);
        let day = g.farm.clock.day;
        g.text_emote(TEXT_EMOTE_SLEEP);
        run(&mut g, 3.5);
        assert_eq!(g.farm.clock.day, day + 1);

        // The horse waits by the house: a right-click rides, the bar's 5 steps down.
        let h = g.horse().unwrap();
        let at = g.layout.npcs[h].at;
        stand_at(&mut g, at);
        g.gossip_hello(npc_guid(h));
        assert!(g.mounted);
        g.command(".ride");
        assert!(!g.mounted);
        g.command(".ride");
        g.cancel_aura();
        assert!(!g.mounted, "clicking off the buff steps down too");

        // Felling a tree is an orange try: Lumberjacking goes up.
        assert_eq!(g.farm.skills.get(Skill::Lumberjacking).0, 1);
        let tree = g.layout.trees[0].1;
        stand_at(&mut g, tree);
        g.command(".chop");
        run(&mut g, 4.0);
        assert_eq!(g.farm.skills.get(Skill::Lumberjacking).0, 2);
        assert!(drain(&mut rx).iter().all(|_| true));
    }

    #[test]
    fn brog_only_shelves_seeds_the_seed_quest_can_still_be_finished_with() {
        let (mut g, mut rx) = new_game(false);
        g.skip_to_quest(4);
        let brog = g.villager_guid(farm_rules::Villager::Brog);
        let farley = g.villager_guid(farm_rules::Villager::Farley);
        g.quest_accept(farley, 904);
        g.farm.money = 200;
        let shelf = |g: &Game| g.wares().iter().map(|w| w.entry).collect::<Vec<_>>();
        let pumpkin = items::entry(Item::Seeds(Crop::Pumpkin));
        assert!(!shelf(&g).contains(&pumpkin), "10 pumpkins would cost 15s");
        assert!(shelf(&g).contains(&items::entry(Item::Seeds(Crop::Peacebloom))));
        g.buy(brog, items::entry(Item::Seeds(Crop::Peacebloom)), 6);
        // 110c left for 4 more: everything up to Earthroot's 30c still fits.
        g.buy(brog, pumpkin, 1);
        assert_eq!(g.farm.bag.count(Item::Seeds(Crop::Pumpkin)), 0);
        g.buy(brog, items::entry(Item::Seeds(Crop::Silverleaf)), 4);
        assert_eq!(g.farm.quests.count(4), 10);
        assert!(
            shelf(&g).contains(&pumpkin),
            "the whole shelf once it is done"
        );
        drain(&mut rx);
    }

    #[test]
    fn previews_run_to_day_ten_twenty_five_and_fifty_and_save_nothing() {
        let (mut g, mut rx) = new_game(false);
        drain(&mut rx);
        g.command(".preview");
        assert_eq!(g.farm.clock.day, 10);
        assert_eq!(g.farm.tiles.len(), g.layout.tiles.len() + 8);
        assert!(g.farm.fences.iter().any(|f| f.built) && g.farm.fences.iter().any(|f| !f.built));
        // Every packet of the rebuild reads back whole in the client.
        let shown = drain(&mut rx);
        assert!(shown.len() > 100);
        g.command(".preview");
        assert_eq!(g.farm.clock.day, 25);
        assert!(g.farm.fences.iter().all(|f| f.built));
        assert!(g.farm.has(Unlock::Herd));
        drain(&mut rx);
        g.command(".preview");
        assert_eq!(g.farm.clock.day, 50);
        assert_eq!(g.farm.tiles.len(), g.layout.field.len(), "the whole field");
        assert_eq!(g.herd.len(), 8 * 25, "twenty-five times the herd");
        drain(&mut rx);
        g.save();
        let kept = g.saves.farm(1).unwrap();
        assert_eq!(kept.clock.day, 1, "a preview is never saved");
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
