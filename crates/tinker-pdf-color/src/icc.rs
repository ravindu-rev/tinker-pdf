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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use crate::{ColorSpace, XYZ_D50_TO_SRGB};

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

    /// The `A2B*` tags, by number, this intent converts through, in the
    /// order it tries them (`docs/design/icc.md`, "The table each intent
    /// compiles"): ICC.1 8.10.2's b), the tag designated for the intent, and
    /// c), `A2B0`; then the profile's other tables in the order the parser
    /// has always read them, so no intent refuses a profile another accepts.
    /// Relative colorimetric's order is that order itself, which is why a
    /// page that names no intent converts as it always has.
    const fn order(self) -> [usize; 3] {
        match self {
            Intent::Perceptual => [0, 1, 2],
            Intent::RelativeColorimetric | Intent::AbsoluteColorimetric => [1, 0, 2],
            Intent::Saturation => [2, 0, 1],
        }
    }
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
}

/// The most tags a profile may declare.
///
/// The tag table is `12 * count` bytes and the count is a 32-bit field, so it
/// is checked before the table is walked (ruling 1).
///
/// | | Tags |
/// | --- | --- |
/// | The most any fixture in this repository spends | 7 |
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
/// | The most any fixture in this repository spends | 882 |
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
    /// The transform this profile can supply.
    pub model: Model,
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

/// What kind of transform a profile's tags describe.
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
    /// One curve and a white point: a grey profile.
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
        /// `wtpt`, as `[X, Y, Z]`.
        white: [f64; 3],
    },
    /// Sampled lookup tables — `mft1`, `mft2` or v4's `mAB ` — at the
    /// profile's `A2B*` tags, one for each intent it carries one for.
    ///
    /// What a printer profile carries, because the relation between ink and
    /// light is not a matrix and no curve makes it one. The census found 409
    /// `mft2` and 6 `mft1` against 3 of v4's `mAB `, and 408 of the 415 take
    /// four channels to three.
    Lut(Tables),
}

/// The `A2B*` tables a profile carries, each read when an intent first
/// converts through it.
///
/// The table relative colorimetric converts through is read by
/// [`Profile::parse`], as the one table the parser always read was, so a
/// profile whose table does not read is refused there and a page that names
/// no intent reads no more than it did. Tags that name one element — the same
/// offset and size, which ICC.1 8.2 lets every intent share — are one table,
/// read once.
#[derive(Clone, Debug, PartialEq)]
pub struct Tables {
    /// For `A2B0`, `A2B1` and `A2B2`, which of `tables` the tag names, or
    /// `None` where the profile does not carry it.
    slots: [Option<usize>; 3],
    /// One a distinct element, in the order the tags first name them.
    tables: Vec<Table>,
    /// Whether the connection space is `Lab` rather than `XYZ`.
    lab: bool,
}

impl Tables {
    /// The tables at the elements `places` names — `A2B0`, `A2B1` and
    /// `A2B2`'s offset and size in `bytes`, where the profile carries them —
    /// with relative colorimetric's read now.
    fn read(
        places: [Option<(usize, usize)>; 3],
        bytes: &[u8],
        lab: bool,
    ) -> Result<Tables, IccError> {
        let mut elements: Vec<(usize, usize)> = Vec::new();
        let mut slots = [None; 3];
        for (slot, place) in slots.iter_mut().zip(places) {
            if let Some(place) = place {
                let index = match elements.iter().position(|&element| element == place) {
                    Some(index) => index,
                    None => {
                        elements.push(place);
                        elements.len() - 1
                    }
                };
                *slot = Some(index);
            }
        }
        let default = Intent::RelativeColorimetric
            .order()
            .into_iter()
            .find_map(|tag| slots[tag])
            .ok_or(IccError::MissingTags)?;
        let mut tables = Vec::with_capacity(elements.len());
        for (index, (offset, size)) in elements.into_iter().enumerate() {
            let data = offset
                .checked_add(size)
                .and_then(|end| bytes.get(offset..end))
                .ok_or(IccError::TagOutOfBounds)?;
            tables.push(if index == default {
                Table {
                    bytes: None,
                    read: OnceLock::from(Some(Arc::new(read_lut(data, lab)?))),
                }
            } else {
                Table {
                    bytes: Some(data.to_vec()),
                    read: OnceLock::new(),
                }
            });
        }
        Ok(Tables { slots, tables, lab })
    }

