//! Colour spaces and the PDF function interpreter (8.6, 7.10).
//!
//! A leaf crate: component values in, RGB out, with no PDF types anywhere in
//! the surface. The caller translates `/ColorSpace` dictionaries into these
//! plain descriptions.
//!
//! Colour management is reduced on purpose for this version. ICC profiles are
//! honoured only by their component count, falling back to the alternate space
//! the document names, and CIE-based spaces are approximated. A full profile
//! parser is a named later capability; what it would change is the exact shade
//! of a managed document, not whether it renders.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use tinker_pdf_math as math;

pub mod function;

pub use function::Function;

/// A colour space, reduced to what conversion needs.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorSpace {
    /// `/DeviceGray`, one component.
    DeviceGray,
    /// `/DeviceRGB`, three components.
    DeviceRgb,
    /// `/DeviceCMYK`, four components.
    DeviceCmyk,
    /// `/Indexed`: a palette over a base space (8.6.6.3).
    Indexed {
        /// The space the palette's entries are in.
        base: Box<ColorSpace>,
        /// The palette, base-space components packed one entry after another.
        lookup: Vec<u8>,
        /// The highest valid index.
        high: u32,
    },
    /// `/Separation` or `/DeviceN`: named inks mapped through a tint transform
    /// into an alternate space (8.6.6.4, 8.6.6.5).
    Separation {
        /// How many inks.
        components: usize,
        /// The space the transform produces.
        alternate: Box<ColorSpace>,
        /// The tint transform.
        tint: Box<Function>,
    },
    /// A CIE-based or ICC space, approximated by its component count.
    ///
    /// 8.6.5.5 lets a reader use the alternate space, and that is what this
    /// is: the shape of the data without the profile's exact rendering.
    Approximated {
        /// How many components.
        components: usize,
    },
    /// `/Lab`: CIE 1976 L*a*b* (8.6.5.4).
    ///
    /// Not an `Approximated`, because its components are not in 0..1: L runs
    /// 0..100 and a/b run roughly -128..127. Treating it as RGB clamps every
    /// value into 0..1 and renders almost the whole space as black.
    Lab {
        /// `/Range`, as `[a_min a_max b_min b_max]`.
        range: [f64; 4],
    },
    /// `/Pattern`, which carries no colour of its own (8.7.3).
    Pattern {
        /// The underlying space of an *uncoloured* pattern space, written
        /// `[/Pattern base]` (8.7.3.2).
        ///
        /// A `PaintType 2` pattern supplies shape and no colour, so the paint
        /// comes from the components an `scn` gives before the pattern name —
        /// and those components are in `base`, not in `/Pattern`. Without it
        /// they cannot be interpreted at all, and the pattern paints whatever
        /// the slot happened to hold.
        base: Option<Box<ColorSpace>>,
    },
}

impl ColorSpace {
    /// How many components a colour in this space has.
    #[must_use]
    pub fn components(&self) -> usize {
        match self {
            ColorSpace::DeviceGray => 1,
            ColorSpace::DeviceRgb => 3,
            ColorSpace::DeviceCmyk => 4,
            ColorSpace::Indexed { .. } => 1,
            ColorSpace::Separation { components, .. } => *components,
            ColorSpace::Approximated { components } => *components,
            ColorSpace::Lab { .. } => 3,
            // 8.7.3.2: an uncoloured pattern's operands are counted in the
            // underlying space. A plain `/Pattern` takes none at all, and 1 is
            // the answer that keeps callers which size a buffer from this from
            // sizing it to nothing.
            ColorSpace::Pattern { base } => base.as_ref().map_or(1, |b| b.components()),
        }
    }

    /// The colour this space's initial value is (8.6.8).
    #[must_use]
    pub fn initial(&self) -> Vec<f64> {
        match self {
            // Black in every device space, which for CMYK means all zeros
            // except the black ink.
            ColorSpace::DeviceCmyk => vec![0.0, 0.0, 0.0, 1.0],
            // 8.6.5.4: black is L=0 with no chroma, and zero is inside every
            // legal /Range, so the generic all-zeros answer is right here for
            // a different reason than it is elsewhere.
            ColorSpace::Lab { .. } => vec![0.0, 0.0, 0.0],
            other => vec![0.0; other.components()],
        }
    }

    /// Converts components to 8-bit sRGB.
    ///
    /// Out-of-range components clamp rather than wrap: a content stream may
    /// name any number, and a wrapped one turns black into white.
    #[must_use]
    pub fn to_rgb(&self, components: &[f64]) -> (u8, u8, u8) {
        let at = |i: usize| components.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);

