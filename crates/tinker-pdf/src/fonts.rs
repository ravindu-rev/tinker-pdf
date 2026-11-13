//! Substitute fonts, supplied by the host (plans 05, 15).
//!
//! Most documents do not embed the standard 14 fonts, because a reader is
//! required to have them. This engine reads no font directories — that is an
//! operating-system dependency and `wasm32-unknown-unknown` has no filesystem
//! at all — so the seam goes here instead. A host that has fonts — a desktop
//! application, a server with a font package installed, a web page with a
//! face already loaded — supplies them through [`FontProvider`], and the
//! engine asks for one only when a document does not embed its own.
//!
//! Without a provider, such a document extracts its text perfectly (the
//! metrics are built in) and draws none of it. That is a visible gap rather
//! than a silent one: the render reports `UnreadableFont`.
//!
//! `BundledFonts` is the answer for a host that has no faces to supply, and
//! it is off by default: `bundled-fonts` carries twelve Liberation faces,
//! asked *after* whatever the host provided. What it is worth was measured
//! rather than argued — 52 % of the corpus's reported degradation was the
//! absence of a face — and the default stays off because a host that has
//! better faces should not carry 4.2 MB of these. `cjk-fonts`, which
//! `bundled-fonts` turns on, carries the four CJK fallback faces, and
//! `BundledFonts` answers a composite font of the registry's CJK collections
//! with its region's face before the Latin families are asked
//! (`docs/design/fallback-faces.md`).

use std::sync::Arc;

use tinker_pdf_cos::{CharacterCollection, CosDocument, Dict, Font, Name, ObjRef, Object, Resolve};

/// What is known about a font that needs substituting.
///
/// Everything here comes from the font dictionary and its descriptor, so a
/// provider can pick a face without parsing anything itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontRequest {
    /// `/BaseFont` with any subset prefix removed — `Helvetica-Bold` rather
    /// than `ABCDEF+Helvetica-Bold`.
    pub base_font: String,
    /// Whether the font declares serifs (`/Flags` bit 2).
    pub serif: bool,
    /// Whether every glyph is the same width (`/Flags` bit 1).
    pub fixed_pitch: bool,
    /// Whether it uses its own encoding rather than a standard one
    /// (`/Flags` bit 3). A symbolic font substituted with a text face draws
    /// the wrong glyphs, so a provider may reasonably decline these.
    pub symbolic: bool,
    /// Whether it is a bold weight, from `/Flags` bit 19 or the name.
    pub bold: bool,
    /// Whether it is italic or oblique, from `/Flags` bit 7 or the name.
    pub italic: bool,
    /// The character collection a composite font's descendant names (9.7.3,
    /// `/CIDSystemInfo`): `Adobe`, `Japan1` and its supplement, say. A
    /// composite font's codes select CIDs in this collection, so it is what a
    /// provider chooses a face by for one (`docs/design/fallback-faces.md`).
    /// Read leniently, as [`tinker_pdf_cos::Font::character_collection`]
    /// reads it; `None` for a simple font, and for a descendant that names
    /// none readably.
    pub character_collection: Option<CharacterCollection>,
    /// The family class the descriptor's `/Style` states (9.8.3.2): the
    /// first two bytes of its twelve-byte `/Panose`, which are the `OS/2`
    /// table's `sFamilyClass` — the class in the high byte, the subclass in
    /// the low — so a provider compares it with a face's own.
    ///
    /// `Some` exactly when [`FontRequest::panose`] is: the two are one
    /// string, and a `/Style` that is not a dictionary, or a `/Panose` that
    /// is not a string of twelve bytes, states neither — a classification
    /// read from part of a string is one the font did not make. Read from
    /// the descriptor the flags are read from, a composite font's
    /// descendant's.
    pub family_class: Option<u16>,
    /// The PANOSE classification the descriptor's `/Style` states (9.8.3.2):
    /// the ten bytes after the family class, in the order the `OS/2` table's
    /// `panose` field holds them.
    pub panose: Option<[u8; 10]>,
}

/// 9.8.2 table 123.
const FIXED_PITCH: i64 = 1;
const SERIF: i64 = 1 << 1;
const SYMBOLIC: i64 = 1 << 2;
const ITALIC: i64 = 1 << 6;
const FORCE_BOLD: i64 = 1 << 18;

