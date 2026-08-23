//! XPS conservation: what the markup says is on a page, and what the
//! synthesised document has (ruling 13, roadmap step 4).
//!
//! > **Every fixed page's size, every painted element and every glyph run the
//! > markup states appears exactly once in the synthesised document, in markup
//! > order, at the place ECMA-388 18.1 puts it.**
//!
//! This is what replaces `xps_mutool.rs`, which asked `mutool draw -F trace`
//! for a device trace of the **package** and a device trace of the **document**
//! and compared the two. Ruling 13 retires that. What is kept is the shape of
//! the comparison — two readings of one source, meeting in the middle — and
//! what is lost is that one of the two readings was somebody else's. That loss
//! is recorded in `docs/verification.md`, and nothing here recovers it.
//!
//! # Why the markup side does not use this repository's own reader
//!
//! [`markup_census`] walks the package with its own crude scanners, out of
//! bytes, exactly as `epub_support::conservation::spine_text` does. Calling
//! `xps::markup` would be shorter and would make the harness blind to the thing
//! it is for: **what the reader decided is the claim being checked**, and a
//! harness that asks the reader what the reader decided has checked nothing.
//! `tinker_pdf_zip::Archive` supplies the bytes and nothing above that layer is
//! borrowed — not the XML parser the reader parses with, not the PNG and JPEG
//! decoders that give an image its pixel count, and not `geometry`'s reader of
//! 11.2.3.
//!
//! # Where the arithmetic is done twice on purpose
//!
//! 18.1 makes one XPS unit 1/96 inch against PDF's 1/72, and puts the origin at
//! the top left. So this module scales by `0.75` and flips `y` itself, written
//! out from the clause — the same reason [`super::obfuscate`] writes 9.1.7.3's
//! XOR out rather than calling `font::deobfuscate`: a defect in the reader's
//! arithmetic cannot cancel itself out against a harness that shares it.
//!
//! # What is conservable, and what is not
//!
//! A rendered comparison can ask about a pixel. This cannot, and asking it to
//! would be asking it to have an opinion about anti-aliasing. What survives is
//! **counts, order, colour, geometry and placement**: three `<Path>` elements
//! are three fills in that order, `#FFDC143C` is that colour, `Data` bounds a
//! rectangle that lands where `RenderTransform` puts it, an image reaches the
//! page at the pixel count the part already had, and `UnicodeString` comes back
//! out of the `/ToUnicode` the writer wrote. Those are the properties the
//! oracle's trace comparison actually compared.

use std::collections::BTreeMap;
use std::sync::Arc;

use tinker_pdf::{CosDocument, Dict, Document, ObjRef, Object};
use tinker_pdf_content::{Token, Tokenizer};
use tinker_pdf_zip::{Archive, Limits};

use super::validated::{numbers, pages, value};

// ---- the vocabulary both sides speak ------------------------------------

/// A rectangle in PDF user space: points, `y` upward, `0,0` at the bottom left.
///
/// Both sides produce these, which is the whole design: the markup side gets
/// there through 18.1's scale and flip, the document side through the content
/// stream's own matrices, and a disagreement is a number rather than a picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    /// The rectangle that holds no points, which every union starts from.
    #[must_use]
    pub fn empty() -> Rect {
        Rect {
            x0: f64::INFINITY,
            y0: f64::INFINITY,
            x1: f64::NEG_INFINITY,
            y1: f64::NEG_INFINITY,
        }
    }

    /// A rectangle from two opposite corners, normalised.
    #[must_use]
    pub fn of(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x1 < self.x0 || self.y1 < self.y0
    }

    /// Grows to hold one more point.
    pub fn add(&mut self, x: f64, y: f64) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }

    /// This rectangle under `m`, as the bounds of its four transformed corners.
    ///
    /// Four rather than two: applying a matrix to two opposite corners loses a
    /// rotation, and a `RenderTransform` may carry one.
    #[must_use]
    pub fn under(&self, m: Matrix) -> Rect {
        if self.is_empty() {
            return *self;
        }
        let mut out = Rect::empty();
        for (x, y) in [
            (self.x0, self.y0),
            (self.x1, self.y0),
            (self.x1, self.y1),
            (self.x0, self.y1),
        ] {
            let (x, y) = m.apply(x, y);
            out.add(x, y);
        }
        out
    }

    fn agrees(&self, other: &Rect, tolerance: f64) -> bool {
        (self.x0 - other.x0).abs() <= tolerance
            && (self.y0 - other.y0).abs() <= tolerance
            && (self.x1 - other.x1).abs() <= tolerance
            && (self.y1 - other.y1).abs() <= tolerance
    }
}

/// An affine transform, `[a b c d e f]` in both vocabularies' own order.
///
/// XPS writes `RenderTransform="1,0,0,1,100,120"` and PDF writes
/// `1 0 0 1 100 120 cm`; the six numbers mean the same six things, which is why
/// one type serves both sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [f64; 6]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    #[must_use]
    pub fn apply(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    /// `inner` applied first, then `self` — the order both vocabularies
    /// compose in: an XPS child's `RenderTransform` runs before its parent's,
    /// and a `cm` runs before the CTM it joined.
    #[must_use]
    pub fn compose(self, inner: Matrix) -> Matrix {
        let [a, b, c, d, e, f] = inner.0;
        let [p, q, r, s, t, u] = self.0;
        Matrix([
            a * p + b * r,
            a * q + b * s,
            c * p + d * r,
            c * q + d * s,
            e * p + f * r + t,
            e * q + f * s + u,
        ])
    }

    /// How much this transform scales, as the larger of its two axes.
    ///
    /// Only ever for an em size, which is a length rather than a vector: a text
    /// matrix carrying 18.1's flip has a negative `d`, and a size is positive.
    #[must_use]
    pub fn scale(self) -> f64 {
        let [a, b, c, d, _, _] = self.0;
        (a * a + b * b).sqrt().max((c * c + d * d).sqrt())
    }
}

/// Which gradient, since 8.7.4.5.3 and 8.7.4.5.4 are different shadings and
/// section 15's two brushes are different brushes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gradient {
    Linear,
    Radial,
}

/// What a painted element is painted with.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// A colour, as three components in 0..1.
    Solid { rgb: [f64; 3] },
    /// A gradient, with its geometry in the element's own space and its stops
    /// in offset order.
    ///
    /// `geometry` is 8.7.4.5's `/Coords` on the document side and the brush's
    /// own points on the markup side — four numbers for an axis, six for two
    /// circles — because those are the same numbers. A reversed axis or a
    /// radius taken from the wrong stop is a page that still looks like a
    /// gradient, which is why this is compared and not merely counted.
    Gradient {
        kind: Gradient,
        geometry: Vec<f64>,
        stops: Vec<(f64, [f64; 3])>,
    },
    /// A picture, at the pixel count of the part it came from, over the
    /// rectangle it covers in user space.
    ///
    /// `copies` is how many times the picture is drawn inside one tile: one for
    /// `TileMode="None"` and `"Tile"`, two for a single flip and four for
    /// `"FlipXY"`, because a flipping tile holds its own reflections.
    Image {
        pixels: (u32, u32),
        tiled: bool,
        copies: usize,
        area: Rect,
    },
}

impl Paint {
    /// The word a divergence message uses, so a kind mismatch reads as one.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Paint::Solid { .. } => "a solid colour",
            Paint::Gradient {
                kind: Gradient::Linear,
                ..
            } => "a linear gradient",
            Paint::Gradient {
                kind: Gradient::Radial,
                ..
            } => "a radial gradient",
            Paint::Image { .. } => "an image",
        }
    }
}

/// One painted element: a `<Path>` on one side, a painting operator on the
/// other.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub paint: Paint,
    /// The bounds of what was painted, in user space.
    pub bounds: Rect,
    /// 11.6.4.4's constant alpha, which `Opacity` becomes.
    pub alpha: f64,
}

/// One `<Glyphs>` run, and what became of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// Where the run starts, in user space.
    pub origin: (f64, f64),
    /// The em size in points, which is `FontRenderingEmSize` at 18.1's scale.
    pub em: f64,
    /// The text the run stands for: `UnicodeString` on one side, the
    /// `/ToUnicode` this engine wrote on the other.
    pub text: String,
    /// How many glyphs were addressed.
    pub glyphs: usize,
    pub rgb: [f64; 3],
    /// How far each glyph moves the pen, in points.
    ///
    /// **The markup states this only where `Indices` does**, which is why the
    /// entries are optional on that side: 12.1.3 lets a cluster override the
    /// advance the face would give, and every cluster that does not takes the
    /// face's own. `Indices=",53"` — which is what the committed corpus
    /// carries — overrides the first and no other, so this is the one number
    /// in the run that comes from the markup rather than from the font, and a
    /// build that dropped it puts every glyph after the first in the wrong
    /// place while the page still reads correctly.
    ///
    /// The document side states all of them, out of the `/W` array the writer
    /// wrote and the `TJ` adjustments beside it; the comparison is only over
    /// the positions the markup states.
    pub advances: Vec<Option<f64>>,
}

/// One fixed page, and the document page made from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageCensus {
    /// The page box in points.
    pub size: (f64, f64),
    /// Painted elements, in the order they were painted.
    pub marks: Vec<Mark>,
    /// Glyph runs, in the order they were drawn.
    pub runs: Vec<Run>,
}

/// A whole package, or a whole document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Census {
    pub pages: Vec<PageCensus>,
}

