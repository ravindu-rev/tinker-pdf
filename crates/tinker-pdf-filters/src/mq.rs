//! The MQ arithmetic decoder (ITU-T T.88 Annex E).
//!
//! A binary adaptive arithmetic coder. It carries no notion of what it is
//! coding: a caller supplies a *context* — one adaptive probability state —
//! per decision, and gets one bit back. Everything that makes a decision
//! predictable lives in how the caller chooses the context, which is why the
//! same coder serves JBIG2's generic regions (T.88 6.2) and, if
//! `docs/plans/gaps/18-jpx-decision.md` ever proceeds, JPEG 2000's code-block
//! coder.
//!
//! **This is a module of its own from the first commit, deliberately.** T.88
//! Annex E and T.800 Annex C are the same coder with different prose around
//! them, and the two consumers would otherwise each grow a copy. The
//! consequence is the test at the bottom: Annex H.2's published test sequence
//! is a **permanent** fixture rather than scaffolding, because a change made
//! for one codec that silently altered the other is exactly the failure a
//! shared module invites.
//!
//! # Which formulation
//!
//! T.88 states the decoder twice — Annex E in terms of the C register whole,
//! and Annex G in "software conventions" with C inverted. This follows Annex
//! E: `C` is a 32-bit register whose top sixteen bits are the `Chigh` the
//! figures compare against `Qe`, so the code reads next to E.3.2's flowcharts
//! rather than next to a transformation of them.
//!
//! # Bounded on any input
//!
//! Ruling 1. There is no input this can panic on and none it can loop on:
//! reading past the end of the data yields `0xFF`, which is what the marker
//! convention of E.3.4 asks for and what makes `BYTEIN` stop advancing; the
//! renormalisation loop shifts a register that is provably non-zero, so it
//! terminates in at most fifteen iterations, and carries a hard bound anyway.

/// One row of T.88 Table E.1: the LPS probability estimate, where the state
/// moves on each renormalisation, and whether the sense of the MPS flips.
#[derive(Clone, Copy)]
struct QeRow {
    qe: u16,
    nmps: u8,
    nlps: u8,
    switch: bool,
}

const fn row(qe: u16, nmps: u8, nlps: u8, switch: bool) -> QeRow {
    QeRow {
        qe,
        nmps,
        nlps,
        switch,
    }
}

/// T.88 Table E.1 — the 47-row Qe table, verbatim.
///
/// The table is the coder. An index off by one row anywhere in it produces a
/// decoder that decodes the first few decisions correctly and then diverges,
/// which looks like a corrupt file rather than like a wrong table; the Annex
/// H.2 test below is what distinguishes the two.
const QE: [QeRow; 47] = [
    row(0x5601, 1, 1, true),
    row(0x3401, 2, 6, false),
    row(0x1801, 3, 9, false),
    row(0x0AC1, 4, 12, false),
    row(0x0521, 5, 29, false),
    row(0x0221, 38, 33, false),
    row(0x5601, 7, 6, true),
    row(0x5401, 8, 14, false),
    row(0x4801, 9, 14, false),
    row(0x3801, 10, 14, false),
    row(0x3001, 11, 17, false),
    row(0x2401, 12, 18, false),
    row(0x1C01, 13, 20, false),
    row(0x1601, 29, 21, false),
    row(0x5601, 15, 14, true),
    row(0x5401, 16, 14, false),
    row(0x5101, 17, 15, false),
    row(0x4801, 18, 16, false),
    row(0x3801, 19, 17, false),
    row(0x3401, 20, 18, false),
    row(0x3001, 21, 19, false),
    row(0x2801, 22, 19, false),
    row(0x2401, 23, 20, false),
    row(0x2201, 24, 21, false),
    row(0x1C01, 25, 22, false),
    row(0x1801, 26, 23, false),
    row(0x1601, 27, 24, false),
    row(0x1401, 28, 25, false),
    row(0x1201, 29, 26, false),
    row(0x1101, 30, 27, false),
    row(0x0AC1, 31, 28, false),
    row(0x09C1, 32, 29, false),
    row(0x08A1, 33, 30, false),
    row(0x0521, 34, 31, false),
    row(0x0441, 35, 32, false),
    row(0x02A1, 36, 33, false),
    row(0x0221, 37, 34, false),
    row(0x0141, 38, 35, false),
    row(0x0111, 39, 36, false),
    row(0x0085, 40, 37, false),
    row(0x0049, 41, 38, false),
    row(0x0025, 42, 39, false),
    row(0x0015, 43, 40, false),
    row(0x0009, 44, 41, false),
    row(0x0005, 45, 42, false),
    row(0x0001, 45, 43, false),
    row(0x5601, 46, 46, false),
];

