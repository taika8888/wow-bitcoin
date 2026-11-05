use crate::feed::FeedStatus;
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Neutral,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub text: String,
    pub tone: Tone,
}

impl Segment {
    pub fn new(text: impl Into<String>, tone: Tone) -> Segment {
        Segment {
            text: text.into(),
            tone,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TickerDisplay {
    pub segments: Vec<Segment>,
    pub dimmed: bool,
}

impl TickerDisplay {
    pub fn new(status: &FeedStatus, settings: &Settings) -> TickerDisplay {
        let label = if settings.show_label { "BTC " } else { "" };
        let (quote, stale) = match status {
            FeedStatus::Loading => return TickerDisplay::dimmed(format!("{label}...")),
            FeedStatus::Offline => return TickerDisplay::dimmed(format!("{label}offline")),
            FeedStatus::Live(quote) => (quote, false),
            FeedStatus::Stale(quote) => (quote, true),
        };
        let mut segments = vec![Segment::new(
            format!("{label}{}", format_price(quote.price, settings.decimals)),
            Tone::Neutral,
        )];
        if settings.show_change {
            let (text, tone) = format_change(quote.change_24h());
            segments.push(Segment::new("  ", Tone::Neutral));
            segments.push(Segment::new(text, tone));
        }
        if stale {
            segments.push(Segment::new("  stale", Tone::Neutral));
        }
        TickerDisplay {
            segments,
            dimmed: stale,
        }
    }

    fn dimmed(text: String) -> TickerDisplay {
        TickerDisplay {
            segments: vec![Segment::new(text, Tone::Neutral)],
            dimmed: true,
        }
    }

    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect()
    }
}

/// "$80,482": en-US grouping, rounded half away from zero at `decimals`.
pub fn format_price(value: f64, decimals: u8) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    format!("{sign}${}", fixed(value.abs(), decimals.into(), true))
}

/// Rounds to the shown precision first, so a change that displays as 0.00% is flat, never "+0.00%" in green.
pub fn format_change(fraction: f64) -> (String, Tone) {
    let percent = (fraction * 10_000.0).round() / 100.0;
    let magnitude = format!("{}%", fixed(percent.abs(), 2, false));
    if percent > 0.0 {
        (format!("+{magnitude}"), Tone::Up)
    } else if percent < 0.0 {
        (format!("-{magnitude}"), Tone::Down)
    } else {
        (magnitude, Tone::Neutral)
    }
}

/// Rounds the shortest decimal form of `value`, not its binary expansion: 80482.45 is stored as 80482.4499...,
/// which `format!("{:.1}")` prints as 80482.4.
fn fixed(value: f64, decimals: usize, grouping: bool) -> String {
    let shortest = format!("{value}");
    let (whole, fraction) = shortest.split_once('.').unwrap_or((&shortest, ""));
    let mut digits: Vec<u8> = whole
        .bytes()
        .chain(
            fraction
                .bytes()
                .chain(std::iter::repeat(b'0'))
                .take(decimals),
        )
        .map(|b| b - b'0')
        .collect();
    if fraction
        .as_bytes()
        .get(decimals)
        .is_some_and(|&d| d >= b'5')
    {
        let mut carry = true;
        for digit in digits.iter_mut().rev() {
            *digit += 1;
            carry = *digit == 10;
            if !carry {
                break;
            }
            *digit = 0;
        }
        if carry {
            digits.insert(0, 1);
        }
    }
    let split = digits.len() - decimals;
    let mut text = String::new();
    for (index, digit) in digits[..split].iter().enumerate() {
        if grouping && index > 0 && (split - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(char::from(b'0' + digit));
    }
    if decimals > 0 {
        text.push('.');
        text.extend(digits[split..].iter().map(|d| char::from(b'0' + d)));
    }
    text
}
