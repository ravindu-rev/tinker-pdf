//! The QM arithmetic decoder (ITU-T T.81 Annex D).
//!
//! The entropy coder of JPEG's arithmetic processes — SOF9, SOF10, SOF11,
//! SOF13, SOF14 and SOF15. A caller supplies a *context* — one adaptive
//! probability state — per decision and gets one bit back; everything that
//! makes a decision predictable lives in how the caller picks the context,
//! which for JPEG is the statistical model of T.81 F.1.4.4 and Table G.2.
//!
//! # It is not [`crate::mq`], and the differences are not cosmetic
//!
//! T.88 Annex E's MQ coder and this one are cousins: both are binary adaptive
//! arithmetic coders with a `(state, MPS)` pair per context, both renormalise
//! while `A < X'8000'`, and both carry the same conditional MPS/LPS exchange
//! and the same `NMPS` / `NLPS` / `SWITCH` transition shape. Nothing else is
//! shared, and each of the differences below changes the decoded bits:
//!
//! | | T.88 Annex E (`mq.rs`) | T.81 Annex D (here) |
//! | --- | --- | --- |
//! | State table | Table E.1, 47 rows | Table D.3, **113 rows** |
//! | Which sub-interval sits at the base | the LPS | **the MPS** |
//! | The comparison | `Chigh < Qe` | **`Cx < A`**, after `A = A - Qe` |
//! | What is subtracted, and on which path | `Chigh -= Qe` on the MPS path | **`Cx -= A`** on the LPS path |
//! | `Initdec` | `C = B<<16`; `BYTEIN`; `C <<= 7`; `CT -= 7`; `A = X'8000'` | **two `Byte_in`s, each followed by `C = SLL C 8`; `CT = 0`; `A = X'10000'`** |
//! | Stuffing | after `X'FF'` the next byte is bit-stuffed, added `<< 9`, `CT = 7` | after `X'FF'` comes a stuffed **zero byte**, skipped; the `X'FF'` is `OR`ed in as `X'FF00'` |
//! | At a marker | `C += X'FF00'`, `CT = 8` — **1-bits** forever | nothing is added at all — **0-bits** forever (D.21, and Table K.8 prints "Marker detected: zero byte fed to decoder") |
//!
//! **Exactly one of Table D.3's 113 `Qe` values also appears in Table E.1's
//! 47, and it is `X'0001'`** — the smallest a 15-bit estimate can be, so the
//! two tables meet only where both bottom out, and nowhere by design. That
//! number is [`tests::the_two_qe_tables_share_only_the_smallest_estimate`]
//! rather than a sentence, because it was written here as "disjoint" first and
//! the test said otherwise. So there is no shared core to factor out and this
//! module is a second implementation on purpose.
//!
//! # What adjudicates it
//!
//! **T.81 K.4.1**, which publishes a 256-bit test sequence, the 32 bytes it
//! encodes to, and a symbol-by-symbol trace of both coders (Tables K.7 and
//! K.8). [`tests::annex_k_4_1_decodes_to_the_published_test_sequence`] feeds
//! the published bytes in and demands the published decisions out. Nothing
//! this project wrote is on either side of that comparison: input and expected
//! output are both printed in the standard, which is the standing
//! `jbig2`'s Annex H.2 fixture and `jpx_annex_j.rs`'s Annex J.10 codestream
//! already have.
//!
//! Being exact about what that covers, because the next paragraph is the
//! limit: it covers **26 of Table D.3's 113 rows** — counted, in
//! [`tests::annex_k_4_1_walks_a_counted_part_of_table_d_3`] — the Decode /
//! Cond_MPS_exchange / Cond_LPS_exchange / Renorm_d / Byte_in / Unstuff_0 /
//! Initdec procedures, the `X'FF' X'00'` unstuffing (the published bytes
//! contain one) and the marker rule (they end with `X'FFD9'`).
//!
//! It says nothing about the statistical model that chooses the contexts.
//! **T.81 publishes no arithmetic-coded image** — searched annex by annex in
//! September 2026, and `jpeg.rs`'s header records the search — so the models
//! of F.1.4.4 and Table G.2 are transcribed from their clauses and pinned
//! against hand-derived decision sequences there, which is a weaker thing and
//! is named as one.
//!
//! # Bounded on any input
//!
//! Ruling 1. Past the end of the data `Byte_in` behaves exactly as it does at
//! a detected marker and stops advancing, so a truncated segment decodes to
//! something and terminates; the renormalisation loop shifts a register that
//! is provably non-zero and carries a hard bound anyway.

/// One row of T.81 Table D.3: the LPS probability estimate, where the state
/// moves on each renormalisation, and whether the sense of the MPS flips.
#[derive(Clone, Copy)]
struct QeRow {
    qe: u16,
    nlps: u8,
    nmps: u8,
    switch: bool,
}

