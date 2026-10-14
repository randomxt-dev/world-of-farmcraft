//! Server packet builders. Each layout is the one `benilla-protocol`'s `parse_server` reads, and
//! the tests parse every builder's bytes back through it.

use benilla_protocol::messages::{opcode, AUTH_OK};

use super::socket::Packet;
use super::wire::Body;
use crate::accounts::CharacterRecord;

/// Where a character stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub map: u32,
    pub zone: u32,
    pub position: [f32; 3],
    pub orientation: f32,
}

/// `SMSG_AUTH_CHALLENGE`: the server seed, sent before header encryption starts.
pub fn auth_challenge(server_seed: u32) -> Packet {
    Packet::new(
        opcode::SMSG_AUTH_CHALLENGE,
        Body::default().u32(server_seed).done(),
    )
}

/// `SMSG_AUTH_RESPONSE` admitting the session: `AUTH_OK`, then the billing group (no time left,
/// no plan flags, no rested minutes).
pub fn auth_ok() -> Packet {
    Packet::new(
        opcode::SMSG_AUTH_RESPONSE,
        Body::default().u8(AUTH_OK).u32(0).u8(0).u32(0).done(),
    )
}

/// `SMSG_AUTH_RESPONSE` refusing the session with `code`.
pub fn auth_refused(code: u8) -> Packet {
    Packet::new(opcode::SMSG_AUTH_RESPONSE, vec![code])
}

/// `SMSG_CHAR_ENUM`: the roster, each character standing at `location`, wearing nothing.
pub fn char_enum(characters: &[CharacterRecord], location: Location) -> Packet {
    let mut b = Body::default();
    b.u8(characters.len() as u8);
    for c in characters {
        b.u64(c.guid)
            .cstring(&c.name)
            .bytes(&[
                c.race,
                c.class,
                c.gender,
                c.skin,
                c.face,
                c.hair_style,
                c.hair_color,
                c.facial_hair,
                c.level,
            ])
            .u32(location.zone)
            .u32(location.map)
            .f32(location.position[0])
            .f32(location.position[1])
            .f32(location.position[2])
            .u32(0) // guild
            .u32(0) // character flags
            .u8(0) // first login
            .u32(0) // pet display
            .u32(0) // pet level
            .u32(0); // pet family
        for _ in 0..19 {
            b.u32(0).u8(0); // equipment: display id, inventory type
        }
        b.u32(0).u8(0); // first bag
    }
    Packet::new(opcode::SMSG_CHAR_ENUM, b.done())
}

/// `SMSG_CHAR_CREATE`: one result byte.
pub fn char_create_result(code: u8) -> Packet {
    Packet::new(opcode::SMSG_CHAR_CREATE, vec![code])
}

/// `SMSG_CHAR_DELETE`: one result byte.
pub fn char_delete_result(code: u8) -> Packet {
    Packet::new(opcode::SMSG_CHAR_DELETE, vec![code])
}

/// `SMSG_LOGIN_VERIFY_WORLD`: the map and pose the client loads into.
pub fn login_verify_world(at: Location) -> Packet {
    Packet::new(
        opcode::SMSG_LOGIN_VERIFY_WORLD,
        Body::default()
            .u32(at.map)
            .f32(at.position[0])
            .f32(at.position[1])
            .f32(at.position[2])
            .f32(at.orientation)
            .done(),
    )
}

/// `SMSG_LOGIN_SETTIMESPEED`: the game clock, held at noon so every round plays in daylight. The
/// date packs minute:6, hour:5, weekday:3, day-1:6, month-1:4, year-2000:5; the speed is the
/// stock one game minute per real minute.
pub fn login_set_time_speed() -> Packet {
    let (minute, hour, weekday, day, month, year) = (0u32, 12u32, 3u32, 14u32, 10u32, 2026u32);
    let packed = minute
        | hour << 6
        | weekday << 11
        | (day - 1) << 14
        | (month - 1) << 20
        | (year - 2000) << 24;
    Packet::new(
        opcode::SMSG_LOGIN_SETTIMESPEED,
        Body::default().u32(packed).f32(1.0 / 60.0).done(),
    )
}

