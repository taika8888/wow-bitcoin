use std::ops::{Range, RangeInclusive};

use crate::chart::CandleChart;
use crate::display::TickerDisplay;
use crate::time::Timestamp;

/// One gridline on a chart axis and the text drawn beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick<T> {
    pub value: T,
    pub label: String,
}

const MINUTE: f64 = 60.0;
const DAY: f64 = 86_400.0;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

impl CandleChart {
    /// The fewest gridlines an axis gets. The next coarser step would leave fewer, so an axis gets 3 to 8, mostly 3 to 5.
    pub const MINIMUM_TICKS: f64 = 3.0;
    const TIME_STEP_MINUTES: [f64; 17] = [
        1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 180.0, 360.0, 720.0, 1440.0, 2880.0, 5760.0, 10_080.0, 20_160.0,
        43_200.0,
    ];

    /// Multiples of a 1, 2 or 5 step inside the price domain, labelled in whole dollars.
    pub fn price_ticks(&self) -> Vec<Tick<f64>> {
        let span = self.price_domain.end() - self.price_domain.start();
        let magnitude = 10f64.powf((span / Self::MINIMUM_TICKS).log10().floor());
        let Some(step) = step(&self.price_domain, &[1.0, 2.0, 5.0].map(|digit| digit * magnitude)) else {
            return Vec::new();
        };
        ticks(&self.price_domain, step, 0.0)
            .into_iter()
            .map(|price| Tick { value: price, label: TickerDisplay::price(price, 0) })
            .collect()
    }

    /// Round local times inside the time domain, `utc_offset` seconds east of UTC. Every tick is placed and labelled
    /// with that one offset: "14:30", or "2:30 PM" when `hour12`, for steps under a day, and "Nov 3" for longer steps.
    pub fn time_ticks(&self, utc_offset: i32, hour12: bool) -> Vec<Tick<Timestamp>> {
        let domain = self.time_domain.start().unix_seconds()..=self.time_domain.end().unix_seconds();
        let Some(step) = step(&domain, &Self::TIME_STEP_MINUTES.map(|minutes| minutes * MINUTE)) else {
            return Vec::new();
        };
        let offset = f64::from(utc_offset);
        ticks(&domain, step, offset)
            .into_iter()
            .map(|seconds| {
                let local = seconds + offset;
                let label = if step < DAY { clock(local, hour12) } else { date(local) };
                Tick { value: Timestamp::from_unix_seconds(seconds), label }
            })
            .collect()
    }
}

/// The coarsest step that still leaves `MINIMUM_TICKS` in the range, or the finest step when none does.
fn step(range: &RangeInclusive<f64>, steps: &[f64]) -> Option<f64> {
    let span = range.end() - range.start();
    if span <= 0.0 {
        return None;
    }
    steps.iter().rev().find(|step| span / **step >= CandleChart::MINIMUM_TICKS).or(steps.first()).copied()
}

/// Every multiple of the step inside the range, with multiples counted from `-offset` rather than zero. Each tick is
/// `first + index * step` rounded once, as Swift's floating-point `stride` computes it with `addingProduct`, so an end
/// tick the Mac draws is never lost to a second rounding.
fn ticks(range: &RangeInclusive<f64>, step: f64, offset: f64) -> Vec<f64> {
    let first = ((range.start() + offset) / step).ceil() * step - offset;
    (0..).map(|index| f64::from(index).mul_add(step, first)).take_while(|tick| tick <= range.end()).collect()
}

fn clock(local: f64, hour12: bool) -> String {
    let minutes = (local.rem_euclid(DAY) / MINUTE).floor() as u32;
    let (hour, minute) = (minutes / 60, minutes % 60);
    if hour12 {
        let suffix = if hour < 12 { "AM" } else { "PM" };
        format!("{}:{minute:02} {suffix}", (hour + 11) % 12 + 1)
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

/// "Nov 3": the proleptic Gregorian month and day of a local time, by Howard Hinnant's `civil_from_days`.
fn date(local: f64) -> String {
    let days = (local / DAY).floor() as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    format!("{} {day}", MONTHS[month as usize - 1])
}

/// Which of a row of axis labels to draw. `extents` are where each label would sit along the axis, in order. A label
/// is drawn only when it starts at least `gap` past the end of the last label drawn, so no two drawn labels touch.
pub fn spaced_labels(extents: &[Range<f64>], gap: f64) -> Vec<bool> {
    let mut end = f64::NEG_INFINITY;
    extents
        .iter()
        .map(|extent| {
            let drawn = extent.start >= end + gap;
            if drawn {
                end = extent.end;
            }
            drawn
        })
        .collect()
}
