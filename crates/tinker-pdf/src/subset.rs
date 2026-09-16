//! Font subsetting on rewrite: the embedded programs of a document that
//! already exists, cut down to the glyphs that document still draws.
//!
//! The build side has subsetted since it could embed anything — it knows every
//! glyph it placed, because it placed them. A **rewrite** knows nothing of the
//! kind: the text was written by somebody else, and until this existed a
//! rewrite copied every font program through untouched however little of it
//! the page used.
//!
//! That is two different costs, and the second is the one that matters:
//!
//! - **Size.** A page of Latin text over a CJK face carries tens of megabytes
//!   of glyphs nobody looks at. Measured on the vendored Liberation Serif
//!   Regular, 393 576 bytes: ten characters of it is **29 376** bytes after
//!   this pass, and a page drawing none of it at all is 26 428 — which is not
//!   zero, and should not be, since `cmap`, `hmtx`, `OS/2` and the three
//!   hinting tables are copied through for the readers that interpret them.
//! - **Disclosure.** [`crate::redact`] removes the *text*. It does not remove
//!   the **glyphs**, which sit in the font program exactly as they did before,
//!   and a redacted document whose face is a subset of the characters that
//!   were on the page still names them: a program with `glyf` entries for
//!   nothing but `J`, `o`, `h`, `n`, `S`, `m`, `i`, `t` and `h` says what the
//!   redaction was for. This is the reason the row exists, and it is why a
//!   redaction that must not disclose has to run this afterwards — see
//!   [`apply`].
//!
//! # Which glyphs count as used
//!
//! Every wrong answer here has the same shape: a glyph that turns out to be
//! used and is not in the program any more renders as a **blank or a wrong
//! glyph**, not as an error, and nothing in the file says so. So the rule is
//! to include, and to refuse the whole font wherever inclusion cannot be
//! bounded.
//!
//! Counted as used:
//!
//! | Where the glyph is shown | Why it is counted |
//! | --- | --- |
//! | A page's own content stream | the obvious case |
//! | A form XObject the page draws, at any depth | 8.10: a form's content is the page's content |
//! | A Type 3 glyph procedure entered because its own glyph was shown | 9.6.5 runs the procedure as a content stream, and the text *inside* it is drawn with a font of its own |
//! | **Every** appearance stream under an annotation's `/AP` — `/N`, `/D` and `/R`, and every state of each, whatever `/AS` currently selects | 12.5.5: a viewer swaps states on its own. The checkbox that is off today is on tomorrow with no edit to the file, and a subset cut to the state that happened to be selected loses the tick |
//! | An appearance of an annotation whose `/F` says hidden | the flag is a viewer's instruction, not a statement that the stream is dead; clearing it is one bit |
//!
//! Not counted as used, and **not subsetted either** — the font passes through
//! whole, named in [`SubsetReport::untouched`]:
//!
//! - **A font named by a form field's `/DA`**, reached through the AcroForm
//!   `/DR` (12.7.3.3). A `/DA` is not a use, it is a *promise about future
//!   uses*: the field's value can be retyped, and the appearance a viewer
//!   generates for it may draw any character the font has. There is no set to
//!   bound, so there is no subset to take. See
//!   [`UntouchedReason::FieldResource`].
//! - **A font whose scope no walk reached**, including one named only by a
//!   form XObject nothing draws. Its resource dictionary was never
//!   interpreted, so "no glyphs were shown through it" is ignorance rather
//!   than a measurement. See [`UntouchedReason::ScopeNotWalked`].
//! - **A font any of whose shown codes resolved only by 9.6.6.4's closing
//!   guess** — read the code as the glyph index. See
//!   [`crate::resources::Selection`], which carries the argument.
//! - **A font named in a Type 3 font's own `/Resources`**. 9.6.5 lets a Type 3
//!   font carry resources for its procedures; this engine's interpreter runs a
//!   procedure in the *enclosing* scope instead, so a `/F1` inside a procedure
//!   is attributed to the enclosing `/F1` and the Type 3 font's own `/F1` is
//!   credited with nothing. The enclosing font merely gains glyphs it does not
//!   need, which is safe; the Type 3 font's own would lose every glyph it
//!   does, which is not. See [`UntouchedReason::Type3Resource`].
//!
//! # When subsetting cannot be done
//!
//! Ruling 2: degrade, do not fail. A program the subsetter refuses — a Type 1
//! program, a CFF whose charstrings cannot be renumbered without guessing,
//! bytes that are neither — is written through **exactly as it arrived**,
//! because a document that renders is worth more than one that is small. A
//! subset that came out no smaller than the face is refused for the same
//! reason the builder refuses it: the whole face is then both smaller and the
//! one the producer tested.
//!
//! Ruling 10: leniency names what it touched. Every font left whole is in
//! [`SubsetReport::untouched`] with its object, its `/BaseFont`, its size and
//! its reason. A caller redacting a document reads that list to find out
//! whether the disclosure above is still in the file.
//!
//! # How the encoding survives
//!
//! This is where a change of this shape usually goes wrong, and the failure
//! renders as the wrong glyphs rather than as an error.
//!
//! It survives because **[`tinker_pdf_font::subset`] does not renumber**. A
//! dropped glyph becomes a zero-length `loca` entry, not a gap the later
//! glyphs shuffle into, so every glyph index in the file still means what it
//! meant. That makes `/FirstChar`, `/LastChar`, `/Widths`, `/W`, `/DW`,
//! `/Encoding`, `/Differences`, `/CIDToGIDMap` and `/ToUnicode` correct after
//! the cut **because they are unchanged** — and this module changes none of
//! them. `font_dictionaries_are_untouched_except_for_the_subset_tag` pins
//! that: the only keys that move are the three that name the font.
//!
//! Three things do have to move, and all three are about the *name*:
//!
//! - `/BaseFont` on the font dictionary gains 9.6.4's six-letter tag and plus
//!   sign, from [`tinker_pdf_cos::subset_tag`] — the same function the builder
//!   names its subsets with, so a document built here and the same document
//!   rewritten here cannot disagree about what a subset of one face is called.
//!   A tag already there is **replaced**, not stacked: the old one named a
//!   different set of glyphs.
//! - `/BaseFont` on a composite font's descendant, which 9.7.6.2 requires to
//!   be the same name.
//! - `/FontName` in the descriptor, which 9.8.1 requires to be the same name.
//!
//! And one thing about the stream: `/Length1` is the *decoded* length of a
//! `/FontFile2` program (Table 126), so it is rewritten to the subset's
//! length. A `/FontFile3` has no `/Length1` — its `/Subtype` is what says what
//! the bytes are — so a stale one is dropped rather than carried.
//!
//! # One program, two font dictionaries
//!
//! Two font dictionaries may point at the same `/FontFile2`. Subsetting it for
//! one of them would take the other's glyphs away, so the unit of work here is
//! the **program stream**, not the font dictionary: every font in the document
//! that names a given stream is found first, their glyph sets are unioned, and
//! the stream is left whole if any one of those fonts is one this module will
//! not bound. That sweep is also what makes [`UntouchedReason::ScopeNotWalked`]
//! sayable at all, since it is the only place the document's fonts are
//! enumerated rather than reached.
//!
//! # The recording device, and the one thing it did not give this walk
//!
//! [`tinker_pdf_content::record`] was promoted out of the interpreter's test
//! module as a prerequisite, and its own documentation names six consumers it
//! was promoted for. This is the first of them to land, so the fit is reported
//! rather than assumed.
//!
//! It fits. The walk is [`interpret`] once into a [`RecordingDevice`] and one
//! pass over [`RecordingDevice::events`]; no second interpreter, no device of
//! its own, nothing derived.
//!
//! What did **not** fit is [`Capture::GLYPHS`], the preset that module
//! introduced *for this consumer* — "a `(font_id, code)` pair per glyph, with
//! no state copy and no path, image or bracketing event recorded at all". The
//! bracketing events are exactly what this cannot do without.
//! [`tinker_pdf_content::Glyph::font_id`] is the interned **resource name**,
//! and a resource name is scope-relative: a form XObject with its own
//! `/Resources` may bind `/F1` to a different font from the page that drew it,
//! and the two then carry the same id. Without [`Event::BeginForm`] and
//! [`Event::EndForm`] there is no way to say which `/F1` a glyph meant, and
//! the glyphs of one font would be credited to another — which drops the
//! glyphs the other font needs. So this walk runs under `text` **and**
//! `structure`, and pays for the form brackets.
//!
//! That is a finding about `Capture::GLYPHS` rather than about this module:
//! the preset is right for the other glyph-only consumer that module names,
//! inferred reading order, which wants glyphs in document order and never asks
//! which dictionary a font came from. It is wrong for this one, and the name
//! does not say so. It is left as it is here — renaming a public constant is
//! not this row's change — and the divergence is written down so that the next
//! consumer reading "what the glyph-usage walk that subsetting wants needs"
//! knows it is one preset short.
//!
//! # What is adjudicated, and what is only self-consistency
//!
//! Stated plainly because the distinction is the point (ruling 13):
//!
//! - The **font program** every test cuts down is third-party: Liberation
//!   Serif Regular, the vendored face `bundled-fonts` embeds, read from
//!   `crates/tinker-pdf-font/data/liberation/`. Which of its glyphs are
//!   composite, what they are built from, and where `loca` puts them are
//!   facts about that face and not about anything written here — which is why
//!   `the_components_of_a_shown_composite_glyph_survive` asks the face which
//!   glyph `é` is and which glyphs it is made of rather than asserting a
//!   number. A fixture face written here could be made to pass a subsetter
//!   that drops components; this one cannot.
//! - The **document** around it is this engine's own writer, and that is only
//!   a container: what is being tested is the cut, not the file format. The
//!   claim a self-built document cannot support — "a real producer's encoding
//!   survives this" — is not made here. The corpus-wide version of it belongs
//!   with `crates/tinker-pdf/tests/cff_subset_census.rs`, which already walks
//!   every embedded program in the fetched corpora, and this row does not add
//!   it.
//! - The **render-identical** assertion compares this engine's raster of the
//!   original against this engine's raster of the rewrite. That is
//!   self-consistency, and it is worth having for exactly one thing: it is
//!   sensitive to a glyph that went missing, which is the failure this module
//!   can cause. It says nothing about whether either raster is right.
//! - The **disclosure** assertion — that a removed character's outline is not
//!   in the output — is a statement about bytes in a file, read back through
//!   the program's own `loca` and `glyf`. Measured: a two-line page over the
//!   whole face is 395 534 bytes with the redaction alone and **30 301** with
//!   this pass after it, the program itself 393 576 down to 28 332, and the
//!   six redacted letters' outlines are not among what is left.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use tinker_pdf_content::record::{Capture, Event, RecordingDevice};
use tinker_pdf_content::{interpret, FontSource, Form, Matrix};
use tinker_pdf_cos::{
    font as cos_font, pages as cos_pages, subset_tag, CosDocument, Dict, DocumentEditor, Name,
    ObjRef, Object, StreamData,
};

use crate::resources::PageResources;

