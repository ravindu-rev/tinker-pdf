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
//! Nor a **Type 3 glyph's procedure**. A use of a glyph whose procedure
//! draws under a rectangle is removed, and the procedure — which every use
//! of that glyph runs — is left in `/CharProcs` exactly as it was (the
//! module's "A Type 3 glyph's procedure"). A procedure that shows the
//! covered words as text says them in the file as plainly as an outline
//! does. The same door is the answer: [`crate::subset::apply`] empties every
//! procedure nothing the document shows still runs (its "Type 3 fonts"), so
//! once the redaction removed a procedure's last use the default save
//! leaves it saying nothing, and a Type 3 font it cannot bound is named
//! (`a_procedure_whose_last_use_was_redacted_is_emptied_by_the_default_save`).
//! Nor an annotation's own text — `/Contents`, a
//! rich-text `/RC`, a field's `/V`: what a redaction cuts is what a page
//! draws, and those are what a viewer *says*, with no position to compare
//! with a rectangle.
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
//! One more thing is named for the same reason, though it is not a run:
//! a stream that invokes more XObjects than one stream's walk follows
//! ([`MAX_XOBJECT_USES`], 4 096). The `Do`s past the bound are written back
//! as they were and never resolved, so an image they draw is tested against
//! no rectangle — [`RedactionWarning::TooManyXObjects`] says how many. Until
//! October 2026 the bound was a bare number in [`rewrite`] and what lay past
//! it was left with nothing in the report.
//!
//! And two kinds of content a page draws that this module does not read at
//! all: a **tiling pattern's cell** (8.7.3.1), painted at every tile of
//! whatever it fills, and a **soft mask's group** (11.6.5.2), drawn as the
//! alpha of what lies under it. Cutting a cell is a form drawn at as many
//! placements as its fill has tiles, which is a design rather than a fix;
//! so a cell or a group that shows text or draws an image is named instead,
//! by the resource name the `scn` or the `gs` gave
//! ([`RedactionWarning::PatternOrMask`], [`unread`]), and one that only
//! paints paths is not, because nothing in it is anything this module
//! removes. Until October 2026 neither was read or named.
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
//! draws the form draws it as it was. When every placement cut something,
//! the answer depends on whether anything else **draws** the form — another
//! page, a form or an annotation there, a Type 3 glyph's procedure anywhere
//! ([`Elsewhere`], read once and only when this case arises): if something
//! does, the object is left as it was for that drawer and every placement
//! here draws a copy; if nothing does, it takes the first placement's
//! outcome, so it is still drawn by this page and is never left in the file
//! holding what a rectangle covered with nothing drawing it. *Drawn*, not
//! *named*: one `/Resources` dictionary shared by every page names every form
//! on every page, and counting that would leave exactly that unreferenced
//! uncut stream. Until October 2026 the first placement's outcome was taken
//! whatever else drew the form, and a page that shared it lost what this
//! page's rectangles covered, unreported. [`crate::subset`] walks the editor's
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
//! An **inline** image (8.9.7) is scrubbed the same way where it stands in
//! the stream, and one no rectangle covers is written back byte for byte:
//! its samples are not tokens, and until September 2026 the rewrite
//! tokenized them anyway and wrote back whatever tokens they spelled — every
//! inline image on a redacted page corrupted, none of them ever scrubbed.
//!
//! # Annotation appearances
//!
//! An annotation is drawn by running its appearance stream (12.5.5), a form
//! XObject placed over the page by 12.5.5's matrix **A**: the form's `/BBox`
//! carried through its `/Matrix`, and the box that results scaled and moved
//! onto `/Rect`. Until October 2026 this module cut the page's content and
//! the forms it drew and nothing else, so the text of a FreeText note, a
//! stamp or a filled field under a rectangle stayed exactly where it was,
//! drawn on the page, and the report did not mention it.
//!
//! Every appearance an annotation on the page can show is now a placement of
//! its form, entered by [`Walk`] at the transform the renderer draws it with
//! ([`appearance_fit`], which is `annots::fit`'s arithmetic): each of `/N`,
//! `/R` and `/D`, every state of each — not only the one `/AS` selects,
//! since a viewer switches states with no edit to the file — and the
//! appearance of an annotation flagged hidden, which is one bit from being
//! drawn. So an appearance is cut as any form is ("A form drawn twice"): in
//! place when one annotation draws it, and through a copy when annotations
//! that share it are covered differently. The covered annotation is pointed
//! at its copy through an `/AP` of its own ([`repoint_appearances`]),
//! because the `/AP` dictionary — or the state dictionary under it — may be
//! an object the others share and none of them draws the copy. An
//! annotation written into `/Annots` itself is edited where it sits. An
//! appearance with no `/Resources` names things in the page's, as the
//! subsetter reads it (8.10.1).
//!
//! The annotation is **rewritten, not removed**: what goes is what the
//! rectangle covers, and the rest of the note stays where it was, which is
//! what a redaction does to a page's own text. What it does not reach is the
//! annotation's own text — "What this module does not remove".
//!
//! # A Type 3 glyph's procedure
//!
//! 9.6.5: a Type 3 glyph is drawn by running its procedure, which can show
//! text in another font or draw an image anywhere — not only inside the box
//! a glyph is measured by. So a use of a glyph whose procedure can draw
//! either is measured through it, under the transform the interpreter runs
//! it with, and a use whose procedure draws under a rectangle is **removed
//! whole**, as a glyph partly under one is ([`cut_stream`]). The procedure
//! itself is not rewritten, and [`cut_stream`]'s doc is the decision: it is
//! the font's, every use of the glyph on every page runs it, and cutting the
//! covered text out of it would cut it out of all of them — the over-removal
//! a form drawn twice used to cost, with no copy to give the uses that were
//! not covered short of a new glyph in the font. A procedure that shows a
//! glyph whose procedure shows a glyph is followed down, a fixed budget of
//! streams per use ([`draws_under`]), past which the answer is *covered*.
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
//!
//! The appearance and glyph-procedure defects were counted on 2 October
//! 2026, over `cargo test --no-fail-fast -p tinker-pdf --lib`, 339 tests.
//! None reports zero, and every count of one is the test written for it:
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | appearances not walked, which is how it used to be | 6 |
//! | an appearance placed in its own form space, the fit to `/Rect` ignored | 6 |
//! | the fit computed from `/BBox` without the form's `/Matrix` | **1** |
//! | only `/N` walked | **1** |
//! | only the first state of a state dictionary walked | **1** |
//! | the appearance of a hidden annotation skipped | **1** |
//! | the copy written into an `/AP` object two annotations share | **1** |
//! | annotations never pointed at their copies | 2 |
//! | an annotation written into `/Annots` itself never edited | **1** |
//! | an appearance with no `/Resources` measured in an empty scope | **1** |
//! | glyph procedures never measured, which is how it used to be | 6 |
//! | a procedure measured without `/FontMatrix` | 3 |
//! | a procedure measured at the start of its run rather than at its own pen | **1** |
//! | the second pass not removing the uses the first found covered | 5 |
//! | one budget for the stream rather than one per use | 2 |
//! | a spent budget answering *not covered* | **1** |
//! | an inline image a procedure draws not counted | **1** |
//! | a form a procedure draws not followed | **1** |
//! | a procedure that draws only an inline image not recognised as drawing anything | **1** |
//!
//! The read of what else draws a form ([`Elsewhere`]) was counted the same
//! day, over 344 tests. None reports zero:
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | a drawer elsewhere never asked about, which is how it used to be | 4 |
//! | the redacted page counted as a drawer elsewhere | 11 |
//! | a form a page's resources name counted as one it draws | **1** |
//! | a form drawn elsewhere not followed into | **1** |
//! | another page's annotations not read | **1** |
//! | a Type 3 face's procedures not read | **1** |
//! | a procedure's `Do` resolved only in its face's own `/Resources` | **1** |
//!
//! And [`MAX_FORM_DEPTH`]'s, over 346:
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the walk stopping at thirteen levels of forms, which is how it used to be | 2 |
//! | the walk's depth test as strict as the interpreter's | 2 |
//!
//! And [`MAX_XOBJECT_USES`]'s, over 356:
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the `Do`s past the cap dropped with nothing said, which is how it used to be | 2 |
//! | the cap one `Do` looser | 2 |
//! | the cap one `Do` tighter | 3 |
//! | the warning raised with no rectangle | **1** |
//! | two passes merged without summing | **1** |
//!
//! And the patterns and masks named, over 364:
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the page's own never read, which is how it used to be | 3 |
//! | a form's never read | **1** |
//! | a glyph procedure's never read | **1** |
//! | a stroking pattern (`SCN`) not recognised | **1** |
//! | a graphics state not read | 2 |
//! | every pattern and mask named, whatever it draws | 2 |
//! | named with no rectangle | **1** |
//! | a procedure that paints with a pattern not measured | **1** |
//! | a procedure that sets a state not measured | **1** |

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
/// Two of the five are a **run left whole** because this module could not
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
/// The fourth and fifth (October 2026) are content this module does not
/// read, rather than a run or a form: [`RedactionWarning::TooManyXObjects`],
/// a stream whose `Do`s ran past what one stream's walk follows, and
/// [`RedactionWarning::PatternOrMask`], a tiling pattern or a soft mask
/// whose content shows text or draws an image. Both were silent before they
/// existed.
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
    /// One content stream invoked more XObjects than this module follows in
    /// one stream (4 096, a private bound), and the `Do`s past it were **not
    /// followed**: an image they draw was tested against no rectangle, and a
    /// form was not entered.
    ///
    /// The cap bounds what one stream's walk holds (ruling 1). Until October
    /// 2026 it was a bare `4096` in the rewrite, and what lay past it was left
    /// with nothing in the report — an image under a rectangle as the
    /// 4 097th `Do` stayed in the file and the report read `images: 0`.
    /// Raised only when there is a rectangle, as every warning here is.
    TooManyXObjects {
        /// How many `Do`s went unfollowed, summed over every pass that
        /// reached the cap — a form's stream counted once at each of its
        /// placements, since each is a pass over it.
        skipped: usize,
    },
    /// A stream painted with a **tiling pattern** (8.7.3.1) or set a **soft
    /// mask** (11.6.5.2) whose content shows text or draws an image, and
    /// this module reads neither: what the cell or the mask's group draws
    /// was tested against no rectangle.
    ///
    /// A cell is painted at every tile of whatever it fills, so cutting one
    /// is a form drawn at as many placements as the fill has tiles, and a
    /// mask's group is drawn as the alpha of what lies under it — glyph
    /// shapes and all. Neither is measured; both are named, by the resource
    /// name the `scn` or the `gs` gave. A cell or a group that only paints
    /// paths is not named, because nothing it draws is anything this module
    /// removes. Until October 2026 neither was named.
    PatternOrMask {
        /// The `/Pattern` or `/ExtGState` resource name.
        resource: Vec<u8>,
    },
}

impl RedactionWarning {
    /// The resource name of the font the run was showing in.
    ///
    /// Empty for [`RedactionWarning::RepeatedForm`], which is about a form
    /// XObject and not about a font, and for
    /// [`RedactionWarning::TooManyXObjects`], which is about a stream.
    /// [`RedactionWarning::resource`] is the accessor that answers for every
    /// variant.
    #[must_use]
    pub fn font(&self) -> &[u8] {
        match self {
            RedactionWarning::UnknownFont { font, .. }
            | RedactionWarning::UnmeasurableFrame { font, .. } => font,
            RedactionWarning::RepeatedForm { .. }
            | RedactionWarning::TooManyXObjects { .. }
            | RedactionWarning::PatternOrMask { .. } => &[],
        }
    }

    /// The resource name this warning is about — a font for the two run
    /// variants, a form XObject for [`RedactionWarning::RepeatedForm`], a
    /// pattern or a graphics state for [`RedactionWarning::PatternOrMask`],
    /// and nothing for [`RedactionWarning::TooManyXObjects`], whose `Do`s past
    /// the cap were never resolved to a resource at all.
    ///
    /// This is what distinguishes two warnings of the same kind, so it is
    /// never empty except where there is no name to quote.
    #[must_use]
    pub fn resource(&self) -> &[u8] {
        match self {
            RedactionWarning::RepeatedForm { form, .. } => form,
            RedactionWarning::PatternOrMask { resource } => resource,
            other => other.font(),
        }
    }

    /// How many bytes of showing operand this warning accounts for.
    ///
    /// Zero for [`RedactionWarning::RepeatedForm`], which leaves no operand
    /// in place — it counts placements instead
    /// ([`RedactionWarning::placements`]) — and for
    /// [`RedactionWarning::TooManyXObjects`], which counts `Do`s.
    #[must_use]
    pub fn bytes(&self) -> usize {
        match self {
            RedactionWarning::UnknownFont { bytes, .. }
            | RedactionWarning::UnmeasurableFrame { bytes, .. } => *bytes,
            RedactionWarning::RepeatedForm { .. }
            | RedactionWarning::TooManyXObjects { .. }
            | RedactionWarning::PatternOrMask { .. } => 0,
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
    /// classes, placements for a form, `Do`s for a stream past the cap —
    /// because a single `usize` that means bytes in one arm and placements in
    /// another is a number nobody can read.
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
            RedactionWarning::TooManyXObjects { skipped } => {
                if let RedactionWarning::TooManyXObjects { skipped: more } = other {
                    *skipped = skipped.saturating_add(*more);
                }
            }
            // A name, and nothing to count: one entry says the resource was
            // not read, however many times it was painted with.
            RedactionWarning::PatternOrMask { .. } => {}
        }
    }
}

/// How many `Do`s of one content stream a redaction follows.
///
/// What the walk holds per stream is a use per `Do` — a name, a transform and
/// where it was written — and a content stream may be as large as
/// `MAX_DECODED_STREAM`, so without a bound six bytes of `/a Do` a use would
/// buy a hundred (ruling 1). The `Do`s past it are written back as they were
/// and not followed, and [`RedactionWarning::TooManyXObjects`] says how many.
/// A page of more than four thousand XObject placements is a map or a tiled
/// scan, and one that wants them all measured has to be told it did not get
/// that, rather than left to believe it did.
const MAX_XOBJECT_USES: usize = 4096;

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
///
/// It is also how many streams one use of a Type 3 glyph may run while its
/// procedure is measured — the procedure, and every form and glyph procedure
/// below it, each a placement of a stream under a transform — past which the
/// use is removed as covered ([`draws_under`]).
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

    let mut measured = Vec::new();
    let (data, mut report, uses) = cut_stream(
        editor,
        &resources,
        &content,
        areas,
        &fonts,
        Matrix::IDENTITY,
        &mut measured,
    );
    for warning in measured {
        note(&mut report.warnings, warning);
    }
    unread(editor, &resources, &content, areas, &mut report.warnings);

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

    // 12.5.5: every appearance stream an annotation on the page can show —
    // each of `/N`, `/R` and `/D`, every state — is a form XObject the page
    // draws over itself, placed where the algorithm in 12.5.5 fits it onto
    // `/Rect`. Measured as a placement like any `Do`, so a stream two
    // annotations share is cut exactly at each, the covered one drawing a
    // copy. Hidden ones too: a flag is one bit a viewer or a caller can
    // clear, and printing ignores `NoView`.
    let appearances = appearances_on(editor, reference);
    let shown: Vec<Option<usize>> = appearances
        .iter()
        .map(|appearance| walk.appearance(editor, appearance, &resources, areas, &mut report))
        .collect();

    let targets = settle(editor, &walk, reference, areas, &mut report);
    let inline_annotations = repoint_appearances(editor, &appearances, &shown, &targets);

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
    // An annotation written into `/Annots` itself rather than as an object
    // is edited where it sits.
    if !inline_annotations.is_empty() {
        let key = editor.intern(b"Annots");
        match dict.get(key).cloned() {
            Some(Object::Array(mut items)) => {
                for (index, annotation) in inline_annotations {
                    if let Some(slot) = items.get_mut(index) {
                        *slot = Object::Dict(annotation);
                    }
                }
                dict.insert(key, Object::Array(items));
            }
            Some(Object::Ref(array)) => {
                if let Some(Object::Array(mut items)) = editor.get(array) {
                    for (index, annotation) in inline_annotations {
                        if let Some(slot) = items.get_mut(index) {
                            *slot = Object::Dict(annotation);
                        }
                    }
                    editor.put(array, Object::Array(items));
                }
            }
            _ => {}
        }
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
    /// A Type 3 font's glyph procedures that can draw text or an image, or
    /// paint with a pattern or a mask that might ([`carries`]), by code: the
    /// only things this module redacts, or names, that a procedure could put
    /// outside its glyph's box. A procedure that only paints paths is not
    /// here, because nothing it draws is anything a redaction removes.
    procedures: GlyphProcedures,
}

