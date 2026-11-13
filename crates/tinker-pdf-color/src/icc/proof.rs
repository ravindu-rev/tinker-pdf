//! Soft proofing (`docs/design/icc.md`, "Soft proofing (RC-05)"): a colour as
//! a proof device would reproduce it, shown on the display.
//!
//! A proof is two legs composed, `V(D(c))`. The **device leg** `D` takes a
//! colour into the proof profile's device components — an sRGB colour through
//! IEC 61966-2-1's decode and `SRGB_TO_XYZ_D50`, or a colour's own
//! connection-space value as its own profile or closed form gives it — then
//! through RC-20's output direction ([`Output`]) at the colour's rendering
//! intent, each intent's compiled the first time a colour asks for it and
//! shared with every other intent that compiles to the same transform. The
//! **display leg** `V` takes the device's components back to sRGB through the
//! profile's input direction at the display intent: media-relative
//! colorimetric, so the paper shows as the display's white, or ICC-absolute
//! colorimetric where the paper is shown ([`ProofDisplay`]).
//!
//! # Integers per colour
//!
//! [`Proof`] is compiled once from profile bytes, and each stage is a table
//! or a fixed-point product built then (ruling 4): the sRGB decode a
//! 256-entry table in s15.16; the matrix into XYZ in s15.16; XYZ to Lab for a
//! profile whose connection space is Lab through 8.6.5.4's cube root sampled
//! at 8 193 points, read linearly in 32 fractional bits; the output
//! direction RC-20's own tables, which take the value as
//! [`Output::apply`] takes it, quantised once on the way in; the device's
//! components rounded to bytes. Back: a grey proof's display leg is a table
//! of the 256 bytes, each the profile's grey evaluated exactly when the
//! proof is compiled; a matrix/TRC proof's each channel's curve sampled at
//! the 256 bytes in s15.16 and its columns times XYZ-to-sRGB in s15.16; a
//! lookup-table proof's the profile's `A2B*` table in integers — its input
//! curves sampled at the 256 bytes, its grid multilinear with 16-bit
//! fractions at its own nodes, its other curves interpolated in 16 bits, a
//! Lab connection space taken to XYZ with the cube in fixed point — and the
//! sRGB encode IEC 61966-2-1's sampled at 2^14 + 1 points, its index
//! rounded. A transcendental is evaluated only while those tables are built,
//! through `tinker_pdf_math`.
//!
//! # What is refused, by name
//!
//! [`ProofError`]: a profile [`Profile::parse`] refuses; a device link, an
//! abstract or a named-colour profile, which has no device side to proof; a
//! data space other than grey, RGB and CMYK, which no canvas holds; a profile
//! with nothing taking the connection space to its device, or nothing taking
//! its device to the display.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use super::{
    interpolate, round_shift, Curve, IccError, Intent, LabScale, Lut, MatrixStage, Model, Output,
    Profile, Step, ONE, PCS_WHITE,
};
use crate::{SRGB_TO_XYZ_D50, XYZ_D50_TO_SRGB};

/// What a proof's display leg shows the proof device's paper as
/// (`docs/design/icc.md`, "Intents and the paper").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProofDisplay {
    /// Media-relative colorimetric: the paper shows as the display's white.
    #[default]
    Relative,
    /// ICC-absolute colorimetric: the profile's media white against the PCS
    /// white, so the paper shows as its own colour (ICC.1 6.2.3).
    Paper,
}

impl ProofDisplay {
    /// The rendering intent the display leg converts through.
    const fn intent(self) -> Intent {
        match self {
            ProofDisplay::Relative => Intent::RelativeColorimetric,
            ProofDisplay::Paper => Intent::AbsoluteColorimetric,
        }
    }
}

/// Why profile bytes make no proof (`docs/design/icc.md`, "Refused by
/// name"): refused when the proof is built, so a caller hears it once rather
/// than every page rendering unproofed (ruling 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofError {
    /// [`Profile::parse`] refuses the bytes, for its reason.
    Profile(IccError),
    /// Nothing takes the connection space to the device: no `B2A*` table or
    /// `B2D*` tag, and no matrix or curve that inverts — an input profile
    /// carrying `A2B*` tables alone, or a matrix with no inverse.
    NoOutputDirection,
    /// Nothing takes the device to the display: no `A2B*` table of the data
    /// space's channels, and no matrix/TRC or grey model.
    NoInputDirection,
    /// A data space other than `GRAY`, `RGB ` and `CMYK`, by its signature:
    /// no canvas holds its components.
    DataSpace([u8; 4]),
    /// A device link, an abstract or a named-colour profile, by its class,
    /// which has no device side to proof.
    Class([u8; 4]),
}

/// A colour on the proof device: its components as the device's canvas holds
/// them, a byte each, as many as its data space has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProofColour {
    values: [u8; 4],
    count: usize,
}

impl ProofColour {
    /// The colour of 16-bit device values, each rounded to the nearest byte.
    fn of(values: &[u16]) -> ProofColour {
        let mut colour = ProofColour {
            values: [0; 4],
            count: values.len().min(4),
        };
        for (slot, &value) in colour.values.iter_mut().zip(values) {
            *slot = ((u32::from(value) * 255 + 32_767) / 65_535) as u8;
        }
        colour
    }

