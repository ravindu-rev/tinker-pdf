//! Type 1 subsetting: the same program with the charstrings nobody draws
//! removed and the subroutines nobody reaches emptied
//! (`docs/design/subsetting.md`, *The Type 1 subsetter*).
//!
//! A Type 1 program addresses its glyphs by name, so nothing outside it holds
//! a glyph's position and a charstring can simply be deleted. Its subroutines
//! are addressed by number from inside the charstrings, so one nobody reaches
//! is not deleted but rewritten in place as a bare `return`, and no
//! `callsubr` operand is ever rewritten — there is no renumbering to get
//! wrong.
//!
//! The program is read by [`crate::type1`]'s own scanner, so the entries
//! edited here are exactly the ones the reader decrypts, and every glyph kept
//! is outlined in both programs before an answer is given. Anything this
//! cannot rewrite without guessing is a [`Type1Refusal`], and the caller embeds
//! the whole program, which is larger and correct (ruling 2).

use std::collections::BTreeSet;
use std::ops::Range;

use crate::type1::{
    encrypt, find, scan_charstrings, scan_subrs, Bound, Entry, Key, Layout, Trace, Type1,
    CHARSTRING_KEY, EEXEC_KEY,
};

/// The operator steps one subset may spend tracing and verifying its glyphs:
/// a program is attacker-controlled (ruling 1), and each glyph's run is
/// bounded, but a program of many glyphs is not.
const WORK_BUDGET: u64 = 40_000_000;

/// The subroutine indices the reader stores (`type1.rs`'s bound).
const MAX_SUBRS: usize = 65_536;

/// A Type 1 program cut down to some of its glyphs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Type1Subset {
    /// The program.
    pub program: Vec<u8>,
    /// ISO 32000-2 Table 125's three lengths: the clear text, the encrypted
    /// portion and the fixed-content portion, which together are the program.
    pub lengths: [usize; 3],
    /// The names of the glyphs it keeps, in program order, `.notdef` among
    /// them.
    pub names: Vec<Vec<u8>>,
}

/// Why a Type 1 program was not cut down (`docs/design/subsetting.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Type1Refusal {
    /// The reader declines the program, or it carries no charstring.
    Unreadable,
    /// A PFB container: PDF embeds the bare program (ISO 32000-2 Table 124).
    Pfb,
    /// The encrypted portion is hexadecimal, which Table 125's note 2 says PDF
    /// does not support.
    HexEncrypted,
    /// A `/lenIV` outside 0 to 16, which the reader does not decrypt as
    /// written.
    LenIv,
    /// `FontDirectory` or `hires` after `/Private` (Type 1 Font Format,
    /// 10.3): a synthetic or hybrid program, two sets of entries of which the
    /// reader reads one.
    SyntheticOrHybrid,
    /// No `closefile` after the charstrings, so where the encrypted portion
    /// ends is not stated.
    NoCloseFile,
    /// Two scanned entries overlap, one lies past `closefile`, or a
    /// charstring's `ND` cannot be told from the next entry.
    Entries,
    /// Two charstrings under one name: a PostScript interpreter keeps the
    /// last and this reader the first.
    DuplicateName,
    /// One `/Subrs` index defined twice.
    DuplicateSubroutine,
    /// More charstrings than the reader reads.
    TooManyGlyphs,
    /// A kept glyph calls a subroutine the program does not define, or a hint
    /// replacement names one.
    MissingSubroutine,
    /// A kept glyph's run calls past ten subroutines deep, where the reader
    /// stops.
    SubroutineDepth,
    /// A kept glyph's run spends the reader's operator budget, or the subset
    /// spends its own.
    Budget,
    /// A kept glyph's run pushes more operands than the stack holds.
    StackOverflow,
    /// The subset does not outline or advance a kept glyph as the program
    /// does.
    Mismatch,
}

