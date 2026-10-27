//! Quest packet builders: the questgiver's `!`/`?`, the accept, progress and reward panels, the
//! quest template the log reads, and the progress toasts. Each layout is the one
//! `benilla-protocol`'s quest parsers read (vmangos `Quest.cpp`), and the tests parse them back.

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::wire::Body;

/// The farm's quests are all level 1 quests of Elwynn Forest.
const LEVEL: u32 = 1;
const ELWYNN_FOREST: i32 = 12;
/// `QUEST_EMOTE_COUNT`: the details panel always carries four (emote, delay) pairs.
const EMOTES: u32 = 4;
/// A counted objective is shown as a found object, named by its own text: `0x80000000` marks a
/// GameObject, so the client asks for no creature name.
pub const OBJECT_OBJECTIVE: u32 = 0x8000_0000;

/// An item handed over or handed out: its entry, how many, its look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestItem {
    pub entry: u32,
    pub count: u32,
    pub display: u32,
}

/// One objective line: a counted deed (`counted` set, named by `text`) or items to bring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    pub counted: Option<(u32, u32)>,
    pub item: Option<(u32, u32)>,
    pub text: String,
}

/// A quest as the quest log and its query read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestText {
    pub id: u32,
    pub title: String,
    pub details: String,
    pub objectives: String,
    pub money: u32,
    pub next: u32,
    pub goals: Vec<Objective>,
}

/// `SMSG_QUESTGIVER_STATUS`: the marker over `npc`, a `dialog_status` value.
pub fn status(npc: u64, status: u32) -> Packet {
    let mut b = Body::default();
    b.u64(npc).u32(status);
    Packet::new(opcode::SMSG_QUESTGIVER_STATUS, b.done())
}

fn items(b: &mut Body, items: &[QuestItem]) {
    b.u32(items.len() as u32);
    for i in items {
        b.u32(i.entry).u32(i.count).u32(i.display);
    }
}

/// `SMSG_QUESTGIVER_QUEST_DETAILS`: the panel offering quest `q`, with its accept button.
pub fn details(npc: u64, q: &QuestText) -> Packet {
    let mut b = Body::default();
    b.u64(npc)
        .u32(q.id)
        .cstring(&q.title)
        .cstring(&q.details)
        .cstring(&q.objectives)
        .u32(0);
    items(&mut b, &[]);
    items(&mut b, &[]);
    b.u32(q.money).u32(0).u32(EMOTES);
    for _ in 0..EMOTES {
        b.u32(0).u32(0);
    }
    Packet::new(opcode::SMSG_QUESTGIVER_QUEST_DETAILS, b.done())
}

/// `SMSG_QUESTGIVER_REQUEST_ITEMS`: the progress panel, asking for `wanted`; its continue button
/// is live when `complete`.
pub fn request_items(
    npc: u64,
    id: u32,
    title: &str,
    text: &str,
    wanted: &[QuestItem],
    complete: bool,
) -> Packet {
    let mut b = Body::default();
    b.u64(npc).u32(id).cstring(title).cstring(text);
    b.u32(0).u32(0).u32(1).u32(0);
    items(&mut b, wanted);
    b.u32(0x02)
        .u32(if complete { 0x03 } else { 0x00 })
        .u32(0x04)
        .u32(0x08);
    Packet::new(opcode::SMSG_QUESTGIVER_REQUEST_ITEMS, b.done())
}

/// `SMSG_QUESTGIVER_OFFER_REWARD`: the turn-in panel, paying `money`.
pub fn offer_reward(npc: u64, id: u32, title: &str, text: &str, money: u32) -> Packet {
    let mut b = Body::default();
    b.u64(npc).u32(id).cstring(title).cstring(text).u32(0);
    b.u32(0);
    items(&mut b, &[]);
    items(&mut b, &[]);
    b.u32(money).u32(0).u32(0);
    Packet::new(opcode::SMSG_QUESTGIVER_OFFER_REWARD, b.done())
}

/// `SMSG_QUESTGIVER_QUEST_COMPLETE`: quest `id` is turned in for `money`.
pub fn complete(id: u32, money: u32) -> Packet {
    let mut b = Body::default();
    b.u32(id).u32(0x03).u32(0).u32(money).u32(0);
    Packet::new(opcode::SMSG_QUESTGIVER_QUEST_COMPLETE, b.done())
}

