//! The server against `benilla-protocol`'s own client, over real sockets: logon, roster, world
//! entry, one player's movement reaching the other, and a game driven by HUD button presses. No
//! game data needed.

use std::time::{Duration, Instant};

use among_rules::wire::{self, Notice, Request};
use among_rules::{Role, Settings};
use among_server::{Accounts, Server, ServerConfig};
use benilla_protocol::messages::{opcode, Object, ObjectType, ServerPacket, CHAT_TYPE_PARTY};
use benilla_protocol::WorldSession;

/// Start a server on free ports with two accounts; returns realmd's `host:port`.
fn start_server() -> String {
    start_server_with(2, Settings::default())
}

/// Start a server on free ports with accounts `among1..=n` (password `secretN`).
fn start_server_with(n: u32, settings: Settings) -> String {
    start_server_full(n, settings, temp_marks("default"))
}

/// A marks file in a fresh temp folder, so a test never writes the repo's survey.
fn temp_marks(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("among-loopback-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("marks.toml")
}

fn start_server_full(n: u32, settings: Settings, marks: std::path::PathBuf) -> String {
    let accounts = Accounts::default();
    for i in 1..=n {
        accounts
            .register(&format!("among{i}"), &format!("secret{i}"))
            .unwrap();
    }
    let config = ServerConfig {
        auth_port: 0,
        world_port: 0,
        settings,
        marks,
        ..ServerConfig::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let server = Server::bind(&config, accounts).await.unwrap();
            tx.send(server.auth_addr().to_string()).unwrap();
            server.serve().await.unwrap();
        });
    });
    rx.recv().unwrap()
}

/// Log on, read the roster and enter the world with its first character; returns the session and
/// that character's guid.
fn enter(host: &str, user: &str, pass: &str) -> (WorldSession, u64) {
    let logon = benilla_protocol::logon(host, user, pass).expect("realmd logon");
    assert_eq!(logon.realms.len(), 1);
    let mut session =
        WorldSession::connect(&logon.realms[0].address, user, logon.session_key).expect("world");
    session
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let roster = session.char_enum().expect("roster");
    assert_eq!(roster.len(), 1, "a fresh account gets one character");
    let guid = roster[0].guid;
    session.player_login(guid).unwrap();
    session.set_active_mover(guid).unwrap();
    (session, guid)
}

/// Read packets until `want` matches one, failing after a few seconds.
fn wait_for<T>(session: &mut WorldSession, mut want: impl FnMut(&ServerPacket) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let packet = session.recv().expect("a packet before the deadline");
        if let Some(found) = want(&packet) {
            return found;
        }
    }
    panic!("the packet never came");
}

/// The guid of a player created in this update, if any.
fn created_player(packet: &ServerPacket) -> Option<u64> {
    let ServerPacket::UpdateObject { objects } = packet else {
        return None;
    };
    objects.iter().find_map(|o| match o {
        Object::Create {
            guid,
            object_type: ObjectType::Player,
            ..
        } => Some(*guid),
        _ => None,
    })
}

/// The guids of the gameobjects created in this update.
fn created_objects(packet: &ServerPacket) -> Vec<u64> {
    let ServerPacket::UpdateObject { objects } = packet else {
        return Vec::new();
    };
    (objects.iter())
        .filter_map(|o| match o {
            Object::Create {
                guid,
                object_type: ObjectType::GameObject,
                ..
            } => Some(*guid),
            _ => None,
        })
        .collect()
}