/// How many objects the font sweep will look at.
///
/// A document whose cross-reference table claims millions of objects is a
/// document this pass must still return from (ruling 1). Reached only by a
/// file that is lying about its size, since the sweep is one dictionary read
/// per entry.
const MAX_SWEPT_OBJECTS: usize = 1 << 21;

/// Why a font's program was written through whole (ruling 10).
///
/// Not [`tinker_pdf_cos::SubsetRefusal`], and the difference is the point:
/// that enum is the *builder's*, and a builder meets only encodings it wrote
/// itself, so its three reasons are all about the program's bytes. A rewrite
/// meets encodings somebody else wrote, and most of the reasons below are
/// about the document rather than the face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UntouchedReason {
    /// [`tinker_pdf_font::subset`] would not rebuild the program: a Type 1
    /// program, a CFF whose charstrings cannot be renumbered without guessing,
    /// or bytes that are neither an sfnt nor a CFF.
    ProgramNotRebuildable,
    /// The subset came out no smaller than the face, so the face — which is
    /// both smaller and the one the producer tested — is what goes in.
    SubsetNotSmaller,
    /// Some code shown through this font resolved to a glyph only by 9.6.6.4's
    /// closing guess, so which glyphs the file needs is not something this
    /// build can state.
    CodeNotMapped,
    /// A form field's `/DA` may draw this font at any character the field is
    /// ever given (12.7.3.3), so there is no set of glyphs to bound.
    FieldResource,
    /// No walked resource dictionary named this font, so nothing was measured
    /// about it — a font reached only from a form XObject nothing draws, or
    /// from a scope this pass never opened.
    ScopeNotWalked,
    /// A Type 3 font's own `/Resources` names this font (9.6.5), and this
    /// engine's interpreter runs a glyph procedure in the enclosing scope, so
    /// glyphs shown through it would be credited to the wrong font.
    Type3Resource,
    /// The font is written directly into a resource dictionary rather than by
    /// reference, so there is no object for a rewrite to address.
    NotAnObject,
}

impl core::fmt::Display for UntouchedReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            UntouchedReason::ProgramNotRebuildable => "the font program cannot be rebuilt",
            UntouchedReason::SubsetNotSmaller => "the subset is no smaller than the face",
            UntouchedReason::CodeNotMapped => {
                "a code shown through it selects a glyph only by guess (9.6.6.4)"
            }
            UntouchedReason::FieldResource => {
                "a form field's /DA may draw it at any character (12.7.3.3)"
            }
            UntouchedReason::ScopeNotWalked => "no walked resource dictionary names it",
            UntouchedReason::Type3Resource => "a Type 3 font's own /Resources names it (9.6.5)",
            UntouchedReason::NotAnObject => "it is written directly into a resource dictionary",
        })
    }
}

/// One font program written through whole, and why (ruling 10).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Untouched {
    /// The stream the program is in.
    pub program: ObjRef,
    /// The `/BaseFont` of the first font dictionary that names it, as written.
    pub base_font: String,
    /// How many bytes the program carries, which is the size a subset would
    /// have been measured against.
    pub bytes: usize,
    /// Why.
    pub reason: UntouchedReason,
}

impl core::fmt::Display for Untouched {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "/{} left whole ({} bytes): {}",
            self.base_font, self.bytes, self.reason
        )
    }
}

/// One font program that was cut down.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Subsetted {
    /// The stream the program is in.
    pub program: ObjRef,
    /// The `/BaseFont` it is now written with, subset tag included.
    pub base_font: String,
    /// How many bytes the program carried before.
    pub before: usize,
    /// How many it carries now.
    pub after: usize,
    /// How many glyphs were asked for, before the subsetter's own closure over
    /// composite components and `.notdef`.
    pub glyphs: usize,
}

/// What a subsetting pass did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubsetReport {
    /// Every program cut down, in object order.
    pub subsetted: Vec<Subsetted>,
    /// Every program written through whole, in object order (ruling 10).
    ///
    /// **Not an error list.** A document of standard-14 fonts embeds nothing
    /// and reports nothing here; a document whose every face is already a
    /// tight subset reports every one of them, and is right to.
    pub untouched: Vec<Untouched>,
}

impl SubsetReport {
    /// How many bytes of font program the document carried before.
    #[must_use]
    pub fn bytes_before(&self) -> usize {
        self.subsetted.iter().map(|s| s.before).sum::<usize>()
            + self.untouched.iter().map(|u| u.bytes).sum::<usize>()
    }

    /// How many it carries now.
    #[must_use]
    pub fn bytes_after(&self) -> usize {
        self.subsetted.iter().map(|s| s.after).sum::<usize>()
            + self.untouched.iter().map(|u| u.bytes).sum::<usize>()
    }
}

/// Cuts every embedded font program down to the glyphs the document draws.
///
/// Whole-document, and it has to be: a font used on page two must keep page
/// two's glyphs however thoroughly page one was redacted, so there is no
/// per-page form of this operation that is not wrong.
///
/// **Run it after every other edit.** It reads the content streams as the
/// editor now has them — [`DocumentEditor::stream_bytes`], not the file — so a
/// redaction applied first is a redaction this sees, and the glyphs it removed
/// are glyphs this drops. Run the other way round, it would keep exactly what
/// the redaction was for.
///
/// **Save with [`tinker_pdf_cos::WriteMode::Rewrite`]** if the removal has to
/// be real. An incremental save appends, leaving the original program's bytes
/// in the file where anyone scanning it finds them — the same caveat
/// [`crate::redact`] carries, for the same reason.
pub fn apply(editor: &mut DocumentEditor) -> SubsetReport {
    let doc = editor.shared_document();
    let usage = collect(editor, &doc);
    rewrite(editor, &doc, &usage)
}

// ---------------------------------------------------------------------------
// The walk.
// ---------------------------------------------------------------------------

/// What the walk learned about one document.
#[derive(Default)]
struct Usage {
    /// The glyphs shown through each font object.
    glyphs: BTreeMap<ObjRef, BTreeSet<u16>>,
    /// Every font object a walked resource dictionary named.
    seen: BTreeSet<ObjRef>,
    /// Fonts this pass will not bound, and why.
    unbounded: BTreeMap<ObjRef, UntouchedReason>,
    /// Programs embedded by a font written **directly** into a resource
    /// dictionary, against the `/BaseFont` of the first such font that named
    /// them.
    ///
    /// Kept separately from everything above because these have no object to
    /// be keyed by, which is the whole reason they cannot be cut down. The
    /// sweep does not find them either — it walks objects — so without this a
    /// program only a direct font embeds would be left whole and **not
    /// reported**, and one that a direct font and an indirect font share would
    /// be cut to the indirect font's glyphs and lose the direct font's.
    direct: BTreeMap<ObjRef, Vec<u8>>,
}

impl Usage {
    fn refuse(&mut self, font: ObjRef, reason: UntouchedReason) {
        self.unbounded.entry(font).or_insert(reason);
    }
}

/// A scope of the walk: one resource dictionary, with the editor's current
/// content substituted for any stream it has already rewritten.
///
/// The one thing this adds to [`PageResources`] is that seam. Everything else
/// delegates, because a second answer to "what does this code decode to" is a
/// second engine, and the glyphs kept have to be the glyphs *this* engine
/// draws.
struct Walk<'a> {
    inner: Arc<PageResources>,
    editor: &'a DocumentEditor,
    /// The object number of every form XObject the interpreter entered.
    ///
    /// Recorded here rather than derived from the event log because
    /// [`Event::BeginForm`] carries the resource *name*, and resolving each
    /// name again afterwards would decode every form's content a second time.
    entered: Arc<Mutex<HashSet<u32>>>,
}

impl<'a> Walk<'a> {
    fn new(inner: Arc<PageResources>, editor: &'a DocumentEditor) -> Arc<Walk<'a>> {
        Arc::new(Walk {
            inner,
            editor,
            entered: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    fn nested(&self, inner: Arc<PageResources>) -> Arc<Walk<'a>> {
        Arc::new(Walk {
            inner,
            editor: self.editor,
            entered: Arc::clone(&self.entered),
        })
    }
}

impl FontSource for Walk<'_> {
    fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
        self.inner.decode(font, bytes)
    }

    fn is_vertical(&self, font: &[u8]) -> bool {
        self.inner.is_vertical(font)
    }

    fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64) {
        self.inner.vertical_metrics(font, code)
    }

    fn font_id(&self, font: &[u8]) -> u64 {
        self.inner.font_id(font)
    }

    fn type3_glyph(&self, font: &[u8], code: u32) -> Option<(Vec<u8>, Matrix)> {
        self.inner.type3_glyph(font, code)
    }

    fn form(&self, name: &[u8]) -> Option<Form> {
        let mut form = self.inner.form(name)?;
        let reference = ObjRef::new(
            u32::try_from(form.stream >> 16).unwrap_or(0),
            u16::try_from(form.stream & 0xffff).unwrap_or(0),
        );
        if let Ok(mut entered) = self.entered.lock() {
            entered.insert(reference.num);
        }
        // The editor's own rewrite of this form, when it has one. Without
        // this the walk reads the form's *original* text out of the file and
        // credits the font with every glyph a redaction just removed.
        if let Some(bytes) = self.editor.stream_bytes(reference) {
            form.content = bytes;
        }
        Some(form)
    }

    fn form_scope(&self, name: &[u8]) -> Option<Arc<Self>> {
        let inner = FontSource::form_scope(&*self.inner, name)?;
        Some(self.nested(inner))
    }
}

/// Walks every content stream the document draws and records what it showed.
fn collect(editor: &DocumentEditor, doc: &Arc<CosDocument>) -> Usage {
    let mut usage = Usage::default();

    for page in cos_pages::collect(doc) {
        let resources = Arc::new(PageResources::new(doc, &page, None));
        let content = page_content(editor, doc, page.reference);
        walk(&mut usage, editor, doc, &content, resources);

        for appearance in appearances_of(editor, doc, page.reference) {
            let Some(content) = editor.stream_bytes(appearance.stream) else {
                continue;
            };
            let resources = Arc::new(PageResources::from_dict(doc, appearance.resources, None));
            walk(&mut usage, editor, doc, &content, resources);
        }
    }

    // 12.7.3.3: a field's `/DA` names a font from the AcroForm `/DR`, and what
    // it will be asked to draw is whatever the field is next given.
    for font in default_resource_fonts(editor, doc) {
        usage.refuse(font, UntouchedReason::FieldResource);
    }

    usage
}

