//! The right-to-left probe kit's line families as ratchets: ruling 14's
//! reading of `BidiCharacterTest.txt` and of wrapped sentences, line by line,
//! held to what it read at `935b077`.
//!
//! One `#[ignore]`d test a family of `tools/probes/rtl/families.tsv` (its
//! `lines` kind), each reading the family's lines exactly as its probe in
//! `tools/probes/rtl/probes/shape/` did, and failing when a line read right at
//! the baseline is read wrong now (`rtl_lines/ratchet.rs`). Gains are printed.
//!
//! ```text
//! cargo test -p tinker-pdf-shape --test rtl_lines -- --ignored --nocapture
//! ```
//!
//! Each test prints one `RAN rtl_lines <family> against 935b077:` line with its
//! row count.

/// A family's line, to the ratchet rather than to stdout.
macro_rules! emit {
    ($($arg:tt)*) => {
        crate::rtl_lines::ratchet::emit(format!($($arg)*))
    };
}

mod rtl_lines {
    pub(crate) mod ratchet;
    pub(crate) mod zz_fv_shape;
    pub(crate) mod zz_k11rv2_sent;
    pub(crate) mod zz_k11rv_bidi;
    pub(crate) mod zz_lane_r_lines;
}

/// One `#[ignore]`d ratchet a family: the test, the family's name in the kit
/// (which names its baseline), and what the family is.
macro_rules! families {
    ($($test:ident = $family:literal: $what:literal,)*) => {
        $(
            #[doc = $what]
            #[test]
            #[ignore = "a probe family; run with -- --ignored --nocapture"]
            fn $test() {
                rtl_lines::ratchet::family($family, rtl_lines::$test::run);
            }
        )*
    };
}

families! {
    zz_k11rv_bidi = "zz_k11rv_bidi": "lines: K-11 review; writes lines/bct-auto.tsv and lines/wrapped.tsv",
    zz_k11rv2_sent = "zz_k11rv2_sent": "lines: K-11 review; writes lines/sent.tsv",
    zz_lane_r_lines = "zz_laneR_lines": "lines: lane R (round 5); writes lines/laneR.tsv",
    zz_fv_shape = "zz_fv_shape": "lines: 56dfd93 verification; LINE; does not compile before 56dfd93",
}
