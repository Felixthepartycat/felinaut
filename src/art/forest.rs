//! A little fruit garden framing a quiet cream-and-mint clearing.
use crate::theme::Palette;
use gpui::{
    Background, Bounds, PathBuilder, Pixels, Point, linear_color_stop, linear_gradient, point, px,
    rgb,
};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy)]
struct Garden {
    bounds: Bounds<Pixels>,
    unit: f32,
}

impl Garden {
    fn at(self, x: f32, y: f32) -> Point<Pixels> {
        point(
            self.bounds.origin.x + self.bounds.size.width * x,
            self.bounds.origin.y + self.bounds.size.height * y,
        )
    }

    // Local ornaments use the shorter dimension, so citrus stays round on wide windows.
    fn local(self, anchor: Point<Pixels>, x: f32, y: f32) -> Point<Pixels> {
        point(anchor.x + px(x * self.unit), anchor.y + px(y * self.unit))
    }
}

fn ink(color: u32) -> Background {
    rgb(color).into()
}

fn wash(a: u32, b: u32) -> Background {
    linear_gradient(
        150.0,
        linear_color_stop(rgb(a), 0.0),
        linear_color_stop(rgb(b), 1.0),
    )
}

fn ellipse(g: Garden, center: Point<Pixels>, rx: f32, ry: f32, angle: f32) -> PathBuilder {
    let (s, c) = angle.sin_cos();
    let at = |x: f32, y: f32| g.local(center, c * x * rx - s * y * ry, s * x * rx + c * y * ry);
    let mut path = PathBuilder::fill();
    let k = 0.552_284_8;
    path.move_to(at(1.0, 0.0));
    path.cubic_bezier_to(at(0.0, 1.0), at(1.0, k), at(k, 1.0));
    path.cubic_bezier_to(at(-1.0, 0.0), at(-k, 1.0), at(-1.0, k));
    path.cubic_bezier_to(at(0.0, -1.0), at(-1.0, -k), at(-k, -1.0));
    path.cubic_bezier_to(at(1.0, 0.0), at(k, -1.0), at(1.0, -k));
    path.close();
    path
}

fn leaf(
    g: Garden,
    base: Point<Pixels>,
    length: f32,
    width: f32,
    angle: f32,
    color: u32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    let (s, c) = angle.sin_cos();
    let at = |x: f32, y: f32| g.local(base, c * x - s * y, s * x + c * y);
    let mut blade = PathBuilder::fill();
    blade.move_to(at(0.0, 0.0));
    blade.cubic_bezier_to(
        at(0.0, -length),
        at(-width, -length * 0.28),
        at(-width * 0.78, -length * 0.82),
    );
    blade.cubic_bezier_to(
        at(0.0, 0.0),
        at(width * 0.88, -length * 0.72),
        at(width * 0.65, -length * 0.2),
    );
    blade.close();
    paint(blade, ink(color));
    let mut vein = PathBuilder::stroke(px(g.unit * 0.0015));
    vein.move_to(at(0.0, 0.0));
    vein.cubic_bezier_to(
        at(0.0, -length * 0.88),
        at(width * 0.08, -length * 0.35),
        at(-width * 0.12, -length * 0.6),
    );
    for i in 1..4 {
        let y = -length * (0.16 + i as f32 * 0.15);
        vein.move_to(at(-width * 0.48, y - length * 0.11));
        vein.line_to(at(0.0, y));
        vein.line_to(at(width * 0.42, y - length * 0.13));
    }
    paint(vein, ink(0x9cdb8a));
}

fn fern(
    g: Garden,
    anchor: Point<Pixels>,
    side: f32,
    sway: f32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    let at = |x: f32, y: f32| g.local(anchor, side * (x + sway * -y), y);
    let mut frond = PathBuilder::fill();
    for i in 0..7 {
        let t = i as f32 / 7.0;
        let x = 0.10 * t;
        let y = -0.25 * t;
        let reach = 0.062 * (1.0 - t * 0.7);
        for direction in [-1.0, 1.0] {
            frond.move_to(at(x, y));
            frond.cubic_bezier_to(
                at(x + direction * reach, y - 0.052),
                at(x + direction * reach * 0.85, y + 0.006),
                at(x + direction * reach * 1.15, y - 0.024),
            );
            frond.cubic_bezier_to(
                at(x, y),
                at(x + direction * reach * 0.25, y - 0.052),
                at(x + direction * reach * 0.15, y - 0.012),
            );
            frond.close();
        }
    }
    paint(frond, ink(0x338c62));
    let mut stem = PathBuilder::stroke(px(g.unit * 0.002));
    stem.move_to(at(0.0, 0.02));
    stem.cubic_bezier_to(at(0.10, -0.26), at(0.025, -0.08), at(0.07, -0.22));
    paint(stem, ink(0x8ac57a));
}

