//! Font parts: 9.1.7's `FontUri`, and 9.1.7.3's thirty-two XORs (gap 30,
//! milestone 7).
//!
//! # The de-obfuscation is a permutation, and the permutation is the whole of
//! it
//!
//! ECMA-388 9.1.7.3 [M2.53] takes the last segment of the part name, drops the
//! extension, reads the remaining characters as a GUID, and XORs the **first
//! thirty-two bytes** of the part with the GUID's sixteen bytes twice over, in
//! the order *B37, B36, B35, B34, B33, B32, B31, B30, B20, B21, B10, B11, B00,
//! B01, B02, B03* — where the segment is written
//! `B03B02B01B00-B11B10-B21B20-B30B31-B32B33B34B35B36B37`.
//!
//! Read against that spelling the permutation is **exactly the sixteen bytes
//! of the hex string reversed**, and milestone 1 recorded what happens to
//! somebody who transcribes the B-names instead: two pairs come out
//! transposed, the sfnt version and the table tags are still right, and
//! `searchRange`, `entrySelector` and `rangeShift` are garbage. That is a font
//! that parses far enough to draw *something*, which is why gap 30's row 7
//! asks for the de-obfuscated **bytes** to be asserted rather than a page that
//! drew.
//!
//! # The content type selects the path, and the extension does not
//!
//! 9.1.7.3 says the extension MAY be arbitrary and only *recommends*
//! `.odttf`. What decides is Table D-4's
//! `application/vnd.ms-package.obfuscated-opentype`, resolved through OPC
//! 7.2.3.5 — which is case-insensitive, and milestone 1 measured a real
//! `<Default Extension="ODTTF">` against a part named `….ODTTF` in both
//! dialects. Two rules with one clause between them, so both directions are
//! tested: a lying extension does not select the path, and a lying extension
//! does not keep it from being selected either.
//!
//! # No cap, and here is the argument
//!
//! Thirty-two XORs over a buffer `tinker-pdf-zip` has already bounded by
//! `MAX_ZIP_ENTRY_BYTES`, allocating one copy of a part that is already in
//! memory. A constant over it could never be the thing that stopped an
//! attacker, which is gap 18a milestone 8's failure reached from the other
//! direction — the same argument gap 30's bounds section makes for it in
//! advance.

use std::collections::HashMap;
use std::rc::Rc;

use tinker_pdf_cos::DocumentBuilder;
use tinker_pdf_font::Sfnt;
use tinker_pdf_xml::{Event, Source};

use super::markup::Trouble;
use super::opc::{Package, PartName};
use super::{dialect_of, Limits, XpsElementDefect};

/// Table D-4's media type for an obfuscated font part.
///
/// Compared ASCII case-insensitively, per OPC 7.2.3.5, because that is how a
/// media type is compared and not because any producer has been seen to vary
/// it.
pub const OBFUSCATED_FONT_MEDIA_TYPE: &str = "application/vnd.ms-package.obfuscated-opentype";

/// How many bytes 9.1.7.3 obfuscates: the first thirty-two, and no others.
const OBFUSCATED_PREFIX: usize = 32;

/// 9.1.7.3's key: the GUID in a part name's last segment, as sixteen bytes in
/// the order the clause's permutation puts them.
///
/// `None` when the segment is not a GUID — thirty-two hex digits, with or
/// without the four hyphens the canonical spelling carries. A part typed as
/// obfuscated whose name carries no key cannot be de-obfuscated at all, and
/// that is reported by its own name rather than as a font that would not
/// parse.
#[must_use]
pub fn obfuscation_key(part: &PartName) -> Option<[u8; 16]> {
    let segment = part.as_str().rsplit('/').next()?;
    // 9.1.7.3: the extension is removed, and it is the *last* dot that starts
    // it — a segment carrying no dot at all is still a GUID.
    let stem = match segment.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => segment,
    };
    let digits: String = stem.chars().filter(|c| *c != '-').collect();
    if digits.len() != 32 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    // The hyphens are not decoration: `595c31af-dbe8-48a5-a032-c677a052f501`
    // has them in four places and nowhere else, and a segment with hyphens
    // somewhere else is not the spelling the clause reads.
    if stem.len() != 32 && stem.len() != 36 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (at, slot) in bytes.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&digits[at * 2..at * 2 + 2], 16).ok()?;
    }
    // The permutation, as one line: the sixteen bytes, reversed.
    bytes.reverse();
    Some(bytes)
}

/// 9.1.7.3's de-obfuscation, applied to a copy of the part's bytes.
///
/// `None` when the part name carries no GUID. A part shorter than thirty-two
/// bytes is **not** an error: the clause XORs the first thirty-two bytes and a
/// shorter part has fewer, which the font parser will then refuse on its own
/// terms rather than this function inventing a length rule.
#[must_use]
pub fn deobfuscate(part: &PartName, bytes: &[u8]) -> Option<Vec<u8>> {
    let key = obfuscation_key(part)?;
    let mut out = bytes.to_vec();
    for (at, byte) in out.iter_mut().take(OBFUSCATED_PREFIX).enumerate() {
        *byte ^= key[at % key.len()];
    }
    Some(out)
}

