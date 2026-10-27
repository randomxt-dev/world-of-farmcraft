//! The farmer as the world shows them: the descriptor they are created with, what they wear and
//! carry, their purse, the packets that seat them at login, and the movement their client sends.

use benilla_protocol::messages::opcode;
use farm_rules::save::Character;

use super::items::{self, Held};
use super::packets::{self, Location};
use super::socket::Packet;
use super::spells;
use super::update::{self, Fields, Living, BASE_SPEEDS};

// Descriptor indices for build 5875 (vmangos `UpdateFields_1_12_1.h`).
const OBJECT_FIELD_GUID: u16 = 0;
const OBJECT_FIELD_TYPE: u16 = 2;
const OBJECT_FIELD_SCALE_X: u16 = 4;
const UNIT_FIELD_HEALTH: u16 = 22;
const UNIT_FIELD_POWER1: u16 = 23;
const UNIT_FIELD_MAXHEALTH: u16 = 28;
const UNIT_FIELD_MAXPOWER1: u16 = 29;
const UNIT_FIELD_LEVEL: u16 = 34;
const UNIT_FIELD_FACTIONTEMPLATE: u16 = 35;
const UNIT_FIELD_BYTES_0: u16 = 36;
const UNIT_FIELD_FLAGS: u16 = 46;
const UNIT_FIELD_BASEATTACKTIME: u16 = 126;
const UNIT_FIELD_BOUNDINGRADIUS: u16 = 129;
const UNIT_FIELD_COMBATREACH: u16 = 130;
const UNIT_FIELD_DISPLAYID: u16 = 131;
const UNIT_FIELD_NATIVEDISPLAYID: u16 = 132;
const UNIT_FIELD_MOUNTDISPLAYID: u16 = 133;
const UNIT_FIELD_BYTES_1: u16 = 138;
const UNIT_MOD_CAST_SPEED: u16 = 145;
/// Byte 0 is the sheath state: 0 sheathed, 1 the melee weapon in hand.
const UNIT_FIELD_BYTES_2: u16 = 164;
const PLAYER_BYTES: u16 = 193;
const PLAYER_BYTES_2: u16 = 194;
const PLAYER_BYTES_3: u16 = 195;
/// `PLAYER_VISIBLE_ITEM_1_0`, the worn entry everyone sees; each slot is 12 words.
const PLAYER_VISIBLE_ITEM_1_0: u16 = 260;
const VISIBLE_ITEM_STRIDE: u16 = 12;
/// `PLAYER_FIELD_INV_SLOT_HEAD`, then two words (an item guid) per inventory slot: equipment
/// 0-18, bags 19-22, the backpack 23-38.
const PLAYER_FIELD_INV_SLOT_HEAD: u16 = 486;
const PLAYER_NEXT_LEVEL_XP: u16 = 717;
/// `PLAYER_EXPLORED_ZONES_1`: 64 words, bit n set for `AreaTable.dbc` exploreFlag n.
const PLAYER_EXPLORED_ZONES_1: u16 = 1111;
const EXPLORED_ZONE_WORDS: u16 = 64;
const PLAYER_FIELD_COINAGE: u16 = 1176;
/// `PLAYER_QUEST_LOG_1_1`: 20 slots of (quest id, counters and state, timer).
const PLAYER_QUEST_LOG_1_1: u16 = 198;
const QUEST_LOG_SLOTS: u16 = 20;
/// `PLAYER_SKILL_INFO_1_1`: 128 skills of three words, the skill id with its step, then the value
/// with its maximum, then a bonus.
const PLAYER_SKILL_INFO_1_1: u16 = 718;
/// `SkillLine.dbc` Herbalism and Mining, known at the top of the trade: the client lights the
/// herb and pick cursors, and casts its gathering spells, only for a skill it can see.
const SKILLS: [u32; 2] = [182, 186];
const SKILL_VALUE: u32 = 300;

/// `TYPEMASK_OBJECT | TYPEMASK_UNIT | TYPEMASK_PLAYER`.
const TYPE_PLAYER: u32 = 0x01 | 0x08 | 0x10;
/// `REST_STATE_NORMAL` in `PLAYER_BYTES_2`'s top byte.
const REST_STATE_NORMAL: u32 = 0x02;

