//! Where everything stands: Stonefield Farm's plot in its fenced field west of the farmhouse, the
//! hay bed and shipping crate beside it, copper veins down Fargodeep Mine, and the villagers of
//! the farm and Goldshire where Elwynn has always had them. World coordinates (x north, y west,
//! z up) on map 0, zone 12; the villagers' spots are vmangos's `creature` spawns.

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
/// Yards between tile centres.
const TILE_STEP: f32 = 3.0;
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
        let mut tiles = Vec::new();
        for r in 0..PLOT_ROWS {
            for c in 0..PLOT_COLUMNS {
                let x = PLOT_CENTRE[0] + (r as f32 - (PLOT_ROWS - 1) as f32 / 2.0) * TILE_STEP;
                let y = PLOT_CENTRE[1] + (c as f32 - (PLOT_COLUMNS - 1) as f32 / 2.0) * TILE_STEP;
                tiles.push(on_ground(x, y));
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
        assert!((sx / n - PLOT_CENTRE[0]).abs() < 0.01 && (sy / n - PLOT_CENTRE[1]).abs() < 0.01);
        assert_eq!(l.npcs.iter().filter(|n| n.role == Role::Store).count(), 1);
    }
}
