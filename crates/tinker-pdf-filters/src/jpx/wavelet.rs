//! Dequantisation (T.800 E.1) and the inverse wavelet (Annex F).
//!
//! This is where a code-block's signed magnitudes stop being integers from an
//! arithmetic coder and become a picture, and it is the first stage with an
//! **independent oracle**: a reversible 5/3 decode must be byte-identical to
//! `opj_decompress`, because both decoders are exact. Everything before this
//! could only be checked against transcribed tables and a round trip through
//! a writer sharing its own assumptions.
//!
//! That gate does more work than it looks like. Byte-identity here pins the
//! container, tier-2's packet arithmetic, tier-1's context *numbering*,
//! dequantisation, and the DC level shift, all at once, against a decoder
//! sharing no code with this one. T.800 publishes no datastream annex — there
//! is no equivalent of T.88's Annex H.1, which is what gap 17 leaned on — so
//! this comparison carries that weight instead.
//!
//! # Two arithmetics, deliberately not mixed
//!
//! The **5/3** transform is reversible and exact: integer lifting, no rounding
//! beyond the floors the standard writes down, and therefore no fixed-point
//! format at all. Coefficient planes are `i32` at Q0 there.
//!
//! The **9/7** is irreversible and is specified in floating point (Table F.4),
//! which ruling 4 forbids on a pixel path: a float comparison that lands
//! differently on a 32-bit target does not shift a result slightly, it changes
//! which quantisation bucket a coefficient falls in and therefore which
//! integer a sample rounds to. So it runs in the fixed-point format
//! `docs/plans/gaps/18a-jpx-decoder.md` settled **before any decoder code
//! existed**, precisely so the decision would not be made implicitly by
//! whoever wrote the wavelet: `i32` planes at Q12, `i64` constants at Q24,
//! every product formed in `i64` at Q36 and rounded back with
//! `(p + (1 << 23)) >> 24`.
//!
//! The two live behind one [`Arith`] trait rather than in two ladders,
//! because the interleave, the subband placement and the resolution walk of
//! F.3.3 are the *same* for both and a second copy of them is a second thing
//! to get wrong. What differs is three methods: what a dequantised
//! coefficient is, what one line of synthesis does to it, and what integer it
//! finally rounds to.
//!
//! # Why the constants are wider than the data
//!
//! This is the counter-intuitive half of the fixed-point answer and it is
//! worth stating where the constants are, not only in the plan. A constant
//! rounded to Q fractional bits is wrong *relative to the coefficient it
//! multiplies*, where a value rounded into the plane is wrong absolutely. So
//! the data need only enough fraction to stay under an output LSB in absolute
//! terms — Q12 gives 2^-4 of a sample unit over the longest path — while the
//! constants need enough to stay under it against the *largest* coefficient
//! in the file. At Q16 that term is 1.9 sample units for an 8-bit image,
//! which is past a whole level; at Q24 it is 0.03.
//!
//! # The overflow bound is a proof, not a hope
//!
//! Every quantity below is bounded because the input is, and the clamp in
//! [`clamp_dyadic`] is what turns the assumption into a bound: a QCD marker's
//! exponent and mantissa are attacker-controlled and a hostile stream can
//! otherwise ask for any magnitude at all (ruling 1). With a plane entry at
//! most [`PLANE_BOUND`]:
//!
//! - the two scaling steps multiply by at most 2/K = 1.6258, giving 2^30.71;
//! - the four lifting steps each compute `X <- X + c*(A + B)` and so multiply
//!   the running bound by `1 + 2|c|` — x1.887 for delta, x2.766 for gamma,
//!   x1.106 for beta, x4.172 for alpha, 24.08 in total and 4.59 bits — giving
//!   2^35.30;
//! - that value times alpha at Q24 is 2^59.96, against `i64`'s 2^63.
//!
//! Three bits of headroom for the worst case a clamped input can construct.
//! The tightest step-by-step figure is smaller still — the largest product any
//! single step can form is 2^58.90, at the alpha step — and [`MAX_PRODUCT`]
//! pins the plan's own conservative 2^59.96 in a `debug_assert`.

