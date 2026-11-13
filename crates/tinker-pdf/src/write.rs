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
//! # `tpdf`'s flag arrived with its write half
//!
//! The roadmap row this module closes asked for a `tpdf` flag as well, and
//! when it closed there was no place for one: every subcommand was read-only.
//! The flag waited for the CLI's write half, its own roadmap row, rather than
//! arriving before the door it wraps — ruling 11 makes a subcommand a wrapper
//! over this function with no logic of its own. That half has landed: `merge`,
//! `split`, `rotate`, `encrypt`, `decrypt`, `attach`, `stamp` and `sanitise`
//! all save through [`save`] and all take `--font-policy subset|keep`, whose
//! default is this module's (`tools/tpdf/src/writing.rs`). The other
//! `DocumentEditor::save` calls inside `tpdf` — `probe`'s rotate-and-crop
//! relations and `check --strict` — still rewrite **in memory to measure**,
//! and take no policy: one there would change what they measure rather than
//! what a user gets, and `check --strict` in particular has to validate the
//! document the reader saw and not a smaller one.
//!
//! # Images, the second pass on this door
//!
//! [`SaveOptions::images`] recodes and downsamples image XObjects, and is
//! **off by default**: [`ImagePolicy::Keep`] never enters the pass, so a
//! default save writes every image stream through as the file stored it.
//! The switch is here and not on a `WriteOptions::images` for the fonts'
//! reason and one of its own — a resolution is a property of where an image
//! is *drawn*, which only the interpreter's walk can say — and `recode.rs`'s
//! header sets both out. It runs before the font pass and reports on
//! [`Saved::images`] in the same shape as [`Saved::fonts`], including the
//! incremental case: an appended save leaves every original image stream in
//! the prefix, so a smaller file is not what it produces, and
//! [`ImageOutcome::RecodedButTheOriginalsRemain`] says so.
//!
//! # Missing fonts, embedded when asked
//!
//! [`SaveOptions::embed`] is a [`FontEmbedding`], **off by default** as the
//! image pass is: [`FontEmbedding::Leave`] never enters the pass, so a save
//! writes what it wrote before the pass existed. Asked with a provider, the
//! save embeds, for each font the pages draw whose dictionary embeds no
//! program, the face that provider answers — the face the page was drawn
//! with — cut to the glyphs drawn, and reports each font embedded or left,
//! with its reason, on [`Saved::embedded`] (`docs/design/font-embedding.md`,
//! `crate::embed`). It runs between the image pass and the font pass: it
//! reads content streams, as the font pass does, and writes none, and the
//! programs it writes are cut already, so the font pass is handed them and
//! passes over them, whatever [`SaveOptions::fonts`] says of the document's
//! own. An incremental save appends them with the dictionaries that name
//! them; the original file embedded none, so nothing of the pass's is left
//! in its prefix.
//!
//! # Type 1 programs as CFF, when asked
//!
//! [`SaveOptions::type1`] is a [`Type1Programs`], **`Keep` by default**: a
//! Type 1 program stays one, cut as [`SaveOptions::fonts`] says. Asked to
//! [`Type1Programs::Convert`], the font pass rewrites each Type 1 program it
//! cuts as `/FontFile3 /Subtype /Type1C` and cuts that with the CFF
//! subsetter, where the conversion draws every glyph as the program did and
//! every code the pages show reaches the same glyph through a CFF program's
//! route (`crate::subset`, *A Type 1 program converted, when asked*); one
//! that cannot be is cut as a Type 1 program and named, with its reason, in
//! [`SubsetReport::unconverted`]. [`FontPolicy::Keep`] runs no font pass, so
//! it converts nothing.
//!
//! # Appending, and reading what was appended
//!
//! [`append`] is the edit-and-reopen door (`docs/design/performance.md`,
//! P-17): it writes an editor's edits as the update section an incremental
//! save appends ([`DocumentEditor::save_update`]), and reopens the document the
//! editor was made from over that section
//! ([`CosDocument::reopen`](crate::CosDocument::reopen)), so the caller holds
//! the bytes to append and the document they make, read as a fresh open of
//! the whole file reads it. Neither costs the document: nothing
//! of its bytes is copied, its cross-reference chain is not walked again and
//! its entries are not probed again, and every object the edits leave alone is
//! its own parse, kept. It runs neither pass [`save`] runs. Both would cut
//! programs and images whose originals an update leaves in the file (7.5.6),
//! as [`SubsetOutcome::CutButTheOriginalsRemain`] says of [`save`] under
//! [`WriteMode::Incremental`]; a caller who wants the smaller programs drawn
//! saves through [`save`] and opens what it wrote.
//!
//! # What this module is not
//!
//! It is not a second serializer. Every byte still comes from
//! [`DocumentEditor::save`] or [`DocumentEditor::save_update`]; this runs its
//! passes over the editor first and then calls one. Signing
//! ([`DocumentEditor::save_signed`]) has no door here on purpose: it is
//! incremental by definition, so what the incremental save's section says
//! applies to all of it, and a caller who wants a signed document with subset
//! fonts subsets, rewrites, reopens and signs that.

