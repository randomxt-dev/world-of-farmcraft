//! Gather steps: pick your share of a step's nodes, a cast each; a crewmate's pick takes the node
//! from everyone until the lobby, and a task is dealt to no more crewmates than it has spots.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

/// A 5-player game on [`gather_layout`] (one impostor, four crewmates); everyone's first task is
/// the shared gathering one, 3 of the 4 [`MEADOW`] nodes as written.
fn meadow_game() -> (Game, PlayerId, Vec<PlayerId>, TaskId) {
    meadow_game_of(5)
}

/// [`instant`] that starts with three and kills back to back.
fn quick() -> Settings {
    Settings {
        min_players: 3,
        kill_cooldown: Duration::ZERO,
        ..instant()
    }
}

fn meadow_game_of(n: u64) -> (Game, PlayerId, Vec<PlayerId>, TaskId) {
    let g = started_on(gather_layout(), quick(), n, 3);
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
fn each_holder_gathers_an_even_share_of_the_spots() {
    // Four crewmates share four nodes: one each, though the layout asks for three.
    let (g, _, crew, task) = meadow_game();
    assert_eq!(crew.len(), 4);
    assert_eq!(g.gather_need(task, 0), 1);
    // Two crewmates (three players) share four: two each.
    let (g, _, crew, task) = meadow_game_of(3);
    assert_eq!(crew.len(), 2);
    assert_eq!(g.gather_need(task, 0), 2);
}

#[test]
fn a_pick_takes_the_node_from_the_whole_crew_and_finishes_the_share() {
    let (mut g, _, crew, task) = meadow_game_of(3);
    let c = crew[0];
    let events = pick(&mut g, c, task, MEADOW[0]);
    assert_eq!(
        events,
        [
            Event::CastFinished { player: c },
            Event::GatherProgress {
                player: c,
                task,
                step: 0,
                node: 0,
                picked: 1,
                count: 2
            },
        ]
    );
    assert!(g.is_taken(task, 0, 0));
    // Gone for the other crewmate too: nothing left in reach there, and the node itself refused.
    let other = crew[1];
    put(&mut g, other, MEADOW[0]);
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: other,
                task
            }
        ),
        Reject::OutOfRange
    );
    let take = Command::GatherNode {
        player: other,
        task,
        node: 0,
    };
    assert_eq!(rejected(&mut g, take), Reject::AlreadyGathered);
    // The second pick is the share: the step is done.
    let events = pick(&mut g, c, task, MEADOW[2]);
    assert!(
        events.contains(&Event::TaskCompleted { player: c, task }),
        "{events:?}"
    );
    // The other crewmate still finds their two among the rest.
    pick(&mut g, other, task, MEADOW[1]);
    let events = pick(&mut g, other, task, MEADOW[3]);
    assert!(events.contains(&Event::TaskCompleted {
        player: other,
        task
    }));
}

#[test]
fn two_casts_at_one_node_count_once() {
    let (mut g, _, crew, task) = meadow_game_of(3);
    for &c in &crew[..2] {
        put(&mut g, c, MEADOW[0]);
        ok(
            &mut g,
            Command::GatherNode {
                player: c,
                task,
                node: 0,
            },
        );
    }
    let events = tick(&mut g, 2.0);
    let progress: Vec<&Event> = (events.iter())
        .filter(|e| matches!(e, Event::GatherProgress { .. }))
        .collect();
    assert_eq!(progress.len(), 1, "{events:?}");
}

