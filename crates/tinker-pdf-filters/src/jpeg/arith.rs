//! The statistical models of JPEG's arithmetic processes (T.81 F.1.4.4 and
//! Table G.2), over the Annex D coder in [`crate::qm`].
//!
//! The coder decides *bits*; this decides *which context* each bit is coded
//! against, and turns the resulting decision trees back into coefficients.
//! The split is the standard's own: Annex D is the coder, Annex F and Annex G
//! are the models, and a decoder gets one right and the other wrong
//! independently.
//!
//! # What adjudicates the two halves, and it is not the same thing
//!
//! **The coder is adjudicated by published data** — T.81 K.4.1's test
//! sequence, in `qm.rs`.
//!
//! **The models are not, because T.81 publishes no arithmetic-coded image.**
//! Annex K was read section by section in September 2026: K.1 and K.2 are
//! quantisation tables, K.3 Huffman tables and their byte lists, K.4 is the
//! arithmetic coder test above and has no second subsection, K.5 to K.10 are
//! filters and guidance with no data in them. T.83 | ISO/IEC 10918-2 is the
//! compliance-test document and its own clause 4.4 says the data ships on
//! diskettes rather than inside it; T.84 | ISO/IEC 10918-3 clause 4.2.1 says
//! the same in its own words. `jpeg.rs`'s header records the URLs and what
//! each returned.
//!
//! So what pins the models here is the next best thing and is named as such:
//! **hand-derived decision sequences**. Each test below states the decisions
//! T.81's own figures say an encoder would emit for a given block, drives
//! them through the real coder, and demands that block back. The derivation
//! is a reading of the clause with no implementation in the middle, in the
//! same spirit as `jpeg/encode.rs`'s two entropy-coded segments derived from
//! Tables K.3 and K.5. It is weaker than K.4.1 and this file says so rather
//! than letting the reader assume otherwise.
//!
//! **A round trip is deliberately absent.** Ruling 13: an encoder written
//! here agreeing with a decoder written here proves the two halves of one
//! misunderstanding agree, and nothing about T.81.
//!
//! # The figures this is
//!
//! | Procedure | Figure | Here |
//! | --- | --- | --- |
//! | `Decode_DC_DIFF` | F.19 | [`decode_dc`] |
//! | `Decode_AC_coefficients` | F.20 | [`decode_ac`] |
//! | `Decode_V(S)` | F.21 | [`decode_v`] |
//! | `Decode_sign_of_V` | F.22 | [`decode_v`]'s first decision |
//! | `Decode_log2_Sz` | F.23 | [`decode_v`] |
//! | `Decode_Sz_bits` | F.24 | [`decode_v`] |
//! | `Encode_AC_coefficients_SA`, reversed | G.10 | [`decode_ac_refine`] |
//! | `CodeSA_ZZ(K)`, reversed | G.11 | [`decode_ac_refine`] |
//!
//! G.2 is why the last two say "reversed": T.81 draws the progressive
//! *encoder* and then states that "decoder operation is defined by reversing
//! the function of each step described in the encoder flow charts, and
//! performing the steps in reverse order". There is no decoder figure to
//! check against, which is one more reason the tests state the decisions
//! rather than the bytes.

use crate::qm::{QmContext, QmDecoder};

/// The arithmetic conditioning of T.81 B.2.4.3, four destinations of each
/// kind, as the DAC marker segment leaves them.
#[derive(Clone, Copy, Debug)]
pub(super) struct Conditioning {
    /// `(L, U)` per DC destination — F.1.4.4.1.2's bounds on the "small"
    /// difference category. B.2.4.3 packs them into one `Cs` byte as
    /// `L + 16 * U`.
    dc: [(u8, u8); 4],
    /// `Kx` per AC destination — F.1.4.4.2's split between the two `X2`
    /// statistics bins.
    ac: [usize; 4],
}

impl Default for Conditioning {
    /// F.1.4.4.1.4: `L = 0` and `U = 1`. F.1.4.4.2.1: `Kx = 5`.
    ///
    /// B.2.4.3 calls these "the default arithmetic coding conditioning tables
    /// established by the SOI marker", so a frame with no DAC segment is
    /// decoded with exactly this and is not an error.
    fn default() -> Conditioning {
        Conditioning {
            dc: [(0, 1); 4],
            ac: [5; 4],
        }
    }
}

impl Conditioning {
    /// One DAC marker segment (B.2.4.3, Figure B.8): `(Tc, Tb)` then `Cs`,
    /// repeated.
    ///
    /// Out-of-range values are clamped rather than refused. `Cs` for an AC
    /// table is "in the range 1 <= Kx <= 63" and a zero or a 64 is a
    /// malformed header rather than a capability this build lacks, so it
    /// decodes against the nearest legal conditioning instead of refusing a
    /// frame (ruling 2).
    pub(super) fn define(&mut self, segment: &[u8]) {
        let mut i = 0usize;
        while i + 1 < segment.len() {
            let (Some(&tc_tb), Some(&cs)) = (segment.get(i), segment.get(i + 1)) else {
                break;
            };
            i += 2;
            let destination = usize::from(tc_tb & 0x0F).min(3);
            if tc_tb >> 4 == 0 {
                // Cs = L + 16 * U, both four bits, both 0 to 15.
                let (l, u) = (cs & 0x0F, cs >> 4);
                // "L shall be less than or equal to U" (B.2.4.3). A header
                // that says otherwise would make the "small" band empty; the
                // lower bound is pulled down to U rather than the frame
                // refused.
                if let Some(slot) = self.dc.get_mut(destination) {
                    *slot = (l.min(u), u);
                }
            } else if let Some(slot) = self.ac.get_mut(destination) {
                *slot = usize::from(cs).clamp(1, 63);
            }
        }
    }
}

