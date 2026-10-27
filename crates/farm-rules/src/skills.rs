//! The farmer's skills, raised as in the game: each gather or craft has a difficulty, and the
//! farmer's level against it gives the colour, and the colour the chance of a point. Orange always
//! teaches, yellow often, green rarely, gray never. Each skill starts at 1 of 75 and its cap goes up
//! a rank (Journeyman, Expert, Artisan) as the farmer nears it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Skill {
    Herbalism,
    Mining,
    Lumberjacking,
    Woodworking,
    Shepherding,
    /// Planting, watering and harvesting the farmer's own crops.
    Farming,
    Riding,
}

impl Skill {
    pub const ALL: [Skill; 7] = [
        Skill::Farming,
        Skill::Herbalism,
        Skill::Mining,
        Skill::Lumberjacking,
        Skill::Woodworking,
        Skill::Shepherding,
        Skill::Riding,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Skill::Herbalism => "Herbalism",
            Skill::Mining => "Mining",
            Skill::Lumberjacking => "Lumberjacking",
            Skill::Woodworking => "Woodworking",
            Skill::Shepherding => "Shepherding",
            Skill::Farming => "Farming",
            Skill::Riding => "Riding",
        }
    }
}

/// The caps: Apprentice, Journeyman, Expert, Artisan.
pub const RANKS: [u32; 4] = [75, 150, 225, 300];
/// How close to its cap a skill gets before the next rank opens.
const RANK_UP_AT: u32 = 25;

/// A difficulty's colour against a level, as the game reckons a gathering node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    Orange,
    Yellow,
    Green,
    Gray,
}

pub fn colour(level: u32, difficulty: u32) -> Colour {
    match level.saturating_sub(difficulty) {
        0..25 => Colour::Orange,
        25..50 => Colour::Yellow,
        50..100 => Colour::Green,
        _ => Colour::Gray,
    }
}

/// The chance in percent that a try of this colour teaches a point.
fn chance(c: Colour) -> u32 {
    match c {
        Colour::Orange => 100,
        Colour::Yellow => 60,
        Colour::Green => 25,
        Colour::Gray => 0,
    }
}

/// Each skill's level and cap.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Skills(pub BTreeMap<Skill, (u32, u32)>);

impl Default for Skills {
    fn default() -> Self {
        Skills(Skill::ALL.iter().map(|&s| (s, (1, RANKS[0]))).collect())
    }
}

impl Skills {
    /// The level and cap of `s`.
    pub fn get(&self, s: Skill) -> (u32, u32) {
        self.0.get(&s).copied().unwrap_or((1, RANKS[0]))
    }

    /// One try at something of `difficulty`, `roll` in 0..100: the new level if it went up.
    pub fn practice(&mut self, s: Skill, difficulty: u32, roll: u32) -> Option<u32> {
        let (level, cap) = self.get(s);
        if level >= cap || roll >= chance(colour(level, difficulty)) {
            return None;
        }
        let level = level + 1;
        let cap = match RANKS.iter().position(|&r| r == cap) {
            Some(i) if i + 1 < RANKS.len() && level + RANK_UP_AT >= cap => RANKS[i + 1],
            _ => cap,
        };
        self.0.insert(s, (level, cap));
        Some(level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_run_orange_to_gray_as_the_farmer_outgrows_a_task() {
        assert_eq!(colour(1, 1), Colour::Orange);
        assert_eq!(colour(10, 50), Colour::Orange);
        assert_eq!(colour(30, 1), Colour::Yellow);
        assert_eq!(colour(60, 1), Colour::Green);
        assert_eq!(colour(101, 1), Colour::Gray);
    }

    #[test]
    fn a_skill_starts_at_one_and_orange_always_teaches() {
        let mut s = Skills::default();
        assert_eq!(s.get(Skill::Lumberjacking), (1, 75));
        assert_eq!(s.practice(Skill::Lumberjacking, 1, 99), Some(2));
        assert_eq!(s.get(Skill::Lumberjacking), (2, 75));
        assert_eq!(s.get(Skill::Mining), (1, 75), "only the one practised");
    }

    #[test]
    fn gray_never_teaches_and_the_cap_rises_by_rank() {
        let mut s = Skills::default();
        s.0.insert(Skill::Mining, (200, 225));
        assert_eq!(s.practice(Skill::Mining, 1, 0), None, "gray");
        s.0.insert(Skill::Herbalism, (49, 75));
        assert_eq!(s.practice(Skill::Herbalism, 50, 0), Some(50));
        assert_eq!(s.get(Skill::Herbalism), (50, 150), "Journeyman opens");
        s.0.insert(Skill::Shepherding, (75, 75));
        assert_eq!(s.practice(Skill::Shepherding, 75, 0), None, "at the cap");
    }
}
