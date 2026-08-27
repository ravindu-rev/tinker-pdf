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
//! them, and v4's `mAB ` — curves on both sides of the grid, each stage
//! carrying its own offset — is refused by name rather than built for three
//! tags.
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

use crate::{ColorSpace, XYZ_D50_TO_SRGB};

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
/// is checked before the table is walked (ruling 1). The corpus's busiest
/// profile declares seventeen.
pub const MAX_ICC_TAGS: u32 = 1024;

/// The most bytes a profile may be.
///
/// The largest in the corpus is 718 672, a printer profile carrying a full set
/// of lookup tables; this is the next power of two above it, which leaves the
/// bound clearing the thing the format is for by better than a factor of two.
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
        /// `kTRC`.
        curve: Curve,
        /// `wtpt`, as `[X, Y, Z]`.
        white: [f64; 3],
    },
    /// A sampled lookup table: `mft1` or `mft2` at an `A2B*` tag.
    ///
    /// What a printer profile carries, because the relation between ink and
    /// light is not a matrix and no curve makes it one. The census found 409
    /// `mft2` and 6 `mft1` against 3 of v4's `mAB `, and 408 of the 415 take
    /// four channels to three — so this one structure is 99.3 % of the corpus's
    /// lookup tables and `mAB ` is refused by name.
    Lut(Lut),
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
    /// Grid points along each axis. Every axis has the same number, which is
    /// `mft1`/`mft2`'s own restriction rather than one imposed here.
    grid: usize,
    /// One curve per input channel, sampled.
    input_tables: Vec<Vec<u16>>,
    /// `grid.pow(inputs)` entries of `outputs` values, the last axis fastest.
    clut: Vec<u16>,
    /// One curve per output channel, sampled.
    output_tables: Vec<Vec<u16>>,
    /// Whether the connection space is `Lab` rather than `XYZ`.
    lab: bool,
}

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
        // sixteen corners for the four-channel case the corpus is made of.
        let last = self.grid - 1;
        let mut base = Vec::with_capacity(self.inputs);
        let mut frac = Vec::with_capacity(self.inputs);
        for coordinate in &coords {
            let scaled = f64::from(*coordinate) / 65535.0 * last as f64;
            let floor = scaled.floor();
            let index = (floor as usize).min(last.saturating_sub(1));
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
                let high = corner & (1 << axis) != 0;
                let step = if high { 1 } else { 0 };
                weight *= if high { frac[axis] } else { 256 - frac[axis] };
                let index = (base[axis] + step).min(last);
                offset = offset * self.grid + index;
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

        // Stage three: each output channel back through its own curve.
        let mut out = [0.0f64; 3];
        for (channel, slot) in out.iter_mut().enumerate().take(self.outputs) {
            let value = (mixed[channel] / total).min(65535) as u16;
            let table = &self.output_tables[channel];
            *slot = f64::from(interpolate(table, f64::from(value) / 65535.0)) / 65535.0;
        }
        out
    }

    /// The sRGB this table's connection-space value is.
    fn to_rgb(&self, components: &[f64]) -> (u8, u8, u8) {
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
            crate::lab_to_rgb(l, a, b)
        } else {
            // XYZ, in the s15Fixed16 convention the connection space uses:
            // 1.0 is 0x8000 of the 16-bit range, so full scale is just under
            // two.
            let scale = 65535.0 / 32768.0;
            let [r, g, b] =
                crate::xyz_d50_to_linear_srgb(pcs[0] * scale, pcs[1] * scale, pcs[2] * scale);
            (
                encode_component(r),
                encode_component(g),
                encode_component(b),
            )
        }
    }
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

        // A profile whose only route to the connection space is a lookup table
        // is refused by name rather than half-read.
        let has_matrix =
            find(b"rXYZ").is_some() && find(b"gXYZ").is_some() && find(b"bXYZ").is_some();
        let has_grey = find(b"kTRC").is_some();

        // A lookup table first, where there is one: a profile carrying both is
        // carrying the table for the case the matrix cannot express, and the
        // census says every such profile in the corpus is a printer's.
        if !has_matrix && !has_grey {
            let lab = &pcs == b"Lab ";
            if !lab && &pcs != b"XYZ " {
                return Err(IccError::UnsupportedPcs);
            }
            // A2B1 is the colorimetric intent and A2B0 the perceptual one;
            // either is a conversion, and a profile carrying only one is
            // ordinary.
            let tag = find(b"A2B1")
                .or_else(|| find(b"A2B0"))
                .or_else(|| find(b"A2B2"));
            let Some(tag) = tag else {
                return Err(IccError::MissingTags);
            };
            return Ok(Profile {
                space,
                pcs,
                class,
                version,
                model: Model::Lut(read_lut(tag, lab)?),
            });
        }
        if &pcs != b"XYZ " {
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
    let reader = Reader { bytes: data };
    let wide = match reader.sig(0).ok_or(IccError::MalformedTag)? {
        s if &s == b"mft2" => true,
        s if &s == b"mft1" => false,
        // `mAB ` and `mBA ` are v4's, and a different structure: curves on both
        // sides of the grid, each stage carrying its own offset. Three tags in
        // the corpus against 415, so they are named rather than built.
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
        grid,
        input_tables,
        clut,
        output_tables,
        lab,
    })
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
#[derive(Clone, Debug, PartialEq)]
pub struct Transform {
    /// Per input channel, the curve sampled into 16-bit linear values.
    curves: Vec<Vec<u16>>,
    /// The profile's columns times the XYZ-to-sRGB matrix, in s15.16, so one
    /// multiply takes linear device values straight to linear sRGB.
    matrix: [[i32; 3]; 3],
    /// How many components go in.
    inputs: usize,
    /// A sampled table, where the profile carries one instead of a matrix.
    lut: Option<Lut>,
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
    /// Compiles a profile into a transform, or `None` for a model that has no
    /// closed form here.
    #[must_use]
    pub fn compile(profile: &Profile) -> Option<Transform> {
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
                })
            }
            // A table is already the transform: there is nothing to compile,
            // because its curves arrived sampled and its grid is the matrix's
            // replacement rather than a factor of it.
            Model::Lut(lut) => Some(Transform {
                curves: Vec::new(),
                matrix: [[0; 3]; 3],
                inputs: lut.inputs,
                lut: Some(lut.clone()),
            }),
            Model::Grey { curve, .. } => {
                // A grey profile's single curve makes a luminance, and the
                // three sRGB channels are that luminance: the white point does
                // not enter, because the answer is achromatic by construction.
                let identity = [[fixed(1.0), 0, 0], [0, fixed(1.0), 0], [0, 0, fixed(1.0)]];
                Some(Transform {
                    curves: vec![compile_curve(curve)],
                    matrix: identity,
                    inputs: 1,
                    lut: None,
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
    /// between one target and the next.
    #[must_use]
    pub fn apply(&self, components: &[f64]) -> (u8, u8, u8) {
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
mod tests {
    use super::*;

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
        let Model::Grey { curve, white } = parsed.model else {
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
        let transform = Transform::compile(&profile).expect("a matrix transform");

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
        let transform = Transform::compile(&parsed).expect("a grey transform");
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
        let bright = Transform::compile(&full).expect("a transform");

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
        let dimmed = Transform::compile(&Profile::parse(&dimmed_bytes).expect("a profile"))
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
        let Model::Lut(lut) = &profile.model else {
            panic!("expected a LUT model");
        };
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
        let Model::Lut(lut) = &profile.model else {
            panic!("expected a LUT model");
        };
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
        let transform = Transform::compile(&profile).expect("a transform");
        assert_eq!(transform.inputs(), 4);

        let Model::Lut(lut) = &profile.model else {
            panic!("expected a LUT model");
        };
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

    /// v4's `mAB ` is refused by name: three tags in the corpus against 415,
    /// and a different structure rather than a variation of this one.
    #[test]
    fn a_v4_lookup_table_is_refused_by_name() {
        let mut tag = b"mAB ".to_vec();
        tag.extend_from_slice(&[0; 28]);
        assert_eq!(
            Profile::parse(&lut_profile(b"CMYK", b"Lab ", tag)),
            Err(IccError::NeedsLut)
        );
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
}
