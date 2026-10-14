//! Guids for the objects the match puts in the world. A creature or gameobject guid carries its
//! template entry in bits 24-47 under its high word (vmangos `ObjectGuid`), which is what the
//! client's creature and gameobject queries ask about.

use among_rules::{CreatureId, TaskId};

/// `HIGHGUID_UNIT`.
const HIGH_UNIT: u64 = 0xF130;
/// `HIGHGUID_GAMEOBJECT`.
const HIGH_GAMEOBJECT: u64 = 0xF110;

/// Gameobject entries are task nodes: `task * STEPS + step + 1`, so entry 0 is never used.
const STEPS: u64 = 16;

fn make(high: u64, entry: u64) -> u64 {
    high << 48 | entry << 24 | entry
}

fn split(guid: u64, high: u64) -> Option<u64> {
    (guid >> 48 == high).then_some(guid >> 24 & 0xFF_FFFF)
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

/// The node `task`'s step `step` is worked at.
pub fn node(task: TaskId, step: usize) -> u64 {
    make(HIGH_GAMEOBJECT, node_entry(task, step).into())
}

pub fn node_entry(task: TaskId, step: usize) -> u32 {
    (task.0 as u64 * STEPS + step as u64 + 1) as u32
}

/// A node guid back to its task and step.
pub fn node_step(guid: u64) -> Option<(TaskId, usize)> {
    entry_step(split(guid, HIGH_GAMEOBJECT)? as u32)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_carry_their_entry_and_map_back() {
        let g = creature(CreatureId(4));
        assert_eq!(g >> 48, 0xF130);
        assert_eq!(benilla_protocol::guid::entry(g), Some(5));
        assert_eq!(creature_id(g), Some(CreatureId(4)));
        assert_eq!(creature_id(node(TaskId(0), 0)), None);

        let n = node(TaskId(3), 1);
        assert_eq!(n >> 48, 0xF110);
        assert_eq!(
            benilla_protocol::guid::entry(n),
            Some(node_entry(TaskId(3), 1))
        );
        assert_eq!(node_step(n), Some((TaskId(3), 1)));
        assert_eq!(node_step(7), None, "a player");
        assert_eq!(entry_step(0), None);
    }
}