/// One adaptive probability state: a row of [`QE`] and the current sense of
/// the more probable symbol.
///
/// T.88 E.3.6 initialises every context to index 0 with MPS 0, which is what
/// [`Default`] gives — and it is what makes the *numbering* of contexts a free
/// choice for a caller as long as it is a bijection, since all states start
/// identical. The one place that stops being true is a pseudo-context whose
/// value the standard fixes (T.88 6.2.5.7's TPGDON), because there the
/// encoder and decoder have to agree on which slot a decision shares with
/// which neighbourhood.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MqContext {
    /// Row of T.88 Table E.1. Always below 47: every write comes from the
    /// table's own NMPS/NLPS columns.
    index: u8,
    /// The more probable symbol, 0 or 1.
    mps: u8,
}

/// An array of adaptive contexts, all starting at T.88 E.3.6's initial state.
///
/// JBIG2's generic-region templates index this by a value formed from the
/// pixels around the one being decoded, so its length is `1 << (template
/// bits)` — 65 536 at most. JPEG 2000 would use nineteen.
#[derive(Clone, Debug)]
pub struct MqContexts {
    states: Vec<MqContext>,
}

impl MqContexts {
    /// `len` contexts, each in the initial state.
    #[must_use]
    pub fn new(len: usize) -> MqContexts {
        MqContexts {
            states: vec![MqContext::default(); len],
        }
    }

    /// Number of contexts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Whether there are no contexts at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Returns every context to the initial state.
    ///
    /// T.88 6.2.5.7 resets a region's contexts between regions unless the
    /// segment says to retain them, so this is the reset rather than a fresh
    /// allocation: the array is up to 64 KB and a page may hold many regions.
    pub fn reset(&mut self) {
        for state in &mut self.states {
            *state = MqContext::default();
        }
    }
}

/// The MQ decoder over one span of bytes (T.88 E.3).
pub struct MqDecoder<'a> {
    data: &'a [u8],
    /// `BP` in E.3: the byte the coder is reading.
    bp: usize,
    /// `C`, whole. `C >> 16` is the figures' `Chigh`.
    c: u32,
    /// `A`, the interval.
    a: u32,
    /// `CT`, the count of bits left in the current byte.
    ct: i32,
}

