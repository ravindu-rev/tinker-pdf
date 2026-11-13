//! Size-scaling complexity tests for the COS parser
//! (`docs/design/resource-exhaustion.md`, "The size-scaling law").
//!
//! Each shape is a document this file writes at a size n and at four times
//! n, opened and read whole through the counted doors,
//! `CosDocument::open_counted` and `CosDocument::get_counted`, whose tally is
//! MS-01's: the tokens the lexer produced, the cross-reference entries read,
//! the objects parsed and the bytes the repair scan looked through. The law is
//! the design's: the work at 4 n is at most 1.1 × 4 times the work at n, for a
//! parser that reads each element a bounded number of times. A quadratic
//! shape costs sixteen times as much and an n¹·⁵ one eight.
//!
//! Every shape is held to what it is as well as to what it costs: each object
//! reads back as written, through the counted and the uncounted doors alike,
//! and the document opens on the rung of the ladder the shape is about. No
//! clock is read anywhere here; a count is the same on every target.

use std::sync::Arc;

use tinker_pdf_cos::{CosDocument, LadderLevel, ObjRef, Object, PdfString, Stats, Work};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// What one shape cost at one size, and what it read.
struct Measured {
    stats: Stats,
    objects: Vec<Arc<Object>>,
}

/// Opens `file` through the counted doors, reads `objects`, and checks the
/// uncounted doors answer the same.
fn measure(file: &[u8], objects: &[u32], ladder: LadderLevel) -> Measured {
    let work = Work::new();
    let doc = CosDocument::open_counted(file.to_vec(), &work).expect("the shape opens");
    assert_eq!(doc.ladder_level(), ladder, "the shape opens on its rung");
    let read: Vec<Arc<Object>> = objects
        .iter()
        .map(|&num| {
            doc.get_counted(ObjRef::new(num, 0), &work)
                .expect("an object reads")
        })
        .collect();
    let plain = CosDocument::open(file.to_vec()).expect("the shape opens uncounted");
    assert_eq!(plain.ladder_level(), ladder);
    for (&num, counted) in objects.iter().zip(&read) {
        let uncounted = plain.get(ObjRef::new(num, 0)).expect("an object reads");
        assert_eq!(
            &uncounted, counted,
            "object {num} reads alike through both doors"
        );
    }
    Measured {
        stats: work.stats(),
        objects: read,
    }
}

/// The law, with both figures in the failure.
fn holds_the_law(shape: &str, small: &Measured, large: &Measured) {
    let (n, kn) = (small.stats.work(), large.stats.work());
    println!("  {shape:14} work at n {n:>9}  at 4n {kn:>9}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}; at n {:?}, at 4n {:?}",
        small.stats,
        large.stats,
    );
}

/// A string object's text: its number, so each one reads back as itself.
fn string_of(num: u32) -> Object {
    text(&format!("object {num}"))
}

/// A literal string object holding `text`.
fn text(text: &str) -> Object {
    Object::String(PdfString {
        bytes: text.as_bytes().to_vec(),
        hex: false,
    })
}

/// A document of `bodies`, numbered from 1, under one classic table.
fn classic(bodies: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The catalog and the page tree every shape opens with, then `rest`.
fn with_catalog(rest: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 0 /Kids [] >>".to_string(),
    ];
    bodies.extend(rest);
    bodies
}

/// n objects under one classic table, each read once.
fn objects(n: u32) -> Measured {
    let numbers: Vec<u32> = (3..3 + n).collect();
    let file = classic(&with_catalog(
        numbers.iter().map(|num| format!("(object {num})")),
    ));
    let read = measure(&file, &numbers, LadderLevel::Trust);
    for (&num, object) in numbers.iter().zip(&read.objects) {
        assert_eq!(**object, string_of(num));
    }
    read
}

#[test]
fn objects_under_a_classic_table_cost_work_linear_in_their_count() {
    holds_the_law("objects", &objects(500), &objects(2_000));
}

/// One array nested `depth` deep, holding a number.
fn nested(depth: u32) -> Measured {
    let body = format!(
        "{}7{}",
        "[ ".repeat(depth as usize),
        " ]".repeat(depth as usize)
    );
    let read = measure(&classic(&with_catalog([body])), &[3], LadderLevel::Trust);
    let mut at: &Object = &read.objects[0];
    for _ in 0..depth {
        let Object::Array(items) = at else {
            panic!("the nesting is cut short");
        };
        assert_eq!(items.len(), 1);
        at = &items[0];
    }
    assert_eq!(*at, Object::Int(7), "the innermost value is read");
    read
}

#[test]
fn an_array_nested_deeper_costs_work_linear_in_its_depth() {
    // Inside `MAX_NEST_DEPTH`, 256, at both sizes.
    holds_the_law("nesting", &nested(50), &nested(200));
}

/// A document updated `updates` times, each update a section of its own that
/// redefines object 3 and adds one object, chained to the last by `/Prev`.
fn sections(updates: u32) -> Measured {
    let mut file = classic(&with_catalog(["(object 3, revision 0)".to_string()]));
    let mut prev = last_startxref(&file);
    for revision in 1..=updates {
        let three = file.len();
        file.extend_from_slice(
            format!("3 0 obj\n(object 3, revision {revision})\nendobj\n").as_bytes(),
        );
        let added_num = 3 + revision;
        let added = file.len();
        file.extend_from_slice(
            format!("{added_num} 0 obj\n(object {added_num})\nendobj\n").as_bytes(),
        );
        let xref = file.len();
        file.extend_from_slice(
            format!(
                "xref\n3 1\n{three:010} 00000 n \n{added_num} 1\n{added:010} 00000 n \n\
                 trailer\n<< /Size {} /Root 1 0 R /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n",
                added_num + 1
            )
            .as_bytes(),
        );
        prev = xref;
    }
    let numbers: Vec<u32> = (3..=3 + updates).collect();
    let read = measure(&file, &numbers, LadderLevel::Trust);
    let doc = CosDocument::open(file.clone()).expect("opens");
    assert_eq!(
        doc.revisions().len(),
        updates as usize + 1,
        "every section of the chain is read"
    );
    assert_eq!(
        *read.objects[0],
        text(&format!("object 3, revision {updates}")),
        "the newest section's object 3 wins"
    );
    read
}

