//! Midnight Party: curled foil streamers and neon satin around a quiet purple stage.

use crate::theme::Palette;
use gpui::{
    Background, Bounds, PathBuilder, Pixels, Point, linear_color_stop, linear_gradient, point, px,
    rgb,
};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy)]
struct Frame(Bounds<Pixels>);

impl Frame {
    fn at(self, x: f32, y: f32) -> Point<Pixels> {
        point(
            self.0.origin.x + self.0.size.width * x,
            self.0.origin.y + self.0.size.height * y,
        )
    }

    fn curve(self, path: &mut PathBuilder, end: (f32, f32), a: (f32, f32), b: (f32, f32)) {
        path.cubic_bezier_to(self.at(end.0, end.1), self.at(a.0, a.1), self.at(b.0, b.1));
    }

    // Decorations keep their proportions on both wide windows and tiny canvases.
    fn local(self, x: f32, y: f32, dx: f32, dy: f32) -> Point<Pixels> {
        let unit = f32::from(self.0.size.width).min(f32::from(self.0.size.height));
        let center = self.at(x, y);
        point(center.x + px(dx * unit), center.y + px(dy * unit))
    }
}

fn gradient(a: u32, b: u32, opacity: f32, angle: f32) -> Background {
    linear_gradient(
        angle,
        linear_color_stop(rgb(a), 0.0).opacity(opacity),
        linear_color_stop(rgb(b), 1.0).opacity(opacity),
    )
}

fn ink(color: u32, opacity: f32) -> Background {
    gpui::rgba((color << 8) | (opacity.clamp(0.0, 1.0) * 255.0) as u32).into()
}

fn seed(index: u32) -> f32 {
    let mut n = index.wrapping_add(0x9e37_79b9);
    n = (n ^ (n >> 16)).wrapping_mul(0x85eb_ca6b);
    n = (n ^ (n >> 13)).wrapping_mul(0xc2b2_ae35);
    ((n ^ (n >> 16)) >> 8) as f32 / 16_777_216.0
}

