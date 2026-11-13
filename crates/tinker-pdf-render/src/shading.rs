//! Shadings: gradients and meshes (8.7.4.5).
//!
//! Types 1 to 3 paint a colour that varies with position, so they are
//! evaluated per pixel rather than composited from a mask, and
//! [`Shading::color_at`] is the whole of them. Types 4 to 7 are *geometry* —
//! triangles and patches — and cannot answer that question at a point, so they
//! carry a [`Mesh`] and are painted by the mesh rasteriser instead. That is why
//! `color_at` returns `None` for them rather than growing a branch that guesses.
//!
//! A shading is evaluated once a pixel, so what an evaluation writes — the
//! function's outputs, and a `/Separation` or `/DeviceN` space's tint — goes
//! into a [`ShadingScratch`] the sweep keeps from one pixel to the next, and
//! painting a shading allocates nothing per pixel.

use tinker_pdf_color::{ColorSpace, Function};

use crate::mesh::Mesh;

/// What evaluating a shading writes, kept by a sweep from one pixel to the
/// next: a function's outputs and a colour space's tint transform's.
#[derive(Clone, Debug, Default)]
pub struct ShadingScratch {
    outputs: Vec<f64>,
    converted: Vec<f64>,
}

impl ShadingScratch {
    /// `function` at `inputs`, converted from `space`.
    pub(crate) fn color(
        &mut self,
        space: &ColorSpace,
        function: &Function,
        inputs: &[f64],
    ) -> (u8, u8, u8) {
        function.eval_into(inputs, &mut self.outputs);
        space.to_rgb_with(&self.outputs, &mut self.converted)
    }

    /// `components` of `space`, converted, with no function between.
    pub(crate) fn convert(&mut self, space: &ColorSpace, components: &[f64]) -> (u8, u8, u8) {
        space.to_rgb_with(components, &mut self.converted)
    }
}

/// A shading, reduced to what painting needs.
#[derive(Clone, Debug)]
pub enum Shading {
    /// Type 1: a function of `(x, y)` over a domain.
    FunctionBased {
        /// The colour space the function's outputs are in.
        space: ColorSpace,
        /// The function.
        function: Function,
        /// `[x0 x1 y0 y1]`, the domain in shading space.
        domain: [f64; 4],
    },
    /// Type 2: a gradient along the line between two points.
    Axial {
        /// The colour space.
        space: ColorSpace,
        /// The function from parameter to colour.
        function: Function,
        /// `[x0 y0 x1 y1]`, the axis in shading space.
        coords: [f64; 4],
        /// Whether the gradient continues past each end.
        extend: (bool, bool),
    },
    /// Type 3: a gradient between two circles.
    Radial {
        /// The colour space.
        space: ColorSpace,
        /// The function from parameter to colour.
        function: Function,
        /// `[x0 y0 r0 x1 y1 r1]`, the two circles.
        coords: [f64; 6],
        /// Whether the gradient continues past each end.
        extend: (bool, bool),
    },
    /// Types 4 to 7: a mesh of Gouraud triangles or of Coons or tensor
    /// patches (8.7.4.5.5 to 8.7.4.5.8).
    Mesh(Box<Mesh>),
}

impl Shading {
    /// The mesh a type 4 to 7 shading paints, if this is one.
    #[must_use]
    pub fn mesh(&self) -> Option<&Mesh> {
        match self {
            Shading::Mesh(mesh) => Some(mesh),
            _ => None,
        }
    }

    /// The colour at a point in shading space, or `None` where the shading
    /// paints nothing. `scratch` is the sweep's, the same one for every pixel.
    ///
    /// Always `None` for a mesh: a mesh is not a function of position, and
    /// answering "what colour is here" would mean locating the containing
    /// triangle per pixel, which is the search the rasteriser exists to avoid.
    #[must_use]
    pub fn color_at(&self, x: f64, y: f64, scratch: &mut ShadingScratch) -> Option<(u8, u8, u8)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }

        match self {
            Shading::Mesh(_) => None,
            Shading::FunctionBased {
                space,
                function,
                domain,
            } => {
                let [x0, x1, y0, y1] = *domain;
                if x < x0.min(x1) || x > x0.max(x1) || y < y0.min(y1) || y > y0.max(y1) {
                    return None;
                }
                Some(scratch.color(space, function, &[x, y]))
            }

            Shading::Axial {
                space,
                function,
                coords,
                extend,
            } => {
                let [ax, ay, bx, by] = *coords;
                let (dx, dy) = (bx - ax, by - ay);
                let denominator = dx * dx + dy * dy;
                if denominator <= f64::EPSILON {
                    return None; // A zero-length axis paints nothing.
                }
                // The parameter is the projection onto the axis.
                let t = ((x - ax) * dx + (y - ay) * dy) / denominator;
                let t = clamp_extend(t, *extend)?;
                Some(scratch.color(space, function, &[t]))
            }

            Shading::Radial {
                space,
                function,
                coords,
                extend,
            } => {
                let t = radial_parameter(*coords, x, y, *extend)?;
                Some(scratch.color(space, function, &[t]))
            }
        }
    }
}

