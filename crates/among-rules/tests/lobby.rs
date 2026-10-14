//! Lobby, host, settings, start, role and task assignment, back to the lobby.

mod common;

use among_rules::*;
use common::*;

#[test]
fn the_first_to_join_hosts() {
    let mut g = Game::new(Settings::default(), layout());
    let events = ok(
        &mut g,
        Command::Join {
            player: id(7),
            class: Class::Warrior,
        },
    );
    assert_eq!(
        events,
        [
            Event::Joined { player: id(7) },
            Event::HostChanged { host: Some(id(7)) }
        ]
    );
    assert_eq!(
        ok(
            &mut g,
            Command::Join {
                player: id(3),
                class: Class::Warrior
            }
        ),
        [Event::Joined { player: id(3) }]
    );
    assert_eq!(g.host(), Some(id(7)));
    assert_eq!(
        rejected(
            &mut g,
            Command::Join {
                player: id(3),
                class: Class::Warrior
            }
        ),
        Reject::AlreadyJoined
    );
}

#[test]
fn a_full_lobby_refuses() {
    let mut g = lobby_with(
        Settings {
            max_players: 4,
            ..Settings::default()
        },
        4,
    );
    assert_eq!(
        rejected(
            &mut g,
            Command::Join {
                player: id(5),
                class: Class::Warrior
            }
        ),
        Reject::LobbyFull
    );
}

#[test]
fn the_host_leaving_hands_over_to_the_lowest_id() {
    let mut g = lobby(3);
    let events = ok(&mut g, Command::Leave { player: id(1) });
    assert_eq!(
        events,
        [
            Event::Left { player: id(1) },
            Event::HostChanged { host: Some(id(2)) }
        ]
    );
    assert!(g.player(id(1)).is_none());
}

#[test]
fn only_the_host_configures_and_starts() {
    let mut g = lobby(5);
    let settings = Settings {
        impostors: 1,
        ..Settings::default()
    };
    assert_eq!(
        rejected(
            &mut g,
            Command::Configure {
                by: id(2),
                settings: Box::new(settings.clone())
            }
        ),
        Reject::NotHost
    );
    assert_eq!(
        ok(
            &mut g,
            Command::Configure {
                by: id(1),
                settings: Box::new(settings.clone())
            }
        ),
        [Event::SettingsChanged]
    );
    assert_eq!(g.settings(), &settings);
    assert_eq!(
        rejected(&mut g, Command::Start { by: id(2), seed: 0 }),
        Reject::NotHost
    );
    ok(&mut g, Command::Start { by: id(1), seed: 0 });
    assert_eq!(g.phase(), &Phase::Playing);
}

#[test]
fn start_needs_min_players_and_never_fewer_than_three() {
    let mut g = lobby(3);
    assert_eq!(
        rejected(&mut g, Command::Start { by: id(1), seed: 0 }),
        Reject::NotEnoughPlayers
    );
    let mut g = lobby_with(
        Settings {
            min_players: 1,
            ..Settings::default()
        },
        2,
    );
    assert_eq!(
        rejected(&mut g, Command::Start { by: id(1), seed: 0 }),
        Reject::NotEnoughPlayers
    );
    let mut g = lobby_with(
        Settings {
            min_players: 1,
            ..Settings::default()
        },
        3,
    );
    ok(&mut g, Command::Start { by: id(1), seed: 0 });
}

#[test]
fn impostor_count_is_capped_by_player_count() {
    for (players, expected) in [(4, 1), (6, 1), (7, 2), (8, 2), (10, 2)] {
        let g = started(players, 42);
        assert_eq!(impostors(&g).len(), expected, "{players} players");
        assert_eq!(crew(&g).len(), players as usize - expected);
    }
    let g = started_with(
        Settings {
            impostors: 3,
            ..Settings::default()
        },
        10,
        42,
    );
    assert_eq!(impostors(&g).len(), 3);
    let g = started_with(
        Settings {
            impostors: 0,
            ..Settings::default()
        },
        10,
        42,
    );
    assert_eq!(impostors(&g).len(), 1);
}

