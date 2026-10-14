//! The game's interface: [`Command`]s in, [`Event`]s out, a [`Reject`] when a command is not legal
//! now. Plain data, so a server can log, replay and route them.

use std::fmt;
use std::time::Duration;

use crate::settings::{Position, SabotageKind, Settings};

/// A player, opaque to the rules; the server maps it to a WoW guid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub u64);

/// An index into [`crate::Layout::tasks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Crewmate,
    Impostor,
}

/// The side that wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Team {
    Crew,
    Impostors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WinReason {
    /// Every crewmate task is done.
    TasksCompleted,
    /// No impostor is alive and connected.
    ImpostorsEliminated,
    /// Living impostors are at least as many as living crewmates.
    Outnumbered,
    /// A critical sabotage reached its deadline.
    Sabotage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeetingReason {
    /// A body was found; `body` is the victim.
    Report {
        body: PlayerId,
    },
    Emergency,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Vote {
    Player(PlayerId),
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoteOutcome {
    Ejected(PlayerId),
    /// The top count is shared, between players or with skip: nobody leaves.
    Tie,
    /// Skip led, or nobody voted.
    Skipped,
}

/// Everything the server can ask of the game. Positions are WoW world coordinates in yards.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Join {
        player: PlayerId,
    },
    Leave {
        player: PlayerId,
    },
    /// The host replaces the settings while in the lobby.
    Configure {
        by: PlayerId,
        settings: Settings,
    },
    /// The host starts; `seed` drives role and task assignment.
    Start {
        by: PlayerId,
        seed: u64,
    },
    Move {
        player: PlayerId,
        position: Position,
    },
    UseTask {
        player: PlayerId,
        task: TaskId,
    },
    Kill {
        killer: PlayerId,
        victim: PlayerId,
    },
    Report {
        player: PlayerId,
        body: PlayerId,
    },
    CallEmergency {
        player: PlayerId,
    },
    Vote {
        voter: PlayerId,
        vote: Vote,
    },
    Sabotage {
        player: PlayerId,
        kind: SabotageKind,
    },
    /// Fix (or, for a hold sabotage, hold) one station of the active sabotage.
    Fix {
        player: PlayerId,
        station: usize,
    },
    /// Let go of any station the player holds.
    ReleaseFix {
        player: PlayerId,
    },
    ReturnToLobby {
        by: PlayerId,
    },
    Tick {
        dt: Duration,
    },
}

/// What happened. Most are for everyone; [`Event::private_to`] names the one player a private event
/// is for. [`Event::Killed`] carries the killer for the server alone: the world shows only a body.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Joined {
        player: PlayerId,
    },
    Left {
        player: PlayerId,
    },
    HostChanged {
        host: Option<PlayerId>,
    },
    SettingsChanged,
    GameStarted {
        players: usize,
        impostors: usize,
    },
    /// Private. An impostor learns the other impostors; a crewmate's list is empty.
    RoleAssigned {
        player: PlayerId,
        role: Role,
        fellow_impostors: Vec<PlayerId>,
    },
    /// Private. An impostor's list is a cover story: it never progresses.
    TasksAssigned {
        player: PlayerId,
        tasks: Vec<TaskId>,
    },
    /// Private. `step` of `steps` are now done.
    TaskProgress {
        player: PlayerId,
        task: TaskId,
        step: usize,
        steps: usize,
    },
    /// Private.
    TaskCompleted {
        player: PlayerId,
        task: TaskId,
    },
    /// `player` began a task cast of `time`. Public: the world shows the cast; an impostor's
    /// cover-task cast looks the same and never progresses.
    CastStarted {
        player: PlayerId,
        task: TaskId,
        time: Duration,
    },
    /// The cast ran out; any progress follows as a private [`Event::TaskProgress`].
    CastFinished {
        player: PlayerId,
    },
    /// The cast broke: the caster moved, a meeting began, or the caster died or left.
    CastInterrupted {
        player: PlayerId,
    },
    /// Crew tasks done of the total still counted (connected crewmates, living or dead).
    TaskBar {
        done: usize,
        total: usize,
    },
    /// `victim` is dead with a body at `at`, and the killer now stands at `at`.
    Killed {
        killer: PlayerId,
        victim: PlayerId,
        at: Position,
    },
    MeetingCalled {
        caller: PlayerId,
        reason: MeetingReason,
        discussion: Duration,
    },
    /// Every body leaves the map when a meeting starts.
    BodiesCleared,
    VotingOpened {
        voting: Duration,
    },
    /// `voter` has voted; the choice stays secret until [`Event::VotingClosed`].
    VoteCast {
        voter: PlayerId,
    },
    /// A vote for a player who left is handed back; `voter` may vote again.
    VoteReturned {
        voter: PlayerId,
    },
    VotingClosed {
        votes: Vec<(PlayerId, Vote)>,
        outcome: VoteOutcome,
    },
    /// `player` is now dead. With confirm ejects on, `impostor` and `impostors_left` say what they
    /// were and how many impostors remain.
    Ejected {
        player: PlayerId,
        impostor: Option<bool>,
        impostors_left: Option<usize>,
    },
    /// The ejection reveal is over; play resumes unless [`Event::GameOver`] follows.
    MeetingEnded,
    SabotageStarted {
        kind: SabotageKind,
        deadline: Option<Duration>,
    },
    StationHeld {
        station: usize,
        player: PlayerId,
    },
    StationReleased {
        station: usize,
    },
    StationFixed {
        station: usize,
        player: PlayerId,
    },
    /// Fixed by the crew, or ended by a meeting (critical sabotages only).
    SabotageFixed {
        kind: SabotageKind,
    },
    GameOver {
        winner: Team,
        reason: WinReason,
        impostors: Vec<PlayerId>,
    },
    ReturnedToLobby,
}

