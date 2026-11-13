//! The ceilings a hostile font is executed under.
//!
//! Parsing a lookup table cannot loop, because [`crate::read::Bytes`] makes
//! every read a bounds check and nothing here recurses over the *structure*.
//! **Executing** one can, and in three separate ways, so there are three
//! ceilings rather than one general-purpose "budget":
//!
//! 1. a context lookup names other lookups by index, and nothing stops lookup
//!    3 from naming lookup 3;
//! 2. an extension subtable names another subtable by 32-bit offset, and
//!    nothing stops it from naming itself;
//! 3. a multiple-substitution lookup replaces one glyph with many, and a
//!    lookup list that runs it repeatedly turns five glyphs into as many as
//!    the arithmetic allows.
//!
//! Each is bounded by its own named constant below, because a single number
//! covering all three would have to be set for the worst of them and would
//! then refuse fonts that are merely large.

/// How much work one run of the lookup list may cost.
///
/// Every field is a *refusal* threshold rather than an allocation: reaching
/// one stops the lookup that reached it and records a [`crate::Warning`],
/// leaving the buffer as it stood. Ruling 2's shape, applied to shaping: a
/// paragraph with one pathological lookup in it sets the rest of the way
/// rather than failing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How deep a context lookup may recurse into another one.
    ///
    /// **Six.** The deepest nesting anybody has a reason to write is a
    /// chaining context selecting a context selecting a ligature, which is
    /// three; the Universal Shaping Engine's own feature files reach four in
    /// the worst script. Six is that with slack, and it is small enough that
    /// the whole recursion fits in a few hundred bytes of stack whatever the
    /// font says.
    ///
    /// A font that reaches it gets [`crate::Warning::NestingTooDeep`] and the
    /// nested lookup does not run — which is the only choice that terminates,
    /// since a lookup naming itself is otherwise a loop with no exit at all.
    pub max_nesting_depth: u32,

    /// How many extension indirections may be followed before a subtable is
    /// declared unreachable.
    ///
    /// **Four.** ISO/IEC 14496-22 gives the answer as *one*: the
    /// `extensionLookupType` of an extension subtable "must not be the
    /// extension lookup type itself", so a conforming font never needs a
    /// second hop. The cap is four rather than one because the *bound* is
    /// what makes a self-pointing offset terminate, and refusing at four
    /// costs three redundant bounds checks in a case no real font reaches,
    /// while refusing at one would need the spec rule to be the only defence.
    /// Both are enforced: the type check refuses a conforming-font mistake by
    /// name, and this cap refuses the hostile case whatever it points at.
    pub max_extension_depth: u32,

    /// How many glyphs one lookup may match as its input sequence.
    ///
    /// **Sixty-four**, which is the same number as the deepest ligature or
    /// chaining rule any shipped font has been observed to carry, and far
    /// past any script's cluster. A rule longer than this is refused rather
    /// than truncated, because a truncated match is a *different* rule
    /// silently applied.
    pub max_context_length: usize,

    /// How many **subtables** one run of the lookup list may search.
    ///
    /// This is the ceiling that bounds work relative to the caller's budget
    /// rather than relative to whatever the font happens to be. A subtable
    /// consulted and refused costs the same as one that substituted, because
    /// a font whose coverage tables never match is exactly as expensive to
    /// run as one whose coverage tables always do.
    ///
    /// **The subtable and not the attempt**, and the distinction is the whole
    /// value of the number. A lookup's `subTableCount` is a 16-bit field the
    /// font writes, and applying a lookup at one position searches every one
    /// of them until one applies; a ceiling counting attempts would leave the
    /// font choosing what each attempt cost. `apply_at` therefore charges one
    /// per subtable, with the attempt's own charge paying for the first — so
    /// a lookup with a single subtable, which is nearly every lookup in every
    /// shipped face, costs exactly one as it always has.
    pub max_operations: u32,

    /// How long the buffer may **grow**.
    ///
    /// Multiple substitution (GSUB type 2) is the only lookup that adds
    /// glyphs, and it can add up to 65 535 of them per input glyph. Two such
    /// lookups in sequence is 65 535 squared, which is why the ceiling is on
    /// the *buffer* rather than on any one lookup.
    ///
    /// **It is not a cap on the shaped run's length**, and the distinction is
    /// worth stating because a reader has already got it wrong. `Shaper::shape`
    /// fills the buffer one glyph per character *before* the limits are
    /// consulted, and [`Limits::for_glyphs`] — the factory used whenever a
    /// caller supplies none — returns a ceiling at or above that fill by
    /// construction. A caller who sets this below the run's own length has not
    /// asked for a shorter run; they have asked for no substitution, and
    /// [`crate::Warning::GlyphBudgetExceeded`] is what says so (ruling 10).
    /// The `shape_text` fuzz target asserted the other reading and failed on
    /// every run of more than eight characters.
    pub max_glyphs: usize,

    /// How many bytes a face's [`crate::Plans`] may hold and read.
    ///
    /// **Eight mebibytes.** Everything a `Plans` keeps is charged here — a
    /// plan its lookup pairs and its key, an index its glyph bitmaps and a
    /// fixed cost per subtable — and so is every coverage byte read to build
    /// an index, because a face may name one coverage of 65 535 ranges from
    /// every subtable of every lookup, and reading it each time is the cost
    /// a hostile face would choose. A shipped face's indexes are a few
    /// hundred kilobytes at most; past this a plan is resolved per run and a
    /// lookup probed at every position, exactly as with no `Plans`, so the
    /// ceiling changes how fast a face shapes and never what it shapes to.
    ///
    /// A ceiling on a face rather than a run, which is why
    /// [`Limits::for_glyphs`] leaves it alone: the charge is the shaper's at
    /// the moment it builds, and a `Plans` holds no more than the largest
    /// ceiling any shaper over it carried.
    pub max_index_bytes: usize,

    /// How large a compressed Graphite table may say it decodes to.
    ///
    /// **Sixteen mebibytes.** A compressed `Silf` or `Glat` states its
    /// decoded size in twenty-seven bits, which is 128 MiB, and the size is
    /// the face's word: a hostile one names the most it can and decodes a
    /// few bytes into it. The largest shipped description this crate has
    /// read decodes to under three, so the ceiling is generous five times
    /// over, and it is checked before a byte is decoded — a table that
    /// states more is refused by name (`graphite::Refusal::TableTooLarge`)
    /// and its face read as though it did not carry it.
    ///
    /// A ceiling on a face, like [`Limits::max_index_bytes`], so
    /// [`Limits::for_glyphs`] leaves it alone.
    pub max_table_bytes: usize,

    /// How many attribute runs one glyph's `Glat` block may be read through.
    ///
    /// **Two hundred and fifty-six.** A glyph's attributes are stored as runs
    /// of consecutive numbers, and asking for one walks the runs in order, so
    /// a face whose every block is the whole of a large `Glat` would make
    /// every question cost the table. Shipped faces store a handful of runs a
    /// glyph; past the ceiling the block is refused by name
    /// (`graphite::Refusal::TooManyAttributeRuns`) and answers nothing. A
    /// face's ceiling, which [`Limits::for_glyphs`] leaves alone.
    pub max_attribute_runs: u32,

    /// How many passes of a Graphite description are run.
    ///
    /// **Sixty-four.** A description states its pass count in a byte, and
    /// each pass is a walk over the whole run, so the count multiplies every
    /// other cost. The richest shipped description this crate has read runs
    /// twenty-one; a pass past the ceiling is refused by name
    /// (`graphite::Refusal::TooManyPasses`) and does not run. A face's
    /// ceiling, which [`Limits::for_glyphs`] leaves alone.
    pub max_passes: u32,

    /// How many rules one position of a Graphite pass may try.
    ///
    /// **One thousand and twenty-four.** The machine collects every rule of
    /// every accepting state it passes through, and a state's rule list is a
    /// length the face writes, so a hostile pass can hand one position tens
    /// of thousands of candidates, each a constraint to run. A shipped
    /// face's state lists a few; past the ceiling the rest are not tried and
    /// the pass is named (`graphite::Refusal::TooManyCandidates`). The walk
    /// itself reads no more than [`Limits::max_context_length`] slots, the
    /// bound a rule shares with an OpenType lookup's input. A face's ceiling,
    /// which [`Limits::for_glyphs`] leaves alone.
    pub max_rule_candidates: u32,

    /// How many transitions one AAT state machine may take over one
    /// subtable.
    ///
    /// **Four thousand and ninety-six, or thirty-two a glyph.** A machine
    /// takes one transition a glyph and one more at the end of the text when
    /// it never holds its position, and an entry with `dontAdvance` set holds
    /// it — rightly, to look at a glyph again once it has changed it, and for
    /// ever where the font says so: text-rendering-tests `MORX-24` substitutes
    /// `A` by `B` through `E` by `A` on transitions that never advance.
    /// Nothing in the state table bounds that, so the ceiling does: past it
    /// the machine stops where it stands, the run as it stood, and the
    /// subtable is named ([`crate::Warning::TransitionBudgetExceeded`]). A
    /// run's ceiling, which [`Limits::for_glyphs`] scales to
    /// `max(4 096, 32 × (glyphs + 1))` (`docs/design/shaping.md`, *Warnings
    /// and ceilings*), so a machine that looks at each glyph a few times
    /// over a long paragraph is never mistaken for one that loops.
    pub max_transitions: u32,

    /// How many chains and subtables of one AAT table are read and run,
    /// each chain and each subtable counting one.
    ///
    /// **Four thousand and ninety-six.** A `morx` states its chain count and
    /// each chain its subtable count in thirty-two bits, and every run walks
    /// every one of them, so a table of thousands of empty subtables would
    /// make every run cost the table. A shipped face's chains hold tens to a
    /// few hundred subtables; past the ceiling the rest are not read and the
    /// table is named ([`crate::Warning::SubtableBudgetExceeded`]). A face's
    /// ceiling, which [`Limits::for_glyphs`] leaves alone.
    pub max_subtables: u32,

    /// How many feature entries of one `morx` chain are read.
    ///
    /// **Four thousand and ninety-six.** A chain states its entry count in
    /// thirty-two bits and every run reads every entry to compute the chain's
    /// flags, so the count is a cost a hostile face chooses. A shipped
    /// chain's entries are a few for each of its feature settings; past the
    /// ceiling the rest are not read and set no flag, and the chain is named
    /// ([`crate::Warning::FeatureEntryBudgetExceeded`]). A face's ceiling,
    /// which [`Limits::for_glyphs`] leaves alone.
    pub max_feature_entries: u32,
    /// How deep one block of Graphite code's stack may grow.
    ///
    /// **Two hundred and fifty-six.** A block's depth is computed before it
    /// runs, so the ceiling refuses code rather than stopping it: a block
    /// that would push past it fails its check by name
    /// (`graphite::Fault::StackOverflow`) and never runs. The deepest block
    /// of the three vendored faces reaches five. A face's ceiling, which
    /// [`Limits::for_glyphs`] leaves alone.
    pub max_stack_depth: u32,

    /// How many Graphite instructions one run may execute.
    ///
    /// **Four million, or sixty-four thousand a glyph where that is more.**
    /// No instruction jumps backwards, so a block cannot loop; what can
    /// grow is how often blocks run — every pass over every position, each
    /// candidate rule's constraint once for each slot of its match — and a
    /// hostile description multiplies those by the counts it writes. The
    /// budget makes the product finite and linear in the run, which is the
    /// shape [`Limits::max_operations`] gives the OpenType path; once it is
    /// spent the run's remaining code is refused by name
    /// (`graphite::Refusal::TooManyInstructions`). Scaled by
    /// [`Limits::for_glyphs`].
    pub max_instructions: u32,
}

