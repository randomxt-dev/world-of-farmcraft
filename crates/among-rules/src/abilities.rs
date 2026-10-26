//! Each class's one ability on a player: an effect on another player or on oneself, with a
//! cooldown, usable by crewmates and impostors alike while play runs. Ranges match the borrowed
//! 1.12 spell's `SpellRange.dbc` reach, so the client and the rules agree on what is in range.

use std::time::Duration;

use crate::classes::Class;

/// A class's ability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ability {
    /// Warrior: the target runs at half speed.
    Hamstring,
    /// Paladin: no kill lands on oneself, and no ability.
    DivineShield,
    /// Hunter: a trap at one's feet that only its owner sees; the first other living player to
    /// step on it is frozen.
    FreezingTrap,
    /// Rogue: unseen by the living until it runs out or the rogue acts.
    Stealth,
    /// Priest: on another player, the next kill on them is stopped.
    PowerWordShield,
    /// Shaman: oneself runs faster, in wolf form.
    GhostWolf,
    /// Mage: the target is a sheep that cannot act.
    Polymorph,
    /// Warlock: the target loses control and cannot act.
    Fear,
    /// Druid: the target cannot move, but may still act.
    EntanglingRoots,
}

/// Who an ability lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    /// The user; any target is ignored.
    Myself,
    /// Another living, visible player in range; never the user.
    Other,
}

impl Ability {
    pub const ALL: [Ability; 9] = [
        Ability::Hamstring,
        Ability::DivineShield,
        Ability::FreezingTrap,
        Ability::Stealth,
        Ability::PowerWordShield,
        Ability::GhostWolf,
        Ability::Polymorph,
        Ability::Fear,
        Ability::EntanglingRoots,
    ];

    /// The class's ability.
    pub fn of(class: Class) -> Self {
        match class {
            Class::Warrior => Ability::Hamstring,
            Class::Paladin => Ability::DivineShield,
            Class::Hunter => Ability::FreezingTrap,
            Class::Rogue => Ability::Stealth,
            Class::Priest => Ability::PowerWordShield,
            Class::Shaman => Ability::GhostWolf,
            Class::Mage => Ability::Polymorph,
            Class::Warlock => Ability::Fear,
            Class::Druid => Ability::EntanglingRoots,
        }
    }

    pub fn aim(self) -> Aim {
        match self {
            Ability::DivineShield
            | Ability::FreezingTrap
            | Ability::Stealth
            | Ability::GhostWolf => Aim::Myself,
            _ => Aim::Other,
        }
    }

    /// The effect the ability puts on its target at once; a trap's comes when it springs.
    pub fn effect(self) -> Option<Effect> {
        Some(match self {
            Ability::Hamstring => Effect::Slow,
            Ability::DivineShield => Effect::DivineShield,
            Ability::FreezingTrap => return None,
            Ability::Stealth => Effect::Stealth,
            Ability::PowerWordShield => Effect::Shield,
            Ability::GhostWolf => Effect::GhostWolf,
            Ability::Polymorph => Effect::Polymorph,
            Ability::Fear => Effect::Fear,
            Ability::EntanglingRoots => Effect::Rooted,
        })
    }

    fn index(self) -> usize {
        Ability::ALL
            .iter()
            .position(|&a| a == self)
            .expect("in ALL")
    }
}

/// What a player is under, for a while.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    Stealth,
    /// Stops the next kill on its bearer.
    Shield,
    GhostWolf,
    /// Immune to kills and abilities.
    DivineShield,
    Polymorph,
    Fear,
    Slow,
    /// Caught in a freezing trap.
    Frozen,
    Rooted,
}

impl Effect {
    /// Whether its bearer can do nothing: no kill, report, task, fix, sabotage, fight or ability.
    pub fn incapacitates(self) -> bool {
        matches!(self, Effect::Polymorph | Effect::Fear | Effect::Frozen)
    }

    /// Whether its bearer cannot move.
    pub fn roots(self) -> bool {
        matches!(self, Effect::Fear | Effect::Frozen | Effect::Rooted)
    }

    /// Its bearer's run speed, as a share of their normal speed.
    pub fn speed(self) -> f32 {
        match self {
            Effect::Slow | Effect::Polymorph => 0.5,
            Effect::GhostWolf => 1.4,
            _ => 1.0,
        }
    }

    /// Whether another player put it on its bearer against their will; a divine shield keeps it
    /// off.
    pub fn harmful(self) -> bool {
        matches!(
            self,
            Effect::Polymorph | Effect::Fear | Effect::Slow | Effect::Frozen | Effect::Rooted
        )
    }
}

/// One ability's numbers: how long its effect lasts, how long until it may be used again, and
/// how far it reaches (unused for an ability on oneself).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbilityDef {
    pub duration: Duration,
    pub cooldown: Duration,
    pub range: f32,
}

/// An [`AbilityDef`] per ability, indexed by [`Ability`].
#[derive(Clone, Debug, PartialEq)]
pub struct Abilities([AbilityDef; 9]);

impl Abilities {
    pub fn get(&self, ability: Ability) -> &AbilityDef {
        &self.0[ability.index()]
    }

    pub fn set(&mut self, ability: Ability, def: AbilityDef) {
        self.0[ability.index()] = def;
    }
}

impl Default for Abilities {
    /// Durations are the user's (stealth 30 s at most, polymorph and fear 10 s) or the spell's
    /// own where it is short; cooldowns are tuning; ranges are each spell's `SpellRange.dbc` reach.
    /// A freezing trap's duration is how long its catch stays frozen.
    fn default() -> Self {
        let def = |duration, cooldown, range| AbilityDef {
            duration: Duration::from_secs(duration),
            cooldown: Duration::from_secs(cooldown),
            range,
        };
        Abilities([
            def(10, 30, 5.0),  // Hamstring 1715: melee
            def(10, 60, 0.0),  // Divine Shield 642
            def(10, 45, 0.0),  // Freezing Trap 1499
            def(30, 45, 0.0),  // Stealth 1784
            def(30, 45, 40.0), // Power Word: Shield 17
            def(10, 30, 0.0),  // Ghost Wolf 2645
            def(10, 45, 30.0), // Polymorph 118
            def(10, 45, 20.0), // Fear 5782
            def(10, 45, 30.0), // Entangling Roots 339
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_has_its_own_ability() {
        let mut seen: Vec<Ability> = Class::ALL.iter().map(|&c| Ability::of(c)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 9);
    }

    #[test]
    fn an_ability_on_another_has_reach_and_every_one_has_a_cooldown() {
        let defs = Abilities::default();
        for ability in Ability::ALL {
            let d = defs.get(ability);
            assert!(
                !d.cooldown.is_zero() && !d.duration.is_zero(),
                "{ability:?}"
            );
            if ability.aim() == Aim::Other {
                assert!(d.range > 0.0, "{ability:?}");
            }
        }
    }

    #[test]
    fn stealth_lasts_at_most_thirty_seconds_and_crowd_control_ten() {
        let defs = Abilities::default();
        assert!(defs.get(Ability::Stealth).duration <= Duration::from_secs(30));
        assert_eq!(
            defs.get(Ability::Polymorph).duration,
            Duration::from_secs(10)
        );
        assert_eq!(defs.get(Ability::Fear).duration, Duration::from_secs(10));
    }
}
