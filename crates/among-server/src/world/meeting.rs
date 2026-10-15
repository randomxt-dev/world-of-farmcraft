//! A meeting on the wire, in the stock client's own frames: the teleport to the table, the
//! raid-warning headline, a draining countdown bar (the mirror timer), and the vote as a gossip
//! menu the server opens on the voter's own guid.

use std::f32::consts::TAU;

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::wire::Body;

/// `CHAT_MSG_RAID_WARNING`: the stock RaidWarningFrame shows it mid-screen.
const CHAT_MSG_RAID_WARNING: u8 = 0x58;
/// `FEIGNDEATH`, the one mirror timer type whose label is empty with no spell: a plain bar.
const MIRROR_TIMER: u32 = 2;
/// The `npc_text` id the vote menu greets with; the server answers its query itself.
pub const VOTE_TEXT: u32 = 1;
/// The vote menu's greeting, for male and female readers alike.
const VOTE_GREETING: &str = "Who is the impostor? Choose with care: the one most voted is ejected.";
/// The fewest yards from the table's centre a seat stands: clear of the button's model.
const SEAT_RADIUS: f32 = 3.5;
/// Yards of ring per seat, so neighbours stand apart rather than overlap however many sit.
const SEAT_SPACING: f32 = 2.0;

/// How far from the centre a ring of `n` seats stands: [`SEAT_SPACING`] of ring each, at least
/// [`SEAT_RADIUS`].
pub fn seat_radius(n: usize) -> f32 {
    (n as f32 * SEAT_SPACING / TAU).max(SEAT_RADIUS)
}

/// `n` seats in a ring around `centre`, each facing it: `(position, orientation)`.
pub fn seats(centre: [f32; 3], n: usize) -> Vec<([f32; 3], f32)> {
    let radius = seat_radius(n);
    (0..n)
        .map(|i| {
            let angle = TAU * i as f32 / n.max(1) as f32;
            let at = [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
                centre[2],
            ];
            // Facing the centre: the angle back toward it.
            let facing = (angle + std::f32::consts::PI).rem_euclid(TAU);
            (at, facing)
        })
        .collect()
}

/// A `MovementInfo` standing still at `at` facing `facing`: flags, time, x, y, z, o, fall time.
fn standing(b: &mut Body, at: [f32; 3], facing: f32) {
    b.u32(0)
        .u32(0)
        .f32(at[0])
        .f32(at[1])
        .f32(at[2])
        .f32(facing)
        .u32(0);
}

/// `MSG_MOVE_TELEPORT_ACK` to the teleported player: packed guid, counter, where they now stand.
pub fn teleport(guid: u64, counter: u32, at: [f32; 3], facing: f32) -> Packet {
    let mut b = Body::default();
    b.packed_guid(guid).u32(counter);
    standing(&mut b, at, facing);
    Packet::new(opcode::MSG_MOVE_TELEPORT_ACK, b.done())
}

/// `MSG_MOVE_TELEPORT` to everyone else: packed guid and where the player now stands.
pub fn teleported(guid: u64, at: [f32; 3], facing: f32) -> Packet {
    let mut b = Body::default();
    b.packed_guid(guid);
    standing(&mut b, at, facing);
    Packet::new(opcode::MSG_MOVE_TELEPORT, b.done())
}

/// The mid-screen headline: `SMSG_MESSAGECHAT` as a raid warning from nobody.
pub fn headline(text: &str) -> Packet {
    Packet::new(
        opcode::SMSG_MESSAGECHAT,
        Body::default()
            .u8(CHAT_MSG_RAID_WARNING)
            .u32(0)
            .u64(0)
            .u32(text.len() as u32 + 1)
            .cstring(text)
            .u8(0)
            .done(),
    )
}

/// `SMSG_START_MIRROR_TIMER`: a bar draining from `ms` to empty.
pub fn countdown(ms: u32) -> Packet {
    Packet::new(
        opcode::SMSG_START_MIRROR_TIMER,
        Body::default()
            .u32(MIRROR_TIMER)
            .u32(ms)
            .u32(ms)
            .u32(-1i32 as u32)
            .u8(0)
            .u32(0)
            .done(),
    )
}

/// `SMSG_STOP_MIRROR_TIMER`: the bar goes away.
pub fn countdown_stop() -> Packet {
    Packet::new(
        opcode::SMSG_STOP_MIRROR_TIMER,
        Body::default().u32(MIRROR_TIMER).done(),
    )
}