use std::collections::BTreeSet;
use std::sync::Arc;

use tinker_pdf_cos::{DocumentEditor, FullOpen, WriteMode, WriteOptions};

pub use crate::embed::{
    EmbedReport, EmbedWarning, EmbeddedFont, EmbeddedFormat, FontEmbedding, LeftFont, LeftReason,
};
pub use crate::recode::{
    BilevelCodec, ContinuousCodec, ImageCoding, ImagePolicy, ImageRecoding, ImageReport,
    JpegTables, Recoded, UntouchedImage, UntouchedImageReason,
};
use crate::stats::{self, Stats};
use crate::subset::{self, SubsetReport};
use crate::Parallelism;

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

/// What the font pass does with the Type 1 programs it cuts
/// (`docs/design/subsetting.md`, *When a save converts*).
///
/// Opt-in, because a save that rewrote every Type 1 program's format would
/// change what other readers are handed without being asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Type1Programs {
    /// A Type 1 program stays one, cut as [`SaveOptions::fonts`] says.
    /// **The default.**
    #[default]
    Keep,
    /// Each Type 1 program the font pass cuts is rewritten as a CFF —
    /// `/FontFile3 /Subtype /Type1C`, `/Length1` to `/Length3` dropped — by
    /// [`tinker_pdf_font::type1_to_cff`], and cut by the CFF subsetter, where
    /// every code the pages show through its fonts reaches the glyph it drew
    /// through a CFF program's route as well. One that cannot be converted so
    /// is cut as a Type 1 program and named in
    /// [`SubsetReport::unconverted`]; under [`FontPolicy::Keep`] nothing is
    /// converted, since every program is written through as it arrived.
    Convert,
}

/// Options for [`save`].
///
/// [`WriteOptions`] verbatim, plus the two decisions it cannot make (see this
/// module's documentation), and whether to count what the save does.
#[derive(Clone, Debug, Default)]
pub struct SaveOptions {
    /// How the bytes are laid out: mode, version, linearization, object
    /// streams, compression, encryption and garbage collection.
    pub write: WriteOptions,
    /// What happens to the embedded font programs before any of that.
    pub fonts: FontPolicy,
    /// Whether the font pass rewrites the Type 1 programs it cuts as CFF.
    /// [`Type1Programs::Keep`] — the default — keeps every Type 1 program a
    /// Type 1 program.
    pub type1: Type1Programs,
    /// What happens to the image XObjects, before the fonts. [`ImagePolicy::Keep`]
    /// — the default — leaves every image stream as the file stored it.
    pub images: ImagePolicy,
    /// What happens to the fonts the pages draw and the document embeds no
    /// program for, after the images and before the font pass.
    /// [`FontEmbedding::Leave`] — the default — embeds nothing;
    /// [`FontEmbedding::Embed`] names the provider whose faces are embedded,
    /// each cut to the glyphs drawn (`docs/design/font-embedding.md`).
    pub embed: FontEmbedding,
    /// Count what the save does and hand it back as [`Saved::stats`]: the
    /// image and font passes' walks of the page tree, the font dictionaries
    /// and forms' scopes the font pass reads, and every stream the passes and
    /// the serializer decode. On an editor [`DocumentEditor::count_work`]
    /// asked to count, the count is everything counted over the editor since,
    /// this save included.
    ///
    /// Off by default. A counted save writes the same bytes and reports the
    /// same passes as an uncounted one.
    pub stats: bool,
    /// The threads the save may spend. [`Parallelism::Serial`] by default.
    ///
    /// Spent on the font pass's walk of the pages: with `Threads(n)` each
    /// page and its annotation appearances are walked on a thread of the
    /// engine's own pool, and what they show is folded in page order, so the
    /// programs cut, the [`SubsetReport`] and every saved byte are the serial
    /// save's. A counted save then counts where each page was walked
    /// ([`crate::Stats::pool_items_on_workers`]); a save asked for no threads
    /// walks on the calling thread and counts no part. Handed to the writer
    /// too, in place of [`WriteOptions::parallelism`], which then compresses
    /// the streams it writes on the same threads; with `Serial` here the
    /// writer's own setting stands.
    pub parallelism: Parallelism,
}

