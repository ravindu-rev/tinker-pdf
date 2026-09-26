//! One document, one script total (`limits::MAX_SCRIPT_TOTAL`).
//!
//! A document keeps its scripts in three places — the field tree's `/AA`
//! (12.6.3 table 198), `/Names /JavaScript` (7.7.4) and the catalog's `/AA`
//! (12.6.3 table 200) — and each of the three walks used to start from the
//! full four-mebibyte total. A file that filled all three therefore surfaced
//! twelve, which is the same class of mistake `MAX_SCRIPT_TOTAL` was written
//! to prevent one level down: a per-item cap is not a total cap when the item
//! count is document-controlled, and a total handed out once per walk is not
//! a total either.
//!
//! # What the fixture can and cannot hold
//!
//! The brief for this milestone asked for three mebibytes in each of the
//! three places. **The catalog cannot hold three mebibytes and never could**:
//! 12.6.3 table 200 defines exactly five triggers — `WC`, `WS`, `DS`, `WP`,
//! `DP` — and `MAX_SCRIPT_LEN` caps each at 64 KiB, so the catalog's ceiling
//! is 320 KiB whatever a file does. That is a finding about the shape of the
//! defect rather than a gap in the test: the catalog was never the surface
//! that could triple the total on its own, and the fixture below puts the
//! pressure where a real file can put it. Three mebibytes across 48 fields,
//! a mebibyte and a half across 24 name-tree entries, and the catalog reads
//! last with nothing left — which is a sharper assertion than a catalog that
//! fits, because "nothing left" is exactly the state the old code could not
//! reach.

use tinker_pdf_cos::{
    catalog_scripts, catalog_scripts_within, document_scripts, document_scripts_within, fields,
    fields_within, script_summary, CosDocument, Script, ScriptBudget,
};

/// 64 KiB, which is `limits::MAX_SCRIPT_LEN` exactly — the largest script
/// this build will surface rather than name.
const ONE: usize = 64 << 10;
/// `limits::MAX_SCRIPT_TOTAL`.
const TOTAL: usize = 4 << 20;
/// Fields carrying a maximal calculate action: three mebibytes of source.
const FIELDS: usize = 48;
/// Name-tree entries carrying one each: a mebibyte and a half more.
const DOCUMENT: usize = 24;

/// A document that fills all three surfaces past what one read may take.
///
/// Written out rather than normalised through a rewrite: it is four and a
/// half megabytes of source text, the repair scanner reads it in one pass,
/// and re-serialising it would double the cost of every test here for a
/// property none of them assert.
fn crowded() -> CosDocument {
    let script = "x".repeat(ONE);
    let mut out = String::with_capacity(5 << 20);
    out.push_str("%PDF-1.7\n");

    let field_refs: Vec<String> = (0..FIELDS).map(|i| format!("{} 0 R", 10 + i)).collect();
    out.push_str(&format!(
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R\n\
         /AcroForm << /Fields [{}] >>\n\
         /Names << /JavaScript 300 0 R >>\n\
         /AA << /WC << /S /JavaScript /JS (wc();) >>\n\
             /WS << /S /JavaScript /JS (ws();) >>\n\
             /DS << /S /JavaScript /JS (ds();) >>\n\
             /WP << /S /JavaScript /JS (wp();) >>\n\
             /DP << /S /JavaScript /JS (dp();) >> >> >>\nendobj\n",
        field_refs.join(" ")
    ));
    out.push_str("2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n");

    for i in 0..FIELDS {
        out.push_str(&format!(
            "{} 0 obj\n<< /FT /Tx /T (f{i:02}) /AA << /C << /S /JavaScript /JS ({script}) >> >> >>\nendobj\n",
            10 + i
        ));
    }

    let entries: Vec<String> = (0..DOCUMENT)
        .map(|i| format!("(js{i:02}) {} 0 R", 100 + i))
        .collect();
    out.push_str(&format!(
        "300 0 obj\n<< /Names [{}] >>\nendobj\n",
        entries.join(" ")
    ));
    for i in 0..DOCUMENT {
        out.push_str(&format!(
            "{} 0 obj\n<< /S /JavaScript /JS ({script}) >>\nendobj\n",
            100 + i
        ));
    }

    out.push_str("trailer\n<< /Size 400 /Root 1 0 R >>\n%%EOF\n");
    CosDocument::open(out.into_bytes()).expect("the fixture opens")
}

/// How many decoded bytes of source a set of scripts actually surfaced.
fn surfaced(scripts: &[Script]) -> usize {
    scripts
        .iter()
        .filter_map(|s| s.source().map(str::len))
        .sum()
}

