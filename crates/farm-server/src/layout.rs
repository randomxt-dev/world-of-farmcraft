//! Where everything stands: Stonefield Farm's plot in its fenced field west of the farmhouse, the
//! hay bed and shipping crate beside it, copper veins down Fargodeep Mine, and the villagers of
//! the farm and Goldshire where Elwynn has always had them. World coordinates (x north, y west,
//! z up) on map 0, zone 12; the villagers' spots are vmangos's `creature` spawns.

use farm_rules::Crop;

use crate::ground::Ground;
use crate::world::creatures::{Kind, Look, Stand, NPC_GOSSIP, NPC_VENDOR};
use crate::world::packets::Location;

/// Eastern Kingdoms.
pub const MAP: u32 = 0;
/// Elwynn Forest.
pub const ZONE: u32 = 12;

/// The plot's centre: Stonefield's field, flat to within a yard.
const PLOT_CENTRE: [f32; 2] = [-9915.0, 360.0];
/// Rows and columns of plot tiles.
pub const PLOT_ROWS: usize = 4;
pub const PLOT_COLUMNS: usize = 4;
/// Yards between tiles along a furrow.
const TILE_STEP: f32 = 2.4;
/// The field's furrows: its ground texture (`Tileset/Elwynn/ElwynnCrop92.blp`) repeats every
/// chunk/8 = 4.1667 yd, and its leafy rows run at 45 degrees, along world (1, -1), on the lines
/// where `x + y` is `ROW_PHASE` past a whole number of repeats. Measured from the texture's leaf
/// pattern and checked from a top-down camera in game: each plot row sits on a row of leaves.
const TEXTURE_REPEAT: f32 = 100.0 / 3.0 / 8.0;
const ROW_PHASE: f32 = 0.52 * TEXTURE_REPEAT;
/// The furrows' heading, along (1, -1): the soil mounds lie lengthwise on it.
pub const FURROW_FACING: f32 = -std::f32::consts::FRAC_PI_4;
/// The field's height where the ground is not read.
const FIELD_HEIGHT: f32 = 35.0;

/// Copper veins along Fargodeep Mine's lower tunnels (vmangos `gameobject` 1731 spawns).
const VEINS: [[f32; 4]; 8] = [
    [-9802.81, 88.5173, 4.70444, -2.74017],
    [-9802.83, 128.656, 7.04257, 2.93215],
    [-9816.0, 148.449, 5.88154, -0.610865],
    [-9840.77, 121.837, 6.28642, 2.82743],
    [-9799.79, 107.987, 6.13216, 2.53073],
    [-9781.55, 92.5236, 6.73226, 3.47321],
    [-9862.2, 155.543, 7.98152, -0.593412],
    [-9828.76, 184.747, 13.2738, 2.30383],
];

/// Wild herbs in the meadows around the farm (vmangos Elwynn herb spawns): (crop, x, y).
const WILD: [(Crop, f32, f32); 16] = [
    (Crop::Peacebloom, -9966.7, 299.4),
    (Crop::Silverleaf, -9978.5, 348.8),
    (Crop::Silverleaf, -9791.8, 321.6),
    (Crop::Silverleaf, -9785.3, 225.8),
    (Crop::Peacebloom, -9965.3, 188.5),
    (Crop::Silverleaf, -10009.4, 465.4),
    (Crop::Silverleaf, -9888.6, 133.3),
    (Crop::Silverleaf, -9663.1, 318.4),
    (Crop::Peacebloom, -10033.5, 133.6),
    (Crop::Peacebloom, -9710.9, 151.0),
    (Crop::Silverleaf, -9895.9, 600.0),
    (Crop::Silverleaf, -9880.6, 51.4),
    (Crop::Silverleaf, -9778.5, 63.9),
    (Crop::Silverleaf, -9973.5, 614.7),
    (Crop::Peacebloom, -9898.1, 635.5),
    (Crop::Peacebloom, -9827.1, 627.0),
];