impl FontRequest {
    /// Reads what a font dictionary says about itself.
    #[must_use]
    pub fn read(doc: &CosDocument, font: &Dict) -> FontRequest {
        // A composite font keeps its descriptor on the descendant, and its
        // character collection.
        let mut font = font.clone();
        let mut character_collection = None;
        let descendants = doc.resolve_key(&font, Name::DESCENDANT_FONTS);
        if let Some(first) = descendants.as_array().and_then(<[Object]>::first) {
            if let Some(dict) = doc.resolve(first).as_dict() {
                font = dict.clone();
                character_collection = tinker_pdf_cos::font::character_collection(doc, &font);
            }
        }

        let base_font = font
            .get_name(Name::BASE_FONT)
            .and_then(|n| doc.name_bytes(n))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();

        // 9.6.4: a subset is named `ABCDEF+RealName`, and the tag is exactly
        // six capitals. Anything else before a plus is part of the name.
        let base_font = match base_font.split_once('+') {
            Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => {
                rest.to_string()
            }
            _ => base_font,
        };

        let descriptor = doc.resolve_key(&font, Name::FONT_DESCRIPTOR);
        let flags = descriptor
            .as_dict()
            .and_then(|d| d.get_int(Name::FLAGS))
            .unwrap_or(0);

        // The name is consulted as well as the flags, because plenty of
        // producers set neither the weight nor the italic flag and encode both
        // in the name alone.
        let lowered = base_font.to_ascii_lowercase();
        let named_bold =
            lowered.contains("bold") || lowered.contains("black") || lowered.contains("heavy");
        let named_italic = lowered.contains("italic") || lowered.contains("oblique");

        let weight = descriptor
            .as_dict()
            .and_then(|d| d.get_number(doc.intern(b"StemV")));

        let (family_class, panose) = descriptor
            .as_dict()
            .and_then(|d| style_panose(doc, d))
            .map_or((None, None), |(class, panose)| (Some(class), Some(panose)));

        FontRequest {
            serif: flags & SERIF != 0,
            fixed_pitch: flags & FIXED_PITCH != 0,
            symbolic: flags & SYMBOLIC != 0,
            // A stem width past about 120 is a bold face whatever the flags
            // say; it is the only numeric signal a descriptor reliably gives.
            bold: flags & FORCE_BOLD != 0 || named_bold || weight.is_some_and(|v| v >= 120.0),
            italic: flags & ITALIC != 0 || named_italic,
            base_font,
            character_collection,
            family_class,
            panose,
        }
    }
}

/// 9.8.3.2: `/Style`'s one entry, `/Panose`, a twelve-byte string — the two
/// bytes of the `OS/2` table's `sFamilyClass`, class then subclass, and the
/// ten of the PANOSE classification. Table 122 lists `/Style` among a
/// CIDFont's descriptor entries, and a simple font's descriptor that carries
/// one is read the same way.
///
/// `None` for a `/Style` that is not a dictionary, a `/Panose` that is not a
/// string, and a string of any other length: none of them states a
/// classification, and part of one is not one.
fn style_panose(doc: &CosDocument, descriptor: &Dict) -> Option<(u16, [u8; 10])> {
    let style = doc.resolve_key(descriptor, doc.intern(b"Style"));
    let panose = doc.resolve_key(style.as_dict()?, doc.intern(b"Panose"));
    let all = <[u8; 12]>::try_from(panose.as_string()?.bytes.as_slice()).ok()?;
    let [class, subclass, rest @ ..] = all;
    Some((u16::from_be_bytes([class, subclass]), rest))
}

/// Supplies font programs the document does not embed.
///
/// Implementations must be cheap to call repeatedly or do their own caching:
/// the engine asks once per font per page.
pub trait FontProvider: Send + Sync {
    /// A TrueType or bare CFF program for this request, or `None` to leave the
    /// glyphs undrawn.
    ///
    /// Returning `None` is a legitimate answer — declining to substitute a
    /// symbolic font is better than drawing confidently wrong glyphs — and it
    /// leaves the render reporting `UnreadableFont` exactly as it would
    /// without a provider at all.
    fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>>;
}

