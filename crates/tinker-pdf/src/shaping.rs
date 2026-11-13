//! Shaped text into a document: milestone 7 of `docs/design/shaping.md`.
//!
//! `DocumentBuilder::glyph_run` has taken caller-positioned glyphs since gap
//! 30, and `Glyph` has paired a glyph index with the characters it stands for
//! since the same milestone, *"so `/ToUnicode` and text extraction survive
//! ligatures"*. So this is wiring rather than machinery, and what it wires is
//! the one thing neither side can do alone: turning a shaped run's **clusters**
//! back into the text each glyph stands for.
//!
//! # The cluster is the only thing that survives
//!
//! After `GSUB` has run there is nothing left of the text but the cluster.
//! Three characters may have become one glyph and one character three, and the
//! glyph indices say nothing about either. `tinker-pdf-shape` keeps a byte
//! offset on every glyph and keeps them monotonic for exactly this moment.
//!
//! Three rules turn that into `/ToUnicode`, and the last two are the ones
//! that are easy to get wrong:
//!
//! - **A glyph stands for the text from its own cluster to the next
//!   *different* one.** A ligature's cluster is the smallest of the glyphs it
//!   replaced, so `ffi` comes back whole.
//! - **Where several glyphs share a cluster, the first takes the text and the
//!   rest take nothing.** A decomposition — one character that became a base
//!   and two marks — would otherwise put the character into the extracted text
//!   three times, and a reader would find `naïve` spelled `naïïïve`.
//! - **A glyph the shaper inserted takes nothing**, and the first glyph of
//!   its cluster that it did not insert takes the text in its place
//!   (`tinker_pdf_shape::ShapedGlyph::inserted`). The dotted circle under a
//!   broken cluster's mark is drawn and is not typed: given the mark's text,
//!   it would map the circle's glyph to whichever mark it stood under first,
//!   and every broken cluster after that would extract as that one. It is
//!   drawn inside a span that states nothing ([`draw`]), because no entry in
//!   `/ToUnicode` reads as nothing.
//!
//! # What is drawn is what was measured
//!
//! The positions written are the shaper's own advances, accumulated. They are
//! *not* recomputed from `/W`: `glyph_run` works out the `TJ` adjustment
//! between where its own pen is and where the caller put each glyph, so the
//! difference between the shaper's advance and the rounded one a reader will
//! use is absorbed exactly, glyph by glyph, rather than accumulating. That is
//! `docs/design/shaping.md`'s answer to its own third risk: *"creation writes
//! the shaper's own advances through `glyph_run`, so what was measured is what
//! is drawn"*.
//!
//! # One face, one set of plans
//!
//! A [`Run`] carries the shaper its caller made rather than the face, so a
//! caller writing many runs in one face makes each shaper with the face's
//! [`tinker_pdf_shape::Plans`], kept beside the face's bytes for the whole
//! document build: each stage's lookups are resolved and each lookup's
//! coverage indexed once for the face, not once a run
//! (`docs/design/shaping.md`, *Lookup plans and coverage indexes*).

use crate::colour_glyphs::ColourFace;
use tinker_pdf_cos::build::{DocumentBuilder, Glyph, PlacedGlyph};
use tinker_pdf_shape::bidi::{reorder, BaseDirection, Paragraph};
use tinker_pdf_shape::shape::itemize;
use tinker_pdf_shape::{ShapedRun, Shaper};

/// One glyph of a shaped run, ready to be written.
///
/// The owned twin of [`tinker_pdf_cos::build::PlacedGlyph`], which borrows the
/// text it stands for. The borrow has to come from somewhere and a run's text
/// is assembled here, so this holds it and [`Placed::as_glyph`] lends it back.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The glyph index in the face the run was shaped against.
    pub id: u16,
    /// The characters this glyph stands for, for `/ToUnicode`. Empty where
    /// another glyph of the same cluster has already claimed them.
    pub text: String,
    /// Its origin along the baseline, in text space, from the run's origin.
    pub x: f64,
    /// Its origin off the baseline — 9.4.3's `Ts`.
    pub rise: f64,
    /// Whether the shaper inserted this glyph rather than mapping it from a
    /// character (`tinker_pdf_shape::ShapedGlyph::inserted`): a dotted circle
    /// under a broken cluster's mark, drawn and standing for no text, which
    /// [`draw`] writes inside a span that says so.
    pub inserted: bool,
}