/// 9.1.7's `FontUri`, split into the part reference and the face selector.
///
/// The fragment is a face index into a TrueType collection. It is stripped
/// before the reference is resolved — `opc::resolve_reference` does the same,
/// and both have to, because `#` is not part of a part name in either place.
#[must_use]
pub fn font_uri(uri: &str) -> (&str, Option<&str>) {
    match uri.split_once('#') {
        Some((reference, fragment)) => (reference, Some(fragment)),
        None => (uri, None),
    }
}

/// One font part, loaded, de-obfuscated where it had to be, and registered
/// with the writer as a composite font.
#[derive(Clone, Debug)]
pub struct Font {
    /// The PDF resource name `PageBuilder`-less content refers to it by.
    pub resource: Vec<u8>,
    /// The font program, after de-obfuscation.
    ///
    /// Shared rather than copied: one part may be named by every page of a
    /// four-thousand-page document, and milestone 1 measured a real one at
    /// 189 252 bytes for eight characters of text.
    pub program: Rc<Vec<u8>>,
    /// `head`'s units per em, which every advance and offset in 12.1.3 is
    /// stated as a fraction of.
    pub units_per_em: f64,
}

impl Font {
    /// The font program's table directory.
    ///
    /// Parsed on demand rather than held, because [`Sfnt`] borrows the bytes
    /// and a struct holding both would be self-referential — and parsed **once
    /// per run** by the caller rather than once per glyph, which is the
    /// difference between reading a table directory eight times and reading it
    /// two million.
    #[must_use]
    pub fn sfnt(&self) -> Option<Sfnt<'_>> {
        Sfnt::parse(&self.program)
    }
}

/// Every font a synthesised document has loaded, keyed by the part and the
/// face a `FontUri` named.
///
/// One per document, because a PDF resource name has to be unique across one:
/// `DocumentBuilder::add_page` copies the document's whole resource table into
/// every page, so two pages naming one `/XF0` for two different faces would be
/// one face.
#[derive(Default)]
pub struct Fonts {
    loaded: HashMap<(PartName, u32), Result<Font, XpsElementDefect>>,
    next: usize,
}

impl Fonts {
    /// Loads every font the `Glyphs` runs of one fixed page part name.
    ///
    /// # Why a pass over the markup rather than a lookup during the drawing
    ///
    /// [`Package::read_part`] hands back a borrow of the package, and the
    /// painter is already holding one for the markup it is walking — so a font
    /// resolved *while* drawing would need the page's bytes copied out first,
    /// and a fixed page part is the one part of this format whose size the file
    /// chooses. A second walk costs time proportional to the part and **no**
    /// memory, which is the trade gap 30's own segment cap is set by.
    ///
    /// Once per part, not once per reference: the caller only reaches here for
    /// a part it has not painted yet, and the cache below is keyed by the part
    /// a URI resolves to, so a page naming one font twice loads it once.
    ///
    /// # Errors
    /// [`Trouble::Exhausted`] when one page names more distinct fonts than the
    /// package could hold parts — refusing the package, because a page that
    /// names eight thousand fonts is not a page whose fonts eight thousand
    /// lookups would find.
    pub fn load(
        &mut self,
        package: &mut Package<'_>,
        page: &PartName,
        builder: &mut DocumentBuilder,
        limits: &Limits,
    ) -> Result<(), Trouble> {
        let wanted = match package.read_part(page) {
            Ok(bytes) => font_references(bytes, page, limits)?,
            Err(_) => return Ok(()),
        };
        if wanted.len() > limits.max_parts {
            return Err(Trouble::Exhausted);
        }
        for (name, face) in wanted {
            if self.loaded.contains_key(&(name.clone(), face)) {
                continue;
            }
            let loaded = self.load_one(package, &name, face, builder);
            self.loaded.insert((name, face), loaded);
        }
        Ok(())
    }

    /// The font a `FontUri` on `page` names.
    ///
    /// Pure: the resolution is arithmetic over two strings and the lookup is
    /// the table [`Fonts::load`] filled, so the drawing walk never needs the
    /// package.
    ///
    /// # Errors
    /// One [`XpsElementDefect`] per way a `FontUri` can fail to be a font, each
    /// by its own name.
    pub fn get(&self, page: &PartName, uri: &str) -> Result<&Font, XpsElementDefect> {
        let (reference, fragment) = font_uri(uri);
        let face = face_index(fragment).ok_or(XpsElementDefect::GlyphsFontFace)?;
        let name = page
            .resolve(reference)
            .ok_or(XpsElementDefect::GlyphsFontUnresolved)?;
        match self.loaded.get(&(name, face)) {
            Some(Ok(font)) => Ok(font),
            Some(Err(defect)) => Err(*defect),
            None => Err(XpsElementDefect::GlyphsFontUnresolved),
        }
    }

