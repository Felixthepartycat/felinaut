//! Procedural edge artwork. The owner supplies elapsed seconds and schedules redraws.
//!
//! Place `backdrop(palette, phase).absolute().inset_0()` behind content in a
//! positioned, clipping parent. Pass `0.0` for a composed, motion-free frame.

use crate::theme::Palette;
use gpui::{
    Background, Bounds, Canvas, PathBuilder, Pixels, Point, Styled, canvas, linear_color_stop,
    linear_gradient, point, px, rgb,
};

/// A full-size, non-interactive background; does not request animation frames.
///
/// Colors follow the palette, with lighter ink on dark themes and reduced
/// saturation-by-opacity on light themes. Painting respects the inherited mask.
pub fn backdrop(palette: Palette, phase: f32) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            compose(bounds, palette, phase, |builder, ink| {
                if let Ok(path) = builder.build() {
                    window.paint_path(path, ink);
                }
            });
        },
    )
    .size_full()
}

#[derive(Clone, Copy)]
struct Frame {
    bounds: Bounds<Pixels>,
}

impl Frame {
    fn at(self, x: f32, y: f32) -> Point<Pixels> {
        point(
            self.bounds.origin.x + self.bounds.size.width * x,
            self.bounds.origin.y + self.bounds.size.height * y,
        )
    }

    fn curve(self, path: &mut PathBuilder, a: (f32, f32), b: (f32, f32), end: (f32, f32)) {
        // GPUI takes the endpoint first, unlike the usual SVG ordering.
        path.cubic_bezier_to(self.at(end.0, end.1), self.at(a.0, a.1), self.at(b.0, b.1));
    }
}

fn gradient(a: u32, alpha_a: f32, b: u32, alpha_b: f32, angle: f32) -> Background {
    linear_gradient(
        angle,
        linear_color_stop(rgb(a), 0.0).opacity(alpha_a),
        linear_color_stop(rgb(b), 1.0).opacity(alpha_b),
    )
}

fn seconds(phase: f32) -> f64 {
    if phase.is_finite() { phase as f64 } else { 0.0 }
}

// A stateless integer hash: decoration never shuffles when a canvas is rebuilt.
fn seed(index: u32) -> f32 {
    let mut n = index.wrapping_add(0x9e37_79b9);
    n = (n ^ (n >> 16)).wrapping_mul(0x85eb_ca6b);
    n = (n ^ (n >> 13)).wrapping_mul(0xc2b2_ae35);
    ((n ^ (n >> 16)) >> 8) as f32 / 16_777_216.0
}

