//! Type 1 fonts: eexec decryption and Type 1 charstrings.
//!
//! A `/FontFile` in a PDF is a Type 1 program — the format Adobe shipped in
//! 1985 — and this crate could not read one. Worse than "not supported": the
//! bytes were handed to the sfnt and CFF parsers, both of which correctly
//! declined them, so an embedded Type 1 font **drew nothing at all** and said
//! nothing about why.
//!
//! Two layers of the same cipher stand in the way. The program's private half
//! is wrapped in *eexec* encryption, and inside it every charstring is
//! encrypted again with the same algorithm and a different key. Both are
//! trivially reversible — they are obfuscation, not security, and were
//! understood as such at the time.
//!
//! The charstring language is the ancestor of the Type 2 one in [`crate::cff`]
//! and differs in ways that matter: numbers encode differently past 255,
//! `hsbw` carries the side bearing that shifts every subsequent point, there
//! is no implicit width, and `flex` and `seac` arrive through a callback
//! protocol (`callothersubr`) rather than as operators.
//!
//! The scanner that finds `/Subrs` and `/CharStrings` entries keeps where
//! each one sits ([`Entry`]), and the reader decrypts what it finds, so
//! [`crate::type1_subset`] edits exactly the entries this reader draws
//! (`docs/design/subsetting.md`).

use std::collections::BTreeSet;
use std::ops::Range;

use crate::outline::{Outline, Segment};

/// The eexec key (Type 1 Font Format, 7.2).
pub const EEXEC_KEY: u16 = 55_665;
/// The charstring key (7.3).
pub const CHARSTRING_KEY: u16 = 4330;
/// How many plaintext bytes to discard after decrypting a charstring.
const LEN_IV: usize = 4;

/// How deep `callsubr` may nest.
const MAX_DEPTH: u32 = 10;
/// A bound on total operators, so a program that calls itself in a loop that
/// the depth cap does not catch still terminates.
const MAX_OPS: u32 = 100_000;
/// The operands the stack holds; a push past them is dropped.
const MAX_OPERANDS: usize = 48;
/// The most charstrings the scanner reads.
pub(crate) const MAX_GLYPHS: usize = 20_000;
/// The subroutine indices the reader stores.
const MAX_SUBRS: usize = 65_536;

/// The cipher's two constants (7.1).
const C1: u16 = 52_845;
const C2: u16 = 22_719;

/// Reverses Adobe's stream cipher (7.1).
///
/// `skip` drops the leading random bytes: four for eexec, `lenIV` for a
/// charstring. Both are there to randomize the first cipher state and carry no
/// information.
#[must_use]
pub fn decrypt(data: &[u8], key: u16, skip: usize) -> Vec<u8> {
    let mut r = key;
    let mut out = Vec::with_capacity(data.len().saturating_sub(skip));
    for (index, byte) in data.iter().enumerate() {
        let plain = byte ^ (r >> 8) as u8;
        r = (u16::from(*byte).wrapping_add(r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        if index >= skip {
            out.push(plain);
        }
    }
    out
}

/// The most masters a multiple-master program holds (Type 1 Font Format
/// Supplement, section 3.2).
pub const MAX_MASTERS: usize = 16;
/// The most design axes it has (section 3.2).
pub const MAX_AXES: usize = 4;
/// The most points one axis's `/BlendDesignMap` holds (section 3.5).
pub const MAX_MAP_POINTS: usize = 12;
/// How deep a cleartext array the multiple-master keys are read from may
/// nest: `/BlendDesignMap`'s three levels.
const MAX_ARRAY_DEPTH: usize = 3;
/// How many values, at every level, such an array may hold: four axes of
/// twelve points is 148, sixteen masters of four coordinates 80.
const MAX_ARRAY_ITEMS: usize = 256;

/// Adobe's stream cipher in the encrypting direction (7.1): `lead` first —
/// the random bytes a reader discards — then `plain`.
///
/// The one copy of it in the repository: [`crate::type1_subset`] writes both
/// layers through it, and every test that builds a Type 1 program builds it
/// with this.
#[must_use]
pub fn encrypt(plain: &[u8], key: u16, lead: &[u8]) -> Vec<u8> {
    let mut r = key;
    let mut out = Vec::with_capacity(lead.len() + plain.len());
    for byte in lead.iter().chain(plain) {
        let cipher = byte ^ (r >> 8) as u8;
        r = (u16::from(cipher).wrapping_add(r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        out.push(cipher);
    }
    out
}

/// A parsed Type 1 font program.
#[derive(Clone, Debug)]
pub struct Type1<'a> {
    /// Charstrings by glyph name, in the order the font declared them.
    glyphs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Local subroutines, by index.
    subrs: Vec<Vec<u8>>,
    /// `/Encoding`, when the font carries a built-in one: code to glyph name.
    encoding: Vec<(u8, Vec<u8>)>,
    /// Whether `/Encoding` is the token `StandardEncoding` (chapter 10.3),
    /// which assigns the Adobe Standard Encoding as the built-in one.
    standard: bool,
    /// `/FontMatrix`, which is usually 1/1000 but need not be.
    pub font_matrix: [f64; 6],
    /// `/WeightVector`, where the program is a multiple-master one: its
    /// default instance (the supplement's section 3.4).
    weight_vector: Option<Vec<f64>>,
    /// The blend design, where the program states one (section 3.5).
    blend: Option<Blend>,
    /// The weights the blend othersubrs blend with: the default instance's
    /// until [`Type1::instance`] sets another.
    weights: Option<Vec<f64>>,
    /// What the cleartext's `/FontBBox` and `/FontInfo` and the Private
    /// dictionary state of the program's style.
    style: Style,
    _marker: core::marker::PhantomData<&'a ()>,
}

/// What a Type 1 program states of its style ([`Type1::style`]): the font
/// dictionary's `/FontBBox`, `/FontInfo`'s entries (the Type 1 book's
/// chapter 2) and the Private dictionary's `/ForceBold` and `/StdVW`
/// (chapter 5), the entries the font flags and the font measurement API read
/// (`docs/design/font-metrics.md`).
///
/// **An absent entry reads as TN #5176 Table 9's default for the same key.**
/// The Type 1 book states no default for one — `/FontBBox` is required, and
/// section 2.3 allows it all zeros; the `/FontInfo` entries are optional —
/// and CFF, which carries Type 1's keys over with their names, states one
/// for each.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// `/FontBBox`, four numbers in an array or a procedure: `0 0 0 0` where
    /// it is absent or is not four finite numbers.
    pub font_bbox: [f64; 4],
    /// `/FontInfo`'s `/isFixedPitch true`: false where absent.
    pub is_fixed_pitch: bool,
    /// `/FontInfo`'s `/ItalicAngle`, degrees counterclockwise from the
    /// vertical: 0 where absent.
    pub italic_angle: f64,
    /// `/FontInfo`'s `/UnderlinePosition`: -100 where absent.
    pub underline_position: f64,
    /// `/FontInfo`'s `/UnderlineThickness`: 50 where absent.
    pub underline_thickness: f64,
    /// `/FontInfo`'s `/Weight`, a string: `None` where absent, not a
    /// string, or not UTF-8.
    pub weight: Option<String>,
    /// The Private dictionary's `/ForceBold true` (section 5.10): false where
    /// absent.
    pub force_bold: bool,
    /// The Private dictionary's `/StdVW`, an array of one number (section
    /// 5.9): the dominant vertical stem's width in character space.
    pub std_vw: Option<f64>,
}

impl Default for Style {
    /// Every entry absent: Table 9's defaults.
    fn default() -> Style {
        Style {
            font_bbox: [0.0; 4],
            is_fixed_pitch: false,
            italic_angle: 0.0,
            underline_position: -100.0,
            underline_thickness: 50.0,
            weight: None,
            force_bold: false,
            std_vw: None,
        }
    }
}

/// A multiple-master program's blend design, as the supplement's section 3.5
/// has `/FontInfo` state it.
#[derive(Clone, Debug, PartialEq)]
pub struct Blend {
    /// `/BlendAxisTypes`: one name an axis, `Weight`, `Width`,
    /// `OpticalSize` and the like.
    pub axes: Vec<Vec<u8>>,
    /// `/BlendDesignPositions`: each master's place in the blend space, one
    /// coordinate an axis, in the order the program holds its masters.
    pub positions: Vec<Vec<f64>>,
    /// `/BlendDesignMap`: each axis's design-to-blend points `(D, B)`.
    pub maps: Vec<Vec<(f64, f64)>>,
}

/// The weights an instance blends its masters with, and whether a design
/// coordinate was past its axis's map.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// One weight a master, in the program's order.
    pub weights: Vec<f64>,
    /// Whether a coordinate lay outside its map and was taken at the map's
    /// end.
    pub clamped: bool,
}

/// Why a design vector was not instanced; the program keeps drawing the
/// instance it drew (`docs/design/type42-and-multiple-master.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceRefusal {
    /// The program has no `/WeightVector` of two to sixteen numbers, so it is
    /// no multiple-master program this crate blends.
    NotMultipleMaster,
    /// Its blend design — `/BlendAxisTypes`, `/BlendDesignPositions` and
    /// `/BlendDesignMap` — is missing, or past the supplement's limits.
    NoDesign,
    /// Its blend design is not the supplement's standard form: one to four
    /// axes, `2^n` masters each at its own corner of the blend space, as many
    /// as `/WeightVector` holds, and each axis's map two to twelve points
    /// rising in design coordinates with blend coordinates from 0 to 1.
    NotStandard,
    /// The design vector does not hold one coordinate an axis.
    AxisCount {
        /// How many axes the program has.
        axes: usize,
        /// How many coordinates were given.
        given: usize,
    },
    /// A design coordinate is not a finite number.
    NotFinite,
}

impl Blend {
    /// How many axes the design has.
    #[must_use]
    pub fn axis_count(&self) -> usize {
        self.axes.len()
    }

    /// How many masters it places.
    #[must_use]
    pub fn master_count(&self) -> usize {
        self.positions.len()
    }

    /// Whether the design is the supplement's standard form, the one sections
    /// 3.8 and 3.9 give the arithmetic for: one to four axes, the same number
    /// of maps and of coordinates in every position, `2^n` masters each at a
    /// corner of the blend space no other holds, and each map two to twelve
    /// points, its design coordinates rising strictly and its blend
    /// coordinates from 0 to 1, every number finite.
    #[must_use]
    pub fn is_standard(&self) -> bool {
        let axes = self.axes.len();
        if axes == 0 || axes > MAX_AXES || self.maps.len() != axes {
            return false;
        }
        if self.positions.len() != 1 << axes {
            return false;
        }
        let mut corners = 0u32;
        for position in &self.positions {
            if position.len() != axes {
                return false;
            }
            let mut corner = 0u32;
            for (axis, coordinate) in position.iter().enumerate() {
                if *coordinate == 1.0 {
                    corner |= 1 << axis;
                } else if *coordinate != 0.0 {
                    return false;
                }
            }
            if corners & (1 << corner) != 0 {
                return false;
            }
            corners |= 1 << corner;
        }
        self.maps.iter().all(|map| {
            (2..=MAX_MAP_POINTS).contains(&map.len())
                && map
                    .iter()
                    .all(|(d, b)| d.is_finite() && (0.0..=1.0).contains(b))
                && map.windows(2).all(|pair| pair[0].0 < pair[1].0)
        })
    }

    /// One axis's blend coordinate for a design coordinate, and whether the
    /// coordinate was past the map and taken at its end (section 3.8,
    /// Appendix C): on the segment `D_i ≤ d ≤ D_(i+1)`,
    /// `B_i + (d − D_i) × (B_(i+1) − B_i) ÷ (D_(i+1) − D_i)`, a map's own
    /// point answering its own blend coordinate. `None` for an axis the
    /// design does not have or a map with no point.
    #[must_use]
    pub fn normalise(&self, axis: usize, design: f64) -> Option<(f64, bool)> {
        let map = self.maps.get(axis)?;
        let first = *map.first()?;
        let last = *map.last()?;
        if design <= first.0 {
            return Some((first.1, design < first.0));
        }
        if design >= last.0 {
            return Some((last.1, design > last.0));
        }
        for pair in map.windows(2) {
            let ((d0, b0), (d1, b1)) = (pair[0], pair[1]);
            if design == d1 {
                return Some((b1, false));
            }
            if design < d1 {
                return Some((b0 + (design - d0) * (b1 - b0) / (d1 - d0), false));
            }
        }
        Some((last.1, false))
    }

    /// The weight vector at a design vector, one coordinate an axis, by the
    /// supplement's arithmetic: each coordinate normalised through its map,
    /// and master `i` at corner `p_i` weighing `Π_j (p_ij = 1 ? b_j : 1 − b_j)`,
    /// the product taken over the axes in order (section 3.9, Appendix D).
    ///
    /// # Errors
    ///
    /// [`InstanceRefusal::NotStandard`] for a design that is not the
    /// standard form, [`InstanceRefusal::AxisCount`] for a vector of another
    /// length, [`InstanceRefusal::NotFinite`] for a coordinate that is no
    /// number.
    pub fn instance(&self, design: &[f64]) -> Result<Instance, InstanceRefusal> {
        if !self.is_standard() {
            return Err(InstanceRefusal::NotStandard);
        }
        if design.len() != self.axes.len() {
            return Err(InstanceRefusal::AxisCount {
                axes: self.axes.len(),
                given: design.len(),
            });
        }
        if !design.iter().all(|d| d.is_finite()) {
            return Err(InstanceRefusal::NotFinite);
        }
        let mut blend = Vec::with_capacity(design.len());
        let mut clamped = false;
        for (axis, coordinate) in design.iter().enumerate() {
            let (b, past) = self
                .normalise(axis, *coordinate)
                .ok_or(InstanceRefusal::NotStandard)?;
            blend.push(b);
            clamped |= past;
        }
        let weights = self
            .positions
            .iter()
            .map(|position| {
                position
                    .iter()
                    .zip(&blend)
                    .fold(1.0, |weight, (corner, b)| {
                        weight * if *corner == 1.0 { *b } else { 1.0 - b }
                    })
            })
            .collect();
        Ok(Instance { weights, clamped })
    }
}

