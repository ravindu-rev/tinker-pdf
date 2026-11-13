//! The policy of the validate-on-save helper: which findings each door admits
//! (`docs/design/validate-on-save.md`).
//!
//! This file names no crate, so every test tree compiles it. A reading — the
//! cos reading beside it, or the facade reading in
//! `crates/tinker-pdf/tests/written_support/mod.rs` — opens the bytes it is
//! handed, runs its validators and hands every finding over as an [`Item`];
//! what a door admits is decided here and nowhere else, so the two readings
//! cannot hold one document to two rules, and a verdict a reading adds is more
//! items with no change to this file.
//!
//! - [`authored`]: a document this engine wrote from nothing it did not write.
//!   Every verdict is empty.
//! - [`rewritten`]: a save of somebody else's file. It owns the structure it
//!   wrote, and leaves every other class of finding no worse than its input.
//! - [`expecting`]: a document damaged on purpose. Its findings are exactly the
//!   ones the test lists.
//!
//! A door that does not admit a document panics at its caller with one line per
//! finding its form does not admit, grouped by verdict.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

/// The verdicts in the order a failure prints them; one a reading adds later
/// prints after these.
const ORDER: [&str; 5] = ["structure", "semantics", "PDF/A", "PDF/UA", "PDF/X"];

/// One finding of one verdict, in terms that need no engine type to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// `structure`, `semantics`, `PDF/A`, `PDF/UA` or `PDF/X`.
    pub verdict: &'static str,
    /// Whether the writer owns it whatever it was handed: the strict
    /// validator's structure tier.
    pub owned: bool,
    /// The finding's kind as `Debug` prints it: the expected form's key.
    pub exact: String,
    /// What survives renumbering: a rewrite's key.
    pub class: String,
    /// The byte it is at, when it has one.
    pub at: Option<u64>,
    /// How a failure prints it.
    pub shown: String,
}

/// What a reading made of one document.
#[derive(Clone, Debug)]
pub struct Read {
    /// The bytes it read, as the file holds them.
    pub bytes: Arc<[u8]>,
    /// Every finding of every verdict the reading ran.
    pub items: Vec<Item>,
    /// What a failure prints beside a verdict's lines: for a claimed profile,
    /// the claim, the groups that ran and how much this build did not decide.
    pub notes: Vec<(&'static str, String)>,
}

/// One finding a test says it wrote on purpose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Want {
    /// The verdict it belongs to.
    pub verdict: &'static str,
    /// Its kind as `Debug` prints it.
    pub exact: String,
}

/// A finding a door did not admit, as a failure prints it.
struct Line {
    verdict: &'static str,
    text: String,
}

/// Holds a document as one this engine wrote from nothing it did not write:
/// every verdict empty.
///
/// `door` is the door the caller came through, because `synthesised` holds
/// what a conversion wrote by this rule and its failure should say so.
#[track_caller]
pub fn authored(label: &str, door: &str, read: &Read) {
    let lines: Vec<Line> = read
        .items
        .iter()
        .map(|item| Line {
            verdict: item.verdict,
            text: item.shown.clone(),
        })
        .collect();
    if !lines.is_empty() {
        fail(label, door, None, &read.notes, &lines, &[]);
    }
}

