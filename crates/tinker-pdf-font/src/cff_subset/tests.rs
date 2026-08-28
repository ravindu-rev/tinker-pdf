//! Every font here is built byte by byte in this file.
//!
//! `testdata/` carries four self-authored PDFs and none embeds a CFF, and the
//! question a subsetting test has to answer — "did the *same* glyph come out"
//! — is only checkable when the fixture's every offset, subroutine number and
//! bias is known in advance. The `Font` builder below is the one in
//! `cff.rs`'s own tests grown a global subroutine INDEX and an FDArray whose
//! members carry their own local subroutines, because those are what a
//! subsetter has to renumber and what the reader's fixtures never needed.

use super::*;

// ---------------------------------------------------------------------------
// Building a CFF font program.
// ---------------------------------------------------------------------------

/// Wraps `items` as a CFF INDEX with four-byte offsets.
///
/// Wide offsets throughout, so a fixture that grows past 255 bytes does not
/// silently change format halfway through a test.
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(4);
    let mut offset = 1u32;
    out.extend_from_slice(&offset.to_be_bytes());
    for item in items {
        offset += item.len() as u32;
        out.extend_from_slice(&offset.to_be_bytes());
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// One DICT entry: operands in the fixed five-byte form, then the operator.
fn entry(op: u16, operands: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in operands {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }
    if op > 0xFF {
        out.push(12);
        out.push((op & 0xFF) as u8);
    } else {
        out.push(op as u8);
    }
    out
}

/// A Type 2 operand in the narrowest form, which is what a real font writes
/// and what makes a renumbering visible as a change of *length*.
fn t2(value: i32) -> Vec<u8> {
    charstring_int(value).expect("the fixture stays in range")
}

/// `callsubr` on a local subroutine, biased for an INDEX of `count`.
fn call_local(number: usize, count: usize) -> Vec<u8> {
    let mut out = t2(number as i32 - bias(count));
    out.push(10);
    out
}

/// `callgsubr` on a global subroutine, biased for an INDEX of `count`.
fn call_global(number: usize, count: usize) -> Vec<u8> {
    let mut out = t2(number as i32 - bias(count));
    out.push(29);
    out
}

/// A charstring drawing a box `size` units on a side, at the origin.
fn box_glyph(size: i32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(t2(0));
    out.extend(t2(0));
    out.push(21); // rmoveto
    for (dx, dy) in [(size, 0), (0, size), (-size, 0)] {
        out.extend(t2(dx));
        out.extend(t2(dy));
        out.push(5); // rlineto
    }
    out.push(14); // endchar
    out
}

/// A subroutine drawing one relative line and returning.
fn line_subr(dx: i32, dy: i32) -> Vec<u8> {
    let mut out = t2(dx);
    out.extend(t2(dy));
    out.push(5); // rlineto
    out.push(11); // return
    out
}

/// One member of an FDArray.
#[derive(Default, Clone)]
struct Fd {
    subrs: Vec<Vec<u8>>,
}

/// A CFF font program assembled from its parts.
#[derive(Default)]
struct Font {
    /// Strings past the standard 391, numbered from SID 391 upward.
    strings: Vec<Vec<u8>>,
    /// The SID — or, when `cid` is set, the CID — of each glyph from 1 up.
    charset: Vec<u16>,
    /// Each glyph's charstring, `.notdef` first.
    charstrings: Vec<Vec<u8>>,
    /// The font's own encoding table, when it carries one.
    encoding: Option<Vec<u8>>,
    /// The global subroutine INDEX, which every glyph shares.
    gsubrs: Vec<Vec<u8>>,
    /// The top-level Private DICT's local subroutines.
    subrs: Vec<Vec<u8>>,
    /// `ROS`, an FDArray and an FDSelect: a CID-keyed program.
    cid: bool,
    /// The FDArray, one entry per Font DICT.
    fds: Vec<Fd>,
    /// Which Font DICT each glyph belongs to; empty means all of them are in
    /// the first.
    fd_of: Vec<u8>,
    /// `CharstringType`, which the Top DICT states only when it is not the
    /// default of 2.
    charstring_type: Option<i32>,
}

/// Where each table landed, so the Top DICT can point at it.
#[derive(Default, Clone, Copy)]
struct At {
    charset: i32,
    encoding: i32,
    charstrings: i32,
    private: (i32, i32),
    fd_array: i32,
    fd_select: i32,
}

/// A Private DICT declaring both widths, and `Subrs` when there are any.
fn private_dict(has_subrs: bool) -> Vec<u8> {
    let mut out = entry(20, &[600]);
    out.extend(entry(21, &[600]));
    if has_subrs {
        // The INDEX follows the DICT immediately, six bytes on.
        out.extend(entry(19, &[out.len() as i32 + 6]));
    }
    out
}

impl Font {
    fn top_dict(&self, at: &At) -> Vec<u8> {
        let mut out = Vec::new();
        if self.cid {
            out.extend(entry(0x0C1E, &[391, 392, 0]));
        }
        if let Some(kind) = self.charstring_type {
            out.extend(entry(0x0C06, &[kind]));
        }
        out.extend(entry(15, &[at.charset]));
        if !self.cid {
            out.extend(entry(16, &[at.encoding]));
        }
        out.extend(entry(17, &[at.charstrings]));
        out.extend(entry(18, &[at.private.0, at.private.1]));
        if !self.fds.is_empty() {
            out.extend(entry(0x0C24, &[at.fd_array]));
            out.extend(entry(0x0C25, &[at.fd_select]));
        }
        out
    }

    /// FDSelect in format 3: runs of glyphs sharing a Font DICT.
    fn fd_select_bytes(&self) -> Vec<u8> {
        if self.fds.is_empty() {
            return Vec::new();
        }
        let glyphs = self.charstrings.len();
        let mut ranges: Vec<(u16, u8)> = Vec::new();
        for gid in 0..glyphs {
            let fd = self.fd_of.get(gid).copied().unwrap_or(0);
            if ranges.last().map(|(_, f)| *f) != Some(fd) {
                ranges.push((gid as u16, fd));
            }
        }
        let mut out = vec![3u8];
        out.extend_from_slice(&(ranges.len() as u16).to_be_bytes());
        for (first, fd) in &ranges {
            out.extend_from_slice(&first.to_be_bytes());
            out.push(*fd);
        }
        // The sentinel that ends the last range.
        out.extend_from_slice(&(glyphs as u16).to_be_bytes());
        out
    }

    fn build(&self) -> Vec<u8> {
        let header = [1u8, 0, 4, 4];
        let names = index(&[b"Fixture".to_vec()]);
        let strings = index(&self.strings);
        let gsubrs = index(&self.gsubrs);
        let charstrings = index(&self.charstrings);
        let encoding = self.encoding.clone().unwrap_or_default();
        let fd_select = self.fd_select_bytes();

        let mut charset = vec![0u8];
        for sid in &self.charset {
            charset.extend_from_slice(&sid.to_be_bytes());
        }

        let top_private = private_dict(!self.subrs.is_empty());
        let top_subrs = if self.subrs.is_empty() {
            Vec::new()
        } else {
            index(&self.subrs)
        };
        let fd_privates: Vec<Vec<u8>> = self
            .fds
            .iter()
            .map(|fd| private_dict(!fd.subrs.is_empty()))
            .collect();
        let fd_subrs: Vec<Vec<u8>> = self
            .fds
            .iter()
            .map(|fd| {
                if fd.subrs.is_empty() {
                    Vec::new()
                } else {
                    index(&fd.subrs)
                }
            })
            .collect();

        // The Top DICT's length cannot change when the offsets in it do, so
        // one pass with zeroes measures it and the second fills it in.
        let top_len = self.top_dict(&At::default()).len();
        let mut cursor =
            header.len() + names.len() + (2 + 1 + 8 + top_len) + strings.len() + gsubrs.len();

        let charset_at = cursor;
        cursor += charset.len();
        let encoding_at = cursor;
        cursor += encoding.len();
        let fd_select_at = cursor;
        cursor += fd_select.len();
        let charstrings_at = cursor;
        cursor += charstrings.len();
        let private_at = cursor;
        cursor += top_private.len() + top_subrs.len();
        let mut fd_private_at = Vec::new();
        for i in 0..self.fds.len() {
            fd_private_at.push(cursor);
            cursor += fd_privates[i].len() + fd_subrs[i].len();
        }
        let fd_array_at = cursor;

        let at = At {
            charset: charset_at as i32,
            encoding: if self.encoding.is_some() {
                encoding_at as i32
            } else {
                0
            },
            charstrings: charstrings_at as i32,
            private: (top_private.len() as i32, private_at as i32),
            fd_array: fd_array_at as i32,
            fd_select: fd_select_at as i32,
        };

        let mut out = header.to_vec();
        out.extend_from_slice(&names);
        out.extend_from_slice(&index(&[self.top_dict(&at)]));
        out.extend_from_slice(&strings);
        out.extend_from_slice(&gsubrs);
        assert_eq!(out.len(), charset_at, "the charset lands where it was put");
        out.extend_from_slice(&charset);
        assert_eq!(out.len(), encoding_at);
        out.extend_from_slice(&encoding);
        assert_eq!(out.len(), fd_select_at);
        out.extend_from_slice(&fd_select);
        assert_eq!(out.len(), charstrings_at);
        out.extend_from_slice(&charstrings);
        assert_eq!(out.len(), private_at);
        out.extend_from_slice(&top_private);
        out.extend_from_slice(&top_subrs);
        for i in 0..self.fds.len() {
            assert_eq!(out.len(), fd_private_at[i]);
            out.extend_from_slice(&fd_privates[i]);
            out.extend_from_slice(&fd_subrs[i]);
        }
        if !self.fds.is_empty() {
            assert_eq!(out.len(), fd_array_at);
            let dicts: Vec<Vec<u8>> = (0..self.fds.len())
                .map(|i| entry(18, &[fd_privates[i].len() as i32, fd_private_at[i] as i32]))
                .collect();
            out.extend_from_slice(&index(&dicts));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Reading the output back apart.
// ---------------------------------------------------------------------------

/// The Top DICT of a program.
fn top_of(program: &[u8]) -> Vec<(u16, Vec<f64>)> {
    let header_size = usize::from(program[2]);
    let (_, names_end) = Index::parse(program, header_size).expect("a name INDEX");
    let (tops, _) = Index::parse(program, names_end).expect("a Top DICT INDEX");
    parse_dict(tops.get(0).expect("one Top DICT"))
}

/// One glyph's charstring bytes.
fn charstring_of(program: &[u8], glyph: usize) -> Vec<u8> {
    let top = top_of(program);
    let at = dict_get(&top, 17).and_then(<[f64]>::first).copied().expect("CharStrings") as usize;
    let (index, _) = Index::parse(program, at).expect("a CharStrings INDEX");
    index.get(glyph).expect("the glyph").to_vec()
}

/// How many subroutines the top-level Private DICT's `Subrs` INDEX carries.
fn local_subr_count(program: &[u8]) -> usize {
    let top = top_of(program);
    let Some(operands) = dict_get(&top, 18) else {
        return 0;
    };
    let (size, offset) = (operands[0] as usize, operands[1] as usize);
    let private = parse_dict(&program[offset..offset + size]);
    let Some(&rel) = dict_get(&private, 19).and_then(<[f64]>::first) else {
        return 0;
    };
    Index::parse(program, offset + rel as usize).map_or(0, |(index, _)| index.len())
}

/// How many subroutines the global INDEX carries.
fn global_subr_count(program: &[u8]) -> usize {
    let header_size = usize::from(program[2]);
    let (_, names_end) = Index::parse(program, header_size).expect("a name INDEX");
    let (_, top_end) = Index::parse(program, names_end).expect("a Top DICT INDEX");
    let (_, strings_end) = Index::parse(program, top_end).expect("a String INDEX");
    Index::parse(program, strings_end).map_or(0, |(index, _)| index.len())
}

/// Asserts that every glyph in `kept` draws in `subset` exactly what it draws
/// in `original`, segment for segment.
///
/// This is the closed-form property the whole exercise rests on. An assertion
/// on the *count* of segments would pass a subset that drew the right number
/// of wrong lines, which is the failure this is looking for.
#[track_caller]
fn outlines_agree(original: &[u8], subset: &[u8], kept: &[u16]) {
    let before = Cff::parse(original).expect("the original parses");
    let after = Cff::parse(subset).expect("the subset parses");
    assert_eq!(
        before.glyph_count(),
        after.glyph_count(),
        "glyph identifiers do not move, so the count cannot either"
    );
    for &glyph in kept {
        let a = before.outline(glyph).expect("the original outlines");
        let b = after.outline(glyph).expect("the subset outlines");
        assert_eq!(
            a.segments, b.segments,
            "glyph {glyph} draws something else in the subset"
        );
    }
}

// ---------------------------------------------------------------------------
// The fixtures the tests share.
// ---------------------------------------------------------------------------

/// Four glyphs, no subroutines: the simplest thing that can be subsetted.
fn plain_font() -> Vec<u8> {
    Font {
        // SIDs 34, 35 and 36 are `A`, `B` and `C` among the standard strings.
        charset: vec![34, 35, 36],
        charstrings: vec![vec![14], box_glyph(600), box_glyph(300), box_glyph(450)],
        ..Font::default()
    }
    .build()
}

/// Eight local subroutines, one per drawn glyph, so a subset keeps exactly the
/// ones the glyphs it kept reach.
fn subr_font() -> Vec<u8> {
    let count = 8usize;
    let subrs: Vec<Vec<u8>> = (0..count).map(|i| line_subr(100 + i as i32 * 10, 0)).collect();
    let mut charstrings = vec![vec![14u8]];
    for i in 0..count {
        let mut code = t2(0);
        code.extend(t2(0));
        code.push(21); // rmoveto
        code.extend(call_local(i, count));
        code.push(14); // endchar
        charstrings.push(code);
    }
    Font {
        charset: (0..count as u16).map(|i| 34 + i).collect(),
        charstrings,
        subrs,
        ..Font::default()
    }
    .build()
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

#[test]
fn the_fixture_is_a_font_this_crate_can_read() {
    let program = plain_font();
    let cff = Cff::parse(&program).expect("it parses");
    assert_eq!(cff.glyph_count(), 4);
    assert_eq!(cff.gid_for_name("B"), Some(2));
    assert!(!cff.outline(2).expect("an outline").segments.is_empty());
}

/// The headline: the glyphs nobody asked for stop drawing, the ones that were
/// asked for draw exactly what they drew, and the file is smaller for it.
#[test]
fn a_subset_keeps_what_it_was_asked_for_and_drops_the_rest() {
    let program = plain_font();
    let wanted: BTreeSet<u16> = [2u16].into_iter().collect();
    let subset = subset_cff(&program, &wanted).expect("it subsets");

    outlines_agree(&program, &subset, &[0, 2]);
    assert!(
        subset.len() < program.len(),
        "the subset is smaller: {} vs {}",
        subset.len(),
        program.len()
    );

    let after = Cff::parse(&subset).expect("the subset parses");
    for dropped in [1u16, 3] {
        assert!(
            after
                .outline(dropped)
                .expect("a dropped glyph still answers")
                .segments
                .is_empty(),
            "glyph {dropped} was not asked for and must draw nothing"
        );
    }
}

/// Identifiers staying put is what `/Widths`, `/W`, `/CIDToGIDMap` and
/// `/ToUnicode` all rest on. The charset is what makes it checkable from
/// inside this crate: `B` must still be glyph 2 and not whichever glyph
/// survived second.
#[test]
fn glyph_identifiers_and_the_charset_do_not_move() {
    let program = plain_font();
    let wanted: BTreeSet<u16> = [3u16].into_iter().collect();
    let subset = subset_cff(&program, &wanted).expect("it subsets");

    let before = Cff::parse(&program).expect("it parses");
    let after = Cff::parse(&subset).expect("the subset parses");
    for name in ["A", "B", "C"] {
        assert_eq!(
            after.gid_for_name(name),
            before.gid_for_name(name),
            "{name} moved"
        );
    }
    assert_eq!(after.gid_for_name("C"), Some(3));
    assert_eq!(after.glyph_name(1), Some("A"));
}

/// `.notdef` is what a reader draws for anything the font does not cover, so
/// it survives whether or not anybody asked.
#[test]
fn the_notdef_glyph_survives_even_unasked() {
    let program = subr_font();
    let subset = subset_cff(&program, &[3u16].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 3]);
}

/// The local subroutine INDEX shrinks to what the kept glyphs reach, and the
/// `callsubr` operand in the surviving charstring is rewritten for the new
/// position *and* the new bias.
#[test]
fn a_local_subroutine_call_is_renumbered() {
    let program = subr_font();
    assert_eq!(local_subr_count(&program), 8);

    let wanted: BTreeSet<u16> = [5u16].into_iter().collect();
    let subset = subset_cff(&program, &wanted).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 5]);

    assert_eq!(
        local_subr_count(&subset),
        1,
        "glyph 5 reaches one subroutine and the other seven are gone"
    );
    // Subroutine 4 in a font of eight is called with `4 - 107`; the one
    // subroutine left is number 0, called with `0 - 107`.
    assert_eq!(
        charstring_of(&subset, 5),
        {
            let mut code = t2(0);
            code.extend(t2(0));
            code.push(21);
            code.extend(call_local(0, 1));
            code.push(14);
            code
        },
        "the operand names the subroutine's new position"
    );
}

/// A global subroutine that calls a local one, from a charstring that calls
/// the global one: three levels, two INDEXes, and both renumbered.
#[test]
fn a_global_subroutine_that_calls_a_local_one_is_renumbered_too() {
    // Six locals and four globals, of which the kept glyph reaches one each.
    let locals: Vec<Vec<u8>> = (0..6).map(|i| line_subr(100 + i * 10, 0)).collect();
    let globals: Vec<Vec<u8>> = (0..4)
        .map(|i| {
            let mut out = call_local(i as usize + 1, 6);
            out.push(11); // return
            out
        })
        .collect();

    let mut drawn = t2(0);
    drawn.extend(t2(0));
    drawn.push(21); // rmoveto
    drawn.extend(call_global(2, 4));
    drawn.push(14);

    let program = Font {
        charset: vec![34, 35],
        charstrings: vec![vec![14], box_glyph(600), drawn],
        gsubrs: globals,
        subrs: locals,
        ..Font::default()
    }
    .build();

    let before = Cff::parse(&program).expect("it parses");
    assert_eq!(
        before.outline(2).expect("glyph 2").segments.len(),
        3,
        "a move, the line global 2 draws through local 3, and the close"
    );

    let subset = subset_cff(&program, &[2u16].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 2]);
    assert_eq!(global_subr_count(&subset), 1);
    assert_eq!(local_subr_count(&subset), 1);
}

/// The first bias step. A font with 1 300 local subroutines biases them by
/// 1 131; a subset that keeps one biases by 107, so the operand for the *same*
/// subroutine changes even though nothing else about it did.
///
/// A test built on a small font never crosses a threshold, and a subsetter
/// that adjusted the operand instead of recomputing it would pass every one of
/// those and fail here.
#[test]
fn the_bias_falls_from_1131_to_107_when_the_index_shrinks() {
    const COUNT: usize = 1300;
    assert_eq!(bias(COUNT), 1131);
    let mut subrs: Vec<Vec<u8>> = vec![vec![11u8]; COUNT];
    subrs[1250] = line_subr(400, 0);

    let mut drawn = t2(0);
    drawn.extend(t2(0));
    drawn.push(21);
    drawn.extend(call_local(1250, COUNT));
    drawn.push(14);

    let program = Font {
        charset: vec![34],
        charstrings: vec![vec![14], drawn],
        subrs,
        ..Font::default()
    }
    .build();

    // The original operand really is on the far side of the step: 1250 less
    // a bias of 1131.
    assert_eq!(call_local(1250, COUNT), [t2(119), vec![10]].concat());
    let subset = subset_cff(&program, &[1u16].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 1]);

    assert_eq!(local_subr_count(&subset), 1);
    assert_eq!(bias(1), 107);
    let code = charstring_of(&subset, 1);
    assert!(
        code.ends_with(&[t2(-107)[0], 10, 14]),
        "the operand is the new index less the new bias, not the old one: {code:?}"
    );
}

