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
/// Three weapon display ids: main hand, off hand, ranged.
const UNIT_VIRTUAL_ITEM_SLOT_DISPLAY: u16 = 37;
/// Two words per weapon slot: class, subclass, material and inventory type bytes, then sheath.
const UNIT_VIRTUAL_ITEM_INFO: u16 = 40;
const UNIT_FIELD_BASEATTACKTIME: u16 = 126;
const UNIT_FIELD_BOUNDINGRADIUS: u16 = 129;
const UNIT_FIELD_COMBATREACH: u16 = 130;
const UNIT_FIELD_DISPLAYID: u16 = 131;
const UNIT_FIELD_NATIVEDISPLAYID: u16 = 132;
const UNIT_MOD_CAST_SPEED: u16 = 145;
/// Byte 0 is the sheath state.
const UNIT_FIELD_BYTES_2: u16 = 164;

/// `TYPEMASK_OBJECT | TYPEMASK_UNIT`.
const TYPE_UNIT: u32 = 0x01 | 0x08;
/// `FactionTemplate.dbc` 14, "Monster": hostile to every player.
const FACTION_MONSTER: u32 = 14;
/// `SHEATH_STATE_MELEE`: vmangos `Creature::UpdateEntry` readies every creature's melee weapon.
const SHEATH_MELEE: u32 = 1;
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
        .f32(UNIT_MOD_CAST_SPEED, 1.0)
        .u32(UNIT_FIELD_BYTES_2, SHEATH_MELEE);
    if let Some(w) = look.weapon {
        // Main hand, packed as vmangos `Creature::SetVirtualItem` writes it.
        let info = u32::from_le_bytes([w.class, w.subclass, w.material, w.inventory_type]);
        f.u32(UNIT_VIRTUAL_ITEM_SLOT_DISPLAY, w.display)
            .u32(UNIT_VIRTUAL_ITEM_INFO, info)
            .u32(UNIT_VIRTUAL_ITEM_INFO + 1, u32::from(w.sheath));
    }
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
/// the creature type, no pet family, normal rank, two unused words, the display, and two zero
/// flags; or the lone `entry | 0x8000_0000` for a miss.
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
                .u32(look.kind as u32)
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

    fn created(look: &CreatureLook) -> benilla_protocol::messages::ObjectFields {
        let ServerPacket::UpdateObject { objects } = parse(&create(&wolf(40), look)) else {
            panic!("not an update");
        };
        let [Object::Create { mask, .. }] = objects.as_slice() else {
            panic!("one create");
        };
        mask.clone()
    }

    #[test]
    fn a_creature_wears_its_display_size_level_and_weapon() {
        let mask = created(&CreatureLook::wolf());
        assert_eq!(mask.object_scale_x(), Some(0.4));
        assert_eq!(mask.unit_level(), Some(1));
        assert_eq!(
            (mask.unit_displayid(), mask.unit_native_displayid()),
            (Some(447), Some(447))
        );
        assert_eq!(mask.unit_virtual_item_display(0), Some(0), "bare fangs");
        assert_eq!(mask.unit_sheath_state(), Some(1));

        let mask = created(&CreatureLook::kobold());
        assert_eq!(mask.object_scale_x(), Some(0.7));
        assert_eq!(mask.unit_displayid(), Some(10913));
        assert_eq!(mask.unit_virtual_item_display(0), Some(5010));
        assert_eq!(mask.unit_virtual_item_info(0), Some((2, 10, 2, 17)));
        assert_eq!(mask.unit_virtual_item_sheath(0), Some(2));

        let mask = created(&CreatureLook::defias());
        assert_eq!(mask.object_scale_x(), Some(1.0));
        assert_eq!(mask.unit_level(), Some(3));
        assert_eq!(mask.unit_displayid(), Some(5035));
        assert_eq!(mask.unit_virtual_item_display(0), Some(1593));
        assert_eq!(mask.unit_virtual_item_info(0), Some((2, 7, 1, 13)));
        assert_eq!(mask.unit_virtual_item_sheath(0), Some(3));
        assert_eq!(mask.unit_virtual_item_display(1), Some(0), "no off hand");
    }

    #[test]
    fn the_template_names_the_creature_type() {
        let ServerPacket::CreatureQueryResponse { info, .. } = parse(&query_response(
            6,
            Some(("Kobold Vermin", &CreatureLook::kobold())),
        )) else {
            panic!("not a template");
        };
        let info = info.expect("found");
        assert_eq!((info.creature_type, info.display_id), (7, 10913));
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