impl Census {
    /// How many facts this census states: one per page for its size, one per
    /// mark and one per run.
    ///
    /// A count rather than a rate, on `corpus/ratchet.json`'s discipline, and
    /// the unit is deliberately the **element** rather than the number: a mark
    /// whose colour is right and whose placement is wrong is not half
    /// conserved.
    #[must_use]
    pub fn facts(&self) -> usize {
        self.pages
            .iter()
            .map(|page| 1 + page.marks.len() + page.runs.len())
            .sum()
    }

    /// Every gradient in the census, for the recorded figure.
    #[must_use]
    pub fn gradients(&self) -> usize {
        self.count(|paint| matches!(paint, Paint::Gradient { .. }))
    }

    /// Every image.
    #[must_use]
    pub fn images(&self) -> usize {
        self.count(|paint| matches!(paint, Paint::Image { .. }))
    }

    /// Every solid fill.
    #[must_use]
    pub fn solids(&self) -> usize {
        self.count(|paint| matches!(paint, Paint::Solid { .. }))
    }

    /// Every glyph of every run.
    #[must_use]
    pub fn glyphs(&self) -> usize {
        self.pages
            .iter()
            .flat_map(|page| page.runs.iter())
            .map(|run| run.glyphs)
            .sum()
    }

    fn count(&self, wanted: fn(&Paint) -> bool) -> usize {
        self.pages
            .iter()
            .flat_map(|page| page.marks.iter())
            .filter(|mark| wanted(&mark.paint))
            .count()
    }
}

// ---- the markup side: scanners, and nothing above `tinker-pdf-zip` -------

/// 18.1: one XPS unit is 1/96 inch and one PDF unit is 1/72.
const UNIT: f64 = 0.75;

/// Every part of a package, by its absolute name.
///
/// Absolute — with the leading solidus OPC 9.1.1.1 gives a part name — because
/// XPS 1.0 writes `ImageSource="/Resources/x.png"` and OpenXPS writes it
/// relative to the page part, and resolving both to one spelling is the only
/// way a census over the two dialects compares like with like.
fn parts(package: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let Ok(mut archive) = Archive::open(package, &Limits::DEFAULT) else {
        return out;
    };
    let names: Vec<(usize, String)> = archive
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| (index, entry.name.clone()))
        .collect();
    for (index, name) in names {
        if let Ok(bytes) = archive.read(index) {
            out.insert(format!("/{name}"), bytes.into_owned());
        }
    }
    out
}

/// A reference resolved against the part that wrote it (OPC 8.1.1.1).
fn resolve(base: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.to_owned();
    }
    let mut segments: Vec<&str> = base.trim_start_matches('/').split('/').collect::<Vec<_>>();
    segments.pop();
    for segment in target.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    format!("/{}", segments.join("/"))
}

/// Everything between `<!--` and `-->`, gone.
///
/// The XPS Object Model writes one into every page it emits, and a scanner that
/// read attributes out of a comment would census markup nobody drew.
fn without_comments(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(at) = rest.find("<!--") {
        out.push_str(&rest[..at]);
        match rest[at..].find("-->") {
            Some(end) => rest = &rest[at + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// One element's start tag, as the crude scanner sees it.
struct Tag<'a> {
    /// The name, namespace prefix and all: `Path`, `LinearGradientBrush`,
    /// `Path.Fill`, `/Canvas`.
    name: &'a str,
    /// Everything after the name, which [`attribute`] scans.
    attributes: &'a str,
    /// `<Path ... />` rather than `<Path ...>`.
    empty: bool,
    /// `</Canvas>`.
    closing: bool,
}

/// Every tag of a markup fragment, in document order.
///
/// A hand-rolled scan rather than `tinker-pdf-xml`, because that is the parser
/// the reader under test parses with, and a harness sharing it cannot see a
/// disagreement about what the markup says.
fn tags(markup: &str) -> Vec<Tag<'_>> {
    let bytes = markup.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'<' {
            at += 1;
            continue;
        }
        // A processing instruction is not an element.
        if markup[at..].starts_with("<?") {
            match markup[at..].find("?>") {
                Some(end) => at += end + 2,
                None => break,
            }
            continue;
        }
        // The scan for the closing angle honours quoting, because a `Data`
        // attribute may hold anything and a scan that stopped at the first `>`
        // would split one tag into two.
        let mut end = at + 1;
        let mut quote: Option<u8> = None;
        while end < bytes.len() {
            match (quote, bytes[end]) {
                (Some(q), b) if b == q => quote = None,
                (Some(_), _) => {}
                (None, b @ (b'"' | b'\'')) => quote = Some(b),
                (None, b'>') => break,
                (None, _) => {}
            }
            end += 1;
        }
        if end >= bytes.len() {
            break;
        }
        let body = &markup[at + 1..end];
        let empty = body.ends_with('/');
        let body = body.trim_end_matches('/');
        let closing = body.starts_with('/');
        let named = body.trim_start_matches('/');
        let split = named
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(named.len());
        out.push(Tag {
            name: &named[..split],
            attributes: &named[split..],
            empty,
            closing,
        });
        at = end + 1;
    }
    out
}

/// One attribute's value, or `None`.
///
/// **The leading space is load-bearing**, and `xps_mutool.rs` recorded why
/// before this file inherited the lesson: `Center="150,150"` is a substring of
/// `GradientOrigin`-adjacent names in the same tag family, and a needle without
/// the space matches the tail of a longer name and compares a plausible wrong
/// number. The tag's attribute run always begins with whitespace because
/// [`tags`] splits the name off at it.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attributes;
    loop {
        let at = rest.find(name)?;
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        let boundary = before.is_none_or(|c| c.is_ascii_whitespace());
        if boundary && after.starts_with('=') {
            let value = after.trim_start_matches('=').trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let end = value[1..].find(quote)?;
                return Some(&value[1..1 + end]);
            }
        }
        rest = &rest[at + name.len()..];
    }
}

/// The numbers of a comma- or space-separated attribute.
fn scalars(value: &str) -> Vec<f64> {
    value
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|piece| !piece.is_empty())
        .filter_map(|piece| piece.parse::<f64>().ok())
        .collect()
}

/// `#AARRGGBB` or `#RRGGBB`, as `(rgb, alpha)`.
///
/// Both spellings are real and both are in the corpus: WPF writes eight digits
/// and the XPS Object Model writes six for the same colour. Anything else —
/// `sc#` scRGB, a `ContextColor` naming a profile — is refused rather than
/// guessed, and shows up as a census that states no colour where the document
/// states one.
fn colour(value: &str) -> Option<([f64; 3], f64)> {
    let digits = value.strip_prefix('#')?;
    let byte = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    let (alpha, offset) = match digits.len() {
        8 => (f64::from(byte(0)?) / 255.0, 2),
        6 => (1.0, 0),
        _ => return None,
    };
    let mut rgb = [0.0; 3];
    for (index, slot) in rgb.iter_mut().enumerate() {
        *slot = f64::from(byte(offset + index * 2)?) / 255.0;
    }
    Some((rgb, alpha))
}

/// The bounds of a `Data` attribute, in the element's own space.
///
/// 11.2.3's abbreviated grammar, read for extent only: every command that
/// states points contributes them, and a command that does not move the pen
/// contributes nothing. Curves contribute their control points, which bounds
/// the curve rather than hugging it — the corpus draws only rectangles, and the
/// looser bound is honest for anything that does not.
fn data_bounds(data: &str) -> Rect {
    // One pass to items, one to points, because a grammar that lets a command
    // repeat its operand run ("M0,0L200,0 200,4 0,4Z" is three points after
    // one L) is far easier to read as a stream of numbers than as a cursor.
    #[derive(Clone, Copy)]
    enum Item {
        Command(u8),
        Number(f64),
    }

    let mut rest = data.trim();
    // 11.2.3 allows an optional fill-rule prefix, which states no geometry.
    if let Some(tail) = rest.strip_prefix("F0").or_else(|| rest.strip_prefix("F1")) {
        rest = tail.trim_start();
    }

    let bytes = rest.as_bytes();
    let mut items = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b',' || byte.is_ascii_whitespace() {
            at += 1;
        } else if byte.is_ascii_alphabetic() && !matches!(byte, b'e' | b'E') {
            items.push(Item::Command(byte));
            at += 1;
        } else {
            let from = at;
            at += 1;
            while at < bytes.len() {
                let byte = bytes[at];
                let exponent_sign =
                    matches!(byte, b'+' | b'-') && matches!(bytes[at - 1], b'e' | b'E');
                if byte.is_ascii_digit()
                    || byte == b'.'
                    || matches!(byte, b'e' | b'E')
                    || exponent_sign
                {
                    at += 1;
                } else {
                    break;
                }
            }
            match rest[from..at].parse::<f64>() {
                Ok(number) => items.push(Item::Number(number)),
                // A `Data` this scanner cannot read states no bounds rather
                // than partial ones, which shows up as a mark the document has
                // and the markup does not.
                Err(_) => return Rect::empty(),
            }
        }
    }

    let mut bounds = Rect::empty();
    let mut pen = (0.0, 0.0);
    let mut opened = (0.0, 0.0);
    let mut command = b'M';
    let mut index = 0;
    while index < items.len() {
        if let Item::Command(byte) = items[index] {
            command = byte;
            index += 1;
            if command == b'Z' || command == b'z' {
                pen = opened;
                bounds.add(pen.0, pen.1);
            }
            continue;
        }
        // How many numbers this command takes per repetition, and how many of
        // the trailing ones move the pen. Curves state their control points
        // first, and those bound the curve without hugging it.
        let (wanted, moves) = match command {
            b'H' | b'h' | b'V' | b'v' => (1, 1),
            b'C' | b'c' => (6, 6),
            b'Q' | b'q' | b'S' | b's' => (4, 4),
            b'A' | b'a' => (7, 2),
            _ => (2, 2),
        };
        let mut taken = Vec::with_capacity(wanted);
        while taken.len() < wanted {
            match items.get(index) {
                Some(Item::Number(number)) => {
                    taken.push(*number);
                    index += 1;
                }
                _ => break,
            }
        }
        if taken.len() < wanted {
            break;
        }
        let relative = command.is_ascii_lowercase();
        let from = pen;
        match command {
            b'H' | b'h' => {
                pen = if relative {
                    (from.0 + taken[0], from.1)
                } else {
                    (taken[0], from.1)
                };
                bounds.add(pen.0, pen.1);
            }
            b'V' | b'v' => {
                pen = if relative {
                    (from.0, from.1 + taken[0])
                } else {
                    (from.0, taken[0])
                };
                bounds.add(pen.0, pen.1);
            }
            b'A' | b'a' => {
                // 11.2.3 puts the endpoint last; the five before it are radii,
                // rotation and the two flags.
                let point = &taken[5..7];
                pen = if relative {
                    (from.0 + point[0], from.1 + point[1])
                } else {
                    (point[0], point[1])
                };
                bounds.add(pen.0, pen.1);
            }
            _ => {
                for (index, pair) in taken[..moves].chunks_exact(2).enumerate() {
                    let point = if relative {
                        (from.0 + pair[0], from.1 + pair[1])
                    } else {
                        (pair[0], pair[1])
                    };
                    bounds.add(point.0, point.1);
                    // Only the last pair of a curve leaves the pen there.
                    if index + 1 == moves / 2 {
                        pen = point;
                    }
                }
                if command == b'M' || command == b'm' {
                    opened = pen;
                }
            }
        }
    }
    bounds
}

