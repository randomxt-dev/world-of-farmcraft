//! [`Game`]: the whole match as one state machine. [`Game::apply`] validates a [`Command`] against
//! the current state and either refuses it (state unchanged) or applies it and returns the events.

mod abilities;
mod combat;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub use abilities::{Trap, Under};
pub use combat::{Creature, SpellCast};

use crate::abilities::Effect;
use crate::classes::Class;
use crate::protocol::{
    Command, CreatureId, Event, MeetingReason, PlayerId, Reject, Role, TaskId, Team, Vote,
    VoteOutcome, WinReason,
};
use crate::rng::SplitMix64;
use crate::settings::{
    distance_squared, max_impostors, within, Layout, Position, SabotageDef, SabotageKind, Settings,
    StepAction, TaskDef, TaskKind, TaskStep,
};

/// One player's standing in the game.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub position: Position,
    /// `None` in the lobby.
    pub role: Option<Role>,
    /// False once killed or ejected: a ghost.
    pub alive: bool,
    /// False once they leave a running game; they no longer count for anything.
    pub connected: bool,
    pub tasks: Vec<PlayerTask>,
    /// Until this impostor may kill again.
    pub kill_cooldown: Duration,
    pub emergencies_left: u32,
    /// A task cast under way.
    pub casting: Option<Casting>,
    pub class: Class,
    pub health: u32,
    /// The creature this player auto-attacks.
    pub attacking: Option<CreatureId>,
    /// Until the next auto-attack swing may land.
    pub swing: Duration,
    /// A class spell cast under way.
    pub spell_cast: Option<SpellCast>,
    /// Until the class spell may be cast again.
    pub spell_cooldown: Duration,
    /// Until the class's ability may be used again.
    pub ability_cooldown: Duration,
    /// What the player is under, and for how long.
    pub effects: BTreeMap<Effect, Under>,
    /// Out-of-combat time not yet turned into regained health.
    regen: Duration,
}

impl Player {
    fn new(class: Class, settings: &Settings) -> Self {
        Self {
            position: [0.0; 3],
            role: None,
            alive: true,
            connected: true,
            tasks: Vec::new(),
            kill_cooldown: Duration::ZERO,
            emergencies_left: settings.emergency_meetings,
            casting: None,
            class,
            health: settings.kits.get(class).health,
            attacking: None,
            swing: Duration::ZERO,
            spell_cast: None,
            spell_cooldown: Duration::ZERO,
            ability_cooldown: Duration::ZERO,
            effects: BTreeMap::new(),
            regen: Duration::ZERO,
        }
    }

    pub fn is_impostor(&self) -> bool {
        self.role == Some(Role::Impostor)
    }

    pub fn is_crewmate(&self) -> bool {
        self.role == Some(Role::Crewmate)
    }

    pub fn is_under(&self, effect: Effect) -> bool {
        self.effects.contains_key(&effect)
    }

    /// Stealthed: unseen by the living, and no one's target.
    pub fn is_hidden(&self) -> bool {
        self.is_under(Effect::Stealth)
    }

    /// Polymorphed, feared or frozen.
    pub fn is_incapacitated(&self) -> bool {
        self.effects.keys().any(|e| e.incapacitates())
    }

    /// Held in place by an effect.
    pub fn is_rooted(&self) -> bool {
        self.effects.keys().any(|e| e.roots())
    }

    /// The share of their normal run speed the player's effects leave them.
    pub fn speed(&self) -> f32 {
        self.effects.keys().map(|e| e.speed()).product()
    }
}

/// A task cast under way: it completes when `remaining` runs out, and breaks if the caster moves
/// off `from`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Casting {
    pub task: TaskId,
    pub remaining: Duration,
    pub from: Position,
    /// The node being gathered, for a Gather step.
    pub node: Option<usize>,
}

/// How far a caster may drift before the cast breaks; WoW breaks a cast on any step, not a turn.
const CAST_DRIFT: f32 = 0.1;

/// A task in a player's list and how many of its steps are done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerTask {
    pub task: TaskId,
    pub step: usize,
    /// Kills toward the current step, when it is a Slay step.
    pub kills: u32,
    /// The nodes already gathered in the current step, one bit each, when it is a Gather step.
    pub picked: u64,
}

/// A dead crewmate's body, where they fell, until a meeting clears the map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub victim: PlayerId,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Lobby,
    Playing,
    Meeting(Meeting),
    /// The ejection reveal; `ejected` is already a ghost.
    Ejecting {
        ejected: Option<PlayerId>,
        remaining: Duration,
    },
    GameOver {
        winner: Team,
        reason: WinReason,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Meeting {
    pub caller: PlayerId,
    pub reason: MeetingReason,
    pub stage: MeetingStage,
    /// Until the stage ends.
    pub remaining: Duration,
    /// Votes cast so far, secret until the tally.
    pub votes: BTreeMap<PlayerId, Vote>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeetingStage {
    Discussion,
    Voting,
}

/// The buttons a player's HUD shows as live; see [`Game::actions`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Actions {
    pub kill: bool,
    pub report: bool,
    /// A task step in reach.
    pub use_: bool,
    /// A sabotage station in reach.
    pub fix: bool,
    /// The emergency button in reach and pressable.
    pub emergency: bool,
    pub sabotage: bool,
    /// Whole seconds until a living impostor may kill again, rounded up; 0 otherwise.
    pub kill_cooldown_secs: u32,
}

/// The sabotage in force and how far its fix has come.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveSabotage {
    pub kind: SabotageKind,
    /// Until the impostors win, for a critical sabotage.
    pub remaining: Option<Duration>,
    /// Indexed like the map's [`SabotageDef::stations`].
    pub stations: Vec<Station>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Station {
    Open,
    /// Held by a player, for a sabotage that needs every station held at once.
    Held(PlayerId),
    Fixed,
}

/// The match: lobby, roles, tasks, kills, meetings, votes, sabotage and the win check.
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    settings: Settings,
    layout: Layout,
    phase: Phase,
    host: Option<PlayerId>,
    players: BTreeMap<PlayerId, Player>,
    bodies: Vec<Body>,
    emergency_cooldown: Duration,
    sabotage: Option<ActiveSabotage>,
    sabotage_cooldown: Duration,
    /// Indexed by [`CreatureId`]; empty outside a game.
    creatures: Vec<Creature>,
    /// Gather nodes picked, as `(task, step, node)`: gone for everyone until the lobby.
    taken: BTreeSet<(TaskId, usize, usize)>,
    /// How many players hold each task, indexed by [`TaskId`]; empty outside a game.
    holders: Vec<usize>,
    /// Freezing traps waiting to be sprung, at most one per owner.
    traps: Vec<Trap>,
    out: Vec<Event>,
}

