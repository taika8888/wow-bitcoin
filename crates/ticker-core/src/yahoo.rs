//! The only place that knows Yahoo Finance's chart API wire format. Everything past these functions trusts the domain
//! types.

use serde_json::{Map, Value};

use crate::market::{Candle, Quote, Timeframe};
use crate::source::{BodyError, bounded, malformed, newest, positive};
use crate::time::Timestamp;

const CHART_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/ANT";
pub const QUOTE_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/ANT?interval=1d&range=1d";

/// The shortest range that still holds `MAX_CANDLES` rows for the timeframe. Regular-session trades only, like the
/// quote, so the quote the chart adds to the newest candle is a price from that candle's session.
pub fn chart_url(timeframe: Timeframe) -> String {
    let query = match timeframe {
        Timeframe::M1 => "interval=1m&range=5d",
        Timeframe::M15 => "interval=15m&range=60d",
        Timeframe::H4 => "interval=4h&range=2y",
    };
    format!("{CHART_URL}?{query}")
}

/// The reference price is the previous close, which on the first trading day is the IPO price.
pub fn parse_quote(body: &[u8], fetched_at: Timestamp) -> Result<Quote, BodyError> {
    let result = chart_result(body)?;
    let meta = result.get("meta").and_then(Value::as_object).ok_or_else(|| malformed("result has no meta"))?;
    let field = |name: &str| {
        let value =
            meta.get(name).and_then(Value::as_f64).ok_or_else(|| malformed(format!("{name} is not a number")))?;
        positive(value, name)
    };
    Ok(Quote { price: field("regularMarketPrice")?, reference: field("chartPreviousClose")?, fetched_at })
}

/// Oldest first, the newest `MAX_CANDLES`. Rows with no trades carry nulls and are skipped. Yahoo ends the series with
/// a zero-range row stamped with the time of the request; it is dropped, because the forming candle before it already
/// holds that trade and the chart adds the live quote to it.
pub fn parse_candles(body: &[u8], timeframe: Timeframe) -> Result<Vec<Candle>, BodyError> {
    let result = chart_result(body)?;
    let Some(times) = result.get("timestamp") else { return Ok(Vec::new()) };
    let times: Vec<f64> = times
        .as_array()
        .ok_or_else(|| malformed("timestamp is not an array"))?
        .iter()
        .map(|time| time.as_f64().ok_or_else(|| malformed("a timestamp is not a number")))
        .collect::<Result<_, _>>()?;
    let quote = result
        .get("indicators")
        .and_then(|indicators| indicators.get("quote"))
        .and_then(Value::as_array)
        .and_then(|quotes| quotes.first())
        .and_then(Value::as_object)
        .ok_or_else(|| malformed("no indicators.quote[0]"))?;
    let column = |name: &str| {
        quote.get(name).and_then(Value::as_array).ok_or_else(|| malformed(format!("quote has no {name} array")))
    };
    let (open, high, low, close) = (column("open")?, column("high")?, column("low")?, column("close")?);
    let live_tick = matches!(times.as_slice(), [.., before, last] if last - before < timeframe.interval_seconds());
    let rows = times.len() - usize::from(live_tick);
    let mut candles = Vec::with_capacity(rows);
    for (index, time) in times.iter().take(rows).enumerate() {
        let cells = [open, high, low, close].map(|column| column.get(index).filter(|cell| !cell.is_null()));
        let [Some(o), Some(h), Some(l), Some(c)] = cells else { continue };
        let field = |cell: &Value, name: &str| {
            let value = cell.as_f64().ok_or_else(|| malformed(format!("row {index} {name} is not a number")))?;
            positive(value, &format!("row {index} {name}"))
        };
        let candle = Candle {
            time: Timestamp::from_unix_seconds(*time),
            open: field(o, "open")?,
            high: field(h, "high")?,
            low: field(l, "low")?,
            close: field(c, "close")?,
        };
        candles.push(bounded(candle, index)?);
    }
    Ok(newest(candles))
}

/// `chart.result[0]`, or the error Yahoo put in `chart.error`.
fn chart_result(body: &[u8]) -> Result<Map<String, Value>, BodyError> {
    let Ok(Value::Object(mut root)) = serde_json::from_slice::<Value>(body) else {
        return Err(malformed("body is not a JSON object"));
    };
    let Some(Value::Object(mut chart)) = root.remove("chart") else {
        return Err(malformed("no chart object"));
    };
    match chart.get("error") {
        None | Some(Value::Null) => {}
        Some(error) => {
            let text = error.get("description").or_else(|| error.get("code")).and_then(Value::as_str);
            return Err(BodyError::Api(vec![text.map_or_else(|| error.to_string(), str::to_string)]));
        }
    }
    match chart.remove("result") {
        Some(Value::Array(mut results)) if !results.is_empty() => match results.swap_remove(0) {
            Value::Object(result) => Ok(result),
            _ => Err(malformed("chart.result[0] is not an object")),
        },
        _ => Err(malformed("no chart.result[0]")),
    }
}