/// **The exit criterion.** All three surfaces read under one budget spend
/// four mebibytes between them and not one byte more, and every script past
/// that comes back named rather than truncated.
#[test]
fn a_document_cannot_surface_twelve_mebibytes() {
    let doc = crowded();
    let mut budget = ScriptBudget::new();

    let found = fields_within(&doc, &mut budget);
    assert_eq!(found.len(), FIELDS);
    let field_sources: Vec<Script> = found
        .iter()
        .filter_map(|f| f.scripts.calculate.clone())
        .collect();
    assert_eq!(field_sources.len(), FIELDS);
    assert_eq!(surfaced(&field_sources), FIELDS * ONE, "three mebibytes");
    assert_eq!(budget.spent(), FIELDS * ONE);
    assert_eq!(budget.left(), TOTAL - FIELDS * ONE);

    // The document-level tree reads next, and the budget runs out inside it:
    // sixteen more fit exactly, and the remaining eight are named.
    let document = document_scripts_within(&doc, &mut budget);
    assert_eq!(document.len(), DOCUMENT);
    let fit = (TOTAL - FIELDS * ONE) / ONE;
    for (at, entry) in document.iter().enumerate() {
        let expected = if at < fit {
            Script::Source("x".repeat(ONE))
        } else {
            Script::Oversize(ONE)
        };
        assert_eq!(
            entry.script, expected,
            "{} is the wrong side of the cap",
            entry.name
        );
    }
    assert_eq!(budget.left(), 0, "the total is spent exactly");

    // And the catalog reads last, with nothing left. Every one of 12.6.3
    // table 200's five triggers is present and every one of them is named.
    let catalog = catalog_scripts_within(&doc, &mut budget);
    assert_eq!(catalog.len(), 5);
    let named: Vec<&str> = catalog
        .iter()
        .filter(|entry| entry.script.is_oversize())
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(named, ["WC", "WS", "DS", "WP", "DP"]);
    assert_eq!(
        surfaced(&catalog.iter().map(|e| e.script.clone()).collect::<Vec<_>>()),
        0
    );

    // Four mebibytes total, across all three, which is the whole claim.
    assert_eq!(budget.spent(), TOTAL);
}

/// `script_summary` is the call that reads every script a document carries,
/// so it is the one that had to stop handing out three totals.
#[test]
fn the_summary_counts_one_total_and_not_three() {
    let doc = crowded();
    let summary = script_summary(&doc);

    assert_eq!(summary.fields_with_scripts, FIELDS);
    assert_eq!(summary.calculate_actions, FIELDS);
    assert_eq!(summary.document_scripts, DOCUMENT);
    assert_eq!(summary.catalog_actions, 5);
    // Eight name-tree entries and all five catalog triggers, and none of the
    // fields: the eight are what the old code let through.
    assert_eq!(summary.oversize, (DOCUMENT - 16) + 5);
}

/// The other half of the contract, stated so it cannot drift: each of the
/// three bare entry points is **one read of one surface** and starts from the
/// full total. A caller that reads more than one and wants the document's
/// answer threads a `ScriptBudget`, which is what `script_summary` does.
///
/// The alternative — a budget living inside the document — was rejected: it
/// would make reading a document mutate it, and the same document read twice
/// would answer differently, which is ruling 4's contract broken for a
/// hardening cap.
#[test]
fn each_bare_entry_point_is_one_read_with_its_own_total() {
    let doc = crowded();

    let alone = document_scripts(&doc);
    assert_eq!(alone.len(), DOCUMENT);
    assert!(
        alone.iter().all(|entry| !entry.script.is_oversize()),
        "a mebibyte and a half fits inside one read of its own"
    );

    let alone = catalog_scripts(&doc);
    assert_eq!(alone.len(), 5);
    assert!(alone.iter().all(|entry| !entry.script.is_oversize()));

    // And the field walk alone still surfaces its three mebibytes whole.
    let found = fields(&doc);
    let sources: Vec<Script> = found
        .iter()
        .filter_map(|f| f.scripts.calculate.clone())
        .collect();
    assert_eq!(surfaced(&sources), FIELDS * ONE);
}

/// A budget refuses whole scripts, never prefixes of them.
///
/// Taking part of a script's length would leave the budget claiming to have
/// surfaced source that was reported as `Oversize` instead, which is the
/// accounting error that makes a total untrustworthy.
#[test]
fn a_budget_takes_a_whole_script_or_none_of_it() {
    let doc = crowded();
    let mut budget = ScriptBudget::new();
    let _ = fields_within(&doc, &mut budget);
    let left = budget.left();
    assert_eq!(left % ONE, 0, "whole scripts only");

    let document = document_scripts_within(&doc, &mut budget);
    let taken = document
        .iter()
        .filter(|entry| !entry.script.is_oversize())
        .count();
    assert_eq!(taken * ONE, left, "every byte left went into whole scripts");
}
