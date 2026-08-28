//! The lookups and structures aots has no fixture for, against tables this
//! repository assembles itself.
//!
//! `tests/aots.rs` is the stronger suite and says so: its expectations were
//! written by the people who wrote the specification, so it can catch a
//! misreading. **This file cannot.** The same author wrote the table and the
//! reader, so it can only catch a disagreement between them — which is why
//! every test here is built to fail loudly if the code it exercises were
//! deleted, and why `aots_does_not_reach_these` in the other file names
//! exactly what has to be covered here rather than leaving the division to
//! memory.
//!
//! What that buys, and it is not nothing: a hand-built table is the only way
//! to reach a case no shipped font contains — a lookup that names itself, an
//! extension subtable pointing at another extension, a substitution that
//! would grow a buffer past its ceiling. Those are the cases ruling 1 is
//! about, and no corpus will ever hold one.
//!
//! # What is covered here, and why aots does not reach it
//!
//! | Structure | Why not aots |
//! |---|---|
//! | `GSUB` type 3, alternate substitution | aots reaches it only through three cases that pick a *different* alternate at each position, which needs a feature-value API this milestone does not have |
//! | `GSUB` type 8, reverse chaining | aots has no fixture at all |
//! | `GDEF` `AttachList` | no aots fixture carries one |
//! | `GDEF` `LigCaretList` | no aots fixture carries one |
//! | `GDEF` mark glyph sets, and `USE_MARK_FILTERING_SET` | no aots fixture carries one |
//! | `GDEF` mark attachment classes | three aots fonts carry the table; none has a lookup that filters by it |
//! | Extension depth, self-naming lookups, the two budgets | a conforming font never does any of this |

use tinker_pdf_shape::{
    Buffer, Caret, Gdef, GlyphClass, Layout, Limits, MarkWidths, Table, Tag, Warning,
};

// --- a very small table assembler ---------------------------------------
//
// Enough of one to write a lookup by hand and no more. Every OpenType Layout
// table is a header of counts and `Offset16`s followed by the things they
// point at, so `assemble` is the whole of it: it appends the parts and
// patches the header slots with where each landed.

fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A header, with `slots[i]` patched to the offset of `parts[i]`.
///
/// Slots not listed keep whatever the header put there, which for an
/// `Offset16` is the zero that means "absent".
fn assemble(mut head: Vec<u8>, slots: &[usize], parts: &[Vec<u8>]) -> Vec<u8> {
    assert_eq!(slots.len(), parts.len(), "a slot for every part");
    let mut body: Vec<u8> = Vec::new();
    for (slot, part) in slots.iter().zip(parts) {
        let offset = u16::try_from(head.len() + body.len()).expect("a table under 64 KB");
        head[*slot..*slot + 2].copy_from_slice(&offset.to_be_bytes());
        body.extend_from_slice(part);
    }
    head.extend(body);
    head
}

/// Coverage format 1: the glyphs, in order.
fn coverage(glyphs: &[u16]) -> Vec<u8> {
    let mut out = u16s(&[1, u16::try_from(glyphs.len()).expect("a small coverage")]);
    out.extend(u16s(glyphs));
    out
}

/// ClassDef format 2: `(first, last, class)` ranges, in order.
fn class_def(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
    let mut out = u16s(&[2, u16::try_from(ranges.len()).expect("a small class def")]);
    for (first, last, class) in ranges {
        out.extend(u16s(&[*first, *last, *class]));
    }
    out
}

/// One lookup: a type, flags, its subtables, and a mark filtering set.
fn lookup(kind: u16, flags: u16, subtables: &[Vec<u8>], filtering_set: Option<u16>) -> Vec<u8> {
    let count = u16::try_from(subtables.len()).expect("a small lookup");
    let mut head = u16s(&[kind, flags, count]);
    head.extend(vec![0u8; subtables.len() * 2]);
    if let Some(set) = filtering_set {
        head.extend(u16s(&[set]));
    }
    let slots: Vec<usize> = (0..subtables.len()).map(|i| 6 + i * 2).collect();
    assemble(head, &slots, subtables)
}

