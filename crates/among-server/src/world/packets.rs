//! Server packet builders. Each layout is the one `benilla-protocol`'s `parse_server` reads, and
//! the tests parse every builder's bytes back through it.

use benilla_protocol::messages::{
    opcode, AUTH_OK, CHAT_MSG_PARTY, CHAT_MSG_SYSTEM, LANGUAGE_ADDON,
};
use benilla_protocol::wire::{read_cstring, read_u32_le};

use super::socket::Packet;
use super::wire::Body;
use crate::accounts::CharacterRecord;

/// Where a character stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub map: u32,
    pub zone: u32,
    pub position: [f32; 3],
    pub orientation: f32,
}

/// `SMSG_AUTH_CHALLENGE`: the server seed, sent before header encryption starts.
pub fn auth_challenge(server_seed: u32) -> Packet {
    Packet::new(
        opcode::SMSG_AUTH_CHALLENGE,
        Body::default().u32(server_seed).done(),
    )
}

/// `SMSG_AUTH_RESPONSE` admitting the session: `AUTH_OK`, then the billing group (no time left,
/// no plan flags, no rested minutes).
pub fn auth_ok() -> Packet {
    Packet::new(
        opcode::SMSG_AUTH_RESPONSE,
        Body::default().u8(AUTH_OK).u32(0).u8(0).u32(0).done(),
    )
}

/// `SMSG_AUTH_RESPONSE` refusing the session with `code`.
pub fn auth_refused(code: u8) -> Packet {
    Packet::new(opcode::SMSG_AUTH_RESPONSE, vec![code])
}

/// `SMSG_CHAR_ENUM`: the roster, each character standing at `location` in its starting gear,
/// each equipment slot a display id and inventory type (vmangos `Player::BuildEnumData`).
pub fn char_enum(characters: &[CharacterRecord], location: Location) -> Packet {
    let mut b = Body::default();
    b.u8(characters.len() as u8);
    for c in characters {
        b.u64(c.guid)
            .cstring(&c.name)
            .bytes(&[
                c.race,
                c.class,
                c.gender,
                c.skin,
                c.face,
                c.hair_style,
                c.hair_color,
                c.facial_hair,
                c.level,
            ])
            .u32(location.zone)
            .u32(location.map)
            .f32(location.position[0])
            .f32(location.position[1])
            .f32(location.position[2])
            .u32(0) // guild
            .u32(0) // character flags
            .u8(0) // first login
            .u32(0) // pet display
            .u32(0) // pet level
            .u32(0); // pet family
        let gear = super::items::starting_gear(c.race, c.class, c.guid);
        for slot in 0..super::items::EQUIPMENT_SLOTS {
            match gear.iter().find(|w| w.slot == slot) {
                Some(w) => b.u32(w.item.display_id).u8(w.item.inventory_type),
                None => b.u32(0).u8(0),
            };
        }
        b.u32(0).u8(0); // first bag
    }
    Packet::new(opcode::SMSG_CHAR_ENUM, b.done())
}

/// `SMSG_CHAR_CREATE`: one result byte.
pub fn char_create_result(code: u8) -> Packet {
    Packet::new(opcode::SMSG_CHAR_CREATE, vec![code])
}

/// `SMSG_CHAR_DELETE`: one result byte.
pub fn char_delete_result(code: u8) -> Packet {
    Packet::new(opcode::SMSG_CHAR_DELETE, vec![code])
}

/// `SMSG_LOGIN_VERIFY_WORLD`: the map and pose the client loads into.
pub fn login_verify_world(at: Location) -> Packet {
    Packet::new(
        opcode::SMSG_LOGIN_VERIFY_WORLD,
        Body::default()
            .u32(at.map)
            .f32(at.position[0])
            .f32(at.position[1])
            .f32(at.position[2])
            .f32(at.orientation)
            .done(),
    )
}

/// `SMSG_LOGIN_SETTIMESPEED`: the game clock, held at noon so every round plays in daylight. The
/// date packs minute:6, hour:5, weekday:3, day-1:6, month-1:4, year-2000:5; the speed is the
/// stock one game minute per real minute.
pub fn login_set_time_speed() -> Packet {
    let (minute, hour, weekday, day, month, year) = (0u32, 12u32, 3u32, 14u32, 10u32, 2026u32);
    let packed = minute
        | hour << 6
        | weekday << 11
        | (day - 1) << 14
        | (month - 1) << 20
        | (year - 2000) << 24;
    Packet::new(
        opcode::SMSG_LOGIN_SETTIMESPEED,
        Body::default().u32(packed).f32(1.0 / 60.0).done(),
    )
}

