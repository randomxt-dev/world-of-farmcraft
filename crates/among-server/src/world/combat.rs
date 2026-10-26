//! Fights on the wire: auto-attack start and stop, swing results, spell damage and health
//! changes, as `benilla-protocol`'s `attack`, `combat_log` and `update_object` readers take them.

use benilla_protocol::messages::opcode;

use super::creatures::{UNIT_FIELD_HEALTH, UNIT_FIELD_MAXHEALTH};
use super::socket::Packet;
use super::update::{self, Fields};
use super::wire::Body;

/// `HITINFO_NORMALSWING2`: a plain landed swing.
const HITINFO_NORMALSWING: u32 = 0x2;
/// `VICTIMSTATE_WOUND`: the victim took the hit.
const VICTIMSTATE_WOUND: u32 = 1;

/// `SMSG_ATTACKSTART`: two raw guids, attacker then victim. The attacker's own echo is what
/// marks its client engaged.
pub fn attack_start(attacker: u64, victim: u64) -> Packet {
    Packet::new(
        opcode::SMSG_ATTACKSTART,
        Body::default().u64(attacker).u64(victim).done(),
    )
}

/// `SMSG_ATTACKSTOP`: packed attacker and victim, then a `u32` the client ignores.
pub fn attack_stop(attacker: u64, victim: u64) -> Packet {
    Packet::new(
        opcode::SMSG_ATTACKSTOP,
        Body::default()
            .packed_guid(attacker)
            .packed_guid(victim)
            .u32(0)
            .done(),
    )
}

/// `SMSG_ATTACKSWING_CANT_ATTACK`, empty: the client drops the swing it asked for.
pub fn cant_attack() -> Packet {
    Packet::new(opcode::SMSG_ATTACKSWING_CANT_ATTACK, Vec::new())
}

/// `SMSG_ATTACKERSTATEUPDATE`: one landed physical swing of `damage`, the attacker's swing cue.
pub fn swing(attacker: u64, victim: u64, damage: u32) -> Packet {
    Packet::new(
        opcode::SMSG_ATTACKERSTATEUPDATE,
        Body::default()
            .u32(HITINFO_NORMALSWING)
            .packed_guid(attacker)
            .packed_guid(victim)
            .u32(damage)
            .u8(1) // one damage block: physical, nothing absorbed or resisted
            .u32(0)
            .f32(damage as f32)
            .u32(damage)
            .u32(0)
            .u32(0)
            .u32(VICTIMSTATE_WOUND)
            .u32(0) // attacker state
            .u32(0) // no melee spell
            .u32(0) // blocked
            .done(),
    )
}

/// `SMSG_SPELLNONMELEEDAMAGELOG`: `attacker`'s `spell` hit `target` for `damage` of `school`.
pub fn spell_damage(target: u64, attacker: u64, spell: u32, damage: u32, school: u8) -> Packet {
    Packet::new(
        opcode::SMSG_SPELLNONMELEEDAMAGELOG,
        Body::default()
            .packed_guid(target)
            .packed_guid(attacker)
            .u32(spell)
            .u32(damage)
            .u8(school)
            .u32(0) // absorbed
            .u32(0) // resisted
            .u8(0) // not periodic
            .u8(0)
            .u32(0) // blocked
            .u32(0) // hit info
            .u8(0)
            .done(),
    )
}

/// A values update setting a unit's health and maximum.
pub fn health(guid: u64, health: u32, max: u32) -> Packet {
    let mut f = Fields::default();
    f.u32(UNIT_FIELD_HEALTH, health)
        .u32(UNIT_FIELD_MAXHEALTH, max);
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(guid, &f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, Object, ServerPacket};

    fn parse(p: &Packet) -> ServerPacket {
        let (packet, tail) =
            parse_server_with_tail(p.opcode, &p.body).expect("the client parses it");
        assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
        packet
    }

    const WOLF: u64 = 0xF130_0000_0100_0001;

    #[test]
    fn fight_packets_read_back_whole_in_the_client() {
        assert!(matches!(
            parse(&attack_start(7, WOLF)),
            ServerPacket::AttackStart {
                attacker: 7,
                victim: WOLF
            }
        ));
        assert!(matches!(
            parse(&attack_stop(WOLF, 7)),
            ServerPacket::AttackStop {
                attacker: WOLF,
                victim: 7
            }
        ));
        assert!(matches!(
            parse(&cant_attack()),
            ServerPacket::AttackSwingError(_)
        ));
        let ServerPacket::AttackerState(s) = parse(&swing(7, WOLF, 9)) else {
            panic!("not a swing");
        };
        assert_eq!(
            (s.attacker, s.victim, s.damage, s.victim_state),
            (7, WOLF, 9, 1)
        );
        assert!(s.displayed());
        let ServerPacket::SpellDamageLog(d) = parse(&spell_damage(WOLF, 7, 133, 18, 2)) else {
            panic!("not a damage log");
        };
        assert_eq!(
            (d.target, d.attacker, d.spell_id, d.damage, d.school),
            (WOLF, 7, 133, 18, 2)
        );
        let ServerPacket::UpdateObject { objects } = parse(&health(WOLF, 22, 40)) else {
            panic!("not an update");
        };
        let [Object::Values { guid, mask }] = objects.as_slice() else {
            panic!("one values block");
        };
        assert_eq!(*guid, WOLF);
        assert_eq!(
            (mask.unit_health(), mask.unit_max_health()),
            (Some(22), Some(40))
        );
    }
}
