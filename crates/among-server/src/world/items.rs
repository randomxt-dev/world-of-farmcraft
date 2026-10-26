//! Class starting gear as 1.12.1 hands it out: each race and class's worn starting items, their
//! templates, the item objects the owner is sent and the `SMSG_ITEM_QUERY_SINGLE_RESPONSE` every
//! client asks for. The data is vmangos's world database (`playercreateinfo_item` and
//! `item_template`, newest row at patch 10 or below) from the `db_latest` release
//! `db-4641790.zip` of https://github.com/vmangos/core. Bag items (food, water, the Hearthstone,
//! quivers, pouches, ammo) are left out: only what is worn, plus the Mining Pick every player carries
//! so the client lets them cast Mining (its Totem is a pick in the bags, not one worn).

use benilla_protocol::messages::opcode;

use super::socket::Packet;
use super::update::{self, Fields};
use super::wire::Body;

/// The `item_template` columns the client reads. Every starting item has no flags, stats, spells,
/// resistances, bonding or class and race limits, so those go out as zero or `-1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemTemplate {
    pub entry: u32,
    pub name: &'static str,
    pub class: u8,
    pub subclass: u8,
    /// `ItemDisplayInfo.dbc` id: icon and model.
    pub display_id: u32,
    pub quality: u8,
    pub buy_price: u32,
    pub sell_price: u32,
    pub inventory_type: u8,
    pub item_level: u8,
    pub required_level: u8,
    pub stackable: u16,
    pub delay_ms: u32,
    pub range_mod: f32,
    pub ammo_type: u8,
    /// Physical damage, min and max.
    pub damage: [f32; 2],
    pub armor: u32,
    pub block: u32,
    pub material: i8,
    pub sheath: u8,
    pub max_durability: u32,
}

macro_rules! templates {
    ($($entry:literal, $name:literal, $class:literal, $sub:literal, $display:literal,
       $quality:literal, $buy:literal, $sell:literal, $inv:literal, $ilvl:literal, $req:literal,
       $stack:literal, $delay:literal, $range:literal, $ammo:literal, $min:literal, $max:literal,
       $armor:literal, $block:literal, $material:literal, $sheath:literal, $dur:literal;)*) => {
        &[$(ItemTemplate {
            entry: $entry,
            name: $name,
            class: $class,
            subclass: $sub,
            display_id: $display,
            quality: $quality,
            buy_price: $buy,
            sell_price: $sell,
            inventory_type: $inv,
            item_level: $ilvl,
            required_level: $req,
            stackable: $stack,
            delay_ms: $delay,
            range_mod: $range,
            ammo_type: $ammo,
            damage: [$min, $max],
            armor: $armor,
            block: $block,
            material: $material,
            sheath: $sheath,
            max_durability: $dur,
        },)*]
    };
}