/// A Type 3 font's glyph procedures, decoded, by the code that shows each.
type GlyphProcedures = HashMap<u32, Arc<[u8]>>;

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
    let mut spaces = glyph_spaces(doc, resources);
    cos_font::from_resources(doc, resources)
        .into_iter()
        .filter_map(|(name, font)| {
            let bytes = doc.name_bytes(name)?.to_vec();
            let (glyph_space, procedures) = if font.kind() == cos_font::FontKind::Type3 {
                let (space, procedures) = spaces
                    .remove(&name)
                    .unwrap_or((GlyphSpace::DEFAULT, HashMap::new()));
                (Some(space), procedures)
            } else {
                (None, HashMap::new())
            };
            Some((
                bytes,
                Arc::new(RunFont {
                    font,
                    glyph_space,
                    procedures,
                }),
            ))
        })
        .collect()
}

/// The glyph space each font in `/Font` declares (9.6.5), and its glyph
/// procedures that can draw text or an image.
///
/// Read here rather than through `cos_font::Font`, which carries neither
/// `/FontMatrix`, `/FontBBox` nor `/CharProcs`: this module is the only
/// caller that builds a glyph box from them. Only a Type 3 font's answer is
/// ever used.
fn glyph_spaces(
    doc: &CosDocument,
    resources: &Dict,
) -> HashMap<Name, (GlyphSpace, GlyphProcedures)> {
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
        out.insert(
            *key,
            (GlyphSpace::read(doc, dict), carrying_procedures(doc, dict)),
        );
    }
    out
}

/// A Type 3 font's glyph procedures that show text or draw an image, by code.
///
/// A code reaches its procedure the way the interpreter reaches it
/// (`PageResources::type3_glyph`): `/Encoding`'s `/Differences` names it and
/// `/CharProcs` holds the stream under that name — there is no built-in
/// encoding for a font whose glyphs the document invented. At most 256
/// codes, one read each.
fn carrying_procedures(doc: &CosDocument, font: &Dict) -> GlyphProcedures {
    let mut out = HashMap::new();
    let subtype = font
        .get_name(doc.intern(b"Subtype"))
        .and_then(|n| doc.name_bytes(n));
    if subtype.as_deref() != Some(b"Type3".as_slice()) {
        return out;
    }
    let procs = doc.resolve_key(font, doc.intern(b"CharProcs"));
    let Some(procs) = procs.as_dict() else {
        return out;
    };
    let encoding = doc.resolve_key(font, doc.intern(b"Encoding"));
    let Some(encoding) = encoding.as_dict() else {
        return out;
    };
    let differences = doc.resolve_key(encoding, doc.intern(b"Differences"));
    let Some(differences) = differences.as_array() else {
        return out;
    };

    // Read exactly as the interpreter reads it, number for number: the first
    // name a code is given is the one it draws, and a real is truncated (a
    // negative or a NaN to zero, which is what `as` does).
    let mut named: HashSet<u32> = HashSet::new();
    let mut code = 0u32;
    for item in differences {
        match doc.resolve(item).as_ref() {
            Object::Int(v) => code = u32::try_from(*v).unwrap_or(0),
            Object::Real(v) => code = *v as u32,
            Object::Name(name) => {
                if code <= 0xff && named.insert(code) {
                    let content = procs
                        .get_ref(*name)
                        .and_then(|r| doc.stream_decoded(r).ok());
                    if let Some(content) = content.filter(|c| carries(c)) {
                        out.insert(code, Arc::from(content.as_slice()));
                    }
                }
                code = code.saturating_add(1);
            }
            _ => {}
        }
    }
    out
}

/// Whether content shows text, draws an image, or paints with a pattern or
/// a graphics state that might: an operator this module would have to
/// measure, or name ([`unread`]), were it in a page.
///
/// `scn` and `SCN` count only with a name last, which is a pattern
/// (8.6.6.2) — a colour's components are numbers, and a glyph that sets one
/// draws nothing more than its paths. `gs` always names a state, and the
/// state may set a mask.
fn carries(content: &[u8]) -> bool {
    let mut tokens = Tokenizer::new(content);
    let mut named = false;
    while let Some(token) = tokens.next_token() {
        match token {
            Token::Operator(op) => {
                match op.as_slice() {
                    b"Tj" | b"TJ" | b"'" | b"\"" | b"Do" | b"BI" | b"gs" => return true,
                    b"scn" | b"SCN" if named => return true,
                    _ => {}
                }
                named = false;
            }
            Token::Name(_) => named = true,
            _ => named = false,
        }
    }
    false
}

/// Names every tiling pattern and soft mask one stream paints with whose
/// content [`carries`] text or an image ([`RedactionWarning::PatternOrMask`]).
///
/// A pattern is selected by the name `scn` or `SCN` ends with (8.6.6.2), its
/// cell the pattern's own stream (8.7.3.1); a soft mask is the `/SMask` of
/// the `/ExtGState` a `gs` names,
/// its group the form in `/G` (11.6.5.2). Both resolve in `scope`, the
/// resources of the stream that painted, through the editor. Only when there
/// is a rectangle, as every warning here is; each name is resolved once.
fn unread(
    editor: &DocumentEditor,
    scope: &Dict,
    content: &[u8],
    areas: &[Redaction],
    warnings: &mut Vec<RedactionWarning>,
) {
    if areas.is_empty() {
        return;
    }
    let mut asked: HashSet<(bool, Vec<u8>)> = HashSet::new();
    let mut tokens = Tokenizer::new(content);
    let mut operands: Vec<Token> = Vec::new();
    while let Some(token) = tokens.next_token() {
        let Token::Operator(op) = &token else {
            operands.push(token);
            continue;
        };
        let pattern = match op.as_slice() {
            // 8.9.7: the samples are not tokens.
            b"BI" => {
                let consumed = tinker_pdf_content::interpret::skip_inline_image(tokens.rest());
                let at = tokens.position();
                tokens.seek(at.saturating_add(consumed));
                None
            }
            b"scn" | b"SCN" => Some(true),
            b"gs" => Some(false),
            _ => None,
        };
        if let (Some(pattern), Some(Token::Name(name))) = (pattern, operands.last()) {
            // Asked once each, while there are few enough names to remember
            // (ruling 1); past that a new name is asked every time, which
            // costs a lookup and not memory, and `note` merges what it finds.
            let key = (pattern, name.clone());
            let fresh = if asked.len() < MAX_XOBJECT_USES {
                asked.insert(key)
            } else {
                !asked.contains(&key)
            };
            if fresh {
                let drawn = if pattern {
                    tiling_cell(editor, scope, name)
                } else {
                    mask_group(editor, scope, name)
                };
                if drawn.is_some_and(|content| carries(&content)) {
                    note(
                        warnings,
                        RedactionWarning::PatternOrMask {
                            resource: name.clone(),
                        },
                    );
                }
            }
        }
        operands.clear();
    }
}

/// The cell of the pattern `name` selects in `scope`, decoded: a pattern's
/// stream, which only a tiling pattern has (8.7.3.1) — a shading pattern is
/// a dictionary, and answers `None`.
fn tiling_cell(editor: &DocumentEditor, scope: &Dict, name: &[u8]) -> Option<Vec<u8>> {
    let table = Resolve::resolve_key(editor, scope, editor.intern(b"Pattern"));
    let reference = table.as_dict()?.get_ref(editor.intern(name))?;
    editor.stream_bytes(reference)
}

/// The group of the soft mask the graphics state `name` sets in `scope`,
/// decoded — `None` for `/SMask /None` and for a state that sets no mask.
fn mask_group(editor: &DocumentEditor, scope: &Dict, name: &[u8]) -> Option<Vec<u8>> {
    let table = Resolve::resolve_key(editor, scope, editor.intern(b"ExtGState"));
    let state = table.as_dict()?.get(editor.intern(name))?.clone();
    let state = Resolve::resolve(editor, &state);
    let mask = Resolve::resolve_key(editor, state.as_dict()?, editor.intern(b"SMask"));
    let group = mask.as_dict()?.get_ref(editor.intern(b"G"))?;
    editor.stream_bytes(group)
}