const fn row(qe: u16, nlps: u8, nmps: u8, switch: bool) -> QeRow {
    QeRow {
        qe,
        nlps,
        nmps,
        switch,
    }
}

/// T.81 Table D.3 — the 113-row probability estimation state machine.
///
/// **Read twice** (rule 8, and the four mis-transcribed JBIG2 Annex B tables
/// that made it one). Once off `tpdf render --fonts` at 300 dpi of PDF page 64
/// of the W3C's copy, and once mechanically out of the text layer of the same
/// page's `tpdf text`. The two readings agree on all 113 `Qe` values and on
/// all 86 rows the text layer resolves to a single parse; on the other 27 the
/// text layer is *ambiguous* rather than different, because T.81's column
/// rules extract as a literal `1` and a one-digit cell is then
/// indistinguishable from a rule followed by a digit — the same artefact
/// `jpeg/encode.rs` records for Table K.1. The rendered page decides those 27.
///
/// That ambiguity is not the last word on them: K.4.1's published test
/// sequence walks part of this table, and a row it touches is adjudicated by
/// the standard's own bytes rather than by a reading.
///
/// Column order is `Qe`, `Next_Index_LPS`, `Next_Index_MPS`, `Switch_MPS`, the
/// order Table D.3 prints them — deliberately *not* `mq.rs`'s
/// `(qe, nmps, nlps, switch)`, because sharing a field order between two
/// tables whose columns are printed in different orders is how a transcription
/// slips.
#[rustfmt::skip]
const QE: [QeRow; 113] = [
    row(0x5A1D,   1,   1, true ), row(0x2586,  14,   2, false), row(0x1114,  16,   3, false),
    row(0x080B,  18,   4, false), row(0x03D8,  20,   5, false), row(0x01DA,  23,   6, false),
    row(0x00E5,  25,   7, false), row(0x006F,  28,   8, false), row(0x0036,  30,   9, false),
    row(0x001A,  33,  10, false), row(0x000D,  35,  11, false), row(0x0006,   9,  12, false),
    row(0x0003,  10,  13, false), row(0x0001,  12,  13, false), row(0x5A7F,  15,  15, true ),
    row(0x3F25,  36,  16, false), row(0x2CF2,  38,  17, false), row(0x207C,  39,  18, false),
    row(0x17B9,  40,  19, false), row(0x1182,  42,  20, false), row(0x0CEF,  43,  21, false),
    row(0x09A1,  45,  22, false), row(0x072F,  46,  23, false), row(0x055C,  48,  24, false),
    row(0x0406,  49,  25, false), row(0x0303,  51,  26, false), row(0x0240,  52,  27, false),
    row(0x01B1,  54,  28, false), row(0x0144,  56,  29, false), row(0x00F5,  57,  30, false),
    row(0x00B7,  59,  31, false), row(0x008A,  60,  32, false), row(0x0068,  62,  33, false),
    row(0x004E,  63,  34, false), row(0x003B,  32,  35, false), row(0x002C,  33,   9, false),
    row(0x5AE1,  37,  37, true ), row(0x484C,  64,  38, false), row(0x3A0D,  65,  39, false),
    row(0x2EF1,  67,  40, false), row(0x261F,  68,  41, false), row(0x1F33,  69,  42, false),
    row(0x19A8,  70,  43, false), row(0x1518,  72,  44, false), row(0x1177,  73,  45, false),
    row(0x0E74,  74,  46, false), row(0x0BFB,  75,  47, false), row(0x09F8,  77,  48, false),
    row(0x0861,  78,  49, false), row(0x0706,  79,  50, false), row(0x05CD,  48,  51, false),
    row(0x04DE,  50,  52, false), row(0x040F,  50,  53, false), row(0x0363,  51,  54, false),
    row(0x02D4,  52,  55, false), row(0x025C,  53,  56, false), row(0x01F8,  54,  57, false),
    row(0x01A4,  55,  58, false), row(0x0160,  56,  59, false), row(0x0125,  57,  60, false),
    row(0x00F6,  58,  61, false), row(0x00CB,  59,  62, false), row(0x00AB,  61,  63, false),
    row(0x008F,  61,  32, false), row(0x5B12,  65,  65, true ), row(0x4D04,  80,  66, false),
    row(0x412C,  81,  67, false), row(0x37D8,  82,  68, false), row(0x2FE8,  83,  69, false),
    row(0x293C,  84,  70, false), row(0x2379,  86,  71, false), row(0x1EDF,  87,  72, false),
    row(0x1AA9,  87,  73, false), row(0x174E,  72,  74, false), row(0x1424,  72,  75, false),
    row(0x119C,  74,  76, false), row(0x0F6B,  74,  77, false), row(0x0D51,  75,  78, false),
    row(0x0BB6,  77,  79, false), row(0x0A40,  77,  48, false), row(0x5832,  80,  81, true ),
    row(0x4D1C,  88,  82, false), row(0x438E,  89,  83, false), row(0x3BDD,  90,  84, false),
    row(0x34EE,  91,  85, false), row(0x2EAE,  92,  86, false), row(0x299A,  93,  87, false),
    row(0x2516,  86,  71, false), row(0x5570,  88,  89, true ), row(0x4CA9,  95,  90, false),
    row(0x44D9,  96,  91, false), row(0x3E22,  97,  92, false), row(0x3824,  99,  93, false),
    row(0x32B4,  99,  94, false), row(0x2E17,  93,  86, false), row(0x56A8,  95,  96, true ),
    row(0x4F46, 101,  97, false), row(0x47E5, 102,  98, false), row(0x41CF, 103,  99, false),
    row(0x3C3D, 104, 100, false), row(0x375E,  99,  93, false), row(0x5231, 105, 102, false),
    row(0x4C0F, 106, 103, false), row(0x4639, 107, 104, false), row(0x415E, 103,  99, false),
    row(0x5627, 105, 106, true ), row(0x50E7, 108, 107, false), row(0x4B85, 109, 103, false),
    row(0x5597, 110, 109, false), row(0x504F, 111, 107, false), row(0x5A10, 110, 111, true ),
    row(0x5522, 112, 109, false), row(0x59EB, 112, 111, true ),
];

