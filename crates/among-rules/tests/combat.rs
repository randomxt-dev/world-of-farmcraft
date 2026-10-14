//! Fights at task spots: creatures aggro, chase, swing, leash and respawn; players auto-attack and
//! cast their class spell; deaths on both sides; Slay steps; meetings stop it all.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

const WOLF: CreatureId = CreatureId(0);
const OTHER_WOLF: CreatureId = CreatureId(1);

/// Players `1..` with these classes on [`combat_layout`], everyone at the button (far from the
/// den), started with `settings`.
fn arena(settings: Settings, classes: &[Class], seed: u64) -> Game {
    let mut g = Game::new(settings, combat_layout());
    for (i, &class) in classes.iter().enumerate() {
        ok(
            &mut g,
            Command::Join {
                player: id(i as u64 + 1),
                class,
            },
        );
    }
    ok(&mut g, Command::Start { by: id(1), seed });
    for i in 1..=classes.len() as u64 {
        put(&mut g, id(i), BUTTON);
    }
    g
}

fn warriors(n: usize) -> Vec<Class> {
    vec![Class::Warrior; n]
}

/// Run the clock in 0.1 s steps, the server's tick, collecting every event.
fn run(g: &mut Game, secs: f64) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..(secs * 10.0).round() as u32 {
        out.extend(ok(
            g,
            Command::Tick {
                dt: Duration::from_millis(100),
            },
        ));
    }
    out
}

/// Standing `yards` north of the first wolf's home.
fn near_den(yards: f32) -> Position {
    [DEN[0], DEN[1] + yards, DEN[2]]
}

fn health(g: &Game, p: PlayerId) -> u32 {
    g.player(p).unwrap().health
}

#[test]
fn creatures_spawn_at_the_start() {
    let mut g = Game::new(instant(), combat_layout());
    for i in 1..=4 {
        ok(
            &mut g,
            Command::Join {
                player: id(i),
                class: Class::Mage,
            },
        );
    }
    let events = ok(&mut g, Command::Start { by: id(1), seed: 1 });
    assert!(events.contains(&Event::CreatureSpawned {
        creature: WOLF,
        at: DEN
    }));
    assert_eq!(g.creatures().len(), 2);
    assert!(g.creatures().iter().all(|c| c.alive && c.health == 40));
    assert_eq!(
        health(&g, id(1)),
        Settings::default().kits.get(Class::Mage).health
    );
}

#[test]
fn a_creature_turns_on_whoever_comes_close_chases_and_bites() {
    let mut g = arena(instant(), &warriors(5), 2);
    let p = id(2);
    put(&mut g, p, near_den(10.5));
    assert_eq!(run(&mut g, 1.0), [], "out of aggro range");
    put(&mut g, p, near_den(10.0));
    let events = run(&mut g, 2.0);
    assert_eq!(
        events[0],
        Event::CreatureAggro {
            creature: WOLF,
            player: p
        }
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::CreatureMoved { creature: WOLF, .. })));
    // It closes to 4 yd (inside 5 yd melee range) and bites at once on arrival.
    let wolf = &g.creatures()[0];
    assert!(within(wolf.position, near_den(10.0), 5.0));
    let bites: Vec<_> = (events.iter())
        .filter(|e| matches!(e, Event::CreatureHit { .. }))
        .collect();
    assert_eq!(bites.len(), 1);
    assert_eq!(health(&g, p), 115);
    // Then once per 2 s swing.
    let bites = run(&mut g, 4.0)
        .iter()
        .filter(|e| matches!(e, Event::CreatureHit { .. }))
        .count();
    assert_eq!(bites, 2);
}

#[test]
fn running_past_the_leash_sends_it_home_healed() {
    let mut g = arena(instant(), &warriors(5), 2);
    let p = id(2);
    put(&mut g, p, near_den(3.0));
    ok(
        &mut g,
        Command::Attack {
            player: p,
            creature: WOLF,
        },
    );
    run(&mut g, 1.0);
    assert!(g.creatures()[0].health < 40);
    put(&mut g, p, near_den(31.0));
    let events = run(&mut g, 0.1);
    assert!(
        events.contains(&Event::CreatureEvaded {
            creature: WOLF,
            at: DEN
        }),
        "{events:?}"
    );
    let wolf = &g.creatures()[0];
    assert_eq!((wolf.health, wolf.position, wolf.target), (40, DEN, None));
}

