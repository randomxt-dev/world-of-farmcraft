//! A small test map and helpers that drive [`Game`] through its public API only.
#![allow(dead_code)]

use std::time::Duration;

use among_rules::*;

pub const BUTTON: Position = [0.0, 0.0, 0.0];
pub const LIGHTS: Position = [0.0, 200.0, 0.0];
pub const COMMS: Position = [0.0, 250.0, 0.0];
pub const O2: [Position; 2] = [[0.0, 300.0, 0.0], [0.0, 350.0, 0.0]];
pub const REACTOR: [Position; 2] = [[0.0, 400.0, 0.0], [0.0, 410.0, 0.0]];
pub const CRITICAL: Duration = Duration::from_secs(30);

pub fn id(n: u64) -> PlayerId {
    PlayerId(n)
}

/// Two common, two long (two steps each) and four short tasks, every spot 10+ yards apart.
pub fn layout() -> Layout {
    let task = |name: &str, kind, steps: &[Position]| TaskDef {
        name: name.into(),
        kind,
        steps: steps.iter().map(|&at| TaskStep::instant(at)).collect(),
    };
    Layout {
        emergency_button: BUTTON,
        tasks: vec![
            task("Swipe card", TaskKind::Common, &[[100.0, 0.0, 0.0]]),
            task("Fix wiring", TaskKind::Common, &[[100.0, 100.0, 0.0]]),
            task(
                "Align engine",
                TaskKind::Long,
                &[[200.0, 0.0, 0.0], [200.0, 50.0, 0.0]],
            ),
            task(
                "Fuel engines",
                TaskKind::Long,
                &[[300.0, 0.0, 0.0], [300.0, 50.0, 0.0]],
            ),
            task("Clear asteroids", TaskKind::Short, &[[400.0, 0.0, 0.0]]),
            task("Empty garbage", TaskKind::Short, &[[410.0, 0.0, 0.0]]),
            task("Prime shields", TaskKind::Short, &[[420.0, 0.0, 0.0]]),
            task("Calibrate", TaskKind::Short, &[[430.0, 0.0, 0.0]]),
        ],
        sabotages: vec![
            SabotageDef {
                kind: SabotageKind::Lights,
                stations: vec![LIGHTS],
                deadline: None,
            },
            SabotageDef {
                kind: SabotageKind::Comms,
                stations: vec![COMMS],
                deadline: None,
            },
            SabotageDef {
                kind: SabotageKind::Oxygen,
                stations: O2.to_vec(),
                deadline: Some(CRITICAL),
            },
            SabotageDef {
                kind: SabotageKind::Reactor,
                stations: REACTOR.to_vec(),
                deadline: Some(CRITICAL),
            },
        ],
    }
}

/// Players `1..=n` joined, player 1 hosting.
pub fn lobby_with(settings: Settings, n: u64) -> Game {
    let mut g = Game::new(settings, layout());
    for i in 1..=n {
        ok(&mut g, Command::Join { player: id(i) });
    }
    g
}

pub fn lobby(n: u64) -> Game {
    lobby_with(Settings::default(), n)
}

/// [`layout`] with every task step a cast of `secs` seconds.
pub fn cast_layout(secs: f32) -> Layout {
    let mut l = layout();
    for step in l.tasks.iter_mut().flat_map(|t| t.steps.iter_mut()) {
        *step = TaskStep::cast(step.at, secs);
    }
    l
}

/// Players `1..=n` on `layout`, started by player 1.
pub fn started_on(layout: Layout, settings: Settings, n: u64, seed: u64) -> Game {
    let mut g = Game::new(settings, layout);
    for i in 1..=n {
        ok(&mut g, Command::Join { player: id(i) });
    }
    ok(&mut g, Command::Start { by: id(1), seed });
    g
}

pub fn started_with(settings: Settings, n: u64, seed: u64) -> Game {
    let mut g = lobby_with(settings, n);
    ok(&mut g, Command::Start { by: id(1), seed });
    g
}

pub fn started(n: u64, seed: u64) -> Game {
    started_with(Settings::default(), n, seed)
}

/// No cooldowns in the way: the tests that use it are about range, targets and outcomes.
pub fn instant() -> Settings {
    Settings {
        initial_kill_cooldown: Duration::ZERO,
        emergency_cooldown: Duration::ZERO,
        sabotage_cooldown: Duration::ZERO,
        ..Settings::default()
    }
}

#[track_caller]
pub fn ok(g: &mut Game, cmd: Command) -> Vec<Event> {
    match g.apply(cmd.clone()) {
        Ok(events) => events,
        Err(r) => panic!("{cmd:?} rejected: {r:?} ({r})"),
    }
}

#[track_caller]
pub fn rejected(g: &mut Game, cmd: Command) -> Reject {
    match g.apply(cmd.clone()) {
        Ok(events) => panic!("{cmd:?} accepted: {events:?}"),
        Err(r) => r,
    }
}

pub fn tick(g: &mut Game, secs: f64) -> Vec<Event> {
    ok(
        g,
        Command::Tick {
            dt: Duration::from_secs_f64(secs),
        },
    )
}

pub fn put(g: &mut Game, player: PlayerId, position: Position) -> Vec<Event> {
    ok(g, Command::Move { player, position })
}

pub fn role_ids(g: &Game, role: Role) -> Vec<PlayerId> {
    g.players()
        .filter(|(_, p)| p.role == Some(role))
        .map(|(id, _)| id)
        .collect()
}

pub fn impostors(g: &Game) -> Vec<PlayerId> {
    role_ids(g, Role::Impostor)
}

pub fn crew(g: &Game) -> Vec<PlayerId> {
    role_ids(g, Role::Crewmate)
}

/// Where `player`'s `task` is used next.
pub fn task_spot(g: &Game, player: PlayerId, task: TaskId) -> Position {
    let t = g
        .player(player)
        .unwrap()
        .tasks
        .iter()
        .find(|t| t.task == task)
        .unwrap();
    g.layout().tasks[task.0].steps[t.step].at
}

/// Walks `player` to every step of every task and uses it; returns the last use's events.
pub fn finish_tasks(g: &mut Game, player: PlayerId) -> Vec<Event> {
    let mut last = Vec::new();
    let tasks: Vec<_> = g.player(player).unwrap().tasks.clone();
    for t in tasks {
        for step in t.step..g.layout().tasks[t.task.0].steps.len() {
            let spot = g.layout().tasks[t.task.0].steps[step].at;
            put(g, player, spot);
            last = ok(
                g,
                Command::UseTask {
                    player,
                    task: t.task,
                },
            );
        }
    }
    last
}

pub fn game_over(events: &[Event]) -> Option<(Team, WinReason)> {
    events.iter().find_map(|e| match *e {
        Event::GameOver { winner, reason, .. } => Some((winner, reason)),
        _ => None,
    })
}
