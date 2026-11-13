//! Tier-1: coefficient bit modelling (ITU-T T.800 Annex D).
//!
//! Tier-2 said which bytes belong to which code-block. This is what turns
//! those bytes into numbers: three coding passes per bit-plane over the MQ
//! coder of Annex C, with the context formation of Annex D.
//!
//! # Nineteen contexts, and why their *numbering* is not free here
//!
//! [`crate::mq`] carries no notion of what it codes — a caller supplies one
//! adaptive probability state per decision and gets a bit back. JBIG2 may
//! therefore number its contexts however it likes, because T.88 E.3.6 starts
//! every one of them identical and any bijection of the numbering is
//! invisible to an encoder and a decoder alike. **That is exactly what stops
//! being true here.** T.800 Table D.7 fixes three starting states — the
//! all-insignificant zero-coding context at 4, run-length at 3, UNIFORM at
//! 46 — so a relabelling moves a context onto the wrong initial
//! probabilities, and a relabelling *within* the eight that all start at zero
//! is invisible to any round trip at all.
//!
//! Gap 17 proved the second half of that the hard way: it transposed two of
//! JBIG2's template 0 context bits and T.88's Annex H.1 still decoded to its
//! published picture byte for byte. So the tables below are asserted **entry
//! by entry against the standard's own numbers** in
//! `super::tests::tier1`, not through a round trip, and JPEG 2000's exposure
//! is the wider one: three zero-coding mappings (Table D.1, one per subband
//! orientation), a sign table with an XOR bit (Tables D.2 and D.3), magnitude
//! refinement (Table D.4) and three non-zero initial states (Table D.7).
//!
//! # The one integrity check the format offers for free
//!
//! When a codestream signals segmentation symbols (Table A.19 bit 0x20), each
//! cleanup pass ends with the four decisions `1010` in the UNIFORM context.
//! They are decoded **and checked**. A build that decoded them and threw them
//! away would have discarded the only thing in JPEG 2000 that says the
//! arithmetic decoder has gone out of step — and tier-1 is precisely where
//! being subtly out of step produces plausible coefficients rather than an
//! error, because everything downstream of here is a smoothing operator.
//!
//! # What comes out
//!
//! Signed magnitudes, in the alignment `CodeBlock::planes` describes: the
//! most significant bit-plane the packet header paid for is bit
//! `planes - 1`. Turning that into a coefficient with a *magnitude* is E.1's
//! job and milestone 4's, because it needs the quantisation step and the
//! guard bits, and both are attacker-controlled numbers that have to be
//! clamped before they reach a plane.

use super::codestream::Codestream;
use super::passes::Schedule;
use super::tier2::{CodeBlock, Orientation, Segment, Tile};
use super::wavelet::Needs;
use super::{Refusal, MAX_JPX_WORK};
use crate::mq::{MqContexts, MqDecoder, MqRun};
use tinker_pdf_pool::Executor;
#[cfg(test)]
use tinker_pdf_pool::Parallelism;

// --- the nineteen contexts (T.800 Annex D) ------------------------------

/// Contexts 0 to 8: zero coding, Table D.1. Context 0 is the
/// all-insignificant neighbourhood and is one of Table D.7's three.
pub(crate) const ZERO_CODING: usize = 0;
/// Contexts 9 to 13: sign coding, Table D.3.
// Read by the Annex D table tests rather than by the decoder, which gets
// absolute indices back from the context functions above. Kept because they
// are the layout D.1 to D.4 describe, and a reader checking the tables against
// the standard needs the boundaries named.
#[allow(dead_code, reason = "the Annex D layout, asserted by the table tests")]
pub(crate) const SIGN_CODING: usize = 9;
/// Contexts 14 to 16: magnitude refinement, Table D.4.
#[allow(dead_code, reason = "the Annex D layout, asserted by the table tests")]
pub(crate) const MAGNITUDE_REFINEMENT: usize = 14;
/// Context 17: run-length, used by the cleanup pass for a column of four
/// insignificant coefficients with an entirely insignificant neighbourhood.
pub(crate) const RUN_LENGTH: usize = 17;
/// Context 18: UNIFORM, used for the two bits that locate the first
/// significant coefficient after a run and for the segmentation symbols.
pub(crate) const UNIFORM: usize = 18;
/// How many there are. Table D.7 has a row for every one of them.
pub(crate) const CONTEXTS: usize = 19;

/// The four decisions D.5 ends a cleanup pass with when segmentation symbols
/// are signalled, most significant first.
const SEGMENTATION_SYMBOL: u8 = 0b1010;

/// The most magnitude bit-planes a code-block may carry.
///
/// A **per-item** cap, and it says so because [`MAX_JPX_WORK`] is the work
/// cap and this is not. T.800's own arithmetic gives 37 — `Mb` is at most
/// `G + eps - 1` with seven guard bits and a five-bit exponent — but a
/// magnitude of 37 bits does not fit beside a sign in an `i32`, and a decoder
/// that truncated it would produce coefficients that are plausible and wrong,
/// which is this codec's whole hazard. So 31 is refused past rather than
/// clamped to.
const MAX_BIT_PLANES: u32 = 31;

/// The most coding passes one code-block may declare: `3 * planes - 2`, since
/// the first bit-plane carries only a cleanup pass.
pub(crate) const MAX_PASSES: u32 = 3 * MAX_BIT_PLANES - 2;

/// T.800 Table D.1: the zero-coding context for a neighbourhood.
///
/// `h` is the number of significant horizontal neighbours (0 to 2), `v` the
/// vertical (0 to 2) and `d` the diagonal (0 to 4). **There are three
/// mappings, not one**, and choosing between them is what the subband
/// orientation is for: LL and LH share one, HL is that one with the two axes
/// interchanged, and HH is a different table altogether keyed on the diagonal
/// count first.
///
/// Reading the wrong column is invisible to a round trip and invisible to the
/// segmentation symbol as well — it is a permutation of nine adaptive states
/// that all start at zero but one, so the coder stays in step and simply
/// codes against the wrong probabilities. What it costs is compression
/// fidelity on a *truncated* stream and nothing at all on a complete one,
/// which is the worst possible failure signature. Hence
/// `super::tests::tier1`'s exhaustive assertion against the table.
///
/// A `const fn`, because the decoder's lookup tables ([`ZERO_CODING_LL_LH`]
/// and its two siblings) are this function evaluated at every neighbourhood
/// when the crate is compiled, so the tables the tests assert and the tables
/// the passes read are one transcription.
pub(crate) const fn zero_coding_context(orientation: Orientation, h: u32, v: u32, d: u32) -> usize {
    // Table D.1's HL column is the LL/LH column with ΣH and ΣV interchanged.
    // Writing it as a swap rather than as a second nine-row table is the one
    // place this transcription is not literal, and it is exactly how the
    // standard words it: "Table D.1 ... with the horizontal and vertical
    // neighbour counts interchanged".
    let (h, v) = match orientation {
        Orientation::Hl => (v, h),
        Orientation::Ll | Orientation::Lh | Orientation::Hh => (h, v),
    };
    if matches!(orientation, Orientation::Hh) {
        // The HH column is keyed on ΣD first and then on ΣH + ΣV.
        let hv = h + v;
        return if d >= 3 {
            8
        } else if d == 2 {
            if hv >= 1 {
                7
            } else {
                6
            }
        } else if d == 1 {
            if hv >= 2 {
                5
            } else if hv == 1 {
                4
            } else {
                3
            }
        } else if hv >= 2 {
            2
        } else if hv == 1 {
            1
        } else {
            0
        };
    }
    if h == 2 {
        8
    } else if h == 1 {
        if v >= 1 {
            7
        } else if d >= 1 {
            6
        } else {
            5
        }
    } else if v == 2 {
        4
    } else if v == 1 {
        3
    } else if d >= 2 {
        2
    } else if d == 1 {
        1
    } else {
        0
    }
}

/// T.800 Table D.2: what one axis's two neighbours contribute to the sign
/// context.
///
/// Each argument is that neighbour's state: 0 insignificant, 1 significant
/// and positive, −1 significant and negative. The table's nine rows collapse
/// to a clamped sum — "both positive, or one positive and one insignificant"
/// is 1; "both insignificant, or one of each sign" is 0 — and the clamp is
/// what makes two positives and one positive the same contribution.
pub(crate) const fn sign_contribution(a: i32, b: i32) -> i32 {
    let sum = a + b;
    if sum > 1 {
        1
    } else if sum < -1 {
        -1
    } else {
        sum
    }
}

