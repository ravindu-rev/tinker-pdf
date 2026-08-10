//! Shadings: axial and radial gradients (8.7.4.5).
//!
//! A shading paints a colour that varies with position, so it is evaluated
//! per pixel rather than composited from a mask. Types 1 to 3 are here; the
//! mesh types (4 to 7) are behind a capability, because they are a large
//! amount of geometry for a construct almost nothing produces.

use tinker_pdf_color::{ColorSpace, Function};

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
}

impl Shading {
    /// The colour at a point in shading space, or `None` where the shading
    /// paints nothing.
    #[must_use]
    pub fn color_at(&self, x: f64, y: f64) -> Option<(u8, u8, u8)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }

        match self {
            Shading::FunctionBased {
                space,
                function,
                domain,
            } => {
                let [x0, x1, y0, y1] = *domain;
                if x < x0.min(x1) || x > x0.max(x1) || y < y0.min(y1) || y > y0.max(y1) {
                    return None;
                }
                Some(space.to_rgb(&function.eval(&[x, y])))
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
                Some(space.to_rgb(&function.eval(&[t])))
            }

            Shading::Radial {
                space,
                function,
                coords,
                extend,
            } => {
                let t = radial_parameter(*coords, x, y, *extend)?;
                Some(space.to_rgb(&function.eval(&[t])))
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
        assert_eq!(shading.color_at(0.0, 0.0), Some((0, 0, 0)), "the start");
        assert_eq!(
            shading.color_at(10.0, 0.0),
            Some((255, 255, 255)),
            "the end"
        );
        let middle = shading.color_at(5.0, 0.0).expect("a colour");
        assert!(
            (125..=130).contains(&middle.0),
            "halfway is mid grey, got {middle:?}"
        );
    }

    #[test]
    fn the_axis_is_a_projection_not_a_distance() {
        // A point off the axis takes the colour of its projection onto it.
        let shading = axial((true, true));
        assert_eq!(shading.color_at(5.0, 100.0), shading.color_at(5.0, 0.0));
    }

    #[test]
    fn extend_decides_what_happens_past_the_ends() {
        let extended = axial((true, true));
        assert_eq!(extended.color_at(-50.0, 0.0), Some((0, 0, 0)));
        assert_eq!(extended.color_at(50.0, 0.0), Some((255, 255, 255)));

        let bounded = axial((false, false));
        assert_eq!(bounded.color_at(-50.0, 0.0), None, "nothing before");
        assert_eq!(bounded.color_at(50.0, 0.0), None, "nothing after");
        assert!(
            bounded.color_at(5.0, 0.0).is_some(),
            "but inside is painted"
        );
    }

    #[test]
    fn a_degenerate_axis_paints_nothing() {
        let shading = Shading::Axial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [3.0, 3.0, 3.0, 3.0],
            extend: (true, true),
        };
        assert_eq!(shading.color_at(3.0, 3.0), None);
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

        assert_eq!(shading.color_at(0.0, 0.0), Some((0, 0, 0)), "the centre");
        assert_eq!(
            shading.color_at(10.0, 0.0),
            Some((255, 255, 255)),
            "the rim"
        );
        let middle = shading.color_at(5.0, 0.0).expect("a colour");
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
        assert!(shading.color_at(5.0, 0.0).is_some(), "inside");
        assert_eq!(shading.color_at(50.0, 0.0), None, "well outside");
    }

    #[test]
    fn nonsense_coordinates_are_refused_rather_than_painted() {
        let shading = axial((true, true));
        assert_eq!(shading.color_at(f64::NAN, 0.0), None);
        assert_eq!(shading.color_at(0.0, f64::INFINITY), None);

        // A radial family with no real solution anywhere.
        let impossible = Shading::Radial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 5.0, 0.0, 0.0, 5.0],
            extend: (false, false),
        };
        let _ = impossible.color_at(100.0, 100.0);
    }
}