/// One adaptive context: a row of Table D.3 and the sense of the MPS.
///
/// D.2.7: "The statistics areas are initialized to an MPS sense of 0 and a Qe
/// index of zero as defined by Table D.3" — which is [`Default`].
///
/// That sentence is quoted rather than paraphrased because the paraphrase was
/// wrong here first: it read "start *with* an MPS sense of 0", and rule 8's
/// second pass against the document caught the preposition. A wrong word in a
/// citation is how a citation stops being checkable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QmContext {
    /// Row of Table D.3. Always below 113: every write comes from the table's
    /// own `Next_Index_LPS` / `Next_Index_MPS` columns.
    index: u8,
    /// The more probable symbol, 0 or 1.
    mps: u8,
}

/// The QM decoder over one entropy-coded segment (T.81 D.2).
pub struct QmDecoder<'a> {
    data: &'a [u8],
    /// The next byte `Byte_in` would read. D.2.7's `BP = BPST - 1` followed by
    /// `Byte_in`'s `BP = BP + 1` is this starting at zero.
    next: usize,
    /// `C`, whole. `C >> 16` is D.2.3's `Cx`; bits 8 to 15 are the "b" bits of
    /// C-low that a byte is inserted into.
    c: u32,
    /// `A`, the probability interval. `X'10000'` at `Initdec` — D.2.7's NOTE
    /// says a 16-bit implementation writes zero and means exactly this, and
    /// Table K.7's first row prints `0000` for the same reason.
    a: u32,
    /// `CT`, the count of bits left in C-low.
    ct: i32,
    /// Set once Figure D.21 detected a marker, or the data ran out.
    ///
    /// D.21 says to "Adjust BP ... so that 0-bytes will be fed to the decoder
    /// until decoding is complete", and suggests pointing `BP` at the byte
    /// before the marker so the detection repeats and nothing is ever added to
    /// `C`. A flag is that loop collapsed: `Byte_in` adds nothing and
    /// [`QmDecoder::position`] keeps naming the `X'FF'`, which is what a
    /// caller that has to carry on parsing needs.
    marker: bool,
}

