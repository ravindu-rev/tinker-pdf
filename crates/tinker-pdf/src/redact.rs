//! Redaction (phase 10).
//!
//! The whole point is that the content is **gone**, not covered. A black
//! rectangle drawn over text leaves the text in the content stream, where any
//! extractor finds it — that is how redaction failures reach the news.
//!
//! So this rewrites the content stream itself: every text-showing operator
//! whose glyphs fall inside a redaction rectangle has those glyphs removed and
//! is re-emitted with a displacement in their place, so the surviving text on
//! the line keeps its position.
//!
//! It lives in the facade rather than in `cos` because deciding *which* glyphs
//! a rectangle covers needs both the content tokenizer and the font metrics,
//! and the leaf crates do not depend on each other (ruling 8).
//!
//! The acceptance test is not "does it look right" but "decompress every
//! stream in the output and assert the needle bytes are absent" — and, since
//! September 2026, "render the redacted page and assert there is no ink inside
//! the rectangle". A stream check alone would pass a build that left the
//! glyph in an untouched duplicate stream; an ink check alone would pass the
//! black-rectangle non-redaction this module exists to refuse. Neither half
//! is the property. Both together are.
//!
//! # What this module does not remove
//!
//! The **glyphs**. This removes the text; the embedded font program still
//! carries an outline for every character the producer embedded, and a face
//! the producer had already subset to the characters its document used names
//! the redacted ones exactly: a `glyf` with entries for nothing but `J`, `o`,
//! `h`, `n`, `S`, `m`, `i`, `t` and `h` says what the redaction was for.
//! Neither acceptance test above sees it — there is no needle in a stream and
//! no ink on the page.
//!
//! [`crate::subset::apply`] is the pass that cuts them out, and
//! [`crate::write::save`] is the door that runs it **by default**, so the
//! ordinary way of writing a redacted document out is one that does not carry
//! them. Saving through [`tinker_pdf_cos::DocumentEditor::save`] instead does
//! not, and does not claim to; [`crate::SubsetOutcome::removed`] is how a
//! caller asks whether this file is finished.
//!
//! # The cut happens in the run's own frame
//!
//! A redaction rectangle is given in page space. A glyph is placed in *text*
//! space, by the text matrix and the transformation matrix in force, and
//! those two carry rotation and skew as readily as they carry a translation.
//! Until September 2026 this module modelled both as `(sx, sy, tx, ty)` and
//! refused any run whose `b` or `c` was non-zero, because a pen that walks
//! along one axis cannot follow a run that does not.
//!
//! Both matrices are now carried whole ([`Matrix`]), which makes three things
//! true at once:
//!
//! - **Coverage is exact.** A glyph's box is a parallelogram in page space,
//!   and whether it meets the rectangle is a separating-axis test
//!   ([`quad_meets_rect`]) rather than a comparison of two intervals.
//! - **The replacement displacement needs no frame of its own.** A `TJ`
//!   number displaces the pen along the *baseline* — 9.4.3 applies it before
//!   the text matrix — so the gap a removed glyph leaves rotates with the run
//!   for free. That is what "cut along its own baseline" turns out to mean in
//!   a content stream: not new geometry, but the one displacement operator
//!   that was already defined in the run's own frame.
//! - **Nothing is re-anchored.** The surviving sub-runs keep the original
//!   run's text matrix, untouched, byte for byte. See [`emit_array`] for why
//!   the alternative — a fresh `Tm` per sub-run — is the plausible-looking
//!   wrong answer.
//!
//! # What still refuses, and why that is not a formality
//!
//! A redaction that silently fails to redact is worse than one that refuses:
//! the caller believes the content is gone and distributes the file. So a run
//! this module cannot *measure* is left whole and named in
//! [`RedactionReport::warnings`], rather than cut from positions that are
//! approximately right. Two classes qualify, and neither has an exit — a run
//! with no metrics, or with positions that are not numbers, has nothing to
//! measure:
//!
//! | Class | Why it cannot be measured |
//! | --- | --- |
//! | [`RedactionWarning::UnknownFont`] | no metrics at all: the `Tf` named a font the resource dictionary in scope does not have |
//! | [`RedactionWarning::UnmeasurableFrame`] | a non-finite entry in the text or transformation matrix, or a position that has run away to infinity |
//!
//! A warning says the run was **not measured**, not that it was covered: this
//! module cannot know whether an unmeasurable run fell under a rectangle,
//! which is the whole reason it will not cut one. That is why warnings are
//! raised only when there is at least one rectangle to fall under.
//!
//! # Vertical writing
//!
//! A fourth class, `VerticalRun`, was refused until September 2026 and is
//! measured now. 9.4.4's vertical branch is a different formula rather than a
//! different matrix — the pen walks text-space **y** by `/W2`'s `w1`, which is
//! signed and carries no horizontal scale, the glyph is drawn with its
//! horizontal origin at minus the position vector `v`, and a `TJ` number
//! displaces along y in thousandths of `Tfs` alone — so [`Pen`] carries one
//! position along the run's own axis and asks the font's writing mode which
//! axis that is. The box is the horizontal one stood on end
//! ([`Pen::glyph_box`]), the replacement gap is emitted in the vertical
//! thousandth ([`Pen::thousandth`]), and nothing else changes: a vertical run
//! is cut by the same separating-axis test, under the same rotated, skewed
//! or scaled matrices, as a horizontal one.
//!
//! The class had hidden behind the rotation refusal, whose matrix test a
//! vertical run passes, and was being cut *horizontally* before it was
//! refused; the variant is gone because nothing raises it.
//!
//! # A Type 3 font's own glyph space
//!
//! A third class, `RescaledType3Font`, was refused until September 2026 and
//! is measured now. 9.6.5 puts a Type 3 font's `/Widths` in *its own* glyph
//! space, which `/FontMatrix` maps to text space; this module divided by
//! 1000, which is right for the conventional matrix and wrong by exactly the
//! matrix for any other. [`GlyphSpace`] reads the matrix whole: the advance
//! is the horizontal component of the width carried through it, and the
//! glyph box is a glyph-space rectangle carried through all six numbers, so
//! a skewed or rotated glyph space is cut where its procedures draw rather
//! than where an upright em would have been.
//!
//! # A form drawn twice
//!
//! A form XObject drawn in two places is two placements of **one stream**
//! (8.10), and until September 2026 only the first was ever measured: a
//! `visited` set keyed by the object was there to stop a self-referential
//! form recursing forever, and it stopped the second `Do` as well. A
//! rectangle over the second placement was tested against nothing, the text
//! stayed, and the report said `glyphs: 0` with no warning — indistinguishable
//! from a rectangle that covered nothing.
//!
//! The guard was then keyed by the object **and** the transform in force
//! ([`placement_key`]), so every placement was measured — and cut, in the one
//! stream they share, so a glyph a rectangle covered at one placement was
//! gone at all of them. `RedactionWarning::RepeatedForm` named that widened
//! cut. What bounds a matrix that creeps by an ulp a round is still
//! [`MAX_PLACEMENTS`] rather than any comparison of floats, because two
//! transforms an ulp apart are two placements and calling them one would be a
//! decision not to cut.
//!
//! **Now each placement is cut exactly at its own rectangles.** [`Walk`]
//! measures every placement against the form as it was, without writing
//! anything; [`settle`] then gives each distinct outcome a stream of its own
//! — a copy of the form, cut in that placement's frame — and points each
//! `Do` at its placement's stream through a fresh resource name
//! ([`with_names`]). A form whose placements cut the same shares one stream,
//! and a form nothing was cut from is not written at all. The form's own
//! object keeps an uncut outcome when there is one, so another page that
//! draws the form draws it as it was; when every placement cut something it
//! takes the first placement's outcome, so it is still drawn by this page and
//! is never left in the file holding what a rectangle covered with nothing
//! drawing it. [`crate::subset`] walks the editor's
//! [`view`](tinker_pdf_cos::DocumentEditor::view), where the copies resolve,
//! so a glyph drawn only in a copy stays in the program.
//!
//! Two kinds of form still go the old way — every placement's cut in the one
//! stream, named by [`RedactionWarning::RepeatedForm`] when that was wider
//! than a placement asked for — and everything they draw goes with them
//! ([`settle`] says why): a form that draws itself, directly or through
//! another, where a copy per placement would be a copy per round of a
//! recursion; and a form with a placement past [`MAX_PLACEMENTS`], which was
//! never measured, so no copy could say what it should hold.
//!
//! The same guard covered images, with the same hole: an image drawn twice
//! and covered only at its second placement was left whole and reported
//! `images: 0`. An image is replaced whole or not at all, so it needs no copy
//! and no warning — every placement is tested and the first covered one
//! scrubs it, at all of them.
//!
//! # The injections that were counted
//!
//! Each defect below was reintroduced on its own and
//! `cargo test --no-fail-fast -p tinker-pdf` run over the result, on
//! 15 September 2026, against a suite of 1 508 tests. The flag matters: a
//! plain `cargo test` stops at the first failure and undercounts. Every one
//! of them is caught, and the two caught by exactly one test are caught by the
//! test that was written for them.
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the rotation ignored, so the cut is axis-aligned | 9 |
//! | the cut applied in page space rather than in the run's own frame | 21 |
//! | a partly covered glyph kept, coverage requiring containment | 13 |
//! | the replacement displacement dropped | 5 |
//! | a fresh `Tm` per cut run, reconstructed and rounded | **1** |
//! | the refusal removed for a vertical run | 2 |
//! | the refusal removed for a rescaled Type 3 glyph space | 2 |
//! | the warning not emitted when a run is skipped | 6 |
//! | the non-showing half of `'` and `"` dropped from a cut run | 2 |
//! | an existing `TJ` adjustment re-emitted with its sign flipped | **1** |
//!
//! The form-placement defects were counted the same way on
//! 20 September 2026, against a suite of 1 593 tests, and none of the seven
//! reports zero:
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the placement key built from `a b c d` only, so two placements one `cm` apart read as one — the defect above, put back | 4 |
//! | the form's content read from the file rather than from the editor, so each placement's pass undoes the one before it | 3 |
//! | the image deduplicated by object number *before* the coverage test, which is how it used to be | **1** |
//! | [`RedactionWarning::RepeatedForm`] never raised | 4 |
//! | `RepeatedForm` raised for any form at two placements, cut or not | 2 |
//! | the placement key forgetting the object number, so two different forms at one placement collide | **1** |
//! | [`MAX_PLACEMENTS`] removed, so a form at more placements than the cap no longer saturates its count | **1** |
//!
//! The three caught by exactly one test are each caught by the test written
//! for them, which is what a count of one is supposed to mean here.
//!
//! The vertical and glyph-space defects were counted on 26 September 2026,
//! over `cargo test --no-fail-fast -p tinker-pdf --lib` (every redaction
//! test lives in the lib), 308 and then 315 tests. None reports zero, and
//! every count of one is the test written for that defect:
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | a vertical advance read with the horizontal formula, from `/W` | 5 |
//! | `w1` negated, so the column runs upward | 5 |
//! | the vertical thousandth carrying `Th` | **1** |
//! | a vertical box measured rightward from the pen, as a horizontal one is | **1** |
//! | a Type 3 advance divided by 1000, the old formula | 3 |
//! | a Type 3 box from the matrix's `a` and `d` alone, upright and untranslated | 3 |
//! | a Type 3 box without the matrix's translation | **1** |
//! | `/FontBBox` alone, not joined with the em | **1** |
//! | `/FontBBox`'s bottom ignored | **1** |
//! | the font selected inside a `q` surviving its `Q` | **1** |
//!
//! The copy-per-placement defects were counted the same way the same day,
//! over 323 tests. The first campaign found one zero: a guard that marked a
//! form met again on its own recursion stack as cyclic fired nothing when
//! removed, because [`settle`]'s ordering already refuses a form that links
//! to itself; the guard was deleted rather than kept for a count, and the
//! injection that removes what does the work is the one below.
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the page's `Do`s never pointed at copies | 8 |
//! | each placement cut from what the one before it left, which is how it used to be | 7 |
//! | the form's own object given to the first placement even when another is uncut | **1** |
//! | a copy per placement rather than per outcome | **1** |
//! | a form decided before the forms it draws | **1** |
//! | a form that draws itself neither ordered nor cut the old way | **1** |
//! | a form placed past the cap cut exactly | 2 |
//! | the old way not carried down to what such a form draws | **1** |
//! | a form's content read from the file rather than from the editor | 2 |
//! | [`crate::subset`]'s walk put back over the file, with the editor's bytes for a rewritten stream | **1** |

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use tinker_pdf_content::{Token, Tokenizer};
use tinker_pdf_cos::{
    font as cos_font, CosDocument, Dict, DocumentEditor, Font, Name, ObjRef, Object, Rect, Resolve,
    StreamData,
};

/// What a redaction covers and how it is marked.
#[derive(Clone, Copy, Debug)]
pub struct Redaction {
    /// The area to clear, in unrotated page space.
    pub area: Rect,
    /// Whether to paint the area black after clearing it.
    ///
    /// Cosmetic: the content is already gone by the time this draws. Its value
    /// is telling a reader that something was removed, rather than leaving a
    /// gap that reads as though nothing was ever there.
    pub mark: bool,
}

/// Something a redaction could not do exactly, named rather than left silent
/// (ruling 10).
///
/// Two of the three are a **run left whole** because this module could not
/// measure it, and each names the resource name of the font in force and how
/// many bytes of showing operand were left in place, because "a run was
/// skipped" with neither is a sentence a caller cannot act on — and this is
/// the leniency that is invisible from outside, since what it costs is
/// content the caller believes was removed.
///
/// `bytes` rather than glyphs: a run whose font is unknown cannot be decoded
/// into glyphs at all, and a count that is a guess for one variant and a
/// measurement for the other is a count nobody can compare. Warnings with the
/// same cause and the same resource are merged, so a page of text in a font
/// that is not in scope yields one entry per font rather than one per
/// operator.
///
/// Two more, `VerticalRun` and `RescaledType3Font`, existed until September
/// 2026 and are gone because nothing raises them: vertical runs and a Type 3
/// font's own glyph space are measured now (the module's "Vertical writing"
/// and "A Type 3 font's own glyph space").
///
/// The third, [`RedactionWarning::RepeatedForm`], is the other direction and
/// is the reason this type is no longer only about runs left whole: it says a
/// cut was made *wider* than the rectangles asked for, which since September
/// 2026 happens only to a form that draws itself or is placed past
/// [`MAX_PLACEMENTS`] — every other form drawn twice is cut exactly, a copy
/// per placement that needs one. Both are leniencies
/// and both are things a caller must be told, so both live here; use
/// [`RedactionWarning::resource`] to name whichever of the two kinds of
/// resource a warning is about, since [`RedactionWarning::font`] and
/// [`RedactionWarning::bytes`] have nothing to say about a form.
///
/// Closed rather than `#[non_exhaustive]`, for `WarningKind`'s reason: a new
/// class this module will not do exactly is a deliberate change to documented
/// behaviour, and a caller matching exhaustively should be made to notice it
/// rather than fall through an arm that says "some other reason".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedactionWarning {
    /// The `Tf` in force named a font that the resource dictionary in scope
    /// does not contain, so the run has no metrics and no glyph can be
    /// placed. `font` is empty when no `Tf` preceded the showing operator.
    UnknownFont {
        /// The resource name the `Tf` named.
        font: Vec<u8>,
        /// How many bytes of showing operand were left in place.
        bytes: usize,
    },
    /// The text rendering matrix carries a non-finite entry, or the pen has
    /// run away to a position that is no longer a number. Nothing can be
    /// measured against a rectangle from there.
    ///
    /// The whole showing operand is left, never half of it: a run this module
    /// abandons part-way through would be a run cut from positions it had
    /// already decided it could not trust.
    UnmeasurableFrame {
        /// The resource name of the font.
        font: Vec<u8>,
        /// How many bytes of showing operand were left in place.
        bytes: usize,
    },
    /// One form XObject is drawn at more than one placement, it could not be
    /// given a copy per placement, and the redaction cut it — so the cut is
    /// **wider** than the rectangles asked for.
    ///
    /// A form is ordinarily cut exactly: each placement that cuts differently
    /// draws a copy of the form cut in its own frame, and this is not raised
    /// (the module's "A form drawn twice"). Two kinds go the old way instead,
    /// with everything they draw: a form that draws itself, directly or
    /// through another, and one with a placement past [`MAX_PLACEMENTS`].
    /// Every placement of such a form is still measured against the
    /// rectangles — a glyph under a rectangle at any of them is removed,
    /// which is what keeps a second placement from leaking — but the removal
    /// happens in the one stream all of them share (8.10: a `Do` executes one
    /// stream), so a glyph cut because the rectangle covered it at one
    /// placement is also gone at placements no rectangle touched.
    ///
    /// Over-removal is the direction this module errs in everywhere (a partly
    /// covered glyph goes whole, a partly covered image goes whole), because
    /// the alternative is the leak. But it is not free, and it is not
    /// something a caller can see from `glyphs` alone — so it is named here.
    ///
    /// `placements` **saturates** at [`MAX_PLACEMENTS`]. A form drawn at more
    /// than that many distinct transforms has the placements past the cap
    /// measured against nothing at all, which is the one case where this
    /// warning still means text may have *survived* under a rectangle rather
    /// than only that too much went; a saturated count is how to tell.
    RepeatedForm {
        /// The resource name the `Do` gave the form.
        form: Vec<u8>,
        /// How many distinct placements of it were measured, saturating at
        /// [`MAX_PLACEMENTS`].
        placements: usize,
    },
}

impl RedactionWarning {
    /// The resource name of the font the run was showing in.
    ///
    /// Empty for [`RedactionWarning::RepeatedForm`], which is about a form
    /// XObject and not about a font. [`RedactionWarning::resource`] is the
    /// accessor that answers for every variant.
    #[must_use]
    pub fn font(&self) -> &[u8] {
        match self {
            RedactionWarning::UnknownFont { font, .. }
            | RedactionWarning::UnmeasurableFrame { font, .. } => font,
            RedactionWarning::RepeatedForm { .. } => &[],
        }
    }

    /// The resource name this warning is about — a font for two of the three
    /// variants, a form XObject for [`RedactionWarning::RepeatedForm`].
    ///
    /// This is what distinguishes two warnings of the same kind, so it is
    /// never empty except where the document gave no name to quote.
    #[must_use]
    pub fn resource(&self) -> &[u8] {
        match self {
            RedactionWarning::RepeatedForm { form, .. } => form,
            other => other.font(),
        }
    }

    /// How many bytes of showing operand this warning accounts for.
    ///
    /// Zero for [`RedactionWarning::RepeatedForm`], which leaves no operand
    /// in place — it counts placements instead
    /// ([`RedactionWarning::placements`]).
    #[must_use]
    pub fn bytes(&self) -> usize {
        match self {
            RedactionWarning::UnknownFont { bytes, .. }
            | RedactionWarning::UnmeasurableFrame { bytes, .. } => *bytes,
            RedactionWarning::RepeatedForm { .. } => 0,
        }
    }

    /// How many distinct placements this warning accounts for, and zero for
    /// every variant that is about a run rather than about a form.
    #[must_use]
    pub fn placements(&self) -> usize {
        match self {
            RedactionWarning::RepeatedForm { placements, .. } => *placements,
            _ => 0,
        }
    }

    /// Whether two warnings are the same cause over the same resource.
    fn same_cause(&self, other: &RedactionWarning) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
            && self.resource() == other.resource()
    }

    /// Folds another warning of the same cause into this one.
    ///
    /// Each variant absorbs its own count — operand bytes for the two run
    /// classes, placements for a form — because a single `usize` that means
    /// bytes in one arm and placements in another is a number nobody can
    /// read.
    fn absorb(&mut self, other: &RedactionWarning) {
        match self {
            RedactionWarning::UnknownFont { bytes, .. }
            | RedactionWarning::UnmeasurableFrame { bytes, .. } => {
                *bytes = bytes.saturating_add(other.bytes());
            }
            RedactionWarning::RepeatedForm { placements, .. } => {
                *placements = placements
                    .saturating_add(other.placements())
                    .min(MAX_PLACEMENTS);
            }
        }
    }
}

/// How many distinct warnings one redaction keeps.
///
/// Three causes times the resources on a page: a document that reaches this cap
/// has a resource dictionary a caller is not going to read through anyway, and
/// the counts of the ones past it are lost rather than the list growing with
/// the file (ruling 1).
const MAX_WARNINGS: usize = 64;

/// How many distinct placements of one XObject a redaction measures.
///
/// A form drawn at more than this many *different* transforms has the
/// placements past the cap measured against nothing, and
/// [`RedactionWarning::RepeatedForm`] then reports a count saturated at this
/// value, which is what tells a caller the difference.
///
/// The cap is what bounds the work, and it has to be a cap rather than a
/// float comparison. Two transforms that differ in the last ulp are not the
/// same placement and are not treated as one ([`placement_key`]), so a form
/// that invokes itself under a matrix that changes by a hair each time
/// generates a fresh placement every round; [`MAX_FORM_DEPTH`] bounds one
/// such chain and this bounds the rest (ruling 1).
pub const MAX_PLACEMENTS: usize = 64;

/// Records a warning, merging it into one with the same cause and resource.
fn note(warnings: &mut Vec<RedactionWarning>, warning: RedactionWarning) {
    for existing in warnings.iter_mut() {
        if existing.same_cause(&warning) {
            existing.absorb(&warning);
            return;
        }
    }
    if warnings.len() < MAX_WARNINGS {
        warnings.push(warning);
    }
}

/// What a redaction did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RedactionReport {
    /// How many text-showing operations had glyphs removed.
    pub operations: usize,
    /// How many glyphs were removed in total.
    pub glyphs: usize,
    /// How many images were scrubbed.
    ///
    /// An image goes whole when a redaction touches it at all: cutting a hole
    /// would mean decoding, editing and re-encoding its samples through a
    /// codec this build may have no encoder for, and leaving the rest is not a
    /// redaction.
    pub images: usize,
    /// Runs this redaction could not measure and therefore left whole.
    ///
    /// Empty is the answer a caller wants. A non-empty list means some text on
    /// the page was never tested against the rectangles at all, so whether the
    /// redaction is complete is not something this report can say.
    pub warnings: Vec<RedactionWarning>,
}

/// A two-dimensional affine transform, `[a b c d e f]` as PDF writes one.
///
/// 8.3.4's row-vector convention: a point `(x, y)` is `[x y 1]`, so it maps to
/// `(a·x + c·y + e, b·x + d·y + f)`, and `m.then(n)` is the matrix that
/// applies `m` and then `n`. That is the order every concatenation in this
/// file needs: 8.3.4 says a new transformation is **premultiplied** with the
/// one in force, which is what `cm` (8.4.4) and a form's `/Matrix` (8.10.1)
/// do, and 9.4.4 composes the text rendering matrix
/// `[Tfs·Th 0 0; 0 Tfs 0; 0 Trise 1] × Tm × CTM` the same way.
///
/// 8.3.4 and not 8.3.3: 8.3.3 is "Common transformations", which gives the
/// four named matrices, and 8.3.4 is "Transformation matrices", which is where
/// the vector form and the multiplication order are. The numbering is the same
/// in ISO 32000-1:2008 and ISO 32000-2:2020.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A pure translation, which is what `Td` and `T*` concatenate (9.4.2).
    fn translate(tx: f64, ty: f64) -> Matrix {
        Matrix {
            e: tx,
            f: ty,
            ..Matrix::IDENTITY
        }
    }

    /// Six numbers as an operator wrote them.
    fn from_operands(v: &[f64]) -> Option<Matrix> {
        match v {
            [a, b, c, d, e, f, ..] => Some(Matrix {
                a: *a,
                b: *b,
                c: *c,
                d: *d,
                e: *e,
                f: *f,
            }),
            _ => None,
        }
    }

    /// `self`, then `other`.
    fn then(self, other: Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Where a point lands.
    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Whether every entry is a number.
    fn is_finite(self) -> bool {
        self.a.is_finite()
            && self.b.is_finite()
            && self.c.is_finite()
            && self.d.is_finite()
            && self.e.is_finite()
            && self.f.is_finite()
    }

    /// Whether the transform rotates or skews.
    fn is_turned(self) -> bool {
        self.b.abs() > 1e-9 || self.c.abs() > 1e-9
    }
}

