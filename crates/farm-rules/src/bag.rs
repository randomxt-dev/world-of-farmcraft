//! What the farmer carries: seeds, harvested crops and ore, counted per kind and laid out in the
//! backpack's slots as stacks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::crops::Crop;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Item {
    Seeds(Crop),
    Produce(Crop),
    CopperOre,
    Wood,
    Egg,
    Milk,
    Wool,
}

/// Items in one backpack slot.
pub const STACK: u32 = 20;
/// Backpack slots for farm goods: the 16-slot backpack less the one the Mining Pick sits in.
pub const SLOTS: usize = 15;

impl Item {
    pub fn name(self) -> String {
        match self {
            Item::Seeds(c) => format!("{} Seeds", c.name()),
            Item::Produce(c) => c.name().to_owned(),
            Item::CopperOre => "Copper Ore".to_owned(),
            Item::Wood => "Simple Wood".to_owned(),
            Item::Egg => "Small Egg".to_owned(),
            Item::Milk => "Ice Cold Milk".to_owned(),
            Item::Wool => "Wool Cloth".to_owned(),
        }
    }

    /// What the general store pays for one.
    pub fn sell_price(self) -> u32 {
        match self {
            Item::Seeds(c) => c.def().seed_price / 2,
            Item::Produce(c) => c.def().sell_price,
            Item::CopperOre => 15,
            Item::Wood => 6,
            Item::Egg => 8,
            Item::Milk => 20,
            Item::Wool => 30,
        }
    }

    /// What the general store asks for one, if it sells it.
    pub fn buy_price(self) -> Option<u32> {
        match self {
            Item::Seeds(c) => Some(c.def().seed_price),
            _ => None,
        }
    }
}

/// Counts per item.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Bag(BTreeMap<Item, u32>);

impl Bag {
    pub fn count(&self, item: Item) -> u32 {
        self.0.get(&item).copied().unwrap_or(0)
    }

    /// Slots the bag fills, a stack of up to [`STACK`] each.
    pub fn slots_used(&self) -> usize {
        self.0.values().map(|&n| n.div_ceil(STACK) as usize).sum()
    }

    /// Whether `n` more of `item` fit in [`SLOTS`].
    pub fn fits(&self, item: Item, n: u32) -> bool {
        let have = self.count(item);
        let more = (have + n).div_ceil(STACK) as usize - have.div_ceil(STACK) as usize;
        self.slots_used() + more <= SLOTS
    }

    /// Add `n` of `item`, if they fit.
    pub fn add(&mut self, item: Item, n: u32) -> bool {
        if n == 0 || !self.fits(item, n) {
            return n == 0;
        }
        *self.0.entry(item).or_default() += n;
        true
    }

    /// Take `n` of `item`, if there are that many.
    pub fn take(&mut self, item: Item, n: u32) -> bool {
        let have = self.count(item);
        if have < n {
            return false;
        }
        if have == n {
            self.0.remove(&item);
        } else {
            self.0.insert(item, have - n);
        }
        true
    }

    /// The stacks in slot order: each item kind in order, full stacks first.
    pub fn stacks(&self) -> Vec<(Item, u32)> {
        let mut out = Vec::new();
        for (&item, &n) in &self.0 {
            let mut left = n;
            while left > 0 {
                let here = left.min(STACK);
                out.push((item, here));
                left -= here;
            }
        }
        out
    }

    /// The kinds of seed the bag holds.
    pub fn seeds(&self) -> Vec<(Crop, u32)> {
        (self.0.iter())
            .filter_map(|(item, &n)| match item {
                Item::Seeds(c) => Some((*c, n)),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_fill_slots_and_a_full_bag_refuses_more() {
        let mut bag = Bag::default();
        assert!(bag.add(Item::Seeds(Crop::Peacebloom), 25));
        assert_eq!(bag.slots_used(), 2);
        assert_eq!(
            bag.stacks(),
            vec![
                (Item::Seeds(Crop::Peacebloom), 20),
                (Item::Seeds(Crop::Peacebloom), 5)
            ]
        );
        assert!(bag.add(Item::CopperOre, STACK * 13));
        assert_eq!(bag.slots_used(), SLOTS);
        assert!(
            bag.add(Item::Seeds(Crop::Peacebloom), 15),
            "the open stack fills"
        );
        assert!(!bag.add(Item::Seeds(Crop::Peacebloom), 1));
        assert!(!bag.add(Item::Produce(Crop::Pumpkin), 1));
    }

    #[test]
    fn taking_more_than_there_is_takes_nothing() {
        let mut bag = Bag::default();
        bag.add(Item::CopperOre, 3);
        assert!(!bag.take(Item::CopperOre, 4));
        assert!(bag.take(Item::CopperOre, 3));
        assert_eq!(bag.count(Item::CopperOre), 0);
        assert!(bag.stacks().is_empty());
    }
}