        match self {
            ColorSpace::DeviceGray => {
                let v = byte(at(0));
                (v, v, v)
            }
            ColorSpace::DeviceRgb => (byte(at(0)), byte(at(1)), byte(at(2))),
            ColorSpace::DeviceCmyk => {
                // 8.6.4.4: the additive complement, with black applied.
                let (c, m, y, k) = (at(0), at(1), at(2), at(3));
                (
                    byte((1.0 - c) * (1.0 - k)),
                    byte((1.0 - m) * (1.0 - k)),
                    byte((1.0 - y) * (1.0 - k)),
                )
            }
            ColorSpace::Indexed { base, lookup, high } => {
                let index = components
                    .first()
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, f64::from(*high))
                    .round() as usize;
                let n = base.components();
                let start = index.saturating_mul(n);
                let entry: Vec<f64> = (0..n)
                    .map(|i| lookup.get(start + i).map_or(0.0, |&b| f64::from(b) / 255.0))
                    .collect();
                base.to_rgb(&entry)
            }
            ColorSpace::Separation {
                alternate, tint, ..
            } => {
                let converted = tint.eval(components);
                alternate.to_rgb(&converted)
            }
            ColorSpace::Lab { range } => {
                // Raw, not `at`: these components are not in 0..1, and
                // clamping them there is precisely the bug this variant fixes.
                let raw = |i: usize| components.get(i).copied().unwrap_or(0.0);
                let l = raw(0).clamp(0.0, 100.0);
                let a = raw(1).clamp(range[0], range[1]);
                let b = raw(2).clamp(range[2], range[3]);
                lab_to_rgb(l, a, b)
            }
            ColorSpace::Approximated { components: n } => match n {
                1 => ColorSpace::DeviceGray.to_rgb(components),
                4 => ColorSpace::DeviceCmyk.to_rgb(components),
                // Three components, or anything unexpected, read as RGB.
                _ => ColorSpace::DeviceRgb.to_rgb(components),
            },
            // A coloured pattern's colour comes from the pattern, not from
            // here. An uncoloured one's does come from here: 8.7.3.2 puts the
            // operands in the underlying space, and they are the only colour a
            // `PaintType 2` pattern will ever have.
            ColorSpace::Pattern { base } => match base {
                Some(base) => base.to_rgb(components),
                None => (0, 0, 0),
            },
        }
    }
}