/// The second bias step, at 33 900, and a subset that lands between the two.
///
/// 34 000 subroutines bias by 32 768; the 1 300 this keeps bias by 1 131.
///
/// Three things about the kept set are deliberate, and each of them closes a
/// way this test could have passed while the renumbering was wrong.
///
/// **It does not start at zero.** A subset whose survivors are a *prefix* of
/// the original numbering renumbers them onto themselves, so a subsetter that
/// ignored the new position entirely would pass. The injection matrix found
/// exactly that: with the range starting at zero, reintroducing "use the
/// original subroutine index" changed nothing here.
///
/// **It is not contiguous.** Every third subroutine is kept, so the map is
/// `20000 + 3k -> k` and no survivor keeps its number. A contiguous range
/// renumbers by a constant, which a subsetter that subtracted a fixed offset
/// would also get right.
///
/// **Every operand form appears on the new side and none on the old.** Under
/// the old bias each call is `3k - 12768`, which only the three-byte form
/// holds; under the new one it is `k - 1131`, which runs from -1131 through
/// -108 (the two-byte negative form), -107 through 107 (the one-byte form)
/// and 108 upward (the two-byte positive form). So the rewriter cannot be
/// patching bytes in place — it has to re-encode — and the assertion below is
/// the whole charstring written out from those rules rather than read back
/// from what the subsetter produced.
#[test]
fn the_bias_falls_from_32768_to_1131_when_the_index_shrinks() {
    const COUNT: usize = 34_000;
    const KEPT: usize = 1300;
    const FIRST: usize = 20_000;
    const STRIDE: usize = 3;
    assert_eq!(bias(COUNT), 32768);
    assert_eq!(bias(KEPT), 1131);

    let called: Vec<usize> = (0..KEPT).map(|k| FIRST + k * STRIDE).collect();
    let mut subrs: Vec<Vec<u8>> = vec![vec![11u8]; COUNT];
    for (k, &index) in called.iter().enumerate() {
        // A different line per subroutine, so calling the wrong one shows.
        subrs[index] = line_subr(1 + (k as i32 % 97), 1 + (k as i32 % 31));
    }

    let mut drawn = t2(0);
    drawn.extend(t2(0));
    drawn.push(21); // rmoveto
    for &index in &called {
        drawn.extend(call_local(index, COUNT));
    }
    drawn.push(14);

    let program = Font {
        charset: vec![34],
        charstrings: vec![vec![14], drawn],
        subrs,
        ..Font::default()
    }
    .build();

    let before = Cff::parse(&program).expect("it parses");
    assert_eq!(
        before.outline(1).expect("glyph 1").segments.len(),
        2 + KEPT,
        "a move, one line per subroutine, and the close"
    );

    let subset = subset_cff(&program, &[1u16].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 1]);
    assert_eq!(local_subr_count(&subset), KEPT);
    assert!(
        subset.len() < program.len() / 2,
        "the 32 700 unreached subroutines are gone"
    );

    // The closed form: what the charstring must be, written out from the
    // renumbering rule rather than read back from the subsetter.
    let mut expected = t2(0);
    expected.extend(t2(0));
    expected.push(21);
    for k in 0..KEPT {
        expected.extend(t2(k as i32 - 1131));
        expected.push(10);
    }
    expected.push(14);
    assert_eq!(
        charstring_of(&subset, 1),
        expected,
        "the operand of every call is its new index less the new bias"
    );

    // And the four operand forms really are all present, so the equality above
    // is not one encoding repeated 1 300 times.
    assert_eq!(t2(0 - 1131), vec![254, 255], "the two-byte negative form");
    assert_eq!(t2(1023 - 1131), vec![251, 0], "its far end");
    assert_eq!(t2(1024 - 1131), vec![32], "the one-byte form");
    assert_eq!(t2(1239 - 1131), vec![247, 0], "the two-byte positive form");
    // Every call in the *original* took the three-byte form, so nothing here
    // could have been left as it was found.
    for &index in &called {
        assert_eq!(call_local(index, COUNT).len(), 4, "three bytes and the call");
    }
}

