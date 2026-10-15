//! Among Azeroth's buttons as existing 1.12 spells on WoW's own action bar, and each class's one
//! combat spell. The server deals them and the client names them, so both read them from here.
//! Using a task, fixing a sabotage and calling a meeting are right-clicks on the world's own
//! objects (task nodes, stations, the emergency button), so they have no button.

use among_rules::{Ability, Class, Effect};

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
/// "Tag: Arrow" (Hunter's Mark's crosshair, `Ability_Hunter_SniperShot`): vote for the target.
/// Instant and ungated, and its implicit target 25 takes any unit, so the client sends the
/// selected player.
pub const VOTE: u32 = 20374;
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

/// The class's ability on the bar. Each is the client-sendable 1.12 row with the real spell's icon:
/// instant, free, no stance or form, so the press reaches the server (Spell.dbc, 5875). Power Word:
/// Shield has no row a client sends at another player, so its button is "Uber Heal Over Time"
/// (`Spell_Holy_HolyProtection`, any unit) and the world shows 17.
pub fn ability_spell(ability: Ability) -> u32 {
    match ability {
        Ability::Hamstring => 26141,   // Hamstring (NPC): no stance, no weapon
        Ability::DivineShield => 642,  // Divine Shield, self
        Ability::FreezingTrap => 1499, // Freezing Trap, self
        Ability::Stealth => 1784,      // Stealth, self
        Ability::PowerWordShield => 1908, // Uber Heal Over Time, any unit
        Ability::GhostWolf => 22660,   // Dire Wolf Form: Ghost Wolf's icon, no shapeshift
        Ability::Polymorph => 29848,   // Polymorph, instant
        Ability::Fear => 29168,        // Fear, instant
        Ability::EntanglingRoots => 24648, // Entangling Roots, instant
    }
}

/// The class's ability button.
pub fn class_ability(class: Class) -> u32 {
    ability_spell(Ability::of(class))
}

/// Power Word: Shield, the spell the world shows the priest casting and the shielded wearing.
pub const POWER_WORD_SHIELD: u32 = 17;

/// The aura an effect shows on its bearer: the ability's own spell, but the real Power Word:
/// Shield, and Freezing Trap Effect on a trap's catch.
pub fn effect_aura(effect: Effect) -> u32 {
    match effect {
        Effect::Stealth => ability_spell(Ability::Stealth),
        Effect::Shield => POWER_WORD_SHIELD,
        Effect::GhostWolf => ability_spell(Ability::GhostWolf),
        Effect::DivineShield => ability_spell(Ability::DivineShield),
        Effect::Polymorph => ability_spell(Ability::Polymorph),
        Effect::Fear => ability_spell(Ability::Fear),
        Effect::Slow => ability_spell(Ability::Hamstring),
        Effect::Frozen => FREEZING_TRAP_EFFECT,
        Effect::Rooted => ability_spell(Ability::EntanglingRoots),
    }
}

/// Freezing Trap Effect: the ice a trap's catch stands in.
pub const FREEZING_TRAP_EFFECT: u32 = 3355;

/// Slots on the main bar.
pub const BAR_SLOTS: usize = 8;

/// The main bar, slots 0-7 (keys 1-8): fight, the buttons everyone has, the impostor's two, then
/// the class's ability; 0 is an empty slot, so a crewmate's bar shows no Kill or Sabotage and
/// every key means the same for both roles.
pub fn main_bar(class: Class, impostor: bool) -> [u32; BAR_SLOTS] {
    let (kill, sabotage) = if impostor { (KILL, SABOTAGE) } else { (0, 0) };
    [
        ATTACK,
        class_spell(class),
        REPORT,
        VOTE,
        SKIP_VOTE,
        kill,
        sabotage,
        class_ability(class),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ability_is_its_own_button_and_none_shadows_another() {
        let mut ids: Vec<u32> = Ability::ALL.into_iter().map(ability_spell).collect();
        let fixed = [ATTACK, KILL, USE, REPORT, SABOTAGE, VOTE, SKIP_VOTE];
        ids.extend(fixed);
        ids.extend(Class::ALL.map(class_spell));
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn the_ability_is_key_8_for_both_roles() {
        for class in Class::ALL {
            for impostor in [false, true] {
                assert_eq!(main_bar(class, impostor)[7], class_ability(class));
            }
        }
    }
}