/// One statistics bin, named the way Tables F.4, F.5 and G.2 name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bin {
    /// A bin of a DC statistics area (Table F.4's 49).
    Dc(usize),
    /// A bin of an AC statistics area (Table F.5's 245, or Table G.2's 189).
    Ac(usize),
    /// F.1.4.4.2's fixed estimate, which adapts nothing and is therefore in no
    /// area at all.
    Fixed,
}

/// The statistics areas of one scan: 49 bins per DC destination (Table F.4)
/// and 245 per AC destination (Table F.5).
///
/// Table G.2's 189 bins for a successive-approximation refinement scan are the
/// first 189 of the AC array — the same area, laid out differently by a
/// different scan, which is what F.1.4.4.2.2's "at the start of a scan ... all
/// statistics bins are re-initialized" makes safe.
pub(super) struct Stats {
    dc: [[QmContext; 49]; 4],
    ac: [[QmContext; 245]; 4],
    /// Which bin each decision was taken against, in order, or `None`.
    ///
    /// **This is the tests' only handle on the part of this file that is not
    /// adjudicated by anything.** A model that puts a decision on the wrong
    /// bin still decodes correctly while every bin is in its initial state,
    /// because the probability is the same in all of them — so asserting
    /// coefficients alone would let a wrong index through. Asserting the bins
    /// catches it on the first decision rather than on whichever later one
    /// happens to diverge.
    ///
    /// It is a field rather than a `#[cfg(test)]` one so that the code the
    /// tests exercise is the code that ships, at the cost of one branch per
    /// decision against a `None` that a decode never sets.
    trace: Option<Vec<Bin>>,
}

impl Stats {
    pub(super) fn new() -> Stats {
        Stats {
            dc: [[QmContext::default(); 49]; 4],
            ac: [[QmContext::default(); 245]; 4],
            trace: None,
        }
    }

    /// F.1.4.4.1.5 and F.1.4.4.2.2: every bin returns to Annex D's initial
    /// state at the start of a scan and at each restart interval.
    ///
    /// The trace is deliberately *not* reset — it spans a whole decode,
    /// because a restart is one of the things a test wants to see the far side
    /// of.
    pub(super) fn reset(&mut self) {
        self.dc = [[QmContext::default(); 49]; 4];
        self.ac = [[QmContext::default(); 245]; 4];
    }
}

/// One decision against `bins[at]`, recorded as `kind(at)`.
///
/// An index past the end cannot come from the models below — every one is
/// bounded by Table F.4's 49 or Table F.5's 245 — so it is a programming error
/// rather than a data condition, and it decodes as 0 without touching the
/// coder rather than panicking (ruling 1).
fn decide(
    coder: &mut QmDecoder,
    bins: &mut [QmContext],
    trace: &mut Option<Vec<Bin>>,
    kind: fn(usize) -> Bin,
    at: usize,
) -> u8 {
    if let Some(trace) = trace {
        trace.push(kind(at));
    }
    match bins.get_mut(at) {
        Some(cx) => coder.decode(cx),
        None => 0,
    }
}

/// One decision against F.1.4.4.2's fixed estimate.
fn decide_fixed(coder: &mut QmDecoder, trace: &mut Option<Vec<Bin>>) -> u8 {
    if let Some(trace) = trace {
        trace.push(Bin::Fixed);
    }
    coder.decode_fixed()
}

/// `Decode_V(S)` (T.81 F.2.4.3.1, Figures F.21 to F.24): the signed, non-zero
/// value whose sign, magnitude category and magnitude bits follow.
///
/// `sign` is the context for `Decode_sign_of_V`, or `None` for the fixed
/// estimate that F.1.4.4.2 gives an AC coefficient's sign. `sp` and `sn` are
/// the two `Sz < 1` bins the sign chooses between — Table F.5 makes them the
/// same bin for AC and Table F.4 keeps them apart for DC. `x1` is the `Sz < 2`
/// bin and `x2` the `Sz < 4` bin, after which the bins run upward one per
/// doubling.
///
/// `None` means T.81 F.2.4.4 b)'s "physically impossible data": a magnitude
/// past `X15`, which is beyond what the model can express and which that
/// clause says a decoder should detect, "as otherwise the decoder may reach a
/// condition where it uses the compressed data very slowly".
#[allow(clippy::too_many_arguments)]
fn decode_v(
    coder: &mut QmDecoder,
    bins: &mut [QmContext],
    trace: &mut Option<Vec<Bin>>,
    kind: fn(usize) -> Bin,
    sign: Option<usize>,
    sp: usize,
    sn: usize,
    x1: usize,
    x2: usize,
) -> Option<i32> {
    // Figure F.22. SIGN = 0 is positive; the context-index for the first
    // magnitude decision is chosen by it, which is F.1.4.4.1.1's
    // "conditioned on the sign of V".
    let negative = match sign {
        Some(ss) => decide(coder, bins, trace, kind, ss),
        None => decide_fixed(coder, trace),
    } == 1;
    let mut s = if negative { sn } else { sp };

    // Figure F.23. M is the upper bound for the magnitude, shifted left until
    // a decision is zero and then shifted right by one to become the leading
    // bit of Sz.
    let mut m: u32 = 1;
    if decide(coder, bins, trace, kind, s) != 0 {
        m = 2;
        s = x1;
        if decide(coder, bins, trace, kind, s) != 0 {
            m = 4;
            s = x2;
            // X2 to X15 is fourteen bins, so the run of 1-decisions this loop
            // will follow is at most thirteen long. A fourteenth would ask for
            // X16, which Table F.4 and Table F.5 both stop short of.
            while decide(coder, bins, trace, kind, s) != 0 {
                if m >= 1 << 15 {
                    return None;
                }
                m <<= 1;
                s += 1;
            }
        }
    }
    m >>= 1;
    let mut sz = m;

    // Figure F.24: the low-order magnitude bits, most significant first, all
    // against the one bin Mn = Xn + 14 that Table F.4 and Table F.5 pair with
    // the Xn the category ended on.
    let s = s + 14;
    loop {
        m >>= 1;
        if m == 0 {
            break;
        }
        if decide(coder, bins, trace, kind, s) != 0 {
            sz |= m;
        }
    }

    // F.2.4.3.1: "the value decoded for Sz must be incremented by 1 to get the
    // actual coefficient magnitude".
    let v = i32::try_from(sz).ok()?.checked_add(1)?;
    Some(if negative { -v } else { v })
}