/// What the image pass did, reported by [`save`]: [`SubsetOutcome`]'s three
/// values, for the same reasons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageOutcome {
    /// [`ImagePolicy::Keep`]: the pass did not run.
    Kept,
    /// The pass ran and this was a [`WriteMode::Rewrite`], so the recoded
    /// streams replaced the originals.
    Recoded(ImageReport),
    /// The pass ran and the save **appended** (7.5.6), so every original
    /// image stream is still in the file's prefix: a reader draws the recoded
    /// ones, and the file is larger, not smaller.
    RecodedButTheOriginalsRemain(ImageReport),
}

impl ImageOutcome {
    /// The report, when the pass ran.
    #[must_use]
    pub fn report(&self) -> Option<&ImageReport> {
        match self {
            ImageOutcome::Kept => None,
            ImageOutcome::Recoded(report) | ImageOutcome::RecodedButTheOriginalsRemain(report) => {
                Some(report)
            }
        }
    }
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
    /// gone. A Type 3 font whose procedures went through whole
    /// ([`SubsetReport::type3_untouched`]) is `false` for the same reason:
    /// its procedures are its outlines, and can show text besides.
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
        matches!(
            self,
            SubsetOutcome::Cut(report)
                if report.untouched.is_empty() && report.type3_untouched.is_empty()
        )
    }
}

