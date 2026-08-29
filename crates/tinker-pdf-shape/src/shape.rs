//! The default shaper: text in, positioned glyphs out.
//!
//! Milestone 1 of `docs/design/shaping.md` built the machinery that executes a
//! lookup list against a [`Buffer`]; this is the layer that decides **which**
//! lookups run and **in what order**, maps the caller's text to glyphs through
//! the face's `cmap`, and keeps every glyph pointing back at the byte of text
//! it came from.
//!
//! # The pipeline, and where each stage is
//!
//! Per paragraph: [`crate::bidi::Paragraph::new`] resolves embedding levels,
//! [`itemize`] cuts the text into runs of one script at one level, and
//! [`Shaper::shape`] shapes one run. The caller breaks lines over the
//! *logical* text and calls [`crate::bidi::Paragraph::line`] per line.
//! [`Shaper::shape_text`] is the three-line version of the first three steps,
//! for a caller that has one paragraph and wants glyphs.
//!
//! # Logical order, in and out
//!
//! A run is shaped in logical order and comes back in logical order, whichever
//! way it reads. That is the specification's own model — `GSUB` and `GPOS`
//! rules are written over the text as typed — and it is what makes
//! [`ShapedGlyph::cluster`] monotonic, which milestone 7 needs to rebuild
//! `/ToUnicode` from a ligature. Turning logical order into visual order is
//! [`crate::bidi::reorder`]'s job and happens per line, after breaking.
//!
//! Milestone 2 recorded that no right-to-left run had a glyph-level fixture,
//! so the claim about one was that it was deterministic and that its levels
//! were right, not that its glyphs were. **Milestone 4 closed that**:
//! text-rendering-tests SHARAN-1 shapes six Nasta‘līq words of Urdu through
//! this function, and `tests/text_rendering.rs` checks every glyph and every
//! position against the fixture — after reordering the runs with
//! [`crate::bidi::reorder`] and walking a right-to-left run's glyphs
//! backwards, which is the two-step a consumer does and the only place
//! direction is read outside this crate.
//!
//! # Joining scripts take a different plan
//!
//! A run whose text contains a character that cursively joins gets
//! [`JOINING_GSUB_STAGES`] rather than [`DEFAULT_GSUB_FEATURES`]: seven stages
//! instead of one, with the four joining features restricted to the glyphs
//! whose letters are in that form. Which run that is comes from the text and
//! not from a list of scripts; `crate::arabic` says why.
//!
//! # Integer, throughout
//!
//! Every number that leaves here is a font design unit, and
//! [`ShapedRun::units_per_em`] is beside them so a consumer can scale. The
//! crate denies `clippy::float_arithmetic`; the reason is ruling 4 and it is
//! written up in `lib.rs`.

use core::ops::Range;

use tinker_pdf_font::Sfnt;

use crate::arabic;
use crate::bidi::{BaseDirection, Level, Paragraph};
use crate::buffer::{Buffer, Direction, ShapedGlyph};
use crate::common::Tag;
use crate::limits::Limits;
use crate::read::Bytes;
use crate::unicode::{self, Script};
use crate::universal;
use crate::{Layout, MarkWidths, Warning};

/// The `GSUB` features the default shaper turns on, in the order they are
/// requested.
///
/// The order in this array does **not** decide the order they run in:
/// [`crate::LayoutTable::lookups_for`] sorts by lookup index, because
/// ISO/IEC 14496-22 says lookups are applied in the order they appear in the
/// lookup list and not in the order the features naming them do. The array is
/// written in the specification's own order anyway, so that reading it tells
/// the truth about intent.
///
/// # Which of these the fixtures adjudicate, and which they do not
///
/// `docs/design/shaping.md` names `ccmp` and `liga` for this milestone. Two
/// more are here because the conformance corpus *requires* them and would fail
/// without them, which is the strongest reason available under ruling 13:
///
/// - `calt` — text-rendering-tests **GSUB-1** puts its whole test behind it.
/// - `ccmp` — **GSUB-2**, the Ethiopic numerals, likewise.
/// - `rlig` — **GSUB-3**, the billion-laughs face.
///
/// `liga`, `clig` and `locl` are on because the OpenType feature registry
/// marks them applied by default in horizontal text, and **no fixture in this
/// corpus reaches any of them**. That is stated rather than implied: three of
/// the six entries below are evidence and three are the registry's word.
pub const DEFAULT_GSUB_FEATURES: &[Tag] = &[
    Tag::new(b"locl"),
    Tag::new(b"ccmp"),
    Tag::new(b"rlig"),
    Tag::new(b"liga"),
    Tag::new(b"clig"),
    Tag::new(b"calt"),
];

/// The `GPOS` features the default shaper turns on.
///
/// `kern`, `mark` and `mkmk` are the three `docs/design/shaping.md` names for
/// milestone 2 and all three are adjudicated: GPOS-1 and GPOS-2 are `kern`,
/// GPOS-3 is `mark`, GPOS-4 is `mkmk`. `curs` was milestone 4's subject and is
/// now adjudicated too — SHARAN-1 is nothing but cursive attachment, and it is
/// what settled which of the two readings of that lookup this crate follows;
/// see [`crate::gpos`]'s type 3.
///
/// `dist`, `abvm` and `blwm` are the registry's other default-on positioning
/// features in horizontal text. `abvm` and `blwm` are here for milestone 5 —
/// the Kannada faces of the `SHKNDA` sections put their mark positioning
/// behind `blwm` — and `dist` is reached by no fixture in this corpus at all.
/// Adding the two moved nothing: the twelve sections milestone 2 was graded on
/// use none of them, and their counts are unchanged.
pub const DEFAULT_GPOS_FEATURES: &[Tag] = &[
    Tag::new(b"abvm"),
    Tag::new(b"blwm"),
    Tag::new(b"kern"),
    Tag::new(b"dist"),
    Tag::new(b"curs"),
    Tag::new(b"mark"),
    Tag::new(b"mkmk"),
];

/// The `GSUB` features a run of a joining script turns on, **in stages**.
///
/// Each entry is one stage: every lookup the stage's features name is run over
/// the whole buffer before the next stage starts. That is not how
/// [`DEFAULT_GSUB_FEATURES`] works — there the lookups of every feature are
/// merged and sorted by lookup index, which is the specification's rule for a
/// single application — and the difference is load-bearing here.
///
/// # Why the four forms are four stages and not one
///
/// A Nasta‘līq face's `medi` lookup is written expecting `init` **not** to
/// have run yet, or the other way round; the two rewrite the same glyphs and
/// the face's designer chose an order. Merging them and sorting by lookup
/// index would apply whichever the face happened to lay out first, which is a
/// property of how the table was compiled rather than of what it means. So
/// each is its own stage, in the order the OpenType feature registry lists
/// them for Arabic: isolated, final, medial, initial.
///
/// # The masks
///
/// The four form features carry a per-glyph mask and the rest are global. A
/// glyph's mask says which single form [`crate::arabic::forms`] put it in, so
/// the `init` lookup reaches the first letter of a word and no other.
///
/// # Which of these a fixture adjudicates
///
/// SHARAN-1 — the corpus's one Arabic-script section, and milestone 4's whole
/// bar — uses `ccmp`, `isol`, `fina`, `medi`, `init` and `rlig`. Six of the
/// nine below are therefore evidence. `locl`, `rclt` and `mset` are the
/// registry's word: `locl` and `rclt` are applied by default in horizontal
/// text, `mset` is the legacy Arabic mark-positioning feature, and **no
/// fixture in this corpus reaches any of the three**.
///
/// `liga` and `clig` are deliberately absent rather than forgotten. A joining
/// script's ligatures are `rlig` — required ligatures, which lam-alef is —
/// and the discretionary ones are turned off, because a reader who typed
/// `ZWNJ` between two letters asked for them not to be joined and a `liga`
/// lookup would join them anyway.
pub const JOINING_GSUB_STAGES: &[&[Tag]] = &[
    &[Tag::new(b"ccmp"), Tag::new(b"locl")],
    &[Tag::new(b"isol")],
    &[Tag::new(b"fina")],
    &[Tag::new(b"medi")],
    &[Tag::new(b"init")],
    &[Tag::new(b"rlig"), Tag::new(b"rclt"), Tag::new(b"calt")],
    &[Tag::new(b"mset")],
];

