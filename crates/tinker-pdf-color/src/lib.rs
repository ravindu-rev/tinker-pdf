//! Colour spaces and the PDF function interpreter (8.6, 7.10).
//!
//! Feature documentation: `docs/features/rendering.md`.
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
pub mod icc;

pub use function::Function;

/// `value` held between `a` and `b`, in whichever order they come.
///
/// `f64::clamp` panics when its minimum is above its maximum or either is
/// NaN, and a `/Lab` `/Range` is the document's: `[10 -10 -100 100]` is
/// four numbers a file may write, and it panicked here until October 2026
/// (ruling 1). An unordered pair is read as the range it spans, as
/// `function.rs`'s `clamp` reads a function's; a pair with no order at all
/// — both NaN — holds nothing.
fn within(value: f64, a: f64, b: f64) -> f64 {
    let (low, high) = (a.min(b), a.max(b));
    if low <= high {
        value.clamp(low, high)
    } else {
        value
    }
}

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
        /// The colorants the space names and a `/DeviceN`'s attributes: what
        /// a device that composites colorants reads, and nothing a conversion
        /// to light reads, so two spaces differing only here convert alike
        /// and compare unequal.
        colorants: Colorants,
    },
    /// An ICC space whose profile was read, with the transforms it compiles
    /// to, one for each rendering intent.
    ///
    /// The profile's own rendering rather than 8.6.5.5's alternate-space
    /// approximation. `Arc` because a transform carries three 4 096-entry
    /// tables and a page may name the same space at every one of a thousand
    /// `cs` operators; compiling it once per resource dictionary and sharing it
    /// is the difference between reading a profile and reading it repeatedly.
    Icc {
        /// The compiled transforms.
        transforms: std::sync::Arc<icc::Transforms>,
        /// How many components the space takes, which is `/N` and is what
        /// every caller that sizes a buffer asks for.
        components: usize,
    },
    /// A CIE-based or ICC space, approximated by its component count.
    ///
    /// 8.6.5.5 lets a reader use the alternate space, and that is what this
    /// is: the shape of the data without the profile's exact rendering. Still
    /// the answer for a CIE space, and for a profile [`icc::Profile::parse`]
    /// refused — a profile that cannot be read leaves the page exactly as it
    /// was before profiles were read at all.
    Approximated {
        /// How many components.
        components: usize,
    },
    /// **CIE-based grey (8.6.5.2).**
    ///
    /// One component through a gamma, scaled by a white point. Aliased to
    /// [`ColorSpace::DeviceGray`] until now, which meant `/WhitePoint` and
    /// `/Gamma` were never read and *nothing said so* — unlike an ICC profile
    /// this build refuses, where [`ColorSpace::Approximated`] records the
    /// approximation on the type.
    CalGray {
        /// `/WhitePoint`, the diffuse white the components are relative to.
        white: [f64; 3],
        /// `/Gamma`, defaulting to 1.
        gamma: f64,
    },
    /// **CIE-based RGB (8.6.5.3).**
    ///
    /// Three components, each through its own gamma, then a 3×3 matrix into
    /// XYZ relative to `/WhitePoint`.
    CalRgb {
        /// `/WhitePoint`.
        white: [f64; 3],
        /// `/Gamma`, defaulting to `[1, 1, 1]`.
        gamma: [f64; 3],
        /// `/Matrix`, column-major as Table 64 writes it —
        /// `[XA YA ZA XB YB ZB XC YC ZC]` — defaulting to the identity.
        matrix: [f64; 9],
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

/// One component of a `/Separation` or `/DeviceN` space, by what its name
/// says (8.6.6.4, 8.6.6.5). The name's bytes, `#` escapes decoded, never a
/// PDF object (ruling 8).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Colorant {
    /// A colorant by its name: one of the four 8.6.6.4 reserves for a CMYK
    /// device's process colorants — `Cyan`, `Magenta`, `Yellow`, `Black` — or
    /// a spot. A `/DeviceN` naming `All`, which 8.6.6.5 forbids it, names a
    /// colorant of that name here, told apart from [`Colorant::All`].
    Named(Vec<u8>),
    /// A `/Separation`'s `/All`: every colorant of the device at once.
    All,
    /// `/None`: a component that never paints.
    None,
}

/// What a `/Separation` or `/DeviceN` space says about its colorants (8.6.6.4,
/// 8.6.6.5, Tables 70 and 71): the name of each component, and a `/DeviceN`'s
/// attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Colorants {
    /// Each component's colorant, in the space's order: the one a
    /// `/Separation` names, or each of a `/DeviceN`'s names. Fewer than the
    /// space has components where its names could not all be read, which a
    /// device compositing colorants reads as a space that reverts to its
    /// alternate.
    pub names: Vec<Colorant>,
    /// Whether a `/DeviceN`'s attributes say `/Subtype /NChannel` (PDF 1.6):
    /// each component is evaluated by itself, and `process` names the process
    /// components.
    pub n_channel: bool,
    /// `/Colorants`: each colorant's own `/Separation` space, which describes
    /// that colorant alone (8.6.6.5), by the name its key gives, in the
    /// dictionary's order. An entry that is no `/Separation` is not kept.
    pub separations: Vec<(Vec<u8>, ColorSpace)>,
    /// `/Process`: the process space the space's process components are of,
    /// and their names.
    pub process: Option<Process>,
}

/// A `/DeviceN` space's process dictionary (Table 71).
#[derive(Clone, Debug, PartialEq)]
pub struct Process {
    /// `/ColorSpace`: the process space.
    pub space: Box<ColorSpace>,
    /// `/Components`: a name for each of the process space's components, in
    /// its order, as the `/DeviceN`'s names array spells them.
    pub components: Vec<Vec<u8>>,
}

impl Colorants {
    /// One colorant, as a `/Separation` names it, and no attributes.
    #[must_use]
    pub fn single(colorant: Colorant) -> Colorants {
        Colorants {
            names: vec![colorant],
            ..Colorants::default()
        }
    }

    /// The process component `name` is under `process` — its index in
    /// `/Components` — where the space is an `NChannel` one with a
    /// `/Process`: 8.6.6.5 has a plain `/DeviceN` support only what it did
    /// before PDF 1.6, so a plain one's names are never process components
    /// here. The four names 8.6.6.4 reserves are their reader's to tell.
    #[must_use]
    pub fn process_component(&self, name: &[u8]) -> Option<usize> {
        if !self.n_channel {
            return None;
        }
        self.process
            .as_ref()?
            .components
            .iter()
            .position(|component| component == name)
    }

    /// The `/Separation` space `/Colorants` gives the colorant `name`.
    #[must_use]
    pub fn separation(&self, name: &[u8]) -> Option<&ColorSpace> {
        self.separations
            .iter()
            .find(|(colorant, _)| colorant == name)
            .map(|(_, space)| space)
    }
}

impl ColorSpace {
    /// The colorants a `/Separation` or `/DeviceN` space names, and `None`
    /// for every other space — an `/Indexed` one's are its base's.
    #[must_use]
    pub fn colorants(&self) -> Option<&Colorants> {
        match self {
            ColorSpace::Separation { colorants, .. } => Some(colorants),
            _ => None,
        }
    }

