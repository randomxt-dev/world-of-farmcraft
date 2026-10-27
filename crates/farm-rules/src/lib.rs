//! World of Farmcraft's rules as a pure state machine: the day clock and weather, the crops and the
//! plot they grow on, ore veins, the bag and money. No network, no Bevy, no game data; the server
//! drives it and shows it in the world.

pub mod bag;
pub mod clock;
pub mod crops;
pub mod farm;
pub mod quests;
pub mod rng;
pub mod save;
pub mod skills;

pub use bag::{Bag, Item};
pub use clock::{Clock, Season, Weather};
pub use crops::{Crop, CropDef};
pub use farm::{DayReport, Farm, Fence, Reject, Tile, TreeStage, FENCE_WOOD};
pub use quests::{quest, Deed, Goal, Marker, Quest, Quests, Unlock, Villager, QUESTS, STUDIES};
pub use skills::{Skill, Skills};
