//! Sabotage: who may, the cooldown, fixing by station, holds, critical deadlines, and meetings.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

/// A 6-player game (1 impostor) with everyone at the button.
fn game(settings: Settings) -> (Game, PlayerId, Vec<PlayerId>) {
    let mut g = started_with(settings, 6, 31);
    for i in 1..=6 {
        put(&mut g, id(i), BUTTON);
    }
    let imp = impostors(&g)[0];
    let crew = crew(&g);
    (g, imp, crew)
}

fn sabotage(g: &mut Game, player: PlayerId, kind: SabotageKind) -> Vec<Event> {
    ok(g, Command::Sabotage { player, kind })
}

fn fix(player: PlayerId, station: usize) -> Command {
    Command::Fix { player, station }
}

#[test]
fn only_impostors_sabotage_one_at_a_time_after_the_cooldown() {
    let (mut g, imp, crew) = game(Settings::default());
    let cmd = |player, kind| Command::Sabotage { player, kind };
    assert_eq!(
        rejected(&mut g, cmd(crew[0], SabotageKind::Lights)),
        Reject::NotImpostor
    );
    assert_eq!(
        rejected(&mut g, cmd(imp, SabotageKind::Lights)),
        Reject::OnCooldown
    );
    tick(&mut g, 30.0);
    assert_eq!(
        sabotage(&mut g, imp, SabotageKind::Lights),
        [Event::SabotageStarted {
            kind: SabotageKind::Lights,
            deadline: None
        }]
    );
    assert_eq!(g.sabotage().map(|s| s.kind), Some(SabotageKind::Lights));
    assert_eq!(
        rejected(&mut g, cmd(imp, SabotageKind::Comms)),
        Reject::SabotageActive
    );
}

#[test]
fn a_map_offers_only_its_own_sabotages() {
    let mut layout = layout();
    layout.sabotages.retain(|s| s.kind != SabotageKind::Oxygen);
    let mut g = Game::new(instant(), layout);
    for i in 1..=4 {
        ok(
            &mut g,
            Command::Join {
                player: id(i),
                class: Class::Warrior,
            },
        );
    }
    ok(&mut g, Command::Start { by: id(1), seed: 0 });
    let imp = impostors(&g)[0];
    assert_eq!(
        rejected(
            &mut g,
            Command::Sabotage {
                player: imp,
                kind: SabotageKind::Oxygen
            }
        ),
        Reject::NoSuchSabotage
    );
    sabotage(&mut g, imp, SabotageKind::Reactor);
}

#[test]
fn a_dead_impostor_still_sabotages() {
    let mut s = instant();
    s.impostors = 2;
    let mut g = started_with(s, 7, 8);
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
    assert_eq!(g.phase(), &Phase::Playing);
    assert!(!g.player(imps[0]).unwrap().alive);
    sabotage(&mut g, imps[0], SabotageKind::Comms);
}

#[test]
fn lights_are_fixed_at_their_station_and_the_cooldown_restarts() {
    let (mut g, imp, crew) = game(instant_with_cooldown());
    tick(&mut g, 30.0);
    assert_eq!(rejected(&mut g, fix(crew[0], 0)), Reject::NoSabotage);
    sabotage(&mut g, imp, SabotageKind::Lights);
    assert_eq!(rejected(&mut g, fix(crew[0], 0)), Reject::OutOfRange);
    assert_eq!(rejected(&mut g, fix(crew[0], 1)), Reject::NoSuchStation);
    put(&mut g, crew[0], LIGHTS);
    assert_eq!(
        ok(&mut g, fix(crew[0], 0)),
        [
            Event::StationFixed {
                station: 0,
                player: crew[0]
            },
            Event::SabotageFixed {
                kind: SabotageKind::Lights
            },
        ]
    );
    assert!(g.sabotage().is_none());
    assert_eq!(g.sabotage_cooldown(), Duration::from_secs(30));
    assert_eq!(
        rejected(
            &mut g,
            Command::Sabotage {
                player: imp,
                kind: SabotageKind::Comms
            }
        ),
        Reject::OnCooldown
    );
}

/// No kill or emergency cooldowns, but the default sabotage cooldown.
fn instant_with_cooldown() -> Settings {
    Settings {
        sabotage_cooldown: Settings::default().sabotage_cooldown,
        ..instant()
    }
}

#[test]
fn ghosts_cannot_fix() {
    let (mut g, imp, crew) = game(instant());
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    sabotage(&mut g, imp, SabotageKind::Lights);
    put(&mut g, crew[0], LIGHTS);
    assert_eq!(rejected(&mut g, fix(crew[0], 0)), Reject::Dead);
}

