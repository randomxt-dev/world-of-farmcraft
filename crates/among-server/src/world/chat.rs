//! Player chat relayed as a 1.12 server does (vmangos `ChatHandler`, `listen_range_*`): `/say` and
//! `/emote` reach 25 yards, `/yell` 300, a whisper its one named player (and echoes to the
//! sender), party and raid the whole lobby. Lines go out in the universal language, so no race's
//! speech garbles another's; the dead are heard only by the dead.

use benilla_protocol::messages::{
    opcode, CHAT_MSG_EMOTE, CHAT_MSG_PARTY, CHAT_MSG_RAID, CHAT_MSG_SAY, CHAT_MSG_WHISPER,
    CHAT_MSG_WHISPER_INFORM, CHAT_MSG_YELL,
};

use super::map::Map;
use super::packets::{self, ClientChat};
use super::socket::Packet;
use super::wire::Body;

/// vmangos `ListenRange.Say` and `ListenRange.TextEmote`.
pub const SAY_RANGE: f32 = 25.0;
/// vmangos `ListenRange.Yell`.
pub const YELL_RANGE: f32 = 300.0;
/// `LANG_UNIVERSAL`: understood by every race.
const LANG_UNIVERSAL: u32 = 0;

/// Relay `chat`, which `speaker` typed; a line type the match has no use for is answered with a
/// system line.
pub fn relay(map: &Map, speaker: u64, chat: &ClientChat) {
    let Ok(kind) = u8::try_from(chat.chat_type) else {
        return;
    };
    let text = chat.text.as_str();
    match kind {
        CHAT_MSG_SAY | CHAT_MSG_EMOTE => {
            map.say(speaker, Some(SAY_RANGE), &message(kind, speaker, text))
        }
        CHAT_MSG_YELL => map.say(speaker, Some(YELL_RANGE), &message(kind, speaker, text)),
        // Everyone in the lobby is one party.
        CHAT_MSG_PARTY | CHAT_MSG_RAID => map.say(speaker, None, &message(kind, speaker, text)),
        CHAT_MSG_WHISPER => whisper(map, speaker, chat.target.as_deref().unwrap_or(""), text),
        _ => map.send(
            speaker,
            packets::system_message("Only say, yell, emote, whisper and party chat work here."),
        ),
    }
}

fn whisper(map: &Map, speaker: u64, to: &str, text: &str) {
    let Some(listener) = map.find(to) else {
        map.send(speaker, player_not_found(to));
        return;
    };
    if !map.hears(listener, speaker) {
        map.send(
            speaker,
            packets::system_message("The living cannot hear the dead."),
        );
        return;
    }
    map.send(listener, message(CHAT_MSG_WHISPER, speaker, text));
    map.send(speaker, message(CHAT_MSG_WHISPER_INFORM, listener, text));
}

/// `SMSG_MESSAGECHAT` from player `sender`: say, party and yell name the sender twice, every other
/// type once (vmangos `BuildChatPacket`); a whisper's echo names the addressee instead.
pub fn message(kind: u8, sender: u64, text: &str) -> Packet {
    let mut b = Body::default();
    b.u8(kind).u32(LANG_UNIVERSAL).u64(sender);
    if matches!(kind, CHAT_MSG_SAY | CHAT_MSG_PARTY | CHAT_MSG_YELL) {
        b.u64(sender);
    }
    b.u32(text.len() as u32 + 1).cstring(text).u8(0);
    Packet::new(opcode::SMSG_MESSAGECHAT, b.done())
}

/// `SMSG_CHAT_PLAYER_NOT_FOUND`: the name as typed.
fn player_not_found(name: &str) -> Packet {
    Packet::new(
        opcode::SMSG_CHAT_PLAYER_NOT_FOUND,
        Body::default().cstring(name).done(),
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
    fn every_relayed_shape_reads_back_in_the_client() {
        for kind in [
            CHAT_MSG_SAY,
            CHAT_MSG_PARTY,
            CHAT_MSG_RAID,
            CHAT_MSG_YELL,
            CHAT_MSG_WHISPER,
            CHAT_MSG_WHISPER_INFORM,
            CHAT_MSG_EMOTE,
        ] {
            let ServerPacket::MessageChat(m) = parse(&message(kind, 7, "hello there")) else {
                panic!("not chat");
            };
            assert_eq!(
                (m.chat_type, m.language, m.sender_guid),
                (kind, 0, 7),
                "{kind:#x}"
            );
            assert_eq!(m.text, "hello there");
        }
        assert!(matches!(
            parse(&player_not_found("Thrall")),
            ServerPacket::ChatPlayerNotFound { name } if name == "Thrall"
        ));
    }
}