use super::codestream::{Codestream, Quant, QuantStyle};
use super::tier2::{CodeBlock, Orientation, Subband, Tile};
use super::Refusal;

// --- the fixed-point format ---------------------------------------------

/// Fractional bits in a 9/7 coefficient plane. 5/3 planes are Q0.
pub(crate) const Q: u32 = 12;

/// Fractional bits in the six constants of T.800 Table F.4.
pub(crate) const QC: u32 = 24;

/// T.800 Table F.4's `alpha`, as `round(1.586134342059924 * 2^24)`.
///
/// The six below are transcribed as integers and *recomputed from Table F.4's
/// decimals* by `tests::wavelet`, rather than restated there, so a
/// transcription slip in the constant cannot agree with a transcription slip
/// in the test. Every residual is under 2^-25, which is the bound the error
/// analysis in the module documentation assumes.
///
/// The signs live in the lifting steps rather than here: Table F.4 gives
/// `alpha` and `beta` as negative and F.3.8.2 subtracts all four, so two of
/// the four steps add and two subtract. Carrying magnitudes and writing the
/// sign at the point of use keeps the recomputation test comparing decimals
/// against integers rather than against a convention.
pub(crate) const ALPHA: i64 = 26_610_918;
/// Table F.4's `beta`, as `round(0.052980118572961 * 2^24)`.
pub(crate) const BETA: i64 = 888_859;
/// Table F.4's `gamma`, as `round(0.882911075530934 * 2^24)`.
pub(crate) const GAMMA: i64 = 14_812_790;
/// Table F.4's `delta`, as `round(0.443506852043971 * 2^24)`.
pub(crate) const DELTA: i64 = 7_440_810;
/// Table F.4's `K`, as `round(1.230174104914001 * 2^24)`.
///
/// F.3.8.2's first two steps scale the **low** band by `K` and the **high**
/// band by `2/K` — not by `1/K` and `K`. JPEG 2000 normalises the analysis
/// lowpass to DC gain 1 and the analysis highpass to Nyquist gain 2, and the
/// factor of two rides on the high band's scaling. Getting this backwards
/// produces an image with correct structure at wrong contrast, which reads as
/// a colour-management problem rather than a wavelet one — which is exactly
/// how milestone 4's halved dequantisation looked before the oracle caught
/// it.
pub(crate) const K: i64 = 20_638_897;
/// Table F.4's `2/K`, as `round(1.625786132231922 * 2^24)`.
pub(crate) const TWO_OVER_K: i64 = 27_276_165;

/// What a coefficient plane entry may hold, in Q12: 2^30, which is 2^18
/// sample units.
///
/// This is the plan's clamped worst case at the maximum supported precision
/// of 16 bits, where the dequantisation clamp of `2^(R+2)` is 2^18 exactly.
/// It is deliberately *not* scaled down with the component precision, because
/// it guards two different things: the dequantiser clamps a coefficient to
/// `2^(R+2)` and reports it, while this bounds every intermediate the lifting
/// stores back between the horizontal and the vertical pass. An 8-bit image's
/// intermediates live around 2^9 and a 16-bit image's around 2^17, so nothing
/// legitimate comes near it and the [`MAX_PRODUCT`] proof holds at every
/// precision from one figure.
pub(crate) const PLANE_BOUND: i64 = 1 << 30;

/// The largest `i64` product the lifting can form from a clamped plane.
///
/// The plan proves 2^59.96 and this is 2^60, a hair above it, so the
/// `debug_assert` fires on a defect rather than on the bound being tight. The
/// worst single step is smaller — 2^58.90 at the alpha step, which is where
/// both the largest constant and the largest running value meet.
pub(crate) const MAX_PRODUCT: i64 = 1 << 60;

// --- planes ---------------------------------------------------------------

/// One tile-component's samples, after the wavelet and the level shift.
///
/// Generic over the arithmetic so that the ladder is written once. The
/// shipped instantiations are [`Exact`] and [`Fixed`]; the test module adds
/// an `f64` one, which is what makes gate 2 a comparison of *arithmetic*
/// rather than of two independently written wavelets.
pub(crate) struct Plane<T = i32> {
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) samples: Vec<T>,
}