    /// The components, in the order the device's data space lists them.
    #[must_use]
    pub fn channels(&self) -> &[u8] {
        self.values.get(..self.count).unwrap_or(&[])
    }
}

/// How many display-leg conversions a [`ProofMemo`] remembers at once.
const MEMO_SLOTS: usize = 4096;

/// The bit that says a memo slot holds a conversion.
const MEMO_HELD: u64 = 1 << 63;

/// What a lookup-table proof's display leg has converted, remembered by the
/// device bytes that asked for it, in a slot those bytes choose: 4 096
/// slots, each the bytes beside the colour, so two pixels that share a slot
/// convert again rather than answer for each other. Every colour is the one
/// [`Proof::display`] gives; only the work is shared, as a picture repeats
/// its colours. The caller keeps one for a page's pixels. A memo remembers
/// one proof's colours: handed to another proof, it forgets what it held
/// before it answers for that one.
pub struct ProofMemo {
    slots: Box<[u64]>,
    /// The proof the slots hold colours of, by its `id`; none at first.
    owner: Option<u64>,
}

impl Default for ProofMemo {
    fn default() -> ProofMemo {
        ProofMemo {
            slots: vec![0; MEMO_SLOTS].into_boxed_slice(),
            owner: None,
        }
    }
}

impl std::fmt::Debug for ProofMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let held = self
            .slots
            .iter()
            .filter(|&&slot| slot & MEMO_HELD != 0)
            .count();
        f.debug_struct("ProofMemo")
            .field("held", &held)
            .finish_non_exhaustive()
    }
}

impl ProofMemo {
    /// The colour `key`'s device bytes stand for under the proof `owner`,
    /// converted by `convert` unless this memo holds it already.
    fn convert(
        &mut self,
        owner: u64,
        key: u32,
        convert: impl FnOnce() -> (u8, u8, u8),
    ) -> (u8, u8, u8) {
        if self.owner != Some(owner) {
            self.slots.fill(0);
            self.owner = Some(owner);
        }
        // Fibonacci hashing of the key, shifted to leave as many bits as the
        // memo is a power of two.
        let shift = 64 - MEMO_SLOTS.trailing_zeros();
        let at = (u64::from(key).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> shift) as usize;
        if let Some(&slot) = self.slots.get(at) {
            if slot & MEMO_HELD != 0 && (slot >> 24) as u32 == key {
                return ((slot >> 16) as u8, (slot >> 8) as u8, slot as u8);
            }
        }
        let colour = convert();
        if let Some(slot) = self.slots.get_mut(at) {
            *slot = MEMO_HELD
                | (u64::from(key) << 24)
                | (u64::from(colour.0) << 16)
                | (u64::from(colour.1) << 8)
                | u64::from(colour.2);
        }
        colour
    }
}

/// A soft proof compiled from a profile: the device leg into its device's
/// components at each rendering intent, and the display leg back to sRGB
/// (the module documentation).
///
/// Built once from profile bytes and shared: each intent's output direction
/// is compiled the first time a colour asks for it, and an intent whose
/// transform is another's shares it, so a proof holds one output transform a
/// distinct source.
#[derive(Clone)]
pub struct Proof {
    /// Which proof this is, among every one this process has built: what a
    /// [`ProofMemo`] knows its colours by. A copy keeps it, its colours being
    /// the same.
    id: u64,
    profile: Profile,
    /// The device's components: 1, 3 or 4.
    channels: usize,
    display: ProofDisplay,
    /// Relative colorimetric's output direction, compiled when the proof is
    /// built: what an intent whose order reaches nothing of the profile's
    /// converts through, the answer 8.6.5.8 gives.
    relative: Arc<Output>,
    /// Each intent's output direction, in [`Intent::ALL`]'s order, compiled
    /// the first time a colour asks for it.
    pub(super) outputs: [OnceLock<Arc<Output>>; 4],
    view: View,
}

/// Two proofs are one where their profiles and their display intents are:
/// which intents have compiled their output direction so far is not a
/// property of the proof.
impl PartialEq for Proof {
    fn eq(&self, other: &Self) -> bool {
        self.profile == other.profile && self.display == other.display
    }
}

/// The device and the display intent, not the tables.
impl std::fmt::Debug for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proof")
            .field("space", &String::from_utf8_lossy(&self.profile.space))
            .field("pcs", &String::from_utf8_lossy(&self.profile.pcs))
            .field("display", &self.display)
            .field("view", &self.view.kind())
            .finish_non_exhaustive()
    }
}

/// Where each intent's output direction sits in [`Proof::outputs`].
const fn slot(intent: Intent) -> usize {
    match intent {
        Intent::Perceptual => 0,
        Intent::RelativeColorimetric => 1,
        Intent::Saturation => 2,
        Intent::AbsoluteColorimetric => 3,
    }
}