impl Placed {
    /// The borrowed form `DocumentBuilder::glyph_run` takes.
    #[must_use]
    pub fn as_glyph(&self) -> PlacedGlyph<'_> {
        PlacedGlyph {
            glyph: Glyph {
                id: self.id,
                text: &self.text,
            },
            x: self.x,
            rise: self.rise,
        }
    }
}

/// The text each glyph of one shaped run stands for, in the run's own
/// **logical** order, one entry per glyph.
///
/// The three rules in this module's header, in one place. It is one place on
/// purpose: `epub/paint.rs` draws shaped runs too, and a second copy of a rule
/// this delicate is a copy that eventually learns something the first one did
/// not — the "one extractor, not two" discipline that
/// [`crate::epub::paint::face_runs`] applies to measurement, applied to the
/// writer side.
///
/// The slices borrow `text`, which is the paragraph the run was shaped from,
/// because a cluster is a byte offset into *that* and not into the run's own
/// slice.
#[must_use]
pub fn cluster_texts<'a>(text: &'a str, run: &ShapedRun) -> Vec<&'a str> {
    let glyphs = run.glyphs();
    let mut out = vec![""; glyphs.len()];
    let mut start = 0usize;
    while let Some(glyph) = glyphs.get(start) {
        // One cluster is the stretch of glyphs sharing its offset. It stands
        // for the text from that offset to the next *different* one — a
        // ligature's cluster is the smallest of the glyphs it replaced, so
        // `ffi` comes back whole — and one glyph of it takes that text, which
        // is what stops a decomposed `ï` from extracting as three characters.
        let end = glyphs
            .get(start..)
            .and_then(|rest| rest.iter().position(|next| next.cluster != glyph.cluster))
            .map_or(glyphs.len(), |length| start.saturating_add(length));
        let from = usize::try_from(glyph.cluster).unwrap_or(0);
        let to = glyphs.get(end).map_or(run.text().end, |next| {
            usize::try_from(next.cluster).unwrap_or(from)
        });
        // The first glyph the shaper did not insert takes it: a dotted
        // circle in front of its mark is drawn, and typed by no one. A
        // cluster of nothing but inserted glyphs gives it to its first, so
        // no text a run stands for is lost.
        let taker = (start..end)
            .find(|at| glyphs.get(*at).is_some_and(|glyph| !glyph.inserted))
            .unwrap_or(start);
        if let Some(slot) = out.get_mut(taker) {
            *slot = text.get(from..to.max(from)).unwrap_or("");
        }
        start = end.max(start.saturating_add(1));
    }
    out
}

/// The text each glyph of one shaped run stands for, and where it goes.
///
/// `text` is the paragraph the run was shaped from, because a cluster is an
/// offset into *that* and not into the run's own slice.
///
/// The run is walked in **logical** order, which is the order its clusters are
/// monotonic in and the order `/ToUnicode` needs. A right-to-left run is not
/// reversed here: reversing is per line and after breaking, and a caller that
/// has already broken hands the reversed run in.
#[must_use]
pub fn place(text: &str, run: &ShapedRun, size: f64) -> Vec<Placed> {
    let units = f64::from(run.units_per_em().max(1));
    let scale = |value: i32| f64::from(value) * size / units;
    let glyphs = run.glyphs();
    let texts = cluster_texts(text, run);
    let mut out = Vec::with_capacity(glyphs.len());
    let mut pen = 0.0f64;
    for (at, glyph) in glyphs.iter().enumerate() {
        out.push(Placed {
            id: glyph.glyph,
            text: texts.get(at).copied().unwrap_or("").to_string(),
            x: pen + scale(glyph.x_offset),
            rise: scale(glyph.y_offset),
            inserted: glyph.inserted,
        });
        pen += scale(glyph.x_advance);
    }
    out
}