    /// The table `intent` converts through: the first of [`Intent::order`]'s
    /// tags the profile carries whose table reads, read now if no intent has
    /// read it yet. A table that does not read is passed over, as ICC.1 8.10.2
    /// passes over a tag the CMM does not support, and relative colorimetric's
    /// was read by [`Profile::parse`], so every intent finds one.
    fn select(&self, intent: Intent) -> Option<Arc<Lut>> {
        intent
            .order()
            .into_iter()
            .filter_map(|tag| self.slots[tag])
            .find_map(|index| self.tables.get(index)?.lut(self.lab))
    }
}

/// One distinct `A2B*` element: its bytes, until they are read, and what
/// reading them gave, kept from the first read.
#[derive(Clone, Debug)]
struct Table {
    /// The element's bytes; `None` for the table [`Profile::parse`] read.
    bytes: Option<Vec<u8>>,
    /// The table, or `None` where its bytes do not read as one.
    read: OnceLock<Option<Arc<Lut>>>,
}

impl Table {
    fn lut(&self, lab: bool) -> Option<Arc<Lut>> {
        self.read
            .get_or_init(|| {
                self.bytes
                    .as_deref()
                    .and_then(|bytes| read_lut(bytes, lab).ok())
                    .map(Arc::new)
            })
            .clone()
    }
}

/// Two tables are one where their bytes are, or, for the ones the parser
/// read, where what it read is: whether another intent has read a table yet
/// is not a property of the profile.
impl PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        match (&self.bytes, &other.bytes) {
            (Some(ours), Some(theirs)) => ours == theirs,
            (None, None) => self.read.get() == other.read.get(),
            _ => false,
        }
    }
}

/// `mft1` and `mft2`: three stages of table with a grid between them.
///
/// Each input channel goes through its own curve, the result indexes a
/// multi-dimensional grid by interpolation, and each output channel comes back
/// through another curve. The matrix ICC.1 puts in front is only applied when
/// the *input* is the connection space, which for an `A2B*` tag it never is,
/// so it is read and ignored rather than left unread.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut {
    inputs: usize,
    outputs: usize,
    /// Grid points along each axis.
    ///
    /// `mft1` and `mft2` write one count for every axis and this is filled with
    /// it repeated; `mAB ` writes one per channel and they may differ. Carrying
    /// the vector either way is what lets one evaluator serve both.
    grids: Vec<usize>,
    /// One curve per input channel, sampled. `mAB `'s A curves.
    input_tables: Vec<Vec<u16>>,
    /// The product of `grids` entries of `outputs` values, the last axis
    /// fastest. Empty when a v4 tag declares no grid at all.
    clut: Vec<u16>,
    /// One curve per output channel, sampled. `mAB `'s B curves, and the last
    /// stage either way.
    output_tables: Vec<Vec<u16>>,
    /// `mAB `'s M curves, between the grid and the matrix. `None` in a v2 tag,
    /// which has no such stage.
    m_tables: Option<Vec<Vec<u16>>>,
    /// `mAB `'s 3×3 matrix and its three offsets, applied after the M curves.
    ///
    /// Not `mft2`'s matrix, which sits at a fixed offset and applies only when
    /// the *input* is the connection space — at an `A2B*` tag it is not, which
    /// is why the v2 reader steps over it.
    matrix: Option<[f64; 12]>,
    /// Whether the connection space is `Lab` rather than `XYZ`.
    lab: bool,
}

