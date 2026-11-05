//! The only place that knows Kraken's wire format. Everything past these functions trusts the domain types.

use std::time::Duration;

use serde_json::{Map, Value};

use crate::market::{Candle, Quote, Timeframe};
use crate::time::Timestamp;

pub const TICKER_URL: &str = "https://api.kraken.com/0/public/Ticker?pair=XBTUSD";
pub const CANDLES_TIMEOUT: Duration = Duration::from_secs(10);
/// Kraken's OHLC endpoint answers with 721 rows, the newest still forming. Keeping 720 lets the chart zoom out with no
/// extra request.
pub const MAX_CANDLES: usize = 720;

pub fn ohlc_url(timeframe: Timeframe) -> String {
    format!("https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval={}", timeframe.spec().kraken_interval_minutes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KrakenError {
    /// Kraken answered with a non-empty `error` array.
    Api(Vec<String>),
    /// The body is not shaped like the documented response.
    Malformed(String),
    /// A price is not finite and positive, or a candle's high and low do not bound its open and close.
    InvalidValue(String),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchFailure {
    Unreachable,
    Status(u16),
    /// Kraken answered 200 with a body that is not a usable response.
    Rejected(KrakenError),
}

impl FetchFailure {
    pub fn message(&self) -> String {
        match self {
            FetchFailure::Unreachable => "Kraken is not answering".to_string(),
            FetchFailure::Status(code) => format!("Kraken answered HTTP {code}"),
            FetchFailure::Rejected(KrakenError::Api(errors)) => format!("Kraken refused: {}", errors.join(", ")),
            FetchFailure::Rejected(_) => "Kraken sent candles this app cannot read".to_string(),
        }
    }
}

pub fn quote_from(outcome: &HttpOutcome, fetched_at: Timestamp) -> Result<Quote, FetchFailure> {
    parse_quote(ok_body(outcome)?, fetched_at).map_err(FetchFailure::Rejected)
}

pub fn candles_from(outcome: &HttpOutcome) -> Result<Vec<Candle>, FetchFailure> {
    parse_candles(ok_body(outcome)?).map_err(FetchFailure::Rejected)
}

fn ok_body(outcome: &HttpOutcome) -> Result<&[u8], FetchFailure> {
    match outcome {
        HttpOutcome::Answered { status: 200, body } => Ok(body),
        HttpOutcome::Answered { status, .. } => Err(FetchFailure::Status(*status)),
        HttpOutcome::Unreachable => Err(FetchFailure::Unreachable),
    }
}

pub fn parse_quote(body: &[u8], fetched_at: Timestamp) -> Result<Quote, KrakenError> {
    let entry = pair_entry(body)?;
    let ticker = entry.as_object();
    let last = ticker
        .and_then(|ticker| ticker.get("c"))
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| malformed("ticker has no c[0]"))?;
    Ok(Quote { price: price(Some(last), "c[0]")?, open_24h: price(ticker.and_then(|t| t.get("o")), "o")?, fetched_at })
}

/// Oldest first, the newest `MAX_CANDLES`. One bad row rejects the response.
pub fn parse_candles(body: &[u8]) -> Result<Vec<Candle>, KrakenError> {
    let entry = pair_entry(body)?;
    let rows = entry.as_array().ok_or_else(|| malformed("OHLC result is not an array"))?;
    let mut candles = Vec::with_capacity(rows.len());
    for (index, raw) in rows.iter().enumerate() {
        let row = raw.as_array().filter(|row| row.len() >= 5).ok_or_else(|| malformed(format!("OHLC row {index}")))?;
        let time = row[0].as_f64().ok_or_else(|| malformed(format!("OHLC row {index}")))?;
        let field = |column: usize, name: &str| price(Some(&row[column]), &format!("row {index} {name}"));
        let candle = Candle {
            time: Timestamp::from_unix_seconds(time),
            open: field(1, "open")?,
            high: field(2, "high")?,
            low: field(3, "low")?,
            close: field(4, "close")?,
        };
        if candle.low > candle.open.min(candle.close) || candle.high < candle.open.max(candle.close) {
            return Err(KrakenError::InvalidValue(format!("row {index} high/low do not bound open/close")));
        }
        candles.push(candle);
    }
    Ok(candles.split_off(candles.len().saturating_sub(MAX_CANDLES)))
}

fn pair_entry(body: &[u8]) -> Result<Value, KrakenError> {
    let Ok(Value::Object(mut root)) = serde_json::from_slice::<Value>(body) else {
        return Err(malformed("body is not a JSON object"));
    };
    if let Some(errors) = api_errors(&root) {
        return Err(KrakenError::Api(errors));
    }
    let Some(Value::Object(result)) = root.remove("result") else {
        return Err(malformed("no result object"));
    };
    let mut pairs: Vec<(String, Value)> = result.into_iter().filter(|(key, _)| key != "last").collect();
    if pairs.len() == 1 {
        return Ok(pairs.remove(0).1);
    }
    let keys: Vec<String> = pairs.into_iter().map(|(key, _)| key).collect();
    Err(malformed(format!("expected one pair, got {keys:?}")))
}

/// A non-empty array of strings. Anything else in `error` is not Kraken's documented error shape and is ignored.
fn api_errors(root: &Map<String, Value>) -> Option<Vec<String>> {
    let errors: Option<Vec<String>> =
        root.get("error")?.as_array()?.iter().map(|error| error.as_str().map(str::to_string)).collect();
    errors.filter(|errors| !errors.is_empty())
}

fn price(raw: Option<&Value>, field: &str) -> Result<f64, KrakenError> {
    let value = match raw {
        Some(Value::String(text)) => text.parse::<f64>().ok(),
        Some(Value::Number(number)) => number.as_f64(),
        _ => None,
    };
    let value = value.ok_or_else(|| malformed(format!("{field} is not a number")))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(KrakenError::InvalidValue(format!("{field} = {value}")));
    }
    Ok(value)
}

fn malformed(detail: impl Into<String>) -> KrakenError {
    KrakenError::Malformed(detail.into())
}