impl Proof {
    /// The proof the profile `bytes` make, its display leg showing the paper
    /// as `display` says.
    ///
    /// # Errors
    ///
    /// [`ProofError`]: in order, the profile's own refusal, its class, its
    /// data space, no transform out of the connection space under relative
    /// colorimetric, and no transform from its device to the display.
    pub fn new(bytes: &[u8], display: ProofDisplay) -> Result<Proof, ProofError> {
        let profile = Profile::parse(bytes).map_err(ProofError::Profile)?;
        if matches!(&profile.class, b"link" | b"abst" | b"nmcl") {
            return Err(ProofError::Class(profile.class));
        }
        let channels = match &profile.space {
            b"GRAY" => 1,
            b"RGB " => 3,
            b"CMYK" => 4,
            _ => return Err(ProofError::DataSpace(profile.space)),
        };
        let relative = Output::compile(&profile, Intent::RelativeColorimetric)
            .ok()
            .filter(|output| output.channels() == channels)
            .map(Arc::new)
            .ok_or(ProofError::NoOutputDirection)?;
        let view =
            View::compile(&profile, display, channels).ok_or(ProofError::NoInputDirection)?;
        let outputs = [
            OnceLock::new(),
            OnceLock::from(Arc::clone(&relative)),
            OnceLock::new(),
            OnceLock::new(),
        ];
        static PROOFS: AtomicU64 = AtomicU64::new(0);
        Ok(Proof {
            id: PROOFS.fetch_add(1, Ordering::Relaxed),
            profile,
            channels,
            display,
            relative,
            outputs,
            view,
        })
    }

    /// How many components the proof device takes: one for a grey proof,
    /// three for an RGB one and four for a CMYK one.
    #[must_use]
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The proof device's data colour space, as its signature.
    #[must_use]
    pub fn space(&self) -> [u8; 4] {
        self.profile.space
    }

    /// What the display leg shows the paper as.
    #[must_use]
    pub fn display_intent(&self) -> ProofDisplay {
        self.display
    }

    /// The proof profile.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The output direction under `intent`, compiled now if no colour has
    /// asked for it yet: shared with an intent whose transform is the same
    /// one, and relative colorimetric's where `intent`'s order reaches
    /// nothing of the profile's, the answer 8.6.5.8 gives.
    fn output(&self, intent: Intent) -> &Output {
        self.outputs[slot(intent)].get_or_init(|| match Output::compile(&self.profile, intent) {
            Ok(output) if output.channels() == self.channels => {
                for held in self.outputs.iter().filter_map(OnceLock::get) {
                    if **held == output {
                        return Arc::clone(held);
                    }
                }
                Arc::new(output)
            }
            _ => Arc::clone(&self.relative),
        })
    }

    /// **The device leg of an sRGB colour**: `rgb` decoded to linear light,
    /// into D50 XYZ, into the profile's connection space, and through the
    /// output direction at `intent` to the device's components.
    #[must_use]
    pub fn device(&self, rgb: [u8; 3], intent: Intent) -> ProofColour {
        self.into_device(self.connection(srgb_to_xyz(rgb)), intent)
    }

    /// **The device leg of a colour's own connection-space value**: `pcs`,
    /// D50 XYZ with the PCS white's `Y` at one where `lab` is false and
    /// `L*`, `a*` and `b*` where it is true — what a colour's own profile
    /// gives at the colour's intent, or a CIE-based space's closed form —
    /// into the profile's connection space and through the output direction
    /// at `intent`. A component that is not a number is taken as zero, and
    /// one past ±1 024 as ±1 024.
    #[must_use]
    pub fn device_from_connection(&self, pcs: [f64; 3], lab: bool, intent: Intent) -> ProofColour {
        let fixed = pcs.map(quantize);
        let own = match (lab, self.profile.pcs == *b"Lab ") {
            (false, true) => xyz_to_lab_fixed(fixed),
            (true, false) => lab_to_xyz_fixed(fixed).map(|component| round_shift(component, 16)),
            _ => fixed,
        };
        self.into_device(own, intent)
    }

    /// XYZ in s15.16 in the profile's own connection space: as it stands, or
    /// as Lab.
    fn connection(&self, xyz: [i64; 3]) -> [i64; 3] {
        if self.profile.pcs == *b"Lab " {
            xyz_to_lab_fixed(xyz)
        } else {
            xyz
        }
    }

    /// `pcs`, s15.16 in the profile's connection space, through the output
    /// direction at `intent`, each component rounded to a byte.
    fn into_device(&self, pcs: [i64; 3], intent: Intent) -> ProofColour {
        // An s15.16 number is a float exactly: the output direction takes the
        // value as `Output::apply` does and quantises it once again, exactly.
        let value = pcs.map(|component| component as f64 / ONE as f64);
        ProofColour::of(self.output(intent).apply(value).channels())
    }

    /// **The display leg**: the device's components `device`, a byte each,
    /// as sRGB through the profile's input direction at the display intent.
    /// A component the slice does not hold is zero, and one past the device's
    /// channels is not read.
    #[must_use]
    pub fn display(&self, device: &[u8]) -> (u8, u8, u8) {
        let at = |index: usize| device.get(index).copied().unwrap_or(0);
        match &self.view {
            View::Grey(table) => {
                let [r, g, b] = table[usize::from(at(0))];
                (r, g, b)
            }
            View::Matrix { curves, matrix } => {
                matrix_display(curves, matrix, [at(0), at(1), at(2)])
            }
            View::Table(view) => view.display([at(0), at(1), at(2), at(3)]),
        }
    }

