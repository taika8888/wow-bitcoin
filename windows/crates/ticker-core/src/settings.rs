use std::ops::RangeInclusive;

use serde::{Deserialize as _, Serialize};
use serde_json::{Map, Value};

use crate::market::{Timeframe, Zoom};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rgba {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl Rgba {
    pub const fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self { red, green, blue, alpha }
    }

    fn clamped(self) -> Self {
        let unit = |value: f64| value.clamp(0.0, 1.0);
        Self::new(unit(self.red), unit(self.green), unit(self.blue), unit(self.alpha))
    }

    /// All four components must be numbers, or the color is unreadable.
    fn parse(value: &Value) -> Option<Self> {
        let component = |key: &str| value.get(key).and_then(Value::as_f64);
        Some(Self::new(component("red")?, component("green")?, component("blue")?, component("alpha")?))
    }
}

/// A saved ticker origin in the shell's coordinates. Windows uses top-left physical pixels;
/// macOS uses bottom-left AppKit points. Each shell keeps its own settings store.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}

impl ScreenPoint {
    fn parse(value: &Value) -> Option<Self> {
        Some(Self { x: value.get("x")?.as_f64()?, y: value.get("y")?.as_f64()? })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub font_size: f64,
    pub opacity: f64,
    pub text_color: Rgba,
    pub up_color: Rgba,
    pub down_color: Rgba,
    pub decimals: u8,
    pub show_label: bool,
    pub show_change: bool,
    pub locked: bool,
    pub refresh_seconds: u32,
    #[serde(rename = "onlyWhenWoWActive")]
    pub only_when_wow_active: bool,
    /// None places the ticker at the default spot, top center of the primary monitor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<ScreenPoint>,
    /// The timeframe the chart opens on, which is the one last chosen.
    pub timeframe: Timeframe,
    pub zoom: Zoom,
    pub chart_width: f64,
    pub chart_height: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: 15.0,
            opacity: 0.55,
            text_color: Rgba::new(1.0, 1.0, 1.0, 1.0),
            up_color: Rgba::new(0.33, 0.86, 0.47, 1.0),
            down_color: Rgba::new(1.0, 0.42, 0.42, 1.0),
            decimals: 0,
            show_label: true,
            show_change: true,
            locked: false,
            refresh_seconds: 5,
            only_when_wow_active: false,
            position: None,
            timeframe: Timeframe::M15,
            zoom: Zoom::Standard,
            chart_width: 360.0,
            chart_height: 180.0,
        }
    }
}

impl Settings {
    pub const FONT_SIZE_RANGE: RangeInclusive<f64> = 9.0..=72.0;
    pub const OPACITY_RANGE: RangeInclusive<f64> = 0.0..=1.0;
    pub const DECIMALS_RANGE: RangeInclusive<u8> = 0..=2;
    pub const REFRESH_SECONDS_RANGE: RangeInclusive<u32> = 2..=300;
    pub const CHART_WIDTH_RANGE: RangeInclusive<f64> = 240.0..=900.0;
    pub const CHART_HEIGHT_RANGE: RangeInclusive<f64> = 120.0..=600.0;

    /// Missing, mistyped, or out-of-range keys fall back to the default or the nearest bound, so an old or
    /// hand-edited file still loads. Anything that is not a JSON object gives the defaults.
    pub fn decode(json: Option<&str>) -> Settings {
        match json.and_then(|text| serde_json::from_str::<Value>(text).ok()) {
            Some(Value::Object(map)) => Self::from_map(&map),
            _ => Settings::default(),
        }
    }

    fn from_map(map: &Map<String, Value>) -> Settings {
        let defaults = Settings::default();
        let number = |key: &str, fallback: f64| map.get(key).and_then(Value::as_f64).unwrap_or(fallback);
        let flag = |key: &str, fallback: bool| map.get(key).and_then(Value::as_bool).unwrap_or(fallback);
        let color = |key: &str, fallback: Rgba| map.get(key).and_then(Rgba::parse).unwrap_or(fallback);
        Settings {
            font_size: number("fontSize", defaults.font_size),
            opacity: number("opacity", defaults.opacity),
            text_color: color("textColor", defaults.text_color),
            up_color: color("upColor", defaults.up_color),
            down_color: color("downColor", defaults.down_color),
            decimals: map
                .get("decimals")
                .and_then(integer)
                .map_or(defaults.decimals, |value| saturate(value, Self::DECIMALS_RANGE)),
            show_label: flag("showLabel", defaults.show_label),
            show_change: flag("showChange", defaults.show_change),
            locked: flag("locked", defaults.locked),
            refresh_seconds: map
                .get("refreshSeconds")
                .and_then(integer)
                .map_or(defaults.refresh_seconds, |value| saturate(value, Self::REFRESH_SECONDS_RANGE)),
            only_when_wow_active: flag("onlyWhenWoWActive", defaults.only_when_wow_active),
            position: map.get("position").and_then(ScreenPoint::parse),
            timeframe: map
                .get("timeframe")
                .and_then(|value| Timeframe::deserialize(value).ok())
                .unwrap_or(defaults.timeframe),
            zoom: map.get("zoom").and_then(integer).and_then(Zoom::from_index).unwrap_or(defaults.zoom),
            chart_width: number("chartWidth", defaults.chart_width),
            chart_height: number("chartHeight", defaults.chart_height),
        }
        .clamped()
    }

    /// Every value pulled into its range and every color component into 0...1.
    pub fn clamped(self) -> Settings {
        let clamp = |value: f64, range: RangeInclusive<f64>| value.clamp(*range.start(), *range.end());
        Settings {
            font_size: clamp(self.font_size, Self::FONT_SIZE_RANGE),
            opacity: clamp(self.opacity, Self::OPACITY_RANGE),
            text_color: self.text_color.clamped(),
            up_color: self.up_color.clamped(),
            down_color: self.down_color.clamped(),
            decimals: self.decimals.clamp(*Self::DECIMALS_RANGE.start(), *Self::DECIMALS_RANGE.end()),
            refresh_seconds: self
                .refresh_seconds
                .clamp(*Self::REFRESH_SECONDS_RANGE.start(), *Self::REFRESH_SECONDS_RANGE.end()),
            chart_width: clamp(self.chart_width, Self::CHART_WIDTH_RANGE),
            chart_height: clamp(self.chart_height, Self::CHART_HEIGHT_RANGE),
            ..self
        }
    }

    /// Sorted keys, so equal settings always write the same bytes.
    pub fn json(&self) -> String {
        serde_json::to_value(self).map(|value| value.to_string()).unwrap_or_default()
    }
}

/// Match Swift Int decoding: integral JSON numbers (including 2.0 and 2e1), within signed 64-bit bounds.
fn integer(value: &Value) -> Option<i128> {
    if let Some(value) = value.as_i64() {
        return Some(i128::from(value));
    }
    let number = value.as_f64()?;
    (number.fract() == 0.0 && number > i64::MIN as f64 && number < -(i64::MIN as f64)).then_some(number as i128)
}

fn saturate<T: Copy + TryFrom<i128> + Into<i128>>(value: i128, range: RangeInclusive<T>) -> T {
    let (low, high) = ((*range.start()).into(), (*range.end()).into());
    T::try_from(value.clamp(low, high)).unwrap_or(*range.start())
}
