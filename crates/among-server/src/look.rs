//! How the match looks in the world: the node each task step is worked at (a herb, an ore vein, a
//! crafting station), the spell its cast shows, and each creature's model. The rules know only
//! positions and durations; this is the WoW dressing the server puts on them.

use among_rules::{Layout, SabotageKind, StepAction, TaskId};

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
    /// Mined with Mining and a Mining Pick, as an ore vein is.
    Mine,
    /// Used at a crafting station; the server shows the craft's cast.
    Station,
}

/// Herb Gathering, 5 s in `Spell.dbc`.
pub const HERB_GATHERING: u32 = 2366;
/// Mining: the pickaxe swing (`SpellVisualKit` 166, anim 62, the pick model at the hand).
pub const MINING: u32 = 2575;

// `GameObjectDisplayInfo.dbc` ids of nodes and stations.
pub const PEACEBLOOM: u32 = 269;
pub const SILVERLEAF: u32 = 270;
pub const EARTHROOT: u32 = 271;
pub const COPPER_VEIN: u32 = 310;
pub const FORGE: u32 = 233;
pub const ANVIL: u32 = 273;
pub const CAMPFIRE: u32 = 192;

// `GameObjectDisplayInfo.dbc` ids of the match's fixtures, all in the 1.12.1 install.
/// `World\Goober\G_ButtonBigRed.mdx`.
pub const BIG_RED_BUTTON: u32 = 339;
/// `World\Kalimdor\Barrens\PassiveDoodads\LampPosts\BarrensLampPost01.mdx`.
pub const LAMP_POST: u32 = 365;
/// `World\Goober\G_GnomeTerminal.mdx`.
pub const GNOME_TERMINAL: u32 = 2091;
/// `World\Goober\G_LeverMetal.mdx`.
pub const METAL_LEVER: u32 = 295;
/// `WORLD\GENERIC\GNOME\ACTIVEDOODADS\GNOMEMACHINE\GNOMEMACHINE.MDX`.
pub const GNOME_MACHINE: u32 = 231;
/// `World\Generic\PassiveDoodads\ParticleEmitters\AuraPurpleVeryTall.mdx`: a tall column of
/// light, seen from far off.
pub const PURPLE_AURA: u32 = 2234;

/// The emergency button: a right-click is the horn's press.
pub fn button() -> NodeLook {
    NodeLook::station("Emergency Button", BIG_RED_BUTTON, 0)
}

/// A sabotage's station: a right-click fixes it, or holds it for the reactor.
pub fn station(kind: SabotageKind) -> NodeLook {
    match kind {
        SabotageKind::Lights => NodeLook::station("Lamp Post", LAMP_POST, 0),
        SabotageKind::Comms => NodeLook::station("Signal Terminal", GNOME_TERMINAL, 0),
        SabotageKind::Oxygen => NodeLook::station("Air Valve", METAL_LEVER, 0),
        SabotageKind::Reactor => NodeLook::station("Arcane Reactor", GNOME_MACHINE, 0),
    }
}

/// The column over a station still to fix while its sabotage runs; a right-click on it is one on
/// the station beneath.
pub fn beacon(kind: SabotageKind) -> NodeLook {
    let mut look = station(kind);
    look.name = format!("Sabotaged {}", look.name);
    look.display = PURPLE_AURA;
    look
}

/// The nodes a layout file may name for a step, by `node = "<name>"`.
pub const NODE_NAMES: [&str; 7] = [
    "peacebloom",
    "silverleaf",
    "earthroot",
    "copper",
    "campfire",
    "forge",
    "anvil",
];

/// The node a layout file names: a herb, a copper vein, or a crafting station showing its craft.
pub fn named(name: &str) -> Option<NodeLook> {
    Some(match name {
        "peacebloom" => NodeLook::herb("Peacebloom", PEACEBLOOM),
        "silverleaf" => NodeLook::herb("Silverleaf", SILVERLEAF),
        "earthroot" => NodeLook::herb("Earthroot", EARTHROOT),
        "copper" => NodeLook::mine("Copper Vein", COPPER_VEIN),
        // Roasted Boar Meat, Smelt Copper and Copper Bracers: the crafts each station shows.
        "campfire" => NodeLook::station("Campfire", CAMPFIRE, 2540),
        "forge" => NodeLook::station("Forge", FORGE, 2657),
        "anvil" => NodeLook::station("Anvil", ANVIL, 2663),
        _ => return None,
    })
}

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
    /// Every cast step a copper vein to mine, every creature a wolf: a look that fits
    /// any layout.
    pub fn plain(layout: &Layout) -> Self {
        Self {
            steps: (layout.tasks.iter())
                .map(|t| {
                    (t.steps.iter())
                        .map(|s| match s.action {
                            StepAction::Slay { .. } => None,
                            StepAction::Cast(_) | StepAction::Gather { .. } => {
                                Some(NodeLook::mine(&t.name, COPPER_VEIN))
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

    pub fn mine(name: &str, display: u32) -> Self {
        Self {
            name: name.into(),
            kind: NodeKind::Mine,
            display,
            spell: MINING,
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
