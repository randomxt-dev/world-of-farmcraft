//! Look-ahead previews: the farm as it might stand on day 10, 25 or 50, with the plots dug and
//! planted, the pen fenced, the animals home, the skills and quests where a steady farmer would
//! have them. `.preview` (the hotbar binds it to F8) steps through them; nothing is saved, so a
//! relog returns the farmer's own farm.

use farm_rules::Tile;

use super::*;

/// The days a preview shows.
const STAGES: [u32; 3] = [10, 25, 50];

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
                24,
                &[
                    Crop::Peacebloom,
                    Crop::Earthroot,
                    Crop::Mageroyal,
                    Crop::Pumpkin,
                ],
            ),
            _ => (
                48,
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

    /// The fences by `day`: the livestock pen half up with the rest marked out on day 10, whole
    /// from day 25, and by day 50 a fence along the far side of the plot too.
    fn preview_fences(&mut self, day: u32) {
        let pen = &self.layout.pen_fences;
        let built_up_to = if day <= 10 {
            pen.len() * 2 / 5
        } else {
            pen.len()
        };
        let mut fences: Vec<Fence> = (pen.iter().enumerate())
            .map(|(k, &(at, facing))| Fence {
                at,
                facing,
                style: FENCE_DISPLAY,
                built: k < built_up_to,
            })
            .collect();
        if day > 25 {
            let row = (self.farm.dug.iter().map(|c| c.0))
                .chain(0..PLOT_ROWS as i32)
                .max()
                .unwrap_or(0)
                + 1;
            let line: Vec<[f32; 3]> = (self.layout.field.range((row, i32::MIN)..=(row, i32::MAX)))
                .map(|(_, &p)| p)
                .step_by(3)
                .collect();
            for pair in line.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let facing = (b[1] - a[1]).atan2(b[0] - a[0]) + std::f32::consts::PI;
                fences.push(Fence {
                    at: a,
                    facing,
                    style: FENCE_DISPLAY,
                    built: true,
                });
            }
        }
        self.farm.fences = fences;
    }
}
