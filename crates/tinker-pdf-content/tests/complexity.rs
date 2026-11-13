//! Size-scaling complexity tests for the content interpreter
//! (`docs/design/resource-exhaustion.md`, "The size-scaling law").
//!
//! Each shape is a content stream this file writes at a size n and at four
//! times n, run through `interpret_counted`, whose tally is MS-01's: the
//! tokens the interpreter took from its tokenizer and the operators it ran,
//! in the stream and in every form it draws. The law is the design's: the
//! work at 4 n is at most 1.1 × 4 times the work at n, for an interpreter that
//! takes each token a bounded number of times. A quadratic shape costs sixteen
//! times as much and an n¹·⁵ one eight.
//!
//! Every shape is held to what it is as well as to what it costs: what the
//! device hears is counted from the shape, and the counted door makes the
//! device hear exactly what the uncounted one does. No clock is read here.

use tinker_pdf_content::{
    interpret, interpret_counted, EventKind, FontSource, Form, Matrix, RecordingDevice,
};
use tinker_pdf_cos::{Stats, Work};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// A source with fonts that map every byte to itself, and forms: `Fm`, which
/// strokes one line, and `F0` to `F15`, each of which strokes a line and draws
/// the next, down to `depth`, whose form strokes its line alone.
struct Forms {
    depth: usize,
}

/// The line every form strokes: three operators over seven tokens.
const LINE: &str = "0 0 m 1 1 l S\n";

impl FontSource for Forms {
    fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
        bytes
            .iter()
            .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
            .collect()
    }

    fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
        (0.0, 880.0, -1000.0)
    }

    fn form(&self, name: &[u8]) -> Option<Form> {
        let form = |content: String, stream: u64| Form {
            content: content.into_bytes(),
            matrix: Matrix::IDENTITY,
            bbox: None,
            group: None,
            // Each form an identity of its own, so a chain of distinct forms
            // is never taken for one running in its own ancestry.
            stream,
        };
        if name == b"Fm" {
            return Some(form(LINE.to_string(), 1 << 16));
        }
        let level: usize = std::str::from_utf8(name.strip_prefix(b"F")?)
            .ok()?
            .parse()
            .ok()?;
        let content = if level + 1 < self.depth {
            format!("{LINE}/F{} Do\n", level + 1)
        } else {
            LINE.to_string()
        };
        Some(form(content, (level as u64 + 2) << 16))
    }
}

/// What one shape cost at one size, and what the device heard.
struct Measured {
    stats: Stats,
    heard: RecordingDevice,
}

/// Runs `content` through the counted door, and checks the uncounted door
/// makes the device hear the same calls.
fn measure(content: &str, depth: usize) -> Measured {
    let fonts = Forms { depth };
    let work = Work::new();
    let mut heard = RecordingDevice::new();
    interpret_counted(
        content.as_bytes(),
        Matrix::IDENTITY,
        &mut heard,
        &fonts,
        &work,
    );
    let mut plain = RecordingDevice::new();
    interpret(content.as_bytes(), Matrix::IDENTITY, &mut plain, &fonts);
    assert_eq!(
        heard.kinds(),
        plain.kinds(),
        "the counted door changed what the device heard"
    );
    Measured {
        stats: work.stats(),
        heard,
    }
}

/// The law, with both figures in the failure.
fn holds_the_law(shape: &str, small: &Measured, large: &Measured) {
    let (n, kn) = (small.stats.work(), large.stats.work());
    println!("  {shape:16} work at n {n:>7}  at 4n {kn:>7}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}; at n {:?}, at 4n {:?}",
        small.stats,
        large.stats,
    );
}

/// `n` lines, each three operators.
fn operators(n: usize) -> Measured {
    let measured = measure(&"10 10 m 20 20 l S\n".repeat(n), 0);
    assert_eq!(measured.stats.operators_run, 3 * n as u64);
    assert_eq!(measured.heard.count(EventKind::StrokePath), n);
    measured
}

#[test]
fn more_operators_cost_work_linear_in_their_count() {
    holds_the_law("operators", &operators(1_000), &operators(4_000));
}

/// One `d` after a dash array of `n` operands, inside the operand stack's 512.
fn operand_run(n: usize) -> Measured {
    let content = format!("[{}] 0 d\n10 10 m 20 20 l S\n", "1 ".repeat(n));
    let measured = measure(&content, 0);
    let dashes = measured
        .heard
        .events()
        .iter()
        .filter_map(|event| event.state())
        .map(|state| state.dashes.len())
        .max();
    assert_eq!(dashes, Some(n), "the whole run reached the operator");
    measured
}

#[test]
fn a_longer_run_of_operands_costs_work_linear_in_its_length() {
    holds_the_law("operand run", &operand_run(100), &operand_run(400));
}

/// `n` nested `q`, then as many `Q`, inside the 64 states saved at once.
fn saves(n: usize) -> Measured {
    let measured = measure(&format!("{}{}", "q ".repeat(n), "Q ".repeat(n)), 0);
    assert_eq!(measured.heard.count(EventKind::SaveState), n);
    assert_eq!(measured.heard.count(EventKind::RestoreState), n);
    measured
}

#[test]
fn deeper_saves_cost_work_linear_in_their_depth() {
    holds_the_law("q and Q", &saves(12), &saves(48));
}

/// `n` nested marked-content sequences, then as many ends, inside the 4 096
/// open at once.
fn marked(n: usize) -> Measured {
    let content = format!(
        "{}{}",
        "/Span << /MCID 0 >> BDC\n".repeat(n),
        "EMC\n".repeat(n)
    );
    let measured = measure(&content, 0);
    assert_eq!(measured.heard.count(EventKind::BeginMarkedContent), n);
    assert_eq!(measured.heard.count(EventKind::EndMarkedContent), n);
    measured
}

#[test]
fn deeper_marked_content_costs_work_linear_in_its_depth() {
    holds_the_law("BDC and EMC", &marked(500), &marked(2_000));
}

/// `n` inline images of four samples each.
fn inline_images(n: usize) -> Measured {
    let measured = measure(
        &"BI /W 2 /H 2 /BPC 8 /CS /G ID \x10\x20\x30\x40 EI\n".repeat(n),
        0,
    );
    assert_eq!(measured.heard.count(EventKind::DrawImage), n);
    measured
}

#[test]
fn more_inline_images_cost_work_linear_in_their_count() {
    holds_the_law("inline images", &inline_images(200), &inline_images(800));
}

/// One form drawn `n` times.
fn form_drawn(n: usize) -> Measured {
    let measured = measure(&"/Fm Do\n".repeat(n), 0);
    assert_eq!(measured.heard.count(EventKind::StrokePath), n);
    measured
}

#[test]
fn a_form_drawn_more_often_costs_work_linear_in_its_draws() {
    holds_the_law("form drawn", &form_drawn(200), &form_drawn(800));
}

/// Forms nested `n` deep, each stroking a line and drawing the next, inside
/// the interpreter's sixteen.
fn forms_nested(n: usize) -> Measured {
    let measured = measure("/F0 Do\n", n);
    assert_eq!(
        measured.heard.count(EventKind::StrokePath),
        n,
        "every level of the chain ran"
    );
    measured
}

#[test]
fn forms_nested_deeper_cost_work_linear_in_their_depth() {
    holds_the_law("forms nested", &forms_nested(3), &forms_nested(12));
}