/// One stretch of text in one script at one embedding level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The byte range of the paragraph's text this run covers.
    pub text: Range<usize>,
    /// The script UAX #24 gives it.
    pub script: Script,
    /// The embedding level UAX #9 gave it.
    pub level: Level,
}

impl Run {
    /// Which way the run is set, from the parity of its level.
    #[must_use]
    pub const fn direction(&self) -> Direction {
        self.level.direction()
    }
}

/// A shaped run: glyphs, positions, and what it took to get them.
#[derive(Clone, Debug)]
pub struct ShapedRun {
    glyphs: Vec<ShapedGlyph>,
    direction: Direction,
    units_per_em: u16,
    text: Range<usize>,
    warnings: Vec<Warning>,
}

impl ShapedRun {
    /// The glyphs, in logical order.
    #[must_use]
    pub fn glyphs(&self) -> &[ShapedGlyph] {
        &self.glyphs
    }

    /// Which way the run is set.
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The face's units per em, which every number in [`ShapedRun::glyphs`]
    /// is measured in.
    ///
    /// A consumer scales by `units * size / units_per_em` — one multiply and
    /// one divide, both correctly rounded by IEEE 754 and therefore identical
    /// on every target, which is the side of ruling 4's line the rule allows.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// The byte range of the original text this run covers.
    #[must_use]
    pub fn text(&self) -> Range<usize> {
        self.text.clone()
    }

    /// The sum of the glyphs' horizontal advances.
    #[must_use]
    pub fn advance(&self) -> i32 {
        self.glyphs
            .iter()
            .fold(0i32, |sum, glyph| sum.saturating_add(glyph.x_advance))
    }

    /// Everything the face asked for that this crate refused to do.
    ///
    /// Ruling 10: empty is not the same fact as "it shaped". A run that hit
    /// [`Limits::max_glyphs`] shaped, and says so here.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

/// One face, ready to shape runs.
#[derive(Clone, Copy, Debug)]
pub struct Shaper<'a> {
    face: &'a Sfnt<'a>,
    layout: Layout<'a>,
    language: Option<Tag>,
    limits: Option<Limits>,
    /// `None` is "whatever the run needs" — [`DEFAULT_GSUB_FEATURES`] in one
    /// stage, or [`JOINING_GSUB_STAGES`] where the run joins. `Some` is a
    /// caller who said exactly what they wanted, and is taken at their word.
    gsub_features: Option<&'a [Tag]>,
    gpos_features: Option<&'a [Tag]>,
}

impl<'a> Shaper<'a> {
    /// Reads a face's layout tables, once, for however many runs follow.
    #[must_use]
    pub fn new(face: &'a Sfnt<'a>) -> Self {
        Self {
            face,
            layout: Layout::parse(face),
            language: None,
            limits: None,
            gsub_features: None,
            gpos_features: None,
        }
    }

    /// Features other than [`DEFAULT_GSUB_FEATURES`] and
    /// [`DEFAULT_GPOS_FEATURES`].
    ///
    /// For a caller that wants a ligature off, a stylistic set on, or — as
    /// `tests/text_rendering.rs` does — **nothing at all**, which is how the
    /// discriminating half of that suite's counts is measured: a case that
    /// produces the same answer with every feature switched off is a case that
    /// proves nothing about the features.
    ///
    /// A caller who says this is taken at their word in one further respect:
    /// the run gets **one stage**, even where its script joins. The staging of
    /// [`JOINING_GSUB_STAGES`] is a default plan, and a caller who has named
    /// their own features has replaced the plan rather than reordered it.
    #[must_use]
    pub const fn with_features(mut self, gsub: &'a [Tag], gpos: &'a [Tag]) -> Self {
        self.gsub_features = Some(gsub);
        self.gpos_features = Some(gpos);
        self
    }

    /// The OpenType language system to ask for, such as `TRK ` for Turkish.
    ///
    /// `None` — the default — takes the script's default language system,
    /// which is what a face's own designer wrote for everybody they did not
    /// single out.
    #[must_use]
    pub const fn with_language(mut self, language: Tag) -> Self {
        self.language = Some(language);
        self
    }

