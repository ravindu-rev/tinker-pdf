//! The faces and pages the embedding pass is held with
//! (`docs/design/font-embedding.md`): `embed_missing.rs` asks every question
//! of them, and `determinism.rs` hashes what one rewrite of them writes.
//!
//! Every face is assembled here byte by byte, as `substitute_fonts.rs`'s
//! are, so the tests are the same on every target — `wasm32-wasip1` reads no
//! file — and the repository carries no face anybody has to licence. Each
//! glyph is a box of its own size, so a page drawn with a wrong glyph is a
//! page with other pixels, which is what a fingerprint can see.

#![allow(
    dead_code,
    reason = "shared by two test binaries; each uses a different subset"
)]

use std::sync::Arc;

use tinker_pdf::{Bitmap, DocumentBuilder, FontProvider, SimpleFontProvider};

/// One glyph of a test face: the character its `cmap` reaches it by, its
/// advance in font units and the box it draws from the origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// The character.
    pub ch: char,
    /// Its `hmtx` advance, or its charstring width.
    pub advance: u16,
    /// The box's width.
    pub width: i16,
    /// The box's height.
    pub height: i16,
}

/// A test face, glyph `n + 1` drawing `glyphs[n]` and glyph 0 `.notdef`,
/// which draws nothing.
#[derive(Clone, Debug)]
pub struct Face {
    /// The PostScript name its `name` table, or its Name INDEX, states.
    pub name: &'static str,
    /// Its glyphs, in glyph order from 1.
    pub glyphs: Vec<Glyph>,
    /// `head`'s units per em.
    pub units_per_em: u16,
    /// `OS/2`'s `fsType`.
    pub fs_type: u16,
    /// `OS/2`'s `sFamilyClass`.
    pub family_class: u16,
    /// Whether the face has a `cmap` at all; one with none has nothing to be
    /// read by.
    pub mapped: bool,
    /// The glyph names a CFF charset gives the glyphs of these characters in
    /// place of their standard names, each a string of the String INDEX.
    pub names: Vec<(char, &'static str)>,
}

impl Face {
    /// A face covering `chars`, each glyph a box of its own size and every
    /// advance `advance`, at 1000 units per em.
    pub fn covering(name: &'static str, chars: &str, advance: u16) -> Face {
        let glyphs = chars
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                let i = index as i16 + 1;
                Glyph {
                    ch,
                    advance,
                    // Distinct for every glyph a test face carries: `i * 37`
                    // is one-to-one modulo 400 below 400.
                    width: 100 + (i * 37) % 400,
                    height: 250 + (i * 53) % 500,
                }
            })
            .collect();
        Face {
            name,
            glyphs,
            units_per_em: 1000,
            fs_type: 0,
            family_class: 0x0801,
            mapped: true,
            names: Vec::new(),
        }
    }

    /// The same face with its CFF charset naming `ch`'s glyph `name`.
    #[must_use]
    pub fn named(mut self, ch: char, name: &'static str) -> Face {
        self.names.push((ch, name));
        self
    }

    /// The String INDEX's strings and the SID each glyph is named by, in
    /// glyph order from 1: a name [`Face::named`] gave it, at 391 on as
    /// Appendix A numbers a font's own strings, else its character's
    /// standard name.
    fn charset_names(&self) -> (Vec<Vec<u8>>, Vec<u16>) {
        let mut strings = Vec::new();
        let mut sids = Vec::with_capacity(self.glyphs.len());
        for glyph in &self.glyphs {
            match self.names.iter().find(|(ch, _)| *ch == glyph.ch) {
                Some((_, name)) => {
                    sids.push(391 + strings.len() as u16);
                    strings.push(name.as_bytes().to_vec());
                }
                None => sids.push(standard_sid(glyph.ch)),
            }
        }
        (strings, sids)
    }

    /// The same face with `ch`'s advance `advance`.
    #[must_use]
    pub fn with_advance(mut self, ch: char, advance: u16) -> Face {
        for glyph in &mut self.glyphs {
            if glyph.ch == ch {
                glyph.advance = advance;
            }
        }
        self
    }

    /// The same face with `fsType` `bits`.
    #[must_use]
    pub fn with_fs_type(mut self, bits: u16) -> Face {
        self.fs_type = bits;
        self
    }

    /// The same face with no `cmap`.
    #[must_use]
    pub fn unmapped(mut self) -> Face {
        self.mapped = false;
        self
    }

    /// The same face at `units` units per em, its boxes and advances as they
    /// are in font units.
    #[must_use]
    pub fn at_units(mut self, units: u16) -> Face {
        self.units_per_em = units;
        self
    }

    /// The glyph `ch` is.
    pub fn glyph_of(&self, ch: char) -> Option<u16> {
        self.glyphs
            .iter()
            .position(|glyph| glyph.ch == ch)
            .map(|index| index as u16 + 1)
    }

    fn widest(&self) -> i16 {
        self.glyphs.iter().map(|g| g.width).max().unwrap_or(0)
    }

    fn tallest(&self) -> i16 {
        self.glyphs.iter().map(|g| g.height).max().unwrap_or(0)
    }

    /// The face as a TrueType program: `cmap` (3,1) where it is mapped,
    /// `glyf`, `head`, `hhea`, `hmtx`, `loca`, `maxp`, `name`, `OS/2` and
    /// `post`.
    pub fn truetype(&self) -> Vec<u8> {
        let mut glyf = Vec::new();
        let mut loca = vec![0u32, 0];
        for glyph in &self.glyphs {
            glyf.extend(box_glyph(glyph.width, glyph.height));
            loca.push(glyf.len() as u32);
        }
        let loca: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();
        let mut tables = self.metrics_tables(1);
        tables.push((*b"glyf", glyf));
        tables.push((*b"loca", loca));
        sfnt(0x0001_0000, tables)
    }

    /// The face as a bare CFF program: its glyphs named by the standard
    /// strings their characters' glyph names are, or the names
    /// [`Face::named`] gave them, each charstring stating its width over a
    /// `nominalWidthX` of zero.
    pub fn cff(&self) -> Vec<u8> {
        let header = [1u8, 0, 4, 4];
        let names = index(&[self.name.as_bytes().to_vec()]);
        let (own, sids) = self.charset_names();
        let strings = index(&own);
        let globals = index(&[]);
        let mut charset = vec![0u8];
        let mut charstrings = vec![vec![14u8]];
        for (glyph, sid) in self.glyphs.iter().zip(sids) {
            charset.extend_from_slice(&sid.to_be_bytes());
            charstrings.push(box_charstring(glyph));
        }
        let charstrings = index(&charstrings);
        let mut private = dict_entry(20, &[0]);
        private.extend(dict_entry(21, &[0]));
        let (wide, tall) = (i32::from(self.widest()), i32::from(self.tallest()));
        let top = |charset_at: i32, charstrings_at: i32, private_at: i32| {
            let mut out = dict_entry(5, &[0, 0, wide, tall]);
            out.extend(dict_entry(15, &[charset_at]));
            out.extend(dict_entry(17, &[charstrings_at]));
            out.extend(dict_entry(18, &[private.len() as i32, private_at]));
            out
        };
        let top_len = index(&[top(0, 0, 0)]).len();
        let charset_at = header.len() + names.len() + top_len + strings.len() + globals.len();
        let charstrings_at = charset_at + charset.len();
        let private_at = charstrings_at + charstrings.len();
        let tops = index(&[top(
            charset_at as i32,
            charstrings_at as i32,
            private_at as i32,
        )]);
        let mut out = header.to_vec();
        for part in [
            &names,
            &tops,
            &strings,
            &globals,
            &charset,
            &charstrings,
            &private,
        ] {
            out.extend_from_slice(part);
        }
        out
    }

    /// The face as an `OpenType/CFF` program: [`Face::cff`] in an `OTTO`
    /// directory beside the tables an sfnt states its metrics in.
    pub fn opentype_cff(&self) -> Vec<u8> {
        let mut tables = self.metrics_tables(0);
        tables.push((*b"CFF ", self.cff()));
        sfnt(0x4F54_544F, tables)
    }

    /// The tables every form of the face shares.
    fn metrics_tables(&self, long_loca: u16) -> Vec<([u8; 4], Vec<u8>)> {
        let count = self.glyphs.len() as u16 + 1;

        let mut head = vec![0u8; 54];
        put32(&mut head, 0, 0x0001_0000);
        put32(&mut head, 12, 0x5F0F_3CF5);
        put16(&mut head, 18, self.units_per_em);
        put16(&mut head, 40, self.widest() as u16);
        put16(&mut head, 42, self.tallest() as u16);
        put16(&mut head, 50, long_loca);

        let mut hhea = vec![0u8; 36];
        put32(&mut hhea, 0, 0x0001_0000);
        put16(&mut hhea, 4, 800);
        put16(&mut hhea, 6, (-200i16) as u16);
        put16(
            &mut hhea,
            10,
            self.glyphs.iter().map(|g| g.advance).max().unwrap_or(500),
        );
        put16(&mut hhea, 34, count);

        let mut hmtx = Vec::new();
        hmtx.extend_from_slice(&500u16.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
        for glyph in &self.glyphs {
            hmtx.extend_from_slice(&glyph.advance.to_be_bytes());
            hmtx.extend_from_slice(&0i16.to_be_bytes());
        }

        let mut maxp = vec![0u8; 6];
        put32(&mut maxp, 0, 0x0000_5000);
        put16(&mut maxp, 4, count);

        let mut os2 = vec![0u8; 96];
        put16(&mut os2, 0, 4);
        put16(&mut os2, 4, 400);
        put16(&mut os2, 6, 5);
        put16(&mut os2, 8, self.fs_type);
        put16(&mut os2, 30, self.family_class);
        put16(&mut os2, 62, 0x0040);
        put16(&mut os2, 68, 800);
        put16(&mut os2, 70, (-200i16) as u16);
        put16(&mut os2, 86, 500);
        put16(&mut os2, 88, 700);

        let mut post = vec![0u8; 32];
        put32(&mut post, 0, 0x0003_0000);

        let name = name_table(self.name);

        let mut tables = vec![
            (*b"OS/2", os2),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"maxp", maxp),
            (*b"name", name),
            (*b"post", post),
        ];
        if self.mapped {
            let mut pairs: Vec<(u16, u16)> = self
                .glyphs
                .iter()
                .enumerate()
                .filter_map(|(index, glyph)| {
                    Some((u16::try_from(u32::from(glyph.ch)).ok()?, index as u16 + 1))
                })
                .collect();
            pairs.sort_unstable();
            tables.push((*b"cmap", unicode_cmap(&pairs)));
        }
        tables
    }
}