/// T.800 Table D.3: `(context, XOR bit)` from the horizontal and vertical
/// contributions, each in −1 to 1.
///
/// **The XOR bit is half the table.** Five contexts serve nine
/// neighbourhoods, because a neighbourhood and its sign-reversal share
/// statistics; the bit says which of the pair this is, and the decoded
/// decision is exclusive-ORed with it to get the sign. Drop the bit and every
/// coefficient in four of the nine neighbourhoods comes out with the wrong
/// sign — a picture with inverted texture in it, not an error, and one no
/// round trip through an encoder that dropped the same bit would ever notice.
pub(crate) const fn sign_coding_context(h: i32, v: i32) -> (usize, u8) {
    match (h, v) {
        (1, 1) => (13, 0),
        (1, 0) => (12, 0),
        (1, -1) => (11, 0),
        (0, 1) => (10, 0),
        (0, 0) => (9, 0),
        (0, -1) => (10, 1),
        (-1, 1) => (11, 1),
        (-1, 0) => (12, 1),
        // (-1, -1). The arms above are exhaustive over the nine the
        // contributions can take; anything else is a caller error and lands
        // here rather than panicking (ruling 1).
        _ => (13, 1),
    }
}

/// T.800 Table D.4: the magnitude refinement context.
///
/// `neighbours` is ΣH + ΣV + ΣD, and `first` says whether this is the
/// coefficient's first refinement — that is, whether it became significant in
/// the bit-plane immediately above. The first-refinement distinction is the
/// row a decoder loses by tracking significance and forgetting that
/// refinement has its own flag, and losing it puts every coefficient in
/// context 16 from the start.
pub(crate) const fn magnitude_refinement_context(neighbours: u32, first: bool) -> usize {
    if !first {
        16
    } else if neighbours >= 1 {
        15
    } else {
        14
    }
}

/// T.800 Table D.7: the nineteen contexts, three of which do not start at
/// zero.
///
/// T.88 E.3.6 starts every context at state 0 with MPS 0, which is what
/// [`MqContexts::new`] gives and what makes JBIG2's numbering a free choice.
/// JPEG 2000 does not start there, and [`MqContexts::set_state`] exists for
/// exactly this — including the half that is easy to miss, that
/// [`MqContexts::reset`] must return here rather than to zero, since D.2
/// resets the contexts at the start of every code-block and the second one in
/// a stream would otherwise decode against T.88's probabilities.
pub(crate) fn initial_contexts() -> MqContexts {
    let mut contexts = MqContexts::new(CONTEXTS);
    contexts.set_state(ZERO_CODING, 4, 0);
    contexts.set_state(RUN_LENGTH, 3, 0);
    contexts.set_state(UNIFORM, 46, 0);
    contexts
}

// --- one code-block -----------------------------------------------------

/// Which of the three passes a pass index is (T.800 D.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pass {
    /// D.2.1, over the coefficients with a significant neighbour.
    Significance,
    /// D.2.3, over the coefficients that were already significant.
    Refinement,
    /// D.2.4, over everything the other two did not code.
    Cleanup,
}

/// The pass and bit-plane that coding pass `i` of a code-block with `planes`
/// magnitude bit-planes is.
///
/// The first pass of a code-block is a **cleanup** pass on the most
/// significant plane and nothing else — there is no significance to propagate
/// and nothing to refine — and every plane below it carries all three. A
/// decoder that opened with a significance propagation pass instead would
/// read the whole code-block one pass out of phase and produce coefficients
/// for it, which is why the order is derived in one place and asserted.
pub(crate) const fn pass_at(i: u32, planes: u32) -> (Pass, u32) {
    let plane = planes - 1 - i.div_ceil(3);
    let pass = match (i + 2) % 3 {
        0 => Pass::Significance,
        1 => Pass::Refinement,
        _ => Pass::Cleanup,
    };
    (pass, plane)
}

// --- the packed state (`docs/design/jpx-speed.md`, "Tier-1: packed per-stripe state") ---

// One word holds one column of one stripe — four coefficients, one above the
// next — and everything forming a context of Table D.1, D.3 or D.4 for any of
// the four reads, so a context costs that one word: a shift, a mask and a
// lookup.
//
// - **σ over a window three columns wide and six rows tall**, bits 0 to 17:
//   the column and its neighbours on either side, from the row above the
//   stripe to the row below it. `(column, row)` is bit `3 * row + column`:
//   `column` 0 the left neighbour, 1 the column itself and 2 the right; `row`
//   0 the row above the stripe, 1 to 4 the stripe's own and 5 the row below.
//   So the 3 × 3 neighbourhood of the stripe's row `r` is the nine bits from
//   `3 * r`, its own σ the fifth of them.
// - **χ, set where the coefficient is negative**, at the same places
//   [`CHI`] bits up. D.3 reads the signs of the four neighbours across a
//   coefficient, so only the column's own six rows and its neighbours' four
//   are ever set.
// - **π**, bits 36 to 39, one for each of the stripe's rows: coded by this
//   bit-plane's significance propagation pass.
// - **Refined**, bits 40 to 43: Table D.4's first-refinement distinction.
//
// A coefficient that becomes significant sets its bits in its own word and the
// neighbour bits in the up to eight words around it, so every word always
// holds its window. The magnitudes and the plane each coefficient is known to
// stay one a coefficient beside the words: they are values, not flags.

/// σ at `(column, row)` of a word's window.
const fn sigma(column: usize, row: usize) -> u64 {
    1 << (3 * row + column)
}

/// How far a word's χ bits sit above its σ bits.
const CHI: usize = 18;

/// χ at `(column, row)` of a word's window.
const fn chi(column: usize, row: usize) -> u64 {
    sigma(column, row) << CHI
}

/// π of the stripe's row `r`.
const fn pi(r: usize) -> u64 {
    1 << (36 + r)
}

/// The refined flag of the stripe's row `r`.
const fn refined(r: usize) -> u64 {
    1 << (40 + r)
}

/// Every σ bit of a word's window.
const WINDOW: u64 = (1 << 18) - 1;

/// σ of the column's own four rows.
const OWN: u64 = sigma(1, 1) | sigma(1, 2) | sigma(1, 3) | sigma(1, 4);

/// Every π bit of a word.
const VISITED: u64 = pi(0) | pi(1) | pi(2) | pi(3);

/// A row's eight neighbours among its nine window bits: all but its own, the
/// fifth.
const NEIGHBOURS: u64 = 0x1EF;

/// The four neighbours across a row's coefficient among its nine window bits
/// — above, left, right and below — whose signs D.3 reads.
const CROSS: u64 = 0xAA;

/// Bit `at` of `bits`, as a count.
const fn flag(bits: usize, at: usize) -> u32 {
    ((bits >> at) & 1) as u32
}

/// Table D.1 in `orientation`'s column at every neighbourhood a row's nine
/// window bits can hold: [`zero_coding_context`] itself, evaluated when the
/// crate is compiled.
const fn zero_coding_table(orientation: Orientation) -> [u8; 512] {
    let mut table = [0; 512];
    let mut bits = 0;
    while bits < 512 {
        // Bits 3 and 5 are the row's left and right neighbours, 1 and 7 the
        // ones above and below, and 0, 2, 6 and 8 the four corners.
        let h = flag(bits, 3) + flag(bits, 5);
        let v = flag(bits, 1) + flag(bits, 7);
        let d = flag(bits, 0) + flag(bits, 2) + flag(bits, 6) + flag(bits, 8);
        // A zero-coding context is at most 8, so the cast keeps every bit.
        table[bits] = zero_coding_context(orientation, h, v, d) as u8;
        bits += 1;
    }
    table
}

/// Table D.1's LL and LH column, by neighbourhood.
pub(crate) static ZERO_CODING_LL_LH: [u8; 512] = zero_coding_table(Orientation::Ll);

/// Table D.1's HL column, by neighbourhood.
pub(crate) static ZERO_CODING_HL: [u8; 512] = zero_coding_table(Orientation::Hl);

/// Table D.1's HH column, by neighbourhood.
pub(crate) static ZERO_CODING_HH: [u8; 512] = zero_coding_table(Orientation::Hh);

/// Neighbour `k`'s state in a [`SIGN_TABLE`] index, as Table D.2 reads it: 0
/// insignificant, 1 significant and positive, −1 significant and negative.
const fn signed(index: usize, k: usize) -> i32 {
    if (index >> (2 * k)) & 1 == 0 {
        0
    } else if (index >> (2 * k + 1)) & 1 == 1 {
        -1
    } else {
        1
    }
}

/// Tables D.2 and D.3 at every state the four neighbours across a coefficient
/// can be in, the context with its XOR bit at bit 7.
///
/// Index bit `2k` is neighbour `k`'s σ and bit `2k + 1` its χ, `k` 0 to 3 for
/// above, left, right and below, which is how [`Packed::sign_at`]
/// interleaves a row's [`CROSS`] bits of σ and χ.
const fn sign_table() -> [u8; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let h = sign_contribution(signed(index, 1), signed(index, 2));
        let v = sign_contribution(signed(index, 0), signed(index, 3));
        let (context, xor) = sign_coding_context(h, v);
        // A sign context is at most 13, which leaves bit 7 to the XOR bit.
        table[index] = context as u8 | (xor << 7);
        index += 1;
    }
    table
}

/// [`sign_table`], evaluated when the crate is compiled.
pub(crate) static SIGN_TABLE: [u8; 256] = sign_table();

/// Row `r`'s eight neighbours in `word`, as an index of a zero-coding table.
const fn neighbourhood(word: u64, r: usize) -> usize {
    ((word >> (3 * r)) & NEIGHBOURS) as usize
}

