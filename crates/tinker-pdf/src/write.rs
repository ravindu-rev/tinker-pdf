//! Saving a document, with the font policy attached to the save rather than
//! left to the caller's memory.
//!
//! [`crate::subset`] does the work and has since it landed; what it did not
//! have is a door that runs it without being asked. Nothing on
//! [`tinker_pdf_cos::WriteOptions`], on [`DocumentEditor::save`] or in `tpdf`
//! ran it, so a redaction that must not disclose had to *remember* to — and a
//! redacted document whose embedded face still carries the removed letters'
//! outlines says what the redaction was for. That is the gap this module
//! closes: [`save`] subsets by **default**, and keeping the programs whole is
//! the thing a caller asks for.
//!
//! # Why the switch is not on `WriteOptions`
//!
//! It is the obvious place and it cannot work, for a reason that is about the
//! crate graph rather than about taste.
//!
//! [`tinker_pdf_cos::WriteOptions`] lives in `tinker-pdf-cos`.
//! [`crate::subset::apply`] lives here, in the facade, because it is driven by
//! the **interpreter's own glyph walk** — it is one `interpret` into a
//! recording device over this crate's own page-resource resolver, so that the
//! glyphs kept are the glyphs this engine actually draws. `tinker-pdf-cos`
//! depends on neither the interpreter nor this crate, and `cargo xtask dag`
//! enforces that direction against a declared graph. So a
//! `WriteOptions::subset_fonts: bool` would be a flag the crate that carries
//! it cannot act on: every caller who wrote through [`DocumentEditor::save`] —
//! which is the documented door, the one `docs/features/writing.md` shows —
//! would set it, get no subsetting, and be told nothing. **A silent flag on
//! the disclosure path is worse than no flag**, because the caller who reaches
//! for it is by definition the caller who needs it.
//!
//! Three ways out were weighed:
//!
//! - **The flag on `WriteOptions`, honoured here, ignored there.** Rejected
//!   for the paragraph above. A flag whose meaning depends on which door you
//!   used is a flag that lies through one of them.
//! - **Move the subsetting down into `tinker-pdf-cos`.** It would make the
//!   flag honest and it costs the thing that makes the pass correct: the walk
//!   is `tinker_pdf_content::interpret` over `PageResources`, which resolves
//!   `/Encoding`, `/Differences`, CMaps and Type 3 procedures, descends into
//!   form XObjects at any depth and reads every state of every `/AP`. Moving
//!   it down means moving the interpreter down, or writing a second glyph
//!   resolver in `tinker-pdf-cos` — and a second answer to "what does this
//!   code decode to" is a second engine, which is exactly what
//!   [`crate::subset`]'s own walk exists to avoid. The cost is not worth a
//!   field.
//! - **The switch on the facade's own save path**, which is this module. The
//!   type that carries the flag is then the one that can act on it, and
//!   `WriteOptions` stays a description of *bytes on disk* — layout, version,
//!   compression, encryption — which is all it has ever been.
//!
//! # What stops a caller believing they subsetted when they did not
//!
//! Two things, and neither is a convention.
//!
//! 1. **The cos door offers nothing to believe in.**
//!    [`tinker_pdf_cos::WriteOptions`] has no font field at all, and its own
//!    documentation says so and points here. A caller writing through
//!    [`DocumentEditor::save`] cannot set a switch, so there is no switch to
//!    be wrong about. `the_two_doors_differ_and_the_difference_is_pinned`
//!    holds the two apart as a tested fact rather than a claim in prose.
//! 2. **This door states what it did.** [`save`] returns [`Saved`], which is
//!    `#[must_use]`, and its [`Saved::fonts`] is a [`SubsetOutcome`] rather
//!    than a `bool` — so "nothing was asked for", "the programs were cut and
//!    the originals are gone" and "the programs were cut and the originals
//!    are **still in the file**" are three different values a caller must
//!    match on. The third is not hypothetical: see below.
//!
//! # The incremental save, which is the second silent failure
//!
//! An incremental update appends (7.5.6): the original bytes survive
//! byte-for-byte as a prefix, the *original font programs included*. Subsetting
//! before one of those is worse than not subsetting at all — the file grows by
//! a second copy of every face, and the outlines the caller thought they had
//! removed are still there, at their original offsets, where anyone scanning
//! the bytes finds them.
//!
//! This is not refused, and the choice is worth defending, because refusing
//! would be the tidier-looking answer. Declining to run a pass the caller
//! asked for — on the grounds that this door thinks it pointless — is this
//! module second-guessing the caller, and a caller may legitimately want a
//! viewer to *use* the smaller program while the signature over the prefix
//! survives. The house style is to do what was asked and name the consequence
//! (ruling 10), not to quietly not do it. So it is **named**: that save
//! returns
//! [`SubsetOutcome::CutButTheOriginalsRemain`], which carries the same report
//! and cannot be mistaken for [`SubsetOutcome::Cut`].
//! [`SubsetOutcome::removed`] is the one-call form of the question
//! "is the disclosure out of this file".
//!
//! # `tpdf` has nowhere to put the flag yet, and that is a finding
//!
//! The roadmap row this module closes asked for a `tpdf` flag as well. There
//! is no place for one: all nine subcommands — `info`, `text`, `render`,
//! `fields`, `fonts`, `outline`, `objects`, `check`, `probe` — are read-only,
//! and none of them writes a PDF. The three `DocumentEditor::save` calls
//! inside `tpdf` are `probe`'s rotate-and-crop relations and `check --strict`,
//! which rewrite **in memory to measure**; a font policy on those would change
//! what they measure rather than what a user gets, and `check --strict` in
//! particular has to validate the document the reader saw and not a smaller
//! one. So the flag waits for the write half of the CLI, which is its own
//! roadmap row (`docs/ROADMAP.md`, tier 5, "A user-facing CLI") and now
//! carries the font policy in its exit criterion. Ruling 11 is why that is the
//! right row: a subcommand is a wrapper over this function with no logic of
//! its own, so it cannot land before the door it wraps has a CLI at all.
//!
//! # What this module is not
//!
//! It is not a second serializer. Every byte still comes from
//! [`DocumentEditor::save`]; this runs one pass over the editor first and then
//! calls it. Signing ([`DocumentEditor::save_signed`]) has no door here on
//! purpose: it is incremental by definition, so the paragraph above applies to
//! all of it, and a caller who wants a signed document with subset fonts
//! subsets, rewrites, reopens and signs that.