/// Applies redactions to one page of an open editor.
///
/// Returns what was removed, or `None` when the page does not exist. Applying
/// an empty list, or one that covers no text, is a no-op that still reports
/// success — a redaction that finds nothing is not an error.
pub fn apply(
    editor: &mut DocumentEditor,
    page: u32,
    areas: &[Redaction],
) -> Option<RedactionReport> {
    // The page **as this editor has it**: its place in the editor's page
    // order, its content as the editor now holds it, and its resources read
    // through the editor. See [`EditorPage`] for the three ways reading the
    // file instead went wrong.
    let reference = editor.page_refs().get(page as usize).copied()?;
    let EditorPage {
        content,
        existing,
        resources,
    } = EditorPage::read(editor, reference)?;
    let fonts = fonts_in(editor.document(), &resources);

    let (data, mut report, uses) = rewrite(&content, areas, &fonts, Matrix::IDENTITY);

    // 8.10: a form XObject holds content like any other, and a redaction that
    // stops at the page stream leaves whatever a form drew exactly where it
    // was. Images the redaction covers are scrubbed for the same reason: a
    // black rectangle over a photograph removes nothing.
    //
    // Measured first and written after: every placement of a form is cut
    // from the form as it was, and only once all of them are known is it
    // decided which placements share a stream and which need a copy of their
    // own ([`settle`]).
    let mut walk = Walk::default();
    let children = walk.uses(editor, &resources, &uses, areas, &mut report, 0);
    let targets = settle(editor, &walk, areas, &mut report);

    // The page's own `Do`s that draw a copy name it by a resource name the
    // page did not have, so the page gets a resources dictionary of its own
    // that carries one.
    let renames = renames_of(&children, &uses, &targets);
    let (mut data, scope) = if renames.is_empty() {
        (data, None)
    } else {
        let (data, scope) = with_names(editor, &data, &resources, &renames);
        (data, Some(scope))
    };

    if areas.iter().any(|r| r.mark) {
        // Painted last, so it covers whatever remains beneath it.
        data.extend_from_slice(b"q 0 g\n");
        for area in areas.iter().filter(|r| r.mark).map(|r| r.area) {
            let line = format!(
                "{} {} {} {} re f\n",
                area.x0,
                area.y0,
                area.x1 - area.x0,
                area.y1 - area.y0
            );
            data.extend_from_slice(line.as_bytes());
        }
        data.extend_from_slice(b"Q\n");
    }

    // The redacted stream **overwrites** the object the old one lived in, and
    // any further parts of a split `/Contents` array are deleted outright.
    //
    // Writing to a freshly allocated object instead would leave the original
    // bytes sitting in the file, unreferenced but perfectly readable — which
    // is the exact failure this whole module exists to prevent, and which the
    // decompress-every-stream test caught when it was written that way.
    let content_ref = match existing.split_first() {
        Some((first, rest)) => {
            for &stale in rest {
                editor.delete(stale);
            }
            *first
        }
        None => editor.allocate(),
    };

    editor.put_stream(
        content_ref,
        StreamData {
            dict: Dict::new(),
            data,
        },
    );

    let Some(Object::Dict(mut dict)) = editor.get(reference) else {
        return None;
    };
    let contents = editor.intern(b"Contents");
    dict.insert(contents, Object::Ref(content_ref));
    // Direct, and on this page alone: the dictionary it replaces may be
    // inherited from the page tree or shared by other pages (7.7.3.4), none
    // of which draws the copies.
    if let Some(scope) = scope {
        dict.insert(Name::RESOURCES, Object::Dict(scope));
    }
    editor.put(reference, Object::Dict(dict));

    Some(report)
}

/// One page, read through the editor rather than out of the file.
///
/// Until September 2026 [`apply`] read the page with `pages::collect` and
/// `content_bytes` over [`DocumentEditor::document`] — the file as it was
/// opened — and three things followed, each an under-redaction:
///
/// - **A second redaction of the same page undid the first.** It read the
///   file's content, cut its own rectangles out of that, and overwrote the
///   stream the first redaction had written: the text the first one removed
///   was back, and its report still said it was gone.
/// - **A page the editor had moved was read from the wrong place.** The
///   file's page *n* is not the editor's page *n* after `move_page`, so the
///   redaction read one page's content and wrote it over another's stream.
/// - **Inherited resources were not read.** `/Resources` was taken from the
///   page dictionary alone, and 7.7.3.4 lets it be inherited from the page
///   tree: every run on such a page was `UnknownFont`, which at least said
///   so, and every form and image on it was silently not followed at all.
struct EditorPage {
    /// The content streams, decoded and joined as the editor has them.
    content: Vec<u8>,
    /// The streams `/Contents` names, in order.
    existing: Vec<ObjRef>,
    /// `/Resources`, inherited through `/Parent` when the page has none.
    resources: Dict,
}

impl EditorPage {
    fn read(editor: &DocumentEditor, reference: ObjRef) -> Option<EditorPage> {
        let Some(Object::Dict(dict)) = editor.get(reference) else {
            return None;
        };

        // 7.7.3.3: `/Contents` is one stream or an array of them, and the
        // array may itself be an indirect object.
        let existing: Vec<ObjRef> = match dict.get(Name::CONTENTS) {
            Some(Object::Ref(r)) => match editor.get(*r) {
                Some(Object::Array(items)) => items.iter().filter_map(Object::as_objref).collect(),
                _ => vec![*r],
            },
            Some(Object::Array(items)) => items.iter().filter_map(Object::as_objref).collect(),
            _ => Vec::new(),
        };
        let mut content = Vec::new();
        for part in &existing {
            if let Some(bytes) = editor.stream_bytes(*part) {
                content.extend_from_slice(&bytes);
                // 7.7.3.3: the parts divide at lexical boundaries only if
                // separated, and a producer may end one mid-token.
                content.push(b'\n');
            }
        }

        Some(EditorPage {
            content,
            existing,
            resources: inherited_resources(editor, &dict),
        })
    }
}

/// A page's `/Resources`, or the nearest ancestor's (7.7.3.4), read through
/// the editor.
///
/// The walk up `/Parent` is bounded by the same depth the page-tree walker
/// uses, so a cycle of parents ends rather than spinning.
fn inherited_resources(editor: &DocumentEditor, page: &Dict) -> Dict {
    let mut node = page.clone();
    for _ in 0..tinker_pdf_cos::limits::MAX_NEST_DEPTH {
        let resources = Resolve::resolve_key(editor, &node, Name::RESOURCES);
        if let Some(dict) = resources.as_dict() {
            return dict.clone();
        }
        let parent = Resolve::resolve_key(editor, &node, Name::PARENT);
        match parent.as_dict() {
            Some(dict) => node = dict.clone(),
            None => break,
        }
    }
    Dict::new()
}

/// A font in scope, and what this module knows about measuring it.
struct RunFont {
    font: Arc<Font>,
    /// 9.6.5: a Type 3 font's glyph space, which its `/FontMatrix` maps into
    /// text space. `None` for every other kind, whose widths are thousandths
    /// of text space by definition (9.2.4).
    glyph_space: Option<GlyphSpace>,
}

/// A Type 3 font's glyph space (9.6.5): where its `/Widths` are measured and
/// its glyph procedures draw, and the matrix that carries both into text
/// space.
///
/// Until September 2026 a Type 3 font whose `/FontMatrix` was not the 1/1000
/// default was refused as `RescaledType3Font`, because this module turned a
/// width into text space by dividing by 1000 — right for the conventional
/// matrix and wrong by exactly the matrix for any other. It is read now,
/// whole:
///
/// - **The advance** is the horizontal component of the width carried
///   through the matrix, `w0 · a`. The PDF reference's note on a Type 3
///   font's `/Widths` says so ("if `FontMatrix` specifies a rotation, only
///   the horizontal component of the transformed width is used"), and it is
///   what this engine's interpreter advances a Type 3 glyph by.
/// - **The box** is a rectangle in glyph space — `0` to `w0` along the
///   baseline, [`GlyphSpace::low`] to [`GlyphSpace::high`] across it —
///   carried through the *whole* matrix, translation included, which is how
///   the interpreter places a glyph procedure (`font_matrix.then(transform)`).
///   A skewed matrix slants the glyph and a rotated one turns it about its
///   origin, and the ink goes where the matrix sends it whichever way the
///   advance points, so a box built from `a` alone would cut the neighbour of
///   the glyph a rectangle actually covers.
#[derive(Clone, Copy)]
struct GlyphSpace {
    matrix: Matrix,
    /// The bottom of the glyph box, in glyph space: `/FontBBox`'s bottom
    /// when that is below the baseline, and the baseline otherwise.
    low: f64,
    /// The top of the glyph box, in glyph space: one em up the glyph space's
    /// own y axis — the length the matrix carries to one unit of text space,
    /// `1 / |(c, d)|`, which is 1000 for the conventional matrix — or
    /// `/FontBBox`'s top when that is higher.
    ///
    /// Joined with the em rather than taken from `/FontBBox` alone because a
    /// bounding box a producer wrote too small would shrink the box under
    /// the ink, which is the one direction a redaction may not err in; one
    /// written too large over-removes, which is the direction this module
    /// errs in everywhere.
    high: f64,
}

impl GlyphSpace {
    /// 9.6.5's conventional glyph space, the one every other font kind has
    /// implicitly.
    const DEFAULT_MATRIX: Matrix = Matrix {
        a: 0.001,
        b: 0.0,
        c: 0.0,
        d: 0.001,
        e: 0.0,
        f: 0.0,
    };

    /// The conventional glyph space with nothing known about the glyphs'
    /// extent: one em, from the baseline up.
    const DEFAULT: GlyphSpace = GlyphSpace {
        matrix: GlyphSpace::DEFAULT_MATRIX,
        low: 0.0,
        high: 1000.0,
    };

    /// Reads one font dictionary's `/FontMatrix` and `/FontBBox`.
    ///
    /// A `/FontMatrix` that is absent, or whose first six entries are not all
    /// numbers, is read as the default. 9.6.5 requires the entry, so either
    /// is malformed, and the default is how this engine's renderer reads it
    /// too: its Type 3 path (`PageResources::type3_glyph`) needs six numbers,
    /// and a font without them is advanced by `w0 / 1000` like any other — so
    /// reading it that way measures the run where it is drawn.
    fn read(doc: &CosDocument, font: &Dict) -> GlyphSpace {
        let first = |key: &[u8], count: usize| -> Option<Vec<f64>> {
            let value = doc.resolve_key(font, doc.intern(key));
            let array = value.as_array()?;
            (0..count)
                .map(|i| array.get(i).and_then(Object::as_number))
                .collect()
        };
        let matrix = first(b"FontMatrix", 6)
            .and_then(|v| Matrix::from_operands(&v))
            .unwrap_or(GlyphSpace::DEFAULT_MATRIX);

        let em = 1.0 / (matrix.c * matrix.c + matrix.d * matrix.d).sqrt();
        let em = if em.is_finite() && em > 0.0 {
            em
        } else {
            1000.0
        };
        // Table 112: four zeros mean "no assumptions are made based on the
        // font bounding box", which is the same as having none.
        let across = first(b"FontBBox", 4)
            .filter(|v| v.iter().all(|x| x.is_finite()) && v.iter().any(|x| *x != 0.0))
            .and_then(|v| Some((*v.get(1)?, *v.get(3)?)));
        let (low, high) = match across {
            Some((y0, y1)) => (y0.min(y1).min(0.0), y0.max(y1).max(em)),
            None => (0.0, em),
        };
        GlyphSpace { matrix, low, high }
    }
}

/// The fonts one resource dictionary puts in scope, by the raw name bytes.
///
/// Re-keyed by the bytes rather than by an interned [`Name`] because the
/// rewrite matches against what the `Tf` operator literally says, and it has
/// no document to intern with.
fn fonts_in(doc: &CosDocument, resources: &Dict) -> HashMap<Vec<u8>, Arc<RunFont>> {
    let spaces = glyph_spaces(doc, resources);
    cos_font::from_resources(doc, resources)
        .into_iter()
        .filter_map(|(name, font)| {
            let bytes = doc.name_bytes(name)?.to_vec();
            let glyph_space = (font.kind() == cos_font::FontKind::Type3)
                .then(|| spaces.get(&name).copied().unwrap_or(GlyphSpace::DEFAULT));
            Some((bytes, Arc::new(RunFont { font, glyph_space })))
        })
        .collect()
}

/// The glyph space each font in `/Font` declares (9.6.5).
///
/// Read here rather than through `cos_font::Font`, which carries neither
/// `/FontMatrix` nor `/FontBBox`: this module is the only caller that builds
/// a glyph box from them. Only a Type 3 font's answer is ever used.
fn glyph_spaces(doc: &CosDocument, resources: &Dict) -> HashMap<Name, GlyphSpace> {
    let mut out = HashMap::new();
    let value = doc.resolve_key(resources, doc.intern(b"Font"));
    let Some(fonts) = value.as_dict() else {
        return out;
    };

    for (key, entry) in fonts.iter() {
        let resolved = doc.resolve(entry);
        let Some(dict) = resolved.as_dict() else {
            continue;
        };
        out.insert(*key, GlyphSpace::read(doc, dict));
    }
    out
}

/// How deep form XObjects may nest before recursion is refused (8.10).
const MAX_FORM_DEPTH: u32 = 12;

/// One placement of an XObject: the six entries of the transform in force,
/// bit for bit.
///
/// **Bitwise, and deliberately not a tolerance.** Two transforms that differ
/// in the last ulp place a form in two slightly different spots, so they are
/// two placements and each has to be measured against the rectangles; a
/// tolerance that called them one would decide, by arithmetic nobody
/// specified, that a glyph a rectangle covers at one of them need not be cut.
/// Bitwise equality is exact, total and free of that judgement — and it is
/// safe to use for a *cycle guard* only because termination does not rest on
/// it: [`MAX_PLACEMENTS`] bounds how many distinct placements of one object
/// are ever entered, so a matrix that creeps by an ulp a round stops at the
/// cap rather than running forever.
///
/// `+ 0.0` normalises `-0.0` to `0.0` — the same point, two bit patterns —
/// and is exact for every other value, so it costs nothing under ruling 4.
type PlacementKey = [u64; 6];

fn placement_key(m: Matrix) -> PlacementKey {
    [
        (m.a + 0.0).to_bits(),
        (m.b + 0.0).to_bits(),
        (m.c + 0.0).to_bits(),
        (m.d + 0.0).to_bits(),
        (m.e + 0.0).to_bits(),
        (m.f + 0.0).to_bits(),
    ]
}

/// A form XObject this redaction met, as it was before anything was written.
struct FormEntry {
    reference: ObjRef,
    /// The resource name that first invoked it.
    ///
    /// The *first*, because one object can be reached by different names from
    /// different scopes and a caller needs a name that appears in the file,
    /// not a list of the ones that do.
    name: Vec<u8>,
    /// Its stream dictionary, as the editor had it.
    dict: Dict,
    /// Its decoded content, as the editor had it. Every placement is cut from
    /// **this**, never from what another placement's cut left: each
    /// placement's result is its own, and which of them share a stream is
    /// decided afterwards ([`settle`]).
    content: Vec<u8>,
    /// The distinct outcomes of cutting `content`, one per outcome rather
    /// than per placement — most placements of most forms cut nothing, and
    /// all of those share one entry.
    cuts: Vec<FormCut>,
    /// Its placements, in the order they were entered.
    nodes: Vec<usize>,
    /// A placement was refused because [`MAX_PLACEMENTS`] was reached, so at
    /// least one `Do` of it was measured against nothing.
    refused: bool,
}

/// One outcome of cutting a form's content.
struct FormCut {
    data: Vec<u8>,
    /// Where each `Do`'s operand sits in `data`, one per [`XObjectUse`] the
    /// cut recorded, so that a `Do` can be pointed at a copy afterwards.
    names: Vec<std::ops::Range<usize>>,
    glyphs: usize,
    operations: usize,
}

/// One placement of a form: which form, under which transform, and what it
/// drew.
struct FormPlacement {
    /// Index into [`Walk::forms`].
    form: usize,
    /// The transform mapping the form's space to the page's, its `/Matrix`
    /// included.
    ctm: Matrix,
    /// What the form's content names things in at this placement: its own
    /// `/Resources`, or the scope that invoked it when it has none (8.10.1).
    resources: Dict,
    /// Index into the form's [`FormEntry::cuts`].
    cut: usize,
    /// For each `Do` in the cut, the placement it made, when it drew a form
    /// this walk entered.
    children: Vec<Option<usize>>,
}

/// Every form placement one page draws, measured and not yet written.
///
/// This replaced a guard that rewrote each form in place, placement after
/// placement, so the cuts accumulated in the one stream every placement
/// shared — a glyph a rectangle covered at one placement was gone at all of
/// them, and `RepeatedForm` said so. Keyed by the object **and** the
/// transform, as that guard was ([`placement_key`]): a form that invokes
/// itself arrives back at a placement already entered, and a form drawn
/// somewhere else arrives under a different matrix and is a placement of its
/// own.
#[derive(Default)]
struct Walk {
    /// Every form met, in the order first met.
    forms: Vec<FormEntry>,
    /// Form object number to index into `forms`.
    by_number: HashMap<u32, usize>,
    /// Every placement, in the order entered.
    nodes: Vec<FormPlacement>,
    /// The placement guard: (object, transform) to placement.
    placed: HashMap<(u32, PlacementKey), usize>,
    /// Images already scrubbed, so one image is reported once however many
    /// placements asked for it.
    scrubbed: HashSet<u32>,
}

impl Walk {
    /// Follows the XObjects one stream invoked, returning for each `Do` the
    /// form placement it made, if any.
    ///
    /// A form is cut the way the page was, with the transform in force at the
    /// `Do` as its starting one — the rectangles stay in page space, so the
    /// form's own coordinates are brought into it rather than the other way
    /// round. An image the redaction covers is scrubbed here and now: it is
    /// replaced whole or not at all, so its placements need no copies.
    fn uses(
        &mut self,
        editor: &mut DocumentEditor,
        resources: &Dict,
        uses: &[XObjectUse],
        areas: &[Redaction],
        report: &mut RedactionReport,
        depth: u32,
    ) -> Vec<Option<usize>> {
        if depth > MAX_FORM_DEPTH {
            return vec![None; uses.len()];
        }
        uses.iter()
            .map(|used| self.one(editor, resources, used, areas, report, depth))
            .collect()
    }

    fn one(
        &mut self,
        editor: &mut DocumentEditor,
        resources: &Dict,
        used: &XObjectUse,
        areas: &[Redaction],
        report: &mut RedactionReport,
        depth: u32,
    ) -> Option<usize> {
        let (reference, dict) = resolve_xobject(editor, resources, &used.name)?;
        let subtype = Resolve::resolve_key(editor, &dict, editor.intern(b"Subtype"))
            .as_name()
            .and_then(|n| editor.document().name_bytes(n))
            .map(|b| b.to_vec());

        match subtype.as_deref() {
            Some(b"Image") => {
                // Tested at **every** placement, and scrubbed once. The only
                // question is whether any placement is covered, and stopping
                // at the first left an image covered only at its second in
                // the file with `images: 0`.
                if covers_unit_square(used, areas) && self.scrubbed.insert(reference.num) {
                    scrub_image(editor, reference, &dict);
                    report.images += 1;
                }
                None
            }
            Some(b"Form") => {
                let placement = Placing {
                    reference,
                    dict,
                    used,
                    scope: resources,
                };
                self.form(editor, placement, areas, report, depth)
            }
            _ => None,
        }
    }

    fn form(
        &mut self,
        editor: &mut DocumentEditor,
        placing: Placing<'_>,
        areas: &[Redaction],
        report: &mut RedactionReport,
        depth: u32,
    ) -> Option<usize> {
        let Placing {
            reference,
            dict,
            used,
            scope,
        } = placing;

        // 8.10.2: the form's own /Matrix sits between its space and the one
        // that invoked it, so it composes with the transform the `Do` was
        // made under.
        let matrix = Resolve::resolve_key(editor, &dict, editor.intern(b"Matrix"))
            .as_array()
            .map(|a| a.iter().filter_map(Object::as_number).collect::<Vec<f64>>())
            .filter(|v| v.len() >= 6 && v.iter().all(|x| x.is_finite()))
            .and_then(|v| Matrix::from_operands(&v));
        let inner = match matrix {
            Some(m) => m.then(used.ctm),
            None => used.ctm,
        };

        let form = match self.by_number.get(&reference.num) {
            Some(&index) => index,
            None => {
                // The bytes this editor has **now**, not the file's: an
                // earlier redaction of this page, or of another page that
                // draws the same form, is an edit this one must keep.
                let content = editor.stream_bytes(reference)?;
                let index = self.forms.len();
                self.forms.push(FormEntry {
                    reference,
                    name: used.name.clone(),
                    dict: dict.clone(),
                    content,
                    cuts: Vec::new(),
                    nodes: Vec::new(),
                    refused: false,
                });
                self.by_number.insert(reference.num, index);
                index
            }
        };

        // A placement already entered is the same placement again — two
        // `Do`s under one transform, or a form that invokes itself arriving
        // back where it started, which is what ends that recursion. Either
        // way the link is recorded, and a form that links to itself is one
        // [`settle`] cannot order, which is how it knows.
        let key = (reference.num, placement_key(inner));
        if let Some(&node) = self.placed.get(&key) {
            return Some(node);
        }
        let entry = self.forms.get_mut(form)?;
        if entry.nodes.len() >= MAX_PLACEMENTS {
            entry.refused = true;
            return None;
        }

        // 8.10.1: a form's own `/Resources` is what its content names things
        // in. A form that omits the dictionary inherits the scope that
        // invoked it, which is why the fallback is the caller's rather than
        // the page's.
        let inner_resources = Resolve::resolve_key(editor, &dict, Name::RESOURCES)
            .as_dict()
            .cloned()
            .unwrap_or_else(|| scope.clone());
        let fonts = fonts_in(editor.document(), &inner_resources);
        let (data, pass, inner_uses) = rewrite(&entry.content, areas, &fonts, inner);
        for warning in pass.warnings {
            note(&mut report.warnings, warning);
        }
        let cut = match entry.cuts.iter().position(|c| c.data == data) {
            Some(index) => index,
            None => {
                entry.cuts.push(FormCut {
                    data,
                    names: inner_uses.iter().map(|u| u.at.clone()).collect(),
                    glyphs: pass.glyphs,
                    operations: pass.operations,
                });
                entry.cuts.len() - 1
            }
        };

        let node = self.nodes.len();
        entry.nodes.push(node);
        self.nodes.push(FormPlacement {
            form,
            ctm: inner,
            resources: inner_resources.clone(),
            cut,
            children: Vec::new(),
        });
        self.placed.insert(key, node);

        let children = self.uses(
            editor,
            &inner_resources,
            &inner_uses,
            areas,
            report,
            depth + 1,
        );
        if let Some(placement) = self.nodes.get_mut(node) {
            placement.children = children;
        }
        Some(node)
    }
}

/// A form `Do` about to be entered.
struct Placing<'a> {
    reference: ObjRef,
    dict: Dict,
    used: &'a XObjectUse,
    /// The resources the `Do` was resolved in.
    scope: &'a Dict,
}

/// Which object a placement draws once the redaction is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The form's own object, as it was or rewritten.
    Original,
    /// A copy of the form, cut at this placement alone.
    Copy(ObjRef),
}

/// What one placement needs its stream to be: which cut, and which of its
/// `Do`s point at copies.
type Outcome = (usize, Vec<(usize, ObjRef)>);

/// Decides which object each placement draws, and writes every form stream
/// that changed.
///
/// **Each placement is cut exactly at its own rectangles.** A form drawn at
/// several placements that cut differently gets a copy per distinct outcome
/// (8.10: a `Do` executes one stream, so two outcomes need two streams), and
/// each `Do` is pointed at the stream holding its placement's outcome. Which
/// outcome keeps the form's own object is chosen so that the object is never
/// left holding anything no placement on this page draws:
///
/// - an **uncut** outcome keeps it untouched, when some placement cut nothing
///   — so every other page that draws the form draws it as it was;
/// - otherwise the **first** placement's outcome is written into it, which
///   that placement then draws.
///
/// Either way the form's own object is drawn by this page, so it never
/// becomes an unreferenced stream still holding what a rectangle covered —
/// which a copy for every placement would have made it.
///
/// Children are decided before the forms that draw them, because a form
/// whose `Do` must point at a child's copy is itself a different outcome. A
/// form this cannot order that way — one that draws itself, directly or
/// through another, or draws one that does — or one with a placement past
/// [`MAX_PLACEMENTS`] is cut the old way instead, and so is everything it
/// draws: every placement's cut in the one stream, and
/// [`RedactionWarning::RepeatedForm`] naming it when that was wider than a
/// placement asked for or when a placement went unmeasured ([`union`]). A
/// copy per placement of a form that draws itself would be a copy per round
/// of a recursion, and a placement past the cap was never measured, so no
/// copy could say what it should hold. Everything such a form draws goes
/// the old way with it because its one stream names its children by their
/// own objects: a child given copies would be drawn, through that stream,
/// from an object some other placement left uncut.
fn settle(
    editor: &mut DocumentEditor,
    walk: &Walk,
    areas: &[Redaction],
    report: &mut RedactionReport,
) -> Vec<Target> {
    let count = walk.forms.len();
    let mut targets = vec![Target::Original; walk.nodes.len()];

    // The forms each form draws.
    let mut kids: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); count];
    for node in &walk.nodes {
        for child in node.children.iter().flatten() {
            if let (Some(set), Some(c)) = (kids.get_mut(node.form), walk.nodes.get(*child)) {
                set.insert(c.form);
            }
        }
    }

    let mut old_way: Vec<bool> = walk.forms.iter().map(|f| f.refused).collect();
    close_downward(&mut old_way, &kids);

    // Children first (Kahn's algorithm over the forms still cut exactly).
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (form, set) in kids.iter().enumerate() {
        for &kid in set {
            if let Some(list) = parents.get_mut(kid) {
                list.push(form);
            }
        }
    }
    let exact = |old_way: &[bool], f: usize| !old_way.get(f).copied().unwrap_or(true);
    let mut waiting: Vec<usize> = kids
        .iter()
        .map(|set| set.iter().filter(|&&k| exact(&old_way, k)).count())
        .collect();
    let mut ready: VecDeque<usize> = (0..count)
        .filter(|&f| exact(&old_way, f) && waiting.get(f) == Some(&0))
        .collect();
    let mut order = Vec::new();
    while let Some(form) = ready.pop_front() {
        order.push(form);
        for &parent in parents.get(form).map(Vec::as_slice).unwrap_or_default() {
            if !exact(&old_way, parent) {
                continue;
            }
            if let Some(left) = waiting.get_mut(parent) {
                *left = left.saturating_sub(1);
                if *left == 0 {
                    ready.push_back(parent);
                }
            }
        }
    }
    // A form the order never reached is in a cycle, or draws one: a form
    // that invokes itself links a placement to itself or to its own
    // descendant, so it never runs out of undecided children.
    let mut ordered = vec![false; count];
    for &form in &order {
        if let Some(slot) = ordered.get_mut(form) {
            *slot = true;
        }
    }
    for (form, slot) in old_way.iter_mut().enumerate() {
        if !ordered.get(form).copied().unwrap_or(false) {
            *slot = true;
        }
    }
    close_downward(&mut old_way, &kids);

    for &form in &order {
        if exact(&old_way, form) {
            decide(editor, walk, form, &mut targets, report);
        }
    }

    // Sorted by object number, because a report that depends on the order a
    // page happened to invoke its forms in is one two equivalent files can
    // disagree about.
    let mut rest: Vec<usize> = (0..count).filter(|&f| !exact(&old_way, f)).collect();
    rest.sort_by_key(|&f| walk.forms.get(f).map_or(0, |e| e.reference.num));
    for form in rest {
        union(editor, walk, form, areas, report);
    }
    targets
}