/// A PNG or JPEG part's pixel dimensions, read from the part's own bytes.
///
/// Deliberately not through `tinker-pdf-filters`: "the picture reaches the page
/// at the size it already was" is only two readings if the two readings are
/// independent, and the document side gets its number from the `/Width` and
/// `/Height` the writer wrote out of that decoder.
fn pixels(part: &[u8]) -> Option<(u32, u32)> {
    // PNG: 12.2's signature, then an `IHDR` whose first eight data bytes are
    // the two dimensions, big-endian.
    if part.starts_with(b"\x89PNG\r\n\x1a\n") && part.get(12..16) == Some(b"IHDR") {
        let word = |at: usize| -> Option<u32> {
            Some(u32::from_be_bytes(part.get(at..at + 4)?.try_into().ok()?))
        };
        return Some((word(16)?, word(20)?));
    }
    // JPEG: scan the marker chain for a start-of-frame, which is where the
    // dimensions are. Every `SOFn` but the four that are not frames.
    if part.starts_with(b"\xff\xd8") {
        let mut at = 2;
        while at + 4 <= part.len() {
            if part[at] != 0xFF {
                at += 1;
                continue;
            }
            let marker = part[at + 1];
            if marker == 0xFF || marker == 0xD8 {
                at += 1;
                continue;
            }
            let length = usize::from(u16::from_be_bytes([part[at + 2], part[at + 3]]));
            let frame = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
            if frame {
                let height = u16::from_be_bytes([*part.get(at + 5)?, *part.get(at + 6)?]);
                let width = u16::from_be_bytes([*part.get(at + 7)?, *part.get(at + 8)?]);
                return Some((u32::from(width), u32::from(height)));
            }
            at += 2 + length;
        }
    }
    None
}

/// A brush the markup states, before it is placed.
#[derive(Clone, Debug)]
enum Brush {
    Solid([f64; 3], f64),
    Gradient {
        kind: Gradient,
        geometry: Vec<f64>,
        stops: Vec<(f64, [f64; 3])>,
    },
    Image {
        source: String,
        viewbox: Rect,
        viewport: Rect,
        tile: String,
    },
}

/// The stops of a gradient brush, in the order the markup wrote them.
fn stops(inner: &str) -> Vec<(f64, [f64; 3])> {
    tags(inner)
        .iter()
        .filter(|tag| tag.name.ends_with("GradientStop"))
        .filter_map(|tag| {
            let offset = attribute(tag.attributes, "Offset")?.parse::<f64>().ok()?;
            let (rgb, _) = colour(attribute(tag.attributes, "Color")?)?;
            Some((offset, rgb))
        })
        .collect()
}

/// One brush element, from its start tag and everything under it.
fn brush(name: &str, attributes: &str, inner: &str) -> Option<Brush> {
    let local = name.rsplit(':').next().unwrap_or(name);
    match local {
        "SolidColorBrush" => {
            let (rgb, alpha) = colour(attribute(attributes, "Color")?)?;
            let opacity = attribute(attributes, "Opacity")
                .and_then(|o| o.parse::<f64>().ok())
                .unwrap_or(1.0);
            Some(Brush::Solid(rgb, alpha * opacity))
        }
        "LinearGradientBrush" => {
            let start = scalars(attribute(attributes, "StartPoint")?);
            let end = scalars(attribute(attributes, "EndPoint")?);
            (start.len() == 2 && end.len() == 2).then(|| Brush::Gradient {
                kind: Gradient::Linear,
                geometry: vec![start[0], start[1], end[0], end[1]],
                stops: stops(inner),
            })
        }
        "RadialGradientBrush" => {
            let centre = scalars(attribute(attributes, "Center")?);
            let radius = attribute(attributes, "RadiusX")?.parse::<f64>().ok()?;
            // 8.7.4.5.4's inner circle is the focus, and it has no radius; XPS
            // calls it `GradientOrigin` and defaults it to the centre.
            let origin = attribute(attributes, "GradientOrigin")
                .map(scalars)
                .filter(|o| o.len() == 2)
                .unwrap_or_else(|| centre.clone());
            (centre.len() == 2).then(|| Brush::Gradient {
                kind: Gradient::Radial,
                geometry: vec![origin[0], origin[1], 0.0, centre[0], centre[1], radius],
                stops: stops(inner),
            })
        }
        "ImageBrush" => Some(Brush::Image {
            source: attribute(attributes, "ImageSource")?.to_owned(),
            viewbox: {
                let v = scalars(attribute(attributes, "Viewbox")?);
                (v.len() == 4).then(|| Rect::of(v[0], v[1], v[0] + v[2], v[1] + v[3]))?
            },
            viewport: {
                let v = scalars(attribute(attributes, "Viewport")?);
                (v.len() == 4).then(|| Rect::of(v[0], v[1], v[0] + v[2], v[1] + v[3]))?
            },
            tile: attribute(attributes, "TileMode")
                .unwrap_or("None")
                .to_owned(),
        }),
        _ => None,
    }
}

/// How many copies of the picture one tile of this mode holds.
fn copies_of(tile: &str) -> usize {
    match tile {
        "FlipX" | "FlipY" => 2,
        "FlipXY" => 4,
        _ => 1,
    }
}

/// The substring one element spans, from its start tag to its matching close.
fn span<'a>(markup: &'a str, name: &str, from: usize) -> (&'a str, usize) {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let Some(at) = markup[from..].find(&open).map(|a| a + from) else {
        return ("", markup.len());
    };
    let after = markup[at..]
        .find('>')
        .map_or(markup.len(), |end| at + end + 1);
    if markup[at..after].trim_end().ends_with("/>") {
        return ("", after);
    }
    match markup[after..].find(&close) {
        Some(end) => (&markup[after..after + end], after + end + close.len()),
        None => (&markup[after..], markup.len()),
    }
}

/// The census a package's own markup states.
///
/// One page per `<PageContent>` in spine order, whatever the reader made of it,
/// so a page the reader lost is a missing page rather than a shorter list on
/// both sides.
#[must_use]
pub fn markup_census(package: &[u8]) -> Census {
    let parts = parts(package);
    let mut out = Census::default();
    let Some(rels) = parts
        .get("/_rels/.rels")
        .map(|b| String::from_utf8_lossy(b))
    else {
        return out;
    };
    let rels = without_comments(&rels);
    let Some(sequence) = tags(&rels).iter().find_map(|tag| {
        let kind = attribute(tag.attributes, "Type")?;
        if !kind.ends_with("/fixedrepresentation") {
            return None;
        }
        // OPC 8.1.1.1 resolves a relationship target against the **source
        // part**, not against the relationships part that carries it, and the
        // source of `/_rels/.rels` is the package itself. WPF writes this
        // target absolute and the XPS Object Model writes it relative, so a
        // harness that resolved against `/_rels/` reads one dialect and finds
        // nothing at all in the other.
        Some(resolve("/", attribute(tag.attributes, "Target")?))
    }) else {
        return out;
    };

    let mut page_parts = Vec::new();
    if let Some(bytes) = parts.get(&sequence) {
        let markup = without_comments(&String::from_utf8_lossy(bytes));
        for document in tags(&markup)
            .iter()
            .filter(|tag| tag.name.ends_with("DocumentReference"))
            .filter_map(|tag| attribute(tag.attributes, "Source"))
            .map(|source| resolve(&sequence, source))
            .collect::<Vec<_>>()
        {
            let Some(bytes) = parts.get(&document) else {
                continue;
            };
            let markup = without_comments(&String::from_utf8_lossy(bytes));
            for page in tags(&markup)
                .iter()
                .filter(|tag| tag.name.ends_with("PageContent"))
                .filter_map(|tag| attribute(tag.attributes, "Source"))
            {
                page_parts.push(resolve(&document, page));
            }
        }
    }

    for name in page_parts {
        let Some(bytes) = parts.get(&name) else {
            out.pages.push(PageCensus::default());
            continue;
        };
        let markup = without_comments(&String::from_utf8_lossy(bytes));
        out.pages.push(page_census(&markup, &name, &parts));
    }
    out
}