/// A `GSUB` or `GPOS` table with one script `latn`, one feature `test`, and
/// every lookup behind it.
fn layout_table(lookups: &[Vec<u8>], variations: Option<Vec<u8>>) -> Vec<u8> {
    let count = u16::try_from(lookups.len()).expect("a small lookup list");

    let lang_sys = u16s(&[0, 0xFFFF, 1, 0]);
    let script = assemble(u16s(&[0, 0]), &[0], &[lang_sys]);
    let mut script_head = u16s(&[1]);
    script_head.extend_from_slice(b"latn");
    script_head.extend(u16s(&[0]));
    let script_list = assemble(script_head, &[6], &[script]);

    let mut feature = u16s(&[0, count]);
    feature.extend(u16s(&(0..count).collect::<Vec<u16>>()));
    let mut feature_head = u16s(&[1]);
    feature_head.extend_from_slice(b"test");
    feature_head.extend(u16s(&[0]));
    let feature_list = assemble(feature_head, &[6], &[feature]);

    let mut list_head = u16s(&[count]);
    list_head.extend(vec![0u8; lookups.len() * 2]);
    let slots: Vec<usize> = (0..lookups.len()).map(|i| 2 + i * 2).collect();
    let lookup_list = assemble(list_head, &slots, lookups);

    match variations {
        None => assemble(
            u16s(&[1, 0, 0, 0, 0]),
            &[4, 6, 8],
            &[script_list, feature_list, lookup_list],
        ),
        Some(variations) => {
            // Version 1.1 adds an `Offset32` at the end, so the three
            // `Offset16`s are patched as usual and the wide one by hand.
            let mut head = u16s(&[1, 1, 0, 0, 0]);
            head.extend(vec![0u8; 4]);
            let mut table = assemble(head, &[4, 6, 8], &[script_list, feature_list, lookup_list]);
            let offset = u32::try_from(table.len()).expect("a table under 4 GB");
            table[10..14].copy_from_slice(&offset.to_be_bytes());
            table.extend(variations);
            table
        }
    }
}

/// `SingleSubstFormat2`: an explicit replacement for each covered glyph.
fn single_subst(map: &[(u16, u16)]) -> Vec<u8> {
    let glyphs: Vec<u16> = map.iter().map(|(from, _)| *from).collect();
    let subs: Vec<u16> = map.iter().map(|(_, to)| *to).collect();
    let mut head = u16s(&[2, 0, u16::try_from(subs.len()).expect("a small subst")]);
    head.extend(u16s(&subs));
    assemble(head, &[2], &[coverage(&glyphs)])
}

/// `MultipleSubstFormat1`, one sequence.
fn multiple_subst(from: u16, to: &[u16]) -> Vec<u8> {
    let mut sequence = u16s(&[u16::try_from(to.len()).expect("a small sequence")]);
    sequence.extend(u16s(to));
    assemble(u16s(&[1, 0, 1, 0]), &[2, 6], &[coverage(&[from]), sequence])
}

/// `AlternateSubstFormat1`, one alternate set.
fn alternate_subst(from: u16, alternates: &[u16]) -> Vec<u8> {
    let mut set = u16s(&[u16::try_from(alternates.len()).expect("a small set")]);
    set.extend(u16s(alternates));
    assemble(u16s(&[1, 0, 1, 0]), &[2, 6], &[coverage(&[from]), set])
}

/// `LigatureSubstFormat1`, one ligature.
fn ligature_subst(first: u16, rest: &[u16], out: u16) -> Vec<u8> {
    let mut ligature = u16s(&[
        out,
        u16::try_from(rest.len() + 1).expect("a small ligature"),
    ]);
    ligature.extend(u16s(rest));
    let ligature_set = assemble(u16s(&[1, 0]), &[2], &[ligature]);
    assemble(
        u16s(&[1, 0, 1, 0]),
        &[2, 6],
        &[coverage(&[first]), ligature_set],
    )
}

/// `SequenceContextFormat3`: one rule, a coverage per position.
fn context_format3(positions: &[Vec<u16>], records: &[(u16, u16)]) -> Vec<u8> {
    let glyphs = u16::try_from(positions.len()).expect("a short rule");
    let count = u16::try_from(records.len()).expect("a short rule");
    let mut head = u16s(&[3, glyphs, count]);
    head.extend(vec![0u8; positions.len() * 2]);
    for (sequence, lookup) in records {
        head.extend(u16s(&[*sequence, *lookup]));
    }
    let slots: Vec<usize> = (0..positions.len()).map(|i| 6 + i * 2).collect();
    let parts: Vec<Vec<u8>> = positions.iter().map(|glyphs| coverage(glyphs)).collect();
    assemble(head, &slots, &parts)
}

/// `ReverseChainSingleSubstFormat1`.
fn reverse_chain(map: &[(u16, u16)], backtrack: &[Vec<u16>], lookahead: &[Vec<u16>]) -> Vec<u8> {
    let glyphs: Vec<u16> = map.iter().map(|(from, _)| *from).collect();
    let subs: Vec<u16> = map.iter().map(|(_, to)| *to).collect();
    let mut head = u16s(&[1, 0, u16::try_from(backtrack.len()).expect("short")]);
    head.extend(vec![0u8; backtrack.len() * 2]);
    head.extend(u16s(&[u16::try_from(lookahead.len()).expect("short")]));
    head.extend(vec![0u8; lookahead.len() * 2]);
    head.extend(u16s(&[u16::try_from(subs.len()).expect("short")]));
    head.extend(u16s(&subs));

    let mut slots = vec![2usize];
    slots.extend((0..backtrack.len()).map(|i| 6 + i * 2));
    let lookahead_at = 6 + backtrack.len() * 2 + 2;
    slots.extend((0..lookahead.len()).map(|i| lookahead_at + i * 2));
    let mut parts = vec![coverage(&glyphs)];
    parts.extend(backtrack.iter().map(|glyphs| coverage(glyphs)));
    parts.extend(lookahead.iter().map(|glyphs| coverage(glyphs)));
    assemble(head, &slots, &parts)
}