use tinker_pdf_cos::{DocumentEditor, WriteMode, WriteOptions};

use crate::subset::{self, SubsetReport};

/// What a save does to the document's embedded font programs.
///
/// Deliberately not a `bool`. The two answers are not "on" and "off" — one of
/// them removes glyph outlines from the file and the other does not — and a
/// stray `!` in front of a `bool` is not a thing that should be able to put
/// somebody's redacted text back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontPolicy {
    /// Cut every embedded program down to the glyphs the document still
    /// draws, immediately before serialising. **The default**, because the
    /// caller who needed it was the caller who forgot.
    ///
    /// It is [`crate::subset::apply`], with everything that module documents:
    /// pages, form XObjects at any depth and every state of every `/AP` count
    /// as uses; a program that cannot be bounded is written through whole and
    /// named in the report (ruling 10); and no glyph identifier moves, so the
    /// document's encoding is correct afterwards because it is unchanged.
    #[default]
    Subset,
    /// Write every embedded program through exactly as it arrived.
    ///
    /// The right answer when the document is going to be edited again — the
    /// glyphs a later edit adds are glyphs a subset taken now does not have —
    /// and when the caller has already subsetted and does not want the walk
    /// run twice.
    Keep,
}

/// Options for [`save`].
///
/// [`WriteOptions`] verbatim, plus the one decision it cannot make (see this
/// module's documentation).
#[derive(Clone, Debug, Default)]
pub struct SaveOptions {
    /// How the bytes are laid out: mode, version, linearization, object
    /// streams, compression, encryption and garbage collection.
    pub write: WriteOptions,
    /// What happens to the embedded font programs before any of that.
    pub fonts: FontPolicy,
}

/// What the font pass did, reported by [`save`].
///
/// Three values rather than an `Option<SubsetReport>`, because the third one
/// is a different fact from the second and an `Option` has nowhere to put it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubsetOutcome {
    /// [`FontPolicy::Keep`]: nothing was asked for, so nothing was done and
    /// there is nothing to report.
    Kept,
    /// The pass ran and this was a [`WriteMode::Rewrite`], so nothing of an
    /// old program survives except what the report says was left whole.
    ///
    /// Not the same as "the disclosure is gone": the report's `untouched` is
    /// the list of programs that went through entire, and it is frequently
    /// non-empty for good reasons. [`SubsetOutcome::removed`] is the question
    /// with that folded in.
    Cut(SubsetReport),
    /// The programs were cut down, and the save **appended** rather than
    /// rewrote, so every original program is still in the file's prefix
    /// (7.5.6).
    ///
    /// The smaller programs are what a reader resolves, so the document
    /// *renders* from the subsets — but nothing has been removed from the
    /// bytes, and a document that must not disclose is not finished.
    CutButTheOriginalsRemain(SubsetReport),
}