/// A saved document, and what happened to its fonts on the way out.
///
/// `#[must_use]`: the bytes are half of it. Dropping this on the floor is
/// dropping the answer to "is the disclosure out of the file", which is the
/// question this whole module exists for.
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Saved {
    /// The file.
    pub bytes: Vec<u8>,
    /// What [`SaveOptions::fonts`] asked for, and what came of it.
    pub fonts: SubsetOutcome,
    /// What [`SaveOptions::images`] asked for, and what came of it.
    pub images: ImageOutcome,
    /// What [`SaveOptions::embed`] asked for, and what came of it: `None`
    /// for [`FontEmbedding::Leave`], which runs nothing, and otherwise every
    /// font the pages draw that embeds no program, embedded or left, with its
    /// reason — an empty report where there was none.
    pub embedded: Option<EmbedReport>,
    /// What [`SaveOptions::stats`] asked to be counted: `None` unless it
    /// asked.
    pub stats: Option<Stats>,
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
    // The editor's tally when it is counting, so that a merge's save reports
    // what was counted over the editor since `count_work`; the save's own
    // when it is not and the caller asked. Either way both passes and the
    // serializer count.
    let work = editor
        .work()
        .cloned()
        .or_else(|| stats::tally(options.stats));
    // Images first: the font pass reads content streams, and nothing this
    // pass writes is one, so the order is free — and this way a later reader
    // of this function meets the passes in the order `SaveOptions` lists
    // them after `write`.
    let images = match &options.images {
        ImagePolicy::Keep => ImageOutcome::Kept,
        ImagePolicy::Recode(recoding) => {
            let report = crate::recode::apply(editor, recoding, work.as_ref());
            match options.write.mode {
                WriteMode::Rewrite => ImageOutcome::Recoded(report),
                WriteMode::Incremental => ImageOutcome::RecodedButTheOriginalsRemain(report),
            }
        }
    };
    // Missing fonts next, so the font pass's walk sees what this wrote: the
    // programs it hands back are cut already, and the font pass passes over
    // them whatever `options.fonts` says of the document's own.
    let (embedded, written) = match &options.embed {
        FontEmbedding::Leave => (None, BTreeSet::new()),
        FontEmbedding::Embed(provider) => {
            let (report, written) =
                crate::embed::apply(editor, provider, work.as_ref(), options.parallelism);
            (Some(report), written)
        }
    };
    let fonts = match options.fonts {
        FontPolicy::Keep => SubsetOutcome::Kept,
        FontPolicy::Subset => {
            let report = subset::apply_counting(
                editor,
                work.as_ref(),
                options.parallelism,
                &written,
                options.type1 == Type1Programs::Convert,
            );
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
    // The save's threads are handed to the writer, which compresses the
    // streams it writes on them, where the save asks for any; otherwise the
    // writer's own `WriteOptions::parallelism` stands.
    let mut write = options.write.clone();
    if matches!(options.parallelism, Parallelism::Threads(threads) if threads > 0) {
        write.parallelism = options.parallelism;
    }
    Saved {
        bytes: editor.save_counted(&write, work.as_deref()),
        fonts,
        images,
        embedded,
        stats: work.filter(|_| options.stats).map(|work| work.stats()),
    }
}

/// Options for [`append`].
///
/// The two [`WriteOptions`] fields an update section reads, and whether to
/// count the reopen. Nothing else `WriteOptions` holds applies to an update:
/// its layout, version, containers and encryption are the document's, and it
/// collects and merges nothing, so a field here the call did not read would
/// be a flag that lies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppendOptions {
    /// Compress the streams the update carries that the caller has not
    /// already encoded, as [`WriteOptions::compress`] does. Off by default.
    pub compress: bool,
    /// The threads the writer may spend compressing them, as
    /// [`WriteOptions::parallelism`] spends them: every byte is the serial
    /// writer's. [`Parallelism::Serial`] by default.
    pub parallelism: Parallelism,
    /// Count what the reopen reads and hand it back as [`Appended::stats`]:
    /// the section's tokens, its cross-reference rows and the objects it
    /// parses, and all a full open reads when the reopen opens whole. On an
    /// editor [`DocumentEditor::count_work`] asked to count, the count is
    /// everything counted over the editor since, this reopen included.
    ///
    /// Off by default. A counted append writes the same bytes and reopens the
    /// same document as an uncounted one.
    pub stats: bool,
}

/// An editor's edits written as an incremental update, and the document
/// reopened over it, by [`append`].
///
/// `#[must_use]`: the update is bytes a caller has to append to the file it
/// holds, and the document is what those two make.
#[derive(Clone)]
#[must_use]
pub struct Appended {
    /// The update section: the bytes an incremental save appends to the
    /// document's own, its offsets counted from the document's start, so the
    /// document's bytes followed by these are the save
    /// [`DocumentEditor::save`] writes under [`WriteMode::Incremental`], byte
    /// for byte. Shared with [`Appended::document`], which reads them where
    /// they are.
    pub update: Arc<[u8]>,
    /// The document's bytes followed by [`Appended::update`], as
    /// [`Document::open`] reads the two joined: the same objects, pages,
    /// pixels and text, and the warnings the document had and then the
    /// update's own. It carries the font provider and the archive report the
    /// document had, and the security it was authenticated with; and, where
    /// the document keeps a cache ([`Document::cache_limits`]), a cache of its
    /// own under the same budget, since what the document's kept was drawn
    /// from the objects before the edit.
    ///
    /// [`Document::open`]: crate::Document::open
    /// [`Document::cache_limits`]: crate::Document::cache_limits
    pub document: crate::Document,
    /// Why the reopen read the whole document again rather than the update
    /// alone, when it did ([`FullOpen`]); `None` when it read only the
    /// update.
    pub full_open: Option<FullOpen>,
    /// The numbers of the objects the update writes, ascending
    /// ([`DocumentEditor::changed_numbers`]): what the reopen parses afresh.
    pub changed: Vec<u32>,
    /// What [`AppendOptions::stats`] asked to be counted: `None` unless it
    /// asked.
    pub stats: Option<Stats>,
    /// The document the update was written over, whose pages
    /// [`Appended::patch`] compares with the reopened document's.
    base: crate::Document,
}

impl Appended {
    /// Re-renders page `index` of [`Appended::document`] over `previous`, the
    /// bitmap [`crate::Page::render`] gave the same page of the document
    /// before the update under the same `options`, rendering only the
    /// rectangle the update changed (`docs/design/performance.md`, P-17).
    ///
    /// The page is patched when the update changed nothing it draws but its
    /// annotations. The rectangle is each changed, added or removed
    /// annotation's `/Rect`, before the edit and after it, where 12.5.5's fit
    /// and its `/BBox` clip confine what it draws, carried through the
    /// render's page transform, their hull rounded outward and widened by a
    /// pixel; it is rendered through [`crate::RenderOptions::region`] and
    /// written over `previous`, and ruling 5 makes that region the full
    /// render's own pixels. The one decision a region cannot see past its
    /// edge — whether an image joins an image run, ruling 5's named exception
    /// — is reported by the region's render, and the rectangle grows around
    /// it until the render sees it. Afterwards `previous` is the bitmap
    /// [`crate::Page::render`] gives the page after the update, pixels and
    /// warnings: an update that changed no pixel leaves it as it was, and a
    /// page that cannot be patched is rendered whole into it, and
    /// [`Patch::fallback`](crate::Patch::fallback) says why
    /// ([`crate::PatchFallback`]). When `options` asks for counts,
    /// `previous.stats` is what the patch counted — the renders of its
    /// rectangle, or the whole page's — and nothing for a patch that rendered
    /// nothing.
    ///
    /// `previous` must be clean: a bitmap with warnings is rendered over
    /// whole, since a render of a rectangle raises only what that part of the
    /// page raises. The fields of `self` are read as [`append`] made them.
    ///
    /// `None` when the reopened document has no page `index`, and then
    /// `previous` is as it was.
    pub fn patch(
        &self,
        index: u32,
        previous: &mut crate::Bitmap,
        options: &crate::RenderOptions,
    ) -> Option<crate::Patch> {
        crate::patch::patch(
            &self.base,
            &self.document,
            &self.changed,
            index,
            previous,
            options,
        )
    }
}

impl core::fmt::Debug for Appended {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The update's bytes are a document's, which is not what a caller
        // printing this wants to read; their length is.
        f.debug_struct("Appended")
            .field("update", &self.update.len())
            .field("document", &self.document)
            .field("full_open", &self.full_open)
            .field("changed", &self.changed)
            .field("stats", &self.stats)
            .finish()
    }
}