/// Every worn starting item, by entry.
pub const TEMPLATES: &[ItemTemplate] = templates! {
    // entry, name, class, subclass, display, quality, buy, sell, inventory type, item level,
    // required level, stackable, delay ms, range mod, ammo type, damage min, damage max, armor,
    // block, material, sheath, max durability
    25, "Worn Shortsword", 2, 7, 1542, 1, 35, 7, 21, 2, 1, 1, 1900, 0.0, 0, 1.0, 3.0, 0, 0, 1, 3, 20;
    35, "Bent Staff", 2, 10, 472, 1, 47, 9, 17, 2, 1, 1, 2900, 0.0, 0, 3.0, 5.0, 0, 0, 2, 2, 25;
    36, "Worn Mace", 2, 4, 5194, 1, 38, 7, 21, 2, 1, 1, 1900, 0.0, 0, 1.0, 3.0, 0, 0, 2, 3, 20;
    37, "Worn Axe", 2, 0, 14029, 1, 38, 7, 21, 2, 1, 1, 2000, 0.0, 0, 1.0, 3.0, 0, 0, 1, 3, 20;
    38, "Recruit's Shirt", 4, 0, 9891, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    39, "Recruit's Pants", 4, 1, 9892, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    40, "Recruit's Boots", 4, 0, 10141, 1, 4, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    43, "Squire's Boots", 4, 0, 10272, 1, 4, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    44, "Squire's Pants", 4, 1, 9937, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    45, "Squire's Shirt", 4, 0, 3265, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    47, "Footpad's Shoes", 4, 0, 9915, 1, 4, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    48, "Footpad's Pants", 4, 1, 9913, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    49, "Footpad's Shirt", 4, 0, 9906, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    51, "Neophyte's Boots", 4, 0, 9946, 1, 5, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    52, "Neophyte's Pants", 4, 1, 9945, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    53, "Neophyte's Shirt", 4, 0, 9944, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    55, "Apprentice's Boots", 4, 0, 9929, 1, 5, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    56, "Apprentice's Robe", 4, 1, 12647, 0, 5, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    57, "Acolyte's Robe", 4, 1, 12645, 0, 5, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    59, "Acolyte's Shoes", 4, 0, 3261, 1, 5, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    120, "Thug Pants", 4, 1, 10006, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    121, "Thug Boots", 4, 0, 10008, 1, 4, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    127, "Trapper's Shirt", 4, 0, 9996, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    129, "Rugged Trapper's Boots", 4, 0, 9977, 1, 5, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    139, "Brawler's Pants", 4, 1, 9988, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    140, "Brawler's Boots", 4, 0, 9992, 1, 4, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    147, "Rugged Trapper's Pants", 4, 1, 9975, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    148, "Rugged Trapper's Shirt", 4, 0, 9976, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    153, "Primitive Kilt", 4, 2, 10050, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 14, 0, 8, 0, 30;
    154, "Primitive Mantle", 4, 0, 10058, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    1395, "Apprentice's Pants", 4, 1, 9924, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    1396, "Acolyte's Pants", 4, 1, 3260, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    2092, "Worn Dagger", 2, 15, 6442, 1, 35, 7, 13, 2, 1, 1, 1600, 0.0, 0, 1.0, 2.0, 0, 0, 1, 3, 16;
    2105, "Thug Shirt", 4, 0, 10005, 1, 5, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    2361, "Battleworn Hammer", 2, 5, 8690, 1, 45, 9, 17, 2, 1, 1, 2900, 0.0, 0, 3.0, 5.0, 0, 0, 2, 1, 25;
    2362, "Worn Wooden Shield", 4, 6, 18730, 0, 7, 1, 14, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 5, 1, 1, 4, 20;
    2504, "Worn Shortbow", 2, 2, 8106, 1, 29, 5, 15, 2, 1, 1, 2300, 100.0, 2, 2.0, 5.0, 0, 0, 2, 0, 20;
    2508, "Old Blunderbuss", 2, 3, 6606, 1, 27, 5, 26, 2, 1, 1, 2300, 100.0, 3, 2.0, 5.0, 0, 0, 1, 0, 20;
    2947, "Small Throwing Knife", 2, 16, 16754, 1, 15, 0, 25, 3, 1, 200, 2000, 100.0, 4, 1.0, 3.0, 0, 0, 1, 0, 0;
    3111, "Crude Throwing Axe", 2, 16, 20777, 1, 15, 0, 25, 3, 1, 200, 2000, 100.0, 4, 1.0, 3.0, 0, 0, 1, 0, 0;
    3661, "Handcrafted Staff", 2, 10, 18530, 1, 45, 9, 17, 2, 1, 1, 2900, 0.0, 0, 3.0, 5.0, 0, 0, 2, 2, 25;
    6096, "Apprentice's Shirt", 4, 0, 2163, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6097, "Acolyte's Shirt", 4, 0, 2470, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6098, "Neophyte's Robe", 4, 1, 12679, 0, 4, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    6117, "Squire's Shirt", 4, 0, 9972, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6118, "Squire's Pants", 4, 1, 9974, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    6119, "Neophyte's Robe", 4, 1, 12681, 0, 4, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    6120, "Recruit's Shirt", 4, 0, 9983, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6121, "Recruit's Pants", 4, 1, 9984, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    6122, "Recruit's Boots", 4, 0, 9985, 1, 4, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6123, "Novice's Robe", 4, 1, 12683, 0, 4, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    6124, "Novice's Pants", 4, 1, 9987, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    6125, "Brawler's Harness", 4, 0, 9995, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6126, "Trapper's Pants", 4, 1, 10002, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    6127, "Trapper's Boots", 4, 0, 10003, 1, 5, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6129, "Acolyte's Robe", 4, 1, 12646, 0, 5, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    6134, "Primitive Mantle", 4, 0, 10108, 1, 1, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6135, "Primitive Kilt", 4, 2, 10109, 0, 5, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 14, 0, 8, 0, 30;
    6136, "Thug Shirt", 4, 0, 10112, 1, 4, 1, 4, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6137, "Thug Pants", 4, 1, 10114, 0, 4, 1, 7, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 2, 0, 7, 0, 25;
    6138, "Thug Boots", 4, 0, 10115, 1, 4, 1, 8, 1, 0, 1, 0, 0.0, 0, 0.0, 0.0, 0, 0, 7, 0, 0;
    6139, "Novice's Robe", 4, 1, 12684, 0, 4, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    6140, "Apprentice's Robe", 4, 1, 12649, 0, 4, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    6144, "Neophyte's Robe", 4, 1, 12680, 0, 5, 1, 20, 1, 1, 1, 0, 0.0, 0, 0.0, 0.0, 3, 0, 7, 0, 35;
    2901, "Mining Pick", 2, 14, 6568, 1, 81, 16, 21, 4, 1, 1, 2000, 0.0, 0, 2.0, 4.0, 0, 0, 1, 0, 0;
    12282, "Worn Battleaxe", 2, 1, 22291, 1, 43, 8, 17, 2, 1, 1, 2900, 0.0, 0, 3.0, 5.0, 0, 0, 1, 1, 25;
};