/// How many players `task` can be dealt to: its scarcest Gather step's spots, each holder
/// taking at least one; unlimited for a task with no Gather step.
fn capacity(task: &TaskDef) -> usize {
    (task.steps.iter())
        .filter_map(|s| match &s.action {
            StepAction::Gather { nodes, .. } => Some(nodes.len().min(64)),
            _ => None,
        })
        .min()
        .unwrap_or(usize::MAX)
}

impl Game {
    pub fn new(settings: Settings, layout: Layout) -> Self {
        Self {
            settings,
            layout,
            phase: Phase::Lobby,
            host: None,
            players: BTreeMap::new(),
            bodies: Vec::new(),
            emergency_cooldown: Duration::ZERO,
            sabotage: None,
            sabotage_cooldown: Duration::ZERO,
            creatures: Vec::new(),
            taken: BTreeSet::new(),
            holders: Vec::new(),
            traps: Vec::new(),
            out: Vec::new(),
        }
    }

    /// Whether node `node` of `task`'s step `step` was picked this game.
    pub fn is_taken(&self, task: TaskId, step: usize, node: usize) -> bool {
        self.taken.contains(&(task, step, node))
    }

    /// How many nodes each holder of `task` gathers at Gather step `step`: the layout's count, cut
    /// to an even share of the spots among the players holding the task (an impostor's fake picks
    /// take nodes too) so none runs out; 0 for any other step.
    pub fn gather_need(&self, task: TaskId, step: usize) -> u32 {
        let action = (self.layout.tasks.get(task.0)).and_then(|t| t.steps.get(step));
        let Some(StepAction::Gather { nodes, count, .. }) = action.map(|s| &s.action) else {
            return 0;
        };
        let holders = self.holders.get(task.0).copied().unwrap_or(0).max(1);
        let share = (nodes.len().min(64) / holders) as u32;
        (*count).min(share.max(1))
    }

    /// Whether node `i` of `t`'s current Gather step is still there for them to pick, within
    /// their share (a crewmate's step moves on at it; an impostor's cover stops there).
    fn node_open(&self, t: &PlayerTask, i: usize) -> bool {
        i < 64
            && t.picked & 1 << i == 0
            && t.picked.count_ones() < self.gather_need(t.task, t.step)
            && !self.is_taken(t.task, t.step, i)
    }