    /// [`Proof::display`], a lookup-table proof's conversions remembered in
    /// `memo` by the bytes that asked for them: the same colour, and once a
    /// colour rather than once a pixel.
    pub fn display_with(&self, device: &[u8], memo: &mut ProofMemo) -> (u8, u8, u8) {
        let View::Table(view) = &self.view else {
            return self.display(device);
        };
        let at = |index: usize| device.get(index).copied().unwrap_or(0);
        let bytes = [at(0), at(1), at(2), at(3)];
        let mut key = 0u32;
        for &byte in bytes.iter().take(self.channels) {
            key = (key << 8) | u32::from(byte);
        }
        memo.convert(self.id, key, || view.display(bytes))
    }

    /// **The proof of an sRGB colour**: its device leg at `intent`, then the
    /// display leg, `V(D(rgb))`.
    #[must_use]
    pub fn apply(&self, rgb: [u8; 3], intent: Intent) -> (u8, u8, u8) {
        self.display(self.device(rgb, intent).channels())
    }
}

/// A connection-space component in s15.16, from a real number: zero for one
/// that is not a number, and held within ±1 024.
fn quantize(value: f64) -> i64 {
    if value.is_nan() {
        return 0;
    }
    // Within ±1 024, so the product with 65 536 is exact and fits.
    (value.clamp(-1024.0, 1024.0) * ONE as f64).round() as i64
}

// ---- the device leg's way into the connection space -------------------------

/// IEC 61966-2-1's decode at each byte, linear light in s15.16.
fn srgb_decode() -> &'static [i64; 256] {
    static TABLE: OnceLock<[i64; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0i64; 256];
        for (byte, slot) in table.iter_mut().enumerate() {
            let encoded = byte as f64 / 255.0;
            // Linear near zero, so the slope stays finite where a plain
            // power would flatten.
            let linear = if encoded <= 0.040_45 {
                encoded / 12.92
            } else {
                tinker_pdf_math::pow((encoded + 0.055) / 1.055, 2.4)
            };
            *slot = (linear * ONE as f64).round() as i64;
        }
        table
    })
}

/// `SRGB_TO_XYZ_D50` in s15.16.
fn srgb_to_xyz_matrix() -> &'static [[i64; 3]; 3] {
    static MATRIX: OnceLock<[[i64; 3]; 3]> = OnceLock::new();
    MATRIX.get_or_init(|| {
        SRGB_TO_XYZ_D50.map(|row| row.map(|entry| (entry * ONE as f64).round() as i64))
    })
}

/// An sRGB colour as D50 XYZ in s15.16: each byte through the decode table,
/// then the matrix.
fn srgb_to_xyz(rgb: [u8; 3]) -> [i64; 3] {
    let decode = srgb_decode();
    let linear = rgb.map(|byte| decode[usize::from(byte)]);
    srgb_to_xyz_matrix().map(|row| {
        let sum: i64 = row
            .iter()
            .zip(linear)
            .map(|(entry, value)| entry * value)
            .sum();
        round_shift(sum, 16)
    })
}

/// Fractional bits of the Lab conversions' fixed point.
const FRACTION: u32 = 32;

/// 1.0 in [`FRACTION`] bits.
const UNIT: i64 = 1 << FRACTION;

/// D50 white the Lab relation reads against (`crate::lab_to_xyz`'s), its
/// reciprocal and itself in [`FRACTION`] bits.
struct LabWhite {
    /// Each component's reciprocal, in s15.16: XYZ in s15.16 times it is the
    /// ratio in [`FRACTION`] bits.
    reciprocal: [i64; 3],
    /// Each component in [`FRACTION`] bits.
    white: [i64; 3],
    /// 8.6.5.4's cube root sampled at every 1/4 096 of the ratio from 0 to 2,
    /// in [`FRACTION`] bits.
    root: Box<[i64]>,
}

/// How finely [`LabWhite::root`] samples the ratio: 2^12 points to the unit.
const ROOT_BITS: u32 = 12;

/// The largest ratio [`LabWhite::root`] holds: two, past which a component
/// is held there.
const ROOT_TOP: i64 = 2 * UNIT;

/// The ratio below which 8.6.5.4's relation is a line: `(6/29)³`.
const LAB_BREAK: f64 = (6.0 / 29.0) * (6.0 / 29.0) * (6.0 / 29.0);

/// The white, its reciprocals and the sampled root, built once through
/// `tinker_pdf_math`.
fn lab_white() -> &'static LabWhite {
    static WHITE: OnceLock<LabWhite> = OnceLock::new();
    WHITE.get_or_init(|| {
        let white = crate::lab_to_xyz(100.0, 0.0, 0.0);
        let points = (2usize << ROOT_BITS) + 1;
        let root = (0..points)
            .map(|index| {
                let ratio = index as f64 / f64::from(1u32 << ROOT_BITS);
                (lab_f(ratio) * UNIT as f64).round() as i64
            })
            .collect();
        LabWhite {
            reciprocal: white.map(|component| (ONE as f64 / component).round() as i64),
            white: white.map(|component| (component * UNIT as f64).round() as i64),
            root,
        }
    })
}