fn citrus(
    g: Garden,
    center: Point<Pixels>,
    radius: f32,
    rotation: f32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    paint(ellipse(g, center, radius, radius, 0.0), ink(0xf4a139));
    paint(
        ellipse(g, center, radius * 0.87, radius * 0.87, 0.0),
        ink(0xfff5cc),
    );
    // Separate juicy wedges leave a thick, unmistakable pith-and-spoke silhouette.
    for i in 0..8 {
        let a = rotation + i as f32 * TAU / 8.0 + 0.065;
        let b = a + TAU / 8.0 - 0.13;
        let r = radius * 0.76;
        let at = |angle: f32, r: f32| g.local(center, angle.cos() * r, angle.sin() * r);
        let mut wedge = PathBuilder::fill();
        wedge.move_to(at((a + b) * 0.5, radius * 0.12));
        wedge.line_to(at(a, r));
        wedge.cubic_bezier_to(
            at(b, r),
            at(a + (b - a) / 3.0, r * 1.05),
            at(b - (b - a) / 3.0, r * 1.05),
        );
        wedge.close();
        paint(wedge, ink(if i % 2 == 0 { 0xffbd53 } else { 0xf89640 }));
    }
}

fn flower(
    g: Garden,
    center: Point<Pixels>,
    radius: f32,
    angle: f32,
    violet: bool,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    // Petals share one path, leaving the whole garden well below the path budget.
    let mut petals = PathBuilder::fill();
    for i in 0..5 {
        let a = angle + i as f32 * TAU / 5.0;
        let (s, c) = a.sin_cos();
        let at =
            |x: f32, y: f32| g.local(center, (c * x - s * y) * radius, (s * x + c * y) * radius);
        petals.move_to(at(0.0, 0.0));
        petals.cubic_bezier_to(at(0.0, -1.0), at(-0.9, -0.55), at(-0.55, -1.3));
        petals.cubic_bezier_to(at(0.0, 0.0), at(0.55, -1.3), at(0.9, -0.55));
        petals.close();
    }
    paint(petals, ink(if violet { 0xa37bc5 } else { 0xef92b2 }));
    paint(
        ellipse(g, center, radius * 0.25, radius * 0.25, 0.0),
        ink(0xffd775),
    );
}

pub(super) fn compose(
    bounds: Bounds<Pixels>,
    p: Palette,
    phase: f32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    let g = Garden {
        bounds,
        unit: f32::from(bounds.size.width).min(f32::from(bounds.size.height)),
    };
    let t = phase;
    let mut field = PathBuilder::fill();
    field.move_to(g.at(0.0, 0.0));
    field.line_to(g.at(1.0, 0.0));
    field.line_to(g.at(1.0, 1.0));
    field.line_to(g.at(0.0, 1.0));
    field.close();
    paint(
        field,
        wash(
            if p.dark { p.background } else { 0xfffbee },
            if p.dark { p.surface } else { 0xe5f3e7 },
        ),
    );

    for (height, color) in [(0.88, 0xd0e7b8), (0.94, 0xb3d89c), (0.985, 0x8dc589)] {
        let mut hill = PathBuilder::fill();
        hill.move_to(g.at(-0.03, height));
        hill.cubic_bezier_to(
            g.at(0.50, height + 0.035),
            g.at(0.14, height - 0.065),
            g.at(0.25, height + 0.07),
        );
        hill.cubic_bezier_to(
            g.at(1.03, height - 0.015),
            g.at(0.77, height + 0.065),
            g.at(0.84, height - 0.09),
        );
        hill.line_to(g.at(1.03, 1.03));
        hill.line_to(g.at(-0.03, 1.03));
        hill.close();
        paint(hill, ink(color));
    }

    for (right, offset) in [(false, 0.0), (true, 1.7)] {
        let side = if right { -1.0 } else { 1.0 };
        let edge = if right { 0.96 } else { 0.04 };
        let sway = (t * 0.95 + offset).sin() * 0.085;
        let base = g.at(edge, 1.025);
        for (i, (length, width, angle)) in [
            (0.37, 0.064, -0.26),
            (0.31, 0.075, 0.48),
            (0.26, 0.068, 0.98),
            (0.23, 0.055, -0.85),
        ]
        .into_iter()
        .enumerate()
        {
            leaf(
                g,
                base,
                length,
                width,
                side * (angle + sway * (1.0 + i as f32 * 0.18)),
                [0x226e52, 0x4e9b62, 0x75b974, 0x31855c][i],
                paint,
            );
        }
        fern(
            g,
            g.at(if right { 0.88 } else { 0.12 }, 1.015),
            side,
            sway,
            paint,
        );
        let fruit = g.local(
            g.at(if right { 0.91 } else { 0.09 }, 0.87),
            0.0,
            (t * 1.2 + offset).sin() * 0.009,
        );
        citrus(
            g,
            fruit,
            if right { 0.061 } else { 0.075 },
            side * 0.15 + sway * 0.5,
            paint,
        );
        let bloom = g.local(
            g.at(if right { 0.84 } else { 0.16 }, 0.96),
            sway * 0.08,
            0.0,
        );
        flower(g, bloom, 0.035, sway, right, paint);
    }

    // A hanging cherry pair and a smaller blossom occupy the left margin only.
    let cherry_anchor = g.at(0.055, 0.49);
    let bob = (t * 1.25).sin() * 0.012;
    let cherries = [
        g.local(cherry_anchor, -0.017 + bob, 0.10),
        g.local(cherry_anchor, 0.031 + bob, 0.12),
    ];
    let mut stems = PathBuilder::stroke(px(g.unit * 0.003));
    for center in cherries {
        stems.move_to(g.local(cherry_anchor, 0.025, 0.0));
        stems.cubic_bezier_to(
            center,
            g.local(cherry_anchor, 0.045 + bob, 0.035),
            g.local(center, 0.0, -0.04),
        );
    }
    paint(stems, ink(0x48835a));
    leaf(
        g,
        g.local(cherry_anchor, 0.025, 0.0),
        0.058,
        0.019,
        1.15 + bob,
        0x74ad67,
        paint,
    );
    for center in cherries {
        paint(
            ellipse(g, center, 0.025, 0.027, -0.12),
            wash(0xf28d9c, 0xcb4f77),
        );
        paint(
            ellipse(g, g.local(center, -0.007, -0.009), 0.005, 0.008, 0.4),
            ink(0xffd8cf),
        );
    }
    flower(
        g,
        g.at(0.95, 0.59),
        0.029,
        0.2 + (t * 0.8).sin() * 0.16,
        true,
        paint,
    );

    for i in 0..8 {
        let (x, y, angle) = petal_pose(i, t);
        paint(
            ellipse(g, g.at(x, y), 0.006, 0.013, angle),
            ink(if i % 2 == 0 { 0xe6a0bd } else { 0xb59acc }),
        );
    }
}

