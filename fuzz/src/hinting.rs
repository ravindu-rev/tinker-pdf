//! The `truetype_hinting` target's input and its run (FT-04b.13): arbitrary
//! `fpgm`, `prep`, `cvt ` and glyph programs on a face built from them, hinted
//! at the sizes the input chooses (`docs/design/hinting.md`).
//!
//! The input is carved into fields rather than parsed as a font, so every
//! byte lands in a program, a control value, a point or a setting, and none is
//! spent on a table directory a mutation would only break:
//!
//! - byte 0: which of [`SIZES`] to hint at, a bit each, and 12 pixels an em
//!   for none;
//! - byte 1: [`MONOCHROME`], [`INTEGER_PPEM`], [`GASP`], and the composite's
//!   [`ROUND_XY`], [`MATCHED`], [`SCALED`] and [`MY_METRICS`];
//! - bytes 2 to 11: `maxp`'s `maxTwilightPoints`, `maxStorage`,
//!   `maxFunctionDefs`, `maxInstructionDefs` and `maxStackElements`, each a
//!   big-endian uint16;
//! - then the `cvt ` table, `fpgm`, `prep`, glyph 0's program and glyph 1's,
//!   each a big-endian uint16 length and that many bytes, cut to what the
//!   input holds;
//! - then glyph 0's points, five bytes each — x and y as big-endian int16,
//!   and a byte whose bit 0 puts the point on the curve and bit 1 ends a
//!   contour there — at most [`MAX_POINTS`], the last always ending one.
//!
//! An input that runs out reads as zeros. The face is glyph 0, a simple glyph
//! of those points and its program, and glyph 1, a composite of glyph 0 twice
//! — once at an offset and once by the options' placement — and its program,
//! in `head`, `hhea`, `hmtx`, `maxp`, `loca`, `glyf`, the three program tables
//! and, under [`GASP`], a `gasp` that grid-fits to 16 pixels an em and no
//! further.
//!
//! The file reads `std` and `tinker_pdf_font` alone, so the font crate's
//! `tests/fuzz_seeds.rs` includes it by path to write the corpus with
//! [`Input::encode`] and replay it on stable, as the target runs it.

use tinker_pdf_font::hint::{Face, Hinted, Unhinted, MAX_POINTS as ZONE_POINTS};
use tinker_pdf_font::{Segment, Sfnt};

/// The sizes byte 0's bits choose, in whole pixels per em: below a pixel's
/// grid at the first, the small sizes, two larger ones, and the largest a
/// glyph is hinted at.
pub const SIZES: [i32; 8] = [1, 7, 9, 12, 16, 33, 72, 1_024];

/// The most points glyph 0 takes from the input.
pub const MAX_POINTS: usize = 64;

/// Byte 1, bit 0: hinted for a glyph drawn whole or empty.
pub const MONOCHROME: u8 = 1;
/// Byte 1, bit 1: `head` asks for integer pixels per em.
pub const INTEGER_PPEM: u8 = 2;
/// Byte 1, bit 2: the face carries a `gasp`.
pub const GASP: u8 = 4;
/// Byte 1, bit 3: the composite's first component rounds its offset to the
/// grid.
pub const ROUND_XY: u8 = 8;
/// Byte 1, bit 4: the composite's second component is placed by matching its
/// point 1 to the composite's point 0, rather than at no offset.
pub const MATCHED: u8 = 16;
/// Byte 1, bit 5: the composite's second component is scaled by a half.
pub const SCALED: u8 = 32;
/// Byte 1, bit 6: the composite takes its first component's metrics.
pub const MY_METRICS: u8 = 64;

/// The steps a run may take (`hint::vm::BUDGET`).
const BUDGET: u64 = 1 << 20;

/// What one input describes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// Byte 0: which of [`SIZES`] to hint at.
    pub sizes: u8,
    /// Byte 1: the options.
    pub options: u8,
    /// `maxp`'s five limits, in the table's order.
    pub profile: [u16; 5],
    /// The `cvt ` table's bytes.
    pub cvt: Vec<u8>,
    /// The font program.
    pub fpgm: Vec<u8>,
    /// The control value program.
    pub prep: Vec<u8>,
    /// Glyph 0's program.
    pub glyph: Vec<u8>,
    /// Glyph 1's program.
    pub composite: Vec<u8>,
    /// Glyph 0's points: x, y and the point's byte.
    pub points: Vec<(i16, i16, u8)>,
}

/// A reader over the input that answers zeros once it runs out.
struct Fields<'a> {
    data: &'a [u8],
    at: usize,
}

