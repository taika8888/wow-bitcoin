use crate::market::Timeframe;
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use std::ops::RangeInclusive;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rgba {
    #[serde(serialize_with = "number")]
    pub red: f64,
    #[serde(serialize_with = "number")]
    pub green: f64,
    #[serde(serialize_with = "number")]
    pub blue: f64,
    #[serde(serialize_with = "number")]
    pub alpha: f64,
}

impl Rgba {
    pub const fn opaque(red: f64, green: f64, blue: f64) -> Rgba {
        Rgba {
            red,
            green,
            blue,
            alpha: 1.0,
        }
    }

    pub fn with_alpha(self, alpha: f64) -> Rgba {
        Rgba { alpha, ..self }
    }

    fn clamped(self) -> Rgba {
        let unit = |v: f64| v.clamp(0.0, 1.0);
        Rgba {
            red: unit(self.red),
            green: unit(self.green),
            blue: unit(self.blue),
            alpha: unit(self.alpha),
        }
    }
}

/// The ticker line's top-left corner. Same JSON key and shape as macOS, different space: Windows virtual-screen
/// physical pixels, y down. Blobs are per machine and `placement::frame` pulls any stray value back on screen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ScreenPoint {
    #[serde(serialize_with = "number")]
    pub x: f64,
    #[serde(serialize_with = "number")]
    pub y: f64,
}

/// Same keys, defaults and ranges as `Settings.swift`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(serialize_with = "number")]
    pub font_size: f64,
    #[serde(serialize_with = "number")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<ScreenPoint>,
    pub timeframe: Timeframe,
    #[serde(serialize_with = "number")]
    pub chart_width: f64,
    #[serde(serialize_with = "number")]
    pub chart_height: f64,
}

impl Settings {
    pub const FONT_SIZE: RangeInclusive<f64> = 9.0..=72.0;
    pub const OPACITY: RangeInclusive<f64> = 0.0..=1.0;
    pub const DECIMALS: RangeInclusive<u8> = 0..=2;
    pub const REFRESH_SECONDS: RangeInclusive<u32> = 2..=300;
    pub const CHART_WIDTH: RangeInclusive<f64> = 240.0..=900.0;
    pub const CHART_HEIGHT: RangeInclusive<f64> = 120.0..=600.0;

    /// Missing, mistyped or out-of-range keys fall back to the default or the nearest bound, key by key, so an old
    /// or hand-edited blob still loads. Written by hand because serde's derive rejects the whole struct on one bad key.
    pub fn decode(json: Option<&str>) -> Settings {
        let defaults = Settings::default();
        let Some(Ok(Value::Object(blob))) = json.map(serde_json::from_str::<Value>) else {
            return defaults;
        };
        let get = |key: &str| blob.get(key);
        let float = |key: &str, fallback: f64, range: RangeInclusive<f64>| {
            get(key)
                .and_then(Value::as_f64)
                .unwrap_or(fallback)
                .clamp(*range.start(), *range.end())
        };
        let int = |key: &str, fallback: i64, range: (i64, i64)| {
            get(key)
                .and_then(Value::as_i64)
                .unwrap_or(fallback)
                .clamp(range.0, range.1)
        };
        let flag =
            |key: &str, fallback: bool| get(key).and_then(Value::as_bool).unwrap_or(fallback);
        let color = |key: &str, fallback: Rgba| {
            get(key)
                .and_then(Value::as_object)
                .and_then(rgba)
                .unwrap_or(fallback)
                .clamped()
        };
        Settings {
            font_size: float("fontSize", defaults.font_size, Self::FONT_SIZE),
            opacity: float("opacity", defaults.opacity, Self::OPACITY),
            text_color: color("textColor", defaults.text_color),
            up_color: color("upColor", defaults.up_color),
            down_color: color("downColor", defaults.down_color),
            decimals: int(
                "decimals",
                defaults.decimals.into(),
                (
                    (*Self::DECIMALS.start()).into(),
                    (*Self::DECIMALS.end()).into(),
                ),
            ) as u8,
            show_label: flag("showLabel", defaults.show_label),
            show_change: flag("showChange", defaults.show_change),
            locked: flag("locked", defaults.locked),
            refresh_seconds: int(
                "refreshSeconds",
                defaults.refresh_seconds.into(),
                (
                    (*Self::REFRESH_SECONDS.start()).into(),
                    (*Self::REFRESH_SECONDS.end()).into(),
                ),
            ) as u32,
            only_when_wow_active: flag("onlyWhenWoWActive", defaults.only_when_wow_active),
            position: get("position")
                .and_then(Value::as_object)
                .and_then(screen_point)
                .or(defaults.position),
            timeframe: get("timeframe")
                .and_then(Value::as_str)
                .and_then(Timeframe::from_raw)
                .unwrap_or(defaults.timeframe),
            chart_width: float("chartWidth", defaults.chart_width, Self::CHART_WIDTH),
            chart_height: float("chartHeight", defaults.chart_height, Self::CHART_HEIGHT),
        }
    }

