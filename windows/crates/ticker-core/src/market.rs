use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quote {
    pub price: f64,
    pub open_24h: f64,
    pub fetched_at: Timestamp,
}

impl Quote {
    /// Fraction of the open, so 0.01 means +1%.
    pub fn change_24h(&self) -> f64 {
        (self.price - self.open_24h) / self.open_24h
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candle {
    pub time: Timestamp,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Timeframe {
    #[serde(rename = "m1")]
    M1,
    #[serde(rename = "m15")]
    M15,
    #[serde(rename = "h4")]
    H4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub label: &'static str,
    pub kraken_interval_minutes: u32,
    pub candles_kept: usize,
    pub refresh_seconds: u32,
}

impl Timeframe {
    /// Left to right, the order the chart shows them in.
    pub const ALL: [Timeframe; 3] = [Timeframe::M1, Timeframe::M15, Timeframe::H4];

    pub const fn spec(self) -> Spec {
        match self {
            Timeframe::M1 => Spec { label: "1m", kraken_interval_minutes: 1, candles_kept: 120, refresh_seconds: 15 },
            Timeframe::M15 => Spec { label: "15m", kraken_interval_minutes: 15, candles_kept: 96, refresh_seconds: 60 },
            Timeframe::H4 => Spec { label: "4h", kraken_interval_minutes: 240, candles_kept: 90, refresh_seconds: 300 },
        }
    }

    pub(crate) fn interval_seconds(self) -> f64 {
        f64::from(self.spec().kraken_interval_minutes * 60)
    }
}
