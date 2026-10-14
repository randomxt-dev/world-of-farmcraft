//! A player as the world shows it: where it spawns, the descriptor it is created with, the packets
//! that seat it at login, and the movement a client sends for it.

use benilla_protocol::messages::opcode;

use super::packets::{self, Location};
use super::socket::Packet;
use super::update::{self, Fields, Living, BASE_SPEEDS};
use crate::accounts::CharacterRecord;

/// Northshire Abbey's steps, the human start (vmangos `playercreateinfo`: map 0, zone 12).
pub const SPAWN: Location = Location {
    map: 0,
    zone: 12,
    position: [-8949.95, -132.493, 83.5312],
    orientation: 0.0,
};

/// A fresh body at [`SPAWN`], standing still at base speeds.
pub fn spawn_living() -> Living {
    Living {
        movement_flags: 0,
        time: 0,
        position: SPAWN.position,
        orientation: SPAWN.orientation,
        speeds: BASE_SPEEDS,
    }
}

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
const UNIT_MOD_CAST_SPEED: u16 = 145;
const PLAYER_BYTES: u16 = 193;
const PLAYER_BYTES_2: u16 = 194;
const PLAYER_BYTES_3: u16 = 195;
const PLAYER_NEXT_LEVEL_XP: u16 = 717;

/// `TYPEMASK_OBJECT | TYPEMASK_UNIT | TYPEMASK_PLAYER`.
const TYPE_PLAYER: u32 = 0x01 | 0x08 | 0x10;
/// `UNIT_FLAG_PVP_ATTACKABLE`, which vmangos sets on every player.
const UNIT_FLAG_PVP_ATTACKABLE: u32 = 0x08;
/// `REST_STATE_NORMAL` in `PLAYER_BYTES_2`'s top byte.
const REST_STATE_NORMAL: u32 = 0x02;

/// The race's `CreatureDisplayInfo` id for `gender` (`ChrRaces.dbc` male/female display).
fn display_id(race: u8, gender: u8) -> u32 {
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

/// The race's player faction template (`ChrRaces.dbc` `FactionID`).
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

/// The class's power type in `UNIT_FIELD_BYTES_0`: rage for warriors, energy for rogues, else mana.
fn power_type(class: u8) -> u8 {
    match class {
        1 => 1,
        4 => 3,
        _ => 0,
    }
}

/// The player's create-time descriptor.
pub fn fields(c: &CharacterRecord) -> Fields {
    let mut f = Fields::default();
    let power = power_type(c.class);
    let max_power = if power == 1 { 1000 } else { 100 };
    f.u64(OBJECT_FIELD_GUID, c.guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_PLAYER)
        .f32(OBJECT_FIELD_SCALE_X, 1.0)
        .u32(UNIT_FIELD_HEALTH, 100)
        .u32(UNIT_FIELD_MAXHEALTH, 100)
        .u32(
            UNIT_FIELD_POWER1 + u16::from(power),
            if power == 1 { 0 } else { max_power },
        )
        .u32(UNIT_FIELD_MAXPOWER1 + u16::from(power), max_power)
        .u32(UNIT_FIELD_LEVEL, c.level.into())
        .u32(UNIT_FIELD_FACTIONTEMPLATE, faction_template(c.race))
        .u32(
            UNIT_FIELD_BYTES_0,
            u32::from(c.race)
                | u32::from(c.class) << 8
                | u32::from(c.gender) << 16
                | u32::from(power) << 24,
        )
        .u32(UNIT_FIELD_FLAGS, UNIT_FLAG_PVP_ATTACKABLE)
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
    f
}

/// The `SMSG_UPDATE_OBJECT` that creates `c` at `living`; `is_self` marks the receiver's own body.
pub fn create_packet(c: &CharacterRecord, living: &Living, is_self: bool) -> Packet {
    Packet::new(
        opcode::SMSG_UPDATE_OBJECT,
        update::create_player(c.guid, is_self, living, &fields(c)),
    )
}

/// What `CMSG_PLAYER_LOGIN` answers, in vmangos's order: the world to load, then the body.
pub fn login_packets(c: &CharacterRecord) -> Vec<Packet> {
    vec![
        packets::login_verify_world(SPAWN),
        packets::login_set_time_speed(),
        create_packet(c, &spawn_living(), true),
    ]
}

/// The `MSG_MOVE_*` a client sends for its own body, which the server relays to the others.
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

/// The pose at the head of a client `MovementInfo`: flags, time, x, y, z, orientation.
pub fn read_pose(movement: &[u8]) -> Option<Living> {
    let word = |i: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            movement.get(i * 4..i * 4 + 4)?.try_into().ok()?,
        ))
    };
    let float = |i: usize| word(i).map(f32::from_bits);
    Some(Living {
        movement_flags: word(0)?,
        time: word(1)?,
        position: [float(2)?, float(3)?, float(4)?],
        orientation: float(5)?,
        speeds: BASE_SPEEDS,
    })
}
