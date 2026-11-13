//! The Universal Shaping Engine's cluster model: what a syllable is, and which
//! of its characters have to move.
//!
//! Milestone 5 of `docs/design/shaping.md`, which calls this the roadmap's
//! largest single item and USE correctness *"effectively unbounded"*. What is
//! here is the part the vendored fixtures adjudicate, and
//! `docs/features/fonts.md` says by name which scripts that leaves shaped but
//! unverified. Nothing here is claimed beyond what SHBALI, SHKNDA and SHLANA
//! check.
//!
//! # The one thing a Brahmic shaper must do that a Latin one need not
//!
//! A dependent vowel is **typed after the consonant it belongs to and drawn on
//! whichever side the script puts it** — and for a good many of them that side
//! is the left. `Indic_Positional_Category` says which, and the value that
//! costs work is `Left`: those characters are stored after their base and have
//! to be moved before it before any lookup that positions them can match.
//!
//! Tai Tham's `U+1A6E TAI THAM VOWEL SIGN E` is the plain case, and
//! text-rendering-tests SHLANA-1/21 states it outright: two characters typed
//! consonant-then-vowel, and an expected rendering of vowel-then-consonant.
//! Balinese `SHBALI-2/1` is the case that says **how far** it moves: past a
//! whole conjunct, to the front of the syllable, and not merely past the
//! consonant it is attached to.
//!
//! `Visual_Order_Left` is the opposite value and the reason the property has
//! two: those characters are *already* stored where they are drawn, and moving
//! one would break a script that was fine.
//!
//! # What a syllable is here
//!
//! A base, and everything that hangs off it. A new one starts at a base
//! character unless a halant came immediately before — because a halant is
//! precisely the statement that the next consonant belongs to this cluster
//! rather than starting a new one. Anything that is not Brahmic at all ends
//! the syllable it interrupts and stands alone.
//!
//! That is a **simplification** of USE's published grammar, which is a regular
//! expression over a dozen categories with named cluster types, and it is
//! stated as one rather than presented as the whole thing. What it captures is
//! the base, the halant chain and the trailing marks, which is what the
//! reordering below and the per-syllable feature application need. What it
//! does not capture is the distinction between the cluster *types* — a numeral
//! cluster from a symbol cluster from a broken one — which USE uses to decide
//! whether to insert a dotted circle. This crate never inserts one; see
//! `docs/features/fonts.md`.
//!
//! # The three rules, reintroduced as defects and counted
//!
//! Each was put back and `cargo test -p tinker-pdf-shape --no-fail-fast` run
//! against it, so these are measured and not expected:
//!
//! | Defect reintroduced | Tests that caught it |
//! | --- | --- |
//! | The halant moves the reordering insertion point again | 3 |
//! | The reordering pause is skipped altogether | 2 |
//! | Canonical decomposition is switched off | 2 |
//!
//! The first is caught by this module's own unit test as well as by
//! `tests/text_rendering.rs` and `tests/fingerprints.rs`; the other two are
//! caught only by those, because both defects are in `crate::shape`'s *use* of
//! this module and [`reorder`] answers correctly for the categories it is
//! handed either way. That asymmetry is the reason the conformance suite is
//! the guard and the unit tests are the explanation.
//!
//! # Why the syllable outlives this module
//!
//! Every USE feature is applied **per syllable**: a rule may not match across a
//! cluster boundary, because two adjacent syllables are two words as far as a
//! conjunct-forming lookup is concerned. That is carried on the buffer rather
//! than done here, so that the whole of `GSUB` obeys it; see
//! [`crate::buffer::Buffer::set_syllable`].

use crate::unicode::{
    general_class, indic_positional, indic_syllabic, joining_type, script, GeneralClass,
    IndicPositional, IndicSyllabic, JoiningType, Script,
};

/// What a character is to the Universal Shaping Engine: its class and, where
/// the class has them, its subclass.
///
/// The specification's own sigla, from *Character classification*
/// (`docs/design/shaping.md`, *Character categories*), with the subclass
/// spelled into the variant — `VPre` is a `VOWEL_PRE`, `CmAbv` a
/// `CONS_MOD_ABOVE` — so that one value says everything the grammar asks of a
/// character. [`Category::role`] is the part milestone 5's cluster model
/// reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Category {
    /// `B`, a base: a consonant, an independent vowel, a number, a letter a
    /// cursive script joins, or a dependent sign that is a letter of its own.
    B,
    /// `CGJ`, U+034F COMBINING GRAPHEME JOINER.
    Cgj,
    /// `CMAbv`, a consonant modifier above: a nukta, a gemination mark, a
    /// consonant killer or a symbol modifier, drawn above.
    CmAbv,
    /// `CMBlw`, the same drawn below.
    CmBlw,
    /// `CS`, a consonant with a stacker.
    Cs,
    /// `FAbv`, a final consonant above.
    FAbv,
    /// `FBlw`, a final consonant below.
    FBlw,
    /// `FPst`, a final consonant after.
    FPst,
    /// `FM`, a final modifier: what closes a syllable.
    Fm,
    /// `G`, a hieroglyph.
    G,
    /// `GB`, a generic base: a placeholder such as U+00A0, a bullet, a
    /// square.
    Gb,
    /// `H`, a halant: a virama, or an invisible stacker such as Tai Tham's
    /// `SAKOT`.
    H,
    /// `HM`, a hieroglyph modifier.
    Hm,
    /// `HN`, a number joiner.
    Hn,
    /// `HR`, a hieroglyph mirror.
    Hr,
    /// `IND`, an independent base: punctuation, a dead consonant, a
    /// modifying letter, a letter of no category at all. It takes nothing
    /// after it.
    Ind,
    /// `J`, a hieroglyph joiner.
    J,
    /// `MPre`, a medial consonant before the base.
    MPre,
    /// `MAbv`, a medial consonant above.
    MAbv,
    /// `MBlw`, a medial consonant below.
    MBlw,
    /// `MPst`, a medial consonant after.
    MPst,
    /// `N`, a number base that joins.
    N,
    /// `O`, other: a space, a common punctuation mark, a digit, anything of
    /// the `Common` or `Inherited` script that is not named elsewhere.
    #[default]
    O,
    /// `R`, a repha: a consonant that prefixes the base.
    R,
    /// `RK`, a reordering killer.
    Rk,
    /// `Rsv`, a code point no script holds.
    Rsv,
    /// `S`, a symbol.
    S,
    /// `SB`, what begins a hieroglyph segment.
    Sb,
    /// `SE`, what ends one.
    Se,
    /// `SUB`, a subjoined consonant.
    Sub,
    /// `VPre`, a vowel drawn before the base.
    VPre,
    /// `VAbv`, a vowel drawn above.
    VAbv,
    /// `VBlw`, a vowel drawn below.
    VBlw,
    /// `VPst`, a vowel drawn after.
    VPst,
    /// `VMPre`, a vowel modifier before the base.
    VmPre,
    /// `VMAbv`, a vowel modifier above: a bindu, a tone mark, a cantillation
    /// mark, a register shifter, a visarga.
    VmAbv,
    /// `VMBlw`, the same below.
    VmBlw,
    /// `VMPst`, the same after.
    VmPst,
    /// `VS`, a variation selector, U+FE00–U+FE0F.
    Vs,
    /// `WJ`, U+2060 WORD JOINER.
    Wj,
    /// `ZWJ`.
    Zwj,
    /// `ZWNJ`.
    Zwnj,
}

