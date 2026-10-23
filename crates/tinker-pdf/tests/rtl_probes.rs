//! The right-to-left probe kit's families as ratchets: every book the K-11 and
//! CD-19 rounds built to measure right-to-left text, held to what it read at
//! `935b077`.
//!
//! One `#[ignore]`d test a family of `tools/probes/rtl/families.tsv` (its
//! `rows` and `tsv` kinds), each building the family's books exactly as its
//! probe in `tools/probes/rtl/probes/pdf/` did and failing when any book reads
//! worse than the committed baseline of that build, on any metric its line
//! carries (`rtl_probes/ratchet.rs` lists them). Gains are printed and never
//! fail; refreshing the baseline after one, or after a price the owner
//! accepted, is the kit README's "Refreshing the baseline".
//!
//! ```text
//! cargo test -p tinker-pdf --test rtl_probes -- --ignored --nocapture
//! cargo test -p tinker-pdf --features bundled-fonts --test rtl_probes -- --ignored --nocapture
//! ```
//!
//! Each test prints one `RAN rtl_probes <family> [<build>] against 935b077:`
//! line with its row count; a run that prints fewer `RAN` lines than there are
//! families did not run them all. The timing probes stay probes: a wall-clock
//! time is not a measurement another machine repeats.

mod epub_support;

/// A family's line, to the ratchet rather than to stdout.
macro_rules! emit {
    ($($arg:tt)*) => {
        crate::rtl_probes::ratchet::emit(format!($($arg)*))
    };
}

mod rtl_probes {
    pub(crate) mod common;
    pub(crate) mod ratchet;
    pub(crate) mod vov;
    pub(crate) mod zz_c1_tw;
    pub(crate) mod zz_code1_neg;
    pub(crate) mod zz_code1_tdprobe;
    pub(crate) mod zz_f6_ems;
    pub(crate) mod zz_f6_h23price;
    pub(crate) mod zz_f6_readas;
    pub(crate) mod zz_f6_transform;
    pub(crate) mod zz_f6_unfired;
    pub(crate) mod zz_k11rv_fixtures;
    pub(crate) mod zz_rr3m_extra;
    pub(crate) mod zz_rr3m_fv_extra;
    pub(crate) mod zz_rr3m_fv_marks;
    pub(crate) mod zz_rr3m_h1_breadth;
    pub(crate) mod zz_rr3m_h1_gaps;
    pub(crate) mod zz_rr3m_h1_h4;
    pub(crate) mod zz_rr3m_h1_inline;
    pub(crate) mod zz_rr3m_h1_marks;
    pub(crate) mod zz_rr3m_h1_ofscripts;
    pub(crate) mod zz_rr3m_h1_tables;
    pub(crate) mod zz_rr3m_h1_v1;
    pub(crate) mod zz_rr3m_h1_v3;
    pub(crate) mod zz_rr3m_h1min;
    pub(crate) mod zz_rr3m_h1min2;
    pub(crate) mod zz_rr3m_h2_faces;
    pub(crate) mod zz_rr3m_h2_gaps2;
    pub(crate) mod zz_rr3m_h2_h4b;
    pub(crate) mod zz_rr3m_h2_marks2;
    pub(crate) mod zz_rr3m_h2_nested;
    pub(crate) mod zz_rr3m_h2_pages;
    pub(crate) mod zz_rr3m_h2_r2gloss;
    pub(crate) mod zz_rr3m_h2_r2ibword;
    pub(crate) mod zz_rr3m_h2_r2lvl2;
    pub(crate) mod zz_rr3m_h2_r2ornate;
    pub(crate) mod zz_rr3m_h2_r2qeglue;
    pub(crate) mod zz_rr3m_h2_r3justgloss;
    pub(crate) mod zz_rr3m_h2_r3rtlgloss;
    pub(crate) mod zz_rr3m_h2_tables;
    pub(crate) mod zz_rr3m_h2_v1b;
    pub(crate) mod zz_rr3m_h2_v3b;
    pub(crate) mod zz_rr3m_hunt_brackets;
    pub(crate) mod zz_rr3m_hunt_cells;
    pub(crate) mod zz_rr3m_hunt_code32;
    pub(crate) mod zz_rr3m_hunt_dirless;
    pub(crate) mod zz_rr3m_hunt_etsweep;
    pub(crate) mod zz_rr3m_hunt_gaps;
    pub(crate) mod zz_rr3m_hunt_gloss;
    pub(crate) mod zz_rr3m_hunt_isolates;
    pub(crate) mod zz_rr3m_hunt_min;
    pub(crate) mod zz_rr3m_hunt_numbers;
    pub(crate) mod zz_rr3m_hunt_pointed;
    pub(crate) mod zz_rr3m_hunt_punct;
    pub(crate) mod zz_rr3m_hunt_rtlpunct;
    pub(crate) mod zz_rr3m_hunt_scripts;
    pub(crate) mod zz_rr3m_hunt_spacing;
    pub(crate) mod zz_rr3m_hunt_urls;
    pub(crate) mod zz_rr3m_hunt_ws;
    pub(crate) mod zz_rr3m_lane_r;
    pub(crate) mod zz_rr3m_probe;
    pub(crate) mod zz_rr3m_tie;
    pub(crate) mod zz_rv6_hunt;
    pub(crate) mod zz_rv6_ib;
    pub(crate) mod zz_v6_auto;
    pub(crate) mod zz_v6_cells;
    pub(crate) mod zz_v6_inline;
    pub(crate) mod zz_v6_lists;
    pub(crate) mod zz_v6_measure;
    pub(crate) mod zz_v6_numsep;
    pub(crate) mod zz_v6_pdf;
    pub(crate) mod zz_v6_scripts2;
    pub(crate) mod zz_v6_transform;
    pub(crate) mod zz_v6_units;
    pub(crate) mod zz_vx_k11r2_extra;
    pub(crate) mod zz_vx_k11rv2_epub;
    pub(crate) mod zz_vx_k11rv3_just;
    pub(crate) mod zz_vx_k11rv3_own;
    pub(crate) mod zz_vx_k11rv_dirless;
    pub(crate) mod zz_vx_k11rv_ext;
    pub(crate) mod zz_vx_k11rv_neutral;
    pub(crate) mod zz_vx_probe_paren;
    pub(crate) mod zz_vx_rv4code;
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
                rtl_probes::ratchet::family($family, rtl_probes::$test::run);
            }
        )*
    };
}