impl SubsetOutcome {
    /// The report, when a pass ran.
    ///
    /// `None` only for [`SubsetOutcome::Kept`] — never as a stand-in for a
    /// pass that ran and found nothing, which is an empty report and says so.
    #[must_use]
    pub fn report(&self) -> Option<&SubsetReport> {
        match self {
            SubsetOutcome::Kept => None,
            SubsetOutcome::Cut(report) | SubsetOutcome::CutButTheOriginalsRemain(report) => {
                Some(report)
            }
        }
    }

    /// Whether **every** embedded program in this file was cut down to the
    /// glyphs the document still draws.
    ///
    /// The question [`crate::redact`] leaves a caller holding, and the reason
    /// it is this question rather than "did the pass run": a pass that ran and
    /// left one program whole leaves that program's every outline in the file,
    /// including the ones a redaction just removed the text of. So a
    /// non-empty [`SubsetReport::untouched`] is `false` here, whatever the
    /// reason in it — a form field's `/DA` that may draw any character, a
    /// program this build cannot rebuild, a subset that came out no smaller,
    /// a descriptor no font names. Each of those is a good reason to keep a
    /// program and none of them is a reason to tell a caller the disclosure is
    /// gone.
    ///
    /// False for [`SubsetOutcome::Kept`], because nothing was cut, and false
    /// for [`SubsetOutcome::CutButTheOriginalsRemain`], because the originals
    /// are still there.
    ///
    /// `false` is not a failure and is frequently the only correct answer —
    /// it is an instruction to read [`SubsetOutcome::report`], which names
    /// every program still whole and why (ruling 10). Over the fetched
    /// corpora it is the answer for most documents: 4 950 of 10 832 programs
    /// went through whole, and 3 406 of those because a rebuild came out no
    /// smaller than the producer's own subset.
    #[must_use]
    pub fn removed(&self) -> bool {
        matches!(self, SubsetOutcome::Cut(report) if report.untouched.is_empty())
    }
}

/// A saved document, and what happened to its fonts on the way out.
///
/// `#[must_use]`: the bytes are half of it. Dropping this on the floor is
/// dropping the answer to "is the disclosure out of the file", which is the
/// question this whole module exists for.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct Saved {
    /// The file.
    pub bytes: Vec<u8>,
    /// What [`SaveOptions::fonts`] asked for, and what came of it.
    pub fonts: SubsetOutcome,
}

