//! `@font-face` resolved against the container: real faces, or a named reason
//! there is none (gap 31, milestone 9).
//!
//! `tinker-pdf-css` reads the rule and says nothing about whether any of it
//! resolves, because it has no archive and is not acquiring one. This is the
//! file that opens the `url()`, undoes whatever obfuscation covers it, refuses
//! the containers this build has no reader for **by name**, and hands
//! [`super::paint`] a set of faces with a `cmap` in them.
//!
//! # Every failure is named, and there are seven of them
//!
//! A book whose fonts do not arrive is set in the standard 14, which looks like
//! a book that never asked for fonts. That is gap 31's whole subject one level
//! down from a CSS property, so each way a face can fail to arrive is its own
//! [`FaceDefect`]: a format this build cannot read, a `local()` this build
//! cannot answer, a `url()` that names nothing in the container, an entry that
//! will not inflate, a key the package cannot produce, bytes that are not a
//! font, and a `src` list every entry of which failed. Seven, and not one
//! "the font did not load", because a producer told the fourth cannot fix the
//! first.
//!
//! # `format()` is a hint and the bytes are the answer
//!
//! §4.3 lets a sheet say what a file is and browsers use it to skip a download.
//! This build does the same — a `format("woff2")` is refused without reading
//! the entry — **and** sniffs the bytes of everything it does read, because a
//! `format()` that lies is a real file and a producer that omits it is
//! commoner still. The two checks catch different books and a build with only
//! one of them would be wrong about the other's.
//!
//! # WOFF and WOFF2 are refused rather than decoded
//!
//! Both are a container around an sfnt: WOFF 1.0 deflates each table, WOFF2
//! rewrites `glyf` into a transformed form and Brotli-compresses the lot.
//! Undoing the first is a table-directory rebuild and undoing the second is a
//! Brotli decoder plus a glyph re-encoder, and neither is in gap 31's scope.
//! What is in scope is that a book whose only face is a WOFF2 **says so**, so a
//! host can convert it, rather than quietly setting the book in Times.

use tinker_pdf_css::font_face::{FontFace, FontFormat, FontSource};
use tinker_pdf_css::property::FontStyle;
use tinker_pdf_font::Sfnt;

use super::obfuscation::{deobfuscate, KeyDefect};
use super::ocf::{resolve_reference, Encryption, Ocf};
use super::Limits;

/// `hhea`, as a big-endian table tag.
const HHEA: u32 = 0x6868_6561;

/// Why one `@font-face` did not become a face.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaceDefect {
    /// The `src` entry declared a `format()` this build has no reader for,
    /// with the keyword the sheet wrote.
    UnsupportedFormat(String),
    /// The bytes are a WOFF or WOFF2 container whatever the sheet said.
    ///
    /// Distinct from [`FaceDefect::UnsupportedFormat`] because the two are
    /// found by different means and a book can have either without the other:
    /// a `src` with no `format()` at all reaches this one, and a `format()`
    /// naming a file that is really an OpenType reaches the other.
    PackedContainer(&'static str),
    /// `local()`: a face installed on the reading system, which this one has
    /// none of.
    LocalUnavailable,
    /// The `url()` resolves to nothing in the container.
    ResourceMissing,
    /// The entry is there and will not inflate.
    Unreadable,
    /// `encryption.xml` covers the resource and the key could not be derived.
    KeyUnavailable(KeyDefect),
    /// The bytes are not an sfnt this build reads.
    NotAFont,
    /// Every entry of the `src` list failed, so the family has no file.
    ///
    /// Reported **beside** the entry defects rather than instead of them: the
    /// entries say what went wrong and this says the rule as a whole came to
    /// nothing, and a book whose first `src` entry failed and whose second
    /// worked produces the first and not this.
    NoUsableSource,
}

impl FaceDefect {
    /// A short name for a report to print.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            FaceDefect::UnsupportedFormat(name) => format!("src format({name}) is not read here"),
            FaceDefect::PackedContainer(name) => format!("the file is a {name} container"),
            FaceDefect::LocalUnavailable => {
                "local() names a face this build has none of".to_owned()
            }
            FaceDefect::ResourceMissing => "the src url is not in the container".to_owned(),
            FaceDefect::Unreadable => "the container entry will not inflate".to_owned(),
            FaceDefect::KeyUnavailable(_) => "the obfuscation key cannot be derived".to_owned(),
            FaceDefect::NotAFont => "the bytes are not a font this build reads".to_owned(),
            FaceDefect::NoUsableSource => "no src entry produced a face".to_owned(),
        }
    }
}