/// The most input channels a lookup table may declare.
///
/// `mAB `'s CLUT header has room for sixteen grid counts, so sixteen is the
/// format's own ceiling; the guard exists because the channel count is one byte
/// and the grid product is what [`MAX_CLUT_ENTRIES`] bounds (ruling 1).
const MAX_LUT_INPUTS: usize = 16;

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
    /// The connection-space value for `components`, each in `0..=1`.
    ///
    /// Integer throughout, like the rest of the pixel path: the input curves
    /// and the grid interpolation are u32 arithmetic on 16-bit samples, and the
    /// only floats are the caller's own components on the way in.
    fn evaluate(&self, components: &[f64]) -> [f64; 3] {
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

        // Stage two: the grid. Multilinear over `inputs` axes, which is
        // sixteen corners for the four-channel case the corpus is made of, and
        // each axis carries its own grid count because `mAB ` may say so.
        let mut mid = [0u16; 3];
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
            let corners = 1usize << self.inputs;
            let mut mixed = [0u64; 3];
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

        // Stage three, v4 only: the M curves and then the matrix. Both are
        // absent from every `mft1` and `mft2`, so this is a no-op there.
        let mut values = [0.0f64; 3];
        for (channel, slot) in values.iter_mut().enumerate().take(self.outputs) {
            let raw = f64::from(mid[channel]) / 65535.0;
            *slot = match &self.m_tables {
                Some(tables) => match tables.get(channel) {
                    Some(table) => f64::from(interpolate(table, raw)) / 65535.0,
                    None => raw,
                },
                None => raw,
            };
        }
        if let Some(m) = &self.matrix {
            let (x, y, z) = (values[0], values[1], values[2]);
            values = [
                m[0] * x + m[1] * y + m[2] * z + m[9],
                m[3] * x + m[4] * y + m[5] * z + m[10],
                m[6] * x + m[7] * y + m[8] * z + m[11],
            ];
        }

        // Stage four: each output channel back through its own curve.
        let mut out = [0.0f64; 3];
        for (channel, slot) in out.iter_mut().enumerate().take(self.outputs) {
            let value = values[channel].clamp(0.0, 1.0);
            let table = &self.output_tables[channel];
            *slot = f64::from(interpolate(table, value)) / 65535.0;
        }
        out
    }

    /// The sRGB this table's connection-space value is.
    fn to_rgb(&self, components: &[f64]) -> (u8, u8, u8) {
        let [r, g, b] = self.to_linear(components);
        let encoding = self.encoding();
        (encoding.encode(r), encoding.encode(g), encoding.encode(b))
    }

    /// The linear sRGB this table's connection-space value is, before it is
    /// encoded and unclamped: the light [`Lut::to_rgb`] encodes, and what a
    /// grid's node holds (`docs/design/icc.md`, P-22, "What a node holds").
    fn to_linear(&self, components: &[f64]) -> [f64; 3] {
        #[cfg(test)]
        tests::PIPELINE_EVALUATIONS.with(|count| count.set(count.get() + 1));
        let pcs = self.evaluate(components);
        if self.lab {
            // ICC v2's *legacy* 16-bit Lab encoding, which is not v4's: full
            // scale is 0xFF00 rather than 0xFFFF, so that an eight-bit 0xFF
            // doubles to it exactly. Getting this wrong shifts every hue by a
            // little under half a percent of the range — a plausible picture in
            // slightly wrong colours, which is the failure this module's
            // refusals exist to avoid, so it is written out rather than
            // folded into a constant.
            const FULL: f64 = 65535.0 / 65280.0;
            let l = pcs[0] * FULL * 100.0;
            let a = pcs[1] * FULL * 255.0 - 128.0;
            let b = pcs[2] * FULL * 255.0 - 128.0;
            crate::lab_to_linear_srgb(l, a, b)
        } else {
            // XYZ, in the s15Fixed16 convention the connection space uses:
            // 1.0 is 0x8000 of the 16-bit range, so full scale is just under
            // two.
            let scale = 65535.0 / 32768.0;
            crate::xyz_d50_to_linear_srgb(pcs[0] * scale, pcs[1] * scale, pcs[2] * scale)
        }
    }

    /// How this table's light is encoded: as `lab_to_rgb` encodes it under a
    /// Lab connection space, as [`encode_component`] does under XYZ.
    const fn encoding(&self) -> Encoding {
        if self.lab {
            Encoding::Lab
        } else {
            Encoding::Xyz
        }
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
    /// One channel of linear light as the pipeline encodes it.
    fn encode(self, linear: f64) -> u8 {
        match self {
            Encoding::Lab => crate::srgb_encode(linear),
            Encoding::Xyz => encode_component(linear),
        }
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

/// A lookup-table transform's pipeline sampled at the nodes of an integer
/// grid, and read between them in fixed point (`docs/design/icc.md`, P-22).
///
/// Each node holds the pipeline's linear sRGB at the node — three signed
/// channels of [`CHANNEL_BITS`], unclamped, beside [`FILLED`] — in one atomic
/// word, evaluated the first time a read reaches it and kept for every read
/// after. A node is a function of the table and its position alone, so two
/// threads that fill one at once store one value, and no read depends on
/// which nodes were filled before it.
struct Grid {
    /// One, three or four.
    inputs: usize,
    /// Nodes along each axis.
    points: usize,
    /// The pipeline's last stage, which the read runs after interpolating.
    encoding: Encoding,
    /// `points.pow(inputs)` nodes, the last axis varying fastest, allocated
    /// at the first read.
    nodes: OnceLock<Box<[AtomicU64]>>,
}

impl Grid {
    /// The grid `lut` converts through, or `None` for an input count no grid
    /// is built over.
    fn over(lut: &Lut) -> Option<Grid> {
        Some(Grid {
            inputs: lut.inputs,
            points: grid_points(lut.inputs)?,
            encoding: lut.encoding(),
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
    fn node(&self, lut: &Lut, nodes: &[AtomicU64], index: usize) -> [i64; 3] {
        let word = match nodes.get(index) {
            Some(slot) => {
                let held = slot.load(Ordering::Relaxed);
                if held & FILLED != 0 {
                    held
                } else {
                    let fresh = self.evaluate(lut, index);
                    slot.store(fresh, Ordering::Relaxed);
                    fresh
                }
            }
            // An index past the grid is not reachable from `read`, whose
            // cells stop a node short of each axis's last; answered all the
            // same rather than indexed into.
            None => self.evaluate(lut, index),
        };
        unpack(word)
    }

    /// Node `index` evaluated: the pipeline's light there, each channel
    /// within `0..=1` pulled to the value nearest it that encodes to the
    /// pipeline's own byte, so a read at the node is the pipeline exactly.
    fn evaluate(&self, lut: &Lut, index: usize) -> u64 {
        let x = self.node_input(index);
        let light = lut.to_linear(&x[..self.inputs]);
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
    fn read(&self, lut: &Lut, components: &[f64]) -> (u8, u8, u8) {
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
        self.read_at(lut, &cells, &fractions)
    }

    /// The grid read at `cells` and `fractions`, one of each an input: the
    /// interpolation every conversion through the grid shares.
    fn read_at(&self, lut: &Lut, cells: &[usize; 4], fractions: &[i64; 4]) -> (u8, u8, u8) {
        let nodes = self.nodes();
        let points = self.points;
        let light = match self.inputs {
            1 => {
                let low = self.node(lut, nodes, cells[0]);
                let high = self.node(lut, nodes, cells[0] + 1);
                [0, 1, 2]
                    .map(|c| round_shift((low[c] << 16) + fractions[0] * (high[c] - low[c]), 16))
            }
            3 => {
                let base = (cells[0] * points + cells[1]) * points + cells[2];
                let strides = [points * points, points, 1];
                let sum = self.tetrahedral(
                    lut,
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
                let low = self.tetrahedral(lut, nodes, base, strides, three);
                let high = self.tetrahedral(lut, nodes, base + 1, strides, three);
                [0, 1, 2]
                    .map(|c| round_shift((low[c] << 16) + fractions[3] * (high[c] - low[c]), 32))
            }
        };
        let table = self.encoding.table();
        (
            table.encode(light[0]),
            table.encode(light[1]),
            table.encode(light[2]),
        )
    }

    /// From node `base`, a step along each of three axes in turn, the
    /// largest fraction first and the earlier axis first between equals,
    /// each node reached weighed by its step's fraction against the one
    /// before: the light, in 2^-16 of [`LIGHT_ONE`]'s unit.
    fn tetrahedral(
        &self,
        lut: &Lut,
        nodes: &[AtomicU64],
        base: usize,
        strides: [usize; 3],
        fractions: [i64; 3],
    ) -> [i64; 3] {
        let mut at = base;
        let mut previous = self.node(lut, nodes, at);
        let mut sum = previous.map(|value| value << 16);
        for axis in largest_first(fractions) {
            at += strides[axis];
            let next = self.node(lut, nodes, at);
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

        let has_matrix =
            find(b"rXYZ").is_some() && find(b"gXYZ").is_some() && find(b"bXYZ").is_some();
        let has_grey = find(b"kTRC").is_some();

        // The lookup tables only where there is no matrix and no grey curve.
        // A profile carrying both is read by its matrix or its curve, for every
        // intent; ICC.1 8.10.2 would read its tables first, and taking that
        // order is the owner's (`docs/design/icc.md`, "The table each intent
        // compiles").
        if !has_matrix && !has_grey {
            let lab = &pcs == b"Lab ";
            if !lab && &pcs != b"XYZ " {
                return Err(IccError::UnsupportedPcs);
            }
            // Each intent's table, by ICC.1 8.10.2 (`Intent::order`), and
            // relative colorimetric's read now: 8.6.5.8 makes it the initial
            // intent, and a profile whose table for it does not read is
            // refused here, whatever intent a page later asks for.
            let place = |want: &[u8; 4]| {
                tags.iter()
                    .find(|(s, _, _)| s == want)
                    .map(|&(_, offset, size)| (offset, size))
            };
            let tables =
                Tables::read([place(b"A2B0"), place(b"A2B1"), place(b"A2B2")], bytes, lab)?;
            return Ok(Profile {
                space,
                pcs,
                class,
                version,
                model: Model::Lut(tables),
            });
        }
        // A matrix profile's columns *are* XYZ by construction, so one
        // declaring any other connection space contradicts itself. A grey
        // profile does not: its one curve produces whatever the connection
        // space's achromatic axis is, `Y` under XYZ and `L*` under Lab, and
        // both are ordinary.
        if has_matrix && &pcs != b"XYZ " {
            return Err(IccError::UnsupportedPcs);
        }
        if !has_matrix && &pcs != b"XYZ " && &pcs != b"Lab " {
            return Err(IccError::UnsupportedPcs);
        }

        let model = if has_matrix {
            if &space != b"RGB " {
                return Err(IccError::UnsupportedSpace);
            }
            let column = |tag: &[u8; 4]| -> Result<[f64; 3], IccError> {
                let data = find(tag).ok_or(IccError::MissingTags)?;
                read_xyz(data)
            };
            let curve = |tag: &[u8; 4]| -> Result<Curve, IccError> {
                let data = find(tag).ok_or(IccError::MissingTags)?;
                read_curve(data)
            };
            Model::MatrixTrc {
                columns: [column(b"rXYZ")?, column(b"gXYZ")?, column(b"bXYZ")?],
                curves: [curve(b"rTRC")?, curve(b"gTRC")?, curve(b"bTRC")?],
            }
        } else {
            if &space != b"GRAY" {
                return Err(IccError::UnsupportedSpace);
            }
            Model::Grey {
                lab: &pcs == b"Lab ",
                curve: read_curve(find(b"kTRC").ok_or(IccError::MissingTags)?)?,
                white: read_xyz(find(b"wtpt").ok_or(IccError::MissingTags)?)?,
            }
        };

        Ok(Profile {
            space,
            pcs,
            class,
            version,
            model,
        })
    }
}

/// An `mft1` or `mft2` tag: ICC.1's two v2 lookup tables.
///
/// They differ in exactly two ways — the sample width, and whether the two
/// curve stages carry their own entry counts or are fixed at 256 — so they are
/// read by one function with a flag rather than two that drift apart.
fn read_lut(data: &[u8], lab: bool) -> Result<Lut, IccError> {
    #[cfg(test)]
    tests::TABLE_READS.with(|reads| reads.set(reads.get() + 1));
    let reader = Reader { bytes: data };
    let wide = match reader.sig(0).ok_or(IccError::MalformedTag)? {
        s if &s == b"mft2" => true,
        s if &s == b"mft1" => false,
        // `mAB ` is v4's, and a different structure: five optional stages
        // where v2 has three fixed ones, each carrying its own offset.
        s if &s == b"mAB " => return read_mab(data, lab),
        // `mBA ` is the *inverse* direction, and an `A2B*` tag never holds one:
        // it would be a profile saying "here is how to get out of the
        // connection space" at the entry that asks how to get in.
        _ => return Err(IccError::NeedsLut),
    };

    let byte = |at: usize| data.get(at).copied().ok_or(IccError::MalformedTag);
    let inputs = byte(8)? as usize;
    let outputs = byte(9)? as usize;
    let grid = byte(10)? as usize;
    if inputs == 0 || outputs == 0 || grid < 2 {
        return Err(IccError::MalformedTag);
    }
    // Three is what the connection space is, and what `to_rgb` reads.
    if outputs != 3 {
        return Err(IccError::UnsupportedSpace);
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
    // space, which at an `A2B*` tag it is not. Read past rather than read.
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
        // v2 has neither stage; `mAB ` is where they come from.
        m_tables: None,
        matrix: None,
        lab,
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
fn read_mab(data: &[u8], lab: bool) -> Result<Lut, IccError> {
    let reader = Reader { bytes: data };
    let byte = |at: usize| data.get(at).copied().ok_or(IccError::MalformedTag);
    let inputs = byte(8)? as usize;
    let outputs = byte(9)? as usize;
    if inputs == 0 || inputs > MAX_LUT_INPUTS {
        return Err(IccError::MalformedTag);
    }
    // Three is what the connection space is, and what `to_rgb` reads.
    if outputs != 3 {
        return Err(IccError::UnsupportedSpace);
    }

    let offset = |at: usize| -> Result<usize, IccError> {
        Ok(reader.u32(at).ok_or(IccError::MalformedTag)? as usize)
    };
    let (b_at, matrix_at, m_at, clut_at, a_at) = (
        offset(12)?,
        offset(16)?,
        offset(20)?,
        offset(24)?,
        offset(28)?,
    );

    // The A curves are per *input* and the M and B curves per output.
    let input_tables = if a_at == 0 {
        (0..inputs)
            .map(|_| sample_curve(&Curve::Identity, V4_CURVE_ENTRIES))
            .collect()
    } else {
        read_curve_run(data, a_at, inputs)?
    };

    let (grids, clut) = if clut_at == 0 {
        // No grid: every input passes straight through to the M stage, which
        // only makes sense when there are as many of one as the other.
        if inputs != outputs {
            return Err(IccError::UnsupportedSpace);
        }
        (vec![2; inputs], Vec::new())
    } else {
        read_mab_clut(data, clut_at, inputs, outputs)?
    };

    let m_tables = if m_at == 0 {
        None
    } else {
        Some(read_curve_run(data, m_at, outputs)?)
    };
    let matrix = if matrix_at == 0 {
        None
    } else {
        let mut values = [0.0f64; 12];
        for (index, slot) in values.iter_mut().enumerate() {
            *slot = reader
                .s15fixed16(matrix_at + index * 4)
                .ok_or(IccError::MalformedTag)?;
        }
        Some(values)
    };
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
        lab,
    })
}

/// `mAB `'s CLUT: sixteen grid counts, a precision byte, then the table.
fn read_mab_clut(
    data: &[u8],
    at: usize,
    inputs: usize,
    outputs: usize,
) -> Result<(Vec<usize>, Vec<u16>), IccError> {
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
    Ok((grids, clut))
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
/// A lookup-table transform over one, three or four inputs converts through
/// a [`Grid`] of its own pipeline, sampled at 256, 33 or 17 nodes an axis and
/// read between them in fixed point (`docs/design/icc.md`, P-22): its
/// pipeline — `Lut::evaluate`, the connection space into sRGB and the
/// encoding — runs once a node, the first time a conversion reaches it,
/// rather than once a colour. A matrix/TRC or grey transform has no grid: its
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
    /// The grid `lut` is read through, where it takes one, three or four
    /// inputs.
    grid: Option<Grid>,
}

/// A copy converts as the original does; its grid starts with no node
/// filled, which no conversion can tell.
impl Clone for Transform {
    fn clone(&self) -> Self {
        Transform {
            curves: self.curves.clone(),
            matrix: self.matrix,
            inputs: self.inputs,
            lut: self.lut.clone(),
            grid: self.grid.clone(),
        }
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
    /// Compiles a profile into its transform for `intent`, or `None` for a
    /// model that has no closed form here.
    ///
    /// A lookup-table profile converts through the table `intent` selects
    /// ([`Intent::order`]). A matrix/TRC or grey profile has one transform,
    /// which ICC.1's Table 25 calls colorimetric, and compiles it alike for
    /// every intent.
    #[must_use]
    pub fn compile(profile: &Profile, intent: Intent) -> Option<Transform> {
        match &profile.model {
            Model::MatrixTrc { columns, curves } => {
                // The profile's columns are the device primaries in XYZ, so
                // the product with XYZ-to-sRGB is device-linear to sRGB-linear
                // — one matrix rather than two, computed once here.
                let mut matrix = [[0i32; 3]; 3];
                for (row, out) in matrix.iter_mut().enumerate() {
                    for (column, slot) in out.iter_mut().enumerate() {
                        let mut sum = 0.0;
                        for k in 0..3 {
                            sum += XYZ_D50_TO_SRGB[row][k] * columns[column][k];
                        }
                        *slot = fixed(sum);
                    }
                }
                Some(Transform {
                    curves: curves.iter().map(compile_curve).collect(),
                    matrix,
                    inputs: 3,
                    lut: None,
                    grid: None,
                })
            }
            // A table is already the transform: its curves arrived sampled and
            // its grid is the matrix's replacement rather than a factor of it.
            // What compiles is the grid it is read through, whose nodes are
            // evaluated as conversions reach them.
            Model::Lut(tables) => {
                let lut = tables.select(intent)?;
                Some(Transform {
                    curves: Vec::new(),
                    matrix: [[0; 3]; 3],
                    inputs: lut.inputs,
                    grid: Grid::over(&lut),
                    lut: Some(lut),
                })
            }
            Model::Grey { lab, curve, .. } => {
                // A grey profile's single curve makes a luminance, and the
                // three sRGB channels are that luminance: the white point does
                // not enter, because the answer is achromatic by construction.
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
                let identity = [[fixed(1.0), 0, 0], [0, fixed(1.0), 0], [0, 0, fixed(1.0)]];
                Some(Transform {
                    curves: vec![table],
                    matrix: identity,
                    inputs: 1,
                    lut: None,
                    grid: None,
                })
            }
        }
    }

    /// How many components this transform takes.
    #[must_use]
    pub const fn inputs(&self) -> usize {
        self.inputs
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
        if let (Some(lut), Some(grid)) = (&self.lut, &self.grid) {
            return grid.read(lut, components);
        }
        self.evaluate(components)
    }

    /// The transform's whole pipeline for one colour, with no grid: what a
    /// grid's node is evaluated as, and what the tests hold a grid to.
    pub(crate) fn evaluate(&self, components: &[f64]) -> (u8, u8, u8) {
        if let Some(lut) = &self.lut {
            return lut.to_rgb(components);
        }
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

        let mut out = [0u8; 3];
        for (row, slot) in out.iter_mut().enumerate() {
            let mut sum = 0i64;
            for (column, value) in linear.iter().enumerate() {
                sum += i64::from(self.matrix[row][column]) * value / ONE;
            }
            *slot = encode_srgb(sum);
        }
        (out[0], out[1], out[2])
    }
}

/// A profile's transforms, one for each intent, each compiled the first time
/// a conversion asks for it (`docs/design/icc.md`, "What compiling per intent
/// costs").
///
/// Relative colorimetric's is compiled with the set: it is 8.6.5.8's initial
/// intent, the one every conversion that names none takes, so a profile used
/// in no other intent compiles exactly the one transform it always did. A
/// matrix/TRC or grey profile serves perceptual and saturation from that same
/// transform; a lookup-table profile compiles one for each intent asked for,
/// sharing the table where two intents select one.
#[derive(Debug)]
pub struct Transforms {
    profile: Profile,
    relative: Arc<Transform>,
    /// Perceptual, saturation and absolute colorimetric's, in that order.
    others: [OnceLock<Arc<Transform>>; 3],
}

impl Transforms {
    /// The profile's transforms, or `None` where it has no relative
    /// colorimetric one, which leaves it with none at all.
    #[must_use]
    pub fn compile(profile: Profile) -> Option<Transforms> {
        let relative = Arc::new(Transform::compile(&profile, Intent::RelativeColorimetric)?);
        Some(Transforms {
            profile,
            relative,
            others: [OnceLock::new(), OnceLock::new(), OnceLock::new()],
        })
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
        let shares =
            !matches!(self.profile.model, Model::Lut(_)) && intent != Intent::AbsoluteColorimetric;
        self.others[slot].get_or_init(|| {
            if shares {
                return Arc::clone(&self.relative);
            }
            // Every intent's order ends in every table the profile carries,
            // relative colorimetric's among them, so this compiles; were it
            // not to, relative colorimetric's is the answer 8.6.5.8 gives.
            Transform::compile(&self.profile, intent)
                .map_or_else(|| Arc::clone(&self.relative), Arc::new)
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
    }

    /// The pipeline evaluations this thread has made since the last call.
    fn pipeline_evaluations() -> u64 {
        PIPELINE_EVALUATIONS.with(|count| count.replace(0))
    }

    /// sRGB's own primaries, chromatically adapted to D50 — which is what an
    /// sRGB ICC profile stores, because ICC.1 puts the connection space there.
    ///
    /// These are the numbers in every sRGB profile ever shipped, and they are
    /// here rather than in the module because they are a *fixture*: the module
    /// reads whatever a profile says.
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

    /// A minimal profile: a header, a tag count, and the tags asked for.
    fn build(space: &[u8; 4], pcs: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut header = vec![0u8; 128];
        header[12..16].copy_from_slice(b"mntr");
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
            /// Refused by name, after the tag table was walked.
            Refusal(IccError),
        }

        let seeds: [(&str, Vec<u8>, Reaches); 7] = [
            // Three lookup tables, one an intent, each a different grid: the
            // seed that reaches every intent's own table.
            (
                "lut-three-intents",
                three_intents_profile(),
                Reaches::Transform,
            ),
            // Three-component matrix/TRC, the commonest profile there is.
            ("srgb-para-curve", srgb_profile(), Reaches::Transform),
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
                Reaches::Transform => {
                    let profile = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    for intent in Intent::ALL {
                        assert!(
                            Transform::compile(&profile, intent).is_some(),
                            "{name}: no {intent:?} transform"
                        );
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
        let Model::MatrixTrc { columns, curves } = profile.model else {
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
        let Model::Grey { curve, white, .. } = parsed.model else {
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
        let Model::Lut(tables) = &profile.model else {
            panic!("expected a LUT model");
        };
        let lut = tables
            .select(Intent::RelativeColorimetric)
            .expect("relative colorimetric's table");
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
        let Model::Lut(tables) = &profile.model else {
            panic!("expected a LUT model");
        };
        let lut = tables
            .select(Intent::RelativeColorimetric)
            .expect("relative colorimetric's table");
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

        let Model::Lut(tables) = &profile.model else {
            panic!("expected a LUT model");
        };
        let lut = tables
            .select(Intent::RelativeColorimetric)
            .expect("relative colorimetric's table");
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
        let lut = transform.lut.as_ref().expect("a lookup-table transform");
        lut.evaluate(&x).map(|v| (v * 65535.0).round() as u32)
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
        let mut tags = Vec::new();
        for (t, &here) in present.iter().enumerate() {
            if !here {
                continue;
            }
            let signature = [*b"A2B0", *b"A2B1", *b"A2B2"][t];
            let data = if broken == Some(t) {
                // The inverse direction's signature, which an `A2B*` tag
                // never holds.
                let mut inverse = b"mBA ".to_vec();
                inverse.extend_from_slice(&[0; 28]);
                inverse
            } else {
                mft2(3, 2, &[[fallback_value(t); 3]; 8])
            };
            tags.push((signature, data));
        }
        build(b"RGB ", b"XYZ ", &tags)
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
        let Model::Lut(tables) = &profile.model else {
            panic!("expected lookup tables");
        };
        assert_eq!(tables.tables.len(), 2, "two elements, two tables");
        assert_eq!(tables.slots, [Some(0), Some(0), Some(1)]);

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
}