/// One fixed page's markup, censused.
fn page_census(markup: &str, part: &str, parts: &BTreeMap<String, Vec<u8>>) -> PageCensus {
    let all = tags(markup);
    let Some(page) = all.iter().find(|tag| tag.name.ends_with("FixedPage")) else {
        return PageCensus::default();
    };
    let width = attribute(page.attributes, "Width")
        .and_then(|w| w.parse::<f64>().ok())
        .unwrap_or(0.0);
    let height = attribute(page.attributes, "Height")
        .and_then(|h| h.parse::<f64>().ok())
        .unwrap_or(0.0);
    let size = (width * UNIT, height * UNIT);

    // 18.1, applied here rather than borrowed: the unit scale, and the flip
    // that puts a top-left origin at the bottom left.
    let unit = Matrix([UNIT, 0.0, 0.0, -UNIT, 0.0, size.1]);

    // The page's own resource dictionary, keyed. `{StaticResource b0}` is one
    // indirection and every real package in the corpus uses it.
    let (resources, _) = span(markup, "FixedPage.Resources", 0);
    let mut keyed: BTreeMap<String, Brush> = BTreeMap::new();
    let mut at = 0;
    for tag in tags(resources) {
        if tag.closing {
            continue;
        }
        let Some(key) = attribute(tag.attributes, "x:Key") else {
            continue;
        };
        let (inner, next) = span(resources, tag.name, at);
        at = next;
        if let Some(brush) = brush(tag.name, tag.attributes, inner) {
            keyed.insert(key.to_owned(), brush);
        }
    }

    // The body is everything inside `<FixedPage>` that the resource dictionary
    // is not. Taken as a span rather than from the first `>`, because the XPS
    // Object Model writes a processing instruction before the element and a
    // scan that stopped at the first angle bracket would start inside it.
    let (inside, _) = span(markup, "FixedPage", 0);
    let closes = "</FixedPage.Resources>";
    let body = inside
        .find(closes)
        .map_or(inside, |at| &inside[at + closes.len()..]);

    let mut census = PageCensus {
        size,
        ..PageCensus::default()
    };
    walk(body, unit, 1.0, &keyed, part, parts, &mut census);
    census
}

/// The body, in document order, composing what a `<Canvas>` contributes.
#[allow(clippy::too_many_arguments, reason = "one frame of a markup walk")]
fn walk(
    body: &str,
    transform: Matrix,
    alpha: f64,
    keyed: &BTreeMap<String, Brush>,
    part: &str,
    parts: &BTreeMap<String, Vec<u8>>,
    out: &mut PageCensus,
) {
    let mut at = 0;
    while at < body.len() {
        let Some(next) = body[at..].find('<').map(|a| a + at) else {
            break;
        };
        let tags = tags(&body[next..]);
        let Some(tag) = tags.first() else { break };
        if tag.closing || tag.name.contains('.') {
            at = next + 1;
            continue;
        }
        let local = tag.name.rsplit(':').next().unwrap_or(tag.name);
        let element_alpha = attribute(tag.attributes, "Opacity")
            .and_then(|o| o.parse::<f64>().ok())
            .unwrap_or(1.0);
        let own = attribute(tag.attributes, "RenderTransform")
            .map(scalars)
            .filter(|numbers| numbers.len() == 6)
            .map_or(Matrix::IDENTITY, |n| {
                Matrix([n[0], n[1], n[2], n[3], n[4], n[5]])
            });
        let composed = transform.compose(own);
        let (inner, after) = span(body, tag.name, next);
        match local {
            "Canvas" => {
                walk(
                    inner,
                    composed,
                    alpha * element_alpha,
                    keyed,
                    part,
                    parts,
                    out,
                );
                at = after;
            }
            "Path" => {
                if let Some(mark) = path_mark(
                    tag.attributes,
                    inner,
                    composed,
                    alpha * element_alpha,
                    keyed,
                    part,
                    parts,
                ) {
                    out.marks.push(mark);
                }
                at = after;
            }
            "Glyphs" => {
                if let Some(run) = glyph_run(tag.attributes, composed) {
                    out.runs.push(run);
                }
                at = after;
            }
            _ => at = next + 1,
        }
    }
}