impl Fields<'_> {
    fn byte(&mut self) -> u8 {
        let byte = self.data.get(self.at).copied().unwrap_or(0);
        self.at = self.at.saturating_add(1);
        byte
    }

    fn u16(&mut self) -> u16 {
        u16::from_be_bytes([self.byte(), self.byte()])
    }

    /// A field: its length, then as many of its bytes as the input holds.
    fn field(&mut self) -> Vec<u8> {
        let length = usize::from(self.u16());
        let start = self.at.min(self.data.len());
        let end = start.saturating_add(length).min(self.data.len());
        self.at = self.at.saturating_add(length);
        self.data[start..end].to_vec()
    }
}

impl Input {
    /// The input `data` describes: every byte read as the module says, and
    /// zeros past its end.
    #[must_use]
    pub fn decode(data: &[u8]) -> Input {
        let mut fields = Fields { data, at: 0 };
        let sizes = fields.byte();
        let options = fields.byte();
        let profile = [(); 5].map(|()| fields.u16());
        let cvt = fields.field();
        let fpgm = fields.field();
        let prep = fields.field();
        let glyph = fields.field();
        let composite = fields.field();
        let mut points = Vec::new();
        while points.len() < MAX_POINTS && fields.at.saturating_add(5) <= data.len() {
            let x = fields.u16() as i16;
            let y = fields.u16() as i16;
            points.push((x, y, fields.byte()));
        }
        Input {
            sizes,
            options,
            profile,
            cvt,
            fpgm,
            prep,
            glyph,
            composite,
            points,
        }
    }

