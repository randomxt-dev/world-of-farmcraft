//! The match the world hosts: among-rules' [`Game`], fed by world entry, movement, the clock, the
//! action bar's casts and the HUD's addon requests. Notices go back as addon messages under
//! [`wire::PREFIX`]; casts, kills and cooldowns go back as WoW's own spell packets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use among_rules::wire::{self, Notice, Request, To};
use among_rules::{
    Actions, Class, Command, CreatureId, Event, Game, Layout, MeetingReason, Phase, PlayerId,
    Position, Reject, SabotageDef, SabotageKind, Settings, Station, StepAction, TaskDef, TaskId,
    TaskKind, TaskStep, Vote,
};
use rand::RngCore;

use crate::announce;
use crate::look::Look;
use crate::world::creatures::{self, Shown};
use crate::world::guid::{Fixture, FixtureTemplate};
use crate::world::player::Life;
use crate::world::spells::{self, CastRequest, Target};
use crate::world::{combat, death, guid, meeting, objects, packets, player, quests, Map, Packet};

/// A kill's strike is shown only this close to it: to the killer, the victim and any witness.
const WITNESS_RANGE: f32 = 25.0;

/// Nodes this close stand on one spot, as overlapping models a click cannot tell apart.
const SAME_SPOT: f32 = 1.0;

/// How long the fallen may lie before their spirit is released for them (vmangos
/// `CORPSE_REPOP_TIME`, the Release Spirit popup's countdown).
const AUTO_RELEASE: Duration = Duration::from_secs(6 * 60);

/// How long a start is counted down in chat, by default.
pub const START_COUNTDOWN: Duration = Duration::from_secs(5);

/// The one match; cheap to clone. Lock order: the host, then the map.
#[derive(Clone)]
pub struct GameHost(Arc<Mutex<Inner>>);

struct Inner {
    game: Game,
    look: Look,
    map: Map,
    /// The buttons each player was last told are live.
    actions: HashMap<PlayerId, Actions>,
    /// Who was last sent the impostor's action bar; absent is the crewmate's bar a login sends.
    impostor_bars: HashMap<PlayerId, bool>,
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
    /// The gather nodes a crewmate picked, gone from the world until the lobby, with the create
    /// that puts each back.
    picked: Vec<(u64, Packet)>,
    /// The fallen and the ghosts, with how long each has lain unreleased; the rest are alive.
    fallen: HashMap<PlayerId, (Life, Duration)>,
    /// The corpses standing in the world, until a meeting or the lobby clears them.
    corpses: Vec<u64>,
    /// How long a start is counted down; zero starts on the press.
    countdown: Duration,
    /// A start counting down: who pressed it, and the time left.
    starting: Option<(PlayerId, Duration)>,
    /// Each open ballot's options by index, as its voter's menu lists them.
    ballots: HashMap<PlayerId, Vec<Vote>>,
    /// Each player's quest log as their client was last told it.
    quest_logs: HashMap<PlayerId, quests::Told>,
    /// The beacons standing over stations still to fix.
    beacons: std::collections::HashSet<Fixture>,
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
        spawn_fixtures(&layout, &map);
        Self(Arc::new(Mutex::new(Inner {
            game: Game::new(settings, layout),
            look,
            map,
            actions: HashMap::new(),
            impostor_bars: HashMap::new(),
            casts: HashMap::new(),
            shown: HashMap::new(),
            cooldowns: HashMap::new(),
            health: HashMap::new(),
            attacking: HashMap::new(),
            hunting: HashMap::new(),
            creature_at: HashMap::new(),
            picked: Vec::new(),
            fallen: HashMap::new(),
            corpses: Vec::new(),
            countdown: START_COUNTDOWN,
            starting: None,
            ballots: HashMap::new(),
            quest_logs: HashMap::new(),
            beacons: std::collections::HashSet::new(),
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
        let told = match inner.apply(Command::Join { player, class }) {
            Ok(events) => (events.iter()).any(|e| matches!(e, Event::HostChanged { .. })),
            Err(r) => {
                inner.notify(To::Player(player), &Notice::Refused(r.to_string()));
                false
            }
        };
        // The first in already heard it as the host change.
        if !told {
            let host = inner.game.host();
            inner.notify(To::Player(player), &Notice::Host(host));
        }
    }

    /// A character left the world.
    pub fn leave(&self, guid: u64) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        let _ = inner.apply(Command::Leave { player });
        inner.actions.remove(&player);
        inner.impostor_bars.remove(&player);
        inner.cooldowns.remove(&player);
        inner.casts.remove(&player);
        inner.health.remove(&player);
        inner.fallen.remove(&player);
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
            .and_then(|cmd| inner.run(cmd));
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
            .and_then(|cmd| inner.run(cmd));
        let map = inner.map.clone();
        match result {
            Err(r) => {
                tracing::debug!("game: {guid} cast {} refused: {r}", cast.spell);
                map.send(guid, spells::cast_failed(cast.spell, spells::FAILED_SILENT));
                let why = match (r, inner.cooldown_left(player, cast.spell)) {
                    (Reject::OnCooldown, Some(left)) => {
                        format!("Ready in {} s", left.as_secs_f32().ceil())
                    }
                    _ => sentence(r),
                };
                map.send(guid, spells::notification(&why));
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

    /// A `CMSG_GAMEOBJ_USE` on an object with no lock: the emergency button, a sabotage's
    /// station or its beacon, else a node: work its task step, or gather that node.
    pub fn use_object(&self, guid: u64, object: u64) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        let command = match Fixture::of_guid(object) {
            Some(fixture) => inner.fixture_command(player, fixture),
            None => inner.node_command(player, object),
        };
        let result = command.and_then(|cmd| inner.run(cmd));
        if let Err(r) = result {
            tracing::debug!("game: {guid} use of {object:#x} refused: {r}");
            // The button has no bar sweep, so its refusal says how long is left.
            let left = inner.game.emergency_cooldown();
            let why = match (r, Fixture::of_guid(object)) {
                (Reject::OnCooldown, Some(Fixture::Button)) if !left.is_zero() => {
                    format!("Ready in {} s", left.as_secs_f32().ceil())
                }
                _ => sentence(r),
            };
            inner.map.send(guid, spells::notification(&why));
        }
    }

    /// Whether party chat is open: before a game and at meetings, when the crew may talk.
    pub fn party_open(&self) -> bool {
        matches!(
            self.lock().game.phase(),
            among_rules::Phase::Lobby | among_rules::Phase::Meeting(_)
        )
    }

    /// Whether a match is running (anything but the lobby).
    pub fn in_game(&self) -> bool {
        *self.lock().game.phase() != among_rules::Phase::Lobby
    }

    /// Run the clock; the fallen who lay unreleased for [`AUTO_RELEASE`] are released.
    pub fn tick(&self, dt: Duration) {
        let mut inner = self.lock();
        inner.count_down(dt);
        let _ = inner.apply(Command::Tick { dt });
        let mut due = Vec::new();
        for (&player, (life, lain)) in &mut inner.fallen {
            if *life == Life::Dead {
                *lain += dt;
                if *lain >= AUTO_RELEASE {
                    due.push(player);
                }
            }
        }
        for player in due {
            inner.release(player);
        }
    }

    /// `.vote <name>` or `.vote skip` typed in chat; the reply line for the voter. With no name,
    /// the reply lists who may be voted for.
    pub fn vote_by_name(&self, guid: u64, who: Option<&str>) -> String {
        let mut inner = self.lock();
        let voter = PlayerId(guid);
        let candidates: Vec<(PlayerId, String)> = (inner.game.players())
            .filter(|(_, p)| p.connected && p.alive)
            .map(|(id, _)| (id, inner.name(id)))
            .collect();
        let vote = match who {
            None => {
                let names: Vec<&str> = candidates.iter().map(|(_, n)| n.as_str()).collect();
                return format!(
                    "Vote with .vote <name> or .vote skip. Still standing: {}.",
                    names.join(", ")
                );
            }
            Some(w) if w.eq_ignore_ascii_case("skip") => Vote::Skip,
            Some(w) => match candidates.iter().find(|(_, n)| n.eq_ignore_ascii_case(w)) {
                Some((id, _)) => Vote::Player(*id),
                None => return format!("Nobody standing is named {w}."),
            },
        };
        match inner.apply(Command::Vote { voter, vote }) {
            Ok(_) => match vote {
                Vote::Skip => "You voted to skip.".into(),
                Vote::Player(p) => format!("You voted for {}.", inner.name(p)),
            },
            Err(r) => format!("{}.", sentence(r)),
        }
    }

    /// A `CMSG_GOSSIP_SELECT_OPTION` on `menu`: on the voter's own ballot, the vote it names.
    pub fn choose(&self, guid: u64, menu: u64, option: u32) {
        let mut inner = self.lock();
        let voter = PlayerId(guid);
        let vote = (menu == guid)
            .then(|| inner.ballots.get(&voter))
            .flatten()
            .and_then(|votes| votes.get(option as usize).copied());
        let Some(vote) = vote else {
            return;
        };
        if let Err(r) = inner.apply(Command::Vote { voter, vote }) {
            inner.map.send(guid, spells::notification(&sentence(r)));
        }
    }

    /// Count a start down for `countdown` (zero: start on the press), as tests and quick
    /// runs set it.
    pub fn set_start_countdown(&self, countdown: Duration) {
        self.lock().countdown = countdown;
    }

    /// A `CMSG_REPOP_REQUEST`, the Release Spirit button: the fallen become ghosts where they
    /// fell, leaving their corpse. There is no graveyard run: a spirit stays with the crew.
    pub fn release_spirit(&self, guid: u64) {
        self.lock().release(PlayerId(guid));
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

    /// The answer to `CMSG_QUEST_QUERY`: a task's quest or the crew's, else none, as vmangos
    /// answers an unknown id with silence.
    pub fn quest_query(&self, quest_id: u32) -> Option<Packet> {
        let inner = self.lock();
        let layout = inner.game.layout();
        let text = if quest_id == quests::CREW_QUEST {
            quests::crew_text(inner.game.task_bar().1)
        } else {
            let task = quests::quest_task(quest_id)?;
            let def = layout.tasks.get(task.0)?;
            let need = |step| inner.game.gather_need(task, step);
            quests::task_text(task, def, &inner.look, &layout.creatures, &need)
        };
        Some(quests::template(quest_id, player::SPAWN.zone, &text))
    }

    /// The answer to `CMSG_GAMEOBJECT_QUERY` for template `entry`: a fixture's, a task node's, or
    /// a miss.
    pub fn gameobject_query(&self, entry: u32) -> Packet {
        if let Some(template) = FixtureTemplate::of_entry(entry) {
            return objects::query_response(entry, Some(&fixture_look(template)));
        }
        let inner = self.lock();
        let node = guid::entry_step(entry).and_then(|(task, step)| inner.look.node(task, step));
        objects::query_response(entry, node)
    }
}

/// Put every task node in the world for the whole session: a Cast step's one node where the step
/// is done, a Gather step's one at each of its spots.
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
            let spots = match &s.action {
                StepAction::Gather { nodes, .. } => nodes.as_slice(),
                _ => std::slice::from_ref(&s.at),
            };
            let entry = guid::node_entry(task, step);
            for (index, &at) in spots.iter().enumerate() {
                let g = guid::node(task, step, index);
                map.spawn(g, objects::create_node(g, entry, node, at, 0.0));
            }
        }
    }
}

/// Put the emergency button and every sabotage's stations in the world for the whole session.
fn spawn_fixtures(layout: &Layout, map: &Map) {
    let place = |fixture: Fixture, at: Position| {
        let g = fixture.guid();
        let look = fixture_look(fixture.template());
        let entry = fixture.template().entry();
        map.spawn(g, objects::create_node(g, entry, &look, at, 0.0));
    };
    place(Fixture::Button, layout.emergency_button);
    for def in &layout.sabotages {
        for (n, &at) in def.stations.iter().enumerate() {
            place(Fixture::Station(def.kind, n), at);
        }
    }
}

fn fixture_look(template: FixtureTemplate) -> crate::look::NodeLook {
    match template {
        FixtureTemplate::Button => crate::look::button(),
        FixtureTemplate::Station(kind) => crate::look::station(kind),
        FixtureTemplate::Beacon(kind) => crate::look::beacon(kind),
    }
}

/// `d` in whole milliseconds, as the wire carries it.
fn millis(d: Duration) -> u32 {
    d.as_millis().min(u32::MAX.into()) as u32
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
    /// A press's command: a start is counted down first, anything else applied now.
    fn run(&mut self, cmd: Command) -> Result<Vec<Event>, Reject> {
        let Command::Start { by, .. } = cmd else {
            return self.apply(cmd);
        };
        if self.countdown.is_zero() {
            return self.apply(cmd);
        }
        self.game.validate(&cmd)?;
        if self.starting.is_none() {
            self.starting = Some((by, self.countdown));
            let secs = self.countdown.as_secs();
            self.announce(&format!("{} starts the game: {secs}...", self.name(by)));
        }
        Ok(Vec::new())
    }

    /// Run a start's countdown by `dt`: each whole second is called in chat, and at zero the
    /// game starts, or the reason it cannot is told.
    fn count_down(&mut self, dt: Duration) {
        let Some((by, left)) = self.starting else {
            return;
        };
        let now = left.saturating_sub(dt);
        if !now.is_zero() {
            self.starting = Some((by, now));
            // A second's mark crossed: 4..., 3..., counting the seconds started in.
            let (was, is) = (left.as_secs_f32().ceil(), now.as_secs_f32().ceil());
            if is < was {
                self.announce(&format!("{is}..."));
                self.map.broadcast(&meeting::headline(&is.to_string()));
            }
            return;
        }
        self.starting = None;
        let seed = rand::thread_rng().next_u64();
        if let Err(r) = self.apply(Command::Start { by, seed }) {
            self.announce(&format!("The start is called off: {r}."));
        }
    }

    /// One system line to everyone in the world.
    fn announce(&self, text: &str) {
        self.map.broadcast(&packets::system_message(text));
    }

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
            self.show_quests(event);
        }
        self.refresh_actions();
        self.refresh_bars();
        self.refresh_beacons();
        self.refresh_cooldowns();
        self.refresh_health();
        Ok(events)
    }

