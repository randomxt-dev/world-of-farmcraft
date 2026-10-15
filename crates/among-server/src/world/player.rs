//! A player as the world shows it: where it spawns, the descriptor it is created with, the packets
//! that seat it at login, and the movement a client sends for it.

use among_rules::{Class, Effect};
use benilla_protocol::messages::opcode;

use super::items;
use super::packets::{self, Location};
use super::socket::Packet;
use super::spells;
use super::update::{self, Fields, Living, PLAYER_SPEEDS};
use crate::accounts::CharacterRecord;

/// Northshire Abbey's steps, the human start (vmangos `playercreateinfo`: map 0, zone 12).
pub const SPAWN: Location = Location {
    map: 0,
    zone: 12,
    position: [-8949.95, -132.493, 83.5312],
    orientation: 0.0,
};

/// Where a player stands between life and death, as the world shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Life {
    #[default]
    Alive,
    /// Fallen where they died, health 0, the Release Spirit popup up.
    Dead,
    /// Released: health 1, `PLAYER_FLAGS_GHOST` and the Ghost aura, seen only by ghosts.
    Ghost,
}

/// A fresh body at [`SPAWN`], standing still at [`PLAYER_SPEEDS`].
pub fn spawn_living() -> Living {
    Living {
        movement_flags: 0,
        time: 0,
        position: SPAWN.position,
        orientation: SPAWN.orientation,
        speeds: PLAYER_SPEEDS,
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
const UNIT_FIELD_AURA: u16 = 47;
const UNIT_FIELD_AURAFLAGS: u16 = 95;
const UNIT_FIELD_AURALEVELS: u16 = 101;
const UNIT_FIELD_AURASTATE: u16 = 125;
const UNIT_FIELD_BASEATTACKTIME: u16 = 126;
const UNIT_FIELD_BOUNDINGRADIUS: u16 = 129;
const UNIT_FIELD_COMBATREACH: u16 = 130;
const UNIT_FIELD_DISPLAYID: u16 = 131;
const UNIT_FIELD_NATIVEDISPLAYID: u16 = 132;
const UNIT_FIELD_BYTES_1: u16 = 138;
const UNIT_MOD_CAST_SPEED: u16 = 145;
const PLAYER_FLAGS: u16 = 190;
const PLAYER_BYTES: u16 = 193;
const PLAYER_BYTES_2: u16 = 194;
const PLAYER_BYTES_3: u16 = 195;
/// `PLAYER_VISIBLE_ITEM_1_0`, the worn entry everyone sees; each slot is 12 words.
const PLAYER_VISIBLE_ITEM_1_0: u16 = 260;
const VISIBLE_ITEM_STRIDE: u16 = 12;
/// `PLAYER_FIELD_INV_SLOT_HEAD`, then two words (an item guid) per equipment slot.
const PLAYER_FIELD_INV_SLOT_HEAD: u16 = 486;
const PLAYER_NEXT_LEVEL_XP: u16 = 717;
/// `PLAYER_EXPLORED_ZONES_1`: 64 words, bit n set for `AreaTable.dbc` exploreFlag n.
const PLAYER_EXPLORED_ZONES_1: u16 = 1111;
const EXPLORED_ZONE_WORDS: u16 = 64;
const PLAYER_FIELD_BYTES: u16 = 1222;

/// `TYPEMASK_OBJECT | TYPEMASK_UNIT | TYPEMASK_PLAYER`.
const TYPE_PLAYER: u32 = 0x01 | 0x08 | 0x10;
/// `UNIT_FLAG_PVP_ATTACKABLE`, which vmangos sets on every player.
const UNIT_FLAG_PVP_ATTACKABLE: u32 = 0x08;
/// `REST_STATE_NORMAL` in `PLAYER_BYTES_2`'s top byte.
const REST_STATE_NORMAL: u32 = 0x02;
/// `PLAYER_FLAGS_FFA_PVP`: every player may attack every other, so Kill can target anyone.
const PLAYER_FLAGS_FFA_PVP: u32 = 0x80;
/// `PLAYER_FLAGS_GHOST`: the client's ghost test, which turns on the death light and sky.
const PLAYER_FLAGS_GHOST: u32 = 0x10;
/// Spell 8326 Ghost, the aura a released spirit wears; the client tints and fades its wearer.
pub const GHOST_AURA: u32 = 8326;
/// Ghost's aura slot: the first debuff slot, as `Spell.dbc` marks 8326 `SPELL_ATTR_NEGATIVE`.
const GHOST_SLOT: u16 = 32;
/// `AFLAG_EFF_INDEX_0 | 1 | 2`: all three of Ghost's effects live, in its slot's nibble.
const GHOST_AURA_FLAGS: u32 = 0x0E;
/// `PLAYER_FIELD_BYTE_RELEASE_TIMER` in `PLAYER_FIELD_BYTES`' low byte: the Release Spirit
/// popup counts down, as vmangos's `KillPlayer` sets it.
const RELEASE_TIMER: u32 = 0x08;
/// `UNIT_BYTE1_FLAG_GHOST` in `UNIT_FIELD_BYTES_1`'s top byte: no name or plate over a ghost.
const UNIT_BYTE1_FLAG_GHOST: u32 = 0x01 << 24;
/// `AURA_STATE_JUDGEMENT` (5) as its `UNIT_FIELD_AURASTATE` bit: Judgement greys out without it.
const AURASTATE_JUDGEMENT: u32 = 1 << (5 - 1);
/// The slot of an item guid the client is never sent, held in the main hand of a character
/// whose race and class start with no weapon: the client's weapon check passes an item it cannot
/// resolve, so Kill and the melee strikes stay castable.
const PHANTOM_SLOT: u8 = 0xFF;

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

/// Creature 12861 "Wisp (Ghost Visual Only)", which vmangos's 20584 turns a night-elf ghost into.
pub const WISP_DISPLAY: u32 = 1825;
/// Creature 16372 "Polymorphed Sheep", Polymorph's transform.
pub const SHEEP_DISPLAY: u32 = 856;
/// Ghost Wolf's form display (vmangos), the coyote-skinned wolf at its 0.8 scale.
pub const GHOST_WOLF_DISPLAY: u32 = 4613;
const GHOST_WOLF_SCALE: f32 = 0.8;
/// `UNIT_FLAG_STUNNED`: no turning, no casting; a trap's frozen catch.
const UNIT_FLAG_STUNNED: u32 = 0x0004_0000;
/// `UNIT_FLAG_FLEEING`: the client gives up control of the body and greys the bar; fear.
const UNIT_FLAG_FLEEING: u32 = 0x0080_0000;
/// `AFLAG_EFF_INDEX_0`: one live effect, and no cancel bit, so a re-press casts (and is refused
/// on cooldown) rather than sending a `CMSG_CANCEL_AURA` the server does not take.
const EFFECT_AURA_FLAGS: u32 = 0x02;

/// Every effect, with the aura slot it shows in: buffs from 0, debuffs from 40, clear of Ghost's
/// slot 32 and the words it shares.
const EFFECT_SLOTS: [(Effect, u16); 9] = [
    (Effect::Stealth, 0),
    (Effect::Shield, 1),
    (Effect::GhostWolf, 2),
    (Effect::DivineShield, 3),
    (Effect::Polymorph, 40),
    (Effect::Fear, 41),
    (Effect::Slow, 42),
    (Effect::Frozen, 43),
    (Effect::Rooted, 44),
];

/// What a player looks like beyond life and death: the display (a ghost's wisp, a sheep, a ghost
/// wolf, else their race's), its scale, the unit flags effects set, and each effect's aura. Every
/// field is written in each state, so an update of them alone undoes the last.
pub fn bearing(c: &CharacterRecord, life: Life, effects: &[Effect]) -> Fields {
    let under = |e| life == Life::Alive && effects.contains(&e);
    let (display, scale) = if life == Life::Ghost {
        (WISP_DISPLAY, 1.0)
    } else if under(Effect::Polymorph) {
        (SHEEP_DISPLAY, 1.0)
    } else if under(Effect::GhostWolf) {
        (GHOST_WOLF_DISPLAY, GHOST_WOLF_SCALE)
    } else {
        (display_id(c.race, c.gender), 1.0)
    };
    let mut flags = UNIT_FLAG_PVP_ATTACKABLE;
    if under(Effect::Frozen) {
        flags |= UNIT_FLAG_STUNNED;
    }
    if under(Effect::Fear) {
        flags |= UNIT_FLAG_FLEEING;
    }
    let mut f = Fields::default();
    f.u32(UNIT_FIELD_DISPLAYID, display)
        .f32(OBJECT_FIELD_SCALE_X, scale)
        .u32(UNIT_FIELD_FLAGS, flags);
    let mut aura_flags: std::collections::BTreeMap<u16, u32> = Default::default();
    let mut levels: std::collections::BTreeMap<u16, u32> = Default::default();
    for (effect, slot) in EFFECT_SLOTS {
        let on = under(effect);
        f.u32(
            UNIT_FIELD_AURA + slot,
            if on {
                among_spells::effect_aura(effect)
            } else {
                0
            },
        );
        // Eight 4-bit flag slots and four 1-byte levels per word.
        let flag_word = aura_flags.entry(slot / 8).or_default();
        let level_word = levels.entry(slot / 4).or_default();
        if on {
            *flag_word |= EFFECT_AURA_FLAGS << (slot % 8 * 4);
            *level_word |= 1 << (slot % 4 * 8);
        }
    }
    for (word, v) in aura_flags {
        f.u32(UNIT_FIELD_AURAFLAGS + word, v);
    }
    for (word, v) in levels {
        f.u32(UNIT_FIELD_AURALEVELS + word, v);
    }
    f
}

/// The values update that shows `guid` wearing `bearing`.
pub fn bearing_change(guid: u64, bearing: &Fields) -> Packet {
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(guid, bearing))
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

/// The class's power type in `UNIT_FIELD_BYTES_0` and the pool it is held full at: rage for
/// warriors (stored x10), energy for rogues, else mana. The client checks a spell's cost against
/// it before sending the cast, and the server never spends it.
fn power(class: u8) -> (u8, u32) {
    match class {
        1 => (1, 1000),
        4 => (3, 100),
        _ => (0, 1000),
    }
}

/// The character's class as the rules know it; anything else fights as a warrior.
pub fn class(c: &CharacterRecord) -> Class {
    Class::from_wow(c.class).unwrap_or(Class::Warrior)
}

/// The player's create-time descriptor in `life`; `is_self` adds the fields only the owner is
/// sent.
pub fn fields(c: &CharacterRecord, is_self: bool, life: Life) -> Fields {
    let mut f = Fields::default();
    let (power, pool) = power(c.class);
    f.u64(OBJECT_FIELD_GUID, c.guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_PLAYER)
        .f32(OBJECT_FIELD_SCALE_X, 1.0)
        .u32(UNIT_FIELD_HEALTH, 100)
        .u32(UNIT_FIELD_MAXHEALTH, 100)
        .u32(UNIT_FIELD_POWER1 + u16::from(power), pool)
        .u32(UNIT_FIELD_MAXPOWER1 + u16::from(power), pool)
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
        .u32(UNIT_FIELD_AURASTATE, AURASTATE_JUDGEMENT)
        .u32(UNIT_FIELD_BASEATTACKTIME, 2000)
        .u32(UNIT_FIELD_BASEATTACKTIME + 1, 2000)
        .f32(UNIT_FIELD_BOUNDINGRADIUS, 0.389)
        .f32(UNIT_FIELD_COMBATREACH, 1.5)
        .u32(UNIT_FIELD_DISPLAYID, display_id(c.race, c.gender))
        .u32(UNIT_FIELD_NATIVEDISPLAYID, display_id(c.race, c.gender))
        .f32(UNIT_MOD_CAST_SPEED, 1.0)
        .u32(PLAYER_FLAGS, PLAYER_FLAGS_FFA_PVP)
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
    let gear = items::starting_gear(c.race, c.class, c.guid);
    for w in &gear {
        let slot = u16::from(w.slot);
        f.u32(
            PLAYER_VISIBLE_ITEM_1_0 + VISIBLE_ITEM_STRIDE * slot,
            w.item.entry,
        );
        if is_self {
            f.u64(PLAYER_FIELD_INV_SLOT_HEAD + 2 * slot, w.guid);
        }
    }
    if is_self {
        // Every area counts as explored, so the world map opens with Northshire drawn in full.
        for i in 0..EXPLORED_ZONE_WORDS {
            f.u32(PLAYER_EXPLORED_ZONES_1 + i, u32::MAX);
        }
        for w in items::carried(c.guid) {
            f.u64(PLAYER_FIELD_INV_SLOT_HEAD + 2 * u16::from(w.slot), w.guid);
        }
    }
    if is_self && !gear.iter().any(|w| w.slot == items::MAINHAND) {
        f.u64(
            PLAYER_FIELD_INV_SLOT_HEAD + 2 * u16::from(items::MAINHAND),
            items::item_guid(c.guid, PHANTOM_SLOT),
        );
    }
    if life != Life::Alive {
        life_fields(&mut f, life, 100);
    }
    f
}

/// The fields that tell `life` apart: health (`max` when alive), the ghost flags and aura. Every
/// one is written in each state, so a values update of them alone undoes the last state.
fn life_fields(f: &mut Fields, life: Life, max: u32) -> &mut Fields {
    let ghost = life == Life::Ghost;
    let health = match life {
        Life::Alive => max,
        Life::Dead => 0,
        // vmangos `BuildPlayerRepop`: a released spirit stands on 1 health.
        Life::Ghost => 1,
    };
    let flags = PLAYER_FLAGS_FFA_PVP | if ghost { PLAYER_FLAGS_GHOST } else { 0 };
    f.u32(UNIT_FIELD_HEALTH, health)
        .u32(UNIT_FIELD_MAXHEALTH, max)
        .u32(
            UNIT_FIELD_AURA + GHOST_SLOT,
            if ghost { GHOST_AURA } else { 0 },
        )
        // Eight 4-bit slots per word, four 1-byte levels per word; slot 32 starts each word.
        .u32(
            UNIT_FIELD_AURAFLAGS + GHOST_SLOT / 8,
            if ghost { GHOST_AURA_FLAGS } else { 0 },
        )
        .u32(UNIT_FIELD_AURALEVELS + GHOST_SLOT / 4, u32::from(ghost))
        .u32(
            UNIT_FIELD_BYTES_1,
            if ghost { UNIT_BYTE1_FLAG_GHOST } else { 0 },
        )
        .u32(PLAYER_FLAGS, flags)
        .u32(
            PLAYER_FIELD_BYTES,
            if life == Life::Dead { RELEASE_TIMER } else { 0 },
        )
}

/// The values update that shows `guid` passing into `life`, with `max` health.
pub fn life_change(guid: u64, life: Life, max: u32) -> Packet {
    let mut f = Fields::default();
    life_fields(&mut f, life, max);
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(guid, &f))
}

/// The `SMSG_UPDATE_OBJECT` that creates `c` at `living` in `life`; `is_self` marks the
/// receiver's own body, which comes after its worn items as vmangos builds it.
pub fn create_packet(c: &CharacterRecord, living: &Living, is_self: bool, life: Life) -> Packet {
    create_wearing(c, living, is_self, life, &Fields::default())
}

/// [`create_packet`] with `bearing` (see [`bearing`]) written over the descriptor.
pub fn create_wearing(
    c: &CharacterRecord,
    living: &Living,
    is_self: bool,
    life: Life,
    bearing: &Fields,
) -> Packet {
    let mut blocks = Vec::new();
    if is_self {
        let gear = items::starting_gear(c.race, c.class, c.guid);
        for w in gear.iter().chain(&items::carried(c.guid)) {
            blocks.push(items::create_block(w, c.guid));
        }
    }
    blocks.push(update::create_player(
        c.guid,
        is_self,
        living,
        fields(c, is_self, life).merge(bearing),
    ));
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::join(&blocks))
}

/// What `CMSG_PLAYER_LOGIN` answers, in vmangos's order: the world to load, the spell book and
/// action bar, the clock, then the body. A login is a lobby seat, so the bar is a crewmate's.
pub fn login_packets(c: &CharacterRecord) -> Vec<Packet> {
    vec![
        packets::login_verify_world(SPAWN),
        spells::initial_spells(class(c)),
        spells::action_buttons(class(c), false),
        packets::login_set_time_speed(),
        create_packet(c, &spawn_living(), true, Life::Alive),
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
        speeds: PLAYER_SPEEDS,
    })
}