impl<'a> MqDecoder<'a> {
    /// `INITDEC` (T.88 E.3.5).
    #[must_use]
    pub fn new(data: &'a [u8]) -> MqDecoder<'a> {
        let mut d = MqDecoder {
            data,
            bp: 0,
            c: 0,
            a: 0,
            ct: 0,
        };
        d.c = u32::from(d.byte_at(0)) << 16;
        d.byte_in();
        d.c <<= 7;
        d.ct -= 7;
        d.a = 0x8000;
        d
    }

    /// How far into the data the coder has read.
    ///
    /// A segment can hold a region's coded data followed by more segments, so
    /// a caller that has to carry on parsing needs this. It saturates at the
    /// end of the data, because past the end the coder stops advancing.
    #[must_use]
    pub fn position(&self) -> usize {
        self.bp.min(self.data.len())
    }

    /// The byte at `at`, or `0xFF` past the end.
    ///
    /// E.3.4 treats `0xFF` followed by a value above `0x8F` as a marker and
    /// feeds 1-bits forever without consuming anything. Reading the end of
    /// the data as `0xFF` puts a truncated stream on exactly that path, so it
    /// terminates instead of running off the buffer — which is the whole of
    /// this decoder's truncation policy.
    fn byte_at(&self, at: usize) -> u8 {
        self.data.get(at).copied().unwrap_or(0xFF)
    }

    /// `BYTEIN` (T.88 E.3.4).
    fn byte_in(&mut self) {
        if self.byte_at(self.bp) == 0xFF {
            if self.byte_at(self.bp + 1) > 0x8F {
                // A marker. The coder feeds 1-bits and never moves past it.
                self.c = self.c.wrapping_add(0xFF00);
                self.ct = 8;
            } else {
                self.bp += 1;
                self.c = self.c.wrapping_add(u32::from(self.byte_at(self.bp)) << 9);
                self.ct = 7;
            }
        } else {
            self.bp += 1;
            self.c = self.c.wrapping_add(u32::from(self.byte_at(self.bp)) << 8);
            self.ct = 8;
        }
    }

    /// `RENORMD` (T.88 E.3.3).
    fn renorm(&mut self) {
        // `A` is never zero here — it is either `A - Qe` with `A >= 0x8000`
        // and `Qe <= 0x5601`, or `Qe` itself, which is at least 1 — so the
        // loop shifts a set bit into 0x8000 within fifteen turns. The bound
        // is belt and braces for ruling 1: an unreachable state must still
        // not hang.
        for _ in 0..32 {
            if self.ct == 0 {
                self.byte_in();
            }
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.a & 0x8000 != 0 {
                break;
            }
        }
    }

    /// `DECODE` (T.88 E.3.2): one decision against one context.
    pub fn decode(&mut self, cx: &mut MqContext) -> u8 {
        // `index` is only ever written from the table's own NMPS/NLPS
        // columns, so this cannot miss; a corrupt state would decode as the
        // last row rather than panic.
        let Some(&QeRow {
            qe,
            nmps,
            nlps,
            switch,
        }) = QE.get(cx.index as usize)
        else {
            return cx.mps;
        };
        let qe = u32::from(qe);

        self.a = self.a.wrapping_sub(qe);
        if (self.c >> 16) < qe {
            // LPS_EXCHANGE (E.3.3). The interval collapses to Qe either way;
            // which symbol that is depends on whether the sub-interval left
            // for the MPS ended up the smaller of the two.
            let d = if self.a < qe {
                cx.index = nmps;
                cx.mps
            } else {
                let d = 1 - cx.mps;
                if switch {
                    cx.mps = d;
                }
                cx.index = nlps;
                d
            };
            self.a = qe;
            self.renorm();
            d
        } else {
            self.c -= qe << 16;
            if self.a & 0x8000 != 0 {
                return cx.mps;
            }
            // MPS_EXCHANGE (E.3.3).
            let d = if self.a < qe {
                let d = 1 - cx.mps;
                if switch {
                    cx.mps = d;
                }
                cx.index = nlps;
                d
            } else {
                cx.index = nmps;
                cx.mps
            };
            self.renorm();
            d
        }
    }

    /// One decision against `contexts[index]`.
    ///
    /// An index past the end cannot come from a template — the array is sized
    /// `1 << template bits` and the template forms fewer bits than that — so
    /// it is a caller error rather than a data condition. It yields 0 and
    /// leaves the coder untouched rather than panicking (ruling 1).
    pub fn decode_at(&mut self, contexts: &mut MqContexts, index: usize) -> u8 {
        let mut scratch = MqContext::default();
        let cx = contexts.states.get_mut(index).unwrap_or(&mut scratch);
        let mut state = *cx;
        let d = self.decode(&mut state);
        if let Some(slot) = contexts.states.get_mut(index) {
            *slot = state;
        }
        d
    }
}

/// The MQ *encoder* (T.88 E.3.7 and E.3.8), for tests only.
///
/// Nothing ships this: the engine reads PDFs, it does not write JBIG2. It
/// exists so a generic-region test can put a picture in and demand the same
/// picture back, which is the only way to exercise a template against a
/// bitmap the standard does not happen to publish.
///
/// **It is pinned against published data itself**, in
/// [`tests::annex_h2_re_encodes_to_the_published_bytes`]: Annex H.2's 256
/// decisions encode to Annex H.2's thirty bytes. Without that, a round-trip
/// would only prove the two halves of one misunderstanding agree.
#[cfg(test)]
pub(crate) mod encoder {
    use super::QE;

    pub(crate) struct MqEncoder {
        /// The output, with one scratch byte in front. E.3.8's `INITENC`
        /// leaves `BP` pointing one byte *before* the first it will write,
        /// and `BYTEOUT` reads that byte to decide whether to stuff — so the
        /// slot has to exist. It is dropped by [`MqEncoder::flush`].
        out: Vec<u8>,
        bp: usize,
        a: u32,
        c: u32,
        ct: i32,
        /// `(index, mps)` per context, the same state the decoder keeps.
        states: Vec<(u8, u8)>,
    }

    impl MqEncoder {
        /// `INITENC` (T.88 E.3.8).
        pub(crate) fn new(contexts: usize) -> MqEncoder {
            MqEncoder {
                out: vec![0],
                bp: 0,
                a: 0x8000,
                c: 0,
                ct: 12,
                states: vec![(0, 0); contexts.max(1)],
            }
        }

