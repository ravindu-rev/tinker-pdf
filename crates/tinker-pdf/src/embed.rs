//! Embedding the fonts a document draws and does not embed, on the save door
//! (`docs/design/font-embedding.md`, FT-12).
//!
//! A document that names a font and embeds no program leaves its reader to
//! supply the glyphs. This engine supplies them through the host's
//! [`FontProvider`], and a file drawn that way draws differently in the next
//! reader. So [`crate::write::save`], asked by
//! [`crate::write::SaveOptions::embed`], writes the face the provider drew into
//! the file, cut to the glyphs drawn, and the rewritten document draws as the
//! provider face drew it wherever it is opened.
//!
//! # What is embedded, and what is checked
//!
//! The walk is the font pass's ([`crate::subset`]), which records the codes
//! shown through each font object beside the glyphs; a font it shows a code
//! through whose descriptor names no program is a candidate. The provider is
//! asked once a font with the request the renderer asks with, chained as a
//! document opened with it draws ([`fonts::effective`]), and its answer is
//! read glyph by glyph through the renderer's own reading of a substituted
//! face ([`resources::substitute_choice`]). A code that reading reaches
//! nothing for leaves the font: embedding would make a missing glyph
//! permanent.
//!
//! Each font's objects — the cut program, a new descriptor, the font
//! dictionary with its new `/Subtype`, `/BaseFont` and, where it had none,
//! `/Widths` — are written in one editor transaction, and before it commits
//! the font is read back through the editor and every drawn code asked what
//! a reader of the new file asks: the glyph the embedded program's mapping
//! chooses, the width it is laid out with and the text it reads as. One that
//! differs from what the document drew and read before rolls the
//! transaction back and leaves the font, named. Nothing a code reads as is
//! assumed to have survived; it is read again.
//!
//! # What is left, and named
//!
//! Every font the walk shows a code through and that embeds no program is in
//! the report, embedded or left with its [`LeftReason`] (ruling 10); a font
//! that embeds a program, and a Type 3 font, whose glyphs are the document's
//! own procedures (9.6.5), are not candidates and are not reported.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tinker_pdf_cos::{
    font as cos_font, subset_tag, Dict, DocumentEditor, Name, ObjRef, Object, Resolve, StreamData,
};
use tinker_pdf_font::{Cff, FaceFacts, Sfnt};

use crate::fonts::{self, FontProvider, FontRequest};
use crate::pdfa::fonts::{ProgramMetrics, WIDTH_TOLERANCE};
use crate::resources::{self, Operation, Route};
use crate::stats::Work;
use crate::subset::{self, UntouchedReason};
use crate::Parallelism;

/// What a save does with the fonts the document draws and embeds no program
/// for.
///
/// [`FontEmbedding::Leave`] by default, as [`crate::write::ImagePolicy::Keep`]
/// is: a default that embedded the host's faces would put them in every saved
/// file unasked, and a face is the host's to hand over.
#[derive(Clone, Default)]
pub enum FontEmbedding {
    /// Embed nothing: the pass is not entered, and a save writes what it
    /// would have written before the pass existed. The default.
    #[default]
    Leave,
    /// Embed, for each font the pages draw whose dictionary embeds no
    /// program, the face this provider answers for it — chained with the
    /// bundled faces in a `bundled-fonts` build, as a document opened with it
    /// draws — cut to the glyphs drawn.
    Embed(Arc<dyn FontProvider>),
}

impl core::fmt::Debug for FontEmbedding {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // A provider is the host's and has no `Debug` of its own to ask.
        match self {
            FontEmbedding::Leave => f.write_str("Leave"),
            FontEmbedding::Embed(_) => f.write_str("Embed(..)"),
        }
    }
}