    /// The world's side of an event: cast bars, launches and breaks, and kill strikes.
    fn show(&mut self, event: &Event) {
        match *event {
            Event::CastStarted { player, task, time } => {
                let p = self.game.player(player);
                let step =
                    (p.and_then(|p| p.tasks.iter().find(|t| t.task == task))).map_or(0, |t| t.step);
                let index = (p.and_then(|p| p.casting)).and_then(|c| c.node);
                let spell = self.look.node(task, step).map_or(spells::USE, |n| n.spell);
                let target = Target::GameObject(guid::node(task, step, index.unwrap_or(0)));
                self.start_cast(player, spell, time, target);
            }
            Event::GatherProgress {
                task, step, node, ..
            } => {
                // The picked herb or vein is gone for the whole crew until the lobby.
                let g = guid::node(task, step, node);
                if let Some(create) = self.map.withdraw(g) {
                    self.picked.push((g, create));
                }
            }
            Event::ReturnedToLobby => {
                for (g, create) in std::mem::take(&mut self.picked) {
                    self.map.spawn(g, create);
                }
                // Everyone walks the lobby alive, and no corpse is left.
                for g in std::mem::take(&mut self.corpses) {
                    self.map.despawn(g);
                }
                let fallen: Vec<PlayerId> = self.fallen.keys().copied().collect();
                for player in fallen {
                    self.revive(player);
                }
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
                    let go = spells::spell_go(player.0, spell, target);
                    self.map.broadcast_about(player.0, &go);
                }
            }
            Event::CastInterrupted { player } | Event::SpellCastInterrupted { player } => {
                if let Some((spell, _)) = self.casts.remove(&player) {
                    let failed = spells::spell_failed_other(player.0, spell);
                    self.map.broadcast_about(player.0, &failed);
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
                self.fall(victim);
            }
            Event::Slain { victim, .. } => self.fall(victim),
            Event::MeetingCalled {
                reason, discussion, ..
            } => {
                self.seat_the_living();
                let headline = match reason {
                    MeetingReason::Report { .. } => "Dead body reported!",
                    MeetingReason::Emergency => "Emergency meeting!",
                };
                self.map.broadcast(&meeting::headline(headline));
                self.map.broadcast(&meeting::countdown(millis(discussion)));
            }
            Event::VotingOpened { voting } => {
                self.map.broadcast(&meeting::headline("Vote!"));
                self.map.broadcast(&meeting::countdown(millis(voting)));
                let voters: Vec<PlayerId> = (self.game.players())
                    .filter(|(_, p)| p.connected && p.alive)
                    .map(|(id, _)| id)
                    .collect();
                for voter in voters {
                    self.open_ballot(voter);
                }
            }
            Event::VoteCast { voter } => {
                if self.ballots.remove(&voter).is_some() {
                    self.map.send(voter.0, meeting::ballot_closed());
                }
            }
            Event::VoteReturned { voter } => self.open_ballot(voter),
            Event::VotingClosed { .. } => {
                self.map.broadcast(&meeting::countdown_stop());
                for (voter, _) in std::mem::take(&mut self.ballots) {
                    self.map.send(voter.0, meeting::ballot_closed());
                }
            }
            // The ejected leave no body: their spirit walks on at once.
            Event::Ejected { player, .. } => self.become_ghost(player, None),
            Event::BodiesCleared => {
                for g in std::mem::take(&mut self.corpses) {
                    self.map.despawn(g);
                }
                // A meeting takes every body off the map, the unreleased with it.
                let lying: Vec<PlayerId> = (self.fallen.iter())
                    .filter(|(_, (life, _))| *life == Life::Dead)
                    .map(|(&p, _)| p)
                    .collect();
                for player in lying {
                    self.become_ghost(player, None);
                }
            }
            _ => {}
        }
        self.show_fight(event);
    }

    /// `victim` falls where they stand: health 0 for everyone, rooted, the Release Spirit popup up.
    fn fall(&mut self, victim: PlayerId) {
        if let Some(counter) = self.map.next_counter(victim.0) {
            self.map.send(victim.0, death::root(victim.0, counter));
        }
        let change = player::life_change(victim.0, Life::Dead, self.max_health(victim));
        self.map.set_life(victim.0, Life::Dead, change);
        self.health.remove(&victim);
        self.fallen.insert(victim, (Life::Dead, Duration::ZERO));
    }