/// Interprets one content stream and folds what it showed into `usage`.
fn walk(
    usage: &mut Usage,
    editor: &DocumentEditor,
    doc: &CosDocument,
    content: &[u8],
    resources: Arc<PageResources>,
) {
    let root = Walk::new(resources, editor);

    // Glyphs and the form brackets that say which scope a glyph's font id is
    // relative to — see this module's note on `Capture::GLYPHS`, which is not
    // enough on its own. Nothing else is recorded: no state copies, no paths,
    // no images.
    let mut device = RecordingDevice::with_capture(Capture {
        text: true,
        structure: true,
        ..Capture::NONE
    });
    interpret(content, Matrix::IDENTITY, &mut device, &*root);

    let mut stack: Vec<Arc<Walk<'_>>> = vec![Arc::clone(&root)];
    note_scope(usage, doc, &root.inner);

    for event in device.events() {
        match event {
            Event::BeginForm { name, entered, .. } => {
                if !*entered {
                    continue;
                }
                let current = stack.last().cloned().unwrap_or_else(|| Arc::clone(&root));
                // 8.10.1: a form without its own `/Resources` keeps the scope
                // that invoked it, which is what `form_scope` returning
                // `None` means and what the interpreter does with it.
                let next = match FontSource::form_scope(&*current, name) {
                    Some(scope) => {
                        note_scope(usage, doc, &scope.inner);
                        scope
                    }
                    None => current,
                };
                stack.push(next);
            }
            Event::EndForm { .. } => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            Event::ShowGlyph { glyph, .. } => {
                let Some(scope) = stack.last() else {
                    continue;
                };
                show(usage, &scope.inner, glyph.font_id, glyph.code);
            }
            _ => {}
        }
    }
}

/// Records that a scope was walked, and what it named.
fn note_scope(usage: &mut Usage, doc: &CosDocument, scope: &PageResources) {
    for font in scope.font_objects() {
        usage.seen.insert(font);
        // 9.6.5: a Type 3 font may carry its own `/Resources`, which this
        // engine's interpreter does not enter a glyph procedure under. The
        // fonts in it would be credited with nothing while the enclosing
        // scope's fonts of the same name were credited with their glyphs.
        for inner in type3_resource_fonts(doc, font) {
            usage.refuse(inner, UntouchedReason::Type3Resource);
        }
    }
    // A font with no object of its own. It cannot be keyed, so its program
    // cannot be cut — and has to be *said*, because a caller reading the
    // report to find out whether a disclosure is still in the file would
    // otherwise be told nothing at all about it.
    for dict in scope.direct_fonts() {
        let Some(program) = embedded_program(doc, &dict) else {
            continue;
        };
        let base_font = dict
            .get_name(doc.intern(b"BaseFont"))
            .and_then(|n| doc.name_bytes(n))
            .map(|b| b.to_vec())
            .unwrap_or_default();
        usage.direct.entry(program).or_insert(base_font);
    }
}

/// The program stream a font dictionary embeds, whatever shape it is (9.7.6,
/// 9.9 Table 126).
///
/// The same three-step walk [`sweep`] makes — a Type 0 font's descendant, then
/// the descriptor, then whichever `/FontFile*` it carries — over a dictionary
/// rather than over an object, because a font written directly into a resource
/// dictionary is not an object and `sweep` never sees it.
fn embedded_program(doc: &CosDocument, dict: &Dict) -> Option<ObjRef> {
    let subtype = dict
        .get_name(doc.intern(b"Subtype"))
        .and_then(|n| doc.name_bytes(n));
    let holder = if subtype.as_deref() == Some(b"Type0".as_slice()) {
        let array = doc.resolve_key(dict, doc.intern(b"DescendantFonts"));
        let first = array.as_array().and_then(|items| items.first().cloned())?;
        doc.resolve(&first).as_dict().cloned()?
    } else {
        dict.clone()
    };
    let descriptor = doc.resolve_key(&holder, doc.intern(b"FontDescriptor"));
    let descriptor = descriptor.as_dict()?;
    program_key(doc, descriptor).and_then(|key| {
        let name = match key {
            cos_font::ProgramKey::FontFile => b"FontFile".as_slice(),
            cos_font::ProgramKey::FontFile2 => b"FontFile2",
            cos_font::ProgramKey::FontFile3 => b"FontFile3",
        };
        descriptor.get_ref(doc.intern(name))
    })
}

/// Records one glyph against the font that drew it.
fn show(usage: &mut Usage, scope: &PageResources, font_id: u64, code: u32) {
    let Some(object) = scope.font_object(font_id) else {
        // A font written directly into the resource dictionary. There is no
        // object to key a glyph against, and `sweep` — which walks objects —
        // never finds it either, so it can only be left whole.
        // [`note_scope`] has already recorded its program as
        // [`UntouchedReason::NotAnObject`], which is what makes that a
        // reported outcome rather than a silent one (ruling 10), and what
        // stops a program it *shares* with a font that does have an object
        // from being cut to the other font's glyphs.
        return;
    };
    usage.seen.insert(object);

    match scope.selection(font_id, code) {
        Some(chosen) if chosen.stated => {
            usage.glyphs.entry(object).or_default().insert(chosen.glyph);
        }
        Some(_) => usage.refuse(object, UntouchedReason::CodeNotMapped),
        // No embedded program, or one neither parser reads, or a Type 1
        // program whose charstring index is not a glyph id. Nothing here can
        // be subsetted; the sweep reports whatever the subsetter says about
        // the bytes.
        None => {}
    }
}

/// A page's content as the editor now has it.
fn page_content(editor: &DocumentEditor, doc: &CosDocument, page: ObjRef) -> Vec<u8> {
    let Some(Object::Dict(dict)) = editor.get(page) else {
        return Vec::new();
    };
    let refs: Vec<ObjRef> = match dict.get(Name::CONTENTS) {
        Some(Object::Ref(r)) => vec![*r],
        Some(Object::Array(items)) => items.iter().filter_map(Object::as_objref).collect(),
        _ => Vec::new(),
    };
    let _ = doc;

    let mut out = Vec::new();
    for r in refs {
        if let Some(bytes) = editor.stream_bytes(r) {
            out.extend_from_slice(&bytes);
        }
        out.push(b'\n');
    }
    out
}

/// One appearance stream to walk.
struct Appearance {
    stream: ObjRef,
    resources: Dict,
}

/// Every appearance stream an annotation on this page can ever show.
///
/// **Every** state of every one of `/N`, `/D` and `/R`, rather than the one
/// `/AS` selects: 12.5.5 lets a viewer switch states with no edit to the file,
/// so a subset cut to today's state loses tomorrow's tick. [`crate::annots`]
/// picks one because it draws one; this counts them all because it is deciding
/// what the file must still be able to draw.
fn appearances_of(editor: &DocumentEditor, doc: &CosDocument, page: ObjRef) -> Vec<Appearance> {
    let mut out = Vec::new();
    let Some(Object::Dict(dict)) = editor.get(page) else {
        return out;
    };
    let page_resources = doc
        .resolve_key(&dict, Name::RESOURCES)
        .as_dict()
        .cloned()
        .unwrap_or_default();

    let annots = doc.resolve_key(&dict, doc.intern(b"Annots"));
    let Some(entries) = annots.as_array() else {
        return out;
    };

    for entry in entries {
        let annotation = match entry {
            Object::Ref(r) => editor.get(*r).and_then(|o| o.as_dict().cloned()),
            Object::Dict(d) => Some(d.clone()),
            _ => None,
        };
        let Some(annotation) = annotation else {
            continue;
        };
        let ap = doc.resolve_key(&annotation, doc.intern(b"AP"));
        let Some(ap) = ap.as_dict() else { continue };

        for key in [b"N".as_slice(), b"D", b"R"] {
            let Some(value) = ap.get(doc.intern(key)) else {
                continue;
            };
            for stream in appearance_streams(doc, value) {
                let resources = doc
                    .get(stream)
                    .ok()
                    .and_then(|o| o.as_dict().cloned())
                    .map(|d| doc.resolve_key(&d, Name::RESOURCES).as_dict().cloned())
                    .unwrap_or_default()
                    // 8.10.1: an appearance without its own `/Resources` names
                    // things in the page's, which is the scope that draws it.
                    .unwrap_or_else(|| page_resources.clone());
                out.push(Appearance { stream, resources });
            }
        }
    }
    out
}

/// The streams one `/AP` entry can be: the stream itself, or every state of a
/// dictionary of them.
fn appearance_streams(doc: &CosDocument, value: &Object) -> Vec<ObjRef> {
    match value {
        Object::Ref(r) => match doc.get(*r) {
            Ok(object) if object.as_stream().is_some() => vec![*r],
            Ok(object) => object
                .as_dict()
                .map(|states| states.iter().filter_map(|(_, v)| v.as_objref()).collect())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        },
        Object::Dict(states) => states.iter().filter_map(|(_, v)| v.as_objref()).collect(),
        _ => Vec::new(),
    }
}