/// What to shape, against what, and where to put it.
///
/// A struct rather than six positional arguments, for the reason the fixture
/// face builder gives for the same choice: two of them are `f64`s in different
/// spaces and a caller that swapped them would still compile.
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    /// The resource name of a font already registered with
    /// [`DocumentBuilder::add_cid_font`].
    pub font: &'a [u8],
    /// A shaper over the same program that font was registered with — a
    /// different one would name glyph indices the embedded face does not
    /// have — made by the caller, with the face's
    /// [`tinker_pdf_shape::Plans`] where it keeps them, so the runs of one
    /// document build in one face share their plans and indexes.
    pub shaper: Shaper<'a>,
    /// The font size, in points.
    pub size: f64,
    /// 9.4.2's text matrix, which puts the run on the page.
    pub matrix: [f64; 6],
    /// The text to shape.
    pub text: &'a str,
    /// The paragraph direction UAX #9 should resolve from.
    pub direction: BaseDirection,
    /// The colour face [`Run::font`] was registered as, through
    /// [`ColourFace::register`], or `None` for a composite font
    /// `DocumentBuilder::add_cid_font` registered.
    ///
    /// A run in a colour face is written as the face's Type 3 glyphs
    /// (`docs/design/colour-fonts.md`): every glyph it draws gets its
    /// procedure from the face first, and the run goes through
    /// `DocumentBuilder::colour_glyph_run`. The shaper is the same either
    /// way — a colour glyph's advances are its outline's (`hmtx`).
    pub colour: Option<&'a ColourFace<'a>>,
}

/// Shapes a run and writes it as one composite-font text object, **as one
/// line in visual order** — or, for a run in a colour face, as one text
/// object of the face's Type 3 glyphs ([`Run::colour`]).
///
/// The whole of milestone 7 in one call, for a caller that has a face, a
/// string and a content stream.
///
/// The text is one line, so UAX #9's rule L2 is applied here: the bidi runs
/// are drawn in the order L2 gives their levels and a right-to-left run's
/// glyphs from its last cluster to its first, which is what
/// `epub/paint.rs`'s `shaped_glyphs` does for a line of a book. Until ruling
/// 14 this wrote every run in logical order from left to right — a
/// right-to-left word drawn backwards on the page — and the milestone's round
/// trip passed *because* extraction read content-stream order; with
/// extraction in logical order (`docs/rulings.md`), a round trip only holds
/// for a page that is drawn right.
///
/// Returns false, having written nothing, for the reasons
/// [`DocumentBuilder::glyph_run`] returns false — and for a colour run,
/// `DocumentBuilder::colour_glyph_run` — and for text that shaped to no
/// glyphs at all.
pub fn write_run(builder: &mut DocumentBuilder, out: &mut Vec<u8>, run: &Run<'_>) -> bool {
    let &Run {
        font,
        shaper,
        size,
        matrix,
        text,
        direction,
        colour,
    } = run;
    let paragraph = Paragraph::new(text, direction);
    let items = itemize(text, &paragraph);
    let levels: Vec<_> = items.iter().map(|item| item.level).collect();
    let mut placed: Vec<Placed> = Vec::new();
    // One pen across the whole line, advancing in **draw** order: L2 over the
    // runs, and a backward run's glyphs walked from the end. A glyph's text is
    // still worked out in logical order by `place`, because that is the order
    // clusters are monotonic in; only the drawing is reversed.
    let mut pen = 0.0f64;
    for index in reorder(&levels) {
        let Some(item) = items.get(index) else {
            continue;
        };
        let run = shaper.shape(text, item);
        let units = f64::from(run.units_per_em().max(1));
        let scale = |value: i32| f64::from(value) * size / units;
        let logical = place(text, &run, size);
        let glyphs = run.glyphs();
        let order: Vec<usize> = if run.direction().is_forward() {
            (0..glyphs.len()).collect()
        } else {
            (0..glyphs.len()).rev().collect()
        };
        for at in order {
            let (Some(glyph), Some(slot)) = (glyphs.get(at), logical.get(at)) else {
                continue;
            };
            placed.push(Placed {
                id: slot.id,
                text: slot.text.clone(),
                x: pen + scale(glyph.x_offset),
                rise: slot.rise,
                inserted: slot.inserted,
            });
            pen += scale(glyph.x_advance);
        }
    }
    if placed.is_empty() {
        return false;
    }
    if let Some(face) = colour {
        // Every glyph's procedure first, so the run names none the builder
        // lacks; a run refused for one writes nothing.
        if !face.add_glyphs(builder, font, placed.iter().map(|glyph| glyph.id)) {
            return false;
        }
    }
    draw(builder, out, font, size, matrix, &placed, colour.is_some())
}

