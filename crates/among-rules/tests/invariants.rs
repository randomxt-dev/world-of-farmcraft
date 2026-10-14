//! Long random command streams against the invariants a server relies on: a refusal changes
//! nothing, `validate` agrees with `apply`, nothing panics, and a decided game always ends.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

/// xorshift64*: the test's own stream, independent of the crate's RNG.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// Spots worth standing on: the button, every task step, every station, and nowhere useful.
fn spots(layout: &Layout) -> Vec<Position> {
    let mut v = vec![layout.emergency_button, [1000.0, 1000.0, 0.0]];
    v.extend(layout.tasks.iter().flat_map(|t| t.steps.iter().copied()));
    v.extend(
        layout
            .sabotages
            .iter()
            .flat_map(|s| s.stations.iter().copied()),
    );
    v
}

fn random_command(rng: &mut Rng, g: &Game, spots: &[Position]) -> Command {
    // Ids 1..=9 are mostly seated; 10 never is.
    let p = PlayerId(1 + rng.below(10) as u64);
    let q = PlayerId(1 + rng.below(10) as u64);
    let near = |rng: &mut Rng| {
        let [x, y, z] = rng.pick(spots);
        [x + rng.below(7) as f32 - 3.0, y, z]
    };
    match rng.below(20) {
        0 => Command::Join { player: p },
        1 if rng.below(8) == 0 => Command::Leave { player: p },
        2 => Command::Start {
            by: p,
            seed: rng.next(),
        },
        3..=5 => Command::Move {
            player: p,
            position: near(rng),
        },
        // Walk onto someone, so kills and reports land in reach.
        6 => Command::Move {
            player: p,
            position: g.player(q).map_or([0.0; 3], |o| o.position),
        },
        7 => Command::UseTask {
            player: p,
            task: TaskId(rng.below(8)),
        },
        8 | 9 => Command::Kill {
            killer: p,
            victim: q,
        },
        10 => Command::Report { player: p, body: q },
        11 => Command::CallEmergency { player: p },
        12 | 13 => Command::Vote {
            voter: p,
            vote: if rng.below(4) == 0 {
                Vote::Skip
            } else {
                Vote::Player(q)
            },
        },
        14 => Command::Sabotage {
            player: p,
            kind: rng.pick(&[
                SabotageKind::Lights,
                SabotageKind::Comms,
                SabotageKind::Oxygen,
                SabotageKind::Reactor,
            ]),
        },
        15 => Command::Fix {
            player: p,
            station: rng.below(3),
        },
        16 => Command::ReleaseFix { player: p },
        17 => Command::ReturnToLobby { by: p },
        _ => Command::Tick {
            dt: Duration::from_millis(rng.below(20_000) as u64),
        },
    }
}

fn check_state(g: &Game) {
    let living = |role| {
        g.players()
            .filter(|(_, p)| p.connected && p.alive && p.role == Some(role))
            .count()
    };
    // The reveal defers the win check; everywhere else a decided game is over.
    if matches!(g.phase(), Phase::Playing | Phase::Meeting(_)) {
        let (imps, crew) = (living(Role::Impostor), living(Role::Crewmate));
        assert!(
            imps >= 1 && imps < crew,
            "undecided with {imps} impostors, {crew} crew"
        );
        let (done, total) = g.task_bar();
        assert!(total == 0 || done < total);
    }
    for b in g.bodies() {
        assert!(!g.player(b.victim).unwrap().alive, "a body of the living");
    }
    if let Some(host) = g.host() {
        assert!(g.player(host).is_some_and(|p| p.connected));
    }
}

#[test]
fn random_streams_keep_the_invariants() {
    let layout = layout();
    let spots = spots(&layout);
    let mut accepted = 0;
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    let mut games_over = 0;
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let settings = Settings {
            impostors: 1 + rng.below(3),
            ..instant()
        };
        let mut g = lobby_with(settings, 4 + rng.below(6) as u64);
        for _ in 0..3000 {
            let cmd = random_command(&mut rng, &g, &spots);
            let verdict = g.validate(&cmd);
            let before = g.clone();
            match g.apply(cmd.clone()) {
                Ok(events) => {
                    assert_eq!(verdict, Ok(()), "{cmd:?}");
                    accepted += 1;
                    let kind = format!("{cmd:?}");
                    let kind = kind.split([' ', '{']).next().unwrap().to_owned();
                    *by_kind.entry(kind).or_default() += 1;
                    for e in &events {
                        if let Some(p) = e.private_to() {
                            assert!(g.player(p).is_some(), "{e:?} for nobody");
                        }
                    }
                    let overs = events
                        .iter()
                        .filter(|e| matches!(e, Event::GameOver { .. }));
                    match overs.count() {
                        0 => {}
                        1 => {
                            games_over += 1;
                            assert!(matches!(g.phase(), Phase::GameOver { .. }));
                        }
                        n => panic!("{n} game overs from {cmd:?}"),
                    }
                }
                Err(r) => {
                    assert_eq!(verdict, Err(r), "{cmd:?}");
                    assert_eq!(
                        g, before,
                        "{cmd:?} was refused ({r:?}) but changed the game"
                    );
                }
            }
            check_state(&g);
        }
    }
    // The stream must actually play games, not bounce off the lobby.
    eprintln!("accepted: {by_kind:?}, games over: {games_over}");
    // Every kind but Configure, which the stream never sends.
    assert_eq!(
        by_kind.len(),
        14,
        "a command kind was never accepted: {by_kind:?}"
    );
    assert!(accepted > 100_000, "only {accepted} commands accepted");
    assert!(games_over > 200, "only {games_over} games finished");
}