/// A CID-keyed font: `FDSelect` decides which Private DICT a glyph's
/// `callsubr` numbers are relative to, so two glyphs in two Font DICTs
/// calling "subroutine 0" call two different subroutines.
#[test]
fn a_cid_keyed_font_renumbers_each_font_dicts_subroutines() {
    let font = Font {
        strings: vec![b"Adobe".to_vec(), b"Identity".to_vec()],
        charset: vec![10, 11, 12],
        charstrings: vec![
            vec![14],
            {
                let mut c = t2(0);
                c.extend(t2(0));
                c.push(21);
                c.extend(call_local(1, 3));
                c.push(14);
                c
            },
            {
                let mut c = t2(0);
                c.extend(t2(0));
                c.push(21);
                c.extend(call_local(2, 4));
                c.push(14);
                c
            },
            box_glyph(450),
        ],
        cid: true,
        fds: vec![
            Fd {
                subrs: vec![line_subr(10, 0), line_subr(20, 0), line_subr(30, 0)],
            },
            Fd {
                subrs: vec![
                    line_subr(40, 0),
                    line_subr(50, 0),
                    line_subr(60, 0),
                    line_subr(70, 0),
                ],
            },
        ],
        // Glyphs 0 and 1 in Font DICT 0; glyphs 2 and 3 in Font DICT 1.
        fd_of: vec![0, 0, 1, 1],
        ..Font::default()
    };
    let program = font.build();

    let before = Cff::parse(&program).expect("it parses");
    assert!(before.is_cid());
    assert_eq!(before.gid_for_cid(11), Some(2));
    // The two glyphs draw different lines, which is what says FDSelect is
    // being honoured rather than one Private DICT being used for both.
    let one = before.outline(1).expect("glyph 1");
    let two = before.outline(2).expect("glyph 2");
    assert_ne!(one.segments, two.segments);

    let subset = subset_cff(&program, &[1u16, 2].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 1, 2]);

    let after = Cff::parse(&subset).expect("the subset parses");
    assert!(after.is_cid(), "the ROS is copied through");
    assert_eq!(
        after.gid_for_cid(11),
        Some(2),
        "and so is the charset, so a CID still reaches its glyph"
    );
    assert!(
        after.outline(3).expect("glyph 3").segments.is_empty(),
        "the glyph in Font DICT 1 that nobody asked for draws nothing"
    );
}