/// Every font the AcroForm `/DR` puts in scope for a `/DA` (12.7.3.3).
fn default_resource_fonts(editor: &DocumentEditor, doc: &CosDocument) -> Vec<ObjRef> {
    let Some(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let Some(acro) = catalog.get_ref(doc.intern(b"AcroForm")) else {
        // A directly written AcroForm is read from the catalog itself.
        let form = doc.resolve_key(&catalog, doc.intern(b"AcroForm"));
        return form
            .as_dict()
            .map(|form| default_resource_fonts_in(doc, form))
            .unwrap_or_default();
    };
    editor
        .get(acro)
        .and_then(|o| o.as_dict().cloned())
        .map(|form| default_resource_fonts_in(doc, &form))
        .unwrap_or_default()
}

fn default_resource_fonts_in(doc: &CosDocument, form: &Dict) -> Vec<ObjRef> {
    let dr = doc.resolve_key(form, doc.intern(b"DR"));
    let Some(dr) = dr.as_dict() else {
        return Vec::new();
    };
    let fonts = doc.resolve_key(dr, doc.intern(b"Font"));
    fonts
        .as_dict()
        .map(|fonts| fonts.iter().filter_map(|(_, v)| v.as_objref()).collect())
        .unwrap_or_default()
}

/// The fonts a Type 3 font's own `/Resources` names (9.6.5).
fn type3_resource_fonts(doc: &CosDocument, font: ObjRef) -> Vec<ObjRef> {
    let Ok(object) = doc.get(font) else {
        return Vec::new();
    };
    let Some(dict) = object.as_dict() else {
        return Vec::new();
    };
    let subtype = dict
        .get_name(doc.intern(b"Subtype"))
        .and_then(|n| doc.name_bytes(n));
    if subtype.as_deref() != Some(b"Type3".as_slice()) {
        return Vec::new();
    }
    let resources = doc.resolve_key(dict, Name::RESOURCES);
    let Some(resources) = resources.as_dict() else {
        return Vec::new();
    };
    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    fonts
        .as_dict()
        .map(|fonts| fonts.iter().filter_map(|(_, v)| v.as_objref()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The rewrite.
// ---------------------------------------------------------------------------

/// One font dictionary that embeds a program.
struct Site {
    /// The dictionary a resource name points at.
    font: ObjRef,
    /// The descendant CIDFont, for a composite font (9.7.6).
    descendant: Option<ObjRef>,
    /// The descriptor holding `/FontName` (9.8.1).
    descriptor: ObjRef,
    /// `/BaseFont`, as written.
    base_font: Vec<u8>,
    /// Which `/FontFile*` key named the program.
    key: cos_font::ProgramKey,
}

/// One program stream, with every font that names it.
struct Job {
    sites: Vec<Site>,
    glyphs: BTreeSet<u16>,
    refused: Option<UntouchedReason>,
}

fn rewrite(editor: &mut DocumentEditor, doc: &Arc<CosDocument>, usage: &Usage) -> SubsetReport {
    let mut jobs: BTreeMap<ObjRef, Job> = BTreeMap::new();
    for site in sweep(editor, doc) {
        let Some(program) = program_of(editor, doc, &site) else {
            continue;
        };
        let job = jobs.entry(program).or_insert_with(|| Job {
            sites: Vec::new(),
            glyphs: BTreeSet::new(),
            refused: None,
        });

        if let Some(reason) = usage.unbounded.get(&site.font) {
            job.refused.get_or_insert(*reason);
        } else if !usage.seen.contains(&site.font) {
            job.refused.get_or_insert(UntouchedReason::ScopeNotWalked);
        }
        if let Some(glyphs) = usage.glyphs.get(&site.font) {
            job.glyphs.extend(glyphs.iter().copied());
        }
        job.sites.push(site);
    }

    // A program a directly written font embeds. If some other font reaches it
    // by reference it is already a job, and refusing that job is what keeps
    // the direct font's glyphs — they were never counted, and cutting to the
    // other font's set would drop them. If nothing else reaches it there is no
    // job at all, and one is made solely so that ruling 10 can name it.
    let mut report = SubsetReport::default();
    for (program, base_font) in &usage.direct {
        match jobs.get_mut(program) {
            Some(job) => {
                job.refused.get_or_insert(UntouchedReason::NotAnObject);
            }
            None => {
                let Some(bytes) = editor.stream_bytes(*program) else {
                    continue;
                };
                report.untouched.push(Untouched {
                    program: *program,
                    base_font: String::from_utf8_lossy(base_font).into_owned(),
                    bytes: bytes.len(),
                    reason: UntouchedReason::NotAnObject,
                });
            }
        }
    }

    for (program, job) in jobs {
        let Some(bytes) = editor.stream_bytes(program) else {
            continue;
        };
        let base_font = job
            .sites
            .first()
            .map(|s| String::from_utf8_lossy(&s.base_font).into_owned())
            .unwrap_or_default();

        let leave = |reason: UntouchedReason, report: &mut SubsetReport| {
            report.untouched.push(Untouched {
                program,
                base_font: base_font.clone(),
                bytes: bytes.len(),
                reason,
            });
        };

        if let Some(reason) = job.refused {
            leave(reason, &mut report);
            continue;
        }
        let Some(reduced) = tinker_pdf_font::subset(&bytes, &job.glyphs) else {
            leave(UntouchedReason::ProgramNotRebuildable, &mut report);
            continue;
        };
        if reduced.len() >= bytes.len() {
            leave(UntouchedReason::SubsetNotSmaller, &mut report);
            continue;
        }

        let tag = subset_tag(&reduced);
        let after = reduced.len();
        write_program(editor, doc, program, &job, reduced);
        let named = rename(editor, doc, &job, &tag);

        report.subsetted.push(Subsetted {
            program,
            base_font: named,
            before: bytes.len(),
            after,
            glyphs: job.glyphs.len(),
        });
    }
    // Object order, as both fields promise. The jobs are already in it — a
    // `BTreeMap` keyed by the program — but the directly-written fonts above
    // were appended before the loop ran.
    report.untouched.sort_by_key(|u| u.program.num);
    report.subsetted.sort_by_key(|s| s.program.num);
    report
}

/// Every font dictionary in the document that embeds a program.
///
/// A sweep of the cross-reference table rather than a walk of the page tree,
/// because the question it answers is "is there any font naming this program
/// that the walk did not reach" — which a walk cannot answer about itself.
fn sweep(editor: &DocumentEditor, doc: &Arc<CosDocument>) -> Vec<Site> {
    let mut sites: Vec<Site> = Vec::new();
    let mut descendants: HashSet<u32> = HashSet::new();

    let numbers: Vec<u32> = doc
        .xref()
        .iter()
        .map(|(number, _)| number)
        .take(MAX_SWEPT_OBJECTS)
        .collect();

    for number in numbers {
        let reference = ObjRef::new(number, 0);
        let Some(object) = editor.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let kind = dict
            .get_name(doc.intern(b"Type"))
            .and_then(|n| doc.name_bytes(n));
        if kind.as_deref() != Some(b"Font".as_slice()) {
            continue;
        }
        let subtype = dict
            .get_name(doc.intern(b"Subtype"))
            .and_then(|n| doc.name_bytes(n))
            .map(|b| b.to_vec());

        let base_font = dict
            .get_name(doc.intern(b"BaseFont"))
            .and_then(|n| doc.name_bytes(n))
            .map(|b| b.to_vec())
            .unwrap_or_default();

        // 9.7.6: a Type 0 font has no descriptor of its own; the descendant
        // carries it, and both dictionaries name the face.
        let descendant = if subtype.as_deref() == Some(b"Type0".as_slice()) {
            let array = doc.resolve_key(dict, doc.intern(b"DescendantFonts"));
            array
                .as_array()
                .and_then(|items| items.first().and_then(Object::as_objref))
        } else {
            None
        };
        if let Some(descendant) = descendant {
            descendants.insert(descendant.num);
        }

        let holder = match descendant {
            Some(descendant) => match editor.get(descendant).and_then(|o| o.as_dict().cloned()) {
                Some(dict) => dict,
                None => continue,
            },
            None => dict.clone(),
        };
        let Some(descriptor) = holder.get_ref(doc.intern(b"FontDescriptor")) else {
            continue;
        };
        let Some(descriptor_dict) = editor.get(descriptor).and_then(|o| o.as_dict().cloned())
        else {
            continue;
        };
        let Some(key) = program_key(doc, &descriptor_dict) else {
            continue;
        };

        sites.push(Site {
            font: reference,
            descendant,
            descriptor,
            base_font,
            key,
        });
    }

    // A descendant CIDFont is a font dictionary in its own right and was swept
    // as one. It is not a font any resource dictionary names, so keeping it
    // would make every composite font's program `ScopeNotWalked`.
    sites.retain(|site| !descendants.contains(&site.font.num));
    sites
}

/// Which `/FontFile*` key a descriptor carries (9.9, Table 126).
fn program_key(doc: &CosDocument, descriptor: &Dict) -> Option<cos_font::ProgramKey> {
    for (key, which) in [
        (b"FontFile2".as_slice(), cos_font::ProgramKey::FontFile2),
        (b"FontFile3", cos_font::ProgramKey::FontFile3),
        (b"FontFile", cos_font::ProgramKey::FontFile),
    ] {
        if descriptor.get_ref(doc.intern(key)).is_some() {
            return Some(which);
        }
    }
    None
}

/// The stream a site's descriptor points at.
fn program_of(editor: &DocumentEditor, doc: &CosDocument, site: &Site) -> Option<ObjRef> {
    let descriptor = editor.get(site.descriptor)?;
    let descriptor = descriptor.as_dict()?;
    let key = match site.key {
        cos_font::ProgramKey::FontFile => b"FontFile".as_slice(),
        cos_font::ProgramKey::FontFile2 => b"FontFile2",
        cos_font::ProgramKey::FontFile3 => b"FontFile3",
    };
    descriptor.get_ref(doc.intern(key))
}

/// Replaces the program, keeping the stream's dictionary consistent with it.
fn write_program(
    editor: &mut DocumentEditor,
    doc: &CosDocument,
    program: ObjRef,
    job: &Job,
    reduced: Vec<u8>,
) {
    let existing = doc
        .get(program)
        .ok()
        .and_then(|o| o.as_dict().cloned())
        .unwrap_or_default();

    let mut dict = Dict::new();
    // `/Subtype` is what a `/FontFile3` *is* (Table 126) — `/Type1C`,
    // `/CIDFontType0C`, `/OpenType` — and the subsetter does not change which.
    if let Some(subtype) = existing.get_name(doc.intern(b"Subtype")) {
        dict.insert(editor.intern(b"Subtype"), Object::Name(subtype));
    }
    // Table 126: `/Length1` is the decoded length of a `/FontFile2` program,
    // so it moves with the program. A `/FontFile3` has none — its `/Subtype`
    // says what the bytes are — and a stale one is dropped rather than
    // carried, because a number that is wrong is worse than one that is
    // absent.
    //
    // The filter keys are deliberately not copied: `put_stream` is handed the
    // decoded bytes and computes `/Length` for them, so a `/Filter` carried
    // over would declare an encoding these bytes are not in.
    if matches!(job.key(), Some(cos_font::ProgramKey::FontFile2)) {
        let length = i64::try_from(reduced.len()).unwrap_or(i64::MAX);
        dict.insert(editor.intern(b"Length1"), Object::Int(length));
    }

    editor.put_stream(
        program,
        StreamData {
            dict,
            data: reduced,
        },
    );
}

impl Job {
    /// Which key named the program, when every site agrees.
    fn key(&self) -> Option<cos_font::ProgramKey> {
        let first = self.sites.first()?.key;
        self.sites.iter().all(|s| s.key == first).then_some(first)
    }
}

/// Puts 9.6.4's subset tag on every name that has to carry it, and returns the
/// name it wrote.
fn rename(editor: &mut DocumentEditor, doc: &CosDocument, job: &Job, tag: &[u8]) -> String {
    let mut written = String::new();
    for site in &job.sites {
        let mut tagged = tag.to_vec();
        tagged.extend_from_slice(strip_tag(&site.base_font));
        written = String::from_utf8_lossy(&tagged).into_owned();
        let name = editor.intern(&tagged);

        set_name(editor, doc, site.font, b"BaseFont", name);
        // 9.7.6.2: the descendant names the same face.
        if let Some(descendant) = site.descendant {
            set_name(editor, doc, descendant, b"BaseFont", name);
        }
        // 9.8.1: `/FontName` shall be the same as `/BaseFont`.
        set_name(editor, doc, site.descriptor, b"FontName", name);
    }
    written
}

fn set_name(editor: &mut DocumentEditor, doc: &CosDocument, at: ObjRef, key: &[u8], value: Name) {
    let _ = doc;
    let Some(Object::Dict(mut dict)) = editor.get(at) else {
        return;
    };
    dict.insert(editor.intern(key), Object::Name(value));
    editor.put(at, Object::Dict(dict));
}

/// A `/BaseFont` with any 9.6.4 subset tag taken off.
///
/// The tag names *a set of glyphs*, so one already there named the set the
/// producer kept and says nothing true about the set this pass kept. Stacking
/// a second tag in front of it would produce `ABCDEF+GHIJKL+Times`, which is
/// not a name 9.6.4 describes.
///
/// The shape is exact — six upper-case letters then a plus — because anything
/// looser eats a real name: `A+B` is a font called `A+B`.
fn strip_tag(base_font: &[u8]) -> &[u8] {
    if base_font.len() > 7
        && base_font[6] == b'+'
        && base_font[..6].iter().all(|b| b.is_ascii_uppercase())
    {
        return &base_font[7..];
    }
    base_font
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_six_letter_tag_and_a_plus_come_off_a_base_font() {
        assert_eq!(strip_tag(b"ABCDEF+Times-Roman"), b"Times-Roman");
    }

    #[test]
    fn a_name_that_merely_contains_a_plus_is_not_a_tag() {
        // `A+B` is a font called `A+B`, and a looser test would eat the name.
        assert_eq!(strip_tag(b"A+B"), b"A+B");
        assert_eq!(strip_tag(b"abcdef+Times"), b"abcdef+Times");
        assert_eq!(strip_tag(b"ABCDE+Times"), b"ABCDE+Times");
        // Six letters, a plus, and nothing after it is not a name with a tag
        // on it — stripping would leave the font with no name at all.
        assert_eq!(strip_tag(b"ABCDEF+"), b"ABCDEF+");
    }

    #[test]
    fn an_untagged_name_is_returned_whole() {
        assert_eq!(strip_tag(b"Times-Roman"), b"Times-Roman");
        assert_eq!(strip_tag(b""), b"");
    }
}

#[cfg(test)]
mod tests_support {
    use super::*;

    pub use tinker_pdf_cos::{DocumentBuilder, WriteMode, WriteOptions};
    pub use tinker_pdf_font::{glyf, Sfnt};

    /// Liberation Serif Regular, read from the directory `bundled-fonts`
    /// embeds it from.
    ///
    /// **Third-party bytes, and that is the point** (ruling 13). Nothing in
    /// this repository decides which of this face's glyphs are composite, how
    /// many contours each carries, or where `loca` puts them. A fixture face
    /// written here could be made to pass a subsetter that drops components;
    /// this one cannot.
    pub fn face() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf");
        std::fs::read(&path).expect("the vendored Liberation face is readable")
    }

    /// Liberation Sans Regular, for the fixtures that need two faces that are
    /// not the same bytes.
    pub fn sans_face() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tinker-pdf-font/data/liberation/LiberationSans-Regular.ttf");
        std::fs::read(&path).expect("the vendored Liberation face is readable")
    }

    pub fn open(bytes: Vec<u8>) -> Arc<CosDocument> {
        Arc::new(CosDocument::open(bytes).expect("it opens"))
    }

    /// The program embedded by the font whose `/BaseFont` ends in `suffix`.
    ///
    /// By name rather than by object order, because "the first program in the
    /// file" is a fact about the writer and this asks a question about the
    /// font.
    pub fn program_named(doc: &CosDocument, suffix: &[u8]) -> Vec<u8> {
        for (_, dict) in font_dicts(doc) {
            if !base_font(doc, &dict).ends_with(suffix) {
                continue;
            }
            let program = embedded_program(doc, &dict).expect("the font embeds a program");
            return doc.stream_decoded(program).expect("the program decodes");
        }
        panic!("no font is named {}", String::from_utf8_lossy(suffix));
    }

    pub fn saved(editor: &DocumentEditor) -> Vec<u8> {
        editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        })
    }

    /// One page of text, over **the whole face**.
    ///
    /// `set_subset_fonts(false)` is the fixture: what the rewrite has to meet
    /// is a program nothing has cut down, which is what a document from any
    /// other producer hands it.
    pub fn whole_face_document(lines: &[(f64, &str)]) -> Vec<u8> {
        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(
            builder.add_embedded_font(b"F0", b"LiberationSerif", &face()),
            "the vendored face parses as a TrueType program"
        );
        builder.add_page(300.0, 100.0, |page| {
            for (y, text) in lines {
                page.text(b"F0", 24.0, 20.0, *y, text);
            }
        });
        builder.finish()
    }

    /// Runs the pass and writes the result whole (9.9).
    pub fn subset(bytes: Vec<u8>) -> (Vec<u8>, SubsetReport) {
        let mut editor = DocumentEditor::new(open(bytes));
        let report = apply(&mut editor);
        (saved(&editor), report)
    }

    /// Every embedded program in a document, in object order.
    pub fn program_refs(doc: &CosDocument) -> Vec<ObjRef> {
        let mut out = Vec::new();
        for (number, _) in doc.xref().iter() {
            let Ok(object) = doc.get(ObjRef::new(number, 0)) else {
                continue;
            };
            let Some(dict) = object.as_dict() else {
                continue;
            };
            for key in [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"] {
                if let Some(r) = dict.get_ref(doc.intern(key)) {
                    out.push(r);
                }
            }
        }
        out.sort_by_key(|r| r.num);
        out.dedup_by_key(|r| r.num);
        out
    }

    /// The one program a single-font fixture carries.
    pub fn only_program(doc: &CosDocument) -> Vec<u8> {
        let refs = program_refs(doc);
        assert_eq!(refs.len(), 1, "the fixture embeds exactly one program");
        doc.stream_decoded(refs[0]).expect("the program decodes")
    }

    /// Every `/Type /Font` dictionary, in object order.
    pub fn font_dicts(doc: &CosDocument) -> Vec<(ObjRef, Dict)> {
        let mut out = Vec::new();
        for (number, _) in doc.xref().iter() {
            let reference = ObjRef::new(number, 0);
            let Ok(object) = doc.get(reference) else {
                continue;
            };
            let Some(dict) = object.as_dict() else {
                continue;
            };
            let kind = dict
                .get_name(doc.intern(b"Type"))
                .and_then(|n| doc.name_bytes(n));
            if kind.as_deref() == Some(b"Font".as_slice()) {
                out.push((reference, dict.clone()));
            }
        }
        out.sort_by_key(|(r, _)| r.num);
        out
    }

    /// A dictionary's `/BaseFont`, as written.
    pub fn base_font(doc: &CosDocument, dict: &Dict) -> Vec<u8> {
        dict.get_name(doc.intern(b"BaseFont"))
            .and_then(|n| doc.name_bytes(n))
            .map(|b| b.to_vec())
            .unwrap_or_default()
    }

    /// Page 0, rendered at the default scale.
    pub fn render(bytes: Vec<u8>) -> crate::Bitmap {
        crate::Document::open(bytes)
            .expect("it reopens")
            .page(0)
            .expect("a page")
            .render(&crate::RenderOptions::default())
    }

    /// Whether a program still draws a glyph.
    ///
    /// Through [`glyf::outline`], which **follows a composite's components**,
    /// so a composite whose components were dropped answers `false` here — it
    /// is the blank the reader would get.
    pub fn draws(program: &[u8], glyph: u16) -> bool {
        let sfnt = Sfnt::parse(program).expect("the program is an sfnt");
        !glyf::outline(&sfnt, glyph).expect("an outline").is_empty()
    }

    /// The same face with its `cmap` renamed out of the table directory.
    ///
    /// Four bytes of a 16-byte table record, so every offset and length in the
    /// file still points where it did and the tables themselves are untouched.
    /// What comes out is a real TrueType program that maps no character —
    /// which is what a producer's own subset frequently is, and the case
    /// 9.6.6.4's closing guess exists for.
    pub fn without_cmap(program: &[u8]) -> Vec<u8> {
        let mut out = program.to_vec();
        let tables = usize::from(u16::from_be_bytes([out[4], out[5]]));
        for index in 0..tables {
            let at = 12 + index * 16;
            if out.get(at..at + 4) == Some(b"cmap".as_slice()) {
                out[at] = b'x';
                return out;
            }
        }
        panic!("the vendored face carries a cmap to rename");
    }

    /// The glyph a character selects through the face's own `cmap`.
    pub fn glyph_of(program: &[u8], ch: char) -> u16 {
        let sfnt = Sfnt::parse(program).expect("the program is an sfnt");
        sfnt.glyph_for_char(ch)
            .filter(|g| *g != 0)
            .unwrap_or_else(|| panic!("the face carries {ch:?}"))
    }

    /// One form XObject, as an appearance stream drawing `text` through the
    /// page's own font.
    pub fn appearance_stream(editor: &mut DocumentEditor, font: ObjRef, text: &str) -> ObjRef {
        let stream = editor.allocate();
        let mut fonts = Dict::new();
        fonts.insert(editor.intern(b"F0"), Object::Ref(font));
        let mut resources = Dict::new();
        resources.insert(editor.intern(b"Font"), Object::Dict(fonts));
        let mut dict = Dict::new();
        dict.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(b"Form")),
        );
        dict.insert(
            editor.intern(b"BBox"),
            Object::Array(vec![
                Object::Int(0),
                Object::Int(0),
                Object::Int(80),
                Object::Int(20),
            ]),
        );
        dict.insert(editor.intern(b"Resources"), Object::Dict(resources));
        editor.put_stream(
            stream,
            StreamData {
                dict,
                data: format!("BT /F0 12 Tf 2 2 Td ({text}) Tj ET").into_bytes(),
            },
        );
        stream
    }

    /// Hangs one annotation dictionary off page 0.
    pub fn attach(editor: &mut DocumentEditor, annotation: Dict) {
        let reference = editor.allocate();
        editor.put(reference, Object::Dict(annotation));
        let page = editor.page_refs()[0];
        let Some(Object::Dict(mut page_dict)) = editor.get(page) else {
            panic!("the page is a dictionary");
        };
        page_dict.insert(
            editor.intern(b"Annots"),
            Object::Array(vec![Object::Ref(reference)]),
        );
        editor.put(page, Object::Dict(page_dict));
    }

    /// The annotation shell every appearance test hangs its `/AP` on.
    pub fn widget(editor: &mut DocumentEditor, ap: Dict) -> Dict {
        let mut annot = Dict::new();
        annot.insert(
            editor.intern(b"Type"),
            Object::Name(editor.intern(b"Annot")),
        );
        annot.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(b"Widget")),
        );
        annot.insert(
            editor.intern(b"Rect"),
            Object::Array(vec![
                Object::Int(10),
                Object::Int(70),
                Object::Int(90),
                Object::Int(90),
            ]),
        );
        annot.insert(editor.intern(b"AP"), Object::Dict(ap));
        annot
    }

    /// The font object a single-font fixture carries.
    pub fn only_font(doc: &CosDocument) -> ObjRef {
        font_dicts(doc)
            .first()
            .map(|(r, _)| *r)
            .expect("the fixture has a font")
    }

    /// The same document with an annotation whose `/N` appearance draws
    /// `text` through the page's own font.
    ///
    /// The font is named by **object**, which is the case that matters: the
    /// appearance's `/F0` and the page's `/F0` are the same font, and a walk
    /// keyed by resource name rather than by object would credit the glyphs to
    /// whichever scope it looked at last.
    pub fn with_appearance_drawing(bytes: Vec<u8>, text: &str) -> Vec<u8> {
        let doc = open(bytes);
        let font = only_font(&doc);
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let stream = appearance_stream(&mut editor, font, text);
        let mut ap = Dict::new();
        ap.insert(editor.intern(b"N"), Object::Ref(stream));
        let annotation = widget(&mut editor, ap);
        attach(&mut editor, annotation);
        saved(&editor)
    }

    /// The same document with an AcroForm whose `/DR` names the font
    /// (12.7.3.3).
    pub fn with_default_resources(bytes: Vec<u8>) -> Vec<u8> {
        let doc = open(bytes);
        let font = only_font(&doc);
        let mut editor = DocumentEditor::new(Arc::clone(&doc));

        let mut fonts = Dict::new();
        fonts.insert(editor.intern(b"F0"), Object::Ref(font));
        let mut dr = Dict::new();
        dr.insert(editor.intern(b"Font"), Object::Dict(fonts));
        let mut acro = Dict::new();
        acro.insert(editor.intern(b"DR"), Object::Dict(dr));
        acro.insert(editor.intern(b"Fields"), Object::Array(Vec::new()));
        let acro_ref = editor.allocate();
        editor.put(acro_ref, Object::Dict(acro));

        let catalog_ref = doc
            .trailer()
            .get_ref(doc.intern(b"Root"))
            .expect("a catalog reference");
        let Some(Object::Dict(mut catalog)) = editor.get(catalog_ref) else {
            panic!("the catalog is a dictionary");
        };
        catalog.insert(editor.intern(b"AcroForm"), Object::Ref(acro_ref));
        editor.put(catalog_ref, Object::Dict(catalog));
        saved(&editor)
    }
}

