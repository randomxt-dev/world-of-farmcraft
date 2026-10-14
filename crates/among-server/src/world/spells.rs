//! Among Azeroth's buttons as existing 1.12 spells on WoW's own action bar, each class's one combat
//! spell, and the cast-pipeline packets that answer a press. Layouts are the ones
//! `benilla-protocol`'s `spellbook`, `action_bar` and `spells` readers take.

use among_rules::Class;
use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::wire::Body;

/// Melee auto-attack.
pub const ATTACK: u32 = 6603;
/// NPC Eviscerate: the Kill button (`Ability_Rogue_Eviscerate`).
pub const KILL: u32 = 15692;
/// "Salt Flats Racer Normal", the Use button (`INV_Misc_Gear_01`).
pub const USE: u32 = 6602;
/// Raise Dead's icon (`Spell_Shadow_RaiseDead`): the Report button.
pub const REPORT: u32 = 6468;
/// Self Destruct: the Sabotage button.
pub const SABOTAGE: u32 = 9874;
/// Ability_Repair, a 3 s cast: the Fix button.
pub const FIX: u32 = 3605;
/// The Lion Horn of Stormwind: the Emergency button.
pub const EMERGENCY: u32 = 18946;
/// Negative Charge: skip the vote.
pub const SKIP_VOTE: u32 = 28084;

/// The class's one combat spell: its level-1 starter.
pub fn class_spell(class: Class) -> u32 {
    match class {
        Class::Warrior => 78,    // Heroic Strike
        Class::Paladin => 20271, // Judgement
        Class::Hunter => 2973,   // Raptor Strike
        Class::Rogue => 1752,    // Sinister Strike
        Class::Priest => 585,    // Smite
        Class::Shaman => 403,    // Lightning Bolt
        Class::Mage => 133,      // Fireball
        Class::Warlock => 686,   // Shadow Bolt
        Class::Druid => 5176,    // Wrath
    }
}

/// The class spell's school, for the damage log: physical strikes, holy, nature, fire, shadow.
pub fn class_school(class: Class) -> u8 {
    match class {
        Class::Warrior | Class::Hunter | Class::Rogue => 0,
        Class::Paladin | Class::Priest => 1,
        Class::Mage => 2,
        Class::Shaman | Class::Druid => 3,
        Class::Warlock => 5,
    }
}

/// The main bar, slots 0-8 (keys 1-9): fight, then the Among Us buttons.
pub fn main_bar(class: Class) -> [u32; 9] {
    [
        ATTACK,
        class_spell(class),
        USE,
        REPORT,
        KILL,
        SABOTAGE,
        FIX,
        EMERGENCY,
        SKIP_VOTE,
    ]
}

/// The spells the client opens task nodes with: it casts the first known spell whose lock type
/// the node's lock names, so they must be in the book, though not on the bar.
pub const OPENERS: [u32; 2] = [crate::look::HERB_GATHERING, crate::look::KNEEL];

/// `SMSG_INITIAL_SPELLS`: `u8 0`, a `u16` count of `{u16 spell, u16 0}`, no cooldowns. The book
/// is the bar, then the node openers.
pub fn initial_spells(class: Class) -> Packet {
    let book: Vec<u32> = main_bar(class).into_iter().chain(OPENERS).collect();
    let mut b = Body::default();
    b.u8(0).u16(book.len() as u16);
    for spell in book {
        b.u16(spell as u16).u16(0);
    }
    b.u16(0);
    Packet::new(opcode::SMSG_INITIAL_SPELLS, b.done())
}

