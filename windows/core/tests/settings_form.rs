use serde_json::{Map, Value};
use std::collections::BTreeSet;
use ticker_core::{FORM, FieldKind, Rgba, ScreenPoint, Settings};

fn keys(settings: &Settings) -> Map<String, Value> {
    match serde_json::from_str(&settings.to_json()).unwrap() {
        Value::Object(map) => map,
        other => panic!("settings serialized to {other}"),
    }
}

#[test]
fn form_covers_every_key() {
    let full = Settings {
        position: Some(ScreenPoint { x: 1.0, y: 2.0 }),
        ..Settings::default()
    };
    let written: BTreeSet<String> = keys(&full)
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| k != "timeframe")
        .collect();
    let rows: Vec<String> = FORM.iter().map(|f| f.key.to_string()).collect();
    let unique: BTreeSet<String> = rows.iter().cloned().collect();
    assert_eq!(rows.len(), unique.len(), "a key appears twice in FORM");
    assert_eq!(written, unique);
}

#[test]
fn keys_match_macos() {
    let mac = [
        "chartHeight",
        "chartWidth",
        "decimals",
        "downColor",
        "fontSize",
        "locked",
        "onlyWhenWoWActive",
        "opacity",
        "refreshSeconds",
        "showChange",
        "showLabel",
        "textColor",
        "timeframe",
        "upColor",
    ];
    let written: Vec<String> = keys(&Settings::default())
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(written, mac, "position is omitted when nil, as on macOS");
}

#[test]
fn each_row_edits_only_its_key() {
    let start = Settings {
        position: Some(ScreenPoint { x: 5.0, y: 6.0 }),
        ..Settings::default()
    };
    for field in FORM {
        let mut edited = start.clone();
        match &field.kind {
            FieldKind::Slider {
                range, get, set, ..
            } => {
                let target = if get(&start) == *range.end() {
                    *range.start()
                } else {
                    *range.end()
                };
                set(&mut edited, target);
                assert_eq!(get(&edited), target, "{}", field.key);
            }
            FieldKind::Stepper {
                range, get, set, ..
            } => {
                set(&mut edited, *range.end());
                assert_eq!(get(&edited), *range.end(), "{}", field.key);
            }
            FieldKind::Choice { options, get, set } => {
                set(&mut edited, options.len() - 1);
                assert_eq!(get(&edited), options.len() - 1, "{}", field.key);
            }
            FieldKind::Toggle { get, set } => set(&mut edited, !get(&start)),
            FieldKind::Color { get, set } => {
                let color = Rgba {
                    red: 0.1,
                    green: 0.2,
                    blue: 0.3,
                    alpha: 0.4,
                };
                set(&mut edited, color);
                assert_eq!(get(&edited), color, "{}: alpha is editable", field.key);
            }
            FieldKind::Action { enabled, apply } => {
                assert!(enabled(&start), "{} is enabled when it has work", field.key);
                apply(&mut edited);
                assert!(!enabled(&edited), "{} is disabled once applied", field.key);
            }
        }
        let (before, after) = (keys(&start), keys(&edited));
        let changed: BTreeSet<&String> = before
            .keys()
            .chain(after.keys())
            .filter(|k| before.get(*k) != after.get(*k))
            .collect();
        assert_eq!(
            changed,
            BTreeSet::from([&field.key.to_string()]),
            "row {} changed other keys",
            field.key
        );
        assert_eq!(
            Settings::decode(Some(&edited.to_json())),
            edited,
            "{} writes a value inside its decode range",
            field.key
        );
    }
}

#[test]
fn every_slider_reads_out_its_value_in_its_unit() {
    let shown: Vec<(&str, String)> = FORM
        .iter()
        .filter_map(|field| match &field.kind {
            FieldKind::Slider { format, get, .. } => {
                Some((field.key, format(get(&Settings::default()))))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("fontSize", "15 pt".to_string()),
            ("opacity", "55%".to_string()),
            ("chartWidth", "360 px".to_string()),
            ("chartHeight", "180 px".to_string()),
        ],
        "each slider's label shows the default in the setting's unit"
    );
}