/// What the embedding pass did, reported by [`crate::write::save`] as
/// [`crate::write::Saved::embedded`].
///
/// **Not an error list.** A document whose every font embeds its program, or
/// is a Type 3 font, reports nothing at all; one whose fonts a provider has
/// no face for reports each of them in [`EmbedReport::left`], and is right to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbedReport {
    /// Every font embedded, in object order.
    pub embedded: Vec<EmbeddedFont>,
    /// Every font left unembedded, and why: those with an object in object
    /// order, then those written directly into a resource dictionary.
    pub left: Vec<LeftFont>,
    /// What the pass met in the fonts it embedded and did not stop for, by
    /// font in object order and then by code (ruling 10).
    pub warnings: Vec<EmbedWarning>,
}

/// What the pass met in a font it embedded, worth saying and no reason to
/// leave the font.
#[derive(Clone, Debug, PartialEq)]
pub enum EmbedWarning {
    /// A drawn code whose glyph's advance in the face differs from the width
    /// the font lays the code out with by more than a thousandth of an em,
    /// the tolerance ISO 19005's metrics clause holds the two to: the face is
    /// not metric-compatible with the font it stands in for. The pen moved
    /// by the dictionary's width before and moves by it after, so the page
    /// draws as it did; the embedded program's advance is set to the width
    /// (`docs/design/font-embedding.md`).
    WidthMismatch {
        /// The font dictionary.
        font: ObjRef,
        /// The code.
        code: u32,
        /// The width the font lays the code out with, in thousandths of an
        /// em: its `/Widths` entry, or, for the standard 14, the published
        /// metric the pass writes there.
        dictionary: f64,
        /// The advance of the code's glyph in the face, in thousandths of an
        /// em.
        program: f64,
    },
}

impl core::fmt::Display for EmbedWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EmbedWarning::WidthMismatch {
                font,
                code,
                dictionary,
                program,
            } => write!(
                f,
                "font {} {}: code {code} is {dictionary} wide in the dictionary and \
                 {program} in the face",
                font.num, font.gen
            ),
        }
    }
}

/// One font the pass embedded a face for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EmbeddedFont {
    /// The font dictionary.
    pub font: ObjRef,
    /// `/BaseFont` as the pass wrote it: 9.9.2's subset tag and the face's
    /// PostScript name.
    pub base_font: String,
    /// The stream the embedded program is in.
    pub program: ObjRef,
    /// How the program is carried (Table 124 of ISO 32000-2).
    pub format: EmbeddedFormat,
    /// How many bytes the provider's face carried.
    pub face: usize,
    /// How many the embedded cut carries.
    pub bytes: usize,
    /// How many glyphs the drawn codes chose, before the cut's closure over
    /// composite components and `.notdef`.
    pub glyphs: usize,
}

/// How an embedded face is carried (9.9, ISO 32000-2 Table 124).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmbeddedFormat {
    /// A TrueType program, `/FontFile2`, under `/Subtype /TrueType`.
    TrueType,
    /// A CFF program, `/FontFile3 /Subtype /Type1C`, under `/Subtype /Type1`.
    Type1C,
}

/// One font the pass left unembedded, and why (ruling 10).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LeftFont {
    /// The font dictionary; `None` for one written directly into a resource
    /// dictionary, which has no object.
    pub font: Option<ObjRef>,
    /// `/BaseFont`, as written.
    pub base_font: String,
    /// Why it was left.
    pub reason: LeftReason,
}

impl core::fmt::Display for LeftFont {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "/{} left unembedded: {}", self.base_font, self.reason)
    }
}