fn compose(
    bounds: Bounds<Pixels>,
    p: Palette,
    phase: f32,
    mut paint: impl FnMut(PathBuilder, Background),
) {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return;
    }
    let f = Frame { bounds };
    let t = seconds(phase);
    let drift = (t * 0.16).sin() as f32 * 0.008;
    let breathe = (t * 0.11).sin() as f32 * 0.006;
    let strength = if p.dark { 1.0 } else { 0.64 };

    let mut base = PathBuilder::fill();
    base.move_to(f.at(0.0, 0.0));
    base.line_to(f.at(1.0, 0.0));
    base.line_to(f.at(1.0, 1.0));
    base.line_to(f.at(0.0, 1.0));
    base.close();
    paint(base, gradient(p.background, 1.0, p.surface, 1.0, 155.0));

    // Low-contrast cloud banks: overlapping scallops, never a central haze.
    for right in [false, true] {
        for layer in 0..2 {
            let lift = layer as f32 * 0.035 + breathe;
            let x = |v: f32| if right { 1.0 - v } else { v };
            let mut cloud = PathBuilder::fill();
            cloud.move_to(f.at(x(-0.04), 0.77 + lift));
            f.curve(
                &mut cloud,
                (x(0.00), 0.69 + lift),
                (x(0.08), 0.70 + lift),
                (x(0.10), 0.78 + lift),
            );
            f.curve(
                &mut cloud,
                (x(0.14), 0.73 + lift),
                (x(0.20), 0.78 + lift),
                (x(0.20), 0.84 + lift),
            );
            f.curve(
                &mut cloud,
                (x(0.27), 0.80 + lift),
                (x(0.32), 0.88 + lift),
                (x(0.32), 0.95 + lift),
            );
            cloud.line_to(f.at(x(0.36), 1.06));
            cloud.line_to(f.at(x(-0.04), 1.06));
            cloud.close();
            paint(
                cloud,
                gradient(
                    if layer == 0 { p.secondary } else { p.elevated },
                    (if layer == 0 { 0.13 } else { 0.32 }) * strength,
                    p.surface,
                    0.02,
                    180.0,
                ),
            );
        }
    }

    // Nested asymmetric silk bands: the center crest stays below 83% height.
    // Each closes underneath its own crest, rather than filling the whole footer.
    let ribbons = [
        (0.000, 0.105, p.secondary, p.primary, 0.42),
        (0.040, 0.062, p.primary, p.accent, 0.82),
        (0.093, 0.022, p.accent, p.primary, 0.90),
        (0.126, 0.046, p.highlight, p.secondary, 0.77),
        (0.173, 0.100, p.secondary, p.surface, 0.85),
    ];
    for (index, (offset, thickness, from, to, opacity)) in ribbons.into_iter().enumerate() {
        let y = offset + breathe;
        let sway = drift * (1.0 - index as f32 * 0.12);
        let mut ribbon = PathBuilder::fill();
        ribbon.move_to(f.at(-0.06, 0.78 + y));
        f.curve(
            &mut ribbon,
            (0.13, 0.77 + y + sway),
            (0.20, 1.00 + y),
            (0.46, 0.90 + y),
        );
        f.curve(
            &mut ribbon,
            (0.67, 0.81 + y - sway),
            (0.79, 0.76 + y),
            (1.06, 0.87 + y),
        );
        ribbon.line_to(f.at(1.06, 0.87 + y + thickness));
        f.curve(
            &mut ribbon,
            (0.79, 0.78 + y + thickness),
            (0.67, 0.84 + y + thickness - sway),
            (0.46, 0.93 + y + thickness),
        );
        f.curve(
            &mut ribbon,
            (0.20, 1.03 + y + thickness),
            (0.13, 0.79 + y + thickness + sway),
            (-0.06, 0.80 + y + thickness),
        );
        ribbon.close();
        paint(
            ribbon,
            gradient(
                from,
                opacity * strength,
                to,
                opacity * strength * 0.65,
                105.0,
            ),
        );

        if index == 1 || index == 3 {
            let mut seam = PathBuilder::stroke(px(0.9));
            seam.move_to(f.at(-0.06, 0.78 + y));
            f.curve(
                &mut seam,
                (0.13, 0.77 + y + sway),
                (0.20, 1.00 + y),
                (0.46, 0.90 + y),
            );
            f.curve(
                &mut seam,
                (0.67, 0.81 + y - sway),
                (0.79, 0.76 + y),
                (1.06, 0.87 + y),
            );
            paint(seam, gradient(p.text, 0.23 * strength, to, 0.03, 90.0));
        }
    }

    // Cropped orbital yarn trails. Broad, faint under-strokes suggest a glow
    // without blur passes; the open curves stay in the outer corners.
    for right in [false, true] {
        let x = |v: f32| if right { 1.0 - v } else { v };
        let shift = if right { -drift } else { drift };
        let color = if right { p.primary } else { p.highlight };
        for (width, alpha) in [(5.0, 0.025), (1.1, 0.23)] {
            let mut orbit = PathBuilder::stroke(px(width));
            orbit.move_to(f.at(x(-0.025), 0.43));
            f.curve(
                &mut orbit,
                (x(0.12 + shift), 0.24),
                (x(0.18 + shift), 0.04),
                (x(0.07), 0.065),
            );
            f.curve(
                &mut orbit,
                (x(0.01), 0.08),
                (x(0.08), 0.19),
                (x(0.25), -0.025),
            );
            paint(
                orbit,
                gradient(color, alpha * strength, p.secondary, 0.01, 20.0),
            );
        }
        let mut satellite = PathBuilder::stroke(px(0.8));
        satellite.move_to(f.at(x(-0.02), 0.56));
        f.curve(
            &mut satellite,
            (x(0.045), 0.46 + shift),
            (x(0.08), 0.39 + shift),
            (x(0.075), 0.33),
        );
        paint(
            satellite,
            gradient(p.accent, 0.19 * strength, color, 0.0, 0.0),
        );
    }

    let scale = (w.min(h) / 720.0).clamp(0.45, 1.35);
    for index in 0..12 {
        let (x, y) = star_position(index);
        let angle = seed(index + 80) as f64 * std::f64::consts::TAU;
        let bob = (t * 0.19 + angle).sin() as f32 * 0.003;
        let center = f.at(x, y + bob);
        let radius = (2.8 + seed(index + 30) * 3.8) * scale;
        let twinkle = 0.80 + (t * 0.31 + angle).sin() as f32 * 0.12;
        let color = match index % 4 {
            0 => p.highlight,
            1 => p.accent,
            2 => p.primary,
            _ => p.text,
        };
        paint(
            sparkle(center, radius),
            gradient(
                color,
                0.63 * twinkle * strength,
                color,
                0.22 * twinkle * strength,
                160.0,
            ),
        );
    }
}

