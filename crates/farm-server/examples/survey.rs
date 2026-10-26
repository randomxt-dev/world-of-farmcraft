//! Survey the ground around a point: a grid of terrain heights and the buildings and doodads
//! nearby, to pick a flat, open spot for the plot.
//!
//! `cargo run -p farm-server --example survey -- <x> <y> [radius]`

use benilla_formats::{load_tiles_around, terrain_height_at, Chain};

fn main() -> anyhow::Result<()> {
    let args: Vec<f32> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let (x, y, r) = (args[0], args[1], args.get(2).copied().unwrap_or(30.0));
    let data = benilla_formats::wow_data().expect("WOW_DATA or a WoW link");
    let mut chain = Chain::open(&data)?;
    let tiles = load_tiles_around(&mut chain, "Azeroth", x, y, 1)?;
    let chunks: Vec<_> = tiles.iter().flat_map(|(_, t)| t.chunks.clone()).collect();
    println!("heights every 5 yd (rows x, columns y), centre {x} {y}:");
    let step = 5.0;
    let n = (r / step) as i32;
    for i in -n..=n {
        let row: Vec<String> = (-n..=n)
            .map(|j| {
                terrain_height_at(&chunks, [x + i as f32 * step, y + j as f32 * step, 0.0])
                    .map_or("  ?  ".into(), |h| format!("{h:5.1}"))
            })
            .collect();
        println!("{:8.1} {}", x + i as f32 * step, row.join(" "));
    }
    for c in &chunks {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &c.positions {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        if (lo[0]..=hi[0]).contains(&x) && (lo[1]..=hi[1]).contains(&y) {
            println!(
                "chunk x {:.1}..{:.1} y {:.1}..{:.1}: base {:?} layers {:?}",
                lo[0], hi[0], lo[1], hi[1], c.base_texture, c.layer_textures
            );
            for i in [0, 1, 9, 17] {
                println!("vertex {i}: {:?} uv {:?}", c.positions[i], c.uvs[i]);
            }
        }
    }
    let near = |p: [f32; 3]| ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt() < r * 1.5;
    for (_, t) in &tiles {
        for w in t.wmos.iter().filter(|w| near(w.position)) {
            println!("wmo {:?} {}", w.position, w.model);
        }
        for d in t.doodads.iter().filter(|d| near(d.position)) {
            println!("doodad {:?} {}", d.position, d.model);
        }
    }
    Ok(())
}
