//! The server against `benilla-protocol`'s own client, over real sockets: logon, roster, world
//! entry, and one player's movement reaching the other. No game data needed.

use std::time::{Duration, Instant};

use among_server::{Accounts, Server, ServerConfig};
use benilla_protocol::messages::{opcode, Object, ServerPacket};
use benilla_protocol::WorldSession;

/// Start a server on free ports with two accounts; returns realmd's `host:port`.
fn start_server() -> String {
    let accounts = Accounts::default();
    accounts.register("among1", "secret1").unwrap();
    accounts.register("among2", "secret2").unwrap();
    let config = ServerConfig {
        auth_port: 0,
        world_port: 0,
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
        Object::Create { guid, .. } => Some(*guid),
        _ => None,
    })
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
