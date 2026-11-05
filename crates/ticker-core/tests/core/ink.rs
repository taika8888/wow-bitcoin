use ticker_core::{ChartInk, Rgba};

/// The macOS chart's shares of the text color's opacity.
const SHARES: [(ChartInk, f64); 4] =
    [(ChartInk::Grid, 0.12), (ChartInk::AxisLabel, 0.7), (ChartInk::LastPrice, 0.75), (ChartInk::Placeholder, 0.6)];

#[test]
fn an_opaque_text_color_draws_each_ink_at_its_share() {
    let white = Rgba::new(1.0, 1.0, 1.0, 1.0);
    for (ink, share) in SHARES {
        assert_eq!(ink.color(white), Rgba::new(1.0, 1.0, 1.0, share), "{ink:?}");
    }
}

#[test]
fn a_translucent_text_color_fades_every_ink_with_it() {
    let amber = Rgba::new(1.0, 0.75, 0.25, 0.4);
    for (ink, share) in SHARES {
        let drawn = ink.color(amber);
        assert_eq!((drawn.red, drawn.green, drawn.blue), (1.0, 0.75, 0.25), "{ink:?} keeps the hue");
        assert!((drawn.alpha - 0.4 * share).abs() < 1e-12, "{ink:?} is drawn at alpha {}", drawn.alpha);
    }
}

#[test]
fn an_invisible_text_color_hides_the_chart_ink() {
    for (ink, _) in SHARES {
        assert_eq!(ink.color(Rgba::new(1.0, 1.0, 1.0, 0.0)).alpha, 0.0, "{ink:?}");
    }
}
