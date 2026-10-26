//! The farm's items: the farmer's clothes and tools, seeds, crops and ore. Their templates, the
//! item objects the owner is sent and the `SMSG_ITEM_QUERY_SINGLE_RESPONSE` the client asks for.
//! Crops and ore keep their 1.12 entries so the client's own caches and icons line up; seeds are
//! ours. Display ids are `ItemDisplayInfo.dbc` rows of the 1.12.1 client.

use benilla_protocol::messages::opcode;
use farm_rules::{Crop, Item};

use super::socket::Packet;
use super::update::{self, Fields};
use super::wire::Body;

/// The template columns the client reads; everything else goes out as zero or `-1`.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemTemplate {
    pub entry: u32,
    pub name: String,
    pub class: u8,
    pub subclass: u8,
    pub display_id: u32,
    pub quality: u8,
    pub buy_price: u32,
    pub sell_price: u32,
    pub inventory_type: u8,
    pub stackable: u16,
    pub description: String,
    pub material: i8,
    pub sheath: u8,
    pub delay_ms: u32,
    pub damage: [f32; 2],
    pub armor: u32,
}

impl ItemTemplate {
    fn goods(entry: u32, name: &str, display_id: u32, sell: u32, description: String) -> Self {
        Self {
            entry,
            name: name.into(),
            class: 7, // trade goods
            subclass: 0,
            display_id,
            quality: 1,
            buy_price: 0,
            sell_price: sell,
            inventory_type: 0,
            stackable: farm_rules::bag::STACK as u16,
            description,
            material: -1,
            sheath: 0,
            delay_ms: 0,
            damage: [0.0, 0.0],
            armor: 0,
        }
    }

    fn worn(entry: u32, name: &str, display_id: u32, inventory_type: u8, armor: u32) -> Self {
        Self {
            entry,
            name: name.into(),
            class: 4, // armor
            subclass: 1,
            display_id,
            quality: 1,
            buy_price: 0,
            sell_price: 0,
            inventory_type,
            stackable: 1,
            description: String::new(),
            material: 7, // cloth
            sheath: 0,
            delay_ms: 0,
            damage: [0.0, 0.0],
            armor,
        }
    }
}

/// `item_template` 2901, the Mining Pick: Mining's Totem, carried in the backpack.
pub const MINING_PICK: u32 = 2901;
/// Our seed packets, one entry per crop from here.
const SEEDS_BASE: u32 = 91_000;
/// `item_template` 4470 Simple Wood and its look, what a felled tree gives.
const WOOD: (u32, u32) = (4470, 21102);
/// `item_template` 768, the Lumberjack Axe the farmer chops with (`INV_Axe_01`).
pub const LUMBERJACK_AXE: u32 = 768;
/// `item_template` 3334, the Farmer's Shovel the farmer works the soil with.
pub const SHOVEL: u32 = 3334;

/// The 1.12 entry and look of a crop's harvest.
fn produce(crop: Crop) -> (u32, u32) {
    match crop {
        Crop::Silverleaf => (765, 18088),
        Crop::Peacebloom => (2447, 7396),
        Crop::Earthroot => (2449, 1464),
        Crop::Mageroyal => (785, 7341),
        Crop::Briarthorn => (2450, 7406),
        Crop::Pumpkin => (4656, 6402),
        Crop::Watermelon => (5057, 6390),
    }
}

/// The item entry of a farm good.
pub fn entry(item: Item) -> u32 {
    match item {
        Item::Seeds(c) => SEEDS_BASE + Crop::ALL.iter().position(|&x| x == c).unwrap_or(0) as u32,
        Item::Produce(c) => produce(c).0,
        Item::CopperOre => 2770,
        Item::Wood => WOOD.0,
    }
}

/// The farm good an item entry is.
pub fn item_of(entry: u32) -> Option<Item> {
    let mut all = Crop::ALL
        .iter()
        .flat_map(|&c| [Item::Seeds(c), Item::Produce(c)])
        .chain([Item::CopperOre, Item::Wood]);
    all.find(|&i| self::entry(i) == entry)
}