/// Why [`append`] handed back no [`Appended`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AppendError {
    /// The editor edits another document than the one handed in: an
    /// update's offsets count from its own document's start, and its trailer
    /// chains to that document's last section, so read after another
    /// document's bytes it names nothing that is there. "The same" is the
    /// same object layer — [`Document::editor`], or a [`DocumentEditor`] over
    /// [`Document::cos`]'s own handle — and not equal bytes opened again.
    ///
    /// [`Document::editor`]: crate::Document::editor
    /// [`Document::cos`]: crate::Document::cos
    NotThisDocument,
    /// The document and its update did not reopen: only a reopen that opens
    /// the whole document again can fail, as [`crate::Document::open`] fails,
    /// and a streamed document whose bytes cannot all be fetched for it is
    /// [`crate::OpenError::SourceUnavailable`], naming the range.
    Reopen(crate::OpenError),
}

impl core::fmt::Display for AppendError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AppendError::NotThisDocument => {
                f.write_str("the editor edits another document than the one handed in")
            }
            AppendError::Reopen(why) => write!(f, "the update did not reopen: {why}"),
        }
    }
}

impl std::error::Error for AppendError {}

/// Writes an editor's edits as an incremental update and reopens the
/// document over it (`docs/design/performance.md`, P-17).
///
/// `editor` must edit `document` itself ([`AppendError::NotThisDocument`]).
/// The update is [`DocumentEditor::save_update`]'s section: what
/// [`DocumentEditor::save`] appends under [`WriteMode::Incremental`], written
/// from the document's length and its last byte, so none of its bytes is
/// copied; on an encrypted document it is sealed with the document's own key
/// (7.6.2), and [`DocumentEditor::check_save`] under that mode is the question
/// of whether it writes anything in the clear. The document is then reopened
/// over the section ([`CosDocument::reopen`](crate::CosDocument::reopen)),
/// which reads the section's `startxref`, its cross-reference data and trailer
/// and the header of each object it places, and keeps the document's entries,
/// parsed objects, object streams and security for every number the section
/// leaves alone. A document the reopen cannot keep that way — linearized and
/// opened from its head, streamed, repaired, shifted, or reopened too deep —
/// or an update that does not chain to it is read whole with the section, and
/// [`Appended::full_open`] says why.
///
/// The update and the document it makes are handed back together with the
/// numbers the update writes and, when [`AppendOptions::stats`] asked, what
/// the reopen read: where it read the section alone, the section's rows and
/// no others, and of whole objects only a cross-reference stream's own, which
/// a classic section has none of. The editor is left as it was; an editor
/// over [`Appended::document`] makes the update that follows this one, and
/// its append reads that update alone in turn.
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use tinker_pdf::write::{append, AppendOptions};
///
/// let original = std::fs::read("in.pdf")?;
/// let doc = tinker_pdf::Document::open(original.clone())?;
/// let mut editor = doc.editor();
/// if editor.rotate_page(0, 90) {
///     let appended = append(&doc, &editor, &AppendOptions::default())?;
///     std::fs::write("out.pdf", [&original[..], &appended.update[..]].concat())?;
///     // The turned page, read without opening `out.pdf` again.
///     assert_eq!(appended.document.page(0).map(|page| page.rotation()), Some(90));
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// [`AppendError::NotThisDocument`] when `editor` edits another document, and
/// [`AppendError::Reopen`] when a reopen that opens the whole document again
/// fails.
pub fn append(
    document: &crate::Document,
    editor: &DocumentEditor,
    options: &AppendOptions,
) -> Result<Appended, AppendError> {
    if !std::ptr::eq(editor.document(), document.cos()) {
        return Err(AppendError::NotThisDocument);
    }
    // The editor's tally when it is counting, as `save` takes it, and the
    // call's own when it is not and the caller asked.
    let work = editor
        .work()
        .cloned()
        .or_else(|| stats::tally(options.stats));
    let update: Arc<[u8]> = editor
        .save_update(&WriteOptions {
            mode: WriteMode::Incremental,
            compress: options.compress,
            parallelism: options.parallelism,
            ..WriteOptions::default()
        })
        .into();
    let reopened = stats::reopen(&document.inner, Arc::clone(&update), work.as_ref())
        .map_err(|error| AppendError::Reopen(reopen_error(error)))?;
    let inner = reopened.document;
    // A cache of its own under the document's budget: an entry the document
    // kept may be of an object the update replaces. Where that budget was
    // stated at open, so that it charges the document's parsed objects, it
    // charges the reopened document's too, as an open does; an object read
    // through the document stays charged where it is held.
    let cache = document
        .cache_limits()
        .map(|limits| Arc::new(crate::cache::DocumentCache::new(limits)));
    if let Some(cache) = cache.as_ref().filter(|_| document.cos().objects_charged()) {
        inner.charge_objects_to(cache.accountant());
    }
    Ok(Appended {
        update,
        document: crate::Document {
            inner: Arc::new(inner),
            // The document's own, already made effective: a `bundled-fonts`
            // build's faces are chained behind a provider once, at open.
            fonts: document.fonts.clone(),
            archive: document.archive.clone(),
            cache,
        },
        full_open: reopened.full_open,
        changed: editor.changed_numbers(),
        stats: work.filter(|_| options.stats).map(|work| work.stats()),
        base: document.clone(),
    })
}

