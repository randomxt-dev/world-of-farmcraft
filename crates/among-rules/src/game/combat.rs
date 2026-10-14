//! The fights at task spots. Creatures turn on the nearest living player in aggro range, chase
//! and swing, give up when dragged past their leash, and respawn after dying. Players auto-attack
//! and cast their class spell. A creature can kill a player for real: a ghost and a reportable
//! body, like an impostor's kill. Kills count toward Slay steps for whoever tapped the creature.

use std::time::Duration;

use super::{Body, Game, Phase};
use crate::protocol::{CreatureId, Event, PlayerId, Reject};
use crate::settings::{distance_squared, within, Position, StepAction};

/// A creature as it stands now.
#[derive(Clone, Debug, PartialEq)]
pub struct Creature {
    pub position: Position,
    pub health: u32,
    pub alive: bool,
    /// The player it is fighting.
    pub target: Option<PlayerId>,
    /// Who damaged it first: its death counts for them (WoW's tap).
    pub tapped_by: Option<PlayerId>,
    /// Until its next swing may land.
    pub swing: Duration,
    /// Until it respawns, while dead.
    pub respawn_in: Duration,
}

/// A class spell cast under way; it breaks if the caster moves off `from`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpellCast {
    pub creature: CreatureId,
    pub remaining: Duration,
    pub from: Position,
}

/// A chasing creature stops this far inside melee range, so a step back does not drop the fight.
const CHASE_MARGIN: f32 = 0.8;

impl Game {
    /// Every creature at its spawn, full health.
    pub(super) fn spawn_creatures(&mut self) {
        self.creatures = (self.layout.creatures.iter())
            .map(|d| Creature {
                position: d.at,
                health: d.health,
                alive: true,
                target: None,
                tapped_by: None,
                swing: Duration::ZERO,
                respawn_in: Duration::ZERO,
            })
            .collect();
        for (i, d) in self.layout.creatures.iter().enumerate() {
            self.out.push(Event::CreatureSpawned {
                creature: CreatureId(i),
                at: d.at,
            });
        }
    }

    /// Auto-attack may start out of reach: swings land once in melee range.
    pub(super) fn validate_attack(
        &self,
        player: PlayerId,
        creature: CreatureId,
    ) -> Result<(), Reject> {
        self.playing()?;
        self.alive(player)?;
        self.living_creature(creature).map(drop)
    }

    pub(super) fn validate_spell(
        &self,
        player: PlayerId,
        creature: CreatureId,
    ) -> Result<(), Reject> {
        self.playing()?;
        let p = self.alive(player)?;
        if p.casting.is_some() || p.spell_cast.is_some() {
            return Err(Reject::AlreadyCasting);
        }
        if !p.spell_cooldown.is_zero() {
            return Err(Reject::OnCooldown);
        }
        let c = self.living_creature(creature)?;
        let range = self.settings.kits.get(p.class).spell.range;
        self.in_reach(p.position, c.position, range)
    }

    fn living_creature(&self, id: CreatureId) -> Result<&Creature, Reject> {
        (self.creatures.get(id.0))
            .filter(|c| c.alive)
            .ok_or(Reject::InvalidTarget)
    }

    pub(super) fn attack(&mut self, player: PlayerId, creature: CreatureId) {
        let p = self.seat(player);
        if p.attacking == Some(creature) {
            return;
        }
        p.attacking = Some(creature);
        p.swing = Duration::ZERO;
        self.out.push(Event::AttackStarted { player, creature });
        self.engage(creature, player);
    }

    pub(super) fn stop_attack(&mut self, player: PlayerId) {
        if self.seat(player).attacking.take().is_some() {
            self.out.push(Event::AttackStopped { player });
        }
    }

    pub(super) fn cast_spell(&mut self, player: PlayerId, creature: CreatureId) {
        let p = &self.players[&player];
        let spell = self.settings.kits.get(p.class).spell;
        if spell.cast.is_zero() {
            self.land_spell(player, creature);
            return;
        }
        let from = p.position;
        self.seat(player).spell_cast = Some(SpellCast {
            creature,
            remaining: spell.cast,
            from,
        });
        self.out.push(Event::SpellCastStarted {
            player,
            creature,
            time: spell.cast,
        });
    }

    pub(super) fn interrupt_spell(&mut self, player: PlayerId) {
        if self.seat(player).spell_cast.take().is_some() {
            self.out.push(Event::SpellCastInterrupted { player });
        }
    }

    /// `player` is out of every fight (dead or gone); creatures fighting them give up.
    pub(super) fn leave_combat(&mut self, player: PlayerId) {
        self.stop_attack(player);
        self.interrupt_spell(player);
        let hunters: Vec<usize> = (0..self.creatures.len())
            .filter(|&i| self.creatures[i].alive && self.creatures[i].target == Some(player))
            .collect();
        for i in hunters {
            self.evade(i);
        }
    }