/// How deep form XObjects may nest before recursion is refused (8.10).
///
/// **At least as deep as this engine draws.** The interpreter enters a form
/// whose `Do` is made at a depth below its own `MAX_FORM_DEPTH`, 16
/// (`interpret.rs`): sixteen levels of forms under a page. This was 12 until
/// October 2026, and a form nested thirteen to sixteen deep was drawn by the
/// renderer and never measured by a redaction — its text left under the
/// rectangle with nothing in the report. The walk's test is `depth >` this,
/// one level looser than the interpreter's `>=`, so a chain under an
/// annotation's appearance, which the renderer runs as content rather than
/// through a `Do`, is measured as deep as it is drawn too; a page's chain is
/// measured one level deeper than it is drawn, which removes nothing the
/// renderer would have shown.
const MAX_FORM_DEPTH: u32 = 16;

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

        let inner = form_transform(editor, &dict, used.ctm);

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
        let (data, pass, inner_uses) = cut_stream(
            editor,
            &inner_resources,
            &entry.content,
            areas,
            &fonts,
            inner,
            &mut report.warnings,
        );
        for warning in pass.warnings {
            note(&mut report.warnings, warning);
        }
        unread(
            editor,
            &inner_resources,
            &entry.content,
            areas,
            &mut report.warnings,
        );
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
/// left holding anything nothing draws, and never loses anything something
/// else draws:
///
/// - an **uncut** outcome keeps it untouched, when some placement cut nothing
///   — so every other page that draws the form draws it as it was;
/// - when every placement cut something and **something other than this
///   page's walk draws the form** — another page, a form or an annotation
///   there, a Type 3 glyph's procedure anywhere ([`Elsewhere`]) — it is left
///   untouched too and every placement here draws a copy: what it holds is
///   what that other drawer shows, and none of it is under these rectangles
///   there;
/// - otherwise the **first** placement's outcome is written into it, which
///   that placement then draws.
///
/// So the form's own object is drawn by this page or by something else, and
/// never becomes an unreferenced stream still holding what a rectangle
/// covered — which a copy for every placement, with nothing else drawing the
/// form, would have made it.
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
    page: ObjRef,
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

    let mut elsewhere = Elsewhere::new(page);
    for &form in &order {
        if exact(&old_way, form) {
            decide(editor, walk, form, &mut targets, report, &mut elsewhere);
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
    elsewhere: &mut Elsewhere,
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
    // The outcome the form's own object holds, or `None` when it is left as
    // it was for something else to draw and every placement here draws a
    // copy ([`settle`] says which).
    let home: Option<Outcome> = match outcomes.iter().find(|o| unchanged(o)) {
        Some(uncut) => Some(uncut.clone()),
        None if elsewhere.draws(editor, entry.reference) => None,
        None => outcomes.first().cloned(),
    };

    // (object, the placement whose scope it is written in, its outcome)
    let mut writes: Vec<(ObjRef, usize, Outcome)> = Vec::new();
    let mut copies: Vec<(Outcome, ObjRef)> = Vec::new();
    let mut home_written = home.as_ref().is_none_or(unchanged);
    for (&n, outcome) in entry.nodes.iter().zip(&outcomes) {
        if home.as_ref() == Some(outcome) {
            if !home_written {
                writes.push((entry.reference, n, outcome.clone()));
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

/// Which forms something other than the redacted page's walk draws, read
/// once, the first time [`decide`] needs to know.
///
/// It is asked only about a form every placement of which on this page cut
/// something — the one case where the form's own object would otherwise take
/// a cut, and a drawer elsewhere would lose what this page's rectangles
/// covered. Read lazily for that reason, and for the reason it is safe to:
/// until the first such form, [`decide`] has written only copies, so the
/// forms the read follows are the objects as they were before this
/// redaction.
struct Elsewhere {
    page: ObjRef,
    drawn: Option<HashSet<u32>>,
}

impl Elsewhere {
    fn new(page: ObjRef) -> Elsewhere {
        Elsewhere { page, drawn: None }
    }

    /// Whether something other than this page's walk draws `form`.
    fn draws(&mut self, editor: &DocumentEditor, form: ObjRef) -> bool {
        let page = self.page;
        self.drawn
            .get_or_insert_with(|| drawn_elsewhere(editor, page))
            .contains(&form.num)
    }
}

/// Every form drawn by something other than `page`'s walk, by object
/// number.
///
/// **Drawn, not named.** A page's resources naming a form is not a page
/// drawing it — one `/Resources` dictionary shared by every page is an
/// ordinary way to write a file — and counting a name would leave a form
/// whose every placement here was cut whole in the file with nothing drawing
/// it, holding exactly what the rectangles covered. So each other page's
/// content is read for its `Do`s, resolved in the scope that makes them
/// (8.10.1), into every form at any depth, every appearance of every
/// annotation on it (12.5.5, every state, as the walk reads them), and the
/// glyph procedures of every Type 3 face it selects (9.6.5).
///
/// This page is read too, for one thing only: the glyph procedures of the
/// Type 3 faces it selects, whose `Do`s the walk does not enter — it
/// measures a procedure rather than cutting it ([`cut_stream`]) — and whose
/// forms are drawn, through the procedure, as they are in the file.
///
/// A procedure's `Do` is resolved in the enclosing scope, where this
/// engine's interpreter runs it, **and** in the face's own `/Resources`,
/// where 9.6.5 puts it: a drawer either reader would find counts. Every
/// procedure of a face counts, not only those of the glyphs shown, and a face
/// is read in the first scope that selects it; both err toward *drawn*,
/// which costs a copy rather than a cut. Not read, as the walk does not read
/// them: a tiling pattern's cell and a soft mask's group.
///
/// One read of each stream for each of the two roles, so the work is the
/// document's size; [`MAX_FORM_DEPTH`] bounds the nesting, as it bounds the
/// walk's.
fn drawn_elsewhere(editor: &DocumentEditor, page: ObjRef) -> HashSet<u32> {
    let mut scan = Scan {
        editor,
        drawn: HashSet::new(),
        entered: HashSet::new(),
        faces: HashSet::new(),
    };
    for other in editor.page_refs() {
        let counting = other != page;
        let Some(read) = EditorPage::read(editor, other) else {
            continue;
        };
        scan.stream(&read.content, &read.resources, counting, 0);
        for appearance in appearances_on(editor, other) {
            if counting {
                scan.drawn.insert(appearance.stream.num);
            }
            scan.form(
                appearance.stream,
                &appearance.dict,
                &read.resources,
                counting,
                1,
            );
        }
    }
    scan.drawn
}

/// One [`drawn_elsewhere`] read in progress.
struct Scan<'a> {
    editor: &'a DocumentEditor,
    drawn: HashSet<u32>,
    /// Forms read, and whether their `Do`s counted: one drawn on this page
    /// is read for its procedures, and again if a procedure draws it.
    entered: HashSet<(u32, bool)>,
    /// Type 3 faces whose procedures were read: by object, or by resource
    /// name for a face written directly into a resource dictionary.
    faces: HashSet<Vec<u8>>,
}

impl Scan<'_> {
    /// Reads one content stream's `Do`s and `Tf`s.
    fn stream(&mut self, content: &[u8], scope: &Dict, counting: bool, depth: u32) {
        if depth > MAX_FORM_DEPTH {
            return;
        }
        let mut tokens = Tokenizer::new(content);
        let mut operands: Vec<Token> = Vec::new();
        while let Some(token) = tokens.next_token() {
            let Token::Operator(op) = &token else {
                operands.push(token);
                continue;
            };
            match op.as_slice() {
                // 8.9.7: the samples are not tokens, and a `Do` spelled by
                // them is not a `Do`.
                b"BI" => {
                    let consumed = tinker_pdf_content::interpret::skip_inline_image(tokens.rest());
                    let at = tokens.position();
                    tokens.seek(at.saturating_add(consumed));
                }
                b"Do" => {
                    if let Some(Token::Name(name)) = operands.last() {
                        let name = name.clone();
                        self.xobject(scope, &name, counting, depth);
                    }
                }
                b"Tf" => {
                    let named = operands.len().checked_sub(2).and_then(|i| operands.get(i));
                    if let Some(Token::Name(name)) = named {
                        let name = name.clone();
                        self.face(scope, &name, depth);
                    }
                }
                _ => {}
            }
            operands.clear();
        }
    }

    fn xobject(&mut self, scope: &Dict, name: &[u8], counting: bool, depth: u32) {
        let Some((reference, dict)) = resolve_xobject(self.editor, scope, name) else {
            return;
        };
        let subtype = Resolve::resolve_key(self.editor, &dict, self.editor.intern(b"Subtype"))
            .as_name()
            .and_then(|n| self.editor.document().name_bytes(n));
        if subtype.as_deref() != Some(b"Form".as_slice()) {
            return;
        }
        if counting {
            self.drawn.insert(reference.num);
        }
        self.form(reference, &dict, scope, counting, depth + 1);
    }

    /// Reads a form's content in its own resources, or the scope that drew
    /// it (8.10.1).
    fn form(&mut self, reference: ObjRef, dict: &Dict, scope: &Dict, counting: bool, depth: u32) {
        if !self.entered.insert((reference.num, counting)) {
            return;
        }
        let Some(content) = self.editor.stream_bytes(reference) else {
            return;
        };
        let resources = Resolve::resolve_key(self.editor, dict, Name::RESOURCES)
            .as_dict()
            .cloned()
            .unwrap_or_else(|| scope.clone());
        self.stream(&content, &resources, counting, depth);
    }

    /// Reads every glyph procedure of the Type 3 face `name` selects, which
    /// draws for any page that shows its glyphs — so its `Do`s count
    /// wherever it was selected.
    fn face(&mut self, scope: &Dict, name: &[u8], depth: u32) {
        let editor = self.editor;
        let fonts = Resolve::resolve_key(editor, scope, editor.intern(b"Font"));
        let Some(entry) = fonts.as_dict().and_then(|f| f.get(editor.intern(name))) else {
            return;
        };
        let (key, font) = match entry {
            Object::Ref(r) => (
                format!("R{} {}", r.num, r.gen).into_bytes(),
                editor.get(*r).and_then(|o| o.as_dict().cloned()),
            ),
            Object::Dict(d) => ([b"/".as_slice(), name].concat(), Some(d.clone())),
            _ => return,
        };
        let Some(font) = font else {
            return;
        };
        let subtype = font
            .get_name(editor.intern(b"Subtype"))
            .and_then(|n| editor.document().name_bytes(n));
        if subtype.as_deref() != Some(b"Type3".as_slice()) || !self.faces.insert(key) {
            return;
        }
        let own = Resolve::resolve_key(editor, &font, Name::RESOURCES)
            .as_dict()
            .cloned();
        let procs = Resolve::resolve_key(editor, &font, editor.intern(b"CharProcs"));
        let Some(procs) = procs.as_dict() else {
            return;
        };
        for (_, value) in procs.iter() {
            let Some(content) = value.as_objref().and_then(|r| editor.stream_bytes(r)) else {
                continue;
            };
            self.stream(&content, scope, true, depth + 1);
            if let Some(own) = &own {
                self.stream(&content, own, true, depth + 1);
            }
        }
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
        let (next, pass, _) = cut_stream(
            editor,
            &node.resources,
            &data,
            areas,
            &fonts,
            node.ctm,
            &mut report.warnings,
        );
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

/// The transform a form's content is drawn under: its own `/Matrix`, then
/// the transform in force at the `Do` (8.10.2).
fn form_transform(editor: &DocumentEditor, dict: &Dict, ctm: Matrix) -> Matrix {
    let matrix = Resolve::resolve_key(editor, dict, editor.intern(b"Matrix"))
        .as_array()
        .map(|a| a.iter().filter_map(Object::as_number).collect::<Vec<f64>>())
        .filter(|v| v.len() >= 6 && v.iter().all(|x| x.is_finite()))
        .and_then(|v| Matrix::from_operands(&v));
    match matrix {
        Some(m) => m.then(ctm),
        None => ctm,
    }
}

/// Cuts one content stream: [`rewrite`], and then again with every Type 3
/// glyph removed whose procedure draws text or an image under a rectangle.
///
/// # A glyph procedure is measured per use, and is not rewritten
///
/// 9.6.5: a Type 3 glyph is drawn by running its procedure, and the
/// procedure can show text in a font of its own or draw an image — anywhere,
/// not only inside the box [`Pen::glyph_box`] measures. So every use of such
/// a glyph is measured through its procedure, under the transform the
/// interpreter runs it with ([`draws_under`]), and a use whose procedure
/// draws under a rectangle is **removed whole**, the way a glyph partly under
/// one is.
///
/// The procedure itself is left exactly as it is, and that is the decision
/// this is written down for. A procedure is the font's: every use of that
/// glyph, on this page and every other, runs the same stream. Cutting the
/// covered text out of it would cut it out of every one of those — the
/// over-removal a form drawn twice used to cost, with nowhere to put a copy
/// short of a new glyph in the font. Worse, a bitmap face draws each glyph as
/// an inline image a shade larger than its advance, so a rectangle beside a
/// word would have blanked that letter throughout the document. Removed per
/// use, what goes is what the rectangle covers at that use and nothing else.
/// What that leaves is the procedure's own bytes in the font, as an embedded
/// program keeps its outlines — see the module's "What this module does not
/// remove".
fn cut_stream(
    editor: &DocumentEditor,
    scope: &Dict,
    content: &[u8],
    areas: &[Redaction],
    fonts: &HashMap<Vec<u8>, Arc<RunFont>>,
    ctm: Matrix,
    warnings: &mut Vec<RedactionWarning>,
) -> (Vec<u8>, RedactionReport, Vec<XObjectUse>) {
    let mut procedures = Procedures::default();
    let first = rewrite(content, areas, fonts, ctm, &mut procedures);
    if procedures.found.is_empty() {
        return first;
    }
    let measure = Measure {
        editor,
        scope,
        fonts,
        areas,
    };
    let mut drop = HashSet::new();
    for glyph in &procedures.found {
        // Per use: one glyph's procedures cannot spend another's.
        let mut budget = MAX_PLACEMENTS;
        if draws_under(&measure, &glyph.procedure, glyph.ctm, warnings, &mut budget) {
            drop.insert(glyph.index);
        }
    }
    if drop.is_empty() {
        return first;
    }
    let mut again = Procedures {
        drop,
        ..Procedures::default()
    };
    rewrite(content, areas, fonts, ctm, &mut again)
}

/// What [`draws_under`] measures in.
struct Measure<'a> {
    editor: &'a DocumentEditor,
    /// The resources `Do` names are resolved in: the scope that showed the
    /// glyph, which is where this engine's interpreter runs a procedure.
    scope: &'a Dict,
    fonts: &'a HashMap<Vec<u8>, Arc<RunFont>>,
    areas: &'a [Redaction],
}

/// Whether content drawn under `ctm` puts text or an image under a
/// rectangle. Measured only: nothing is written.
///
/// Text and inline images are what [`rewrite`] measures; an XObject is
/// followed as the walk follows one — an image by its unit square, a form
/// into its content — and a Type 3 glyph the content shows into its own
/// procedure.
///
/// Every stream run spends one of `budget`, which [`cut_stream`] sets to
/// [`MAX_PLACEMENTS`] for each use of a glyph: procedures that show glyphs
/// whose procedures show glyphs branch at every level, a procedure can show
/// its own glyph, and either would otherwise make the work exponential or
/// endless. The budget is per use rather than per stream because a page of a
/// benign two-level face — a glyph whose procedure draws a form, or shows a
/// word in another Type 3 face — spends a few runs on every use, and a budget
/// for the stream would run out a few dozen uses in and remove every use
/// after (`every_use_of_a_glyph_has_a_budget_of_its_own`: two runs a use, so
/// thirty-two). Past it the answer is *yes*, the direction that removes a
/// glyph rather than leaving one; only a face that recurses ever reaches it.
fn draws_under(
    measure: &Measure<'_>,
    content: &[u8],
    ctm: Matrix,
    warnings: &mut Vec<RedactionWarning>,
    budget: &mut usize,
) -> bool {
    if *budget == 0 {
        return true;
    }
    *budget -= 1;

    let mut procedures = Procedures::default();
    let (_, pass, uses) = rewrite(content, measure.areas, measure.fonts, ctm, &mut procedures);
    for warning in pass.warnings {
        note(warnings, warning);
    }
    unread(
        measure.editor,
        measure.scope,
        content,
        measure.areas,
        warnings,
    );
    if pass.glyphs > 0 || pass.images > 0 {
        return true;
    }

    for used in &uses {
        let Some((reference, dict)) = resolve_xobject(measure.editor, measure.scope, &used.name)
        else {
            continue;
        };
        let subtype =
            Resolve::resolve_key(measure.editor, &dict, measure.editor.intern(b"Subtype"))
                .as_name()
                .and_then(|n| measure.editor.document().name_bytes(n));
        match subtype.as_deref() {
            Some(b"Image") => {
                if covers_unit_square(used, measure.areas) {
                    return true;
                }
            }
            Some(b"Form") => {
                let Some(inner_content) = measure.editor.stream_bytes(reference) else {
                    continue;
                };
                // 8.10.1, as the walk reads it: the form's own resources, or
                // the scope that drew it.
                let resources = Resolve::resolve_key(measure.editor, &dict, Name::RESOURCES)
                    .as_dict()
                    .cloned()
                    .unwrap_or_else(|| measure.scope.clone());
                let fonts = fonts_in(measure.editor.document(), &resources);
                let inner = Measure {
                    editor: measure.editor,
                    scope: &resources,
                    fonts: &fonts,
                    areas: measure.areas,
                };
                let placed = form_transform(measure.editor, &dict, used.ctm);
                if draws_under(&inner, &inner_content, placed, warnings, budget) {
                    return true;
                }
            }
            _ => {}
        }
    }

    procedures
        .found
        .iter()
        .any(|glyph| draws_under(measure, &glyph.procedure, glyph.ctm, warnings, budget))
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

/// Where an annotation's dictionary is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnnotationAt {
    /// An object of its own, which is how nearly every file writes one.
    Object(ObjRef),
    /// Written into `/Annots` itself, at this index.
    Inline(usize),
}

/// One appearance stream an annotation on the page can show, and where
/// 12.5.5 puts it.
struct AppearanceAt {
    annotation: AnnotationAt,
    /// The annotation's dictionary as the walk read it, which is where an
    /// inline one is edited from.
    annotation_dict: Dict,
    /// `N`, `R` or `D`.
    key: &'static [u8],
    /// The state, when the entry is a dictionary of them.
    state: Option<Name>,
    stream: ObjRef,
    /// The stream's dictionary.
    dict: Dict,
    /// 12.5.5's matrix **A**: what maps the form's space, after its own
    /// `/Matrix`, onto the page. The walk composes the `/Matrix` itself, as
    /// it does for any form.
    fit: Matrix,
}

/// Every appearance stream every annotation on a page can show, read through
/// the editor.
///
/// All of `/N`, `/R` and `/D`, and every state of each — not only the one
/// `/AS` selects, because a viewer switches states with no edit to the file,
/// and the state that is off today draws tomorrow. An annotation without a
/// `/Rect` enclosing any area, or an appearance whose `/BBox` 12.5.5 cannot
/// fit onto it, is drawn nowhere by this engine's renderer
/// (`annots::prepare`) and is not measured either.
fn appearances_on(editor: &DocumentEditor, page: ObjRef) -> Vec<AppearanceAt> {
    let mut out = Vec::new();
    let Some(Object::Dict(page)) = editor.get(page) else {
        return out;
    };
    let annots = Resolve::resolve_key(editor, &page, editor.intern(b"Annots"));
    let Some(entries) = annots.as_array() else {
        return out;
    };

    for (index, entry) in entries.iter().enumerate() {
        let (at, annotation) = match entry {
            Object::Ref(r) => match editor.get(*r).and_then(|o| o.as_dict().cloned()) {
                Some(dict) => (AnnotationAt::Object(*r), dict),
                None => continue,
            },
            Object::Dict(dict) => (AnnotationAt::Inline(index), dict.clone()),
            _ => continue,
        };
        let Some(rect) = Resolve::resolve_key(editor, &annotation, editor.intern(b"Rect"))
            .as_array()
            .and_then(Rect::from_array)
            .filter(|r| !r.is_empty())
        else {
            continue;
        };
        let ap = Resolve::resolve_key(editor, &annotation, editor.intern(b"AP"));
        let Some(ap) = ap.as_dict() else {
            continue;
        };

        for key in [b"N".as_slice(), b"R", b"D"] {
            let Some(value) = ap.get(editor.intern(key)) else {
                continue;
            };
            for (state, stream) in appearance_streams(editor, value) {
                let Some(dict) = editor.get(stream).and_then(|o| o.as_dict().cloned()) else {
                    continue;
                };
                let Some(fit) = appearance_fit(editor, &dict, rect) else {
                    continue;
                };
                out.push(AppearanceAt {
                    annotation: at,
                    annotation_dict: annotation.clone(),
                    key,
                    state,
                    stream,
                    dict,
                    fit,
                });
            }
        }
    }
    out
}

/// The streams one `/AP` entry can be: a stream, or a dictionary of states
/// each naming one — told apart by being a stream, as the renderer tells
/// them apart, rather than by carrying a `/BBox`.
fn appearance_streams(editor: &DocumentEditor, value: &Object) -> Vec<(Option<Name>, ObjRef)> {
    let states = |dict: &Dict| -> Vec<(Option<Name>, ObjRef)> {
        dict.iter()
            .filter_map(|(name, v)| Some((Some(*name), v.as_objref()?)))
            .collect()
    };
    match value {
        Object::Ref(r) => {
            if editor.stream_bytes(*r).is_some() {
                return vec![(None, *r)];
            }
            editor
                .get(*r)
                .and_then(|o| o.as_dict().map(&states))
                .unwrap_or_default()
        }
        Object::Dict(dict) => states(dict),
        _ => Vec::new(),
    }
}

/// 12.5.5's matrix **A**, as `annots::fit` computes it for the renderer: the
/// form's `/BBox` carried through its `/Matrix`, and the axis-aligned box of
/// the result scaled and moved onto `/Rect`.
///
/// `None` where the renderer draws nothing: a box with no extent, or one
/// that is not finite. A form with no `/BBox` has nothing to fit and is drawn
/// where its own matrix puts it, so **A** is the identity.
fn appearance_fit(editor: &DocumentEditor, form: &Dict, rect: Rect) -> Option<Matrix> {
    let Some(bbox) = Resolve::resolve_key(editor, form, editor.intern(b"BBox"))
        .as_array()
        .and_then(Rect::from_array)
    else {
        return Some(Matrix::IDENTITY);
    };
    if bbox.is_empty() {
        return None;
    }
    let matrix = form_transform(editor, form, Matrix::IDENTITY);
    let corners = [
        matrix.apply(bbox.x0, bbox.y0),
        matrix.apply(bbox.x1, bbox.y0),
        matrix.apply(bbox.x1, bbox.y1),
        matrix.apply(bbox.x0, bbox.y1),
    ];
    if corners
        .iter()
        .any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return None;
    }
    let (mut x0, mut y0) = corners[0];
    let (mut x1, mut y1) = corners[0];
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (dx, dy) = (x1 - x0, y1 - y0);
    if dx <= f64::EPSILON || dy <= f64::EPSILON {
        return None;
    }
    let sx = (rect.x1 - rect.x0) / dx;
    let sy = (rect.y1 - rect.y0) / dy;
    Some(Matrix {
        a: sx,
        b: 0.0,
        c: 0.0,
        d: sy,
        e: rect.x0 - x0 * sx,
        f: rect.y0 - y0 * sy,
    })
}

impl Walk {
    /// Enters one annotation appearance as a placement of its form.
    ///
    /// Its scope is the page's resources, for an appearance that has none of
    /// its own (8.10.1), and its name — for a report that has to name it — is
    /// `AP/` and the entry, with the state when there is one.
    fn appearance(
        &mut self,
        editor: &mut DocumentEditor,
        appearance: &AppearanceAt,
        page_resources: &Dict,
        areas: &[Redaction],
        report: &mut RedactionReport,
    ) -> Option<usize> {
        let mut name = b"AP/".to_vec();
        name.extend_from_slice(appearance.key);
        if let Some(state) = appearance.state {
            name.push(b'/');
            name.extend_from_slice(&editor.document().name_bytes(state).unwrap_or_default());
        }
        let used = XObjectUse {
            name,
            ctm: appearance.fit,
            at: 0..0,
        };
        let placing = Placing {
            reference: appearance.stream,
            dict: appearance.dict.clone(),
            used: &used,
            scope: page_resources,
        };
        self.form(editor, placing, areas, report, 0)
    }
}

/// Points each annotation whose appearance was given a copy at the copy.
///
/// The annotation's `/AP` is written as a direct dictionary of its own — and
/// the state dictionary under it, when the entry is one — because either may
/// be an object other annotations share, none of which draws the copy.
/// Returns the edits to annotations written inline in `/Annots`, which the
/// caller makes where the page is written.
fn repoint_appearances(
    editor: &mut DocumentEditor,
    appearances: &[AppearanceAt],
    shown: &[Option<usize>],
    targets: &[Target],
) -> Vec<(usize, Dict)> {
    let mut edited: Vec<(AnnotationAt, Dict)> = Vec::new();
    for (appearance, node) in appearances.iter().zip(shown) {
        let Some(Target::Copy(copy)) = node.and_then(|n| targets.get(n).copied()) else {
            continue;
        };
        let slot = match edited
            .iter()
            .position(|(at, _)| *at == appearance.annotation)
        {
            Some(slot) => slot,
            None => {
                let dict = match appearance.annotation {
                    AnnotationAt::Object(r) => editor.get(r).and_then(|o| o.as_dict().cloned()),
                    AnnotationAt::Inline(_) => Some(appearance.annotation_dict.clone()),
                };
                let Some(dict) = dict else {
                    continue;
                };
                edited.push((appearance.annotation, dict));
                edited.len() - 1
            }
        };
        let Some((_, annotation)) = edited.get_mut(slot) else {
            continue;
        };

        let ap_key = editor.intern(b"AP");
        let key = editor.intern(appearance.key);
        let mut ap = Resolve::resolve_key(editor, annotation, ap_key)
            .as_dict()
            .cloned()
            .unwrap_or_default();
        match appearance.state {
            None => {
                ap.insert(key, Object::Ref(copy));
            }
            Some(state) => {
                let mut states = Resolve::resolve_key(editor, &ap, key)
                    .as_dict()
                    .cloned()
                    .unwrap_or_default();
                states.insert(state, Object::Ref(copy));
                ap.insert(key, Object::Dict(states));
            }
        }
        annotation.insert(ap_key, Object::Dict(ap));
    }

    let mut inline = Vec::new();
    for (at, annotation) in edited {
        match at {
            AnnotationAt::Object(r) => editor.put(r, Object::Dict(annotation)),
            AnnotationAt::Inline(index) => inline.push((index, annotation)),
        }
    }
    inline
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

/// The inline image that replaces one a redaction covered: a single blank
/// sample, as [`scrub_image`] writes for an XObject, and a stencil mask kept a
/// stencil mask whose one sample paints nothing (8.9.6.2).
///
/// `span` is the original's, from after `BI` through `EI`; only its
/// dictionary is read, for `/IM` (or `/ImageMask`), and none of it is kept.
fn blank_inline_image(span: &[u8]) -> &'static [u8] {
    let dictionary = span
        .windows(2)
        .position(|w| w == b"ID")
        .and_then(|end| span.get(..end))
        .unwrap_or(span);
    let mut tokens = Tokenizer::new(dictionary);
    let mut previous: Option<Token> = None;
    let mut mask = false;
    while let Some(token) = tokens.next_token() {
        if let (Some(Token::Name(key)), Token::Bool(true)) = (&previous, &token) {
            if key.as_slice() == b"IM" || key.as_slice() == b"ImageMask" {
                mask = true;
            }
        }
        previous = Some(token);
    }
    if mask {
        b"BI /IM true /W 1 /H 1 /BPC 1 ID \x80 EI"
    } else {
        b"BI /W 1 /H 1 /CS /G /BPC 8 ID \xFF EI"
    }
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

/// The Type 3 glyphs one pass over a stream showed through a procedure that
/// can draw text or an image, and those a second pass is to remove.
///
/// Counted by occurrence, in stream order: a pass is deterministic, so the
/// `n`th such glyph of the first pass is the `n`th of the second.
#[derive(Default)]
struct Procedures {
    /// Occurrences to remove whatever their box says.
    drop: HashSet<usize>,
    /// The next occurrence's index.
    next: usize,
    /// Every occurrence the pass kept, with where its procedure runs.
    found: Vec<GlyphUse>,
}

/// One Type 3 glyph kept, and the transform its procedure runs under: the
/// font matrix, then the text rendering matrix at the glyph's origin (9.4.4),
/// then the transform in force — where the interpreter runs it.
struct GlyphUse {
    index: usize,
    procedure: Arc<[u8]>,
    ctm: Matrix,
}

/// Rewrites a content stream with redacted glyphs removed.
///
/// A glyph is measured by its box. The Type 3 glyph occurrences `procedures`
/// names are removed whatever their box says, and the other Type 3 glyphs
/// whose procedures could draw beyond it are recorded in it, for
/// [`cut_stream`] to measure.
fn rewrite(
    content: &[u8],
    areas: &[Redaction],
    fonts: &HashMap<Vec<u8>, Arc<RunFont>>,
    initial: Matrix,
    procedures: &mut Procedures,
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
    // `Do`s past [`MAX_XOBJECT_USES`], written back and not followed.
    let mut unfollowed = 0usize;

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
            b"BI" => {
                // 8.9.7: an inline image's samples are not tokens, so the
                // span through `EI` is taken whole — where the interpreter
                // says it ends — and written back byte for byte. Tokenizing
                // it re-serialized the samples as whatever tokens they
                // happened to spell, which corrupted every inline image on a
                // redacted page and scrubbed none of them.
                let rest = tokens.rest();
                let consumed = tinker_pdf_content::interpret::skip_inline_image(rest);
                let span = rest.get(..consumed).unwrap_or(rest);
                let at = tokens.position();
                tokens.seek(at.saturating_add(consumed));

                // 8.9.7: it occupies the unit square of the transform in
                // force, as an XObject image does, and goes whole or not at
                // all for the same reason ([`scrub_image`]).
                let placed = XObjectUse {
                    name: Vec::new(),
                    ctm: pen.ctm,
                    at: 0..0,
                };
                if covers_unit_square(&placed, areas) {
                    report.images += 1;
                    out.extend_from_slice(blank_inline_image(span));
                } else {
                    out.extend_from_slice(b"BI");
                    out.extend_from_slice(span);
                }
                out.push(b'\n');
                rewritten = true;
            }
            b"Do" => {
                // 8.8: the operand names an XObject. Which kind it is, and
                // what to do about it, is the caller's business — this crate
                // has the transform, and the caller has the dictionaries.
                if let Some(Token::Name(name)) = operands.last() {
                    if uses.len() < MAX_XOBJECT_USES {
                        uses.push(XObjectUse {
                            name: name.clone(),
                            ctm: pen.ctm,
                            at: 0..0,
                        });
                        recorded = true;
                    } else {
                        unfollowed = unfollowed.saturating_add(1);
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
                    let cut = redact_string(&bytes, &pen, areas, procedures);
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
                            let cut = redact_string(s, &local, areas, procedures);
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

    if unfollowed > 0 && !areas.is_empty() {
        note(
            &mut report.warnings,
            RedactionWarning::TooManyXObjects {
                skipped: unfollowed,
            },
        );
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
///
/// A Type 3 glyph whose procedure can draw text or an image
/// ([`RunFont::procedures`]) is counted in `procedures`: removed when a
/// previous pass found its procedure drawing under a rectangle, and otherwise
/// recorded, with the transform its procedure runs under, for that pass to
/// measure ([`cut_stream`]).
fn redact_string(bytes: &[u8], pen: &Pen, areas: &[Redaction], procedures: &mut Procedures) -> Cut {
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
    let mut origins: Vec<f64> = Vec::with_capacity(codes.len());
    let mut along = pen.along;
    for code in &codes {
        origins.push(along);
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

    for ((code, (quad, advance)), origin) in codes.iter().zip(&boxes).zip(&origins) {
        let mut inside = areas
            .iter()
            .any(|redaction| quad_meets_rect(quad, redaction.area));

        // 9.6.5: the procedure is what the glyph draws, and it can draw
        // beyond the box — text in a font of its own, an image. A use whose
        // procedure draws under a rectangle goes whole, as a glyph partly
        // under one does; the procedure itself, which every use of the glyph
        // shares, is left as it is.
        if let (Some(space), Some(procedure)) =
            (selected.glyph_space, selected.procedures.get(&code.code))
        {
            let index = procedures.next;
            procedures.next += 1;
            if procedures.drop.contains(&index) {
                inside = true;
            } else if !inside {
                let placed = Matrix {
                    a: pen.size * pen.horizontal_scale,
                    b: 0.0,
                    c: 0.0,
                    d: pen.size,
                    e: *origin,
                    f: pen.rise,
                };
                procedures.found.push(GlyphUse {
                    index,
                    procedure: Arc::clone(procedure),
                    ctm: space.matrix.then(placed).then(frame),
                });
            }
        }

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

    /// A page drawing `/Fm1`, which draws `/Fm2`, and so on down to
    /// `/Fm{levels}`, which draws `PUBLIC SECRET` in Helvetica at 12 points
    /// from (10, 50) — the line [`document`] draws, at the bottom of a chain.
    fn nested(levels: u32) -> Arc<CosDocument> {
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 100]\n\
             /Resources << /XObject << /Fm1 10 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        out.push_str("4 0 obj\n<< /Length 8 >>\nstream\n/Fm1 Do\nendstream\nendobj\n");
        out.push_str("5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
        for level in 1..=levels {
            let body = if level == levels {
                "BT /F0 12 Tf 10 50 Td (PUBLIC SECRET) Tj ET".to_string()
            } else {
                format!("/Fm{} Do", level + 1)
            };
            out.push_str(&format!(
                "{} 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 100]\n\
                 /Resources << /XObject << /Fm{} {} 0 R >> /Font << /F0 5 0 R >> >>\n\
                 /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
                9 + level,
                level + 1,
                10 + level,
                body.len() + 1
            ));
        }
        out.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            10 + levels
        ));
        Arc::new(CosDocument::open(out.into_bytes()).expect("it opens"))
    }

    fn plain_text(bytes: Vec<u8>) -> String {
        crate::Document::open(bytes)
            .expect("it opens")
            .page(0)
            .expect("the page")
            .text()
            .plain_text()
    }

    /// Text sixteen forms down — as deep as this engine's interpreter draws
    /// — is redacted. Until October 2026 the walk stopped at thirteen, and
    /// this line stayed on the page, extracted and drawn, with `glyphs: 0`
    /// and no warning.
    #[test]
    fn text_as_deep_in_forms_as_the_renderer_draws_is_redacted() {
        let doc = nested(16);
        let before = plain_text(doc.bytes().to_vec());
        assert!(
            before.contains("PUBLIC SECRET"),
            "the renderer draws all sixteen levels: {before:?}"
        );
        let (bytes, report) = redact(doc, &[second_word()]);
        assert_eq!(report.glyphs, 6, "SECRET, sixteen forms down");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let text = plain_text(bytes.clone());
        assert!(
            text.contains("PUBLIC") && !text.contains("SECRET"),
            "{text:?}"
        );
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "{streams}");
    }

    /// The other side of the same line, pinned so that the two limits cannot
    /// drift apart unseen: seventeen levels down the interpreter draws
    /// nothing, and the walk still measures — the one level it goes past the
    /// renderer, toward removal — so a renderer that one day draws deeper
    /// fails this before it leaves a redaction behind.
    #[test]
    fn the_renderer_draws_no_deeper_than_the_walk_measures() {
        let doc = nested(17);
        let before = plain_text(doc.bytes().to_vec());
        assert!(
            !before.contains("SECRET"),
            "the interpreter stops at sixteen: {before:?}"
        );
        let (_, report) = redact(doc, &[second_word()]);
        assert_eq!(report.glyphs, 6, "the walk measured the seventeenth level");
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

    /// A page with an inline image of four gray samples at x 100..150,
    /// y 100..150, and `SECRET` in the boxed Type 3 font at x 10..70, y 10..20.
    fn inline_image_page(dictionary: &str) -> Vec<u8> {
        let mut content = format!("q 50 0 0 50 100 100 cm BI {dictionary} ID ").into_bytes();
        content.extend_from_slice(&[0x10, 0x20, 0x30, 0x40]);
        content.extend_from_slice(b" EI Q BT /F0 10 Tf 10 10 Td (SECRET) Tj ET");
        let body = String::from_utf8(content).expect("ASCII");
        super::tests_support::boxed_glyph_document(
            200.0,
            200.0,
            super::tests_support::DEFAULT_FONT_MATRIX,
            &body,
        )
    }

    /// An inline image on a redacted page comes back **byte for byte**.
    ///
    /// 8.9.7's samples are not tokens. Until September 2026 the rewrite
    /// tokenized them like the rest of the stream and wrote back whatever
    /// tokens they spelled: these four samples — a control byte, a space, a
    /// `0` and an `@` — came back as three bytes, and every inline image on
    /// every redacted page was corrupted, the redaction reporting success.
    #[test]
    fn an_inline_image_is_carried_through_a_rewrite_byte_for_byte() {
        let bytes = inline_image_page("/W 2 /H 2 /CS /G /BPC 8");
        let text = Rect {
            x0: 0.0,
            y0: 5.0,
            x1: 200.0,
            y1: 25.0,
        };
        let before = super::tests_support::render(bytes.clone());

        let (after, report) = redact(
            Arc::new(CosDocument::open(bytes).expect("it opens")),
            &[Redaction {
                area: text,
                mark: false,
            }],
        );
        assert_eq!((report.glyphs, report.images), (6, 0));
        let streams = all_streams(&CosDocument::open(after.clone()).expect("it reopens"));
        assert!(
            streams.contains("ID \u{10} 0@ EI"),
            "the samples are there as they were: {streams:?}"
        );
        assert_eq!(
            super::tests_support::differing_outside(
                &before,
                &super::tests_support::render(after),
                200.0,
                text
            ),
            0,
            "and the image draws exactly as it did"
        );
    }

    /// An inline image under a rectangle is scrubbed, as an XObject image is:
    /// whole, to one blank sample, and counted.
    #[test]
    fn an_inline_image_under_a_redaction_is_scrubbed() {
        for (dictionary, what) in [
            ("/W 2 /H 2 /CS /G /BPC 8", "an image"),
            ("/IM true /W 2 /H 2 /BPC 1", "a stencil mask"),
        ] {
            let bytes = inline_image_page(dictionary);
            let over = Rect {
                x0: 120.0,
                y0: 120.0,
                x1: 130.0,
                y1: 130.0,
            };
            let image = Rect {
                x0: 101.0,
                y0: 101.0,
                x1: 149.0,
                y1: 149.0,
            };
            assert!(
                super::tests_support::ink_in(
                    &super::tests_support::render(bytes.clone()),
                    200.0,
                    image
                ) > 0,
                "{what} starts inked"
            );

            let (after, report) = redact(
                Arc::new(CosDocument::open(bytes).expect("it opens")),
                &[Redaction {
                    area: over,
                    mark: false,
                }],
            );
            assert_eq!((report.glyphs, report.images), (0, 1), "{what}");
            let streams = all_streams(&CosDocument::open(after.clone()).expect("it reopens"));
            assert!(
                !streams.contains("\u{10} 0@"),
                "{what}'s samples are gone: {streams:?}"
            );
            // A stencil stays a stencil: a gray sample in its place would
            // paint a white square over whatever the mask left showing.
            assert_eq!(
                streams.contains("BI /IM true /W 1 /H 1 /BPC 1 ID"),
                dictionary.starts_with("/IM"),
                "{what} is replaced by its own kind: {streams:?}"
            );
            assert_eq!(
                super::tests_support::ink_in(&super::tests_support::render(after), 200.0, image),
                0,
                "{what} draws nothing"
            );
        }
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
        let (out, report, _) = rewrite(
            content,
            &[second_word()],
            &fonts,
            Matrix::IDENTITY,
            &mut Procedures::default(),
        );
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

/// Annotation appearance streams (12.5.5), cut where 12.5.5 draws them.
///
/// Every fixture is one 400 by 300 page whose font is the vendored Liberation
/// Serif, embedded whole, so an appearance both renders and — flattened into
/// the page, which is how an extractor that does not read annotations comes
/// to read one — extracts. The standard appearance draws `PUBLIC SECRET` at
/// 24 points in a `/BBox` of `0 0 400 40`, and 12.5.5 fits that box onto a
/// `/Rect` half its size, so on the page the words are 12 points tall:
/// `PUBLIC` x 10..52.67, the space to 55.67 and `SECRET` to 100.35, on a
/// baseline 5 points above the rectangle's bottom. None of that is in page
/// space until the fit is applied, so a walk that drew an appearance where its
/// form space lands — at the origin, twice the size — cuts nothing at all.
///
/// The positions read back are this engine's extractor and renderer compared
/// with themselves before and after the cut: self-consistency, as the other
/// modules here label it.
#[cfg(test)]
mod appearance_streams {
    use super::tests_support::*;
    use super::*;

    fn area(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    fn band(area: Rect) -> Redaction {
        Redaction { area, mark: false }
    }

    /// Over `SECRET` where the standard appearance puts it on a `/Rect`
    /// whose bottom is y 40, and clear of the space before it.
    fn over_secret_at(bottom: f64) -> Rect {
        area(56.0, bottom, 400.0, bottom + 30.0)
    }

    fn numbers(values: &[f64]) -> Object {
        Object::Array(values.iter().map(|v| Object::Real(*v)).collect())
    }

    /// The page: `KEEP` in Liberation Serif at the top, drawn by the builder
    /// so the font is registered, and nothing else. Returns it opened, with
    /// the font's object.
    fn page() -> (Arc<CosDocument>, ObjRef) {
        let mut builder = tinker_pdf_cos::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(
            b"F0",
            b"LiberationSerif",
            &crate::subset::tests_support::face()
        ));
        builder.add_page(400.0, 300.0, |p| p.text(b"F0", 12.0, 300.0, 280.0, "KEEP"));
        let doc = open(builder.finish());
        let font = crate::subset::tests_support::only_font(&doc);
        (doc, font)
    }

    /// An appearance stream drawing `content` in `/F0`, the page's font.
    fn appearance(
        editor: &mut DocumentEditor,
        font: ObjRef,
        bbox: [f64; 4],
        matrix: Option<[f64; 6]>,
        content: &str,
    ) -> ObjRef {
        let mut fonts = Dict::new();
        fonts.insert(editor.intern(b"F0"), Object::Ref(font));
        let mut resources = Dict::new();
        resources.insert(editor.intern(b"Font"), Object::Dict(fonts));
        let mut dict = Dict::new();
        dict.insert(
            editor.intern(b"Type"),
            Object::Name(editor.intern(b"XObject")),
        );
        dict.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(b"Form")),
        );
        dict.insert(editor.intern(b"BBox"), numbers(&bbox));
        if let Some(matrix) = matrix {
            dict.insert(editor.intern(b"Matrix"), numbers(&matrix));
        }
        dict.insert(Name::RESOURCES, Object::Dict(resources));
        let stream = editor.allocate();
        editor.put_stream(
            stream,
            StreamData {
                dict,
                data: content.as_bytes().to_vec(),
            },
        );
        stream
    }

    /// The standard appearance: `PUBLIC SECRET` at 24 points in a box twice
    /// the size of the `/Rect` it is fitted onto.
    fn public_secret(editor: &mut DocumentEditor, font: ObjRef) -> ObjRef {
        appearance(
            editor,
            font,
            [0.0, 0.0, 400.0, 40.0],
            None,
            "BT /F0 24 Tf 0 10 Td (PUBLIC SECRET) Tj ET",
        )
    }

    /// The `/Rect` the standard appearance is fitted onto, with its bottom at
    /// `bottom`.
    fn rect_at(bottom: f64) -> [f64; 4] {
        [10.0, bottom, 210.0, bottom + 20.0]
    }

    /// An annotation dictionary of `subtype` whose `/AP` is `ap`.
    fn annotation(editor: &DocumentEditor, subtype: &[u8], rect: [f64; 4], ap: Object) -> Dict {
        let mut dict = Dict::new();
        dict.insert(
            editor.intern(b"Type"),
            Object::Name(editor.intern(b"Annot")),
        );
        dict.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(subtype)),
        );
        dict.insert(editor.intern(b"Rect"), numbers(&rect));
        dict.insert(editor.intern(b"AP"), ap);
        dict
    }

    /// `/AP << /N stream >>`, direct.
    fn normal(editor: &DocumentEditor, stream: ObjRef) -> Object {
        let mut ap = Dict::new();
        ap.insert(editor.intern(b"N"), Object::Ref(stream));
        Object::Dict(ap)
    }

    /// An annotation as an object of its own.
    fn object(editor: &mut DocumentEditor, annotation: Dict) -> Object {
        let reference = editor.allocate();
        editor.put(reference, Object::Dict(annotation));
        Object::Ref(reference)
    }

    /// Page zero's `/Annots`, set to `entries`.
    fn annots(editor: &mut DocumentEditor, entries: Vec<Object>) {
        let page = editor.page_refs()[0];
        let Some(Object::Dict(mut dict)) = editor.get(page) else {
            panic!("the page is a dictionary");
        };
        dict.insert(editor.intern(b"Annots"), Object::Array(entries));
        editor.put(page, Object::Dict(dict));
    }

    fn saved(editor: &DocumentEditor) -> Vec<u8> {
        editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            ..tinker_pdf_cos::WriteOptions::default()
        })
    }

    /// What the extractor reads once page zero's annotations are flattened
    /// into its content (12.5.5's fit, as `flatten_annotations` writes it),
    /// bottom to top.
    fn flattened_lines(bytes: Vec<u8>) -> Vec<(f64, String)> {
        let mut editor = DocumentEditor::new(open(bytes));
        editor.flatten_annotations(0).expect("page zero");
        lines_of(saved(&editor))
    }

    /// The annotation at `index` in page zero's `/Annots`, resolved.
    fn annotation_at(doc: &CosDocument, index: usize) -> Dict {
        let pages = tinker_pdf_cos::pages::collect(doc);
        let page = doc.get(pages[0].reference).expect("the page");
        let annots = doc.resolve_key(page.as_dict().expect("a dict"), doc.intern(b"Annots"));
        let entry = annots.as_array().expect("an array")[index].clone();
        doc.resolve(&entry).as_dict().expect("a dict").clone()
    }

    /// The stream an annotation's `/AP` `/N` names.
    fn normal_of(doc: &CosDocument, annotation: &Dict) -> ObjRef {
        let ap = doc.resolve_key(annotation, doc.intern(b"AP"));
        ap.as_dict()
            .expect("an /AP")
            .get_ref(doc.intern(b"N"))
            .expect("an /N stream")
    }

    /// The row's exit, for an annotation: a FreeText annotation whose
    /// appearance draws `PUBLIC SECRET`, and a rectangle over `SECRET` where
    /// 12.5.5 puts it on the page.
    ///
    /// `SECRET` is gone from every stream, from the extractor and from the
    /// render; `PUBLIC` is in all three, and renders exactly as it did. The
    /// appearance is cut **in place**: one annotation draws it, so it needs
    /// no copy, and its object is the one the annotation still names.
    #[test]
    fn an_appearance_under_a_redaction_is_cut_where_its_annotation_draws_it() {
        let (doc, font) = page();
        let mut editor = DocumentEditor::new(doc);
        let stream = public_secret(&mut editor, font);
        let ap = normal(&editor, stream);
        let note = annotation(&editor, b"FreeText", rect_at(40.0), ap);
        let entry = object(&mut editor, note);
        annots(&mut editor, vec![entry]);
        let bytes = saved(&editor);

        assert_eq!(
            flattened_lines(bytes.clone()),
            vec![
                (45.0, "PUBLIC SECRET".to_string()),
                (280.0, "KEEP".to_string())
            ],
            "the fixture draws what its comment says, where it says"
        );
        let before = render(bytes.clone());
        let over = over_secret_at(40.0);
        assert!(ink_in(&before, 300.0, over) > 20, "SECRET starts inked");

        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 6, "S, E, C, R, E and T");
        assert_eq!(report.operations, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        assert_eq!(
            flattened_lines(after.clone()),
            vec![(45.0, "PUBLIC".to_string()), (280.0, "KEEP".to_string())]
        );
        let rendered = render(after.clone());
        assert_eq!(ink_in(&rendered, 300.0, over), 0, "no ink under the band");
        assert_eq!(
            differing_outside(&before, &rendered, 300.0, area(55.0, 40.0, 101.0, 60.0)),
            0,
            "PUBLIC, and everything else, renders exactly as it did"
        );

        let reopened = CosDocument::open(after).expect("it reopens");
        let streams = all_streams(&reopened);
        assert!(
            streams.contains("PUBLIC") && !streams.contains("SECRET"),
            "{streams}"
        );
        let annotation = annotation_at(&reopened, 0);
        let drawn = reopened
            .stream_decoded(normal_of(&reopened, &annotation))
            .expect("the appearance decodes");
        assert!(
            String::from_utf8_lossy(&drawn).contains("PUBLIC"),
            "the annotation still names the stream that was cut"
        );
        assert_eq!(forms_in(&reopened), 1, "and no copy was made");
    }

    /// 12.5.5 maps the form's `/BBox` **through its `/Matrix`** before
    /// fitting it, so an appearance turned a quarter turn runs up the page.
    ///
    /// `/Matrix [0 1 -1 0 0 0]` turns form (x, y) to (−y, x); the box
    /// 0 0 400 40 turns to x −40..0, y 0..400, and fitted onto
    /// `[300 10 320 210]` that is a halving and a move: page
    /// (320 − y/2, 10 + x/2). `PUBLIC` runs up from y 10 to 52.67 and
    /// `SECRET` from 55.67 to 100.35, both between x 303 and 315. A fit that
    /// ignored the matrix squeezes the 400-wide box into 20 points across and
    /// stretches it five times up, and cuts something else entirely.
    #[test]
    fn a_turned_appearance_is_cut_where_its_matrix_turns_it() {
        let (doc, font) = page();
        let mut editor = DocumentEditor::new(doc);
        let stream = appearance(
            &mut editor,
            font,
            [0.0, 0.0, 400.0, 40.0],
            Some([0.0, 1.0, -1.0, 0.0, 0.0, 0.0]),
            "BT /F0 24 Tf 0 10 Td (PUBLIC SECRET) Tj ET",
        );
        let ap = normal(&editor, stream);
        let stamp = annotation(&editor, b"Stamp", [300.0, 10.0, 320.0, 210.0], ap);
        let entry = object(&mut editor, stamp);
        annots(&mut editor, vec![entry]);
        let bytes = saved(&editor);

        let over = area(295.0, 56.0, 325.0, 120.0);
        let before = render(bytes.clone());
        assert!(ink_in(&before, 300.0, over) > 20, "SECRET starts inked");

        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 6, "SECRET, and not PUBLIC");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let rendered = render(after.clone());
        assert_eq!(ink_in(&rendered, 300.0, over), 0, "no ink under the band");
        assert_eq!(
            differing_outside(&before, &rendered, 300.0, area(300.0, 55.0, 318.0, 101.0)),
            0,
            "PUBLIC renders exactly as it did"
        );
        let streams = all_streams(&CosDocument::open(after).expect("it reopens"));
        assert!(
            streams.contains("PUBLIC") && !streams.contains("SECRET"),
            "{streams}"
        );
    }

    /// Every stream a viewer can show is cut, not only the one it shows
    /// today: both states of `/N` and of `/D`, the single `/R`, and the
    /// appearance of an annotation flagged hidden.
    ///
    /// A checkbox's off state is what `/AS` selects; its on state draws on the
    /// next click with no edit to the file, and a hidden annotation is one
    /// bit from being drawn. Six streams, six glyphs each. Turned on after
    /// the cut, the checkbox still draws nothing under the band.
    #[test]
    fn every_appearance_a_viewer_can_show_is_cut() {
        let (doc, font) = page();
        let mut editor = DocumentEditor::new(doc);
        let states = |editor: &mut DocumentEditor| {
            let on = public_secret(editor, font);
            let off = public_secret(editor, font);
            let mut dict = Dict::new();
            dict.insert(editor.intern(b"On"), Object::Ref(on));
            dict.insert(editor.intern(b"Off"), Object::Ref(off));
            Object::Dict(dict)
        };
        let n = states(&mut editor);
        let d = states(&mut editor);
        let r = public_secret(&mut editor, font);
        let mut ap = Dict::new();
        ap.insert(editor.intern(b"N"), n);
        ap.insert(editor.intern(b"D"), d);
        ap.insert(editor.intern(b"R"), Object::Ref(r));
        let mut checkbox = annotation(&editor, b"Widget", rect_at(40.0), Object::Dict(ap));
        checkbox.insert(editor.intern(b"AS"), Object::Name(editor.intern(b"Off")));
        let checkbox = object(&mut editor, checkbox);

        let hidden_stream = public_secret(&mut editor, font);
        let ap = normal(&editor, hidden_stream);
        let mut hidden = annotation(&editor, b"FreeText", rect_at(40.0), ap);
        hidden.insert(editor.intern(b"F"), Object::Int(2));
        let hidden = object(&mut editor, hidden);
        annots(&mut editor, vec![checkbox, hidden]);
        let bytes = saved(&editor);

        let over = over_secret_at(40.0);
        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 36, "six streams, SECRET from each");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let reopened = CosDocument::open(after.clone()).expect("it reopens");
        let streams = all_streams(&reopened);
        assert!(!streams.contains("SECRET"), "{streams}");
        assert_eq!(forms_in(&reopened), 6, "each cut in place, no copies");

        // Ticked: the state that was not drawn when the page was redacted.
        let mut editor = DocumentEditor::new(open(after));
        let page_ref = editor.page_refs()[0];
        let Some(Object::Dict(page)) = editor.get(page_ref) else {
            panic!("the page is a dictionary");
        };
        let checkbox = Resolve::resolve_key(&editor, &page, editor.intern(b"Annots"))
            .as_array()
            .and_then(|a| a.first().and_then(Object::as_objref))
            .expect("the checkbox is an object");
        let Some(Object::Dict(mut dict)) = editor.get(checkbox) else {
            panic!("the checkbox is a dictionary");
        };
        dict.insert(editor.intern(b"AS"), Object::Name(editor.intern(b"On")));
        editor.put(checkbox, Object::Dict(dict));
        let ticked = render(saved(&editor));
        assert_eq!(ink_in(&ticked, 300.0, over), 0, "the on state is cut too");
        assert!(
            ink_in(&ticked, 300.0, area(11.0, 46.0, 51.0, 53.0)) > 20,
            "and still draws PUBLIC"
        );
    }

    /// One appearance stream, and one `/AP` dictionary, shared by two
    /// annotations — one under the band and one not.
    ///
    /// Both are fitted onto `/Rect`s of the same size, at y 40 and y 190, so
    /// they are one form at two placements and are cut exactly at each: the
    /// covered annotation is given a copy, cut, through an `/AP` of its own,
    /// and the other keeps the shared dictionary and the stream exactly as
    /// they were. Writing the copy into the shared `/AP` would cut both.
    #[test]
    fn an_appearance_two_annotations_share_is_cut_only_where_it_is_covered() {
        let (doc, font) = page();
        let mut editor = DocumentEditor::new(doc);
        let stream = public_secret(&mut editor, font);
        let shared_ap = editor.allocate();
        let ap = normal(&editor, stream);
        editor.put(shared_ap, ap);
        let lower = annotation(&editor, b"FreeText", rect_at(40.0), Object::Ref(shared_ap));
        let upper = annotation(&editor, b"FreeText", rect_at(190.0), Object::Ref(shared_ap));
        let lower = object(&mut editor, lower);
        let upper = object(&mut editor, upper);
        annots(&mut editor, vec![lower, upper]);
        let bytes = saved(&editor);

        let over = over_secret_at(40.0);
        let before = render(bytes.clone());
        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 6, "one placement's SECRET");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        assert_eq!(
            flattened_lines(after.clone()),
            vec![
                (45.0, "PUBLIC".to_string()),
                (195.0, "PUBLIC SECRET".to_string()),
                (280.0, "KEEP".to_string())
            ],
            "each annotation lost what its own placement put under the band"
        );
        let rendered = render(after.clone());
        assert_eq!(ink_in(&rendered, 300.0, over), 0, "no ink under the band");
        assert_eq!(
            differing_outside(&before, &rendered, 300.0, area(55.0, 40.0, 101.0, 60.0)),
            0,
            "the upper annotation, and the lower's PUBLIC, render as they did"
        );

        let reopened = CosDocument::open(after).expect("it reopens");
        let (lower, upper) = (annotation_at(&reopened, 0), annotation_at(&reopened, 1));
        let (cut, whole) = (normal_of(&reopened, &lower), normal_of(&reopened, &upper));
        assert_ne!(cut, whole, "the covered annotation draws a copy");
        let text = |r: ObjRef| {
            String::from_utf8_lossy(&reopened.stream_decoded(r).expect("it decodes")).into_owned()
        };
        assert!(text(whole).contains("PUBLIC SECRET"), "{}", text(whole));
        assert!(
            text(cut).contains("PUBLIC") && !text(cut).contains("SECRET"),
            "{}",
            text(cut)
        );
        assert!(
            matches!(upper.get(reopened.intern(b"AP")), Some(Object::Ref(_))),
            "the uncovered annotation still names the shared /AP"
        );
    }

    /// The covered annotation is written **into** `/Annots` rather than as an
    /// object, and shares its appearance with one that is not covered. It is
    /// pointed at its copy where it sits, in the page's own array.
    #[test]
    fn an_annotation_written_into_annots_itself_is_pointed_at_its_copy() {
        let (doc, font) = page();
        let mut editor = DocumentEditor::new(doc);
        let stream = public_secret(&mut editor, font);
        let ap = normal(&editor, stream);
        let lower = annotation(&editor, b"FreeText", rect_at(40.0), ap.clone());
        let upper = annotation(&editor, b"FreeText", rect_at(190.0), ap);
        let upper = object(&mut editor, upper);
        annots(&mut editor, vec![Object::Dict(lower), upper]);
        let bytes = saved(&editor);

        let over = over_secret_at(40.0);
        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 6);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let rendered = render(after.clone());
        assert_eq!(ink_in(&rendered, 300.0, over), 0, "no ink under the band");
        assert!(
            ink_in(&rendered, 300.0, area(57.0, 196.0, 99.0, 203.0)) > 20,
            "the upper annotation still draws SECRET"
        );
        let reopened = CosDocument::open(after).expect("it reopens");
        let (lower, upper) = (annotation_at(&reopened, 0), annotation_at(&reopened, 1));
        assert_ne!(normal_of(&reopened, &lower), normal_of(&reopened, &upper));
    }

    /// A rectangle that covers no appearance writes no appearance: every
    /// stream, every `/AP` and every annotation is the object it was, and
    /// nothing is copied.
    #[test]
    fn an_appearance_no_rectangle_touches_is_left_as_it_was() {
        let (doc, font) = page();
        let mut editor = DocumentEditor::new(doc);
        let stream = public_secret(&mut editor, font);
        let ap = normal(&editor, stream);
        let note = annotation(&editor, b"FreeText", rect_at(40.0), ap);
        let note = object(&mut editor, note);
        annots(&mut editor, vec![note]);
        let bytes = saved(&editor);
        let original = open(bytes.clone());
        let was = annotation_at(&original, 0);

        let mut editor = DocumentEditor::new(Arc::clone(&original));
        let report =
            apply(&mut editor, 0, &[band(area(0.0, 100.0, 400.0, 150.0))]).expect("page zero");
        assert_eq!(report, RedactionReport::default());
        let reopened = CosDocument::open(saved(&editor)).expect("it reopens");
        let is = annotation_at(&reopened, 0);
        let (before, after) = (normal_of(&original, &was), normal_of(&reopened, &is));
        assert_eq!(
            original.stream_decoded(before).expect("it decodes"),
            reopened.stream_decoded(after).expect("it decodes"),
            "the appearance is byte for byte what it was"
        );
        assert_eq!(forms_in(&reopened), 1, "and was not copied");
    }

    /// An appearance with no `/Resources` of its own names its font in the
    /// page's (8.10.1, as the subsetter reads an appearance too), and is
    /// measured there — not left as a run in a font no scope has.
    #[test]
    fn an_appearance_without_resources_is_measured_in_the_pages() {
        let (doc, font) = page();
        let mut editor = DocumentEditor::new(doc);
        let stream = public_secret(&mut editor, font);
        let Some(Object::Dict(dict)) = editor.get(stream) else {
            panic!("the appearance has a dictionary");
        };
        let dict: Dict = dict
            .iter()
            .filter(|(key, _)| *key != Name::RESOURCES)
            .cloned()
            .collect();
        let data = editor
            .stream_bytes(stream)
            .expect("the appearance's content");
        editor.put_stream(stream, StreamData { dict, data });
        let ap = normal(&editor, stream);
        let note = annotation(&editor, b"FreeText", rect_at(40.0), ap);
        let note = object(&mut editor, note);
        annots(&mut editor, vec![note]);

        let (after, report) = redact(open(saved(&editor)), &[band(over_secret_at(40.0))]);
        assert_eq!(report.glyphs, 6);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let streams = all_streams(&CosDocument::open(after).expect("it reopens"));
        assert!(
            streams.contains("PUBLIC") && !streams.contains("SECRET"),
            "{streams}"
        );
    }
}

/// Type 3 glyph procedures (9.6.5) that draw text or an image, measured at
/// each use and removed at the use that draws under a rectangle.
///
/// The fixture's face, `/T3`, has glyphs one point wide at `1 Tf` whose
/// procedures draw far outside that point: `A` shows `SECRET` and `B`
/// `PUBLIC` in 12-point Liberation Serif from the glyph's origin, `C` draws
/// a twelve-point square as an inline image, `D` draws `/Fm0` — a form
/// showing `SECRET` — `E` shows its own glyph twice, and `F` shows text in a
/// font no scope has. So a rectangle over a procedure's text, clear of the
/// glyph's own one-point box, is a rectangle only the procedure's
/// measurement can see, and every test here puts one there.
///
/// The extractor reads a Type 3 glyph by running its procedure, so what it
/// reads back is the procedure's text: this engine's extractor and renderer,
/// compared with themselves before and after.
#[cfg(test)]
mod glyph_procedures {
    use super::tests_support::*;
    use super::*;

    fn area(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    fn band(area: Rect) -> Redaction {
        Redaction { area, mark: false }
    }

    /// Each glyph's procedure, by its name in `/CharProcs`, in code order
    /// from 65 (`A`).
    const PROCEDURES: [(&str, &[u8]); 6] = [
        ("secret", b"1000 0 d0 BT /F0 12000 Tf 0 0 Td (SECRET) Tj ET"),
        ("public", b"1000 0 d0 BT /F0 12000 Tf 0 0 Td (PUBLIC) Tj ET"),
        (
            "square",
            b"1000 0 d0 q 12000 0 0 12000 0 0 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI Q",
        ),
        ("form", b"1000 0 d0 /Fm0 Do"),
        ("itself", b"1000 0 d0 BT /T3 1000 Tf (EE) Tj ET"),
        ("lost", b"1000 0 d0 BT /Nowhere 12000 Tf (SECRET) Tj ET"),
    ];

    fn stream(editor: &mut DocumentEditor, dict: Dict, data: &[u8]) -> ObjRef {
        let reference = editor.allocate();
        editor.put_stream(
            reference,
            StreamData {
                dict,
                data: data.to_vec(),
            },
        );
        reference
    }

    /// A 400 by 300 page drawing `content` with `/T3` in scope, and `/F0`
    /// (Liberation Serif, embedded whole) and `/Fm0` beside it — in the
    /// page's scope, which is where this engine runs a procedure, and in the
    /// Type 3 font's own `/Resources`, which is where 9.6.5 says to look.
    fn document(content: &str) -> Vec<u8> {
        let mut builder = tinker_pdf_cos::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(
            b"F0",
            b"LiberationSerif",
            &crate::subset::tests_support::face()
        ));
        builder.add_page(400.0, 300.0, |p| p.text(b"F0", 12.0, 300.0, 280.0, "KEEP"));
        let doc = open(builder.finish());
        let font = crate::subset::tests_support::only_font(&doc);
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let name = |editor: &DocumentEditor, n: &str| editor.intern(n.as_bytes());

        let mut fonts = Dict::new();
        fonts.insert(name(&editor, "F0"), Object::Ref(font));
        let mut form_resources = Dict::new();
        form_resources.insert(name(&editor, "Font"), Object::Dict(fonts.clone()));
        let mut form = Dict::new();
        form.insert(
            name(&editor, "Subtype"),
            Object::Name(name(&editor, "Form")),
        );
        form.insert(
            name(&editor, "BBox"),
            Object::Array(
                [0, 0, 60000, 15000]
                    .iter()
                    .map(|v| Object::Int(*v))
                    .collect(),
            ),
        );
        form.insert(Name::RESOURCES, Object::Dict(form_resources));
        let form = stream(&mut editor, form, b"BT /F0 12000 Tf 0 0 Td (SECRET) Tj ET");

        let mut procs = Dict::new();
        let mut differences = vec![Object::Int(65)];
        for (glyph, body) in PROCEDURES {
            let procedure = stream(&mut editor, Dict::new(), body);
            procs.insert(name(&editor, glyph), Object::Ref(procedure));
            differences.push(Object::Name(name(&editor, glyph)));
        }
        let mut xobjects = Dict::new();
        xobjects.insert(name(&editor, "Fm0"), Object::Ref(form));
        let mut own = Dict::new();
        own.insert(name(&editor, "Font"), Object::Dict(fonts.clone()));
        own.insert(name(&editor, "XObject"), Object::Dict(xobjects.clone()));

        let mut encoding = Dict::new();
        encoding.insert(name(&editor, "Differences"), Object::Array(differences));
        let mut type3 = Dict::new();
        for (key, value) in [
            ("Type", Object::Name(name(&editor, "Font"))),
            ("Subtype", Object::Name(name(&editor, "Type3"))),
            ("FontMatrix", {
                let m = [0.001, 0.0, 0.0, 0.001, 0.0, 0.0];
                Object::Array(m.iter().map(|v| Object::Real(*v)).collect())
            }),
            (
                "FontBBox",
                Object::Array([0, 0, 1000, 1000].iter().map(|v| Object::Int(*v)).collect()),
            ),
            ("CharProcs", Object::Dict(procs)),
            ("Encoding", Object::Dict(encoding)),
            ("FirstChar", Object::Int(65)),
            ("LastChar", Object::Int(70)),
            ("Widths", Object::Array(vec![Object::Int(1000); 6])),
            ("Resources", Object::Dict(own)),
        ] {
            type3.insert(name(&editor, key), value);
        }
        let type3_ref = editor.allocate();
        editor.put(type3_ref, Object::Dict(type3));

        let mut page_fonts = fonts;
        page_fonts.insert(name(&editor, "T3"), Object::Ref(type3_ref));
        let mut resources = Dict::new();
        resources.insert(name(&editor, "Font"), Object::Dict(page_fonts));
        resources.insert(name(&editor, "XObject"), Object::Dict(xobjects));
        let content = stream(&mut editor, Dict::new(), content.as_bytes());
        let page_ref = editor.page_refs()[0];
        let Some(Object::Dict(mut page)) = editor.get(page_ref) else {
            panic!("the page is a dictionary");
        };
        page.insert(Name::RESOURCES, Object::Dict(resources));
        page.insert(Name::CONTENTS, Object::Ref(content));
        editor.put(page_ref, Object::Dict(page));
        editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            ..tinker_pdf_cos::WriteOptions::default()
        })
    }

    /// The decoded procedure `/CharProcs` names `glyph` by.
    fn procedure(doc: &CosDocument, glyph: &str) -> Vec<u8> {
        for (_, dict) in crate::subset::tests_support::font_dicts(doc) {
            let procs = doc.resolve_key(&dict, doc.intern(b"CharProcs"));
            if let Some(r) = procs
                .as_dict()
                .and_then(|p| p.get_ref(doc.intern(glyph.as_bytes())))
            {
                return doc.stream_decoded(r).expect("the procedure decodes");
            }
        }
        panic!("no procedure is named {glyph}");
    }

    /// The row's exit, for a glyph procedure: `A`, whose procedure shows
    /// `SECRET`, at y 125 and again at y 200, and `B`, whose procedure shows
    /// `PUBLIC`, at y 50; a band over the middle `SECRET` from x 20, ten
    /// points clear of the glyph's own box (x 10..11).
    ///
    /// That use is removed — one glyph, gone from the extractor and the
    /// render — and the other use of the same glyph, and `B`, are not. The
    /// procedure, which both uses of `A` run, is byte for byte what it was:
    /// the decision in [`cut_stream`]'s doc, pinned.
    #[test]
    fn a_glyph_whose_procedure_shows_text_under_a_rectangle_is_removed_at_that_use() {
        let bytes = document("BT /T3 1 Tf 10 50 Td (B) Tj 0 75 Td (A) Tj 0 75 Td (A) Tj ET");
        assert_eq!(
            lines_of(bytes.clone()),
            vec![
                (50.0, "PUBLIC".to_string()),
                (125.0, "SECRET".to_string()),
                (200.0, "SECRET".to_string()),
            ],
            "the fixture draws what its comment says, where it says"
        );
        let over = area(20.0, 120.0, 80.0, 140.0);
        let before = render(bytes.clone());
        assert!(ink_in(&before, 300.0, over) > 20, "SECRET starts inked");

        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 1, "the one use of A under the band");
        assert_eq!(report.operations, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        assert_eq!(
            lines_of(after.clone()),
            vec![(50.0, "PUBLIC".to_string()), (200.0, "SECRET".to_string())]
        );
        let rendered = render(after.clone());
        assert_eq!(ink_in(&rendered, 300.0, over), 0, "no ink under the band");
        assert_eq!(
            differing_outside(&before, &rendered, 300.0, area(9.0, 120.0, 60.0, 140.0)),
            0,
            "the other SECRET and PUBLIC render exactly as they did"
        );
        let reopened = CosDocument::open(after).expect("it reopens");
        assert_eq!(
            procedure(&reopened, "secret"),
            PROCEDURES[0].1,
            "the procedure every use of A runs is left as it was"
        );
    }

    /// `C`'s procedure draws a twelve-point square as an inline image, the
    /// way a bitmap face draws every glyph. Two uses in one `TJ`, the second
    /// moved a hundred points along by the array's number, so it is a pen
    /// position along the run rather than a line start; the band takes the
    /// corner of the second square, not the glyph's own box (x 110..111).
    /// That use goes, and the first square — the same procedure — stays.
    #[test]
    fn a_glyph_whose_procedure_draws_an_image_under_a_rectangle_is_removed_at_that_use() {
        let bytes = document("BT /T3 1 Tf 10 50 Td [(C) -99000 (C)] TJ ET");
        let over = area(116.0, 55.0, 130.0, 70.0);
        let second = area(109.0, 49.0, 123.0, 63.0);
        let before = render(bytes.clone());
        assert!(ink_in(&before, 300.0, over) > 20, "the square starts inked");
        assert!(
            ink_in(&before, 300.0, area(9.0, 49.0, 23.0, 63.0)) > 100,
            "and so does the first"
        );

        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 1);
        assert_eq!(report.images, 0, "nothing was scrubbed: a use was removed");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let rendered = render(after);
        assert_eq!(ink_in(&rendered, 300.0, second), 0);
        assert_eq!(
            differing_outside(&before, &rendered, 300.0, second),
            0,
            "the first square renders exactly as it did"
        );
    }

    /// Forty uses of `D`, each of whose measurements runs two streams — the
    /// procedure and the form it draws — and a band nowhere near any of
    /// them. Nothing is removed: the budget is each use's, so a page of an
    /// ordinary two-level face does not run a shared one out a few dozen
    /// glyphs in and remove every use after.
    #[test]
    fn every_use_of_a_glyph_has_a_budget_of_its_own() {
        let uses = "D".repeat(40);
        let bytes = document(&format!("BT /T3 1 Tf 10 50 Td ({uses}) Tj ET"));
        let (_, report) = redact(open(bytes), &[band(area(300.0, 0.0, 400.0, 10.0))]);
        assert_eq!(report, RedactionReport::default());
    }

    /// `D`'s procedure draws a form, and the form shows `SECRET`: measured
    /// through the form, under the transform the procedure draws it with.
    #[test]
    fn a_glyph_whose_procedure_draws_a_form_is_measured_through_the_form() {
        let bytes = document("BT /T3 1 Tf 10 50 Td (D) Tj 0 150 Td (D) Tj ET");
        assert_eq!(
            lines_of(bytes.clone()),
            vec![(50.0, "SECRET".to_string()), (200.0, "SECRET".to_string())]
        );
        let over = area(20.0, 45.0, 80.0, 65.0);
        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(lines_of(after.clone()), vec![(200.0, "SECRET".to_string())]);
        assert_eq!(ink_in(&render(after), 300.0, over), 0);
    }

    /// `E`'s procedure shows `E` twice, so measuring it never bottoms out.
    /// It ends — the budget is spent — and the use is removed, which is the
    /// direction a measurement that could not finish errs in. A face that
    /// draws itself is not one a reader is looking at.
    #[test]
    fn a_glyph_procedure_that_shows_its_own_glyph_ends_and_errs_toward_removal() {
        let bytes = document("BT /T3 1 Tf 10 50 Td (E) Tj 100 0 Td (B) Tj ET");
        let far = area(300.0, 0.0, 400.0, 10.0);
        let (after, report) = redact(open(bytes), &[band(far)]);
        assert_eq!(report.glyphs, 1, "E, and not B");
        assert_eq!(
            lines_of(after.clone()),
            vec![(50.0, "PUBLIC".to_string())],
            "B's PUBLIC is still read"
        );
        let reopened = CosDocument::open(after).expect("it reopens");
        assert_eq!(procedure(&reopened, "itself"), PROCEDURES[4].1);
    }

    /// `F`'s procedure shows text in `/Nowhere`, which no scope has: its run
    /// cannot be measured, so it is named — under the name the procedure
    /// gave — and the glyph is left, as every unmeasurable run is. The
    /// renderer draws nothing for it either.
    #[test]
    fn a_procedure_showing_text_in_a_font_no_scope_has_is_reported_and_left() {
        let bytes = document("BT /T3 1 Tf 10 50 Td (F) Tj ET");
        let (_, report) = redact(open(bytes), &[band(area(20.0, 45.0, 80.0, 65.0))]);
        assert_eq!(report.glyphs, 0);
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::UnknownFont {
                font: b"Nowhere".to_vec(),
                bytes: 6,
            }]
        );
    }

    /// The decision's cost, paid by the default save: the procedure a
    /// redaction leaves in the font is emptied once the redaction removed its
    /// last use. `A` is shown once, under the band, and `B` once beside it;
    /// after [`crate::write::save`] with its defaults `A`'s procedure is the
    /// empty one and no longer says `SECRET`, `B`'s still says `PUBLIC`, and
    /// the page reads as the redaction left it.
    #[test]
    fn a_procedure_whose_last_use_was_redacted_is_emptied_by_the_default_save() {
        let bytes = document("BT /T3 1 Tf 10 50 Td (B) Tj 0 75 Td (A) Tj ET");
        let mut editor = DocumentEditor::new(open(bytes));
        let report = apply(&mut editor, 0, &[band(area(20.0, 120.0, 80.0, 140.0))])
            .expect("the page exists");
        assert_eq!(report.glyphs, 1);
        let saved = crate::write::save(&mut editor, &crate::SaveOptions::default());
        let subset = saved.fonts.report().expect("the pass ran");
        assert_eq!(
            subset.type3.iter().map(|t| t.kept).collect::<Vec<_>>(),
            vec![1],
            "of the six procedures, B's alone is still run"
        );

        let reopened = CosDocument::open(saved.bytes.clone()).expect("it reopens");
        assert_eq!(procedure(&reopened, "secret").trim_ascii_end(), b"0 0 d0");
        assert_eq!(procedure(&reopened, "public"), PROCEDURES[1].1);
        assert_eq!(lines_of(saved.bytes), vec![(50.0, "PUBLIC".to_string())]);
    }

    /// The procedure's measurement composes with every transform above it: a
    /// use of `A` inside a form placed twice, and in an annotation's
    /// appearance, is measured where each draws it — the walk's placements,
    /// with a procedure under each.
    #[test]
    fn a_glyph_in_a_form_drawn_twice_is_measured_at_each_placement() {
        let mut editor = DocumentEditor::new(open(document("")));
        let page_ref = editor.page_refs()[0];
        let Some(Object::Dict(mut page)) = editor.get(page_ref) else {
            panic!("the page is a dictionary");
        };
        let resources = Resolve::resolve_key(&editor, &page, Name::RESOURCES)
            .as_dict()
            .cloned()
            .expect("resources");
        let mut form = Dict::new();
        form.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(b"Form")),
        );
        form.insert(
            editor.intern(b"BBox"),
            Object::Array([0, 0, 400, 100].iter().map(|v| Object::Int(*v)).collect()),
        );
        form.insert(Name::RESOURCES, Object::Dict(resources.clone()));
        let form_ref = stream(&mut editor, form, b"BT /T3 1 Tf 10 50 Td (A) Tj ET");
        let mut xobjects = Resolve::resolve_key(&editor, &resources, editor.intern(b"XObject"))
            .as_dict()
            .cloned()
            .expect("an /XObject");
        xobjects.insert(editor.intern(b"Twice"), Object::Ref(form_ref));
        let mut resources = resources;
        resources.insert(editor.intern(b"XObject"), Object::Dict(xobjects));
        page.insert(Name::RESOURCES, Object::Dict(resources));
        let content = stream(
            &mut editor,
            Dict::new(),
            b"/Twice Do q 1 0 0 1 0 150 cm /Twice Do Q",
        );
        page.insert(Name::CONTENTS, Object::Ref(content));
        editor.put(page_ref, Object::Dict(page));
        let bytes = editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            ..tinker_pdf_cos::WriteOptions::default()
        });
        assert_eq!(
            lines_of(bytes.clone()),
            vec![(50.0, "SECRET".to_string()), (200.0, "SECRET".to_string())]
        );

        let over = area(20.0, 195.0, 80.0, 215.0);
        let (after, report) = redact(open(bytes), &[band(over)]);
        assert_eq!(report.glyphs, 1, "the upper placement's A");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(lines_of(after), vec![(50.0, "SECRET".to_string())]);
    }
}