/// `SMSG_QUESTUPDATE_ADD_KILL`: the toast for objective `objective` of quest `id` reaching
/// `count` of `required`.
pub fn progress(id: u32, objective: u32, count: u32, required: u32, guid: u64) -> Packet {
    let mut b = Body::default();
    b.u32(id).u32(objective).u32(count).u32(required).u64(guid);
    Packet::new(opcode::SMSG_QUESTUPDATE_ADD_KILL, b.done())
}

/// `SMSG_QUESTUPDATE_COMPLETE`: every objective of quest `id` is done.
pub fn objectives_done(id: u32) -> Packet {
    Packet::new(opcode::SMSG_QUESTUPDATE_COMPLETE, id.to_le_bytes().to_vec())
}

/// `SMSG_QUEST_QUERY_RESPONSE`: the quest's template, which the quest log shows.
pub fn query_response(q: &QuestText) -> Packet {
    let mut b = Body::default();
    b.u32(q.id)
        .u32(2)
        .u32(LEVEL)
        .u32(ELWYNN_FOREST as u32)
        .u32(0);
    b.u32(0).u32(0).u32(0).u32(0);
    b.u32(q.next).u32(q.money).u32(0).u32(0).u32(0).u32(0);
    for _ in 0..4 + 6 {
        b.u32(0).u32(0);
    }
    b.u32(0).f32(0.0).f32(0.0).u32(0);
    b.cstring(&q.title)
        .cstring(&q.objectives)
        .cstring(&q.details)
        .cstring("");
    for i in 0..4 {
        let goal = q.goals.get(i);
        let (who, need) = goal.and_then(|g| g.counted).unwrap_or((0, 0));
        let (item, count) = goal.and_then(|g| g.item).unwrap_or((0, 0));
        b.u32(who).u32(need).u32(item).u32(count);
    }
    for i in 0..4 {
        let text = (q.goals.get(i))
            .filter(|g| g.counted.is_some())
            .map_or("", |g| g.text.as_str());
        b.cstring(text);
    }
    Packet::new(opcode::SMSG_QUEST_QUERY_RESPONSE, b.done())
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};

    fn parse(p: &Packet) -> ServerPacket {
        let (parsed, tail) = parse_server_with_tail(p.opcode, &p.body).unwrap();
        assert_eq!(tail, 0);
        parsed
    }

    fn quest() -> QuestText {
        QuestText {
            id: 900,
            title: "Breaking Ground".into(),
            details: "Plant them.".into(),
            objectives: "Plant 8 seeds.".into(),
            money: 10,
            next: 901,
            goals: vec![Objective {
                counted: Some((OBJECT_OBJECTIVE | 1, 8)),
                item: None,
                text: "Seeds planted".into(),
            }],
        }
    }

    #[test]
    fn every_quest_packet_reads_back_in_the_client() {
        let q = quest();
        match parse(&query_response(&q)) {
            ServerPacket::QuestQueryResponse(t) => {
                assert_eq!(t.title, "Breaking Ground");
                assert_eq!(t.objectives[0].required_count, 8);
                assert_eq!(t.objectives[0].text, "Seeds planted");
                assert_eq!(t.money, 10);
            }
            _ => panic!("another packet"),
        }
        match parse(&details(7, &q)) {
            ServerPacket::QuestGiverDetails(d) => assert_eq!(d.objectives, "Plant 8 seeds."),
            _ => panic!("another packet"),
        }
        let wood = QuestItem {
            entry: 4470,
            count: 4,
            display: 1,
        };
        match parse(&request_items(7, 900, "T", "Got it?", &[wood], true)) {
            ServerPacket::QuestGiverRequestItems(r) => {
                assert!(r.is_complete);
                assert_eq!(r.required_items[0].count, 4);
            }
            _ => panic!("another packet"),
        }
        match parse(&offer_reward(7, 900, "T", "Thanks!", 25)) {
            ServerPacket::QuestGiverOfferReward(r) => assert_eq!(r.money, 25),
            _ => panic!("another packet"),
        }
        match parse(&complete(900, 25)) {
            ServerPacket::QuestGiverComplete(c) => assert_eq!(c.money, 25),
            _ => panic!("another packet"),
        }
        parse(&status(7, 5));
        parse(&progress(900, OBJECT_OBJECTIVE | 1, 3, 8, 1));
        parse(&objectives_done(900));
    }
}