/// 8.6.5.4's forward relation at the ratio `t`: the cube root above
/// [`LAB_BREAK`], the line below it.
fn lab_f(t: f64) -> f64 {
    const DELTA: f64 = 6.0 / 29.0;
    if t > LAB_BREAK {
        tinker_pdf_math::cbrt(t)
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

/// `t / (3δ²) + 4/29` in [`FRACTION`] bits, for a ratio on the line.
fn lab_line(t: i64) -> i64 {
    const DELTA: f64 = 6.0 / 29.0;
    static SLOPE: OnceLock<(i64, i64)> = OnceLock::new();
    let (slope, offset) = *SLOPE.get_or_init(|| {
        (
            (UNIT as f64 / (3.0 * DELTA * DELTA)).round() as i64,
            (4.0 / 29.0 * UNIT as f64).round() as i64,
        )
    });
    let product = (i128::from(t) * i128::from(slope)) >> FRACTION;
    product as i64 + offset
}

/// 8.6.5.4's forward relation at a ratio in [`FRACTION`] bits: the sampled
/// root, read linearly between its two points either side, above the
/// break; the line, which needs no table, below it.
fn lab_f_fixed(white: &LabWhite, t: i64) -> i64 {
    static BREAK: OnceLock<i64> = OnceLock::new();
    let at_break = *BREAK.get_or_init(|| (LAB_BREAK * UNIT as f64).round() as i64);
    if t <= at_break {
        return lab_line(t);
    }
    let t = t.min(ROOT_TOP);
    let shift = FRACTION - ROOT_BITS;
    let index = (t >> shift) as usize;
    let fraction = t & ((1 << shift) - 1);
    let low = white.root.get(index).copied().unwrap_or(0);
    let high = white.root.get(index + 1).copied().unwrap_or(low);
    low + (((high - low) * fraction + (1 << (shift - 1))) >> shift)
}

/// D50 XYZ in s15.16 as CIE L*a*b* in s15.16 (8.6.5.4), each component's
/// ratio to the white in [`FRACTION`] bits through [`lab_f_fixed`].
fn xyz_to_lab_fixed(xyz: [i64; 3]) -> [i64; 3] {
    let white = lab_white();
    let f = |channel: usize| {
        let ratio = xyz[channel] * white.reciprocal[channel];
        lab_f_fixed(white, ratio)
    };
    let (fx, fy, fz) = (f(0), f(1), f(2));
    let shift = FRACTION - 16;
    [
        round_shift(116 * fy - 16 * UNIT, shift),
        round_shift(500 * (fx - fy), shift),
        round_shift(200 * (fy - fz), shift),
    ]
}

/// CIE L*a*b* in s15.16 as D50 XYZ in [`FRACTION`] bits (8.6.5.4 run
/// backwards): the cube in fixed point above `6/29`, the line below it.
fn lab_to_xyz_fixed(lab: [i64; 3]) -> [i64; 3] {
    const DELTA: f64 = 6.0 / 29.0;
    static LINE: OnceLock<(i64, i64, i64)> = OnceLock::new();
    let (delta, slope, offset) = *LINE.get_or_init(|| {
        (
            (DELTA * UNIT as f64).round() as i64,
            (3.0 * DELTA * DELTA * UNIT as f64).round() as i64,
            (4.0 / 29.0 * UNIT as f64).round() as i64,
        )
    });
    let white = lab_white();
    let widen = FRACTION - 16;
    let (l, a, b) = (lab[0] << widen, lab[1] << widen, lab[2] << widen);
    let fy = divided(l + 16 * UNIT, 116);
    let fx = fy + divided(a, 500);
    let fz = fy - divided(b, 200);
    let inverse = |t: i64| -> i64 {
        if t > delta {
            let square = (i128::from(t) * i128::from(t)) >> FRACTION;
            ((square * i128::from(t)) >> FRACTION) as i64
        } else {
            ((i128::from(t - offset) * i128::from(slope)) >> FRACTION) as i64
        }
    };
    let ratios = [inverse(fx), inverse(fy), inverse(fz)];
    [0, 1, 2].map(|channel| {
        ((i128::from(ratios[channel]) * i128::from(white.white[channel])) >> FRACTION) as i64
    })
}

/// `value` over `divisor`, rounded to the nearest, a half away from zero.
fn divided(value: i64, divisor: i64) -> i64 {
    let half = divisor / 2;
    if value >= 0 {
        (value + half) / divisor
    } else {
        (value - half) / divisor
    }
}

// ---- the display leg -------------------------------------------------------

/// IEC 61966-2-1's encode sampled at every 1/16 384 of linear light, the
/// display leg's last stage: its index rounded, the step `12.92 × 255 /
/// (2 × 16 384)`, a tenth of a level, at the steepest point of the curve.
fn srgb_encode_table() -> &'static [u8] {
    static TABLE: OnceLock<Box<[u8]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..=ENCODE_STEPS)
            .map(|index| crate::srgb_encode(index as f64 / ENCODE_STEPS as f64))
            .collect()
    })
}