/// One code-block's state while the passes run over it: a word to each
/// column of each stripe, laid out as the comment above [`sigma`] says, and
/// each coefficient's magnitude and the plane it is known to.
pub(crate) struct Packed {
    w: usize,
    h: usize,
    stripes: usize,
    /// Words to a row of stripes: the code-block's columns and one on either
    /// side of them. No coefficient is in the outer two, which no context
    /// reads, so a coefficient at either edge tells its neighbours' words
    /// with no bounds to check.
    stride: usize,
    /// `(stripes + 2) * stride` words, a row above the first stripe and one
    /// below the last for the same reason.
    words: Vec<u64>,
    magnitude: Vec<u32>,
    /// The **lowest bit-plane whose magnitude bit was actually decoded** for
    /// each coefficient, which is not the same as the lowest plane the
    /// code-block reached.
    ///
    /// It exists because E.1.1.2's reconstruction point is defined on the
    /// interval a decoder *knows* a coefficient lies in, and on a truncated
    /// stream that interval differs per coefficient. One that became
    /// significant in a cleanup pass and never met a refinement pass is known
    /// only to `[2^p, 2^(p+1))`, and reconstructing it at the midpoint of the
    /// bottom plane instead puts it a quarter of its own magnitude low. A
    /// block carries at most [`MAX_BIT_PLANES`] planes, so a plane fits a
    /// byte.
    known: Vec<u8>,
    /// Table A.19 bit 3. Vertically causal context formation treats the next
    /// stripe as insignificant, so a stripe can be decoded knowing nothing
    /// below it, and only a stripe's last row has a neighbour there. So under
    /// it a coefficient of a stripe's first row sets nothing in the words of
    /// the stripe above, whose window row below stays clear, and causal and
    /// not read the same tables.
    causal: bool,
    /// Table D.1 in this block's orientation.
    zero_coding: &'static [u8; 512],
    /// What forming the contexts has read, for `a_context_costs_one_state_word`.
    #[cfg(test)]
    tally: std::cell::Cell<Tally>,
}

/// What a code-block's context formation did, counted: the contexts formed
/// and the state words read to form them.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    pub(crate) contexts: u64,
    pub(crate) words_read: u64,
}

impl Packed {
    pub(crate) fn new(w: usize, h: usize, orientation: Orientation, causal: bool) -> Packed {
        let stripes = h.div_ceil(4);
        let stride = w + 2;
        Packed {
            w,
            h,
            stripes,
            stride,
            words: vec![0; (stripes + 2) * stride],
            magnitude: vec![0; w * h],
            known: vec![0; w * h],
            causal,
            zero_coding: match orientation {
                Orientation::Ll | Orientation::Lh => &ZERO_CODING_LL_LH,
                Orientation::Hl => &ZERO_CODING_HL,
                Orientation::Hh => &ZERO_CODING_HH,
            },
            #[cfg(test)]
            tally: std::cell::Cell::new(Tally::default()),
        }
    }

    /// The word of column `x` of stripe `stripe`.
    const fn index(&self, x: usize, stripe: usize) -> usize {
        (stripe + 1) * self.stride + x + 1
    }

    /// The rows stripe `stripe` holds: four, or what is left of the block in
    /// its last. A stripe is four rows whatever the last one holds, and a row
    /// past the block's bottom holds no coefficient, so nothing ever sets its
    /// bits.
    fn rows(&self, stripe: usize) -> usize {
        (self.h - 4 * stripe).min(4)
    }

    /// The word at `i`, read to form a context: the one state read a context
    /// costs.
    fn word(&self, i: usize) -> u64 {
        #[cfg(test)]
        self.count(|tally| tally.words_read += 1);
        self.words[i]
    }

    /// Table D.1's context for row `r` of the column whose word is at `i`, in
    /// this block's orientation.
    fn zero_coding_at(&self, i: usize, r: usize) -> usize {
        #[cfg(test)]
        self.count(|tally| tally.contexts += 1);
        usize::from(self.zero_coding[neighbourhood(self.word(i), r)])
    }

    /// Tables D.2 and D.3 for row `r` of the column at `i`: the sign context
    /// and its XOR bit.
    fn sign_at(&self, i: usize, r: usize) -> (usize, u8) {
        #[cfg(test)]
        self.count(|tally| tally.contexts += 1);
        let word = self.word(i);
        let across = (word >> (3 * r)) & CROSS;
        let signs = (word >> (CHI + 3 * r)) & CROSS;
        let entry = SIGN_TABLE[((across >> 1) | signs) as usize];
        (usize::from(entry & 0x7F), entry >> 7)
    }

    /// Table D.4's context for row `r` of the column at `i`.
    fn refinement_at(&self, i: usize, r: usize) -> usize {
        #[cfg(test)]
        self.count(|tally| tally.contexts += 1);
        let word = self.word(i);
        magnitude_refinement_context(
            u32::from(neighbourhood(word, r) != 0),
            word & refined(r) == 0,
        )
    }

    /// Marks row `r` of the column at `i` significant — the coefficient at
    /// `at` — with its sign and the bit of the plane being coded, and sets its
    /// bits in the windows of the words around it.
    fn make_significant(&mut self, i: usize, at: usize, r: usize, negative: bool, plane: u32) {
        let sign = |column: usize, row: usize| if negative { chi(column, row) } else { 0 };
        let row = r + 1;
        self.words[i] |= sigma(1, row) | sign(1, row);
        self.words[i - 1] |= sigma(2, row) | sign(2, row);
        self.words[i + 1] |= sigma(0, row) | sign(0, row);
        if r == 0 && !self.causal {
            // The stripe above reads this row as the one below its own.
            let above = i - self.stride;
            self.words[above - 1] |= sigma(2, 5);
            self.words[above] |= sigma(1, 5) | sign(1, 5);
            self.words[above + 1] |= sigma(0, 5);
        }
        if r == 3 {
            // And the stripe below reads it as the one above its own.
            let below = i + self.stride;
            self.words[below - 1] |= sigma(2, 0);
            self.words[below] |= sigma(1, 0) | sign(1, 0);
            self.words[below + 1] |= sigma(0, 0);
        }
        self.magnitude[at] |= 1 << plane;
        // At most 30, below `MAX_BIT_PLANES`, so the cast keeps every bit.
        self.known[at] = plane as u8;
    }

    /// π is per bit-plane: cleared where a cleanup pass ends one.
    fn clear_visited(&mut self) {
        self.words.iter_mut().for_each(|word| *word &= !VISITED);
    }

    /// The signed coefficients, with the magnitude as `planes` bits.
    fn coefficients(&self) -> Vec<i32> {
        let mut out = Vec::with_capacity(self.w * self.h);
        // A block is at least one coefficient wide, `decode_code_block` having
        // returned before any state exists for one that is not; the `max`
        // keeps `chunks_exact` from ever being handed a zero.
        for (y, magnitudes) in self.magnitude.chunks_exact(self.w.max(1)).enumerate() {
            let (first, row) = (self.index(0, y / 4), y % 4 + 1);
            for (x, &m) in magnitudes.iter().enumerate() {
                // The magnitude is at most `MAX_BIT_PLANES` bits, so it fits
                // an `i32` beside its sign; the cast is not a truncation and
                // `MAX_BIT_PLANES` is what makes that true.
                let m = m as i32;
                out.push(if self.words[first + x] & chi(1, row) != 0 {
                    -m
                } else {
                    m
                });
            }
        }
        out
    }

    /// The decode's yield: the coefficients and, per coefficient, the plane
    /// whose *half* E.1.1.2 reconstructs at — zero for anything insignificant,
    /// which the dequantiser never reads because a coefficient that never
    /// became significant stays at zero rather than at half a step.
    fn into_decoded(self, planes: u32) -> Decoded {
        Decoded {
            coefficients: self.coefficients(),
            half_planes: self.known,
            planes,
        }
    }

    #[cfg(test)]
    fn count(&self, add: impl FnOnce(&mut Tally)) {
        let mut tally = self.tally.get();
        add(&mut tally);
        self.tally.set(tally);
    }
}

/// The packed state read coefficient by coefficient, for the tests that hold
/// it to the transcribed [`Block`].
#[cfg(test)]
impl Packed {
    /// The word and stripe row of `(x, y)`.
    const fn place(&self, x: usize, y: usize) -> (usize, usize) {
        (self.index(x, y / 4), y % 4)
    }

    /// `(σ, χ, π, refined)` at `(x, y)`, as [`Block::flags_at`] gives them.
    pub(crate) fn flags_at(&self, x: usize, y: usize) -> (bool, bool, bool, bool) {
        let (i, r) = self.place(x, y);
        let word = self.words[i];
        (
            word & sigma(1, r + 1) != 0,
            word & chi(1, r + 1) != 0,
            word & pi(r) != 0,
            word & refined(r) != 0,
        )
    }

    /// The three contexts at `(x, y)` — Table D.1's, Tables D.2 and D.3's
    /// with the XOR bit, and Table D.4's — each formed as the passes form it.
    pub(crate) fn contexts_at(&self, x: usize, y: usize) -> (usize, (usize, u8), usize) {
        let (i, r) = self.place(x, y);
        (
            self.zero_coding_at(i, r),
            self.sign_at(i, r),
            self.refinement_at(i, r),
        )
    }