/// `DC_Context(Da)` (T.81 F.1.4.4.1.2 and F.1.4.4.1.3): which of the five
/// four-bin sets the previous block's difference puts this one in.
///
/// Figure F.10's five classes are zero, small positive, small negative, large
/// positive and large negative. The bounds are F.1.4.4.1.2's: the lower bound
/// is *exclusive* and is zero for `L = 0` and `2^(L-1)` otherwise; the upper
/// bound is *inclusive* and is `2^U`. With the default `L = 0`, `U = 1` that
/// makes 1 and 2 small and 3 upward large, which is what Figure F.10 draws.
///
/// **Which class gets which of 0, 4, 8, 12 and 16 is not pinned by T.81, and
/// does not need to be.** F.1.4.4.1.3 says only that `DC_Context(Da)`
/// "provides a value of 0, 4, 8, 12 or 16, depending on the difference
/// classification of Da". Any bijection decodes an encoder's output
/// identically, because every statistics bin starts in the same state
/// (D.2.7) and each class's bin therefore accumulates exactly that class's
/// decisions whichever slot it is kept in — the same argument `mq.rs` records
/// for JBIG2's free choice of context numbering. What is *not* free is the
/// classification itself, which is why the bounds above are the part with
/// clause numbers on them.
fn dc_context(da: i32, l: u8, u: u8) -> usize {
    let lower = if l == 0 {
        0i64
    } else {
        1i64 << (l.min(15) - 1)
    };
    let upper = 1i64 << u.min(15);
    let magnitude = i64::from(da).abs();

    if magnitude <= lower {
        0
    } else if magnitude <= upper {
        if da > 0 {
            4
        } else {
            8
        }
    } else if da > 0 {
        12
    } else {
        16
    }
}

/// `Decode_DC_DIFF` (T.81 F.2.4.1, Figure F.19), and G.1.3.1's refinement.
///
/// `prediction` is the running DC predictor of F.2.1.3.1 and `da` is
/// F.1.4.4.1.2's difference from the previous block of this component; both
/// belong to the component and both are reset at a scan and at every restart.
#[allow(clippy::too_many_arguments)]
fn decode_dc(
    coder: &mut QmDecoder,
    stats: &mut Stats,
    cond: &Conditioning,
    destination: usize,
    prediction: &mut i32,
    da: &mut i32,
    block: &mut [i32; 64],
    ah: u32,
    al: u32,
) -> bool {
    let al = al.min(15);
    if ah != 0 {
        // G.1.3.1: "In subsequent scans using successive approximation the
        // least significant bits shall be coded as binary decisions using a
        // fixed probability estimate of 0.5 (Qe = X'5A1D', MPS = 0)."
        if decide_fixed(coder, &mut stats.trace) == 1 {
            block[0] |= 1 << al;
        }
        return true;
    }

    let Stats { dc, trace, .. } = stats;
    let Some(bins) = dc.get_mut(destination.min(3)) else {
        return false;
    };
    let (l, u) = cond.dc.get(destination.min(3)).copied().unwrap_or((0, 1));
    let s0 = dc_context(*da, l, u);

    let diff = if decide(coder, bins, trace, Bin::Dc, s0) == 0 {
        0
    } else {
        // Table F.4: SS = S0 + 1, SP = S0 + 2, SN = S0 + 3, X1 = 20, X2 = 21.
        match decode_v(
            coder,
            bins,
            trace,
            Bin::Dc,
            Some(s0 + 1),
            s0 + 2,
            s0 + 3,
            20,
            21,
        ) {
            Some(v) => v,
            None => return false,
        }
    };

    *da = diff;
    *prediction = prediction.saturating_add(diff);
    block[0] = prediction.saturating_mul(1 << al);
    true
}

