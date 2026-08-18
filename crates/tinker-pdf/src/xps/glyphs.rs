//! 12.1's `Glyphs`: the `Indices` grammar, and where each glyph lands (gap 30,
//! milestone 7).
//!
//! # `Indices` is one grammar and four independent statements
//!
//! 12.1.3 writes a run as `GlyphMapping *(";" GlyphMapping)`, and one mapping
//! is
//!
//! ```text
//! [ "(" ClusterCodeUnitCount [ ":" ClusterGlyphCount ] ")" ]
//! [ GlyphIndex ] [ "," [ AdvanceWidth ] [ "," [ uOffset ] [ "," [ vOffset ] ] ] ]
//! ```
//!
//! Every part is optional and each says something different about the glyph:
//! **which** glyph it is, **how far** the pen moves after it, and **where** the
//! glyph sits relative to where the pen is, in two directions. Each is
//! independently droppable, so each is tested on its own — gap 30's rule from
//! milestone 5, in the one grammar in this format that has four of them.
//!
//! The forms that matter and are easy to miss:
//!
//! - **An empty `GlyphIndex`.** `Indices=",53"` is what Microsoft's own
//!   serialiser wrote for the first real file this repository ever opened: no
//!   glyph index at all, an advance of 53, and a parser that requires a digit
//!   before the comma fails on it. An absent index means *look the code unit
//!   up in the font's own `cmap`*, which is [`tinker_pdf_font::Sfnt::glyph_for_char`].
//! - **A cluster, `(m:n)`,** maps `m` UTF-16 code units of `UnicodeString` to
//!   `n` glyphs. Both counts matter and neither implies the other: `(2:1)` is a
//!   surrogate pair or a ligature and `(1:2)` is a character drawn as two
//!   marks.
//! - **Fewer mappings than code units.** The mappings run out and the rest of
//!   the string carries on one code unit to one glyph, which is what makes
//!   `,53` a statement about the *first* of eight characters.
//! - **A trailing empty mapping.** `"53;"` is two mappings by the grammar and
//!   the second is entirely defaulted. It draws a glyph when there is a code
//!   unit left for it to be, and draws nothing when there is not — because a
//!   producer that writes a separator after its last item has not asked for a
//!   ninth glyph out of eight characters.
//!
//! # The units, which are not the units anything else here uses
//!
//! `AdvanceWidth`, `uOffset` and `vOffset` are in **hundredths of the font em
//! size**, which is 12.1.3's own word for it. So an advance of `53` at
//! `FontRenderingEmSize="24"` is 12.72 XPS units, and the same number under a
//! different em size is a different distance.
//!
//! `uOffset` runs along the advance direction and `vOffset` across it, positive
//! **away from the descenders** — up the page, which is the direction of
//! *decreasing* y in a format whose origin is the top left.

use tinker_pdf_font::Sfnt;

use super::font::Font;
use super::markup::{self, Budget, Node, Trouble};

/// 12.1.3's units: an advance or an offset is a hundredth of the em size.
const HUNDREDTHS: f64 = 100.0;

/// One `GlyphMapping`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mapping {
    /// `(m:n)`, when the mapping opened with one.
    pub cluster: Option<Cluster>,
    /// The glyph index, or `None` for the empty form that asks the `cmap`.
    pub index: Option<u16>,
    /// The advance, in hundredths of the em, or `None` for the font's own.
    pub advance: Option<f64>,
    /// The offset along the advance direction, in hundredths of the em.
    pub u_offset: Option<f64>,
    /// The offset across it, in hundredths of the em, positive up the page.
    pub v_offset: Option<f64>,
}

impl Mapping {
    /// Whether every part of the mapping was left out, which is what a
    /// trailing `;` produces.
    fn empty(&self) -> bool {
        *self == Mapping::default()
    }
}

/// 12.1.3's `ClusterMapping`: how many code units, and how many glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cluster {
    /// `ClusterCodeUnitCount`, at least one.
    pub code_units: usize,
    /// `ClusterGlyphCount`, at least one; `(m)` means one.
    pub glyphs: usize,
}