/// `SMSG_GOSSIP_MESSAGE` on `voter`'s own guid: one option per line of `options`, by index.
pub fn ballot(voter: u64, options: &[String]) -> Packet {
    let mut b = Body::default();
    b.u64(voter).u32(VOTE_TEXT).u32(options.len() as u32);
    for (i, text) in options.iter().enumerate() {
        // Icon 0, the chat bubble; never coded.
        b.u32(i as u32).u8(0).u8(0).cstring(text);
    }
    b.u32(0); // no quests
    Packet::new(opcode::SMSG_GOSSIP_MESSAGE, b.done())
}

/// `SMSG_GOSSIP_COMPLETE`: the vote menu closes.
pub fn ballot_closed() -> Packet {
    Packet::new(opcode::SMSG_GOSSIP_COMPLETE, Vec::new())
}

/// `SMSG_NPC_TEXT_UPDATE` for the vote menu's greeting: eight blocks, only the first weighted.
pub fn vote_text() -> Packet {
    let mut b = Body::default();
    b.u32(VOTE_TEXT);
    for i in 0..8 {
        let (weight, text) = if i == 0 {
            (1.0, VOTE_GREETING)
        } else {
            (0.0, "")
        };
        b.f32(weight).cstring(text).cstring(text).u32(0);
        for _ in 0..3 {
            b.u32(0).u32(0);
        }
    }
    Packet::new(opcode::SMSG_NPC_TEXT_UPDATE, b.done())
}

/// A `CMSG_GOSSIP_SELECT_OPTION`: the menu's guid and the chosen option's index.
pub fn read_choice(body: &[u8]) -> Option<(u64, u32)> {
    let guid = u64::from_le_bytes(body.get(..8)?.try_into().ok()?);
    let option = u32::from_le_bytes(body.get(8..12)?.try_into().ok()?);
    Some((guid, option))
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
    fn seats_ring_the_table_facing_it() {
        let centre = [100.0, 50.0, 10.0];
        let ring = seats(centre, 4);
        assert_eq!(ring.len(), 4);
        for (at, facing) in ring {
            let d = ((at[0] - centre[0]).powi(2) + (at[1] - centre[1]).powi(2)).sqrt();
            assert!((d - SEAT_RADIUS).abs() < 1e-3);
            // A step along the facing comes closer to the centre.
            let ahead = [at[0] + facing.cos(), at[1] + facing.sin()];
            let closer = ((ahead[0] - centre[0]).powi(2) + (ahead[1] - centre[1]).powi(2)).sqrt();
            assert!(closer < d);
        }
    }

    #[test]
    fn a_crowded_table_widens_its_ring_to_keep_neighbours_apart() {
        assert_eq!(seat_radius(4), SEAT_RADIUS);
        for n in [10, 15, 30] {
            let ring = seats([0.0; 3], n);
            let (a, b) = (ring[0].0, ring[1].0);
            let gap = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
            assert!(gap >= SEAT_SPACING * 0.95, "{n} seats: {gap} yd apart");
        }
    }

    #[test]
    fn meeting_packets_read_back_in_the_client() {
        let ServerPacket::Teleport {
            guid,
            counter,
            position,
            ..
        } = parse(&teleport(7, 3, [1.0, 2.0, 3.0], 0.5))
        else {
            panic!("not a teleport");
        };
        assert_eq!((guid, counter), (7, 3));
        assert_eq!([position.x, position.y, position.z], [1.0, 2.0, 3.0]);
        let ServerPacket::MessageChat(m) = parse(&headline("Emergency meeting!")) else {
            panic!("not chat");
        };
        assert_eq!((m.chat_type, m.text.as_str()), (0x58, "Emergency meeting!"));
        let ServerPacket::MirrorTimerStart(t) = parse(&countdown(15_000)) else {
            panic!("not a timer");
        };
        assert_eq!(
            (t.kind, t.remaining_ms, t.duration_ms, t.scale),
            (2, 15_000, 15_000, -1)
        );
        assert!(matches!(
            parse(&countdown_stop()),
            ServerPacket::MirrorTimerStop { .. }
        ));
        let ServerPacket::GossipMessage {
            npc,
            text_id,
            options,
            ..
        } = parse(&ballot(7, &["Vote: Ashen".into(), "Skip vote".into()]))
        else {
            panic!("not a gossip menu");
        };
        assert_eq!((npc, text_id, options.len()), (7, VOTE_TEXT, 2));
        assert_eq!(
            (options[1].index, options[1].message.as_str()),
            (1, "Skip vote")
        );
        assert!(matches!(
            parse(&ballot_closed()),
            ServerPacket::GossipComplete
        ));
        let ServerPacket::NpcText { text_id, blocks } = parse(&vote_text()) else {
            panic!("not npc text");
        };
        assert_eq!((text_id, blocks.len()), (VOTE_TEXT, 8));
        assert_eq!(blocks[0].female, VOTE_GREETING);
    }
}
