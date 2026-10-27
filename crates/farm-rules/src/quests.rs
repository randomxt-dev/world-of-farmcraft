//! The quest chain that walks a new farmer from a shovel and a pouch of seeds to a full farm: each
//! quest is given by one villager, turned in to one, and unlocks the next thing to do. The server
//! shows them through WoW's own quest windows, log and `!`/`?` markers.

use serde::{Deserialize, Serialize};

use crate::bag::Item;
use crate::crops::Crop;

/// Something a quest or the store unlocks for good.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Unlock {
    WateringCan,
    Horse,
    /// Innkeeper Farley buys produce.
    Market,
    /// The Shipping Crate by the farmhouse, emptied by Farley's cart each morning.
    ShippingCrate,
    Axe,
    Fence,
    /// The pair of hens in the pen.
    Hens,
    /// The cow and the sheep.
    Herd,
    MiningPick,
    /// Till Soil, to dig plots beyond the first patch.
    Tilling,
}

/// The villagers who give and take quests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Villager {
    /// Ma Stonefield, by the farmhouse.
    Ma,
    /// Innkeeper Farley at the Lion's Pride Inn, who buys produce.
    Farley,
    /// Brog Hamfist at the Lion's Pride Inn, who sells seeds and tools.
    Brog,
}

/// Things the farmer does that quests count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deed {
    Plant,
    Water,
    Sleep,
    BuySeeds,
    BuildFence,
    GatherEgg,
    Mine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// Do the deed this many times, counted while the quest is in the log.
    Do(Deed, u32, &'static str),
    /// Hand over these items at the turn-in.
    Bring(Item, u32),
    /// Only go and see the turn-in villager.
    Visit,
}

#[derive(Clone, Copy, Debug)]
pub struct Quest {
    pub title: &'static str,
    pub giver: Villager,
    pub ender: Villager,
    pub goal: Goal,
    /// Copper paid at the turn-in.
    pub money: u32,
    pub unlocks: Option<Unlock>,
    /// The giver's words when offering it.
    pub details: &'static str,
    /// The one-line summary in the quest log.
    pub objectives: &'static str,
    /// The turn-in villager's words while it is not done yet.
    pub progress: &'static str,
    /// The turn-in villager's words when it is done.
    pub reward: &'static str,
}

