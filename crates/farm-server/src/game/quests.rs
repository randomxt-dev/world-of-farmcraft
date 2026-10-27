//! The farm's quest chain shown through WoW's own quest system: the `!` and `?` over Ma Stonefield,
//! Innkeeper Farley and Brog Hamfist, the accept and turn-in panels, the quest log, and what each
//! quest unlocks appearing in the world.

use farm_rules::{Deed, Goal, Marker, Unlock, Villager, QUESTS};

use super::*;
use crate::world::quests::{self, Objective, QuestItem, QuestText, OBJECT_OBJECTIVE};

/// The first quest's id; quest `q` of the chain is `QUEST_BASE + q`.
pub(super) const QUEST_BASE: u32 = 900;

/// What a tool costs at Brog's: the Woodcutter's Axe and the Mining Pick.
pub(super) const AXE_PRICE: u32 = 100;
pub(super) const PICK_PRICE: u32 = 200;

const VILLAGERS: [Villager; 3] = [Villager::Ma, Villager::Farley, Villager::Brog];
/// The addon prefix the map markers travel under.
const MARKERS_PREFIX: &str = "FarmcraftMarks";

/// The greeting texts of the villagers' gossip windows.
pub(super) const TEXT_FARLEY: u32 = 4;
pub(super) const TEXT_BROG: u32 = 5;

/// `dialog_status` values: nothing, a gray `?`, a yellow `!`, a yellow `?`.
fn status(m: Marker) -> u32 {
    match m {
        Marker::None => 0,
        Marker::Waiting => 3,
        Marker::Offer => 5,
        Marker::TurnIn => 7,
    }
}

fn item_of(item: Item, count: u32) -> QuestItem {
    let entry = items::entry(item);
    QuestItem {
        entry,
        count,
        display: items::template(entry).map_or(0, |t| t.display_id),
    }
}

/// Quest `q` as the client's quest windows and log read it.
pub(super) fn text(q: usize) -> QuestText {
    let quest = &QUESTS[q];
    let goals = match quest.goal {
        Goal::Do(_, need, text) => vec![Objective {
            counted: Some((OBJECT_OBJECTIVE | (q as u32 + 1), need)),
            item: None,
            text: text.into(),
        }],
        Goal::Bring(item, n) => vec![Objective {
            counted: None,
            item: Some((items::entry(item), n)),
            text: String::new(),
        }],
        Goal::Visit => Vec::new(),
    };
    QuestText {
        id: QUEST_BASE + q as u32,
        title: quest.title.into(),
        details: quest.details.into(),
        objectives: quest.objectives.into(),
        money: quest.money,
        next: if q + 1 < QUESTS.len() {
            QUEST_BASE + q as u32 + 1
        } else {
            0
        },
        goals,
    }
}

impl Game {
    fn villager(&self, npc: u64) -> Option<Villager> {
        match self.layout.npcs[self.npc_index(npc)?].role {
            Role::Quest(v) => Some(v),
            _ => None,
        }
    }

    pub(super) fn villager_guid(&self, v: Villager) -> u64 {
        let i = (self.layout.npcs.iter())
            .position(|n| n.role == Role::Quest(v))
            .unwrap_or(0);
        npc_guid(i)
    }

    fn quest_of(id: u32) -> Option<usize> {
        let q = id.checked_sub(QUEST_BASE)? as usize;
        (q < QUESTS.len()).then_some(q)
    }

    /// The quest log as the player's fields carry it.
    pub(super) fn sync_quest_log(&mut self) {
        let slots: Vec<(u32, u32)> = (self.farm.quests.active.iter())
            .map(|&(q, n)| {
                let done = u32::from(self.farm.quest_ready(q)) << 24;
                (QUEST_BASE + q as u32, n.min(63) | done)
            })
            .collect();
        if slots != self.quest_log {
            self.send(player::quest_log(self.guid(), &slots));
            self.quest_log = slots;
        }
    }

    /// Sends each villager's marker that changed, over their head and to the maps.
    pub(super) fn refresh_markers(&mut self) {
        let mut changed = false;
        for (k, v) in VILLAGERS.into_iter().enumerate() {
            let s = status(self.farm.marker(v));
            if self.markers[k] != s {
                self.markers[k] = s;
                changed = true;
                self.send(quests::status(self.villager_guid(v), s));
            }
        }
        if changed {
            self.send_markers();
        }
    }