impl core::fmt::Display for Type1Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Type1Refusal::Unreadable => "the program does not read as Type 1",
            Type1Refusal::Pfb => "a PFB container, which PDF does not embed",
            Type1Refusal::HexEncrypted => "a hexadecimal encrypted portion",
            Type1Refusal::LenIv => "a /lenIV outside 0 to 16",
            Type1Refusal::SyntheticOrHybrid => "a synthetic or hybrid program",
            Type1Refusal::NoCloseFile => "no closefile ends the encrypted portion",
            Type1Refusal::Entries => "the program's entries overlap",
            Type1Refusal::DuplicateName => "two charstrings under one name",
            Type1Refusal::DuplicateSubroutine => "one subroutine index defined twice",
            Type1Refusal::TooManyGlyphs => "more charstrings than the reader reads",
            Type1Refusal::MissingSubroutine => "a kept glyph calls a missing subroutine",
            Type1Refusal::SubroutineDepth => "a kept glyph calls subroutines past ten deep",
            Type1Refusal::Budget => "a kept glyph spends the operator budget",
            Type1Refusal::StackOverflow => "a kept glyph overflows the operand stack",
            Type1Refusal::Mismatch => "the subset does not draw a kept glyph as the program does",
        })
    }
}

/// Builds the program `program` would be with only `glyphs` — indices in the
/// order [`Type1::parse`] reads the charstrings — their `seac` components and
/// `.notdef` kept, and every subroutine they reach.
///
/// # Errors
///
/// A [`Type1Refusal`] naming what the program says that this cannot rewrite
/// without guessing; the caller embeds the original.
pub fn subset_type1(program: &[u8], glyphs: &BTreeSet<u16>) -> Result<Type1Subset, Type1Refusal> {
    if program.first() == Some(&0x80) {
        return Err(Type1Refusal::Pfb);
    }
    let layout = Layout::read(program).ok_or(Type1Refusal::Unreadable)?;
    if layout.hex {
        return Err(Type1Refusal::HexEncrypted);
    }
    if !(0..=16).contains(&layout.len_iv.unwrap_or(4)) {
        return Err(Type1Refusal::LenIv);
    }
    let font = Type1::parse(program).ok_or(Type1Refusal::Unreadable)?;
    if font.glyph_count() == 0 {
        return Err(Type1Refusal::Unreadable);
    }
    let private = &layout.private;

    let subrs = scan_subrs(private);
    let charstrings = scan_charstrings(private);
    if charstrings.capped {
        return Err(Type1Refusal::TooManyGlyphs);
    }
    // The reader's glyphs are these entries, one for one; anything else is a
    // scan this module does not understand.
    if charstrings.entries.len() != font.glyph_count() {
        return Err(Type1Refusal::Entries);
    }

    // 2.4: the encrypted portion ends with `mark currentfile closefile`.
    let after_entries = charstrings
        .entries
        .iter()
        .chain(&subrs.entries)
        .map(|entry| entry.data.end)
        .max()
        .unwrap_or(0);
    let close = find(
        private.get(after_entries..).unwrap_or_default(),
        b"closefile",
    )
    .map(|at| after_entries + at)
    .ok_or(Type1Refusal::NoCloseFile)?;
    let close_end = close + b"closefile".len();

    let text = text_between(private, &subrs.entries, &charstrings.entries, close)?;
    if synthetic_or_hybrid(private, &text) {
        return Err(Type1Refusal::SyntheticOrHybrid);
    }
    distinct(&subrs.entries, &charstrings.entries)?;

    // ---- what stays --------------------------------------------------------

    let mut budget = WORK_BUDGET;
    let mut spend = |steps: u64| -> Result<(), Type1Refusal> {
        budget = budget.checked_sub(steps).ok_or(Type1Refusal::Budget)?;
        Ok(())
    };

    let count = font.glyph_count();
    let mut keep: BTreeSet<u16> = glyphs
        .iter()
        .copied()
        .filter(|&glyph| usize::from(glyph) < count)
        .collect();
    // 9.6.5.2: `.notdef` is what a name the program lacks draws, so every
    // subset defines it (9.9.2).
    if let Some(notdef) = font.glyph_for_name(b".notdef") {
        keep.insert(notdef);
    }

    let defined: BTreeSet<usize> = subrs
        .entries
        .iter()
        .filter_map(|entry| match entry.key {
            Key::Subr(index) => stored(index),
            Key::Glyph(_) => None,
        })
        .collect();

    // Chapter 8.4: subroutines 0 to 3 are flex's and hint replacement's.
    let mut reached: BTreeSet<usize> = (0..4).filter(|i| defined.contains(i)).collect();
    let mut pending: Vec<u16> = keep.iter().copied().collect();
    let mut hint_subrs: Vec<usize> = Vec::new();
    while let Some(glyph) = pending.pop() {
        let (trace, steps) = font.trace_glyph(glyph).ok_or(Type1Refusal::Unreadable)?;
        spend(steps)?;
        take(&trace, &defined, &mut reached, &mut hint_subrs)?;
        // Chapter 6's `seac`: two StandardEncoding glyphs drawn as one.
        if let Some((base, accent)) = trace.seac {
            for code in [base, accent] {
                let component =
                    crate::encoding::base_glyph_name(crate::BaseEncoding::Standard, code)
                        .and_then(|name| font.glyph_for_name(name.as_bytes()));
                if let Some(component) = component {
                    if keep.insert(component) {
                        pending.push(component);
                    }
                }
            }
        }
    }
    // Chapter 8.1: the subroutine a hint replacement names is what an
    // interpreter that replaces hints calls, with whatever it calls.
    let mut hinted: BTreeSet<usize> = BTreeSet::new();
    while let Some(index) = hint_subrs.pop() {
        if !hinted.insert(index) {
            continue;
        }
        if !defined.contains(&index) {
            return Err(Type1Refusal::MissingSubroutine);
        }
        reached.insert(index);
        let (trace, steps) = font
            .trace_subr(index)
            .ok_or(Type1Refusal::MissingSubroutine)?;
        spend(steps)?;
        take(&trace, &defined, &mut reached, &mut hint_subrs)?;
    }

    // ---- the private section, edited ---------------------------------------

    // 7.3: `return`, behind `lenIV` zero lead bytes.
    let lead = [0u8; 16];
    let stub = encrypt(&[11], CHARSTRING_KEY, &lead[..layout.len_iv()]);
    let mut edits: Vec<(Range<usize>, Vec<u8>)> = Vec::new();
    for (glyph, entry) in charstrings.entries.iter().enumerate() {
        let kept = u16::try_from(glyph).is_ok_and(|glyph| keep.contains(&glyph));
        if !kept {
            let end = definition_end(private, entry, close)?;
            edits.push((entry.start..end, Vec::new()));
        }
    }
    for entry in &subrs.entries {
        let Key::Subr(index) = entry.key else {
            continue;
        };
        // An index the reader does not store is text it never reads.
        let Some(index) = stored(index) else {
            continue;
        };
        if reached.contains(&index) {
            continue;
        }
        // `<length> RD <bytes>` becomes the stub's, behind the entry's own
        // `dup <index>` and before its own `NP`.
        let mut replacement = format!("{} ", stub.len()).into_bytes();
        replacement.extend_from_slice(private.get(entry.rd.clone()).unwrap_or(b"RD".as_slice()));
        replacement.push(b' ');
        replacement.extend_from_slice(&stub);
        edits.push((entry.length..entry.data.end, replacement));
    }
    edits.sort_by_key(|(range, _)| range.start);

    let mut plain: Vec<u8> = Vec::with_capacity(close_end + 1);
    let mut at = 0usize;
    for (range, replacement) in &edits {
        if range.start < at || range.end > close_end {
            return Err(Type1Refusal::Entries);
        }
        plain.extend_from_slice(private.get(at..range.start).ok_or(Type1Refusal::Entries)?);
        plain.extend_from_slice(replacement);
        at = range.end;
    }
    plain.extend_from_slice(private.get(at..close_end).ok_or(Type1Refusal::Entries)?);
    // The token needs a delimiter, inside the encryption.
    plain.push(b'\n');

    // ---- the three segments ------------------------------------------------

    // 7.2: four zero lead bytes give a first ciphertext byte of 0xD9, neither
    // white space nor a hexadecimal digit.
    let encrypted = encrypt(&plain, EEXEC_KEY, &[0; 4]);
    let data = layout.data;
    let clear = data
        .get(..layout.encrypted_at)
        .ok_or(Type1Refusal::Unreadable)?;
    // The encrypted portion runs at least one byte past `closefile`: eexec
    // decrypts the delimiter that ends the token before it stops.
    let trailer_at = trailer_start(data, layout.encrypted_at + 4 + close_end + 1);
    let trailer = data.get(trailer_at..).unwrap_or_default();

    let mut out = Vec::with_capacity(clear.len() + encrypted.len() + trailer.len());
    out.extend_from_slice(clear);
    out.extend_from_slice(&encrypted);
    out.extend_from_slice(trailer);
    let lengths = [clear.len(), encrypted.len(), trailer.len()];

    verify(&font, &out, &keep, &mut spend)?;

    let names = keep
        .iter()
        .filter_map(|&glyph| font.glyph_name(usize::from(glyph)).map(<[u8]>::to_vec))
        .collect();
    Ok(Type1Subset {
        program: out,
        lengths,
        names,
    })
}

