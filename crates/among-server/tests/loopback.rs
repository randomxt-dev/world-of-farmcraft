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
    start_server_for(accounts, settings, marks)
}

/// Start a server on free ports for `accounts`; returns realmd's `host:port`.
fn start_server_for(accounts: Accounts, settings: Settings, marks: std::path::PathBuf) -> String {
    let config = ServerConfig {
        auth_port: 0,
        world_port: 0,
        settings,
        marks,
        // Games start on the press; the countdown has its own host test.
        start_countdown: Duration::ZERO,
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
fn wait_for<T>(session: &mut WorldSession, want: impl FnMut(&ServerPacket) -> Option<T>) -> T {
    wait_for_secs(session, 5, want)
}

/// [`wait_for`] with a deadline of `secs` seconds, for what waits on a cast.
fn wait_for_secs<T>(
    session: &mut WorldSession,
    secs: u64,
    mut want: impl FnMut(&ServerPacket) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    // A cast can be silent for longer than the socket's usual timeout.
    session
        .set_read_timeout(Some(Duration::from_secs(secs)))
        .unwrap();
    while Instant::now() < deadline {
        let packet = session.recv().expect("a packet before the deadline");
        if let Some(found) = want(&packet) {
            session
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
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
    assert_eq!(
        bar + 4,
        book.len(),
        "the lobby's crewmate bar, then Kill, Sabotage and the two node openers"
    );
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

#[test]
fn an_open_lobby_lets_any_name_in_with_its_code_and_no_other_password() {
    let accounts = Accounts::open_lobby("K3XQ9P").unwrap();
    let host = start_server_for(accounts, Settings::default(), temp_marks("lobby"));
    let (_session, guid) = enter(&host, "thrall", "k3xq9p");
    assert_ne!(guid, 0);
    let err = match benilla_protocol::logon(&host, "jaina", "guess1") {
        Ok(_) => panic!("only the lobby code logs on"),
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
fn the_button_starts_a_game_and_the_bar_kills_by_casting() {
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
    // The host right-clicks the emergency button in the lobby: the game starts.
    let button = among_server::world::guid::Fixture::Button.guid();
    players[0].0.gameobj_use(button).unwrap();
    let roles: Vec<Role> = (players.iter_mut())
        .map(|(s, _)| {
            wait_notice(s, |n| match n {
                Notice::Role { role, .. } => Some(*role),
                _ => None,
            })
        })
        .collect();
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

/// An item object created for its owner: its guid and template entry.
type ItemObject = (u64, Option<u32>);

/// The item objects and the player's own fields in the update that creates `guid`'s body.
fn self_create_with_items(
    packet: &ServerPacket,
    guid: u64,
) -> Option<(Vec<ItemObject>, benilla_protocol::messages::ObjectFields)> {
    let ServerPacket::UpdateObject { objects } = packet else {
        return None;
    };
    let mut items = Vec::new();
    for o in objects {
        match o {
            Object::Create {
                guid: g,
                object_type: ObjectType::Item,
                mask,
                ..
            } => items.push((*g, mask.object_entry())),
            Object::Create {
                guid: g,
                object_type: ObjectType::Player,
                mask,
                ..
            } if *g == guid => return Some((items, mask.clone())),
            _ => {}
        }
    }
    None
}

#[test]
fn a_client_wears_its_starting_gear_and_learns_each_item() {
    // among1 plays a human warrior, made up front (a fresh account's own class is random).
    let accounts = Accounts::default();
    accounts.register("among1", "secret1").unwrap();
    accounts.register("among2", "secret2").unwrap();
    let warrior = among_server::accounts::CharacterRecord {
        guid: 0,
        name: "Among".into(),
        race: 1,
        class: 1,
        gender: 0,
        skin: 0,
        face: 0,
        hair_style: 0,
        hair_color: 0,
        facial_hair: 0,
        level: 1,
    };
    accounts.create_character("AMONG1", warrior).unwrap();
    let host = start_server_for(accounts, Settings::default(), temp_marks("gear"));
    let (mut one, one_guid) = enter(&host, "among1", "secret1");
    // The human warrior's recruit kit and the Mining Pick, ahead of the body.
    let (items, me) = wait_for(&mut one, |p| self_create_with_items(p, one_guid));
    let entries: Vec<Option<u32>> = items.iter().map(|(_, e)| *e).collect();
    assert_eq!(
        entries,
        [
            Some(38),
            Some(39),
            Some(40),
            Some(25),
            Some(2362),
            Some(2901)
        ]
    );
    for (slot, (guid, _)) in [3u8, 6, 7, 15, 16].into_iter().zip(&items) {
        assert_eq!(me.player_inv_slot(slot), Some(*guid), "slot {slot}");
    }
    assert_eq!(
        me.player_pack_slot(0),
        Some(items[5].0),
        "the pick in the backpack"
    );
    assert_eq!(me.player_visible_item_entry(15), Some(25));

    // The character sheet asks for each template; the sword comes back whole.
    one.item_query(25, items[3].0).unwrap();
    let (entry, name, delay) = wait_for(&mut one, |p| match p {
        ServerPacket::ItemQueryResponse {
            entry,
            info: Some(info),
        } => Some((*entry, info.name.clone(), info.delay_ms)),
        _ => None,
    });
    assert_eq!((entry, name.as_str(), delay), (25, "Worn Shortsword", 1900));

    // Another client sees the gear on the body, but no item objects.
    let (mut two, _) = enter(&host, "among2", "secret2");
    let seen = wait_for(&mut two, |p| {
        let ServerPacket::UpdateObject { objects } = p else {
            return None;
        };
        objects.iter().find_map(|o| match o {
            Object::Create {
                guid,
                object_type: ObjectType::Player,
                mask,
                ..
            } if *guid == one_guid => Some(mask.clone()),
            _ => None,
        })
    });
    assert_eq!(seen.player_visible_item_entry(16), Some(2362));
    assert_eq!(
        seen.player_inv_slot(15),
        None,
        "the inventory is the owner's"
    );
}

#[test]
fn a_newcomer_sees_every_task_node_and_learns_its_template() {
    let host = start_server();
    let (mut one, _) = enter(&host, "among1", "secret1");
    let layout = among_server::game::placeholder_layout();
    // A cast step is worked at one node, a gather step at one per spot; Slay steps are fought.
    let expected = (layout.tasks.iter())
        .flat_map(|t| &t.steps)
        .map(|s| match &s.action {
            among_rules::StepAction::Cast(_) => 1,
            among_rules::StepAction::Gather { nodes, .. } => nodes.len(),
            among_rules::StepAction::Slay { .. } => 0,
        })
        .sum::<usize>();
    assert!(expected > 10, "the placeholder gathers");
    let mut nodes = Vec::new();
    while nodes.len() < expected {
        nodes.extend(wait_for(&mut one, |p| {
            let created = created_objects(p);
            (!created.is_empty()).then_some(created)
        }));
    }
    assert_eq!(
        nodes.len(),
        expected,
        "one node per cast step and gather spot"
    );
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

#[test]
fn a_dealt_task_lands_in_the_quest_log_and_its_quest_query_is_answered() {
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
    // The owner's values update fills its log: the crew's quest, then its first task.
    let quests = wait_for(one, |p| {
        let ServerPacket::UpdateObject { objects } = p else {
            return None;
        };
        objects.iter().find_map(|o| match o {
            Object::Values { guid, mask } if guid == me => {
                let ids: Vec<u32> = (0..20)
                    .filter_map(|s| mask.player_quest_log(s))
                    .map(|q| q.quest_id)
                    .filter(|&id| id != 0)
                    .collect();
                (!ids.is_empty()).then_some(ids)
            }
            _ => None,
        })
    });
    assert_eq!(quests[0], among_server::world::quests::CREW_QUEST);
    assert!(quests.len() > 1, "{quests:?}");
    // The client asks the task's template as the log does when a new quest id appears.
    one.quest_query(quests[1]).unwrap();
    let template = wait_for(one, |p| match p {
        ServerPacket::QuestQueryResponse(t) if t.quest_id == quests[1] => Some(t.clone()),
        _ => None,
    });
    let task = among_server::world::quests::quest_task(quests[1]).unwrap();
    let layout = among_server::game::placeholder_layout();
    // An impostor's copy is marked fake; roles are dealt at random, so either may come back.
    let name = &layout.tasks[task.0].name;
    assert!(
        template.title == *name || template.title == format!("{name} (fake)"),
        "{}",
        template.title
    );
    assert!(template.objectives[0].required_count > 0);
}

/// The next system chat line, skipping everything else.
fn wait_system_line(session: &mut WorldSession) -> String {
    wait_for(session, |p| match p {
        ServerPacket::MessageChat(m) if m.chat_type == 0x0A => Some(m.text.clone()),
        _ => None,
    })
}

/// The next chat line of type `kind` `session` is sent, the HUD's addon traffic aside: its
/// sender and text.
fn wait_chat(session: &mut WorldSession, kind: u8) -> (u64, String) {
    wait_for(session, |p| match p {
        ServerPacket::MessageChat(m) if m.chat_type == kind && !m.is_addon() => {
            Some((m.sender_guid, m.text.clone()))
        }
        _ => None,
    })
}

#[test]
fn say_works_party_only_before_the_game_and_whispers_never() {
    let settings = Settings {
        min_players: 3,
        ..Settings::default()
    };
    let host = start_server_with(3, settings);
    let mut players: Vec<(WorldSession, u64)> = (1..=3)
        .map(|i| enter(&host, &format!("among{i}"), &format!("secret{i}")))
        .collect();
    let a_guid = players[0].1;
    players[0].0.send_chat("hello crew").unwrap();
    assert_eq!(
        wait_chat(&mut players[1].0, 0x00),
        (a_guid, "hello crew".into())
    );
    // In the lobby the party is everyone.
    players[0].0.send_chat_kind(0x01, None, "ready?").unwrap();
    assert_eq!(
        wait_chat(&mut players[2].0, 0x01),
        (a_guid, "ready?".into())
    );
    players[0]
        .0
        .send_chat_kind(0x06, Some("among2"), "psst")
        .unwrap();
    let refusal = wait_for(&mut players[0].0, |p| match p {
        ServerPacket::MessageChat(m) if m.chat_type == 0x0A && m.text.contains("whispers") => {
            Some(m.text.clone())
        }
        _ => None,
    });
    assert_eq!(refusal, "There are no whispers in Among Azeroth.");
    // Once the game runs, party chat waits for a meeting.
    let button = among_server::world::guid::Fixture::Button.guid();
    players[0].0.gameobj_use(button).unwrap();
    for (session, _) in &mut players {
        wait_notice(session, |n| matches!(n, Notice::Role { .. }).then_some(()));
    }
    players[1]
        .0
        .send_chat_kind(0x01, None, "it was red")
        .unwrap();
    let refusal = wait_for(&mut players[1].0, |p| match p {
        ServerPacket::MessageChat(m) if m.chat_type == 0x0A && m.text.starts_with("Party") => {
            Some(m.text.clone())
        }
        _ => None,
    });
    assert_eq!(refusal, "Party chat opens in the lobby and at meetings.");
}

#[test]
fn only_the_operator_may_survey_not_a_guest_with_the_code() {
    // An open lobby whose operator also has their own account, as `--account` gives them.
    let marks = temp_marks("guest");
    let accounts = Accounts::open_lobby("K3XQ9P").unwrap();
    accounts.register("among1", "secret1").unwrap();
    let host = start_server_for(accounts, Settings::default(), marks.clone());
    // The guest is in first, so holds the host seat, and still may not survey.
    let (mut guest, _) = enter(&host, "guest", "k3xq9p");
    assert!(wait_system_line(&mut guest).starts_with("You host this game"));
    let (mut first, _) = enter(&host, "among1", "secret1");
    assert_eq!(wait_system_line(&mut first), "Guest hosts this game.");
    guest.send_chat(".mark herb-east").unwrap();
    assert_eq!(
        wait_system_line(&mut guest),
        "Survey commands are for the server's owner."
    );
    assert!(among_server::survey::read_marks(&marks)
        .unwrap_or_default()
        .is_empty());
    first.send_chat(".mark herb-east").unwrap();
    assert!(wait_system_line(&mut first).starts_with("Marked herb-east"));
}

#[test]
fn goto_takes_the_host_back_to_a_mark() {
    let marks = temp_marks("goto");
    let host = start_server_full(1, Settings::default(), marks);
    let (mut me, guid) = enter(&host, "among1", "secret1");
    assert!(wait_system_line(&mut me).starts_with("You host this game"));
    me.start_forward([-8930.5, -120.25, 84.0], 0.5).unwrap();
    me.send_chat(".mark herb-north").unwrap();
    assert!(wait_system_line(&mut me).starts_with("Marked herb-north"));
    me.start_forward([-8960.0, -140.0, 84.0], 0.0).unwrap();
    me.send_chat(".goto herb-north").unwrap();
    let to = wait_for(&mut me, |p| match p {
        ServerPacket::Teleport {
            guid: g, position, ..
        } if *g == guid => Some([position.x, position.y, position.z]),
        _ => None,
    });
    assert_eq!(to, [-8930.5, -120.25, 84.0]);
    assert_eq!(wait_system_line(&mut me), "Went to herb-north.");
    me.send_chat(".goto nowhere").unwrap();
    assert_eq!(wait_system_line(&mut me), "No mark named nowhere.");
}

#[test]
fn typing_mark_in_chat_records_where_you_stand() {
    let marks = temp_marks("survey");
    let host = start_server_full(1, Settings::default(), marks.clone());
    let (mut me, _) = enter(&host, "among1", "secret1");
    // The first in is told in chat that they host.
    assert!(wait_system_line(&mut me).starts_with("You host this game"));
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
    assert_eq!(
        (to, speed),
        (guid, 31.5),
        "3 x the 10.5 yd/s player run speed"
    );
    assert_eq!(wait_system_line(&mut me), "Run speed x3 (31.5 yd/s)");
    me.send_chat(".speed 50").unwrap();
    assert_eq!(
        wait_system_line(&mut me),
        "Usage: .speed <0.5 to 10>, 1 is normal"
    );
}

#[test]
fn a_crewmate_gathers_a_herb_by_casting_at_it_and_it_vanishes_for_everyone() {
    // Both common tasks dealt, so every crewmate has the gathering one.
    let settings = Settings {
        min_players: 3,
        common_tasks: 2,
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
    let roles: Vec<Role> = (players.iter_mut())
        .map(|(s, _)| {
            wait_notice(s, |n| match n {
                Notice::Role { role, .. } => Some(*role),
                _ => None,
            })
        })
        .collect();
    let crew = roles.iter().position(|r| *r == Role::Crewmate).unwrap();
    let layout = among_server::game::placeholder_layout();
    let (task, spot) = (layout.tasks.iter().enumerate())
        .find_map(|(i, t)| match &t.steps[0].action {
            among_rules::StepAction::Gather { nodes, .. } => {
                Some((among_rules::TaskId(i), nodes[3]))
            }
            _ => None,
        })
        .unwrap();
    let herb = among_server::world::guid::node(task, 0, 3);
    let (me, guid) = &mut players[crew];
    let guid = *guid;
    me.start_forward(spot, 0.0).unwrap();
    me.stop(spot, 0.0).unwrap();
    // Right-clicking a herb casts Herb Gathering at it: the bar is shown at that node.
    me.cast_spell_gameobject(among_server::look::HERB_GATHERING, herb)
        .unwrap();
    let target = wait_for(me, |p| match p {
        ServerPacket::SpellStart(s) if s.caster == guid => Some(s.targets.go_target),
        _ => None,
    });
    assert_eq!(target, Some(herb));
    // Five seconds later the herb is picked, then gone from everyone's world. Three players hold
    // the task (the impostor's fake picks take spots too), so each picks two of the seven.
    // The gatherer sees the herb go before its progress line: watch for both at once.
    let (mut progress, mut gone) = (None, false);
    wait_for_secs(me, 8, |p| {
        match p {
            ServerPacket::DestroyObject { guid } if *guid == herb => gone = true,
            ServerPacket::MessageChat(m) => {
                let payload = (m.text.strip_prefix(wire::PREFIX))
                    .and_then(|t| t.strip_prefix('\t'))
                    .and_then(Notice::decode);
                if let Some(Notice::Gather { picked, count, .. }) = payload {
                    progress = Some((picked, count));
                }
            }
            _ => {}
        }
        (gone && progress.is_some()).then_some(())
    });
    assert_eq!(progress, Some((1, 2)));
    for (i, (session, _)) in players.iter_mut().enumerate() {
        if i == crew {
            continue;
        }
        let gone = wait_for(session, |p| match p {
            ServerPacket::DestroyObject { guid } if *guid == herb => Some(*guid),
            _ => None,
        });
        assert_eq!(gone, herb);
    }
}

/// The guid's health in this update, if it carries one.
fn health_in(packet: &ServerPacket, guid: u64) -> Option<u32> {
    let ServerPacket::UpdateObject { objects } = packet else {
        return None;
    };
    objects.iter().find_map(|o| match o {
        Object::Values { guid: g, mask } if *g == guid => mask.unit_health(),
        _ => None,
    })
}

#[test]
fn a_kill_fells_the_victim_who_releases_into_a_ghost_beside_a_reportable_corpse() {
    let settings = Settings {
        min_players: 4,
        initial_kill_cooldown: Duration::ZERO,
        ..Settings::default()
    };
    let host = start_server_with(4, settings);
    let mut players: Vec<(WorldSession, u64)> = Vec::new();
    for i in 1..=4 {
        let (mut session, guid) = enter(&host, &format!("among{i}"), &format!("secret{i}"));
        wait_notice(&mut session, |n| matches!(n, Notice::Host(_)).then_some(()));
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
    let imp = roles.iter().position(|r| *r == Role::Impostor).unwrap();
    let victim = (0..4).find(|&i| i != imp).unwrap();
    let witness = (0..4).find(|&i| i != imp && i != victim).unwrap();
    let victim_guid = players[victim].1;

    // The strike: the victim's health goes to 0 on a bystander's screen, and the victim is
    // rooted where it fell.
    players[imp].0.cast_spell(15692, Some(victim_guid)).unwrap();
    wait_for(&mut players[witness].0, |p| {
        (health_in(p, victim_guid) == Some(0)).then_some(())
    });
    wait_for(&mut players[victim].0, |p| match p {
        ServerPacket::MoveMode {
            guid,
            mode: benilla_protocol::messages::MoveMode::Root,
            apply: true,
            ..
        } if *guid == victim_guid => Some(()),
        _ => None,
    });

    // Release Spirit: the bystander sees a corpse appear and the ghost vanish.
    players[victim].0.repop_request().unwrap();
    let mut corpse = None;
    let mut ghost_gone = false;
    wait_for(&mut players[witness].0, |p| {
        match p {
            ServerPacket::UpdateObject { objects } => {
                for o in objects {
                    if let Object::Create {
                        guid,
                        object_type: ObjectType::Corpse,
                        ..
                    } = o
                    {
                        corpse = Some(*guid);
                    }
                }
            }
            ServerPacket::DestroyObject { guid } if *guid == victim_guid => ghost_gone = true,
            _ => {}
        }
        (corpse.is_some() && ghost_gone).then_some(())
    });
    // The ghost itself is told it is one, and has no corpse to run back to.
    wait_for(&mut players[victim].0, |p| {
        let ServerPacket::UpdateObject { objects } = p else {
            return None;
        };
        objects.iter().find_map(|o| match o {
            Object::Values { guid, mask } if *guid == victim_guid && mask.player_is_ghost() => {
                Some(())
            }
            _ => None,
        })
    });
    players[victim].0.corpse_query().unwrap();
    let found = wait_for(&mut players[victim].0, |p| match p {
        ServerPacket::CorpseQuery(c) => Some(c.found),
        _ => None,
    });
    assert!(!found);

    // The bystander reports the body; the meeting takes the corpse away.
    press(&mut players[witness].0, Request::Report);
    let corpse = corpse.unwrap();
    wait_for(&mut players[witness].0, |p| match p {
        ServerPacket::DestroyObject { guid } if *guid == corpse => Some(()),
        _ => None,
    });
}

/// The cooldowns of `packet` if it starts this client's own, as the client keeps them.
fn own_cooldowns(packet: &ServerPacket, me: u64) -> Option<Vec<(u32, u32)>> {
    match packet {
        ServerPacket::SpellCooldownList { caster, cooldowns } if *caster == me => {
            Some(cooldowns.clone())
        }
        _ => None,
    }
}

#[test]
fn the_impostor_is_sent_its_kill_cooldown_after_a_kill() {
    let settings = Settings {
        min_players: 4,
        initial_kill_cooldown: Duration::ZERO,
        ..Settings::default()
    };
    let host = start_server_with(4, settings);
    let mut players: Vec<(WorldSession, u64)> = Vec::new();
    for i in 1..=4 {
        let (mut session, guid) = enter(&host, &format!("among{i}"), &format!("secret{i}"));
        wait_notice(&mut session, |n| matches!(n, Notice::Host(_)).then_some(()));
        players.push((session, guid));
    }
    let button = among_server::world::guid::Fixture::Button.guid();
    players[0].0.gameobj_use(button).unwrap();
    let roles: Vec<Role> = (players.iter_mut())
        .map(|(s, _)| {
            wait_notice(s, |n| match n {
                Notice::Role { role, .. } => Some(*role),
                _ => None,
            })
        })
        .collect();
    // The impostor's Kill sweeps 25 s after the strike (four players, so the game goes on).
    let imp = roles.iter().position(|r| *r == Role::Impostor).unwrap();
    let victim = players[(imp + 1) % 4].1;
    let me = players[imp].1;
    players[imp].0.cast_spell(15692, Some(victim)).unwrap();
    let cds = wait_for(&mut players[imp].0, |p| {
        own_cooldowns(p, me).filter(|c| c.iter().any(|&(s, _)| s == 15692))
    });
    assert!(cds.contains(&(15692, 25_000)), "{cds:?}");
}