/// One `<Path>`, as the mark it paints.
fn path_mark(
    attributes: &str,
    inner: &str,
    transform: Matrix,
    alpha: f64,
    keyed: &BTreeMap<String, Brush>,
    part: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Option<Mark> {
    let data = attribute(attributes, "Data")
        .map(str::to_owned)
        .or_else(|| {
            // `<Path.Data>` with a geometry element under it states the same
            // thing the attribute does; only the abbreviated form is censused,
            // and a path stating neither is not a mark.
            let (geometry, _) = span(inner, "Path.Data", 0);
            tags(geometry)
                .iter()
                .find_map(|tag| attribute(tag.attributes, "Figures").map(str::to_owned))
        })?;
    let bounds = data_bounds(&data).under(transform);

    let brush = match attribute(attributes, "Fill") {
        Some(fill) => match fill.strip_prefix("{StaticResource ") {
            Some(key) => keyed.get(key.trim_end_matches('}').trim()).cloned(),
            None => colour(fill).map(|(rgb, a)| Brush::Solid(rgb, a)),
        },
        None => {
            let (fill, _) = span(inner, "Path.Fill", 0);
            let mut found = None;
            let mut at = 0;
            for tag in tags(fill) {
                if tag.closing || tag.name.contains('.') {
                    continue;
                }
                let (nested, next) = span(fill, tag.name, at);
                at = next;
                if let Some(brush) = brush(tag.name, tag.attributes, nested) {
                    found = Some(brush);
                    break;
                }
            }
            found
        }
    }?;

    let (paint, brush_alpha) = match brush {
        Brush::Solid(rgb, a) => (Paint::Solid { rgb }, a),
        Brush::Gradient {
            kind,
            geometry,
            stops,
        } => (
            Paint::Gradient {
                kind,
                geometry,
                stops,
            },
            1.0,
        ),
        Brush::Image {
            source,
            viewport,
            tile,
            // 13.4.1's viewbox states the picture in its own units and the
            // viewport states where it lands. It is read when the brush is
            // parsed — a brush stating no viewbox is not a brush — and the
            // rectangle compared is the viewport, because that is what the
            // pattern matrix on the other side encodes.
            viewbox: _,
        } => {
            // XPS 1.0 writes this absolute and OpenXPS writes it relative to
            // the page part, which is why both spellings resolve here rather
            // than one being assumed: the same picture under two dialects has
            // to census as the same part.
            let name = resolve(part, &source);
            let pixels = parts.get(&name).and_then(|bytes| pixels(bytes))?;
            (
                Paint::Image {
                    pixels,
                    tiled: tile != "None",
                    copies: copies_of(&tile),
                    area: viewport.under(transform),
                },
                1.0,
            )
        }
    };
    Some(Mark {
        paint,
        bounds,
        alpha: alpha * brush_alpha,
    })
}

/// One `<Glyphs>` run.
fn glyph_run(attributes: &str, transform: Matrix) -> Option<Run> {
    let x = attribute(attributes, "OriginX")?.parse::<f64>().ok()?;
    let y = attribute(attributes, "OriginY")?.parse::<f64>().ok()?;
    let em = attribute(attributes, "FontRenderingEmSize")?
        .parse::<f64>()
        .ok()?;
    let text = attribute(attributes, "UnicodeString")
        .unwrap_or_default()
        .to_owned();
    // 12.1.3: `Indices` may state fewer clusters than the string has
    // characters, and a cluster may carry several glyphs. The corpus's one run
    // is one glyph per character with the first cluster's advance overridden,
    // which is exactly the shape that makes a glyph count worth asserting.
    let indices = attribute(attributes, "Indices").unwrap_or_default();
    let clusters: Vec<&str> = indices
        .split(';')
        .filter(|cluster| !cluster.trim().is_empty())
        .collect();
    let glyphs = text.chars().count().max(clusters.len());
    // Every cluster the markup states an advance for, in points. 12.1.3 states
    // it in hundredths of the em, and a cluster may state none — an entry the
    // face answers instead, which is not a fact the markup owns.
    let mut advances: Vec<Option<f64>> = vec![None; glyphs];
    for (index, cluster) in clusters.iter().enumerate() {
        let Some(slot) = advances.get_mut(index) else {
            break;
        };
        // The optional `(m:n)` prefix states the cluster mapping and no
        // geometry, so it is stripped before the comma-separated fields.
        let fields = cluster.rsplit(')').next().unwrap_or(cluster);
        *slot = fields
            .split(',')
            .nth(1)
            .and_then(|advance| advance.trim().parse::<f64>().ok())
            .map(|advance| advance / 100.0 * em * transform.scale());
    }
    let (rgb, _) = attribute(attributes, "Fill")
        .and_then(colour)
        .unwrap_or(([0.0, 0.0, 0.0], 1.0));
    let (x, y) = transform.apply(x, y);
    Some(Run {
        origin: (x, y),
        em: em * transform.scale(),
        text,
        glyphs,
        rgb,
        advances,
    })
}

// ---- the document side: the dictionaries, as the file spells them --------

/// The census the synthesised document states.
///
/// Read through [`super::validated`]'s raw helpers rather than through the
/// typed readers, for that module's own reason: 8.7.4.5 defaults a missing
/// `/Extend` and 7.7.3.3 makes "absent" and "equal to the default" different
/// documents, so a round trip through this repository's own reader would agree
/// with itself about a shading that carried neither.
#[must_use]
pub fn document_census(document: &Document) -> Census {
    let cos = document.cos();
    let mut out = Census::default();
    for (index, (_, page)) in pages(cos).into_iter().enumerate() {
        let media = numbers(cos, &page, b"MediaBox").unwrap_or_default();
        let size = if media.len() == 4 {
            ((media[2] - media[0]).abs(), (media[3] - media[1]).abs())
        } else {
            (0.0, 0.0)
        };
        let mut census = PageCensus {
            size,
            ..PageCensus::default()
        };
        let content = page_content(cos, &page);
        let resources = value(cos, &page, b"Resources");
        let resources = resources.as_dict().cloned().unwrap_or_default();
        let mut walker = Walk::new(cos);
        walker.run(&content, &resources, Matrix::IDENTITY, 0);
        census.marks = walker.marks;
        census.runs = walker.runs;
        // The text each run stands for comes from the facade, because that is
        // the `/ToUnicode` road a caller travels: the walk has the codes, and
        // the codes alone cannot say what letter they draw.
        //
        // **Matched by where the line starts, not by position in a list.** Both
        // sides state a user-space origin, so the association is a measurement
        // rather than an assumption — a build that drew two runs in the wrong
        // order would otherwise have its text follow the swap and conserve.
        if let Some(page) = document.page(u32::try_from(index).unwrap_or(u32::MAX)) {
            let extracted = page.text();
            let lines: Vec<((f64, f64), String)> = extracted
                .lines()
                .iter()
                .filter_map(|line| {
                    let first = line.chars.first()?;
                    Some((first.origin, line.text.clone()))
                })
                .collect();
            for run in &mut census.runs {
                let nearest = lines.iter().min_by(|a, b| {
                    let distance = |origin: (f64, f64)| {
                        (origin.0 - run.origin.0).hypot(origin.1 - run.origin.1)
                    };
                    distance(a.0).total_cmp(&distance(b.0))
                });
                if let Some((origin, text)) = nearest {
                    // A run and a line that start a point apart are the same
                    // run; further than that and the census states no text and
                    // the comparison says so.
                    if (origin.0 - run.origin.0).hypot(origin.1 - run.origin.1) <= 1.0 {
                        run.text = text.clone();
                    }
                }
            }
        }
        out.pages.push(census);
    }
    out
}

/// A page's content stream, concatenated across the parts of an array.
fn page_content(cos: &CosDocument, page: &Dict) -> Vec<u8> {
    let mut out = Vec::new();
    let contents = value(cos, page, b"Contents");
    let mut push = |object: &Object| {
        if let Some(reference) = object.as_objref() {
            if let Ok(bytes) = cos.stream_decoded(reference) {
                out.extend_from_slice(&bytes);
                out.push(b'\n');
            }
        }
    };
    if let Some(array) = contents.as_array() {
        for part in array {
            push(part);
        }
    } else if let Some(reference) = page.get_ref(cos.intern(b"Contents")) {
        push(&Object::Ref(reference));
    }
    out
}

/// One entry of one resource category, resolved.
fn entry(cos: &CosDocument, resources: &Dict, category: &[u8], name: &[u8]) -> Option<Arc<Object>> {
    let group = cos.resolve_key(resources, cos.intern(category));
    let group = group.as_dict()?;
    Some(cos.resolve_key(group, cos.intern(name)))
}

/// The same entry, as the reference the resource dictionary names.
///
/// Kept beside [`entry`] rather than folded into it, because a stream is only
/// decodable through its reference and a dictionary is readable without one.
fn entry_ref(cos: &CosDocument, resources: &Dict, category: &[u8], name: &[u8]) -> Option<ObjRef> {
    let group = cos.resolve_key(resources, cos.intern(category));
    group.as_dict()?.get_ref(cos.intern(name))
}

/// A key of a dictionary, resolved.
fn key(cos: &CosDocument, dict: &Dict, name: &[u8]) -> Arc<Object> {
    cos.resolve_key(dict, cos.intern(name))
}

/// A dictionary's array of numbers, as the file spells it.
fn array(cos: &CosDocument, dict: &Dict, name: &[u8]) -> Option<Vec<f64>> {
    key(cos, dict, name)
        .as_array()?
        .iter()
        .map(Object::as_number)
        .collect()
}

/// The stops a 7.10 function states, as `(offset, colour)` in offset order.
///
/// Type 2 is one interpolation and states two; type 3 stitches several and
/// states one more than it has bounds. Read from the dictionaries rather than
/// through `parse_function`, which defaults a missing `/Domain`.
fn function_stops(cos: &CosDocument, function: &Object) -> Vec<(f64, [f64; 3])> {
    let Some(dict) = function.as_dict() else {
        return Vec::new();
    };
    let colour_of = |name: &[u8]| -> Option<[f64; 3]> {
        let values = array(cos, dict, name)?;
        (values.len() == 3).then(|| [values[0], values[1], values[2]])
    };
    match key(cos, dict, b"FunctionType").as_int() {
        Some(2) => match (colour_of(b"C0"), colour_of(b"C1")) {
            (Some(c0), Some(c1)) => vec![(0.0, c0), (1.0, c1)],
            _ => Vec::new(),
        },
        Some(3) => {
            let bounds = array(cos, dict, b"Bounds").unwrap_or_default();
            let parts = key(cos, dict, b"Functions");
            let Some(parts) = parts.as_array() else {
                return Vec::new();
            };
            let mut out: Vec<(f64, [f64; 3])> = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                let resolved = match part.as_objref() {
                    Some(reference) => cos.get(reference).unwrap_or(Arc::new(Object::Null)),
                    None => Arc::new(part.clone()),
                };
                let inner = function_stops(cos, &resolved);
                let (from, to) = (
                    if index == 0 {
                        0.0
                    } else {
                        bounds.get(index - 1).copied().unwrap_or(0.0)
                    },
                    bounds.get(index).copied().unwrap_or(1.0),
                );
                for (offset, colour) in inner {
                    let placed = from + offset * (to - from);
                    // A stitching boundary is one stop, not two: the sub
                    // function before it ends where the next begins, and a
                    // census that recorded both would report a gradient with
                    // more stops than the markup wrote.
                    if out
                        .last()
                        .is_none_or(|(last, _)| (last - placed).abs() > 1e-9)
                    {
                        out.push((placed, colour));
                    }
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// A shading dictionary, as a paint.
fn shading_paint(cos: &CosDocument, shading: &Object) -> Option<Paint> {
    let dict = shading.as_dict()?;
    let kind = match key(cos, dict, b"ShadingType").as_int()? {
        2 => Gradient::Linear,
        3 => Gradient::Radial,
        _ => return None,
    };
    let geometry = array(cos, dict, b"Coords")?;
    let stops = function_stops(cos, &key(cos, dict, b"Function"));
    Some(Paint::Gradient {
        kind,
        geometry,
        stops,
    })
}

/// A tiling or shading pattern, as the paint it stands for.
///
/// `PatternType 1` is 8.7.3's tiling pattern, which is what an `ImageBrush`
/// becomes: the picture's own rectangle is the pattern matrix composed with the
/// `cm` in front of each `Do`, so the number compared is where the picture
/// lands rather than what its matrix says — the two differ by 18.1's flip, and
/// asserting the six numbers were equal would be asserting the flip is absent.
fn pattern_paint(cos: &CosDocument, pattern: &Object, reference: Option<ObjRef>) -> Option<Paint> {
    let dict = match pattern {
        Object::Stream(stream) => &stream.dict,
        other => other.as_dict()?,
    };
    match key(cos, dict, b"PatternType").as_int() {
        Some(2) => shading_paint(cos, &key(cos, dict, b"Shading")),
        Some(1) => {
            let matrix =
                array(cos, dict, b"Matrix").unwrap_or_else(|| vec![1., 0., 0., 1., 0., 0.]);
            let matrix = (matrix.len() == 6).then(|| {
                Matrix([
                    matrix[0], matrix[1], matrix[2], matrix[3], matrix[4], matrix[5],
                ])
            })?;
            let step = key(cos, dict, b"XStep").as_number().unwrap_or(0.0);
            let bbox = array(cos, dict, b"BBox").filter(|bbox| bbox.len() == 4)?;
            // 8.7.3.1: a step is how far apart the cells are, so a step wider
            // than the cell is the writer saying this brush does not repeat.
            // The one it writes for `TileMode="None"` is enormous rather than
            // absent, because a tiling pattern has no way to say never.
            let tiled = step <= (bbox[2] - bbox[0]).abs() * 2.0;

            // Decoded rather than raw, because a document saved with
            // `compress` carries the same pattern behind a `/FlateDecode` and
            // a census that read the raw bytes would find no picture in it.
            let content = reference
                .and_then(|reference| cos.stream_decoded(reference).ok())
                .unwrap_or_default();
            let resources = key(cos, dict, b"Resources");
            let resources = resources.as_dict()?.clone();
            let mut walker = Walk::new(cos);
            walker.run(&content, &resources, matrix, 0);
            let first = walker.marks.first()?;
            let Paint::Image { pixels, area, .. } = first.paint.clone() else {
                return None;
            };
            Some(Paint::Image {
                pixels,
                tiled,
                copies: walker.marks.len(),
                area,
            })
        }
        _ => None,
    }
}

/// One frame of the graphics state this walk models.
#[derive(Clone, Debug)]
struct Frame {
    ctm: Matrix,
    rgb: [f64; 3],
    alpha: f64,
    pattern: Option<Vec<u8>>,
}

/// The content stream, walked for what it paints.
///
/// Not the interpreter: `tinker_pdf_content::interpret` needs a `FontSource`,
/// which is the facade's private resource layer, and a walk that went through
/// it would be asking the renderer's own resolution what the file says. This
/// tokenizes and composes the matrices itself, which is the same second reading
/// the markup side is.
struct Walk<'a> {
    cos: &'a CosDocument,
    marks: Vec<Mark>,
    runs: Vec<Run>,
    frame: Frame,
    saved: Vec<Frame>,
    /// The path under construction, in user space.
    path: Rect,
    at: (f64, f64),
    start: (f64, f64),
    /// What `W`/`W*` armed and `n` committed.
    pending_clip: bool,
    clip: Option<Rect>,
    /// The text object's matrix and font.
    text: Matrix,
    size: f64,
    wide: bool,
    glyphs: usize,
    origin: Option<(f64, f64)>,
    /// The widths the font in force states, in thousandths of the em: the
    /// `/W` array by CID, and `/DW` for every code it does not name.
    widths: Widths,
    /// How far each glyph shown so far moves the pen, in thousandths, with
    /// each `TJ` adjustment already taken off the glyph it followed.
    pen: Vec<f64>,
}

/// 9.7.4.3's two width statements, read out of the descendant font.
#[derive(Clone, Debug, Default)]
struct Widths {
    stated: BTreeMap<u32, f64>,
    default: f64,
}

impl Widths {
    /// The width of one code, in thousandths of the em.
    fn of(&self, code: u32) -> f64 {
        self.stated.get(&code).copied().unwrap_or(self.default)
    }
}

impl<'a> Walk<'a> {
    fn new(cos: &'a CosDocument) -> Walk<'a> {
        Walk {
            cos,
            marks: Vec::new(),
            runs: Vec::new(),
            frame: Frame {
                ctm: Matrix::IDENTITY,
                rgb: [0.0, 0.0, 0.0],
                alpha: 1.0,
                pattern: None,
            },
            saved: Vec::new(),
            path: Rect::empty(),
            at: (0.0, 0.0),
            start: (0.0, 0.0),
            pending_clip: false,
            clip: None,
            text: Matrix::IDENTITY,
            size: 0.0,
            wide: false,
            glyphs: 0,
            origin: None,
            widths: Widths::default(),
            pen: Vec::new(),
        }
    }
}

/// 9.4.3's two show-then-move operators, spelled by code point because their
/// own glyphs are quotation marks and a source line carrying them reads worse
/// than a named constant does.
const SHOW_NEXT_LINE: &[u8] = &[0x27];
const SHOW_SPACED: &[u8] = &[0x22];

impl Walk<'_> {
    /// Runs one stream under `base`, which is the CTM the stream inherits.
    fn run(&mut self, content: &[u8], resources: &Dict, base: Matrix, depth: u32) {
        if depth > 8 {
            return;
        }
        self.frame.ctm = base;
        let mut tokens = Tokenizer::new(content);
        let mut operands: Vec<Token> = Vec::new();
        while let Some(token) = tokens.next_token() {
            let Token::Operator(operator) = &token else {
                operands.push(token);
                continue;
            };
            let operator = operator.clone();
            self.operator(&operator, &operands, resources, depth);
            operands.clear();
        }
    }

    fn numbers(operands: &[Token]) -> Vec<f64> {
        operands
            .iter()
            .filter_map(|token| match token {
                Token::Number(number) => Some(*number),
                _ => None,
            })
            .collect()
    }

    fn moved(&mut self, x: f64, y: f64) {
        let (x, y) = self.frame.ctm.apply(x, y);
        self.path.add(x, y);
        self.at = (x, y);
    }

    #[allow(clippy::too_many_lines, reason = "one match over the operator set")]
    fn operator(&mut self, operator: &[u8], operands: &[Token], resources: &Dict, depth: u32) {
        let numbers = Self::numbers(operands);
        if operator == SHOW_NEXT_LINE || operator == SHOW_SPACED {
            self.show(operands);
            return;
        }
        match operator {
            b"q" => self.saved.push(self.frame.clone()),
            b"Q" => {
                if let Some(frame) = self.saved.pop() {
                    self.frame = frame;
                }
            }
            b"cm" if numbers.len() == 6 => {
                let m = Matrix([
                    numbers[0], numbers[1], numbers[2], numbers[3], numbers[4], numbers[5],
                ]);
                self.frame.ctm = self.frame.ctm.compose(m);
            }
            b"gs" => {
                if let Some(Token::Name(name)) = operands.last() {
                    if let Some(alpha) =
                        entry(self.cos, resources, b"ExtGState", name).and_then(|state| {
                            state
                                .as_dict()
                                .and_then(|dict| key(self.cos, dict, b"ca").as_number())
                        })
                    {
                        self.frame.alpha = alpha;
                    }
                }
            }
            b"rg" if numbers.len() == 3 => {
                self.frame.rgb = [numbers[0], numbers[1], numbers[2]];
                self.frame.pattern = None;
            }
            b"g" if numbers.len() == 1 => {
                self.frame.rgb = [numbers[0]; 3];
                self.frame.pattern = None;
            }
            b"k" if numbers.len() == 4 => {
                // The conversion 10.4.2.4 states, so that a census over a CMYK
                // page compares a colour rather than nothing.
                let (c, m, y, k) = (numbers[0], numbers[1], numbers[2], numbers[3]);
                self.frame.rgb = [
                    (1.0 - c) * (1.0 - k),
                    (1.0 - m) * (1.0 - k),
                    (1.0 - y) * (1.0 - k),
                ];
                self.frame.pattern = None;
            }
            b"cs" => self.frame.pattern = None,
            b"scn" | b"sc" => match operands.last() {
                Some(Token::Name(name)) => self.frame.pattern = Some(name.clone()),
                _ if numbers.len() == 3 => {
                    self.frame.rgb = [numbers[0], numbers[1], numbers[2]];
                }
                _ => {}
            },
            b"m" if numbers.len() >= 2 => {
                self.moved(numbers[0], numbers[1]);
                self.start = self.at;
            }
            b"l" if numbers.len() >= 2 => self.moved(numbers[0], numbers[1]),
            b"c" | b"v" | b"y" if numbers.len() >= 4 => {
                for pair in numbers.chunks_exact(2) {
                    self.moved(pair[0], pair[1]);
                }
            }
            b"re" if numbers.len() >= 4 => {
                let (x, y, w, h) = (numbers[0], numbers[1], numbers[2], numbers[3]);
                for (px, py) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)] {
                    let (px, py) = self.frame.ctm.apply(px, py);
                    self.path.add(px, py);
                }
                self.at = self.frame.ctm.apply(x, y);
                self.start = self.at;
            }
            b"h" => self.at = self.start,
            b"W" | b"W*" => self.pending_clip = true,
            b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"S" | b"s" | b"n" => {
                if operator != b"n" {
                    if let Some(paint) = self.paint(resources) {
                        self.marks.push(Mark {
                            paint,
                            bounds: self.path,
                            alpha: self.frame.alpha,
                        });
                    }
                }
                if self.pending_clip {
                    self.clip = Some(self.path);
                    self.pending_clip = false;
                }
                self.path = Rect::empty();
            }
            b"sh" => {
                if let Some(Token::Name(name)) = operands.last() {
                    if let Some(paint) = entry(self.cos, resources, b"Shading", name)
                        .and_then(|shading| shading_paint(self.cos, &shading))
                    {
                        self.marks.push(Mark {
                            paint,
                            // 8.7.4.2: `sh` paints the current clip, and the
                            // writer states one for exactly that reason.
                            bounds: self.clip.unwrap_or(self.path),
                            alpha: self.frame.alpha,
                        });
                    }
                }
            }
            b"Do" => {
                if let Some(Token::Name(name)) = operands.last() {
                    let name = name.clone();
                    self.xobject(&name, resources, depth);
                }
            }
            b"BT" => {
                self.text = Matrix::IDENTITY;
                self.glyphs = 0;
                self.origin = None;
                self.pen.clear();
            }
            b"Tf" if !numbers.is_empty() => {
                self.size = numbers[numbers.len() - 1];
                if let Some(Token::Name(name)) = operands.first() {
                    self.wide = self.is_two_byte(resources, name);
                    self.widths = self.widths_of(resources, name);
                }
            }
            b"Tm" if numbers.len() == 6 => {
                self.text = Matrix([
                    numbers[0], numbers[1], numbers[2], numbers[3], numbers[4], numbers[5],
                ]);
            }
            b"Td" | b"TD" if numbers.len() == 2 => {
                self.text = self
                    .text
                    .compose(Matrix([1.0, 0.0, 0.0, 1.0, numbers[0], numbers[1]]));
            }
            b"Tj" | b"TJ" => self.show(operands),
            b"ET" => {
                if self.glyphs > 0 {
                    if let Some(origin) = self.origin {
                        let placed = self.frame.ctm.compose(self.text);
                        let em = self.size * placed.scale();
                        self.runs.push(Run {
                            origin,
                            em,
                            text: String::new(),
                            glyphs: self.glyphs,
                            rgb: self.frame.rgb,
                            // 9.4.4: a displacement is the width in
                            // thousandths, less the `TJ` adjustment, times the
                            // size — and then into points by the matrix the
                            // run is set under.
                            advances: self
                                .pen
                                .iter()
                                .map(|width| Some(width / 1_000.0 * em))
                                .collect(),
                        });
                    }
                }
                self.glyphs = 0;
                self.origin = None;
                self.pen.clear();
            }
            _ => {}
        }
    }

    /// One show operator: where the run starts, and how many glyphs it named.
    ///
    /// The codes are the string bytes, which under 9.7.5 with `/Identity-H`
    /// are the glyph indices themselves — the assertion `xps_mutool.rs` called
    /// its sharpest, since a run drawn through the face own `cmap` instead
    /// would put the right-looking letters at different numbers.
    fn show(&mut self, operands: &[Token]) {
        let placed = self.frame.ctm.compose(self.text);
        if self.origin.is_none() {
            self.origin = Some(placed.apply(0.0, 0.0));
        }
        let per = usize::from(self.wide) + 1;
        for token in operands {
            match token {
                Token::String(bytes) => {
                    for code in bytes.chunks(per) {
                        let code = code
                            .iter()
                            .fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
                        self.pen.push(self.widths.of(code));
                        self.glyphs += 1;
                    }
                }
                // 9.4.3: a number in a `TJ` array moves the pen back by that
                // many thousandths, which belongs to the glyph before it. A
                // number before any glyph moves the run's start instead, and
                // this walk leaves that to the origin the matrix states.
                Token::Number(adjustment) => {
                    if let Some(last) = self.pen.last_mut() {
                        *last -= adjustment;
                    }
                }
                _ => {}
            }
        }
    }

    /// The width statements of the named font.
    ///
    /// Read out of the descendant font's own `/W` and `/DW` rather than
    /// through a typed font reader, for `super::validated`'s reason: 9.7.4.3
    /// defaults `/DW` to 1000, and a reader that supplied that default would
    /// agree with a writer that stated nothing.
    fn widths_of(&self, resources: &Dict, name: &[u8]) -> Widths {
        let mut out = Widths {
            stated: BTreeMap::new(),
            default: 1_000.0,
        };
        let Some(font) = entry(self.cos, resources, b"Font", name) else {
            return out;
        };
        let Some(font) = font.as_dict() else {
            return out;
        };
        let descendants = key(self.cos, font, b"DescendantFonts");
        let descendant = match descendants.as_array().and_then(<[Object]>::first) {
            Some(Object::Ref(reference)) => self
                .cos
                .get(*reference)
                .unwrap_or_else(|_| Arc::new(Object::Null)),
            Some(other) => Arc::new(other.clone()),
            None => return out,
        };
        let Some(descendant) = descendant.as_dict() else {
            return out;
        };
        if let Some(default) = key(self.cos, descendant, b"DW").as_number() {
            out.default = default;
        }
        // 9.7.4.3 Table 121: `c [w1 ... wn]` names consecutive codes from `c`,
        // and `c_first c_last w` gives one width to a range. Both spellings,
        // because a writer may use either and a reader of one is a reader of
        // half the array.
        let array = key(self.cos, descendant, b"W");
        let Some(array) = array.as_array() else {
            return out;
        };
        let mut at = 0;
        while at < array.len() {
            let Some(first) = array[at].as_number() else {
                break;
            };
            match array.get(at + 1) {
                Some(Object::Array(widths)) => {
                    for (offset, width) in widths.iter().enumerate() {
                        if let (Ok(code), Some(width)) = (
                            u32::try_from(first as i64 + offset as i64),
                            width.as_number(),
                        ) {
                            out.stated.insert(code, width);
                        }
                    }
                    at += 2;
                }
                Some(last) => {
                    let (Some(last), Some(width)) = (
                        last.as_number(),
                        array.get(at + 2).and_then(Object::as_number),
                    ) else {
                        break;
                    };
                    let (from, to) = (first as i64, last as i64);
                    for code in from..=to.min(from + 65_536) {
                        if let Ok(code) = u32::try_from(code) {
                            out.stated.insert(code, width);
                        }
                    }
                    at += 3;
                }
                None => break,
            }
        }
        out
    }

    /// Whether the named font addresses glyphs two bytes at a time, which is
    /// 9.7.5 with `/Identity-H` and is how every XPS glyph run reaches a PDF.
    fn is_two_byte(&self, resources: &Dict, name: &[u8]) -> bool {
        entry(self.cos, resources, b"Font", name)
            .and_then(|font| {
                let dict = font.as_dict()?;
                key(self.cos, dict, b"Subtype")
                    .as_name()
                    .and_then(|n| self.cos.name_bytes(n))
            })
            .as_deref()
            == Some(&b"Type0"[..])
    }

    /// What the current colour operators say this fill is painted with.
    fn paint(&self, resources: &Dict) -> Option<Paint> {
        match &self.frame.pattern {
            Some(name) => {
                let pattern = entry(self.cos, resources, b"Pattern", name)?;
                let reference = entry_ref(self.cos, resources, b"Pattern", name);
                pattern_paint(self.cos, &pattern, reference)
            }
            None => Some(Paint::Solid {
                rgb: self.frame.rgb,
            }),
        }
    }

    /// `Do`: an image is a mark, and a form is more stream.
    fn xobject(&mut self, name: &[u8], resources: &Dict, depth: u32) {
        let Some(object) = entry(self.cos, resources, b"XObject", name) else {
            return;
        };
        let Object::Stream(stream) = object.as_ref() else {
            return;
        };
        let dict = &stream.dict;
        let subtype = key(self.cos, dict, b"Subtype")
            .as_name()
            .and_then(|n| self.cos.name_bytes(n));
        match subtype.as_deref() {
            Some(b"Image") => {
                let width = key(self.cos, dict, b"Width").as_int().unwrap_or(0);
                let height = key(self.cos, dict, b"Height").as_int().unwrap_or(0);
                // 8.9.5.2: an image is drawn into the unit square of the CTM,
                // so where it lands is that square under it.
                let area = Rect::of(0.0, 0.0, 1.0, 1.0).under(self.frame.ctm);
                self.marks.push(Mark {
                    paint: Paint::Image {
                        pixels: (
                            u32::try_from(width).unwrap_or(0),
                            u32::try_from(height).unwrap_or(0),
                        ),
                        tiled: false,
                        copies: 1,
                        area,
                    },
                    bounds: area,
                    alpha: self.frame.alpha,
                });
            }
            Some(b"Form") => {
                let matrix = array(self.cos, dict, b"Matrix")
                    .filter(|m| m.len() == 6)
                    .map_or(Matrix::IDENTITY, |m| {
                        Matrix([m[0], m[1], m[2], m[3], m[4], m[5]])
                    });
                let own = key(self.cos, dict, b"Resources");
                let inherited = own.as_dict().cloned().unwrap_or_else(|| resources.clone());
                let base = self.frame.ctm.compose(matrix);
                let content = entry_ref(self.cos, resources, b"XObject", name)
                    .and_then(|reference| self.cos.stream_decoded(reference).ok())
                    .unwrap_or_default();
                let mut inner = Walk::new(self.cos);
                inner.frame = Frame {
                    ctm: base,
                    ..self.frame.clone()
                };
                inner.run(&content, &inherited, base, depth + 1);
                self.marks.append(&mut inner.marks);
                self.runs.append(&mut inner.runs);
            }
            _ => {}
        }
    }
}

// ---- the comparator -----------------------------------------------------

/// A quarter of a point, over pages six hundred points wide.
///
/// The size `xps_mutool.rs` measured and stated its reason for: an image
/// viewbox is stated in the picture own units, and the two sides reach the
/// device rectangle by different routes. Anything larger than this is a
/// placement defect rather than a rounding difference — the 4 336-point one
/// gap 30 found is four orders of magnitude above it.
const PLACEMENT: f64 = 0.25;

/// One part in ten thousand, on a colour.
///
/// The writer states four decimal places and the markup states a byte:
/// `#FFDC143C` is `0.8627` in a content stream and `0.862745` in a function
/// dictionary, and those are the same colour.
const COLOUR: f64 = 1e-4;

/// Five thousandths, on a gradient geometry and on a page box.
///
/// Both sides state these exactly; the tolerance exists so that a future
/// writer rounding a coordinate differently is not a failure.
const GEOMETRY: f64 = 5e-3;

/// A hundredth of a point, on an em size.
const EM: f64 = 1e-2;

/// One thing the document does not conserve about the markup.
///
/// Every variant carries both readings and where they disagreed, because a
/// message naming only one of the two makes a disagreement about *order* read
/// as a disagreement about a value — `xps_mutool.rs` learned that and this
/// inherits it.
#[derive(Clone, Debug, PartialEq)]
pub enum Divergence {
    PageCount {
        markup: usize,
        document: usize,
    },
    PageSize {
        page: usize,
        markup: (f64, f64),
        document: (f64, f64),
    },
    MarkCount {
        page: usize,
        markup: usize,
        document: usize,
    },
    PaintKind {
        page: usize,
        mark: usize,
        markup: &'static str,
        document: &'static str,
    },
    Colour {
        page: usize,
        mark: usize,
        markup: [f64; 3],
        document: [f64; 3],
    },
    GradientGeometry {
        page: usize,
        mark: usize,
        markup: Vec<f64>,
        document: Vec<f64>,
    },
    Stops {
        page: usize,
        mark: usize,
        markup: Vec<(f64, [f64; 3])>,
        document: Vec<(f64, [f64; 3])>,
    },
    Pixels {
        page: usize,
        mark: usize,
        markup: (u32, u32),
        document: (u32, u32),
    },
    Copies {
        page: usize,
        mark: usize,
        markup: usize,
        document: usize,
    },
    Placement {
        page: usize,
        mark: usize,
        markup: Rect,
        document: Rect,
    },
    Alpha {
        page: usize,
        mark: usize,
        markup: f64,
        document: f64,
    },
    RunCount {
        page: usize,
        markup: usize,
        document: usize,
    },
    GlyphCount {
        page: usize,
        run: usize,
        markup: usize,
        document: usize,
    },
    Text {
        page: usize,
        run: usize,
        markup: String,
        document: String,
    },
    Origin {
        page: usize,
        run: usize,
        markup: (f64, f64),
        document: (f64, f64),
    },
    EmSize {
        page: usize,
        run: usize,
        markup: f64,
        document: f64,
    },
    Advance {
        page: usize,
        run: usize,
        glyph: usize,
        markup: f64,
        document: f64,
    },
}

/// What a comparison found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verdict {
    /// Facts the markup states: one per page, one per mark, one per run.
    pub facts: usize,
    /// Facts the document conserved, whole.
    pub conserved: usize,
    /// Every disagreement, in page then element order.
    pub divergences: Vec<Divergence>,
}

impl Verdict {
    #[must_use]
    pub fn holds(&self) -> bool {
        self.divergences.is_empty()
    }

    /// The recorded pair: conserved of stated.
    #[must_use]
    pub fn figure(&self) -> (usize, usize) {
        (self.conserved, self.facts)
    }
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

fn rgb_near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(a, b)| near(*a, *b, COLOUR))
}