/// Marks everything a marked form draws, at any depth.
fn close_downward(marked: &mut [bool], kids: &[BTreeSet<usize>]) {
    let mut stack: Vec<usize> = (0..marked.len())
        .filter(|&f| marked.get(f).copied().unwrap_or(false))
        .collect();
    while let Some(form) = stack.pop() {
        for &kid in kids.get(form).into_iter().flatten() {
            if let Some(slot) = marked.get_mut(kid) {
                if !*slot {
                    *slot = true;
                    stack.push(kid);
                }
            }
        }
    }
}

/// Decides one exactly-cut form's placements and writes its streams.
fn decide(
    editor: &mut DocumentEditor,
    walk: &Walk,
    form: usize,
    targets: &mut [Target],
    report: &mut RedactionReport,
) {
    let Some(entry) = walk.forms.get(form) else {
        return;
    };
    let outcomes: Vec<Outcome> = entry
        .nodes
        .iter()
        .map(|&n| {
            let Some(node) = walk.nodes.get(n) else {
                return (0, Vec::new());
            };
            let renames = node
                .children
                .iter()
                .enumerate()
                .filter_map(|(i, child)| match targets.get((*child)?) {
                    Some(Target::Copy(copy)) => Some((i, *copy)),
                    _ => None,
                })
                .collect();
            (node.cut, renames)
        })
        .collect();

    let unchanged = |outcome: &Outcome| {
        outcome.1.is_empty() && entry.cuts.get(outcome.0).is_none_or(|c| c.glyphs == 0)
    };
    let Some(home) = outcomes
        .iter()
        .find(|o| unchanged(o))
        .or_else(|| outcomes.first())
        .cloned()
    else {
        return;
    };

    // (object, the placement whose scope it is written in, its outcome)
    let mut writes: Vec<(ObjRef, usize, Outcome)> = Vec::new();
    let mut copies: Vec<(Outcome, ObjRef)> = Vec::new();
    let mut home_written = unchanged(&home);
    for (&n, outcome) in entry.nodes.iter().zip(&outcomes) {
        if *outcome == home {
            if !home_written {
                writes.push((entry.reference, n, home.clone()));
                home_written = true;
            }
            continue;
        }
        let copy = match copies.iter().find(|(o, _)| o == outcome) {
            Some((_, copy)) => *copy,
            None => {
                let copy = editor.allocate();
                copies.push((outcome.clone(), copy));
                writes.push((copy, n, outcome.clone()));
                copy
            }
        };
        if let Some(slot) = targets.get_mut(n) {
            *slot = Target::Copy(copy);
        }
    }

    for (at, n, (cut, renames)) in writes {
        let (Some(node), Some(cut)) = (walk.nodes.get(n), entry.cuts.get(cut)) else {
            continue;
        };
        // The operators are plain, so the dictionary must stop saying
        // otherwise ([`plain_stream_dict`]).
        let mut dict = plain_stream_dict(editor, &entry.dict);
        let data = if renames.is_empty() {
            cut.data.clone()
        } else {
            let renames: Vec<(std::ops::Range<usize>, ObjRef)> = renames
                .iter()
                .filter_map(|(i, copy)| Some((cut.names.get(*i)?.clone(), *copy)))
                .collect();
            let (data, scope) = with_names(editor, &cut.data, &node.resources, &renames);
            dict.insert(Name::RESOURCES, Object::Dict(scope));
            data
        };
        editor.put_stream(at, StreamData { dict, data });
        report.glyphs += cut.glyphs;
        report.operations += cut.operations;
    }
}

/// Cuts a form the old way: every placement's cut in its one stream.
///
/// For a form that draws itself, and for one with a placement past
/// [`MAX_PLACEMENTS`] — and everything either draws ([`settle`] says why).
/// Overwritten in place, for the same reason the page's content is: a freshly
/// allocated object would leave the original text in the file, unreferenced
/// and perfectly readable.
///
/// Nothing a rectangle covers at any placement survives; what that costs is
/// named rather than absorbed. A glyph removed because a rectangle covered
/// it at one placement is gone at all of them, and
/// [`RedactionWarning::RepeatedForm`] says so — raised only when a cut was
/// actually made, or when a placement went unmeasured, since a form drawn
/// twice that nothing was cut from is exact.
fn union(
    editor: &mut DocumentEditor,
    walk: &Walk,
    form: usize,
    areas: &[Redaction],
    report: &mut RedactionReport,
) {
    let Some(entry) = walk.forms.get(form) else {
        return;
    };
    let mut data = entry.content.clone();
    let mut glyphs = 0usize;
    for &n in &entry.nodes {
        let Some(node) = walk.nodes.get(n) else {
            continue;
        };
        let fonts = fonts_in(editor.document(), &node.resources);
        let (next, pass, _) = rewrite(&data, areas, &fonts, node.ctm);
        data = next;
        glyphs += pass.glyphs;
        report.operations += pass.operations;
    }
    report.glyphs += glyphs;
    if glyphs > 0 {
        let dict = plain_stream_dict(editor, &entry.dict);
        editor.put_stream(entry.reference, StreamData { dict, data });
    }

    // Only when there is a rectangle to fall under, which is the rule every
    // other warning in this module follows: with no rectangles nothing was
    // cut and nothing was widened.
    let placements = entry.nodes.len();
    if !areas.is_empty() && placements >= 2 && (glyphs > 0 || entry.refused) {
        note(
            &mut report.warnings,
            RedactionWarning::RepeatedForm {
                form: entry.name.clone(),
                placements: placements.min(MAX_PLACEMENTS),
            },
        );
    }
}

/// The `Do`s of one stream that must draw a copy, and the copy each draws.
fn renames_of(
    children: &[Option<usize>],
    uses: &[XObjectUse],
    targets: &[Target],
) -> Vec<(std::ops::Range<usize>, ObjRef)> {
    children
        .iter()
        .zip(uses)
        .filter_map(|(child, used)| match targets.get((*child)?)? {
            Target::Copy(copy) => Some((used.at.clone(), *copy)),
            Target::Original => None,
        })
        .collect()
}

/// Points `Do`s at copies: a fresh resource name for each copy, added to a
/// copy of `scope`'s `/XObject`, and written over each `Do`'s operand.
///
/// Returns the stream with the names replaced and the resources dictionary
/// that resolves them — everything in `scope` as it was, with `/XObject` a
/// direct dictionary holding the old names and the new.
///
/// The name is `Rd` and the copy's object number, which no two copies share,
/// lengthened while `scope` already uses it.
fn with_names(
    editor: &DocumentEditor,
    data: &[u8],
    scope: &Dict,
    renames: &[(std::ops::Range<usize>, ObjRef)],
) -> (Vec<u8>, Dict) {
    let key = editor.intern(b"XObject");
    let mut table = Resolve::resolve_key(editor, scope, key)
        .as_dict()
        .cloned()
        .unwrap_or_default();

    let mut chosen: HashMap<u32, Vec<u8>> = HashMap::new();
    let mut edits: Vec<(std::ops::Range<usize>, Vec<u8>)> = Vec::new();
    for (range, copy) in renames {
        let name = match chosen.get(&copy.num) {
            Some(name) => name.clone(),
            None => {
                let mut name = format!("Rd{}", copy.num).into_bytes();
                // Terminates: each round lengthens the name, and the table
                // has finitely many.
                while table.get(editor.intern(&name)).is_some() {
                    name.push(b'x');
                }
                table.insert(editor.intern(&name), Object::Ref(*copy));
                chosen.insert(copy.num, name.clone());
                name
            }
        };
        edits.push((range.clone(), name));
    }

    // Back to front, so each range still means what it meant.
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut out = data.to_vec();
    for (range, name) in edits {
        // Every range is one `rewrite` recorded around a name it wrote, so
        // it is inside the data and not empty; checked rather than trusted.
        if range.start >= range.end || range.end > out.len() {
            continue;
        }
        let mut token = Vec::with_capacity(name.len() + 1);
        token.push(b'/');
        token.extend_from_slice(&name);
        out.splice(range, token);
    }

    let mut resources = scope.clone();
    resources.insert(key, Object::Dict(table));
    (out, resources)
}

/// A stream dictionary made fit for bytes this module wrote.
///
/// [`StreamData::data`] is the stream's *encoded* bytes, and what a rewrite
/// hands over is plain operators. So every key that describes an encoding the
/// data is no longer in goes (7.3.8.2, Table 5): `/Filter` and `/DecodeParms`,
/// `/DL` (the decoded length of bytes that are gone), `/Length` (the writer
/// computes it), and the three external-file keys `/F`, `/FFilter` and
/// `/FDecodeParms`, since the data now lives in the stream rather than in a
/// file they name. Everything else — `/BBox`, `/Matrix`, `/Resources`,
/// `/Group` — is the form's and stays.
///
/// Until September 2026 a compressed form kept its `/Filter /FlateDecode` over
/// the plain operators, and the saved file carried a stream no reader can
/// decode: the form drew nothing at all, the text it was *not* asked to
/// remove included.
fn plain_stream_dict(editor: &DocumentEditor, dict: &Dict) -> Dict {
    let encoding: Vec<Name> = [
        b"Filter".as_slice(),
        b"DecodeParms",
        b"DL",
        b"Length",
        b"F",
        b"FFilter",
        b"FDecodeParms",
    ]
    .iter()
    .map(|key| editor.intern(key))
    .collect();
    dict.iter()
        .filter(|(key, _)| !encoding.contains(key))
        .cloned()
        .collect()
}

/// The reference and dictionary a resource name selects from `/XObject`.
///
/// The caller supplies the resource dictionary that is *in scope*, which is
/// the whole correctness condition here. This used to look the name up in
/// `page_refs().first()` — page zero — whatever page was being redacted and
/// however deep in a form the name appeared.
///
/// Two ways that failed, and the second is the bad one:
///
/// - Redacting page one found nothing and reported nothing, leaving the
///   content it was asked to remove in the file.
/// - Redacting page one when page zero happened to have a resource of the
///   same name — `/Im0` is the commonest name there is — scrubbed *page
///   zero's* image and left page one's. It reported `images: 1`, so it looked
///   like it had worked.
fn resolve_xobject(
    editor: &DocumentEditor,
    resources: &Dict,
    name: &[u8],
) -> Option<(ObjRef, Dict)> {
    // Through the editor, like every read here: an XObject a redaction has
    // already rewritten is the editor's, not the file's.
    let table = Resolve::resolve_key(editor, resources, editor.intern(b"XObject"));
    let reference = table.as_dict()?.get_ref(editor.intern(name))?;
    let object = editor.get(reference)?;
    Some((reference, object.as_dict()?.clone()))
}

/// Whether a redaction covers the unit square an XObject was drawn into.
///
/// 8.9.5.2: an image occupies the unit square of the transform in force. A
/// rotated or skewed transform makes that square something four numbers cannot
/// describe, and rather than guess at its extent the image is treated as
/// covered — over-removing a rotated image is the safe direction here, and it
/// is rare enough to be worth the bluntness.
///
/// [`quad_meets_rect`] could now answer this exactly, since the transform is
/// carried whole. It deliberately is not asked: making a rotated image's
/// coverage exact changes which images survive a redaction, which is a
/// decision about image content and belongs to the roadmap row that owns it
/// rather than to the one about text runs.
fn covers_unit_square(used: &XObjectUse, areas: &[Redaction]) -> bool {
    if used.ctm.is_turned() {
        return !areas.is_empty();
    }
    let x0 = used.ctm.e.min(used.ctm.e + used.ctm.a);
    let x1 = used.ctm.e.max(used.ctm.e + used.ctm.a);
    let y0 = used.ctm.f.min(used.ctm.f + used.ctm.d);
    let y1 = used.ctm.f.max(used.ctm.f + used.ctm.d);

    areas.iter().any(|redaction| {
        let a = redaction.area;
        x1 > a.x0 && x0 < a.x1 && y1 > a.y0 && y0 < a.y1
    })
}

/// Replaces an image with a single blank sample.
///
/// The samples are what has to go, so the stream is rewritten rather than
/// covered: a rectangle painted over a photograph removes nothing, and the
/// original bytes stay in the file for anyone who decompresses it.
///
/// The whole image goes even when the rectangle covers only part of it.
/// Cutting a hole would mean decoding the samples, editing them and
/// re-encoding through a codec this build may have no encoder for, and leaving
/// the rest is not a redaction.
fn scrub_image(editor: &mut DocumentEditor, reference: ObjRef, dict: &Dict) {
    let is_mask = dict
        .get(editor.intern(b"ImageMask"))
        .and_then(Object::as_bool)
        .unwrap_or(false);

    let mut replacement = Dict::new();
    replacement.insert(Name::TYPE, Object::Name(editor.intern(b"XObject")));
    replacement.insert(
        editor.intern(b"Subtype"),
        Object::Name(editor.intern(b"Image")),
    );
    replacement.insert(editor.intern(b"Width"), Object::Int(1));
    replacement.insert(editor.intern(b"Height"), Object::Int(1));

    // Nothing else carries over — no /Filter, no /DecodeParms, no /SMask, no
    // /Decode. Every one of them describes bytes that no longer exist.
    if is_mask {
        // A stencil keeps its flag: one carrying a colour space is malformed,
        // and a viewer may reject the page over it.
        replacement.insert(editor.intern(b"ImageMask"), Object::Bool(true));
        replacement.insert(editor.intern(b"BitsPerComponent"), Object::Int(1));
        editor.put_stream(
            reference,
            StreamData {
                dict: replacement,
                // A set bit paints nothing (8.9.6.2).
                data: vec![0x80],
            },
        );
        return;
    }

    replacement.insert(editor.intern(b"BitsPerComponent"), Object::Int(8));
    replacement.insert(
        editor.intern(b"ColorSpace"),
        Object::Name(editor.intern(b"DeviceGray")),
    );
    editor.put_stream(
        reference,
        StreamData {
            dict: replacement,
            data: vec![0xFF],
        },
    );
}

/// One `Do` invocation, and the transform in force when it happened.
struct XObjectUse {
    name: Vec<u8>,
    /// The transform mapping the XObject's space to the page's.
    ctm: Matrix,
    /// Where the rewritten stream wrote the `Do`'s operand, `/` included, so
    /// that it can be pointed at a copy of the form afterwards.
    at: std::ops::Range<usize>,
}

/// The text state needed to place a glyph: everything in 9.4.4's displacement
/// formula, and nothing else.
///
/// Positions live in **unscaled text space** — the space the text matrix maps
/// *out of*. That is the run's own frame: `along` is how far the pen has
/// walked along the run's own axis, whatever direction that axis points on
/// the page, and the glyph box's other two sides are measured across it in
/// the same units. A `TJ` displacement is defined in exactly this space
/// (9.4.3), which is why a rewritten run needs no new matrix of its own.
///
/// **The axis is the font's writing mode** (9.7.4.3). Horizontal text walks
/// text-space x, by `w0` and the horizontal scale; vertical text walks
/// text-space y, by `/W2`'s `w1`, which is signed — negative, down the page —
/// and has no horizontal scale in it. A `TJ` number displaces along the same
/// axis in both. One number therefore carries both, and [`Pen::vertical`]
/// says which axis it is.
#[derive(Clone)]
struct Pen {
    /// How far along the run's axis the pen has walked since the text matrix
    /// was last set, in unscaled text space: text-space x for horizontal
    /// writing, text-space y for vertical.
    along: f64,
    /// The text matrix, `T_m` (9.4.2).
    text: Matrix,
    /// The text line matrix, `T_lm`. `Td`, `TD` and `T*` are relative to
    /// this one rather than to `text`, which is why both are carried.
    line: Matrix,
    /// The transformation matrix in force, whole.
    ///
    /// A redaction rectangle is given in *page* space, so content drawn under
    /// a `cm` has to be mapped into it. Ignoring the transform measures every
    /// glyph in the wrong space: `q 2 0 0 2 0 0 cm` halves every computed
    /// position, so the rectangle covers the wrong half of the line.
    ctm: Matrix,
    /// The font in force, when the `Tf` named one in scope.
    font: Option<Arc<RunFont>>,
    /// The resource name the last `Tf` gave, whether or not it resolved.
    ///
    /// Carried so that a refusal can name the font a caller would have to go
    /// and look at, including the case where the name resolved to nothing.
    font_name: Vec<u8>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
}