/// What milestone 5's cluster model reads of a [`Category`]: the one question
/// its syllable rule, its reordering pause and `rphf`'s offer each ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// The thing a syllable is built around.
    Base,
    /// Says the next base continues this syllable rather than starting one.
    Halant,
    /// A dependent drawn before its base, and so one the pause moves there.
    PreBase,
    /// Any other dependent.
    Mark,
    /// Outside every syllable: a space, Latin, punctuation, a symbol.
    Other,
}

impl Category {
    /// The role milestone 5's model gives this class.
    ///
    /// On every character the vendored corpora type this is the answer the
    /// five categories gave; `docs/design/shaping.md` names the classes whose
    /// role differs off them, each one the specification classifies
    /// differently from milestone 5.
    pub(crate) const fn role(self) -> Role {
        match self {
            Category::B | Category::Gb | Category::Cs | Category::N | Category::R | Category::G => {
                Role::Base
            }
            Category::H => Role::Halant,
            Category::MPre | Category::VPre | Category::VmPre => Role::PreBase,
            Category::CmAbv
            | Category::CmBlw
            | Category::FAbv
            | Category::FBlw
            | Category::FPst
            | Category::Fm
            | Category::Hm
            | Category::Hn
            | Category::Hr
            | Category::J
            | Category::MAbv
            | Category::MBlw
            | Category::MPst
            | Category::Rk
            | Category::Sub
            | Category::VAbv
            | Category::VBlw
            | Category::VPst
            | Category::VmAbv
            | Category::VmBlw
            | Category::VmPst
            | Category::Zwj
            | Category::Zwnj => Role::Mark,
            Category::Cgj
            | Category::Ind
            | Category::O
            | Category::Rsv
            | Category::S
            | Category::Sb
            | Category::Se
            | Category::Vs
            | Category::Wj => Role::Other,
        }
    }
}

/// A syllabic value the specification's override file assigns: one of
/// Unicode's, one of its own two, or a hieroglyph class of its
/// `Extended_Syllabic_Category`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Syllabic {
    /// A value Unicode's `Indic_Syllabic_Category` has.
    Unicode(IndicSyllabic),
    /// `Symbol_Modifier`, a consonant modifier to the classification.
    SymbolModifier,
    /// `Consonant_Final_Modifier`, which the classification table predates and
    /// this crate reads as `FM`.
    ConsonantFinalModifier,
    /// `Hieroglyph`.
    Hieroglyph,
    /// `Hieroglyph_Joiner`.
    HieroglyphJoiner,
    /// `Hieroglyph_Mark_Begin` and `Hieroglyph_Segment_Begin`.
    HieroglyphBegin,
    /// `Hieroglyph_Mark_End` and `Hieroglyph_Segment_End`.
    HieroglyphEnd,
    /// `Hieroglyph_Mirror`.
    HieroglyphMirror,
    /// `Hieroglyph_Modifier`.
    HieroglyphModifier,
}