/// `SMSG_ACTION_BUTTONS`: 120 slot words, `spell | kind << 24` with kind 0 for a spell; the main
/// bar filled, the rest empty.
pub fn action_buttons(class: Class) -> Packet {
    let mut slots = [0u32; 120];
    slots[..9].copy_from_slice(&main_bar(class));
    let mut b = Body::default();
    for s in slots {
        b.u32(s);
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

/// `SMSG_NOTIFICATION`: one line of text for the receiver.
pub fn notification(text: &str) -> Packet {
    Packet::new(
        opcode::SMSG_NOTIFICATION,
        Body::default().cstring(text).done(),
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

    #[test]
    fn the_book_and_the_bar_hold_the_class_spell_and_every_button() {
        let ServerPacket::InitialSpells {
            spell_ids,
            cooldowns,
        } = parse(&initial_spells(Class::Mage))
        else {
            panic!("not a spell book");
        };
        assert_eq!(
            spell_ids,
            [6603, 133, 6602, 6468, 15692, 9874, 3605, 18946, 28084, 2366, 6478]
        );
        assert!(cooldowns.is_empty());

        let ServerPacket::ActionButtons { buttons } = parse(&action_buttons(Class::Rogue)) else {
            panic!("not an action bar");
        };
        let bar: Vec<(u8, u32, u8)> = buttons.iter().map(|b| (b.slot, b.action, b.kind)).collect();
        assert_eq!(bar.len(), 9);
        assert_eq!(bar[0], (0, ATTACK, 0));
        assert_eq!(bar[1], (1, 1752, 0), "Sinister Strike on key 2");
        assert_eq!(bar[4], (4, KILL, 0));
        assert_eq!(bar[8], (8, SKIP_VOTE, 0));
    }

    #[test]
    fn cast_packets_read_back_whole_in_the_client() {
        use benilla_protocol::messages::CastOutcome;
        let ServerPacket::CastResult { spell_id, outcome } =
            parse(&cast_failed(KILL, FAILED_SILENT))
        else {
            panic!("not a cast result");
        };
        assert_eq!(spell_id, KILL);
        assert_eq!(
            outcome,
            CastOutcome::Failed {
                reason: 0x17,
                arg: None
            }
        );

        let target = Target::Unit(0x2a);
        let ServerPacket::SpellStart(start) = parse(&spell_start(7, 2366, 5000, target)) else {
            panic!("not a spell start");
        };
        assert_eq!(
            (start.caster, start.spell_id, start.cast_time_ms),
            (7, 2366, 5000)
        );
        assert_eq!(start.targets.unit_target, Some(0x2a));
        let ServerPacket::SpellStart(start) = parse(&spell_start(7, USE, 0, Target::Caster)) else {
            panic!("not a spell start");
        };
        assert_eq!((start.targets.mask, start.cast_time_ms), (0, 0));

        let node = Target::GameObject(0xF110_0000_0100_0001);
        let ServerPacket::SpellGo(go) = parse(&spell_go(7, 2366, node)) else {
            panic!("not a spell go");
        };
        assert_eq!((go.caster, go.spell_id), (7, 2366));
        assert_eq!(go.hits, [0xF110_0000_0100_0001]);
        assert_eq!(go.targets.go_target, Some(0xF110_0000_0100_0001));
        let ServerPacket::SpellGo(go) = parse(&spell_go(7, REPORT, Target::Caster)) else {
            panic!("not a spell go");
        };
        assert!(go.hits.is_empty() && go.misses.is_empty());

        assert!(matches!(
            parse(&spell_failed_other(7, 2366)),
            ServerPacket::SpellFailedOther {
                caster: 7,
                spell_id: 2366
            }
        ));
        let ServerPacket::SpellCooldownList { caster, cooldowns } =
            parse(&spell_cooldown(7, &[(KILL, 25_000), (133, 1500)]))
        else {
            panic!("not a cooldown list");
        };
        assert_eq!((caster, cooldowns), (7, vec![(KILL, 25_000), (133, 1500)]));
        let ServerPacket::Notification { text } = parse(&notification("too far away")) else {
            panic!("not a notification");
        };
        assert_eq!(text, "too far away");
    }

    /// The client's own cast bodies, read back.
    #[test]
    fn a_cast_request_reads_what_the_client_sends() {
        use benilla_protocol::messages::{cast_spell, cast_spell_gameobject};
        let read = |b: Vec<u8>| CastRequest::read(&b).unwrap();
        assert_eq!(
            read(cast_spell(KILL, Some(9))),
            CastRequest {
                spell: KILL,
                target: Target::Unit(9)
            }
        );
        assert_eq!(read(cast_spell(USE, None)).target, Target::Caster);
        assert_eq!(
            read(cast_spell_gameobject(2366, 0xF110_0000_0100_0001)).target,
            Target::GameObject(0xF110_0000_0100_0001)
        );
    }

    #[test]
    fn every_class_spell_fits_the_books_u16() {
        for class in Class::ALL {
            assert!(class_spell(class) <= u32::from(u16::MAX), "{class:?}");
        }
        assert!(main_bar(Class::Druid)
            .iter()
            .all(|&s| s <= u32::from(u16::MAX)));
    }
}