/// `SMSG_QUERY_TIME_RESPONSE`: wall-clock unix seconds.
pub fn query_time_response(unix_seconds: u32) -> Packet {
    Packet::new(
        opcode::SMSG_QUERY_TIME_RESPONSE,
        Body::default().u32(unix_seconds).done(),
    )
}

/// `SMSG_PONG`: the ping's sequence echoed.
pub fn pong(sequence: u32) -> Packet {
    Packet::new(opcode::SMSG_PONG, Body::default().u32(sequence).done())
}

/// `SMSG_NAME_QUERY_RESPONSE`: guid, name, an empty realm name, then race, gender and class.
pub fn name_query_response(c: &CharacterRecord) -> Packet {
    Packet::new(
        opcode::SMSG_NAME_QUERY_RESPONSE,
        Body::default()
            .u64(c.guid)
            .cstring(&c.name)
            .cstring("")
            .u32(c.race.into())
            .u32(c.gender.into())
            .u32(c.class.into())
            .done(),
    )
}

/// `SMSG_MESSAGECHAT` on the party lane in `LANG_ADDON`, text `prefix`, a TAB, `payload`. The party
/// shape names the sender twice (vmangos `BuildChatPacket`); the client holds an addon line until
/// the sender's name resolves, so `sender` must be a guid the name query answers.
pub fn addon_message(sender: u64, prefix: &str, payload: &str) -> Packet {
    let text = format!("{prefix}\t{payload}");
    Packet::new(
        opcode::SMSG_MESSAGECHAT,
        Body::default()
            .u8(CHAT_MSG_PARTY)
            .u32(LANGUAGE_ADDON)
            .u64(sender)
            .u64(sender)
            .u32(text.len() as u32 + 1)
            .cstring(&text)
            .u8(0)
            .done(),
    )
}

/// `SMSG_MESSAGECHAT` as a system line (`CHAT_MSG_SYSTEM`, the shape GM command replies take):
/// one sender guid (0), universal language, the text.
pub fn system_message(text: &str) -> Packet {
    Packet::new(
        opcode::SMSG_MESSAGECHAT,
        Body::default()
            .u8(CHAT_MSG_SYSTEM)
            .u32(0)
            .u64(0)
            .u32(text.len() as u32 + 1)
            .cstring(text)
            .u8(0)
            .done(),
    )
}

/// `SMSG_FORCE_RUN_SPEED_CHANGE`: packed guid, the change counter the client's ack echoes, the
/// new run speed in yards per second.
pub fn force_run_speed(guid: u64, counter: u32, speed: f32) -> Packet {
    Packet::new(
        opcode::SMSG_FORCE_RUN_SPEED_CHANGE,
        Body::default()
            .packed_guid(guid)
            .u32(counter)
            .f32(speed)
            .done(),
    )
}

/// A client's `CMSG_MESSAGECHAT`: `u32` type and language, the target or channel name for a
/// whisper (6) or channel (14) line, then the text (vmangos `Server/Packets/Chat.cpp:3-12`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientChat {
    pub chat_type: u32,
    pub language: u32,
    pub text: String,
}

impl ClientChat {
    pub fn read(body: &[u8]) -> std::io::Result<Self> {
        let mut cur = body;
        let chat_type = read_u32_le(&mut cur)?;
        let language = read_u32_le(&mut cur)?;
        if matches!(chat_type, 0x6 | 0xE) {
            read_cstring(&mut cur)?;
        }
        let text = read_cstring(&mut cur)?;
        Ok(Self {
            chat_type,
            language,
            text,
        })
    }

    /// The payload of an addon line under `prefix`, if this is one.
    pub fn addon_payload(&self, prefix: &str) -> Option<&str> {
        let (p, payload) = self.text.split_once('\t')?;
        (self.language == LANGUAGE_ADDON && p == prefix).then_some(payload)
    }
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

