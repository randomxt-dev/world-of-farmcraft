//! The match the world hosts: among-rules' [`Game`], fed by world entry, movement, the clock, the
//! action bar's casts and the HUD's addon requests. Notices go back as addon messages under
//! [`wire::PREFIX`]; casts, kills and cooldowns go back as WoW's own spell packets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use among_rules::wire::{self, Notice, Request, To};
use among_rules::{
    Actions, Class, Command, CreatureId, Event, Game, Layout, Phase, PlayerId, Position, Reject,
    SabotageDef, SabotageKind, Settings, TaskDef, TaskId, TaskKind, TaskStep, Vote,
};
use rand::RngCore;

use crate::look::Look;
use crate::world::creatures::{self, Shown};
use crate::world::spells::{self, CastRequest, Target};
use crate::world::{combat, guid, objects, packets, player, Map, Packet};

/// A kill's strike is shown only this close to it: to the killer, the victim and any witness.
const WITNESS_RANGE: f32 = 25.0;

/// The one match; cheap to clone. Lock order: the host, then the map.
#[derive(Clone)]
pub struct GameHost(Arc<Mutex<Inner>>);

struct Inner {
    game: Game,
    look: Look,
    map: Map,
    /// The buttons each player was last told are live.
    actions: HashMap<PlayerId, Actions>,
    /// Each cast on show, by caster: its spell and target, for the launch or the break.
    casts: HashMap<PlayerId, (u32, Target)>,
    /// The spell each caster's command was shown as, during one cast's apply.
    shown: HashMap<PlayerId, u32>,
    /// Each player's cooldowns as they stood at the last refresh, by spell.
    cooldowns: HashMap<PlayerId, Vec<(u32, Duration)>>,
    /// Each living player's health and maximum as clients were last told.
    health: HashMap<PlayerId, (u32, u32)>,
    /// The creature each player auto-attacks, for the stop that names it.
    attacking: HashMap<PlayerId, CreatureId>,
    /// The player each creature fights, for the stop that names them.
    hunting: HashMap<CreatureId, PlayerId>,
    /// Where each creature was last shown, the start of its next move.
    creature_at: HashMap<CreatureId, Position>,
}

impl GameHost {
    /// A match on `layout`, dressed by `look`; a look that does not fit the layout is replaced
    /// by a plain one.
    pub fn new(settings: Settings, layout: Layout, look: Look, map: Map) -> Self {
        let look = if look.covers(&layout) {
            look
        } else {
            tracing::warn!("game: the look does not fit the layout; using a plain one");
            Look::plain(&layout)
        };
        spawn_nodes(&layout, &look, &map);
        Self(Arc::new(Mutex::new(Inner {
            game: Game::new(settings, layout),
            look,
            map,
            actions: HashMap::new(),
            casts: HashMap::new(),
            shown: HashMap::new(),
            cooldowns: HashMap::new(),
            health: HashMap::new(),
            attacking: HashMap::new(),
            hunting: HashMap::new(),
            creature_at: HashMap::new(),
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A character entered the world: seat it in the lobby with its WoW class (`ChrClasses` id),
    /// or refuse while a game runs.
    pub fn join(&self, guid: u64, wow_class: u8) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        // Character creation offers only the nine 1.12 classes; anything else fights as a warrior.
        let class = Class::from_wow(wow_class).unwrap_or(Class::Warrior);
        if let Err(r) = inner.apply(Command::Join { player, class }) {
            inner.notify(To::Player(player), &Notice::Refused(r.to_string()));
        }
        let host = inner.game.host();
        inner.notify(To::Player(player), &Notice::Host(host));
    }

    /// A character left the world.
    pub fn leave(&self, guid: u64) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        let _ = inner.apply(Command::Leave { player });
        inner.actions.remove(&player);
        inner.cooldowns.remove(&player);
        inner.casts.remove(&player);
        inner.health.remove(&player);
    }

    /// A character moved; spectators outside the game are ignored.
    pub fn moved(&self, guid: u64, position: Position) {
        let _ = self.lock().apply(Command::Move {
            player: PlayerId(guid),
            position,
        });
    }

    /// A HUD button press; a refusal goes back to the presser with its reason.
    pub fn request(&self, guid: u64, request: Request) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        let result = inner
            .command(player, request)
            .and_then(|cmd| inner.apply(cmd));
        if let Err(r) = result {
            tracing::debug!("game: {guid} {request:?} refused: {r}");
            inner.notify(To::Player(player), &Notice::Refused(r.to_string()));
        }
    }

    /// An addon payload that is not a [`Request`].
    pub fn unknown_request(&self, guid: u64, payload: &str) {
        tracing::debug!("game: {guid} sent an unknown request {payload:?}");
        self.lock().notify(
            To::Player(PlayerId(guid)),
            &Notice::Refused("unknown request".into()),
        );
    }

    /// A `CMSG_CAST_SPELL`: an action-bar button, the class spell, or a gathering cast at a
    /// node. Every cast is answered, or the client's in-flight guard holds the bar for seconds:
    /// a refusal is a silent cast failure plus the rule's reason as red error text; an accepted
    /// press is shown by the events it caused, or as an instant cast of the pressed spell.
    pub fn cast(&self, guid: u64, cast: CastRequest) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        inner.shown.clear();
        let result = inner
            .cast_command(player, cast)
            .and_then(|cmd| inner.apply(cmd));
        let map = inner.map.clone();
        match result {
            Err(r) => {
                tracing::debug!("game: {guid} cast {} refused: {r}", cast.spell);
                map.send(guid, spells::cast_failed(cast.spell, spells::FAILED_SILENT));
                map.send(guid, spells::notification(&sentence(r)));
            }
            Ok(_) => match inner.shown.get(&player) {
                Some(&shown) if shown == cast.spell => {}
                // Shown as another spell (a task's gathering cast): free the pressed one.
                Some(_) => map.send(guid, spells::cast_ok(cast.spell)),
                None => {
                    map.send(guid, spells::spell_start(guid, cast.spell, 0, cast.target));
                    map.send(guid, spells::spell_go(guid, cast.spell, cast.target));
                }
            },
        }
    }

    /// A `CMSG_GAMEOBJ_USE` on a node with no lock: work its task step.
    pub fn use_object(&self, guid: u64, object: u64) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        let result = inner
            .node_command(player, object)
            .and_then(|cmd| inner.apply(cmd));
        if let Err(r) = result {
            tracing::debug!("game: {guid} use of {object:#x} refused: {r}");
            inner.map.send(guid, spells::notification(&sentence(r)));
        }
    }

