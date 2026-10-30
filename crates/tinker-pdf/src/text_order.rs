//! Ruling 14: extracted text is reported in **logical** order.
//!
//! A page draws its glyphs in *visual* order — left to right along the
//! baseline — and a right-to-left script is read the other way. So the
//! characters `tinker-pdf-content`'s `TextDevice` collects in content-stream
//! order are, for a producer that draws visually, an Arabic or Hebrew line
//! read backwards; and for one that draws each glyph in reading order with the
//! pen moving left, they are already right. [`into_logical_order`] answers
//! both the same way, because it never trusts the stream's order for a line
//! that holds a right-to-left character — including for where the line ends:
//!
//! 0. **A line the stream's order cut is one line again.** A right-to-left
//!    word drawn in reading order as several text objects, inside a
//!    left-to-right line, reaches `TextDevice` as pieces on one baseline,
//!    since a line resumes after an `ET` only where it stopped. Consecutive
//!    lines that meet end to end on one baseline make a chain, and in a chain
//!    any piece of which holds a right-to-left character, a piece joins the
//!    one before it where either holds one ([`rejoin_split_lines`]). The
//!    chain is joined whole where the bracket tie-break turns it read so — a
//!    `(` cut off at the right end of a right-to-left line from the Latin run
//!    it opens, neither holding a right-to-left character, with the rest.
//! 1. **Marks are kept with their base.** A character whose first code point
//!    is a nonspacing mark (`Bidi_Class` `NSM`) belongs to the base glyph it
//!    sits on — the neighbour, in content order, whose extent along the
//!    baseline holds the mark's centre, or failing that the nearer one. Which
//!    side of its base a producer wrote a mark on is a producer's habit, and
//!    the pairing has to survive both habits.
//! 2. **The line is put in visual order**, by where each base glyph starts
//!    along the baseline. A stable sort, so glyphs a producer overprinted at
//!    one position keep the order it wrote them in.
//! 3. **The visual line is read back through UAX #9**, by
//!    [`tinker_pdf_shape::bidi::logical_order`]: the order whose text, drawn
//!    by the algorithm, is the line as it stands. Not L2 applied to levels
//!    resolved over the drawn line — those are other levels, because W2, W5
//!    and W7 look backwards for a strong character and N1 at both neighbours,
//!    and a `%` drawn beside Arabic digits would join them. Every order is
//!    checked forwards before it is used.
//! 4. **The paragraph direction is read off the drawn line**
//!    ([`tinker_pdf_shape::bidi::drawn_direction`]): P2's first strong
//!    character is the leftmost strong one for a left-to-right paragraph and
//!    the rightmost for a right-to-left one, so a line whose two ends agree is
//!    that direction, and a line whose ends disagree goes by its majority.
//!    Classes are `Bidi_Class`, so N'Ko and Adlam read right to left like
//!    Hebrew. **One tie-break** (the ruling as amended 10 October 2026): a
//!    line with no left-to-right strong character whose leftmost unit is a
//!    strong right-to-left character and whose rightmost is punctuation
//!    (`CS`, `ON`, `ES` or `ET`, but not an opening bracket or quotation
//!    mark; whitespace and invisible format characters passed over) reads
//!    as a left-to-right paragraph **if a left-to-right paragraph draws
//!    it** — a Hebrew or Arabic word quoted in left-to-right
//!    text with its comma after it, `חו,` drawn `וח,`, which both paragraphs
//!    draw alike and which read right to left came back `,חו`. Where no
//!    left-to-right reading draws the line, it stays right to left. **And a
//!    bracket tie-break** (amended 10 October 2026, a second time;
//!    accepted by the owner on 11 October 2026): a line that draws a closing
//!    bracket left of an opening one of its kind, `)histone deacetylase(`,
//!    is read in the other direction where that direction draws the line,
//!    pairs more brackets in all, and pairs more of them facing outward and
//!    hugging what they enclose — a pair faces outward only at a
//!    right-to-left level whose `/ToUnicode` names the typed character, as
//!    this engine's writers name it, and two brackets that are no pair, a
//!    list marker's `1)` and a later `(`, pair only with the spaces outside
//!    them turned inside.
//!    [`TextLine::rtl`] is set to the direction read, for every line this
//!    touches.
//!
//! A line with no right-to-left character is left exactly as it was
//! collected, byte for byte: no sort, no reorder. That is what keeps every
//! left-to-right page this engine has ever extracted unchanged by the ruling,
//! and it is asserted rather than assumed.
//!
//! # What it does not undo
//!
//! - **Mirroring (rule L4).** A right-to-left `(` is drawn with the glyph of
//!   `)`, and whether a producer's `/ToUnicode` names the character it stands
//!   for or the glyph's own shape is not recoverable from the line. This
//!   engine's own writers name the character, so nothing is swapped.
//! - **Texts UAX #9 draws alike.** The algorithm is not one-to-one: in a
//!   right-to-left paragraph `שלום 2026 now` and `שלום now 2026` are one
//!   picture, and this reads it as the second. `logical_order` states which
//!   one it returns; every answer draws the line as drawn.
//! - **Bracket pairs.** Rule N0 pairs brackets in the logical text, and a
//!   right-to-left run draws them mirrored, so a line holding a bracket pair
//!   can read back as another text the same picture could be:
//!   `BidiCharacterTest.txt` has 669 such cases of 91 616, and no others.
//!   And a pair drawn facing inward does not say which paragraph drew it: a
//!   line with no `dir`, Arabic at both ends and `(HDAC).` between is drawn
//!   exactly as a right-to-left paragraph whose `/ToUnicode` names the
//!   mirrored glyph draws another text, and reads as that one, `.)HDAC(`.
//! - **The paragraph.** A line is resolved on its own. A line of a
//!   right-to-left paragraph that begins and ends with Latin reads as a
//!   left-to-right one, and its runs come back each in the right order but
//!   placed as a left-to-right paragraph would place them. A line holding
//!   one bracket of a pair a line break parted, or quotation marks, which
//!   BD16 does not pair, gives the bracket tie-break nothing to count: where
//!   its ends disagree its majority decides, so `deacetylase). وقد أصبحنا`
//!   reads `وقد أصبحنا .)deacetylase`. And a line of a
//!   right-to-left paragraph that holds nothing left to right and *opens*
//!   with a mark of `Bidi_Class` `CS`, `ON`, `ES` or `ET` that is not `Ps` or
//!   `Pi` — a dash (`— שלום` drawn `םולש —`), a bullet, `*`, `#`, `%`, an
//!   ellipsis, or a straight or closing-form quotation mark used to open — is
//!   the tie-break's shape, a left-to-right paragraph draws it too, and it
//!   reads with the mark at its end, `שלום —`: the price of the tie-break,
//!   chosen because a quoted word's trailing punctuation is far commoner
//!   (`text_logical_order.rs` pins it by name). An opening bracket or
//!   quotation mark does not pay where `/ToUnicode` names the typed character,
//!   `(١) بند`, `“שלום` (where it names the mirrored glyph, `»` for `«`, the
//!   line ends in a closing mark and pays like a dash); nor does a mark a
//!   European number follows, `(1) פריט`, `— 2026 שלום`, `• 5 תפוחים`,
//!   since no left-to-right paragraph draws one next to the mark.
//!   Arabic-Indic digits (`AN`) do not spare it, `— ١ بند` reading
//!   `١ بند —`, nor does a number further on, `— שלום 5 חו`.
//! - **Vertical lines**, which UAX #9 does not describe.
//!
//! [`crate::Page::text_with`] with [`TextOptions::content_order`] is the
//! opt-out: the characters in the order the content stream showed them, as
//! `TextDevice` collected them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use tinker_pdf_content::{Quad, TextBlock, TextChar, TextLine, TextPage, WritingMode};
use tinker_pdf_shape::bidi::{
    drawn_reading, faces_outward, logical_order, BaseDirection, DrawnReading,
};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

/// How [`crate::Page::text_with`] orders a page's lines.
///
/// `Default` is [`crate::Page::text`]'s behaviour, to the byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextOptions {
    /// Report every line's characters in the order the content stream showed
    /// them, rather than in logical order (ruling 14).
    ///
    /// For a caller that needs the stream's own order — a redaction tool
    /// matching operators, a diff of two producers' output — and for nothing
    /// that reads the text as text.
    pub content_order: bool,
}

/// What a page says of the direction its lines are read in (ISO 32000-2
/// §14.8.5.4.2): each marked sequence's, by `(stream, /MCID)`, where its
/// structure element declares one (`structure::declared_directions`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Declared<'a> {
    /// Each sequence's, where its element declares one.
    BySequence(&'a BTreeMap<(u64, u32), BaseDirection>),
}

impl Declared<'_> {
    /// The direction `chars` are read in: the one every character of them
    /// that draws something belongs to a sequence declaring, where all agree.
    fn of(self, chars: &[TextChar]) -> Option<BaseDirection> {
        match self {
            Declared::BySequence(declared) => {
                let mut found = None;
                for c in chars.iter().filter(|c| !c.text.trim().is_empty()) {
                    let direction = c.mcid.and_then(|mcid| declared.get(&(c.stream, mcid)))?;
                    if found.is_some_and(|seen| seen != *direction) {
                        return None;
                    }
                    found = Some(*direction);
                }
                found
            }
        }
    }
}

/// Puts every line of `page` into logical order, each in the direction its
/// page declares where it declares one, and returns how many lines it
/// changed.
pub(crate) fn into_logical_order(page: &mut TextPage, declared: Option<Declared<'_>>) -> usize {
    in_logical_order(page, None, declared)
}