/// One face a book embedded, with its program and the descriptors it was
/// declared under.
#[derive(Clone, Debug)]
pub struct EmbeddedFace {
    /// The `@font-face` family, lower-cased.
    pub family: String,
    /// The `font-weight` descriptor, as a range.
    pub weight: (u16, u16),
    /// The `font-style` descriptor.
    pub style: FontStyle,
    /// The container path the program came from, which is what a report names.
    pub path: String,
    /// The font program, **de-obfuscated**, exactly as it will be embedded.
    pub program: Vec<u8>,
    /// The PDF resource name this face is registered under.
    pub resource: Vec<u8>,
}

impl EmbeddedFace {
    /// The glyph this face has for a character, if it has one.
    ///
    /// Glyph 0 is `.notdef` and a `cmap` that answers with it is a `cmap`
    /// saying *no* — so a face whose table maps every unmapped code to zero,
    /// which is what a format 4 subtable with a wide segment does, does not
    /// claim to cover the whole plane.
    #[must_use]
    pub fn glyph(&self, ch: char) -> Option<u16> {
        let sfnt = Sfnt::parse(&self.program)?;
        sfnt.glyph_for_char(ch).filter(|id| *id != 0)
    }

    /// Whether this face can draw a character.
    #[must_use]
    pub fn covers(&self, ch: char) -> bool {
        self.glyph(ch).is_some()
    }

    /// A character's advance, as a fraction of the em.
    ///
    /// From the face's own `hmtx`, which is the only number that can agree
    /// with the glyphs a viewer will draw. A face with no `hmtx` — a bare CFF
    /// wrapped in an `OTTO` with the table dropped by a bad subsetter — gets
    /// half an em rather than nothing, because a run measured at zero collapses
    /// to a point and a run measured at nothing cannot be laid out at all.
    #[must_use]
    pub fn advance_em(&self, ch: char) -> f64 {
        let Some(sfnt) = Sfnt::parse(&self.program) else {
            return 0.5;
        };
        let units = f64::from(sfnt.units_per_em.max(1));
        let glyph = sfnt.glyph_for_char(ch).unwrap_or(0);
        match sfnt.advance(glyph) {
            Some(advance) => f64::from(advance) / units,
            None => 0.5,
        }
    }

    /// The face's ascent and descent, as fractions of the em, both positive.
    ///
    /// `hhea`'s own numbers, with the descender's sign flipped because
    /// [`tinker_pdf_layout::metrics::Vertical`] measures a depth below the
    /// baseline as a positive number. A face with no readable `hhea` gets
    /// `None` and the caller uses the standard face's proportions, which is a
    /// worse answer than the file's and a much better one than zero.
    ///
    /// **`ascender` is at byte 4 and `descender` at byte 6.** Bytes 0 to 3 are
    /// the table's version, which is `0x0001_0000` in every font there has ever
    /// been — so a build that read the first two fields from the front of the
    /// table gets an ascent of **one unit** and a descent of zero for every
    /// face alike, which is a line height of a thousandth of an em and looks
    /// from a distance like a working build with tight leading. Milestone 9
    /// shipped that bug and `an_embedded_faces_line_height_is_its_own_hhea`
    /// found it, which is why the offsets are named here rather than left to
    /// the specification: the wrong ones are *plausible* and the right ones
    /// have to be argued for.
    #[must_use]
    pub fn vertical_fractions(&self) -> Option<(f64, f64)> {
        let sfnt = Sfnt::parse(&self.program)?;
        let hhea = sfnt.table(HHEA)?;
        let units = f64::from(sfnt.units_per_em.max(1));
        let ascender = i16::from_be_bytes([*hhea.get(4)?, *hhea.get(5)?]);
        let descender = i16::from_be_bytes([*hhea.get(6)?, *hhea.get(7)?]);
        if ascender <= 0 {
            return None;
        }
        Some((
            f64::from(ascender) / units,
            f64::from(descender.saturating_neg().max(0)) / units,
        ))
    }
}

/// Every face a book brought with it, and every reason one is missing.
#[derive(Clone, Debug, Default)]
pub struct FaceSet {
    faces: Vec<EmbeddedFace>,
    defects: Vec<(String, FaceDefect)>,
}

