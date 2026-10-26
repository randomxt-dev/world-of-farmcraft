//! `SMSG_UPDATE_OBJECT` bodies: the create blocks players, creatures and gameobjects are spawned
//! with, and the values update that changes a descriptor, as `benilla-protocol`'s
//! `update_object` reads them.

use std::collections::BTreeMap;

use super::wire::Body;

/// `UPDATETYPE_VALUES`: a descriptor change.
const UPDATETYPE_VALUES: u8 = 0;
/// `UPDATETYPE_CREATE_OBJECT`, the create vmangos sends for an object coming into view.
const UPDATETYPE_CREATE_OBJECT: u8 = 2;
/// `UPDATETYPE_CREATE_OBJECT2`, the create vmangos sends for players.
const UPDATETYPE_CREATE_OBJECT2: u8 = 3;
const TYPEID_ITEM: u8 = 1;
const TYPEID_UNIT: u8 = 3;
const TYPEID_PLAYER: u8 = 4;
const TYPEID_GAMEOBJECT: u8 = 5;
const TYPEID_CORPSE: u8 = 7;

/// The movement block's update flags (vmangos `ObjectDefines.h`).
pub const UPDATEFLAG_SELF: u8 = 0x01;
pub const UPDATEFLAG_ALL: u8 = 0x10;
pub const UPDATEFLAG_LIVING: u8 = 0x20;
pub const UPDATEFLAG_HAS_POSITION: u8 = 0x40;

/// A sparse descriptor: field index to raw `u32` value.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Fields(BTreeMap<u16, u32>);

impl Fields {
    /// Every field of `other` written over these.
    pub fn merge(&mut self, other: &Fields) -> &mut Self {
        self.0.extend(&other.0);
        self
    }

    pub fn u32(&mut self, index: u16, v: u32) -> &mut Self {
        self.0.insert(index, v);
        self
    }
    pub fn f32(&mut self, index: u16, v: f32) -> &mut Self {
        self.u32(index, v.to_bits())
    }
    /// A guid across two fields, low word first.
    pub fn u64(&mut self, index: u16, v: u64) -> &mut Self {
        self.u32(index, v as u32).u32(index + 1, (v >> 32) as u32)
    }

    /// The mask-block count, the mask words, then each set field's value in index order.
    fn write(&self, b: &mut Body) {
        let blocks = self
            .0
            .keys()
            .next_back()
            .map_or(0, |&i| i as usize / 32 + 1);
        let mut masks = vec![0u32; blocks];
        for &i in self.0.keys() {
            masks[i as usize / 32] |= 1 << (i % 32);
        }
        b.u8(blocks as u8);
        for m in masks {
            b.u32(m);
        }
        for &v in self.0.values() {
            b.u32(v);
        }
    }
}

/// A living unit's pose and speeds, as a `LIVING` movement block carries them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Living {
    pub movement_flags: u32,
    pub time: u32,
    pub position: [f32; 3],
    pub orientation: f32,
    /// `[walk, run, run_back, swim, swim_back, turn_rate]`.
    pub speeds: [f32; 6],
}

/// The 1.12 base speeds (vmangos `baseMoveSpeed`): walk 2.5, run 7, back 4.5, swim 4.72,
/// swim back 2.5 yd/s, and a turn rate of π rad/s.
pub const BASE_SPEEDS: [f32; 6] = [2.5, 7.0, 4.5, 4.722_222, 2.5, std::f32::consts::PI];

/// One `CREATE_OBJECT2` of a player with a `LIVING` block, wrapped as a whole update body.
pub fn create_player(guid: u64, is_self: bool, living: &Living, fields: &Fields) -> Vec<u8> {
    // vmangos's player flags: `0x71` for the receiver's own body, `0x70` for anyone else's.
    let self_flag = if is_self { UPDATEFLAG_SELF } else { 0 };
    create_living(
        UPDATETYPE_CREATE_OBJECT2,
        guid,
        TYPEID_PLAYER,
        self_flag,
        living,
        fields,
    )
}

/// One `CREATE_OBJECT` of a creature with a `LIVING` block (vmangos's `0x70`).
pub fn create_creature(guid: u64, living: &Living, fields: &Fields) -> Vec<u8> {
    create_living(
        UPDATETYPE_CREATE_OBJECT,
        guid,
        TYPEID_UNIT,
        0,
        living,
        fields,
    )
}