/// [`into_logical_order`], asking `known` for each line's reading before
/// [`drawn_reading`] searches for it, and keeping what that finds there
/// ([`Readings`]): for a caller that reads lines of the same characters over
/// and over.
pub(crate) fn into_logical_order_knowing(
    page: &mut TextPage,
    known: &mut Readings,
    declared: Option<Declared<'_>>,
) -> usize {
    in_logical_order(page, Some(known), declared)
}

/// The `(stream, /MCID)` of each character in a block of `page` that holds a
/// right-to-left character: the sequences whose declared direction
/// [`into_logical_order`] may read a line in. The whole block's, as the lines
/// it reads are joined within a block where the stream split them
/// ([`rejoin_split_lines`]), a piece with no right-to-left character among
/// them.
pub(crate) fn sequences_read(page: &TextPage) -> BTreeSet<(u64, u32)> {
    page.blocks
        .iter()
        .filter(|block| block.lines.iter().any(|line| holds_rtl(&line.chars)))
        .flat_map(|block| block.lines.iter())
        .flat_map(|line| line.chars.iter())
        .filter_map(|c| c.mcid.map(|mcid| (c.stream, mcid)))
        .collect()
}

/// [`into_logical_order`], each line's reading asked of `known` where there
/// is one.
fn in_logical_order(
    page: &mut TextPage,
    mut known: Option<&mut Readings>,
    declared: Option<Declared<'_>>,
) -> usize {
    let mut changed = 0usize;
    let TextPage { blocks, stated, .. } = page;
    let mut reads: Vec<Vec<Option<Read>>> = Vec::with_capacity(blocks.len());
    for block in blocks.iter_mut() {
        let (joined, read) = rejoin_split_lines(&mut block.lines);
        changed += joined;
        reads.push(read);
    }
    if !stated.is_empty() {
        keep_stated_on_first_line(blocks, Some(&mut reads));
    }
    for (block, mut read) in blocks.iter_mut().zip(reads) {
        for (at, line) in block.lines.iter_mut().enumerate() {
            let read = read.get_mut(at).and_then(Option::take);
            let direction = declared.and_then(|declared| declared.of(&line.chars));
            let moved = match known.as_deref_mut() {
                Some(known) => logical_line_reading(line, read, stated, direction, |visual| {
                    known.reading(visual, direction)
                }),
                None => logical_line(line, read, stated, direction),
            };
            if moved {
                changed += 1;
            }
        }
    }
    changed
}

/// Puts each `/ActualText` span's text where its glyphs stand, on a page in
/// the order its content stream drew it ([`TextOptions::content_order`]): at
/// the span's first glyph, on the first line the page reads its glyphs on
/// (14.9.4, `TextChar::stated`).
pub(crate) fn state_in_content_order(page: &mut TextPage) {
    let TextPage { blocks, stated, .. } = page;
    if stated.is_empty() {
        return;
    }
    keep_stated_on_first_line(blocks, None);
    for line in blocks.iter_mut().flat_map(|block| block.lines.iter_mut()) {
        state_in_place(line, stated);
    }
}

/// Drops each `/ActualText` span's glyphs from every line after the first the
/// page reads them on, where its text will stand; a line or a block left with
/// nothing goes, and so does its entry in `reads`. A line that loses glyphs
/// so is read again rather than as `reads` found it.
fn keep_stated_on_first_line(
    blocks: &mut Vec<TextBlock>,
    mut reads: Option<&mut Vec<Vec<Option<Read>>>>,
) {
    let mut placed: BTreeSet<u32> = BTreeSet::new();
    for (b, block) in blocks.iter_mut().enumerate() {
        let mut at = 0usize;
        while at < block.lines.len() {
            let line = &mut block.lines[at];
            let here: BTreeSet<u32> = line.chars.iter().filter_map(|c| c.stated).collect();
            if here.iter().any(|span| placed.contains(span)) {
                line.chars
                    .retain(|c| c.stated.is_none_or(|span| !placed.contains(&span)));
                line.text = line.chars.iter().map(|c| c.text.as_str()).collect();
                if let Some(quad) = line.chars.iter().map(|c| c.quad).reduce(enclose) {
                    line.quad = quad;
                }
                if let Some(read) = reads.as_deref_mut().and_then(|reads| reads.get_mut(b)) {
                    if let Some(slot) = read.get_mut(at) {
                        *slot = None;
                    }
                }
            }
            placed.extend(here);
            if block.lines[at].chars.is_empty() {
                block.lines.remove(at);
                if let Some(read) = reads.as_deref_mut().and_then(|reads| reads.get_mut(b)) {
                    if at < read.len() {
                        read.remove(at);
                    }
                }
                continue;
            }
            at += 1;
        }
        if let Some(quad) = block.lines.iter().map(|l| l.quad).reduce(enclose) {
            block.quad = quad;
        }
    }
    let mut b = 0usize;
    blocks.retain(|block| {
        let keep = !block.lines.is_empty();
        if !keep {
            if let Some(reads) = reads.as_deref_mut() {
                if b < reads.len() {
                    reads.remove(b);
                }
            }
        } else {
            b += 1;
        }
        keep
    });
}

/// Puts each span `line` holds in its text, at the span's first glyph in the
/// line's order, and leaves the line's other characters where they are: on a
/// line in the content stream's order, and on one ruling 14 does not reorder.
/// Returns whether it put any.
fn state_in_place(line: &mut TextLine, stated: &[String]) -> bool {
    let mut spans: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (at, c) in line.chars.iter().enumerate() {
        if let Some(span) = c.stated {
            spans.entry(span).or_default().push(at);
        }
    }
    if spans.is_empty() {
        return false;
    }
    let chars = std::mem::take(&mut line.chars);
    let mut out = Vec::with_capacity(chars.len());
    for (at, c) in chars.iter().enumerate() {
        match c.stated {
            None => out.push(c.clone()),
            Some(span) => {
                if let Some(held) = spans.get(&span).filter(|held| held.first() == Some(&at)) {
                    let glyphs: Vec<&TextChar> =
                        held.iter().filter_map(|k| chars.get(*k)).collect();
                    out.extend(stated_chars(stated_text(stated, span), &glyphs, span));
                }
            }
        }
    }
    line.chars = out;
    line.text = line.chars.iter().map(|c| c.text.as_str()).collect();
    true
}

/// Span `span`'s text, or nothing where the page records none.
fn stated_text(stated: &[String], span: u32) -> &str {
    stated
        .get(usize::try_from(span).unwrap_or(usize::MAX))
        .map_or("", String::as_str)
}

/// The longest glyph text, in characters, [`stated_chars`] asks for at each
/// step: a ligature's glyph is a few characters, and a longer text is no
/// glyph's a reader names one character at a time.
const MAX_GLYPH_TEXT: usize = 16;

/// The characters an `/ActualText` span's `text` stands as (14.9.4): each on a
/// glyph of the span that draws it — the first of `glyphs`, in the order the
/// content stream drew them, not yet taken, whose text `text` continues with
/// — with that glyph's quad, `/MCID`, stream and font, so that search finds
/// the text where it is drawn and the structure tree's order holds; and a
/// character no glyph draws on the span's box. Each step asks each length of
/// glyph text up to [`MAX_GLYPH_TEXT`] once, so it is linear in the text.
fn stated_chars(text: &str, glyphs: &[&TextChar], span: u32) -> Vec<TextChar> {
    let Some(first) = glyphs.first() else {
        return Vec::new();
    };
    let mut by_text: BTreeMap<&str, VecDeque<usize>> = BTreeMap::new();
    let mut longest = 0usize;
    for (at, glyph) in glyphs.iter().enumerate() {
        if glyph.text.is_empty() {
            continue;
        }
        longest = longest.max(glyph.text.chars().count());
        by_text
            .entry(glyph.text.as_str())
            .or_default()
            .push_back(at);
    }
    let longest = longest.min(MAX_GLYPH_TEXT);
    let whole = glyphs
        .iter()
        .map(|g| g.quad)
        .reduce(enclose)
        .unwrap_or(first.quad);
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some(next) = rest.chars().next() {
        let mut best: Option<(usize, usize)> = None;
        let mut end = 0usize;
        for c in rest.chars().take(longest) {
            end += c.len_utf8();
            if let Some(&at) = rest
                .get(..end)
                .and_then(|p| by_text.get(p))
                .and_then(VecDeque::front)
            {
                if best.is_none_or(|(taken, _)| at < taken) {
                    best = Some((at, end));
                }
            }
        }
        match best.and_then(|(at, end)| Some((glyphs.get(at)?, end))) {
            Some((glyph, end)) => {
                if let Some(queue) = rest.get(..end).and_then(|p| by_text.get_mut(p)) {
                    queue.pop_front();
                }
                let mut c = (*glyph).clone();
                c.stated = Some(span);
                out.push(c);
                rest = rest.get(end..).unwrap_or_default();
            }
            None => {
                out.push(TextChar {
                    text: next.to_string(),
                    quad: whole,
                    size: first.size,
                    origin: first.origin,
                    mcid: first.mcid,
                    stream: first.stream,
                    font: first.font.clone(),
                    stated: Some(span),
                });
                rest = rest.get(next.len_utf8()..).unwrap_or_default();
            }
        }
    }
    out
}

/// `units` with each `/ActualText` span's clusters one unit, where the first
/// of them is drawn, read as U+FFFC OBJECT REPLACEMENT CHARACTER: a neutral,
/// which takes its level from the letters either side (14.9.4).
fn stated_units(units: Units, chars: &[TextChar]) -> Units {
    let mut first: BTreeMap<u32, usize> = BTreeMap::new();
    let mut clusters: Vec<Vec<usize>> = Vec::with_capacity(units.clusters.len());
    let mut texts: Vec<String> = Vec::with_capacity(units.texts.len());
    for (cluster, text) in units.clusters.into_iter().zip(units.texts) {
        let span = cluster
            .first()
            .and_then(|at| chars.get(*at))
            .and_then(|c| c.stated);
        match span {
            Some(span) => match first.get(&span) {
                Some(&unit) => {
                    if let Some(held) = clusters.get_mut(unit) {
                        held.extend(cluster);
                    }
                }
                None => {
                    first.insert(span, clusters.len());
                    clusters.push(cluster);
                    texts.push("\u{FFFC}".to_owned());
                }
            },
            None => {
                clusters.push(cluster);
                texts.push(text);
            }
        }
    }
    Units { clusters, texts }
}

