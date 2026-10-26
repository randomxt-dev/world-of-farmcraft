//! Task casts: gathering and crafting take time standing still, and anything that moves the caster
//! or stops play breaks them. Impostors fake casts on their cover tasks.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

const CAST: Duration = Duration::from_secs(3);

/// A 5-player game (1 impostor) where every step is a 3 s cast, everyone at the button.
fn casting_game() -> (Game, PlayerId, Vec<PlayerId>) {
    let mut g = started_on(cast_layout(3.0), instant(), 5, 7);
    for i in 1..=5 {
        put(&mut g, id(i), BUTTON);
    }
    let imp = impostors(&g)[0];
    let crew = crew(&g);
    (g, imp, crew)
}

/// Walks `player` to their first task and starts its cast; returns the task.
fn start_cast(g: &mut Game, player: PlayerId) -> TaskId {
    let task = g.player(player).unwrap().tasks[0].task;
    let spot = task_spot(g, player, task);
    put(g, player, spot);
    let events = ok(g, Command::UseTask { player, task });
    assert_eq!(
        events,
        [Event::CastStarted {
            player,
            task,
            time: CAST
        }]
    );
    task
}

#[test]
fn a_cast_completes_its_step_when_it_runs_out() {
    let (mut g, _, crew) = casting_game();
    let c = crew[0];
    let task = start_cast(&mut g, c);
    assert_eq!(g.player(c).unwrap().tasks[0].step, 0, "no progress yet");
    assert_eq!(tick(&mut g, 2.5), []);
    let events = tick(&mut g, 0.5);
    assert_eq!(events[0], Event::CastFinished { player: c });
    assert_eq!(
        events[1],
        Event::TaskProgress {
            player: c,
            task,
            step: 1,
            steps: 1
        }
    );
    assert!(g.player(c).unwrap().casting.is_none());
}

#[test]
fn one_cast_at_a_time() {
    let (mut g, _, crew) = casting_game();
    let task = start_cast(&mut g, crew[0]);
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: crew[0],
                task
            }
        ),
        Reject::AlreadyCasting
    );
}

#[test]
fn moving_breaks_the_cast_and_standing_still_does_not() {
    let (mut g, _, crew) = casting_game();
    let c = crew[0];
    let task = start_cast(&mut g, c);
    let here = g.player(c).unwrap().position;
    // A heartbeat or a turn on the spot reports the same position.
    assert_eq!(put(&mut g, c, here), []);
    let [x, y, z] = here;
    assert_eq!(
        put(&mut g, c, [x + 0.5, y, z]),
        [Event::CastInterrupted { player: c }]
    );
    assert_eq!(tick(&mut g, 5.0), [], "a broken cast never finishes");
    assert_eq!(g.player(c).unwrap().tasks[0].step, 0);
    // The step is still there to start over.
    put(&mut g, c, here);
    ok(&mut g, Command::UseTask { player: c, task });
}

#[test]
fn a_meeting_breaks_every_cast() {
    let (mut g, _, crew) = casting_game();
    start_cast(&mut g, crew[0]);
    start_cast(&mut g, crew[1]);
    let events = ok(&mut g, Command::CallEmergency { player: crew[2] });
    assert!(events.contains(&Event::CastInterrupted { player: crew[0] }));
    assert!(events.contains(&Event::CastInterrupted { player: crew[1] }));
    assert!(g.players().all(|(_, p)| p.casting.is_none()));
}

#[test]
fn a_kill_breaks_the_victims_cast() {
    let (mut g, imp, crew) = casting_game();
    start_cast(&mut g, crew[0]);
    let at = g.player(crew[0]).unwrap().position;
    put(&mut g, imp, at);
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    assert_eq!(events[0], Event::CastInterrupted { player: crew[0] });
}

#[test]
fn an_impostor_fakes_a_cast_that_never_progresses() {
    let (mut g, imp, _) = casting_game();
    let task = start_cast(&mut g, imp);
    let events = tick(&mut g, 3.0);
    assert_eq!(
        events,
        [Event::CastFinished { player: imp }],
        "no progress, no bar"
    );
    assert_eq!(g.task_bar(), (0, 16));
    // And again, as often as they like.
    ok(&mut g, Command::UseTask { player: imp, task });
}

#[test]
fn a_ghost_impostor_cannot_fake() {
    let mut s = instant();
    s.impostors = 2;
    let mut g = started_on(cast_layout(3.0), s, 7, 8);
    for i in 1..=7 {
        put(&mut g, id(i), BUTTON);
    }
    let imps = impostors(&g);
    ok(&mut g, Command::CallEmergency { player: imps[1] });
    tick(&mut g, 15.0);
    for i in 1..=7 {
        ok(
            &mut g,
            Command::Vote {
                voter: id(i),
                vote: Vote::Player(imps[0]),
            },
        );
    }
    tick(&mut g, 6.0);
    let task = g.player(imps[0]).unwrap().tasks[0].task;
    let spot = task_spot(&g, imps[0], task);
    put(&mut g, imps[0], spot);
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: imps[0],
                task
            }
        ),
        Reject::Dead
    );
}

#[test]
fn the_end_of_the_game_breaks_every_cast() {
    let mut g = started_on(cast_layout(3.0), instant(), 4, 7);
    for i in 1..=4 {
        put(&mut g, id(i), BUTTON);
    }
    let (imp, crew) = (impostors(&g)[0], crew(&g));
    start_cast(&mut g, crew[0]);
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[1],
        },
    );
    tick(&mut g, 2.0);
    tick(&mut g, 23.0);
    // crew[0] stood still the whole time, so its 3 s cast finished long ago: start another.
    start_cast_again(&mut g, crew[0]);
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[2],
        },
    );
    assert!(
        events.contains(&Event::CastInterrupted { player: crew[0] }),
        "{events:?}"
    );
    assert!(matches!(g.phase(), Phase::GameOver { .. }));
}

/// Starts a cast on `player`'s first unfinished task, wherever they stand.
fn start_cast_again(g: &mut Game, player: PlayerId) {
    let t = *g
        .player(player)
        .unwrap()
        .tasks
        .iter()
        .find(|t| t.step < g.layout().tasks[t.task.0].steps.len())
        .unwrap();
    let spot = task_spot(g, player, t.task);
    put(g, player, spot);
    ok(
        g,
        Command::UseTask {
            player,
            task: t.task,
        },
    );
}
