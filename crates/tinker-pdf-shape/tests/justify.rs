//! Justification (`docs/design/shaping.md`, *Justification*): a face's
//! `JSTF` table read back as the test assembled it.
//!
//! No face this repository carries has a `JSTF`, so every table here is
//! written by the builder below from the chapter's field lists (OpenType
//! 1.9.1's `JSTF` chapter, the text ISO/IEC 14496-22 holds), and every
//! expected value is the one the builder was handed — never one the reader
//! computed.

use tinker_pdf_shape::{
    Buffer, Change, Jstf, JstfLangSys, JstfPriority, JstfScript, Layout, Limits, MarkWidths, Mode,
    Table, Tag,
};

fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn offset(at: usize) -> u16 {
    u16::try_from(at).expect("a test table fits Offset16")
}

/// A `SinglePos` lookup (`GPOS` lookup type 1) of one format 1 subtable that
/// adds `x_advance` to `glyph`: the form a `JstfMax` lookup takes, as the
/// chapter's Example 4 writes one.
fn single_pos_lookup(glyph: u16, x_advance: i16) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1); // lookupType: SinglePos
    push16(&mut out, 0); // lookupFlag
    push16(&mut out, 1); // subTableCount
    push16(&mut out, 8); // subtableOffsets[0]
                         // SinglePosFormat1, at 8.
    push16(&mut out, 1); // format
    push16(&mut out, 8); // coverageOffset, from the subtable
    push16(&mut out, 0x0004); // valueFormat: XAdvance
    push_i16(&mut out, x_advance);
    // CoverageFormat1, at 16.
    push16(&mut out, 1);
    push16(&mut out, 1);
    push16(&mut out, glyph);
    out
}

/// A lookup of another type with no subtables, so that a `JstfMax` holding a
/// kind the chapter excludes reads back as that kind.
fn empty_lookup(kind: u16) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, kind);
    push16(&mut out, 0);
    push16(&mut out, 0);
    out
}

/// One priority as a test states it: the eight modification lists, in the
/// chapter's order — `GSUB` shrinkage enable and disable, `GPOS` shrinkage
/// enable and disable, then the same four for extension — and the two
/// `JstfMax` tables' lookups. `None` writes a NULL offset.
#[derive(Clone, Debug, Default)]
struct PrioritySpec {
    shrinkage: [Option<Vec<u16>>; 4],
    shrinkage_max: Option<Vec<Vec<u8>>>,
    extension: [Option<Vec<u16>>; 4],
    extension_max: Option<Vec<Vec<u8>>>,
}

/// A `JstfModList`: a count and the lookup indices.
fn mod_list(lookups: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, u16::try_from(lookups.len()).expect("few"));
    for lookup in lookups {
        push16(&mut out, *lookup);
    }
    out
}

/// A `JstfMax`: a count, the offsets, then the lookups, each offset measured
/// from the `JstfMax` table.
fn jstf_max(lookups: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, u16::try_from(lookups.len()).expect("few"));
    let mut at = 2 + 2 * lookups.len();
    for lookup in lookups {
        push16(&mut out, offset(at));
        at += lookup.len();
    }
    for lookup in lookups {
        out.extend_from_slice(lookup);
    }
    out
}

/// A `JstfPriority`: its ten offsets, then the tables they name, each
/// measured from the priority.
fn priority(spec: &PrioritySpec) -> Vec<u8> {
    let mut children: Vec<Option<Vec<u8>>> = Vec::new();
    for list in &spec.shrinkage {
        children.push(list.as_ref().map(|l| mod_list(l)));
    }
    children.push(spec.shrinkage_max.as_ref().map(|m| jstf_max(m)));
    for list in &spec.extension {
        children.push(list.as_ref().map(|l| mod_list(l)));
    }
    children.push(spec.extension_max.as_ref().map(|m| jstf_max(m)));
    let mut out = Vec::new();
    let mut body = Vec::new();
    for child in &children {
        match child {
            Some(bytes) => {
                push16(&mut out, offset(20 + body.len()));
                body.extend_from_slice(bytes);
            }
            None => push16(&mut out, 0),
        }
    }
    out.extend(body);
    out
}

