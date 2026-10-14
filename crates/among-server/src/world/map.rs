//! The one map every player stands on: who is in the world, where, and how to reach them, plus
//! the gameobjects and creatures the match shows. A move from one player is relayed to every other
//! as the same `MSG_MOVE_*`, prefixed with the mover's packed guid, as vmangos's
//! `MovementHandler` broadcasts it.

use std::collections::{BTreeMap, HashMap};
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

#[derive(Default)]
struct World {
    occupants: HashMap<u64, Occupant>,
    /// Every non-player object in the world, by guid: its `SMSG_UPDATE_OBJECT` create as it
    /// stands now, sent to each newcomer.
    objects: BTreeMap<u64, Packet>,
}

/// The shared world; cheap to clone.
#[derive(Clone, Default)]
pub struct Map(Arc<Mutex<World>>);

impl Map {
    fn lock(&self) -> std::sync::MutexGuard<'_, World> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Put an object in the world, or replace it: `create` goes to everyone now and to every
    /// later newcomer.
    pub fn spawn(&self, guid: u64, create: Packet) {
        let mut world = self.lock();
        for o in world.occupants.values() {
            let _ = o.outbox.send(create.clone());
        }
        world.objects.insert(guid, create);
    }

    /// Keep `create` for later newcomers without telling anyone now: the object changed and
    /// the players already here were sent the change itself.
    pub fn restate(&self, guid: u64, create: Packet) {
        if let Some(c) = self.lock().objects.get_mut(&guid) {
            *c = create;
        }
    }

    /// Take an object out of the world and destroy it on every client.
    pub fn despawn(&self, guid: u64) {
        let mut world = self.lock();
        if world.objects.remove(&guid).is_some() {
            let destroy = Packet::new(opcode::SMSG_DESTROY_OBJECT, guid.to_le_bytes().to_vec());
            for o in world.occupants.values() {
                let _ = o.outbox.send(destroy.clone());
            }
        }
    }

    /// Queue `packet` for everyone in the world.
    pub fn broadcast(&self, packet: &Packet) {
        for o in self.lock().occupants.values() {
            let _ = o.outbox.send(packet.clone());
        }
    }

    /// Where `guid` stands, if it is a player in the world.
    pub fn position(&self, guid: u64) -> Option<[f32; 3]> {
        self.lock().occupants.get(&guid).map(|o| o.living.position)
    }

    /// Seat `character` at `living`: everyone already here is created for the newcomer, and the
    /// newcomer for everyone already here.
    pub fn enter(&self, character: CharacterRecord, outbox: Outbox, living: Living) {
        let mut world = self.lock();
        for create in world.objects.values() {
            let _ = outbox.send(create.clone());
        }
        let occupants = &mut world.occupants;
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
        let mut world = self.lock();
        let occupants = &mut world.occupants;
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
        let mut world = self.lock();
        let occupants = &mut world.occupants;
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
        if let Some(o) = self.lock().occupants.get(&guid) {
            let _ = o.outbox.send(packet);
        }
    }

    /// Every guid in the world.
    pub fn guids(&self) -> Vec<u64> {
        self.lock().occupants.keys().copied().collect()
    }

    /// Record `guid`'s run speed, which later creates of it carry.
    pub fn set_run_speed(&self, guid: u64, speed: f32) {
        if let Some(o) = self.lock().occupants.get_mut(&guid) {
            o.living.speeds[1] = speed;
        }
    }

    /// Where `guid` last reported standing.
    pub fn pose(&self, guid: u64) -> Option<Living> {
        self.lock().occupants.get(&guid).map(|o| o.living)
    }

    /// The character behind an in-world guid, for name queries.
    pub fn character(&self, guid: u64) -> Option<CharacterRecord> {
        self.lock()
            .occupants
            .get(&guid)
            .map(|o| o.character.clone())
    }

    /// How many players are in the world.
    pub fn population(&self) -> usize {
        self.lock().occupants.len()
    }
}
