//! COS syntax, generated: whole files written from a model, every object of
//! 7.3 in every spelling the clause allows, in one to four sections.
//!
//! `cos_object` and `cos_document` hand mutated bytes to the parser, and most
//! of what they reach is the parser refusing them. This target reads its
//! input as choices instead (`tinker_pdf_fuzz::Choices`) and builds a file out
//! of them with `tinker_pdf_fuzz::cos`: sections that are cross-reference
//! tables (7.5.4) or cross-reference streams (7.5.8) with object streams
//! (7.5.7), incremental updates that redefine, free and reuse objects
//! (7.5.6), streams whose `/Length` is direct or indirect and honest or
//! dishonest (7.3.8.2), and each token spelled one of the ways 7.3 allows —
//! white space and comments of every kind or none, signs, leading zeros and
//! points on numbers, every escape of a literal string, hex strings with
//! white space and an odd last digit, `#xx` in names. Because it knows what
//! it wrote, it asserts that the reader read it back.
//!
//! The caps — four sections, six new objects a section, four levels of
//! nesting, six items a container, 24 bytes a string, ten a name, 96 a
//! stream — are `tinker_pdf_fuzz::cos`'s constants.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **Every object reads back as written.** Each object, read where the file
//!   holds it by `parse_object_at` — at the top level or in its object
//!   stream's data — is the model's, every value, every key in its order, a
//!   string's bytes and its form; it raises no warning, and reading stops
//!   where the object ends. Read by `parse_indirect_at` at the offset its
//!   entry names, it is the model's under the model's reference, and a
//!   stream's data starts where the writer put it.
//! - **A file written clean opens with no repair.** With every `/Length`
//!   honest, the file opens on `LadderLevel::Trust`, every live object reads
//!   as the model's, a stream's raw data is its data and its decoded data is
//!   what was compressed, every freed object reads as null, the trailer names
//!   the catalog and the last `/Size`, the document has one revision per
//!   section at the offsets the sections were written at, nothing raised a
//!   warning, and the strict validator finds no structure-tier defect.
//! - **A dishonest `/Length` is recovered.** The offsets are still honest, so
//!   the file still opens on `Trust`. Each stream whose `/Length` lies reads
//!   its data whole, with one `StreamLengthRecovered` naming the declared
//!   length and the data's, where 7.3.8.2 makes the extent recoverable: the
//!   declared length does not end at an `endstream` of its own accord, the
//!   data holds no `endstream`, and the ending before the keyword cannot be
//!   read as the data's last byte. Nothing else warns.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above compares this repository's reader with this
//! repository's writer of the same clauses. The writer spells from 7.3 and
//! 7.5 rather than from the reader's code, but a misreading of a clause that
//! both share — an escape read the same wrong way on both sides — round-trips
//! perfectly here and is wrong everywhere else. The evidence about the
//! standard is the corpus ratchet (`corpus/ratchet*.json`), which holds the
//! reader to files real producers wrote, and the grammar tests in
//! `crates/tinker-pdf-cos`, which compare parsed objects with the clauses'
//! own examples. Nor does a generated file reach the repair ladder:
//! `cos_document` mutates files into the damage the ladder repairs.
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_cos::{
    parse_indirect_at, parse_object_at, CosDocument, LadderLevel, Name, NameTable, ObjRef, Object,
    Tier, WarningKind, WarningSink,
};
use tinker_pdf_fuzz::cos::{self, Body, Ending, File, Place, Placed, Stream, Value, Written};
use tinker_pdf_fuzz::Choices;

fuzz_target!(|data: &[u8]| {
    let mut choices = Choices::new(data);
    let (file, _) = cos::generate(&mut choices);
    let written = cos::write(&file, &mut choices);
    every_object_reads_back(&written);
    the_file_reads_back(&file, &written);
});

/// The object's reference, for a failure to name.
fn named(placed: &Placed) -> String {
    format!("{} {} R", placed.num, placed.gen)
}

fn holds(outcome: Result<(), String>, placed: &Placed, how: &str) {
    if let Err(why) = outcome {
        panic!("{} read back by {how} otherwise: {why}", named(placed));
    }
}

/// Where the reader stops after `model`: past its last byte, or, for a bare
/// integer, at the token after it, which it reads to rule out `num gen R`.
fn stop(model: &Value, end: u64, next: u64) -> u64 {
    if matches!(model, Value::Int(_)) {
        next
    } else {
        end
    }
}