impl<T: Copy + Default> Plane<T> {
    pub(crate) fn at(&self, x: u32, y: u32) -> T {
        self.get(x, y)
    }

    fn get(&self, x: u32, y: u32) -> T {
        if x < self.width && y < self.height {
            self.samples[(y as usize) * (self.width as usize) + (x as usize)]
        } else {
            T::default()
        }
    }

    fn set(&mut self, x: u32, y: u32, v: T) {
        if x < self.width && y < self.height {
            self.samples[(y as usize) * (self.width as usize) + (x as usize)] = v;
        }
    }
}

// --- the two arithmetics --------------------------------------------------

/// The arithmetic one tile-component is reconstructed in.
///
/// Three methods, which is exactly what the 5/3 and the 9/7 disagree about.
/// Everything else — B.5's subband geometry, F.3.3's interleave, the walk up
/// the resolution ladder — is shared, and is shared deliberately: it is the
/// part gate 1 already pins byte-identically against `opj_decompress`, and a
/// second copy written for the 9/7 would be a second copy outside that gate.
pub(crate) trait Arith: Copy + Default {
    /// A dequantised coefficient, given exactly as `m * 2^e`.
    ///
    /// E.1's step size and E.1.1.2's reconstruction offset are both dyadic
    /// rationals, so the dequantiser can hand every arithmetic the same exact
    /// pair and let each round it its own way — which is the property that
    /// keeps the `f64` reference reading the *same* coefficients as the
    /// fixed-point path rather than a separately derived set.
    fn dyadic(m: i64, e: i32) -> Self;

    /// F.3.8's 1D synthesis over one interleaved line, whose first sample
    /// sits at index `i0` of the subband coordinate system — so `i0`'s parity
    /// is what says which end the low band starts on.
    fn synthesise(line: &mut [Self], i0: u32);

    /// The integer sample this coefficient reconstructs to, before G.1's
    /// level shift.
    fn sample(self) -> i32;
}

/// The reversible 5/3's arithmetic: exact integers, Q0, no fraction at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Exact(i32);

/// The irreversible 9/7's arithmetic: Q12 in `i32`, products in `i64` at Q36.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fixed(i32);

impl Arith for Exact {
    fn dyadic(m: i64, e: i32) -> Self {
        Exact(clamp_i32(shift_round(m, e)))
    }

    fn synthesise(line: &mut [Self], i0: u32) {
        // The 5/3 is left on `i32` exactly as milestone 4 wrote it and gate 1
        // proved it. The copy in and out is one allocation per line, which is
        // what its own `out` buffer already cost.
        let mut scratch: Vec<i32> = line.iter().map(|v| v.0).collect();
        synthesise_53(&mut scratch, i0);
        for (dst, src) in line.iter_mut().zip(scratch) {
            *dst = Exact(src);
        }
    }

    fn sample(self) -> i32 {
        self.0
    }
}

impl Arith for Fixed {
    fn dyadic(m: i64, e: i32) -> Self {
        // Q12: the value is `m * 2^e`, so the plane holds `m * 2^(e + 12)`.
        Fixed(clamp_plane(shift_round(m, e + Q as i32)))
    }

    fn synthesise(line: &mut [Self], i0: u32) {
        // The lifting line is `i64` and the plane is `i32`, and that is not
        // an accident of convenience: the running value grows 4.59 bits
        // inside a pass (the four steps multiply the bound by 24.08 in total)
        // and does not fit the plane's own format mid-flight. Keeping the
        // plane at `i32` is what holds a 4096x4096 four-component tile at
        // 268 MB of scratch rather than 536.
        let mut scratch: Vec<i64> = line.iter().map(|v| i64::from(v.0)).collect();
        synthesise_97(&mut scratch, i0);
        for (dst, src) in line.iter_mut().zip(scratch) {
            *dst = Fixed(clamp_plane(src));
        }
    }

    fn sample(self) -> i32 {
        // Round-half-up on an arithmetic shift, which Rust guarantees for
        // signed integers and which is therefore the same on every target.
        // "Round to nearest" has three meanings and only one of them is a
        // specification.
        (self.0 + (1 << (Q - 1))) >> Q
    }
}