/// The characters a text comparison keeps.
///
/// Whitespace is dropped for `epub_support::conservation` own reason, halved:
/// a `<Glyphs>` run states its own spacing and an extractor decides where a
/// line ends, so the largest stream that can survive both is the non-space
/// one. Every other character is compared exactly, in order.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether the document conserves what the markup states.
///
/// Element by element, in order, because order is one of the claims: a page
/// whose three fills are painted back to front carries every colour it should
/// and is a different page.
#[must_use]
pub fn conserve(markup: &Census, document: &Census) -> Verdict {
    let mut out = Verdict {
        facts: markup.facts(),
        ..Verdict::default()
    };
    if markup.pages.len() != document.pages.len() {
        out.divergences.push(Divergence::PageCount {
            markup: markup.pages.len(),
            document: document.pages.len(),
        });
    }

    for (page, source) in markup.pages.iter().enumerate() {
        let Some(made) = document.pages.get(page) else {
            continue;
        };
        if near(source.size.0, made.size.0, GEOMETRY) && near(source.size.1, made.size.1, GEOMETRY)
        {
            out.conserved += 1;
        } else {
            out.divergences.push(Divergence::PageSize {
                page,
                markup: source.size,
                document: made.size,
            });
        }

        if source.marks.len() != made.marks.len() {
            out.divergences.push(Divergence::MarkCount {
                page,
                markup: source.marks.len(),
                document: made.marks.len(),
            });
        }
        for (mark, stated) in source.marks.iter().enumerate() {
            let Some(painted) = made.marks.get(mark) else {
                continue;
            };
            let before = out.divergences.len();
            compare_mark(page, mark, stated, painted, &mut out.divergences);
            if out.divergences.len() == before {
                out.conserved += 1;
            }
        }

        if source.runs.len() != made.runs.len() {
            out.divergences.push(Divergence::RunCount {
                page,
                markup: source.runs.len(),
                document: made.runs.len(),
            });
        }
        for (run, stated) in source.runs.iter().enumerate() {
            let Some(drawn) = made.runs.get(run) else {
                continue;
            };
            let before = out.divergences.len();
            compare_run(page, run, stated, drawn, &mut out.divergences);
            if out.divergences.len() == before {
                out.conserved += 1;
            }
        }
    }
    out
}