pub(super) fn compose(
    bounds: Bounds<Pixels>,
    p: Palette,
    phase: f32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    let f = Frame(bounds);
    // Reducing angles in f64 keeps long-running animation stable without a timer here.
    let angle = |speed: f64, offset: f32| {
        ((phase as f64 * speed + offset as f64).rem_euclid(std::f64::consts::TAU)) as f32
    };
    let mut base = PathBuilder::fill();
    base.move_to(f.at(0.0, 0.0));
    base.line_to(f.at(1.0, 0.0));
    base.line_to(f.at(1.0, 1.0));
    base.line_to(f.at(0.0, 1.0));
    base.close();
    paint(base, gradient(p.background, p.surface, 1.0, 155.0));

    // Scalloped pools of violet light stay on the wings, not behind the search field.
    for right in [false, true] {
        let x = |x: f32| if right { 1.0 - x } else { x };
        let sway = angle(0.65, if right { 1.7 } else { 0.0 }).sin() * 0.022;
        let mut wing = PathBuilder::fill();
        wing.move_to(f.at(x(-0.02), 0.03));
        f.curve(
            &mut wing,
            (x(0.16 + sway), 0.46),
            (x(0.20), 0.16),
            (x(0.02), 0.28),
        );
        f.curve(&mut wing, (x(0.19), 0.90), (x(0.27), 0.64), (x(0.03), 0.72));
        wing.line_to(f.at(x(-0.02), 1.03));
        wing.close();
        paint(wing, gradient(p.secondary, p.background, 0.25, 130.0));
    }

    // Each ribbon is a closed band, never a filled landscape. The central upper
    // edge remains below .87, including its animated control points.
    for i in 0..5 {
        let n = i as f32;
        let wave = angle(0.85, n * 0.85).sin();
        let offset = n * 0.023;
        let left = 0.79 + offset + wave * 0.025;
        let right = 0.81 + offset - wave * 0.030;
        let middle = 0.91 + offset + angle(0.72, n).sin() * 0.025;
        let thickness = [0.045, 0.026, 0.018, 0.029, 0.043][i];
        let (a, b) = [
            (p.secondary, p.primary),
            (p.primary, p.accent),
            (0xffdf68, p.accent),
            (p.highlight, p.secondary),
            (p.primary, p.secondary),
        ][i];
        for glow in [true, false] {
            let spread = if glow { 0.010 } else { 0.0 };
            let mut ribbon = PathBuilder::fill();
            ribbon.move_to(f.at(-0.06, left - spread));
            f.curve(
                &mut ribbon,
                (0.49, middle - spread),
                (0.12, left - 0.02),
                (0.22, middle + 0.08),
            );
            f.curve(
                &mut ribbon,
                (1.06, right - spread),
                (0.75, middle - 0.035),
                (0.84, right - 0.05),
            );
            ribbon.line_to(f.at(1.06, right + thickness + spread));
            f.curve(
                &mut ribbon,
                (0.49, middle + thickness + spread),
                (0.84, right - 0.05 + thickness),
                (0.75, middle - 0.035 + thickness),
            );
            f.curve(
                &mut ribbon,
                (-0.06, left + thickness + spread),
                (0.22, middle + 0.08 + thickness),
                (0.12, left - 0.02 + thickness),
            );
            ribbon.close();
            paint(
                ribbon,
                gradient(a, b, if glow { 0.10 } else { 0.85 }, 100.0 + wave * 25.0),
            );
        }
    }

    // Hanging corkscrews are tapered foil, with alternating lit and shaded turns.
    for i in 0..6 {
        let right = i % 2 == 1;
        let x = |x: f32| if right { 1.0 - x } else { x };
        let lane = (i / 2) as f32;
        let cx = 0.035 + lane * 0.061;
        let top = -0.06 + lane * 0.18;
        let sway = angle(1.1, i as f32 * 1.9).sin() * 0.018;
        let color = [p.primary, p.highlight, 0xffd65b][i / 2];
        for turn in 0..3 {
            let y = top + turn as f32 * 0.080;
            let mut curl = PathBuilder::fill();
            curl.move_to(f.at(x(cx + sway), y));
            f.curve(
                &mut curl,
                (x(cx + sway), y + 0.080),
                (x(cx + 0.065 + sway), y + 0.010),
                (x(cx - 0.055 + sway), y + 0.068),
            );
            curl.line_to(f.at(x(cx + 0.008 + sway), y + 0.083));
            f.curve(
                &mut curl,
                (x(cx + 0.011 + sway), y + 0.004),
                (x(cx - 0.035 + sway), y + 0.060),
                (x(cx + 0.080 + sway), y + 0.014),
            );
            curl.close();
            paint(
                curl,
                gradient(
                    color,
                    p.secondary,
                    if turn % 2 == 0 { 0.85 } else { 0.48 },
                    70.0,
                ),
            );
        }
    }

    // Gold four-point stars form a loose corner constellation, not a starfield.
    for i in 0..14 {
        let right = i % 2 == 1;
        let s = seed(i + 90);
        let x = 0.035 + seed(i + 31) * 0.17;
        let x = if right { 1.0 - x } else { x };
        let twinkle = angle(1.8, s * TAU);
        let y = 0.035 + seed(i + 70) * 0.25 + twinkle.sin() * 0.005;
        let radius = (0.006 + s * 0.010) * (1.0 + twinkle.cos() * 0.20);
        let mut star = PathBuilder::fill();
        for vertex in 0..8 {
            let a = vertex as f32 * PI / 4.0 - PI / 2.0;
            let r = radius * if vertex % 2 == 0 { 1.0 } else { 0.25 };
            let pt = f.local(x, y, a.cos() * r, a.sin() * r);
            if vertex == 0 {
                star.move_to(pt);
            } else {
                star.line_to(pt);
            }
        }
        star.close();
        paint(
            star,
            gradient(
                0xffed9c,
                0xffba26,
                0.66 + 0.28 * (twinkle.sin() * 0.5 + 0.5),
                160.0,
            ),
        );
    }

    // Confetti follows continuous elliptical currents: no respawn popping.
    // Side lanes cannot enter x=.25..=.75; footer pieces stay below y=.89.
    for i in 0..32 {
        let s = seed(i + 180);
        let orbit = angle(0.8 + s as f64 * 0.5, s * TAU);
        let (x, y) = if i < 24 {
            let x = 0.035 + seed(i + 260) * 0.15 + orbit.sin() * 0.015;
            (
                if i % 2 == 0 { x } else { 1.0 - x },
                0.15 + seed(i + 300) * 0.67 + orbit.cos() * 0.028,
            )
        } else {
            (
                0.14 + seed(i + 280) * 0.72 + orbit.sin() * 0.025,
                0.935 + orbit.cos() * 0.024,
            )
        };
        let rotation = angle(1.6 + s as f64, s * TAU);
        let tumble = 0.28 + 0.72 * angle(2.1, s * 11.0).cos().abs();
        let width = (0.004 + s * 0.003) * tumble;
        let height = 0.008 + seed(i + 400) * 0.006;
        let mut piece = PathBuilder::fill();
        for (j, (dx, dy)) in [
            (-width, -height),
            (width, -height),
            (width, height),
            (-width, height),
        ]
        .into_iter()
        .enumerate()
        {
            let pt = f.local(
                x,
                y,
                dx * rotation.cos() - dy * rotation.sin(),
                dx * rotation.sin() + dy * rotation.cos(),
            );
            if j == 0 {
                piece.move_to(pt);
            } else {
                piece.line_to(pt);
            }
        }
        piece.close();
        paint(
            piece,
            ink(
                [p.primary, 0xffdc61, p.highlight, p.secondary, p.accent][i as usize % 5],
                0.70 + 0.25 * tumble,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use gpui::size;

    fn paths(width: f32, height: f32, phase: f32) -> Vec<String> {
        let mut result = Vec::new();
        compose(
            Bounds::new(point(px(7.0), px(11.0)), size(px(width), px(height))),
            Theme::MidnightParty.palette(),
            phase,
            &mut |builder, _| {
                let path = builder.build().expect("party path must tessellate");
                let geometry = format!("{path:?}");
                assert!(!geometry.contains("NaN") && !geometry.contains("inf"));
                result.push(geometry);
            },
        );
        result
    }

    #[test]
    fn all_party_paths_tessellate_at_responsive_sizes_and_phases() {
        for (width, height) in [(600.0, 500.0), (1440.0, 900.0), (1.0, 1.0)] {
            for phase in [0.0, 1.0, 13.0] {
                let paths = paths(width, height, phase);
                assert_eq!(paths.len(), 77);
                assert!(paths.len() <= 100);
            }
        }
    }

    #[test]
    fn geometry_moves_within_one_second_and_is_deterministic() {
        let still = paths(600.0, 500.0, 0.0);
        let moving = paths(600.0, 500.0, 1.0);
        assert_eq!(still, paths(600.0, 500.0, 0.0));
        assert_eq!(still[0], moving[0], "the quiet base stays still");
        let changed = still.iter().zip(&moving).filter(|(a, b)| a != b).count();
        assert!(
            changed > 65,
            "ribbons, streamers, stars and confetti must move"
        );
    }
}