#[test]
fn auto_attack_kills_it_and_the_tapper_gets_the_kill() {
    let mut g = arena(instant(), &warriors(5), 2);
    let c = crew(&g)[0];
    put(&mut g, c, near_den(3.0));
    assert_eq!(
        ok(
            &mut g,
            Command::Attack {
                player: c,
                creature: WOLF
            }
        ),
        [
            Event::AttackStarted {
                player: c,
                creature: WOLF
            },
            Event::CreatureAggro {
                creature: WOLF,
                player: c
            },
        ]
    );
    // 9 per 2 s swing against 40 health: the fifth swing, at 8 s, kills.
    let events = run(&mut g, 8.1);
    let swings = events
        .iter()
        .filter(|e| matches!(e, Event::MeleeHit { .. }))
        .count();
    assert_eq!(swings, 5);
    assert!(events.contains(&Event::CreatureDied {
        creature: WOLF,
        credit: Some(c)
    }));
    assert!(events.contains(&Event::AttackStopped { player: c }));
    assert!(!g.creatures()[0].alive);
    assert_eq!(
        rejected(
            &mut g,
            Command::Attack {
                player: c,
                creature: WOLF
            }
        ),
        Reject::InvalidTarget
    );
    // It respawns 20 s later.
    let events = run(&mut g, 20.0);
    assert!(events.contains(&Event::CreatureSpawned {
        creature: WOLF,
        at: DEN
    }));
}

#[test]
fn two_kills_finish_a_slay_step_but_only_for_crewmates() {
    let mut g = arena(instant(), &warriors(5), 2);
    let c = crew(&g)[0];
    let slay = (g.player(c).unwrap().tasks.iter())
        .map(|t| t.task)
        .find(|t| g.layout().tasks[t.0].kind == TaskKind::Short)
        .unwrap();
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: c,
                task: slay
            }
        ),
        Reject::NotAUseStep
    );
    let mut all = Vec::new();
    for (wolf, x) in [(WOLF, 0.0), (OTHER_WOLF, 20.0)] {
        put(&mut g, c, [DEN[0] + x, DEN[1] + 3.0, DEN[2]]);
        ok(
            &mut g,
            Command::Attack {
                player: c,
                creature: wolf,
            },
        );
        all.extend(run(&mut g, 8.1));
    }
    assert!(all.contains(&Event::SlayProgress {
        player: c,
        task: slay,
        kills: 1,
        count: 2
    }));
    assert!(
        all.contains(&Event::TaskCompleted {
            player: c,
            task: slay
        }),
        "{all:?}"
    );

    // The impostor's kills are for show.
    let imp = impostors(&g)[0];
    run(&mut g, 20.0);
    put(&mut g, imp, near_den(3.0));
    ok(
        &mut g,
        Command::Attack {
            player: imp,
            creature: WOLF,
        },
    );
    let events = run(&mut g, 8.1);
    assert!(events.contains(&Event::CreatureDied {
        creature: WOLF,
        credit: Some(imp)
    }));
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::SlayProgress { .. })));
}

#[test]
fn the_first_to_hit_it_gets_the_credit() {
    let mut g = arena(instant(), &warriors(5), 2);
    let crew = crew(&g);
    let (first, finisher) = (crew[0], crew[1]);
    put(&mut g, first, near_den(3.0));
    put(&mut g, finisher, near_den(-3.0));
    ok(
        &mut g,
        Command::Attack {
            player: first,
            creature: WOLF,
        },
    );
    run(&mut g, 0.1);
    ok(&mut g, Command::StopAttack { player: first });
    ok(
        &mut g,
        Command::Attack {
            player: finisher,
            creature: WOLF,
        },
    );
    let events = run(&mut g, 8.0);
    assert!(events.contains(&Event::CreatureDied {
        creature: WOLF,
        credit: Some(first)
    }));
}