/// Holds `output` as a save of `input`, a file this engine did not write.
///
/// An output that begins with every byte of its input is an incremental update
/// (7.5.6), and any other a full rewrite:
///
/// - a full rewrite re-serialises every object, so it owns every structure
///   finding, and of every other class it has no more than its input;
/// - an incremental update keeps its input's sections, and with them any
///   structure defect of the input's, so its owned findings are held to no
///   more of each class than the input's and to none at or past the input's
///   last byte, where everything is the update's;
/// - a profile the output claims is held to no more findings of each class
///   than the input's under the same claim, which is part of the class.
///
/// With a clean input this is [`authored`]: no more than nothing is nothing.
#[track_caller]
pub fn rewritten(label: &str, input: &Read, output: &Read) {
    let incremental = output.bytes.starts_with(&input.bytes);
    let end = input.bytes.len() as u64;
    let had = classes(&input.items);
    let has = classes(&output.items);
    let mut lines = Vec::new();
    for item in &output.items {
        let key = (item.verdict, item.class.as_str());
        let before = had.get(&key).copied().unwrap_or(0);
        let after = has.get(&key).copied().unwrap_or(0);
        let mut why = Vec::new();
        if item.owned && !incremental {
            why.push("a full rewrite owns the structure it writes".to_string());
        } else if after > before {
            why.push(format!(
                "`{}`: {after} in the output, {before} in the input",
                item.class
            ));
        }
        if item.owned && incremental {
            if let Some(at) = item.at.filter(|&at| at >= end) {
                why.push(format!(
                    "at byte {at}, past the input's last byte: the input is {end} bytes, \
                     and what follows them is the update's"
                ));
            }
        }
        if !why.is_empty() {
            lines.push(Line {
                verdict: item.verdict,
                text: format!("{} ({})", item.shown, why.join("; ")),
            });
        }
    }
    if !lines.is_empty() {
        let form = if incremental {
            "an incremental update of its input"
        } else {
            "a full rewrite of its input"
        };
        fail(label, "rewritten", Some(form), &output.notes, &lines, &[]);
    }
}

/// Holds a document damaged on purpose to exactly the findings `expected`
/// lists, matched by verdict and exact kind as a multiset: each listed as often
/// as it occurs, nothing found that is not listed, nothing listed that is not
/// found. An empty list is [`authored`].
#[track_caller]
pub fn expecting(label: &str, read: &Read, expected: &[Want]) {
    let mut unmatched: Vec<&Want> = expected.iter().collect();
    let mut lines = Vec::new();
    for item in &read.items {
        let listed = unmatched
            .iter()
            .position(|want| want.verdict == item.verdict && want.exact == item.exact);
        match listed {
            Some(at) => {
                unmatched.remove(at);
            }
            None => lines.push(Line {
                verdict: item.verdict,
                text: format!("{} (found and not listed: {})", item.shown, item.exact),
            }),
        }
    }
    let missing: Vec<String> = unmatched
        .iter()
        .map(|want| format!("{}: {}", want.verdict, want.exact))
        .collect();
    if !lines.is_empty() || !missing.is_empty() {
        fail(label, "expecting", None, &read.notes, &lines, &missing);
    }
}

/// Fails a door before any verdict: bytes that do not open, a key that is
/// missing or wrong, a document a reading cannot hold.
#[track_caller]
pub fn refuse(label: &str, door: &str, why: &str) -> ! {
    panic!("`{label}` fails `{door}`: {why}")
}

/// How many findings of each verdict and class a document holds.
fn classes(items: &[Item]) -> BTreeMap<(&'static str, &str), usize> {
    let mut out = BTreeMap::new();
    for item in items {
        *out.entry((item.verdict, item.class.as_str())).or_default() += 1;
    }
    out
}

/// The panic every door fails with: one line per finding not admitted,
/// grouped by verdict, each group beside its verdict's note.
#[track_caller]
fn fail(
    label: &str,
    door: &str,
    form: Option<&str>,
    notes: &[(&'static str, String)],
    lines: &[Line],
    missing: &[String],
) -> ! {
    let mut out = format!("`{label}` fails `{door}`");
    if let Some(form) = form {
        let _ = write!(out, ", held as {form}");
    }
    out.push(':');
    let mut verdicts: Vec<&'static str> = Vec::new();
    for line in lines {
        if !verdicts.contains(&line.verdict) {
            verdicts.push(line.verdict);
        }
    }
    verdicts.sort_by_key(|verdict| {
        ORDER
            .iter()
            .position(|known| known == verdict)
            .unwrap_or(ORDER.len())
    });
    for verdict in verdicts {
        let _ = write!(out, "\n  {verdict}");
        if let Some((_, note)) = notes.iter().find(|(of, _)| *of == verdict) {
            let _ = write!(out, " ({note})");
        }
        out.push(':');
        for line in lines.iter().filter(|line| line.verdict == verdict) {
            let _ = write!(out, "\n    {}", line.text);
        }
    }
    if !missing.is_empty() {
        out.push_str("\n  listed and not found:");
        for want in missing {
            let _ = write!(out, "\n    {want}");
        }
    }
    panic!("{out}")
}