    /// Play stops: every fight ends and every disturbed creature goes home.
    pub(super) fn halt_combat(&mut self) {
        let fighters: Vec<PlayerId> = (self.players.iter())
            .filter(|(_, p)| p.attacking.is_some() || p.spell_cast.is_some())
            .map(|(&id, _)| id)
            .collect();
        for player in fighters {
            self.stop_attack(player);
            self.interrupt_spell(player);
        }
        let disturbed: Vec<usize> = (0..self.creatures.len())
            .filter(|&i| {
                let (c, d) = (&self.creatures[i], &self.layout.creatures[i]);
                c.alive && (c.target.is_some() || c.position != d.at || c.health < d.health)
            })
            .collect();
        for i in disturbed {
            self.evade(i);
        }
    }

    /// Back home at full health, untapped and without a target.
    fn evade(&mut self, i: usize) {
        let d = &self.layout.creatures[i];
        let (at, health) = (d.at, d.health);
        let c = &mut self.creatures[i];
        c.target = None;
        c.tapped_by = None;
        c.health = health;
        c.position = at;
        c.swing = Duration::ZERO;
        self.out.push(Event::CreatureEvaded {
            creature: CreatureId(i),
            at,
        });
    }

    /// A creature without a target turns on whoever engages it.
    fn engage(&mut self, creature: CreatureId, player: PlayerId) {
        let c = &mut self.creatures[creature.0];
        if c.target.is_none() {
            c.target = Some(player);
            self.out.push(Event::CreatureAggro { creature, player });
        }
    }

    fn land_spell(&mut self, player: PlayerId, creature: CreatureId) {
        let spell = self.settings.kits.get(self.players[&player].class).spell;
        self.seat(player).spell_cooldown = spell.cooldown;
        self.damage_creature(player, creature, spell.damage, true);
    }

    fn damage_creature(
        &mut self,
        player: PlayerId,
        creature: CreatureId,
        damage: u32,
        spell: bool,
    ) {
        let c = &mut self.creatures[creature.0];
        c.health = c.health.saturating_sub(damage);
        c.tapped_by.get_or_insert(player);
        let health = c.health;
        self.out.push(if spell {
            Event::SpellHit {
                player,
                creature,
                damage,
                health,
            }
        } else {
            Event::MeleeHit {
                player,
                creature,
                damage,
                health,
            }
        });
        if health == 0 {
            self.creature_dies(creature);
        } else {
            self.engage(creature, player);
        }
    }

    fn creature_dies(&mut self, creature: CreatureId) {
        let respawn = self.layout.creatures[creature.0].respawn;
        let c = &mut self.creatures[creature.0];
        c.alive = false;
        c.target = None;
        c.respawn_in = respawn;
        let credit =
            (c.tapped_by.take()).filter(|p| self.players.get(p).is_some_and(|p| p.connected));
        self.out.push(Event::CreatureDied { creature, credit });
        let engaged: Vec<PlayerId> = (self.players.iter())
            .filter(|(_, p)| {
                p.attacking == Some(creature)
                    || p.spell_cast.is_some_and(|s| s.creature == creature)
            })
            .map(|(&id, _)| id)
            .collect();
        for player in engaged {
            self.stop_attack(player);
            self.interrupt_spell(player);
        }
        if let Some(player) = credit {
            self.credit_slay(player, creature);
        }
    }

    /// One kill toward `player`'s current Slay step that counts `creature`; impostors only pretend.
    fn credit_slay(&mut self, player: PlayerId, creature: CreatureId) {
        let p = &self.players[&player];
        if !p.is_crewmate() {
            return;
        }
        let layout = &self.layout;
        let found = p.tasks.iter().position(|t| {
            let step = layout.tasks[t.task.0].steps.get(t.step);
            matches!(step.map(|s| &s.action), Some(StepAction::Slay { creatures, .. }) if creatures.contains(&creature))
        });
        let Some(i) = found else { return };
        let task = p.tasks[i].task;
        let StepAction::Slay { count, .. } = layout.tasks[task.0].steps[p.tasks[i].step].action
        else {
            return;
        };
        let t = &mut self.seat(player).tasks[i];
        t.kills += 1;
        let kills = t.kills;
        self.out.push(Event::SlayProgress {
            player,
            task,
            kills,
            count,
        });
        if kills >= count {
            self.advance(player, task);
        }
    }