/// *Overrides to Unicode categories*: `IndicSyllabicCategory-Additional.txt`,
/// transcribed at its Unicode 18.0 revision, sorted, every range assigned at
/// the pinned 17.0.0 (`docs/design/shaping.md`, *Character categories*).
static SYLLABIC_OVERRIDES: &[(u32, u32, Syllabic)] = {
    use IndicSyllabic as I;
    use Syllabic::{
        ConsonantFinalModifier, Hieroglyph, HieroglyphBegin, HieroglyphEnd, HieroglyphJoiner,
        HieroglyphMirror, HieroglyphModifier, SymbolModifier, Unicode,
    };
    &[
        (0x005B, 0x005B, HieroglyphBegin),
        (0x005D, 0x005D, HieroglyphEnd),
        (0x007B, 0x007B, HieroglyphBegin),
        (0x007D, 0x007D, HieroglyphEnd),
        (0x07EB, 0x07F3, Unicode(I::ToneMark)),
        (0x07FD, 0x07FD, Unicode(I::ToneMark)),
        (0x0800, 0x0815, Unicode(I::Consonant)),
        (0x0840, 0x0858, Unicode(I::Consonant)),
        (0x0859, 0x085B, Unicode(I::Nukta)),
        (0x0B55, 0x0B55, Unicode(I::VowelDependent)),
        (0x0F00, 0x0F01, Unicode(I::Consonant)),
        (0x0F04, 0x0F06, Unicode(I::Consonant)),
        (0x0F39, 0x0F39, Unicode(I::Nukta)),
        (0x0F71, 0x0F71, Unicode(I::Nukta)),
        (0x0F7F, 0x0F7F, Unicode(I::ConsonantDead)),
        (0x0F86, 0x0F87, Unicode(I::ToneMark)),
        (0x17CF, 0x17CF, Unicode(I::ToneMark)),
        (0x1800, 0x1800, Unicode(I::Consonant)),
        (0x1807, 0x1807, Unicode(I::Consonant)),
        (0x180A, 0x180A, Unicode(I::Consonant)),
        (0x1820, 0x1878, Unicode(I::Consonant)),
        (0x1880, 0x1884, Unicode(I::ConsonantPlaceholder)),
        (0x1885, 0x1886, Unicode(I::Nukta)),
        (0x18A9, 0x18A9, Unicode(I::Nukta)),
        (0x193A, 0x193A, Unicode(I::Bindu)),
        (0x19C1, 0x19C7, Unicode(I::Consonant)),
        (0x1A7B, 0x1A7C, Unicode(I::ToneMark)),
        (0x1A7F, 0x1A7F, Unicode(I::ToneMark)),
        (0x1B6B, 0x1B73, SymbolModifier),
        (0x1C36, 0x1C36, ConsonantFinalModifier),
        (0x1CF8, 0x1CF9, Unicode(I::CantillationMark)),
        (0x25CC, 0x25CC, Unicode(I::Consonant)),
        (0x27E6, 0x27E6, HieroglyphBegin),
        (0x27E7, 0x27E7, HieroglyphEnd),
        (0x27E8, 0x27E8, HieroglyphBegin),
        (0x27E9, 0x27E9, HieroglyphEnd),
        (0x2D30, 0x2D67, Unicode(I::Consonant)),
        (0x2D6F, 0x2D6F, Unicode(I::Consonant)),
        (0x2D7F, 0x2D7F, Unicode(I::Virama)),
        (0x2E22, 0x2E22, HieroglyphBegin),
        (0x2E23, 0x2E23, HieroglyphEnd),
        (0x2E24, 0x2E24, HieroglyphBegin),
        (0x2E25, 0x2E25, HieroglyphEnd),
        (0xAA29, 0xAA29, Unicode(I::Bindu)),
        (0xAAB1, 0xAAB1, Unicode(I::VowelIndependent)),
        (0xAABA, 0xAABA, Unicode(I::VowelIndependent)),
        (0xAABD, 0xAABD, Unicode(I::VowelIndependent)),
        (0xFE00, 0xFE0F, Unicode(I::ModifyingLetter)),
        (0x1_0570, 0x1_057A, Unicode(I::Consonant)),
        (0x1_057C, 0x1_058A, Unicode(I::Consonant)),
        (0x1_058C, 0x1_0592, Unicode(I::Consonant)),
        (0x1_0594, 0x1_0595, Unicode(I::Consonant)),
        (0x1_0597, 0x1_05A1, Unicode(I::Consonant)),
        (0x1_05A3, 0x1_05B1, Unicode(I::Consonant)),
        (0x1_05B3, 0x1_05B9, Unicode(I::Consonant)),
        (0x1_05BB, 0x1_05BC, Unicode(I::Consonant)),
        (0x1_0A0D, 0x1_0A0D, Unicode(I::Bindu)),
        (0x1_0AC0, 0x1_0AC7, Unicode(I::Consonant)),
        (0x1_0AC9, 0x1_0AE4, Unicode(I::Consonant)),
        (0x1_0AE5, 0x1_0AE6, Unicode(I::Nukta)),
        (0x1_0D00, 0x1_0D23, Unicode(I::Consonant)),
        (0x1_0D24, 0x1_0D26, Unicode(I::ToneMark)),
        (0x1_0D27, 0x1_0D27, Unicode(I::GeminationMark)),
        (0x1_0D30, 0x1_0D39, Unicode(I::Number)),
        (0x1_0D4A, 0x1_0D65, Unicode(I::Consonant)),
        (0x1_0D69, 0x1_0D6D, Unicode(I::VowelDependent)),
        (0x1_0D6F, 0x1_0D6F, Unicode(I::Consonant)),
        (0x1_0D70, 0x1_0D85, Unicode(I::Consonant)),
        (0x1_0E80, 0x1_0EA9, Unicode(I::Consonant)),
        (0x1_0EAB, 0x1_0EAC, Unicode(I::VowelDependent)),
        (0x1_0EB0, 0x1_0EB1, Unicode(I::Consonant)),
        (0x1_0F30, 0x1_0F45, Unicode(I::Consonant)),
        (0x1_0F46, 0x1_0F50, Unicode(I::ToneMark)),
        (0x1_0F51, 0x1_0F54, Unicode(I::Number)),
        (0x1_0F70, 0x1_0F81, Unicode(I::Consonant)),
        (0x1_0F82, 0x1_0F85, Unicode(I::Nukta)),
        (0x1_1134, 0x1_1134, Unicode(I::GeminationMark)),
        (0x1_11DA, 0x1_11DA, Unicode(I::Consonant)),
        (0x1_13CE, 0x1_13CE, Unicode(I::Bindu)),
        (0x1_13CF, 0x1_13CF, Unicode(I::Nukta)),
        (0x1_1A3B, 0x1_1A3E, Unicode(I::ConsonantSubjoined)),
        (0x1_3000, 0x1_342F, Hieroglyph),
        (0x1_3430, 0x1_3436, HieroglyphJoiner),
        (0x1_3437, 0x1_3437, HieroglyphBegin),
        (0x1_3438, 0x1_3438, HieroglyphEnd),
        (0x1_3439, 0x1_343B, HieroglyphJoiner),
        (0x1_343C, 0x1_343F, Hieroglyph),
        (0x1_3440, 0x1_3440, HieroglyphMirror),
        (0x1_3441, 0x1_3446, Hieroglyph),
        (0x1_3447, 0x1_3455, HieroglyphModifier),
        (0x1_3460, 0x1_43FA, Hieroglyph),
        (0x1_6AC0, 0x1_6AC9, Unicode(I::Number)),
        (0x1_6B00, 0x1_6B2F, Unicode(I::Consonant)),
        (0x1_6B30, 0x1_6B36, Unicode(I::ToneMark)),
        (0x1_6F00, 0x1_6F4A, Unicode(I::Consonant)),
        (0x1_6F4F, 0x1_6F4F, Unicode(I::Nukta)),
        (0x1_6F50, 0x1_6F50, Unicode(I::ModifyingLetter)),
        (0x1_6F51, 0x1_6F87, Unicode(I::VowelDependent)),
        (0x1_6F8F, 0x1_6F92, Unicode(I::ToneMark)),
        (0x1_6FE4, 0x1_6FE4, Unicode(I::Consonant)),
        (0x1_8B00, 0x1_8CD5, Unicode(I::Consonant)),
        (0x1_8CFF, 0x1_8CFF, Unicode(I::Consonant)),
        (0x1_BC00, 0x1_BC6A, Unicode(I::Consonant)),
        (0x1_BC70, 0x1_BC7C, Unicode(I::Consonant)),
        (0x1_BC80, 0x1_BC88, Unicode(I::Consonant)),
        (0x1_BC90, 0x1_BC99, Unicode(I::Consonant)),
        (0x1_BC9D, 0x1_BC9E, Unicode(I::Nukta)),
        (0x1_E100, 0x1_E12C, Unicode(I::Consonant)),
        (0x1_E130, 0x1_E136, Unicode(I::ToneMark)),
        (0x1_E137, 0x1_E13D, Unicode(I::Consonant)),
        (0x1_E140, 0x1_E149, Unicode(I::Number)),
        (0x1_E14E, 0x1_E14F, Unicode(I::Consonant)),
        (0x1_E290, 0x1_E2AD, Unicode(I::Consonant)),
        (0x1_E2AE, 0x1_E2AE, Unicode(I::ToneMark)),
        (0x1_E2C0, 0x1_E2EB, Unicode(I::Consonant)),
        (0x1_E2EC, 0x1_E2EF, Unicode(I::ToneMark)),
        (0x1_E2F0, 0x1_E2F9, Unicode(I::Number)),
        (0x1_E4D0, 0x1_E4EB, Unicode(I::Consonant)),
        (0x1_E4EC, 0x1_E4EF, Unicode(I::VowelDependent)),
        (0x1_E4F0, 0x1_E4F9, Unicode(I::Number)),
        (0x1_E5D0, 0x1_E5ED, Unicode(I::Consonant)),
        (0x1_E5EE, 0x1_E5EF, Unicode(I::VowelDependent)),
        (0x1_E5F0, 0x1_E5F0, Unicode(I::Consonant)),
        (0x1_E5F1, 0x1_E5FA, Unicode(I::Number)),
        (0x1_E900, 0x1_E943, Unicode(I::Consonant)),
        (0x1_E944, 0x1_E94A, Unicode(I::Nukta)),
        (0x1_E94B, 0x1_E94B, Unicode(I::Consonant)),
        (0x1_E950, 0x1_E959, Unicode(I::Number)),
    ]
};

