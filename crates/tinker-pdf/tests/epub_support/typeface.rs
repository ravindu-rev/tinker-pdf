//! Font programs a test can state the coverage of (gap 31, milestone 9).
//!
//! `tests/substitute_fonts.rs` records why a fixture face is **synthesised**
//! rather than read from the system: the test is then identical on every
//! platform and the repository carries no font anybody has to licence. That
//! argument is unchanged here; what changed is what a face has to be able to
//! say.
//!
//! # `boxy_font` cannot declare coverage, and milestone 9 is about coverage
//!
//! The face `tests/substitute_fonts.rs` and `tests/epub_package.rs` share emits
//! `head`, `loca` and `glyf` and nothing else. That is exactly enough to answer
//! *"did a glyph get drawn"* for a [`tinker_pdf::FontProvider`], which reaches
//! a glyph by code and never asks a `cmap` anything. It cannot answer *"which
//! characters does this face have"*, because it has no `cmap` to answer with —
//! and `css-fonts-4` §5.3's per-character matching is that question and nothing
//! else. So [`covering`] builds a real one.
//!
//! # What "real" has to mean here, and the three tables that had to be right
//!
//! - **`cmap`, format 4**, in the (3, 1) Windows BMP encoding this build's
//!   `Sfnt::glyph_for_char` prefers, with one segment per contiguous run of
//!   covered characters and the mandatory `0xFFFF` terminator. A character
//!   outside every segment reads back as glyph 0, which is `.notdef`, which is
//!   a `cmap` saying **no** — so a fixture face refuses characters rather than
//!   claiming the whole plane.
//! - **`hmtx` and `hhea`**, because an embedded face's advances and line height
//!   come from the file rather than from a table of guesses. `hhea`'s
//!   `numberOfHMetrics` is at offset 34 and its `ascender` at offset 4, and a
//!   fixture that got either wrong would agree with a reader that got the same
//!   one wrong.
//! - **`loca` and `glyf`**, so the program survives subsetting on the way into
//!   a PDF: `DocumentBuilder` subsets a composite font to the glyphs a document
//!   drew, and a face whose outlines are absent is one the subsetter drops.
//!
//! `name` and `maxp` are written because a font without them is not a font a
//! third party will read, and gap 31's oracle is qpdf.

/// One glyph's outline: a filled square, as one contour of four on-curve
/// points.
///
/// The shape is not the claim anywhere in this file — coverage is — but an
/// **empty** glyph is indistinguishable from a missing one to everything
/// downstream, so every covered character gets ink.
fn box_glyph(side: i16) -> Vec<u8> {
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes()); // one contour
    glyph.extend_from_slice(&0i16.to_be_bytes()); // xMin
    glyph.extend_from_slice(&0i16.to_be_bytes()); // yMin
    glyph.extend_from_slice(&side.to_be_bytes()); // xMax
    glyph.extend_from_slice(&side.to_be_bytes()); // yMax
    glyph.extend_from_slice(&3u16.to_be_bytes()); // last point of contour 0
    glyph.extend_from_slice(&0u16.to_be_bytes()); // no instructions
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on-curve, word deltas
    for dx in [0i16, side, 0, -side] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, side, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }
    glyph
}

/// How a synthesised face is described: what it covers and how wide it is.
///
/// A struct rather than four positional arguments, because two of the four are
/// `u16`s in the same units and a fixture that swapped them would still build
/// a font.
#[derive(Clone, Debug)]
pub struct Face {
    /// The family name written into the `name` table, `nameID` 1.
    pub family: String,
    /// Exactly the characters this face has a glyph for. Order does not
    /// matter; duplicates are ignored.
    pub covers: Vec<char>,
    /// Every covered glyph's advance, in font units.
    pub advance: u16,
    /// The em square, in font units.
    pub units_per_em: u16,
    /// `hhea`'s `ascender`, in font units.
    pub ascender: i16,
    /// `hhea`'s `descender`, in font units and **negative**, which is the
    /// sfnt's own sign convention.
    pub descender: i16,
}

impl Face {
    /// A face covering exactly `covers`, at 1000 units per em.
    #[must_use]
    pub fn new(family: &str, covers: &str) -> Face {
        Face {
            family: family.to_owned(),
            covers: covers.chars().collect(),
            advance: 500,
            units_per_em: 1000,
            ascender: 800,
            descender: -200,
        }
    }