    /// What forming the contexts has read so far.
    pub(crate) fn tally(&self) -> Tally {
        self.tally.get()
    }

    /// Marks `(x, y)` significant with the given sign, as a pass does.
    pub(crate) fn set_significant(&mut self, x: usize, y: usize, negative: bool) {
        let (i, r) = self.place(x, y);
        self.make_significant(i, y * self.w + x, r, negative, 0);
    }

    /// Marks `(x, y)` refined, as a refinement pass does.
    pub(crate) fn set_refined(&mut self, x: usize, y: usize) {
        let (i, r) = self.place(x, y);
        self.words[i] |= refined(r);
    }
}

/// One code-block's state as D.2 describes it, a flag a coefficient and a
/// neighbourhood of eight read flag by flag: the transcription the decoder's
/// [`Packed`] state replaced, kept for tests only.
///
/// The test encoder forms its contexts from it, so a round trip pits the
/// packed decoder against an independent state rather than against itself
/// (gap 17's lesson, this module's header), and
/// `the_packed_contexts_are_the_transcribed_ones_at_every_coefficient` holds
/// every context the packed state forms to this one's.
#[cfg(test)]
pub(crate) struct Block {
    w: usize,
    h: usize,
    orientation: Orientation,
    /// σ: significant. Once set it is never cleared.
    sigma: Vec<bool>,
    /// The sign, true for negative. Meaningless where σ is false, and read
    /// only through [`Block::signed`], which checks σ first.
    negative: Vec<bool>,
    /// Whether this coefficient has been magnitude-refined already, which is
    /// Table D.4's first-refinement distinction and is *not* σ.
    refined: Vec<bool>,
    /// π: coded in the significance propagation pass of the current
    /// bit-plane, and therefore skipped by this plane's cleanup pass. Cleared
    /// at the end of every cleanup pass.
    visited: Vec<bool>,
    /// Table A.19 bit 3, and the row the current stripe stops at.
    ///
    /// Vertically causal context formation treats everything in the *next*
    /// stripe as insignificant, so a stripe can be decoded knowing nothing
    /// below it. Only a coefficient in a stripe's last row has a neighbour
    /// there at all, and the four passes reach those neighbours through one
    /// function, so this is one field and one branch rather than four
    /// context rules.
    causal: bool,
    stripe_end: usize,
}

#[cfg(test)]
impl Block {
    pub(crate) fn new(w: usize, h: usize, orientation: Orientation) -> Block {
        let n = w * h;
        Block {
            w,
            h,
            orientation,
            sigma: vec![false; n],
            negative: vec![false; n],
            refined: vec![false; n],
            visited: vec![false; n],
            causal: false,
            stripe_end: usize::MAX,
        }
    }

    /// Turns on vertically causal context formation (Table A.19 bit 3).
    pub(crate) fn set_causal(&mut self, causal: bool) {
        self.causal = causal;
    }

    /// Names the stripe now being coded, for [`Block::significant`].
    fn enter_stripe(&mut self, top: usize) {
        // A stripe is four rows whatever the last one holds, and the bound is
        // the *stripe's*, not the rows present in it: a code-block whose
        // height is not a multiple of four has a short final stripe with
        // nothing below it either way.
        self.stripe_end = top.saturating_add(4);
    }

    const fn at(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    /// Whether `(x, y)` is significant, with everything outside the
    /// code-block insignificant.
    ///
    /// D.2's context formation is over the code-block alone: a neighbour in
    /// the next code-block of the same subband is *not* consulted, which is
    /// what makes a code-block independently decodable and what makes tier-2
    /// free to hand them over in any order.
    fn significant(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return false;
        }
        if self.causal && y as usize >= self.stripe_end {
            // Table A.19 bit 3: the stripe below this one does not exist for
            // context formation. Every one of D.2's context rules reaches its
            // neighbours through here, so this is the whole of the mode.
            return false;
        }
        self.sigma[self.at(x as usize, y as usize)]
    }

    /// `(x, y)`'s contribution to a sign context: 0 insignificant, 1
    /// significant positive, −1 significant negative.
    fn signed(&self, x: isize, y: isize) -> i32 {
        if !self.significant(x, y) {
            return 0;
        }
        if self.negative[self.at(x as usize, y as usize)] {
            -1
        } else {
            1
        }
    }

    /// `(ΣH, ΣV, ΣD)` for `(x, y)`.
    pub(crate) fn neighbours(&self, x: usize, y: usize) -> (u32, u32, u32) {
        let (x, y) = (x as isize, y as isize);
        let h = u32::from(self.significant(x - 1, y)) + u32::from(self.significant(x + 1, y));
        let v = u32::from(self.significant(x, y - 1)) + u32::from(self.significant(x, y + 1));
        let d = u32::from(self.significant(x - 1, y - 1))
            + u32::from(self.significant(x + 1, y - 1))
            + u32::from(self.significant(x - 1, y + 1))
            + u32::from(self.significant(x + 1, y + 1));
        (h, v, d)
    }

    /// Table D.1's context for `(x, y)`, in this block's own orientation.
    pub(crate) fn zero_coding(&self, x: usize, y: usize) -> usize {
        let (h, v, d) = self.neighbours(x, y);
        zero_coding_context(self.orientation, h, v, d)
    }

    /// Tables D.2 and D.3 for `(x, y)`: the sign context and its XOR bit.
    pub(crate) fn sign_coding(&self, x: usize, y: usize) -> (usize, u8) {
        let (x, y) = (x as isize, y as isize);
        let h = sign_contribution(self.signed(x - 1, y), self.signed(x + 1, y));
        let v = sign_contribution(self.signed(x, y - 1), self.signed(x, y + 1));
        sign_coding_context(h, v)
    }

    /// Table D.4's context for `(x, y)`.
    pub(crate) fn refinement(&self, x: usize, y: usize) -> usize {
        let (h, v, d) = self.neighbours(x, y);
        magnitude_refinement_context(h + v + d, !self.refined[self.at(x, y)])
    }

    /// Marks `(x, y)` significant with the given sign, as a pass does.
    pub(crate) fn set_significant(&mut self, x: usize, y: usize, negative: bool) {
        let i = self.at(x, y);
        self.sigma[i] = true;
        self.negative[i] = negative;
    }

    /// Marks `(x, y)` refined, as a refinement pass does.
    pub(crate) fn set_refined(&mut self, x: usize, y: usize) {
        let i = self.at(x, y);
        self.refined[i] = true;
    }

    /// `(σ, χ, π, refined)` at `(x, y)`, χ being the sign flag, true for
    /// negative and set only beside σ.
    pub(crate) fn flags_at(&self, x: usize, y: usize) -> (bool, bool, bool, bool) {
        let i = self.at(x, y);
        (
            self.sigma[i],
            self.negative[i],
            self.visited[i],
            self.refined[i],
        )
    }

    /// The three contexts at `(x, y)` as a pass coding `(x, y)`'s stripe
    /// forms them: Table D.1's, Tables D.2 and D.3's with the XOR bit, and
    /// Table D.4's.
    pub(crate) fn contexts_at(&mut self, x: usize, y: usize) -> (usize, (usize, u8), usize) {
        self.enter_stripe(y - y % 4);
        (
            self.zero_coding(x, y),
            self.sign_coding(x, y),
            self.refinement(x, y),
        )
    }

    /// Every `(x, y)` in D.2's scan order: stripes of four rows, column by
    /// column within a stripe, top to bottom within a column.
    ///
    /// Written once because all three passes share it, and because a pass
    /// that scanned row-major instead would still decode every coefficient,
    /// still consume every bit and still produce a picture.
    fn stripes(&self) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
        let (w, h) = (self.w, self.h);
        (0..h).step_by(4).flat_map(move |top| {
            let rows = (h - top).min(4);
            (0..w).map(move |x| (top, rows, x))
        })
    }
}

// --- the three passes (T.800 D.2.1 to D.2.4) ----------------------------

/// D.6's raw bit reader: the bits a bypassed pass gets instead of MQ
/// decisions.
///
/// "(A routine that undoes the effects of bit stuffing precedes the return of
/// bits. Specifically, this routine throws out the first bit after an 0xFF
/// byte value.)" — which is the same stuffing rule B.10.1 applies to packet
/// header bits, written in a different clause for a different stream.
/// [`super::tier2::PacketBits`] implements the other one, and
/// `the_two_stuffed_bit_readers_agree` pins that they agree, because two
/// copies of one rule is exactly the shape that drifts.
///
/// **Past the end the byte stream continues with 0xFF, and that is the
/// standard's answer rather than this build's.** D.4.1 gives it for the
/// arithmetic decoder — "the decoder shall extend the input bit stream to the
/// arithmetic coder with 0xFF bytes, as necessary, until all symbols have
/// been decoded" — and D.6 extends it to the raw stream in its own words, in
/// NOTE 2: "Since the decoder appends 0xFF values, as necessary, to the bit
/// stream representing the coding pass (see D.4.1), truncation of the bit
/// stream may be possible."
///
/// An earlier draft of this reader returned zero past the end and said in
/// this comment that "D.6 says nothing at all about a raw one". That was a
/// transcription failure rather than a judgement call: the sentence above is
/// two paragraphs below Equation (D-2) on the same page. It matters because a
/// truncated raw segment is exactly the case D.6's NOTE 2 says an encoder may
/// *deliberately* produce, so it is reachable from a conforming file and not
/// only from a damaged one.
///
/// The extension needs no bound: 0xFF's bits are all ones, and an 0xFF after
/// an 0xFF is unstuffed to seven of them, so every bit past the end is one
/// whichever byte preceded it. `at` stops at the end of the slice rather than
/// running on.
struct RawBits<'a> {
    data: &'a [u8],
    at: usize,
    buf: u8,
    ct: u32,
}