fn every_object_reads_back(written: &Written) {
    let names = NameTable::new();
    for placed in &written.objects {
        let mut sink = WarningSink::new();
        match &placed.place {
            Place::Top {
                header,
                value,
                next,
                data,
                end,
            } => {
                let indirect = parse_indirect_at(&written.bytes, *header, &names, &mut sink)
                    .unwrap_or_else(|| panic!("{}: no `N G obj` read at {header}", named(placed)));
                assert_eq!(
                    indirect.reference,
                    ObjRef::new(placed.num, placed.gen),
                    "the header at {header} named another reference"
                );
                let direct = parse_object_at(&written.bytes, value.start, &names, &mut sink);
                match &placed.body {
                    Body::Value(model) => {
                        holds(
                            cos::same(model, &indirect.object, &names),
                            placed,
                            "parse_indirect_at",
                        );
                        assert_eq!(
                            indirect.end_offset,
                            *end,
                            "{} ended elsewhere",
                            named(placed)
                        );
                        holds(
                            cos::same(model, &direct.object, &names),
                            placed,
                            "parse_object_at",
                        );
                        assert_eq!(
                            direct.end_offset,
                            stop(model, value.end, *next),
                            "{} stopped elsewhere",
                            named(placed)
                        );
                    }
                    Body::Stream(stream) => {
                        let Object::Stream(read) = &indirect.object else {
                            panic!(
                                "{} read as {:?}, not a stream",
                                named(placed),
                                indirect.object
                            );
                        };
                        holds(
                            cos::same_entries(&stream.dict, &read.dict, &names),
                            placed,
                            "parse_indirect_at",
                        );
                        let data = data.clone().unwrap_or_default();
                        assert_eq!(
                            read.data_start,
                            data.start,
                            "{}'s data starts elsewhere",
                            named(placed)
                        );
                        let direct_length = match stream.length() {
                            Some(Value::Int(len)) => u64::try_from(*len).ok(),
                            _ => None,
                        };
                        assert_eq!(
                            read.len_hint,
                            direct_length,
                            "{}'s /Length read otherwise",
                            named(placed)
                        );
                        holds(
                            cos::same(&Value::Dict(stream.dict.clone()), &direct.object, &names),
                            placed,
                            "parse_object_at",
                        );
                        assert_eq!(
                            direct.end_offset,
                            value.end,
                            "{}'s dictionary stopped elsewhere",
                            named(placed)
                        );
                    }
                }
            }
            Place::Packed {
                container,
                value,
                next,
            } => {
                let Body::Value(model) = &placed.body else {
                    panic!("{} is a stream in an object stream", named(placed));
                };
                let data = &written.containers[container];
                let direct = parse_object_at(data, value.start as u64, &names, &mut sink);
                holds(
                    cos::same(model, &direct.object, &names),
                    placed,
                    "parse_object_at in its object stream",
                );
                assert_eq!(
                    direct.end_offset,
                    stop(model, value.end as u64, *next as u64),
                    "{} stopped elsewhere in its object stream",
                    named(placed)
                );
            }
        }
        assert!(
            sink.warnings().is_empty(),
            "reading {} warned: {:?}",
            named(placed),
            sink.warnings()
        );
    }
}

/// What a stream's `/Length` says, through an indirect object where it is
/// one.
fn declared(stream: &Stream, live: &BTreeMap<u32, &Placed>) -> Option<u64> {
    let value = match stream.length()? {
        Value::Ref(num, _) => match &live.get(num)?.body {
            Body::Value(value) => value,
            Body::Stream(_) => return None,
        },
        value => value,
    };
    match value {
        Value::Int(len) => u64::try_from(*len).ok(),
        _ => None,
    }
}

/// Whether 7.3.8.2 lets a reader recover a lying stream's data whole: the
/// declared length does not end at an `endstream` of its own accord, the data
/// holds no `endstream`, and the ending before the keyword cannot be read as
/// the data's last byte.
fn recoverable(bytes: &[u8], stream: &Stream, data_start: u64, declared: u64) -> bool {
    let lands = data_start
        .checked_add(declared)
        .and_then(|at| usize::try_from(at).ok())
        .filter(|at| *at <= bytes.len())
        .is_some_and(|mut at| {
            let mut skipped = 0;
            while skipped < 32
                && bytes
                    .get(at)
                    .is_some_and(|b| matches!(b, 0 | 9 | 10 | 12 | 13 | 32))
            {
                at += 1;
                skipped += 1;
            }
            bytes
                .get(at..)
                .is_some_and(|rest| rest.starts_with(b"endstream"))
        });
    let holds_keyword = stream.data.windows(9).any(|w| w == b"endstream");
    let last = stream.data.last().copied();
    let ambiguous = match stream.ending {
        Ending::Bare => matches!(last, Some(b'\n' | b'\r')),
        Ending::Lf => last == Some(b'\r'),
        Ending::CrLf | Ending::Cr => false,
    };
    !lands && !holds_keyword && !ambiguous
}