fn compare_mark(
    page: usize,
    mark: usize,
    stated: &Mark,
    painted: &Mark,
    out: &mut Vec<Divergence>,
) {
    match (&stated.paint, &painted.paint) {
        (Paint::Solid { rgb: wanted }, Paint::Solid { rgb: got }) => {
            if !rgb_near(*wanted, *got) {
                out.push(Divergence::Colour {
                    page,
                    mark,
                    markup: *wanted,
                    document: *got,
                });
            }
        }
        (
            Paint::Gradient {
                kind: wanted_kind,
                geometry: wanted_geometry,
                stops: wanted_stops,
            },
            Paint::Gradient {
                kind: got_kind,
                geometry: got_geometry,
                stops: got_stops,
            },
        ) => {
            if wanted_kind != got_kind {
                out.push(Divergence::PaintKind {
                    page,
                    mark,
                    markup: stated.paint.kind(),
                    document: painted.paint.kind(),
                });
            } else if wanted_geometry.len() != got_geometry.len()
                || !wanted_geometry
                    .iter()
                    .zip(got_geometry)
                    .all(|(a, b)| near(*a, *b, GEOMETRY))
            {
                out.push(Divergence::GradientGeometry {
                    page,
                    mark,
                    markup: wanted_geometry.clone(),
                    document: got_geometry.clone(),
                });
            }
            let stops_agree = wanted_stops.len() == got_stops.len()
                && wanted_stops
                    .iter()
                    .zip(got_stops)
                    .all(|(a, b)| near(a.0, b.0, COLOUR) && rgb_near(a.1, b.1));
            if !stops_agree {
                out.push(Divergence::Stops {
                    page,
                    mark,
                    markup: wanted_stops.clone(),
                    document: got_stops.clone(),
                });
            }
        }
        (
            Paint::Image {
                pixels: wanted_pixels,
                copies: wanted_copies,
                area: wanted_area,
                ..
            },
            Paint::Image {
                pixels: got_pixels,
                copies: got_copies,
                area: got_area,
                ..
            },
        ) => {
            if wanted_pixels != got_pixels {
                out.push(Divergence::Pixels {
                    page,
                    mark,
                    markup: *wanted_pixels,
                    document: *got_pixels,
                });
            }
            if wanted_copies != got_copies {
                out.push(Divergence::Copies {
                    page,
                    mark,
                    markup: *wanted_copies,
                    document: *got_copies,
                });
            }
            if !wanted_area.agrees(got_area, PLACEMENT) {
                out.push(Divergence::Placement {
                    page,
                    mark,
                    markup: *wanted_area,
                    document: *got_area,
                });
            }
        }
        (wanted, got) => out.push(Divergence::PaintKind {
            page,
            mark,
            markup: wanted.kind(),
            document: got.kind(),
        }),
    }

    if !stated.bounds.agrees(&painted.bounds, PLACEMENT) {
        out.push(Divergence::Placement {
            page,
            mark,
            markup: stated.bounds,
            document: painted.bounds,
        });
    }
    if !near(stated.alpha, painted.alpha, COLOUR) {
        out.push(Divergence::Alpha {
            page,
            mark,
            markup: stated.alpha,
            document: painted.alpha,
        });
    }
}