/// A provider backed by a fixed set of faces, chosen by weight and slope.
///
/// Enough for the common case: a host that has one family available and wants
/// the engine to use it. Faces left empty are filled from whichever is set,
/// so a host with only a regular face still gets text on the page — wrongly
/// weighted, which is better than absent.
#[derive(Default)]
pub struct SimpleFontProvider {
    regular: Option<Arc<Vec<u8>>>,
    bold: Option<Arc<Vec<u8>>>,
    italic: Option<Arc<Vec<u8>>>,
    bold_italic: Option<Arc<Vec<u8>>>,
    /// Whether to answer for symbolic fonts, which is usually wrong.
    substitute_symbolic: bool,
}

impl SimpleFontProvider {
    /// A provider with one face, used for everything.
    #[must_use]
    pub fn new(regular: Vec<u8>) -> SimpleFontProvider {
        SimpleFontProvider {
            regular: Some(Arc::new(regular)),
            ..SimpleFontProvider::default()
        }
    }

    /// Adds a bold face.
    #[must_use]
    pub fn with_bold(mut self, bytes: Vec<u8>) -> SimpleFontProvider {
        self.bold = Some(Arc::new(bytes));
        self
    }

    /// Adds an italic face.
    #[must_use]
    pub fn with_italic(mut self, bytes: Vec<u8>) -> SimpleFontProvider {
        self.italic = Some(Arc::new(bytes));
        self
    }

    /// Adds a bold italic face.
    #[must_use]
    pub fn with_bold_italic(mut self, bytes: Vec<u8>) -> SimpleFontProvider {
        self.bold_italic = Some(Arc::new(bytes));
        self
    }

    /// Answers for symbolic fonts too.
    ///
    /// Off by default. A symbolic font defines its own meaning for every code,
    /// so a text face substituted for one draws confidently wrong glyphs —
    /// a page of letters where there should be arrows or mathematics, which
    /// reads as correct and is not.
    #[must_use]
    pub fn substituting_symbolic(mut self) -> SimpleFontProvider {
        self.substitute_symbolic = true;
        self
    }
}

impl FontProvider for SimpleFontProvider {
    fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
        if request.symbolic && !self.substitute_symbolic {
            return None;
        }

        let exact = match (request.bold, request.italic) {
            (true, true) => &self.bold_italic,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (false, false) => &self.regular,
        };

        exact
            .as_ref()
            .or(self.regular.as_ref())
            .or(self.bold.as_ref())
            .or(self.italic.as_ref())
            .or(self.bold_italic.as_ref())
            .map(Arc::clone)
    }
}

// ---- a font selected by reference (8.4.5's `/Font [font size]`) ----------

/// The first byte of a reserved text-font name. 7.3.5 allows any byte in a
/// name but NUL, so no conforming resource dictionary binds one and no
/// conforming `Tf` writes one.
const RESERVED: u8 = 0;

/// The text-font name a graphics state that sets `/Font [font size]` gives the
/// font in a scope: the scope's own name for the object, when its `/Font`
/// binds one to that reference — the first such entry in file order — so the
/// state and its `Tf` twin select the same name; otherwise the reserved name
/// that spells the reference ([`reserved_name`]), which means the same object
/// in every scope.
pub(crate) fn state_font<R: Resolve + ?Sized>(
    doc: &R,
    resources: &Dict,
    reference: ObjRef,
) -> Vec<u8> {
    let table = doc.resolve_key(resources, Name::FONT);
    let bound = table.as_dict().and_then(|table| {
        table
            .iter()
            .find(|(_, value)| value.as_objref() == Some(reference))
            .and_then(|(name, _)| doc.name_bytes(*name))
    });
    match bound {
        Some(name) => name.to_vec(),
        None => reserved_name(reference),
    }
}

/// The text font and size the `/ExtGState` a scope names `state` sets, under
/// the name [`state_font`] gives it; `None` where it sets none, or sets one a
/// reader cannot read (Table 58: an indirect reference and a finite size).
pub(crate) fn font_of_state<R: Resolve + ?Sized>(
    doc: &R,
    resources: &Dict,
    state: &[u8],
) -> Option<(Vec<u8>, f64)> {
    let table = doc.resolve_key(resources, Name::EXT_G_STATE);
    let entry = doc.resolve(table.as_dict()?.get(doc.intern(state))?);
    let value = doc.resolve(entry.as_dict()?.get(Name::FONT)?);
    let [font, size] = value.as_array()? else {
        return None;
    };
    let reference = font.as_objref()?;
    let size = doc
        .resolve(size)
        .as_number()
        .filter(|size| size.is_finite())?;
    Some((state_font(doc, resources, reference), size))
}