        /// `BYTEOUT` (T.88 E.3.8), including the carry that propagates into
        /// the byte already written and the 0xFF stuffing that keeps a marker
        /// from appearing by accident.
        fn byte_out(&mut self) {
            if self.out[self.bp] == 0xFF {
                self.push(self.c >> 20);
                self.c &= 0x000F_FFFF;
                self.ct = 7;
            } else if self.c < 0x0800_0000 {
                self.push(self.c >> 19);
                self.c &= 0x0007_FFFF;
                self.ct = 8;
            } else {
                self.out[self.bp] = self.out[self.bp].wrapping_add(1);
                if self.out[self.bp] == 0xFF {
                    self.c &= 0x07FF_FFFF;
                    self.push(self.c >> 20);
                    self.c &= 0x000F_FFFF;
                    self.ct = 7;
                } else {
                    self.push(self.c >> 19);
                    self.c &= 0x0007_FFFF;
                    self.ct = 8;
                }
            }
        }

        fn push(&mut self, byte: u32) {
            self.out.push(byte as u8);
            self.bp += 1;
        }

        /// `RENORME` (T.88 E.3.8).
        fn renorm(&mut self) {
            loop {
                self.a <<= 1;
                self.c <<= 1;
                self.ct -= 1;
                if self.ct == 0 {
                    self.byte_out();
                }
                if self.a & 0x8000 != 0 {
                    return;
                }
            }
        }

        /// `ENCODE` (T.88 E.3.7): one decision against one context.
        pub(crate) fn encode_at(&mut self, index: usize, decision: u8) {
            let Some(&(state, mps)) = self.states.get(index) else {
                return;
            };
            let row = QE[state as usize];
            let qe = u32::from(row.qe);
            self.a -= qe;
            if decision == mps {
                // CODEMPS.
                if self.a & 0x8000 == 0 {
                    if self.a < qe {
                        self.a = qe;
                    } else {
                        self.c = self.c.wrapping_add(qe);
                    }
                    self.states[index].0 = row.nmps;
                    self.renorm();
                } else {
                    self.c = self.c.wrapping_add(qe);
                }
            } else {
                // CODELPS.
                if self.a < qe {
                    self.c = self.c.wrapping_add(qe);
                } else {
                    self.a = qe;
                }
                if row.switch {
                    self.states[index].1 = 1 - mps;
                }
                self.states[index].0 = row.nlps;
                self.renorm();
            }
        }