/// `m * 2^e`, rounded half-up, saturating rather than wrapping.
///
/// Saturation is unreachable after [`clamp_dyadic`] has run, and is here
/// because "unreachable" is a claim about the clamp rather than about this
/// function, and ruling 1's inputs do not respect claims.
fn shift_round(m: i64, e: i32) -> i64 {
    if e >= 0 {
        if e >= 62 {
            return match m.signum() {
                1 => i64::MAX,
                -1 => i64::MIN,
                _ => 0,
            };
        }
        m.saturating_mul(1i64 << e)
    } else {
        let shift = e.unsigned_abs();
        if shift >= 63 {
            return 0;
        }
        m.saturating_add(1i64 << (shift - 1)) >> shift
    }
}

fn clamp_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn clamp_plane(v: i64) -> i32 {
    v.clamp(-PLANE_BOUND, PLANE_BOUND) as i32
}

/// One Q24 product, rounded back to the line's own Q12.
pub(crate) fn mul_q24(v: i64, c: i64) -> i64 {
    let p = v * c;
    debug_assert!(
        p.unsigned_abs() <= MAX_PRODUCT.unsigned_abs(),
        "the 9/7 lifting formed a product of {p}, past the {MAX_PRODUCT} the \
         clamped worst case bounds it by -- which means either the plane \
         clamp did not run or the bound in the module documentation is wrong"
    );
    (p + (1 << (QC - 1))) >> QC
}

// --- the reversible 5/3 (F.3.8.1) -----------------------------------------

/// Symmetric extension (F.3.6): the signal is mirrored about its ends without
/// repeating the end sample, so index -1 reads index 1.
///
/// The mirroring repeats, which matters for a signal shorter than the
/// extension a filter asks for. Replacing this with zero padding leaves every
/// unit test in the crate passing and fails only the oracle comparison, which
/// is the sharpest available statement of what that gate buys.
fn mirror(n: i64, mut i: i64) -> usize {
    while i < 0 || i >= n {
        if i < 0 {
            i = -i;
        }
        if i >= n {
            i = 2 * (n - 1) - i;
        }
    }
    i as usize
}

/// F.3.8.1's 1D synthesis lifting for the reversible 5/3, in place.
///
/// `y` is the interleaved signal, `i0` its first index. The two steps are the
/// standard's own, and their **order is not interchangeable**: the even step
/// runs first and the odd step reads the evens it just produced. Swapping
/// them is the analysis order, and it decodes to something that still looks
/// like an image.
fn synthesise_53(y: &mut [i32], i0: u32) {
    let n = y.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        // F.3.7: a single sample is scaled rather than lifted, and only when
        // it sits on an odd index.
        if i0 % 2 == 1 {
            y[0] /= 2;
        }
        return;
    }

    let at = |y: &[i32], i: i64| -> i32 { y[mirror(n as i64, i)] };

    let odd_first = i0 % 2 == 1;
    let evens: Vec<usize> = (0..n).filter(|i| (*i % 2 == 0) != odd_first).collect();
    let odds: Vec<usize> = (0..n).filter(|i| (*i % 2 == 0) == odd_first).collect();

    // Step 1: every even sample, from its odd neighbours.
    let mut out = y.to_vec();
    for &i in &evens {
        let i = i as i64;
        out[i as usize] = at(y, i) - (at(y, i - 1) + at(y, i + 1) + 2).div_euclid(4);
    }
    // Step 2: every odd sample, from the evens step 1 just wrote.
    for &i in &odds {
        let i = i as i64;
        let left = at(&out, i - 1);
        let right = at(&out, i + 1);
        out[i as usize] = at(y, i) + (left + right).div_euclid(2);
    }
    y.copy_from_slice(&out);
}

// --- the irreversible 9/7 (F.3.8.2) ---------------------------------------

