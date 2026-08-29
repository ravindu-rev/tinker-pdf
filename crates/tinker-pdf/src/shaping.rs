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
//! Two rules turn that into `/ToUnicode`, and the second is the one that is
//! easy to get wrong:
//!
//! - **A glyph stands for the text from its own cluster to the next
//!   *different* one.** A ligature's cluster is the smallest of the glyphs it
//!   replaced, so `ffi` comes back whole.
//! - **Where several glyphs share a cluster, the first takes the text and the
//!   rest take nothing.** A decomposition — one character that became a base
//!   and two marks — would otherwise put the character into the extracted text
//!   three times, and a reader would find `naïve` spelled `naïïïve`.
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

use tinker_pdf_cos::build::{DocumentBuilder, Glyph, PlacedGlyph};
use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::BaseDirection;
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
/// The two rules in this module's header, in one place. It is one place on
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
    let mut out = Vec::with_capacity(glyphs.len());
    for (at, glyph) in glyphs.iter().enumerate() {
        // A glyph stands for the text from its own cluster to the next
        // *different* one — a ligature's cluster is the smallest of the glyphs
        // it replaced, so `ffi` comes back whole — and nothing at all if an
        // earlier glyph of the same cluster already took it, which is what
        // stops a decomposed `ï` from extracting as three characters.
        let first = at == 0 || glyphs[at.saturating_sub(1)].cluster != glyph.cluster;
        let from = usize::try_from(glyph.cluster).unwrap_or(0);
        let to = glyphs[at.saturating_add(1)..]
            .iter()
            .find(|next| next.cluster != glyph.cluster)
            .map_or(run.text().end, |next| {
                usize::try_from(next.cluster).unwrap_or(from)
            });
        out.push(if first {
            text.get(from..to.max(from)).unwrap_or("")
        } else {
            ""
        });
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
    /// The same program that font was registered with. A different one would
    /// name glyph indices the embedded face does not have.
    pub face: &'a Sfnt<'a>,
    /// The font size, in points.
    pub size: f64,
    /// 9.4.2's text matrix, which puts the run on the page.
    pub matrix: [f64; 6],
    /// The text to shape.
    pub text: &'a str,
    /// The paragraph direction UAX #9 should resolve from.
    pub direction: BaseDirection,
}

/// Shapes a run and writes it as one composite-font text object.
///
/// The whole of milestone 7 in one call, for a caller that has a face, a
/// string and a content stream.
///
/// Returns false, having written nothing, for the reasons
/// [`DocumentBuilder::glyph_run`] returns false, and for text that shaped to no
/// glyphs at all.
pub fn write_run(builder: &mut DocumentBuilder, out: &mut Vec<u8>, run: &Run<'_>) -> bool {
    let &Run {
        font,
        face,
        size,
        matrix,
        text,
        direction,
    } = run;
    let shaper = Shaper::new(face);
    let (_, runs) = shaper.shape_text(text, direction);
    let mut placed: Vec<Placed> = Vec::new();
    for run in &runs {
        placed.extend(place(text, run, size));
    }
    // Each run's positions are its own, so the runs are laid end to end: a
    // paragraph of mixed script is one pen, not one per script.
    let mut pen = 0.0f64;
    let mut at = 0usize;
    for run in &runs {
        let width = f64::from(run.advance()) * size / f64::from(run.units_per_em().max(1));
        for slot in placed.iter_mut().skip(at).take(run.glyphs().len()) {
            slot.x += pen;
        }
        at += run.glyphs().len();
        pen += width;
    }
    if placed.is_empty() {
        return false;
    }
    let borrowed: Vec<PlacedGlyph<'_>> = placed.iter().map(Placed::as_glyph).collect();
    builder.glyph_run(out, font, size, matrix, &borrowed)
}