    /// Ceilings other than the ones [`Limits::for_glyphs`] would pick.
    #[must_use]
    pub const fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// The face's layout tables, for a caller that wants to look.
    #[must_use]
    pub const fn layout(&self) -> &Layout<'a> {
        &self.layout
    }

    /// Resolves a paragraph, itemizes it, and shapes every run.
    ///
    /// The whole of steps 1 to 3 of `docs/design/shaping.md`'s pipeline. The
    /// [`Paragraph`] comes back with it because the caller needs it for step 5
    /// — reordering each line after breaking — and rebuilding it there would
    /// resolve the same levels twice.
    #[must_use]
    pub fn shape_text(&self, text: &str, direction: BaseDirection) -> (Paragraph, Vec<ShapedRun>) {
        let paragraph = Paragraph::new(text, direction);
        let runs = itemize(text, &paragraph)
            .iter()
            .map(|run| self.shape(text, run))
            .collect();
        (paragraph, runs)
    }

    /// Shapes one run of one script at one level.
    ///
    /// `text` is the whole paragraph and `run.text` the part to shape, so that
    /// [`ShapedGlyph::cluster`] is an offset into the text the caller has
    /// rather than into a substring it would have to add back.
    #[must_use]
    pub fn shape(&self, text: &str, run: &Run) -> ShapedRun {
        let slice = text.get(run.text.clone()).unwrap_or("");
        let mut buffer = Buffer::new();
        buffer.set_direction(run.direction());

        // Whether this run joins is asked of the text and not of the script;
        // `crate::arabic::joins` says why. A caller that asked for its own
        // feature list is taken at its word and gets one stage, because the
        // staging below is the *default* plan for a joining script and not a
        // property of the script itself.
        let plan = self.plan(slice);
        let characters = self.characters(slice, run.text.start, plan == Plan::Universal);
        let from = self.map(&characters, &mut buffer);
        if plan == Plan::Joining {
            self.mark_joining_forms(&characters, &from, &mut buffer);
        }
        if plan == Plan::Universal {
            self.mark_syllables(&characters, &from, &mut buffer);
        }

        let limits = self
            .limits
            .unwrap_or_else(|| Limits::for_glyphs(buffer.len()));
        let mut warnings = Vec::new();
        if let Some(gsub) = self.layout.gsub() {
            let script = self.script_tag(gsub, run.script);
            for (index, stage) in plan.gsub_stages(self.gsub_features).iter().enumerate() {
                // The pair `rphf` was offered has to still be a pair. See
                // [`withdraw_broken_repha_pairs`].
                if plan == Plan::Universal && index == USE_RPHF_STAGE {
                    withdraw_broken_repha_pairs(&mut buffer);
                }
                let wanted: Vec<(Tag, u32)> =
                    stage.iter().map(|tag| (*tag, feature_mask(*tag))).collect();
                let lookups = gsub.lookups_for_masked(script, self.language, &wanted);
                warnings.extend(self.layout.substitute_masked(&mut buffer, &lookups, limits));
                // Which syllables actually got a reph, asked here and nowhere
                // else because here is the only moment the answer is
                // unambiguous; see [`Buffer::set_repha`] and [`mark_repha`].
                if plan == Plan::Universal && index == USE_RPHF_STAGE {
                    mark_repha(&mut buffer);
                }
                // The Universal Shaping Engine's one reordering pause. It sits
                // where it does because the basic features build the conjuncts
                // and the presentation features expect them already in visual
                // order; see `crate::universal`.
                if plan == Plan::Universal && index == USE_REORDER_AFTER {
                    reorder_syllables(&mut buffer);
                }
            }
        }

        // The joiners leave here: after the last `GSUB` stage, so that every
        // lookup saw them, and before the advances are filled, so that a face
        // which gives `ZWNJ` an outline and a width cannot spend either. See
        // [`Buffer::set_ignorable`].
        buffer.delete_ignorable();

        // The advances are filled in **after** substitution and not before,
        // and the ordering is the whole of a bug this crate had for an
        // afternoon. A substitution replaces a glyph and leaves its position
        // alone — deliberately, because `GPOS` may already have moved it — so
        // a buffer given `hmtx` widths first carries the width of the glyph
        // that *used to be* there. text-rendering-tests GSUB-2 is what caught
        // it: an Ethiopic numeral whose medial form is 985 units wide kept the
        // 1149 of the isolated form it was substituted from, and every glyph
        // after it in the line was 164 units too far right.
        for at in 0..buffer.len() {
            let Some(glyph) = buffer.glyph(at).map(|g| g.glyph) else {
                continue;
            };
            let advance = i32::from(self.face.advance(glyph).unwrap_or(0));
            if let Some(slot) = buffer.glyph_mut(at) {
                slot.x_advance = advance;
            }
        }

        if let Some(gpos) = self.layout.gpos() {
            let script = self.script_tag(gpos, run.script);
            let wanted: Vec<(Tag, u32)> = self
                .gpos_features
                .unwrap_or(DEFAULT_GPOS_FEATURES)
                .iter()
                .map(|tag| (*tag, Buffer::GLOBAL))
                .collect();
            let lookups = gpos.lookups_for_masked(script, self.language, &wanted);
            warnings.extend(self.layout.position_masked(
                &mut buffer,
                &lookups,
                limits,
                plan.mark_widths(),
            ));
        } else if plan.mark_widths() == MarkWidths::ZeroByGdef {
            // A face with no `GPOS` still has marks in it, and a combining
            // accent drawn with the advance `hmtx` gave it pushes the pen
            // along and takes the word apart. `Layout::position` does this at
            // the end of positioning; without a `GPOS` nothing would.
            //
            // Conditioned for the same reason the `GPOS` branch is: a Brahmic
            // face with no `GPOS` at all would otherwise keep the behaviour
            // this commit is removing, and its syllables would stack.
            self.layout.zero_marks(&mut buffer);
        }

        ShapedRun {
            glyphs: buffer.glyphs().to_vec(),
            direction: run.direction(),
            units_per_em: self.face.units_per_em,
            text: run.text.clone(),
            warnings,
        }
    }

    /// Which plan this run gets. See [`Plan`].
    fn plan(&self, text: &str) -> Plan {
        if self.gsub_features.is_some() {
            return Plan::AsAsked;
        }
        if arabic::joins(text) {
            return Plan::Joining;
        }
        if text
            .chars()
            .any(|c| universal::category(c) != universal::Category::Other)
        {
            return Plan::Universal;
        }
        Plan::Default
    }

    /// Numbers each glyph with the Brahmic cluster its character belongs to,
    /// and records what the cluster model calls it.
    ///
    /// Both are read off the **character**, here, before any lookup has run —
    /// and both then travel on the glyph. The category has to, because by the
    /// time [`reorder_syllables`] runs there may be no character left to ask:
    /// a conjunct is one glyph standing for three, and asking the text how
    /// many glyphs a syllable ought to have is what made milestone 5 skip the
    /// clusters it most needed to reorder.
    fn mark_syllables(&self, characters: &[(char, u32)], from: &[usize], buffer: &mut Buffer) {
        let letters: Vec<char> = characters.iter().map(|(c, _)| *c).collect();
        let syllables = universal::syllables(&letters);
        let categories: Vec<universal::Category> =
            letters.iter().map(|c| universal::category(*c)).collect();
        let rphf = repha_positions(&syllables, &categories);
        for (at, index) in from.iter().enumerate() {
            if let Some(syllable) = syllables.get(*index) {
                buffer.set_syllable(at, *syllable);
            }
            if let Some(category) = categories.get(*index) {
                buffer.set_category(at, *category);
            }
            if rphf.get(*index) == Some(&true) {
                buffer.set_mask(at, Buffer::GLOBAL | MASK_RPHF);
            }
        }
    }

    /// Puts each glyph in the joining form its character is in.
    ///
    /// The forms are computed over the **text**, before `cmap`, because that
    /// is where the property lives; the mask then travels with the glyph
    /// through every substitution.
    fn mark_joining_forms(&self, characters: &[(char, u32)], from: &[usize], buffer: &mut Buffer) {
        let letters: String = characters.iter().map(|(c, _)| *c).collect();
        let forms = arabic::forms(&letters);
        for (at, index) in from.iter().enumerate() {
            if let Some(form) = forms.get(*index) {
                buffer.set_mask(at, Buffer::GLOBAL | form_mask(*form));
            }
        }
    }

    /// The OpenType script tag to ask this table for.
    ///
    /// The first of [`Script::opentype_tags`] the face actually declares, then
    /// the registry's default rule, then `DFLT`. Asked per table because a
    /// face may declare a script in `GSUB` and not in `GPOS`, and because the
    /// two are consulted separately anyway.
    ///
    /// This is where the version-2 Indic tags earn their place: a face that
    /// declares `knd2` is asked for `knd2`, and a face that declares only
    /// `knda` is asked for `knda`, without either being told about the other.
    fn script_tag(&self, table: &crate::LayoutTable<'a>, script: Script) -> Tag {
        let scripts = table.scripts();
        for tag in script.opentype_tags() {
            if scripts.find(tag).is_some() {
                return tag;
            }
        }
        let default = script.opentype_tag();
        if scripts.find(default).is_some() {
            return default;
        }
        Tag::DEFAULT_SCRIPT
    }

    /// `cmap`: text to glyph indices, with the cluster each one came from.
    ///
    /// # Variation selectors, and the one `cmap` subtable this crate reads
    /// itself
    ///
    /// `tinker_pdf_font::Sfnt::glyph_for_char` reads `cmap` formats 0, 4, 6
    /// and 12, which is every format a *renderer* needs: a PDF names glyphs by
    /// code and a variation sequence never reaches it. A **shaper** is where a
    /// variation selector is consumed — it selects a glyph and then vanishes,
    /// exactly like a joining form does — so format 14, the Unicode Variation
    /// Sequences subtable, is read here.
    ///
    /// That is the one place this crate parses a `cmap` subtable rather than
    /// asking the font crate, and the boundary is deliberate: the base
    /// mapping stays `Sfnt`'s, so there is still one reader of formats 0, 4, 6
    /// and 12 in this workspace.
    ///
    /// A selector the face has nothing to say about is **dropped**, not mapped
    /// to `.notdef`: `FVS` and `VS1`–`VS256` are `Default_Ignorable_Code_Point`
    /// and a reader that drew a box for one would put a box in the middle of a
    /// Han sentence.
    fn map(&self, characters: &[(char, u32)], buffer: &mut Buffer) -> Vec<usize> {
        let uvs = variation_subtable(self.face);
        let mut from = Vec::with_capacity(characters.len());
        let mut at = 0usize;
        while let Some((c, cluster)) = characters.get(at).copied() {
            at += 1;
            if is_variation_selector(c) {
                // A selector with no base before it selects nothing. It is
                // still ignorable, so it is dropped rather than drawn.
                continue;
            }
            let selector = characters
                .get(at)
                .map(|(next, _)| *next)
                .filter(|next| is_variation_selector(*next));
            let mut glyph = None;
            if let Some(selector) = selector {
                at += 1;
                glyph = uvs.and_then(|data| variation_glyph(data, c, selector, self.face));
            }
            let glyph = glyph.or_else(|| self.face.glyph_for_char(c)).unwrap_or(0);
            buffer.push(glyph, cluster);
            // A joiner is pushed like anything else and marked for deletion at
            // the end of `GSUB`. It has to be *in* the buffer for the whole of
            // substitution, because blocking a ligature is the whole of what
            // it is for; see [`Buffer::set_ignorable`].
            if is_joiner(c) {
                buffer.set_ignorable(buffer.len().saturating_sub(1), true);
            }
            from.push(at - 1 - usize::from(selector.is_some()));
        }
        from
    }

    /// The characters this run is shaped from, each with the cluster it stands
    /// for.
    ///
    /// Ordinarily this is just the run's own characters and their byte
    /// offsets. For a Brahmic run it is those characters **canonically
    /// decomposed**, which is the one preprocessing step the cluster model
    /// cannot do without.
    ///
    /// # Why decomposition is a shaping step and not a nicety
    ///
    /// `U+1B40 BALINESE VOWEL SIGN TALING TEDUNG` is
    /// `Indic_Positional_Category` `Left_And_Right`: it is drawn on **both**
    /// sides of its consonant. As one character it is neither `Left` nor
    /// `Right`, so `crate::universal::category` can only call it a mark that
    /// stays where it is, and its left half stays where it was typed. Its
    /// canonical decomposition — `U+1B3E` (`Left`) and `U+1B35` (`Right`) — is
    /// two characters with two positions, and the reordering pause can then
    /// move the first and leave the second.
    ///
    /// Every part keeps the **whole** character's cluster, so a decomposition
    /// invents no offsets the caller's text does not have and `/ToUnicode`
    /// still rebuilds the original.
    ///
    /// # Three limits, each deliberate
    ///
    /// - It is applied only where the cluster model has an opinion — a
    ///   character `crate::universal::category` calls anything but `Other`.
    ///   Latin's precomposed accents are left alone: nothing here needs their
    ///   halves, and decomposing them would change every Latin run's glyphs
    ///   for no fixture's benefit.
    /// - It is applied only when the face has a glyph for **every** part.
    ///   Decomposing into a `.notdef` would replace a drawable character with
    ///   an undrawable one, which is worse than not decomposing.
    /// - Canonical **ordering** — the `Canonical_Combining_Class` sort that
    ///   makes a decomposition NFD rather than merely decomposed — is not
    ///   done. Nothing in the vendored corpus reaches a Brahmic cluster whose
    ///   parts are out of canonical order; `docs/features/fonts.md` records
    ///   it, and Hangul, whose decomposition is algorithmic rather than
    ///   tabulated, is not decomposed at all.
    fn characters(&self, text: &str, base: usize, decompose: bool) -> Vec<(char, u32)> {
        let mut out = Vec::with_capacity(text.len());
        for (at, c) in text.char_indices() {
            let cluster = u32::try_from(base.saturating_add(at)).unwrap_or(u32::MAX);
            let parts = decompose
                .then(|| unicode::canonical_decomposition(c))
                .flatten()
                .filter(|parts| {
                    parts
                        .iter()
                        .all(|part| self.face.glyph_for_char(*part).is_some_and(|g| g != 0))
                });
            match parts {
                Some(parts) => out.extend(parts.iter().map(|part| (*part, cluster))),
                None => out.push((c, cluster)),
            }
        }
        out
    }
}

