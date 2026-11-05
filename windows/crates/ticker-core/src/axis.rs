use std::ops::Range;

/// Which of a row of axis labels to draw. `extents` are where each label would sit along the axis, in order. A label
/// is drawn only when it starts at least `gap` past the end of the last label drawn, so no two drawn labels touch.
pub fn spaced_labels(extents: &[Range<f64>], gap: f64) -> Vec<bool> {
    let mut end = f64::NEG_INFINITY;
    extents
        .iter()
        .map(|extent| {
            let drawn = extent.start >= end + gap;
            if drawn {
                end = extent.end;
            }
            drawn
        })
        .collect()
}