    /// Whether a match is running (anything but the lobby).
    pub fn in_game(&self) -> bool {
        *self.lock().game.phase() != among_rules::Phase::Lobby
    }

    pub fn tick(&self, dt: Duration) {
        let _ = self.lock().apply(Command::Tick { dt });
    }

    /// A `CMSG_ATTACKSWING`: auto-attack a creature. Players are never fought: their FFA flag
    /// exists for Kill, so a swing at one is refused.
    pub fn attack(&self, guid: u64, target: u64) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        let result = guid::creature_id(target)
            .ok_or(Reject::InvalidTarget)
            .and_then(|creature| inner.apply(Command::Attack { player, creature }));
        let started =
            |e: &Event| matches!(e, Event::AttackStarted { player: p, .. } if *p == player);
        match result {
            Err(r) => {
                tracing::debug!("game: {guid} swing at {target:#x} refused: {r}");
                inner.map.send(guid, combat::cant_attack());
            }
            // Already on it: the client still waits for its echo to count as engaged.
            Ok(events) if !events.iter().any(started) => {
                inner.map.send(guid, combat::attack_start(guid, target));
            }
            Ok(_) => {}
        }
    }

    /// A `CMSG_ATTACKSTOP`.
    pub fn stop_attack(&self, guid: u64) {
        let _ = self.lock().apply(Command::StopAttack {
            player: PlayerId(guid),
        });
    }

    /// The answer to `CMSG_CREATURE_QUERY` for template `entry`: a monster's, or a miss.
    pub fn creature_query(&self, entry: u32) -> Packet {
        let inner = self.lock();
        let found = guid::creature_entry(entry).and_then(|id| {
            let def = inner.game.layout().creatures.get(id.0)?;
            Some((def.name.as_str(), inner.look.creatures.get(id.0)?))
        });
        creatures::query_response(entry, found)
    }

    /// The answer to `CMSG_GAMEOBJECT_QUERY` for template `entry`: a task node's, or a miss.
    pub fn gameobject_query(&self, entry: u32) -> Packet {
        let inner = self.lock();
        let node = guid::entry_step(entry).and_then(|(task, step)| inner.look.node(task, step));
        objects::query_response(entry, node)
    }
}

/// Put every task step's node in the world, where the step is done, for the whole session.
fn spawn_nodes(layout: &Layout, look: &Look, map: &Map) {
    for (t, def) in layout.tasks.iter().enumerate() {
        let task = TaskId(t);
        if !guid::fits(def.steps.len()) {
            tracing::warn!("game: task {:?} has too many steps to show", def.name);
            continue;
        }
        for (step, s) in def.steps.iter().enumerate() {
            let Some(node) = look.node(task, step) else {
                continue;
            };
            let g = guid::node(task, step);
            let create = objects::create_node(g, guid::node_entry(task, step), node, s.at, 0.0);
            map.spawn(g, create);
        }
    }
}