/// Which of the three plans a run gets.
///
/// The choice is made from the **text**, never from a transcribed list of
/// scripts, for the reason `crate::arabic::joins` gives: a list of scripts here
/// would be somebody else's list and would be wrong the day Unicode gives an
/// existing script a joining letter or a dependent vowel, and the properties
/// themselves cannot be.
///
/// A run can satisfy both tests — Arabic-script text with a Brahmic character
/// in it does — and joining wins, because the joining forms are what a reader
/// of that run would notice first and because no fixture in the corpus mixes
/// them. That precedence is a choice and is recorded as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    /// One stage, [`DEFAULT_GSUB_FEATURES`]: Latin, Greek, Han, Hebrew.
    Default,
    /// [`JOINING_GSUB_STAGES`], and a form per glyph.
    Joining,
    /// [`USE_GSUB_STAGES`], syllables, and one reordering pause.
    Universal,
    /// A caller who named their own features, taken at their word.
    AsAsked,
}

impl Plan {
    /// Whether this plan zeroes the advance of a glyph `GDEF` calls a mark.
    ///
    /// The two families want opposite answers and the corpus is what says so.
    ///
    /// A **joining** script's marks are drawn on top of the letter they belong
    /// to, and a face that gives one a real `hmtx` advance would push the pen
    /// along and take the word apart -- so those are zeroed. Every `GDEF` mark
    /// in `TestShapeAran` and in the three `TestGPOS` faces has an advance of
    /// zero already, which is the same statement made by the faces.
    ///
    /// A **Brahmic** script's are not marks in that sense. Thirteen of
    /// `NotoSansKannada`'s fourteen `GDEF` marks are *spacing* matras with real
    /// advances -- U+0CC2 UU is 1526 units at 2048 per em -- and
    /// text-rendering-tests SHKNDA-3's expected positions are exactly the
    /// cumulative `hmtx` sum with those advances intact. Zeroing them stacked
    /// every glyph of a syllable at one x.
    ///
    /// # What this is not conditioned on, and why
    ///
    /// `General_Category` is the obvious refinement -- zero `Mn`, keep `Mc` --
    /// and it is **wrong**. U+0CBF (advance 669) and U+0CC6 (712) are `Mn`, and
    /// the corpus counts their advances. It was tried and refuted before this
    /// was written.
    ///
    /// Nor on a list of scripts: the plan is already chosen from the text (see
    /// [`Plan`]), so conditioning on the plan inherits that and adds no list.
    const fn mark_widths(self) -> MarkWidths {
        match self {
            // 14.7's own answer for a caller who named their own features: the
            // default, because a caller asking for `liga` on Latin is not
            // asking about matras.
            Plan::Default | Plan::Joining | Plan::AsAsked => MarkWidths::ZeroByGdef,
            Plan::Universal => MarkWidths::AsSupplied,
        }
    }

