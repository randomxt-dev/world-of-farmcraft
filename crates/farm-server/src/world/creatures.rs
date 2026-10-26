//! The farm's creatures as the client sees them: villagers, the farm's animals and the farmer's
//! pet. The unit create, the template the creature query asks for, and the monster moves that
//! walk them, as `benilla-protocol` reads them.

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::update::{self, Fields, Living, BASE_SPEEDS};
use super::wire::Body;

// Descriptor indices for build 5875 (vmangos `UpdateFields_1_12_1.h`).
const OBJECT_FIELD_GUID: u16 = 0;
const OBJECT_FIELD_TYPE: u16 = 2;
const OBJECT_FIELD_ENTRY: u16 = 3;
const OBJECT_FIELD_SCALE_X: u16 = 4;
const UNIT_FIELD_SUMMONEDBY: u16 = 12;
const UNIT_FIELD_CREATEDBY: u16 = 16;
const UNIT_FIELD_HEALTH: u16 = 22;
const UNIT_FIELD_MAXHEALTH: u16 = 28;
const UNIT_FIELD_LEVEL: u16 = 34;
const UNIT_FIELD_FACTIONTEMPLATE: u16 = 35;
const UNIT_FIELD_FLAGS: u16 = 46;
const UNIT_FIELD_BOUNDINGRADIUS: u16 = 129;
const UNIT_FIELD_COMBATREACH: u16 = 130;
const UNIT_FIELD_DISPLAYID: u16 = 131;
const UNIT_FIELD_NATIVEDISPLAYID: u16 = 132;
const UNIT_FIELD_BYTES_1: u16 = 138;
const UNIT_MOD_CAST_SPEED: u16 = 145;
const UNIT_NPC_FLAGS: u16 = 147;

/// `TYPEMASK_OBJECT | TYPEMASK_UNIT`.
const TYPE_UNIT: u32 = 0x01 | 0x08;
/// `FactionTemplate.dbc` 35, "Friendly": friendly to every player of either faction.
const FACTION_FRIENDLY: u32 = 35;
/// `UNIT_FLAG_PASSIVE`: never attacks, never attacked.
const UNIT_FLAG_PASSIVE: u32 = 0x200;
/// `SPLINEFLAG_RUNMODE`: move at run speed.
const SPLINE_RUN: u32 = 0x100;

/// `UNIT_NPC_FLAG_GOSSIP`: a right-click opens the gossip menu.
pub const NPC_GOSSIP: u32 = 0x01;
/// `UNIT_NPC_FLAG_VENDOR`: a right-click opens the merchant window.
pub const NPC_VENDOR: u32 = 0x04;

/// `UNIT_STAND_STATE_*`, the low byte of `UNIT_FIELD_BYTES_1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stand {
    Stand = 0,
    Sit = 1,
    Sleep = 3,
    Kneel = 8,
}

/// `CREATURE_TYPE_*`: what the tooltip names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Beast = 1,
    Humanoid = 7,
    Critter = 8,
}

/// A creature's look, name and what a right-click on it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub name: String,
    /// The line under the name: `<General Goods>`.
    pub subname: String,
    /// `CreatureDisplayInfo.dbc` id.
    pub display: u32,
    pub scale: f32,
    pub level: u32,
    pub kind: Kind,
    pub npc_flags: u32,
    pub stand: Stand,
}

impl Look {
    pub fn new(name: &str, display: u32, kind: Kind) -> Self {
        Self {
            name: name.into(),
            subname: String::new(),
            display,
            scale: 1.0,
            level: 5,
            kind,
            npc_flags: 0,
            stand: Stand::Stand,
        }
    }
}

/// A creature as it stands now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub guid: u64,
    pub entry: u32,
    pub position: [f32; 3],
    pub orientation: f32,
    /// The player it follows, shown as its summoner: the tooltip names it "<Owner>'s pet".
    pub owner: Option<u64>,
}