/// The two halves of the honest claim: the rewrite draws the same pixels, and
/// the program it draws them from is smaller.
///
/// **What adjudicates what** (ruling 13). The face is third-party and so is
/// every `glyf` entry these assertions count. The bitmap comparison is
/// *self-consistency* — this engine's raster of the original against this
/// engine's raster of the rewrite — and it is kept for exactly one thing: it
/// is sensitive to a glyph that went missing, which is the failure this module
/// can cause. It says nothing about whether either raster is right.
#[cfg(test)]
mod render_and_size {
    use super::tests_support::*;
    use super::*;

    #[test]
    fn the_rewrite_renders_identically_and_the_program_shrinks() {
        let before = whole_face_document(&[(40.0, "Handgloves")]);
        let whole = only_program(&open(before.clone()));
        let (after, report) = subset(before.clone());
        let cut = only_program(&open(after.clone()));

        assert!(
            report.untouched.is_empty(),
            "nothing should be left whole here: {:?}",
            report.untouched
        );
        assert_eq!(report.subsetted.len(), 1, "one program, cut once");
        assert_eq!(report.subsetted[0].before, whole.len());
        assert_eq!(report.subsetted[0].after, cut.len());

        // Ten characters of a face carrying thousands. A ratio rather than a
        // byte count, because the vendored face is allowed to be revised and a
        // byte count would then be a defect in this test rather than in the
        // subsetter.
        assert!(
            cut.len() * 4 < whole.len(),
            "the cut program is {} bytes against the face's {}",
            cut.len(),
            whole.len()
        );

        let original = render(before);
        let rewritten = render(after);
        assert_eq!(
            (original.width, original.height),
            (rewritten.width, rewritten.height)
        );
        assert!(
            original.data == rewritten.data,
            "the rewrite must draw the same pixels as the original"
        );
        assert!(
            original.data.iter().any(|b| *b < 200),
            "the fixture draws ink at all, or the comparison above is vacuous"
        );
    }