impl<'a> QmDecoder<'a> {
    /// `Initdec` (T.81 D.2.7, Figure D.22).
    #[must_use]
    pub fn new(data: &'a [u8]) -> QmDecoder<'a> {
        let mut d = QmDecoder {
            data,
            next: 0,
            c: 0,
            a: 0x1_0000,
            ct: 0,
            marker: false,
        };
        d.byte_in();
        d.c <<= 8;
        d.byte_in();
        d.c <<= 8;
        d.ct = 0;
        d
    }

    /// How far into the data the coder has read.
    ///
    /// At a detected marker this is the index of its `X'FF'`, because D.21
    /// leaves `BP` before the marker rather than past it.
    #[must_use]
    pub fn position(&self) -> usize {
        self.next.min(self.data.len())
    }

    /// Whether the coder ran off the end of the segment instead of stopping
    /// at a marker.
    ///
    /// The two are the same condition inside `Byte_in` — both stop it feeding
    /// `C` — and they are not the same thing to a caller. A segment that ends
    /// at a marker is a segment that ended; one whose last byte is simply the
    /// last byte is truncated, and JPEG's scan loop reports that as damage.
    #[must_use]
    pub fn overran(&self) -> bool {
        self.marker && self.next >= self.data.len()
    }

    /// `Byte_in` (Figure D.20) with `Unstuff_0` (Figure D.21) folded in.
    ///
    /// The two are one function here because `Unstuff_0` is called from
    /// nowhere else and its outcomes are the outcomes of reading a byte: a
    /// plain byte is inserted, an `X'FF' X'00'` pair inserts the `X'FF'` and
    /// skips the stuffed zero, and an `X'FF'` followed by anything else is the
    /// marker that ends the segment.
    fn byte_in(&mut self) {
        if self.marker {
            // D.21's "Adjust BP": nothing is added to C from here on, which is
            // what "0-bytes will be fed to the decoder" means.
            return;
        }
        match self.data.get(self.next) {
            // Running out is the same condition as a marker — the segment is
            // over and every further Byte_in contributes nothing. That is the
            // whole of this coder's truncation policy (ruling 1).
            None => self.marker = true,
            Some(&0xFF) => match self.data.get(self.next + 1) {
                Some(&0x00) => {
                    self.c |= 0xFF00;
                    self.next += 2;
                }
                _ => self.marker = true,
            },
            Some(&byte) => {
                self.c = self.c.wrapping_add(u32::from(byte) << 8);
                self.next += 1;
            }
        }
    }

    /// `Renorm_d` (T.81 D.2.6, Figure D.19).
    fn renorm(&mut self) {
        // `A` here is either `A - Qe` on a path that renormalises only when
        // the result is below X'8000' and above zero, or `Qe` itself, which is
        // at least 1 — so a set bit reaches X'8000' within sixteen turns. The
        // bound is belt and braces for ruling 1: an unreachable state must
        // still not hang.
        for _ in 0..32 {
            if self.ct == 0 {
                self.byte_in();
                self.ct = 8;
            }
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.a & 0x8000 != 0 {
                break;
            }
        }
    }

    /// `Estimate_Qe(S)_after_MPS` (T.81 D.1.5.3, Figure D.5).
    fn after_mps(cx: &mut QmContext, nmps: u8) {
        cx.index = nmps;
    }

    /// `Estimate_Qe(S)_after_LPS` (T.81 D.1.5.4, Figure D.6).
    fn after_lps(cx: &mut QmContext, nlps: u8, switch: bool) {
        if switch {
            cx.mps = 1 - cx.mps;
        }
        cx.index = nlps;
    }

    /// `Decode(S)` (T.81 D.2.4, Figure D.16): one decision against one context.
    pub fn decode(&mut self, cx: &mut QmContext) -> u8 {
        // `index` is only ever written from the table's own Next_Index
        // columns, so this cannot miss; a corrupt state decodes as the MPS
        // rather than panicking (ruling 1).
        let Some(&QeRow {
            qe,
            nlps,
            nmps,
            switch,
        }) = QE.get(cx.index as usize)
        else {
            return cx.mps;
        };
        let qe = u32::from(qe);

        self.a = self.a.wrapping_sub(qe);
        if (self.c >> 16) < self.a {
            if self.a & 0x8000 != 0 {
                return cx.mps;
            }
            // Cond_MPS_exchange(S) (Figure D.18). Neither register moves here:
            // only the sense of the symbol, and only when the MPS sub-interval
            // ended up the smaller of the two.
            let d = if self.a < qe {
                let d = 1 - cx.mps;
                QmDecoder::after_lps(cx, nlps, switch);
                d
            } else {
                let d = cx.mps;
                QmDecoder::after_mps(cx, nmps);
                d
            };
            self.renorm();
            d
        } else {
            // Cond_LPS_exchange(S) (Figure D.17). `Cx = Cx - A` happens before
            // `A = Qe`, in that order: what is subtracted is the MPS
            // sub-interval, which is the `A` the caller is standing on.
            self.c = self.c.wrapping_sub(self.a << 16);
            let d = if self.a < qe {
                let d = cx.mps;
                QmDecoder::after_mps(cx, nmps);
                d
            } else {
                let d = 1 - cx.mps;
                QmDecoder::after_lps(cx, nlps, switch);
                d
            };
            self.a = qe;
            self.renorm();
            d
        }
    }

    /// One decision against the fixed estimate `Qe = X'5A1D'`, `MPS = 0`.
    ///
    /// T.81 F.1.4.4.2 codes the sign of an AC coefficient this way, G.1.3.1 a
    /// DC successive-approximation bit, and G.1.3.3 the sign of a coefficient
    /// that has just become non-zero. "Fixed" is the point: the context is
    /// discarded after the decision instead of adapting, so every such
    /// decision is coded at approximately 0.5.
    ///
    /// Index 0 of Table D.3 *is* `X'5A1D'` with `MPS = 0`, so this is a fresh
    /// context rather than a special case inside [`QmDecoder::decode`].
    pub fn decode_fixed(&mut self) -> u8 {
        let mut fixed = QmContext::default();
        self.decode(&mut fixed)
    }

    /// Re-initialises at the next `RSTn` marker (T.81 E.2.4, F.2.4.4).
    ///
    /// A restart interval ends by flushing the coder and appending a marker,
    /// so the next interval starts with a fresh `Initdec` at the byte after
    /// it. Returns false when no `RSTn` follows, and then leaves the decoder
    /// at the end of the data rather than anywhere surprising.
    pub fn restart(&mut self) -> bool {
        // The search starts where the decoder stopped. In the marker case
        // `position` is the X'FF' itself, which is exactly where it wants to
        // begin; in every other case it is the first byte not yet consumed.
        let mut at = self.position();
        while at + 1 < self.data.len() {
            if self.data.get(at) == Some(&0xFF) {
                if let Some(&m) = self.data.get(at + 1) {
                    if (0xD0..=0xD7).contains(&m) {
                        *self = QmDecoder::new(self.data.get(at + 2..).unwrap_or_default());
                        return true;
                    }
                }
            }
            at += 1;
        }
        self.next = self.data.len();
        self.marker = true;
        false
    }
}

/// The QM *encoder* (T.81 D.1), for tests only.
///
/// **Nothing ships this and nothing is meant to.** This engine reads PDFs; it
/// does not write arithmetic JPEG, and `jpeg/encode.rs` excludes arithmetic
/// coding by name. It exists because the *models* in `jpeg/arith.rs` need
/// fixtures and T.81 publishes no arithmetic-coded image, so the only way to
/// put a hand-derived decision sequence in front of the decoder is to encode
/// it.
///
/// **What keeps that from being a self round trip.** Ruling 13 is precise: an
/// encoder written here agreeing with a decoder written here proves that the
/// two halves of one misunderstanding agree. So neither half rests on the
/// other. The decoder is pinned to K.4.1's published *decisions* and this
/// encoder to K.4.1's published *bytes* — Table K.7's output for the same 256
/// decisions, in [`tests::annex_k_4_1_encodes_to_the_published_bytes`]. Each
/// side answers to the standard on its own, and only then is the pair used to
/// carry a decision sequence from one end of a model to the other.
#[cfg(test)]
pub(crate) mod encoder {
    use super::{QeRow, QmContext, QE};