#[test]
fn a_client_logs_in_and_another_sees_it_move() {
    let host = start_server();

    let (mut one, one_guid) = enter(&host, "among1", "secret1");
    let map = wait_for(&mut one, |p| match p {
        ServerPacket::LoginVerifyWorld { map, .. } => Some(*map),
        _ => None,
    });
    assert_eq!(map, 0);
    // The spell book and the action bar: auto-attack, the class spell, then the buttons.
    let book = wait_for(&mut one, |p| match p {
        ServerPacket::InitialSpells { spell_ids, .. } => Some(spell_ids.clone()),
        _ => None,
    });
    assert!(book.contains(&6603) && book.contains(&15692), "{book:?}");
    let bar = wait_for(&mut one, |p| match p {
        ServerPacket::ActionButtons { buttons } => Some(buttons.len()),
        _ => None,
    });
    assert_eq!(bar + 2, book.len(), "the bar, then the two node openers");
    assert_eq!(wait_for(&mut one, created_player), one_guid, "its own body");

    let (mut two, two_guid) = enter(&host, "among2", "secret2");
    // The second client sees itself and the first; the first sees the second arrive.
    let mut seen = vec![
        wait_for(&mut two, created_player),
        wait_for(&mut two, created_player),
    ];
    seen.sort_unstable();
    let mut both = vec![one_guid, two_guid];
    both.sort_unstable();
    assert_eq!(seen, both);
    assert_eq!(wait_for(&mut one, created_player), two_guid);

    // One walks forward; two hears it under the same opcode, with one's guid and pose.
    one.start_forward([-8950.0, -130.0, 83.5], 1.5).unwrap();
    let (guid, op, position) = wait_for(&mut two, |p| match p {
        ServerPacket::PlayerMove {
            guid,
            opcode,
            position,
            ..
        } => Some((*guid, *opcode, *position)),
        _ => None,
    });
    assert_eq!(guid, one_guid);
    assert_eq!(op, opcode::MSG_MOVE_START_FORWARD);
    assert_eq!(
        [position.x, position.y, position.z],
        [-8950.0, -130.0, 83.5]
    );
}

#[test]
fn a_wrong_password_is_refused_at_logon() {
    let host = start_server();
    let err = match benilla_protocol::logon(&host, "among1", "nope") {
        Ok(_) => panic!("a wrong password must not log on"),
        Err(e) => e,
    };
    let reject = err
        .downcast_ref::<benilla_protocol::AuthReject>()
        .expect("a typed refusal");
    assert_eq!(reject.code, 0x04);
}

/// Press a HUD button the way the HUD will: an addon line on the party lane.
fn press(session: &mut WorldSession, request: Request) {
    let text = format!("{}\t{}", wire::PREFIX, request.encode());
    session
        .send_addon_message(CHAT_TYPE_PARTY, None, &text)
        .unwrap();
}

/// The next Among Azeroth notice `want` accepts, skipping everything else.
fn wait_notice<T>(session: &mut WorldSession, mut want: impl FnMut(&Notice) -> Option<T>) -> T {
    wait_for(session, |p| {
        let ServerPacket::MessageChat(m) = p else {
            return None;
        };
        let payload = m.text.strip_prefix(wire::PREFIX)?.strip_prefix('\t')?;
        assert!(m.is_addon(), "{m:?}");
        want(&Notice::decode(payload).expect(payload))
    })
}

#[test]
fn three_clients_start_a_game_by_button_and_a_kill_press_lands() {
    let settings = Settings {
        min_players: 3,
        initial_kill_cooldown: Duration::ZERO,
        ..Settings::default()
    };
    let host = start_server_with(3, settings);
    // One at a time, each told the host before the next logs in, so "first" is unambiguous.
    let mut players: Vec<(WorldSession, u64)> = Vec::new();
    for i in 1..=3 {
        let (mut session, guid) = enter(&host, &format!("among{i}"), &format!("secret{i}"));
        let told = wait_notice(&mut session, |n| match n {
            Notice::Host(h) => Some(*h),
            _ => None,
        });
        let first = players.first().map_or(guid, |(_, g)| *g);
        assert_eq!(
            told,
            Some(among_rules::PlayerId(first)),
            "the first in hosts"
        );
        players.push((session, guid));
    }

    press(&mut players[0].0, Request::Start);
    let roles: Vec<Role> = (players.iter_mut())
        .map(|(s, _)| {
            wait_notice(s, |n| match n {
                Notice::Role { role, .. } => Some(*role),
                _ => None,
            })
        })
        .collect();
    assert_eq!(roles.iter().filter(|r| **r == Role::Impostor).count(), 1);
    let imp = roles.iter().position(|r| *r == Role::Impostor).unwrap();

    // Everyone stands on the spawn: the impostor's Kill lights up, and the press lands.
    wait_notice(&mut players[imp].0, |n| match n {
        Notice::Actions(a) if a.kill => Some(()),
        _ => None,
    });
    press(&mut players[imp].0, Request::Kill);
    // Each crewmate hears either its own death or, first, the game ending (1 crew vs 1 impostor).
    let told_dead = (0..3)
        .filter(|&i| i != imp)
        .filter(|&i| {
            wait_notice(&mut players[i].0, |n| match n {
                Notice::Dead => Some(true),
                Notice::Over { .. } => Some(false),
                _ => None,
            })
        })
        .count();
    assert_eq!(told_dead, 1, "exactly one victim is told");
    let (winner, impostors) = wait_notice(&mut players[imp].0, |n| match n {
        Notice::Over {
            winner, impostors, ..
        } => Some((*winner, impostors.clone())),
        _ => None,
    });
    assert_eq!(winner, among_rules::Team::Impostors);
    assert_eq!(impostors, [among_rules::PlayerId(players[imp].1)]);
}

