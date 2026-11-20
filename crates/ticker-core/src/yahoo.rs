//! The only place that knows Yahoo Finance's chart API wire format. Everything past these functions trusts the domain
//! types.

use std::ops::Range;

use serde_json::{Map, Value};

use crate::market::{Candle, Quote, Session, Timeframe};
use crate::source::{BodyError, bounded, malformed, newest, positive};
use crate::time::Timestamp;

const CHART_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/ANT";
/// Yahoo's 4h candles restart at 04:00, 09:30, 16:00 New York time, so they start on the half hour. New York's offset
/// from UTC is a whole number of half hours, so this holds in Unix seconds too.
const BUCKET_GRID_SECONDS: f64 = 1800.0;
/// Only a one-day range gives the data day's trading periods, and its last row is the latest trade of any session.
pub const QUOTE_URL: &str =
    "https://query1.finance.yahoo.com/v8/finance/chart/ANT?interval=4h&range=1d&includePrePost=true";

/// The shortest range that still holds `MAX_CANDLES` rows for the timeframe, pre-market and after-hours trades
/// included like the quote, so the quote the chart adds to the newest candle is a price from that candle's session.
pub fn chart_url(timeframe: Timeframe) -> String {
    let query = match timeframe {
        Timeframe::M1 => "interval=1m&range=5d",
        Timeframe::M15 => "interval=15m&range=60d",
        Timeframe::H4 => "interval=4h&range=2y",
    };
    format!("{CHART_URL}?{query}&includePrePost=true")
}

/// In the regular session the price is Yahoo's regular market price, against the previous close, which on the first
/// trading day is the IPO price. Once the last trade falls outside the session, the price is that pre-market or
/// after-hours trade, against the last regular close. A closing-auction print after 16:00 is still the regular close.
pub fn parse_quote(body: &[u8], fetched_at: Timestamp) -> Result<Quote, BodyError> {
    let result = chart_result(body)?;
    let meta = result.get("meta").and_then(Value::as_object).ok_or_else(|| malformed("result has no meta"))?;
    let number =
        |name: &str| meta.get(name).and_then(Value::as_f64).ok_or_else(|| malformed(format!("{name} is not a number")));
    let regular_price = positive(number("regularMarketPrice")?, "regularMarketPrice")?;
    let previous_close = positive(number("chartPreviousClose")?, "chartPreviousClose")?;
    let regular_time = number("regularMarketTime")?;
    let regular = regular_period(meta)?;
    let quote = |price, reference, session| Quote { price, reference, session, fetched_at };
    Ok(match last_trade(&result)? {
        Some((time, price)) if time > regular_time && !regular.contains(&time) => {
            quote(price, regular_price, if time < regular.start { Session::Pre } else { Session::Post })
        }
        _ => quote(regular_price, previous_close, Session::Regular),
    })
}

/// `tradingPeriods.regular[0][0]`, in Unix seconds.
fn regular_period(meta: &Map<String, Value>) -> Result<Range<f64>, BodyError> {
    let period = meta
        .get("tradingPeriods")
        .and_then(|periods| periods.get("regular"))
        .and_then(|days| days.get(0))
        .and_then(|day| day.get(0))
        .ok_or_else(|| malformed("no tradingPeriods.regular[0][0]"))?;
    let bound = |name: &str| {
        period
            .get(name)
            .and_then(Value::as_f64)
            .ok_or_else(|| malformed(format!("regular period {name} is not a number")))
    };
    Ok(bound("start")?..bound("end")?)
}

/// The time and close of the newest row with a close.
fn last_trade(result: &Map<String, Value>) -> Result<Option<(f64, f64)>, BodyError> {
    let Some(series) = Series::of(result)? else { return Ok(None) };
    let closes = series.column("close")?;
    let Some((time, close)) = series.times.iter().zip(closes).rev().find(|(_, close)| !close.is_null()) else {
        return Ok(None);
    };
    let close = close.as_f64().ok_or_else(|| malformed("the last close is not a number"))?;
    Ok(Some((*time, positive(close, "the last close")?)))
}

/// Oldest first, the newest `MAX_CANDLES`. Rows with no trades carry nulls and are skipped. While a session trades,
/// Yahoo ends the series with a zero-range row stamped with the time of the last trade. It is dropped, because the
/// forming candle before it already holds that trade and the chart adds the live quote to it. A forming candle still
/// all null takes the tick's trade instead, so the newest candle is the current interval's and later quotes land in it.
///
/// Every real candle starts on the grid of its interval, capped at half an hour, and the tick almost never does. A gap
/// test cannot tell the tick from a 4h candle that a session boundary cut short.
pub fn parse_candles(body: &[u8], timeframe: Timeframe) -> Result<Vec<Candle>, BodyError> {
    let result = chart_result(body)?;
    let Some(series) = Series::of(&result)? else { return Ok(Vec::new()) };
    let column = |name: &str| series.column(name);
    let (open, high, low, close) = (column("open")?, column("high")?, column("low")?, column("close")?);
    let times = series.times;
    let grid = timeframe.interval_seconds().min(BUCKET_GRID_SECONDS);
    let live_tick = matches!(times.as_slice(), [.., before, last] if last % grid != 0.0 || last <= before);
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

/// The rows of a chart answer: their times, and `indicators.quote[0]`, which holds a column of values per field.
struct Series<'a> {
    times: Vec<f64>,
    quote: &'a Map<String, Value>,
}

impl<'a> Series<'a> {
    /// None when the answer has no rows.
    fn of(result: &'a Map<String, Value>) -> Result<Option<Self>, BodyError> {
        let Some(times) = result.get("timestamp") else { return Ok(None) };
        let times = times
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
        Ok(Some(Series { times, quote }))
    }

    fn column(&self, name: &str) -> Result<&'a Vec<Value>, BodyError> {
        self.quote.get(name).and_then(Value::as_array).ok_or_else(|| malformed(format!("quote has no {name} array")))
    }
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