/// A `JstfLangSys`: the priority count, the offsets, then the priorities,
/// each measured from the language system.
fn lang_sys(priorities: &[PrioritySpec]) -> Vec<u8> {
    let bodies: Vec<Vec<u8>> = priorities.iter().map(priority).collect();
    let mut out = Vec::new();
    push16(&mut out, u16::try_from(bodies.len()).expect("few"));
    let mut at = 2 + 2 * bodies.len();
    for body in &bodies {
        push16(&mut out, offset(at));
        at += body.len();
    }
    for body in bodies {
        out.extend(body);
    }
    out
}

/// One script as a test states it.
#[derive(Clone, Debug, Default)]
struct ScriptSpec {
    tag: [u8; 4],
    extenders: Option<Vec<u16>>,
    default: Option<Vec<PrioritySpec>>,
    languages: Vec<([u8; 4], Vec<PrioritySpec>)>,
}

/// A `JstfScript`: the extender offset, the default language system's, the
/// record count and the records, then the extender table, the default and
/// each language system, every offset measured from the script.
fn script(spec: &ScriptSpec) -> Vec<u8> {
    let header = 6 + 6 * spec.languages.len();
    let mut body = Vec::new();
    let extender_at = spec.extenders.as_ref().map(|glyphs| {
        let at = header + body.len();
        push16(&mut body, u16::try_from(glyphs.len()).expect("few"));
        for glyph in glyphs {
            push16(&mut body, *glyph);
        }
        at
    });
    let default_at = spec.default.as_ref().map(|priorities| {
        let at = header + body.len();
        body.extend(lang_sys(priorities));
        at
    });
    let mut records = Vec::new();
    for (tag, priorities) in &spec.languages {
        records.push((*tag, header + body.len()));
        body.extend(lang_sys(priorities));
    }
    let mut out = Vec::new();
    push16(&mut out, extender_at.map_or(0, offset));
    push16(&mut out, default_at.map_or(0, offset));
    push16(&mut out, u16::try_from(records.len()).expect("few"));
    for (tag, at) in records {
        out.extend_from_slice(&tag);
        push16(&mut out, offset(at));
    }
    out.extend(body);
    out
}

/// A `JSTF` table: version 1.0, the script count and records, then each
/// script, every offset measured from the header.
fn jstf(scripts: &[ScriptSpec]) -> Vec<u8> {
    let header = 6 + 6 * scripts.len();
    let bodies: Vec<Vec<u8>> = scripts.iter().map(script).collect();
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push16(&mut out, u16::try_from(scripts.len()).expect("few"));
    let mut at = header;
    for (spec, body) in scripts.iter().zip(&bodies) {
        out.extend_from_slice(&spec.tag);
        push16(&mut out, offset(at));
        at += body.len();
    }
    for body in bodies {
        out.extend(body);
    }
    out
}

/// The table every test below reads: the chapter's own examples, joined —
/// an Arabic script with two extender glyphs, a default language system of
/// two priorities and a Farsi one of one, and a Latin script with no
/// extenders and no default, whose one language system has a priority that
/// fills every one of the ten offsets.
fn assembled() -> Vec<ScriptSpec> {
    let ligatures = PrioritySpec {
        shrinkage: [Some(vec![46, 53, 99]), None, None, None],
        extension: [None, Some(vec![46, 53, 99]), None, None],
        ..PrioritySpec::default()
    };
    let kerning = PrioritySpec {
        shrinkage: [None, None, None, Some(vec![108, 110, 112])],
        extension: [None, None, Some(vec![108, 110, 112]), None],
        extension_max: Some(vec![single_pos_lookup(0x22, 360)]),
        ..PrioritySpec::default()
    };
    let farsi = PrioritySpec {
        extension: [None, None, None, Some(vec![7])],
        ..PrioritySpec::default()
    };
    let everything = PrioritySpec {
        shrinkage: [Some(vec![1]), Some(vec![2, 3]), Some(vec![4]), Some(vec![])],
        shrinkage_max: Some(vec![single_pos_lookup(5, -40), empty_lookup(7)]),
        extension: [
            Some(vec![9]),
            Some(vec![10]),
            Some(vec![11, 12, 13]),
            Some(vec![14]),
        ],
        extension_max: Some(vec![single_pos_lookup(6, 80)]),
    };
    vec![
        ScriptSpec {
            tag: *b"arab",
            extenders: Some(vec![0x01D3, 0x01D4]),
            default: Some(vec![ligatures, kerning]),
            languages: vec![(*b"FAR ", vec![farsi])],
        },
        ScriptSpec {
            tag: *b"latn",
            extenders: None,
            default: None,
            languages: vec![(*b"ENG ", vec![everything])],
        },
    ]
}

