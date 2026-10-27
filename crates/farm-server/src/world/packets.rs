//! Server packet builders. Each layout is the one `benilla-protocol`'s `parse_server` reads, and
//! the tests parse every builder's bytes back through it.

use benilla_protocol::messages::{opcode, AUTH_OK, CHAT_MSG_SYSTEM, LANGUAGE_ADDON};
use benilla_protocol::wire::{read_cstring, read_u32_le};

use super::socket::Packet;
use super::wire::Body;
use farm_rules::save::Character;

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

/// `SMSG_CHAR_ENUM`: the roster, each character standing at `location` in the farmer's outfit,
/// each equipment slot a display id and inventory type (vmangos `Player::BuildEnumData`).
pub fn char_enum(characters: &[Character], location: Location) -> Packet {
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
                1, // level
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
        for slot in 0..super::items::EQUIPMENT_SLOTS {
            let worn = (super::items::OUTFIT.iter())
                .find(|(s, _)| *s == slot)
                .and_then(|&(_, entry)| super::items::template(entry));
            match worn {
                Some(t) => b.u32(t.display_id).u8(t.inventory_type),
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

/// `SMSG_LOGIN_SETTIMESPEED`: the game clock at `hour:minute` on day `day` of the farm, running
/// `speed` game minutes per real second. The date packs minute:6, hour:5, weekday:3, day-1:6,
/// month-1:4, year-2000:5; the farm's day picks the date so the moon turns with it.
pub fn time_speed(hour: u32, minute: u32, day: u32, speed: f32) -> Packet {
    let d = day.saturating_sub(1);
    let (day_of_month, month, year) = (d % 28 + 1, (d / 28) % 12 + 1, 2026 + d / (28 * 12));
    let packed = minute.min(59)
        | hour.min(23) << 6
        | (d % 7) << 11
        | (day_of_month - 1) << 14
        | (month - 1) << 20
        | (year - 2000).min(31) << 24;
    Packet::new(
        opcode::SMSG_LOGIN_SETTIMESPEED,
        Body::default().u32(packed).f32(speed).done(),
    )
}

/// `SMSG_WEATHER`: `u32` type (0 fine, 1 rain, 2 snow), `f32` grade, the ambience sound
/// (vmangos `WeatherSounds`: 8534 medium rain, 8537 medium snow, 0 none), and a ramp, not
/// instant.
pub fn weather(kind: u32, grade: f32, instant: bool) -> Packet {
    let sound = match kind {
        1 => 8534,
        2 => 8537,
        _ => 0,
    };
    Packet::new(
        opcode::SMSG_WEATHER,
        Body::default()
            .u32(kind)
            .f32(grade)
            .u32(sound)
            .u8(u8::from(instant))
            .done(),
    )
}

/// `SMSG_GOSSIP_MESSAGE`: `speaker`'s menu, greeting text `text_id`, chat-bubble options in
/// order, no quests.
pub fn gossip_menu(speaker: u64, text_id: u32, options: &[String]) -> Packet {
    let mut b = Body::default();
    b.u64(speaker).u32(text_id).u32(options.len() as u32);
    for (i, text) in options.iter().enumerate() {
        b.u32(i as u32).u8(0).u8(0).cstring(text);
    }
    b.u32(0);
    Packet::new(opcode::SMSG_GOSSIP_MESSAGE, b.done())
}

/// `SMSG_GOSSIP_COMPLETE`: the gossip window closes.
pub fn gossip_complete() -> Packet {
    Packet::new(opcode::SMSG_GOSSIP_COMPLETE, Vec::new())
}

/// `SMSG_NPC_TEXT_UPDATE`: text `id` is `text`, the first of eight weighted lines.
pub fn npc_text(id: u32, text: &str) -> Packet {
    let mut b = Body::default();
    b.u32(id);
    for i in 0..8 {
        let (weight, line) = if i == 0 { (1.0, text) } else { (0.0, "") };
        b.f32(weight).cstring(line).cstring(line).u32(0);
        for _ in 0..3 {
            b.u32(0).u32(0);
        }
    }
    Packet::new(opcode::SMSG_NPC_TEXT_UPDATE, b.done())
}

/// One merchant row: the item entry, its look, the price in copper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ware {
    pub entry: u32,
    pub display: u32,
    pub price: u32,
}

/// `SMSG_LIST_INVENTORY`: `vendor`'s wares, unlimited, one per purchase; an empty list carries a
/// trailing 0 (vmangos `ItemHandler.cpp:728-733`).
pub fn list_inventory(vendor: u64, wares: &[Ware]) -> Packet {
    let mut b = Body::default();
    b.u64(vendor).u8(wares.len() as u8);
    if wares.is_empty() {
        b.u8(0);
    }
    for (i, w) in wares.iter().enumerate() {
        b.u32(i as u32 + 1)
            .u32(w.entry)
            .u32(w.display)
            .u32(u32::MAX) // unlimited
            .u32(w.price)
            .u32(0) // durability
            .u32(1); // one per purchase
    }
    Packet::new(opcode::SMSG_LIST_INVENTORY, b.done())
}

/// `SMSG_BUY_ITEM`: the row bought from `vendor`, unlimited stock, `count` bought.
pub fn buy_item(vendor: u64, row: u32, count: u32) -> Packet {
    Packet::new(
        opcode::SMSG_BUY_ITEM,
        Body::default()
            .u64(vendor)
            .u32(row)
            .u32(u32::MAX)
            .u32(count)
            .done(),
    )
}

/// `SMSG_BUY_FAILED`: `vendor`, the entry, the `BuyResult` reason.
pub fn buy_failed(vendor: u64, entry: u32, reason: u8) -> Packet {
    Packet::new(
        opcode::SMSG_BUY_FAILED,
        Body::default().u64(vendor).u32(entry).u8(reason).done(),
    )
}

/// `SMSG_SELL_ITEM`: a refused sale of `item` to `vendor` for the `SellResult` reason.
pub fn sell_failed(vendor: u64, item: u64, reason: u8) -> Packet {
    Packet::new(
        opcode::SMSG_SELL_ITEM,
        Body::default().u64(vendor).u64(item).u8(reason).done(),
    )
}

/// `MSG_MOVE_TELEPORT_ACK` to the teleported player: packed guid, counter, then a `MovementInfo`
/// standing at `at` facing `facing` (flags, time, x, y, z, o, fall time).
pub fn teleport(guid: u64, counter: u32, at: [f32; 3], facing: f32) -> Packet {
    let mut b = Body::default();
    b.packed_guid(guid)
        .u32(counter)
        .u32(0)
        .u32(0)
        .f32(at[0])
        .f32(at[1])
        .f32(at[2])
        .f32(facing)
        .u32(0);
    Packet::new(opcode::MSG_MOVE_TELEPORT_ACK, b.done())
}

/// `SMSG_MESSAGECHAT` as a raid warning from nobody: the big mid-screen line.
pub fn headline(text: &str) -> Packet {
    Packet::new(
        opcode::SMSG_MESSAGECHAT,
        Body::default()
            .u8(benilla_protocol::messages::CHAT_MSG_RAID_WARNING)
            .u32(0)
            .u64(0)
            .u32(text.len() as u32 + 1)
            .cstring(text)
            .u8(0)
            .done(),
    )
}

/// `SMSG_PLAY_SOUND`: `SoundEntries.dbc` row `sound`, played flat for the receiver.
pub fn play_sound(sound: u32) -> Packet {
    Packet::new(opcode::SMSG_PLAY_SOUND, Body::default().u32(sound).done())
}

/// `SMSG_EMOTE`: `guid` plays `Emotes.dbc` row `emote`'s animation, as everyone nearby sees it.
pub fn emote(guid: u64, emote: u32) -> Packet {
    Packet::new(
        opcode::SMSG_EMOTE,
        Body::default().u32(emote).u64(guid).done(),
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
pub fn name_query_response(c: &Character) -> Packet {
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

/// `SMSG_GOSSIP_POI`: the receiver's one map flag at world `x, y`, drawn on the world map and the
/// minimap, with a gold rim arrow when out of minimap range. Flags 99 and the red flag icon 6 are
/// what vmangos sends; `data` 0 ranks it before every `AreaPOI.dbc` landmark.
pub fn gossip_poi(x: f32, y: f32, name: &str) -> Packet {
    Packet::new(
        opcode::SMSG_GOSSIP_POI,
        Body::default()
            .u32(99)
            .f32(x)
            .f32(y)
            .u32(6)
            .u32(0)
            .cstring(name)
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
    /// The whisper's addressee or the channel's name.
    pub target: Option<String>,
    pub text: String,
}

impl ClientChat {
    pub fn read(body: &[u8]) -> std::io::Result<Self> {
        let mut cur = body;
        let chat_type = read_u32_le(&mut cur)?;
        let language = read_u32_le(&mut cur)?;
        let target = match chat_type {
            0x6 | 0xE => Some(read_cstring(&mut cur)?),
            _ => None,
        };
        let text = read_cstring(&mut cur)?;
        Ok(Self {
            chat_type,
            language,
            target,
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

    fn character() -> Character {
        Character {
            guid: 7,
            name: "Hoggle".into(),
            race: 1,
            class: 1,
            gender: 1,
            skin: 2,
            face: 3,
            hair_style: 4,
            hair_color: 5,
            facial_hair: 6,
        }
    }

    #[test]
    fn every_packet_reads_back_whole_in_the_client() {
        let at = Location {
            map: 0,
            zone: 12,
            position: [-9900.0, 345.0, 35.5],
            orientation: 0.0,
        };
        let ServerPacket::CharEnum { characters } = parse(&char_enum(&[character()], at)) else {
            panic!("not a roster");
        };
        assert_eq!(characters[0].name, "Hoggle");
        let ServerPacket::TimeSpeed { hours, minutes, .. } = parse(&time_speed(18, 45, 40, 1.2))
        else {
            panic!("not the clock");
        };
        assert_eq!((hours, minutes), (18, 45));
        for p in [
            auth_challenge(5),
            auth_ok(),
            login_verify_world(at),
            weather(1, 0.5, false),
            gossip_menu(7, 1, &["Plant Peacebloom Seeds".into()]),
            gossip_complete(),
            npc_text(1, "Howdy."),
            list_inventory(
                9,
                &[Ware {
                    entry: 91_000,
                    display: 20505,
                    price: 10,
                }],
            ),
            list_inventory(9, &[]),
            buy_item(9, 1, 1),
            buy_failed(9, 91_000, 2),
            sell_failed(9, 3, 1),
            pong(3),
            name_query_response(&character()),
            system_message("Good morning!"),
            force_run_speed(7, 1, 7.0),
        ] {
            parse(&p);
        }
    }

    #[test]
    fn client_chat_reads_what_the_client_sends() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x1u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(b".sleep\0");
        let chat = ClientChat::read(&body).unwrap();
        assert_eq!(chat.text, ".sleep");
    }
}
