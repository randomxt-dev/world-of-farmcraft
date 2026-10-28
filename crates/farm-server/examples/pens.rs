//! Find open rectangles near the farm for pens: off the field, clear of
//! buildings, props, trees and villagers, on gentle ground.
//!
//! `cargo run -p farm-server --example pens -- <width> <height>` with `WOW_DATA` set.
use benilla_formats::{load_tiles_around, Chain};
use farm_server::{ground::Ground, Layout};

fn main() -> anyhow::Result<()> {
    let args: Vec<f32> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let (w, h) = (args[0], args[1]);
    let data = benilla_formats::wow_data().expect("WOW_DATA");
    let ground = Ground::open(&data, -9915.0, 360.0)?;
    let l = Layout::stonefield(Some(&ground));
    let mut chain = Chain::open(&data)?;
    let tiles = load_tiles_around(&mut chain, "Azeroth", -9900.0, 350.0, 1)?;
    let mut wmos = Vec::new();
    let mut props = Vec::new();
    for (_, t) in &tiles {
        wmos.extend(t.wmos.iter().map(|w| [w.position[0], w.position[1]]));
        for d in &t.doodads {
            let m = d.model.to_lowercase();
            if !(m.contains("grass")
                || m.contains("flower")
                || m.contains("butterfl")
                || m.contains("bush"))
            {
                props.push([d.position[0], d.position[1]]);
            }
        }
    }
    let mut found = Vec::new();
    let mut x0 = -10000.0;
    while x0 < -9790.0 {
        let mut y0 = 240.0;
        while y0 < 440.0 {
            let (x1, y1) = (x0 + w, y0 + h);
            let inside = |p: [f32; 2], m: f32| {
                p[0] > x0 - m && p[0] < x1 + m && p[1] > y0 - m && p[1] < y1 + m
            };
            let mut ok = wmos.iter().all(|&p| !inside(p, 18.0))
                && props.iter().all(|&p| !inside(p, 2.0))
                && l.trees.iter().all(|t| !inside([t.1[0], t.1[1]], 3.0))
                && l.npcs.iter().all(|n| !inside([n.at[0], n.at[1]], 3.0))
                && !inside([l.home.position[0], l.home.position[1]], 6.0);
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            let mut gx = x0;
            while ok && gx <= x1 {
                let mut gy = y0;
                while gy <= y1 {
                    match ground.height(gx, gy) {
                        Some(z) => {
                            lo = lo.min(z);
                            hi = hi.max(z);
                        }
                        None => ok = false,
                    }
                    if ground.crop(gx, gy) > 20 {
                        ok = false;
                    }
                    gy += 2.0;
                }
                gx += 2.0;
            }
            if ok && hi - lo < 4.0 {
                let d = (x0 + w / 2.0 + 9900.0).hypot(y0 + h / 2.0 - 350.0);
                found.push((d, x0, y0, hi - lo));
            }
            y0 += 2.0;
        }
        x0 += 2.0;
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    for f in found.iter().take(12) {
        println!(
            "corner {:.0},{:.0}  {:.0} yd from home  slope {:.1}",
            f.1, f.2, f.0, f.3
        );
    }
    Ok(())
}