/// Intervals [`srgb_encode_table`] divides linear light into.
const ENCODE_STEPS: i64 = 1 << 14;

/// Linear light in s15.16 as an sRGB byte.
fn encode16(linear: i64) -> u8 {
    let index = (linear.clamp(0, ONE) * ENCODE_STEPS + ONE / 2) / ONE;
    srgb_encode_table()
        .get(index as usize)
        .copied()
        .unwrap_or(0)
}

/// Linear light in [`FRACTION`] bits as an sRGB byte.
fn encode32(linear: i64) -> u8 {
    let index = (i128::from(linear.clamp(0, UNIT)) * i128::from(ENCODE_STEPS)
        + i128::from(UNIT / 2))
        >> FRACTION;
    srgb_encode_table()
        .get(index as usize)
        .copied()
        .unwrap_or(0)
}

/// What a proof's display leg converts through.
#[derive(Clone)]
enum View {
    /// A grey proof: each of the 256 device bytes' sRGB.
    Grey(Box<[[u8; 3]; 256]>),
    /// A matrix/TRC proof.
    Matrix {
        /// Each channel's curve at the 256 bytes, linear light in s15.16.
        curves: Box<[[i64; 256]; 3]>,
        /// The profile's columns, scaled by the media white where the paper
        /// is shown, times XYZ-to-sRGB, in s15.16.
        matrix: [[i64; 3]; 3],
    },
    /// A lookup-table proof of three or four channels.
    Table(Box<TableView>),
}

impl View {
    /// The kind of view, for a proof's `Debug`.
    fn kind(&self) -> &'static str {
        match self {
            View::Grey(_) => "grey",
            View::Matrix { .. } => "matrix",
            View::Table(_) => "table",
        }
    }

    /// The display leg of `profile`'s device of `channels` components at
    /// `display`'s intent: the first source ICC.1 8.10.2's order names for
    /// it among the profile's `A2B*` tables of that many inputs and its
    /// model, a table that does not read passed over; `None` where it
    /// carries none. A `D2B*` tag's elements are no display leg of this
    /// proof's.
    fn compile(profile: &Profile, display: ProofDisplay, channels: usize) -> Option<View> {
        let intent = display.intent();
        for step in intent.order() {
            match step {
                Step::Pipeline(_) => {}
                Step::Table(tag) => {
                    let Some((_, element)) = profile.tables.at(tag) else {
                        continue;
                    };
                    let Ok(lut) = profile.tables.read_table(element) else {
                        continue;
                    };
                    if lut.inputs != channels || lut.outputs != 3 {
                        continue;
                    }
                    // A version 5 profile's `A2B3` is ICC-absolute already.
                    let view = TableView::new(lut, profile.scale_for(intent, tag == 3));
                    return Some(if channels == 1 {
                        View::Grey(grey_table(|grey| view.display([grey, 0, 0, 0])))
                    } else {
                        View::Table(Box::new(view))
                    });
                }
                Step::Model => {
                    let media = profile.scale_for(intent, false);
                    match (&profile.model, channels) {
                        (Some(Model::MatrixTrc { columns, curves }), 3) => {
                            return Some(matrix_view(columns, curves, media))
                        }
                        (Some(Model::Grey { lab, curve }), 1) => {
                            return Some(View::Grey(grey_table(|grey| {
                                grey_model(*lab, curve, media, grey)
                            })))
                        }
                        _ => {}
                    }
                }
            }
        }
        None
    }
}

/// A grey proof's 256 colours, each `colour` of its byte.
fn grey_table(colour: impl Fn(u8) -> (u8, u8, u8)) -> Box<[[u8; 3]; 256]> {
    let mut table = Box::new([[0u8; 3]; 256]);
    for (grey, slot) in (0..=u8::MAX).zip(table.iter_mut()) {
        let (r, g, b) = colour(grey);
        *slot = [r, g, b];
    }
    table
}

/// A grey model's colour at the device byte `grey`, exactly: its curve's
/// value on the PCS white under XYZ, or as `L*` under Lab, scaled by `media`
/// in XYZ where the paper is shown, as sRGB.
fn grey_model(lab: bool, curve: &Curve, media: Option<[f64; 3]>, grey: u8) -> (u8, u8, u8) {
    let value = curve.eval(f64::from(grey) / 255.0);
    let xyz = if lab {
        crate::lab_to_xyz(value * 100.0, 0.0, 0.0)
    } else {
        PCS_WHITE.map(|white| white * value)
    };
    let scale = media.unwrap_or([1.0; 3]);
    let linear =
        crate::xyz_d50_to_linear_srgb(xyz[0] * scale[0], xyz[1] * scale[1], xyz[2] * scale[2]);
    let [r, g, b] = linear.map(crate::srgb_encode);
    (r, g, b)
}