    #[test]
    fn every_glyph_the_page_shows_is_still_in_the_program() {
        let before = whole_face_document(&[(40.0, "Handgloves")]);
        let (after, _) = subset(before);
        let cut = only_program(&open(after));
        for ch in "Handgloves".chars() {
            let glyph = glyph_of(&cut, ch);
            assert!(draws(&cut, glyph), "{ch:?} is glyph {glyph} and is gone");
        }
    }

    #[test]
    fn the_subset_tag_is_six_upper_case_letters_and_a_plus() {
        let (after, report) = subset(whole_face_document(&[(40.0, "Handgloves")]));
        let doc = open(after);

        assert_eq!(report.subsetted.len(), 1);
        let named = report.subsetted[0].base_font.clone();
        assert!(
            named.ends_with("+LiberationSerif"),
            "the report names {named:?}"
        );

        let mut descriptors = 0;
        for (reference, dict) in font_dicts(&doc) {
            let _ = reference;
            let name = base_font(&doc, &dict);
            assert!(
                name.len() > 7 && name[6] == b'+',
                "/BaseFont is {:?}, which carries no 9.6.4 tag",
                String::from_utf8_lossy(&name)
            );
            assert!(
                name[..6].iter().all(u8::is_ascii_uppercase),
                "the tag in {:?} is not six upper-case letters",
                String::from_utf8_lossy(&name)
            );
            assert_eq!(&name[7..], b"LiberationSerif", "the face keeps its name");
            assert_eq!(String::from_utf8_lossy(&name), named);
        }

        // 9.8.1: `/FontName` shall be the same as `/BaseFont`.
        for (number, _) in doc.xref().iter() {
            let Ok(object) = doc.get(ObjRef::new(number, 0)) else {
                continue;
            };
            let Some(dict) = object.as_dict() else {
                continue;
            };
            let kind = dict
                .get_name(doc.intern(b"Type"))
                .and_then(|n| doc.name_bytes(n));
            if kind.as_deref() != Some(b"FontDescriptor".as_slice()) {
                continue;
            }
            descriptors += 1;
            let font_name = dict
                .get_name(doc.intern(b"FontName"))
                .and_then(|n| doc.name_bytes(n))
                .expect("a descriptor names its face");
            assert_eq!(String::from_utf8_lossy(&font_name), named);
        }
        assert_eq!(descriptors, 1, "the fixture has one descriptor");
    }

    /// The encoding survives because nothing moves it.
    ///
    /// [`tinker_pdf_font::subset`] does not renumber, so every glyph index in
    /// the file still means what it meant and the arrays that address glyphs
    /// are correct **because they are unchanged**. This pins that: the only
    /// keys this pass may touch are the three that name the font.
    #[test]
    fn font_dictionaries_are_untouched_except_for_the_subset_tag() {
        let before = whole_face_document(&[(40.0, "Handgloves")]);
        let before_doc = open(before.clone());
        let (after, _) = subset(before);
        let after_doc = open(after);

        let old = font_dicts(&before_doc);
        let new = font_dicts(&after_doc);
        assert_eq!(old.len(), new.len(), "no font dictionary appeared or left");
        assert_eq!(old.len(), 1, "the fixture has one font dictionary");

        for ((_, old), (_, new)) in old.iter().zip(new.iter()) {
            let keys = |doc: &CosDocument, dict: &Dict| {
                let mut names: Vec<String> = dict
                    .iter()
                    .filter_map(|(k, _)| doc.name_bytes(*k))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .collect();
                names.sort();
                names
            };
            assert_eq!(
                keys(&before_doc, old),
                keys(&after_doc, new),
                "the key set of the font dictionary moved"
            );

            for key in [b"FirstChar".as_slice(), b"LastChar"] {
                let old_value = old.get(before_doc.intern(key)).and_then(Object::as_int);
                assert!(
                    old_value.is_some(),
                    "the fixture has /{}",
                    String::from_utf8_lossy(key)
                );
                assert_eq!(
                    old_value,
                    new.get(after_doc.intern(key)).and_then(Object::as_int),
                    "/{} moved",
                    String::from_utf8_lossy(key)
                );
            }

            let widths = |doc: &CosDocument, dict: &Dict| {
                doc.resolve_key(dict, doc.intern(b"Widths"))
                    .as_array()
                    .map(|items| items.iter().filter_map(Object::as_int).collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            let old_widths = widths(&before_doc, old);
            assert!(!old_widths.is_empty(), "the fixture has /Widths at all");
            assert_eq!(
                old_widths,
                widths(&after_doc, new),
                "/Widths moved, which renders as the wrong advances"
            );
        }
    }

    /// `/Length1` is the **decoded** length of a `/FontFile2` (Table 126), so
    /// a stale one describes the face rather than the subset.
    #[test]
    fn length1_describes_the_subset_rather_than_the_face() {
        let (after, report) = subset(whole_face_document(&[(40.0, "Handgloves")]));
        let doc = open(after);
        let program = program_refs(&doc)[0];
        let dict = doc
            .get(program)
            .expect("the program object")
            .as_dict()
            .cloned()
            .expect("a stream dictionary");
        let length1 = dict
            .get(doc.intern(b"Length1"))
            .and_then(Object::as_int)
            .expect("/FontFile2 carries /Length1");
        assert_eq!(
            usize::try_from(length1).expect("a length"),
            report.subsetted[0].after
        );
    }
}

/// The disclosure property, which is why this row matters for redaction.
///
/// A redaction removes the text. Until this module existed it did not remove
/// the **glyphs**, so a program whose `glyf` entries were exactly the letters
/// of a removed name still named it. Adjudicated against the program's own
/// `loca` and `glyf` tables — a statement about bytes in a file, not about a
/// raster.
#[cfg(test)]
mod disclosure {
    use super::tests_support::*;
    use super::*;

    use crate::redact::{apply as redact_apply, Redaction};
    use tinker_pdf_cos::pages::Rect;

    /// The two lines share no letter, so "the removed line's glyphs are gone"
    /// is not weakened by a glyph the kept line needs anyway.
    const KEPT: &str = "bcdfgh";
    const REMOVED: &str = "vwxyzk";

    fn cut_area() -> Redaction {
        Redaction {
            area: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 300.0,
                y1: 45.0,
            },
            mark: false,
        }
    }

    #[test]
    fn the_glyphs_of_redacted_text_are_not_in_the_output() {
        let built = whole_face_document(&[(60.0, KEPT), (20.0, REMOVED)]);
        let mut editor = DocumentEditor::new(open(built));
        let report = redact_apply(&mut editor, 0, &[cut_area()]).expect("the page exists");
        assert!(
            report.warnings.is_empty(),
            "the redaction measured every run: {:?}",
            report.warnings
        );
        assert_eq!(report.glyphs, REMOVED.chars().count());

        // The order is the whole contract: the walk reads the content the
        // editor now has, so the redaction has to be in the editor first.
        apply(&mut editor);
        let cut = only_program(&open(saved(&editor)));

        for ch in REMOVED.chars() {
            let glyph = glyph_of(&cut, ch);
            assert!(
                !draws(&cut, glyph),
                "{ch:?} was redacted and its outline (glyph {glyph}) is still in the file"
            );
        }
        for ch in KEPT.chars() {
            let glyph = glyph_of(&cut, ch);
            assert!(
                draws(&cut, glyph),
                "{ch:?} is still on the page and its outline (glyph {glyph}) is gone"
            );
        }
    }

    /// The text the redaction left still draws, after the program under it was
    /// cut down. Self-consistency, and sensitive to exactly one failure: a
    /// glyph the page still shows that the subset dropped.
    #[test]
    fn the_surviving_text_renders_the_same_as_before_the_subset() {
        let built = whole_face_document(&[(60.0, KEPT), (20.0, REMOVED)]);
        let mut editor = DocumentEditor::new(open(built));
        redact_apply(&mut editor, 0, &[cut_area()]).expect("the page exists");
        let redacted_only = saved(&editor);
        apply(&mut editor);
        let and_subsetted = saved(&editor);

        let before = render(redacted_only);
        let after = render(and_subsetted);
        assert_eq!((before.width, before.height), (after.width, after.height));
        assert!(
            before.data == after.data,
            "cutting the program changed what the remaining text draws"
        );
        assert!(before.data.iter().any(|b| *b < 200), "there is ink to lose");
    }
}

/// Which glyphs count as used, argued one inclusion at a time.
#[cfg(test)]
mod what_counts_as_used {
    use super::tests_support::*;
    use super::*;

