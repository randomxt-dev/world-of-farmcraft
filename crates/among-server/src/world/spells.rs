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

/// `SMSG_INITIAL_SPELLS`: `u8 0`, a `u16` count of `{u16 spell, u16 0}`, no cooldowns. Every
/// spell on the bar is in the book.
pub fn initial_spells(class: Class) -> Packet {
    let book = main_bar(class);
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
            [6603, 133, 6602, 6468, 15692, 9874, 3605, 18946, 28084]
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
    fn every_class_spell_fits_the_books_u16() {
        for class in Class::ALL {
            assert!(class_spell(class) <= u32::from(u16::MAX), "{class:?}");
        }
        assert!(main_bar(Class::Druid)
            .iter()
            .all(|&s| s <= u32::from(u16::MAX)));
    }
}