/// Folds one trace into what the subset keeps, refusing a bound by name.
fn take(
    trace: &Trace,
    defined: &BTreeSet<usize>,
    reached: &mut BTreeSet<usize>,
    hints: &mut Vec<usize>,
) -> Result<(), Type1Refusal> {
    if let Some(bound) = trace.bound {
        return Err(match bound {
            Bound::Depth => Type1Refusal::SubroutineDepth,
            Bound::Budget => Type1Refusal::Budget,
            Bound::Stack => Type1Refusal::StackOverflow,
            Bound::Missing => Type1Refusal::MissingSubroutine,
        });
    }
    for &index in &trace.called {
        // A call into a hole of the array runs nothing here and fails in an
        // interpreter that reads the array as written.
        if !defined.contains(&index) {
            return Err(Type1Refusal::MissingSubroutine);
        }
        reached.insert(index);
    }
    hints.extend(trace.hints.iter().copied());
    Ok(())
}

/// The text of the private section outside every entry's binary bytes, up to
/// `close`, as ranges: where a token can be looked for without reading a
/// charstring's ciphertext as text.
///
/// Refuses entries whose definitions — from `dup` or `/` to their last byte
/// — overlap: a `dup` the subroutine scan found inside a charstring's bytes
/// is no subroutine, and rewriting it would rewrite that charstring.
fn text_between(
    private: &[u8],
    subrs: &[Entry],
    charstrings: &[Entry],
    close: usize,
) -> Result<Vec<Range<usize>>, Type1Refusal> {
    let mut spans: Vec<(usize, Range<usize>)> = subrs
        .iter()
        .chain(charstrings)
        .map(|entry| (entry.start, entry.data.clone()))
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    let mut text = Vec::with_capacity(spans.len() + 1);
    let mut at = 0usize;
    for (start, data) in spans {
        if start < at || data.start < start || data.end > close || data.end > private.len() {
            return Err(Type1Refusal::Entries);
        }
        text.push(at..data.start);
        at = data.end;
    }
    text.push(at..close);
    Ok(text)
}