    /// The stages this plan runs, in order.
    fn gsub_stages(self, asked: Option<&[Tag]>) -> Vec<&[Tag]> {
        match self {
            Plan::AsAsked => vec![asked.unwrap_or(DEFAULT_GSUB_FEATURES)],
            Plan::Default => vec![DEFAULT_GSUB_FEATURES],
            Plan::Joining => JOINING_GSUB_STAGES.to_vec(),
            Plan::Universal => USE_GSUB_STAGES.to_vec(),
        }
    }
}

/// The `GSUB` features a Brahmic run turns on, in stages.
///
/// The Universal Shaping Engine's own order. Each entry is a stage for the
/// same reason [`JOINING_GSUB_STAGES`]'s are: the face's `blwf` lookup is
/// written expecting `rphf` to have finished, and merging them would apply
/// whichever the compiler happened to lay out first.
///
/// # The four groups
///
/// 1. **Normalisation.** `locl`, `ccmp`, `nukt`, `akhn` — get the cluster into
///    the shape the rest of the plan expects.
/// 2. **Reordering group.** `rphf` then `pref`, each its own stage because
///    each records something the next depends on.
/// 3. **Orthographic unit shaping.** The seven that build the conjuncts:
///    `rkrf`, `abvf`, `blwf`, `half`, `pstf`, `vatu`, `cjct`. The reordering
///    pause is **after** this group, at [`USE_REORDER_AFTER`].
/// 4. **Presentation.** `abvs`, `blws`, `haln`, `pres`, `psts`, applied to a
///    cluster that is by then in the order it will be drawn in.
///
/// The topographical group — `isol`, `init`, `medi`, `fina` — is deliberately
/// **absent**. USE applies them to scripts that join, and the four masks this
/// crate has are set by `crate::arabic::forms`, which a Brahmic run does not
/// run; requesting them unmasked would apply a joining form to every glyph of
/// every syllable. No face in the vendored corpus declares them for a Brahmic
/// script, so nothing here is lost that a fixture could see, and the omission
/// is named in `docs/features/fonts.md` rather than left to be discovered.
///
/// The default features are the last stage rather than the first, so that a
/// Brahmic face's `liga` and `calt` see the cluster after it has been built.
pub const USE_GSUB_STAGES: &[&[Tag]] = &[
    &[
        Tag::new(b"locl"),
        Tag::new(b"ccmp"),
        Tag::new(b"nukt"),
        Tag::new(b"akhn"),
    ],
    &[Tag::new(b"rphf")],
    &[Tag::new(b"pref")],
    &[
        Tag::new(b"rkrf"),
        Tag::new(b"abvf"),
        Tag::new(b"blwf"),
        Tag::new(b"half"),
        Tag::new(b"pstf"),
        Tag::new(b"vatu"),
        Tag::new(b"cjct"),
    ],
    &[
        Tag::new(b"abvs"),
        Tag::new(b"blws"),
        Tag::new(b"haln"),
        Tag::new(b"pres"),
        Tag::new(b"psts"),
    ],
    DEFAULT_GSUB_FEATURES,
];

/// Moves every pre-base glyph of every syllable in front of its base.
///
/// # The permutation is over glyphs, and that is the whole of the fix
///
/// This was computed over the **characters** of each syllable and applied to
/// the *glyphs* carrying that syllable's number, which is the same thing only
/// while substitution has not changed how many glyphs a character stands for.
/// Where it had — a conjunct built out of three characters, a decomposition
/// that made two glyphs out of one — the two lengths differed and the syllable
/// was **left alone**, so a face that forms its conjuncts before the
/// reordering pause got no reordering in exactly the clusters where it
/// mattered. text-rendering-tests SHKNDA-2 is where that cost the most.
///
/// The categories now travel on the glyphs ([`Buffer::set_category`]), so
/// there is no length to disagree about: whatever `GSUB` did to the syllable,
/// every glyph in it still says what the cluster model calls it, and a
/// ligature says what its first component did.
fn reorder_syllables(buffer: &mut Buffer) {
    for range in buffer.syllable_ranges() {
        let of_syllable: Vec<universal::Category> = range
            .clone()
            .filter_map(|at| buffer.props_category(at))
            .collect();
        if of_syllable.len() != range.len() {
            continue;
        }
        if let Some(order) = universal::reorder(&of_syllable) {
            buffer.reorder(range.clone(), &order);
        }
        // And then the reph, which is a different move for a different reason
        // and is therefore not `universal::reorder`'s: that function answers
        // the cluster model's question about categories, and this one is about
        // what a face's `rphf` did. See [`move_repha`].
        move_repha(buffer, range);
    }
}

/// Which stage of [`USE_GSUB_STAGES`] the reordering pause follows.
///
/// Index 3, the orthographic-unit-shaping group. Before it a pre-base vowel is
/// still where it was typed, which is what the conjunct-forming lookups are
/// written against; after it the cluster is in the order it will be drawn in,
/// which is what the presentation lookups are written against.
const USE_REORDER_AFTER: usize = 3;

/// Which stage of [`USE_GSUB_STAGES`] is `rphf`'s.
///
/// Index 1, its own stage, and [`the_stage_indices_name_the_stages_they_mean`]
/// is what stops this and [`USE_REORDER_AFTER`] drifting off the array they
/// index.
const USE_RPHF_STAGE: usize = 1;

/// Where `rphf` may fire, as a flag per character.
///
/// # A repha is a consonant that has a base to sit on
///
/// `rphf` rewrites a syllable-initial `RA` and its halant into a reph — the
/// mark drawn above the syllable — and the *whole* of what makes it a reph
/// rather than a dead consonant is that there is something after it for the
/// syllable to be about. A word-final `RA` + halant is not a reph; it is a
/// consonant with its vowel killed, and the face's `haln` lookup is what draws
/// it.
///
/// This crate asked the face for `rphf` over every syllable, so
/// text-rendering-tests `SHKNDA-2/7` — `ಜಾ಼ಕಿರ್`, which ends U+0CB0 U+0CCD —
/// got the reph gid94 where the fixture wants the halant form gid193.
///
/// So the condition is stated here and carried as a mask: the first two
/// characters of the syllable are a base and a halant, and a base follows them
/// **inside the same syllable**. Both of the two are marked, because a
/// feature's mask is checked against every glyph of a rule's *input* and
/// `rphf` is a ligature over the pair; see [`crate::apply`]'s `Skipper::mask`.
///
/// # What this is not
///
/// It does not say which consonant `RA` is, and it must not: that is the
/// face's own `rphf` coverage table, which is the only place in the system
/// that knows what a face means by a repha. This narrows *where* the lookup is
/// offered a position, never *which* glyph it accepts. A face whose `rphf`
/// covers nothing loses nothing.
fn repha_positions(syllables: &[u16], categories: &[universal::Category]) -> Vec<bool> {
    let mut out = vec![false; syllables.len()];
    let mut at = 0usize;
    while at < syllables.len() {
        let syllable = syllables.get(at).copied().unwrap_or(0);
        let mut end = at;
        while syllables.get(end) == Some(&syllable) {
            end = end.saturating_add(1);
        }
        let after = at.saturating_add(2);
        if syllable != 0
            && categories.get(at) == Some(&universal::Category::Base)
            && categories.get(at.saturating_add(1)) == Some(&universal::Category::Halant)
            && categories
                .get(after..end)
                .is_some_and(|rest| rest.contains(&universal::Category::Base))
        {
            for flag in out.get_mut(at..after).unwrap_or_default() {
                *flag = true;
            }
        }
        at = end.max(at.saturating_add(1));
    }
    out
}

