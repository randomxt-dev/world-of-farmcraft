//! Death on the wire, as vmangos sends it: the root at death, the unroot and water walk at release,
//! the ghost's speeds, the corpse left where the player fell, and the corpse query's answer.

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::update::{self, Fields, BASE_SPEEDS};
use super::wire::Body;
use crate::accounts::CharacterRecord;

// Descriptor indices for build 5875 (vmangos `UpdateFields_1_12_1.h`).
const OBJECT_FIELD_GUID: u16 = 0;
const OBJECT_FIELD_TYPE: u16 = 2;
const OBJECT_FIELD_SCALE_X: u16 = 4;
const CORPSE_FIELD_OWNER: u16 = 6;
const CORPSE_FIELD_FACING: u16 = 8;
const CORPSE_FIELD_POS_X: u16 = 9;
const CORPSE_FIELD_DISPLAY_ID: u16 = 12;
const CORPSE_FIELD_BYTES_1: u16 = 32;
const CORPSE_FIELD_BYTES_2: u16 = 33;
const CORPSE_FIELD_FLAGS: u16 = 35;

/// `TYPEMASK_OBJECT | TYPEMASK_CORPSE`.
const TYPE_CORPSE: u32 = 0x01 | 0x80;
/// `CORPSE_FLAG_UNK2`, which vmangos's `Player::CreateCorpse` sets on every fresh corpse.
const CORPSE_FLAG_UNK2: u32 = 0x04;

/// Spell 8326 Ghost's speeds (`Spell.dbc`: +25% run, +25% swim): `[run, swim]` in yd/s.
pub const GHOST_SPEEDS: [f32; 2] = [BASE_SPEEDS[1] * 1.25, BASE_SPEEDS[3] * 1.25];
/// The living's `[run, swim]`.
pub const LIVING_SPEEDS: [f32; 2] = [BASE_SPEEDS[1], BASE_SPEEDS[3]];

/// An acked movement change (`SMSG_FORCE_MOVE_ROOT` and kin): packed guid, then the counter.
fn move_mode(op: u16, guid: u64, counter: u32) -> Packet {
    Packet::new(op, Body::default().packed_guid(guid).u32(counter).done())
}

/// `SMSG_FORCE_MOVE_ROOT`, sent at death (vmangos `Unit::SetDeathState`).
pub fn root(guid: u64, counter: u32) -> Packet {
    move_mode(opcode::SMSG_FORCE_MOVE_ROOT, guid, counter)
}

/// `SMSG_FORCE_MOVE_UNROOT`, sent at release (vmangos `BuildPlayerRepop`).
pub fn unroot(guid: u64, counter: u32) -> Packet {
    move_mode(opcode::SMSG_FORCE_MOVE_UNROOT, guid, counter)
}

/// `SMSG_MOVE_WATER_WALK` (a ghost's, vmangos `ApplyGhostForm`) or `SMSG_MOVE_LAND_WALK`.
pub fn water_walk(guid: u64, counter: u32, on: bool) -> Packet {
    let op = if on {
        opcode::SMSG_MOVE_WATER_WALK
    } else {
        opcode::SMSG_MOVE_LAND_WALK
    };
    move_mode(op, guid, counter)
}

/// `SMSG_FORCE_SWIM_SPEED_CHANGE`: packed guid, counter, speed, as the run change is.
pub fn force_swim_speed(guid: u64, counter: u32, speed: f32) -> Packet {
    Packet::new(
        opcode::SMSG_FORCE_SWIM_SPEED_CHANGE,
        Body::default()
            .packed_guid(guid)
            .u32(counter)
            .f32(speed)
            .done(),
    )
}

/// `MSG_CORPSE_QUERY`'s not-found, a lone `u8(0)`: a ghost here never runs back to its corpse,
/// so the client draws no marker and never offers Resurrect.
pub fn corpse_not_found() -> Packet {
    Packet::new(opcode::MSG_CORPSE_QUERY, vec![0])
}