/// A program's portions as the reader finds them (chapter 2; ISO 32000-2
/// Table 125): where the clear text ends and the encrypted portion begins,
/// and the private section decrypted.
pub(crate) struct Layout<'d> {
    /// The program, PFB headers and all.
    pub(crate) data: &'d [u8],
    /// Where the clear text ends: the first `eexec`'s first byte.
    pub(crate) eexec: usize,
    /// Where the encrypted portion begins: after `eexec` and the white space
    /// after it.
    pub(crate) encrypted_at: usize,
    /// Whether the reader takes the encrypted portion as hexadecimal: its
    /// first four bytes are all hexadecimal digits.
    pub(crate) hex: bool,
    /// How many bytes of ciphertext the reader decrypted.
    pub(crate) cipher_len: usize,
    /// Everything after the encrypted portion's start, decrypted, its four
    /// lead bytes dropped.
    pub(crate) private: Vec<u8>,
    /// `/lenIV` as written, before the reader clamps it.
    pub(crate) len_iv: Option<i64>,
}

impl Layout<'_> {
    /// Finds the portions, or `None` for a program with no `eexec`.
    pub(crate) fn read(data: &[u8]) -> Option<Layout<'_>> {
        let data = strip_pfb(data);

        // The cleartext half ends at `eexec`; everything after it is
        // encrypted. A font with no `eexec` at all is not one this can read.
        let at = find(data, b"eexec")?;
        let rest = &data[at + 5..];

        // Whitespace between the keyword and the ciphertext is not part of it.
        let start = rest
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(0);
        let encrypted_at = at + 5 + start;
        let encrypted = &rest[start..];

        // PFA stores the private half as hexadecimal. Four bytes are enough to
        // tell: real ciphertext is binary and almost never all hex digits.
        let hex = encrypted.iter().take(4).all(u8::is_ascii_hexdigit);
        let binary;
        let cipher = if hex {
            binary = unhex(encrypted);
            &binary[..]
        } else {
            encrypted
        };

        let private = decrypt(cipher, EEXEC_KEY, 4);
        let len_iv = read_int_after(&private, b"/lenIV");
        Some(Layout {
            data,
            eexec: at,
            encrypted_at,
            hex,
            cipher_len: cipher.len(),
            private,
            len_iv,
        })
    }

    /// `/lenIV` as the reader uses it: 4 when absent, clamped to 0 to 16.
    pub(crate) fn len_iv(&self) -> usize {
        self.len_iv.unwrap_or(LEN_IV as i64).clamp(0, 16) as usize
    }
}

impl Type1<'_> {
    /// Reads a Type 1 program: PFB, PFA, or the bare bytes a PDF embeds.
    #[must_use]
    pub fn parse(data: &[u8]) -> Option<Type1<'static>> {
        Type1::parse_with(data, &mut 0)
    }

    /// [`Type1::parse`], and the steps it took
    /// (`docs/design/resource-exhaustion.md`, "The counters"): a byte for each
    /// one of the `eexec` section decrypted, each byte of the private section
    /// the reading of `/CharStrings` and `/Subrs` walked, and each byte of a
    /// charstring or a subroutine decrypted.
    #[must_use]
    pub fn parse_counted(data: &[u8]) -> (Option<Type1<'static>>, u64) {
        let mut steps = 0u64;
        let font = Type1::parse_with(data, &mut steps);
        (font, steps)
    }

    fn parse_with(data: &[u8], steps: &mut u64) -> Option<Type1<'static>> {
        let layout = Layout::read(data)?;
        step(steps, layout.cipher_len);
        let len_iv = layout.len_iv();
        let clear = &layout.data[..layout.eexec];

        let weight_vector = read_weight_vector(clear);
        Some(Type1 {
            glyphs: read_charstrings(&layout.private, len_iv, steps),
            subrs: read_subrs(&layout.private, len_iv, steps),
            encoding: read_encoding(clear),
            standard: says_standard_encoding(clear),
            font_matrix: read_font_matrix(clear).unwrap_or([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]),
            blend: read_blend(clear),
            weights: weight_vector.clone(),
            weight_vector,
            style: read_style(clear, &layout.private),
            _marker: core::marker::PhantomData,
        })
    }

    /// What the program states of its style, each absent entry its default
    /// ([`Style`]).
    #[must_use]
    pub fn style(&self) -> &Style {
        &self.style
    }

    /// Whether this is a multiple-master program this crate blends: one with
    /// a `/WeightVector` of two to sixteen numbers (the supplement's section
    /// 3.4). Any other program's blend othersubrs, 14 to 18, are othersubrs
    /// it does not know, which hand `pop` their arguments — the first
    /// master's values — as the Type 1 book's chapter 8 says.
    #[must_use]
    pub fn is_multiple_master(&self) -> bool {
        self.weight_vector.is_some()
    }

    /// The program's own `/WeightVector`: its default instance.
    #[must_use]
    pub fn weight_vector(&self) -> Option<&[f64]> {
        self.weight_vector.as_deref()
    }

    /// The weights the program draws with now: its `/WeightVector`, or the
    /// last instance [`Type1::instance`] set.
    #[must_use]
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }

    /// The program's blend design, where it states one this crate reads.
    #[must_use]
    pub fn blend(&self) -> Option<&Blend> {
        self.blend.as_ref()
    }

    /// Draws the program at the instance a design vector names, one design
    /// coordinate an axis, from now on: its outlines and advances are the
    /// masters blended with the weights [`Blend::instance`] computes.
    ///
    /// # Errors
    ///
    /// The [`InstanceRefusal`] that kept the instance the program drew,
    /// which is then unchanged: its default, unless an earlier call set
    /// another.
    pub fn instance(&mut self, design: &[f64]) -> Result<Instance, InstanceRefusal> {
        let masters = self
            .weight_vector
            .as_ref()
            .ok_or(InstanceRefusal::NotMultipleMaster)?
            .len();
        let blend = self.blend.as_ref().ok_or(InstanceRefusal::NoDesign)?;
        if blend.master_count() != masters {
            return Err(InstanceRefusal::NotStandard);
        }
        let instance = blend.instance(design)?;
        self.weights = Some(instance.weights.clone());
        Ok(instance)
    }

    /// Draws the program at its default instance again, its `/WeightVector`.
    pub fn default_instance(&mut self) {
        self.weights = self.weight_vector.clone();
    }

    /// How many glyphs the font defines.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// The name of a glyph, by index.
    #[must_use]
    pub fn glyph_name(&self, index: usize) -> Option<&[u8]> {
        self.glyphs.get(index).map(|(name, _)| name.as_slice())
    }

    /// The index of a glyph, by name.
    #[must_use]
    pub fn glyph_for_name(&self, name: &[u8]) -> Option<u16> {
        self.glyphs
            .iter()
            .position(|(n, _)| n == name)
            .and_then(|i| u16::try_from(i).ok())
    }

    /// The glyph a character code selects through the font's built-in
    /// `/Encoding`, when it has one: its `dup <code> /<name> put` entries, or
    /// the Adobe Standard Encoding where the program says `/Encoding
    /// StandardEncoding` (chapter 10.3).
    #[must_use]
    pub fn glyph_for_code(&self, code: u8) -> Option<u16> {
        let name = self
            .encoding
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, n)| n.as_slice());
        match name {
            Some(name) => self.glyph_for_name(name),
            None if self.standard => {
                let name = crate::encoding::base_glyph_name(crate::BaseEncoding::Standard, code)?;
                self.glyph_for_name(name.as_bytes())
            }
            None => None,
        }
    }

    /// The outline of one glyph, in font units.
    #[must_use]
    pub fn outline(&self, glyph: u16) -> Option<Outline> {
        self.outline_counted(glyph).0
    }

    /// [`Type1::outline`], and the steps it took: each operand and operator
    /// the charstring machine ran, in the glyph's charstring, the subroutines
    /// it called and, for a `seac`, both glyphs it is built from.
    #[must_use]
    pub fn outline_counted(&self, glyph: u16) -> (Option<Outline>, u64) {
        let Some((_, charstring)) = self.glyphs.get(usize::from(glyph)) else {
            return (None, 0);
        };
        let mut run = Run::new(self, None);
        run.execute(charstring, 0);
        run.close();
        let steps = u64::from(run.ops);

        // `seac` builds a glyph from two others, and can only be resolved once
        // the base charstring has finished telling us which two.
        if let Some((base, accent, adx, ady)) = run.seac {
            let (outline, composed) = self.compose(base, accent, adx, ady);
            return (outline, steps.saturating_add(composed));
        }
        (Some(run.outline), steps)
    }

    /// The advance width of a glyph, from its `hsbw`.
    #[must_use]
    pub fn advance(&self, glyph: u16) -> Option<f64> {
        let (_, charstring) = self.glyphs.get(usize::from(glyph))?;
        let mut run = Run::new(self, None);
        run.execute(charstring, 0);
        Some(run.width)
    }

    /// The charstring of the glyph at `index`, decrypted, as this reader runs
    /// it: what [`crate::type1_to_cff`] converts.
    pub(crate) fn charstring(&self, index: usize) -> Option<&[u8]> {
        self.glyphs.get(index).map(|(_, code)| code.as_slice())
    }

    /// The subroutine the reader stores at `index`, decrypted: empty at an
    /// index below the highest that the program does not define.
    pub(crate) fn subr(&self, index: usize) -> Option<&[u8]> {
        self.subrs.get(index).map(Vec::as_slice)
    }

    /// A glyph's run as [`crate::type1_subset`] reads it: the subroutines it
    /// called, the hint replacements it asked for, its `seac` components and
    /// the first bound it met, and the steps it took. `None` for a glyph the
    /// font does not have.
    pub(crate) fn trace_glyph(&self, glyph: u16) -> Option<(Trace, u64)> {
        let (_, charstring) = self.glyphs.get(usize::from(glyph))?;
        Some(self.trace(charstring, 0))
    }

    /// [`Type1::trace_glyph`] for a subroutine, run as a `callsubr` from a
    /// charstring would run it: at depth 1. `None` for an index the program
    /// stores nothing at.
    pub(crate) fn trace_subr(&self, index: usize) -> Option<(Trace, u64)> {
        let code = self.subrs.get(index)?;
        Some(self.trace(code, 1))
    }

    /// Runs `code` at `depth`, tracing.
    fn trace(&self, code: &[u8], depth: u32) -> (Trace, u64) {
        let mut run = Run::new(self, Some(Trace::default()));
        run.execute(code, depth);
        let steps = u64::from(run.ops);
        let mut trace = run.trace.unwrap_or_default();
        trace.seac = run.seac.map(|(base, accent, _, _)| (base, accent));
        (trace, steps)
    }

    /// A `seac` glyph from its two components, and the steps their outlines
    /// took.
    fn compose(&self, base: u8, accent: u8, adx: f64, ady: f64) -> (Option<Outline>, u64) {
        let mut steps = 0u64;
        let composed = self.compose_with(base, accent, adx, ady, &mut steps);
        (composed, steps)
    }

    fn compose_with(
        &self,
        base: u8,
        accent: u8,
        adx: f64,
        ady: f64,
        steps: &mut u64,
    ) -> Option<Outline> {
        // The two components are named by their StandardEncoding codes, which
        // is the one place this format assumes a specific encoding.
        let base_name = crate::encoding::base_glyph_name(crate::BaseEncoding::Standard, base)?;
        let accent_name = crate::encoding::base_glyph_name(crate::BaseEncoding::Standard, accent)?;

        let (outline, taken) = self.component(self.glyph_for_name(base_name.as_bytes())?);
        *steps = steps.saturating_add(taken);
        let mut outline = outline?;
        let (accent, taken) = self.component(self.glyph_for_name(accent_name.as_bytes())?);
        *steps = steps.saturating_add(taken);
        let accent = accent?;

        for segment in accent.segments {
            outline.segments.push(shift(segment, adx, ady));
        }
        Some(outline)
    }

    /// One component of a `seac`: its own charstring, never composed again.
    ///
    /// Chapter 6 builds an accented character from two characters of the
    /// StandardEncoding, none of which is itself accented, so a component
    /// that is a `seac` is drawn as its own charstring. Following it would be
    /// unbounded: a `seac` that names itself recursed until the thread's
    /// stack ran out.
    fn component(&self, glyph: u16) -> (Option<Outline>, u64) {
        let Some((_, charstring)) = self.glyphs.get(usize::from(glyph)) else {
            return (None, 0);
        };
        let mut run = Run::new(self, None);
        run.execute(charstring, 0);
        run.close();
        (Some(run.outline), u64::from(run.ops))
    }
}

fn shift(segment: Segment, dx: f64, dy: f64) -> Segment {
    match segment {
        Segment::MoveTo { x, y } => Segment::MoveTo {
            x: x + dx,
            y: y + dy,
        },
        Segment::LineTo { x, y } => Segment::LineTo {
            x: x + dx,
            y: y + dy,
        },
        Segment::QuadTo { cx, cy, x, y } => Segment::QuadTo {
            cx: cx + dx,
            cy: cy + dy,
            x: x + dx,
            y: y + dy,
        },
        Segment::CurveTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        } => Segment::CurveTo {
            c1x: c1x + dx,
            c1y: c1y + dy,
            c2x: c2x + dx,
            c2y: c2y + dy,
            x: x + dx,
            y: y + dy,
        },
        Segment::Close => Segment::Close,
    }
}

/// What one charstring's run reached, as [`crate::type1_subset`] reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Trace {
    /// Every subroutine index a `callsubr` called, whether or not the program
    /// stores one there.
    pub(crate) called: BTreeSet<usize>,
    /// Every `subr#` handed to OtherSubrs entry 3 (chapter 8.1), which an
    /// interpreter that replaces hints calls where this one calls entry 3.
    pub(crate) hints: BTreeSet<usize>,
    /// The `seac` components, as StandardEncoding codes.
    pub(crate) seac: Option<(u8, u8)>,
    /// The first bound the run met.
    pub(crate) bound: Option<Bound>,
}

