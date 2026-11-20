//! The only place that knows Kraken's wire format. Everything past these functions trusts the domain types.

use serde_json::{Map, Value};

use crate::market::{Candle, Quote, Session, Timeframe};
use crate::source::{BodyError, bounded, malformed, newest, positive};
use crate::time::Timestamp;

pub const TICKER_URL: &str = "https://api.kraken.com/0/public/Ticker?pair=XBTUSD";

pub fn ohlc_url(timeframe: Timeframe) -> String {
    format!("https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval={}", timeframe.spec().interval_minutes)
}

/// The reference price is Kraken's open for the day.
pub fn parse_quote(body: &[u8], fetched_at: Timestamp) -> Result<Quote, BodyError> {
    let entry = pair_entry(body)?;
    let ticker = entry.as_object();
    let last = ticker
        .and_then(|ticker| ticker.get("c"))
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| malformed("ticker has no c[0]"))?;
    Ok(Quote {
        price: price(Some(last), "c[0]")?,
        reference: price(ticker.and_then(|t| t.get("o")), "o")?,
        session: Session::Regular,
        fetched_at,
    })
}

/// Oldest first, the newest `MAX_CANDLES`. One bad row rejects the response.
pub fn parse_candles(body: &[u8]) -> Result<Vec<Candle>, BodyError> {
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
        candles.push(bounded(candle, index)?);
    }
    Ok(newest(candles))
}

fn pair_entry(body: &[u8]) -> Result<Value, BodyError> {
    let Ok(Value::Object(mut root)) = serde_json::from_slice::<Value>(body) else {
        return Err(malformed("body is not a JSON object"));
    };
    if let Some(errors) = api_errors(&root) {
        return Err(BodyError::Api(errors));
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

/// Kraken sends prices as decimal strings; a JSON number is read too.
fn price(raw: Option<&Value>, field: &str) -> Result<f64, BodyError> {
    let value = match raw {
        Some(Value::String(text)) => text.parse::<f64>().ok(),
        Some(Value::Number(number)) => number.as_f64(),
        _ => None,
    };
    positive(value.ok_or_else(|| malformed(format!("{field} is not a number")))?, field)
}