/// *Overrides to Unicode categories*: `IndicPositionalCategory-Additional.txt`,
/// transcribed at its Unicode 18.0 revision, sorted, every range assigned at
/// the pinned 17.0.0.
static POSITIONAL_OVERRIDES: &[(u32, u32, IndicPositional)] = {
    use IndicPositional as P;
    &[
        (0x07EB, 0x07F3, P::Top),
        (0x07FD, 0x07FD, P::Top),
        (0x0859, 0x085B, P::Bottom),
        (0x0E33, 0x0E33, P::TopAndRight),
        (0x0EB3, 0x0EB3, P::TopAndRight),
        (0x0F72, 0x0F72, P::Bottom),
        (0x0F74, 0x0F74, P::Top),
        (0x0F7A, 0x0F7D, P::Bottom),
        (0x0F80, 0x0F80, P::Bottom),
        (0x103C, 0x103C, P::Left),
        (0x180B, 0x180D, P::NotApplicable),
        (0x180F, 0x180F, P::NotApplicable),
        (0x1885, 0x1886, P::Top),
        (0x18A9, 0x18A9, P::Bottom),
        (0x1A18, 0x1A18, P::Top),
        (0x1B6C, 0x1B6C, P::Top),
        (0x1BF2, 0x1BF3, P::Bottom),
        (0x1C29, 0x1C29, P::Left),
        (0x1CF8, 0x1CF9, P::Top),
        (0x2D7F, 0x2D7F, P::NotApplicable),
        (0xA9BE, 0xA9BE, P::Right),
        (0xA9BF, 0xA9BF, P::Bottom),
        (0xAA35, 0xAA35, P::Top),
        (0xAAB0, 0xAAB0, P::Bottom),
        (0xAAB2, 0xAAB3, P::Bottom),
        (0xAAB4, 0xAAB4, P::Top),
        (0xAAB7, 0xAAB8, P::Bottom),
        (0xAABE, 0xAABE, P::Bottom),
        (0x1_0A0C, 0x1_0A0C, P::Right),
        (0x1_0A38, 0x1_0A38, P::Bottom),
        (0x1_0AE5, 0x1_0AE6, P::Bottom),
        (0x1_0D24, 0x1_0D27, P::Top),
        (0x1_0D69, 0x1_0D6D, P::Top),
        (0x1_0EAB, 0x1_0EAC, P::Top),
        (0x1_0F46, 0x1_0F50, P::Bottom),
        (0x1_0F82, 0x1_0F85, P::Bottom),
        (0x1_1127, 0x1_1129, P::Bottom),
        (0x1_112A, 0x1_112B, P::Top),
        (0x1_112D, 0x1_112D, P::Bottom),
        (0x1_1130, 0x1_1130, P::Bottom),
        (0x1_1131, 0x1_1132, P::Top),
        (0x1_13CF, 0x1_13CF, P::Bottom),
        (0x1_1942, 0x1_1942, P::Right),
        (0x1_6B30, 0x1_6B36, P::Top),
        (0x1_6F4F, 0x1_6F4F, P::Bottom),
        (0x1_6F51, 0x1_6F87, P::Bottom),
        (0x1_6F8F, 0x1_6F92, P::Bottom),
        (0x1_BC9D, 0x1_BC9E, P::Overstruck),
        (0x1_E130, 0x1_E136, P::Top),
        (0x1_E2AE, 0x1_E2AE, P::Top),
        (0x1_E2EC, 0x1_E2EF, P::Top),
        (0x1_E4EC, 0x1_E4EF, P::Top),
        (0x1_E5EE, 0x1_E5EF, P::Bottom),
        (0x1_E944, 0x1_E94A, P::Top),
    ]
};

/// A code point's value in a sorted `(first, last, value)` table.
fn overridden<T: Copy>(table: &[(u32, u32, T)], code: u32) -> Option<T> {
    let at = table.partition_point(|(_, last, _)| *last < code);
    table
        .get(at)
        .filter(|(first, _, _)| *first <= code)
        .map(|(_, _, value)| *value)
}

/// Where a dependent sits, reduced to the four cardinal positions the
/// subclasses name: a split position goes to its first part in the order
/// before, above, below, after, and `Overstruck` is below, as *Character
/// classification* has it for vowels and vowel modifiers. `None` is a
/// character with no position, or one already typed where it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cardinal {
    Pre,
    Abv,
    Blw,
    Pst,
    None,
}

fn cardinal(position: IndicPositional) -> Cardinal {
    use IndicPositional as P;
    match position {
        P::Left
        | P::LeftAndRight
        | P::TopAndLeft
        | P::TopAndLeftAndRight
        | P::BottomAndLeft
        | P::TopAndBottomAndLeft => Cardinal::Pre,
        P::Top | P::TopAndBottom | P::TopAndRight | P::TopAndBottomAndRight => Cardinal::Abv,
        P::Bottom | P::BottomAndRight | P::Overstruck => Cardinal::Blw,
        P::Right => Cardinal::Pst,
        P::NotApplicable | P::VisualOrderLeft => Cardinal::None,
    }
}

/// A consonant modifier's subclass: above, or below for every other place,
/// the class having no other.
fn consonant_modifier(at: Cardinal) -> Category {
    match at {
        Cardinal::Abv => Category::CmAbv,
        Cardinal::Pre | Cardinal::Blw | Cardinal::Pst | Cardinal::None => Category::CmBlw,
    }
}

/// A final consonant's: above, below, or after for every other place.
fn final_consonant(at: Cardinal) -> Category {
    match at {
        Cardinal::Abv => Category::FAbv,
        Cardinal::Blw => Category::FBlw,
        Cardinal::Pre | Cardinal::Pst | Cardinal::None => Category::FPst,
    }
}

/// A medial consonant's, after where it has no place.
fn medial(at: Cardinal) -> Category {
    match at {
        Cardinal::Pre => Category::MPre,
        Cardinal::Abv => Category::MAbv,
        Cardinal::Blw => Category::MBlw,
        Cardinal::Pst | Cardinal::None => Category::MPst,
    }
}

/// A vowel's, after where it has no place.
fn vowel(at: Cardinal) -> Category {
    match at {
        Cardinal::Pre => Category::VPre,
        Cardinal::Abv => Category::VAbv,
        Cardinal::Blw => Category::VBlw,
        Cardinal::Pst | Cardinal::None => Category::VPst,
    }
}

/// A vowel modifier's, after where it has no place.
fn vowel_modifier(at: Cardinal) -> Category {
    match at {
        Cardinal::Pre => Category::VmPre,
        Cardinal::Abv => Category::VmAbv,
        Cardinal::Blw => Category::VmBlw,
        Cardinal::Pst | Cardinal::None => Category::VmPst,
    }
}