/// A modification list's lookups, read.
fn lookups_of(
    priority: &JstfPriority<'_>,
    mode: Mode,
    table: Table,
    change: Change,
) -> Option<Vec<u16>> {
    let list = priority.mod_list(mode, table, change)?;
    Some(
        (0..list.len())
            .map(|i| list.lookup(i).expect("in range"))
            .collect(),
    )
}

/// The four lists of one mode, in the chapter's order.
fn lists_of(priority: &JstfPriority<'_>, mode: Mode) -> [Option<Vec<u16>>; 4] {
    [
        lookups_of(priority, mode, Table::Gsub, Change::Enable),
        lookups_of(priority, mode, Table::Gsub, Change::Disable),
        lookups_of(priority, mode, Table::Gpos, Change::Enable),
        lookups_of(priority, mode, Table::Gpos, Change::Disable),
    ]
}

/// A `JstfMax`'s lookups as the bytes they stand on: each lookup's type and
/// its first subtable's first eight bytes, which for a `SinglePos` lookup
/// holds the format, the coverage offset, the value format and the value.
fn max_of(priority: &JstfPriority<'_>, mode: Mode) -> Option<Vec<(u16, Option<Vec<u8>>)>> {
    let max = priority.max(mode)?;
    Some(
        (0..max.len())
            .map(|i| {
                let lookup = max.lookup(i).expect("in range");
                let first = lookup
                    .subtable(0)
                    .map(|sub| sub.as_slice().iter().take(8).copied().collect());
                (lookup.kind(), first)
            })
            .collect(),
    )
}

/// The same, from what the test stated.
fn stated_max(lookups: &Option<Vec<Vec<u8>>>) -> Option<Vec<(u16, Option<Vec<u8>>)>> {
    lookups.as_ref().map(|lookups| {
        lookups
            .iter()
            .map(|bytes| {
                let kind = u16::from_be_bytes([bytes[0], bytes[1]]);
                let first = (bytes.len() > 8).then(|| bytes[8..16].to_vec());
                (kind, first)
            })
            .collect()
    })
}

/// A language system's priorities, compared with what was stated.
fn check_lang_sys(read: &JstfLangSys<'_>, stated: &[PrioritySpec], what: &str) {
    assert_eq!(read.len(), stated.len(), "{what}: the priority count");
    for (index, spec) in stated.iter().enumerate() {
        let priority = read.get(index).expect("a priority");
        assert_eq!(
            lists_of(&priority, Mode::Shrinkage),
            spec.shrinkage,
            "{what} {index}"
        );
        assert_eq!(
            lists_of(&priority, Mode::Extension),
            spec.extension,
            "{what} {index}"
        );
        assert_eq!(
            max_of(&priority, Mode::Shrinkage),
            stated_max(&spec.shrinkage_max),
            "{what} {index}: the shrinkage maximum"
        );
        assert_eq!(
            max_of(&priority, Mode::Extension),
            stated_max(&spec.extension_max),
            "{what} {index}: the extension maximum"
        );
    }
    assert!(read.get(stated.len()).is_none(), "{what}: past the count");
}

