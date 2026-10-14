//! [`Game`]: the whole match as one state machine. [`Game::apply`] validates a [`Command`] against
//! the current state and either refuses it (state unchanged) or applies it and returns the events.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::protocol::{
    Command, Event, MeetingReason, PlayerId, Reject, Role, TaskId, Team, Vote, VoteOutcome,
    WinReason,
};
use crate::rng::SplitMix64;
use crate::settings::{max_impostors, within, Layout, Position, Settings, TaskKind};

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
        }
    }

    pub fn is_impostor(&self) -> bool {
        self.role == Some(Role::Impostor)
    }

    pub fn is_crewmate(&self) -> bool {
        self.role == Some(Role::Crewmate)
    }
}

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
    /// The ejection reveal; ejected is already a ghost.
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

/// The match: lobby, roles, tasks, kills, meetings, votes, sabotage and the win check.
#[derive(Clone, Debug)]
pub struct Game {
    settings: Settings,
    layout: Layout,
    phase: Phase,
    host: Option<PlayerId>,
    players: BTreeMap<PlayerId, Player>,
    bodies: Vec<Body>,
    emergency_cooldown: Duration,
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
                if !p.is_crewmate() {
                    return Err(Reject::NotCrewmate);
                }
                let t = p
                    .tasks
                    .iter()
                    .find(|t| t.task == task)
                    .ok_or(Reject::TaskNotAssigned)?;
                let at = self.layout.tasks[task.0]
                    .steps
                    .get(t.step)
                    .ok_or(Reject::TaskDone)?;
                self.in_reach(p.position, *at, self.settings.use_distance)
            }
            Command::Kill { killer, victim } => {
                self.playing()?;
                let k = self.alive(killer)?;
                if !k.is_impostor() {
                    return Err(Reject::NotImpostor);
                }
                let v = (self.players.get(&victim))
                    .filter(|v| v.connected && v.alive && v.is_crewmate())
                    .ok_or(Reject::InvalidTarget)?;
                if !k.kill_cooldown.is_zero() {
                    return Err(Reject::OnCooldown);
                }
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
            Command::ReturnToLobby { by } => {
                if !matches!(self.phase, Phase::GameOver { .. }) {
                    return Err(Reject::WrongPhase);
                }
                self.is_host(by)
            }
            Command::Tick { .. } => Ok(()),
            _ => Err(Reject::WrongPhase),
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
            Command::Move { player, position } => self.seat(player).position = position,
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
            Command::ReturnToLobby { .. } => self.return_to_lobby(),
            Command::Tick { dt } => self.tick(dt),
            _ => {}
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
        self.phase = Phase::Playing;
        self.push_task_bar();
    }

    fn use_task(&mut self, player: PlayerId, task: TaskId) {
        let steps = self.layout.tasks[task.0].steps.len();
        let p = self.seat(player);
        let t = p.tasks.iter_mut().find(|t| t.task == task).unwrap();
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
        self.out.push(Event::Killed { killer, victim, at });
        self.check_win();
    }

    fn leave(&mut self, player: PlayerId) {
        if self.phase == Phase::Lobby {
            self.players.remove(&player);
        } else {
            self.seat(player).connected = false;
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
        self.phase = Phase::Lobby;
        self.out.push(Event::ReturnedToLobby);
    }

    /// Runs the clock; a long dt carries through every stage boundary it crosses, in order.
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
        let impostors = (self.players.iter())
            .filter(|(_, p)| p.is_impostor())
            .map(|(&id, _)| id)
            .collect();
        self.phase = Phase::GameOver { winner, reason };
        self.out.push(Event::GameOver {
            winner,
            reason,
            impostors,
        });
        true
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