/// A character's [`Category`], by *Character classification* and its
/// overrides, read in the order `docs/design/shaping.md` gives: the fixed
/// code points, then the syllabic category with its `General_Category`
/// split, then, for a character it calls `Other`, `Joining_Type`,
/// `General_Category` and `Script`.
///
/// The syllabic mapping is stated exhaustively rather than with a catch-all
/// arm, so a value Unicode adds to `Indic_Syllabic_Category` or
/// `Indic_Positional_Category` fails to compile here instead of quietly
/// becoming an independent base and losing a script's clusters.
pub(crate) fn category(c: char) -> Category {
    let code = u32::from(c);
    match code {
        0x034F => return Category::Cgj,
        0xFE00..=0xFE0F => return Category::Vs,
        0x2060 => return Category::Wj,
        0x2015 | 0x2022 | 0x25FB..=0x25FE => return Category::Gb,
        _ => {}
    }
    let at =
        cardinal(overridden(POSITIONAL_OVERRIDES, code).unwrap_or_else(|| indic_positional(c)));
    let letter = general_class(c) == Some(GeneralClass::Lo);
    let syllabic = match overridden(SYLLABIC_OVERRIDES, code) {
        Some(Syllabic::Unicode(syllabic)) => syllabic,
        Some(Syllabic::SymbolModifier) => return consonant_modifier(at),
        Some(Syllabic::ConsonantFinalModifier) => return Category::Fm,
        Some(Syllabic::Hieroglyph) => return Category::G,
        Some(Syllabic::HieroglyphJoiner) => return Category::J,
        Some(Syllabic::HieroglyphBegin) => return Category::Sb,
        Some(Syllabic::HieroglyphEnd) => return Category::Se,
        Some(Syllabic::HieroglyphMirror) => return Category::Hr,
        Some(Syllabic::HieroglyphModifier) => return Category::Hm,
        None => indic_syllabic(c),
    };
    use IndicSyllabic as S;
    match syllabic {
        S::Number
        | S::Consonant
        | S::ConsonantHeadLetter
        | S::ToneLetter
        | S::VowelIndependent
        // A value the classification table predates; milestone 5 had it a
        // base, and `SHLANA-3/6` draws it on an advance of its own.
        | S::ConsonantInitialPostfixed => Category::B,
        S::Avagraha => {
            if letter {
                Category::B
            } else {
                unclassified(c)
            }
        }
        S::Bindu => {
            if letter {
                Category::B
            } else {
                vowel_modifier(at)
            }
        }
        S::ConsonantFinal => {
            if letter {
                Category::B
            } else {
                final_consonant(at)
            }
        }
        S::ConsonantMedial => {
            if letter {
                Category::B
            } else {
                medial(at)
            }
        }
        S::ConsonantSubjoined => {
            if letter {
                Category::B
            } else {
                Category::Sub
            }
        }
        S::Vowel | S::VowelDependent => {
            if letter {
                Category::B
            } else {
                vowel(at)
            }
        }
        S::PureKiller => vowel(at),
        S::CantillationMark | S::RegisterShifter | S::ToneMark | S::Visarga => vowel_modifier(at),
        S::ConsonantKiller | S::GeminationMark | S::Nukta => consonant_modifier(at),
        S::ConsonantSucceedingRepha => final_consonant(at),
        S::SyllableModifier => Category::Fm,
        S::ConsonantWithStacker => Category::Cs,
        S::ConsonantPlaceholder => Category::Gb,
        S::ConsonantPrecedingRepha | S::ConsonantPrefixed => Category::R,
        S::ReorderingKiller => Category::Rk,
        S::Virama | S::InvisibleStacker => Category::H,
        S::NumberJoiner => Category::Hn,
        S::BrahmiJoiningNumber => Category::N,
        S::ConsonantDead | S::ModifyingLetter => Category::Ind,
        S::Joiner => Category::Zwj,
        S::NonJoiner => Category::Zwnj,
        S::Other => unclassified(c),
    }
}

/// The class of a character no syllabic row takes: a letter a cursive script
/// joins is a base; punctuation is independent; a symbol is a symbol; a
/// space, and anything else of the `Common` or `Inherited` script, is other;
/// a code point no script holds is reserved; anything else is independent.
fn unclassified(c: char) -> Category {
    if matches!(
        joining_type(c),
        JoiningType::C | JoiningType::D | JoiningType::L | JoiningType::R
    ) {
        return Category::B;
    }
    match general_class(c) {
        // U+104E and U+2022 are the two exceptions the table names; U+2022
        // is a generic base above and U+104E a placeholder.
        Some(GeneralClass::Po) if c != '\u{104E}' => return Category::Ind,
        Some(GeneralClass::So | GeneralClass::Sc) => return Category::S,
        Some(GeneralClass::Zs) => return Category::O,
        Some(GeneralClass::Lo | GeneralClass::Po) | None => {}
    }
    match script(c) {
        Script::Common | Script::Inherited => Category::O,
        Script::Unknown => Category::Rsv,
        _ => Category::Ind,
    }
}

/// Whether a character puts its run in the Brahmic plan: Unicode's own
/// `Indic_Syllabic_Category` gives it a value. Milestone 5's test, kept as it
/// was, so the categories do not change which runs take the plan.
pub(crate) fn is_brahmic(c: char) -> bool {
    indic_syllabic(c) != IndicSyllabic::Other
}

/// Which syllable each character of `text` belongs to, numbered from one.
///
/// Zero is never returned, because zero is the buffer's "this glyph is in no
/// syllable" and a run that is not Brahmic must not be restricted by a
/// boundary it does not have.
///
/// The numbering saturates at [`u16::MAX`]: a paragraph with sixty-five
/// thousand syllables in it would start sharing numbers between the last of
/// them, which merges two clusters that should have been separate rather than
/// splitting one that should not — the safe direction, and unreachable in any
/// text a reader would look at.
pub(crate) fn syllables(text: &[char]) -> Vec<u16> {
    let mut out = Vec::new();
    let mut current = 0u16;
    let mut previous = Role::Other;
    for c in text.iter().copied() {
        let role = category(c).role();
        let starts = match role {
            // A base starts a syllable unless a halant just said it does not.
            Role::Base => previous != Role::Halant,
            // A mark with no base before it is a syllable of its own, which is
            // the "broken cluster" USE names. It still gets a number, so the
            // features that would fix it are applied to it and to nothing
            // else.
            Role::Halant | Role::PreBase | Role::Mark => matches!(previous, Role::Other),
            Role::Other => false,
        };
        if starts {
            current = current.saturating_add(1);
        }
        out.push(if role == Role::Other { 0 } else { current });
        previous = role;
    }
    out
}