    /// The bytes [`Input::decode`] reads back as this input: each field at
    /// the length it has, which a field longer than a uint16 counts cannot
    /// have, and the points past [`MAX_POINTS`] left out.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.sizes, self.options];
        for limit in self.profile {
            out.extend(limit.to_be_bytes());
        }
        for field in [
            &self.cvt,
            &self.fpgm,
            &self.prep,
            &self.glyph,
            &self.composite,
        ] {
            let field = &field[..field.len().min(usize::from(u16::MAX))];
            out.extend((field.len() as u16).to_be_bytes());
            out.extend(field);
        }
        for &(x, y, flags) in self.points.iter().take(MAX_POINTS) {
            out.extend(x.to_be_bytes());
            out.extend(y.to_be_bytes());
            out.push(flags);
        }
        out
    }

    /// The sizes byte 0 chooses, in pixels per em, 12 where it chooses none.
    #[must_use]
    pub fn sizes(&self) -> Vec<i32> {
        let chosen: Vec<i32> = SIZES
            .iter()
            .enumerate()
            .filter(|&(bit, _)| self.sizes & (1 << bit) != 0)
            .map(|(_, &size)| size)
            .collect();
        if chosen.is_empty() {
            vec![12]
        } else {
            chosen
        }
    }

    /// Glyph 0's `glyf` entry: its points in contours, each ending at a point
    /// whose bit 1 is set and the last at the last point, its program, every
    /// flag spelled out and every delta a word; empty, a glyph with no
    /// outline, where it has no point.
    fn simple(&self) -> Vec<u8> {
        if self.points.is_empty() {
            return Vec::new();
        }
        let last = self.points.len() - 1;
        let ends: Vec<u16> = self
            .points
            .iter()
            .enumerate()
            .filter(|&(i, &(_, _, flags))| flags & 2 != 0 || i == last)
            .map(|(i, _)| i as u16)
            .collect();
        let mut g = Vec::new();
        g.extend((ends.len() as i16).to_be_bytes());
        let xs = self.points.iter().map(|p| p.0);
        let ys = self.points.iter().map(|p| p.1);
        for v in [
            xs.clone().min().unwrap_or(0),
            ys.clone().min().unwrap_or(0),
            xs.max().unwrap_or(0),
            ys.max().unwrap_or(0),
        ] {
            g.extend(v.to_be_bytes());
        }
        for end in &ends {
            g.extend(end.to_be_bytes());
        }
        let program = &self.glyph[..self.glyph.len().min(usize::from(u16::MAX))];
        g.extend((program.len() as u16).to_be_bytes());
        g.extend(program);
        for &(_, _, flags) in &self.points {
            g.push(flags & 1);
        }
        let mut previous = 0i16;
        for &(x, _, _) in &self.points {
            g.extend(x.wrapping_sub(previous).to_be_bytes());
            previous = x;
        }
        previous = 0;
        for &(_, y, _) in &self.points {
            g.extend(y.wrapping_sub(previous).to_be_bytes());
            previous = y;
        }
        g
    }

    /// Glyph 1's `glyf` entry: glyph 0 at (37, −21), rounded to the grid
    /// under [`ROUND_XY`] and lending its metrics under [`MY_METRICS`], then
    /// glyph 0 again — its point 1 matched to the composite's point 0 under
    /// [`MATCHED`], at no offset otherwise, and scaled by a half under
    /// [`SCALED`] — and the composite's program after the last record.
    fn composite(&self) -> Vec<u8> {
        let mut g = Vec::new();
        g.extend((-1i16).to_be_bytes());
        g.extend([0u8; 8]);
        // ARG_1_AND_2_ARE_WORDS, ARGS_ARE_XY_VALUES and MORE_COMPONENTS.
        let mut first = 0x0001u16 | 0x0002 | 0x0020;
        if self.options & ROUND_XY != 0 {
            first |= 0x0004;
        }
        if self.options & MY_METRICS != 0 {
            first |= 0x0200;
        }
        g.extend(first.to_be_bytes());
        g.extend(0u16.to_be_bytes());
        g.extend(37i16.to_be_bytes());
        g.extend((-21i16).to_be_bytes());
        let program = &self.composite[..self.composite.len().min(usize::from(u16::MAX))];
        let mut second = 0x0001u16;
        if self.options & MATCHED == 0 {
            second |= 0x0002;
        }
        if self.options & SCALED != 0 {
            second |= 0x0008;
        }
        if !program.is_empty() {
            second |= 0x0100;
        }
        g.extend(second.to_be_bytes());
        g.extend(0u16.to_be_bytes());
        let (one, two) = if self.options & MATCHED != 0 {
            (0i16, 1i16)
        } else {
            (0, 0)
        };
        g.extend(one.to_be_bytes());
        g.extend(two.to_be_bytes());
        if self.options & SCALED != 0 {
            g.extend(8_192i16.to_be_bytes());
        }
        if !program.is_empty() {
            g.extend((program.len() as u16).to_be_bytes());
            g.extend(program);
        }
        g
    }

    /// The face: `head` at 2 048 units an em, its flags bit 3 under
    /// [`INTEGER_PPEM`]; `hhea` with an ascender of 1 600 and a descender of
    /// −400; `hmtx`, each glyph 1 200 wide at a side bearing of 0; `maxp`
    /// version 1.0 with the input's limits; a long `loca`; `glyf`; the
    /// input's `cvt `, `fpgm` and `prep`; and under [`GASP`] a `gasp`.
    #[must_use]
    pub fn face(&self) -> Vec<u8> {
        let mut head = vec![0u8; 54];
        let flags: u16 = if self.options & INTEGER_PPEM != 0 {
            1 << 3
        } else {
            0
        };
        head[16..18].copy_from_slice(&flags.to_be_bytes());
        head[18..20].copy_from_slice(&2_048u16.to_be_bytes());
        head[50..52].copy_from_slice(&1i16.to_be_bytes());
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&1_600i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-400i16).to_be_bytes());
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());
        let mut hmtx = Vec::new();
        for _ in 0..2 {
            hmtx.extend(1_200u16.to_be_bytes());
            hmtx.extend(0i16.to_be_bytes());
        }
        let mut maxp = vec![0u8; 32];
        maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&2u16.to_be_bytes());
        for (k, limit) in self.profile.iter().enumerate() {
            let at = 16 + 2 * k;
            maxp[at..at + 2].copy_from_slice(&limit.to_be_bytes());
        }
        let (simple, composite) = (self.simple(), self.composite());
        let mut loca = vec![0u8; 4];
        loca.extend((simple.len() as u32).to_be_bytes());
        loca.extend(((simple.len() + composite.len()) as u32).to_be_bytes());
        let mut glyf = simple;
        glyf.extend(composite);
        let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"maxp", maxp),
            (*b"loca", loca),
            (*b"glyf", glyf),
            (*b"cvt ", self.cvt.clone()),
            (*b"fpgm", self.fpgm.clone()),
            (*b"prep", self.prep.clone()),
        ];
        if self.options & GASP != 0 {
            let mut gasp = Vec::new();
            for value in [1u16, 2, 16, 1, 0xFFFF, 0] {
                gasp.extend(value.to_be_bytes());
            }
            tables.push((*b"gasp", gasp));
        }
        let mut out = Vec::new();
        out.extend(0x0001_0000u32.to_be_bytes());
        out.extend((tables.len() as u16).to_be_bytes());
        out.extend([0u8; 6]);
        let directory = 12 + 16 * tables.len();
        let mut body: Vec<u8> = Vec::new();
        for (tag, data) in &tables {
            out.extend(tag);
            out.extend(0u32.to_be_bytes());
            out.extend(((directory + body.len()) as u32).to_be_bytes());
            out.extend((data.len() as u32).to_be_bytes());
            body.extend(data);
            while body.len() % 4 != 0 {
                body.push(0);
            }
        }
        out.extend(body);
        out
    }
}