/// A form the redacted page shares with something else, cut at **every**
/// placement on the redacted page.
///
/// Every fixture is two pages over one form, `/Fm0`, drawing `PUBLIC SECRET`
/// in the vendored Liberation Serif at y 50 (the form of
/// [`tests_support::public_secret_drawn_by`]). Page one draws it at y 50 and
/// at y 200, and the two rectangles take `SECRET` from the lower placement
/// and `PUBLIC` from the upper, so no placement on page one is uncut. Page
/// two draws the form some other way, or only names it.
///
/// Until October 2026 the form's own object took page one's first outcome
/// whatever else drew it, and page two lost `SECRET` to a rectangle on page
/// one, unreported.
#[cfg(test)]
mod forms_elsewhere {
    use super::tests_support::*;
    use super::*;

    fn band(x0: f64, y0: f64, x1: f64, y1: f64) -> Redaction {
        Redaction {
            area: Rect { x0, y0, x1, y1 },
            mark: false,
        }
    }

    /// `SECRET` from the lower placement, `PUBLIC` from the upper.
    fn bands() -> [Redaction; 2] {
        [band(56.0, 45.0, 400.0, 70.0), band(0.0, 195.0, 52.0, 220.0)]
    }

    const PAGE_ONE: &[u8] = b"q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q";