#[test]
fn the_action_bar_starts_a_game_and_kills_by_casting() {
    let settings = Settings {
        min_players: 3,
        initial_kill_cooldown: Duration::ZERO,
        ..Settings::default()
    };
    let host = start_server_with(3, settings);
    let mut players: Vec<(WorldSession, u64)> = Vec::new();
    for i in 1..=3 {
        let (mut session, guid) = enter(&host, &format!("among{i}"), &format!("secret{i}"));
        wait_notice(&mut session, |n| matches!(n, Notice::Host(_)).then_some(()));
        players.push((session, guid));
    }
    // The host sounds the horn (Emergency) in the lobby: the game starts, the press is answered.
    players[0].0.cast_spell(18946, None).unwrap();
    let roles: Vec<Role> = (players.iter_mut())
        .map(|(s, _)| {
            wait_notice(s, |n| match n {
                Notice::Role { role, .. } => Some(*role),
                _ => None,
            })
        })
        .collect();
    // The game's events come first, then the answer to the press.
    let answered = wait_for(&mut players[0].0, |p| match p {
        ServerPacket::SpellGo(go) if go.spell_id == 18946 => Some(go.caster),
        _ => None,
    });
    assert_eq!(answered, players[0].1);
    let imp = roles.iter().position(|r| *r == Role::Impostor).unwrap();
    let victim = (0..3).find(|&i| i != imp).unwrap();
    let victim_guid = players[victim].1;

    // A crewmate's Kill is refused silently, the reason as red text.
    let crew = (0..3).find(|&i| i != imp && i != victim).unwrap();
    players[crew]
        .0
        .cast_spell(15692, Some(victim_guid))
        .unwrap();
    let why = wait_for(&mut players[crew].0, |p| match p {
        ServerPacket::Notification { text } => Some(text.clone()),
        _ => None,
    });
    assert_eq!(why, "Only impostors can do that");

    // The impostor casts Kill at the victim; the victim is told it died, then sees the strike.
    players[imp].0.cast_spell(15692, Some(victim_guid)).unwrap();
    wait_notice(&mut players[victim].0, |n| {
        (*n == Notice::Dead).then_some(())
    });
    let (caster, hits) = wait_for(&mut players[victim].0, |p| match p {
        ServerPacket::SpellGo(go) if go.spell_id == 15692 => Some((go.caster, go.hits.clone())),
        _ => None,
    });
    assert_eq!((caster, hits), (players[imp].1, vec![victim_guid]));
}

#[test]
fn a_newcomer_sees_every_task_node_and_learns_its_template() {
    let host = start_server();
    let (mut one, _) = enter(&host, "among1", "secret1");
    let layout = among_server::game::placeholder_layout();
    // Slay steps are fought, not worked at a node.
    let steps = (layout.tasks.iter())
        .flat_map(|t| &t.steps)
        .filter(|s| matches!(s.action, among_rules::StepAction::Cast(_)))
        .count();
    let mut nodes = Vec::new();
    while nodes.len() < steps {
        nodes.extend(wait_for(&mut one, |p| {
            let created = created_objects(p);
            (!created.is_empty()).then_some(created)
        }));
    }
    assert_eq!(nodes.len(), steps, "one node per cast step");
    // The client asks for a node's template by the entry in its guid.
    let entry = benilla_protocol::guid::entry(nodes[0]).unwrap();
    one.gameobject_query(entry, nodes[0]).unwrap();
    let (answered, name) = wait_for(&mut one, |p| match p {
        ServerPacket::GameObjectQueryResponse {
            entry,
            info: Some(info),
        } => Some((*entry, info.name.clone())),
        _ => None,
    });
    assert_eq!(answered, entry);
    assert!(!name.is_empty());
}