/// The template of `entry`: a farm good, a piece of the farmer's outfit or the Mining Pick.
pub fn template(entry: u32) -> Option<ItemTemplate> {
    if let Some(item) = item_of(entry) {
        return Some(match item {
            Item::Seeds(c) => {
                let d = c.def();
                let mut t = ItemTemplate::goods(
                    entry,
                    &item.name(),
                    // The crop's own icon; the hotbar draws a seed sack over it.
                    produce(c).1,
                    item.sell_price(),
                    format!(
                        "Plant in tilled soil and water daily. Ripe in {} days.",
                        d.days
                    ),
                );
                t.buy_price = d.seed_price;
                t
            }
            Item::Produce(c) => ItemTemplate::goods(
                entry,
                c.name(),
                produce(c).1,
                item.sell_price(),
                "Grown on Stonefield Farm.".into(),
            ),
            Item::CopperOre => {
                ItemTemplate::goods(entry, "Copper Ore", 4681, item.sell_price(), String::new())
            }
            Item::Wood => ItemTemplate::goods(
                entry,
                "Simple Wood",
                WOOD.1,
                item.sell_price(),
                "Fresh from the grove.".into(),
            ),
        });
    }
    Some(match entry {
        MINING_PICK => ItemTemplate {
            entry,
            name: "Mining Pick".into(),
            class: 2,
            subclass: 14,
            display_id: 6568,
            quality: 1,
            buy_price: 0,
            sell_price: 0,
            inventory_type: 21,
            stackable: 1,
            description: String::new(),
            material: 1,
            sheath: 3,
            delay_ms: 2000,
            damage: [1.0, 3.0],
            armor: 0,
        },
        LUMBERJACK_AXE => ItemTemplate {
            entry,
            name: "Lumberjack Axe".into(),
            class: 2,
            subclass: 0,
            display_id: 5012,
            quality: 1,
            buy_price: 0,
            sell_price: 0,
            inventory_type: 21,
            stackable: 1,
            description: "Right-click a grove tree to chop it down.".into(),
            material: 1,
            sheath: 3,
            delay_ms: 2200,
            damage: [3.0, 6.0],
            armor: 0,
        },
        // Farmer's Shovel, held two-handed.
        SHOVEL => ItemTemplate {
            entry,
            name: "Farmer's Shovel".into(),
            class: 2,
            subclass: 5,
            display_id: 7495,
            quality: 1,
            buy_price: 0,
            sell_price: 0,
            inventory_type: 17,
            stackable: 1,
            description: "It has turned a lot of Elwynn soil.".into(),
            material: 2,
            sheath: 1,
            delay_ms: 3000,
            damage: [2.0, 4.0],
            armor: 0,
        },
        4344 => ItemTemplate::worn(entry, "Brown Linen Shirt", 12802, 4, 0),
        4343 => ItemTemplate::worn(entry, "Brown Linen Pants", 12388, 7, 3),
        11191 => ItemTemplate::worn(entry, "Farmer's Boots", 28167, 8, 2),
        5606 => ItemTemplate::worn(entry, "Gardening Gloves", 16817, 10, 1),
        _ => return None,
    })
}

/// The outfit every farmer wears: (equipment slot, entry).
pub const OUTFIT: [(u8, u32); 5] = [(3, 4344), (6, 4343), (7, 11191), (9, 5606), (15, SHOVEL)];

/// The 19 equipment slots, head (0) to tabard (18).
pub const EQUIPMENT_SLOTS: u8 = 19;
/// `INVENTORY_SLOT_ITEM_START`: the backpack's first slot.
pub const BACKPACK: u8 = 23;
/// The backpack's 16 slots end before this one.
pub const BACKPACK_END: u8 = 39;

/// One item the player holds, worn or in the backpack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub slot: u8,
    pub guid: u64,
    pub entry: u32,
    pub count: u32,
}

/// `HIGHGUID_ITEM`.
const HIGH_ITEM: u64 = 0x4000;

/// An item guid: the owner's low word and `n`, unique for the session.
pub fn item_guid(owner: u64, n: u32) -> u64 {
    HIGH_ITEM << 48 | (owner & 0xFFFF) << 32 | u64::from(n)
}

