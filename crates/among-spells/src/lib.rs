//! Among Azeroth's buttons as existing 1.12 spells on WoW's own action bar, and each class's one
//! combat spell. The server deals them and the client names them, so both read them from here.
//! Using a task, fixing a sabotage and calling a meeting are right-clicks on the world's own
//! objects (task nodes, stations, the emergency button), so they have no button.

use among_rules::Class;

/// Melee auto-attack.
pub const ATTACK: u32 = 6603;
/// NPC Eviscerate: the Kill button (`Ability_Rogue_Eviscerate`).
pub const KILL: u32 = 15692;
/// "Salt Flats Racer Normal" (`INV_Misc_Gear_01`): the cast bar of a task step with no node look.
pub const USE: u32 = 6602;
/// Raise Dead's icon (`Spell_Shadow_RaiseDead`): the Report button.
pub const REPORT: u32 = 6468;
/// Self Destruct: the Sabotage button.
pub const SABOTAGE: u32 = 9874;
/// Negative Charge: skip the vote.
pub const SKIP_VOTE: u32 = 28084;

/// The class's one combat spell: its level-1 starter.
pub fn class_spell(class: Class) -> u32 {
    match class {
        Class::Warrior => 78,    // Heroic Strike
        Class::Paladin => 20271, // Judgement
        Class::Hunter => 2973,   // Raptor Strike
        Class::Rogue => 1752,    // Sinister Strike
        Class::Priest => 585,    // Smite
        Class::Shaman => 403,    // Lightning Bolt
        Class::Mage => 133,      // Fireball
        Class::Warlock => 686,   // Shadow Bolt
        Class::Druid => 5176,    // Wrath
    }
}

/// The class spell's school, for the damage log: physical strikes, holy, nature, fire, shadow.
pub fn class_school(class: Class) -> u8 {
    match class {
        Class::Warrior | Class::Hunter | Class::Rogue => 0,
        Class::Paladin | Class::Priest => 1,
        Class::Mage => 2,
        Class::Shaman | Class::Druid => 3,
        Class::Warlock => 5,
    }
}

/// Slots on the main bar.
pub const BAR_SLOTS: usize = 6;

/// The main bar, slots 0-5 (keys 1-6): fight, the buttons everyone has, then the impostor's two;
/// 0 is an empty slot, so a crewmate's bar shows no Kill or Sabotage.
pub fn main_bar(class: Class, impostor: bool) -> [u32; BAR_SLOTS] {
    let (kill, sabotage) = if impostor { (KILL, SABOTAGE) } else { (0, 0) };
    [
        ATTACK,
        class_spell(class),
        REPORT,
        SKIP_VOTE,
        kill,
        sabotage,
    ]
}