/// A matrix/TRC model's display leg: each curve sampled at the 256 bytes in
/// s15.16, and `XYZ_D50_TO_SRGB` times the columns, each scaled by `media`
/// where the paper is shown, in s15.16 — as `Transform::compile` folds them.
fn matrix_view(columns: &[[f64; 3]; 3], curves: &[Curve; 3], media: Option<[f64; 3]>) -> View {
    let scale = media.unwrap_or([1.0; 3]);
    let mut matrix = [[0i64; 3]; 3];
    for (row, out) in matrix.iter_mut().enumerate() {
        for (channel, slot) in out.iter_mut().enumerate() {
            let entry: f64 = (0..3)
                .map(|axis| XYZ_D50_TO_SRGB[row][axis] * scale[axis] * columns[channel][axis])
                .sum();
            *slot = (entry * ONE as f64).round() as i64;
        }
    }
    let mut sampled = Box::new([[0i64; 256]; 3]);
    for (curve, table) in curves.iter().zip(sampled.iter_mut()) {
        for (byte, slot) in table.iter_mut().enumerate() {
            let linear = curve.eval(byte as f64 / 255.0).clamp(0.0, 1.0);
            *slot = (linear * ONE as f64).round() as i64;
        }
    }
    View::Matrix {
        curves: sampled,
        matrix,
    }
}

/// A matrix/TRC proof's display leg at `rgb`: each byte's light, the matrix
/// in s15.16, each channel encoded.
fn matrix_display(curves: &[[i64; 256]; 3], matrix: &[[i64; 3]; 3], rgb: [u8; 3]) -> (u8, u8, u8) {
    let linear = [0, 1, 2].map(|channel| curves[channel][usize::from(rgb[channel])]);
    let [r, g, b] = matrix.map(|row| {
        let sum: i64 = row
            .iter()
            .zip(linear)
            .map(|(entry, value)| entry * value)
            .sum();
        encode16(round_shift(sum, 16))
    });
    (r, g, b)
}

/// A lookup-table proof's display leg: the profile's `A2B*` table in
/// integers.
#[derive(Clone)]
struct TableView {
    lut: Arc<Lut>,
    /// Each input channel's curve at the 256 bytes, 16-bit.
    input: Vec<[u16; 256]>,
    /// An `mAB `'s matrix after its grid, nine entries and three offsets, in
    /// s15.16.
    matrix: Option<[i64; 12]>,
    /// The media white over the PCS white where the paper is shown, or the
    /// inverse where an ICC-absolute table shows media-relative colour, in
    /// [`FRACTION`] bits.
    media: Option<[i64; 3]>,
}

impl TableView {
    /// The display leg of `lut`, scaled in XYZ by `media` where that is given.
    fn new(lut: Arc<Lut>, media: Option<[f64; 3]>) -> TableView {
        let input = (0..lut.inputs)
            .map(|channel| {
                let table = lut.input_tables.get(channel).map_or(&[][..], Vec::as_slice);
                let mut sampled = [0u16; 256];
                for (byte, slot) in sampled.iter_mut().enumerate() {
                    *slot = interpolate(table, byte as f64 / 255.0);
                }
                sampled
            })
            .collect();
        let matrix = lut
            .matrix
            .filter(|_| lut.matrix_stage == MatrixStage::AfterGrid)
            .map(|entries| entries.map(|entry| (entry * ONE as f64).round() as i64));
        let media = media.map(|scale| scale.map(|factor| (factor * UNIT as f64).round() as i64));
        TableView {
            lut,
            input,
            matrix,
            media,
        }
    }

    /// The table's colour at the device bytes `bytes`, its first
    /// [`Lut::inputs`] read: input curves, grid, M curves and matrix where the
    /// table carries them, output curves, the connection space to XYZ, the
    /// media where it is given, and the matrix into sRGB.
    fn display(&self, bytes: [u8; 4]) -> (u8, u8, u8) {
        let lut = &self.lut;
        let mut coords = [0u32; 4];
        for (channel, slot) in coords.iter_mut().enumerate().take(lut.inputs.min(4)) {
            *slot = u32::from(
                self.input
                    .get(channel)
                    .map_or(0, |table| table[usize::from(bytes[channel])]),
            );
        }
        let mut values = [0u32; 3];
        for (channel, slot) in values.iter_mut().enumerate() {
            *slot = if lut.clut.is_empty() {
                coords.get(channel).copied().unwrap_or(0)
            } else {
                grid16(lut, &coords, channel)
            };
        }
        if lut.matrix_stage == MatrixStage::AfterGrid {
            if let Some(tables) = &lut.m_tables {
                for (channel, slot) in values.iter_mut().enumerate() {
                    if let Some(table) = tables.get(channel) {
                        *slot = through16(table, *slot);
                    }
                }
            }
            if let Some(matrix) = &self.matrix {
                values = matrix16(matrix, values);
            }
        }
        for (channel, slot) in values.iter_mut().enumerate() {
            if let Some(table) = lut.output_tables.get(channel) {
                *slot = through16(table, *slot);
            }
        }
        let mut xyz = if lut.lab {
            lab_to_xyz_fixed(lab_fixed(values, lut.lab_scale))
        } else {
            // `u1Fixed15`: 1.0 at `8000h`, so a value is its 32-bit fraction
            // shifted up by seventeen.
            values.map(|value| i64::from(value) << (FRACTION - 15))
        };
        if let Some(media) = &self.media {
            for (component, factor) in xyz.iter_mut().zip(media) {
                *component = ((i128::from(*component) * i128::from(*factor)) >> FRACTION) as i64;
            }
        }
        let [r, g, b] = xyz_to_srgb_matrix().map(|row| {
            let sum: i128 = row
                .iter()
                .zip(xyz)
                .map(|(entry, value)| i128::from(*entry) * i128::from(value))
                .sum();
            encode32((sum >> FRACTION) as i64)
        });
        (r, g, b)
    }
}