    /// How many components a colour in this space has.
    #[must_use]
    pub fn components(&self) -> usize {
        match self {
            ColorSpace::DeviceGray => 1,
            ColorSpace::DeviceRgb => 3,
            ColorSpace::DeviceCmyk => 4,
            ColorSpace::Indexed { .. } => 1,
            ColorSpace::Separation { components, .. } => *components,
            ColorSpace::Icc { components, .. } => *components,
            ColorSpace::Approximated { components } => *components,
            ColorSpace::Lab { .. } => 3,
            ColorSpace::CalGray { .. } => 1,
            ColorSpace::CalRgb { .. } => 3,
            // 8.7.3.2: an uncoloured pattern's operands are counted in the
            // underlying space. A plain `/Pattern` takes none at all, and 1 is
            // the answer that keeps callers which size a buffer from this from
            // sizing it to nothing.
            ColorSpace::Pattern { base } => base.as_ref().map_or(1, |b| b.components()),
        }
    }

    /// The colour this space's initial value is (8.6.8), which each space's
    /// own clause gives.
    #[must_use]
    pub fn initial(&self) -> Vec<f64> {
        match self {
            // Black in every device space, which for CMYK means all zeros
            // except the black ink.
            ColorSpace::DeviceCmyk => vec![0.0, 0.0, 0.0, 1.0],
            // 8.6.6.4 and 8.6.6.5: a `/Separation`'s initial tint is 1.0 and
            // a `/DeviceN`'s is 1.0 in every component — the whole colorant,
            // where zeros would be none of it.
            ColorSpace::Separation { components, .. } => vec![1.0; *components],
            // 8.6.8: an ICCBased space's initial colour is all zeros, whatever
            // the profile makes of them — which for a subtractive profile is
            // white rather than black, and is what the clause says.
            ColorSpace::Icc { components, .. } => vec![0.0; *components],
            // 8.6.5.4: all three zero, unless a component's range leaves zero
            // out, when it is the nearest value the range allows. Table 65
            // does not make `/Range` straddle zero, so `a*` and `b*` are each
            // held to theirs, read as `to_rgb` reads them.
            ColorSpace::Lab { range } => vec![
                0.0,
                within(0.0, range[0], range[1]),
                within(0.0, range[2], range[3]),
            ],
            other => vec![0.0; other.components()],
        }
    }

    /// Converts components to 8-bit sRGB, an ICC colour in relative
    /// colorimetric, 8.6.5.8's initial intent.
    ///
    /// Out-of-range components clamp rather than wrap: a content stream may
    /// name any number, and a wrapped one turns black into white.
    #[must_use]
    pub fn to_rgb(&self, components: &[f64]) -> (u8, u8, u8) {
        self.to_rgb_in(components, icc::Conversion::default())
    }

    /// [`ColorSpace::to_rgb`], an ICC colour converted as `conversion` says.
    ///
    /// An ICC space converts through its transform for the conversion's
    /// intent. A space that holds one — an `/Indexed` base, a `/Separation`
    /// or `/DeviceN` alternate, an uncoloured pattern's underlying space —
    /// passes the conversion to it; every other space converts as `to_rgb`
    /// converts it, since no rendering intent changes a device colour or a
    /// CIE-based space's closed form (`docs/design/icc.md`, "Where each
    /// conversion takes its intent").
    #[must_use]
    pub fn to_rgb_in(&self, components: &[f64], conversion: icc::Conversion) -> (u8, u8, u8) {
        let at = |i: usize| components.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);

        match self {
            ColorSpace::DeviceGray => {
                let v = byte(at(0));
                (v, v, v)
            }
            ColorSpace::DeviceRgb => (byte(at(0)), byte(at(1)), byte(at(2))),
            ColorSpace::DeviceCmyk => {
                let [r, g, b] = cmyk_light(at(0), at(1), at(2), at(3)).map(byte);
                (r, g, b)
            }
            ColorSpace::Indexed { base, lookup, high } => {
                base.to_rgb_in(&palette_entry(base, lookup, *high, components), conversion)
            }
            ColorSpace::Separation {
                alternate, tint, ..
            } => {
                let converted = tint.eval(components);
                alternate.to_rgb_in(&converted, conversion)
            }
            ColorSpace::CalGray { white, gamma } => {
                xyz_to_rgb(cal_gray_xyz(*white, *gamma, at(0)), *white)
            }
            ColorSpace::CalRgb {
                white,
                gamma,
                matrix,
            } => xyz_to_rgb(cal_rgb_xyz(*gamma, matrix, [at(0), at(1), at(2)]), *white),
            ColorSpace::Lab { range } => {
                let [l, a, b] = lab_components(range, components);
                lab_to_rgb(l, a, b)
            }
            // The profile's own transform, which is what this whole module
            // exists to make possible.
            ColorSpace::Icc { transforms, .. } => {
                transforms.get(conversion.intent).apply(components)
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
                Some(base) => base.to_rgb_in(components, conversion),
                None => (0, 0, 0),
            },
        }
    }

    /// [`ColorSpace::to_rgb`] for a caller converting once a pixel, the same
    /// colour: a `/Separation` or `/DeviceN` space evaluates its tint
    /// transform into `scratch`, which the caller keeps, rather than into a
    /// buffer of its own.
    #[must_use]
    pub fn to_rgb_with(&self, components: &[f64], scratch: &mut Vec<f64>) -> (u8, u8, u8) {
        self.to_rgb_with_in(components, scratch, icc::Conversion::default())
    }

    /// [`ColorSpace::to_rgb_with`], an ICC colour converted as `conversion`
    /// says, as [`ColorSpace::to_rgb_in`] converts it.
    #[must_use]
    pub fn to_rgb_with_in(
        &self,
        components: &[f64],
        scratch: &mut Vec<f64>,
        conversion: icc::Conversion,
    ) -> (u8, u8, u8) {
        match self {
            ColorSpace::Separation {
                alternate, tint, ..
            } => {
                tint.eval_into(components, scratch);
                alternate.to_rgb_in(scratch, conversion)
            }
            other => other.to_rgb_in(components, conversion),
        }
    }

    /// [`ColorSpace::to_rgb`] at sixteen bits a channel, an ICC colour in
    /// relative colorimetric as `to_rgb` converts it.
    #[must_use]
    pub fn to_rgb16(&self, components: &[f64]) -> [u16; 3] {
        self.to_rgb16_in(components, icc::Conversion::default())
    }

    /// [`ColorSpace::to_rgb_in`] at sixteen bits a channel
    /// (`docs/design/sixteen-bit.md`, "Resolved colour").
    ///
    /// The components are evaluated exactly as `to_rgb_in` evaluates them —
    /// the device relations, the CIE-based closed forms, `/Lab`, an
    /// `/Indexed` entry through its base, a `/Separation` or `/DeviceN`
    /// colour through its alternate, an ICC colour through its transform —
    /// and each channel is quantised twice: to the byte `to_rgb_in` gives
    /// it, and to sixteen bits held to that byte (the design's `q16`), the
    /// relation ruling 23 settles. The top byte of each channel is
    /// `to_rgb_in`'s byte, bit for bit, and the low byte refines it. An ICC
    /// colour is its transform's one evaluation
    /// ([`icc::Transform::apply16`]), never a second transform.
    #[must_use]
    pub fn to_rgb16_in(&self, components: &[f64], conversion: icc::Conversion) -> [u16; 3] {
        let at = |i: usize| components.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);

        match self {
            ColorSpace::DeviceGray => [sample16(at(0)); 3],
            ColorSpace::DeviceRgb => [sample16(at(0)), sample16(at(1)), sample16(at(2))],
            ColorSpace::DeviceCmyk => cmyk_light(at(0), at(1), at(2), at(3)).map(sample16),
            ColorSpace::Indexed { base, lookup, high } => {
                base.to_rgb16_in(&palette_entry(base, lookup, *high, components), conversion)
            }
            ColorSpace::Separation {
                alternate, tint, ..
            } => {
                let converted = tint.eval(components);
                alternate.to_rgb16_in(&converted, conversion)
            }
            ColorSpace::CalGray { white, gamma } => {
                xyz_to_rgb16(cal_gray_xyz(*white, *gamma, at(0)), *white)
            }
            ColorSpace::CalRgb {
                white,
                gamma,
                matrix,
            } => xyz_to_rgb16(cal_rgb_xyz(*gamma, matrix, [at(0), at(1), at(2)]), *white),
            ColorSpace::Lab { range } => {
                let [l, a, b] = lab_components(range, components);
                lab_to_rgb16(l, a, b)
            }
            ColorSpace::Icc { transforms, .. } => {
                transforms.get(conversion.intent).apply16(components)
            }
            ColorSpace::Approximated { components: n } => match n {
                1 => ColorSpace::DeviceGray.to_rgb16(components),
                4 => ColorSpace::DeviceCmyk.to_rgb16(components),
                _ => ColorSpace::DeviceRgb.to_rgb16(components),
            },
            ColorSpace::Pattern { base } => match base {
                Some(base) => base.to_rgb16_in(components, conversion),
                None => [0; 3],
            },
        }
    }
}

