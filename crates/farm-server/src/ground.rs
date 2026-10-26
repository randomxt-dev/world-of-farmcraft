//! The ground under the farm: terrain heights from Azeroth's ADT tiles in the player's own
//! 1.12.1 data, so each plot tile sits on the grass where it is laid out.

use std::path::Path;

use anyhow::{Context, Result};
use benilla_formats::{load_tiles_around, terrain_height_at, Chain, ChunkMesh};

/// The terrain chunks around a point, read once.
pub struct Ground {
    chunks: Vec<ChunkMesh>,
}

impl Ground {
    /// Read the Eastern Kingdoms tiles within one tile (533 yd) of world `x, y` from `data`.
    pub fn open(data: &Path, x: f32, y: f32) -> Result<Self> {
        let mut chain = Chain::open(data)
            .with_context(|| format!("opening the game data in {}", data.display()))?;
        let tiles = load_tiles_around(&mut chain, "Azeroth", x, y, 1)?;
        Ok(Self {
            chunks: tiles.into_iter().flat_map(|(_, t)| t.chunks).collect(),
        })
    }

    /// The terrain height at `x, y`, if it is on the loaded tiles.
    pub fn height(&self, x: f32, y: f32) -> Option<f32> {
        terrain_height_at(&self.chunks, [x, y, 0.0])
    }
}