    /// The same face at another advance, which is what makes two faces
    /// measurably different rather than merely differently named.
    #[must_use]
    pub fn with_advance(mut self, advance: u16) -> Face {
        self.advance = advance;
        self
    }

    /// The same face at another ascent and descent.
    #[must_use]
    pub fn with_vertical(mut self, ascender: i16, descender: i16) -> Face {
        self.ascender = ascender;
        self.descender = descender;
        self
    }

    /// The characters this face covers, sorted and deduplicated — which is the
    /// order glyph identifiers are assigned in, so a caller can predict them.
    #[must_use]
    pub fn glyph_order(&self) -> Vec<char> {
        let mut sorted: Vec<char> = self.covers.clone();
        sorted.sort_unstable();
        sorted.dedup();
        sorted
    }

    /// The glyph identifier this face's `cmap` will give a character.
    ///
    /// Glyph 0 is `.notdef` and the covered characters follow in sorted order,
    /// so a test can name the glyph it expects rather than reading it back out
    /// of the font it is testing.
    #[must_use]
    pub fn glyph_of(&self, ch: char) -> Option<u16> {
        let at = self.glyph_order().iter().position(|c| *c == ch)?;
        u16::try_from(at + 1).ok()
    }

    /// The program itself.
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        build(self)
    }
}

/// A face covering exactly the characters of `covers`, and nothing else.
#[must_use]
pub fn covering(family: &str, covers: &str) -> Vec<u8> {
    Face::new(family, covers).build()
}

fn build(face: &Face) -> Vec<u8> {
    let order = face.glyph_order();
    let glyph_count = order.len() + 1;

    // ---- glyf and loca ------------------------------------------------------
    //
    // Glyph 0 is `.notdef` and is **empty**: a glyph is empty precisely when
    // its `loca` entry equals the next one, which is what a reader draws as
    // nothing. Every covered character gets its own copy of the outline,
    // because `loca` gives each glyph its own slice and offsets that merely
    // repeat make every glyph empty.
    let outline = box_glyph(i16::try_from(face.units_per_em * 7 / 10).unwrap_or(700));
    let mut glyf = Vec::with_capacity(outline.len() * order.len());
    let mut loca = vec![0u32];
    for _ in &order {
        glyf.extend_from_slice(&outline);
        loca.push(u32::try_from(glyf.len()).unwrap_or(0));
    }
    let loca: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();

    // ---- head ---------------------------------------------------------------
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    head[18..20].copy_from_slice(&face.units_per_em.to_be_bytes());
    head[36..38].copy_from_slice(&0i16.to_be_bytes()); // xMin
    head[38..40].copy_from_slice(&0i16.to_be_bytes()); // yMin
    head[40..42].copy_from_slice(&(face.units_per_em as i16).to_be_bytes()); // xMax
    head[42..44].copy_from_slice(&(face.units_per_em as i16).to_be_bytes()); // yMax
    head[46..48].copy_from_slice(&8u16.to_be_bytes()); // lowestRecPPEM
    head[48..50].copy_from_slice(&2i16.to_be_bytes()); // fontDirectionHint
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    // ---- hhea ---------------------------------------------------------------
    //
    // The offsets are the specification's and are worth naming, because two of
    // them are read by code this repository owns: `ascender` at 4 and
    // `descender` at 6, **not** at 0 and 2, which is where the version is.
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    hhea[4..6].copy_from_slice(&face.ascender.to_be_bytes());
    hhea[6..8].copy_from_slice(&face.descender.to_be_bytes());
    hhea[8..10].copy_from_slice(&0i16.to_be_bytes()); // lineGap
    hhea[10..12].copy_from_slice(&face.advance.to_be_bytes()); // advanceWidthMax
    hhea[18..20].copy_from_slice(&1i16.to_be_bytes()); // caretSlopeRise
    hhea[34..36].copy_from_slice(&(glyph_count as u16).to_be_bytes()); // numberOfHMetrics

    // ---- hmtx ---------------------------------------------------------------
    //
    // A full entry per glyph, `.notdef` included: `numberOfHMetrics` equals the
    // glyph count, so nothing here depends on the trailing side-bearing form.
    let mut hmtx = Vec::with_capacity(glyph_count * 4);
    for _ in 0..glyph_count {
        hmtx.extend_from_slice(&face.advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // leftSideBearing
    }

    // ---- maxp ---------------------------------------------------------------
    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&(glyph_count as u16).to_be_bytes());
    maxp[6..8].copy_from_slice(&4u16.to_be_bytes()); // maxPoints
    maxp[8..10].copy_from_slice(&1u16.to_be_bytes()); // maxContours

    let cmap = cmap_format_4(&order);
    let name = name_table(&face.family);

    assemble(&[
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca),
        (b"maxp", &maxp),
        (b"name", &name),
    ])
}