/// F.3.8.2's 1D synthesis for the irreversible 9/7, over an `i64` Q12 line.
///
/// Six steps, in the standard's own order, and the order is the whole of it:
/// **scale, then delta, gamma, beta, alpha.** Running them as alpha, beta,
/// gamma, delta is the *analysis* order, which is a defect the eye cannot
/// find because an inverse wavelet run backwards is still a smoothing
/// operator and still produces a photograph.
///
/// Each lifting step reads only the opposite parity, which is why they are
/// safe in place and why lifting exists at all: there is no ordering hazard
/// within a step, only between them.
pub(crate) fn synthesise_97(y: &mut [i64], i0: u32) {
    let n = y.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        // F.3.7 again, and it is the reversible rule: a lone sample has no
        // partner band to lift against, so the scaling has nothing to
        // normalise either.
        if i0 % 2 == 1 {
            y[0] /= 2;
        }
        return;
    }

    let odd_first = i0 % 2 == 1;
    // Element `k` sits at subband index `i0 + k`, so it is an even (low-band)
    // sample when `i0 + k` is even.
    let is_even = |k: usize| (k % 2 == 0) != odd_first;

    // STEP1 and STEP2 (F-8, F-9): the low band by K, the high band by 2/K.
    for (k, v) in y.iter_mut().enumerate() {
        *v = mul_q24(*v, if is_even(k) { K } else { TWO_OVER_K });
    }

    // STEP3 to STEP6 (F-10 to F-13). Table F.4 gives alpha and beta as
    // negative and gamma and delta as positive, and all four equations
    // *subtract*, so with magnitudes the signs come out as below. That is not
    // a simplification: it is what OpenJPEG's own inverse does, and the two
    // middle steps changing sign is the detail a transcription loses.
    lift(y, odd_first, true, -DELTA);
    lift(y, odd_first, false, -GAMMA);
    lift(y, odd_first, true, BETA);
    lift(y, odd_first, false, ALPHA);
}

/// One lifting step: every sample of one parity, updated by `c` times the sum
/// of its two neighbours of the other parity.
fn lift(y: &mut [i64], odd_first: bool, target_even: bool, c: i64) {
    let n = y.len() as i64;
    for k in 0..y.len() {
        let even = (k % 2 == 0) != odd_first;
        if even != target_even {
            continue;
        }
        let i = k as i64;
        let left = y[mirror(n, i - 1)];
        let right = y[mirror(n, i + 1)];
        y[k] += mul_q24(left + right, c);
    }
}

// --- dequantisation (E.1) -------------------------------------------------

/// Everything a subband's dequantisation needs that is not in the code-block.
struct BandContext<'a, 'b> {
    stream: &'a Codestream<'b>,
    tile: u16,
    component: usize,
    /// This subband's index in the quantisation marker's list: 0 for
    /// resolution 0's LL, then `3(r-1) + 1` for HL, LH and HH of resolution
    /// `r`. E.1's own band order, and QCD writes its step sizes in it.
    band: usize,
    /// The component's bit depth, which is `R_b` for the irreversible
    /// transform.
    precision: u8,
}

/// E.1's step size for one subband, resolving A.6.4's two spellings.
///
/// Expounded carries one `(exponent, mantissa)` per subband and is a lookup.
/// Derived carries one for the whole component and E-5 derives the rest:
/// `eps_b = eps_0 - N_L + n_b`, which for the band ordering above is
/// `eps_0 - (r - 1)`. Reading a derived marker as though it were expounded
/// takes the LL band's step for every subband, which is a picture with soft
/// detail rather than an error.
fn step_for(quant: &Quant, band: usize) -> Result<(u8, u16), Refusal> {
    match quant.style {
        QuantStyle::Derived => {
            let &(exponent, mantissa) = quant
                .steps
                .first()
                .ok_or(Refusal::Structure("a derived quantisation with no step"))?;
            let resolution = if band == 0 { 0 } else { (band - 1) / 3 + 1 };
            let drop = u8::try_from(resolution.saturating_sub(1)).unwrap_or(u8::MAX);
            Ok((exponent.saturating_sub(drop), mantissa))
        }
        _ => quant
            .steps
            .get(band)
            .copied()
            .ok_or(Refusal::Structure("a subband with no quantisation step")),
    }
}