    /// An appearance stream is a use. It is not on the page's content stream,
    /// and a walk that stopped at `/Contents` would drop its glyphs — which
    /// renders as a blank annotation, not as an error.
    #[test]
    fn a_glyph_shown_only_in_an_annotation_appearance_is_kept() {
        let page_only = whole_face_document(&[(40.0, "aaa")]);
        let with_annotation = with_appearance_drawing(page_only, "zzz");
        let (after, report) = subset(with_annotation);
        let cut = only_program(&open(after));

        assert!(report.untouched.is_empty(), "{:?}", report.untouched);
        let z = glyph_of(&cut, 'z');
        assert!(
            draws(&cut, z),
            "the appearance draws 'z' (glyph {z}) and the subset dropped it"
        );
        let a = glyph_of(&cut, 'a');
        assert!(draws(&cut, a), "the page's own glyph");
    }

    /// Every state of `/N`, `/D` and `/R`, rather than the one `/AS` selects:
    /// 12.5.5 lets a viewer switch states with no edit to the file, so a
    /// subset cut to today's state loses tomorrow's tick.
    #[test]
    fn a_glyph_in_an_unselected_appearance_state_is_kept() {
        let built = whole_face_document(&[(40.0, "aaa")]);
        let doc = open(built);
        let font = only_font(&doc);
        let mut editor = DocumentEditor::new(Arc::clone(&doc));

        let mut states = Dict::new();
        for (state, text) in [(b"Off".as_slice(), "aaa"), (b"On", "zzz")] {
            let stream = appearance_stream(&mut editor, font, text);
            states.insert(editor.intern(state), Object::Ref(stream));
        }
        let mut ap = Dict::new();
        ap.insert(editor.intern(b"N"), Object::Dict(states));
        let mut annotation = widget(&mut editor, ap);
        // `/AS` names the *other* state, so a walk that honoured it would lose
        // the tick in `/On`.
        annotation.insert(editor.intern(b"AS"), Object::Name(editor.intern(b"Off")));
        attach(&mut editor, annotation);

        let (after, _) = subset(saved(&editor));
        let cut = only_program(&open(after));
        let z = glyph_of(&cut, 'z');
        assert!(
            draws(&cut, z),
            "/AS names /Off, and the tick in /On (glyph {z}) was dropped"
        );
    }

    /// The same thing when `/AP /N` is an **indirect reference** to the state
    /// dictionary rather than the dictionary itself, which is how nearly every
    /// real producer writes it — the states are shared between widgets, so they
    /// get an object.
    ///
    /// A separate branch of [`appearance_streams`] reads it: the reference has
    /// to be resolved and then asked whether what came back is a stream (one
    /// appearance) or a dictionary (a state for each key). Without this the
    /// branch that tells those apart was reached by no test at all, and a
    /// version of it that kept only the stream case scored **zero** — every
    /// fixture wrote its states inline.
    #[test]
    fn a_glyph_in_an_appearance_state_reached_by_reference_is_kept() {
        let built = whole_face_document(&[(40.0, "aaa")]);
        let doc = open(built);
        let font = only_font(&doc);
        let mut editor = DocumentEditor::new(Arc::clone(&doc));

        let mut states = Dict::new();
        for (state, text) in [(b"Off".as_slice(), "aaa"), (b"On", "zzz")] {
            let stream = appearance_stream(&mut editor, font, text);
            states.insert(editor.intern(state), Object::Ref(stream));
        }
        // The one difference from the test above: the state dictionary is an
        // object, and `/AP /N` names it.
        let states_ref = editor.allocate();
        editor.put(states_ref, Object::Dict(states));
        let mut ap = Dict::new();
        ap.insert(editor.intern(b"N"), Object::Ref(states_ref));
        let mut annotation = widget(&mut editor, ap);
        annotation.insert(editor.intern(b"AS"), Object::Name(editor.intern(b"Off")));
        attach(&mut editor, annotation);

        let (after, _) = subset(saved(&editor));
        let cut = only_program(&open(after));
        let z = glyph_of(&cut, 'z');
        assert!(
            draws(&cut, z),
            "the tick in /On (glyph {z}) was dropped, and /AP /N is a reference"
        );
    }

    /// 12.7.3.3: a `/DA` is a promise about *future* uses — the field's value
    /// can be retyped — so there is no set of glyphs to bound and the program
    /// passes through whole, named (ruling 10).
    #[test]
    fn a_font_the_acroform_default_resources_name_is_left_whole_and_reported() {
        let built = whole_face_document(&[(40.0, "aaa")]);
        let whole = only_program(&open(built.clone()));
        let (after, report) = subset(with_default_resources(built));
        let cut = only_program(&open(after));

        assert_eq!(cut.len(), whole.len(), "the face passed through as it was");
        assert_eq!(report.subsetted, Vec::new());
        assert_eq!(report.untouched.len(), 1);
        assert_eq!(report.untouched[0].reason, UntouchedReason::FieldResource);
        assert_eq!(report.untouched[0].bytes, whole.len());
        assert_eq!(
            report.untouched[0].to_string(),
            format!(
                "/LiberationSerif left whole ({} bytes): a form field's /DA may draw it at any \
                 character (12.7.3.3)",
                whole.len()
            )
        );
    }

    /// A font a resource dictionary names and nothing draws through keeps
    /// `.notdef` and nothing else. The scope *was* walked, so "no glyphs" is a
    /// measurement rather than ignorance — which is the distinction
    /// [`UntouchedReason::ScopeNotWalked`] exists for.
    #[test]
    fn a_named_but_undrawn_font_is_cut_to_nothing() {
        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"LiberationSerif", &face()));
        builder.add_page(300.0, 100.0, |page| {
            page.text(b"F0", 24.0, 20.0, 40.0, "");
        });
        let built = builder.finish();
        let whole = only_program(&open(built.clone()));

        let (after, report) = subset(built);
        let cut = only_program(&open(after));
        assert_eq!(report.untouched, Vec::new());
        assert_eq!(report.subsetted.len(), 1);
        assert_eq!(report.subsetted[0].glyphs, 0, "nothing was shown");
        // Not zero bytes, and it should not be: `cmap`, `hmtx`, `OS/2` and the
        // three hinting tables are copied through whatever is drawn, because
        // another reader of the same file interprets them. What goes is
        // `glyf`, which is where the megabytes are.
        assert!(
            cut.len() * 10 < whole.len(),
            "{} bytes against the face's {}",
            cut.len(),
            whole.len()
        );
    }
}

/// The classic TrueType subsetting bug: a composite glyph names its components
/// by index, and dropping them renders as a blank or a wrong glyph rather than
/// as an error.
///
/// Third-party throughout. Which glyph `é` is, whether it is composite, and
/// which glyphs it is built from are all read out of the vendored face.
#[cfg(test)]
mod composite_glyphs {
    use super::tests_support::*;

    use tinker_pdf_cos::Glyph;

    #[test]
    fn the_components_of_a_shown_composite_glyph_survive() {
        let face = face();
        let sfnt = Sfnt::parse(&face).expect("an sfnt");
        let composite = sfnt.glyph_for_char('é').expect("the face carries e-acute");
        let components = tinker_pdf_font::subset::components(&sfnt, composite);
        assert!(
            !components.is_empty(),
            "e-acute is a composite in this face, or the test proves nothing"
        );
        assert!(
            !components.contains(&composite),
            "a component that is the glyph itself would make the closure vacuous"
        );

        // A composite *font*, because it is the only way to address a glyph
        // index directly: `/Identity-H` with `/CIDToGIDMap /Identity` makes
        // the two-byte code the CID and the CID the glyph (9.7.4.2).
        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_cid_font(b"F0", b"LiberationSerif", &face));
        builder.add_page(200.0, 100.0, |page| {
            assert!(page.glyphs(
                b"F0",
                48.0,
                20.0,
                30.0,
                &[Glyph {
                    id: composite,
                    text: "é",
                }],
            ));
        });
        let before = builder.finish();
        let (after, report) = subset(before.clone());
        let cut = only_program(&open(after.clone()));

        assert!(report.untouched.is_empty(), "{:?}", report.untouched);
        assert_eq!(report.subsetted.len(), 1);
        assert_eq!(
            report.subsetted[0].glyphs, 1,
            "one glyph was *asked* for; the closure is the subsetter's own"
        );
        for component in &components {
            assert!(
                draws(&cut, *component),
                "component {component} of glyph {composite} was dropped"
            );
        }
        assert!(
            draws(&cut, composite),
            "the composite draws nothing, which is what a lost component looks like"
        );

        let original = render(before);
        let rewritten = render(after);
        assert!(
            original.data == rewritten.data,
            "the composite renders differently after the cut"
        );
        assert!(original.data.iter().any(|b| *b < 200), "there is ink");
    }
}

/// Ruling 2 and ruling 10 together: a program that cannot be cut down passes
/// through whole, and says so.
#[cfg(test)]
mod refusals {
    use super::tests_support::*;
    use super::*;

    /// Running the pass twice is the cheapest real refusal there is: the
    /// second pass meets a program with nothing left to remove, and a subset
    /// that is no smaller is refused for the reason the builder refuses it —
    /// the face is both smaller and the one the producer tested.
    #[test]
    fn a_subset_that_is_no_smaller_is_refused_and_named() {
        let (once, _) = subset(whole_face_document(&[(40.0, "Handgloves")]));
        let program = only_program(&open(once.clone()));
        let (twice, report) = subset(once);

        assert_eq!(
            report.subsetted,
            Vec::new(),
            "nothing was cut the second time"
        );
        assert_eq!(report.untouched.len(), 1);
        assert_eq!(
            report.untouched[0].reason,
            UntouchedReason::SubsetNotSmaller
        );
        assert_eq!(
            only_program(&open(twice)).len(),
            program.len(),
            "the refused program went through as it was"
        );
    }

    /// The report accounts for every program either way, which is what a
    /// caller asking "is the disclosure still in the file" reads.
    #[test]
    fn the_report_accounts_for_every_program() {
        let (_, report) = subset(whole_face_document(&[(40.0, "Handgloves")]));
        assert!(report.bytes_after() < report.bytes_before());
        assert_eq!(
            report.bytes_before(),
            report.subsetted.iter().map(|s| s.before).sum::<usize>()
        );

        let (_, none) = subset(with_default_resources(whole_face_document(&[(
            40.0,
            "Handgloves",
        )])));
        assert_eq!(
            none.bytes_before(),
            none.bytes_after(),
            "a document where nothing was cut carries the same bytes"
        );
    }