/// A bound a Type 1 run meets, where what it draws is this reader's rather
/// than the program's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    /// A call past ten levels deep (chapter 6, *callsubr*), which does not
    /// run.
    Depth,
    /// The operator budget, past which nothing runs.
    Budget,
    /// More operands than the stack holds, the excess dropped.
    Stack,
    /// A `callsubr` naming no subroutine the program stores: a negative index,
    /// or one past the array.
    Missing,
}

/// One charstring in flight.
struct Run<'f, 'a> {
    font: &'f Type1<'a>,
    outline: Outline,
    stack: Vec<f64>,
    /// The PostScript operand stack `callothersubr` and `pop` communicate over.
    ps_stack: Vec<f64>,
    x: f64,
    y: f64,
    open: bool,
    width: f64,
    left_side_bearing: f64,
    /// Points collected between the flex othersubrs.
    flex: Vec<(f64, f64)>,
    in_flex: bool,
    ops: u32,
    seac: Option<(u8, u8, f64, f64)>,
    /// What a subsetter asked to learn of the run; `None` when drawing.
    trace: Option<Trace>,
}

impl<'f, 'a> Run<'f, 'a> {
    fn new(font: &'f Type1<'a>, trace: Option<Trace>) -> Run<'f, 'a> {
        Run {
            font,
            outline: Outline::default(),
            stack: Vec::new(),
            ps_stack: Vec::new(),
            x: 0.0,
            y: 0.0,
            open: false,
            width: 0.0,
            left_side_bearing: 0.0,
            flex: Vec::new(),
            in_flex: false,
            ops: 0,
            seac: None,
            trace,
        }
    }
}

impl Run<'_, '_> {
    /// Records the first bound the run meets, when it is being traced.
    fn meet(&mut self, bound: Bound) {
        if let Some(trace) = self.trace.as_mut() {
            trace.bound.get_or_insert(bound);
        }
    }

    fn close(&mut self) {
        if self.open {
            self.outline.segments.push(Segment::Close);
            self.open = false;
        }
    }

    fn move_to(&mut self, x: f64, y: f64) {
        if self.in_flex {
            // Between othersubr 1 and 0 the rmovetos are the flex control
            // points rather than real moves.
            self.flex.push((x, y));
            self.x = x;
            self.y = y;
            return;
        }
        self.close();
        self.outline.segments.push(Segment::MoveTo { x, y });
        self.open = true;
        self.x = x;
        self.y = y;
    }

    fn line_to(&mut self, x: f64, y: f64) {
        self.outline.segments.push(Segment::LineTo { x, y });
        self.x = x;
        self.y = y;
    }

    fn curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        self.outline.segments.push(Segment::CurveTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        });
        self.x = x;
        self.y = y;
    }

    fn execute(&mut self, code: &[u8], depth: u32) {
        if depth > MAX_DEPTH {
            self.meet(Bound::Depth);
            return;
        }

        let mut i = 0usize;
        while i < code.len() {
            self.ops += 1;
            if self.ops > MAX_OPS {
                self.meet(Bound::Budget);
                return;
            }

            let b = code[i];
            i += 1;

            // Type 1 numbers: 32..246 are one byte, 247..254 two, 255 a
            // 32-bit integer. Note the difference from Type 2, where 255 is a
            // 16.16 fixed-point value — reading one as the other scales every
            // coordinate by 65536.
            if b >= 32 {
                let value = if b <= 246 {
                    f64::from(i16::from(b) - 139)
                } else if b <= 250 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from((i16::from(b) - 247) * 256 + i16::from(w) + 108)
                } else if b <= 254 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from(-((i16::from(b) - 251) * 256) - i16::from(w) - 108)
                } else {
                    let mut v = 0i32;
                    for _ in 0..4 {
                        v = (v << 8) | i32::from(*code.get(i).unwrap_or(&0));
                        i += 1;
                    }
                    f64::from(v)
                };
                if self.stack.len() < MAX_OPERANDS {
                    self.stack.push(value);
                } else {
                    self.meet(Bound::Stack);
                }
                continue;
            }

            match b {
                // hstem, vstem: hints, which this build does not use.
                1 | 3 => self.stack.clear(),
                4 => {
                    // vmoveto
                    let dy = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x, self.y + dy);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                5 => {
                    // rlineto
                    let dy = self.stack.pop().unwrap_or(0.0);
                    let dx = self.stack.pop().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y + dy);
                    self.line_to(x, y);
                    self.stack.clear();
                }
                6 => {
                    // hlineto
                    let dx = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y);
                    self.line_to(x, y);
                    self.stack.clear();
                }
                7 => {
                    // vlineto
                    let dy = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x, self.y + dy);
                    self.line_to(x, y);
                    self.stack.clear();
                }
                8 => {
                    // rrcurveto
                    let v = self.take(6);
                    let c1 = (self.x + v[0], self.y + v[1]);
                    let c2 = (c1.0 + v[2], c1.1 + v[3]);
                    let end = (c2.0 + v[4], c2.1 + v[5]);
                    self.curve_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                    self.stack.clear();
                }
                9 => {
                    self.close();
                    self.stack.clear();
                }
                10 => {
                    // callsubr
                    let index = self.stack.pop().unwrap_or(0.0);
                    // Flex and hint replacement are implemented as subroutine
                    // calls in every Type 1 font, and subrs 0..3 are reserved
                    // for them by convention.
                    let target = usize::try_from(index as i64).ok();
                    if let Some(trace) = self.trace.as_mut() {
                        if let Some(target) = target {
                            trace.called.insert(target);
                        }
                    }
                    match target.and_then(|i| self.font.subrs.get(i)) {
                        Some(subr) => {
                            let subr = subr.clone();
                            self.execute(&subr, depth + 1);
                        }
                        None => self.meet(Bound::Missing),
                    }
                }
                11 => return, // return
                13 => {
                    // hsbw: side bearing and width. The side bearing shifts
                    // the origin, so every later point is relative to it.
                    let width = self.stack.pop().unwrap_or(0.0);
                    let sbx = self.stack.pop().unwrap_or(0.0);
                    self.width = width;
                    self.left_side_bearing = sbx;
                    self.x = sbx;
                    self.y = 0.0;
                    self.stack.clear();
                }
                14 => return, // endchar
                21 => {
                    // rmoveto
                    let dy = self.stack.pop().unwrap_or(0.0);
                    let dx = self.stack.pop().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y + dy);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                22 => {
                    // hmoveto
                    let dx = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                30 => {
                    // vhcurveto
                    let v = self.take(4);
                    let c1 = (self.x, self.y + v[0]);
                    let c2 = (c1.0 + v[1], c1.1 + v[2]);
                    let end = (c2.0 + v[3], c2.1);
                    self.curve_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                    self.stack.clear();
                }
                31 => {
                    // hvcurveto
                    let v = self.take(4);
                    let c1 = (self.x + v[0], self.y);
                    let c2 = (c1.0 + v[1], c1.1 + v[2]);
                    let end = (c2.0, c2.1 + v[3]);
                    self.curve_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                    self.stack.clear();
                }
                12 => {
                    let b2 = *code.get(i).unwrap_or(&0);
                    i += 1;
                    self.escape(b2);
                }
                _ => self.stack.clear(),
            }
        }
    }

    /// The two-byte operators.
    fn escape(&mut self, op: u8) {
        match op {
            // dotsection, vstem3, hstem3: hints, which this build does not use.
            0..=2 => self.stack.clear(),
            6 => {
                // seac: an accented character built from two StandardEncoding
                // glyphs. Recorded rather than acted on, because the base
                // charstring has to finish first.
                let v = self.take(5);
                self.seac = Some((v[3] as u8, v[4] as u8, v[1], v[2]));
                self.stack.clear();
            }
            7 => {
                // sbw: the two-dimensional form of hsbw.
                let v = self.take(4);
                self.left_side_bearing = v[0];
                self.width = v[2];
                self.x = v[0];
                self.y = v[1];
                self.stack.clear();
            }
            12 => {
                // div
                let b = self.stack.pop().unwrap_or(1.0);
                let a = self.stack.pop().unwrap_or(0.0);
                self.stack.push(if b == 0.0 { 0.0 } else { a / b });
            }
            16 => self.othersubr(),
            17 => {
                // pop: takes a value the othersubr left behind.
                let value = self.ps_stack.pop().unwrap_or(0.0);
                self.stack.push(value);
            }
            33 => {
                // setcurrentpoint
                let v = self.take(2);
                self.x = v[0];
                self.y = v[1];
                self.stack.clear();
            }
            _ => self.stack.clear(),
        }
    }

    /// `callothersubr`: the callback protocol flex and hint replacement use.
    ///
    /// The interpreter is expected to know othersubrs 0 to 3 itself; a font
    /// that defines others is asking for a PostScript interpreter, which this
    /// is not — except for a multiple-master program's blend othersubrs, 14 to
    /// 18, which [`Run::blend`] answers without one.
    fn othersubr(&mut self) {
        let index = self.stack.pop().unwrap_or(0.0) as i64;
        let count = self.stack.pop().unwrap_or(0.0).max(0.0) as usize;

        // The arguments were pushed before the count and index.
        let at = self.stack.len().saturating_sub(count);
        let args: Vec<f64> = self.stack.split_off(at);

        if (14..=18).contains(&index) {
            if let Some(results) = self.blend(index, count, &args) {
                // `pop` takes the results in the order of their quantities,
                // `a` first: the last pushed is the first taken.
                for value in results.into_iter().rev() {
                    self.ps_stack.push(value);
                }
                return;
            }
        }

        match index {
            0 => {
                // End of flex: seven collected points become two curves.
                self.in_flex = false;
                if self.flex.len() >= 7 {
                    let p: Vec<(f64, f64)> = self.flex[self.flex.len() - 7..].to_vec();
                    self.curve_to(p[1].0, p[1].1, p[2].0, p[2].1, p[3].0, p[3].1);
                    self.curve_to(p[4].0, p[4].1, p[5].0, p[5].1, p[6].0, p[6].1);
                } else if let Some(last) = self.flex.last().copied() {
                    // A malformed flex still has to leave the pen somewhere.
                    self.line_to(last.0, last.1);
                }
                self.flex.clear();
                // The two values `pop` will ask for are the final point.
                self.ps_stack.push(self.y);
                self.ps_stack.push(self.x);
            }
            1 => {
                self.in_flex = true;
                self.flex.clear();
            }
            2 => {}
            3 => {
                // Hint replacement: the following `pop` wants a subr number,
                // and 3 is the conventional no-op answer. The number asked
                // for is what an interpreter that replaces hints calls.
                if let Some(trace) = self.trace.as_mut() {
                    if let Some(subr) = args.first().and_then(|v| usize::try_from(*v as i64).ok()) {
                        trace.hints.insert(subr);
                    }
                }
                self.ps_stack.push(3.0);
            }
            _ => {
                // An unknown othersubr leaves its arguments for `pop`, which
                // is what the specification says to do.
                for value in args.into_iter().rev() {
                    self.ps_stack.push(value);
                }
            }
        }
    }

    /// A multiple-master program's blend othersubr: 14 to 18 blend 1, 2, 3, 4
    /// and 6 quantities (the supplement's section 3.12).
    ///
    /// The `k × m` arguments are the `m` quantities' first-master values and
    /// then, quantity by quantity, the other `k − 1` masters' deltas from them;
    /// each result is `a1 + ((Δ2 × W_1 + Δ3 × W_2) + … + Δk × W_(k−1))`,
    /// `$Blend`'s own order of operations (section 3.4), `W_0` unused as it
    /// is there. `None` — the othersubr unknown, its arguments handed to
    /// `pop` — for a program with no weights or a call whose count is not `k`
    /// times its results.
    fn blend(&self, index: i64, count: usize, args: &[f64]) -> Option<Vec<f64>> {
        let weights = self.font.weights.as_deref()?;
        let masters = weights.len();
        let quantities = match index {
            14 => 1,
            15 => 2,
            16 => 3,
            17 => 4,
            18 => 6,
            _ => return None,
        };
        if masters < 2 || count != masters * quantities || args.len() != count {
            return None;
        }
        let deltas = masters - 1;
        let mut results = Vec::with_capacity(quantities);
        // In range: `args` holds `masters × quantities` values, checked above,
        // and `weights` `masters`, at least two.
        for quantity in 0..quantities {
            let first = args[quantity];
            let from = quantities + quantity * deltas;
            let mut sum = args[from] * weights[1];
            for master in 1..deltas {
                sum += args[from + master] * weights[master + 1];
            }
            results.push(first + sum);
        }
        Some(results)
    }

    /// The last `n` operands, oldest first, padded with zeros.
    fn take(&self, n: usize) -> Vec<f64> {
        let at = self.stack.len().saturating_sub(n);
        let mut v = self.stack[at..].to_vec();
        while v.len() < n {
            v.insert(0, 0.0);
        }
        v
    }
}

/// Strips the segment headers of a PFB container, if there are any.
fn strip_pfb(data: &[u8]) -> &[u8] {
    // A PFB begins 0x80 0x01; a PDF embeds the bare program, and a PFA is
    // text. Only the first segment is needed: it holds everything up to and
    // including the private half.
    if data.first() != Some(&0x80) {
        return data;
    }
    // Rebuilding the concatenation would need an allocation this function
    // cannot return, and every PFB this has met keeps the cleartext and the
    // ciphertext in the first two segments. Callers wanting exactness can
    // strip it themselves.
    data
}

pub(crate) fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn unhex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut high: Option<u8> = None;
    for byte in data {
        let Some(value) = (*byte as char).to_digit(16) else {
            continue;
        };
        let value = value as u8;
        match high {
            Some(h) => {
                out.push((h << 4) | value);
                high = None;
            }
            None => high = Some(value),
        }
    }
    out
}

