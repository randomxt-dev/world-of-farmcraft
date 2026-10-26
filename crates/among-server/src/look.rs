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

/// `GameObjectDisplayInfo.dbc` 3071: `G_FreezingTrap.mdx`, gameobject 2561's look.
const FREEZING_TRAP: u32 = 3071;

/// A hunter's freezing trap, as only its owner sees it.
pub fn trap() -> NodeLook {
    NodeLook::station("Freezing Trap", FREEZING_TRAP, 0)
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

/// A creature's model, size, level, type and the weapon it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatureLook {
    /// `CreatureDisplayInfo.dbc` id.
    pub display: u32,
    /// `OBJECT_FIELD_SCALE_X`, which the client sizes the model by alone: the display's native
    /// scale (`CreatureModelData.modelScale` x `CreatureDisplayInfo.scale`) for a faithful size.
    pub scale: f32,
    pub level: u32,
    pub kind: CreatureType,
    /// The main-hand weapon it wields, shown with no item behind it.
    pub weapon: Option<Weapon>,
}

/// `CREATURE_TYPE_*`: what the tooltip names and what a spell's target-type check reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreatureType {
    Beast = 1,
    Humanoid = 7,
}

/// The kinds a layout file may give a creature, by `kind = "<name>"`.
pub const CREATURE_TYPE_NAMES: [&str; 2] = ["beast", "humanoid"];

impl CreatureType {
    /// The type a layout file names.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "beast" => Some(Self::Beast),
            "humanoid" => Some(Self::Humanoid),
            _ => None,
        }
    }
}

/// A creature's virtual weapon: what vmangos `Creature::SetVirtualItem` copies from the item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weapon {
    /// `ItemDisplayInfo.dbc` id.
    pub display: u32,
    pub class: u8,
    pub subclass: u8,
    /// Keys the draw and stow sound only.
    pub material: u8,
    pub inventory_type: u8,
    pub sheath: u8,
}

/// The weapons a layout file may name, by `weapon = "<name>"` ("none" for bare hands).
pub const WEAPON_NAMES: [&str; 3] = ["none", "staff", "sword"];

impl Weapon {
    /// Item 5276 "Monster - Staff, 3 Piece Taped Staff": a two-hand staff (class 2, subclass
    /// 10, inventory type 17, sheath 2), material 2 wood assumed.
    pub const TAPED_STAFF: Self = Self {
        display: 5010,
        class: 2,
        subclass: 10,
        material: 2,
        inventory_type: 17,
        sheath: 2,
    };
    /// Item 1896 "Monster - Sword, Short Basic": a one-hand sword (class 2, subclass 7,
    /// inventory type 13, sheath 3), material 1 metal assumed.
    pub const SHORT_SWORD: Self = Self {
        display: 1593,
        class: 2,
        subclass: 7,
        material: 1,
        inventory_type: 13,
        sheath: 3,
    };

    /// The weapon a layout file names: `Some(None)` for "none", `None` for an unknown name.
    pub fn named(name: &str) -> Option<Option<Self>> {
        match name {
            "none" => Some(None),
            "staff" => Some(Some(Self::TAPED_STAFF)),
            "sword" => Some(Some(Self::SHORT_SWORD)),
            _ => None,
        }
    }
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
    /// Every cast step a copper vein to mine, every creature its name's look: a look that fits
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
                .map(|c| CreatureLook::named(&c.name))
                .collect(),
        }
    }
}

impl CreatureLook {
    /// Young Wolf (299): display 447 at its native 0.4, level 1, a beast.
    pub fn wolf() -> Self {
        Self {
            display: 447,
            scale: 0.4,
            level: 1,
            kind: CreatureType::Beast,
            weapon: None,
        }
    }

    /// Kobold Vermin (6): display 10913 at its native 0.7, level 1, a humanoid with a staff.
    pub fn kobold() -> Self {
        Self {
            display: 10913,
            scale: 0.7,
            level: 1,
            kind: CreatureType::Humanoid,
            weapon: Some(Weapon::TAPED_STAFF),
        }
    }

    /// Defias Thug (38): the male display 5035 at 1.0, level 3, a humanoid with a short sword.
    pub fn defias() -> Self {
        Self {
            display: 5035,
            scale: 1.0,
            level: 3,
            kind: CreatureType::Humanoid,
            weapon: Some(Weapon::SHORT_SWORD),
        }
    }

    /// The look a creature has by its name, as its 1.12 entry does; any other name is a wolf.
    pub fn named(name: &str) -> Self {
        match name {
            "Kobold Vermin" => Self::kobold(),
            "Defias Thug" => Self::defias(),
            _ => Self::wolf(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_creature_looks_like_its_name_and_anything_else_like_a_wolf() {
        let young_wolf = CreatureLook::named("Young Wolf");
        assert_eq!((young_wolf.display, young_wolf.level), (447, 1));
        assert_eq!(young_wolf.scale, 0.4, "the wolf model's native size");
        assert_eq!(young_wolf.kind, CreatureType::Beast);
        assert_eq!(young_wolf.weapon, None);

        let kobold = CreatureLook::named("Kobold Vermin");
        assert_eq!(
            (kobold.display, kobold.level, kobold.scale),
            (10913, 1, 0.7)
        );
        assert_eq!(kobold.kind, CreatureType::Humanoid);
        assert_eq!(kobold.weapon.map(|w| w.display), Some(5010));

        let defias = CreatureLook::named("Defias Thug");
        assert_eq!((defias.display, defias.level, defias.scale), (5035, 3, 1.0));
        assert_eq!(defias.kind, CreatureType::Humanoid);
        assert_eq!(defias.weapon.map(|w| w.display), Some(1593));

        assert_eq!(CreatureLook::named("Mangy Wolf"), CreatureLook::wolf());
        assert_eq!(CreatureLook::named(""), CreatureLook::wolf());
    }

    #[test]
    fn every_listed_kind_and_weapon_name_is_known() {
        for name in CREATURE_TYPE_NAMES {
            assert!(CreatureType::named(name).is_some(), "{name}");
        }
        for name in WEAPON_NAMES {
            assert!(Weapon::named(name).is_some(), "{name}");
        }
        assert_eq!(CreatureType::named("dragon"), None);
        assert_eq!(Weapon::named("axe"), None);
        assert_eq!(Weapon::named("none"), Some(None));
    }
}