    fn load_one(
        &mut self,
        package: &mut Package<'_>,
        name: &PartName,
        face: u32,
        builder: &mut DocumentBuilder,
    ) -> Result<Font, XpsElementDefect> {
        // 9.1.7's face selector, before the part is read: a reference to the
        // second face of a collection is refused **by name** rather than
        // silently drawn in the first, which is what `Sfnt::parse` would do
        // because a PDF embedding a collection has no way to say otherwise.
        if face != 0 {
            return Err(XpsElementDefect::GlyphsFontFace);
        }
        if !package.has(name) {
            return Err(XpsElementDefect::GlyphsFontUnresolved);
        }
        // Read before the bytes, and copied, because both borrow the package.
        let obfuscated = match package.media_type(name) {
            Ok(Some(media)) => media.eq_ignore_ascii_case(OBFUSCATED_FONT_MEDIA_TYPE),
            // A part with no media type at all is not one 7.2.3.5 resolved,
            // and nothing may be assumed about it from its name.
            _ => false,
        };
        let program = match package.read_part(name) {
            Ok(bytes) if obfuscated => {
                deobfuscate(name, bytes).ok_or(XpsElementDefect::GlyphsFontObfuscation)?
            }
            Ok(bytes) => bytes.to_vec(),
            Err(_) => return Err(XpsElementDefect::GlyphsFontUnresolved),
        };

        let units_per_em = f64::from(
            Sfnt::parse(&program)
                .ok_or(XpsElementDefect::GlyphsFontUnreadable)?
                .units_per_em
                .max(1),
        );
        let resource = format!("XF{}", self.next).into_bytes();
        // The writer parses the program a second time and refuses what it
        // cannot read; a font that got past `Sfnt::parse` here and not there
        // would be a resource name no page could use.
        if !builder.add_cid_font(&resource, b"XpsFont", &program) {
            return Err(XpsElementDefect::GlyphsFontUnreadable);
        }
        self.next += 1;
        Ok(Font {
            resource,
            program: Rc::new(program),
            units_per_em,
        })
    }
}

/// 9.1.7's face selector: `None` for no fragment, `Some(n)` for `#n`.
///
/// A fragment that is not a decimal index is not a face and answers `None`,
/// which the caller turns into the same refusal a face other than the first
/// gets — both are a `FontUri` naming a face this build will not draw in.
fn face_index(fragment: Option<&str>) -> Option<u32> {
    match fragment {
        None => Some(0),
        Some(text) => text.parse::<u32>().ok(),
    }
}

/// Every `(part, face)` a fixed page part's `Glyphs` runs name, deduplicated.
///
/// A `FontUri` that resolves to no part name at all is dropped here rather
/// than recorded: [`Fonts::get`] resolves the same reference the same way and
/// answers `GlyphsFontUnresolved` for it, so the table only holds what a
/// lookup could find.
fn font_references(
    bytes: &[u8],
    page: &PartName,
    limits: &Limits,
) -> Result<Vec<(PartName, u32)>, Trouble> {
    let Ok(source) = Source::new(bytes) else {
        return Ok(Vec::new());
    };
    // A `Vec` for the order and a set for the membership, and the order is
    // load-bearing: it decides which font gets `/XF0`, and a resource name
    // that depended on a hash's iteration order would make the synthesised
    // document differ between two runs over the same package.
    let mut out: Vec<(PartName, u32)> = Vec::new();
    let mut seen: std::collections::HashSet<(PartName, u32)> = std::collections::HashSet::new();
    for event in source.reader(&limits.xml) {
        let element = match event {
            Ok(Event::Start(element)) => element,
            // Comments, whitespace and end tags: a fixed page part is full of
            // all three and this pass is looking for one attribute.
            Ok(_) => continue,
            // Markup that will not read is the painter's to report, and it
            // will: this pass answers with the fonts it managed to find and
            // the drawing walk meets the same failure a moment later.
            Err(_) => break,
        };
        if element.local() != "Glyphs" || dialect_of(element.namespace()).is_none() {
            continue;
        }
        let Some(uri) = element.attribute(None, "FontUri") else {
            continue;
        };
        let (reference, fragment) = font_uri(uri);
        let (Some(face), Some(name)) = (face_index(fragment), page.resolve(reference)) else {
            continue;
        };
        if seen.insert((name.clone(), face)) {
            out.push((name, face));
            if out.len() > limits.max_parts {
                return Err(Trouble::Exhausted);
            }
        }
    }
    Ok(out)
}