/// A format 4 subtable mapping each character of `order` to its index plus one.
///
/// One segment per **contiguous run** of characters, which is what lets
/// `idDelta` alone carry the mapping: within a run the codes rise by one and so
/// do the glyphs, so `glyph = code + delta` holds for the whole segment and no
/// `idRangeOffset` array is needed. A face covering three scattered characters
/// gets three segments and a face covering a range gets one, and both read
/// back the same.
fn cmap_format_4(order: &[char]) -> Vec<u8> {
    // Only the BMP: a format 4 subtable has nowhere to put anything else, and
    // a fixture that silently dropped an astral character would look like a
    // face that does not cover it.
    let mut codes: Vec<(u16, u16)> = Vec::new();
    for (at, ch) in order.iter().enumerate() {
        let code = u32::from(*ch);
        assert!(
            code < 0xFFFF,
            "a fixture face may only cover the BMP; {ch:?} is not in it"
        );
        let glyph = u16::try_from(at + 1).expect("a fixture face has few glyphs");
        codes.push((code as u16, glyph));
    }

    let mut segments: Vec<(u16, u16, u16)> = Vec::new(); // start, end, first glyph
    for (code, glyph) in codes {
        match segments.last_mut() {
            Some(last) if last.1 + 1 == code && last.2 + (last.1 - last.0) + 1 == glyph => {
                last.1 = code;
            }
            _ => segments.push((code, code, glyph)),
        }
    }
    // 9.6.6.4's terminator: the last segment must end at 0xFFFF, and this one
    // maps it to glyph 0.
    segments.push((0xFFFF, 0xFFFF, 1));

    let count = segments.len();
    let seg2 = u16::try_from(count * 2).expect("a fixture face has few segments");
    let mut entry_selector = 0u16;
    while 1u32 << (entry_selector + 1) <= count as u32 {
        entry_selector += 1;
    }
    let search_range = 2u16 * (1 << entry_selector);

    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes()); // format
    sub.extend_from_slice(&0u16.to_be_bytes()); // length, filled in below
    sub.extend_from_slice(&0u16.to_be_bytes()); // language
    sub.extend_from_slice(&seg2.to_be_bytes());
    sub.extend_from_slice(&search_range.to_be_bytes());
    sub.extend_from_slice(&entry_selector.to_be_bytes());
    sub.extend_from_slice(&(seg2 - search_range).to_be_bytes()); // rangeShift
    for (_, end, _) in &segments {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for (start, _, _) in &segments {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for (start, _, glyph) in &segments {
        sub.extend_from_slice(&glyph.wrapping_sub(*start).to_be_bytes()); // idDelta
    }
    for _ in &segments {
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }
    let length = u16::try_from(sub.len()).expect("a fixture subtable is small");
    sub[2..4].copy_from_slice(&length.to_be_bytes());

    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // version
    table.extend_from_slice(&1u16.to_be_bytes()); // numTables
    table.extend_from_slice(&3u16.to_be_bytes()); // platformID: Windows
    table.extend_from_slice(&1u16.to_be_bytes()); // encodingID: Unicode BMP
    table.extend_from_slice(&12u32.to_be_bytes()); // offset
    table.extend_from_slice(&sub);
    table
}

/// A `name` table carrying one record: `nameID` 1, the family, in UTF-16BE.
fn name_table(family: &str) -> Vec<u8> {
    let text: Vec<u8> = family
        .encode_utf16()
        .flat_map(|unit| unit.to_be_bytes())
        .collect();
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // format 0
    table.extend_from_slice(&1u16.to_be_bytes()); // count
    table.extend_from_slice(&18u16.to_be_bytes()); // stringOffset: 6 + 1 * 12
    table.extend_from_slice(&3u16.to_be_bytes()); // platformID: Windows
    table.extend_from_slice(&1u16.to_be_bytes()); // encodingID: UCS-2
    table.extend_from_slice(&0x0409u16.to_be_bytes()); // languageID
    table.extend_from_slice(&1u16.to_be_bytes()); // nameID: family
    table.extend_from_slice(&(text.len() as u16).to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes()); // offset into the storage
    table.extend_from_slice(&text);
    table
}

/// The table directory and the tables, each padded to a four-byte boundary.
///
/// The tags are written in the order the caller gave them, which every caller
/// here gives alphabetically — the order a real sfnt uses and the one a reader
/// that binary-searched the directory would need.
fn assemble(tables: &[(&[u8; 4], &Vec<u8>)]) -> Vec<u8> {
    let count = tables.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // sfntVersion
    out.extend_from_slice(&(count as u16).to_be_bytes());
    let mut entry_selector = 0u16;
    while 1u32 << (entry_selector + 1) <= count as u32 {
        entry_selector += 1;
    }
    let search_range = 16u16 * (1 << entry_selector);
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&((count as u16 * 16).wrapping_sub(search_range)).to_be_bytes());

    let mut offset = 12 + count * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + count * 16 + body.len();
    }
    out.extend_from_slice(&body);
    out
}