families! {
    zz_k11rv_fixtures = "zz_k11rv_fixtures": "kit: K-11 round 4 kit",
    zz_rr3m_extra = "zz_rr3m_extra": "kit: K-11 round 4 kit",
    zz_rr3m_probe = "zz_rr3m_probe": "kit: K-11 round 4 kit",
    zz_rr3m_tie = "zz_rr3m_tie": "kit: K-11 round 4 kit",
    zz_vx_k11r2_extra = "zz_vx_k11r2_extra": "vx: round-5 review (confirm); zz_k11r2_extra with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_k11rv2_epub = "zz_vx_k11rv2_epub": "vx: round-5 review (confirm); zz_k11rv2_epub with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_k11rv3_just = "zz_vx_k11rv3_just": "vx: round-5 review (confirm); zz_k11rv3_just with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_k11rv3_own = "zz_vx_k11rv3_own": "vx: round-5 review (confirm); zz_k11rv3_own with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_k11rv_dirless = "zz_vx_k11rv_dirless": "vx: round-5 review (confirm); zz_k11rv_dirless with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_k11rv_ext = "zz_vx_k11rv_ext": "vx: round-5 review (confirm); zz_k11rv_ext with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_k11rv_neutral = "zz_vx_k11rv_neutral": "vx: round-5 review (confirm); zz_k11rv_neutral with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_probe_paren = "zz_vx_probe_paren": "vx: round-5 review (confirm); zz_probe_paren with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_vx_rv4code = "zz_vx_rv4code": "vx: round-5 review (confirm); zz_rv4code with the overprint shim: the same ROW lines and a VOVX line a book",
    zz_c1_tw = "zz_c1_tw": "tw: round-5 review (confirm); TWROW: code 32 and Tw of the overflow font",
    zz_rr3m_hunt_brackets = "zz_rr3m_hunt_brackets": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_numbers = "zz_rr3m_hunt_numbers": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_urls = "zz_rr3m_hunt_urls": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_gloss = "zz_rr3m_hunt_gloss": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_isolates = "zz_rr3m_hunt_isolates": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_pointed = "zz_rr3m_hunt_pointed": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_scripts = "zz_rr3m_hunt_scripts": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_spacing = "zz_rr3m_hunt_spacing": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_ws = "zz_rr3m_hunt_ws": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_punct = "zz_rr3m_hunt_punct": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_code32 = "zz_rr3m_hunt_code32": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_gaps = "zz_rr3m_hunt_gaps": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_cells = "zz_rr3m_hunt_cells": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_rtlpunct = "zz_rr3m_hunt_rtlpunct": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_etsweep = "zz_rr3m_hunt_etsweep": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_dirless = "zz_rr3m_hunt_dirless": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_hunt_min = "zz_rr3m_hunt_min": "hunt: K-11 round-4 review (hunt)",
    zz_rr3m_fv_extra = "zz_rr3m_fv_extra": "fv: 56dfd93 verification",
    zz_rr3m_fv_marks = "zz_rr3m_fv_marks": "fv: 56dfd93 verification",
    zz_rr3m_lane_r = "zz_rr3m_laneR": "laneR: lane R (round 5); holds price/ and soft/, ruling 14's price",
    zz_rr3m_h1_gaps = "zz_rr3m_h1_gaps": "h1: round-5 review (hunt)",
    zz_rr3m_h1_h4 = "zz_rr3m_h1_h4": "h1: round-5 review (hunt)",
    zz_rr3m_h1_marks = "zz_rr3m_h1_marks": "h1: round-5 review (hunt)",
    zz_rr3m_h1_ofscripts = "zz_rr3m_h1_ofscripts": "h1: round-5 review (hunt)",
    zz_rr3m_h1_tables = "zz_rr3m_h1_tables": "h1: round-5 review (hunt)",
    zz_rr3m_h1_v1 = "zz_rr3m_h1_v1": "h1: round-5 review (hunt)",
    zz_rr3m_h1_v3 = "zz_rr3m_h1_v3": "h1: round-5 review (hunt)",
    zz_rr3m_h1_inline = "zz_rr3m_h1_inline": "h1: round-5 review (hunt)",
    zz_rr3m_h1_breadth = "zz_rr3m_h1_breadth": "h1: round-5 review (hunt)",
    zz_rr3m_h1min = "zz_rr3m_h1min": "h1: round-5 review (hunt)",
    zz_rr3m_h1min2 = "zz_rr3m_h1min2": "h1: round-5 review (hunt)",
    zz_rr3m_h2_faces = "zz_rr3m_h2_faces": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_gaps2 = "zz_rr3m_h2_gaps2": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_h4b = "zz_rr3m_h2_h4b": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_marks2 = "zz_rr3m_h2_marks2": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_nested = "zz_rr3m_h2_nested": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_pages = "zz_rr3m_h2_pages": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_tables = "zz_rr3m_h2_tables": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_v1b = "zz_rr3m_h2_v1b": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_v3b = "zz_rr3m_h2_v3b": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_r2gloss = "zz_rr3m_h2_r2gloss": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_r2ibword = "zz_rr3m_h2_r2ibword": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_r2lvl2 = "zz_rr3m_h2_r2lvl2": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_r2ornate = "zz_rr3m_h2_r2ornate": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_r2qeglue = "zz_rr3m_h2_r2qeglue": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_r3justgloss = "zz_rr3m_h2_r3justgloss": "h2: CD-19 fix review (hunt)",
    zz_rr3m_h2_r3rtlgloss = "zz_rr3m_h2_r3rtlgloss": "h2: CD-19 fix review (hunt)",
    zz_code1_neg = "zz_code1_neg": "neg: round-5 review (code); ZZN: negative word spacing, glyphs drawn over another",
    zz_code1_tdprobe = "zz_code1_tdprobe": "td: round-5 review (code); content streams built by hand, read by TextDevice and ruling 14; writes <build>/zz_code1_tdprobe.tsv",
    zz_v6_inline = "zz_v6_inline": "v6: round 6 lane V (hunt); nested inline styles, sup/sub, sizes, code, shadows, colour-only spans, links, spacing spans",
    zz_v6_lists = "zz_v6_lists": "v6: round 6 lane V (hunt); lists, figures and captions, generated content, q, blockquote, heading",
    zz_v6_units = "zz_v6_units": "v6: round 6 lane V (hunt); spacing units, the 0.45em and 0.5em thresholds, sums, alignments and indents",
    zz_v6_scripts2 = "zz_v6_scripts2": "v6: round 6 lane V (hunt); supplementary-plane right-to-left scripts, presentation forms, Urdu, ZWNJ, kashida, Yiddish",
    zz_v6_numsep = "zz_v6_numsep": "v6: round 6 lane V (hunt); numbers with every separator",
    zz_v6_cells = "zz_v6_cells": "v6: round 6 lane V (hunt); word and letter spacing around table cells, wrapping cells",
    zz_v6_measure = "zz_v6_measure": "v6: round 6 lane V (hunt); narrow and wide measures, type sizes, preserved white space, page breaks",
    zz_v6_auto = "zz_v6_auto": "v6: round 6 lane V (hunt); dir=auto, unicode-bidi: plaintext, bdi",
    zz_v6_transform = "zz_v6_transform": "v6t: round 6 lane V (hunt); CSS transforms and tight line heights under spacing",
    zz_v6_pdf = "zz_v6_pdf": "v6pdf: round 6 lane V (hunt); plain synthetic PDFs drawn as producers draw right-to-left lines",
    zz_f6_transform = "zz_f6_transform": "f6: round 6 lane F (sweep); the class of V1: CSS transforms of one axis or both, turns, skews, a matrix, shifts, on the paragraph, its block, both, an inline block",
    zz_f6_ems = "zz_f6_ems": "f6: round 6 lane F (sweep); the edges of the fix of V1: two cells near half an em apart, inline blocks of about half an em, marks of no advance opening a styled run, under no transform, a scale of one axis and a skew",
    zz_f6_readas = "zz_f6_readas": "f6inj: round 6 lane F (injections); characters the page's fonts read otherwise than typed (a NO-BREAK SPACE, a SOFT HYPHEN, letters past the Basic Multilingual Plane) under spacing and a scale, the books of read_as",
    zz_f6_unfired = "zz_f6_unfired": "f6inj: round 6 lane F (injections); books for the rules of the read-back no suite fired in a build: reads_whole and reads_closer with bundled-fonts, read_as",
    zz_f6_h23price = "zz_f6_h23price": "f6pdf: round 6 lane F (owner options); plain synthetic PDFs, two words of one script in two text objects a gap apart: the price of answering H2-3 in the reader",
    zz_rv6_hunt = "zz_rv6_hunt": "rv6: round 6 review (hunt); a styled ?! and brackets, numbers with a NO-BREAK, NARROW NO-BREAK or FIGURE SPACE and Arabic separators, SOFT HYPHENs, directional marks, a widened and a narrowed inline block in one line, a line repeated under transforms and over pages; under none, a scale of one axis and a quarter turn, word and letter spacing, with a text shadow and without",
    zz_rv6_ib = "zz_rv6_ib": "rv6: round 6 review (hunt); the class of the first hunt's ar/mixib/none: inline blocks widened by scaleX(2) or narrowed by scaleX(0.6) to 0.8, holding a word, a styled ?! or both, under none, a quarter turn and scaleX(2), word spacing 0.4em to 0.55em and letter spacing",
}