/// The integer following a key, as `/lenIV 4`.
fn read_int_after(data: &[u8], key: &[u8]) -> Option<i64> {
    let at = find(data, key)? + key.len();
    let rest = data.get(at..)?;
    let start = rest.iter().position(|b| !b.is_ascii_whitespace())?;
    let end = rest[start..]
        .iter()
        .position(|b| !(b.is_ascii_digit() || *b == b'-'))
        .unwrap_or(rest.len() - start);
    core::str::from_utf8(&rest[start..start + end])
        .ok()?
        .parse()
        .ok()
}

/// `/FontMatrix [a b c d e f]` from the cleartext half.
fn read_font_matrix(clear: &[u8]) -> Option<[f64; 6]> {
    let at = find(clear, b"/FontMatrix")?;
    let rest = &clear[at..];
    let open = rest.iter().position(|b| *b == b'[')?;
    let close = rest.iter().position(|b| *b == b']')?;
    let text = core::str::from_utf8(rest.get(open + 1..close)?).ok()?;

    let values: Vec<f64> = text
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    (values.len() >= 6).then(|| {
        [
            values[0], values[1], values[2], values[3], values[4], values[5],
        ]
    })
}

/// The style the cleartext's `/FontBBox` and `/FontInfo` and the Private
/// dictionary state, each absent entry its default ([`Style`]).
///
/// The Private dictionary's entries are read before its `/Subrs` and
/// `/CharStrings`, whose binary charstrings could spell anything.
fn read_style(clear: &[u8], private: &[u8]) -> Style {
    let entries_end = [find(private, b"/Subrs"), find(private, b"/CharStrings")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(private.len());
    let entries = &private[..entries_end];
    let defaults = Style::default();
    Style {
        font_bbox: read_bbox(clear).unwrap_or(defaults.font_bbox),
        is_fixed_pitch: value_after(clear, b"/isFixedPitch", boolean).unwrap_or(false),
        italic_angle: value_after(clear, b"/ItalicAngle", number).unwrap_or(0.0),
        underline_position: value_after(clear, b"/UnderlinePosition", number)
            .unwrap_or(defaults.underline_position),
        underline_thickness: value_after(clear, b"/UnderlineThickness", number)
            .unwrap_or(defaults.underline_thickness),
        weight: value_after(clear, b"/Weight", string),
        force_bold: value_after(entries, b"/ForceBold", boolean).unwrap_or(false),
        std_vw: read_array(entries, b"/StdVW").and_then(|value| value.array()?.first()?.number()),
    }
}

/// The first value `read` takes from where a definition of `key` places
/// one: each occurrence of `key` that ends where a name ends — so
/// `/Weight` is not `/WeightVector`'s first seven bytes — and after white
/// space a value `read` reads; an occurrence after which `read` reads
/// nothing, as `/BlendAxisTypes [/Weight /Width]` names `/Weight` and
/// defines nothing, is passed over.
fn value_after<T>(data: &[u8], key: &[u8], read: impl Fn(&[u8]) -> Option<T>) -> Option<T> {
    let mut from = 0usize;
    while let Some(found) = find(data.get(from..)?, key) {
        let after = from + found + key.len();
        from = after;
        if regular_end(data, after) != after {
            continue;
        }
        let rest = data.get(after..)?;
        let Some(start) = rest.iter().position(|b| !b.is_ascii_whitespace()) else {
            return None;
        };
        if let Some(value) = read(&rest[start..]) {
            return Some(value);
        }
    }
    None
}

/// A PostScript number at the start of `data`.
fn number(data: &[u8]) -> Option<f64> {
    ps_number(data.get(..regular_end(data, 0))?)
}

/// `true` or `false` at the start of `data`, as a whole token.
fn boolean(data: &[u8]) -> Option<bool> {
    match data.get(..regular_end(data, 0))? {
        b"true" => Some(true),
        b"false" => Some(false),
        _ => None,
    }
}

/// A PostScript string at the start of `data`, `(` to its balancing `)`,
/// a backslash taking the byte after it as it is, as UTF-8: `None` past 256
/// bytes, which no `/Weight` reaches.
fn string(data: &[u8]) -> Option<String> {
    if data.first() != Some(&b'(') {
        return None;
    }
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut escaped = false;
    for &byte in data.get(1..)? {
        if out.len() > 256 {
            return None;
        }
        if escaped {
            out.push(byte);
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'(' => {
                depth += 1;
                out.push(byte);
            }
            b')' if depth == 0 => return String::from_utf8(out).ok(),
            b')' => {
                depth -= 1;
                out.push(byte);
            }
            _ => out.push(byte),
        }
    }
    None
}

/// `/FontBBox`: four finite numbers in an array, `[` to `]`, or in a
/// procedure, `{` to `}`, as the Type 1 book's example writes it.
fn read_bbox(clear: &[u8]) -> Option<[f64; 4]> {
    value_after(clear, b"/FontBBox", |rest| {
        let close = match rest.first()? {
            b'[' => b']',
            b'{' => b'}',
            _ => return None,
        };
        let end = rest.iter().position(|b| *b == close)?;
        let values: Vec<f64> = rest
            .get(1..end)?
            .split(|b| b.is_ascii_whitespace())
            .filter(|token| !token.is_empty())
            .map(ps_number)
            .collect::<Option<_>>()?;
        <[f64; 4]>::try_from(values).ok()
    })
}

/// A value of the cleartext arrays the multiple-master keys are written as.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Number(f64),
    Name(Vec<u8>),
    Array(Vec<Value>),
}