/// Whether `text`'s first strong character reads right to left: P2 of a text
/// a span states in logical order.
fn first_strong_rtl(text: &str) -> bool {
    text.chars()
        .find_map(|c| match bidi_class(c) {
            BidiClass::L => Some(false),
            BidiClass::R | BidiClass::AL => Some(true),
            _ => None,
        })
        .unwrap_or(false)
}

/// The readings [`drawn_reading`] found of lines of units, kept for a caller
/// that reads lines of the same characters over and over
/// ([`into_logical_order_knowing`]).
///
/// A line's reading is a function of its units alone, the text of each of
/// its characters with its marks in the order drawn ([`drawn_units`]): a
/// line of the units of one read before is read as that one was, wherever
/// its characters lie. The read-back of an EPUB page reads a line in every
/// drawing it tries, and under a transform on the page and in its runs' own
/// space as well (`epub::paint::Fonts::set_readings`), and a reader parts a
/// letter-spaced line at every text object into pieces of a word or two that
/// the line repeats: each line of units [`logical_line`] reads is searched
/// once, and found again wherever it is met again. A chain of pieces
/// [`turned_whole`] reads, to ask whether the bracket tie-break turns it, is
/// searched each time.
#[derive(Clone, Debug, Default)]
pub(crate) struct Readings {
    /// Each line of units searched, by its [`fingerprint`].
    found: BTreeMap<u64, Vec<Searched>>,
}

/// A line of units [`Readings`] searched: its units' text, where each unit
/// ends in that text, the direction declared for it, and its reading.
type Searched = (String, Vec<usize>, Option<BaseDirection>, DrawnReading);

impl Readings {
    /// What [`search`] finds for `visual` read in `declared`: what it found
    /// for these units so before, or else what it finds now, kept.
    fn reading(&mut self, visual: &[&str], declared: Option<BaseDirection>) -> DrawnReading {
        let lines = self.found.entry(fingerprint(visual)).or_default();
        if let Some((_, _, _, reading)) = lines.iter().find(|(text, ends, direction, _)| {
            *direction == declared && holds_units(text, ends, visual)
        }) {
            return reading.clone();
        }
        let reading = search(visual, declared);
        let mut text = String::new();
        let mut ends = Vec::with_capacity(visual.len());
        for unit in visual {
            text.push_str(unit);
            ends.push(text.len());
        }
        lines.push((text, ends, declared, reading.clone()));
        reading
    }
}

