//! The HUD/server vocabulary: every message survives a round trip, garbage is refused, and each
//! event reaches only those allowed to know it.

mod common;

use std::time::Duration;

use among_rules::wire::{notices, Notice, Request, To};
use among_rules::*;
use common::*;

#[test]
fn every_request_round_trips() {
    let requests = [
        Request::Start,
        Request::Use,
        Request::Kill,
        Request::Report,
        Request::Vote(Vote::Skip),
        Request::Vote(Vote::Player(PlayerId(17))),
        Request::Sabotage(SabotageKind::Reactor),
        Request::Release,
        Request::Fix,
        Request::Emergency,
        Request::Lobby,
    ];
    for r in requests {
        assert_eq!(Request::decode(&r.encode()), Some(r), "{}", r.encode());
    }
    for garbage in [
        "",
        "kil",
        "kill now",
        "vote",
        "vote 0",
        "vote -3",
        "sabotage doors",
    ] {
        assert_eq!(Request::decode(garbage), None, "{garbage:?}");
    }
}

#[test]
fn every_notice_round_trips() {
    let p = PlayerId;
    let notices = [
        Notice::Host(Some(p(4))),
        Notice::Host(None),
        Notice::Started {
            players: 8,
            impostors: 2,
        },
        Notice::Role {
            role: Role::Crewmate,
            fellow_impostors: vec![],
        },
        Notice::Role {
            role: Role::Impostor,
            fellow_impostors: vec![p(3), p(9)],
        },
        Notice::Role {
            role: Role::Impostor,
            fellow_impostors: vec![],
        },
        Notice::Task {
            task: TaskId(2),
            step: 1,
            steps: 2,
            name: "Align engine output".into(),
        },
        Notice::TaskBar { done: 3, total: 20 },
        Notice::Slay {
            task: TaskId(4),
            kills: 1,
            count: 2,
        },
        Notice::Gather {
            task: TaskId(0),
            picked: 3,
            count: 5,
        },
        Notice::Dead,
        Notice::Actions(Actions {
            kill: true,
            report: false,
            use_: true,
            fix: false,
            emergency: true,
            sabotage: true,
            kill_cooldown_secs: 12,
        }),
        Notice::Refused("too far away".into()),
        Notice::Meeting {
            caller: p(5),
            body: Some(p(6)),
            discussion: Duration::from_secs(15),
        },
        Notice::Meeting {
            caller: p(5),
            body: None,
            discussion: Duration::ZERO,
        },
        Notice::Voting(Duration::from_millis(120_500)),
        Notice::Voted(p(5)),
        Notice::Unvoted(p(5)),
        Notice::Tally {
            outcome: VoteOutcome::Ejected(p(6)),
            votes: vec![(p(1), Vote::Player(p(6))), (p(2), Vote::Skip)],
        },
        Notice::Tally {
            outcome: VoteOutcome::Tie,
            votes: vec![],
        },
        Notice::Tally {
            outcome: VoteOutcome::Skipped,
            votes: vec![(p(1), Vote::Skip)],
        },
        Notice::Ejected {
            player: p(6),
            impostor: Some(true),
            impostors_left: Some(0),
        },
        Notice::Ejected {
            player: p(6),
            impostor: None,
            impostors_left: None,
        },
        Notice::Resume,
        Notice::Sabotage {
            kind: SabotageKind::Oxygen,
            deadline: Some(Duration::from_secs(30)),
        },
        Notice::Sabotage {
            kind: SabotageKind::Lights,
            deadline: None,
        },
        Notice::Station {
            station: 1,
            state: Station::Held(p(2)),
        },
        Notice::Station {
            station: 0,
            state: Station::Open,
        },
        Notice::Station {
            station: 0,
            state: Station::Fixed,
        },
        Notice::Fixed(SabotageKind::Comms),
        Notice::Over {
            winner: Team::Impostors,
            reason: WinReason::Sabotage,
            impostors: vec![p(3)],
        },
        Notice::Over {
            winner: Team::Crew,
            reason: WinReason::TasksCompleted,
            impostors: vec![],
        },
        Notice::Lobby,
    ];
    for n in notices {
        assert_eq!(
            Notice::decode(&n.encode()),
            Some(n.clone()),
            "{}",
            n.encode()
        );
    }
    for garbage in [
        "",
        "dead now",
        "bar 1",
        "role ghost",
        "station 0 broken",
        "over crew",
    ] {
        assert_eq!(Notice::decode(garbage), None, "{garbage:?}");
    }
}

/// Every notice the events of `cmd` produce, flattened.
fn routed(g: &mut Game, cmd: Command) -> Vec<(To, Notice)> {
    let events = ok(g, cmd);
    events.iter().flat_map(|e| notices(e, g)).collect()
}

#[test]
fn roles_and_tasks_go_only_to_their_player() {
    let mut g = lobby(5);
    let out = routed(&mut g, Command::Start { by: id(1), seed: 3 });
    assert_eq!(
        out[0],
        (
            To::All,
            Notice::Started {
                players: 5,
                impostors: 1
            }
        )
    );
    for (to, n) in &out {
        match n {
            Notice::Role { .. } | Notice::Task { .. } => assert!(matches!(to, To::Player(_))),
            _ => assert_eq!(*to, To::All, "{n:?}"),
        }
    }
    let c = crew(&g)[0];
    let mine: Vec<_> = (out.iter())
        .filter(|(to, n)| *to == To::Player(c) && matches!(n, Notice::Task { .. }))
        .collect();
    assert_eq!(mine.len(), 4);
    let Notice::Task {
        task,
        step: 0,
        steps,
        name,
    } = &mine[0].1
    else {
        panic!()
    };
    assert_eq!(*steps, g.layout().tasks[task.0].steps.len());
    assert_eq!(name, &g.layout().tasks[task.0].name);
}

#[test]
fn a_kill_tells_the_victim_and_nobody_learns_the_killer() {
    let mut g = started_with(instant(), 5, 3);
    for i in 1..=5 {
        put(&mut g, id(i), [0.0; 3]);
    }
    let (imp, victim) = (impostors(&g)[0], crew(&g)[0]);
    let out = routed(
        &mut g,
        Command::Kill {
            killer: imp,
            victim,
        },
    );
    assert_eq!(out, [(To::Player(victim), Notice::Dead)]);
}
