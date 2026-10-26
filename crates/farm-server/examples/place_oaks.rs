//! Find spots for oaks around Stonefield Farm that touch nothing: open, gently sloped grass, off
//! the crop field, clear of every building, tree and doodad the map places and of everything the
//! farm itself puts down. Prints the spots as a Rust array for `layout.rs`.
//!
//! `cargo run -p farm-server --example place_oaks -- [count]` with `WOW_DATA` set.

use benilla_formats::{load_tiles_around, terrain_height_at, Chain, ChunkMesh};
use farm_server::ground::Ground;
use farm_server::Layout;

/// Where the oaks go: a ring around the farmhouse.
const CENTRE: [f32; 2] = [-9900.0, 330.0];
const RADIUS: f32 = 110.0;
/// Room each oak needs from other oaks, and from what is already there.
const OAK_SPACING: f32 = 9.0;
const CLEAR_DOODAD: f32 = 6.0;
const CLEAR_BUILDING: f32 = 20.0;
const CLEAR_FARM: f32 = 6.0;

/// How much of the crop texture is painted at `x, y` (0-255), from its chunk's alpha map.
fn crop_alpha(chunks: &[ChunkMesh], x: f32, y: f32) -> u8 {
    for c in chunks {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &c.positions {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        if !((lo[0]..=hi[0]).contains(&x) && (lo[1]..=hi[1]).contains(&y)) {
            continue;
        }
        let Some(layer) = c.layer_textures.iter().position(|t| t.contains("Crop")) else {
            return 0;
        };
        if layer == 0 {
            return 255;
        }
        let Some(alpha) = &c.alpha_map else {
            return 0;
        };
        let size = ((alpha.len() / 4) as f32).sqrt() as usize;
        let row = (((hi[0] - x) / (hi[0] - lo[0])) * size as f32).min(size as f32 - 1.0) as usize;
        let col = (((hi[1] - y) / (hi[1] - lo[1])) * size as f32).min(size as f32 - 1.0) as usize;
        return alpha[(row * size + col) * 4 + (layer - 1)];
    }
    0
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn main() -> anyhow::Result<()> {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(40);
    let data = benilla_formats::wow_data().expect("WOW_DATA or a WoW link");
    let mut chain = Chain::open(&data)?;
    let tiles = load_tiles_around(&mut chain, "Azeroth", CENTRE[0], CENTRE[1], 1)?;
    let chunks: Vec<_> = tiles.iter().flat_map(|(_, t)| t.chunks.clone()).collect();
    let mut blockers: Vec<([f32; 2], f32)> = Vec::new();
    for (_, t) in &tiles {
        for d in &t.doodads {
            blockers.push(([d.position[0], d.position[1]], CLEAR_DOODAD));
        }
        for w in &t.wmos {
            blockers.push(([w.position[0], w.position[1]], CLEAR_BUILDING));
        }
    }
    let ground = Ground::open(&data, -9915.0, 360.0)?;
    let layout = Layout::stonefield(Some(&ground));
    let mut farm: Vec<[f32; 2]> = Vec::new();
    farm.extend(layout.tiles.iter().map(|p| [p[0], p[1]]));
    farm.extend(layout.veins.iter().map(|(p, _)| [p[0], p[1]]));
    farm.extend(layout.wild.iter().map(|(_, p)| [p[0], p[1]]));
    farm.extend(layout.npcs.iter().map(|n| [n.at[0], n.at[1]]));
    farm.extend([layout.bed, layout.shipping, layout.home.position].map(|p| [p[0], p[1]]));
    for p in &farm {
        blockers.push((*p, CLEAR_FARM));
    }

    let height = |x: f32, y: f32| terrain_height_at(&chunks, [x, y, 0.0]);
    let mut candidates = Vec::new();
    let step = 3.0;
    let n = (RADIUS / step) as i32;
    for i in -n..=n {
        for j in -n..=n {
            let p = [CENTRE[0] + i as f32 * step, CENTRE[1] + j as f32 * step];
            if dist(p, CENTRE) > RADIUS || dist(p, CENTRE) < 25.0 {
                continue;
            }
            let Some(h) = height(p[0], p[1]) else {
                continue;
            };
            // Gentle ground only: no hillsides, no lake shore below the meadow.
            let around = [[2.0, 0.0], [-2.0, 0.0], [0.0, 2.0], [0.0, -2.0]];
            let steep = around
                .iter()
                .any(|o| height(p[0] + o[0], p[1] + o[1]).is_none_or(|g| (g - h).abs() > 0.9));
            if steep || h < 31.0 {
                continue;
            }
            if crop_alpha(&chunks, p[0], p[1]) > 40 {
                continue;
            }
            if blockers.iter().any(|(b, r)| dist(p, *b) < *r) {
                continue;
            }
            candidates.push((p, h));
        }
    }
    // Spread them out: farthest-point picks, so the oaks scatter around the whole property.
    let mut chosen: Vec<[f32; 2]> = Vec::new();
    while chosen.len() < count {
        let best = candidates
            .iter()
            .filter(|(p, _)| chosen.iter().all(|c| dist(*p, *c) >= OAK_SPACING))
            .max_by(|a, b| {
                let score = |p: [f32; 2]| {
                    chosen
                        .iter()
                        .map(|c| dist(p, *c))
                        .fold(dist(p, CENTRE) * 0.3, f32::min)
                };
                score(a.0).total_cmp(&score(b.0))
            });
        match best {
            Some((p, _)) => chosen.push(*p),
            None => break,
        }
    }
    eprintln!("{} candidates, {} oaks", candidates.len(), chosen.len());
    println!("const OAKS: [[f32; 2]; {}] = [", chosen.len());
    for p in &chosen {
        println!("    [{:.1}, {:.1}],", p[0], p[1]);
    }
    println!("];");
    Ok(())
}