fn the_file_reads_back(file: &File, written: &Written) {
    let doc = CosDocument::open(written.bytes.clone())
        .unwrap_or_else(|error| panic!("the file did not open: {error}"));
    assert_eq!(
        doc.ladder_level(),
        LadderLevel::Trust,
        "a file whose offsets are honest needed repair: {:?}",
        doc.warnings()
    );
    let names = doc.names_table();
    let live = written.live(file);

    let mut lying: BTreeSet<ObjRef> = BTreeSet::new();
    let mut recovered: BTreeMap<ObjRef, (u64, u64)> = BTreeMap::new();
    for placed in live.values() {
        let r = ObjRef::new(placed.num, placed.gen);
        let read = doc
            .get(r)
            .unwrap_or_else(|error| panic!("{}: {error}", named(placed)));
        match &placed.body {
            Body::Value(model) => holds(cos::same(model, &read, names), placed, "CosDocument::get"),
            Body::Stream(stream) => {
                let Object::Stream(object) = &*read else {
                    panic!("{} read as {read:?}, not a stream", named(placed));
                };
                holds(
                    cos::same_entries(&stream.dict, &object.dict, names),
                    placed,
                    "CosDocument::get",
                );
                let declared = declared(stream, &live);
                let raw = doc
                    .stream_raw(r)
                    .unwrap_or_else(|error| panic!("{}: {error}", named(placed)));
                if declared == Some(stream.data.len() as u64) {
                    assert_eq!(raw, stream.data, "{}'s data read otherwise", named(placed));
                    if let Some(decoded) = &stream.decoded {
                        let read = doc
                            .stream_decoded(r)
                            .unwrap_or_else(|error| panic!("{}: {error}", named(placed)));
                        assert_eq!(&read, decoded, "{}'s data decoded otherwise", named(placed));
                    }
                } else {
                    lying.insert(r);
                    let Some(declared) = declared else {
                        panic!("{}'s /Length names no integer", named(placed));
                    };
                    if recoverable(&written.bytes, stream, object.data_start, declared) {
                        assert_eq!(
                            raw,
                            stream.data,
                            "{}'s data was not recovered whole",
                            named(placed)
                        );
                        recovered.insert(r, (declared, stream.data.len() as u64));
                    }
                }
            }
        }
    }
    for num in written.freed(file) {
        let read = doc
            .get(ObjRef::new(num, 0))
            .unwrap_or_else(|error| panic!("{num}: {error}"));
        assert!(read.is_null(), "freed object {num} read as {read:?}");
    }

    let trailer = doc.trailer();
    assert_eq!(
        trailer.get_ref(Name::ROOT),
        Some(ObjRef::new(1, 0)),
        "the trailer's /Root"
    );
    assert_eq!(
        trailer.get_int(Name::SIZE),
        written.sizes.last().map(|size| i64::from(*size)),
        "the trailer's /Size"
    );
    let revisions = doc.revisions();
    assert_eq!(
        revisions.len(),
        written.sections.len(),
        "one revision per section"
    );
    for (revision, section) in revisions.iter().rev().zip(&written.sections) {
        assert_eq!(
            revision.xref_at, section.xref_at,
            "a revision's section is elsewhere"
        );
        assert_eq!(
            revision.byte_range.end, section.eof,
            "a revision ends elsewhere"
        );
    }

    let warnings = doc.warnings();
    for warning in &warnings {
        let WarningKind::StreamLengthRecovered { declared, actual } = warning.kind else {
            panic!("the reader warned {warning}: {warnings:?}");
        };
        let object = warning
            .object
            .unwrap_or_else(|| panic!("a recovered length named no object: {warning}"));
        assert!(
            lying.contains(&object),
            "{warning}, of a stream whose /Length is honest"
        );
        if let Some(&(lie, len)) = recovered.get(&object) {
            assert_eq!((declared, actual), (Some(lie), len), "{warning}");
        }
    }
    for r in recovered.keys() {
        assert!(
            warnings.iter().any(|warning| warning.object == Some(*r)),
            "{} {} R lied about its length and nothing said so",
            r.num,
            r.gen
        );
    }

    if lying.is_empty() {
        let structure: Vec<String> = tinker_pdf_cos::validate(&doc)
            .iter()
            .filter(|defect| defect.kind.tier() == Tier::Structure)
            .map(ToString::to_string)
            .collect();
        assert!(
            structure.is_empty(),
            "a file written clean has structure defects: {structure:?}"
        );
    }
}
