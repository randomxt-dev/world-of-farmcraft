//! Using tasks: reach, steps, who may, the task bar and the task win.

mod common;

use among_rules::*;
use common::*;

fn first_task(g: &Game, player: PlayerId) -> TaskId {
    g.player(player).unwrap().tasks[0].task
}

#[test]
fn a_task_is_used_within_reach_of_its_spot() {
    let mut g = started(5, 1);
    let c = crew(&g)[0];
    let task = first_task(&g, c);
    let [x, y, z] = task_spot(&g, c, task);
    put(&mut g, c, [x + 5.5, y, z]);
    assert_eq!(
        rejected(&mut g, Command::UseTask { player: c, task }),
        Reject::OutOfRange
    );
    put(&mut g, c, [x + 4.0, y, z]);
    let events = ok(&mut g, Command::UseTask { player: c, task });
    assert_eq!(
        events[..2],
        [
            Event::TaskProgress {
                player: c,
                task,
                step: 1,
                steps: 1
            },
            Event::TaskCompleted { player: c, task },
        ]
    );
    assert!(events[..2].iter().all(|e| e.private_to() == Some(c)));
    assert_eq!(events[2], Event::TaskBar { done: 1, total: 16 });
    assert_eq!(
        rejected(&mut g, Command::UseTask { player: c, task }),
        Reject::TaskDone
    );
}

#[test]
fn a_long_task_takes_each_step_in_order_at_its_own_spot() {
    let mut g = started(5, 1);
    let c = crew(&g)[0];
    let long = (g.player(c).unwrap().tasks.iter())
        .map(|t| t.task)
        .find(|t| g.layout().tasks[t.0].kind == TaskKind::Long)
        .unwrap();
    let [first, second] = [0, 1].map(|i| g.layout().tasks[long.0].steps[i].at);
    put(&mut g, c, second);
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: c,
                task: long
            }
        ),
        Reject::OutOfRange
    );
    put(&mut g, c, first);
    assert_eq!(
        ok(
            &mut g,
            Command::UseTask {
                player: c,
                task: long
            }
        ),
        [Event::TaskProgress {
            player: c,
            task: long,
            step: 1,
            steps: 2
        }]
    );
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: c,
                task: long
            }
        ),
        Reject::OutOfRange
    );
    put(&mut g, c, second);
    let events = ok(
        &mut g,
        Command::UseTask {
            player: c,
            task: long,
        },
    );
    assert!(events.contains(&Event::TaskCompleted {
        player: c,
        task: long
    }));
}

#[test]
fn tasks_are_your_own_and_an_impostor_only_pretends() {
    let mut g = started(5, 1);
    let imp = impostors(&g)[0];
    let task = first_task(&g, imp);
    let spot = task_spot(&g, imp, task);
    put(&mut g, imp, spot);
    // An instant cover task: accepted, and nothing happens.
    assert_eq!(ok(&mut g, Command::UseTask { player: imp, task }), []);
    assert_eq!(g.task_bar(), (0, 16));

    let c = crew(&g)[0];
    let mine: Vec<_> = g.player(c).unwrap().tasks.iter().map(|t| t.task).collect();
    let other = (0..g.layout().tasks.len())
        .map(TaskId)
        .find(|t| !mine.contains(t))
        .unwrap();
    let spot = g.layout().tasks[other.0].steps[0].at;
    put(&mut g, c, spot);
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: c,
                task: other
            }
        ),
        Reject::TaskNotAssigned
    );
}

#[test]
fn the_crew_wins_when_every_crew_task_is_done() {
    let mut g = started(5, 2);
    let crew = crew(&g);
    for &c in &crew[..3] {
        assert_eq!(game_over(&finish_tasks(&mut g, c)), None);
    }
    let events = finish_tasks(&mut g, crew[3]);
    assert!(events.contains(&Event::TaskBar {
        done: 16,
        total: 16
    }));
    assert_eq!(
        game_over(&events),
        Some((Team::Crew, WinReason::TasksCompleted))
    );
    assert!(matches!(
        g.phase(),
        Phase::GameOver {
            winner: Team::Crew,
            ..
        }
    ));
}

#[test]
fn a_crewmate_leaving_takes_their_tasks_off_the_bar() {
    let mut g = started(6, 2);
    let crew = crew(&g);
    for &c in &crew[..4] {
        finish_tasks(&mut g, c);
    }
    assert_eq!(g.phase(), &Phase::Playing);
    let events = ok(&mut g, Command::Leave { player: crew[4] });
    assert!(events.contains(&Event::TaskBar {
        done: 16,
        total: 16
    }));
    assert_eq!(
        game_over(&events),
        Some((Team::Crew, WinReason::TasksCompleted))
    );
}

#[test]
fn no_tasks_in_the_layout_means_no_task_win() {
    let mut g = Game::new(Settings::default(), Layout::default());
    for i in 1..=4 {
        ok(
            &mut g,
            Command::Join {
                player: id(i),
                class: Class::Warrior,
            },
        );
    }
    let events = ok(&mut g, Command::Start { by: id(1), seed: 0 });
    assert_eq!(events.last(), Some(&Event::TaskBar { done: 0, total: 0 }));
    assert_eq!(g.phase(), &Phase::Playing);
}