impl Value {
    fn number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// The literal array a cleartext key is defined as: the first occurrence of
/// `key` that `[` follows, so a procedure that names the key — the
/// supplement's `makeblendedfont` reads `/WeightVector get` — is passed over.
/// `None` where no definition is an array this reads, nested at most
/// [`MAX_ARRAY_DEPTH`] deep and holding at most [`MAX_ARRAY_ITEMS`] values.
fn read_array(clear: &[u8], key: &[u8]) -> Option<Value> {
    let mut from = 0usize;
    while let Some(found) = find(clear.get(from..)?, key) {
        let after = from + found + key.len();
        let rest = clear.get(after..)?;
        let start = rest.iter().position(|b| !b.is_ascii_whitespace())?;
        if rest[start] == b'[' {
            let mut at = after + start;
            let mut items = 0usize;
            return parse_array(clear, &mut at, 0, &mut items);
        }
        from = after;
    }
    None
}

/// The array that opens at `data[*at]`, `[`, and every array inside it.
fn parse_array(data: &[u8], at: &mut usize, depth: usize, items: &mut usize) -> Option<Value> {
    if depth >= MAX_ARRAY_DEPTH {
        return None;
    }
    *at += 1;
    let mut out = Vec::new();
    loop {
        while data.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
        let value = match data.get(*at)? {
            b']' => {
                *at += 1;
                return Some(Value::Array(out));
            }
            b'[' => parse_array(data, at, depth + 1, items)?,
            b'/' => {
                let start = *at + 1;
                let end = regular_end(data, start);
                *at = end;
                Value::Name(data.get(start..end)?.to_vec())
            }
            _ => {
                let start = *at;
                let end = regular_end(data, start);
                if end == start {
                    return None;
                }
                *at = end;
                Value::Number(ps_number(data.get(start..end)?)?)
            }
        };
        *items += 1;
        if *items > MAX_ARRAY_ITEMS {
            return None;
        }
        out.push(value);
    }
}

/// Where a run of PostScript's regular characters from `start` ends.
fn regular_end(data: &[u8], start: usize) -> usize {
    data.get(start..)
        .and_then(|rest| {
            rest.iter().position(|b| {
                b.is_ascii_whitespace()
                    || matches!(
                        b,
                        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
                    )
            })
        })
        .map_or(data.len(), |at| start + at)
}

/// A PostScript integer or real, `0.53`, `.53`, `-12` or `1e-3`, finite.
pub(crate) fn ps_number(token: &[u8]) -> Option<f64> {
    let body = token
        .strip_prefix(b"-")
        .or_else(|| token.strip_prefix(b"+"))
        .unwrap_or(token);
    let starts = body
        .first()
        .is_some_and(|b| b.is_ascii_digit() || *b == b'.');
    let allowed = body
        .iter()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !starts || !allowed {
        return None;
    }
    core::str::from_utf8(token)
        .ok()?
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
}

/// `/WeightVector [w1 … wk]`: two to [`MAX_MASTERS`] numbers.
fn read_weight_vector(clear: &[u8]) -> Option<Vec<f64>> {
    let values = read_array(clear, b"/WeightVector")?;
    let weights: Vec<f64> = values
        .array()?
        .iter()
        .map(Value::number)
        .collect::<Option<_>>()?;
    (2..=MAX_MASTERS)
        .contains(&weights.len())
        .then_some(weights)
}

/// `/BlendAxisTypes`, `/BlendDesignPositions` and `/BlendDesignMap`, read as
/// written, within the supplement's limits: one to [`MAX_AXES`] axes, two to
/// [`MAX_MASTERS`] masters, at most [`MAX_MAP_POINTS`] points a map, every
/// point a pair. Whether the design is the standard form is
/// [`Blend::is_standard`]'s question, not this one's.
fn read_blend(clear: &[u8]) -> Option<Blend> {
    let axes: Vec<Vec<u8>> = read_array(clear, b"/BlendAxisTypes")?
        .array()?
        .iter()
        .map(|value| match value {
            Value::Name(name) => Some(name.clone()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if axes.is_empty() || axes.len() > MAX_AXES {
        return None;
    }
    let positions: Vec<Vec<f64>> = read_array(clear, b"/BlendDesignPositions")?
        .array()?
        .iter()
        .map(|position| {
            let coordinates: Vec<f64> = position
                .array()?
                .iter()
                .map(Value::number)
                .collect::<Option<_>>()?;
            (1..=MAX_AXES)
                .contains(&coordinates.len())
                .then_some(coordinates)
        })
        .collect::<Option<_>>()?;
    if !(2..=MAX_MASTERS).contains(&positions.len()) {
        return None;
    }
    let maps: Vec<Vec<(f64, f64)>> = read_array(clear, b"/BlendDesignMap")?
        .array()?
        .iter()
        .map(|map| {
            let points: Vec<(f64, f64)> = map
                .array()?
                .iter()
                .map(|point| match point.array()? {
                    [d, b] => Some((d.number()?, b.number()?)),
                    _ => None,
                })
                .collect::<Option<_>>()?;
            (1..=MAX_MAP_POINTS)
                .contains(&points.len())
                .then_some(points)
        })
        .collect::<Option<_>>()?;
    if maps.is_empty() || maps.len() > MAX_AXES {
        return None;
    }
    Some(Blend {
        axes,
        positions,
        maps,
    })
}

/// Whether the clear text's `/Encoding` is the token `StandardEncoding`.
fn says_standard_encoding(clear: &[u8]) -> bool {
    let Some(at) = find(clear, b"/Encoding") else {
        return false;
    };
    let rest = &clear[at + b"/Encoding".len()..];
    let start = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let token = &rest[start..];
    let word = b"StandardEncoding";
    token.starts_with(word)
        && token
            .get(word.len())
            .is_none_or(|b| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(b))
}

/// `dup <code> /<name> put` entries of a built-in `/Encoding`.
fn read_encoding(clear: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let Some(at) = find(clear, b"/Encoding") else {
        return Vec::new();
    };
    let region = &clear[at..];

    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(found) = find(&region[i..], b"dup ") {
        let start = i + found + 4;
        i = start;
        let rest = &region[start..];

        let digits = rest
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(rest.len());
        let Ok(text) = core::str::from_utf8(&rest[..digits]) else {
            continue;
        };
        let Ok(code) = text.parse::<u32>() else {
            continue;
        };

        let Some(slash) = rest.iter().position(|b| *b == b'/') else {
            break;
        };
        let name_start = slash + 1;
        let name_end = rest[name_start..]
            .iter()
            .position(|b| b.is_ascii_whitespace() || *b == b'/')
            .unwrap_or(0)
            + name_start;

        if code < 256 && name_end > name_start {
            out.push((code as u8, rest[name_start..name_end].to_vec()));
        }
        if out.len() > 256 {
            break;
        }
    }
    out
}

/// Which array an entry belongs to, and its key there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// A `/Subrs` entry, by the index its `dup` gives, as written.
    Subr(i64),
    /// A `/CharStrings` entry, by its name.
    Glyph(Vec<u8>),
}

/// One `RD` entry of the private section, as the scanner found it, every
/// offset into the decrypted private section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) key: Key,
    /// The first byte of its text: `dup`'s `d`, or the name's `/`.
    pub(crate) start: usize,
    /// The first byte of its length operand.
    pub(crate) length: usize,
    /// The `RD` token, as the program spells it (`RD`, `-|`).
    pub(crate) rd: Range<usize>,
    /// Its binary bytes.
    pub(crate) data: Range<usize>,
}

/// What the scanner found in one of the private section's two arrays.
pub(crate) struct Scan {
    pub(crate) entries: Vec<Entry>,
    /// How many bytes of its region the scan walked, which the reader counts.
    walked: usize,
    /// Whether the charstrings' scan stopped at [`MAX_GLYPHS`].
    pub(crate) capped: bool,
}

/// `/Subrs <n> array` followed by `dup <i> <len> RD <bytes> NP`.
pub(crate) fn scan_subrs(private: &[u8]) -> Scan {
    let mut scan = Scan {
        entries: Vec::new(),
        walked: 0,
        capped: false,
    };
    let Some(at) = find(private, b"/Subrs") else {
        return scan;
    };
    let region = &private[at..];

    let mut i = 0usize;
    while let Some(found) = find(&region[i..], b"dup ") {
        let start = i + found + 4;
        let rest = &region[start..];

        let Some((index, _, after_index)) = read_number(rest) else {
            i = start;
            continue;
        };
        let Some((length, length_at, after_length)) = read_number(&rest[after_index..]) else {
            i = start;
            continue;
        };
        let length_at = after_index + length_at;
        let after_length = after_index + after_length;

        let Some((rd, data_start)) = binary_start(&rest[after_length..]) else {
            i = start;
            continue;
        };
        let from = after_length + data_start;
        // A negative length ends the array, as one past the section does.
        let Some(to) = usize::try_from(length)
            .ok()
            .and_then(|length| from.checked_add(length))
        else {
            break;
        };
        if rest.get(from..to).is_none() {
            break;
        }

        let base = at + start;
        scan.entries.push(Entry {
            key: Key::Subr(index),
            start: base - 4,
            length: base + length_at,
            rd: base + after_length + rd.start..base + after_length + rd.end,
            data: base + from..base + to,
        });
        i = start + to;
    }
    scan.walked = i.min(region.len());
    scan
}

/// `/CharStrings <n> dict dup begin` then `/<name> <len> RD <bytes> ND`.
pub(crate) fn scan_charstrings(private: &[u8]) -> Scan {
    let mut scan = Scan {
        entries: Vec::new(),
        walked: 0,
        capped: false,
    };
    let Some(at) = find(private, b"/CharStrings") else {
        return scan;
    };
    // Past the header, so `/CharStrings` itself is not read as a glyph name.
    let region_at = at + 12;
    let region = &private[region_at..];

    let mut i = 0usize;
    while i < region.len() {
        let Some(slash) = region[i..].iter().position(|b| *b == b'/') else {
            break;
        };
        let name_start = i + slash + 1;
        let rest = &region[name_start..];

        let name_end = rest
            .iter()
            .position(|b| b.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_vec();

        let Some((length, length_at, after_length)) = read_number(&rest[name_end..]) else {
            i = name_start;
            continue;
        };
        let length_at = name_end + length_at;
        let after_length = name_end + after_length;

        let Some((rd, data_start)) = binary_start(&rest[after_length..]) else {
            i = name_start;
            continue;
        };
        let from = after_length + data_start;
        let Some(to) = usize::try_from(length.max(0))
            .ok()
            .and_then(|length| from.checked_add(length))
        else {
            break;
        };
        if rest.get(from..to).is_none() {
            break;
        }

        let base = region_at + name_start;
        scan.entries.push(Entry {
            key: Key::Glyph(name),
            start: base - 1,
            length: base + length_at,
            rd: base + after_length + rd.start..base + after_length + rd.end,
            data: base + from..base + to,
        });
        if scan.entries.len() > MAX_GLYPHS {
            scan.capped = true;
            break;
        }
        i = name_start + to;
    }
    scan.walked = i.min(region.len());
    scan
}

/// The subroutines the reader stores, by index: the last entry for an index
/// wins, as PostScript's `put` does, and an index past 65 535 is passed over.
fn read_subrs(private: &[u8], len_iv: usize, steps: &mut u64) -> Vec<Vec<u8>> {
    let scan = scan_subrs(private);
    let mut subrs: Vec<Vec<u8>> = Vec::new();
    for entry in &scan.entries {
        let Key::Subr(index) = entry.key else {
            continue;
        };
        let Some(index) = usize::try_from(index.max(0))
            .ok()
            .filter(|index| *index < MAX_SUBRS)
        else {
            continue;
        };
        let Some(bytes) = private.get(entry.data.clone()) else {
            continue;
        };
        if subrs.len() <= index {
            subrs.resize(index + 1, Vec::new());
        }
        subrs[index] = decrypt(bytes, CHARSTRING_KEY, len_iv);
        step(steps, bytes.len());
    }
    step(steps, scan.walked);
    subrs
}

/// The charstrings the reader draws, in the order the program gives them.
fn read_charstrings(private: &[u8], len_iv: usize, steps: &mut u64) -> Vec<(Vec<u8>, Vec<u8>)> {
    let scan = scan_charstrings(private);
    let mut out = Vec::with_capacity(scan.entries.len());
    for entry in &scan.entries {
        let Key::Glyph(name) = &entry.key else {
            continue;
        };
        let Some(bytes) = private.get(entry.data.clone()) else {
            continue;
        };
        out.push((name.clone(), decrypt(bytes, CHARSTRING_KEY, len_iv)));
        step(steps, bytes.len());
    }
    step(steps, scan.walked);
    out
}

/// `n` steps more, saturating.
fn step(steps: &mut u64, n: usize) {
    *steps = steps.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
}

/// An integer, where its digits begin, and how many bytes it and its leading
/// whitespace took.
fn read_number(data: &[u8]) -> Option<(i64, usize, usize)> {
    let start = data.iter().position(|b| !b.is_ascii_whitespace())?;
    let rest = &data[start..];
    let end = rest
        .iter()
        .position(|b| !(b.is_ascii_digit() || *b == b'-'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let value: i64 = core::str::from_utf8(&rest[..end]).ok()?.parse().ok()?;
    Some((value, start, start + end))
}

/// The `RD` or `-|` token after a length, and where the binary data begins.
///
/// Exactly one space separates the token from the bytes, and the bytes may
/// begin with anything at all — including another space — so counting is the
/// only correct way to find them.
fn binary_start(data: &[u8]) -> Option<(Range<usize>, usize)> {
    let start = data.iter().position(|b| !b.is_ascii_whitespace())?;
    let rest = &data[start..];
    let token_end = rest
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(rest.len());
    Some((start..start + token_end, start + token_end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lead bytes the tests' programs carry, as a font tool would choose
    /// them: their value is irrelevant, since a reader discards them.
    const PAD: [u8; 4] = [0x55; 4];

    #[test]
    fn the_cipher_round_trips() {
        let plain = b"/CharStrings 1 dict dup begin".to_vec();
        let cipher = encrypt(&plain, EEXEC_KEY, &PAD);
        assert_eq!(decrypt(&cipher, EEXEC_KEY, 4), plain);
        // 7.3's worked example: four zero lead bytes before the block letter
        // "C"'s 37 bytes give the 41 bytes of ciphertext the chapter prints.
        let charstring = [
            0xBD, 0xF9, 0xB4, 0x0D, 0x8B, 0xEF, 0x03, 0x8B, 0xEF, 0x01, 0xF8, 0xEC, 0xEF, 0x01,
            0x8B, 0x16, 0xF9, 0x50, 0x06, 0xEF, 0x07, 0xFC, 0xEC, 0x06, 0xF8, 0x88, 0x07, 0xF8,
            0xEC, 0x06, 0xEF, 0x07, 0xFD, 0x50, 0x06, 0x09, 0x0E,
        ];
        let expected = [
            0x10, 0xBF, 0x31, 0x70, 0x4F, 0xAB, 0x5B, 0x1F, 0x03, 0xF9, 0xB6, 0x8B, 0x1F, 0x39,
            0xA6, 0x65, 0x21, 0xB1, 0x84, 0x1F, 0x14, 0x81, 0x69, 0x7F, 0x8E, 0x12, 0xB7, 0xF7,
            0xDD, 0xD6, 0xE3, 0xD7, 0x24, 0x8D, 0x96, 0x5B, 0x1C, 0xD4, 0x5E, 0x21, 0x14,
        ];
        assert_eq!(encrypt(&charstring, CHARSTRING_KEY, &[0; 4]), expected);
        assert_eq!(decrypt(&expected, CHARSTRING_KEY, 4), charstring);
    }

    /// A Type 1 number in its shortest form.
    fn num(v: i32, out: &mut Vec<u8>) {
        if (-107..=107).contains(&v) {
            out.push((v + 139) as u8);
        } else if (108..=1131).contains(&v) {
            let v = v - 108;
            out.push((v / 256 + 247) as u8);
            out.push((v % 256) as u8);
        } else if (-1131..=-108).contains(&v) {
            let v = -v - 108;
            out.push((v / 256 + 251) as u8);
            out.push((v % 256) as u8);
        } else {
            out.push(255);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }

    /// A program of `glyphs` and `subrs`, each a plaintext charstring.
    fn program(glyphs: &[(&[u8], Vec<u8>)], subrs: &[(i64, Vec<u8>)]) -> Vec<u8> {
        let mut private = Vec::new();
        private.extend_from_slice(b"dup /Private 8 dict dup begin\n/lenIV 4 def\n");
        private.extend_from_slice(format!("/Subrs {} array\n", subrs.len()).as_bytes());
        for (index, code) in subrs {
            let encrypted = encrypt(code, CHARSTRING_KEY, &PAD);
            private.extend_from_slice(format!("dup {index} {} RD ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(b" NP\n");
        }
        private.extend_from_slice(b"ND\n");
        private.extend_from_slice(
            format!("2 index /CharStrings {} dict dup begin\n", glyphs.len()).as_bytes(),
        );
        for (name, code) in glyphs {
            let encrypted = encrypt(code, CHARSTRING_KEY, &PAD);
            private.push(b'/');
            private.extend_from_slice(name);
            private.extend_from_slice(format!(" {} RD ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(b" ND\n");
        }
        private.extend_from_slice(b"end\nend\nmark currentfile closefile\n");

        let mut out = b"%!PS-AdobeFont-1.0: Test\n\
            /FontMatrix [0.001 0 0 0.001 0 0] readonly def\ncurrentfile eexec\n"
            .to_vec();
        out.extend(encrypt(&private, EEXEC_KEY, &PAD));
        out
    }

    /// A minimal but genuine Type 1 program: one glyph, a square, placed by
    /// `hsbw` and drawn with the relative operators.
    fn font_with_square() -> Vec<u8> {
        square_with("", "")
    }

    /// [`font_with_square`]'s program with `cleartext` written before its
    /// `/FontMatrix` and `private_entries` at the head of its Private
    /// dictionary: the entries `Type1::style` reads.
    fn square_with(cleartext: &str, private_entries: &str) -> Vec<u8> {
        // hsbw 50 600, rmoveto 0 0, then a 500-unit box, closepath, endchar.
        let mut cs: Vec<u8> = Vec::new();
        num(50, &mut cs);
        num(600, &mut cs);
        cs.push(13); // hsbw
        num(0, &mut cs);
        num(0, &mut cs);
        cs.push(21); // rmoveto
        num(500, &mut cs);
        cs.push(6); // hlineto
        num(500, &mut cs);
        cs.push(7); // vlineto
        num(-500, &mut cs);
        cs.push(6); // hlineto
        cs.push(9); // closepath
        cs.push(14); // endchar

        let encrypted = encrypt(&cs, CHARSTRING_KEY, &PAD);

        let mut private = Vec::new();
        private.extend_from_slice(b"dup /Private 8 dict dup begin\n/lenIV 4 def\n");
        private.extend_from_slice(private_entries.as_bytes());
        private.extend_from_slice(b"/Subrs 0 array ND\n");
        private.extend_from_slice(b"/CharStrings 1 dict dup begin\n");
        private.extend_from_slice(format!("/square {} RD ", encrypted.len()).as_bytes());
        private.extend_from_slice(&encrypted);
        private.extend_from_slice(b" ND\nend\nend\n");

        let mut out = Vec::new();
        out.extend_from_slice(b"%!PS-AdobeFont-1.0: Test\n");
        out.extend_from_slice(cleartext.as_bytes());
        out.extend_from_slice(b"/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n");
        out.extend_from_slice(b"/Encoding 256 array\ndup 65 /square put\nreadonly def\n");
        out.extend_from_slice(b"currentfile eexec\n");
        out.extend_from_slice(&encrypt(&private, EEXEC_KEY, &PAD));
        out
    }

    #[test]
    fn a_program_parses_and_finds_its_glyph() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert_eq!(font.glyph_count(), 1);
        assert_eq!(font.glyph_name(0), Some(b"square".as_slice()));
        assert_eq!(font.glyph_for_name(b"square"), Some(0));
    }

    /// The built-in `/Encoding` maps a code to a glyph, which is how a
    /// symbolic Type 1 font addresses its own glyphs.
    #[test]
    fn the_built_in_encoding_is_read() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert_eq!(font.glyph_for_code(b'A'), Some(0));
        assert_eq!(font.glyph_for_code(b'B'), None);
    }

    #[test]
    fn a_charstring_becomes_an_outline() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        let outline = font.outline(0).expect("it draws");

        // hsbw put the origin at x = 50, so the box runs 50..550.
        let points: Vec<(f64, f64)> = outline
            .segments
            .iter()
            .filter_map(|s| match *s {
                Segment::MoveTo { x, y } | Segment::LineTo { x, y } => Some((x, y)),
                _ => None,
            })
            .collect();

        assert_eq!(
            points.first(),
            Some(&(50.0, 0.0)),
            "the side bearing shifts the start"
        );
        assert!(
            points.contains(&(550.0, 0.0)),
            "and the box is 500 wide: {points:?}"
        );
        assert!(points.contains(&(550.0, 500.0)));
        assert!(
            matches!(outline.segments.last(), Some(Segment::Close)),
            "closepath closes it"
        );
    }

    /// `hsbw` carries the advance, which is what a PDF needs when the font
    /// dictionary has no `/Widths`.
    #[test]
    fn the_advance_comes_from_hsbw() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert_eq!(font.advance(0), Some(600.0));
    }

    #[test]
    fn the_font_matrix_is_read() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert!((font.font_matrix[0] - 0.001).abs() < 1e-9);
    }

    /// Bytes that are not a Type 1 program at all must be declined, not
    /// half-read.
    #[test]
    fn arbitrary_bytes_are_declined() {
        assert!(Type1::parse(b"").is_none());
        assert!(Type1::parse(b"not a font").is_none());
        assert!(Type1::parse(&[0u8; 512]).is_none());
    }

    /// A truncated program must not panic, whatever it has been cut through.
    #[test]
    fn every_truncation_is_survivable() {
        let full = font_with_square();
        for cut in 0..full.len() {
            let font = Type1::parse(&full[..cut]);
            if let Some(font) = font {
                for glyph in 0..font.glyph_count().min(4) as u16 {
                    let _ = font.outline(glyph);
                    let _ = font.advance(glyph);
                }
            }
        }
    }

    /// A number in the four-byte form is a plain integer here, not the
    /// 16.16 fixed-point value Type 2 uses. Reading one as the other scales
    /// every coordinate by 65536.
    #[test]
    fn the_large_number_form_is_an_integer() {
        let mut cs = vec![255u8];
        cs.extend_from_slice(&2000i32.to_be_bytes());
        cs.push(139); // 0
        cs.push(13); // hsbw: width 0, sidebearing 2000

        let font = Type1 {
            glyphs: vec![(b"g".to_vec(), cs)],
            subrs: Vec::new(),
            encoding: Vec::new(),
            standard: false,
            font_matrix: [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
            weight_vector: None,
            blend: None,
            weights: None,
            style: Style::default(),
            _marker: core::marker::PhantomData,
        };
        // The side bearing lands at 2000, not at 2000/65536.
        let outline = font.outline(0).expect("it runs");
        assert!(outline.is_empty(), "no drawing operators");
        assert_eq!(font.advance(0), Some(0.0));
    }

    /// One number in the Type 1 charstring encodings, whichever is shortest.
    fn push_number(v: i32, out: &mut Vec<u8>) {
        if (-107..=107).contains(&v) {
            out.push((v + 139) as u8);
        } else if (108..=1131).contains(&v) {
            let u = v - 108;
            out.push((u / 256 + 247) as u8);
            out.push((u % 256) as u8);
        } else if (-1131..=-108).contains(&v) {
            let u = -v - 108;
            out.push((u / 256 + 251) as u8);
            out.push((u % 256) as u8);
        } else {
            out.push(255);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }

    /// One blended group of a multiple-master charstring, as the supplement's
    /// section 3.11 writes it: `masters[i][q]` is master `i`'s value of
    /// quantity `q`, written as each quantity's first-master value, then each
    /// quantity's deltas from it, then the count, the blend othersubr for that
    /// many quantities, `callothersubr` and one `pop` a quantity.
    fn blended(masters: &[Vec<i32>], out: &mut Vec<u8>) {
        let quantities = masters[0].len();
        for q in 0..quantities {
            push_number(masters[0][q], out);
        }
        for q in 0..quantities {
            for master in &masters[1..] {
                push_number(master[q] - masters[0][q], out);
            }
        }
        push_number((masters.len() * quantities) as i32, out);
        let othersubr = match quantities {
            1 => 14,
            2 => 15,
            3 => 16,
            4 => 17,
            6 => 18,
            other => panic!("no blend othersubr takes {other} quantities"),
        };
        push_number(othersubr, out);
        out.extend_from_slice(&[12, 16]);
        for _ in 0..quantities {
            out.extend_from_slice(&[12, 17]);
        }
    }

    /// A program whose cleartext carries `keys` before `/FontMatrix`, and
    /// whose `/CharStrings` holds `charstrings`.
    fn mm_program(keys: &str, charstrings: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut private = Vec::new();
        private.extend_from_slice(b"dup /Private 8 dict dup begin\n/lenIV 4 def\n");
        private.extend_from_slice(b"/Subrs 0 array ND\n");
        private.extend_from_slice(
            format!("/CharStrings {} dict dup begin\n", charstrings.len()).as_bytes(),
        );
        for (name, charstring) in charstrings {
            let encrypted = encrypt(charstring, CHARSTRING_KEY, &PAD);
            private.extend_from_slice(format!("/{name} {} RD ", encrypted.len()).as_bytes());
            private.extend_from_slice(&encrypted);
            private.extend_from_slice(b" ND\n");
        }
        private.extend_from_slice(b"end\nend\n");

        let mut out = Vec::new();
        out.extend_from_slice(b"%!PS-AdobeFont-1.0: TestMM 001.000\n");
        out.extend_from_slice(keys.as_bytes());
        out.extend_from_slice(b"/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n");
        out.extend_from_slice(b"/Encoding 256 array\ndup 65 /square put\nreadonly def\n");
        out.extend_from_slice(b"currentfile eexec\n");
        out.extend_from_slice(&encrypt(&private, EEXEC_KEY, &PAD));
        out
    }

    /// The two-axis, four-master design the tests instance: a weight axis
    /// mapped linearly from 100 to 900, and a width axis of three points, so
    /// one segment of each kind is crossed. Each master at its own corner,
    /// written out of the corners' order, and a procedure before the real
    /// definition that names `/WeightVector` as `makeblendedfont` does.
    const FOUR_MASTERS: &str = "/FontInfo 5 dict dup begin\n\
         /BlendAxisTypes [/Weight /Width ] def\n\
         /BlendDesignPositions [[0 0] [1 0] [0 1] [1 1]] def\n\
         /BlendDesignMap [[[100 0][900 1]] [[0 0][400 0.5][1000 1]]] def\n\
         end readonly def\n\
         /FontName /TestMM def\n\
         /makeblendedfont { dup /WeightVector get pop } def\n\
         /WeightVector [0.125 0.375 0.125 0.375 ] def\n\
         /$Blend {0.375 mul exch 0.125 mul add exch 0.375 mul add add} bind def\n";

    /// The four masters of the test glyph: an advance, a move, a box's two
    /// sides and a curve back, each quantity different in every master.
    struct Masters {
        hsbw: Vec<Vec<i32>>,
        moveto: Vec<Vec<i32>>,
        width: Vec<Vec<i32>>,
        height: Vec<Vec<i32>>,
        curve_a: Vec<Vec<i32>>,
        curve_b: Vec<Vec<i32>>,
    }

    fn four_masters() -> Masters {
        Masters {
            hsbw: vec![vec![10, 500], vec![30, 700], vec![50, 600], vec![70, 900]],
            moveto: vec![vec![0, 0], vec![20, 10], vec![40, 20], vec![60, 30]],
            width: vec![vec![400], vec![600], vec![500], vec![800]],
            height: vec![vec![300], vec![300], vec![500], vec![500]],
            curve_a: vec![
                vec![-100, 50, -100],
                vec![-150, 60, -150],
                vec![-120, 70, -130],
                vec![-200, 80, -200],
            ],
            curve_b: vec![
                vec![0, -200, -50],
                vec![0, -300, -60],
                vec![10, -250, -80],
                vec![20, -400, -100],
            ],
        }
    }

    /// The test glyph's charstring: every operand blended, the curve's six
    /// split over two calls of othersubr 16 as section 3.11 splits them.
    fn square_charstring(masters: &Masters) -> Vec<u8> {
        let mut cs = Vec::new();
        blended(&masters.hsbw, &mut cs);
        cs.push(13); // hsbw
        blended(&masters.moveto, &mut cs);
        cs.push(21); // rmoveto
        blended(&masters.width, &mut cs);
        cs.push(6); // hlineto
        blended(&masters.height, &mut cs);
        cs.push(7); // vlineto
        blended(&masters.curve_a, &mut cs);
        blended(&masters.curve_b, &mut cs);
        cs.push(8); // rrcurveto
        cs.push(9); // closepath
        cs.push(14); // endchar
        cs
    }

    /// 10.3: `/Encoding StandardEncoding def` assigns the Adobe Standard
    /// Encoding, so a code reaches the glyph its Standard name names; a
    /// program with `dup` entries reads those, and one with neither reads
    /// nothing.
    #[test]
    fn a_program_that_says_standard_encoding_reads_its_codes_through_it() {
        let glyphs: [(&[u8], Vec<u8>); 3] = [
            (b"A", box_charstring(0, 600, 500)),
            (b"quoteright", box_charstring(0, 300, 50)),
            (b"B", box_charstring(0, 600, 400)),
        ];
        let standard = {
            let mut out = program(&glyphs, &[]);
            let at = find(&out, b"currentfile eexec").expect("the clear text");
            out.splice(at..at, b"/Encoding StandardEncoding def\n".iter().copied());
            out
        };
        let font = Type1::parse(&standard).expect("it parses");
        assert_eq!(font.glyph_for_code(b'A'), Some(0));
        assert_eq!(font.glyph_for_code(b'B'), Some(2));
        // StandardEncoding's code 39 is `quoteright`, not `quotesingle`.
        assert_eq!(font.glyph_for_code(39), Some(1));
        assert_eq!(font.glyph_for_code(b'C'), None, "no glyph C");

        let none = program(&glyphs, &[]);
        let font = Type1::parse(&none).expect("it parses");
        assert_eq!(font.glyph_for_code(b'A'), None, "no /Encoding, no code");

        let named = {
            let mut out = program(&glyphs, &[]);
            let at = find(&out, b"currentfile eexec").expect("the clear text");
            out.splice(
                at..at,
                b"/Encoding 256 array\ndup 65 /B put\nreadonly def\n"
                    .iter()
                    .copied(),
            );
            out
        };
        let font = Type1::parse(&named).expect("it parses");
        assert_eq!(
            font.glyph_for_code(b'A'),
            Some(2),
            "the entry, not Standard's"
        );
        assert_eq!(font.glyph_for_code(b'B'), None);

        // `StandardEncodingX` is another name.
        let other = {
            let mut out = program(&glyphs, &[]);
            let at = find(&out, b"currentfile eexec").expect("the clear text");
            out.splice(at..at, b"/Encoding StandardEncodingX def\n".iter().copied());
            out
        };
        let font = Type1::parse(&other).expect("it parses");
        assert_eq!(font.glyph_for_code(b'A'), None);
    }

    /// A box drawn from `(x, 0)` by `side` units, as a charstring after
    /// `hsbw`.
    fn box_charstring(sbx: i32, width: i32, side: i32) -> Vec<u8> {
        let mut cs = Vec::new();
        num(sbx, &mut cs);
        num(width, &mut cs);
        cs.push(13); // hsbw
        num(0, &mut cs);
        num(0, &mut cs);
        cs.push(21); // rmoveto
        num(side, &mut cs);
        cs.push(6); // hlineto
        num(side, &mut cs);
        cs.push(7); // vlineto
        num(-side, &mut cs);
        cs.push(6); // hlineto
        cs.push(9); // closepath
        cs.push(14); // endchar
        cs
    }

    /// The test's own piecewise-linear map, written from section 3.8.
    fn through_map(map: &[(f64, f64)], d: f64) -> f64 {
        if d <= map[0].0 {
            return map[0].1;
        }
        for pair in map.windows(2) {
            if d <= pair[1].0 {
                let ((d0, b0), (d1, b1)) = (pair[0], pair[1]);
                return b0 + (d - d0) * (b1 - b0) / (d1 - d0);
            }
        }
        map[map.len() - 1].1
    }

    /// The test's own weights at a design vector, by section 3.9's
    /// multilinear rule over the corners the design places its masters at.
    fn weights_for(design: [f64; 2]) -> Vec<f64> {
        let b = [
            through_map(&[(100.0, 0.0), (900.0, 1.0)], design[0]),
            through_map(&[(0.0, 0.0), (400.0, 0.5), (1000.0, 1.0)], design[1]),
        ];
        let corners = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        corners
            .iter()
            .map(|corner: &[f64; 2]| {
                (0..2)
                    .map(|axis| {
                        if corner[axis] == 1.0 {
                            b[axis]
                        } else {
                            1.0 - b[axis]
                        }
                    })
                    .product()
            })
            .collect()
    }

    /// Each quantity of a group blended with `weights`, the masters' values
    /// times their weights, summed.
    fn mix(group: &[Vec<i32>], weights: &[f64]) -> Vec<f64> {
        (0..group[0].len())
            .map(|q| {
                group
                    .iter()
                    .zip(weights)
                    .map(|(master, w)| f64::from(master[q]) * w)
                    .sum()
            })
            .collect()
    }

    /// The test glyph's outline and advance at `weights`, drawn by hand as
    /// the charstring's commands draw them.
    fn expected_square(masters: &Masters, weights: &[f64]) -> (Vec<Segment>, f64) {
        let hsbw = mix(&masters.hsbw, weights);
        let moveto = mix(&masters.moveto, weights);
        let width = mix(&masters.width, weights)[0];
        let height = mix(&masters.height, weights)[0];
        let a = mix(&masters.curve_a, weights);
        let b = mix(&masters.curve_b, weights);
        let (x, y) = (hsbw[0] + moveto[0], moveto[1]);
        let after_width = x + width;
        let after_height = y + height;
        let c1 = (after_width + a[0], after_height + a[1]);
        let c2 = (c1.0 + a[2], c1.1 + b[0]);
        let end = (c2.0 + b[1], c2.1 + b[2]);
        (
            vec![
                Segment::MoveTo { x, y },
                Segment::LineTo { x: after_width, y },
                Segment::LineTo {
                    x: after_width,
                    y: after_height,
                },
                Segment::CurveTo {
                    c1x: c1.0,
                    c1y: c1.1,
                    c2x: c2.0,
                    c2y: c2.1,
                    x: end.0,
                    y: end.1,
                },
                Segment::Close,
            ],
            hsbw[1],
        )
    }

    fn four_master_font() -> Type1<'static> {
        let masters = four_masters();
        Type1::parse(&mm_program(
            FOUR_MASTERS,
            &[("square", square_charstring(&masters))],
        ))
        .expect("the program parses")
    }

    #[test]
    fn the_blend_design_is_read_as_written() {
        let font = four_master_font();
        assert!(font.is_multiple_master());
        assert_eq!(
            font.weight_vector(),
            Some([0.125, 0.375, 0.125, 0.375].as_slice())
        );
        let blend = font.blend().expect("a blend design");
        assert_eq!(blend.axes, vec![b"Weight".to_vec(), b"Width".to_vec()]);
        assert_eq!(
            blend.positions,
            vec![
                vec![0.0, 0.0],
                vec![1.0, 0.0],
                vec![0.0, 1.0],
                vec![1.0, 1.0]
            ]
        );
        assert_eq!(
            blend.maps,
            vec![
                vec![(100.0, 0.0), (900.0, 1.0)],
                vec![(0.0, 0.0), (400.0, 0.5), (1000.0, 1.0)]
            ]
        );
        assert!(blend.is_standard());
        assert_eq!((blend.axis_count(), blend.master_count()), (2, 4));
    }

    /// **FT-10.4's exit.** Instanced at several design vectors, the program's
    /// outline points and advance are its masters blended with weights the
    /// test computes from the blend design itself, and the weights it keeps
    /// are those weights.
    #[test]
    fn an_instance_is_its_masters_blended_at_the_design_weights() {
        let masters = four_masters();
        let mut font = four_master_font();
        for design in [
            [500.0, 700.0],
            [300.0, 400.0],
            [100.0, 0.0],
            [900.0, 1000.0],
            [700.0, 100.0],
        ] {
            let weights = weights_for(design);
            let instance = font.instance(&design).expect("a standard design instances");
            assert_eq!(instance.weights, weights, "{design:?}");
            assert!(!instance.clamped, "{design:?}");
            assert_eq!(font.weights(), Some(weights.as_slice()));
            let (segments, advance) = expected_square(&masters, &weights);
            assert_eq!(
                font.outline(0).expect("it draws").segments,
                segments,
                "{design:?}"
            );
            assert_eq!(font.advance(0), Some(advance), "{design:?}");
        }

        // A corner draws its own master, and past the maps a coordinate is
        // taken at the map's end, which the instance says.
        let instance = font.instance(&[50.0, 2000.0]).expect("it instances");
        assert!(instance.clamped);
        assert_eq!(instance.weights, vec![0.0, 0.0, 1.0, 0.0]);
        let (segments, advance) = expected_square(&masters, &instance.weights);
        assert_eq!(font.outline(0).expect("it draws").segments, segments);
        assert_eq!(font.advance(0), Some(advance));
        assert_eq!(advance, 600.0, "master 3's own width");
    }

    #[test]
    fn the_weight_vector_is_the_default_instance() {
        let masters = four_masters();
        let mut font = four_master_font();
        let default = vec![0.125, 0.375, 0.125, 0.375];
        assert_eq!(font.weights(), Some(default.as_slice()));
        let (segments, advance) = expected_square(&masters, &default);
        assert_eq!(font.outline(0).expect("it draws").segments, segments);
        assert_eq!(font.advance(0), Some(advance));

        // Another instance, and back.
        font.instance(&[500.0, 700.0]).expect("it instances");
        assert_ne!(font.outline(0).expect("it draws").segments, segments);
        font.default_instance();
        assert_eq!(font.weights(), Some(default.as_slice()));
        assert_eq!(font.outline(0).expect("it draws").segments, segments);

        // A refused instance keeps what the program drew.
        font.instance(&[300.0, 400.0]).expect("it instances");
        let drawn = font.outline(0).expect("it draws").segments;
        assert_eq!(
            font.instance(&[300.0]),
            Err(InstanceRefusal::AxisCount { axes: 2, given: 1 })
        );
        assert_eq!(
            font.instance(&[f64::NAN, 1.0]),
            Err(InstanceRefusal::NotFinite)
        );
        assert_eq!(font.outline(0).expect("it draws").segments, drawn);
    }

    /// A design other than the standard form is not instanced, and draws the
    /// program's default.
    #[test]
    fn a_design_off_the_standard_form_keeps_the_default_instance() {
        let masters = four_masters();
        let charstrings = [("square", square_charstring(&masters))];
        let default = vec![0.125, 0.375, 0.125, 0.375];
        for (from, to) in [
            // An intermediate master.
            ("[[0 0] [1 0] [0 1] [1 1]]", "[[0 0] [1 0] [0 1] [0.5 1]]"),
            // A corner held twice.
            ("[[0 0] [1 0] [0 1] [1 1]]", "[[0 0] [1 0] [0 1] [1 0]]"),
            // A map out of order.
            ("[[100 0][900 1]]", "[[900 0][100 1]]"),
            // A blend coordinate past 1.
            ("[[100 0][900 1]]", "[[100 0][900 1.5]]"),
            // Three axes' types for two axes' maps.
            ("[/Weight /Width ]", "[/Weight /Width /OpticalSize]"),
        ] {
            let keys = FOUR_MASTERS.replace(from, to);
            assert_ne!(keys, FOUR_MASTERS);
            let mut font = Type1::parse(&mm_program(&keys, &charstrings)).expect("it parses");
            assert_eq!(
                font.instance(&[500.0, 700.0]),
                Err(InstanceRefusal::NotStandard),
                "{to}"
            );
            assert_eq!(font.weights(), Some(default.as_slice()), "{to}");
            let (segments, _) = expected_square(&masters, &default);
            assert_eq!(
                font.outline(0).expect("it draws").segments,
                segments,
                "{to}"
            );
        }

        // Masters other than the weight vector holds.
        let keys = FOUR_MASTERS.replace("[0.125 0.375 0.125 0.375 ]", "[0.25 0.25 0.5]");
        let mut font = Type1::parse(&mm_program(&keys, &charstrings)).expect("it parses");
        assert_eq!(
            font.instance(&[500.0, 700.0]),
            Err(InstanceRefusal::NotStandard)
        );

        // No design at all, or one past the supplement's limits.
        for (from, to) in [
            ("/BlendDesignMap", "/BlendDesignMapping"),
            (
                "[[0 0][400 0.5][1000 1]]",
                "[[0 0][1 0][2 0][3 0][4 0][5 0][6 0][7 0][8 0][9 0][10 0][11 0][12 1]]",
            ),
            ("[[100 0][900 1]]", "[[[100 0][900 1]]]"),
        ] {
            let keys = FOUR_MASTERS.replace(from, to);
            let mut font = Type1::parse(&mm_program(&keys, &charstrings)).expect("it parses");
            assert!(font.blend().is_none(), "{to}");
            assert_eq!(
                font.instance(&[500.0, 700.0]),
                Err(InstanceRefusal::NoDesign),
                "{to}"
            );
            assert!(font.is_multiple_master());
        }
    }

    /// A program with no weight vector — every ordinary Type 1 program, and a
    /// multiple-master one whose `/WeightVector` is past sixteen numbers —
    /// draws as it always has: its blend othersubrs are unknown, and `pop`
    /// takes their arguments, the first master's values.
    #[test]
    fn a_program_without_a_weight_vector_draws_its_first_master_as_before() {
        let masters = four_masters();
        let charstrings = [("square", square_charstring(&masters))];
        let first = [1.0, 0.0, 0.0, 0.0];
        let (segments, advance) = expected_square(&masters, &first);
        for keys in [
            FOUR_MASTERS.replace("/WeightVector", "/WeightVectors"),
            FOUR_MASTERS.replace(
                "[0.125 0.375 0.125 0.375 ]",
                "[0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1]",
            ),
            FOUR_MASTERS.replace("[0.125 0.375 0.125 0.375 ]", "[1]"),
        ] {
            let mut font = Type1::parse(&mm_program(&keys, &charstrings)).expect("it parses");
            assert!(!font.is_multiple_master());
            assert_eq!(font.weights(), None);
            assert_eq!(
                font.instance(&[500.0, 700.0]),
                Err(InstanceRefusal::NotMultipleMaster)
            );
            assert_eq!(font.outline(0).expect("it draws").segments, segments);
            assert_eq!(font.advance(0), Some(advance));
        }

        // A blend call whose count is not the masters times its quantities
        // is an othersubr this interpreter does not know, in a program that
        // blends as well.
        let mut cs = Vec::new();
        for v in [10, 500, 20, 40, 60] {
            push_number(v, &mut cs);
        }
        push_number(5, &mut cs);
        push_number(15, &mut cs);
        cs.extend_from_slice(&[12, 16, 12, 17, 12, 17, 13, 14]);
        let font = Type1::parse(&mm_program(FOUR_MASTERS, &[("square", cs)])).expect("it parses");
        assert!(font.is_multiple_master());
        assert_eq!(
            font.advance(0),
            Some(500.0),
            "the first master's width, popped as written"
        );
    }

    /// Othersubrs 17 and 18, the four- and six-quantity blends, on a one-axis
    /// program of two masters.
    #[test]
    fn the_four_and_six_quantity_blends_return_their_quantities_in_order() {
        let keys = "/FontInfo 4 dict dup begin\n\
             /BlendAxisTypes [/Weight] def\n\
             /BlendDesignPositions [[0] [1]] def\n\
             /BlendDesignMap [[[0 0] [1000 1]]] def\n\
             end readonly def\n\
             /WeightVector [0.5 0.5] def\n";
        let hsbw = vec![vec![20, 600], vec![40, 800]];
        let moveto = vec![vec![0, 0], vec![10, 10]];
        let curve = vec![
            vec![100, 0, 100, 100, 0, 100],
            vec![140, 0, 160, 120, 0, 160],
        ];
        let vh = vec![vec![-100, -50, -50, -100], vec![-160, -60, -80, -120]];
        let mut cs = Vec::new();
        blended(&hsbw, &mut cs);
        cs.push(13);
        blended(&moveto, &mut cs);
        cs.push(21);
        blended(&curve, &mut cs);
        cs.push(8);
        blended(&vh, &mut cs);
        cs.push(30); // vhcurveto
        cs.extend_from_slice(&[9, 14]);
        let mut font = Type1::parse(&mm_program(keys, &[("curve", cs)])).expect("it parses");

        for (design, weights) in [(None, [0.5, 0.5]), (Some(250.0), [0.75, 0.25])] {
            if let Some(design) = design {
                let instance = font.instance(&[design]).expect("it instances");
                assert_eq!(instance.weights, weights.to_vec());
            }
            let h = mix(&hsbw, &weights);
            let m = mix(&moveto, &weights);
            let c = mix(&curve, &weights);
            let v = mix(&vh, &weights);
            let start = (h[0] + m[0], m[1]);
            let c1 = (start.0 + c[0], start.1 + c[1]);
            let c2 = (c1.0 + c[2], c1.1 + c[3]);
            let end = (c2.0 + c[4], c2.1 + c[5]);
            let d1 = (end.0, end.1 + v[0]);
            let d2 = (d1.0 + v[1], d1.1 + v[2]);
            let back = (d2.0 + v[3], d2.1);
            assert_eq!(
                font.outline(0).expect("it draws").segments,
                vec![
                    Segment::MoveTo {
                        x: start.0,
                        y: start.1
                    },
                    Segment::CurveTo {
                        c1x: c1.0,
                        c1y: c1.1,
                        c2x: c2.0,
                        c2y: c2.1,
                        x: end.0,
                        y: end.1
                    },
                    Segment::CurveTo {
                        c1x: d1.0,
                        c1y: d1.1,
                        c2x: d2.0,
                        c2y: d2.1,
                        x: back.0,
                        y: back.1
                    },
                    Segment::Close,
                ],
                "{design:?}"
            );
            assert_eq!(font.advance(0), Some(h[1]));
        }
    }

    /// Section 3.8's normalisation: a map's own points answer their own
    /// blend coordinates, a segment interpolates, and past the ends a
    /// coordinate takes the end and says so.
    #[test]
    fn a_design_coordinate_is_normalised_through_its_axis_map() {
        let font = four_master_font();
        let blend = font.blend().expect("a design");
        assert_eq!(blend.normalise(0, 100.0), Some((0.0, false)));
        assert_eq!(blend.normalise(0, 500.0), Some((0.5, false)));
        assert_eq!(blend.normalise(0, 900.0), Some((1.0, false)));
        assert_eq!(blend.normalise(0, 99.0), Some((0.0, true)));
        assert_eq!(blend.normalise(0, 901.0), Some((1.0, true)));
        assert_eq!(blend.normalise(1, 400.0), Some((0.5, false)));
        assert_eq!(blend.normalise(1, 200.0), Some((0.25, false)));
        assert_eq!(blend.normalise(1, 700.0), Some((0.75, false)));
        assert_eq!(blend.normalise(2, 0.0), None, "no third axis");

        // Appendix C's Minion optical-size axis, segment by segment.
        let minion = Blend {
            axes: vec![b"OpticalSize".to_vec()],
            positions: vec![vec![0.0], vec![1.0]],
            maps: vec![vec![
                (6.0, 0.0),
                (8.0, 0.35),
                (11.0, 0.5),
                (18.0, 0.75),
                (72.0, 1.0),
            ]],
        };
        assert!(minion.is_standard());
        assert_eq!(minion.normalise(0, 8.0), Some((0.35, false)));
        assert_eq!(minion.normalise(0, 11.0), Some((0.5, false)));
        let (at_45, _) = minion.normalise(0, 45.0).expect("a coordinate");
        assert_eq!(at_45, 0.75 + (45.0 - 18.0) * (1.0 - 0.75) / (72.0 - 18.0));
        assert_eq!(
            minion.instance(&[11.0]),
            Ok(Instance {
                weights: vec![0.5, 0.5],
                clamped: false
            })
        );
    }

    #[test]
    fn postscript_numbers_read_as_written() {
        assert_eq!(ps_number(b"0.125"), Some(0.125));
        assert_eq!(ps_number(b".53"), Some(0.53));
        assert_eq!(ps_number(b"-12"), Some(-12.0));
        assert_eq!(ps_number(b"+3"), Some(3.0));
        assert_eq!(ps_number(b"1e-3"), Some(0.001));
        assert_eq!(ps_number(b"inf"), None);
        assert_eq!(ps_number(b"NaN"), None);
        assert_eq!(ps_number(b"1e400"), None);
        assert_eq!(ps_number(b"-"), None);
        assert_eq!(ps_number(b""), None);
        assert_eq!(ps_number(b"/Weight"), None);
    }

    /// A cleartext array is read within its limits and no further: nested
    /// past three levels, holding more than 256 values, or never closing, it
    /// is not read, and a definition that is no array is passed over.
    #[test]
    fn the_cleartext_arrays_are_bounded() {
        assert!(read_array(b"/K [1 [2 [3]]] def", b"/K").is_some());
        assert!(read_array(b"/K [1 [2 [3 [4]]]] def", b"/K").is_none());
        let many = format!("/K [{}] def", "1 ".repeat(MAX_ARRAY_ITEMS + 1));
        assert!(read_array(many.as_bytes(), b"/K").is_none());
        let enough = format!("/K [{}] def", "1 ".repeat(MAX_ARRAY_ITEMS));
        assert!(read_array(enough.as_bytes(), b"/K").is_some());
        assert!(read_array(b"/K [1 2", b"/K").is_none());
        assert!(read_array(b"/K [1 (two)] def", b"/K").is_none());
        assert_eq!(
            read_array(b"/K get /K [1 /n] def", b"/K"),
            Some(Value::Array(vec![
                Value::Number(1.0),
                Value::Name(b"n".to_vec())
            ]))
        );
        assert!(read_array(b"/Kx [1] def", b"/K").is_none());
    }

    /// Every truncation of a multiple-master program, and every instance asked
    /// of what it parses to, is survived.
    #[test]
    fn every_truncation_of_a_multiple_master_program_is_survivable() {
        let full = mm_program(
            FOUR_MASTERS,
            &[("square", square_charstring(&four_masters()))],
        );
        for cut in (0..full.len()).step_by(3) {
            if let Some(mut font) = Type1::parse(&full[..cut]) {
                for design in [[500.0, 700.0], [0.0, 0.0]] {
                    let _ = font.instance(&design);
                    for glyph in 0..font.glyph_count().min(4) as u16 {
                        let _ = font.outline(glyph);
                        let _ = font.advance(glyph);
                    }
                }
            }
        }
    }

    /// The `type1` fuzz target's multiple-master seed, the program the tests
    /// above instance, written by this file's own builder rather than
    /// fetched: run by name, `cargo test -p tinker-pdf-font --lib --
    /// --ignored type1::tests::write_the_multiple_master_fuzz_seed --exact`,
    /// and its output committed.
    #[test]
    #[ignore = "writes fuzz/corpus/type1/multiple-master.t1; run it when the program it is built from changes"]
    fn write_the_multiple_master_fuzz_seed() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fuzz/corpus/type1/multiple-master.t1");
        let program = mm_program(
            FOUR_MASTERS,
            &[("square", square_charstring(&four_masters()))],
        );
        std::fs::write(path, program).expect("the seed is written");
    }

    /// Chapter 6's `seac` composes two StandardEncoding glyphs, and a
    /// component is drawn as its own charstring. A `seac` naming itself — `A`
    /// is StandardEncoding code 65 — recursed until the stack ran out; it now
    /// draws its base as the base's own charstring, which composes nothing.
    #[test]
    fn a_seac_that_names_itself_is_drawn_and_not_followed() {
        let mut seac = Vec::new();
        num(0, &mut seac);
        num(600, &mut seac);
        seac.push(13); // hsbw
        for v in [0, 100, 200, 65, 65] {
            num(v, &mut seac);
        }
        seac.extend([12, 6]); // seac: asb adx ady bchar achar
        let looping = program(&[(b"A", seac)], &[]);
        let font = Type1::parse(&looping).expect("it parses");
        let outline = font.outline(0).expect("it composes");
        assert!(
            outline.is_empty(),
            "the components are `A` itself, whose own charstring draws nothing"
        );

        // A well-formed accented glyph still composes both components.
        let mut aacute = Vec::new();
        num(0, &mut aacute);
        num(600, &mut aacute);
        aacute.push(13);
        for v in [0, 100, 200, 65, 194] {
            num(v, &mut aacute);
        }
        aacute.extend([12, 6]);
        let accented = program(
            &[
                (b"A".as_slice(), box_charstring(0, 600, 500)),
                (b"acute".as_slice(), box_charstring(0, 300, 50)),
                (b"Aacute".as_slice(), aacute),
            ],
            &[],
        );
        let font = Type1::parse(&accented).expect("it parses");
        let base = font.outline(0).expect("A").segments;
        let accent = font.outline(1).expect("acute").segments;
        let composed = font.outline(2).expect("Aacute").segments;
        assert_eq!(composed.len(), base.len() + accent.len());
        assert_eq!(composed[..base.len()], base[..]);
        assert_eq!(
            composed[base.len()..].to_vec(),
            accent
                .iter()
                .map(|s| shift(*s, 100.0, 200.0))
                .collect::<Vec<_>>(),
            "the accent moved by (adx, ady)"
        );
    }

    /// A negative subroutine length ends the array, as a length past the
    /// section does: the entries before it are read, and nothing overflows.
    #[test]
    fn a_negative_subroutine_length_ends_the_array() {
        let mut private = b"/Subrs 2 array\ndup 0 3 RD ".to_vec();
        private.extend(encrypt(&[11], CHARSTRING_KEY, &[]));
        private.extend_from_slice(b"xx NP\ndup 1 -5 RD xxxxx NP\nND\n");
        let scan = scan_subrs(&private);
        assert_eq!(scan.entries.len(), 1);
        assert_eq!(scan.entries[0].key, Key::Subr(0));
        let subrs = read_subrs(&private, 0, &mut 0);
        assert_eq!(subrs.len(), 1);
    }

    /// The scanner's spans are where the entries are: `dup`'s `d` or the
    /// name's `/`, the length's first digit, the `RD` token and the bytes.
    #[test]
    fn the_scanner_keeps_where_each_entry_is() {
        let private = b"/Subrs 1 array\ndup 0 2 -| ab |\nND\n/CharStrings 1 dict dup begin\n/one 3 RD xyz ND\nend\n";
        let subrs = scan_subrs(private);
        assert_eq!(subrs.entries.len(), 1);
        let entry = &subrs.entries[0];
        assert_eq!(&private[entry.start..entry.start + 3], b"dup");
        assert_eq!(private[entry.length], b'2');
        assert_eq!(&private[entry.rd.clone()], b"-|");
        assert_eq!(&private[entry.data.clone()], b"ab");

        let glyphs = scan_charstrings(private);
        assert_eq!(glyphs.entries.len(), 1);
        let entry = &glyphs.entries[0];
        assert_eq!(entry.key, Key::Glyph(b"one".to_vec()));
        assert_eq!(&private[entry.start..entry.start + 4], b"/one");
        assert_eq!(private[entry.length], b'3');
        assert_eq!(&private[entry.rd.clone()], b"RD");
        assert_eq!(&private[entry.data.clone()], b"xyz");
    }

    /// An index past the 65 535 the reader stores is passed over on every
    /// target, a 32-bit one included, rather than folded onto a small index.
    #[test]
    fn a_subroutine_index_past_65535_is_passed_over() {
        let mut private = b"/Subrs 1 array\ndup 4294967301 1 RD ".to_vec();
        private.push(0);
        private.extend_from_slice(b" NP\ndup 2 1 RD ");
        private.push(0);
        private.extend_from_slice(b" NP\nND\n");
        let subrs = read_subrs(&private, 0, &mut 0);
        assert_eq!(subrs.len(), 3, "index 2 is stored and the huge one is not");
    }

    /// A traced run records the subroutines it calls, the hint replacements
    /// it asks for and the bounds it meets; a drawing run records nothing.
    #[test]
    fn a_trace_records_calls_hint_replacements_and_bounds() {
        // Subroutine 4 calls subroutine 5; the glyph asks for hint
        // replacement with subroutine 6, calls 4, and calls 9, which is not
        // there.
        let mut sub4 = Vec::new();
        num(5, &mut sub4);
        sub4.push(10);
        sub4.push(11);
        let sub5 = vec![11u8];
        let sub6 = vec![11u8];
        let mut glyph = Vec::new();
        num(0, &mut glyph);
        num(500, &mut glyph);
        glyph.push(13);
        for v in [6, 1, 3] {
            num(v, &mut glyph);
        }
        glyph.extend([12, 16, 12, 17]); // callothersubr pop
        glyph.push(10); // callsubr 3, the conventional answer
        num(4, &mut glyph);
        glyph.push(10);
        num(9, &mut glyph);
        glyph.push(10);
        glyph.push(14);
        let calling = program(
            &[(b"g", glyph)],
            &[(3, vec![11]), (4, sub4), (5, sub5), (6, sub6)],
        );
        let font = Type1::parse(&calling).expect("it parses");
        let (trace, steps) = font.trace_glyph(0).expect("the glyph is there");
        assert!(steps > 0);
        assert_eq!(trace.called, [3, 4, 5, 9].into_iter().collect());
        assert_eq!(trace.hints, [6].into_iter().collect());
        assert_eq!(
            trace.bound,
            Some(Bound::Missing),
            "subroutine 9 is not there"
        );

        // A subroutine that calls itself meets the depth cap.
        let mut recursive = Vec::new();
        num(0, &mut recursive);
        recursive.push(10);
        recursive.push(11);
        let mut glyph = Vec::new();
        num(0, &mut glyph);
        glyph.push(10);
        glyph.push(14);
        let recursing = program(&[(b"g", glyph)], &[(0, recursive)]);
        let font = Type1::parse(&recursing).expect("it parses");
        let (trace, _) = font.trace_glyph(0).expect("the glyph is there");
        assert_eq!(trace.bound, Some(Bound::Depth));

        // Forty-nine operands overflow the stack.
        let mut glyph = Vec::new();
        for _ in 0..49 {
            num(1, &mut glyph);
        }
        glyph.push(14);
        let overflowing = program(&[(b"g", glyph)], &[]);
        let font = Type1::parse(&overflowing).expect("it parses");
        let (trace, _) = font.trace_glyph(0).expect("the glyph is there");
        assert_eq!(trace.bound, Some(Bound::Stack));

        // And a charstring longer than the budget spends it.
        let glyph = vec![9u8; MAX_OPS as usize + 1];
        let overflowing = program(&[(b"g", glyph)], &[]);
        let font = Type1::parse(&overflowing).expect("it parses");
        let (trace, _) = font.trace_glyph(0).expect("the glyph is there");
        assert_eq!(trace.bound, Some(Bound::Budget));
    }

    // ---- FT-16.3: `/FontBBox`, `/FontInfo` and the Private dictionary -----

    /// FT-16.3: `/FontBBox`, `/FontInfo`'s `/isFixedPitch`, `/ItalicAngle`,
    /// `/UnderlinePosition`, `/UnderlineThickness` and `/Weight`, and the
    /// Private dictionary's `/ForceBold` and `/StdVW`, read as the cleartext
    /// and the Private dictionary written in the test state them; every
    /// absent one its default; and the committed `square.pfb`, whose
    /// cleartext, read here, states none of them.
    #[test]
    fn font_bbox_and_font_info_are_read() {
        let program = square_with(
            "/FontInfo 6 dict dup begin\n\
             /version (001.000) readonly def\n\
             /Weight (Bold) readonly def\n\
             /ItalicAngle -12.5 def\n\
             /isFixedPitch true def\n\
             /UnderlinePosition -75 def\n\
             /UnderlineThickness 40 def\n\
             end readonly def\n\
             /FontBBox {-50 -250 1050 900} readonly def\n",
            "/ForceBold true def\n/StdVW [88] def\n",
        );
        let font = Type1::parse(&program).expect("it parses");
        assert_eq!(
            font.style(),
            &Style {
                font_bbox: [-50.0, -250.0, 1050.0, 900.0],
                is_fixed_pitch: true,
                italic_angle: -12.5,
                underline_position: -75.0,
                underline_thickness: 40.0,
                weight: Some("Bold".to_string()),
                force_bold: true,
                std_vw: Some(88.0),
            }
        );
        assert_eq!(font.glyph_count(), 1, "the program still reads whole");

        // An array, rather than a procedure, and a real in it.
        let font = Type1::parse(&square_with(
            "/FontBBox [0 -10 500.5 700] readonly def\n",
            "/ForceBold false def\n",
        ))
        .expect("it parses");
        assert_eq!(font.style().font_bbox, [0.0, -10.0, 500.5, 700.0]);
        assert!(!font.style().force_bold);
        assert_eq!(font.style().std_vw, None);

        // Values that are not what their keys take read as absent, and a
        // name that only begins as a key's does, or a key that defines
        // nothing, is passed over for the definition after it.
        let font = Type1::parse(&square_with(
            "/FontBBox {1 2 3} readonly def\n\
             /isFixedPitch maybe def\n\
             /ItalicAngle x def\n\
             /WeightVector [0.5 0.5] def\n\
             /BlendAxisTypes [/Weight /Width] def\n\
             /Weight (Extra \\(Semi\\) (light)) readonly def\n",
            "",
        ))
        .expect("it parses");
        assert_eq!(font.style().font_bbox, [0.0; 4]);
        assert!(!font.style().is_fixed_pitch);
        assert_eq!(font.style().italic_angle, 0.0);
        assert_eq!(
            font.style().weight.as_deref(),
            Some("Extra (Semi) (light)"),
            "escapes taken as written, a balanced pair kept"
        );
        let font = Type1::parse(&square_with("/Weight /Bold def\n", "")).expect("it parses");
        assert_eq!(font.style().weight, None, "a name is no string");

        // Nothing stated: Table 9's defaults.
        let defaults = Style {
            font_bbox: [0.0; 4],
            is_fixed_pitch: false,
            italic_angle: 0.0,
            underline_position: -100.0,
            underline_thickness: 50.0,
            weight: None,
            force_bold: false,
            std_vw: None,
        };
        assert_eq!(Style::default(), defaults);
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert_eq!(font.style(), &defaults);

        // The committed seed is that program, and its cleartext — before
        // `eexec` — names none of the keys, so every value is its default.
        let seed = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fuzz/corpus/type1/square.pfb"),
        )
        .expect("the committed seed");
        assert_eq!(seed, font_with_square(), "the seed is the fixture");
        let clear = &seed[..find(&seed, b"eexec").expect("a cleartext half")];
        for key in [
            &b"/FontBBox"[..],
            b"/FontInfo",
            b"/ItalicAngle",
            b"/isFixedPitch",
            b"/Underline",
            b"/Weight",
        ] {
            assert!(
                find(clear, key).is_none(),
                "{}",
                String::from_utf8_lossy(key)
            );
        }
        assert_eq!(Type1::parse(&seed).expect("it parses").style(), &defaults);

        // The multiple-master seed's `/FontInfo` holds its blend design and
        // nothing a style reads; `/BlendAxisTypes` names `/Weight` and
        // `/WeightVector` begins with it, and neither is a `/Weight`.
        let seed = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fuzz/corpus/type1/multiple-master.t1"),
        )
        .expect("the committed seed");
        assert_eq!(Type1::parse(&seed).expect("it parses").style(), &defaults);
    }
}
