//! Look-ahead previews: the farm as it might stand on day 10, 25 or 50, with the plots dug and
//! planted (the whole field by day 50), the pens fenced and full of animals (ten times the
//! starting herd by day 25, twenty-five times by day 50), the skills and quests where a steady
//! farmer would have them. `.preview` (the hotbar binds it to F8) steps through them; nothing is saved, so a
//! relog returns the farmer's own farm.

use farm_rules::Tile;

use super::*;

/// The days a preview shows.
const STAGES: [u32; 3] = [10, 25, 50];
/// The preview herd's guids start here, clear of every villager's.
const HERD_GUIDS: u64 = 0x80_0000;

impl Game {
    /// `.preview [day]`: the next stage after today's, or `day`'s.
    pub(super) fn preview(&mut self, day: Option<u32>) {
        let today = self.farm.clock.day;
        let day = day.unwrap_or_else(|| {
            (STAGES.iter().copied())
                .find(|&d| d > today)
                .unwrap_or(STAGES[0])
        });
        self.ride(false);
        self.interrupt();
        self.stop_chop();
        let had_pick = self.farm.has(Unlock::MiningPick);
        self.clear_world();
        self.farm.preview(day);
        self.preview_plots(day);
        self.preview_fences(day);
        self.preview_clearing(day);
        self.previewing = true;

        let tiles = self.farm.tiles.len();
        self.soil = vec![(0, 0); tiles];
        self.crops = vec![None; tiles];
        self.veins = vec![None; self.layout.veins.len()];
        self.wild = vec![None; self.layout.wild.len()];
        self.trees = vec![(0, 0); self.layout.trees.len()];
        self.fences.clear();
        self.critter_at = (self.layout.critters.iter()).map(|(_, s)| s[0]).collect();
        self.markers = [u32::MAX; 4];
        self.show_world();
        self.preview_herd(day);
        if !had_pick && self.farm.has(Unlock::MiningPick) {
            let pick = player::pick(self.guid());
            self.send(Packet::new(
                benilla_protocol::messages::opcode::SMSG_UPDATE_OBJECT,
                items::create_block(&pick, self.guid()),
            ));
            self.send(player::slot_change(self.guid(), pick.slot, pick.guid));
        }
        self.send(self.clock_packet());
        self.send(self.weather_packet(true));
        self.sync_bag(None, false);
        self.sync_money();
        self.send(player::skill_change(self.guid(), &self.skill_lines()));
        self.sync_quest_log();
        self.refresh_markers();
        self.send(packets::headline(&format!("Preview: day {day}")));
        let next = (STAGES.iter())
            .find(|&&d| d > day)
            .map_or("back to day 10".to_owned(), |d| format!("day {d}"));
        self.say(&format!(
            "This is how the farm could look on day {day}. Nothing is saved: log out to return \
             to your own farm. Press F8 for {next}."
        ));
    }

    /// Takes every object of the farm out of the client's world.
    fn clear_world(&mut self) {
        let mut gone: Vec<u64> = Vec::new();
        gone.extend(self.soil.iter().map(|&(g, _)| g));
        gone.extend(self.crops.iter().flatten().map(|&(g, _)| g));
        gone.extend(self.veins.iter().flatten());
        gone.extend(self.wild.iter().flatten());
        gone.extend(self.trees.iter().map(|&(g, _)| g));
        gone.extend(self.fences.iter());
        gone.extend(self.fixtures.iter());
        if let Some((_, marker)) = self.fence_start.take() {
            gone.push(marker);
        }
        gone.extend((0..self.layout.npcs.len()).map(npc_guid));
        gone.push(self.dog_guid());
        gone.append(&mut self.herd);
        for guid in gone.into_iter().filter(|&g| g != 0) {
            self.send(crate::world::destroy(guid));
        }
        self.fixtures = [0; 2];
    }

    /// The plot by `day`: the starting patch and the cells dug next to it, nearest first, planted
    /// with the crops of the day at every stage of growth.
    fn preview_plots(&mut self, day: u32) {
        let (extra, crops): (usize, &[Crop]) = match day {
            0..=10 => (8, &[Crop::Silverleaf, Crop::Peacebloom, Crop::Earthroot]),
            11..=25 => (
                96,
                &[
                    Crop::Peacebloom,
                    Crop::Earthroot,
                    Crop::Mageroyal,
                    Crop::Pumpkin,
                ],
            ),
            _ => (
                usize::MAX,
                &[
                    Crop::Mageroyal,
                    Crop::Briarthorn,
                    Crop::Pumpkin,
                    Crop::Watermelon,
                    Crop::Earthroot,
                ],
            ),
        };
        let start = |cell: (i32, i32)| {
            (0..PLOT_ROWS as i32).contains(&cell.0) && (0..PLOT_COLUMNS as i32).contains(&cell.1)
        };
        let (mid_r, mid_c) = (
            (PLOT_ROWS as f32 - 1.0) / 2.0,
            (PLOT_COLUMNS as f32 - 1.0) / 2.0,
        );
        let mut cells: Vec<(i32, i32)> = (self.layout.field.keys().copied())
            .filter(|&c| !start(c))
            .collect();
        let yards = |c: &(i32, i32)| {
            let (dr, dc) = ((c.0 as f32 - mid_r) * 4.17, (c.1 as f32 - mid_c) * 1.4);
            dr.hypot(dc)
        };
        cells.sort_by(|a, b| yards(a).total_cmp(&yards(b)));
        cells.truncate(extra);
        self.farm.dug = cells;
        let tiles = self.layout.tiles.len() + self.farm.dug.len();
        self.farm.tiles = (0..tiles)
            .map(|i| {
                let crop = crops[(i / 3) % crops.len()];
                let days = crop.def().days;
                let age = (i as u32 * 7 + day) % (days + 1);
                Tile {
                    crop: Some((crop, age)),
                    watered: age < days && i % 3 != 0,
                }
            })
            .collect();
    }

