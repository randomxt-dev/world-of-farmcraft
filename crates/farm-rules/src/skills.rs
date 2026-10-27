//! The farmer's skills, raised as in the game: each gather or craft has a difficulty, and the
//! farmer's level against it gives the colour, and the colour the chance of a point. Orange always
//! teaches, yellow often, green rarely, gray never. Each skill starts at 1 of 75; the caps open a
//! rank (Journeyman, Expert, Artisan) through the study quests in Stormwind's Royal Library.
//! Every 25 levels the work gets quicker and luckier.

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
/// Levels per bonus step.
pub const TIER: u32 = 25;

/// The bonus steps a level has earned: one at 25, two at 50, and so on.
pub fn tier(level: u32) -> u32 {
    level / TIER
}

/// How long a skilled job takes, as a share of the unskilled time: 6% quicker each step, at most
/// half again as quick.
pub fn quickness(level: u32) -> f32 {
    1.0 - 0.06 * tier(level).min(8) as f32
}

/// The chance in percent of something extra (a second herb, ore or log, a fence that saves its
/// wood): 4% a step.
pub fn luck(level: u32) -> u32 {
    4 * tier(level)
}

/// The horse's pace as a share of its plain pace: 4% faster each step.
pub fn pace(level: u32) -> f32 {
    1.0 + 0.04 * tier(level) as f32
}

/// Axe swings to fell a tree: three, two from Lumberjacking 50, one from 150.
pub fn swings(level: u32) -> u32 {
    match tier(level) {
        0..2 => 3,
        2..6 => 2,
        _ => 1,
    }
}

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
        self.0.insert(s, (level, cap));
        Some(level)
    }

    /// Whether any skill has reached its cap.
    pub fn any_capped(&self, cap: u32) -> bool {
        Skill::ALL.iter().any(|&s| self.get(s) == (cap, cap))
    }

    /// Every skill's cap to `cap`: a rank studied.
    pub fn open_rank(&mut self, cap: u32) {
        for s in Skill::ALL {
            let (level, _) = self.get(s);
            self.0.insert(s, (level, cap));
        }
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
    fn gray_never_teaches_and_a_cap_holds_until_studied() {
        let mut s = Skills::default();
        s.0.insert(Skill::Mining, (200, 225));
        assert_eq!(s.practice(Skill::Mining, 1, 0), None, "gray");
        s.0.insert(Skill::Herbalism, (74, 75));
        assert_eq!(s.practice(Skill::Herbalism, 60, 0), Some(75));
        assert_eq!(s.practice(Skill::Herbalism, 60, 0), None, "at the cap");
        assert!(s.any_capped(75));
        s.open_rank(150);
        assert_eq!(s.get(Skill::Herbalism), (75, 150));
        assert_eq!(s.get(Skill::Riding), (1, 150));
    }

    #[test]
    fn every_25_levels_is_quicker_luckier_and_fewer_swings() {
        assert_eq!(quickness(24), 1.0);
        assert!((quickness(50) - 0.88).abs() < 1e-6);
        assert!((quickness(300) - 0.52).abs() < 1e-6);
        assert_eq!(luck(75), 12);
        assert_eq!((swings(1), swings(50), swings(150)), (3, 2, 1));
        assert!(pace(100) > pace(99) - 1e-6 && pace(100) > 1.15);
    }
}
