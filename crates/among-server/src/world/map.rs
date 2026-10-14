//! The one map every player stands on: who is in the world, where, and how to reach them. A move
//! from one player is relayed to every other as the same `MSG_MOVE_*`, prefixed with the mover's
//! packed guid, as vmangos's `MovementHandler` broadcasts it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use benilla_protocol::messages::opcode;

use super::player;
use super::socket::{Outbox, Packet};
use super::update::Living;
use super::wire::Body;
use crate::accounts::CharacterRecord;

struct Occupant {
    character: CharacterRecord,
    outbox: Outbox,
    living: Living,
}

/// The shared world; cheap to clone.
#[derive(Clone, Default)]
pub struct Map(Arc<Mutex<HashMap<u64, Occupant>>>);

impl Map {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Occupant>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Seat `character` at `living`: everyone already here is created for the newcomer, and the
    /// newcomer for everyone already here.
    pub fn enter(&self, character: CharacterRecord, outbox: Outbox, living: Living) {
        let mut occupants = self.lock();
        let guid = character.guid;
        let newcomer = player::create_packet(&character, &living, false);
        for other in occupants.values() {
            let _ = other.outbox.send(newcomer.clone());
            let _ = outbox.send(player::create_packet(
                &other.character,
                &other.living,
                false,
            ));
        }
        occupants.insert(
            guid,
            Occupant {
                character,
                outbox,
                living,
            },
        );
    }

    /// Record `guid`'s new pose and relay its move to everyone else.
    pub fn relay_move(&self, guid: u64, op: u16, movement: &[u8], pose: Option<Living>) {
        let mut occupants = self.lock();
        if let (Some(me), Some(pose)) = (occupants.get_mut(&guid), pose) {
            me.living = Living {
                speeds: me.living.speeds,
                ..pose
            };
        }
        let relay = Packet::new(op, Body::default().packed_guid(guid).bytes(movement).done());
        for (&other, occupant) in occupants.iter() {
            if other != guid {
                let _ = occupant.outbox.send(relay.clone());
            }
        }
    }

    /// Take `guid` out of the world and destroy it on every other client.
    pub fn leave(&self, guid: u64) {
        let mut occupants = self.lock();
        if occupants.remove(&guid).is_none() {
            return;
        }
        let destroy = Packet::new(opcode::SMSG_DESTROY_OBJECT, guid.to_le_bytes().to_vec());
        for occupant in occupants.values() {
            let _ = occupant.outbox.send(destroy.clone());
        }
    }

    /// Queue `packet` for `guid`, if it is in the world.
    pub fn send(&self, guid: u64, packet: Packet) {
        if let Some(o) = self.lock().get(&guid) {
            let _ = o.outbox.send(packet);
        }
    }

    /// Every guid in the world.
    pub fn guids(&self) -> Vec<u64> {
        self.lock().keys().copied().collect()
    }

    /// The character behind an in-world guid, for name queries.
    pub fn character(&self, guid: u64) -> Option<CharacterRecord> {
        self.lock().get(&guid).map(|o| o.character.clone())
    }

    /// How many players are in the world.
    pub fn population(&self) -> usize {
        self.lock().len()
    }
}