impl Default for Pen {
    fn default() -> Pen {
        Pen {
            along: 0.0,
            text: Matrix::IDENTITY,
            line: Matrix::IDENTITY,
            ctm: Matrix::IDENTITY,
            font: None,
            font_name: Vec::new(),
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

impl Pen {
    /// Starts a new line at an offset from the current one, per `Td`.
    ///
    /// 9.4.2: relative to the text **line** matrix, not the text matrix, so
    /// the displacement a showing operator accumulated does not carry over.
    fn offset(&mut self, tx: f64, ty: f64) {
        self.line = Matrix::translate(tx, ty).then(self.line);
        self.text = self.line;
        self.along = 0.0;
    }

    /// Moves to the next line, per `T*`.
    fn next_line(&mut self) {
        self.offset(0.0, -self.leading);
    }

    /// The transform from the run's own frame to page space.
    fn frame(&self) -> Matrix {
        self.text.then(self.ctm)
    }

    /// Whether the font in force writes vertically (9.7.4.3): `/WMode 1` in
    /// its encoding CMap, which only a composite font has.
    fn vertical(&self) -> bool {
        self.font.as_ref().is_some_and(|f| f.font.is_vertical())
    }

    /// The displacement of one decoded code along the run's axis, per 9.4.4.
    ///
    /// In unscaled text space: the text matrix is *not* in it, because that
    /// is what [`Pen::frame`] applies and what a `TJ` number is measured
    /// before.
    ///
    /// The two branches are 9.4.4's two formulas, and they differ in more
    /// than the axis:
    ///
    /// - horizontal, `tx = (w0 · Tfs / 1000 + Tc + Tw) · Th`;
    /// - vertical, `ty = w1 · Tfs / 1000 + Tc`, where `w1` is the CID's
    ///   `/W2` entry (or `/DW2`'s) and is **signed** — a run that goes down
    ///   the page has a negative one, added rather than subtracted — and
    ///   there is no `Th`, because horizontal scaling scales horizontal
    ///   motion and a vertical run has none.
    ///
    /// Word spacing is left out of the vertical branch because this engine's
    /// interpreter leaves it out (`interpret.rs`, the glyph loop of `show`),
    /// and a cut is measured where the renderer draws. It can only matter for
    /// a single-byte code 32, which a vertical CMap — two bytes a code for
    /// `Identity-V` and every predefined one — does not produce.
    ///
    /// `w0` is in thousandths of text space for every font but a Type 3 one,
    /// whose `/Widths` are in its own glyph space: there the width in text
    /// space is `w0 · a`, the horizontal component of the width carried
    /// through `/FontMatrix` ([`GlyphSpace`]).
    fn advance(&self, code: &tinker_pdf_cos::DecodedCode) -> f64 {
        if let Some(selected) = self.font.as_ref().filter(|f| f.font.is_vertical()) {
            let (_, _, w1) = selected.font.vertical_metrics(code.cid);
            return w1 / 1000.0 * self.size + self.char_spacing;
        }
        let width = match self.font.as_ref().and_then(|f| f.glyph_space) {
            Some(space) => code.width * space.matrix.a,
            None => code.width / 1000.0,
        };
        // Word spacing applies to single-byte code 32 only — the classic bug
        // is applying it to a two-byte CID that happens to equal 32.
        let word = if code.code == 32 && code.bytes == 1 {
            self.word_spacing
        } else {
            0.0
        };
        (width * self.size + self.char_spacing + word) * self.horizontal_scale
    }

    /// The box one glyph occupies, as four corners in unscaled text space,
    /// with the pen at `along` on the run's axis.
    ///
    /// Horizontal: from the pen to the pen plus the advance along x, and from
    /// the rise to one em above it across — the em box, approximated from
    /// the advance and the font size rather than from an outline, which errs
    /// toward removal ([`redact_string`] says why that is right).
    ///
    /// Vertical, the same box stood on end (9.7.4.3): along y it runs from
    /// the pen to the pen plus the (negative) advance, shifted by the rise,
    /// which 9.4.4 puts in text-space y in both modes; across it, the glyph is
    /// drawn with its horizontal origin at *minus* the position vector `v`,
    /// so it spans `-v_x` to `w0 - v_x` — centred on the pen for the default
    /// `v_x = w0 / 2`. That is where this engine's interpreter puts a
    /// vertical glyph, and the ideographic em cell a CJK face fills.
    ///
    /// Type 3, a rectangle in the font's own glyph space carried through its
    /// `/FontMatrix` and then scaled as any text-space point is (9.4.4):
    /// `0` to `w0` along, [`GlyphSpace::low`] to [`GlyphSpace::high`] across.
    /// Without the character and word spacing the other two boxes take from
    /// the advance, because the glyph procedure draws the glyph and spacing is
    /// only where the pen goes next.
    fn glyph_box(&self, code: &tinker_pdf_cos::DecodedCode, along: f64) -> [(f64, f64); 4] {
        if let Some(space) = self.font.as_ref().and_then(|f| f.glyph_space) {
            let w0 = code.width;
            return [
                (0.0, space.low),
                (w0, space.low),
                (w0, space.high),
                (0.0, space.high),
            ]
            .map(|(x, y)| {
                let (x, y) = space.matrix.apply(x, y);
                (
                    along + x * self.size * self.horizontal_scale,
                    self.rise + y * self.size,
                )
            });
        }
        let advance = self.advance(code);
        if let Some(selected) = self.font.as_ref().filter(|f| f.font.is_vertical()) {
            let (v_x, _, _) = selected.font.vertical_metrics(code.cid);
            let unit = self.size / 1000.0 * self.horizontal_scale;
            let (x0, x1) = (-v_x * unit, (code.width - v_x) * unit);
            let (y0, y1) = (along + self.rise, along + self.rise + advance);
            return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        }
        let (y0, y1) = (self.rise, self.rise + self.size);
        [
            (along, y0),
            (along + advance, y0),
            (along + advance, y1),
            (along, y1),
        ]
    }

    /// The displacement of a whole string.
    fn advance_of(&self, bytes: &[u8]) -> f64 {
        let Some(selected) = self.font.as_ref() else {
            return 0.0;
        };
        selected
            .font
            .decode(bytes)
            .iter()
            .map(|c| self.advance(c))
            .sum()
    }

    /// The unit a `TJ` number is measured in: one thousandth of this moves the
    /// pen by one (9.4.3).
    ///
    /// `Tfs · Th` along a horizontal run and `Tfs` alone along a vertical one:
    /// 9.4.4's `ty` subtracts `Tj / 1000` inside the product with `Tfs` and
    /// has no `Th` to multiply by.
    fn thousandth(&self) -> f64 {
        if self.vertical() {
            self.size
        } else {
            self.size * self.horizontal_scale
        }
    }
}

/// Rewrites a content stream with redacted glyphs removed.
fn rewrite(
    content: &[u8],
    areas: &[Redaction],
    fonts: &HashMap<Vec<u8>, Arc<RunFont>>,
    initial: Matrix,
) -> (Vec<u8>, RedactionReport, Vec<XObjectUse>) {
    let mut out = Vec::with_capacity(content.len());
    let mut tokens = Tokenizer::new(content);
    let mut operands: Vec<Token> = Vec::new();
    let mut uses: Vec<XObjectUse> = Vec::new();
    let mut pen = Pen {
        ctm: initial,
        ..Pen::default()
    };
    let mut saved: Vec<Pen> = Vec::new();
    let mut report = RedactionReport::default();

    while let Some(token) = tokens.next_token() {
        let Token::Operator(op) = &token else {
            operands.push(token);
            continue;
        };

        // Operands are counted from the end, because that is where the
        // operator's own arguments are regardless of what preceded them.
        let number = |back: usize| -> f64 {
            let index = operands.len().checked_sub(back + 1);
            match index.and_then(|i| operands.get(i)) {
                Some(Token::Number(v)) if v.is_finite() => *v,
                _ => 0.0,
            }
        };

        let mut rewritten = false;
        // Whether this operator is a `Do` that was recorded as a use.
        let mut recorded = false;

        match op.as_slice() {
            b"Do" => {
                // 8.8: the operand names an XObject. Which kind it is, and
                // what to do about it, is the caller's business — this crate
                // has the transform, and the caller has the dictionaries.
                if let Some(Token::Name(name)) = operands.last() {
                    if uses.len() < 4096 {
                        uses.push(XObjectUse {
                            name: name.clone(),
                            ctm: pen.ctm,
                            at: 0..0,
                        });
                        recorded = true;
                    }
                }
            }
            b"cm" => {
                // Composed with whatever is already in force, because `cm`
                // concatenates rather than replaces (8.4.4).
                let operand = Matrix {
                    a: number(5),
                    b: number(4),
                    c: number(3),
                    d: number(2),
                    e: number(1),
                    f: number(0),
                };
                pen.ctm = operand.then(pen.ctm);
            }
            b"q" => saved.push(pen.clone()),
            b"Q" => {
                if let Some(restored) = saved.pop() {
                    pen = restored;
                }
            }
            b"BT" => {
                // The text matrices reset; the text *state* does not.
                pen.text = Matrix::IDENTITY;
                pen.line = Matrix::IDENTITY;
                pen.along = 0.0;
            }
            b"Tf" => {
                pen.size = number(0);
                pen.font_name = operands
                    .len()
                    .checked_sub(2)
                    .and_then(|i| operands.get(i))
                    .and_then(|t| match t {
                        Token::Name(name) => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                pen.font = fonts.get(&pen.font_name).map(Arc::clone);
            }
            b"Tc" => pen.char_spacing = number(0),
            b"Tw" => pen.word_spacing = number(0),
            b"Tz" => pen.horizontal_scale = number(0) / 100.0,
            b"TL" => pen.leading = number(0),
            b"Ts" => pen.rise = number(0),
            b"Td" => pen.offset(number(1), number(0)),
            b"TD" => {
                pen.leading = -number(0);
                pen.offset(number(1), number(0));
            }
            b"Tm" => {
                // 9.4.2: the matrix *replaces* both text matrices, and it
                // scales, rotates and skews the glyph space as well as
                // placing it.
                pen.line = Matrix {
                    a: number(5),
                    b: number(4),
                    c: number(3),
                    d: number(2),
                    e: number(1),
                    f: number(0),
                };
                pen.text = pen.line;
                pen.along = 0.0;
            }
            b"T*" => pen.next_line(),
            b"Tj" | b"'" | b"\"" => {
                if op.as_slice() != b"Tj" {
                    pen.next_line();
                }
                if op.as_slice() == b"\"" {
                    // `aw ac string "` sets both spacings before showing.
                    pen.word_spacing = number(2);
                    pen.char_spacing = number(1);
                }
                if let Some(Token::String(bytes)) = operands.last().cloned() {
                    let cut = redact_string(&bytes, &pen, areas);
                    if let Some(warning) = cut.warning {
                        note(&mut report.warnings, warning);
                    }
                    if cut.removed > 0 {
                        report.operations += 1;
                        report.glyphs += cut.removed;
                        // 9.4.3: `'` is `T*` then `Tj`, and `" ` is
                        // `aw Tw ac Tc` then `'`. A cut run is re-emitted as
                        // `TJ`, which is only the showing half — so the
                        // halves that are not the showing are written out
                        // first. Dropping them moves every surviving glyph
                        // of the run onto the previous line, which is a cut
                        // that is approximately right.
                        if op.as_slice() == b"\"" {
                            let spacings =
                                format!("{} Tw {} Tc\n", pen.word_spacing, pen.char_spacing);
                            out.extend_from_slice(spacings.as_bytes());
                        }
                        if op.as_slice() != b"Tj" {
                            out.extend_from_slice(b"T*\n");
                        }
                        emit_array(&mut out, &cut.runs, pen.thousandth());
                        rewritten = true;
                    }
                    pen.along += pen.advance_of(&bytes);
                }
            }
            b"TJ" => {
                // Built once rather than probed and rebuilt: the pen ends in
                // the same place either way, and a second pass would raise
                // every refusal twice.
                let mut runs: Vec<Run> = Vec::new();
                let mut removed = 0usize;
                let mut local = pen.clone();
                for token in &operands {
                    match token {
                        Token::String(s) => {
                            let cut = redact_string(s, &local, areas);
                            if let Some(warning) = cut.warning {
                                note(&mut report.warnings, warning);
                            }
                            removed += cut.removed;
                            runs.extend(cut.runs);
                            local.along += local.advance_of(s);
                        }
                        Token::Number(v) if v.is_finite() => {
                            // 9.4.3: the number moves the pen *backwards* by
                            // its value in thousandths, along the baseline.
                            let shift = -v / 1000.0 * local.thousandth();
                            runs.push(Run::Gap(shift));
                            local.along += shift;
                        }
                        _ => {}
                    }
                }

                if removed > 0 {
                    report.operations += 1;
                    report.glyphs += removed;
                    emit_array(&mut out, &runs, pen.thousandth());
                    rewritten = true;
                }
                pen = local;
            }
            _ => {}
        }

        if !rewritten {
            let mut last = 0..0;
            for operand in &operands {
                let start = out.len();
                write_token(&mut out, operand);
                last = start..out.len();
                out.push(b' ');
            }
            // A `Do` is never rewritten, so its operand is always written
            // here, and it is the last one.
            if recorded {
                if let Some(used) = uses.last_mut() {
                    used.at = last;
                }
            }
            out.extend_from_slice(op);
            out.push(b'\n');
        }
        operands.clear();
    }

    (out, report, uses)
}

/// A piece of a rewritten showing operation.
enum Run {
    /// Bytes that survived.
    Text(Vec<u8>),
    /// Displacement along the baseline, in unscaled text space, where glyphs
    /// used to be — or an adjustment that was already in the original `TJ`
    /// array.
    Gap(f64),
}

/// Writes runs as a single `TJ` array.
///
/// Everything becomes `TJ` because that is the only showing operator that can
/// express "move by this much without drawing", which is exactly what a
/// removed glyph leaves behind.
///
/// # Why no new text matrix
///
/// The surviving sub-runs are emitted into the text matrix the original run
/// was already using. Giving each sub-run a fresh `Tm` at its own origin is
/// the answer that looks plausible on screen and is wrong four ways:
///
/// - `Tm` replaces the text **line** matrix as well as the text matrix, so
///   any later `Td`, `TD` or `T*` in the same `BT` — all of which are
///   relative to the line matrix — would be measured from the last sub-run's
///   origin instead of from the line's.
/// - The six numbers would have to be *reconstructed*, and the rotation among
///   them rounded; neighbouring sub-runs rounded differently sit on baselines
///   that no longer line up.
/// - The position a run is at need not have come from a `Tm` at all. It can
///   be a `Td` chain from a matrix set in an earlier `BT`, so reconstructing
///   one means writing out a matrix that was only ever inferred.
/// - After the operation the text matrix has to have advanced by the whole
///   string's displacement, because that is what the next operator expects.
///   `TJ` does that natively.
///
/// So the only numbers this writes are displacements along the baseline, and
/// 9.4.3 measures those in unscaled text space — the run's own frame —
/// whatever the run's matrix does to the page. That is the whole of what
/// "cut along its own baseline" costs.
///
/// `thousandth` is `Tfs · Th`: one thousandth of it is what a `TJ` number of
/// one moves the pen by. When it is zero no `TJ` number can express any
/// displacement at all — but a run with a zero font size or a zero horizontal
/// scale draws a glyph box of zero area, so there is nothing whose position
/// the lost gap could disturb.
fn emit_array(out: &mut Vec<u8>, runs: &[Run], thousandth: f64) {
    out.push(b'[');
    for run in runs {
        match run {
            Run::Text(bytes) if bytes.is_empty() => {}
            Run::Text(bytes) => {
                out.push(b'(');
                escape_into(out, bytes);
                out.extend_from_slice(b") ");
            }
            Run::Gap(shift) => {
                if thousandth.abs() <= f64::EPSILON || !shift.is_finite() {
                    continue;
                }
                let thousandths = -shift / thousandth * 1000.0;
                if thousandths.abs() >= 0.0005 {
                    out.extend_from_slice(format!("{thousandths:.3} ").as_bytes());
                }
            }
        }
    }
    out.extend_from_slice(b"] TJ\n");
}

/// The result of cutting one string.
struct Cut {
    runs: Vec<Run>,
    removed: usize,
    /// Why the string was left whole, when it was.
    warning: Option<RedactionWarning>,
}

/// Removes the glyphs of `bytes` that fall inside a redaction.
///
/// The glyph box is measured in the run's own frame — along the run's axis
/// by the advance, across it by the em ([`Pen::glyph_box`], which stands the
/// box on end for vertical writing) — and carried into page space by
/// [`Pen::frame`], which is the text matrix and the transformation matrix
/// composed. A rotated or skewed matrix turns that box into a parallelogram
/// rather than making it unmeasurable, and [`quad_meets_rect`] answers
/// exactly.
///
/// A glyph the rectangle covers **partly** is removed. There is no third
/// option: a content stream can show a glyph or not show it, so the choice is
/// between removing a glyph that was partly visible and leaving one that was
/// partly covered — and only one of those two can leak. The visible cost is a
/// `mark`ed rectangle with a bite of blank page beside it where the
/// over-removed glyph was, which is a thing a reader can see rather than a
/// thing an extractor can find.
fn redact_string(bytes: &[u8], pen: &Pen, areas: &[Redaction]) -> Cut {
    let whole = |warning: Option<RedactionWarning>| Cut {
        runs: vec![Run::Text(bytes.to_vec())],
        removed: 0,
        warning,
    };

    // No rectangle, nothing to be uncertain about: a refusal warning says
    // "this run was not tested against your rectangles", and with no
    // rectangles that sentence has no content.
    if areas.is_empty() {
        return whole(None);
    }

    let font = || pen.font_name.clone();
    let left = bytes.len();

    let Some(selected) = pen.font.as_ref() else {
        return whole(Some(RedactionWarning::UnknownFont {
            font: font(),
            bytes: left,
        }));
    };
    let frame = pen.frame();
    if !frame.is_finite()
        || !pen.along.is_finite()
        || !pen.size.is_finite()
        || !pen.rise.is_finite()
        || !pen.horizontal_scale.is_finite()
    {
        return whole(Some(RedactionWarning::UnmeasurableFrame {
            font: font(),
            bytes: left,
        }));
    }

    // Measured in full before anything is cut, so that a run which turns out
    // to be unmeasurable part-way along is left whole rather than half-cut.
    let codes = selected.font.decode(bytes);
    let mut boxes: Vec<([(f64, f64); 4], f64)> = Vec::with_capacity(codes.len());
    let mut along = pen.along;
    for code in &codes {
        let advance = pen.advance(code);
        // The glyph's box, approximated from its advance and the font size.
        // Approximating is right here: an exact outline would let a descender
        // poking one hundredth of a point into the box decide the redaction,
        // and erring towards removal is the safe direction anyway.
        let quad = pen.glyph_box(code, along).map(|(x, y)| frame.apply(x, y));
        if !advance.is_finite() || quad.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
            return whole(Some(RedactionWarning::UnmeasurableFrame {
                font: font(),
                bytes: left,
            }));
        }
        boxes.push((quad, advance));
        along += advance;
    }

    let mut runs: Vec<Run> = Vec::new();
    let mut kept: Vec<u8> = Vec::new();
    let mut gap = 0.0f64;
    let mut removed = 0usize;

    for (code, (quad, advance)) in codes.iter().zip(&boxes) {
        let inside = areas
            .iter()
            .any(|redaction| quad_meets_rect(quad, redaction.area));

        if inside {
            removed += 1;
            if !kept.is_empty() {
                runs.push(Run::Text(std::mem::take(&mut kept)));
            }
            gap += advance;
        } else {
            if gap != 0.0 {
                runs.push(Run::Gap(std::mem::take(&mut gap)));
            }
            encode_code(&mut kept, code.code, code.bytes);
        }
    }

    if !kept.is_empty() {
        runs.push(Run::Text(kept));
    }
    if gap != 0.0 {
        // A trailing gap still matters: it holds the pen's position for
        // whatever the next operator shows.
        runs.push(Run::Gap(gap));
    }

    Cut {
        runs,
        removed,
        warning: None,
    }
}

/// Whether a convex quadrilateral and an axis-aligned rectangle overlap.
///
/// The separating-axis theorem: two convex shapes miss each other exactly when
/// some direction separates their projections. For a rectangle and a
/// parallelogram the candidate directions are four — the rectangle's two axes
/// and the parallelogram's two edge normals — and a direction along which one
/// shape's projection ends where the other's begins is a separation, so a
/// glyph box that merely *touches* the rectangle is not covered. That matches
/// the strict comparisons this test replaced, which is why the fixtures that
/// sit a boundary exactly on a glyph edge still mean what they meant.
///
/// Projections are taken over the corners as given, so a rectangle written
/// with its corners the wrong way round — `x0` greater than `x1` — describes
/// the same area rather than an empty one.
///
/// A degenerate quadrilateral is handled rather than special-cased: a zero
/// advance collapses one edge, whose normal is then skipped, and the test
/// reduces to a segment against the rectangle. Both edges collapsing reduces
/// it to a point.
fn quad_meets_rect(quad: &[(f64, f64); 4], area: Rect) -> bool {
    let corners = [
        (area.x0, area.y0),
        (area.x1, area.y0),
        (area.x1, area.y1),
        (area.x0, area.y1),
    ];

    let edges = [
        (quad[1].0 - quad[0].0, quad[1].1 - quad[0].1),
        (quad[2].0 - quad[1].0, quad[2].1 - quad[1].1),
    ];
    let axes = [
        (1.0, 0.0),
        (0.0, 1.0),
        (-edges[0].1, edges[0].0),
        (-edges[1].1, edges[1].0),
    ];

    for axis in axes {
        let length = (axis.0 * axis.0 + axis.1 * axis.1).sqrt();
        if !length.is_finite() || length <= 1e-9 {
            continue;
        }
        let unit = (axis.0 / length, axis.1 / length);
        let span = |points: &[(f64, f64)]| {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for point in points {
                let at = point.0 * unit.0 + point.1 * unit.1;
                lo = lo.min(at);
                hi = hi.max(at);
            }
            (lo, hi)
        };
        let (glyph_lo, glyph_hi) = span(quad);
        let (area_lo, area_hi) = span(&corners);
        if glyph_hi <= area_lo || area_hi <= glyph_lo {
            return false;
        }
    }

    true
}

/// Re-encodes a code as the bytes it was decoded from.
fn encode_code(out: &mut Vec<u8>, code: u32, width: u8) {
    let width = width.clamp(1, 4);
    for shift in (0..width).rev() {
        out.push(((code >> (u32::from(shift) * 8)) & 0xFF) as u8);
    }
}

fn escape_into(out: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
}

fn write_token(out: &mut Vec<u8>, token: &Token) {
    match token {
        Token::Number(v) => out.extend_from_slice(format!("{v}").as_bytes()),
        Token::String(s) => {
            out.push(b'(');
            escape_into(out, s);
            out.push(b')');
        }
        Token::Name(n) => {
            out.push(b'/');
            out.extend_from_slice(n);
        }
        Token::ArrayOpen => out.push(b'['),
        Token::ArrayClose => out.push(b']'),
        Token::DictOpen => out.extend_from_slice(b"<<"),
        Token::DictClose => out.extend_from_slice(b">>"),
        Token::Bool(true) => out.extend_from_slice(b"true"),
        Token::Bool(false) => out.extend_from_slice(b"false"),
        Token::Null => out.extend_from_slice(b"null"),
        Token::Operator(o) => out.extend_from_slice(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_cos::{DocumentBuilder, WriteMode, WriteOptions};

    fn document(text: &str) -> Arc<CosDocument> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, text);
        });
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    }

    /// Every stream in a document, decompressed. The acceptance test for a
    /// redaction is that the needle is absent from all of them — not from the
    /// one stream we happen to look at.
    fn all_streams(doc: &CosDocument) -> String {
        let mut out = Vec::new();
        for (number, _) in doc.xref().iter() {
            if let Ok(bytes) = doc.stream_decoded(ObjRef::new(number, 0)) {
                out.extend_from_slice(&bytes);
                out.push(b'\n');
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn redact(doc: Arc<CosDocument>, areas: &[Redaction]) -> (Vec<u8>, RedactionReport) {
        let mut editor = DocumentEditor::new(doc);
        let report = apply(&mut editor, 0, areas).expect("the page exists");
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        (bytes, report)
    }

    /// Redacts, reopens, and returns every stream the result contains.
    fn redact_to_streams(doc: Arc<CosDocument>, areas: &[Redaction]) -> (String, RedactionReport) {
        let (bytes, report) = redact(doc, areas);
        let reopened = CosDocument::open(bytes).expect("it reopens");
        (all_streams(&reopened), report)
    }

    /// Covers the second word of `PUBLIC SECRET` and nothing else.
    ///
    /// `PUBLIC` ends at about x=55.3 in 12pt Helvetica, so the band starts
    /// past that. A glyph that merely touches the band is removed, which is
    /// the safe direction but does mean the boundary is not arbitrary.
    fn second_word() -> Redaction {
        Redaction {
            area: Rect {
                x0: 57.0,
                y0: 45.0,
                x1: 400.0,
                y1: 70.0,
            },
            mark: false,
        }
    }

    /// The test that matters.
    #[test]
    fn redacted_bytes_are_absent_from_every_stream() {
        let doc = document("PUBLIC SECRET");
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts out present, or the test proves nothing"
        );

        let (streams, report) = redact_to_streams(doc, &[second_word()]);

        assert!(report.glyphs > 0, "glyphs were removed");
        assert!(
            !streams.contains("SECRET"),
            "the redacted bytes must be gone from the file, got: {streams}"
        );
    }

    #[test]
    fn text_outside_the_area_survives() {
        let doc = document("PUBLIC SECRET");
        let (streams, _) = redact_to_streams(doc, &[second_word()]);
        assert!(
            streams.contains("PUBLIC"),
            "text outside the rectangle is untouched"
        );
    }

    /// Reading the page back through the engine's own extractor, which is how
    /// anyone looking for the secret would read it.
    #[test]
    fn extraction_no_longer_finds_it() {
        let doc = document("PUBLIC SECRET");
        let (bytes, _) = redact(doc, &[second_word()]);

        let reopened = crate::Document::open(bytes).expect("it reopens");
        let page = reopened.page(0).expect("the page is there");
        let text = page.text().plain_text();
        assert!(
            !text.contains("SECRET"),
            "the extractor must not find it either, got: {text:?}"
        );
        assert!(
            text.contains("PUBLIC"),
            "but it still finds the rest, got: {text:?}"
        );
    }

    #[test]
    fn a_redaction_covering_nothing_removes_nothing() {
        let doc = document("PUBLIC SECRET");
        let (streams, report) = redact_to_streams(
            doc,
            &[Redaction {
                area: Rect {
                    x0: 1000.0,
                    y0: 1000.0,
                    x1: 1100.0,
                    y1: 1100.0,
                },
                mark: false,
            }],
        );

        assert_eq!(report, RedactionReport::default());
        assert!(streams.contains("PUBLIC SECRET"));
    }

    #[test]
    fn an_empty_redaction_list_is_a_no_op() {
        let doc = document("PUBLIC SECRET");
        let (streams, report) = redact_to_streams(doc, &[]);
        assert_eq!(report, RedactionReport::default());
        assert!(streams.contains("PUBLIC SECRET"));
    }

    #[test]
    fn a_marked_redaction_paints_the_area() {
        let doc = document("PUBLIC SECRET");
        let area = Redaction {
            mark: true,
            ..second_word()
        };
        let (streams, _) = redact_to_streams(doc, &[area]);

        assert!(streams.contains(" re f"), "a rectangle is painted");
        assert!(streams.contains("0 g"), "in black");
        assert!(!streams.contains("SECRET"), "and the text is still gone");
    }

    /// Builds a page whose text is placed by a scaling `Tm` rather than by
    /// `Td` with a sized font — `/F0 1 Tf` plus `12 0 0 12 x y Tm` is an
    /// ordinary way to write twelve-point text.
    fn scaled_document(text: &str) -> Arc<CosDocument> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 100.0, |page| {
            page.raw(
                format!(
                    "BT /F0 1 Tf 12 0 0 12 10 50 Tm ({text}) Tj ET
"
                )
                .as_bytes(),
            );
        });
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    }

    /// Reading only `Tm`'s translation left every advance twelve times too
    /// small, so the positions drifted further wrong across the line and the
    /// rectangle cut the wrong glyphs — or none at all.
    #[test]
    fn a_scaling_text_matrix_places_the_glyphs() {
        let doc = scaled_document("PUBLIC SECRET");
        assert!(all_streams(&doc).contains("SECRET"));

        let (streams, report) = redact_to_streams(doc, &[second_word()]);
        assert!(report.glyphs > 0, "something was found to cut");
        assert!(
            !streams.contains("SECRET"),
            "the second word is gone: {streams}"
        );
        assert!(streams.contains("PUBLIC"), "and the first survives");
    }

    /// A redaction rectangle is in page space. Content drawn under a `cm` is
    /// in some other space, and ignoring the transform measured every glyph in
    /// the wrong one — so the rectangle covered the wrong part of the line, or
    /// nothing at all.
    #[test]
    fn a_transform_is_mapped_into_page_space() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 200.0, |page| {
            // Everything at double scale: the text is written at (5, 25) in
            // its own space, which is (10, 50) on the page.
            page.raw(
                b"q 2 0 0 2 0 0 cm BT /F0 6 Tf 5 25 Td (PUBLIC SECRET) Tj ET Q
",
            );
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));
        assert!(all_streams(&doc).contains("SECRET"));

        // In page space the text starts at x = 10 and is 12pt tall. `PUBLIC`
        // ends near x = 65, so a band from 57 rightwards takes the second word.
        let band = Redaction {
            area: Rect {
                x0: 57.0,
                y0: 45.0,
                x1: 400.0,
                y1: 70.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[band]);
        assert!(report.glyphs > 0, "the transform was followed");
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(streams.contains("PUBLIC"), "and the first word survives");
    }

    /// The transform is restored by `Q`, so a rectangle over content outside
    /// the `q`/`Q` pair is not measured through it.
    #[test]
    fn a_transform_does_not_outlive_its_q() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 200.0, |page| {
            page.raw(
                b"q 2 0 0 2 0 0 cm BT /F0 6 Tf 5 60 Td (SCALED) Tj ET Q
                  BT /F0 12 Tf 10 30 Td (PLAIN) Tj ET
",
            );
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        // A band over the lower line only, in page space.
        let band = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 25.0,
                x1: 400.0,
                y1: 45.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[band]);
        assert!(report.glyphs > 0);
        assert!(!streams.contains("PLAIN"), "the lower line went");
        assert!(
            streams.contains("SCALED"),
            "and the scaled line, which sits higher, stayed: {streams}"
        );
    }

    /// A form XObject holds content like any other. A redaction that stopped
    /// at the page stream left everything a form drew exactly where it was —
    /// and forms are how most producers place repeated content, so this was a
    /// hole a redaction could drive through.
    #[test]
    fn text_inside_a_form_xobject_is_redacted() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 24 >>\nstream\n\
q 1 0 0 1 0 0 cm /Fm0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 200]\n\
   /Resources << /Font << /F0 6 0 R >> >> /Length 46 >>\nstream\n\
BT /F0 12 Tf 10 50 Td (PUBLIC SECRET) Tj ET\n\
endstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n";

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts present"
        );

        let (streams, report) = redact_to_streams(doc, &[second_word()]);
        assert!(report.glyphs > 0, "the form was followed into");
        assert!(
            !streams.contains("SECRET"),
            "and the text inside it is gone from every stream: {streams}"
        );
        assert!(streams.contains("PUBLIC"), "the rest survives");
    }

    /// One form drawing `SECRET`, invoked at page y 50 and again at y 200.
    ///
    /// The fixture the pinned defect was written against, kept because it is
    /// the evidence: the same bytes that used to demonstrate the second
    /// placement going unmeasured now demonstrate it being measured.
    fn twice_placed_form() -> &'static [u8] {
        b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 56 >>\nstream\n\
q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 300]\n\
   /Resources << /Font << /F0 6 0 R >> >> /Length 39 >>\nstream\n\