/// `XYZ_D50_TO_SRGB` in [`FRACTION`] bits.
fn xyz_to_srgb_matrix() -> &'static [[i64; 3]; 3] {
    static MATRIX: OnceLock<[[i64; 3]; 3]> = OnceLock::new();
    MATRIX.get_or_init(|| {
        XYZ_D50_TO_SRGB.map(|row| row.map(|entry| (entry * UNIT as f64).round() as i64))
    })
}

/// Channel `channel` of `lut`'s grid at 16-bit coordinates `coords`:
/// multilinear at the grid's own nodes, each axis's fraction of its cell in
/// 16 bits, one axis at a time, each step rounded to the nearest.
fn grid16(lut: &Lut, coords: &[u32; 4], channel: usize) -> u32 {
    let inputs = lut.inputs.min(4);
    let mut base = [0usize; 4];
    let mut fraction = [0u64; 4];
    for axis in 0..inputs {
        let points = lut.grids.get(axis).copied().unwrap_or(2).max(2);
        let last = points as u64 - 1;
        let position = u64::from(coords[axis].min(65_535)) * last;
        let index = ((position / 65_535) as usize).min(points - 2);
        let remainder = position - index as u64 * 65_535;
        base[axis] = index;
        fraction[axis] = (remainder * 65_536 + 32_767) / 65_535;
    }
    blend16(lut, inputs, 0, 0, channel, &base, &fraction).min(65_535) as u32
}

/// The grid interpolated over the axes from `axis` on, the axes before it
/// fixed at the node index `offset` they make: linearly along `axis` between
/// the cell's two nodes, its fraction in 16 bits.
fn blend16(
    lut: &Lut,
    inputs: usize,
    axis: usize,
    offset: usize,
    channel: usize,
    base: &[usize; 4],
    fraction: &[u64; 4],
) -> u64 {
    if axis == inputs {
        let at = offset * lut.outputs + channel;
        return u64::from(lut.clut.get(at).copied().unwrap_or(0));
    }
    let points = lut.grids.get(axis).copied().unwrap_or(2).max(2);
    let cell = base[axis];
    let low = blend16(
        lut,
        inputs,
        axis + 1,
        offset * points + cell,
        channel,
        base,
        fraction,
    );
    let weight = fraction[axis].min(65_536);
    if weight == 0 {
        return low;
    }
    let next = (cell + 1).min(points - 1);
    let high = blend16(
        lut,
        inputs,
        axis + 1,
        offset * points + next,
        channel,
        base,
        fraction,
    );
    (low * (65_536 - weight) + high * weight + 32_768) >> 16
}

/// A sampled curve at the 16-bit `value`, read linearly between its two
/// entries either side, the fraction in 16 bits; an empty table is the
/// identity.
fn through16(table: &[u16], value: u32) -> u32 {
    match table.len() {
        0 => value.min(65_535),
        1 => u32::from(table[0]),
        entries => {
            let last = entries as u64 - 1;
            let position = u64::from(value.min(65_535)) * last;
            let index = ((position / 65_535) as usize).min(entries - 2);
            let remainder = position - index as u64 * 65_535;
            let weight = ((remainder * 65_536 + 32_767) / 65_535) as i64;
            let (low, high) = (i64::from(table[index]), i64::from(table[index + 1]));
            (low + (((high - low) * weight + 32_768) >> 16)).clamp(0, 65_535) as u32
        }
    }
}

/// An `mAB `'s matrix and offsets (ICC.1 10.10.5) over 16-bit values, each
/// result held to the currency, as the clause clips it.
fn matrix16(matrix: &[i64; 12], values: [u32; 3]) -> [u32; 3] {
    [0usize, 1, 2].map(|row| {
        let sum: i64 = (0..3)
            .map(|column| matrix[3 * row + column] * i64::from(values[column]))
            .sum::<i64>()
            + matrix[9 + row] * 65_535;
        round_shift(sum, 16).clamp(0, 65_535) as u32
    })
}

/// A table's 16-bit connection-space values as CIE L*a*b* in s15.16, in the
/// table's own scale: `L*` 100 at `FF00h` and `a*`, `b*` 0 at `8000h` in
/// version 2's legacy encoding, `L*` 100 at `FFFFh` in the full one.
fn lab_fixed(values: [u32; 3], scale: LabScale) -> [i64; 3] {
    let top: i64 = match scale {
        LabScale::Legacy => 65_280,
        LabScale::Full => 65_535,
    };
    let of = |value: u32, range: i64| divided(i64::from(value) * range * ONE, top);
    [
        of(values[0], 100),
        of(values[1], 255) - 128 * ONE,
        of(values[2], 255) - 128 * ONE,
    ]
}
