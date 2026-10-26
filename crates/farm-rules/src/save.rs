//! The save file: every farmer's character and farm, as RON text.

use serde::{Deserialize, Serialize};

use crate::farm::Farm;

/// A character as the roster shows it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Character {
    pub guid: u64,
    pub name: String,
    pub race: u8,
    pub class: u8,
    pub gender: u8,
    pub skin: u8,
    pub face: u8,
    pub hair_style: u8,
    pub hair_color: u8,
    pub facial_hair: u8,
}

/// One farmer: who they are and their farm.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Farmer {
    pub character: Character,
    pub farm: Farm,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Save {
    pub farmers: Vec<Farmer>,
}

impl Save {
    pub fn from_text(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_text(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a save serialises")
    }

    pub fn farmer(&self, guid: u64) -> Option<&Farmer> {
        self.farmers.iter().find(|f| f.character.guid == guid)
    }

    pub fn farmer_mut(&mut self, guid: u64) -> Option<&mut Farmer> {
        self.farmers.iter_mut().find(|f| f.character.guid == guid)
    }

    /// A guid no farmer has.
    pub fn next_guid(&self) -> u64 {
        self.farmers
            .iter()
            .map(|f| f.character.guid)
            .max()
            .unwrap_or(0)
            + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crops::Crop;

    #[test]
    fn a_save_round_trips_through_text() {
        let mut farm = Farm::new(16, 3, 9);
        farm.plant(2, Crop::Peacebloom).unwrap();
        farm.money = 1234;
        let save = Save {
            farmers: vec![Farmer {
                character: Character {
                    guid: 1,
                    name: "Farmer".into(),
                    race: 1,
                    class: 1,
                    gender: 0,
                    skin: 1,
                    face: 2,
                    hair_style: 3,
                    hair_color: 4,
                    facial_hair: 5,
                },
                farm,
            }],
        };
        let text = save.to_text();
        assert_eq!(Save::from_text(&text).unwrap(), save);
        assert_eq!(save.next_guid(), 2);
    }
}
