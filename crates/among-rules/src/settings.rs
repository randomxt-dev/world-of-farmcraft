//! What a host can tune ([`Settings`]) and what a map provides ([`Layout`]): where the tasks, the
//! emergency button, the sabotage stations and the monsters stand, in WoW world coordinates (yards).

use std::time::Duration;

use crate::abilities::Abilities;
use crate::classes::Kits;
use crate::protocol::CreatureId;

/// A point in WoW world coordinates (x, y, z), in yards.
pub type Position = [f32; 3];

/// Whether `a` and `b` are at most `range` yards apart in 3D.
pub fn within(a: Position, b: Position, range: f32) -> bool {
    distance_squared(a, b) <= range * range
}

/// Squared 3D distance in yards, for ranking by nearness.
pub fn distance_squared(a: Position, b: Position) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// The host's rules for one game. Distances are WoW yards.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Fewest players `Start` accepts; never below 3, so one impostor faces two crewmates.
    pub min_players: usize,
    pub max_players: usize,
    /// Asked-for impostors; [`max_impostors`] caps it by player count.
    pub impostors: usize,
    /// Between kills, and after every meeting.
    pub kill_cooldown: Duration,
    /// Before an impostor's first kill of the game.
    pub initial_kill_cooldown: Duration,
    pub kill_distance: f32,
    pub report_distance: f32,
    /// Reach for tasks, the emergency button and sabotage stations.
    pub use_distance: f32,
    /// Emergency meetings each player may call per game.
    pub emergency_meetings: u32,
    /// After the game starts and after every meeting, before the button works.
    pub emergency_cooldown: Duration,
    pub discussion_time: Duration,
    pub voting_time: Duration,
    /// The ejection reveal between the vote result and play resuming.
    pub ejection_time: Duration,
    /// Whether an ejection reveals if the player was an impostor.
    pub confirm_ejects: bool,
    pub common_tasks: usize,
    pub long_tasks: usize,
    pub short_tasks: usize,
    /// At game start and after each sabotage ends, before impostors may sabotage again.
    pub sabotage_cooldown: Duration,
    /// What each class fights with.
    pub kits: Kits,
    /// Where auto-attack swings land, both ways.
    pub melee_range: f32,
    /// Health a living player regains per second while out of combat.
    pub health_regen: u32,
    /// Each class's ability on players.
    pub abilities: Abilities,
    /// Before anyone's first ability of the game.
    pub initial_ability_cooldown: Duration,
    /// How close a step must come to a freezing trap to spring it.
    pub trap_radius: f32,
    /// How long a freezing trap waits before it crumbles.
    pub trap_lifetime: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            min_players: 4,
            max_players: 15,
            impostors: 2,
            kill_cooldown: Duration::from_secs(25),
            initial_kill_cooldown: Duration::from_secs(10),
            // 5 yd is WoW's melee range and interaction distance (vmangos `INTERACTION_DISTANCE`).
            kill_distance: 5.0,
            report_distance: 8.0,
            use_distance: 5.0,
            emergency_meetings: 1,
            emergency_cooldown: Duration::from_secs(15),
            discussion_time: Duration::from_secs(15),
            voting_time: Duration::from_secs(120),
            ejection_time: Duration::from_secs(6),
            confirm_ejects: true,
            common_tasks: 1,
            long_tasks: 1,
            short_tasks: 2,
            sabotage_cooldown: Duration::from_secs(30),
            kits: Kits::default(),
            melee_range: 5.0,
            health_regen: 2,
            abilities: Abilities::default(),
            initial_ability_cooldown: Duration::from_secs(10),
            // A 1.12 freezing trap's own trigger radius, in yards.
            trap_radius: 2.5,
            trap_lifetime: Duration::from_secs(60),
        }
    }
}

/// Most impostors a game of `players` allows, Among Us's table: 4-6 players 1, 7-8 2, 9+ 3.
pub fn max_impostors(players: usize) -> usize {
    match players {
        0..=6 => 1,
        7..=8 => 2,
        _ => 3,
    }
}

/// A map's fixed points: what a crewmate walks to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub emergency_button: Position,
    /// Indexed by [`crate::TaskId`].
    pub tasks: Vec<TaskDef>,
    /// At most one per [`SabotageKind`]; the first of a kind wins.
    pub sabotages: Vec<SabotageDef>,
    /// Indexed by [`crate::CreatureId`]: each spawns at game start and respawns after dying.
    pub creatures: Vec<CreatureDef>,
}