/// An extension subtable pointing at `inner`, declaring it to be of type
/// `kind`.
fn extension(kind: u16, inner: Vec<u8>) -> Vec<u8> {
    let mut out = u16s(&[1, kind]);
    out.extend(8u32.to_be_bytes());
    out.extend(inner);
    out
}

/// `depth` nested extension subtables around one single substitution.
///
/// The innermost declares the real type; every one outside it declares the
/// extension type, which is what 14496-22 forbids and what
/// [`Warning::ExtensionChained`] reports.
fn extension_chain(depth: usize, inner: Vec<u8>) -> Vec<u8> {
    let mut table = inner;
    let mut kind = 1;
    for _ in 0..depth {
        table = extension(kind, table);
        kind = 7;
    }
    table
}

/// A `GDEF` 1.2 with whichever of the five structures are wanted.
fn gdef(
    classes: Option<Vec<u8>>,
    attachments: Option<Vec<u8>>,
    carets: Option<Vec<u8>>,
    mark_attach: Option<Vec<u8>>,
    mark_sets: Option<Vec<u8>>,
) -> Vec<u8> {
    let head = u16s(&[1, 2, 0, 0, 0, 0, 0]);
    let mut slots = Vec::new();
    let mut parts = Vec::new();
    for (slot, part) in [
        (4usize, classes),
        (6, attachments),
        (8, carets),
        (10, mark_attach),
        (12, mark_sets),
    ] {
        if let Some(part) = part {
            slots.push(slot);
            parts.push(part);
        }
    }
    assemble(head, &slots, &parts)
}

/// A `MarkGlyphSetsDef`, whose coverage offsets are 32 bits wide where every
/// other offset in `GDEF` is 16 — the one place in the table where reading
/// the wrong width silently halves every offset in the array.
fn mark_glyph_sets(sets: &[Vec<u16>]) -> Vec<u8> {
    let mut head = u16s(&[1, u16::try_from(sets.len()).expect("a few sets")]);
    head.extend(vec![0u8; sets.len() * 4]);
    let mut body: Vec<u8> = Vec::new();
    for (index, set) in sets.iter().enumerate() {
        let offset = u32::try_from(head.len() + body.len()).expect("a small table");
        let at = 4 + index * 4;
        head[at..at + 4].copy_from_slice(&offset.to_be_bytes());
        body.extend(coverage(set));
    }
    head.extend(body);
    head
}

/// An `AttachList`: per glyph, the outline points a mark may attach at.
fn attach_list(entries: &[(u16, Vec<u16>)]) -> Vec<u8> {
    let glyphs: Vec<u16> = entries.iter().map(|(glyph, _)| *glyph).collect();
    let mut head = u16s(&[0, u16::try_from(entries.len()).expect("a few glyphs")]);
    head.extend(vec![0u8; entries.len() * 2]);
    let mut slots = vec![0usize];
    let mut parts = vec![coverage(&glyphs)];
    for (index, (_, points)) in entries.iter().enumerate() {
        slots.push(4 + index * 2);
        let mut part = u16s(&[u16::try_from(points.len()).expect("a few points")]);
        part.extend(u16s(points));
        parts.push(part);
    }
    assemble(head, &slots, &parts)
}

/// A `LigCaretList` whose carets are given in the three formats the table
/// allows: a coordinate, an outline point, and a coordinate with a device
/// table.
fn lig_caret_list(glyph: u16) -> Vec<u8> {
    let device = u16s(&[11, 12, 1, 0b1100_0000_0000_0000]);
    let format1 = u16s(&[1, 250]);
    let format2 = u16s(&[2, 7]);
    let format3 = assemble(u16s(&[3, 500, 0]), &[4], &[device]);

    let mut lig_head = u16s(&[3]);
    lig_head.extend(vec![0u8; 6]);
    let lig = assemble(lig_head, &[2, 4, 6], &[format1, format2, format3]);

    let mut head = u16s(&[0, 1]);
    head.extend(vec![0u8; 2]);
    assemble(head, &[0, 4], &[coverage(&[glyph]), lig])
}

// --- the harness --------------------------------------------------------

fn substitute(gdef: Option<&[u8]>, gsub: &[u8], input: &[u16]) -> (Vec<u16>, Vec<Warning>) {
    substitute_with(gdef, gsub, input, Limits::DEFAULT)
}

fn substitute_with(
    gdef: Option<&[u8]>,
    gsub: &[u8],
    input: &[u16],
    limits: Limits,
) -> (Vec<u16>, Vec<Warning>) {
    let layout = Layout::from_tables(gdef, Some(gsub), None);
    let table = layout.gsub().expect("a readable GSUB");
    let lookups = table.lookups_for(Tag::new(b"latn"), None, &[Tag::new(b"test")]);
    let mut buffer = Buffer::from_glyphs(input);
    let warnings = layout.substitute(&mut buffer, &lookups, limits);
    (
        buffer.glyphs().iter().map(|glyph| glyph.glyph).collect(),
        warnings,
    )
}