/// The plan's clamp, applied in the dyadic domain so that nothing has to be
/// evaluated to find out whether it overflows.
///
/// A coefficient is held to `+/- 2^bound` sample units, which is E.1's
/// nominal dynamic range plus one guard bit. This is not tidiness: the
/// exponent and mantissa in a QCD marker are attacker-controlled and a
/// hostile stream can otherwise ask for any magnitude at all, which is what
/// turns the overflow figures in the module documentation from an assumption
/// into a bound (ruling 1).
fn clamp_dyadic(m: i64, e: i32, bound: i32, clamped: &mut bool) -> (i64, i32) {
    // |m * 2^e| <= 2^bound is |m| <= 2^(bound - e).
    let headroom = bound - e;
    if headroom >= 63 {
        return (m, e);
    }
    if headroom < 0 {
        *clamped = true;
        return (m.signum(), bound);
    }
    let limit = 1i64 << headroom;
    if m > limit {
        *clamped = true;
        (limit, e)
    } else if m < -limit {
        *clamped = true;
        (-limit, e)
    } else {
        (m, e)
    }
}

/// E.1: turn a code-block's coded magnitudes into coefficients.
///
/// For the **reversible** path this is almost nothing, and the *almost* is
/// the interesting part. `SPqcd` carries an exponent and no step size, and
/// the coefficient **is** the signed magnitude tier-1 decoded — no
/// realignment.
///
/// The obvious code is wrong here, and it is wrong in a way that looks right.
/// E.1 gives a subband `Mb = G + exp - 1` magnitude bits and a code-block
/// codes `Mb - zero_planes` of them, which reads as though the decoded
/// magnitudes were left-aligned and wanted shifting down by the difference.
/// They are not: tier-1 accumulates with `|= 1 << plane` where `plane` counts
/// down to zero, so the most significant bit it decodes already sits where
/// `Mb - zero_planes` puts it.
///
/// Applying that shift anyway halves every coefficient in a typical stream,
/// and an inverse wavelet turns a uniformly halved subband into a picture of
/// exactly the right shape at half the contrast — which is to say, into
/// something that looks like a decode rather than like a defect. Nothing in
/// this repository would have caught it. `opj_decompress` did, on the first
/// comparison.
///
/// For the **irreversible** path there are three factors and E.1.1.2 supplies
/// the third:
///
/// - the step size `delta_b = 2^(R_b - eps_b) * (1 + mu_b / 2^11)`, where
///   `R_b` is the component precision. The subband gains of Table E.1 are the
///   *reversible* filter's; the 9/7's analysis filters are normalised, so
///   the irreversible gain is zero. Applying Table E.1's gains here scales
///   HL, LH and HH by two, two and four — a picture with the wrong detail
///   contrast, which is precisely the failure this codec is dangerous for.
/// - `2^(Mb - zero_planes - planes)`, the distance a **truncated** stream
///   left at the bottom. A lossy codestream stops sending bit-planes part way
///   down and tier-1 decodes only the ones it was sent, so its magnitudes
///   need putting back where they belong.
/// - `r = 0.5`, E.1.1.2's reconstruction point inside the quantisation
///   interval, which the standard leaves to the decoder. A coefficient that
///   became significant is known to lie in `[q, q+1) * step`, and the
///   midpoint is the choice OpenJPEG makes (its tier-1 carries an extra half
///   bit from the plane a coefficient became significant on, and its
///   dequantiser then halves the step size to pay for it). One that never
///   became significant stays at zero rather than at half a step, which is
///   the asymmetry a decoder that adds `r` unconditionally gets wrong.
///
/// All three are dyadic, so the result is exact as `m * 2^e` with
/// `m = +/-(2q + 1)(2048 + mu)` and `e = t - 12 + R_b - eps_b`.
fn dequantise<A: Arith>(
    ctx: &BandContext<'_, '_>,
    block: &CodeBlock,
    clamped: &mut bool,
) -> Result<Vec<A>, Refusal> {
    let quant = ctx.stream.quant_for(ctx.tile, ctx.component);
    match quant.style {
        QuantStyle::None => Ok(block
            .coefficients
            .iter()
            .map(|&v| A::dyadic(i64::from(v), 0))
            .collect()),
        QuantStyle::Derived | QuantStyle::Expounded => {
            let (exponent, mantissa) = step_for(quant, ctx.band)?;
            // E-2's Mb, and the bit-planes a truncated stream never sent.
            let mb = i32::from(quant.guard_bits) + i32::from(exponent) - 1;
            let truncated =
                (mb - i32::from(block.zero_planes) - i32::from(block.planes)).clamp(0, 64);
            // `(2q + 1)` carries a factor of two and `(2048 + mu)` carries
            // 2^11, so the twelve below is the two of them together.
            let e = truncated - 12 + i32::from(ctx.precision) - i32::from(exponent);
            let scale = 2048 + i64::from(mantissa);
            let bound = i32::from(ctx.precision) + 2;
            Ok(block
                .coefficients
                .iter()
                .map(|&v| {
                    if v == 0 {
                        // Never significant: zero, not half a step.
                        return A::default();
                    }
                    let magnitude = i64::from(v.unsigned_abs());
                    let m = (2 * magnitude + 1) * scale * i64::from(v.signum());
                    let (m, e) = clamp_dyadic(m, e, bound, clamped);
                    A::dyadic(m, e)
                })
                .collect())
        }
    }
}