fn compare_run(page: usize, run: usize, stated: &Run, drawn: &Run, out: &mut Vec<Divergence>) {
    if stated.glyphs != drawn.glyphs {
        out.push(Divergence::GlyphCount {
            page,
            run,
            markup: stated.glyphs,
            document: drawn.glyphs,
        });
    }
    if squeezed(&stated.text) != squeezed(&drawn.text) {
        out.push(Divergence::Text {
            page,
            run,
            markup: stated.text.clone(),
            document: drawn.text.clone(),
        });
    }
    if !near(stated.origin.0, drawn.origin.0, PLACEMENT)
        || !near(stated.origin.1, drawn.origin.1, PLACEMENT)
    {
        out.push(Divergence::Origin {
            page,
            run,
            markup: stated.origin,
            document: drawn.origin,
        });
    }
    if !near(stated.em, drawn.em, EM) {
        out.push(Divergence::EmSize {
            page,
            run,
            markup: stated.em,
            document: drawn.em,
        });
    }
    // Only where the markup states one. A cluster that states no advance takes
    // the face's own, which is the font's fact rather than the document's, and
    // comparing it would be asserting that this harness can read an `hmtx`.
    for (glyph, (wanted, got)) in stated.advances.iter().zip(&drawn.advances).enumerate() {
        let (Some(wanted), Some(got)) = (wanted, got) else {
            continue;
        };
        if !near(*wanted, *got, EM) {
            out.push(Divergence::Advance {
                page,
                run,
                glyph,
                markup: *wanted,
                document: *got,
            });
        }
    }
}

/// The whole comparison for one package: open it, census both sides, compare.
///
/// The document side is the facade route a caller travels — `Document::open`
/// routes by ECMA-388 E.3 and synthesises — so what is censused is what a
/// caller gets rather than an intermediate nothing ships.
#[must_use]
pub fn conservation(package: &[u8]) -> Verdict {
    let markup = markup_census(package);
    let Ok(document) = Document::open(package.to_vec()) else {
        return Verdict {
            facts: markup.facts(),
            conserved: 0,
            divergences: vec![Divergence::PageCount {
                markup: markup.pages.len(),
                document: 0,
            }],
        };
    };
    conserve(&markup, &document_census(&document))
}
