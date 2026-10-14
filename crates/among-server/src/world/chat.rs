//! Player chat, relayed as a 1.12 server does (vmangos `ChatHandler`, `listen_range_*`) within the
//! match's rules: `/say` and `/emote` reach 25 yards at any time, party (and raid) the whole lobby
//! but only before a game and at meetings, and yells and whispers are off, so talk across the map
//! waits for a meeting as in Among Us. Lines go out in the universal language, so no race's speech
//! garbles another's; the dead are heard only by the dead.

use benilla_protocol::messages::{
    opcode, CHAT_MSG_EMOTE, CHAT_MSG_PARTY, CHAT_MSG_RAID, CHAT_MSG_SAY, CHAT_MSG_WHISPER,
    CHAT_MSG_YELL,
};

use super::map::Map;
use super::packets::{self, ClientChat};
use super::socket::Packet;
use super::wire::Body;

/// vmangos `ListenRange.Say` and `ListenRange.TextEmote`.
pub const SAY_RANGE: f32 = 25.0;
/// `LANG_UNIVERSAL`: understood by every race.
const LANG_UNIVERSAL: u32 = 0;

/// Where a line goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Route {
    /// Everyone within this many yards.
    Near(f32),
    /// Everyone in the world.
    Everyone,
    /// Nobody: the speaker is told why.
    Refused(&'static str),
}

/// The match's chat rules for a line of `kind`: say and emote nearby, party only while
/// `party_open` (the lobby and meetings), yell and whisper never.
pub fn route(kind: u8, party_open: bool) -> Route {
    match kind {
        CHAT_MSG_SAY | CHAT_MSG_EMOTE => Route::Near(SAY_RANGE),
        // Everyone in the lobby is one party.
        CHAT_MSG_PARTY | CHAT_MSG_RAID if party_open => Route::Everyone,
        CHAT_MSG_PARTY | CHAT_MSG_RAID => {
            Route::Refused("Party chat opens in the lobby and at meetings.")
        }
        CHAT_MSG_YELL => Route::Refused("There is no yelling in Among Azeroth."),
        CHAT_MSG_WHISPER => Route::Refused("There are no whispers in Among Azeroth."),
        _ => Route::Refused("Only say, emote and party chat work here."),
    }
}

/// Relay `chat`, which `speaker` typed, by [`route`].
pub fn relay(map: &Map, speaker: u64, chat: &ClientChat, party_open: bool) {
    let Ok(kind) = u8::try_from(chat.chat_type) else {
        return;
    };
    let line = || message(kind, speaker, &chat.text);
    match route(kind, party_open) {
        Route::Near(range) => map.say(speaker, Some(range), &line()),
        Route::Everyone => map.say(speaker, None, &line()),
        Route::Refused(why) => map.send(speaker, packets::system_message(why)),
    }
}

/// `SMSG_MESSAGECHAT` from player `sender`: say, party and yell name the sender twice, every other
/// type once (vmangos `BuildChatPacket`).
pub fn message(kind: u8, sender: u64, text: &str) -> Packet {
    let mut b = Body::default();
    b.u8(kind).u32(LANG_UNIVERSAL).u64(sender);
    if matches!(kind, CHAT_MSG_SAY | CHAT_MSG_PARTY | CHAT_MSG_YELL) {
        b.u64(sender);
    }
    b.u32(text.len() as u32 + 1).cstring(text).u8(0);
    Packet::new(opcode::SMSG_MESSAGECHAT, b.done())
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
        for kind in [CHAT_MSG_SAY, CHAT_MSG_PARTY, CHAT_MSG_RAID, CHAT_MSG_EMOTE] {
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
    }

    #[test]
    fn say_is_always_open_party_only_at_the_lobby_and_meetings_and_yell_and_whisper_never() {
        for open in [true, false] {
            assert_eq!(route(CHAT_MSG_SAY, open), Route::Near(SAY_RANGE));
            assert_eq!(route(CHAT_MSG_EMOTE, open), Route::Near(SAY_RANGE));
            assert!(matches!(route(CHAT_MSG_YELL, open), Route::Refused(_)));
            assert!(matches!(route(CHAT_MSG_WHISPER, open), Route::Refused(_)));
        }
        assert_eq!(route(CHAT_MSG_PARTY, true), Route::Everyone);
        assert_eq!(route(CHAT_MSG_RAID, true), Route::Everyone);
        assert!(matches!(route(CHAT_MSG_PARTY, false), Route::Refused(_)));
    }
}