/// The reserved name for a font object: a NUL byte, then its object and
/// generation numbers in decimal.
pub(crate) fn reserved_name(reference: ObjRef) -> Vec<u8> {
    let mut name = vec![RESERVED];
    name.extend_from_slice(format!("{} {}", reference.num, reference.gen).as_bytes());
    name
}

/// The object a reserved name spells, or `None` for any other name.
pub(crate) fn reserved_reference(name: &[u8]) -> Option<ObjRef> {
    let rest = name.strip_prefix(&[RESERVED])?;
    let (num, gen) = core::str::from_utf8(rest).ok()?.split_once(' ')?;
    Some(ObjRef::new(num.parse().ok()?, gen.parse().ok()?))
}

/// What a warning calls a text-font name: the reference a reserved name
/// spells, `5 0 R` (ruling 10: the warning names the object), or the name
/// itself.
pub(crate) fn shown(name: &[u8]) -> Vec<u8> {
    match reserved_reference(name) {
        Some(reference) => format!("{} {} R", reference.num, reference.gen).into_bytes(),
        None => name.to_vec(),
    }
}

/// The font a text-font name stands for in a scope, as a font dictionary and
/// its reference: the scope's `/Font` entry for the name; else, for a reserved
/// name, the object it spells; else nothing, the missing font it is. The scope
/// is asked first, so no name that resolves through `/Font` resolves
/// differently.
pub(crate) fn text_font<R: Resolve + ?Sized>(
    doc: &R,
    resources: &Dict,
    name: &[u8],
) -> Option<(Dict, Option<ObjRef>)> {
    let table = doc.resolve_key(resources, Name::FONT);
    if let Some(entry) = table
        .as_dict()
        .and_then(|table| table.get(doc.intern(name)))
    {
        let reference = entry.as_objref();
        return doc
            .resolve(entry)
            .as_dict()
            .cloned()
            .map(|dict| (dict, reference));
    }
    let reference = reserved_reference(name)?;
    let object = doc.get(reference).ok()?;
    object
        .as_dict()
        .cloned()
        .map(|dict| (dict, Some(reference)))
}

/// A reserved name's font id: its reference with the top bit set, which no
/// interned name's id (a `u32`) can be, and the same in every scope.
pub(crate) fn reserved_id(reference: ObjRef) -> u64 {
    (1 << 63) | (u64::from(reference.num) << 16) | u64::from(reference.gen)
}

/// The reference a reserved name's font id carries, or `None` for any other
/// id.
pub(crate) fn reserved_id_reference(id: u64) -> Option<ObjRef> {
    (id >> 63 == 1).then(|| ObjRef::new(((id >> 16) & 0xFFFF_FFFF) as u32, (id & 0xFFFF) as u16))
}

/// Whether a font is one whose glyphs a substitute can stand in for.
///
/// A Type 3 font's glyphs are content streams in the document itself, so
/// there is nothing to substitute: it either draws from its own procedures or
/// not at all.
#[must_use]
pub(crate) fn is_substitutable(font: &Font) -> bool {
    !matches!(font.kind(), tinker_pdf_cos::FontKind::Type3)
}

/// The faces the build carries: the four CJK fallback faces with
/// `cjk-fonts`, and the twelve Liberation faces with `bundled-fonts`, which
/// turns `cjk-fonts` on.
///
/// Not something a caller has to install: it is the *last* place a request
/// goes, after whatever provider the host supplied. A host that has better
/// faces keeps them and this answers only what that one declines, which is
/// what makes the feature additive rather than a policy change.
///
/// **A composite font of the registry's CJK collections is answered by its
/// region's face, and by nothing else** (`docs/design/fallback-faces.md`):
/// Adobe-Japan1 by Source Han Sans JP, Adobe-GB1 by CN, Adobe-CNS1 by TW,
/// Adobe-Korea1 and Adobe-KR by KR. Its name and its descriptor's flags are
/// not read for it — a CJK font's descriptor says it is symbolic, and its
/// name is often a Latin family's — and a Latin face is never its answer:
/// its CIDs stand for characters no Liberation face draws, so a request whose
/// region's face cannot be made ready is declined, not passed on. The face is
/// read by character, through the collection's inverted CMaps
/// (`resources.rs`'s `substitute_choice`), as a host's face is.
///
/// Off by default, and off is still the right default. What it is worth was
/// measured rather than argued — `corpus/ratchet-fonts.json` against
/// `corpus/ratchet.json`, and 52 % of all reported degradation was the absence
/// of a face — but a desktop application, a server with a font package or a
/// web page with a face already loaded all have something better than
/// Liberation and a way to hand it over.
#[cfg(any(feature = "bundled-fonts", feature = "cjk-fonts"))]
#[derive(Clone, Copy, Debug, Default)]
pub struct BundledFonts;