/// Where each character of one syllable moves to, as a permutation.
///
/// Returns `None` when nothing moves, which is the overwhelmingly common case
/// and worth not allocating for.
///
/// # The rule
///
/// Every pre-base member of the syllable is moved to the **front of the
/// syllable**, and everything it passes shuffles up one. The insertion point
/// advances after each move, so two pre-base characters in one syllable keep
/// the order they were typed in.
///
/// # The halant used to move the insertion point, and the corpus said no
///
/// This walked the syllable keeping an insertion point just after the last
/// halant, on the reasoning that *"a pre-base vowel belongs in front of the
/// consonant it attaches to and not in front of the whole conjunct"*. That is
/// a plausible sentence and it is not what the fixtures say. Balinese
/// `SHBALI-2/1` is `KA ADEG-ADEG PA TALING` — a conjunct of two consonants
/// followed by a pre-base vowel — and the expected rendering puts the taling
/// **first**, in front of the whole conjunct rather than in front of the
/// second consonant. Dropping the halant rule moved thirty cases across six
/// sections and cost none, which is as close to adjudication as ruling 13
/// permits.
///
/// What survives of the old reasoning is a real question this corpus does not
/// answer: a `pref` consonant — one a face reorders to *before the base* — is
/// a different move from a pre-base vowel's, and USE has a separate step for
/// it. Nothing here implements that step, and no fixture reaches one.
///
/// # Two pre-base characters come out reversed, and that is adjudicated now
///
/// The insertion point does **not** advance after a move, so the second
/// pre-base member of a syllable ends up in front of the first. That stood
/// here as a guess, on the grounds that USE says where a pre-base character
/// goes and does not say what two of them do relative to each other, and that
/// no case in the vendored corpus had two.
///
/// The second half of that was wrong. Tai Tham `SHLANA-6/2` and `SHLANA-6/4`
/// each have a syllable holding both `U+1A55 CONSONANT SIGN MEDIAL RA` and a
/// pre-base vowel — two `Left` characters in one cluster — and both expect the
/// vowel first, which is the reverse of the order they are typed in. Advancing
/// the insertion point was tried and costs those two cases and one of
/// `SHLANA-10`'s; not advancing it is what the fixtures say.
///
/// # A second move was tried here and the corpus refused it, 6 to 69
///
/// The rule tried: **every dependent mark moved to immediately after the last
/// base of its syllable**, so that a matra separated from its base by a
/// subjoined consonant is next to it again when the presentation features run.
///
/// It is not a guess about Kannada. `ನ್ನಾ` — `NA VIRAMA NA AA`,
/// text-rendering-tests `SHKNDA-2/1` — shapes here to three glyphs the fixture
/// does not want; typing the same characters with the matra already next to
/// its base, `NA AA VIRAMA NA`, shapes through this crate *unchanged* to
/// gid150, gid57, gid116 at 0, 711 and 1160, which is that case's expected
/// rendering exactly. Two of the face's lookups need the adjacency and they
/// are different kinds: `NA` + `AA` rewrites the **base** (gid39 becomes
/// gid150, because a Kannada consonant gives up part of its own shape to the
/// matra beside it), and `NA` + `E` **ligates**, to gid256, which is what
/// `SHKNDA-2/8` and `SHKNDA-2/9` want.
///
/// Applied to every Brahmic run it gains **six** cases in `SHKNDA-2` and costs
/// **sixty-nine**, measured and not estimated:
///
/// | Section | Before | After |
/// | --- | --- | --- |
/// | `SHKNDA-2` | 8 | **14** |
/// | `SHKNDA-1`, `SHKNDA-3` | 34, 31 | 33, 30 |
/// | `SHBALI-1`, `SHBALI-2` | 20, 12 | 13, 7 |
/// | `SHLANA-1`, `SHLANA-2`, `SHLANA-3`, `SHLANA-4` | 51, 32, 12, 2 | 48, 16, 10, 1 |
/// | `SHLANA-5`, `SHLANA-6`, `SHLANA-7`, `SHLANA-8` | 13, 7, 16, 11 | 11, 5, 5, 6 |
/// | `SHLANA-10` | 35 | 22 |
///
/// `SHLANA-2/6` is the sentence the other side is making. `ᩉ᩠ᨶᩦ` is a
/// consonant, a sakot and a consonant with a vowel sign, and it expects
/// gid226, gid383, gid270 — **base, subjoined consonant, mark**, which is the
/// exact reverse of what `SHKNDA-2/1` expects for the same shape. The
/// Universal Shaping Engine's own published cluster order is Tai Tham's:
/// below-base forms, then post-base forms, then the vowels. Kannada's is the
/// Indic model's, and `SHKNDA-2`'s own description says so in as many words —
/// *"a correct implementation should call the Kannada Shaping Engine"*.
///
/// **No property this crate reads separates the two.** Both matras are
/// `Vowel_Dependent`; `Indic_Positional_Category` calls U+0CBE `Right` and
/// U+1A66 the same. What separates them is which shaping *engine* the script
/// belongs to, and choosing one per script is a second cluster model — the
/// thing `docs/design/shaping.md` calls effectively unbounded and the reason
/// milestone 5 is the roadmap's largest item. So the six cases stay unclaimed
/// and this is the record of what they would cost.
pub(crate) fn reorder(categories: &[Category]) -> Option<Vec<usize>> {
    if !categories
        .iter()
        .any(|category| category.role() == Role::PreBase)
    {
        return None;
    }
    let mut order: Vec<usize> = (0..categories.len()).collect();
    let mut moved = false;
    for (at, category) in categories.iter().enumerate() {
        // `at` indexes the *original* sequence, and `order` says where each
        // original character now sits, so the position to lift from is the
        // one holding `at`.
        let Some(from) = order.iter().position(|which| *which == at) else {
            continue;
        };
        // The insertion point is the front of the syllable and stays there,
        // which is what reverses a pair of pre-base characters.
        if category.role() == Role::PreBase && from > 0 {
            let lifted = order.remove(from);
            order.insert(0, lifted);
            moved = true;
        }
    }
    moved.then_some(order)
}

#[cfg(test)]
mod tests {
    use super::{category, is_brahmic, reorder, syllables, Category, Role};

    /// Tai Tham, and the case text-rendering-tests SHLANA-1/21 states: a
    /// consonant and a vowel typed in that order and drawn in the other.
    #[test]
    fn a_pre_base_vowel_is_a_pre_base_vowel() {
        assert_eq!(category('\u{1A3D}'), Category::B, "TAI THAM LETTER BHA");
        assert_eq!(
            category('\u{1A6E}'),
            Category::VPre,
            "TAI THAM VOWEL SIGN E is Vowel_Dependent and Left"
        );
        assert_eq!(category('\u{1A6E}').role(), Role::PreBase);
        // And one that is drawn above rather than before, so the positional
        // property is doing the work rather than the syllabic one alone.
        assert_eq!(category('\u{1A65}'), Category::VAbv, "VOWEL SIGN I is Top");
        assert_eq!(category('\u{1A65}').role(), Role::Mark);
        assert_eq!(category('\u{1A60}'), Category::H, "SAKOT");
        assert_eq!(category('a').role(), Role::Other);
    }

    /// `Visual_Order_Left` is already where it is drawn. Thai's `SARA E` is
    /// the classic one, and moving it would break a script that was correct.
    #[test]
    fn a_visual_order_left_vowel_does_not_move() {
        // `U+0E40 THAI CHARACTER SARA E` is a dependent vowel drawn before its
        // consonant and **typed** there too, which is what `Visual_Order_Left`
        // means. It is a letter (`Lo`), so the classification makes it a base
        // of its own, and the one thing that must not happen to it is being
        // moved somewhere it already is.
        assert_eq!(category('\u{0E40}'), Category::B);
        assert_eq!(category('\u{0E01}'), Category::B, "THAI CHARACTER KO KAI");
        assert!(reorder(&[category('\u{0E40}'), category('\u{0E01}')]).is_none());
    }

    #[test]
    fn a_pre_base_vowel_moves_in_front_of_its_base() {
        let cats = [Category::B, Category::VPre];
        assert_eq!(reorder(&cats), Some(vec![1, 0]));
        // A pre-base medial and a pre-base vowel modifier move the same way.
        assert_eq!(reorder(&[Category::B, Category::MPre]), Some(vec![1, 0]));
        assert_eq!(reorder(&[Category::B, Category::VmPre]), Some(vec![1, 0]));
    }

    /// A halant does **not** move the insertion point: the vowel goes in front
    /// of the whole conjunct.
    ///
    /// text-rendering-tests `SHBALI-2/1` is this shape — `KA ADEG-ADEG PA
    /// TALING` — and its expected rendering draws the taling first. The
    /// opposite rule stood here until the fixtures were run against it, and
    /// dropping it moved thirty cases across six sections.
    #[test]
    fn a_halant_does_not_move_the_insertion_point() {
        let cats = [Category::B, Category::H, Category::B, Category::VPre];
        assert_eq!(reorder(&cats), Some(vec![3, 0, 1, 2]));
    }

    /// Two pre-base characters in one syllable come out in the **reverse** of
    /// the order they were typed in.
    ///
    /// Tai Tham `SHLANA-6/2` and `SHLANA-6/4` are where that is adjudicated:
    /// each holds `U+1A55 CONSONANT SIGN MEDIAL RA` and a pre-base vowel in
    /// one syllable, and each expects the vowel drawn first. The opposite rule
    /// was tried and costs three cases across two sections.
    #[test]
    fn two_pre_base_characters_come_out_reversed() {
        let cats = [Category::B, Category::MPre, Category::VPre];
        assert_eq!(reorder(&cats), Some(vec![2, 1, 0]));
        assert_eq!(category('\u{1A55}'), Category::MPre);
    }