/// The index the reader stores a `/Subrs` entry at (`type1.rs`): a negative
/// one at 0, and one past 65 535 nowhere.
fn stored(index: i64) -> Option<usize> {
    usize::try_from(index.max(0))
        .ok()
        .filter(|index| *index < MAX_SUBRS)
}

/// Whether the program is synthetic or hybrid (chapter 10.3): `FontDirectory`
/// or `hires` as a token after `/Private`, in the text between the entries.
fn synthetic_or_hybrid(private: &[u8], text: &[Range<usize>]) -> bool {
    let from = find(private, b"/Private").unwrap_or(0);
    text.iter().any(|range| {
        let start = range.start.max(from);
        private
            .get(start..range.end.max(start))
            .is_some_and(|span| has_token(span, b"FontDirectory") || has_token(span, b"hires"))
    })
}

/// Whether `word` stands in `text` as a whole token: bounded at each end by
/// the text's end, white space or a PostScript delimiter, a leading `/`
/// included.
fn has_token(text: &[u8], word: &[u8]) -> bool {
    let regular = |b: u8| !b.is_ascii_whitespace() && !b"()<>[]{}/%".contains(&b);
    let mut from = 0usize;
    while let Some(at) = text.get(from..).and_then(|rest| find(rest, word)) {
        let start = from + at;
        let end = start + word.len();
        let before = start.checked_sub(1).and_then(|i| text.get(i)).copied();
        let after = text.get(end).copied();
        if !before.is_some_and(regular) && !after.is_some_and(regular) {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Refuses two charstrings under one name and one subroutine index given
/// twice.
fn distinct(subrs: &[Entry], charstrings: &[Entry]) -> Result<(), Type1Refusal> {
    let mut names: BTreeSet<&[u8]> = BTreeSet::new();
    for entry in charstrings {
        if let Key::Glyph(name) = &entry.key {
            if !names.insert(name.as_slice()) {
                return Err(Type1Refusal::DuplicateName);
            }
        }
    }
    let mut indices: BTreeSet<usize> = BTreeSet::new();
    for entry in subrs {
        if let Key::Subr(index) = entry.key {
            if stored(index).is_some_and(|index| !indices.insert(index)) {
                return Err(Type1Refusal::DuplicateSubroutine);
            }
        }
    }
    Ok(())
}

/// Where a charstring's definition ends: past the token after its binary
/// bytes — `ND`, `|-` or whatever the program named it — and the line end
/// after it. A token that is no such name, or one past `close`, is refused.
fn definition_end(private: &[u8], entry: &Entry, close: usize) -> Result<usize, Type1Refusal> {
    let mut at = entry.data.end;
    while private.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    let token = at;
    while private.get(at).is_some_and(|b| !b.is_ascii_whitespace()) {
        at += 1;
    }
    // No token, or the next definition's name where `ND` should be: removing
    // up to there would take the next charstring with this one.
    if at == token || private.get(token) == Some(&b'/') || at > close {
        return Err(Type1Refusal::Entries);
    }
    match (private.get(at), private.get(at + 1)) {
        (Some(b'\r'), Some(b'\n')) => at += 2,
        (Some(b'\r' | b'\n'), _) => at += 1,
        _ => {}
    }
    Ok(at.min(close))
}

/// Where the fixed-content portion begins: the first `cleartomark` at or after
/// `from`, back through the zeros and white space before it, but not before
/// `from`; the end of the program when there is none (Table 125's `Length3`
/// of 0).
fn trailer_start(data: &[u8], from: usize) -> usize {
    let Some(rest) = data.get(from..) else {
        return data.len();
    };
    let Some(mark) = find(rest, b"cleartomark") else {
        return data.len();
    };
    let mut at = from + mark;
    while at > from
        && data
            .get(at - 1)
            .is_some_and(|b| *b == b'0' || b.is_ascii_whitespace())
    {
        at -= 1;
    }
    at
}

/// Outlines and advances every kept glyph in both programs: the subset must
/// read, hold exactly the kept glyphs, and draw each as the program does.
fn verify(
    font: &Type1<'_>,
    subset: &[u8],
    keep: &BTreeSet<u16>,
    spend: &mut impl FnMut(u64) -> Result<(), Type1Refusal>,
) -> Result<(), Type1Refusal> {
    let after = Type1::parse(subset).ok_or(Type1Refusal::Mismatch)?;
    if after.glyph_count() != keep.len() {
        return Err(Type1Refusal::Mismatch);
    }
    for &glyph in keep {
        let name = font
            .glyph_name(usize::from(glyph))
            .ok_or(Type1Refusal::Mismatch)?;
        let there = after.glyph_for_name(name).ok_or(Type1Refusal::Mismatch)?;
        let (before_outline, before_steps) = font.outline_counted(glyph);
        let (after_outline, after_steps) = after.outline_counted(there);
        spend(before_steps.saturating_add(after_steps))?;
        let same = match (before_outline, after_outline) {
            (Some(a), Some(b)) => a.segments == b.segments,
            (None, None) => true,
            _ => false,
        };
        if !same || font.advance(glyph) != after.advance(there) {
            return Err(Type1Refusal::Mismatch);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
