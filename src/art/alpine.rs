//! Low-poly ice ranges, with motion confined to snow and light at the edges.
use crate::theme::Palette;
use gpui::{
    Background, Bounds, PathBuilder, Pixels, Point, linear_color_stop, linear_gradient, point, rgb,
};

#[derive(Clone, Copy)]
struct Frame(Bounds<Pixels>);

impl Frame {
    fn at(self, x: f32, y: f32) -> Point<Pixels> {
        point(
            self.0.origin.x + self.0.size.width * x,
            self.0.origin.y + self.0.size.height * y,
        )
    }

    fn polygon(self, vertices: &[(f32, f32)]) -> PathBuilder {
        let mut path = PathBuilder::fill();
        path.move_to(self.at(vertices[0].0, vertices[0].1));
        for &(x, y) in &vertices[1..] {
            path.line_to(self.at(x, y));
        }
        path.close();
        path
    }
}

fn ice(a: u32, b: u32, opacity: f32, angle: f32) -> Background {
    linear_gradient(
        angle,
        linear_color_stop(rgb(a), 0.0).opacity(opacity),
        linear_color_stop(rgb(b), 1.0).opacity(opacity),
    )
}

fn seed(i: u32) -> f32 {
    let n = i.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let n = ((n >> ((n >> 28) + 4)) ^ n).wrapping_mul(277_803_737);
    ((n ^ (n >> 22)) >> 8) as f32 / 16_777_216.0
}

// Never cross the central reading area, even while wrapping vertically.
fn snow_position(i: u32, t: f32) -> (f32, f32) {
    let side = 0.022 + seed(i + 40) * 0.155;
    let sway = (t * 0.9 + i as f32 * 2.4).sin() * 0.012;
    let x = side + sway;
    (
        if i.is_multiple_of(2) { x } else { 1.0 - x },
        (seed(i + 90) + t * (0.035 + seed(i + 130) * 0.025)).rem_euclid(1.0),
    )
}

fn glint_position(right: bool, t: f32) -> (f32, f32) {
    let u = (t * 0.24 + if right { 0.52 } else { 0.08 }).rem_euclid(1.0);
    let x = 0.105 + u * 0.151;
    (if right { 1.0 - x } else { x }, 0.565 + u * 0.315)
}

fn crystal(f: Frame, center: (f32, f32), radius: f32) -> PathBuilder {
    // Pixel-sized crystals stay delicate on wide and tiny canvases alike.
    let dx = radius / f32::from(f.0.size.width);
    let dy = radius / f32::from(f.0.size.height);
    let (x, y) = center;
    f.polygon(&[
        (x, y - dy),
        (x + dx * 0.23, y - dy * 0.23),
        (x + dx, y),
        (x + dx * 0.23, y + dy * 0.23),
        (x, y + dy),
        (x - dx * 0.23, y + dy * 0.23),
        (x - dx, y),
        (x - dx * 0.23, y - dy * 0.23),
    ])
}