/// The corpse `c` leaves at `at` facing `facing`, as vmangos's `Player::CreateCorpse` dresses it:
/// the race's body with the character's own face and hair, no gear.
pub fn create_corpse(guid: u64, c: &CharacterRecord, at: [f32; 3], facing: f32) -> Packet {
    let mut f = Fields::default();
    f.u64(OBJECT_FIELD_GUID, guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_CORPSE)
        .f32(OBJECT_FIELD_SCALE_X, 1.0)
        .u64(CORPSE_FIELD_OWNER, c.guid)
        .f32(CORPSE_FIELD_FACING, facing)
        .f32(CORPSE_FIELD_POS_X, at[0])
        .f32(CORPSE_FIELD_POS_X + 1, at[1])
        .f32(CORPSE_FIELD_POS_X + 2, at[2])
        .u32(
            CORPSE_FIELD_DISPLAY_ID,
            super::player::display_id(c.race, c.gender),
        )
        .u32(
            CORPSE_FIELD_BYTES_1,
            u32::from(c.race) << 8 | u32::from(c.gender) << 16 | u32::from(c.skin) << 24,
        )
        .u32(
            CORPSE_FIELD_BYTES_2,
            u32::from(c.face)
                | u32::from(c.hair_style) << 8
                | u32::from(c.hair_color) << 16
                | u32::from(c.facial_hair) << 24,
        )
        .u32(CORPSE_FIELD_FLAGS, CORPSE_FLAG_UNK2);
    Packet::new(
        opcode::SMSG_UPDATE_OBJECT,
        update::create_corpse(guid, at, facing, &f),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, MoveMode, ServerPacket};
    use benilla_protocol::{decode, EntityKind, SessionEvent};

    fn parse(p: &Packet) -> ServerPacket {
        let (packet, tail) =
            parse_server_with_tail(p.opcode, &p.body).expect("the client parses it");
        assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
        packet
    }

    fn character() -> CharacterRecord {
        CharacterRecord {
            guid: 7,
            name: "Anna".into(),
            race: 4,
            class: 1,
            gender: 1,
            skin: 2,
            face: 3,
            hair_style: 4,
            hair_color: 5,
            facial_hair: 6,
            level: 1,
        }
    }

    #[test]
    fn a_corpse_is_a_drawn_body_of_its_owner_where_they_fell() {
        let at = [-8950.0, -130.0, 83.5];
        let events = decode(parse(&create_corpse(
            crate::world::guid::corpse(7),
            &character(),
            at,
            1.5,
        )));
        let [SessionEvent::ObjectCreate {
            guid,
            kind,
            position,
            fields,
            ..
        }] = events.as_slice()
        else {
            panic!("one object create, got {events:?}");
        };
        assert_eq!(*guid, crate::world::guid::corpse(7));
        assert_eq!(*kind, EntityKind::Corpse);
        assert_eq!(*position, at);
        assert_eq!(fields.corpse_owner(), Some(7));
        assert_eq!(fields.corpse_display_id(), Some(56), "a night elf female");
        assert_eq!(fields.corpse_race(), 4);
        assert!(!fields.corpse_is_bones(), "a fresh body, not bones");
    }

    #[test]
    fn death_moves_read_back_in_the_client() {
        for (packet, want, apply) in [
            (root(7, 3), MoveMode::Root, true),
            (unroot(7, 4), MoveMode::Root, false),
            (water_walk(7, 5, true), MoveMode::WaterWalk, true),
            (water_walk(7, 6, false), MoveMode::WaterWalk, false),
        ] {
            let ServerPacket::MoveMode {
                guid,
                mode,
                apply: a,
                ..
            } = parse(&packet)
            else {
                panic!("not a move mode");
            };
            assert_eq!((guid, mode, a), (7, want, apply));
        }
        assert!(matches!(
            parse(&force_swim_speed(7, 8, GHOST_SPEEDS[1])),
            ServerPacket::ForceSpeedChange {
                guid: 7,
                counter: 8,
                ..
            }
        ));
        let ServerPacket::CorpseQuery(found) = parse(&corpse_not_found()) else {
            panic!("not a corpse query answer");
        };
        assert!(!found.found);
    }

    #[test]
    fn each_life_reads_back_as_the_client_judges_death() {
        use crate::world::player::{create_packet, spawn_living, Life};
        let fields = |life| {
            let events = decode(parse(&create_packet(
                &character(),
                &spawn_living(),
                false,
                life,
            )));
            let [SessionEvent::ObjectCreate { fields, .. }] = events.as_slice() else {
                panic!("one object create, got {events:?}");
            };
            fields.clone()
        };
        let alive = fields(Life::Alive);
        assert!(!alive.is_dead_or_ghost());

        let dead = fields(Life::Dead);
        assert!(dead.unit_is_dead() && !dead.player_is_ghost());
        assert!(
            dead.player_release_timer_running(),
            "Release Spirit counts down"
        );

        let ghost = fields(Life::Ghost);
        assert!(ghost.player_is_ghost() && !ghost.unit_is_dead());
        assert_eq!(ghost.unit_health(), Some(1));
        assert!(ghost.unit_is_ghost_visual());
        let auras: Vec<_> = ghost.unit_auras().collect();
        assert_eq!(auras.len(), 1, "{auras:?}");
        assert_eq!((auras[0].slot, auras[0].spell_id), (32, 8326), "a debuff");

        // A values update back to life clears every ghost field it set.
        let ServerPacket::UpdateObject { objects } =
            parse(&crate::world::player::life_change(7, Life::Alive, 120))
        else {
            panic!("not an update");
        };
        let [benilla_protocol::messages::Object::Values { mask, .. }] = objects.as_slice() else {
            panic!("one values block");
        };
        assert_eq!(mask.unit_health(), Some(120));
        assert!(!mask.player_is_ghost() && !mask.unit_is_ghost_visual());
        assert_eq!(mask.unit_auras().count(), 0);
    }
}