/// Every companion and farm animal 1.12 has a model for, at its own size (`CreatureDisplayInfo`
/// scale): (name, display, scale). The first is the farm dog every farmer starts with.
pub const PETS: [(&str, u32, f32); 59] = [
    ("Biscuit the Wolf Pup", 447, 0.45),
    ("Worg Pup", 9563, 0.4),
    ("Timber Wolf", 604, 0.55),
    ("Coyote", 643, 0.85),
    ("Young Forest Bear", 1006, 0.85),
    ("Bombay Cat", 5556, 0.5),
    ("Cornish Rex Cat", 5586, 0.5),
    ("Orange Tabby Cat", 5554, 0.5),
    ("Silver Tabby Cat", 5555, 0.5),
    ("White Kitten", 9989, 0.35),
    ("Siamese Cat", 5585, 0.5),
    ("Black Tabby Cat", 5448, 0.5),
    ("Green Wing Macaw", 5207, 0.25),
    ("Hyacinth Macaw", 6192, 0.25),
    ("Senegal", 6190, 0.25),
    ("Cockatiel", 6191, 0.25),
    ("Great Horned Owl", 4615, 0.25),
    ("Hawk Owl", 6299, 0.25),
    ("Ancona Chicken", 5369, 1.0),
    ("Westfall Chicken", 304, 1.0),
    ("Mechanical Chicken", 7920, 1.0),
    ("Emerald Whelpling", 6291, 0.5),
    ("Crimson Whelpling", 6290, 0.5),
    ("Dark Whelpling", 6288, 0.5),
    ("Sprite Darter", 6294, 0.3),
    ("Tiny Red Dragon", 14779, 0.4),
    ("Tiny Green Dragon", 14778, 0.4),
    ("Black Kingsnake", 1206, 1.0),
    ("Brown Snake", 2957, 1.0),
    ("Crimson Snake", 6303, 1.0),
    ("Snowshoe Rabbit", 328, 1.0),
    ("Prairie Dog", 1072, 1.0),
    ("Squirrel", 134, 1.3),
    ("Mechanical Squirrel", 7937, 1.3),
    ("Cockroach", 2177, 1.0),
    ("Tree Frog", 6295, 1.0),
    ("Wood Frog", 901, 1.0),
    ("Jubling", 14938, 1.5),
    ("Pet Bombling", 8909, 0.15),
    ("Lil' Smoky", 8910, 0.15),
    ("Smolderweb Hatchling", 9997, 0.25),
    ("Zergling", 10993, 0.25),
    ("Panda Cub", 10990, 0.25),
    ("Mini Diablo", 10992, 0.25),
    ("Murky", 15369, 0.65),
    ("Snapjaw Turtle", 14657, 0.25),
    ("Mechanical Yeti", 10269, 0.25),
    ("Baby Shark", 15595, 0.2),
    ("Fawn", 654, 0.6),
    ("Piglet", 10015, 0.3),
    ("Pig", 10015, 0.4),
    ("Lamb", 856, 0.6),
    ("Sheep", 856, 1.0),
    ("Black Sheep", 857, 1.0),
    ("Calf", 1060, 0.6),
    ("Cow", 1060, 1.0),
    ("Kodo Calf", 1451, 0.3),
    ("Ram", 10000, 1.0),
    ("Pony", 229, 0.6),
];

/// The pet pen's spots, on open grass south of the field: a 3 yd grid kept 4.5 yd clear of every
/// tree, bush, fence and building the ADT places there.
const PEN: [[f32; 2]; 58] = [
    [-9938.0, 292.0],
    [-9935.0, 292.0],
    [-9932.0, 292.0],
    [-9941.0, 295.5],
    [-9938.0, 295.5],
    [-9935.0, 295.5],
    [-9932.0, 295.5],
    [-9920.0, 295.5],
    [-9944.0, 299.0],
    [-9941.0, 299.0],
    [-9938.0, 299.0],
    [-9923.0, 299.0],
    [-9920.0, 299.0],
    [-9917.0, 299.0],
    [-9944.0, 302.5],
    [-9941.0, 302.5],
    [-9938.0, 302.5],
    [-9920.0, 302.5],
    [-9917.0, 302.5],
    [-9944.0, 306.0],
    [-9941.0, 306.0],
    [-9938.0, 306.0],
    [-9935.0, 306.0],
    [-9932.0, 306.0],
    [-9920.0, 306.0],
    [-9917.0, 306.0],
    [-9914.0, 306.0],
    [-9944.0, 309.5],
    [-9941.0, 309.5],
    [-9938.0, 309.5],
    [-9935.0, 309.5],
    [-9932.0, 309.5],
    [-9929.0, 309.5],
    [-9926.0, 309.5],
    [-9923.0, 309.5],
    [-9920.0, 309.5],
    [-9917.0, 309.5],
    [-9941.0, 313.0],
    [-9938.0, 313.0],
    [-9935.0, 313.0],
    [-9932.0, 313.0],
    [-9929.0, 313.0],
    [-9926.0, 313.0],
    [-9923.0, 313.0],
    [-9920.0, 313.0],
    [-9917.0, 313.0],
    [-9941.0, 316.5],
    [-9938.0, 316.5],
    [-9935.0, 316.5],
    [-9932.0, 316.5],
    [-9929.0, 316.5],
    [-9926.0, 316.5],
    [-9923.0, 316.5],
    [-9920.0, 316.5],
    [-9935.0, 320.0],
    [-9932.0, 320.0],
    [-9929.0, 320.0],
    [-9926.0, 320.0],
];