/// Clamps a parameter to `0..=1`, or refuses it where the shading does not
/// extend (8.7.4.5.3).
fn clamp_extend(t: f64, extend: (bool, bool)) -> Option<f64> {
    if !t.is_finite() {
        return None;
    }
    if t < 0.0 {
        return extend.0.then_some(0.0);
    }
    if t > 1.0 {
        return extend.1.then_some(1.0);
    }
    Some(t)
}

/// The parameter of the largest circle in the family that contains a point
/// (8.7.4.5.4).
///
/// The family interpolates centre and radius linearly, so the condition is a
/// quadratic in `s`; the largest root with a non-negative radius wins, which
/// is what makes a radial gradient paint front-to-back correctly.
fn radial_parameter(coords: [f64; 6], x: f64, y: f64, extend: (bool, bool)) -> Option<f64> {
    let [x0, y0, r0, x1, y1, r1] = coords;

    let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
    let (px, py) = (x - x0, y - y0);

    let a = dx * dx + dy * dy - dr * dr;
    let b = px * dx + py * dy + r0 * dr;
    let c = px * px + py * py - r0 * r0;

    let candidates: [Option<f64>; 2] = if a.abs() < 1e-9 {
        // Degenerate: the circles have the same size, so it is linear.
        if b.abs() < 1e-12 {
            return None;
        }
        [Some(c / (2.0 * b)), None]
    } else {
        let discriminant = b * b - a * c;
        if discriminant < 0.0 {
            return None; // No circle in the family reaches this point.
        }
        let root = discriminant.sqrt();
        [Some((b + root) / a), Some((b - root) / a)]
    };

    // Prefer the larger parameter: the later circle is painted on top.
    let mut best: Option<f64> = None;
    for candidate in candidates.into_iter().flatten() {
        if !candidate.is_finite() {
            continue;
        }
        // A circle with a negative radius does not exist.
        if r0 + candidate * dr < 0.0 {
            continue;
        }
        let Some(clamped) = clamp_extend(candidate, extend) else {
            continue;
        };
        best = Some(match best {
            None => clamped,
            Some(previous) => previous.max(clamped),
        });
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The colour at a point, from a scratch of its own.
    fn at(shading: &Shading, x: f64, y: f64) -> Option<(u8, u8, u8)> {
        shading.color_at(x, y, &mut ShadingScratch::default())
    }

    fn ramp() -> Function {
        // Black at zero, white at one.
        Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0],
            c1: vec![1.0],
            n: 1.0,
        }
    }

    fn axial(extend: (bool, bool)) -> Shading {
        Shading::Axial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 10.0, 0.0],
            extend,
        }
    }

    #[test]
    fn an_axial_gradient_runs_along_its_axis() {
        let shading = axial((true, true));
        assert_eq!(at(&shading, 0.0, 0.0), Some((0, 0, 0)), "the start");
        assert_eq!(at(&shading, 10.0, 0.0), Some((255, 255, 255)), "the end");
        let middle = at(&shading, 5.0, 0.0).expect("a colour");
        assert!(
            (125..=130).contains(&middle.0),
            "halfway is mid grey, got {middle:?}"
        );
    }

    #[test]
    fn the_axis_is_a_projection_not_a_distance() {
        // A point off the axis takes the colour of its projection onto it.
        let shading = axial((true, true));
        assert_eq!(at(&shading, 5.0, 100.0), at(&shading, 5.0, 0.0));
    }

    #[test]
    fn extend_decides_what_happens_past_the_ends() {
        let extended = axial((true, true));
        assert_eq!(at(&extended, -50.0, 0.0), Some((0, 0, 0)));
        assert_eq!(at(&extended, 50.0, 0.0), Some((255, 255, 255)));

        let bounded = axial((false, false));
        assert_eq!(at(&bounded, -50.0, 0.0), None, "nothing before");
        assert_eq!(at(&bounded, 50.0, 0.0), None, "nothing after");
        assert!(at(&bounded, 5.0, 0.0).is_some(), "but inside is painted");
    }

    #[test]
    fn a_degenerate_axis_paints_nothing() {
        let shading = Shading::Axial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [3.0, 3.0, 3.0, 3.0],
            extend: (true, true),
        };
        assert_eq!(at(&shading, 3.0, 3.0), None);
    }

    #[test]
    fn a_radial_gradient_runs_between_its_circles() {
        // Concentric: a point at the inner radius is at zero, the outer at one.
        let shading = Shading::Radial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 0.0, 0.0, 0.0, 10.0],
            extend: (true, true),
        };

        assert_eq!(at(&shading, 0.0, 0.0), Some((0, 0, 0)), "the centre");
        assert_eq!(at(&shading, 10.0, 0.0), Some((255, 255, 255)), "the rim");
        let middle = at(&shading, 5.0, 0.0).expect("a colour");
        assert!(
            (125..=130).contains(&middle.0),
            "halfway out is mid grey, got {middle:?}"
        );
    }

    #[test]
    fn a_radial_gradient_that_does_not_extend_stops_at_its_rim() {
        let shading = Shading::Radial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 0.0, 0.0, 0.0, 10.0],
            extend: (false, false),
        };
        assert!(at(&shading, 5.0, 0.0).is_some(), "inside");
        assert_eq!(at(&shading, 50.0, 0.0), None, "well outside");
    }

    #[test]
    fn nonsense_coordinates_are_refused_rather_than_painted() {
        let shading = axial((true, true));
        assert_eq!(at(&shading, f64::NAN, 0.0), None);
        assert_eq!(at(&shading, 0.0, f64::INFINITY), None);

        // A radial family with no real solution anywhere.
        let impossible = Shading::Radial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 5.0, 0.0, 0.0, 5.0],
            extend: (false, false),
        };
        let _ = at(&impossible, 100.0, 100.0);
    }

    /// **A sweep's one scratch paints what a fresh one would at every
    /// pixel.** Nothing an evaluation leaves in the scratch may reach the
    /// next: not a longer function's outputs before a shorter one's, not a
    /// tint transform's, not a type 4 program's stack. Each shading is swept
    /// over a grid through one scratch, against a fresh scratch per point,
    /// and the shadings take turns on the one scratch so their outputs
    /// differ in length from one point to the next.
    #[test]
    fn a_kept_scratch_paints_what_a_fresh_one_does() {
        let gamma = |c0: Vec<f64>, c1: Vec<f64>, n: f64| Function::Exponential {
            domain: (0.0, 1.0),
            c0,
            c1,
            n,
        };
        let spot = ColorSpace::Separation {
            components: 1,
            alternate: Box::new(ColorSpace::DeviceCmyk),
            tint: Box::new(gamma(vec![0.0; 4], vec![0.1, 0.7, 0.2, 0.05], 1.8)),
        };
        let two_inks = ColorSpace::Separation {
            components: 2,
            alternate: Box::new(ColorSpace::DeviceRgb),
            tint: Box::new(Function::PostScript {
                domain: vec![(0.0, 1.0); 2],
                range: vec![(0.0, 1.0); 3],
                program: tinker_pdf_color::function::parse_postscript(
                    b"{ 2 copy add 0.5 mul 3 1 roll }",
                ),
            }),
        };
        let shadings = [
            Shading::Axial {
                space: ColorSpace::DeviceRgb,
                function: gamma(vec![0.0, 0.1, 0.6], vec![1.0, 0.9, 0.2], 1.0),
                coords: [0.0, 0.0, 40.0, 25.0],
                extend: (true, false),
            },
            Shading::Radial {
                space: ColorSpace::DeviceRgb,
                function: Function::Stitching {
                    domain: (0.0, 1.0),
                    functions: vec![
                        gamma(vec![1.0, 1.0, 1.0], vec![0.9, 0.2, 0.1], 2.2),
                        gamma(vec![0.9, 0.2, 0.1], vec![0.0, 0.0, 0.3], 0.5),
                    ],
                    bounds: vec![0.4],
                    encode: vec![(0.0, 1.0), (0.0, 1.0)],
                },
                coords: [20.0, 20.0, 2.0, 24.0, 18.0, 30.0],
                extend: (false, true),
            },
            Shading::Axial {
                space: spot,
                function: ramp(),
                coords: [40.0, 0.0, 0.0, 40.0],
                extend: (true, true),
            },
            Shading::FunctionBased {
                space: two_inks,
                function: Function::PostScript {
                    domain: vec![(0.0, 40.0); 2],
                    range: vec![(0.0, 1.0); 2],
                    program: tinker_pdf_color::function::parse_postscript(
                        b"{ 40 div exch 40 div 2 copy gt { exch } if }",
                    ),
                },
                domain: [0.0, 40.0, 0.0, 40.0],
            },
            Shading::Axial {
                space: ColorSpace::DeviceGray,
                function: Function::Sampled {
                    domain: vec![(0.0, 1.0)],
                    range: vec![(0.0, 1.0)],
                    size: vec![5],
                    bits: 8,
                    encode: vec![],
                    decode: vec![],
                    samples: vec![0, 60, 130, 200, 255],
                },
                coords: [0.0, 40.0, 40.0, 0.0],
                extend: (true, true),
            },
        ];
        let mut kept = ShadingScratch::default();
        for row in 0..40 {
            for column in 0..40 {
                let (x, y) = (f64::from(column) + 0.5, f64::from(row) + 0.5);
                for (which, shading) in shadings.iter().enumerate() {
                    assert_eq!(
                        shading.color_at(x, y, &mut kept),
                        at(shading, x, y),
                        "shading {which} at ({x}, {y})"
                    );
                }
            }
        }
    }
}
