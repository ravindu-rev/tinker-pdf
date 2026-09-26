//! Form data exchange: the FDF (12.7.8) and XFDF readers and writers, over
//! arbitrary bytes.
//!
//! Every input goes to **both** readers, with no control byte: the two
//! formats announce themselves (`%FDF-` and `<?xml`/`<xfdf`), so a seed of
//! either kind is a seed of the other's refusal path too, and a mutation that
//! turns one into the other is one this target wants to see.
//!
//! # What this target checks
//!
//! **That neither reader panics, hangs or exhausts memory** (ruling 1) — the
//! FDF reader's `/Kids` walk with its visited set and the field tree's depth
//! cap, and the XFDF reader's element stack under `tinker-pdf-xml`'s bounds.
//!
//! And one property beyond that, which is what makes the target worth more
//! than a crash hunt: **what a reader read, the matching writer writes and
//! the reader reads back the same.** For FDF, the same fields with the same
//! values as a multiset — the writer puts qualified names back into a tree,
//! which may order siblings differently from a file that interleaved them.
//! For XFDF, the same, with values compared as text, since XFDF does not
//! distinguish a button's state from a word; and only where the writer did
//! not refuse, because a control character XML 1.0 cannot carry is refused
//! by name rather than written. A writer that dropped an escape, split a name
//! at the wrong period or lost a selection fails here on the first input that
//! exercises it.
//!
//! What it does not check: that either reader is right about the files it is
//! given. That is the hand-authored fixtures in
//! `crates/tinker-pdf/tests/form_data.rs`.
//!
//! # Before the first session
//!
//! **No `cargo fuzz` session has run this target**: the container it was
//! written in had neither a nightly toolchain nor `cargo-fuzz`. What did run,
//! on 26 September 2026, was this body's round-trip property as a throwaway
//! stable-toolchain test over the six seeds here — 360 000 mutations in
//! release, bit flips, deletions, splices between seeds and a splice of
//! fourteen FDF and XFDF tokens — and it found **two** defects in the writers
//! before they were committed, neither of which a crash hunt would have:
//!
//! - a field with **no value** and fields beneath it was lost, because a
//!   reader takes a node with kids and no value for a group; it now stands
//!   beside the group;
//! - a name with an **empty partial name** (`.x`, `a..b`) split into an empty
//!   `/T` or `name`, which reads back as no name — `.x` came back as `x`; such
//!   a name is now written whole.
//!
//! Both are pinned by `what_the_round_trip_property_found_reads_back` in the
//! test file above. The throwaway harness was not kept; this target is the
//! form it keeps, and `hostile_input.rs`'s `mutated_form_data_never_panics`
//! is the stable-toolchain sweep that runs on every commit.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::form_data::{read_fdf, read_xfdf, FieldData, FormData};
use tinker_pdf::FieldValue;

/// A field as a comparable pair, values as text lists.
fn key(field: &FieldData, text_only: bool) -> (String, u8, Vec<String>) {
    let (kind, values) = match &field.value {
        FieldValue::Text(text) => (1, vec![text.clone()]),
        FieldValue::State(state) => (if text_only { 1 } else { 2 }, vec![state.clone()]),
        FieldValue::Many(values) => (3, values.clone()),
        _ => (0, Vec::new()),
    };
    (field.name.clone(), kind, values)
}

fn same_fields(a: &FormData, b: &FormData, text_only: bool) -> bool {
    let mut left: Vec<_> = a.fields.iter().map(|f| key(f, text_only)).collect();
    let mut right: Vec<_> = b.fields.iter().map(|f| key(f, text_only)).collect();
    left.sort();
    right.sort();
    left == right
}

fuzz_target!(|data: &[u8]| {
    if let Ok(read) = read_fdf(data) {
        let again = read_fdf(&read.to_fdf()).expect("our own FDF reads back");
        assert!(same_fields(&read, &again, false), "FDF round trip");
        assert_eq!(read.source, again.source);
    }
    if let Ok(read) = read_xfdf(data) {
        if let Ok(xml) = read.to_xfdf() {
            let again = read_xfdf(xml.as_bytes()).expect("our own XFDF reads back");
            assert!(same_fields(&read, &again, true), "XFDF round trip");
        }
        let again = read_fdf(&read.to_fdf()).expect("our own FDF reads back");
        assert!(same_fields(&read, &again, false), "XFDF to FDF");
    }
});