/// One race and class's worn starting items, `(entry, count)` each.
type Kit = &'static [(u32, u32)];

/// `playercreateinfo_item` for the 40 race and class pairs 1.12 allows, worn items only, as
/// `(race, class, kit)`; a thrown stack is equipped whole (`StoreNewItemInBestSlots`).
#[rustfmt::skip]
const START_GEAR: &[(u8, u8, Kit)] = &[
    (1, 1, &[(38, 1), (39, 1), (40, 1), (25, 1), (2362, 1)]),
    (1, 2, &[(45, 1), (44, 1), (43, 1), (2361, 1)]),
    (1, 4, &[(49, 1), (48, 1), (47, 1), (2092, 1), (2947, 100)]),
    (1, 5, &[(53, 1), (6098, 1), (52, 1), (51, 1), (36, 1)]),
    (1, 8, &[(6096, 1), (56, 1), (1395, 1), (55, 1), (35, 1)]),
    (1, 9, &[(6097, 1), (57, 1), (1396, 1), (59, 1), (2092, 1)]),
    (2, 1, &[(6125, 1), (139, 1), (140, 1), (12282, 1)]),
    (2, 3, &[(127, 1), (6126, 1), (6127, 1), (37, 1), (2504, 1)]),
    (2, 4, &[(2105, 1), (120, 1), (121, 1), (2092, 1), (3111, 100)]),
    (2, 7, &[(154, 1), (153, 1), (36, 1)]),
    (2, 9, &[(6129, 1), (1396, 1), (59, 1), (2092, 1)]),
    (3, 1, &[(38, 1), (39, 1), (40, 1), (12282, 1)]),
    (3, 2, &[(6117, 1), (6118, 1), (43, 1), (2361, 1)]),
    (3, 3, &[(148, 1), (147, 1), (129, 1), (37, 1), (2508, 1)]),
    (3, 4, &[(49, 1), (48, 1), (47, 1), (2092, 1), (3111, 100)]),
    (3, 5, &[(53, 1), (6098, 1), (52, 1), (51, 1), (36, 1)]),
    (4, 1, &[(6120, 1), (6121, 1), (6122, 1), (25, 1), (2362, 1)]),
    (4, 3, &[(148, 1), (147, 1), (129, 1), (2092, 1), (2504, 1)]),
    (4, 4, &[(49, 1), (48, 1), (47, 1), (2092, 1), (2947, 100)]),
    (4, 5, &[(53, 1), (6119, 1), (52, 1), (51, 1), (36, 1)]),
    (4, 11, &[(6123, 1), (6124, 1), (3661, 1)]),
    (5, 1, &[(6125, 1), (139, 1), (140, 1), (25, 1), (2362, 1)]),
    (5, 4, &[(2105, 1), (120, 1), (121, 1), (2092, 1), (2947, 100)]),
    (5, 5, &[(53, 1), (6144, 1), (52, 1), (51, 1), (36, 1)]),
    (5, 8, &[(6096, 1), (6140, 1), (1395, 1), (55, 1), (35, 1)]),
    (5, 9, &[(6129, 1), (1396, 1), (59, 1), (2092, 1)]),
    (6, 1, &[(6125, 1), (139, 1), (2361, 1)]),
    (6, 3, &[(127, 1), (6126, 1), (37, 1), (2508, 1)]),
    (6, 7, &[(154, 1), (153, 1), (36, 1)]),
    (6, 11, &[(6139, 1), (6124, 1), (35, 1)]),
    (7, 1, &[(38, 1), (39, 1), (40, 1), (25, 1), (2362, 1)]),
    (7, 4, &[(49, 1), (48, 1), (47, 1), (2092, 1), (2947, 100)]),
    (7, 8, &[(6096, 1), (56, 1), (1395, 1), (55, 1), (35, 1)]),
    (7, 9, &[(6097, 1), (57, 1), (1396, 1), (59, 1), (2092, 1)]),
    (8, 1, &[(6125, 1), (139, 1), (37, 1), (2362, 1), (3111, 200)]),
    (8, 3, &[(127, 1), (6126, 1), (37, 1), (2504, 1)]),
    (8, 4, &[(6136, 1), (6137, 1), (6138, 1), (2092, 1), (3111, 100)]),
    (8, 5, &[(53, 1), (6144, 1), (52, 1), (36, 1)]),
    (8, 7, &[(6134, 1), (6135, 1), (36, 1)]),
    (8, 8, &[(6096, 1), (6140, 1), (1395, 1), (55, 1), (35, 1)]),
];