/// What hinting one input came to: the face's answer, and at each size the
/// size's answer and then each glyph's.
#[derive(Clone, Debug)]
pub struct Report {
    /// The face: whether its `fpgm` ran.
    pub face: Result<(), Unhinted>,
    /// Each size hinted at, in pixels per em, with its `prep`'s answer and,
    /// where that ran, glyph 0's and glyph 1's.
    pub sizes: Vec<(i32, Result<(), Unhinted>, Vec<Result<Hinted, Unhinted>>)>,
}

impl Report {
    /// Every answer the run gave, the face's first: each a hint or the named
    /// reason it was refused.
    #[must_use]
    pub fn refusals(&self) -> Vec<Unhinted> {
        let mut out: Vec<Unhinted> = self.face.err().into_iter().collect();
        for (_, size, glyphs) in &self.sizes {
            out.extend(size.err());
            out.extend(
                glyphs
                    .iter()
                    .filter_map(|glyph| glyph.as_ref().err().copied()),
            );
        }
        out
    }
}

/// Holds what a hinted glyph is to what it could be: an outline inside
/// `Outline`'s segment cap with every coordinate a finite multiple of 1/128,
/// four phantom points, and steps within its runs' budgets — one run for
/// glyph 0, and for glyph 1 its program and each of its two components'.
fn check(glyph: usize, hinted: &Hinted) {
    assert!(
        hinted.outline.segments.len() <= 1 << 16,
        "glyph {glyph} passed the segment cap"
    );
    for segment in &hinted.outline.segments {
        let values: Vec<f64> = match *segment {
            Segment::MoveTo { x, y } | Segment::LineTo { x, y } => vec![x, y],
            Segment::QuadTo { cx, cy, x, y } => vec![cx, cy, x, y],
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => vec![c1x, c1y, c2x, c2y, x, y],
            Segment::Close => Vec::new(),
        };
        for v in values {
            assert!(
                v.is_finite() && (v * 128.0).fract() == 0.0,
                "glyph {glyph}: {v} is not a hinted coordinate"
            );
        }
    }
    let runs = if glyph == 0 { 1 } else { 3 };
    assert!(
        hinted.steps <= runs * BUDGET,
        "glyph {glyph} took {} steps, past {runs} runs' budget",
        hinted.steps
    );
    assert!(
        hinted.ran_program || hinted.steps == 0,
        "glyph {glyph} counted steps without a program"
    );
}

/// The target's run over `data`: the face built, its `fpgm` run, and at each
/// size it chooses its `prep` and then glyph 0 and glyph 1 hinted, every
/// hinted glyph held by [`check`] and every refusal a named [`Unhinted`].
///
/// # Panics
///
/// Where the face built from the input does not parse, which no input can
/// make it do, and where a hinted glyph breaks what [`check`] holds — the
/// findings the target exists to make.
#[must_use]
pub fn exercise(data: &[u8]) -> Report {
    let input = Input::decode(data);
    let bytes = input.face();
    let sfnt = Sfnt::parse(&bytes).expect("the face built from the input parses");
    let face = match Face::new(&sfnt, None) {
        Ok(face) => face,
        Err(why) => {
            return Report {
                face: Err(why),
                sizes: Vec::new(),
            }
        }
    };
    let monochrome = input.options & MONOCHROME != 0;
    let mut sizes = Vec::new();
    for pixels in input.sizes() {
        let ppem = pixels * 64;
        match face.at_size(ppem, ppem, monochrome, None) {
            Err(why) => sizes.push((pixels, Err(why), Vec::new())),
            Ok(at) => {
                let glyphs: Vec<Result<Hinted, Unhinted>> = (0..2u16)
                    .map(|glyph| {
                        let result = at.glyph(&sfnt, glyph, None);
                        if let Ok(hinted) = &result {
                            check(usize::from(glyph), hinted);
                        }
                        if let Err(Unhinted::TooManyPoints) = result {
                            panic!(
                                "glyph {glyph} assembled past {ZONE_POINTS} points from at \
                                 most {MAX_POINTS} twice"
                            );
                        }
                        result
                    })
                    .collect();
                sizes.push((pixels, Ok(()), glyphs));
            }
        }
    }
    Report {
        face: Ok(()),
        sizes,
    }
}