/// The race's `CreatureDisplayInfo` id for `gender` (`ChrRaces.dbc` male/female display).
pub fn display_id(race: u8, gender: u8) -> u32 {
    let (male, female) = match race {
        2 => (51, 52),     // orc
        3 => (53, 54),     // dwarf
        4 => (55, 56),     // night elf
        5 => (57, 58),     // undead
        6 => (59, 60),     // tauren
        7 => (1563, 1564), // gnome
        8 => (1478, 1479), // troll
        _ => (49, 50),     // human
    };
    if gender == 0 {
        male
    } else {
        female
    }
}

/// `FactionTemplate.dbc` row a race's players carry (vmangos `ChrRaces.dbc` faction).
fn faction_template(race: u8) -> u32 {
    match race {
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 5,
        6 => 6,
        7 => 115,
        8 => 116,
        _ => 1,
    }
}

/// The power a class uses and its pool at level 1: mana 0, rage 1, energy 3.
fn power(class: u8) -> (u8, u32) {
    match class {
        1 => (1, 1000),
        4 => (3, 100),
        _ => (0, 100),
    }
}

/// The farmer's worn outfit, with the guids it is sent under.
pub fn outfit(owner: u64) -> Vec<Held> {
    (items::OUTFIT.iter())
        .map(|&(slot, entry)| Held {
            slot,
            guid: items::item_guid(owner, u32::from(slot)),
            entry,
            count: 1,
        })
        .collect()
}

/// `EQUIPMENT_SLOT_MAINHAND`.
const MAINHAND: u8 = 15;
/// The tool not in hand: carried, in no slot, so the hand can swap to it.
const SPARE: u8 = 0xFE;

/// The spare tool: the axe, while the shovel starts in hand.
pub fn spare_tool(owner: u64) -> Held {
    Held {
        slot: SPARE,
        guid: items::item_guid(owner, u32::from(SPARE)),
        entry: items::LUMBERJACK_AXE,
        count: 1,
    }
}

/// The item guid of the tool `entry` (the shovel or the axe).
pub fn tool_guid(owner: u64, entry: u32) -> u64 {
    let slot = if entry == items::LUMBERJACK_AXE {
        SPARE
    } else {
        MAINHAND
    };
    items::item_guid(owner, u32::from(slot))
}

/// The values update that puts tool `entry` in the farmer's hand, for them and everyone.
pub fn wield_change(owner: u64, entry: u32) -> Packet {
    let mut f = Fields::default();
    f.u32(
        PLAYER_VISIBLE_ITEM_1_0 + VISIBLE_ITEM_STRIDE * u16::from(MAINHAND),
        entry,
    )
    .u64(
        PLAYER_FIELD_INV_SLOT_HEAD + 2 * u16::from(MAINHAND),
        tool_guid(owner, entry),
    );
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(owner, &f))
}

/// The Mining Pick, always in the backpack's first slot.
pub fn pick(owner: u64) -> Held {
    Held {
        slot: items::BACKPACK,
        guid: items::item_guid(owner, u32::from(items::BACKPACK)),
        entry: items::MINING_PICK,
        count: 1,
    }
}