/// The StandardEncoding glyph name's SID of a letter or the space (Adobe
/// Technical Note 5176, Appendix A): what a CFF charset names it by.
fn standard_sid(ch: char) -> u16 {
    match ch {
        ' ' => 1,
        'A'..='Z' => 34 + (ch as u16 - 'A' as u16),
        'a'..='z' => 66 + (ch as u16 - 'a' as u16),
        _ => 0,
    }
}

fn put16(table: &mut [u8], at: usize, value: u16) {
    table[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn put32(table: &mut [u8], at: usize, value: u32) {
    table[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// A filled box `width` by `height` from the origin, one contour of four
/// on-curve points (`glyf`).
fn box_glyph(width: i16, height: i16) -> Vec<u8> {
    let mut glyph = Vec::new();
    for value in [1i16, 0, 0, width, height] {
        glyph.extend_from_slice(&value.to_be_bytes());
    }
    glyph.extend_from_slice(&3u16.to_be_bytes());
    glyph.extend_from_slice(&0u16.to_be_bytes());
    glyph.extend_from_slice(&[0x01; 4]);
    for dx in [0i16, width, 0, -width] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, height, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }
    while glyph.len() % 4 != 0 {
        glyph.push(0);
    }
    glyph
}

/// A Type 2 operand in its narrowest form.
fn t2(value: i32) -> Vec<u8> {
    match value {
        -107..=107 => vec![(value + 139) as u8],
        108..=1131 => {
            let v = value - 108;
            vec![(247 + (v >> 8)) as u8, (v & 0xFF) as u8]
        }
        -1131..=-108 => {
            let v = -value - 108;
            vec![(251 + (v >> 8)) as u8, (v & 0xFF) as u8]
        }
        _ => {
            let mut out = vec![28];
            out.extend_from_slice(&(value as i16).to_be_bytes());
            out
        }
    }
}

/// A charstring stating `glyph`'s width and drawing its box: the width, then
/// `0 0 rmoveto`, three `rlineto`s and `endchar`.
fn box_charstring(glyph: &Glyph) -> Vec<u8> {
    let mut out = t2(i32::from(glyph.advance));
    out.extend(t2(0));
    out.extend(t2(0));
    out.push(21);
    let (w, h) = (i32::from(glyph.width), i32::from(glyph.height));
    for (dx, dy) in [(w, 0), (0, h), (-w, 0)] {
        out.extend(t2(dx));
        out.extend(t2(dy));
        out.push(5);
    }
    out.push(14);
    out
}

/// Wraps `items` as a CFF INDEX with four-byte offsets.
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(4);
    let mut offset = 1u32;
    out.extend_from_slice(&offset.to_be_bytes());
    for item in items {
        offset += item.len() as u32;
        out.extend_from_slice(&offset.to_be_bytes());
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// One DICT entry, its operands in the fixed five-byte form.
fn dict_entry(op: u16, operands: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in operands {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }
    if op > 0xFF {
        out.extend_from_slice(&[12, (op & 0xFF) as u8]);
    } else {
        out.push(op as u8);
    }
    out
}

/// A `cmap` of one (3,1) format 4 subtable mapping each code point to its
/// glyph, a segment each, and the terminating `0xFFFF` segment.
fn unicode_cmap(pairs: &[(u16, u16)]) -> Vec<u8> {
    let segments = pairs.len() + 1;
    let mut select = 0u16;
    while (1usize << (select + 1)) <= segments {
        select += 1;
    }
    let search = 2 * (1u16 << select);
    let mut sub = Vec::new();
    let x2 = (segments * 2) as u16;
    for value in [4u16, 0, 0, x2, search, select, x2 - search] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for (code, _) in pairs {
        sub.extend_from_slice(&code.to_be_bytes());
    }
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    for (code, _) in pairs {
        sub.extend_from_slice(&code.to_be_bytes());
    }
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    for (code, glyph) in pairs {
        sub.extend_from_slice(&glyph.wrapping_sub(*code).to_be_bytes());
    }
    sub.extend_from_slice(&1u16.to_be_bytes());
    sub.extend(std::iter::repeat_n(0u8, segments * 2));
    let length = sub.len() as u16;
    put16(&mut sub, 2, length);

    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend(sub);
    cmap
}

/// A `name` table with one record, ID 6, the PostScript name.
fn name_table(name: &str) -> Vec<u8> {
    let text: Vec<u8> = name.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut out = Vec::new();
    for value in [0u16, 1, 18, 3, 1, 0x0409, 6, text.len() as u16, 0] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend(text);
    out
}

/// A table directory over `tables`, sorted by tag as the format asks, each
/// table padded to four bytes; no checksums.
fn sfnt(version: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = version.to_be_bytes().to_vec();
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut padded = data.clone();
        while padded.len() % 4 != 0 {
            padded.push(0);
        }
        offset += padded.len();
        body.extend(padded);
    }
    out.extend(body);
    out
}

/// The face every page here is drawn with unless a test says otherwise: the
/// letters and the space, 600 units each.
pub fn sans() -> Face {
    Face::covering(
        "TinkerSans-Regular",
        " ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        600,
    )
}

/// A host provider with one face, answering every request with it.
pub fn provider(face: Vec<u8>) -> Arc<dyn FontProvider> {
    Arc::new(SimpleFontProvider::new(face).substituting_symbolic())
}

/// A stream object: `entries`, its `/Length`, then `data`.
pub fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// An object written as `text`.
pub fn object(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// A document of `objects`, numbered from one, the first its catalog, with a
/// cross-reference table that states where each is.
pub fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102030405060708090A0B0C0D0E0F10> \
             <0102030405060708090A0B0C0D0E0F10>] >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A page showing `text` at 24 points in `/F0`, the builder's Helvetica,
/// which embeds nothing and states no widths: the standard 14 as most
/// documents name them.
pub fn standard_page(text: &str) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(320.0, 80.0, |page| {
        page.text(b"F0", 24.0, 12.0, 30.0, text);
    });
    builder.finish()
}

/// A page showing `content` with `/F0`, object 4, the font `font` writes
/// inside `<< >>`, its descriptor — where `descriptor` is not empty — object
/// 5, and every further object `extra` gives from 6 on. Object 1 is the
/// catalog, 2 the page tree, 3 the page, and the content stream the last.
pub fn font_page(font: &str, descriptor: &str, extra: &[Vec<u8>], content: &str) -> Vec<u8> {
    let content_object = 6 + extra.len();
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 80] \
             /Resources << /Font << /F0 4 0 R >> >> /Contents {content_object} 0 R >>"
        )),
        object(&format!("<< {font} >>")),
        if descriptor.is_empty() {
            object("null")
        } else {
            object(&format!("<< {descriptor} >>"))
        },
    ];
    objects.extend(extra.iter().cloned());
    objects.push(stream("", content.as_bytes()));
    pdf(&objects)
}