// --- the ladder (F.3.3 and F.3.4) -----------------------------------------

/// F.3.4's 2D synthesis for one resolution step.
fn synthesise_2d<A: Arith>(plane: &mut Plane<A>, x0: u32, y0: u32) {
    // Rows, then columns -- F.3.4's HOR_SR followed by VER_SR.
    let mut row = vec![A::default(); plane.width as usize];
    for y in 0..plane.height {
        for x in 0..plane.width {
            row[x as usize] = plane.get(x, y);
        }
        A::synthesise(&mut row, x0);
        for x in 0..plane.width {
            plane.set(x, y, row[x as usize]);
        }
    }

    let mut column = vec![A::default(); plane.height as usize];
    for x in 0..plane.width {
        for y in 0..plane.height {
            column[y as usize] = plane.get(x, y);
        }
        A::synthesise(&mut column, y0);
        for y in 0..plane.height {
            plane.set(x, y, column[y as usize]);
        }
    }
}

/// Reconstruct one tile-component in `A`'s arithmetic: dequantise every
/// subband, then run the inverse wavelet up the resolution ladder.
///
/// `pub(crate)` because the test module instantiates it a third time, with an
/// `f64` arithmetic that performs F.3.8.2's steps in floating point. That is
/// the plan's gate 2, and it is only a gate on the *arithmetic* if both sides
/// walk the same ladder over the same coefficients.
pub(crate) fn ladder<A: Arith>(
    stream: &Codestream<'_>,
    tile: &Tile,
    component: usize,
    clamped: &mut bool,
) -> Result<Plane<A>, Refusal> {
    let tc = tile
        .components
        .get(component)
        .ok_or(Refusal::Structure("a tile with no such component"))?;
    let precision = stream
        .siz
        .components
        .get(component)
        .ok_or(Refusal::Structure(
            "a component the SIZ marker does not declare",
        ))?
        .precision;

    // Start from the coarsest LL, which is resolution 0's only subband.
    let mut plane = Plane {
        x0: tc.x0,
        y0: tc.y0,
        width: 0,
        height: 0,
        samples: Vec::new(),
    };

    for (r, resolution) in tc.resolutions.iter().enumerate() {
        let width = resolution.x1.saturating_sub(resolution.x0);
        let height = resolution.y1.saturating_sub(resolution.y0);
        let mut next = Plane {
            x0: resolution.x0,
            y0: resolution.y0,
            width,
            height,
            samples: vec![A::default(); (width as usize) * (height as usize)],
        };

        if r == 0 {
            // Resolution 0 is the LL band alone, placed as it stands.
            for band in &resolution.bands {
                let ctx = BandContext {
                    stream,
                    tile: tile.index,
                    component,
                    band: 0,
                    precision,
                };
                write_band(&ctx, band, &mut next, (0, 0), false, clamped)?;
            }
        } else {
            // F.3.3's interleave: the previous resolution supplies the even
            // rows and columns, and this resolution's three bands supply the
            // rest.
            for y in 0..plane.height {
                for x in 0..plane.width {
                    next.set(2 * x, 2 * y, plane.get(x, y));
                }
            }
            for (k, band) in resolution.bands.iter().enumerate() {
                let offsets = match band.orientation {
                    Orientation::Hl => (1, 0),
                    Orientation::Lh => (0, 1),
                    Orientation::Hh => (1, 1),
                    Orientation::Ll => (0, 0),
                };
                let ctx = BandContext {
                    stream,
                    tile: tile.index,
                    component,
                    band: 3 * (r - 1) + 1 + k,
                    precision,
                };
                write_band(&ctx, band, &mut next, offsets, true, clamped)?;
            }
            synthesise_2d(&mut next, resolution.x0, resolution.y0);
        }
        plane = next;
    }

    Ok(plane)
}

