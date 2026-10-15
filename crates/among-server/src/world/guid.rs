//! Guids for the objects the match puts in the world. A creature or gameobject guid carries its
//! template entry in bits 24-47 under its high word (vmangos `ObjectGuid`), which is what the
//! client's creature and gameobject queries ask about.

use among_rules::{CreatureId, SabotageKind, TaskId};

/// `HIGHGUID_UNIT`.
const HIGH_UNIT: u64 = 0xF130;
/// `HIGHGUID_GAMEOBJECT`.
const HIGH_GAMEOBJECT: u64 = 0xF110;
/// `HIGHGUID_CORPSE` (vmangos `ObjectGuid.h`).
const HIGH_CORPSE: u64 = 0xF101;

/// Gameobject entries are task steps: `task * STEPS + step + 1`, so entry 0 is never used.
const STEPS: u64 = 16;

fn make(high: u64, entry: u64) -> u64 {
    make_low(high, entry, entry)
}

/// `low` is the per-object counter under the entry, unique among objects of that entry.
fn make_low(high: u64, entry: u64, low: u64) -> u64 {
    high << 48 | entry << 24 | low
}

fn split(guid: u64, high: u64) -> Option<u64> {
    (guid >> 48 == high).then_some(guid >> 24 & 0xFF_FFFF)
}

/// The corpse of the player `owner`: a player leaves at most one, so its low word is theirs.
pub fn corpse(owner: u64) -> u64 {
    HIGH_CORPSE << 48 | owner & 0xFFFF_FFFF
}

/// The creature at spawn point `id`; its entry is `id + 1`.
pub fn creature(id: CreatureId) -> u64 {
    make(HIGH_UNIT, id.0 as u64 + 1)
}

pub fn creature_id(guid: u64) -> Option<CreatureId> {
    let entry = split(guid, HIGH_UNIT)?;
    Some(CreatureId(entry.checked_sub(1)? as usize))
}

/// A creature template entry back to its spawn point.
pub fn creature_entry(entry: u32) -> Option<CreatureId> {
    Some(CreatureId(entry.checked_sub(1)? as usize))
}

/// Node `index` of `task`'s step `step`: a Cast step's one node is index 0, a Gather step has
/// one per spot. Every node of a step shares the step's entry; the counter is `index + 1`.
pub fn node(task: TaskId, step: usize, index: usize) -> u64 {
    make_low(
        HIGH_GAMEOBJECT,
        node_entry(task, step).into(),
        index as u64 + 1,
    )
}

pub fn node_entry(task: TaskId, step: usize) -> u32 {
    (task.0 as u64 * STEPS + step as u64 + 1) as u32
}

/// A node guid back to its task, step and node index.
pub fn node_step(guid: u64) -> Option<(TaskId, usize, usize)> {
    let (task, step) = entry_step(split(guid, HIGH_GAMEOBJECT)? as u32)?;
    let index = (guid & 0xFF_FFFF).checked_sub(1)?;
    Some((task, step, index as usize))
}

/// A gameobject template entry back to its task and step.
pub fn entry_step(entry: u32) -> Option<(TaskId, usize)> {
    let i = u64::from(entry).checked_sub(1)?;
    Some((TaskId((i / STEPS) as usize), (i % STEPS) as usize))
}

/// Whether a task with `steps` steps fits the node entries.
pub fn fits(steps: usize) -> bool {
    steps as u64 <= STEPS
}

/// The fixtures every match shares: the emergency button, each sabotage's stations, and the
/// beacon over a station still to fix. Their entries sit past every task step's
/// (`FIXTURE_ENTRIES` is far above `tasks * STEPS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fixture {
    Button,
    /// Station `n` of the sabotage `kind`, as the layout lists them.
    Station(SabotageKind, usize),
    Beacon(SabotageKind, usize),
    /// The freezing trap of the hunter whose guid's low 24 bits are these; only they see it.
    Trap(u64),
}

/// A fixture's template: the button, or a kind's station or beacon, or a freezing trap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureTemplate {
    Button,
    Station(SabotageKind),
    Beacon(SabotageKind),
    Trap,
}

const FIXTURE_ENTRIES: u32 = 0xF0_0000;
const KINDS: [SabotageKind; 4] = [
    SabotageKind::Lights,
    SabotageKind::Comms,
    SabotageKind::Oxygen,
    SabotageKind::Reactor,
];

