//! Substitute fonts, supplied by the host (plans 05, 15).
//!
//! Most documents do not embed the standard 14 fonts, because a reader is
//! required to have them. This engine has none: it bundles no font programs
//! and reads no directories, because bundling a face is a licensing decision
//! and reading a directory is an operating-system dependency, and
//! `wasm32-unknown-unknown` has no filesystem at all.
//!
//! So the seam goes here instead. A host that has fonts — a desktop
//! application, a server with a font package installed, a web page with a
//! face already loaded — supplies them through [`FontProvider`], and the
//! engine asks for one only when a document does not embed its own.
//!
//! Without a provider, such a document extracts its text perfectly (the
//! metrics are built in) and draws none of it. That is a visible gap rather
//! than a silent one: the render reports `UnreadableFont`.

use std::sync::Arc;

use tinker_pdf_cos::{CosDocument, Dict, Font, Object};

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
        // A composite font keeps its descriptor on the descendant.
        let mut font = font.clone();
        let descendants = doc.resolve_key(&font, doc.intern(b"DescendantFonts"));
        if let Some(first) = descendants.as_array().and_then(<[Object]>::first) {
            if let Some(dict) = doc.resolve(first).as_dict() {
                font = dict.clone();
            }
        }

        let base_font = font
            .get_name(doc.intern(b"BaseFont"))
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

        let descriptor = doc.resolve_key(&font, doc.intern(b"FontDescriptor"));
        let flags = descriptor
            .as_dict()
            .and_then(|d| d.get_int(doc.intern(b"Flags")))
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

        FontRequest {
            serif: flags & SERIF != 0,
            fixed_pitch: flags & FIXED_PITCH != 0,
            symbolic: flags & SYMBOLIC != 0,
            // A stem width past about 120 is a bold face whatever the flags
            // say; it is the only numeric signal a descriptor reliably gives.
            bold: flags & FORCE_BOLD != 0 || named_bold || weight.is_some_and(|v| v >= 120.0),
            italic: flags & ITALIC != 0 || named_italic,
            base_font,
        }
    }
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

/// Reads the font dictionary a resource name selects, for building a request.
#[must_use]
pub(crate) fn font_dict(doc: &CosDocument, resources: &Dict, name: &[u8]) -> Option<Dict> {
    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    let dict = fonts.as_dict()?;
    doc.resolve_key(dict, doc.intern(name)).as_dict().cloned()
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
}
