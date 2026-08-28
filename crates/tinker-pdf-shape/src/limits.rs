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

    /// How many lookup applications one run of the lookup list may attempt.
    ///
    /// This is the ceiling that bounds work relative to the caller's budget
    /// rather than relative to whatever the font happens to be. It counts
    /// every attempt — a subtable consulted and refused costs the same as one
    /// that substituted — because a font whose coverage tables never match is
    /// exactly as expensive to run as one whose coverage tables always do.
    pub max_operations: u32,

    /// How long the buffer may grow.
    ///
    /// Multiple substitution (GSUB type 2) is the only lookup that adds
    /// glyphs, and it can add up to 65 535 of them per input glyph. Two such
    /// lookups in sequence is 65 535 squared, which is why the ceiling is on
    /// the *buffer* rather than on any one lookup.
    pub max_glyphs: usize,
}

impl Limits {
    /// The ceilings a run of ordinary length is shaped under.
    ///
    /// The two work numbers are absolute here and scaled by
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
    };

    /// The default ceilings, with the two work numbers scaled to a run of
    /// `count` glyphs.
    ///
    /// **A thousand operations and thirty-two glyphs each.** Both multipliers
    /// are generous on purpose: an Indic cluster of eight code points can
    /// legitimately cost a few hundred lookup applications, and a
    /// decomposition-heavy face can double a run's length twice over. What
    /// they bound is the *shape* of the growth — linear in the input — which
    /// is the property that makes a paragraph's cost predictable and a
    /// hostile face's cost finite.
    #[must_use]
    pub fn for_glyphs(count: usize) -> Self {
        let ops = u32::try_from(count)
            .unwrap_or(u32::MAX)
            .saturating_mul(1_000)
            .max(Self::DEFAULT.max_operations);
        let glyphs = count
            .saturating_mul(32)
            .max(Self::DEFAULT.max_glyphs)
            .min(u32::MAX as usize);
        Self {
            max_operations: ops,
            max_glyphs: glyphs,
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
    }

    #[test]
    fn scaling_cannot_wrap() {
        let huge = Limits::for_glyphs(usize::MAX);
        assert_eq!(huge.max_operations, u32::MAX);
        assert!(huge.max_glyphs >= Limits::DEFAULT.max_glyphs);
    }
}
