//! Confetti Sunset: a quiet cream sky above a paper-cut, golden-hour landscape.
use crate::theme::Palette;
use gpui::{
    Background, Bounds, PathBuilder, Pixels, Point, linear_color_stop, linear_gradient, point, px,
    rgb,
};
use std::f32::consts::TAU;

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
}

fn ink(a: u32, b: u32, alpha: f32) -> Background {
    linear_gradient(
        180.0,
        linear_color_stop(rgb(a), 0.0).opacity(alpha),
        linear_color_stop(rgb(b), 1.0).opacity(alpha),
    )
}

fn ellipse(center: Point<Pixels>, rx: f32, ry: f32) -> PathBuilder {
    let at = |x: f32, y: f32| point(center.x + px(x * rx), center.y + px(y * ry));
    let k = 0.552_284_8;
    let mut path = PathBuilder::fill();
    path.move_to(at(1.0, 0.0));
    path.cubic_bezier_to(at(0.0, 1.0), at(1.0, k), at(k, 1.0));
    path.cubic_bezier_to(at(-1.0, 0.0), at(-k, 1.0), at(-1.0, k));
    path.cubic_bezier_to(at(0.0, -1.0), at(-1.0, -k), at(-k, -1.0));
    path.cubic_bezier_to(at(1.0, 0.0), at(k, -1.0), at(1.0, -k));
    path.close();
    path
}

#[derive(Debug, PartialEq)]
struct Paper {
    x: f32,
    y: f32,
    angle: f32,
    fold: f32,
    opacity: f32,
}

fn paper(index: usize, phase: f32) -> Paper {
    // Wrapping occurs beyond the canvas, with an additional fade at either end.
    // A whole number of turns per fall also makes the orientation loop continuous.
    let cycle = (phase as f64 / (15.0 + (index % 5) as f64 * 1.7) + index as f64 * 0.381_966)
        .rem_euclid(1.0) as f32;
    let angle = cycle * TAU * 3.0 + index as f32 * 1.7;
    let lane = 0.035 + (index % 5) as f32 * 0.034;
    Paper {
        x: if index.is_multiple_of(2) {
            lane
        } else {
            1.0 - lane
        } + (cycle * TAU * 2.0 + index as f32).sin() * 0.014,
        y: -0.08 + cycle * 1.16,
        angle,
        fold: 0.22 + 0.78 * (angle * 2.0).cos().abs(),
        opacity: (cycle * 14.0).min((1.0 - cycle) * 14.0).clamp(0.0, 1.0),
    }
}