    fn character() -> CharacterRecord {
        CharacterRecord {
            guid: 7,
            name: "Among1".into(),
            race: 1,
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
    fn every_packet_reads_back_whole_in_the_client() {
        let at = crate::world::player::SPAWN;
        assert!(matches!(
            parse(&auth_challenge(5)),
            ServerPacket::AuthChallenge { server_seed: 5 }
        ));
        assert!(matches!(
            parse(&auth_ok()),
            ServerPacket::AuthResponse {
                result: AUTH_OK,
                ..
            }
        ));
        let ServerPacket::CharEnum { characters } = parse(&char_enum(&[character()], at)) else {
            panic!("not a roster");
        };
        assert_eq!(characters[0].guid, 7);
        assert_eq!(characters[0].name, "Among1");
        assert_eq!(characters[0].map, at.map);
        assert_eq!(
            (characters[0].hair_style, characters[0].facial_hair),
            (4, 6)
        );
        let worn: Vec<(usize, u32, u8)> = (characters[0].equipment.iter().enumerate())
            .filter(|(_, e)| e.display_id != 0)
            .map(|(slot, e)| (slot, e.display_id, e.inventory_type))
            .collect();
        assert_eq!(
            worn,
            [
                (3, 9891, 4),
                (6, 9892, 7),
                (7, 10141, 8),
                (15, 1542, 21),
                (16, 18730, 14)
            ],
            "the human warrior's recruit kit"
        );
        let ServerPacket::LoginVerifyWorld { map, position, .. } = parse(&login_verify_world(at))
        else {
            panic!("not a world verify");
        };
        assert_eq!((map, position.x), (at.map, at.position[0]));
        let ServerPacket::TimeSpeed { hours, minutes, .. } = parse(&login_set_time_speed()) else {
            panic!("not a time speed");
        };
        assert_eq!((hours, minutes), (12, 0));
        assert!(matches!(
            parse(&query_time_response(9)),
            ServerPacket::QueryTimeResponse { unix_time: 9 }
        ));
        assert!(matches!(
            parse(&pong(3)),
            ServerPacket::Pong { sequence: 3 }
        ));
        let ServerPacket::NameQueryResponse {
            guid,
            name,
            race,
            gender,
            class,
        } = parse(&name_query_response(&character()))
        else {
            panic!("not a name response");
        };
        assert_eq!(
            (guid, name.as_str(), race, gender, class),
            (7, "Among1", 1, 1, 1)
        );
    }

    #[test]
    fn an_addon_message_reaches_the_client_as_addon_traffic() {
        let ServerPacket::MessageChat(m) = parse(&addon_message(7, "AMONG", "role crew")) else {
            panic!("not a chat line");
        };
        assert!(m.is_addon());
        assert_eq!((m.chat_type, m.sender_guid), (CHAT_MSG_PARTY, 7));
        assert_eq!(m.text, "AMONG\trole crew");
    }

    #[test]
    fn a_speed_change_reads_back_in_the_client() {
        use benilla_protocol::messages::SpeedKind;
        let ServerPacket::ForceSpeedChange {
            guid,
            kind,
            counter,
            speed,
        } = parse(&force_run_speed(7, 3, 21.0))
        else {
            panic!("not a speed change");
        };
        assert_eq!((guid, kind, counter, speed), (7, SpeedKind::Run, 3, 21.0));
    }

    #[test]
    fn a_system_line_reaches_the_client_as_one() {
        let ServerPacket::MessageChat(m) = parse(&system_message("Marked gate")) else {
            panic!("not a chat line");
        };
        assert_eq!((m.chat_type, m.sender_guid), (CHAT_MSG_SYSTEM, 0));
        assert_eq!(m.text, "Marked gate");
        assert!(!m.is_addon());
    }

    /// The client's own builders, read back: an addon line, and a whisper with its target name.
    #[test]
    fn client_chat_reads_what_the_client_sends() {
        use benilla_protocol::messages::{messagechat, messagechat_whisper, CHAT_TYPE_PARTY};
        let addon =
            ClientChat::read(&messagechat(CHAT_TYPE_PARTY, LANGUAGE_ADDON, "AMONG\tkill")).unwrap();
        assert_eq!(addon.addon_payload("AMONG"), Some("kill"));
        assert_eq!(addon.addon_payload("OTHER"), None);
        let whisper = ClientChat::read(&messagechat_whisper(7, "Bob", "hi")).unwrap();
        assert_eq!((whisper.chat_type, whisper.text.as_str()), (6, "hi"));
        assert_eq!(whisper.addon_payload("AMONG"), None, "not addon language");
    }

    /// The self create decodes into a placed, living, displayed player with base speeds.
    #[test]
    fn the_self_create_is_a_living_player_the_client_can_draw() {
        use benilla_protocol::{decode, EntityKind, SessionEvent};
        let p = crate::world::player::create_packet(
            &character(),
            &crate::world::player::spawn_living(),
            true,
            crate::world::player::Life::Alive,
        );
        let events = decode(parse(&p));
        let [worn @ .., SessionEvent::ObjectCreate {
            guid,
            kind,
            display_id,
            position,
            speeds,
            fields,
            ..
        }] = events.as_slice()
        else {
            panic!("the body last, got {events:?}");
        };
        assert_eq!(worn.len(), 5, "the warrior's five worn items first");
        assert!(worn
            .iter()
            .all(|e| matches!(e, SessionEvent::ItemCreate { .. })));
        assert_eq!(*guid, 7);
        assert_eq!(*kind, EntityKind::Player);
        assert_eq!(*display_id, Some(50), "a human female");
        assert_eq!(*position, crate::world::player::SPAWN.position);
        assert!(speeds.is_some());
        assert_eq!(fields.unit_health(), Some(100), "alive");
        assert_eq!(fields.unit_race(), Some(1), "race in bytes 0");
    }

    /// What the client's cast checks read: a full pool, a main hand, FFA PvP and Judgement's
    /// aura state; another client sees no inventory.
    #[test]
    fn a_player_is_created_ready_to_cast_every_button() {
        use benilla_protocol::{decode, SessionEvent};
        let living = crate::world::player::spawn_living();
        let created = |c: &CharacterRecord, is_self| {
            let events = decode(parse(&crate::world::player::create_packet(
                c,
                &living,
                is_self,
                crate::world::player::Life::Alive,
            )));
            let [.., SessionEvent::ObjectCreate { fields, .. }] = events.as_slice() else {
                panic!("the body last, got {events:?}");
            };
            fields.clone()
        };
        for (class, power, pool) in [(1u8, 1u8, 1000u32), (4, 3, 100), (8, 0, 1000), (2, 0, 1000)] {
            let c = CharacterRecord {
                class,
                ..character()
            };
            let me = created(&c, true);
            assert_eq!(me.unit_power_type(), power, "class {class}");
            assert_eq!(
                me.unit_power(power),
                Some(pool),
                "class {class} starts full"
            );
            assert_eq!(me.unit_max_power(power), Some(pool));
            assert_ne!(me.player_inv_slot(15), Some(0), "a main hand");
            assert!(me.player_inv_slot(15).is_some());
            assert_eq!(me.player_flags() & 0x80, 0x80, "FFA PvP");
            assert_eq!(me.unit_aura_state() & 0x10, 0x10, "Judgement usable");
            let them = created(&c, false);
            assert_eq!(them.player_flags() & 0x80, 0x80, "attackable to others");
            assert_eq!(
                them.player_inv_slot(15),
                None,
                "the inventory is the owner's"
            );
        }
    }

    /// Every race and class is created in its starting gear: everyone sees the worn entries, the
    /// owner alone gets the item objects its slots name. A pair 1.12 never allowed wears nothing
    /// but still holds a main hand the client cannot resolve, so Kill stays castable.
    #[test]
    fn every_class_is_created_in_its_starting_gear() {
        use crate::world::items;
        use benilla_protocol::{decode, SessionEvent};
        let living = crate::world::player::spawn_living();
        let created = |c: &CharacterRecord, is_self| {
            decode(parse(&crate::world::player::create_packet(
                c,
                &living,
                is_self,
                crate::world::player::Life::Alive,
            )))
        };
        let mut pairs = 0;
        for race in 1..=8u8 {
            for class in 1..=11u8 {
                let c = CharacterRecord {
                    race,
                    class,
                    ..character()
                };
                let gear = items::starting_gear(race, class, c.guid);
                let events = created(&c, true);
                let [worn @ .., SessionEvent::ObjectCreate { fields: me, .. }] = events.as_slice()
                else {
                    panic!("the body last, got {events:?}");
                };
                assert_eq!(
                    worn.len(),
                    gear.len(),
                    "{race}/{class}: one create per item"
                );
                for (w, e) in gear.iter().zip(worn) {
                    let SessionEvent::ItemCreate { guid, fields, .. } = e else {
                        panic!("{race}/{class}: not an item create: {e:?}");
                    };
                    assert_eq!(*guid, w.guid);
                    assert_eq!(fields.object_entry(), Some(w.item.entry));
                    assert_eq!(fields.item_stack_count(), Some(w.count));
                    assert_eq!(me.player_inv_slot(w.slot), Some(w.guid), "{race}/{class}");
                    assert_eq!(me.player_visible_item_entry(w.slot), Some(w.item.entry));
                }
                let main = me.player_inv_slot(items::MAINHAND).unwrap_or(0);
                assert_ne!(main, 0, "{race}/{class}: a main hand for Kill");
                let events = created(&c, false);
                let [SessionEvent::ObjectCreate { fields: them, .. }] = events.as_slice() else {
                    panic!("only the body for another client, got {events:?}");
                };
                for slot in 0..items::EQUIPMENT_SLOTS {
                    let w = gear.iter().find(|w| w.slot == slot);
                    assert_eq!(
                        them.player_visible_item_entry(slot),
                        w.map(|w| w.item.entry),
                        "{race}/{class} slot {slot}: seen by others"
                    );
                    assert_eq!(
                        them.player_inv_slot(slot),
                        None,
                        "the inventory is the owner's"
                    );
                }
                if !gear.is_empty() {
                    pairs += 1;
                }
            }
        }
        assert_eq!(pairs, 40, "every 1.12 race and class wears its kit");
    }
}