// Item descriptor indices for build 5875 (vmangos `UpdateFields_1_12_1.h`).
const OBJECT_FIELD_GUID: u16 = 0;
const OBJECT_FIELD_TYPE: u16 = 2;
const OBJECT_FIELD_ENTRY: u16 = 3;
const OBJECT_FIELD_SCALE_X: u16 = 4;
const ITEM_FIELD_OWNER: u16 = 6;
const ITEM_FIELD_CONTAINED: u16 = 8;
const ITEM_FIELD_STACK_COUNT: u16 = 14;
/// `TYPEMASK_OBJECT | TYPEMASK_ITEM`.
const TYPE_ITEM: u32 = 0x01 | 0x02;

/// The create block of a held item, sent to its owner only; it is contained by its owner.
pub fn create_block(h: &Held, owner: u64) -> Vec<u8> {
    let mut f = Fields::default();
    f.u64(OBJECT_FIELD_GUID, h.guid)
        .u32(OBJECT_FIELD_TYPE, TYPE_ITEM)
        .u32(OBJECT_FIELD_ENTRY, h.entry)
        .f32(OBJECT_FIELD_SCALE_X, 1.0)
        .u64(ITEM_FIELD_OWNER, owner)
        .u64(ITEM_FIELD_CONTAINED, owner)
        .u32(ITEM_FIELD_STACK_COUNT, h.count);
    update::create_item(h.guid, &f)
}

/// The values update of a held stack's new count.
pub fn count_change(guid: u64, count: u32) -> Packet {
    let mut f = Fields::default();
    f.u32(ITEM_FIELD_STACK_COUNT, count);
    Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(guid, &f))
}

/// `SMSG_ITEM_PUSH_RESULT`: "You receive loot: [x]xN" in chat, or "You receive item:" for one
/// `bought`; `slot` is the backpack slot it went to (vmangos `Player::SendNewItem`).
pub fn push_result(owner: u64, entry: u32, slot: u8, count: u32, bought: bool) -> Packet {
    Packet::new(
        opcode::SMSG_ITEM_PUSH_RESULT,
        Body::default()
            .u64(owner)
            .u32(u32::from(bought)) // received from an NPC
            .u32(0) // received, not created
            .u32(1) // show in chat
            .u8(255) // the backpack
            .u32(u32::from(slot))
            .u32(entry)
            .u32(0) // suffix factor
            .u32(0) // random property
            .u32(count)
            .done(),
    )
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
        .cstring(&t.name)
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
        .u32(1) // item level
        .u32(0); // required level
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
    b.u32(t.delay_ms).u32(0).f32(0.0); // ammo type, range mod
    for _ in 0..5 {
        // An empty spell block: id, trigger, charges, cooldown -1, category, category cooldown -1.
        b.u32(0).u32(0).u32(0).u32(u32::MAX).u32(0).u32(u32::MAX);
    }
    b.u32(0) // bonding
        .cstring(&t.description)
        .u32(0) // page text
        .u32(0) // language
        .u32(0) // page material
        .u32(0) // start quest
        .u32(0) // lock
        .u32(t.material as u32)
        .u32(t.sheath.into())
        .u32(0) // random property
        .u32(0) // block
        .u32(0) // item set
        .u32(0) // max durability
        .u32(0) // area
        .u32(0) // map
        .u32(0); // bag family
    Packet::new(opcode::SMSG_ITEM_QUERY_SINGLE_RESPONSE, b.done())
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};

    fn parse(p: &Packet) -> ServerPacket {
        let (packet, tail) =
            parse_server_with_tail(p.opcode, &p.body).expect("the client parses it");
        assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
        packet
    }

    #[test]
    fn every_farm_good_has_its_own_entry_and_reads_back_in_the_client() {
        let mut seen = std::collections::HashSet::new();
        for crop in Crop::ALL {
            for item in [Item::Seeds(crop), Item::Produce(crop)] {
                let e = entry(item);
                assert!(seen.insert(e), "{item:?} shares entry {e}");
                assert_eq!(item_of(e), Some(item));
                assert!(matches!(
                    parse(&query_response(e)),
                    ServerPacket::ItemQueryResponse { .. }
                ));
            }
        }
        for (_, e) in OUTFIT {
            assert!(template(e).is_some(), "outfit {e}");
            parse(&query_response(e));
        }
        parse(&query_response(MINING_PICK));
        parse(&query_response(1));
        assert!(matches!(
            parse(&push_result(1, 2447, 24, 2, false)),
            ServerPacket::ItemPushResult(_)
        ));
    }
}
