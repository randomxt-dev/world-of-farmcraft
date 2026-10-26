//! Add a human farmer to a save without the create screen, for scripted runs.
//!
//! `cargo run -p farm-server --example new_farmer -- <save.ron> <Name>`

use farm_rules::save::Character;
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
    Ok(())
}
