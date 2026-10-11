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

    /// The scale an em takes under a text matrix that may be **sheared**:
    /// the advance axis's length, or the height the other axis stands to it
    /// at — the area over the base — whichever is larger.
    ///
    /// The same as [`Matrix::scale`] for every unsheared matrix. Under 12.1.5's
    /// italic shear the second axis is longer than the glyph is tall by
    /// `1 / cos 20°`, and an em read off its length would be 6% too large.
    #[must_use]
    pub fn em_scale(self) -> f64 {
        let [a, b, c, d, _, _] = self.0;
        let base = (a * a + b * b).sqrt();
        if base == 0.0 {
            return self.scale();
        }
        base.max((a * d - b * c).abs() / base)
    }

    /// How far the second axis leans from perpendicular to the first, in
    /// degrees, positive when the top of a glyph leans along the advance.
    #[must_use]
    pub fn lean(self) -> f64 {
        let [a, b, c, d, _, _] = self.0;
        let along = a * c + b * d;
        let across = (a * d - b * c).abs();
        along.atan2(across).to_degrees()
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
        /// The stops' alphas, as `(offset, alpha)`, where they differ.
        ///
        /// `None` where every stop states one alpha, which is then the mark's
        /// own constant [`Mark::alpha`]. On the document side this is the
        /// `/DeviceGray` ramp of the `/Luminosity` soft mask in force when the
        /// gradient was painted — 18.3.2 interpolates the alpha between stops
        /// as it does each colour component, and a mask over a ramp is the one
        /// PDF construction that says so.
        alphas: Option<Vec<(f64, f64)>>,
        /// The colour halfway along each interval between stops, as
        /// `(offset, colour)`, the ends padded to `0` and `1` as 15.4.2 pads
        /// them and intervals narrower than a thousandth left out.
        ///
        /// The stops are where a ramp is pinned and these are how it bends:
        /// 18.3.1.2's `ColorInterpolationMode` changes nothing at a stop and
        /// everything between, so a census of stops alone conserves a ramp
        /// blended in the wrong space. The markup side computes each from the
        /// clause — the plain mean in sRGB, the mean of the linear light
        /// re-encoded in scRGB — and the document side evaluates the
        /// function there.
        middles: Vec<(f64, [f64; 3])>,
        /// Whether any stop's colour came from a `ContextColor` evaluated
        /// through its profile, on the markup side.
        ///
        /// Such a stop is converted to sRGB through an eight-bit transform
        /// (18.3.1.2), so its colour, and the middles beside it, are held to a
        /// byte rather than to [`COLOUR`]: the conversion's own resolution, and
        /// not a loosening of any stop the file stated in sRGB.
        profiled: bool,
    },
    /// 15.2.5's `ContextColor`: the components in the profile's own space,
    /// as the markup states them and as the content stream's `scn` writes
    /// them under a colour space that is not a device one.
    ///
    /// Compared as numbers, because a translation is the claim: the profile
    /// does the colour management and the components must arrive unchanged.
    Context { values: Vec<f64> },
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
            Paint::Context { .. } => "a context colour",
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
    /// 12.1.5's emboldening: on the markup side, `BoldSimulation` or
    /// `BoldItalicSimulation`; on the document side, Table 106's fill-and-stroke
    /// mode with a line width of 2% of the em.
    pub bold: bool,
    /// 12.1.5's italic: on the markup side, `ItalicSimulation` or
    /// `BoldItalicSimulation`; on the document side, a text matrix whose
    /// second axis leans 20° along the advance.
    pub italic: bool,
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

    /// Every solid fill — a `ContextColor` is one, in its profile's space.
    #[must_use]
    pub fn solids(&self) -> usize {
        self.count(|paint| matches!(paint, Paint::Solid { .. } | Paint::Context { .. }))
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

/// The picture a `{ColorConvertedBitmap picture profile}` wrapper names.
///
/// The census's own copy of the engine's rule, deliberately: this harness
/// shares no code with the engine above `tinker-pdf-zip`, and a shared parser
/// would make the two agree by construction rather than by measurement.
fn colour_converted_bitmap(uri: &str) -> Option<&str> {
    let inner = uri.trim().strip_prefix('{')?.strip_suffix('}')?;
    let rest = inner.trim().strip_prefix("ColorConvertedBitmap")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let picture = parts.next()?;
    parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some(picture.split('#').next().unwrap_or(picture))
}

/// Every part of a package, by its absolute name.
///
/// Absolute — with the leading solidus OPC 9.1.1.1 gives a part name — because
/// XPS 1.0 writes `ImageSource="/Resources/x.png"` and OpenXPS writes it
/// relative to the page part, and resolving both to one spelling is the only
/// way a census over the two dialects compares like with like.
///
/// **Interleaved pieces are joined here, by this harness's own reading of
/// 7.2.4** and not by `xps::opc`'s: an item named `<part>/[<n>].piece` or
/// `<part>/[<n>].last.piece` is piece `n` of `<part>`, and the part is its
/// pieces in number order. Crude on purpose — it checks nothing a reader
/// should refuse — because its job is to find the part the markup is in, and
/// a harness that asked the reader how to do that would inherit the reader's
/// answer.
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
    let mut pieces: BTreeMap<String, Vec<(u64, Vec<u8>)>> = BTreeMap::new();
    for (index, name) in names {
        let Ok(bytes) = archive.read(index) else {
            continue;
        };
        let piece = name.rsplit_once('/').and_then(|(part, last)| {
            let number = last
                .strip_suffix(".last.piece")
                .or_else(|| last.strip_suffix(".piece"))?
                .strip_prefix('[')?
                .strip_suffix(']')?
                .parse::<u64>()
                .ok()?;
            Some((part.to_owned(), number))
        });
        match piece {
            Some((part, number)) => pieces
                .entry(format!("/{part}"))
                .or_default()
                .push((number, bytes.into_owned())),
            None => {
                out.insert(format!("/{name}"), bytes.into_owned());
            }
        }
    }
    for (part, mut group) in pieces {
        group.sort_by_key(|(number, _)| *number);
        out.insert(
            part,
            group.into_iter().flat_map(|(_, bytes)| bytes).collect(),
        );
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

/// 15.2.5's `ContextColor <uri> a,c1,…,cn`, as `(components, alpha)`, each
/// clamped to `[0, 1]` as 15.2.5 says before any further processing.
fn context(value: &str) -> Option<(Vec<f64>, f64)> {
    let rest = value.trim().strip_prefix("ContextColor")?;
    let (_, numbers) = rest.trim_start().split_once(char::is_whitespace)?;
    let numbers = scalars(numbers);
    let (alpha, values) = numbers.split_first()?;
    Some((
        values.iter().map(|v| v.clamp(0.0, 1.0)).collect(),
        alpha.clamp(0.0, 1.0),
    ))
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
    //
    // The grammar is `"F" wsp* ("0" | "1")` — **whitespace between the letter
    // and the digit** — and this read `F0` and `F1` only, which is the whole of
    // what the two Microsoft serialisers write. Ghostscript writes `F 1`, so
    // the scanner met the `F` as a command it did not know, ran out of operands
    // and answered no bounds at all: two of `gs-paths.xps`'s six marks were
    // present in the document, absent from the markup census, and reported as a
    // conservation failure the engine had nothing to do with. Corrected here,
    // against the clause rather than against the file.
    if let Some(tail) = rest.strip_prefix('F') {
        let tail = tail.trim_start();
        if let Some(digit) = tail.strip_prefix('0').or_else(|| tail.strip_prefix('1')) {
            rest = digit.trim_start();
        }
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
    // TIFF 6.0: a byte-order mark, `42`, and an offset to the first image file
    // directory. The directory is a count and then twelve-byte entries, of
    // which tags 256 and 257 are the width and the length. Read here because
    // Ghostscript writes every picture as a TIFF, and a census that could not
    // measure one counted zero pictures on a page that states two.
    //
    // Only the first directory, and only SHORT and LONG: that is what these
    // packages carry, and a census that guessed at the rest would be stating
    // more than it measured.
    if part.starts_with(b"II*\x00") || part.starts_with(b"MM\x00*") {
        let big = part.starts_with(b"MM");
        let u16_at = |at: usize| -> Option<u16> {
            let raw: [u8; 2] = part.get(at..at + 2)?.try_into().ok()?;
            Some(if big {
                u16::from_be_bytes(raw)
            } else {
                u16::from_le_bytes(raw)
            })
        };
        let u32_at = |at: usize| -> Option<u32> {
            let raw: [u8; 4] = part.get(at..at + 4)?.try_into().ok()?;
            Some(if big {
                u32::from_be_bytes(raw)
            } else {
                u32::from_le_bytes(raw)
            })
        };
        let ifd = u32_at(4)? as usize;
        let count = u16_at(ifd)? as usize;
        let (mut width, mut height) = (None, None);
        for index in 0..count.min(512) {
            let entry = ifd + 2 + index * 12;
            let tag = u16_at(entry)?;
            let kind = u16_at(entry + 2)?;
            // A SHORT sits in the first two bytes of the value field and a
            // LONG fills it, both at the entry's own offset because a single
            // value of either fits inline.
            let value = match kind {
                3 => u32::from(u16_at(entry + 8)?),
                4 => u32_at(entry + 8)?,
                _ => continue,
            };
            match tag {
                256 => width = Some(value),
                257 => height = Some(value),
                _ => {}
            }
        }
        return Some((width?, height?));
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
    /// A `ContextColor`'s components and its alpha.
    Context(Vec<f64>, f64),
    Gradient {
        kind: Gradient,
        geometry: Vec<f64>,
        stops: Vec<(f64, [f64; 3])>,
        /// Where the stops' alphas differ, each stop's.
        alphas: Option<Vec<(f64, f64)>>,
        /// See [`Paint::Gradient::middles`].
        middles: Vec<(f64, [f64; 3])>,
        /// See [`Paint::Gradient::profiled`].
        profiled: bool,
        /// The one alpha the stops share, times the brush's `Opacity`; the
        /// `Opacity` alone where they differ.
        alpha: f64,
    },
    Image {
        source: String,
        viewbox: Rect,
        viewport: Rect,
        tile: String,
        /// `<ImageBrush.Transform>`, which XPS writes as a child element
        /// rather than as an attribute.
        ///
        /// Read because Ghostscript writes every picture this way: the
        /// viewport states a 32-unit square and the transform is what puts it
        /// on the page at its real size. A census that read only the
        /// attribute form measured those pictures in the wrong place — and
        /// said nothing, because a placement is compared and not counted.
        transform: Matrix,
    },
}

/// The stops of a gradient brush, in the order the markup wrote them, with
/// each stop's alpha and whether its colour came through a profile.
fn stops(
    inner: &str,
    part: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Vec<(f64, [f64; 3], f64, bool)> {
    tags(inner)
        .iter()
        .filter(|tag| tag.name.ends_with("GradientStop"))
        .filter_map(|tag| {
            let offset = attribute(tag.attributes, "Offset")?.parse::<f64>().ok()?;
            let value = attribute(tag.attributes, "Color")?;
            if let Some((values, alpha)) = context(value) {
                let rgb = profiled_srgb(value, &values, part, parts)?;
                return Some((offset, rgb, alpha, true));
            }
            let (rgb, alpha) = colour(value)?;
            Some((offset, rgb, alpha, false))
        })
        .collect()
}

/// The sRGB a `ContextColor` comes to, by 18.3.1.2's conversion, for the one
/// kind of profile this harness evaluates itself: a `GRAY` profile over an
/// `XYZ` connection space whose `kTRC` is the identity or one gamma.
///
/// Written out from ICC.1 rather than borrowed: `Y = v^γ` from the curve
/// (`curv` with no entries is the identity, with one a `u8Fixed8` gamma), and
/// an achromatic `Y` is that light in each sRGB channel, encoded by
/// IEC 61966-2-1. Anything else answers `None`, and a census that cannot read
/// a stop states one fewer — which the comparison then reports.
fn profiled_srgb(
    value: &str,
    components: &[f64],
    part: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Option<[f64; 3]> {
    let rest = value.trim().strip_prefix("ContextColor")?;
    let (uri, _) = rest.trim_start().split_once(char::is_whitespace)?;
    let bytes = parts.get(&resolve(part, uri))?;
    if bytes.get(16..20)? != b"GRAY" || bytes.get(20..24)? != b"XYZ " {
        return None;
    }
    let count = u32::from_be_bytes(bytes.get(128..132)?.try_into().ok()?) as usize;
    let curve = (0..count.min(64)).find_map(|index| {
        let at = 132 + index * 12;
        let entry = bytes.get(at..at + 12)?;
        (&entry[0..4] == b"kTRC").then(|| {
            let offset = u32::from_be_bytes(entry[4..8].try_into().ok()?) as usize;
            bytes.get(offset..offset + 14)
        })?
    })?;
    if &curve[0..4] != b"curv" {
        return None;
    }
    let gamma = match u32::from_be_bytes(curve[8..12].try_into().ok()?) {
        0 => 1.0,
        1 => f64::from(u16::from_be_bytes([curve[12], curve[13]])) / 256.0,
        _ => return None,
    };
    let v = components.first().copied().unwrap_or(0.0).clamp(0.0, 1.0);
    let light = v.powf(gamma);
    let encoded = if light <= 0.003_130_8 {
        light * 12.92
    } else {
        1.055 * light.powf(1.0 / 2.4) - 0.055
    };
    Some([encoded; 3])
}

/// A gradient brush from its kind, its geometry, its stops and its start tag.
///
/// The stops' alphas are one fact when they agree — a constant alpha over the
/// whole element, with the brush's `Opacity` — and a ramp of their own when
/// they do not.
fn gradient_brush(
    kind: Gradient,
    geometry: Vec<f64>,
    attributes: &str,
    inner: &str,
    part: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Brush {
    let read = stops(inner, part, parts);
    let profiled = read.iter().any(|stop| stop.3);
    let read: Vec<(f64, [f64; 3], f64)> = read
        .into_iter()
        .map(|(offset, rgb, alpha, _)| (offset, rgb, alpha))
        .collect();
    let opacity = attribute(attributes, "Opacity")
        .and_then(|o| o.parse::<f64>().ok())
        .unwrap_or(1.0);
    // 18.3.1.1's artificial stops: a gradient whose stops do not reach 0 or 1
    // takes its nearest stop's colour out to them.
    let mut read = read;
    read.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let (Some(first), Some(last)) = (read.first().copied(), read.last().copied()) {
        if first.0 > 0.0 {
            read.insert(0, (0.0, first.1, first.2));
        }
        if last.0 < 1.0 {
            read.push((1.0, last.1, last.2));
        }
    }
    let uniform = read.first().map_or(1.0, |stop| stop.2);
    let varying = read.iter().any(|stop| stop.2 != uniform);
    let linear_light =
        attribute(attributes, "ColorInterpolationMode") == Some("ScRgbLinearInterpolation");
    Brush::Gradient {
        kind,
        geometry,
        stops: read.iter().map(|stop| (stop.0, stop.1)).collect(),
        alphas: varying.then(|| read.iter().map(|stop| (stop.0, stop.2)).collect()),
        alpha: if varying { opacity } else { uniform * opacity },
        middles: stated_middles(&read, linear_light),
        profiled,
    }
}

/// [`Paint::Gradient::middles`], from the markup's stops, by 18.3.1.2.
fn stated_middles(read: &[(f64, [f64; 3], f64)], linear_light: bool) -> Vec<(f64, [f64; 3])> {
    // IEC 61966-2-1 both ways, written out here rather than borrowed.
    let decode = |c: f64| {
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |l: f64| {
        if l <= 0.003_130_8 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        }
    };
    let mut points: Vec<(f64, [f64; 3])> = read.iter().map(|stop| (stop.0, stop.1)).collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) {
        if first.0 > 0.0 {
            points.insert(0, (0.0, first.1));
        }
        if last.0 < 1.0 {
            points.push((1.0, last.1));
        }
    }
    points
        .windows(2)
        .filter(|pair| pair[1].0 - pair[0].0 >= 1e-3)
        .map(|pair| {
            let (a, b) = (pair[0].1, pair[1].1);
            let mut mid = [0.0; 3];
            for (channel, slot) in mid.iter_mut().enumerate() {
                *slot = if linear_light {
                    encode((decode(a[channel]) + decode(b[channel])) / 2.0)
                } else {
                    (a[channel] + b[channel]) / 2.0
                };
            }
            ((pair[0].0 + pair[1].0) / 2.0, mid)
        })
        .collect()
}

/// One brush element, from its start tag and everything under it.
fn brush(
    name: &str,
    attributes: &str,
    inner: &str,
    part: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Option<Brush> {
    let local = name.rsplit(':').next().unwrap_or(name);
    match local {
        "SolidColorBrush" => {
            let value = attribute(attributes, "Color")?;
            let opacity = attribute(attributes, "Opacity")
                .and_then(|o| o.parse::<f64>().ok())
                .unwrap_or(1.0);
            if let Some((values, alpha)) = context(value) {
                return Some(Brush::Context(values, alpha * opacity));
            }
            let (rgb, alpha) = colour(value)?;
            Some(Brush::Solid(rgb, alpha * opacity))
        }
        "LinearGradientBrush" => {
            let start = scalars(attribute(attributes, "StartPoint")?);
            let end = scalars(attribute(attributes, "EndPoint")?);
            (start.len() == 2 && end.len() == 2).then(|| {
                gradient_brush(
                    Gradient::Linear,
                    vec![start[0], start[1], end[0], end[1]],
                    attributes,
                    inner,
                    part,
                    parts,
                )
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
            (centre.len() == 2).then(|| {
                gradient_brush(
                    Gradient::Radial,
                    vec![origin[0], origin[1], 0.0, centre[0], centre[1], radius],
                    attributes,
                    inner,
                    part,
                    parts,
                )
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
            transform: element_transform(inner),
        }),
        _ => None,
    }
}

/// The matrix a `<…Brush.Transform><MatrixTransform Matrix="a,b,c,d,e,f"/>`
/// child states, or the identity where there is none.
///
/// XPS gives a transform two spellings and this census read only one of them.
/// The attribute form is handled where elements are walked; this is the
/// element form, which is what every Ghostscript package uses.
fn element_transform(inner: &str) -> Matrix {
    let Some(at) = inner.find(".Transform") else {
        return Matrix::IDENTITY;
    };
    let rest = &inner[at..];
    let Some(start) = rest.find("Matrix") else {
        return Matrix::IDENTITY;
    };
    let rest = &rest[start + "Matrix".len()..];
    let Some(open) = rest.find('"') else {
        return Matrix::IDENTITY;
    };
    let rest = &rest[open + 1..];
    let Some(close) = rest.find('"') else {
        return Matrix::IDENTITY;
    };
    let numbers = scalars(&rest[..close]);
    if numbers.len() != 6 {
        return Matrix::IDENTITY;
    }
    Matrix([
        numbers[0], numbers[1], numbers[2], numbers[3], numbers[4], numbers[5],
    ])
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
        if let Some(brush) = brush(tag.name, tag.attributes, inner, part, parts) {
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
            None => context(fill)
                .map(|(values, a)| Brush::Context(values, a))
                .or_else(|| colour(fill).map(|(rgb, a)| Brush::Solid(rgb, a))),
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
                if let Some(brush) = brush(tag.name, tag.attributes, nested, part, parts) {
                    found = Some(brush);
                    break;
                }
            }
            found
        }
    }?;

    let (paint, brush_alpha) = match brush {
        Brush::Solid(rgb, a) => (Paint::Solid { rgb }, a),
        Brush::Context(values, a) => (Paint::Context { values }, a),
        Brush::Gradient {
            kind,
            geometry,
            stops,
            alphas,
            alpha,
            middles,
            profiled,
        } => (
            Paint::Gradient {
                kind,
                geometry,
                stops,
                alphas,
                middles,
                profiled,
            },
            alpha,
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
            transform: own,
        } => {
            let transform = transform.compose(own);
            // XPS 1.0 writes this absolute and OpenXPS writes it relative to
            // the page part, which is why both spellings resolve here rather
            // than one being assumed: the same picture under two dialects has
            // to census as the same part.
            // 9.1.5's `{ColorConvertedBitmap picture profile}` names the
            // picture in its first reference. The markup census reads the
            // wrapper for the same reason the engine does — a census that
            // could not address these pictures counted zero of them, which is
            // what kept `gs-images.xps` out of the sweep until the engine
            // learned to draw them.
            let stated = colour_converted_bitmap(&source).unwrap_or(source.as_str());
            let name = resolve(part, stated);
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
    let simulation = attribute(attributes, "StyleSimulations").unwrap_or("None");
    let bold = matches!(simulation, "BoldSimulation" | "BoldItalicSimulation");
    let italic = matches!(simulation, "ItalicSimulation" | "BoldItalicSimulation");
    // 12.1.5's S5.6, written out from the clause: an emboldened glyph moves
    // up and to the right by 1% of the em, which in the element's own
    // y-down space is `+x, −y` upright and, for a sideways run whose advance
    // runs down the page and whose glyphs stand to its right, `+x, +y`.
    let (x, y) = if bold {
        let offset = em * 0.01;
        if attribute(attributes, "IsSideways") == Some("true") {
            (x + offset, y + offset)
        } else {
            (x + offset, y - offset)
        }
    } else {
        (x, y)
    };
    let (x, y) = transform.apply(x, y);
    Some(Run {
        origin: (x, y),
        em: em * transform.scale(),
        text,
        glyphs,
        rgb,
        advances,
        bold,
        italic,
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
fn function_stops(
    cos: &CosDocument,
    function: &Object,
    reference: Option<ObjRef>,
) -> Vec<(f64, [f64; 3])> {
    let values = function_values(cos, function, reference);
    if values.iter().any(|(_, value)| value.len() != 3) {
        return Vec::new();
    }
    values
        .into_iter()
        .map(|(offset, value)| (offset, [value[0], value[1], value[2]]))
        .collect()
}

/// [`Paint::Gradient::middles`], out of a 7.10 function: each interval's
/// colour halfway along it, evaluated.
///
/// Type 2 is `C0 + x^N (C1 − C0)` at `x = ½`; a one-input type 0 is its
/// samples interpolated at the middle of its domain, by 7.10.2's default
/// linear order; type 3 is its pieces, placed by its bounds, with any piece
/// narrower than a thousandth — the nudge a hard stop is written with — left
/// out, as the markup side leaves out the interval it stands for.
fn function_middles(
    cos: &CosDocument,
    function: &Object,
    reference: Option<ObjRef>,
) -> Vec<(f64, [f64; 3])> {
    let dict = match function {
        Object::Stream(stream) => &stream.dict,
        other => match other.as_dict() {
            Some(dict) => dict,
            None => return Vec::new(),
        },
    };
    let three = |values: &[f64]| (values.len() == 3).then(|| [values[0], values[1], values[2]]);
    match key(cos, dict, b"FunctionType").as_int() {
        Some(2) => {
            let (Some(c0), Some(c1)) = (array(cos, dict, b"C0"), array(cos, dict, b"C1")) else {
                return Vec::new();
            };
            let n = key(cos, dict, b"N").as_number().unwrap_or(1.0);
            let t = 0.5f64.powf(n);
            let mid: Vec<f64> = c0.iter().zip(&c1).map(|(a, b)| a + t * (b - a)).collect();
            three(&mid).map_or_else(Vec::new, |mid| vec![(0.5, mid)])
        }
        Some(0) => {
            let size = array(cos, dict, b"Size").unwrap_or_default();
            let range = array(cos, dict, b"Range").unwrap_or_default();
            let bits = key(cos, dict, b"BitsPerSample").as_int().unwrap_or(0);
            let (Some(reference), [points], 6, 8 | 16) =
                (reference, size.as_slice(), range.len(), bits)
            else {
                return Vec::new();
            };
            let Ok(data) = cos.stream_decoded(reference) else {
                return Vec::new();
            };
            let points = *points as usize;
            let width = if bits == 16 { 2 } else { 1 };
            let max = if bits == 16 { 65_535.0 } else { 255.0 };
            let sample = |index: usize, channel: usize| -> f64 {
                let at = (index * 3 + channel) * width;
                let raw = if width == 2 {
                    data.get(at..at + 2)
                        .map_or(0.0, |b| f64::from(u16::from_be_bytes([b[0], b[1]])))
                } else {
                    data.get(at).map_or(0.0, |b| f64::from(*b))
                };
                let (lo, hi) = (range[channel * 2], range[channel * 2 + 1]);
                lo + raw / max * (hi - lo)
            };
            let position = (points as f64 - 1.0) / 2.0;
            let (low, frac) = (position.floor() as usize, position - position.floor());
            let high = (low + 1).min(points - 1);
            let mut mid = [0.0; 3];
            for (channel, slot) in mid.iter_mut().enumerate() {
                let (a, b) = (sample(low, channel), sample(high, channel));
                *slot = a + frac * (b - a);
            }
            vec![(0.5, mid)]
        }
        Some(3) => {
            let bounds = array(cos, dict, b"Bounds").unwrap_or_default();
            let parts = key(cos, dict, b"Functions");
            let Some(parts) = parts.as_array() else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                let from = if index == 0 {
                    0.0
                } else {
                    bounds.get(index - 1).copied().unwrap_or(0.0)
                };
                let to = bounds.get(index).copied().unwrap_or(1.0);
                if to - from < 1e-3 {
                    continue;
                }
                let (resolved, inner_ref) = match part.as_objref() {
                    Some(r) => (cos.get(r).unwrap_or(Arc::new(Object::Null)), Some(r)),
                    None => (Arc::new(part.clone()), None),
                };
                for (offset, mid) in function_middles(cos, &resolved, inner_ref) {
                    out.push((from + offset * (to - from), mid));
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// [`function_stops`] for a function of any number of outputs — one, for the
/// `/DeviceGray` ramp a soft mask's alphas are painted as.
///
/// A one-input type 0 states its two ends, its first and last samples
/// through `/Range` — which is where the writer pins each stop of a ramp it
/// had to sample — and needs its `reference` to be decoded.
fn function_values(
    cos: &CosDocument,
    function: &Object,
    reference: Option<ObjRef>,
) -> Vec<(f64, Vec<f64>)> {
    let dict = match function {
        Object::Stream(stream) => &stream.dict,
        other => match other.as_dict() {
            Some(dict) => dict,
            None => return Vec::new(),
        },
    };
    let colour_of = |name: &[u8]| -> Option<Vec<f64>> { array(cos, dict, name) };
    match key(cos, dict, b"FunctionType").as_int() {
        Some(2) => match (colour_of(b"C0"), colour_of(b"C1")) {
            (Some(c0), Some(c1)) if c0.len() == c1.len() => vec![(0.0, c0), (1.0, c1)],
            _ => Vec::new(),
        },
        Some(0) => {
            let size = array(cos, dict, b"Size").unwrap_or_default();
            let range = array(cos, dict, b"Range").unwrap_or_default();
            let bits = key(cos, dict, b"BitsPerSample").as_int().unwrap_or(0);
            let outputs = range.len() / 2;
            let (Some(reference), [points], 8 | 16) = (reference, size.as_slice(), bits) else {
                return Vec::new();
            };
            let Ok(data) = cos.stream_decoded(reference) else {
                return Vec::new();
            };
            let width = if bits == 16 { 2 } else { 1 };
            let max = if bits == 16 { 65_535.0 } else { 255.0 };
            let at = |index: usize| -> Vec<f64> {
                (0..outputs)
                    .map(|channel| {
                        let byte = (index * outputs + channel) * width;
                        let raw = if width == 2 {
                            data.get(byte..byte + 2)
                                .map_or(0.0, |b| f64::from(u16::from_be_bytes([b[0], b[1]])))
                        } else {
                            data.get(byte).map_or(0.0, |b| f64::from(*b))
                        };
                        let (lo, hi) = (range[channel * 2], range[channel * 2 + 1]);
                        lo + raw / max * (hi - lo)
                    })
                    .collect()
            };
            let last = (*points as usize).saturating_sub(1);
            vec![(0.0, at(0)), (1.0, at(last))]
        }
        Some(3) => {
            let bounds = array(cos, dict, b"Bounds").unwrap_or_default();
            let parts = key(cos, dict, b"Functions");
            let Some(parts) = parts.as_array() else {
                return Vec::new();
            };
            let mut out: Vec<(f64, Vec<f64>)> = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                let (resolved, inner_ref) = match part.as_objref() {
                    Some(reference) => (
                        cos.get(reference).unwrap_or(Arc::new(Object::Null)),
                        Some(reference),
                    ),
                    None => (Arc::new(part.clone()), None),
                };
                let inner = function_values(cos, &resolved, inner_ref);
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
    let function = key(cos, dict, b"Function");
    let reference = dict.get_ref(cos.intern(b"Function"));
    let stops = function_stops(cos, &function, reference);
    Some(Paint::Gradient {
        kind,
        geometry,
        stops,
        alphas: None,
        middles: function_middles(cos, &function, reference),
        profiled: false,
    })
}

/// The alphas a `/Luminosity` soft mask paints: the `(offset, grey)` ramp of
/// the one shading its group form floods with `sh`.
///
/// Read out of the form's own stream, because that is where the writer puts
/// the grey — the mask is a group whose content is the alphas.
fn mask_alphas(cos: &CosDocument, resources: &Dict, state: &Dict) -> Option<Vec<(f64, f64)>> {
    let mask = key(cos, state, b"SMask");
    let mask = mask.as_dict()?;
    let form = mask.get_ref(cos.intern(b"G"))?;
    let content = cos.stream_decoded(form).ok()?;
    let object = cos.get(form).ok()?;
    let Object::Stream(stream) = object.as_ref() else {
        return None;
    };
    let own = key(cos, &stream.dict, b"Resources");
    let scope = own.as_dict().cloned().unwrap_or_else(|| resources.clone());
    let mut tokens = Tokenizer::new(&content);
    let mut last: Option<Vec<u8>> = None;
    while let Some(token) = tokens.next_token() {
        match token {
            Token::Name(name) => last = Some(name),
            Token::Operator(operator) if operator.as_slice() == b"sh" => {
                let shading = entry(cos, &scope, b"Shading", last.as_deref()?)?;
                let dict = shading.as_dict()?;
                let values = function_values(
                    cos,
                    &key(cos, dict, b"Function"),
                    dict.get_ref(cos.intern(b"Function")),
                );
                if values.iter().any(|(_, value)| value.len() != 1) {
                    return None;
                }
                return Some(
                    values
                        .into_iter()
                        .map(|(at, value)| (at, value[0]))
                        .collect(),
                );
            }
            _ => {}
        }
    }
    None
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
    /// Table 106's text rendering mode, which is graphics state.
    render: i64,
    /// The alphas the `/Luminosity` soft mask in force paints, if any — see
    /// [`Paint::Gradient::alphas`].
    soft: Option<Vec<(f64, f64)>>,
    /// The components an `scn` set under a colour space that is not one of
    /// the device spaces — an `/ICCBased` or a `/DeviceN` — and `Some` from
    /// the moment such a space is set.
    components: Option<Vec<f64>>,
    /// `w`, in user space.
    width: f64,
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
                render: 0,
                width: 1.0,
                soft: None,
                components: None,
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
                    let state = entry(self.cos, resources, b"ExtGState", name);
                    if let Some(alpha) = state.as_ref().and_then(|state| {
                        state
                            .as_dict()
                            .and_then(|dict| key(self.cos, dict, b"ca").as_number())
                    }) {
                        self.frame.alpha = alpha;
                    }
                    if let Some(soft) = state
                        .as_ref()
                        .and_then(|state| state.as_dict())
                        .and_then(|dict| mask_alphas(self.cos, resources, dict))
                    {
                        self.frame.soft = Some(soft);
                    }
                }
            }
            b"rg" if numbers.len() == 3 => {
                self.frame.rgb = [numbers[0], numbers[1], numbers[2]];
                self.frame.pattern = None;
                self.frame.components = None;
            }
            b"g" if numbers.len() == 1 => {
                self.frame.rgb = [numbers[0]; 3];
                self.frame.pattern = None;
                self.frame.components = None;
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
                self.frame.components = None;
            }
            b"cs" => {
                self.frame.pattern = None;
                // A named space that is not a device one — a resource — sets
                // components rather than a colour.
                self.frame.components = match operands.last() {
                    Some(Token::Name(name))
                        if !matches!(
                            name.as_slice(),
                            b"DeviceRGB" | b"DeviceGray" | b"DeviceCMYK" | b"Pattern"
                        ) =>
                    {
                        Some(Vec::new())
                    }
                    _ => None,
                };
            }
            b"scn" | b"sc" => match operands.last() {
                Some(Token::Name(name)) => self.frame.pattern = Some(name.clone()),
                _ if self.frame.components.is_some() => {
                    self.frame.components = Some(numbers.clone());
                }
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
                            paint: self.faded(paint),
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
                            paint: self.faded(paint),
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
            b"Tr" if numbers.len() == 1 => self.frame.render = numbers[0] as i64,
            b"w" if numbers.len() == 1 => self.frame.width = numbers[0],
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
                        let em = self.size * placed.em_scale();
                        // Mode 2 fills and then strokes, and the stroke is the
                        // emboldening only at 12.1.5's width: 2% of the em, in
                        // the user space the CTM carries to points.
                        let stroke = self.frame.width * self.frame.ctm.scale();
                        let bold = self.frame.render == 2 && near(stroke, em * 0.02, em * 1e-3);
                        let italic = near(placed.lean(), 20.0, 0.05);
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
                            bold,
                            italic,
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

    /// A gradient painted under a soft mask of alphas carries them.
    fn faded(&self, paint: Paint) -> Paint {
        match paint {
            Paint::Gradient {
                kind,
                geometry,
                stops,
                middles,
                profiled,
                ..
            } => Paint::Gradient {
                kind,
                geometry,
                stops,
                alphas: self.frame.soft.clone(),
                middles,
                profiled,
            },
            other => other,
        }
    }

    /// What the current colour operators say this fill is painted with.
    fn paint(&self, resources: &Dict) -> Option<Paint> {
        match &self.frame.pattern {
            Some(name) => {
                let pattern = entry(self.cos, resources, b"Pattern", name)?;
                let reference = entry_ref(self.cos, resources, b"Pattern", name);
                pattern_paint(self.cos, &pattern, reference)
            }
            None => Some(match &self.frame.components {
                Some(values) => Paint::Context {
                    values: values.clone(),
                },
                None => Paint::Solid {
                    rgb: self.frame.rgb,
                },
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
                // 11.6.6: a transparency group starts with no soft mask of its
                // own; the one in force applies to the group's result.
                let group = !matches!(key(self.cos, dict, b"Group").as_ref(), Object::Null);
                inner.frame = Frame {
                    ctm: base,
                    soft: if group { None } else { self.frame.soft.clone() },
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
    /// A `ContextColor`'s components, not the ones the markup stated.
    Components {
        page: usize,
        mark: usize,
        markup: Vec<f64>,
        document: Vec<f64>,
    },
    /// A gradient's colours between its stops: blended in a space the
    /// markup's `ColorInterpolationMode` does not name.
    Middles {
        page: usize,
        mark: usize,
        markup: Vec<(f64, [f64; 3])>,
        document: Vec<(f64, [f64; 3])>,
    },
    /// A gradient's stop alphas: stated and not masked, masked and not
    /// stated, or a different ramp.
    StopAlphas {
        page: usize,
        mark: usize,
        markup: Option<Vec<(f64, f64)>>,
        document: Option<Vec<(f64, f64)>>,
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
    /// 12.1.5's simulations, as `(bold, italic)` on each side.
    Simulation {
        page: usize,
        run: usize,
        markup: (bool, bool),
        document: (bool, bool),
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
                alphas: wanted_alphas,
                middles: wanted_middles,
                profiled,
            },
            Paint::Gradient {
                kind: got_kind,
                geometry: got_geometry,
                stops: got_stops,
                alphas: got_alphas,
                middles: got_middles,
                ..
            },
        ) => {
            // Half an eight-bit step: the writer samples a linear-light ramp
            // at sixteen bits and interpolates between samples, and 0.0018 is
            // the most that comes to anywhere (`brush.rs`'s own measurement).
            // A byte, where a stop came through a profile's eight-bit
            // transform — see `Paint::Gradient::profiled`.
            let byte = 1.5 / 255.0;
            let (stop_tolerance, middle_tolerance) = if *profiled {
                (byte, byte)
            } else {
                (COLOUR, 2e-3)
            };
            let middles_agree = wanted_middles.len() == got_middles.len()
                && wanted_middles.iter().zip(got_middles).all(|(a, b)| {
                    near(a.0, b.0, GEOMETRY)
                        && a.1
                            .iter()
                            .zip(&b.1)
                            .all(|(x, y)| near(*x, *y, middle_tolerance))
                });
            if !middles_agree {
                out.push(Divergence::Middles {
                    page,
                    mark,
                    markup: wanted_middles.clone(),
                    document: got_middles.clone(),
                });
            }
            let alphas_agree = match (wanted_alphas, got_alphas) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.len() == b.len()
                        && a.iter()
                            .zip(b)
                            .all(|(x, y)| near(x.0, y.0, COLOUR) && near(x.1, y.1, COLOUR))
                }
                _ => false,
            };
            if !alphas_agree {
                out.push(Divergence::StopAlphas {
                    page,
                    mark,
                    markup: wanted_alphas.clone(),
                    document: got_alphas.clone(),
                });
            }
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
                && wanted_stops.iter().zip(got_stops).all(|(a, b)| {
                    near(a.0, b.0, COLOUR)
                        && a.1
                            .iter()
                            .zip(&b.1)
                            .all(|(x, y)| near(*x, *y, stop_tolerance))
                });
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
        (Paint::Context { values: wanted }, Paint::Context { values: got }) => {
            if wanted.len() != got.len()
                || !wanted.iter().zip(got).all(|(a, b)| near(*a, *b, COLOUR))
            {
                out.push(Divergence::Components {
                    page,
                    mark,
                    markup: wanted.clone(),
                    document: got.clone(),
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
    if (stated.bold, stated.italic) != (drawn.bold, drawn.italic) {
        out.push(Divergence::Simulation {
            page,
            run,
            markup: (stated.bold, stated.italic),
            document: (drawn.bold, drawn.italic),
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