/// Writes `placed` — one line's glyphs, in drawing order — as a composite
/// font's text through [`DocumentBuilder::glyph_run`], or, `colour`, a colour
/// face's through `DocumentBuilder::colour_glyph_run`, its glyphs' procedures
/// already added.
///
/// # A glyph the shaper inserted is drawn inside a span that states nothing
///
/// A dotted circle stands for no text, and `/ToUnicode` cannot say so: a code
/// it does not map is one a reader may read as a character of its own
/// choosing (ISO 32000-2 9.10.2), and a code it maps — because the document
/// also draws a typed U+25CC in the face — reads as that character. So each
/// stretch of glyphs the shaper inserted is written inside
/// `/Span <</ActualText ()>> BDC … EMC`, a span whose replacement text is
/// empty (14.9.4), and a reader reads nothing where the circle is drawn. The
/// glyphs around it are written as before, each stretch its own text object
/// at the same matrix, so every glyph is where the line put it.
///
/// A line with no inserted glyph is one call, written byte for byte as it
/// was. Returns false, having written nothing, for the reasons the call
/// returns false, the checks made before any stretch is written.
pub(crate) fn draw(
    builder: &mut DocumentBuilder,
    out: &mut Vec<u8>,
    font: &[u8],
    size: f64,
    matrix: [f64; 6],
    placed: &[Placed],
    colour: bool,
) -> bool {
    let run = |builder: &mut DocumentBuilder, out: &mut Vec<u8>, glyphs: &[Placed]| {
        let borrowed: Vec<PlacedGlyph<'_>> = glyphs.iter().map(Placed::as_glyph).collect();
        if colour {
            builder.colour_glyph_run(out, font, size, matrix, &borrowed)
        } else {
            builder.glyph_run(out, font, size, matrix, &borrowed)
        }
    };
    if !placed.iter().any(|glyph| glyph.inserted) {
        return run(builder, out, placed);
    }
    // What the call refuses for the whole line, refused before any stretch
    // is written, so a refusal writes nothing.
    let drawable = size.is_finite()
        && size > 0.0
        && matrix.iter().all(|value| value.is_finite())
        && placed
            .iter()
            .all(|glyph| glyph.x.is_finite() && glyph.rise.is_finite());
    if !drawable {
        return false;
    }
    let mut written = Vec::new();
    for stretch in placed.chunk_by(|one, next| one.inserted == next.inserted) {
        let silent = stretch.first().is_some_and(|glyph| glyph.inserted);
        if silent {
            written.extend_from_slice(b"/Span <</ActualText ()>> BDC\n");
        }
        if !run(builder, &mut written, stretch) {
            return false;
        }
        if silent {
            written.extend_from_slice(b"EMC\n");
        }
    }
    out.extend_from_slice(&written);
    true
}