fn kind_index(kind: SabotageKind) -> u32 {
    KINDS.iter().position(|&k| k == kind).expect("every kind") as u32
}

impl FixtureTemplate {
    pub fn entry(self) -> u32 {
        FIXTURE_ENTRIES
            + match self {
                FixtureTemplate::Button => 0,
                FixtureTemplate::Station(k) => 0x10 + kind_index(k),
                FixtureTemplate::Beacon(k) => 0x20 + kind_index(k),
                FixtureTemplate::Trap => 0x30,
            }
    }

    pub fn of_entry(entry: u32) -> Option<Self> {
        let i = entry.checked_sub(FIXTURE_ENTRIES)?;
        let kind = |base: u32| KINDS.get(i.checked_sub(base)? as usize).copied();
        match i {
            0 => Some(FixtureTemplate::Button),
            0x10..=0x13 => kind(0x10).map(FixtureTemplate::Station),
            0x20..=0x23 => kind(0x20).map(FixtureTemplate::Beacon),
            0x30 => Some(FixtureTemplate::Trap),
            _ => None,
        }
    }
}

impl Fixture {
    pub fn template(self) -> FixtureTemplate {
        match self {
            Fixture::Button => FixtureTemplate::Button,
            Fixture::Station(k, _) => FixtureTemplate::Station(k),
            Fixture::Beacon(k, _) => FixtureTemplate::Beacon(k),
            Fixture::Trap(_) => FixtureTemplate::Trap,
        }
    }

    /// The trap `owner` sets.
    pub fn trap(owner: u64) -> Self {
        Fixture::Trap(owner & 0xFF_FFFF)
    }

    pub fn guid(self) -> u64 {
        let low = match self {
            Fixture::Button => 1,
            Fixture::Station(_, n) | Fixture::Beacon(_, n) => n as u64 + 1,
            Fixture::Trap(owner) => owner,
        };
        make_low(HIGH_GAMEOBJECT, self.template().entry().into(), low)
    }

    pub fn of_guid(guid: u64) -> Option<Self> {
        let template = FixtureTemplate::of_entry(split(guid, HIGH_GAMEOBJECT)? as u32)?;
        let n = ((guid & 0xFF_FFFF).checked_sub(1))? as usize;
        Some(match template {
            FixtureTemplate::Button => Fixture::Button,
            FixtureTemplate::Station(k) => Fixture::Station(k, n),
            FixtureTemplate::Beacon(k) => Fixture::Beacon(k, n),
            FixtureTemplate::Trap => Fixture::Trap(n as u64 + 1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_carry_their_entry_and_map_back() {
        let g = creature(CreatureId(4));
        assert_eq!(g >> 48, 0xF130);
        assert_eq!(benilla_protocol::guid::entry(g), Some(5));
        assert_eq!(creature_id(g), Some(CreatureId(4)));
        assert_eq!(creature_id(node(TaskId(0), 0, 0)), None);

        let n = node(TaskId(3), 1, 6);
        assert_eq!(n >> 48, 0xF110);
        assert_eq!(
            benilla_protocol::guid::entry(n),
            Some(node_entry(TaskId(3), 1))
        );
        assert_eq!(node_step(n), Some((TaskId(3), 1, 6)));
        assert_ne!(
            n,
            node(TaskId(3), 1, 5),
            "each node of a step is its own object"
        );
        assert_eq!(node_step(7), None, "a player");
        assert_eq!(entry_step(0), None);
    }

    #[test]
    fn fixtures_map_back_and_stay_clear_of_task_nodes() {
        let all = [
            Fixture::Button,
            Fixture::Station(SabotageKind::Oxygen, 1),
            Fixture::Beacon(SabotageKind::Reactor, 0),
            Fixture::trap(7),
        ];
        for f in all {
            let g = f.guid();
            assert_eq!(Fixture::of_guid(g), Some(f));
            assert_eq!(benilla_protocol::guid::entry(g), Some(f.template().entry()));
            assert_eq!(
                FixtureTemplate::of_entry(f.template().entry()),
                Some(f.template())
            );
        }
        assert_eq!(Fixture::of_guid(node(TaskId(3), 1, 0)), None);
        assert_eq!(FixtureTemplate::of_entry(node_entry(TaskId(200), 15)), None);
        assert_ne!(
            Fixture::Station(SabotageKind::Oxygen, 0).guid(),
            Fixture::Station(SabotageKind::Oxygen, 1).guid()
        );
    }
}
