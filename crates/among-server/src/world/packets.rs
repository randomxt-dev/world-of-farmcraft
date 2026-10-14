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
