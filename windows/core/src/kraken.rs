use crate::market::{Candle, Quote, Time, Timeframe};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KrakenError {
    /// Kraken answered with a non-empty `error` array.
    Api(Vec<String>),
    /// The body is not shaped like the documented response.
    Malformed(String),
    /// A price is not finite and positive, or a candle's high and low do not bound its open and close.
    InvalidValue(String),
}

pub const TICKER_URL: &str = "https://api.kraken.com/0/public/Ticker?pair=XBTUSD";

pub fn ohlc_url(timeframe: Timeframe) -> String {
    format!(
        "https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval={}",
        timeframe.spec().kraken_interval_minutes
    )
}

pub fn parse_quote(body: &[u8], fetched_at: Time) -> Result<Quote, KrakenError> {
    let entry = pair_entry(body)?;
    let last = entry
        .get("c")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| KrakenError::Malformed("ticker has no c[0]".into()))?;
    Ok(Quote {
        price: price(Some(last), "c[0]")?,
        open_24h: price(entry.get("o"), "o")?,
        fetched_at,
    })
}

/// Oldest first, trimmed to the timeframe's `candles_kept`. One bad row rejects the response.
pub fn parse_candles(body: &[u8], timeframe: Timeframe) -> Result<Vec<Candle>, KrakenError> {
    let entry = pair_entry(body)?;
    let rows = entry
        .as_array()
        .ok_or_else(|| KrakenError::Malformed("OHLC result is not an array".into()))?;
    let mut candles = Vec::with_capacity(rows.len());
    for (index, raw) in rows.iter().enumerate() {
        let row = raw
            .as_array()
            .filter(|row| row.len() >= 5)
            .ok_or_else(|| KrakenError::Malformed(format!("OHLC row {index}")))?;
        let time = row[0]
            .as_f64()
            .ok_or_else(|| KrakenError::Malformed(format!("OHLC row {index}")))?;
        let candle = Candle {
            time: Time(time),
            open: price(Some(&row[1]), &format!("row {index} open"))?,
            high: price(Some(&row[2]), &format!("row {index} high"))?,
            low: price(Some(&row[3]), &format!("row {index} low"))?,
            close: price(Some(&row[4]), &format!("row {index} close"))?,
        };
        if candle.low > candle.open.min(candle.close) || candle.high < candle.open.max(candle.close)
        {
            return Err(KrakenError::InvalidValue(format!(
                "row {index} high/low do not bound open/close"
            )));
        }
        candles.push(candle);
    }
    let keep = timeframe.spec().candles_kept;
    Ok(candles.split_off(candles.len().saturating_sub(keep)))
}

fn pair_entry(body: &[u8]) -> Result<Value, KrakenError> {
    let Ok(Value::Object(mut root)) = serde_json::from_slice::<Value>(body) else {
        return Err(KrakenError::Malformed("body is not a JSON object".into()));
    };
    if let Some(errors) = root.get("error").and_then(Value::as_array) {
        let errors: Option<Vec<String>> = errors
            .iter()
            .map(|e| e.as_str().map(String::from))
            .collect();
        if let Some(errors) = errors.filter(|e| !e.is_empty()) {
            return Err(KrakenError::Api(errors));
        }
    }
    let Some(Value::Object(result)) = root.remove("result") else {
        return Err(KrakenError::Malformed("no result object".into()));
    };
    let mut pairs: Vec<(String, Value)> = result.into_iter().filter(|(k, _)| k != "last").collect();
    if pairs.len() != 1 {
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        return Err(KrakenError::Malformed(format!(
            "expected one pair, got {keys:?}"
        )));
    }
    Ok(pairs.remove(0).1)
}

/// Accepts a JSON string or number, so "nan", "inf" and "-0.0" parse and then fail the finite-and-positive check.
fn price(raw: Option<&Value>, field: &str) -> Result<f64, KrakenError> {
    let value = match raw {
        Some(Value::String(text)) => text.parse::<f64>().ok(),
        Some(Value::Number(number)) => number.as_f64(),
        _ => None,
    }
    .ok_or_else(|| KrakenError::Malformed(format!("{field} is not a number")))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(KrakenError::InvalidValue(format!("{field} = {value}")));
    }
    Ok(value)
}
