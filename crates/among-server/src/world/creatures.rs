//! The match's monsters as the client sees them: the unit create, the template the creature query
//! asks for, and the monster moves that walk them, as `benilla-protocol` reads them.

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::update::{self, Fields, Living, BASE_SPEEDS};
use super::wire::Body;
use crate::look::CreatureLook;

// Descriptor indices for build 5875 (vmangos `UpdateFields_1_12_1.h`).
const OBJECT_FIELD_GUID: u16 = 0;
const OBJECT_FIELD_TYPE: u16 = 2;
const OBJECT_FIELD_ENTRY: u16 = 3;
const OBJECT_FIELD_SCALE_X: u16 = 4;
pub(super) const UNIT_FIELD_HEALTH: u16 = 22;
pub(super) const UNIT_FIELD_MAXHEALTH: u16 = 28;
const UNIT_FIELD_LEVEL: u16 = 34;
const UNIT_FIELD_FACTIONTEMPLATE: u16 = 35;
const UNIT_FIELD_BASEATTACKTIME: u16 = 126;
const UNIT_FIELD_BOUNDINGRADIUS: u16 = 129;
const UNIT_FIELD_COMBATREACH: u16 = 130;
const UNIT_FIELD_DISPLAYID: u16 = 131;
const UNIT_FIELD_NATIVEDISPLAYID: u16 = 132;
const UNIT_MOD_CAST_SPEED: u16 = 145;

/// `TYPEMASK_OBJECT | TYPEMASK_UNIT`.
const TYPE_UNIT: u32 = 0x01 | 0x08;
/// `FactionTemplate.dbc` 14, "Monster": hostile to every player.
const FACTION_MONSTER: u32 = 14;
/// `CREATURE_TYPE_BEAST`.
const TYPE_BEAST: u32 = 1;
/// `SPLINEFLAG_RUNMODE`: chase at run speed.
const SPLINE_RUN: u32 = 0x100;

/// A creature as it stands now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub guid: u64,
    pub entry: u32,
    pub position: [f32; 3],
    pub orientation: f32,
    pub health: u32,
    pub max_health: u32,
    pub swing_ms: u32,
}

/// The `SMSG_UPDATE_OBJECT` that creates the creature, alive or (at zero health) dead.
pub fn create(c: &Shown, look: &CreatureLook) -> Packet {
    let mut f = Fields::default();
    f.u64(OBJECT_FIELD_GUID, c.guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_UNIT)
        .u32(OBJECT_FIELD_ENTRY, c.entry)
        .f32(OBJECT_FIELD_SCALE_X, look.scale)
        .u32(UNIT_FIELD_HEALTH, c.health)
        .u32(UNIT_FIELD_MAXHEALTH, c.max_health)
        .u32(UNIT_FIELD_LEVEL, look.level)
        .u32(UNIT_FIELD_FACTIONTEMPLATE, FACTION_MONSTER)
        .u32(UNIT_FIELD_BASEATTACKTIME, c.swing_ms)
        .u32(UNIT_FIELD_BASEATTACKTIME + 1, c.swing_ms)
        .f32(UNIT_FIELD_BOUNDINGRADIUS, 0.5 * look.scale)
        .f32(UNIT_FIELD_COMBATREACH, 1.5)
        .u32(UNIT_FIELD_DISPLAYID, look.display)
        .u32(UNIT_FIELD_NATIVEDISPLAYID, look.display)
        .f32(UNIT_MOD_CAST_SPEED, 1.0);
    let living = Living {
        movement_flags: 0,
        time: 0,
        position: c.position,
        orientation: c.orientation,
        speeds: BASE_SPEEDS,
    };
    Packet::new(
        opcode::SMSG_UPDATE_OBJECT,
        update::create_creature(c.guid, &living, &f),
    )
}