/// The 19 equipment slots, head (0) to tabard (18).
pub const EQUIPMENT_SLOTS: u8 = 19;
/// `EQUIPMENT_SLOT_MAINHAND`.
pub const MAINHAND: u8 = 15;

/// The template of `entry`, if it is a starting item.
pub fn template(entry: u32) -> Option<&'static ItemTemplate> {
    TEMPLATES.iter().find(|t| t.entry == entry)
}

/// The equipment slot an `InventoryType` first goes to (vmangos `Player::FindEquipSlot`, empty
/// slots, no dual wield); `None` for what is not worn.
pub fn equip_slot(inventory_type: u8) -> Option<u8> {
    Some(match inventory_type {
        1 => 0,             // head
        2 => 1,             // neck
        3 => 2,             // shoulders
        4 => 3,             // shirt
        5 | 20 => 4,        // chest, robe
        6 => 5,             // waist
        7 => 6,             // legs
        8 => 7,             // feet
        9 => 8,             // wrists
        10 => 9,            // hands
        11 => 10,           // finger
        12 => 12,           // trinket
        16 => 14,           // back
        13 | 17 | 21 => 15, // one-hand, two-hand, main hand
        14 | 22 | 23 => 16, // shield, off hand, held in off hand
        15 | 25 | 26 => 17, // ranged, thrown, gun or wand
        19 => 18,           // tabard
        _ => return None,
    })
}

/// One item the player holds, worn or in the backpack: its slot, its object guid, its template
/// and its stack count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Worn {
    pub slot: u8,
    pub guid: u64,
    pub item: &'static ItemTemplate,
    pub count: u32,
}

