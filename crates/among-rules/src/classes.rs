//! The nine 1.12 classes and what each fights with: health, an auto-attack, and exactly one class
//! spell. Numbers are game tuning for monster fights at the task spots, not WoW's level curves.

use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    Warrior,
    Paladin,
    Hunter,
    Rogue,
    Priest,
    Shaman,
    Mage,
    Warlock,
    Druid,
}

impl Class {
    pub const ALL: [Class; 9] = [
        Class::Warrior,
        Class::Paladin,
        Class::Hunter,
        Class::Rogue,
        Class::Priest,
        Class::Shaman,
        Class::Mage,
        Class::Warlock,
        Class::Druid,
    ];

    /// The class from its `ChrClasses.dbc` id (the character's `class` byte); 6 and 10 are unused
    /// in 1.12.
    pub fn from_wow(id: u8) -> Option<Self> {
        Some(match id {
            1 => Class::Warrior,
            2 => Class::Paladin,
            3 => Class::Hunter,
            4 => Class::Rogue,
            5 => Class::Priest,
            7 => Class::Shaman,
            8 => Class::Mage,
            9 => Class::Warlock,
            11 => Class::Druid,
            _ => return None,
        })
    }

    /// The class's `ChrClasses.dbc` id, as [`Class::from_wow`] reads it.
    pub fn wow_id(self) -> u8 {
        match self {
            Class::Warrior => 1,
            Class::Paladin => 2,
            Class::Hunter => 3,
            Class::Rogue => 4,
            Class::Priest => 5,
            Class::Shaman => 7,
            Class::Mage => 8,
            Class::Warlock => 9,
            Class::Druid => 11,
        }
    }

    /// The class after this one in [`Class::ALL`], wrapping: each round's class for a player.
    pub fn next(self) -> Self {
        Class::ALL[(self.index() + 1) % Class::ALL.len()]
    }

    fn index(self) -> usize {
        Class::ALL.iter().position(|&c| c == self).expect("in ALL")
    }
}

/// How a class fights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kit {
    pub health: u32,
    /// Auto-attack damage per swing, landing within melee range.
    pub melee: u32,
    pub swing: Duration,
    pub spell: ClassSpell,
}

/// The one class spell: damage on landing, a cast (zero is instant), a cooldown and a range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassSpell {
    pub damage: u32,
    pub cast: Duration,
    pub cooldown: Duration,
    pub range: f32,
}

/// A kit per class, indexed by [`Class`].
#[derive(Clone, Debug, PartialEq)]
pub struct Kits([Kit; 9]);

impl Kits {
    pub fn get(&self, class: Class) -> &Kit {
        &self.0[class.index()]
    }

    pub fn set(&mut self, class: Class, kit: Kit) {
        self.0[class.index()] = kit;
    }
}

impl Default for Kits {
    /// Each class's starter spell: Heroic Strike, Judgement, Raptor Strike, Sinister Strike,
    /// Smite, Lightning Bolt, Fireball, Shadow Bolt, Wrath. Cast times, ranges and the Raptor
    /// Strike and Judgement cooldowns are this install's Spell.dbc values; health, damage and the
    /// melee strikes' cooldowns (theirs are rage/energy-gated) are tuning.
    fn default() -> Self {
        let secs = Duration::from_secs_f32;
        let kit = |health, melee, swing, damage, cast, cooldown, range| Kit {
            health,
            melee,
            swing: secs(swing),
            spell: ClassSpell {
                damage,
                cast: secs(cast),
                cooldown: secs(cooldown),
                range,
            },
        };
        // Casters' cooldown is the 1.5 s global cooldown the client enforces after each cast.
        Kits([
            kit(120, 9, 2.0, 15, 0.0, 3.0, 5.0), // Warrior: Heroic Strike 78
            kit(110, 8, 2.0, 18, 0.0, 10.0, 10.0), // Paladin: Judgement 20271
            kit(100, 7, 2.0, 16, 0.0, 6.0, 5.0), // Hunter: Raptor Strike 2973
            kit(100, 8, 1.6, 12, 0.0, 3.0, 5.0), // Rogue: Sinister Strike 1752
            kit(80, 4, 2.0, 16, 1.5, 1.5, 30.0), // Priest: Smite 585
            kit(100, 7, 2.0, 15, 1.5, 1.5, 30.0), // Shaman: Lightning Bolt 403
            kit(75, 4, 2.0, 18, 1.5, 1.5, 35.0), // Mage: Fireball 133
            kit(80, 4, 2.0, 18, 1.7, 1.5, 30.0), // Warlock: Shadow Bolt 686
            kit(90, 6, 2.0, 15, 1.5, 1.5, 30.0), // Druid: Wrath 5176
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wow_class_ids_map_both_ways_and_skip_the_gaps() {
        let ids: Vec<u8> = (0..=12).filter(|&i| Class::from_wow(i).is_some()).collect();
        assert_eq!(ids, [1, 2, 3, 4, 5, 7, 8, 9, 11]);
        let classes: Vec<Class> = ids.iter().map(|&i| Class::from_wow(i).unwrap()).collect();
        assert_eq!(classes, Class::ALL);
        let back: Vec<u8> = classes.iter().map(|c| c.wow_id()).collect();
        assert_eq!(back, ids);
    }

    #[test]
    fn next_walks_every_class_once_and_wraps() {
        let mut seen = vec![Class::Druid.next()];
        while seen.len() < Class::ALL.len() {
            seen.push(seen.last().unwrap().next());
        }
        assert_eq!(seen, Class::ALL);
        assert_eq!(Class::Druid.next(), Class::Warrior);
    }

    #[test]
    fn every_class_has_a_kit_it_can_win_with() {
        let kits = Kits::default();
        for class in Class::ALL {
            let k = kits.get(class);
            assert!(
                k.health > 0 && k.melee > 0 && k.spell.damage > 0,
                "{class:?}"
            );
        }
    }
}