    /// Two pages: `/Fm0`, then `/Fm1` drawing `/Fm0` (registered after it, so
    /// its resources name it), page one as above, and page two drawing
    /// `page_two`.
    fn two_pages(page_two: &[u8]) -> Vec<u8> {
        let mut builder = tinker_pdf_cos::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(
            b"F0",
            b"LiberationSerif",
            &crate::subset::tests_support::face()
        ));
        for (name, content) in [
            (
                b"Fm0".as_slice(),
                b"BT /F0 12 Tf 10 50 Td (PUBLIC SECRET) Tj ET".as_slice(),
            ),
            (b"Fm1", b"/Fm0 Do"),
        ] {
            assert!(builder.add_form(
                name,
                &tinker_pdf_cos::FormXObject {
                    bbox: [0.0, 0.0, 400.0, 300.0],
                    matrix: None,
                    group: None,
                    content,
                }
            ));
        }
        builder.add_page(400.0, 300.0, |p| p.raw(PAGE_ONE));
        builder.add_page(400.0, 300.0, |p| p.raw(page_two));
        builder.finish()
    }

    /// Page `index`, rendered with its annotations.
    fn render_page(bytes: Vec<u8>, index: u32) -> crate::Bitmap {
        crate::Document::open(bytes)
            .expect("it reopens")
            .page(index)
            .expect("the page")
            .render(&crate::RenderOptions::default())
    }

    /// Page one lost exactly what its rectangles covered, at each placement.
    fn page_one_is_cut_exactly(bytes: &[u8], report: &RedactionReport) {
        assert_eq!(report.glyphs, 12, "SECRET below and PUBLIC above");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(
            lines_of(bytes.to_vec()),
            vec![(50.0, "PUBLIC".to_string()), (200.0, "SECRET".to_string())]
        );
        let rendered = render_page(bytes.to_vec(), 0);
        for redaction in bands() {
            assert_eq!(ink_in(&rendered, 300.0, redaction.area), 0);
        }
    }

    fn saved(editor: &DocumentEditor) -> Vec<u8> {
        editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            ..tinker_pdf_cos::WriteOptions::default()
        })
    }

    /// The row's exit: page two draws `/Fm0` itself, and still draws all of
    /// it. The form's own object is left as it was — page two is what draws
    /// it now — and each of page one's placements draws a copy of its own.
    #[test]
    fn a_form_another_page_draws_is_left_whole_when_every_placement_here_is_cut() {
        let bytes = two_pages(b"/Fm0 Do");
        let before = render_page(bytes.clone(), 1);
        let (after, report) = redact(open(bytes), &bands());
        page_one_is_cut_exactly(&after, &report);

        assert_eq!(
            lines_on(after.clone(), 1),
            vec![(50.0, "PUBLIC SECRET".to_string())],
            "page two was not redacted and lost nothing"
        );
        assert_eq!(
            differing_outside(
                &before,
                &render_page(after.clone(), 1),
                300.0,
                Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 0.0,
                    y1: 0.0
                }
            ),
            0,
            "page two renders exactly as it did"
        );
        let reopened = CosDocument::open(after).expect("it reopens");
        assert_eq!(
            forms_in(&reopened),
            4,
            "Fm0 and Fm1 as they were, and a copy for each placement on page one"
        );
    }

    /// Page two draws `/Fm0` only through `/Fm1`: drawn at a depth is drawn.
    #[test]
    fn a_form_another_page_draws_through_a_form_of_its_own_is_left_whole() {
        let (after, report) = redact(open(two_pages(b"/Fm1 Do")), &bands());
        page_one_is_cut_exactly(&after, &report);
        assert_eq!(
            lines_on(after, 1),
            vec![(50.0, "PUBLIC SECRET".to_string())]
        );
    }

    /// Page two draws `/Fm0` as an annotation's appearance, not from its
    /// content: 12.5.5 draws it there all the same.
    #[test]
    fn a_form_an_annotation_on_another_page_shows_is_left_whole() {
        let bytes = two_pages(b"");
        let mut editor = DocumentEditor::new(open(bytes));
        let page_two = editor.page_refs()[1];
        let Some(Object::Dict(page)) = editor.get(page_two) else {
            panic!("page two is a dictionary");
        };
        let resources = inherited_resources(&editor, &page);
        let (form, _) = resolve_xobject(&editor, &resources, b"Fm0").expect("Fm0 is in scope");
        let mut ap = Dict::new();
        ap.insert(editor.intern(b"N"), Object::Ref(form));
        let mut annotation = Dict::new();
        annotation.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(b"Stamp")),
        );
        annotation.insert(
            editor.intern(b"Rect"),
            Object::Array([0, 0, 400, 300].iter().map(|v| Object::Int(*v)).collect()),
        );
        annotation.insert(editor.intern(b"AP"), Object::Dict(ap));
        let mut page = page;
        page.insert(
            editor.intern(b"Annots"),
            Object::Array(vec![Object::Dict(annotation)]),
        );
        editor.put(page_two, Object::Dict(page));
        let bytes = saved(&editor);
        let before = render_page(bytes.clone(), 1);
        assert!(
            ink_in(
                &before,
                300.0,
                Rect {
                    x0: 57.0,
                    y0: 51.0,
                    x1: 99.0,
                    y1: 58.0
                }
            ) > 20,
            "page two's stamp draws SECRET"
        );

        let (after, report) = redact(open(bytes), &bands());
        page_one_is_cut_exactly(&after, &report);
        assert_eq!(
            differing_outside(
                &before,
                &render_page(after, 1),
                300.0,
                Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 0.0,
                    y1: 0.0
                }
            ),
            0,
            "page two's stamp renders exactly as it did"
        );
    }

    /// Page two draws `/Fm0` from a Type 3 glyph's procedure, which runs it
    /// in page two's scope: a glyph shown is a form drawn.
    #[test]
    fn a_form_a_glyph_procedure_on_another_page_draws_is_left_whole() {
        let bytes = two_pages(b"");
        let mut editor = DocumentEditor::new(open(bytes));
        let page_two = editor.page_refs()[1];
        let Some(Object::Dict(mut page)) = editor.get(page_two) else {
            panic!("page two is a dictionary");
        };
        let procedure = editor.allocate();
        editor.put_stream(
            procedure,
            StreamData {
                dict: Dict::new(),
                data: b"1000 0 d0 1000 0 0 1000 0 0 cm /Fm0 Do".to_vec(),
            },
        );
        let mut face = Dict::new();
        let mut procs = Dict::new();
        procs.insert(editor.intern(b"a"), Object::Ref(procedure));
        let mut encoding = Dict::new();
        encoding.insert(
            editor.intern(b"Differences"),
            Object::Array(vec![Object::Int(65), Object::Name(editor.intern(b"a"))]),
        );
        for (key, value) in [
            (b"Type".as_slice(), Object::Name(editor.intern(b"Font"))),
            (b"Subtype", Object::Name(editor.intern(b"Type3"))),
            (
                b"FontMatrix",
                Object::Array(
                    [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]
                        .iter()
                        .map(|v| Object::Real(*v))
                        .collect(),
                ),
            ),
            (
                b"FontBBox",
                Object::Array([0, 0, 1000, 1000].iter().map(|v| Object::Int(*v)).collect()),
            ),
            (b"CharProcs", Object::Dict(procs)),
            (b"Encoding", Object::Dict(encoding)),
            (b"FirstChar", Object::Int(65)),
            (b"LastChar", Object::Int(65)),
            (b"Widths", Object::Array(vec![Object::Int(1000)])),
        ] {
            face.insert(editor.intern(key), value);
        }
        let face_ref = editor.allocate();
        editor.put(face_ref, Object::Dict(face));

        let mut resources = inherited_resources(&editor, &page);
        let mut fonts = Resolve::resolve_key(&editor, &resources, editor.intern(b"Font"))
            .as_dict()
            .cloned()
            .unwrap_or_default();
        fonts.insert(editor.intern(b"T3"), Object::Ref(face_ref));
        resources.insert(editor.intern(b"Font"), Object::Dict(fonts));
        page.insert(Name::RESOURCES, Object::Dict(resources));
        let content = editor.allocate();
        editor.put_stream(
            content,
            StreamData {
                dict: Dict::new(),
                data: b"BT /T3 1 Tf 0 0 Td (A) Tj ET".to_vec(),
            },
        );
        page.insert(Name::CONTENTS, Object::Ref(content));
        editor.put(page_two, Object::Dict(page));
        let bytes = saved(&editor);
        assert_eq!(
            lines_on(bytes.clone(), 1),
            vec![(50.0, "PUBLIC SECRET".to_string())],
            "page two's one glyph draws the form"
        );

        let (after, report) = redact(open(bytes), &bands());
        page_one_is_cut_exactly(&after, &report);
        assert_eq!(
            lines_on(after, 1),
            vec![(50.0, "PUBLIC SECRET".to_string())]
        );
    }

    /// Page two's resources **name** `/Fm0` and its content never draws it.
    /// Named is not drawn: the form's own object takes page one's first
    /// outcome, as it does when nothing else names it, so the uncut text is
    /// in no stream — leaving it whole for a page that never draws it would
    /// have left it in the file with nothing drawing it.
    #[test]
    fn a_form_another_page_only_names_is_still_cut_in_place() {
        let bytes = two_pages(b"");
        let doc = open(bytes.clone());
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let page_two = editor.page_refs()[1];
        let Some(Object::Dict(page)) = editor.get(page_two) else {
            panic!("page two is a dictionary");
        };
        let resources = inherited_resources(&editor, &page);
        assert!(
            resolve_xobject(&editor, &resources, b"Fm0").is_some(),
            "the fixture: page two's resources name the form"
        );

        let report = apply(&mut editor, 0, &bands()).expect("page one");
        let after = saved(&editor);
        page_one_is_cut_exactly(&after, &report);
        let reopened = CosDocument::open(after).expect("it reopens");
        let streams = all_streams(&reopened);
        assert!(
            !streams.contains("PUBLIC SECRET"),
            "no stream holds the uncut text: {streams}"
        );
        assert_eq!(forms_in(&reopened), 3, "Fm0, Fm1 and one copy");
    }
}