/// The `/Widths` of `face` for codes `first` to `last`, read through
/// `WinAnsiEncoding`'s ASCII, in thousandths of an em: what an unembedded
/// TrueType font metric-compatible with the face states.
pub fn widths_of(face: &Face, first: u8, last: u8) -> String {
    let units = f64::from(face.units_per_em);
    let widths: Vec<String> = (first..=last)
        .map(|code| {
            let advance = face
                .glyphs
                .iter()
                .find(|glyph| u32::from(glyph.ch) == u32::from(code))
                .map_or(0.0, |glyph| f64::from(glyph.advance) * 1000.0 / units);
            format!("{advance}")
        })
        .collect();
    format!(
        "/FirstChar {first} /LastChar {last} /Widths [{}]",
        widths.join(" ")
    )
}

/// A page showing `text` with an unembedded, non-symbolic TrueType font,
/// `/Arial` under `WinAnsiEncoding`, its widths `face`'s and its descriptor
/// naming no program: the shape an office suite writes when it does not
/// embed.
pub fn truetype_page(face: &Face, text: &str) -> Vec<u8> {
    font_page(
        &format!(
            "/Type /Font /Subtype /TrueType /BaseFont /Arial /Encoding /WinAnsiEncoding \
             {} /FontDescriptor 5 0 R",
            widths_of(face, 32, 122)
        ),
        "/Type /FontDescriptor /FontName /Arial /Flags 32 /FontBBox [0 -200 1000 800] \
         /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80",
        &[],
        &format!("BT /F0 24 Tf 12 30 Td ({text}) Tj ET"),
    )
}

/// The rendered bytes of a page, with its dimensions, hashed: two renders
/// are the same picture exactly when these are equal.
pub fn fingerprint(bitmap: &Bitmap) -> String {
    let mut input = Vec::with_capacity(bitmap.data.len() + 8);
    input.extend_from_slice(&bitmap.width.to_be_bytes());
    input.extend_from_slice(&bitmap.height.to_be_bytes());
    input.extend_from_slice(&bitmap.data);
    tinker_pdf_crypto::sha2::sha256(&input)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The pixels a page painted, away from white.
pub fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel[0] < 250)
        .count()
}
