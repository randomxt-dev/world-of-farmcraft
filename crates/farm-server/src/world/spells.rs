//! The spell book, the action bar and the cast-pipeline packets that answer a press. Layouts are
//! the ones `benilla-protocol`'s `spellbook`, `action_bar` and `spells` readers take.

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::wire::Body;

/// Herb Gathering: the client casts it on a ripe crop, which carries Herbalism's lock.
pub const HERB_GATHERING: u32 = 2366;
/// Mining: the pickaxe swing, cast on an ore vein with the Mining Pick in the bags.
pub const MINING: u32 = 2575;
/// "Plant Seeds" (`Spell.dbc` 5206): the bar shown while sowing.
pub const PLANT_SEEDS: u32 = 5206;
/// "Sprinkling Purified Water" (17155): the bar shown while watering.
pub const WATERING: u32 = 17155;

/// The spells the client opens nodes with: it casts the first known spell whose lock type the
/// node's lock names, so they must be in the book.
const BOOK: [u32; 2] = [HERB_GATHERING, MINING];

/// `SMSG_INITIAL_SPELLS`: `u8 0`, a `u16` count of `{u16 spell, u16 0}`, no cooldowns.
pub fn initial_spells() -> Packet {
    let mut b = Body::default();
    b.u8(0).u16(BOOK.len() as u16);
    for spell in BOOK {
        b.u16(spell as u16).u16(0);
    }
    b.u16(0);
    Packet::new(opcode::SMSG_INITIAL_SPELLS, b.done())
}

/// `SMSG_ACTION_BUTTONS`: 120 empty slot words; the farm needs no buttons.
pub fn action_buttons() -> Packet {
    let mut b = Body::default();
    for _ in 0..120 {
        b.u32(0);
    }
    Packet::new(opcode::SMSG_ACTION_BUTTONS, b.done())
}

/// What a cast aims at, as `SpellCastTargets` carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Mask 0: the caster itself, or the spell's implicit target.
    Caster,
    /// `TARGET_FLAG_UNIT`.
    Unit(u64),
    /// `TARGET_FLAG_GAMEOBJECT`.
    GameObject(u64),
}

const TARGET_FLAG_UNIT: u16 = 0x0002;
const TARGET_FLAG_GAMEOBJECT: u16 = 0x0800;

impl Target {
    fn write(self, b: &mut Body) {
        match self {
            Target::Caster => b.u16(0),
            Target::Unit(g) => b.u16(TARGET_FLAG_UNIT).packed_guid(g),
            Target::GameObject(g) => b.u16(TARGET_FLAG_GAMEOBJECT).packed_guid(g),
        };
    }
}

/// A client's `CMSG_CAST_SPELL`: `u32` spell, then `SpellCastTargets`. Only the unit and
/// gameobject guids matter here; any other target reads as [`Target::Caster`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastRequest {
    pub spell: u32,
    pub target: Target,
}

impl CastRequest {
    pub fn read(body: &[u8]) -> std::io::Result<Self> {
        use benilla_protocol::wire::{read_packed_guid, read_u16_le, read_u32_le};
        let mut cur = body;
        let spell = read_u32_le(&mut cur)?;
        let mask = read_u16_le(&mut cur)?;
        let target = if mask & TARGET_FLAG_UNIT != 0 {
            Target::Unit(read_packed_guid(&mut cur)?)
        } else if mask & TARGET_FLAG_GAMEOBJECT != 0 {
            Target::GameObject(read_packed_guid(&mut cur)?)
        } else {
            Target::Caster
        };
        Ok(Self { spell, target })
    }
}

/// `SPELL_FAILED_INTERRUPTED`: a cast broke by moving, a meeting or death.
pub const FAILED_INTERRUPTED: u8 = 0x23;
/// `SPELL_FAILED_DONT_REPORT`: refused with no client message of its own; the reason goes out as
/// an [`notification`].
pub const FAILED_SILENT: u8 = 0x17;

/// `SMSG_CAST_RESULT` refusing the caster's `spell` for `reason`.
pub fn cast_failed(spell: u32, reason: u8) -> Packet {
    Packet::new(
        opcode::SMSG_CAST_RESULT,
        Body::default().u32(spell).u8(2).u8(reason).done(),
    )
}

/// `SMSG_CAST_RESULT` accepting the caster's `spell`: frees the client's in-flight guard for it
/// when the cast is shown as another spell.
pub fn cast_ok(spell: u32) -> Packet {
    Packet::new(
        opcode::SMSG_CAST_RESULT,
        Body::default().u32(spell).u8(0).done(),
    )
}

/// `SMSG_SPELL_START`: `caster` began `spell`, a bar of `cast_ms` (0 for an instant).
pub fn spell_start(caster: u64, spell: u32, cast_ms: u32, target: Target) -> Packet {
    let mut b = Body::default();
    b.packed_guid(caster)
        .packed_guid(caster)
        .u32(spell)
        .u16(0x0002)
        .u32(cast_ms);
    target.write(&mut b);
    Packet::new(opcode::SMSG_SPELL_START, b.done())
}

/// `SMSG_SPELL_GO`: `caster`'s `spell` launched, hitting the target unit or gameobject if any.
pub fn spell_go(caster: u64, spell: u32, target: Target) -> Packet {
    let mut b = Body::default();
    b.packed_guid(caster)
        .packed_guid(caster)
        .u32(spell)
        .u16(0x0100);
    match target {
        Target::Unit(g) | Target::GameObject(g) => b.u8(1).u64(g),
        Target::Caster => b.u8(0),
    };
    b.u8(0); // no misses
    target.write(&mut b);
    Packet::new(opcode::SMSG_SPELL_GO, b.done())
}

/// `SMSG_SPELL_FAILED_OTHER`: an observer sees `caster`'s `spell` break.
pub fn spell_failed_other(caster: u64, spell: u32) -> Packet {
    Packet::new(
        opcode::SMSG_SPELL_FAILED_OTHER,
        Body::default().u64(caster).u32(spell).done(),
    )
}

/// `SMSG_SPELL_COOLDOWN`: `caster`'s spells cool down for the given milliseconds.
pub fn spell_cooldown(caster: u64, cooldowns: &[(u32, u32)]) -> Packet {
    let mut b = Body::default();
    b.u64(caster);
    for &(spell, ms) in cooldowns {
        b.u32(spell).u32(ms);
    }
    Packet::new(opcode::SMSG_SPELL_COOLDOWN, b.done())
}

/// `SMSG_CLEAR_COOLDOWN`: `spell`'s cooldown on `caster`'s client ends outright, so a shorter one
/// sent after replaces the borrowed spell's own.
pub fn clear_cooldown(caster: u64, spell: u32) -> Packet {
    Packet::new(
        opcode::SMSG_CLEAR_COOLDOWN,
        Body::default().u32(spell).u64(caster).done(),
    )
}

/// `SMSG_NOTIFICATION`: one line of text for the receiver.
pub fn notification(text: &str) -> Packet {
    Packet::new(
        opcode::SMSG_NOTIFICATION,
        Body::default().cstring(text).done(),
    )
}
