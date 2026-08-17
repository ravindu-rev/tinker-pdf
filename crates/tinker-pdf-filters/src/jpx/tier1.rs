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
use super::tier2::{CodeBlock, Orientation, Tile};
use super::{Refusal, MAX_JPX_WORK};
use crate::mq::{MqContexts, MqDecoder};

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
pub(crate) fn zero_coding_context(orientation: Orientation, h: u32, v: u32, d: u32) -> usize {
    // Table D.1's HL column is the LL/LH column with ΣH and ΣV interchanged.
    // Writing it as a swap rather than as a second nine-row table is the one
    // place this transcription is not literal, and it is exactly how the
    // standard words it: "Table D.1 ... with the horizontal and vertical
    // neighbour counts interchanged".
    let (h, v) = match orientation {
        Orientation::Hl => (v, h),
        Orientation::Ll | Orientation::Lh | Orientation::Hh => (h, v),
    };
    if orientation == Orientation::Hh {
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

/// One code-block's state while the passes run over it.
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
    /// The **lowest bit-plane whose magnitude bit was actually decoded** for
    /// this coefficient, which is not the same as the lowest plane the
    /// code-block reached.
    ///
    /// It exists because E.1.1.2's reconstruction point is defined on the
    /// interval a decoder *knows* a coefficient lies in, and on a truncated
    /// stream that interval differs per coefficient. One that became
    /// significant in a cleanup pass and never met a refinement pass is known
    /// only to `[2^p, 2^(p+1))`, and reconstructing it at the midpoint of the
    /// bottom plane instead puts it a quarter of its own magnitude low.
    known: Vec<u32>,
    /// π: coded in the significance propagation pass of the current
    /// bit-plane, and therefore skipped by this plane's cleanup pass. Cleared
    /// at the end of every cleanup pass.
    visited: Vec<bool>,
    magnitude: Vec<u32>,
}

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
            known: vec![0; n],
            visited: vec![false; n],
            magnitude: vec![0; n],
        }
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

    /// Marks `(x, y)` significant with the given sign, setting the bit of the
    /// plane being coded.
    fn make_significant(&mut self, x: usize, y: usize, negative: bool, plane: u32) {
        let i = self.at(x, y);
        self.sigma[i] = true;
        self.negative[i] = negative;
        self.magnitude[i] |= 1 << plane;
        self.known[i] = plane;
    }

    /// The signed coefficients, with the magnitude as `planes` bits.
    pub(crate) fn coefficients(&self) -> Vec<i32> {
        self.magnitude
            .iter()
            .zip(&self.negative)
            .map(|(&m, &neg)| {
                // The magnitude is at most `MAX_BIT_PLANES` bits, so it fits
                // an `i32` beside its sign; the cast is not a truncation and
                // `MAX_BIT_PLANES` is what makes that true.
                let m = m as i32;
                if neg {
                    -m
                } else {
                    m
                }
            })
            .collect()
    }

    /// Per coefficient, the plane whose *half* E.1.1.2 reconstructs at.
    ///
    /// Zero for anything insignificant, which the dequantiser never reads
    /// because a coefficient that never became significant stays at zero
    /// rather than at half a step.
    pub(crate) fn half_planes(&self) -> Vec<u8> {
        self.known
            .iter()
            .map(|&p| u8::try_from(p).unwrap_or(u8::MAX))
            .collect()
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

/// Everything one code-block decode needs that is not the block itself.
struct Coder<'a, 'b> {
    mq: MqDecoder<'a>,
    contexts: &'b mut MqContexts,
    segmentation_symbols: bool,
}

impl Coder<'_, '_> {
    fn decode(&mut self, context: usize) -> u8 {
        self.mq.decode_at(self.contexts, context)
    }

    /// D.2.2: the sign of a coefficient that has just become significant.
    fn sign(&mut self, block: &Block, x: usize, y: usize) -> bool {
        let (context, xorbit) = block.sign_coding(x, y);
        (self.decode(context) ^ xorbit) == 1
    }

    /// D.2.1: every insignificant coefficient with at least one significant
    /// neighbour.
    fn significance_pass(&mut self, block: &mut Block, plane: u32) {
        for (top, rows, x) in block.stripes().collect::<Vec<_>>() {
            for y in top..top + rows {
                let i = block.at(x, y);
                if block.sigma[i] {
                    continue;
                }
                let (h, v, d) = block.neighbours(x, y);
                // The "preferred neighbourhood": at least one of the eight.
                // Equivalently a non-zero Table D.1 context, since all three
                // of its mappings send the all-insignificant neighbourhood
                // and only that one to context 0 — but the condition is D.2.1's
                // and is written as D.2.1 words it rather than inferred from
                // the table, so a change to one cannot silently move the other.
                if h + v + d == 0 {
                    continue;
                }
                block.visited[i] = true;
                let context = zero_coding_context(block.orientation, h, v, d);
                if self.decode(context) == 1 {
                    let negative = self.sign(block, x, y);
                    block.make_significant(x, y, negative, plane);
                }
            }
        }
    }

    /// D.2.3: every coefficient that was already significant before this
    /// bit-plane's significance propagation pass.
    fn refinement_pass(&mut self, block: &mut Block, plane: u32) {
        for (top, rows, x) in block.stripes().collect::<Vec<_>>() {
            for y in top..top + rows {
                let i = block.at(x, y);
                // π excludes the coefficients this plane's significance pass
                // just took, which is the whole reason π exists: a
                // coefficient that became significant a moment ago has no
                // magnitude bit left in this plane to refine.
                if !block.sigma[i] || block.visited[i] {
                    continue;
                }
                let context = block.refinement(x, y);
                let bit = self.decode(context);
                block.refined[i] = true;
                // The plane is now known whichever way the bit went: a zero
                // narrows the interval exactly as a one does, and it is the
                // interval E.1.1.2 reconstructs the midpoint of.
                block.known[i] = plane;
                if bit == 1 {
                    block.magnitude[i] |= 1 << plane;
                }
            }
        }
    }

    /// D.2.4: everything the other two passes did not code, with the
    /// run-length mode for a column of four that is entirely quiet.
    fn cleanup_pass(&mut self, block: &mut Block, plane: u32) -> Result<(), Refusal> {
        for (top, rows, x) in block.stripes().collect::<Vec<_>>() {
            let mut y = top;
            // The run-length mode. Four rows, none significant, none coded by
            // this plane's significance pass, and every one of the four with
            // an entirely insignificant neighbourhood — which is one decision
            // instead of four, and is why a flat region costs almost nothing.
            if rows == 4
                && (top..top + 4).all(|y| {
                    let i = block.at(x, y);
                    let (h, v, d) = block.neighbours(x, y);
                    !block.sigma[i] && !block.visited[i] && h + v + d == 0
                })
            {
                if self.decode(RUN_LENGTH) == 0 {
                    // All four stay insignificant, and not one of them cost a
                    // decision of its own.
                    continue;
                }
                // Two UNIFORM bits, most significant first, locating the
                // first significant coefficient of the four.
                let first =
                    usize::from(self.decode(UNIFORM)) * 2 + usize::from(self.decode(UNIFORM));
                y = top + first;
                let negative = self.sign(block, x, y);
                block.make_significant(x, y, negative, plane);
                y += 1;
            }
            for y in y..top + rows {
                let i = block.at(x, y);
                if block.sigma[i] || block.visited[i] {
                    continue;
                }
                let context = block.zero_coding(x, y);
                if self.decode(context) == 1 {
                    let negative = self.sign(block, x, y);
                    block.make_significant(x, y, negative, plane);
                }
            }
        }
        // π is per bit-plane, and this is where the plane ends.
        block.visited.iter_mut().for_each(|v| *v = false);

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
    data: &[u8],
    width: u32,
    height: u32,
    passes: u32,
    orientation: Orientation,
    segmentation_symbols: bool,
    contexts: &mut MqContexts,
    work: &mut u64,
) -> Result<Decoded, Refusal> {
    if width == 0 || height == 0 || passes == 0 {
        return Ok(Decoded::default());
    }
    if passes > MAX_PASSES {
        // T.800's own arithmetic, not an invented cap: a code-block carries
        // at most 31 magnitude bit-planes that fit an `i32` beside a sign,
        // and the first plane carries one pass where the rest carry three.
        return Err(Refusal::Structure(
            "more coding passes than any legal bit-plane count allows",
        ));
    }
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
    let area = u64::from(width) * u64::from(height);

    // The whole block's cost, charged before its first decision is decoded.
    //
    // Every one of the `passes` passes below visits every one of the `area`
    // coefficients, so this is exactly what the block is about to spend and
    // not an estimate of it. Charging it here rather than inside the loop is
    // what makes the budget a check on what the codestream *declared*: the
    // pass count came out of the packet header, so it is known before any
    // work is done, and a per-pass charge would refuse the ninety-first pass
    // having already run ninety — which is to say, having already spent the
    // time the budget exists to prevent. That is `5adf502`'s method, whose
    // whole point was that the refusal be assertable by the warning rather
    // than by a clock. `the_work_charge_is_the_whole_block_before_any_of_it`
    // pins the boundary at `area * passes` exactly, which is the assertion
    // the two designs disagree on.
    charge(work, area.saturating_mul(u64::from(passes)))?;

    // D.2: the contexts are reset at the start of every code-block. `reset`
    // returns to Table D.7's states rather than to T.88's zero, which is the
    // half of `set_state` that had to exist for this line to be right.
    contexts.reset();
    let mut block = Block::new(width as usize, height as usize, orientation);
    let mut coder = Coder {
        mq: MqDecoder::new(data),
        contexts,
        segmentation_symbols,
    };

    for i in 0..passes {
        let (pass, plane) = pass_at(i, planes);
        match pass {
            Pass::Significance => coder.significance_pass(&mut block, plane),
            Pass::Refinement => coder.refinement_pass(&mut block, plane),
            Pass::Cleanup => coder.cleanup_pass(&mut block, plane)?,
        }
    }
    Ok(Decoded {
        coefficients: block.coefficients(),
        half_planes: block.half_planes(),
        planes,
    })
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

/// Spends `n` from [`MAX_JPX_WORK`]. Spent and never refunded — see [`super`].
fn charge(work: &mut u64, n: u64) -> Result<(), Refusal> {
    *work = work
        .checked_sub(n)
        .ok_or(Refusal::Budget("tier-1 coefficient passes"))?;
    Ok(())
}

/// Runs tier-1 over every code-block of every tile.
///
/// The budget is one total across the whole codestream, because the shape
/// this guards is a *branching* one: tiles times components times resolutions
/// times precincts times code-blocks times layers times bit-planes times
/// three passes, each factor individually bounded by the standard and their
/// product not. `5adf502` landed that lesson one layer up — an 1 851-byte
/// page that took 19.3 seconds inside a depth cap that never fired — and a
/// per-code-block cap here would be exactly that mistake again.
pub(crate) fn decode_tiles(stream: &Codestream<'_>, tiles: &mut [Tile]) -> Result<(), Refusal> {
    let mut work = MAX_JPX_WORK;
    // One context array for the whole codestream, reset per code-block. That
    // is what `MqContexts::reset` is for, and the 64 KB it avoids
    // reallocating for JBIG2 is nineteen entries here — what matters is that
    // reset returns to Table D.7 rather than to zero.
    let mut contexts = initial_contexts();

    for tile in tiles.iter_mut() {
        let index = tile.index;
        for (c, component) in tile.components.iter_mut().enumerate() {
            let style = stream.style_for(index, c);
            let segmentation_symbols = style.segmentation_symbols();
            for resolution in &mut component.resolutions {
                for band in &mut resolution.bands {
                    let orientation = band.orientation;
                    for precinct in &mut band.precincts {
                        for block in &mut precinct.blocks {
                            decode_one(
                                block,
                                orientation,
                                segmentation_symbols,
                                &mut contexts,
                                &mut work,
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn decode_one(
    block: &mut CodeBlock,
    orientation: Orientation,
    segmentation_symbols: bool,
    contexts: &mut MqContexts,
    work: &mut u64,
) -> Result<(), Refusal> {
    let decoded = decode_code_block(
        &block.data,
        block.width(),
        block.height(),
        block.passes,
        orientation,
        segmentation_symbols,
        contexts,
        work,
    )?;
    block.coefficients = decoded.coefficients;
    block.half_planes = decoded.half_planes;
    block.planes = u8::try_from(decoded.planes).unwrap_or(u8::MAX);
    Ok(())
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

    /// Encodes `values` — signed coefficients whose magnitudes fit `planes`
    /// bits — as `3 * planes - 2` coding passes.
    pub(crate) fn encode_code_block(
        values: &[i32],
        w: usize,
        h: usize,
        planes: u32,
        orientation: Orientation,
        segmentation_symbols: bool,
    ) -> (Vec<u8>, u32) {
        let mut state = super::Block::new(w, h, orientation);
        let mut mq = MqEncoder::new(super::CONTEXTS);
        for (index, state_) in [(super::ZERO_CODING, 4u8), (RUN_LENGTH, 3), (UNIFORM, 46)] {
            mq.set_state(index, state_, 0);
        }

        let magnitude: Vec<u32> = values.iter().map(|v| v.unsigned_abs()).collect();
        let negative: Vec<bool> = values.iter().map(|v| *v < 0).collect();
        let passes = 3 * planes - 2;

        for i in 0..passes {
            let (pass, plane) = super::pass_at(i, planes);
            match pass {
                Pass::Significance => {
                    for (top, rows, x) in state.stripes().collect::<Vec<_>>() {
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
        }
        (mq.flush(), passes)
    }
}