/// Reads 12.1.3's `Indices`.
///
/// `None` for anything that is not the grammar — a glyph index past sixteen
/// bits, a cluster count of zero, a fifth field, a real this reader will not
/// carry into a content stream.
#[must_use]
pub fn indices(text: &str) -> Option<Vec<Mapping>> {
    let mut out = Vec::new();
    for mapping in text.split(';') {
        out.push(one_mapping(mapping)?);
    }
    Some(out)
}

fn one_mapping(text: &str) -> Option<Mapping> {
    let mut mapping = Mapping::default();
    // The cluster comes first and holds no comma, so the split is unambiguous.
    let (head, tail) = match text.split_once(',') {
        Some((head, tail)) => (head, Some(tail)),
        None => (text, None),
    };

    let head = head.trim();
    let index = match head.strip_prefix('(') {
        Some(rest) => {
            let (inside, after) = rest.split_once(')')?;
            mapping.cluster = Some(cluster(inside)?);
            after
        }
        None => head,
    };
    let index = index.trim();
    if !index.is_empty() {
        // 12.1.3: the index is the glyph's index in the physical font, which
        // is sixteen bits. A decimal integer and nothing else — no sign, no
        // exponent, no fraction, because a glyph index is not a real.
        if !index.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        mapping.index = Some(index.parse::<u16>().ok()?);
    }

    let Some(tail) = tail else {
        return Some(mapping);
    };
    let mut fields = tail.split(',');
    mapping.advance = real(fields.next().unwrap_or(""))?;
    mapping.u_offset = real(fields.next().unwrap_or(""))?;
    mapping.v_offset = real(fields.next().unwrap_or(""))?;
    // A fifth field is not 12.1.3's grammar, and taking the first four of five
    // would be reading a mapping this reader made up out of one it does not
    // understand.
    if fields.next().is_some() {
        return None;
    }
    Some(mapping)
}

fn cluster(text: &str) -> Option<Cluster> {
    let (code_units, glyphs) = match text.split_once(':') {
        Some((units, glyphs)) => (units.trim(), Some(glyphs.trim())),
        None => (text.trim(), None),
    };
    let code_units = code_units.parse::<usize>().ok()?;
    let glyphs = match glyphs {
        // `(m)` is `(m:1)`, which is 12.1.3's own default.
        None => 1,
        Some(text) => text.parse::<usize>().ok()?,
    };
    // A cluster of no code units maps nothing, and one of no glyphs draws
    // nothing while claiming to consume text. Neither is a mapping.
    (code_units >= 1 && glyphs >= 1).then_some(Cluster { code_units, glyphs })
}

/// One optional real, in the form 12.1.3 writes them — exponents included.
///
/// `Ok(None)` for an empty field, which every one of the three may be:
/// `"5,,3"` states an index, no advance and a `uOffset`. The outer `Option` is
/// the parse failing.
fn real(text: &str) -> Option<Option<f64>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(None);
    }
    // `markup::number` is the same parse every coordinate in 11 to 15 goes
    // through, so an exponent reads and an infinity does not.
    markup::number(text).map(Some)
}

/// One glyph of a run, placed in the element's own coordinate space.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The glyph index.
    pub id: u16,
    /// The code units this glyph stands for, for `/ToUnicode`.
    pub text: String,
    /// Where its origin sits along the baseline, from the run's origin.
    pub x: f64,
    /// How far off the baseline it sits, positive up the page.
    pub rise: f64,
}

/// Why a `Glyphs` run did not become a run of glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunError {
    /// `Indices` is not 12.1.3's grammar, or its cluster counts do not add up
    /// against `UnicodeString`.
    Indices,
    /// The document's glyph total is spent. The **package** is refused.
    Exhausted,
}

impl From<RunError> for Trouble {
    fn from(error: RunError) -> Trouble {
        match error {
            RunError::Indices => Trouble::Markup,
            RunError::Exhausted => Trouble::Exhausted,
        }
    }
}