impl<'a> RawBits<'a> {
    const fn new(data: &'a [u8]) -> RawBits<'a> {
        RawBits {
            data,
            at: 0,
            buf: 0,
            ct: 0,
        }
    }

    fn bit(&mut self) -> u8 {
        if self.ct == 0 {
            let next = match self.data.get(self.at) {
                Some(&byte) => {
                    self.at += 1;
                    byte
                }
                // D.4.1 and D.6's NOTE 2: the decoder appends 0xFF.
                None => 0xFF,
            };
            // The stuffing rule: a byte following an 0xFF carries seven bits,
            // its most significant having been stuffed by the encoder.
            self.ct = if self.buf == 0xFF { 7 } else { 8 };
            self.buf = next;
        }
        self.ct -= 1;
        (self.buf >> self.ct) & 1
    }
}

/// [`RawBits`] as a closure, for `tests::code_block_styles`.
///
/// The reader is private because nothing outside this module decodes a raw
/// pass, and it stays private: what the test needs is a stream of bits to
/// compare against [`super::tier2::PacketBits`], not the type.
#[cfg(test)]
pub(crate) fn raw_bits_for_test(data: &[u8]) -> impl FnMut() -> u8 + '_ {
    let mut raw = RawBits::new(data);
    move || raw.bit()
}

/// Where one codeword segment's decisions come from.
///
/// Table D.9 makes a segment wholly raw or wholly arithmetic — a raw
/// significance propagation pass is never terminated, so it always shares its
/// segment with the raw magnitude refinement pass after it, and a cleanup
/// pass is always arithmetic and always ends a segment once bypass is on.
/// `passes::tests::a_codeword_segment_never_mixes_raw_and_arithmetic_passes`
/// is that invariant, and it is what lets this be one enum per segment rather
/// than a branch inside every decision.
enum Source<'a> {
    Mq(MqDecoder<'a>),
    Raw(RawBits<'a>),
}

/// Where a coding pass's decisions come from: a run of its segment's MQ
/// decoder, its registers held in locals for the pass, or D.6's raw reader.
///
/// The passes are written once over this, so an arithmetic pass carries no
/// branch on which of the two it is reading at any decision.
trait Decisions {
    /// One decision in `context`, or a raw pass's next bit.
    fn decide(&mut self, contexts: &mut MqContexts, context: usize) -> u8;

    /// The sign of a coefficient that has just become significant where a
    /// raw pass reads it straight from the stream, or `None` where it is
    /// D.2.2's context and XOR bit.
    fn raw_sign(&mut self) -> Option<bool>;
}

impl Decisions for MqRun<'_, '_> {
    #[inline(always)]
    fn decide(&mut self, contexts: &mut MqContexts, context: usize) -> u8 {
        self.decode_at(contexts, context)
    }

    #[inline(always)]
    fn raw_sign(&mut self) -> Option<bool> {
        None
    }
}

impl Decisions for RawBits<'_> {
    /// D.6: "the bits that would have been returned from the arithmetic
    /// coder are instead returned directly from the bit stream". The context
    /// is computed and discarded rather than not computed, because a raw pass
    /// leaves the context states untouched and the next arithmetic pass must
    /// find them as the last arithmetic pass left them.
    #[inline]
    fn decide(&mut self, _: &mut MqContexts, _: usize) -> u8 {
        self.bit()
    }

    #[inline]
    fn raw_sign(&mut self) -> Option<bool> {
        Some(self.bit() == 1)
    }
}

/// Everything one coding pass needs that is not the block itself.
struct Coder<'s, 'c, D> {
    source: &'s mut D,
    contexts: &'c mut MqContexts,
    segmentation_symbols: bool,
}

impl<D: Decisions> Coder<'_, '_, D> {
    fn decode(&mut self, context: usize) -> u8 {
        self.source.decide(self.contexts, context)
    }

    /// Runs `pass` on the plane `plane` of `block`.
    fn pass(&mut self, pass: Pass, block: &mut Packed, plane: u32) -> Result<(), Refusal> {
        match pass {
            Pass::Significance => self.significance_pass(block, plane),
            Pass::Refinement => self.refinement_pass(block, plane),
            Pass::Cleanup => self.cleanup_pass(block, plane)?,
        }
        Ok(())
    }

    /// D.2.2: the sign of row `r` of the column at `i`, which has just
    /// become significant.
    ///
    /// In a raw pass this is **not** D.2.2's context-and-XOR decode. Equation
    /// (D-2) replaces the whole of it: "signbit = raw_value, where raw_value
    /// = 1 is a negative sign bit and raw_value = 0 is a positive sign bit".
    /// Running the XOR bit over a raw sign inverts roughly half the signs of
    /// every bypassed pass, which is a picture rather than an error.
    fn sign(&mut self, block: &Packed, i: usize, r: usize) -> bool {
        if let Some(sign) = self.source.raw_sign() {
            return sign;
        }
        let (context, xorbit) = block.sign_at(i, r);
        (self.decode(context) ^ xorbit) == 1
    }

    /// D.2.1: every insignificant coefficient with at least one significant
    /// neighbour, in D.2's scan order: stripes of four rows, column by column
    /// within a stripe, top to bottom within a column.
    fn significance_pass(&mut self, block: &mut Packed, plane: u32) {
        for stripe in 0..block.stripes {
            let (first, rows) = (block.index(0, stripe), block.rows(stripe));
            for x in 0..block.w {
                let i = first + x;
                // Nothing significant in the column's window, so none of its
                // coefficients has a significant neighbour.
                if block.words[i] & WINDOW == 0 {
                    continue;
                }
                for r in 0..rows {
                    let word = block.words[i];
                    // The "preferred neighbourhood": at least one of the
                    // eight. Equivalently a non-zero Table D.1 context, since
                    // all three of its mappings send the all-insignificant
                    // neighbourhood and only that one to context 0 — but the
                    // condition is D.2.1's and is written as D.2.1 words it
                    // rather than inferred from the table, so a change to one
                    // cannot silently move the other.
                    if word & sigma(1, r + 1) != 0 || neighbourhood(word, r) == 0 {
                        continue;
                    }
                    block.words[i] |= pi(r);
                    let context = block.zero_coding_at(i, r);
                    if self.decode(context) == 1 {
                        let negative = self.sign(block, i, r);
                        let at = (4 * stripe + r) * block.w + x;
                        block.make_significant(i, at, r, negative, plane);
                    }
                }
            }
        }
    }

    /// D.2.3: every coefficient that was already significant before this
    /// bit-plane's significance propagation pass.
    fn refinement_pass(&mut self, block: &mut Packed, plane: u32) {
        for stripe in 0..block.stripes {
            let (first, rows) = (block.index(0, stripe), block.rows(stripe));
            for x in 0..block.w {
                let i = first + x;
                if block.words[i] & OWN == 0 {
                    continue;
                }
                for r in 0..rows {
                    let word = block.words[i];
                    // π excludes the coefficients this plane's significance
                    // pass just took, which is the whole reason π exists: a
                    // coefficient that became significant a moment ago has no
                    // magnitude bit left in this plane to refine.
                    if word & sigma(1, r + 1) == 0 || word & pi(r) != 0 {
                        continue;
                    }
                    let context = block.refinement_at(i, r);
                    let bit = self.decode(context);
                    block.words[i] |= refined(r);
                    // The plane is now known whichever way the bit went: a
                    // zero narrows the interval exactly as a one does, and it
                    // is the interval E.1.1.2 reconstructs the midpoint of.
                    let at = (4 * stripe + r) * block.w + x;
                    // At most 30, below `MAX_BIT_PLANES`, so the cast keeps
                    // every bit.
                    block.known[at] = plane as u8;
                    if bit == 1 {
                        block.magnitude[at] |= 1 << plane;
                    }
                }
            }
        }
    }