#[test]
fn a_started_game_shows_the_wolves_and_a_swing_at_one_is_echoed() {
    let settings = Settings {
        min_players: 3,
        ..Settings::default()
    };
    let host = start_server_with(3, settings);
    let mut players: Vec<(WorldSession, u64)> = Vec::new();
    for i in 1..=3 {
        let (mut session, guid) = enter(&host, &format!("among{i}"), &format!("secret{i}"));
        wait_notice(&mut session, |n| matches!(n, Notice::Host(_)).then_some(()));
        players.push((session, guid));
    }
    press(&mut players[0].0, Request::Start);
    let (one, me) = &mut players[1];
    let wolf = wait_for(one, |p| {
        let ServerPacket::UpdateObject { objects } = p else {
            return None;
        };
        objects.iter().find_map(|o| match o {
            Object::Create {
                guid,
                object_type: ObjectType::Unit,
                ..
            } => Some(*guid),
            _ => None,
        })
    });
    let entry = benilla_protocol::guid::entry(wolf).unwrap();
    one.creature_query(entry, wolf).unwrap();
    let name = wait_for(one, |p| match p {
        ServerPacket::CreatureQueryResponse {
            info: Some(info), ..
        } => Some(info.name.clone()),
        _ => None,
    });
    assert_eq!(name, "Young Wolf");
    // Auto-attack may start out of reach; the server's echo is what engages the client.
    one.attack_swing(wolf).unwrap();
    let (attacker, victim) = wait_for(one, |p| match p {
        ServerPacket::AttackStart { attacker, victim } if *attacker == *me => {
            Some((*attacker, *victim))
        }
        _ => None,
    });
    assert_eq!((attacker, victim), (*me, wolf));
}

/// The next system chat line, skipping everything else.
fn wait_system_line(session: &mut WorldSession) -> String {
    wait_for(session, |p| match p {
        ServerPacket::MessageChat(m) if m.chat_type == 0x0A => Some(m.text.clone()),
        _ => None,
    })
}

#[test]
fn typing_mark_in_chat_records_where_you_stand() {
    let marks = temp_marks("survey");
    let host = start_server_full(1, Settings::default(), marks.clone());
    let (mut me, _) = enter(&host, "among1", "secret1");
    // Walk somewhere, then mark it the way a surveyor would: a /say line.
    me.start_forward([-8930.5, -120.25, 84.0], 0.5).unwrap();
    me.send_chat(".mark herb-north").unwrap();
    let reply = wait_system_line(&mut me);
    assert_eq!(
        reply,
        "Marked herb-north at map 0 (-8930.50, -120.25, 84.00) facing 0.50"
    );
    let saved = among_server::survey::read_marks(&marks).unwrap();
    assert_eq!(saved["herb-north"].at, [-8930.5, -120.25, 84.0]);
    // Ordinary chat is left alone; unknown dot commands get the usage line.
    me.send_chat("hello").unwrap();
    me.send_chat(".nonsense").unwrap();
    assert!(wait_system_line(&mut me).starts_with("Survey: .loc"));
}

#[test]
fn typing_speed_in_chat_makes_you_run_faster() {
    let host = start_server_full(1, Settings::default(), temp_marks("speed"));
    let (mut me, guid) = enter(&host, "among1", "secret1");
    me.send_chat(".speed 3").unwrap();
    let (to, speed) = wait_for(&mut me, |p| match p {
        ServerPacket::ForceSpeedChange { guid, speed, .. } => Some((*guid, *speed)),
        _ => None,
    });
    assert_eq!((to, speed), (guid, 21.0), "3 x the 7 yd/s base run speed");
    assert_eq!(wait_system_line(&mut me), "Run speed x3 (21.0 yd/s)");
    me.send_chat(".speed 50").unwrap();
    assert_eq!(
        wait_system_line(&mut me),
        "Usage: .speed <0.5 to 10>, 1 is normal"
    );
}