impl FaceSet {
    /// The shared empty set.
    ///
    /// A `const` reference rather than a `Default` call so that
    /// [`super::paint::BookMetrics::STANDARD`] can be a `const` too — which is
    /// what lets a caller name *the standard 14 and nothing else* without
    /// having a book to hand.
    pub const EMPTY: &'static FaceSet = &FaceSet {
        faces: Vec::new(),
        defects: Vec::new(),
    };

    /// A book with no `@font-face` at all.
    #[must_use]
    pub fn new() -> FaceSet {
        FaceSet::default()
    }

    /// The faces, in declaration order.
    #[must_use]
    pub fn faces(&self) -> &[EmbeddedFace] {
        &self.faces
    }

    /// What could not be loaded, as `(family, defect)` pairs in the order the
    /// rules were written.
    #[must_use]
    pub fn defects(&self) -> &[(String, FaceDefect)] {
        &self.defects
    }

    /// Whether this book has any embedded face at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The best face for a family at a weight and a slope **that covers a
    /// character**, or `None`.
    ///
    /// `css-fonts-4` §5.2's ordering — slope first, then weight — with §5.3's
    /// per-character step folded in: a family whose faces exist but none of
    /// which has the glyph is not a match for *this character*, and the caller
    /// moves to the next family. That is the difference between per-character
    /// fallback and a single face with holes in it, and it is why coverage is
    /// a parameter here rather than a filter the caller applies afterwards.
    ///
    /// `ch` of `None` asks the same question ignoring coverage, which is what a
    /// line height needs: a run's leading is a property of its nominal face
    /// and does not change part way along because one character fell through.
    #[must_use]
    pub fn best(
        &self,
        family: &str,
        weight: u16,
        style: FontStyle,
        ch: Option<char>,
    ) -> Option<usize> {
        let mut best: Option<(u32, u32, usize)> = None;
        for (index, face) in self.faces.iter().enumerate() {
            if !face.family.eq_ignore_ascii_case(family) {
                continue;
            }
            if let Some(ch) = ch {
                if !face.covers(ch) {
                    continue;
                }
            }
            let key = (
                style_distance(face.style, style),
                weight_distance(face.weight, weight),
                index,
            );
            // §5.2 sorts on slope before weight, and the index is the last
            // criterion so that two faces a book declared identically resolve
            // to the first — which makes the choice a function of the book
            // rather than of a hash order.
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best.map(|(_, _, index)| index)
    }

    /// Any face at all that covers a character, in declaration order.
    ///
    /// §5.3's system fallback, for a reading system whose "system" is the book:
    /// once the author's `font-family` list is exhausted a browser looks
    /// through the faces it has, and the faces this build has are the standard
    /// 14 and whatever the book embedded. Trying the book's own faces before
    /// giving up is what puts a CJK line on the page when the book shipped a
    /// CJK face under a family its `body` rule never mentions.
    #[must_use]
    pub fn any_covering(&self, ch: char) -> Option<usize> {
        self.faces.iter().position(|face| face.covers(ch))
    }
}

/// §5.2's slope distance: italic and oblique substitute for one another.
fn style_distance(have: FontStyle, want: FontStyle) -> u32 {
    match (have, want) {
        (a, b) if a == b => 0,
        (FontStyle::Italic, FontStyle::Oblique) | (FontStyle::Oblique, FontStyle::Italic) => 1,
        _ => 2,
    }
}

/// §5.2's weight distance, with the direction the specification prefers.
fn weight_distance(range: (u16, u16), wanted: u16) -> u32 {
    let (low, high) = range;
    if wanted >= low && wanted <= high {
        return 0;
    }
    let (distance, heavier) = if wanted < low {
        (u32::from(low - wanted), true)
    } else {
        (u32::from(wanted - high), false)
    };
    let deprioritised = if wanted <= 450 { heavier } else { !heavier };
    distance + u32::from(deprioritised) * 10_000
}

/// Whether a `format()` hint names a container this build has no reader for.
///
/// `None` means *read the bytes and find out*, which is what an absent hint and
/// a hint naming a plain sfnt both mean.
fn refused_by_hint(format: Option<&FontFormat>) -> Option<String> {
    match format? {
        FontFormat::OpenType | FontFormat::TrueType | FontFormat::Collection => None,
        // An unrecognised keyword is **not** refused on the hint: §4.3 makes
        // `format()` advisory, and a sheet that wrote a vendor keyword over a
        // perfectly ordinary OpenType file should still get its font. The
        // bytes decide.
        FontFormat::Other(_) => None,
        other => Some(other.name().to_owned()),
    }
}

/// Whether the bytes are a packed container rather than a bare sfnt.
fn packed_container(bytes: &[u8]) -> Option<&'static str> {
    match bytes.get(..4) {
        Some(b"wOFF") => Some("woff"),
        Some(b"wOF2") => Some("woff2"),
        _ => None,
    }
}