    /// `Initenc` through `Flush` (T.81 D.1.7, D.1.4 to D.1.6, D.1.8).
    pub(crate) struct QmEncoder {
        /// The entropy-coded segment so far. D.1.6's `B` is its last byte and
        /// `BP` its last index.
        out: Vec<u8>,
        /// `C`, in D.1.3's layout: a carry bit, eight "b" bits that leave as a
        /// byte, three spacer bits, and sixteen fractional bits.
        c: u32,
        /// `A`, the probability interval.
        a: u32,
        /// `CT`, the count of shifts before the next `Byte_out`.
        ct: i32,
        /// `ST`, the count of `X'FF'` bytes stacked waiting for a carry.
        st: u32,
    }

    impl QmEncoder {
        /// `Initenc` (Figure D.12). `CT = 11` because with `A = X'10000'`
        /// three spacer bits plus eight output bits must fill before the first
        /// byte leaves.
        pub(crate) fn new() -> QmEncoder {
            QmEncoder {
                out: Vec::new(),
                c: 0,
                a: 0x1_0000,
                ct: 11,
                st: 0,
            }
        }

        /// `Code_1(S)` and `Code_0(S)` (Figures D.1 and D.2), which differ only
        /// in which sense of the MPS sends them down which path.
        pub(crate) fn encode(&mut self, cx: &mut QmContext, d: u8) {
            let Some(&QeRow {
                qe,
                nlps,
                nmps,
                switch,
            }) = QE.get(cx.index as usize)
            else {
                return;
            };
            let qe = u32::from(qe);

            if d == cx.mps {
                // Code_MPS(S) (Figure D.4): the MPS takes the lower
                // sub-interval unless it is the smaller of the two.
                self.a -= qe;
                if self.a & 0x8000 != 0 {
                    return;
                }
                if self.a < qe {
                    self.c += self.a;
                    self.a = qe;
                }
                cx.index = nmps;
                self.renorm();
            } else {
                // Code_LPS(S) (Figure D.3): the LPS takes the upper
                // sub-interval unless the conditional exchange has swapped
                // which symbol each one means.
                self.a -= qe;
                if self.a >= qe {
                    self.c += self.a;
                    self.a = qe;
                }
                if switch {
                    cx.mps = 1 - cx.mps;
                }
                cx.index = nlps;
                self.renorm();
            }
        }