// --- GSUB type 3: alternate substitution --------------------------------

#[test]
fn an_alternate_substitution_takes_the_first_alternate() {
    let gsub = layout_table(
        &[lookup(3, 0, &[alternate_subst(10, &[20, 21, 22])], None)],
        None,
    );
    let (glyphs, warnings) = substitute(None, &gsub, &[9, 10, 11]);
    assert_eq!(
        glyphs,
        vec![9, 20, 11],
        "the first alternate is the default"
    );
    assert!(warnings.is_empty());
}

#[test]
fn an_alternate_set_that_is_empty_substitutes_nothing() {
    let gsub = layout_table(&[lookup(3, 0, &[alternate_subst(10, &[])], None)], None);
    let (glyphs, warnings) = substitute(None, &gsub, &[9, 10, 11]);
    assert_eq!(glyphs, vec![9, 10, 11]);
    assert!(warnings.is_empty(), "an empty set is legal, not malformed");
}

// --- GSUB type 8: reverse chaining single substitution ------------------

/// The test that separates a reverse chaining lookup from a forward one.
///
/// The rule replaces glyph 1 with glyph 2 when a glyph 1 follows it. Run from
/// the end, the last glyph has nothing after it and is left alone, the middle
/// one sees an untouched glyph 1 and is replaced, and the first then sees the
/// glyph 2 the middle one became and is *not*. Run from the front — which is
/// what a shaper that treated this as an ordinary lookup would do — the first
/// two are both replaced and the answer is different.
///
/// This is the whole reason 14496-22 makes this lookup type run backwards,
/// and it is why the assertion below is `[1, 2, 1]` rather than `[2, 2, 1]`.
#[test]
fn a_reverse_chaining_lookup_reads_a_lookahead_it_has_not_rewritten() {
    let gsub = layout_table(
        &[lookup(
            8,
            0,
            &[reverse_chain(&[(1, 2)], &[], &[vec![1]])],
            None,
        )],
        None,
    );
    let (glyphs, warnings) = substitute(None, &gsub, &[1, 1, 1]);
    assert_eq!(
        glyphs,
        vec![1, 2, 1],
        "a forward pass would have produced [2, 2, 1]"
    );
    assert!(warnings.is_empty());
}

#[test]
fn a_reverse_chaining_lookup_honours_its_backtrack() {
    let gsub = layout_table(
        &[lookup(
            8,
            0,
            &[reverse_chain(&[(1, 2)], &[vec![5]], &[])],
            None,
        )],
        None,
    );
    let (matched, _) = substitute(None, &gsub, &[5, 1]);
    assert_eq!(matched, vec![5, 2], "the backtrack was there");
    let (missed, _) = substitute(None, &gsub, &[6, 1]);
    assert_eq!(missed, vec![6, 1], "the backtrack was not");
}

// --- GDEF: the two lists nothing in this crate reads --------------------

#[test]
fn attachment_points_come_back_in_order() {
    let bytes = gdef(
        None,
        Some(attach_list(&[(3, vec![1, 4, 9]), (7, vec![2])])),
        None,
        None,
        None,
    );
    let gdef = Gdef::parse(&bytes).expect("a version 1 table");
    let attachments = gdef.attachments().expect("an attach list");
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments.points(3), vec![1, 4, 9]);
    assert_eq!(attachments.points(7), vec![2]);
    assert!(
        attachments.points(4).is_empty(),
        "a glyph outside the coverage has no points"
    );
}

#[test]
fn ligature_carets_come_back_in_all_three_formats() {
    let bytes = gdef(None, None, Some(lig_caret_list(12)), None, None);
    let gdef = Gdef::parse(&bytes).expect("a version 1 table");
    let carets = gdef.ligature_carets().expect("a caret list");
    assert_eq!(carets.len(), 1);
    let found = carets.carets(12);
    assert_eq!(
        found.len(),
        3,
        "a three-component ligature has three carets"
    );
    match found[0] {
        Caret::Coordinate { x, device } => {
            assert_eq!(x, 250);
            assert!(device.is_none(), "format 1 carries no device table");
        }
        Caret::Point { .. } => panic!("format 1 is a coordinate"),
    }
    match found[1] {
        Caret::Point { point } => assert_eq!(point, 7),
        Caret::Coordinate { .. } => panic!("format 2 is an outline point"),
    }
    match found[2] {
        Caret::Coordinate { x, device } => {
            assert_eq!(x, 500);
            let device = device.expect("format 3 carries a device table");
            assert_eq!(device.delta(11), -1, "the hinted correction at 11 ppem");
            assert_eq!(device.delta(20), 0, "and nothing outside the range");
        }
        Caret::Point { .. } => panic!("format 3 is a coordinate"),
    }
    assert!(carets.carets(13).is_empty());
}

// --- GDEF: the two filters that make a lookup flag mean something -------