/// `HIGHGUID_ITEM`.
const HIGH_ITEM: u64 = 0x4000;

/// The item worn in `slot` by the player `owner`: one per slot, so the owner's low word and the
/// slot make it unique.
pub fn item_guid(owner: u64, slot: u8) -> u64 {
    HIGH_ITEM << 48 | (owner & 0xFFFF_FFFF) << 8 | u64::from(slot)
}

/// `item_template` 2901, Mining's Totem.
pub const MINING_PICK: u32 = 2901;
/// `INVENTORY_SLOT_ITEM_START`: the backpack's first slot.
pub const BACKPACK: u8 = 23;

/// What every player carries in the backpack: the Mining Pick, in its first slot.
pub fn carried(owner: u64) -> Vec<Worn> {
    template(MINING_PICK)
        .map(|item| Worn {
            slot: BACKPACK,
            guid: item_guid(owner, BACKPACK),
            item,
            count: 1,
        })
        .into_iter()
        .collect()
}

/// What a `race` `class` player starts wearing, by slot; nothing for a pair 1.12 does not allow.
pub fn starting_gear(race: u8, class: u8, owner: u64) -> Vec<Worn> {
    let Some((_, _, kit)) = START_GEAR
        .iter()
        .find(|(r, c, _)| (*r, *c) == (race, class))
    else {
        return Vec::new();
    };
    let mut worn: Vec<Worn> = kit
        .iter()
        .filter_map(|&(entry, count)| {
            let item = template(entry)?;
            let slot = equip_slot(item.inventory_type)?;
            Some(Worn {
                slot,
                guid: item_guid(owner, slot),
                item,
                count,
            })
        })
        .collect();
    worn.sort_by_key(|w| w.slot);
    worn
}

// Item descriptor indices for build 5875 (vmangos `UpdateFields_1_12_1.h`).
const OBJECT_FIELD_GUID: u16 = 0;
const OBJECT_FIELD_TYPE: u16 = 2;
const OBJECT_FIELD_ENTRY: u16 = 3;
const OBJECT_FIELD_SCALE_X: u16 = 4;
const ITEM_FIELD_OWNER: u16 = 6;
const ITEM_FIELD_CONTAINED: u16 = 8;
const ITEM_FIELD_STACK_COUNT: u16 = 14;
const ITEM_FIELD_DURABILITY: u16 = 46;
const ITEM_FIELD_MAXDURABILITY: u16 = 47;
/// `TYPEMASK_OBJECT | TYPEMASK_ITEM`.
const TYPE_ITEM: u32 = 0x01 | 0x02;

/// The worn item's descriptor: an equipped item is contained by its owner (`Player::EquipItem`)
/// and starts at full durability (`Item::Create`).
pub fn fields(w: &Worn, owner: u64) -> Fields {
    let mut f = Fields::default();
    f.u64(OBJECT_FIELD_GUID, w.guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_ITEM)
        .u32(OBJECT_FIELD_ENTRY, w.item.entry)
        .f32(OBJECT_FIELD_SCALE_X, 1.0)
        .u64(ITEM_FIELD_OWNER, owner)
        .u64(ITEM_FIELD_CONTAINED, owner)
        .u32(ITEM_FIELD_STACK_COUNT, w.count);
    if w.item.max_durability > 0 {
        f.u32(ITEM_FIELD_DURABILITY, w.item.max_durability)
            .u32(ITEM_FIELD_MAXDURABILITY, w.item.max_durability);
    }
    f
}

/// The create block of the worn item, sent to its owner only.
pub fn create_block(w: &Worn, owner: u64) -> Vec<u8> {
    update::create_item(w.guid, &fields(w, owner))
}

