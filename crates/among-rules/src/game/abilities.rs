//! Class abilities on players: validation, effects and their timers, freezing traps, the
//! shield that stops a kill, and stealth breaking when the rogue acts. Effects last only while
//! play runs: a meeting, a death or the end of the game clears them.

use std::time::Duration;

use super::{Game, Phase};
use crate::abilities::{Ability, Aim, Effect};
use crate::protocol::{Event, PlayerId, Reject};
use crate::settings::{within, Position};

/// An effect on a player: how long it has left, and who put it there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Under {
    pub remaining: Duration,
    pub by: PlayerId,
}

/// A freezing trap waiting at `at` for anyone but its owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trap {
    pub owner: PlayerId,
    pub at: Position,
    pub remaining: Duration,
}

impl Game {
    /// Freezing traps waiting to be sprung.
    pub fn traps(&self) -> &[Trap] {
        &self.traps
    }

    /// The living, connected player `id`, if they may act: not polymorphed, feared or frozen.
    pub(super) fn able(&self, id: PlayerId) -> Result<&super::Player, Reject> {
        let p = self.alive(id)?;
        if p.is_incapacitated() {
            return Err(Reject::Incapacitated);
        }
        Ok(p)
    }

    pub(super) fn validate_ability(
        &self,
        player: PlayerId,
        target: Option<PlayerId>,
    ) -> Result<(), Reject> {
        self.playing()?;
        let p = self.able(player)?;
        if !p.ability_cooldown.is_zero() {
            return Err(Reject::OnCooldown);
        }
        let ability = Ability::of(p.class);
        if ability.aim() == Aim::Myself {
            return Ok(());
        }
        let target = target
            .filter(|&t| t != player)
            .ok_or(Reject::InvalidTarget)?;
        let t = (self.players.get(&target))
            .filter(|t| t.connected && t.alive && !t.is_hidden())
            .ok_or(Reject::InvalidTarget)?;
        if t.is_under(Effect::DivineShield) {
            return Err(Reject::Immune);
        }
        let range = self.settings.abilities.get(ability).range;
        self.in_reach(p.position, t.position, range)
    }

    pub(super) fn use_ability(&mut self, player: PlayerId, target: Option<PlayerId>) {
        let class = self.players[&player].class;
        let ability = Ability::of(class);
        let def = *self.settings.abilities.get(ability);
        let target = match ability.aim() {
            Aim::Myself => None,
            Aim::Other => target,
        };
        self.seat(player).ability_cooldown = def.cooldown;
        self.out.push(Event::AbilityUsed {
            player,
            ability,
            target,
        });
        if ability != Ability::Stealth {
            self.break_stealth(player);
        }
        match ability.effect() {
            Some(effect) => self.put_under(target.unwrap_or(player), effect, def.duration, player),
            None => self.set_trap(player),
        }
    }

    /// `player` is under `effect` for `duration`, or again for the full time if already.
    fn put_under(&mut self, player: PlayerId, effect: Effect, duration: Duration, by: PlayerId) {
        self.seat(player).effects.insert(
            effect,
            Under {
                remaining: duration,
                by,
            },
        );
        self.out.push(Event::EffectStarted {
            player,
            effect,
            duration,
            by,
        });
        if effect.incapacitates() {
            // Helpless: every cast, swing and held station is dropped.
            self.interrupt(player);
            self.stop_attack(player);
            self.interrupt_spell(player);
            self.release_stations(player);
        }
        if effect == Effect::Stealth {
            // Out of sight is out of every fight.
            self.leave_combat(player);
        }
    }

    pub(super) fn end_effect(&mut self, player: PlayerId, effect: Effect) {
        if (self.players.get_mut(&player)).is_some_and(|p| p.effects.remove(&effect).is_some()) {
            self.out.push(Event::EffectEnded { player, effect });
        }
    }

    /// The rogue acted (a kill, a fight, a task, a fix, an ability): seen again.
    pub(super) fn break_stealth(&mut self, player: PlayerId) {
        self.end_effect(player, Effect::Stealth);
    }

    /// Everything on `player` ends, and their trap crumbles: a death, or leaving.
    pub(super) fn clear_effects_of(&mut self, player: PlayerId) {
        let effects: Vec<Effect> = (self.players.get(&player))
            .map(|p| p.effects.keys().copied().collect())
            .unwrap_or_default();
        for effect in effects {
            self.end_effect(player, effect);
        }
        self.remove_trap(player);
    }

    /// Play stops: every effect ends and every trap crumbles.
    pub(super) fn clear_all_effects(&mut self) {
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            self.clear_effects_of(id);
        }
    }

    fn set_trap(&mut self, owner: PlayerId) {
        self.remove_trap(owner);
        let at = self.players[&owner].position;
        self.traps.push(Trap {
            owner,
            at,
            remaining: self.settings.trap_lifetime,
        });
        self.out.push(Event::TrapSet { owner, at });
    }

    fn remove_trap(&mut self, owner: PlayerId) {
        let before = self.traps.len();
        self.traps.retain(|t| t.owner != owner);
        if self.traps.len() < before {
            self.out.push(Event::TrapGone { owner });
        }
    }

    /// `player` moved: the first trap of another's they now stand on freezes them.
    pub(super) fn spring_traps(&mut self, player: PlayerId) {
        if self.phase != Phase::Playing {
            return;
        }
        let Some(p) = self.players.get(&player).filter(|p| p.connected && p.alive) else {
            return;
        };
        let radius = self.settings.trap_radius;
        let Some(i) =
            (self.traps.iter()).position(|t| t.owner != player && within(t.at, p.position, radius))
        else {
            return;
        };
        let owner = self.traps.remove(i).owner;
        self.out.push(Event::TrapSprung {
            owner,
            victim: player,
        });
        if self.players[&player].is_under(Effect::DivineShield) {
            return;
        }
        let duration = self.settings.abilities.get(Ability::FreezingTrap).duration;
        self.break_stealth(player);
        self.put_under(player, Effect::Frozen, duration, owner);
    }

    /// A kill on a shielded victim: the shield breaks instead, and the kill is spent. True if it
    /// did.
    pub(super) fn shield_stops(&mut self, killer: PlayerId, victim: PlayerId) -> bool {
        let Some(shield) = self.players[&victim].effects.get(&Effect::Shield).copied() else {
            return false;
        };
        self.end_effect(victim, Effect::Shield);
        self.seat(killer).kill_cooldown = self.settings.kill_cooldown;
        self.out.push(Event::KillBlocked {
            killer,
            victim,
            shielder: shield.by,
        });
        true
    }

    /// Run cooldowns, effects and traps down by `dt`.
    pub(super) fn tick_abilities(&mut self, dt: Duration) {
        let mut ended = Vec::new();
        for (&id, p) in self.players.iter_mut() {
            p.ability_cooldown = p.ability_cooldown.saturating_sub(dt);
            for (&effect, under) in p.effects.iter_mut() {
                if dt >= under.remaining {
                    ended.push((id, effect));
                } else {
                    under.remaining -= dt;
                }
            }
        }
        for (player, effect) in ended {
            self.end_effect(player, effect);
        }
        let mut crumbled = Vec::new();
        for t in &mut self.traps {
            if dt >= t.remaining {
                crumbled.push(t.owner);
            } else {
                t.remaining -= dt;
            }
        }
        for owner in crumbled {
            self.remove_trap(owner);
        }
    }
}