/// The grove west of the pen: 7 yd apart, 6 yd clear of the world's own trees.
const GROVE: [[f32; 2]; 23] = [
    [-9975.0, 299.0],
    [-9975.0, 306.0],
    [-9982.0, 299.0],
    [-9975.0, 292.0],
    [-9982.0, 306.0],
    [-9968.0, 306.0],
    [-9982.0, 292.0],
    [-9968.0, 292.0],
    [-9975.0, 313.0],
    [-9989.0, 299.0],
    [-9961.0, 299.0],
    [-9982.0, 313.0],
    [-9968.0, 313.0],
    [-9975.0, 285.0],
    [-9989.0, 306.0],
    [-9961.0, 306.0],
    [-9989.0, 292.0],
    [-9961.0, 292.0],
    [-9982.0, 285.0],
    [-9968.0, 285.0],
    [-9989.0, 313.0],
    [-9961.0, 313.0],
    [-9989.0, 285.0],
];

/// Every tree 1.12 has a gameobject look for (`GameObjectDisplayInfo.dbc`) that draws well:
/// (name, display, scale). The Burning Steppes tree is left out: its smoke draws black. The grove
/// plants them in turn.
pub const TREES: [(&str, u32, f32); 15] = [
    ("Westfall Oak", 5975, 0.36),
    ("Arathi Pine", 967, 0.36),
    ("Ironforge Pine", 927, 0.36),
    ("Deadwind Elm", 2474, 0.3),
    ("Plaguelands Mushroom Tree", 1847, 0.6),
    ("Maraudon Spore Tree", 3813, 0.3),
    ("Maraudon Spewer Tree", 3812, 0.3),
    ("Maraudon Spawner Tree", 3912, 0.3),
    ("Corrupted Spewer Tree", 5318, 0.3),
    ("Coral Tree", 887, 0.4),
    ("Winter Veil Tree", 5251, 0.7),
    ("Great Alliance Winter Tree", 5253, 0.45),
    ("Alliance Winter Tree", 5315, 0.6),
    ("Snowy Winter Tree", 5316, 0.45),
    ("Horde Winter Tree", 5371, 0.6),
];

/// A villager or animal: its look and where it stands.
#[derive(Clone, Debug)]
pub struct Npc {
    pub look: Look,
    pub at: [f32; 3],
    pub facing: f32,
    /// What a right-click on it does.
    pub role: Role,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Scenery: no menu.
    None,
    /// Sells seeds and buys the harvest.
    Store,
    /// Talks about the farm.
    Advice,
    /// One of the pet pen's animals: a right-click offers it as the farmer's companion.
    Pet(usize),
}

#[derive(Clone, Debug)]
pub struct Layout {
    /// Where a farmer starts each morning: by the farmhouse, facing the field.
    pub home: Location,
    /// Each plot tile's centre on the ground, row by row.
    pub tiles: Vec<[f32; 3]>,
    pub veins: Vec<([f32; 3], f32)>,
    /// The hay pile a farmer sleeps in.
    pub bed: [f32; 3],
    /// The crate that buys the whole harvest at once.
    pub shipping: [f32; 3],
    pub npcs: Vec<Npc>,
    /// Wild herbs to pick around the farm: what they are and where.
    pub wild: Vec<(Crop, [f32; 3])>,
    /// The grove: which of [`TREES`] stands where. `FARMCRAFT_TREE_LINE=1` lines them up in order
    /// instead, south-west of the pen, to look at.
    pub trees: Vec<(usize, [f32; 3])>,
}

