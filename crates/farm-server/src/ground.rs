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

    /// How much of the crop field's furrow texture is painted at `x, y`, 0-255: the chunk's alpha
    /// map for its `Crop` layer.
    pub fn crop(&self, x: f32, y: f32) -> u8 {
        for c in &self.chunks {
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
            let cell = |v: f32| ((v * size as f32) as usize).min(size - 1);
            let row = cell((hi[0] - x) / (hi[0] - lo[0]));
            let col = cell((hi[1] - y) / (hi[1] - lo[1]));
            return alpha[(row * size + col) * 4 + (layer - 1)];
        }
        0
    }

    /// The terrain height at `x, y`, if it is on the loaded tiles.
    pub fn height(&self, x: f32, y: f32) -> Option<f32> {
        terrain_height_at(&self.chunks, [x, y, 0.0])
    }
}