fn star_position(index: u32) -> (f32, f32) {
    // Four edge clusters; deliberately no stars in the reading area.
    let a = seed(index + 1);
    let b = seed(index + 101);
    match index % 4 {
        0 => (0.025 + a * 0.10, 0.14 + b * 0.55),
        1 => (0.875 + a * 0.10, 0.12 + b * 0.55),
        2 => (0.16 + a * 0.68, 0.025 + b * 0.065),
        _ => (0.08 + a * 0.84, 0.925 + b * 0.045),
    }
}

fn sparkle(center: Point<Pixels>, radius: f32) -> PathBuilder {
    let at = |x, y| point(center.x + px(x * radius), center.y + px(y * radius));
    let mut path = PathBuilder::fill();
    path.move_to(at(0.0, -1.0));
    path.curve_to(at(0.78, 0.0), at(0.12, -0.12));
    path.curve_to(at(0.0, 1.0), at(0.12, 0.12));
    path.curve_to(at(-0.78, 0.0), at(-0.12, 0.12));
    path.curve_to(at(0.0, -1.0), at(-0.12, -0.12));
    path.close();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use gpui::size;

    #[test]
    fn seeds_and_stars_are_stable_and_leave_the_center_empty() {
        assert_eq!(seed(42).to_bits(), seed(42).to_bits());
        assert_ne!(seed(41), seed(42));
        for i in 0..12 {
            let (x, y) = star_position(i);
            assert!((0.0..1.0).contains(&x) && (0.0..1.0).contains(&y));
            assert!(!(0.13..=0.87).contains(&x) || !(0.10..=0.92).contains(&y));
            assert_eq!(star_position(i), star_position(i));
        }
    }

    #[test]
    fn all_paths_tessellate_across_themes_sizes_and_phases() {
        for theme in Theme::ALL {
            for (w, h) in [(1440.0, 900.0), (320.0, 800.0), (2400.0, 320.0), (1.0, 1.0)] {
                for phase in [0.0, 1.0, 37.0, 600.0, f32::MAX, f32::NAN, f32::INFINITY] {
                    let mut count = 0;
                    compose(
                        Bounds::new(point(px(17.0), px(31.0)), size(px(w), px(h))),
                        theme.palette(),
                        phase,
                        |path, _| {
                            path.build().expect("art geometry must tessellate");
                            count += 1;
                        },
                    );
                    assert_eq!(count, 30);
                }
            }
        }
    }

    #[test]
    fn invalid_phase_is_static_and_empty_bounds_do_not_paint() {
        assert_eq!(seconds(f32::NAN), seconds(0.0));
        assert_eq!(seconds(f32::NEG_INFINITY), seconds(0.0));
        for (w, h) in [(0.0, 20.0), (20.0, 0.0), (-1.0, 20.0), (f32::NAN, 20.0)] {
            compose(
                Bounds::new(point(px(0.0), px(0.0)), size(px(w), px(h))),
                Theme::MidnightParty.palette(),
                0.0,
                |_, _| panic!("empty canvas should not paint"),
            );
        }
    }
}
