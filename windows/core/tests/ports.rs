//! Every Swift test in Tests/TickerCoreTests has a Rust test of the same name in snake case, so a test added on
//! macOS fails here until it is ported or listed in `NOT_PORTED` with its reason.

const SWIFT: &[(&str, &str)] = &[
    (
        "ChartTests.swift",
        include_str!("../../../Tests/TickerCoreTests/ChartTests.swift"),
    ),
    (
        "DisplayAndSettingsTests.swift",
        include_str!("../../../Tests/TickerCoreTests/DisplayAndSettingsTests.swift"),
    ),
    (
        "KrakenTests.swift",
        include_str!("../../../Tests/TickerCoreTests/KrakenTests.swift"),
    ),
    (
        "OverlayModelCheckTests.swift",
        include_str!("../../../Tests/TickerCoreTests/OverlayModelCheckTests.swift"),
    ),
    (
        "PlacementTests.swift",
        include_str!("../../../Tests/TickerCoreTests/PlacementTests.swift"),
    ),
];

const RUST: &[&str] = &[
    include_str!("app_driver.rs"),
    include_str!("chart.rs"),
    include_str!("display_settings.rs"),
    include_str!("kraken.rs"),
    include_str!("overlay_model_check.rs"),
    include_str!("placement.rs"),
    include_str!("settings_form.rs"),
];

/// Swift test name, and why it has no Rust counterpart.
const NOT_PORTED: &[(&str, &str)] = &[];

fn swift_tests(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut after_attribute = false;
    for line in source.lines().map(str::trim) {
        if line.starts_with("@Test") {
            after_attribute = true;
        }
        if after_attribute && let Some((_, rest)) = line.split_once("func ") {
            names.push(rest.chars().take_while(|c| c.is_alphanumeric()).collect());
            after_attribute = false;
        }
    }
    names
}

/// camelCase to snake_case, keeping acronyms whole: "isAnAPIError" becomes "is_an_api_error".
fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let after_lower = i > 0 && chars[i - 1].is_lowercase();
        let acronym_end = i > 0
            && chars[i - 1].is_uppercase()
            && chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        if c.is_uppercase() && (after_lower || acronym_end) {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

#[test]
fn snake_case_keeps_acronyms_whole() {
    assert_eq!(
        snake("krakenErrorArrayIsAnAPIError"),
        "kraken_error_array_is_an_api_error"
    );
    assert_eq!(snake("parsesARow"), "parses_a_row");
    assert_eq!(snake("price"), "price");
}

#[test]
fn every_swift_test_has_a_rust_counterpart() {
    let rust = RUST.concat();
    let mut missing = Vec::new();
    let mut total = 0;
    for (file, source) in SWIFT {
        let names = swift_tests(source);
        assert!(!names.is_empty(), "found no tests in {file}");
        for name in &names {
            total += 1;
            let wanted = names.iter().filter(|n| *n == name).count();
            let found = rust.matches(&format!("fn {}(", snake(name))).count();
            let excused = NOT_PORTED.iter().any(|(n, _)| n == name);
            if found < wanted && !excused {
                missing.push(format!("{file}: {name} -> fn {}", snake(name)));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Swift tests with no Rust port:\n{}",
        missing.join("\n")
    );
    let attributes: usize = SWIFT.iter().map(|(_, s)| s.matches("@Test").count()).sum();
    assert_eq!(
        total, attributes,
        "every @Test attribute was matched to its function"
    );
}

#[test]
fn every_exception_names_a_real_swift_test() {
    let all: Vec<String> = SWIFT.iter().flat_map(|(_, s)| swift_tests(s)).collect();
    for (name, _) in NOT_PORTED {
        assert!(all.iter().any(|n| n == name), "{name} is not a Swift test");
    }
}