/// The `SMSG_UPDATE_OBJECT` that creates the creature.
pub fn create(c: &Shown, look: &Look) -> Packet {
    let mut f = Fields::default();
    f.u64(OBJECT_FIELD_GUID, c.guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_UNIT)
        .u32(OBJECT_FIELD_ENTRY, c.entry)
        .f32(OBJECT_FIELD_SCALE_X, look.scale)
        .u32(UNIT_FIELD_HEALTH, 100)
        .u32(UNIT_FIELD_MAXHEALTH, 100)
        .u32(UNIT_FIELD_LEVEL, look.level)
        .u32(UNIT_FIELD_FACTIONTEMPLATE, FACTION_FRIENDLY)
        .u32(UNIT_FIELD_FLAGS, UNIT_FLAG_PASSIVE)
        .f32(UNIT_FIELD_BOUNDINGRADIUS, 0.5 * look.scale)
        .f32(UNIT_FIELD_COMBATREACH, 1.5)
        .u32(UNIT_FIELD_DISPLAYID, look.display)
        .u32(UNIT_FIELD_NATIVEDISPLAYID, look.display)
        .u32(UNIT_FIELD_BYTES_1, look.stand as u32)
        .f32(UNIT_MOD_CAST_SPEED, 1.0)
        .u32(UNIT_NPC_FLAGS, look.npc_flags);
    if let Some(owner) = c.owner {
        f.u64(UNIT_FIELD_SUMMONEDBY, owner)
            .u64(UNIT_FIELD_CREATEDBY, owner);
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

/// The values update that sets `guid`'s stand state: sitting, sleeping, kneeling.
pub fn stand(guid: u64, stand: Stand) -> Packet {
    let mut f = Fields::default();
    f.u32(UNIT_FIELD_BYTES_1, stand as u32);
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(guid, &f))
}

/// `SMSG_CREATURE_QUERY_RESPONSE`: name, three empty names, the subname, then type flags, the
/// creature type, no pet family, normal rank, two unused words, the display, and two zero flags;
/// or the lone `entry | 0x8000_0000` for a miss.
pub fn query_response(entry: u32, look: Option<&Look>) -> Packet {
    let mut b = Body::default();
    match look {
        None => {
            b.u32(entry | 0x8000_0000);
        }
        Some(look) => {
            b.u32(entry).cstring(&look.name);
            for _ in 0..3 {
                b.cstring("");
            }
            b.cstring(&look.subname)
                .u32(0) // type flags
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

/// `SMSG_MONSTER_MOVE`: walk (or run) from `from` to `to` over `duration_ms`, a one-point spline.
pub fn move_to(guid: u64, from: [f32; 3], to: [f32; 3], duration_ms: u32, run: bool) -> Packet {
    let mut b = Body::default();
    b.packed_guid(guid)
        .f32(from[0])
        .f32(from[1])
        .f32(from[2])
        .u32(0) // spline id
        .u8(0) // no final facing
        .u32(if run { SPLINE_RUN } else { 0 })
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

    #[test]
    fn a_creature_is_a_friendly_unit_wearing_its_look() {
        let mut look = Look::new("Brog Hamfist", 1292, Kind::Humanoid);
        look.npc_flags = NPC_VENDOR;
        let shown = Shown {
            guid: 0xF130_0000_0000_0001,
            entry: 7,
            position: [-9465.0, 9.6, 57.1],
            orientation: 1.4,
            owner: None,
        };
        let ServerPacket::UpdateObject { objects } = parse(&create(&shown, &look)) else {
            panic!("not an update");
        };
        let [Object::Create {
            guid,
            object_type,
            mask,
            ..
        }] = objects.as_slice()
        else {
            panic!("one create");
        };
        assert_eq!(*guid, shown.guid);
        assert_eq!(*object_type, ObjectType::Unit);
        assert_eq!(mask.unit_faction_template(), Some(35));
        assert_eq!(mask.unit_displayid(), Some(1292));
        assert!(!mask.unit_reads_dead());

        let ServerPacket::CreatureQueryResponse { info, .. } =
            parse(&query_response(7, Some(&look)))
        else {
            panic!("not a template");
        };
        assert!(info.is_some());
        assert!(matches!(
            parse(&move_to(shown.guid, [0.0; 3], [1.0, 0.0, 0.0], 400, false)),
            ServerPacket::MonsterMove { .. }
        ));
    }
}
