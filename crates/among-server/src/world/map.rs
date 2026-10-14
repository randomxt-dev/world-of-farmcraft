//! The one map every player stands on: who is in the world, where, and how to reach them, plus
//! the gameobjects and creatures the match shows. A move from one player is relayed to every other
//! as the same `MSG_MOVE_*`, prefixed with the mover's packed guid, as vmangos's
//! `MovementHandler` broadcasts it. A ghost is shown only to other ghosts, as vmangos's
//! `GHOST_VISIBILITY_GHOST` keeps it from the living.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use benilla_protocol::messages::opcode;

use super::player::{self, Life};
use super::socket::{Outbox, Packet};
use super::update::Living;
use super::wire::Body;
use crate::accounts::CharacterRecord;

struct Occupant {
    character: CharacterRecord,
    outbox: Outbox,
    living: Living,
    life: Life,
    /// The last counter a forced movement change (`SMSG_FORCE_*`) carried, which the ack echoes.
    counter: u32,
}

/// Whether a client in `viewer`'s state is shown a player in `subject`'s: anyone not a ghost, and
/// ghosts to ghosts.
fn sees(viewer: Life, subject: Life) -> bool {
    subject != Life::Ghost || viewer == Life::Ghost
}

/// Whether a listener in `listener`'s state hears a speaker in `speaker`'s: the living are heard
/// by all, the dead (fallen or ghost) only by the dead, as in Among Us.
fn hears(listener: Life, speaker: Life) -> bool {
    speaker == Life::Alive || listener != Life::Alive
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Occupant {
    fn create(&self) -> Packet {
        player::create_packet(&self.character, &self.living, false, self.life)
    }
}

fn destroy(guid: u64) -> Packet {
    Packet::new(opcode::SMSG_DESTROY_OBJECT, guid.to_le_bytes().to_vec())
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
            for o in world.occupants.values() {
                let _ = o.outbox.send(destroy(guid));
            }
        }
    }

    /// Destroy `object` on `player`'s client only; the object stays in the world for everyone else.
    pub fn hide_from(&self, player: u64, object: u64) {
        self.send(player, destroy(object));
    }

    /// Create `object` again on `player`'s client, as it stands now, after [`Map::hide_from`].
    pub fn show_to(&self, player: u64, object: u64) {
        let world = self.lock();
        if let (Some(create), Some(o)) = (world.objects.get(&object), world.occupants.get(&player))
        {
            let _ = o.outbox.send(create.clone());
        }
    }

    /// Queue `packet` for everyone in the world.
    pub fn broadcast(&self, packet: &Packet) {
        for o in self.lock().occupants.values() {
            let _ = o.outbox.send(packet.clone());
        }
    }

    /// Queue `packet`, which shows player `subject` doing something, for everyone who sees them:
    /// a ghost's casts reach only ghosts. A guid that is no player reaches everyone.
    pub fn broadcast_about(&self, subject: u64, packet: &Packet) {
        let world = self.lock();
        let life = world
            .occupants
            .get(&subject)
            .map_or(Life::Alive, |o| o.life);
        for (&guid, o) in &world.occupants {
            if guid == subject || sees(o.life, life) {
                let _ = o.outbox.send(packet.clone());
            }
        }
    }

    /// `guid` is now `life`: `change` (the values update that shows it) goes to whoever saw them
    /// before and still does, and to themself. A ghost vanishes from the living and appears to
    /// whoever is newly a ghost, who in turn now sees the other ghosts.
    pub fn set_life(&self, guid: u64, life: Life, change: Packet) {
        let mut world = self.lock();
        let Some(me) = world.occupants.get_mut(&guid) else {
            return;
        };
        let was = std::mem::replace(&mut me.life, life);
        let me = &world.occupants[&guid];
        let _ = me.outbox.send(change.clone());
        for (&other, o) in world.occupants.iter().filter(|(&g, _)| g != guid) {
            let _ = match (sees(o.life, was), sees(o.life, life)) {
                (true, true) => o.outbox.send(change.clone()),
                (true, false) => o.outbox.send(destroy(guid)),
                (false, true) => o.outbox.send(me.create()),
                (false, false) => Ok(()),
            };
            let _ = match (sees(was, o.life), sees(life, o.life)) {
                (true, false) => me.outbox.send(destroy(other)),
                (false, true) => me.outbox.send(o.create()),
                _ => Ok(()),
            };
        }
    }

    /// Move `guid` to `at` facing `facing` at once: their client is told with the acked teleport,
    /// everyone who sees them with `MSG_MOVE_TELEPORT`, sent twice as vmangos sends a near one.
    pub fn teleport(&self, guid: u64, at: [f32; 3], facing: f32) {
        let mut world = self.lock();
        let Some(me) = world.occupants.get_mut(&guid) else {
            return;
        };
        me.counter = me.counter.wrapping_add(1);
        me.living = Living {
            movement_flags: 0,
            position: at,
            orientation: facing,
            ..me.living
        };
        let _ = me
            .outbox
            .send(super::meeting::teleport(guid, me.counter, at, facing));
        let life = me.life;
        let seen = super::meeting::teleported(guid, at, facing);
        for (&other, o) in &world.occupants {
            if other != guid && sees(o.life, life) {
                let _ = o.outbox.send(seen.clone());
                let _ = o.outbox.send(seen.clone());
            }
        }
    }

    /// The next forced-change counter for `guid`, or `None` if they are not in the world.
    pub fn next_counter(&self, guid: u64) -> Option<u32> {
        let mut world = self.lock();
        let o = world.occupants.get_mut(&guid)?;
        o.counter = o.counter.wrapping_add(1);
        Some(o.counter)
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
        let newcomer = player::create_packet(&character, &living, false, Life::Alive);
        for other in occupants.values() {
            let _ = other.outbox.send(newcomer.clone());
            if sees(Life::Alive, other.life) {
                let _ = outbox.send(other.create());
            }
        }
        occupants.insert(
            guid,
            Occupant {
                character,
                outbox,
                living,
                life: Life::Alive,
                counter: 0,
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
        let life = occupants.get(&guid).map_or(Life::Alive, |o| o.life);
        for (&other, occupant) in occupants.iter() {
            if other != guid && sees(occupant.life, life) {
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
        for occupant in occupants.values() {
            let _ = occupant.outbox.send(destroy(guid));
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

    /// Record `guid`'s swim speed, which later creates of it carry.
    pub fn set_swim_speed(&self, guid: u64, speed: f32) {
        if let Some(o) = self.lock().occupants.get_mut(&guid) {
            o.living.speeds[3] = speed;
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

    /// Queue a chat line `speaker` said for everyone who hears it: within `range` yards of them
    /// (`None`: anywhere), and a dead speaker only by the dead.
    pub fn say(&self, speaker: u64, range: Option<f32>, packet: &Packet) {
        let world = self.lock();
        let Some(me) = world.occupants.get(&speaker) else {
            return;
        };
        let (from, life) = (me.living.position, me.life);
        for o in world.occupants.values() {
            let near = range.is_none_or(|r| distance(from, o.living.position) <= r);
            if near && hears(o.life, life) {
                let _ = o.outbox.send(packet.clone());
            }
        }
    }

    /// How many players are in the world.
    pub fn population(&self) -> usize {
        self.lock().occupants.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dead_are_heard_only_by_the_dead() {
        for listener in [Life::Alive, Life::Dead, Life::Ghost] {
            assert!(
                hears(listener, Life::Alive),
                "the living reach {listener:?}"
            );
        }
        for speaker in [Life::Dead, Life::Ghost] {
            assert!(
                !hears(Life::Alive, speaker),
                "{speaker:?} is not heard by the living"
            );
            assert!(hears(Life::Ghost, speaker) && hears(Life::Dead, speaker));
        }
    }

    #[test]
    fn distance_is_3d() {
        assert_eq!(distance([0.0, 0.0, 0.0], [3.0, 4.0, 12.0]), 13.0);
    }
}