impl Event {
    /// The one player a private event is for; `None` for an event everyone may see.
    pub fn private_to(&self) -> Option<PlayerId> {
        match *self {
            Event::RoleAssigned { player, .. }
            | Event::TasksAssigned { player, .. }
            | Event::TaskProgress { player, .. }
            | Event::TaskCompleted { player, .. } => Some(player),
            _ => None,
        }
    }
}

/// Why a command was refused; the game is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reject {
    /// Not legal in the current phase (e.g. a kill during a meeting).
    WrongPhase,
    UnknownPlayer,
    AlreadyJoined,
    LobbyFull,
    NotHost,
    NotEnoughPlayers,
    Dead,
    NotImpostor,
    NotCrewmate,
    /// The other player is not a legal target (dead, gone, an impostor, oneself).
    InvalidTarget,
    OnCooldown,
    OutOfRange,
    TaskNotAssigned,
    TaskDone,
    NoSuchBody,
    NoEmergencyMeetingsLeft,
    SabotageActive,
    NoSabotage,
    NoSuchSabotage,
    NoSuchStation,
    StationFixed,
    StationBusy,
    AlreadyVoted,
    AlreadyCasting,
}

impl fmt::Display for Reject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Reject::WrongPhase => "not now",
            Reject::UnknownPlayer => "no such player",
            Reject::AlreadyJoined => "already in the lobby",
            Reject::LobbyFull => "the lobby is full",
            Reject::NotHost => "only the host can do that",
            Reject::NotEnoughPlayers => "not enough players",
            Reject::Dead => "the dead cannot do that",
            Reject::NotImpostor => "only impostors can do that",
            Reject::NotCrewmate => "only crewmates can do that",
            Reject::InvalidTarget => "not a valid target",
            Reject::OnCooldown => "on cooldown",
            Reject::OutOfRange => "too far away",
            Reject::TaskNotAssigned => "not your task",
            Reject::TaskDone => "task already done",
            Reject::NoSuchBody => "no such body",
            Reject::NoEmergencyMeetingsLeft => "no emergency meetings left",
            Reject::SabotageActive => "a sabotage is active",
            Reject::NoSabotage => "nothing is sabotaged",
            Reject::NoSuchSabotage => "this map has no such sabotage",
            Reject::NoSuchStation => "no such station",
            Reject::StationFixed => "station already fixed",
            Reject::StationBusy => "someone else is at that station",
            Reject::AlreadyVoted => "already voted",
            Reject::AlreadyCasting => "already busy",
        })
    }
}

impl std::error::Error for Reject {}