impl Limits {
    /// The ceilings a run of ordinary length is shaped under.
    ///
    /// The work numbers are absolute here and scaled by
    /// [`Limits::for_glyphs`] where the run's length is known, which is the
    /// distinction the layout crate's own budgets already draw: a fixed
    /// ceiling bounds a hostile input, and a scaled one keeps a legitimate
    /// long paragraph from hitting the same wall.
    pub const DEFAULT: Self = Self {
        max_nesting_depth: 6,
        max_extension_depth: 4,
        max_context_length: 64,
        max_operations: 500_000,
        max_glyphs: 65_536,
        max_index_bytes: 8 << 20,
        max_table_bytes: 16 << 20,
        max_attribute_runs: 256,
        max_passes: 64,
        max_rule_candidates: 1_024,
        max_transitions: 4_096,
        max_subtables: 4_096,
        max_feature_entries: 4_096,
        max_stack_depth: 256,
        max_instructions: 4_000_000,
    };

    /// The default ceilings, with the four work numbers scaled to a run of
    /// `count` glyphs.
    ///
    /// **A thousand operations, thirty-two glyphs and thirty-two machine
    /// transitions each.** The multipliers are generous on purpose: an Indic
    /// cluster of eight code points can legitimately cost a few hundred
    /// lookup applications, a decomposition-heavy face can double a run's
    /// length twice over, and an AAT machine may look at each glyph several
    /// times. What they bound is the *shape* of the growth — linear in the
    /// input — which is the property that makes a paragraph's cost
    /// predictable and a hostile face's cost finite.
    /// **A thousand operations, thirty-two glyphs and sixty-four thousand
    /// Graphite instructions each.** The multipliers are generous on
    /// purpose: an Indic cluster of eight code points can legitimately cost a
    /// few hundred lookup applications, a decomposition-heavy face can
    /// double a run's length twice over, and a Graphite description runs
    /// every one of its passes over every position. What they bound is the
    /// *shape* of the growth — linear in the input — which is the property
    /// that makes a paragraph's cost predictable and a hostile face's cost
    /// finite.
    #[must_use]
    pub fn for_glyphs(count: usize) -> Self {
        let count32 = u32::try_from(count).unwrap_or(u32::MAX);
        let ops = count32
            .saturating_mul(1_000)
            .max(Self::DEFAULT.max_operations);
        let glyphs = count
            .saturating_mul(32)
            .max(Self::DEFAULT.max_glyphs)
            .min(u32::MAX as usize);
        let transitions = u32::try_from(count)
            .unwrap_or(u32::MAX)
            .saturating_add(1)
            .saturating_mul(32)
            .max(Self::DEFAULT.max_transitions);
        let instructions = count32
            .saturating_mul(64_000)
            .max(Self::DEFAULT.max_instructions);
        Self {
            max_operations: ops,
            max_glyphs: glyphs,
            max_transitions: transitions,
            max_instructions: instructions,
            ..Self::DEFAULT
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::Limits;

    #[test]
    fn scaling_never_lowers_the_floor() {
        let small = Limits::for_glyphs(0);
        assert_eq!(small.max_operations, Limits::DEFAULT.max_operations);
        assert_eq!(small.max_glyphs, Limits::DEFAULT.max_glyphs);
        assert_eq!(small.max_transitions, Limits::DEFAULT.max_transitions);
    }

    /// An AAT machine's ceiling is `max(4 096, 32 × (glyphs + 1))`: the floor
    /// holds a short run, thirty-two transitions a glyph and one more for the
    /// end of the text a long one, and no run wraps it.
    #[test]
    fn the_transition_ceiling_is_thirty_two_a_glyph_over_its_floor() {
        assert_eq!(Limits::DEFAULT.max_transitions, 4_096);
        for (count, ceiling) in [
            (0usize, 4_096u32),
            (1, 4_096),
            (127, 4_096),
            (128, 4_128),
            (1_000, 32_032),
            (100_000, 3_200_032),
        ] {
            assert_eq!(
                Limits::for_glyphs(count).max_transitions,
                ceiling,
                "{count}"
            );
        }
        assert_eq!(Limits::for_glyphs(usize::MAX).max_transitions, u32::MAX);
        for count in [0usize, 7, 64, 2_048, 65_536, 1 << 20] {
            let ceiling = u64::from(Limits::for_glyphs(count).max_transitions);
            let walk = u64::try_from(count).expect("small") + 1;
            assert!(ceiling >= walk, "a run of {count} glyphs cannot be walked");
        }
    }

    /// **A ceiling derived from a run is never below that run**, which is the
    /// property `Shaper::shape` relies on: it fills the buffer before it asks
    /// for the limits, so a `for_glyphs` ceiling that could land under the
    /// fill would make `may_grow` refuse every substitution on an ordinary
    /// paragraph.
    #[test]
    fn the_scaled_ceiling_is_never_below_the_run_it_was_made_for() {
        for count in [0usize, 1, 7, 64, 2_048, 65_536, 1 << 20, 1 << 26] {
            assert!(
                Limits::for_glyphs(count).max_glyphs >= count,
                "a run of {count} glyphs got a ceiling below itself"
            );
        }
    }

    #[test]
    fn scaling_cannot_wrap() {
        let huge = Limits::for_glyphs(usize::MAX);
        assert_eq!(huge.max_operations, u32::MAX);
        assert_eq!(huge.max_instructions, u32::MAX);
        assert!(huge.max_glyphs >= Limits::DEFAULT.max_glyphs);
    }

    /// The instruction budget is a run's: its floor is the default, and
    /// past the floor it grows with the run, sixty-four thousand a glyph.
    #[test]
    fn the_instruction_budget_scales_with_the_run() {
        assert_eq!(Limits::DEFAULT.max_instructions, 4_000_000);
        assert_eq!(Limits::for_glyphs(0).max_instructions, 4_000_000);
        assert_eq!(Limits::for_glyphs(62).max_instructions, 4_000_000);
        assert_eq!(Limits::for_glyphs(63).max_instructions, 4_032_000);
        assert_eq!(Limits::for_glyphs(1_000).max_instructions, 64_000_000);
        // The stack's ceiling is a face's: no run moves it.
        for count in [0usize, 1, 1_000, usize::MAX] {
            assert_eq!(Limits::for_glyphs(count).max_stack_depth, 256);
        }
    }

    /// The index ceiling is a face's, not a run's: no run length moves it.
    #[test]
    fn scaling_leaves_the_index_ceiling_alone() {
        for count in [0usize, 1, 1_000, 1 << 20, usize::MAX] {
            assert_eq!(
                Limits::for_glyphs(count).max_index_bytes,
                Limits::DEFAULT.max_index_bytes
            );
        }
    }

    /// The Graphite tables' ceilings are a face's too: no run length moves
    /// the size a table may decode to or the runs a glyph's block is read
    /// through.
    #[test]
    fn scaling_leaves_the_graphite_table_ceilings_alone() {
        for count in [0usize, 1, 1_000, 1 << 20, usize::MAX] {
            let limits = Limits::for_glyphs(count);
            assert_eq!(limits.max_table_bytes, Limits::DEFAULT.max_table_bytes);
            assert_eq!(
                limits.max_attribute_runs,
                Limits::DEFAULT.max_attribute_runs
            );
            assert_eq!(limits.max_passes, Limits::DEFAULT.max_passes);
            assert_eq!(
                limits.max_rule_candidates,
                Limits::DEFAULT.max_rule_candidates
            );
            assert_eq!(limits.max_stack_depth, Limits::DEFAULT.max_stack_depth);
        }
        assert_eq!(Limits::DEFAULT.max_table_bytes, 16 << 20);
        assert_eq!(Limits::DEFAULT.max_attribute_runs, 256);
        assert_eq!(Limits::DEFAULT.max_passes, 64);
        assert_eq!(Limits::DEFAULT.max_rule_candidates, 1_024);
    }

    /// The AAT table ceilings are a face's: no run length moves how many
    /// chains, subtables and feature entries are read.
    #[test]
    fn scaling_leaves_the_aat_table_ceilings_alone() {
        for count in [0usize, 1, 1_000, 1 << 20, usize::MAX] {
            let limits = Limits::for_glyphs(count);
            assert_eq!(limits.max_subtables, Limits::DEFAULT.max_subtables);
            assert_eq!(
                limits.max_feature_entries,
                Limits::DEFAULT.max_feature_entries
            );
        }
        assert_eq!(Limits::DEFAULT.max_subtables, 4_096);
        assert_eq!(Limits::DEFAULT.max_feature_entries, 4_096);
    }
}
