//! Dump a texture from the game data as raw RGBA, for looking at.
//!
//! `cargo run -p farm-server --example texdump -- <texture path> <out.rgba>`

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = benilla_formats::wow_data().expect("WOW_DATA or a WoW link");
    let mut chain = benilla_formats::Chain::open(&data)?;
    let (w, h, rgba) = benilla_formats::read_texture_rgba(&mut chain, &args[0])?;
    std::fs::write(&args[1], &rgba)?;
    println!("{w} {h}");
    Ok(())
}