/// The player's create-time descriptor; `is_self` adds what only the owner is sent: the bags,
/// the purse, the explored map.
pub fn fields(c: &Character, is_self: bool, carried: &[Held], money: u32) -> Fields {
    let mut f = Fields::default();
    let (power, pool) = power(c.class);
    f.u64(OBJECT_FIELD_GUID, c.guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_PLAYER)
        .f32(OBJECT_FIELD_SCALE_X, 1.0)
        .u32(UNIT_FIELD_HEALTH, 100)
        .u32(UNIT_FIELD_MAXHEALTH, 100)
        .u32(UNIT_FIELD_POWER1 + u16::from(power), pool)
        .u32(UNIT_FIELD_MAXPOWER1 + u16::from(power), pool)
        .u32(UNIT_FIELD_LEVEL, 1)
        .u32(UNIT_FIELD_FACTIONTEMPLATE, faction_template(c.race))
        .u32(
            UNIT_FIELD_BYTES_0,
            u32::from(c.race)
                | u32::from(c.class) << 8
                | u32::from(c.gender) << 16
                | u32::from(power) << 24,
        )
        .u32(UNIT_FIELD_FLAGS, 0)
        .u32(UNIT_FIELD_BASEATTACKTIME, 2000)
        .u32(UNIT_FIELD_BASEATTACKTIME + 1, 2000)
        .f32(UNIT_FIELD_BOUNDINGRADIUS, 0.389)
        .f32(UNIT_FIELD_COMBATREACH, 1.5)
        .u32(UNIT_FIELD_DISPLAYID, display_id(c.race, c.gender))
        .u32(UNIT_FIELD_NATIVEDISPLAYID, display_id(c.race, c.gender))
        .f32(UNIT_MOD_CAST_SPEED, 1.0)
        .u32(
            PLAYER_BYTES,
            u32::from(c.skin)
                | u32::from(c.face) << 8
                | u32::from(c.hair_style) << 16
                | u32::from(c.hair_color) << 24,
        )
        .u32(
            PLAYER_BYTES_2,
            u32::from(c.facial_hair) | REST_STATE_NORMAL << 24,
        )
        .u32(PLAYER_BYTES_3, c.gender.into())
        .u32(PLAYER_NEXT_LEVEL_XP, 400);
    for w in outfit(c.guid) {
        let slot = u16::from(w.slot);
        f.u32(
            PLAYER_VISIBLE_ITEM_1_0 + VISIBLE_ITEM_STRIDE * slot,
            w.entry,
        );
        if is_self {
            f.u64(PLAYER_FIELD_INV_SLOT_HEAD + 2 * slot, w.guid);
        }
    }
    if is_self {
        for i in 0..EXPLORED_ZONE_WORDS {
            f.u32(PLAYER_EXPLORED_ZONES_1 + i, u32::MAX);
        }
        for h in carried.iter().filter(|h| h.slot < items::BACKPACK_END) {
            f.u64(PLAYER_FIELD_INV_SLOT_HEAD + 2 * u16::from(h.slot), h.guid);
        }
        f.u32(PLAYER_FIELD_COINAGE, money);
        for (i, skill) in SKILLS.into_iter().enumerate() {
            let at = PLAYER_SKILL_INFO_1_1 + 3 * i as u16;
            f.u32(at, skill)
                .u32(at + 1, SKILL_VALUE | SKILL_VALUE << 16)
                .u32(at + 2, 0);
        }
    }
    f
}

/// The values update that puts `guid` (0 to empty it) in the backpack's `slot`.
pub fn slot_change(owner: u64, slot: u8, guid: u64) -> Packet {
    let mut f = Fields::default();
    f.u64(PLAYER_FIELD_INV_SLOT_HEAD + 2 * u16::from(slot), guid);
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(owner, &f))
}

/// The values update of the purse.
pub fn money_change(owner: u64, money: u32) -> Packet {
    let mut f = Fields::default();
    f.u32(PLAYER_FIELD_COINAGE, money);
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(owner, &f))
}

/// The values update that writes the whole quest log: each slot's quest id and its counter word
/// (four 6-bit counters, the state in the top byte), the rest empty.
pub fn quest_log(owner: u64, slots: &[(u32, u32)]) -> Packet {
    let mut f = Fields::default();
    for i in 0..QUEST_LOG_SLOTS {
        let (id, word) = slots.get(usize::from(i)).copied().unwrap_or((0, 0));
        let at = PLAYER_QUEST_LOG_1_1 + 3 * i;
        f.u32(at, id).u32(at + 1, word).u32(at + 2, 0);
    }
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(owner, &f))
}

/// The values update that seats the farmer on `display` (0 to step down), for them and everyone.
pub fn mount_change(owner: u64, display: u32) -> Packet {
    let mut f = Fields::default();
    f.u32(UNIT_FIELD_MOUNTDISPLAYID, display);
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(owner, &f))
}

/// The values update that draws the melee weapon (`true`) or sheathes it.
pub fn sheath_change(owner: u64, drawn: bool) -> Packet {
    let mut f = Fields::default();
    f.u32(UNIT_FIELD_BYTES_2, u32::from(drawn));
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(owner, &f))
}

/// The values update of the stand state: 0 stand, 1 sit, 3 sleep, 8 kneel.
pub fn stand_change(owner: u64, stand: u8) -> Packet {
    let mut f = Fields::default();
    f.u32(UNIT_FIELD_BYTES_1, stand.into());
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(owner, &f))
}