/// A monster at a task spot that fights back.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatureDef {
    pub name: String,
    /// Home: where it spawns, and where it returns when it gives up.
    pub at: Position,
    pub health: u32,
    /// Per swing, landing within melee range.
    pub damage: u32,
    pub swing: Duration,
    /// It turns on the nearest living player this close.
    pub aggro: f32,
    /// It gives up once its target is this far from home.
    pub leash: f32,
    /// Chase speed, yards per second.
    pub speed: f32,
    pub respawn: Duration,
}

impl CreatureDef {
    /// A young-wolf-like beast at `at`: weak alone, dangerous to ignore.
    pub fn beast(name: &str, at: Position) -> Self {
        Self {
            name: name.into(),
            at,
            health: 40,
            damage: 5,
            swing: Duration::from_secs(2),
            aggro: 10.0,
            leash: 30.0,
            speed: 6.0,
            respawn: Duration::from_secs(20),
        }
    }
}

/// One task: a named sequence of places to use, one step each.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskDef {
    pub name: String,
    pub kind: TaskKind,
    /// A task with no steps is never assigned.
    pub steps: Vec<TaskStep>,
}

/// One step of a task: where it is done and what doing it takes.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskStep {
    pub at: Position,
    pub action: StepAction,
}

impl TaskStep {
    /// A step done by a cast of `secs` seconds at `at` (gathering a node, crafting at a station).
    pub fn cast(at: Position, secs: f32) -> Self {
        Self {
            at,
            action: StepAction::Cast(Duration::from_secs_f32(secs)),
        }
    }

    /// A step done by picking the one node at `at` (a herb, a vein), a cast of `secs` seconds;
    /// once a crewmate picks it, it is gone for everyone, as a Gather step's nodes are.
    pub fn pick(at: Position, secs: f32) -> Self {
        Self {
            at,
            action: StepAction::Gather {
                nodes: vec![at],
                cast: Duration::from_secs_f32(secs),
                count: 1,
            },
        }
    }

    /// A step done the moment it is used.
    pub fn instant(at: Position) -> Self {
        Self {
            at,
            action: StepAction::Cast(Duration::ZERO),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StepAction {
    /// A cast of this long, standing still; zero completes on use.
    Cast(Duration),
    /// Gather `count` of these nodes (herbs, ore), a cast of `cast` each. A crewmate's pick takes
    /// the node from everyone until the lobby, so each holder needs only their share of the spots
    /// ([`crate::Game::gather_need`]). At most 64 nodes.
    Gather {
        nodes: Vec<Position>,
        cast: Duration,
        count: u32,
    },
    /// Kill `count` of these creatures: kills you tapped (damaged first) count.
    Slay {
        creatures: Vec<CreatureId>,
        count: u32,
    },
}

/// Common tasks are the same for every player in a game; short and long ones are drawn per player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Common,
    Short,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SabotageKind {
    /// Crew vision drops until every station is fixed.
    Lights,
    /// Task lists hide until every station is fixed.
    Comms,
    /// Critical: every station fixed once before the deadline.
    Oxygen,
    /// Critical: every station held at the same time before the deadline.
    Reactor,
}

impl SabotageKind {
    /// Whether a fix needs every station held at once rather than each fixed in turn.
    pub fn needs_simultaneous_hold(self) -> bool {
        self == Self::Reactor
    }
}

/// A sabotage this map offers and where it is fixed.
#[derive(Clone, Debug, PartialEq)]
pub struct SabotageDef {
    pub kind: SabotageKind,
    pub stations: Vec<Position>,
    /// `Some` makes it critical: unfixed at the deadline, the impostors win.
    pub deadline: Option<Duration>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_is_inclusive_and_3d() {
        assert!(within([0.0, 0.0, 0.0], [3.0, 4.0, 0.0], 5.0));
        assert!(!within([0.0, 0.0, 0.0], [3.0, 4.0, 0.1], 5.0));
        assert!(!within([0.0, 0.0, 0.0], [0.0, 0.0, 5.1], 5.0));
    }

    #[test]
    fn impostor_cap_follows_player_count() {
        let caps: Vec<_> = (4..=10).map(max_impostors).collect();
        assert_eq!(caps, [1, 1, 1, 2, 2, 3, 3]);
    }
}