/// Takes [`MASK_RPHF`] back from a syllable whose pair is no longer a pair.
///
/// # The normalisation stage can eat the halant, and this is what it costs
///
/// [`repha_positions`] marks two *characters*, and by the time `rphf` runs the
/// stage before it has had a turn at them. `akhn` is the one that matters:
/// KA + VIRAMA + SSA is the classic akhand ligature and every Indic face has
/// it, and the one glyph it produces keeps KA's props — including the bit.
///
/// The syllable then looks exactly like a reph that has already formed: one
/// glyph carrying the bit followed by one that does not. text-rendering-tests
/// `SHKNDA-3/31` is where that showed: `ಕ್ಷಿ` became gid282 with the bit still
/// on it, [`mark_repha`] called it a reph, and moving it to the end of the
/// syllable stopped `ಕ್ಷ` and its `I` ligating into gid285.
///
/// So the bit is withdrawn from any syllable whose first *two* glyphs do not
/// both still carry it, immediately before the stage that would use it. That
/// makes both readings of the bit exact: `rphf` is offered only a pair that is
/// still two glyphs, and one glyph carrying the bit afterwards means the
/// lookup fired.
fn withdraw_broken_repha_pairs(buffer: &mut Buffer) {
    for range in buffer.syllable_ranges() {
        let second = range.start.saturating_add(1);
        let intact = second < range.end
            && buffer.props_mask(range.start) & MASK_RPHF != 0
            && buffer.props_mask(second) & MASK_RPHF != 0;
        if intact {
            continue;
        }
        for at in range {
            let mask = buffer.props_mask(at) & !MASK_RPHF;
            buffer.set_mask(at, mask);
        }
    }
}

/// Records, per syllable, whether `rphf` actually produced a reph.
///
/// # The question the mask cannot answer on its own
///
/// [`repha_positions`] says where the lookup was *offered* a position. Whether
/// it took one is the face's answer, and the buffer is where it is written:
/// the pair went in as two glyphs both carrying [`MASK_RPHF`], and a reph
/// comes out as **one** glyph carrying it with the halant gone.
///
/// So the test is exactly that — the syllable's first glyph carries the bit
/// and its second does not — and it is asked immediately after the `rphf`
/// stage, at [`USE_RPHF_STAGE`], because that is the only moment it is
/// unambiguous. `half` in the stage after next forms the same shape out of the
/// same two characters and is not a reph; asking at the reordering pause would
/// call it one.
fn mark_repha(buffer: &mut Buffer) {
    for range in buffer.syllable_ranges() {
        let first = range.start;
        let second = first.saturating_add(1);
        let fired = buffer.props_mask(first) & MASK_RPHF != 0
            && second < range.end
            && buffer.props_mask(second) & MASK_RPHF == 0;
        if fired {
            buffer.set_repha(first, true);
        }
    }
}

/// Moves a reph to the end of its syllable.
///
/// # One case says this, and it says it exactly
///
/// text-rendering-tests `SHKNDA-2/12` is `ಮಾರ್ಚ್` — `MA AA RA VIRAMA CHA
/// VIRAMA` — and its second syllable reaches the reordering pause as gid94
/// (the reph `rphf` just made), gid25 (`CHA`) and gid70 (the virama). The
/// fixture expects gid172 then gid94: the reph **last**, and `haln` in the
/// stage after the pause is what turns the remaining `CHA` and virama into
/// gid172. Moving the reph to the end is what puts those two next to each
/// other for it.
///
/// # What is claimed, and what is a guess
///
/// The Indic model gives a face several places a reph may be repositioned to —
/// after the base, after the first matra, before a post-base matra, at the end
/// of the syllable — and reads which from the face's own tables. **This
/// implements one of them and does not read anything.** `SHKNDA-2/12` is the
/// only case in either vendored corpus with a reph in it, so "the end of the
/// syllable" is what one fixture says and not a position this crate chose
/// between alternatives it could see. A face wanting one of the others has no
/// fixture here and would fail here first, which is the honest state to leave
/// it in.
fn move_repha(buffer: &mut Buffer, range: Range<usize>) {
    let Some(at) = range.clone().find(|at| buffer.props_repha(*at)) else {
        return;
    };
    if at.saturating_add(1) >= range.end {
        return;
    }
    let mut order: Vec<usize> = (0..range.len()).collect();
    let lifted = order.remove(at.saturating_sub(range.start));
    order.push(lifted);
    buffer.reorder(range, &order);
}

/// The mask bit of each joining form.
///
/// Bit 0 is [`Buffer::GLOBAL`] and is on every glyph, so these start at bit 1.
/// Four bits and no more: a glyph is in exactly one of the four forms, or in
/// none.
const MASK_ISOL: u32 = 1 << 1;
/// See [`MASK_ISOL`].
const MASK_FINA: u32 = 1 << 2;
/// See [`MASK_ISOL`].
const MASK_MEDI: u32 = 1 << 3;
/// See [`MASK_ISOL`].
const MASK_INIT: u32 = 1 << 4;

/// The mask bit of a position `rphf` is offered. See [`repha_positions`].
///
/// Bit 5, and it cannot collide with the four above: a run gets **one** plan,
/// [`Plan::Joining`] is the only one that sets a form bit and [`Plan::Universal`]
/// is the only one that sets this, so no glyph in any run carries bits from
/// both sets.
const MASK_RPHF: u32 = 1 << 5;

/// The mask a glyph in this form carries.
const fn form_mask(form: arabic::Form) -> u32 {
    match form {
        arabic::Form::Isolated => MASK_ISOL,
        arabic::Form::Final => MASK_FINA,
        arabic::Form::Medial => MASK_MEDI,
        arabic::Form::Initial => MASK_INIT,
        arabic::Form::None => 0,
    }
}

/// The mask a feature is requested under.
///
/// The four joining features are restricted to the glyphs in their form;
/// everything else is global. The match is on the tag rather than on a flag
/// beside it in [`JOINING_GSUB_STAGES`], so that a caller who names `fina` in
/// [`Shaper::with_features`] gets the same restriction the default plan would
/// have given it — one answer to "what does `fina` mean", not two.
const fn feature_mask(tag: Tag) -> u32 {
    match tag.0 {
        t if t == Tag::new(b"isol").0 => MASK_ISOL,
        t if t == Tag::new(b"fina").0 => MASK_FINA,
        t if t == Tag::new(b"medi").0 => MASK_MEDI,
        t if t == Tag::new(b"init").0 => MASK_INIT,
        // And the same restriction for `rphf`, for the same reason and by the
        // same route: the face's lookup covers every `RA` in the font and the
        // *position* is what says whether one is a repha. See
        // [`repha_positions`].
        t if t == Tag::new(b"rphf").0 => MASK_RPHF,
        _ => Buffer::GLOBAL,
    }
}

