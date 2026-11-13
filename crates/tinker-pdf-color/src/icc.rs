//! ICC profiles (ICC.1 / ISO 15076-1), parsed.
//!
//! Bytes in, values out, with no PDF anywhere near it (ruling 8): an
//! `ICCBased` stream is a COS object and resolving one is the facade's
//! business, so what arrives here is the profile's own bytes and nothing else.
//!
//! # What is read, and why that is the right subset
//!
//! The census in `crates/tinker-pdf/tests/icc_census.rs` walked every profile
//! in the four corpora — 2 750 of them, in 2 313 files — and the shape it
//! found decides the shape of this module. (Of those files only 449 name an
//! `ICCBased` colour space that paints something; the rest carry the profile
//! as a PDF/A `/OutputIntent`, which declares what a file was prepared for
//! rather than converting anything.) **2 287 are matrix/TRC** (three
//! `XYZ` columns and three tone curves), **323 are grey** (one curve and a
//! white point), and the remaining 140 carry a multi-dimensional `A2B*` lookup
//! table — what a printer profile needs, because the relation between ink and
//! light is not a matrix and no curve makes one of it.
//!
//! A second census asked what sits *at* those tags, since `A2B0` names four
//! different structures: **409 `mft2`, 6 `mft1`, 3 `mAB `**, and 408 of the
//! 415 v2 tables take four channels to three. So one structure is 99.3 % of
//! them; v4's `mAB ` — curves on both sides of the grid, each stage carrying
//! its own offset — reads beside it.
//!
//! All three models together compile **2 744 of the corpus's 2 750 profiles,
//! 99.8 %**.
//!
//! Beside them, a `D2B*` tag's floating-point transform reads — ICC.1's
//! `multiProcessElementsType`, curve sets, matrices and grids chained, read
//! by the `mpe` module and compiled into integer stages — and converts its
//! intent before that intent's `A2B*` table, as ICC.1 8.10.2 a) puts it.
//! ICC.2's calculator element (the `calc` module) is one of its elements: a
//! program validated and bounded before it runs, and run over a grid of its
//! inputs when its table is built.
//!
//! The other direction, from the connection space to a profile's device
//! components, is [`Output`]: through a `B2A*` table — `mft1`, `mft2` or
//! v4's `mBA ` — or a matrix/TRC or grey model inverted when it is built and
//! integer per colour (`docs/design/icc.md`, "The output transform"). A
//! device link, which takes one device's values to another's, is [`Link`],
//! and the colours an `ncl2` tag names, looked up by name, are
//! [`NamedColours`]. The two directions composed are a soft proof,
//! [`Proof`]: a colour taken into a proof profile's device components at its
//! intent and back to sRGB through the same profile, compiled once into
//! integer tables (`docs/design/icc.md`, "Soft proofing (RC-05)").
//!
//! The same census found the versions: v2 2 739, v4 9, v5 2. So v2 is not a
//! legacy case to tolerate on the way to v4 — it is the case, and the two
//! agree about everything this module reads.
//!
//! # Injection, counted
//!
//! Two defects were reintroduced and the suite run to see what caught them,
//! which is this repository's standing practice for a guard: a guard that
//! catches nothing when its defect is injected is not one.
//!
//! | Injected | Caught by, of 3 012 |
//! | --- | ---: |
//! | one coefficient of the D50-to-sRGB matrix off by 0.1 | **2** |
//! | every gamma exponent multiplied by 1.05 | **3** |
//!
//! The matrix defect is caught by `an_srgb_profile_transforms_to_itself` and by
//! the facade's `an_iccbased_space_is_converted_through_its_profile`, and by
//! nothing else in the workspace — which is the argument for the round trip
//! being the exit criterion rather than a nice-to-have. The curve defect adds
//! `a_linear_curve_compiles_to_a_linear_ramp`, which is the assertion that
//! localises it: the round trip says the answer moved, the ramp says the table
//! is what moved.
//!
//! # Why a wrong parse is loud
//!
//! Unlike the arithmetic-coded formats elsewhere in this workspace, a profile
//! read wrongly does not usually produce a plausible answer: the header
//! carries `acsp` at a fixed offset, every tag names its own extent, and those
//! extents have to lie inside the profile. A mis-parse hits one of those and
//! becomes an [`IccError`], which the caller turns back into the
//! component-count approximation it was already using. That is why this can be
//! reconstructed from the format's structure with some confidence, where a
//! JBIG2 context template could not.

use std::mem::size_of;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use crate::{ColorSpace, XYZ_D50_TO_SRGB};

mod calc;
mod mpe;
mod named;
mod output;
mod proof;
mod spectral;

pub use calc::{
    CalculatorLimit, MAX_CALC_DEPTH, MAX_CALC_OPERATIONS, MAX_CALC_STACK, MAX_CALC_TEMPORARIES,
    MAX_CALC_WORK,
};
pub use mpe::MAX_MPE_ELEMENTS;
pub use named::{NamedColour, NamedColours, MAX_NAMED_COLOURS};
pub use output::{lab_to_xyz, xyz_to_lab, DeviceColour, Link, Output, MAX_DEVICE_CHANNELS};
pub use proof::{Proof, ProofColour, ProofDisplay, ProofError, ProofMemo};
pub use spectral::{SpectralRefusal, ViewingConditions, MAX_SPECTRAL_STEPS};

/// A rendering intent: ICC.1's four (6.1), the ones a transform is compiled
/// for.
///
/// PDF names the same four (8.6.5.8, Table 69) and reads a name it does not
/// recognise as relative colorimetric, which is why the default is that one
/// and why this type has no fifth value: the caller that reads a document's
/// name settles it before it gets here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Intent {
    /// Perceptual: `A2B0`.
    Perceptual,
    /// Media-relative colorimetric: `A2B1`, and PDF's initial intent.
    #[default]
    RelativeColorimetric,
    /// Saturation: `A2B2`.
    Saturation,
    /// ICC-absolute colorimetric: relative colorimetric's table, the
    /// connection-space value scaled by the media white (6.2.3).
    AbsoluteColorimetric,
}

impl Intent {
    /// Every intent, in ICC.1's order of their tags.
    pub const ALL: [Intent; 4] = [
        Intent::Perceptual,
        Intent::RelativeColorimetric,
        Intent::Saturation,
        Intent::AbsoluteColorimetric,
    ];

    /// The sources this intent converts through, in the order it tries them
    /// (`docs/design/icc.md`, "The table each intent compiles"): ICC.1
    /// 8.10.2's a), the `D2B*` tag designated for the intent; b), the `A2B*`
    /// tag designated for it; c), `A2B0`; d), the profile's matrix/TRC or
    /// grey model; then the profile's other tables in the order the parser
    /// reads them, reached only where the profile carries none of those, so
    /// no intent refuses a profile another accepts. `D2B0` to `D2B3` are
    /// perceptual, media-relative colorimetric, saturation and ICC-absolute
    /// colorimetric's (9.2), and only `D2B3` is absolute colorimetric's.
    ///
    /// `A2B3` is a version 5 profile's (ICC.2 9.2.4), ICC-absolute
    /// colorimetric's own table: absolute colorimetric's row reads it after
    /// `D2B3` and before `A2B1`, and every other row last, ICC.2 8.14.2's d).
    /// A version 2 or 4 profile carries none, so its rows are ICC.1's.
    const fn order(self) -> [Step; 6] {
        match self {
            Intent::Perceptual => [
                Step::Pipeline(0),
                Step::Table(0),
                Step::Model,
                Step::Table(1),
                Step::Table(2),
                Step::Table(3),
            ],
            Intent::RelativeColorimetric => [
                Step::Pipeline(1),
                Step::Table(1),
                Step::Table(0),
                Step::Model,
                Step::Table(2),
                Step::Table(3),
            ],
            Intent::Saturation => [
                Step::Pipeline(2),
                Step::Table(2),
                Step::Table(0),
                Step::Model,
                Step::Table(1),
                Step::Table(3),
            ],
            Intent::AbsoluteColorimetric => [
                Step::Pipeline(3),
                Step::Table(3),
                Step::Table(1),
                Step::Table(0),
                Step::Model,
                Step::Table(2),
            ],
        }
    }
}

impl Intent {
    /// The spectral `D2B*` tags this intent converts through, where nothing
    /// colorimetric serves it, in ICC.2 8.14.2's order for a spectral
    /// connection: its own, then `D2B0`, `D2B1` and `D2B3`.
    fn spectral_order(self) -> [usize; 4] {
        let own = match self {
            Intent::Perceptual => 0,
            Intent::RelativeColorimetric => 1,
            Intent::Saturation => 2,
            Intent::AbsoluteColorimetric => 3,
        };
        [own, 0, 1, 3]
    }
}

/// One place an intent's transform may come from, as [`Intent::order`]
/// lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// The `D2B*` tag of this number, `D2B0` to `D2B3`: ICC.1 8.10.2 a); out
    /// of the connection space, the `B2D*` tag of this number.
    Pipeline(usize),
    /// The `A2B*` tag of this number, `A2B0` to `A2B2` and a version 5
    /// profile's `A2B3`; out of the connection space, the `B2A*` tag.
    Table(usize),
    /// The matrix/TRC or grey model, ICC.1 8.10.2 d).
    Model,
}

/// How a colour is converted through a profile: the one parameter every
/// conversion of an ICC colour takes (`docs/design/icc.md`, "The one
/// conversion parameter").
///
/// Its default is relative colorimetric, PDF's initial intent, so a caller
/// that names nothing converts as every conversion did before intents were
/// read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Conversion {
    /// The rendering intent the colour converts in.
    pub intent: Intent,
}

/// Why a profile could not be read.
///
/// Every one of these is a *refusal*, not a repair: a profile this module
/// cannot make a transform out of leaves the caller with the approximation it
/// already had, which is 8.6.5.5's alternate-space reading and is what the
/// engine did before profiles were read at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IccError {
    /// Shorter than the 128-byte header plus a tag count.
    TooShort,
    /// The `acsp` signature at offset 36 is absent, so these bytes are not a
    /// profile however long they are.
    NotAProfile,
    /// The header's own size field disagrees with how many bytes arrived.
    SizeMismatch,
    /// A tag's offset and size do not lie inside the profile.
    TagOutOfBounds,
    /// More tags than [`MAX_ICC_TAGS`], or a profile past [`MAX_ICC_BYTES`].
    TooLarge,
    /// A tag this build does not read, where reading it is the only way to
    /// build a transform — the `A2B*` lookup tables.
    NeedsLut,
    /// A connection space other than `XYZ`, which is the only one a matrix
    /// profile can have.
    UnsupportedPcs,
    /// A data space this build has no transform for.
    UnsupportedSpace,
    /// The tags a transform needs are not all there.
    MissingTags,
    /// A curve or matrix tag whose own contents do not parse.
    MalformedTag,
    /// A multi-process element of a type this build does not read, by its
    /// signature: ICC.1 10.14.1 makes a `multiProcessElementsType` tag
    /// holding one unusable, and 8.10.2 a) passes such a `D2B*` tag over.
    UnsupportedElement([u8; 4]),
    /// A model with no inverse, so no transform out of the connection space
    /// can be built from it ([`Output::compile`]): colorant columns whose
    /// determinant is below one s15.16 step, or a tone curve that decreases
    /// anywhere or never rises.
    NoInverse,
    /// A calculator element's program (ICC.2 11.2.1) past one of the caps
    /// every count it can spend is held to, named: its operations and the
    /// steps a run takes ([`MAX_CALC_OPERATIONS`]), its stack
    /// ([`MAX_CALC_STACK`]), its nesting ([`MAX_CALC_DEPTH`]) or its
    /// temporary channels ([`MAX_CALC_TEMPORARIES`]), or a stricter
    /// `calculatorLimits` the element states; or a tag whose calculators'
    /// runs would take more steps than [`MAX_CALC_WORK`]. Found before the
    /// program runs.
    CalculatorLimit(CalculatorLimit),
    /// A version 5 profile's spectral connection space this engine does not
    /// reduce, or viewing conditions it does not connect, named
    /// ([`SpectralRefusal`], `docs/design/icc.md`, "Version 5").
    Spectral(SpectralRefusal),
}

/// The most tags a profile may declare.
///
/// The tag table is `12 * count` bytes and the count is a 32-bit field, so it
/// is checked before the table is walked (ruling 1).
///
/// | | Tags |
/// | --- | --- |
/// | The most any fixture in this repository spends | 10 |
/// | A 200-page comic archive | 0 |
/// | The busiest profile in the corpus, reached as an XPS `ContextColor` part | 18 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **1 024** |
///
/// The two zeros are facts about those formats rather than absences of
/// measurement, and both have the same cause: nothing in this engine reads a
/// PNG `iCCP` chunk or a JPEG `APP2` profile, so a comic page and a book
/// picture carry their profiles past this parser without opening them. A fixed
/// document does not, because 15.2.5's `ContextColor` names a profile part,
/// `xps::profiles` embeds it verbatim as an `/ICCBased` space, and rendering
/// the synthesised document hands it to [`Profile::parse`] like any other.
///
/// Reachable: the count is a 32-bit field at offset 128 and it is read before
/// the table it describes, so **132 bytes** may declare 4 294 967 295 tags —
/// which is the whole reason the check sits where it does, and what
/// `a_tag_count_past_the_cap_is_refused_before_the_table_is_walked` builds.
pub const MAX_ICC_TAGS: u32 = 1024;

/// The most bytes a profile may be.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends | 2 624 |
/// | A 200-page comic archive | 0 |
/// | The largest profile in the corpus: a printer profile carrying a full set of lookup tables | 718 672 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **2 MiB** |
///
/// The next power of two above the corpus's largest, which leaves the bound
/// clearing the thing the format is for by better than a factor of two. The
/// two zeros are [`MAX_ICC_TAGS`]'s, for its reason.
///
/// Reachable: a profile arrives as a decoded stream, and
/// `tinker_pdf_cos::limits::MAX_DECODED_STREAM` bounds one of those at 128 MiB
/// — sixty-four times this cap, so the cap is a cap.
/// `a_profile_past_the_byte_cap_is_refused_before_its_signature_is_read` builds
/// one past it.
pub const MAX_ICC_BYTES: usize = 1 << 21;

/// A tone reproduction curve.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// `curv` with a count of zero: the identity.
    Identity,
    /// `curv` with a count of one: a pure gamma, stored as u8Fixed8.
    Gamma(f64),
    /// `curv` with a count above one: a sampled curve, interpolated.
    Sampled(Vec<u16>),
    /// `para`: one of ICC.1's five parametric forms, by function type and its
    /// parameters in order.
    Parametric {
        /// Which of ICC.1's five forms.
        function: u16,
        /// Its parameters, in the order the clause lists them.
        params: Vec<f64>,
    },
}

impl Curve {
    /// The curve at `x` in `0..=1`, answering in `0..=1`.
    ///
    /// Floating point, and deliberately: this runs once per entry when a
    /// transform's tables are built, never per pixel. Ruling 4's ban is on the
    /// pixel path, and `cargo xtask libm` sees the whole crate — which is why
    /// the transcendental here is `tinker_pdf_math::pow` rather than the
    /// platform's.
    #[must_use]
    pub fn eval(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Identity => x,
            Curve::Gamma(g) => tinker_pdf_math::pow(x, *g),
            Curve::Sampled(points) => sample(points, x),
            Curve::Parametric { function, params } => parametric(*function, params, x),
        }
    }
}

/// Linear interpolation through a sampled curve.
fn sample(points: &[u16], x: f64) -> f64 {
    if points.is_empty() {
        return x;
    }
    if points.len() == 1 {
        return f64::from(points[0]) / 65535.0;
    }
    let last = points.len() - 1;
    let position = x * last as f64;
    let lower = position.floor();
    let index = (lower as usize).min(last);
    let next = (index + 1).min(last);
    let fraction = position - lower;
    let a = f64::from(points[index]) / 65535.0;
    let b = f64::from(points[next]) / 65535.0;
    a + (b - a) * fraction
}

/// ICC.1's five parametric curve forms.
///
/// Written from the definitions rather than from a table of coefficients, so
/// the shared shape — a power law with a linear segment below a threshold — is
/// visible in the code the way it is in the clause. Type 3 is sRGB's own form
/// and type 4 is the general one; the first three are it with terms dropped.
fn parametric(function: u16, params: &[f64], x: f64) -> f64 {
    let at = |i: usize| params.get(i).copied().unwrap_or(0.0);
    let power = |base: f64, exponent: f64| {
        if base <= 0.0 {
            0.0
        } else {
            tinker_pdf_math::pow(base, exponent)
        }
    };
    let (g, a, b, c, d, e, f) = (at(0), at(1), at(2), at(3), at(4), at(5), at(6));
    match function {
        0 => power(x, g),
        1 => {
            if a != 0.0 && x >= -b / a {
                power(a * x + b, g)
            } else {
                0.0
            }
        }
        2 => {
            if a != 0.0 && x >= -b / a {
                power(a * x + b, g) + c
            } else {
                c
            }
        }
        3 => {
            if x >= d {
                power(a * x + b, g)
            } else {
                c * x
            }
        }
        4 => {
            if x >= d {
                power(a * x + b, g) + e
            } else {
                c * x + f
            }
        }
        // A form ICC.1 does not define. Answering the identity would be a
        // guess; the caller sees `MalformedTag` through `Profile::parse`.
        _ => x,
    }
}

/// A profile, in the parts a transform is built from.
///
/// A profile may carry both of the things a transform is built from: the
/// `A2B*` tables, and a matrix/TRC or grey model. ICC.1 8.10.2 orders them,
/// the tables first ([`Intent::order`]), so an intent converts through its
/// model only where the profile carries none of the tables 8.10.2 names for
/// it.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// The data colour space, as its four-byte signature.
    pub space: [u8; 4],
    /// The profile connection space.
    pub pcs: [u8; 4],
    /// The device class.
    pub class: [u8; 4],
    /// The major version, which the census says is 2 for all but eleven
    /// profiles in the corpus.
    pub version: u8,
    /// The matrix/TRC or grey model, where the profile carries one that
    /// reads: ICC.1 8.10.2 d)'s source. `None` where it carries neither, or
    /// one that contradicts the profile's own header.
    pub model: Option<Model>,
    /// The `A2B*` tables the profile carries, 8.10.2's b) and c); empty
    /// where it carries none.
    pub tables: Tables,
    /// The media white, `mediaWhitePointTag` (`wtpt`), as `[X, Y, Z]`, where
    /// the profile carries one that reads. ICC-absolute colorimetry scales
    /// by it ([`Profile::media_scale`]); no other intent reads it.
    pub white: Option<[f64; 3]>,
    /// The colours its `namedColor2Tag` (`ncl2`, ICC.1 10.15) names, where it
    /// carries one that reads: what a profile of class `nmcl` (8.9) is made
    /// of, and what any other may carry beside its transforms.
    pub named: Option<NamedColours>,
    /// What a version 5 profile's header adds to ICC.1's (ICC.2 7.2.2, Table
    /// 12); `None` for a version 2 or 4 profile, whose bytes 100 to 127 are
    /// reserved.
    pub version5: Option<Version5>,
}

/// The fields a version 5 profile's header adds (ICC.2 7.2.2, Table 12):
/// bytes 100 to 123, each read as it stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Version5 {
    /// The spectral connection space (7.2.21, Table 21): a two-letter type —
    /// `rs` reflectance, `ts` transmission, `es` emission, `bs` bi-spectral,
    /// `sm` sparse — and a 16-bit channel count, or zero where the
    /// connection-space field alone defines the connection space.
    pub spectral_pcs: [u8; 4],
    /// The spectral connection space's wavelengths (7.2.22).
    pub spectral_range: SpectralRange,
    /// The incident wavelengths of a bi-spectral connection space (7.2.23).
    pub bispectral_range: SpectralRange,
    /// The multiplex connection space (7.2.24), or zero.
    pub mcs: [u8; 4],
    /// The profile's device sub-class (7.2.25), or zero.
    pub subclass: [u8; 4],
}

/// A `spectralRange` (ICC.2 4.2.8, Table 6): a start and an end wavelength,
/// each a `float16Number`, and how many steps sample the range, the interval
/// between them `(end − start) / (steps − 1)` (7.2.22, formula 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectralRange {
    /// The first wavelength, in nanometres.
    pub start: f64,
    /// The last wavelength, in nanometres.
    pub end: f64,
    /// How many wavelengths the range samples.
    pub steps: u16,
}

impl SpectralRange {
    /// The range at `at` in `bytes`: two half floats and a step count.
    fn read(reader: &Reader<'_>, at: usize) -> Option<SpectralRange> {
        Some(SpectralRange {
            start: half_float(reader.u16(at)?),
            end: half_float(reader.u16(at + 2)?),
            steps: reader.u16(at + 4)?,
        })
    }
}

/// An IEEE 754 half-precision number (ICC.2 4.2.2's `float16Number`) as the
/// `f64` it is exactly: a sign, five bits of exponent and ten of fraction;
/// an exponent of all ones is an infinity or not a number, which a reader
/// that wants a finite value refuses.
fn half_float(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = (bits >> 10) & 0x1F;
    let fraction = f64::from(bits & 0x03FF);
    match exponent {
        // Subnormal: the fraction in units of 2^-24, which is exact.
        0 => sign * fraction / 16_777_216.0,
        0x1F if fraction == 0.0 => sign * f64::INFINITY,
        0x1F => f64::NAN,
        // `(1024 + fraction) * 2^(exponent - 25)`, the power of two built
        // from its bits so it is exact.
        _ => {
            let power = f64::from_bits((1023 + u64::from(exponent) - 25) << 52);
            sign * (1024.0 + fraction) * power
        }
    }
}

/// The PCS white, 6.3.4.3's D50, as the s15Fixed16Numbers an `XYZType` tag
/// carries it in (`0x0000F6D6`, `0x00010000`, `0x0000D32D`): a `wtpt` of D50
/// reads back as exactly these, so its absolute colorimetry scales by exactly
/// one.
pub const PCS_WHITE: [f64; 3] = [63_190.0 / 65_536.0, 1.0, 54_061.0 / 65_536.0];

/// The engine's own sRGB profile (`docs/design/icc.md`, "A first-party sRGB
/// profile (RC-19)"): an ICC profile of version 2.4.0, class `mntr`, data
/// space `RGB ` and connection space `XYZ `, which `build.rs` computes from
/// IEC 61966-2-1's published primaries, D65 white point and transfer
/// function — the colorants adapted to the D50 PCS white by ICC.1:2001-04
/// E.1.2's linearized Bradford transform, the three tone curves one sampled
/// curve of 1 024 entries, the media white the PCS white and `chad` the
/// adaptation.
///
/// Computed, not vendored: no file of anyone else's is read to make it, and
/// the bytes are one string on every build host, which
/// `crates/tinker-pdf/tests/determinism.rs` pins. A caller states it where a
/// profile is stated — as a PDF/A destination profile, or as the stream of an
/// `/ICCBased` `/DefaultRGB` — and this crate reads it as it reads any:
/// [`Profile::parse`] gives [`Model::MatrixTrc`], and its transforms take
/// every colour back to itself within one level.
pub const SRGB_PROFILE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/srgb.icc"));

impl Profile {
    /// The factors ICC-absolute colorimetry scales a connection-space value
    /// by, channel by channel: the media white over the PCS white, 6.2.3's
    /// `Xmw / Xi`, `Ymw / Yi` and `Zmw / Zi` (equations 4 to 6). The engine's
    /// destination is sRGB, whose media white is the PCS white, so the
    /// destination's own ratio (equations 1 to 3) is one
    /// (`docs/design/icc.md`, "Absolute colorimetric against the engine's
    /// output").
    ///
    /// One in every channel where the profile carries no media white, or one
    /// with a component that is not positive: such a profile is taken to have
    /// the PCS white for its media white, which 8.2 requires of it and which
    /// adds no colour the profile does not state, so its absolute
    /// colorimetry is its relative colorimetry.
    #[must_use]
    pub fn media_scale(&self) -> [f64; 3] {
        match self.white {
            Some(white) if white.iter().all(|&component| component > 0.0) => [
                white[0] / PCS_WHITE[0],
                white[1] / PCS_WHITE[1],
                white[2] / PCS_WHITE[2],
            ],
            _ => [1.0; 3],
        }
    }

    /// The scaling `intent`'s transform applies to the connection-space
    /// value a source gives — ICC-absolute where `absolute`, `D2B3` and a
    /// version 5 profile's `A2B3`, media-relative otherwise: under absolute
    /// colorimetric [`Profile::media_scale`] through a media-relative source
    /// (6.2.3); under another intent its inverse through an ICC-absolute
    /// one, the relative value found again from the absolute (ICC.2
    /// 9.2.93); and none where the two agree, or the scale is one. A
    /// transform out of the connection space divides by the same scaling
    /// before its source.
    fn scale_for(&self, intent: Intent, absolute: bool) -> Option<[f64; 3]> {
        let scale = Some(self.media_scale()).filter(|scale| *scale != [1.0; 3])?;
        match (intent == Intent::AbsoluteColorimetric, absolute) {
            (true, false) => Some(scale),
            (false, true) => Some(scale.map(|channel| 1.0 / channel)),
            _ => None,
        }
    }
}

/// The data colour space signature of a profile, read from the **header** and
/// nothing else.
///
/// ICC.1 §7.2 puts the signature at offset 16 and `acsp` at offset 36, both
/// inside the fixed 128-byte header, so this needs neither the tag table nor a
/// transform. That is the whole reason it exists next to [`Profile::parse`]
/// rather than behind it: `parse` refuses a profile it cannot build a
/// transform out of — a CMYK profile with no `A2B*` LUT is
/// [`IccError::UnsupportedSpace`] — and a caller that only wants to *embed*
/// the profile and let a PDF reader do the colour management needs the channel
/// count from a profile this build could never transform.
///
/// `None` for anything too short to hold a header, or whose `acsp` is absent.
#[must_use]
pub fn data_space(bytes: &[u8]) -> Option<[u8; 4]> {
    if bytes.len() < 128 || bytes.get(36..40)? != b"acsp" {
        return None;
    }
    bytes.get(16..20)?.try_into().ok()
}

/// How many components a data colour space signature takes (ICC.1 §7.2.6).
///
/// The two families are read differently: the named spaces are a table, and
/// the `nCLR` family spells its own count in the first character — `2CLR`
/// through `FCLR`, hexadecimal, so `F` is fifteen. Nothing here guesses; a
/// signature this does not hold answers `None` rather than a plausible three.
#[must_use]
pub fn channels(space: [u8; 4]) -> Option<u8> {
    Some(match &space {
        b"GRAY" => 1,
        b"CMY " => 3,
        b"CMYK" => 4,
        // Every three-component space ICC.1 names. They differ in what the
        // three numbers *mean* and not in how many there are, which is all
        // this answers.
        b"RGB " | b"XYZ " | b"Lab " | b"Luv " | b"YCbr" | b"Yxy " | b"HSV " | b"HLS " => 3,
        // §7.2.6's `nCLR`: the count is the first character in hex.
        [digit, b'C', b'L', b'R'] => {
            let count = char::from(*digit).to_digit(16)?;
            if !(2..=15).contains(&count) {
                return None;
            }
            u8::try_from(count).ok()?
        }
        _ => return None,
    })
}

impl Profile {
    /// How many components the profile's **data** colour space takes.
    ///
    /// ICC.1 §7.2.6 states the signature and the count together, and the two
    /// families are read differently: the named spaces are a table, and the
    /// `nCLR` family spells its own count in the first character — `2CLR`
    /// through `FCLR`, hexadecimal, so `F` is fifteen. Nothing here guesses
    /// from the tag data; a signature this table does not hold answers `None`
    /// rather than a plausible three.
    ///
    /// The count is a fact about the *space*, not about whether this build can
    /// transform it, which is why it is here and not on [`Model`]: a caller
    /// embedding the profile in a PDF `/ICCBased` space needs the number and
    /// never evaluates the profile at all.
    #[must_use]
    pub fn channels(&self) -> Option<u8> {
        channels(self.space)
    }
}

/// The closed-form transform a profile's tags describe, beside or instead of
/// its lookup tables: ICC.1 8.10.2 d)'s tone curves and colorant columns, or
/// its `grayTRC`.
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    /// Three columns and three curves: `linear = TRC(encoded)`, then a matrix
    /// into the connection space.
    MatrixTrc {
        /// The `rXYZ`, `gXYZ`, `bXYZ` columns, row-major as `[X, Y, Z]` each.
        columns: [[f64; 3]; 3],
        /// `rTRC`, `gTRC`, `bTRC`.
        curves: [Curve; 3],
    },
    /// One curve: a grey profile.
    Grey {
        /// Whether the curve's output is `L*` rather than `Y` — that is,
        /// whether the connection space is `Lab` rather than `XYZ`.
        ///
        /// A grey profile has one axis and the connection space decides what
        /// that axis *is*. Under XYZ the curve gives luminance directly; under
        /// Lab it gives lightness, which is the same quantity seen through
        /// 8.6.5.4's cube root — and a visibly different grey if the two are
        /// confused.
        lab: bool,
        /// `kTRC`.
        curve: Curve,
    },
}

impl Model {
    /// The connection-space value the model gives `components`, in its own
    /// connection space: a matrix/TRC model's columns over its curves'
    /// outputs (8.10.2 d)), a grey model's curve on the PCS white — `Y` times
    /// the white's XYZ, or `L*` with `a*` and `b*` nought (F.2).
    fn connection(&self, components: &[f64]) -> [f64; 3] {
        let at = |index: usize| components.get(index).copied().unwrap_or(0.0);
        match self {
            Model::MatrixTrc { columns, curves } => {
                let linear = [0, 1, 2].map(|channel| curves[channel].eval(at(channel)));
                [0, 1, 2].map(|row| {
                    columns
                        .iter()
                        .zip(linear)
                        .map(|(column, value)| column[row] * value)
                        .sum()
                })
            }
            Model::Grey { lab, curve } => {
                let value = curve.eval(at(0));
                if *lab {
                    [value * 100.0, 0.0, 0.0]
                } else {
                    PCS_WHITE.map(|white| white * value)
                }
            }
        }
    }
}

/// The `A2B*` tables a profile carries — sampled lookup tables, `mft1`,
/// `mft2` or v4's `mAB ` — and its `D2B*` multi-process pipelines, each read
/// when an intent first converts through it.
///
/// What a printer profile carries, because the relation between ink and
/// light is not a matrix and no curve makes it one. The census found 409
/// `mft2` and 6 `mft1` against 3 of v4's `mAB `, and 408 of the 415 take
/// four channels to three.
///
/// The tables relative colorimetric's order reaches before it finds one that
/// reads are read by [`Profile::parse`], so a profile whose table does not
/// read is refused there where nothing else serves the intent, and a page
/// that names no intent reads no table it does not convert through. Tags
/// that name one element — the same offset and size, which ICC.1 8.2 lets
/// every intent share — are one table, read once.
///
/// Beside them the `B2A*` tables, the other direction's (ICC.1 9.2.6 to
/// 9.2.8) — `mft1`, `mft2` or v4's `mBA ` — from the connection space to
/// the profile's device channels, kept unread, as an intent's own `A2B*`
/// table is, until a transform out of the connection space first selects one
/// ([`Output::compile`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Tables {
    /// For `A2B0` to `A2B3`, which of `tables` the tag names where it holds a
    /// lookup table, or `None`. `A2B3` is a version 5 profile's.
    slots: [Option<usize>; 4],
    /// One a distinct `A2B*` table, in the order the tags first name them.
    tables: Vec<Element<Lut>>,
    /// For `D2B0` to `D2B3`, then `A2B0` to `A2B3` where a version 5
    /// profile's holds a `multiProcessElementsType` (ICC.2 9.2.1 to 9.2.4),
    /// which of `pipelines` the tag names, or `None`.
    pipeline_slots: [Option<usize>; 8],
    /// One a distinct element of those, in the order the tags first name
    /// them.
    pipelines: Vec<Element<mpe::Pipeline>>,
    /// For `B2A0` to `B2A3`, which of `outputs` the tag names where it holds
    /// a lookup table, or `None`.
    output_slots: [Option<usize>; 4],
    /// One a distinct `B2A*` table, in the order the tags first name them.
    outputs: Vec<Element<Lut>>,
    /// For `B2D0` to `B2D3`, then `B2A0` to `B2A3` where a version 5
    /// profile's holds a `multiProcessElementsType`, which of
    /// `output_pipelines` the tag names, or `None`: the other direction's
    /// pipelines, out of the connection space.
    output_pipeline_slots: [Option<usize>; 8],
    /// One a distinct element of those, in the order the tags first name
    /// them.
    output_pipelines: Vec<Element<mpe::Pipeline>>,
    /// The gamut tag, `gamt` (ICC.1 9.2.28): a table from the connection
    /// space to one channel, kept unread, as a `B2A*` table is, until a
    /// transform out of the connection space reads it.
    gamut: Option<Element<Lut>>,
    /// A device link's `A2B0` and `D2B0`, from one device to another
    /// (ICC.1 8.6): located only for a profile of class `link`, and never
    /// an intent's table.
    link: Option<LinkTags>,
    /// For `D2B0` to `D2B3` of a version 5 profile whose connection space is
    /// spectral (ICC.2 9.2.77), which of `spectral` the tag names, or
    /// `None`: what an intent converts through where nothing colorimetric
    /// serves it, reduced to XYZ.
    spectral_slots: [Option<usize>; 4],
    /// One a distinct such tag, in the order the tags first name them.
    spectral: Vec<Element<mpe::Pipeline>>,
    /// The reduction those tags end in: three rows of a weight a wavelength
    /// of the spectral connection space ([`ViewingConditions::reduction`]).
    weights: Vec<f64>,
    /// Why a version 5 profile's spectral tags are not read, where its
    /// header names a spectral connection space they are not reduced from:
    /// the profile's refusal where nothing else serves it.
    spectral_refusal: Option<IccError>,
    /// The connection tags of a version 5 profile whose viewing conditions
    /// are not the standard ones (ICC.2 10.2.22); `None` for one whose are,
    /// or which states none.
    custom: Option<Custom>,
    /// Whether the connection space is `Lab` rather than `XYZ`.
    lab: bool,
}

/// How a version 5 profile's own viewing conditions connect to the standard
/// ones (ICC.2 10.2.22): its `customToStandardPccTag` (`c2sp`, 9.2.57) after
/// each transform into the connection space, and its
/// `standardToCustomPccTag` (`s2cp`, 9.2.113) before each one out of it,
/// each kept unread until a transform first needs it.
#[derive(Clone, Debug, PartialEq)]
struct Custom {
    to_standard: Option<Element<mpe::Pipeline>>,
    from_standard: Option<Element<mpe::Pipeline>>,
}

/// What a device link (ICC.1 8.6) converts through: its `D2B0`, which
/// 8.10.3 a) puts first where it reads, and its `A2B0`, each kept unread
/// until [`Profile::parse`] reads it.
#[derive(Clone, Debug, PartialEq)]
struct LinkTags {
    /// The data space's channels, which the link takes.
    inputs: usize,
    /// The channels of the space the connection-space field names, which
    /// the link gives (7.2.7).
    outputs: usize,
    /// The `A2B0` tag's element: `mft1`, `mft2` or `mAB `.
    table: Option<Element<Lut>>,
    /// The `D2B0` tag's element, a `multiProcessElementsType`.
    pipeline: Option<Element<mpe::Pipeline>>,
}

/// What a device link's transform runs through.
#[derive(Clone, Debug, PartialEq)]
enum LinkSource {
    /// Its `A2B0`: `mft1`, `mft2` or `mAB `.
    Table(Arc<Lut>),
    /// Its `D2B0`'s elements.
    Elements(Arc<mpe::Pipeline>),
}

impl LinkTags {
    /// The link's tags at `table` and `pipeline` — `A2B0`'s and `D2B0`'s
    /// offset and size in `bytes`, where the profile carries them — unread,
    /// between a data space of `inputs` channels and an output space of
    /// `outputs`.
    fn locate(
        inputs: usize,
        outputs: usize,
        table: Option<(usize, usize)>,
        pipeline: Option<(usize, usize)>,
        bytes: &[u8],
    ) -> Result<LinkTags, IccError> {
        let (_, mut tables) = Element::locate([table], bytes)?;
        let (_, mut pipelines) = Element::locate([pipeline], bytes)?;
        Ok(LinkTags {
            inputs,
            outputs,
            table: tables.pop(),
            pipeline: pipelines.pop(),
        })
    }

    /// What the link converts through, read now if nothing has read it yet:
    /// its `D2B0` where that reads, as ICC.1 8.10.3 a) has it, and otherwise
    /// its `A2B0`. A `D2B0` that does not read is passed over; where neither
    /// reads, the `A2B0`'s refusal, else the `D2B0`'s, else
    /// [`IccError::MissingTags`] for a link carrying neither.
    fn source(&self) -> Result<LinkSource, IccError> {
        let pipeline = self
            .pipeline
            .as_ref()
            .map(|element| element.read(|bytes| mpe::read_link(bytes, self.inputs, self.outputs)));
        if let Some(Ok(pipeline)) = &pipeline {
            return Ok(LinkSource::Elements(Arc::clone(pipeline)));
        }
        let table = self.table.as_ref().map(|element| {
            element.read(|bytes| {
                read_lut(
                    bytes,
                    false,
                    Direction::Link {
                        inputs: self.inputs,
                        outputs: self.outputs,
                    },
                )
            })
        });
        match (table, pipeline) {
            (Some(Ok(table)), _) => Ok(LinkSource::Table(table)),
            (Some(Err(error)), _) | (None, Some(Err(error))) => Err(error),
            (None, _) => Err(IccError::MissingTags),
        }
    }

    /// Lets go of the bytes of each tag that has been read.
    fn release_read(&mut self) {
        if let Some(table) = &mut self.table {
            table.release_read();
        }
        if let Some(pipeline) = &mut self.pipeline {
            pipeline.release_read();
        }
    }

    /// The most bytes the link's tags can come to hold, read or not.
    fn held_bound(&self) -> usize {
        let table = self.table.as_ref().map_or(0, |table| {
            table.held_bound(Lut::held_bytes, |raw| lut_bound(raw.len()))
        });
        let pipeline = self.pipeline.as_ref().map_or(0, |pipeline| {
            pipeline.held_bound(mpe::Pipeline::held_bytes, mpe::bound)
        });
        table.saturating_add(pipeline)
    }

    /// The bytes the link's tags hold now.
    #[cfg(test)]
    fn held(&self) -> usize {
        self.table
            .as_ref()
            .map_or(0, |table| table.held(Lut::held_bytes))
            + self
                .pipeline
                .as_ref()
                .map_or(0, |pipeline| pipeline.held(mpe::Pipeline::held_bytes))
    }
}

/// Where a profile's transform tags are, as [`Tables::locate`] takes them:
/// each tag's offset and size, where the profile carries it.
struct Places {
    /// `A2B0` to `A2B3`.
    a2b: [Option<(usize, usize)>; 4],
    /// `D2B0` to `D2B3`.
    d2b: [Option<(usize, usize)>; 4],
    /// `B2A0` to `B2A3`.
    b2a: [Option<(usize, usize)>; 4],
    /// `B2D0` to `B2D3`.
    b2d: [Option<(usize, usize)>; 4],
    /// `gamt`.
    gamut: Option<(usize, usize)>,
    /// Whether an `A2B*` or `B2A*` tag may hold a
    /// `multiProcessElementsType`, as a version 5 profile's may (ICC.2 9.2.1,
    /// 9.2.38).
    multi_process: bool,
}

impl Tables {
    /// The tables and pipelines at `places` in `bytes`, none of them read
    /// yet: an `A2B*` or `B2A*` tag holding a `multiProcessElementsType`
    /// among the pipelines where `places` lets it, among the tables
    /// otherwise.
    fn locate(places: Places, bytes: &[u8], lab: bool) -> Result<Tables, IccError> {
        let holds_elements = |place: Option<(usize, usize)>| {
            places.multi_process
                && place.is_some_and(|(offset, _)| bytes.get(offset..offset + 4) == Some(b"mpet"))
        };
        let split = |tags: [Option<(usize, usize)>; 4]| {
            let tables = tags.map(|place| place.filter(|_| !holds_elements(place)));
            let elements = tags.map(|place| place.filter(|_| holds_elements(place)));
            (tables, elements)
        };
        let (a2b, a2b_elements) = split(places.a2b);
        let (b2a, b2a_elements) = split(places.b2a);
        let joined = |first: [Option<(usize, usize)>; 4], then: [Option<(usize, usize)>; 4]| {
            [
                first[0], first[1], first[2], first[3], then[0], then[1], then[2], then[3],
            ]
        };
        let (slots, tables) = Element::locate(a2b, bytes)?;
        let (pipeline_slots, pipelines) = Element::locate(joined(places.d2b, a2b_elements), bytes)?;
        let (output_slots, outputs) = Element::locate(b2a, bytes)?;
        let (output_pipeline_slots, output_pipelines) =
            Element::locate(joined(places.b2d, b2a_elements), bytes)?;
        let (_, mut gamut) = Element::locate([places.gamut], bytes)?;
        Ok(Tables {
            slots,
            tables,
            pipeline_slots,
            pipelines,
            output_slots,
            outputs,
            output_pipeline_slots,
            output_pipelines,
            gamut: gamut.pop(),
            link: None,
            spectral_slots: [None; 4],
            spectral: Vec::new(),
            weights: Vec::new(),
            spectral_refusal: None,
            custom: None,
            lab,
        })
    }

    /// No tables: a profile that carries none, or whose connection space no
    /// table converts into.
    fn none(lab: bool) -> Tables {
        Tables {
            slots: [None; 4],
            tables: Vec::new(),
            pipeline_slots: [None; 8],
            pipelines: Vec::new(),
            output_slots: [None; 4],
            outputs: Vec::new(),
            output_pipeline_slots: [None; 8],
            output_pipelines: Vec::new(),
            gamut: None,
            link: None,
            spectral_slots: [None; 4],
            spectral: Vec::new(),
            weights: Vec::new(),
            spectral_refusal: None,
            custom: None,
            lab,
        }
    }

    /// The table `A2B*` tag `tag` names, where the profile carries it.
    fn at(&self, tag: usize) -> Option<(usize, &Element<Lut>)> {
        let index = (*self.slots.get(tag)?)?;
        Some((index, self.tables.get(index)?))
    }

    /// The pipeline `D2B*` tag `tag` names, where the profile carries it.
    fn pipeline_at(&self, tag: usize) -> Option<(usize, &Element<mpe::Pipeline>)> {
        let index = (*self.pipeline_slots.get(tag)?)?;
        Some((index, self.pipelines.get(index)?))
    }

    /// The pipeline a version 5 profile's `A2B*` tag `tag` names, where it
    /// holds a `multiProcessElementsType`.
    fn table_pipeline_at(&self, tag: usize) -> Option<(usize, &Element<mpe::Pipeline>)> {
        self.pipeline_at(tag.checked_add(4)?)
    }

    /// The table `B2A*` tag `tag` names, where the profile carries it.
    fn output_at(&self, tag: usize) -> Option<(usize, &Element<Lut>)> {
        let index = (*self.output_slots.get(tag)?)?;
        Some((index, self.outputs.get(index)?))
    }

    /// The pipeline `B2D*` tag `tag` names, where the profile carries it.
    fn output_pipeline_at(&self, tag: usize) -> Option<(usize, &Element<mpe::Pipeline>)> {
        let index = (*self.output_pipeline_slots.get(tag)?)?;
        Some((index, self.output_pipelines.get(index)?))
    }

    /// The pipeline a version 5 profile's `B2A*` tag `tag` names, where it
    /// holds a `multiProcessElementsType`.
    fn output_table_pipeline_at(&self, tag: usize) -> Option<(usize, &Element<mpe::Pipeline>)> {
        self.output_pipeline_at(tag.checked_add(4)?)
    }

    /// The pipeline out of the connection space at `element`, to `channels`
    /// device channels, read now if nothing has read it yet.
    fn read_output_pipeline(
        &self,
        element: &Element<mpe::Pipeline>,
        channels: usize,
    ) -> Result<Arc<mpe::Pipeline>, IccError> {
        element.read(|bytes| mpe::read_output(bytes, self.lab, channels))
    }

    /// The `A2B*` table at `element`, read now if nothing has read it yet.
    fn read_table(&self, element: &Element<Lut>) -> Result<Arc<Lut>, IccError> {
        element.read(|bytes| read_lut(bytes, self.lab, Direction::Forward))
    }

    /// The `B2A*` table at `element`, from the connection space to
    /// `channels` device channels, read now if nothing has read it yet.
    fn read_output(&self, element: &Element<Lut>, channels: usize) -> Result<Arc<Lut>, IccError> {
        element.read(|bytes| read_lut(bytes, self.lab, Direction::Output { channels }))
    }

    /// The gamut tag's table, from the connection space to one channel, read
    /// now if nothing has read it yet; `None` where the profile carries no
    /// `gamt`.
    fn read_gamut(&self) -> Option<Result<Arc<Lut>, IccError>> {
        let element = self.gamut.as_ref()?;
        Some(element.read(|bytes| read_lut(bytes, self.lab, Direction::Output { channels: 1 })))
    }

    /// The `D2B*` pipeline, or a version 5 profile's `A2B*`, at `element`,
    /// read now if nothing has read it yet.
    fn read_pipeline(
        &self,
        element: &Element<mpe::Pipeline>,
    ) -> Result<Arc<mpe::Pipeline>, IccError> {
        element.read(|bytes| mpe::read(bytes, self.lab))
    }

    /// The spectral `D2B*` tag `tag` names, where the profile's spectral
    /// tags are read at all.
    fn spectral_at(&self, tag: usize) -> Option<(usize, &Element<mpe::Pipeline>)> {
        let index = (*self.spectral_slots.get(tag)?)?;
        Some((index, self.spectral.get(index)?))
    }

    /// The spectral `D2B*` tag at `element`, reduced to XYZ through
    /// `weights`, read now if nothing has read it yet.
    fn read_spectral(
        &self,
        element: &Element<mpe::Pipeline>,
    ) -> Result<Arc<mpe::Pipeline>, IccError> {
        element.read(|bytes| mpe::read_spectral(bytes, &self.weights))
    }

    /// What a transform into the connection space connects to the standard
    /// viewing conditions through: nothing for a profile in them, and its
    /// `c2sp` for one that is not, read now if nothing has read it yet.
    ///
    /// # Errors
    ///
    /// [`SpectralRefusal::NoCustomToStandard`] for a profile that carries
    /// none, and the tag's own refusal where it does not read.
    fn to_standard(&self) -> Result<Option<Arc<mpe::Pipeline>>, IccError> {
        let Some(custom) = &self.custom else {
            return Ok(None);
        };
        let element = custom
            .to_standard
            .as_ref()
            .ok_or(IccError::Spectral(SpectralRefusal::NoCustomToStandard))?;
        element.read(mpe::read_connection).map(Some)
    }

    /// What a transform out of the connection space takes a standard value
    /// through first: nothing for a profile in the standard viewing
    /// conditions, and its `s2cp` for one that is not.
    ///
    /// # Errors
    ///
    /// [`SpectralRefusal::NoStandardToCustom`] for a profile that carries
    /// none, and the tag's own refusal where it does not read.
    fn from_standard(&self) -> Result<Option<Arc<mpe::Pipeline>>, IccError> {
        let Some(custom) = &self.custom else {
            return Ok(None);
        };
        let element = custom
            .from_standard
            .as_ref()
            .ok_or(IccError::Spectral(SpectralRefusal::NoStandardToCustom))?;
        element.read(mpe::read_connection).map(Some)
    }

    /// Lets go of the bytes of every table and pipeline that has been read,
    /// whatever reading it gave: what it gave is kept, and nothing is read
    /// twice.
    fn release_read(&mut self) {
        for table in &mut self.tables {
            table.release_read();
        }
        for pipeline in &mut self.pipelines {
            pipeline.release_read();
        }
        for pipeline in &mut self.spectral {
            pipeline.release_read();
        }
        if let Some(custom) = &mut self.custom {
            for pipeline in custom
                .to_standard
                .iter_mut()
                .chain(&mut custom.from_standard)
            {
                pipeline.release_read();
            }
        }
        if let Some(link) = &mut self.link {
            link.release_read();
        }
    }
}

/// One distinct element a profile's tags name: its bytes, until they are
/// read, and what reading them gave, kept from the first read.
#[derive(Clone, Debug)]
struct Element<T> {
    /// The element's bytes; `None` once [`Profile::parse`] has read them.
    bytes: Option<Vec<u8>>,
    /// What the bytes read as, or why they do not.
    read: OnceLock<Result<Arc<T>, IccError>>,
}

impl<T> Element<T> {
    /// For each of `places` — a tag's offset and size, where the profile
    /// carries the tag — which of the distinct elements it names, and those
    /// elements, unread.
    fn locate<const N: usize>(
        places: [Option<(usize, usize)>; N],
        bytes: &[u8],
    ) -> Result<([Option<usize>; N], Vec<Element<T>>), IccError> {
        let mut distinct: Vec<(usize, usize)> = Vec::new();
        let mut slots = [None; N];
        for (slot, place) in slots.iter_mut().zip(places) {
            if let Some(place) = place {
                let index = match distinct.iter().position(|&element| element == place) {
                    Some(index) => index,
                    None => {
                        distinct.push(place);
                        distinct.len() - 1
                    }
                };
                *slot = Some(index);
            }
        }
        let mut elements = Vec::with_capacity(distinct.len());
        for (offset, size) in distinct {
            let data = offset
                .checked_add(size)
                .and_then(|end| bytes.get(offset..end))
                .ok_or(IccError::TagOutOfBounds)?;
            elements.push(Element {
                bytes: Some(data.to_vec()),
                read: OnceLock::new(),
            });
        }
        Ok((slots, elements))
    }

    /// What the element reads as by `reader`, read now if nothing has read
    /// it yet, or why it does not read.
    fn read(&self, reader: impl FnOnce(&[u8]) -> Result<T, IccError>) -> Result<Arc<T>, IccError> {
        self.read
            .get_or_init(|| {
                let bytes = self.bytes.as_deref().ok_or(IccError::MalformedTag)?;
                reader(bytes).map(Arc::new)
            })
            .clone()
    }

    /// Lets go of the bytes once they have been read.
    fn release_read(&mut self) {
        if self.read.get().is_some() {
            self.bytes = None;
        }
    }
}

/// Two elements are one where their bytes are, or, for the ones the parser
/// read, where what it read is: whether another intent has read one yet is
/// not a property of the profile.
impl<T: PartialEq> PartialEq for Element<T> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.bytes, &other.bytes) {
            (Some(ours), Some(theirs)) => ours == theirs,
            (None, None) => self.read.get() == other.read.get(),
            _ => false,
        }
    }
}

/// Where one intent's transform comes from.
#[derive(Clone, Debug)]
enum Source<'a> {
    /// The pipeline at this index of [`Tables`]'s distinct elements, reached
    /// through the `D2B*` tag of number `tag`, or a version 5 profile's
    /// `A2B*`.
    Pipeline {
        tag: usize,
        index: usize,
        pipeline: Arc<mpe::Pipeline>,
    },
    /// The pipeline at this index of [`Tables`]'s spectral elements, reached
    /// through a version 5 profile's spectral `D2B*` tag of number `tag` and
    /// reduced to XYZ.
    Spectral {
        tag: usize,
        index: usize,
        pipeline: Arc<mpe::Pipeline>,
    },
    /// The table at this index of [`Tables`]'s distinct elements, reached
    /// through the `A2B*` tag of number `tag`.
    Table {
        tag: usize,
        index: usize,
        lut: Arc<Lut>,
    },
    /// The profile's matrix/TRC or grey model.
    Model(&'a Model),
}

impl Source<'_> {
    /// Which source this is, for telling whether two intents share one.
    fn id(&self) -> SourceId {
        match self {
            Source::Pipeline { index, .. } => SourceId::Pipeline(*index),
            Source::Spectral { index, .. } => SourceId::Spectral(*index),
            Source::Table { index, .. } => SourceId::Table(*index),
            Source::Model(_) => SourceId::Model,
        }
    }

    /// Whether the source's connection-space value is ICC-absolute already:
    /// `D2B3`'s (6.2.3, 9.2), a version 5 profile's `A2B3` (ICC.2 9.2.4) and
    /// its spectral `D2B3` (9.2.80). Every other tag, and a model (Table 25),
    /// is media-relative.
    fn absolute(&self) -> bool {
        matches!(
            self,
            Source::Pipeline { tag: 3, .. }
                | Source::Spectral { tag: 3, .. }
                | Source::Table { tag: 3, .. }
        )
    }
}

/// A [`Source`], by identity alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceId {
    Pipeline(usize),
    Spectral(usize),
    Table(usize),
    Model,
}

impl Profile {
    /// The connection-space value `components` convert to in `intent`, in
    /// the profile's own connection space — D50 XYZ with the PCS white's `Y`
    /// at one, or `L*`, `a*` and `b*` as they stand: through the source the
    /// intent's transform converts through ([`Intent::order`]), its pipeline
    /// evaluated with no grid between, and ICC-absolute colorimetric's scaled
    /// channel by channel in XYZ by the media white over the PCS white
    /// except through `D2B3` (6.2.3). What a transform out of another
    /// profile's connection space takes ([`Output::apply`]); `None` where
    /// the intent has nothing to convert through.
    #[must_use]
    pub fn connection_value(&self, intent: Intent, components: &[f64]) -> Option<[f64; 3]> {
        let source = self.source(intent)?;
        let own = &self.pcs == b"Lab ";
        let (pcs, lab) = match &source {
            Source::Table { lut, .. } => (lut.connection(components), lut.lab),
            Source::Pipeline { pipeline, .. } | Source::Spectral { pipeline, .. } => {
                (pipeline.evaluate(components), pipeline.lab())
            }
            Source::Model(model) => (model.connection(components), own),
        };
        let then = self.tables.to_standard().ok().flatten();
        let (pcs, lab) = standardised(pcs, lab, then.as_deref());
        let pcs = encoded(pcs, lab, own);
        Some(match self.scale_for(intent, source.absolute()) {
            Some(scale) => output::scale_connection(pcs, own, scale),
            None => pcs,
        })
    }

    /// Where `intent`'s transform comes from: the first of
    /// [`Intent::order`]'s sources the profile carries that reads, a table or
    /// pipeline read now if no intent has read it yet. One that does not read
    /// is passed over, as ICC.1 8.10.2 passes over a tag the CMM does not
    /// support. An intent whose order finds nothing — a profile whose only
    /// source is a `D2B*` tag of another intent's — converts as relative
    /// colorimetric does, whose source [`Profile::parse`] found.
    fn source(&self, intent: Intent) -> Option<Source<'_>> {
        self.walk(intent).or_else(|| {
            (intent != Intent::RelativeColorimetric)
                .then(|| self.walk(Intent::RelativeColorimetric))
                .flatten()
        })
    }

    /// The first of `intent`'s own sources that reads and connects: in a
    /// version 5 profile's own viewing conditions, where they are not the
    /// standard ones, a table or pipeline through its `c2sp`, and nothing
    /// where it carries none that reads — its model passed over, since a
    /// model's transform takes no stage after it — and its spectral `D2B*`
    /// tags last, in [`Intent::spectral_order`].
    fn walk(&self, intent: Intent) -> Option<Source<'_>> {
        self.tables.to_standard().ok()?;
        let custom = self.tables.custom.is_some();
        let colorimetric = intent.order().into_iter().find_map(|step| match step {
            Step::Pipeline(tag) => {
                let (index, element) = self.tables.pipeline_at(tag)?;
                let pipeline = self.tables.read_pipeline(element).ok()?;
                Some(Source::Pipeline {
                    tag,
                    index,
                    pipeline,
                })
            }
            Step::Table(tag) => match self.tables.at(tag) {
                Some((index, element)) => Some(Source::Table {
                    tag,
                    index,
                    lut: self.tables.read_table(element).ok()?,
                }),
                // A version 5 profile's `A2B*` may hold elements (ICC.2
                // 9.2.1), which read as a `D2B*` tag's do.
                None => {
                    let (index, element) = self.tables.table_pipeline_at(tag)?;
                    let pipeline = self.tables.read_pipeline(element).ok()?;
                    Some(Source::Pipeline {
                        tag,
                        index,
                        pipeline,
                    })
                }
            },
            Step::Model if custom => None,
            Step::Model => self.model.as_ref().map(Source::Model),
        });
        colorimetric.or_else(|| {
            intent.spectral_order().into_iter().find_map(|tag| {
                let (index, element) = self.tables.spectral_at(tag)?;
                let pipeline = self.tables.read_spectral(element).ok()?;
                Some(Source::Spectral {
                    tag,
                    index,
                    pipeline,
                })
            })
        })
    }
}

/// `pcs`, a connection-space value in XYZ or, where `lab`, in Lab, taken
/// through `then` where it is given — a `c2sp` from a version 5 profile's
/// own viewing conditions to the standard ones (ICC.2 9.2.57), which takes
/// and gives XYZ: the value, and whether it is in Lab after.
fn standardised(pcs: [f64; 3], lab: bool, then: Option<&mpe::Pipeline>) -> ([f64; 3], bool) {
    match then {
        None => (pcs, lab),
        Some(then) => {
            let xyz = if lab { lab_to_xyz(pcs) } else { pcs };
            (then.evaluate(&xyz), false)
        }
    }
}

/// `pcs`, in Lab where `lab` and in XYZ otherwise, in Lab where `own` and in
/// XYZ otherwise: a source's value in its profile's own connection space.
fn encoded(pcs: [f64; 3], lab: bool, own: bool) -> [f64; 3] {
    match (lab, own) {
        (false, true) => xyz_to_lab(pcs),
        (true, false) => lab_to_xyz(pcs),
        _ => pcs,
    }
}

/// `mft1` and `mft2`: three stages of table with a grid between them.
///
/// Each input channel goes through its own curve, the result indexes a
/// multi-dimensional grid by interpolation, and each output channel comes back
/// through another curve. The matrix ICC.1 puts in front is only applied when
/// the *input* is the connection space in XYZ (10.8, 10.9): never for an
/// `A2B*` tag, whose input is the device's, and for a `B2A*` tag over XYZ,
/// whose input is the connection space.
///
/// An `A2B*` table gives the connection space's three channels; a `B2A*`
/// table, the data space's, up to [`MAX_DEVICE_CHANNELS`].
///
/// v4's `mAB ` and `mBA ` read into it too: five optional stages, a curve
/// set either side of the grid and a matrix with M curves on one side of it
/// — after the grid in an `mAB `, before it in an `mBA `.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut {
    inputs: usize,
    outputs: usize,
    /// Grid points along each axis.
    ///
    /// `mft1` and `mft2` write one count for every axis and this is filled with
    /// it repeated; `mAB ` and `mBA ` write one per channel and they may
    /// differ. Carrying the vector either way is what lets one evaluator serve
    /// all four.
    grids: Vec<usize>,
    /// One curve per input channel, sampled. `mAB `'s A curves, `mBA `'s B
    /// curves.
    input_tables: Vec<Vec<u16>>,
    /// The product of `grids` entries of `outputs` values, the last axis
    /// fastest. Empty when a v4 tag declares no grid at all.
    clut: Vec<u16>,
    /// One curve per output channel, sampled. `mAB `'s B curves, `mBA `'s A
    /// curves, and the last stage every way.
    output_tables: Vec<Vec<u16>>,
    /// A v4 tag's M curves: in an `mAB ` between the grid and the matrix, in
    /// an `mBA ` between the matrix and the grid. `None` in a v2 tag, which
    /// has no such stage.
    m_tables: Option<Vec<Vec<u16>>>,
    /// A v4 tag's 3×3 matrix and its three offsets, its results clipped to
    /// `0..=1` (10.10.5, 10.11.3): in an `mAB ` after the M curves, in an
    /// `mBA ` after the B curves.
    ///
    /// Not `mft2`'s matrix, which sits at a fixed offset and applies only when
    /// the *input* is the connection space — at an `A2B*` tag it is not, which
    /// is why the v2 reader steps over it there.
    matrix: Option<[f64; 12]>,
    /// Which side of the grid [`Lut::matrix`] and [`Lut::m_tables`] run on.
    matrix_stage: MatrixStage,
    /// `mft1`'s and `mft2`'s 3×3 matrix, row by row, where the table's input
    /// is the connection space in XYZ — a `B2A*` table's — and the matrix is
    /// not the identity: applied to the XYZ before the input curves (10.8).
    input_matrix: Option<[f64; 9]>,
    /// Whether the connection space is `Lab` rather than `XYZ`.
    lab: bool,
    /// How the table encodes a Lab connection space in its currency.
    lab_scale: LabScale,
    /// How many bits the table's grid holds a value in: 8 for an `mft1` and
    /// a v4 grid of one byte an entry, 16 otherwise. A gamut tag's answer is
    /// read in it (ICC.1 9.2.28, [`Output::in_gamut`]).
    bits: u32,
}

/// How a table encodes `L*a*b*` in its 16-bit currency (ICC.1 6.3.4.2,
/// `docs/design/icc.md`, "The connection space each table type encodes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabScale {
    /// `lut16Type`'s, version 2's legacy encoding: `L*` 100 at `FF00h`, and
    /// `a*` and `b*` 0 at `8000h` and 127 at `FF00h`, the currency reaching a
    /// little past the space (note 3).
    Legacy,
    /// Each component's range over the whole currency: `L*` 100 at `FFFFh`,
    /// `a*` and `b*` from −128 at 0 to 127 at `FFFFh` — `lutAToBType`'s and
    /// `lutBToAType`'s (10.10.1, 10.11.1), and `lut8Type`'s 8-bit encoding,
    /// `L*` 100 at `FFh` and 0 at `80h`, widened.
    Full,
}

/// Where a table's matrix and M curves run, which its type fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatrixStage {
    /// After the grid, on its three outputs: the M curves, then the matrix,
    /// then the B curves — `mAB `'s order (10.10.1), and `mft1`'s and
    /// `mft2`'s, which carry neither stage.
    AfterGrid,
    /// Before the grid, on the B curves' three outputs: the matrix, then the
    /// M curves, then the grid — `mBA `'s order (10.11.1).
    BeforeGrid,
}

impl LabScale {
    /// What the full currency is worth on this scale, over what it is on the
    /// whole range: 65 535 over 65 280 for the legacy encoding, one for the
    /// full.
    fn reach(self) -> f64 {
        match self {
            LabScale::Legacy => 65535.0 / 65280.0,
            LabScale::Full => 1.0,
        }
    }
}

/// Which way a table converts, which fixes what each of its sides is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    /// An `A2B*` table: the device's channels in, the connection space's
    /// three out.
    Forward,
    /// A `B2A*` table: the connection space's three in, the data space's
    /// `channels` out.
    Output {
        /// How many channels the profile's data space has.
        channels: usize,
    },
    /// A device link's `A2B0` (ICC.1 8.6): one device's channels in and
    /// another's out, with no connection space at either end.
    Link {
        /// How many channels the link's data space has.
        inputs: usize,
        /// How many channels the space its connection-space field names
        /// has: the device it converts to.
        outputs: usize,
    },
}

/// The most input channels a lookup table may declare, and the most channels
/// a multi-process element may take or give.
///
/// `mAB `'s and a `clut` element's grid headers have room for sixteen grid
/// counts, so sixteen is the format's own ceiling; the guard exists because
/// the channel count is one byte, or two in a multi-process element, and the
/// grid product is what [`MAX_CLUT_ENTRIES`] bounds (ruling 1). An `mft1` or
/// `mft2` is held to it as well, since one grid count for every axis lets two
/// points an axis name twenty inputs inside that bound.
const MAX_LUT_INPUTS: usize = 16;

/// The most inputs a table's grid is interpolated over as one weighted sum.
///
/// Each corner's weight is a product of one eighth-bit fraction an axis, so
/// over `n` inputs a weight reaches `256^n` and a channel's sum `256^n x
/// 65 535`: under 2^64 for six inputs and past it for seven. A table over
/// more — a seven- or eight-ink printer profile an XPS document embeds, say —
/// is interpolated one axis at a time instead ([`Lut::blend`]).
const EXACT_INPUTS: usize = 6;

/// The most grid points an axis may declare, and the most entries a CLUT may
/// hold.
///
/// `grid.pow(inputs)` is where this format branches: the grid count is one
/// byte and the input count another, so a profile can ask for 255^15 entries
/// in two bytes. The corpus's largest is 11 points over four channels — 14 641
/// entries — so the cap clears the thing the format is for by a wide margin
/// while refusing the arithmetic that would not fit a machine (ruling 1).
pub const MAX_CLUT_ENTRIES: usize = 1 << 22;

impl Lut {
    /// The table's output channels for `components`, each in `0..=1`, each
    /// output in `0..=1` of the table's currency: the connection space's
    /// three for an `A2B*` table, the device's for a `B2A*` one, in the first
    /// [`Lut::outputs`] places.
    ///
    /// Integer throughout, like the rest of the pixel path: the input curves
    /// and the grid interpolation are u32 arithmetic on 16-bit samples, and the
    /// only floats are the caller's own components on the way in.
    fn evaluate(&self, components: &[f64]) -> [f64; MAX_DEVICE_CHANNELS] {
        // Stage one: each channel through its own curve.
        let mut coords = Vec::with_capacity(self.inputs);
        for channel in 0..self.inputs {
            let value = components
                .get(channel)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let table = &self.input_tables[channel];
            coords.push(interpolate(table, value));
        }
        // `mBA `'s matrix and M curves, between its B curves and its grid.
        if self.matrix_stage == MatrixStage::BeforeGrid {
            self.before_grid(&mut coords);
        }

        // Stage two: the grid. Multilinear over `inputs` axes, which is
        // sixteen corners for the four-channel case the corpus is made of, and
        // each axis carries its own grid count because `mAB ` may say so.
        let mut mid = [0u16; MAX_DEVICE_CHANNELS];
        if self.clut.is_empty() {
            // A v4 tag with no CLUT passes each channel straight through, which
            // `read_mab` has already checked means as many outputs as inputs.
            for (channel, slot) in mid.iter_mut().enumerate().take(self.outputs) {
                *slot = coords.get(channel).copied().unwrap_or(0);
            }
        } else {
            let mut base = Vec::with_capacity(self.inputs);
            let mut frac = Vec::with_capacity(self.inputs);
            for (axis, coordinate) in coords.iter().enumerate() {
                let last = self.grids.get(axis).copied().unwrap_or(2) - 1;
                let scaled = f64::from(*coordinate) / 65535.0 * last as f64;
                let index = (scaled.floor() as usize).min(last.saturating_sub(1));
                base.push(index);
                frac.push(((scaled - index as f64) * 256.0).round().clamp(0.0, 256.0) as u64);
            }
            if self.inputs > EXACT_INPUTS {
                for (channel, slot) in mid.iter_mut().enumerate().take(self.outputs) {
                    *slot = self.blend(0, 0, channel, &base, &frac).min(65535) as u16;
                }
            } else {
                let corners = 1usize << self.inputs;
                let mut mixed = [0u64; MAX_DEVICE_CHANNELS];
                let mut total = 0u64;
                for corner in 0..corners {
                    let mut weight = 1u64;
                    let mut offset = 0usize;
                    for axis in 0..self.inputs {
                        let points = self.grids.get(axis).copied().unwrap_or(2);
                        let last = points - 1;
                        let high = corner & (1 << axis) != 0;
                        let step = usize::from(high);
                        weight *= if high { frac[axis] } else { 256 - frac[axis] };
                        let index = (base[axis] + step).min(last);
                        offset = offset * points + index;
                    }
                    if weight == 0 {
                        continue;
                    }
                    total += weight;
                    for (channel, slot) in mixed.iter_mut().enumerate().take(self.outputs) {
                        let at = offset * self.outputs + channel;
                        *slot += weight * u64::from(self.clut.get(at).copied().unwrap_or(0));
                    }
                }
                let total = total.max(1);
                for (channel, slot) in mid.iter_mut().enumerate().take(self.outputs) {
                    *slot = (mixed[channel] / total).min(65535) as u16;
                }
            }
        }

        // Stage three, `mAB ` only: the M curves and then the matrix. Both
        // are absent from every `mft1` and `mft2`, so this is a no-op there,
        // and an `mBA ` ran them before its grid; the matrix is three
        // channels', which a tag carrying one has.
        let after_grid = self.matrix_stage == MatrixStage::AfterGrid;
        let mut values = [0.0f64; MAX_DEVICE_CHANNELS];
        for (channel, slot) in values.iter_mut().enumerate().take(self.outputs) {
            let raw = f64::from(mid[channel]) / 65535.0;
            *slot = match &self.m_tables {
                Some(tables) if after_grid => match tables.get(channel) {
                    Some(table) => f64::from(interpolate(table, raw)) / 65535.0,
                    None => raw,
                },
                _ => raw,
            };
        }
        if let (Some(m), true) = (&self.matrix, after_grid) {
            let (x, y, z) = (values[0], values[1], values[2]);
            values[0] = m[0] * x + m[1] * y + m[2] * z + m[9];
            values[1] = m[3] * x + m[4] * y + m[5] * z + m[10];
            values[2] = m[6] * x + m[7] * y + m[8] * z + m[11];
        }

        // Stage four: each output channel back through its own curve.
        let mut out = [0.0f64; MAX_DEVICE_CHANNELS];
        for (channel, slot) in out.iter_mut().enumerate().take(self.outputs) {
            let value = values[channel].clamp(0.0, 1.0);
            let table = &self.output_tables[channel];
            *slot = f64::from(interpolate(table, value)) / 65535.0;
        }
        out
    }

    /// `mBA `'s middle stages, on `coords`, its B curves' three outputs in
    /// the 16-bit currency: the matrix and its offsets, each result clipped
    /// to `0..=1` (10.11.3), then each channel through its M curve (10.11.4),
    /// back in the currency the grid reads. A stage the tag does not carry
    /// passes its channels through.
    fn before_grid(&self, coords: &mut [u16]) {
        let mut values = [0.0f64; 3];
        for (slot, coordinate) in values.iter_mut().zip(coords.iter()) {
            *slot = f64::from(*coordinate) / 65535.0;
        }
        if let Some(m) = &self.matrix {
            let [x, y, z] = values;
            values = [0, 1, 2].map(|row| {
                (m[3 * row] * x + m[3 * row + 1] * y + m[3 * row + 2] * z + m[9 + row])
                    .clamp(0.0, 1.0)
            });
        }
        let curves = self.m_tables.as_deref().unwrap_or(&[]);
        for (channel, coordinate) in coords.iter_mut().enumerate().take(3) {
            *coordinate = match curves.get(channel) {
                Some(table) => interpolate(table, values[channel]),
                None => (values[channel] * 65535.0).round() as u16,
            };
        }
    }

    /// Channel `channel` of the grid interpolated over the axes from `axis`
    /// on, the axes before it fixed at the node index `offset` they make:
    /// linearly along `axis` between the cell's two nodes in the eighth-bit
    /// fractions the weighted sum uses, rounding down as it does.
    ///
    /// Multilinear interpolation one axis at a time, so nothing is a product
    /// of more than two numbers and a grid over any number of inputs fits;
    /// the depth is the input count, which `MAX_LUT_INPUTS` bounds.
    fn blend(
        &self,
        axis: usize,
        offset: usize,
        channel: usize,
        base: &[usize],
        frac: &[u64],
    ) -> u64 {
        let Some(&points) = self.grids.get(axis) else {
            let at = offset * self.outputs + channel;
            return u64::from(self.clut.get(at).copied().unwrap_or(0));
        };
        let cell = base.get(axis).copied().unwrap_or(0);
        let fraction = frac.get(axis).copied().unwrap_or(0).min(256);
        let low = self.blend(axis + 1, offset * points + cell, channel, base, frac);
        if fraction == 0 {
            return low;
        }
        let next = (cell + 1).min(points - 1);
        let high = self.blend(axis + 1, offset * points + next, channel, base, frac);
        (low * (256 - fraction) + high * fraction) / 256
    }

    /// The sRGB this table's connection-space value is, that value first
    /// scaled channel by channel in XYZ where `media` says so — ICC-absolute
    /// colorimetry's media white (6.2.3).
    fn to_rgb(&self, components: &[f64], media: Option<[f64; 3]>) -> (u8, u8, u8) {
        let [r, g, b] = self.to_linear(components, media);
        let encoding = self.encoding();
        (encoding.encode(r), encoding.encode(g), encoding.encode(b))
    }

    /// [`Lut::to_rgb`] at sixteen bits: the same light, each channel held to
    /// the byte [`Lut::to_rgb`] encodes it as ([`Encoding::encode16`]).
    fn to_rgb16(&self, components: &[f64], media: Option<[f64; 3]>) -> [u16; 3] {
        let light = self.to_linear(components, media);
        let encoding = self.encoding();
        light.map(|channel| encoding.encode16(channel))
    }

    /// The linear sRGB this table's connection-space value is, scaled by
    /// `media` as [`Lut::to_rgb`] scales it, before it is encoded and
    /// unclamped: the light [`Lut::to_rgb`] encodes, and what a grid's node
    /// holds (`docs/design/icc.md`, P-22, "What a node holds").
    fn to_linear(&self, components: &[f64], media: Option<[f64; 3]>) -> [f64; 3] {
        #[cfg(test)]
        tests::PIPELINE_EVALUATIONS.with(|count| count.set(count.get() + 1));
        let encoded = self.evaluate(components);
        pcs_to_linear(
            self.decode_connection([encoded[0], encoded[1], encoded[2]]),
            self.lab,
            media,
        )
    }

    /// The connection-space value an `A2B*` table's three outputs encode, in
    /// its own units: D50 XYZ with the PCS white's `Y` at one, or `L*`, `a*`
    /// and `b*` as they stand.
    ///
    /// XYZ in `u1Fixed15`, 1.0 at `8000h` of the currency, so its full scale
    /// is just under two (10.8, 10.9: `mft1`'s XYZ is the implementation's
    /// own, and read as its widening). Lab by the table's own scale: an
    /// `mft2`'s version 2 legacy encoding, whose full scale is `FF00h` so that
    /// an eight-bit `FFh` doubles to it exactly; an `mft1`'s 8-bit one, `L*`
    /// 100 at `FFh`; an `mAB `'s or `mBA `'s each component's range over the
    /// whole currency, `L*` 100 at `FFFFh`. Getting the scale wrong shifts
    /// every hue by a little under half a percent of the range — a plausible
    /// picture in slightly wrong colours, which is the failure this module's
    /// refusals exist to avoid.
    fn decode_connection(&self, encoded: [f64; 3]) -> [f64; 3] {
        if self.lab {
            let reach = self.lab_scale.reach();
            [
                encoded[0] * reach * 100.0,
                encoded[1] * reach * 255.0 - 128.0,
                encoded[2] * reach * 255.0 - 128.0,
            ]
        } else {
            let scale = 65535.0 / 32768.0;
            encoded.map(|value| value * scale)
        }
    }

    /// A connection-space value in its own units, as a `B2A*` table's three
    /// inputs take it, each in `0..=1` of the currency: [`Lut::decode_connection`]
    /// run backwards, XYZ first through the table's matrix where it carries
    /// one (10.8). A component that is not a number is zero.
    fn encode_connection(&self, pcs: [f64; 3]) -> [f64; 3] {
        let finite = pcs.map(|value| if value.is_nan() { 0.0 } else { value });
        let encoded = if self.lab {
            let reach = self.lab_scale.reach();
            [
                finite[0] / 100.0 / reach,
                (finite[1] + 128.0) / 255.0 / reach,
                (finite[2] + 128.0) / 255.0 / reach,
            ]
        } else {
            let xyz = match &self.input_matrix {
                Some(e) => [0, 1, 2].map(|row| {
                    e[3 * row] * finite[0] + e[3 * row + 1] * finite[1] + e[3 * row + 2] * finite[2]
                }),
                None => finite,
            };
            let scale = 32768.0 / 65535.0;
            xyz.map(|value| value * scale)
        };
        encoded.map(|value| value.clamp(0.0, 1.0))
    }

    /// How many channels the table gives.
    const fn outputs(&self) -> usize {
        self.outputs
    }

    /// The connection-space value an `A2B*` table gives `components`: its
    /// pipeline and its encoding read back, no grid between.
    fn connection(&self, components: &[f64]) -> [f64; 3] {
        let encoded = self.evaluate(components);
        self.decode_connection([encoded[0], encoded[1], encoded[2]])
    }

    /// How this table's light is encoded: as `lab_to_rgb` encodes it under a
    /// Lab connection space, as [`encode_component`] does under XYZ.
    const fn encoding(&self) -> Encoding {
        Encoding::of(self.lab)
    }
}

/// The last stage of a lookup-table transform's pipeline: its linear sRGB
/// encoded as a byte, which differs by connection space because the two
/// paths into sRGB were written separately and each is what a page draws
/// today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    /// IEC 61966-2-1's transfer function through `tinker_pdf_math::pow`, as
    /// `lab_to_rgb` encodes a Lab connection space's light.
    Lab,
    /// [`encode_component`]'s 4 096-entry table, as an XYZ connection space's
    /// light is encoded.
    Xyz,
}

impl Encoding {
    /// The encoding of a connection space: Lab's where `lab`, XYZ's
    /// otherwise.
    const fn of(lab: bool) -> Encoding {
        if lab {
            Encoding::Lab
        } else {
            Encoding::Xyz
        }
    }

    /// One channel of linear light as the pipeline encodes it.
    fn encode(self, linear: f64) -> u8 {
        match self {
            Encoding::Lab => crate::srgb_encode(linear),
            Encoding::Xyz => encode_component(linear),
        }
    }

    /// One channel of linear light at sixteen bits: IEC 61966-2-1's transfer
    /// of it, held to the byte [`Encoding::encode`] gives it, so its top byte
    /// is that byte under either encoding (`docs/design/sixteen-bit.md`).
    fn encode16(self, linear: f64) -> u16 {
        crate::held16(crate::srgb_transfer(linear), self.encode(linear))
    }

    /// This encoding sampled at every 16-bit value of light, built once: what
    /// a grid's read encodes its interpolated light through.
    fn table(self) -> &'static EncodingTable {
        static LAB: OnceLock<EncodingTable> = OnceLock::new();
        static XYZ: OnceLock<EncodingTable> = OnceLock::new();
        let slot = match self {
            Encoding::Lab => &LAB,
            Encoding::Xyz => &XYZ,
        };
        slot.get_or_init(|| EncodingTable::sampled(self))
    }
}

/// An [`Encoding`] at each of the 65 536 values of light a grid's read can
/// hold, where 65 535 is 1.0, and for each byte the run of values that
/// encode to it.
struct EncodingTable {
    /// The byte the encoding gives each value.
    bytes: Box<[u8]>,
    /// For each byte, the first and last value that encode to it; the first
    /// above the last for a byte nothing encodes to.
    runs: [(u16, u16); 256],
}

impl EncodingTable {
    /// The table of `encoding`, each entry the pipeline's own encoding of its
    /// value, so a read encodes as the pipeline does.
    fn sampled(encoding: Encoding) -> EncodingTable {
        let bytes: Box<[u8]> = (0..=u16::MAX)
            .map(|value| encoding.encode(f64::from(value) / LIGHT_ONE as f64))
            .collect();
        let mut runs = [(u16::MAX, 0u16); 256];
        for (value, &byte) in (0..=u16::MAX).zip(bytes.iter()) {
            let run = &mut runs[usize::from(byte)];
            if run.0 > run.1 {
                *run = (value, value);
            } else {
                run.1 = value;
            }
        }
        EncodingTable { bytes, runs }
    }

    /// The byte light `value` encodes to, `value` clamped to `0..=1`.
    fn encode(&self, value: i64) -> u8 {
        let index = value.clamp(0, LIGHT_ONE) as usize;
        self.bytes.get(index).copied().unwrap_or(0)
    }
}

/// 1.0 of light, as a grid's node and its read hold it.
const LIGHT_ONE: i64 = 65535;

/// A grid's nodes along each axis, for a transform over one, three or four
/// inputs, and `None` for any other count, which keeps today's evaluation
/// (`docs/design/icc.md`, P-22, "The grid's size").
const fn grid_points(inputs: usize) -> Option<usize> {
    match inputs {
        1 => Some(256),
        3 => Some(33),
        4 => Some(17),
        _ => None,
    }
}

/// Bits a channel of a node's light takes in its word: signed, so from
/// `-2^20` to `2^20 - 1`, which with [`LIGHT_ONE`] as 1.0 is from −16 to 16.
const CHANNEL_BITS: u32 = 21;

/// The bit that says a node has been evaluated.
const FILLED: u64 = 1 << 63;

/// What a grid samples, up to the linear sRGB its nodes hold: a transform's
/// `A2B*` table, or its `D2B*` tag's elements (`docs/design/icc.md`, P-22,
/// "Which transforms a grid stands in for").
#[derive(Clone, Copy)]
enum Light<'a> {
    /// A sampled table, and the `c2sp` its value connects through where its
    /// profile's viewing conditions are not the standard ones.
    Table(&'a Lut, Option<&'a mpe::Pipeline>),
    /// A multi-process pipeline, and the same.
    Elements(&'a mpe::Pipeline, Option<&'a mpe::Pipeline>),
}

impl Light<'_> {
    /// How many components the pipeline takes.
    fn inputs(self) -> usize {
        match self {
            Light::Table(lut, _) => lut.inputs,
            Light::Elements(pipeline, _) => pipeline.inputs(),
        }
    }

    /// How its light is encoded, which its connection space decides: XYZ's
    /// after a `c2sp`, which gives XYZ.
    fn encoding(self) -> Encoding {
        match self {
            Light::Table(lut, None) => lut.encoding(),
            Light::Elements(pipeline, None) => Encoding::of(pipeline.lab()),
            Light::Table(_, Some(_)) | Light::Elements(_, Some(_)) => Encoding::of(false),
        }
    }

    /// The pipeline's light at `components`, its connection-space value
    /// connected through its `c2sp` where it has one and scaled by `media`
    /// where that is given.
    fn at(self, components: &[f64], media: Option<[f64; 3]>) -> [f64; 3] {
        match self {
            Light::Table(lut, None) => lut.to_linear(components, media),
            Light::Table(lut, then) => {
                #[cfg(test)]
                tests::PIPELINE_EVALUATIONS.with(|count| count.set(count.get() + 1));
                let (pcs, lab) = standardised(lut.connection(components), lut.lab, then);
                pcs_to_linear(pcs, lab, media)
            }
            Light::Elements(pipeline, then) => {
                #[cfg(test)]
                tests::PIPELINE_EVALUATIONS.with(|count| count.set(count.get() + 1));
                let (pcs, lab) = standardised(pipeline.evaluate(components), pipeline.lab(), then);
                pcs_to_linear(pcs, lab, media)
            }
        }
    }
}

/// A lookup-table transform's pipeline — a table's, or a `D2B*` tag's
/// elements' — sampled at the nodes of an integer grid, and read between them
/// in fixed point (`docs/design/icc.md`, P-22).
///
/// Each node holds the pipeline's linear sRGB at the node — three signed
/// channels of [`CHANNEL_BITS`], unclamped, beside [`FILLED`] — in one atomic
/// word, evaluated the first time a read reaches it and kept for every read
/// after. A node is a function of the table, the conversion and its position
/// alone, so two threads that fill one at once store one value, and no read
/// depends on which nodes were filled before it.
struct Grid {
    /// One, three or four.
    inputs: usize,
    /// Nodes along each axis.
    points: usize,
    /// The pipeline's last stage, which the read runs after interpolating.
    encoding: Encoding,
    /// The media scale the pipeline applies to its connection-space value,
    /// where the transform is ICC-absolute colorimetry's (6.2.3): a stage the
    /// nodes evaluate, as [`Transform`]'s `media` is.
    media: Option<[f64; 3]>,
    /// `points.pow(inputs)` nodes, the last axis varying fastest, allocated
    /// at the first read.
    nodes: OnceLock<Box<[AtomicU64]>>,
}

impl Grid {
    /// The grid `source` converts through, its connection-space value scaled
    /// by `media` where that is given, or `None` for an input count no grid
    /// is built over.
    fn over(source: Light<'_>, media: Option<[f64; 3]>) -> Option<Grid> {
        Some(Grid {
            inputs: source.inputs(),
            points: grid_points(source.inputs())?,
            encoding: source.encoding(),
            media,
            nodes: OnceLock::new(),
        })
    }

    /// How many nodes the grid holds.
    fn count(&self) -> usize {
        // At most 17^4, so the product cannot overflow.
        self.points.pow(self.inputs as u32)
    }

    /// The node array, allocated now if no read has reached it yet.
    fn nodes(&self) -> &[AtomicU64] {
        self.nodes
            .get_or_init(|| (0..self.count()).map(|_| AtomicU64::new(0)).collect())
    }

    /// The inputs node `index` stands at: each axis's node over `0..=1`.
    fn node_input(&self, index: usize) -> [f64; 4] {
        let mut x = [0.0f64; 4];
        let mut rest = index;
        let last = (self.points - 1) as f64;
        for slot in x[..self.inputs].iter_mut().rev() {
            *slot = (rest % self.points) as f64 / last;
            rest /= self.points;
        }
        x
    }

    /// Node `index`'s word, filled: evaluated through `lut` now if no read
    /// has reached it before.
    fn node(&self, source: Light<'_>, nodes: &[AtomicU64], index: usize) -> [i64; 3] {
        let word = match nodes.get(index) {
            Some(slot) => {
                let held = slot.load(Ordering::Relaxed);
                if held & FILLED != 0 {
                    held
                } else {
                    let fresh = self.evaluate(source, index);
                    slot.store(fresh, Ordering::Relaxed);
                    fresh
                }
            }
            // An index past the grid is not reachable from `read`, whose
            // cells stop a node short of each axis's last; answered all the
            // same rather than indexed into.
            None => self.evaluate(source, index),
        };
        unpack(word)
    }

    /// Node `index` evaluated: the pipeline's light there, each channel
    /// within `0..=1` pulled to the value nearest it that encodes to the
    /// pipeline's own byte, so a read at the node is the pipeline exactly.
    fn evaluate(&self, source: Light<'_>, index: usize) -> u64 {
        let x = self.node_input(index);
        let light = source.at(&x[..self.inputs], self.media);
        let table = self.encoding.table();
        let mut word = FILLED;
        for (channel, light) in light.into_iter().enumerate() {
            let byte = self.encoding.encode(light);
            let held = node_value(light, byte, table);
            word |= pack(held) << (CHANNEL_BITS * channel as u32);
        }
        word
    }

    /// `components` converted through the grid: each located on its axis,
    /// the nodes around it interpolated in fixed point, the light encoded as
    /// the pipeline encodes it.
    fn read(&self, source: Light<'_>, components: &[f64]) -> (u8, u8, u8) {
        let (cells, fractions) = self.located(components);
        self.read_at(source, &cells, &fractions)
    }

    /// [`Grid::read`] at sixteen bits: the same interpolated light, each
    /// channel encoded as [`Grid::read`] encodes it and held to that byte
    /// (`docs/design/sixteen-bit.md`).
    fn read16(&self, source: Light<'_>, components: &[f64]) -> [u16; 3] {
        let (cells, fractions) = self.located(components);
        let table = self.encoding.table();
        let light = self.light_with(source, self.nodes(), &cells, &fractions);
        light.map(|value| {
            let byte = table.encode(value);
            let linear = value.clamp(0, LIGHT_ONE) as f64 / LIGHT_ONE as f64;
            crate::held16(crate::srgb_transfer(linear), byte)
        })
    }

    /// Each component located on its axis: its cell and its fraction in it.
    fn located(&self, components: &[f64]) -> ([usize; 4], [i64; 4]) {
        let mut cells = [0usize; 4];
        let mut fractions = [0i64; 4];
        for (axis, (cell, fraction)) in cells
            .iter_mut()
            .zip(fractions.iter_mut())
            .enumerate()
            .take(self.inputs)
        {
            let x = components.get(axis).copied().unwrap_or(0.0);
            (*cell, *fraction) = locate(x, self.points);
        }
        (cells, fractions)
    }

    /// The grid read at `cells` and `fractions`, one of each an input: the
    /// interpolation every conversion through the grid shares.
    fn read_at(&self, source: Light<'_>, cells: &[usize; 4], fractions: &[i64; 4]) -> (u8, u8, u8) {
        self.read_with(
            source,
            self.nodes(),
            self.encoding.table(),
            cells,
            fractions,
        )
    }

    /// [`Grid::read_at`] over `nodes`, this grid's, and `table`, its
    /// encoding's: what a row hoists out of its loop.
    fn read_with(
        &self,
        source: Light<'_>,
        nodes: &[AtomicU64],
        table: &EncodingTable,
        cells: &[usize; 4],
        fractions: &[i64; 4],
    ) -> (u8, u8, u8) {
        let light = self.light_with(source, nodes, cells, fractions);
        (
            table.encode(light[0]),
            table.encode(light[1]),
            table.encode(light[2]),
        )
    }

    /// The light the grid holds at `cells` and `fractions`, interpolated
    /// from `nodes` and not yet encoded, in [`LIGHT_ONE`]'s unit: what
    /// [`Grid::read_with`] encodes as bytes and [`Grid::read16`] at sixteen
    /// bits.
    fn light_with(
        &self,
        source: Light<'_>,
        nodes: &[AtomicU64],
        cells: &[usize; 4],
        fractions: &[i64; 4],
    ) -> [i64; 3] {
        let points = self.points;
        match self.inputs {
            1 => {
                let low = self.node(source, nodes, cells[0]);
                let high = self.node(source, nodes, cells[0] + 1);
                [0, 1, 2]
                    .map(|c| round_shift((low[c] << 16) + fractions[0] * (high[c] - low[c]), 16))
            }
            3 => {
                let base = (cells[0] * points + cells[1]) * points + cells[2];
                let strides = [points * points, points, 1];
                let sum = self.tetrahedral(
                    source,
                    nodes,
                    base,
                    strides,
                    [fractions[0], fractions[1], fractions[2]],
                );
                sum.map(|channel| round_shift(channel, 16))
            }
            _ => {
                // Tetrahedral over the first three at the fourth's two nodes
                // either side, then linear between them.
                let base = ((cells[0] * points + cells[1]) * points + cells[2]) * points + cells[3];
                let strides = [points * points * points, points * points, points];
                let three = [fractions[0], fractions[1], fractions[2]];
                let low = self.tetrahedral(source, nodes, base, strides, three);
                let high = self.tetrahedral(source, nodes, base + 1, strides, three);
                [0, 1, 2]
                    .map(|c| round_shift((low[c] << 16) + fractions[3] * (high[c] - low[c]), 32))
            }
        }
    }

    /// From node `base`, a step along each of three axes in turn, the
    /// largest fraction first and the earlier axis first between equals,
    /// each node reached weighed by its step's fraction against the one
    /// before: the light, in 2^-16 of [`LIGHT_ONE`]'s unit.
    fn tetrahedral(
        &self,
        source: Light<'_>,
        nodes: &[AtomicU64],
        base: usize,
        strides: [usize; 3],
        fractions: [i64; 3],
    ) -> [i64; 3] {
        let mut at = base;
        let mut previous = self.node(source, nodes, at);
        let mut sum = previous.map(|value| value << 16);
        for axis in largest_first(fractions) {
            at += strides[axis];
            let next = self.node(source, nodes, at);
            for ((slot, now), before) in sum.iter_mut().zip(next).zip(previous) {
                *slot += fractions[axis] * (now - before);
            }
            previous = next;
        }
        sum
    }
}

/// A grid's nodes are a cache of its pipeline, so a copy starts with none
/// filled and reads exactly as the original does.
impl Clone for Grid {
    fn clone(&self) -> Self {
        Grid {
            inputs: self.inputs,
            points: self.points,
            encoding: self.encoding,
            media: self.media,
            nodes: OnceLock::new(),
        }
    }
}

/// The shape and not the nodes: 83 521 words say nothing a reader needs.
impl std::fmt::Debug for Grid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grid")
            .field("inputs", &self.inputs)
            .field("points", &self.points)
            .field("encoding", &self.encoding)
            .field("media", &self.media)
            .field("allocated", &self.nodes.get().is_some())
            .finish()
    }
}

/// Where `x` falls on an axis of `points` nodes: the lower node of its cell,
/// and the fraction past it in 16 bits, 65 536 at the top of the last cell.
/// `x` is clamped to `0..=1`, and a NaN locates at 0.
fn locate(x: f64, points: usize) -> (usize, i64) {
    let last = points - 1;
    let scaled = (x.clamp(0.0, 1.0) * last as f64 * 65536.0).round();
    // `as` saturates and takes a NaN to 0.
    let scaled = scaled as i64;
    let cell = (scaled >> 16) as usize;
    if cell >= last {
        (last - 1, 65536)
    } else {
        (cell, scaled & 0xFFFF)
    }
}

/// The three axes in the order a tetrahedral read steps along them: the
/// largest fraction first, the earlier axis first between equals.
fn largest_first(fractions: [i64; 3]) -> [usize; 3] {
    let mut order = [0usize, 1, 2];
    if fractions[order[1]] > fractions[order[0]] {
        order.swap(0, 1);
    }
    if fractions[order[2]] > fractions[order[1]] {
        order.swap(1, 2);
        if fractions[order[1]] > fractions[order[0]] {
            order.swap(0, 1);
        }
    }
    order
}

/// `value`, a fixed-point number with `bits` fractional bits, rounded to a
/// whole one, a half rounding up.
const fn round_shift(value: i64, bits: u32) -> i64 {
    (value + (1 << (bits - 1))) >> bits
}

/// One channel of a node's light as the grid holds it: the light in units
/// of [`LIGHT_ONE`], held within [`CHANNEL_BITS`]; where it is within
/// `0..=1`, the value nearest it among those `table` encodes to `byte`, the
/// pipeline's own byte for it.
fn node_value(light: f64, byte: u8, table: &EncodingTable) -> i64 {
    let limit = 1i64 << (CHANNEL_BITS - 1);
    // `as` saturates, and takes a NaN to 0.
    let value = ((light * LIGHT_ONE as f64).round() as i64).clamp(-limit, limit - 1);
    if (0..=LIGHT_ONE).contains(&value) {
        let (low, high) = table.runs[usize::from(byte)];
        if low <= high {
            return value.clamp(i64::from(low), i64::from(high));
        }
    }
    value
}

/// One channel's value in its [`CHANNEL_BITS`], offset to be unsigned.
fn pack(value: i64) -> u64 {
    let offset = value + (1i64 << (CHANNEL_BITS - 1));
    (offset as u64) & ((1u64 << CHANNEL_BITS) - 1)
}

/// A filled node's three channels.
fn unpack(word: u64) -> [i64; 3] {
    let mask = (1u64 << CHANNEL_BITS) - 1;
    [0u32, 1, 2].map(|channel| {
        let offset = (word >> (CHANNEL_BITS * channel)) & mask;
        offset as i64 - (1i64 << (CHANNEL_BITS - 1))
    })
}

/// A sampled table at `x` in `0..=1`, linearly interpolated.
fn interpolate(table: &[u16], x: f64) -> u16 {
    if table.is_empty() {
        return (x.clamp(0.0, 1.0) * 65535.0).round() as u16;
    }
    if table.len() == 1 {
        return table[0];
    }
    let last = table.len() - 1;
    let position = x.clamp(0.0, 1.0) * last as f64;
    let floor = position.floor();
    let index = (floor as usize).min(last);
    let next = (index + 1).min(last);
    let fraction = position - floor;
    let a = f64::from(table[index]);
    let b = f64::from(table[next]);
    (a + (b - a) * fraction).round() as u16
}

/// A connection-space value in its own units — XYZ with the PCS white's `Y`
/// at one, or `L*`, `a*` and `b*` as they are, the float encoding a `D2B*`
/// tag gives (6.3.4.2) — as linear sRGB before it is encoded and unclamped,
/// scaled channel by channel in XYZ first where `media` says so (equations 4
/// to 6): a Lab value to XYZ by the relation `lab_to_linear_srgb` runs, then
/// `XYZ_D50_TO_SRGB`. A table's value and a pipeline's both go this way.
fn pcs_to_linear(pcs: [f64; 3], lab: bool, media: Option<[f64; 3]>) -> [f64; 3] {
    let [x, y, z] = if lab {
        crate::lab_to_xyz(pcs[0], pcs[1], pcs[2])
    } else {
        pcs
    };
    match media {
        None => crate::xyz_d50_to_linear_srgb(x, y, z),
        Some(scale) => crate::xyz_d50_to_linear_srgb(x * scale[0], y * scale[1], z * scale[2]),
    }
}

/// [`pcs_to_linear`]'s light as sRGB bytes, encoded as its connection
/// space's tables encode theirs.
fn pcs_to_rgb(pcs: [f64; 3], lab: bool, media: Option<[f64; 3]>) -> (u8, u8, u8) {
    let [r, g, b] = pcs_to_linear(pcs, lab, media);
    let encoding = Encoding::of(lab);
    (encoding.encode(r), encoding.encode(g), encoding.encode(b))
}

/// [`pcs_to_rgb`] at sixteen bits: the same light, each channel held to the
/// byte [`pcs_to_rgb`] encodes it as.
fn pcs_to_rgb16(pcs: [f64; 3], lab: bool, media: Option<[f64; 3]>) -> [u16; 3] {
    let light = pcs_to_linear(pcs, lab, media);
    let encoding = Encoding::of(lab);
    light.map(|channel| encoding.encode16(channel))
}

/// Linear light to an sRGB byte, through the same table the matrix path uses.
fn encode_component(linear: f64) -> u8 {
    let clamped = (linear.clamp(0.0, 1.0) * 65536.0) as i64;
    encode_srgb(clamped)
}

/// A big-endian reader that answers `None` rather than panicking.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn u32(&self, at: usize) -> Option<u32> {
        let s = self.bytes.get(at..at.checked_add(4)?)?;
        Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let s = self.bytes.get(at..at.checked_add(2)?)?;
        Some(u16::from_be_bytes([s[0], s[1]]))
    }

    fn sig(&self, at: usize) -> Option<[u8; 4]> {
        let s = self.bytes.get(at..at.checked_add(4)?)?;
        Some([s[0], s[1], s[2], s[3]])
    }

    /// ICC.1's `s15Fixed16Number`: a signed 16.16 fixed-point value.
    fn s15fixed16(&self, at: usize) -> Option<f64> {
        Some(f64::from(self.u32(at)? as i32) / 65536.0)
    }
}

impl Profile {
    /// Reads a profile far enough to build a transform from it.
    ///
    /// # Errors
    ///
    /// Every failure is an [`IccError`] naming what was wrong, and every one of
    /// them leaves the caller with the component-count approximation it had
    /// before — a profile this cannot read never degrades a page, it only fails
    /// to improve one.
    pub fn parse(bytes: &[u8]) -> Result<Profile, IccError> {
        if bytes.len() > MAX_ICC_BYTES {
            return Err(IccError::TooLarge);
        }
        // 128 bytes of header, then a four-byte tag count.
        if bytes.len() < 132 {
            return Err(IccError::TooShort);
        }
        let reader = Reader { bytes };
        if reader.sig(36) != Some(*b"acsp") {
            return Err(IccError::NotAProfile);
        }
        // The header's own size, which a truncated stream disagrees with. A
        // profile longer than its field is tolerated — trailing bytes after a
        // profile are somebody else's — but a shorter one has lost data every
        // tag offset below is about to index into.
        let declared = reader.u32(0).ok_or(IccError::TooShort)? as usize;
        if declared > bytes.len() {
            return Err(IccError::SizeMismatch);
        }

        let class = reader.sig(12).ok_or(IccError::TooShort)?;
        let space = reader.sig(16).ok_or(IccError::TooShort)?;
        let pcs = reader.sig(20).ok_or(IccError::TooShort)?;
        let version = (reader.u32(8).ok_or(IccError::TooShort)? >> 24) as u8;
        // Bytes 100 to 123 are a version 5 profile's (ICC.2 7.2.2) and
        // reserved in a version 2 or 4 one; the header is all there, so they
        // read.
        let version5 = (version >= 5)
            .then(|| {
                Some(Version5 {
                    spectral_pcs: reader.sig(100)?,
                    spectral_range: SpectralRange::read(&reader, 104)?,
                    bispectral_range: SpectralRange::read(&reader, 110)?,
                    mcs: reader.sig(116)?,
                    subclass: reader.sig(120)?,
                })
            })
            .flatten();

        let count = reader.u32(128).ok_or(IccError::TooShort)?;
        if count > MAX_ICC_TAGS {
            return Err(IccError::TooLarge);
        }
        let mut tags: Vec<([u8; 4], usize, usize)> = Vec::new();
        for i in 0..count as usize {
            let at = 132 + i.checked_mul(12).ok_or(IccError::TooLarge)?;
            let signature = reader.sig(at).ok_or(IccError::TooShort)?;
            let offset = reader.u32(at + 4).ok_or(IccError::TooShort)? as usize;
            let size = reader.u32(at + 8).ok_or(IccError::TooShort)? as usize;
            let end = offset.checked_add(size).ok_or(IccError::TagOutOfBounds)?;
            if end > bytes.len() {
                return Err(IccError::TagOutOfBounds);
            }
            tags.push((signature, offset, size));
        }

        let find = |want: &[u8; 4]| {
            tags.iter()
                .find(|(s, _, _)| s == want)
                .map(|(_, offset, size)| &bytes[*offset..*offset + *size])
        };
        let place = |want: &[u8; 4]| {
            tags.iter()
                .find(|(s, _, _)| s == want)
                .map(|&(_, offset, size)| (offset, size))
        };
        // The media white, which only ICC-absolute colorimetry reads
        // (6.2.3): one that is absent or does not read leaves the profile
        // with the PCS white for it (`Profile::media_scale`), and refuses
        // nothing a relative transform could be built from.
        let white = find(b"wtpt").and_then(|data| read_xyz(data).ok());
        let lab = &pcs == b"Lab ";
        // Named colours (10.15), their coordinates in the profile's
        // connection space: what a profile of class `nmcl` is (8.9), so one
        // whose tag is absent or does not read is refused for it; any other
        // profile carries them beside its transforms, and one that does not
        // read refuses nothing a transform could be built from.
        let named = find(b"ncl2").map(|data| NamedColours::read(data, lab));
        if &class == b"nmcl" {
            if !lab && &pcs != b"XYZ " {
                return Err(IccError::UnsupportedPcs);
            }
            let named = named.ok_or(IccError::MissingTags)??;
            return Ok(Profile {
                space,
                pcs,
                class,
                version,
                model: None,
                tables: Tables::none(lab),
                white,
                named: Some(named),
                version5,
            });
        }
        let named = named.and_then(Result::ok);

        // A device link (8.6) converts one device's values to another's: its
        // connection-space field names the space its transform gives, a
        // device's (7.2.7), and it carries one `AToB0`, or a `DToB0` that
        // 8.10.3 a) reads first. It is read now, so a link nothing in it
        // converts through is refused here, and it is never an intent's
        // source: no transform to sRGB is built from it.
        if &class == b"link" {
            let inputs = channels(space).ok_or(IccError::UnsupportedSpace)?;
            let outputs = channels(pcs).ok_or(IccError::UnsupportedSpace)?;
            let mut link = LinkTags::locate(
                usize::from(inputs),
                usize::from(outputs),
                place(b"A2B0"),
                place(b"D2B0"),
                bytes,
            )?;
            link.source()?;
            link.release_read();
            let mut tables = Tables::none(false);
            tables.link = Some(link);
            return Ok(Profile {
                space,
                pcs,
                class,
                version,
                model: None,
                tables,
                white,
                named: None,
                version5,
            });
        }

        let has_matrix =
            find(b"rXYZ").is_some() && find(b"gXYZ").is_some() && find(b"bXYZ").is_some();
        let has_grey = find(b"kTRC").is_some();

        // The matrix/TRC or grey model, where the profile carries the tags of
        // one: ICC.1 8.10.2 d), read whether or not relative colorimetric
        // reaches it, since an intent whose tables the profile does not
        // carry converts through it. One that contradicts the header is kept
        // as its refusal, so a profile nothing else serves is refused for it.
        let model = (has_matrix || has_grey).then(|| read_model(&find, space, pcs, has_matrix));
        // A table converts into the connection space, which it reads as XYZ
        // or as Lab. A profile carrying no model and naming neither is
        // refused; one carrying a model has its tables passed over, and the
        // model, which reads only those two, refuses it if it is all there is.
        let tables_convert = lab || &pcs == b"XYZ ";
        // A version 5 profile's viewing conditions (ICC.2 9.2.111), which its
        // colorimetric and its spectral connection both are in; where they
        // are not the standard ones, its transforms connect to the standard
        // ones through its `c2sp` and `s2cp` (10.2.22). Its spectral
        // connection space (7.2.21), where its header names one, reduces to
        // XYZ against them, or why not. An `svcn` that does not read leaves
        // the colorimetric tags in the standard conditions and refuses the
        // spectral ones for its reason.
        let v5 = version >= 5;
        let viewing = if v5 {
            find(b"svcn").map(ViewingConditions::read)
        } else {
            None
        };
        let custom = matches!(&viewing, Some(Ok(conditions)) if !conditions.standard());
        let reduction = version5
            .filter(|header| v5 && header.spectral_pcs != [0; 4])
            .map(|header| {
                spectral::channels(&header)?;
                match &viewing {
                    None => Err(IccError::Spectral(SpectralRefusal::NoViewingConditions)),
                    Some(Err(error)) => Err(*error),
                    Some(Ok(conditions)) => conditions.reduction(header.spectral_range),
                }
            });
        // A spectral-only profile's connection-space field is zero (7.2.21),
        // and its spectral tags are what it converts through.
        if model.is_none() && !tables_convert {
            match &reduction {
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(*error),
                None => return Err(IccError::UnsupportedPcs),
            }
        }
        // A version 5 profile adds `A2B3` and `B2A3` (ICC.2 9.2.4, 9.2.41),
        // and its `A2B*` and `B2A*` tags may hold elements; its `D2B*` and
        // `B2D*` tags take a spectral connection space instead (9.2.42,
        // 9.2.77), which its `D2B*` tags reduce from below and its `B2D*`
        // tags, which take a spectrum, are not read for.
        let tags_of = |names: [&[u8; 4]; 4], keep: [bool; 4]| {
            [0, 1, 2, 3].map(|tag| place(names[tag]).filter(|_| keep[tag]))
        };
        let mut tables = if tables_convert {
            Tables::locate(
                Places {
                    a2b: tags_of([b"A2B0", b"A2B1", b"A2B2", b"A2B3"], [true, true, true, v5]),
                    d2b: tags_of([b"D2B0", b"D2B1", b"D2B2", b"D2B3"], [!v5; 4]),
                    b2a: tags_of([b"B2A0", b"B2A1", b"B2A2", b"B2A3"], [true, true, true, v5]),
                    b2d: tags_of([b"B2D0", b"B2D1", b"B2D2", b"B2D3"], [!v5; 4]),
                    gamut: place(b"gamt"),
                    multi_process: v5,
                },
                bytes,
                lab,
            )?
        } else {
            Tables::none(lab)
        };
        match reduction {
            Some(Ok(weights)) => {
                let (slots, elements) = Element::locate(
                    tags_of([b"D2B0", b"D2B1", b"D2B2", b"D2B3"], [true; 4]),
                    bytes,
                )?;
                tables.spectral_slots = slots;
                tables.spectral = elements;
                tables.weights = weights;
            }
            Some(Err(error)) => tables.spectral_refusal = Some(error),
            None => {}
        }
        if custom {
            let one = |name: &[u8; 4]| {
                Element::locate([place(name)], bytes).map(|(_, mut found)| found.pop())
            };
            tables.custom = Some(Custom {
                to_standard: one(b"c2sp")?,
                from_standard: one(b"s2cp")?,
            });
            // Every transform into the connection space connects through
            // the `c2sp`, so a profile with none that reads converts nothing.
            tables.to_standard()?;
        }

        // Relative colorimetric's source, found now: 8.6.5.8 makes it the
        // initial intent, so a profile with none is refused here, whatever
        // intent a page later asks for. Its `D2B1` comes first, its tables
        // next and its model after them, as 8.10.2 orders them
        // (`Intent::order`). A `D2B*` tag that does not read is passed over
        // always, 8.10.2 a)'s tag the CMM does not support; a table that does
        // not read is passed over where the profile carries a model. A
        // profile carrying none is refused for the first table the order
        // names when that table does not read: in a profile of tables alone,
        // a damaged table is a damaged profile.
        let mut found = false;
        let mut passed_over = None;
        for step in Intent::RelativeColorimetric.order() {
            match step {
                Step::Pipeline(tag) => {
                    let Some((_, pipeline)) = tables.pipeline_at(tag) else {
                        continue;
                    };
                    match tables.read_pipeline(pipeline) {
                        Ok(_) => found = true,
                        Err(error) => {
                            passed_over.get_or_insert(error);
                        }
                    }
                }
                Step::Table(tag) => {
                    if let Some((_, table)) = tables.at(tag) {
                        match tables.read_table(table) {
                            Ok(_) => found = true,
                            Err(error) if model.is_none() => return Err(error),
                            Err(_) => {}
                        }
                    } else if let Some((_, pipeline)) = tables.table_pipeline_at(tag) {
                        // A version 5 `A2B*` tag's elements, passed over as
                        // a `D2B*` tag's are where they do not read (ICC.2
                        // 10.2.17).
                        match tables.read_pipeline(pipeline) {
                            Ok(_) => found = true,
                            Err(error) => {
                                passed_over.get_or_insert(error);
                            }
                        }
                    }
                }
                // In viewing conditions other than the standard ones a model
                // is passed over: its transform takes no `c2sp` after it.
                Step::Model if custom => {
                    if matches!(model, Some(Ok(_))) {
                        passed_over.get_or_insert(IccError::Spectral(SpectralRefusal::CustomModel));
                    }
                }
                Step::Model => found = matches!(model, Some(Ok(_))),
            }
            if found {
                break;
            }
        }
        // A version 5 profile's spectral tags last, in ICC.2 8.14.2's order.
        if !found {
            for tag in Intent::RelativeColorimetric.spectral_order() {
                let Some((_, element)) = tables.spectral_at(tag) else {
                    continue;
                };
                match tables.read_spectral(element) {
                    Ok(_) => {
                        found = true;
                        break;
                    }
                    Err(error) => {
                        passed_over.get_or_insert(error);
                    }
                }
            }
        }
        if !found {
            // A profile carrying a model is refused for the model's own
            // reason, one whose only source was a pipeline that does not
            // read for that pipeline's, by name, one whose spectral tags are
            // not reduced for why not, and one carrying neither a model nor
            // a table for lacking both.
            return Err(match model {
                Some(Err(error)) => error,
                _ => passed_over
                    .or(tables.spectral_refusal)
                    .unwrap_or(IccError::MissingTags),
            });
        }
        tables.release_read();

        Ok(Profile {
            space,
            pcs,
            class,
            version,
            model: model.and_then(Result::ok),
            tables,
            white,
            named,
            version5,
        })
    }
}

/// A profile's matrix/TRC model, where `has_matrix`, and otherwise its grey
/// model, read from the tags `find` answers for, or why the header
/// contradicts it.
fn read_model<'a>(
    find: &impl Fn(&[u8; 4]) -> Option<&'a [u8]>,
    space: [u8; 4],
    pcs: [u8; 4],
    has_matrix: bool,
) -> Result<Model, IccError> {
    // A matrix profile's columns *are* XYZ by construction, so one declaring
    // any other connection space contradicts itself. A grey profile does not:
    // its one curve produces whatever the connection space's achromatic axis
    // is, `Y` under XYZ and `L*` under Lab, and both are ordinary.
    if has_matrix && &pcs != b"XYZ " {
        return Err(IccError::UnsupportedPcs);
    }
    if !has_matrix && &pcs != b"XYZ " && &pcs != b"Lab " {
        return Err(IccError::UnsupportedPcs);
    }
    if has_matrix {
        if &space != b"RGB " {
            return Err(IccError::UnsupportedSpace);
        }
        let column = |tag: &[u8; 4]| -> Result<[f64; 3], IccError> {
            read_xyz(find(tag).ok_or(IccError::MissingTags)?)
        };
        let curve = |tag: &[u8; 4]| -> Result<Curve, IccError> {
            read_curve(find(tag).ok_or(IccError::MissingTags)?)
        };
        Ok(Model::MatrixTrc {
            columns: [column(b"rXYZ")?, column(b"gXYZ")?, column(b"bXYZ")?],
            curves: [curve(b"rTRC")?, curve(b"gTRC")?, curve(b"bTRC")?],
        })
    } else {
        if &space != b"GRAY" {
            return Err(IccError::UnsupportedSpace);
        }
        Ok(Model::Grey {
            lab: &pcs == b"Lab ",
            curve: read_curve(find(b"kTRC").ok_or(IccError::MissingTags)?)?,
        })
    }
}

/// A lookup-table tag, read as the tag that names it converts — an `A2B*`
/// tag forward, a `B2A*` tag out of the connection space ([`Direction`]):
/// `mft1` or `mft2`, ICC.1's two v2 lookup tables, read here, or v4's `mAB `
/// forward ([`read_mab`]) and `mBA ` out ([`read_mba`]).
///
/// The two v2 tables differ in exactly two ways — the sample width, and
/// whether the two curve stages carry their own entry counts or are fixed at
/// 256 — so they are read by one function with a flag rather than two that
/// drift apart. And in how each encodes a Lab connection space: an `mft2` in
/// version 2's legacy 16-bit encoding, an `mft1` in its own 8-bit one
/// (6.3.4.2).
fn read_lut(data: &[u8], lab: bool, direction: Direction) -> Result<Lut, IccError> {
    #[cfg(test)]
    tests::TABLE_READS.with(|reads| reads.set(reads.get() + 1));
    let reader = Reader { bytes: data };
    let wide = match (reader.sig(0).ok_or(IccError::MalformedTag)?, direction) {
        (s, _) if &s == b"mft2" => true,
        (s, _) if &s == b"mft1" => false,
        // `mAB ` and `mBA ` are v4's, and a different structure: five
        // optional stages where v2 has three fixed ones, each carrying its
        // own offset, one type a direction. A device link's `AToB0` is an
        // `mAB ` too (8.6), from one device to another.
        (s, Direction::Forward) if &s == b"mAB " => return read_mab(data, lab, None),
        (s, Direction::Link { inputs, outputs }) if &s == b"mAB " => {
            return read_mab(data, false, Some((inputs, outputs)))
        }
        (s, Direction::Output { channels }) if &s == b"mBA " => {
            return read_mba(data, lab, channels)
        }
        // `mBA ` is the *inverse* direction, and an `A2B*` tag never holds one:
        // it would be a profile saying "here is how to get out of the
        // connection space" at the entry that asks how to get in — and an
        // `mAB ` at a `B2A*` tag the same the other way round.
        _ => return Err(IccError::NeedsLut),
    };

    let byte = |at: usize| data.get(at).copied().ok_or(IccError::MalformedTag);
    let inputs = byte(8)? as usize;
    let outputs = byte(9)? as usize;
    let grid = byte(10)? as usize;
    // The input count is a byte, and a grid of two points an axis keeps
    // `grid.pow(inputs)` under `MAX_CLUT_ENTRIES` up to twenty of them, so it
    // is held to `MAX_LUT_INPUTS` as `mAB `'s is.
    if inputs == 0 || outputs == 0 || grid < 2 || inputs > MAX_LUT_INPUTS {
        return Err(IccError::MalformedTag);
    }
    match direction {
        // Three is what the connection space is, and what `to_rgb` reads.
        Direction::Forward if outputs != 3 => return Err(IccError::UnsupportedSpace),
        Direction::Forward => {}
        // The output count is a byte, held to the most channels a data
        // space has before a curve is read for any of them; then to the
        // profile's own data space, which is what the channels are. The
        // input is the connection space's three.
        Direction::Output { .. } if outputs > MAX_DEVICE_CHANNELS => {
            return Err(IccError::TooLarge)
        }
        Direction::Output { channels } if outputs != channels => {
            return Err(IccError::UnsupportedSpace)
        }
        Direction::Output { .. } if inputs != 3 => return Err(IccError::MalformedTag),
        Direction::Output { .. } => {}
        // A link's two sides are two devices: the output count held to the
        // most channels a data space has before a curve is read, then each
        // count to its space's, the data space's and the one the
        // connection-space field names (7.2.7).
        Direction::Link { .. } if outputs > MAX_DEVICE_CHANNELS => return Err(IccError::TooLarge),
        Direction::Link {
            inputs: data,
            outputs: device,
        } if inputs != data || outputs != device => return Err(IccError::UnsupportedSpace),
        Direction::Link { .. } => {}
    }
    // `grid.pow(inputs)` is the branch this format hides in two bytes.
    let points = grid
        .checked_pow(u32::try_from(inputs).map_err(|_| IccError::TooLarge)?)
        .ok_or(IccError::TooLarge)?;
    let entries = points.checked_mul(outputs).ok_or(IccError::TooLarge)?;
    if entries > MAX_CLUT_ENTRIES {
        return Err(IccError::TooLarge);
    }

    // The 3x3 matrix at offset 12 applies only when the input is the connection
    // space in XYZ (10.8, 10.9): at an `A2B*` tag it is not, and the matrix is
    // read past; at a `B2A*` tag over XYZ it is, and a matrix other than the
    // identity is kept. Over Lab the clause has it the identity, and a link's
    // input is a device's, so it is read past there too.
    let input_matrix = match direction {
        Direction::Output { .. } if !lab => {
            let mut e = [0.0f64; 9];
            for (index, slot) in e.iter_mut().enumerate() {
                *slot = reader
                    .s15fixed16(12 + 4 * index)
                    .ok_or(IccError::MalformedTag)?;
            }
            let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
            (e != identity).then_some(e)
        }
        _ => None,
    };
    let mut at = 12 + 9 * 4;
    let (input_entries, output_entries) = if wide {
        let i = reader.u16(at).ok_or(IccError::MalformedTag)? as usize;
        let o = reader.u16(at + 2).ok_or(IccError::MalformedTag)? as usize;
        at += 4;
        if i < 2 || o < 2 {
            return Err(IccError::MalformedTag);
        }
        (i, o)
    } else {
        (256, 256)
    };

    let width = if wide { 2 } else { 1 };
    let mut take = |count: usize| -> Result<Vec<u16>, IccError> {
        let end = at
            .checked_add(count.checked_mul(width).ok_or(IccError::TooLarge)?)
            .ok_or(IccError::TooLarge)?;
        let slice = data.get(at..end).ok_or(IccError::MalformedTag)?;
        at = end;
        Ok(if wide {
            slice
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect()
        } else {
            // An eight-bit table is scaled to the sixteen-bit currency
            // everything downstream speaks, so one evaluator serves both.
            slice.iter().map(|b| u16::from(*b) * 257).collect()
        })
    };

    let mut input_tables = Vec::with_capacity(inputs);
    for _ in 0..inputs {
        input_tables.push(take(input_entries)?);
    }
    let clut = take(entries)?;
    let mut output_tables = Vec::with_capacity(outputs);
    for _ in 0..outputs {
        output_tables.push(take(output_entries)?);
    }

    Ok(Lut {
        inputs,
        outputs,
        // One count for every axis, which is `mft1`/`mft2`'s own restriction.
        grids: vec![grid; inputs],
        input_tables,
        clut,
        output_tables,
        // v2 has neither stage; `mAB ` and `mBA ` are where they come from.
        m_tables: None,
        matrix: None,
        matrix_stage: MatrixStage::AfterGrid,
        input_matrix,
        lab,
        // 6.3.4.2, note 3: only `lut16Type` keeps version 2's legacy
        // encoding; `lut8Type`'s is its own 8-bit one, `L*` 100 at `FFh`.
        lab_scale: if wide {
            LabScale::Legacy
        } else {
            LabScale::Full
        },
        bits: if wide { 16 } else { 8 },
    })
}

/// How many entries a curve sampled out of a v4 tag carries.
///
/// `mft1` and `mft2` hand over tables the file wrote; `mAB `'s stages are
/// `curv` and `para` tags, which are functions rather than tables, so they are
/// sampled to the same currency everything downstream already speaks. 1 024 is
/// four times the eight-bit table `mft1` supplies and is exact for the
/// identity, which is what most of these curves are.
const V4_CURVE_ENTRIES: usize = 1024;

/// One curve tag, and how many bytes it occupied.
///
/// `mAB `'s stages are runs of consecutive curves, so a reader has to know
/// where each one ends: `curv` is twelve bytes plus two per entry and `para`
/// twelve plus four per parameter, each padded to a four-byte boundary.
fn read_curve_sized(data: &[u8], at: usize) -> Result<(Curve, usize), IccError> {
    let tag = data.get(at..).ok_or(IccError::MalformedTag)?;
    let reader = Reader { bytes: tag };
    let signature = reader.sig(0).ok_or(IccError::MalformedTag)?;
    let raw = match &signature {
        b"curv" => 12 + 2 * reader.u32(8).ok_or(IccError::MalformedTag)? as usize,
        b"para" => {
            let function = reader.u16(8).ok_or(IccError::MalformedTag)?;
            let count = match function {
                0 => 1,
                1 => 3,
                2 => 4,
                3 => 5,
                4 => 7,
                _ => return Err(IccError::MalformedTag),
            };
            12 + 4 * count
        }
        _ => return Err(IccError::MalformedTag),
    };
    let padded = raw.checked_next_multiple_of(4).ok_or(IccError::TooLarge)?;
    if at.checked_add(padded).is_none_or(|end| end > data.len()) {
        return Err(IccError::MalformedTag);
    }
    let curve = read_curve(tag.get(..raw).ok_or(IccError::MalformedTag)?)?;
    Ok((curve, padded))
}

/// `count` consecutive curves at `at`, each sampled into a table.
fn read_curve_run(data: &[u8], at: usize, count: usize) -> Result<Vec<Vec<u16>>, IccError> {
    let mut tables = Vec::with_capacity(count);
    let mut cursor = at;
    for _ in 0..count {
        let (curve, size) = read_curve_sized(data, cursor)?;
        tables.push(sample_curve(&curve, V4_CURVE_ENTRIES));
        cursor = cursor.checked_add(size).ok_or(IccError::TooLarge)?;
    }
    Ok(tables)
}

/// A curve as a table of `entries` samples, evenly spaced over `0..=1`.
fn sample_curve(curve: &Curve, entries: usize) -> Vec<u16> {
    (0..entries)
        .map(|i| {
            let x = i as f64 / (entries - 1) as f64;
            let y = curve.eval(x).clamp(0.0, 1.0);
            (y * 65535.0).round() as u16
        })
        .collect()
}

/// **ICC.1's `mAB ` (lutAtoBType): v4's A-to-B transform.**
///
/// Where `mft2` is three fixed stages, this is five optional ones, and the
/// order they run in is not the order the header lists their offsets:
///
/// ```text
///   A curves  ->  CLUT  ->  M curves  ->  matrix  ->  B curves
/// ```
///
/// Any stage whose offset is zero is absent, and a tag may legitimately be B
/// curves alone — a v4 profile writes a plain three-curve transform this way
/// rather than as a matrix/TRC profile. The header's five offsets are, in
/// order, B, matrix, M, CLUT, A: the *reverse* of the pipeline, because the
/// type is named for the direction it converts and laid out for the direction
/// it inverts.
///
/// Two things here that `mft2` does not have:
///
/// - **the grid may differ per axis.** `mft1` and `mft2` write one grid count
///   for every input; `mAB `'s CLUT header writes sixteen bytes, one per
///   channel. [`Lut::grids`] carries that, and the v2 reader fills it with the
///   same number repeated so one evaluator serves both.
/// - **the stages are functions, not tables.** They are `curv` and `para`
///   tags, so they are sampled rather than read.
///
/// Its Lab connection space, like `mBA `'s, is each component's range over
/// the whole currency (10.10.1): `L*` 100 at `FFFFh`, not `mft2`'s `FF00h`.
///
/// At a device link's `A2B0` (8.6), `link` gives the data space's channels
/// and the output device's, which the tag's two counts are held to — the
/// output count to [`MAX_DEVICE_CHANNELS`] first, before any stage is read.
/// The matrix is three by three and the M curves one an output channel
/// (10.10.4, 10.10.5), so a tag carrying either has three outputs.
fn read_mab(data: &[u8], lab: bool, link: Option<(usize, usize)>) -> Result<Lut, IccError> {
    let reader = Reader { bytes: data };
    let byte = |at: usize| data.get(at).copied().ok_or(IccError::MalformedTag);
    let inputs = byte(8)? as usize;
    let outputs = byte(9)? as usize;
    if inputs == 0 || inputs > MAX_LUT_INPUTS {
        return Err(IccError::MalformedTag);
    }
    match link {
        // Three is what the connection space is, and what `to_rgb` reads.
        None if outputs != 3 => return Err(IccError::UnsupportedSpace),
        None => {}
        Some(_) if outputs == 0 => return Err(IccError::MalformedTag),
        Some(_) if outputs > MAX_DEVICE_CHANNELS => return Err(IccError::TooLarge),
        Some((data_space, device)) if inputs != data_space || outputs != device => {
            return Err(IccError::UnsupportedSpace)
        }
        Some(_) => {}
    }

    let [b_at, matrix_at, m_at, clut_at, a_at] = v4_offsets(&reader)?;
    if (matrix_at != 0 || m_at != 0) && outputs != 3 {
        return Err(IccError::MalformedTag);
    }

    // The A curves are per *input* and the M and B curves per output.
    let input_tables = if a_at == 0 {
        (0..inputs)
            .map(|_| sample_curve(&Curve::Identity, V4_CURVE_ENTRIES))
            .collect()
    } else {
        read_curve_run(data, a_at, inputs)?
    };

    let (grids, clut, bits) = if clut_at == 0 {
        // No grid: every input passes straight through to the M stage, which
        // only makes sense when there are as many of one as the other.
        if inputs != outputs {
            return Err(IccError::UnsupportedSpace);
        }
        (vec![2; inputs], Vec::new(), 16)
    } else {
        read_mab_clut(data, clut_at, inputs, outputs)?
    };

    let m_tables = if m_at == 0 {
        None
    } else {
        Some(read_curve_run(data, m_at, outputs)?)
    };
    let matrix = read_v4_matrix(&reader, matrix_at)?;
    let output_tables = if b_at == 0 {
        // B is the one stage ICC.1 makes required; a tag without it describes
        // no transform at all.
        return Err(IccError::MalformedTag);
    } else {
        read_curve_run(data, b_at, outputs)?
    };

    Ok(Lut {
        inputs,
        outputs,
        grids,
        input_tables,
        clut,
        output_tables,
        m_tables,
        matrix,
        matrix_stage: MatrixStage::AfterGrid,
        input_matrix: None,
        lab,
        lab_scale: LabScale::Full,
        bits,
    })
}

/// **ICC.1's `mBA ` (lutBtoAType): v4's B-to-A transform**, out of the
/// connection space (10.11).
///
/// `mAB `'s five optional stages, run the other way round:
///
/// ```text
///   B curves  ->  matrix  ->  M curves  ->  CLUT  ->  A curves
/// ```
///
/// the header listing their offsets in that same order this time. The B
/// curves, the matrix and the M curves are the connection space's three
/// channels', the matrix's results clipped to `0..=1` (10.11.3); the grid
/// takes them to the data space's channels, one A curve each. A stage whose
/// offset is zero is absent and passes its channels through, except the B
/// curves, which ICC.1 makes required; a tag with no grid has as many outputs
/// as inputs. The output count is held as an `mft2`'s at a `B2A*` tag is: to
/// [`MAX_DEVICE_CHANNELS`] before any stage is read, then to the data
/// space's `channels`, the inputs to the connection space's three.
///
/// Its Lab connection space, like `mAB `'s, is each component's range over
/// the whole currency (10.11.1): `L*` 100 at `FFFFh`, `a*` 0 at `8080h`. Its
/// XYZ is `u1Fixed15`, and there is no input matrix of `mft2`'s kind: the
/// matrix is a stage of its own.
fn read_mba(data: &[u8], lab: bool, channels: usize) -> Result<Lut, IccError> {
    let reader = Reader { bytes: data };
    let byte = |at: usize| data.get(at).copied().ok_or(IccError::MalformedTag);
    let inputs = byte(8)? as usize;
    let outputs = byte(9)? as usize;
    if inputs == 0 || outputs == 0 || inputs > MAX_LUT_INPUTS {
        return Err(IccError::MalformedTag);
    }
    if outputs > MAX_DEVICE_CHANNELS {
        return Err(IccError::TooLarge);
    }
    if outputs != channels {
        return Err(IccError::UnsupportedSpace);
    }
    if inputs != 3 {
        return Err(IccError::MalformedTag);
    }

    let [b_at, matrix_at, m_at, clut_at, a_at] = v4_offsets(&reader)?;

    // B and M curves one an input, A curves one an output.
    let input_tables = if b_at == 0 {
        return Err(IccError::MalformedTag);
    } else {
        read_curve_run(data, b_at, inputs)?
    };
    let matrix = read_v4_matrix(&reader, matrix_at)?;
    let m_tables = if m_at == 0 {
        None
    } else {
        Some(read_curve_run(data, m_at, inputs)?)
    };
    let (grids, clut, bits) = if clut_at == 0 {
        // No grid: the M stage's channels are the outputs, which only makes
        // sense when there are as many of one as the other (10.11.5).
        if inputs != outputs {
            return Err(IccError::UnsupportedSpace);
        }
        (vec![2; inputs], Vec::new(), 16)
    } else {
        read_mab_clut(data, clut_at, inputs, outputs)?
    };
    let output_tables = if a_at == 0 {
        (0..outputs)
            .map(|_| sample_curve(&Curve::Identity, V4_CURVE_ENTRIES))
            .collect()
    } else {
        read_curve_run(data, a_at, outputs)?
    };

    Ok(Lut {
        inputs,
        outputs,
        grids,
        input_tables,
        clut,
        output_tables,
        m_tables,
        matrix,
        matrix_stage: MatrixStage::BeforeGrid,
        input_matrix: None,
        lab,
        lab_scale: LabScale::Full,
        bits,
    })
}

/// A v4 table's five offsets, in the order its header lists them: B curves,
/// matrix, M curves, CLUT, A curves, each zero where the stage is absent.
fn v4_offsets(reader: &Reader<'_>) -> Result<[usize; 5], IccError> {
    let mut offsets = [0usize; 5];
    for (index, slot) in offsets.iter_mut().enumerate() {
        *slot = reader.u32(12 + 4 * index).ok_or(IccError::MalformedTag)? as usize;
    }
    Ok(offsets)
}

/// A v4 table's matrix at `at` — nine entries row by row, then three
/// offsets, each an `s15Fixed16Number` (10.10.5, 10.11.3) — or `None` where
/// `at` is zero, the stage absent.
fn read_v4_matrix(reader: &Reader<'_>, at: usize) -> Result<Option<[f64; 12]>, IccError> {
    if at == 0 {
        return Ok(None);
    }
    let mut values = [0.0f64; 12];
    for (index, slot) in values.iter_mut().enumerate() {
        *slot = at
            .checked_add(index * 4)
            .and_then(|place| reader.s15fixed16(place))
            .ok_or(IccError::MalformedTag)?;
    }
    Ok(Some(values))
}

/// A v4 table's CLUT, `mAB `'s and `mBA `'s alike: sixteen grid counts, a
/// precision byte, then the table; with the bits an entry holds, 8 or 16.
fn read_mab_clut(
    data: &[u8],
    at: usize,
    inputs: usize,
    outputs: usize,
) -> Result<(Vec<usize>, Vec<u16>, u32), IccError> {
    let head = data.get(at..).ok_or(IccError::MalformedTag)?;
    let mut grids = Vec::with_capacity(inputs);
    let mut points: usize = 1;
    for channel in 0..inputs {
        let count = head.get(channel).copied().ok_or(IccError::MalformedTag)? as usize;
        if count < 2 {
            return Err(IccError::MalformedTag);
        }
        points = points.checked_mul(count).ok_or(IccError::TooLarge)?;
        if points > MAX_CLUT_ENTRIES {
            return Err(IccError::TooLarge);
        }
        grids.push(count);
    }
    let precision = head.get(16).copied().ok_or(IccError::MalformedTag)?;
    let entries = points.checked_mul(outputs).ok_or(IccError::TooLarge)?;
    if entries > MAX_CLUT_ENTRIES {
        return Err(IccError::TooLarge);
    }
    let body = head.get(20..).ok_or(IccError::MalformedTag)?;
    let clut = match precision {
        1 => body
            .get(..entries)
            .ok_or(IccError::MalformedTag)?
            .iter()
            // Scaled to the sixteen-bit currency, as `mft1`'s tables are.
            .map(|b| u16::from(*b) * 257)
            .collect(),
        2 => body
            .get(..entries * 2)
            .ok_or(IccError::MalformedTag)?
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect(),
        _ => return Err(IccError::MalformedTag),
    };
    Ok((grids, clut, u32::from(precision) * 8))
}

/// An `XYZType` tag: a signature, four reserved bytes, then one `XYZNumber`.
fn read_xyz(data: &[u8]) -> Result<[f64; 3], IccError> {
    let reader = Reader { bytes: data };
    if reader.sig(0) != Some(*b"XYZ ") {
        return Err(IccError::MalformedTag);
    }
    let at = |offset: usize| reader.s15fixed16(offset).ok_or(IccError::MalformedTag);
    Ok([at(8)?, at(12)?, at(16)?])
}

/// A `curv` or `para` tag.
fn read_curve(data: &[u8]) -> Result<Curve, IccError> {
    let reader = Reader { bytes: data };
    match reader.sig(0).ok_or(IccError::MalformedTag)? {
        s if &s == b"curv" => {
            let count = reader.u32(8).ok_or(IccError::MalformedTag)? as usize;
            match count {
                0 => Ok(Curve::Identity),
                // One entry is a gamma in u8Fixed8 rather than a sample.
                1 => Ok(Curve::Gamma(
                    f64::from(reader.u16(12).ok_or(IccError::MalformedTag)?) / 256.0,
                )),
                _ => {
                    let end = 12usize.checked_add(count.checked_mul(2).ok_or(IccError::TooLarge)?);
                    let end = end.ok_or(IccError::TooLarge)?;
                    if end > data.len() {
                        return Err(IccError::MalformedTag);
                    }
                    let mut points = Vec::with_capacity(count);
                    for i in 0..count {
                        points.push(reader.u16(12 + i * 2).ok_or(IccError::MalformedTag)?);
                    }
                    Ok(Curve::Sampled(points))
                }
            }
        }
        s if &s == b"para" => {
            let function = reader.u16(8).ok_or(IccError::MalformedTag)?;
            // ICC.1 gives each form its own parameter count, in this order.
            let needed = match function {
                0 => 1,
                1 => 3,
                2 => 4,
                3 => 5,
                4 => 7,
                _ => return Err(IccError::MalformedTag),
            };
            let mut params = Vec::with_capacity(needed);
            for i in 0..needed {
                params.push(
                    reader
                        .s15fixed16(12 + i * 4)
                        .ok_or(IccError::MalformedTag)?,
                );
            }
            Ok(Curve::Parametric { function, params })
        }
        _ => Err(IccError::MalformedTag),
    }
}

/// A compiled transform: a profile's colours into 8-bit sRGB.
///
/// # Why this is integers, and where the floats went
///
/// Ruling 4 wants a page to render to the same bytes on every target, and the
/// transcendental a tone curve is made of does not: `pow` rounds differently
/// on every platform's libm, which is why `tinker-pdf-math` exists. So the
/// curves are evaluated **once**, at compile time, into a table of
/// [`CURVE_ENTRIES`] fixed-point entries, and per-pixel evaluation is a table
/// lookup and an integer matrix multiply. Nothing on the pixel path can round
/// differently anywhere.
///
/// The matrix is held in s15.16 — ICC.1's own `s15Fixed16Number`, which is
/// what the profile stored it as, so no precision is invented on the way in.
///
/// # A lookup table is read through a grid
///
/// A lookup-table transform over one, three or four inputs — an `A2B*`
/// table, or a `D2B*` tag's elements — converts through a [`Grid`] of its own
/// pipeline, sampled at 256, 33 or 17 nodes an axis and read between them in
/// fixed point (`docs/design/icc.md`, P-22): its pipeline — `Lut::evaluate`
/// or the elements, the connection space into sRGB and the encoding — runs
/// once a node, the first time a conversion reaches it, rather than once a
/// colour. A matrix/TRC or grey transform has no grid: its
/// pipeline is already integer, and exact.
#[derive(Debug)]
pub struct Transform {
    /// Per input channel, the curve sampled into 16-bit linear values.
    curves: Vec<Vec<u16>>,
    /// The profile's columns times the XYZ-to-sRGB matrix, in s15.16, so one
    /// multiply takes linear device values straight to linear sRGB.
    matrix: [[i32; 3]; 3],
    /// How many components go in.
    inputs: usize,
    /// A sampled table, where the profile carries one instead of a matrix:
    /// shared with the profile's [`Tables`] and with every other intent that
    /// converts through it.
    lut: Option<Arc<Lut>>,
    /// A `D2B*` tag's multi-process pipeline, where the transform converts
    /// through one: shared with the profile's [`Tables`] and with every
    /// other intent that converts through it.
    pipeline: Option<Arc<mpe::Pipeline>>,
    /// The `c2sp` the table's or pipeline's connection-space value connects
    /// through before it becomes sRGB, where a version 5 profile's viewing
    /// conditions are not the standard ones (ICC.2 10.2.22): shared with
    /// the profile's [`Tables`].
    then: Option<Arc<mpe::Pipeline>>,
    /// The table's or pipeline's connection-space value scaled channel by
    /// channel in XYZ before it becomes sRGB, where this is ICC-absolute
    /// colorimetry's transform through one (6.2.3): the media white over the
    /// PCS white. A model's scaling is folded into its matrix instead.
    media: Option<[f64; 3]>,
    /// The grid `lut` or `pipeline` is read through, where it takes one,
    /// three or four inputs.
    grid: Option<Grid>,
    /// What a row of 8-bit samples converts through, built at the first row
    /// ([`Transform::convert_row`]).
    row: OnceLock<RowTables>,
}

/// A copy converts as the original does; its grid starts with no node
/// filled and its row tables unbuilt, which no conversion can tell.
impl Clone for Transform {
    fn clone(&self) -> Self {
        Transform {
            curves: self.curves.clone(),
            matrix: self.matrix,
            inputs: self.inputs,
            lut: self.lut.clone(),
            pipeline: self.pipeline.clone(),
            then: self.then.clone(),
            media: self.media,
            grid: self.grid.clone(),
            row: OnceLock::new(),
        }
    }
}

/// The most inputs [`Transform::convert_row`] reads a pixel through a buffer
/// on the stack for: ICC.1's colour spaces have at most fifteen channels.
const MAX_ROW_INPUTS: usize = 16;

/// What [`Transform::convert_row`] reads a byte through, one set a
/// transform (`docs/design/icc.md`, P-22, "The row path").
enum RowTables {
    /// A grid over three or four inputs: each byte's cell and 16-bit
    /// fraction on an axis, every axis having the grid's points. The same
    /// function [`Grid::read`] locates a component with, at `byte / 255`.
    Grid(Box<[(usize, i64)]>),
    /// A matrix/TRC transform: for each channel, each byte's curve entry in
    /// s15.16, as [`Transform::apply`] reads it.
    Matrix(Box<[[i64; 256]; 3]>),
    /// A transform over one input: the colour each byte converts to.
    Colours(Box<[[u8; 3]]>),
    /// Any other input count: a pixel at a time through `apply`.
    Pixels,
}

/// Which table a row reads, and not its 768 entries.
impl std::fmt::Debug for RowTables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RowTables::Grid(_) => "RowTables::Grid",
            RowTables::Matrix(_) => "RowTables::Matrix",
            RowTables::Colours(_) => "RowTables::Colours",
            RowTables::Pixels => "RowTables::Pixels",
        })
    }
}

/// Two transforms are one where their pipelines are: a grid is the
/// pipeline's, and which of its nodes are filled is not a property of the
/// transform.
impl PartialEq for Transform {
    fn eq(&self, other: &Self) -> bool {
        self.curves == other.curves
            && self.matrix == other.matrix
            && self.inputs == other.inputs
            && self.lut == other.lut
            && self.pipeline == other.pipeline
            && self.then == other.then
            && self.media == other.media
    }
}

/// Entries in a compiled tone curve.
///
/// Enough that the step between neighbours is below what an eight-bit output
/// can show — a 4 096-entry table over a gamma of 2.4 moves less than a
/// thousandth of full scale per step, so the interpolation the lookup skips
/// could not change a byte.
pub const CURVE_ENTRIES: usize = 4096;

/// One in s15.16.
const ONE: i64 = 1 << 16;

impl Transform {
    /// Compiles a profile into its transform for `intent`, or `None` where
    /// the profile has nothing that intent converts through.
    ///
    /// The transform comes from the first source [`Intent::order`] names that
    /// the profile carries and that reads: a table, or after the tables
    /// ICC.1 8.10.2 names for the intent, the matrix/TRC or grey model, one
    /// transform, which ICC.1's Table 25 calls colorimetric, compiled alike
    /// for every intent that reaches it.
    ///
    /// Absolute colorimetric's is relative colorimetric's source with the
    /// connection-space value scaled by [`Profile::media_scale`] (6.2.3):
    /// folded into the s15.16 matrix for a model, applied to a table's value
    /// in XYZ. Where the scale is one in every channel — a `wtpt` of D50, or
    /// none — it is relative colorimetric's transform bit for bit.
    #[must_use]
    pub fn compile(profile: &Profile, intent: Intent) -> Option<Transform> {
        let source = profile.source(intent)?;
        let media = profile.scale_for(intent, source.absolute());
        let then = profile.tables.to_standard().ok().flatten();
        Some(Transform::of_source(source, media, then))
    }

    /// The transform `source` converts through, its connection-space value
    /// connected through `then` — a `c2sp` — where that is given and scaled
    /// by `media` where that is.
    fn of_source(
        source: Source<'_>,
        media: Option<[f64; 3]>,
        then: Option<Arc<mpe::Pipeline>>,
    ) -> Transform {
        match source {
            // A pipeline was compiled when it was read: its stages are the
            // transform, ending in the connection space, and like a table it
            // is read through a grid of its own light. A spectral one ends in
            // its reduction to XYZ.
            Source::Pipeline { pipeline, .. } | Source::Spectral { pipeline, .. } => Transform {
                curves: Vec::new(),
                matrix: [[0; 3]; 3],
                inputs: pipeline.inputs(),
                grid: Grid::over(Light::Elements(&pipeline, then.as_deref()), media),
                lut: None,
                pipeline: Some(pipeline),
                then,
                media,
                row: OnceLock::new(),
            },
            // A table is already the transform: its curves arrived sampled and
            // its grid is the matrix's replacement rather than a factor of it.
            // What compiles is the grid it is read through, whose nodes are
            // evaluated as conversions reach them.
            Source::Table { lut, .. } => Transform {
                curves: Vec::new(),
                matrix: [[0; 3]; 3],
                inputs: lut.inputs,
                grid: Grid::over(Light::Table(&lut, then.as_deref()), media),
                lut: Some(lut),
                pipeline: None,
                then,
                row: OnceLock::new(),
                media,
            },
            Source::Model(model) => Transform::of_model(model, media),
        }
    }

    /// A matrix/TRC or grey model's transform, its connection-space value
    /// scaled by `media` where that is given.
    fn of_model(model: &Model, media: Option<[f64; 3]>) -> Transform {
        match model {
            Model::MatrixTrc { columns, curves } => {
                // The profile's columns are the device primaries in XYZ, so
                // the product with XYZ-to-sRGB is device-linear to sRGB-linear
                // — one matrix rather than two, computed once here. The
                // absolute intent's scaling sits between the two, on the XYZ
                // the columns give: `XYZ_D50_TO_SRGB x diag(s) x columns`.
                let mut matrix = [[0i32; 3]; 3];
                for (row, out) in matrix.iter_mut().enumerate() {
                    for (column, slot) in out.iter_mut().enumerate() {
                        let mut sum = 0.0;
                        for k in 0..3 {
                            sum += match media {
                                None => XYZ_D50_TO_SRGB[row][k] * columns[column][k],
                                Some(scale) => {
                                    XYZ_D50_TO_SRGB[row][k] * scale[k] * columns[column][k]
                                }
                            };
                        }
                        *slot = fixed(sum);
                    }
                }
                Transform {
                    curves: curves.iter().map(compile_curve).collect(),
                    matrix,
                    inputs: 3,
                    lut: None,
                    pipeline: None,
                    then: None,
                    grid: None,
                    row: OnceLock::new(),
                    media: None,
                }
            }
            Model::Grey { lab, curve } => {
                // A grey profile's single curve makes a luminance, and the
                // three sRGB channels are that luminance: the white point does
                // not enter a relative transform, because the answer is
                // achromatic by construction.
                let mut table = compile_curve(curve);
                if *lab {
                    // Under a Lab connection space the curve gives `L*` rather
                    // than `Y`, and the two differ by 8.6.5.4's cube root — a
                    // mid grey is 0.18 of the light and 0.50 of the lightness.
                    // Baked into the table rather than applied per pixel, so
                    // the pixel path stays a lookup and a matrix multiply.
                    for slot in &mut table {
                        let l = f64::from(*slot) / 65535.0 * 100.0;
                        let f = (l + 16.0) / 116.0;
                        const DELTA: f64 = 6.0 / 29.0;
                        let y = if f > DELTA {
                            f * f * f
                        } else {
                            3.0 * DELTA * DELTA * (f - 4.0 / 29.0)
                        };
                        *slot = (y.clamp(0.0, 1.0) * 65535.0).round() as u16;
                    }
                }
                let matrix = match media {
                    None => [[fixed(1.0), 0, 0], [0, fixed(1.0), 0], [0, 0, fixed(1.0)]],
                    // ICC-absolute: F.2's connection-space value, the curve's
                    // `Y` on the PCS white, scaled channel by channel, is `Y`
                    // on the media white, and that XYZ goes to sRGB through
                    // `XYZ_D50_TO_SRGB` — a grey in the medium's tint. The
                    // three channels all read the one curve, so the first
                    // column carries it.
                    Some(scale) => {
                        let mut matrix = [[0i32; 3]; 3];
                        for (row, out) in matrix.iter_mut().enumerate() {
                            let mut sum = 0.0;
                            for k in 0..3 {
                                sum += XYZ_D50_TO_SRGB[row][k] * PCS_WHITE[k] * scale[k];
                            }
                            out[0] = fixed(sum);
                        }
                        matrix
                    }
                };
                Transform {
                    curves: vec![table],
                    matrix,
                    inputs: 1,
                    lut: None,
                    pipeline: None,
                    then: None,
                    grid: None,
                    row: OnceLock::new(),
                    media: None,
                }
            }
        }
    }

    /// How many components this transform takes.
    #[must_use]
    pub const fn inputs(&self) -> usize {
        self.inputs
    }

    /// What this transform's grid samples: its table, or its `D2B*` tag's
    /// elements; `None` for a model, which has no grid.
    fn light_source(&self) -> Option<Light<'_>> {
        match (&self.lut, &self.pipeline) {
            (Some(lut), _) => Some(Light::Table(lut, self.then.as_deref())),
            (None, Some(pipeline)) => Some(Light::Elements(pipeline, self.then.as_deref())),
            (None, None) => None,
        }
    }

    /// Converts every whole pixel of `samples` — [`Transform::inputs`] bytes
    /// a pixel, each component its byte over 255 — onto the end of `rgb`,
    /// three bytes a pixel, each the colour [`Transform::apply`] gives the
    /// same components. Nothing is allocated but the room `rgb` makes for
    /// the row, so an image converted a strip at a time allocates no more
    /// for its strips than for one.
    ///
    /// The 8-bit row path (`docs/design/icc.md`, P-22, "The row path"): a
    /// transform read through a grid over three or four inputs locates each
    /// byte through one 256-entry table of cell and fraction and reads the
    /// grid as `apply` does, the nodes and the encoding table fetched once a
    /// row; a matrix/TRC transform indexes each channel's curve through a
    /// 256-entry table of the entry `apply` reads, then multiplies and
    /// encodes as `apply` does, term for term; a transform over one input
    /// reads a 256-entry table of the colour each byte converts to. The
    /// tables are built at the first row, once a transform, and a row
    /// computes in integers with no float and no call of `apply`. Any other
    /// input count converts a pixel at a time through `apply`. Bytes after
    /// the last whole pixel are not a pixel and are passed over.
    pub fn convert_row(&self, samples: &[u8], rgb: &mut Vec<u8>) {
        let inputs = self.inputs.max(1);
        rgb.reserve((samples.len() / inputs).saturating_mul(3));
        match self.row_tables() {
            RowTables::Grid(axis) => {
                let (Some(source), Some(grid)) = (self.light_source(), &self.grid) else {
                    return;
                };
                let nodes = grid.nodes();
                let table = grid.encoding.table();
                let at = |byte: u8| axis.get(usize::from(byte)).copied().unwrap_or((0, 0));
                if grid.inputs == 3 {
                    for pixel in samples.chunks_exact(3) {
                        let [(c0, f0), (c1, f1), (c2, f2)] =
                            [at(pixel[0]), at(pixel[1]), at(pixel[2])];
                        let (r, g, b) = grid.read_with(
                            source,
                            nodes,
                            table,
                            &[c0, c1, c2, 0],
                            &[f0, f1, f2, 0],
                        );
                        rgb.extend_from_slice(&[r, g, b]);
                    }
                } else {
                    for pixel in samples.chunks_exact(4) {
                        let [(c0, f0), (c1, f1), (c2, f2), (c3, f3)] =
                            [at(pixel[0]), at(pixel[1]), at(pixel[2]), at(pixel[3])];
                        let (r, g, b) = grid.read_with(
                            source,
                            nodes,
                            table,
                            &[c0, c1, c2, c3],
                            &[f0, f1, f2, f3],
                        );
                        rgb.extend_from_slice(&[r, g, b]);
                    }
                }
            }
            RowTables::Matrix(channels) => {
                for pixel in samples.chunks_exact(3) {
                    let linear = [
                        channels[0][usize::from(pixel[0])],
                        channels[1][usize::from(pixel[1])],
                        channels[2][usize::from(pixel[2])],
                    ];
                    let mut out = [0u8; 3];
                    for (row, slot) in out.iter_mut().enumerate() {
                        let mut sum = 0i64;
                        for (column, value) in linear.iter().enumerate() {
                            sum += i64::from(self.matrix[row][column]) * value / ONE;
                        }
                        *slot = encode_srgb(sum);
                    }
                    rgb.extend_from_slice(&out);
                }
            }
            RowTables::Colours(colours) => {
                for &byte in samples {
                    let colour = colours.get(usize::from(byte)).copied().unwrap_or([0; 3]);
                    rgb.extend_from_slice(&colour);
                }
            }
            RowTables::Pixels => {
                // ICC.1's colour spaces have at most fifteen channels, so a
                // pixel's components fit a buffer on the stack, and a row
                // allocates nothing however often it is converted; a
                // transform over more inputs than that keeps one of its own.
                let mut held = [0.0f64; MAX_ROW_INPUTS];
                let mut wide = Vec::new();
                let components: &mut [f64] = match held.get_mut(..inputs) {
                    Some(components) => components,
                    None => {
                        wide.resize(inputs, 0.0);
                        &mut wide
                    }
                };
                for pixel in samples.chunks_exact(inputs) {
                    for (slot, &byte) in components.iter_mut().zip(pixel) {
                        *slot = f64::from(byte) / 255.0;
                    }
                    let (r, g, b) = self.apply(components);
                    rgb.extend_from_slice(&[r, g, b]);
                }
            }
        }
    }

    /// The tables [`Transform::convert_row`] reads, built now if no row has
    /// been converted yet.
    fn row_tables(&self) -> &RowTables {
        self.row.get_or_init(|| {
            #[cfg(test)]
            tests::ROW_TABLE_BUILDS.with(|count| count.set(count.get() + 1));
            let component = |byte: u8| f64::from(byte) / 255.0;
            match (self.light_source(), &self.grid) {
                (Some(source), Some(grid)) if grid.inputs == 1 => RowTables::Colours(
                    (0..=u8::MAX)
                        .map(|byte| {
                            let (cell, fraction) = locate(component(byte), grid.points);
                            let (r, g, b) =
                                grid.read_at(source, &[cell, 0, 0, 0], &[fraction, 0, 0, 0]);
                            [r, g, b]
                        })
                        .collect(),
                ),
                (Some(_), Some(grid)) => RowTables::Grid(
                    (0..=u8::MAX)
                        .map(|byte| locate(component(byte), grid.points))
                        .collect(),
                ),
                (None, _) if self.inputs == 3 => {
                    let mut channels = Box::new([[0i64; 256]; 3]);
                    for (channel, table) in channels.iter_mut().enumerate() {
                        let curve = self
                            .curves
                            .get(channel.min(self.curves.len().saturating_sub(1)));
                        for (byte, slot) in (0..=u8::MAX).zip(table.iter_mut()) {
                            // `apply`'s own reading of a component: its
                            // curve's entry at the nearest of 4 096, in s15.16.
                            let entry =
                                (component(byte) * (CURVE_ENTRIES - 1) as f64).round() as usize;
                            let sampled = curve
                                .and_then(|c| c.get(entry.min(CURVE_ENTRIES - 1)))
                                .copied()
                                .unwrap_or(0);
                            *slot = i64::from(sampled) * ONE / 65535;
                        }
                    }
                    RowTables::Matrix(channels)
                }
                (None, _) if self.inputs == 1 => RowTables::Colours(
                    (0..=u8::MAX)
                        .map(|byte| {
                            let (r, g, b) = self.evaluate(&[component(byte)]);
                            [r, g, b]
                        })
                        .collect(),
                ),
                _ => RowTables::Pixels,
            }
        })
    }

    /// Converts one colour to 8-bit sRGB.
    ///
    /// Integers throughout: a table lookup per channel, an s15.16 matrix
    /// multiply in `i64`, then the sRGB transfer function from a second
    /// compiled table. No float, no `pow`, nothing that rounds differently
    /// between one target and the next. A lookup-table transform with a grid
    /// reads its grid: each component located in 16-bit fixed point, the
    /// nodes around it interpolated in `i64` and the light encoded through a
    /// table, every node it reads evaluated once and kept.
    #[must_use]
    pub fn apply(&self, components: &[f64]) -> (u8, u8, u8) {
        #[cfg(test)]
        tests::APPLY_CALLS.with(|count| count.set(count.get() + 1));
        if let (Some(source), Some(grid)) = (self.light_source(), &self.grid) {
            return grid.read(source, components);
        }
        self.evaluate(components)
    }

    /// [`Transform::apply`] at sixteen bits (`docs/design/sixteen-bit.md`):
    /// the same evaluation — the grid's interpolated light, the table's, the
    /// pipeline's or the matrix's — encoded by IEC 61966-2-1's transfer and
    /// held to the byte [`Transform::apply`] gives each channel, so its top
    /// bytes are that colour's bytes and a second transform never runs.
    #[must_use]
    pub fn apply16(&self, components: &[f64]) -> [u16; 3] {
        if let (Some(source), Some(grid)) = (self.light_source(), &self.grid) {
            return grid.read16(source, components);
        }
        self.evaluate16(components)
    }

    /// [`Transform::evaluate`] at sixteen bits, as [`Transform::apply16`]
    /// quantises it.
    pub(crate) fn evaluate16(&self, components: &[f64]) -> [u16; 3] {
        let then = self.then.as_deref();
        if let Some(lut) = &self.lut {
            if then.is_none() {
                return lut.to_rgb16(components, self.media);
            }
            let (pcs, lab) = standardised(lut.connection(components), lut.lab, then);
            return pcs_to_rgb16(pcs, lab, self.media);
        }
        if let Some(pipeline) = &self.pipeline {
            let (pcs, lab) = standardised(pipeline.evaluate(components), pipeline.lab(), then);
            return pcs_to_rgb16(pcs, lab, self.media);
        }
        self.matrix_light(components).map(|sum| {
            let linear = sum.clamp(0, ONE) as f64 / ONE as f64;
            crate::held16(crate::srgb_transfer(linear), encode_srgb(sum))
        })
    }

    /// The transform's whole pipeline for one colour, with no grid: what a
    /// grid's node is evaluated as, and what the tests hold a grid to.
    pub(crate) fn evaluate(&self, components: &[f64]) -> (u8, u8, u8) {
        let then = self.then.as_deref();
        if let Some(lut) = &self.lut {
            if then.is_none() {
                return lut.to_rgb(components, self.media);
            }
            let (pcs, lab) = standardised(lut.connection(components), lut.lab, then);
            return pcs_to_rgb(pcs, lab, self.media);
        }
        if let Some(pipeline) = &self.pipeline {
            #[cfg(test)]
            tests::PIPELINE_EVALUATIONS.with(|count| count.set(count.get() + 1));
            let (pcs, lab) = standardised(pipeline.evaluate(components), pipeline.lab(), then);
            return pcs_to_rgb(pcs, lab, self.media);
        }
        let [r, g, b] = self.matrix_light(components).map(encode_srgb);
        (r, g, b)
    }

    /// A matrix/TRC or grey transform's linear light for one colour, in
    /// s15.16, before it is encoded: each component through its curve, then
    /// the matrix.
    fn matrix_light(&self, components: &[f64]) -> [i64; 3] {
        let mut linear = [0i64; 3];
        for (channel, slot) in linear.iter_mut().enumerate() {
            let index = if self.inputs == 1 { 0 } else { channel };
            let value = components
                .get(index)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let entry = (value * (CURVE_ENTRIES - 1) as f64).round() as usize;
            let curve = self.curves.get(index.min(self.curves.len() - 1));
            let sampled = curve
                .and_then(|c| c.get(entry.min(CURVE_ENTRIES - 1)))
                .copied()
                .unwrap_or(0);
            // 16-bit linear, promoted to s15.16 for the multiply below.
            *slot = i64::from(sampled) * ONE / 65535;
        }

        let mut out = [0i64; 3];
        for (row, slot) in out.iter_mut().enumerate() {
            let mut sum = 0i64;
            for (column, value) in linear.iter().enumerate() {
                sum += i64::from(self.matrix[row][column]) * value / ONE;
            }
            *slot = sum;
        }
        out
    }
}

/// A profile's transforms, one for each intent, each compiled the first time
/// a conversion asks for it (`docs/design/icc.md`, "What compiling per intent
/// costs").
///
/// Relative colorimetric's is compiled with the set: it is 8.6.5.8's initial
/// intent, the one every conversion that names none takes, so a profile used
/// in no other intent compiles one transform. The others compile one
/// transform a distinct source and scaling: an intent shares the transform of
/// one that converts through the same table or the same model unscaled, so
/// absolute colorimetric compiles one of its own only where its media white
/// is not the PCS white.
#[derive(Debug)]
pub struct Transforms {
    profile: Profile,
    /// The bytes the profile was read from, where the transforms were
    /// compiled from them ([`Transforms::compile_shared`]): what a command
    /// stream writes an ICC space by, so its reader compiles the same
    /// transforms. Shared with whoever handed them over, and counted there.
    source: Option<Arc<Vec<u8>>>,
    relative: Arc<Transform>,
    /// Which source relative colorimetric's transform came from.
    relative_source: SourceId,
    /// Perceptual, saturation and absolute colorimetric's, in that order.
    others: [OnceLock<Arc<Transform>>; 3],
}

impl Transforms {
    /// The profile's transforms, or `None` where it has no relative
    /// colorimetric one, which leaves it with none at all.
    #[must_use]
    pub fn compile(profile: Profile) -> Option<Transforms> {
        let source = profile.source(Intent::RelativeColorimetric)?;
        let relative_source = source.id();
        let media = profile.scale_for(Intent::RelativeColorimetric, source.absolute());
        let then = profile.tables.to_standard().ok().flatten();
        let relative = Arc::new(Transform::of_source(source, media, then));
        Some(Transforms {
            profile,
            source: None,
            relative,
            relative_source,
            others: [OnceLock::new(), OnceLock::new(), OnceLock::new()],
        })
    }

    /// The transforms of the profile `bytes` hold, keeping the bytes
    /// ([`Transforms::source`]) without a copy; `None` where they are no
    /// profile [`Profile::parse`] reads, or one with no relative
    /// colorimetric transform.
    #[must_use]
    pub fn compile_shared(bytes: Arc<Vec<u8>>) -> Option<Transforms> {
        let profile = Profile::parse(&bytes).ok()?;
        let mut transforms = Transforms::compile(profile)?;
        transforms.source = Some(bytes);
        Some(transforms)
    }

    /// [`Transforms::compile_shared`] of a copy of `bytes`.
    #[must_use]
    pub fn compile_bytes(bytes: &[u8]) -> Option<Transforms> {
        Transforms::compile_shared(Arc::new(bytes.to_vec()))
    }

    /// The bytes the profile was read from, where the transforms were
    /// compiled from them ([`Transforms::compile_shared`]); `None` for
    /// transforms compiled from a profile handed over parsed.
    #[must_use]
    pub fn source(&self) -> Option<&[u8]> {
        self.source.as_deref().map(Vec::as_slice)
    }

    /// The transform `intent` converts through, compiled now if no colour has
    /// asked for it yet.
    #[must_use]
    pub fn get(&self, intent: Intent) -> &Transform {
        let slot = match intent {
            Intent::RelativeColorimetric => return &self.relative,
            Intent::Perceptual => 0,
            Intent::Saturation => 1,
            Intent::AbsoluteColorimetric => 2,
        };
        self.others[slot].get_or_init(|| {
            // Every intent's order holds every table the profile carries and
            // its model, so relative colorimetric's source is in it and this
            // finds one; were it not to, relative colorimetric's transform is
            // the answer 8.6.5.8 gives.
            let Some(source) = self.profile.source(intent) else {
                return Arc::clone(&self.relative);
            };
            let media = self.profile.scale_for(intent, source.absolute());
            if media.is_none() {
                // An unscaled transform is shared only with another
                // unscaled one through the same source: relative
                // colorimetric's through a version 5 profile's `A2B3` is
                // scaled to the media-relative value.
                let id = source.id();
                if id == self.relative_source && self.relative.media.is_none() {
                    return Arc::clone(&self.relative);
                }
                for (other, at) in [(Intent::Perceptual, 0), (Intent::Saturation, 1)] {
                    if other == intent {
                        continue;
                    }
                    let shared = self.others[at].get().filter(|transform| {
                        transform.media.is_none()
                            && self.profile.source(other).map(|source| source.id()) == Some(id)
                    });
                    if let Some(transform) = shared {
                        return Arc::clone(transform);
                    }
                }
            }
            let then = self.profile.tables.to_standard().ok().flatten();
            Arc::new(Transform::of_source(source, media, then))
        })
    }

    /// How many components the transforms take.
    #[must_use]
    pub fn inputs(&self) -> usize {
        self.relative.inputs()
    }

    /// The profile the transforms were compiled from.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The most bytes these transforms can come to hold, whichever intents
    /// conversions ask for later and whatever they convert: the profile's
    /// model and its named colours, and its tables and pipelines as read —
    /// one not read yet at the
    /// most reading it can allocate ([`lut_bound`], [`mpe::bound`]) — and
    /// every transform an intent compiles of its own at the most it holds
    /// once its grid's every node is filled and its row tables built
    /// ([`transform_bound`]). That is relative colorimetric's, and one more
    /// for each of the other three intents, each at the most any of the
    /// profile's sources compiles — a model's curves, or the largest grid a
    /// table or a pipeline is read through ([`Tables::grid_bound`]) — since
    /// each may convert through any of them, absolute colorimetric's scaled
    /// over the same source as another's.
    ///
    /// What a cache that keeps a set charges for it, so that compiling an
    /// intent after it is kept never takes it past what it was charged. It
    /// only falls as tables and pipelines are read, each to what it holds.
    #[must_use]
    pub fn bytes_bound(&self) -> usize {
        let model = self.profile.model.as_ref().map_or(0, Model::held_bytes);
        let named = self
            .profile
            .named
            .as_ref()
            .map_or(0, NamedColours::held_bytes);
        let compiled = self
            .profile
            .model
            .as_ref()
            .map_or(0, Model::compiled_bytes)
            .max(self.profile.tables.grid_bound());
        size_of::<Transforms>()
            .saturating_add(model)
            .saturating_add(named)
            .saturating_add(self.profile.tables.held_bound())
            .saturating_add(self.relative.bytes_bound())
            .saturating_add(transform_bound(0, compiled).saturating_mul(3))
    }

    /// The bytes these transforms hold now: [`Transforms::bytes_bound`]'s
    /// parts as they stand, every table and pipeline read counted as what it
    /// holds and every transform compiled so far once, however many intents
    /// share it.
    #[cfg(test)]
    fn held_bytes(&self) -> usize {
        let model = self.profile.model.as_ref().map_or(0, Model::held_bytes)
            + self
                .profile
                .named
                .as_ref()
                .map_or(0, NamedColours::held_bytes);
        let mut counted = vec![Arc::as_ptr(&self.relative)];
        let mut others = 0;
        for other in self.others.iter().filter_map(OnceLock::get) {
            if !counted.contains(&Arc::as_ptr(other)) {
                counted.push(Arc::as_ptr(other));
                others += other.held_bytes();
            }
        }
        size_of::<Transforms>()
            + model
            + self.profile.tables.held()
            + self.relative.held_bytes()
            + others
    }
}

impl Transform {
    /// The most bytes this transform holds ([`transform_bound`]): its
    /// compiled curves and its grid, whose input count is its table's or its
    /// pipeline's. The table or pipeline is the profile's, and is counted
    /// there.
    fn bytes_bound(&self) -> usize {
        let grid = self.grid.as_ref().map_or(0, |grid| grid_bytes(grid.inputs));
        transform_bound(curve_bytes(&self.curves), grid)
    }

    /// The bytes this transform holds now: [`Transform::bytes_bound`]'s parts
    /// as they stand, the grid's nodes where a read has allocated them and the
    /// row tables where a row has built them.
    #[cfg(test)]
    fn held_bytes(&self) -> usize {
        let grid = self
            .grid
            .as_ref()
            .and_then(|grid| grid.nodes.get())
            .map_or(0, |nodes| nodes.len() * size_of::<AtomicU64>());
        let rows = match self.row.get() {
            Some(RowTables::Grid(cells)) => cells.len() * size_of::<(usize, i64)>(),
            Some(RowTables::Matrix(_)) => size_of::<[[i64; 256]; 3]>(),
            Some(RowTables::Colours(colours)) => colours.len() * size_of::<[u8; 3]>(),
            Some(RowTables::Pixels) | None => 0,
        };
        size_of::<Transform>() + curve_bytes(&self.curves) + grid + rows
    }
}

/// The most bytes a transform holds: its own size, which takes in its grid's
/// and its row tables' own fields, its compiled curves of `curves` bytes, a
/// grid of `grid` bytes once every node is filled ([`grid_bytes`]), and the
/// largest row tables [`Transform::convert_row`] builds — a cell and a
/// fraction for each of 256 bytes, each channel's 256 curve entries, or the
/// colour of each of 256 bytes. A transform has curves or a grid, never
/// both, so either may be the larger of the two it could have.
fn transform_bound(curves: usize, grid: usize) -> usize {
    let rows = (256 * size_of::<(usize, i64)>())
        .max(size_of::<[[i64; 256]; 3]>())
        .max(256 * size_of::<[u8; 3]>());
    size_of::<Transform>()
        .saturating_add(curves)
        .saturating_add(grid)
        .saturating_add(rows)
}

/// The bytes the nodes of a grid over `inputs` take once a read has
/// allocated them, or none for an input count no grid is built over.
fn grid_bytes(inputs: usize) -> usize {
    // At most 17^4 nodes, so the product cannot overflow.
    grid_points(inputs).map_or(0, |points| {
        points.pow(inputs as u32) * size_of::<AtomicU64>()
    })
}

/// The bytes a run of sampled curves holds.
fn curve_bytes(curves: &[Vec<u16>]) -> usize {
    curves
        .iter()
        .map(|curve| curve.len().saturating_mul(size_of::<u16>()))
        .fold(0, usize::saturating_add)
}

/// The most bytes reading a lookup table of `raw` bytes can allocate: every
/// value of its grid and of `mft1`'s and `mft2`'s curves at sixteen bits,
/// which at most doubles what an eight-bit table spells, and a v4 table's
/// curves, each sampled to [`V4_CURVE_ENTRIES`] whatever it spells — an
/// `mAB `'s at most [`MAX_LUT_INPUTS`] A curves and three each of M and B,
/// an `mBA `'s three each of B and M and at most [`MAX_DEVICE_CHANNELS`] A
/// curves, fewer — beside the table's own fields and its grid counts.
fn lut_bound(raw: usize) -> usize {
    let curves = (MAX_LUT_INPUTS + 6) * V4_CURVE_ENTRIES * size_of::<u16>();
    raw.saturating_mul(2)
        .saturating_add(curves)
        .saturating_add(size_of::<Lut>())
        .saturating_add(MAX_LUT_INPUTS * size_of::<usize>())
}

impl Curve {
    /// The bytes a curve holds beside its own size: a sampled curve's
    /// entries, and a parametric one's parameters.
    fn held_bytes(&self) -> usize {
        match self {
            Curve::Sampled(entries) => entries.len().saturating_mul(size_of::<u16>()),
            Curve::Parametric { params, .. } => params.len().saturating_mul(size_of::<f64>()),
            Curve::Identity | Curve::Gamma(_) => 0,
        }
    }
}

impl Model {
    /// The bytes the model's curves hold.
    fn held_bytes(&self) -> usize {
        match self {
            Model::MatrixTrc { curves, .. } => curves
                .iter()
                .map(Curve::held_bytes)
                .fold(0, usize::saturating_add),
            Model::Grey { curve, .. } => curve.held_bytes(),
        }
    }

    /// The bytes a transform of the model compiles: its curves, each sampled
    /// to [`CURVE_ENTRIES`] ([`compile_curve`]).
    fn compiled_bytes(&self) -> usize {
        let curves = match self {
            Model::MatrixTrc { .. } => 3,
            Model::Grey { .. } => 1,
        };
        curves * CURVE_ENTRIES * size_of::<u16>()
    }
}

impl Lut {
    /// The bytes a table holds: its own fields, its grid counts, its curves
    /// and its grid.
    fn held_bytes(&self) -> usize {
        size_of::<Lut>()
            .saturating_add(self.grids.len().saturating_mul(size_of::<usize>()))
            .saturating_add(curve_bytes(&self.input_tables))
            .saturating_add(self.clut.len().saturating_mul(size_of::<u16>()))
            .saturating_add(curve_bytes(&self.output_tables))
            .saturating_add(self.m_tables.as_deref().map_or(0, curve_bytes))
    }
}

impl<T> Element<T> {
    /// The most bytes the element can come to hold, by `held` and `bound`:
    /// its bytes, kept until the parser lets them go, and what reading them
    /// gave or, not read yet, the most reading them can give. A refusal holds
    /// nothing beside the element's bytes.
    fn held_bound(&self, held: impl Fn(&T) -> usize, bound: impl Fn(&[u8]) -> usize) -> usize {
        let raw = self.bytes.as_deref().unwrap_or_default();
        let read = match self.read.get() {
            Some(Ok(value)) => held(value),
            Some(Err(_)) => 0,
            None => bound(raw),
        };
        raw.len().saturating_add(read)
    }

    /// The bytes the element holds now: its bytes, and what reading them
    /// gave, where it has been read.
    #[cfg(test)]
    fn held(&self, held: impl Fn(&T) -> usize) -> usize {
        let read = match self.read.get() {
            Some(Ok(value)) => held(value),
            _ => 0,
        };
        self.bytes.as_ref().map_or(0, Vec::len) + read
    }

    /// The inputs a transform over the element takes, by `inputs` once it is
    /// read and otherwise as its bytes spell them, by `spelt`; none for one
    /// that will not read, which no transform is built over.
    fn inputs(&self, inputs: impl Fn(&T) -> usize, spelt: impl Fn(&[u8]) -> usize) -> usize {
        match self.read.get() {
            Some(Ok(value)) => inputs(value),
            Some(Err(_)) => 0,
            None => self.bytes.as_deref().map_or(0, spelt),
        }
    }
}

impl Tables {
    /// The most bytes the tables and pipelines can come to hold: their
    /// lists, and each element's bytes kept until it is read and what reading
    /// it gave or, for one not read yet, the most reading it can give — the
    /// `B2A*` tables among them, which a transform out of the connection
    /// space reads ([`Output::compile`]).
    fn held_bound(&self) -> usize {
        let lists = (self.tables.capacity() + self.outputs.capacity()) * size_of::<Element<Lut>>()
            + (self.pipelines.capacity()
                + self.output_pipelines.capacity()
                + self.spectral.capacity())
                * size_of::<Element<mpe::Pipeline>>()
            + self.weights.capacity() * size_of::<f64>();
        let tables = self
            .tables
            .iter()
            .chain(&self.outputs)
            .chain(&self.gamut)
            .map(|table| table.held_bound(Lut::held_bytes, |raw| lut_bound(raw.len())));
        let pipelines = self
            .pipelines
            .iter()
            .chain(&self.output_pipelines)
            .chain(self.custom_elements())
            .map(|pipeline| pipeline.held_bound(mpe::Pipeline::held_bytes, mpe::bound));
        let channels = self.weights.len() / 3;
        let spectral = self.spectral.iter().map(|pipeline| {
            pipeline.held_bound(mpe::Pipeline::held_bytes, |raw| {
                mpe::spectral_bound(raw, channels)
            })
        });
        let link = self.link.as_ref().map_or(0, LinkTags::held_bound);
        tables
            .chain(pipelines)
            .chain(spectral)
            .fold(lists, usize::saturating_add)
            .saturating_add(link)
    }

    /// The `c2sp` and `s2cp` tags, where the profile's viewing conditions
    /// are not the standard ones and it carries them.
    fn custom_elements(&self) -> impl Iterator<Item = &Element<mpe::Pipeline>> {
        self.custom
            .iter()
            .flat_map(|custom| custom.to_standard.iter().chain(&custom.from_standard))
    }

    /// The most bytes a transform's grid over one of the tables or pipelines
    /// can take ([`grid_bytes`]), the largest of theirs: an element's inputs
    /// are its own, read or not — the byte at 8 that `mft1`, `mft2` and
    /// `mAB ` spell them in, the sixteen bits there that `mpet` does — and
    /// need not be another element's.
    fn grid_bound(&self) -> usize {
        let tables = self.tables.iter().map(|table| {
            table.inputs(
                |lut| lut.inputs,
                |raw| raw.get(8).map_or(0, |&inputs| usize::from(inputs)),
            )
        });
        let pipelines = self.pipelines.iter().chain(&self.spectral).map(|pipeline| {
            pipeline.inputs(mpe::Pipeline::inputs, |raw| {
                Reader { bytes: raw }.u16(8).map_or(0, usize::from)
            })
        });
        tables.chain(pipelines).map(grid_bytes).max().unwrap_or(0)
    }

    /// The bytes the tables and pipelines hold now: their lists, and each
    /// element's kept bytes and what reading it gave, where it has been read,
    /// the `B2A*` tables among them.
    #[cfg(test)]
    fn held(&self) -> usize {
        let lists = (self.tables.capacity() + self.outputs.capacity()) * size_of::<Element<Lut>>()
            + (self.pipelines.capacity()
                + self.output_pipelines.capacity()
                + self.spectral.capacity())
                * size_of::<Element<mpe::Pipeline>>()
            + self.weights.capacity() * size_of::<f64>();
        let tables: usize = self
            .tables
            .iter()
            .chain(&self.outputs)
            .chain(&self.gamut)
            .map(|table| table.held(Lut::held_bytes))
            .sum();
        let pipelines: usize = self
            .pipelines
            .iter()
            .chain(&self.output_pipelines)
            .chain(&self.spectral)
            .chain(self.custom_elements())
            .map(|pipeline| pipeline.held(mpe::Pipeline::held_bytes))
            .sum();
        let link = self.link.as_ref().map_or(0, LinkTags::held);
        lists + tables + pipelines + link
    }
}

/// Two sets of transforms are one where their profiles are: what has been
/// compiled so far follows from the profile and from which intents have been
/// asked for, which is not a property of the colour space.
impl PartialEq for Transforms {
    fn eq(&self, other: &Self) -> bool {
        self.profile == other.profile
    }
}

/// A curve, sampled into [`CURVE_ENTRIES`] 16-bit linear values.
fn compile_curve(curve: &Curve) -> Vec<u16> {
    (0..CURVE_ENTRIES)
        .map(|i| {
            let x = i as f64 / (CURVE_ENTRIES - 1) as f64;
            let y = curve.eval(x).clamp(0.0, 1.0);
            (y * 65535.0).round() as u16
        })
        .collect()
}

/// A real number in s15.16, saturating rather than wrapping.
fn fixed(value: f64) -> i32 {
    let scaled = (value * 65536.0).round();
    if scaled > f64::from(i32::MAX) {
        i32::MAX
    } else if scaled < f64::from(i32::MIN) {
        i32::MIN
    } else {
        scaled as i32
    }
}

/// sRGB's transfer function, from a table built once.
///
/// The encode direction — linear light to the byte a display expects — and the
/// only place the output side of the pipeline could have needed a `pow` per
/// pixel. 4 096 entries over `0..=1`, which is finer than the byte it produces.
fn encode_srgb(linear: i64) -> u8 {
    static TABLE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..CURVE_ENTRIES)
            .map(|i| {
                let x = i as f64 / (CURVE_ENTRIES - 1) as f64;
                // IEC 61966-2-1: a straight segment near black, a power law
                // above it, joined so the two agree at the threshold.
                let encoded = if x <= 0.003_130_8 {
                    12.92 * x
                } else {
                    1.055 * tinker_pdf_math::pow(x, 1.0 / 2.4) - 0.055
                };
                (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
            })
            .collect()
    });
    let clamped = linear.clamp(0, ONE);
    let index = (clamped * (CURVE_ENTRIES as i64 - 1) / ONE) as usize;
    table.get(index).copied().unwrap_or(0)
}

/// The colour space a profile's transform stands in for, when one cannot be
/// built: 8.6.5.5's alternate-space reading, by component count.
#[must_use]
pub fn approximated(components: usize) -> ColorSpace {
    ColorSpace::Approximated { components }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    std::thread_local! {
        /// Every lookup table this thread has read out of a tag's bytes, so a
        /// test can say how many reads compiling an intent cost.
        pub(super) static TABLE_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        /// Every time this thread has run a lookup table's pipeline, from
        /// `Lut::evaluate` to linear sRGB: what a grid's nodes cost, and what
        /// a conversion through a filled grid must not.
        pub(super) static PIPELINE_EVALUATIONS: std::cell::Cell<u64> =
            const { std::cell::Cell::new(0) };
        /// Every call of [`Transform::apply`] on this thread.
        pub(super) static APPLY_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        /// Every set of row tables a transform has built on this thread.
        pub(super) static ROW_TABLE_BUILDS: std::cell::Cell<u64> =
            const { std::cell::Cell::new(0) };
    }

    /// The pipeline evaluations this thread has made since the last call.
    fn pipeline_evaluations() -> u64 {
        PIPELINE_EVALUATIONS.with(|count| count.replace(0))
    }

    /// sRGB's own primaries, chromatically adapted to D50 — which is what an
    /// sRGB ICC profile stores, because ICC.1 puts the connection space there.
    ///
    /// These are the numbers widely published sRGB profiles carry;
    /// [`SRGB_PROFILE`], computed from IEC 61966-2-1's chromaticities,
    /// differs from them in the fifth decimal place. They are here rather
    /// than in the module because they are a *fixture*: the module reads
    /// whatever a profile says.
    const SRGB_R: [f64; 3] = [0.436_065_674, 0.222_488_403, 0.013_916_015];
    const SRGB_G: [f64; 3] = [0.385_147_095, 0.716_873_169, 0.097_076_416];
    const SRGB_B: [f64; 3] = [0.143_066_406, 0.060_607_910, 0.714_096_069];

    /// A `para` type 3 tag carrying sRGB's transfer function.
    fn srgb_curve() -> Vec<u8> {
        let mut out = b"para".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        for value in [2.4f64, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.040_45] {
            out.extend_from_slice(&(((value * 65536.0).round() as i32) as u32).to_be_bytes());
        }
        out
    }

    /// A profile that says "these colours are already sRGB".
    fn srgb_profile() -> Vec<u8> {
        build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(SRGB_R[0], SRGB_R[1], SRGB_R[2])),
                (*b"gXYZ", xyz(SRGB_G[0], SRGB_G[1], SRGB_G[2])),
                (*b"bXYZ", xyz(SRGB_B[0], SRGB_B[1], SRGB_B[2])),
                (*b"rTRC", srgb_curve()),
                (*b"gTRC", srgb_curve()),
                (*b"bTRC", srgb_curve()),
                (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
            ],
        )
    }

    /// A minimal monitor profile: a header, a tag count, and the tags asked
    /// for.
    fn build(space: &[u8; 4], pcs: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        build_as(b"mntr", space, pcs, tags)
    }

    /// [`build`], of the device class `class`.
    fn build_as(
        class: &[u8; 4],
        space: &[u8; 4],
        pcs: &[u8; 4],
        tags: &[([u8; 4], Vec<u8>)],
    ) -> Vec<u8> {
        let mut header = vec![0u8; 128];
        header[12..16].copy_from_slice(class);
        header[16..20].copy_from_slice(space);
        header[20..24].copy_from_slice(pcs);
        header[36..40].copy_from_slice(b"acsp");
        header[8..12].copy_from_slice(&0x0200_0000u32.to_be_bytes());

        let mut table = Vec::new();
        table.extend_from_slice(&(tags.len() as u32).to_be_bytes());
        let mut body = Vec::new();
        let start = 132 + tags.len() * 12;
        for (signature, data) in tags {
            table.extend_from_slice(signature);
            table.extend_from_slice(&((start + body.len()) as u32).to_be_bytes());
            table.extend_from_slice(&(data.len() as u32).to_be_bytes());
            body.extend_from_slice(data);
        }

        let mut out = header;
        out.extend_from_slice(&table);
        out.extend_from_slice(&body);
        let size = out.len() as u32;
        out[0..4].copy_from_slice(&size.to_be_bytes());
        out
    }

    fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut out = b"XYZ ".to_vec();
        out.extend_from_slice(&[0; 4]);
        for value in [x, y, z] {
            out.extend_from_slice(&(((value * 65536.0).round() as i32) as u32).to_be_bytes());
        }
        out
    }

    fn gamma(g: f64) -> Vec<u8> {
        let mut out = b"curv".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&((g * 256.0).round() as u16).to_be_bytes());
        out
    }

    /// The seeds `fuzz/corpus/icc_profile/` carries, written from the
    /// fixtures above so the two cannot drift apart.
    ///
    /// Run with `--ignored` when a fixture changes; the corpus is committed,
    /// and a run that rewrites it is a diff to look at rather than to apply
    /// blindly. The same arrangement `pki_cms`, `pki_der`, `crypt` and `cff`
    /// use, for the reason those files record: a hand-laid corpus that no
    /// longer reaches what it was chosen for looks exactly like one that does.
    ///
    /// **`icc_profile` was the one target with no seeds at all** — found by
    /// listing `fuzz/Cargo.toml`'s targets against `fuzz/corpus`'s
    /// directories rather than by reading either. Its twenty seconds in the
    /// `fuzz-seeds` job were spent on random bytes, which for a format whose
    /// first gate is the `acsp` signature at byte 36 means the parser was
    /// reached essentially never.
    ///
    /// # Why half of these are profiles this build refuses
    ///
    /// The fuzz target returns the moment `Profile::parse` says no, so a
    /// refused seed exercises less than an accepted one — but it is not
    /// nothing, and it is the *interesting* less. `NeedsLut` and
    /// `MissingTags` are reached **after** the header check, the tag count
    /// and the whole `132 + i * 12` table walk, which is where this module's
    /// doc comment says ruling 1 lives. A corpus of only-accepted profiles
    /// would leave the refusal paths to random mutation.
    ///
    /// So each seed states which it is, and the test asserts it. A seed that
    /// quietly changed from parsing to refusing would otherwise sit in the
    /// corpus looking like the coverage it no longer is.
    #[test]
    #[ignore = "writes into fuzz/corpus/, which is committed"]
    fn write_the_fuzz_seeds() {
        /// What this build does with a seed, asserted so it cannot drift.
        enum Reaches {
            /// Parses, so the target also drives `Transform`.
            Transform,
            /// Parses, and every intent's transform out of the connection
            /// space compiles too, so the target drives `Output` as well.
            Output,
            /// Parses as a device link, whose transform the target drives,
            /// and builds no transform to sRGB.
            Link,
            /// Parses with named colours, which the target looks up and
            /// converts, and builds no transform.
            Named,
            /// Refused by name, after the tag table was walked.
            Refusal(IccError),
        }

        let seeds: [(&str, Vec<u8>, Reaches); 20] = [
            // An RGB version 5 profile whose connection space is reflectance
            // spectra over four wavelengths, its `D2B0` a matrix from the
            // device to them, its `svcn` the CIE 1931 observer under D65 as
            // four steps of its own, and its `c2sp`, `s2cp` and `B2A0`: the
            // seed that reaches the spectral reduction, the viewing
            // conditions and the connection tags both ways.
            ("spectral-v5", spectral_seed(), Reaches::Output),
            // A version 5 profile whose `A2B0` is a calculator element —
            // channels in and out, a matrix sub-element, temporary channels,
            // a conditional and a selection — and whose `B2A0` takes the
            // connection space through sampled calculator curves: the seed
            // that reaches the calculator and its validation.
            ("calc-v5", calc_seed(), Reaches::Output),
            // A matrix/TRC profile with a `D2B0` of gamma curves and a
            // matrix, and a `B2D0` of single sampled curves, a matrix and an
            // extended grid of 16-bit values: the seed that reaches the
            // multi-process reader out of the connection space and ICC.2's
            // elements.
            ("b2d-elements", b2d_seed(), Reaches::Output),
            // A version 5 profile whose `A2B0` and `B2A0` are multi-process
            // elements — ICC.2's formula segments, a single sampled curve,
            // an extended grid — the seed that reaches the version 5 header
            // and its tags.
            ("v5-mpet", v5_seed(), Reaches::Output),
            // A named-colour profile (ICC.1 8.9) over CMYK and Lab whose
            // `ncl2` names three inks with a prefix and a suffix, each with
            // its four device coordinates: the seed that reaches the
            // named-colour reader.
            ("named-nmcl", named_seed(), Reaches::Named),
            // [`b2a_seed`]'s profile with a gamut tag beside its `B2A0`: the
            // seed that reaches the `gamt` reader and its answer.
            ("gamut-mft2-cmyk", gamut_seed(), Reaches::Output),
            // A CMYK-to-CMYK device link carrying a `D2B0`, a matrix that
            // swaps two inks and halves a third, and an `mft2` `A2B0` beside
            // it: the seed that reaches the link readers, the elements first
            // as ICC.1 8.10.3 has them and the table where they break.
            ("link-cmyk", link_seed(), Reaches::Link),
            // A CMYK printer profile carrying both directions over Lab: an
            // `A2B0` of four inks to the connection space and a `B2A0` back,
            // each an `mft2`, the seed that reaches the output tables.
            ("b2a-lut16-cmyk", b2a_seed(), Reaches::Output),
            // The same profile written the v4 way: an `mAB ` forward and an
            // `mBA ` back carrying all five of its stages, the seed that
            // reaches v4's output table.
            ("b2a-mba-cmyk", mba_seed(), Reaches::Output),
            // `D2B0` and `D2B1` naming one multi-process element, a curve
            // set with every segment kind, a matrix and a grid: the seed
            // that reaches the `mpet` reader and its stages.
            (
                "d2b-elements",
                build_sharing(
                    b"RGB ",
                    b"XYZ ",
                    &[(*b"D2B0", 0), (*b"D2B1", 0)],
                    &[Chain { t: 0.0 }.tag()],
                ),
                Reaches::Transform,
            ),
            // Three lookup tables, one an intent, each a different grid: the
            // seed that reaches every intent's own table.
            (
                "lut-three-intents",
                three_intents_profile(),
                Reaches::Transform,
            ),
            // A matrix/TRC model with an `A2B1` beside it: relative
            // colorimetric converts through the table and perceptual and
            // saturation through the model, as ICC.1 8.10.2 orders them.
            (
                "model-and-table",
                model_and_tables(
                    b"RGB ",
                    b"XYZ ",
                    &matrix_tags(),
                    3,
                    [false, true, false],
                    [false; 3],
                ),
                Reaches::Transform,
            ),
            // Matrix columns over a data space that is not RGB, which the
            // matrix branch refuses, beside an `A2B0` every intent converts
            // through instead: the model passed over.
            (
                "contradicting-model-and-table",
                model_and_tables(
                    b"LAB ",
                    b"XYZ ",
                    &matrix_tags()[..3],
                    3,
                    [true, false, false],
                    [false; 3],
                ),
                Reaches::Transform,
            ),
            // Three-component matrix/TRC, the commonest profile there is.
            ("srgb-para-curve", srgb_profile(), Reaches::Transform),
            // The engine's own sRGB profile, as `build.rs` writes it: a
            // sampled `curv` the three curves share, `desc`'s and `cprt`'s
            // text types and a `chad` this module does not read.
            (
                "srgb-first-party",
                SRGB_PROFILE.to_vec(),
                Reaches::Transform,
            ),
            // The other curve encoding: `curv` with a single gamma word,
            // which is a different branch of `read_curve` from `para`.
            ("rgb-gamma-curve", matrix_profile(), Reaches::Transform),
            // One component, reached through `kTRC` rather than the three
            // `*XYZ` tags.
            (
                "grey-gamma",
                build(
                    b"GRAY",
                    b"XYZ ",
                    &[(*b"kTRC", gamma(1.8)), (*b"wtpt", xyz(0.9642, 1.0, 0.8249))],
                ),
                Reaches::Transform,
            ),
            // Four components into Lab through a lookup table: the tag the
            // parser reaches by a wholly separate path, and the one whose
            // internal offsets are its own arithmetic. Refused, and refused
            // *after* all of that has been read.
            (
                "cmyk-lut",
                build(b"CMYK", b"Lab ", &[(*b"A2B0", vec![0; 32])]),
                Reaches::Refusal(IccError::NeedsLut),
            ),
            // A tag table with no entries. Legal, and the boundary of the
            // `132 + i * 12` arithmetic from below.
            (
                "no-tags",
                build(b"RGB ", b"XYZ ", &[]),
                Reaches::Refusal(IccError::MissingTags),
            ),
            // A tag whose data is empty: an offset in range and a size of
            // zero, which is the other end of the same arithmetic.
            (
                "empty-tag",
                build(b"RGB ", b"XYZ ", &[(*b"rXYZ", Vec::new())]),
                Reaches::Refusal(IccError::MissingTags),
            ),
        ];

        // Asserted before anything is written, so a drifted fixture leaves
        // the committed corpus alone rather than half-rewriting it.
        for (name, bytes, reaches) in &seeds {
            let got = Profile::parse(bytes);
            match reaches {
                Reaches::Named => {
                    let profile = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    assert!(
                        profile
                            .named
                            .as_ref()
                            .is_some_and(|named| !named.is_empty()),
                        "{name}: no named colours"
                    );
                    assert!(Transforms::compile(profile).is_none(), "{name}");
                }
                Reaches::Link => {
                    let profile = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    assert!(Link::compile(&profile).is_ok(), "{name}: no link");
                    for intent in Intent::ALL {
                        assert!(
                            Transform::compile(&profile, intent).is_none(),
                            "{name}: a link built a transform to sRGB"
                        );
                    }
                }
                Reaches::Transform | Reaches::Output => {
                    let profile = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    for intent in Intent::ALL {
                        assert!(
                            Transform::compile(&profile, intent).is_some(),
                            "{name}: no {intent:?} transform"
                        );
                        if matches!(reaches, Reaches::Output) {
                            assert!(
                                Output::compile(&profile, intent).is_ok(),
                                "{name}: no {intent:?} transform out of the connection space"
                            );
                        }
                    }
                }
                Reaches::Refusal(expected) => {
                    assert_eq!(got.err().as_ref(), Some(expected), "{name}");
                }
            }
        }

        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/icc_profile");
        std::fs::create_dir_all(&base).expect("the corpus directory is creatable");
        for (name, bytes, _) in &seeds {
            std::fs::write(base.join(format!("{name}.icc")), bytes)
                .unwrap_or_else(|error| panic!("writing {name}: {error}"));
        }
    }

    /// An `mft2` of `inputs` and `outputs` over a two-point grid, its matrix
    /// and curves the identity, node `n`'s channel `c` the value
    /// `value(n, c)`, the first input's axis slowest.
    fn graded_mft2(inputs: usize, outputs: usize, value: impl Fn(usize, usize) -> u16) -> Vec<u8> {
        let mut out = b"mft2".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[inputs as u8, outputs as u8, 2, 0]);
        for row in 0..3 {
            for column in 0..3 {
                let entry: u32 = if row == column { 0x0001_0000 } else { 0 };
                out.extend_from_slice(&entry.to_be_bytes());
            }
        }
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&2u16.to_be_bytes());
        for _ in 0..inputs {
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&u16::MAX.to_be_bytes());
        }
        for node in 0..1usize << inputs {
            for channel in 0..outputs {
                out.extend_from_slice(&value(node, channel).to_be_bytes());
            }
        }
        for _ in 0..outputs {
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&u16::MAX.to_be_bytes());
        }
        out
    }

    /// The `b2a-lut16-cmyk` seed: a CMYK profile over Lab whose `A2B0` takes
    /// each corner of the ink cube to a grey that darkens with every ink it
    /// holds — `L*` from `FF00h` down `3000h` an ink, `a*` and `b*` at
    /// `8000h` — and whose `B2A0` gives each corner of the connection
    /// space's cube four inks spread over the range, `((5n + 3c) mod 17) ×
    /// 3 855` for node `n`'s channel `c`.
    fn b2a_seed() -> Vec<u8> {
        build(
            b"CMYK",
            b"Lab ",
            &[
                (
                    *b"A2B0",
                    graded_mft2(4, 3, |node, channel| match channel {
                        0 => 0xFF00 - node.count_ones() as u16 * 0x3000,
                        _ => 0x8000,
                    }),
                ),
                (
                    *b"B2A0",
                    graded_mft2(3, 4, |node, channel| {
                        (((node * 5 + channel * 3) % 17) * 3855) as u16
                    }),
                ),
            ],
        )
    }

    /// An `ncl2` tag (ICC.1 10.15, Table 63) naming `entries` — a root name,
    /// three connection-space coordinates and `device` device coordinates
    /// each — with `prefix` and `suffix`, its vendor flag zero.
    fn ncl2_tag(
        prefix: &str,
        suffix: &str,
        device: usize,
        entries: &[(&str, [u16; 3], &[u16])],
    ) -> Vec<u8> {
        let field = |text: &str| {
            let mut out = text.as_bytes().to_vec();
            out.resize(32, 0);
            out
        };
        let mut out = b"ncl2".to_vec();
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        out.extend_from_slice(&(device as u32).to_be_bytes());
        out.extend(field(prefix));
        out.extend(field(suffix));
        for (root, pcs, coordinates) in entries {
            out.extend(field(root));
            for value in pcs.iter().chain(coordinates.iter()) {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        out
    }

    /// The `named-nmcl` seed: a named-colour profile over CMYK and Lab whose
    /// `ncl2` names `Ink 1 C`, `Ink 2 C` and `Ink 3 C`, each with its legacy
    /// Lab coordinates and four device coordinates.
    fn named_seed() -> Vec<u8> {
        build_as(
            b"nmcl",
            b"CMYK",
            b"Lab ",
            &[(
                *b"ncl2",
                ncl2_tag(
                    "Ink ",
                    " C",
                    4,
                    &[
                        ("1", [0x7A00, 0xD200, 0xA600], &[0, 0xF5C2, 0xC28F, 0]),
                        ("2", [0x5F00, 0x6E00, 0x2800], &[0xFFFF, 0x2E14, 0, 0x051E]),
                        (
                            "3",
                            [0x8A00, 0x7F80, 0x7C00],
                            &[0x3333, 0x2666, 0x199A, 0x570A],
                        ),
                    ],
                ),
            )],
        )
    }

    /// The `gamut-mft2-cmyk` seed: [`b2a_seed`]'s two tables, and a `gamt`
    /// (ICC.1 9.2.28), an `mft2` from the connection space to one channel
    /// over a two-point grid, whose corners holding at most one full
    /// component are zero, in gamut, and the rest `FFFFh`, out.
    fn gamut_seed() -> Vec<u8> {
        build(
            b"CMYK",
            b"Lab ",
            &[
                (
                    *b"A2B0",
                    graded_mft2(4, 3, |node, channel| match channel {
                        0 => 0xFF00 - node.count_ones() as u16 * 0x3000,
                        _ => 0x8000,
                    }),
                ),
                (
                    *b"B2A0",
                    graded_mft2(3, 4, |node, channel| {
                        (((node * 5 + channel * 3) % 17) * 3855) as u16
                    }),
                ),
                (
                    *b"gamt",
                    graded_mft2(
                        3,
                        1,
                        |node, _| {
                            if node.count_ones() <= 1 {
                                0
                            } else {
                                0xFFFF
                            }
                        },
                    ),
                ),
            ],
        )
    }

    /// A single sampled curve (`sngf`, ICC.2 11.2.2.2) of float32 `entries`
    /// from `first` to `last`, extended past them where `extend` is one.
    fn sngf(first: f64, last: f64, extend: u16, entries: &[f64]) -> Vec<u8> {
        let mut out = b"sngf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        out.extend_from_slice(&float(first));
        out.extend_from_slice(&float(last));
        out.extend_from_slice(&extend.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        for &entry in entries {
            out.extend_from_slice(&float(entry));
        }
        out
    }

    /// An extended grid (`xclt`, ICC.2 11.2.7) of `grids` points an axis
    /// giving `q` outputs, its 16-bit `values`, the last axis fastest.
    fn xclt(grids: &[u8], q: u16, values: &[u16]) -> Vec<u8> {
        let mut body = 2u32.to_be_bytes().to_vec();
        let mut counts = [0u8; 16];
        counts[..grids.len()].copy_from_slice(grids);
        body.extend_from_slice(&counts);
        for value in values {
            body.extend_from_slice(&value.to_be_bytes());
        }
        element_of(b"xclt", grids.len() as u16, q, &body)
    }

    /// The `b2d-elements` seed: a matrix/TRC profile whose `D2B0` takes each
    /// channel through a gamma of 2.2 and mixes them, and whose `B2D0` takes
    /// the connection space through single sampled curves, one extended past
    /// its last entry, a matrix and an extended grid of 16-bit values, node
    /// `n`'s channel `c` `((3n + 5c) mod 11) × 6 553`.
    fn b2d_seed() -> Vec<u8> {
        let gamma = curf(&[], &[parf(0, &[2.2, 1.0, 0.0, 0.0])]);
        let mix = matf(
            &[
                vec![0.5, 0.3, 0.2],
                vec![0.25, 0.65, 0.1],
                vec![0.05, 0.15, 0.8],
            ],
            &[0.0, 0.0, 0.0],
        );
        let mut tags = matrix_tags();
        tags.push((
            *b"D2B0",
            mpet(3, 3, &[cvst(&[gamma.clone(), gamma.clone(), gamma]), mix]),
        ));
        let values: Vec<u16> = (0..8usize)
            .flat_map(|node| (0..3usize).map(move |c| (((node * 3 + c * 5) % 11) * 6553) as u16))
            .collect();
        tags.push((
            *b"B2D0",
            mpet(
                3,
                3,
                &[
                    cvst(&[
                        sngf(0.0, 2.0, 0, &[0.0, 0.3, 0.6, 1.0]),
                        sngf(0.0, 1.0, 1, &[0.0, 0.5, 0.9]),
                        sngf(0.0, 2.0, 0, &[0.0, 1.0]),
                    ]),
                    matf(
                        &[
                            vec![1.0, -0.1, 0.0],
                            vec![0.0, 1.0, -0.1],
                            vec![-0.1, 0.0, 1.0],
                        ],
                        &[0.05, 0.05, 0.05],
                    ),
                    xclt(&[2, 2, 2], 3, &values),
                ],
            ),
        ));
        build(b"RGB ", b"XYZ ", &tags)
    }

    /// The `v5-mpet` seed: a version 5 display profile whose `A2B0` takes
    /// each channel through ICC.2's formula segments and a single sampled
    /// curve, then a matrix and an extended grid of 16-bit values, and whose
    /// `B2A0` takes the connection space back through a curve of type 3 on
    /// each channel and a matrix.
    fn v5_seed() -> Vec<u8> {
        let values: Vec<u16> = (0..8usize)
            .flat_map(|node| {
                let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
                [
                    (8_000 + 40_000 * i) as u16,
                    (9_000 + 45_000 * j) as u16,
                    (7_000 + 35_000 * k) as u16,
                ]
            })
            .collect();
        let forward = mpet(
            3,
            3,
            &[
                cvst(&[
                    curf(
                        &[0.5],
                        &[
                            parf(3, &[2.0, 1.0, 1.0, 0.0, 0.0]),
                            parf(5, &[1.0, 1.0, 0.0, 0.0, 1.0, 0.25]),
                        ],
                    ),
                    sngf(0.0, 1.0, 1, &[0.0, 0.4, 1.0]),
                    curf(&[], &[parf(7, &[1.0, 1.0, 0.0, 1.0, 0.5, 1.0])]),
                ]),
                matf(
                    &[
                        vec![0.6, 0.3, 0.1],
                        vec![0.2, 0.7, 0.1],
                        vec![0.1, 0.1, 0.8],
                    ],
                    &[0.0, 0.0, 0.0],
                ),
                xclt(&[2, 2, 2], 3, &values),
            ],
        );
        let half = curf(&[], &[parf(3, &[1.0, 0.5, 1.0, 0.0, 0.0])]);
        let back = mpet(
            3,
            3,
            &[
                cvst(&[half.clone(), half.clone(), half]),
                matf(
                    &[
                        vec![1.2, -0.2, 0.0],
                        vec![-0.1, 1.1, 0.0],
                        vec![0.0, -0.1, 1.1],
                    ],
                    &[0.0, 0.0, 0.0],
                ),
            ],
        );
        let mut out = build(b"RGB ", b"XYZ ", &[(*b"A2B0", forward), (*b"B2A0", back)]);
        out[8..12].copy_from_slice(&0x0500_0000u32.to_be_bytes());
        out
    }

    /// A calculator operation (ICC.2 11.2.1) of a 16-bit `s` and `t`.
    fn calc_op(signature: &[u8; 4], s: u16, t: u16) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(signature);
        out[4..6].copy_from_slice(&s.to_be_bytes());
        out[6..].copy_from_slice(&t.to_be_bytes());
        out
    }

    /// A calculator operation carrying a 32-bit count, or a float32's bits.
    fn calc_word(signature: &[u8; 4], word: u32) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(signature);
        out[4..].copy_from_slice(&word.to_be_bytes());
        out
    }

    /// `data`: `value` pushed, as a float32.
    fn calc_data(value: f64) -> [u8; 8] {
        calc_word(b"data", (value as f32).to_bits())
    }

    /// A calculator element of `inputs` and `outputs` channels whose main
    /// function is `ops`, after its header and sub-element positions, and
    /// whose sub-elements `subs` follow it, each on a four-byte boundary.
    fn calc_of(inputs: u16, outputs: u16, ops: &[[u8; 8]], subs: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"calc".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&inputs.to_be_bytes());
        out.extend_from_slice(&outputs.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&(subs.len() as u16).to_be_bytes());
        let header = 24 + 8 * subs.len();
        let function = 12 + 8 * ops.len();
        out.extend_from_slice(&(header as u32).to_be_bytes());
        out.extend_from_slice(&(function as u32).to_be_bytes());
        let start = header + function;
        let mut body = Vec::new();
        for sub in subs {
            while (start + body.len()) % 4 != 0 {
                body.push(0);
            }
            out.extend_from_slice(&((start + body.len()) as u32).to_be_bytes());
            out.extend_from_slice(&(sub.len() as u32).to_be_bytes());
            body.extend_from_slice(sub);
        }
        out.extend_from_slice(b"func");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(ops.len() as u32).to_be_bytes());
        for op in ops {
            out.extend_from_slice(op);
        }
        out.extend(body);
        out
    }

    /// A sampled calculator curve (`clcf`, ICC.2 11.2.2.3) of `entries`
    /// recommended samples from `first` to `last`, clipped past them, its
    /// curve `calculator`'s.
    fn clcf(entries: u32, first: f64, last: f64, calculator: &[u8]) -> Vec<u8> {
        let mut out = b"clcf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&entries.to_be_bytes());
        out.extend_from_slice(&float(first));
        out.extend_from_slice(&float(last));
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(calculator);
        out
    }

    /// The `calc-v5` seed: a version 5 display profile whose `A2B0` is one
    /// calculator element — its inputs through a matrix sub-element into
    /// three temporary channels, then the second where the first is at
    /// least a half and the third otherwise, the second times three
    /// selecting one of three constants, and the third's square root — and
    /// whose `B2A0` squares each component through sampled calculator
    /// curves of sixteen samples over XYZ's range, then mixes them.
    fn spectral_seed() -> Vec<u8> {
        let spectrum = mpet(
            3,
            4,
            &[matf(
                &[
                    vec![0.6, 0.2, 0.1],
                    vec![0.3, 0.5, 0.1],
                    vec![0.1, 0.4, 0.3],
                    vec![0.05, 0.2, 0.6],
                ],
                &[0.05, 0.05, 0.05, 0.05],
            )],
        );
        // 400 nm and 700 nm as half floats, and four steps.
        let range: [u16; 3] = [0x5E40, 0x6178, 4];
        let range_bytes: Vec<u8> = range.iter().flat_map(|value| value.to_be_bytes()).collect();
        let mut viewing = b"svcn".to_vec();
        viewing.extend_from_slice(&[0; 4]);
        viewing.extend_from_slice(&1u32.to_be_bytes());
        viewing.extend_from_slice(&range_bytes);
        viewing.extend_from_slice(&[0; 2]);
        for value in [0.2, 0.4, 1.0, 0.3, 0.1, 0.9, 0.6, 0.1, 1.2, 0.3, 0.0, 0.0] {
            viewing.extend_from_slice(&float(value));
        }
        viewing.extend_from_slice(&2u32.to_be_bytes());
        viewing.extend_from_slice(&float(6504.0));
        viewing.extend_from_slice(&range_bytes);
        viewing.extend_from_slice(&[0; 2]);
        for value in [0.8, 1.0, 1.1, 0.9] {
            viewing.extend_from_slice(&float(value));
        }
        viewing.extend_from_slice(&[0; 24]);
        let diagonal = |values: [f64; 3]| {
            mpet(
                3,
                3,
                &[matf(
                    &[
                        vec![values[0], 0.0, 0.0],
                        vec![0.0, values[1], 0.0],
                        vec![0.0, 0.0, values[2]],
                    ],
                    &[0.0, 0.0, 0.0],
                )],
            )
        };
        let mut out = build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"D2B0", spectrum),
                (*b"svcn", viewing),
                (*b"c2sp", diagonal([0.95, 1.0, 1.25])),
                (*b"s2cp", diagonal([1.0 / 0.95, 1.0, 0.8])),
                (*b"B2A0", diagonal([1.0, 1.0, 1.0])),
            ],
        );
        out[8..12].copy_from_slice(&0x0500_0000u32.to_be_bytes());
        out[100..104].copy_from_slice(b"rs\x00\x04");
        out[104..110].copy_from_slice(&range_bytes);
        out
    }

    fn calc_seed() -> Vec<u8> {
        let mixing = matf(
            &[
                vec![0.6, 0.3, 0.1],
                vec![0.2, 0.7, 0.1],
                vec![0.1, 0.1, 0.8],
            ],
            &[0.0, 0.0, 0.0],
        );
        let program = [
            calc_op(b"in  ", 0, 2),
            calc_op(b"mtx ", 0, 0),
            calc_op(b"tput", 0, 2),
            calc_op(b"tget", 0, 0),
            calc_word(b"if  ", 1),
            calc_word(b"else", 1),
            calc_op(b"tget", 1, 0),
            calc_op(b"tget", 2, 0),
            calc_op(b"tget", 1, 0),
            calc_data(3.0),
            calc_op(b"mul ", 0, 0),
            calc_word(b"sel ", 0),
            calc_word(b"case", 1),
            calc_word(b"case", 1),
            calc_word(b"dflt", 1),
            calc_data(0.1),
            calc_data(0.2),
            calc_data(0.3),
            calc_op(b"tget", 2, 0),
            calc_op(b"sqrt", 0, 0),
            calc_op(b"out ", 0, 2),
        ];
        let forward = mpet(3, 3, &[calc_of(3, 3, &program, &[mixing])]);
        let square = calc_of(
            1,
            1,
            &[
                calc_op(b"in  ", 0, 0),
                calc_op(b"sq  ", 0, 0),
                calc_op(b"out ", 0, 0),
            ],
            &[],
        );
        let curve = clcf(16, 0.0, 2.0, &square);
        let back = mpet(
            3,
            3,
            &[
                cvst(&[curve.clone(), curve.clone(), curve]),
                matf(
                    &[
                        vec![1.0, 0.0, 0.0],
                        vec![0.0, 1.0, 0.0],
                        vec![0.0, 0.0, 1.0],
                    ],
                    &[0.0, 0.0, 0.0],
                ),
            ],
        );
        let mut out = build(b"RGB ", b"XYZ ", &[(*b"A2B0", forward), (*b"B2A0", back)]);
        out[8..12].copy_from_slice(&0x0500_0000u32.to_be_bytes());
        out
    }

    /// The `link-cmyk` seed: a CMYK-to-CMYK device link (ICC.1 8.6) whose
    /// `D2B0` is one matrix element taking `(c, m, y, k)` to `(m, c, y, k/2
    /// + 1/4)`, and whose `A2B0`, an `mft2` over a two-point grid, gives node
    /// `n`'s channel `c` `((3n + 7c) mod 13) × 5 041`.
    fn link_seed() -> Vec<u8> {
        build_as(
            b"link",
            b"CMYK",
            b"CMYK",
            &[
                (
                    *b"D2B0",
                    mpet(
                        4,
                        4,
                        &[matf(
                            &[
                                vec![0.0, 1.0, 0.0, 0.0],
                                vec![1.0, 0.0, 0.0, 0.0],
                                vec![0.0, 0.0, 1.0, 0.0],
                                vec![0.0, 0.0, 0.0, 0.5],
                            ],
                            &[0.0, 0.0, 0.0, 0.25],
                        )],
                    ),
                ),
                (
                    *b"A2B0",
                    graded_mft2(4, 4, |node, channel| {
                        (((node * 3 + channel * 7) % 13) * 5041) as u16
                    }),
                ),
            ],
        )
    }

    /// The `b2a-mba-cmyk` seed: [`b2a_seed`]'s profile written the v4 way.
    /// Its `A2B0` is an `mAB ` of identity A curves, the same graded grid of
    /// the ink cube's corners and identity B curves; its `B2A0` an `mBA `
    /// carrying all five stages — B curves of gamma 1.5, 1 and 2, a matrix
    /// with offsets, M curves of gamma 2, 1 and 0.5, a grid of three points
    /// on its first axis and two on the others giving four inks, `((7n + 5c)
    /// mod 23) × 2 849` for node `n`'s channel `c`, and A curves of gamma 1,
    /// 1.8, 2.2 and 1.
    fn mba_seed() -> Vec<u8> {
        let forward = V4Stages {
            b: identities(3),
            clut: Some((
                vec![2; 4],
                (0..16usize)
                    .flat_map(|node| [0xFF00 - node.count_ones() as u16 * 0x3000, 0x8000, 0x8000])
                    .collect(),
            )),
            a: identities(4),
            ..V4Stages::default()
        };
        let output = V4Stages {
            b: vec![gamma(1.5), gamma(1.0), gamma(2.0)],
            matrix: Some([
                0.5, 0.25, 0.25, 0.0, 1.0, 0.0, 0.25, 0.0, 0.75, 0.0, 0.0625, -0.0625,
            ]),
            m: vec![gamma(2.0), gamma(1.0), gamma(0.5)],
            clut: Some((
                vec![3, 2, 2],
                (0..12usize)
                    .flat_map(|node| {
                        (0..4usize)
                            .map(move |channel| (((node * 7 + channel * 5) % 23) * 2849) as u16)
                    })
                    .collect(),
            )),
            a: vec![gamma(1.0), gamma(1.8), gamma(2.2), gamma(1.0)],
        };
        build(
            b"CMYK",
            b"Lab ",
            &[
                (*b"A2B0", forward.tag(b"mAB ", 4, 3)),
                (*b"B2A0", output.tag(b"mBA ", 3, 4)),
            ],
        )
    }

    fn matrix_profile() -> Vec<u8> {
        build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(0.4360, 0.2225, 0.0139)),
                (*b"gXYZ", xyz(0.3851, 0.7169, 0.0971)),
                (*b"bXYZ", xyz(0.1431, 0.0606, 0.7141)),
                (*b"rTRC", gamma(2.2)),
                (*b"gTRC", gamma(2.2)),
                (*b"bTRC", gamma(2.2)),
                (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
            ],
        )
    }

    /// **A matrix/TRC profile parses into its columns and its curves**, which
    /// is 95 % of what the corpus carries.
    #[test]
    fn a_matrix_profile_reads_its_columns_and_curves() {
        let profile = Profile::parse(&matrix_profile()).expect("a well-formed profile");
        assert_eq!(&profile.space, b"RGB ");
        assert_eq!(profile.version, 2);
        let Some(Model::MatrixTrc { columns, curves }) = profile.model else {
            panic!("expected a matrix model");
        };
        // s15Fixed16 carries about five decimal digits, so the columns come
        // back to within a rounding step of what was written.
        assert!((columns[0][0] - 0.4360).abs() < 1e-4, "{:?}", columns[0]);
        assert!((columns[1][1] - 0.7169).abs() < 1e-4);
        assert!((columns[2][2] - 0.7141).abs() < 1e-4);
        // `curv` with one entry stores gamma as u8Fixed8, whose resolution is
        // 1/256 — so 2.2 comes back as 563/256, and asserting equality with
        // 2.2 would be asserting the format is finer than it is.
        let Curve::Gamma(g) = curves[0] else {
            panic!("expected a gamma curve");
        };
        assert_eq!(g, 563.0 / 256.0, "u8Fixed8 quantises to 1/256");
        assert!((g - 2.2).abs() < 1.0 / 256.0);
    }

    /// §7.2's 128-byte header and a tag count, and nothing behind them: the
    /// smallest thing [`Profile::parse`] will read a count out of.
    ///
    /// The declared size at offset 0 is 132 as well, so a refusal from one of
    /// these is the tag count's and not [`IccError::SizeMismatch`] standing in
    /// front of it.
    fn header_declaring(tags: u32) -> Vec<u8> {
        let mut out = matrix_profile()[..132].to_vec();
        out[0..4].copy_from_slice(&132u32.to_be_bytes());
        out[128..132].copy_from_slice(&tags.to_be_bytes());
        out
    }

    /// **[`MAX_ICC_BYTES`] fires, and it fires before anything else does.**
    ///
    /// The row `crates/tinker-pdf/tests/bounds_ledger.rs` names as proving this
    /// cap. It is a separate test from
    /// [`every_refusal_is_reachable_and_named`], which also builds an
    /// over-large profile among its ten: the ledger asks each bound to name
    /// **one** test that fires **it**, so that a test deleted or renamed fails
    /// the sweep rather than quietly leaving a cap unproven behind nine other
    /// assertions.
    ///
    /// What it adds beyond the refusal is the **order**. The bytes here are all
    /// zero, so there is no `acsp` at offset 36 and no 132-byte header either;
    /// a build that measured the profile after reading its signature would
    /// answer [`IccError::NotAProfile`] or [`IccError::TooShort`] and would
    /// have looked at a 2 MiB allocation first. Ruling 1: the size is checked
    /// before the bytes are.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_profile_past_the_byte_cap_is_refused_before_its_signature_is_read() {
        let over = vec![0u8; MAX_ICC_BYTES + 1];
        assert_eq!(
            Profile::parse(&over),
            Err(IccError::TooLarge),
            "a profile one byte past the cap is refused by name"
        );

        // And exactly at the cap it is *not* this refusal: the same zeros get
        // past the size check and fail on the signature instead, which is what
        // makes the boundary the constant's own rather than an approximation
        // of it.
        let at = vec![0u8; MAX_ICC_BYTES];
        assert_eq!(
            Profile::parse(&at),
            Err(IccError::NotAProfile),
            "a profile exactly at the cap is admitted to the signature check"
        );
    }

    /// **[`MAX_ICC_TAGS`] fires, and it fires before the table is walked.**
    ///
    /// The row `crates/tinker-pdf/tests/bounds_ledger.rs` names as proving this
    /// cap, separate from [`every_refusal_is_reachable_and_named`] for the
    /// reason recorded on the test above.
    ///
    /// The second half is the one worth having. A 132-byte profile — a header
    /// and a count, and nothing behind it — declares 4 294 967 295 tags, and
    /// the tag table it describes would be 51 GiB. Nothing is allocated,
    /// because the count is checked where it is read; a build that walked
    /// first and counted afterwards would answer [`IccError::TooShort`] here,
    /// forty-eight bytes into a table that does not exist.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_tag_count_past_the_cap_is_refused_before_the_table_is_walked() {
        let mut many = matrix_profile();
        many[128..132].copy_from_slice(&(MAX_ICC_TAGS + 1).to_be_bytes());
        assert_eq!(
            Profile::parse(&many),
            Err(IccError::TooLarge),
            "one tag past the cap is refused by name"
        );

        // The reachability the ledger records: the field is 32 bits wide and
        // the header it sits in is 132 bytes long. The declared size at offset
        // 0 is rewritten to match, so what refuses this is the count and not
        // §7.2's own size disagreeing with the stream.
        let header_only = header_declaring(u32::MAX);
        assert_eq!(header_only.len(), 132, "and that is all it takes");
        assert_eq!(
            Profile::parse(&header_only),
            Err(IccError::TooLarge),
            "132 bytes may declare four billion tags, and are refused for it \
             rather than walked"
        );

        // And the cap itself is admitted to the walk, so the constant is the
        // boundary and not one below it: the table is not there, so this is
        // `TooShort` rather than `TooLarge`.
        let at = header_declaring(MAX_ICC_TAGS);
        assert_eq!(
            Profile::parse(&at),
            Err(IccError::TooShort),
            "a count exactly at the cap is walked rather than refused"
        );
    }

    /// **Every refusal is reachable and named**, which is the half of a
    /// capability that a caller relies on: a profile this cannot read must
    /// leave the page exactly as the component-count approximation did.
    #[test]
    fn every_refusal_is_reachable_and_named() {
        assert_eq!(Profile::parse(&[]), Err(IccError::TooShort));
        assert_eq!(
            Profile::parse(&[0u8; 200]),
            Err(IccError::NotAProfile),
            "no acsp"
        );
        assert_eq!(
            Profile::parse(&vec![0u8; MAX_ICC_BYTES + 1][..]),
            Err(IccError::TooLarge)
        );

        // A size field claiming more than arrived.
        let mut truncated = matrix_profile();
        let long = (truncated.len() as u32 + 64).to_be_bytes();
        truncated[0..4].copy_from_slice(&long);
        assert_eq!(Profile::parse(&truncated), Err(IccError::SizeMismatch));

        // A tag whose extent leaves the profile.
        let mut out_of_bounds = matrix_profile();
        let huge = 0xFFFF_0000u32.to_be_bytes();
        out_of_bounds[136..140].copy_from_slice(&huge);
        assert_eq!(
            Profile::parse(&out_of_bounds),
            Err(IccError::TagOutOfBounds)
        );

        // A tag count past the cap.
        let mut many = matrix_profile();
        many[128..132].copy_from_slice(&(MAX_ICC_TAGS + 1).to_be_bytes());
        assert_eq!(Profile::parse(&many), Err(IccError::TooLarge));

        // A profile whose only road to the connection space is a lookup table.
        let lut = build(b"CMYK", b"Lab ", &[(*b"A2B0", vec![0; 32])]);
        assert_eq!(Profile::parse(&lut), Err(IccError::NeedsLut));

        // A connection space a matrix cannot reach.
        let lab_pcs = build(
            b"RGB ",
            b"Lab ",
            &[
                (*b"rXYZ", xyz(1.0, 0.0, 0.0)),
                (*b"gXYZ", xyz(0.0, 1.0, 0.0)),
                (*b"bXYZ", xyz(0.0, 0.0, 1.0)),
            ],
        );
        assert_eq!(Profile::parse(&lab_pcs), Err(IccError::UnsupportedPcs));

        // Columns without curves.
        let partial = build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(1.0, 0.0, 0.0)),
                (*b"gXYZ", xyz(0.0, 1.0, 0.0)),
                (*b"bXYZ", xyz(0.0, 0.0, 1.0)),
            ],
        );
        assert_eq!(Profile::parse(&partial), Err(IccError::MissingTags));

        // A curve tag that is not a curve.
        let bad_curve = build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(1.0, 0.0, 0.0)),
                (*b"gXYZ", xyz(0.0, 1.0, 0.0)),
                (*b"bXYZ", xyz(0.0, 0.0, 1.0)),
                (*b"rTRC", b"nope____".to_vec()),
                (*b"gTRC", gamma(1.0)),
                (*b"bTRC", gamma(1.0)),
            ],
        );
        assert_eq!(Profile::parse(&bad_curve), Err(IccError::MalformedTag));

        // The output direction's: a model with no inverse. Columns all one
        // colour, which still convert forward, and a grey curve that falls.
        let singular = build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(0.3, 0.3, 0.3)),
                (*b"gXYZ", xyz(0.3, 0.3, 0.3)),
                (*b"bXYZ", xyz(0.3, 0.3, 0.3)),
                (*b"rTRC", gamma(1.0)),
                (*b"gTRC", gamma(1.0)),
                (*b"bTRC", gamma(1.0)),
            ],
        );
        let singular = Profile::parse(&singular).expect("one colour still converts forward");
        assert_eq!(
            Output::compile(&singular, Intent::RelativeColorimetric),
            Err(IccError::NoInverse)
        );
        let mut falling = b"curv".to_vec();
        falling.extend_from_slice(&[0; 4]);
        falling.extend_from_slice(&2u32.to_be_bytes());
        falling.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
        let falling = build(b"GRAY", b"XYZ ", &[(*b"kTRC", falling)]);
        let falling = Profile::parse(&falling).expect("a falling curve still converts forward");
        assert_eq!(
            Output::compile(&falling, Intent::Perceptual),
            Err(IccError::NoInverse)
        );

        // A device link with no `A2B0` (8.6 requires one), and nothing else
        // to convert through.
        let empty_link = build_as(
            b"link",
            b"CMYK",
            b"CMYK",
            &[(*b"wtpt", xyz(0.9642, 1.0, 0.8249))],
        );
        assert_eq!(Profile::parse(&empty_link), Err(IccError::MissingTags));
        // One whose connection-space field names no space a registry
        // defines, so nothing says how many channels it gives.
        let unnamed = build_as(
            b"link",
            b"CMYK",
            b"YYY ",
            &[(*b"A2B0", graded_mft2(4, 3, |_, _| 0))],
        );
        assert_eq!(Profile::parse(&unnamed), Err(IccError::UnsupportedSpace));
        // One whose table gives another count than that space has.
        let miscounted = build_as(
            b"link",
            b"CMYK",
            b"RGB ",
            &[(*b"A2B0", graded_mft2(4, 4, |_, _| 0))],
        );
        assert_eq!(Profile::parse(&miscounted), Err(IccError::UnsupportedSpace));
    }

    /// **A device link reads its `D2B0` before its `A2B0`** (ICC.1 8.10.3):
    /// the link seed's matrix element answers where it reads, its `mft2`
    /// where the element is one this build does not read, and a link whose
    /// only tag does not read is refused for it, by name. Its counts are
    /// held before anything is read — sixteen outputs is too many, a table
    /// of four inputs for a space of three another space — and an `mAB `
    /// with a matrix or M curves gives the matrix's three channels or is
    /// malformed. A link builds no transform to sRGB, nor one out of the
    /// connection space, and its tags are counted in the profile's bound.
    #[test]
    fn a_link_reads_its_d2b0_before_its_a2b0() {
        let profile = Profile::parse(&link_seed()).expect("the link seed parses");
        let link = Link::compile(&profile).expect("a link");
        assert_eq!((link.inputs(), link.channels()), (4, 4));
        assert_eq!(link.output_space(), *b"CMYK");
        let got = link.apply(&[0.2, 0.6, 0.4, 0.8]);
        let want = [0.6, 0.2, 0.4, 0.8 * 0.5 + 0.25].map(|v: f64| (v * 65535.0).round());
        for (got, want) in got.channels().iter().zip(want) {
            assert!(
                (f64::from(*got) - want).abs() <= 2.0,
                "the D2B0's matrix: {:?} against {want:?}",
                got
            );
        }
        for intent in Intent::ALL {
            assert!(Transform::compile(&profile, intent).is_none(), "{intent:?}");
            assert_eq!(
                Output::compile(&profile, intent),
                Err(IccError::MissingTags)
            );
            assert_eq!(profile.connection_value(intent, &[0.5; 4]), None);
        }
        assert!(Transforms::compile(profile.clone()).is_none());
        assert!(profile.tables.held_bound() >= profile.tables.held());

        // The D2B0 broken: the A2B0, node by node at the grid's corners.
        let table = graded_mft2(4, 4, |node, channel| {
            (((node * 3 + channel * 7) % 13) * 5041) as u16
        });
        let broken = mpet(4, 4, &[element_of(b"JtoX", 4, 4, &[0; 16])]);
        let fallen = build_as(
            b"link",
            b"CMYK",
            b"CMYK",
            &[(*b"D2B0", broken.clone()), (*b"A2B0", table.clone())],
        );
        let link =
            Link::compile(&Profile::parse(&fallen).expect("the A2B0 serves")).expect("a link");
        for node in 0..16usize {
            let corner: Vec<f64> = (0..4)
                .map(|axis| ((node >> (3 - axis)) & 1) as f64)
                .collect();
            let got = link.apply(&corner);
            for (channel, value) in got.channels().iter().enumerate() {
                let want = (((node * 3 + channel * 7) % 13) * 5041) as u16;
                assert!(
                    value.abs_diff(want) <= 1,
                    "node {node}, channel {channel}: {value} against {want}"
                );
            }
        }
        // Alone, the broken D2B0 refuses the link by the element's name.
        let alone = build_as(b"link", b"CMYK", b"CMYK", &[(*b"D2B0", broken)]);
        assert_eq!(
            Profile::parse(&alone),
            Err(IccError::UnsupportedElement(*b"JtoX"))
        );

        // Counts held before anything is read.
        let link_of = |inputs, outputs| Direction::Link { inputs, outputs };
        assert_eq!(
            read_lut(&graded_mft2(4, 16, |_, _| 0), false, link_of(4, 15)),
            Err(IccError::TooLarge),
            "sixteen outputs"
        );
        assert_eq!(
            read_lut(&graded_mft2(4, 4, |_, _| 0), false, link_of(3, 4)),
            Err(IccError::UnsupportedSpace),
            "four inputs for a space of three"
        );
        assert_eq!(
            mpe::read_link(&mpet(4, 16, &[element_of(b"matf", 4, 16, &[])]), 4, 15),
            Err(IccError::TooLarge),
            "sixteen outputs in elements"
        );
        let with_matrix = V4Stages {
            b: identities(4),
            matrix: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
            m: identities(4),
            clut: Some((vec![2, 2, 2], vec![0; 8 * 4])),
            a: identities(3),
        };
        assert_eq!(
            read_lut(&with_matrix.tag(b"mAB ", 3, 4), false, link_of(3, 4)),
            Err(IccError::MalformedTag),
            "a matrix with four outputs"
        );
        let plain = V4Stages {
            matrix: None,
            m: Vec::new(),
            ..with_matrix
        };
        assert!(read_lut(&plain.tag(b"mAB ", 3, 4), false, link_of(3, 4)).is_ok());
        assert_eq!(
            read_lut(&plain.tag(b"mBA ", 3, 4), false, link_of(3, 4)),
            Err(IccError::NeedsLut),
            "an mBA at a link's A2B0"
        );
    }

    /// **A grey profile is one curve and a white point**, and 323 of the
    /// corpus's profiles are exactly that.
    #[test]
    fn a_grey_profile_reads_its_single_curve() {
        let profile = build(
            b"GRAY",
            b"XYZ ",
            &[(*b"kTRC", gamma(1.8)), (*b"wtpt", xyz(0.9642, 1.0, 0.8249))],
        );
        let parsed = Profile::parse(&profile).expect("a grey profile");
        let white = parsed.white.expect("the media white");
        let Some(Model::Grey { curve, .. }) = parsed.model else {
            panic!("expected a grey model");
        };
        let Curve::Gamma(g) = curve else {
            panic!("expected a gamma curve");
        };
        assert!((g - 1.8).abs() < 1.0 / 256.0, "u8Fixed8 quantises to 1/256");
        assert!((white[1] - 1.0).abs() < 1e-4);
    }

    /// **The five parametric forms are the clause's own expressions**, checked
    /// where they are supposed to agree with each other.
    ///
    /// Types 0 to 4 are one family with terms dropped: every one of them is a
    /// power law, and the later ones add a linear segment below a threshold and
    /// then an offset. So at parameters that switch the extra terms off, each
    /// form must reproduce the one before it — which is checkable without a
    /// table of expected outputs, and catches a parameter read in the wrong
    /// order, which is the mistake this shape invites.
    #[test]
    fn the_parametric_forms_agree_where_they_should() {
        let g = 2.4;
        for x in [0.0, 0.1, 0.25, 0.5, 0.75, 1.0] {
            let plain = Curve::Parametric {
                function: 0,
                params: vec![g],
            }
            .eval(x);

            // Type 1 with a = 1, b = 0 is type 0.
            let one = Curve::Parametric {
                function: 1,
                params: vec![g, 1.0, 0.0],
            }
            .eval(x);
            assert!((plain - one).abs() < 1e-12, "type 1 at {x}");

            // Type 2 adds c; with c = 0 it is type 1.
            let two = Curve::Parametric {
                function: 2,
                params: vec![g, 1.0, 0.0, 0.0],
            }
            .eval(x);
            assert!((plain - two).abs() < 1e-12, "type 2 at {x}");

            // Type 3 with d = 0 never takes its linear branch.
            let three = Curve::Parametric {
                function: 3,
                params: vec![g, 1.0, 0.0, 0.0, 0.0],
            }
            .eval(x);
            assert!((plain - three).abs() < 1e-12, "type 3 at {x}");

            // Type 4 with e = f = 0 and d = 0 is type 3.
            let four = Curve::Parametric {
                function: 4,
                params: vec![g, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            }
            .eval(x);
            assert!((plain - four).abs() < 1e-12, "type 4 at {x}");
        }
    }

    /// A parametric curve takes its linear branch below the threshold.
    ///
    /// sRGB's own transfer function is a type 3, and the point of the form is
    /// the straight segment near black: a build that ignored `d` would run the
    /// power law all the way down and crush every shadow.
    #[test]
    fn a_parametric_curve_is_linear_below_its_threshold() {
        let srgb = Curve::Parametric {
            function: 3,
            params: vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
        };
        // Below the threshold: a straight line through the origin.
        assert!((srgb.eval(0.02) - 0.02 / 12.92).abs() < 1e-9);
        assert!((srgb.eval(0.0) - 0.0).abs() < 1e-12);
        // Above it: the power law, and 1.0 maps to 1.0.
        assert!((srgb.eval(1.0) - 1.0).abs() < 1e-6, "{}", srgb.eval(1.0));
        // And it is continuous across the join, which is what the constants
        // were chosen for.
        let below = srgb.eval(0.040_449);
        let above = srgb.eval(0.040_451);
        assert!((below - above).abs() < 1e-5, "{below} against {above}");
    }

    /// A sampled curve interpolates, and its ends are its ends.
    #[test]
    fn a_sampled_curve_interpolates_between_its_points() {
        let curve = Curve::Sampled(vec![0, 32_768, 65_535]);
        assert!((curve.eval(0.0) - 0.0).abs() < 1e-9);
        assert!((curve.eval(1.0) - 1.0).abs() < 1e-9);
        assert!((curve.eval(0.5) - 0.5).abs() < 1e-4, "{}", curve.eval(0.5));
        // A quarter of the way is halfway into the first segment.
        assert!((curve.eval(0.25) - 0.25).abs() < 1e-4);
    }

    /// **An sRGB profile is the identity**, which is the exit criterion for a
    /// transform and the one check that needs no table of expected numbers.
    ///
    /// A profile whose primaries are sRGB's and whose curves are sRGB's says
    /// "these components are already sRGB". So compiling it and applying it
    /// must give back the byte that went in. Every stage is exercised — the
    /// parametric curve, the compiled table, the s15.16 matrix product with
    /// XYZ-to-sRGB, and the encode table — and any one of them wrong by more
    /// than a rounding step shows here.
    ///
    /// Within one level, which is what the design doc asks for and what the
    /// arithmetic can promise: the curve table quantises to 4 096 steps and the
    /// encode table to another 4 096, so two roundings sit between input and
    /// output.
    #[test]
    fn an_srgb_profile_transforms_to_itself() {
        let profile = Profile::parse(&srgb_profile()).expect("an sRGB profile");
        let transform =
            Transform::compile(&profile, Intent::RelativeColorimetric).expect("a matrix transform");

        let mut worst = 0i32;
        for step in 0..=32u32 {
            let value = f64::from(step) / 32.0;
            let want = (value * 255.0).round() as i32;
            let (r, g, b) = transform.apply(&[value, value, value]);
            for got in [r, g, b] {
                worst = worst.max((i32::from(got) - want).abs());
            }
            assert!(
                (i32::from(r) - want).abs() <= 1,
                "grey {value}: wanted {want}, got {r}"
            );
        }
        assert!(worst <= 1, "worst channel error {worst} levels");

        // And the primaries stay themselves rather than merely staying bright.
        let (r, g, b) = transform.apply(&[1.0, 0.0, 0.0]);
        assert!(r > 250 && g < 5 && b < 5, "red became ({r}, {g}, {b})");
        let (r, g, b) = transform.apply(&[0.0, 0.0, 1.0]);
        assert!(b > 250 && r < 5 && g < 5, "blue became ({r}, {g}, {b})");
    }

    /// **A grey profile makes greys**, and its curve decides which.
    ///
    /// The three output channels must be equal — a grey profile is achromatic
    /// by construction — and a gamma of 1.0 makes the transform the sRGB
    /// encode function alone, so mid-scale lands well above mid-grey. That is
    /// the direction a build which forgot the encode step gets wrong.
    #[test]
    fn a_grey_profile_makes_greys() {
        let profile = build(
            b"GRAY",
            b"XYZ ",
            &[(*b"kTRC", gamma(1.0)), (*b"wtpt", xyz(0.9642, 1.0, 0.8249))],
        );
        let parsed = Profile::parse(&profile).expect("a grey profile");
        let transform =
            Transform::compile(&parsed, Intent::RelativeColorimetric).expect("a grey transform");
        assert_eq!(transform.inputs(), 1);

        for value in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let (r, g, b) = transform.apply(&[value]);
            assert_eq!((r, g), (r, b), "grey {value} came out coloured");
            assert_eq!(g, b);
        }
        assert_eq!(transform.apply(&[0.0]).0, 0, "black stays black");
        assert_eq!(transform.apply(&[1.0]).0, 255, "white stays white");
        // Linear 0.5 encodes to about 0.735 of full scale, not to 128.
        let mid = transform.apply(&[0.5]).0;
        assert!((186..=190).contains(&mid), "linear half encoded to {mid}");
    }

    /// **Nothing on the pixel path evaluates a curve.**
    ///
    /// The compiled tables are what ruling 4 rests on here: `pow` is not
    /// target-stable, so it may run when a transform is built and never when
    /// one is applied. Asserted by construction — the tables are the right
    /// length and their ends are the curve's ends — because the alternative is
    /// asserting the absence of a call, which no test can see.
    #[test]
    fn a_compiled_curve_is_a_table_of_the_right_shape() {
        let curve = Curve::Parametric {
            function: 3,
            params: vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.040_45],
        };
        let table = compile_curve(&curve);
        assert_eq!(table.len(), CURVE_ENTRIES);
        assert_eq!(table[0], 0, "the curve starts at zero");
        assert_eq!(table[CURVE_ENTRIES - 1], 65_535, "and ends at one");
        // Monotone, which every transfer function is and a mis-indexed table
        // is not.
        assert!(
            table.windows(2).all(|w| w[0] <= w[1]),
            "the compiled curve is not monotone"
        );
    }

    /// **The two fixed-point encodings decode to the numbers the format
    /// defines**, bit pattern by bit pattern.
    ///
    /// `s15Fixed16Number` and `u8Fixed8Number` are the whole of ICC.1's
    /// numeric vocabulary for the tags read here, and they are the one part of
    /// this module whose right answers are *exact* — a scale or a shift wrong
    /// by a factor of two is not a rounding difference, it is every colour in
    /// the profile moved. Rounding, tolerances and the corpus have nothing to
    /// say about these; they either are the format or they are not.
    #[test]
    fn the_fixed_point_encodings_are_exactly_what_the_format_says() {
        // s15Fixed16: one is 0x0001_0000, and the sign is two's complement.
        let s15 = |bits: u32| {
            let tag = {
                let mut out = b"XYZ ".to_vec();
                out.extend_from_slice(&[0; 4]);
                out.extend_from_slice(&bits.to_be_bytes());
                out.extend_from_slice(&[0; 8]);
                out
            };
            read_xyz(&tag).expect("an XYZ tag")[0]
        };
        assert_eq!(s15(0x0001_0000), 1.0, "one");
        assert_eq!(s15(0x0000_8000), 0.5, "a half");
        assert_eq!(s15(0x0000_0001), 1.0 / 65536.0, "the smallest step");
        assert_eq!(s15(0x0000_0000), 0.0, "zero");
        assert_eq!(s15(0xFFFF_0000), -1.0, "minus one is two's complement");
        assert_eq!(s15(0x8000_0000), -32768.0, "the most negative");
        assert_eq!(
            s15(0x7FFF_FFFF),
            32768.0 - 1.0 / 65536.0,
            "the most positive"
        );

        // u8Fixed8, which is what a single-entry `curv` stores a gamma in.
        let u8f8 = |bits: u16| {
            let mut out = b"curv".to_vec();
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&1u32.to_be_bytes());
            out.extend_from_slice(&bits.to_be_bytes());
            match read_curve(&out).expect("a curve") {
                Curve::Gamma(g) => g,
                other => panic!("expected a gamma, got {other:?}"),
            }
        };
        assert_eq!(u8f8(0x0100), 1.0, "one");
        assert_eq!(u8f8(0x0233), 563.0 / 256.0, "the usual 2.2");
        assert_eq!(u8f8(0x0080), 0.5, "a half");
        assert_eq!(u8f8(0xFFFF), 65535.0 / 256.0, "the largest");
    }

    /// **A linear curve compiles to a linear ramp**, every one of its 4 096
    /// entries.
    ///
    /// The exact statement of what compiling *is*: a gamma of one is the
    /// identity, so entry `i` must be `i` rescaled from the table's range to
    /// sixteen bits and nothing else. Asserted for every entry rather than at
    /// the ends, because a table that is right at both ends and wrong in the
    /// middle is exactly what an off-by-one in the index arithmetic produces —
    /// and the sRGB round trip would absorb it, since it samples the same
    /// wrong table on the way in and out.
    #[test]
    fn a_linear_curve_compiles_to_a_linear_ramp() {
        let table = compile_curve(&Curve::Gamma(1.0));
        assert_eq!(table.len(), CURVE_ENTRIES);
        for (i, entry) in table.iter().enumerate() {
            let want = ((i as f64 / (CURVE_ENTRIES - 1) as f64) * 65535.0).round() as u16;
            assert_eq!(
                *entry, want,
                "entry {i} of {CURVE_ENTRIES} is {entry}, not {want}"
            );
        }

        // And `Identity` is the same table, since it is the same function.
        assert_eq!(compile_curve(&Curve::Identity), table);
    }

    /// **A profile's columns reach the matrix**, which is the step between
    /// parsing and transforming and the one the sRGB round trip cannot see.
    ///
    /// Halving the green column must halve green's contribution. The round trip
    /// would notice too, but only as "not the identity any more"; this says
    /// *which* number moved, which is what a failure needs to be actionable.
    #[test]
    fn the_columns_are_what_the_matrix_is_built_from() {
        let full = Profile::parse(&srgb_profile()).expect("a profile");
        let bright = Transform::compile(&full, Intent::RelativeColorimetric).expect("a transform");

        let dimmed_bytes = build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(SRGB_R[0], SRGB_R[1], SRGB_R[2])),
                (
                    *b"gXYZ",
                    xyz(SRGB_G[0] / 2.0, SRGB_G[1] / 2.0, SRGB_G[2] / 2.0),
                ),
                (*b"bXYZ", xyz(SRGB_B[0], SRGB_B[1], SRGB_B[2])),
                (*b"rTRC", srgb_curve()),
                (*b"gTRC", srgb_curve()),
                (*b"bTRC", srgb_curve()),
                (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
            ],
        );
        let dimmed = Transform::compile(
            &Profile::parse(&dimmed_bytes).expect("a profile"),
            Intent::RelativeColorimetric,
        )
        .expect("a transform");

        let (_, bright_g, _) = bright.apply(&[0.0, 1.0, 0.0]);
        let (_, dim_g, _) = dimmed.apply(&[0.0, 1.0, 0.0]);
        assert!(
            dim_g < bright_g,
            "halving the green column did not dim green: {bright_g} then {dim_g}"
        );
        // Half the light, encoded: sRGB puts linear 0.5 near 188 of 255.
        assert!(
            (186..=190).contains(&dim_g),
            "half of green's light should encode near 188, got {dim_g}"
        );
    }

    /// An `mft2` tag with the grid given verbatim.
    ///
    /// `clut` is `grid.pow(inputs)` entries of three values each, and the order
    /// is the one the format states: the **last** input axis varies fastest.
    fn mft2(inputs: usize, grid: usize, clut: &[[u16; 3]]) -> Vec<u8> {
        let mut out = b"mft2".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.push(inputs as u8);
        out.push(3);
        out.push(grid as u8);
        out.push(0);
        // The 3x3 matrix, identity, which an A2B tag never applies.
        for row in 0..3 {
            for column in 0..3 {
                let value: i32 = if row == column { 65536 } else { 0 };
                out.extend_from_slice(&(value as u32).to_be_bytes());
            }
        }
        // Two entries each side is the fewest the format allows, and makes the
        // input and output stages the identity.
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&2u16.to_be_bytes());
        for _ in 0..inputs {
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&65535u16.to_be_bytes());
        }
        for entry in clut {
            for value in entry {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        for _ in 0..3 {
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&65535u16.to_be_bytes());
        }
        out
    }

    fn lut_profile(space: &[u8; 4], pcs: &[u8; 4], tag: Vec<u8>) -> Vec<u8> {
        build(space, pcs, &[(*b"A2B0", tag)])
    }

    /// The table relative colorimetric converts `profile` through.
    fn relative_table(profile: &Profile) -> Arc<Lut> {
        match profile.source(Intent::RelativeColorimetric) {
            Some(Source::Table { lut, .. }) => lut,
            other => panic!("expected a lookup table, got {other:?}"),
        }
    }

    /// **The grid interpolates linearly between its points**, which is the
    /// closed form the whole table rests on.
    ///
    /// One input, two grid points, black at one end and white at the other: the
    /// value at a half must be a half. Checked in the connection space rather
    /// than through a colour conversion, so a failure means the interpolation
    /// and not the encoding.
    #[test]
    fn a_lookup_table_interpolates_between_its_grid_points() {
        let tag = mft2(1, 2, &[[0, 0, 0], [65535, 65535, 65535]]);
        let profile = Profile::parse(&lut_profile(b"GRAY", b"XYZ ", tag)).expect("a LUT profile");
        let lut = relative_table(&profile);
        for (input, want) in [(0.0, 0.0), (0.25, 0.25), (0.5, 0.5), (1.0, 1.0)] {
            let got = lut.evaluate(&[input])[0];
            assert!(
                (got - want).abs() < 0.002,
                "at {input} the grid gave {got}, not {want}"
            );
        }
    }

    /// **The last input axis varies fastest**, which is the one thing about a
    /// multi-dimensional grid that cannot be inferred from a picture.
    ///
    /// Two axes, two points each, and a grid whose four entries are all
    /// different. A build that strided the other way round reads `(0, 1)` where
    /// `(1, 0)` is, which for a printer profile swaps cyan with magenta — a
    /// plausible picture in the wrong colours, and exactly what the corpus
    /// check above would catch only if the profile happened to be asymmetric.
    /// This says it directly.
    #[test]
    fn the_last_grid_axis_varies_fastest() {
        // (a, b) -> red channel: (0,0)=0, (0,1)=1, (1,0)=2, (1,1)=3, scaled.
        let q = |n: u16| n * 21845; // 0, 21845, 43690, 65535
        let tag = mft2(
            2,
            2,
            &[[q(0), 0, 0], [q(1), 0, 0], [q(2), 0, 0], [q(3), 0, 0]],
        );
        let profile = Profile::parse(&lut_profile(b"GRAY", b"XYZ ", tag)).expect("a LUT profile");
        let lut = relative_table(&profile);
        let at = |a: f64, b: f64| lut.evaluate(&[a, b])[0];
        let near = |got: f64, want: f64| (got - want).abs() < 0.01;

        assert!(near(at(0.0, 0.0), 0.0), "(0,0) gave {}", at(0.0, 0.0));
        assert!(
            near(at(0.0, 1.0), 1.0 / 3.0),
            "(0,1) gave {} — the axes are strided the wrong way round",
            at(0.0, 1.0)
        );
        assert!(
            near(at(1.0, 0.0), 2.0 / 3.0),
            "(1,0) gave {} — the axes are strided the wrong way round",
            at(1.0, 0.0)
        );
        assert!(near(at(1.0, 1.0), 1.0), "(1,1) gave {}", at(1.0, 1.0));
    }

    /// A four-channel table is read at four channels, which is 408 of the
    /// corpus's 415 v2 tables.
    #[test]
    fn a_four_channel_table_takes_four_channels() {
        let grid = 2usize;
        let entries = grid.pow(4);
        // Every entry the same, so the answer is that value wherever it is
        // asked — which is what a mis-sized CLUT cannot produce, because it
        // would run off the end and read zeros.
        let clut: Vec<[u16; 3]> = (0..entries).map(|_| [32768, 16384, 8192]).collect();
        let tag = mft2(4, grid, &clut);
        let profile = Profile::parse(&lut_profile(b"CMYK", b"XYZ ", tag)).expect("a LUT profile");
        let transform =
            Transform::compile(&profile, Intent::RelativeColorimetric).expect("a transform");
        assert_eq!(transform.inputs(), 4);

        let lut = relative_table(&profile);
        for corner in [
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0, 1.0],
            [0.3, 0.6, 0.1, 0.9],
        ] {
            let pcs = lut.evaluate(&corner);
            assert!((pcs[0] - 0.5).abs() < 0.01, "{corner:?} gave {pcs:?}");
            assert!((pcs[1] - 0.25).abs() < 0.01, "{corner:?} gave {pcs:?}");
        }
    }

    /// v4's `mAB ` reads, a different structure from `mft2` rather than a
    /// variation of it, and its inverse, `mBA `, which an `A2B*` tag never
    /// holds, is refused by name.
    #[test]
    fn a_v4_lookup_table_reads_and_its_inverse_is_refused_by_name() {
        // An `mAB ` with every offset zero declares no B curves, and B is the
        // one stage ICC.1 makes required -- a tag without it describes no
        // transform at all.
        let mut empty = b"mAB ".to_vec();
        empty.extend_from_slice(&[0; 28]);
        assert_eq!(
            Profile::parse(&lut_profile(b"CMYK", b"Lab ", empty)),
            Err(IccError::MalformedTag)
        );

        // `mBA ` is the *inverse* direction, and an `A2B*` tag never holds
        // one: it would be a profile saying how to get out of the connection
        // space at the entry that asks how to get in.
        let mut inverse = b"mBA ".to_vec();
        inverse.extend_from_slice(&[0; 28]);
        assert_eq!(
            Profile::parse(&lut_profile(b"CMYK", b"Lab ", inverse)),
            Err(IccError::NeedsLut)
        );

        // And a tag that carries only its B curves is a transform: v4 writes a
        // plain three-curve conversion this way rather than as matrix/TRC.
        let profile = Profile::parse(&lut_profile(b"RGB ", b"XYZ ", mab_b_curves_only()))
            .expect("B curves alone are a transform");
        let transform =
            Transform::compile(&profile, Intent::RelativeColorimetric).expect("and it compiles");
        // The curves below are the identity, so the connection space value is
        // the input and white stays white.
        let white = transform.apply(&[1.0, 1.0, 1.0]);
        assert!(
            white.0 > 200 && white.1 > 200 && white.2 > 200,
            "an identity A-to-B must not darken white: {white:?}"
        );
    }

    /// An `mAB ` whose only stage is three identity B curves.
    fn mab_b_curves_only() -> Vec<u8> {
        // A `curv` with a count of zero is the identity (ICC.1), and it is
        // twelve bytes, which is already four-byte aligned.
        let identity = {
            let mut c = b"curv".to_vec();
            c.extend_from_slice(&[0; 4]);
            c.extend_from_slice(&0u32.to_be_bytes());
            c
        };
        let mut tag = b"mAB ".to_vec();
        tag.extend_from_slice(&[0; 4]);
        tag.push(3); // input channels
        tag.push(3); // output channels
        tag.extend_from_slice(&[0; 2]);
        // The five offsets, in the header's own order: B, matrix, M, CLUT, A.
        tag.extend_from_slice(&32u32.to_be_bytes());
        tag.extend_from_slice(&0u32.to_be_bytes());
        tag.extend_from_slice(&0u32.to_be_bytes());
        tag.extend_from_slice(&0u32.to_be_bytes());
        tag.extend_from_slice(&0u32.to_be_bytes());
        for _ in 0..3 {
            tag.extend_from_slice(&identity);
        }
        tag
    }

    /// A grid whose declared size cannot fit is refused before it is
    /// allocated (ruling 1).
    ///
    /// `grid.pow(inputs)` is the branch this format hides in two bytes: 255
    /// points over 15 channels is an entry count no machine holds, and both
    /// numbers are one byte of file.
    #[test]
    fn an_impossible_grid_is_refused_before_it_is_allocated() {
        let mut tag = b"mft2".to_vec();
        tag.extend_from_slice(&[0; 4]);
        tag.push(15); // inputs
        tag.push(3); // outputs
        tag.push(255); // grid points per axis
        tag.push(0);
        tag.extend_from_slice(&[0; 36 + 4]);
        let parsed = Profile::parse(&lut_profile(b"CMYK", b"Lab ", tag));
        assert!(
            matches!(parsed, Err(IccError::TooLarge)),
            "expected TooLarge, got {parsed:?}"
        );
    }

    // ---- rendering intents ---------------------------------------------------

    /// Table `table`'s value at grid node `node`, channel `channel`, in
    /// [`three_intents_profile`]: a multiple of eight, so the average of any
    /// two, four or eight nodes is a whole number and a grid read halfway
    /// between nodes has one right answer.
    fn intent_node(table: usize, node: usize, channel: usize) -> u16 {
        (8 * ((node * 97 + channel * 31 + table * 1201) % 8192)) as u16
    }

    /// An RGB profile carrying three `mft2` tables over three-point grids at
    /// `A2B0`, `A2B1` and `A2B2`, each a different grid: the profile in which
    /// every intent has its own answer.
    pub(crate) fn three_intents_profile() -> Vec<u8> {
        let table = |t: usize| {
            let clut: Vec<[u16; 3]> = (0..27)
                .map(|node| [0, 1, 2].map(|channel| intent_node(t, node, channel)))
                .collect();
            mft2(3, 3, &clut)
        };
        build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"A2B0", table(0)),
                (*b"A2B1", table(1)),
                (*b"A2B2", table(2)),
            ],
        )
    }

    /// The connection-space value, as 16-bit integers, that `table`'s grid
    /// gives `x`, recomputed by multilinear interpolation over its three
    /// three-point axes; `x`'s components are quarters, so every position is
    /// a node or halfway between two.
    fn multilinear(table: usize, x: [f64; 3]) -> [u32; 3] {
        let positions = x.map(|v| v * 2.0);
        let mut sum = [0.0f64; 3];
        for corner in 0..8usize {
            let mut weight = 1.0;
            let mut node = 0;
            for (axis, &position) in positions.iter().enumerate() {
                let low = position.floor().min(1.0);
                let fraction = position - low;
                let high = corner & (1 << (2 - axis)) != 0;
                weight *= if high { fraction } else { 1.0 - fraction };
                node = node * 3 + low as usize + usize::from(high);
            }
            for (channel, slot) in sum.iter_mut().enumerate() {
                *slot += weight * f64::from(intent_node(table, node, channel));
            }
        }
        sum.map(|v| v.round() as u32)
    }

    /// The connection-space value `transform` gives `x`, as 16-bit integers.
    fn pcs_of(transform: &Transform, x: [f64; 3]) -> [u32; 3] {
        pcs_at(transform, &x)
    }

    /// [`pcs_of`], for any number of components.
    fn pcs_at(transform: &Transform, x: &[f64]) -> [u32; 3] {
        let lut = transform.lut.as_ref().expect("a lookup-table transform");
        let values = lut.evaluate(x);
        [values[0], values[1], values[2]].map(|v| (v * 65535.0).round() as u32)
    }

    /// Every component a quarter: nodes and the points halfway between them.
    fn quarters() -> Vec<[f64; 3]> {
        let steps = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut out = Vec::new();
        for r in steps {
            for g in steps {
                for b in steps {
                    out.push([r, g, b]);
                }
            }
        }
        out
    }

    /// **Each intent compiles its own table.** A profile carrying three
    /// different `mft2` grids at `A2B0`, `A2B1` and `A2B2` answers
    /// perceptual through the first, relative and absolute colorimetric
    /// through the second and saturation through the third: at every input of
    /// quarters, each intent's transform gives the connection-space value the
    /// test recomputes from that table's grid by multilinear interpolation,
    /// and the set of transforms a colour space carries converts in each
    /// intent as that intent's transform does. The three tables part at some
    /// input, so no intent can pass by reading another's.
    #[test]
    fn each_intent_compiles_its_own_table() {
        let profile = Profile::parse(&three_intents_profile()).expect("three tables");
        let transforms =
            Transforms::compile(profile.clone()).expect("relative colorimetric's transform");
        for (intent, table) in [
            (Intent::Perceptual, 0),
            (Intent::RelativeColorimetric, 1),
            (Intent::Saturation, 2),
            (Intent::AbsoluteColorimetric, 1),
        ] {
            let transform = Transform::compile(&profile, intent).expect("a transform");
            for x in quarters() {
                assert_eq!(
                    pcs_of(&transform, x),
                    multilinear(table, x),
                    "{intent:?} at {x:?}"
                );
                assert_eq!(
                    transforms.get(intent).apply(&x),
                    transform.apply(&x),
                    "{intent:?} at {x:?}: the space's transform"
                );
            }
        }
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            assert!(
                quarters()
                    .into_iter()
                    .any(|x| multilinear(a, x) != multilinear(b, x)),
                "tables {a} and {b} are one grid"
            );
        }
    }

    /// A profile carrying, of `A2B0`, `A2B1` and `A2B2`, the ones `present`
    /// marks, each a table whose every node is `fallback_value(t)`; and, where
    /// `broken` names one, that tag's data a table that does not read.
    fn tables_present(present: [bool; 3], broken: Option<usize>) -> Vec<u8> {
        let mut broken_mask = [false; 3];
        if let Some(t) = broken {
            broken_mask[t] = true;
        }
        model_and_tables(b"RGB ", b"XYZ ", &[], 3, present, broken_mask)
    }

    /// A profile in `space` and `pcs` carrying the tags `model` lists and, of
    /// `A2B0`, `A2B1` and `A2B2`, the ones `present` marks: each a table over
    /// `inputs` channels whose every node is `fallback_value(t)`, or, where
    /// `broken` marks it, a table that does not read.
    fn model_and_tables(
        space: &[u8; 4],
        pcs: &[u8; 4],
        model: &[([u8; 4], Vec<u8>)],
        inputs: usize,
        present: [bool; 3],
        broken: [bool; 3],
    ) -> Vec<u8> {
        let mut tags = model.to_vec();
        for (t, &here) in present.iter().enumerate() {
            if !here {
                continue;
            }
            let signature = [*b"A2B0", *b"A2B1", *b"A2B2"][t];
            let data = if broken[t] {
                inverse_table()
            } else {
                constant_table(inputs, fallback_value(t))
            };
            tags.push((signature, data));
        }
        build(space, pcs, &tags)
    }

    /// A tag carrying the inverse direction's signature, which an `A2B*` tag
    /// never holds: a table that does not read.
    fn inverse_table() -> Vec<u8> {
        let mut inverse = b"mBA ".to_vec();
        inverse.extend_from_slice(&[0; 28]);
        inverse
    }

    /// An `mft2` over `inputs` channels on a two-point grid whose every node
    /// is `value` in each of its three outputs.
    fn constant_table(inputs: usize, value: u16) -> Vec<u8> {
        mft2(inputs, 2, &vec![[value; 3]; 1 << inputs])
    }

    /// The value every node of [`tables_present`]'s table `t` holds.
    fn fallback_value(t: usize) -> u16 {
        8_000 * (t as u16 + 1)
    }

    /// The tag a transform converts through, told by its constant value.
    fn table_of(transform: &Transform) -> usize {
        let value = pcs_of(transform, [0.3, 0.6, 0.9])[0];
        (0..3)
            .find(|&t| u32::from(fallback_value(t)) == value)
            .unwrap_or_else(|| panic!("{value} is no table's"))
    }

    /// **An absent intent table falls back as ICC.1 says.** For every set of
    /// `A2B*` tags a profile may carry, each intent converts through the
    /// first tag of its row of `docs/design/icc.md`'s table the profile
    /// carries: perceptual `A2B0`, `A2B1`, `A2B2`; relative and absolute
    /// colorimetric `A2B1`, `A2B0`, `A2B2`; saturation `A2B2`, `A2B0`,
    /// `A2B1` — 8.10.2's b) and c), then the parser's order. A table that does
    /// not read is passed over as 8.10.2 passes over an unsupported tag,
    /// unless it is relative colorimetric's, which refuses the profile as it
    /// always did. A matrix/TRC and a grey profile convert alike in every
    /// intent, perceptual and saturation through relative colorimetric's own
    /// transform.
    #[test]
    fn an_absent_intent_table_falls_back_as_icc_1_says() {
        let rows = [
            (Intent::Perceptual, [0, 1, 2]),
            (Intent::RelativeColorimetric, [1, 0, 2]),
            (Intent::Saturation, [2, 0, 1]),
            (Intent::AbsoluteColorimetric, [1, 0, 2]),
        ];
        for set in 1..8usize {
            let present = [set & 1 != 0, set & 2 != 0, set & 4 != 0];
            let profile = Profile::parse(&tables_present(present, None))
                .unwrap_or_else(|e| panic!("{present:?}: {e:?}"));
            let transforms = Transforms::compile(profile.clone()).expect("a transform");
            for (intent, order) in rows {
                let expected = order
                    .into_iter()
                    .find(|&t| present[t])
                    .expect("a tag is present");
                let transform = Transform::compile(&profile, intent).expect("a transform");
                assert_eq!(table_of(&transform), expected, "{present:?}, {intent:?}");
                assert_eq!(
                    table_of(transforms.get(intent)),
                    expected,
                    "{present:?}, {intent:?}: the space's"
                );
            }
        }

        // A broken `A2B0` is passed over by perceptual, whose next is `A2B1`.
        let profile = Profile::parse(&tables_present([true, true, true], Some(0)))
            .expect("relative colorimetric's table reads");
        for (intent, expected) in [
            (Intent::Perceptual, 1),
            (Intent::RelativeColorimetric, 1),
            (Intent::Saturation, 2),
        ] {
            let transform = Transform::compile(&profile, intent).expect("a transform");
            assert_eq!(table_of(&transform), expected, "{intent:?}");
        }
        // A broken `A2B2` is passed over by saturation, whose next is `A2B0`.
        let profile = Profile::parse(&tables_present([true, true, true], Some(2)))
            .expect("relative colorimetric's table reads");
        let transform = Transform::compile(&profile, Intent::Saturation).expect("a transform");
        assert_eq!(table_of(&transform), 0);
        // Relative colorimetric's table broken refuses the profile, with
        // another table beside it or without.
        for present in [[true, true, true], [false, true, false]] {
            assert_eq!(
                Profile::parse(&tables_present(present, Some(1))),
                Err(IccError::NeedsLut),
                "{present:?}"
            );
        }
        // And with no `A2B1`, `A2B0` is relative colorimetric's.
        assert_eq!(
            Profile::parse(&tables_present([true, false, true], Some(0))),
            Err(IccError::NeedsLut)
        );

        // A matrix/TRC and a grey profile: one transform for every intent but
        // absolute colorimetric, which compiles alike.
        for bytes in [matrix_profile(), grey(b"XYZ ", 2.2)] {
            let profile = Profile::parse(&bytes).expect("a profile");
            let relative =
                Transform::compile(&profile, Intent::RelativeColorimetric).expect("a transform");
            for intent in Intent::ALL {
                assert_eq!(
                    Transform::compile(&profile, intent).as_ref(),
                    Some(&relative),
                    "{intent:?}"
                );
            }
            let transforms = Transforms::compile(profile).expect("a transform");
            for intent in [Intent::Perceptual, Intent::Saturation] {
                assert!(
                    std::ptr::eq(
                        transforms.get(intent),
                        transforms.get(Intent::RelativeColorimetric)
                    ),
                    "{intent:?} compiled a transform of its own"
                );
            }
            assert_eq!(
                transforms.get(Intent::AbsoluteColorimetric),
                &relative,
                "absolute colorimetric"
            );
        }
    }

    /// A profile whose tag table names `entries` — each a signature and the
    /// index of the element in `elements` it points at — so two tags can name
    /// one element, at one offset and with one size.
    fn build_sharing(
        space: &[u8; 4],
        pcs: &[u8; 4],
        entries: &[([u8; 4], usize)],
        elements: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut header = vec![0u8; 128];
        header[12..16].copy_from_slice(b"mntr");
        header[16..20].copy_from_slice(space);
        header[20..24].copy_from_slice(pcs);
        header[36..40].copy_from_slice(b"acsp");
        header[8..12].copy_from_slice(&0x0200_0000u32.to_be_bytes());
        let start = 132 + entries.len() * 12;
        let mut offsets = Vec::new();
        let mut body = Vec::new();
        for element in elements {
            offsets.push(start + body.len());
            body.extend_from_slice(element);
        }
        let mut out = header;
        out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for (signature, element) in entries {
            out.extend_from_slice(signature);
            out.extend_from_slice(&(offsets[*element] as u32).to_be_bytes());
            out.extend_from_slice(&(elements[*element].len() as u32).to_be_bytes());
        }
        out.extend_from_slice(&body);
        let size = out.len() as u32;
        out[0..4].copy_from_slice(&size.to_be_bytes());
        out
    }

    /// The tables read since the count was last zeroed, zeroing it.
    fn tables_read() -> u64 {
        TABLE_READS.with(|reads| reads.replace(0))
    }

    /// **Tags sharing an offset are read once.** `A2B0` and `A2B1` naming one
    /// element and `A2B2` another make two tables: parsing reads relative
    /// colorimetric's, and no other; perceptual then converts through the
    /// table already read, saturation reads the second, and nothing is read
    /// again however often an intent asks. A profile carrying three distinct
    /// tables parses one of them, so a page that names no intent reads what it
    /// always read.
    #[test]
    fn tags_sharing_an_offset_are_read_once() {
        let grid = |value: u16| mft2(3, 2, &[[value; 3]; 8]);
        let bytes = build_sharing(
            b"RGB ",
            b"XYZ ",
            &[(*b"A2B0", 0), (*b"A2B1", 0), (*b"A2B2", 1)],
            &[grid(9_000), grid(27_000)],
        );
        tables_read();
        let profile = Profile::parse(&bytes).expect("two tables");
        assert_eq!(
            tables_read(),
            1,
            "parsing reads relative colorimetric's table"
        );
        let tables = &profile.tables;
        assert_eq!(tables.tables.len(), 2, "two elements, two tables");
        assert_eq!(tables.slots, [Some(0), Some(0), Some(1), None]);

        let transforms = Transforms::compile(profile).expect("a transform");
        assert_eq!(
            tables_read(),
            0,
            "compiling relative colorimetric reads nothing"
        );
        let perceptual = pcs_of(transforms.get(Intent::Perceptual), [0.5; 3]);
        let relative = pcs_of(transforms.get(Intent::RelativeColorimetric), [0.5; 3]);
        assert_eq!(perceptual, relative, "one element, one table");
        assert_eq!(perceptual, [9_000; 3]);
        assert_eq!(
            tables_read(),
            0,
            "perceptual's table is relative's, read already"
        );
        assert_eq!(
            pcs_of(transforms.get(Intent::Saturation), [0.5; 3]),
            [27_000; 3]
        );
        assert_eq!(tables_read(), 1, "saturation reads its own");
        for _ in 0..3 {
            for intent in Intent::ALL {
                let _ = transforms.get(intent).apply(&[0.2, 0.4, 0.6]);
            }
        }
        assert_eq!(tables_read(), 0, "nothing is read twice");

        // Three distinct tables: parsing reads one.
        let _ = Profile::parse(&three_intents_profile()).expect("three tables");
        assert_eq!(tables_read(), 1, "a profile parses one table");
    }

    // ---- a model beside tables, in ICC.1 8.10.2's order ---------------------

    /// The tag ICC.1 8.10.2 b) designates for each intent, by number:
    /// perceptual `A2B0`, the two colorimetric intents `A2B1` (Table 25,
    /// 9.1), saturation `A2B2`.
    const DESIGNATED: [(Intent, usize); 4] = [
        (Intent::Perceptual, 0),
        (Intent::RelativeColorimetric, 1),
        (Intent::Saturation, 2),
        (Intent::AbsoluteColorimetric, 1),
    ];

    /// The matrix/TRC tags of [`matrix_profile`], its white point left out:
    /// a model to carry beside tables.
    fn matrix_tags() -> Vec<([u8; 4], Vec<u8>)> {
        vec![
            (*b"rXYZ", xyz(0.4360, 0.2225, 0.0139)),
            (*b"gXYZ", xyz(0.3851, 0.7169, 0.0971)),
            (*b"bXYZ", xyz(0.1431, 0.0606, 0.7141)),
            (*b"rTRC", gamma(2.2)),
            (*b"gTRC", gamma(2.2)),
            (*b"bTRC", gamma(2.2)),
        ]
    }

    /// A grey model's tags: a `kTRC` of gamma 1.8 and the white point the
    /// grey branch reads.
    fn grey_tags() -> Vec<([u8; 4], Vec<u8>)> {
        vec![(*b"kTRC", gamma(1.8)), (*b"wtpt", xyz(0.9642, 1.0, 0.8249))]
    }

    /// Asserts that `transform` converts through the table `expected` names,
    /// or through `model` where it names none.
    fn converts_through(
        transform: &Transform,
        expected: Option<usize>,
        model: &Transform,
        what: &str,
    ) {
        match expected {
            Some(t) => assert_eq!(table_of(transform), t, "{what}: a table"),
            None => assert_eq!(transform, model, "{what}: the model"),
        }
    }

    /// **A profile carrying a model and tables converts through its tables
    /// first**, as ICC.1 8.10.2 orders them. For a matrix/TRC profile, and a
    /// grey one under XYZ and under Lab, carrying each set of `A2B0`, `A2B1`
    /// and `A2B2`, every intent converts through b) the tag designated for it,
    /// else c) `A2B0`, else d) the model — never the profile's other tables,
    /// which come after the model — on its own and through the set a colour
    /// space carries; and the model's transform is the one the profile
    /// without its tables compiles.
    #[test]
    fn a_profile_carrying_a_model_and_tables_reads_its_tables_first() {
        for (space, pcs, inputs, model) in [
            (b"RGB ", b"XYZ ", 3, matrix_tags()),
            (b"GRAY", b"XYZ ", 1, grey_tags()),
            (b"GRAY", b"Lab ", 1, grey_tags()),
        ] {
            let alone = Profile::parse(&build(space, pcs, &model)).expect("the model alone");
            let model_transform =
                Transform::compile(&alone, Intent::RelativeColorimetric).expect("a transform");
            assert!(model_transform.lut.is_none(), "{space:?}: a closed form");
            for set in 0..8usize {
                let present = [set & 1 != 0, set & 2 != 0, set & 4 != 0];
                let bytes = model_and_tables(space, pcs, &model, inputs, present, [false; 3]);
                let profile = Profile::parse(&bytes)
                    .unwrap_or_else(|error| panic!("{space:?} {present:?}: {error:?}"));
                assert_eq!(
                    profile.model, alone.model,
                    "{space:?} {present:?}: the model"
                );
                let transforms = Transforms::compile(profile.clone()).expect("a transform");
                for (intent, designated) in DESIGNATED {
                    let expected = if present[designated] {
                        Some(designated)
                    } else if present[0] {
                        Some(0)
                    } else {
                        None
                    };
                    let what = format!("{space:?} {pcs:?} {present:?} {intent:?}");
                    let transform = Transform::compile(&profile, intent).expect("a transform");
                    converts_through(&transform, expected, &model_transform, &what);
                    converts_through(
                        transforms.get(intent),
                        expected,
                        &model_transform,
                        &format!("{what}, the space's"),
                    );
                }
            }
        }
    }

    /// **A table that does not read is passed over to the next source**
    /// where the profile carries a model that reads, for every intent,
    /// relative colorimetric's among them: such a profile is never refused
    /// for its tables, and each intent converts through the first of b), c)
    /// and d) that reads. With every table it carries broken, every intent
    /// converts through the model.
    #[test]
    fn a_table_that_does_not_read_is_passed_over_to_the_model() {
        let alone = Profile::parse(&build(b"RGB ", b"XYZ ", &matrix_tags())).expect("a model");
        let model_transform =
            Transform::compile(&alone, Intent::RelativeColorimetric).expect("a transform");
        for set in 1..8usize {
            let present = [set & 1 != 0, set & 2 != 0, set & 4 != 0];
            for breaks in 1..8usize {
                let broken = [breaks & 1 != 0, breaks & 2 != 0, breaks & 4 != 0];
                if (0..3).any(|t| broken[t] && !present[t]) {
                    continue;
                }
                let bytes = model_and_tables(b"RGB ", b"XYZ ", &matrix_tags(), 3, present, broken);
                let profile = Profile::parse(&bytes).unwrap_or_else(|error| {
                    panic!("{present:?}, {broken:?} broken: refused for its tables, {error:?}")
                });
                let reads = |t: usize| present[t] && !broken[t];
                let transforms = Transforms::compile(profile.clone()).expect("a transform");
                for (intent, designated) in DESIGNATED {
                    let expected = if reads(designated) {
                        Some(designated)
                    } else if reads(0) {
                        Some(0)
                    } else {
                        None
                    };
                    let what = format!("{present:?}, {broken:?} broken, {intent:?}");
                    let transform = Transform::compile(&profile, intent).expect("a transform");
                    converts_through(&transform, expected, &model_transform, &what);
                    converts_through(
                        transforms.get(intent),
                        expected,
                        &model_transform,
                        &format!("{what}, the space's"),
                    );
                }
            }
        }
    }

    /// The value node `node`, channel `channel`, of [`two_point_table`]
    /// `seed` holds: a multiple of sixteen, so the mean of any of its corners
    /// up to sixteen is a whole number and a point halfway between nodes has
    /// one right answer.
    fn grid_node(seed: usize, node: usize, channel: usize) -> u16 {
        (16 * ((node * 37 + channel * 11 + seed * 503) % 4096)) as u16
    }

    /// An `mft2` over `inputs` channels on a two-point grid, every node its
    /// own value ([`grid_node`]).
    fn two_point_table(inputs: usize, seed: usize) -> Vec<u8> {
        let clut: Vec<[u16; 3]> = (0..1usize << inputs)
            .map(|node| [0, 1, 2].map(|channel| grid_node(seed, node, channel)))
            .collect();
        mft2(inputs, 2, &clut)
    }

    /// The connection-space value [`two_point_table`] `seed` gives `x`, each
    /// component nought, a half or one, recomputed: the mean of the corners
    /// of the cell's face `x` stands on, the last axis varying fastest.
    fn two_point_value(inputs: usize, seed: usize, x: &[f64]) -> [u32; 3] {
        let mut sum = [0u32; 3];
        let mut count = 0u32;
        for node in 0..1usize << inputs {
            let on = (0..inputs).all(|axis| {
                let high = (node >> (inputs - 1 - axis)) & 1 == 1;
                if x[axis] == 0.0 {
                    !high
                } else if x[axis] == 1.0 {
                    high
                } else {
                    true
                }
            });
            if on {
                count += 1;
                for (channel, slot) in sum.iter_mut().enumerate() {
                    *slot += u32::from(grid_node(seed, node, channel));
                }
            }
        }
        sum.map(|total| total / count)
    }

    /// Every input of `inputs` components each nought, a half or one.
    fn halves(inputs: usize) -> Vec<Vec<f64>> {
        (0..3usize.pow(inputs as u32))
            .map(|index| {
                let mut x = vec![0.0; inputs];
                let mut rest = index;
                for slot in x.iter_mut().rev() {
                    *slot = [0.0, 0.5, 1.0][rest % 3];
                    rest /= 3;
                }
                x
            })
            .collect()
    }

    /// **A profile whose model contradicts its header reads through its
    /// table.** The three shapes the corpus census names — a monitor profile
    /// whose data space is `LAB` carrying the matrix columns a matrix applies
    /// to linear RGB, one naming the three-channel space `YYY` no registry
    /// defines, and a printer profile carrying one `kTRC` for its four inks
    /// into Lab — each with an `A2B0` beside the model, compile, and every
    /// intent converts as the table says, its value recomputed from the grid
    /// at every input of noughts, halves and ones: the model is 8.10.2 d),
    /// after the table, and one that does not read is passed over. Without
    /// the table each is refused for its model, `UnsupportedSpace`, as
    /// `every_refusal_is_reachable_and_named`'s profiles are; with an `A2B2`
    /// alone, relative colorimetric reaches it past the model; and with a
    /// table that does not read either, the model's refusal is the profile's.
    #[test]
    fn a_profile_whose_matrix_contradicts_its_space_reads_through_its_table() {
        let columns = || {
            vec![
                (*b"rXYZ", xyz(0.4360, 0.2225, 0.0139)),
                (*b"gXYZ", xyz(0.3851, 0.7169, 0.0971)),
                (*b"bXYZ", xyz(0.1431, 0.0606, 0.7141)),
            ]
        };
        let shapes: [(&[u8; 4], &[u8; 4], &[u8; 4], Vec<([u8; 4], Vec<u8>)>, usize); 3] = [
            (b"mntr", b"LAB ", b"XYZ ", columns(), 3),
            (b"mntr", b"YYY ", b"XYZ ", columns(), 3),
            (b"prtr", b"CMYK", b"Lab ", vec![(*b"kTRC", gamma(1.0))], 4),
        ];
        for (class, space, pcs, model, inputs) in shapes {
            let what = format!("{class:?} {space:?} -> {pcs:?}");
            assert_eq!(
                Profile::parse(&build_as(class, space, pcs, &model)),
                Err(IccError::UnsupportedSpace),
                "{what}: the model alone"
            );

            let with = |tag: &[u8; 4], data: Vec<u8>| {
                let mut tags = model.clone();
                tags.push((*tag, data));
                build_as(class, space, pcs, &tags)
            };
            let profile = Profile::parse(&with(b"A2B0", two_point_table(inputs, 3)))
                .unwrap_or_else(|error| panic!("{what}: {error:?}"));
            assert_eq!(profile.model, None, "{what}: the model does not read");
            let transforms = Transforms::compile(profile.clone()).expect("a transform");
            for intent in Intent::ALL {
                let transform = Transform::compile(&profile, intent).expect("a transform");
                for x in halves(inputs) {
                    let want = two_point_value(inputs, 3, &x);
                    assert_eq!(pcs_at(&transform, &x), want, "{what}, {intent:?} at {x:?}");
                    assert_eq!(
                        pcs_at(transforms.get(intent), &x),
                        want,
                        "{what}, {intent:?} at {x:?}: the space's"
                    );
                }
            }

            // An `A2B2` alone: relative colorimetric's c) and b) are absent
            // and its d) does not read, so it takes the table after them.
            let profile = Profile::parse(&with(b"A2B2", constant_table(inputs, 24_000)))
                .unwrap_or_else(|error| panic!("{what}, A2B2: {error:?}"));
            for intent in Intent::ALL {
                let transform = Transform::compile(&profile, intent).expect("a transform");
                assert_eq!(
                    pcs_at(&transform, &[0.5; 4][..inputs]),
                    [24_000; 3],
                    "{what}, A2B2, {intent:?}"
                );
            }

            // A table that does not read beside a model that does not: the
            // profile is the model's to refuse.
            assert_eq!(
                Profile::parse(&with(b"A2B0", inverse_table())),
                Err(IccError::UnsupportedSpace),
                "{what}: a broken table"
            );
        }
    }

    /// **Intents sharing a source share a transform.** A matrix/TRC profile
    /// carrying only an `A2B1` converts relative colorimetric through the
    /// table and perceptual and saturation through the model, so the set a
    /// colour space carries compiles the model once for the two of them;
    /// carrying only an `A2B0`, perceptual and saturation convert through
    /// relative colorimetric's own transform, the table, and absolute
    /// colorimetric compiles alike.
    #[test]
    fn intents_sharing_a_source_share_one_transform() {
        let one = |present: [bool; 3]| {
            let bytes = model_and_tables(b"RGB ", b"XYZ ", &matrix_tags(), 3, present, [false; 3]);
            Transforms::compile(Profile::parse(&bytes).expect("a profile")).expect("transforms")
        };

        let transforms = one([false, true, false]);
        assert_eq!(table_of(transforms.get(Intent::RelativeColorimetric)), 1);
        assert!(
            transforms.get(Intent::Perceptual).lut.is_none(),
            "the model"
        );
        assert!(
            std::ptr::eq(
                transforms.get(Intent::Perceptual),
                transforms.get(Intent::Saturation)
            ),
            "perceptual and saturation compiled the model twice"
        );
        assert_eq!(table_of(transforms.get(Intent::AbsoluteColorimetric)), 1);

        let transforms = one([true, false, false]);
        for intent in [Intent::Perceptual, Intent::Saturation] {
            assert!(
                std::ptr::eq(
                    transforms.get(intent),
                    transforms.get(Intent::RelativeColorimetric)
                ),
                "{intent:?} compiled a transform of its own"
            );
        }
        assert_eq!(
            transforms.get(Intent::AbsoluteColorimetric),
            transforms.get(Intent::RelativeColorimetric),
            "absolute colorimetric"
        );
        assert_eq!(table_of(transforms.get(Intent::RelativeColorimetric)), 0);
    }

    // ---- absolute colorimetric ----------------------------------------------

    /// A media white other than D50, every channel its own ratio to the PCS
    /// white: a warm paper, darker than the reference.
    const PAPER: [f64; 3] = [0.88, 0.91, 0.72];

    /// What an `XYZType` tag holding `value` reads back as: the
    /// s15Fixed16Number it is stored in.
    fn stored(value: f64) -> f64 {
        (value * 65536.0).round() / 65536.0
    }

    /// Linear sRGB as the byte IEC 61966-2-1's encode makes of it.
    fn encoded(linear: f64) -> i32 {
        let v = linear.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 {
            12.92 * v
        } else {
            1.055 * tinker_pdf_math::pow(v, 1.0 / 2.4) - 0.055
        };
        (s.clamp(0.0, 1.0) * 255.0).round() as i32
    }

    /// A D50 XYZ value as sRGB, converted in the test: the destination's own
    /// matrix, then the clause's encode.
    fn srgb_of(xyz: [f64; 3]) -> [i32; 3] {
        crate::xyz_d50_to_linear_srgb(xyz[0], xyz[1], xyz[2]).map(encoded)
    }

    /// `got` is `want` within the level of 255 the integer path rounds by.
    fn within_a_level(got: (u8, u8, u8), want: [i32; 3], what: &str) {
        let got = [i32::from(got.0), i32::from(got.1), i32::from(got.2)];
        for (channel, (g, w)) in got.iter().zip(want).enumerate() {
            assert!(
                (g - w).abs() <= 1,
                "{what}: channel {channel} is {g}, not {w}"
            );
        }
    }

    /// `x` scaled channel by channel by `scale`.
    fn scaled(x: [f64; 3], scale: [f64; 3]) -> [f64; 3] {
        [x[0] * scale[0], x[1] * scale[1], x[2] * scale[2]]
    }

    /// The scale absolute colorimetry applies for a stored media white of
    /// `white`, in closed form: 6.2.3's media white over the PCS white.
    fn closed_scale(white: [f64; 3]) -> [f64; 3] {
        [
            stored(white[0]) / stored(0.9642),
            stored(white[1]) / stored(1.0),
            stored(white[2]) / stored(0.8249),
        ]
    }

    /// CIE L*a*b* as D50 XYZ, 8.6.5.4's relation run backwards, written out in
    /// the test.
    fn xyz_from_lab(l: f64, a: f64, b: f64) -> [f64; 3] {
        const WHITE: [f64; 3] = [0.964_212, 1.0, 0.825_188];
        const DELTA: f64 = 6.0 / 29.0;
        let finv = |t: f64| {
            if t > DELTA {
                t * t * t
            } else {
                3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
            }
        };
        let fy = (l + 16.0) / 116.0;
        [
            WHITE[0] * finv(fy + a / 500.0),
            WHITE[1] * finv(fy),
            WHITE[2] * finv(fy - b / 200.0),
        ]
    }

    /// The profiles absolute colorimetry is held on, each carrying the
    /// `wtpt` `white` gives, or none: a matrix/TRC profile on sRGB's
    /// primaries with linear curves, a linear grey profile under XYZ and
    /// under Lab, and a three-input `mft2` whose two-point grid holds the
    /// D50 XYZ of linear sRGB's corners, under XYZ and, in v2's legacy
    /// 16-bit Lab, under Lab.
    fn media_profiles(white: Option<Vec<u8>>) -> Vec<(&'static str, Vec<u8>)> {
        let with = |space: &[u8; 4], pcs: &[u8; 4], mut tags: Vec<([u8; 4], Vec<u8>)>| {
            if let Some(white) = &white {
                tags.push((*b"wtpt", white.clone()));
            }
            build(space, pcs, &tags)
        };
        let corner_table = |lab: bool| {
            let clut: Vec<[u16; 3]> = (0..8usize)
                .map(|node| {
                    let rgb = [(node >> 2) & 1, (node >> 1) & 1, node & 1].map(|bit| bit as f64);
                    let xyz = xyz_of(rgb);
                    let encode = |v: f64| v.round().clamp(0.0, 65535.0) as u16;
                    if lab {
                        let [l, a, b] = lab_of(xyz);
                        [
                            encode(l / 100.0 * 65280.0),
                            encode((a + 128.0) / 255.0 * 65280.0),
                            encode((b + 128.0) / 255.0 * 65280.0),
                        ]
                    } else {
                        xyz.map(|v| encode(v * 32768.0))
                    }
                })
                .collect();
            mft2(3, 2, &clut)
        };
        vec![
            (
                "matrix/TRC",
                with(
                    b"RGB ",
                    b"XYZ ",
                    vec![
                        (*b"rXYZ", xyz(SRGB_R[0], SRGB_R[1], SRGB_R[2])),
                        (*b"gXYZ", xyz(SRGB_G[0], SRGB_G[1], SRGB_G[2])),
                        (*b"bXYZ", xyz(SRGB_B[0], SRGB_B[1], SRGB_B[2])),
                        (*b"rTRC", gamma(1.0)),
                        (*b"gTRC", gamma(1.0)),
                        (*b"bTRC", gamma(1.0)),
                    ],
                ),
            ),
            (
                "grey under XYZ",
                with(b"GRAY", b"XYZ ", vec![(*b"kTRC", gamma(1.0))]),
            ),
            (
                "grey under Lab",
                with(b"GRAY", b"Lab ", vec![(*b"kTRC", gamma(1.0))]),
            ),
            (
                "mft2 under XYZ",
                with(b"RGB ", b"XYZ ", vec![(*b"A2B0", corner_table(false))]),
            ),
            (
                "mft2 under Lab",
                with(b"RGB ", b"Lab ", vec![(*b"A2B0", corner_table(true))]),
            ),
        ]
    }

    /// The inputs absolute colorimetry is checked at: every component a
    /// fifth to all but all of the range, clear of the dark end where the
    /// encode's straight segment magnifies a rounding.
    fn mid_tones(inputs: usize) -> Vec<Vec<f64>> {
        let steps = [0.2, 0.45, 0.7, 0.95];
        (0..steps.len().pow(inputs as u32))
            .map(|index| {
                let mut x = vec![0.0; inputs];
                let mut rest = index;
                for slot in x.iter_mut().rev() {
                    *slot = steps[rest % steps.len()];
                    rest /= steps.len();
                }
                x
            })
            .collect()
    }

    /// The connection-space value, as D50 XYZ, `profile`'s relative
    /// colorimetric transform gives `x`: the model's closed form for a
    /// matrix/TRC or grey profile — the columns over the linear device
    /// values, the curve's `Y` on the PCS white (F.2) — and the table's own
    /// evaluation decoded from its 16-bit encoding for a table.
    fn relative_pcs(profile: &Profile, x: &[f64]) -> [f64; 3] {
        let transform =
            Transform::compile(profile, Intent::RelativeColorimetric).expect("a transform");
        if let Some(lut) = &transform.lut {
            let pcs = lut.evaluate(x);
            return if lut.lab {
                let full = 65535.0 / 65280.0;
                xyz_from_lab(
                    pcs[0] * full * 100.0,
                    pcs[1] * full * 255.0 - 128.0,
                    pcs[2] * full * 255.0 - 128.0,
                )
            } else {
                [pcs[0], pcs[1], pcs[2]].map(|v| v * 65535.0 / 32768.0)
            };
        }
        match profile.model.as_ref().expect("a model") {
            Model::MatrixTrc { columns, .. } => {
                let mut xyz = [0.0; 3];
                for (column, &linear) in columns.iter().zip(x) {
                    for (slot, value) in xyz.iter_mut().zip(column) {
                        *slot += value * linear;
                    }
                }
                xyz
            }
            Model::Grey { lab, .. } => {
                let y = if *lab {
                    xyz_from_lab(x[0] * 100.0, 0.0, 0.0)[1]
                } else {
                    x[0]
                };
                PCS_WHITE.map(|white| white * y)
            }
        }
    }

    /// **Absolute colorimetric scales by the media white.** For a matrix/TRC
    /// profile, a grey one under XYZ and under Lab, and an `mft2` one under
    /// XYZ and under Lab, each with a `wtpt` other than D50, the absolute
    /// transform's pipeline gives the relative transform's connection-space
    /// value scaled in closed form — the media white over the PCS white,
    /// channel by channel (6.2.3) — and converted in the test, on its own and
    /// through the set a colour space carries; and it is not the relative
    /// transform's. A table's grid holds the scaled pipeline: at its nodes a
    /// conversion is that pipeline exactly, and between them within P-22's
    /// two levels. A `wtpt` of D50 makes the two intents one transform, byte
    /// for byte; so do no `wtpt`, one with a component that is not positive,
    /// and one that does not read, each taken to be the PCS white. A grey
    /// profile with no `wtpt`, or one that does not read, compiles.
    #[test]
    fn absolute_colorimetric_scales_by_the_media_white() {
        let scale = closed_scale(PAPER);
        for (name, bytes) in media_profiles(Some(xyz(PAPER[0], PAPER[1], PAPER[2]))) {
            let profile =
                Profile::parse(&bytes).unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(profile.media_scale(), scale, "{name}: the scale");
            let absolute =
                Transform::compile(&profile, Intent::AbsoluteColorimetric).expect("a transform");
            let relative =
                Transform::compile(&profile, Intent::RelativeColorimetric).expect("a transform");
            let transforms = Transforms::compile(profile.clone()).expect("a transform");
            let spaces = transforms.get(Intent::AbsoluteColorimetric);
            let mut parted = false;
            for x in mid_tones(absolute.inputs()) {
                let want = srgb_of(scaled(relative_pcs(&profile, &x), scale));
                let what = format!("{name} at {x:?}");
                within_a_level(absolute.evaluate(&x), want, &what);
                within_a_level(spaces.evaluate(&x), want, &format!("{what}, the space's"));
                for (transform, whose) in [(&absolute, "its own"), (spaces, "the space's")] {
                    let read = apart(transform.apply(&x), transform.evaluate(&x));
                    assert!(read <= 2, "{what}, {whose}: the read is {read} levels off");
                }
                let (a, r) = (absolute.apply(&x), relative.apply(&x));
                parted |= [a.0.abs_diff(r.0), a.1.abs_diff(r.1), a.2.abs_diff(r.2)]
                    .into_iter()
                    .any(|difference| difference > 2);
            }
            assert!(parted, "{name}: absolute colorimetric is relative's");
            if let (Some(a), Some(r)) = (&absolute.lut, &relative.lut) {
                assert!(Arc::ptr_eq(a, r), "{name}: one table, scaled after it");
            }
            // At a node of the grid the read is the scaled pipeline itself: the
            // media white is a stage the nodes evaluate.
            if let Some(grid) = &absolute.grid {
                assert_eq!(grid.media, Some(scale), "{name}: the grid's scale");
                for index in [0, grid.count() / 3, grid.count() / 2, grid.count() - 1] {
                    let node = grid.node_input(index);
                    let x = &node[..grid.inputs];
                    assert_eq!(
                        absolute.apply(x),
                        absolute.evaluate(x),
                        "{name} at node {index}"
                    );
                }
            }
        }

        // A grey in the medium's tint: the paper is warm, so its greys are.
        let grey = Profile::parse(&media_profiles(Some(xyz(PAPER[0], PAPER[1], PAPER[2])))[1].1)
            .expect("a grey profile");
        let (r, _, b) = Transform::compile(&grey, Intent::AbsoluteColorimetric)
            .expect("a transform")
            .apply(&[0.95]);
        assert!(r > b + 2, "a warm paper's grey is ({r}, _, {b})");

        // The PCS white, none, a component that is not positive and a tag
        // that does not read: one transform for both intents.
        let mut unread = xyz(0.9642, 1.0, 0.8249);
        unread[0..4].copy_from_slice(b"text");
        for (white, what) in [
            (Some(xyz(0.9642, 1.0, 0.8249)), "a wtpt of D50"),
            (None, "no wtpt"),
            (Some(xyz(0.0, 1.0, 0.8249)), "a wtpt with a zero"),
            (Some(xyz(0.9642, -1.0, 0.8249)), "a wtpt with a negative"),
            (Some(unread), "a wtpt that does not read"),
        ] {
            for (name, bytes) in media_profiles(white.clone()) {
                let profile = Profile::parse(&bytes)
                    .unwrap_or_else(|error| panic!("{name}, {what}: {error:?}"));
                assert_eq!(profile.media_scale(), [1.0; 3], "{name}, {what}");
                assert_eq!(
                    Transform::compile(&profile, Intent::AbsoluteColorimetric),
                    Transform::compile(&profile, Intent::RelativeColorimetric),
                    "{name}, {what}"
                );
                let transforms = Transforms::compile(profile).expect("a transform");
                assert!(
                    std::ptr::eq(
                        transforms.get(Intent::AbsoluteColorimetric),
                        transforms.get(Intent::RelativeColorimetric)
                    ),
                    "{name}, {what}: absolute compiled a transform of its own"
                );
            }
        }
    }

    // ---- multi-process elements: the D2B* tags ------------------------------

    /// `value` as the float32 a multi-process element stores it in.
    fn float(value: f64) -> [u8; 4] {
        (value as f32).to_bits().to_be_bytes()
    }

    /// What a float32 field holding `value` reads back as.
    fn as_stored(value: f64) -> f64 {
        f64::from(value as f32)
    }

    /// `out` padded with zeros to a four-byte boundary, as every element and
    /// curve starts on one (10.14.1, 10.14.2.2).
    fn padded(mut out: Vec<u8>) -> Vec<u8> {
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }

    /// A `parf` formula segment of function type `function`.
    fn parf(function: u16, params: &[f64]) -> Vec<u8> {
        let mut out = b"parf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&function.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        for &param in params {
            out.extend_from_slice(&float(param));
        }
        out
    }

    /// A `samf` sampled segment.
    fn samf(samples: &[f64]) -> Vec<u8> {
        let mut out = b"samf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        for &sample in samples {
            out.extend_from_slice(&float(sample));
        }
        out
    }

    /// A `curf` segmented curve: `segments` with `breaks` between them.
    fn curf(breaks: &[f64], segments: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"curf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(segments.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        for &point in breaks {
            out.extend_from_slice(&float(point));
        }
        for segment in segments {
            out.extend_from_slice(segment);
        }
        out
    }

    /// A processing element's header — its signature, four reserved bytes,
    /// `p` inputs and `q` outputs — and `body`.
    fn element_of(signature: &[u8; 4], p: u16, q: u16, body: &[u8]) -> Vec<u8> {
        let mut out = signature.to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&p.to_be_bytes());
        out.extend_from_slice(&q.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    /// A `cvst` curve set, one `curf` a channel, each starting on a
    /// four-byte boundary.
    fn cvst(curves: &[Vec<u8>]) -> Vec<u8> {
        let channels = curves.len();
        let mut positions = Vec::new();
        let mut data = Vec::new();
        let start = 12 + 8 * channels;
        for curve in curves {
            positions.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
            positions.extend_from_slice(&(curve.len() as u32).to_be_bytes());
            data.extend_from_slice(&padded(curve.clone()));
        }
        positions.extend_from_slice(&data);
        element_of(b"cvst", channels as u16, channels as u16, &positions)
    }

    /// A `matf` matrix: `rows`, each the coefficients of one output, then an
    /// offset a row.
    fn matf(rows: &[Vec<f64>], offsets: &[f64]) -> Vec<u8> {
        let p = rows.first().map_or(0, Vec::len);
        let mut body = Vec::new();
        for row in rows {
            for &value in row {
                body.extend_from_slice(&float(value));
            }
        }
        for &offset in offsets {
            body.extend_from_slice(&float(offset));
        }
        element_of(b"matf", p as u16, rows.len() as u16, &body)
    }

    /// A `clut` lookup table with grid counts `grids` and `q` outputs: the
    /// sixteen grid-count bytes, then `values`.
    fn clut(grids: &[u8], q: u16, values: &[f64]) -> Vec<u8> {
        let mut body = [0u8; 16].to_vec();
        body[..grids.len()].copy_from_slice(grids);
        for &value in values {
            body.extend_from_slice(&float(value));
        }
        element_of(b"clut", grids.len() as u16, q, &body)
    }

    /// An `mpet` tag of `inputs` and `outputs` channels chaining `elements`,
    /// each starting on a four-byte boundary.
    fn mpet(inputs: u16, outputs: u16, elements: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"mpet".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&inputs.to_be_bytes());
        out.extend_from_slice(&outputs.to_be_bytes());
        out.extend_from_slice(&(elements.len() as u32).to_be_bytes());
        let start = 16 + 8 * elements.len();
        let mut data = Vec::new();
        for element in elements {
            out.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
            out.extend_from_slice(&(element.len() as u32).to_be_bytes());
            data.extend_from_slice(&padded(element.clone()));
        }
        out.extend_from_slice(&data);
        out
    }

    /// The pipeline the `D2B*` tests chain, its parameters shifted by `t` so
    /// each tag answers its own: a curve set — a power with a sampled segment
    /// between its two formulas, an exponential and a logarithm, one a
    /// channel — then a matrix with offsets, then a grid of two points an
    /// axis.
    struct Chain {
        t: f64,
    }

    impl Chain {
        /// The first channel's samples, over `(0.25, 0.75]`.
        fn samples(&self) -> [f64; 4] {
            [0.2, 0.35, 0.5, 0.65].map(|v| v + 0.02 * self.t)
        }

        /// The curve set's three `curf`s.
        fn curves(&self) -> [Vec<u8>; 3] {
            let last = self.samples()[3];
            [
                curf(
                    &[0.25, 0.75],
                    &[
                        parf(0, &[2.0, 1.0, 0.0, 0.05 * self.t]),
                        samf(&self.samples()),
                        // Continuous at 0.75: 0.8 x + (last - 0.6).
                        parf(0, &[1.0, 0.8, last - 0.6, 0.0]),
                    ],
                ),
                curf(&[], &[parf(2, &[0.5, 2.0, 1.0, -1.0, 0.02 * self.t])]),
                curf(&[], &[parf(1, &[1.0, 0.5, 9.0, 1.0, 0.01 * self.t])]),
            ]
        }

        fn rows(&self) -> Vec<Vec<f64>> {
            vec![
                vec![0.6 + 0.01 * self.t, 0.3, 0.1],
                vec![0.2, 0.7, 0.1],
                vec![0.1, 0.1, 0.8],
            ]
        }

        fn offsets(&self) -> [f64; 3] {
            [0.05, 0.0, -0.02]
        }

        /// Node `node`'s three values, the first axis slowest.
        fn node(&self, node: usize) -> [f64; 3] {
            let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
            let (i, j, k) = (i as f64, j as f64, k as f64);
            [
                0.2 + 0.6 * i + 0.01 * self.t,
                0.15 + 0.5 * j + 0.1 * i,
                0.1 + 0.6 * k + 0.02 * self.t,
            ]
        }

        /// The `mpet` tag.
        fn tag(&self) -> Vec<u8> {
            let values: Vec<f64> = (0..8).flat_map(|node| self.node(node)).collect();
            mpet(
                3,
                3,
                &[
                    cvst(&self.curves()),
                    matf(&self.rows(), &self.offsets()),
                    clut(&[2, 2, 2], 3, &values),
                ],
            )
        }

        /// Channel `channel`'s curve at `x`, its segments' closed forms
        /// (ICC.1:2010 10.14.2.2, Table 57) over the float32 values stored.
        fn curve(&self, channel: usize, x: f64) -> f64 {
            let r = as_stored;
            match channel {
                0 => {
                    if x <= 0.25 {
                        x * x + r(0.05 * self.t)
                    } else if x <= 0.75 {
                        let samples = self.samples().map(r);
                        let position = (x - 0.25) / 0.5 * 4.0;
                        let below = (position.floor() as usize).min(3);
                        let start = if below == 0 {
                            0.25 * 0.25 + r(0.05 * self.t)
                        } else {
                            samples[below - 1]
                        };
                        start + (samples[below] - start) * (position - below as f64)
                    } else {
                        r(0.8) * x + r(self.samples()[3] - 0.6)
                    }
                }
                1 => r(0.5) * tinker_pdf_math::pow(2.0, x - 1.0) + r(0.02 * self.t),
                _ => r(0.5) * tinker_pdf_math::log10(9.0 * x + 1.0) + r(0.01 * self.t),
            }
        }

        /// The pipeline at `x`, recomputed: the curves, the matrix, the
        /// grid's inputs held to `0..=1`, and trilinear interpolation.
        fn value(&self, x: [f64; 3]) -> [f64; 3] {
            let curved = [0, 1, 2].map(|channel| self.curve(channel, x[channel]));
            let rows = self.rows();
            let offsets = self.offsets();
            let mixed = [0, 1, 2].map(|row| {
                let sum: f64 = (0..3)
                    .map(|column| as_stored(rows[row][column]) * curved[column])
                    .sum();
                (sum + as_stored(offsets[row])).clamp(0.0, 1.0)
            });
            let mut out = [0.0; 3];
            for node in 0..8usize {
                let bits = [(node >> 2) & 1, (node >> 1) & 1, node & 1];
                let weight: f64 = (0..3)
                    .map(|axis| {
                        if bits[axis] == 1 {
                            mixed[axis]
                        } else {
                            1.0 - mixed[axis]
                        }
                    })
                    .product();
                for (slot, value) in out.iter_mut().zip(self.node(node)) {
                    *slot += weight * as_stored(value);
                }
            }
            out
        }
    }

    /// The inputs a pipeline is checked at: each component one of seven
    /// values across the range, every segment of the first curve among them.
    fn pipeline_inputs() -> Vec<[f64; 3]> {
        let steps = [0.0, 0.1, 0.3, 0.5, 0.62, 0.8, 1.0];
        let mut out = Vec::new();
        for a in steps {
            for b in steps {
                for c in steps {
                    out.push([a, b, c]);
                }
            }
        }
        out
    }

    /// The pipeline `transform` converts through, evaluated at `x`.
    fn pipeline_pcs(transform: &Transform, x: [f64; 3]) -> [f64; 3] {
        transform
            .pipeline
            .as_ref()
            .expect("a multi-process transform")
            .evaluate(&x)
    }

    /// **A `D2B*` tag converts by its element pipeline.** A profile whose
    /// `D2B0` to `D2B3` each chain a curve set — a power with a sampled
    /// segment between its formulas, an exponential, a logarithm — a matrix
    /// with offsets and a grid answers each intent with its own tag, the
    /// value the test computes from the elements' closed forms, within the
    /// fixed point the stages compute in, on its own and through the set a
    /// colour space carries: ICC.1 8.10.2 a) before the `A2B*` tables and the
    /// matrix the profile also carries. Absolute colorimetric's `D2B3` is
    /// ICC-absolute already, so the media white, though not D50, scales
    /// nothing; with no `D2B3`, absolute colorimetric converts through
    /// `A2B1`, scaled, and not through `D2B1`. Under a Lab connection space
    /// the pipeline's value is `L*a*b*` as it stands.
    #[test]
    fn a_d2b_tag_converts_by_its_element_pipeline() {
        let mut tags = matrix_tags();
        tags.push((*b"wtpt", xyz(PAPER[0], PAPER[1], PAPER[2])));
        for (t, signature) in [*b"D2B0", *b"D2B1", *b"D2B2", *b"D2B3"]
            .into_iter()
            .enumerate()
        {
            tags.push((signature, Chain { t: t as f64 }.tag()));
        }
        for (t, signature) in [*b"A2B0", *b"A2B1", *b"A2B2"].into_iter().enumerate() {
            tags.push((signature, constant_table(3, fallback_value(t))));
        }
        let profile = Profile::parse(&build(b"RGB ", b"XYZ ", &tags)).expect("a D2B profile");
        let transforms = Transforms::compile(profile.clone()).expect("transforms");
        for (t, intent) in [
            (0, Intent::Perceptual),
            (1, Intent::RelativeColorimetric),
            (2, Intent::Saturation),
            (3, Intent::AbsoluteColorimetric),
        ] {
            let chain = Chain { t: f64::from(t) };
            let transform = Transform::compile(&profile, intent).expect("a transform");
            assert_eq!(
                transform.media, None,
                "{intent:?}: D2B{t} is scaled by nothing"
            );
            for x in pipeline_inputs() {
                let want = chain.value(x);
                for (what, got) in [
                    ("its own", pipeline_pcs(&transform, x)),
                    ("the space's", pipeline_pcs(transforms.get(intent), x)),
                ] {
                    for channel in 0..3 {
                        assert!(
                            (got[channel] - want[channel]).abs() < 2e-3,
                            "{intent:?} at {x:?}, {what}: channel {channel} is {}, not {}",
                            got[channel],
                            want[channel]
                        );
                    }
                }
                assert_eq!(
                    transform.evaluate(&x),
                    pcs_to_rgb(pipeline_pcs(&transform, x), false, None),
                    "{intent:?} at {x:?}: the sRGB of its value"
                );
                let read = apart(transform.apply(&x), transform.evaluate(&x));
                assert!(
                    read <= 2,
                    "{intent:?} at {x:?}: the grid's read is {read} levels off"
                );
            }
        }
        // The four tags are four pipelines.
        let x = [0.62, 0.3, 0.8];
        let values: Vec<[u64; 3]> = Intent::ALL
            .iter()
            .map(|&intent| {
                let transform = Transform::compile(&profile, intent).expect("a transform");
                pipeline_pcs(&transform, x).map(|v| (v * 10_000.0).round() as u64)
            })
            .collect();
        for (a, b) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
            assert_ne!(values[a], values[b], "two intents read one tag");
        }

        // With no `D2B3`, absolute colorimetric's row goes on to `A2B1`,
        // scaled by the media white; `D2B1` is not in it.
        let without: Vec<([u8; 4], Vec<u8>)> = tags
            .iter()
            .filter(|(signature, _)| signature != b"D2B3")
            .cloned()
            .collect();
        let profile = Profile::parse(&build(b"RGB ", b"XYZ ", &without)).expect("a profile");
        let absolute =
            Transform::compile(&profile, Intent::AbsoluteColorimetric).expect("a transform");
        assert!(absolute.pipeline.is_none(), "absolute read a D2B tag");
        assert_eq!(table_of(&absolute), 1, "absolute colorimetric's A2B1");
        assert_eq!(
            absolute.media,
            Some(closed_scale(PAPER)),
            "scaled by the paper"
        );

        // Under Lab, the pipeline's value is L*a*b* itself.
        let lab_chain = Chain { t: 1.0 };
        let lab = Profile::parse(&build(b"RGB ", b"Lab ", &[(*b"D2B1", lab_chain.tag())]))
            .expect("a Lab D2B profile");
        let transform =
            Transform::compile(&lab, Intent::RelativeColorimetric).expect("a transform");
        for x in pipeline_inputs() {
            let pcs = pipeline_pcs(&transform, x);
            assert_eq!(
                transform.evaluate(&x),
                crate::lab_to_rgb(pcs[0], pcs[1], pcs[2]),
                "Lab at {x:?}"
            );
            let read = apart(transform.apply(&x), transform.evaluate(&x));
            assert!(
                read <= 2,
                "Lab at {x:?}: the grid's read is {read} levels off"
            );
        }
    }

    /// **A `D2B*` tag's elements convert through a grid of their light**, as
    /// a table does (`docs/design/icc.md`, P-22, "Which transforms a grid
    /// stands in for"): over three inputs, and over one, a grid of the
    /// design's size whose every node is the elements' pipeline exactly, and
    /// whose reads between nodes are within two levels of it; over two
    /// inputs no grid, every conversion the pipeline itself. A row of bytes
    /// converts through each as `apply` does.
    #[test]
    fn a_d2b_transform_reads_through_a_grid_of_its_elements() {
        let one = mpet(
            1,
            3,
            &[matf(
                &[vec![0.9], vec![0.95], vec![0.8]],
                &[0.02, 0.0, 0.01],
            )],
        );
        let two = mpet(
            2,
            3,
            &[matf(
                &[vec![0.5, 0.4], vec![0.3, 0.6], vec![0.1, 0.7]],
                &[0.0, 0.01, 0.0],
            )],
        );
        let levels: Vec<u8> = (0..17u32).map(|j| ((j * 255 + 8) / 16) as u8).collect();
        for (name, space, tag, inputs) in [
            ("three inputs", b"RGB ", Chain { t: 1.0 }.tag(), 3usize),
            ("one input", b"GRAY", one, 1),
            ("two inputs", b"2CLR", two, 2),
        ] {
            let profile = Profile::parse(&build(space, b"XYZ ", &[(*b"D2B1", tag)]))
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            let transform =
                Transform::compile(&profile, Intent::RelativeColorimetric).expect("a transform");
            assert!(transform.pipeline.is_some(), "{name}: the elements");
            assert_eq!(transform.inputs(), inputs, "{name}");
            match &transform.grid {
                Some(grid) => {
                    assert_eq!(Some(grid.points), grid_points(inputs), "{name}: its size");
                    for index in 0..grid.count() {
                        let node = grid.node_input(index);
                        let x = &node[..inputs];
                        assert_eq!(
                            transform.apply(x),
                            transform.evaluate(x),
                            "{name} at node {index}"
                        );
                    }
                    for x in sweep(inputs) {
                        let read = apart(transform.apply(&x), transform.evaluate(&x));
                        assert!(read <= 2, "{name} at {x:?}: {read} levels off");
                    }
                }
                None => {
                    assert_eq!(grid_points(inputs), None, "{name}: no grid");
                    for x in sweep(inputs) {
                        assert_eq!(
                            transform.apply(&x),
                            transform.evaluate(&x),
                            "{name} at {x:?}"
                        );
                    }
                }
            }
            // Every byte over one input, a lattice of 17 levels over more.
            let mut samples = Vec::new();
            match inputs {
                1 => samples.extend(0..=u8::MAX),
                2 => {
                    for &a in &levels {
                        for &b in &levels {
                            samples.extend_from_slice(&[a, b]);
                        }
                    }
                }
                _ => {
                    for &a in &levels {
                        for &b in &levels {
                            for &c in &levels {
                                samples.extend_from_slice(&[a, b, c]);
                            }
                        }
                    }
                }
            }
            let mut row = Vec::new();
            transform.convert_row(&samples, &mut row);
            row_is_apply(name, &transform, &samples, &row);
        }
    }

    /// An element of a type this build does not read: ICC.2's `JtoX`, a
    /// colour appearance element (11.2.9), outside what this engine reads.
    fn unread_element() -> Vec<u8> {
        element_of(b"JtoX", 3, 3, &[0; 16])
    }

    /// **An unread element is refused by name**, and its intent falls to its
    /// `A2B*` tag. A `D2B*` tag holding a `JtoX` element — ICC.2's colour
    /// appearance element — is refused as `UnsupportedElement(JtoX)` wherever
    /// the element stands, whatever is wrong elsewhere in the tag; a profile
    /// carrying it beside an `A2B1` converts every intent through the table,
    /// and one carrying nothing else is refused for it, by name. The `bACS`
    /// and `eACS` placeholders read, and pass their channels through
    /// unchanged.
    #[test]
    fn an_unread_element_is_refused_by_name() {
        let chain = Chain { t: 0.0 };
        let values: Vec<f64> = (0..8).flat_map(|node| chain.node(node)).collect();
        let curves = cvst(&chain.curves());
        let matrix = matf(&chain.rows(), &chain.offsets());
        let grid = clut(&[2, 2, 2], 3, &values);
        for elements in [
            vec![
                unread_element(),
                curves.clone(),
                matrix.clone(),
                grid.clone(),
            ],
            vec![curves.clone(), unread_element(), grid.clone()],
            vec![
                curves.clone(),
                matrix.clone(),
                grid.clone(),
                unread_element(),
            ],
            // A channel count that does not chain is not what refuses it.
            vec![element_of(b"matf", 9, 3, &[]), unread_element()],
        ] {
            assert_eq!(
                mpe::read(&mpet(3, 3, &elements), false),
                Err(IccError::UnsupportedElement(*b"JtoX")),
                "{} elements",
                elements.len()
            );
        }
        let broken = mpet(3, 3, &[curves.clone(), unread_element(), grid.clone()]);

        // Beside an `A2B1`: passed over, as 8.10.2 a) has a tag the CMM does
        // not support passed over.
        let profile = Profile::parse(&build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"D2B1", broken.clone()),
                (*b"A2B1", constant_table(3, fallback_value(1))),
            ],
        ))
        .expect("the table serves");
        for intent in Intent::ALL {
            let transform = Transform::compile(&profile, intent).expect("a transform");
            assert!(
                transform.pipeline.is_none(),
                "{intent:?} read the broken tag"
            );
            assert_eq!(table_of(&transform), 1, "{intent:?}");
        }

        // Alone: refused, by the element's name.
        assert_eq!(
            Profile::parse(&build(b"RGB ", b"XYZ ", &[(*b"D2B1", broken)])),
            Err(IccError::UnsupportedElement(*b"JtoX"))
        );

        // A tag at a `D2B*` tag that is not a multi-process one.
        assert_eq!(
            mpe::read(&constant_table(3, 9_000), false),
            Err(IccError::MalformedTag)
        );

        // The placeholders pass through: the pipeline with them answers as
        // the pipeline without them.
        let placeholder = |signature: &[u8; 4]| element_of(signature, 3, 3, b"none");
        let plain = mpe::read(&chain.tag(), false).expect("the chain");
        let padded_out = mpe::read(
            &mpet(
                3,
                3,
                &[
                    placeholder(b"bACS"),
                    curves,
                    placeholder(b"eACS"),
                    matrix,
                    placeholder(b"bACS"),
                    grid,
                    placeholder(b"eACS"),
                ],
            ),
            false,
        )
        .expect("the chain with its placeholders");
        assert_eq!(padded_out, plain, "a placeholder changed the pipeline");
    }

    /// **An impossible element count is refused before it is allocated**
    /// (ruling 1). An `mpet` declaring four billion elements in sixteen bytes
    /// is `TooLarge` before its position table is walked, one past
    /// `MAX_MPE_ELEMENTS` likewise, and one at the cap is walked; channel
    /// counts past `MAX_LUT_INPUTS`, a grid whose volume passes
    /// `MAX_CLUT_ENTRIES`, and a grid that fits on its own but not beside the
    /// curves already compiled, are `TooLarge` before their values are read;
    /// a segment count, a sample count and a grid that the tag's bytes cannot
    /// hold are `MalformedTag` before anything is allocated for them.
    #[test]
    fn an_impossible_element_count_is_refused_before_it_is_allocated() {
        let header = |inputs: u16, outputs: u16, count: u32| {
            let mut out = b"mpet".to_vec();
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&inputs.to_be_bytes());
            out.extend_from_slice(&outputs.to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
            out
        };
        assert_eq!(
            mpe::read(&header(3, 3, u32::MAX), false),
            Err(IccError::TooLarge),
            "four billion elements in sixteen bytes"
        );
        assert_eq!(
            mpe::read(&header(3, 3, MAX_MPE_ELEMENTS as u32 + 1), false),
            Err(IccError::TooLarge)
        );
        assert_eq!(
            mpe::read(&header(3, 3, MAX_MPE_ELEMENTS as u32), false),
            Err(IccError::MalformedTag),
            "the cap itself is walked, and its table is not there"
        );
        assert_eq!(
            mpe::read(&header(u16::MAX, 3, 1), false),
            Err(IccError::TooLarge),
            "sixty-five thousand inputs"
        );

        let identity_curve = curf(&[], &[parf(0, &[1.0, 1.0, 0.0, 0.0])]);
        let three = cvst(&[
            identity_curve.clone(),
            identity_curve.clone(),
            identity_curve.clone(),
        ]);
        // An output count past the cap, at a matrix.
        assert_eq!(
            mpe::read(&mpet(3, 3, &[element_of(b"matf", 3, u16::MAX, &[])]), false),
            Err(IccError::TooLarge)
        );
        // A grid of 255 points on each of sixteen axes.
        let identity16 = cvst(&vec![identity_curve.clone(); 16]);
        let mut wide = element_of(b"clut", 16, 3, &[255; 16]);
        wide.extend_from_slice(&[0; 4]);
        assert_eq!(
            mpe::read(&mpet(16, 3, &[identity16.clone(), wide]), false),
            Err(IccError::TooLarge),
            "a grid of 255^16 points"
        );
        // A grid the bytes cannot hold.
        assert_eq!(
            mpe::read(
                &mpet(3, 3, &[three.clone(), clut(&[9, 9, 9], 3, &[0.5; 10])]),
                false
            ),
            Err(IccError::MalformedTag)
        );
        // Sixty-three curve sets of sixteen curves spend all but 65 536 of
        // `MAX_CLUT_ENTRIES`, so a grid of 131 072 entries, under the cap on
        // its own, is past what is left.
        let mut chain: Vec<Vec<u8>> = vec![identity16.clone(); MAX_MPE_ELEMENTS - 1];
        chain.push(element_of(b"clut", 16, 2, &[2; 16]));
        assert_eq!(
            mpe::read(&mpet(16, 3, &chain), false),
            Err(IccError::TooLarge),
            "past the budget the curves left"
        );

        // A segment count, and a sample count, the curve's bytes cannot hold.
        let mut many = b"curf".to_vec();
        many.extend_from_slice(&[0; 4]);
        many.extend_from_slice(&u16::MAX.to_be_bytes());
        many.extend_from_slice(&[0; 2]);
        let mut huge = b"samf".to_vec();
        huge.extend_from_slice(&[0; 4]);
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        let sampled = curf(
            &[0.25, 0.75],
            &[
                parf(0, &[1.0, 1.0, 0.0, 0.0]),
                huge,
                parf(0, &[1.0, 1.0, 0.0, 0.0]),
            ],
        );
        for curve in [many, sampled] {
            let set = cvst(&[curve, identity_curve.clone(), identity_curve.clone()]);
            assert_eq!(
                mpe::read(&mpet(3, 3, &[set]), false),
                Err(IccError::MalformedTag)
            );
        }
    }

    /// A value of [`wide_table`]'s node `node`, channel `channel`: a multiple
    /// of 256, so halving it eight times leaves a whole number.
    fn wide_node(node: usize, channel: usize) -> u16 {
        (256 * ((node * 29 + channel * 7) % 256)) as u16
    }

    /// An `mft2` over `inputs` channels on a two-point grid, every node its
    /// own value ([`wide_node`]).
    fn wide_table(inputs: usize) -> Vec<u8> {
        let clut: Vec<[u16; 3]> = (0..1usize << inputs)
            .map(|node| [0, 1, 2].map(|channel| wide_node(node, channel)))
            .collect();
        mft2(inputs, 2, &clut)
    }

    /// **A table over seven or more inputs interpolates without overflow.**
    /// Over six inputs a corner's weight is a product of fractions that a
    /// 64-bit sum holds; over seven it is not, and an XPS document embeds
    /// seven- and eight-ink printer profiles. A grid over six, seven and
    /// eight inputs gives, at every input whose components are nought, a half
    /// or one, the mean of the nodes of the cell's face it stands on; a
    /// table declaring seventeen inputs is refused before its grid is read.
    #[test]
    fn a_table_over_seven_or_more_inputs_interpolates_without_overflow() {
        for inputs in [6usize, 7, 8] {
            let profile =
                Profile::parse(&build(b"CMYK", b"XYZ ", &[(*b"A2B0", wide_table(inputs))]))
                    .unwrap_or_else(|error| panic!("{inputs} inputs: {error:?}"));
            let transform =
                Transform::compile(&profile, Intent::RelativeColorimetric).expect("a transform");
            // Every third input of the 3^n, so eight inputs check 2 187.
            for x in halves(inputs).into_iter().step_by(3) {
                let mut sum = [0u64; 3];
                let mut count = 0u64;
                for node in 0..1usize << inputs {
                    let on = (0..inputs).all(|axis| {
                        let high = (node >> (inputs - 1 - axis)) & 1 == 1;
                        match x[axis] {
                            v if v == 0.0 => !high,
                            v if v == 1.0 => high,
                            _ => true,
                        }
                    });
                    if on {
                        count += 1;
                        for (channel, slot) in sum.iter_mut().enumerate() {
                            *slot += u64::from(wide_node(node, channel));
                        }
                    }
                }
                let want = sum.map(|total| (total / count) as u32);
                assert_eq!(pcs_at(&transform, &x), want, "{inputs} inputs at {x:?}");
                let _ = transform.apply(&x);
            }
        }
        let mut seventeen = b"mft2".to_vec();
        seventeen.extend_from_slice(&[0; 4]);
        seventeen.extend_from_slice(&[17, 3, 2, 0]);
        assert_eq!(
            Profile::parse(&build(b"CMYK", b"XYZ ", &[(*b"A2B0", seventeen)])),
            Err(IccError::MalformedTag),
            "seventeen inputs"
        );
    }

    // ---- the price of an integer grid --------------------------------------

    /// sRGB's primaries under three `curv` gammas of `g`: a matrix/TRC
    /// profile.
    fn matrix_trc(g: f64) -> Vec<u8> {
        build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(SRGB_R[0], SRGB_R[1], SRGB_R[2])),
                (*b"gXYZ", xyz(SRGB_G[0], SRGB_G[1], SRGB_G[2])),
                (*b"bXYZ", xyz(SRGB_B[0], SRGB_B[1], SRGB_B[2])),
                (*b"rTRC", gamma(g)),
                (*b"gTRC", gamma(g)),
                (*b"bTRC", gamma(g)),
                (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
            ],
        )
    }

    /// A grey profile whose `kTRC` is a gamma of `g`, into the connection
    /// space `pcs`.
    fn grey(pcs: &[u8; 4], g: f64) -> Vec<u8> {
        build(
            b"GRAY",
            pcs,
            &[(*b"kTRC", gamma(g)), (*b"wtpt", xyz(0.9642, 1.0, 0.8249))],
        )
    }

    /// The light a printed CMYK patch gives back, as linear sRGB: each ink's
    /// dot spread by a fifth of the paper at half cover, and the paper's light
    /// through what the inks leave of it.
    fn printed(cmyk: [f64; 4]) -> [f64; 3] {
        let spread = |t: f64| t + 0.8 * t * (1.0 - t);
        let black = 1.0 - spread(cmyk[3]);
        [
            (1.0 - spread(cmyk[0])) * black,
            (1.0 - spread(cmyk[1])) * black,
            (1.0 - spread(cmyk[2])) * black,
        ]
    }

    /// Linear sRGB as D50 XYZ.
    fn xyz_of(rgb: [f64; 3]) -> [f64; 3] {
        let row = |i: usize| -> f64 {
            crate::SRGB_TO_XYZ_D50[i]
                .iter()
                .zip(rgb)
                .map(|(m, v)| m * v)
                .sum()
        };
        [row(0), row(1), row(2)]
    }

    /// D50 XYZ as CIE L*a*b*, 8.6.5.4's relation run forwards.
    fn lab_of(xyz: [f64; 3]) -> [f64; 3] {
        const WHITE: [f64; 3] = [0.964_212, 1.0, 0.825_188];
        const DELTA: f64 = 6.0 / 29.0;
        let f = |t: f64| {
            if t > DELTA * DELTA * DELTA {
                tinker_pdf_math::cbrt(t)
            } else {
                t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
            }
        };
        let (fx, fy, fz) = (
            f(xyz[0] / WHITE[0]),
            f(xyz[1] / WHITE[1]),
            f(xyz[2] / WHITE[2]),
        );
        [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
    }

    /// A four-input `mft2` printer table over a nine-point grid holding
    /// [`printed`]'s light, in v2's legacy 16-bit Lab where `lab` and
    /// otherwise in XYZ, whose 1.0 is `0x8000`.
    fn cmyk_table(lab: bool) -> Vec<u8> {
        let grid = 9usize;
        let encode = |v: f64| v.round().clamp(0.0, 65535.0) as u16;
        let clut: Vec<[u16; 3]> = (0..grid.pow(4))
            .map(|index| {
                let mut cmyk = [0.0; 4];
                let mut rest = index;
                for slot in cmyk.iter_mut().rev() {
                    *slot = (rest % grid) as f64 / (grid - 1) as f64;
                    rest /= grid;
                }
                let xyz = xyz_of(printed(cmyk));
                if lab {
                    let [l, a, b] = lab_of(xyz);
                    [
                        encode(l / 100.0 * 65280.0),
                        encode((a + 128.0) / 255.0 * 65280.0),
                        encode((b + 128.0) / 255.0 * 65280.0),
                    ]
                } else {
                    xyz.map(|v| encode(v * 32768.0))
                }
            })
            .collect();
        let pcs = if lab { b"Lab " } else { b"XYZ " };
        lut_profile(b"CMYK", pcs, mft2(4, grid, &clut))
    }

    /// A three-input `mAB ` carrying all five of its stages: A curves of
    /// gamma 2.2, a grid of nine, seven and five points that passes its inputs
    /// through, identity M curves, the matrix that takes linear sRGB to the
    /// connection space's XYZ, and identity B curves — an RGB profile on
    /// sRGB's primaries written the v4 way, with a grid count of its own on
    /// each axis.
    fn mab_every_stage() -> Vec<u8> {
        fn pad(tag: &mut Vec<u8>) {
            while tag.len() % 4 != 0 {
                tag.push(0);
            }
        }
        let identity = {
            let mut curve = b"curv".to_vec();
            curve.extend_from_slice(&[0; 4]);
            curve.extend_from_slice(&0u32.to_be_bytes());
            curve
        };
        let mut tag = b"mAB ".to_vec();
        tag.extend_from_slice(&[0; 4]);
        tag.push(3);
        tag.push(3);
        tag.extend_from_slice(&[0; 2]);
        // The five offsets, filled in below in the header's order: B,
        // matrix, M, CLUT, A.
        tag.extend_from_slice(&[0; 20]);
        let mut offsets = [0u32; 5];

        offsets[0] = tag.len() as u32;
        for _ in 0..3 {
            tag.extend_from_slice(&identity);
        }

        // 1.0 of XYZ is 0x8000 of the 16-bit range the stages speak.
        offsets[1] = tag.len() as u32;
        let scale = 32768.0 / 65535.0;
        for row in crate::SRGB_TO_XYZ_D50 {
            for value in row {
                let fixed = (value * scale * 65536.0).round() as i32;
                tag.extend_from_slice(&(fixed as u32).to_be_bytes());
            }
        }
        tag.extend_from_slice(&[0; 12]);

        offsets[2] = tag.len() as u32;
        for _ in 0..3 {
            tag.extend_from_slice(&identity);
        }

        offsets[3] = tag.len() as u32;
        let grids = [9usize, 7, 5];
        let mut counts = [0u8; 16];
        for (slot, &points) in counts.iter_mut().zip(&grids) {
            *slot = points as u8;
        }
        tag.extend_from_slice(&counts);
        // Two bytes a sample, then three of padding.
        tag.push(2);
        tag.extend_from_slice(&[0; 3]);
        for i in 0..grids[0] {
            for j in 0..grids[1] {
                for k in 0..grids[2] {
                    for (at, points) in [(i, grids[0]), (j, grids[1]), (k, grids[2])] {
                        let value = (at as f64 / (points - 1) as f64 * 65535.0).round() as u16;
                        tag.extend_from_slice(&value.to_be_bytes());
                    }
                }
            }
        }
        pad(&mut tag);

        offsets[4] = tag.len() as u32;
        for _ in 0..3 {
            let mut curve = gamma(2.2);
            pad(&mut curve);
            tag.extend_from_slice(&curve);
        }

        for (slot, offset) in offsets.iter().enumerate() {
            tag[12 + 4 * slot..16 + 4 * slot].copy_from_slice(&offset.to_be_bytes());
        }
        tag
    }

    /// How a probe grid is read between its nodes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Form {
        /// One step along each axis in turn from the cell's lowest node,
        /// the largest fraction first, through the simplex that holds the
        /// point: linear over one input, tetrahedral over three, the
        /// 4-simplex's five nodes over four.
        Simplex,
        /// Tetrahedral over the first three inputs at the two nodes of the
        /// fourth that bracket it, then linear between them: C, M and Y, and
        /// then K, eight nodes.
        TetrahedralThenLinear,
        /// Every corner of the cell, weighted by the product of its
        /// fractions: sixteen nodes over four inputs.
        Multilinear,
    }

    impl Form {
        /// The forms a grid over `inputs` is priced in: the one simplex over
        /// one or three, and each form over four that the design weighs.
        fn priced(inputs: usize) -> &'static [Form] {
            if inputs == 4 {
                &[
                    Form::Simplex,
                    Form::TetrahedralThenLinear,
                    Form::Multilinear,
                ]
            } else {
                &[Form::Simplex]
            }
        }

        /// The form's name in the table, for a grid over `inputs`.
        fn name(self, inputs: usize) -> &'static str {
            match (self, inputs) {
                (Form::Simplex, 1) => "linear",
                (Form::Simplex, 3) => "tetrahedral",
                (Form::Simplex, _) => "simplex",
                (Form::TetrahedralThenLinear, _) => "tetrahedral then linear",
                (Form::Multilinear, _) => "multilinear",
            }
        }
    }

    /// A probe's integer grid: a transform's sRGB at every node of `points`
    /// per axis over its inputs, the last axis fastest, three bytes a node.
    /// A node holds the direct pipeline's own answer there,
    /// [`Transform::evaluate`]'s, so between the nodes the grid's difference
    /// from the pipeline is what it costs.
    struct ProbeGrid {
        inputs: usize,
        points: usize,
        nodes: Vec<[u8; 3]>,
    }

    impl ProbeGrid {
        /// The grid, and the pipeline evaluations its build made.
        fn build(transform: &Transform, points: usize) -> (ProbeGrid, u64) {
            let inputs = transform.inputs();
            let count = points.pow(inputs as u32);
            let mut evaluations = 0u64;
            let nodes = (0..count)
                .map(|index| {
                    evaluations += 1;
                    let (r, g, b) = transform.evaluate(&Self::node_input(index, inputs, points));
                    [r, g, b]
                })
                .collect();
            (
                ProbeGrid {
                    inputs,
                    points,
                    nodes,
                },
                evaluations,
            )
        }

        /// The input node `index` stands at: each axis's node over 0..=1.
        fn node_input(index: usize, inputs: usize, points: usize) -> Vec<f64> {
            let mut x = vec![0.0; inputs];
            let mut rest = index;
            for slot in x.iter_mut().rev() {
                *slot = (rest % points) as f64 / (points - 1) as f64;
                rest /= points;
            }
            x
        }

        fn node(&self, at: &[usize]) -> [u8; 3] {
            self.nodes[at.iter().fold(0, |index, &i| index * self.points + i)]
        }

        /// The grid's bytes, one a channel.
        fn bytes(&self) -> usize {
            self.nodes.len() * 3
        }

        /// `x` read through the grid in `form`, in 16-bit fixed point, each
        /// channel rounded once, at the end, to its byte.
        fn read(&self, form: Form, x: &[f64]) -> [u8; 3] {
            let (cells, fractions): (Vec<usize>, Vec<i64>) =
                x.iter().map(|&v| locate(v, self.points)).unzip();
            let round =
                |v: i128, bits: u32| ((v + (1i128 << (bits - 1))) >> bits).clamp(0, 255) as u8;
            match form {
                Form::Simplex => {
                    let axes: Vec<usize> = (0..self.inputs).collect();
                    self.simplex(cells, &axes, &fractions)
                        .map(|channel| round(channel, 16))
                }
                Form::TetrahedralThenLinear => {
                    let last = self.inputs - 1;
                    let axes: Vec<usize> = (0..last).collect();
                    let low = self.simplex(cells.clone(), &axes, &fractions);
                    let mut above = cells;
                    above[last] += 1;
                    let high = self.simplex(above, &axes, &fractions);
                    let along = i128::from(fractions[last]);
                    [0, 1, 2].map(|c| round(low[c] * 65536 + along * (high[c] - low[c]), 32))
                }
                Form::Multilinear => {
                    let mut sum = [0i128; 3];
                    for corner in 0..1usize << self.inputs {
                        let mut weight = 1i128;
                        let mut at = cells.clone();
                        for (axis, &fraction) in fractions.iter().enumerate() {
                            if corner & (1 << axis) != 0 {
                                weight *= i128::from(fraction);
                                at[axis] += 1;
                            } else {
                                weight *= 65536 - i128::from(fraction);
                            }
                        }
                        let node = self.node(&at);
                        for (slot, value) in sum.iter_mut().zip(node) {
                            *slot += weight * i128::from(value);
                        }
                    }
                    sum.map(|channel| round(channel, 16 * self.inputs as u32))
                }
            }
        }

        /// From the node at `at`, a step along each of `axes` in turn, the
        /// largest fraction first, the earlier axis first between equals:
        /// each node so reached weighed by its step's fraction against the
        /// one before it. In units of 2^-16 of a level.
        fn simplex(&self, mut at: Vec<usize>, axes: &[usize], fractions: &[i64]) -> [i128; 3] {
            let mut order = axes.to_vec();
            order.sort_by(|&a, &b| fractions[b].cmp(&fractions[a]).then(a.cmp(&b)));
            let mut previous = self.node(&at);
            let mut sum = previous.map(|value| i128::from(value) << 16);
            for axis in order {
                at[axis] += 1;
                let next = self.node(&at);
                for ((slot, now), before) in sum.iter_mut().zip(next).zip(previous) {
                    *slot += i128::from(fractions[axis]) * (i128::from(now) - i128::from(before));
                }
                previous = next;
            }
            sum
        }
    }

    /// The inputs a grid is priced over: 8-bit values, every one over one
    /// input, every fifth over three, every seventeenth over four, every
    /// combination of them.
    fn sweep(inputs: usize) -> Vec<Vec<f64>> {
        let step = match inputs {
            1 => 1,
            3 => 5,
            _ => 17,
        };
        let axis: Vec<f64> = (0..=255u32)
            .step_by(step)
            .map(|v| f64::from(v) / 255.0)
            .collect();
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

    /// The head of `crates/tinker-pdf-color/data/icc_grid_prices.tsv`, which
    /// [`the_grid_prices_the_design_quotes`] writes before its rows.
    const GRID_PRICES_HEAD: &str = "\
# The price of an integer grid standing in for each compiled ICC transform,
# written by the_grid_prices_the_design_quotes in
# crates/tinker-pdf-color/src/icc.rs:
#
#   cargo test -p tinker-pdf-color --release -- --ignored the_grid_prices_the_design_quotes
#
# Nine tab-separated fields, a row for each profile, grid size and
# interpolation:
#
#   profile        the model the tree reads, as the probe builds it
#   inputs         the transform's input components
#   points         grid points along each axis
#   interpolation  how the grid is read between its nodes, in 16-bit fixed point
#   nodes          points to the power of inputs
#   bytes          three a node, an sRGB byte a channel
#   evaluations    pipeline evaluations the grid's build made
#   worst          the largest difference, in levels of 255, from the direct
#                  pipeline, Transform::evaluate
#   mean           the mean difference, over every channel of every input swept
#
# The sweep is of 8-bit inputs: every value over one input, every fifth over
# three, every seventeenth over four. Every node reproduces the direct
# pipeline exactly, which the probe asserts before it prices a grid. The
# probe's own grids hold sRGB bytes, three a node; the row whose
# interpolation is \"the transform's grid\" is the grid Transform::apply reads
# (docs/design/icc.md, P-22), eight bytes a node, its evaluations the nodes
# the sweep filled.
profile\tinputs\tpoints\tinterpolation\tnodes\tbytes\tevaluations\tworst\tmean
";

    /// **The grid prices the design quotes.**
    ///
    /// For each profile model the tree reads — matrix/TRC at gamma 1.0 and
    /// 2.2, grey under XYZ and under Lab, `mft2` CMYK into Lab and into XYZ,
    /// and a three-input `mAB ` with every stage — a 17- and a 33-point grid
    /// built from [`Transform::evaluate`], the direct pipeline, at its nodes,
    /// read in fixed point in each form [`Form::priced`] names, against the
    /// pipeline over [`sweep`]'s inputs: its nodes, bytes and build
    /// evaluations, and its worst and mean difference in levels; and, for a
    /// transform `apply` reads through a [`Grid`], that grid priced the same
    /// way. The pipeline is the reference because it is this engine's own,
    /// pinned by this module's known-answer tests; no other colour engine is
    /// asked (ruling 13).
    ///
    /// Every node is read back through every form and must be the pipeline's
    /// answer exactly before the grid is priced, so a wrong grid cannot price
    /// itself. The rows are printed after a `RAN` banner and written to the
    /// committed table, which this run regenerates.
    #[test]
    #[ignore = "prices every grid, about six million pipeline evaluations; rewrites \
                crates/tinker-pdf-color/data/icc_grid_prices.tsv"]
    fn the_grid_prices_the_design_quotes() {
        let profiles: [(&str, Vec<u8>); 7] = [
            ("matrix/TRC, gamma 1.0", matrix_trc(1.0)),
            ("matrix/TRC, gamma 2.2", matrix_trc(2.2)),
            ("grey under XYZ, gamma 2.2", grey(b"XYZ ", 2.2)),
            ("grey under Lab, gamma 1.0", grey(b"Lab ", 1.0)),
            ("mft2 CMYK to Lab", cmyk_table(true)),
            ("mft2 CMYK to XYZ", cmyk_table(false)),
            (
                "mAB RGB to XYZ",
                lut_profile(b"RGB ", b"XYZ ", mab_every_stage()),
            ),
        ];
        let mut rows = String::new();
        for (name, bytes) in &profiles {
            let profile = Profile::parse(bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let transform = Transform::compile(&profile, Intent::RelativeColorimetric)
                .unwrap_or_else(|| panic!("{name}"));
            let inputs = transform.inputs();
            let samples = sweep(inputs);
            let direct: Vec<[u8; 3]> = samples
                .iter()
                .map(|x| {
                    let (r, g, b) = transform.evaluate(x);
                    [r, g, b]
                })
                .collect();
            for points in [17usize, 33] {
                let (grid, evaluations) = ProbeGrid::build(&transform, points);
                assert_eq!(
                    evaluations,
                    grid.nodes.len() as u64,
                    "{name}: one evaluation a node"
                );
                for index in 0..grid.nodes.len() {
                    let x = ProbeGrid::node_input(index, inputs, points);
                    let (r, g, b) = transform.evaluate(&x);
                    for &form in Form::priced(inputs) {
                        assert_eq!(
                            grid.read(form, &x),
                            [r, g, b],
                            "{name}, {points} points, {form:?}: node {index} is not the pipeline"
                        );
                    }
                }
                for &form in Form::priced(inputs) {
                    let (mut worst, mut total) = (0u8, 0u64);
                    for (x, want) in samples.iter().zip(&direct) {
                        for (got, want) in grid.read(form, x).into_iter().zip(*want) {
                            let difference = got.abs_diff(want);
                            worst = worst.max(difference);
                            total += u64::from(difference);
                        }
                    }
                    let mean = total as f64 / (samples.len() * 3) as f64;
                    rows.push_str(&format!(
                        "{name}\t{inputs}\t{points}\t{}\t{}\t{}\t{evaluations}\t{worst}\t{mean:.4}\n",
                        form.name(inputs),
                        grid.nodes.len(),
                        grid.bytes(),
                    ));
                }
            }
            if let Some(built) = &transform.grid {
                for index in 0..built.count() {
                    let x = built.node_input(index);
                    assert_eq!(
                        transform.apply(&x[..inputs]),
                        transform.evaluate(&x[..inputs]),
                        "{name}: the transform's grid at node {index} is not the pipeline"
                    );
                }
                let fresh = transform.clone();
                let _ = pipeline_evaluations();
                let (mut worst, mut total) = (0u8, 0u64);
                for (x, want) in samples.iter().zip(&direct) {
                    let (r, g, b) = fresh.apply(x);
                    for (got, want) in [r, g, b].into_iter().zip(*want) {
                        let difference = got.abs_diff(want);
                        worst = worst.max(difference);
                        total += u64::from(difference);
                    }
                }
                let filled = pipeline_evaluations();
                let mean = total as f64 / (samples.len() * 3) as f64;
                rows.push_str(&format!(
                    "{name}\t{inputs}\t{}\tthe transform's grid\t{}\t{}\t{filled}\t{worst}\t{mean:.4}\n",
                    built.points,
                    built.count(),
                    built.count() * 8,
                ));
            }
        }
        println!("icc-grid-prices: RAN, {} rows", rows.lines().count());
        print!("{rows}");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("icc_grid_prices.tsv");
        std::fs::write(&path, format!("{GRID_PRICES_HEAD}{rows}"))
            .unwrap_or_else(|error| panic!("writing {path:?}: {error}"));
    }

    // ---- the grid (docs/design/icc.md, P-22) --------------------------------

    /// A one-input `mft2` grey printer table into XYZ over nine nodes, each
    /// the D50 white scaled by its input squared: a curve the one-input grid
    /// has to follow between its 256 nodes, and the model P-22.1 had no row
    /// for, since its grey profiles are curves rather than tables.
    fn grey_table() -> Vec<u8> {
        let white = [0.9642f64, 1.0, 0.8249];
        let clut: Vec<[u16; 3]> = (0..9u32)
            .map(|node| {
                let y = f64::from(node * node) / 64.0;
                white.map(|w| (w * y * 32768.0).round() as u16)
            })
            .collect();
        lut_profile(b"GRAY", b"XYZ ", mft2(1, 9, &clut))
    }

    /// Each model P-22.1 priced, and [`grey_table`], compiled for relative
    /// colorimetric, by name.
    fn priced_models() -> Vec<(&'static str, Transform)> {
        [
            ("matrix/TRC, gamma 1.0", matrix_trc(1.0)),
            ("matrix/TRC, gamma 2.2", matrix_trc(2.2)),
            ("grey under XYZ, gamma 2.2", grey(b"XYZ ", 2.2)),
            ("grey under Lab, gamma 1.0", grey(b"Lab ", 1.0)),
            ("mft2 CMYK to Lab", cmyk_table(true)),
            ("mft2 CMYK to XYZ", cmyk_table(false)),
            (
                "mAB RGB to XYZ",
                lut_profile(b"RGB ", b"XYZ ", mab_every_stage()),
            ),
            ("mft2 grey to XYZ", grey_table()),
        ]
        .into_iter()
        .map(|(name, bytes)| {
            let profile = Profile::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let transform = Transform::compile(&profile, Intent::RelativeColorimetric)
                .unwrap_or_else(|| panic!("{name}: no transform"));
            (name, transform)
        })
        .collect()
    }

    /// The largest of three channels' differences between two colours.
    fn apart(a: (u8, u8, u8), b: (u8, u8, u8)) -> u8 {
        a.0.abs_diff(b.0)
            .max(a.1.abs_diff(b.1))
            .max(a.2.abs_diff(b.2))
    }

    /// **The grid is the pipeline at its nodes, and within the bound
    /// between** (`docs/design/icc.md`, P-22, "The bound").
    ///
    /// For each lookup-table model P-22.1 priced, and a one-input table: a
    /// grid of the design's size, every node of which converts to the direct
    /// pipeline's bytes exactly; every 8-bit input over one input exact, as
    /// each is a node, and a component between two within one level; every
    /// input of P-22.1's sweep over three and four within two levels. The
    /// matrix/TRC and grey models build no grid and convert as their pipeline
    /// does. A failure names the worst input it met.
    #[test]
    fn the_grid_is_the_pipeline_at_its_nodes_and_within_the_bound_between() {
        for (name, transform) in priced_models() {
            let inputs = transform.inputs();
            let Some(grid) = &transform.grid else {
                assert!(
                    transform.lut.is_none(),
                    "{name}: a lookup table over {inputs} inputs with no grid"
                );
                for x in sweep(inputs) {
                    assert_eq!(
                        transform.apply(&x),
                        transform.evaluate(&x),
                        "{name} at {x:?}: a transform with no grid is its pipeline"
                    );
                }
                continue;
            };
            assert_eq!(Some(grid.points), grid_points(inputs), "{name}: its size");
            assert_eq!(grid.count(), grid.points.pow(inputs as u32));
            for index in 0..grid.count() {
                let x = grid.node_input(index);
                assert_eq!(
                    transform.apply(&x[..inputs]),
                    transform.evaluate(&x[..inputs]),
                    "{name}: node {index} at {:?} is not the pipeline",
                    &x[..inputs]
                );
            }
            let bound = if inputs == 1 { 0 } else { 2 };
            let mut worst: (u8, Vec<f64>) = (0, Vec::new());
            for x in sweep(inputs) {
                let difference = apart(transform.apply(&x), transform.evaluate(&x));
                if difference > worst.0 {
                    worst = (difference, x);
                }
            }
            assert!(
                worst.0 <= bound,
                "{name}: {} levels from the pipeline at {:?}, past the bound of {bound}",
                worst.0,
                worst.1
            );
            if inputs == 1 {
                for step in 0..=4096u32 {
                    let x = [f64::from(step) / 4096.0];
                    let difference = apart(transform.apply(&x), transform.evaluate(&x));
                    assert!(
                        difference <= 1,
                        "{name}: {difference} levels from the pipeline at {x:?}, between nodes"
                    );
                }
            }
        }
    }

    /// **A conversion through a filled grid evaluates no pipeline.** Before
    /// every node is filled, any number of conversions evaluate at most one
    /// pipeline a node; once every node is, a sweep on and between the nodes
    /// evaluates none.
    #[test]
    fn a_conversion_evaluates_no_pipeline() {
        for (name, transform) in priced_models() {
            let Some(grid) = &transform.grid else {
                continue;
            };
            let inputs = transform.inputs();
            let _ = pipeline_evaluations();
            for _ in 0..2 {
                for x in sweep(inputs) {
                    let _ = transform.apply(&x);
                }
            }
            let filling = pipeline_evaluations();
            assert!(filling > 0, "{name}: the sweep filled no node");
            assert!(
                filling <= grid.count() as u64,
                "{name}: {filling} evaluations for {} nodes",
                grid.count()
            );
            for index in 0..grid.count() {
                let x = grid.node_input(index);
                let _ = transform.apply(&x[..inputs]);
            }
            let _ = pipeline_evaluations();
            let mut state = 0x9E37_79B9u32;
            let mut component = || {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f64::from(state >> 8) / f64::from(1u32 << 24)
            };
            for _ in 0..4096 {
                let x: Vec<f64> = (0..inputs).map(|_| component()).collect();
                let _ = transform.apply(&x);
            }
            for x in sweep(inputs) {
                let _ = transform.apply(&x);
            }
            assert_eq!(
                pipeline_evaluations(),
                0,
                "{name}: a conversion through a filled grid evaluated the pipeline"
            );
        }
    }

    /// **Each encoding table is its pipeline's encoding**: at every one of
    /// the 65 536 values the byte the pipeline's own encoding gives, never
    /// falling as the light rises, every byte reached, and each byte's run the
    /// values that encode to it and no others.
    #[test]
    fn each_encoding_table_is_its_pipelines_encoding_and_reaches_every_byte() {
        for encoding in [Encoding::Lab, Encoding::Xyz] {
            let table = encoding.table();
            assert_eq!(table.bytes.len(), 65_536, "{encoding:?}");
            let mut previous = 0u8;
            for value in 0..=u16::MAX {
                let byte = table.bytes[usize::from(value)];
                assert_eq!(
                    byte,
                    encoding.encode(f64::from(value) / 65535.0),
                    "{encoding:?} at {value}"
                );
                assert!(byte >= previous, "{encoding:?} falls at {value}");
                previous = byte;
            }
            for byte in 0..=u8::MAX {
                let (low, high) = table.runs[usize::from(byte)];
                assert!(low <= high, "{encoding:?}: nothing encodes to {byte}");
                assert_eq!(table.bytes[usize::from(low)], byte, "{encoding:?}");
                assert_eq!(table.bytes[usize::from(high)], byte, "{encoding:?}");
                if low > 0 {
                    assert_ne!(table.bytes[usize::from(low) - 1], byte, "{encoding:?}");
                }
                if high < u16::MAX {
                    assert_ne!(table.bytes[usize::from(high) + 1], byte, "{encoding:?}");
                }
            }
            assert_eq!(table.encode(-5), 0, "{encoding:?}: below the gamut");
            assert_eq!(table.encode(1 << 20), 255, "{encoding:?}: above it");
        }
    }

    /// **A node holds each channel's light signed and unclamped**, from −16
    /// to 16, in one word beside its flag; light within the gamut is pulled
    /// into the run of the pipeline's byte, and light past ±16 is held at
    /// the end of the range.
    #[test]
    fn a_node_holds_its_light_signed_and_reads_back_the_pipelines_byte() {
        let limit = 1i64 << (CHANNEL_BITS - 1);
        for values in [
            [0, 0, 0],
            [-limit, limit - 1, 12_345],
            [LIGHT_ONE, -1, LIGHT_ONE + 1],
            [-7, 0, limit - 1],
        ] {
            let mut word = FILLED;
            for (channel, value) in values.into_iter().enumerate() {
                word |= pack(value) << (CHANNEL_BITS * channel as u32);
            }
            assert_ne!(word & FILLED, 0, "{values:?}: the flag");
            assert_eq!(unpack(word), values, "{values:?}");
        }
        let table = Encoding::Lab.table();
        assert_eq!(node_value(-0.25, 0, table), -16_384, "outside the gamut");
        assert_eq!(node_value(40.0, 255, table), limit - 1, "past 16");
        assert_eq!(node_value(-40.0, 0, table), -limit, "past -16");
        assert_eq!(node_value(f64::NAN, 0, table), 0, "a NaN");
        for byte in [0u8, 1, 17, 128, 254, 255] {
            let (low, high) = table.runs[usize::from(byte)];
            let held = node_value(0.5, byte, table);
            assert!(
                (i64::from(low)..=i64::from(high)).contains(&held),
                "{byte}: {held} outside {low}..={high}"
            );
            assert_eq!(table.encode(held), byte);
        }
    }

    /// **A grid exists over one, three or four inputs only** (ruling 1): a
    /// table over two or five converts through its pipeline at every
    /// conversion, as before grids, and one over one, three or four is read
    /// through a grid of the design's size whatever the profile's own grid
    /// declares.
    #[test]
    fn a_grid_is_built_over_one_three_or_four_inputs_only() {
        for inputs in 1usize..=5 {
            let clut: Vec<[u16; 3]> = (0..1usize << inputs)
                .map(|node| {
                    [
                        (node * 4_099 % 32_768) as u16,
                        (node * 9_157 % 32_768) as u16,
                        (node * 1_237 % 32_768) as u16,
                    ]
                })
                .collect();
            let space = [b'0' + inputs as u8, b'C', b'L', b'R'];
            let profile = Profile::parse(&lut_profile(&space, b"XYZ ", mft2(inputs, 2, &clut)))
                .unwrap_or_else(|e| panic!("{inputs} inputs: {e:?}"));
            let transform = Transform::compile(&profile, Intent::RelativeColorimetric)
                .unwrap_or_else(|| panic!("{inputs} inputs"));
            let x = vec![0.3; inputs];
            match grid_points(inputs) {
                Some(points) => {
                    let grid = transform.grid.as_ref().expect("a grid");
                    assert_eq!(grid.points, points, "{inputs} inputs");
                }
                None => {
                    assert!(transform.grid.is_none(), "{inputs} inputs: a grid");
                    let _ = pipeline_evaluations();
                    assert_eq!(transform.apply(&x), transform.evaluate(&x));
                    assert_eq!(
                        pipeline_evaluations(),
                        2,
                        "{inputs} inputs: each conversion runs the pipeline"
                    );
                }
            }
        }
    }

    /// **Two threads filling one grid read what one alone reads**: a node is
    /// a function of its table and position, so conversions racing to fill
    /// the same nodes, in opposite orders, return the colours a fresh copy
    /// converting alone returns.
    #[test]
    fn two_threads_filling_one_grid_read_what_one_alone_reads() {
        let profile = Profile::parse(&cmyk_table(true)).expect("a table");
        let shared =
            Transform::compile(&profile, Intent::RelativeColorimetric).expect("a transform");
        let alone = shared.clone();
        let inputs = sweep(4);
        let (forward, backward) = std::thread::scope(|scope| {
            let forward =
                scope.spawn(|| inputs.iter().map(|x| shared.apply(x)).collect::<Vec<_>>());
            let backward = scope.spawn(|| {
                inputs
                    .iter()
                    .rev()
                    .map(|x| shared.apply(x))
                    .collect::<Vec<_>>()
            });
            (
                forward.join().expect("the first thread"),
                backward.join().expect("the second thread"),
            )
        });
        let expected: Vec<_> = inputs.iter().map(|x| alone.apply(x)).collect();
        assert_eq!(forward, expected, "the first thread");
        let mut backward = backward;
        backward.reverse();
        assert_eq!(backward, expected, "the second thread");
    }

    // ---- the row path (docs/design/icc.md, P-22, "The row path") -----------

    /// A two-input table, the count no grid is built over: what a row
    /// converts a pixel at a time.
    fn two_input_table() -> Transform {
        let clut: Vec<[u16; 3]> = (0..9usize)
            .map(|node| {
                [
                    (node * 3_571 % 32_768) as u16,
                    (node * 2_903 % 32_768) as u16,
                    (node * 1_499 % 32_768) as u16,
                ]
            })
            .collect();
        let profile =
            Profile::parse(&lut_profile(b"2CLR", b"XYZ ", mft2(2, 3, &clut))).expect("a table");
        Transform::compile(&profile, Intent::RelativeColorimetric).expect("a transform")
    }

    /// `row` holds what `apply` gives each pixel of `samples`, `inputs` bytes
    /// a pixel, each component its byte over 255.
    fn row_is_apply(name: &str, transform: &Transform, samples: &[u8], row: &[u8]) {
        let inputs = transform.inputs();
        assert_eq!(
            row.len(),
            samples.len() / inputs * 3,
            "{name}: one colour a pixel"
        );
        for (pixel, got) in samples.chunks_exact(inputs).zip(row.chunks_exact(3)) {
            let x: Vec<f64> = pixel.iter().map(|&byte| f64::from(byte) / 255.0).collect();
            let (r, g, b) = transform.apply(&x);
            assert_eq!(got, [r, g, b], "{name} at {pixel:?}");
        }
    }

    /// **A row of bytes converts as `apply` does.** Every 8-bit input of each
    /// one- and three-input model P-22.1 priced, and of the one-input and
    /// two-input tables, and every input of a 17-level lattice over each
    /// four-input model, converted as rows, is the colour `apply` gives the
    /// same components: a fill whose components are `k / 255` and an image
    /// sample `k` are one colour. A partial pixel after the last whole one
    /// is passed over.
    #[test]
    fn a_row_of_bytes_converts_as_apply_does() {
        let mut models = priced_models();
        models.push(("a two-input mft2", two_input_table()));
        for (name, transform) in &models {
            let inputs = transform.inputs();
            match inputs {
                3 => {
                    for r in 0..=u8::MAX {
                        for g in 0..=u8::MAX {
                            let samples: Vec<u8> = (0..=u8::MAX).flat_map(|b| [r, g, b]).collect();
                            let mut row = Vec::new();
                            transform.convert_row(&samples, &mut row);
                            row_is_apply(name, transform, &samples, &row);
                        }
                    }
                }
                4 => {
                    let levels: Vec<u8> = (0..17u32).map(|j| ((j * 255 + 8) / 16) as u8).collect();
                    let mut samples = Vec::with_capacity(17usize.pow(4) * 4);
                    for &c in &levels {
                        for &m in &levels {
                            for &y in &levels {
                                for &k in &levels {
                                    samples.extend_from_slice(&[c, m, y, k]);
                                }
                            }
                        }
                    }
                    let mut row = Vec::new();
                    transform.convert_row(&samples, &mut row);
                    row_is_apply(name, transform, &samples, &row);
                }
                _ => {
                    // One input, or two, every byte of each.
                    let samples: Vec<u8> = if inputs == 1 {
                        (0..=u8::MAX).collect()
                    } else {
                        (0..=u8::MAX)
                            .flat_map(|a| (0..=u8::MAX).flat_map(move |b| [a, b]))
                            .collect()
                    };
                    let mut row = Vec::new();
                    transform.convert_row(&samples, &mut row);
                    row_is_apply(name, transform, &samples, &row);
                }
            }
            // A partial pixel is no pixel: the row ends at the last whole one.
            let mut samples = vec![128u8; inputs * 2];
            samples.push(7);
            let mut row = Vec::new();
            transform.convert_row(&samples, &mut row);
            assert_eq!(row.len(), 6, "{name}: two whole pixels");
        }
    }

    /// **A row reads its axis tables and no float**: a transform builds its
    /// row tables once, at its first row, and converting rows after calls
    /// `apply` not once — except where no grid or table serves the input
    /// count, which converts a pixel at a time through it. A copy builds its
    /// own, once.
    #[test]
    fn a_row_reads_its_axis_tables_and_no_float() {
        let builds = || ROW_TABLE_BUILDS.with(|count| count.replace(0));
        let applies = || APPLY_CALLS.with(|count| count.replace(0));
        let mut models = priced_models();
        models.push(("a two-input mft2", two_input_table()));
        for (name, transform) in &models {
            let inputs = transform.inputs();
            let samples: Vec<u8> = (0..=u8::MAX)
                .flat_map(|byte| (0..inputs).map(move |channel| byte ^ (channel as u8 * 85)))
                .collect();
            let pixels = samples.len() / inputs;
            let _ = (builds(), applies());
            let mut rgb = Vec::new();
            for _ in 0..3 {
                transform.convert_row(&samples, &mut rgb);
            }
            assert_eq!(builds(), 1, "{name}: row tables built once a transform");
            let expected = if transform.lut.is_some() && transform.grid.is_none() {
                3 * pixels as u64
            } else {
                0
            };
            assert_eq!(applies(), expected, "{name}: apply calls over three rows");
            assert_eq!(rgb.len(), 3 * pixels * 3, "{name}");
            let copy = transform.clone();
            copy.convert_row(&samples, &mut rgb);
            copy.convert_row(&samples, &mut rgb);
            assert_eq!(builds(), 1, "{name}: a copy builds its own, once");
            let _ = applies();
        }
    }

    /// **A set of transforms never holds more than its bound**: for a
    /// matrix/TRC, a grey, a three-intent lookup-table and a v4 profile, a
    /// CMYK table, an RGB profile whose perceptual table takes four inputs —
    /// so perceptual's and saturation's grids are larger than relative
    /// colorimetric's — one carrying a matrix, three `A2B*` tables and four
    /// `D2B*` pipelines beside a media white that is not D50, so every intent
    /// compiles its own, and CMYK profiles carrying a `B2A0` beside their
    /// `A2B0`, in `mft2` and in v4's `mBA ` and `mAB `, the bound read as the
    /// set is compiled is at least what the set holds once every intent has
    /// compiled and converted a colour and a row, its grids' nodes allocated
    /// and its row tables built, and its transform out of the connection
    /// space compiled, reading the `B2A*` table into the profile the set
    /// holds — a version 5 profile's spectral `D2B0`, reduced to XYZ, and
    /// its `c2sp` and `s2cp` among them; and the bound only falls as tables
    /// are read, staying a bound.
    #[test]
    fn transforms_hold_no_more_than_their_bound() {
        let wider = build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"A2B0", mft2(4, 2, &[[4096, 8192, 12288]; 16])),
                (*b"A2B1", mft2(3, 2, &[[8192, 4096, 2048]; 8])),
            ],
        );
        let mut tags = matrix_tags();
        tags.push((*b"wtpt", xyz(PAPER[0], PAPER[1], PAPER[2])));
        for (t, signature) in [*b"D2B0", *b"D2B1", *b"D2B2", *b"D2B3"]
            .into_iter()
            .enumerate()
        {
            tags.push((signature, Chain { t: t as f64 }.tag()));
        }
        for (t, signature) in [*b"A2B0", *b"A2B1", *b"A2B2"].into_iter().enumerate() {
            tags.push((signature, constant_table(3, fallback_value(t))));
        }
        let everything = build(b"RGB ", b"XYZ ", &tags);
        let profiles: [(&str, Vec<u8>); 10] = [
            ("matrix/TRC", matrix_trc(2.2)),
            ("grey", grey(b"XYZ ", 1.8)),
            ("three intents", three_intents_profile()),
            (
                "mAB RGB to XYZ",
                lut_profile(b"RGB ", b"XYZ ", mab_every_stage()),
            ),
            ("mft2 CMYK to Lab", cmyk_table(true)),
            ("a wider perceptual table", wider),
            ("a model, tables and pipelines", everything),
            ("a B2A table beside an A2B one", b2a_seed()),
            ("an mBA table beside an mAB one", mba_seed()),
            (
                "a spectral profile in viewing conditions of its own",
                spectral_seed(),
            ),
        ];
        for (what, bytes) in &profiles {
            let profile = Profile::parse(bytes).unwrap_or_else(|e| panic!("{what}: {e:?}"));
            let transforms =
                Transforms::compile(profile).unwrap_or_else(|| panic!("{what}: it compiles"));
            let bound = transforms.bytes_bound();
            assert!(
                transforms.held_bytes() <= bound,
                "{what}: {} held as compiled, bound {bound}",
                transforms.held_bytes()
            );
            let inputs = transforms.inputs();
            let samples: Vec<u8> = (0..inputs * 4).map(|i| (i * 61 % 256) as u8).collect();
            for intent in [
                Intent::RelativeColorimetric,
                Intent::Perceptual,
                Intent::Saturation,
                Intent::AbsoluteColorimetric,
            ] {
                let transform = transforms.get(intent);
                let _ = transform.apply(&vec![0.4; inputs]);
                let mut rgb = Vec::new();
                transform.convert_row(&samples, &mut rgb);
                // And the other direction, which reads a `B2A*` table into
                // the profile the set holds.
                let _ = Output::compile(transforms.profile(), intent);
            }
            let held = transforms.held_bytes();
            assert!(
                held <= bound,
                "{what}: {held} held once every intent compiled, its grid read and its \
                 rows built, bound {bound}"
            );
            let after = transforms.bytes_bound();
            assert!(after <= bound, "{what}: the bound only falls");
            assert!(after >= held, "{what}: and stays a bound");
        }
    }

    // ---- sixteen bits a channel (docs/design/sixteen-bit.md) ---------------

    /// A profile over each path [`Transform::apply16`] reads, by name: a
    /// matrix/TRC model and a grey curve, which build no grid; tables over
    /// one, three and four inputs, under an XYZ and a Lab connection space,
    /// read through their grids; a `D2B1` pipeline over three inputs, read
    /// through its grid, and one over two, read through its elements; and
    /// tables over two and seven inputs, which build no grid. Shared with the
    /// colour spaces' own sixteen-bit test.
    pub(crate) fn sixteen_bit_profiles() -> Vec<(&'static str, Vec<u8>)> {
        let two_inputs: Vec<[u16; 3]> = (0..9usize)
            .map(|node| {
                [
                    (node * 3_571 % 32_768) as u16,
                    (node * 2_903 % 32_768) as u16,
                    (node * 1_499 % 32_768) as u16,
                ]
            })
            .collect();
        let two_elements = mpet(
            2,
            3,
            &[matf(
                &[vec![0.5, 0.4], vec![0.3, 0.6], vec![0.1, 0.7]],
                &[0.0, 0.01, 0.0],
            )],
        );
        vec![
            ("matrix/TRC, gamma 2.2", matrix_trc(2.2)),
            ("grey under Lab, gamma 1.0", grey(b"Lab ", 1.0)),
            ("mft2 grey to XYZ", grey_table()),
            (
                "mAB RGB to XYZ",
                lut_profile(b"RGB ", b"XYZ ", mab_every_stage()),
            ),
            ("mft2 CMYK to Lab", cmyk_table(true)),
            ("mft2 CMYK to XYZ", cmyk_table(false)),
            (
                "D2B1 over three inputs",
                build(b"RGB ", b"XYZ ", &[(*b"D2B1", Chain { t: 1.0 }.tag())]),
            ),
            (
                "D2B1 over two inputs",
                build(b"2CLR", b"XYZ ", &[(*b"D2B1", two_elements)]),
            ),
            (
                "mft2 over two inputs",
                lut_profile(b"2CLR", b"XYZ ", mft2(2, 3, &two_inputs)),
            ),
            (
                "mft2 over seven inputs",
                build(b"CMYK", b"XYZ ", &[(*b"A2B0", wide_table(7))]),
            ),
        ]
    }

    /// The inputs a transform is held to its bytes over: every 8-bit value
    /// over one input, every fifteenth over two, every seventeenth over
    /// three, every fifty-first over four and nought, a half and one over
    /// more, every combination of them, the last component varying fastest.
    fn sixteen_bit_inputs(inputs: usize) -> Vec<Vec<f64>> {
        let axis: Vec<f64> = match inputs {
            1 => (0..=255u32).map(|v| f64::from(v) / 255.0).collect(),
            2 | 3 | 4 => {
                let step = [15, 17, 51][inputs - 2];
                (0..=255u32)
                    .step_by(step)
                    .map(|v| f64::from(v) / 255.0)
                    .collect()
            }
            _ => vec![0.0, 0.5, 1.0],
        };
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

    /// The top byte of each channel of a 16-bit colour.
    fn top_bytes(wide: [u16; 3]) -> (u8, u8, u8) {
        let [r, g, b] = wide.map(|channel| (channel >> 8) as u8);
        (r, g, b)
    }

    /// **A transform at sixteen bits tops out at its bytes**
    /// (`docs/design/sixteen-bit.md`, "Resolved colour"). For each profile
    /// [`sixteen_bit_profiles`] names, in every intent, every input of
    /// [`sixteen_bit_inputs`] converts through [`Transform::apply16`] to
    /// channels whose top bytes are [`Transform::apply`]'s — read from the
    /// grid where `apply` reads one, from the table, the elements or the
    /// matrix where it does not — and a component between two 8-bit values,
    /// 0.1981, does too. A grey ramp of 4 097 steps through the matrix/TRC
    /// model, the one-input grid and the two-input elements takes more
    /// 16-bit colours than 8-bit ones: the low bytes refine the bytes rather
    /// than repeat them.
    #[test]
    fn a_transform_at_sixteen_bits_tops_out_at_its_bytes() {
        for (name, bytes) in sixteen_bit_profiles() {
            let profile = Profile::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let transforms =
                Transforms::compile(profile).unwrap_or_else(|| panic!("{name}: no transforms"));
            let inputs = transforms.inputs();
            for intent in Intent::ALL {
                let transform = transforms.get(intent);
                let mut between = vec![0.1981; inputs];
                for x in sixteen_bit_inputs(inputs)
                    .into_iter()
                    .chain([between.clone()])
                {
                    assert_eq!(
                        top_bytes(transform.apply16(&x)),
                        transform.apply(&x),
                        "{name}, {intent:?}, at {x:?}"
                    );
                }
                between[0] = 0.8019;
                assert_eq!(
                    top_bytes(transform.apply16(&between)),
                    transform.apply(&between),
                    "{name}, {intent:?}, at {between:?}"
                );
            }
            if ![
                "matrix/TRC, gamma 2.2",
                "mft2 grey to XYZ",
                "D2B1 over two inputs",
            ]
            .contains(&name)
            {
                continue;
            }
            let transform = transforms.get(Intent::RelativeColorimetric);
            let mut wide = std::collections::BTreeSet::new();
            let mut narrow = std::collections::BTreeSet::new();
            for step in 0..=4096u32 {
                let x = vec![f64::from(step) / 4096.0; inputs];
                wide.insert(transform.apply16(&x));
                narrow.insert(transform.apply(&x));
            }
            assert!(
                wide.len() > narrow.len(),
                "{name}: {} colours at sixteen bits and {} at eight over a grey ramp",
                wide.len(),
                narrow.len()
            );
        }
    }

    /// An `mft2` or, `wide` false, `mft1` tag of `inputs` and `outputs` over
    /// a two-point grid, its curves the identity and its matrix `matrix`
    /// row by row, each grid entry `entry` of the 16-bit currency (an
    /// `mft1`'s its top byte). Only the header when `bare`, so a reader that
    /// gets past it finds no curve.
    fn v2_table(
        wide: bool,
        inputs: u8,
        outputs: u8,
        matrix: [f64; 9],
        entry: u16,
        bare: bool,
    ) -> Vec<u8> {
        let mut out = if wide {
            b"mft2".to_vec()
        } else {
            b"mft1".to_vec()
        };
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[inputs, outputs, 2, 0]);
        for value in matrix {
            out.extend_from_slice(&(((value * 65536.0).round() as i32) as u32).to_be_bytes());
        }
        if bare {
            return out;
        }
        let curve = |out: &mut Vec<u8>| {
            if wide {
                out.extend_from_slice(&0u16.to_be_bytes());
                out.extend_from_slice(&u16::MAX.to_be_bytes());
            } else {
                out.extend((0..=255u8).collect::<Vec<u8>>());
            }
        };
        if wide {
            out.extend_from_slice(&2u16.to_be_bytes());
            out.extend_from_slice(&2u16.to_be_bytes());
        }
        for _ in 0..inputs {
            curve(&mut out);
        }
        let nodes = 1usize << inputs;
        for _ in 0..nodes * usize::from(outputs) {
            if wide {
                out.extend_from_slice(&entry.to_be_bytes());
            } else {
                out.push((entry >> 8) as u8);
            }
        }
        for _ in 0..outputs {
            curve(&mut out);
        }
        out
    }

    /// The identity, row by row.
    const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    /// **A `B2A*` table is read as the connection space in and the data
    /// space out.** Its output count is held to [`MAX_DEVICE_CHANNELS`]
    /// before any curve is read — a header of sixteen outputs and nothing
    /// after it is too large, not short — then to the data space's channels;
    /// its input count is the connection space's three; an `mAB ` at a
    /// `B2A*` tag is refused as an `mBA ` is at an `A2B*` one. Over XYZ the
    /// table keeps a matrix that is not the identity (10.8), and over Lab,
    /// where the clause has it the identity, keeps none. Forward, a table of
    /// four outputs is still no `A2B*` table.
    #[test]
    fn a_b2a_table_is_read_as_the_connection_space_in() {
        let output = |channels| Direction::Output { channels };
        // Every entry a whole number of s15.16 steps, so it reads back as
        // itself.
        let skewed = [0.875, 0.125, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert_eq!(
            read_lut(&v2_table(true, 3, 16, IDENTITY, 0, true), false, output(4)),
            Err(IccError::TooLarge),
            "sixteen outputs, refused before a curve is looked for"
        );
        assert_eq!(
            read_lut(&v2_table(true, 3, 4, IDENTITY, 0, false), false, output(3)),
            Err(IccError::UnsupportedSpace),
            "four outputs for a three-channel space"
        );
        assert_eq!(
            read_lut(&v2_table(true, 4, 4, IDENTITY, 0, false), false, output(4)),
            Err(IccError::MalformedTag),
            "four inputs where the connection space is three"
        );
        assert_eq!(
            read_lut(&mab_b_curves_only(), false, output(3)),
            Err(IccError::NeedsLut),
            "an mAB at a B2A tag"
        );
        assert_eq!(
            read_lut(
                &v2_table(true, 3, 4, IDENTITY, 0, false),
                false,
                Direction::Forward
            ),
            Err(IccError::UnsupportedSpace),
            "an A2B table gives the connection space's three"
        );

        let xyz = read_lut(&v2_table(true, 3, 4, skewed, 0, false), false, output(4))
            .expect("a CMYK B2A table over XYZ");
        assert_eq!(xyz.input_matrix, Some(skewed));
        assert_eq!(xyz.outputs(), 4);
        let plain = read_lut(&v2_table(true, 3, 4, IDENTITY, 0, false), false, output(4))
            .expect("the identity matrix");
        assert_eq!(plain.input_matrix, None, "the identity is not kept");
        let lab = read_lut(&v2_table(false, 3, 1, skewed, 0, false), true, output(1))
            .expect("a grey B2A table over Lab");
        assert_eq!(
            lab.input_matrix, None,
            "over Lab the matrix is the identity"
        );
        let forward = read_lut(
            &v2_table(true, 3, 3, skewed, 0, false),
            false,
            Direction::Forward,
        )
        .expect("an A2B table");
        assert_eq!(forward.input_matrix, None, "an A2B table's input is no PCS");
    }

    /// **Each table type reads Lab in its own scale** (6.3.4.2): an `mft2`
    /// in version 2's legacy encoding, `L*` 100 at `FF00h` and `a*`, `b*` 0
    /// at `8000h`; an `mft1` in its own 8-bit one, `L*` 100 at `FFh` and 0 at
    /// `80h`, widened; an `mAB ` and an `mBA ` in v4's, each component's
    /// range over the whole currency, `L*` 100 at `FFFFh` and `a*`, `b*` 0 at
    /// `8080h` (10.10.1, 10.11.1). Each decodes its full scale to `L*` 100
    /// and `a*`, `b*` 0, and each encodes them back to its own; under XYZ
    /// every one takes `u1Fixed15`, 1.0 at `8000h`, an `mft2` through its
    /// matrix where one is kept and an `mBA ` through none.
    #[test]
    fn each_table_type_reads_lab_in_its_own_scale() {
        let legacy = read_lut(
            &v2_table(true, 3, 3, IDENTITY, 0, false),
            true,
            Direction::Forward,
        )
        .expect("an mft2");
        let full = read_lut(
            &v2_table(false, 3, 3, IDENTITY, 0, false),
            true,
            Direction::Forward,
        )
        .expect("an mft1");
        assert_eq!(legacy.lab_scale, LabScale::Legacy);
        assert_eq!(full.lab_scale, LabScale::Full);
        let close = |got: [f64; 3], want: [f64; 3], what: &str| {
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() < 1e-9, "{what}: {got:?} against {want:?}");
            }
        };
        let white = [100.0, 0.0, 0.0];
        close(
            legacy.decode_connection([65280.0 / 65535.0, 32768.0 / 65535.0, 32768.0 / 65535.0]),
            white,
            "mft2 decodes FF00h and 8000h",
        );
        close(
            full.decode_connection([1.0, 32896.0 / 65535.0, 32896.0 / 65535.0]),
            white,
            "mft1 decodes FFh and 80h, widened",
        );
        close(
            legacy.encode_connection(white),
            [65280.0 / 65535.0, 32768.0 / 65535.0, 32768.0 / 65535.0],
            "mft2 encodes white",
        );
        close(
            full.encode_connection(white),
            [1.0, 128.0 / 255.0, 128.0 / 255.0],
            "mft1 encodes white",
        );
        let curves_only = V4Stages {
            b: identities(3),
            ..V4Stages::default()
        };
        let mab = read_lut(&mab_b_curves_only(), true, Direction::Forward).expect("an mAB");
        let mba = read_lut(
            &curves_only.tag(b"mBA ", 3, 3),
            true,
            Direction::Output { channels: 3 },
        )
        .expect("an mBA");
        for (table, what) in [(&mab, "mAB"), (&mba, "mBA")] {
            assert_eq!(table.lab_scale, LabScale::Full, "{what}");
            close(
                table.decode_connection([1.0, 32896.0 / 65535.0, 32896.0 / 65535.0]),
                white,
                &format!("{what} decodes FFFFh and 8080h"),
            );
            close(
                table.encode_connection(white),
                [1.0, 128.0 / 255.0, 128.0 / 255.0],
                &format!("{what} encodes white"),
            );
        }

        let skewed = [0.5, 0.25, 0.0, 0.0, 1.0, 0.0, 0.25, 0.0, 0.5];
        let xyz = read_lut(
            &v2_table(true, 3, 3, skewed, 0, false),
            false,
            Direction::Output { channels: 3 },
        )
        .expect("an XYZ B2A table");
        let scale = 32768.0 / 65535.0;
        close(
            xyz.encode_connection([0.8, 0.4, 0.2]),
            [0.5 * 0.8 + 0.25 * 0.4, 0.4, 0.25 * 0.8 + 0.5 * 0.2].map(|v| v * scale),
            "XYZ through the matrix, in u1Fixed15",
        );
        // NaN is zero; the matrix makes 2.25, 9 and -0.5 of the rest, and
        // the currency holds its ends.
        close(
            xyz.encode_connection([f64::NAN, 9.0, -1.0]),
            [1.0, 1.0, 0.0],
            "NaN is zero, and the currency holds its ends",
        );
        let v4_xyz = read_lut(
            &curves_only.tag(b"mBA ", 3, 3),
            false,
            Direction::Output { channels: 3 },
        )
        .expect("an XYZ mBA");
        assert_eq!(v4_xyz.input_matrix, None, "an mBA's matrix is a stage");
        close(
            v4_xyz.encode_connection([0.8, 0.4, 0.2]),
            [0.8, 0.4, 0.2].map(|v| v * scale),
            "an mBA's XYZ in u1Fixed15",
        );
    }

    /// **A profile's connection value is its source's**: a matrix/TRC
    /// model's columns over its curves, a grey model's curve on the PCS
    /// white under XYZ and as `L*` under Lab, and a table's decoded
    /// evaluation; absolute colorimetric's scaled by the media white in XYZ,
    /// relative colorimetric's not.
    #[test]
    fn a_profiles_connection_value_is_its_sources() {
        let matrix = Profile::parse(&build(
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz(0.5, 0.25, 0.0)),
                (*b"gXYZ", xyz(0.25, 0.5, 0.25)),
                (*b"bXYZ", xyz(0.125, 0.25, 0.5)),
                (*b"rTRC", gamma(1.0)),
                (*b"gTRC", gamma(2.0)),
                (*b"bTRC", gamma(1.0)),
                (*b"wtpt", xyz(0.8, 0.9, 0.7)),
            ],
        ))
        .expect("a matrix profile");
        let value = matrix
            .connection_value(Intent::RelativeColorimetric, &[0.5, 0.5, 1.0])
            .expect("a value");
        let want = [
            0.5 * 0.5 + 0.25 * 0.25 + 0.125,
            0.25 * 0.5 + 0.5 * 0.25 + 0.25,
            0.25 * 0.25 + 0.5,
        ];
        for (got, w) in value.iter().zip(want) {
            assert!((got - w).abs() < 1e-4, "{value:?} against {want:?}");
        }
        let absolute = matrix
            .connection_value(Intent::AbsoluteColorimetric, &[0.5, 0.5, 1.0])
            .expect("a value");
        let scale = matrix.media_scale();
        for channel in 0..3 {
            assert!((absolute[channel] - value[channel] * scale[channel]).abs() < 1e-12);
        }

        let grey_xyz = Profile::parse(&build(b"GRAY", b"XYZ ", &[(*b"kTRC", gamma(1.0))]))
            .expect("a grey profile");
        assert_eq!(
            grey_xyz.connection_value(Intent::Perceptual, &[0.5]),
            Some(PCS_WHITE.map(|white| white * 0.5))
        );
        let grey_lab = Profile::parse(&build(b"GRAY", b"Lab ", &[(*b"kTRC", gamma(1.0))]))
            .expect("a grey Lab profile");
        assert_eq!(
            grey_lab.connection_value(Intent::Saturation, &[0.5]),
            Some([50.0, 0.0, 0.0])
        );

        let table = Profile::parse(&lut_profile(
            b"RGB ",
            b"Lab ",
            v2_table(false, 3, 3, IDENTITY, 0xFF00, false),
        ))
        .expect("an mft1 profile");
        let value = table
            .connection_value(Intent::RelativeColorimetric, &[0.2, 0.4, 0.6])
            .expect("a value");
        assert!(
            (value[0] - 100.0).abs() < 1e-9,
            "every node's L* is 100: {value:?}"
        );
    }

    // ---- v4 tables out of the connection space ------------------------------

    /// A `curv` of no points, ICC.1's identity (10.5).
    fn identity_curve() -> Vec<u8> {
        let mut out = b"curv".to_vec();
        out.extend_from_slice(&[0; 8]);
        out
    }

    /// `count` identity curves, a stage that is there and does nothing.
    fn identities(count: usize) -> Vec<Vec<u8>> {
        vec![identity_curve(); count]
    }

    /// A v4 lookup table's stages, as [`V4Stages::tag`] lays them out: each
    /// curve set a run of curve tags, empty where the stage is absent.
    #[derive(Clone, Default)]
    struct V4Stages {
        b: Vec<Vec<u8>>,
        /// Nine entries row by row, then three offsets.
        matrix: Option<[f64; 12]>,
        m: Vec<Vec<u8>>,
        /// One grid count an input, and the 16-bit entries, the last input's
        /// axis fastest.
        clut: Option<(Vec<u8>, Vec<u16>)>,
        a: Vec<Vec<u8>>,
    }

    impl V4Stages {
        /// The tag: `signature`'s header over `inputs` and `outputs`
        /// channels, then each stage present in the order the header lists
        /// their offsets — B, matrix, M, CLUT, A — each curve and stage
        /// padded to a four-byte boundary.
        fn tag(&self, signature: &[u8; 4], inputs: u8, outputs: u8) -> Vec<u8> {
            fn pad(tag: &mut Vec<u8>) {
                while tag.len() % 4 != 0 {
                    tag.push(0);
                }
            }
            fn run(tag: &mut Vec<u8>, curves: &[Vec<u8>]) -> u32 {
                let at = tag.len() as u32;
                for curve in curves {
                    tag.extend_from_slice(curve);
                    pad(tag);
                }
                at
            }
            let mut tag = signature.to_vec();
            tag.extend_from_slice(&[0; 4]);
            tag.extend_from_slice(&[inputs, outputs, 0, 0]);
            tag.extend_from_slice(&[0; 20]);
            let mut offsets = [0u32; 5];
            if !self.b.is_empty() {
                offsets[0] = run(&mut tag, &self.b);
            }
            if let Some(matrix) = self.matrix {
                offsets[1] = tag.len() as u32;
                for value in matrix {
                    let fixed = (value * 65536.0).round() as i32;
                    tag.extend_from_slice(&(fixed as u32).to_be_bytes());
                }
            }
            if !self.m.is_empty() {
                offsets[2] = run(&mut tag, &self.m);
            }
            if let Some((grids, entries)) = &self.clut {
                offsets[3] = tag.len() as u32;
                let mut counts = [0u8; 16];
                for (slot, count) in counts.iter_mut().zip(grids) {
                    *slot = *count;
                }
                tag.extend_from_slice(&counts);
                // Two bytes an entry, then three of padding.
                tag.extend_from_slice(&[2, 0, 0, 0]);
                for entry in entries {
                    tag.extend_from_slice(&entry.to_be_bytes());
                }
                pad(&mut tag);
            }
            if !self.a.is_empty() {
                offsets[4] = run(&mut tag, &self.a);
            }
            for (slot, offset) in offsets.iter().enumerate() {
                tag[12 + 4 * slot..16 + 4 * slot].copy_from_slice(&offset.to_be_bytes());
            }
            tag
        }
    }

    /// The offset `tag`'s header gives stage `slot`, in its order: B, matrix,
    /// M, CLUT, A.
    fn stage_offset(tag: &[u8], slot: usize) -> usize {
        let at = 12 + 4 * slot;
        u32::from_be_bytes([tag[at], tag[at + 1], tag[at + 2], tag[at + 3]]) as usize
    }

    /// **An `mBA ` is read as the connection space in**, as an `mft2` at a
    /// `B2A*` tag is: its output count held to [`MAX_DEVICE_CHANNELS`]
    /// before any stage is read — a header of sixteen outputs and nothing
    /// after it is too large, not short — then to the data space's channels,
    /// its input count to the connection space's three. Its B curves are
    /// required, a tag with no grid has as many outputs as inputs, and a
    /// stage whose bytes are not all there — curves past the end, a grid of
    /// one point or of three bytes an entry, a matrix cut short — is
    /// malformed. At an `A2B*` tag it is refused by name. An `mBA ` of B
    /// curves alone reads, its matrix and M curves on the grid's input side,
    /// and passes the connection space through.
    #[test]
    fn an_mba_tag_is_read_as_the_connection_space_in() {
        let read = |tag: &[u8], channels| read_lut(tag, false, Direction::Output { channels });
        let curves_only = V4Stages {
            b: identities(3),
            ..V4Stages::default()
        };
        let every = V4Stages {
            b: identities(3),
            matrix: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
            m: identities(3),
            clut: Some((vec![2, 2, 2], vec![0; 8 * 4])),
            a: identities(4),
        };
        let mut header_only = curves_only.tag(b"mBA ", 3, 16);
        header_only.truncate(32);
        let no_grid = V4Stages {
            clut: None,
            a: Vec::new(),
            ..every.clone()
        };
        for (tag, channels, expected, what) in [
            (
                header_only,
                4,
                IccError::TooLarge,
                "sixteen outputs, refused before a stage is read",
            ),
            (
                every.tag(b"mBA ", 3, 4),
                3,
                IccError::UnsupportedSpace,
                "four outputs for a three-channel space",
            ),
            (
                every.tag(b"mBA ", 4, 4),
                4,
                IccError::MalformedTag,
                "four inputs where the connection space is three",
            ),
            (
                every.tag(b"mBA ", 0, 4),
                4,
                IccError::MalformedTag,
                "no inputs",
            ),
            (
                V4Stages::default().tag(b"mBA ", 3, 3),
                3,
                IccError::MalformedTag,
                "no B curves",
            ),
            (
                no_grid.tag(b"mBA ", 3, 4),
                4,
                IccError::UnsupportedSpace,
                "no grid between three inputs and four outputs",
            ),
        ] {
            assert_eq!(read(&tag, channels), Err(expected), "{what}");
        }

        let whole = every.tag(b"mBA ", 3, 4);
        assert!(read(&whole, 4).is_ok(), "every stage, well formed");
        let mut past_the_end = whole.clone();
        past_the_end[12..16].copy_from_slice(&(whole.len() as u32).to_be_bytes());
        let mut one_point = whole.clone();
        one_point[stage_offset(&whole, 3)] = 1;
        let mut three_bytes = whole.clone();
        three_bytes[stage_offset(&whole, 3) + 16] = 3;
        let mut short_matrix = whole.clone();
        short_matrix[16..20].copy_from_slice(&((whole.len() - 8) as u32).to_be_bytes());
        for (tag, what) in [
            (past_the_end, "B curves past the end"),
            (one_point, "a grid of one point"),
            (three_bytes, "three bytes a grid entry"),
            (short_matrix, "a matrix cut short"),
        ] {
            assert_eq!(read(&tag, 4), Err(IccError::MalformedTag), "{what}");
        }
        assert_eq!(
            read_lut(&whole, false, Direction::Forward),
            Err(IccError::NeedsLut),
            "an mBA at an A2B tag"
        );

        let lut = read(&curves_only.tag(b"mBA ", 3, 3), 3).expect("B curves alone");
        assert_eq!(lut.matrix_stage, MatrixStage::BeforeGrid);
        assert_eq!(lut.lab_scale, LabScale::Full);
        assert_eq!((lut.matrix, lut.m_tables.is_none()), (None, true));
        assert!(lut.clut.is_empty());
        assert_eq!(lut.outputs(), 3);
        let x = [0.25, 0.5, 0.75];
        let got = lut.evaluate(&x);
        for channel in 0..3 {
            assert!(
                (got[channel] - x[channel]).abs() < 2.0 / 65535.0,
                "identity B curves pass the value through: {got:?}"
            );
        }
    }

    /// **An `mBA ` runs its matrix and M curves before its grid** (10.11).
    /// B curves of gamma 2; a matrix that turns the channels round with an
    /// offset on two of them, the third's result below zero and clipped to
    /// it (10.11.3); M curves of three points bent at their middle; and no
    /// grid, so the identity A curves its absence leaves: each channel is
    /// what those three stages give it in that order, within five units of
    /// the 16-bit currency for the roundings of four sampled curves. With a
    /// grid of four outputs between identity B curves and A curves of gamma
    /// 1, 2, 1 and 2, the centre of the connection space's cube reads the
    /// grid's centre node, each output through its own A curve.
    #[test]
    fn an_mba_runs_its_matrix_and_m_curves_before_its_grid() {
        let bent = |middle: u16| {
            let mut out = b"curv".to_vec();
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&3u32.to_be_bytes());
            for point in [0, middle, u16::MAX] {
                out.extend_from_slice(&point.to_be_bytes());
            }
            out
        };
        let stages = V4Stages {
            b: vec![gamma(2.0); 3],
            matrix: Some([0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.25, 0.0, -0.5]),
            m: vec![bent(0x4000); 3],
            ..V4Stages::default()
        };
        let lut = read_lut(
            &stages.tag(b"mBA ", 3, 3),
            false,
            Direction::Output { channels: 3 },
        )
        .expect("B curves, a matrix and M curves");
        assert_eq!(lut.matrix_stage, MatrixStage::BeforeGrid);
        let x: [f64; 3] = [0.4, 0.6, 0.9];
        let b = x.map(|v| v * v);
        let turned = [b[1] + 0.25, b[2], (b[0] - 0.5).max(0.0)];
        let quarter = 16384.0 / 65535.0;
        let want = turned.map(|t| {
            if t <= 0.5 {
                t * 2.0 * quarter
            } else {
                quarter + (t - 0.5) * 2.0 * (1.0 - quarter)
            }
        });
        let got = lut.evaluate(&x);
        for channel in 0..3 {
            assert!(
                (got[channel] - want[channel]).abs() < 5.0 / 65535.0,
                "channel {channel}: {got:?} against {want:?}"
            );
        }

        let centre = 13;
        let node: [u16; 4] = [0x4000, 0x8000, 0xC000, 0xFFFF];
        let mut entries = vec![0u16; 27 * 4];
        entries[centre * 4..centre * 4 + 4].copy_from_slice(&node);
        let gridded = V4Stages {
            b: identities(3),
            clut: Some((vec![3, 3, 3], entries)),
            a: vec![gamma(1.0), gamma(2.0), gamma(1.0), gamma(2.0)],
            ..V4Stages::default()
        };
        let lut = read_lut(
            &gridded.tag(b"mBA ", 3, 4),
            false,
            Direction::Output { channels: 4 },
        )
        .expect("B curves, a grid and A curves");
        assert_eq!(lut.outputs(), 4);
        let got = lut.evaluate(&[0.5, 0.5, 0.5]);
        let unit = node.map(|value| f64::from(value) / 65535.0);
        let want = [unit[0], unit[1] * unit[1], unit[2], unit[3] * unit[3]];
        for channel in 0..4 {
            assert!(
                (got[channel] - want[channel]).abs() < 3.0 / 65535.0,
                "channel {channel}: {got:?} against {want:?}"
            );
        }
    }

    // ---- the gamut tag -----------------------------------------------------

    /// **A gamut tag is read beside the output tables** (ICC.1 9.2.28): kept
    /// unread at parse, read when a transform out of the connection space
    /// first asks, from the connection space to one channel. Each table
    /// type says the precision its answer is read in: 8 bits for an `mft1`
    /// and an `mBA ` whose grid holds a byte an entry, 16 for an `mft2` and
    /// an `mBA ` of two. The seed's corners holding at most one full
    /// component answer in gamut and the rest out; a `gamt` of three outputs
    /// does not read and answers none, its transform compiled all the same;
    /// a profile of tables alone carrying only a `gamt` has no transform out
    /// of the connection space for it to stand beside; and the tag is
    /// counted in the profile's bound, read or not.
    #[test]
    fn a_gamut_tag_is_read_beside_the_output_tables() {
        let profile = Profile::parse(&gamut_seed()).expect("the gamut seed parses");
        let gamut = profile.tables.gamut.as_ref().expect("a gamt element");
        assert!(gamut.read.get().is_none(), "unread at parse");
        let bound = profile.tables.held_bound();
        let output = Output::compile(&profile, Intent::RelativeColorimetric).expect("a B2A0");
        assert!(
            profile
                .tables
                .gamut
                .as_ref()
                .and_then(|g| g.read.get())
                .is_some(),
            "read by the transform out of the connection space"
        );
        assert!(
            profile.tables.held_bound() <= bound,
            "the bound only falls as it is read"
        );
        // The legacy Lab encoding (10.8), whose full currency is 65 535 over
        // 65 280 of the space: each corner of the encoded cube in `L*a*b*`.
        let reach = 65535.0 / 65280.0;
        let corner = |bits: [u8; 3]| -> [f64; 3] {
            let [l, a, b] = bits.map(f64::from);
            [
                l * 100.0 * reach,
                a * 255.0 * reach - 128.0,
                b * 255.0 * reach - 128.0,
            ]
        };
        for node in 0..8usize {
            let bits = [
                ((node >> 2) & 1) as u8,
                ((node >> 1) & 1) as u8,
                (node & 1) as u8,
            ];
            let pcs = corner(bits);
            let want = node.count_ones() <= 1;
            assert_eq!(output.in_gamut(pcs), Some(want), "corner {node}: {pcs:?}");
        }

        for (tag, bits) in [
            (graded_mft2(3, 1, |_, _| 0), 16),
            (
                {
                    let mut eight = b"mft1".to_vec();
                    eight.extend_from_slice(&[0; 4]);
                    eight.extend_from_slice(&[3, 1, 2, 0]);
                    for entry in [1i32, 0, 0, 0, 1, 0, 0, 0, 1] {
                        eight.extend_from_slice(&(entry << 16).to_be_bytes());
                    }
                    for _ in 0..3 {
                        eight.extend((0..=255u8).collect::<Vec<u8>>());
                    }
                    eight.extend_from_slice(&[0; 8]);
                    eight.extend((0..=255u8).collect::<Vec<u8>>());
                    eight
                },
                8,
            ),
            (
                V4Stages {
                    b: identities(3),
                    clut: Some((vec![2, 2, 2], vec![0; 8])),
                    ..V4Stages::default()
                }
                .tag(b"mBA ", 3, 1),
                16,
            ),
        ] {
            let lut = read_lut(&tag, true, Direction::Output { channels: 1 })
                .unwrap_or_else(|error| panic!("{bits} bits: {error:?}"));
            assert_eq!(lut.bits, bits);
        }
        let mut byte_grid = V4Stages {
            b: identities(3),
            clut: Some((vec![2, 2, 2], vec![0; 8])),
            ..V4Stages::default()
        }
        .tag(b"mBA ", 3, 1);
        let at = stage_offset(&byte_grid, 3);
        byte_grid[at + 16] = 1;
        let lut = read_lut(&byte_grid, true, Direction::Output { channels: 1 })
            .expect("a grid of a byte an entry");
        assert_eq!(lut.bits, 8, "an mBA whose grid holds a byte an entry");

        // Three outputs: the tag does not read, and answers none.
        let broken = build(
            b"CMYK",
            b"Lab ",
            &[
                (*b"A2B0", graded_mft2(4, 3, |_, _| 0x8000)),
                (*b"B2A0", graded_mft2(3, 4, |_, _| 0)),
                (*b"gamt", graded_mft2(3, 3, |_, _| 0)),
            ],
        );
        let broken = Profile::parse(&broken).expect("the tables read");
        let output = Output::compile(&broken, Intent::Perceptual).expect("a B2A0");
        assert_eq!(output.in_gamut([50.0, 0.0, 0.0]), None);

        // A gamut tag alone stands beside nothing.
        let alone = build(
            b"CMYK",
            b"Lab ",
            &[
                (*b"A2B0", graded_mft2(4, 3, |_, _| 0x8000)),
                (*b"gamt", graded_mft2(3, 1, |_, _| 0)),
            ],
        );
        let alone = Profile::parse(&alone).expect("the A2B0 reads");
        assert_eq!(
            Output::compile(&alone, Intent::RelativeColorimetric),
            Err(IccError::MissingTags)
        );
    }

    // ---- multi-process tags out of the connection space, and version 5 -----

    /// An `mpet` of three inputs whose one matrix element gives every one of
    /// its `outputs` channels `value`, whatever comes in.
    fn constant_elements(outputs: usize, value: f64) -> Vec<u8> {
        mpet(
            3,
            outputs as u16,
            &[matf(&vec![vec![0.0; 3]; outputs], &vec![value; outputs])],
        )
    }

    /// An `mpet` whose one matrix element passes its three channels through.
    fn identity_elements() -> Vec<u8> {
        mpet(
            3,
            3,
            &[matf(
                &[
                    vec![1.0, 0.0, 0.0],
                    vec![0.0, 1.0, 0.0],
                    vec![0.0, 0.0, 1.0],
                ],
                &[0.0; 3],
            )],
        )
    }

    /// An `mft2` out of the connection space over a two-point grid whose
    /// nodes are its corners: each output its input's coordinate, so XYZ in
    /// `u1Fixed15` comes out as half itself.
    fn identity_table() -> Vec<u8> {
        graded_mft2(3, 3, |node, channel| {
            if (node >> (2 - channel)) & 1 == 1 {
                0xFFFF
            } else {
                0
            }
        })
    }

    /// How far [`identity_table`] reads from its exact output: a two-point
    /// grid is interpolated in 256ths of its one cell, and two units for the
    /// roundings. The media white moves a value by a tenth, far past it.
    const TABLE_STEP: f64 = 65535.0 / 256.0 + 2.0;

    /// The constants [`constant_elements`] give each tag of a number here.
    const ELEMENT_VALUES: [f64; 4] = [0.125, 0.25, 0.5, 0.75];

    /// **A `B2D*` tag converts out of the connection space before its `B2A*`
    /// table** (ICC.1 8.10.2 a), with `B` for `A` and `D`): each intent
    /// through its own `B2D*` tag where the profile carries one, before its
    /// `B2A*` table and its model; absolute colorimetric through `B2D3` as it
    /// stands, its value not divided by the media white, and through `B2A1`
    /// with it divided where there is no `B2D3`; a `B2D*` tag that does not
    /// read passed over to the table; and a version 5 profile's `B2D*` tags,
    /// a spectral connection space's (ICC.2 9.2.42), not read at all.
    #[test]
    fn a_b2d_tag_converts_out_of_the_connection_space_before_its_table() {
        let mut tags = matrix_tags();
        tags.push((*b"wtpt", xyz(PAPER[0], PAPER[1], PAPER[2])));
        for (tag, signature) in [*b"B2D0", *b"B2D1", *b"B2D2"].into_iter().enumerate() {
            tags.push((signature, constant_elements(3, ELEMENT_VALUES[tag])));
        }
        tags.push((*b"B2D3", identity_elements()));
        tags.push((*b"B2A1", identity_table()));
        let profile = Profile::parse(&build(b"RGB ", b"XYZ ", &tags)).expect("a profile");
        let pcs = [0.3, 0.4, 0.2];
        let through = |profile: &Profile, intent: Intent| {
            let output = Output::compile(profile, intent).expect("a transform out");
            output.apply(pcs).channels().to_vec()
        };
        for (tag, intent) in [
            (0, Intent::Perceptual),
            (1, Intent::RelativeColorimetric),
            (2, Intent::Saturation),
        ] {
            let want = (ELEMENT_VALUES[tag] * 65535.0).round() as u16;
            for value in through(&profile, intent) {
                assert!(value.abs_diff(want) <= 1, "{intent:?}: B2D{tag}, {value}");
            }
        }
        // `B2D3` is ICC-absolute already: the value goes in as it stands.
        let absolute = through(&profile, Intent::AbsoluteColorimetric);
        for (value, want) in absolute.iter().zip(pcs) {
            assert!(
                (f64::from(*value) - want * 65535.0).abs() <= 2.0,
                "B2D3, undivided: {absolute:?}"
            );
        }

        // No `B2D3`: absolute colorimetric through `B2A1`, divided by the
        // media white over the PCS white first.
        let without: Vec<([u8; 4], Vec<u8>)> = tags
            .iter()
            .filter(|(signature, _)| signature != b"B2D3")
            .cloned()
            .collect();
        let profile = Profile::parse(&build(b"RGB ", b"XYZ ", &without)).expect("a profile");
        let scale = closed_scale(PAPER);
        let got = through(&profile, Intent::AbsoluteColorimetric);
        for (channel, value) in got.iter().enumerate() {
            let want = pcs[channel] / scale[channel] * 32768.0;
            assert!(
                (f64::from(*value) - want).abs() <= TABLE_STEP,
                "B2A1, divided: {got:?}, channel {channel} {want}"
            );
        }

        // A `B2D1` that does not read: the table.
        let mut broken = tags.clone();
        broken.retain(|(signature, _)| signature != b"B2D1");
        broken.push((*b"B2D1", mpet(3, 3, &[element_of(b"JtoX", 3, 3, &[0; 16])])));
        let profile = Profile::parse(&build(b"RGB ", b"XYZ ", &broken)).expect("a profile");
        let got = through(&profile, Intent::RelativeColorimetric);
        for (channel, value) in got.iter().enumerate() {
            let want = pcs[channel] * 32768.0;
            assert!(
                (f64::from(*value) - want).abs() <= TABLE_STEP,
                "passed over to B2A1: {got:?}"
            );
        }

        // A version 5 profile's `B2D*` tags take spectra: not read.
        let mut v5 = build(b"RGB ", b"XYZ ", &tags);
        v5[8..12].copy_from_slice(&0x0500_0000u32.to_be_bytes());
        let profile = Profile::parse(&v5).expect("a version 5 profile");
        assert!(profile.tables.output_pipelines.is_empty());
        let got = through(&profile, Intent::RelativeColorimetric);
        for (channel, value) in got.iter().enumerate() {
            assert!(
                (f64::from(*value) - pcs[channel] * 32768.0).abs() <= TABLE_STEP,
                "through B2A1: {got:?}"
            );
        }
    }

    /// The bytes of a version 5 profile of `tags`, its header's fields past
    /// byte 100 zero.
    fn version5(space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = build(space, b"XYZ ", tags);
        out[8..12].copy_from_slice(&0x0500_0000u32.to_be_bytes());
        out
    }

    /// **A version 5 profile's `A2B3` is absolute colorimetric's own**
    /// (ICC.2 9.2.4, 8.14.2): its `A2B*` and `B2A*` tags may hold elements,
    /// which read as `D2B*` tags do; absolute colorimetric converts through
    /// `A2B3` as it stands, before `A2B1`, and relative colorimetric through
    /// `A2B1`; a profile whose only table is `A2B3` converts every other
    /// intent through it, the media white divided out to find the relative
    /// value (9.2.93), and its transform out of the connection space through
    /// `B2A3` with the white multiplied in. A version 2 or 4 profile's `A2B3`
    /// is no tag ICC.1 defines and is not read. The header's version 5
    /// fields read as they stand, a version 2 profile's not at all.
    #[test]
    fn a_version_5_profiles_a2b3_is_absolute_colorimetrics_own() {
        let mut tags = vec![(*b"wtpt", xyz(PAPER[0], PAPER[1], PAPER[2]))];
        for (tag, signature) in [*b"A2B0", *b"A2B1", *b"A2B2", *b"A2B3"]
            .into_iter()
            .enumerate()
        {
            tags.push((signature, constant_elements(3, ELEMENT_VALUES[tag])));
        }
        let profile = Profile::parse(&version5(b"RGB ", &tags)).expect("a version 5 profile");
        assert_eq!(
            profile.version5.map(|header| header.spectral_pcs),
            Some([0; 4])
        );
        let value = |profile: &Profile, intent: Intent| {
            profile
                .connection_value(intent, &[0.5; 3])
                .expect("a value")
        };
        for (tag, intent) in [
            (0, Intent::Perceptual),
            (1, Intent::RelativeColorimetric),
            (2, Intent::Saturation),
            (3, Intent::AbsoluteColorimetric),
        ] {
            let got = value(&profile, intent);
            for channel in got {
                assert!(
                    (channel - ELEMENT_VALUES[tag]).abs() < 1e-4,
                    "{intent:?} through A2B{tag}, unscaled: {got:?}"
                );
            }
            assert!(
                Transform::compile(&profile, intent)
                    .is_some_and(|transform| transform.media.is_none()),
                "{intent:?}"
            );
        }

        // `A2B3` alone: every intent through it, the others scaled back.
        let alone = vec![
            tags[0].clone(),
            (*b"A2B3", constant_elements(3, 0.5)),
            (*b"B2A3", identity_elements()),
        ];
        let profile = Profile::parse(&version5(b"RGB ", &alone)).expect("A2B3 alone");
        let scale = closed_scale(PAPER);
        let absolute = value(&profile, Intent::AbsoluteColorimetric);
        let relative = value(&profile, Intent::RelativeColorimetric);
        for channel in 0..3 {
            assert!((absolute[channel] - 0.5).abs() < 1e-4, "{absolute:?}");
            assert!(
                (relative[channel] - 0.5 / scale[channel]).abs() < 1e-4,
                "relative is the absolute value over the media white: {relative:?}"
            );
        }
        let transforms = Transforms::compile(profile.clone()).expect("transforms");
        assert!(
            transforms.relative.media.is_some(),
            "relative colorimetric's is scaled"
        );
        assert!(
            transforms.get(Intent::AbsoluteColorimetric).media.is_none(),
            "absolute colorimetric's shares nothing with it"
        );
        let out = Output::compile(&profile, Intent::RelativeColorimetric).expect("B2A3");
        let got = out.apply([0.4, 0.4, 0.4]).channels().to_vec();
        for (channel, value) in got.iter().enumerate() {
            let want = (0.4 * scale[channel]).clamp(0.0, 1.0) * 65535.0;
            assert!(
                (f64::from(*value) - want).abs() < 4.0,
                "the media white multiplied in before B2A3: {got:?}"
            );
        }

        // In a version 4 profile `A2B3` is nothing ICC.1 defines: a profile
        // whose only table it is has none.
        let mut v4 = build(b"RGB ", b"XYZ ", &alone);
        v4[8..12].copy_from_slice(&0x0400_0000u32.to_be_bytes());
        assert_eq!(Profile::parse(&v4), Err(IccError::MissingTags));
        let v2 = Profile::parse(&build(b"RGB ", b"XYZ ", &matrix_tags())).expect("v2");
        assert_eq!(v2.version5, None);
    }

    // ---- soft proofing (RC-05.2) -------------------------------------------

    /// The proof transform held to ICC.1's equations: three proof profiles
    /// built here, each leg of each recomputed in `f64` — curves and matrices
    /// in closed form, a lookup table at its grid and between its nodes by
    /// this module's own multilinear interpolation, which pins the engine's
    /// interpolation (`docs/design/icc.md`, "Verification") — and each sample
    /// held to the bound the tables the proof compiles give it
    /// (`docs/design/icc.md`, "The proof's integer form, its tables, and the
    /// bound").
    mod proofs {
        use super::*;

        /// The paper the proof profiles print on: a warm white, below the
        /// PCS white, so ICC-absolute colorimetry shows it.
        const PAPER: [f64; 3] = [0.93, 0.96, 0.79];

        /// The most a D50 XYZ component of an sRGB colour is off after the
        /// device leg's first stages, in XYZ: half a step of s15.16 for each
        /// decode-table entry, carried through a row of `SRGB_TO_XYZ_D50`
        /// (each row sums to at most one), half a step for each of the row's
        /// three s15.16 entries times light of at most one, and half a step
        /// for the sum's rounding.
        const XYZ_ERROR: f64 = (0.5 + 1.5 + 0.5) / 65_536.0;

        /// Entries a compiled tone curve and its inverse carry.
        const ENTRIES: f64 = CURVE_ENTRIES as f64;

        /// A `curv` of `points`, spaced evenly over `0..=1`.
        fn sampled_curve(points: &[u16]) -> Vec<u8> {
            let mut out = b"curv".to_vec();
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&(points.len() as u32).to_be_bytes());
            for point in points {
                out.extend_from_slice(&point.to_be_bytes());
            }
            out
        }

        /// An `mft2` of `inputs` to `outputs` over `grid` points an axis,
        /// its matrix and curves the identity, each node — the first input's
        /// axis slowest, its coordinates `i / (grid - 1)` — the values
        /// `value` gives its coordinates, each in `0..=1` of the currency;
        /// and the grid as stored.
        fn lut16(
            inputs: usize,
            outputs: usize,
            grid: usize,
            value: impl Fn(&[f64]) -> Vec<f64>,
        ) -> (Vec<u8>, Vec<u16>) {
            let mut tag = b"mft2".to_vec();
            tag.extend_from_slice(&[0; 4]);
            tag.extend_from_slice(&[inputs as u8, outputs as u8, grid as u8, 0]);
            for row in 0..3 {
                for column in 0..3 {
                    let entry: u32 = if row == column { 0x0001_0000 } else { 0 };
                    tag.extend_from_slice(&entry.to_be_bytes());
                }
            }
            tag.extend_from_slice(&2u16.to_be_bytes());
            tag.extend_from_slice(&2u16.to_be_bytes());
            for _ in 0..inputs {
                tag.extend_from_slice(&0u16.to_be_bytes());
                tag.extend_from_slice(&u16::MAX.to_be_bytes());
            }
            let mut clut = Vec::new();
            for node in 0..grid.pow(inputs as u32) {
                let mut coordinates = vec![0.0; inputs];
                let mut rest = node;
                for slot in coordinates.iter_mut().rev() {
                    *slot = (rest % grid) as f64 / (grid - 1) as f64;
                    rest /= grid;
                }
                let values = value(&coordinates);
                for value in values.iter().take(outputs) {
                    let stored = (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
                    clut.push(stored);
                    tag.extend_from_slice(&stored.to_be_bytes());
                }
            }
            for _ in 0..outputs {
                tag.extend_from_slice(&0u16.to_be_bytes());
                tag.extend_from_slice(&u16::MAX.to_be_bytes());
            }
            (tag, clut)
        }

        /// A grid as stored, and its shape, read by this module's own
        /// multilinear interpolation.
        struct Grid {
            clut: Vec<u16>,
            inputs: usize,
            outputs: usize,
            points: usize,
        }

        impl Grid {
            /// Channel `channel` at `x`, each coordinate held to `0..=1`,
            /// over `0..=1` of the currency: the corners of the cell `x`
            /// falls in, each weighted by the product of its fractions.
            fn at(&self, x: &[f64], channel: usize) -> f64 {
                let last = (self.points - 1) as f64;
                let mut base = vec![0usize; self.inputs];
                let mut fraction = vec![0.0; self.inputs];
                for axis in 0..self.inputs {
                    let position = x[axis].clamp(0.0, 1.0) * last;
                    let index = (position.floor() as usize).min(self.points - 2);
                    base[axis] = index;
                    fraction[axis] = position - index as f64;
                }
                let mut sum = 0.0;
                for corner in 0..1usize << self.inputs {
                    let mut weight = 1.0;
                    let mut offset = 0usize;
                    for axis in 0..self.inputs {
                        let high = (corner >> (self.inputs - 1 - axis)) & 1 == 1;
                        weight *= if high {
                            fraction[axis]
                        } else {
                            1.0 - fraction[axis]
                        };
                        offset = offset * self.points + base[axis] + usize::from(high);
                    }
                    let node = self.clut[offset * self.outputs + channel];
                    sum += weight * f64::from(node) / 65535.0;
                }
                sum
            }
        }

        /// The RGB proof: a wide-gamut display's columns, a gamma of 2.2 for
        /// red, sRGB's own `para` (function type 3) for green and five
        /// samples for blue, printed on [`PAPER`].
        fn rgb_proof() -> Vec<u8> {
            build(
                b"RGB ",
                b"XYZ ",
                &[
                    (*b"rXYZ", xyz(0.6097, 0.3111, 0.0195)),
                    (*b"gXYZ", xyz(0.2053, 0.6257, 0.0609)),
                    (*b"bXYZ", xyz(0.1492, 0.0632, 0.7446)),
                    (*b"rTRC", gamma(2.2)),
                    (*b"gTRC", srgb_curve()),
                    (*b"bTRC", sampled_curve(&[0, 4_000, 15_000, 34_000, 65_535])),
                    (*b"wtpt", xyz(PAPER[0], PAPER[1], PAPER[2])),
                ],
            )
        }

        /// The grey proof: one gamma of 1.8 into XYZ, on [`PAPER`].
        fn grey_proof() -> Vec<u8> {
            build(
                b"GRAY",
                b"XYZ ",
                &[
                    (*b"kTRC", gamma(1.8)),
                    (*b"wtpt", xyz(PAPER[0], PAPER[1], PAPER[2])),
                ],
            )
        }

        /// The CMYK proof's `A2B0` at inks `ink`: the light each ink leaves,
        /// scaled by what the black leaves, as a lightness and two opponent
        /// axes, in `mft2`'s legacy Lab encoding over `0..=1` of the
        /// currency.
        fn ink_lab(ink: &[f64]) -> Vec<f64> {
            let black = 1.0 - 0.92 * ink[3];
            let red = (1.0 - 0.85 * ink[0]) * black;
            let green = (1.0 - 0.8 * ink[1]) * black;
            let blue = (1.0 - 0.75 * ink[2]) * black;
            let l = 100.0 * (0.3 * red + 0.59 * green + 0.11 * blue);
            let a = 90.0 * (red - green);
            let b = 70.0 * (0.5 * (red + green) - blue);
            let legacy = 65280.0 / 65535.0;
            vec![
                l / 100.0 * legacy,
                (a + 128.0) / 255.0 * legacy,
                (b + 128.0) / 255.0 * legacy,
            ]
        }

        /// Table `table`'s inks at the encoded Lab coordinates `p`: each ink
        /// rising with the darkness and leaning on its own opponent axis, the
        /// three tables a little apart so each intent answers its own.
        fn separate(p: &[f64], table: f64) -> Vec<f64> {
            let dark = 1.0 - p[0];
            vec![
                (dark * (1.1 - p[1]) * 0.9 + 0.02 * table).clamp(0.0, 1.0),
                (dark * (0.4 + p[1]) * 0.8).clamp(0.0, 1.0),
                (dark * (0.3 + p[2]) * 0.85).clamp(0.0, 1.0),
                (dark * dark * 0.9 - 0.01 * table).clamp(0.0, 1.0),
            ]
        }

        /// The CMYK proof over Lab — an `A2B0` of six points an axis, so a
        /// node every 51st byte, and a `B2A0`, `B2A1` and `B2A2` of nine
        /// points an axis, one each intent, printed on [`PAPER`] — and its
        /// grids: the `A2B0`'s, then the three `B2A*`s'.
        fn cmyk_proof() -> (Vec<u8>, Grid, [Grid; 3]) {
            let (a2b0, forward) = lut16(4, 3, 6, ink_lab);
            let outputs: Vec<(Vec<u8>, Vec<u16>)> = (0..3u8)
                .map(|table| lut16(3, 4, 9, |p| separate(p, f64::from(table))))
                .collect();
            let bytes = build_as(
                b"prtr",
                b"CMYK",
                b"Lab ",
                &[
                    (*b"A2B0", a2b0),
                    (*b"B2A0", outputs[0].0.clone()),
                    (*b"B2A1", outputs[1].0.clone()),
                    (*b"B2A2", outputs[2].0.clone()),
                    (*b"wtpt", xyz(PAPER[0], PAPER[1], PAPER[2])),
                ],
            );
            let grid = |clut: Vec<u16>, inputs: usize, outputs: usize, points: usize| Grid {
                clut,
                inputs,
                outputs,
                points,
            };
            let [b2a0, b2a1, b2a2] =
                [0usize, 1, 2].map(|table| grid(outputs[table].1.clone(), 3, 4, 9));
            (bytes, grid(forward, 4, 3, 6), [b2a0, b2a1, b2a2])
        }

        // ---- ICC.1's equations, in `f64` ----------------------------------

        /// IEC 61966-2-1's decode of an encoded value in `0..=1`.
        fn decode(encoded: f64) -> f64 {
            if encoded <= 0.040_45 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            }
        }

        /// IEC 61966-2-1's encode of linear light, held to `0..=1`.
        fn encode(linear: f64) -> f64 {
            let v = linear.clamp(0.0, 1.0);
            if v <= 0.003_130_8 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        }

        /// An sRGB colour, each channel encoded in `0..=1`, as D50 XYZ.
        fn srgb_xyz(rgb: [f64; 3]) -> [f64; 3] {
            let linear = rgb.map(decode);
            crate::SRGB_TO_XYZ_D50.map(|row| row.iter().zip(linear).map(|(m, v)| m * v).sum())
        }

        /// D50 XYZ as encoded sRGB in `0..=1`.
        fn xyz_srgb(xyz: [f64; 3]) -> [f64; 3] {
            crate::XYZ_D50_TO_SRGB.map(|row| encode(row.iter().zip(xyz).map(|(m, v)| m * v).sum()))
        }

        /// The D50 white the Lab relation reads against.
        fn lab_white() -> [f64; 3] {
            crate::lab_to_xyz(100.0, 0.0, 0.0)
        }

        /// D50 XYZ as CIE L*a*b* (8.6.5.4), the cube root the platform's.
        fn lab_of(xyz: [f64; 3]) -> [f64; 3] {
            let white = lab_white();
            let delta = 6.0f64 / 29.0;
            let f = |t: f64| {
                if t > delta * delta * delta {
                    t.cbrt()
                } else {
                    t / (3.0 * delta * delta) + 4.0 / 29.0
                }
            };
            let [fx, fy, fz] = [0, 1, 2].map(|c| f(xyz[c] / white[c]));
            [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
        }

        /// CIE L*a*b* as D50 XYZ.
        fn xyz_of(lab: [f64; 3]) -> [f64; 3] {
            let white = lab_white();
            let delta = 6.0f64 / 29.0;
            let finv = |t: f64| {
                if t > delta {
                    t * t * t
                } else {
                    3.0 * delta * delta * (t - 4.0 / 29.0)
                }
            };
            let fy = (lab[0] + 16.0) / 116.0;
            let f = [fy + lab[1] / 500.0, fy, fy - lab[2] / 200.0];
            [0, 1, 2].map(|c| white[c] * finv(f[c]))
        }

        /// A tone curve at `x` in `0..=1`, by ICC.1 10.5 and 10.18.
        fn curve_at(curve: &Curve, x: f64) -> f64 {
            match curve {
                Curve::Identity => x,
                Curve::Gamma(g) => x.powf(*g),
                Curve::Parametric { params, .. } => {
                    let (g, a, b, c, d) = (params[0], params[1], params[2], params[3], params[4]);
                    if x >= d {
                        (a * x + b).max(0.0).powf(g)
                    } else {
                        c * x
                    }
                }
                Curve::Sampled(points) => {
                    let last = (points.len() - 1) as f64;
                    let position = x.clamp(0.0, 1.0) * last;
                    let index = (position.floor() as usize).min(points.len() - 2);
                    let (low, high) = (f64::from(points[index]), f64::from(points[index + 1]));
                    (low + (high - low) * (position - index as f64)) / 65535.0
                }
            }
        }

        /// The `x` a tone curve takes to light `y`, held to `0..=1`: the
        /// lowest, where the curve is flat or jumps.
        fn curve_inverse(curve: &Curve, y: f64) -> f64 {
            let y = y.clamp(0.0, 1.0);
            let x = match curve {
                Curve::Identity => y,
                Curve::Gamma(g) => y.powf(1.0 / g),
                Curve::Parametric { params, .. } => {
                    let (g, a, b, c, d) = (params[0], params[1], params[2], params[3], params[4]);
                    let (foot, head) = (c * d, (a * d + b).max(0.0).powf(g));
                    if y < foot {
                        y / c
                    } else if y <= head {
                        d
                    } else {
                        (y.powf(1.0 / g) - b) / a
                    }
                }
                Curve::Sampled(points) => {
                    let values: Vec<f64> = points.iter().map(|p| f64::from(*p) / 65535.0).collect();
                    let last = (values.len() - 1) as f64;
                    if y <= values[0] {
                        0.0
                    } else {
                        match values.windows(2).position(|pair| y <= pair[1]) {
                            None => 1.0,
                            Some(index) => {
                                let (low, high) = (values[index], values[index + 1]);
                                (index as f64 + (y - low) / (high - low)) / last
                            }
                        }
                    }
                }
            };
            x.clamp(0.0, 1.0)
        }

        /// What an inverted curve's table can be off by at light `y`, in
        /// device units over `0..=1`: the span of the exact inverse over the
        /// interval of the table's 4 096 values `y` falls in and the one
        /// either side, which the table's entries and its interpolation
        /// between them stay within, and one 16-bit step for its rounding.
        fn inverse_span(curve: &Curve, y: f64) -> f64 {
            let last = ENTRIES - 1.0;
            let node = (y.clamp(0.0, 1.0) * last).floor().min(last - 1.0);
            let low = (node - 1.0).max(0.0) / last;
            let high = (node + 2.0).min(last) / last;
            curve_inverse(curve, high) - curve_inverse(curve, low) + 1.0 / 65535.0
        }

        /// The most `f` moves, summed over its inputs, when each moves by up
        /// to its own `error` either way: the error a stage leaves, carried
        /// to the output through the stages after it.
        fn carried<const N: usize>(
            f: impl Fn([f64; N]) -> f64,
            at: [f64; N],
            error: [f64; N],
        ) -> f64 {
            let base = f(at);
            (0..N)
                .map(|k| {
                    let mut up = at;
                    up[k] += error[k];
                    let mut down = at;
                    down[k] -= error[k];
                    (f(up) - base).abs().max((f(down) - base).abs())
                })
                .sum()
        }

        /// The matrix taking linear device values to XYZ — `columns` as its
        /// columns — inverted by its cofactors over its determinant.
        fn inverse_columns(columns: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
            let m = |row: usize, column: usize| columns[column][row];
            let cofactor = |row: usize, column: usize| {
                let (r0, r1) = ((row + 1) % 3, (row + 2) % 3);
                let (c0, c1) = ((column + 1) % 3, (column + 2) % 3);
                m(r0, c0) * m(r1, c1) - m(r0, c1) * m(r1, c0)
            };
            let determinant = (0..3).map(|c| m(0, c) * cofactor(0, c)).sum::<f64>();
            [0, 1, 2].map(|row| [0, 1, 2].map(|column| cofactor(column, row) / determinant))
        }

        /// The media white over the PCS white, as stored, where `absolute`
        /// (ICC.1 6.2.3), and one otherwise: what a device leg divides a
        /// value by under absolute colorimetric, and a display leg multiplies
        /// one by where the paper is shown.
        fn media(profile: &Profile, absolute: bool) -> [f64; 3] {
            if absolute {
                let white = profile
                    .white
                    .expect("the proof profiles carry a media white");
                [0, 1, 2].map(|c| white[c] / PCS_WHITE[c])
            } else {
                [1.0; 3]
            }
        }

        /// sRGB colours every seventeenth byte on each channel, the corners
        /// among them: 4 096 of them.
        fn srgb_inputs() -> Vec<[u8; 3]> {
            let levels: Vec<u8> = (0..16u8).map(|step| step * 17).collect();
            let mut out = Vec::new();
            for &r in &levels {
                for &g in &levels {
                    for &b in &levels {
                        out.push([r, g, b]);
                    }
                }
            }
            out
        }

        /// **The device leg of a matrix/TRC proof**, at `rgb` under `intent`:
        /// each channel the columns' inverse times the colour's XYZ, divided
        /// by the media white under absolute colorimetric, through its
        /// curve's inverse. The bound carries the XYZ's error and the
        /// inverse matrix's s15.16 steps through the curve's inverse, and the
        /// inverted curve's table step.
        fn matrix_device_holds(proof: &Proof, profile: &Profile, rgb: [u8; 3], intent: Intent) {
            let Some(Model::MatrixTrc { columns, curves }) = &profile.model else {
                panic!("a matrix/TRC proof");
            };
            let scale = media(profile, intent == Intent::AbsoluteColorimetric);
            let inverse = inverse_columns(columns);
            let xyz = srgb_xyz(rgb.map(|byte| f64::from(byte) / 255.0));
            let got = proof.device(rgb, intent);
            assert_eq!(got.channels().len(), 3);
            for (channel, curve) in curves.iter().enumerate() {
                let light = |xyz: [f64; 3]| -> f64 {
                    (0..3)
                        .map(|k| inverse[channel][k] * xyz[k] / scale[k])
                        .sum()
                };
                let device = |xyz: [f64; 3]| 255.0 * curve_inverse(curve, light(xyz));
                let want = device(xyz);
                let linear = light(xyz).clamp(0.0, 1.0);
                // The inverse matrix's s15.16 entries, each half a step,
                // times the XYZ, and the sum's rounding.
                let matrix_error =
                    (0.5 * xyz.iter().map(|v| v.abs()).sum::<f64>() + 0.5) / 65_536.0;
                let error = carried(device, xyz, [XYZ_ERROR; 3])
                    + carried(
                        |[y]: [f64; 1]| 255.0 * curve_inverse(curve, y),
                        [linear],
                        [matrix_error],
                    )
                    + 255.0 * inverse_span(curve, linear);
                let value = f64::from(got.channels()[channel]);
                assert!(
                    (value - want).abs() <= 0.5 + error,
                    "{intent:?} at {rgb:?}, channel {channel}: {value}, not {want:.2} within \
                     {:.2}",
                    0.5 + error
                );
            }
        }

        /// **The device leg of a grey proof**: the colour's luminance,
        /// divided by the media white's under absolute colorimetric, through
        /// the curve's inverse.
        fn grey_device_holds(proof: &Proof, profile: &Profile, rgb: [u8; 3], intent: Intent) {
            let Some(Model::Grey { curve, .. }) = &profile.model else {
                panic!("a grey proof");
            };
            let scale = media(profile, intent == Intent::AbsoluteColorimetric)[1];
            let xyz = srgb_xyz(rgb.map(|byte| f64::from(byte) / 255.0));
            let got = proof.device(rgb, intent);
            assert_eq!(got.channels().len(), 1);
            let device = |xyz: [f64; 3]| 255.0 * curve_inverse(curve, xyz[1] / scale);
            let want = device(xyz);
            let light = (xyz[1] / scale).clamp(0.0, 1.0);
            let factor_error = (0.5 * xyz[1].abs() + 0.5) / 65_536.0;
            let error = carried(device, xyz, [XYZ_ERROR; 3])
                + carried(
                    |[y]: [f64; 1]| 255.0 * curve_inverse(curve, y),
                    [light],
                    [factor_error],
                )
                + 255.0 * inverse_span(curve, light);
            let value = f64::from(got.channels()[0]);
            assert!(
                (value - want).abs() <= 0.5 + error,
                "{intent:?} at {rgb:?}: {value}, not {want:.2} within {:.2}",
                0.5 + error
            );
        }

        /// The `B2A*` table an intent's device leg reads in the CMYK proof:
        /// perceptual `B2A0`, saturation `B2A2`, relative colorimetric
        /// `B2A1`, and absolute colorimetric `B2A1` too, its value first
        /// divided by the media white.
        fn table_of(intent: Intent) -> usize {
            match intent {
                Intent::Perceptual => 0,
                Intent::Saturation => 2,
                Intent::RelativeColorimetric | Intent::AbsoluteColorimetric => 1,
            }
        }

        /// A Lab value as a `B2A*` table of `mft2` takes it: under absolute
        /// colorimetric through XYZ divided by the media white, then in the
        /// legacy encoding over `0..=1` of the currency, each held there.
        fn encoded_for(lab: [f64; 3], scale: [f64; 3]) -> [f64; 3] {
            let lab = if scale == [1.0; 3] {
                lab
            } else {
                let xyz = xyz_of(lab);
                lab_of([0, 1, 2].map(|c| xyz[c] / scale[c]))
            };
            let reach = 65535.0 / 65280.0;
            [
                lab[0] / 100.0 / reach,
                (lab[1] + 128.0) / 255.0 / reach,
                (lab[2] + 128.0) / 255.0 / reach,
            ]
            .map(|v| v.clamp(0.0, 1.0))
        }

        /// What the device leg's sampled cube root can be off by at the ratio
        /// `t` to the white's component `white`, over the root's own scale:
        /// its point spacing squared over eight times the root's curvature
        /// at the near end of the interval `t` falls in — the line below the
        /// break has none — the white's reciprocal's s15.16 step carried
        /// through the root's slope, and a step of the 32-bit fraction for
        /// each rounding.
        fn root_error(t: f64, white: f64) -> f64 {
            let delta = 6.0f64 / 29.0;
            let break_at = delta * delta * delta;
            let spacing = 1.0 / 4096.0;
            let reciprocal = t.abs() * white / (2.0 * 65_536.0);
            let rounding = 3.0 / 4_294_967_296.0;
            if t <= break_at {
                reciprocal / (3.0 * delta * delta) + rounding
            } else {
                let near = ((t / spacing).floor() * spacing).max(break_at);
                let curvature = 2.0 / 9.0 * near.powf(-5.0 / 3.0);
                let slope = 1.0 / (3.0 * near.powf(2.0 / 3.0));
                spacing * spacing / 8.0 * curvature + reciprocal * slope + rounding
            }
        }

        /// **The device leg of a lookup-table proof**, at an sRGB colour
        /// (`Ok`) or at a Lab value handed over as it stands (`Err`): the
        /// colour's Lab through the intent's `B2A*` table, read by this
        /// module's multilinear interpolation at its own nodes. The bound
        /// carries the XYZ's error through the root, the sampled root's own
        /// and the Lab's s15.16 step through the table, the table's input
        /// curve's rounding and its eighth-bit fractions of a cell through
        /// its grid, and its sum's truncation and output curve's rounding.
        fn lut_device_holds(
            proof: &Proof,
            profile: &Profile,
            outputs: &[Grid; 3],
            from: Result<[u8; 3], [f64; 3]>,
            intent: Intent,
        ) {
            let scale = media(profile, intent == Intent::AbsoluteColorimetric);
            let grid = &outputs[table_of(intent)];
            let (got, lab, xyz) = match from {
                Ok(rgb) => {
                    let xyz = srgb_xyz(rgb.map(|byte| f64::from(byte) / 255.0));
                    (proof.device(rgb, intent), lab_of(xyz), Some(xyz))
                }
                Err(lab) => (proof.device_from_connection(lab, true, intent), lab, None),
            };
            assert_eq!(got.channels().len(), 4);
            let white = lab_white();
            let step = 0.5 / 65_536.0;
            let lab_error = match xyz {
                Some(xyz) => {
                    let [ex, ey, ez] = [0, 1, 2].map(|c| root_error(xyz[c] / white[c], white[c]));
                    [
                        116.0 * ey + step,
                        500.0 * (ex + ey) + step,
                        200.0 * (ey + ez) + step,
                    ]
                }
                None => [step; 3],
            };
            let cell = 1.0 / (grid.points - 1) as f64;
            let coordinate_error = 0.5 / 65535.0 + 0.5 / 256.0 * cell;
            for channel in 0..4 {
                let from_lab = |lab: [f64; 3]| 255.0 * grid.at(&encoded_for(lab, scale), channel);
                let from_coordinates = |p: [f64; 3]| 255.0 * grid.at(&p, channel);
                let want = from_lab(lab);
                let mut error = carried(from_lab, lab, lab_error)
                    + carried(
                        from_coordinates,
                        encoded_for(lab, scale),
                        [coordinate_error; 3],
                    )
                    + 255.0 * 1.5 / 65535.0;
                if let Some(xyz) = xyz {
                    error += carried(|xyz| from_lab(lab_of(xyz)), xyz, [XYZ_ERROR; 3]);
                }
                let value = f64::from(got.channels()[channel]);
                assert!(
                    (value - want).abs() <= 0.5 + error,
                    "{intent:?} from {from:?}, ink {channel}: {value}, not {want:.2} within \
                     {:.2}",
                    0.5 + error
                );
            }
        }

        /// **The display leg of a matrix/TRC proof** at the device bytes
        /// `rgb`: each byte through its curve, the columns, the media white
        /// where the paper is shown, `XYZ_D50_TO_SRGB` and the encode. The
        /// bound carries the curves' and the folded matrix's s15.16 steps and
        /// the sum's rounding, and the encode table's index, through the
        /// encode.
        fn matrix_display_holds(proof: &Proof, profile: &Profile, rgb: [u8; 3], paper: bool) {
            let Some(Model::MatrixTrc { columns, curves }) = &profile.model else {
                panic!("a matrix/TRC proof");
            };
            let scale = media(profile, paper);
            let linear = [0, 1, 2].map(|c| curve_at(&curves[c], f64::from(rgb[c]) / 255.0));
            let folded = [0, 1, 2].map(|row| {
                [0, 1, 2].map(|channel| {
                    (0..3)
                        .map(|axis| {
                            crate::XYZ_D50_TO_SRGB[row][axis] * scale[axis] * columns[channel][axis]
                        })
                        .sum::<f64>()
                })
            });
            let got = proof.display(&rgb);
            let got = [got.0, got.1, got.2];
            for row in 0..3 {
                let light: f64 = (0..3).map(|c| folded[row][c] * linear[c]).sum();
                let want = 255.0 * encode(light);
                let light_error = (0.5 * linear.iter().sum::<f64>()
                    + 0.5 * folded[row].iter().map(|m| m.abs()).sum::<f64>()
                    + 0.5)
                    / 65_536.0
                    + 0.5 / 16_384.0;
                let error = carried(|[v]: [f64; 1]| 255.0 * encode(v), [light], [light_error]);
                let value = f64::from(got[row]);
                assert!(
                    (value - want).abs() <= 0.5 + error,
                    "paper {paper}, {rgb:?}, channel {row}: {value}, not {want:.2} within {:.2}",
                    0.5 + error
                );
            }
        }

        /// **The display leg of a grey proof**: the byte through the curve,
        /// on the PCS white, the media white where the paper is shown, as
        /// sRGB — the table the proof holds is the function at every byte,
        /// so each is its rounding.
        fn grey_display_holds(proof: &Proof, profile: &Profile, grey: u8, paper: bool) {
            let Some(Model::Grey { curve, .. }) = &profile.model else {
                panic!("a grey proof");
            };
            let scale = media(profile, paper);
            let value = curve_at(curve, f64::from(grey) / 255.0);
            let want = xyz_srgb([0, 1, 2].map(|c| PCS_WHITE[c] * value * scale[c]));
            let (r, g, b) = proof.display(&[grey]);
            for (got, want) in [r, g, b].into_iter().zip(want) {
                assert!(
                    (f64::from(got) - 255.0 * want).abs() <= 0.5 + 1e-6,
                    "paper {paper}, grey {grey}: {got}, not {:.3}",
                    255.0 * want
                );
            }
        }

        /// **The display leg of a lookup-table proof** at the device bytes
        /// `ink`: the `A2B0` grid at them by this module's interpolation, its
        /// Lab decoded from the legacy encoding, to XYZ, the media white
        /// where the paper is shown, and to sRGB. The bound carries the
        /// grid's 16-bit fractions of a cell and its four interpolation
        /// steps' and its output curve's roundings, the Lab's s15.16 step, and the linear light's
        /// fixed point and the encode table's index, through the stages after
        /// each.
        fn lut_display_holds(
            proof: &Proof,
            profile: &Profile,
            forward: &Grid,
            ink: [u8; 4],
            paper: bool,
        ) {
            let scale = media(profile, paper);
            let reach = 65535.0 / 65280.0;
            let decode_lab = |e: [f64; 3]| {
                [
                    e[0] * reach * 100.0,
                    e[1] * reach * 255.0 - 128.0,
                    e[2] * reach * 255.0 - 128.0,
                ]
            };
            let to_light = |lab: [f64; 3]| -> [f64; 3] {
                let xyz = xyz_of(lab);
                crate::XYZ_D50_TO_SRGB
                    .map(|row| (0..3).map(|c| row[c] * xyz[c] * scale[c]).sum::<f64>())
            };
            let coordinates = ink.map(|byte| f64::from(byte) / 255.0);
            let encoded = [0, 1, 2].map(|channel| forward.at(&coordinates, channel));
            let lab = decode_lab(encoded);
            let light = to_light(lab);
            let cell = 1.0 / (forward.points - 1) as f64;
            let fraction_error = [0.5 / 65_536.0 * cell; 4];
            let encoded_error = [0, 1, 2].map(|channel| {
                carried(
                    |p: [f64; 4]| forward.at(&p, channel),
                    coordinates,
                    fraction_error,
                ) + 3.0 / 65535.0
            });
            let got = proof.display(&ink);
            let got = [got.0, got.1, got.2];
            for row in 0..3 {
                let want = 255.0 * encode(light[row]);
                let error = carried(
                    |e| 255.0 * encode(to_light(decode_lab(e))[row]),
                    encoded,
                    encoded_error,
                ) + carried(
                    |l| 255.0 * encode(to_light(l)[row]),
                    lab,
                    [0.5 / 65_536.0; 3],
                ) + carried(
                    |[v]: [f64; 1]| 255.0 * encode(v),
                    [light[row]],
                    [64.0 / 4_294_967_296.0 + 0.5 / 16_384.0],
                );
                let value = f64::from(got[row]);
                assert!(
                    (value - want).abs() <= 0.5 + error,
                    "paper {paper}, {ink:?}, channel {row}: {value}, not {want:.2} within {:.2}",
                    0.5 + error
                );
            }
        }

        /// **A proof transform is its legs composed** (RC-05.2). For a
        /// matrix/TRC RGB, a grey and a lookup-table CMYK proof profile, each
        /// printed on a paper other than the PCS white, under each intent and
        /// with the paper shown and not: the device leg of sRGB colours
        /// across the cube — and for the CMYK proof, of Lab values at its
        /// `B2A*` grids' nodes and halfway between them — is ICC.1's output
        /// direction recomputed here; the display leg at every byte of the
        /// grey proof, every fifteenth of the RGB proof's channels, and the
        /// CMYK proof's `A2B0` nodes — every 51st byte — and halfway between
        /// them, is its input direction recomputed here; each within the
        /// bound its tables give that sample. The proof of an sRGB colour is
        /// the display leg of its device leg's bytes, byte for byte.
        #[test]
        fn a_proof_transform_is_its_legs_composed() {
            let srgb = srgb_inputs();

            let rgb_bytes = rgb_proof();
            let rgb_profile = Profile::parse(&rgb_bytes).expect("the RGB proof parses");
            let grey_bytes = grey_proof();
            let grey_profile = Profile::parse(&grey_bytes).expect("the grey proof parses");
            let (cmyk_bytes, forward, outputs) = cmyk_proof();
            let cmyk_profile = Profile::parse(&cmyk_bytes).expect("the CMYK proof parses");

            for display in [ProofDisplay::Relative, ProofDisplay::Paper] {
                let paper = display == ProofDisplay::Paper;
                let rgb = Proof::new(&rgb_bytes, display).expect("the RGB proof compiles");
                let grey = Proof::new(&grey_bytes, display).expect("the grey proof compiles");
                let cmyk = Proof::new(&cmyk_bytes, display).expect("the CMYK proof compiles");
                assert_eq!(
                    (rgb.channels(), grey.channels(), cmyk.channels()),
                    (3, 1, 4)
                );
                assert_eq!(rgb.display_intent(), display);
                assert_eq!(&cmyk.space(), b"CMYK");

                for intent in Intent::ALL {
                    for &colour in &srgb {
                        matrix_device_holds(&rgb, &rgb_profile, colour, intent);
                        grey_device_holds(&grey, &grey_profile, colour, intent);
                        lut_device_holds(&cmyk, &cmyk_profile, &outputs, Ok(colour), intent);
                        for proof in [&rgb, &grey, &cmyk] {
                            assert_eq!(
                                proof.apply(colour, intent),
                                proof.display(proof.device(colour, intent).channels()),
                                "{intent:?} at {colour:?}: the proof is its legs composed"
                            );
                        }
                    }
                    // Lab at the `B2A*` grids' nodes and halfway between
                    // them, a coordinate `k / 16` of the legacy currency each.
                    let reach = 65535.0 / 65280.0;
                    for l in 0..=16u32 {
                        for a in (0..=16u32).step_by(2) {
                            for b in (0..=16u32).step_by(4) {
                                let [pl, pa, pb] = [l, a, b].map(|k| f64::from(k) / 16.0);
                                let lab = [
                                    pl * reach * 100.0,
                                    pa * reach * 255.0 - 128.0,
                                    pb * reach * 255.0 - 128.0,
                                ];
                                lut_device_holds(&cmyk, &cmyk_profile, &outputs, Err(lab), intent);
                            }
                        }
                    }
                }

                for grey_byte in 0..=u8::MAX {
                    grey_display_holds(&grey, &grey_profile, grey_byte, paper);
                }
                let levels: Vec<u8> = (0..=17u8).map(|step| step * 15).collect();
                for &r in &levels {
                    for &g in &levels {
                        for &b in &levels {
                            matrix_display_holds(&rgb, &rgb_profile, [r, g, b], paper);
                        }
                    }
                }
                // The `A2B0` grid's nodes, every 51st byte, and halfway
                // between them.
                let inks: Vec<u8> = (0..=10u16).map(|step| (step * 51 / 2) as u8).collect();
                for &c in &inks {
                    for &m in &inks {
                        for &y in inks.iter().step_by(2) {
                            for &k in inks.iter().step_by(2) {
                                lut_display_holds(
                                    &cmyk,
                                    &cmyk_profile,
                                    &forward,
                                    [c, m, y, k],
                                    paper,
                                );
                            }
                        }
                    }
                }
            }
        }

        /// **Each intent's device leg is its own table's, and an intent that
        /// converts through another's shares its transform.** The CMYK
        /// proof's perceptual, relative and saturation device legs are three
        /// tables, so a colour lands three ways; absolute colorimetric's
        /// divides by the paper first. The RGB proof's model serves every
        /// intent but absolute colorimetric with one transform, which the
        /// proof compiles once.
        #[test]
        fn each_intent_reaches_the_device_through_its_own_table() {
            let (cmyk_bytes, _, _) = cmyk_proof();
            let cmyk = Proof::new(&cmyk_bytes, ProofDisplay::Relative).expect("compiles");
            let colour = [40, 120, 200];
            let inks: Vec<Vec<u8>> = Intent::ALL
                .iter()
                .map(|&intent| cmyk.device(colour, intent).channels().to_vec())
                .collect();
            assert_ne!(inks[0], inks[1], "perceptual is B2A0 and relative B2A1");
            assert_ne!(inks[2], inks[1], "saturation is B2A2");
            assert_ne!(
                inks[3], inks[1],
                "absolute colorimetric divides by the paper"
            );

            let rgb = Proof::new(&rgb_proof(), ProofDisplay::Relative).expect("compiles");
            for intent in Intent::ALL {
                let _ = rgb.device(colour, intent);
            }
            let [perceptual, relative, saturation, absolute] =
                [0, 1, 2, 3].map(|slot| rgb.outputs[slot].get().map(Arc::as_ptr));
            assert_eq!(perceptual, relative, "the model's one transform");
            assert_eq!(saturation, relative, "the model's one transform");
            assert_ne!(
                absolute, relative,
                "absolute colorimetric is scaled by the paper"
            );
        }

        /// **The paper shows only where it is asked for.** The display leg of
        /// each proof's paper — no ink on the CMYK proof, full scale on the
        /// grey and RGB ones — is white under relative colorimetric and
        /// [`PAPER`]'s warm white, short of white in its blue, when the paper
        /// is shown.
        #[test]
        fn the_paper_shows_only_where_it_is_asked_for() {
            let (cmyk_bytes, _, _) = cmyk_proof();
            for (bytes, paper) in [
                (rgb_proof(), vec![255u8, 255, 255]),
                (grey_proof(), vec![255]),
                (cmyk_bytes, vec![0, 0, 0, 0]),
            ] {
                let relative = Proof::new(&bytes, ProofDisplay::Relative).expect("compiles");
                let shown = Proof::new(&bytes, ProofDisplay::Paper).expect("compiles");
                let white = relative.display(&paper);
                let tinted = shown.display(&paper);
                assert!(
                    white.0 >= 254 && white.1 >= 254 && white.2 >= 254,
                    "{white:?} is white"
                );
                assert!(tinted.2 < tinted.0, "{tinted:?} is the paper's warm white");
                assert_ne!(relative, shown, "the display intent is the proof's");
            }
        }

        /// **A memo changes no byte.** Every display leg through a memo — the
        /// CMYK proof at a spread of inks, twice over so the second pass is
        /// answered from the memo, and one memo handed between two proofs —
        /// is the display leg without one, and the RGB and grey proofs, which
        /// convert in a few operations, answer through a memo as without.
        #[test]
        fn a_memo_changes_no_byte() {
            let (cmyk_bytes, _, _) = cmyk_proof();
            let cmyk = Proof::new(&cmyk_bytes, ProofDisplay::Relative).expect("compiles");
            let mut memo = ProofMemo::default();
            for _ in 0..2 {
                for c in (0..=255u8).step_by(51) {
                    for m in (0..=255u8).step_by(51) {
                        for y in (0..=255u8).step_by(17) {
                            for k in (0..=255u8).step_by(85) {
                                let ink = [c, m, y, k];
                                assert_eq!(cmyk.display_with(&ink, &mut memo), cmyk.display(&ink));
                            }
                        }
                    }
                }
            }
            assert!(format!("{memo:?}").contains("held"), "{memo:?}");
            // One memo handed between two proofs of one profile, which show
            // the paper differently: each answers its own colour, the memo
            // forgetting the other's.
            let shown = Proof::new(&cmyk_bytes, ProofDisplay::Paper).expect("compiles");
            for ink in [[0u8, 0, 0, 0], [51, 102, 153, 0], [255, 0, 0, 51]] {
                for proof in [&cmyk, &shown, &cmyk] {
                    assert_eq!(proof.display_with(&ink, &mut memo), proof.display(&ink));
                }
            }
            assert_ne!(
                cmyk.display(&[0, 0, 0, 0]),
                shown.display(&[0, 0, 0, 0]),
                "the two show the paper differently"
            );
            for bytes in [rgb_proof(), grey_proof()] {
                let proof = Proof::new(&bytes, ProofDisplay::Paper).expect("compiles");
                for value in [0u8, 17, 128, 255] {
                    let device = [value, 255 - value, value / 2];
                    assert_eq!(
                        proof.display_with(&device, &mut memo),
                        proof.display(&device)
                    );
                }
            }
        }

        /// **Each refusal is reached, by name** (`docs/design/icc.md`,
        /// "Refused by name"): bytes that are no profile; a device link, an
        /// abstract and a named-colour profile, by class; a three-ink `CMY `
        /// printer profile, by data space; an input profile carrying only an
        /// `A2B0`, with nothing out of the connection space; and a profile
        /// converting only through a `D2B1` beside its `B2A0`, with nothing
        /// from its device to the display.
        #[test]
        fn each_proof_refusal_is_reached() {
            assert_eq!(
                Proof::new(b"not a profile", ProofDisplay::Relative).err(),
                Some(ProofError::Profile(IccError::TooShort))
            );
            let link = build_as(
                b"link",
                b"RGB ",
                b"CMYK",
                &[(
                    *b"A2B0",
                    graded_mft2(3, 4, |node, channel| (node * 4000 + channel * 900) as u16),
                )],
            );
            assert_eq!(
                Proof::new(&link, ProofDisplay::Relative).err(),
                Some(ProofError::Class(*b"link"))
            );
            let abstract_profile = build_as(
                b"abst",
                b"Lab ",
                b"Lab ",
                &[(
                    *b"A2B0",
                    graded_mft2(3, 3, |node, channel| (node * 7000 + channel * 300) as u16),
                )],
            );
            assert_eq!(
                Proof::new(&abstract_profile, ProofDisplay::Relative).err(),
                Some(ProofError::Class(*b"abst"))
            );
            let named = build_as(
                b"nmcl",
                b"CMYK",
                b"Lab ",
                &[(
                    *b"ncl2",
                    ncl2_tag(
                        "",
                        "",
                        4,
                        &[("Ink", [0x8000, 0x8000, 0x8000], &[0, 0, 0, 0xFFFF])],
                    ),
                )],
            );
            assert_eq!(
                Proof::new(&named, ProofDisplay::Relative).err(),
                Some(ProofError::Class(*b"nmcl"))
            );
            let three_inks = build_as(
                b"prtr",
                b"CMY ",
                b"Lab ",
                &[
                    (
                        *b"A2B0",
                        graded_mft2(3, 3, |node, channel| {
                            (0xFF00 - node * 0x1000 + channel * 16) as u16
                        }),
                    ),
                    (
                        *b"B2A0",
                        graded_mft2(3, 3, |node, channel| (node * 5000 + channel * 700) as u16),
                    ),
                ],
            );
            assert_eq!(
                Proof::new(&three_inks, ProofDisplay::Relative).err(),
                Some(ProofError::DataSpace(*b"CMY "))
            );
            let input_only = build_as(
                b"scnr",
                b"RGB ",
                b"Lab ",
                &[(
                    *b"A2B0",
                    graded_mft2(3, 3, |node, channel| {
                        (0x4000 + node * 0x1000 + channel * 32) as u16
                    }),
                )],
            );
            assert_eq!(
                Proof::new(&input_only, ProofDisplay::Relative).err(),
                Some(ProofError::NoOutputDirection)
            );
            let elements_only = build(
                b"RGB ",
                b"XYZ ",
                &[
                    (*b"D2B1", Chain { t: 0.0 }.tag()),
                    (
                        *b"B2A0",
                        graded_mft2(3, 3, |node, channel| (node * 6000 + channel * 500) as u16),
                    ),
                ],
            );
            assert!(Profile::parse(&elements_only).is_ok(), "the D2B1 serves it");
            assert_eq!(
                Proof::new(&elements_only, ProofDisplay::Relative).err(),
                Some(ProofError::NoInputDirection)
            );
        }

        /// **Any input answers a colour on the device and one on the
        /// display**: connection-space values past either end, infinities and
        /// NaN, as Lab and as XYZ, through each proof; device bytes of every
        /// value in every channel, and slices too short and too long; never a
        /// panic, and always the device's count of components.
        #[test]
        fn any_input_answers_a_proofed_colour() {
            let (cmyk_bytes, _, _) = cmyk_proof();
            for bytes in [rgb_proof(), grey_proof(), cmyk_bytes] {
                for display in [ProofDisplay::Relative, ProofDisplay::Paper] {
                    let proof = Proof::new(&bytes, display).expect("compiles");
                    for intent in Intent::ALL {
                        for pcs in [
                            [0.0; 3],
                            [100.0, 0.0, 0.0],
                            [0.9642, 1.0, 0.8249],
                            [-5.0, 300.0, -300.0],
                            [1e300, -1e300, 2.0],
                            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY],
                        ] {
                            for lab in [false, true] {
                                let colour = proof.device_from_connection(pcs, lab, intent);
                                assert_eq!(colour.channels().len(), proof.channels());
                            }
                        }
                    }
                    let mut memo = ProofMemo::default();
                    for value in 0..=u8::MAX {
                        let device = [value; 4];
                        let _ = proof.display(&device);
                        let _ = proof.display_with(&device, &mut memo);
                    }
                    for slice in [&[][..], &[7][..], &[1, 2, 3, 4, 5, 6][..]] {
                        let _ = proof.display(slice);
                        let _ = proof.display_with(slice, &mut memo);
                    }
                }
            }
        }
    }
}