/// The chain, in order: each quest opens once the one before it is turned in.
pub const QUESTS: [Quest; 10] = [
    Quest {
        title: "Breaking Ground",
        giver: Villager::Ma,
        ender: Villager::Ma,
        goal: Goal::Do(Deed::Plant, 8, "Seeds planted"),
        money: 0,
        unlocks: Some(Unlock::WateringCan),
        details:
            "So you're the one taking on the old Stonefield field! It's been fallow since the \
                  Defias ran us off, but the soil remembers.$B$BYou've a shovel and a pouch of \
                  Silverleaf seeds. Pick the seeds on your bar, then right-click the dirt mounds \
                  in the field to sow them. All eight, mind.",
        objectives: "Plant your 8 Silverleaf seeds in the mounds of the Stonefield field.",
        progress: "Still some seeds in that pouch?",
        reward: "Now that's a proper start! Here, take my old watering can. Seeds won't sprout on \
                 good intentions.",
    },
    Quest {
        title: "Thirsty Work",
        giver: Villager::Ma,
        ender: Villager::Ma,
        goal: Goal::Do(Deed::Water, 8, "Crops watered"),
        money: 0,
        unlocks: None,
        details: "A sprout that isn't watered doesn't grow, and that's the whole of farming in a \
                  sentence.$B$BPick the watering can on your bar and right-click each planted \
                  mound. Once a day is enough, and on a rainy day the sky does it for you.",
        objectives: "Water 8 of your crops.",
        progress: "The ones you missed look mighty thirsty.",
        reward: "Good, good. They'll be stretching by morning.",
    },
    Quest {
        title: "Early to Bed",
        giver: Villager::Ma,
        ender: Villager::Ma,
        goal: Goal::Do(Deed::Sleep, 1, "Nights slept in your bed"),
        money: 0,
        unlocks: Some(Unlock::Horse),
        details: "Crops grow overnight, and so do farmers. Your bed's inside the farmhouse: \
                  go in and type /sleep, and you'll wake in the morning.$B$BStay up past two and \
                  you'll drop where you stand, and Goldshire's healer doesn't work for free.",
        objectives: "Type /sleep inside the Stonefield farmhouse, then see Ma Stonefield.",
        progress: "Bed's inside, dear. Through the door.",
        reward: "Rested? Good. Now, the old farm horse has been eating his head off by the \
                 house. He's yours: right-click him to ride, or press 5 anywhere, and again to \
                 get down. He'll get you to Goldshire and back twice as quick as your feet.",
    },
    Quest {
        title: "Market Day",
        giver: Villager::Ma,
        ender: Villager::Farley,
        goal: Goal::Visit,
        money: 0,
        unlocks: Some(Unlock::Market),
        details: "A farm that doesn't sell is a garden. Ride up the road north-east to Goldshire \
                  and find Innkeeper Farley at the Lion's Pride Inn.$B$BTell him the Stonefield \
                  farm is growing again. He buys good produce for his kitchen.",
        objectives: "Ride to Goldshire and speak to Innkeeper Farley at the Lion's Pride Inn.",
        progress: "",
        reward: "Stonefield produce again? Light be praised, the stew's been all turnip since \
                 spring. Bring me what you grow and I'll pay fair coin for it.",
    },
    Quest {
        title: "Seeds for Spring",
        giver: Villager::Farley,
        ender: Villager::Brog,
        goal: Goal::Do(Deed::BuySeeds, 10, "Seeds bought"),
        money: 0,
        unlocks: Some(Unlock::Tilling),
        details: "One field of Silverleaf won't fill my larder. Brog Hamfist, right here in the \
                  inn, sells seeds of every sort.$B$BBuy ten from him. Peacebloom grows quick and \
                  Earthroot sells better, if you've the coin.",
        objectives: "Buy 10 seeds from Brog Hamfist in the Lion's Pride Inn.",
        progress: "Seeds are on the shelf, friend. Have a look.",
        reward: "Farley sent you? Then you'll be back, and I like customers who come back. \
                 Ten more seeds want ten more mounds: take this old hoe. Pick Till Soil on \
                 your bar and click the furrows to dig new ones.",
    },
    Quest {
        title: "The First Harvest",
        giver: Villager::Brog,
        ender: Villager::Farley,
        goal: Goal::Bring(Item::Produce(Crop::Silverleaf), 10),
        money: 90,
        unlocks: Some(Unlock::ShippingCrate),
        details: "Farley's waiting on that first harvest of yours, and so's my lunch.\
                  $B$BSilverleaf ripens in two days if you water it each morning. Right-click a \
                  ripe one to pick it, then bring Farley ten.",
        objectives: "Bring 10 Silverleaf to Innkeeper Farley in Goldshire.",
        progress: "Is that the Silverleaf I smell?",
        reward: "Beautiful! Tell you what: leave your goods in the crate by your farmhouse and \
                 my cart will fetch them every morning and pay you for them. Saves you the \
                 ride.",
    },
    Quest {
        title: "Timber!",
        giver: Villager::Brog,
        ender: Villager::Ma,
        goal: Goal::Bring(Item::Wood, 4),
        money: 0,
        unlocks: Some(Unlock::Fence),
        details: "A farm needs fences, and fences need wood. I've a Woodcutter's Axe on the \
                  shelf for one silver.$B$BBuy it, chop one of those Westfall oaks around your \
                  farm, and bring Ma Stonefield the wood. She'll know what to do with it.",
        objectives: "Buy a Woodcutter's Axe from Brog Hamfist, then bring 4 Simple Wood to Ma \
                     Stonefield.",
        progress: "Got the wood, dear?",
        reward: "Fine split oak! My Pa's fence mallet is yours. Two pieces of wood make a section \
                 of fence.",
    },
    Quest {
        title: "Good Fences",
        giver: Villager::Ma,
        ender: Villager::Ma,
        goal: Goal::Do(Deed::BuildFence, 8, "Fence sections built"),
        money: 0,
        unlocks: Some(Unlock::Hens),
        details: "Good fences make good neighbors, and they keep the hens out of the cabbages.\
                  $B$BPick Build Fence on your bar, click where a fence starts and click where it \
                  ends. Put up eight sections and I'll have something for you.",
        objectives: "Build 8 sections of fence.",
        progress: "Coming along, those fences?",
        reward: "Sturdy! Here, I've kept a pair of hens for when we had a pen again. They're \
                 out by the barn now.",
    },
    Quest {
        title: "Eggs for Breakfast",
        giver: Villager::Ma,
        ender: Villager::Ma,
        goal: Goal::Do(Deed::GatherEgg, 3, "Eggs gathered"),
        money: 0,
        unlocks: Some(Unlock::Herd),
        details: "Hens lay every morning. Right-click one to take its egg; you'll get one a day \
                  from each.$B$BBring in three and we'll talk about something bigger.",
        objectives: "Gather 3 eggs from your hens.",
        progress: "They lay once a day, dear. Patience.",
        reward: "Lovely. Homer's brought the cow and the sheep back from the Eastvale pasture. \
                 Milk them and shear them each day.",
    },
    Quest {
        title: "Into Fargodeep",
        giver: Villager::Brog,
        ender: Villager::Brog,
        goal: Goal::Do(Deed::Mine, 5, "Copper Ore mined"),
        money: 0,
        unlocks: None,
        details: "There's copper in Fargodeep Mine, south-west of your farm, and the kobolds \
                  can't dig it all.$B$BI sell a Mining Pick for two silver. Buy one, mine five \
                  Copper Ore, and come tell me about it. Mind the candles.",
        objectives: "Buy a Mining Pick from Brog Hamfist and mine 5 Copper Ore in Fargodeep Mine.",
        progress: "Find the copper yet?",
        reward: "Now you're a farmer and a miner both. Farley pays for copper too, and one \
                 day a smith will want it for your tools.",
    },
];