pub(super) fn compose(
    bounds: Bounds<Pixels>,
    p: Palette,
    phase: f32,
    paint: &mut dyn FnMut(PathBuilder, Background),
) {
    let f = Frame(bounds);
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    let t = phase as f64;
    let pulse = (t * 1.7).sin() as f32;
    let bob = (t * 1.15).sin() as f32;
    let mut sky = PathBuilder::fill();
    sky.move_to(f.at(0.0, 0.0));
    sky.line_to(f.at(1.0, 0.0));
    sky.line_to(f.at(1.0, 1.0));
    sky.line_to(f.at(0.0, 1.0));
    sky.close();
    paint(
        sky,
        linear_gradient(
            180.0,
            linear_color_stop(rgb(0xffe1c5), 0.0),
            linear_color_stop(rgb(0xfffaf0), 0.46),
        ),
    );

    // Physical-pixel radii preserve a circular sun even in a wide, shallow panel.
    let sun = f.at(0.12, 0.785 + bob * 0.005);
    let radius = (w * 0.082).min(h * 0.135);
    for (scale, alpha) in [(1.85, 0.045), (1.53, 0.07), (1.26, 0.12)] {
        let r = radius * (scale + pulse * 0.035);
        paint(ellipse(sun, r, r), ink(0xffae48, 0xffc477, alpha));
    }
    for index in 0..11 {
        let angle = std::f32::consts::PI * (1.04 + index as f32 * 0.092);
        let length = 1.46 + pulse * 0.1;
        let at = |a: f32, r: f32| {
            point(
                sun.x + px(a.cos() * radius * r),
                sun.y + px(a.sin() * radius * r),
            )
        };
        let mut ray = PathBuilder::fill();
        ray.move_to(at(angle - 0.018, 1.13));
        ray.line_to(at(angle, length));
        ray.line_to(at(angle + 0.018, 1.13));
        ray.close();
        paint(ray, ink(0xffb649, 0xffd591, 0.42 + pulse * 0.12));
    }
    paint(ellipse(sun, radius, radius), ink(0xffdf70, 0xff914b, 1.0));
    paint(
        ellipse(
            point(sun.x - px(radius * 0.24), sun.y - px(radius * 0.30)),
            radius * 0.37,
            radius * 0.18,
        ),
        ink(0xfff4bf, 0xffeaa0, 0.44),
    );

    let colors = [0xf47565, p.secondary, 0xed5382, 0xf6b833, p.highlight];
    let unit = w.min(h) * 0.009;
    for index in 0..22 {
        let paper = paper(index, phase);
        let center = f.at(paper.x, paper.y);
        let (sin, cos) = paper.angle.sin_cos();
        let width = unit * (0.55 + (index % 3) as f32 * 0.13) * paper.fold;
        let height = unit * (1.0 + (index % 4) as f32 * 0.16);
        let at = |x: f32, y: f32| {
            point(
                center.x + px(x * width * cos - y * height * sin),
                center.y + px(x * width * sin + y * height * cos),
            )
        };
        let mut scrap = PathBuilder::fill();
        scrap.move_to(at(-1.0, -1.0));
        scrap.line_to(at(1.0, -0.8));
        scrap.line_to(at(1.0, 1.0));
        scrap.line_to(at(-1.0, 0.8));
        scrap.close();
        let color = colors[index % colors.len()];
        paint(scrap, ink(color, color, 0.78 * paper.opacity));
    }

    // Scalloped, drifting cloud banks frame the sun, not the central reading area.
    for right in [false, true] {
        let x = |v: f32| if right { 1.0 - v } else { v };
        let drift = (t * 0.95 + if right { 1.7 } else { 0.0 }).sin() as f32 * 0.009;
        let y = |v: f32| v + drift;
        let mut cloud = PathBuilder::fill();
        cloud.move_to(f.at(x(-0.03), y(0.81)));
        f.curve(
            &mut cloud,
            (x(0.045), y(0.76)),
            (x(-0.02), y(0.73)),
            (x(0.02), y(0.72)),
        );
        f.curve(
            &mut cloud,
            (x(0.13), y(0.79)),
            (x(0.07), y(0.67)),
            (x(0.13), y(0.70)),
        );
        f.curve(
            &mut cloud,
            (x(0.23), y(0.86)),
            (x(0.18), y(0.74)),
            (x(0.22), y(0.80)),
        );
        cloud.line_to(f.at(x(0.25), 1.0));
        cloud.line_to(f.at(x(-0.03), 1.0));
        cloud.close();
        paint(cloud, ink(0xffedd9, 0xffbfa9, 0.83));
    }

    // Full silhouettes rather than ribbons: three receding paper-cut hill ranges.
    for (index, (left, middle, right, a, b)) in [
        (0.855, 0.93, 0.84, 0xffba94, 0xff9d86),
        (0.91, 0.965, 0.88, 0xf88387, 0xec6c85),
        (0.965, 0.992, 0.94, 0xd94d78, 0xc8446d),
    ]
    .into_iter()
    .enumerate()
    {
        let sway = (t * 0.65 + index as f64 * 1.3).sin() as f32 * 0.003;
        let mut hill = PathBuilder::fill();
        hill.move_to(f.at(-0.02, left));
        f.curve(
            &mut hill,
            (0.48, middle),
            (0.15, left - 0.065 + sway),
            (0.25, middle + 0.005),
        );
        f.curve(
            &mut hill,
            (1.02, right),
            (0.70, middle + 0.015),
            (0.78, right - 0.08 - sway),
        );
        hill.line_to(f.at(1.02, 1.02));
        hill.line_to(f.at(-0.02, 1.02));
        hill.close();
        paint(hill, ink(a, b, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use gpui::size;

    #[test]
    fn sunset_paths_tessellate_across_sizes_and_phases() {
        for (w, h) in [(1440.0, 900.0), (320.0, 800.0), (2400.0, 320.0), (1.0, 1.0)] {
            for phase in [0.0, 0.5, 2.0, 15.0, 37.0, 600.0, f32::MAX] {
                let mut count = 0;
                compose(
                    Bounds::new(point(px(17.0), px(31.0)), size(px(w), px(h))),
                    Theme::ConfettiSunset.palette(),
                    phase,
                    &mut |path, _| {
                        path.build().expect("sunset path must tessellate");
                        count += 1;
                    },
                );
                assert_eq!(count, 44);
            }
        }
    }

    #[test]
    fn confetti_moves_visibly_and_stays_out_of_the_center() {
        for index in 0..22 {
            let start = paper(index, 0.0);
            let later = paper(index, 2.0);
            assert_ne!(start, later);
            assert!((start.y - later.y).abs() > 0.08);
            for phase in [0.0, 0.5, 2.0, 7.0, 15.0, 30.0, 1000.0] {
                let state = paper(index, phase);
                assert!(state.x < 0.20 || state.x > 0.80);
                assert!(state.angle.is_finite() && state.fold > 0.0);
            }
        }
    }

    #[test]
    fn confetti_reset_is_invisible() {
        // Index zero starts at its cycle boundary, above the visible canvas.
        let start = paper(0, 0.0);
        let before = paper(0, 15.0 - 0.0001);
        let after = paper(0, 15.0 + 0.0001);
        assert_eq!(start.opacity, 0.0);
        assert!(before.opacity < 0.001 && after.opacity < 0.001);
        assert!(before.y > 1.0 && after.y < 0.0);
        assert!((before.x - after.x).abs() < 0.001);
    }
}