#[test]
fn a_cast_spell_needs_range_stands_still_and_has_a_cooldown() {
    let mut g = arena(instant(), &[Class::Mage; 5], 2);
    let m = id(2);
    let fireball = Command::CastSpell {
        player: m,
        creature: WOLF,
    };
    put(&mut g, m, near_den(36.0));
    assert_eq!(rejected(&mut g, fireball.clone()), Reject::OutOfRange);
    put(&mut g, m, near_den(30.0));
    assert_eq!(
        ok(&mut g, fireball.clone()),
        [Event::SpellCastStarted {
            player: m,
            creature: WOLF,
            time: Duration::from_millis(1500)
        }]
    );
    assert_eq!(rejected(&mut g, fireball.clone()), Reject::AlreadyCasting);
    // A step breaks it.
    assert_eq!(
        put(&mut g, m, near_den(29.0)),
        [Event::SpellCastInterrupted { player: m }]
    );
    ok(&mut g, fireball.clone());
    let events = run(&mut g, 1.5);
    assert!(events.contains(&Event::SpellHit {
        player: m,
        creature: WOLF,
        damage: 18,
        health: 22
    }));
    assert!(events.contains(&Event::CreatureAggro {
        creature: WOLF,
        player: m
    }));
    // The 1.5 s global cooldown starts as the spell lands.
    assert_eq!(rejected(&mut g, fireball.clone()), Reject::OnCooldown);
    run(&mut g, 1.5);
    ok(&mut g, fireball);
}

#[test]
fn an_instant_strike_lands_at_once() {
    let mut g = arena(instant(), &warriors(5), 2);
    let w = id(2);
    put(&mut g, w, near_den(4.0));
    let events = ok(
        &mut g,
        Command::CastSpell {
            player: w,
            creature: WOLF,
        },
    );
    assert_eq!(
        events,
        [
            Event::SpellHit {
                player: w,
                creature: WOLF,
                damage: 15,
                health: 25
            },
            Event::CreatureAggro {
                creature: WOLF,
                player: w
            },
        ]
    );
    assert_eq!(g.player(w).unwrap().spell_cooldown, Duration::from_secs(3));
}

#[test]
fn a_creature_can_kill_and_leaves_a_body_to_report() {
    let mut s = instant();
    let mut kit = *s.kits.get(Class::Mage);
    kit.health = 12;
    s.kits.set(Class::Mage, kit);
    let mut g = arena(s, &[Class::Mage; 6], 3);
    let victim = crew(&g)[0];
    put(&mut g, victim, near_den(4.0));
    // 5, 5, then the third bite at 4 s kills: 12 health.
    let events = run(&mut g, 4.1);
    assert!(
        events.contains(&Event::Slain {
            victim,
            creature: WOLF,
            at: near_den(4.0)
        }),
        "{events:?}"
    );
    assert!(events.contains(&Event::CreatureEvaded {
        creature: WOLF,
        at: DEN
    }));
    assert!(!g.player(victim).unwrap().alive);
    assert_eq!(
        g.bodies(),
        [Body {
            victim,
            position: near_den(4.0)
        }]
    );
    // The world shows a body like any other: anyone near can report it.
    let witness = crew(&g)[1];
    put(&mut g, witness, near_den(-4.0));
    let events = ok(
        &mut g,
        Command::Report {
            player: witness,
            body: victim,
        },
    );
    assert!(matches!(events[0], Event::MeetingCalled { .. }));
}

#[test]
fn a_monster_kill_can_hand_the_impostors_the_win() {
    let mut s = instant();
    let mut kit = *s.kits.get(Class::Warrior);
    kit.health = 5;
    s.kits.set(Class::Warrior, kit);
    let mut g = arena(s, &warriors(4), 4);
    let (imp, crew) = (impostors(&g)[0], crew(&g));
    // Two crewmates die to wolves: one crewmate left against one impostor.
    put(&mut g, crew[0], near_den(4.0));
    run(&mut g, 0.1);
    put(&mut g, crew[1], [DEN[0] + 20.0, DEN[1] + 4.0, DEN[2]]);
    let events = run(&mut g, 0.1);
    assert_eq!(
        game_over(&events),
        Some((Team::Impostors, WinReason::Outnumbered))
    );
    assert!(g.player(imp).unwrap().alive);
}

