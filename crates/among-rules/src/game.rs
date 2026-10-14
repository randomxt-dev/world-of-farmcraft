//! [`Game`]: the whole match as one state machine. [`Game::apply`] validates a [`Command`] against
//! the current state and either refuses it (state unchanged) or applies it and returns the events.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::protocol::{
    Command, Event, MeetingReason, PlayerId, Reject, Role, TaskId, Team, Vote, VoteOutcome,
    WinReason,
};
use crate::rng::SplitMix64;
use crate::settings::{
    distance_squared, max_impostors, within, Layout, Position, SabotageDef, SabotageKind, Settings,
    StepAction, TaskKind,
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
}

impl Player {
    fn new(emergencies: u32) -> Self {
        Self {
            position: [0.0; 3],
            role: None,
            alive: true,
            connected: true,
            tasks: Vec::new(),
            kill_cooldown: Duration::ZERO,
            emergencies_left: emergencies,
            casting: None,
        }
    }

    pub fn is_impostor(&self) -> bool {
        self.role == Some(Role::Impostor)
    }

    pub fn is_crewmate(&self) -> bool {
        self.role == Some(Role::Crewmate)
    }
}

/// A task cast under way: it completes when `remaining` runs out, and breaks if the caster moves
/// off `from`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Casting {
    pub task: TaskId,
    pub remaining: Duration,
    pub from: Position,
}

/// How far a caster may drift before the cast breaks; WoW breaks a cast on any step, not a turn.
const CAST_DRIFT: f32 = 0.1;