/// `SMSG_ITEM_QUERY_SINGLE_RESPONSE` for `entry` in vmangos `HandleItemQuerySingleOpcode`'s
/// order at build 5875; an unknown entry is the lone `entry | 0x8000_0000`.
pub fn query_response(entry: u32) -> Packet {
    let mut b = Body::default();
    let Some(t) = template(entry) else {
        b.u32(entry | 0x8000_0000);
        return Packet::new(opcode::SMSG_ITEM_QUERY_SINGLE_RESPONSE, b.done());
    };
    b.u32(t.entry)
        .u32(t.class.into())
        .u32(t.subclass.into())
        .cstring(t.name)
        .cstring("")
        .cstring("")
        .cstring("")
        .u32(t.display_id)
        .u32(t.quality.into())
        .u32(0) // flags
        .u32(t.buy_price)
        .u32(t.sell_price)
        .u32(t.inventory_type.into())
        .u32(u32::MAX) // allowable class: any
        .u32(u32::MAX) // allowable race: any
        .u32(t.item_level.into())
        .u32(t.required_level.into());
    // Required skill and rank, spell, honor rank, city rank, reputation faction and rank,
    // max count.
    for _ in 0..8 {
        b.u32(0);
    }
    b.u32(t.stackable.into()).u32(0); // container slots
    for _ in 0..10 {
        b.u32(0).u32(0); // stat type, value
    }
    b.f32(t.damage[0]).f32(t.damage[1]).u32(0); // physical
    for _ in 0..4 {
        b.f32(0.0).f32(0.0).u32(0);
    }
    b.u32(t.armor);
    for _ in 0..6 {
        b.u32(0); // holy, fire, nature, frost, shadow, arcane
    }
    b.u32(t.delay_ms).u32(t.ammo_type.into()).f32(t.range_mod);
    for _ in 0..5 {
        // An empty spell block: id, trigger, charges, cooldown -1, category, category cooldown -1.
        b.u32(0).u32(0).u32(0).u32(u32::MAX).u32(0).u32(u32::MAX);
    }
    b.u32(0) // bonding
        .cstring("") // description
        .u32(0) // page text
        .u32(0) // language
        .u32(0) // page material
        .u32(0) // start quest
        .u32(0) // lock
        .u32(t.material as u32)
        .u32(t.sheath.into())
        .u32(0) // random property
        .u32(t.block)
        .u32(0) // item set
        .u32(t.max_durability)
        .u32(0) // area
        .u32(0) // map
        .u32(0); // bag family
    Packet::new(opcode::SMSG_ITEM_QUERY_SINGLE_RESPONSE, b.done())
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};

    /// Every 1.12 race and class pair: 6 human, 5 orc, 5 dwarf, 5 night elf, 5 undead, 4 tauren,
    /// 4 gnome, 6 troll.
    const LEGAL: [(u8, &[u8]); 8] = [
        (1, &[1, 2, 4, 5, 8, 9]),
        (2, &[1, 3, 4, 7, 9]),
        (3, &[1, 2, 3, 4, 5]),
        (4, &[1, 3, 4, 5, 11]),
        (5, &[1, 4, 5, 8, 9]),
        (6, &[1, 3, 7, 11]),
        (7, &[1, 4, 8, 9]),
        (8, &[1, 3, 4, 5, 7, 8]),
    ];

    fn parse(p: &Packet) -> ServerPacket {
        let (packet, tail) =
            parse_server_with_tail(p.opcode, &p.body).expect("the client parses it");
        assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
        packet
    }

    /// Each pair wears one item per slot, every entry has a template, and the main hand holds a
    /// melee weapon Kill's `Spell.dbc` row accepts: `EquippedItemClass` 2 (weapon) with
    /// `EquippedItemSubClassMask` `0x2a5f3` (axes, maces, swords, polearm, staff, fist, dagger,
    /// spear), as `benilla-formats`' `spell_row` example reads 15692.
    #[test]
    fn every_class_starts_armed_for_kill() {
        assert_eq!(START_GEAR.len(), 40);
        for (race, classes) in LEGAL {
            for &class in classes {
                let kit = START_GEAR
                    .iter()
                    .find(|(r, c, _)| (*r, *c) == (race, class))
                    .unwrap_or_else(|| panic!("no kit for {race}/{class}"));
                let worn = starting_gear(race, class, 7);
                assert_eq!(worn.len(), kit.2.len(), "{race}/{class}: every item worn");
                let mut slots: Vec<u8> = worn.iter().map(|w| w.slot).collect();
                slots.dedup();
                assert_eq!(slots.len(), worn.len(), "{race}/{class}: one item per slot");
                let main = worn
                    .iter()
                    .find(|w| w.slot == MAINHAND)
                    .expect("a main hand");
                assert_eq!(main.item.class, 2, "{race}/{class}: a weapon");
                assert_ne!(
                    0x2a5f3 & 1 << main.item.subclass,
                    0,
                    "{race}/{class}: melee"
                );
                assert!(worn.iter().any(|w| w.slot == 6), "{race}/{class}: legs");
            }
        }
        assert!(starting_gear(3, 8, 7).is_empty(), "no dwarf mage in 1.12");
    }

    #[test]
    fn the_human_warrior_wears_the_recruit_kit() {
        let worn = starting_gear(1, 1, 7);
        let by_slot: Vec<(u8, u32, &str)> = worn
            .iter()
            .map(|w| (w.slot, w.item.entry, w.item.name))
            .collect();
        assert_eq!(
            by_slot,
            [
                (3, 38, "Recruit's Shirt"),
                (6, 39, "Recruit's Pants"),
                (7, 40, "Recruit's Boots"),
                (15, 25, "Worn Shortsword"),
                (16, 2362, "Worn Wooden Shield"),
            ]
        );
        let thrown = starting_gear(8, 1, 7);
        let axes = thrown.iter().find(|w| w.slot == 17).expect("a ranged slot");
        assert_eq!((axes.item.entry, axes.count), (3111, 200), "a whole stack");
    }

    /// The client reads the template whole, with the weapon's damage and speed.
    #[test]
    fn an_item_query_reads_back_whole_in_the_client() {
        let ServerPacket::ItemQueryResponse { entry, info } = parse(&query_response(25)) else {
            panic!("not an item response");
        };
        assert_eq!(entry, 25);
        let info = info.expect("a known item");
        assert_eq!(info.name, "Worn Shortsword");
        assert_eq!((info.class, info.subclass), (2, 7));
        assert_eq!((info.display_info_id, info.inventory_type), (1542, 21));
        assert_eq!(
            (info.dmg_min, info.dmg_max, info.delay_ms),
            (1.0, 3.0, 1900)
        );
        assert_eq!((info.allowable_class, info.allowable_race), (-1, -1));
        assert_eq!(info.max_durability, 20);
        assert!(info.use_spell.is_none() && info.stats.is_empty());
        let ServerPacket::ItemQueryResponse { entry, info } = parse(&query_response(2362)) else {
            panic!("not an item response");
        };
        let info = info.expect("a known item");
        assert_eq!((entry, info.armor, info.block), (2362, 5, 1));
        let ServerPacket::ItemQueryResponse { entry, info } = parse(&query_response(6948)) else {
            panic!("not an item response");
        };
        assert_eq!((entry, info), (6948, None), "a bag item is not served");
    }

    /// The item create decodes as an item owned and contained by its wearer, at full durability.
    #[test]
    fn a_worn_item_is_created_as_its_owner_holds_it() {
        use benilla_protocol::{decode, SessionEvent};
        let worn = starting_gear(1, 1, 7);
        let sword = worn.iter().find(|w| w.slot == MAINHAND).unwrap();
        let p = Packet::new(opcode::SMSG_UPDATE_OBJECT, create_block(sword, 7));
        let events = decode(parse(&p));
        let [SessionEvent::ItemCreate {
            guid,
            container,
            fields,
        }] = events.as_slice()
        else {
            panic!("one item create, got {events:?}");
        };
        assert_eq!(*guid, sword.guid);
        assert!(benilla_protocol::guid::is_item(*guid));
        assert!(!container);
        assert_eq!(fields.object_entry(), Some(25));
        assert_eq!(fields.item_stack_count(), Some(1));
        assert_eq!(fields.item_durability(), Some(20));
        assert_eq!(fields.item_max_durability(), Some(20));
    }
}