/// One global subroutine reached from two Font DICTs. The `callsubr` inside it
/// means two different subroutines depending on which Font DICT is running, so
/// it cannot be one entry in the new global INDEX.
#[test]
fn a_global_subroutine_reached_from_two_font_dicts_is_written_twice() {
    // Global 0 calls local 1, whatever "local 1" is where it is called from.
    let global = {
        let mut out = call_local(1, 2);
        out.push(11);
        out
    };
    let glyph = |gsubr_count: usize| {
        let mut c = t2(0);
        c.extend(t2(0));
        c.push(21);
        c.extend(call_global(0, gsubr_count));
        c.push(14);
        c
    };

    let program = Font {
        strings: vec![b"Adobe".to_vec(), b"Identity".to_vec()],
        charset: vec![10, 11],
        charstrings: vec![vec![14], glyph(1), glyph(1)],
        gsubrs: vec![global],
        cid: true,
        fds: vec![
            Fd {
                subrs: vec![line_subr(11, 0), line_subr(22, 0)],
            },
            Fd {
                subrs: vec![line_subr(33, 0), line_subr(44, 0)],
            },
        ],
        fd_of: vec![0, 0, 1],
        ..Font::default()
    }
    .build();

    let before = Cff::parse(&program).expect("it parses");
    assert_ne!(
        before.outline(1).expect("glyph 1").segments,
        before.outline(2).expect("glyph 2").segments,
        "the same global subroutine draws two different lines"
    );

    let subset = subset_cff(&program, &[1u16, 2].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 1, 2]);
    assert_eq!(
        global_subr_count(&subset),
        2,
        "one copy per Font DICT that reaches it"
    );
}