BT /F0 12 Tf 10 50 Td (SECRET) Tj ET\n\
endstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n"
    }

    /// **The pin, flipped twice.** A form drawn twice is measured at *both*
    /// placements and cut **only** at the one the rectangle covers, and the
    /// fixture is the one that used to prove the second placement was not
    /// measured at all.
    ///
    /// The form draws `SECRET` at page y 50 and again at page y 200; the
    /// rectangle covers the second placement only. Until the September 2026
    /// guard keyed by the transform, the second `Do` was a no-op: the text
    /// stayed and the report said `glyphs: 0`, the silent under-redaction.
    /// That guard then measured it, and cut it out of the one stream both
    /// placements shared — so the first placement lost `SECRET` too, and
    /// `RepeatedForm` named the widened cut.
    ///
    /// Now the covered placement draws a copy of the form cut at its own
    /// frame, and the uncovered one draws the form as it was. So, together:
    ///
    /// - the page draws no ink inside the rectangle, and the form object that
    ///   still carries `SECRET` is the one the *first* placement draws;
    /// - the first placement still draws its text, where it was;
    /// - nothing is reported, because nothing was cut wider than asked.
    #[test]
    fn a_form_drawn_twice_is_cut_at_the_placement_the_rectangle_covers() {
        let doc = Arc::new(CosDocument::open(twice_placed_form()).expect("it opens"));
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts present"
        );

        // The second placement's baseline is page y 200; the first's is y 50.
        let over_the_second = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 190.0,
                x1: 400.0,
                y1: 230.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[over_the_second]);
        assert_eq!(
            report.glyphs, 6,
            "the second placement was measured: every glyph of SECRET went"
        );
        assert!(
            report.warnings.is_empty(),
            "and nothing was cut wider than asked: {:?}",
            report.warnings
        );

        // The stream check on its own would pass a build that left the glyph
        // in a second, unreferenced copy. The page has to draw nothing there.
        let bitmap = super::tests_support::render(bytes.clone());
        assert_eq!(
            super::tests_support::ink_in(&bitmap, 300.0, over_the_second.area),
            0,
            "the page draws no ink inside the rectangle"
        );
        // Helvetica is not embedded and this build carries no standard
        // faces, so the page draws no glyph of it anywhere: the ink check
        // above is the stream check's partner, not evidence of what stayed.
        // Extraction is that evidence.
        assert_eq!(
            super::tests_support::lines_of(bytes.clone()),
            vec![(50.0, "SECRET".to_string())],
            "the first placement, which no rectangle covered, still shows SECRET, \
             and only there"
        );

        // The page now draws the second placement from a copy: its content
        // names the copy, and the form object itself is untouched.
        let reopened = CosDocument::open(bytes).expect("it reopens");
        let page = super::tests_support::page_content(&reopened);
        assert!(
            page.contains("/Fm0 Do") && page.contains("/Rd"),
            "one `Do` names the form and the other its copy: {page}"
        );
    }

    /// A form drawn twice under the **same** transform is still one
    /// measurement, because it is one placement.
    ///
    /// This is the half the old `visited` set got right, and the half a fix
    /// that copied a form per `Do` unconditionally would have broken: two
    /// invocations in the same frame are the same rectangle test over the
    /// same geometry, so a second pass has nothing to find and a second copy
    /// would bloat every ordinary file for no gain. No `RepeatedForm` either
    /// — the output is exactly what the rectangles asked for.
    #[test]
    fn a_form_drawn_twice_under_one_transform_is_measured_once() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 56 >>\nstream\n\
q 1 0 0 1 0 150 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 300]\n\
   /Resources << /Font << /F0 6 0 R >> >> /Length 39 >>\nstream\n\
BT /F0 12 Tf 10 50 Td (SECRET) Tj ET\n\
endstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n";

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        let band = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 190.0,
                x1: 400.0,
                y1: 230.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[band]);
        assert_eq!(report.glyphs, 6, "the six glyphs of SECRET, counted once");
        assert_eq!(
            report.operations, 1,
            "one showing operator was rewritten, not the same one twice"
        );
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(
            report.warnings.is_empty(),
            "one placement is exact, so nothing is reported: {:?}",
            report.warnings
        );
    }

    /// Both placements covered: each is measured in its own frame, and the
    /// report counts the glyphs each one removed rather than one placement's
    /// twice.
    ///
    /// The rectangle is the full height of the page, so it takes `SECRET` at
    /// page y 50 and at page y 200. The first pass removes all six; the
    /// second pass rewrites what the first left and finds nothing, which is
    /// the arithmetic that keeps `glyphs` a count of glyphs rather than of
    /// passes.
    #[test]
    fn a_form_drawn_twice_with_both_placements_covered_is_cut_at_both() {
        let doc = Arc::new(CosDocument::open(twice_placed_form()).expect("it opens"));
        let whole_page = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 400.0,
                y1: 300.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[whole_page]);
        let reopened = CosDocument::open(bytes.clone()).expect("it reopens");
        assert!(
            !all_streams(&reopened).contains("SECRET"),
            "the text is gone from every stream"
        );
        assert_eq!(report.glyphs, 6, "six glyphs, not twelve");

        let bitmap = super::tests_support::render(bytes);
        assert_eq!(
            super::tests_support::ink_in(&bitmap, 300.0, whole_page.area),
            0,
            "and neither placement draws anything"
        );
    }

    /// A form drawn twice that no rectangle touches is exact, so it raises no
    /// warning.
    ///
    /// The guard this pins is that `RepeatedForm` reports a *widened cut*
    /// rather than the mere existence of a second placement. A warning on
    /// every repeated form would fire on most real files, where a header or a
    /// logo is placed on every page, and a warning that is always present is
    /// one nobody reads.
    #[test]
    fn a_form_drawn_twice_that_nothing_is_cut_from_raises_no_warning() {
        let doc = Arc::new(CosDocument::open(twice_placed_form()).expect("it opens"));
        // Between the two baselines: page y 50 and page y 200 are both clear
        // of it.
        let between = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 100.0,
                x1: 400.0,
                y1: 120.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[between]);
        assert_eq!(report.glyphs, 0, "the rectangle covers neither placement");
        assert!(
            streams.contains("SECRET"),
            "so the text stays, at both placements"
        );
        assert!(
            report.warnings.is_empty(),
            "and there is nothing to report: {:?}",
            report.warnings
        );
    }

    /// A form inside a form, the outer one drawn at two placements.
    ///
    /// The inner form is where the text is, and the rectangle covers it only
    /// through the outer form's second placement. Both the transform composed
    /// down through two levels and the guard at the inner level have to be
    /// right for this to be found: the inner form is reached twice, under two
    /// different composed transforms, and the second reach is a placement of
    /// its own.
    ///
    /// And cut there only: the inner form's second placement is a copy, and
    /// the outer form's second placement, whose `Do` has to name that copy,
    /// is a copy too — a copy of a form is a different outcome for every form
    /// that draws it. The first placement of both is the file's own objects,
    /// untouched, and still draws `SECRET`.
    #[test]
    fn a_nested_form_is_measured_at_every_placement_of_its_parent() {
        let doc = Arc::new(CosDocument::open(bytes_of_nested()).expect("it opens"));
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts present"
        );

        let over_the_second = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 190.0,
                x1: 400.0,
                y1: 230.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[over_the_second]);
        assert_eq!(report.glyphs, 6, "the inner form was measured at depth two");
        assert!(
            report.warnings.is_empty(),
            "and cut exactly: {:?}",
            report.warnings
        );

        let bitmap = super::tests_support::render(bytes.clone());
        assert_eq!(
            super::tests_support::ink_in(&bitmap, 300.0, over_the_second.area),
            0,
            "no ink under the rectangle"
        );
        assert_eq!(
            super::tests_support::lines_of(bytes.clone()),
            vec![(50.0, "SECRET".to_string())],
            "the first placement still draws SECRET, where it was"
        );

        // Two copies: the inner form's, and the outer form's that names it.
        let after = CosDocument::open(bytes).expect("it reopens");
        assert_eq!(
            super::tests_support::forms_in(&after),
            4,
            "the two forms and one copy of each, and no more"
        );
    }

    fn area(x0: f64, y0: f64, x1: f64, y1: f64) -> Redaction {
        Redaction {
            area: Rect { x0, y0, x1, y1 },
            mark: false,
        }
    }

    /// **The row's exit fixture.** A form whose two placements are cut
    /// differently is cut exactly at each: one rectangle over `SECRET` at
    /// the lower placement, another over `PUBLIC` at the upper, and each
    /// placement loses exactly the word its own rectangle covered.
    ///
    /// No placement is uncut, so the form's own object takes the first
    /// placement's outcome and the second draws a copy. Nothing anywhere
    /// holds `PUBLIC SECRET` whole: an object no placement draws would still
    /// be in the file, and a copy for every placement would have left the
    /// original exactly that.
    #[test]
    fn a_form_whose_placements_are_cut_differently_is_cut_exactly_at_each() {
        let lower = area(56.0, 45.0, 400.0, 70.0);
        let upper = area(0.0, 195.0, 52.0, 220.0);

        let (bytes, report) = redact(
            Arc::new(
                CosDocument::open(super::tests_support::public_secret_twice()).expect("it opens"),
            ),
            &[lower, upper],
        );
        assert_eq!(report.glyphs, 12, "SECRET below and PUBLIC above");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let lines = super::tests_support::lines_of(bytes.clone());
        assert_eq!(
            lines,
            vec![(50.0, "PUBLIC".to_string()), (200.0, "SECRET".to_string())],
            "each placement kept exactly what its own rectangle did not cover"
        );

        let bitmap = super::tests_support::render(bytes.clone());
        for (covered, what) in [(lower, "SECRET below"), (upper, "PUBLIC above")] {
            assert_eq!(
                super::tests_support::ink_in(&bitmap, 300.0, covered.area),
                0,
                "no ink where {what} was"
            );
        }
        let kept_below = Rect {
            x0: 11.0,
            y0: 51.0,
            x1: 51.0,
            y1: 58.0,
        };
        let kept_above = Rect {
            x0: 57.0,
            y0: 201.0,
            x1: 99.0,
            y1: 208.0,
        };
        for (kept, what) in [(kept_below, "PUBLIC below"), (kept_above, "SECRET above")] {
            assert!(
                super::tests_support::ink_in(&bitmap, 300.0, kept) > 20,
                "{what} is still drawn"
            );
        }

        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(
            !streams.contains("PUBLIC SECRET"),
            "no stream holds the uncut text: {streams}"
        );
    }

    /// Placements whose outcomes are the same share one stream: a copy is
    /// per distinct outcome, not per `Do`.
    ///
    /// Three placements, at page y 50, 125 and 200; one tall rectangle takes
    /// `SECRET` from the upper two, which cut identically. The lowest is
    /// uncut and keeps the form's own object, and the upper two share one
    /// copy — one object more than the file had, not two.
    #[test]
    fn placements_that_cut_the_same_share_one_copy() {
        let bytes = super::tests_support::public_secret_drawn_by(
            "q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 75 cm /Fm0 Do Q \
             q 1 0 0 1 0 150 cm /Fm0 Do Q",
        );
        let (after, report) = redact(open_arc(bytes), &[area(56.0, 120.0, 400.0, 215.0)]);
        assert_eq!(report.glyphs, 6, "one copy's six, not twelve");
        assert_eq!(
            super::tests_support::lines_of(after.clone()),
            vec![
                (50.0, "PUBLIC SECRET".to_string()),
                (125.0, "PUBLIC".to_string()),
                (200.0, "PUBLIC".to_string()),
            ]
        );
        let reopened = CosDocument::open(after).expect("it reopens");
        assert_eq!(
            super::tests_support::forms_in(&reopened),
            2,
            "the form and one copy for the two placements that cut the same"
        );
    }

    fn open_arc(bytes: Vec<u8>) -> Arc<CosDocument> {
        Arc::new(CosDocument::open(bytes).expect("it opens"))
    }

    /// A second redaction of the page keeps the copies the first one made,
    /// and cuts them where they are drawn.
    ///
    /// The first takes `SECRET` from the lower placement, which gets a copy;
    /// the second takes `PUBLIC` from the upper, which draws the form's own
    /// object. Read back, each placement has lost what its rectangle covered
    /// and nothing else — the same answer as the two rectangles at once.
    #[test]
    fn a_second_redaction_keeps_the_copies_the_first_made() {
        let mut editor = DocumentEditor::new(open_arc(super::tests_support::public_secret_twice()));
        let first = apply(&mut editor, 0, &[area(56.0, 45.0, 400.0, 70.0)]).expect("page 0");
        let second = apply(&mut editor, 0, &[area(0.0, 195.0, 52.0, 220.0)]).expect("page 0");
        assert_eq!((first.glyphs, second.glyphs), (6, 6));
        assert!(first.warnings.is_empty() && second.warnings.is_empty());

        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        assert_eq!(
            super::tests_support::lines_of(bytes),
            vec![(50.0, "PUBLIC".to_string()), (200.0, "SECRET".to_string())]
        );
    }

    /// A second redaction reaches a copy the first one made, and cuts it.
    ///
    /// The first takes `SECRET` from the lower placement, which is given a
    /// copy holding `PUBLIC`; the second takes `PUBLIC` from the same
    /// placement, which now means from the copy — an object only the editor
    /// has, so a walk that read forms out of the file would not find it and
    /// would leave `PUBLIC` under the second rectangle.
    #[test]
    fn a_second_redaction_cuts_the_copy_the_first_made() {
        let mut editor = DocumentEditor::new(open_arc(super::tests_support::public_secret_twice()));
        let first = apply(&mut editor, 0, &[area(56.0, 45.0, 400.0, 70.0)]).expect("page 0");
        let second = apply(&mut editor, 0, &[area(0.0, 45.0, 52.0, 70.0)]).expect("page 0");
        assert_eq!((first.glyphs, second.glyphs), (6, 6));

        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        assert_eq!(
            super::tests_support::lines_of(bytes),
            vec![(200.0, "PUBLIC SECRET".to_string())],
            "the lower placement lost both words, the upper neither"
        );
    }

    /// A second redaction of a form cut in place keeps the first cut: it
    /// reads the form as the editor has it, not as the file had it.
    #[test]
    fn a_second_redaction_of_a_form_keeps_the_first_cut() {
        let mut editor = DocumentEditor::new(open_arc(
            super::tests_support::public_secret_drawn_by("/Fm0 Do"),
        ));
        apply(&mut editor, 0, &[area(56.0, 45.0, 400.0, 70.0)]).expect("page 0");
        apply(&mut editor, 0, &[area(0.0, 45.0, 52.0, 70.0)]).expect("page 0");

        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(
            !streams.contains("PUBLIC") && !streams.contains("SECRET"),
            "neither word is anywhere: {streams}"
        );
        assert!(super::tests_support::lines_of(bytes).is_empty());
    }

    /// When a placement on the page is uncut, the form's own object is left
    /// exactly as it was — so another page that draws the same form still
    /// draws all of it.
    ///
    /// Page one draws the form at y 50 and y 200, page two once at y 50. The
    /// rectangle takes `SECRET` from page one's **first** placement, so the
    /// placement that could keep the form's object is the second: choosing by
    /// order rather than by "uncut" would write page one's cut into the object
    /// page two draws, and page two would lose a word no rectangle on it
    /// covered.
    #[test]
    fn a_form_another_page_draws_is_left_whole_when_a_placement_here_is_uncut() {
        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(
            b"F0",
            b"LiberationSerif",
            &crate::subset::tests_support::face()
        ));
        assert!(builder.add_form(
            b"Fm0",
            &tinker_pdf_cos::FormXObject {
                bbox: [0.0, 0.0, 400.0, 300.0],
                matrix: None,
                group: None,
                content: b"BT /F0 12 Tf 10 50 Td (PUBLIC SECRET) Tj ET",
            }
        ));
        builder.add_page(400.0, 300.0, |p| {
            p.raw(b"q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q");
        });
        builder.add_page(400.0, 300.0, |p| p.raw(b"/Fm0 Do"));

        let (bytes, report) = redact(open_arc(builder.finish()), &[area(56.0, 45.0, 400.0, 70.0)]);
        assert_eq!(report.glyphs, 6);
        assert_eq!(
            super::tests_support::lines_of(bytes.clone()),
            vec![
                (50.0, "PUBLIC".to_string()),
                (200.0, "PUBLIC SECRET".to_string())
            ]
        );
        assert_eq!(
            super::tests_support::lines_on(bytes, 1),
            vec![(50.0, "PUBLIC SECRET".to_string())],
            "page two was not redacted and lost nothing"
        );
    }

    /// A form placed more times than [`MAX_PLACEMENTS`] is cut the old way,
    /// in its one stream — and so is **every form it draws**.
    ///
    /// The text is in `Fm1`, drawn only through `Fm0`, and `Fm0` is placed
    /// past the cap, so `Fm0`'s placements share one stream whose `/Fm1 Do`
    /// is never pointed at a copy. Were `Fm1` cut exactly, its covered
    /// placement would get a copy nothing draws, and the stream every `Fm0`
    /// placement draws would still name `Fm1`'s own object — uncut, because
    /// most of its placements are. So it is not: `Fm1` is cut in place too,
    /// `SECRET` is in no stream, and both forms are named.
    #[test]
    fn a_form_drawn_by_one_placed_past_the_cap_is_cut_the_old_way_too() {
        let placements = MAX_PLACEMENTS + 2;
        let mut content = String::new();
        for i in 0..placements {
            content.push_str(&format!("q 1 0 0 1 0 {} cm /Fm0 Do Q\n", i * 4));
        }
        let inner = "BT /F0 12 Tf 10 50 Td (SECRET) Tj ET";
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 600]\n\
             /Resources << /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        out.push_str(&super::tests_support::stream_object(4, &content));
        out.push_str(
            "5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 600]\n\
             /Resources << /XObject << /Fm1 7 0 R >> >> /Length 8 >>\nstream\n\
             /Fm1 Do\nendstream\nendobj\n",
        );
        out.push_str("6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
        out.push_str(&format!(
            "7 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 600]\n\
             /Resources << /Font << /F0 6 0 R >> >> /Length {} >>\nstream\n\
             {inner}\nendstream\nendobj\n",
            inner.len() + 1
        ));
        out.push_str("trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");

        let (streams, report) =
            redact_to_streams(open_arc(out.into_bytes()), &[area(0.0, 45.0, 400.0, 65.0)]);
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert_eq!(
            report.warnings,
            vec![
                RedactionWarning::RepeatedForm {
                    form: b"Fm0".to_vec(),
                    placements: MAX_PLACEMENTS,
                },
                RedactionWarning::RepeatedForm {
                    form: b"Fm1".to_vec(),
                    placements: MAX_PLACEMENTS,
                },
            ]
        );
    }

    /// `SECRET` in `Fm1`, drawn only through `Fm0`, which the page draws at
    /// page y 0 and again 150 points up.
    fn bytes_of_nested() -> Vec<u8> {
        b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]\n\
   /Resources << /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 56 >>\nstream\n\
q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 300]\n\
   /Resources << /XObject << /Fm1 7 0 R >> >> /Length 10 >>\nstream\n\
/Fm1 Do\n\
endstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
7 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 300]\n\
   /Resources << /Font << /F0 6 0 R >> >> /Length 39 >>\nstream\n\
BT /F0 12 Tf 10 50 Td (SECRET) Tj ET\n\
endstream\nendobj\n\
trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n"
            .to_vec()
    }

    /// An image drawn twice and covered only at its **second** placement is
    /// scrubbed.
    ///
    /// The same `visited` set caused this, and it is the same silent failure
    /// — `images: 0`, reading exactly like a rectangle over nothing. An image
    /// needs no copy to fix it: it is replaced whole or not at all, so every
    /// placement is tested and the first covered one scrubs it, once.
    #[test]
    fn an_image_drawn_twice_is_scrubbed_from_its_second_placement() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.7\n");
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        bytes.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]\n\
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        // The first placement sits at page y 10..40, the second at y 210..240.
        let content = b"q 30 0 0 30 60 10 cm /Im0 Do Q q 30 0 0 30 60 210 cm /Im0 Do Q\n";
        bytes.extend_from_slice(
            format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        );
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(b"endstream\nendobj\n");
        bytes.extend_from_slice(
            b"5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
              /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\nstream\n",
        );
        bytes.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");
        bytes.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        let over_the_second = Redaction {
            area: Rect {
                x0: 50.0,
                y0: 200.0,
                x1: 120.0,
                y1: 250.0,
            },
            mark: false,
        };

        let (_, report) = redact(doc, &[over_the_second]);
        assert_eq!(
            report.images, 1,
            "the second placement was tested and the samples went"
        );
    }

    /// A form that invokes itself must not recurse forever.
    #[test]
    fn a_self_referential_form_terminates() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 10 >>\nstream\n\
/Fm0 Do\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> >> /Length 10 >>\nstream\n\
/Fm0 Do\n\
endstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        let (_, report) = redact_to_streams(doc, &[second_word()]);
        assert_eq!(report.glyphs, 0, "there is no text, and it terminated");
    }

    /// A form that invokes itself under a transform that **moves each round**
    /// must terminate too, and this is the case the object-number guard used
    /// to cover for free.
    ///
    /// Every round is a genuinely different placement — the form lands ten
    /// points further up the page each time — so no comparison of matrices
    /// may call two of them one, and none does. What stops it is a pair of
    /// counts: `MAX_FORM_DEPTH` bounds a chain that recurses, which is this
    /// one, and `MAX_PLACEMENTS` bounds one that spreads
    /// (`a_form_placed_more_times_than_the_cap_saturates_its_count`). That is
    /// the whole reason the placement key is bitwise rather than a tolerance:
    /// a tolerance would have to be loose enough to stop this, and would then
    /// be loose enough to call two real placements one and leave one of them
    /// uncut.
    ///
    /// Each round is a `Do` inside the form, so the chain is bounded by the
    /// depth and the count is `MAX_FORM_DEPTH` placements plus the one the
    /// page itself made.
    #[test]
    fn a_self_referential_form_under_a_moving_transform_terminates() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 10 >>\nstream\n\
/Fm0 Do\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
   /Length 60 >>\nstream\n\