fn petal_pose(i: usize, t: f32) -> (f32, f32, f32) {
    let n = i as f32;
    let right = i % 2 == 1;
    let drift = (t * 0.78 + n * 1.9).sin();
    let x = 0.055 + (i / 2) as f32 * 0.033 + drift * 0.014;
    let y = 0.16 + (i / 2) as f32 * 0.16 + (t * 0.65 + n).sin() * 0.024;
    (
        if right { 1.0 - x } else { x },
        y,
        n * 0.8 + (t * 1.1 + n).sin() * 0.65 + PI * 0.15,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use gpui::size;

    #[test]
    fn forest_tessellates_at_all_requested_sizes_and_phases() {
        for (w, h) in [(600.0, 500.0), (1440.0, 900.0), (1.0, 1.0)] {
            for phase in [0.0, 1.0, 13.0] {
                let mut count = 0;
                compose(
                    Bounds::new(point(px(17.0), px(31.0)), size(px(w), px(h))),
                    Theme::ForestCarnival.palette(),
                    phase,
                    &mut |builder, _| {
                        builder
                            .build()
                            .expect("forest paths must tessellate, including tiny windows");
                        count += 1;
                    },
                );
                assert!(
                    (60..=100).contains(&count),
                    "unexpected path budget: {count}"
                );
            }
        }
    }

    #[test]
    fn tessellated_geometry_changes_but_the_clearing_stays_still() {
        let snapshot = |phase| {
            let mut paths = Vec::new();
            compose(
                Bounds::new(point(px(0.0), px(0.0)), size(px(600.0), px(500.0))),
                Theme::ForestCarnival.palette(),
                phase,
                &mut |builder, _| paths.push(format!("{:?}", builder.build().unwrap())),
            );
            paths
        };
        let still = snapshot(0.0);
        assert_eq!(still.len(), 65);
        assert_eq!(still, snapshot(0.0));
        for phase in [1.0, 13.0] {
            let moving = snapshot(phase);
            assert_eq!(still[..4], moving[..4], "field and hills must stay calm");
            assert!(still.iter().zip(&moving).filter(|(a, b)| a != b).count() > 40);
        }
    }

    #[test]
    fn petals_move_visibly_without_entering_the_clearing() {
        for i in 0..8 {
            let start = petal_pose(i, 0.0);
            let next = petal_pose(i, 1.0);
            assert_ne!(start, next);
            let distance = ((start.0 - next.0).powi(2) + (start.1 - next.1).powi(2)).sqrt();
            assert!(distance > 0.004, "petal must visibly move within a second");
            for t in [0.0, 1.0, 2.0, 13.0] {
                let (x, y, _) = petal_pose(i, t);
                assert!(!(0.22..=0.78).contains(&x));
                assert!((0.1..0.85).contains(&y));
            }
        }
    }
}