/// A `hintmask` is as long as the stems declared before it, and a subroutine
/// can declare them. Counting only the caller's stems reads one byte too few
/// and every token after it lands in the wrong place.
///
/// The mask's second byte is `0x0A` — `callsubr` — on purpose: a rewriter that
/// stopped one byte short would read it as a call with an empty stack, which
/// ends the charstring, and the line the glyph draws afterwards would vanish.
#[test]
fn a_hintmask_counts_the_stems_a_subroutine_declared() {
    // Sixteen operands then `hstem`: eight stem hints, declared inside a
    // subroutine and counted by the caller.
    let mut stems = Vec::new();
    for i in 0..8 {
        stems.extend(t2(20 + i * 4));
        stems.extend(t2(2));
    }
    stems.push(1); // hstem
    stems.push(11); // return

    let subrs = vec![stems, line_subr(250, 40)];

    let mut drawn = call_local(0, 2);
    // Two more operands, so the ninth stem takes the mask into a second byte.
    drawn.extend(t2(90));
    drawn.extend(t2(3));
    drawn.push(19); // hintmask
    drawn.extend_from_slice(&[0xFF, 0x0A]);
    drawn.extend(t2(0));
    drawn.extend(t2(0));
    drawn.push(21); // rmoveto
    drawn.extend(call_local(1, 2));
    drawn.push(14);

    let program = Font {
        charset: vec![34],
        charstrings: vec![vec![14], drawn],
        subrs,
        ..Font::default()
    }
    .build();

    let before = Cff::parse(&program).expect("it parses");
    assert_eq!(
        before.outline(1).expect("glyph 1").segments.len(),
        3,
        "a move, the line the second subroutine draws, and the close"
    );

    let subset = subset_cff(&program, &[1u16].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[1]);
    assert_eq!(local_subr_count(&subset), 2, "both subroutines are reached");
}