/// [`MAX_XOBJECT_USES`], at the line and one past it.
#[cfg(test)]
mod xobject_cap {
    use super::tests_support::*;
    use super::*;

    /// A page whose content draws `/Im0` — two by two gray samples spelling
    /// `SECR` — `placements` times: every placement but the last at
    /// (300, 300), clear of the rectangle, and the last at the origin, under
    /// it.
    fn many(placements: usize) -> Vec<u8> {
        let mut content = "q 10 0 0 10 300 300 cm /Im0 Do Q\n".repeat(placements - 1);
        content.push_str("q 10 0 0 10 0 0 cm /Im0 Do Q\n");
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400]\n\
             /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        out.push_str(&stream_object(4, &content));
        out.push_str(
            "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
             /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
             stream\nSECR\nendstream\nendobj\n",
        );
        out.push_str("trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    fn under() -> Redaction {
        Redaction {
            area: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 20.0,
                y1: 20.0,
            },
            mark: false,
        }
    }

    /// Exactly as many `Do`s as the walk follows: the last is followed, and
    /// the image it draws under the rectangle is scrubbed.
    #[test]
    fn as_many_xobjects_as_the_walk_follows_are_all_followed() {
        let (bytes, report) = redact(open(many(MAX_XOBJECT_USES)), &[under()]);
        assert_eq!(report.images, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(!all_streams(&open(bytes)).contains("SECR"));
    }

    /// One more, and the one past the cap is the one under the rectangle. It
    /// is not followed and the image is not scrubbed — and the report says
    /// so, where until October 2026 it read `images: 0` and nothing else.
    #[test]
    fn a_stream_of_more_xobjects_than_the_walk_follows_is_reported() {
        let (bytes, report) = redact(open(many(MAX_XOBJECT_USES + 1)), &[under()]);
        assert_eq!(report.images, 0, "the last Do was not followed");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::TooManyXObjects { skipped: 1 }]
        );
        assert!(
            all_streams(&open(bytes)).contains("SECR"),
            "which is what the warning is for"
        );
    }

    /// No rectangle, nothing to be uncertain about, as for every warning.
    #[test]
    fn the_cap_raises_nothing_when_there_is_no_rectangle() {
        let (_, report) = redact(open(many(MAX_XOBJECT_USES + 1)), &[]);
        assert_eq!(report, RedactionReport::default());
    }

    /// A form whose content is past the cap, drawn at two placements: each
    /// placement is a pass over the stream, each leaves one `Do` unfollowed,
    /// and the one warning sums them.
    #[test]
    fn a_form_past_the_cap_counts_at_each_placement() {
        let form = "/Im0 Do\n".repeat(MAX_XOBJECT_USES + 1);
        let page = "/Fm0 Do q 1 0 0 1 0 50 cm /Fm0 Do Q";
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400]\n\
             /Resources << /XObject << /Fm0 6 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        out.push_str(&stream_object(4, page));
        out.push_str(
            "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
             /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
             stream\nSECR\nendstream\nendobj\n",
        );
        out.push_str(&format!(
            "6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 400]\n\
             /Resources << /XObject << /Im0 5 0 R >> >> /Length {} >>\n\
             stream\n{form}\nendstream\nendobj\n",
            form.len() + 1
        ));
        out.push_str("trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");

        let (_, report) = redact(open(out.into_bytes()), &[under()]);
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::TooManyXObjects { skipped: 2 }]
        );
    }
}

