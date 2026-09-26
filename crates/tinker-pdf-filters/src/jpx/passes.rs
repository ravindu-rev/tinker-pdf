//! T.800 Tables D.8 and D.9: which coding passes are coded raw, and which
//! ones terminate.
//!
//! Two of Table A.19's code-block styles are answered from here, and this
//! module exists because they are **two capabilities over one mechanism**
//! rather than one change or two unrelated ones:
//!
//! - `BYPASS` (Table A.19 bit 0, "selective arithmetic coding bypass",
//!   D.6) makes some passes read raw bits instead of MQ decisions. That is a
//!   tier-1 change and nothing else can do it.
//! - `TERMALL` (Table A.19 bit 2, which Table A.19 itself calls "termination
//!   on each coding pass"; D.4 and Table D.8) makes every pass its own
//!   codeword segment. It introduces no new way of reading a decision.
//!
//! What they share is that both put **more than one codeword segment** in a
//! code-block's contribution to a packet, which is B.10.7.2, and both
//! therefore need tier-1 to start a fresh reader at each segment. That shared
//! part is one mechanism; the raw reader on top of it is only `BYPASS`'s.
//!
//! The note this module replaced said the pair was "not a tier-1 change".
//! **Half of that was wrong.** `TERMALL` really is only about where bytes
//! start; `BYPASS` changes what a pass *reads*, which is squarely tier-1 —
//! D.6's own sentence is "the bits that would have been returned from the
//! arithmetic coder are instead returned directly from the bit stream".
//!
//! # Pass numbering
//!
//! `i` counts coding passes from zero over the whole code-block, in the order
//! [`super::tier1::pass_at`] walks them: pass 0 is the most significant
//! bit-plane's cleanup pass, and every plane below it contributes a
//! significance propagation, a magnitude refinement and a cleanup pass in
//! that order. So `i % 3` is 0 for cleanup, 1 for significance propagation
//! and 2 for magnitude refinement, and bit-plane number `p` (Table D.9 counts
//! these from 1) holds passes `3p - 5`, `3p - 4` and `3p - 3` for `p > 1`.

/// D.6's boundary: the first coding pass index that `BYPASS` may code raw.
///
/// **Transcribed from two statements that count different things**, which is
/// why the value is named rather than spelled inline. D.6 opens with
/// "bypassing the arithmetic coder for the significance propagation pass and
/// magnitude refinement coding passes **starting in the fifth significant
/// bit-plane** of the code-block", and the same clause later says "Starting
/// with the **fourth** significance propagation and magnitude refinement
/// coding passes". Those agree: bit-planes 2, 3, 4 and 5 carry the first,
/// second, third and fourth significance propagation passes, so the fourth
/// one is bit-plane 5's, and Table D.9 prints bit-plane 5's significance
/// propagation row as the first `raw` row. Reading either sentence as
/// counting the other's units puts the boundary three passes out, which
/// decodes as a picture.
///
/// With pass 0 the first bit-plane's cleanup pass, bit-planes 2, 3 and 4
/// carry passes 1..=9, so bit-plane 5's significance propagation pass is
/// pass 10.
pub(crate) const FIRST_BYPASS_PASS: u32 = 10;

/// D.9's last always-arithmetic cleanup pass, by index.
///
/// Table D.9 gives bit-plane 4's cleanup pass as "AC, terminate" and every
/// cleanup pass below it as plain "AC"; bit-plane 4's cleanup is pass 9. The
/// clause says the same thing in prose — "The cleanup coding passes continue
/// to receive compressed image data directly from the arithmetic coder and
/// are always terminated" — so from here down a cleanup pass ends a segment.
const FIRST_TERMINATED_CLEANUP: u32 = FIRST_BYPASS_PASS - 1;

/// How one code-block's coding passes divide into codeword segments, and
/// which of them are raw.
///
/// The two flags are Table A.19's bits 0 and 2 for the tile-component this
/// code-block belongs to, after A.6.2's COC-over-COD precedence has been
/// resolved. Both default to clear, which is the single-segment,
/// all-arithmetic case the decoder had before either was implemented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Schedule {
    /// Table A.19 bit 0, D.6.
    pub(crate) bypass: bool,
    /// Table A.19 bit 2, D.4 and Table D.8.
    pub(crate) terminate_all: bool,
}