    /// The fences by `day`: on day 10 the livestock pen half up and the rest marked out with
    /// posts; from day 25 both pens whole.
    fn preview_fences(&mut self, day: u32) {
        let pens: &[crate::layout::Pen] = if day <= 10 {
            &self.layout.pens[..1]
        } else {
            &self.layout.pens
        };
        let mut fences = Vec::new();
        for pen in pens {
            let built_up_to = if day <= 10 {
                pen.fences.len() * 2 / 5
            } else {
                usize::MAX
            };
            fences.extend(
                pen.fences
                    .iter()
                    .enumerate()
                    .map(|(k, &(at, facing))| Fence {
                        at,
                        facing,
                        style: FENCE_DISPLAY,
                        built: k < built_up_to,
                    }),
            );
        }
        self.farm.fences = fences;
    }

    /// The trees that stood where the pens went up are gone.
    fn preview_clearing(&mut self, day: u32) {
        let inside = |p: [f32; 3], a: [f32; 4]| {
            (a[0] - 2.0..=a[2] + 2.0).contains(&p[0]) && (a[1] - 2.0..=a[3] + 2.0).contains(&p[1])
        };
        for (t, &(_, at)) in self.layout.trees.iter().enumerate() {
            let cleared = self.layout.pens.iter().any(|pen| inside(at, pen.area));
            if cleared {
                if let Some(felled) = self.farm.felled.get_mut(t) {
                    *felled = Some(day);
                }
            }
        }
    }

    /// The herd by `day`, in the pens: the farm's own animals stand aside, and the pens fill with
    /// cows, sheep and pigs, and the run with chickens, ten times the starting herd by day 25 and
    /// twenty-five times by day 50.
    fn preview_herd(&mut self, day: u32) {
        let times = match day {
            0..=10 => return,
            11..=25 => 10,
            _ => 25,
        };
        let own: Vec<usize> = (0..self.layout.npcs.len())
            .filter(|&i| {
                let n = &self.layout.npcs[i];
                matches!(n.role, Role::Animal(..)) || (n.role == Role::None && n.look.name == "Pig")
            })
            .collect();
        for &i in &own {
            self.send(crate::world::destroy(npc_guid(i)));
        }
        let kind = |name: &str| {
            own.iter()
                .copied()
                .find(|&i| self.layout.npcs[i].look.name == name)
        };
        let count = |name: &str| {
            own.iter()
                .filter(|&&i| self.layout.npcs[i].look.name == name)
                .count()
        };
        let livestock: Vec<usize> = ["Cow", "Sheep", "Pig"]
            .into_iter()
            .flat_map(|name| std::iter::repeat_n(kind(name), count(name) * times))
            .flatten()
            .collect();
        let chickens: Vec<usize> = std::iter::repeat_n(kind("Chicken"), count("Chicken") * times)
            .flatten()
            .collect();
        let mut low = HERD_GUIDS;
        for (pen, herd) in self.layout.pens.iter().zip([livestock, chickens]) {
            // Spread the herd over the pen's spots, every other one first, so a small herd
            // still fills the pen.
            let n = pen.spots.len().max(1);
            for (k, &i) in herd.iter().enumerate() {
                let spot = pen.spots[(k * 7) % n];
                let jitter =
                    |salt: u64| (farm_rules::rng::below(low, salt, 100) as f32 / 100.0 - 0.5) * 1.2;
                let at = [spot[0] + jitter(1), spot[1] + jitter(2), spot[2]];
                let facing = farm_rules::rng::below(low, 3, 628) as f32 / 100.0;
                let entry = NPC_BASE + i as u32;
                low += 1;
                let guid = HIGH_UNIT << 48 | u64::from(entry) << 24 | low;
                let shown = Shown {
                    guid,
                    entry,
                    position: at,
                    orientation: facing,
                    owner: None,
                };
                self.send(creatures::create(&shown, &self.layout.npcs[i].look));
                self.herd.push(guid);
            }
        }
    }
}
