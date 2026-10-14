//! Kills: who, whom, cooldown, reach, the body left behind, ghosts, and the impostor win.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

/// A 5-player game (1 impostor) with everyone at `spot`.
fn gathered(settings: Settings, spot: Position) -> (Game, PlayerId, Vec<PlayerId>) {
    let mut g = started_with(settings, 5, 11);
    let ids: Vec<_> = g.players().map(|(id, _)| id).collect();
    for id in ids {
        put(&mut g, id, spot);
    }
    let imp = impostors(&g)[0];
    let crew = crew(&g);
    (g, imp, crew)
}

#[test]
fn a_kill_leaves_a_body_where_the_victim_stood_and_moves_the_killer_there() {
    let (mut g, imp, crew) = gathered(instant(), [0.0; 3]);
    let at = [3.0, 0.0, 0.0];
    put(&mut g, crew[0], at);
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    assert_eq!(
        events,
        [Event::Killed {
            killer: imp,
            victim: crew[0],
            at
        }]
    );
    assert_eq!(events[0].private_to(), None);
    assert!(!g.player(crew[0]).unwrap().alive);
    assert_eq!(g.player(imp).unwrap().position, at);
    assert_eq!(
        g.bodies(),
        [Body {
            victim: crew[0],
            position: at
        }]
    );
    assert_eq!(
        g.player(imp).unwrap().kill_cooldown,
        Settings::default().kill_cooldown
    );
}

#[test]
fn kills_wait_for_the_cooldown() {
    let (mut g, imp, crew) = gathered(Settings::default(), [0.0; 3]);
    let kill = |victim| Command::Kill {
        killer: imp,
        victim,
    };
    assert_eq!(rejected(&mut g, kill(crew[0])), Reject::OnCooldown);
    tick(&mut g, 9.9);
    assert_eq!(rejected(&mut g, kill(crew[0])), Reject::OnCooldown);
    tick(&mut g, 0.1);
    ok(&mut g, kill(crew[0]));
    tick(&mut g, 24.0);
    assert_eq!(g.player(imp).unwrap().kill_cooldown, Duration::from_secs(1));
    assert_eq!(rejected(&mut g, kill(crew[1])), Reject::OnCooldown);
    tick(&mut g, 1.0);
    ok(&mut g, kill(crew[1]));
}

#[test]
fn kills_need_reach() {
    let (mut g, imp, crew) = gathered(instant(), [0.0; 3]);
    put(&mut g, crew[0], [5.01, 0.0, 0.0]);
    assert_eq!(
        rejected(
            &mut g,
            Command::Kill {
                killer: imp,
                victim: crew[0]
            }
        ),
        Reject::OutOfRange
    );
    put(&mut g, crew[0], [0.0, 0.0, 5.0]);
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
}

#[test]
fn only_a_living_impostor_kills_a_living_crewmate() {
    let mut s = instant();
    s.impostors = 2;
    let mut g = started_with(s, 8, 4);
    let ids: Vec<_> = g.players().map(|(id, _)| id).collect();
    for &id in &ids {
        put(&mut g, id, [0.0; 3]);
    }
    let (imps, crew) = (impostors(&g), crew(&g));
    let kill = |killer, victim| Command::Kill { killer, victim };
    assert_eq!(
        rejected(&mut g, kill(crew[0], crew[1])),
        Reject::NotImpostor
    );
    assert_eq!(
        rejected(&mut g, kill(imps[0], imps[1])),
        Reject::InvalidTarget
    );
    assert_eq!(
        rejected(&mut g, kill(imps[0], imps[0])),
        Reject::InvalidTarget
    );
    assert_eq!(
        rejected(&mut g, kill(imps[0], id(99))),
        Reject::InvalidTarget
    );
    ok(&mut g, kill(imps[0], crew[0]));
    assert_eq!(
        rejected(&mut g, kill(imps[1], crew[0])),
        Reject::InvalidTarget
    );
    ok(&mut g, Command::Leave { player: crew[1] });
    assert_eq!(
        rejected(&mut g, kill(imps[1], crew[1])),
        Reject::InvalidTarget
    );
}

#[test]
fn a_ghost_crewmate_keeps_doing_tasks_and_they_count() {
    let (mut g, imp, crew) = gathered(instant(), [0.0; 3]);
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    let events = finish_tasks(&mut g, crew[0]);
    assert!(events.contains(&Event::TaskBar { done: 4, total: 16 }));
}

#[test]
fn the_impostors_win_once_they_match_the_living_crew() {
    let (mut g, imp, crew) = gathered(instant(), [0.0; 3]);
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    assert_eq!(game_over(&events), None);
    tick(&mut g, 25.0);
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[1],
        },
    );
    assert_eq!(game_over(&events), None);
    tick(&mut g, 25.0);
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[2],
        },
    );
    assert_eq!(
        game_over(&events),
        Some((Team::Impostors, WinReason::Outnumbered))
    );
    assert!(events.contains(&Event::GameOver {
        winner: Team::Impostors,
        reason: WinReason::Outnumbered,
        impostors: vec![imp],
    }));
    assert_eq!(
        rejected(
            &mut g,
            Command::Kill {
                killer: imp,
                victim: crew[3]
            }
        ),
        Reject::WrongPhase
    );
}

#[test]
fn crewmates_leaving_can_hand_the_impostors_the_win() {
    let (mut g, _, crew) = gathered(instant(), [0.0; 3]);
    ok(&mut g, Command::Leave { player: crew[0] });
    ok(&mut g, Command::Leave { player: crew[1] });
    let events = ok(&mut g, Command::Leave { player: crew[2] });
    assert_eq!(
        game_over(&events),
        Some((Team::Impostors, WinReason::Outnumbered))
    );
}