/// Glyphs 5 and 6 are both marks; 5 is in mark glyph set 0 and 6 is not.
fn mark_gdef() -> Vec<u8> {
    gdef(
        Some(class_def(&[(5, 6, GlyphClass::Mark.value())])),
        None,
        None,
        Some(class_def(&[(5, 5, 1), (6, 6, 2)])),
        Some(mark_glyph_sets(&[vec![5]])),
    )
}

#[test]
fn a_mark_filtering_set_hides_the_marks_outside_it() {
    let classes = mark_gdef();
    let gsub = layout_table(
        &[lookup(
            4,
            0x0010, // USE_MARK_FILTERING_SET
            &[ligature_subst(1, &[2], 9)],
            Some(0),
        )],
        None,
    );
    // Glyph 5 is *in* the set, so the lookup can see it and the two
    // components are not adjacent.
    let (blocked, _) = substitute(Some(&classes), &gsub, &[1, 5, 2]);
    assert_eq!(blocked, vec![1, 5, 2], "a mark inside the set is visible");
    // Glyph 6 is outside it, so the lookup steps over it and ligates.
    let (ligated, _) = substitute(Some(&classes), &gsub, &[1, 6, 2]);
    assert_eq!(ligated, vec![9, 6], "a mark outside the set is skipped");
}

#[test]
fn a_mark_attachment_class_hides_the_marks_of_other_classes() {
    let classes = mark_gdef();
    let gsub = layout_table(
        &[lookup(
            4,
            0x0100, // markAttachmentType 1, in the high byte
            &[ligature_subst(1, &[2], 9)],
            None,
        )],
        None,
    );
    let (blocked, _) = substitute(Some(&classes), &gsub, &[1, 5, 2]);
    assert_eq!(blocked, vec![1, 5, 2], "class 1 is the class asked for");
    let (ligated, _) = substitute(Some(&classes), &gsub, &[1, 6, 2]);
    assert_eq!(ligated, vec![9, 6], "class 2 is not, so it is skipped");
}

#[test]
fn a_mark_glyph_set_is_read_with_thirty_two_bit_offsets() {
    let bytes = mark_gdef();
    let gdef = Gdef::parse(&bytes).expect("a version 1.2 table");
    assert_eq!(gdef.mark_glyph_set_count(), 1);
    let set = gdef.mark_glyph_set(0).expect("set 0");
    assert!(set.covers(5));
    assert!(!set.covers(6));
    assert!(gdef.mark_glyph_set(1).is_none());
    assert_eq!(gdef.mark_attach_class(5), 1);
    assert_eq!(gdef.mark_attach_class(6), 2);
    assert_eq!(gdef.glyph_class(5), GlyphClass::Mark);
}

// --- what a conforming font never does ----------------------------------

#[test]
fn an_extension_resolves_transparently() {
    let gsub = layout_table(
        &[lookup(
            7,
            0,
            &[extension_chain(1, single_subst(&[(1, 2)]))],
            None,
        )],
        None,
    );
    let (glyphs, warnings) = substitute(None, &gsub, &[1]);
    assert_eq!(glyphs, vec![2]);
    assert!(warnings.is_empty(), "one hop is what the format is for");
}

#[test]
fn an_extension_that_names_the_extension_type_is_followed_and_reported() {
    let gsub = layout_table(
        &[lookup(
            7,
            0,
            &[extension_chain(2, single_subst(&[(1, 2)]))],
            None,
        )],
        None,
    );
    let (glyphs, warnings) = substitute(None, &gsub, &[1]);
    assert_eq!(glyphs, vec![2], "followed, because refusing would be worse");
    assert_eq!(
        warnings,
        vec![Warning::ExtensionChained {
            table: Table::Gsub,
            lookup: 0
        }],
        "and reported, because 14496-22 forbids it"
    );
}

#[test]
fn an_extension_chain_stops_at_the_cap() {
    let gsub = layout_table(
        &[lookup(
            7,
            0,
            &[extension_chain(6, single_subst(&[(1, 2)]))],
            None,
        )],
        None,
    );
    let (glyphs, warnings) = substitute(None, &gsub, &[1]);
    assert_eq!(glyphs, vec![1], "the subtable was never reached");
    assert!(
        warnings.contains(&Warning::ExtensionTooDeep {
            table: Table::Gsub,
            lookup: 0
        }),
        "got {warnings:?}"
    );
}

/// A contextual lookup whose rule names itself.
///
/// Nothing in the table says this is wrong and nothing can: a lookup naming
/// itself at a *different* position is a legitimate and common construction,
/// so the only thing that ends this is the depth cap.
#[test]
fn a_lookup_that_names_itself_terminates() {
    let gsub = layout_table(
        &[lookup(
            5,
            0,
            &[context_format3(&[vec![1]], &[(0, 0)])],
            None,
        )],
        None,
    );
    let (glyphs, warnings) = substitute(None, &gsub, &[1, 1]);
    assert_eq!(
        glyphs,
        vec![1, 1],
        "nothing was substituted, and it stopped"
    );
    assert!(
        warnings.contains(&Warning::NestingTooDeep {
            table: Table::Gsub,
            lookup: 0
        }),
        "got {warnings:?}"
    );
}