#[test]
fn oxygen_needs_every_station_fixed_once_before_the_deadline() {
    let (mut g, imp, crew) = game(instant());
    let events = sabotage(&mut g, imp, SabotageKind::Oxygen);
    assert_eq!(
        events,
        [Event::SabotageStarted {
            kind: SabotageKind::Oxygen,
            deadline: Some(CRITICAL)
        }]
    );
    put(&mut g, crew[0], O2[0]);
    assert_eq!(
        ok(&mut g, fix(crew[0], 0)),
        [Event::StationFixed {
            station: 0,
            player: crew[0]
        }]
    );
    assert_eq!(rejected(&mut g, fix(crew[0], 0)), Reject::StationFixed);
    tick(&mut g, 29.0);
    put(&mut g, crew[1], O2[1]);
    let events = ok(&mut g, fix(crew[1], 1));
    assert_eq!(
        events.last(),
        Some(&Event::SabotageFixed {
            kind: SabotageKind::Oxygen
        })
    );
    tick(&mut g, 60.0);
    assert_eq!(g.phase(), &Phase::Playing);
}

#[test]
fn an_unfixed_critical_sabotage_wins_for_the_impostors_at_its_deadline() {
    let (mut g, imp, _) = game(instant());
    sabotage(&mut g, imp, SabotageKind::Oxygen);
    assert_eq!(tick(&mut g, 29.5), []);
    assert_eq!(
        g.sabotage().unwrap().remaining,
        Some(Duration::from_millis(500))
    );
    let events = tick(&mut g, 0.5);
    assert_eq!(
        game_over(&events),
        Some((Team::Impostors, WinReason::Sabotage))
    );
    assert!(g.sabotage().is_none());
}

#[test]
fn the_reactor_needs_both_stations_held_at_once() {
    let (mut g, imp, crew) = game(instant());
    sabotage(&mut g, imp, SabotageKind::Reactor);
    let (a, b) = (crew[0], crew[1]);
    put(&mut g, a, REACTOR[0]);
    put(&mut g, b, REACTOR[1]);
    assert_eq!(
        ok(&mut g, fix(a, 0)),
        [Event::StationHeld {
            station: 0,
            player: a
        }]
    );
    assert_eq!(rejected(&mut g, fix(b, 0)), Reject::StationBusy);
    // Walking away lets go.
    assert_eq!(
        put(&mut g, a, BUTTON),
        [Event::StationReleased { station: 0 }]
    );
    put(&mut g, a, REACTOR[0]);
    ok(&mut g, fix(a, 0));
    assert_eq!(
        ok(&mut g, Command::ReleaseFix { player: a }),
        [Event::StationReleased { station: 0 }]
    );
    ok(&mut g, fix(a, 0));
    assert_eq!(
        ok(&mut g, fix(b, 1)),
        [
            Event::StationHeld {
                station: 1,
                player: b
            },
            Event::SabotageFixed {
                kind: SabotageKind::Reactor
            },
        ]
    );
}

#[test]
fn one_player_holds_one_station() {
    let (mut g, imp, crew) = game(instant());
    sabotage(&mut g, imp, SabotageKind::Reactor);
    // The two reactor stations are 10 yd apart: stand between them, in reach of both.
    put(&mut g, crew[0], [0.0, 405.0, 0.0]);
    ok(&mut g, fix(crew[0], 0));
    assert_eq!(
        ok(&mut g, fix(crew[0], 1)),
        [
            Event::StationReleased { station: 0 },
            Event::StationHeld {
                station: 1,
                player: crew[0]
            },
        ]
    );
    assert!(g.sabotage().is_some());
}

#[test]
fn a_holder_killed_lets_go() {
    let (mut g, imp, crew) = game(instant());
    sabotage(&mut g, imp, SabotageKind::Reactor);
    put(&mut g, crew[0], REACTOR[0]);
    ok(&mut g, fix(crew[0], 0));
    put(&mut g, imp, REACTOR[0]);
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    assert!(events.contains(&Event::StationReleased { station: 0 }));
}

#[test]
fn no_emergency_during_a_sabotage_but_a_report_ends_a_critical_one() {
    let (mut g, imp, crew) = game(instant());
    sabotage(&mut g, imp, SabotageKind::Reactor);
    assert_eq!(
        rejected(&mut g, Command::CallEmergency { player: crew[0] }),
        Reject::SabotageActive
    );
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    let events = ok(
        &mut g,
        Command::Report {
            player: crew[1],
            body: crew[0],
        },
    );
    assert!(events.contains(&Event::SabotageFixed {
        kind: SabotageKind::Reactor
    }));
    assert!(g.sabotage().is_none());
}

#[test]
fn lights_stay_out_through_a_meeting() {
    let (mut g, imp, crew) = game(instant());
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: crew[0],
        },
    );
    sabotage(&mut g, imp, SabotageKind::Lights);
    let events = ok(
        &mut g,
        Command::Report {
            player: crew[1],
            body: crew[0],
        },
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::SabotageFixed { .. })));
    tick(&mut g, 15.0 + 120.0 + 6.0);
    assert_eq!(g.phase(), &Phase::Playing);
    assert_eq!(g.sabotage().map(|s| s.kind), Some(SabotageKind::Lights));
}