/// What a villager shows over their head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    None,
    /// A gray `?`: a quest of theirs is not done yet.
    Waiting,
    /// A yellow `!`: they have a quest to give.
    Offer,
    /// A yellow `?`: a quest can be turned in to them.
    TurnIn,
}

/// The farmer's quests: those turned in, in order, and the ones in the log with their counts.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Quests {
    /// How many of the chain are turned in; the chain is done in order.
    pub done: usize,
    /// Quests in the log, by index into [`QUESTS`], with the count of their deed.
    pub active: Vec<(usize, u32)>,
}

impl Quests {
    pub fn is_active(&self, q: usize) -> bool {
        self.active.iter().any(|&(a, _)| a == q)
    }

    pub fn count(&self, q: usize) -> u32 {
        (self.active.iter().find(|&&(a, _)| a == q)).map_or(0, |&(_, n)| n)
    }

    /// The quest `who` can give now.
    pub fn offer(&self, who: Villager) -> Option<usize> {
        let next = self.done;
        (next < QUESTS.len() && QUESTS[next].giver == who && !self.is_active(next)).then_some(next)
    }

    /// Takes quest `q` into the log.
    pub fn accept(&mut self, q: usize) -> bool {
        if q != self.done || self.is_active(q) || q >= QUESTS.len() {
            return false;
        }
        self.active.push((q, 0));
        true
    }

    /// Counts `deed` for the quests in the log; the quests it moved on, with their new counts.
    pub fn note(&mut self, deed: Deed) -> Vec<(usize, u32)> {
        let mut moved = Vec::new();
        for (q, n) in &mut self.active {
            if let Goal::Do(d, need, _) = QUESTS[*q].goal {
                if d == deed && *n < need {
                    *n += 1;
                    moved.push((*q, *n));
                }
            }
        }
        moved
    }

    /// Takes quest `q` out of the log, undone.
    pub fn abandon(&mut self, q: usize) {
        self.active.retain(|&(a, _)| a != q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chain_runs_villager_to_villager_without_gaps() {
        for pair in QUESTS.windows(2) {
            assert!(!pair[0].title.is_empty() && !pair[1].title.is_empty());
        }
        assert_eq!(QUESTS[0].giver, Villager::Ma);
        assert_eq!(QUESTS[3].ender, Villager::Farley);
        assert_eq!(QUESTS[4].giver, Villager::Farley);
        assert_eq!(QUESTS[4].ender, Villager::Brog);
    }

    #[test]
    fn a_quest_is_offered_only_after_the_one_before_it() {
        let mut q = Quests::default();
        assert_eq!(q.offer(Villager::Ma), Some(0));
        assert_eq!(q.offer(Villager::Farley), None);
        assert!(!q.accept(1));
        assert!(q.accept(0));
        assert_eq!(q.offer(Villager::Ma), None);
        assert!(!q.accept(0));
    }

    #[test]
    fn deeds_count_only_for_their_quest_and_stop_at_the_need() {
        let mut q = Quests::default();
        assert!(q.note(Deed::Plant).is_empty());
        q.accept(0);
        assert!(q.note(Deed::Water).is_empty());
        for n in 1..=8 {
            assert_eq!(q.note(Deed::Plant), vec![(0, n)]);
        }
        assert!(q.note(Deed::Plant).is_empty());
        assert_eq!(q.count(0), 8);
    }
}