#[test]
fn a_rule_that_names_a_lookup_that_is_not_there_is_reported() {
    let gsub = layout_table(
        &[lookup(
            5,
            0,
            &[context_format3(&[vec![1]], &[(0, 9)])],
            None,
        )],
        None,
    );
    let (glyphs, warnings) = substitute(None, &gsub, &[1]);
    assert_eq!(glyphs, vec![1]);
    assert_eq!(
        warnings,
        vec![Warning::MissingLookup {
            table: Table::Gsub,
            lookup: 9
        }]
    );
}

#[test]
fn the_glyph_budget_refuses_growth_rather_than_growing() {
    let gsub = layout_table(
        &[lookup(2, 0, &[multiple_subst(1, &[7, 8, 9, 10])], None)],
        None,
    );
    let roomy = substitute(None, &gsub, &[1]).0;
    assert_eq!(roomy, vec![7, 8, 9, 10], "the substitution itself works");

    let limits = Limits {
        max_glyphs: 3,
        ..Limits::DEFAULT
    };
    let (glyphs, warnings) = substitute_with(None, &gsub, &[1], limits);
    assert_eq!(glyphs, vec![1], "the buffer was left as it stood");
    assert!(
        warnings.contains(&Warning::GlyphBudgetExceeded { table: Table::Gsub }),
        "got {warnings:?}"
    );
}

#[test]
fn the_operation_budget_stops_the_run() {
    let gsub = layout_table(&[lookup(1, 0, &[single_subst(&[(1, 2)])], None)], None);
    let limits = Limits {
        max_operations: 1,
        ..Limits::DEFAULT
    };
    let (glyphs, warnings) = substitute_with(None, &gsub, &[1, 1, 1], limits);
    assert_eq!(
        glyphs,
        vec![2, 1, 1],
        "the first application ran and the rest were refused"
    );
    assert!(
        warnings.contains(&Warning::OperationBudgetExceeded { table: Table::Gsub }),
        "got {warnings:?}"
    );
}

#[test]
fn a_lookup_of_an_unknown_type_is_named_rather_than_ignored() {
    let gsub = layout_table(&[lookup(11, 0, &[u16s(&[1, 0])], None)], None);
    let (glyphs, warnings) = substitute(None, &gsub, &[1]);
    assert_eq!(glyphs, vec![1]);
    assert_eq!(
        warnings,
        vec![Warning::UnknownLookupType {
            table: Table::Gsub,
            lookup: 0,
            kind: 11
        }]
    );
}

// --- feature selection, and the table that is parsed but not applied ----

#[test]
fn a_feature_nobody_asked_for_contributes_no_lookups() {
    let gsub = layout_table(&[lookup(1, 0, &[single_subst(&[(1, 2)])], None)], None);
    let layout = Layout::from_tables(None, Some(&gsub), None);
    let table = layout.gsub().expect("a readable GSUB");
    assert_eq!(
        table.lookups_for(Tag::new(b"latn"), None, &[Tag::new(b"liga")]),
        Vec::<u16>::new()
    );
    assert_eq!(
        table.lookups_for(Tag::new(b"latn"), None, &[Tag::new(b"test")]),
        vec![0]
    );
    // A script the face does not declare falls back to `DFLT`, and this face
    // declares no `DFLT` either, so nothing runs.
    assert_eq!(
        table.lookups_for(Tag::new(b"arab"), None, &[Tag::new(b"test")]),
        Vec::<u16>::new()
    );
}