    /// Release a fallen player's spirit: their corpse stays where they fell while their body is
    /// still on the map, and they rise as a ghost.
    fn release(&mut self, player: PlayerId) {
        if !matches!(self.fallen.get(&player), Some((Life::Dead, _))) {
            return;
        }
        let body = (self.game.bodies().iter())
            .find(|b| b.victim == player)
            .map(|b| b.position);
        self.become_ghost(player, body);
    }

    /// `player` rises as a ghost, as vmangos's `BuildPlayerRepop` raises one: the corpse (at
    /// `corpse`, if any), water walking, the Ghost aura's speeds, unrooted, health 1, and seen
    /// by ghosts alone.
    fn become_ghost(&mut self, player: PlayerId, corpse: Option<Position>) {
        let g = player.0;
        let was = self.fallen.get(&player).map_or(Life::Alive, |f| f.0);
        if was == Life::Ghost {
            return;
        }
        if let (Some(at), Some(c)) = (corpse, self.map.character(g)) {
            let facing = self.map.pose(g).map_or(0.0, |p| p.orientation);
            let corpse = guid::corpse(g);
            self.map
                .spawn(corpse, death::create_corpse(corpse, &c, at, facing));
            self.corpses.push(corpse);
        }
        let [run, swim] = death::GHOST_SPEEDS;
        self.force_moves(g, was == Life::Dead, true, run, swim);
        self.send_node_templates(g, Life::Ghost);
        let change = player::life_change(g, Life::Ghost, self.max_health(player));
        self.map.set_life(g, Life::Ghost, change);
        self.fallen.insert(player, (Life::Ghost, Duration::ZERO));
    }

    /// Back to life in the lobby: unrooted or back on land at living speeds, full health, seen
    /// by all.
    fn revive(&mut self, player: PlayerId) {
        let Some((was, _)) = self.fallen.remove(&player) else {
            return;
        };
        let g = player.0;
        let [run, swim] = death::LIVING_SPEEDS;
        self.force_moves(g, was == Life::Dead, false, run, swim);
        if was == Life::Ghost {
            self.send_node_templates(g, Life::Alive);
        }
        let change = player::life_change(g, Life::Alive, self.max_health(player));
        self.map.set_life(g, Life::Alive, change);
        // The next refresh tells everyone the health the rules hold.
        self.health.remove(&player);
    }

    /// The acked movement changes of a death or a revival: an unroot if `unroot`, water or land
    /// walking, and the run and swim speeds, which later creates carry.
    fn force_moves(&self, g: u64, unroot: bool, water_walk: bool, run: f32, swim: f32) {
        let next = || self.map.next_counter(g).unwrap_or(0);
        if unroot {
            self.map.send(g, death::unroot(g, next()));
        }
        self.map.send(g, death::water_walk(g, next(), water_walk));
        self.map.send(g, packets::force_run_speed(g, next(), run));
        self.map.send(g, death::force_swim_speed(g, next(), swim));
        self.map.set_run_speed(g, run);
        self.map.set_swim_speed(g, swim);
    }

    /// Seat every living player in a ring around the meeting table (the emergency button),
    /// facing it, as Among Us gathers the crew at the table.
    fn seat_the_living(&mut self) {
        let living: Vec<PlayerId> = (self.game.players())
            .filter(|(_, p)| p.connected && p.alive)
            .map(|(id, _)| id)
            .collect();
        let table = self.game.layout().emergency_button;
        for (player, (at, facing)) in living.iter().zip(meeting::seats(table, living.len())) {
            self.map.teleport(player.0, at, facing);
            let _ = self.game.apply(Command::Move {
                player: *player,
                position: at,
            });
        }
    }

    /// Open `voter`'s ballot: a menu with every living player and a skip.
    fn open_ballot(&mut self, voter: PlayerId) {
        let mut votes: Vec<Vote> = (self.game.players())
            .filter(|(_, p)| p.connected && p.alive)
            .map(|(id, _)| Vote::Player(id))
            .collect();
        votes.push(Vote::Skip);
        let lines: Vec<String> = (votes.iter())
            .map(|v| match v {
                Vote::Player(p) => format!("Vote: {}", self.name(*p)),
                Vote::Skip => "Skip vote".into(),
            })
            .collect();
        self.map.send(voter.0, meeting::ballot(voter.0, &lines));
        self.ballots.insert(voter, votes);
    }

    /// Every task node's template as `life` sees it, replacing what `g`'s client cached: a ghost's
    /// nodes have no lock, a living player's their herb or mining lock again.
    fn send_node_templates(&self, g: u64, life: Life) {
        for (t, def) in self.game.layout().tasks.iter().enumerate() {
            for step in 0..def.steps.len() {
                let task = TaskId(t);
                let (Some(node), true) = (self.look.node(task, step), guid::fits(def.steps.len()))
                else {
                    continue;
                };
                let entry = guid::node_entry(task, step);
                let template = match life {
                    Life::Ghost => objects::ghost_template(entry, node),
                    _ => objects::query_response(entry, Some(node)),
                };
                self.map.send(g, template);
            }
        }
    }

    /// How long until `player` may press `spell` again, for a button that cools down.
    fn cooldown_left(&self, player: PlayerId, spell: u32) -> Option<Duration> {
        let p = self.game.player(player)?;
        let left = match spell {
            spells::KILL => p.kill_cooldown,
            spells::SABOTAGE => self.game.sabotage_cooldown(),
            s if s == spells::class_spell(p.class) => p.spell_cooldown,
            _ => return None,
        };
        (!left.is_zero()).then_some(left)
    }

    /// The player's character name, as chat shows it.
    fn name(&self, player: PlayerId) -> String {
        (self.map.character(player.0)).map_or_else(|| format!("Player {}", player.0), |c| c.name)
    }