    /// D.2.4: everything the other two passes did not code, with the
    /// run-length mode for a column of four that is entirely quiet.
    fn cleanup_pass(&mut self, block: &mut Packed, plane: u32) -> Result<(), Refusal> {
        let width = block.w;
        for stripe in 0..block.stripes {
            let (first, rows) = (block.index(0, stripe), block.rows(stripe));
            for x in 0..width {
                let i = first + x;
                let at = |r: usize| (4 * stripe + r) * width + x;
                let mut from = 0;
                // The run-length mode. Four rows, none significant, none coded
                // by this plane's significance pass, and every one of the four
                // with an entirely insignificant neighbourhood — which is one
                // decision instead of four, and is why a flat region costs
                // almost nothing. The four neighbourhoods and the four
                // coefficients together are the whole window.
                if rows == 4 && block.words[i] & (WINDOW | VISITED) == 0 {
                    if self.decode(RUN_LENGTH) == 0 {
                        // All four stay insignificant, and not one of them
                        // cost a decision of its own.
                        continue;
                    }
                    // Two UNIFORM bits, most significant first, locating the
                    // first significant coefficient of the four.
                    let r =
                        usize::from(self.decode(UNIFORM)) * 2 + usize::from(self.decode(UNIFORM));
                    let negative = self.sign(block, i, r);
                    block.make_significant(i, at(r), r, negative, plane);
                    from = r + 1;
                }
                for r in from..rows {
                    if block.words[i] & (sigma(1, r + 1) | pi(r)) != 0 {
                        continue;
                    }
                    let context = block.zero_coding_at(i, r);
                    if self.decode(context) == 1 {
                        let negative = self.sign(block, i, r);
                        block.make_significant(i, at(r), r, negative, plane);
                    }
                }
            }
        }
        // π is per bit-plane, and this is where the plane ends.
        block.clear_visited();

        if self.segmentation_symbols {
            // D.5, and the only free integrity check in the format. Four
            // UNIFORM decisions that must be 1010: if the arithmetic decoder
            // has drifted by so much as one decision anywhere in this
            // code-block, the chance of landing back on all four is 1 in 16,
            // and the alternative to checking is a code-block full of
            // coefficients that are wrong and look right.
            let mut symbol = 0u8;
            for _ in 0..4 {
                symbol = (symbol << 1) | self.decode(UNIFORM);
            }
            if symbol != SEGMENTATION_SYMBOL {
                return Err(Refusal::SegmentationSymbol);
            }
        }
        Ok(())
    }
}

/// Decodes one code-block's coding passes into signed coefficients.
///
/// `passes` and `planes` both come from the packet header and are therefore
/// attacker numbers; both are bounded before a single decision is decoded,
/// and the work they buy — all of it, `area * passes` — is charged against
/// the caller's remaining budget before the first pass runs rather than one
/// pass at a time as it goes.
// Eight, because a code-block's decode genuinely depends on eight things and
// bundling them into a struct would only move the list. T.800 D.4's scan needs
// the block, its geometry, its coding style, its bit-plane count, its passes,
// the contexts, the decoder and the budget.
#[allow(clippy::too_many_arguments, reason = "T.800 D.4's own parameter list")]
pub(crate) fn decode_code_block(
    segments: &[Segment],
    width: u32,
    height: u32,
    passes: u32,
    orientation: Orientation,
    style: CodingStyle,
    contexts: &mut MqContexts,
    work: &mut u64,
) -> Result<Decoded, Refusal> {
    decode_code_block_observed(
        segments,
        width,
        height,
        passes,
        orientation,
        style,
        contexts,
        work,
        |_, _| {},
    )
}

/// [`decode_code_block`], handing `after_pass` the block's state after each
/// coding pass it runs, with the pass's index: how the tests watch the
/// passes, and a closure that does nothing on every other path.
// [`decode_code_block`]'s eight, and the observer.
#[allow(clippy::too_many_arguments, reason = "T.800 D.4's own parameter list")]
pub(crate) fn decode_code_block_observed(
    segments: &[Segment],
    width: u32,
    height: u32,
    passes: u32,
    orientation: Orientation,
    style: CodingStyle,
    contexts: &mut MqContexts,
    work: &mut u64,
    mut after_pass: impl FnMut(u32, &Packed),
) -> Result<Decoded, Refusal> {
    if width == 0 || height == 0 || passes == 0 {
        return Ok(Decoded::default());
    }
    // The pass count and the whole block's cost, before its first decision
    // ([`account`], below, says why it is charged whole).
    account(width, height, passes, work)?;
    // The first pass is a cleanup on the most significant plane and every
    // plane below it carries three, so `passes = 1 + 3(planes - 1)` and this
    // is that inverted — rounding **up**, because a truncated stream stops
    // part way through a plane and the plane it stopped inside was still
    // decoded.
    //
    // `passes.div_ceil(3)` is the plausible-looking wrong answer and it was
    // here until milestone 6. It agrees for `passes = 1, 4, 7, ...` — a
    // code-block that ends on a cleanup pass, which is every code-block the
    // in-tree test encoder writes, because it encodes `3 * planes - 2` passes
    // by construction. For every other count it is one plane short, and
    // `pass_at` then subtracts past zero: a panic in a debug build and, in a
    // release one, a shift masked to `plane & 31` writing a coefficient bit
    // in the wrong place. A picture, in other words. It took a lossy
    // three-component stream from `opj_compress` to produce a code-block that
    // stops on a significance or refinement pass.
    let planes = (passes - 1).div_ceil(3) + 1;

    // D.2: the contexts are reset at the start of every code-block. `reset`
    // returns to Table D.7's states rather than to T.88's zero, which is the
    // half of `set_state` that had to exist for this line to be right.
    contexts.reset();
    let mut block = Packed::new(
        width as usize,
        height as usize,
        orientation,
        style.vertically_causal,
    );

    // One reader per codeword segment (B.10.7). With neither Table A.19 bit
    // set there is exactly one, holding every byte, and this loop is the loop
    // that was here before.
    //
    // **What is re-initialised at a segment boundary, and what is not.** A
    // new [`MqDecoder`] is INITDEC on the new segment's bytes (T.88 E.3.5),
    // which is what "terminate" means on the encoder's side: D.4.2 requires a
    // termination to leave the decoder needing no backtracking. The *context
    // states* are untouched, because D.4's termination says nothing about
    // them and Table D.7's reset is a different bit — `RESET`, handled below.
    // Resetting contexts at every termination decodes a `TERMALL` stream's
    // second pass as noise; not re-initialising the decoder decodes it as
    // noise the other way.
    let mut i = 0u32;
    for segment in segments {
        // The kind is taken from the segment's first pass. The passes in one
        // segment are all of a kind — see [`Source`] — so any of them would
        // answer, and the first is the one that exists whatever the count.
        let mut source = if Schedule::new(style.bypass, style.terminate_all).raw(i) {
            Source::Raw(RawBits::new(&segment.bytes))
        } else {
            Source::Mq(MqDecoder::new(&segment.bytes))
        };
        let segmentation_symbols = style.segmentation_symbols;
        for _ in 0..segment.passes {
            if i >= passes {
                break;
            }
            let (pass, plane) = pass_at(i, planes);
            match &mut source {
                // The decoder's registers held in locals for the pass, and
                // written back when it ends.
                Source::Mq(mq) => Coder {
                    source: &mut mq.run(),
                    contexts: &mut *contexts,
                    segmentation_symbols,
                }
                .pass(pass, &mut block, plane)?,
                Source::Raw(raw) => Coder {
                    source: raw,
                    contexts: &mut *contexts,
                    segmentation_symbols,
                }
                .pass(pass, &mut block, plane)?,
            }
            if style.reset_contexts {
                // Table A.19 bit 1: back to Table D.7's states at every pass
                // boundary. The arithmetic decoder's own registers are *not*
                // reset — that is `TERMALL`, a different bit and a different
                // question, and conflating the two decodes the second pass of
                // every block as noise.
                contexts.reset();
            }
            after_pass(i, &block);
            i += 1;
        }
    }
    Ok(block.into_decoded(planes))
}

/// The Table A.19 code-block style bits that change how decisions are read.
///
/// A struct rather than three `bool` parameters: they arrive together, they are
/// all `bool`, and two of them transposed at a call site decodes plausibly
/// wrong rather than visibly so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodingStyle {
    /// D.5's four-decision check at the end of each cleanup pass.
    pub(crate) segmentation_symbols: bool,
    /// Table A.19 bit 1.
    pub(crate) reset_contexts: bool,
    /// Table A.19 bit 3.
    pub(crate) vertically_causal: bool,
    /// Table A.19 bit 0, D.6. Read here only to decide whether a segment's
    /// passes are raw; the *segmentation* it causes is tier-2's, and arrives
    /// as the shape of [`Segment`].
    pub(crate) bypass: bool,
    /// Table A.19 bit 2, D.4. Read here for the same reason and with the same
    /// division of labour — and it is here rather than absent because a
    /// segment's kind is a function of both bits together, not of `bypass`
    /// alone.
    pub(crate) terminate_all: bool,
}

/// What one code-block decode yields.
///
/// `half_planes` rides beside the coefficients rather than being derived from
/// `planes` because it **cannot** be: on a truncated stream two coefficients
/// of the same code-block are known to different depths, and E.1.1.2's
/// reconstruction point is a property of each one's own interval.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Decoded {
    pub(crate) coefficients: Vec<i32>,
    pub(crate) half_planes: Vec<u8>,
    /// How many magnitude bit-planes the passes covered.
    pub(crate) planes: u32,
}

