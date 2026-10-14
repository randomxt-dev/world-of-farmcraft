//! Among Azeroth's game as a pure state machine: lobby, roles, tasks, kills, bodies, meetings, votes,
//! ejections, sabotage and the win check.
//!
//! No Bevy, no network, no game data, no clock and no OS randomness: the server feeds
//! [`Command`]s (including [`Command::Tick`] with the elapsed time, and a seed in
//! [`Command::Start`]) into [`Game::apply`] and routes the [`Event`]s it returns. Players are opaque
//! [`PlayerId`]s; positions are WoW world coordinates in yards.

mod game;
mod protocol;
mod rng;
mod settings;

pub use game::{Body, Game, Meeting, MeetingStage, Phase, Player, PlayerTask};
pub use protocol::{
    Command, Event, MeetingReason, PlayerId, Reject, Role, TaskId, Team, Vote, VoteOutcome,
    WinReason,
};
pub use settings::{
    max_impostors, within, Layout, Position, SabotageDef, SabotageKind, Settings, TaskDef, TaskKind,
};
