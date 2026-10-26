//! The farm's gameobjects: soil, crops, ore veins, the hay bed and the shipping crate. Each
//! object's create and the template its entry answers, as the client's `update_object` and
//! `gameobject` readers take them.

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::update::{self, Fields};
use super::wire::Body;

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
/// `GAMEOBJECT_TYPE_GOOBER`: used with `CMSG_GAMEOBJ_USE`.
const TYPE_GOOBER: u32 = 10;
/// `GAMEOBJECT_TYPE_GENERIC`: scenery, no right-click.
const TYPE_GENERIC: u32 = 5;
/// `Lock.dbc` 29: Herbalism, skill 0, which Herb Gathering opens.
const LOCK_HERB: u32 = 29;
/// `Lock.dbc` 38: Copper Vein's, Mining, skill 0, which Mining opens with a pick.
const LOCK_MINE: u32 = 38;

/// What a right-click on the object does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// Picked with Herb Gathering.
    Herb,
    /// Mined with Mining and a Mining Pick.
    Mine,
    /// Used: the server decides what happens.
    Goober,
    /// Scenery.
    None,
}

/// An object template: its look, name and use.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    /// `GameObjectDisplayInfo.dbc` id.
    pub display: u32,
    pub use_: Use,
}

impl Template {
    pub fn new(name: impl Into<String>, display: u32, use_: Use) -> Self {
        Self {
            name: name.into(),
            display,
            use_,
        }
    }

    /// The `GAMEOBJECT_TYPE` and the lock in its `data[0]`.
    fn kind(&self) -> (u32, u32) {
        match self.use_ {
            Use::Herb => (TYPE_CHEST, LOCK_HERB),
            Use::Mine => (TYPE_CHEST, LOCK_MINE),
            Use::Goober => (TYPE_GOOBER, 0),
            Use::None => (TYPE_GENERIC, 0),
        }
    }
}

/// The `SMSG_UPDATE_OBJECT` that puts template `entry` at `at`, facing `facing`, `scale` times
/// its size.
pub fn create(
    guid: u64,
    entry: u32,
    t: &Template,
    at: [f32; 3],
    facing: f32,
    scale: f32,
) -> Packet {
    let (type_id, _) = t.kind();
    let mut f = Fields::default();
    f.u64(OBJECT_FIELD_GUID, guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_GAMEOBJECT)
        .u32(OBJECT_FIELD_ENTRY, entry)
        .f32(OBJECT_FIELD_SCALE_X, scale)
        .u32(GAMEOBJECT_DISPLAYID, t.display)
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
pub fn query_response(entry: u32, t: Option<&Template>) -> Packet {
    let mut b = Body::default();
    match t {
        None => {
            b.u32(entry | 0x8000_0000);
        }
        Some(t) => {
            let (type_id, lock) = t.kind();
            b.u32(entry).u32(type_id).u32(t.display).cstring(&t.name);
            for _ in 0..4 {
                b.cstring("");
            }
            b.u32(lock);
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
    fn an_object_stands_where_placed_at_its_scale_and_its_template_names_its_lock() {
        let herb = Template::new("Peacebloom", 269, Use::Herb);
        let at = [-9915.0, 360.0, 35.0];
        let ServerPacket::UpdateObject { objects } =
            parse(&create(0xF110_0000_0000_0001, 900, &herb, at, 1.0, 0.5))
        else {
            panic!("not an update");
        };
        let [Object::Create { movement, mask, .. }] = objects.as_slice() else {
            panic!("one create");
        };
        let (pos, _) = movement.position.expect("placed");
        assert_eq!([pos.x, pos.y, pos.z], at);
        assert_eq!(mask.object_scale_x(), Some(0.5));
        assert_eq!(mask.gameobject_type_id(), 3);

        for (t, type_id, lock) in [
            (herb, 3, 29),
            (Template::new("Copper Vein", 310, Use::Mine), 3, 38),
            (Template::new("Tilled Soil", 20, Use::Goober), 10, 0),
            (Template::new("Haystack", 251, Use::None), 5, 0),
        ] {
            let ServerPacket::GameObjectQueryResponse { info, .. } =
                parse(&query_response(900, Some(&t)))
            else {
                panic!("not a template");
            };
            let info = info.expect("found");
            assert_eq!((info.type_id, info.data[0]), (type_id, lock), "{}", t.name);
            assert_eq!(info.name, t.name);
        }
        let ServerPacket::GameObjectQueryResponse { info, .. } = parse(&query_response(5, None))
        else {
            panic!("not a template");
        };
        assert_eq!(info, None);
    }
}