/// What a code-block of `width` by `height` coefficients and `passes` coding
/// passes is refused for, and charged, before its first decision: nothing
/// for a block of no area or no passes; more passes than any legal bit-plane
/// count allows; and its whole cost, `area * passes`, from the budget.
///
/// T.800's own arithmetic bounds the passes, not an invented cap: a
/// code-block carries at most 31 magnitude bit-planes that fit an `i32`
/// beside a sign, and the first plane carries one pass where the rest carry
/// three. And every one of the passes visits every one of the coefficients,
/// so the charge is exactly what the block is about to spend and not an
/// estimate of it. Charging it whole rather than inside the pass loop is
/// what makes the budget a check on what the codestream *declared*: the pass
/// count came out of the packet header, so it is known before any work is
/// done, and a per-pass charge would refuse the ninety-first pass having
/// already run ninety — which is to say, having already spent the time the
/// budget exists to prevent. That is `5adf502`'s method, whose whole point
/// was that the refusal be assertable by the warning rather than by a clock.
/// `the_work_charge_is_the_whole_block_before_any_of_it` pins the boundary at
/// `area * passes` exactly, which is the assertion the two designs disagree
/// on.
///
/// Both are read off the packet headers alone, so a decode charges every
/// block of a kept resolution this way whether or not it decodes it
/// ([`decode_tiles_counted`]): a window then refuses for its budget and its
/// pass counts exactly where a decode of every block does.
fn account(width: u32, height: u32, passes: u32, work: &mut u64) -> Result<(), Refusal> {
    if width == 0 || height == 0 || passes == 0 {
        return Ok(());
    }
    if passes > MAX_PASSES {
        return Err(Refusal::Structure(
            "more coding passes than any legal bit-plane count allows",
        ));
    }
    let area = u64::from(width) * u64::from(height);
    charge(work, area.saturating_mul(u64::from(passes)))
}

/// Spends `n` from [`MAX_JPX_WORK`]. Spent and never refunded — see [`super`].
fn charge(work: &mut u64, n: u64) -> Result<(), Refusal> {
    *work = work
        .checked_sub(n)
        .ok_or(Refusal::Budget("tier-1 coefficient passes"))?;
    Ok(())
}

/// Runs tier-1 over every code-block of every tile that a decode dropping
/// the top `discard` resolutions keeps and, where a window says what it
/// reads, that the window reaches, and returns how many code-blocks it
/// decoded.
///
/// `needs` is a window's, one entry a tile: `None` for a tile it misses,
/// whose blocks are all skipped, and each component's [`Needs`] for the rest,
/// whose blocks are decoded where they hold a coefficient the window's
/// synthesis reads. `None` for `needs` is no window: every block of a kept
/// resolution. `discard` is already clamped to the fewest levels any
/// tile-component has, so every tile-component keeps at least its
/// resolution 0.
///
/// **A window refuses where a decode of every block does.** A block skipped
/// is not decoded, but it is accounted for as it would be decoded
/// ([`account`]): its pass count checked and its whole cost charged, in its
/// place in index order, so the budget and the pass counts refuse the same
/// block whatever the window. And a tile-component that signals
/// segmentation symbols (Table A.19) decodes every block whatever the
/// window: D.5's symbol is checked only by decoding, and a window that left
/// a drifted block out would draw what the whole image's decode refuses.
///
/// The budget is one total across the whole codestream, because the shape
/// this guards is a *branching* one: tiles times components times resolutions
/// times precincts times code-blocks times layers times bit-planes times
/// three passes, each factor individually bounded by the standard and their
/// product not. `5adf502` landed that lesson one layer up — an 1 851-byte
/// page that took 19.3 seconds inside a depth cap that never fired — and a
/// per-code-block cap here would be exactly that mistake again.
///
/// What the work budget was charged is added to `steps`
/// (`docs/design/resource-exhaustion.md`, "The counters"): each coefficient
/// of each code-block once a coding pass, the whole of every block the decode
/// reached, charged before its first decision.
pub(crate) fn decode_tiles_counted(
    stream: &Codestream<'_>,
    tiles: &mut [Tile],
    discard: u8,
    needs: Option<&[Option<Vec<Needs>>]>,
    executor: &dyn Executor,
    steps: &mut u64,
) -> Result<u64, Refusal> {
    let mut work = MAX_JPX_WORK;
    let decoded = decode_within(stream, tiles, discard, needs, executor, &mut work);
    *steps = steps.saturating_add(MAX_JPX_WORK.saturating_sub(work));
    decoded
}

/// [`decode_tiles_counted`] with its count dropped, serial, the door the
/// module's tests read tier-1 through.
#[cfg(test)]
pub(crate) fn decode_tiles(
    stream: &Codestream<'_>,
    tiles: &mut [Tile],
    discard: u8,
    needs: Option<&[Option<Vec<Needs>>]>,
) -> Result<u64, Refusal> {
    decode_tiles_counted(stream, tiles, discard, needs, &Parallelism::Serial, &mut 0)
}

/// [`decode_tiles_counted`] on `executor` from a budget of `budget` rather
/// than [`MAX_JPX_WORK`], with what is left of it: the door
/// `tests::parallel` reaches a budget spent part way through.
#[cfg(test)]
pub(crate) fn decode_tiles_within(
    stream: &Codestream<'_>,
    tiles: &mut [Tile],
    discard: u8,
    executor: &dyn Executor,
    budget: u64,
) -> (Result<u64, Refusal>, u64) {
    let mut work = budget;
    let decoded = decode_within(stream, tiles, discard, None, executor, &mut work);
    (decoded, work)
}

/// One code-block a decode lists for tier-1, by where it is: its tile,
/// component, resolution, band, precinct and its own place in the precinct,
/// with what decoding it needs to know of its band and its tile-component,
/// and the budget left once it was charged.
struct Listed {
    at: [usize; 6],
    orientation: Orientation,
    style: CodingStyle,
    left: u64,
}