    /// Players' casts, cooldowns and swings, then every creature, then out-of-combat regen.
    pub(super) fn tick_combat(&mut self, dt: Duration) {
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            if self.phase != Phase::Playing {
                return;
            }
            self.tick_fighter(id, dt);
        }
        for i in 0..self.creatures.len() {
            if self.phase != Phase::Playing {
                return;
            }
            self.tick_creature(i, dt);
        }
        self.regen(dt);
    }

    fn tick_fighter(&mut self, id: PlayerId, dt: Duration) {
        let kit = *self.settings.kits.get(self.players[&id].class);
        let melee_range = self.settings.melee_range;
        let p = self.seat(id);
        p.spell_cooldown = p.spell_cooldown.saturating_sub(dt);
        p.swing = p.swing.saturating_sub(dt);
        if !p.connected || !p.alive {
            return;
        }
        let from = p.position;
        let mut landed = None;
        if let Some(c) = &mut p.spell_cast {
            if dt >= c.remaining {
                landed = Some(c.creature);
                p.spell_cast = None;
            } else {
                c.remaining -= dt;
            }
        }
        if let Some(creature) = landed {
            let c = &self.creatures[creature.0];
            if c.alive && within(from, c.position, kit.spell.range) {
                self.land_spell(id, creature);
            } else {
                self.out.push(Event::SpellCastInterrupted { player: id });
            }
        }
        let p = &self.players[&id];
        let Some(creature) = p.attacking else { return };
        let c = &self.creatures[creature.0];
        if p.swing.is_zero() && c.alive && within(p.position, c.position, melee_range) {
            self.seat(id).swing = kit.swing;
            self.damage_creature(id, creature, kit.melee, false);
        }
    }

    fn tick_creature(&mut self, i: usize, dt: Duration) {
        let creature = CreatureId(i);
        let d = &self.layout.creatures[i];
        let (home, health, aggro, leash, speed, swing, damage) =
            (d.at, d.health, d.aggro, d.leash, d.speed, d.swing, d.damage);
        let melee = self.settings.melee_range;
        let c = &mut self.creatures[i];
        if !c.alive {
            if dt >= c.respawn_in {
                *c = Creature {
                    position: home,
                    health,
                    alive: true,
                    target: None,
                    tapped_by: None,
                    swing: Duration::ZERO,
                    respawn_in: Duration::ZERO,
                };
                self.out.push(Event::CreatureSpawned { creature, at: home });
            } else {
                c.respawn_in -= dt;
            }
            return;
        }
        // A target that died, left or fled past the leash ends the fight.
        if let Some(t) = c.target {
            let keep = (self.players.get(&t))
                .is_some_and(|p| p.connected && p.alive && within(home, p.position, leash));
            if !keep {
                self.evade(i);
                return;
            }
        }
        let c = &self.creatures[i];
        let target = match c.target {
            Some(t) => t,
            None => {
                let from = c.position;
                let nearest = (self.players.iter())
                    .filter(|(_, p)| p.connected && p.alive && within(from, p.position, aggro))
                    .min_by(|a, b| {
                        distance_squared(from, a.1.position)
                            .total_cmp(&distance_squared(from, b.1.position))
                    })
                    .map(|(&id, _)| id);
                let Some(t) = nearest else { return };
                self.engage(creature, t);
                t
            }
        };
        let goal = self.players[&target].position;
        let c = &mut self.creatures[i];
        let d2 = distance_squared(c.position, goal);
        if d2 > melee * melee {
            let dist = d2.sqrt();
            let step = (speed * dt.as_secs_f32()).min(dist - melee * CHASE_MARGIN);
            let k = step / dist;
            for (p, g) in c.position.iter_mut().zip(goal) {
                *p += (g - *p) * k;
            }
            let position = c.position;
            self.out.push(Event::CreatureMoved { creature, position });
        }
        let c = &mut self.creatures[i];
        c.swing = c.swing.saturating_sub(dt);
        if c.swing.is_zero() && within(c.position, goal, melee) {
            c.swing = swing;
            self.creature_hits(creature, target, damage);
        }
    }

    fn creature_hits(&mut self, creature: CreatureId, player: PlayerId, damage: u32) {
        let p = self.seat(player);
        p.health = p.health.saturating_sub(damage);
        let health = p.health;
        self.out.push(Event::CreatureHit {
            creature,
            player,
            damage,
            health,
        });
        if health == 0 {
            self.slain(player, creature);
        }
    }

    /// A creature killed `victim`: a ghost with a reportable body, exactly like a kill.
    fn slain(&mut self, victim: PlayerId, creature: CreatureId) {
        let p = self.seat(victim);
        p.alive = false;
        let at = p.position;
        self.bodies.push(Body {
            victim,
            position: at,
        });
        self.out.push(Event::Slain {
            victim,
            creature,
            at,
        });
        self.interrupt(victim);
        self.leave_combat(victim);
        self.release_stations(victim);
        self.check_win();
    }

    /// Living players out of combat (not attacking, no creature on them) regain health.
    fn regen(&mut self, dt: Duration) {
        let hunted: Vec<PlayerId> = (self.creatures.iter())
            .filter(|c| c.alive)
            .filter_map(|c| c.target)
            .collect();
        let per_second = self.settings.health_regen;
        for (id, p) in self.players.iter_mut() {
            let max = self.settings.kits.get(p.class).health;
            if !p.alive || p.attacking.is_some() || hunted.contains(id) || p.health >= max {
                p.regen = Duration::ZERO;
                continue;
            }
            p.regen += dt;
            while p.regen >= Duration::from_secs(1) {
                p.regen -= Duration::from_secs(1);
                p.health = (p.health + per_second).min(max);
            }
        }
    }
}