/// FNV-1a over `visual`'s units, each followed by a byte no UTF-8 text holds,
/// so that the same text parted into other units prints otherwise.
fn fingerprint(visual: &[&str]) -> u64 {
    let mut print = 0xcbf2_9ce4_8422_2325_u64;
    for unit in visual {
        for byte in unit.bytes().chain(core::iter::once(0xff)) {
            print = (print ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    print
}

/// Whether `visual` is the units `text` holds, each ending where `ends` says.
fn holds_units(text: &str, ends: &[usize], visual: &[&str]) -> bool {
    ends.len() == visual.len()
        && ends
            .iter()
            .zip(visual)
            .try_fold(0usize, |start, (end, unit)| {
                (text.get(start..*end) == Some(*unit)).then_some(*end)
            })
            .is_some()
}

/// [`drawn_reading`]'s search for a line's reading, or in a direction the
/// page declares, [`logical_order`]'s for it, no tie-break asked: the page's
/// word, not P2 read off the line. Counted only under `cfg(test)`, where the
/// tests hold how often the read-back searches by count ([`Readings`]), not
/// by a clock.
fn search(visual: &[&str], declared: Option<BaseDirection>) -> DrawnReading {
    #[cfg(test)]
    SEARCHES.with(|searches| searches.set(searches.get().saturating_add(1)));
    match declared {
        Some(direction) => DrawnReading {
            direction,
            order: logical_order(visual, direction),
            turned_by_brackets: false,
        },
        None => drawn_reading(visual),
    }
}

/// Joins consecutive lines of one block that the content stream's order split
/// and that are one line on the page: on one baseline, end to end, a
/// right-to-left character among them. Returns how many joins it made, and,
/// for each line it leaves, the reading [`drawn_reading`] found for it where
/// choosing whether to join it read it already — so [`logical_line`] does not
/// read it again.
///
/// # Why a line drawn in reading order can arrive in pieces
///
/// `TextDevice` resumes a line after an `ET` only where the last glyph
/// stopped, give or take half an em — which is what keeps two cells of a table
/// row two lines. A producer that draws a right-to-left word in reading order
/// moves its pen left, and when that word is several text objects inside a
/// left-to-right line — `a ب<span>ح</span>م b`, each span its own object — the
/// object drawn first is the rightmost: `a ` ends at one place and `ب` starts
/// two letters further on, so the line was cut there, and again after `م`,
/// which ends at the word's left edge while ` b` starts at its right. Three
/// lines on one baseline came back, and the reading order below, which never
/// trusts the stream's order, was handed three lines to order instead of one.
/// Ruling 14 says the content stream's order decides nothing; this is that
/// sentence applied to where a line ends as well as to how one is read.
///
/// So a line in a row is the rest of the one before it — a **chain** of
/// pieces — when their baselines agree to half an em and their extents along
/// it meet to within the same half em the device's own rule allows; and a
/// chain is joined into one line when **any** of its pieces holds a
/// right-to-left character. A chain with none is left exactly as it was
/// collected — a left-to-right page is not touched — and so is a vertical
/// line. Within a chain, a piece joins the one before it where either holds
/// a right-to-left character, as at 9297d2d: those pieces are the chain's
/// **groups**, and each is one line.
///
/// # And the chain, not the pair, holds a bracket the line pairs
///
/// A right-to-left paragraph's line that opens with a left-to-right run in
/// brackets — `(histone deacetylase). وقد أصبحنا`, a paragraph narrowed so
/// the line breaks before the bracket — is three runs at three levels, and an
/// EPUB writes each as its own text object in reading order: the `(` at the
/// line's right end, then the Latin words left of it, then the Arabic left
/// of those. The device cut the line twice. The join asked whether either of
/// the **two** lines it was joining held a right-to-left character, and the
/// `(` and the Latin words hold none, so they stayed apart and only the Latin
/// and the Arabic were joined: the `(` was read as a line of its own, and the
/// rest as a line with no bracket to pair (ROADMAP K-11). So a chain of more
/// than one group is read whole too, and joined whole where the bracket
/// tie-break turns it read so ([`DrawnReading::turned_by_brackets`]): a pair
/// drawn facing outward across its groups, which only the whole chain pairs.
/// That reading is the one handed on.
///
/// Only there. Joined whole wherever any piece held a right-to-left
/// character, as the K-11 fix first had it, a chain whose groups each read
/// right fell into other limits: the line after K-11's, `deacetylase).
/// מִלְחָמָה סֵפֶר בַּיִת.`, is three pieces too, and two groups —
/// `deacetylase` and `). מִלְחָמָה …` — that each read right; whole, it is
/// a bracket a line break parted, its Latin letters as many as its Hebrew
/// ones, and read `.מִלְחָמָה סֵפֶר בַּיִת .)deacetylase` (review of the K-11
/// fix). A chain the tie-break does not turn is joined group by group.
///
/// # And two lines on top of each other are two drawings
///
/// **Meeting is not overlapping.** The pieces of a split line lie end to end,
/// touching or a kerned hair into each other; text overdrawn in a second text
/// object — a fake bold, a hand-made shadow, a word stroked twice — lies over
/// the first copy. Joined, the two copies became one line whose characters
/// [`logical_line`] interleaved: `אבג` drawn twice half a point apart read
/// back `אאבבגג`, where every reader before this join read two lines of
/// `אבג` (review of lane 8C). So a line overlapping the chain so far by more
/// than half the shorter one's extent is not its rest. A zero-width mark in
/// an object of its own overlaps nothing and still joins its base's line.
///
/// # And a join costs the piece it joins
///
/// What a join asks of the chain so far — where it lies along its baseline,
/// which way that baseline runs, whether it holds a right-to-left character —
/// is kept in a [`Reach`] and grown by each piece, never read again from the
/// chain's first character; and a chain is joined once, when it ends. Read
/// again, a line rejoined from `n` pieces cost `n²`: a right-to-left word
/// drawn as one text object per letter or two is all a page needs to be that
/// line, and this runs on every PDF's default extraction (review of lane 8C).
/// A chain of more than one group is asked for the bracket tie-break's shape
/// ([`faces_outward`]), linear in its characters, and read only where it has
/// it — a reading [`logical_line`] is handed where the chain is joined whole,
/// and that a group joined on its own would have read in part anyway.
fn rejoin_split_lines(lines: &mut Vec<TextLine>) -> (usize, Vec<Option<Read>>) {
    // First, where each chain starts, whether it holds a right-to-left
    // character, and where each of its groups starts: read off the pieces
    // where they lie, nothing moved yet.
    let mut chains: Vec<Chain> = Vec::new();
    let mut reach: Option<Reach> = None;
    // The group being gathered: its reach, and whether it holds a
    // right-to-left character.
    let mut group: Option<(Reach, bool)> = None;
    for (at, line) in lines.iter().enumerate() {
        let rtl = holds_rtl(&line.chars);
        if let (Some(reached), Some(chain)) = (reach.as_mut(), chains.last_mut()) {
            if let Some(along) = continues_on_page(reached, line) {
                reached.grow(lines.get(chain.start..=at).unwrap_or_default(), along);
                chain.holds |= rtl;
                let from = chain.groups.last().copied().unwrap_or(chain.start);
                let grouped = match group.as_mut() {
                    Some((gathered, holds)) if *holds || rtl => continues_on_page(gathered, line)
                        .is_some_and(|along| {
                            gathered.grow(lines.get(from..=at).unwrap_or_default(), along);
                            *holds |= rtl;
                            true
                        }),
                    _ => false,
                };
                if !grouped {
                    chain.groups.push(at);
                    group = Some((Reach::of(line), rtl));
                }
                continue;
            }
        }
        let started = Reach::of(line);
        group = Some((started.clone(), rtl));
        reach = Some(started);
        chains.push(Chain {
            start: at,
            holds: rtl,
            groups: vec![at],
        });
    }

    // Then each chain of more than one piece that holds a right-to-left
    // character becomes one line where its groups are one, or where the
    // bracket tie-break turns it whole; otherwise each of its groups does.
    // Every other piece is kept as collected.
    let mut joined = 0usize;
    let mut out: Vec<TextLine> = Vec::with_capacity(chains.len());
    let mut read: Vec<Option<Read>> = Vec::with_capacity(chains.len());
    let mut taken = std::mem::take(lines).into_iter().enumerate().peekable();
    let mut ends = chains.iter().skip(1).map(|chain| chain.start);
    for chain in &chains {
        let end = ends.next();
        let mut pieces: Vec<TextLine> = Vec::new();
        while let Some((_, piece)) = taken.next_if(|(at, _)| end.is_none_or(|end| *at < end)) {
            pieces.push(piece);
        }
        if !chain.holds || pieces.len() < 2 {
            read.extend(pieces.iter().map(|_| None));
            out.extend(pieces);
            continue;
        }
        let whole = if chain.groups.len() < 2 {
            Some(None)
        } else {
            turned_whole(&pieces).map(Some)
        };
        if let Some(found) = whole {
            joined += pieces.len() - 1;
            out.push(join(pieces));
            read.push(found);
            continue;
        }
        // The groups, as the pairwise rule joined them.
        let mut starts = chain
            .groups
            .iter()
            .skip(1)
            .map(|at| at - chain.start)
            .peekable();
        let mut gathered: Vec<TextLine> = Vec::new();
        for (at, piece) in pieces.into_iter().enumerate() {
            if starts.next_if(|start| *start == at).is_some() {
                joined += gathered.len().saturating_sub(1);
                out.push(join(std::mem::take(&mut gathered)));
                read.push(None);
            }
            gathered.push(piece);
        }
        joined += gathered.len().saturating_sub(1);
        out.push(join(gathered));
        read.push(None);
    }
    *lines = out;
    (joined, read)
}

/// A chain of pieces [`rejoin_split_lines`] found where they lie: its first piece's
/// index, whether any piece holds a right-to-left character, and where each
/// of its groups starts — the pieces the pairwise rule joined, as at
/// 9297d2d: a piece joined the group before it where either holds a
/// right-to-left character.
struct Chain {
    start: usize,
    holds: bool,
    groups: Vec<usize>,
}

/// The reading of a chain's pieces joined whole, where the bracket tie-break
/// turns it — a pair of brackets drawn facing outward across the pieces,
/// which only the chain read whole pairs — and `None` otherwise, where the
/// chain's groups are joined as the pairwise rule joined them. See
/// [`rejoin_split_lines`].
fn turned_whole(pieces: &[TextLine]) -> Option<Read> {
    let chars: Vec<TextChar> = pieces
        .iter()
        .flat_map(|piece| piece.chars.iter().cloned())
        .collect();
    let units = drawn_units(&chars);
    let visual: Vec<&str> = units.texts.iter().map(String::as_str).collect();
    if !faces_outward(&visual) {
        return None;
    }
    let reading = search(&visual, None);
    let turned = reading.turned_by_brackets;
    turned.then_some(Read { units, reading })
}

/// `pieces` as one line, in their order: its text its characters', made from
/// them once.
fn join(pieces: Vec<TextLine>) -> TextLine {
    let mut pieces = pieces.into_iter();
    let Some(mut line) = pieces.next() else {
        return TextLine {
            chars: Vec::new(),
            text: String::new(),
            quad: Quad {
                ll: (0.0, 0.0),
                lr: (0.0, 0.0),
                ul: (0.0, 0.0),
                ur: (0.0, 0.0),
            },
            wmode: WritingMode::Horizontal,
            rtl: false,
            size: 0.0,
        };
    };
    let Some(first) = pieces.next() else {
        return line;
    };
    line.text.clear();
    push_text(&mut line.text, &line.chars);
    for piece in core::iter::once(first).chain(pieces) {
        push_text(&mut line.text, &piece.chars);
        line.chars.extend(piece.chars);
        line.size = line.size.max(piece.size);
        line.quad = enclose(line.quad, piece.quad);
    }
    line
}

/// What [`continues_on_page`] asks of the chain a piece might continue, kept
/// as pieces join it — so that a join looks at the piece and not at the
/// chain so far. See [`rejoin_split_lines`].
#[derive(Clone)]
struct Reach {
    /// The chain's [`baseline`].
    axis: (f64, f64),
    /// Whether a character of the chain gave `axis` — until one does, it is
    /// the page's `x` axis standing in, and a piece joined later may give
    /// the chain its first, as `baseline` over the joined characters would.
    settled: bool,
    /// Where the chain's characters start and end along `axis`.
    span: (f64, f64),
    /// Where the chain's first character stands, which a piece's first is
    /// measured across the baseline from; `None` for a chain whose first
    /// piece holds no character, which nothing continues.
    first: Option<(f64, f64)>,
    /// The largest size of the chain's pieces.
    size: f64,
    /// Whether the chain's first piece is vertical, which nothing continues.
    vertical: bool,
}

impl Reach {
    /// The reach of a chain of one piece, `line`.
    fn of(line: &TextLine) -> Reach {
        let found = found_baseline(&line.chars);
        let axis = found.unwrap_or((1.0, 0.0));
        Reach {
            axis,
            settled: found.is_some(),
            span: span(&line.chars, axis),
            first: line.chars.first().map(|c| c.origin),
            size: line.size,
            vertical: line.wmode == WritingMode::Vertical,
        }
    }

    /// The reach of the chain `chain` once its last piece, whose extent along
    /// `axis` is `along`, has joined it.
    fn grow(&mut self, chain: &[TextLine], along: (f64, f64)) {
        self.span = (self.span.0.min(along.0), self.span.1.max(along.1));
        let Some(piece) = chain.last() else {
            return;
        };
        self.size = self.size.max(piece.size);
        if !self.settled {
            if let Some(axis) = found_baseline(&piece.chars) {
                // The chain's first baseline, from this piece: the extent is
                // measured again along it — once for the chain, since a
                // settled axis never moves.
                self.axis = axis;
                self.settled = true;
                self.span = chain
                    .iter()
                    .map(|line| span(&line.chars, axis))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| {
                        (lo.min(a), hi.max(b))
                    });
            }
        }
    }
}

/// Whether `next` is the rest of the chain whose [`Reach`] is `reach`, and if
/// it is, `next`'s extent along that chain's baseline. See
/// [`rejoin_split_lines`].
fn continues_on_page(reach: &Reach, next: &TextLine) -> Option<(f64, f64)> {
    if reach.vertical || next.wmode == WritingMode::Vertical {
        return None;
    }
    let (Some(first), Some(other)) = (reach.first, next.chars.first()) else {
        return None;
    };
    let axis = reach.axis;
    let theirs = baseline(&next.chars);
    if axis.0 * theirs.0 + axis.1 * theirs.1 < 0.999 {
        return None;
    }
    let slack = reach.size.max(next.size).max(1.0) * 0.5;
    // Across the baseline: the two first glyphs' origins, on the normal.
    let normal = (-axis.1, axis.0);
    let across = (other.origin.0 - first.0) * normal.0 + (other.origin.1 - first.1) * normal.1;
    if !across.is_finite() || across.abs() > slack {
        return None;
    }
    // Along it: the gap between the two extents, zero where they overlap.
    let (lo1, hi1) = reach.span;
    let (lo2, hi2) = span(&next.chars, axis);
    let gap = (lo2 - hi1).max(lo1 - hi2).max(0.0);
    let overlap = (hi1.min(hi2) - lo1.max(lo2)).max(0.0);
    let shorter = (hi1 - lo1).min(hi2 - lo2);
    (gap.is_finite() && gap <= slack && overlap <= shorter * 0.5).then_some((lo2, hi2))
}

/// Whether any of `chars` holds a right-to-left character.
fn holds_rtl(chars: &[TextChar]) -> bool {
    chars.iter().any(|c| c.text.chars().any(right_to_left))
}

/// Adds the text of `chars`, in their order, to `text`.
fn push_text(text: &mut String, chars: &[TextChar]) {
    for c in chars {
        look();
        text.push_str(&c.text);
    }
}

/// Where `chars` start and end along `axis`: the outermost of their
/// [`extent`]s.
fn span(chars: &[TextChar], axis: (f64, f64)) -> (f64, f64) {
    chars
        .iter()
        .map(|c| extent(c, axis))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| {
            (lo.min(a), hi.max(b))
        })
}

/// The smallest upright box around two quads.
fn enclose(a: Quad, b: Quad) -> Quad {
    let (ax0, ay0, ax1, ay1) = a.bounds();
    let (bx0, by0, bx1, by1) = b.bounds();
    let (x0, y0, x1, y1) = (ax0.min(bx0), ay0.min(by0), ax1.max(bx1), ay1.max(by1));
    Quad {
        ul: (x0, y1),
        ur: (x1, y1),
        ll: (x0, y0),
        lr: (x1, y0),
    }
}

/// Whether a character reads right to left, or may open a right-to-left
/// level, by its `Bidi_Class`.
///
/// The trigger for touching a line at all. `AN` is not in it: an Arabic-Indic
/// number in a left-to-right line resolves to level 2, which L2 reverses twice
/// and so leaves where it was.
fn right_to_left(c: char) -> bool {
    look();
    matches!(
        bidi_class(c),
        BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI
    )
}

/// Whether a character is a nonspacing mark, which rides on a base glyph.
fn is_mark(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| bidi_class(c) == BidiClass::NSM)
}

