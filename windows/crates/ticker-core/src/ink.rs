use crate::settings::Rgba;

/// What the chart draws in the text color, each at its own share of that color's opacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartInk {
    Grid,
    AxisLabel,
    LastPrice,
    Placeholder,
}

impl ChartInk {
    /// The text color with its own alpha scaled, so a translucent text color fades the whole chart with it.
    pub fn color(self, text: Rgba) -> Rgba {
        let share = match self {
            ChartInk::Grid => 0.12,
            ChartInk::AxisLabel => 0.7,
            ChartInk::LastPrice => 0.75,
            ChartInk::Placeholder => 0.6,
        };
        Rgba { alpha: text.alpha * share, ..text }
    }
}