BT /F0 12 Tf 10 50 Td (SECRET) Tj ET\n\
q 1 0 0 1 0 10 cm /Fm0 Do Q\n\
endstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n";

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        let band = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 45.0,
                x1: 400.0,
                y1: 65.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[band]);
        assert_eq!(
            report.glyphs, 6,
            "the placement under the rectangle was measured"
        );
        assert!(!streams.contains("SECRET"), "and cut: {streams}");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::RepeatedForm {
                form: b"Fm0".to_vec(),
                placements: MAX_FORM_DEPTH as usize + 1,
            }],
            "the page's placement and one per level of depth, and then it stops"
        );
    }

    /// A form placed more times than [`MAX_PLACEMENTS`] has the placements
    /// past the cap measured against nothing, and the saturated count in the
    /// warning is what says so.
    ///
    /// This is the one case where `RepeatedForm` still means content may have
    /// *survived* under a rectangle rather than only that too much went, so
    /// the two have to be tellable apart from the report alone — a count
    /// equal to the cap is the signal, and it is why the field is a count
    /// rather than a flag.
    #[test]
    fn a_form_placed_more_times_than_the_cap_saturates_its_count() {
        let placements = MAX_PLACEMENTS + 6;
        let mut content = String::new();
        for i in 0..placements {
            // Each one four points further up: every placement distinct, and
            // all of them clear of the rectangle except the first.
            content.push_str(&format!("q 1 0 0 1 0 {} cm /Fm0 Do Q\n", i * 4));
        }

        let form = "BT /F0 12 Tf 10 50 Td (SECRET) Tj ET";
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 600]\n\
             /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
             /Contents 4 0 R >>\nendobj\n",
        );
        out.push_str(&format!(
            "4 0 obj\n<< /Length {} >>\nstream\n{content}endstream\nendobj\n",
            content.len()
        ));
        out.push_str(&format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 600]\n\
             /Resources << /Font << /F0 6 0 R >> >> /Length {} >>\nstream\n\
             {form}\nendstream\nendobj\n",
            form.len() + 1
        ));
        out.push_str("6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
        out.push_str("trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");

        let doc = Arc::new(CosDocument::open(out.into_bytes()).expect("it opens"));
        let over_the_first = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 45.0,
                x1: 400.0,
                y1: 65.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[over_the_first]);
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::RepeatedForm {
                form: b"Fm0".to_vec(),
                placements: MAX_PLACEMENTS,
            }],
            "the count saturates at the cap rather than reporting {placements}"
        );
    }

    /// An image under a redaction is scrubbed, not covered. A rectangle
    /// painted over a photograph removes nothing at all: the samples stay in
    /// the file for anyone who decompresses it.
    #[test]
    fn an_image_under_a_redaction_is_scrubbed() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.7\n");
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        bytes.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        let content = b"q 100 0 0 100 60 50 cm /Im0 Do Q\n";
        bytes.extend_from_slice(
            format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        );
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(b"endstream\nendobj\n");

        // Recognisable samples, so their absence is checkable.
        let samples = b"SECRETPIXELDATA!";
        bytes.extend_from_slice(
            format!(
                "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4\n\
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                samples.len()
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(samples);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");
        bytes.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        assert!(
            all_streams(&doc).contains("SECRETPIXEL"),
            "the samples start present"
        );

        // The image occupies 60..160 by 50..150 on the page.
        let over_the_image = Redaction {
            area: Rect {
                x0: 80.0,
                y0: 70.0,
                x1: 120.0,
                y1: 110.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[over_the_image]);

        assert_eq!(report.images, 1, "the image was scrubbed");
        assert!(
            !streams.contains("SECRETPIXEL"),
            "and its samples are gone from every stream: {streams}"
        );
    }

    /// An image the redaction does not touch is left alone. Scrubbing every
    /// image on a page would destroy content nobody asked to remove.
    #[test]
    fn an_image_outside_the_redaction_survives() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.7\n");
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        bytes.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        let content = b"q 20 0 0 20 5 5 cm /Im0 Do Q\n";
        bytes.extend_from_slice(
            format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        );
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(b"endstream\nendobj\n");

        let samples = b"KEEPTHESEPIXELS!";
        bytes.extend_from_slice(
            format!(
                "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4\n\
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                samples.len()
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(samples);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");
        bytes.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));

        // Far from the image, which sits at 5..25 on both axes.
        let elsewhere = Redaction {
            area: Rect {
                x0: 300.0,
                y0: 150.0,
                x1: 380.0,
                y1: 190.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[elsewhere]);

        assert_eq!(report.images, 0);
        assert!(
            streams.contains("KEEPTHESE"),
            "an untouched image keeps its samples"
        );
    }

    #[test]
    fn a_page_that_does_not_exist_is_refused() {
        let doc = document("text");
        let mut editor = DocumentEditor::new(doc);
        assert!(apply(&mut editor, 9, &[second_word()]).is_none());
    }

    /// Removing the middle of a line must not shift what follows it: the gap
    /// is re-emitted as a `TJ` displacement.
    #[test]
    fn surviving_text_keeps_its_position() {
        let doc = document("AAA BBB CCC");
        let (streams, report) = redact_to_streams(
            doc,
            &[Redaction {
                // A band over the middle word only. In 12pt Helvetica
                // `AAA ` ends at 37.35 and ` CCC` starts at 61.36, so this
                // takes all three Bs and neither space.
                area: Rect {
                    x0: 37.5,
                    y0: 45.0,
                    x1: 61.0,
                    y1: 70.0,
                },
                mark: false,
            }],
        );

        assert!(report.glyphs > 0);
        assert!(streams.contains("] TJ"), "re-emitted as an array");
        assert_eq!(report.glyphs, 3, "the three Bs and nothing else");
        assert!(
            streams.contains("AAA") && streams.contains("CCC"),
            "the outer words survive: {streams}"
        );
        assert!(!streams.contains("BBB"), "the middle word is gone");
    }

    /// A form whose content was **compressed** is written back as a stream
    /// that decodes.
    ///
    /// The rewrite hands the writer plain operators, and until September 2026
    /// it handed them over with the file's own stream dictionary, so a
    /// `/FlateDecode` form kept its `/Filter` over bytes that were never
    /// deflated. The saved file then carried a stream no reader can decode:
    /// the form drew nothing — the text nobody asked to remove included — and
    /// the strict validator names it (7.4). Every level is asserted, because
    /// the failure showed at all of them: the validator, the streams,
    /// extraction and the ink. Both writer settings too, since `compress`
    /// is the path that would have encoded plain bytes and so hidden the
    /// defect on half the saves.
    #[test]
    fn a_compressed_form_is_written_back_as_a_stream_that_decodes() {
        use super::tests_support::{compressed_form_document, ink_in, open, render};

        // Ten-point boxes one em wide from x 20: `PUBLIC` is x 20..80 and
        // `SECRET` is x 80..140, all on y 100..110. The Helvetica line below
        // is for the extractor: `PUBLIC ` ends near x 59 at ten point.
        let doc = open(compressed_form_document(
            "BT /F0 10 Tf 20 100 Td (PUBLICSECRET) Tj ET \
             BT /F1 10 Tf 20 50 Td (PUBLIC SECRET) Tj ET",
        ));
        assert!(
            all_streams(&doc).contains("PUBLICSECRET"),
            "the needle starts present, compressed"
        );
        let band = Redaction {
            area: Rect {
                x0: 82.0,
                y0: 95.0,
                x1: 150.0,
                y1: 115.0,
            },
            mark: false,
        };

        let line = Redaction {
            area: Rect {
                x0: 60.0,
                y0: 45.0,
                x1: 150.0,
                y1: 65.0,
            },
            mark: false,
        };

        for compress in [false, true] {
            let mut editor = DocumentEditor::new(Arc::clone(&doc));
            let report = apply(&mut editor, 0, &[band, line]).expect("the page exists");
            assert_eq!(report.glyphs, 12, "SECRET twice, measured inside the form");
            let bytes = editor.save(&WriteOptions {
                mode: WriteMode::Rewrite,
                compress,
                ..WriteOptions::default()
            });

            let reopened = CosDocument::open(bytes.clone()).expect("it reopens");
            let defects = tinker_pdf_cos::validate(&reopened);
            assert!(
                defects.is_empty(),
                "the saved file is clean (compress: {compress}): {defects:?}"
            );
            let streams = all_streams(&reopened);
            assert!(!streams.contains("SECRET"), "got: {streams}");
            assert!(streams.contains("PUBLIC"), "got: {streams}");

            let text = crate::Document::open(bytes.clone())
                .expect("it reopens")
                .page(0)
                .expect("a page")
                .text()
                .plain_text();
            assert!(
                text.contains("PUBLIC") && !text.contains("SECRET"),
                "the form still draws what it kept (compress: {compress}): {text:?}"
            );

            let bitmap = render(bytes);
            assert_eq!(ink_in(&bitmap, 200.0, band.area), 0, "no ink under it");
            assert!(
                ink_in(
                    &bitmap,
                    200.0,
                    Rect {
                        x0: 22.0,
                        y0: 102.0,
                        x1: 78.0,
                        y1: 108.0,
                    }
                ) > 100,
                "and the first word still renders (compress: {compress})"
            );
        }
    }

    /// A content stream the tokenizer cannot make sense of must come back
    /// intact rather than truncated — never fail the page (ruling 2).
    #[test]
    fn garbage_content_survives_the_rewrite() {
        let fonts = HashMap::new();
        let content = b"q 1 0 0 1 0 0 cm ) ) ) >> BI garbage EI Q";
        let (out, report, _) = rewrite(content, &[second_word()], &fonts, Matrix::IDENTITY);
        assert_eq!(report, RedactionReport::default());
        assert!(out.contains(&b'q'), "the operators survive");
    }
}

/// Runs whose text matrix rotates or skews, which this module used to refuse.
///
/// # What is adjudicated by what
///
/// Nothing here is checked against another implementation, and nothing needs
/// to be: the third-party data is **the standard itself** — ISO 32000-1's
/// 9.4.2 (the text and text line matrices), 9.4.3 (`TJ`'s displacement, and
/// what `'` and `\"` expand to) and 9.4.4 (the glyph displacement formula and
/// the text rendering matrix), all three carrying the same numbers in ISO
/// 32000-2 — and the fixtures are arithmetic done by hand from
/// those clauses and written into the assertions as expected page-space
/// coordinates. A fixture says "glyph six of this run occupies page x 90..100,
/// y 80..90", which is a claim about the specification that a reader can check
/// with a pencil, not a claim that two of this repository's own components
/// agree with each other.
///
/// The one link that is **self-consistency and is labelled as such**: the ink
/// assertions read this repository's own renderer. That the renderer draws
/// glyph six where the specification puts it is not proved here, and stating
/// where it *is* proved is part of labelling this link honestly:
/// `crates/tinker-pdf/tests/render_analytic.rs` holds the renderer to formulas
/// ISO 32000-1 publishes as expressions, and `render_differential.rs` holds
/// pairs of documents that must draw the same picture. The golden bitmaps
/// beside them are `UNREVIEWED` and the roadmap says so, which is why they are
/// not named here. What the ink assertions add is orthogonal to all of that
/// and is the
/// reason they exist: a redaction that removed the bytes from one stream while
/// leaving a second, unreferenced copy of them elsewhere passes every
/// byte-level check in this file and fails the ink check, because the page
/// still draws the glyph. Neither level is the safety property on its own.
#[cfg(test)]
mod rotated_runs {
    use super::tests_support::*;
    use super::*;

    /// A quarter turn: the run walks **up** the page.
    ///
    /// `0 1 -1 0 100 20 Tm` maps unscaled text space `(x, y)` to page
    /// `(100 - y, 20 + x)`. With a ten-point font whose every glyph is one em
    /// wide, glyph `k` therefore occupies page x 90..100 and page
    /// y `20 + 10k` .. `30 + 10k`. `PUBLIC` is glyphs 0..5, page y 20..80;
    /// `SECRET` is glyphs 6..11, page y 80..140.
    fn quarter_turn() -> Vec<u8> {
        boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 0 1 -1 0 100 20 Tm (PUBLICSECRET) Tj ET",
        )
    }

    /// The band over `SECRET` and nothing else.
    ///
    /// Two points of clearance below glyph six's box rather than a boundary
    /// sitting exactly on glyph five's edge, so that the ink assertion is not
    /// reading an antialiased pixel of the glyph that survived.
    fn upper_band() -> Redaction {
        Redaction {
            area: Rect {
                x0: 85.0,
                y0: 82.0,
                x1: 105.0,
                y1: 145.0,
            },
            mark: false,
        }
    }

    /// The headline, and the row's exit criterion: a rotated run is cut, and
    /// cut along its own baseline rather than along the page's.
    ///
    /// Both levels of the safety property are asserted here, because either
    /// alone passes a build the other fails: the bytes are gone from **every**
    /// decompressed stream in the file, and the redacted page **draws no ink**
    /// inside the rectangle.
    #[test]
    fn a_rotated_run_is_cut_along_its_own_baseline() {
        let doc = open(quarter_turn());
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts out present, or the test proves nothing"
        );

        let (bytes, report) = redact(doc, &[upper_band()]);
        assert_eq!(report.glyphs, 6, "the six glyphs of the second word");
        assert!(report.warnings.is_empty(), "nothing was refused");

        let reopened = CosDocument::open(bytes.clone()).expect("it reopens");
        let streams = all_streams(&reopened);
        assert!(
            !streams.contains("SECRET"),
            "the covered glyphs' codes are gone from every stream: {streams}"
        );
        assert!(
            streams.contains("PUBLIC"),
            "and the first word is still there: {streams}"
        );

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, upper_band().area),
            0,
            "no ink survives inside the redacted rectangle"
        );
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 90.0,
                    y0: 21.0,
                    x1: 100.0,
                    y1: 79.0,
                }
            ) > 100,
            "and the part of the run nobody redacted still draws"
        );
    }

    /// The run's matrix is the original bytes, untouched, and there is still
    /// exactly one of them.
    ///
    /// Re-anchoring each surviving sub-run with a `Tm` of its own is the
    /// plausible-looking wrong answer; [`emit_array`] gives the four reasons.
    #[test]
    fn a_cut_rotated_run_keeps_the_text_matrix_it_had() {
        let (bytes, _) = redact(open(quarter_turn()), &[upper_band()]);
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));

        assert!(
            streams.contains("0 1 -1 0 100 20 Tm"),
            "the original matrix is re-emitted verbatim: {streams}"
        );
        assert_eq!(
            streams.matches("Tm").count(),
            1,
            "and no second one was invented: {streams}"
        );
    }

    /// Removing a glyph from the middle must leave everything after it where
    /// it was on the page — which, for a rotated run, means further **up**,
    /// not further right.
    ///
    /// The band covers glyph three (`L`, page y 50..60) and nothing else. If
    /// the removed glyph's displacement were dropped, every later glyph would
    /// slide ten points back down the baseline toward the run's origin: the
    /// sample at the tail would go blank and the sample inside the cut would
    /// light up. Both are asserted, so that neither a shift nor a leak passes.
    #[test]
    fn removing_a_rotated_glyph_leaves_the_tail_where_it_was() {
        let band = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 51.0,
                x1: 105.0,
                y1: 59.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(open(quarter_turn()), &[band]);
        assert_eq!(report.glyphs, 1, "glyph three alone");

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, band.area),
            0,
            "the cut glyph is gone"
        );
        // Glyph four, which follows the cut, is still at page y 60..70.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 92.0,
                    y0: 62.0,
                    x1: 98.0,
                    y1: 68.0,
                }
            ) > 20,
            "the glyph after the cut did not slide back down the baseline"
        );
        // And the last glyph is still at page y 130..140.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 92.0,
                    y0: 132.0,
                    x1: 98.0,
                    y1: 138.0,
                }
            ) > 20,
            "nor did the end of the run"
        );
    }

    /// A glyph the rectangle covers only partly goes, because the only other
    /// choice is leaving a glyph that was partly covered — and only one of
    /// those two can leak.
    ///
    /// The band is a seven-by-four sliver straddling the boundary between
    /// glyphs three and four, clipping a corner of each. Neither is contained.
    #[test]
    fn a_partly_covered_rotated_glyph_is_removed() {
        let sliver = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 58.0,
                x1: 92.0,
                y1: 62.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(open(quarter_turn()), &[sliver]);
        assert_eq!(report.glyphs, 2, "both partly covered glyphs went");

        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(
            !streams.contains("LI"),
            "glyphs three and four are both gone: {streams}"
        );
    }

    /// A rotation that is not a multiple of a quarter turn, where every glyph
    /// box is a square standing on a corner and no axis-aligned reading of the
    /// run can come close.
    ///
    /// `0.6 0.8 -0.8 0.6 20 20 Tm` maps `(x, y)` to
    /// `(0.6x - 0.8y + 20, 0.8x + 0.6y + 20)`. Glyph `k` of a ten-point run
    /// therefore has corners at `(6k+20, 8k+20)`, `(6k+26, 8k+28)`,
    /// `(6k+18, 8k+34)` and `(6k+12, 8k+26)` — a square of side ten rotated
    /// 53 degrees, stepping ten points up the diagonal per glyph.
    ///
    /// The rectangle starts at `x = 73, y = 107`. Glyph eleven lies wholly
    /// inside it; glyph ten reaches it at the corner `(78, 114)`; glyph nine's
    /// highest corner is `(72, 106)`, a point below the rectangle's floor. So
    /// exactly two glyphs go, and the last of them only because the test
    /// followed the rotation.
    #[test]
    fn an_obliquely_rotated_run_is_cut_along_its_own_baseline() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 0.6 0.8 -0.8 0.6 20 20 Tm (PUBLICSECRET) Tj ET",
        ));
        assert!(all_streams(&doc).contains("PUBLICSECRET"));

        let corner = Redaction {
            area: Rect {
                x0: 73.0,
                y0: 107.0,
                x1: 200.0,
                y1: 200.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[corner]);
        assert_eq!(report.glyphs, 2, "the last two glyphs and no others");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(
            !streams.contains("PUBLICSECRET"),
            "the run was cut: {streams}"
        );
        assert!(
            streams.contains("PUBLICSECR"),
            "and cut in exactly one place: {streams}"
        );

        assert_eq!(
            ink_in(&render(bytes), 200.0, corner.area),
            0,
            "no ink survives inside the redacted rectangle"
        );
    }

    /// A skew, where the two edges of a glyph box are not perpendicular and
    /// the box's own extent is not its bounding box.
    ///
    /// `1 0 2 1 20 100 Tm` leans the em box two units right for every one up:
    /// glyph `k` of a ten-point run spans page x `10k+20 .. 10k+30` at its
    /// baseline and `10k+40 .. 10k+50` at its top. The rectangle is the
    /// quarter-plane `x >= 62, y >= 105`. Glyphs four and five reach it even
    /// if the lean is ignored; glyphs two and three reach it **only** through
    /// the lean — their upright boxes stop at x 50 and x 60. Four glyphs go,
    /// and a build that read the box upright would cut two.
    #[test]
    fn a_skewed_run_is_cut_along_its_own_baseline() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 1 0 2 1 20 100 Tm (SECRET) Tj ET",
        ));
        assert!(all_streams(&doc).contains("SECRET"));

        let leaning = Redaction {
            area: Rect {
                x0: 62.0,
                y0: 105.0,
                x1: 200.0,
                y1: 200.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[leaning]);
        assert_eq!(
            report.glyphs, 4,
            "the four glyphs whose leaning boxes reach the rectangle"
        );

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("CRET"), "got: {streams}");

        assert_eq!(
            ink_in(&render(bytes), 200.0, leaning.area),
            0,
            "no ink survives inside the redacted rectangle"
        );
    }

    /// A rotated run is measured through the `cm` in force as well as through
    /// its own matrix, because the rectangle is in page space and neither
    /// matrix alone gets there.
    ///
    /// The same quarter turn as above, drawn under `1 0 0 1 0 -60 cm`, which
    /// slides the whole run sixty points down the page. `SECRET` now occupies
    /// page y 20..80, and the band that used to take it takes nothing.
    #[test]
    fn a_rotated_run_is_measured_through_the_transform_too() {
        let shifted = boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "q 1 0 0 1 0 -60 cm BT /F0 10 Tf 0 1 -1 0 100 20 Tm (PUBLICSECRET) Tj ET Q",
        );

        let (_, high) = redact(open(shifted.clone()), &[upper_band()]);
        assert_eq!(high.glyphs, 0, "the run moved out from under the band");

        let lowered = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 22.0,
                x1: 105.0,
                y1: 85.0,
            },
            mark: false,
        };
        let (bytes, low) = redact(open(shifted), &[lowered]);
        assert_eq!(low.glyphs, 6, "and is under the band that followed it");
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
    }

    /// The rotation can live in the `cm` while the `Tm` is perfectly ordinary,
    /// and the run is still rotated on the page.
    ///
    /// This is the hole the old refusal had. It carried one `skewed` flag,
    /// `cm` could only set it and `Tm` *assigned* it — so a rotated `cm`
    /// followed by an axis-aligned `Tm` cleared the flag and the run was cut
    /// by a pen walking along the page's x axis under a quarter turn. Not a
    /// refusal that was too broad: a refusal that was not there, on a run it
    /// then cut from positions ninety degrees out.
    ///
    /// `0 1 -1 0 200 0 cm` maps `(x, y)` to `(200 - y, x)` and the `Tm` puts
    /// the run's origin at text `(20, 20)`, so unscaled text space `(x, y)`
    /// lands at page `(180 - y, x + 20)`: the run climbs the page at
    /// x 170..180, glyph `k` at y `20 + 10k` .. `30 + 10k`, and `SECRET` is
    /// y 80..140 again.
    #[test]
    fn a_rotation_that_lives_in_the_transform_is_followed_too() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "q 0 1 -1 0 200 0 cm BT /F0 10 Tf 1 0 0 1 20 20 Tm (PUBLICSECRET) Tj ET Q",
        ));

        let band = Redaction {
            area: Rect {
                x0: 165.0,
                y0: 82.0,
                x1: 185.0,
                y1: 145.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[band]);
        assert_eq!(report.glyphs, 6, "the six glyphs of the second word");
        assert!(report.warnings.is_empty(), "nothing was refused");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(streams.contains("PUBLIC"), "got: {streams}");

        assert_eq!(
            ink_in(&render(bytes), 200.0, band.area),
            0,
            "no ink survives inside the redacted rectangle"
        );
    }

    /// A `TJ` number in a rotated run keeps its own meaning, and a `Tm` that
    /// carries the font size does not multiply into the replacement gap.
    ///
    /// `/F0 1 Tf` with `0 10 -10 0 100 20 Tm` is the same ten-point quarter
    /// turn written the other ordinary way: the size is in the matrix rather
    /// than in the `Tf`. 9.4.3 measures a `TJ` number in thousandths of
    /// `Tfs · Th` — which is one here, not ten — so a gap re-emitted with the
    /// matrix's scale folded in would be ten times too long and the tail of
    /// the run would fly off the page.
    #[test]
    fn a_gap_is_re_emitted_in_the_runs_own_units() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 1 Tf 0 10 -10 0 100 20 Tm (PUBLICSECRET) Tj ET",
        ));

        // Glyph three again: page y 50..60, exactly as in the `10 Tf` form.
        let band = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 51.0,
                x1: 105.0,
                y1: 59.0,
            },
            mark: false,
        };
        let (bytes, report) = redact(doc, &[band]);
        assert_eq!(report.glyphs, 1, "the matrix placed the glyphs");

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, band.area),
            0,
            "the cut glyph is gone"
        );
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 92.0,
                    y0: 132.0,
                    x1: 98.0,
                    y1: 138.0,
                }
            ) > 20,
            "and the end of the run is still on the page where it was"
        );
    }

    /// The font is part of the graphics state `q` saves, so a `Tf` inside a
    /// `q`/`Q` pair does not survive the `Q`.
    ///
    /// The size was already being restored and the face was not, which meant
    /// the advances after a `Q` were computed from one font's widths at
    /// another font's size — positions that are wrong by however much the two
    /// faces differ, which is the "approximately right" cut this whole module
    /// refuses.
    ///
    /// Inside the pair the font is a Type 3 face whose glyph space is ten
    /// times the conventional one, so each of its glyphs is ten ems wide. The
    /// run after the `Q` names no font of its own — the `Tf` before the `q`
    /// is the one that still applies — so a selection that leaked past the
    /// `Q` would measure the outer run a hundred points a glyph, and the band
    /// over `SECRET` would take `P` and `U` instead.
    ///
    /// Until September 2026 the inner face was one redaction refused to
    /// measure, and the leak showed as a refusal in the report; it shows in
    /// the geometry now, which is where the cut is.
    #[test]
    fn a_font_selected_inside_a_q_does_not_outlive_it() {
        let doc = open(two_font_document(
            "/F0 10 Tf
             q /F1 10 Tf BT 10 150 Td (INSIDE) Tj ET Q
             BT 0 1 -1 0 100 20 Tm (PUBLICSECRET) Tj ET",
        ));

        let (bytes, report) = redact(doc, &[upper_band()]);
        assert_eq!(report.glyphs, 6, "the outer run was measured and cut");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(
            streams.contains("PUBLIC") && !streams.contains("SECRET"),
            "the six it cut were SECRET's: {streams}"
        );
        assert!(
            streams.contains("INSIDE"),
            "and the inner run, whose ten-em glyphs start above the band, is whole"
        );
    }
}

/// Runs redaction still refuses to measure, each left whole and named.
///
/// A redaction that silently fails to redact is worse than one that refuses:
/// the caller believes the content is gone and distributes the file. So the
/// question each of these asks is the same — was the text left **and** was the
/// caller told — and both halves are asserted, because a refusal nobody hears
/// is an under-redaction with a clean report.
#[cfg(test)]
mod refusals {
    use super::tests_support::*;
    use super::*;

    /// Everything on the page, so that nothing escapes by being outside.
    fn everywhere() -> Redaction {
        Redaction {
            area: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 200.0,
                y1: 200.0,
            },
            mark: false,
        }
    }

    /// A `Tf` naming a font the resource dictionary does not have leaves the
    /// run with no metrics at all.
    ///
    /// This was silent: the run was kept, nothing was counted, and the report
    /// was indistinguishable from a page where the rectangle covered nothing.
    #[test]
    fn a_run_whose_font_is_not_in_scope_is_left_uncut_and_reported() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /Missing 10 Tf 10 100 Td (SECRET) Tj ET",
        ));

        let (bytes, report) = redact(doc, &[everywhere()]);
        assert_eq!(report.glyphs, 0);
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::UnknownFont {
                font: b"Missing".to_vec(),
                bytes: 6,
            }]
        );
        assert!(all_streams(&CosDocument::open(bytes).expect("it reopens")).contains("SECRET"));
    }

    /// A text rendering matrix with an entry that is not a number.
    ///
    /// A single operand cannot deliver one: every operand this module reads is
    /// rejected unless it is finite. It arrives instead the way a real file
    /// would deliver it, out of the *composition* — a text matrix scaled by
    /// 10^300 under a `cm` scaled by 10^300, each perfectly readable on its
    /// own, whose product is not a number. Nothing can be measured against a
    /// rectangle from there, and the run is left **whole**: one abandoned
    /// part-way through would be a run cut from positions this module had
    /// already decided it could not trust.
    #[test]
    fn a_non_finite_text_matrix_is_left_uncut_and_reported() {
        let huge = format!("1{}", "0".repeat(300));
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            &format!(
                "q {huge} 0 0 {huge} 0 0 cm \
                 BT /F0 10 Tf {huge} 0 0 {huge} 10 100 Tm (SECRET) Tj ET Q"
            ),
        ));

        let (bytes, report) = redact(doc, &[everywhere()]);
        assert_eq!(report.glyphs, 0, "nothing was cut");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::UnmeasurableFrame {
                font: b"F0".to_vec(),
                bytes: 6,
            }]
        );
        assert!(all_streams(&CosDocument::open(bytes).expect("it reopens")).contains("SECRET"));
    }

    /// A refusal is about a rectangle, so with no rectangles there is nothing
    /// to refuse. Warning on every unmeasurable run of every page a caller
    /// merely opened would make the list say nothing.
    ///
    /// Written over a vertical run until September 2026, when vertical runs
    /// stopped being refused; a font that is not in scope is the refusal now.
    #[test]
    fn nothing_is_refused_when_there_is_nothing_to_redact() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /Missing 10 Tf 100 100 Td (SECRET) Tj ET",
        ));
        let (_, report) = redact(doc, &[]);
        assert_eq!(report, RedactionReport::default());
    }

    /// Warnings with the same cause and the same font merge, so a page of
    /// text in a font that is not in scope yields one entry rather than one
    /// per operator.
    #[test]
    fn refusals_of_the_same_cause_and_font_merge() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /Missing 10 Tf 100 100 Td (SECRET) Tj 0 -12 Td (AGAIN) Tj ET",
        ));
        let (_, report) = redact(doc, &[everywhere()]);
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::UnknownFont {
                font: b"Missing".to_vec(),
                bytes: 11,
            }],
            "one entry, carrying both runs' operand lengths"
        );
    }
}

/// Vertical runs (9.7.4.3), which this module refused until September 2026.
///
/// # What is adjudicated by what
///
/// The fixture's font writes every one of its metrics out — `/W`, `/W2` with
/// one glyph of a different height — so the expected column is arithmetic
/// from 9.4.4 and 9.7.4.3 done by hand and written into the comments: glyph
/// by glyph, where each box starts and ends. What reads the result back is
/// this engine's own extractor, and the positions it reports are compared
/// **with themselves**, before and after the cut: the property is that every
/// glyph the redaction kept is exactly where it was. That is
/// self-consistency and is labelled as such — it says the rewrite moved
/// nothing, and it is the interpreter's placement (`interpret.rs`, `show`)
/// that says where "where it was" is. The font has no program, so there is no
/// ink to read; the needle bytes are read out of every decoded stream as
/// everywhere else in this file.
#[cfg(test)]
mod vertical_runs {
    use super::tests_support::*;
    use super::*;