#[test]
fn feature_variations_are_surfaced_and_not_applied() {
    // One record: a condition on axis 0 over the top half of its range, and a
    // substitution of feature 0 by a feature that runs lookup 0 twice.
    let condition = u16s(&[1, 0, 0x2000, 0x4000]);
    let mut condition_set = u16s(&[1]);
    condition_set.extend(vec![0u8; 4]);
    let offset = u32::try_from(condition_set.len()).expect("small");
    condition_set[2..6].copy_from_slice(&offset.to_be_bytes());
    condition_set.extend(condition);

    let alternate_feature = u16s(&[0, 2, 0, 0]);
    let mut substitution = u16s(&[1, 0, 1, 0]);
    substitution.extend(vec![0u8; 4]);
    let offset = u32::try_from(substitution.len()).expect("small");
    substitution[8..12].copy_from_slice(&offset.to_be_bytes());
    substitution.extend(alternate_feature);

    let mut variations = u16s(&[1, 0]);
    variations.extend(1u32.to_be_bytes());
    variations.extend(vec![0u8; 8]);
    let at = u32::try_from(variations.len()).expect("small");
    variations[8..12].copy_from_slice(&at.to_be_bytes());
    let at = at + u32::try_from(condition_set.len()).expect("small");
    variations[12..16].copy_from_slice(&at.to_be_bytes());
    variations.extend(condition_set);
    variations.extend(substitution);

    let gsub = layout_table(
        &[lookup(1, 0, &[single_subst(&[(1, 2)])], None)],
        Some(variations),
    );
    let layout = Layout::from_tables(None, Some(&gsub), None);
    let table = layout.gsub().expect("a readable GSUB");
    assert_eq!(table.minor_version(), 1);
    let found = table.feature_variations().expect("a variations table");
    assert_eq!(found.len(), 1);

    let conditions = found.conditions(0).expect("a condition set");
    assert_eq!(conditions.len(), 1);
    let condition = conditions.get(0).expect("a format 1 condition");
    assert_eq!(condition.axis, 0);
    // F2DOT14, handed back raw: 0x2000 is a half and 0x4000 is one, and
    // dividing them is the first float this crate would contain.
    assert_eq!((condition.min, condition.max), (0x2000, 0x4000));

    let substitutions = found.substitutions(0).expect("a substitution table");
    assert_eq!(substitutions.len(), 1);
    let (index, feature) = substitutions.get(0).expect("one substitution");
    assert_eq!(index, 0);
    assert_eq!(feature.len(), 2);

    // Surfaced, and not applied: the run is what the unvaried feature says.
    let (glyphs, warnings) = substitute(None, &gsub, &[1]);
    assert_eq!(glyphs, vec![2]);
    assert!(warnings.is_empty());
}

// --- the fuzz corpus ----------------------------------------------------

/// Writes the seeds `fuzz/corpus/shape/` carries, so the seeds and the tables
/// the tests above assert against cannot drift apart.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
/// This is the arrangement `crypt`, `cff` and `pki_der` already use, and the
/// reason is the one those files record: a hand-laid corpus that no longer
/// reaches what it was chosen for looks exactly like one that does.
///
/// # The layout a seed has to have
///
/// `fuzz/fuzz_targets/shape.rs` reads two control bytes, then a glyph run,
/// then the face. The control bytes here choose **roomy** ceilings on purpose:
/// a seed exists to reach the code, and the mutator will find the small
/// ceilings on its own within a few hundred iterations by flipping two bits.
///
/// Eight seeds, each chosen for a region a mutation is unlikely to reach on
/// its own — four real faces from the aots corpus, and four tables assembled
/// here for the cases no conforming font contains.
#[test]
#[ignore = "writes into fuzz/corpus/, which is committed"]
fn write_the_fuzz_seeds() {
    /// Roomy limits: nesting 3, extensions 3, context 64, 500 000 operations.
    const KNOBS: u8 = 3 | (3 << 2) | (2 << 4) | (3 << 6);

    fn seed(glyphs: &[u16], face: &[u8]) -> Vec<u8> {
        let count = u8::try_from(glyphs.len()).expect("at most 31 glyphs");
        assert!(count < 32, "the glyph count is five bits wide");
        let mut out = vec![KNOBS, 3 | (count << 3)];
        out.extend(u16s(glyphs));
        out.extend_from_slice(face);
        out
    }

    fn aots(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data/aots/fonts")
            .join(format!("{name}.otf"));
        std::fs::read(&path).expect("an aots fixture")
    }

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/shape");
    std::fs::create_dir_all(&base).expect("the corpus directory is creatable");

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        // Four real faces, one per region of the executor: a ligature that
        // steps over marks, a mark attached to a base, a chaining rule with
        // backtrack and lookahead, and an extension subtable.
        (
            "aots-ligature",
            seed(
                &[17, 18, 19, 20, 17, 18, 19, 22, 20],
                &aots("gsub4_1_simple_f1"),
            ),
        ),
        (
            "aots-mark-to-base",
            seed(&[17, 18, 19, 17], &aots("gpos4_simple_1")),
        ),
        (
            "aots-chaining-context",
            seed(&[0, 20, 21, 22, 23, 0], &aots("gsub_chaining3_simple_f1")),
        ),
        (
            "aots-extension",
            seed(&[17, 18, 19, 20, 21], &aots("gsub7_font1")),
        ),
        // Four tables no conforming font would contain.
        (
            "extension-chain",
            seed(
                &[1],
                &layout_table(
                    &[lookup(
                        7,
                        0,
                        &[extension_chain(6, single_subst(&[(1, 2)]))],
                        None,
                    )],
                    None,
                ),
            ),
        ),
        (
            "self-naming-lookup",
            seed(
                &[1, 1],
                &layout_table(
                    &[lookup(
                        5,
                        0,
                        &[context_format3(&[vec![1]], &[(0, 0)])],
                        None,
                    )],
                    None,
                ),
            ),
        ),
        (
            "multiple-substitution",
            seed(
                &[1, 1, 1],
                &layout_table(
                    &[lookup(2, 0, &[multiple_subst(1, &[7, 8, 9, 10])], None)],
                    None,
                ),
            ),
        ),
        (
            "gdef-every-structure",
            seed(
                &[3, 5, 6, 12],
                &gdef(
                    Some(class_def(&[(5, 6, GlyphClass::Mark.value())])),
                    Some(attach_list(&[(3, vec![1, 4, 9])])),
                    Some(lig_caret_list(12)),
                    Some(class_def(&[(5, 5, 1), (6, 6, 2)])),
                    Some(mark_glyph_sets(&[vec![5]])),
                ),
            ),
        ),
    ];

    for (name, bytes) in &seeds {
        // A seed that reaches nothing looks exactly like one that reaches
        // everything, so each one is checked here for the thing it was chosen
        // for before it is written.
        let face = &bytes[2 + usize::from(bytes[1] >> 3) * 2..];
        let by_directory = tinker_pdf_font::Sfnt::parse(face).map(|sfnt| Layout::parse(&sfnt));
        let by_table = Layout::from_tables(Some(face), Some(face), Some(face));
        let reached = by_directory.is_some_and(|layout| {
            layout.gsub().is_some_and(|t| !t.lookups().is_empty())
                || layout.gpos().is_some_and(|t| !t.lookups().is_empty())
        }) || by_table.gsub().is_some_and(|t| !t.lookups().is_empty())
            || by_table.gdef().is_some();
        assert!(reached, "the seed {name} reaches no lookup and no GDEF");
        std::fs::write(base.join(name), bytes).expect("the seed is writable");
    }
    assert_eq!(seeds.len(), 8, "the seed count moved");
}