    /// The villagers' markers for the minimap and world map, which `markers.lua` draws: an addon
    /// line of `x,y,status` per villager, in world yards.
    pub(super) fn send_markers(&self) {
        let rows: Vec<String> = (VILLAGERS.iter().enumerate())
            .map(|(k, &v)| {
                let at = self.layout.npcs[self.npc_index(self.villager_guid(v)).unwrap_or(0)].at;
                format!("{:.1},{:.1},{}", at[0], at[1], self.markers[k] % 8)
            })
            .collect();
        self.send(packets::addon(self.guid(), MARKERS_PREFIX, &rows.join(";")));
    }

    /// `CMSG_QUESTGIVER_STATUS_QUERY`.
    pub fn quest_status(&self, npc: u64) {
        if let Some(v) = self.villager(npc) {
            self.send(quests::status(npc, status(self.farm.marker(v))));
        }
    }

    /// `CMSG_QUEST_QUERY`.
    pub fn quest_query(&self, id: u32) {
        if let Some(q) = Self::quest_of(id) {
            self.send(quests::query_response(&text(q)));
        }
    }

    /// A villager's gossip window: their service, if any, and their quests. With nothing but one
    /// quest, the quest opens straight away.
    pub(super) fn villager_hello(&mut self, npc: u64, v: Villager) {
        let mut options = Vec::new();
        match v {
            Villager::Brog if self.farm.has(Unlock::Market) => {
                options.push("Let me browse your goods.".to_owned())
            }
            Villager::Farley if self.farm.has(Unlock::Market) => {
                options.push("I have produce to sell.".to_owned())
            }
            _ => {}
        }
        let mut rows = Vec::new();
        for &(q, _) in &self.farm.quests.active {
            if QUESTS[q].ender == v {
                let icon = if self.farm.quest_ready(q) { 4 } else { 3 };
                rows.push((QUEST_BASE + q as u32, icon, QUESTS[q].title.to_owned()));
            }
        }
        if let Some(q) = self.farm.quests.offer(v) {
            rows.push((QUEST_BASE + q as u32, 5, QUESTS[q].title.to_owned()));
        }
        if options.is_empty() && rows.len() == 1 {
            return self.quest_open(npc, rows[0].0);
        }
        self.menu = (!options.is_empty()).then_some(Menu::Shop(npc));
        let greeting = match v {
            Villager::Ma => TEXT_MA,
            Villager::Farley => TEXT_FARLEY,
            Villager::Brog => TEXT_BROG,
        };
        self.send(packets::gossip_quests(npc, greeting, &options, &rows));
    }

    /// `CMSG_QUESTGIVER_QUERY_QUEST`, or a quest row picked in the gossip window: the offer, the
    /// progress or the reward panel, as the quest stands.
    pub fn quest_open(&mut self, npc: u64, id: u32) {
        let (Some(v), Some(q)) = (self.villager(npc), Self::quest_of(id)) else {
            return;
        };
        if self.farm.quests.offer(v) == Some(q) {
            return self.send(quests::details(npc, &text(q)));
        }
        if self.farm.quests.is_active(q) && QUESTS[q].ender == v {
            self.quest_progress(npc, q);
        }
    }

    /// The progress panel of quest `q`, or straight to the reward when it hands nothing over.
    fn quest_progress(&mut self, npc: u64, q: usize) {
        let quest = &QUESTS[q];
        let ready = self.farm.quest_ready(q);
        let id = QUEST_BASE + q as u32;
        match quest.goal {
            Goal::Bring(item, n) => self.send(quests::request_items(
                npc,
                id,
                quest.title,
                quest.progress,
                &[item_of(item, n)],
                ready,
            )),
            _ if ready => self.send(quests::offer_reward(
                npc,
                id,
                quest.title,
                quest.reward,
                quest.money,
            )),
            _ => self.send(quests::request_items(
                npc,
                id,
                quest.title,
                quest.progress,
                &[],
                false,
            )),
        }
    }

    /// `CMSG_QUESTGIVER_ACCEPT_QUEST`.
    pub fn quest_accept(&mut self, npc: u64, id: u32) {
        let (Some(v), Some(q)) = (self.villager(npc), Self::quest_of(id)) else {
            return;
        };
        if QUESTS[q].giver != v || !self.farm.quests.accept(q) {
            return;
        }
        if self.farm.quest_ready(q) {
            self.send(quests::objectives_done(id));
        }
        self.sync_quest_log();
        self.refresh_markers();
        self.save();
    }

    /// `CMSG_QUESTGIVER_COMPLETE_QUEST` and `CMSG_QUESTGIVER_REQUEST_REWARD`: the reward panel
    /// once the quest is done.
    pub fn quest_complete(&mut self, npc: u64, id: u32) {
        let (Some(v), Some(q)) = (self.villager(npc), Self::quest_of(id)) else {
            return;
        };
        if !self.farm.quests.is_active(q) || QUESTS[q].ender != v {
            return;
        }
        let quest = &QUESTS[q];
        if self.farm.quest_ready(q) {
            self.send(quests::offer_reward(
                npc,
                id,
                quest.title,
                quest.reward,
                quest.money,
            ));
        } else {
            self.quest_progress(npc, q);
        }
    }

