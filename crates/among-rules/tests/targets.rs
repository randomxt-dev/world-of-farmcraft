//! What each HUD button would do right now: the nearest legal target, and which buttons are live.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

/// A 6-player game (1 impostor) with everyone far from everything.
fn spread(settings: Settings) -> (Game, PlayerId, Vec<PlayerId>) {
    let mut g = started_with(settings, 6, 41);
    for i in 1..=6 {
        put(&mut g, id(i), [1000.0 + 100.0 * i as f32, 1000.0, 0.0]);
    }
    let imp = impostors(&g)[0];
    let crew = crew(&g);
    (g, imp, crew)
}

#[test]
fn kill_targets_the_nearest_legal_crewmate() {
    let (mut g, imp, crew) = spread(instant());
    assert_eq!(g.kill_target(imp), None);
    put(&mut g, imp, [0.0; 3]);
    put(&mut g, crew[0], [4.0, 0.0, 0.0]);
    put(&mut g, crew[1], [2.0, 0.0, 0.0]);
    assert_eq!(g.kill_target(imp), Some(crew[1]));
    assert_eq!(g.kill_target(crew[0]), None, "crewmates have no kill");
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[1],
        },
    );
    // On cooldown now, though crew[0] is in reach.
    assert_eq!(g.kill_target(imp), None);
    assert_eq!(g.actions(imp).kill_cooldown_secs, 25);
    tick(&mut g, 24.1);
    assert_eq!(g.actions(imp).kill_cooldown_secs, 1);
    tick(&mut g, 1.0);
    assert_eq!(g.kill_target(imp), Some(crew[0]));
}

#[test]
fn report_targets_the_nearest_body() {
    let (mut g, imp, crew) = spread(instant());
    for (c, x) in [(crew[0], 0.0), (crew[1], 6.0)] {
        put(&mut g, imp, [x, 0.0, 0.0]);
        put(&mut g, c, [x, 0.0, 0.0]);
        ok(
            &mut g,
            Command::Kill {
                killer: imp,
                victim: c,
            },
        );
        tick(&mut g, 25.0);
    }
    put(&mut g, crew[2], [5.0, 0.0, 0.0]);
    assert_eq!(g.report_target(crew[2]), Some(crew[1]));
    assert_eq!(g.report_target(crew[0]), None, "ghosts do not report");
}

#[test]
fn use_picks_the_nearest_legal_task_step_only() {
    let (mut g, imp, crew) = spread(instant());
    let c = crew[0];
    assert_eq!(g.task_target(c), None);
    put(&mut g, c, BUTTON);
    assert_eq!(
        g.task_target(c),
        None,
        "the button is Emergency's, not Use's"
    );
    let task = g.player(c).unwrap().tasks[0].task;
    let spot = task_spot(&g, c, task);
    put(&mut g, c, spot);
    assert_eq!(g.task_target(c), Some(task));
    // An impostor's Use fakes their own cover task.
    let cover = g.player(imp).unwrap().tasks[0].task;
    let cover_spot = task_spot(&g, imp, cover);
    put(&mut g, imp, cover_spot);
    assert_eq!(g.task_target(imp), Some(cover));
}

#[test]
fn fix_picks_the_nearest_open_station_of_the_sabotage() {
    let (mut g, imp, crew) = spread(instant());
    let c = crew[0];
    put(&mut g, c, O2[0]);
    assert_eq!(g.fix_target(c), None, "nothing is sabotaged");
    ok(
        &mut g,
        Command::Sabotage {
            player: imp,
            kind: SabotageKind::Oxygen,
        },
    );
    assert_eq!(g.fix_target(c), Some(0));
    ok(
        &mut g,
        Command::Fix {
            player: c,
            station: 0,
        },
    );
    assert_eq!(g.fix_target(c), None, "a fixed station is done");
    put(&mut g, c, O2[1]);
    assert_eq!(g.fix_target(c), Some(1));
}

#[test]
fn sabotage_aims_away_from_the_impostor() {
    let (mut g, imp, crew) = spread(instant());
    assert_eq!(
        g.sabotage_target(crew[0]),
        None,
        "crewmates have no sabotage"
    );
    // Next to Lights, the farthest sabotage is the Reactor.
    put(&mut g, imp, LIGHTS);
    assert_eq!(g.sabotage_target(imp), Some(SabotageKind::Reactor));
    put(&mut g, imp, REACTOR[0]);
    assert_eq!(g.sabotage_target(imp), Some(SabotageKind::Lights));
    ok(
        &mut g,
        Command::Sabotage {
            player: imp,
            kind: SabotageKind::Lights,
        },
    );
    assert_eq!(g.sabotage_target(imp), None, "one sabotage at a time");
}

#[test]
fn emergency_is_its_own_button() {
    let (mut g, _imp, crew) = spread(instant());
    let c = crew[0];
    assert!(!g.actions(c).emergency);
    put(&mut g, c, BUTTON);
    let a = g.actions(c);
    assert!(a.emergency && !a.use_ && !a.fix);
}

#[test]
fn actions_light_the_buttons_the_server_would_accept() {
    let settings = Settings {
        sabotage_cooldown: Duration::from_secs(30),
        ..instant()
    };
    let (mut g, imp, crew) = spread(settings);
    assert_eq!(g.actions(crew[0]), Actions::default());
    assert_eq!(
        g.actions(imp),
        Actions::default(),
        "sabotage on cooldown, nobody in reach"
    );
    tick(&mut g, 30.0);
    assert!(g.actions(imp).sabotage);
    put(&mut g, crew[0], [500.0, 0.0, 0.0]);
    put(&mut g, imp, [501.0, 0.0, 0.0]);
    let a = g.actions(imp);
    assert!(a.kill && a.sabotage && !a.report);
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    put(&mut g, crew[1], [503.0, 0.0, 0.0]);
    assert_eq!(
        g.actions(crew[1]),
        Actions {
            report: true,
            ..Actions::default()
        }
    );
}