/// Loads every `@font-face` a book declared.
///
/// `identifier` is the package's unique identifier, which is the only input
/// either obfuscation key has; `encryption` is what `META-INF/encryption.xml`
/// said covers which resource.
///
/// Faces are registered in declaration order and resource names follow that
/// order, so a document's font resources are a function of the book rather
/// than of the order the reader happened to meet its stylesheets in.
///
/// # One face per file, and the caller cannot do this part
///
/// The caller deduplicates the **rules** before calling, which collapses the
/// thirteen chapters that `<link>` one stylesheet into one rule: an imported
/// sheet's faces carry that sheet's address, and thirteen chapters linking it
/// produce thirteen identical [`FontFace`]s. A `<style>` element has no address
/// of its own, so its rules carry the **content document's** — which is a
/// different string in every chapter, and thirteen chapters with the same
/// `<style>` block are thirteen rules the caller cannot tell apart.
///
/// They resolve to one container entry, and that is what this sees. So a rule
/// whose family, weight, slope and resolved path match a face already loaded
/// adds no second face: a thirteen-chapter book with a two-megabyte CJK face in
/// a `<style>` block inflates and parses it **once**. The defect list is not
/// collapsed with it — thirteen rules that all failed are thirteen rules, and
/// [`super::ArchiveWarning::FontFace`]'s count is where that is said.
pub fn load(
    book: &mut Ocf<'_>,
    faces: &[FontFace],
    identifier: Option<&str>,
    encryption: &Encryption,
    limits: &Limits,
) -> FaceSet {
    let mut out = FaceSet::default();
    for rule in faces {
        match load_one(book, rule, identifier, encryption, limits, &mut out.defects) {
            Some((path, program)) => {
                if out.faces.iter().any(|face| {
                    face.path == path
                        && face.family == rule.family
                        && face.weight == rule.weight
                        && face.style == rule.style
                }) {
                    continue;
                }
                let resource = format!("Bf{}", out.faces.len()).into_bytes();
                out.faces.push(EmbeddedFace {
                    family: rule.family.clone(),
                    weight: rule.weight,
                    style: rule.style,
                    path,
                    program,
                    resource,
                });
            }
            None => out
                .defects
                .push((rule.family.clone(), FaceDefect::NoUsableSource)),
        }
    }
    out
}

/// Walks one rule's `src` list and returns the first entry that produced a
/// face.
///
/// **The list is walked to the end.** §4.3 makes it a preference order and a
/// build that stopped at the first failure would lose every book that writes
/// `url(x.woff2) format("woff2"), url(x.otf) format("opentype")` — which is
/// what a modern producer writes, with the entry this build cannot use first.
fn load_one(
    book: &mut Ocf<'_>,
    rule: &FontFace,
    identifier: Option<&str>,
    encryption: &Encryption,
    limits: &Limits,
    defects: &mut Vec<(String, FaceDefect)>,
) -> Option<(String, Vec<u8>)> {
    for source in &rule.sources {
        let (url, format) = match source {
            FontSource::Local(_) => {
                defects.push((rule.family.clone(), FaceDefect::LocalUnavailable));
                continue;
            }
            FontSource::Url { url, format } => (url, format.as_ref()),
        };
        if let Some(name) = refused_by_hint(format) {
            defects.push((rule.family.clone(), FaceDefect::UnsupportedFormat(name)));
            continue;
        }
        // A `<style>` element has no address of its own, and the content
        // document that holds it is the base the caller put in `base`.
        let base = rule.base.as_deref().unwrap_or("");
        let Ok(path) = resolve_reference(base, url, limits) else {
            defects.push((rule.family.clone(), FaceDefect::ResourceMissing));
            continue;
        };
        let Some(index) = book.index_of(&path) else {
            defects.push((rule.family.clone(), FaceDefect::ResourceMissing));
            continue;
        };
        let Ok(bytes) = book.read(index).map(<[u8]>::to_vec) else {
            defects.push((rule.family.clone(), FaceDefect::Unreadable));
            continue;
        };
        let mut program = bytes;
        if let Some(entry) = encryption.entries().iter().find(|e| e.path == path) {
            if let Err(defect) =
                deobfuscate(entry.algorithm, identifier.unwrap_or(""), &mut program)
            {
                defects.push((rule.family.clone(), FaceDefect::KeyUnavailable(defect)));
                continue;
            }
        }
        // After de-obfuscation and not before: a WOFF2 that a book obfuscated
        // is `wOF2` only once the XOR is undone, and a build that sniffed the
        // obfuscated bytes would see neither the packed container nor a font.
        if let Some(name) = packed_container(&program) {
            defects.push((rule.family.clone(), FaceDefect::PackedContainer(name)));
            continue;
        }
        if Sfnt::parse(&program).is_none() {
            defects.push((rule.family.clone(), FaceDefect::NotAFont));
            continue;
        }
        return Some((path, program));
    }
    None
}