impl Schedule {
    pub(crate) const fn new(bypass: bool, terminate_all: bool) -> Schedule {
        Schedule {
            bypass,
            terminate_all,
        }
    }

    /// D.6: whether coding pass `i` is read as raw bits rather than as MQ
    /// decisions.
    ///
    /// Cleanup passes never are — "The cleanup coding passes continue to
    /// receive compressed image data directly from the arithmetic coder" —
    /// so only the significance propagation and magnitude refinement passes
    /// from [`FIRST_BYPASS_PASS`] on are raw, and only when the style bit is
    /// set.
    pub(crate) const fn raw(self, i: u32) -> bool {
        self.bypass && i >= FIRST_BYPASS_PASS && i % 3 != 0
    }

    /// Tables D.8 and D.9: whether coding pass `i` ends a codeword segment.
    ///
    /// This is B.10.7.2's set `T` — "the set of indices of terminated coding
    /// passes included for the code-block in the packet as indicated in
    /// Tables D.8 and D.9" — as a predicate. It deliberately does **not**
    /// include B.10.7.2's next sentence, "If the index final coding pass
    /// included in the packet is not a member of T, then it is added to T":
    /// that one closes a segment because the *packet* ended rather than
    /// because the coder terminated, and conflating the two is what would
    /// make a decoder restart its MQ decoder in the middle of a segment that
    /// merely spans two layers. [`super::tier2`] applies it separately and
    /// records which of the two closed each segment.
    ///
    /// The three cases:
    ///
    /// - `TERMALL` set: every pass, whatever else is set. Table D.8's second
    ///   column is "AC, terminate" on every row, and D.6 says it again for
    ///   the bypass case — "If termination on each coding pass is selected
    ///   (see A.6.1 and A.6.2), then every pass is terminated (including
    ///   both raw passes)".
    /// - `BYPASS` set alone: Table D.9's "terminate" rows, which are the
    ///   magnitude refinement passes that are raw and every cleanup pass from
    ///   bit-plane 4 down. A raw significance propagation pass is **not**
    ///   terminated — Table D.9 prints it as plain "raw" — so it shares a
    ///   segment with the magnitude refinement pass after it.
    /// - Neither: no pass terminates here. Table D.8's left column does
    ///   terminate one pass — its last row is the code-block's *final*
    ///   cleanup pass, "AC, terminate" — and this predicate deliberately
    ///   leaves it out, because it is given `i` alone and cannot know which
    ///   `i` is last. Nothing observes the omission: a packet that includes a
    ///   code-block's final pass has it as its own last included pass too, so
    ///   B.10.7.2 adds it to `T` anyway and `K` is the same either way. The
    ///   only difference is [`super::tier2::Segment::terminated`] on a
    ///   segment that no later contribution can reach, since the code-block
    ///   has no passes left to contribute.
    pub(crate) const fn terminates(self, i: u32) -> bool {
        if self.terminate_all {
            return true;
        }
        if !self.bypass {
            return false;
        }
        match i % 3 {
            // Cleanup, from bit-plane 4's down.
            0 => i >= FIRST_TERMINATED_CLEANUP,
            // Magnitude refinement, once it is raw.
            2 => i >= FIRST_BYPASS_PASS,
            // Significance propagation: raw but not terminated.
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Schedule, FIRST_BYPASS_PASS};

    /// Table D.8, both columns, as the standard prints them.
    ///
    /// The left column ("Termination only on last pass") is every pass plain
    /// `AC` except its last row, the final cleanup pass, which is
    /// "AC, terminate"; the right ("Termination on every pass") is every pass
    /// "AC, terminate". Neither column has a raw row, because Table D.8 is
    /// the no-bypass table — D.4's own sentence above it is "In normal
    /// operation (not selective arithmetic coding bypass)".
    ///
    /// The left column's one terminated row is not asserted here, and
    /// [`Schedule::terminates`] says why: it is the code-block's last pass,
    /// which B.10.7.2 adds to `T` regardless.
    #[test]
    fn table_d8_both_columns() {
        let plain = Schedule::new(false, false);
        let termall = Schedule::new(false, true);
        for i in 0..40 {
            assert!(!plain.raw(i), "Table D.8 has no raw row (pass {i})");
            assert!(!termall.raw(i), "Table D.8 has no raw row (pass {i})");
            assert!(
                !plain.terminates(i),
                "termination only on the last pass, which B.10.7.2 adds (pass {i})"
            );
            assert!(
                termall.terminates(i),
                "\"Termination on every pass\" (pass {i})"
            );
        }
    }

    /// Table D.9, row by row, at the bit-plane numbers the table prints.
    ///
    /// `(bit-plane, pass type, coding operation)` exactly as the three
    /// columns read. The table runs over two pages: bit-planes 1 to 4 are
    /// printed in full on the first, bit-plane 5 and a "..." row and a
    /// "final" set on the second. D.6's prose states the first page's rows a
    /// second time — "The first cleanup pass ... and the next three sets of
    /// significance propagation, magnitude refinement, and cleanup coding
    /// passes are decoded with the arithmetic coder. The fourth cleanup pass
    /// shall include an arithmetic coder termination" — so those ten rows are
    /// transcribed from two independent statements and the rest from one.
    #[test]
    fn table_d9_row_by_row() {
        // (pass index, is raw, terminates) for bit-planes 1 to 6.
        let rows: &[(u32, bool, bool)] = &[
            // Bit-plane 1: cleanup, AC.
            (0, false, false),
            // Bit-plane 2: AC throughout.
            (1, false, false),
            (2, false, false),
            (3, false, false),
            // Bit-plane 3: AC throughout.
            (4, false, false),
            (5, false, false),
            (6, false, false),
            // Bit-plane 4: AC, and its cleanup pass terminates.
            (7, false, false),
            (8, false, false),
            (9, false, true),
            // Bit-plane 5, the three rows the second page opens with:
            // "raw", "raw, terminate", "AC, terminate".
            (10, true, false),
            (11, true, true),
            (12, false, true),
            // Bit-plane 6. The table prints "..." and then a "final" set with
            // the same three operations, so this row is the pattern repeating
            // rather than a printed row — and for a six-plane code-block it
            // *is* the printed "final" set.
            (13, true, false),
            (14, true, true),
            (15, false, true),
        ];
        let s = Schedule::new(true, false);
        for &(i, raw, terminates) in rows {
            assert_eq!(s.raw(i), raw, "Table D.9's coding operation for pass {i}");
            assert_eq!(
                s.terminates(i),
                terminates,
                "Table D.9's termination for pass {i}"
            );
        }
        assert_eq!(
            FIRST_BYPASS_PASS, 10,
            "D.6: the fourth significance propagation pass, in bit-plane 5"
        );
    }

    /// D.6's last paragraph: `BYPASS` and `TERMALL` together terminate
    /// everything, raw passes included.
    #[test]
    fn bypass_with_termall_terminates_every_pass() {
        let s = Schedule::new(true, true);
        for i in 0..40 {
            assert!(
                s.terminates(i),
                "\"then every pass is terminated (including both raw passes)\" (pass {i})"
            );
        }
        // And the raw rows are still the raw rows: TERMALL changes where
        // segments end, never how a pass is read.
        let bypass_only = Schedule::new(true, false);
        for i in 0..40 {
            assert_eq!(s.raw(i), bypass_only.raw(i), "pass {i}");
        }
    }

    /// Every codeword segment holds passes of one kind.
    ///
    /// Not a restatement of the two predicates but a consequence of them, and
    /// the invariant tier-1 leans on: it opens one reader per segment, so a
    /// segment holding a raw pass and an arithmetic pass would decode the
    /// second of them out of the wrong reader. The check walks the segment
    /// boundaries the way B.10.7.2 does and asserts each run is uniform.
    #[test]
    fn a_codeword_segment_never_mixes_raw_and_arithmetic_passes() {
        for &(bypass, termall) in &[(false, false), (true, false), (false, true), (true, true)] {
            let s = Schedule::new(bypass, termall);
            let mut start = 0u32;
            for i in 0..60u32 {
                if s.terminates(i) || i == 59 {
                    for j in start..=i {
                        assert_eq!(
                            s.raw(j),
                            s.raw(start),
                            "segment {start}..={i} mixes kinds at pass {j} \
                             (bypass={bypass}, termall={termall})"
                        );
                    }
                    start = i + 1;
                }
            }
        }
    }
}