#[test]
fn between_two_nodes_the_nearer_open_one_is_gathered() {
    let (mut g, _, crew, task) = meadow_game_of(3);
    let c = crew[0];
    pick(&mut g, c, task, MEADOW[1]);
    // 4 yd from MEADOW[1] (taken) and 6 yd from MEADOW[0]: out of reach of the open one.
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
fn moving_breaks_a_gather_and_the_node_stays() {
    let (mut g, _, crew, task) = meadow_game();
    let c = crew[0];
    put(&mut g, c, MEADOW[0]);
    ok(&mut g, Command::UseTask { player: c, task });
    assert_eq!(
        put(&mut g, c, [1.0, 700.0, 0.0]),
        [Event::CastInterrupted { player: c }]
    );
    assert!(!g.is_taken(task, 0, 0));
    pick(&mut g, c, task, MEADOW[0]);
}

#[test]
fn use_points_at_the_nearest_open_node() {
    let (mut g, _, crew, task) = meadow_game_of(3);
    let c = crew[0];
    put(&mut g, c, MEADOW[0]);
    assert_eq!(g.task_target(c), Some(task));
    pick(&mut g, c, task, MEADOW[0]);
    assert_eq!(g.task_target(c), None, "the only node in reach is taken");
}

#[test]
fn an_impostor_gathers_for_show_and_takes_nothing() {
    let (mut g, imp, crew, task) = meadow_game();
    let events = pick(&mut g, imp, task, MEADOW[0]);
    assert_eq!(events, [Event::CastFinished { player: imp }]);
    // The node stands for everyone: the impostor fakes it again, and a crewmate still picks it.
    assert!(!g.is_taken(task, 0, 0));
    ok(&mut g, Command::UseTask { player: imp, task });
    pick(&mut g, crew[0], task, MEADOW[0]);
}

#[test]
fn a_chosen_node_is_the_one_gathered() {
    let (mut g, _, crew, task) = meadow_game_of(3);
    let c = crew[0];
    // Halfway between MEADOW[0] and MEADOW[1], 5 yd from each: both in reach.
    put(&mut g, c, [5.0, 700.0, 0.0]);
    let gather = |node| Command::GatherNode {
        player: c,
        task,
        node,
    };
    ok(&mut g, gather(1));
    assert_eq!(g.player(c).unwrap().casting.unwrap().node, Some(1));
    let events = tick(&mut g, 2.0);
    assert!(
        events.contains(&Event::GatherProgress {
            player: c,
            task,
            step: 0,
            node: 1,
            picked: 1,
            count: 2
        }),
        "{events:?}"
    );
    // That node is taken; one out of reach, or not on the step, is refused.
    assert_eq!(rejected(&mut g, gather(1)), Reject::AlreadyGathered);
    assert_eq!(rejected(&mut g, gather(2)), Reject::OutOfRange);
    assert_eq!(rejected(&mut g, gather(4)), Reject::InvalidTarget);
    // Use still finds the other one in reach.
    ok(&mut g, Command::UseTask { player: c, task });
    assert_eq!(g.player(c).unwrap().casting.unwrap().node, Some(0));
}

#[test]
fn a_chosen_node_needs_a_gather_step() {
    let (mut g, _, crew, _) = meadow_game();
    let c = crew[0];
    let other = (g.player(c).unwrap().tasks.iter())
        .map(|t| t.task)
        .find(|t| g.layout().tasks[t.0].kind != TaskKind::Common)
        .unwrap();
    let cmd = Command::GatherNode {
        player: c,
        task: other,
        node: 0,
    };
    assert_eq!(rejected(&mut g, cmd), Reject::InvalidTarget);
}

#[test]
fn a_task_is_dealt_to_no_more_crewmates_than_its_spots() {
    // Two spots per common task, four crewmates: neither common task can serve them all.
    let mut layout = gather_layout();
    for t in layout
        .tasks
        .iter_mut()
        .filter(|t| t.kind == TaskKind::Common)
    {
        if let StepAction::Gather { nodes, .. } = &mut t.steps[0].action {
            nodes.truncate(2);
        }
    }
    for seed in 0..20 {
        let g = started_on(layout.clone(), quick(), 5, seed);
        for c in crew(&g) {
            let tasks = &g.player(c).unwrap().tasks;
            assert!(
                tasks
                    .iter()
                    .all(|t| g.layout().tasks[t.task.0].kind != TaskKind::Common),
                "seed {seed}: a common task dealt past its spots"
            );
        }
    }
}

#[test]
fn the_lobby_puts_every_node_back() {
    let (mut g, _, crew, task) = meadow_game();
    pick(&mut g, crew[0], task, MEADOW[0]);
    assert!(g.is_taken(task, 0, 0));
    // The impostor kills until the crew is outnumbered and the game ends.
    let host = id(1);
    while !matches!(g.phase(), Phase::GameOver { .. }) {
        let imp = impostors(&g)[0];
        let victim = crew.iter().copied().find(|&c| g.player(c).unwrap().alive);
        let Some(victim) = victim else { break };
        let at = g.player(victim).unwrap().position;
        put(&mut g, imp, at);
        ok(
            &mut g,
            Command::Kill {
                killer: imp,
                victim,
            },
        );
    }
    ok(&mut g, Command::ReturnToLobby { by: host });
    assert!(!g.is_taken(task, 0, 0));
    assert_eq!(g.gather_need(task, 0), 3, "no holders: the layout's count");
}