    #[test]
    fn a_syllable_with_nothing_to_move_moves_nothing() {
        assert!(reorder(&[Category::B, Category::VAbv]).is_none());
        assert!(reorder(&[]).is_none());
        // A pre-base character already at the front is already right.
        assert!(reorder(&[Category::VPre, Category::B]).is_none());
    }

    #[test]
    fn a_halant_keeps_two_consonants_in_one_syllable() {
        // Devanagari ka, virama, ka: one syllable, not two.
        assert_eq!(
            syllables(&"\u{915}\u{94D}\u{915}".chars().collect::<Vec<char>>()),
            vec![1, 1, 1]
        );
        // Without the virama they are two.
        assert_eq!(
            syllables(&"\u{915}\u{915}".chars().collect::<Vec<char>>()),
            vec![1, 2]
        );
    }

    #[test]
    fn what_is_not_brahmic_is_in_no_syllable() {
        assert_eq!(
            syllables(&"a\u{915}b".chars().collect::<Vec<char>>()),
            vec![0, 1, 0]
        );
        assert!(syllables(&"hello".chars().collect::<Vec<char>>())
            .iter()
            .all(|id| *id == 0));
        assert!(syllables(&"".chars().collect::<Vec<char>>()).is_empty());
    }

    /// A mark with no base in front of it is USE's "broken cluster". It is
    /// still numbered, so the features that would repair it reach it and
    /// nothing else.
    #[test]
    fn a_mark_with_no_base_is_still_a_syllable() {
        assert_eq!(
            syllables(&"\u{94D}\u{915}".chars().collect::<Vec<char>>()),
            vec![1, 1]
        );
        assert_eq!(
            syllables(&"a\u{94D}".chars().collect::<Vec<char>>()),
            vec![0, 1]
        );
    }

