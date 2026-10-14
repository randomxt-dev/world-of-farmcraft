//! The match's gameobjects: each task node's create and its template, as the client's
//! `update_object` and `gameobject` readers take them.

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::update::{self, Fields};
use super::wire::Body;
use crate::look::{NodeKind, NodeLook};

// Descriptor indices for build 5875 (vmangos `UpdateFields_1_12_1.h`).
const OBJECT_FIELD_GUID: u16 = 0;
const OBJECT_FIELD_TYPE: u16 = 2;
const OBJECT_FIELD_ENTRY: u16 = 3;
const OBJECT_FIELD_SCALE_X: u16 = 4;
const GAMEOBJECT_DISPLAYID: u16 = 8;
const GAMEOBJECT_ROTATION: u16 = 10;
const GAMEOBJECT_STATE: u16 = 14;
const GAMEOBJECT_POS_X: u16 = 15;
const GAMEOBJECT_FACING: u16 = 18;
const GAMEOBJECT_TYPE_ID: u16 = 21;

/// `TYPEMASK_OBJECT | TYPEMASK_GAMEOBJECT`.
const TYPE_GAMEOBJECT: u32 = 0x01 | 0x20;
/// `GO_STATE_READY`: closed, so a lock's open action applies.
const GO_STATE_READY: u32 = 1;
/// `GAMEOBJECT_TYPE_CHEST`: herbs and veins, opened through their lock.
const TYPE_CHEST: u32 = 3;
/// `GAMEOBJECT_TYPE_GOOBER`: a lockless station, used with `CMSG_GAMEOBJ_USE`.
const TYPE_GOOBER: u32 = 10;
/// `Lock.dbc` 29: Herbalism (lock type 2), skill 0, which Herb Gathering opens.
const LOCK_HERB: i32 = 29;
/// `Lock.dbc` 43: lock type 13, skill 0, which the "Opening" kneel cast opens.
const LOCK_KNEEL: i32 = 43;

/// The node's `GAMEOBJECT_TYPE` and the lock in its `data[0]`.
fn kind(node: &NodeLook) -> (u32, i32) {
    match node.kind {
        NodeKind::Herb => (TYPE_CHEST, LOCK_HERB),
        NodeKind::Kneel => (TYPE_CHEST, LOCK_KNEEL),
        NodeKind::Station => (TYPE_GOOBER, 0),
    }
}

/// The `SMSG_UPDATE_OBJECT` that puts `node` (template `entry`) at `at`, facing `facing`.
pub fn create_node(guid: u64, entry: u32, node: &NodeLook, at: [f32; 3], facing: f32) -> Packet {
    let (type_id, _) = kind(node);
    let mut f = Fields::default();
    f.u64(OBJECT_FIELD_GUID, guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_GAMEOBJECT)
        .u32(OBJECT_FIELD_ENTRY, entry)
        .f32(OBJECT_FIELD_SCALE_X, 1.0)
        .u32(GAMEOBJECT_DISPLAYID, node.display)
        // A turn about z: the quaternion (0, 0, sin(f/2), cos(f/2)).
        .f32(GAMEOBJECT_ROTATION + 2, (facing / 2.0).sin())
        .f32(GAMEOBJECT_ROTATION + 3, (facing / 2.0).cos())
        .u32(GAMEOBJECT_STATE, GO_STATE_READY)
        .f32(GAMEOBJECT_POS_X, at[0])
        .f32(GAMEOBJECT_POS_X + 1, at[1])
        .f32(GAMEOBJECT_POS_X + 2, at[2])
        .f32(GAMEOBJECT_FACING, facing)
        .u32(GAMEOBJECT_TYPE_ID, type_id);
    Packet::new(
        opcode::SMSG_UPDATE_OBJECT,
        update::create_gameobject(guid, at, facing, &f),
    )
}

/// `SMSG_GAMEOBJECT_QUERY_RESPONSE`: `entry`'s template (type, display, name, four empty names,
/// 24 data words with the lock in `data[0]`), or the lone `entry | 0x8000_0000` for a miss.
pub fn query_response(entry: u32, node: Option<&NodeLook>) -> Packet {
    let mut b = Body::default();
    match node {
        None => {
            b.u32(entry | 0x8000_0000);
        }
        Some(node) => {
            let (type_id, lock) = kind(node);
            b.u32(entry)
                .u32(type_id)
                .u32(node.display)
                .cstring(&node.name);
            for _ in 0..4 {
                b.cstring("");
            }
            b.u32(lock as u32);
            for _ in 1..24 {
                b.u32(0);
            }
        }
    }
    Packet::new(opcode::SMSG_GAMEOBJECT_QUERY_RESPONSE, b.done())
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

    #[test]
    fn a_node_is_created_where_it_stands_and_its_template_names_its_lock() {
        let herb = NodeLook::herb("Peacebloom", 269);
        let guid = super::super::guid::node(among_rules::TaskId(2), 1, 0);
        let at = [-8939.95, -132.5, 83.5];
        let ServerPacket::UpdateObject { objects } = parse(&create_node(guid, 34, &herb, at, 1.0))
        else {
            panic!("not an update");
        };
        let [Object::Create {
            guid: g,
            movement,
            mask,
            ..
        }] = objects.as_slice()
        else {
            panic!("one create");
        };
        assert_eq!(*g, guid);
        let (pos, facing) = movement.position.expect("placed");
        assert_eq!(([pos.x, pos.y, pos.z], facing), (at, 1.0));
        assert_eq!(mask.gameobject_type_id(), 3);
        assert_eq!(mask.gameobject_flags(), 0, "usable");

        let ServerPacket::GameObjectQueryResponse { entry, info } =
            parse(&query_response(34, Some(&herb)))
        else {
            panic!("not a template");
        };
        let info = info.expect("found");
        assert_eq!((entry, info.type_id, info.display_id), (34, 3, 269));
        assert_eq!(info.name, "Peacebloom");
        assert_eq!(info.data[0], 29, "Herbalism's lock");

        let station = NodeLook::station("Forge", 233, 2657);
        let ServerPacket::GameObjectQueryResponse { info, .. } =
            parse(&query_response(35, Some(&station)))
        else {
            panic!("not a template");
        };
        let info = info.expect("found");
        assert_eq!((info.type_id, info.data[0]), (10, 0), "a lockless goober");

        let ServerPacket::GameObjectQueryResponse { entry, info } =
            parse(&query_response(99, None))
        else {
            panic!("not a template");
        };
        assert_eq!((entry, info), (99, None));
    }
}