/// A table's checksum: the sum of its big-endian 32-bit words, wrapping.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut chunk = [0u8; 4];
    for (at, byte) in data.iter().enumerate() {
        chunk[at % 4] = *byte;
        if at % 4 == 3 {
            sum = sum.wrapping_add(u32::from_be_bytes(chunk));
            chunk = [0; 4];
        }
    }
    if data.len() % 4 != 0 {
        for slot in chunk.iter_mut().skip(data.len() % 4) {
            *slot = 0;
        }
        sum = sum.wrapping_add(u32::from_be_bytes(chunk));
    }
    sum
}

/// A TrueType face whose every glyph from 32 upward is one filled box, reached
/// **by glyph identifier** and not through a `cmap`.
///
/// `tests/substitute_fonts.rs` records why it is synthesised rather than read
/// from the system. It is here rather than copied into each test binary that
/// wants it, and it is deliberately **not** what [`covering`] builds: this face
/// answers *"did a glyph get drawn"* for a [`tinker_pdf::FontProvider`], which
/// reaches a glyph by the code a document wrote and asks no `cmap` anything, so
/// giving it one would test nothing and change what four existing tests mean.
#[must_use]
pub fn boxy_font() -> Vec<u8> {
    let glyph = box_glyph(700);

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    const FIRST: usize = 32;
    const LAST: usize = 255;
    let size = glyph.len() as u32;

    let mut glyf = Vec::with_capacity(glyph.len() * (LAST + 1 - FIRST));
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }
    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        let offset = (index.saturating_sub(FIRST)) as u32 * size;
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// Every `BT … ET` text object of a content stream, in the order they were
/// written, as `(resource, operators)`.
///
/// A parse rather than a substring search, because the count is the assertion:
/// `a_run_needing_three_faces_becomes_three_text_objects` fails if a build
/// draws one object or five, and a `contains` over three resource names passes
/// on both.
#[must_use]
pub fn text_objects(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(at) = rest.find("BT /") {
        let body = &rest[at + 4..];
        let Some(end) = body.find(" ET") else { break };
        let object = &body[..end];
        let resource = object
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        out.push((resource, object.to_owned()));
        rest = &body[end + 3..];
    }
    out
}

/// The `x` and `y` of one text object's `Td`, which is where it starts.
///
/// `Td` is the only positioning operator this build writes into a text object,
/// so the pair before it is the origin.
#[must_use]
pub fn origin_of(object: &str) -> (f64, f64) {
    let words: Vec<&str> = object.split_whitespace().collect();
    let at = words
        .iter()
        .position(|word| *word == "Td")
        .unwrap_or_else(|| panic!("no Td in {object:?}"));
    let x = words[at - 2].parse().expect("an x before Td");
    let y = words[at - 1].parse().expect("a y before Td");
    (x, y)
}