/// 8.6.4.4's relation: each ink's additive complement, with black applied,
/// the components already in `0..=1`.
fn cmyk_light(c: f64, m: f64, y: f64, k: f64) -> [f64; 3] {
    [
        (1.0 - c) * (1.0 - k),
        (1.0 - m) * (1.0 - k),
        (1.0 - y) * (1.0 - k),
    ]
}

/// The base-space components an `/Indexed` colour names (8.6.6.3): the
/// entry its index rounds to, held to `0..=high`, each byte over 255, and
/// zero past the end of `lookup`.
fn palette_entry(base: &ColorSpace, lookup: &[u8], high: u32, components: &[f64]) -> Vec<f64> {
    let index = components
        .first()
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, f64::from(high))
        .round() as usize;
    let n = base.components();
    let start = index.saturating_mul(n);
    (0..n)
        .map(|i| lookup.get(start + i).map_or(0.0, |&b| f64::from(b) / 255.0))
        .collect()
}

/// A `CalGray` component as XYZ (8.6.5.2): `A^G` scales the white point. `a`
/// is already in `0..=1`, which is this space's own range.
fn cal_gray_xyz(white: [f64; 3], gamma: f64, a: f64) -> [f64; 3] {
    let a = math::pow(a, gamma);
    [white[0] * a, white[1] * a, white[2] * a]
}

/// `CalRGB` components as XYZ (8.6.5.3): each component through its own
/// gamma, then Table 64's matrix — which is written column by column, so the
/// first three numbers are the *A* column and not the X row. The components
/// are already in `0..=1`.
fn cal_rgb_xyz(gamma: [f64; 3], matrix: &[f64; 9], abc: [f64; 3]) -> [f64; 3] {
    let a = math::pow(abc[0], gamma[0]);
    let b = math::pow(abc[1], gamma[1]);
    let c = math::pow(abc[2], gamma[2]);
    [
        matrix[0] * a + matrix[3] * b + matrix[6] * c,
        matrix[1] * a + matrix[4] * b + matrix[7] * c,
        matrix[2] * a + matrix[5] * b + matrix[8] * c,
    ]
}

/// A `/Lab` colour's `L*`, `a*` and `b*` as the space reads them: raw, not in
/// 0..1 — clamping them there is precisely the bug [`ColorSpace::Lab`]
/// fixes — `L*` held to 0..100 and `a*` and `b*` to `range`, in whichever
/// order its pairs come.
fn lab_components(range: &[f64; 4], components: &[f64]) -> [f64; 3] {
    let raw = |i: usize| components.get(i).copied().unwrap_or(0.0);
    [
        raw(0).clamp(0.0, 100.0),
        within(raw(1), range[0], range[1]),
        within(raw(2), range[2], range[3]),
    ]
}

fn byte(value: f64) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A value in `0..=1` at sixteen bits, held to `byte`, the byte the 8-bit
/// arithmetic rounds it to: `docs/design/sixteen-bit.md`'s `q16`, the
/// relation ruling 23 settles.
///
/// `round(65 535·value)` clamped into `256·byte ..= 256·byte + 255`, so the
/// top byte is `byte` exactly, and the low byte is the value's own 16-bit
/// rounding wherever that falls inside the interval and the interval's
/// nearer end where it does not. A value that is no number is nought before
/// it is held.
pub(crate) fn held16(value: f64, byte: u8) -> u16 {
    let wide = if value.is_finite() {
        (value.clamp(0.0, 1.0) * 65_535.0).round() as u16
    } else {
        0
    };
    let low = u16::from(byte) << 8;
    wide.clamp(low, low | 0xff)
}

/// A component in `0..=1` at sixteen bits, held to the byte [`byte`] rounds
/// it to: a device relation's quantisation at sixteen bits.
fn sample16(value: f64) -> u16 {
    held16(value, byte(value))
}

/// XYZ at D50 to linear sRGB, Bradford-adapted.
///
/// ICC.1 puts the profile connection space at D50 and sRGB is defined at D65,
/// so the chromatic adaptation is part of this relation rather than a step
/// beside it.
///
/// One constant with two callers: `/Lab` conversion (8.6.5.4) reaches the
/// connection space through its own arithmetic and an ICC transform reaches it
/// through a profile's columns, but from there both are doing the same thing.
/// It was written out twice before an ICC transform existed to want it, and the
/// two copies had already drifted in the fourth decimal.
pub(crate) const XYZ_D50_TO_SRGB: [[f64; 3]; 3] = [
    [3.134_136, -1.617_036, -0.490_662],
    [-0.978_755, 1.916_143, 0.033_454],
    [0.071_95, -0.228_988, 1.405_386],
];

/// The inverse of [`XYZ_D50_TO_SRGB`], for the one direction that needs it.
///
/// **Computed from the matrix above rather than quoted beside it.** The values
/// here were produced by inverting those, and the round trip is asserted in
/// this module's tests — because two matrices that are meant to be inverses
/// and are typed out independently are exactly the pair this file has already
/// watched drift once.
pub(crate) const SRGB_TO_XYZ_D50: [[f64; 3]; 3] = [
    [0.436_035_143, 0.385_067_848, 0.143_066_613],
    [0.222_481_132, 0.716_877_077, 0.060_610_132],
    [0.013_926_979, 0.097_091_202, 0.714_099_436],
];

/// [`XYZ_D50_TO_SRGB`], applied.
pub(crate) fn xyz_d50_to_linear_srgb(x: f64, y: f64, z: f64) -> [f64; 3] {
    let row = |r: usize| {
        XYZ_D50_TO_SRGB[r][0] * x + XYZ_D50_TO_SRGB[r][1] * y + XYZ_D50_TO_SRGB[r][2] * z
    };
    [row(0), row(1), row(2)]
}