/// A refusal as red error text: "Too far away".
fn sentence(r: Reject) -> String {
    let text = r.to_string();
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

impl Inner {
    /// Apply `cmd`, send what its events tell whom and show them in the world, then any changed
    /// buttons and cooldowns.
    fn apply(&mut self, cmd: Command) -> Result<Vec<Event>, Reject> {
        let events = self.game.apply(cmd)?;
        for event in &events {
            tracing::debug!("game: {event:?}");
            for (to, notice) in wire::notices(event, &self.game) {
                self.notify(to, &notice);
            }
            self.show(event);
        }
        self.refresh_actions();
        self.refresh_cooldowns();
        self.refresh_health();
        Ok(events)
    }

    /// The world's side of an event: cast bars, launches and breaks, and kill strikes.
    fn show(&mut self, event: &Event) {
        match *event {
            Event::CastStarted { player, task, time } => {
                let step = (self.game.player(player))
                    .and_then(|p| p.tasks.iter().find(|t| t.task == task))
                    .map_or(0, |t| t.step);
                let spell = self.look.node(task, step).map_or(spells::USE, |n| n.spell);
                let target = Target::GameObject(guid::node(task, step));
                self.start_cast(player, spell, time, target);
            }
            Event::SpellCastStarted {
                player,
                creature,
                time,
            } => {
                let spell = self.class_spell(player);
                let target = Target::Unit(guid::creature(creature));
                self.start_cast(player, spell, time, target);
            }
            Event::CastFinished { player } => {
                if let Some((spell, target)) = self.casts.remove(&player) {
                    self.map
                        .broadcast(&spells::spell_go(player.0, spell, target));
                }
            }
            Event::CastInterrupted { player } | Event::SpellCastInterrupted { player } => {
                if let Some((spell, _)) = self.casts.remove(&player) {
                    self.map
                        .broadcast(&spells::spell_failed_other(player.0, spell));
                }
            }
            Event::SpellHit {
                player, creature, ..
            } => {
                let target = Target::Unit(guid::creature(creature));
                let spell = match self.casts.remove(&player) {
                    Some((spell, _)) => spell,
                    None => {
                        let spell = self.class_spell(player);
                        let start = spells::spell_start(player.0, spell, 0, target);
                        self.map.broadcast(&start);
                        spell
                    }
                };
                self.map
                    .broadcast(&spells::spell_go(player.0, spell, target));
                self.shown.insert(player, spell);
            }
            Event::Killed { killer, victim, at } => {
                let target = Target::Unit(victim.0);
                let start = spells::spell_start(killer.0, spells::KILL, 0, target);
                let go = spells::spell_go(killer.0, spells::KILL, target);
                for witness in self.witnesses(at, &[killer, victim]) {
                    self.map.send(witness.0, start.clone());
                    self.map.send(witness.0, go.clone());
                }
                self.shown.insert(killer, spells::KILL);
            }
            _ => {}
        }
        self.show_fight(event);
    }

    /// Monsters and auto-attack: spawns, chases, swings, damage, deaths and despawns.
    fn show_fight(&mut self, event: &Event) {
        let unit = guid::creature;
        match *event {
            Event::CreatureSpawned { creature, at } => {
                self.creature_at.insert(creature, at);
                self.hunting.remove(&creature);
                // A respawn replaces the corpse.
                self.map.despawn(unit(creature));
                if let Some(create) = self.creature_create(creature) {
                    self.map.spawn(unit(creature), create);
                }
            }
            Event::CreatureAggro { creature, player } => {
                self.hunting.insert(creature, player);
                self.map
                    .broadcast(&combat::attack_start(unit(creature), player.0));
            }
            Event::CreatureMoved { creature, position } => self.move_creature(creature, position),
            Event::CreatureEvaded { creature, at } => {
                let victim = self.hunting.remove(&creature).map_or(0, |p| p.0);
                self.map
                    .broadcast(&combat::attack_stop(unit(creature), victim));
                self.move_creature(creature, at);
                self.show_creature_health(creature);
            }
            Event::CreatureHit {
                creature,
                player,
                damage,
                ..
            } => {
                self.map
                    .broadcast(&combat::swing(unit(creature), player.0, damage));
            }
            Event::AttackStarted { player, creature } => {
                self.attacking.insert(player, creature);
                self.map
                    .broadcast(&combat::attack_start(player.0, unit(creature)));
            }
            Event::AttackStopped { player } => {
                let victim = self.attacking.remove(&player).map_or(0, unit);
                self.map.broadcast(&combat::attack_stop(player.0, victim));
            }
            Event::MeleeHit {
                player,
                creature,
                damage,
                ..
            } => {
                self.map
                    .broadcast(&combat::swing(player.0, unit(creature), damage));
                self.show_creature_health(creature);
            }
            Event::SpellHit {
                player,
                creature,
                damage,
                ..
            } => {
                let class = self.game.player(player).map_or(Class::Warrior, |p| p.class);
                self.map.broadcast(&combat::spell_damage(
                    unit(creature),
                    player.0,
                    spells::class_spell(class),
                    damage,
                    spells::class_school(class),
                ));
                self.show_creature_health(creature);
            }
            Event::CreatureDied { creature, .. } => {
                if let Some(victim) = self.hunting.remove(&creature) {
                    self.map
                        .broadcast(&combat::attack_stop(unit(creature), victim.0));
                }
                self.show_creature_health(creature);
            }
            Event::ReturnedToLobby => {
                for i in 0..self.game.layout().creatures.len() {
                    self.map.despawn(unit(CreatureId(i)));
                }
                self.creature_at.clear();
                self.hunting.clear();
                self.attacking.clear();
            }
            _ => {}
        }
    }

    /// The creature as it stands now, for newcomers; `None` outside a game.
    fn creature_create(&self, creature: CreatureId) -> Option<Packet> {
        let c = self.game.creatures().get(creature.0)?;
        let def = self.game.layout().creatures.get(creature.0)?;
        let look = self.look.creatures.get(creature.0)?;
        let shown = Shown {
            guid: guid::creature(creature),
            entry: creature.0 as u32 + 1,
            position: c.position,
            orientation: 0.0,
            health: if c.alive { c.health } else { 0 },
            max_health: def.health,
            swing_ms: def.swing.as_millis() as u32,
        };
        Some(creatures::create(&shown, look))
    }

    /// Run the creature from where it was last shown to `to`, at its chase speed.
    fn move_creature(&mut self, creature: CreatureId, to: Position) {
        let from = self.creature_at.insert(creature, to).unwrap_or(to);
        let speed = (self.game.layout().creatures.get(creature.0)).map_or(6.0, |d| d.speed);
        let yards = among_rules::distance_squared(from, to).sqrt();
        let ms = (yards / speed.max(0.1) * 1000.0) as u32;
        let g = guid::creature(creature);
        self.map.broadcast(&creatures::move_to(g, from, to, ms));
        if let Some(create) = self.creature_create(creature) {
            self.map.restate(g, create);
        }
    }

    fn show_creature_health(&mut self, creature: CreatureId) {
        let (Some(c), Some(def)) = (
            self.game.creatures().get(creature.0),
            self.game.layout().creatures.get(creature.0),
        ) else {
            return;
        };
        let health = if c.alive { c.health } else { 0 };
        let g = guid::creature(creature);
        self.map.broadcast(&combat::health(g, health, def.health));
        if let Some(create) = self.creature_create(creature) {
            self.map.restate(g, create);
        }
    }

    /// Tell everyone each living player's health when it changed: hits, regeneration, and the
    /// class's pool at join and after a meeting. A dead player's health is left as it was; the
    /// world's side of a death (corpse, ghost) is not shown yet.
    fn refresh_health(&mut self) {
        let kits = &self.game.settings().kits;
        let changed: Vec<(PlayerId, (u32, u32))> = (self.game.players())
            .filter(|(_, p)| p.connected && p.alive)
            .map(|(id, p)| (id, (p.health.max(1), kits.get(p.class).health)))
            .filter(|(id, h)| self.health.get(id) != Some(h))
            .collect();
        for (id, (health, max)) in changed {
            self.health.insert(id, (health, max));
            self.map.broadcast(&combat::health(id.0, health, max));
        }
    }

    /// Show `player`'s cast of `spell` to everyone: a bar of `time`, at `target`.
    fn start_cast(&mut self, player: PlayerId, spell: u32, time: Duration, target: Target) {
        let ms = time.as_millis().min(u32::MAX.into()) as u32;
        self.map
            .broadcast(&spells::spell_start(player.0, spell, ms, target));
        self.casts.insert(player, (spell, target));
        self.shown.insert(player, spell);
    }

    fn class_spell(&self, player: PlayerId) -> u32 {
        let class = self.game.player(player).map_or(Class::Warrior, |p| p.class);
        spells::class_spell(class)
    }

    /// `always`, then every other connected player within [`WITNESS_RANGE`] of `at`.
    fn witnesses(&self, at: Position, always: &[PlayerId]) -> Vec<PlayerId> {
        let mut out = always.to_vec();
        out.extend(
            (self.game.players())
                .filter(|(id, p)| {
                    p.connected
                        && !always.contains(id)
                        && among_rules::within(p.position, at, WITNESS_RANGE)
                })
                .map(|(id, _)| id),
        );
        out
    }

    /// What a cast means: the button it is, the class spell at a creature, or work at a node.
    fn cast_command(&self, player: PlayerId, cast: CastRequest) -> Result<Command, Reject> {
        let g = &self.game;
        if let Target::GameObject(object) = cast.target {
            return self.node_command(player, object);
        }
        let request = match cast.spell {
            // Kill strikes the player the client aimed at, if it aimed at one.
            spells::KILL => match cast.target {
                Target::Unit(victim) if g.player(PlayerId(victim)).is_some() => {
                    return Ok(Command::Kill {
                        killer: player,
                        victim: PlayerId(victim),
                    })
                }
                _ => Request::Kill,
            },
            spells::USE => Request::Use,
            spells::REPORT => Request::Report,
            spells::FIX => Request::Fix,
            spells::SKIP_VOTE => Request::Vote(Vote::Skip),
            spells::SABOTAGE => match g.sabotage_target(player) {
                Some(kind) => Request::Sabotage(kind),
                None => return Err(no_sabotage(g, player)),
            },
            // The horn starts the game from the lobby and goes back to it after one.
            spells::EMERGENCY => match g.phase() {
                Phase::Lobby => Request::Start,
                Phase::GameOver { .. } => Request::Lobby,
                _ => Request::Emergency,
            },
            spell if spell == self.class_spell(player) => {
                return match cast.target {
                    Target::Unit(unit) => guid::creature_id(unit)
                        .map(|creature| Command::CastSpell { player, creature })
                        .ok_or(Reject::InvalidTarget),
                    _ => Err(Reject::InvalidTarget),
                };
            }
            _ => return Err(Reject::InvalidTarget),
        };
        self.command(player, request)
    }

    /// Work at a task node: the step it stands for, if that is the player's current step.
    fn node_command(&self, player: PlayerId, object: u64) -> Result<Command, Reject> {
        let (task, step) = guid::node_step(object).ok_or(Reject::InvalidTarget)?;
        let cmd = Command::UseTask { player, task };
        let current = (self.game.player(player))
            .and_then(|p| p.tasks.iter().find(|t| t.task == task))
            .map(|t| t.step);
        match current {
            Some(s) if s == step => Ok(cmd),
            // Past this node's step: done here.
            Some(s) if s > step => Err(Reject::TaskDone),
            // Validation names the rule in the way: phase, death, not your task.
            _ => Err(self
                .game
                .validate(&cmd)
                .err()
                .unwrap_or(Reject::TaskNotAssigned)),
        }
    }

    /// The command a press means now. Kill, report, use and fix aim at the nearest legal target;
    /// with none, the refusal names the first rule in the way, or "too far away".
    fn command(&self, player: PlayerId, request: Request) -> Result<Command, Reject> {
        let g = &self.game;
        let none = |probe: Command| match g.validate(&probe) {
            Err(Reject::InvalidTarget | Reject::NoSuchBody) | Ok(()) => Reject::OutOfRange,
            Err(r) => r,
        };
        Ok(match request {
            Request::Start => Command::Start {
                by: player,
                seed: rand::thread_rng().next_u64(),
            },
            Request::Kill => match g.kill_target(player) {
                Some(victim) => Command::Kill {
                    killer: player,
                    victim,
                },
                None => {
                    return Err(none(Command::Kill {
                        killer: player,
                        victim: player,
                    }))
                }
            },
            Request::Report => match g.report_target(player) {
                Some(body) => Command::Report { player, body },
                None => {
                    return Err(none(Command::Report {
                        player,
                        body: player,
                    }))
                }
            },
            Request::Use => match g.task_target(player) {
                Some(task) => Command::UseTask { player, task },
                None => return Err(no_task(g, player)),
            },
            Request::Fix => match g.fix_target(player) {
                Some(station) => Command::Fix { player, station },
                None => {
                    return Err(none(Command::Fix {
                        player,
                        station: usize::MAX,
                    }))
                }
            },
            Request::Emergency => Command::CallEmergency { player },
            Request::Lobby => Command::ReturnToLobby { by: player },
            Request::Vote(vote) => Command::Vote {
                voter: player,
                vote,
            },
            Request::Sabotage(kind) => Command::Sabotage { player, kind },
            Request::Release => Command::ReleaseFix { player },
        })
    }

    /// Tell every seated player whose live buttons changed.
    fn refresh_actions(&mut self) {
        let changed: Vec<(PlayerId, Actions)> = (self.game.players())
            .filter(|(_, p)| p.connected)
            .map(|(id, _)| (id, self.game.actions(id)))
            .filter(|(id, a)| self.actions.get(id) != Some(a))
            .collect();
        for (id, a) in changed {
            self.actions.insert(id, a);
            self.notify(To::Player(id), &Notice::Actions(a));
        }
    }

    /// Start the cooldown sweep of every button whose cooldown began since the last refresh:
    /// kill, sabotage, emergency and the class spell. A client's sweep can grow but not shrink,
    /// so only a cooldown that rose is sent.
    fn refresh_cooldowns(&mut self) {
        let playing = *self.game.phase() == Phase::Playing;
        let g = &self.game;
        let now: Vec<(PlayerId, Vec<(u32, Duration)>)> = (g.players())
            .filter(|(_, p)| p.connected)
            .map(|(id, p)| {
                let mut cds = vec![(spells::class_spell(p.class), p.spell_cooldown)];
                if playing {
                    cds.push((spells::EMERGENCY, g.emergency_cooldown()));
                    if p.is_impostor() {
                        cds.push((spells::KILL, p.kill_cooldown));
                        if g.sabotage().is_none() {
                            cds.push((spells::SABOTAGE, g.sabotage_cooldown()));
                        }
                    }
                }
                (id, cds)
            })
            .collect();
        for (id, cds) in now {
            let before = self.cooldowns.get(&id);
            let rose: Vec<(u32, u32)> = (cds.iter())
                .filter(|(spell, d)| {
                    let was = (before.into_iter().flatten())
                        .find(|(s, _)| s == spell)
                        .map_or(Duration::ZERO, |(_, d)| *d);
                    *d > was
                })
                .map(|&(spell, d)| (spell, d.as_millis().min(u32::MAX.into()) as u32))
                .collect();
            if !rose.is_empty() {
                self.map.send(id.0, spells::spell_cooldown(id.0, &rose));
            }
            self.cooldowns.insert(id, cds);
        }
    }

    /// Each recipient gets the line under its own guid, which its name query always answers.
    fn notify(&self, to: To, notice: &Notice) {
        let payload = notice.encode();
        let send = |guid: u64| {
            self.map
                .send(guid, packets::addon_message(guid, wire::PREFIX, &payload));
        };
        match to {
            To::Player(p) => send(p.0),
            To::All => self.map.guids().into_iter().for_each(send),
        }
    }
}

/// Why a Sabotage press found nothing to sabotage: the rule a sabotage of any kind meets first.
fn no_sabotage(g: &Game, player: PlayerId) -> Reject {
    let kind = (g.layout().sabotages.first()).map_or(SabotageKind::Lights, |s| s.kind);
    g.validate(&Command::Sabotage { player, kind })
        .err()
        .unwrap_or(Reject::NoSuchSabotage)
}

/// Why a Use press found no task step. Phase and player rules refuse every task alike; past them
/// a cast step is out of reach, a Slay step is fought, and with no step left the tasks are done.
fn no_task(g: &Game, player: PlayerId) -> Reject {
    let use_task = |task| Command::UseTask { player, task };
    let reasons: Vec<Reject> = (g.player(player).into_iter())
        .flat_map(|p| p.tasks.iter())
        .filter(|t| t.step < g.layout().tasks[t.task.0].steps.len())
        .filter_map(|t| g.validate(&use_task(t.task)).err())
        .collect();
    if reasons.is_empty() {
        // No step left, or no tasks at all (the lobby): the rule a made-up task meets first.
        return match g.validate(&use_task(among_rules::TaskId(usize::MAX))) {
            Err(Reject::TaskNotAssigned) | Ok(()) => Reject::TaskDone,
            Err(r) => r,
        };
    }
    (reasons.iter().copied())
        .find(|r| *r != Reject::NotAUseStep)
        .unwrap_or(Reject::NotAUseStep)
}

/// Stand-in spots around Northshire Abbey's steps until the map is surveyed: the button on the
/// spawn, tasks and stations 8-25 yd out at the spawn's height, two wolves 40 yd out.
pub fn placeholder_layout() -> Layout {
    placeholder().0
}

/// How [`placeholder_layout`] looks: herbs, a copper vein, crafting stations and wolves.
pub fn placeholder_look() -> Look {
    placeholder().1
}

fn placeholder() -> (Layout, Look) {
    use crate::look::{self, CreatureLook, NodeLook};
    use among_rules::{CreatureDef, StepAction};
    let [x, y, z] = player::SPAWN.position;
    let at = |dx: f32, dy: f32| [x + dx, y + dy, z];
    // Gathering takes 5 s, as Herb Gathering and the kneeling "Opening" do; crafting 3 s.
    let herb = |dx, dy, name, display| {
        (
            TaskStep::cast(at(dx, dy), 5.0),
            NodeLook::herb(name, display),
        )
    };
    let vein = |dx, dy| {
        let node = NodeLook::kneel("Copper Vein", look::COPPER_VEIN);
        (TaskStep::cast(at(dx, dy), 5.0), node)
    };
    let craft = |dx, dy, name, display, spell| {
        let node = NodeLook::station(name, display, spell);
        (TaskStep::cast(at(dx, dy), 3.0), node)
    };
    // Two young wolves past the tasks, beyond aggro range of every task spot.
    let den = at(-30.0, 29.0);
    let creatures = vec![
        CreatureDef::beast("Young Wolf", at(-30.0, 25.0)),
        CreatureDef::beast("Young Wolf", at(-30.0, 33.0)),
    ];
    let slay = TaskStep {
        at: den,
        action: StepAction::Slay {
            creatures: vec![CreatureId(0), CreatureId(1)],
            count: 2,
        },
    };
    // Each task's name, kind, and steps with the node each is worked at; a Slay step has none.
    type Steps = Vec<(TaskStep, Option<NodeLook>)>;
    let herb = |dx, dy, name, display| {
        let (s, n) = herb(dx, dy, name, display);
        (s, Some(n))
    };
    let vein = |dx, dy| {
        let (s, n) = vein(dx, dy);
        (s, Some(n))
    };
    let craft = |dx, dy, name, display, spell| {
        let (s, n) = craft(dx, dy, name, display, spell);
        (s, Some(n))
    };
    let tasks: Vec<(&str, TaskKind, Steps)> = vec![
        (
            "Pick Peacebloom",
            TaskKind::Common,
            vec![herb(10.0, 0.0, "Peacebloom", look::PEACEBLOOM)],
        ),
        (
            "Roast Boar Meat",
            TaskKind::Common,
            vec![craft(-10.0, 0.0, "Campfire", look::CAMPFIRE, 2540)],
        ),
        (
            "Mine and smelt Copper",
            TaskKind::Long,
            vec![
                vein(20.0, 10.0),
                craft(20.0, -10.0, "Forge", look::FORGE, 2657),
            ],
        ),
        (
            "Gather and roast Earthroot",
            TaskKind::Long,
            vec![
                herb(-20.0, 10.0, "Earthroot", look::EARTHROOT),
                craft(-20.0, -10.0, "Campfire", look::CAMPFIRE, 2538),
            ],
        ),
        (
            "Pick Silverleaf",
            TaskKind::Short,
            vec![herb(0.0, 10.0, "Silverleaf", look::SILVERLEAF)],
        ),
        (
            "Pick Earthroot",
            TaskKind::Short,
            vec![herb(0.0, -10.0, "Earthroot", look::EARTHROOT)],
        ),
        (
            "Mine a Copper Vein",
            TaskKind::Short,
            vec![vein(10.0, 15.0)],
        ),
        (
            "Hammer Copper Bracers",
            TaskKind::Short,
            vec![craft(-10.0, -15.0, "Anvil", look::ANVIL, 2663)],
        ),
        ("Cull the Young Wolves", TaskKind::Short, vec![(slay, None)]),
    ];
    let sabotage = |kind, stations, deadline| SabotageDef {
        kind,
        stations,
        deadline,
    };
    let critical = Some(Duration::from_secs(30));
    let layout = Layout {
        emergency_button: at(0.0, 0.0),
        tasks: (tasks.iter())
            .map(|(name, kind, steps)| TaskDef {
                name: (*name).into(),
                kind: *kind,
                steps: steps.iter().map(|(s, _)| s.clone()).collect(),
            })
            .collect(),
        sabotages: vec![
            sabotage(SabotageKind::Lights, vec![at(8.0, 8.0)], None),
            sabotage(SabotageKind::Comms, vec![at(-8.0, 8.0)], None),
            sabotage(
                SabotageKind::Oxygen,
                vec![at(15.0, -20.0), at(-15.0, -20.0)],
                critical,
            ),
            sabotage(
                SabotageKind::Reactor,
                vec![at(25.0, 0.0), at(25.0, 8.0)],
                critical,
            ),
        ],
        creatures,
    };
    let look = Look {
        steps: (tasks.into_iter())
            .map(|(_, _, steps)| steps.into_iter().map(|(_, n)| n).collect())
            .collect(),
        creatures: vec![CreatureLook::wolf(), CreatureLook::wolf()],
    };
    (layout, look)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::CharacterRecord;
    use crate::world::Packet;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    /// A host on the placeholder map with players `1..=n` in the world at the spawn; each
    /// receiver is that player's outbox.
    fn hosted(settings: Settings, n: u64) -> (GameHost, Vec<UnboundedReceiver<Packet>>) {
        let map = Map::default();
        let host = GameHost::new(
            settings,
            placeholder_layout(),
            placeholder_look(),
            map.clone(),
        );
        let mut inboxes = Vec::new();
        for guid in 1..=n {
            let (tx, rx) = unbounded_channel();
            let character = CharacterRecord {
                guid,
                name: format!("P{guid}"),
                race: 1,
                class: 1,
                gender: 0,
                skin: 0,
                face: 0,
                hair_style: 0,
                hair_color: 0,
                facial_hair: 0,
                level: 1,
            };
            map.enter(character, tx, player::spawn_living());
            host.join(guid, 1);
            inboxes.push(rx);
        }
        (host, inboxes)
    }

    /// The Among Azeroth notices waiting in `inbox`, decoded; everything else is skipped.
    fn notices(inbox: &mut UnboundedReceiver<Packet>) -> Vec<Notice> {
        let mut out = Vec::new();
        while let Ok(p) = inbox.try_recv() {
            let Ok((ServerPacket::MessageChat(m), _)) = parse_server_with_tail(p.opcode, &p.body)
            else {
                continue;
            };
            let (prefix, payload) = m.text.split_once('\t').unwrap();
            assert_eq!(prefix, wire::PREFIX);
            out.push(Notice::decode(payload).expect(payload));
        }
        out
    }

    fn quick() -> Settings {
        Settings {
            min_players: 3,
            initial_kill_cooldown: Duration::ZERO,
            ..Settings::default()
        }
    }

    /// Every packet waiting in `inbox`, as the client reads it.
    fn received(inbox: &mut UnboundedReceiver<Packet>) -> Vec<ServerPacket> {
        let mut out = Vec::new();
        while let Ok(p) = inbox.try_recv() {
            let (packet, tail) = parse_server_with_tail(p.opcode, &p.body).expect("parses");
            assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
            out.push(packet);
        }
        out
    }

    fn drain(inboxes: &mut [UnboundedReceiver<Packet>]) {
        inboxes.iter_mut().for_each(|i| drop(received(i)));
    }

    /// A cast of `spell` at `target` from `guid`.
    fn cast(host: &GameHost, guid: u64, spell: u32, target: Target) {
        host.cast(guid, CastRequest { spell, target });
    }

    /// The started game's impostor and crewmates.
    fn roles(host: &GameHost) -> (u64, Vec<u64>) {
        let inner = host.lock();
        let imp = (inner.game.players())
            .find(|(_, p)| p.is_impostor())
            .unwrap()
            .0
             .0;
        let crew = (inner.game.players())
            .filter(|(_, p)| p.is_crewmate())
            .map(|(id, _)| id.0)
            .collect();
        (imp, crew)
    }

    fn launches(seen: &[ServerPacket], spell: u32) -> Vec<u64> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::SpellGo(go) if go.spell_id == spell => Some(go.caster),
                _ => None,
            })
            .collect()
    }

    /// Whether `seen` holds a values update giving `guid` this health.
    fn health_of(seen: &[ServerPacket], guid: u64) -> Vec<u32> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::UpdateObject { objects } => Some(objects),
                _ => None,
            })
            .flatten()
            .filter_map(|o| match o {
                benilla_protocol::messages::Object::Values { guid: g, mask } if *g == guid => {
                    mask.unit_health()
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_wolf_fight_shows_aggro_swings_damage_and_death() {
        let (host, mut inboxes) = hosted(quick(), 3);
        host.request(1, Request::Start);
        let wolf = guid::creature(CreatureId(0));
        // Everyone is shown both wolves, and their template answers by entry.
        let seen = received(&mut inboxes[2]);
        let created: Vec<u64> = (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::UpdateObject { objects } => Some(objects),
                _ => None,
            })
            .flatten()
            .filter_map(|o| match o {
                benilla_protocol::messages::Object::Create { guid, .. } => Some(*guid),
                _ => None,
            })
            .collect();
        assert!(created.contains(&wolf) && created.contains(&guid::creature(CreatureId(1))));
        let entry = benilla_protocol::guid::entry(wolf).unwrap();
        let ServerPacket::CreatureQueryResponse { info, .. } =
            parse_server_with_tail(0x61, &host.creature_query(entry).body)
                .unwrap()
                .0
        else {
            panic!("not a creature template");
        };
        assert_eq!(info.unwrap().name, "Young Wolf");

        // A crewmate walks up; the nearer wolf turns on it and runs in.
        let c = roles(&host).1[0];
        let me = c as usize - 1;
        let home = host.lock().game.layout().creatures[0].at;
        host.moved(c, [home[0] + 6.0, home[1] - 3.0, home[2]]);
        host.tick(Duration::from_millis(100));
        let seen = received(&mut inboxes[me]);
        assert!(seen.iter().any(|p| matches!(p,
            ServerPacket::AttackStart { attacker, victim } if *attacker == wolf && *victim == c)));
        assert!(seen.iter().any(|p| matches!(p,
            ServerPacket::MonsterMove { guid, stop: false, .. } if *guid == wolf)));

        // Swinging at a player is refused; at the wolf, it is echoed so the client engages.
        host.attack(c, roles(&host).0);
        assert!(received(&mut inboxes[me])
            .iter()
            .any(|p| matches!(p, ServerPacket::AttackSwingError(_))));
        host.attack(c, wolf);
        assert!(received(&mut inboxes[me]).iter().any(|p| matches!(p,
            ServerPacket::AttackStart { attacker, victim } if *attacker == c && *victim == wolf)));

        // Heroic Strike, instant, once the wolf is in reach: the cast shows to everyone, with
        // the damage in the log.
        for _ in 0..10 {
            host.tick(Duration::from_millis(100));
            let inner = host.lock();
            let at = inner.game.player(PlayerId(c)).unwrap().position;
            if among_rules::within(inner.game.creatures()[0].position, at, 5.0) {
                break;
            }
        }
        drain(&mut inboxes);
        cast(
            &host,
            c,
            spells::class_spell(Class::Warrior),
            Target::Unit(wolf),
        );
        let seen = received(&mut inboxes[(me + 1) % 3]);
        assert_eq!(launches(&seen, 78), [c]);
        assert!(seen.iter().any(|p| matches!(p,
            ServerPacket::SpellDamageLog(d) if d.target == wolf && d.attacker == c && d.damage == 15)));
        let left = host.lock().game.creatures()[0].health;
        assert!(left <= 25);
        assert_eq!(health_of(&seen, wolf).last(), Some(&left));

        // The fight runs: swings both ways, the crewmate's health drops, the wolf dies.
        let mut all = Vec::new();
        for _ in 0..100 {
            host.tick(Duration::from_millis(100));
            all.extend(received(&mut inboxes[me]));
            if !host.lock().game.creatures()[0].alive {
                break;
            }
        }
        assert!(!host.lock().game.creatures()[0].alive, "the wolf died");
        assert!(all.iter().any(|p| matches!(p,
            ServerPacket::AttackerState(s) if s.attacker == c && s.victim == wolf)));
        assert!(all.iter().any(|p| matches!(p,
            ServerPacket::AttackerState(s) if s.attacker == wolf && s.victim == c)));
        assert_eq!(health_of(&all, wolf).last(), Some(&0), "a corpse");
        let mine = health_of(&all, c);
        assert!(mine.last().is_some_and(|&h| h < 120), "{mine:?}");
        assert!(all.iter().any(|p| matches!(p,
            ServerPacket::AttackStop { attacker, victim } if *attacker == c && *victim == wolf)));
    }

    #[test]
    fn the_horn_starts_the_game_from_the_lobby() {
        let (host, mut inboxes) = hosted(quick(), 3);
        drain(&mut inboxes);
        cast(&host, 2, spells::EMERGENCY, Target::Caster);
        let seen = received(&mut inboxes[1]);
        assert!(seen.iter().any(|p| matches!(
            p,
            ServerPacket::CastResult {
                spell_id: spells::EMERGENCY,
                outcome: benilla_protocol::messages::CastOutcome::Failed { reason: 0x17, .. }
            }
        )));
        assert!(seen.iter().any(|p| matches!(p,
            ServerPacket::Notification { text } if text == "Only the host can do that")));
        cast(&host, 1, spells::EMERGENCY, Target::Caster);
        assert!(matches!(host.lock().game.phase(), Phase::Playing));
        let seen = received(&mut inboxes[0]);
        assert_eq!(
            launches(&seen, spells::EMERGENCY),
            [1],
            "the press is answered"
        );
    }

    #[test]
    fn a_kill_cast_strikes_the_aimed_crewmate_before_witnesses_only() {
        let (host, mut inboxes) = hosted(
            Settings {
                min_players: 4,
                ..quick()
            },
            4,
        );
        host.request(1, Request::Start);
        let (imp, crew) = roles(&host);
        // One crewmate walks off out of sight; a crewmate's Kill is refused with the reason.
        let far = crew[2];
        host.moved(far, [-8900.0, -132.0, 83.5]);
        drain(&mut inboxes);
        cast(&host, crew[0], spells::KILL, Target::Unit(crew[1]));
        assert!(received(&mut inboxes[crew[0] as usize - 1])
            .iter()
            .any(|p| matches!(p,
            ServerPacket::Notification { text } if text == "Only impostors can do that")));

        cast(&host, imp, spells::KILL, Target::Unit(crew[1]));
        assert!(!host.lock().game.player(PlayerId(crew[1])).unwrap().alive);
        for g in [imp, crew[0], crew[1]] {
            let seen = received(&mut inboxes[g as usize - 1]);
            assert_eq!(launches(&seen, spells::KILL), [imp], "{g} sees the strike");
            if g == imp {
                // The kill cooldown starts its sweep on the button.
                assert!(seen.iter().any(|p| matches!(p,
                    ServerPacket::SpellCooldownList { caster, cooldowns }
                        if *caster == imp && cooldowns.contains(&(spells::KILL, 25_000)))));
            }
        }
        let seen = received(&mut inboxes[far as usize - 1]);
        assert!(
            launches(&seen, spells::KILL).is_empty(),
            "too far to witness"
        );
    }

    #[test]
    fn a_task_cast_shows_its_gathering_spell_to_everyone_and_breaks_on_a_step() {
        let (host, mut inboxes) = hosted(quick(), 3);
        host.request(1, Request::Start);
        // A crewmate, whose task advances; an impostor's cover task never does.
        let c = roles(&host).1[0];
        let me = c as usize - 1;
        let other = (me + 1) % 3;
        let (task, at, node) = {
            let inner = host.lock();
            let p = inner.game.player(PlayerId(c)).unwrap();
            let t = p.tasks[0];
            let at = inner.game.layout().tasks[t.task.0].steps[t.step].at;
            (t.task, at, inner.look.node(t.task, t.step).unwrap().clone())
        };
        host.moved(c, at);
        drain(&mut inboxes);
        // Use from the bar: shown as the node's own cast, the Use press freed.
        cast(&host, c, spells::USE, Target::Caster);
        let mine = received(&mut inboxes[me]);
        assert!(mine.iter().any(|p| matches!(
            p,
            ServerPacket::CastResult {
                spell_id: spells::USE,
                outcome: benilla_protocol::messages::CastOutcome::Ok
            }
        )));
        for seen in [mine, received(&mut inboxes[other])] {
            let start = seen.iter().find_map(|p| match p {
                ServerPacket::SpellStart(s) => Some(s.clone()),
                _ => None,
            });
            let start = start.expect("a cast bar");
            assert_eq!((start.caster, start.spell_id), (c, node.spell));
            assert_eq!(start.targets.go_target, Some(guid::node(task, 0)));
            assert!(start.cast_time_ms >= 3000);
        }
        // A step breaks it for everyone.
        host.moved(c, [at[0] + 1.0, at[1], at[2]]);
        for inbox in &mut inboxes {
            assert!(
                received(inbox).iter().any(|p| matches!(p,
                ServerPacket::SpellFailedOther { caster, spell_id } if *caster == c && *spell_id == node.spell))
            );
        }
        // Right-clicking the node casts at it; the cast runs out and launches for everyone.
        host.moved(c, at);
        cast(
            &host,
            c,
            node.spell,
            Target::GameObject(guid::node(task, 0)),
        );
        host.tick(Duration::from_secs(6));
        for inbox in &mut inboxes {
            assert_eq!(launches(&received(inbox), node.spell), [c]);
        }
        // The node of a later step, or someone else's task, is refused with the reason.
        cast(
            &host,
            c,
            node.spell,
            Target::GameObject(guid::node(task, 0)),
        );
        let seen = received(&mut inboxes[me]);
        let names: Vec<String> = seen.iter().map(|p| p.name()).collect();
        assert!(
            seen.iter()
                .any(|p| matches!(p, ServerPacket::Notification { .. })),
            "{names:?}"
        );
    }

    #[test]
    fn joiners_learn_the_host_and_only_the_host_starts() {
        let (host, mut inboxes) = hosted(quick(), 3);
        for inbox in &mut inboxes {
            assert!(notices(inbox).contains(&Notice::Host(Some(PlayerId(1)))));
        }
        host.request(2, Request::Start);
        assert_eq!(
            notices(&mut inboxes[1]),
            [Notice::Refused(Reject::NotHost.to_string())]
        );
        host.request(1, Request::Start);
        let all = notices(&mut inboxes[2]);
        assert!(all.contains(&Notice::Started {
            players: 3,
            impostors: 1
        }));
        assert!(all.iter().any(|n| matches!(n, Notice::Role { .. })));
        assert_eq!(
            all.iter()
                .filter(|n| matches!(n, Notice::Task { .. }))
                .count(),
            4
        );
    }

    #[test]
    fn a_kill_press_aims_at_the_nearest_crewmate_and_tells_only_the_victim() {
        let (host, mut inboxes) = hosted(quick(), 3);
        host.request(1, Request::Start);
        let roles: Vec<Vec<Notice>> = inboxes.iter_mut().map(notices).collect();
        let impostor = (1..=3u64)
            .find(|&g| {
                roles[g as usize - 1].iter().any(|n| {
                    matches!(
                        n,
                        Notice::Role {
                            role: among_rules::Role::Impostor,
                            ..
                        }
                    )
                })
            })
            .unwrap();
        // Everyone stands on the spawn, so the impostor's Kill is live.
        assert!(roles[impostor as usize - 1]
            .iter()
            .any(|n| matches!(n, Notice::Actions(Actions { kill: true, .. }))));
        // Crewmates' presses are refused with the rule's reason.
        let crew = (1..=3u64).find(|&g| g != impostor).unwrap();
        host.request(crew, Request::Kill);
        assert_eq!(
            notices(&mut inboxes[crew as usize - 1]),
            [Notice::Refused(Reject::NotImpostor.to_string())]
        );

        host.request(impostor, Request::Kill);
        let told: Vec<(u64, Vec<Notice>)> = (1..=3u64)
            .map(|g| (g, notices(&mut inboxes[g as usize - 1])))
            .collect();
        let victims: Vec<u64> = (told.iter())
            .filter(|(_, n)| n.contains(&Notice::Dead))
            .map(|(g, _)| *g)
            .collect();
        assert_eq!(victims.len(), 1, "{told:?}");
        assert_ne!(victims[0], impostor);
        // One crewmate left against one impostor: the game is over for everyone.
        for (_, n) in &told {
            assert!(n.iter().any(|n| matches!(n, Notice::Over { .. })), "{n:?}");
        }
    }

    #[test]
    fn report_and_vote_presses_run_a_meeting() {
        let (host, mut inboxes) = hosted(
            Settings {
                min_players: 4,
                ..quick()
            },
            4,
        );
        host.request(1, Request::Start);
        let impostor = (1..=4u64)
            .find(|&g| {
                notices(&mut inboxes[g as usize - 1]).iter().any(|n| {
                    matches!(
                        n,
                        Notice::Role {
                            role: among_rules::Role::Impostor,
                            ..
                        }
                    )
                })
            })
            .unwrap();
        host.request(impostor, Request::Kill);
        let reporter = (1..=4u64)
            .find(|&g| {
                g != impostor && !notices(&mut inboxes[g as usize - 1]).contains(&Notice::Dead)
            })
            .unwrap();
        host.request(reporter, Request::Report);
        let seen = notices(&mut inboxes[reporter as usize - 1]);
        assert!(
            seen.iter().any(|n| matches!(n, Notice::Meeting { caller, body: Some(_), .. } if caller.0 == reporter)),
            "{seen:?}"
        );
        // Votes wait for the discussion to end.
        host.request(reporter, Request::Vote(among_rules::Vote::Skip));
        assert_eq!(
            notices(&mut inboxes[reporter as usize - 1]),
            [Notice::Refused(Reject::WrongPhase.to_string())]
        );
        host.tick(Duration::from_secs(15));
        host.request(
            reporter,
            Request::Vote(among_rules::Vote::Player(PlayerId(impostor))),
        );
        let seen = notices(&mut inboxes[reporter as usize - 1]);
        assert!(seen.contains(&Notice::Voting(Duration::from_secs(120))));
        assert!(seen.contains(&Notice::Voted(PlayerId(reporter))));
    }
}