    /// `CMSG_QUESTGIVER_CHOOSE_REWARD`: the turn-in. The villager's next quest follows at once.
    pub fn quest_choose(&mut self, npc: u64, id: u32) {
        let (Some(v), Some(q)) = (self.villager(npc), Self::quest_of(id)) else {
            return;
        };
        if QUESTS[q].ender != v {
            return;
        }
        if let Err(r) = self.farm.turn_in(q) {
            return self.refuse(&r);
        }
        let quest = &QUESTS[q];
        self.send(quests::complete(id, quest.money));
        self.sync_money();
        self.sync_bag(None, false);
        self.sync_quest_log();
        if let Some(u) = quest.unlocks {
            self.unlocked(u);
        }
        self.refresh_markers();
        self.save();
        if let Some(next) = self.farm.quests.offer(v) {
            self.send(quests::details(npc, &text(next)));
        }
    }

    /// `CMSG_QUESTLOG_REMOVE_QUEST`: the quest in log slot `slot` is abandoned.
    pub fn quest_abandon(&mut self, slot: u8) {
        if let Some(&(q, _)) = self.farm.quests.active.get(usize::from(slot)) {
            self.farm.quests.abandon(q);
            self.sync_quest_log();
            self.refresh_markers();
        }
    }

    /// The farmer did `deed`: the quests counting it move on, with a toast.
    pub(super) fn noted(&mut self, deed: Deed) {
        for (q, n) in self.farm.note(deed) {
            let id = QUEST_BASE + q as u32;
            if let Goal::Do(_, need, _) = QUESTS[q].goal {
                self.send(quests::progress(
                    id,
                    OBJECT_OBJECTIVE | (q as u32 + 1),
                    n,
                    need,
                    0,
                ));
            }
            if self.farm.quest_ready(q) {
                self.send(quests::objectives_done(id));
            }
        }
        self.sync_quest_log();
        self.refresh_markers();
    }

    /// `.quest <n>`: skip the chain to quest `n`, with everything the ones before it unlock.
    pub(super) fn skip_to_quest(&mut self, n: usize) {
        let n = n.min(QUESTS.len());
        self.farm.quests.done = n;
        self.farm.quests.active.clear();
        let mut gained: Vec<Unlock> = QUESTS[..n].iter().filter_map(|q| q.unlocks).collect();
        if n > 6 {
            gained.push(Unlock::Axe);
        }
        if n > 9 {
            gained.push(Unlock::MiningPick);
        }
        for u in gained {
            if self.farm.unlocked.insert(u) {
                self.unlocked(u);
            }
        }
        self.sync_quest_log();
        self.refresh_markers();
        self.say(&format!("Skipped to quest {}.", n + 1));
        self.save();
    }

    /// Something new is the farmer's: put it on the bar or in the world.
    pub(super) fn unlocked(&mut self, u: Unlock) {
        match u {
            Unlock::WateringCan => self.say("New on your bar: the Watering Can (2)."),
            Unlock::Axe => self.say("New on your bar: the Woodcutter's Axe (3)."),
            Unlock::Fence => self.say("New on your bar: Build Fence (4)."),
            Unlock::Horse => {
                self.say("Biscuit the farm horse waits by the house: right-click him, or press 5.");
                if let Some(h) = self.horse() {
                    self.show_npc(h);
                }
            }
            Unlock::Tilling => self.say("New on your bar: Till Soil (1)."),
            Unlock::ShippingCrate => {
                self.show_fixture(1);
                self.say("The Shipping Crate stands by your farmhouse now.");
            }
            Unlock::Hens | Unlock::Herd => {
                for i in 0..self.layout.npcs.len() {
                    if self.animal_unlock(i) == Some(u) {
                        self.show_npc(i);
                    }
                }
            }
            Unlock::MiningPick => {
                let pick = player::pick(self.guid());
                self.send(Packet::new(
                    benilla_protocol::messages::opcode::SMSG_UPDATE_OBJECT,
                    items::create_block(&pick, self.guid()),
                ));
                self.send(player::slot_change(self.guid(), pick.slot, pick.guid));
                self.say("The Mining Pick is in your bag. Right-click a copper vein to mine it.");
            }
            Unlock::Market => {}
        }
        self.sync_bar();
    }
}
