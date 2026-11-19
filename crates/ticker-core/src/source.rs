//! Where prices come from: the one place that maps a source to its URLs and its parser, and the checks every parser
//! shares. Each source's wire format stays in its own module.

use std::time::Duration;

use crate::market::{Candle, Quote, Timeframe};
use crate::time::Timestamp;
use crate::{kraken, yahoo};

pub const CANDLES_TIMEOUT: Duration = Duration::from_secs(10);
/// Kraken's OHLC endpoint answers with 721 rows, the newest still forming. Keeping 720 lets the chart zoom out with no
/// extra request.
pub const MAX_CANDLES: usize = 720;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// Kraken's public API, pair XBTUSD.
    Kraken,
    /// Yahoo Finance's public chart API, symbol ANT.
    Yahoo,
}

impl Source {
    pub const fn name(self) -> &'static str {
        match self {
            Source::Kraken => "Kraken",
            Source::Yahoo => "Yahoo Finance",
        }
    }

    pub const fn quote_url(self) -> &'static str {
        match self {
            Source::Kraken => kraken::TICKER_URL,
            Source::Yahoo => yahoo::QUOTE_URL,
        }
    }

    pub fn candles_url(self, timeframe: Timeframe) -> String {
        match self {
            Source::Kraken => kraken::ohlc_url(timeframe),
            Source::Yahoo => yahoo::chart_url(timeframe),
        }
    }

    pub fn quote_from(self, outcome: &HttpOutcome, fetched_at: Timestamp) -> Result<Quote, FetchFailure> {
        let body = self.ok_body(outcome)?;
        let quote = match self {
            Source::Kraken => kraken::parse_quote(body, fetched_at),
            Source::Yahoo => yahoo::parse_quote(body, fetched_at),
        };
        quote.map_err(|error| self.failure(FetchProblem::Rejected(error)))
    }

    pub fn candles_from(self, outcome: &HttpOutcome, timeframe: Timeframe) -> Result<Vec<Candle>, FetchFailure> {
        let body = self.ok_body(outcome)?;
        let candles = match self {
            Source::Kraken => kraken::parse_candles(body),
            Source::Yahoo => yahoo::parse_candles(body, timeframe),
        };
        candles.map_err(|error| self.failure(FetchProblem::Rejected(error)))
    }

    fn ok_body(self, outcome: &HttpOutcome) -> Result<&[u8], FetchFailure> {
        match outcome {
            HttpOutcome::Answered { status: 200, body } => Ok(body),
            HttpOutcome::Answered { status, .. } => Err(self.failure(FetchProblem::Status(*status))),
            HttpOutcome::Unreachable => Err(self.failure(FetchProblem::Unreachable)),
        }
    }

    fn failure(self, problem: FetchProblem) -> FetchFailure {
        FetchFailure { source: self, problem }
    }
}

/// How a request the shell ran on the core's behalf ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpOutcome {
    Answered {
        status: u16,
        body: Vec<u8>,
    },
    /// No answer at all: offline, timed out, refused.
    Unreachable,
}

/// Why a 200 answer is not a usable response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyError {
    /// The source answered with its own error in the body.
    Api(Vec<String>),
    /// The body is not shaped like the documented response.
    Malformed(String),
    /// A price is not finite and positive, or a candle's high and low do not bound its open and close.
    InvalidValue(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchFailure {
    pub source: Source,
    pub problem: FetchProblem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchProblem {
    Unreachable,
    Status(u16),
    Rejected(BodyError),
}

impl FetchFailure {
    pub fn message(&self) -> String {
        let name = self.source.name();
        match &self.problem {
            FetchProblem::Unreachable => format!("{name} is not answering"),
            FetchProblem::Status(code) => format!("{name} answered HTTP {code}"),
            FetchProblem::Rejected(BodyError::Api(errors)) => format!("{name} refused: {}", errors.join(", ")),
            FetchProblem::Rejected(_) => format!("{name} sent data this app cannot read"),
        }
    }
}

/// A price must be finite and positive, or the response is rejected.
pub(crate) fn positive(value: f64, field: &str) -> Result<f64, BodyError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(BodyError::InvalidValue(format!("{field} = {value}")));
    }
    Ok(value)
}

/// A candle's high and low must bound its open and close.
pub(crate) fn bounded(candle: Candle, row: usize) -> Result<Candle, BodyError> {
    if candle.low > candle.open.min(candle.close) || candle.high < candle.open.max(candle.close) {
        return Err(BodyError::InvalidValue(format!("row {row} high/low do not bound open/close")));
    }
    Ok(candle)
}

/// The newest `MAX_CANDLES`, oldest first.
pub(crate) fn newest(mut candles: Vec<Candle>) -> Vec<Candle> {
    candles.split_off(candles.len().saturating_sub(MAX_CANDLES))
}

pub(crate) fn malformed(detail: impl Into<String>) -> BodyError {
    BodyError::Malformed(detail.into())
}
