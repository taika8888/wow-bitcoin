//! The only place that knows Yahoo Finance's chart API wire format. Everything past these functions trusts the domain
//! types.

use serde_json::{Map, Value};

use crate::market::{Candle, Quote, Timeframe};
use crate::source::{BodyError, bounded, malformed, newest, positive};
use crate::time::Timestamp;

const CHART_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/ANT";
/// The latest trading day in 2-minute candles, pre-market and after-hours included. Its newest close is the newest
/// trade, which the meta's `regularMarketPrice` is not outside regular hours. 2m keeps the body near 20 KB; 5m and
/// coarser candles stop at 7 PM New York time, an hour before after-hours trading ends.
pub const QUOTE_URL: &str =
    "https://query1.finance.yahoo.com/v8/finance/chart/ANT?interval=2m&range=1d&includePrePost=true";

/// The shortest range that still holds `MAX_CANDLES` rows for the timeframe. Pre-market and after-hours trades are
/// included, like the quote, so the quote the chart adds to the newest candle is a price from that candle's session.
/// Yahoo has nothing for Nasdaq's overnight session, 8 PM to 4 AM New York time, and its 15m and 4h candles stop at
/// 7 PM.
pub fn chart_url(timeframe: Timeframe) -> String {
    let query = match timeframe {
        Timeframe::M1 => "interval=1m&range=5d",
        Timeframe::M15 => "interval=15m&range=1mo",
        Timeframe::H4 => "interval=4h&range=1y",
    };
    format!("{CHART_URL}?{query}&includePrePost=true")
}

/// The price is the newest close in the series, extended hours included, or the regular-session price when the day
/// has no trades yet. The reference price is the previous regular close, which on the first trading day is the IPO
/// price.
pub fn parse_quote(body: &[u8], fetched_at: Timestamp) -> Result<Quote, BodyError> {
    let result = chart_result(body)?;
    let meta = result.get("meta").and_then(Value::as_object).ok_or_else(|| malformed("result has no meta"))?;
    let field = |name: &str| {
        let value =
            meta.get(name).and_then(Value::as_f64).ok_or_else(|| malformed(format!("{name} is not a number")))?;
        positive(value, name)
    };
    let price = match newest_close(&result)? {
        Some(close) => close,
        None => field("regularMarketPrice")?,
    };
    Ok(Quote { price, reference: field("chartPreviousClose")?, fetched_at })
}

/// The last non-null close, the zero-range row stamped with the time of the request included.
fn newest_close(result: &Map<String, Value>) -> Result<Option<f64>, BodyError> {
    let Some(closes) = result
        .get("indicators")
        .and_then(|indicators| indicators.get("quote"))
        .and_then(Value::as_array)
        .and_then(|quotes| quotes.first())
        .and_then(|quote| quote.get("close"))
    else {
        return Ok(None);
    };
    let closes = closes.as_array().ok_or_else(|| malformed("close is not an array"))?;
    let Some(close) = closes.iter().rfind(|close| !close.is_null()) else { return Ok(None) };
    let close = close.as_f64().ok_or_else(|| malformed("the newest close is not a number"))?;
    positive(single_precision_decimal(close), "newest close").map(Some)
}

/// Yahoo's candles hold single-precision prices widened to JSON numbers, so 95.76 arrives as 95.76000213623047. The
/// shortest decimal that reads back as the same `f32` is the price Yahoo meant.
fn single_precision_decimal(value: f64) -> f64 {
    #[expect(clippy::cast_possible_truncation, reason = "the value came from an f32")]
    let narrow = value as f32;
    narrow.to_string().parse().unwrap_or(value)
}

/// Oldest first, the newest `MAX_CANDLES`. Rows with no trades carry nulls and are skipped. Yahoo ends the series with
/// a zero-range row stamped with the time of the request, or of the close once the market has closed. It is dropped, because the forming candle before it already
/// holds that trade and the chart adds the live quote to it. A forming candle still all null takes the tick's trade
/// instead, so the newest candle is the current interval's and later quotes land in it.
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
    let zero_range = |row: usize| {
        let prices = [open, high, low, close].map(|column| column.get(row).and_then(Value::as_f64));
        prices[0].is_some() && prices.iter().all(|price| *price == prices[0])
    };
    let live_tick = matches!(times.as_slice(), [.., before, last] if last - before < timeframe.interval_seconds())
        && zero_range(times.len() - 1);
    let rows = times.len() - usize::from(live_tick);
    let mut candles = Vec::with_capacity(rows);
    let cells_at = |row: usize| [open, high, low, close].map(|column| column.get(row).filter(|cell| !cell.is_null()));
    for (index, time) in times.iter().take(rows).enumerate() {
        let mut cells = cells_at(index);
        if live_tick && index + 1 == rows && cells.iter().all(Option::is_none) {
            cells = cells_at(rows);
        }
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