/// The `seac` form of `endchar` builds an accented letter out of two glyphs
/// named by StandardEncoding codes. Keeping the letter without them draws
/// blank space in every reader that honours it.
///
/// This engine's own reader does not draw `seac`, so an outline comparison
/// cannot see the difference — which is exactly why the assertion here is that
/// the components' charstrings survived, and why it is worth writing down that
/// the outline test would pass either way.
#[test]
fn a_seac_component_is_kept_with_the_glyph_that_names_it() {
    // Glyph 3 is `Aacute`: `adx ady bchar achar endchar`, where bchar 65 is
    // `A` and achar 194 is `acute` in StandardEncoding.
    let mut accented = t2(0);
    accented.extend(t2(0));
    accented.extend(t2(65));
    accented.extend(t2(194));
    accented.push(14);

    let program = Font {
        // `A` is SID 34 and StandardEncoding code 65; `acute` is SID 125 and
        // code 194; `Aacute` is a custom string, numbered from 391 up.
        strings: vec![b"Aacute".to_vec()],
        charset: vec![34, 125, 391],
        charstrings: vec![vec![14], box_glyph(600), box_glyph(120), accented],
        ..Font::default()
    }
    .build();

    let before = Cff::parse(&program).expect("it parses");
    assert_eq!(before.gid_for_name("A"), Some(1));
    assert_eq!(before.gid_for_name("acute"), Some(2));

    let subset = subset_cff(&program, &[3u16].into_iter().collect()).expect("it subsets");
    outlines_agree(&program, &subset, &[0, 1, 2, 3]);

    let after = Cff::parse(&subset).expect("the subset parses");
    for component in [1u16, 2] {
        assert!(
            !after
                .outline(component)
                .expect("the component answers")
                .segments
                .is_empty(),
            "component {component} was dropped, so the accented letter is blank \
             in any reader that draws seac"
        );
    }
}

