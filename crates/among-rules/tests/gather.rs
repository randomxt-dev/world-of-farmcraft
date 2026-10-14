//! Gather steps: pick `count` of a step's nodes, a cast each; a picked node is spent for you.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

/// A 5-player game on [`gather_layout`]; everyone's first task is the shared gathering one.
fn meadow_game() -> (Game, PlayerId, Vec<PlayerId>, TaskId) {
    let g = started_on(gather_layout(), instant(), 5, 3);
    let imp = impostors(&g)[0];
    let crew = crew(&g);
    let task = g.player(crew[0]).unwrap().tasks[0].task;
    assert_eq!(g.layout().tasks[task.0].kind, TaskKind::Common);
    (g, imp, crew, task)
}

fn pick(g: &mut Game, player: PlayerId, task: TaskId, at: Position) -> Vec<Event> {
    put(g, player, at);
    let started = ok(g, Command::UseTask { player, task });
    assert_eq!(
        started,
        [Event::CastStarted {
            player,
            task,
            time: Duration::from_secs(2)
        }]
    );
    tick(g, 2.0)
}

#[test]
fn each_node_counts_once_and_the_count_finishes_the_step() {
    let (mut g, _, crew, task) = meadow_game();
    let c = crew[0];
    let events = pick(&mut g, c, task, MEADOW[0]);
    assert_eq!(
        events,
        [
            Event::CastFinished { player: c },
            Event::GatherProgress {
                player: c,
                task,
                picked: 1,
                count: 3
            },
        ]
    );
    // The same node again: nothing left to pick within reach.
    assert_eq!(
        rejected(&mut g, Command::UseTask { player: c, task }),
        Reject::OutOfRange
    );
    pick(&mut g, c, task, MEADOW[2]);
    let events = pick(&mut g, c, task, MEADOW[3]);
    assert!(events.contains(&Event::GatherProgress {
        player: c,
        task,
        picked: 3,
        count: 3
    }));
    assert!(
        events.contains(&Event::TaskCompleted { player: c, task }),
        "{events:?}"
    );
    // Spent nodes are per player: another crewmate still finds MEADOW[0] fresh.
    pick(&mut g, crew[1], task, MEADOW[0]);
}

#[test]
fn between_two_nodes_the_nearer_unpicked_one_is_gathered() {
    let (mut g, _, crew, task) = meadow_game();
    let c = crew[0];
    pick(&mut g, c, task, MEADOW[1]);
    // 4 yd from MEADOW[1] (picked) and 6 yd from MEADOW[0]: out of reach of the fresh one.
    put(&mut g, c, [6.0, 700.0, 0.0]);
    assert_eq!(
        rejected(&mut g, Command::UseTask { player: c, task }),
        Reject::OutOfRange
    );
    // 4 yd from MEADOW[0]: that one.
    put(&mut g, c, [4.0, 700.0, 0.0]);
    ok(&mut g, Command::UseTask { player: c, task });
    assert_eq!(g.player(c).unwrap().casting.unwrap().node, Some(0));
}

#[test]
fn moving_breaks_a_gather_and_the_node_stays_fresh() {
    let (mut g, _, crew, task) = meadow_game();
    let c = crew[0];
    put(&mut g, c, MEADOW[0]);
    ok(&mut g, Command::UseTask { player: c, task });
    assert_eq!(
        put(&mut g, c, [1.0, 700.0, 0.0]),
        [Event::CastInterrupted { player: c }]
    );
    pick(&mut g, c, task, MEADOW[0]);
}

#[test]
fn use_points_at_the_nearest_fresh_node() {
    let (mut g, _, crew, task) = meadow_game();
    let c = crew[0];
    put(&mut g, c, MEADOW[0]);
    assert_eq!(g.use_target(c), Some(Command::UseTask { player: c, task }));
    pick(&mut g, c, task, MEADOW[0]);
    assert_eq!(g.use_target(c), None, "the only node in reach is spent");
}

#[test]
fn an_impostor_gathers_for_show() {
    let (mut g, imp, _, task) = meadow_game();
    let events = pick(&mut g, imp, task, MEADOW[0]);
    assert_eq!(events, [Event::CastFinished { player: imp }]);
    // Nothing spent either: the same node fakes again.
    ok(&mut g, Command::UseTask { player: imp, task });
}