/// Why a font the pages draw, and that embeds no program, was left so.
///
/// Closed, as [`UntouchedReason`] is: a caller matches every reason, and a
/// new one is a compile error where it is handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LeftReason {
    /// The provider answered `None` for it.
    Declined,
    /// The provider's answer is not a TrueType program, an `OpenType/CFF`
    /// face or a bare CFF program for a simple font that this build reads: a
    /// Type 1 program, a `CFF2` face, a WOFF file, a CID-keyed CFF answering
    /// a simple font, or bytes neither parser reads.
    Unreadable,
    /// The face's `OS/2` `fsType` asks for restricted-licence embedding or
    /// for bitmap embedding only (9.9.1).
    EmbeddingForbidden,
    /// The face's `OS/2` `fsType` forbids subsetting it, and the pass embeds
    /// a subset.
    SubsettingForbidden,
    /// The font is written directly into a resource dictionary, so there is
    /// no object for a rewrite to address.
    NotAnObject,
    /// A form field's `/DA` may draw it at any character it is ever given
    /// (12.7.3.3), which a cut face cannot.
    FieldResource,
    /// A Type 3 font's own `/Resources` names it, and the interpreter runs a
    /// glyph procedure in the enclosing scope, so the codes shown through it
    /// are credited to another font (9.6.5).
    Type3Resource,
    /// More distinct codes were shown through it than the pass holds
    /// (`subset::MAX_DRAWN_CODES`).
    TooManyCodes,
    /// The face's reading reaches no glyph but `.notdef` for this code, or
    /// reaches one only by the glyph number the code gives — a face with
    /// nothing to be read by: embedding would make the missing glyph
    /// permanent, where a reader with a better face draws it.
    NoGlyph {
        /// The first such code.
        code: u32,
    },
    /// Read back as written, the font would draw this code with another
    /// glyph, lay it out at another width or read it as other text than the
    /// document drew and read it: the pass put everything back.
    DrawsOtherwise {
        /// The first such code.
        code: u32,
    },
    /// The cut refused the face: `tinker_pdf_font::subset_with` would not
    /// rebuild it to the drawn glyphs with their advances set — a CFF
    /// charstring whose width must be set and is stated behind a subroutine
    /// call among the reasons.
    NotSubsettable,
    /// A composite font: the pass embeds simple fonts.
    Composite,
}

impl core::fmt::Display for LeftReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LeftReason::Declined => f.write_str("the provider has no face for it"),
            LeftReason::Unreadable => {
                f.write_str("the provider's face is not a TrueType or CFF program for it")
            }
            LeftReason::EmbeddingForbidden => {
                f.write_str("the face's licence forbids embedding it")
            }
            LeftReason::SubsettingForbidden => {
                f.write_str("the face's licence forbids embedding a subset of it")
            }
            LeftReason::NotAnObject => {
                f.write_str("it is written directly into a resource dictionary")
            }
            LeftReason::FieldResource => {
                f.write_str("a form field's /DA may draw it at any character (12.7.3.3)")
            }
            LeftReason::Type3Resource => {
                f.write_str("a Type 3 font's own /Resources names it (9.6.5)")
            }
            LeftReason::TooManyCodes => {
                f.write_str("more codes are drawn through it than are held")
            }
            LeftReason::NoGlyph { code } => write!(f, "the face has no glyph for code {code}"),
            LeftReason::DrawsOtherwise { code } => {
                write!(f, "embedded, it would draw or read code {code} otherwise")
            }
            LeftReason::NotSubsettable => {
                f.write_str("the face could not be cut to the drawn glyphs")
            }
            LeftReason::Composite => f.write_str("it is a composite font"),
        }
    }
}

/// What a candidate font is, by its dictionary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Type 1, its multiple-master variant, the standard 14, TrueType, or a
    /// simple font whose `/Subtype` is missing or another name.
    Simple,
    /// Type 0, whose descendant embeds nothing.
    Composite,
}

/// The form an answer is embedded in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    TrueType,
    Cff,
}

/// The provider's answer, read: what is embedded from it and how.
struct Answer<'a> {
    format: Format,
    /// The bytes the cut is taken from: the whole sfnt for a TrueType
    /// program, the CFF itself — bare, or an `OTTO` face's `CFF ` table.
    program: &'a [u8],
}

/// `glyf`.
const GLYF: u32 = u32::from_be_bytes(*b"glyf");
/// `loca`.
const LOCA: u32 = u32::from_be_bytes(*b"loca");
/// `CFF `.
const CFF: u32 = u32::from_be_bytes(*b"CFF ");

