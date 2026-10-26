//! The farm's clock: a day runs from 06:00 to 02:00 the next morning in about 17 real minutes,
//! four 28-day seasons make a year, and each morning rolls the day's weather.

use serde::{Deserialize, Serialize};

use crate::rng;

/// Minutes past midnight a day begins at: 06:00.
pub const DAY_START: u32 = 6 * 60;
/// Minutes past the day's midnight it ends at: 02:00 the next morning, when the farmer passes out.
pub const DAY_END: u32 = 26 * 60;
/// Game minutes per real second: 20 game hours in 1000 s, a little under 17 minutes.
pub const GAME_MINUTES_PER_SECOND: f32 = 1.2;
/// Days in a season.
pub const SEASON_DAYS: u32 = 28;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Fall,
    Winter,
}

impl Season {
    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "Spring",
            Season::Summer => "Summer",
            Season::Fall => "Fall",
            Season::Winter => "Winter",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    Sunny,
    Rain,
    Snow,
}

impl Weather {
    /// The day's weather: a quarter of days rain (snow in winter), the first day of a season is
    /// always clear.
    pub fn roll(seed: u64, day: u32, season: Season) -> Self {
        if (day - 1).is_multiple_of(SEASON_DAYS) || rng::below(seed, u64::from(day), 4) != 0 {
            Weather::Sunny
        } else if season == Season::Winter {
            Weather::Snow
        } else {
            Weather::Rain
        }
    }

    /// Whether the sky waters the crops.
    pub fn waters(self) -> bool {
        self == Weather::Rain
    }

    pub fn name(self) -> &'static str {
        match self {
            Weather::Sunny => "sunny",
            Weather::Rain => "rainy",
            Weather::Snow => "snowy",
        }
    }
}

/// The day number (from 1) and the minute of that day.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub day: u32,
    /// Minutes past the day's midnight, from [`DAY_START`] up to [`DAY_END`].
    pub minute: f32,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            day: 1,
            minute: DAY_START as f32,
        }
    }
}

impl Clock {
    /// Run `seconds` of real time; `true` once the day has reached [`DAY_END`].
    pub fn advance(&mut self, seconds: f32) -> bool {
        self.minute = (self.minute + seconds * GAME_MINUTES_PER_SECOND).min(DAY_END as f32);
        self.minute >= DAY_END as f32
    }

    /// The next morning at [`DAY_START`].
    pub fn next_day(&mut self) {
        self.day += 1;
        self.minute = DAY_START as f32;
    }

    /// The clock face: hour 0-23 and minute 0-59.
    pub fn hour_minute(&self) -> (u32, u32) {
        let m = self.minute as u32 % (24 * 60);
        (m / 60, m % 60)
    }

    pub fn season(&self) -> Season {
        match ((self.day - 1) / SEASON_DAYS) % 4 {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Fall,
            _ => Season::Winter,
        }
    }

    /// The day of the season, 1 to [`SEASON_DAYS`].
    pub fn day_of_season(&self) -> u32 {
        (self.day - 1) % SEASON_DAYS + 1
    }

    /// The year, from 1.
    pub fn year(&self) -> u32 {
        (self.day - 1) / (SEASON_DAYS * 4) + 1
    }

    /// "06:30".
    pub fn face(&self) -> String {
        let (h, m) = self.hour_minute();
        format!("{h:02}:{m:02}")
    }

    /// "Spring 3, Year 1".
    pub fn date(&self) -> String {
        format!(
            "{} {}, Year {}",
            self.season().name(),
            self.day_of_season(),
            self.year()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_lasts_about_seventeen_real_minutes_and_ends_at_two() {
        let mut c = Clock::default();
        assert_eq!(c.face(), "06:00");
        assert!(!c.advance(500.0));
        assert_eq!(c.face(), "16:00");
        assert!(c.advance(500.0));
        assert_eq!(c.face(), "02:00");
        c.next_day();
        assert_eq!((c.day, c.face()), (2, "06:00".into()));
    }

    #[test]
    fn seasons_turn_every_28_days() {
        let mut c = Clock::default();
        assert_eq!(c.date(), "Spring 1, Year 1");
        for _ in 0..28 {
            c.next_day();
        }
        assert_eq!(c.date(), "Summer 1, Year 1");
        for _ in 0..84 {
            c.next_day();
        }
        assert_eq!(c.date(), "Spring 1, Year 2");
    }

    #[test]
    fn weather_is_clear_on_a_seasons_first_day_and_snows_only_in_winter() {
        for seed in 0..50 {
            assert_eq!(Weather::roll(seed, 1, Season::Spring), Weather::Sunny);
            for day in 2..28 {
                assert_ne!(Weather::roll(seed, day, Season::Summer), Weather::Snow);
                assert_ne!(Weather::roll(seed, day, Season::Winter), Weather::Rain);
            }
        }
        let rainy = (2..1000)
            .filter(|&d| Weather::roll(7, d, Season::Spring) == Weather::Rain)
            .count();
        assert!((150..350).contains(&rainy), "about a quarter: {rainy}");
    }
}