#[test]
fn roles_are_private_and_impostors_know_each_other() {
    let mut g = lobby(8);
    let events = ok(&mut g, Command::Start { by: id(1), seed: 9 });
    assert_eq!(
        events[0],
        Event::GameStarted {
            players: 8,
            impostors: 2
        }
    );
    let imps = impostors(&g);
    let mut reveals = 0;
    for e in &events {
        if let Event::RoleAssigned {
            player,
            role,
            fellow_impostors,
        } = e
        {
            reveals += 1;
            assert_eq!(e.private_to(), Some(*player));
            if *role == Role::Impostor {
                let others: Vec<_> = imps.iter().copied().filter(|i| i != player).collect();
                assert_eq!(fellow_impostors, &others);
            } else {
                assert!(fellow_impostors.is_empty());
            }
        }
    }
    assert_eq!(reveals, 8);
}

#[test]
fn the_seed_decides_the_roles() {
    let roles = |seed| impostors(&started(10, seed));
    assert_eq!(roles(1), roles(1));
    let distinct: std::collections::HashSet<_> = (0..20).map(roles).collect();
    assert!(
        distinct.len() > 10,
        "20 seeds gave only {} role sets",
        distinct.len()
    );
}

#[test]
fn every_player_draws_the_common_tasks_plus_their_own() {
    let mut g = lobby(6);
    let events = ok(&mut g, Command::Start { by: id(1), seed: 3 });
    let lists: Vec<&Vec<TaskId>> = (events.iter())
        .filter_map(|e| match e {
            Event::TasksAssigned { tasks, .. } => Some(tasks),
            _ => None,
        })
        .collect();
    assert_eq!(lists.len(), 6);
    let kind = |t: &TaskId| g.layout().tasks[t.0].kind;
    for tasks in &lists {
        assert_eq!(tasks.len(), 4);
        assert_eq!(kind(&tasks[0]), TaskKind::Common);
        assert_eq!(tasks[0], lists[0][0], "common tasks are shared");
        assert_eq!(
            tasks.iter().filter(|t| kind(t) == TaskKind::Long).count(),
            1
        );
        assert_eq!(
            tasks.iter().filter(|t| kind(t) == TaskKind::Short).count(),
            2
        );
    }
    // Impostor lists are cover stories: only the five crewmates' 4 tasks count.
    assert_eq!(events.last(), Some(&Event::TaskBar { done: 0, total: 20 }));
}

#[test]
fn a_running_game_refuses_joins_and_lobby_commands() {
    let mut g = started(5, 0);
    assert_eq!(
        rejected(
            &mut g,
            Command::Join {
                player: id(9),
                class: Class::Warrior
            }
        ),
        Reject::WrongPhase
    );
    assert_eq!(
        rejected(&mut g, Command::Start { by: id(1), seed: 0 }),
        Reject::WrongPhase
    );
    assert_eq!(
        rejected(&mut g, Command::ReturnToLobby { by: id(1) }),
        Reject::WrongPhase
    );
}

#[test]
fn after_game_over_the_host_returns_everyone_connected_to_the_lobby() {
    let mut g = started(4, 5);
    let imp = impostors(&g)[0];
    let events = ok(&mut g, Command::Leave { player: imp });
    assert_eq!(
        game_over(&events),
        Some((Team::Crew, WinReason::ImpostorsEliminated))
    );
    let host = g.host().unwrap();
    assert_eq!(
        ok(&mut g, Command::ReturnToLobby { by: host }),
        [Event::ReturnedToLobby]
    );
    assert_eq!(g.phase(), &Phase::Lobby);
    assert_eq!(g.players().count(), 3);
    assert!(g
        .players()
        .all(|(_, p)| p.role.is_none() && p.alive && p.tasks.is_empty()));
    ok(
        &mut g,
        Command::Join {
            player: id(9),
            class: Class::Warrior,
        },
    );
    ok(&mut g, Command::Start { by: host, seed: 6 });
}