    fn max_health(&self, player: PlayerId) -> u32 {
        let class = self.game.player(player).map_or(Class::Warrior, |p| p.class);
        self.game.settings().kits.get(class).health
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
    /// class's pool at join and after a meeting. The dead's is shown by their fall.
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

    /// Show `player`'s cast of `spell` to everyone who sees them: a bar of `time`, at `target`.
    fn start_cast(&mut self, player: PlayerId, spell: u32, time: Duration, target: Target) {
        let ms = time.as_millis().min(u32::MAX.into()) as u32;
        let start = spells::spell_start(player.0, spell, ms, target);
        self.map.broadcast_about(player.0, &start);
        self.casts.insert(player, (spell, target));
        self.shown.insert(player, spell);
    }

    fn is_gather_step(&self, task: TaskId, step: usize) -> bool {
        (self.game.layout().tasks.get(task.0)).is_some_and(|t| {
            matches!(
                t.steps.get(step).map(|s| &s.action),
                Some(StepAction::Gather { .. })
            )
        })
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
            spells::REPORT => Request::Report,
            spells::SKIP_VOTE => Request::Vote(Vote::Skip),
            spells::SABOTAGE => match g.sabotage_target(player) {
                Some(kind) => Request::Sabotage(kind),
                None => return Err(no_sabotage(g, player)),
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

    /// The horn and the emergency button: start the game from the lobby, go back to it after one,
    /// and call a meeting in between.
    fn horn(&self) -> Request {
        match self.game.phase() {
            Phase::Lobby => Request::Start,
            Phase::GameOver { .. } => Request::Lobby,
            _ => Request::Emergency,
        }
    }

    /// A right-click on a fixture: the button is the horn; a station, or the beacon over it,
    /// fixes (or holds) that station while its sabotage runs.
    fn fixture_command(&self, player: PlayerId, fixture: Fixture) -> Result<Command, Reject> {
        match fixture {
            Fixture::Button => self.command(player, self.horn()),
            Fixture::Station(kind, station) | Fixture::Beacon(kind, station) => {
                match self.game.sabotage() {
                    Some(s) if s.kind == kind => Ok(Command::Fix { player, station }),
                    _ => Err(Reject::NoSabotage),
                }
            }
        }
    }

    /// Show a beacon over each station of the running sabotage still to fix, and take down the
    /// ones fixed or ended.
    fn refresh_beacons(&mut self) {
        let want: std::collections::HashSet<Fixture> = (self.game.sabotage().into_iter())
            .flat_map(|s| {
                (s.stations.iter().enumerate())
                    .filter(|(_, st)| **st != Station::Fixed)
                    .map(move |(n, _)| Fixture::Beacon(s.kind, n))
            })
            .collect();
        for gone in self.beacons.difference(&want) {
            self.map.despawn(gone.guid());
        }
        for &new in want.difference(&self.beacons) {
            let Fixture::Beacon(kind, n) = new else {
                continue;
            };
            let Some(at) = (self.game.layout().sabotages.iter())
                .find(|d| d.kind == kind)
                .and_then(|d| d.stations.get(n).copied())
            else {
                continue;
            };
            let g = new.guid();
            let look = fixture_look(new.template());
            let create = objects::create_node(g, new.template().entry(), &look, at, 0.0);
            self.map.spawn(g, create);
        }
        self.beacons = want;
    }

    /// Work at a task node: the step it stands for, if that is the player's current step; at a
    /// Gather step's node, gather that one.
    fn node_command(&self, player: PlayerId, object: u64) -> Result<Command, Reject> {
        let (task, step, node) = guid::node_step(object).ok_or(Reject::InvalidTarget)?;
        let exact = self.exact_node_command(player, task, step, node);
        if exact
            .as_ref()
            .is_ok_and(|cmd| self.game.validate(cmd).is_ok())
        {
            return exact;
        }
        // Two tasks may share a spot (one surveyed campfire): their nodes overlap, so a click
        // may land on the other task's; work this player's own there instead.
        self.node_spot(task, step, node)
            .and_then(|at| self.own_node_at(player, at))
            .map_or(exact, Ok)
    }

    /// Where node `node` of `task`'s step `step` stands.
    fn node_spot(&self, task: TaskId, step: usize, node: usize) -> Option<Position> {
        let s = self.game.layout().tasks.get(task.0)?.steps.get(step)?;
        match &s.action {
            StepAction::Gather { nodes, .. } => nodes.get(node).copied(),
            _ => Some(s.at),
        }
    }

    /// A legal command working one of `player`'s current steps at a node standing on `at`.
    fn own_node_at(&self, player: PlayerId, at: Position) -> Option<Command> {
        let p = self.game.player(player)?;
        (p.tasks.iter())
            .flat_map(|t| {
                let s = self.game.layout().tasks[t.task.0].steps.get(t.step);
                let task = t.task;
                let spots: Vec<(Position, Command)> = match s.map(|s| (&s.action, s.at)) {
                    Some((StepAction::Gather { nodes, .. }, _)) => (nodes.iter().enumerate())
                        .map(|(node, &n)| (n, Command::GatherNode { player, task, node }))
                        .collect(),
                    Some((StepAction::Cast(_), spot)) => {
                        vec![(spot, Command::UseTask { player, task })]
                    }
                    _ => Vec::new(),
                };
                spots
            })
            .find(|(spot, cmd)| {
                among_rules::within(*spot, at, SAME_SPOT) && self.game.validate(cmd).is_ok()
            })
            .map(|(_, cmd)| cmd)
    }

    /// The command for exactly the node clicked: its step, if that is the player's current one.
    fn exact_node_command(
        &self,
        player: PlayerId,
        task: TaskId,
        step: usize,
        node: usize,
    ) -> Result<Command, Reject> {
        let cmd = if self.is_gather_step(task, step) {
            Command::GatherNode { player, task, node }
        } else {
            Command::UseTask { player, task }
        };
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

    /// Send the action bar again to each player whose role changed it: Kill and Sabotage arrive
    /// with the impostor role and go at the lobby, where roles end.
    fn refresh_bars(&mut self) {
        let changed: Vec<(PlayerId, Class, bool)> = (self.game.players())
            .filter(|(_, p)| p.connected)
            .map(|(id, p)| (id, p.class, p.is_impostor()))
            .filter(|(id, _, impostor)| {
                self.impostor_bars.get(id).copied().unwrap_or(false) != *impostor
            })
            .collect();
        for (id, class, impostor) in changed {
            self.impostor_bars.insert(id, impostor);
            self.map.send(id.0, spells::action_buttons(class, impostor));
        }
    }

    /// Start the cooldown sweep of every button whose cooldown began since the last refresh:
    /// kill, sabotage and the class spell. A client's sweep can grow but not shrink,
    /// so only a cooldown that rose is sent.
    fn refresh_cooldowns(&mut self) {
        let playing = *self.game.phase() == Phase::Playing;
        let g = &self.game;
        let now: Vec<(PlayerId, Vec<(u32, Duration)>)> = (g.players())
            .filter(|(_, p)| p.connected)
            .map(|(id, p)| {
                let mut cds = vec![(spells::class_spell(p.class), p.spell_cooldown)];
                if playing && p.is_impostor() {
                    cds.push((spells::KILL, p.kill_cooldown));
                    if g.sabotage().is_none() {
                        cds.push((spells::SABOTAGE, g.sabotage_cooldown()));
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

    /// Each recipient gets the line under its own guid, which its name query always answers, and
    /// the notice in words as a system line in its chat.
    fn notify(&self, to: To, notice: &Notice) {
        let payload = notice.encode();
        let send = |guid: u64| {
            self.map
                .send(guid, packets::addon_message(guid, wire::PREFIX, &payload));
            let name = |p: PlayerId| self.name(p);
            if let Some(text) = announce::line(notice, PlayerId(guid), &self.game, &name) {
                self.map.send(guid, packets::system_message(&text));
            }
            if let Some(text) = announce::headline(notice, &name) {
                self.map.send(guid, meeting::headline(&text));
            }
        };
        match to {
            To::Player(p) => send(p.0),
            To::All => self.map.guids().into_iter().for_each(send),
        }
    }
}

/// The quest log: each player's tasks, and the crew's count, kept in WoW's own quest log.
impl Inner {
    /// Refresh the logs an event changed: the owner's on its task events, everyone's on the
    /// crew's count and on the way back to the lobby, which empties them.
    fn show_quests(&mut self, event: &Event) {
        match *event {
            Event::TasksAssigned { player, .. }
            | Event::TaskProgress { player, .. }
            | Event::GatherProgress { player, .. }
            | Event::SlayProgress { player, .. }
            | Event::TaskCompleted { player, .. } => self.refresh_quest_log(player),
            Event::TaskBar { .. } | Event::ReturnedToLobby => {
                let players: std::collections::HashSet<PlayerId> =
                    (self.game.players().map(|(id, _)| id))
                        .chain(self.quest_logs.keys().copied())
                        .collect();
                for player in players {
                    self.refresh_quest_log(player);
                }
            }
            _ => {}
        }
    }

    /// Send `player` the slots of its log that changed, after a fresh crew template when the
    /// crew's total moved (an unasked template replaces the cached one); one who left is forgotten.
    fn refresh_quest_log(&mut self, player: PlayerId) {
        let Some(p) = self.game.player(player).filter(|p| p.connected) else {
            self.quest_logs.remove(&player);
            return;
        };
        let (done, total) = self.game.task_bar();
        let need = |task, step| self.game.gather_need(task, step);
        let now = quests::player_log(&p.tasks, &self.game.layout().tasks, (done, total), &need);
        // A task's share of its spots changes from game to game: send its text again when it does.
        let layout = self.game.layout();
        let texts: Vec<(u32, quests::QuestText)> = (p.tasks.iter())
            .filter_map(|t| {
                let def = layout.tasks.get(t.task.0)?;
                let need = |step| self.game.gather_need(t.task, step);
                let text = quests::task_text(t.task, def, &self.look, &layout.creatures, &need);
                Some((quests::task_quest(t.task), text))
            })
            .collect();
        let told = self.quest_logs.entry(player).or_default();
        for (quest, text) in texts {
            if told.texts.get(&quest) != Some(&text.objectives) {
                let template = quests::template(quest, player::SPAWN.zone, &text);
                self.map.send(player.0, template);
                told.texts.insert(quest, text.objectives);
            }
        }
        if now[0].0 == quests::CREW_QUEST && told.crew_total != Some(total) {
            let text = quests::crew_text(total);
            let crew = quests::template(quests::CREW_QUEST, player::SPAWN.zone, &text);
            self.map.send(player.0, crew);
            told.crew_total = Some(total);
        }
        if let Some(update) = quests::log_update(player.0, &told.log, &now) {
            self.map.send(player.0, update);
            told.log = now;
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
    // Gathering and mining take 5 s, as Herb Gathering does; crafting 3 s.
    let herb = |dx, dy, name, display| {
        (
            TaskStep::pick(at(dx, dy), 5.0),
            NodeLook::herb(name, display),
        )
    };
    let vein = |dx, dy| {
        let node = NodeLook::mine("Copper Vein", look::COPPER_VEIN);
        (TaskStep::pick(at(dx, dy), 5.0), node)
    };
    let craft = |dx, dy, name, display, spell| {
        let node = NodeLook::station(name, display, spell);
        (TaskStep::cast(at(dx, dy), 3.0), node)
    };
    // Peacebloom grows at 7 spots 5-6 yd apart around the spawn's east side; 5 are picked.
    let meadow = [
        (10.0, 0.0),
        (14.0, -4.0),
        (13.0, 5.0),
        (18.0, 2.0),
        (12.0, -10.0),
        (6.0, -6.0),
        (5.0, 5.0),
    ];
    let meadow: Vec<Position> = meadow.iter().map(|&(dx, dy)| at(dx, dy)).collect();
    let gather = TaskStep {
        at: meadow[0],
        action: StepAction::Gather {
            nodes: meadow,
            cast: Duration::from_secs(5),
            count: 5,
        },
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
            vec![(gather, Some(NodeLook::herb("Peacebloom", look::PEACEBLOOM)))],
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
        hosted_on(settings, placeholder_layout(), n)
    }

    /// [`hosted`] on `layout`, dressed as the placeholder is.
    fn hosted_on(
        settings: Settings,
        layout: Layout,
        n: u64,
    ) -> (GameHost, Vec<UnboundedReceiver<Packet>>) {
        let map = Map::default();
        let host = GameHost::new(settings, layout, placeholder_look(), map.clone());
        // Games start on the press here; the countdown has its own test.
        host.set_start_countdown(Duration::ZERO);
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
            // The system lines in words ride beside each notice.
            if !m.is_addon() {
                continue;
            }
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

    /// The red error text `seen` holds.
    fn notifications(seen: Vec<ServerPacket>) -> Vec<String> {
        (seen.into_iter())
            .filter_map(|p| match p {
                ServerPacket::Notification { text } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_the_host_starts_the_game_at_the_button() {
        let (host, mut inboxes) = hosted(quick(), 3);
        drain(&mut inboxes);
        host.use_object(2, Fixture::Button.guid());
        assert_eq!(
            notifications(received(&mut inboxes[1])),
            ["Only the host can do that"]
        );
        assert_eq!(*host.lock().game.phase(), Phase::Lobby);
        host.use_object(1, Fixture::Button.guid());
        assert!(matches!(host.lock().game.phase(), Phase::Playing));
    }

    #[test]
    fn a_cooling_button_says_how_long_is_left() {
        let (host, mut inboxes) = hosted(quick(), 3);
        host.use_object(1, Fixture::Button.guid());
        host.moved(2, placeholder_layout().emergency_button);
        drain(&mut inboxes);
        host.use_object(2, Fixture::Button.guid());
        let left = quick().emergency_cooldown.as_secs_f32().ceil();
        assert_eq!(
            notifications(received(&mut inboxes[1])),
            [format!("Ready in {left} s")]
        );
    }

    /// The action bars `seen` replaced the whole bar with, as the client reads them.
    fn bars(seen: &[ServerPacket]) -> Vec<Vec<u32>> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::ActionButtons { buttons } => {
                    Some(buttons.iter().map(|b| b.action).collect())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn kill_and_sabotage_reach_only_the_impostors_bar_and_leave_at_the_lobby() {
        let (host, mut inboxes) = hosted(quick(), 3);
        drain(&mut inboxes);
        host.request(1, Request::Start);
        let (imp, crew) = roles(&host);
        let warrior = |impostor| spells::main_bar(Class::Warrior, impostor);
        let sent =
            |bar: [u32; spells::BAR_SLOTS]| bar.into_iter().filter(|&s| s != 0).collect::<Vec<_>>();
        assert_eq!(
            bars(&received(&mut inboxes[imp as usize - 1])),
            [sent(warrior(true))]
        );
        for c in &crew {
            let seen = received(&mut inboxes[*c as usize - 1]);
            assert!(bars(&seen).is_empty(), "crewmate {c} keeps the login bar");
        }

        host.request(imp, Request::Kill);
        assert!(matches!(host.lock().game.phase(), Phase::GameOver { .. }));
        drain(&mut inboxes);
        host.request(1, Request::Lobby);
        assert_eq!(
            bars(&received(&mut inboxes[imp as usize - 1])),
            [sent(warrior(false))]
        );
        for c in &crew {
            assert!(bars(&received(&mut inboxes[*c as usize - 1])).is_empty());
        }
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
            let layout = inner.game.layout();
            // A task begun at one node: a station's cast, or a lone herb or vein to pick.
            let t = *(p.tasks.iter())
                .find(|t| match &layout.tasks[t.task.0].steps[0].action {
                    StepAction::Cast(_) => true,
                    StepAction::Gather { nodes, .. } => nodes.len() == 1,
                    StepAction::Slay { .. } => false,
                })
                .unwrap();
            let at = inner.game.layout().tasks[t.task.0].steps[t.step].at;
            (t.task, at, inner.look.node(t.task, t.step).unwrap().clone())
        };
        host.moved(c, at);
        drain(&mut inboxes);
        // A right-click on the node: shown as the node's own cast.
        host.use_object(c, guid::node(task, 0, 0));
        for seen in [received(&mut inboxes[me]), received(&mut inboxes[other])] {
            let start = seen.iter().find_map(|p| match p {
                ServerPacket::SpellStart(s) => Some(s.clone()),
                _ => None,
            });
            let start = start.expect("a cast bar");
            assert_eq!((start.caster, start.spell_id), (c, node.spell));
            assert_eq!(start.targets.go_target, Some(guid::node(task, 0, 0)));
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
            Target::GameObject(guid::node(task, 0, 0)),
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
            Target::GameObject(guid::node(task, 0, 0)),
        );
        let seen = received(&mut inboxes[me]);
        let names: Vec<String> = seen.iter().map(|p| p.name()).collect();
        assert!(
            seen.iter()
                .any(|p| matches!(p, ServerPacket::Notification { .. })),
            "{names:?}"
        );
    }

    /// Guids destroyed in `seen`.
    fn destroyed(seen: &[ServerPacket]) -> Vec<u64> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::DestroyObject { guid } => Some(*guid),
                _ => None,
            })
            .collect()
    }

    /// Guids created in `seen`.
    fn created(seen: &[ServerPacket]) -> Vec<u64> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::UpdateObject { objects } => Some(objects),
                _ => None,
            })
            .flatten()
            .filter_map(|o| match o {
                benilla_protocol::messages::Object::Create { guid, .. } => Some(*guid),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_gather_spot_is_a_node_and_a_picked_one_vanishes_for_everyone() {
        let map = Map::default();
        let (task, nodes) = (placeholder_layout().tasks.iter().enumerate())
            .find_map(|(i, t)| match &t.steps[0].action {
                StepAction::Gather { nodes, .. } => Some((TaskId(i), nodes.clone())),
                _ => None,
            })
            .expect("the placeholder gathers");
        let _host = GameHost::new(
            quick(),
            placeholder_layout(),
            placeholder_look(),
            map.clone(),
        );
        let (tx, mut rx) = unbounded_channel();
        let character = CharacterRecord {
            guid: 9,
            name: "P9".into(),
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
        let seen = created(&received(&mut rx));
        for i in 0..nodes.len() {
            assert!(seen.contains(&guid::node(task, 0, i)), "node {i} stands");
        }

        // Both common tasks dealt, so everyone has the gathering one.
        let settings = Settings {
            common_tasks: 2,
            ..quick()
        };
        let (host, mut inboxes) = hosted(settings, 3);
        host.request(1, Request::Start);
        let (imp, crew) = roles(&host);
        let c = crew[0];
        let me = c as usize - 1;
        let node = guid::node(task, 0, 2);
        host.moved(c, nodes[2]);
        drain(&mut inboxes);
        // Right-clicking a herb casts Herb Gathering at it; the bar names that node.
        cast(
            &host,
            c,
            crate::look::HERB_GATHERING,
            Target::GameObject(node),
        );
        let start = received(&mut inboxes[(me + 1) % 3])
            .into_iter()
            .find_map(|p| match p {
                ServerPacket::SpellStart(s) => Some(s),
                _ => None,
            })
            .expect("a cast bar");
        assert_eq!(start.targets.go_target, Some(node));
        host.tick(Duration::from_secs(6));
        for (i, inbox) in inboxes.iter_mut().enumerate() {
            assert_eq!(destroyed(&received(inbox)), [node], "{i} still sees it");
        }
        // Picked again: refused, the reason in red.
        cast(
            &host,
            c,
            crate::look::HERB_GATHERING,
            Target::GameObject(node),
        );
        assert!(received(&mut inboxes[me]).iter().any(|p| matches!(p,
            ServerPacket::Notification { text } if text == "Already gathered here")));

        // The impostor ends it; back in the lobby the herb grows again for everyone.
        host.request(imp, Request::Kill);
        assert!(matches!(host.lock().game.phase(), Phase::GameOver { .. }));
        drain(&mut inboxes);
        host.request(1, Request::Lobby);
        for inbox in &mut inboxes {
            assert!(created(&received(inbox)).contains(&node));
        }
    }

    #[test]
    fn the_button_and_every_station_stand_in_the_world() {
        let (host, mut inboxes) = hosted(quick(), 1);
        let seen = created(&received(&mut inboxes[0]));
        let layout = placeholder_layout();
        assert!(seen.contains(&Fixture::Button.guid()));
        for def in &layout.sabotages {
            for n in 0..def.stations.len() {
                assert!(
                    seen.contains(&Fixture::Station(def.kind, n).guid()),
                    "{def:?}"
                );
            }
        }
        let template = |t: FixtureTemplate| {
            let p = host.gameobject_query(t.entry());
            let (ServerPacket::GameObjectQueryResponse { info, .. }, _) =
                parse_server_with_tail(p.opcode, &p.body).unwrap()
            else {
                panic!("not a template");
            };
            let info = info.expect("found");
            (info.name, info.type_id, info.data[0])
        };
        assert_eq!(
            template(FixtureTemplate::Button),
            ("Emergency Button".into(), 10, 0),
            "a lockless goober, used with a right-click"
        );
        assert_eq!(
            template(FixtureTemplate::Station(SabotageKind::Reactor)).0,
            "Arcane Reactor"
        );
    }

    #[test]
    fn the_button_is_the_horn() {
        let (host, mut inboxes) = hosted(quick(), 3);
        drain(&mut inboxes);
        host.use_object(1, Fixture::Button.guid());
        assert!(matches!(host.lock().game.phase(), Phase::Playing));
        // At the button, once it cools: a meeting.
        host.tick(quick().emergency_cooldown);
        host.moved(2, placeholder_layout().emergency_button);
        drain(&mut inboxes);
        host.use_object(2, Fixture::Button.guid());
        let said: Vec<String> = (received(&mut inboxes[1]).into_iter())
            .filter_map(|p| match p {
                ServerPacket::Notification { text } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            matches!(host.lock().game.phase(), Phase::Meeting(_)),
            "{said:?}"
        );
    }

    #[test]
    fn a_sabotage_raises_a_beacon_over_each_station_until_it_is_fixed() {
        let settings = Settings {
            sabotage_cooldown: Duration::ZERO,
            ..quick()
        };
        let (host, mut inboxes) = hosted(settings, 3);
        host.request(1, Request::Start);
        let (imp, crew) = roles(&host);
        let lamp = placeholder_layout().sabotages[0].clone();
        assert_eq!(lamp.kind, SabotageKind::Lights);
        let station = Fixture::Station(SabotageKind::Lights, 0);
        let beacon = Fixture::Beacon(SabotageKind::Lights, 0);
        // No sabotage yet: nothing to fix.
        host.moved(crew[0], lamp.stations[0]);
        drain(&mut inboxes);
        host.use_object(crew[0], station.guid());
        assert!(received(&mut inboxes[crew[0] as usize - 1]).iter().any(
            |p| matches!(p, ServerPacket::Notification { text } if text.contains("sabotage"))
        ));

        host.request(imp, Request::Sabotage(SabotageKind::Lights));
        assert!(created(&received(&mut inboxes[crew[1] as usize - 1])).contains(&beacon.guid()));
        // The beacon is the station's handle too.
        host.use_object(crew[0], beacon.guid());
        assert!(
            host.lock().game.sabotage().is_none(),
            "the lights are fixed"
        );
        assert!(destroyed(&received(&mut inboxes[crew[1] as usize - 1])).contains(&beacon.guid()));
        assert!(
            !destroyed(&received(&mut inboxes[crew[0] as usize - 1])).contains(&station.guid()),
            "the station itself stays"
        );
    }

    #[test]
    fn a_node_on_a_shared_spot_works_for_whichever_task_is_yours() {
        // Every task that starts at one node (a station's cast, a lone herb or vein) starts it at
        // one spot, as two tasks sharing one surveyed campfire do.
        let mut layout = placeholder_layout();
        let spot = layout.tasks[1].steps[0].at;
        for t in &mut layout.tasks {
            match &mut t.steps[0].action {
                StepAction::Cast(_) => t.steps[0].at = spot,
                StepAction::Gather { nodes, .. } if nodes.len() == 1 => {
                    nodes[0] = spot;
                    t.steps[0].at = spot;
                }
                _ => {}
            }
        }
        let shared: Vec<TaskId> = (layout.tasks.iter().enumerate())
            .filter(|(_, t)| t.steps[0].at == spot)
            .map(|(i, _)| TaskId(i))
            .collect();
        let (host, mut inboxes) = hosted_on(quick(), layout, 3);
        host.request(1, Request::Start);
        let c = roles(&host).1[0];
        let me = c as usize - 1;
        let mine: Vec<TaskId> = (host.lock().game.player(PlayerId(c)).unwrap().tasks.iter())
            .map(|t| t.task)
            .collect();
        let theirs = *shared.iter().find(|t| !mine.contains(t)).unwrap();
        host.moved(c, spot);
        drain(&mut inboxes);
        // A right-click lands on another task's node at the spot: this player's own is worked.
        host.use_object(c, guid::node(theirs, 0, 0));
        let start = received(&mut inboxes[me])
            .into_iter()
            .find_map(|p| match p {
                ServerPacket::SpellStart(s) => Some(s),
                _ => None,
            })
            .expect("a cast bar");
        let worked = guid::node_step(start.targets.go_target.unwrap()).unwrap().0;
        assert!(
            mine.contains(&worked) && shared.contains(&worked),
            "{worked:?}"
        );
    }

    /// The descriptor changes `seen` gives player `guid`, in order.
    fn changes_of(
        seen: &[ServerPacket],
        guid: u64,
    ) -> Vec<benilla_protocol::messages::ObjectFields> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::UpdateObject { objects } => Some(objects),
                _ => None,
            })
            .flatten()
            .filter_map(|o| match o {
                benilla_protocol::messages::Object::Values { guid: g, mask } if *g == guid => {
                    Some(mask.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The acked movement modes `seen` sends `guid`: (mode, apply).
    fn modes(seen: &[ServerPacket], guid: u64) -> Vec<(MoveMode, bool)> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::MoveMode {
                    guid: g,
                    mode,
                    apply,
                    ..
                } if *g == guid => Some((*mode, *apply)),
                _ => None,
            })
            .collect()
    }

    /// The run speeds `seen` forces on its reader.
    fn run_speeds(seen: &[ServerPacket]) -> Vec<f32> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::ForceSpeedChange {
                    kind: benilla_protocol::messages::SpeedKind::Run,
                    speed,
                    ..
                } => Some(*speed),
                _ => None,
            })
            .collect()
    }

    use benilla_protocol::messages::MoveMode;

    /// Four players at the spawn, one walked off out of witness range; returns the host, the
    /// inboxes, the impostor and the crew (the far one last).
    fn four_with_one_far() -> (GameHost, Vec<UnboundedReceiver<Packet>>, u64, Vec<u64>) {
        let (host, mut inboxes) = hosted(
            Settings {
                min_players: 4,
                ..quick()
            },
            4,
        );
        host.request(1, Request::Start);
        let (imp, crew) = roles(&host);
        host.moved(crew[2], [-8900.0, -132.0, 83.5]);
        drain(&mut inboxes);
        (host, inboxes, imp, crew)
    }

    #[test]
    fn a_victim_falls_for_everyone_and_rises_a_ghost_only_ghosts_see_beside_its_corpse() {
        let (host, mut inboxes, imp, crew) = four_with_one_far();
        let (victim, reporter, far) = (crew[0], crew[1], crew[2]);
        let inbox = |g: u64| g as usize - 1;
        let fell_at = host.lock().map.position(victim).unwrap();
        cast(&host, imp, spells::KILL, Target::Unit(victim));

        // Health 0 reaches everyone, near or far: the body lies there for all to find.
        for g in [imp, victim, reporter, far] {
            let seen = received(&mut inboxes[inbox(g)]);
            assert_eq!(
                health_of(&seen, victim).last(),
                Some(&0),
                "{g} sees it fall"
            );
            if g == victim {
                assert_eq!(modes(&seen, victim), [(MoveMode::Root, true)], "rooted");
                let fell = changes_of(&seen, victim);
                assert!(fell.iter().any(|f| f.player_release_timer_running()));
            }
        }

        // Release Spirit: a corpse where they fell, and a ghost the living no longer see.
        host.release_spirit(victim);
        let corpse = guid::corpse(victim);
        for g in [imp, victim, reporter, far] {
            let seen = received(&mut inboxes[inbox(g)]);
            assert!(created(&seen).contains(&corpse), "{g} sees the corpse");
            let gone = destroyed(&seen).contains(&victim);
            assert_eq!(
                gone,
                g != victim,
                "{g}: only the living lose sight of the ghost"
            );
            if g == victim {
                let ghost = changes_of(&seen, victim);
                assert!(ghost.iter().any(|f| f.player_is_ghost()), "flagged a ghost");
                assert!(ghost.iter().all(|f| !f.unit_is_dead()), "on 1 health");
                let m = modes(&seen, victim);
                assert!(
                    m.contains(&(MoveMode::Root, false))
                        && m.contains(&(MoveMode::WaterWalk, true))
                );
                assert_eq!(run_speeds(&seen), [death::GHOST_SPEEDS[0]]);
            }
        }
        let standing = host.lock().map.position(victim);
        assert!(
            standing.is_some_and(|at| among_rules::within(at, fell_at, 1.0)),
            "no graveyard run: {standing:?} {fell_at:?}"
        );
        // A second press changes nothing.
        host.release_spirit(victim);
        assert!(received(&mut inboxes[inbox(reporter)]).is_empty());

        // The ghost's steps and casts reach no living client.
        let step = vec![0u8; 28];
        host.lock().map.relay_move(
            victim,
            benilla_protocol::messages::opcode::MSG_MOVE_HEARTBEAT,
            &step,
            None,
        );
        assert!(received(&mut inboxes[inbox(reporter)]).is_empty());

        // The living still find and report the body; the meeting clears the corpse for all.
        host.request(reporter, Request::Report);
        assert!(matches!(host.lock().game.phase(), Phase::Meeting(_)));
        for g in [imp, victim, reporter, far] {
            assert!(destroyed(&received(&mut inboxes[inbox(g)])).contains(&corpse));
        }
    }

    #[test]
    fn a_meeting_raises_the_unreleased_and_the_lobby_brings_everyone_back() {
        let (host, mut inboxes, imp, crew) = four_with_one_far();
        let (victim, reporter, far) = (crew[0], crew[1], crew[2]);
        let inbox = |g: u64| g as usize - 1;
        cast(&host, imp, spells::KILL, Target::Unit(victim));
        drain(&mut inboxes);

        // Reported before release: the body goes with the meeting, no corpse left behind.
        host.request(reporter, Request::Report);
        let seen = received(&mut inboxes[inbox(reporter)]);
        assert!(
            destroyed(&seen).contains(&victim),
            "the unreleased rise as ghosts"
        );
        assert!(!created(&seen).contains(&guid::corpse(victim)));
        let mine = received(&mut inboxes[inbox(victim)]);
        assert!(changes_of(&mine, victim)
            .iter()
            .any(|f| f.player_is_ghost()));

        // The living vote the impostor out: ejected, a ghost too, and the crew wins.
        host.tick(Duration::from_secs(15));
        for g in [imp, reporter, far] {
            host.request(g, Request::Vote(Vote::Player(PlayerId(imp))));
        }
        host.tick(Duration::from_secs(6));
        assert!(matches!(host.lock().game.phase(), Phase::GameOver { .. }));
        assert!(destroyed(&received(&mut inboxes[inbox(reporter)])).contains(&imp));

        // Back in the lobby every ghost lives again, seen by all at full health.
        drain(&mut inboxes);
        let host_player = host.lock().game.host().unwrap().0;
        host.request(host_player, Request::Lobby);
        let seen = received(&mut inboxes[inbox(reporter)]);
        for g in [victim, imp] {
            assert!(created(&seen).contains(&g), "{g} is shown again");
        }
        for g in [victim, imp] {
            let mine = received(&mut inboxes[inbox(g)]);
            let back = changes_of(&mine, g);
            assert!(back
                .iter()
                .any(|f| f.unit_health() == Some(120) && !f.player_is_ghost()));
            assert!(modes(&mine, g).contains(&(MoveMode::WaterWalk, false)));
            assert_eq!(run_speeds(&mine), [death::LIVING_SPEEDS[0]]);
        }
    }

    #[test]
    fn a_wolf_kill_fells_its_victim_like_a_knife() {
        let (host, mut inboxes) = hosted(quick(), 3);
        host.request(1, Request::Start);
        let c = roles(&host).1[0];
        let home = host.lock().game.layout().creatures[0].at;
        host.moved(c, [home[0] + 2.0, home[1], home[2]]);
        drain(&mut inboxes);
        for _ in 0..1200 {
            host.tick(Duration::from_millis(100));
            if !host.lock().game.player(PlayerId(c)).unwrap().alive {
                break;
            }
        }
        assert!(
            !host.lock().game.player(PlayerId(c)).unwrap().alive,
            "the wolf killed"
        );
        let seen = received(&mut inboxes[(c as usize) % 3]);
        assert_eq!(health_of(&seen, c).last(), Some(&0));
        assert!(modes(&received(&mut inboxes[c as usize - 1]), c).contains(&(MoveMode::Root, true)));
        // Unreleased after the popup's six minutes, the spirit is released for them.
        host.tick(AUTO_RELEASE);
        assert!(created(&received(&mut inboxes[(c as usize) % 3])).contains(&guid::corpse(c)));
    }

    /// The system lines waiting in `inbox`, in order; everything else is skipped.
    fn chat_lines(inbox: &mut UnboundedReceiver<Packet>) -> Vec<String> {
        (received(inbox).into_iter())
            .filter_map(|p| match p {
                // `CHAT_MSG_SYSTEM`; the raid-warning headlines ride beside them.
                ServerPacket::MessageChat(m) if m.chat_type == 0x0A => Some(m.text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_horn_counts_the_start_down_in_chat_then_tells_each_their_role() {
        let (host, mut inboxes) = hosted(quick(), 3);
        host.set_start_countdown(Duration::from_secs(5));
        drain(&mut inboxes);
        host.use_object(1, Fixture::Button.guid());
        assert_eq!(*host.lock().game.phase(), Phase::Lobby, "counting down");
        assert_eq!(chat_lines(&mut inboxes[1]), ["P1 starts the game: 5..."]);
        let mut heard = Vec::new();
        for _ in 0..50 {
            host.tick(Duration::from_millis(100));
            heard.extend(chat_lines(&mut inboxes[1]));
        }
        assert!(matches!(host.lock().game.phase(), Phase::Playing));
        assert_eq!(heard[..4], ["4...", "3...", "2...", "1..."]);
        assert!(heard[4].starts_with("The game begins: 3 players, 1 impostor"));

        // Each is told their own role and tasks, and nobody else's role.
        let (imp, _) = roles(&host);
        for (i, inbox) in inboxes.iter_mut().enumerate() {
            // Player 2's lines so far were read into `heard`.
            let mut lines = chat_lines(inbox);
            if i == 1 {
                lines = [heard.clone(), lines].concat();
            }
            let impostor = lines.iter().filter(|l| l.contains("IMPOSTOR")).count();
            let crewmate = lines.iter().filter(|l| l.contains("CREWMATE")).count();
            let me = i as u64 + 1 == imp;
            assert_eq!((impostor, crewmate), if me { (1, 0) } else { (0, 1) });
            assert!(
                lines.iter().any(|l| l.starts_with("Your task: ")),
                "{lines:?}"
            );
        }
    }

    #[test]
    fn a_ghost_crewmate_works_a_herb_by_right_click_unseen_by_the_living() {
        let settings = Settings {
            common_tasks: 2,
            min_players: 4,
            ..quick()
        };
        let (host, mut inboxes) = hosted(settings, 4);
        host.request(1, Request::Start);
        let (imp, crew) = roles(&host);
        let (ghost, living) = (crew[0], crew[1]);
        cast(&host, imp, spells::KILL, Target::Unit(ghost));
        drain(&mut inboxes);
        host.release_spirit(ghost);

        // The ghost's client is told every node has no lock, so a right-click is a plain use.
        let seen = received(&mut inboxes[ghost as usize - 1]);
        let locks: Vec<i32> = (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::GameObjectQueryResponse { info: Some(t), .. } => Some(t.data[0]),
                _ => None,
            })
            .collect();
        assert!(
            !locks.is_empty() && locks.iter().all(|&l| l == 0),
            "{locks:?}"
        );

        // It gathers a herb: the cast bar shows to itself, not to the living.
        let (task, nodes) = (placeholder_layout().tasks.iter().enumerate())
            .find_map(|(i, t)| match &t.steps[0].action {
                StepAction::Gather { nodes, .. } => Some((TaskId(i), nodes.clone())),
                _ => None,
            })
            .unwrap();
        host.moved(ghost, nodes[0]);
        drain(&mut inboxes);
        host.use_object(ghost, guid::node(task, 0, 0));
        let bar = |seen: &[ServerPacket]| {
            seen.iter()
                .any(|p| matches!(p, ServerPacket::SpellStart(s) if s.caster == ghost))
        };
        assert!(
            bar(&received(&mut inboxes[ghost as usize - 1])),
            "the ghost casts"
        );
        assert!(
            !bar(&received(&mut inboxes[living as usize - 1])),
            "unseen by the living"
        );
        host.tick(Duration::from_secs(6));
        let picked = (host
            .lock()
            .game
            .player(PlayerId(ghost))
            .unwrap()
            .tasks
            .iter())
        .find(|t| t.task == task)
        .unwrap()
        .picked;
        assert_eq!(picked, 1, "the herb counts");

        // Back in the lobby its client gets the locks back.
        host.tick(Duration::from_secs(30));
        host.request(imp, Request::Kill);
        assert!(matches!(host.lock().game.phase(), Phase::GameOver { .. }));
        drain(&mut inboxes);
        host.request(1, Request::Lobby);
        let seen = received(&mut inboxes[ghost as usize - 1]);
        assert!(seen.iter().any(|p| matches!(p,
            ServerPacket::GameObjectQueryResponse { info: Some(t), .. } if t.data[0] != 0)));
    }

    #[test]
    fn a_meeting_seats_the_living_at_the_table_and_hands_each_a_ballot() {
        let (host, mut inboxes, imp, crew) = four_with_one_far();
        let (victim, reporter, far) = (crew[0], crew[1], crew[2]);
        cast(&host, imp, spells::KILL, Target::Unit(victim));
        drain(&mut inboxes);
        host.request(reporter, Request::Report);
        let table = host.lock().game.layout().emergency_button;

        // The living are teleported to seats around the table; the dead stay where they are.
        for g in [imp, reporter, far] {
            let seen = received(&mut inboxes[g as usize - 1]);
            let to = (seen.iter()).find_map(|p| match p {
                ServerPacket::Teleport { guid, position, .. } if *guid == g => {
                    Some([position.x, position.y, position.z])
                }
                _ => None,
            });
            let to = to.unwrap_or_else(|| panic!("{g} is seated"));
            assert!(among_rules::within(to, table, 4.0), "{g} at the table");
            let rules_at = host.lock().game.player(PlayerId(g)).unwrap().position;
            assert_eq!(rules_at, to, "the rules know the seat");
            assert!(seen.iter().any(|p| matches!(p,
                ServerPacket::MessageChat(m) if m.chat_type == 0x58 && m.text == "Dead body reported!")));
            assert!(seen.iter().any(|p| matches!(p,
                ServerPacket::MirrorTimerStart(t) if t.remaining_ms == 15_000)));
        }
        let mine = received(&mut inboxes[victim as usize - 1]);
        assert!(!mine
            .iter()
            .any(|p| matches!(p, ServerPacket::Teleport { .. })));

        // Voting opens a ballot on each living voter's own guid: everyone standing, and skip.
        host.tick(Duration::from_secs(15));
        let seen = received(&mut inboxes[reporter as usize - 1]);
        let options = (seen.iter())
            .find_map(|p| match p {
                ServerPacket::GossipMessage { npc, options, .. } if *npc == reporter => {
                    Some(options.clone())
                }
                _ => None,
            })
            .expect("a ballot");
        let lines: Vec<&str> = options.iter().map(|o| o.message.as_str()).collect();
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(*lines.last().unwrap(), "Skip vote");
        assert!(!lines.contains(&format!("Vote: P{victim}").as_str()));
        assert!(received(&mut inboxes[victim as usize - 1])
            .iter()
            .all(|p| !matches!(p, ServerPacket::GossipMessage { .. })));

        // Picking a line votes, and the ballot closes for that voter.
        let pick = lines
            .iter()
            .position(|l| *l == format!("Vote: P{imp}"))
            .unwrap();
        host.choose(reporter, reporter, pick as u32);
        assert_eq!(
            match host.lock().game.phase() {
                Phase::Meeting(m) => m.votes.get(&PlayerId(reporter)).copied(),
                _ => None,
            },
            Some(Vote::Player(PlayerId(imp)))
        );
        assert!(received(&mut inboxes[reporter as usize - 1])
            .iter()
            .any(|p| matches!(p, ServerPacket::GossipComplete)));
    }

    #[test]
    fn a_button_pressed_while_cooling_down_says_when_it_is_ready() {
        let (host, mut inboxes, imp, crew) = four_with_one_far();
        cast(&host, imp, spells::KILL, Target::Unit(crew[0]));
        host.tick(Duration::from_millis(500));
        drain(&mut inboxes);
        cast(&host, imp, spells::KILL, Target::Unit(crew[1]));
        assert!(received(&mut inboxes[imp as usize - 1])
            .iter()
            .any(|p| matches!(p,
            ServerPacket::Notification { text } if text == "Ready in 25 s")));
    }

    #[test]
    fn dot_vote_names_a_player_or_skips_and_explains_itself() {
        let (host, _inboxes, imp, crew) = four_with_one_far();
        assert_eq!(host.vote_by_name(crew[1], Some("skip")), "Not now.");
        cast(&host, imp, spells::KILL, Target::Unit(crew[0]));
        host.request(crew[1], Request::Report);
        host.tick(Duration::from_secs(15));
        let list = host.vote_by_name(crew[1], None);
        assert!(
            list.starts_with("Vote with .vote <name> or .vote skip."),
            "{list}"
        );
        assert!(
            !list.contains(&format!("P{}", crew[0])),
            "the dead are not candidates"
        );
        assert_eq!(
            host.vote_by_name(crew[1], Some("Nobody")),
            "Nobody standing is named Nobody."
        );
        let target = format!("p{imp}");
        assert_eq!(
            host.vote_by_name(crew[1], Some(&target)),
            format!("You voted for P{imp}.")
        );
        assert_eq!(host.vote_by_name(crew[1], Some("skip")), "Already voted.");
        assert_eq!(host.vote_by_name(imp, Some("SKIP")), "You voted to skip.");
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

    type QuestLog = [Option<benilla_protocol::messages::QuestLogSlot>; quests::SLOTS];

    /// Every quest-log slot `seen` sets, applied to `log`; each must be on `owner`, as the log's
    /// fields are private to it.
    fn read_quest_log(seen: &[ServerPacket], owner: u64, log: &mut QuestLog) {
        use benilla_protocol::messages::Object;
        for p in seen {
            let ServerPacket::UpdateObject { objects } = p else {
                continue;
            };
            for o in objects {
                let Object::Values { guid, mask } = o else {
                    continue;
                };
                for (s, slot) in log.iter_mut().enumerate() {
                    if let Some(q) = mask.player_quest_log(s as u8) {
                        assert_eq!(*guid, owner, "a quest log goes to its owner alone");
                        *slot = Some(q);
                    }
                }
            }
        }
    }

    /// The quests in `log`, slot by slot, empty slots left out.
    fn quest_ids(log: &QuestLog) -> Vec<u32> {
        (log.iter().flatten())
            .map(|q| q.quest_id)
            .filter(|&id| id != 0)
            .collect()
    }

    /// The crew totals of the crew templates in `seen`.
    fn crew_totals(seen: &[ServerPacket]) -> Vec<u32> {
        (seen.iter())
            .filter_map(|p| match p {
                ServerPacket::QuestQueryResponse(t) if t.quest_id == quests::CREW_QUEST => {
                    Some(t.objectives[0].required_count)
                }
                _ => None,
            })
            .collect()
    }

    /// The log slot `task` fills for `g`: after the crew's quest, in list order.
    fn slot_of(host: &GameHost, g: u64, task: TaskId) -> usize {
        let inner = host.lock();
        let tasks = &inner.game.player(PlayerId(g)).unwrap().tasks;
        tasks.iter().position(|t| t.task == task).unwrap() + 1
    }

    /// `g` works `task` where it stands and the clock runs `secs`.
    fn work(host: &GameHost, g: u64, task: TaskId, at: Position, secs: u64) {
        host.moved(g, at);
        let player = PlayerId(g);
        host.lock()
            .apply(Command::UseTask { player, task })
            .unwrap();
        host.tick(Duration::from_secs(secs));
    }

    #[test]
    fn dealt_tasks_fill_each_owners_quest_log_and_the_crew_quest_counts_for_everyone() {
        use benilla_protocol::messages::quest_slot_state::COMPLETE;
        // Both common tasks and nothing else: Pick Peacebloom (gather 5) and Roast Boar Meat.
        let settings = Settings {
            common_tasks: 2,
            long_tasks: 0,
            short_tasks: 0,
            ..quick()
        };
        let (host, mut inboxes) = hosted(settings, 3);
        drain(&mut inboxes);
        host.request(1, Request::Start);
        let (imp, crew) = roles(&host);
        let mut logs: [QuestLog; 3] = [[None; quests::SLOTS]; 3];
        for g in 1..=3u64 {
            let seen = received(&mut inboxes[g as usize - 1]);
            assert_eq!(crew_totals(&seen), [4], "two crewmates with two tasks each");
            let log = &mut logs[g as usize - 1];
            read_quest_log(&seen, g, log);
            // The crew's quest first, then the dealt tasks; an impostor's cover tasks too.
            let tasks: Vec<u32> = (host.lock().game.player(PlayerId(g)).unwrap().tasks)
                .iter()
                .map(|t| quests::task_quest(t.task))
                .collect();
            assert_eq!(quest_ids(log), [&[quests::CREW_QUEST], &tasks[..]].concat());
            assert!((log.iter().flatten()).all(|q| q.counters == [0; 4] && q.state == 0));
        }

        let (roast, roast_at, peacebloom, herbs) = {
            let inner = host.lock();
            let layout = inner.game.layout();
            let task =
                |name: &str| TaskId((layout.tasks.iter()).position(|t| t.name == name).unwrap());
            let (roast, peacebloom) = (task("Roast Boar Meat"), task("Pick Peacebloom"));
            let StepAction::Gather { nodes, .. } = &layout.tasks[peacebloom.0].steps[0].action
            else {
                panic!("Peacebloom is gathered");
            };
            let roast_at = layout.tasks[roast.0].steps[0].at;
            (roast, roast_at, peacebloom, nodes.clone())
        };

        // A crewmate roasts the meat: its quest completes, and the crew's counts one for all.
        let c = crew[0];
        work(&host, c, roast, roast_at, 4);
        for g in 1..=3u64 {
            let seen = received(&mut inboxes[g as usize - 1]);
            assert!(crew_totals(&seen).is_empty(), "the total did not move");
            let log = &mut logs[g as usize - 1];
            read_quest_log(&seen, g, log);
            assert_eq!(log[0].unwrap().counters[0], 1, "{g} sees the crew's count");
        }
        let roasted = logs[c as usize - 1][slot_of(&host, c, roast)].unwrap();
        assert_eq!(roasted.quest_id, quests::task_quest(roast));
        assert_eq!((roasted.counters[0], roasted.state), (1, COMPLETE));

        // A herb picked counts on the Peacebloom line; the quest is not done yet.
        work(&host, c, peacebloom, herbs[0], 6);
        let mine = &mut logs[c as usize - 1];
        read_quest_log(&received(&mut inboxes[c as usize - 1]), c, mine);
        let picking = mine[slot_of(&host, c, peacebloom)].unwrap();
        assert_eq!(picking.quest_id, quests::task_quest(peacebloom));
        assert_eq!((picking.counters[0], picking.state), (1, 0));

        // The impostor's kill ends the game; the fallen keep their log until the lobby clears it.
        drain(&mut inboxes);
        host.request(imp, Request::Kill);
        assert!(matches!(host.lock().game.phase(), Phase::GameOver { .. }));
        for g in 1..=3u64 {
            let mut after = logs[g as usize - 1];
            read_quest_log(&received(&mut inboxes[g as usize - 1]), g, &mut after);
            assert_eq!(after, logs[g as usize - 1]);
        }
        let host_player = host.lock().game.host().unwrap().0;
        host.request(host_player, Request::Lobby);
        for g in 1..=3u64 {
            let log = &mut logs[g as usize - 1];
            read_quest_log(&received(&mut inboxes[g as usize - 1]), g, log);
            assert!(quest_ids(log).is_empty(), "{g}'s log is empty in the lobby");
        }
    }

    #[test]
    fn a_quest_query_is_answered_with_its_task_or_the_crew() {
        let (host, _inboxes) = hosted(quick(), 3);
        let parse = |p: Packet| match parse_server_with_tail(p.opcode, &p.body).unwrap() {
            (ServerPacket::QuestQueryResponse(t), 0) => t,
            _ => panic!("not a whole quest template"),
        };
        let name = placeholder_layout().tasks[1].name.clone();
        let t = parse(host.quest_query(quests::task_quest(TaskId(1))).unwrap());
        assert_eq!(t.title, name);
        assert_eq!(t.zone_or_sort, player::SPAWN.zone as i32);
        let crew = parse(host.quest_query(quests::CREW_QUEST).unwrap());
        assert_eq!(crew.title, "Crew Tasks");
        assert!(host.quest_query(quests::task_quest(TaskId(99))).is_none());
        assert!(host.quest_query(1).is_none());
    }
}