        /// One decision against the fixed estimate, the mirror of
        /// [`super::QmDecoder::decode_fixed`].
        pub(crate) fn encode_fixed(&mut self, d: u8) {
            let mut fixed = QmContext::default();
            self.encode(&mut fixed, d);
        }

        /// `Renorm_e` (Figure D.7). The shift comes *before* the byte here and
        /// after it in `Renorm_d`, which is the kind of asymmetry that makes
        /// pinning both ends to published data worth the trouble.
        fn renorm(&mut self) {
            loop {
                self.a <<= 1;
                self.c <<= 1;
                self.ct -= 1;
                if self.ct == 0 {
                    self.byte_out();
                    self.ct = 8;
                }
                if self.a & 0x8000 != 0 {
                    break;
                }
            }
        }

        /// `Byte_out` (Figure D.8) with `Stuff_0`, `Output_stacked_zeros` and
        /// `Output_stacked_X'FF's` (Figures D.9 to D.11) folded in.
        fn byte_out(&mut self) {
            let t = self.c >> 19;
            if t > 0xFF {
                // A carry. It is added to the byte already written, any X'FF's
                // stacked behind it have become zeros, and a carry that turns
                // the previous byte into X'FF' needs its own stuffed zero.
                if let Some(last) = self.out.last_mut() {
                    *last = last.wrapping_add(1);
                }
                if self.out.last() == Some(&0xFF) {
                    self.out.push(0x00);
                }
                for _ in 0..self.st {
                    self.out.push(0x00);
                }
                self.st = 0;
                self.out.push((t & 0xFF) as u8);
            } else if t == 0xFF {
                // The byte cannot be written until a later carry is resolved.
                self.st += 1;
            } else {
                for _ in 0..self.st {
                    self.out.push(0xFF);
                    self.out.push(0x00);
                }
                self.st = 0;
                self.out.push((t & 0xFF) as u8);
            }
            self.c &= 0x7_FFFF;
        }