/// Lays a `Glyphs` element's run out, in the element's own space.
///
/// The origin is `(0, 0)`: the caller places the run with a matrix, so the
/// numbers here are displacements along the baseline and nothing has to be
/// added twice.
///
/// # Errors
/// [`RunError::Indices`] for a run that does not describe glyphs, and
/// [`RunError::Exhausted`] when the document's glyph total is spent.
pub fn run(
    node: &Node,
    font: &Font,
    em: f64,
    budget: &mut Budget,
) -> Result<Vec<Placed>, RunError> {
    let units = unicode_string(node.attr("UnicodeString"));
    let mappings = match node.attr("Indices") {
        None => Vec::new(),
        Some(text) => {
            // **Charged before the parse, and this is the whole reason the
            // glyph total exists.** One `GlyphMapping` is eighty bytes of
            // value out of as little as one byte of markup, and the mappings
            // are separated by `;`, so the count is exactly one more than the
            // separators — known without allocating any of them. A cap checked
            // after `indices` returned would be a cap checked after the
            // allocation it is there to stop.
            //
            // A mapping is one glyph, so this is the count as well as the
            // bound: `place` is called once per mapping and once per code unit
            // the mappings did not reach. The one over-charge is a trailing
            // empty mapping that turns out to be a separator, which is
            // `tinker-pdf-zip`'s posture — a permit is what was promised.
            budget
                .glyphs(text.matches(';').count() + 1)
                .map_err(|_| RunError::Exhausted)?;
            indices(text).ok_or(RunError::Indices)?
        }
    };

    // Parsed once for the whole run rather than once per glyph: the table
    // directory is the same twenty-four entries every time.
    let sfnt = font.sfnt();
    let metrics = Metrics {
        sfnt: sfnt.as_ref(),
        units_per_em: font.units_per_em,
        em,
    };

    let mut out: Vec<Placed> = Vec::new();
    let mut pen = 0.0f64;
    let mut at = 0usize;
    // How many more glyphs the cluster now open still owes. A glyph inside a
    // cluster consumes **no** code units of its own: 12.1.3 puts the counts on
    // the cluster's first mapping and every mapping after it is one of the `n`
    // glyphs that mapping promised.
    let mut owed = 0usize;

    for (which, mapping) in mappings.iter().enumerate() {
        let last = which + 1 == mappings.len();
        let text = if mapping.cluster.is_some() || owed == 0 {
            let cluster = mapping.cluster.unwrap_or(Cluster {
                code_units: 1,
                glyphs: 1,
            });
            if at >= units.len() {
                // A trailing `;` is the separator a producer wrote after its
                // last item, not a ninth glyph out of eight characters — but
                // only when the mapping says nothing at all. One that names a
                // glyph is a glyph, string or no string.
                if last && mapping.empty() {
                    break;
                }
                if mapping.index.is_none() {
                    // No code unit to look up and no index: this mapping
                    // describes no glyph, which is a run that does not add up.
                    return Err(RunError::Indices);
                }
            } else if at + cluster.code_units > units.len() {
                // A cluster claiming more code units than the string holds is
                // 12.1.3's counts disagreeing with 12.1.1's string, and
                // reading the ones that are there would silently redraw the
                // rest of the run against the wrong characters.
                return Err(RunError::Indices);
            }
            owed = cluster.glyphs - 1;
            // The cluster's whole text goes on its **first** glyph and every
            // glyph after it carries none. `/ToUnicode` maps a code to one
            // string, so a two-glyph cluster whose glyphs both claimed the
            // text would extract it twice.
            take(&units, &mut at, cluster.code_units)
        } else {
            owed -= 1;
            String::new()
        };
        place(&mut out, &mut pen, *mapping, text, &metrics);
    }
    if owed != 0 {
        // A cluster promising more glyphs than the list holds is the same
        // disagreement from the other side.
        return Err(RunError::Indices);
    }

    // 12.1.1: the mappings may run out before the string does, and the rest is
    // one code unit to one glyph through the `cmap`. That is what makes
    // `Indices=",53"` a statement about the first of eight characters.
    if at < units.len() {
        budget
            .glyphs(units.len() - at)
            .map_err(|_| RunError::Exhausted)?;
    }
    while at < units.len() {
        let text = take(&units, &mut at, 1);
        place(&mut out, &mut pen, Mapping::default(), text, &metrics);
    }

    Ok(out)
}