fn byte(value: f64) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// CIE L*a*b* to sRGB, through XYZ (8.6.5.4).
///
/// The white point is D50, which is what PDF's `/WhitePoint` defaults to and
/// what almost every file that uses Lab declares. A document with a different
/// one is converted slightly wrongly rather than not at all — visibly closer
/// than the alternative, which was rendering the whole space black.
fn lab_to_rgb(l: f64, a: f64, b: f64) -> (u8, u8, u8) {
    // D50, normalized so Y is 1.
    const WHITE: [f64; 3] = [0.964_212, 1.0, 0.825_188];

    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;

    // The inverse of the piecewise cube root: linear near zero, so the
    // gradient stays finite where a plain cube would flatten.
    let finv = |t: f64| -> f64 {
        const DELTA: f64 = 6.0 / 29.0;
        if t > DELTA {
            t * t * t
        } else {
            3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
        }
    };

    let x = WHITE[0] * finv(fx);
    let y = WHITE[1] * finv(fy);
    let z = WHITE[2] * finv(fz);

    // XYZ (D50) to linear sRGB, Bradford-adapted.
    let r = 3.134_136 * x - 1.617_036 * y - 0.490_662 * z;
    let g = -0.978_755 * x + 1.916_143 * y + 0.033_454 * z;
    let bl = 0.071_95 * x - 0.228_988 * y + 1.405_386 * z;

    let encode = |v: f64| -> u8 {
        let v = v.clamp(0.0, 1.0);
        // The sRGB transfer function, linear near zero for the same reason.
        let s = if v <= 0.003_130_8 {
            12.92 * v
        } else {
            1.055 * math::pow(v, 1.0 / 2.4) - 0.055
        };
        byte(s)
    };

    (encode(r), encode(g), encode(bl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_spaces_convert_as_the_specification_says() {
        assert_eq!(ColorSpace::DeviceGray.to_rgb(&[0.0]), (0, 0, 0));
        assert_eq!(ColorSpace::DeviceGray.to_rgb(&[1.0]), (255, 255, 255));
        assert_eq!(ColorSpace::DeviceRgb.to_rgb(&[1.0, 0.0, 0.0]), (255, 0, 0));

        // CMYK: no ink is white, full black ink is black.
        assert_eq!(
            ColorSpace::DeviceCmyk.to_rgb(&[0.0, 0.0, 0.0, 0.0]),
            (255, 255, 255)
        );
        assert_eq!(
            ColorSpace::DeviceCmyk.to_rgb(&[0.0, 0.0, 0.0, 1.0]),
            (0, 0, 0)
        );
        assert_eq!(
            ColorSpace::DeviceCmyk.to_rgb(&[1.0, 0.0, 0.0, 0.0]),
            (0, 255, 255),
            "cyan"
        );
    }

    #[test]
    fn cmyk_starts_black_and_rgb_starts_black() {
        assert_eq!(ColorSpace::DeviceCmyk.initial(), vec![0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            ColorSpace::DeviceCmyk.to_rgb(&ColorSpace::DeviceCmyk.initial()),
            (0, 0, 0)
        );
        assert_eq!(
            ColorSpace::DeviceRgb.to_rgb(&ColorSpace::DeviceRgb.initial()),
            (0, 0, 0)
        );
    }

    /// 8.7.3: a plain `/Pattern` space has no colour, and 8.7.3.2's
    /// `[/Pattern base]` has exactly one — `base`'s reading of the operands
    /// that precede the pattern name, which is what a `PaintType 2` pattern
    /// paints with. Answering black for both makes every uncoloured pattern
    /// black whatever the page asked for.
    #[test]
    fn an_uncoloured_pattern_space_reads_its_underlying_space() {
        let bare = ColorSpace::Pattern { base: None };
        assert_eq!(bare.to_rgb(&[1.0, 0.0, 0.0]), (0, 0, 0));
        assert_eq!(bare.components(), 1);

        let over_rgb = ColorSpace::Pattern {
            base: Some(Box::new(ColorSpace::DeviceRgb)),
        };
        assert_eq!(over_rgb.to_rgb(&[1.0, 0.0, 0.0]), (255, 0, 0));
        assert_eq!(over_rgb.components(), 3);

        // Not merely "three components": a CMYK underlying space reads the
        // same operand count differently, and full black ink is black where
        // three zeros in RGB are also black but one ink in CMYK is not.
        let over_cmyk = ColorSpace::Pattern {
            base: Some(Box::new(ColorSpace::DeviceCmyk)),
        };
        assert_eq!(over_cmyk.components(), 4);
        assert_eq!(over_cmyk.to_rgb(&[1.0, 0.0, 0.0, 0.0]), (0, 255, 255));
    }

    #[test]
    fn an_indexed_space_reads_its_palette() {
        let space = ColorSpace::Indexed {
            base: Box::new(ColorSpace::DeviceRgb),
            lookup: vec![255, 0, 0, 0, 255, 0, 0, 0, 255],
            high: 2,
        };
        assert_eq!(space.to_rgb(&[0.0]), (255, 0, 0));
        assert_eq!(space.to_rgb(&[1.0]), (0, 255, 0));
        assert_eq!(space.to_rgb(&[2.0]), (0, 0, 255));
        // Beyond the palette clamps rather than reading past it.
        assert_eq!(space.to_rgb(&[99.0]), (0, 0, 255));
        assert_eq!(space.to_rgb(&[-5.0]), (255, 0, 0));
        assert_eq!(space.components(), 1);
    }

    #[test]
    fn a_separation_runs_its_tint_transform() {
        // A single ink that maps to grey through a linear function.
        let space = ColorSpace::Separation {
            components: 1,
            alternate: Box::new(ColorSpace::DeviceGray),
            tint: Box::new(Function::Exponential {
                domain: (0.0, 1.0),
                c0: vec![1.0],
                c1: vec![0.0],
                n: 1.0,
            }),
        };
        assert_eq!(space.to_rgb(&[0.0]), (255, 255, 255), "no ink is white");
        assert_eq!(space.to_rgb(&[1.0]), (0, 0, 0), "full ink is black");
    }

    #[test]
    fn an_approximated_space_reads_by_its_component_count() {
        assert_eq!(
            ColorSpace::Approximated { components: 1 }.to_rgb(&[0.5]),
            ColorSpace::DeviceGray.to_rgb(&[0.5])
        );
        assert_eq!(
            ColorSpace::Approximated { components: 4 }.to_rgb(&[0.0, 0.0, 0.0, 1.0]),
            (0, 0, 0)
        );
    }

    #[test]
    fn nonsense_components_clamp_rather_than_wrap() {
        assert_eq!(ColorSpace::DeviceGray.to_rgb(&[5.0]), (255, 255, 255));
        assert_eq!(ColorSpace::DeviceGray.to_rgb(&[-5.0]), (0, 0, 0));
        assert_eq!(ColorSpace::DeviceGray.to_rgb(&[f64::NAN]), (0, 0, 0));
        // Too few components read as zero rather than panicking.
        assert_eq!(ColorSpace::DeviceRgb.to_rgb(&[]), (0, 0, 0));
        assert_eq!(ColorSpace::DeviceCmyk.to_rgb(&[0.5]), (128, 255, 255));
    }
}