/// The facade's reading of a reopen's refusal, as a streamed open reads one.
fn reopen_error(error: tinker_pdf_cos::OpenError) -> crate::OpenError {
    match error {
        tinker_pdf_cos::OpenError::SourceUnavailable(miss) => {
            crate::OpenError::SourceUnavailable(miss)
        }
        tinker_pdf_cos::OpenError::NoObjects => crate::OpenError::NotAPdf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use tinker_pdf_cos::CosDocument;

    use crate::subset::tests_support::written_support::authored;
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
        authored("the whole face, saved by default", &saved.bytes);

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

        authored("the whole face, kept", &saved.bytes);
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

        authored("the whole face, through the cos door", &through_cos);
        authored("the whole face, through the facade door", &through_facade);
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

        authored("the whole face, cut in an update", &saved.bytes);
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
        authored("a redacted page, saved by default", &saved.bytes);
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
        authored("a redacted page, its programs kept", &saved.bytes);

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

    /// A reopen's refusal reads as a streamed open's: a source that could not
    /// supply the bytes names the range it was asked for, and bytes with no
    /// objects are not a PDF.
    #[test]
    fn a_reopen_refusal_reads_as_an_open_refusal() {
        let miss = tinker_pdf_cos::SourceMiss::at(3..9);
        assert_eq!(
            reopen_error(tinker_pdf_cos::OpenError::SourceUnavailable(miss.clone())),
            crate::OpenError::SourceUnavailable(miss)
        );
        assert_eq!(
            reopen_error(tinker_pdf_cos::OpenError::NoObjects),
            crate::OpenError::NotAPdf
        );
        let error = AppendError::Reopen(crate::OpenError::NotAPdf);
        assert!(error.to_string().contains("not a PDF"), "{error}");
    }
}