#[cfg(any(feature = "bundled-fonts", feature = "cjk-fonts"))]
impl FontProvider for BundledFonts {
    fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
        #[cfg(feature = "cjk-fonts")]
        if let Some(region) = cjk_region(request) {
            return tinker_pdf_font::bundled::cjk_face(region);
        }
        #[cfg(feature = "bundled-fonts")]
        {
            latin_face(request)
        }
        #[cfg(not(feature = "bundled-fonts"))]
        {
            None
        }
    }
}

/// The CJK region whose face answers `request`: a composite font's character
/// collection, where it is one of the registry's five CJK collections.
#[cfg(feature = "cjk-fonts")]
fn cjk_region(request: &FontRequest) -> Option<tinker_pdf_font::bundled::Region> {
    let collection = request.character_collection.as_ref()?;
    tinker_pdf_font::bundled::Region::of(&collection.registry, &collection.ordering)
}

/// The Liberation face for a request, by its name and flags, or `None` where
/// `family_for` declines it: a symbolic font, by its flag or its name.
#[cfg(feature = "bundled-fonts")]
fn latin_face(request: &FontRequest) -> Option<Arc<Vec<u8>>> {
    use tinker_pdf_font::bundled::{face, family_for, Family};

    let family = family_for(
        &request.base_font,
        request.serif,
        request.fixed_pitch,
        request.symbolic,
    )?;
    let index = match family {
        Family::Sans => 0,
        Family::Serif => 4,
        Family::Mono => 8,
    } + usize::from(request.bold)
        + (usize::from(request.italic) << 1);

    // One heap copy per face that is actually asked for, kept forever.
    // The trait hands back an `Arc<Vec<u8>>` and the faces are
    // `&'static [u8]`, so a copy is unavoidable; what is avoidable is
    // making it per page, and making all twelve when a document uses one.
    static FACES: [std::sync::OnceLock<Arc<Vec<u8>>>; 12] =
        [const { std::sync::OnceLock::new() }; 12];
    let slot = FACES.get(index)?;
    Some(Arc::clone(slot.get_or_init(|| {
        Arc::new(face(family, request.bold, request.italic).to_vec())
    })))
}

/// Two providers in order: the host's, then whatever answers for it.
#[cfg(any(feature = "bundled-fonts", feature = "cjk-fonts"))]
struct Chain {
    first: Arc<dyn FontProvider>,
    then: Arc<dyn FontProvider>,
}

#[cfg(any(feature = "bundled-fonts", feature = "cjk-fonts"))]
impl FontProvider for Chain {
    fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
        // Per request, not per document. A host provider that has a face for
        // the body text and none for the monospaced code sample must be able
        // to answer one and decline the other, which a document-level "did the
        // host supply a provider?" test cannot express.
        self.first
            .substitute(request)
            .or_else(|| self.then.substitute(request))
    }
}