/// UAX #24 itemization: runs of one script at one embedding level.
///
/// # What the rule is
///
/// `Common` and `Inherited` characters have no script of their own — a space,
/// a full stop, a combining accent — so they take the script of what they
/// follow, and, at the start of the text, of what they precede. A run of
/// nothing else stays `Common`, which resolves to an OpenType tag no face
/// declares and therefore to `DFLT`, which is the right answer for a line of
/// mathematical symbols.
///
/// # What it is not
///
/// UAX #24's full algorithm uses `Script_Extensions` — the set of scripts a
/// character is used in, which is a different and larger property than
/// `Script` — and resolves paired brackets to a common script. Neither is
/// vendored here. The consequence is a run boundary in the wrong place for a
/// character several scripts share, such as U+0964 DEVANAGARI DANDA between
/// Devanagari and Bengali; every script that reaches is scheduled for
/// milestone 5, and none of this milestone's fixtures contains one.
#[must_use]
pub fn itemize(text: &str, paragraph: &Paragraph) -> Vec<Run> {
    let mut scripts: Vec<(usize, Script)> = Vec::new();
    for (at, c) in text.char_indices() {
        scripts.push((at, unicode::script(c)));
    }
    // Forwards: a Common or Inherited character joins what precedes it.
    let mut carried = Script::Common;
    for (_, script) in &mut scripts {
        if matches!(*script, Script::Common | Script::Inherited) {
            *script = carried;
        } else {
            carried = *script;
        }
    }
    // Backwards: and the ones at the very start join what follows, since
    // there was nothing in front of them to join.
    let mut carried = Script::Common;
    for (_, script) in scripts.iter_mut().rev() {
        if matches!(*script, Script::Common | Script::Inherited) {
            *script = carried;
        } else {
            carried = *script;
        }
    }

    let levels = paragraph.levels();
    let mut runs: Vec<Run> = Vec::new();
    for (index, (at, script)) in scripts.iter().enumerate() {
        let level = levels.get(index).copied().unwrap_or(paragraph.base_level());
        match runs.last_mut() {
            Some(run) if run.script == *script && run.level == level => run.text.end = *at,
            _ => runs.push(Run {
                text: *at..*at,
                script: *script,
                level,
            }),
        }
    }
    if let Some(run) = runs.last_mut() {
        run.text.end = text.len();
    }
    // The `end` of every run but the last was set to the start of the
    // character that ended it, which is where the next run begins.
    for index in 0..runs.len().saturating_sub(1) {
        runs[index].text.end = runs[index + 1].text.start;
    }
    runs
}

/// `ZWJ` and `ZWNJ`, and nothing else.
///
/// # Which property, and which one deliberately not
///
/// `Indic_Syllabic_Category`'s `Joiner` and `Non_Joiner`, which this crate
/// already parses for [`universal::category`] — so the predicate costs no new
/// table and cannot drift from the one the cluster model reads.
///
/// The obvious alternative is `Default_Ignorable_Code_Point`, which is the
/// property a shaper is *usually* written against and which covers these two
/// along with the variation selectors, the Mongolian free variation selectors,
/// `U+00AD SOFT HYPHEN` and some seventy more. It is deliberately **not** used
/// here: nothing in either vendored corpus reaches a default-ignorable
/// character that is not one of these two, so vendoring a hundred-odd-entry
/// property to widen a predicate no fixture exercises would be a table nobody
/// could adjudicate. The narrower predicate is named as narrower, in
/// `docs/features/fonts.md`, rather than being quietly the whole answer.
///
/// The variation selectors *are* default-ignorable and *are* dropped — in
/// [`Shaper::map`], where they are consumed rather than deleted, because a
/// selector chooses a glyph and a joiner does not.
fn is_joiner(c: char) -> bool {
    matches!(
        unicode::indic_syllabic(c),
        unicode::IndicSyllabic::Joiner | unicode::IndicSyllabic::NonJoiner
    )
}

/// The variation selectors: `VS1`–`VS16`, and the 240 ideographic ones.
///
/// The Mongolian free variation selectors at U+180B–U+180F are deliberately
/// not here: they are `Mn`, they take part in Mongolian shaping rather than in
/// `cmap` format 14, and treating one as a selector would swallow it.
const fn is_variation_selector(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// The face's `cmap` format 14 subtable, if it has one.
fn variation_subtable<'a>(face: &Sfnt<'a>) -> Option<Bytes<'a>> {
    let cmap = Bytes::new(face.table(u32::from_be_bytes(*b"cmap"))?);
    let count = usize::from(cmap.u16(2)?);
    for n in 0..count.min(64) {
        let at = 4usize.checked_add(n.checked_mul(8)?)?;
        let sub = cmap.offset32(at.checked_add(4)?)?;
        if sub.u16(0) == Some(14) {
            return Some(sub);
        }
    }
    None
}

/// One `UnicodeValue`, the 24-bit code point a format 14 subtable stores.
fn u24(data: Bytes<'_>, at: usize) -> Option<u32> {
    Some(u32::from(data.u8(at)?) << 16 | u32::from(data.u16(at.checked_add(1)?)?))
}