/// `SMSG_QUERY_TIME_RESPONSE`: wall-clock unix seconds.
pub fn query_time_response(unix_seconds: u32) -> Packet {
    Packet::new(
        opcode::SMSG_QUERY_TIME_RESPONSE,
        Body::default().u32(unix_seconds).done(),
    )
}

/// `SMSG_PONG`: the ping's sequence echoed.
pub fn pong(sequence: u32) -> Packet {
    Packet::new(opcode::SMSG_PONG, Body::default().u32(sequence).done())
}

/// `SMSG_NAME_QUERY_RESPONSE`: guid, name, an empty realm name, then race, gender and class.
pub fn name_query_response(c: &CharacterRecord) -> Packet {
    Packet::new(
        opcode::SMSG_NAME_QUERY_RESPONSE,
        Body::default()
            .u64(c.guid)
            .cstring(&c.name)
            .cstring("")
            .u32(c.race.into())
            .u32(c.gender.into())
            .u32(c.class.into())
            .done(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};

    fn parse(p: &Packet) -> ServerPacket {
        let (packet, tail) =
            parse_server_with_tail(p.opcode, &p.body).expect("the client parses it");
        assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
        packet
    }

    fn character() -> CharacterRecord {
        CharacterRecord {
            guid: 7,
            name: "Among1".into(),
            race: 1,
            class: 1,
            gender: 1,
            skin: 2,
            face: 3,
            hair_style: 4,
            hair_color: 5,
            facial_hair: 6,
            level: 1,
        }
    }

    #[test]
    fn every_packet_reads_back_whole_in_the_client() {
        let at = crate::world::player::SPAWN;
        assert!(matches!(
            parse(&auth_challenge(5)),
            ServerPacket::AuthChallenge { server_seed: 5 }
        ));
        assert!(matches!(
            parse(&auth_ok()),
            ServerPacket::AuthResponse {
                result: AUTH_OK,
                ..
            }
        ));
        let ServerPacket::CharEnum { characters } = parse(&char_enum(&[character()], at)) else {
            panic!("not a roster");
        };
        assert_eq!(characters[0].guid, 7);
        assert_eq!(characters[0].name, "Among1");
        assert_eq!(characters[0].map, at.map);
        assert_eq!(
            (characters[0].hair_style, characters[0].facial_hair),
            (4, 6)
        );
        let ServerPacket::LoginVerifyWorld { map, position, .. } = parse(&login_verify_world(at))
        else {
            panic!("not a world verify");
        };
        assert_eq!((map, position.x), (at.map, at.position[0]));
        let ServerPacket::TimeSpeed { hours, minutes, .. } = parse(&login_set_time_speed()) else {
            panic!("not a time speed");
        };
        assert_eq!((hours, minutes), (12, 0));
        assert!(matches!(
            parse(&query_time_response(9)),
            ServerPacket::QueryTimeResponse { unix_time: 9 }
        ));
        assert!(matches!(
            parse(&pong(3)),
            ServerPacket::Pong { sequence: 3 }
        ));
        let ServerPacket::NameQueryResponse {
            guid,
            name,
            race,
            gender,
            class,
        } = parse(&name_query_response(&character()))
        else {
            panic!("not a name response");
        };
        assert_eq!(
            (guid, name.as_str(), race, gender, class),
            (7, "Among1", 1, 1, 1)
        );
    }

    /// The self create decodes into a placed, living, displayed player with base speeds.
    #[test]
    fn the_self_create_is_a_living_player_the_client_can_draw() {
        use benilla_protocol::{decode, EntityKind, SessionEvent};
        let p = crate::world::player::create_packet(
            &character(),
            &crate::world::player::spawn_living(),
            true,
        );
        let events = decode(parse(&p));
        let [SessionEvent::ObjectCreate {
            guid,
            kind,
            display_id,
            position,
            speeds,
            fields,
            ..
        }] = events.as_slice()
        else {
            panic!("one object create, got {events:?}");
        };
        assert_eq!(*guid, 7);
        assert_eq!(*kind, EntityKind::Player);
        assert_eq!(*display_id, Some(50), "a human female");
        assert_eq!(*position, crate::world::player::SPAWN.position);
        assert!(speeds.is_some());
        assert_eq!(fields.unit_health(), Some(100), "alive");
        assert_eq!(fields.unit_race(), Some(1), "race in bytes 0");
    }
}