/// What the engine will actually ask, given what the caller supplied.
///
/// The one place the bundled faces are attached, so `open_with` and
/// `with_fonts` cannot disagree about whether they apply. With neither
/// `bundled-fonts` nor `cjk-fonts` this is the identity and compiles to
/// nothing; with `cjk-fonts` alone, the faces attached answer CJK requests
/// only.
#[must_use]
pub(crate) fn effective(supplied: Option<Arc<dyn FontProvider>>) -> Option<Arc<dyn FontProvider>> {
    #[cfg(not(any(feature = "bundled-fonts", feature = "cjk-fonts")))]
    {
        supplied
    }
    #[cfg(any(feature = "bundled-fonts", feature = "cjk-fonts"))]
    {
        Some(match supplied {
            None => Arc::new(BundledFonts),
            Some(first) => Arc::new(Chain {
                first,
                then: Arc::new(BundledFonts),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_cos::CosDocument;

    fn doc_with(font: &str) -> (CosDocument, Dict) {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n{font}\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = CosDocument::open(bytes.into_bytes()).expect("it opens");
        let dict = doc
            .get(tinker_pdf_cos::ObjRef::new(3, 0))
            .expect("the font loads")
            .as_dict()
            .cloned()
            .expect("a dictionary");
        (doc, dict)
    }

    #[test]
    fn a_subset_prefix_is_stripped() {
        let (doc, dict) = doc_with("<< /Type /Font /BaseFont /ABCDEF+Helvetica >>");
        assert_eq!(FontRequest::read(&doc, &dict).base_font, "Helvetica");
    }

    /// The tag is exactly six capitals. A plus in an ordinary name is part of
    /// the name, and stripping it would lose half of it.
    #[test]
    fn something_that_only_looks_like_a_subset_prefix_is_kept() {
        for name in ["/Abc+Helvetica", "/TOOLONGX+Helvetica", "/abcdef+Helvetica"] {
            let (doc, dict) = doc_with(&format!("<< /Type /Font /BaseFont {name} >>"));
            let read = FontRequest::read(&doc, &dict).base_font;
            assert!(read.contains('+'), "{name} became {read}");
        }
    }

    #[test]
    fn the_descriptor_flags_are_read() {
        let (doc, dict) = doc_with(
            "<< /Type /Font /BaseFont /Times /FontDescriptor \
             << /Flags 7 /StemV 40 >> >>",
        );
        let request = FontRequest::read(&doc, &dict);
        assert!(request.fixed_pitch, "bit 1");
        assert!(request.serif, "bit 2");
        assert!(request.symbolic, "bit 3");
        assert!(!request.bold);
    }

    /// Plenty of producers set neither flag and put both in the name.
    #[test]
    fn weight_and_slope_are_taken_from_the_name_as_well() {
        let (doc, dict) = doc_with("<< /Type /Font /BaseFont /Helvetica-BoldOblique >>");
        let request = FontRequest::read(&doc, &dict);
        assert!(request.bold, "from the name");
        assert!(request.italic, "oblique counts");
    }

    #[test]
    fn a_wide_stem_reads_as_bold() {
        let (doc, dict) =
            doc_with("<< /Type /Font /BaseFont /Plain /FontDescriptor << /StemV 160 >> >>");
        assert!(FontRequest::read(&doc, &dict).bold);
    }

    /// The descriptor of a composite font is on its descendant, and looking
    /// only at the parent finds nothing at all.
    #[test]
    fn a_composite_fonts_descriptor_is_found_on_the_descendant() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /GHIJKL+Noto-Bold\n\
   /DescendantFonts [4 0 R] >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Noto-Bold\n\
   /FontDescriptor << /Flags 2 >> >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes).expect("it opens");
        let dict = doc
            .get(tinker_pdf_cos::ObjRef::new(3, 0))
            .expect("loads")
            .as_dict()
            .cloned()
            .expect("a dictionary");

        let request = FontRequest::read(&doc, &dict);
        assert!(request.serif, "the descendant's flags are read");
        assert!(request.bold, "and the name still applies");
        assert_eq!(request.base_font, "Noto-Bold");
    }

    /// 9.8.3.2: the family class and the ten PANOSE bytes, from a simple
    /// font's descriptor and from a composite font's descendant's, which is
    /// where the flags are read from too.
    #[test]
    fn the_descriptor_panose_is_read() {
        // Class 8 (sans serif), subclass 1, then a PANOSE of a sans face.
        let (doc, dict) = doc_with(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Sans /FontDescriptor \
             << /Flags 32 /Style << /Panose <0801020B0604020202020204> >> >> >>",
        );
        let request = FontRequest::read(&doc, &dict);
        assert_eq!(request.family_class, Some(0x0801));
        assert_eq!(request.panose, Some([2, 0x0B, 6, 4, 2, 2, 2, 2, 2, 4]));

        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Mincho-Identity-H\n\
   /Encoding /Identity-H /DescendantFonts [4 0 R]\n\
   /FontDescriptor << /Style << /Panose <000000000000000000000000> >> >> >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /CIDFontType0 /BaseFont /Mincho\n\
   /FontDescriptor << /Flags 6 /Style << /Panose <01 05 02 02 03 00 00 00 00 00 00 00> >> >> >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes).expect("it opens");
        let dict = doc
            .get(tinker_pdf_cos::ObjRef::new(3, 0))
            .expect("loads")
            .as_dict()
            .cloned()
            .expect("a dictionary");
        let request = FontRequest::read(&doc, &dict);
        assert_eq!(
            request.family_class,
            Some(0x0105),
            "the descendant's descriptor, as 9.8.3.2's own example writes it, \
             and not the one a Type 0 dictionary has no business carrying"
        );
        assert_eq!(request.panose, Some([2, 2, 3, 0, 0, 0, 0, 0, 0, 0]));
    }

    /// A string of eleven or thirteen bytes, a name, a `/Style` with no
    /// `/Panose`, one that is not a dictionary and a descriptor with no
    /// `/Style` at all: each states no classification, and none is made up
    /// from what is there — neither the family class nor the PANOSE.
    #[test]
    fn a_malformed_panose_is_absent_not_guessed() {
        for style in [
            "/Style << /Panose <0801020B06040202020202> >>",
            "/Style << /Panose <0801020B060402020202020400> >>",
            "/Style << /Panose /Sans >>",
            "/Style << /Panose [8 1 2 11 6 4 2 2 2 2 2 4] >>",
            "/Style << >>",
            "/Style 7",
            "/Style [<0801020B0604020202020204>]",
            "",
        ] {
            let (doc, dict) = doc_with(&format!(
                "<< /Type /Font /Subtype /TrueType /BaseFont /Sans \
                 /FontDescriptor << /Flags 32 {style} >> >>"
            ));
            let request = FontRequest::read(&doc, &dict);
            assert_eq!(request.family_class, None, "{style:?} gave a family class");
            assert_eq!(request.panose, None, "{style:?} gave a PANOSE");
        }

        // A font with no descriptor, which the standard 14 are, has nothing
        // to state either.
        let (doc, dict) = doc_with("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        let request = FontRequest::read(&doc, &dict);
        assert_eq!((request.family_class, request.panose), (None, None));
    }

    fn request(bold: bool, italic: bool, symbolic: bool) -> FontRequest {
        FontRequest {
            base_font: "Whatever".to_string(),
            bold,
            italic,
            symbolic,
            ..FontRequest::default()
        }
    }

    #[test]
    fn the_closest_face_is_chosen() {
        let provider = SimpleFontProvider::new(b"regular".to_vec())
            .with_bold(b"bold".to_vec())
            .with_italic(b"italic".to_vec())
            .with_bold_italic(b"both".to_vec());

        let of = |b, i| {
            provider
                .substitute(&request(b, i, false))
                .map(|f| String::from_utf8_lossy(&f).into_owned())
        };
        assert_eq!(of(false, false).as_deref(), Some("regular"));
        assert_eq!(of(true, false).as_deref(), Some("bold"));
        assert_eq!(of(false, true).as_deref(), Some("italic"));
        assert_eq!(of(true, true).as_deref(), Some("both"));
    }

    /// A host with only one face still gets text on the page. Wrongly
    /// weighted text is better than none.
    #[test]
    fn a_missing_face_falls_back_rather_than_declining() {
        let provider = SimpleFontProvider::new(b"regular".to_vec());
        assert!(provider.substitute(&request(true, true, false)).is_some());
    }

    /// A text face standing in for a symbolic font draws letters where there
    /// should be arrows, which reads as correct and is not.
    #[test]
    fn symbolic_fonts_are_declined_unless_asked_for() {
        let provider = SimpleFontProvider::new(b"regular".to_vec());
        assert!(provider.substitute(&request(false, false, true)).is_none());

        let willing = SimpleFontProvider::new(b"regular".to_vec()).substituting_symbolic();
        assert!(willing.substitute(&request(false, false, true)).is_some());
    }

    #[test]
    fn a_provider_with_nothing_declines() {
        let provider = SimpleFontProvider::default();
        assert!(provider.substitute(&request(false, false, false)).is_none());
    }

    /// A request for an unembedded composite font of `registry` and
    /// `ordering`, named `base_font`, symbolic or not.
    #[cfg(feature = "cjk-fonts")]
    fn composite(base_font: &str, registry: &str, ordering: &str, symbolic: bool) -> FontRequest {
        FontRequest {
            base_font: base_font.to_string(),
            symbolic,
            character_collection: Some(CharacterCollection {
                registry: registry.to_string(),
                ordering: ordering.to_string(),
                supplement: Some(0),
            }),
            ..FontRequest::default()
        }
    }

    /// **A CJK collection is answered by its region's face** (FT-01.9),
    /// whatever its name or its symbolic flag says: the program
    /// `tinker_pdf_font::bundled::cjk_face` made ready for the region, the
    /// one every request shares. A name the Latin families would claim, and
    /// the bold and italic flags, change nothing.
    #[cfg(feature = "cjk-fonts")]
    #[test]
    fn a_cjk_collection_is_answered_by_its_regions_face() {
        use tinker_pdf_font::bundled::{cjk_face, Region};

        for (ordering, region) in [
            ("Japan1", Region::Japan),
            ("GB1", Region::SimplifiedChinese),
            ("CNS1", Region::TraditionalChinese),
            ("Korea1", Region::Korea),
            ("KR", Region::Korea),
        ] {
            let face = cjk_face(region).expect("the region's face is made ready");
            for name in [
                "KozMinPr6N-Regular",
                "Helvetica-Bold",
                "TimesNewRoman",
                "Courier",
            ] {
                for symbolic in [true, false] {
                    let mut request = composite(name, "Adobe", ordering, symbolic);
                    request.bold = name.contains("Bold");
                    let answer = BundledFonts
                        .substitute(&request)
                        .unwrap_or_else(|| panic!("{ordering} {name} symbolic={symbolic}"));
                    assert!(
                        Arc::ptr_eq(&answer, &face),
                        "{ordering} {name} symbolic={symbolic}: its region's face"
                    );
                }
            }
        }
    }

    /// **A CJK request is never answered with a Latin face**, in a build that
    /// carries both: the Liberation face its name would choose is not what
    /// comes back.
    #[cfg(feature = "bundled-fonts")]
    #[test]
    fn a_cjk_request_is_answered_by_no_latin_face() {
        use tinker_pdf_font::bundled::{face, Family};

        let request = composite("Helvetica", "Adobe", "Japan1", false);
        let answer = BundledFonts.substitute(&request).expect("an answer");
        for family in [Family::Sans, Family::Serif, Family::Mono] {
            for bold in [false, true] {
                for italic in [false, true] {
                    assert_ne!(
                        answer.as_slice(),
                        face(family, bold, italic),
                        "{family:?} bold={bold} italic={italic}"
                    );
                }
            }
        }
    }

    /// **A collection of no region is no CJK request**: `Identity`, another
    /// registry and the registry's Manga1 are read as the font's name and
    /// flags read it — Liberation Sans for `Helvetica`, nothing for a
    /// symbolic font.
    #[cfg(feature = "bundled-fonts")]
    #[test]
    fn a_collection_of_no_region_falls_to_the_latin_families() {
        use tinker_pdf_font::bundled::{face, Family};

        for (registry, ordering) in [
            ("Adobe", "Identity"),
            ("Acme", "Japan1"),
            ("Adobe", "Manga1"),
        ] {
            let latin = composite("Helvetica", registry, ordering, false);
            assert_eq!(
                BundledFonts
                    .substitute(&latin)
                    .as_deref()
                    .map(Vec::as_slice),
                Some(face(Family::Sans, false, false)),
                "{registry}-{ordering}"
            );
            let symbolic = composite("Helvetica", registry, ordering, true);
            assert!(
                BundledFonts.substitute(&symbolic).is_none(),
                "{registry}-{ordering}, symbolic"
            );
        }
    }

    /// **With `cjk-fonts` and no `bundled-fonts`, only a CJK request is
    /// answered**: the wasm CJK profile carries no Latin face, so a base-14
    /// request, and a composite font of no region, are declined.
    #[cfg(all(feature = "cjk-fonts", not(feature = "bundled-fonts")))]
    #[test]
    fn without_the_latin_faces_only_a_cjk_request_is_answered() {
        assert!(BundledFonts
            .substitute(&request(false, false, false))
            .is_none());
        let identity = composite("Helvetica", "Adobe", "Identity", false);
        assert!(BundledFonts.substitute(&identity).is_none());
        let japan1 = composite("Helvetica", "Adobe", "Japan1", false);
        assert!(BundledFonts.substitute(&japan1).is_some());
    }
}