/// The offset the last `startxref` names.
fn last_startxref(file: &[u8]) -> usize {
    let text = String::from_utf8_lossy(file);
    let at = text.rfind("startxref\n").expect("a startxref") + "startxref\n".len();
    text[at..]
        .lines()
        .next()
        .and_then(|line| line.trim().parse().ok())
        .expect("its offset")
}

#[test]
fn a_longer_prev_chain_costs_work_linear_in_its_sections() {
    // Inside `MAX_XREF_CHAIN`, 64 sections, at both sizes.
    holds_the_law("sections", &sections(12), &sections(48));
}

/// n objects in one object stream, which a cross-reference stream names.
fn object_stream(n: u32) -> Measured {
    let first_member = 4u32;
    let members: Vec<u32> = (first_member..first_member + n).collect();
    let mut prologue = String::new();
    let mut objects = String::new();
    for &num in &members {
        prologue.push_str(&format!("{num} {} ", objects.len()));
        objects.push_str(&format!("(object {num}) "));
    }
    let data = format!("{prologue}{objects}");

    let mut file = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for body in [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 0 /Kids [] >>".to_string(),
    ] {
        offsets.push(file.len());
        file.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", offsets.len()).as_bytes());
    }
    offsets.push(file.len());
    file.extend_from_slice(
        format!(
            "3 0 obj\n<< /Type /ObjStm /N {n} /First {} /Length {} >>\nstream\n{data}\nendstream\nendobj\n",
            prologue.len(),
            data.len()
        )
        .as_bytes(),
    );

    // The cross-reference stream: free 0, the three objects in the file, the
    // members in the stream, and itself, at /W [1 4 2], uncompressed.
    let xref_num = first_member + n;
    let xref_at = file.len();
    let mut rows = Vec::new();
    let row = |rows: &mut Vec<u8>, kind: u8, field: u32, index: u16| {
        rows.push(kind);
        rows.extend_from_slice(&field.to_be_bytes());
        rows.extend_from_slice(&index.to_be_bytes());
    };
    row(&mut rows, 0, 0, 0xFFFF);
    for offset in &offsets {
        row(&mut rows, 1, *offset as u32, 0);
    }
    for (index, _) in members.iter().enumerate() {
        row(&mut rows, 2, 3, index as u16);
    }
    row(&mut rows, 1, xref_at as u32, 0);
    file.extend_from_slice(
        format!(
            "{xref_num} 0 obj\n<< /Type /XRef /Size {} /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
            xref_num + 1,
            rows.len()
        )
        .as_bytes(),
    );
    file.extend_from_slice(&rows);
    file.extend_from_slice(
        format!("\nendstream\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );

    let read = measure(&file, &members, LadderLevel::Trust);
    for (&num, object) in members.iter().zip(&read.objects) {
        assert_eq!(**object, string_of(num));
    }
    read
}

#[test]
fn an_object_stream_of_more_objects_costs_work_linear_in_its_objects() {
    holds_the_law("object stream", &object_stream(500), &object_stream(2_000));
}

/// n objects in a file whose `startxref` names nothing, so the open rescans
/// it and the repair scan finds every object.
fn repaired(n: u32) -> Measured {
    let numbers: Vec<u32> = (3..3 + n).collect();
    let mut file = b"%PDF-1.7\n".to_vec();
    let bodies = with_catalog(numbers.iter().map(|num| format!("(object {num})")));
    for (index, body) in bodies.iter().enumerate() {
        file.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    file.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\nstartxref\n0\n%%EOF\n");
    let read = measure(&file, &numbers, LadderLevel::Rescan);
    for (&num, object) in numbers.iter().zip(&read.objects) {
        assert_eq!(**object, string_of(num));
    }
    assert!(
        read.stats.bytes_scanned > 0,
        "the repair scan searched the file"
    );
    read
}

#[test]
fn the_repair_scan_over_a_damaged_file_costs_work_linear_in_its_objects() {
    holds_the_law("repair scan", &repaired(500), &repaired(2_000));
}

/// The counters are the parse's own: a document read twice through the
/// counted door costs nothing the second time, because the store answers, and
/// the open alone counts the sections it read.
#[test]
fn a_read_the_store_answers_parses_nothing() {
    let file = classic(&with_catalog(["(object 3)".to_string()]));
    let work = Work::new();
    let doc = CosDocument::open_counted(file, &work).expect("opens");
    let opened = work.stats();
    assert!(
        opened.tokens_lexed > 0,
        "the open lexed its table and trailer"
    );
    assert_eq!(opened.xref_entries_read, 4, "four rows in the table");
    let first = Work::new();
    doc.get_counted(ObjRef::new(3, 0), &first).expect("reads");
    assert_eq!(first.stats().objects_parsed, 1);
    let again = Work::new();
    doc.get_counted(ObjRef::new(3, 0), &again).expect("reads");
    assert_eq!(
        again.stats().work(),
        0,
        "the store answered the second read"
    );
}