    /// Sorted keys, and whole numbers written without a fraction, which is exactly what Swift's
    /// `JSONEncoder(.sortedKeys)` writes for the same values.
    pub fn to_json(&self) -> String {
        serde_json::to_value(self)
            .expect("Settings always serializes")
            .to_string()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            font_size: 15.0,
            opacity: 0.55,
            text_color: Rgba::opaque(1.0, 1.0, 1.0),
            up_color: Rgba::opaque(0.33, 0.86, 0.47),
            down_color: Rgba::opaque(1.0, 0.42, 0.42),
            decimals: 0,
            show_label: true,
            show_change: true,
            locked: false,
            refresh_seconds: 5,
            only_when_wow_active: false,
            position: None,
            timeframe: Timeframe::M15,
            chart_width: 360.0,
            chart_height: 180.0,
        }
    }
}

/// The model check keeps visited states in a HashSet. Hashing only the discrete fields keeps Hash consistent with
/// the derived PartialEq (equal settings hash equal) without having to normalize -0.0 in the float fields.
impl Eq for Settings {}
impl std::hash::Hash for Settings {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (
            self.decimals,
            self.show_label,
            self.show_change,
            self.locked,
            self.refresh_seconds,
            self.only_when_wow_active,
            self.position.is_some(),
            self.timeframe,
        )
            .hash(state);
    }
}

fn rgba(object: &Map<String, Value>) -> Option<Rgba> {
    let channel = |key: &str| object.get(key).and_then(Value::as_f64);
    Some(Rgba {
        red: channel("red")?,
        green: channel("green")?,
        blue: channel("blue")?,
        alpha: channel("alpha")?,
    })
}

fn screen_point(object: &Map<String, Value>) -> Option<ScreenPoint> {
    Some(ScreenPoint {
        x: object.get("x").and_then(Value::as_f64)?,
        y: object.get("y").and_then(Value::as_f64)?,
    })
}

fn number<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

/// One row of the Settings window. The shell builds one native control per row and never names a setting itself.
pub struct Field {
    /// The JSON key, which is also the control's identity.
    pub key: &'static str,
    pub label: &'static str,
    pub section: &'static str,
    pub kind: FieldKind,
}

pub enum FieldKind {
    /// Trackbar with a live value label, in the setting's own unit.
    Slider {
        range: RangeInclusive<f64>,
        step: f64,
        unit: &'static str,
        get: fn(&Settings) -> f64,
        set: fn(&mut Settings, f64),
    },
    /// Up-down (spin) control.
    Stepper {
        range: RangeInclusive<u32>,
        format: fn(u32) -> String,
        get: fn(&Settings) -> u32,
        set: fn(&mut Settings, u32),
    },
    /// Radio row.
    Choice {
        options: &'static [&'static str],
        get: fn(&Settings) -> usize,
        set: fn(&mut Settings, usize),
    },
    Toggle {
        get: fn(&Settings) -> bool,
        set: fn(&mut Settings, bool),
    },
    /// A ChooseColor swatch for red, green and blue, and an alpha slider beside it (ChooseColor has no alpha).
    /// Each control edits its part of the one color and hands the whole color to `set`.
    Color {
        get: fn(&Settings) -> Rgba,
        set: fn(&mut Settings, Rgba),
    },
    /// Button that applies a fixed edit, disabled while `enabled` is false.
    Action {
        enabled: fn(&Settings) -> bool,
        apply: fn(&mut Settings),
    },
}