    /// FT-08.4: a character of every class and subclass *Character
    /// classification* derives, as `docs/design/shaping.md`'s *Character
    /// categories* transcribes it — each row of its table, the subclasses by
    /// position, the overrides, and the four cases the table leaves open and
    /// this crate reads.
    #[test]
    fn every_use_category_is_the_published_tables() {
        let cases: &[(char, Category, &str)] = &[
            // B: a consonant, an independent vowel, a number, a tone letter,
            // a dependent sign that is a letter (Lo), a joining letter.
            (
                '\u{1A20}',
                Category::B,
                "TAI THAM LETTER HIGH KA, Consonant",
            ),
            (
                '\u{0C86}',
                Category::B,
                "KANNADA LETTER AA, Vowel_Independent",
            ),
            ('\u{1A80}', Category::B, "TAI THAM HORA DIGIT ZERO, Number"),
            (
                '\u{0E32}',
                Category::B,
                "THAI SARA AA, Vowel_Dependent and Lo",
            ),
            ('\u{07CA}', Category::B, "NKO LETTER A, Joining_Type D"),
            (
                '\u{AA29}',
                Category::VmAbv,
                "CHAM VOWEL SIGN AA, Bindu by override, Mn",
            ),
            // The fixed code points.
            ('\u{034F}', Category::Cgj, "COMBINING GRAPHEME JOINER"),
            (
                '\u{FE00}',
                Category::Vs,
                "VARIATION SELECTOR-1, over its override",
            ),
            ('\u{2060}', Category::Wj, "WORD JOINER"),
            ('\u{2022}', Category::Gb, "BULLET"),
            ('\u{25FB}', Category::Gb, "WHITE MEDIUM SQUARE"),
            (
                '\u{00A0}',
                Category::Gb,
                "NO-BREAK SPACE, Consonant_Placeholder",
            ),
            // The consonant modifiers, above and below.
            (
                '\u{0A71}',
                Category::CmAbv,
                "GURMUKHI ADDAK, Gemination_Mark, Top",
            ),
            ('\u{093C}', Category::CmBlw, "DEVANAGARI SIGN NUKTA, Bottom"),
            ('\u{0CBC}', Category::CmBlw, "KANNADA SIGN NUKTA, Bottom"),
            // A consonant with a stacker.
            ('\u{0CF1}', Category::Cs, "KANNADA SIGN JIHVAMULIYA"),
            // The finals.
            (
                '\u{1A58}',
                Category::FAbv,
                "TAI THAM SIGN MAI KANG LAI, Top",
            ),
            ('\u{1939}', Category::FBlw, "LIMBU SIGN MUKPHRENG, Bottom"),
            ('\u{1930}', Category::FPst, "LIMBU SMALL LETTER KA, Right"),
            (
                '\u{17CC}',
                Category::FAbv,
                "KHMER SIGN ROBAT, Consonant_Succeeding_Repha",
            ),
            (
                '\u{09FE}',
                Category::Fm,
                "BENGALI SANDHI MARK, Syllable_Modifier",
            ),
            // The halants.
            ('\u{094D}', Category::H, "DEVANAGARI SIGN VIRAMA"),
            (
                '\u{1A60}',
                Category::H,
                "TAI THAM SIGN SAKOT, Invisible_Stacker",
            ),
            ('\u{1107F}', Category::Hn, "BRAHMI NUMBER JOINER"),
            // Independent bases.
            ('\u{1AAD}', Category::Ind, "TAI THAM SIGN CAANG, Po"),
            (
                'a',
                Category::Ind,
                "a letter of no syllabic value and not Common",
            ),
            // The medials, by position.
            (
                '\u{1A55}',
                Category::MPre,
                "TAI THAM CONSONANT SIGN MEDIAL RA, Left",
            ),
            (
                '\u{1171F}',
                Category::MAbv,
                "AHOM CONSONANT SIGN MEDIAL LIGATING RA, Top",
            ),
            (
                '\u{1A56}',
                Category::MBlw,
                "TAI THAM CONSONANT SIGN MEDIAL LA, Bottom",
            ),
            (
                '\u{103B}',
                Category::MPst,
                "MYANMAR CONSONANT SIGN MEDIAL YA, Right",
            ),
            // A number base that joins.
            (
                '\u{11052}',
                Category::N,
                "BRAHMI NUMBER ONE, Brahmi_Joining_Number",
            ),
            // Others.
            (' ', Category::O, "SPACE, Zs"),
            ('1', Category::O, "DIGIT ONE, Common"),
            ('\u{0300}', Category::O, "COMBINING GRAVE ACCENT, Inherited"),
            // The rephas.
            (
                '\u{0D4E}',
                Category::R,
                "MALAYALAM LETTER DOT REPH, Consonant_Preceding_Repha",
            ),
            (
                '\u{111C2}',
                Category::R,
                "SHARADA SIGN JIHVAMULIYA, Consonant_Prefixed",
            ),
            // A reordering killer.
            ('\u{1BF2}', Category::Rk, "BATAK PANGOLAT"),
            // A reserved code point.
            ('\u{0378}', Category::Rsv, "unassigned"),
            // The symbols.
            ('\u{09F2}', Category::S, "BENGALI RUPEE MARK, Sc"),
            ('\u{1B66}', Category::S, "BALINESE MUSICAL SYMBOL, So"),
            // A subjoined consonant.
            (
                '\u{1A5B}',
                Category::Sub,
                "TAI THAM CONSONANT SIGN HIGH RATHA OR LOW PA",
            ),
            // The vowels, by position.
            ('\u{1A6E}', Category::VPre, "TAI THAM VOWEL SIGN E, Left"),
            ('\u{1A65}', Category::VAbv, "TAI THAM VOWEL SIGN I, Top"),
            ('\u{1A69}', Category::VBlw, "TAI THAM VOWEL SIGN U, Bottom"),
            ('\u{1A63}', Category::VPst, "TAI THAM VOWEL SIGN AA, Right"),
            (
                '\u{1A7A}',
                Category::VAbv,
                "TAI THAM SIGN RA HAAM, Pure_Killer",
            ),
            ('\u{093F}', Category::VPre, "DEVANAGARI VOWEL SIGN I, Left"),
            // The vowel modifiers, by position.
            (
                '\u{1C34}',
                Category::VmPre,
                "LEPCHA CONSONANT SIGN NYIN-DO, Left",
            ),
            (
                '\u{1A75}',
                Category::VmAbv,
                "TAI THAM SIGN TONE-1, Tone_Mark",
            ),
            (
                '\u{0952}',
                Category::VmBlw,
                "DEVANAGARI STRESS SIGN ANUDATTA, Bottom",
            ),
            (
                '\u{0C82}',
                Category::VmPst,
                "KANNADA SIGN ANUSVARA, Bindu, Right",
            ),
            // The joiners.
            ('\u{200D}', Category::Zwj, "ZERO WIDTH JOINER"),
            ('\u{200C}', Category::Zwnj, "ZERO WIDTH NON-JOINER"),
            // The hieroglyph classes, from the override file's
            // Extended_Syllabic_Category.
            ('\u{13000}', Category::G, "EGYPTIAN HIEROGLYPH A001"),
            (
                '\u{13430}',
                Category::J,
                "EGYPTIAN HIEROGLYPH VERTICAL JOINER",
            ),
            (
                '\u{13437}',
                Category::Sb,
                "EGYPTIAN HIEROGLYPH BEGIN SEGMENT",
            ),
            (
                '[',
                Category::Sb,
                "LEFT SQUARE BRACKET, Hieroglyph_Mark_Begin",
            ),
            ('\u{13438}', Category::Se, "EGYPTIAN HIEROGLYPH END SEGMENT"),
            (
                ']',
                Category::Se,
                "RIGHT SQUARE BRACKET, Hieroglyph_Mark_End",
            ),
            (
                '\u{13440}',
                Category::Hr,
                "EGYPTIAN HIEROGLYPH MIRROR HORIZONTALLY",
            ),
            (
                '\u{13447}',
                Category::Hm,
                "EGYPTIAN HIEROGLYPH MODIFIER DAMAGED AT TOP START",
            ),
            // The syllabic overrides the corpus reaches.
            (
                '\u{1A7B}',
                Category::VmAbv,
                "TAI THAM SIGN MAI SAM, Tone_Mark by override",
            ),
            (
                '\u{25CC}',
                Category::B,
                "DOTTED CIRCLE, Consonant by override",
            ),
            (
                '\u{1B6B}',
                Category::CmAbv,
                "BALINESE COMBINING TEGEH, Symbol_Modifier by override",
            ),
            (
                '\u{1820}',
                Category::B,
                "MONGOLIAN LETTER A, Consonant by override",
            ),
            (
                '\u{0F7F}',
                Category::Ind,
                "TIBETAN SIGN RNAM BCAD, Consonant_Dead by override",
            ),
            // The positional overrides.
            (
                '\u{103C}',
                Category::MPre,
                "MYANMAR MEDIAL RA, Left by override",
            ),
            (
                '\u{0E33}',
                Category::B,
                "THAI SARA AM, a letter whatever its position",
            ),
            (
                '\u{1A18}',
                Category::VAbv,
                "BUGINESE VOWEL SIGN U, Top by override",
            ),
            // The four this crate reads where the table is silent.
            (
                '\u{1A5A}',
                Category::B,
                "TAI THAM CONSONANT SIGN LOW PA, Consonant_Initial_Postfixed",
            ),
            (
                '\u{1C36}',
                Category::Fm,
                "LEPCHA SIGN RAN, Consonant_Final_Modifier",
            ),
            (
                '\u{1171E}',
                Category::MPre,
                "AHOM MEDIAL RA, Top_And_Bottom_And_Left",
            ),
            (
                '\u{1BC9D}',
                Category::CmBlw,
                "DUPLOYAN THICK LETTER SELECTOR, Overstruck",
            ),
        ];
        for (c, wanted, what) in cases {
            assert_eq!(category(*c), *wanted, "U+{:04X} {what}", u32::from(*c));
        }
        // Every class and subclass is reached by one of the cases above.
        let reached = |class: Category| cases.iter().any(|(_, got, _)| *got == class);
        for class in [
            Category::B,
            Category::Cgj,
            Category::CmAbv,
            Category::CmBlw,
            Category::Cs,
            Category::FAbv,
            Category::FBlw,
            Category::FPst,
            Category::Fm,
            Category::G,
            Category::Gb,
            Category::H,
            Category::Hm,
            Category::Hn,
            Category::Hr,
            Category::Ind,
            Category::J,
            Category::MPre,
            Category::MAbv,
            Category::MBlw,
            Category::MPst,
            Category::N,
            Category::O,
            Category::R,
            Category::Rk,
            Category::Rsv,
            Category::S,
            Category::Sb,
            Category::Se,
            Category::Sub,
            Category::VPre,
            Category::VAbv,
            Category::VBlw,
            Category::VPst,
            Category::VmPre,
            Category::VmAbv,
            Category::VmBlw,
            Category::VmPst,
            Category::Vs,
            Category::Wj,
            Category::Zwj,
            Category::Zwnj,
        ] {
            assert!(reached(class), "no case reaches {class:?}");
        }
    }

    /// The roles milestone 5's model reads keep its answers on every class
    /// the corpus types, and the plan's test is still Unicode's syllabic
    /// category, untouched by the overrides.
    #[test]
    fn the_roles_are_milestone_fives_categories() {
        for (class, role) in [
            (Category::B, Role::Base),
            (Category::Gb, Role::Base),
            (Category::H, Role::Halant),
            (Category::VPre, Role::PreBase),
            (Category::MPre, Role::PreBase),
            (Category::VAbv, Role::Mark),
            (Category::VmAbv, Role::Mark),
            (Category::FAbv, Role::Mark),
            (Category::Sub, Role::Mark),
            (Category::Zwnj, Role::Mark),
            (Category::Cgj, Role::Other),
            (Category::Ind, Role::Other),
            (Category::S, Role::Other),
            (Category::O, Role::Other),
        ] {
            assert_eq!(class.role(), role, "{class:?}");
        }
        assert!(is_brahmic('\u{1A20}'));
        assert!(is_brahmic('\u{1A7B}'));
        assert!(!is_brahmic('a'));
        // A Tifinagh letter is a base by the override file and takes no
        // Brahmic plan by it.
        assert_eq!(category('\u{2D30}'), Category::B);
        assert!(!is_brahmic('\u{2D30}'));
    }
}