/// The unit vector along the line's baseline, from the first character whose
/// quad has one; the page's own `x` axis when none does.
fn baseline(chars: &[TextChar]) -> (f64, f64) {
    found_baseline(chars).unwrap_or((1.0, 0.0))
}

/// The unit vector along the baseline of the first of `chars` whose quad has
/// one, if any does.
fn found_baseline(chars: &[TextChar]) -> Option<(f64, f64)> {
    chars.iter().find_map(|c| {
        let (dx, dy) = (c.quad.lr.0 - c.quad.ll.0, c.quad.lr.1 - c.quad.ll.1);
        let length = (dx * dx + dy * dy).sqrt();
        (length.is_finite() && length > 1e-9).then(|| (dx / length, dy / length))
    })
}

/// Where a character's box starts and ends along `axis`.
fn extent(c: &TextChar, (ux, uy): (f64, f64)) -> (f64, f64) {
    look();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (x, y) in [c.quad.ll, c.quad.lr, c.quad.ul, c.quad.ur] {
        let s = x * ux + y * uy;
        lo = lo.min(s);
        hi = hi.max(s);
    }
    (lo, hi)
}

#[cfg(test)]
thread_local! {
    /// [`look`]'s count, on this thread.
    static LOOKS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    /// How many lines [`drawn_reading`] searched for a reading, on this
    /// thread ([`search`]).
    pub(crate) static SEARCHES: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// One look at one character: its extent along a baseline ([`extent`]), its
/// `Bidi_Class` ([`right_to_left`]) or its text added to a line's
/// ([`push_text`]) — the three things a join asks of every character of a
/// line, and so the three a crafted page could make it ask over and over.
/// Counted only under `cfg(test)`, where the tests below hold a page's total
/// to a small multiple of its characters, so that a lookup over the whole
/// line so far put back **fails** rather than runs slowly — which
/// `cargo test`, having no timeout, would not notice. Everywhere else it is
/// nothing.
#[inline]
fn look() {
    #[cfg(test)]
    LOOKS.with(|looks| looks.set(looks.get().saturating_add(1)));
}

/// How far `at` lies outside `(lo, hi)`; zero inside it.
fn outside(at: f64, (lo, hi): (f64, f64)) -> f64 {
    if at < lo {
        lo - at
    } else if at > hi {
        at - hi
    } else {
        0.0
    }
}

/// A line's characters as ruling 14 reads them: each base glyph with the
/// marks that ride on it — a cluster, as indices into the line's
/// characters — in the order drawn, and each cluster's text.
struct Units {
    clusters: Vec<Vec<usize>>,
    texts: Vec<String>,
}

/// A line's [`Units`] and how they are read, found once ([`rejoin_split_lines`]).
struct Read {
    units: Units,
    reading: DrawnReading,
}

/// Steps 1 and 2 of [`logical_line`] over `chars`: each mark kept with its
/// base, and the clusters in visual order.
fn drawn_units(chars: &[TextChar]) -> Units {
    let count = chars.len();
    let axis = baseline(chars);
    let extents: Vec<(f64, f64)> = chars.iter().map(|c| extent(c, axis)).collect();
    let marks: Vec<bool> = chars.iter().map(|c| is_mark(&c.text)).collect();

    // The nearest base on each side of every position, in two linear passes —
    // a line of nothing but marks is hostile input, and a search per mark
    // would make it quadratic.
    let mut before: Vec<Option<usize>> = vec![None; count];
    let mut last = None;
    for at in 0..count {
        before[at] = last;
        if !marks[at] {
            last = Some(at);
        }
    }
    let mut after: Vec<Option<usize>> = vec![None; count];
    let mut next = None;
    for at in (0..count).rev() {
        after[at] = next;
        if !marks[at] {
            next = Some(at);
        }
    }

    // Step 1: each mark's base. A mark with no base at all stands alone.
    let mut owner: Vec<usize> = (0..count).collect();
    for at in 0..count {
        if !marks[at] {
            continue;
        }
        let (lo, hi) = extents[at];
        let centre = (lo + hi) / 2.0;
        owner[at] = match (before[at], after[at]) {
            (Some(p), Some(q)) => {
                if outside(centre, extents[q]) < outside(centre, extents[p]) {
                    q
                } else {
                    p
                }
            }
            (Some(p), None) => p,
            (None, Some(q)) => q,
            (None, None) => at,
        };
    }
    let mut cluster_of: Vec<Option<usize>> = vec![None; count];
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for at in 0..count {
        if owner[at] == at {
            cluster_of[at] = Some(clusters.len());
            clusters.push(vec![at]);
        }
    }
    for at in 0..count {
        if owner[at] == at {
            continue;
        }
        if let Some(cluster) = cluster_of[owner[at]].and_then(|c| clusters.get_mut(c)) {
            cluster.push(at);
        }
    }

    // Step 2: visual order, by where each base starts. `sort_by` is stable.
    clusters.sort_by(|a, b| {
        let start = |cluster: &Vec<usize>| cluster.first().map_or(0.0, |at| extents[*at].0);
        start(a).total_cmp(&start(b))
    });
    let texts: Vec<String> = clusters
        .iter()
        .map(|cluster| {
            cluster
                .iter()
                .filter_map(|at| chars.get(*at))
                .map(|c| c.text.as_str())
                .collect()
        })
        .collect();
    Units { clusters, texts }
}

/// Puts one line into logical order, reading it as `read` says where
/// [`rejoin_split_lines`] already read it, in `declared` where its page
/// declares a direction for it, and each `/ActualText` span it holds in its
/// text (`stated`). Returns whether anything moved.
fn logical_line(
    line: &mut TextLine,
    read: Option<Read>,
    stated: &[String],
    declared: Option<BaseDirection>,
) -> bool {
    logical_line_reading(line, read, stated, declared, |visual| {
        search(visual, declared)
    })
}

/// [`logical_line`], a line it reads read by `reading_of`, which finds what
/// [`search`] finds ([`Readings`]).
fn logical_line_reading(
    line: &mut TextLine,
    read: Option<Read>,
    stated: &[String],
    declared: Option<BaseDirection>,
    reading_of: impl FnOnce(&[&str]) -> DrawnReading,
) -> bool {
    let held = line.chars.iter().any(|c| c.stated.is_some());
    if line.wmode == WritingMode::Vertical || !holds_rtl(&line.chars) {
        return held && state_in_place(line, stated);
    }
    // A line one span holds whole reads as its text, which is in logical
    // order already (14.9.4).
    let whole = line
        .chars
        .first()
        .and_then(|c| c.stated)
        .filter(|span| line.chars.iter().all(|c| c.stated == Some(*span)));
    if let Some(span) = whole {
        let text = stated_text(stated, span);
        let glyphs: Vec<&TextChar> = line.chars.iter().collect();
        line.chars = stated_chars(text, &glyphs, span);
        line.text = line.chars.iter().map(|c| c.text.as_str()).collect();
        line.rtl = declared.map_or_else(
            || first_strong_rtl(&line.text),
            |direction| direction == BaseDirection::RightToLeft,
        );
        return true;
    }
    let count = line.chars.len();

    // Steps 3 and 4: the paragraph's direction, and the order the visual line
    // is read in — found together, one search in the direction chosen. A span
    // holding part of the line is one unit of it, read as U+FFFC. A reading
    // the join found reads off the line, which a declared direction is not.
    let read = if held || declared.is_some() {
        None
    } else {
        read
    };
    let Read { units, reading } = read.unwrap_or_else(|| {
        let mut units = drawn_units(&line.chars);
        if held {
            units = stated_units(units, &line.chars);
        }
        let visual: Vec<&str> = units.texts.iter().map(String::as_str).collect();
        let reading = reading_of(&visual);
        Read { units, reading }
    });
    line.rtl = reading.direction == BaseDirection::RightToLeft;
    if held {
        let chars = std::mem::take(&mut line.chars);
        let mut out = Vec::with_capacity(count);
        for unit in &reading.order {
            let Some(cluster) = units.clusters.get(*unit) else {
                continue;
            };
            match cluster
                .first()
                .and_then(|at| chars.get(*at))
                .and_then(|c| c.stated)
            {
                Some(span) => {
                    let mut at = cluster.clone();
                    at.sort_unstable();
                    let glyphs: Vec<&TextChar> = at.iter().filter_map(|k| chars.get(*k)).collect();
                    out.extend(stated_chars(stated_text(stated, span), &glyphs, span));
                }
                None => out.extend(cluster.iter().filter_map(|at| chars.get(*at)).cloned()),
            }
        }
        line.chars = out;
        line.text = line.chars.iter().map(|c| c.text.as_str()).collect();
        return true;
    }

    let sequence: Vec<usize> = reading
        .order
        .iter()
        .filter_map(|k| units.clusters.get(*k))
        .flat_map(|cluster| cluster.iter().copied())
        .collect();
    if sequence.len() != count || sequence.iter().enumerate().all(|(i, at)| i == *at) {
        return false;
    }
    let mut taken: Vec<Option<TextChar>> = std::mem::take(&mut line.chars)
        .into_iter()
        .map(Some)
        .collect();
    line.chars = sequence
        .iter()
        .filter_map(|at| taken.get_mut(*at).and_then(Option::take))
        .collect();
    line.text = line.chars.iter().map(|c| c.text.as_str()).collect();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A character `width` wide whose box starts at `x` on the baseline.
    fn ch(text: &str, x: f64, width: f64) -> TextChar {
        TextChar {
            text: text.to_string(),
            quad: Quad {
                ll: (x, -2.0),
                lr: (x + width, -2.0),
                ul: (x, 8.0),
                ur: (x + width, 8.0),
            },
            size: 10.0,
            origin: (x, 0.0),
            mcid: None,
            stream: 0,
            font: None,
            stated: None,
        }
    }

    fn line(chars: Vec<TextChar>, rtl: bool) -> TextLine {
        let text = chars.iter().map(|c| c.text.as_str()).collect();
        TextLine {
            quad: chars[0].quad,
            chars,
            text,
            wmode: WritingMode::Horizontal,
            rtl,
            size: 10.0,
        }
    }

    /// Hebrew alef, bet, gimel.
    const ALEF: &str = "\u{5D0}";
    const BET: &str = "\u{5D1}";
    const GIMEL: &str = "\u{5D2}";
    /// Hebrew point qamats, a nonspacing mark.
    const QAMATS: &str = "\u{5B8}";

    #[test]
    fn a_visual_hebrew_line_reads_forwards() {
        // Drawn left to right as the eye sees it: gimel, bet, alef.
        let mut l = line(
            vec![ch(GIMEL, 0.0, 5.0), ch(BET, 5.0, 5.0), ch(ALEF, 10.0, 5.0)],
            true,
        );
        assert!(logical_line(&mut l, None, &[], None));
        assert_eq!(l.text, format!("{ALEF}{BET}{GIMEL}"));
    }

    #[test]
    fn a_line_drawn_in_reading_order_is_already_right() {
        // Alef drawn first at the right, the pen moving left.
        let mut l = line(
            vec![ch(ALEF, 10.0, 5.0), ch(BET, 5.0, 5.0), ch(GIMEL, 0.0, 5.0)],
            true,
        );
        assert!(
            !logical_line(&mut l, None, &[], None),
            "nothing should have moved"
        );
        assert_eq!(l.text, format!("{ALEF}{BET}{GIMEL}"));
    }

    #[test]
    fn a_mark_stays_with_its_base_whichever_side_it_was_written() {
        // Qamats sits on bet, in the middle of the box bet occupies. A
        // producer that writes the mark after its base, and one that writes
        // it before, describe the same page.
        let after = vec![
            ch(GIMEL, 0.0, 5.0),
            ch(BET, 5.0, 5.0),
            ch(QAMATS, 7.0, 0.0),
            ch(ALEF, 10.0, 5.0),
        ];
        let before = vec![
            ch(GIMEL, 0.0, 5.0),
            ch(QAMATS, 7.0, 0.0),
            ch(BET, 5.0, 5.0),
            ch(ALEF, 10.0, 5.0),
        ];
        for chars in [after, before] {
            let mut l = line(chars, true);
            logical_line(&mut l, None, &[], None);
            assert_eq!(l.text, format!("{ALEF}{BET}{QAMATS}{GIMEL}"));
        }
    }

    #[test]
    fn digits_inside_a_right_to_left_line_keep_their_own_order() {
        // Visual: "12 בא" — the number reads left to right, the letters
        // right to left, and the line is right to left.
        let mut l = line(
            vec![
                ch("1", 0.0, 5.0),
                ch("2", 5.0, 5.0),
                ch(" ", 10.0, 5.0),
                ch(BET, 15.0, 5.0),
                ch(ALEF, 20.0, 5.0),
            ],
            true,
        );
        logical_line(&mut l, None, &[], None);
        assert_eq!(l.text, format!("{ALEF}{BET} 12"));
    }

    #[test]
    fn a_left_to_right_line_is_not_touched() {
        // Out of order on purpose: a line with no right-to-left character is
        // left as collected, even where its geometry disagrees with its order.
        let mut l = line(vec![ch("b", 5.0, 5.0), ch("a", 0.0, 5.0)], false);
        assert!(!logical_line(&mut l, None, &[], None));
        assert_eq!(l.text, "ba");
    }

    /// The direction comes from the drawn line, not from the flag it arrived
    /// with: `TextDevice`'s count of letters by block took this line as right
    /// to left, and it is read as left to right and says so.
    #[test]
    fn the_direction_is_read_off_the_line_and_reported() {
        let mut l = line(
            vec![
                ch("a", 0.0, 5.0),
                ch(" ", 5.0, 5.0),
                ch(GIMEL, 10.0, 5.0),
                ch(BET, 15.0, 5.0),
                ch(ALEF, 20.0, 5.0),
                ch(" ", 25.0, 5.0),
                ch("b", 30.0, 5.0),
            ],
            true,
        );
        assert!(logical_line(&mut l, None, &[], None));
        assert_eq!(l.text, format!("a {ALEF}{BET}{GIMEL} b"));
        assert!(!l.rtl, "the line was read left to right");
    }

    /// **The comma tie-break** (ruling 14, amended 10 October 2026): a word
    /// and the comma after it, alone on a line, drawn by a left-to-right
    /// paragraph — the comma at the right — read as that paragraph, and the
    /// line says so. The same word with its comma drawn at the left, as a
    /// right-to-left paragraph draws a trailing one, is still right to left.
    #[test]
    fn a_word_and_its_comma_alone_read_as_their_paragraph_drew_them() {
        let mut quoted = line(
            vec![
                ch(GIMEL, 0.0, 5.0),
                ch(BET, 5.0, 5.0),
                ch(ALEF, 10.0, 5.0),
                ch(",", 15.0, 3.0),
            ],
            true,
        );
        assert!(logical_line(&mut quoted, None, &[], None));
        assert_eq!(quoted.text, format!("{ALEF}{BET}{GIMEL},"));
        assert!(!quoted.rtl, "the line was read left to right");

        let mut own = line(
            vec![
                ch(",", 0.0, 3.0),
                ch(GIMEL, 3.0, 5.0),
                ch(BET, 8.0, 5.0),
                ch(ALEF, 13.0, 5.0),
            ],
            false,
        );
        assert!(logical_line(&mut own, None, &[], None));
        assert_eq!(own.text, format!("{ALEF}{BET}{GIMEL},"));
        assert!(own.rtl, "the line was read right to left");
    }

    /// A percentage after Arabic digits, the `%` drawn on the number's left.
    #[test]
    fn a_percentage_reads_back_as_typed() {
        let typed = "\u{646}\u{633} 50%";
        let drawn: Vec<TextChar> = ["%", "5", "0", " ", "\u{633}", "\u{646}"]
            .iter()
            .enumerate()
            .map(|(i, t)| ch(t, i as f64 * 5.0, 5.0))
            .collect();
        let mut l = line(drawn, false);
        logical_line(&mut l, None, &[], None);
        assert_eq!(l.text, typed);
        assert!(l.rtl);
    }

    #[test]
    fn a_line_of_nothing_but_marks_is_linear_and_whole() {
        let chars: Vec<TextChar> = (0..20_000)
            .map(|i| ch(QAMATS, f64::from(i), 0.0))
            .chain(std::iter::once(ch(ALEF, 0.0, 5.0)))
            .collect();
        let mut l = line(chars, true);
        logical_line(&mut l, None, &[], None);
        assert_eq!(l.chars.len(), 20_001, "no character may be lost");
    }

    /// `a ב<span>ג</span>א b`-shaped, as an EPUB draws it: each span its own
    /// text object, in reading order, so the word's objects are drawn right to
    /// left and the device cut the line at both ends of the word.
    fn split_in_three() -> Vec<TextLine> {
        // Each line's quad encloses its characters, as the device's do.
        let whole = |chars: Vec<TextChar>, rtl: bool| {
            let mut l = line(chars, rtl);
            l.quad = l.chars.iter().fold(l.quad, |q, c| enclose(q, c.quad));
            l
        };
        vec![
            whole(vec![ch("a", 0.0, 5.0), ch(" ", 5.0, 5.0)], false),
            whole(
                vec![
                    ch(ALEF, 20.0, 5.0),
                    ch(BET, 15.0, 5.0),
                    ch(GIMEL, 10.0, 5.0),
                ],
                true,
            ),
            whole(vec![ch(" ", 25.0, 5.0), ch("b", 30.0, 5.0)], false),
        ]
    }

    #[test]
    fn a_line_the_stream_split_on_one_baseline_is_read_as_one() {
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines: split_in_three(),
            quad: ch("a", 0.0, 35.0).quad,
        });
        into_logical_order(&mut page, None);
        let lines = &page.blocks[0].lines;
        assert_eq!(lines.len(), 1, "the three pieces are one line");
        assert_eq!(lines[0].text, format!("a {ALEF}{BET}{GIMEL} b"));
        let (x0, _, x1, _) = lines[0].quad.bounds();
        assert_eq!((x0, x1), (0.0, 35.0), "the joined line encloses all three");
    }

    #[test]
    fn lines_apart_on_one_baseline_stay_apart() {
        // Two table cells: the second starts a whole em past the first.
        let mut lines = vec![
            line(vec![ch(ALEF, 0.0, 5.0)], true),
            line(vec![ch(BET, 15.0, 5.0)], true),
        ];
        assert_eq!(rejoin_split_lines(&mut lines).0, 0);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn lines_on_two_baselines_stay_apart() {
        let mut lower = ch(BET, 5.0, 5.0);
        lower.origin.1 = -12.0;
        let mut lines = vec![
            line(vec![ch(ALEF, 0.0, 5.0)], true),
            line(vec![lower], true),
        ];
        assert_eq!(rejoin_split_lines(&mut lines).0, 0);
    }

    /// **A line drawn twice is two lines, not one line of each letter
    /// twice** (review of lane 8C): `אבג` in reading order, and the same again
    /// half a point to the right in a second text object — a fake bold. Each
    /// copy reads back whole.
    #[test]
    fn a_line_drawn_over_itself_is_not_joined_to_its_copy() {
        let copy = |dx: f64| {
            line(
                vec![
                    ch(ALEF, 20.0 + dx, 5.0),
                    ch(BET, 15.0 + dx, 5.0),
                    ch(GIMEL, 10.0 + dx, 5.0),
                ],
                true,
            )
        };
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines: vec![copy(0.0), copy(0.5)],
            quad: ch(ALEF, 10.0, 15.5).quad,
        });
        into_logical_order(&mut page, None);
        let texts: Vec<&str> = page.blocks[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect();
        let word = format!("{ALEF}{BET}{GIMEL}");
        assert_eq!(texts, [word.as_str(), word.as_str()]);
    }

    /// A mark in a text object of its own, with no width, lies inside its
    /// base's extent and overlaps nothing: it is still the base's line.
    #[test]
    fn a_zero_width_mark_drawn_apart_still_joins_its_line() {
        let mut lines = vec![
            line(vec![ch(BET, 15.0, 5.0), ch(ALEF, 10.0, 5.0)], true),
            line(vec![ch(QAMATS, 17.0, 0.0)], true),
        ];
        assert_eq!(rejoin_split_lines(&mut lines).0, 1);
        assert_eq!(lines.len(), 1);
    }

    /// **A line rejoined from many pieces costs its characters, not their
    /// square** (review of lane 8C). A right-to-left word drawn as one text
    /// object per two letters, in reading order, after a long left-to-right
    /// run: every piece meets the line so far end to end and is joined to
    /// it. Each join used to rebuild the line's text from every character
    /// joined so far, measure its extent over all of them and look for a
    /// right-to-left character from its start — past the whole left-to-right
    /// run — so the page cost the square of its pieces, on every PDF's
    /// default extraction.
    ///
    /// Held by count, not by a clock: [`look`] counts every character's
    /// extent, `Bidi_Class` and text looked at, under `cfg(test)`, and a page
    /// of `C` characters is held to `8 C`, 49 152 here, where a join that
    /// looks at the line so far took 12 595 201 (its text rebuilt from every
    /// character at every join, uncounted then, besides). And the line is
    /// whole: one line, every character in it once, read in logical order.
    #[test]
    fn a_line_rejoined_from_many_pieces_costs_its_characters() {
        const RUN: usize = 2048;
        const PIECES: usize = 2048;
        let start = -5.0 * RUN as f64;
        let mut lines = vec![line(
            (0..RUN)
                .map(|i| ch("a", start + 5.0 * i as f64, 5.0))
                .collect(),
            false,
        )];
        // Piece `k` spans `[10k, 10k + 10]`: alef drawn first on the right,
        // the pen moving left to bet. The next piece starts where it ends.
        lines.extend((0..PIECES).map(|k| {
            let x = 10.0 * k as f64;
            line(vec![ch(ALEF, x + 5.0, 5.0), ch(BET, x, 5.0)], true)
        }));
        let characters = RUN + 2 * PIECES;
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines,
            quad: ch("a", start, 5.0 * characters as f64).quad,
        });

        LOOKS.with(|looks| looks.set(0));
        into_logical_order(&mut page, None);
        let looks = LOOKS.with(core::cell::Cell::get);

        let bound = 8 * characters;
        assert!(
            looks <= bound,
            "{looks} characters looked at to rejoin and read a page of {characters}; \
             a join that costs its piece looks at no more than {bound}"
        );
        let lines = &page.blocks[0].lines;
        assert_eq!(lines.len(), 1, "every piece is the one line");
        assert_eq!(lines[0].chars.len(), characters, "no character lost");
        let word = format!("{ALEF}{BET}").repeat(PIECES);
        assert_eq!(lines[0].text, format!("{word}{}", "a".repeat(RUN)));
        assert_eq!(
            lines[0].text,
            lines[0]
                .chars
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>(),
            "the text is its characters'"
        );
    }

    /// The join without [`Reach`]: everything about the chain so far, and
    /// about the group so far, read again from its characters at every
    /// piece; a chain that holds a right-to-left character joined whole where
    /// its groups are one or the bracket tie-break turns it, and group by
    /// group otherwise. Quadratic, and kept only to say what the kept reach
    /// must answer.
    fn rejoin_by_rereading(lines: &mut Vec<TextLine>) -> usize {
        fn continues(previous: &TextLine, next: &TextLine) -> bool {
            if previous.wmode == WritingMode::Vertical || next.wmode == WritingMode::Vertical {
                return false;
            }
            let (Some(first), Some(other)) = (previous.chars.first(), next.chars.first()) else {
                return false;
            };
            let axis = baseline(&previous.chars);
            let theirs = baseline(&next.chars);
            if axis.0 * theirs.0 + axis.1 * theirs.1 < 0.999 {
                return false;
            }
            let slack = previous.size.max(next.size).max(1.0) * 0.5;
            let normal = (-axis.1, axis.0);
            let across = (other.origin.0 - first.origin.0) * normal.0
                + (other.origin.1 - first.origin.1) * normal.1;
            if !across.is_finite() || across.abs() > slack {
                return false;
            }
            let (lo1, hi1) = span(&previous.chars, axis);
            let (lo2, hi2) = span(&next.chars, axis);
            let gap = (lo2 - hi1).max(lo1 - hi2).max(0.0);
            let overlap = (hi1.min(hi2) - lo1.max(lo2)).max(0.0);
            let shorter = (hi1 - lo1).min(hi2 - lo2);
            gap.is_finite() && gap <= slack && overlap <= shorter * 0.5
        }
        fn grown(whole: &mut TextLine, line: &TextLine) {
            whole.chars.extend(line.chars.iter().cloned());
            whole.text = whole.chars.iter().map(|c| c.text.as_str()).collect();
            whole.size = whole.size.max(line.size);
            whole.quad = enclose(whole.quad, line.quad);
        }
        // Each chain as its pieces, as the one line they make, and as its
        // groups, each the one line its pieces make and how many they are.
        type Groups = Vec<(TextLine, usize)>;
        let mut chains: Vec<(Vec<TextLine>, TextLine, Groups)> = Vec::new();
        for line in std::mem::take(lines) {
            match chains.last_mut() {
                Some((pieces, whole, groups)) if continues(whole, &line) => {
                    grown(whole, &line);
                    match groups.last_mut() {
                        Some((group, count))
                            if (holds_rtl(&group.chars) || holds_rtl(&line.chars))
                                && continues(group, &line) =>
                        {
                            grown(group, &line);
                            *count += 1;
                        }
                        _ => groups.push((line.clone(), 1)),
                    }
                    pieces.push(line);
                }
                _ => chains.push((vec![line.clone()], line.clone(), vec![(line, 1)])),
            }
        }
        let mut joined = 0usize;
        for (pieces, whole, groups) in chains {
            if pieces.len() < 2 || !pieces.iter().any(|piece| holds_rtl(&piece.chars)) {
                lines.extend(pieces);
            } else if groups.len() < 2 || turned_whole(&pieces).is_some() {
                joined += pieces.len() - 1;
                lines.push(whole);
            } else {
                for (group, count) in groups {
                    joined += count - 1;
                    lines.push(group);
                }
            }
        }
        joined
    }

    /// **The kept reach answers what reading the line again answered**, to
    /// the bit, on 2 000 made-up blocks of pieces that meet, miss, overlap,
    /// sit a hair or a line apart, run along baselines a degree, two and
    /// thirty off the page's, and start with characters whose quads have no
    /// baseline at all — so that a line's axis is first the page's and then
    /// its first real piece's, and a piece a degree off is measured along
    /// whichever the line has. The lines are compared by their `Debug`
    /// form, every coordinate in it exact.
    #[test]
    fn the_kept_reach_joins_exactly_what_rereading_the_line_joined() {
        // (cos, sin) of 0, 1, 2 and 30 degrees: within the join's 0.999 of
        // each other but for the last.
        const AXES: [(f64, f64); 4] = [
            (1.0, 0.0),
            (0.999_847_695_156_391_3, 0.017_452_406_437_283_51),
            (0.999_390_827_019_095_8, 0.034_899_496_702_500_97),
            (0.866_025_403_784_438_6, 0.5),
        ];
        const TEXTS: [&str; 5] = ["a", ALEF, BET, QAMATS, " "];
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = |below: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % below
        };
        let mut joins = 0usize;
        for _ in 0..2000 {
            let mut lines = Vec::new();
            let mut along = 0.0f64;
            for _ in 0..1 + next(10) {
                let (ux, uy) = AXES[next(4) as usize];
                let rise = [0.0, 0.3, 20.0][next(3) as usize];
                // Where this piece starts against where the last one ended:
                // overlapping, touching, a hair apart, or an em.
                along += [-4.0, -1.0, 0.0, 0.5, 2.0, 12.0][next(6) as usize];
                let mut chars = Vec::new();
                for _ in 0..1 + next(4) {
                    let width = [0.0, 5.0, 5.0, 3.0][next(4) as usize];
                    let at =
                        |s: f64, up: f64| (s * ux - (rise + up) * uy, s * uy + (rise + up) * ux);
                    chars.push(TextChar {
                        text: TEXTS[next(5) as usize].to_string(),
                        quad: Quad {
                            ll: at(along, -2.0),
                            lr: at(along + width, -2.0),
                            ul: at(along, 8.0),
                            ur: at(along + width, 8.0),
                        },
                        size: [10.0, 12.0][next(2) as usize],
                        origin: at(along, 0.0),
                        mcid: None,
                        stream: 0,
                        font: None,
                        stated: None,
                    });
                    along += width;
                }
                let mut piece = line(chars, false);
                piece.size = piece.chars.iter().map(|c| c.size).fold(0.0, f64::max);
                piece.quad = piece
                    .chars
                    .iter()
                    .fold(piece.quad, |q, c| enclose(q, c.quad));
                lines.push(piece);
            }
            let mut kept = lines.clone();
            let mut reread = lines;
            let joined = rejoin_split_lines(&mut kept).0;
            assert_eq!(joined, rejoin_by_rereading(&mut reread));
            assert_eq!(format!("{kept:?}"), format!("{reread:?}"));
            joins += joined;
        }
        assert!(
            joins > 1000,
            "only {joins} joins: the blocks test too little"
        );
    }

    /// **The chain, not the pair, holds a bracket the line pairs** (ROADMAP
    /// K-11). `(ab) אב`-shaped, as an EPUB draws a right-to-left line that
    /// opens with a bracketed Latin run: the `(` at the right end drawn first,
    /// then the Latin, then the Hebrew and the `)` at the left — three pieces,
    /// each meeting the next. The first two hold nothing right to left, and a
    /// join that asked that of each pair left the `(` a line of its own. Read
    /// whole, the pair faces outward and the bracket tie-break turns the line:
    /// all three are one line, read in logical order.
    #[test]
    fn a_chain_the_bracket_tie_break_turns_is_joined_whole() {
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines: vec![
                line(vec![ch("(", 25.0, 5.0)], false),
                line(vec![ch("a", 15.0, 5.0), ch("b", 20.0, 5.0)], false),
                line(
                    vec![ch(BET, 0.0, 5.0), ch(ALEF, 5.0, 5.0), ch(")", 10.0, 5.0)],
                    true,
                ),
            ],
            quad: ch("a", 0.0, 30.0).quad,
        });
        into_logical_order(&mut page, None);
        let lines = &page.blocks[0].lines;
        assert_eq!(lines.len(), 1, "the three pieces are one line");
        assert_eq!(lines[0].text, format!("(ab){ALEF}{BET}"));
        assert!(lines[0].rtl, "and it reads right to left");
    }

    /// **A chain whose groups each read right is joined group by group** —
    /// the review of the K-11 fix. The line after K-11's, `de). אב.`-shaped:
    /// the Latin at the right end, then the `)` left of it, then the Hebrew
    /// and its stops — three pieces, each meeting the next, and two groups,
    /// `de` and `). אב.`, as 9297d2d joined them. Read whole, its strong
    /// characters' ends disagree and its Latin letters are as many as its
    /// Hebrew ones, so it is read left to right and the `)` lands after the
    /// Hebrew; no bracket faces outward for the tie-break to turn it. Each
    /// group is read on its own, as before the fix.
    #[test]
    fn a_chain_the_bracket_tie_break_does_not_turn_is_joined_group_by_group() {
        let pieces = vec![
            line(vec![ch("d", 30.0, 5.0), ch("e", 35.0, 5.0)], false),
            line(vec![ch(")", 25.0, 5.0)], false),
            line(
                vec![
                    ch(".", 0.0, 5.0),
                    ch(BET, 5.0, 5.0),
                    ch(ALEF, 10.0, 5.0),
                    ch(" ", 15.0, 5.0),
                    ch(".", 20.0, 5.0),
                ],
                true,
            ),
        ];
        let whole = join(pieces.clone());
        let mut read_whole = whole.clone();
        logical_line(&mut read_whole, None, &[], None);
        assert!(
            !read_whole.text.starts_with(')'),
            "read whole, the `)` does not open the line: {:?}",
            read_whole.text
        );
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines: pieces,
            quad: whole.quad,
        });
        into_logical_order(&mut page, None);
        let lines = &page.blocks[0].lines;
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["de".to_string(), format!("). {ALEF}{BET}.")]);
        assert!(lines[1].rtl, "the Hebrew group reads right to left");
    }

    /// And a chain with no right-to-left character anywhere is left exactly
    /// as collected, however many pieces it has.
    #[test]
    fn a_chain_with_no_right_to_left_character_is_left_as_collected() {
        let mut lines = vec![
            line(vec![ch("(", 25.0, 5.0)], false),
            line(vec![ch("a", 15.0, 5.0), ch("b", 20.0, 5.0)], false),
            line(vec![ch("c", 5.0, 5.0), ch(")", 10.0, 5.0)], false),
        ];
        let collected = format!("{lines:?}");
        assert_eq!(rejoin_split_lines(&mut lines).0, 0);
        assert_eq!(format!("{lines:?}"), collected);
    }

    #[test]
    fn left_to_right_lines_are_never_joined() {
        // The same shape as the split word, with no right-to-left character:
        // the device's lines stand exactly as it collected them.
        let mut lines = vec![
            line(vec![ch("a", 0.0, 5.0)], false),
            line(vec![ch("c", 10.0, 5.0)], false),
            line(vec![ch("b", 5.0, 5.0)], false),
        ];
        assert_eq!(rejoin_split_lines(&mut lines).0, 0);
        assert_eq!(lines.len(), 3);
    }

    /// **A line of units read before is read as it was, and no other line
    /// is** (round 6, lane F:search): [`Readings`] hands back what
    /// [`drawn_reading`] found for the same units, and searches every other
    /// line, among them the same units in another order and the same text
    /// parted into other units.
    #[test]
    fn a_line_of_units_read_before_is_read_as_it_was_and_no_other_is() {
        let lines: [&[&str]; 4] = [
            &[ALEF, BET, " ", "1", "2", "."],
            &["1", "2", " ", ALEF, BET, "."],
            &["\u{5D0}\u{5D1}", " ", "12", "."],
            &[GIMEL, QAMATS, BET, " ", "(", "1", ")"],
        ];
        let mut known = Readings::default();
        SEARCHES.with(|searches| searches.set(0));
        for visual in lines.iter().chain(lines.iter()) {
            assert_eq!(
                known.reading(visual, None),
                drawn_reading(visual),
                "{visual:?}"
            );
        }
        assert_eq!(
            SEARCHES.with(core::cell::Cell::get),
            lines.len(),
            "each line of units searched once"
        );
    }

    /// **A line read in a direction its page declares is kept apart from
    /// the same units read off the line** (H-152): [`Readings`] hands back,
    /// for each direction asked, what [`search`] found in it, so a line a
    /// page declares and the same line on a page that declares nothing are
    /// each read their own way, and each searched once.
    #[test]
    fn a_line_read_in_a_declared_direction_is_kept_apart() {
        // `— אב` as a right-to-left paragraph draws it, `בא —`, which read
        // off the line keeps its dash last.
        let visual: &[&str] = &[BET, ALEF, " ", "\u{2014}"];
        let declared = logical_order(visual, BaseDirection::RightToLeft);
        assert_ne!(drawn_reading(visual).order, declared);
        let mut known = Readings::default();
        SEARCHES.with(|searches| searches.set(0));
        for _ in 0..2 {
            assert_eq!(known.reading(visual, None), drawn_reading(visual));
            let reading = known.reading(visual, Some(BaseDirection::RightToLeft));
            assert_eq!(reading.direction, BaseDirection::RightToLeft);
            assert_eq!(reading.order, declared);
        }
        assert_eq!(
            SEARCHES.with(core::cell::Cell::get),
            2,
            "each direction searched once"
        );
    }

    /// And a page put in logical order knowing the readings of lines read
    /// before, on another page and on itself, reads as one put in order
    /// afresh, line for line.
    #[test]
    fn a_page_read_knowing_lines_read_before_reads_as_one_read_afresh() {
        let word = |x: f64| {
            line(
                vec![
                    ch(GIMEL, x, 5.0),
                    ch(BET, x + 5.0, 5.0),
                    ch(ALEF, x + 10.0, 5.0),
                ],
                true,
            )
        };
        let page = |lines: Vec<TextLine>| {
            let mut page = TextPage::default();
            for (at, line) in lines.into_iter().enumerate() {
                let quad = line.quad;
                let mut line = line;
                let shift = |(x, y): (f64, f64)| (x, y - 20.0 * at as f64);
                for c in &mut line.chars {
                    c.origin = shift(c.origin);
                    c.quad = Quad {
                        ll: shift(c.quad.ll),
                        lr: shift(c.quad.lr),
                        ul: shift(c.quad.ul),
                        ur: shift(c.quad.ur),
                    };
                }
                page.blocks.push(tinker_pdf_content::TextBlock {
                    lines: vec![line],
                    quad,
                });
            }
            page
        };
        let numbers = || {
            line(
                vec![
                    ch("1", 0.0, 5.0),
                    ch("2", 5.0, 5.0),
                    ch(" ", 10.0, 5.0),
                    ch(BET, 15.0, 5.0),
                    ch(ALEF, 20.0, 5.0),
                ],
                true,
            )
        };
        let marked = || {
            line(
                vec![
                    ch(GIMEL, 0.0, 5.0),
                    ch(QAMATS, 1.0, 0.0),
                    ch(BET, 5.0, 5.0),
                    ch(ALEF, 10.0, 5.0),
                ],
                true,
            )
        };
        let pages = [
            page(vec![word(0.0), numbers(), marked()]),
            page(vec![marked(), word(100.0), numbers(), word(0.0)]),
        ];
        let mut known = Readings::default();
        for page in &pages {
            let mut afresh = page.clone();
            let mut knowing = page.clone();
            assert_eq!(
                into_logical_order_knowing(&mut knowing, &mut known, None),
                into_logical_order(&mut afresh, None)
            );
            assert_eq!(
                format!("{:?}", knowing.blocks),
                format!("{:?}", afresh.blocks)
            );
        }
    }
}
