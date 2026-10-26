//! The save file every farmer lives in: read at start, written after each change that matters
//! (a new character, a night's sleep, logging out), atomically through a temporary file.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result};
use farm_rules::save::{Character, Farmer, Save};
use farm_rules::Farm;

/// `CHAR_CREATE_NAME_IN_USE`, `CHAR_CREATE_SERVER_LIMIT` and `CHAR_CREATE_ERROR`.
const NAME_IN_USE: u8 = benilla_protocol::messages::CHAR_CREATE_NAME_IN_USE;
const SERVER_LIMIT: u8 = benilla_protocol::messages::CHAR_CREATE_SERVER_LIMIT;
pub const CREATE_ERROR: u8 = 0x2F;
/// The 1.12 roster's ten rows.
const MAX_FARMERS: usize = 10;

struct Inner {
    save: Save,
    path: Option<PathBuf>,
    tiles: usize,
    veins: usize,
    wild: usize,
}

/// The shared save; cheap to clone.
#[derive(Clone)]
pub struct Saves(Arc<Mutex<Inner>>);

impl Saves {
    /// The save at `path`, or a new one if there is none yet; farms fit a layout of `tiles` plot
    /// tiles and `veins` ore veins. `None` keeps everything in memory.
    pub fn open(path: Option<PathBuf>, tiles: usize, veins: usize, wild: usize) -> Result<Self> {
        let mut save = match &path {
            Some(p) if p.exists() => {
                let text = std::fs::read_to_string(p)
                    .with_context(|| format!("reading the save {}", p.display()))?;
                Save::from_text(&text)
                    .map_err(|e| anyhow::anyhow!("the save {} is damaged: {e}", p.display()))?
            }
            _ => Save::default(),
        };
        for f in &mut save.farmers {
            f.farm.fit(tiles, veins, wild);
        }
        Ok(Self(Arc::new(Mutex::new(Inner {
            save,
            path,
            tiles,
            veins,
            wild,
        }))))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn roster(&self) -> Vec<Character> {
        (self.lock().save.farmers.iter())
            .map(|f| f.character.clone())
            .collect()
    }

    pub fn character(&self, guid: u64) -> Option<Character> {
        self.lock().save.farmer(guid).map(|f| f.character.clone())
    }

    /// Add a farmer with a new farm; `Err` is the `SMSG_CHAR_CREATE` code.
    pub fn create(&self, mut character: Character) -> Result<(), u8> {
        let mut inner = self.lock();
        let taken = (inner.save.farmers.iter())
            .any(|f| f.character.name.eq_ignore_ascii_case(&character.name));
        if taken {
            return Err(NAME_IN_USE);
        }
        if inner.save.farmers.len() >= MAX_FARMERS {
            return Err(SERVER_LIMIT);
        }
        character.guid = inner.save.next_guid();
        let seed = rand::random();
        let mut farm = Farm::new(inner.tiles, inner.veins, seed);
        farm.fit(inner.tiles, inner.veins, inner.wild);
        inner.save.farmers.push(Farmer { character, farm });
        Self::write(&inner);
        Ok(())
    }

    pub fn delete(&self, guid: u64) -> bool {
        let mut inner = self.lock();
        let before = inner.save.farmers.len();
        inner.save.farmers.retain(|f| f.character.guid != guid);
        let gone = inner.save.farmers.len() != before;
        if gone {
            Self::write(&inner);
        }
        gone
    }

    pub fn farm(&self, guid: u64) -> Option<Farm> {
        self.lock().save.farmer(guid).map(|f| f.farm.clone())
    }

    /// Keep `farm` as `guid`'s and write the file.
    pub fn store(&self, guid: u64, farm: &Farm) {
        let mut inner = self.lock();
        if let Some(f) = inner.save.farmer_mut(guid) {
            f.farm = farm.clone();
            Self::write(&inner);
        }
    }

    fn write(inner: &Inner) {
        let Some(path) = &inner.path else {
            return;
        };
        let result = (|| -> Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("ron.tmp");
            std::fs::write(&tmp, inner.save.to_text())?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if let Err(e) = result {
            tracing::warn!("farm-server: could not write {}: {e:#}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn farmer(name: &str) -> Character {
        Character {
            guid: 0,
            name: name.into(),
            race: 1,
            class: 1,
            gender: 0,
            skin: 0,
            face: 0,
            hair_style: 0,
            hair_color: 0,
            facial_hair: 0,
        }
    }

    #[test]
    fn farmers_are_made_named_once_and_written_to_disk() {
        let dir = std::env::temp_dir().join(format!("farm-saves-{}", std::process::id()));
        let path = dir.join("save.ron");
        let saves = Saves::open(Some(path.clone()), 16, 4, 2).unwrap();
        saves.create(farmer("Hoggle")).unwrap();
        assert_eq!(saves.create(farmer("hoggle")), Err(NAME_IN_USE));
        let guid = saves.roster()[0].guid;
        let mut farm = saves.farm(guid).unwrap();
        assert_eq!(farm.tiles.len(), 16);
        farm.money = 999;
        saves.store(guid, &farm);

        let again = Saves::open(Some(path), 20, 4, 2).unwrap();
        let farm = again.farm(guid).unwrap();
        assert_eq!(farm.money, 999);
        assert_eq!(farm.tiles.len(), 20, "fitted to the new layout");
        assert!(again.delete(guid));
        assert!(again.roster().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
