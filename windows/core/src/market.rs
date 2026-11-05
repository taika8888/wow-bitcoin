/// Wall-clock seconds since 1970. The shell passes it into every `App::handle`, so nothing in core reads a clock
/// and tests drive time explicitly.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default)]
pub struct Time(pub f64);

impl Time {
    pub const DISTANT_PAST: Time = Time(f64::NEG_INFINITY);

    pub fn since(self, earlier: Time) -> f64 {
        self.0 - earlier.0
    }

    pub fn plus(self, seconds: f64) -> Time {
        Time(self.0 + seconds)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quote {
    pub price: f64,
    pub open_24h: f64,
    pub fetched_at: Time,
}

impl Quote {
    /// Fraction of the open, so 0.01 means +1%.
    pub fn change_24h(&self) -> f64 {
        (self.price - self.open_24h) / self.open_24h
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candle {
    pub time: Time,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Serialized as "m1" / "m15" / "h4", the Swift raw values, so a settings blob means the same on both platforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Timeframe {
    M1,
    M15,
    H4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeframeSpec {
    pub label: &'static str,
    pub kraken_interval_minutes: u32,
    pub candles_kept: usize,
    pub refresh_seconds: u32,
}

impl Timeframe {
    pub const ALL: [Timeframe; 3] = [Timeframe::M1, Timeframe::M15, Timeframe::H4];

    pub const fn spec(self) -> TimeframeSpec {
        match self {
            Timeframe::M1 => TimeframeSpec {
                label: "1m",
                kraken_interval_minutes: 1,
                candles_kept: 120,
                refresh_seconds: 15,
            },
            Timeframe::M15 => TimeframeSpec {
                label: "15m",
                kraken_interval_minutes: 15,
                candles_kept: 96,
                refresh_seconds: 60,
            },
            Timeframe::H4 => TimeframeSpec {
                label: "4h",
                kraken_interval_minutes: 240,
                candles_kept: 90,
                refresh_seconds: 300,
            },
        }
    }

    pub fn raw(self) -> &'static str {
        match self {
            Timeframe::M1 => "m1",
            Timeframe::M15 => "m15",
            Timeframe::H4 => "h4",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Timeframe> {
        Self::ALL.into_iter().find(|t| t.raw() == raw)
    }

    pub fn interval_seconds(self) -> f64 {
        f64::from(self.spec().kraken_interval_minutes * 60)
    }
}

impl serde::Serialize for Timeframe {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.raw())
    }
}