/// Drives every committed fuzz seed through the crate, under ceilings small
/// enough that each of them fires.
///
/// The fuzz target itself cannot run here: libFuzzer is unsupported on
/// `x86_64-pc-windows-msvc`, so `cargo fuzz run` is a Linux job. This is the
/// part of it that does not need a fuzzer — the seeds, the two entry paths and
/// the determinism assertion — so a seed that starts panicking is caught by
/// `cargo test` on every platform rather than only by the nightly run.
#[test]
fn every_fuzz_seed_shapes_without_panicking() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/shape");
    let mut count = 0usize;
    for entry in std::fs::read_dir(&base).expect("the seed corpus is committed") {
        let path = entry.expect("a readable entry").path();
        let bytes = std::fs::read(&path).expect("a readable seed");
        let glyph_count = usize::from(bytes[1] >> 3);
        let glyphs: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .take(glyph_count)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        let face = &bytes[2 + glyph_count * 2..];

        for limits in [
            Limits::DEFAULT,
            Limits {
                max_nesting_depth: 0,
                max_extension_depth: 0,
                max_context_length: 1,
                max_operations: 1,
                max_glyphs: 1,
            },
        ] {
            for layout in [
                tinker_pdf_font::Sfnt::parse(face).map(|sfnt| Layout::parse(&sfnt)),
                Some(Layout::from_tables(Some(face), Some(face), Some(face))),
            ]
            .into_iter()
            .flatten()
            {
                let run = |limits| {
                    let mut buffer = Buffer::from_glyphs(&glyphs);
                    let mut warnings = Vec::new();
                    if let Some(gsub) = layout.gsub() {
                        let lookups =
                            gsub.lookups_for(Tag::new(b"latn"), None, &[Tag::new(b"test")]);
                        warnings.extend(layout.substitute(&mut buffer, &lookups, limits));
                    }
                    if let Some(gpos) = layout.gpos() {
                        let lookups =
                            gpos.lookups_for(Tag::new(b"latn"), None, &[Tag::new(b"test")]);
                        warnings.extend(layout.position(
                            &mut buffer,
                            &lookups,
                            limits,
                            MarkWidths::ZeroByGdef,
                        ));
                    }
                    (buffer.glyphs().to_vec(), warnings)
                };
                let once = run(limits);
                let twice = run(limits);
                assert!(once == twice, "shaping is not deterministic");
                for glyph in &once.0 {
                    assert!(
                        usize::try_from(glyph.cluster).is_ok_and(|at| at < glyphs.len()),
                        "a cluster that was never in the input"
                    );
                }
            }
        }
        count += 1;
    }
    assert_eq!(count, 8, "the seed corpus changed size");
}

// --- positioning, end to end on a hand-built GPOS -----------------------

#[test]
fn positioning_leaves_the_advances_it_was_given_where_no_lookup_touches_them() {
    let gsub = layout_table(&[lookup(1, 0, &[single_subst(&[(1, 2)])], None)], None);
    let layout = Layout::from_tables(None, Some(&gsub), None);
    let mut buffer = Buffer::from_glyphs(&[1, 3]);
    buffer.glyph_mut(0).expect("in range").x_advance = 500;
    buffer.glyph_mut(1).expect("in range").x_advance = 600;
    let warnings = layout.position(&mut buffer, &[0], Limits::DEFAULT, MarkWidths::ZeroByGdef);
    assert!(warnings.is_empty(), "a face with no GPOS positions nothing");
    assert_eq!(buffer.glyph(0).expect("in range").x_advance, 500);
    assert_eq!(buffer.glyph(1).expect("in range").x_advance, 600);
}
