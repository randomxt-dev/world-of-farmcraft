//! Add a human farmer to a save without the create screen, for scripted runs.
//!
//! `cargo run -p farm-server --example new_farmer -- <save.ron> <Name> [demo]`: with `demo`, the
//! plot is planted with every crop at every stage, to look at.

use farm_rules::save::Character;
use farm_rules::Crop;
use farm_server::{Layout, Saves};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, name) = (args[0].clone(), args[1].clone());
    let layout = Layout::stonefield(None);
    let saves = Saves::open(Some(path.into()), layout.tiles.len(), layout.veins.len())?;
    let farmer = Character {
        guid: 0,
        name,
        race: 1,
        class: 1,
        gender: 0,
        skin: 2,
        face: 3,
        hair_style: 4,
        hair_color: 2,
        facial_hair: 1,
    };
    match saves.create(farmer) {
        Ok(()) => println!("created"),
        Err(code) => println!("refused: {code:#x}"),
    }
    if args.get(2).map(String::as_str) == Some("demo") {
        let guid = saves.roster().last().map(|c| c.guid).unwrap_or(1);
        let mut farm = saves.farm(guid).expect("the new farmer");
        for (i, tile) in farm.tiles.iter_mut().enumerate() {
            let crop = Crop::ALL[i % Crop::ALL.len()];
            let age = (i / Crop::ALL.len()) as u32 * crop.def().days / 2;
            tile.crop = Some((crop, age.min(crop.def().days)));
            tile.watered = i % 2 == 0;
        }
        saves.store(guid, &farm);
        println!("planted");
    }
    Ok(())
}
