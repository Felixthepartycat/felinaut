//! Theme-specific procedural artwork. Owners advance seconds only while active.

#[path = "art/alpine.rs"]
mod alpine;
#[path = "art/forest.rs"]
mod forest;
#[path = "art/midnight.rs"]
mod midnight;
#[path = "art/sunset.rs"]
mod sunset;
#[path = "art/wool.rs"]
mod wool;

use crate::theme::{Palette, Theme};
use gpui::{Background, Bounds, Canvas, PathBuilder, Pixels, Styled, canvas};

/// Responsive native vector artwork, with a complete static frame at phase zero.
pub fn backdrop(palette: Palette, phase: f32) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            compose(bounds, palette, phase, &mut |builder, ink| {
                if let Ok(path) = builder.build() {
                    window.paint_path(path, ink);
                }
            });
        },
    )
    .size_full()
}

fn compose(
    bounds: Bounds<Pixels>,
    p: Palette,
    phase: f32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. {
        return;
    }
    let phase = if phase.is_finite() {
        phase.rem_euclid(3600.)
    } else {
        0.
    };
    match p.theme {
        Theme::MidnightParty => midnight::compose(bounds, p, phase, paint),
        Theme::ConfettiSunset => sunset::compose(bounds, p, phase, paint),
        Theme::NeonWool => wool::compose(bounds, p, phase, paint),
        Theme::AlpinePop => alpine::compose(bounds, p, phase, paint),
        Theme::ForestCarnival => forest::compose(bounds, p, phase, paint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn every_theme_has_distinct_animated_geometry() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1200.), px(800.)));
        let mut signatures = Vec::new();
        for theme in Theme::ALL {
            let frame = |phase| {
                let mut signature = String::new();
                compose(bounds, theme.palette(), phase, &mut |builder, _| {
                    let path = builder.build().expect("valid theme geometry");
                    signature.push_str(&format!("{path:?}"));
                });
                signature
            };
            let still = frame(0.);
            assert!(!still.is_empty());
            assert_ne!(
                still,
                frame(1.),
                "{} must move within one second",
                theme.name()
            );
            assert_eq!(still, frame(f32::NAN));
            assert!(
                !signatures.contains(&still),
                "themes must have distinct shapes"
            );
            signatures.push(still);
        }
    }

    #[test]
    fn empty_bounds_do_not_paint() {
        compose(
            Bounds::default(),
            Theme::default().palette(),
            0.,
            &mut |_, _| panic!("empty canvas"),
        );
    }
}