        /// `FLUSH` (T.88 E.3.8), which ends every arithmetically coded
        /// segment in Annex H.1 with the same `FF AC` this produces.
        pub(crate) fn flush(mut self) -> Vec<u8> {
            // SETBITS.
            let temp = self.c.wrapping_add(self.a);
            self.c |= 0xFFFF;
            if self.c >= temp {
                self.c -= 0x8000;
            }
            self.c <<= self.ct;
            self.byte_out();
            self.c <<= self.ct;
            self.byte_out();
            if self.out[self.bp] != 0xFF {
                self.push(0xFF);
            }
            self.out.push(0xAC);
            self.out.remove(0);
            self.out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::encoder::MqEncoder;
    use super::*;

    /// ITU-T T.88 Annex H.2, "Test sequence for arithmetic coder" — the
    /// thirty compressed bytes.
    const H2_ENCODED: [u8; 30] = [
        0x84, 0xC7, 0x3B, 0xFC, 0xE1, 0xA1, 0x43, 0x04, 0x02, 0x20, 0x00, 0x00, 0x41, 0x0D, 0xBB,
        0x86, 0xF4, 0x31, 0x7F, 0xFF, 0x88, 0xFF, 0x37, 0x47, 0x1A, 0xDB, 0x6A, 0xDF, 0xFF, 0xAC,
    ];

    /// The 256 decisions they decode to, packed most significant bit first —
    /// thirty-two bytes, the same figure's test data.
    const H2_DECODED: [u8; 32] = [
        0x00, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0xC0, 0x03, 0x52, 0x87, 0x2A, 0xAA, 0xAA, 0xAA,
        0xAA, 0x82, 0xC0, 0x20, 0x00, 0xFC, 0xD7, 0x9E, 0xF6, 0xBF, 0x7F, 0xED, 0x90, 0x4F, 0x46,
        0xA3, 0xBF,
    ];

    /// **Permanent, not scaffolding.** The plan's risk table names the
    /// failure this guards: the coder is shared, so a change made for JPEG
    /// 2000 that shifted a row of the Qe table or reordered an exchange would
    /// otherwise break JBIG2 in silence — a wrong table still decodes, it
    /// just decodes something else.
    ///
    /// Everything is pinned at once by these thirty bytes: the table, both
    /// exchange procedures, `BYTEIN`'s marker rule, `INITDEC`'s seven-bit
    /// shift and the renormalisation. Two hundred and fifty-six decisions all
    /// on one context is enough to walk a long way up and down the state
    /// machine, so a single wrong row diverges well before the end.
    #[test]
    fn annex_h2_test_sequence_reproduces_exactly() {
        let mut decoder = MqDecoder::new(&H2_ENCODED);
        let mut cx = MqContext::default();
        let mut out = [0u8; 32];
        for bit in 0..256usize {
            let d = decoder.decode(&mut cx);
            assert!(d <= 1, "a decision is one bit");
            out[bit / 8] |= d << (7 - (bit % 8));
        }
        assert_eq!(
            out, H2_DECODED,
            "T.88 Annex H.2 did not reproduce; the Qe table or an exchange is wrong"
        );
    }

    /// The same sequence through the context-array entry point, which is how
    /// every real caller reaches the coder.
    #[test]
    fn annex_h2_through_the_context_array() {
        let mut decoder = MqDecoder::new(&H2_ENCODED);
        let mut contexts = MqContexts::new(1 << 16);
        let mut out = [0u8; 32];
        for bit in 0..256usize {
            let d = decoder.decode_at(&mut contexts, 0);
            out[bit / 8] |= d << (7 - (bit % 8));
        }
        assert_eq!(out, H2_DECODED);
    }

    /// The encoder, against the same published data, from the other side.
    ///
    /// This is what stops a generic-region round-trip from being circular.
    /// An encoder written to match a wrong decoder would round-trip
    /// perfectly and reproduce nothing; these thirty bytes are the standard's
    /// and they leave it nowhere to hide — the carry propagation, the 0xFF
    /// stuffing and `FLUSH`'s trailing `FF AC` are all in them.
    #[test]
    fn annex_h2_re_encodes_to_the_published_bytes() {
        let mut encoder = MqEncoder::new(1);
        for bit in 0..256usize {
            let d = (H2_DECODED[bit / 8] >> (7 - (bit % 8))) & 1;
            encoder.encode_at(0, d);
        }
        assert_eq!(
            encoder.flush(),
            H2_ENCODED.to_vec(),
            "T.88 Annex H.2 did not re-encode; the test encoder is not the \
             standard's and no round-trip built on it means anything"
        );
    }

    /// Every NMPS and NLPS names a row that exists, and the three rows with
    /// SWITCH set are the ones T.88 Table E.1 marks.
    #[test]
    fn qe_table_is_well_formed() {
        assert_eq!(QE.len(), 47);
        for (i, r) in QE.iter().enumerate() {
            assert!((r.nmps as usize) < QE.len(), "row {i} NMPS out of range");
            assert!((r.nlps as usize) < QE.len(), "row {i} NLPS out of range");
            assert!(r.qe > 0 && r.qe <= 0x5601, "row {i} Qe out of range");
        }
        let switches: Vec<usize> = QE
            .iter()
            .enumerate()
            .filter(|(_, r)| r.switch)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(switches, vec![0, 6, 14]);
    }

    /// Ruling 1: no input makes the coder panic, hang or read past its data.
    #[test]
    fn empty_and_truncated_input_terminate() {
        for data in [&[][..], &[0xFF][..], &[0xFF, 0xFF][..], &[0x00][..]] {
            let mut decoder = MqDecoder::new(data);
            let mut cx = MqContext::default();
            for _ in 0..4096 {
                let _ = decoder.decode(&mut cx);
            }
            assert!(decoder.position() <= data.len());
        }
    }

    #[test]
    fn out_of_range_context_index_is_inert() {
        let mut decoder = MqDecoder::new(&H2_ENCODED);
        let mut contexts = MqContexts::new(4);
        assert_eq!(decoder.decode_at(&mut contexts, 99), 0);
        assert_eq!(contexts.len(), 4);
        assert!(!contexts.is_empty());
    }

    #[test]
    fn reset_returns_contexts_to_the_initial_state() {
        let mut decoder = MqDecoder::new(&H2_ENCODED);
        let mut contexts = MqContexts::new(2);
        for _ in 0..64 {
            let _ = decoder.decode_at(&mut contexts, 0);
        }
        assert_ne!(contexts.states[0], MqContext::default());
        contexts.reset();
        assert_eq!(contexts.states[0], MqContext::default());
    }
}