    fn band(x0: f64, y0: f64, x1: f64, y1: f64) -> Redaction {
        Redaction {
            area: Rect { x0, y0, x1, y1 },
            mark: false,
        }
    }

    /// `PUBLICSECRET` down a column from page (100, 180), ten point.
    ///
    /// Each glyph advances ten points down and `I` five, so the boxes run
    /// P 170..180, U 160..170, B 150..160, L 140..150, **I 135..140**,
    /// C 125..135, S 115..125, E 105..115, C 95..105, R 85..95, E 75..85,
    /// T 65..75 — all at x 95..105.
    fn column(prefix: &str) -> Vec<u8> {
        cid_vertical_document(&format!(
            "BT /F0 10 Tf {prefix} 100 180 Td {} Tj ET",
            cid_hex("PUBLICSECRET")
        ))
    }

    /// The needle as it sits in a stream: two bytes a code.
    fn wide(text: &str) -> String {
        text.chars().flat_map(|c| ['\0', c]).collect()
    }

    /// What survived, in order, with where it was drawn.
    fn kept_positions(before: &[(String, (f64, f64))], kept: &str) -> Vec<(String, (f64, f64))> {
        let mut out = Vec::new();
        let mut wanted = kept.chars().peekable();
        for (text, origin) in before {
            if wanted.peek().map(|c| c.to_string()) == Some(text.clone()) {
                wanted.next();
                out.push((text.clone(), *origin));
            }
        }
        out
    }

    fn assert_same_places(after: &[(String, (f64, f64))], expected: &[(String, (f64, f64))]) {
        assert_eq!(
            after.iter().map(|(t, _)| t.as_str()).collect::<String>(),
            expected.iter().map(|(t, _)| t.as_str()).collect::<String>(),
            "the kept glyphs, in order"
        );
        for ((text, got), (_, want)) in after.iter().zip(expected) {
            assert!(
                (got.0 - want.0).abs() < 1e-6 && (got.1 - want.1).abs() < 1e-6,
                "{text} moved from {want:?} to {got:?}"
            );
        }
    }

    /// **The pin, flipped.** The fixture the refusal was written against —
    /// `Identity-V` with nothing but `/DW` — is measured and cut now, and
    /// nothing is reported, because nothing was left unmeasured.
    #[test]
    fn a_vertical_run_is_measured_rather_than_refused() {
        let doc = open(vertical_document("BT /F0 10 Tf 100 100 Td (SECRET) Tj ET"));
        assert!(all_streams(&doc).contains("SECRET"));

        let (bytes, report) = redact(doc, &[band(0.0, 0.0, 200.0, 200.0)]);
        assert_eq!(report.glyphs, 3, "three two-byte codes: SE, CR and ET");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
    }

    /// The row's exit criterion: a rectangle over part of a column cuts
    /// exactly the glyphs it covers, and every glyph it does not cover is
    /// still drawn exactly where it was.
    ///
    /// The band is y 64..124: `T`'s box (65..75) is inside it and `S`'s
    /// (115..125) reaches into it; `C`'s (125..135) stops a point above its
    /// top. So `SECRET` goes and `PUBLIC` stays — and a build that walked the
    /// column horizontally would find every glyph at y 180 and cut nothing.
    #[test]
    fn a_vertical_run_is_cut_exactly_at_the_covered_glyphs() {
        let bytes = column("");
        let before = extracted(bytes.clone());
        assert_eq!(
            before.iter().map(|(t, _)| t.as_str()).collect::<String>(),
            "PUBLICSECRET",
            "the extractor reads the column before the cut"
        );

        let secret = band(90.0, 64.0, 110.0, 124.0);
        let (after_bytes, report) = redact(open(bytes), &[secret]);
        assert_eq!(report.glyphs, 6, "S, E, C, R, E and T");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let streams = all_streams(&CosDocument::open(after_bytes.clone()).expect("it reopens"));
        assert!(!streams.contains(&wide("SECRET")), "the codes are gone");
        assert!(
            streams.contains(&wide("PUBLIC")),
            "the kept codes are there"
        );

        let after = extracted(after_bytes);
        assert_same_places(&after, &kept_positions(&before, "PUBLIC"));
    }

    /// A cut in the middle of the column leaves the tail where it was, which
    /// is only true if the gap was emitted **down** the column and in the
    /// vertical thousandth.
    ///
    /// The band (y 136..149) takes `L` (140..150) and the short `I`
    /// (135..140) and neither neighbour: the gap is fifteen points, not
    /// twenty, so a gap computed from `/W` or from a uniform `w1` puts the
    /// tail five points off. The run is at `50 Tz` as well, which 9.4.4 puts
    /// in `tx` and not in `ty`: a gap divided by `Tfs · Th` rather than `Tfs`
    /// is emitted twice as long.
    #[test]
    fn removing_a_vertical_glyph_leaves_the_tail_where_it_was() {
        let bytes = column("50 Tz");
        let before = extracted(bytes.clone());

        // At `50 Tz` the box is x 97.5..102.5; the band still spans it.
        let (after_bytes, report) = redact(open(bytes), &[band(90.0, 136.0, 110.0, 149.0)]);
        assert_eq!(report.glyphs, 2, "L and I");

        let after = extracted(after_bytes);
        assert_same_places(&after, &kept_positions(&before, "PUBCSECRET"));
    }

    /// A `TJ` number in a vertical run displaces **down** the column, and one
    /// the array already carried keeps its sign and its size.
    ///
    /// `[P 500 SECRET]`: the adjustment carries the pen five points further
    /// down after `P`, so `S` is 165..160 — its box 155..165 — and the band
    /// (y 156..164) takes it alone.
    #[test]
    fn a_tj_number_in_a_vertical_run_keeps_its_axis() {
        let bytes = cid_vertical_document(&format!(
            "BT /F0 10 Tf 100 180 Td [{} 500 {}] TJ ET",
            cid_hex("P"),
            cid_hex("SECRET")
        ));
        let before = extracted(bytes.clone());

        let (after_bytes, report) = redact(open(bytes), &[band(90.0, 156.0, 110.0, 164.0)]);
        assert_eq!(report.glyphs, 1, "the S alone");
        let streams = all_streams(&CosDocument::open(after_bytes.clone()).expect("it reopens"));
        assert!(
            streams.contains("500"),
            "the adjustment came back: {streams}"
        );

        let after = extracted(after_bytes);
        assert_same_places(&after, &kept_positions(&before, "PECRET"));
    }

    /// A vertical glyph is drawn centred on its pen — its horizontal origin
    /// at minus the position vector, `v_x = 500` — so a rectangle over the
    /// left half of the column covers it.
    ///
    /// The band is x 94..99 over `S` (y 116..124): all of it left of the
    /// pen at x 100. A box measured from the pen rightward, as a horizontal
    /// glyph's is, starts at x 100 and misses it.
    #[test]
    fn a_vertical_glyph_is_centred_on_its_pen() {
        let (after_bytes, report) = redact(open(column("")), &[band(94.0, 116.0, 99.0, 124.0)]);
        assert_eq!(report.glyphs, 1, "the S");
        let after = extracted(after_bytes);
        assert_eq!(
            after.iter().map(|(t, _)| t.as_str()).collect::<String>(),
            "PUBLICECRET"
        );
    }

    /// A column turned a quarter turn runs **left to right** across the
    /// page, and is cut along its own axis all the same.
    ///
    /// `0 1 -1 0 20 100 Tm` maps text `(x, y)` to page `(20 - y, 100 + x)`,
    /// so a pen walking text-space y downward walks page x rightward: glyph
    /// boxes P 20..30, U 30..40, B 40..50, L 50..60, I 60..65, C 65..75 and
    /// then S 75..85 onwards, all at page y 95..105.
    #[test]
    fn a_turned_vertical_run_is_cut_along_its_own_axis() {
        let bytes = cid_vertical_document(&format!(
            "BT /F0 10 Tf 0 1 -1 0 20 100 Tm {} Tj ET",
            cid_hex("PUBLICSECRET")
        ));
        let before = extracted(bytes.clone());

        let (after_bytes, report) = redact(open(bytes), &[band(76.0, 90.0, 200.0, 110.0)]);
        assert_eq!(report.glyphs, 6, "SECRET");
        let after = extracted(after_bytes);
        assert_same_places(&after, &kept_positions(&before, "PUBLIC"));
    }
}

/// A Type 3 font's own glyph space (9.6.5), which this module refused to
/// measure until September 2026 whenever its `/FontMatrix` was not the
/// 1/1000 default.
///
/// # What is adjudicated by what
///
/// Each fixture's glyph procedure fills a known rectangle of glyph space, so
/// where every glyph's ink lands is arithmetic from 9.4.4 and the font matrix,
/// done by hand in each test's comment. The render is this engine's own and
/// is compared **with itself**: the property is that the redacted page draws
/// nothing inside the rectangle, draws every pixel outside the removed
/// glyphs' own boxes exactly as it did before the cut, and that the codes
/// removed are the ones the arithmetic says were covered. A box built from
/// the matrix's `a` alone — the plausible half-fix — cuts the neighbour of
/// the covered glyph in the skewed and rotated fixtures, and each of those
/// says which neighbour.
#[cfg(test)]
mod type3_glyph_space {
    use super::tests_support::*;
    use super::*;

    fn area(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    fn band(area: Rect) -> Redaction {
        Redaction { area, mark: false }
    }

    /// A glyph space in hundredths: `/FontMatrix [0.01 0 0 0.01 0 0]`, each
    /// glyph 100 units wide and filling `0 0 100 100` — one em square.
    ///
    /// At `10 Tf` from `10 100 Td`, glyph `k` of `PUBLICSECRET` is page
    /// x `10 + 10k` .. `20 + 10k`, y 100..110; `SECRET` is x 70..130. The
    /// band (x 72..125) reaches into `S` and `T` and covers the four between.
    /// `width / 1000` would put all twelve glyphs in x 10..22 and cut none.
    #[test]
    fn a_glyph_space_in_hundredths_is_cut_exactly_at_the_covered_glyphs() {
        let bytes = type3_document(
            200.0,
            "[0.01 0 0 0.01 0 0]",
            "/FontBBox [0 0 100 100]",
            100,
            "100 0 d0 0 0 100 100 re f",
            "BT /F0 10 Tf 10 100 Td (PUBLICSECRET) Tj ET",
        );
        let over = area(72.0, 95.0, 125.0, 115.0);
        let before = render(bytes.clone());
        assert!(ink_in(&before, 200.0, over) > 0, "the band starts inked");

        let (after_bytes, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 6, "S, E, C, R, E and T");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let streams = all_streams(&CosDocument::open(after_bytes.clone()).expect("it reopens"));
        assert!(
            streams.contains("PUBLIC") && !streams.contains("SECRET"),
            "{streams}"
        );

        let after = render(after_bytes);
        assert_eq!(ink_in(&after, 200.0, over), 0, "no ink under the band");
        assert_eq!(
            differing_outside(&before, &after, 200.0, area(70.0, 100.0, 130.0, 110.0)),
            0,
            "PUBLIC renders exactly as it did"
        );
    }

    /// A skewed glyph space: `/FontMatrix [0.001 0 0.0005 0.001 0 0]` slants
    /// each glyph half an em to the right over its height.
    ///
    /// At `20 Tf` from `10 100 Td` glyph `k`'s pen is at x `10 + 20k` and
    /// its ink is the parallelogram whose bottom edge is pen..pen+20 at
    /// y 100 and whose top edge is pen+10..pen+30 at y 120. `S` (k = 6, pen
    /// 130) spans x 137.5..157.5 at y 115 and 139.5..159.5 at y 119; the
    /// band x 151..157, y 115..119 is inside it. `E` (pen 150) starts at
    /// x 157.5 at y 115, past the band. An upright box from `a` alone puts
    /// `S` at x 130..150 and `E` at 150..170 — and cuts `E`.
    #[test]
    fn a_skewed_glyph_space_is_cut_at_the_glyph_its_slant_carries_under_the_band() {
        let bytes = type3_document(
            300.0,
            "[0.001 0 0.0005 0.001 0 0]",
            "/FontBBox [0 0 1500 1000]",
            1000,
            "1000 0 d0 0 0 1000 1000 re f",
            "BT /F0 20 Tf 10 100 Td (PUBLICSECRET) Tj ET",
        );
        let over = area(151.0, 115.0, 157.0, 119.0);
        let before = render(bytes.clone());
        assert!(ink_in(&before, 200.0, over) > 0, "the band starts inked");

        let (after_bytes, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 1, "S alone");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let streams = all_streams(&CosDocument::open(after_bytes.clone()).expect("it reopens"));
        assert!(
            streams.contains("PUBLIC") && streams.contains("ECRET") && !streams.contains("SECRET"),
            "{streams}"
        );

        let after = render(after_bytes);
        assert_eq!(ink_in(&after, 200.0, over), 0, "no ink under the band");
        assert!(
            ink_in(&after, 200.0, area(159.0, 115.0, 162.0, 119.0)) > 0,
            "E, the glyph an upright box would have cut, is still drawn beside it"
        );
        assert_eq!(
            differing_outside(&before, &after, 200.0, area(130.0, 100.0, 160.0, 120.0)),
            0,
            "every other glyph renders exactly as it did"
        );
    }

    /// A rotated glyph space: `/FontMatrix [0.0008 0.0006 -0.0006 0.0008 0 0]`
    /// turns each glyph about 36.87° about its origin, at the conventional
    /// scale.
    ///
    /// Table 112's advance is the horizontal component of the transformed
    /// width, `1000 · 0.0008`: sixteen points at `20 Tf`, so glyph `k`'s pen
    /// is at x `30 + 16k`. Its ink is the square with corners pen + (0, 0),
    /// (16, 12), (4, 28) and (−12, 16) at baseline y 100. The band
    /// x 127..133, y 122..126 sits under `S`'s top corner (k = 6, pen 126,
    /// the corner at 130, 128): `S` spans x 122..134.5 at y 122, `E` (pen
    /// 142) starts at x 138 there and `C` (pen 110) ends at 118.5. An upright
    /// box from `a` alone reaches y 120 and cuts nothing at all.
    #[test]
    fn a_rotated_glyph_space_is_cut_where_its_glyphs_are_turned_to() {
        let bytes = type3_document(
            300.0,
            "[0.0008 0.0006 -0.0006 0.0008 0 0]",
            "/FontBBox [0 0 1000 1000]",
            1000,
            "1000 0 d0 0 0 1000 1000 re f",
            "BT /F0 20 Tf 30 100 Td (PUBLICSECRET) Tj ET",
        );
        let over = area(127.0, 122.0, 133.0, 126.0);
        let before = render(bytes.clone());
        assert!(ink_in(&before, 200.0, over) > 0, "the band starts inked");

        let (after_bytes, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 1, "S alone");
        let streams = all_streams(&CosDocument::open(after_bytes.clone()).expect("it reopens"));
        assert!(
            streams.contains("PUBLIC") && streams.contains("ECRET") && !streams.contains("SECRET"),
            "{streams}"
        );

        let after = render(after_bytes);
        assert_eq!(ink_in(&after, 200.0, over), 0, "no ink under the band");
        // S's own bounding box: x 114..142, y 100..128.
        assert_eq!(
            differing_outside(&before, &after, 200.0, area(114.0, 100.0, 142.0, 128.0)),
            0,
            "every other glyph renders exactly as it did"
        );
    }

    /// A glyph space with a translation: `/FontMatrix [0.001 0 0 0.001 0.5 0]`
    /// draws every glyph half an em to the right of its pen, and advances it
    /// by the width alone — a width is a displacement, which a translation
    /// does not move.
    ///
    /// At `10 Tf` glyph `k`'s pen is x `10 + 10k` and its ink x `15 + 10k` ..
    /// `25 + 10k`. The band x 81..84 is inside `S`'s ink (75..85); a box that
    /// dropped the translation puts `S` at 70..80 and `E` at 80..90, and cuts
    /// `E`.
    #[test]
    fn a_translated_glyph_space_is_cut_where_it_is_drawn() {
        let bytes = type3_document(
            200.0,
            "[0.001 0 0 0.001 0.5 0]",
            "/FontBBox [0 0 1000 1000]",
            1000,
            "1000 0 d0 0 0 1000 1000 re f",
            "BT /F0 10 Tf 10 100 Td (PUBLICSECRET) Tj ET",
        );
        let over = area(81.0, 101.0, 84.0, 109.0);
        let before = render(bytes.clone());
        assert!(ink_in(&before, 200.0, over) > 0, "the band starts inked");

        let (after_bytes, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 1, "S alone");
        let after = render(after_bytes);
        assert_eq!(ink_in(&after, 200.0, over), 0, "no ink under the band");
        assert_eq!(
            differing_outside(&before, &after, 200.0, area(75.0, 100.0, 85.0, 110.0)),
            0,
            "E, the glyph a box without the translation would have cut, and \
             every other glyph render exactly as they did"
        );
    }

    /// **The pin, flipped.** The fixture the refusal was written against —
    /// `/FontMatrix [0.01 0 0 0.01 0 0]` over thousand-unit widths, so each
    /// glyph is ten ems wide — is measured and cut, and nothing is reported.
    ///
    /// At `10 Tf` from x 10 the glyphs of `SECRET` are a hundred points each:
    /// `S` x 10..110 and `E` x 110..210 meet a rectangle over the whole
    /// 200-point page, and the four after them are past its edge.
    #[test]
    fn a_rescaled_type3_font_is_measured_rather_than_refused() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            "[0.01 0 0 0.01 0 0]",
            "BT /F0 10 Tf 10 100 Td (SECRET) Tj ET",
        ));
        let (bytes, report) = redact(doc, &[band(area(0.0, 0.0, 200.0, 200.0))]);
        assert_eq!(report.glyphs, 2, "the two on the page");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(
            streams.contains("CRET") && !streams.contains("SE"),
            "{streams}"
        );
    }

    /// The conventional matrix is the same arithmetic as before any of this:
    /// every glyph one em, cut where the rectangle is.
    #[test]
    fn a_type3_font_with_the_conventional_matrix_is_cut() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 10 100 Td (SECRET) Tj ET",
        ));
        let (_, report) = redact(doc, &[band(area(0.0, 0.0, 200.0, 200.0))]);
        assert_eq!(report.glyphs, 6);
        assert!(report.warnings.is_empty());
    }

    /// A `/FontMatrix` this engine cannot read — here three numbers — is read
    /// as the default, which is where the renderer places the run too: its
    /// Type 3 path needs six numbers, and without them it advances each code
    /// by `w0 / 1000` like any other font.
    ///
    /// Read that way `SECRET` is x 70..130 at `10 Tf`, and the band over it
    /// takes exactly those six.
    #[test]
    fn a_font_matrix_that_cannot_be_read_is_measured_as_the_default() {
        for matrix in ["[0.01 0 0]", "(not an array)"] {
            let doc = open(boxed_glyph_document(
                200.0,
                200.0,
                matrix,
                "BT /F0 10 Tf 10 100 Td (PUBLICSECRET) Tj ET",
            ));
            let (bytes, report) = redact(doc, &[band(area(72.0, 95.0, 125.0, 115.0))]);
            assert_eq!(report.glyphs, 6, "{matrix}");
            let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
            assert!(
                streams.contains("PUBLIC") && !streams.contains("SECRET"),
                "{matrix}"
            );
        }
    }

    /// A `/FontBBox` that reaches below the baseline carries the box down
    /// with it: these glyphs are drawn from half an em below the baseline to
    /// one em above it, and a band under the baseline alone covers their
    /// descenders.
    ///
    /// `S` is x 70..80 at `10 Tf`; its descender is y 95..100. The band
    /// x 72..78, y 96..99 is inside that and nowhere else. An em box from the
    /// baseline up misses it and leaves the ink.
    #[test]
    fn a_bounding_box_below_the_baseline_carries_the_glyph_box_down() {
        let bytes = type3_document(
            200.0,
            DEFAULT_FONT_MATRIX,
            "/FontBBox [0 -500 1000 1000]",
            1000,
            "1000 0 d0 0 -500 1000 1500 re f",
            "BT /F0 10 Tf 10 100 Td (PUBLICSECRET) Tj ET",
        );
        let under = area(72.0, 96.0, 78.0, 99.0);
        assert!(ink_in(&render(bytes.clone()), 200.0, under) > 0);

        let (after, report) = redact(open(bytes), &[band(under)]);
        assert_eq!(report.glyphs, 1, "S, by its descender");
        assert_eq!(ink_in(&render(after), 200.0, under), 0);
    }

    /// A `/FontBBox` written too small does not shrink the box under the
    /// ink: the box is joined with one em, because a bounding box that
    /// understates the glyphs would leave exactly the ink a rectangle
    /// covered.
    ///
    /// These glyphs fill the whole em square and the font claims a tenth of
    /// it. The band x 72..78, y 105..109 is over the top half of `S`.
    #[test]
    fn a_bounding_box_written_too_small_does_not_shrink_the_glyph_box() {
        let bytes = type3_document(
            200.0,
            DEFAULT_FONT_MATRIX,
            "/FontBBox [0 0 1000 100]",
            1000,
            "1000 0 d0 0 0 1000 1000 re f",
            "BT /F0 10 Tf 10 100 Td (PUBLICSECRET) Tj ET",
        );
        let top = area(72.0, 105.0, 78.0, 109.0);
        assert!(ink_in(&render(bytes.clone()), 200.0, top) > 0);

        let (after, report) = redact(open(bytes), &[band(top)]);
        assert_eq!(report.glyphs, 1, "S");
        assert_eq!(ink_in(&render(after), 200.0, top), 0);
    }
}

/// A cut run is re-emitted as `TJ`, and what the original operator did
/// *besides* showing has to survive that.
///
/// Every failure here is the same one, and it is the failure the rest of this
/// module exists to refuse: a redaction that removed the right glyphs and
/// moved the wrong ones. The needle-bytes-absent check cannot see it, because
/// the bytes that had to go are gone; what catches it is where the rest of the
/// run lands, which each of these asserts twice — once in the rewritten
/// stream and once in the ink.
#[cfg(test)]
mod showing_operators {
    use super::tests_support::*;
    use super::*;

    /// `'` is `T*` then `Tj` (9.4.3). The line advance has to survive the cut.
    #[test]
    fn a_cut_quote_keeps_its_line_advance() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 20 TL 20 150 Td (PUBLIC) Tj (SECRET) ' ET",
        ));

        // Glyph zero of the second line: page x 20..30, y 130..140. The
        // first line's baseline is twenty points above it.
        let first_glyph = Redaction {
            area: Rect {
                x0: 18.0,
                y0: 131.0,
                x1: 29.0,
                y1: 139.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[first_glyph]);
        assert_eq!(report.glyphs, 1, "the S of the second line alone");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "the glyph went: {streams}");
        assert!(
            streams.contains("T*"),
            "and the line advance stayed: {streams}"
        );

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, first_glyph.area),
            0,
            "no ink where the cut glyph was"
        );
        // The E that followed it, still on the second line rather than
        // wherever the first line's pen had got to.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 31.0,
                    y0: 131.0,
                    x1: 39.0,
                    y1: 139.0,
                }
            ) > 20,
            "the rest of the run is still on its own line"
        );
    }

    /// `aw ac string "` is `aw Tw ac Tc` then `'` (9.4.3). Both spacings have
    /// to survive the cut, and the character spacing is visible in the
    /// geometry: at `5 Tc` the run's glyphs stand fifteen points apart rather
    /// than ten, so the last of six sits twenty points further along.
    #[test]
    fn a_cut_double_quote_keeps_both_spacings() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 20 TL 20 150 Td (PUBLIC) Tj 1 5 (SECRET) \" ET",
        ));

        let first_glyph = Redaction {
            area: Rect {
                x0: 18.0,
                y0: 131.0,
                x1: 29.0,
                y1: 139.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[first_glyph]);
        assert_eq!(report.glyphs, 1);

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(
            streams.contains("1 Tw 5 Tc"),
            "both spacings were re-emitted: {streams}"
        );

        // Glyph five of the second line: page x 95..105 at `5 Tc`, and
        // x 75..85 without it.
        assert!(
            ink_in(
                &render(bytes),
                200.0,
                Rect {
                    x0: 97.0,
                    y0: 131.0,
                    x1: 103.0,
                    y1: 139.0,
                }
            ) > 20,
            "the character spacing placed the tail of the run"
        );
    }

    /// An adjustment the original `TJ` array already carried is re-emitted
    /// with its own sign and its own magnitude.
    ///
    /// 9.4.3 makes a `TJ` number move the pen *backwards* by its value in
    /// thousandths, so `-2000` is twenty points forward at ten point. A
    /// rewrite that re-emits it as `2000` moves twenty points back instead,
    /// and every glyph after it lands forty points from where it was — which
    /// is a redaction that cut the right glyph and relocated the rest of the
    /// line.
    #[test]
    fn an_existing_tj_adjustment_keeps_its_sign_and_size() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 20 100 Td [(PUB) -2000 (SECRET)] TJ ET",
        ));

        // `PUB` is page x 20..50; the adjustment carries the pen to x 70, so
        // `SECRET` is x 70..130 and its S is x 70..80.
        let first_glyph = Redaction {
            area: Rect {
                x0: 68.0,
                y0: 101.0,
                x1: 79.0,
                y1: 109.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[first_glyph]);
        assert_eq!(report.glyphs, 1, "the S alone");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(
            streams.contains("-2000"),
            "the original adjustment came back as it went in: {streams}"
        );

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, first_glyph.area),
            0,
            "no ink where the cut glyph was"
        );
        // The T that ends the run, still at page x 120..130.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 122.0,
                    y0: 102.0,
                    x1: 128.0,
                    y1: 108.0,
                }
            ) > 20,
            "the tail of the run did not move"
        );
    }
}

