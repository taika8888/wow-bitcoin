use serde::Serialize;

use crate::feed::FeedStatus;
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Neutral,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplaySegment {
    pub text: String,
    pub tone: Tone,
}

impl DisplaySegment {
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self { text: text.into(), tone }
    }

    fn neutral(text: impl Into<String>) -> Self {
        Self::new(text, Tone::Neutral)
    }
}

/// The ticker line as runs of text, each colored by its tone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickerDisplay {
    pub segments: Vec<DisplaySegment>,
    pub dimmed: bool,
}

impl TickerDisplay {
    pub fn new(status: &FeedStatus, settings: &Settings) -> Self {
        let label = if settings.show_label { "BTC " } else { "" };
        match status {
            FeedStatus::Loading => {
                Self { segments: vec![DisplaySegment::neutral(format!("{label}..."))], dimmed: true }
            }
            FeedStatus::Offline => {
                Self { segments: vec![DisplaySegment::neutral(format!("{label}offline"))], dimmed: true }
            }
            FeedStatus::Live(quote) | FeedStatus::Stale(quote) => {
                let stale = matches!(status, FeedStatus::Stale(_));
                let mut segments =
                    vec![DisplaySegment::neutral(format!("{label}{}", Self::price(quote.price, settings.decimals)))];
                if settings.show_change {
                    let (text, tone) = Self::change(quote.change_24h());
                    segments.push(DisplaySegment::neutral("  "));
                    segments.push(DisplaySegment::new(text, tone));
                }
                if stale {
                    segments.push(DisplaySegment::neutral("  stale"));
                }
                Self { segments, dimmed: stale }
            }
        }
    }

    pub fn text(&self) -> String {
        self.segments.iter().map(|segment| segment.text.as_str()).collect()
    }

    /// US dollars with en_US grouping, rounded half away from zero to `decimals` places.
    pub fn price(value: f64, decimals: u8) -> String {
        format!("${}", decimal(value, decimals, true))
    }

    /// Rounds to the shown precision first, so a change that displays as 0.00% is flat, never "+0.00%" in green.
    pub fn change(fraction: f64) -> (String, Tone) {
        let percent = (fraction * 10_000.0).round() / 100.0;
        let magnitude = format!("{}%", decimal(percent.abs(), 2, false));
        if percent > 0.0 {
            (format!("+{magnitude}"), Tone::Up)
        } else if percent < 0.0 {
            (format!("-{magnitude}"), Tone::Down)
        } else {
            (magnitude, Tone::Neutral)
        }
    }
}

/// Rounds the value's shortest decimal form, the digits a person reads, rather than its binary approximation,
/// so 1.005 rounds to 1.01 the way a number formatter would.
fn decimal(value: f64, decimals: u8, grouped: bool) -> String {
    let places = usize::from(decimals);
    let shortest = format!("{}", value.abs());
    let (whole, fraction) = shortest.split_once('.').unwrap_or((&shortest, ""));
    let mut digits: Vec<u8> =
        whole.bytes().chain(fraction.bytes().chain(std::iter::repeat(b'0')).take(places)).collect();
    if fraction.as_bytes().get(places).is_some_and(|digit| *digit >= b'5') {
        let mut index = digits.len();
        loop {
            if index == 0 {
                digits.insert(0, b'1');
                break;
            }
            index -= 1;
            if digits[index] == b'9' {
                digits[index] = b'0';
            } else {
                digits[index] += 1;
                break;
            }
        }
    }
    let split = digits.len() - places;
    let mut whole = String::from_utf8_lossy(&digits[..split]).into_owned();
    if grouped {
        whole = group_thousands(&whole);
    }
    let negative = value < 0.0 && digits.iter().any(|digit| *digit != b'0');
    let sign = if negative { "-" } else { "" };
    if places == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{}", String::from_utf8_lossy(&digits[split..]))
    }
}

fn group_thousands(whole: &str) -> String {
    let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}