/// The `SMSG_UPDATE_OBJECT` that creates the farmer at `living` for themself: their worn and
/// carried items first, then the body, as vmangos builds it.
pub fn create_self(c: &Character, living: &Living, carried: &[Held], money: u32) -> Packet {
    let mut blocks = Vec::new();
    for h in outfit(c.guid).iter().chain(carried) {
        blocks.push(items::create_block(h, c.guid));
    }
    blocks.push(update::create_player(
        c.guid,
        true,
        living,
        &fields(c, true, carried, money),
    ));
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::join(&blocks))
}

/// A body standing at `at`.
pub fn standing(at: Location) -> Living {
    Living {
        movement_flags: 0,
        time: 0,
        position: at.position,
        orientation: at.orientation,
        speeds: BASE_SPEEDS,
    }
}

/// What `CMSG_PLAYER_LOGIN` answers, in vmangos's order: the world to load, the spell book and
/// action bar, the clock, then the body.
pub fn login_packets(
    c: &Character,
    at: Location,
    clock: Packet,
    carried: &[Held],
    money: u32,
) -> Vec<Packet> {
    vec![
        packets::login_verify_world(at),
        spells::initial_spells(),
        spells::action_buttons(),
        clock,
        create_self(c, &standing(at), carried, money),
    ]
}

/// The `MSG_MOVE_*` a client sends for its own body.
pub fn is_client_move(op: u16) -> bool {
    matches!(
        op,
        opcode::MSG_MOVE_START_FORWARD
            | opcode::MSG_MOVE_START_BACKWARD
            | opcode::MSG_MOVE_STOP
            | opcode::MSG_MOVE_START_STRAFE_LEFT
            | opcode::MSG_MOVE_START_STRAFE_RIGHT
            | opcode::MSG_MOVE_STOP_STRAFE
            | opcode::MSG_MOVE_JUMP
            | opcode::MSG_MOVE_START_TURN_LEFT
            | opcode::MSG_MOVE_START_TURN_RIGHT
            | opcode::MSG_MOVE_STOP_TURN
            | opcode::MSG_MOVE_START_PITCH_UP
            | opcode::MSG_MOVE_START_PITCH_DOWN
            | opcode::MSG_MOVE_STOP_PITCH
            | opcode::MSG_MOVE_SET_RUN_MODE
            | opcode::MSG_MOVE_SET_WALK_MODE
            | opcode::MSG_MOVE_FALL_LAND
            | opcode::MSG_MOVE_START_SWIM
            | opcode::MSG_MOVE_STOP_SWIM
            | opcode::MSG_MOVE_SET_FACING
            | opcode::MSG_MOVE_SET_PITCH
            | opcode::MSG_MOVE_HEARTBEAT
    )
}

/// The position and facing at the head of a client `MovementInfo`: flags, time, x, y, z,
/// orientation.
pub fn read_pose(movement: &[u8]) -> Option<([f32; 3], f32)> {
    let word = |i: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            movement.get(i * 4..i * 4 + 4)?.try_into().ok()?,
        ))
    };
    let float = |i: usize| word(i).map(f32::from_bits);
    Some(([float(2)?, float(3)?, float(4)?], float(5)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};

    fn farmer() -> Character {
        Character {
            guid: 1,
            name: "Hoggle".into(),
            race: 1,
            class: 1,
            gender: 0,
            skin: 1,
            face: 1,
            hair_style: 1,
            hair_color: 1,
            facial_hair: 1,
        }
    }

    #[test]
    fn the_login_packets_read_back_whole_in_the_client() {
        let at = Location {
            map: 0,
            zone: 12,
            position: [-9900.0, 345.0, 35.5],
            orientation: 0.0,
        };
        let carried = [pick(1)];
        for p in login_packets(
            &farmer(),
            at,
            packets::time_speed(6, 0, 1, 1.2),
            &carried,
            200,
        ) {
            let (parsed, tail) = parse_server_with_tail(p.opcode, &p.body).expect("parses");
            assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
            if let ServerPacket::UpdateObject { objects } = parsed {
                assert_eq!(objects.len(), items::OUTFIT.len() + 2, "outfit, pick, body");
            }
        }
        for p in [
            slot_change(1, 24, 7),
            money_change(1, 5),
            stand_change(1, 3),
        ] {
            parse_server_with_tail(p.opcode, &p.body).expect("parses");
        }
    }
}