pub(super) fn compose(
    bounds: Bounds<Pixels>,
    p: Palette,
    phase: f32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    let f = Frame(bounds);
    let t = phase;
    paint(
        f.polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ice(p.background, p.elevated, 1.0, 180.0),
    );

    // Translucent aurora curtains hug the sides, never washing over the mascot.
    for right in [false, true] {
        let x = |v: f32| if right { 1.0 - v } else { v };
        let offset = if right { 1.7 } else { 0.0 };
        let sway = (t * 0.7 + offset).sin() * 0.018;
        for layer in 0..3 {
            let inset = layer as f32 * 0.021;
            let mut glow = PathBuilder::fill();
            glow.move_to(f.at(x(-0.02), 0.16));
            glow.cubic_bezier_to(
                f.at(x(0.10 + inset + sway), 0.39),
                f.at(x(0.16 + sway), 0.17),
                f.at(x(0.03 + inset), 0.28),
            );
            glow.cubic_bezier_to(
                f.at(x(0.055), 0.75),
                f.at(x(0.19 + sway), 0.49),
                f.at(x(0.11), 0.64),
            );
            glow.line_to(f.at(x(-0.02), 0.81));
            glow.close();
            let alpha = (0.065 + (t * 0.85 + offset).sin() * 0.022) * (1.0 - layer as f32 * 0.18);
            paint(
                glow,
                ice(
                    if layer == 1 { p.secondary } else { p.primary },
                    p.elevated,
                    alpha,
                    125.0,
                ),
            );
        }
    }

    // A distant serrated range keeps every central summit below the search area.
    paint(
        f.polygon(&[
            (0.0, 0.72),
            (0.07, 0.67),
            (0.15, 0.76),
            (0.22, 0.78),
            (0.29, 0.87),
            (0.36, 0.84),
            (0.44, 0.89),
            (0.53, 0.855),
            (0.60, 0.89),
            (0.69, 0.82),
            (0.76, 0.86),
            (0.85, 0.72),
            (0.94, 0.66),
            (1.0, 0.73),
            (1.0, 1.0),
            (0.0, 1.0),
        ]),
        ice(0xd5e3ff, p.secondary, 0.24, 160.0),
    );
    paint(
        f.polygon(&[
            (0.0, 0.84),
            (0.17, 0.79),
            (0.30, 0.91),
            (0.43, 0.87),
            (0.56, 0.93),
            (0.72, 0.86),
            (0.89, 0.78),
            (1.0, 0.85),
            (1.0, 1.0),
            (0.0, 1.0),
        ]),
        ice(0xdafaff, p.primary, 0.30, 175.0),
    );

    // Mirrored massing, asymmetric color and secondary peaks: solid geology,
    // not moving ribbons. Each face shares exact ridge vertices with its cap.
    for right in [false, true] {
        let x = |v: f32| if right { 1.0 - v } else { v };
        let face = |vertices: &[(f32, f32)]| {
            let vertices: Vec<_> = vertices.iter().map(|&(a, b)| (x(a), b)).collect();
            f.polygon(&vertices)
        };
        paint(
            face(&[
                (-0.03, 0.80),
                (0.035, 0.65),
                (0.105, 0.565),
                (0.256, 0.88),
                (0.36, 1.02),
                (-0.03, 1.02),
            ]),
            ice(0xe4fcff, 0x91cce8, 1.0, 150.0),
        );
        paint(
            face(&[
                (0.105, 0.565),
                (0.256, 0.88),
                (0.36, 1.02),
                (0.145, 0.94),
                (0.158, 0.78),
            ]),
            ice(0xa5dcf5, p.primary, 0.68, 115.0),
        );
        paint(
            face(&[
                (-0.03, 0.80),
                (0.105, 0.565),
                (0.078, 0.80),
                (0.145, 0.94),
                (-0.03, 1.02),
            ]),
            ice(0xffffff, 0xc7f5fa, 1.0, 160.0),
        );
        paint(
            face(&[(0.078, 0.80), (0.105, 0.565), (0.158, 0.78), (0.145, 0.94)]),
            ice(0xd8eeff, p.secondary, 0.30, 100.0),
        );
        // Jagged snowline, with a long white cornice running down the ridge.
        paint(
            face(&[
                (0.035, 0.65),
                (0.105, 0.565),
                (0.168, 0.697),
                (0.132, 0.67),
                (0.12, 0.693),
                (0.10, 0.64),
                (0.078, 0.69),
                (0.068, 0.665),
            ]),
            ice(0xffffff, 0xe9faff, 1.0, 180.0),
        );
        paint(
            face(&[(0.105, 0.565), (0.256, 0.88), (0.236, 0.85), (0.121, 0.63)]),
            ice(0xffffff, 0xd2faff, 0.85, 150.0),
        );
        // A lower, cropped foreground spire and its triangular glacier shards.
        paint(
            face(&[(-0.04, 0.91), (0.024, 0.742), (0.19, 1.02), (-0.04, 1.02)]),
            ice(0xc9f7fc, p.primary, 0.82, 130.0),
        );
        paint(
            face(&[(0.024, 0.742), (0.058, 0.91), (0.19, 1.02), (-0.008, 0.93)]),
            ice(0xf6ffff, 0xb4dcf7, 1.0, 180.0),
        );
        paint(
            face(&[
                (0.024, 0.742),
                (0.072, 0.823),
                (0.038, 0.805),
                (0.023, 0.835),
                (0.009, 0.80),
                (-0.009, 0.829),
            ]),
            ice(0xffffff, 0xe6fbff, 1.0, 180.0),
        );
        paint(
            face(&[(0.158, 0.78), (0.256, 0.88), (0.29, 1.0), (0.215, 0.934)]),
            ice(0x95dbe9, p.secondary, 0.26, 120.0),
        );

        // A glint traverses each snow ridge in about four seconds.
        let position = glint_position(right, t);
        paint(
            crystal(f, position, 7.0),
            ice(0xffffff, 0xb7faff, 0.22, 135.0),
        );
        paint(
            crystal(f, position, 3.8),
            ice(0xffffff, 0xe9ffff, 0.96, 135.0),
        );
    }

    // Rounded powder at the very bottom contrasts with the angular ice above.
    for layer in 0..2 {
        let y = 0.948 + layer as f32 * 0.034;
        let mut drift = PathBuilder::fill();
        drift.move_to(f.at(-0.02, y));
        drift.cubic_bezier_to(
            f.at(0.48, y + 0.009),
            f.at(0.14, y - 0.055),
            f.at(0.31, y + 0.045),
        );
        drift.cubic_bezier_to(
            f.at(1.02, y - 0.013),
            f.at(0.70, y - 0.04),
            f.at(0.86, y - 0.06),
        );
        drift.line_to(f.at(1.02, 1.03));
        drift.line_to(f.at(-0.02, 1.03));
        drift.close();
        paint(
            drift,
            ice(
                if layer == 0 { 0xe8faff } else { 0xffffff },
                p.elevated,
                0.94,
                180.0,
            ),
        );
    }

    let scale =
        (f32::from(bounds.size.width).min(f32::from(bounds.size.height)) / 600.0).clamp(0.25, 1.35);
    for i in 0..28 {
        let position = snow_position(i, t);
        let radius = (1.3 + seed(i + 190) * 2.4) * scale;
        let alpha = 0.48 + 0.23 * (t * 1.8 + i as f32).sin();
        paint(
            crystal(f, position, radius),
            ice(
                0xffffff,
                if i % 3 == 0 { p.secondary } else { p.primary },
                alpha,
                160.0,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use gpui::{px, size};

    #[test]
    fn paths_tessellate_at_all_requested_sizes_and_phases() {
        for (w, h) in [(600.0, 500.0), (1440.0, 900.0), (1.0, 1.0)] {
            for phase in [0.0, 1.0, 13.0] {
                let mut count = 0;
                compose(
                    Bounds::new(point(px(17.0), px(31.0)), size(px(w), px(h))),
                    Theme::AlpinePop.palette(),
                    phase,
                    &mut |path, _| {
                        path.build().expect("alpine geometry must tessellate");
                        count += 1;
                    },
                );
                assert_eq!(count, 63);
                assert!(count <= 100);
            }
        }
    }

    #[test]
    fn snow_and_ridge_geometry_move_within_one_second() {
        for i in 0..28 {
            let a = snow_position(i, 0.0);
            let b = snow_position(i, 1.0);
            assert_ne!(a, b);
            assert!((a.1 - b.1).abs() > 0.03);
            for t in [0.0, 1.0, 13.0] {
                let (x, y) = snow_position(i, t);
                assert!(!(0.20..=0.80).contains(&x));
                assert!((0.0..1.0).contains(&y));
            }
        }
        for right in [false, true] {
            let a = glint_position(right, 0.0);
            let b = glint_position(right, 1.0);
            assert!((a.1 - b.1).abs() > 0.07);
            assert_eq!(a, glint_position(right, 0.0));
        }
    }
}