/// `Decode_AC_coefficients` (T.81 F.2.4.2, Figure F.20).
///
/// This is both the sequential scan of F.2.4.2 — where `Kmin` is 1 and `Se` is
/// 63 — and the first successive-approximation scan of G.1.3.2, where they are
/// the scan header's `Ss` and `Se` and the decoded value is scaled by the
/// point transform. G.1.3.2 says so in as many words: "Except for the point
/// transform scaling of the DCT coefficients and the grouping of the
/// coefficients into bands, the first scan(s) of successive approximation is
/// identical to the sequential encoding procedure described in F.1.4."
fn decode_ac(
    coder: &mut QmDecoder,
    stats: &mut Stats,
    cond: &Conditioning,
    destination: usize,
    block: &mut [i32; 64],
    band: (usize, usize),
    al: u32,
) -> bool {
    let destination = destination.min(3);
    let Stats { ac, trace, .. } = stats;
    let Some(bins) = ac.get_mut(destination) else {
        return false;
    };
    let kx = cond.ac.get(destination).copied().unwrap_or(5);
    let (kmin, se) = band;
    let al = al.min(15);

    let mut k = kmin.max(1);
    while k <= se {
        // SE = 3 * (K - 1): the end-of-block decision, and in a progressive
        // band the end-of-band one (G.1.3.2's NOTE).
        if decide(coder, bins, trace, Bin::Ac, 3 * (k - 1)) == 1 {
            return true;
        }
        loop {
            let s0 = 3 * (k - 1) + 1;
            if decide(coder, bins, trace, Bin::Ac, s0) == 0 {
                // A run of zero coefficients: the inner loop of Figure F.20,
                // which does not code an EOB decision on the way.
                k += 1;
                if k > se {
                    // Figure F.20 cannot leave the band without an EOB
                    // decision or a coefficient at Se, so this is F.2.4.4 b)'s
                    // impossible data.
                    return false;
                }
                continue;
            }
            // Table F.5: SS is the fixed estimate, SN = SP = X1 = S0 + 1, and
            // X2 = AC_Context(K) is 189 at or below Kx and 217 above it.
            let x2 = if k <= kx { 189 } else { 217 };
            let Some(v) = decode_v(
                coder,
                bins,
                trace,
                Bin::Ac,
                None,
                s0 + 1,
                s0 + 1,
                s0 + 1,
                x2,
            ) else {
                return false;
            };
            if let Some(slot) = block.get_mut(k) {
                *slot = v.saturating_mul(1 << al);
            }
            if k == se {
                return true;
            }
            k += 1;
            break;
        }
    }
    true
}

/// `Encode_AC_coefficients_SA` and `CodeSA_ZZ(K)` reversed (T.81 G.1.3.3,
/// Figures G.10 and G.11, under G.2's instruction to reverse them).
///
/// A refinement scan carries two different things in one decision stream: a
/// correction bit for each coefficient an earlier scan already found, and the
/// run-lengths that place coefficients becoming non-zero now. Which one is
/// next is not in the stream — the decoder knows it from the coefficients it
/// already holds, which is why Figure G.11 codes the correction bit at `SC`
/// with no `S0` decision in front of it.
///
/// `EOBx` is not carried between scans either. G.1.3.2 defines the EOB as "the
/// position following the last non-zero coefficient in the band", so it is
/// recomputed from the block, and Figure G.10 skips the end-of-band decision
/// for every `K` below it.
fn decode_ac_refine(
    coder: &mut QmDecoder,
    stats: &mut Stats,
    destination: usize,
    block: &mut [i32; 64],
    band: (usize, usize),
    al: u32,
) -> bool {
    let Stats { ac, trace, .. } = stats;
    let Some(bins) = ac.get_mut(destination.min(3)) else {
        return false;
    };
    let (ss, se) = band;
    let al = al.min(14);
    let step = 1i32 << al;

    // G.1.3.2's EOB: one past the last non-zero coefficient in the band.
    let mut eobx = ss;
    for k in ss..=se.min(63) {
        if block.get(k).copied().unwrap_or(0) != 0 {
            eobx = k + 1;
        }
    }

    let mut k = ss.max(1);
    while k <= se {
        if k >= eobx && decide(coder, bins, trace, Bin::Ac, 3 * (k - 1)) == 1 {
            return true;
        }
        loop {
            let s0 = 3 * (k - 1) + 1;
            let coefficient = block.get(k).copied().unwrap_or(0);
            if coefficient != 0 {
                // Figure G.11's `|ZZ(K)| > 1` branch, from the decoder's side:
                // this coefficient was already non-zero, so what follows is
                // its correction bit at SC = S0 + 1 and nothing else.
                if decide(coder, bins, trace, Bin::Ac, s0 + 1) == 1 && coefficient & step == 0 {
                    if let Some(slot) = block.get_mut(k) {
                        *slot = if coefficient >= 0 {
                            coefficient.saturating_add(step)
                        } else {
                            coefficient.saturating_sub(step)
                        };
                    }
                }
                break;
            }
            if decide(coder, bins, trace, Bin::Ac, s0) == 0 {
                k += 1;
                if k > se {
                    return false;
                }
                continue;
            }
            // Figure G.11's other branch: a coefficient of magnitude one,
            // whose sign is coded with the fixed estimate. Code_0(SS) is
            // positive, Code_1(SS) negative.
            let value = if decide_fixed(coder, trace) == 1 {
                -step
            } else {
                step
            };
            if let Some(slot) = block.get_mut(k) {
                *slot = value;
            }
            break;
        }
        if k == se {
            return true;
        }
        k += 1;
    }
    true
}