fn npc(name: &str, display: u32, kind: Kind, at: [f32; 4]) -> Npc {
    Npc {
        look: Look::new(name, display, kind),
        at: [at[0], at[1], at[2]],
        facing: at[3],
        role: Role::None,
    }
}

impl Layout {
    /// The layout with heights read from `ground` where it covers the farm.
    pub fn stonefield(ground: Option<&Ground>) -> Self {
        let height = |x: f32, y: f32| ground.and_then(|g| g.height(x, y)).unwrap_or(FIELD_HEIGHT);
        let on_ground = |x: f32, y: f32| [x, y, height(x, y)];
        // Rows on the field's own furrows nearest the plot centre, tiles stepping along each.
        let centre_sum = PLOT_CENTRE[0] + PLOT_CENTRE[1];
        let first =
            ((centre_sum - ROW_PHASE) / TEXTURE_REPEAT).round() - (PLOT_ROWS as f32 - 1.0) / 2.0;
        let first = first.round();
        let along = [FURROW_FACING.cos(), FURROW_FACING.sin()];
        let mut tiles = Vec::new();
        for r in 0..PLOT_ROWS {
            let row_sum = ROW_PHASE + TEXTURE_REPEAT * (first + r as f32);
            // The point of this furrow nearest the plot centre.
            let shift = (row_sum - centre_sum) / 2.0;
            let mid = [PLOT_CENTRE[0] + shift, PLOT_CENTRE[1] + shift];
            for c in 0..PLOT_COLUMNS {
                let t = (c as f32 - (PLOT_COLUMNS - 1) as f32 / 2.0) * TILE_STEP;
                tiles.push(on_ground(mid[0] + t * along[0], mid[1] + t * along[1]));
            }
        }
        let home = on_ground(-9902.0, 349.0);

        let mut npcs = vec![
            npc(
                "Ma Stonefield",
                3330,
                Kind::Humanoid,
                [-9895.0, 332.424, 36.5505, 2.11185],
            ),
            npc(
                "Gramma Stonefield",
                2959,
                Kind::Humanoid,
                [-9880.63, 322.619, 37.8244, std::f32::consts::PI],
            ),
            npc(
                "\"Auntie\" Bernice Stonefield",
                3329,
                Kind::Humanoid,
                [-9889.68, 338.467, 36.6486, 2.75762],
            ),
            npc(
                "Homer Stonefield",
                3332,
                Kind::Humanoid,
                [-9858.46, 365.972, 36.3371, 1.81514],
            ),
            npc(
                "Brog Hamfist",
                1292,
                Kind::Humanoid,
                [-9465.29, 9.63314, 57.1459, 1.44862],
            ),
            npc(
                "Innkeeper Farley",
                1291,
                Kind::Humanoid,
                [-9462.66, 16.1915, 57.0459, 3.03687],
            ),
            npc(
                "Barkeep Dobbins",
                3266,
                Kind::Humanoid,
                [-9459.98, 8.41135, 57.1459, 1.46608],
            ),
            npc(
                "Marshal Dughan",
                1985,
                Kind::Humanoid,
                [-9465.52, 74.0069, 56.7785, 4.59022],
            ),
            npc(
                "Stormwind Guard",
                3258,
                Kind::Humanoid,
                [-9481.31, 74.4224, 56.5493, 3.06882],
            ),
            npc(
                "Stormwind Guard",
                3258,
                Kind::Humanoid,
                [-9455.43, 73.7274, 56.9503, std::f32::consts::PI],
            ),
            npc(
                "Smith Argus",
                1288,
                Kind::Humanoid,
                [-9456.65, 87.8553, 58.4271, 2.40855],
            ),
            npc(
                "Corina Steele",
                1287,
                Kind::Humanoid,
                [-9464.87, 93.9896, 58.5271, 0.0],
            ),
            npc(
                "William Pestle",
                5038,
                Kind::Humanoid,
                [-9460.3, 31.9389, 57.0494, 2.98451],
            ),
            npc(
                "Remy \"Two Times\"",
                3254,
                Kind::Humanoid,
                [-9496.32, 72.8264, 56.598, 6.23083],
            ),
            npc(
                "Toddrick",
                3236,
                Kind::Humanoid,
                [-9466.42, 0.061252, 57.0335, 6.12611],
            ),
            npc(
                "Chicken",
                304,
                Kind::Critter,
                [-9472.19, 50.399, 57.3361, 2.92712],
            ),
            npc(
                "Chicken",
                304,
                Kind::Critter,
                [-9465.21, 50.8185, 57.0477, 0.621393],
            ),
        ];
        // The farm's own animals, in the yard between the farmhouse and the barn.
        for (name, display, x, y, facing) in [
            ("Cow", 1060, -9838.0, 380.0, 4.0),
            ("Cow", 1060, -9842.0, 388.0, 2.2),
            ("Sheep", 856, -9870.0, 395.0, 1.0),
            ("Chicken", 304, -9893.0, 322.0, 0.5),
            ("Chicken", 304, -9896.0, 324.0, 2.5),
            ("Pig", 10015, -9872.0, 380.0, 5.0),
        ] {
            let mut n = npc(name, display, Kind::Critter, [x, y, height(x, y), facing]);
            n.look.level = 1;
            n.look.scale = PETS.iter().find(|p| p.0 == name).map_or(1.0, |p| p.2);
            npcs.push(n);
        }
        // The pet pen: every companion in rows on the grass south of the field, facing the house.
        for (i, &(name, display, scale)) in PETS.iter().enumerate().skip(1) {
            let [x, y] = PEN[(i - 1) % PEN.len()];
            let mut n = npc(name, display, Kind::Critter, [x, y, height(x, y), 0.0]);
            n.look.level = 1;
            n.look.scale = scale;
            n.look.subname = "Pet Pen".into();
            n.look.npc_flags = NPC_GOSSIP;
            n.role = Role::Pet(i);
            npcs.push(n);
        }
        let ma = &mut npcs[0];
        ma.look.npc_flags = NPC_GOSSIP;
        ma.role = Role::Advice;
        let brog = &mut npcs[4];
        brog.look.subname = "General Supplies".into();
        brog.look.npc_flags = NPC_VENDOR;
        brog.role = Role::Store;
        npcs[1].look.stand = Stand::Sit;

        Self {
            home: Location {
                map: MAP,
                zone: ZONE,
                position: home,
                orientation: 2.49,
            },
            tiles,
            veins: VEINS.iter().map(|v| ([v[0], v[1], v[2]], v[3])).collect(),
            bed: on_ground(-9902.0, 318.0),
            shipping: on_ground(-9904.0, 345.0),
            npcs,
            wild: WILD
                .iter()
                .map(|&(crop, x, y)| (crop, on_ground(x, y)))
                .collect(),
            trees: if std::env::var_os("FARMCRAFT_TREE_LINE").is_some() {
                (0..TREES.len())
                    .map(|i| (i, on_ground(-9990.0, 270.0 + 5.0 * i as f32)))
                    .collect()
            } else {
                (GROVE.iter().enumerate())
                    .map(|(i, &[x, y])| (i % TREES.len(), on_ground(x, y)))
                    .collect()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plot_is_a_grid_of_tiles_around_the_field_centre() {
        let l = Layout::stonefield(None);
        assert_eq!(l.tiles.len(), PLOT_ROWS * PLOT_COLUMNS);
        let (sx, sy) = l
            .tiles
            .iter()
            .fold((0.0, 0.0), |(x, y), t| (x + t[0], y + t[1]));
        let n = l.tiles.len() as f32;
        assert!((sx / n - PLOT_CENTRE[0]).abs() < 3.0 && (sy / n - PLOT_CENTRE[1]).abs() < 3.0);
        for t in &l.tiles {
            let phase = (t[0] + t[1] - ROW_PHASE).rem_euclid(TEXTURE_REPEAT);
            assert!(
                phase < 0.01 || phase > TEXTURE_REPEAT - 0.01,
                "on a furrow: {phase}"
            );
        }
        assert_eq!(l.npcs.iter().filter(|n| n.role == Role::Store).count(), 1);
        assert!(PEN.len() >= PETS.len() - 1, "a spot for every pet");
        assert!(GROVE.len() >= TREES.len(), "every tree in the grove");
    }
}