/// A `(base, selector)` pair's glyph, through `cmap` format 14.
///
/// Three outcomes, and the difference between the second and the third is the
/// whole reason this subtable exists:
///
/// - the pair is a **non-default** sequence, and the subtable names the glyph;
/// - the pair is a **default** sequence, meaning "the ordinary glyph for the
///   base is already the right one", so the base `cmap` answers;
/// - the subtable has never heard of the pair, and the selector is ignored,
///   which also means the base `cmap` answers.
fn variation_glyph(data: Bytes<'_>, base: char, selector: char, face: &Sfnt<'_>) -> Option<u16> {
    let records = data.u32(6)?;
    let wanted = u32::from(selector);
    // The records are sorted by selector, and there are at most 256 of them.
    let mut found = None;
    for n in 0..records.min(1024) {
        let at = 10usize.checked_add(usize::try_from(n).ok()?.checked_mul(11)?)?;
        if u24(data, at)? == wanted {
            found = Some(at);
            break;
        }
    }
    let at = found?;
    let default_uvs = data.u32(at.checked_add(3)?)?;
    let non_default_uvs = data.u32(at.checked_add(7)?)?;
    let code = u32::from(base);

    if non_default_uvs != 0 {
        let table = data.at(usize::try_from(non_default_uvs).ok()?)?;
        let count = table.u32(0)?;
        for n in 0..count.min(0x0011_0000) {
            let entry = 4usize.checked_add(usize::try_from(n).ok()?.checked_mul(5)?)?;
            if u24(table, entry)? == code {
                return table.u16(entry.checked_add(3)?);
            }
        }
    }
    if default_uvs != 0 {
        let table = data.at(usize::try_from(default_uvs).ok()?)?;
        let count = table.u32(0)?;
        for n in 0..count.min(0x0011_0000) {
            let entry = 4usize.checked_add(usize::try_from(n).ok()?.checked_mul(4)?)?;
            let start = u24(table, entry)?;
            let extra = u32::from(table.u8(entry.checked_add(3)?)?);
            if code >= start && code <= start.saturating_add(extra) {
                // A default sequence says the base glyph is already right.
                return face.glyph_for_char(base);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{is_joiner, is_variation_selector, itemize, Plan};
    use crate::bidi::{BaseDirection, Paragraph};
    use crate::common::Tag;
    use crate::unicode::Script;
    use crate::MarkWidths;

    /// Two stage indices point into [`USE_GSUB_STAGES`], and this is what
    /// stops either drifting off it.
    ///
    /// Both are plain integers because the array is a `const` of slices and
    /// searching it at runtime for a tag would be work done per run for an
    /// answer that never changes. The cost of that is exactly this test.
    #[test]
    fn the_stage_indices_name_the_stages_they_mean() {
        assert_eq!(
            super::USE_GSUB_STAGES.get(super::USE_RPHF_STAGE),
            Some(&[Tag::new(b"rphf")].as_slice()),
            "USE_RPHF_STAGE no longer indexes the rphf stage"
        );
        assert!(
            super::USE_GSUB_STAGES
                .get(super::USE_REORDER_AFTER)
                .is_some_and(|stage| stage.contains(&Tag::new(b"blwf"))),
            "USE_REORDER_AFTER no longer indexes the orthographic-unit group"
        );
        const { assert!(super::USE_RPHF_STAGE < super::USE_REORDER_AFTER) };
    }

    /// Where `rphf` is offered a position, over the four shapes that matter.
    ///
    /// The pair that adjudicates it is text-rendering-tests `SHKNDA-2/7`
    /// against `SHKNDA-2/12`: both end a syllable with U+0CB0 U+0CCD, and only
    /// one of them is a reph, because only one of them has a base after it.
    /// Offering the lookup at both cost the first case and got the second
    /// right for the wrong reason.
    #[test]
    fn rphf_is_offered_only_where_the_syllable_has_a_base_for_it() {
        fn offered(text: &str) -> Vec<bool> {
            let letters: Vec<char> = text.chars().collect();
            let categories: Vec<crate::universal::Category> = letters
                .iter()
                .map(|c| crate::universal::category(*c))
                .collect();
            super::repha_positions(&crate::universal::syllables(&letters), &categories)
        }

        // KANNADA RA, VIRAMA, KA: a repha, and both of its two characters are
        // marked, because the mask is checked against every glyph of the
        // ligature's input.
        assert_eq!(offered("\u{0CB0}\u{0CCD}\u{0C95}"), [true, true, false]);
        // The same two characters with nothing after them: a dead consonant,
        // which `haln` draws. `SHKNDA-2/7` ends this way.
        assert_eq!(offered("\u{0CB0}\u{0CCD}"), [false, false]);
        // `SHKNDA-2/12`'s second syllable — RA VIRAMA CHA VIRAMA — is a repha
        // in front of a base that is itself dead.
        assert_eq!(
            offered("\u{0CB0}\u{0CCD}\u{0C9A}\u{0CCD}"),
            [true, true, false, false]
        );
        // A base with no halant after it offers nothing, and neither does a
        // run with no Brahmic character in it at all.
        assert_eq!(offered("\u{0CB0}\u{0C95}"), [false, false]);
        assert_eq!(offered("ab"), [false, false]);
    }

    /// The two joiners, and the width of the predicate stated as a limit.
    ///
    /// The behavioural half of this is in `tests/text_rendering.rs` and is a
    /// pair rather than a single case, because a joiner has to do two opposite
    /// things: `SHKNDA-3/31` says it must not reach the output, and
    /// `SHLANA-5/10` and `SHLANA-5/12` say it must still block a ligature on
    /// the way. Deleting it at `cmap` time satisfies the first and breaks the
    /// second, and that injection costs exactly those two cases.
    ///
    /// What is here instead is the **narrowness**. Every character below is
    /// `Default_Ignorable_Code_Point` and none of them is deleted, which is
    /// the whole difference between this predicate and the one a shaper is
    /// usually written against; [`is_joiner`] says why the wider property is
    /// not vendored.
    #[test]
    fn the_only_ignorable_characters_are_the_two_joiners() {
        assert!(is_joiner('\u{200C}'), "ZWNJ");
        assert!(is_joiner('\u{200D}'), "ZWJ");
        for c in [
            '\u{00AD}', // SOFT HYPHEN
            '\u{200B}', // ZERO WIDTH SPACE
            '\u{2060}', // WORD JOINER
            '\u{180B}', // MONGOLIAN FREE VARIATION SELECTOR ONE
            '\u{FE00}', // VARIATION SELECTOR-1, consumed in `map` instead
            '\u{0CBE}', // KANNADA VOWEL SIGN AA, which draws
            'a',
        ] {
            assert!(!is_joiner(c), "{c:?} is not one of the two");
        }
    }

    /// Every plan's mark-width answer, tabled.
    ///
    /// A table test and not a behavioural one, and the reason is a finding
    /// rather than a shortcut. **No face in either conformance corpus can tell
    /// the two answers apart for a non-Brahmic run.** `TestShapeAran` — the
    /// only Arabic face here — and the three `TestGPOS` faces have *zero*
    /// `GDEF` marks with a non-zero `hmtx` advance between them, so
    /// `ZeroByGdef` and `AsSupplied` produce identical output for every one of
    /// their cases. Setting `AsSupplied` for all four plans was injected and
    /// **failed nothing at all**.
    ///
    /// So the joining half of this condition rests on the argument in
    /// [`Plan::mark_widths`] and on nothing that runs. Real Arabic faces do
    /// give marks non-zero advances — it is why [`MarkWidths::ZeroByGdef`] is
    /// the default and what its own doc comment describes — but this
    /// repository has no such face, and saying so is worth more than a guard
    /// that looks measured and is not.
    ///
    /// What this test does buy: a future edit that flips a plan's answer fails
    /// here and has to argue with the sentence above.
    #[test]
    fn each_plan_answers_the_mark_width_question_for_its_own_reason() {
        assert_eq!(Plan::Universal.mark_widths(), MarkWidths::AsSupplied);
        for plan in [Plan::Default, Plan::Joining, Plan::AsAsked] {
            assert_eq!(plan.mark_widths(), MarkWidths::ZeroByGdef, "{plan:?}");
        }
    }

    fn runs(text: &str) -> Vec<(std::ops::Range<usize>, Script, u8)> {
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        itemize(text, &paragraph)
            .into_iter()
            .map(|run| (run.text, run.script, run.level.number()))
            .collect()
    }

    #[test]
    fn one_script_is_one_run() {
        assert_eq!(runs("hello"), vec![(0..5, Script::Latin, 0)]);
    }

    #[test]
    fn a_space_joins_the_word_before_it_rather_than_starting_a_run() {
        assert_eq!(runs("a a"), vec![(0..3, Script::Latin, 0)]);
    }

    #[test]
    fn a_leading_common_character_joins_what_follows() {
        // The full stop has no script of its own and there is nothing before
        // it, so it takes Ethiopic from the character after it.
        assert_eq!(runs(".\u{1208}"), vec![(0..4, Script::Ethiopic, 0)]);
    }

    #[test]
    fn two_scripts_are_two_runs() {
        let out = runs("a\u{1208}");
        assert_eq!(
            out,
            vec![(0..1, Script::Latin, 0), (1..4, Script::Ethiopic, 0)]
        );
    }

    /// A change of embedding level cuts a run even where the script does not,
    /// because a run is what one call to `GPOS` sees and two directions in one
    /// call would kern across the seam.
    #[test]
    fn a_change_of_level_cuts_a_run() {
        let out = runs("a\u{05D0}");
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0].2, 0);
        assert_eq!(out[1].2, 1);
    }

    #[test]
    fn text_with_nothing_in_it_has_no_runs() {
        assert!(runs("").is_empty());
    }

    #[test]
    fn the_selectors_are_the_two_blocks_and_not_the_mongolian_ones() {
        assert!(is_variation_selector('\u{FE00}'));
        assert!(is_variation_selector('\u{FE0F}'));
        assert!(is_variation_selector('\u{E0100}'));
        assert!(is_variation_selector('\u{E01EF}'));
        assert!(!is_variation_selector('\u{180B}'));
        assert!(!is_variation_selector('a'));
    }
}