/// Tiling patterns and soft masks: not read, and named when what they draw is
/// text or an image ([`RedactionWarning::PatternOrMask`]).
///
/// One page, in Helvetica: `/P0`, a tiling pattern whose cell shows `SECRET`,
/// and `/P1`, one whose cell is a filled square; `/GS0`, a graphics state
/// whose luminosity mask's group shows `SECRET`, `/GS1` setting
/// `/SMask /None`, and `/GS2` — by reference — a mask whose group is a filled
/// square; and `/Fm0`, a form with resources of its own that paints with its
/// own `/Q0`, the same cell as `/P0`.
#[cfg(test)]
mod patterns_and_masks {
    use super::tests_support::*;
    use super::*;

    fn document(content: &str) -> Vec<u8> {
        let text_cell = "BT /F0 12 Tf 0 5 Td (SECRET) Tj ET";
        let square = "0 0 25 10 re f";
        let mask_text = "BT /F0 48 Tf 10 10 Td (SECRET) Tj ET";
        let form = "/Pattern cs /Q0 scn 0 0 100 100 re f";
        let pattern = |number: u32, body: &str| {
            format!(
                "{number} 0 obj\n<< /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1\n\
                 /BBox [0 0 50 20] /XStep 50 /YStep 20 /Resources << /Font << /F0 6 0 R >> >>\n\
                 /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
                body.len() + 1
            )
        };
        let group = |number: u32, body: &str| {
            format!(
                "{number} 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 400]\n\
                 /Group << /S /Transparency /CS /DeviceGray >>\n\
                 /Resources << /Font << /F0 6 0 R >> >> /Length {} >>\n\
                 stream\n{body}\nendstream\nendobj\n",
                body.len() + 1
            )
        };
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400]\n\
             /Resources << /Font << /F0 6 0 R /T3 13 0 R >> /Pattern << /P0 7 0 R /P1 8 0 R >>\n\
             /ExtGState << /GS0 << /SMask << /S /Luminosity /G 9 0 R >> >>\n\
             /GS1 << /SMask /None >> /GS2 10 0 R >>\n\
             /XObject << /Fm0 11 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        out.push_str(&stream_object(4, content));
        out.push_str("6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
        out.push_str(&pattern(7, text_cell));
        out.push_str(&pattern(8, square));
        out.push_str(&group(9, mask_text));
        out.push_str(
            "10 0 obj\n<< /Type /ExtGState /SMask << /S /Luminosity /G 12 0 R >> >>\nendobj\n",
        );
        out.push_str(&format!(
            "11 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 400]\n\
             /Resources << /Pattern << /Q0 7 0 R >> >> /Length {} >>\n\
             stream\n{form}\nendstream\nendobj\n",
            form.len() + 1
        ));
        out.push_str(&group(12, square));
        out.push_str(
            "13 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
             /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /p 14 0 R /q 15 0 R >>\n\
             /Encoding << /Type /Encoding /Differences [65 /p /q] >>\n\
             /FirstChar 65 /LastChar 66 /Widths [1000 1000] >>\nendobj\n",
        );
        out.push_str(&stream_object(
            14,
            "1000 0 d0 /Pattern cs /P0 scn 0 0 1000 1000 re f",
        ));
        out.push_str(&stream_object(15, "1000 0 d0 /GS0 gs 0 0 1000 1000 re f"));
        out.push_str("trailer\n<< /Size 16 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    fn anywhere() -> Redaction {
        Redaction {
            area: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 400.0,
                y1: 400.0,
            },
            mark: false,
        }
    }

