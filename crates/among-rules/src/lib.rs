//! Among Azeroth's game as a pure state machine: lobby, roles, tasks, kills, bodies, meetings, votes,
//! ejections, sabotage and the win check.
//!
//! No Bevy, no network, no game data, no clock and no OS randomness: the server feeds
//! [`Command`]s (including [`Command::Tick`] with the elapsed time, and a seed in
//! [`Command::Start`]) into [`Game::apply`] and routes the [`Event`]s it returns. Players are opaque
//! [`PlayerId`]s; positions are WoW world coordinates in yards.
//!
//! ```
//! use std::time::Duration;
//! use among_rules::*;
//!
//! let mut game = Game::new(Settings::default(), Layout::default());
//! for id in 1..=4 {
//!     game.apply(Command::Join { player: PlayerId(id) }).unwrap();
//! }
//! let events = game.apply(Command::Start { by: PlayerId(1), seed: 42 }).unwrap();
//! for event in &events {
//!     match event.private_to() {
//!         Some(_player) => {} // that player's session only: their role, their tasks
//!         None => {}          // everyone
//!     }
//! }
//! // A refused command changes nothing and says why.
//! let kill = Command::Kill { killer: PlayerId(1), victim: PlayerId(2) };
//! assert!(game.apply(kill).is_err());
//! // Each server tick:
//! game.apply(Command::Tick { dt: Duration::from_millis(100) }).unwrap();
//! assert_eq!(game.phase(), &Phase::Playing);
//! ```

mod game;
mod protocol;
mod rng;
mod settings;

pub use game::{
    Actions, ActiveSabotage, Body, Game, Meeting, MeetingStage, Phase, Player, PlayerTask, Station,
};
pub use protocol::{
    Command, Event, MeetingReason, PlayerId, Reject, Role, TaskId, Team, Vote, VoteOutcome,
    WinReason,
};
pub use settings::{
    distance_squared, max_impostors, within, Layout, Position, SabotageDef, SabotageKind, Settings,
    TaskDef, TaskKind,
};
