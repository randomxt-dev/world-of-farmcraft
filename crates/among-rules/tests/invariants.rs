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
    v.extend(
        layout
            .tasks
            .iter()
            .flat_map(|t| t.steps.iter().map(|s| s.at)),
    );
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
    let wolf = CreatureId(rng.below(2));
    match rng.below(23) {
        0 => Command::Join {
            player: p,
            class: rng.pick(&Class::ALL),
        },
        18 => Command::Attack {
            player: p,
            creature: wolf,
        },
        19 => Command::StopAttack { player: p },
        20 => Command::CastSpell {
            player: p,
            creature: wolf,
        },
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
    if !matches!(g.phase(), Phase::Playing) {
        assert!(
            g.players().all(|(_, p)| p.casting.is_none()
                && p.spell_cast.is_none()
                && p.attacking.is_none()),
            "a cast or fight outside play"
        );
        assert!(
            g.creatures().iter().all(|c| c.target.is_none()),
            "a creature hunting outside play"
        );
    }
    for (_, p) in g.players() {
        if !p.alive || !p.connected {
            assert!(
                p.attacking.is_none() && p.spell_cast.is_none(),
                "a ghost fighting"
            );
        }
        assert!(p.health <= g.settings().kits.get(p.class).health);
    }
    for (i, c) in g.creatures().iter().enumerate() {
        assert!(c.health <= g.layout().creatures[i].health);
        if let Some(t) = c.target {
            assert!(c.alive, "a dead creature with a target");
            let t = g.player(t).unwrap();
            assert!(t.alive && t.connected, "a creature hunting a ghost");
        }
    }
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
    // Every layout's spots, plus the den and a step from each wolf.
    let mut spots = spots(&layout());
    spots.extend(MEADOW);
    let [x, y, z] = DEN;
    spots.extend([
        DEN,
        [x, y + 4.0, z],
        [x + 20.0, y + 4.0, z],
        [x, y + 9.0, z],
    ]);
    let mut accepted = 0;
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    let mut games_over = 0;
    let mut casts = [0usize; 3];
    // Creature deaths, monster kills, spell hits, Slay progress.
    let mut fights = [0usize; 4];
    let mut gathered = 0usize;
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let settings = Settings {
            impostors: 1 + rng.below(3),
            ..instant()
        };
        // A quarter of the seeds each: wolves and Slay steps, 2 s casts, gathering, instant steps.
        let layout = match seed % 4 {
            0 => combat_layout(),
            1 => cast_layout(2.0),
            2 => gather_layout(),
            _ => layout(),
        };
        let mut g = Game::new(settings, layout);
        for i in 1..=4 + rng.below(6) as u64 {
            let class = rng.pick(&Class::ALL);
            ok(
                &mut g,
                Command::Join {
                    player: id(i),
                    class,
                },
            );
        }
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
                        match e {
                            Event::CastStarted { .. } => casts[0] += 1,
                            Event::CastFinished { .. } => casts[1] += 1,
                            Event::CastInterrupted { .. } => casts[2] += 1,
                            Event::CreatureDied { .. } => fights[0] += 1,
                            Event::Slain { .. } => fights[1] += 1,
                            Event::SpellHit { .. } => fights[2] += 1,
                            Event::SlayProgress { .. } => fights[3] += 1,
                            Event::GatherProgress { .. } => gathered += 1,
                            _ => {}
                        }
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
    eprintln!(
        "accepted: {by_kind:?}, games over: {games_over}, casts started/finished/broken: {casts:?}, \
         creature deaths/monster kills/spell hits/slay progress: {fights:?}"
    );
    assert!(
        fights.iter().all(|&n| n > 10),
        "fights barely exercised: {fights:?}"
    );
    assert!(gathered > 10, "gathering barely exercised: {gathered}");
    assert!(
        casts.iter().all(|&n| n > 10),
        "casts barely exercised: {casts:?}"
    );
    // Every kind but Configure, which the stream never sends.
    assert_eq!(
        by_kind.len(),
        17,
        "a command kind was never accepted: {by_kind:?}"
    );
    assert!(accepted > 100_000, "only {accepted} commands accepted");
    assert!(games_over > 200, "only {games_over} games finished");
}