/// Tier-1 over the blocks a decode keeps, in three steps
/// (`docs/design/jpx-speed.md`, "Code-blocks and synthesis on P-15's pool").
///
/// 1. **In index order** — tile, component, resolution, band, precinct,
///    block — every block of a kept resolution is accounted for
///    ([`account`]): its pass count checked and its whole cost charged, the
///    blocks the decode reads listed and the rest left there. A refusal
///    stops the walk where it is.
/// 2. **The listed blocks decoded on `executor`**, each into a slot of its
///    own with MQ contexts of its own, from no budget, since each was charged
///    in the walk. A block a discard or a window drops is never listed, so it
///    never reaches a worker.
/// 3. **The slots read back in index order**: the first block whose decode
///    refused gives the refusal, as a decode one block after another would
///    have stopped there — the budget then as it stood once that block was
///    charged — and a refusal the walk stopped at, after every listed block,
///    gives its own. So a refusal names the same block, and the budget ends
///    where it would, at every thread count.
fn decode_within(
    stream: &Codestream<'_>,
    tiles: &mut [Tile],
    discard: u8,
    needs: Option<&[Option<Vec<Needs>>]>,
    executor: &dyn Executor,
    work: &mut u64,
) -> Result<u64, Refusal> {
    let mut listed: Vec<Listed> = Vec::new();
    let mut stopped = None;
    'walk: for (t, tile) in tiles.iter().enumerate() {
        let index = tile.index;
        // `None` reads every block; `Some(None)` a tile the window misses,
        // none of its blocks; `Some(Some(_))` the blocks its needs reach.
        let wanted: Option<Option<&Vec<Needs>>> =
            needs.map(|needs| needs.get(t).and_then(Option::as_ref));
        for (c, component) in tile.components.iter().enumerate() {
            let need: Option<Option<&Needs>> =
                wanted.map(|wanted| wanted.and_then(|wanted| wanted.get(c)));
            let coding = stream.style_for(index, c);
            let style = CodingStyle {
                segmentation_symbols: coding.segmentation_symbols(),
                reset_contexts: coding.reset_contexts(),
                vertically_causal: coding.vertically_causal(),
                bypass: coding.bypass(),
                terminate_all: coding.terminate_all(),
            };
            let kept = component
                .resolutions
                .len()
                .saturating_sub(usize::from(discard));
            for (r, resolution) in component.resolutions.iter().enumerate().take(kept) {
                for (b, band) in resolution.bands.iter().enumerate() {
                    let orientation = band.orientation;
                    for (p, precinct) in band.precincts.iter().enumerate() {
                        for (k, block) in precinct.blocks.iter().enumerate() {
                            let read = style.segmentation_symbols
                                || match need {
                                    None => true,
                                    Some(need) => need.is_some_and(|need| need.wants(r, b, block)),
                                };
                            if let Err(refusal) =
                                account(block.width(), block.height(), block.passes, work)
                            {
                                stopped = Some(refusal);
                                break 'walk;
                            }
                            if read {
                                listed.push(Listed {
                                    at: [t, c, r, b, p, k],
                                    orientation,
                                    style,
                                    left: *work,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let shared: &[Tile] = tiles;
    let slots = tinker_pdf_pool::map(executor, listed.len(), |i| {
        let Some(entry) = listed.get(i) else {
            return Ok(Decoded::default());
        };
        let Some(block) = block_at(shared, entry.at) else {
            return Ok(Decoded::default());
        };
        // D.2 resets the contexts at every block, so a block's own are the
        // state one array reset between blocks would hold; and the walk has
        // charged it, so it decodes from a budget of its own.
        let mut contexts = initial_contexts();
        let mut charged = u64::MAX;
        decode_code_block(
            &block.segments,
            block.width(),
            block.height(),
            block.passes,
            entry.orientation,
            entry.style,
            &mut contexts,
            &mut charged,
        )
    });

    let mut decoded = 0u64;
    for (entry, slot) in listed.iter().zip(slots) {
        match slot {
            Ok(result) => {
                if let Some(block) = block_at_mut(tiles, entry.at) {
                    block.coefficients = result.coefficients;
                    block.half_planes = result.half_planes;
                    block.planes = u8::try_from(result.planes).unwrap_or(u8::MAX);
                }
                decoded += 1;
            }
            Err(refusal) => {
                *work = entry.left;
                return Err(refusal);
            }
        }
    }
    match stopped {
        Some(refusal) => Err(refusal),
        None => Ok(decoded),
    }
}

/// The block [`Listed::at`] names, read.
fn block_at(tiles: &[Tile], at: [usize; 6]) -> Option<&CodeBlock> {
    let [t, c, r, b, p, k] = at;
    tiles
        .get(t)?
        .components
        .get(c)?
        .resolutions
        .get(r)?
        .bands
        .get(b)?
        .precincts
        .get(p)?
        .blocks
        .get(k)
}

/// The block [`Listed::at`] names, to write.
fn block_at_mut(tiles: &mut [Tile], at: [usize; 6]) -> Option<&mut CodeBlock> {
    let [t, c, r, b, p, k] = at;
    tiles
        .get_mut(t)?
        .components
        .get_mut(c)?
        .resolutions
        .get_mut(r)?
        .bands
        .get_mut(b)?
        .precincts
        .get_mut(p)?
        .blocks
        .get_mut(k)
}

/// The tier-1 *encoder*, for tests only (T.800 D.2, from the other side).
///
/// Nothing ships this: the engine reads PDFs, it does not write JPEG 2000. It
/// exists so a code-block whose coefficients are known by construction can be
/// put in and demanded back, which is the only way to exercise the three
/// passes over a code-block the standard does not happen to publish.
///
/// **And its limit is stated rather than discovered.** It shares this
/// module's context formation deliberately, so a round trip through it cannot
/// see a relabelling of the Annex D tables — every adaptive state starts
/// identical bar three, and an encoder's slot histories and a decoder's stay
/// in step under any permutation. That is gap 17's finding, and it is why the
/// tables are asserted against the standard's own numbers instead.
#[cfg(test)]
pub(crate) mod encoder {
    use super::{
        magnitude_refinement_context, zero_coding_context, Orientation, Pass, RUN_LENGTH,
        SEGMENTATION_SYMBOL, UNIFORM,
    };
    use crate::mq::encoder::MqEncoder;

    /// Table D.7's initial states, which the coder starts from and which
    /// `RESET` returns to at every coding pass boundary.
    fn initial_states(mq: &mut MqEncoder) {
        for index in 0..super::CONTEXTS {
            mq.set_state(index, 0, 0);
        }
        for (index, state_) in [(super::ZERO_CODING, 4u8), (RUN_LENGTH, 3), (UNIFORM, 46)] {
            mq.set_state(index, state_, 0);
        }
    }

    /// Encodes `values` — signed coefficients whose magnitudes fit `planes`
    /// bits — as `3 * planes - 2` coding passes.
    pub(crate) fn encode_code_block(
        values: &[i32],
        w: usize,
        h: usize,
        planes: u32,
        orientation: Orientation,
        style: super::CodingStyle,
    ) -> (Vec<u8>, u32) {
        encode_code_block_observed(values, w, h, planes, orientation, style, |_, _| {})
    }

    /// [`encode_code_block`], handing `after_pass` the transcribed state
    /// after each coding pass it codes, with the pass's index: the state the
    /// decoder's must be in after the same pass.
    pub(crate) fn encode_code_block_observed(
        values: &[i32],
        w: usize,
        h: usize,
        planes: u32,
        orientation: Orientation,
        style: super::CodingStyle,
        mut after_pass: impl FnMut(u32, &mut super::Block),
    ) -> (Vec<u8>, u32) {
        let segmentation_symbols = style.segmentation_symbols;
        let mut state = super::Block::new(w, h, orientation);
        // The encoder forms its contexts from the same neighbourhood the
        // decoder will, so a mode that changes context formation has to be
        // mirrored here or the round trip proves only that both are wrong in
        // the same way.
        state.set_causal(style.vertically_causal);
        let mut mq = MqEncoder::new(super::CONTEXTS);
        initial_states(&mut mq);

        let magnitude: Vec<u32> = values.iter().map(|v| v.unsigned_abs()).collect();
        let negative: Vec<bool> = values.iter().map(|v| *v < 0).collect();
        let passes = 3 * planes - 2;

        for i in 0..passes {
            let (pass, plane) = super::pass_at(i, planes);
            match pass {
                Pass::Significance => {
                    for (top, rows, x) in state.stripes().collect::<Vec<_>>() {
                        state.enter_stripe(top);
                        for y in top..top + rows {
                            let at = state.at(x, y);
                            if state.sigma[at] {
                                continue;
                            }
                            let (nh, nv, nd) = state.neighbours(x, y);
                            if nh + nv + nd == 0 {
                                continue;
                            }
                            state.visited[at] = true;
                            let context = zero_coding_context(orientation, nh, nv, nd);
                            let bit = (magnitude[at] >> plane) & 1;
                            mq.encode_at(context, bit as u8);
                            if bit == 1 {
                                let (sc, xorbit) = state.sign_coding(x, y);
                                mq.encode_at(sc, u8::from(negative[at]) ^ xorbit);
                                state.sigma[at] = true;
                                state.negative[at] = negative[at];
                            }
                        }
                    }
                }
                Pass::Refinement => {
                    for (top, rows, x) in state.stripes().collect::<Vec<_>>() {
                        state.enter_stripe(top);
                        for y in top..top + rows {
                            let at = state.at(x, y);
                            if !state.sigma[at] || state.visited[at] {
                                continue;
                            }
                            let (nh, nv, nd) = state.neighbours(x, y);
                            let context =
                                magnitude_refinement_context(nh + nv + nd, !state.refined[at]);
                            mq.encode_at(context, ((magnitude[at] >> plane) & 1) as u8);
                            state.refined[at] = true;
                        }
                    }
                }
                Pass::Cleanup => {
                    for (top, rows, x) in state.stripes().collect::<Vec<_>>() {
                        state.enter_stripe(top);
                        let mut y = top;
                        if rows == 4
                            && (top..top + 4).all(|y| {
                                let at = state.at(x, y);
                                let (nh, nv, nd) = state.neighbours(x, y);
                                !state.sigma[at] && !state.visited[at] && nh + nv + nd == 0
                            })
                        {
                            let first = (top..top + 4)
                                .find(|&y| (magnitude[state.at(x, y)] >> plane) & 1 == 1);
                            match first {
                                None => {
                                    mq.encode_at(RUN_LENGTH, 0);
                                    continue;
                                }
                                Some(fy) => {
                                    mq.encode_at(RUN_LENGTH, 1);
                                    let k = fy - top;
                                    mq.encode_at(UNIFORM, ((k >> 1) & 1) as u8);
                                    mq.encode_at(UNIFORM, (k & 1) as u8);
                                    let at = state.at(x, fy);
                                    let (sc, xorbit) = state.sign_coding(x, fy);
                                    mq.encode_at(sc, u8::from(negative[at]) ^ xorbit);
                                    state.sigma[at] = true;
                                    state.negative[at] = negative[at];
                                    y = fy + 1;
                                }
                            }
                        }
                        for y in y..top + rows {
                            let at = state.at(x, y);
                            if state.sigma[at] || state.visited[at] {
                                continue;
                            }
                            let context = state.zero_coding(x, y);
                            let bit = (magnitude[at] >> plane) & 1;
                            mq.encode_at(context, bit as u8);
                            if bit == 1 {
                                let (sc, xorbit) = state.sign_coding(x, y);
                                mq.encode_at(sc, u8::from(negative[at]) ^ xorbit);
                                state.sigma[at] = true;
                                state.negative[at] = negative[at];
                            }
                        }
                    }
                    state.visited.iter_mut().for_each(|v| *v = false);
                    if segmentation_symbols {
                        for shift in (0..4).rev() {
                            mq.encode_at(UNIFORM, (SEGMENTATION_SYMBOL >> shift) & 1);
                        }
                    }
                }
            }
            if style.reset_contexts {
                initial_states(&mut mq);
            }
            after_pass(i, &mut state);
        }
        (mq.flush(), passes)
    }
}