/// One block, in whichever of the four arithmetic codings this scan is using.
///
/// The four are the same four the Huffman path has — sequential, progressive
/// DC first, progressive DC refinement, progressive AC first and refinement —
/// because the coding model is chosen by the scan header rather than by the
/// entropy coder. The seam is exactly `decode_block`'s.
#[allow(clippy::too_many_arguments)]
pub(super) fn decode_block(
    coder: &mut QmDecoder,
    stats: &mut Stats,
    cond: &Conditioning,
    destinations: (usize, usize),
    prediction: &mut i32,
    da: &mut i32,
    block: &mut [i32; 64],
    progressive: bool,
    band: (usize, usize),
    approximation: (u32, u32),
) -> bool {
    let (ss, se) = band;
    let (ah, al) = approximation;
    let (dc_destination, ac_destination) = destinations;

    if !progressive {
        return decode_dc(
            coder,
            stats,
            cond,
            dc_destination,
            prediction,
            da,
            block,
            0,
            0,
        ) && decode_ac(coder, stats, cond, ac_destination, block, (1, 63), 0);
    }

    if ss == 0 {
        return decode_dc(
            coder,
            stats,
            cond,
            dc_destination,
            prediction,
            da,
            block,
            ah,
            al,
        );
    }

    if ah == 0 {
        decode_ac(coder, stats, cond, ac_destination, block, (ss, se), al)
    } else {
        decode_ac_refine(coder, stats, ac_destination, block, (ss, se), al)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qm::encoder::QmEncoder;

    /// Encodes a hand-derived decision list into an entropy-coded segment.
    ///
    /// The list is the test's own statement of what T.81's figures say an
    /// encoder emits, bin by bin; nothing in `super` is consulted to build it,
    /// which is the point. Both ends of the coder it passes through are pinned
    /// to K.4.1's published data in `qm.rs`, so what this carries from one side
    /// to the other is the *model*, not a shared misunderstanding (ruling 13).
    fn encode(decisions: &[(Bin, u8)]) -> Vec<u8> {
        let mut encoder = QmEncoder::new();
        let mut dc = [QmContext::default(); 49];
        let mut ac = [QmContext::default(); 245];
        for &(bin, d) in decisions {
            match bin {
                Bin::Dc(at) => encoder.encode(&mut dc[at], d),
                Bin::Ac(at) => encoder.encode(&mut ac[at], d),
                Bin::Fixed => encoder.encode_fixed(d),
            }
        }
        encoder.finish()
    }

    fn traced() -> Stats {
        let mut stats = Stats::new();
        stats.trace = Some(Vec::new());
        stats
    }

    fn bins(decisions: &[(Bin, u8)]) -> Vec<Bin> {
        decisions.iter().map(|&(bin, _)| bin).collect()
    }

    /// One sequential block, decision by decision, from Figures F.19 to F.24.
    ///
    /// `DIFF = +1` and then `ZZ(1) = -3` and an end-of-block. Every line below
    /// is a step of a named figure:
    ///
    /// - `Dc(0) = 1` — F.19's `Decode(S0)`. `Da` is zero at the start of a
    ///   scan (F.1.4.4.1.5), so `DC_Context(0)` is the zero class.
    /// - `Dc(1) = 0` — F.22's sign, at `SS = S0 + 1`; 0 is positive.
    /// - `Dc(2) = 0` — F.23's first decision, at `SP = S0 + 2`. Zero means
    ///   `Sz < 1`, so `Sz = 0` and `V = Sz + 1 = 1`.
    /// - `Ac(0) = 0` — F.20's EOB decision at `SE = 3(K - 1)` with `K = 1`.
    /// - `Ac(1) = 1` — F.20's `Decode(S0)` at `S0 = SE + 1`: this coefficient
    ///   is not zero.
    /// - `Fixed = 1` — F.1.4.4.2: an AC sign is coded at the fixed estimate,
    ///   never adaptively. 1 is negative.
    /// - `Ac(2) = 1` twice — Table F.5 makes `SN`, `SP` and `X1` **the same
    ///   bin**, `S0 + 1`, so `Sz >= 1` and `Sz >= 2` are two decisions against
    ///   one statistics bin. That repeat is the table's own oddity and the
    ///   trace is where it is visible.
    /// - `Ac(189) = 0` — `X2 = AC_Context(K)`, which is 189 for `K <= Kx`, and
    ///   `Kx` defaults to 5. Zero ends the category at `Sz < 4`, so `Sz = 2`.
    /// - `Ac(203) = 0` — F.24's magnitude bit at `M2 = X2 + 14`. So `Sz` stays
    ///   2 and `V = 3`, negative.
    /// - `Ac(3) = 1` — F.20 again at `K = 2`, where `SE = 3`: end of block.
    #[test]
    fn a_sequential_block_is_figures_f_19_to_f_24_decision_by_decision() {
        let decisions = [
            (Bin::Dc(0), 1),
            (Bin::Dc(1), 0),
            (Bin::Dc(2), 0),
            (Bin::Ac(0), 0),
            (Bin::Ac(1), 1),
            (Bin::Fixed, 1),
            (Bin::Ac(2), 1),
            (Bin::Ac(2), 1),
            (Bin::Ac(189), 0),
            (Bin::Ac(203), 0),
            (Bin::Ac(3), 1),
        ];

        let data = encode(&decisions);
        let mut coder = QmDecoder::new(&data);
        let mut stats = traced();
        let mut block = [0i32; 64];
        let (mut prediction, mut da) = (0i32, 0i32);

        assert!(decode_block(
            &mut coder,
            &mut stats,
            &Conditioning::default(),
            (0, 0),
            &mut prediction,
            &mut da,
            &mut block,
            false,
            (0, 63),
            (0, 0),
        ));

        assert_eq!(block[0], 1, "DIFF");
        assert_eq!(block[1], -3, "ZZ(1)");
        assert!(block[2..].iter().all(|&c| c == 0), "the rest of the block");
        assert_eq!(da, 1, "F.1.4.4.1.2's Da is the difference, not the value");
        assert_eq!(stats.trace.unwrap(), bins(&decisions));
    }

    /// **F.1.4.4.1.2's five conditioning classes, one block each.**
    ///
    /// `DC_Context(Da)` is the only part of the DC model that depends on
    /// anything outside the current block, and its bounds are the part T.81
    /// pins: with the default `L = 0` and `U = 1` the "small" band is a
    /// magnitude of 1 or 2 and everything above it is "large", which is what
    /// Figure F.10 draws. The five blocks below walk `Da` through zero, small
    /// positive, small negative, large positive and large negative, and the
    /// trace shows the first decision of each landing on a different four-bin
    /// set.
    ///
    /// Blocks three and four also reach Figure F.23's loop: `V = 5` means
    /// `Sz = 4`, which needs `X1`, `X2` and `X3` and then two magnitude bits
    /// at `M3 = X3 + 14 = 36`.
    #[test]
    fn the_dc_conditioning_class_moves_the_whole_four_bin_set() {
        /// The context-index the block's first decision should land on, the
        /// decisions T.81's figures say it is coded as, and the `DIFF` they
        /// carry.
        type Block = (usize, Vec<(Bin, u8)>, i32);

        let blocks: [Block; 5] = [
            (
                0,
                vec![(Bin::Dc(0), 1), (Bin::Dc(1), 0), (Bin::Dc(2), 0)],
                1,
            ),
            (
                4,
                vec![(Bin::Dc(4), 1), (Bin::Dc(5), 1), (Bin::Dc(7), 0)],
                -1,
            ),
            (
                8,
                vec![
                    (Bin::Dc(8), 1),
                    (Bin::Dc(9), 0),
                    (Bin::Dc(10), 1),
                    (Bin::Dc(20), 1),
                    (Bin::Dc(21), 1),
                    (Bin::Dc(22), 0),
                    (Bin::Dc(36), 0),
                    (Bin::Dc(36), 0),
                ],
                5,
            ),
            (
                12,
                vec![
                    (Bin::Dc(12), 1),
                    (Bin::Dc(13), 1),
                    (Bin::Dc(15), 1),
                    (Bin::Dc(20), 1),
                    (Bin::Dc(21), 1),
                    (Bin::Dc(22), 0),
                    (Bin::Dc(36), 0),
                    (Bin::Dc(36), 0),
                ],
                -5,
            ),
            (16, vec![(Bin::Dc(16), 0)], 0),
        ];

        let mut decisions = Vec::new();
        for (_, block, _) in &blocks {
            decisions.extend_from_slice(block);
            // Each block ends with an immediate end-of-block at K = 1.
            decisions.push((Bin::Ac(0), 1));
        }

        let data = encode(&decisions);
        let mut coder = QmDecoder::new(&data);
        let mut stats = traced();
        let (mut prediction, mut da) = (0i32, 0i32);

        let mut expected = 0i32;
        for (s0, _, diff) in &blocks {
            let mut block = [0i32; 64];
            assert!(decode_block(
                &mut coder,
                &mut stats,
                &Conditioning::default(),
                (0, 0),
                &mut prediction,
                &mut da,
                &mut block,
                false,
                (0, 63),
                (0, 0),
            ));
            expected += diff;
            assert_eq!(block[0], expected, "S0 = {s0}");
            assert_eq!(da, *diff);
        }

        assert_eq!(stats.trace.unwrap(), bins(&decisions));
    }

    /// The decisions for one AC coefficient at zig-zag index `k`, preceded by
    /// the run of zeros that reaches it and followed by an end-of-block.
    ///
    /// `x2` is `AC_Context(k)`, which the caller states rather than this
    /// deriving — the whole point of the test below is that the model picks
    /// the same one.
    fn one_ac_coefficient(k: usize, x2: usize) -> Vec<(Bin, u8)> {
        let mut decisions = vec![(Bin::Ac(0), 0)];
        for run in 1..k {
            decisions.push((Bin::Ac(3 * (run - 1) + 1), 0));
        }
        let s0 = 3 * (k - 1) + 1;
        decisions.extend_from_slice(&[
            (Bin::Ac(s0), 1),
            (Bin::Fixed, 0),
            (Bin::Ac(s0 + 1), 1),
            (Bin::Ac(s0 + 1), 1),
            (Bin::Ac(x2), 0),
            (Bin::Ac(x2 + 14), 1),
            (Bin::Ac(3 * k), 1),
        ]);
        decisions
    }

    /// **`AC_Context(K)` is 189 at or below `Kx` and 217 above it**
    /// (F.1.4.4.2), and `Kx` is 5 until a DAC segment says otherwise
    /// (F.1.4.4.2.1).
    ///
    /// The same coefficient at `K = 6` is decoded twice: once under the
    /// default conditioning, where it is above `Kx` and uses 217, and once
    /// with `Kx = 6` from a DAC segment, where it is not and uses 189. The
    /// decoded value is identical both times — which is exactly why the trace
    /// exists, because the coefficients alone could not tell the two apart.
    #[test]
    fn ac_context_splits_at_kx_and_dac_moves_where() {
        for (kx, dac) in [(5usize, None), (6, Some([0x10u8, 0x06]))] {
            let decisions = one_ac_coefficient(6, if 6 <= kx { 189 } else { 217 });
            let data = encode(&decisions);

            let mut conditioning = Conditioning::default();
            if let Some(segment) = dac {
                // B.2.4.3: Tc = 1 is an AC table, Tb = 0 the destination, and
                // Cs is Kx itself.
                conditioning.define(&segment);
            }
            assert_eq!(conditioning.ac[0], kx);

            let mut coder = QmDecoder::new(&data);
            let mut stats = traced();
            let mut block = [0i32; 64];
            let (mut prediction, mut da) = (0i32, 0i32);
            // Ss = 1 skips the DC half: this is a progressive AC-only scan,
            // which G.1.3.2 says is the sequential model with Kmin = Ss.
            assert!(decode_block(
                &mut coder,
                &mut stats,
                &conditioning,
                (0, 0),
                &mut prediction,
                &mut da,
                &mut block,
                true,
                (1, 63),
                (0, 0),
            ));

            assert_eq!(block[6], 4, "Kx = {kx}");
            assert_eq!(stats.trace.unwrap(), bins(&decisions), "Kx = {kx}");
        }
    }

    /// **A DAC segment moves `L` and `U`, and that moves which class a `Da`
    /// falls in** (B.2.4.3 and F.1.4.4.1.2).
    ///
    /// With `L = 1` and `U = 3` the lower bound becomes `2^(L-1) = 1` and the
    /// upper `2^U = 8`, so a `Da` of 1 is no longer "small positive" — it is
    /// at the lower bound and conditions as zero. The second block's first
    /// decision lands on bin 0 instead of bin 4 and nothing else changes.
    ///
    /// This is also the test that would have caught the DAC marker being
    /// skipped: before September 2026 `jpeg.rs` stepped over `X'FFCC'` as
    /// though it were a comment, while a comment beside the SOF refusals said
    /// it was handled.
    #[test]
    fn dac_moves_the_dc_conditioning_bounds() {
        for (dac, second_s0) in [(None, 4usize), (Some([0x00u8, 0x31]), 0)] {
            let decisions = vec![
                (Bin::Dc(0), 1),
                (Bin::Dc(1), 0),
                (Bin::Dc(2), 0),
                (Bin::Ac(0), 1),
                (Bin::Dc(second_s0), 0),
                (Bin::Ac(0), 1),
            ];
            let data = encode(&decisions);

            let mut conditioning = Conditioning::default();
            if let Some(segment) = dac {
                // Cs = L + 16 * U, so X'31' is L = 1, U = 3.
                conditioning.define(&segment);
            }

            let mut coder = QmDecoder::new(&data);
            let mut stats = traced();
            let (mut prediction, mut da) = (0i32, 0i32);
            for _ in 0..2 {
                let mut block = [0i32; 64];
                assert!(decode_block(
                    &mut coder,
                    &mut stats,
                    &conditioning,
                    (0, 0),
                    &mut prediction,
                    &mut da,
                    &mut block,
                    false,
                    (0, 63),
                    (0, 0),
                ));
            }
            assert_eq!(prediction, 1);
            assert_eq!(stats.trace.unwrap(), bins(&decisions));
        }

        // The bounds themselves, read straight off F.1.4.4.1.2: the lower one
        // is exclusive and the upper inclusive.
        assert_eq!(dc_context(0, 0, 1), 0);
        assert_eq!(dc_context(2, 0, 1), 4);
        assert_eq!(dc_context(3, 0, 1), 12);
        assert_eq!(dc_context(-2, 0, 1), 8);
        assert_eq!(dc_context(-3, 0, 1), 16);
        assert_eq!(dc_context(1, 1, 3), 0, "|Da| <= 2^(L-1) conditions as zero");
        assert_eq!(dc_context(8, 1, 3), 4, "and 2^U is inclusive");
        assert_eq!(dc_context(9, 1, 3), 12);
    }

    /// **Progressive DC: the first scan is F.19 scaled, and every later one is
    /// a single decision at the fixed estimate** (G.1.3.1).
    #[test]
    fn progressive_dc_refines_one_bit_at_the_fixed_estimate() {
        let first = [(Bin::Dc(0), 1), (Bin::Dc(1), 0), (Bin::Dc(2), 0)];
        let refinement = [(Bin::Fixed, 1)];

        let mut block = [0i32; 64];
        let (mut prediction, mut da) = (0i32, 0i32);
        let mut stats = traced();

        let data = encode(&first);
        let mut coder = QmDecoder::new(&data);
        assert!(decode_block(
            &mut coder,
            &mut stats,
            &Conditioning::default(),
            (0, 0),
            &mut prediction,
            &mut da,
            &mut block,
            true,
            (0, 0),
            (0, 1),
        ));
        assert_eq!(block[0], 2, "Al = 1, so the point transform doubles it");

        let data = encode(&refinement);
        let mut coder = QmDecoder::new(&data);
        assert!(decode_block(
            &mut coder,
            &mut stats,
            &Conditioning::default(),
            (0, 0),
            &mut prediction,
            &mut da,
            &mut block,
            true,
            (0, 0),
            (1, 0),
        ));
        assert_eq!(block[0], 3, "the next bit down is ORed in");

        let mut expected = bins(&first);
        expected.extend(bins(&refinement));
        assert_eq!(stats.trace.unwrap(), expected);
    }

    /// **Progressive AC refinement, Figures G.10 and G.11 read backwards.**
    ///
    /// The band is `Ss = 1` to `Se = 5` and an earlier scan left a coefficient
    /// at `K = 1` and nothing else, so G.1.3.2's end-of-band — "the position
    /// following the last non-zero coefficient in the band" — is 2.
    ///
    /// - `K = 1` is below `EOBx`, so Figure G.10 skips the end-of-band
    ///   decision entirely. The coefficient there is already non-zero, so what
    ///   follows is Figure G.11's correction bit at `SC = S0 + 1 = 2`, and no
    ///   `S0` decision at all.
    /// - `K = 2` is not below `EOBx`, so the end-of-band decision at `SE = 3`
    ///   is read; it is 0. The coefficient is zero, so `S0 = 4` decides
    ///   whether it becomes non-zero; it does, and G.11 codes the sign of a
    ///   magnitude-one coefficient at the fixed estimate.
    /// - `K = 3`: the end-of-band decision at `SE = 6` is 1.
    ///
    /// A decoder that read an `S0` decision for the already-significant
    /// coefficient, or an end-of-band decision below `EOBx`, would take one
    /// decision too many here and desynchronise everything after it.
    #[test]
    fn progressive_ac_refinement_is_figures_g_10_and_g_11() {
        let decisions = [
            (Bin::Ac(2), 1),
            (Bin::Ac(3), 0),
            (Bin::Ac(4), 1),
            (Bin::Fixed, 1),
            (Bin::Ac(6), 1),
        ];

        let data = encode(&decisions);
        let mut coder = QmDecoder::new(&data);
        let mut stats = traced();
        let mut block = [0i32; 64];
        block[1] = 4;
        let (mut prediction, mut da) = (0i32, 0i32);

        assert!(decode_block(
            &mut coder,
            &mut stats,
            &Conditioning::default(),
            (0, 0),
            &mut prediction,
            &mut da,
            &mut block,
            true,
            (1, 5),
            (1, 0),
        ));

        assert_eq!(block[1], 5, "the correction bit at SC");
        assert_eq!(block[2], -1, "newly non-zero, negative");
        assert!(block[3..].iter().all(|&c| c == 0));
        assert_eq!(stats.trace.unwrap(), bins(&decisions));
    }

    /// **T.81 F.2.4.4 b): "decoding a magnitude beyond the range of values
    /// allowed by the model is quite likely when the compressed data are
    /// corrupted", and "for arithmetic decoders this error condition is
    /// extremely important to detect".**
    ///
    /// Figure F.23's loop has fourteen bins, `X2` to `X15`. A fifteenth
    /// 1-decision asks for an `X16` that neither Table F.4 nor Table F.5 has,
    /// and the block is reported damaged rather than decoded to whatever the
    /// next bin happened to hold.
    #[test]
    fn a_magnitude_past_x15_is_reported_rather_than_decoded() {
        let mut decisions = vec![
            // A sequential block decodes its DC difference first, so the AC
            // decisions this test is about have to come after one.
            (Bin::Dc(0), 0),
            (Bin::Ac(0), 0),
            (Bin::Ac(1), 1),
            (Bin::Fixed, 0),
            (Bin::Ac(2), 1),
            (Bin::Ac(2), 1),
        ];
        for bin in 189..=202usize {
            decisions.push((Bin::Ac(bin), 1));
        }

        let data = encode(&decisions);
        let mut coder = QmDecoder::new(&data);
        let mut stats = Stats::new();
        let mut block = [0i32; 64];
        let (mut prediction, mut da) = (0i32, 0i32);

        assert!(!decode_block(
            &mut coder,
            &mut stats,
            &Conditioning::default(),
            (0, 0),
            &mut prediction,
            &mut da,
            &mut block,
            false,
            (0, 63),
            (0, 0),
        ));
    }

    /// Ruling 1 over the models rather than the coder: arbitrary bytes through
    /// every scan shape, and nothing panics or hangs.
    #[test]
    fn no_input_panics_or_hangs() {
        let mut data = Vec::new();
        for byte in 0..=255u8 {
            data.push(byte);
            data.push(byte ^ 0xFF);
        }

        for shape in [
            (false, (0usize, 63usize), (0u32, 0u32)),
            (true, (0, 0), (0, 3)),
            (true, (0, 0), (1, 2)),
            (true, (1, 63), (0, 3)),
            (true, (1, 63), (1, 2)),
            (true, (5, 5), (1, 0)),
        ] {
            let mut coder = QmDecoder::new(&data);
            let mut stats = Stats::new();
            let (mut prediction, mut da) = (0i32, 0i32);
            for _ in 0..64 {
                let mut block = [0i32; 64];
                decode_block(
                    &mut coder,
                    &mut stats,
                    &Conditioning::default(),
                    (0, 0),
                    &mut prediction,
                    &mut da,
                    &mut block,
                    shape.0,
                    shape.1,
                    shape.2,
                );
            }
        }
    }

    /// A DAC segment whose values fall outside B.2.4.3's ranges is clamped
    /// rather than refused (ruling 2), and an odd-length one stops cleanly.
    #[test]
    fn a_malformed_dac_segment_is_clamped_rather_than_refused() {
        let mut conditioning = Conditioning::default();
        // Kx = 0 and Kx = 64 are both outside "1 <= Kx <= 63".
        conditioning.define(&[0x10, 0x00, 0x11, 0x40]);
        assert_eq!(conditioning.ac[0], 1);
        assert_eq!(conditioning.ac[1], 63);

        // "L shall be less than or equal to U": X'12' is L = 2, U = 1.
        conditioning.define(&[0x00, 0x12]);
        assert_eq!(conditioning.dc[0], (1, 1));

        // Tb above 3 has no destination; it lands on the last rather than
        // panicking, and a trailing half-entry is dropped.
        conditioning.define(&[0x0F, 0x21, 0x00]);
        assert_eq!(conditioning.dc[3], (1, 2));
    }
}