#[test]
fn a_meeting_stops_every_fight_and_heals_the_living() {
    let mut g = arena(instant(), &warriors(5), 2);
    let crew = crew(&g);
    put(&mut g, crew[0], near_den(3.0));
    ok(
        &mut g,
        Command::Attack {
            player: crew[0],
            creature: WOLF,
        },
    );
    run(&mut g, 3.0);
    assert!(health(&g, crew[0]) < 120);
    let events = ok(&mut g, Command::CallEmergency { player: crew[1] });
    assert!(events.contains(&Event::AttackStopped { player: crew[0] }));
    assert!(events.contains(&Event::CreatureEvaded {
        creature: WOLF,
        at: DEN
    }));
    // Back from the meeting away from the den, or the wolf bites again as play resumes.
    put(&mut g, crew[0], BUTTON);
    run(&mut g, 15.0 + 120.0 + 6.0);
    assert_eq!(g.phase(), &Phase::Playing);
    assert_eq!(health(&g, crew[0]), 120);
}

#[test]
fn health_comes_back_out_of_combat() {
    let mut g = arena(instant(), &warriors(5), 2);
    let p = id(2);
    put(&mut g, p, near_den(4.0));
    run(&mut g, 2.1);
    let hurt = health(&g, p);
    assert_eq!(hurt, 110);
    // Out of reach of the leash: the wolf gives up, and 2 health a second comes back.
    put(&mut g, p, BUTTON);
    run(&mut g, 3.0);
    assert_eq!(health(&g, p), 116);
    run(&mut g, 10.0);
    assert_eq!(health(&g, p), 120, "capped at the kit's health");
}

#[test]
fn ghosts_neither_fight_nor_draw_aggro() {
    let mut g = arena(instant(), &warriors(5), 2);
    let (imp, crew) = (impostors(&g)[0], crew(&g));
    let ghost = crew[0];
    put(&mut g, imp, [0.0; 3]);
    put(&mut g, ghost, [0.0; 3]);
    ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: ghost,
        },
    );
    put(&mut g, ghost, near_den(2.0));
    assert_eq!(
        rejected(
            &mut g,
            Command::Attack {
                player: ghost,
                creature: WOLF
            }
        ),
        Reject::Dead
    );
    assert_eq!(run(&mut g, 2.0), []);
}

#[test]
fn a_ghost_passes_the_slay_steps_it_can_no_longer_fight() {
    // Every long task's second step is a Slay step, after an instant one.
    let mut layout = combat_layout();
    for t in layout.tasks.iter_mut().filter(|t| t.kind == TaskKind::Long) {
        t.steps[1].action = StepAction::Slay {
            creatures: vec![WOLF, OTHER_WOLF],
            count: 2,
        };
    }
    let mut g = Game::new(instant(), layout);
    for i in 1..=5 {
        ok(
            &mut g,
            Command::Join {
                player: id(i),
                class: Class::Warrior,
            },
        );
    }
    ok(&mut g, Command::Start { by: id(1), seed: 2 });
    let (imp, ghost) = (impostors(&g)[0], crew(&g)[0]);
    put(&mut g, imp, BUTTON);
    put(&mut g, ghost, BUTTON);
    let kind = |g: &Game, task: TaskId| g.layout().tasks[task.0].kind;
    let shorts: Vec<TaskId> = (g.player(ghost).unwrap().tasks.iter())
        .map(|t| t.task)
        .filter(|&t| kind(&g, t) == TaskKind::Short)
        .collect();
    assert!(!shorts.is_empty());

    // Killed standing at a Slay step: it is passed at once.
    let events = ok(
        &mut g,
        Command::Kill {
            killer: imp,
            victim: ghost,
        },
    );
    for task in &shorts {
        assert!(
            events.contains(&Event::TaskCompleted {
                player: ghost,
                task: *task
            }),
            "{events:?}"
        );
    }

    // A ghost finishing a step onto a Slay step passes that one too.
    let long = (g.player(ghost).unwrap().tasks.iter())
        .map(|t| t.task)
        .find(|&t| kind(&g, t) == TaskKind::Long)
        .unwrap();
    let first = g.layout().tasks[long.0].steps[0].at;
    put(&mut g, ghost, first);
    let events = ok(
        &mut g,
        Command::UseTask {
            player: ghost,
            task: long,
        },
    );
    assert!(
        events.contains(&Event::TaskCompleted {
            player: ghost,
            task: long
        }),
        "{events:?}"
    );

    // A living crewmate still has to fight theirs.
    let living = crew(&g)[1];
    let theirs = (g.player(living).unwrap().tasks.iter())
        .find(|t| kind(&g, t.task) == TaskKind::Short)
        .unwrap();
    assert_eq!(theirs.step, 0);
}