/// `SMSG_CREATURE_QUERY_RESPONSE`: `entry`'s name, three empty names, no subname, then type flags,
/// beast, no pet family, normal rank, two unused words, the display, and two zero flags; or the
/// lone `entry | 0x8000_0000` for a miss.
pub fn query_response(entry: u32, found: Option<(&str, &CreatureLook)>) -> Packet {
    let mut b = Body::default();
    match found {
        None => {
            b.u32(entry | 0x8000_0000);
        }
        Some((name, look)) => {
            b.u32(entry).cstring(name);
            for _ in 0..4 {
                b.cstring("");
            }
            b.u32(0) // type flags
                .u32(TYPE_BEAST)
                .u32(0) // pet family
                .u32(0) // rank: normal
                .u32(0)
                .u32(0) // pet spell list
                .u32(look.display)
                .u8(0) // civilian
                .u8(0); // racial leader
        }
    }
    Packet::new(opcode::SMSG_CREATURE_QUERY_RESPONSE, b.done())
}

/// `SMSG_MONSTER_MOVE`: run from `from` to `to` over `duration_ms`, a one-point ground spline.
pub fn move_to(guid: u64, from: [f32; 3], to: [f32; 3], duration_ms: u32) -> Packet {
    let mut b = Body::default();
    b.packed_guid(guid)
        .f32(from[0])
        .f32(from[1])
        .f32(from[2])
        .u32(0) // spline id
        .u8(0) // no final facing
        .u32(SPLINE_RUN)
        .u32(duration_ms)
        .u32(1)
        .f32(to[0])
        .f32(to[1])
        .f32(to[2]);
    Packet::new(opcode::SMSG_MONSTER_MOVE, b.done())
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, Object, ObjectType, ServerPacket};

    fn parse(p: &Packet) -> ServerPacket {
        let (packet, tail) =
            parse_server_with_tail(p.opcode, &p.body).expect("the client parses it");
        assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
        packet
    }

    fn wolf(health: u32) -> Shown {
        Shown {
            guid: super::super::guid::creature(among_rules::CreatureId(0)),
            entry: 1,
            position: [-8975.0, -112.0, 83.5],
            orientation: 0.5,
            health,
            max_health: 40,
            swing_ms: 2000,
        }
    }

    #[test]
    fn a_creature_is_a_hostile_living_unit_that_reads_dead_at_zero_health() {
        let ServerPacket::UpdateObject { objects } =
            parse(&create(&wolf(40), &CreatureLook::wolf()))
        else {
            panic!("not an update");
        };
        let [Object::Create {
            guid,
            object_type,
            movement,
            mask,
        }] = objects.as_slice()
        else {
            panic!("one create");
        };
        assert_eq!(*guid, wolf(40).guid);
        assert_eq!(*object_type, ObjectType::Unit);
        assert!(movement.speeds.is_some(), "a living block");
        assert_eq!(mask.unit_health(), Some(40));
        assert_eq!(mask.unit_faction_template(), Some(14));
        assert!(!mask.unit_reads_dead());

        let ServerPacket::UpdateObject { objects } =
            parse(&create(&wolf(0), &CreatureLook::wolf()))
        else {
            panic!("not an update");
        };
        let [Object::Create { mask, .. }] = objects.as_slice() else {
            panic!("one create");
        };
        assert!(mask.unit_reads_dead(), "a corpse");
    }

    #[test]
    fn the_template_and_the_moves_read_back() {
        let ServerPacket::CreatureQueryResponse { entry, info } = parse(&query_response(
            1,
            Some(("Young Wolf", &CreatureLook::wolf())),
        )) else {
            panic!("not a template");
        };
        let info = info.expect("found");
        assert_eq!((entry, info.name.as_str()), (1, "Young Wolf"));
        assert_eq!((info.creature_type, info.display_id), (1, 447));
        let ServerPacket::CreatureQueryResponse { info, .. } = parse(&query_response(9, None))
        else {
            panic!("not a template");
        };
        assert!(info.is_none());

        let from = [1.0, 2.0, 3.0];
        let to = [4.0, 6.0, 3.0];
        let ServerPacket::MonsterMove {
            guid,
            path,
            stop,
            duration_ms,
            run_mode,
            ..
        } = parse(&move_to(0xF130_0000_0100_0001, from, to, 833))
        else {
            panic!("not a move");
        };
        assert_eq!(
            (guid, stop, duration_ms, run_mode),
            (0xF130_0000_0100_0001, false, 833, true)
        );
        let path: Vec<[f32; 3]> = path.iter().map(|v| [v.x, v.y, v.z]).collect();
        assert_eq!(path, [from, to]);
    }
}