/// XYZ relative to `white`, as sRGB.
///
/// **The adaptation is von Kries in XYZ, and saying which one it is matters.**
/// The matrix below takes XYZ relative to *D50*; a CIE-based space names its
/// own white point and D65 is as common as D50 in the wild, so the two have to
/// be reconciled. Scaling each axis by the ratio of the two whites is the
/// simplest transform that maps one white exactly onto the other, and it is
/// what 8.6.5.3's own note describes when it says the components are relative
/// to the diffuse white.
///
/// It is not Bradford, which is what [`XYZ_D50_TO_SRGB`] already has baked in
/// for the *profile* path, and the difference shows on saturated colours far
/// from the neutral axis. Stated rather than hidden: a CIE-based space is a
/// space a producer chose over an ICC profile, and this is the accuracy that
/// choice buys.
fn xyz_to_rgb(xyz: [f64; 3], white: [f64; 3]) -> (u8, u8, u8) {
    let [r, g, b] = xyz_to_linear_srgb(xyz, white);
    (srgb_encode(r), srgb_encode(g), srgb_encode(b))
}

/// [`xyz_to_rgb`] at sixteen bits a channel: the same light, each channel
/// held to the byte `xyz_to_rgb` encodes it as ([`srgb_encode16`]).
fn xyz_to_rgb16(xyz: [f64; 3], white: [f64; 3]) -> [u16; 3] {
    xyz_to_linear_srgb(xyz, white).map(srgb_encode16)
}

/// XYZ relative to `white` as linear sRGB, before it is encoded: the light
/// [`xyz_to_rgb`] encodes, adapted to D50 von Kries' way.
fn xyz_to_linear_srgb(xyz: [f64; 3], white: [f64; 3]) -> [f64; 3] {
    const D50: [f64; 3] = [0.964_212, 1.0, 0.825_188];
    let scale = |v: f64, from: f64, to: f64| if from > 0.0 { v * to / from } else { v };
    let adapted = [
        scale(xyz[0], white[0], D50[0]),
        scale(xyz[1], white[1], D50[1]),
        scale(xyz[2], white[2], D50[2]),
    ];
    xyz_d50_to_linear_srgb(adapted[0], adapted[1], adapted[2])
}

/// One linear-light channel as an sRGB byte (IEC 61966-2-1).
pub(crate) fn srgb_encode(v: f64) -> u8 {
    byte(srgb_transfer(v))
}

/// [`srgb_encode`] at sixteen bits: the same transfer, held to the byte
/// `srgb_encode` rounds it to.
fn srgb_encode16(v: f64) -> u16 {
    let encoded = srgb_transfer(v);
    held16(encoded, byte(encoded))
}

/// IEC 61966-2-1's transfer function: one linear-light channel, clamped to
/// `0..=1`, as its encoded value in `0..=1`, before it is rounded to any
/// width.
pub(crate) fn srgb_transfer(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    // Linear near zero, so the gradient stays finite where a plain power
    // would flatten.
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * math::pow(v, 1.0 / 2.4) - 0.055
    }
}

/// **An sRGB triple as CIE L*a*b*** (8.6.5.4), relative to D50.
///
/// The inverse of [`lab_to_srgb`], and the direction nothing in this engine
/// needed until a transparency group asked to composite in `/Lab`: a group
/// buffer holds the space the group declared, so something has to put a colour
/// *into* Lab as well as read one out.
///
/// `L` runs 0..100 and `a`/`b` roughly -128..127, which is why a Lab group
/// cannot use the 0..1 buffers every other space does.
#[must_use]
pub fn srgb_to_lab(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let encoded = |v: u8| f64::from(v) / 255.0;
    encoded_srgb_to_lab([encoded(r), encoded(g), encoded(b)])
}

/// [`srgb_to_lab`] for an sRGB colour at sixteen bits a channel: the same
/// evaluation, each sample over 65 535 where `srgb_to_lab`'s is over 255.
///
/// A byte widened to sixteen bits, `257·v`, is the same value over 65 535 as
/// `v` over 255, and each division is correctly rounded, so a widened colour
/// reads as its bytes do, exactly; the low bytes of any other colour move it
/// between them (`docs/design/sixteen-bit.md`, "Resolved colour").
#[must_use]
pub fn srgb_to_lab16(r: u16, g: u16, b: u16) -> (f64, f64, f64) {
    let encoded = |v: u16| f64::from(v) / 65_535.0;
    encoded_srgb_to_lab([encoded(r), encoded(g), encoded(b)])
}