/// Mirrors SettingsView.swift's sections and order. `timeframe` has no row: it is chosen on the chart.
pub static FORM: &[Field] = &[
    Field {
        key: "fontSize",
        label: "Size",
        section: "Text",
        kind: FieldKind::Slider {
            range: Settings::FONT_SIZE,
            step: 1.0,
            unit: "pt",
            get: |s| s.font_size,
            set: |s, v| s.font_size = v,
        },
    },
    Field {
        key: "decimals",
        label: "Decimals",
        section: "Text",
        kind: FieldKind::Choice {
            options: &["0", "1", "2"],
            get: |s| s.decimals.into(),
            set: |s, i| s.decimals = i as u8,
        },
    },
    Field {
        key: "showLabel",
        label: "Show BTC label",
        section: "Text",
        kind: FieldKind::Toggle {
            get: |s| s.show_label,
            set: |s, v| s.show_label = v,
        },
    },
    Field {
        key: "showChange",
        label: "Show change since the daily open",
        section: "Text",
        kind: FieldKind::Toggle {
            get: |s| s.show_change,
            set: |s, v| s.show_change = v,
        },
    },
    Field {
        key: "textColor",
        label: "Price",
        section: "Colors",
        kind: FieldKind::Color {
            get: |s| s.text_color,
            set: |s, c| s.text_color = c,
        },
    },
    Field {
        key: "upColor",
        label: "Up",
        section: "Colors",
        kind: FieldKind::Color {
            get: |s| s.up_color,
            set: |s, c| s.up_color = c,
        },
    },
    Field {
        key: "downColor",
        label: "Down",
        section: "Colors",
        kind: FieldKind::Color {
            get: |s| s.down_color,
            set: |s, c| s.down_color = c,
        },
    },
    Field {
        key: "opacity",
        label: "Background",
        section: "Colors",
        kind: FieldKind::Slider {
            range: Settings::OPACITY,
            step: 0.01,
            unit: "",
            get: |s| s.opacity,
            set: |s, v| s.opacity = v,
        },
    },
    Field {
        key: "chartWidth",
        label: "Width",
        section: "Chart",
        kind: FieldKind::Slider {
            range: Settings::CHART_WIDTH,
            step: 10.0,
            unit: "px",
            get: |s| s.chart_width,
            set: |s, v| s.chart_width = v,
        },
    },
    Field {
        key: "chartHeight",
        label: "Height",
        section: "Chart",
        kind: FieldKind::Slider {
            range: Settings::CHART_HEIGHT,
            step: 10.0,
            unit: "px",
            get: |s| s.chart_height,
            set: |s, v| s.chart_height = v,
        },
    },
    Field {
        key: "refreshSeconds",
        label: "Refresh",
        section: "Behavior",
        kind: FieldKind::Stepper {
            range: Settings::REFRESH_SECONDS,
            format: |n| format!("Refresh every {n} s"),
            get: |s| s.refresh_seconds,
            set: |s, v| s.refresh_seconds = v,
        },
    },
    Field {
        key: "onlyWhenWoWActive",
        label: "Show only while World of Warcraft is frontmost",
        section: "Behavior",
        kind: FieldKind::Toggle {
            get: |s| s.only_when_wow_active,
            set: |s, v| s.only_when_wow_active = v,
        },
    },
    Field {
        key: "locked",
        label: "Lock position",
        section: "Behavior",
        kind: FieldKind::Toggle {
            get: |s| s.locked,
            set: |s, v| s.locked = v,
        },
    },
    Field {
        key: "position",
        label: "Reset position",
        section: "Behavior",
        kind: FieldKind::Action {
            enabled: |s| s.position.is_some(),
            apply: |s| s.position = None,
        },
    },
];
