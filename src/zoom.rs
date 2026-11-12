//! Page zoom levels, stepped like other browsers.

const LEVELS: [f64; 17] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

/// The next level after `current`: `1` zooms in, `-1` out, `0` resets to 100 %.
pub fn step(current: f64, direction: i8) -> f64 {
    let current = if current.is_finite() { current } else { 1.0 };
    match direction {
        0 => 1.0,
        1.. => LEVELS
            .into_iter()
            .find(|level| *level > current + 0.001)
            .unwrap_or(LEVELS[16]),
        _ => LEVELS
            .into_iter()
            .rev()
            .find(|level| *level < current - 0.001)
            .unwrap_or(LEVELS[0]),
    }
}

/// The level as a percentage, or `None` at 100 %.
pub fn label(zoom: f64) -> Option<String> {
    ((zoom - 1.0).abs() > 0.001).then(|| format!("{:.0}%", zoom * 100.))
}

#[cfg(test)]
mod tests {
    use super::{label, step};

    #[test]
    fn steps_through_levels_and_stops_at_the_ends() {
        assert_eq!(step(1.0, 1), 1.1);
        assert_eq!(step(1.0, -1), 0.9);
        assert_eq!(step(1.25, 0), 1.0);
        assert_eq!(step(5.0, 1), 5.0);
        assert_eq!(step(0.25, -1), 0.25);
        // A level set elsewhere snaps to the next one in that direction.
        assert_eq!(step(1.05, 1), 1.1);
        assert_eq!(step(1.05, -1), 1.0);
        assert_eq!(step(f64::NAN, 1), 1.1);
    }

    #[test]
    fn labels_only_zoomed_pages() {
        assert_eq!(label(1.0), None);
        assert_eq!(label(1.25).as_deref(), Some("125%"));
        assert_eq!(label(0.67).as_deref(), Some("67%"));
    }
}