/// A script compared with what was stated.
fn check_script(read: &JstfScript<'_>, spec: &ScriptSpec) {
    let what = String::from_utf8_lossy(&spec.tag).to_string();
    let extenders = read.extender_glyphs().map(|glyphs| {
        (0..glyphs.len())
            .map(|i| glyphs.get(i).expect("in range"))
            .collect::<Vec<_>>()
    });
    assert_eq!(extenders, spec.extenders, "{what}: the extender glyphs");
    match (&spec.default, read.default_lang_sys()) {
        (Some(stated), Some(lang)) => check_lang_sys(&lang, stated, &format!("{what} default")),
        (None, None) => {}
        (stated, read) => panic!("{what}: the default is {stated:?} and read {read:?}"),
    }
    assert_eq!(read.len(), spec.languages.len());
    for (index, (tag, stated)) in spec.languages.iter().enumerate() {
        assert_eq!(read.tag(index), Some(Tag::new(tag)));
        let lang = read.get(index).expect("a language system");
        check_lang_sys(
            &lang,
            stated,
            &format!("{what} {}", String::from_utf8_lossy(tag)),
        );
        let by_tag = read
            .lang_sys(Some(Tag::new(tag)))
            .expect("found by its tag");
        check_lang_sys(&by_tag, stated, &format!("{what} by tag"));
    }
    // A language the script has no record for takes the default.
    match (&spec.default, read.lang_sys(Some(Tag::new(b"ZZZ ")))) {
        (Some(stated), Some(lang)) => check_lang_sys(&lang, stated, "the default by fallback"),
        (None, None) => {}
        (stated, read) => panic!("{what}: fallback {stated:?} read {read:?}"),
    }
}

#[test]
fn a_jstf_table_reads_back_as_assembled() {
    let scripts = assembled();
    let table = jstf(&scripts);
    let read = Jstf::parse(&table).expect("the table reads");
    assert_eq!(read.minor_version(), 0);
    assert_eq!(read.len(), scripts.len());
    assert!(!read.is_empty());
    for (index, spec) in scripts.iter().enumerate() {
        assert_eq!(read.tag(index), Some(Tag::new(&spec.tag)));
        check_script(&read.get(index).expect("a script"), spec);
        check_script(&read.find(Tag::new(&spec.tag)).expect("found by tag"), spec);
    }
    assert!(read.find(Tag::new(b"cyrl")).is_none());
    assert!(read.get(scripts.len()).is_none() && read.tag(scripts.len()).is_none());

    // The chapter's Example 4, through the lookup reader: the space widened
    // by up to 360 units.
    let arab = read.find(Tag::new(b"arab")).expect("arab");
    let kerning = arab
        .default_lang_sys()
        .and_then(|lang| lang.get(1))
        .expect("the second priority");
    let max = kerning.max(Mode::Extension).expect("a JstfMax");
    assert_eq!(max.len(), 1);
    let lookup = max.lookup(0).expect("its lookup");
    assert_eq!((lookup.kind(), lookup.flags(), lookup.len()), (1, 0, 1));
    let subtable = lookup.subtable(0).expect("its subtable");
    assert_eq!(subtable.u16(0), Some(1), "SinglePosFormat1");
    assert_eq!(subtable.u16(4), Some(0x0004), "XAdvance only");
    assert_eq!(subtable.i16(6), Some(360));
    assert!(kerning.max(Mode::Shrinkage).is_none(), "a NULL offset");

    // A face carries it beside the other three, and a Layout made from loose
    // tables takes it from `with_jstf`.
    let layout = Layout::from_tables(None, None, None).with_jstf(Some(&table));
    assert_eq!(layout.jstf().map(Jstf::len), Some(2));
    assert!(Layout::from_tables(None, None, None).jstf().is_none());
    assert!(Layout::from_tables(None, None, None)
        .with_jstf(None)
        .jstf()
        .is_none());
    let face = sfnt(&[(*b"JSTF", table.clone())]);
    let parsed = tinker_pdf_font::Sfnt::parse(&face).expect("a face");
    assert_eq!(Layout::parse(&parsed).jstf().map(Jstf::len), Some(2));
}