/// An sRGB colour, each channel its encoded value in `0..=1`, as CIE
/// L*a*b* relative to D50: what [`srgb_to_lab`] and [`srgb_to_lab16`] each
/// evaluate.
fn encoded_srgb_to_lab(encoded: [f64; 3]) -> (f64, f64, f64) {
    const WHITE: [f64; 3] = [0.964_212, 1.0, 0.825_188];
    let linear = |v: f64| -> f64 {
        // The inverse of the sRGB transfer function, linear near zero.
        if v <= 0.040_45 {
            v / 12.92
        } else {
            math::pow((v + 0.055) / 1.055, 2.4)
        }
    };
    let (lr, lg, lb) = (linear(encoded[0]), linear(encoded[1]), linear(encoded[2]));
    let row = |i: usize| {
        SRGB_TO_XYZ_D50[i][0] * lr + SRGB_TO_XYZ_D50[i][1] * lg + SRGB_TO_XYZ_D50[i][2] * lb
    };
    let (x, y, z) = (row(0), row(1), row(2));

    // The forward piecewise cube root, linear near zero for the reason its
    // inverse is: a plain cube root has an infinite gradient at the origin.
    let f = |t: f64| -> f64 {
        const DELTA: f64 = 6.0 / 29.0;
        if t > DELTA * DELTA * DELTA {
            math::cbrt(t)
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };
    let fx = f(x / WHITE[0]);
    let fy = f(y / WHITE[1]);
    let fz = f(z / WHITE[2]);
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// **A CIE L*a*b* triple as sRGB**, relative to D50.
#[must_use]
pub fn lab_to_srgb(l: f64, a: f64, b: f64) -> (u8, u8, u8) {
    lab_to_rgb(l, a, b)
}

/// [`lab_to_srgb`] at sixteen bits a channel: the same evaluation quantised
/// twice, each channel held to the byte `lab_to_srgb` gives it, so its top
/// byte is that byte and its low byte refines it
/// (`docs/design/sixteen-bit.md`, "Resolved colour").
#[must_use]
pub fn lab_to_srgb16(l: f64, a: f64, b: f64) -> [u16; 3] {
    lab_to_rgb16(l, a, b)
}

/// A CIE L*a*b* triple, relative to D50, as sRGB: [`lab_to_linear_srgb`],
/// encoded.
pub(crate) fn lab_to_rgb(l: f64, a: f64, b: f64) -> (u8, u8, u8) {
    let [r, g, bl] = lab_to_linear_srgb(l, a, b);
    (srgb_encode(r), srgb_encode(g), srgb_encode(bl))
}

/// [`lab_to_rgb`] at sixteen bits a channel: [`lab_to_linear_srgb`],
/// encoded by [`srgb_encode16`].
fn lab_to_rgb16(l: f64, a: f64, b: f64) -> [u16; 3] {
    lab_to_linear_srgb(l, a, b).map(srgb_encode16)
}

/// A CIE L*a*b* triple, relative to D50, as linear sRGB before it is
/// encoded and unclamped: [`lab_to_rgb`]'s light, which an ICC transform's
/// grid holds at its nodes (`docs/design/icc.md`, P-22, "What a node
/// holds"): [`lab_to_xyz`], then [`XYZ_D50_TO_SRGB`].
pub(crate) fn lab_to_linear_srgb(l: f64, a: f64, b: f64) -> [f64; 3] {
    let [x, y, z] = lab_to_xyz(l, a, b);
    xyz_d50_to_linear_srgb(x, y, z)
}

/// A CIE L*a*b* triple as the XYZ it is relative to D50: 8.6.5.4's relation
/// run backwards.
pub(crate) fn lab_to_xyz(l: f64, a: f64, b: f64) -> [f64; 3] {
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

    [
        WHITE[0] * finv(fx),
        WHITE[1] * finv(fy),
        WHITE[2] * finv(fz),
    ]
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

    /// A `/Lab` `/Range` is the document's, and no order or value of it
    /// panics: an unordered pair is the range it spans, the same colour as
    /// the ordered one, and a NaN pair holds nothing.
    #[test]
    fn a_lab_range_in_any_order_converts() {
        let lab = |range: [f64; 4], c: [f64; 3]| ColorSpace::Lab { range }.to_rgb(&c);
        let ordered = lab([-10.0, 10.0, -100.0, 100.0], [50.0, 40.0, 0.0]);
        assert_eq!(
            lab([10.0, -10.0, -100.0, 100.0], [50.0, 40.0, 0.0]),
            ordered
        );
        assert_eq!(
            lab([10.0, -10.0, 100.0, -100.0], [50.0, 40.0, 0.0]),
            ordered
        );
        let _ = lab([f64::NAN, f64::NAN, f64::NAN, 1.0], [50.0, 40.0, 0.0]);
        let _ = ColorSpace::Lab {
            range: [f64::NAN, f64::NAN, 5.0, -5.0],
        }
        .initial();
    }

    /// Each space's own clause gives its initial colour: a spot space's is
    /// the whole colorant, 1.0 in every component (8.6.6.4, 8.6.6.5), and a
    /// `/Lab` one's is zero or, where a `/Range` leaves zero out, the nearest
    /// value it allows (8.6.5.4).
    #[test]
    fn every_space_starts_where_its_clause_says() {
        let spot = |components: usize| ColorSpace::Separation {
            components,
            alternate: Box::new(ColorSpace::DeviceGray),
            tint: Box::new(Function::Exponential {
                domain: (0.0, 1.0),
                c0: vec![1.0],
                c1: vec![0.0],
                n: 1.0,
            }),
            colorants: Colorants::default(),
        };
        assert_eq!(spot(1).initial(), vec![1.0], "/Separation");
        assert_eq!(spot(3).initial(), vec![1.0; 3], "/DeviceN");
        assert_eq!(spot(1).to_rgb(&spot(1).initial()), (0, 0, 0), "full tint");

        let lab = |range: [f64; 4]| ColorSpace::Lab { range }.initial();
        assert_eq!(lab([-100.0, 100.0, -100.0, 100.0]), vec![0.0; 3]);
        assert_eq!(lab([10.0, 20.0, -30.0, -5.0]), vec![0.0, 10.0, -5.0]);
        assert_eq!(
            lab([20.0, 10.0, 0.0, 1.0]),
            vec![0.0, 10.0, 0.0],
            "unordered"
        );
        assert_eq!(
            ColorSpace::Indexed {
                base: Box::new(ColorSpace::DeviceRgb),
                lookup: vec![0; 3],
                high: 0,
            }
            .initial(),
            vec![0.0]
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

    /// **A separation keeps its colorant names** (RC-04.5): a `/Separation`
    /// names one colorant, `/All` and `/None` told apart from a colorant of
    /// either name; a `/DeviceN` names each component, `/None` among them,
    /// with its attributes — `NChannel`, `/Colorants` and `/Process`; the
    /// names reach no conversion to light, so spaces differing only in them
    /// convert alike, and compare unequal, as `PartialEq` tells every field
    /// apart.
    #[test]
    fn a_separation_keeps_its_colorant_names() {
        let tint = || {
            Box::new(Function::Exponential {
                domain: (0.0, 1.0),
                c0: vec![0.0, 0.0, 0.0, 0.0],
                c1: vec![0.0, 0.9, 0.2, 0.0],
                n: 1.0,
            })
        };
        let separation = |colorant: Colorant| ColorSpace::Separation {
            components: 1,
            alternate: Box::new(ColorSpace::DeviceCmyk),
            tint: tint(),
            colorants: Colorants::single(colorant),
        };
        let gold = separation(Colorant::Named(b"PANTONE 131".to_vec()));
        assert_eq!(
            gold.colorants().map(|c| c.names.clone()),
            Some(vec![Colorant::Named(b"PANTONE 131".to_vec())])
        );
        let all = separation(Colorant::All);
        let none = separation(Colorant::None);
        let named_all = separation(Colorant::Named(b"All".to_vec()));
        assert_ne!(all, named_all, "/All is not a colorant called All");
        assert_ne!(none, separation(Colorant::Named(b"None".to_vec())));
        assert_ne!(gold, all, "two names, two spaces");
        for space in [&all, &none, &named_all] {
            for tint in [0.0, 0.3, 1.0] {
                assert_eq!(
                    space.to_rgb(&[tint]),
                    gold.to_rgb(&[tint]),
                    "the names convert nothing"
                );
            }
        }

        let red = ColorSpace::Separation {
            components: 1,
            alternate: Box::new(ColorSpace::DeviceRgb),
            tint: Box::new(Function::Identity),
            colorants: Colorants::single(Colorant::Named(b"Red".to_vec())),
        };
        let device_n = ColorSpace::Separation {
            components: 4,
            alternate: Box::new(ColorSpace::DeviceRgb),
            tint: tint(),
            colorants: Colorants {
                names: vec![
                    Colorant::Named(b"ProcessRed".to_vec()),
                    Colorant::Named(b"ProcessGreen".to_vec()),
                    Colorant::Named(b"ProcessBlue".to_vec()),
                    Colorant::Named(b"Red".to_vec()),
                ],
                n_channel: true,
                separations: vec![(b"Red".to_vec(), red.clone())],
                process: Some(Process {
                    space: Box::new(ColorSpace::DeviceRgb),
                    components: vec![
                        b"ProcessRed".to_vec(),
                        b"ProcessGreen".to_vec(),
                        b"ProcessBlue".to_vec(),
                    ],
                }),
            },
        };
        let colorants = device_n.colorants().expect("a DeviceN's colorants");
        assert_eq!(colorants.names.len(), device_n.components());
        assert_eq!(colorants.process_component(b"ProcessGreen"), Some(1));
        assert_eq!(
            colorants.process_component(b"Red"),
            None,
            "a spot, not a process component"
        );
        assert_eq!(colorants.separation(b"Red"), Some(&red));
        assert_eq!(colorants.separation(b"Blue"), None);
        let plain = Colorants {
            n_channel: false,
            ..colorants.clone()
        };
        assert_eq!(
            plain.process_component(b"ProcessGreen"),
            None,
            "a plain /DeviceN's /Process is not read"
        );
        assert_eq!(ColorSpace::DeviceCmyk.colorants(), None);
        let indexed = ColorSpace::Indexed {
            base: Box::new(device_n.clone()),
            lookup: vec![0; 4],
            high: 0,
        };
        assert_eq!(
            indexed.colorants(),
            None,
            "an index's colorants are its base's"
        );

        let unnamed = ColorSpace::Separation {
            components: 4,
            alternate: Box::new(ColorSpace::DeviceRgb),
            tint: tint(),
            colorants: Colorants::default(),
        };
        assert_ne!(unnamed, device_n);
        for components in [[0.0, 0.0, 0.0, 0.0], [0.2, 0.4, 0.6, 0.8], [1.0; 4]] {
            assert_eq!(unnamed.to_rgb(&components), device_n.to_rgb(&components));
        }
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
            colorants: Colorants::default(),
        };
        assert_eq!(space.to_rgb(&[0.0]), (255, 255, 255), "no ink is white");
        assert_eq!(space.to_rgb(&[1.0]), (0, 0, 0), "full ink is black");
    }

    /// `to_rgb_with` is `to_rgb` whatever its scratch held: a two-ink
    /// `/DeviceN` into CMYK, the tint transform an array of four, converted
    /// over and over through one buffer, and a space with no transform beside
    /// it.
    #[test]
    fn converting_through_a_kept_buffer_is_converting() {
        let ink = |c0: f64, c1: f64| Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![c0],
            c1: vec![c1],
            n: 2.2,
        };
        let space = ColorSpace::Separation {
            components: 2,
            alternate: Box::new(ColorSpace::DeviceCmyk),
            tint: Box::new(Function::Array(vec![
                ink(0.0, 0.9),
                ink(0.1, 0.4),
                ink(0.0, 0.0),
                ink(0.2, 0.7),
            ])),
            colorants: Colorants::default(),
        };
        let mut scratch = vec![0.5; 9];
        for step in 0..=20 {
            let a = f64::from(step) / 20.0;
            let components = [a, 1.0 - a];
            assert_eq!(
                space.to_rgb_with(&components, &mut scratch),
                space.to_rgb(&components),
                "{components:?}"
            );
            assert_eq!(
                ColorSpace::DeviceRgb.to_rgb_with(&[a, 0.5, 1.0 - a], &mut scratch),
                ColorSpace::DeviceRgb.to_rgb(&[a, 0.5, 1.0 - a])
            );
        }
    }

    /// **An ICC space converts in the intent it is asked for**, and so does
    /// every space that holds one: an `/Indexed` palette over it, a
    /// `/Separation` alternate through `to_rgb_in` and `to_rgb_with_in`, and
    /// an uncoloured pattern's underlying space. `to_rgb` is relative
    /// colorimetric, and the intents part, since the profile carries a table
    /// of its own for each.
    #[test]
    fn an_icc_space_converts_in_the_intent_it_is_asked_for() {
        let profile =
            icc::Profile::parse(&icc::tests::three_intents_profile()).expect("three tables");
        let transforms =
            std::sync::Arc::new(icc::Transforms::compile(profile).expect("a transform"));
        let space = ColorSpace::Icc {
            transforms: std::sync::Arc::clone(&transforms),
            components: 3,
        };
        let in_intent = |intent| icc::Conversion { intent };
        let colours = [[0.0, 0.25, 1.0], [0.5, 0.5, 0.5], [0.75, 0.0, 0.25]];
        for components in colours {
            assert_eq!(
                space.to_rgb(&components),
                space.to_rgb_in(&components, icc::Conversion::default()),
                "{components:?}"
            );
            assert_eq!(
                space.to_rgb(&components),
                transforms
                    .get(icc::Intent::RelativeColorimetric)
                    .apply(&components)
            );
            for intent in icc::Intent::ALL {
                assert_eq!(
                    space.to_rgb_in(&components, in_intent(intent)),
                    transforms.get(intent).apply(&components),
                    "{intent:?} at {components:?}"
                );
            }
        }
        assert!(
            colours.iter().any(|c| {
                space.to_rgb_in(c, in_intent(icc::Intent::Perceptual))
                    != space.to_rgb_in(c, in_intent(icc::Intent::Saturation))
            }),
            "the intents convert alike"
        );

        // The spaces that hold one pass the intent to it.
        let palette: Vec<u8> = colours
            .iter()
            .flat_map(|c| c.map(|v| (v * 255.0).round() as u8))
            .collect();
        let indexed = ColorSpace::Indexed {
            base: Box::new(space.clone()),
            lookup: palette,
            high: 2,
        };
        let separation = ColorSpace::Separation {
            components: 3,
            alternate: Box::new(space.clone()),
            tint: Box::new(separation_tint()),
            colorants: Colorants::default(),
        };
        let pattern = ColorSpace::Pattern {
            base: Some(Box::new(space.clone())),
        };
        let mut scratch = Vec::new();
        for intent in icc::Intent::ALL {
            let conversion = in_intent(intent);
            for (index, components) in colours.iter().enumerate() {
                let entry = components.map(|v| f64::from((v * 255.0).round() as u8) / 255.0);
                assert_eq!(
                    indexed.to_rgb_in(&[index as f64], conversion),
                    space.to_rgb_in(&entry, conversion),
                    "indexed, {intent:?}"
                );
                let through_tint = space.to_rgb_in(&separation_tint().eval(components), conversion);
                assert_eq!(
                    separation.to_rgb_in(components, conversion),
                    through_tint,
                    "separation, {intent:?}"
                );
                assert_eq!(
                    separation.to_rgb_with_in(components, &mut scratch, conversion),
                    through_tint,
                    "separation through a kept buffer, {intent:?}"
                );
                assert_eq!(
                    pattern.to_rgb_in(components, conversion),
                    space.to_rgb_in(components, conversion),
                    "pattern, {intent:?}"
                );
            }
        }
    }

    /// The tint transform of
    /// [`an_icc_space_converts_in_the_intent_it_is_asked_for`]'s separation:
    /// its first ink into the profile's three components, the first as it is,
    /// the second over half the range and the third held at a half.
    fn separation_tint() -> Function {
        Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0, 0.25, 0.5],
            c1: vec![1.0, 0.75, 0.5],
            n: 1.0,
        }
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

    /// **The two XYZ matrices are inverses**, which is the only thing standing
    /// between them and the drift this file has already had once.
    ///
    /// They are typed out separately because a matrix inverted at runtime would
    /// put a division on a path ruling 4 wants exact; the assertion is what
    /// makes that safe. A tolerance of 1e-6 is two orders below the precision
    /// either matrix is written to.
    #[test]
    fn the_two_xyz_matrices_are_inverses() {
        for (i, row) in XYZ_D50_TO_SRGB.iter().enumerate() {
            for j in 0..3 {
                let product: f64 = row
                    .iter()
                    .zip(SRGB_TO_XYZ_D50.iter())
                    .map(|(a, b)| a * b[j])
                    .sum();
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (product - expected).abs() < 1e-6,
                    "row {i} column {j} is {product} and should be {expected}"
                );
            }
        }
    }

    /// **Lab round-trips through sRGB**, which is what a group buffer needs:
    /// a colour goes in, is blended, and comes back out.
    #[test]
    fn srgb_survives_a_trip_through_lab() {
        for (r, g, b) in [
            (0, 0, 0),
            (255, 255, 255),
            (128, 128, 128),
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (12, 200, 71),
            (200, 40, 130),
        ] {
            let (l, a, bb) = srgb_to_lab(r, g, b);
            let back = lab_to_srgb(l, a, bb);
            for (before, after) in [(r, back.0), (g, back.1), (b, back.2)] {
                assert!(
                    before.abs_diff(after) <= 1,
                    "{:?} came back as {back:?} through Lab {:?}",
                    (r, g, b),
                    (l, a, bb)
                );
            }
        }
    }

    // ---- sixteen bits a channel (docs/design/sixteen-bit.md) ---------------

    /// `x`, a value in `0..=1`, at sixteen bits by the design's relation,
    /// computed here from its definition: `round(65 535·x)` held to the
    /// interval of `round(255·x)`, the byte the 8-bit relation gives it,
    /// `256·byte ..= 256·byte + 255`.
    fn q16(x: f64) -> u16 {
        let byte = (x * 255.0).round() as u32;
        let wide = (x * 65_535.0).round() as u32;
        wide.clamp(256 * byte, 256 * byte + 255) as u16
    }

    /// The values a component is swept over: every sixty-fourth from nought
    /// to one, a third, and 0.1981 and 0.8019, which the 8-bit and the
    /// 16-bit roundings part on.
    fn component_axis() -> Vec<f64> {
        let mut axis: Vec<f64> = (0..=64u32).map(|i| f64::from(i) / 64.0).collect();
        axis.extend([1.0 / 3.0, 0.1981, 0.8019]);
        axis
    }

    /// **The device spaces at sixteen bits are their relations** (8.6.4,
    /// 8.6.4.4): grey and RGB the components themselves and CMYK each ink's
    /// complement with black applied, each value at sixteen bits by the
    /// design's relation, computed here ([`q16`]). A component on which the
    /// two roundings part is held to its byte — 0.1981 is 51 at eight bits
    /// and 12 982 at sixteen, whose top byte is 50, so it is 13 056,
    /// `256·51` — the ends are exact, and out-of-range, missing and NaN
    /// components read as the bytes read them.
    #[test]
    fn device_spaces_at_sixteen_bits_are_their_relations() {
        assert_eq!(q16(0.1981), 13_056, "the relation holds the parting value");
        assert_eq!(ColorSpace::DeviceGray.to_rgb16(&[0.1981]), [13_056; 3]);
        assert_eq!(ColorSpace::DeviceGray.to_rgb16(&[0.0]), [0; 3]);
        assert_eq!(ColorSpace::DeviceGray.to_rgb16(&[1.0]), [65_535; 3]);

        let axis = component_axis();
        for &v in &axis {
            assert_eq!(
                ColorSpace::DeviceGray.to_rgb16(&[v]),
                [q16(v); 3],
                "grey {v}"
            );
        }
        for &r in &axis {
            for &g in axis.iter().step_by(5) {
                for &b in axis.iter().step_by(7) {
                    assert_eq!(
                        ColorSpace::DeviceRgb.to_rgb16(&[r, g, b]),
                        [q16(r), q16(g), q16(b)],
                        "rgb {r} {g} {b}"
                    );
                }
            }
        }
        for &c in axis.iter().step_by(3) {
            for &m in axis.iter().step_by(7) {
                for &y in axis.iter().step_by(11) {
                    for &k in axis.iter().step_by(4) {
                        let light = |ink: f64| q16((1.0 - ink) * (1.0 - k));
                        assert_eq!(
                            ColorSpace::DeviceCmyk.to_rgb16(&[c, m, y, k]),
                            [light(c), light(m), light(y)],
                            "cmyk {c} {m} {y} {k}"
                        );
                    }
                }
            }
        }

        assert_eq!(ColorSpace::DeviceGray.to_rgb16(&[5.0]), [65_535; 3]);
        assert_eq!(ColorSpace::DeviceGray.to_rgb16(&[-5.0]), [0; 3]);
        assert_eq!(ColorSpace::DeviceGray.to_rgb16(&[f64::NAN]), [0; 3]);
        assert_eq!(ColorSpace::DeviceRgb.to_rgb16(&[]), [0; 3]);
        assert_eq!(
            ColorSpace::DeviceCmyk.to_rgb16(&[0.5]),
            [q16(0.5), 65_535, 65_535]
        );
    }

    /// The top byte of each channel of a 16-bit colour.
    fn top_bytes(wide: [u16; 3]) -> (u8, u8, u8) {
        let [r, g, b] = wide.map(|channel| (channel >> 8) as u8);
        (r, g, b)
    }

    /// Every combination of `axis`'s values over `inputs` components, the
    /// last varying fastest.
    fn combinations(inputs: usize, axis: &[f64]) -> Vec<Vec<f64>> {
        (0..axis.len().pow(inputs as u32))
            .map(|index| {
                let mut x = vec![0.0; inputs];
                let mut rest = index;
                for slot in x.iter_mut().rev() {
                    *slot = axis[rest % axis.len()];
                    rest /= axis.len();
                }
                x
            })
            .collect()
    }

    /// The grid a space of `inputs` components in `0..=1` is swept over:
    /// fewer steps an axis the more components, and 0.1981, between two
    /// 8-bit values, on every axis of four or fewer.
    fn component_grid(inputs: usize) -> Vec<Vec<f64>> {
        let steps: u32 = match inputs {
            0 | 1 => 64,
            2 => 16,
            3 => 8,
            4 => 4,
            _ => 2,
        };
        let mut axis: Vec<f64> = (0..=steps)
            .map(|i| f64::from(i) / f64::from(steps))
            .collect();
        if inputs <= 4 {
            axis.push(0.1981);
        }
        combinations(inputs.max(1), &axis)
    }

    /// **Every space at sixteen bits tops out at its 8-bit colour**
    /// (`docs/design/sixteen-bit.md`, ruling 23). Over a grid of components
    /// per space — the device spaces; `CalGray` and `CalRGB` under a white
    /// other than D50; `/Lab` over its range and past it; `/Indexed` over an
    /// RGB, a CMYK and an ICC palette; a `/Separation` and a `/DeviceN`
    /// through their tint transforms; each `Approximated` count; an
    /// uncoloured pattern's underlying space; and an ICC space over each path
    /// its transform takes — in every intent, the top byte of each channel
    /// `to_rgb16_in` gives is the byte `to_rgb_in` gives, and `to_rgb16` is
    /// `to_rgb16_in` in relative colorimetric. The low bytes refine: in every
    /// space but the RGB palette, whose entries are bytes and so widen
    /// exactly, some colour has a low byte that is not its top byte
    /// repeated, which is all a byte widened to sixteen bits would carry.
    #[test]
    fn every_space_at_sixteen_bits_tops_out_at_its_eight_bit_colour() {
        let d65 = [0.950_47, 1.0, 1.088_83];
        let ink = |c0: f64, c1: f64| Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![c0],
            c1: vec![c1],
            n: 2.2,
        };
        let mut lab_inputs: Vec<Vec<f64>> = Vec::new();
        let axis = [-150.0, -128.0, -64.0, -10.0, 0.0, 10.0, 64.0, 127.0, 150.0];
        for l in [0.0, 12.5, 25.0, 37.5, 50.0, 62.5, 75.0, 87.5, 100.0, 120.0] {
            for a in axis {
                for b in axis {
                    lab_inputs.push(vec![l, a, b]);
                }
            }
        }
        let index_inputs: Vec<Vec<f64>> = [-1.0, 0.0, 0.4, 1.0, 1.6, 2.0, 3.0, 99.0, f64::NAN]
            .iter()
            .map(|&i| vec![i])
            .collect();
        let palette_icc = {
            let profile =
                icc::Profile::parse(&icc::tests::three_intents_profile()).expect("three tables");
            ColorSpace::Icc {
                transforms: std::sync::Arc::new(
                    icc::Transforms::compile(profile).expect("a transform"),
                ),
                components: 3,
            }
        };

        let mut spaces: Vec<(String, ColorSpace, Vec<Vec<f64>>, bool)> = vec![
            (
                "DeviceGray".into(),
                ColorSpace::DeviceGray,
                component_grid(1),
                true,
            ),
            (
                "DeviceRGB".into(),
                ColorSpace::DeviceRgb,
                component_grid(3),
                true,
            ),
            (
                "DeviceCMYK".into(),
                ColorSpace::DeviceCmyk,
                component_grid(4),
                true,
            ),
            (
                "CalGray".into(),
                ColorSpace::CalGray {
                    white: d65,
                    gamma: 2.2,
                },
                component_grid(1),
                true,
            ),
            (
                "CalRGB".into(),
                ColorSpace::CalRgb {
                    white: d65,
                    gamma: [1.8, 2.2, 2.4],
                    matrix: [
                        0.4124, 0.2126, 0.0193, 0.3576, 0.7152, 0.1192, 0.1805, 0.0722, 0.9505,
                    ],
                },
                component_grid(3),
                true,
            ),
            (
                "Lab".into(),
                ColorSpace::Lab {
                    range: [-128.0, 127.0, -100.0, 100.0],
                },
                lab_inputs,
                true,
            ),
            (
                "an RGB palette".into(),
                ColorSpace::Indexed {
                    base: Box::new(ColorSpace::DeviceRgb),
                    lookup: vec![12, 200, 71, 255, 0, 0, 0, 0, 0, 200, 40, 130],
                    high: 3,
                },
                index_inputs.clone(),
                false,
            ),
            (
                "a CMYK palette".into(),
                ColorSpace::Indexed {
                    base: Box::new(ColorSpace::DeviceCmyk),
                    lookup: vec![12, 200, 71, 33, 255, 0, 0, 128, 90, 90, 90, 90],
                    high: 2,
                },
                index_inputs.clone(),
                true,
            ),
            (
                "an ICC palette".into(),
                ColorSpace::Indexed {
                    base: Box::new(palette_icc.clone()),
                    lookup: vec![12, 200, 71, 255, 0, 0, 0, 0, 0, 200, 40, 130],
                    high: 3,
                },
                index_inputs,
                true,
            ),
            (
                "a Separation".into(),
                ColorSpace::Separation {
                    components: 1,
                    alternate: Box::new(ColorSpace::DeviceCmyk),
                    tint: Box::new(Function::Exponential {
                        domain: (0.0, 1.0),
                        c0: vec![0.0, 0.0, 0.0, 0.0],
                        c1: vec![0.1, 0.9, 0.3, 0.05],
                        n: 1.0,
                    }),
                    colorants: Colorants::default(),
                },
                component_grid(1),
                true,
            ),
            (
                "a DeviceN".into(),
                ColorSpace::Separation {
                    components: 2,
                    alternate: Box::new(ColorSpace::DeviceCmyk),
                    tint: Box::new(Function::Array(vec![
                        ink(0.0, 0.9),
                        ink(0.1, 0.4),
                        ink(0.0, 0.0),
                        ink(0.2, 0.7),
                    ])),
                    colorants: Colorants::default(),
                },
                component_grid(2),
                true,
            ),
            (
                "one approximated component".into(),
                ColorSpace::Approximated { components: 1 },
                component_grid(1),
                true,
            ),
            (
                "three approximated components".into(),
                ColorSpace::Approximated { components: 3 },
                component_grid(3),
                true,
            ),
            (
                "four approximated components".into(),
                ColorSpace::Approximated { components: 4 },
                component_grid(4),
                true,
            ),
            (
                "an uncoloured pattern over CMYK".into(),
                ColorSpace::Pattern {
                    base: Some(Box::new(ColorSpace::DeviceCmyk)),
                },
                component_grid(4),
                true,
            ),
            (
                "a bare pattern".into(),
                ColorSpace::Pattern { base: None },
                component_grid(1),
                false,
            ),
        ];
        for (name, bytes) in icc::tests::sixteen_bit_profiles() {
            let profile = icc::Profile::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let transforms =
                icc::Transforms::compile(profile).unwrap_or_else(|| panic!("{name}: no transform"));
            let components = transforms.inputs();
            spaces.push((
                format!("ICC, {name}"),
                ColorSpace::Icc {
                    transforms: std::sync::Arc::new(transforms),
                    components,
                },
                component_grid(components),
                true,
            ));
        }

        for (name, space, inputs, refines) in &spaces {
            let mut refined = false;
            for intent in icc::Intent::ALL {
                let conversion = icc::Conversion { intent };
                for x in inputs {
                    let wide = space.to_rgb16_in(x, conversion);
                    assert_eq!(
                        top_bytes(wide),
                        space.to_rgb_in(x, conversion),
                        "{name}, {intent:?}, at {x:?}"
                    );
                    refined |= wide.iter().any(|&channel| channel & 0xff != channel >> 8);
                }
            }
            for x in inputs {
                assert_eq!(
                    space.to_rgb16(x),
                    space.to_rgb16_in(x, icc::Conversion::default()),
                    "{name} at {x:?}"
                );
            }
            assert_eq!(
                refined, *refines,
                "{name}: whether a low byte refines its top byte"
            );
        }
    }

    /// **The Lab conversions at sixteen bits keep their bytes.**
    /// `lab_to_srgb16` tops out at `lab_to_srgb` over `L*` from 0 to 100 and
    /// `a*` and `b*` from −128 to 127, and some colour's low byte refines its
    /// top byte; `srgb_to_lab16` of a colour whose bytes are widened, `257·v`,
    /// is `srgb_to_lab` of those bytes exactly, and a 16-bit colour between
    /// two 8-bit ones reads between their readings: its lightness between
    /// theirs, for a grey.
    #[test]
    fn the_lab_conversions_at_sixteen_bits_keep_their_bytes() {
        let mut refined = false;
        for l in (0..=20u32).map(|i| f64::from(i) * 5.0) {
            for a in (-128..=127i32).step_by(17).map(f64::from) {
                for b in (-128..=127i32).step_by(15).map(f64::from) {
                    let wide = lab_to_srgb16(l, a, b);
                    assert_eq!(top_bytes(wide), lab_to_srgb(l, a, b), "{l} {a} {b}");
                    refined |= wide.iter().any(|&channel| channel & 0xff != channel >> 8);
                }
            }
        }
        assert!(refined, "no low byte refines its top byte");

        for r in (0..=255u16).step_by(5) {
            for g in (0..=255u16).step_by(15) {
                for b in (0..=255u16).step_by(17) {
                    let widened = srgb_to_lab16(257 * r, 257 * g, 257 * b);
                    let narrow = srgb_to_lab(r as u8, g as u8, b as u8);
                    assert_eq!(
                        (
                            widened.0.to_bits(),
                            widened.1.to_bits(),
                            widened.2.to_bits()
                        ),
                        (narrow.0.to_bits(), narrow.1.to_bits(), narrow.2.to_bits()),
                        "{r} {g} {b}"
                    );
                }
            }
        }
        for v in 0..255u16 {
            let below = srgb_to_lab(v as u8, v as u8, v as u8).0;
            let above = srgb_to_lab(v as u8 + 1, v as u8 + 1, v as u8 + 1).0;
            let between = 257 * v + 128;
            let lightness = srgb_to_lab16(between, between, between).0;
            assert!(
                below < lightness && lightness < above,
                "grey {between}: L* {lightness}, not between {below} and {above}"
            );
        }
    }
}
