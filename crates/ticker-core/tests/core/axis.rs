use ticker_core::spaced_labels;

const GAP: f64 = 6.0;

#[test]
fn labels_with_room_between_them_are_all_drawn() {
    assert_eq!(spaced_labels(&[0.0..30.0, 40.0..70.0, 80.0..110.0], GAP), [true, true, true]);
}

#[test]
fn a_label_that_would_overlap_the_one_before_it_is_skipped() {
    assert_eq!(spaced_labels(&[0.0..30.0, 20.0..50.0], GAP), [true, false]);
}

#[test]
fn the_next_label_is_spaced_from_the_last_one_drawn_not_the_one_skipped() {
    assert_eq!(spaced_labels(&[0.0..30.0, 20.0..50.0, 40.0..70.0], GAP), [true, false, true]);
}

#[test]
fn a_label_needs_the_whole_gap_after_the_last_one_drawn() {
    assert_eq!(spaced_labels(&[0.0..30.0, 35.0..60.0], GAP), [true, false]);
    assert_eq!(spaced_labels(&[0.0..30.0, 36.0..60.0], GAP), [true, true]);
}

#[test]
fn a_crowded_row_keeps_every_label_that_fits() {
    let crowded: Vec<_> = (0..17).map(|index| f64::from(index) * 12.0).map(|left| left..left + 30.0).collect();
    let drawn = spaced_labels(&crowded, GAP);
    assert_eq!(drawn.iter().filter(|drawn| **drawn).count(), 6);
    assert!(drawn.iter().step_by(3).all(|drawn| *drawn), "every third label starts 36 past the last drawn: {drawn:?}");
}

#[test]
fn no_labels_draw_nothing() {
    assert_eq!(spaced_labels(&[], GAP), Vec::<bool>::new());
}