    /// A document that embeds nothing reports nothing — not an error, and not
    /// a refusal either.
    #[test]
    fn a_document_with_no_embedded_program_reports_nothing() {
        let mut builder = DocumentBuilder::new();
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"Helvetica", 24.0, 20.0, 40.0, "Handgloves");
        });
        let (_, report) = subset(builder.finish());
        assert_eq!(report, SubsetReport::default());
    }
}

/// The four refusals that are about the **document** rather than about the
/// face, each with a fixture: a reason nothing can reach is not a guard.
#[cfg(test)]
mod document_refusals {
    use super::tests_support::*;
    use super::*;

    /// A font written straight into `/Resources /Font` has no object, so
    /// nothing can key its glyphs — and a program only it embeds would
    /// otherwise be left whole with the report saying nothing at all about it.
    #[test]
    fn a_font_written_directly_into_the_resources_is_left_whole_and_reported() {
        let built = whole_face_document(&[(40.0, "Handgloves")]);
        let doc = open(built.clone());
        let whole = only_program(&doc).len();
        let font = only_font(&doc);
        let inline = doc
            .get(font)
            .expect("the font object")
            .as_dict()
            .cloned()
            .expect("a dictionary");

        // The same dictionary, by value, and the object itself deleted so that
        // the sweep cannot reach the program the other way.
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let page = editor.page_refs()[0];
        let Some(Object::Dict(mut page_dict)) = editor.get(page) else {
            panic!("the page is a dictionary");
        };
        let mut fonts = Dict::new();
        fonts.insert(editor.intern(b"F0"), Object::Dict(inline));
        let mut resources = Dict::new();
        resources.insert(editor.intern(b"Font"), Object::Dict(fonts));
        page_dict.insert(editor.intern(b"Resources"), Object::Dict(resources));
        editor.put(page, Object::Dict(page_dict));
        editor.delete(font);

        let (after, report) = subset(saved(&editor));
        assert_eq!(report.subsetted, Vec::new());
        assert_eq!(report.untouched.len(), 1, "{:?}", report.untouched);
        assert_eq!(report.untouched[0].reason, UntouchedReason::NotAnObject);
        assert_eq!(report.untouched[0].bytes, whole);
        assert_eq!(
            only_program(&open(after)).len(),
            whole,
            "the program went through as it was"
        );
    }

    /// A font object no walked resource dictionary names. "No glyphs were
    /// shown through it" is then ignorance rather than a measurement, and
    /// cutting on it would empty a program some scope this pass never opened
    /// still draws from.
    #[test]
    fn a_font_no_walked_scope_names_is_left_whole_and_reported() {
        let built = whole_face_document(&[(40.0, "Handgloves")]);
        let doc = open(built);
        let whole = only_program(&doc).len();

        // The font object stays, with its descriptor and its program; only the
        // page's reference to it goes.
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let page = editor.page_refs()[0];
        let Some(Object::Dict(mut page_dict)) = editor.get(page) else {
            panic!("the page is a dictionary");
        };
        let mut resources = Dict::new();
        resources.insert(editor.intern(b"Font"), Object::Dict(Dict::new()));
        page_dict.insert(editor.intern(b"Resources"), Object::Dict(resources));
        editor.put(page, Object::Dict(page_dict));

        let (after, report) = subset(saved(&editor));
        assert_eq!(report.subsetted, Vec::new());
        assert_eq!(report.untouched.len(), 1, "{:?}", report.untouched);
        assert_eq!(report.untouched[0].reason, UntouchedReason::ScopeNotWalked);
        assert_eq!(only_program(&open(after)).len(), whole);
    }

    /// 9.6.5 lets a Type 3 font carry resources for its glyph procedures. This
    /// engine's interpreter runs a procedure in the **enclosing** scope, so a
    /// `/F0` inside a procedure is credited to the enclosing `/F0` and the
    /// Type 3 font's own `/F0` is credited with nothing — which would empty
    /// its program.
    #[test]
    fn a_font_a_type3_fonts_own_resources_name_is_left_whole_and_reported() {
        let built = whole_face_document(&[(40.0, "Handgloves")]);
        let doc = open(built);
        let whole = only_program(&doc).len();
        let embedded = only_font(&doc);

        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let procedure = editor.allocate();
        editor.put_stream(
            procedure,
            StreamData {
                dict: Dict::new(),
                data: b"1000 0 d0 BT /F0 10 Tf (a) Tj ET".to_vec(),
            },
        );

        let mut procedures = Dict::new();
        procedures.insert(editor.intern(b"g"), Object::Ref(procedure));
        let mut inner_fonts = Dict::new();
        inner_fonts.insert(editor.intern(b"F0"), Object::Ref(embedded));
        let mut inner_resources = Dict::new();
        inner_resources.insert(editor.intern(b"Font"), Object::Dict(inner_fonts));

        let type3 = editor.allocate();
        let mut dict = Dict::new();
        dict.insert(editor.intern(b"Type"), Object::Name(editor.intern(b"Font")));
        dict.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(b"Type3")),
        );
        dict.insert(
            editor.intern(b"FontMatrix"),
            Object::Array(vec![
                Object::Real(0.001),
                Object::Int(0),
                Object::Int(0),
                Object::Real(0.001),
                Object::Int(0),
                Object::Int(0),
            ]),
        );
        dict.insert(editor.intern(b"CharProcs"), Object::Dict(procedures));
        dict.insert(editor.intern(b"Resources"), Object::Dict(inner_resources));
        editor.put(type3, Object::Dict(dict));

        // The page names the Type 3 font; the embedded face is reachable only
        // through that font's own `/Resources`.
        let page = editor.page_refs()[0];
        let Some(Object::Dict(mut page_dict)) = editor.get(page) else {
            panic!("the page is a dictionary");
        };
        let mut fonts = Dict::new();
        fonts.insert(editor.intern(b"T3"), Object::Ref(type3));
        let mut resources = Dict::new();
        resources.insert(editor.intern(b"Font"), Object::Dict(fonts));
        page_dict.insert(editor.intern(b"Resources"), Object::Dict(resources));
        editor.put(page, Object::Dict(page_dict));

        let (after, report) = subset(saved(&editor));
        assert_eq!(report.subsetted, Vec::new());
        assert_eq!(report.untouched.len(), 1, "{:?}", report.untouched);
        assert_eq!(report.untouched[0].reason, UntouchedReason::Type3Resource);
        assert_eq!(only_program(&open(after)).len(), whole);
    }

    /// 9.6.6.4 ends its selection order with a **guess** — read the code as
    /// the glyph index — which is what every reader does with a subset font
    /// whose producer dropped the `cmap`, and which is not a statement the
    /// font made. Two readers are free to guess differently, so a glyph
    /// dropped on the strength of one guess is a glyph the other reader draws
    /// and no longer has.
    ///
    /// The fixture renames the vendored face's `cmap` tag to `xmap` in its
    /// table directory: four bytes, every offset still valid, and the result
    /// is a real TrueType program with no character mapping — which is exactly
    /// what a producer's own subset often is.
    #[test]
    fn a_font_whose_codes_resolve_only_by_guess_is_left_whole_and_reported() {
        let blinded = without_cmap(&face());
        assert!(
            Sfnt::parse(&blinded)
                .expect("still an sfnt")
                .table(0x636D_6170)
                .is_none(),
            "the face no longer carries a cmap"
        );

        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"Blinded", &blinded));
        builder.add_page(300.0, 100.0, |page| {
            page.text(b"F0", 24.0, 20.0, 40.0, "Handgloves");
        });

        let (after, report) = subset(builder.finish());
        assert_eq!(report.subsetted, Vec::new());
        assert_eq!(report.untouched.len(), 1, "{:?}", report.untouched);
        assert_eq!(report.untouched[0].reason, UntouchedReason::CodeNotMapped);
        assert_eq!(
            only_program(&open(after)).len(),
            blinded.len(),
            "the program went through as it was"
        );
    }
}

/// Two things that are only visible when a document has more than one font,
/// or more than one subsetting pass.
#[cfg(test)]
mod identity_and_naming {
    use super::tests_support::*;
    use super::*;

    /// Glyph usage is keyed by the font's **object**, not by the resource name
    /// [`tinker_pdf_content::Glyph::font_id`] carries. Two fonts in one scope
    /// must each keep their own glyphs and neither the other's — merging them
    /// gives one font glyphs it does not need, which is merely wasteful, and
    /// the other none of the ones it does, which is a blank on the page.
    #[test]
    fn two_fonts_in_one_scope_each_keep_their_own_glyphs() {
        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"LiberationSerif", &face()));
        assert!(builder.add_embedded_font(b"F1", b"LiberationSans", &sans_face()));
        builder.add_page(300.0, 100.0, |page| {
            page.text(b"F0", 24.0, 20.0, 60.0, "aaa");
            page.text(b"F1", 24.0, 20.0, 20.0, "zzz");
        });

        let (after, report) = subset(builder.finish());
        assert_eq!(report.subsetted.len(), 2, "{report:?}");
        let doc = open(after);
        let serif = program_named(&doc, b"LiberationSerif");
        let sans = program_named(&doc, b"LiberationSans");

        assert!(draws(&serif, glyph_of(&serif, 'a')), "the serif drew 'a'");
        assert!(
            !draws(&serif, glyph_of(&serif, 'z')),
            "'z' was never drawn in the serif and its outline is still there"
        );
        assert!(draws(&sans, glyph_of(&sans, 'z')), "the sans drew 'z'");
        assert!(
            !draws(&sans, glyph_of(&sans, 'a')),
            "'a' was never drawn in the sans and its outline is still there"
        );
    }

    /// A `/BaseFont` that already carries a 9.6.4 tag gets that tag
    /// **replaced**, not a second one stacked in front of it: the old tag named
    /// the set of glyphs the producer kept, which is not the set this pass
    /// kept, and `ABCDEF+GHIJKL+Times` is not a name 9.6.4 describes.
    #[test]
    fn an_existing_subset_tag_is_replaced_rather_than_stacked() {
        let built = whole_face_document(&[(40.0, "Handgloves")]);
        let doc = open(built);
        let font = only_font(&doc);

        // The producer's own tag, as a document that arrived already subset
        // would carry it.
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let tagged = editor.intern(b"AAAAAA+LiberationSerif");
        let Some(Object::Dict(mut dict)) = editor.get(font) else {
            panic!("the font is a dictionary");
        };
        dict.insert(editor.intern(b"BaseFont"), Object::Name(tagged));
        editor.put(font, Object::Dict(dict));

        let (after, report) = subset(saved(&editor));
        let named = report.subsetted[0].base_font.clone();
        assert_eq!(
            named.matches('+').count(),
            1,
            "{named:?} carries more than one tag"
        );
        assert!(named.ends_with("+LiberationSerif"), "{named:?}");
        assert_ne!(&named[..6], "AAAAAA", "the old tag named the old glyph set");

        let doc = open(after);
        for (_, dict) in font_dicts(&doc) {
            let name = base_font(&doc, &dict);
            assert_eq!(String::from_utf8_lossy(&name), named);
        }
    }
}