/// An sfnt holding the tables given, in the order given.
fn sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    push16(&mut out, u16::try_from(tables.len()).expect("few"));
    out.extend_from_slice(&[0; 6]);
    let mut at = 12 + 16 * tables.len();
    for (tag, bytes) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(at).expect("small").to_be_bytes());
        out.extend_from_slice(&u32::try_from(bytes.len()).expect("small").to_be_bytes());
        at += bytes.len().div_ceil(4) * 4;
    }
    for (_, bytes) in tables {
        out.extend_from_slice(bytes);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

/// `table` with the `u16` at `at` replaced.
fn with_u16(mut table: Vec<u8>, at: usize, value: u16) -> Vec<u8> {
    table[at..at + 2].copy_from_slice(&value.to_be_bytes());
    table
}

#[test]
fn an_offset_past_the_jstf_answers_none() {
    let table = jstf(&assembled());
    // Nothing to read, a header cut short, and a version the chapter does
    // not define.
    assert!(Jstf::parse(&[]).is_none());
    assert!(Jstf::parse(&table[..5]).is_none());
    assert!(Jstf::parse(&with_u16(table.clone(), 0, 2)).is_none());
    assert!(Jstf::parse(&with_u16(table.clone(), 0, 0)).is_none());
    assert!(Jstf::parse(&table[..6]).is_some(), "the header alone reads");

    // A script record whose offset passes the table, and a count past the
    // records.
    let past = with_u16(table.clone(), 10, 0xFFFF);
    let read = Jstf::parse(&past).expect("the header reads");
    assert_eq!(read.tag(0), Some(Tag::new(b"arab")));
    assert!(read.get(0).is_none());
    assert!(read.find(Tag::new(b"arab")).is_none());
    let counted = with_u16(table.clone(), 4, 0xFFFF);
    let read = Jstf::parse(&counted).expect("the header reads");
    assert_eq!(read.len(), 0xFFFF);
    assert!(read.tag(0xFFFE).is_none() && read.get(0xFFFE).is_none());
    assert!(
        read.find(Tag::new(b"cyrl")).is_none(),
        "a scan past the records stops"
    );
    let short = Jstf::parse(&table[..8]).expect("the header reads");
    assert!(short.tag(0).is_none() && short.get(0).is_none());

    // Inside the Arabic script: the extender table, the default language
    // system and the Farsi record each pointed past the table.
    let script_at = usize::from(u16::from_be_bytes([table[10], table[11]]));
    for (field, what) in [(0usize, "extender"), (2, "default"), (10, "FAR record")] {
        let bad = with_u16(table.clone(), script_at + field, 0xFFF0);
        let read = Jstf::parse(&bad).expect("the header reads");
        let arab = read.find(Tag::new(b"arab")).expect("the script reads");
        match what {
            "extender" => assert!(arab.extender_glyphs().is_none()),
            "default" => {
                assert!(arab.default_lang_sys().is_none());
                assert!(arab.lang_sys(None).is_none());
            }
            _ => {
                assert!(arab.get(0).is_none());
                assert!(arab.lang_sys(Some(Tag::new(b"FAR "))).is_none());
            }
        }
    }

    // The default language system's first priority pointed past the table,
    // and one cut short of its ten offsets.
    let arab = Jstf::parse(&table)
        .and_then(|t| t.find(Tag::new(b"arab")))
        .expect("arab");
    let default_at = script_at
        + usize::from(u16::from_be_bytes([
            table[script_at + 2],
            table[script_at + 3],
        ]));
    let bad = with_u16(table.clone(), default_at + 2, 0xFFF0);
    let read = Jstf::parse(&bad)
        .and_then(|t| t.find(Tag::new(b"arab")))
        .expect("arab");
    assert!(read.default_lang_sys().expect("reads").get(0).is_none());
    assert!(read.default_lang_sys().expect("reads").get(1).is_some());
    assert_eq!(arab.default_lang_sys().map(|lang| lang.len()), Some(2));
    let truncated = &table[..default_at + 2 + 2 * 2 + 10];
    let read = Jstf::parse(truncated)
        .and_then(|t| t.find(Tag::new(b"arab")))
        .expect("arab");
    assert!(
        read.default_lang_sys().expect("reads").get(0).is_none(),
        "a priority without its ten offsets"
    );

    // A priority's modification list and its JstfMax pointed past the
    // table, and a JstfMax lookup pointed past it.
    let priority_at = default_at
        + usize::from(u16::from_be_bytes([
            table[default_at + 4],
            table[default_at + 5],
        ]));
    let bad = with_u16(table.clone(), priority_at + 14, 0xFFF0);
    let read = Jstf::parse(&bad)
        .and_then(|t| t.find(Tag::new(b"arab")))
        .and_then(|s| s.default_lang_sys())
        .and_then(|l| l.get(1))
        .expect("the kerning priority");
    assert!(read
        .mod_list(Mode::Extension, Table::Gpos, Change::Enable)
        .is_none());
    assert!(
        read.mod_list(Mode::Shrinkage, Table::Gpos, Change::Disable)
            .is_some(),
        "the other lists still read"
    );
    let max_field = priority_at + 18;
    let bad = with_u16(table.clone(), max_field, 0xFFF0);
    let read = Jstf::parse(&bad)
        .and_then(|t| t.find(Tag::new(b"arab")))
        .and_then(|s| s.default_lang_sys())
        .and_then(|l| l.get(1))
        .expect("the kerning priority");
    assert!(read.max(Mode::Extension).is_none());
    let max_at =
        priority_at + usize::from(u16::from_be_bytes([table[max_field], table[max_field + 1]]));
    let bad = with_u16(table.clone(), max_at + 2, 0xFFF0);
    let read = Jstf::parse(&bad)
        .and_then(|t| t.find(Tag::new(b"arab")))
        .and_then(|s| s.default_lang_sys())
        .and_then(|l| l.get(1))
        .and_then(|p| p.max(Mode::Extension))
        .expect("the JstfMax reads");
    assert_eq!(read.len(), 1);
    assert!(read.lookup(0).is_none());

    // A modification list reads no lookup past its count.
    let everything_count = Jstf::parse(&table)
        .and_then(|t| t.find(Tag::new(b"latn")))
        .and_then(|s| s.get(0))
        .and_then(|l| l.get(0))
        .and_then(|p| p.mod_list(Mode::Shrinkage, Table::Gsub, Change::Enable))
        .expect("a list");
    assert_eq!(everything_count.len(), 1);
    assert!(everything_count.lookup(1).is_none(), "past its count");

    // Every truncation and every single-byte change of the table reads or
    // answers None, and never panics.
    for cut in 0..table.len() {
        walk(&table[..cut]);
    }
    for at in 0..table.len() {
        for value in [0x00, 0x01, 0x7F, 0x80, 0xFF] {
            let mut changed = table.clone();
            changed[at] = value;
            walk(&changed);
        }
    }
}

/// Reads every structure of a table's bytes, as the `shape` fuzz target
/// does, and counts the scripts and priorities it reached.
fn walk(bytes: &[u8]) -> usize {
    Jstf::parse(bytes).map_or(0, |jstf| walk_table(&jstf))
}

/// The same over a table already read.
fn walk_table(jstf: &Jstf<'_>) -> usize {
    let mut reached = 0usize;
    for index in 0..jstf.len().min(64) {
        let _ = jstf.tag(index);
        let Some(script) = jstf.get(index) else {
            continue;
        };
        reached += 1;
        if let Some(extenders) = script.extender_glyphs() {
            for at in 0..extenders.len().min(64) {
                let _ = extenders.get(at);
            }
        }
        let mut systems: Vec<JstfLangSys<'_>> = script.default_lang_sys().into_iter().collect();
        for lang in 0..script.len().min(64) {
            let _ = script.tag(lang);
            systems.extend(script.get(lang));
        }
        let _ = script.lang_sys(Some(Tag::new(b"FAR ")));
        for system in systems {
            for at in 0..system.len().min(64) {
                let Some(priority) = system.get(at) else {
                    continue;
                };
                reached += 1;
                for mode in [Mode::Shrinkage, Mode::Extension] {
                    for table in [Table::Gsub, Table::Gpos] {
                        for change in [Change::Enable, Change::Disable] {
                            if let Some(list) = priority.mod_list(mode, table, change) {
                                for lookup in 0..list.len().min(64) {
                                    let _ = list.lookup(lookup);
                                }
                            }
                        }
                    }
                    if let Some(max) = priority.max(mode) {
                        for lookup in 0..max.len().min(64) {
                            if let Some(lookup) = max.lookup(lookup) {
                                let _ = (lookup.kind(), lookup.flags());
                                for sub in 0..lookup.len().min(64) {
                                    let _ = lookup.subtable(sub);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    reached
}

/// The `shape` fuzz target's seeds that carry a `JSTF`: two control bytes,
/// a glyph run whose length is the second byte's top five bits, and the
/// rest, read by the target as a face and as each table.
fn jstf_seeds() -> Vec<(String, Vec<u8>)> {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/shape");
    let mut seeds = Vec::new();
    for entry in std::fs::read_dir(&base).expect("the seed corpus is committed") {
        let path = entry.expect("a readable entry").path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with("jstf-") {
            seeds.push((name, std::fs::read(&path).expect("a readable seed")));
        }
    }
    seeds.sort();
    seeds
}

/// Writes the `JSTF` seed of `fuzz/corpus/shape/`, so that the seed and the
/// table the tests above read cannot drift apart: the assembled table after
/// the target's two control bytes — roomy ceilings — and a run of four
/// glyphs, the first two the extender glyphs.
///
/// Run with `--ignored` when the table changes; the corpus is committed, and
/// a run that rewrites it is a diff to look at.
#[test]
#[ignore = "writes into fuzz/corpus/, which is committed"]
fn write_the_jstf_fuzz_seed() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/shape");
    std::fs::write(base.join("jstf-every-structure"), jstf_seed()).expect("the seed is writable");
}

/// The seed's bytes.
fn jstf_seed() -> Vec<u8> {
    // Roomy limits, as `synthetic.rs`'s seeds choose them: nesting 3,
    // extensions 3, context 64, 500 000 operations, 65 536 glyphs.
    const KNOBS: u8 = 3 | (3 << 2) | (2 << 4) | (3 << 6);
    let glyphs: [u16; 4] = [0x01D3, 0x01D4, 0x22, 5];
    let mut out = vec![KNOBS, 3 | (4 << 3)];
    for glyph in glyphs {
        push16(&mut out, glyph);
    }
    out.extend(jstf(&assembled()));
    out
}

#[test]
fn every_jstf_fuzz_seed_shapes_without_panicking() {
    let seeds = jstf_seeds();
    assert_eq!(
        seeds
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["jstf-every-structure"],
        "the JSTF seeds changed"
    );
    for (name, bytes) in &seeds {
        let glyph_count = usize::from(bytes[1] >> 3);
        let glyphs: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .take(glyph_count)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        let face = &bytes[2 + glyph_count * 2..];
        // It reaches the table it was written for: every script and every
        // priority of it.
        assert!(
            walk(face) >= 6,
            "the seed {name} reaches the JSTF it carries"
        );
        for limits in [
            Limits::DEFAULT,
            Limits {
                max_nesting_depth: 0,
                max_extension_depth: 0,
                max_context_length: 1,
                max_operations: 1,
                max_glyphs: 1,
                max_index_bytes: 0,
                max_table_bytes: 0,
                max_attribute_runs: 0,
                max_passes: 0,
                max_rule_candidates: 0,
                max_transitions: 0,
                max_subtables: 0,
                max_feature_entries: 0,
            },
        ] {
            let layouts = [
                tinker_pdf_font::Sfnt::parse(face).map(|sfnt| Layout::parse(&sfnt)),
                Some(Layout::from_tables(Some(face), Some(face), Some(face)).with_jstf(Some(face))),
            ];
            for layout in layouts.into_iter().flatten() {
                if let Some(jstf) = layout.jstf() {
                    let _ = walk_table(jstf);
                }
                let run = || {
                    let mut buffer = Buffer::from_glyphs(&glyphs);
                    let mut warnings = Vec::new();
                    if let Some(gsub) = layout.gsub() {
                        let lookups =
                            gsub.lookups_for(Tag::new(b"arab"), None, &[Tag::new(b"liga")]);
                        warnings.extend(layout.substitute(&mut buffer, &lookups, limits));
                    }
                    if let Some(gpos) = layout.gpos() {
                        let lookups =
                            gpos.lookups_for(Tag::new(b"arab"), None, &[Tag::new(b"kern")]);
                        warnings.extend(layout.position(
                            &mut buffer,
                            &lookups,
                            limits,
                            MarkWidths::ZeroByGdef,
                        ));
                    }
                    (buffer.glyphs().to_vec(), warnings)
                };
                assert!(run() == run(), "{name}: shaping is not deterministic");
            }
        }
    }
    // The committed seed is the one the writer above writes.
    assert_eq!(
        seeds.first().map(|(_, bytes)| bytes.clone()),
        Some(jstf_seed()),
        "the committed JSTF seed is not the writer's: run write_the_jstf_fuzz_seed"
    );
}