/// What the font says, held together so the placement reads as one rule.
struct Metrics<'a, 'b> {
    sfnt: Option<&'a Sfnt<'b>>,
    units_per_em: f64,
    em: f64,
}

impl Metrics<'_, '_> {
    /// A glyph's own advance, in the element's units.
    ///
    /// One em where the font states none, which is the `/DW` the writer emits
    /// for a CID with no `/W` entry — so a run positioned from this number and
    /// a reader positioning from the font dictionary agree.
    fn advance(&self, id: u16) -> f64 {
        self.sfnt
            .and_then(|sfnt| sfnt.advance(id))
            .map_or(1.0, |advance| f64::from(advance) / self.units_per_em)
            * self.em
    }
}

/// Places one glyph and moves the pen past it.
///
/// Infallible, and charged by its callers rather than here: the glyph total is
/// spent **before** the mappings are materialised, and a charge at the moment
/// of placement would be a charge after the allocation.
fn place(
    out: &mut Vec<Placed>,
    pen: &mut f64,
    mapping: Mapping,
    text: String,
    metrics: &Metrics<'_, '_>,
) {
    let id = match mapping.index {
        Some(id) => id,
        // 12.1.3's empty `GlyphIndex`: the code unit decides, through the
        // font's own `cmap`. A character the font does not cover is glyph 0,
        // which is `.notdef` and is what a consumer draws for a character it
        // has no glyph for.
        None => text
            .chars()
            .next()
            .and_then(|c| metrics.sfnt.and_then(|sfnt| sfnt.glyph_for_char(c)))
            .unwrap_or(0),
    };
    let em = metrics.em;
    let advance = mapping
        .advance
        .map_or_else(|| metrics.advance(id), |stated| stated * em / HUNDREDTHS);
    let u = mapping.u_offset.unwrap_or(0.0) * em / HUNDREDTHS;
    let v = mapping.v_offset.unwrap_or(0.0) * em / HUNDREDTHS;
    out.push(Placed {
        id,
        text,
        x: *pen + u,
        rise: v,
    });
    *pen += advance;
}

/// How far the run reaches along its baseline, as `(first, last)`.
///
/// Not the ink: a glyph's outline may reach past its own advance and this
/// reader does not read outlines. It is what the run *occupies*, which is what
/// 14.3's overlap test and a `RelativeToBoundingBox` brush each need a box
/// for, and both of those are decisions about where a thing is rather than
/// about which pixels it covers.
#[must_use]
pub fn extent(placed: &[Placed], font: &Font, em: f64) -> (f64, f64) {
    let sfnt = font.sfnt();
    let metrics = Metrics {
        sfnt: sfnt.as_ref(),
        units_per_em: font.units_per_em,
        em,
    };
    let mut low = 0.0f64;
    let mut high = 0.0f64;
    for glyph in placed {
        low = low.min(glyph.x);
        high = high.max(glyph.x + metrics.advance(glyph.id));
    }
    (low, high)
}

/// 12.1.1's `UnicodeString`, as UTF-16 code units.
///
/// Code units and not characters, because 12.1.3 counts clusters in code
/// units and a surrogate pair is two of them and one character.
///
/// The `{}` prefix is XAML's escape for a value that would otherwise be a
/// markup extension: `UnicodeString="{}{StaticResource a}"` is the literal
/// text, and a reader that did not strip the two braces would draw them.
fn unicode_string(text: Option<&str>) -> Vec<u16> {
    let text = text.unwrap_or("");
    let text = text.strip_prefix("{}").unwrap_or(text);
    text.encode_utf16().collect()
}

/// `count` code units from `at`, as a string, advancing `at`.
fn take(units: &[u16], at: &mut usize, count: usize) -> String {
    let end = (*at + count).min(units.len());
    let slice = &units[*at..end];
    *at = end;
    // Lossy because a lone surrogate is a code unit `UnicodeString` may hold
    // and no character: it becomes the replacement character rather than
    // truncating the text a glyph stands for at the first one.
    String::from_utf16_lossy(slice)
}