/// Place one subband's dequantised coefficients into the plane.
fn write_band<A: Arith>(
    ctx: &BandContext<'_, '_>,
    band: &Subband,
    plane: &mut Plane<A>,
    (ox, oy): (u32, u32),
    interleaved: bool,
    clamped: &mut bool,
) -> Result<(), Refusal> {
    let bw = band.x1.saturating_sub(band.x0);
    for precinct in &band.precincts {
        for block in &precinct.blocks {
            if block.coefficients.is_empty() {
                continue;
            }
            let values = dequantise::<A>(ctx, block, clamped)?;
            let w = block.width();
            for (i, &v) in values.iter().enumerate() {
                let i = i as u32;
                if w == 0 {
                    break;
                }
                let (lx, ly) = (i % w, i / w);
                let bx = block.x0.saturating_sub(band.x0) + lx;
                let by = block.y0.saturating_sub(band.y0) + ly;
                if bx >= bw && bw != 0 {
                    continue;
                }
                if interleaved {
                    plane.set(2 * bx + ox, 2 * by + oy, v);
                } else {
                    plane.set(bx, by, v);
                }
            }
        }
    }
    Ok(())
}

/// Reconstruct one tile-component, choosing the arithmetic Annex F's two
/// filters need.
///
/// `clamped` records whether E.1's clamp ever fired, which the caller turns
/// into a warning: a stream whose declared step size asks for a magnitude the
/// format cannot hold has been altered to fit, and ruling 10 wants that
/// recorded rather than silently absorbed.
pub(crate) fn reconstruct(
    stream: &Codestream<'_>,
    tile: &Tile,
    component: usize,
    clamped: &mut bool,
) -> Result<Plane, Refusal> {
    let reversible = stream.style_for(tile.index, component).reversible;
    let style = stream.quant_for(tile.index, component).style;
    // A.6.1 pairs the transform with the quantisation style and Table A.28
    // permits only the two combinations below: the reversible 5/3 with no
    // quantisation, the irreversible 9/7 with a scalar one. The mismatch is
    // refused rather than resolved in favour of one of them, because either
    // choice decodes -- exactly the plausible-picture failure this codec is
    // dangerous for.
    match (reversible, style) {
        (true, QuantStyle::None) => Ok(into_samples(ladder::<Exact>(
            stream, tile, component, clamped,
        )?)),
        (false, QuantStyle::Derived | QuantStyle::Expounded) => Ok(into_samples(ladder::<Fixed>(
            stream, tile, component, clamped,
        )?)),
        _ => Err(Refusal::Structure(
            "a wavelet transform and a quantisation style Table A.28 does not pair",
        )),
    }
}

/// The last rounding: one coefficient plane becomes one plane of integers.
pub(crate) fn into_samples<A: Arith>(plane: Plane<A>) -> Plane<i32> {
    Plane {
        x0: plane.x0,
        y0: plane.y0,
        width: plane.width,
        height: plane.height,
        samples: plane.samples.into_iter().map(A::sample).collect(),
    }
}

/// G.1's DC level shift, and the clamp to the component's precision.
pub(crate) fn level_shift(plane: &mut Plane, precision: u8, signed: bool) {
    let shift = if signed {
        0
    } else {
        1i32 << (i32::from(precision) - 1)
    };
    let (lo, hi) = if signed {
        (
            -(1i32 << (i32::from(precision) - 1)),
            (1i32 << (i32::from(precision) - 1)) - 1,
        )
    } else {
        (0, (1i32 << i32::from(precision)) - 1)
    };
    for sample in &mut plane.samples {
        *sample = (*sample + shift).clamp(lo, hi);
    }
}