/// The font's own encoding decides which glyph a code selects when the
/// document names none, so the subset carries the table through unchanged.
#[test]
fn the_fonts_own_encoding_is_carried_through() {
    // Format 0: three codes, for glyphs 1, 2 and 3 in order, and deliberately
    // not the standard ones — code 65 selects glyph 2.
    let encoding = vec![0, 3, 0x43, 0x41, 0x42];
    let program = Font {
        charset: vec![34, 35, 36],
        charstrings: vec![vec![14], box_glyph(600), box_glyph(300), box_glyph(450)],
        encoding: Some(encoding),
        ..Font::default()
    }
    .build();

    let before = Cff::parse(&program).expect("it parses");
    assert_eq!(before.gid_for_code(0x41), Some(2));

    let subset = subset_cff(&program, &[2u16].into_iter().collect()).expect("it subsets");
    let after = Cff::parse(&subset).expect("the subset parses");
    assert_eq!(
        after.gid_for_code(0x41),
        Some(2),
        "the built-in encoding still names the same glyph"
    );
    assert_eq!(after.gid_for_code(0x43), Some(1));
}

/// A `callsubr` whose number was computed rather than written down has no byte
/// to rewrite. Refusing embeds the whole face, which is larger and correct.
#[test]
fn a_computed_subroutine_number_is_refused() {
    // `0 get callsubr`: the number comes off the transient array, so the token
    // before the call is an operator and not the number it pops.
    let mut drawn = t2(0);
    drawn.extend([12, 21]); // get
    drawn.push(10); // callsubr
    drawn.push(14);

    let program = Font {
        charset: vec![34],
        charstrings: vec![vec![14], drawn],
        subrs: (0..200).map(|i| line_subr(10 + i, 0)).collect(),
        ..Font::default()
    }
    .build();
    assert!(Cff::parse(&program).is_some(), "the reader accepts it");
    assert!(
        subset_cff(&program, &[1u16].into_iter().collect()).is_none(),
        "and the writer will not guess which subroutine it meant"
    );
}

/// A call to a subroutine the font does not carry. The reader skips it; a
/// subset cannot, because the new bias would make the same operand name a
/// subroutine that *does* exist.
#[test]
fn a_call_to_a_subroutine_that_is_not_there_is_refused() {
    let mut drawn = t2(0);
    drawn.extend(t2(0));
    drawn.push(21);
    drawn.extend(call_local(50, 2)); // there are two
    drawn.push(14);

    let program = Font {
        charset: vec![34],
        charstrings: vec![vec![14], drawn],
        subrs: vec![line_subr(10, 0), line_subr(20, 0)],
        ..Font::default()
    }
    .build();
    assert!(Cff::parse(&program).is_some(), "the reader accepts it");
    assert!(subset_cff(&program, &[1u16].into_iter().collect()).is_none());
}

