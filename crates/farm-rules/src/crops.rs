//! What grows on the farm: Azeroth's herbs and garden crops, how long each takes, what its seeds
//! cost and what the harvest sells for. Prices are copper.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Crop {
    Silverleaf,
    Peacebloom,
    Earthroot,
    Mageroyal,
    Briarthorn,
    Pumpkin,
    Watermelon,
}

/// A crop's numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropDef {
    pub name: &'static str,
    /// Watered days from planting to ripe.
    pub days: u32,
    /// One seed packet at the general store.
    pub seed_price: u32,
    /// One harvested crop at the vendor.
    pub sell_price: u32,
    /// Crops one ripe plant gives.
    pub harvest: u32,
}

impl Crop {
    pub const ALL: [Crop; 7] = [
        Crop::Silverleaf,
        Crop::Peacebloom,
        Crop::Earthroot,
        Crop::Mageroyal,
        Crop::Briarthorn,
        Crop::Pumpkin,
        Crop::Watermelon,
    ];

    pub fn def(self) -> CropDef {
        let (name, days, seed_price, sell_price, harvest) = match self {
            Crop::Silverleaf => ("Silverleaf", 2, 10, 9, 2),
            Crop::Peacebloom => ("Peacebloom", 3, 15, 12, 2),
            Crop::Earthroot => ("Earthroot", 4, 30, 22, 2),
            Crop::Mageroyal => ("Mageroyal", 5, 60, 40, 2),
            Crop::Briarthorn => ("Briarthorn", 6, 90, 55, 2),
            Crop::Pumpkin => ("Pumpkin", 7, 150, 220, 1),
            Crop::Watermelon => ("Watermelon", 9, 250, 400, 1),
        };
        CropDef {
            name,
            days,
            seed_price,
            sell_price,
            harvest,
        }
    }

    pub fn name(self) -> &'static str {
        self.def().name
    }

    /// How far along a plant of `age` watered days is, 0.0 to 1.0.
    pub fn growth(self, age: u32) -> f32 {
        (age as f32 / self.def().days as f32).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_crop_earns_more_than_its_seed_and_slower_crops_earn_more() {
        let mut last_profit = 0;
        for crop in Crop::ALL {
            let d = crop.def();
            let profit = d.sell_price * d.harvest - d.seed_price;
            assert!(profit > 0, "{} loses money", d.name);
            assert!(profit >= last_profit, "{} earns less than a faster crop", d.name);
            last_profit = profit;
        }
    }

    #[test]
    fn growth_runs_from_seed_to_ripe() {
        assert_eq!(Crop::Peacebloom.growth(0), 0.0);
        assert_eq!(Crop::Peacebloom.growth(3), 1.0);
        assert_eq!(Crop::Peacebloom.growth(9), 1.0);
    }
}