/// Saves a document, subsetting its embedded fonts unless asked not to.
///
/// This is the facade's save door and the only place the font policy can be
/// named. [`DocumentEditor::save`] is still there, still correct and still
/// what this calls — it writes the bytes, and it does not subset, and it does
/// not pretend to.
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use tinker_pdf::write::{save, SaveOptions};
///
/// let doc = tinker_pdf::Document::open(std::fs::read("in.pdf")?)?;
/// let mut editor = doc.editor();
/// // ... edits, a redaction, whatever ...
/// let saved = save(&mut editor, &SaveOptions::default());
/// if !saved.fonts.removed() {
///     // Not a failure: some program went through whole, and the report says
///     // which and why (ruling 10). For a document that must not disclose,
///     // this is the list to read.
///     for whole in saved.fonts.report().into_iter().flat_map(|r| &r.untouched) {
///         eprintln!("{whole}");
///     }
/// }
/// std::fs::write("out.pdf", &saved.bytes)?;
/// # Ok(())
/// # }
/// ```
///
/// **Order.** The pass runs after every edit and before the serializer,
/// which is the only order that is right: it reads the content streams as the
/// editor now has them, so a redaction applied first is a redaction it sees,
/// and the glyphs that redaction removed are glyphs it drops. Run the other
/// way round it would keep exactly what the redaction was for.
pub fn save(editor: &mut DocumentEditor, options: &SaveOptions) -> Saved {
    let fonts = match options.fonts {
        FontPolicy::Keep => SubsetOutcome::Kept,
        FontPolicy::Subset => {
            let report = subset::apply(editor);
            match options.write.mode {
                WriteMode::Rewrite => SubsetOutcome::Cut(report),
                // 7.5.6: the original bytes are the output's prefix, the
                // original programs with them. So the cut happened and the
                // uncut programs are still in the file, which is the one
                // case `Cut` would be a lie about — a caller who redacted
                // and then appended would be told the outlines were gone
                // while they sit in the prefix.
                WriteMode::Incremental => SubsetOutcome::CutButTheOriginalsRemain(report),
            }
        }
    };
    Saved {
        bytes: editor.save(&options.write),
        fonts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use tinker_pdf_cos::CosDocument;

    use crate::subset::tests_support::{
        draws, face, glyph_of, only_program, open, program_named, whole_face_document,
    };
    use crate::subset::{Untouched, UntouchedReason};

    fn editor_over(bytes: Vec<u8>) -> DocumentEditor {
        DocumentEditor::new(Arc::new(CosDocument::open(bytes).expect("it opens")))
    }

    /// The whole vendored face, as the fixture embeds it.
    fn whole_face_bytes() -> usize {
        face().len()
    }

    #[test]
    fn the_default_save_cuts_the_programs_down() {
        let source = whole_face_document(&[(40.0, "Hello")]);
        let mut editor = editor_over(source);
        let saved = save(&mut editor, &SaveOptions::default());

        assert!(saved.fonts.removed(), "a rewrite removes them");
        let report = saved.fonts.report().expect("a pass ran");
        assert_eq!(report.subsetted.len(), 1, "the one embedded program");
        assert!(report.bytes_after() < report.bytes_before());

        let out = Arc::new(CosDocument::open(saved.bytes).expect("the output opens"));
        let program = program_named(&out, b"LiberationSerif");
        assert!(
            program.len() < whole_face_bytes(),
            "{} is not smaller than the face's {}",
            program.len(),
            whole_face_bytes()
        );
    }

    #[test]
    fn keeping_is_asked_for_by_name_and_reported_as_kept() {
        let source = whole_face_document(&[(40.0, "Hello")]);
        let mut editor = editor_over(source);
        let saved = save(
            &mut editor,
            &SaveOptions {
                fonts: FontPolicy::Keep,
                ..SaveOptions::default()
            },
        );

        assert_eq!(saved.fonts, SubsetOutcome::Kept);
        assert!(saved.fonts.report().is_none(), "no pass ran, so no report");
        assert!(
            !saved.fonts.removed(),
            "nothing was cut, so nothing was removed"
        );

        let out = Arc::new(CosDocument::open(saved.bytes).expect("the output opens"));
        assert_eq!(
            program_named(&out, b"LiberationSerif").len(),
            whole_face_bytes(),
            "the face goes through whole"
        );
    }

    /// The pin that holds the two doors apart.
    ///
    /// `tinker-pdf-cos` has no font switch and this asserts what that means:
    /// the same editor written through [`DocumentEditor::save`] carries the
    /// whole face, and written through [`save`] does not. If somebody ever
    /// adds a `WriteOptions` field that quietly does nothing, the two halves
    /// of this test still disagree and the field is still a lie — so this is
    /// not the guard against that. The guard against that is that the field
    /// does not exist, and `WriteOptions`' own documentation says why.
    #[test]
    fn the_two_doors_differ_and_the_difference_is_pinned() {
        let source = whole_face_document(&[(40.0, "Hello")]);

        let through_cos = {
            let editor = editor_over(source.clone());
            editor.save(&WriteOptions::default())
        };
        let through_facade = {
            let mut editor = editor_over(source);
            save(&mut editor, &SaveOptions::default()).bytes
        };

        let cos_out = Arc::new(CosDocument::open(through_cos).expect("it opens"));
        let facade_out = Arc::new(CosDocument::open(through_facade).expect("it opens"));
        assert_eq!(
            program_named(&cos_out, b"LiberationSerif").len(),
            whole_face_bytes(),
            "the cos door writes the face through whole, and always has"
        );
        assert!(
            program_named(&facade_out, b"LiberationSerif").len() < whole_face_bytes(),
            "the facade door cuts it down"
        );
    }

    /// 7.5.6, and the reason [`SubsetOutcome`] has three values.
    #[test]
    fn an_incremental_save_says_the_originals_are_still_in_the_file() {
        let source = whole_face_document(&[(40.0, "Hello")]);
        let mut editor = editor_over(source.clone());
        let saved = save(
            &mut editor,
            &SaveOptions {
                write: WriteOptions {
                    mode: WriteMode::Incremental,
                    ..WriteOptions::default()
                },
                ..SaveOptions::default()
            },
        );

        assert!(
            matches!(saved.fonts, SubsetOutcome::CutButTheOriginalsRemain(_)),
            "an append cannot remove anything: {:?}",
            saved.fonts
        );
        assert!(
            !saved.fonts.removed(),
            "and `removed` has to say so, because this is the question"
        );
        assert!(
            saved.fonts.report().is_some(),
            "the pass did run; what it did not do is take the originals away"
        );
        assert!(
            saved.bytes.starts_with(&source),
            "the prefix invariant, which is exactly why the originals remain"
        );
        assert!(
            saved.bytes.len() > source.len(),
            "a second, smaller copy of the face was appended to the first"
        );
    }

    /// The whole row, in one test: **nobody asked**.
    ///
    /// A redaction and then an ordinary save. No `subset::apply`, no policy
    /// named, no flag — `SaveOptions::default()`. The letters the redaction
    /// removed have no outline in the file that comes out, and the letters it
    /// left do. Adjudicated against the output program's own `loca` and
    /// `glyf`, as `subset.rs`'s own disclosure test is: a statement about
    /// bytes in a file, not about a raster.
    #[test]
    fn a_redaction_saved_the_ordinary_way_does_not_carry_the_removed_glyphs() {
        use crate::redact::{apply as redact_apply, Redaction};
        use tinker_pdf_cos::pages::Rect;

        /// Disjoint, so "the removed line's glyphs are gone" is not weakened
        /// by a glyph the kept line needs anyway.
        const KEPT: &str = "bcdfgh";
        const REMOVED: &str = "vwxyzk";

        let mut editor = editor_over(whole_face_document(&[(60.0, KEPT), (20.0, REMOVED)]));
        let report = redact_apply(
            &mut editor,
            0,
            &[Redaction {
                area: Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 300.0,
                    y1: 45.0,
                },
                mark: false,
            }],
        )
        .expect("the page exists");
        assert_eq!(report.glyphs, REMOVED.chars().count());

        let saved = save(&mut editor, &SaveOptions::default());
        assert!(
            saved.fonts.removed(),
            "the one program was cut and nothing was left whole: {:?}",
            saved.fonts.report()
        );

        let cut = only_program(&open(saved.bytes));
        for ch in REMOVED.chars() {
            let glyph = glyph_of(&cut, ch);
            assert!(
                !draws(&cut, glyph),
                "{ch:?} was redacted, nobody asked for a subset, and its \
                 outline (glyph {glyph}) is still in the file"
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

    /// The counterfactual, and the sentence the roadmap row ended on.
    ///
    /// The same redaction written through the door that does not subset. The
    /// removed letters' outlines are still there — which is what every save
    /// did before this module, and is why the default is the other way round.
    #[test]
    fn keeping_the_programs_leaves_the_redacted_letters_outlines_in_the_file() {
        use crate::redact::{apply as redact_apply, Redaction};
        use tinker_pdf_cos::pages::Rect;

        const REMOVED: &str = "vwxyzk";

        let mut editor = editor_over(whole_face_document(&[(60.0, "bcdfgh"), (20.0, REMOVED)]));
        redact_apply(
            &mut editor,
            0,
            &[Redaction {
                area: Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 300.0,
                    y1: 45.0,
                },
                mark: false,
            }],
        )
        .expect("the page exists");

        let saved = save(
            &mut editor,
            &SaveOptions {
                fonts: FontPolicy::Keep,
                ..SaveOptions::default()
            },
        );
        assert!(!saved.fonts.removed());

        let whole = only_program(&open(saved.bytes));
        for ch in REMOVED.chars() {
            let glyph = glyph_of(&whole, ch);
            assert!(
                draws(&whole, glyph),
                "{ch:?} is redacted text whose outline this door was asked to \
                 keep, so it has to still be there or this test proves nothing"
            );
        }
    }

    /// `removed()` is about the **file**, not about whether the pass ran.
    ///
    /// A `Cut` whose report names a program left whole is a file that still
    /// carries that program's every outline, so answering `true` would be the
    /// exact lie this module exists to prevent. Built rather than measured:
    /// the outcome is constructed from a report, because what is under test is
    /// the predicate and not the pass.
    #[test]
    fn a_program_left_whole_means_the_disclosure_is_not_out_of_the_file() {
        let mut report = crate::subset::SubsetReport::default();
        assert!(
            SubsetOutcome::Cut(report.clone()).removed(),
            "an empty report is a rewrite that cut everything it found"
        );

        report.untouched.push(Untouched {
            program: tinker_pdf_cos::ObjRef::new(7, 0),
            base_font: "ABCDEF+Helvetica".to_string(),
            bytes: 12_345,
            reason: UntouchedReason::FieldResource,
        });
        assert!(
            !SubsetOutcome::Cut(report.clone()).removed(),
            "a form field's /DA may draw any character, so that face went \
             through entire and the redacted letters are in it"
        );
        assert!(
            SubsetOutcome::Cut(report.clone()).report().is_some(),
            "and the list naming it is what `removed` sends a caller to"
        );
        assert!(!SubsetOutcome::CutButTheOriginalsRemain(report).removed());
        assert!(!SubsetOutcome::Kept.removed());
    }
}
