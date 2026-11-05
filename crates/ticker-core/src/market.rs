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
    /// How many of the newest candles the chart shows at `Zoom::Standard`.
    pub candles_shown: usize,
    pub refresh_seconds: u32,
}

impl Timeframe {
    /// Left to right, the order the chart shows them in.
    pub const ALL: [Timeframe; 3] = [Timeframe::M1, Timeframe::M15, Timeframe::H4];

    pub const fn spec(self) -> Spec {
        match self {
            Timeframe::M1 => Spec { label: "1m", kraken_interval_minutes: 1, candles_shown: 120, refresh_seconds: 15 },
            Timeframe::M15 => {
                Spec { label: "15m", kraken_interval_minutes: 15, candles_shown: 96, refresh_seconds: 60 }
            }
            Timeframe::H4 => {
                Spec { label: "4h", kraken_interval_minutes: 240, candles_shown: 90, refresh_seconds: 300 }
            }
        }
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    pub(crate) fn interval_seconds(self) -> f64 {
        f64::from(self.spec().kraken_interval_minutes * 60)
    }
}

/// A rung on the chart's zoom ladder. Each one shows a multiple of the timeframe's `candles_shown`, or every candle held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Zoom {
    Quarter,
    Half,
    #[default]
    Standard,
    Double,
    Quadruple,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZoomStep {
    /// Fewer, wider candles.
    In,
    Out,
}

impl Zoom {
    /// Narrowest first; a rung's index is what the settings file stores.
    pub const ALL: [Zoom; 6] = [Zoom::Quarter, Zoom::Half, Zoom::Standard, Zoom::Double, Zoom::Quadruple, Zoom::All];

    pub fn from_index(index: i128) -> Option<Zoom> {
        usize::try_from(index).ok().and_then(|index| Self::ALL.get(index).copied())
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Zoom::Quarter => "×1/4",
            Zoom::Half => "×1/2",
            Zoom::Standard => "×1",
            Zoom::Double => "×2",
            Zoom::Quadruple => "×4",
            Zoom::All => "all",
        }
    }

    /// One rung toward the step's direction, staying put at either end of the ladder.
    pub fn stepped(self, step: ZoomStep) -> Zoom {
        let index = match step {
            ZoomStep::In => self.index().checked_sub(1),
            ZoomStep::Out => Some(self.index() + 1),
        };
        index.and_then(|index| Self::ALL.get(index).copied()).unwrap_or(self)
    }

    /// How many of the newest candles to show out of `available`, at least one when there are any.
    pub fn visible_count(self, timeframe: Timeframe, available: usize) -> usize {
        let shown = timeframe.spec().candles_shown;
        let wanted = match self {
            Zoom::Quarter => shown / 4,
            Zoom::Half => shown / 2,
            Zoom::Standard => shown,
            Zoom::Double => shown * 2,
            Zoom::Quadruple => shown * 4,
            Zoom::All => available,
        };
        wanted.max(1).min(available)
    }
}

impl Serialize for Zoom {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.index() as u64)
    }
}