/// One `CREATE_OBJECT` of a gameobject standing at `position` facing `orientation` (vmangos's
/// `UPDATEFLAG_ALL | UPDATEFLAG_HAS_POSITION`).
pub fn create_gameobject(
    guid: u64,
    position: [f32; 3],
    orientation: f32,
    fields: &Fields,
) -> Vec<u8> {
    create_placed(TYPEID_GAMEOBJECT, guid, position, orientation, fields)
}

/// One `CREATE_OBJECT` of a corpse, placed as a gameobject is (vmangos's `Corpse` constructor).
pub fn create_corpse(guid: u64, position: [f32; 3], orientation: f32, fields: &Fields) -> Vec<u8> {
    create_placed(TYPEID_CORPSE, guid, position, orientation, fields)
}

fn create_placed(
    type_id: u8,
    guid: u64,
    position: [f32; 3],
    orientation: f32,
    fields: &Fields,
) -> Vec<u8> {
    let mut b = Body::default();
    b.u32(1).u8(0); // one object, no transport
    b.u8(UPDATETYPE_CREATE_OBJECT)
        .packed_guid(guid)
        .u8(type_id)
        .u8(UPDATEFLAG_ALL | UPDATEFLAG_HAS_POSITION)
        .f32(position[0])
        .f32(position[1])
        .f32(position[2])
        .f32(orientation)
        .u32(1); // UPDATEFLAG_ALL's word
    fields.write(&mut b);
    b.done()
}

/// One `CREATE_OBJECT` of an item: no pose, `UPDATEFLAG_ALL` alone (vmangos's `Item` constructor).
pub fn create_item(guid: u64, fields: &Fields) -> Vec<u8> {
    let mut b = Body::default();
    b.u32(1).u8(0);
    b.u8(UPDATETYPE_CREATE_OBJECT)
        .packed_guid(guid)
        .u8(TYPEID_ITEM)
        .u8(UPDATEFLAG_ALL)
        .u32(1); // UPDATEFLAG_ALL's word
    fields.write(&mut b);
    b.done()
}

/// One body of several blocks in order, from one-object bodies of this module: each one's count
/// and transport byte dropped, one count for all in front (vmangos's `UpdateData`).
pub fn join(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut b = Body::default();
    b.u32(bodies.len() as u32).u8(0);
    for body in bodies {
        debug_assert_eq!(body[..5], [1, 0, 0, 0, 0], "a one-object body");
        b.bytes(&body[5..]);
    }
    b.done()
}

/// One `VALUES` update: `fields` changed on `guid`.
pub fn values(guid: u64, fields: &Fields) -> Vec<u8> {
    let mut b = Body::default();
    b.u32(1).u8(0);
    b.u8(UPDATETYPE_VALUES).packed_guid(guid);
    fields.write(&mut b);
    b.done()
}

fn create_living(
    update_type: u8,
    guid: u64,
    type_id: u8,
    extra_flags: u8,
    living: &Living,
    fields: &Fields,
) -> Vec<u8> {
    let mut b = Body::default();
    b.u32(1).u8(0); // one object, no transport
    b.u8(update_type).packed_guid(guid).u8(type_id);
    let flags = UPDATEFLAG_ALL | UPDATEFLAG_LIVING | UPDATEFLAG_HAS_POSITION | extra_flags;
    b.u8(flags)
        .u32(living.movement_flags)
        .u32(living.time)
        .f32(living.position[0])
        .f32(living.position[1])
        .f32(living.position[2])
        .f32(living.orientation)
        .f32(0.0); // fall time
    for s in living.speeds {
        b.f32(s);
    }
    b.u32(1); // UPDATEFLAG_ALL's word
    fields.write(&mut b);
    b.done()
}

/// `UPDATETYPE_OUT_OF_RANGE_OBJECTS`: the guids leave the client's view.
pub fn out_of_range(guids: &[u64]) -> Vec<u8> {
    let mut b = Body::default();
    b.u32(1).u8(0);
    b.u8(4).u32(guids.len() as u32);
    for &g in guids {
        b.packed_guid(g);
    }
    b.done()
}
