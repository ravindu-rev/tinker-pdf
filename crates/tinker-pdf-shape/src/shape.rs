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
//! **A right-to-left run has no conformance fixture at this milestone.** The
//! text-rendering-tests sections this milestone is graded on are all left to
//! right; `docs/design/shaping.md` puts Arabic at milestone 4, and until then
//! the claim about right-to-left runs is that they are deterministic and that
//! the levels are right, not that the glyphs are.
//!
//! # Integer, throughout
//!
//! Every number that leaves here is a font design unit, and
//! [`ShapedRun::units_per_em`] is beside them so a consumer can scale. The
//! crate denies `clippy::float_arithmetic`; the reason is ruling 4 and it is
//! written up in `lib.rs`.

use core::ops::Range;

use tinker_pdf_font::Sfnt;

use crate::bidi::{BaseDirection, Level, Paragraph};
use crate::buffer::{Buffer, Direction, ShapedGlyph};
use crate::common::Tag;
use crate::limits::Limits;
use crate::read::Bytes;
use crate::unicode::{self, Script};
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
/// this milestone and all three are adjudicated: GPOS-1 and GPOS-2 are `kern`,
/// GPOS-3 is `mark`, GPOS-4 is `mkmk`. `dist` and `curs` are the registry's
/// other two default-on positioning features; neither is reached by a fixture
/// here, and `curs` is milestone 4's subject.
pub const DEFAULT_GPOS_FEATURES: &[Tag] = &[
    Tag::new(b"kern"),
    Tag::new(b"dist"),
    Tag::new(b"curs"),
    Tag::new(b"mark"),
    Tag::new(b"mkmk"),
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
    gsub_features: &'a [Tag],
    gpos_features: &'a [Tag],
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
            gsub_features: DEFAULT_GSUB_FEATURES,
            gpos_features: DEFAULT_GPOS_FEATURES,
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
    #[must_use]
    pub const fn with_features(mut self, gsub: &'a [Tag], gpos: &'a [Tag]) -> Self {
        self.gsub_features = gsub;
        self.gpos_features = gpos;
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
        self.map(slice, run.text.start, &mut buffer);

        let limits = self
            .limits
            .unwrap_or_else(|| Limits::for_glyphs(buffer.len()));
        let script = run.script.opentype_tag();
        let mut warnings = Vec::new();
        if let Some(gsub) = self.layout.gsub() {
            let lookups = gsub.lookups_for(script, self.language, self.gsub_features);
            warnings.extend(self.layout.substitute(&mut buffer, &lookups, limits));
        }

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
            let lookups = gpos.lookups_for(script, self.language, self.gpos_features);
            warnings.extend(self.layout.position(
                &mut buffer,
                &lookups,
                limits,
                MarkWidths::ZeroByGdef,
            ));
        } else {
            // A face with no `GPOS` still has marks in it, and a combining
            // accent drawn with the advance `hmtx` gave it pushes the pen
            // along and takes the word apart. `Layout::position` does this at
            // the end of positioning; without a `GPOS` nothing would.
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
    fn map(&self, text: &str, base: usize, buffer: &mut Buffer) {
        let uvs = variation_subtable(self.face);
        let mut chars = text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            let cluster = u32::try_from(base.saturating_add(at)).unwrap_or(u32::MAX);
            if is_variation_selector(c) {
                // A selector with no base before it selects nothing. It is
                // still ignorable, so it is dropped rather than drawn.
                continue;
            }
            let selector = chars
                .peek()
                .map(|(_, next)| *next)
                .filter(|next| is_variation_selector(*next));
            let mut glyph = None;
            if let Some(selector) = selector {
                chars.next();
                glyph = uvs.and_then(|data| variation_glyph(data, c, selector, self.face));
            }
            let glyph = glyph.or_else(|| self.face.glyph_for_char(c)).unwrap_or(0);
            buffer.push(glyph, cluster);
        }
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
    use super::{is_variation_selector, itemize};
    use crate::bidi::{BaseDirection, Paragraph};
    use crate::unicode::Script;

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