impl<'a> Answer<'a> {
    /// Reads a simple font's answer: a TrueType program, or a CFF that is
    /// not CID-keyed — 9.9 Table 124's Type1C is a Type 1-equivalent program,
    /// and a CID-keyed one answers no simple font.
    fn simple(face: &'a [u8]) -> Option<Answer<'a>> {
        if let Some(sfnt) = Sfnt::parse(face) {
            if sfnt.table(GLYF).is_some() && sfnt.table(LOCA).is_some() {
                return Some(Answer {
                    format: Format::TrueType,
                    program: face,
                });
            }
            if sfnt.table(GLYF).is_some() {
                return None;
            }
            let table = sfnt.table(CFF)?;
            return Cff::parse(table)
                .filter(|cff| !cff.is_cid())
                .map(|_| Answer {
                    format: Format::Cff,
                    program: table,
                });
        }
        Cff::parse(face)
            .filter(|cff| !cff.is_cid())
            .map(|_| Answer {
                format: Format::Cff,
                program: face,
            })
    }
}

/// Embeds a face for every font the pages draw that embeds no program, and
/// returns the report with the programs it wrote, which the font pass passes
/// over (`subset::apply_counting`): they are cut to what is drawn already.
///
/// Counted into `work` when the save is counting, and walking the pages on
/// the threads `parallelism` names, as the font pass walks them.
pub(crate) fn apply(
    editor: &mut DocumentEditor,
    provider: &Arc<dyn FontProvider>,
    work: Option<&Arc<Work>>,
    parallelism: Parallelism,
) -> (EmbedReport, BTreeSet<ObjRef>) {
    let mut report = EmbedReport::default();
    let mut written = BTreeSet::new();
    // A view that cannot be built — the editor's own update failing to
    // reopen, a defect rather than a property of the input — walks nothing,
    // and nothing is known to be drawn.
    let Ok(view) = editor.view() else {
        return (report, written);
    };
    let op = Operation::new(work.cloned());
    let usage = subset::collect(&view, &op, parallelism, true);
    // The faces a page drew with this provider: the bundled ones behind the
    // host's in a `bundled-fonts` build.
    let provider =
        fonts::effective(Some(Arc::clone(provider))).unwrap_or_else(|| Arc::clone(provider));

    for (&font, codes) in &usage.codes {
        // Whether the font is missing its program is the view's question: it
        // reads the document as the editor has it as a file, where a stream
        // the editor wrote is a stream, as the renderer reads it.
        let Ok(object) = view.get(font) else {
            continue;
        };
        let Some(seen) = object.as_dict() else {
            continue;
        };
        let Some(kind) = candidate(&*view, seen) else {
            continue;
        };
        let base_font = base_font_of(&*view, seen);
        // What is written is read through the editor, whose names every
        // object written here speaks: the view's are its own where it
        // reopened whole.
        let Some(Object::Dict(dict)) = editor.get(font) else {
            continue;
        };
        let early = match usage.unbounded.get(&font) {
            Some(UntouchedReason::FieldResource) => Some(LeftReason::FieldResource),
            Some(UntouchedReason::Type3Resource) => Some(LeftReason::Type3Resource),
            _ => None,
        }
        .or_else(|| {
            usage
                .overflow
                .contains(&font)
                .then_some(LeftReason::TooManyCodes)
        })
        .or_else(|| (kind == Kind::Composite).then_some(LeftReason::Composite));
        let outcome = match early {
            Some(reason) => Err(reason),
            None => {
                // The request as the renderer read it, through the view.
                let request = FontRequest::read(&view, seen);
                embed_simple(editor, font, &dict, codes, &request, &*provider)
            }
        };
        match outcome {
            Ok((embedded, warnings)) => {
                written.insert(embedded.program);
                report.embedded.push(embedded);
                report.warnings.extend(warnings);
            }
            Err(reason) => report.left.push(LeftFont {
                font: Some(font),
                base_font,
                reason,
            }),
        }
    }

    for (base_font, dict) in &usage.direct_shown {
        if candidate(&*view, dict).is_some() {
            report.left.push(LeftFont {
                font: None,
                base_font: String::from_utf8_lossy(base_font).into_owned(),
                reason: LeftReason::NotAnObject,
            });
        }
    }
    (report, written)
}

/// Whether a font is one the pass embeds for, and what kind: a font whose
/// dictionary — a composite font's descendant's — names no program, and that
/// is not a Type 3 font.
fn candidate<R: Resolve + ?Sized>(doc: &R, dict: &Dict) -> Option<Kind> {
    let subtype = dict
        .get_name(Name::SUBTYPE)
        .and_then(|name| doc.name_bytes(name));
    match subtype.as_deref() {
        Some(b"Type3") => None,
        Some(b"Type0") => {
            let descendants = doc.resolve_key(dict, Name::DESCENDANT_FONTS);
            let first = descendants.as_array()?.first()?;
            let descendant = doc.resolve(first);
            let descendant = descendant.as_dict()?;
            (!names_a_program(doc, descendant)).then_some(Kind::Composite)
        }
        _ => (!names_a_program(doc, dict)).then_some(Kind::Simple),
    }
}

/// Whether a font dictionary's descriptor names a `/FontFile*` stream — the
/// test ISO 19005's embedding rule makes (`pdfa/fonts.rs`): a key whose value
/// is a reference to a stream. A key naming nothing, or something that is not
/// a stream, embeds nothing, and the renderer draws such a font by a face.
///
/// Asked of a document read as a file — the editor's view — where a stream
/// is a stream: the editor hands a stream it wrote back as its dictionary.
fn names_a_program<R: Resolve + ?Sized>(doc: &R, holder: &Dict) -> bool {
    let descriptor = doc.resolve_key(holder, Name::FONT_DESCRIPTOR);
    let Some(descriptor) = descriptor.as_dict() else {
        return false;
    };
    [Name::FONT_FILE, Name::FONT_FILE2, Name::FONT_FILE3]
        .into_iter()
        .any(|key| {
            descriptor
                .get_ref(key)
                .and_then(|reference| doc.get(reference).ok())
                .is_some_and(|object| object.as_stream().is_some())
        })
}

/// `/BaseFont`, as written; empty where there is none.
fn base_font_of<R: Resolve + ?Sized>(doc: &R, dict: &Dict) -> String {
    dict.get_name(Name::BASE_FONT)
        .and_then(|name| doc.name_bytes(name))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// `fsType`'s verdict (9.9.1): a face whose low four bits ask for
/// restricted-licence embedding alone, or that asks for bitmap embedding
/// only, may not be embedded as outlines; one that forbids subsetting may
/// not be cut. A face that states nothing restricts nothing.
fn licence(fs_type: Option<u16>) -> Result<(), LeftReason> {
    let Some(bits) = fs_type else {
        return Ok(());
    };
    if bits & 0x000F == 0x0002 || bits & 0x0200 != 0 {
        return Err(LeftReason::EmbeddingForbidden);
    }
    if bits & 0x0100 != 0 {
        return Err(LeftReason::SubsettingForbidden);
    }
    Ok(())
}

/// Embeds the provider's face for one simple font, with the width
/// mismatches it met, or says why not.
fn embed_simple(
    editor: &mut DocumentEditor,
    font: ObjRef,
    dict: &Dict,
    codes: &BTreeSet<u32>,
    request: &FontRequest,
    provider: &dyn FontProvider,
) -> Result<(EmbeddedFont, Vec<EmbedWarning>), LeftReason> {
    let face = provider.substitute(request).ok_or(LeftReason::Declined)?;
    let answer = Answer::simple(&face).ok_or(LeftReason::Unreadable)?;
    let facts = tinker_pdf_font::face_facts(&face).unwrap_or_default();
    licence(facts.fs_type)?;

    // What the face drew for each code: the renderer's own reading of a
    // substituted face, handed the face as the renderer is.
    let original = cos_font::read_in(&*editor, dict);
    let sfnt = Sfnt::parse(&face);
    let cff = match &sfnt {
        Some(sfnt) if sfnt.table(GLYF).is_none() => sfnt.table(CFF).and_then(Cff::parse),
        Some(_) => None,
        None => Cff::parse(&face),
    };
    let mut chosen: BTreeMap<u32, u16> = BTreeMap::new();
    for &code in codes {
        let drew =
            resources::substitute_choice(&face, sfnt.as_ref(), cff.as_ref(), &original, code);
        if drew.route != Route::Stated || drew.glyph == 0 {
            return Err(LeftReason::NoGlyph { code });
        }
        chosen.insert(code, drew.glyph);
    }

    let glyphs: BTreeSet<u16> = chosen.values().copied().collect();
    let (warnings, advances) = widths(font, &original, answer.program, &chosen);
    let cut = tinker_pdf_font::subset_with(
        answer.program,
        &glyphs,
        &tinker_pdf_font::Reshape { advances },
    )
    .ok_or(LeftReason::NotSubsettable)?;

    let mut name = subset_tag(&cut);
    name.extend_from_slice(&stem(&facts, &base_font_of(&*editor, dict)));
    let format = answer.format;

    editor
        .transaction(|editor| {
            let program = editor.allocate();
            let mut stream = Dict::new();
            match format {
                // Table 125: `/Length1` is a TrueType program's decoded length.
                Format::TrueType => {
                    stream.insert(
                        editor.intern(b"Length1"),
                        Object::Int(i64::try_from(cut.len()).unwrap_or(i64::MAX)),
                    );
                }
                // Table 125: a `/FontFile3`'s `/Subtype` says what it is, and it
                // has no `/Length1` to `/Length3`.
                Format::Cff => {
                    stream.insert(Name::SUBTYPE, Object::Name(editor.intern(b"Type1C")));
                }
            }
            editor.put_stream(
                program,
                StreamData {
                    dict: stream,
                    data: cut.clone(),
                },
            );

            let base_font = editor.intern(&name);
            let (subtype, key) = match format {
                Format::TrueType => (b"TrueType".as_slice(), Name::FONT_FILE2),
                Format::Cff => (b"Type1".as_slice(), Name::FONT_FILE3),
            };
            let descriptor = editor.allocate();
            let written = descriptor_for(
                &*editor,
                dict,
                &facts,
                request,
                base_font,
                (key, program),
                original.is_symbolic(),
            );
            editor.put(descriptor, Object::Dict(written));

            let mut rewritten = dict.clone();
            rewritten.insert(Name::SUBTYPE, Object::Name(editor.intern(subtype)));
            rewritten.insert(Name::BASE_FONT, Object::Name(base_font));
            rewritten.insert(Name::FONT_DESCRIPTOR, Object::Ref(descriptor));
            // 9.6.2.2 lets the standard 14 omit `/Widths`; an embedded font may
            // not, and the widths are the ones the font was laid out with.
            if editor.resolve_key(dict, Name::WIDTHS).as_array().is_none() {
                if let (Some(&first), Some(&last)) = (codes.first(), codes.last()) {
                    let widths = (first..=last)
                        .map(|code| number(original.width_of(code).0))
                        .collect();
                    rewritten.insert(Name::FIRST_CHAR, Object::Int(i64::from(first)));
                    rewritten.insert(editor.intern(b"LastChar"), Object::Int(i64::from(last)));
                    rewritten.insert(Name::WIDTHS, Object::Array(widths));
                }
            }
            editor.put(font, Object::Dict(rewritten.clone()));

            let read = cos_font::read_in(&*editor, &rewritten);
            read_back(&read, &original, &cut, format, &chosen)?;

            Ok(EmbeddedFont {
                font,
                base_font: String::from_utf8_lossy(&name).into_owned(),
                program,
                format: match format {
                    Format::TrueType => EmbeddedFormat::TrueType,
                    Format::Cff => EmbeddedFormat::Type1C,
                },
                face: face.len(),
                bytes: cut.len(),
                glyphs: glyphs.len(),
            })
        })
        .map(|embedded| (embedded, warnings))
}

/// Each drawn code's width against its glyph's advance in `program`, as
/// ISO 19005's metrics rule reads both ([`ProgramMetrics`]): a warning for
/// each code past [`WIDTH_TOLERANCE`], and the advance each glyph is to take
/// in the program's own units — the width of the lowest drawn code that
/// chose it, where the face's advance is past the tolerance of that width.
/// A glyph two codes chose at two widths takes the lower code's; the other
/// stays a warning.
fn widths(
    font: ObjRef,
    original: &cos_font::Font,
    program: &[u8],
    chosen: &BTreeMap<u32, u16>,
) -> (Vec<EmbedWarning>, BTreeMap<u16, f64>) {
    let mut warnings = Vec::new();
    let mut advances = BTreeMap::new();
    let Some(metrics) = ProgramMetrics::read(program) else {
        return (warnings, advances);
    };
    let mut first: BTreeMap<u16, f64> = BTreeMap::new();
    for (&code, &glyph) in chosen {
        let width = original.width_of(code).0;
        let Some(advance) = metrics.advance(glyph) else {
            continue;
        };
        if (advance - width).abs() > WIDTH_TOLERANCE {
            warnings.push(EmbedWarning::WidthMismatch {
                font,
                code,
                dictionary: width,
                program: advance,
            });
        }
        first.entry(glyph).or_insert(width);
    }
    for (glyph, width) in first {
        let Some(advance) = metrics.advance(glyph) else {
            continue;
        };
        if (advance - width).abs() > WIDTH_TOLERANCE {
            if let Some(units) = metrics.units(glyph, width) {
                advances.insert(glyph, units);
            }
        }
    }
    (warnings, advances)
}

/// The difference between two widths, in thousandths of an em, that a
/// rewrite of `/Widths` may introduce: what a written number rounds to.
const WIDTH_EPSILON: f64 = 1e-3;

/// Asks every drawn code of the font as written what a reader of the new
/// file asks it, and compares the answers with what the document drew and
/// read before: the glyph the embedded program's mapping chooses — the
/// renderer's own functions for an embedded program — the width the code is
/// laid out with and the text it reads as.
fn read_back(
    read: &cos_font::Font,
    original: &cos_font::Font,
    program: &[u8],
    format: Format,
    chosen: &BTreeMap<u32, u16>,
) -> Result<(), LeftReason> {
    let sfnt = Sfnt::parse(program);
    let cff = Cff::parse(program);
    for (&code, &glyph) in chosen {
        let selected = match format {
            Format::TrueType => sfnt
                .as_ref()
                .and_then(|sfnt| resources::simple_glyph_by_cmap(sfnt, read, code)),
            Format::Cff => cff
                .as_ref()
                .and_then(|cff| resources::cff_glyph(cff, read, code)),
        };
        let width = (read.width_of(code).0 - original.width_of(code).0).abs();
        if selected != Some(glyph)
            || width > WIDTH_EPSILON
            || read.text_of(code) != original.text_of(code)
        {
            return Err(LeftReason::DrawsOtherwise { code });
        }
    }
    Ok(())
}

/// The name a subset is named after (9.9.2): the face's PostScript name,
/// else the document's `/BaseFont` with any subset tag taken off, else
/// `Embedded`, so that the tag always stands before a name.
fn stem(facts: &FaceFacts, base_font: &str) -> Vec<u8> {
    if let Some(name) = &facts.postscript_name {
        return name.as_bytes().to_vec();
    }
    let document = subset::strip_tag(base_font.as_bytes());
    if document.is_empty() {
        b"Embedded".to_vec()
    } else {
        document.to_vec()
    }
}

/// A number as 7.3.3 writes it: an integer where it is one, and no number
/// that is not finite, which 7.3.3 has no spelling for.
fn number(value: f64) -> Object {
    if !value.is_finite() {
        return Object::Int(0);
    }
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        Object::Int(value as i64)
    } else {
        Object::Real(value)
    }
}

/// A length in thousandths of an em, rounded to the integer a descriptor
/// conventionally states.
fn length(value: f64) -> Object {
    if value.is_finite() {
        Object::Int(value.round() as i64)
    } else {
        Object::Int(0)
    }
}

/// 9.8.2 Table 121's flags.
const FIXED_PITCH: i64 = 1;
const SERIF: i64 = 1 << 1;
const SYMBOLIC: i64 = 1 << 2;
const SCRIPT: i64 = 1 << 3;
const NONSYMBOLIC: i64 = 1 << 5;
const ITALIC: i64 = 1 << 6;

/// The new descriptor for a font: its own descriptor's entries copied where
/// it had one, then `/FontName`, `/Flags`, each metric key Table 120
/// requires that the copy lacks, `/XHeight`, and the program's key; and no
/// `/FontFile*`, `/CharSet` or `/CIDSet` the copy carried, each of which
/// describes another program.
///
/// The flags are the document's where it had a descriptor; otherwise fixed
/// pitch, serif, script and italic as the face says, else as the request
/// says. The symbolic flag is the font's own (`symbolic`), so the codes are
/// read in the order they were, and the nonsymbolic flag its inverse
/// (9.8.2).
fn descriptor_for(
    editor: &DocumentEditor,
    holder: &Dict,
    facts: &FaceFacts,
    request: &FontRequest,
    font_name: Name,
    (key, program): (Name, ObjRef),
    symbolic: bool,
) -> Dict {
    let existing = editor.resolve_key(holder, Name::FONT_DESCRIPTOR);
    let existing = existing.as_dict();
    let stale = [
        Name::FONT_FILE,
        Name::FONT_FILE2,
        Name::FONT_FILE3,
        editor.intern(b"CharSet"),
        editor.intern(b"CIDSet"),
    ];
    let mut out = Dict::new();
    for (name, value) in existing.into_iter().flat_map(Dict::iter) {
        if !stale.contains(name) {
            out.insert(*name, value.clone());
        }
    }
    out.insert(Name::TYPE, Object::Name(editor.intern(b"FontDescriptor")));
    out.insert(editor.intern(b"FontName"), Object::Name(font_name));

    let stated = existing.and_then(|d| editor.resolve_key(d, Name::FLAGS).as_int());
    let base = match stated {
        Some(flags) => flags & !(SYMBOLIC | NONSYMBOLIC),
        None => {
            let class = facts.family_class.map(|class| class >> 8);
            let mut flags = 0;
            if facts.fixed_pitch.unwrap_or(request.fixed_pitch) {
                flags |= FIXED_PITCH;
            }
            // `sFamilyClass` 1 to 5 and 7 are the serif classes, 10 the
            // scripts.
            if class.map_or(request.serif, |class| matches!(class, 1..=5 | 7)) {
                flags |= SERIF;
            }
            if class == Some(10) {
                flags |= SCRIPT;
            }
            let italic = match (facts.italic, facts.italic_angle) {
                (None, None) => request.italic,
                (italic, angle) => italic == Some(true) || angle.is_some_and(|a| a != 0.0),
            };
            if italic {
                flags |= ITALIC;
            }
            flags
        }
    };
    let flags = base | if symbolic { SYMBOLIC } else { NONSYMBOLIC };
    out.insert(Name::FLAGS, Object::Int(flags));

    let bbox = facts.bbox.unwrap_or([0.0; 4]);
    let ascent = facts.ascent.unwrap_or(bbox[3]);
    let descent = facts.descent.unwrap_or(bbox[1]);
    let complete: [(&[u8], Option<Object>); 7] = [
        (b"FontBBox", Some(Object::Array(bbox.map(length).to_vec()))),
        (
            b"ItalicAngle",
            Some(number(
                (facts.italic_angle.unwrap_or(0.0) * 100.0).round() / 100.0,
            )),
        ),
        (b"Ascent", Some(length(ascent))),
        (b"Descent", Some(length(descent))),
        (
            b"CapHeight",
            Some(length(facts.cap_height.unwrap_or(ascent))),
        ),
        (b"StemV", Some(length(facts.stem_v.unwrap_or(0.0)))),
        (b"XHeight", facts.x_height.map(length)),
    ];
    for (name, value) in complete {
        let name = editor.intern(name);
        if let Some(value) = value.filter(|_| !out.contains_key(name)) {
            out.insert(name, value);
        }
    }
    out.insert(key, Object::Ref(program));
    out
}

#[cfg(test)]
mod tests;
