//! How the match looks in the world: the node each task step is worked at (a herb, an ore vein, a
//! crafting station), the spell its cast shows, and each creature's model. The rules know only
//! positions and durations; this is the WoW dressing the server puts on them.

use among_rules::{Layout, StepAction, TaskId};

/// What a task step is worked at.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeLook {
    pub name: String,
    pub kind: NodeKind,
    /// `GameObjectDisplayInfo.dbc` id.
    pub display: u32,
    /// The spell the step's cast shows on cast bars.
    pub spell: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// Picked with Herb Gathering, as a herb node is.
    Herb,
    /// Worked kneeling, as a lock with no skill is opened: veins and other spots with no tool.
    Kneel,
    /// Used at a crafting station; the server shows the craft's cast.
    Station,
}

/// Herb Gathering, 5 s in `Spell.dbc`.
pub const HERB_GATHERING: u32 = 2366;
/// "Opening" kneel cast (`Lock.dbc` 43, no skill), 5 s.
pub const KNEEL: u32 = 6478;

// `GameObjectDisplayInfo.dbc` ids of nodes and stations.
pub const PEACEBLOOM: u32 = 269;
pub const SILVERLEAF: u32 = 270;
pub const EARTHROOT: u32 = 271;
pub const COPPER_VEIN: u32 = 310;
pub const FORGE: u32 = 233;
pub const ANVIL: u32 = 273;
pub const CAMPFIRE: u32 = 192;

/// A creature's model and level.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatureLook {
    /// `CreatureDisplayInfo.dbc` id.
    pub display: u32,
    pub scale: f32,
    pub level: u32,
}

/// The dressing for one [`Layout`]: per task, per step, the node (`None` for a Slay step, which
/// is fought); per creature, its look.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Look {
    pub steps: Vec<Vec<Option<NodeLook>>>,
    pub creatures: Vec<CreatureLook>,
}

impl Look {
    /// The node of `task`'s step `step`, if it is worked at one.
    pub fn node(&self, task: TaskId, step: usize) -> Option<&NodeLook> {
        self.steps.get(task.0)?.get(step)?.as_ref()
    }

    /// Whether every cast step of `layout` has a node and every creature a look.
    pub fn covers(&self, layout: &Layout) -> bool {
        layout.tasks.len() == self.steps.len()
            && (layout.tasks.iter().zip(&self.steps)).all(|(t, looks)| {
                t.steps.len() == looks.len()
                    && (t.steps.iter().zip(looks))
                        .all(|(s, l)| matches!(s.action, StepAction::Slay { .. }) == l.is_none())
            })
            && layout.creatures.len() == self.creatures.len()
    }
}

impl Look {
    /// Every cast step a kneeling spot at a copper vein, every creature a wolf: a look that fits
    /// any layout.
    pub fn plain(layout: &Layout) -> Self {
        Self {
            steps: (layout.tasks.iter())
                .map(|t| {
                    (t.steps.iter())
                        .map(|s| match s.action {
                            StepAction::Slay { .. } => None,
                            StepAction::Cast(_) | StepAction::Gather { .. } => {
                                Some(NodeLook::kneel(&t.name, COPPER_VEIN))
                            }
                        })
                        .collect()
                })
                .collect(),
            creatures: layout
                .creatures
                .iter()
                .map(|_| CreatureLook::wolf())
                .collect(),
        }
    }
}

impl CreatureLook {
    /// A young wolf: `CreatureDisplayInfo` 447.
    pub fn wolf() -> Self {
        Self {
            display: 447,
            scale: 1.0,
            level: 2,
        }
    }
}

impl NodeLook {
    pub fn herb(name: &str, display: u32) -> Self {
        Self {
            name: name.into(),
            kind: NodeKind::Herb,
            display,
            spell: HERB_GATHERING,
        }
    }

    pub fn kneel(name: &str, display: u32) -> Self {
        Self {
            name: name.into(),
            kind: NodeKind::Kneel,
            display,
            spell: KNEEL,
        }
    }

    /// A station whose use shows `spell`, the craft's own cast.
    pub fn station(name: &str, display: u32, spell: u32) -> Self {
        Self {
            name: name.into(),
            kind: NodeKind::Station,
            display,
            spell,
        }
    }
}