    fn warnings(content: &str, areas: &[Redaction]) -> Vec<RedactionWarning> {
        redact(open(document(content)), areas).1.warnings
    }

    fn named(resource: &[u8]) -> RedactionWarning {
        RedactionWarning::PatternOrMask {
            resource: resource.to_vec(),
        }
    }

    #[test]
    fn a_tiling_pattern_whose_cell_shows_text_is_named() {
        assert_eq!(
            warnings(
                "/Pattern cs /P0 scn 0 0 200 200 re f /P0 scn",
                &[anywhere()]
            ),
            vec![named(b"P0")],
            "once, however often it is painted with"
        );
    }

    #[test]
    fn a_stroking_pattern_is_named_too() {
        assert_eq!(
            warnings("/Pattern CS /P0 SCN 0 0 m 100 100 l S", &[anywhere()]),
            vec![named(b"P0")]
        );
    }

    #[test]
    fn a_tiling_pattern_of_paths_is_not_named() {
        assert_eq!(
            warnings("/Pattern cs /P1 scn 0 0 200 200 re f", &[anywhere()]),
            Vec::new()
        );
    }

    #[test]
    fn a_soft_mask_whose_group_shows_text_is_named() {
        assert_eq!(
            warnings("/GS0 gs 0 0 200 200 re f", &[anywhere()]),
            vec![named(b"GS0")]
        );
    }

    #[test]
    fn no_mask_and_a_mask_of_paths_are_not_named() {
        assert_eq!(
            warnings("/GS1 gs /GS2 gs 0 0 200 200 re f", &[anywhere()]),
            Vec::new()
        );
    }

    /// The name resolves in the resources of the stream that painted: the
    /// page has no `/Q0`, the form does.
    #[test]
    fn a_pattern_a_form_paints_with_is_named_in_the_forms_scope() {
        assert_eq!(warnings("/Fm0 Do", &[anywhere()]), vec![named(b"Q0")]);
    }

    #[test]
    fn what_a_glyph_procedure_paints_with_is_named() {
        // `/T3`'s `A` fills its em with `/P0` and its `B` under `/GS0`:
        // each measured as a procedure that might draw text, and what it
        // paints with named. The band is clear of the glyphs' own boxes,
        // which would otherwise remove them unmeasured.
        let clear = Redaction {
            area: Rect {
                x0: 200.0,
                y0: 200.0,
                x1: 400.0,
                y1: 400.0,
            },
            mark: false,
        };
        assert_eq!(
            warnings("BT /T3 10 Tf 10 10 Td (AB) Tj ET", &[clear]),
            vec![named(b"P0"), named(b"GS0")]
        );
    }

    #[test]
    fn nothing_is_named_without_a_rectangle() {
        assert_eq!(
            warnings("/Pattern cs /P0 scn /GS0 gs 0 0 200 200 re f", &[]),
            Vec::new()
        );
    }
}

/// Ruling 1 over everything a redaction now reads: annotation appearances
/// and their states, Type 3 glyph procedures that show text, draw a form or
/// paint with a pattern, forms drawn twice and on a second page, tiling
/// patterns and soft masks — in one small file, put through deterministic
/// damage and then redacted, subsetted and saved. Nothing is asserted but
/// that nothing panics; `tests/hostile_input.rs` does the same for the
/// reading surface, and this is the editing one.
#[cfg(test)]
mod hostile {
    use super::*;

    /// xorshift64, as `tests/hostile_input.rs` has it: the same damage on
    /// every machine (ruling 4).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, bound: usize) -> usize {
            if bound == 0 {
                0
            } else {
                (self.next() % bound as u64) as usize
            }
        }
    }

    fn stream(number: u32, dict: &str, body: &str) -> String {
        format!(
            "{number} 0 obj\n<< {dict} /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
            body.len() + 1
        )
    }

    /// Two pages over everything the walk and its reads follow.
    fn everything() -> Vec<u8> {
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 2 /Kids [3 0 R 20 0 R] >>\nendobj\n");
        let resources = "/Resources << /Font << /F0 5 0 R /T3 6 0 R >>\n\
             /XObject << /Fm0 9 0 R /Im0 10 0 R >> /Pattern << /P0 11 0 R >>\n\
             /ExtGState << /GS0 << /SMask << /S /Luminosity /G 12 0 R >> >> >> >>";
        out.push_str(&format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]\n{resources}\n\
             /Annots [13 0 R << /Subtype /FreeText /Rect [10 100 210 120]\n\
             /AP << /N 15 0 R >> >>] /Contents 4 0 R >>\nendobj\n"
        ));
        out.push_str(&stream(
            4,
            "",
            "q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q\n\
             BT /T3 1 Tf 10 50 Td (ABC) Tj 0 20 Td [(A) -5000 (B)] TJ ET\n\
             /Pattern cs /P0 scn /GS0 gs 0 0 50 50 re f\n\
             q 20 0 0 20 300 200 cm /Im0 Do Q\n\
             q 10 0 0 10 350 250 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x7f EI Q",
        ));
        out.push_str("5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
        out.push_str(
            "6 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
             /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /a 7 0 R /b 8 0 R /c 16 0 R >>\n\
             /Encoding << /Type /Encoding /Differences [65 /a /b /c] >>\n\
             /FirstChar 65 /LastChar 67 /Widths [1000 1000 1000]\n\
             /Resources << /Font << /F0 5 0 R >> >> >>\nendobj\n",
        );
        out.push_str(&stream(
            7,
            "",
            "1000 0 d0 BT /F0 12000 Tf 0 0 Td (SECRET) Tj ET",
        ));
        out.push_str(&stream(8, "", "1000 0 d0 /Fm0 Do /Pattern cs /P0 scn"));
        out.push_str(&stream(
            9,
            "/Type /XObject /Subtype /Form /BBox [0 0 400 300]\n\
             /Resources << /Font << /F0 5 0 R >> /XObject << /Im0 10 0 R >> >>",
            "BT /F0 12 Tf 10 60 Td (PUBLIC SECRET) Tj ET q 5 0 0 5 200 60 cm /Im0 Do Q",
        ));
        out.push_str(
            "10 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
             /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
             stream\nSECR\nendstream\nendobj\n",
        );
        out.push_str(&stream(
            11,
            "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 50 20]\n\
             /XStep 50 /YStep 20 /Resources << /Font << /F0 5 0 R >> >>",
            "BT /F0 12 Tf 0 5 Td (SECRET) Tj ET",
        ));
        out.push_str(&stream(
            12,
            "/Type /XObject /Subtype /Form /BBox [0 0 400 300] /Group << /S /Transparency >>\n\
             /Resources << /Font << /F0 5 0 R >> >>",
            "BT /F0 48 Tf 10 10 Td (SECRET) Tj ET",
        ));
        out.push_str(
            "13 0 obj\n<< /Type /Annot /Subtype /Widget /Rect [10 200 90 220] /AS /Off\n\
             /AP << /N << /On 14 0 R /Off 15 0 R >> /D 14 0 R >> /F 2 >>\nendobj\n",
        );
        out.push_str(&stream(
            14,
            "/Type /XObject /Subtype /Form /BBox [0 0 80 20] /Matrix [0 1 -1 0 0 0]\n\
             /Resources << /Font << /F0 5 0 R >> >>",
            "BT /F0 10 Tf 2 2 Td (SECRET) Tj ET /Fm0 Do",
        ));
        out.push_str(&stream(
            15,
            "/Type /XObject /Subtype /Form /BBox [0 0 400 40]\n\
             /Resources << /Font << /F0 5 0 R /T3 6 0 R >> /XObject << /Fm0 9 0 R >> >>",
            "BT /F0 24 Tf 0 10 Td (PUBLIC SECRET) Tj /T3 1000 Tf (A) Tj ET",
        ));
        out.push_str(&stream(16, "", "1000 0 d0 BT /T3 1000 Tf (CC) Tj ET"));
        out.push_str(&format!(
            "20 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]\n{resources}\n\
             /Contents 21 0 R >>\nendobj\n"
        ));
        out.push_str(&stream(21, "", "/Fm0 Do BT /T3 1 Tf 10 10 Td (B) Tj ET"));
        out.push_str("trailer\n<< /Size 22 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// One deterministic injury: flipped bytes, a deleted span, a repeated
    /// span, or a digit run replaced with a large or negative number.
    fn mutate(original: &[u8], rng: &mut Rng) -> Vec<u8> {
        let mut bytes = original.to_vec();
        match rng.below(4) {
            0 => {
                for _ in 0..1 + rng.below(8) {
                    let at = rng.below(bytes.len());
                    if let Some(b) = bytes.get_mut(at) {
                        *b ^= 1 << rng.below(8);
                    }
                }
            }
            1 => {
                let at = rng.below(bytes.len());
                let end = (at + 1 + rng.below(64)).min(bytes.len());
                bytes.drain(at..end);
            }
            2 => {
                let at = rng.below(bytes.len());
                let end = (at + 1 + rng.below(64)).min(bytes.len());
                let span: Vec<u8> = bytes.get(at..end).map(<[u8]>::to_vec).unwrap_or_default();
                let _ = bytes.splice(at..at, span);
            }
            _ => {
                let digits: Vec<usize> = bytes
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.is_ascii_digit())
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&at) = digits.get(rng.below(digits.len())) {
                    let with: &[u8] = match rng.below(4) {
                        0 => b"99999999999",
                        1 => b"-1",
                        2 => b"0",
                        _ => b"1e308",
                    };
                    let _ = bytes.splice(at..at + 1, with.iter().copied());
                }
            }
        }
        bytes
    }

    fn exercise(bytes: Vec<u8>) {
        let Ok(doc) = CosDocument::open(bytes) else {
            return;
        };
        let mut editor = DocumentEditor::new(Arc::new(doc));
        let areas = [
            Redaction {
                area: Rect {
                    x0: 56.0,
                    y0: 40.0,
                    x1: 400.0,
                    y1: 70.0,
                },
                mark: true,
            },
            Redaction {
                area: Rect {
                    x0: 0.0,
                    y0: 190.0,
                    x1: 120.0,
                    y1: 230.0,
                },
                mark: false,
            },
        ];
        for page in 0..2 {
            let _ = apply(&mut editor, page, &areas);
        }
        let _ = apply(&mut editor, 0, &areas[..1]);
        let saved = crate::write::save(&mut editor, &crate::SaveOptions::default());
        let _ = saved.fonts.removed();
    }

    #[test]
    fn the_fixture_redacts_cleanly_before_it_is_damaged() {
        let doc = Arc::new(CosDocument::open(everything()).expect("it opens"));
        let mut editor = DocumentEditor::new(doc);
        let report = apply(
            &mut editor,
            0,
            &[Redaction {
                area: Rect {
                    x0: 56.0,
                    y0: 40.0,
                    x1: 400.0,
                    y1: 70.0,
                },
                mark: false,
            }],
        )
        .expect("page zero");
        assert!(report.glyphs > 0, "{report:?}");
    }

    #[test]
    fn damaged_fixtures_never_panic_through_redaction_and_the_save() {
        let original = everything();
        let mut rng = Rng(0x00C0_FFEE_D1CE_4003);
        for case in 0..300 {
            let mutated = mutate(&original, &mut rng);
            let result = std::panic::catch_unwind(|| exercise(mutated));
            assert!(result.is_ok(), "case {case} panicked");
        }
    }
}