        /// `Flush` (Figure D.13) with `Clear_final_bits` (Figure D.14) and
        /// `Discard_final_zeros` (Figure D.15).
        ///
        /// **The last of those is optional in the clause and not optional
        /// here.** D.1.8 says trailing zero bytes "may, optionally, be
        /// discarded", which read like something to leave out -- and leaving
        /// it out put one byte of `X'00'` on the end of K.4.1's segment, where
        /// Table K.7's `Flush` row lists `X'F6'` as the last completed byte
        /// and nothing after it. The published bytes decide it.
        pub(crate) fn finish(mut self) -> Vec<u8> {
            // Clear_final_bits: zero as many low bits of C as possible without
            // pointing outside the final interval.
            let t = (self.c + self.a - 1) & 0xFFFF_0000;
            self.c = if t < self.c { t + 0x8000 } else { t };

            self.c <<= self.ct;
            self.byte_out();
            self.c <<= 8;
            self.byte_out();

            // Any X'FF' still stacked belongs in the segment, each with its
            // stuffed zero, or the marker that follows it would be ambiguous.
            for _ in 0..self.st {
                self.out.push(0xFF);
                self.out.push(0x00);
            }

            // Discard_final_zeros (Figure D.15): drop trailing zeros, then put
            // one back if what is now last is an X'FF' -- that zero was
            // stuffed, and D.1.8 says a stuffed zero "shall not be discarded".
            while self.out.last() == Some(&0x00) {
                self.out.pop();
            }
            if self.out.last() == Some(&0xFF) {
                self.out.push(0x00);
            }
            self.out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T.81 K.4.1's "actual compressed data sequence", read twice: once out of
    /// the text layer of PDF page 163 of the W3C's copy and once off `tpdf
    /// render --fonts`'s picture of that page. The two readings agree.
    const K_4_1_COMPRESSED: [u8; 32] = [
        0x65, 0x5B, 0x51, 0x44, 0xF7, 0x96, 0x9D, 0x51, 0x78, 0x55, 0xBF, 0xFF, 0x00, 0xFC, 0x51,
        0x84, 0xC7, 0xCE, 0xF9, 0x39, 0x00, 0x28, 0x7D, 0x46, 0x70, 0x8E, 0xCB, 0xC0, 0xF6, 0xFF,
        0xD9, 0x00,
    ];

    /// T.81 K.4.1's 256-bit test sequence, read the same two ways.
    const K_4_1_DECISIONS: [u8; 32] = [
        0x00, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0xC0, 0x03, 0x52, 0x87, 0x2A, 0xAA, 0xAA, 0xAA,
        0xAA, 0x82, 0xC0, 0x20, 0x00, 0xFC, 0xD7, 0x9E, 0xF6, 0x74, 0xEA, 0xAB, 0xF7, 0x69, 0x7E,
        0xE7, 0x4C,
    ];

    fn decode_all() -> (QmDecoder<'static>, [u8; 32]) {
        let mut decoder = QmDecoder::new(&K_4_1_COMPRESSED);
        let mut cx = QmContext::default();
        let mut got = [0u8; 32];
        for bit in 0..256usize {
            let d = decoder.decode(&mut cx);
            assert!(d <= 1, "decision {bit} was neither 0 nor 1: {d}");
            got[bit / 8] |= d << (7 - bit % 8);
        }
        (decoder, got)
    }

    /// **The adjudicator.** T.81 K.4.1 publishes both halves of this: the 32
    /// bytes go in and the 256 decisions come out, and this repository wrote
    /// neither of them.
    ///
    /// The sequence "is structured to test many of the encoder and decoder
    /// paths" in K.4.1's own words, and what it reaches is counted by the two
    /// tests below rather than taken on trust.
    #[test]
    fn annex_k_4_1_decodes_to_the_published_test_sequence() {
        let (_, got) = decode_all();
        assert_eq!(
            got, K_4_1_DECISIONS,
            "T.81 K.4.1: the published bytes did not decode to the published \
             256-bit test sequence"
        );
    }

    /// **The adjudicator, from the other side.** K.4.1's 256 decisions encode
    /// to the entropy-coded segment K.4.1 prints, and then T.81's own
    /// `X'FFD9'` appended to it is the published sequence.
    ///
    /// Where the segment ends is read off Table K.7 rather than inferred:
    /// sheet 7's last row is labelled `Flush:` and lists `X'F6'` in the `B`
    /// column, with `FFD9` on the line below it and nothing else. So the
    /// segment is the 29 bytes ending at `X'F6'`, the marker is the next two,
    /// and the `X'00'` the printed sequence ends with is the pad that makes
    /// K.4.1's "a total of 256 bits are output" come out even.
    ///
    /// Without this, [`encoder`] would be an implementation nothing checks and
    /// the model fixtures it builds for `jpeg/arith.rs` would be a round trip
    /// between two guesses (ruling 13).
    #[test]
    fn annex_k_4_1_encodes_to_the_published_bytes() {
        let mut encoder = encoder::QmEncoder::new();
        let mut cx = QmContext::default();
        for bit in 0..256usize {
            let d = (K_4_1_DECISIONS[bit / 8] >> (7 - bit % 8)) & 1;
            encoder.encode(&mut cx, d);
        }

        let mut segment = encoder.finish();
        assert_eq!(segment, K_4_1_COMPRESSED[..29], "T.81 Table K.7's output");

        segment.extend_from_slice(&[0xFF, 0xD9]);
        assert_eq!(segment, K_4_1_COMPRESSED[..31]);
        assert_eq!(K_4_1_COMPRESSED[31], 0x00, "the pad to 256 bits");
    }

    /// How much of Table D.3 the adjudicator actually reaches. Pinned so the
    /// module header's claim cannot drift from the fixture, and so a table
    /// edit that moves the walk shows up as a number rather than silently.
    #[test]
    fn annex_k_4_1_walks_a_counted_part_of_table_d_3() {
        let mut decoder = QmDecoder::new(&K_4_1_COMPRESSED);
        let mut cx = QmContext::default();

        let mut seen = [false; 113];
        let mut mps_flipped = 0usize;
        for _ in 0..256 {
            seen[cx.index as usize] = true;
            let before = cx.mps;
            decoder.decode(&mut cx);
            mps_flipped += usize::from(cx.mps != before);
        }

        assert!(seen[0], "index 0 is where Initdec starts every context");
        assert_eq!(seen.iter().filter(|&&s| s).count(), K_4_1_ROWS_WALKED);
        assert_eq!(mps_flipped, K_4_1_MPS_FLIPS);
    }

    /// Measured from the fixture, not chosen: K.4.1's 256 decisions walk 26 of
    /// Table D.3's 113 rows and invert the sense of the MPS three times. So
    /// the adjudicator covers **26 rows**, and the other 87 rest on the
    /// double reading above them.
    const K_4_1_ROWS_WALKED: usize = 26;
    const K_4_1_MPS_FLIPS: usize = 3;

    /// The published bytes exercise both of D.20/D.21's `X'FF'` outcomes: the
    /// pair `FF 00` at offset 11, which inserts the `X'FF'` and skips the
    /// stuffed zero, and the `FF D9` at offset 29, which is the marker that
    /// ends the segment. So the two rules this coder does *not* share with
    /// `mq.rs` are both under the adjudicator rather than beside it.
    ///
    /// K.4.1 also says "the coded bit count is 240", which is 30 bytes; with
    /// the stuffed `X'00'` that is the 31 bytes before `X'D9'`, and the marker
    /// D.21 stops at is the `X'FF'` at 29.
    #[test]
    fn annex_k_4_1_takes_both_stuffing_paths() {
        assert_eq!(K_4_1_COMPRESSED[11..13], [0xFF, 0x00]);
        assert_eq!(K_4_1_COMPRESSED[29..31], [0xFF, 0xD9]);

        let (decoder, _) = decode_all();
        assert!(
            !decoder.overran(),
            "the coder ran off the end instead of stopping at X'FFD9'"
        );
        assert_eq!(
            decoder.position(),
            29,
            "D.21 leaves BP before the marker, not past it"
        );

        // And the other way round, so the distinction is pinned from both
        // sides: the same bytes with the marker cut off do overrun.
        let mut truncated = QmDecoder::new(&K_4_1_COMPRESSED[..29]);
        let mut cx = QmContext::default();
        for _ in 0..256 {
            truncated.decode(&mut cx);
        }
        assert!(truncated.overran());
    }

    /// Table D.3's own shape, which no fixture reaches every row of.
    #[test]
    fn table_d_3_is_a_closed_113_state_machine() {
        assert_eq!(QE.len(), 113);
        for (index, r) in QE.iter().enumerate() {
            assert!(
                (r.nlps as usize) < QE.len(),
                "row {index}: Next_Index_LPS {} is off the table",
                r.nlps
            );
            assert!(
                (r.nmps as usize) < QE.len(),
                "row {index}: Next_Index_MPS {} is off the table",
                r.nmps
            );
            // D.1.5.1: Qe is a 15-bit integer, and D.2.4's comparisons assume
            // the MPS sub-interval can be non-empty, so Qe stays below X'8000'.
            assert!(
                r.qe > 0 && r.qe < 0x8000,
                "row {index}: Qe {:#06X} is out of range",
                r.qe
            );
        }
        // The seven Switch_MPS rows are the heads of D.1.5.1's "learning"
        // sequences; every other row leaves the sense of the MPS alone.
        assert_eq!(QE.iter().filter(|r| r.switch).count(), 10);
    }

    /// The two coders' tables meet in one value. This is the module header's
    /// claim that `mq.rs` is not reusable, made an assertion — and it is here
    /// in this exact form because the header first said "disjoint", which is
    /// off by one row.
    #[test]
    fn the_two_qe_tables_share_only_the_smallest_estimate() {
        let theirs = crate::mq::table_e_1_qe_values();
        assert_eq!(theirs.len(), 47);
        let shared: Vec<u16> = QE
            .iter()
            .map(|r| r.qe)
            .filter(|qe| theirs.contains(qe))
            .collect();
        assert_eq!(
            shared,
            [0x0001],
            "the overlap between Table D.3 and Table E.1 moved"
        );
    }

    /// Ruling 1, for the byte-level paths a fixture cannot reach: a segment
    /// that stops in the middle, one that is empty, and one that is nothing
    /// but stuffing.
    #[test]
    fn no_input_panics_or_hangs() {
        for data in [
            &[][..],
            &[0xFF][..],
            &[0xFF, 0x00][..],
            &[0xFF, 0xD9][..],
            &[0x00][..],
            &K_4_1_COMPRESSED[..5],
            &K_4_1_COMPRESSED[..12],
        ] {
            let mut decoder = QmDecoder::new(data);
            let mut cx = QmContext::default();
            for _ in 0..1000 {
                decoder.decode(&mut cx);
                decoder.decode_fixed();
            }
            assert!(decoder.position() <= data.len());
        }
    }

    /// A restart re-runs `Initdec` after the `RSTn`, and a segment with no
    /// `RSTn` left says so rather than running off the end.
    #[test]
    fn restart_resumes_after_the_marker() {
        let mut data = Vec::new();
        data.extend_from_slice(&K_4_1_COMPRESSED[..4]);
        data.extend_from_slice(&[0xFF, 0xD0]);
        data.extend_from_slice(&K_4_1_COMPRESSED);

        let mut decoder = QmDecoder::new(&data);
        let mut cx = QmContext::default();
        for _ in 0..8 {
            decoder.decode(&mut cx);
        }
        assert!(decoder.restart());

        let mut cx = QmContext::default();
        let mut got = [0u8; 32];
        for bit in 0..256usize {
            got[bit / 8] |= decoder.decode(&mut cx) << (7 - bit % 8);
        }
        assert_eq!(got, K_4_1_DECISIONS);
        assert!(!decoder.restart());
    }
}