    /// The monsters at the task spots, indexed by [`CreatureId`].
    pub fn creatures(&self) -> &[Creature] {
        &self.creatures
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn host(&self) -> Option<PlayerId> {
        self.host
    }

    /// Every seat, connected or not, in id order.
    pub fn players(&self) -> impl Iterator<Item = (PlayerId, &Player)> {
        self.players.iter().map(|(&id, p)| (id, p))
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(&id)
    }

    /// Unreported bodies, oldest first.
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    /// Until the emergency button works again.
    pub fn emergency_cooldown(&self) -> Duration {
        self.emergency_cooldown
    }

    pub fn sabotage(&self) -> Option<&ActiveSabotage> {
        self.sabotage.as_ref()
    }

    /// Until the impostors may sabotage again; it runs only while no sabotage is active.
    pub fn sabotage_cooldown(&self) -> Duration {
        self.sabotage_cooldown
    }

    /// Crew tasks done, and the total over connected crewmates (living or dead).
    pub fn task_bar(&self) -> (usize, usize) {
        let mut done = 0;
        let mut total = 0;
        for p in self
            .players
            .values()
            .filter(|p| p.connected && p.is_crewmate())
        {
            total += p.tasks.len();
            done += p.tasks.iter().filter(|t| self.task_done(t)).count();
        }
        (done, total)
    }

    /// The nearest living crewmate `killer` may kill right now: what a Kill button would hit.
    pub fn kill_target(&self, killer: PlayerId) -> Option<PlayerId> {
        let from = self.players.get(&killer)?.position;
        self.nearest(
            from,
            self.players.iter().map(|(&id, p)| (p.position, id)),
            |&victim| Command::Kill { killer, victim },
        )
    }

    /// The nearest body `player` may report right now.
    pub fn report_target(&self, player: PlayerId) -> Option<PlayerId> {
        let from = self.players.get(&player)?.position;
        let bodies = self.bodies.iter().map(|b| (b.position, b.victim));
        self.nearest(from, bodies, |&body| Command::Report { player, body })
    }

    /// The task a Use button would work on for `player` right now: the nearest step in reach,
    /// a cover task's for an impostor.
    pub fn task_target(&self, player: PlayerId) -> Option<TaskId> {
        let p = self.players.get(&player)?;
        let mut steps = Vec::new();
        for t in &p.tasks {
            let Some(step) = self.layout.tasks[t.task.0].steps.get(t.step) else {
                continue;
            };
            match &step.action {
                // A Gather step is in reach at any node not picked yet.
                StepAction::Gather { nodes, .. } => steps.extend(
                    (nodes.iter().enumerate())
                        .filter(|(i, _)| self.node_open(t, *i))
                        .map(|(_, &at)| (at, t.task)),
                ),
                _ => steps.push((step.at, t.task)),
            }
        }
        self.nearest(p.position, steps, |&task| Command::UseTask { player, task })
    }

    /// The station of the active sabotage a Fix button would work on for `player` right now.
    pub fn fix_target(&self, player: PlayerId) -> Option<usize> {
        let p = self.players.get(&player)?;
        let s = self.sabotage.as_ref()?;
        let stations = (0..s.stations.len()).map(|i| (self.station_position(s.kind, i), i));
        self.nearest(p.position, stations, |&station| Command::Fix {
            player,
            station,
        })
    }

    /// The sabotage a Sabotage button would start for `player` right now: of those legal, the one
    /// whose nearest station is farthest away, so the crew is drawn from the impostor.
    pub fn sabotage_target(&self, player: PlayerId) -> Option<SabotageKind> {
        let from = self.players.get(&player)?.position;
        let reach = |def: &SabotageDef| {
            (def.stations.iter())
                .map(|&at| distance_squared(from, at))
                .fold(f32::INFINITY, f32::min)
        };
        (self.layout.sabotages.iter())
            .filter(|def| {
                let cmd = Command::Sabotage {
                    player,
                    kind: def.kind,
                };
                // The first definition of a kind is the one in force.
                self.sabotage_def(def.kind)
                    .is_some_and(|first| std::ptr::eq(first, *def))
                    && self.validate(&cmd).is_ok()
            })
            .max_by(|a, b| reach(a).total_cmp(&reach(b)))
            .map(|def| def.kind)
    }

    /// Which buttons `player` has live right now.
    pub fn actions(&self, player: PlayerId) -> Actions {
        let sabotage = [
            SabotageKind::Lights,
            SabotageKind::Comms,
            SabotageKind::Oxygen,
            SabotageKind::Reactor,
        ]
        .into_iter()
        .any(|kind| self.validate(&Command::Sabotage { player, kind }).is_ok());
        let kill_cooldown = (self.players.get(&player))
            .filter(|p| p.is_impostor() && p.alive && self.phase == Phase::Playing)
            .map_or(Duration::ZERO, |p| p.kill_cooldown);
        Actions {
            kill: self.kill_target(player).is_some(),
            report: self.report_target(player).is_some(),
            use_: self.task_target(player).is_some(),
            fix: self.fix_target(player).is_some(),
            emergency: self.validate(&Command::CallEmergency { player }).is_ok(),
            sabotage,
            kill_cooldown_secs: kill_cooldown.as_secs_f32().ceil() as u32,
        }
    }

    /// The candidate nearest `from` whose command validates.
    fn nearest<T>(
        &self,
        from: Position,
        candidates: impl IntoIterator<Item = (Position, T)>,
        command: impl Fn(&T) -> Command,
    ) -> Option<T> {
        candidates
            .into_iter()
            .filter(|(_, c)| self.validate(&command(c)).is_ok())
            .min_by(|a, b| distance_squared(from, a.0).total_cmp(&distance_squared(from, b.0)))
            .map(|(_, c)| c)
    }

    /// Applies `cmd` if it is legal now and returns what happened; otherwise changes nothing.
    pub fn apply(&mut self, cmd: Command) -> Result<Vec<Event>, Reject> {
        self.validate(&cmd)?;
        self.execute(cmd);
        Ok(std::mem::take(&mut self.out))
    }

    /// Whether `cmd` would be accepted now, without applying it (a HUD can grey out a button).
    pub fn validate(&self, cmd: &Command) -> Result<(), Reject> {
        match *cmd {
            Command::Join { player, .. } => {
                self.in_lobby()?;
                if self.players.contains_key(&player) {
                    return Err(Reject::AlreadyJoined);
                }
                if self.players.len() >= self.settings.max_players {
                    return Err(Reject::LobbyFull);
                }
                Ok(())
            }
            Command::Leave { player } | Command::Move { player, .. } => {
                self.connected(player).map(drop)
            }
            Command::Configure { by, .. } => {
                self.in_lobby()?;
                self.is_host(by)
            }
            Command::Start { by, .. } => {
                self.in_lobby()?;
                self.is_host(by)?;
                if self.players.len() < self.settings.min_players.max(3) {
                    return Err(Reject::NotEnoughPlayers);
                }
                Ok(())
            }
            Command::UseTask { player, task } => {
                let (p, t, step) = self.workable_step(player, task)?;
                match &step.action {
                    StepAction::Slay { .. } => Err(Reject::NotAUseStep),
                    StepAction::Gather { nodes, .. } => (self.gather_node(p.position, t, nodes))
                        .map(drop)
                        .ok_or(Reject::OutOfRange),
                    StepAction::Cast(_) => {
                        self.in_reach(p.position, step.at, self.settings.use_distance)
                    }
                }
            }
            Command::GatherNode { player, task, node } => {
                let (p, t, step) = self.workable_step(player, task)?;
                let StepAction::Gather { nodes, .. } = &step.action else {
                    return Err(Reject::InvalidTarget);
                };
                let at = *(nodes.get(node))
                    .filter(|_| node < 64)
                    .ok_or(Reject::InvalidTarget)?;
                if !self.node_open(t, node) {
                    return Err(Reject::AlreadyGathered);
                }
                self.in_reach(p.position, at, self.settings.use_distance)
            }
            Command::Kill { killer, victim } => {
                self.playing()?;
                let k = self.able(killer)?;
                if !k.is_impostor() {
                    return Err(Reject::NotImpostor);
                }
                // Cooldown before target: a refused press says "on cooldown", aimed or not.
                if !k.kill_cooldown.is_zero() {
                    return Err(Reject::OnCooldown);
                }
                let v = (self.players.get(&victim))
                    .filter(|v| v.connected && v.alive && v.is_crewmate() && !v.is_hidden())
                    .ok_or(Reject::InvalidTarget)?;
                if v.is_under(Effect::DivineShield) {
                    return Err(Reject::Immune);
                }
                self.in_reach(k.position, v.position, self.settings.kill_distance)
            }
            Command::Report { player, body } => {
                self.playing()?;
                let p = self.able(player)?;
                let b = (self.bodies.iter())
                    .find(|b| b.victim == body)
                    .ok_or(Reject::NoSuchBody)?;
                self.in_reach(p.position, b.position, self.settings.report_distance)
            }
            Command::CallEmergency { player } => {
                self.playing()?;
                let p = self.able(player)?;
                if p.emergencies_left == 0 {
                    return Err(Reject::NoEmergencyMeetingsLeft);
                }
                if self.sabotage.is_some() {
                    return Err(Reject::SabotageActive);
                }
                if !self.emergency_cooldown.is_zero() {
                    return Err(Reject::OnCooldown);
                }
                let button = self.layout.emergency_button;
                self.in_reach(p.position, button, self.settings.use_distance)
            }
            Command::Vote { voter, vote } => {
                let Phase::Meeting(m) = &self.phase else {
                    return Err(Reject::WrongPhase);
                };
                if m.stage != MeetingStage::Voting {
                    return Err(Reject::WrongPhase);
                }
                self.alive(voter)?;
                if m.votes.contains_key(&voter) {
                    return Err(Reject::AlreadyVoted);
                }
                match vote {
                    Vote::Player(target) => self
                        .alive(target)
                        .map(drop)
                        .map_err(|_| Reject::InvalidTarget),
                    Vote::Skip => Ok(()),
                }
            }
            Command::Sabotage { player, kind } => {
                self.playing()?;
                let p = self.connected(player)?;
                if !p.is_impostor() {
                    return Err(Reject::NotImpostor);
                }
                if p.is_incapacitated() {
                    return Err(Reject::Incapacitated);
                }
                if self.sabotage.is_some() {
                    return Err(Reject::SabotageActive);
                }
                if !self.sabotage_cooldown.is_zero() {
                    return Err(Reject::OnCooldown);
                }
                self.sabotage_def(kind)
                    .map(drop)
                    .ok_or(Reject::NoSuchSabotage)
            }
            Command::Fix { player, station } => {
                self.playing()?;
                let p = self.able(player)?;
                let s = self.sabotage.as_ref().ok_or(Reject::NoSabotage)?;
                match s.stations.get(station).ok_or(Reject::NoSuchStation)? {
                    Station::Fixed => return Err(Reject::StationFixed),
                    Station::Held(other) if *other != player => return Err(Reject::StationBusy),
                    _ => {}
                }
                let at = self.station_position(s.kind, station);
                self.in_reach(p.position, at, self.settings.use_distance)
            }
            Command::ReleaseFix { player } => self.connected(player).map(drop),
            Command::Attack { player, creature } => self.validate_attack(player, creature),
            Command::StopAttack { player } => self.connected(player).map(drop),
            Command::CastSpell { player, creature } => self.validate_spell(player, creature),
            Command::UseAbility { player, target } => self.validate_ability(player, target),
            Command::ReturnToLobby { by } => {
                if !matches!(self.phase, Phase::GameOver { .. }) {
                    return Err(Reject::WrongPhase);
                }
                self.is_host(by)
            }
            Command::Tick { .. } => Ok(()),
        }
    }

    fn execute(&mut self, cmd: Command) {
        match cmd {
            Command::Join { player, class } => {
                self.players
                    .insert(player, Player::new(class, &self.settings));
                self.out.push(Event::Joined { player });
                if self.host.is_none() {
                    self.set_host(Some(player));
                }
            }
            Command::Leave { player } => self.leave(player),
            Command::Configure { settings, .. } => {
                self.settings = *settings;
                for p in self.players.values_mut() {
                    p.emergencies_left = self.settings.emergency_meetings;
                }
                self.out.push(Event::SettingsChanged);
            }
            Command::Start { seed, .. } => self.start(seed),
            Command::Move { player, position } => {
                let p = self.seat(player);
                p.position = position;
                let task_cast_broken = p
                    .casting
                    .is_some_and(|c| !within(c.from, position, CAST_DRIFT));
                let spell_broken = p
                    .spell_cast
                    .is_some_and(|c| !within(c.from, position, CAST_DRIFT));
                if task_cast_broken {
                    self.interrupt(player);
                }
                if spell_broken {
                    self.interrupt_spell(player);
                }
                self.release_out_of_reach(player);
                self.spring_traps(player);
            }
            Command::UseTask { player, task } => {
                self.break_stealth(player);
                self.use_task(player, task);
            }
            Command::GatherNode { player, task, node } => {
                self.break_stealth(player);
                let time = match &self.layout.tasks[task.0].steps[self.step_of(player, task)].action
                {
                    StepAction::Gather { cast, .. } => *cast,
                    _ => return,
                };
                self.start_step_cast(player, task, time, Some(node));
            }
            Command::Kill { killer, victim } => self.kill(killer, victim),
            Command::Report { player, body } => {
                self.start_meeting(player, MeetingReason::Report { body })
            }
            Command::CallEmergency { player } => {
                self.seat(player).emergencies_left -= 1;
                self.start_meeting(player, MeetingReason::Emergency);
            }
            Command::Vote { voter, vote } => {
                if let Phase::Meeting(m) = &mut self.phase {
                    m.votes.insert(voter, vote);
                }
                self.out.push(Event::VoteCast { voter });
                if self.everyone_voted() {
                    self.close_voting();
                }
            }
            Command::Sabotage { kind, .. } => self.start_sabotage(kind),
            Command::Fix { player, station } => {
                self.break_stealth(player);
                self.fix(player, station);
            }
            Command::ReleaseFix { player } => self.release_stations(player),
            Command::Attack { player, creature } => {
                self.break_stealth(player);
                self.attack(player, creature);
            }
            Command::StopAttack { player } => self.stop_attack(player),
            Command::CastSpell { player, creature } => {
                self.break_stealth(player);
                self.cast_spell(player, creature);
            }
            Command::UseAbility { player, target } => self.use_ability(player, target),
            Command::ReturnToLobby { .. } => self.return_to_lobby(),
            Command::Tick { dt } => self.tick(dt),
        }
    }

    fn start(&mut self, seed: u64) {
        let mut rng = SplitMix64::new(seed);
        let n = self.players.len();
        let impostors = self.settings.impostors.clamp(1, max_impostors(n));
        let mut ids: Vec<PlayerId> = self.players.keys().copied().collect();
        rng.shuffle(&mut ids);
        let mut impostor_ids = ids[..impostors].to_vec();
        impostor_ids.sort();

        // Common tasks are drawn once for everyone; long and short ones per player, in id order.
        // Every pick takes a node from everyone (an impostor's fake one too), so a task is dealt
        // to no more players than its scarcest Gather step has spots.
        let layout = &self.layout;
        let mut room: Vec<usize> = layout.tasks.iter().map(capacity).collect();
        let mut draw = |kind: TaskKind, count: usize, need: usize, room: &mut Vec<usize>| {
            let mut ids: Vec<TaskId> = (layout.tasks.iter().enumerate())
                .filter(|&(i, t)| t.kind == kind && !t.steps.is_empty() && room[i] >= need)
                .map(|(i, _)| TaskId(i))
                .collect();
            rng.shuffle(&mut ids);
            ids.truncate(count);
            for t in &ids {
                room[t.0] -= need;
            }
            ids
        };
        let common = draw(TaskKind::Common, self.settings.common_tasks, n, &mut room);
        let mut lists: Vec<(PlayerId, Vec<TaskId>)> = Vec::new();
        // Crewmates draw first, so an impostor's cover takes only the spots they leave.
        let (crew, cover): (Vec<PlayerId>, Vec<PlayerId>) = (self.players.keys())
            .copied()
            .partition(|id| !impostor_ids.contains(id));
        for id in crew.into_iter().chain(cover) {
            let mut tasks = common.clone();
            tasks.extend(draw(TaskKind::Long, self.settings.long_tasks, 1, &mut room));
            tasks.extend(draw(
                TaskKind::Short,
                self.settings.short_tasks,
                1,
                &mut room,
            ));
            lists.push((id, tasks));
        }
        self.holders = vec![0; self.layout.tasks.len()];
        for (_, tasks) in &lists {
            for t in tasks {
                self.holders[t.0] += 1;
            }
        }
        self.taken.clear();
        self.traps.clear();

        // Each player is dealt the round's class after roles and tasks, so those draws stand. The
        // classes come off a shuffled deck of all nine, so none repeats until every one is out.
        let mut dealt = Vec::new();
        if self.settings.random_classes {
            let mut deck: Vec<Class> = Vec::new();
            for (&id, p) in &mut self.players {
                if deck.is_empty() {
                    deck = Class::ALL.to_vec();
                    rng.shuffle(&mut deck);
                }
                let class = deck.pop().expect("refilled when empty");
                if class != p.class {
                    p.class = class;
                    dealt.push(Event::ClassChanged { player: id, class });
                }
            }
        }

        self.out.push(Event::GameStarted {
            players: n,
            impostors,
        });
        self.out.extend(dealt);
        for (id, tasks) in lists {
            let impostor = impostor_ids.contains(&id);
            let role = if impostor {
                Role::Impostor
            } else {
                Role::Crewmate
            };
            let p = self.players.get_mut(&id).expect("seated");
            p.role = Some(role);
            p.alive = true;
            p.tasks = tasks
                .iter()
                .map(|&task| PlayerTask {
                    task,
                    step: 0,
                    kills: 0,
                    picked: 0,
                })
                .collect();
            p.health = self.settings.kits.get(p.class).health;
            p.kill_cooldown = self.settings.initial_kill_cooldown;
            p.ability_cooldown = self.settings.initial_ability_cooldown;
            p.effects.clear();
            p.emergencies_left = self.settings.emergency_meetings;
            let fellow_impostors = if impostor {
                impostor_ids.iter().copied().filter(|&i| i != id).collect()
            } else {
                Vec::new()
            };
            self.out.push(Event::RoleAssigned {
                player: id,
                role,
                fellow_impostors,
            });
            self.out.push(Event::TasksAssigned { player: id, tasks });
        }
        self.emergency_cooldown = self.settings.emergency_cooldown;
        self.sabotage = None;
        self.sabotage_cooldown = self.settings.sabotage_cooldown;
        self.phase = Phase::Playing;
        self.push_task_bar();
        self.spawn_creatures();
    }

    /// Start the step's cast, or do the step at once if it takes no time.
    fn use_task(&mut self, player: PlayerId, task: TaskId) {
        let p = &self.players[&player];
        let t = p
            .tasks
            .iter()
            .find(|t| t.task == task)
            .expect("validated task");
        // Slay steps never reach here: validate refuses Use on them.
        let (time, node) = match &self.layout.tasks[task.0].steps[t.step].action {
            StepAction::Cast(time) => (*time, None),
            StepAction::Gather { nodes, cast, .. } => {
                (*cast, self.gather_node(p.position, t, nodes))
            }
            StepAction::Slay { .. } => return,
        };
        self.start_step_cast(player, task, time, node);
    }

    /// Cast `time` at `task`'s current step (at `node` of a Gather step), or finish it at once.
    fn start_step_cast(
        &mut self,
        player: PlayerId,
        task: TaskId,
        time: Duration,
        node: Option<usize>,
    ) {
        if time.is_zero() {
            self.finish_step_cast(player, task, node);
            return;
        }
        let from = self.players[&player].position;
        self.seat(player).casting = Some(Casting {
            task,
            remaining: time,
            from,
            node,
        });
        self.out.push(Event::CastStarted { player, task, time });
    }

    /// The step of `task` that `player` is on; the task is assigned (validated).
    fn step_of(&self, player: PlayerId, task: TaskId) -> usize {
        (self.players[&player].tasks.iter())
            .find(|t| t.task == task)
            .expect("validated task")
            .step
    }

    /// The nearest node of a Gather step `t` has not picked yet, within reach of `from`.
    fn gather_node(&self, from: Position, t: &PlayerTask, nodes: &[Position]) -> Option<usize> {
        (nodes.iter().enumerate())
            .filter(|&(i, &at)| {
                self.node_open(t, i) && within(from, at, self.settings.use_distance)
            })
            .min_by(|a, b| distance_squared(from, *a.1).total_cmp(&distance_squared(from, *b.1)))
            .map(|(i, _)| i)
    }

    /// A task cast ran out: a gathered node counts toward its step; any other cast is the step.
    fn finish_step_cast(&mut self, player: PlayerId, task: TaskId, node: Option<usize>) {
        let Some(node) = node else {
            self.advance(player, task);
            return;
        };
        let p = &self.players[&player];
        let impostor = p.is_impostor();
        let i = (p.tasks.iter())
            .position(|t| t.task == task)
            .expect("validated task");
        let step = p.tasks[i].step;
        // Another pick of the same node landed first: this cast found nothing.
        if self.is_taken(task, step, node) {
            return;
        }
        let count = self.gather_need(task, step);
        // An impostor's fake pick takes the node like a real one, so it is no tell.
        self.taken.insert((task, step, node));
        self.out.push(Event::NodeTaken { task, step, node });
        // Whoever else was casting at it finds it gone.
        let rivals: Vec<PlayerId> = (self.players.iter())
            .filter(|(&id, p)| {
                id != player
                    && p.casting
                        .is_some_and(|c| c.task == task && c.node == Some(node))
                    && p.tasks.iter().any(|t| t.task == task && t.step == step)
            })
            .map(|(&id, _)| id)
            .collect();
        for rival in rivals {
            self.interrupt(rival);
        }
        let t = &mut self.seat(player).tasks[i];
        t.picked |= 1 << node;
        let picked = t.picked.count_ones();
        self.out.push(Event::GatherProgress {
            player,
            task,
            step,
            node,
            picked,
            count,
        });
        // An impostor's cover counts up for them but never finishes.
        if picked >= count && !impostor {
            self.advance(player, task);
        }
    }

    /// Break every cast and fight: play stops for a meeting or the end of the game.
    fn interrupt_all(&mut self) {
        let casters: Vec<PlayerId> = (self.players.iter())
            .filter(|(_, p)| p.casting.is_some())
            .map(|(&id, _)| id)
            .collect();
        for player in casters {
            self.interrupt(player);
        }
        self.halt_combat();
    }

    fn interrupt(&mut self, player: PlayerId) {
        if self.seat(player).casting.take().is_some() {
            self.out.push(Event::CastInterrupted { player });
        }
    }

    /// Run every task cast down by `dt`; finished ones advance their step.
    fn tick_casts(&mut self, dt: Duration) {
        let mut finished = Vec::new();
        for (&id, p) in self.players.iter_mut() {
            let Some(c) = &mut p.casting else { continue };
            if dt >= c.remaining {
                finished.push((id, c.task, c.node));
                p.casting = None;
            } else {
                c.remaining -= dt;
            }
        }
        for (player, task, node) in finished {
            if self.phase != Phase::Playing {
                return;
            }
            self.out.push(Event::CastFinished { player });
            self.finish_step_cast(player, task, node);
        }
    }

    /// One step of `task` done; an impostor's cover task never moves.
    fn advance(&mut self, player: PlayerId, task: TaskId) {
        let steps = self.layout.tasks[task.0].steps.len();
        let p = self.seat(player);
        if p.is_impostor() {
            return;
        }
        let t = p
            .tasks
            .iter_mut()
            .find(|t| t.task == task)
            .expect("validated task");
        t.step += 1;
        t.kills = 0;
        t.picked = 0;
        let step = t.step;
        self.out.push(Event::TaskProgress {
            player,
            task,
            step,
            steps,
        });
        if step == steps {
            self.out.push(Event::TaskCompleted { player, task });
            self.push_task_bar();
            self.check_win();
        } else if !self.players[&player].alive && self.is_slay_step(task, step) {
            self.advance(player, task);
        }
    }

    fn kill(&mut self, killer: PlayerId, victim: PlayerId) {
        self.break_stealth(killer);
        if self.shield_stops(killer, victim) {
            return;
        }
        self.clear_effects_of(victim);
        self.interrupt(victim);
        self.leave_combat(victim);
        let v = self.seat(victim);
        v.alive = false;
        let at = v.position;
        let cooldown = self.settings.kill_cooldown;
        let k = self.seat(killer);
        k.position = at;
        k.kill_cooldown = cooldown;
        self.bodies.push(Body {
            victim,
            position: at,
        });
        self.release_stations(victim);
        self.out.push(Event::Killed { killer, victim, at });
        if !self.check_win() {
            self.pass_slay_steps(victim);
        }
    }

    /// A ghost cannot fight, so each Slay step a ghost crewmate stands at is passed: their tasks
    /// can still be finished, as a ghost's are in Among Us.
    fn pass_slay_steps(&mut self, player: PlayerId) {
        let p = &self.players[&player];
        if p.alive || !p.is_crewmate() {
            return;
        }
        let at_slay: Vec<TaskId> = (p.tasks.iter())
            .filter(|t| self.is_slay_step(t.task, t.step))
            .map(|t| t.task)
            .collect();
        for task in at_slay {
            if self.phase != Phase::Playing {
                return;
            }
            self.advance(player, task);
        }
    }

    fn is_slay_step(&self, task: TaskId, step: usize) -> bool {
        matches!(
            self.layout.tasks[task.0].steps.get(step).map(|s| &s.action),
            Some(StepAction::Slay { .. })
        )
    }

    fn leave(&mut self, player: PlayerId) {
        if self.phase == Phase::Lobby {
            self.players.remove(&player);
        } else {
            self.interrupt(player);
            self.leave_combat(player);
            self.clear_effects_of(player);
            self.seat(player).connected = false;
            self.release_stations(player);
        }
        self.out.push(Event::Left { player });
        if self.host == Some(player) {
            let next = self
                .players
                .iter()
                .find(|(_, p)| p.connected)
                .map(|(&id, _)| id);
            self.set_host(next);
        }
        if let Phase::Meeting(m) = &mut self.phase {
            m.votes.remove(&player);
            let returned: Vec<PlayerId> = (m.votes.iter())
                .filter(|(_, &v)| v == Vote::Player(player))
                .map(|(&voter, _)| voter)
                .collect();
            for voter in returned {
                m.votes.remove(&voter);
                self.out.push(Event::VoteReturned { voter });
            }
        }
        if self.in_progress() {
            self.push_task_bar();
            if !self.check_win() && self.everyone_voted() {
                self.close_voting();
            }
        }
    }

    fn start_meeting(&mut self, caller: PlayerId, reason: MeetingReason) {
        self.interrupt_all();
        self.clear_all_effects();
        let discussion = self.settings.discussion_time;
        self.out.push(Event::MeetingCalled {
            caller,
            reason,
            discussion,
        });
        if !self.bodies.is_empty() {
            self.bodies.clear();
            self.out.push(Event::BodiesCleared);
        }
        // A meeting ends a critical sabotage; lights and comms stay broken through it.
        if self
            .sabotage
            .as_ref()
            .is_some_and(|s| s.remaining.is_some())
        {
            self.end_sabotage();
        }
        self.phase = Phase::Meeting(Meeting {
            caller,
            reason,
            stage: MeetingStage::Discussion,
            remaining: discussion,
            votes: BTreeMap::new(),
        });
        if discussion.is_zero() {
            self.open_voting();
        }
    }

    fn open_voting(&mut self) {
        let voting = self.settings.voting_time;
        if let Phase::Meeting(m) = &mut self.phase {
            m.stage = MeetingStage::Voting;
            m.remaining = voting;
        }
        self.out.push(Event::VotingOpened { voting });
    }

    /// Whether every living, connected player has a vote in an open ballot.
    fn everyone_voted(&self) -> bool {
        let Phase::Meeting(m) = &self.phase else {
            return false;
        };
        m.stage == MeetingStage::Voting
            && (self.players.iter())
                .filter(|(_, p)| p.connected && p.alive)
                .all(|(id, _)| m.votes.contains_key(id))
    }

    fn close_voting(&mut self) {
        let Phase::Meeting(m) = std::mem::replace(&mut self.phase, Phase::Playing) else {
            return;
        };
        let votes: Vec<(PlayerId, Vote)> = m.votes.into_iter().collect();
        let outcome = tally(&votes);
        self.out.push(Event::VotingClosed { votes, outcome });
        let ejected = match outcome {
            VoteOutcome::Ejected(player) => Some(player),
            VoteOutcome::Tie | VoteOutcome::Skipped => None,
        };
        if let Some(player) = ejected {
            let p = self.seat(player);
            p.alive = false;
            let impostor = p.is_impostor();
            let left = (self.players.values())
                .filter(|p| p.connected && p.alive && p.is_impostor())
                .count();
            let confirm = self.settings.confirm_ejects;
            self.out.push(Event::Ejected {
                player,
                impostor: confirm.then_some(impostor),
                impostors_left: confirm.then_some(left),
            });
        }
        self.phase = Phase::Ejecting {
            ejected,
            remaining: self.settings.ejection_time,
        };
    }

    fn end_ejection(&mut self, ejected: Option<PlayerId>) {
        self.out.push(Event::MeetingEnded);
        self.phase = Phase::Playing;
        if self.check_win() {
            return;
        }
        if let Some(ejected) = ejected {
            self.pass_slay_steps(ejected);
            if self.phase != Phase::Playing {
                return;
            }
        }
        let cooldown = self.settings.kill_cooldown;
        for p in self.players.values_mut().filter(|p| p.is_impostor()) {
            p.kill_cooldown = cooldown;
        }
        // Everyone walks out of a meeting at full health.
        for p in self.players.values_mut().filter(|p| p.alive) {
            p.health = self.settings.kits.get(p.class).health;
        }
        self.emergency_cooldown = self.settings.emergency_cooldown;
    }

    fn return_to_lobby(&mut self) {
        self.players.retain(|_, p| p.connected);
        self.bodies.clear();
        self.creatures.clear();
        self.taken.clear();
        self.holders.clear();
        self.traps.clear();
        for p in self.players.values_mut() {
            *p = Player {
                position: p.position,
                ..Player::new(p.class, &self.settings)
            };
        }
        self.emergency_cooldown = Duration::ZERO;
        self.sabotage = None;
        self.sabotage_cooldown = Duration::ZERO;
        self.phase = Phase::Lobby;
        self.out.push(Event::ReturnedToLobby);
    }

    /// Runs the clock; a long `dt` carries through every stage boundary it crosses, in order.
    fn tick(&mut self, mut dt: Duration) {
        loop {
            match &mut self.phase {
                Phase::Meeting(m) => {
                    if dt < m.remaining {
                        m.remaining -= dt;
                        return;
                    }
                    dt -= m.remaining;
                    match m.stage {
                        MeetingStage::Discussion => self.open_voting(),
                        MeetingStage::Voting => self.close_voting(),
                    }
                }
                Phase::Ejecting { remaining, ejected } => {
                    if dt < *remaining {
                        *remaining -= dt;
                        return;
                    }
                    dt -= *remaining;
                    let ejected = *ejected;
                    self.end_ejection(ejected);
                }
                Phase::Playing => {
                    self.emergency_cooldown = self.emergency_cooldown.saturating_sub(dt);
                    for p in self.players.values_mut() {
                        p.kill_cooldown = p.kill_cooldown.saturating_sub(dt);
                    }
                    self.tick_abilities(dt);
                    self.tick_casts(dt);
                    if self.phase != Phase::Playing {
                        return;
                    }
                    self.tick_combat(dt);
                    if self.phase != Phase::Playing {
                        return;
                    }
                    match &mut self.sabotage {
                        None => self.sabotage_cooldown = self.sabotage_cooldown.saturating_sub(dt),
                        Some(ActiveSabotage {
                            remaining: Some(left),
                            ..
                        }) => {
                            if dt >= *left {
                                self.game_over(Team::Impostors, WinReason::Sabotage);
                            } else {
                                *left -= dt;
                            }
                        }
                        Some(_) => {}
                    }
                    return;
                }
                Phase::Lobby | Phase::GameOver { .. } => return,
            }
        }
    }

    /// Ends the game if a side has won; true if it did.
    fn check_win(&mut self) -> bool {
        let Some((winner, reason)) = self.winner() else {
            return false;
        };
        self.game_over(winner, reason);
        true
    }

    fn game_over(&mut self, winner: Team, reason: WinReason) {
        self.interrupt_all();
        self.clear_all_effects();
        let impostors = (self.players.iter())
            .filter(|(_, p)| p.is_impostor())
            .map(|(&id, _)| id)
            .collect();
        self.sabotage = None;
        self.phase = Phase::GameOver { winner, reason };
        self.out.push(Event::GameOver {
            winner,
            reason,
            impostors,
        });
    }

    fn start_sabotage(&mut self, kind: SabotageKind) {
        let def = self.sabotage_def(kind).expect("validated sabotage");
        let deadline = def.deadline;
        self.sabotage = Some(ActiveSabotage {
            kind,
            remaining: deadline,
            stations: vec![Station::Open; def.stations.len()],
        });
        self.out.push(Event::SabotageStarted { kind, deadline });
    }

    fn fix(&mut self, player: PlayerId, station: usize) {
        let Some(s) = &self.sabotage else { return };
        if !s.kind.needs_simultaneous_hold() {
            self.set_station(station, Station::Fixed);
            self.out.push(Event::StationFixed { station, player });
        } else if s.stations[station] != Station::Held(player) {
            self.release_stations(player);
            self.set_station(station, Station::Held(player));
            self.out.push(Event::StationHeld { station, player });
        }
        let s = self.sabotage.as_ref().expect("still active");
        let done = |st: &Station| match st {
            Station::Fixed => true,
            Station::Held(_) => s.kind.needs_simultaneous_hold(),
            Station::Open => false,
        };
        if s.stations.iter().all(done) {
            self.end_sabotage();
        }
    }

    fn set_station(&mut self, station: usize, state: Station) {
        if let Some(s) = &mut self.sabotage {
            s.stations[station] = state;
        }
    }

    fn end_sabotage(&mut self) {
        if let Some(s) = self.sabotage.take() {
            self.sabotage_cooldown = self.settings.sabotage_cooldown;
            self.out.push(Event::SabotageFixed { kind: s.kind });
        }
    }

    /// Lets go of every station `player` holds.
    fn release_stations(&mut self, player: PlayerId) {
        self.release_where(|station, _| station == Station::Held(player));
    }

    /// Lets go of the stations `player` holds that are now out of their reach.
    fn release_out_of_reach(&mut self, player: PlayerId) {
        let Some(from) = self.players.get(&player).map(|p| p.position) else {
            return;
        };
        let range = self.settings.use_distance;
        self.release_where(|station, at| {
            station == Station::Held(player) && !within(from, at, range)
        });
    }

    fn release_where(&mut self, release: impl Fn(Station, Position) -> bool) {
        let Some(s) = &self.sabotage else { return };
        let kind = s.kind;
        let freed: Vec<usize> = (0..s.stations.len())
            .filter(|&i| release(s.stations[i], self.station_position(kind, i)))
            .collect();
        for station in freed {
            self.set_station(station, Station::Open);
            self.out.push(Event::StationReleased { station });
        }
    }

    fn sabotage_def(&self, kind: SabotageKind) -> Option<&SabotageDef> {
        self.layout.sabotages.iter().find(|s| s.kind == kind)
    }

    fn station_position(&self, kind: SabotageKind, station: usize) -> Position {
        self.sabotage_def(kind)
            .expect("active sabotage is on the map")
            .stations[station]
    }

    fn winner(&self) -> Option<(Team, WinReason)> {
        let living = |role| {
            (self.players.values())
                .filter(|p| p.connected && p.alive && p.role == Some(role))
                .count()
        };
        let (impostors, crew) = (living(Role::Impostor), living(Role::Crewmate));
        if impostors == 0 {
            return Some((Team::Crew, WinReason::ImpostorsEliminated));
        }
        if impostors >= crew {
            return Some((Team::Impostors, WinReason::Outnumbered));
        }
        let (done, total) = self.task_bar();
        (total > 0 && done == total).then_some((Team::Crew, WinReason::TasksCompleted))
    }

    fn push_task_bar(&mut self) {
        let (done, total) = self.task_bar();
        self.out.push(Event::TaskBar { done, total });
    }

    fn set_host(&mut self, host: Option<PlayerId>) {
        self.host = host;
        self.out.push(Event::HostChanged { host });
    }

    fn task_done(&self, t: &PlayerTask) -> bool {
        t.step >= self.layout.tasks[t.task.0].steps.len()
    }

    /// A seat `validate` has already vouched for.
    fn seat(&mut self, id: PlayerId) -> &mut Player {
        self.players.get_mut(&id).expect("validated player")
    }

    fn in_lobby(&self) -> Result<(), Reject> {
        match self.phase {
            Phase::Lobby => Ok(()),
            _ => Err(Reject::WrongPhase),
        }
    }

    /// Between Start and GameOver.
    fn in_progress(&self) -> bool {
        matches!(
            self.phase,
            Phase::Playing | Phase::Meeting(_) | Phase::Ejecting { .. }
        )
    }

    fn playing(&self) -> Result<(), Reject> {
        match self.phase {
            Phase::Playing => Ok(()),
            _ => Err(Reject::WrongPhase),
        }
    }

    fn is_host(&self, id: PlayerId) -> Result<(), Reject> {
        self.connected(id)?;
        if self.host == Some(id) {
            Ok(())
        } else {
            Err(Reject::NotHost)
        }
    }

    fn alive(&self, id: PlayerId) -> Result<&Player, Reject> {
        let p = self.connected(id)?;
        if p.alive {
            Ok(p)
        } else {
            Err(Reject::Dead)
        }
    }

    fn connected(&self, id: PlayerId) -> Result<&Player, Reject> {
        self.players
            .get(&id)
            .filter(|p| p.connected)
            .ok_or(Reject::UnknownPlayer)
    }

    /// The player, their task and its current step, if `player` may work at it now.
    fn workable_step(
        &self,
        player: PlayerId,
        task: TaskId,
    ) -> Result<(&Player, &PlayerTask, &TaskStep), Reject> {
        self.playing()?;
        let p = self.connected(player)?;
        // Ghost crewmates still work; a ghost impostor has no one left to fool.
        if !p.alive && p.is_impostor() {
            return Err(Reject::Dead);
        }
        if p.is_incapacitated() {
            return Err(Reject::Incapacitated);
        }
        if p.casting.is_some() || p.spell_cast.is_some() {
            return Err(Reject::AlreadyCasting);
        }
        let t = (p.tasks.iter())
            .find(|t| t.task == task)
            .ok_or(Reject::TaskNotAssigned)?;
        let step = (self.layout.tasks[task.0].steps.get(t.step)).ok_or(Reject::TaskDone)?;
        Ok((p, t, step))
    }

    fn in_reach(&self, from: Position, to: Position, range: f32) -> Result<(), Reject> {
        if within(from, to, range) {
            Ok(())
        } else {
            Err(Reject::OutOfRange)
        }
    }
}

/// A strict plurality ejects; a shared top count (players, or a player and skip) is a tie; skip
/// leading, or no votes at all, skips.
fn tally(votes: &[(PlayerId, Vote)]) -> VoteOutcome {
    let mut counts: BTreeMap<PlayerId, usize> = BTreeMap::new();
    let mut skips = 0;
    for &(_, vote) in votes {
        match vote {
            Vote::Player(target) => *counts.entry(target).or_default() += 1,
            Vote::Skip => skips += 1,
        }
    }
    let top = counts.values().copied().max().unwrap_or(0);
    if skips > top || top == 0 {
        return VoteOutcome::Skipped;
    }
    let mut leaders = counts.iter().filter(|(_, &c)| c == top).map(|(&id, _)| id);
    match (leaders.next(), leaders.next()) {
        (Some(only), None) if skips < top => VoteOutcome::Ejected(only),
        _ => VoteOutcome::Tie,
    }
}