/// Fixtures whose glyphs are boxes, so that measured geometry and drawn ink
/// are the same rectangle.
///
/// A Type 3 font is what makes the ink assertions possible without a font
/// file: its glyphs are content streams, so `1000 0 d0 0 0 1000 1000 re f`
/// fills the whole em square and a ten-point glyph is exactly a ten-point
/// black box. With `/FontMatrix [0.001 0 0 0.001 0 0]` — 9.6.5's conventional
/// glyph space, and the one every other font kind has implicitly — a
/// `/Widths` entry of 1000 is one em, which is the same arithmetic redaction
/// does for a Type 1 or TrueType face. So the fixture is not a special case
/// of the code under test; it is the ordinary case with visible geometry.
///
/// Redaction rewrites the *page's* showing operators, so the glyph procedures
/// are never entered for a code that was removed. Text drawn **inside** a
/// glyph procedure is a different question and is still refused — see
/// `docs/features/editing.md`.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// 9.6.5's conventional Type 3 glyph space, and the one every other font
    /// kind has implicitly.
    pub const DEFAULT_FONT_MATRIX: &str = "[0.001 0 0 0.001 0 0]";

    pub fn open(bytes: Vec<u8>) -> Arc<CosDocument> {
        Arc::new(CosDocument::open(bytes).expect("it opens"))
    }

    pub fn all_streams(doc: &CosDocument) -> String {
        let mut out = Vec::new();
        for (number, _) in doc.xref().iter() {
            if let Ok(bytes) = doc.stream_decoded(ObjRef::new(number, 0)) {
                out.extend_from_slice(&bytes);
                out.push(b'\n');
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    pub fn redact(doc: Arc<CosDocument>, areas: &[Redaction]) -> (Vec<u8>, RedactionReport) {
        let mut editor = DocumentEditor::new(doc);
        let report = apply(&mut editor, 0, areas).expect("the page exists");
        let bytes = editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            ..tinker_pdf_cos::WriteOptions::default()
        });
        (bytes, report)
    }

    pub fn render(bytes: Vec<u8>) -> crate::Bitmap {
        crate::Document::open(bytes)
            .expect("it reopens")
            .page(0)
            .expect("a page")
            .render(&crate::RenderOptions::default())
    }

    /// How many inked pixels fall inside a page-space rectangle.
    ///
    /// The render is one pixel per point with the page origin at the bottom
    /// left, so device rows count down from the page height. A pixel is
    /// counted by its centre, which keeps a rectangle's own edge from
    /// deciding the answer.
    pub fn ink_in(bitmap: &crate::Bitmap, page_height: f64, area: Rect) -> usize {
        let mut inked = 0;
        for y in 0..bitmap.height {
            let py = page_height - (f64::from(y) + 0.5);
            if py < area.y0 || py > area.y1 {
                continue;
            }
            for x in 0..bitmap.width {
                let px = f64::from(x) + 0.5;
                if px < area.x0 || px > area.x1 {
                    continue;
                }
                let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
                if bitmap.data.get(at).copied().unwrap_or(255) < 200 {
                    inked += 1;
                }
            }
        }
        inked
    }

    /// The `/Differences` and `/Widths` for codes 65..=90, every one of them
    /// the same box-filling procedure one em wide.
    fn every_letter() -> (String, String) {
        (
            format!("[65 {}]", ["/g"; 26].join(" ")),
            ["1000"; 26].join(" "),
        )
    }

    /// The boxed Type 3 font as object `number`, drawing every letter with
    /// the procedure in object 5 (which the caller writes, with
    /// [`stream_object`] and [`BOX_PROCEDURE`]).
    pub fn boxed_font(number: u32, font_matrix: &str) -> String {
        let (differences, widths) = every_letter();
        format!(
            "{number} 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
             /FontMatrix {font_matrix}\n\
             /CharProcs << /g 5 0 R >>\n\
             /Encoding << /Type /Encoding /Differences {differences} >>\n\
             /FirstChar 65 /LastChar 90 /Widths [{widths}]\n\
             /Resources << >> >>\nendobj\n"
        )
    }

    /// The glyph procedure every letter of [`boxed_font`] draws: its whole em
    /// square, filled.
    pub const BOX_PROCEDURE: &str = "1000 0 d0 0 0 1000 1000 re f";

    pub fn stream_object(number: u32, body: &str) -> String {
        format!(
            "{number} 0 obj\n<< /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
            body.len() + 1
        )
    }

    /// How many pixels differ between two renders of one page, counting only
    /// those whose centre lies **outside** a page-space rectangle.
    ///
    /// What says a cut was exact: everything a redaction did not remove
    /// renders as it did before, bit for bit, so every pixel outside the
    /// removed glyphs' own boxes is unchanged.
    pub fn differing_outside(
        before: &crate::Bitmap,
        after: &crate::Bitmap,
        page_height: f64,
        area: Rect,
    ) -> usize {
        assert_eq!(
            (before.width, before.height),
            (after.width, after.height),
            "the same page"
        );
        let mut differ = 0;
        for y in 0..before.height {
            let py = page_height - (f64::from(y) + 0.5);
            for x in 0..before.width {
                let px = f64::from(x) + 0.5;
                if (area.x0..=area.x1).contains(&px) && (area.y0..=area.y1).contains(&py) {
                    continue;
                }
                let at = (y as usize) * before.stride + (x as usize) * before.components();
                if before.data.get(at) != after.data.get(at) {
                    differ += 1;
                }
            }
        }
        differ
    }

    /// A one-page document, `width` by 200 points, whose one font `/F0` is a
    /// Type 3 face in the glyph space `font_matrix` declares.
    ///
    /// Codes 65..=90 are each `em` wide in that glyph space and drawn by
    /// `procedure` (object 5); `font_bbox` is written as given, so a test can
    /// make it honest, too small or absent.
    pub fn type3_document(
        width: f64,
        font_matrix: &str,
        font_bbox: &str,
        em: u32,
        procedure: &str,
        content: &str,
    ) -> Vec<u8> {
        let widths = vec![em.to_string(); 26].join(" ");
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(&format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} 200]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n"
        ));
        out.push_str(&format!(
            "4 0 obj\n<< /Type /Font /Subtype /Type3 {font_bbox}\n\
             /FontMatrix {font_matrix}\n\
             /CharProcs << /g 5 0 R >>\n\
             /Encoding << /Type /Encoding /Differences [65 {}] >>\n\
             /FirstChar 65 /LastChar 90 /Widths [{widths}]\n\
             /Resources << >> >>\nendobj\n",
            ["/g"; 26].join(" ")
        ));
        out.push_str(&stream_object(5, procedure));
        out.push_str(&stream_object(7, content));
        out.push_str("trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// A one-page document with one Type 3 font whose every glyph fills its em
    /// square, and `content` as the page's content stream.
    pub fn boxed_glyph_document(
        width: f64,
        height: f64,
        font_matrix: &str,
        content: &str,
    ) -> Vec<u8> {
        let (differences, widths) = every_letter();
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(&format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n"
        ));
        out.push_str(&format!(
            "4 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
             /FontMatrix {font_matrix}\n\
             /CharProcs << /g 5 0 R >>\n\
             /Encoding << /Type /Encoding /Differences {differences} >>\n\
             /FirstChar 65 /LastChar 90 /Widths [{widths}]\n\
             /Resources << >> >>\nendobj\n"
        ));
        out.push_str(&stream_object(5, "1000 0 d0 0 0 1000 1000 re f"));
        out.push_str(&stream_object(7, content));
        out.push_str("trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// The same page with a second font, `/F1`, whose glyph space is ten
    /// times the conventional one (`/FontMatrix [0.01 0 0 0.01 0 0]` over the
    /// same thousand-unit widths and procedure), so each of its glyphs is ten
    /// ems wide and which font is in force shows in where a cut lands.
    pub fn two_font_document(content: &str) -> Vec<u8> {
        let (differences, widths) = every_letter();
        let font = |number: u32, matrix: &str| {
            format!(
                "{number} 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
                 /FontMatrix {matrix}\n\
                 /CharProcs << /g 5 0 R >>\n\
                 /Encoding << /Type /Encoding /Differences {differences} >>\n\
                 /FirstChar 65 /LastChar 90 /Widths [{widths}]\n\
                 /Resources << >> >>\nendobj\n"
            )
        };

        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
             /Resources << /Font << /F0 4 0 R /F1 8 0 R >> >> /Contents 7 0 R >>\nendobj\n",
        );
        out.push_str(&font(4, DEFAULT_FONT_MATRIX));
        out.push_str(&stream_object(5, "1000 0 d0 0 0 1000 1000 re f"));
        out.push_str(&stream_object(7, content));
        out.push_str(&font(8, "[0.01 0 0 0.01 0 0]"));
        out.push_str("trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// A one-page document drawing `/Fm0`, a form XObject whose content is
    /// `form` **compressed** (`/FlateDecode`). Two fonts are in its scope:
    /// `/F0`, a Type 3 font whose every glyph fills its em square, for the
    /// ink, and `/F1`, Helvetica, for extraction — this engine's extractor
    /// reports no glyph for a Type 3 font, whose procedure it runs instead.
    pub fn compressed_form_document(form: &str) -> Vec<u8> {
        let letters: Vec<String> = (b'A'..=b'Z').map(|c| format!("/{}", c as char)).collect();
        let procs: Vec<String> = (b'A'..=b'Z')
            .map(|c| format!("/{} 5 0 R", c as char))
            .collect();
        let packed = tinker_pdf_filters::zlib_compress(form.as_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n");
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
              /Resources << /XObject << /Fm0 8 0 R >> >> /Contents 7 0 R >>\nendobj\n",
        );
        out.extend_from_slice(
            format!(
                // `/BaseFont` is not a Type 3 entry (Table 112); it is here
                // because the strict validator asks every simple font for
                // one, and this fixture is held to that validator.
                "4 0 obj\n<< /Type /Font /Subtype /Type3 /BaseFont /Boxed\n\
                 /FontBBox [0 0 1000 1000]\n\
                 /FontMatrix {DEFAULT_FONT_MATRIX}\n/CharProcs << {} >>\n\
                 /Encoding << /Type /Encoding /Differences [65 {}] >>\n\
                 /FirstChar 65 /LastChar 90 /Widths [{}]\n/Resources << >> >>\nendobj\n",
                procs.join(" "),
                letters.join(" "),
                ["1000"; 26].join(" ")
            )
            .as_bytes(),
        );
        out.extend_from_slice(stream_object(5, "1000 0 d0 0 0 1000 1000 re f").as_bytes());
        out.extend_from_slice(
            b"6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        );
        out.extend_from_slice(stream_object(7, "q /Fm0 Do Q").as_bytes());
        out.extend_from_slice(
            format!(
                "8 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 200 200]\n\
                 /Resources << /Font << /F0 4 0 R /F1 6 0 R >> >>\n\
                 /Filter /FlateDecode /Length {} >>\nstream\n",
                packed.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&packed);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out.extend_from_slice(b"trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n");
        out
    }

    /// A one-page document with an `/Identity-V` font whose metrics are all
    /// written out, and a `/ToUnicode` so that extraction reads letters back.
    ///
    /// CIDs 65..=90 are the capital letters, each `/W` 1000. `/W2` gives each
    /// `w1 = -1000` and the position vector `(500, 880)` — except CID 73,
    /// `I`, whose `w1` is `-500`, so the column is not a lattice and a gap of
    /// the wrong length moves every glyph after it. At `10 Tf` a glyph's box
    /// is x `-5..5` about the pen and one advance tall below it.
    pub fn cid_vertical_document(content: &str) -> Vec<u8> {
        let to_unicode = "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
            /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
            /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
            1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
            1 beginbfrange\n<0041> <005A> <0041>\nendbfrange\n\
            endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend";
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n",
        );
        out.push_str(
            "4 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Column\n\
             /Encoding /Identity-V /DescendantFonts [5 0 R] /ToUnicode 6 0 R >>\nendobj\n",
        );
        out.push_str(
            "5 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Column\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
             /DW 1000 /W [65 90 1000]\n\
             /W2 [65 72 -1000 500 880 73 [-500 500 880] 74 90 -1000 500 880] >>\nendobj\n",
        );
        out.push_str(&stream_object(6, to_unicode));
        out.push_str(&stream_object(7, content));
        out.push_str("trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// Two-byte `Identity` codes for capital letters, as a hex string.
    pub fn cid_hex(text: &str) -> String {
        let mut out = String::from("<");
        for c in text.chars() {
            out.push_str(&format!("{:04X}", c as u32));
        }
        out.push('>');
        out
    }

    /// Each line the facade's extractor reads on page zero: the page y of its
    /// first character's baseline, rounded to a hundredth, and its text with
    /// the ends trimmed — bottom to top.
    ///
    /// Extraction reports PDF user space, y upward, so the y is the page's.
    pub fn lines_of(bytes: Vec<u8>) -> Vec<(f64, String)> {
        lines_on(bytes, 0)
    }

    /// [`lines_of`] for any page.
    pub fn lines_on(bytes: Vec<u8>, page: u32) -> Vec<(f64, String)> {
        let doc = crate::Document::open(bytes).expect("it reopens");
        let text = doc.page(page).expect("a page").text();
        let mut out: Vec<(f64, String)> = text
            .lines()
            .into_iter()
            .filter_map(|line| {
                let y = line.chars.first()?.origin.1;
                Some(((y * 100.0).round() / 100.0, line.text.trim().to_string()))
            })
            .filter(|(_, text)| !text.is_empty())
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// One form drawing `PUBLIC SECRET` in 12-point Liberation Serif from
    /// x 10, at page y 50 and again at page y 200.
    ///
    /// The face is embedded — the vendored third-party bytes the subsetting
    /// tests use — so the page both draws ink and extracts, and neither is
    /// this repository's arithmetic about its own fixture. Its widths are
    /// Times', so `PUBLIC` is x 10..52.67, the space 52.67..55.67 and `SECRET`
    /// 55.67..100.35.
    pub fn public_secret_twice() -> Vec<u8> {
        public_secret_drawn_by("q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q")
    }

    /// The same form, drawn by `page`.
    pub fn public_secret_drawn_by(page: &str) -> Vec<u8> {
        let mut builder = tinker_pdf_cos::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(
            b"F0",
            b"LiberationSerif",
            &crate::subset::tests_support::face()
        ));
        assert!(builder.add_form(
            b"Fm0",
            &tinker_pdf_cos::FormXObject {
                bbox: [0.0, 0.0, 400.0, 300.0],
                matrix: None,
                group: None,
                content: b"BT /F0 12 Tf 10 50 Td (PUBLIC SECRET) Tj ET",
            }
        ));
        builder.add_page(400.0, 300.0, |p| p.raw(page.as_bytes()));
        builder.finish()
    }

    /// How many form XObjects a document holds.
    pub fn forms_in(doc: &CosDocument) -> usize {
        doc.xref()
            .iter()
            .filter_map(|(number, _)| doc.get(ObjRef::new(number, 0)).ok())
            .filter(|object| {
                object
                    .as_dict()
                    .and_then(|d| d.get_name(doc.intern(b"Subtype")))
                    .and_then(|n| doc.name_bytes(n))
                    .as_deref()
                    == Some(b"Form".as_slice())
            })
            .count()
    }

    /// Page zero's content, decoded, as text.
    pub fn page_content(doc: &CosDocument) -> String {
        let pages = tinker_pdf_cos::pages::collect(doc);
        let page = pages.first().expect("a page");
        String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(doc, page)).into_owned()
    }

    /// Every character the facade's extractor reports on page zero, with the
    /// origin it placed it at.
    pub fn extracted(bytes: Vec<u8>) -> Vec<(String, (f64, f64))> {
        let doc = crate::Document::open(bytes).expect("it reopens");
        let text = doc.page(0).expect("a page").text();
        text.lines()
            .into_iter()
            .flat_map(|line| line.chars.iter())
            .map(|c| (c.text.clone(), c.origin))
            .collect()
    }

    /// A one-page document whose font writes vertically (`/Identity-V`).
    ///
    /// A composite font, because writing mode is a property of the encoding
    /// CMap and only a composite font has one. `/DW` supplies the horizontal
    /// widths, which is exactly the trap: they are present and plausible, so
    /// a redaction that does not ask about the writing mode measures the run
    /// along the wrong axis and finds an answer.
    pub fn vertical_document(content: &str) -> Vec<u8> {
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n",
        );
        out.push_str(
            "4 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Boxed\n\
             /Encoding /Identity-V /DescendantFonts [5 0 R] >>\nendobj\n",
        );
        out.push_str(
            "5 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Boxed\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
             /DW 1000 >>\nendobj\n",
        );
        out.push_str(&stream_object(7, content));
        out.push_str("trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }
}

/// A redaction reads the page the **editor** has, not the page the file had.
///
/// Each of these failed before [`EditorPage`], and each failure was an
/// under-redaction with a report that looked like success.
#[cfg(test)]
mod editor_reads {
    use super::tests_support::*;
    use super::*;

    fn saved(editor: &DocumentEditor) -> Vec<u8> {
        editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            ..tinker_pdf_cos::WriteOptions::default()
        })
    }

    fn area(x0: f64, y0: f64, x1: f64, y1: f64) -> Redaction {
        Redaction {
            area: Rect { x0, y0, x1, y1 },
            mark: false,
        }
    }

    /// Two redactions of one page, one after the other, both hold.
    ///
    /// The second used to read the file's content, cut its own rectangle out
    /// of that and overwrite the stream the first had written — so `SECRET`,
    /// which the first redaction removed and reported removed, was back.
    #[test]
    fn a_second_redaction_of_a_page_keeps_the_first() {
        // `PUBLICSECRET` in ten-point boxes from x 20: `PUB` is x 20..50 and
        // `SECRET` x 80..140, on y 100..110.
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 20 100 Td (PUBLICSECRET) Tj ET",
        ));
        let secret = area(82.0, 95.0, 150.0, 115.0);
        let pub_ = area(15.0, 95.0, 48.0, 115.0);

        let mut editor = DocumentEditor::new(doc);
        let first = apply(&mut editor, 0, &[secret]).expect("the page exists");
        assert_eq!(first.glyphs, 6);
        let second = apply(&mut editor, 0, &[pub_]).expect("the page exists");
        assert_eq!(second.glyphs, 3, "P, U and B");

        let bytes = saved(&editor);
        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(
            !streams.contains("SECRET") && !streams.contains("SECR"),
            "the first redaction survived the second: {streams}"
        );
        assert!(
            !streams.contains("PUB"),
            "and the second happened: {streams}"
        );
        assert!(streams.contains("LIC"), "and the rest is there: {streams}");

        let bitmap = render(bytes);
        assert_eq!(ink_in(&bitmap, 200.0, secret.area), 0);
        assert_eq!(ink_in(&bitmap, 200.0, pub_.area), 0);
    }

    /// Two pages of boxed text, `AAAA` on the first and `BBBB` on the second.
    fn two_pages() -> Vec<u8> {
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 2 /Kids [3 0 R 6 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n",
        );
        out.push_str(&boxed_font(4, DEFAULT_FONT_MATRIX));
        out.push_str(&stream_object(5, BOX_PROCEDURE));
        out.push_str(
            "6 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 8 0 R >>\nendobj\n",
        );
        out.push_str(&stream_object(7, "BT /F0 10 Tf 20 100 Td (AAAA) Tj ET"));
        out.push_str(&stream_object(8, "BT /F0 10 Tf 20 100 Td (BBBB) Tj ET"));
        out.push_str("trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// A page the editor has moved is redacted where the editor has it.
    ///
    /// The file's page zero is not the editor's page zero after `move_page`.
    /// Reading the file, the redaction of the editor's page zero cut the
    /// *file's* page zero and wrote the result over that page's stream, then
    /// pointed the page it was asked about at it: `BBBB` stayed in the file,
    /// unreferenced and readable, and `AAAA` was gone from a page nobody
    /// redacted.
    #[test]
    fn a_moved_page_is_redacted_where_the_editor_has_it() {
        let mut editor = DocumentEditor::new(open(two_pages()));
        assert!(editor.move_page(1, 0), "BBBB is now first");

        let report =
            apply(&mut editor, 0, &[area(0.0, 0.0, 200.0, 200.0)]).expect("the page exists");
        assert_eq!(report.glyphs, 4, "the four Bs");

        let bytes = saved(&editor);
        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("BBBB"), "got: {streams}");
        assert!(
            streams.contains("AAAA"),
            "the other page is untouched: {streams}"
        );

        let reopened = crate::Document::open(bytes).expect("it reopens");
        let second = reopened
            .page(1)
            .expect("two pages")
            .render(&crate::RenderOptions::default());
        assert!(
            ink_in(&second, 200.0, area(22.0, 102.0, 58.0, 108.0).area) > 100,
            "and still draws its text"
        );
    }

    /// A page whose `/Resources` live on the page tree node above it
    /// (7.7.3.4) has its forms and images followed and its fonts measured.
    ///
    /// Read from the page dictionary alone, the resources were empty: the run
    /// was refused as `UnknownFont`, which at least said so, and the image was
    /// **not followed at all**, which said nothing.
    #[test]
    fn inherited_resources_are_read() {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n");
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.extend_from_slice(
            b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R]\n\
              /Resources << /Font << /F0 4 0 R >> /XObject << /Im0 6 0 R >> >> >>\nendobj\n",
        );
        out.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
              /Contents 7 0 R >>\nendobj\n",
        );
        out.extend_from_slice(boxed_font(4, DEFAULT_FONT_MATRIX).as_bytes());
        out.extend_from_slice(stream_object(5, BOX_PROCEDURE).as_bytes());
        let samples = b"SECRETPIXELDATA!";
        out.extend_from_slice(
            format!(
                "6 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4\n\
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                samples.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(samples);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out.extend_from_slice(
            stream_object(
                7,
                "q 40 0 0 40 120 20 cm /Im0 Do Q BT /F0 10 Tf 20 100 Td (SECRET) Tj ET",
            )
            .as_bytes(),
        );
        out.extend_from_slice(b"trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");

        let doc = open(out);
        assert!(all_streams(&doc).contains("SECRETPIXEL"));
        let mut editor = DocumentEditor::new(doc);
        let report =
            apply(&mut editor, 0, &[area(0.0, 0.0, 200.0, 200.0)]).expect("the page exists");
        assert_eq!(report.images, 1, "the inherited /Im0 was followed");
        assert_eq!(report.glyphs, 6, "and the inherited /F0 measured");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let streams = all_streams(&CosDocument::open(saved(&editor)).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
    }
}

#[cfg(test)]
mod page_scope_tests {
    use super::*;
    use std::sync::Arc;
    use tinker_pdf_cos::CosDocument;

    /// Two pages, each with an image under the same resource name `/Im0`,
    /// and a redaction covering the whole of page one.
    fn two_pages() -> Vec<u8> {
        let content = "q 100 0 0 100 0 0 cm /Im0 Do Q";
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 2 /Kids [3 0 R 6 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {len} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
stream\nPAGE\nendstream\nendobj\n\
6 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << /XObject << /Im0 8 0 R >> >> /Contents 7 0 R >>\nendobj\n\
7 0 obj\n<< /Length {len} >>\nstream\n{content}\nendstream\nendobj\n\
8 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
stream\nSECR\nendstream\nendobj\n\
trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
            len = content.len()
        )
        .into_bytes()
    }

    /// Redacting page one must remove page one's image.
    ///
    /// This failed in the worst possible way: resource names were resolved
    /// against page *zero*, so redacting page one scrubbed page zero's `/Im0`
    /// — a name so common that the collision is the normal case — reported
    /// `images: 1`, and left the secret on page one untouched. It looked like
    /// it had worked.
    #[test]
    fn redacting_the_second_page_removes_the_second_pages_image() {
        let doc = Arc::new(CosDocument::open(two_pages()).expect("it opens"));
        let mut editor = DocumentEditor::new(doc);

        let report = apply(
            &mut editor,
            1,
            &[Redaction {
                area: tinker_pdf_cos::Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 100.0,
                    y1: 100.0,
                },
                mark: false,
            }],
        )
        .expect("it redacts");
        assert_eq!(report.images, 1, "it found page one's image");

        let saved = editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            compress: false,
            object_streams: false,
            ..tinker_pdf_cos::WriteOptions::default()
        });
        let text = String::from_utf8_lossy(&saved);

        assert!(
            !text.contains("SECR"),
            "page one's samples are gone from the file"
        );
        assert!(
            text.contains("PAGE"),
            "and page zero's image, which nobody asked about, is untouched"
        );
    }

    /// The mirror: redacting page zero must not reach page one.
    #[test]
    fn redacting_the_first_page_leaves_the_second_alone() {
        let doc = Arc::new(CosDocument::open(two_pages()).expect("it opens"));
        let mut editor = DocumentEditor::new(doc);

        apply(
            &mut editor,
            0,
            &[Redaction {
                area: tinker_pdf_cos::Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 100.0,
                    y1: 100.0,
                },
                mark: false,
            }],
        )
        .expect("it redacts");

        let saved = editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            compress: false,
            object_streams: false,
            ..tinker_pdf_cos::WriteOptions::default()
        });
        let text = String::from_utf8_lossy(&saved);

        assert!(!text.contains("PAGE"), "page zero's samples are gone");
        assert!(text.contains("SECR"), "page one is untouched");
    }
}