/// A subroutine that calls itself never terminates on paper; the reader stops
/// it with a depth cap and draws whatever it had by then. Rewriting that means
/// deciding what the font meant, which is not this code's decision to make.
#[test]
fn a_self_recursive_subroutine_is_refused() {
    let recursive = {
        let mut out = line_subr(10, 0);
        out.pop(); // drop the `return`
        out.extend(call_local(0, 1));
        out.push(11);
        out
    };
    let mut drawn = t2(0);
    drawn.extend(t2(0));
    drawn.push(21);
    drawn.extend(call_local(0, 1));
    drawn.push(14);

    let program = Font {
        charset: vec![34],
        charstrings: vec![vec![14], drawn],
        subrs: vec![recursive],
        ..Font::default()
    }
    .build();
    assert!(
        Cff::parse(&program)
            .and_then(|cff| cff.outline(1))
            .is_some(),
        "the reader draws something for it"
    );
    assert!(subset_cff(&program, &[1u16].into_iter().collect()).is_none());
}

/// `CharstringType 1` means the CharStrings INDEX holds a different language,
/// and rewriting Type 2 tokens over Type 1 bytes produces a font that parses
/// and draws nonsense.
#[test]
fn a_type_1_charstring_font_is_refused() {
    let make = |kind: Option<i32>| {
        Font {
            charset: vec![34, 35, 36],
            charstrings: vec![vec![14], box_glyph(600), box_glyph(300), box_glyph(450)],
            charstring_type: kind,
            ..Font::default()
        }
        .build()
    };
    let wanted: BTreeSet<u16> = [1u16].into_iter().collect();
    assert!(subset_cff(&make(Some(1)), &wanted).is_none());
    // The declared default and the undeclared one both subset, so the refusal
    // is about the value and not about the operator being present.
    assert!(subset_cff(&make(Some(2)), &wanted).is_some());
    assert!(subset_cff(&make(None), &wanted).is_some());
}

/// Ruling 1: a font program is attacker-controlled, and the writer must not
/// panic on anything the reader accepted.
#[test]
fn garbage_is_refused_rather_than_panicked_on() {
    assert!(subset_cff(&[], &BTreeSet::new()).is_none());
    assert!(subset_cff(b"not a font", &BTreeSet::new()).is_none());

    let program = subr_font();
    // Every prefix, every single-byte corruption at a stride, and a wanted set
    // that names glyphs the font does not have.
    let wanted: BTreeSet<u16> = [0u16, 1, 5, 9999].into_iter().collect();
    for cut in 0..program.len() {
        let _ = subset_cff(&program[..cut], &wanted);
    }
    for at in (0..program.len()).step_by(3) {
        let mut damaged = program.clone();
        damaged[at] ^= 0xFF;
        let _ = subset_cff(&damaged, &wanted);
    }
}

/// The primitives, which everything above depends on and none of it pins
/// directly: a number that does not survive the round trip is a subroutine
/// call to somewhere else.
#[test]
fn a_charstring_number_reads_back_as_itself() {
    for value in [-32768i32, -1132, -1131, -108, -107, 0, 107, 108, 1131, 32767] {
        let bytes = charstring_int(value).expect("in range");
        let mut stems = 0usize;
        let mut stack = Vec::new();
        let token = next_token(&bytes, 0, &mut stems, &mut stack).expect("a token");
        assert!(matches!(token.step, Step::Operand), "{value} is an operand");
        assert_eq!(token.end, bytes.len(), "{value} takes all of its bytes");
        assert_eq!(stack, vec![f64::from(value)], "{value} reads back");
    }
    assert!(charstring_int(32768).is_none());
    assert!(charstring_int(-32769).is_none());
}

/// A DICT operand that does not survive the round trip is a table offset
/// pointing at the wrong byte, or a font matrix at the wrong scale.
#[test]
fn a_dict_operand_reads_back_as_itself() {
    for value in [0.0f64, 1.0, -1.0, 2_147_483_647.0, -2_147_483_648.0] {
        let dict = [dict_operand(value), vec![15u8]].concat();
        let parsed = parse_dict(&dict);
        assert_eq!(dict_get(&parsed, 15), Some([value].as_slice()));
    }
    for value in [0.001f64, -0.001, 0.5, 1e-7, 1.5e10] {
        let dict = [dict_operand(value), vec![15u8]].concat();
        let parsed = parse_dict(&dict);
        let back = dict_get(&parsed, 15).and_then(<[f64]>::first).copied();
        assert_eq!(back, Some(value), "{value} did not survive as a real");
    }
}

/// An INDEX this writes is an INDEX the reader reads, at every offset width.
#[test]
fn an_index_reads_back_as_what_was_written() {
    for size in [0usize, 1, 3, 300, 70_000] {
        let items: Vec<Vec<u8>> = if size == 0 {
            Vec::new()
        } else {
            vec![vec![0x8Bu8; size], vec![0x8C; 1], vec![0x8D; size]]
        };
        let bytes = write_index(&items).expect("it writes");
        let (index, end) = Index::parse(&bytes, 0).expect("it parses");
        assert_eq!(end, bytes.len(), "the next structure begins after it");
        assert_eq!(index.len(), items.len());
        for (i, item) in items.iter().enumerate() {
            assert_eq!(index.get(i), Some(item.as_slice()), "item {i} of {size}");
        }
    }
    // 65 536 items is one more than the two-byte count can state.
    assert!(write_index(&vec![vec![0u8]; 65_536]).is_none());
}