/// A task in a player's list and how many of its steps are done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerTask {
    pub task: TaskId,
    pub step: usize,
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
    pub use_: bool,
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
    out: Vec<Event>,
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
            out: Vec::new(),
        }
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

    /// What a Use button would do for `player` right now: fix the nearest sabotage station, do
    /// the nearest task step, or press the emergency button, whichever legal one is closest.
    pub fn use_target(&self, player: PlayerId) -> Option<Command> {
        let p = self.players.get(&player)?;
        let mut spots: Vec<(Position, Command)> = Vec::new();
        if let Some(s) = &self.sabotage {
            for station in 0..s.stations.len() {
                let at = self.station_position(s.kind, station);
                spots.push((at, Command::Fix { player, station }));
            }
        }
        for t in &p.tasks {
            if let Some(step) = self.layout.tasks[t.task.0].steps.get(t.step) {
                spots.push((
                    step.at,
                    Command::UseTask {
                        player,
                        task: t.task,
                    },
                ));
            }
        }
        spots.push((
            self.layout.emergency_button,
            Command::CallEmergency { player },
        ));
        self.nearest(p.position, spots, Clone::clone)
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
            use_: self.use_target(player).is_some(),
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
            Command::Join { player } => {
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
                self.playing()?;
                let p = self.connected(player)?;
                // Ghost crewmates still work; a ghost impostor has no one left to fool.
                if !p.alive && p.is_impostor() {
                    return Err(Reject::Dead);
                }
                if p.casting.is_some() {
                    return Err(Reject::AlreadyCasting);
                }
                let t = p
                    .tasks
                    .iter()
                    .find(|t| t.task == task)
                    .ok_or(Reject::TaskNotAssigned)?;
                let step = self.layout.tasks[task.0]
                    .steps
                    .get(t.step)
                    .ok_or(Reject::TaskDone)?;
                self.in_reach(p.position, step.at, self.settings.use_distance)
            }
            Command::Kill { killer, victim } => {
                self.playing()?;
                let k = self.alive(killer)?;
                if !k.is_impostor() {
                    return Err(Reject::NotImpostor);
                }
                // Cooldown before target: a refused press says "on cooldown", aimed or not.
                if !k.kill_cooldown.is_zero() {
                    return Err(Reject::OnCooldown);
                }
                let v = (self.players.get(&victim))
                    .filter(|v| v.connected && v.alive && v.is_crewmate())
                    .ok_or(Reject::InvalidTarget)?;
                self.in_reach(k.position, v.position, self.settings.kill_distance)
            }
            Command::Report { player, body } => {
                self.playing()?;
                let p = self.alive(player)?;
                let b = (self.bodies.iter())
                    .find(|b| b.victim == body)
                    .ok_or(Reject::NoSuchBody)?;
                self.in_reach(p.position, b.position, self.settings.report_distance)
            }
            Command::CallEmergency { player } => {
                self.playing()?;
                let p = self.alive(player)?;
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
                if !self.connected(player)?.is_impostor() {
                    return Err(Reject::NotImpostor);
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
                let p = self.alive(player)?;
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
            Command::Join { player } => {
                self.players
                    .insert(player, Player::new(self.settings.emergency_meetings));
                self.out.push(Event::Joined { player });
                if self.host.is_none() {
                    self.set_host(Some(player));
                }
            }
            Command::Leave { player } => self.leave(player),
            Command::Configure { settings, .. } => {
                self.settings = settings;
                for p in self.players.values_mut() {
                    p.emergencies_left = self.settings.emergency_meetings;
                }
                self.out.push(Event::SettingsChanged);
            }
            Command::Start { seed, .. } => self.start(seed),
            Command::Move { player, position } => {
                let p = self.seat(player);
                p.position = position;
                if p.casting
                    .is_some_and(|c| !within(c.from, position, CAST_DRIFT))
                {
                    self.interrupt(player);
                }
                self.release_out_of_reach(player);
            }
            Command::UseTask { player, task } => self.use_task(player, task),
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
            Command::Fix { player, station } => self.fix(player, station),
            Command::ReleaseFix { player } => self.release_stations(player),
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
        let layout = &self.layout;
        let mut draw = |kind: TaskKind, count: usize| -> Vec<TaskId> {
            let mut ids: Vec<TaskId> = (layout.tasks.iter().enumerate())
                .filter(|(_, t)| t.kind == kind && !t.steps.is_empty())
                .map(|(i, _)| TaskId(i))
                .collect();
            rng.shuffle(&mut ids);
            ids.truncate(count);
            ids
        };
        let common = draw(TaskKind::Common, self.settings.common_tasks);
        let lists: Vec<(PlayerId, Vec<TaskId>)> = (self.players.keys())
            .map(|&id| {
                let mut tasks = common.clone();
                tasks.extend(draw(TaskKind::Long, self.settings.long_tasks));
                tasks.extend(draw(TaskKind::Short, self.settings.short_tasks));
                (id, tasks)
            })
            .collect();

        self.out.push(Event::GameStarted {
            players: n,
            impostors,
        });
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
                .map(|&task| PlayerTask { task, step: 0 })
                .collect();
            p.kill_cooldown = self.settings.initial_kill_cooldown;
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
    }

    /// Start the step's cast, or do the step at once if it takes no time.
    fn use_task(&mut self, player: PlayerId, task: TaskId) {
        let p = &self.players[&player];
        let t = p
            .tasks
            .iter()
            .find(|t| t.task == task)
            .expect("validated task");
        let StepAction::Cast(time) = self.layout.tasks[task.0].steps[t.step].action;
        if time.is_zero() {
            self.advance(player, task);
            return;
        }
        let from = p.position;
        self.seat(player).casting = Some(Casting {
            task,
            remaining: time,
            from,
        });
        self.out.push(Event::CastStarted { player, task, time });
    }

    /// Break every cast: play stops for a meeting or the end of the game.
    fn interrupt_all(&mut self) {
        let casters: Vec<PlayerId> = (self.players.iter())
            .filter(|(_, p)| p.casting.is_some())
            .map(|(&id, _)| id)
            .collect();
        for player in casters {
            self.interrupt(player);
        }
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
                finished.push((id, c.task));
                p.casting = None;
            } else {
                c.remaining -= dt;
            }
        }
        for (player, task) in finished {
            if self.phase != Phase::Playing {
                return;
            }
            self.out.push(Event::CastFinished { player });
            self.advance(player, task);
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
        }
    }

    fn kill(&mut self, killer: PlayerId, victim: PlayerId) {
        self.interrupt(victim);
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
        self.check_win();
    }

    fn leave(&mut self, player: PlayerId) {
        if self.phase == Phase::Lobby {
            self.players.remove(&player);
        } else {
            self.interrupt(player);
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

    fn end_ejection(&mut self) {
        self.out.push(Event::MeetingEnded);
        self.phase = Phase::Playing;
        if self.check_win() {
            return;
        }
        let cooldown = self.settings.kill_cooldown;
        for p in self.players.values_mut().filter(|p| p.is_impostor()) {
            p.kill_cooldown = cooldown;
        }
        self.emergency_cooldown = self.settings.emergency_cooldown;
    }

    fn return_to_lobby(&mut self) {
        self.players.retain(|_, p| p.connected);
        self.bodies.clear();
        for p in self.players.values_mut() {
            *p = Player {
                position: p.position,
                ..Player::new(self.settings.emergency_meetings)
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
                Phase::Ejecting { remaining, .. } => {
                    if dt < *remaining {
                        *remaining -= dt;
                        return;
                    }
                    dt -= *remaining;
                    self.end_ejection();
                }
                Phase::Playing => {
                    self.emergency_cooldown = self.emergency_cooldown.saturating_sub(dt);
                    for p in self.players.values_mut() {
                        p.kill_cooldown = p.kill_cooldown.saturating_sub(dt);
                    }
                    self.tick_casts(dt);
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
