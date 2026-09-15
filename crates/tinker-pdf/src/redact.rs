//! Redaction (phase 10).
//!
//! The whole point is that the content is **gone**, not covered. A black
//! rectangle drawn over text leaves the text in the content stream, where any
//! extractor finds it — that is how redaction failures reach the news.
//!
//! So this rewrites the content stream itself: every text-showing operator
//! whose glyphs fall inside a redaction rectangle has those glyphs removed and
//! is re-emitted with a displacement in their place, so the surviving text on
//! the line keeps its position.
//!
//! It lives in the facade rather than in `cos` because deciding *which* glyphs
//! a rectangle covers needs both the content tokenizer and the font metrics,
//! and the leaf crates do not depend on each other (ruling 8).
//!
//! The acceptance test is not "does it look right" but "decompress every
//! stream in the output and assert the needle bytes are absent" — and, since
//! September 2026, "render the redacted page and assert there is no ink inside
//! the rectangle". A stream check alone would pass a build that left the
//! glyph in an untouched duplicate stream; an ink check alone would pass the
//! black-rectangle non-redaction this module exists to refuse. Neither half
//! is the property. Both together are.
//!
//! # The cut happens in the run's own frame
//!
//! A redaction rectangle is given in page space. A glyph is placed in *text*
//! space, by the text matrix and the transformation matrix in force, and
//! those two carry rotation and skew as readily as they carry a translation.
//! Until September 2026 this module modelled both as `(sx, sy, tx, ty)` and
//! refused any run whose `b` or `c` was non-zero, because a pen that walks
//! along one axis cannot follow a run that does not.
//!
//! Both matrices are now carried whole ([`Matrix`]), which makes three things
//! true at once:
//!
//! - **Coverage is exact.** A glyph's box is a parallelogram in page space,
//!   and whether it meets the rectangle is a separating-axis test
//!   ([`quad_meets_rect`]) rather than a comparison of two intervals.
//! - **The replacement displacement needs no frame of its own.** A `TJ`
//!   number displaces the pen along the *baseline* — 9.4.3 applies it before
//!   the text matrix — so the gap a removed glyph leaves rotates with the run
//!   for free. That is what "cut along its own baseline" turns out to mean in
//!   a content stream: not new geometry, but the one displacement operator
//!   that was already defined in the run's own frame.
//! - **Nothing is re-anchored.** The surviving sub-runs keep the original
//!   run's text matrix, untouched, byte for byte. See [`emit_array`] for why
//!   the alternative — a fresh `Tm` per sub-run — is the plausible-looking
//!   wrong answer.
//!
//! # What still refuses, and why that is not a formality
//!
//! A redaction that silently fails to redact is worse than one that refuses:
//! the caller believes the content is gone and distributes the file. So a run
//! this module cannot *measure* is left whole and named in
//! [`RedactionReport::warnings`], rather than cut from positions that are
//! approximately right. Four classes qualify, and two of them were being cut
//! wrongly before this file carried a matrix — the rotation refusal was
//! guarding the axis it knew about and none of the others:
//!
//! | Class | Why it cannot be measured |
//! | --- | --- |
//! | [`RedactionWarning::VerticalRun`] | 9.4.4's vertical branch advances by `w1` **down** the page and takes its metrics from `/W2`; a `TJ` number displaces vertically too. Every one of those is a different formula, not a different matrix |
//! | [`RedactionWarning::RescaledType3Font`] | 9.6.5: a Type 3 font's `/Widths` are in *its own* glyph space, which `/FontMatrix` maps to text space. `Font::width_of` hands them back raw and this module divides by 1000, which is right for the 1/1000 default and wrong by exactly the matrix for anything else |
//! | [`RedactionWarning::UnknownFont`] | no metrics at all: the `Tf` named a font the resource dictionary in scope does not have |
//! | [`RedactionWarning::UnmeasurableFrame`] | a non-finite entry in the text or transformation matrix, or a position that has run away to infinity |
//!
//! A warning says the run was **not measured**, not that it was covered: this
//! module cannot know whether an unmeasurable run fell under a rectangle,
//! which is the whole reason it will not cut one. That is why warnings are
//! raised only when there is at least one rectangle to fall under.
//!
//! # One under-redaction this module does *not* name
//!
//! A form XObject drawn twice is rewritten once, so only its first placement
//! is ever measured against the rectangles. The `visited` set in [`follow`] is
//! there to stop a self-referential form recursing forever, and it stops the
//! second `Do` as well — right when both invocations share a transform,
//! wrong when they do not. What that case reports is `glyphs: 0` with no
//! warning, which is exactly what a rectangle covering nothing reports.
//!
//! It is pinned by `a_form_drawn_twice_is_cut_only_at_its_first_placement` and
//! carries a roadmap row of its own. It is written here rather than left in
//! the test because somebody reading this module to decide whether to trust it
//! should not have to go and find it.
//!
//! # The injections that were counted
//!
//! Each defect below was reintroduced on its own and
//! `cargo test --no-fail-fast -p tinker-pdf` run over the result, on
//! 15 September 2026, against a suite of 1 508 tests. The flag matters: a
//! plain `cargo test` stops at the first failure and undercounts. Every one
//! of them is caught, and the two caught by exactly one test are caught by the
//! test that was written for them.
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the rotation ignored, so the cut is axis-aligned | 9 |
//! | the cut applied in page space rather than in the run's own frame | 21 |
//! | a partly covered glyph kept, coverage requiring containment | 13 |
//! | the replacement displacement dropped | 5 |
//! | a fresh `Tm` per cut run, reconstructed and rounded | **1** |
//! | the refusal removed for a vertical run | 2 |
//! | the refusal removed for a rescaled Type 3 glyph space | 2 |
//! | the warning not emitted when a run is skipped | 6 |
//! | the non-showing half of `'` and `"` dropped from a cut run | 2 |
//! | an existing `TJ` adjustment re-emitted with its sign flipped | **1** |

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tinker_pdf_content::{Token, Tokenizer};
use tinker_pdf_cos::{
    font as cos_font, pages as cos_pages, CosDocument, Dict, DocumentEditor, Font, Name, ObjRef,
    Object, Rect, StreamData,
};

/// What a redaction covers and how it is marked.
#[derive(Clone, Copy, Debug)]
pub struct Redaction {
    /// The area to clear, in unrotated page space.
    pub area: Rect,
    /// Whether to paint the area black after clearing it.
    ///
    /// Cosmetic: the content is already gone by the time this draws. Its value
    /// is telling a reader that something was removed, rather than leaving a
    /// gap that reads as though nothing was ever there.
    pub mark: bool,
}

/// A run redaction left whole because it could not measure it (ruling 10).
///
/// Every variant names the resource name of the font in force and how many
/// bytes of showing operand were left in place, because "a run was skipped"
/// with neither is a sentence a caller cannot act on — and this is the
/// leniency that is invisible from outside, since what it costs is content
/// the caller believes was removed.
///
/// `bytes` rather than glyphs: a run whose font is unknown cannot be decoded
/// into glyphs at all, and a count that is a guess for one variant and a
/// measurement for the other three is a count nobody can compare. Warnings
/// with the same cause and the same font are merged, so a page of vertical
/// text yields one entry per font rather than one per operator.
///
/// Closed rather than `#[non_exhaustive]`, for `WarningKind`'s reason: a new
/// class of run this module will not measure is a deliberate change to
/// documented behaviour, and a caller matching exhaustively should be made to
/// notice it rather than fall through an arm that says "some other reason".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedactionWarning {
    /// The `Tf` in force named a font that the resource dictionary in scope
    /// does not contain, so the run has no metrics and no glyph can be
    /// placed. `font` is empty when no `Tf` preceded the showing operator.
    UnknownFont {
        /// The resource name the `Tf` named.
        font: Vec<u8>,
        /// How many bytes of showing operand were left in place.
        bytes: usize,
    },
    /// The font's writing mode is vertical — 9.7.5's `/WMode` in the encoding
    /// CMap, which is why only a composite font can have one.
    ///
    /// 9.4.4 then computes `ty` from the glyph's *vertical* displacement `w1`
    /// instead of `tx` from `w0`, and the vertical formula has no horizontal
    /// scale in it at all, where the horizontal one ends in `× Th`. 9.7.4.3
    /// puts `w1` in `/W2` and `/DW2`, not in the `/W` and `/DW` this module
    /// reads. A different formula over different entries, rather than a
    /// different matrix, and one this module does not implement.
    ///
    /// This was being cut *horizontally* until September 2026: the rotation
    /// refusal it hid behind looked at the matrix, and a vertical run's
    /// matrix is perfectly ordinary.
    VerticalRun {
        /// The resource name of the font.
        font: Vec<u8>,
        /// How many bytes of showing operand were left in place.
        bytes: usize,
    },
    /// 9.6.5: a Type 3 font whose `/FontMatrix` is not the 1/1000 default.
    ///
    /// `Font::width_of` returns `/Widths` as written, in the font's own glyph
    /// space, and this module turns that into text space by dividing by 1000
    /// — which is the `/FontMatrix` for every other font kind and for the
    /// Type 3 fonts that use the conventional one. A font that picks a
    /// different glyph space has every advance wrong by exactly that matrix,
    /// so every position after the first glyph is wrong and the rectangle
    /// cuts the wrong text.
    ///
    /// An absent `/FontMatrix` is read as the default rather than as a
    /// refusal: 9.6.5 requires the entry, so a font without one is malformed,
    /// and the conventional reading of a malformed one is what every
    /// consumer does.
    RescaledType3Font {
        /// The resource name of the font.
        font: Vec<u8>,
        /// How many bytes of showing operand were left in place.
        bytes: usize,
    },
    /// The text rendering matrix carries a non-finite entry, or the pen has
    /// run away to a position that is no longer a number. Nothing can be
    /// measured against a rectangle from there.
    ///
    /// The whole showing operand is left, never half of it: a run this module
    /// abandons part-way through would be a run cut from positions it had
    /// already decided it could not trust.
    UnmeasurableFrame {
        /// The resource name of the font.
        font: Vec<u8>,
        /// How many bytes of showing operand were left in place.
        bytes: usize,
    },
}

impl RedactionWarning {
    /// The resource name of the font the run was showing in.
    #[must_use]
    pub fn font(&self) -> &[u8] {
        match self {
            RedactionWarning::UnknownFont { font, .. }
            | RedactionWarning::VerticalRun { font, .. }
            | RedactionWarning::RescaledType3Font { font, .. }
            | RedactionWarning::UnmeasurableFrame { font, .. } => font,
        }
    }

    /// How many bytes of showing operand this warning accounts for.
    #[must_use]
    pub fn bytes(&self) -> usize {
        match self {
            RedactionWarning::UnknownFont { bytes, .. }
            | RedactionWarning::VerticalRun { bytes, .. }
            | RedactionWarning::RescaledType3Font { bytes, .. }
            | RedactionWarning::UnmeasurableFrame { bytes, .. } => *bytes,
        }
    }

    /// Whether two warnings are the same cause in the same font.
    fn same_cause(&self, other: &RedactionWarning) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other) && self.font() == other.font()
    }

    /// Adds another run's operand length to this warning.
    fn absorb(&mut self, more: usize) {
        match self {
            RedactionWarning::UnknownFont { bytes, .. }
            | RedactionWarning::VerticalRun { bytes, .. }
            | RedactionWarning::RescaledType3Font { bytes, .. }
            | RedactionWarning::UnmeasurableFrame { bytes, .. } => {
                *bytes = bytes.saturating_add(more);
            }
        }
    }
}

/// How many distinct warnings one redaction keeps.
///
/// Four causes times the fonts on a page: a document that reaches this cap has
/// a resource dictionary a caller is not going to read through anyway, and the
/// bytes of the ones past it are lost rather than the list growing with the
/// file (ruling 1).
const MAX_WARNINGS: usize = 64;

/// Records a warning, merging it into one with the same cause and font.
fn note(warnings: &mut Vec<RedactionWarning>, warning: RedactionWarning) {
    for existing in warnings.iter_mut() {
        if existing.same_cause(&warning) {
            existing.absorb(warning.bytes());
            return;
        }
    }
    if warnings.len() < MAX_WARNINGS {
        warnings.push(warning);
    }
}

/// What a redaction did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RedactionReport {
    /// How many text-showing operations had glyphs removed.
    pub operations: usize,
    /// How many glyphs were removed in total.
    pub glyphs: usize,
    /// How many images were scrubbed.
    ///
    /// An image goes whole when a redaction touches it at all: cutting a hole
    /// would mean decoding, editing and re-encoding its samples through a
    /// codec this build may have no encoder for, and leaving the rest is not a
    /// redaction.
    pub images: usize,
    /// Runs this redaction could not measure and therefore left whole.
    ///
    /// Empty is the answer a caller wants. A non-empty list means some text on
    /// the page was never tested against the rectangles at all, so whether the
    /// redaction is complete is not something this report can say.
    pub warnings: Vec<RedactionWarning>,
}

/// A two-dimensional affine transform, `[a b c d e f]` as PDF writes one.
///
/// 8.3.4's row-vector convention: a point `(x, y)` is `[x y 1]`, so it maps to
/// `(a·x + c·y + e, b·x + d·y + f)`, and `m.then(n)` is the matrix that
/// applies `m` and then `n`. That is the order every concatenation in this
/// file needs: 8.3.4 says a new transformation is **premultiplied** with the
/// one in force, which is what `cm` (8.4.4) and a form's `/Matrix` (8.10.1)
/// do, and 9.4.4 composes the text rendering matrix
/// `[Tfs·Th 0 0; 0 Tfs 0; 0 Trise 1] × Tm × CTM` the same way.
///
/// 8.3.4 and not 8.3.3: 8.3.3 is "Common transformations", which gives the
/// four named matrices, and 8.3.4 is "Transformation matrices", which is where
/// the vector form and the multiplication order are. The numbering is the same
/// in ISO 32000-1:2008 and ISO 32000-2:2020.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A pure translation, which is what `Td` and `T*` concatenate (9.4.2).
    fn translate(tx: f64, ty: f64) -> Matrix {
        Matrix {
            e: tx,
            f: ty,
            ..Matrix::IDENTITY
        }
    }

    /// Six numbers as an operator wrote them.
    fn from_operands(v: &[f64]) -> Option<Matrix> {
        match v {
            [a, b, c, d, e, f, ..] => Some(Matrix {
                a: *a,
                b: *b,
                c: *c,
                d: *d,
                e: *e,
                f: *f,
            }),
            _ => None,
        }
    }

    /// `self`, then `other`.
    fn then(self, other: Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Where a point lands.
    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Whether every entry is a number.
    fn is_finite(self) -> bool {
        self.a.is_finite()
            && self.b.is_finite()
            && self.c.is_finite()
            && self.d.is_finite()
            && self.e.is_finite()
            && self.f.is_finite()
    }

    /// Whether the transform rotates or skews.
    fn is_turned(self) -> bool {
        self.b.abs() > 1e-9 || self.c.abs() > 1e-9
    }
}

/// Applies redactions to one page of an open editor.
///
/// Returns what was removed, or `None` when the page does not exist. Applying
/// an empty list, or one that covers no text, is a no-op that still reports
/// success — a redaction that finds nothing is not an error.
pub fn apply(
    editor: &mut DocumentEditor,
    page: u32,
    areas: &[Redaction],
) -> Option<RedactionReport> {
    let reference = editor.page_refs().get(page as usize).copied()?;

    let (content, existing, fonts) = {
        let doc = editor.document();
        let collected = cos_pages::collect(doc);
        let info = collected.get(page as usize)?;
        let content = cos_pages::content_bytes(doc, info);
        let existing = cos_pages::contents(doc, info);
        let resources = page_resources(doc, reference).unwrap_or_default();
        (content, existing, fonts_in(doc, &resources))
    };

    let (mut data, mut report, uses) = rewrite(&content, areas, &fonts, Matrix::IDENTITY);

    // 8.10: a form XObject holds content like any other, and a redaction that
    // stops at the page stream leaves whatever a form drew exactly where it
    // was. Images the redaction covers are scrubbed for the same reason: a
    // black rectangle over a photograph removes nothing.
    let mut visited: HashSet<u32> = HashSet::new();
    // The resources of the page being redacted, not of page zero.
    let resources = page_resources(editor.document(), reference).unwrap_or_default();
    follow(
        editor,
        &resources,
        &uses,
        areas,
        &mut report,
        &mut visited,
        0,
    );

    if areas.iter().any(|r| r.mark) {
        // Painted last, so it covers whatever remains beneath it.
        data.extend_from_slice(b"q 0 g\n");
        for area in areas.iter().filter(|r| r.mark).map(|r| r.area) {
            let line = format!(
                "{} {} {} {} re f\n",
                area.x0,
                area.y0,
                area.x1 - area.x0,
                area.y1 - area.y0
            );
            data.extend_from_slice(line.as_bytes());
        }
        data.extend_from_slice(b"Q\n");
    }

    // The redacted stream **overwrites** the object the old one lived in, and
    // any further parts of a split `/Contents` array are deleted outright.
    //
    // Writing to a freshly allocated object instead would leave the original
    // bytes sitting in the file, unreferenced but perfectly readable — which
    // is the exact failure this whole module exists to prevent, and which the
    // decompress-every-stream test caught when it was written that way.
    let content_ref = match existing.split_first() {
        Some((first, rest)) => {
            for &stale in rest {
                editor.delete(stale);
            }
            *first
        }
        None => editor.allocate(),
    };

    editor.put_stream(
        content_ref,
        StreamData {
            dict: Dict::new(),
            data,
        },
    );

    let Some(Object::Dict(mut dict)) = editor.get(reference) else {
        return None;
    };
    let contents = editor.intern(b"Contents");
    dict.insert(contents, Object::Ref(content_ref));
    editor.put(reference, Object::Dict(dict));

    Some(report)
}

fn page_resources(doc: &CosDocument, page: ObjRef) -> Option<Dict> {
    let object = doc.get(page).ok()?;
    let dict = object.as_dict()?;
    doc.resolve_key(dict, Name::RESOURCES).as_dict().cloned()
}

/// A font in scope, and what this module knows about measuring it.
struct RunFont {
    font: Arc<Font>,
    /// 9.6.5: a Type 3 font whose `/FontMatrix` is not the 1/1000 default, so
    /// its `/Widths` are in a glyph space this module's `width / 1000` does
    /// not map out of.
    rescaled_type3: bool,
}

/// The fonts one resource dictionary puts in scope, by the raw name bytes.
///
/// Re-keyed by the bytes rather than by an interned [`Name`] because the
/// rewrite matches against what the `Tf` operator literally says, and it has
/// no document to intern with.
fn fonts_in(doc: &CosDocument, resources: &Dict) -> HashMap<Vec<u8>, Arc<RunFont>> {
    let default_matrix = type3_matrices(doc, resources);
    cos_font::from_resources(doc, resources)
        .into_iter()
        .filter_map(|(name, font)| {
            let bytes = doc.name_bytes(name)?.to_vec();
            let rescaled_type3 = font.kind() == cos_font::FontKind::Type3
                && !default_matrix.get(&name).copied().unwrap_or(true);
            Some((
                bytes,
                Arc::new(RunFont {
                    font,
                    rescaled_type3,
                }),
            ))
        })
        .collect()
}

/// Whether each font in `/Font` carries the conventional 1/1000 `/FontMatrix`.
///
/// Read here rather than through `cos_font::Font`, which does not carry the
/// entry: this module is the only caller that needs it, and it needs it only
/// to decide whether to refuse. A font with no `/FontMatrix` at all answers
/// `true` — see [`RedactionWarning::RescaledType3Font`].
fn type3_matrices(doc: &CosDocument, resources: &Dict) -> HashMap<Name, bool> {
    let mut out = HashMap::new();
    let value = doc.resolve_key(resources, doc.intern(b"Font"));
    let Some(fonts) = value.as_dict() else {
        return out;
    };

    for (key, entry) in fonts.iter() {
        let resolved = doc.resolve(entry);
        let Some(dict) = resolved.as_dict() else {
            continue;
        };
        let matrix = doc.resolve_key(dict, doc.intern(b"FontMatrix"));
        let default = match matrix.as_array() {
            None => true,
            Some(array) => {
                let v = array
                    .iter()
                    .filter_map(Object::as_number)
                    .collect::<Vec<f64>>();
                let wanted = [0.001, 0.0, 0.0, 0.001, 0.0, 0.0];
                v.len() == 6
                    && v.iter()
                        .zip(wanted)
                        .all(|(got, want)| (got - want).abs() < 1e-12)
            }
        };
        out.insert(*key, default);
    }
    out
}

/// How deep form XObjects may nest before recursion is refused (8.10).
const MAX_FORM_DEPTH: u32 = 12;

/// Recurses into the XObjects a stream invoked.
///
/// A form is rewritten the way the page was, with the transform in force at
/// the `Do` as its starting one — the rectangles stay in page space, so the
/// form's own coordinates are brought into it rather than the other way round.
/// An image the redaction covers is scrubbed.
///
/// `visited` stops a form that invokes itself, directly or through another,
/// from recursing forever. It also means a form used twice is rewritten once,
/// which is correct only while both invocations share a transform: the first
/// pass removed the glyphs that were under a rectangle *at the first
/// placement*, and a form drawn somewhere else as well has its second
/// placement measured against nothing at all. See the module header.
#[allow(clippy::too_many_arguments)]
fn follow(
    editor: &mut DocumentEditor,
    resources: &Dict,
    uses: &[XObjectUse],
    areas: &[Redaction],
    report: &mut RedactionReport,
    visited: &mut HashSet<u32>,
    depth: u32,
) {
    if depth > MAX_FORM_DEPTH {
        return;
    }

    for used in uses {
        let Some((reference, dict)) = resolve_xobject(editor, resources, &used.name) else {
            continue;
        };
        if !visited.insert(reference.num) {
            continue;
        }

        let doc = editor.document();
        let subtype = doc
            .resolve_key(&dict, doc.intern(b"Subtype"))
            .as_name()
            .and_then(|n| doc.name_bytes(n))
            .map(|b| b.to_vec());

        match subtype.as_deref() {
            Some(b"Image") => {
                if covers_unit_square(used, areas) {
                    scrub_image(editor, reference, &dict);
                    report.images += 1;
                }
            }
            Some(b"Form") => {
                // 8.10.2: the form's own /Matrix sits between its space and the
                // one that invoked it, so it composes with the transform the
                // `Do` was made under.
                let matrix = doc
                    .resolve_key(&dict, doc.intern(b"Matrix"))
                    .as_array()
                    .map(|a| a.iter().filter_map(Object::as_number).collect::<Vec<f64>>())
                    .filter(|v| v.len() >= 6 && v.iter().all(|x| x.is_finite()))
                    .and_then(|v| Matrix::from_operands(&v));

                let inner = match matrix {
                    Some(m) => m.then(used.ctm),
                    None => used.ctm,
                };

                let Ok(content) = doc.stream_decoded(reference) else {
                    continue;
                };
                // 8.10.1: a form's own `/Resources` is what its content
                // names things in. A form that omits the dictionary inherits
                // the scope that invoked it, which is why the fallback is the
                // caller's rather than the page's.
                let inner_resources = doc
                    .resolve_key(&dict, Name::RESOURCES)
                    .as_dict()
                    .cloned()
                    .unwrap_or_else(|| resources.clone());
                let fonts = fonts_in(doc, &inner_resources);

                let (data, inner_report, inner_uses) = rewrite(&content, areas, &fonts, inner);
                report.operations += inner_report.operations;
                report.glyphs += inner_report.glyphs;
                report.images += inner_report.images;
                for warning in inner_report.warnings {
                    note(&mut report.warnings, warning);
                }

                // Overwritten in place, for the same reason the page's content
                // is: a freshly allocated object leaves the original text in
                // the file, unreferenced and perfectly readable.
                editor.put_stream(reference, StreamData { dict, data });
                follow(
                    editor,
                    &inner_resources,
                    &inner_uses,
                    areas,
                    report,
                    visited,
                    depth + 1,
                );
            }
            _ => {}
        }
    }
}

/// The reference and dictionary a resource name selects from `/XObject`.
///
/// The caller supplies the resource dictionary that is *in scope*, which is
/// the whole correctness condition here. This used to look the name up in
/// `page_refs().first()` — page zero — whatever page was being redacted and
/// however deep in a form the name appeared.
///
/// Two ways that failed, and the second is the bad one:
///
/// - Redacting page one found nothing and reported nothing, leaving the
///   content it was asked to remove in the file.
/// - Redacting page one when page zero happened to have a resource of the
///   same name — `/Im0` is the commonest name there is — scrubbed *page
///   zero's* image and left page one's. It reported `images: 1`, so it looked
///   like it had worked.
fn resolve_xobject(
    editor: &DocumentEditor,
    resources: &Dict,
    name: &[u8],
) -> Option<(ObjRef, Dict)> {
    let doc = editor.document();
    let table = doc.resolve_key(resources, doc.intern(b"XObject"));
    let reference = table.as_dict()?.get_ref(doc.intern(name))?;
    let object = doc.get(reference).ok()?;
    Some((reference, object.as_dict()?.clone()))
}

/// Whether a redaction covers the unit square an XObject was drawn into.
///
/// 8.9.5.2: an image occupies the unit square of the transform in force. A
/// rotated or skewed transform makes that square something four numbers cannot
/// describe, and rather than guess at its extent the image is treated as
/// covered — over-removing a rotated image is the safe direction here, and it
/// is rare enough to be worth the bluntness.
///
/// [`quad_meets_rect`] could now answer this exactly, since the transform is
/// carried whole. It deliberately is not asked: making a rotated image's
/// coverage exact changes which images survive a redaction, which is a
/// decision about image content and belongs to the roadmap row that owns it
/// rather than to the one about text runs.
fn covers_unit_square(used: &XObjectUse, areas: &[Redaction]) -> bool {
    if used.ctm.is_turned() {
        return !areas.is_empty();
    }
    let x0 = used.ctm.e.min(used.ctm.e + used.ctm.a);
    let x1 = used.ctm.e.max(used.ctm.e + used.ctm.a);
    let y0 = used.ctm.f.min(used.ctm.f + used.ctm.d);
    let y1 = used.ctm.f.max(used.ctm.f + used.ctm.d);

    areas.iter().any(|redaction| {
        let a = redaction.area;
        x1 > a.x0 && x0 < a.x1 && y1 > a.y0 && y0 < a.y1
    })
}

/// Replaces an image with a single blank sample.
///
/// The samples are what has to go, so the stream is rewritten rather than
/// covered: a rectangle painted over a photograph removes nothing, and the
/// original bytes stay in the file for anyone who decompresses it.
///
/// The whole image goes even when the rectangle covers only part of it.
/// Cutting a hole would mean decoding the samples, editing them and
/// re-encoding through a codec this build may have no encoder for, and leaving
/// the rest is not a redaction.
fn scrub_image(editor: &mut DocumentEditor, reference: ObjRef, dict: &Dict) {
    let is_mask = dict
        .get(editor.intern(b"ImageMask"))
        .and_then(Object::as_bool)
        .unwrap_or(false);

    let mut replacement = Dict::new();
    replacement.insert(Name::TYPE, Object::Name(editor.intern(b"XObject")));
    replacement.insert(
        editor.intern(b"Subtype"),
        Object::Name(editor.intern(b"Image")),
    );
    replacement.insert(editor.intern(b"Width"), Object::Int(1));
    replacement.insert(editor.intern(b"Height"), Object::Int(1));

    // Nothing else carries over — no /Filter, no /DecodeParms, no /SMask, no
    // /Decode. Every one of them describes bytes that no longer exist.
    if is_mask {
        // A stencil keeps its flag: one carrying a colour space is malformed,
        // and a viewer may reject the page over it.
        replacement.insert(editor.intern(b"ImageMask"), Object::Bool(true));
        replacement.insert(editor.intern(b"BitsPerComponent"), Object::Int(1));
        editor.put_stream(
            reference,
            StreamData {
                dict: replacement,
                // A set bit paints nothing (8.9.6.2).
                data: vec![0x80],
            },
        );
        return;
    }

    replacement.insert(editor.intern(b"BitsPerComponent"), Object::Int(8));
    replacement.insert(
        editor.intern(b"ColorSpace"),
        Object::Name(editor.intern(b"DeviceGray")),
    );
    editor.put_stream(
        reference,
        StreamData {
            dict: replacement,
            data: vec![0xFF],
        },
    );
}

/// One `Do` invocation, and the transform in force when it happened.
struct XObjectUse {
    name: Vec<u8>,
    /// The transform mapping the XObject's space to the page's.
    ctm: Matrix,
}

/// The text state needed to place a glyph: everything in 9.4.4's displacement
/// formula, and nothing else.
///
/// Positions live in **unscaled text space** — the space the text matrix maps
/// *out of*. That is the run's own frame: `x` is how far along the baseline
/// the pen has walked, whatever direction the baseline points on the page, and
/// `rise` and `size` are the glyph box's bottom and top in the same units. A
/// `TJ` displacement is defined in exactly this space (9.4.3), which is why a
/// rewritten run needs no new matrix of its own.
#[derive(Clone)]
struct Pen {
    /// How far along the baseline the pen has walked since the text matrix
    /// was last set, in unscaled text space.
    x: f64,
    /// The text matrix, `T_m` (9.4.2).
    text: Matrix,
    /// The text line matrix, `T_lm`. `Td`, `TD` and `T*` are relative to
    /// this one rather than to `text`, which is why both are carried.
    line: Matrix,
    /// The transformation matrix in force, whole.
    ///
    /// A redaction rectangle is given in *page* space, so content drawn under
    /// a `cm` has to be mapped into it. Ignoring the transform measures every
    /// glyph in the wrong space: `q 2 0 0 2 0 0 cm` halves every computed
    /// position, so the rectangle covers the wrong half of the line.
    ctm: Matrix,
    /// The font in force, when the `Tf` named one in scope.
    font: Option<Arc<RunFont>>,
    /// The resource name the last `Tf` gave, whether or not it resolved.
    ///
    /// Carried so that a refusal can name the font a caller would have to go
    /// and look at, including the case where the name resolved to nothing.
    font_name: Vec<u8>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
}

impl Default for Pen {
    fn default() -> Pen {
        Pen {
            x: 0.0,
            text: Matrix::IDENTITY,
            line: Matrix::IDENTITY,
            ctm: Matrix::IDENTITY,
            font: None,
            font_name: Vec::new(),
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

impl Pen {
    /// Starts a new line at an offset from the current one, per `Td`.
    ///
    /// 9.4.2: relative to the text **line** matrix, not the text matrix, so
    /// the displacement a showing operator accumulated does not carry over.
    fn offset(&mut self, tx: f64, ty: f64) {
        self.line = Matrix::translate(tx, ty).then(self.line);
        self.text = self.line;
        self.x = 0.0;
    }

    /// Moves to the next line, per `T*`.
    fn next_line(&mut self) {
        self.offset(0.0, -self.leading);
    }

    /// The transform from the run's own frame to page space.
    fn frame(&self) -> Matrix {
        self.text.then(self.ctm)
    }

    /// The displacement of one decoded code, per 9.4.4.
    ///
    /// In unscaled text space: the horizontal scale is in, because 9.4.4 puts
    /// it there, and the text matrix is *not*, because that is what
    /// [`Pen::frame`] applies and what a `TJ` number is measured before.
    fn advance(&self, code: &tinker_pdf_cos::DecodedCode) -> f64 {
        // Word spacing applies to single-byte code 32 only — the classic bug
        // is applying it to a two-byte CID that happens to equal 32.
        let word = if code.code == 32 && code.bytes == 1 {
            self.word_spacing
        } else {
            0.0
        };
        (code.width / 1000.0 * self.size + self.char_spacing + word) * self.horizontal_scale
    }

    /// The displacement of a whole string.
    fn advance_of(&self, bytes: &[u8]) -> f64 {
        let Some(selected) = self.font.as_ref() else {
            return 0.0;
        };
        selected
            .font
            .decode(bytes)
            .iter()
            .map(|c| self.advance(c))
            .sum()
    }

    /// The unit a `TJ` number is measured in: one thousandth of this moves the
    /// pen by one (9.4.3).
    fn thousandth(&self) -> f64 {
        self.size * self.horizontal_scale
    }
}

/// Rewrites a content stream with redacted glyphs removed.
fn rewrite(
    content: &[u8],
    areas: &[Redaction],
    fonts: &HashMap<Vec<u8>, Arc<RunFont>>,
    initial: Matrix,
) -> (Vec<u8>, RedactionReport, Vec<XObjectUse>) {
    let mut out = Vec::with_capacity(content.len());
    let mut tokens = Tokenizer::new(content);
    let mut operands: Vec<Token> = Vec::new();
    let mut uses: Vec<XObjectUse> = Vec::new();
    let mut pen = Pen {
        ctm: initial,
        ..Pen::default()
    };
    let mut saved: Vec<Pen> = Vec::new();
    let mut report = RedactionReport::default();

    while let Some(token) = tokens.next_token() {
        let Token::Operator(op) = &token else {
            operands.push(token);
            continue;
        };

        // Operands are counted from the end, because that is where the
        // operator's own arguments are regardless of what preceded them.
        let number = |back: usize| -> f64 {
            let index = operands.len().checked_sub(back + 1);
            match index.and_then(|i| operands.get(i)) {
                Some(Token::Number(v)) if v.is_finite() => *v,
                _ => 0.0,
            }
        };

        let mut rewritten = false;

        match op.as_slice() {
            b"Do" => {
                // 8.8: the operand names an XObject. Which kind it is, and
                // what to do about it, is the caller's business — this crate
                // has the transform, and the caller has the dictionaries.
                if let Some(Token::Name(name)) = operands.last() {
                    if uses.len() < 4096 {
                        uses.push(XObjectUse {
                            name: name.clone(),
                            ctm: pen.ctm,
                        });
                    }
                }
            }
            b"cm" => {
                // Composed with whatever is already in force, because `cm`
                // concatenates rather than replaces (8.4.4).
                let operand = Matrix {
                    a: number(5),
                    b: number(4),
                    c: number(3),
                    d: number(2),
                    e: number(1),
                    f: number(0),
                };
                pen.ctm = operand.then(pen.ctm);
            }
            b"q" => saved.push(pen.clone()),
            b"Q" => {
                if let Some(restored) = saved.pop() {
                    pen = restored;
                }
            }
            b"BT" => {
                // The text matrices reset; the text *state* does not.
                pen.text = Matrix::IDENTITY;
                pen.line = Matrix::IDENTITY;
                pen.x = 0.0;
            }
            b"Tf" => {
                pen.size = number(0);
                pen.font_name = operands
                    .len()
                    .checked_sub(2)
                    .and_then(|i| operands.get(i))
                    .and_then(|t| match t {
                        Token::Name(name) => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                pen.font = fonts.get(&pen.font_name).map(Arc::clone);
            }
            b"Tc" => pen.char_spacing = number(0),
            b"Tw" => pen.word_spacing = number(0),
            b"Tz" => pen.horizontal_scale = number(0) / 100.0,
            b"TL" => pen.leading = number(0),
            b"Ts" => pen.rise = number(0),
            b"Td" => pen.offset(number(1), number(0)),
            b"TD" => {
                pen.leading = -number(0);
                pen.offset(number(1), number(0));
            }
            b"Tm" => {
                // 9.4.2: the matrix *replaces* both text matrices, and it
                // scales, rotates and skews the glyph space as well as
                // placing it.
                pen.line = Matrix {
                    a: number(5),
                    b: number(4),
                    c: number(3),
                    d: number(2),
                    e: number(1),
                    f: number(0),
                };
                pen.text = pen.line;
                pen.x = 0.0;
            }
            b"T*" => pen.next_line(),
            b"Tj" | b"'" | b"\"" => {
                if op.as_slice() != b"Tj" {
                    pen.next_line();
                }
                if op.as_slice() == b"\"" {
                    // `aw ac string "` sets both spacings before showing.
                    pen.word_spacing = number(2);
                    pen.char_spacing = number(1);
                }
                if let Some(Token::String(bytes)) = operands.last().cloned() {
                    let cut = redact_string(&bytes, &pen, areas);
                    if let Some(warning) = cut.warning {
                        note(&mut report.warnings, warning);
                    }
                    if cut.removed > 0 {
                        report.operations += 1;
                        report.glyphs += cut.removed;
                        // 9.4.3: `'` is `T*` then `Tj`, and `" ` is
                        // `aw Tw ac Tc` then `'`. A cut run is re-emitted as
                        // `TJ`, which is only the showing half — so the
                        // halves that are not the showing are written out
                        // first. Dropping them moves every surviving glyph
                        // of the run onto the previous line, which is a cut
                        // that is approximately right.
                        if op.as_slice() == b"\"" {
                            let spacings =
                                format!("{} Tw {} Tc\n", pen.word_spacing, pen.char_spacing);
                            out.extend_from_slice(spacings.as_bytes());
                        }
                        if op.as_slice() != b"Tj" {
                            out.extend_from_slice(b"T*\n");
                        }
                        emit_array(&mut out, &cut.runs, pen.thousandth());
                        rewritten = true;
                    }
                    pen.x += pen.advance_of(&bytes);
                }
            }
            b"TJ" => {
                // Built once rather than probed and rebuilt: the pen ends in
                // the same place either way, and a second pass would raise
                // every refusal twice.
                let mut runs: Vec<Run> = Vec::new();
                let mut removed = 0usize;
                let mut local = pen.clone();
                for token in &operands {
                    match token {
                        Token::String(s) => {
                            let cut = redact_string(s, &local, areas);
                            if let Some(warning) = cut.warning {
                                note(&mut report.warnings, warning);
                            }
                            removed += cut.removed;
                            runs.extend(cut.runs);
                            local.x += local.advance_of(s);
                        }
                        Token::Number(v) if v.is_finite() => {
                            // 9.4.3: the number moves the pen *backwards* by
                            // its value in thousandths, along the baseline.
                            let shift = -v / 1000.0 * local.thousandth();
                            runs.push(Run::Gap(shift));
                            local.x += shift;
                        }
                        _ => {}
                    }
                }

                if removed > 0 {
                    report.operations += 1;
                    report.glyphs += removed;
                    emit_array(&mut out, &runs, pen.thousandth());
                    rewritten = true;
                }
                pen = local;
            }
            _ => {}
        }

        if !rewritten {
            for operand in &operands {
                write_token(&mut out, operand);
                out.push(b' ');
            }
            out.extend_from_slice(op);
            out.push(b'\n');
        }
        operands.clear();
    }

    (out, report, uses)
}

/// A piece of a rewritten showing operation.
enum Run {
    /// Bytes that survived.
    Text(Vec<u8>),
    /// Displacement along the baseline, in unscaled text space, where glyphs
    /// used to be — or an adjustment that was already in the original `TJ`
    /// array.
    Gap(f64),
}

/// Writes runs as a single `TJ` array.
///
/// Everything becomes `TJ` because that is the only showing operator that can
/// express "move by this much without drawing", which is exactly what a
/// removed glyph leaves behind.
///
/// # Why no new text matrix
///
/// The surviving sub-runs are emitted into the text matrix the original run
/// was already using. Giving each sub-run a fresh `Tm` at its own origin is
/// the answer that looks plausible on screen and is wrong four ways:
///
/// - `Tm` replaces the text **line** matrix as well as the text matrix, so
///   any later `Td`, `TD` or `T*` in the same `BT` — all of which are
///   relative to the line matrix — would be measured from the last sub-run's
///   origin instead of from the line's.
/// - The six numbers would have to be *reconstructed*, and the rotation among
///   them rounded; neighbouring sub-runs rounded differently sit on baselines
///   that no longer line up.
/// - The position a run is at need not have come from a `Tm` at all. It can
///   be a `Td` chain from a matrix set in an earlier `BT`, so reconstructing
///   one means writing out a matrix that was only ever inferred.
/// - After the operation the text matrix has to have advanced by the whole
///   string's displacement, because that is what the next operator expects.
///   `TJ` does that natively.
///
/// So the only numbers this writes are displacements along the baseline, and
/// 9.4.3 measures those in unscaled text space — the run's own frame —
/// whatever the run's matrix does to the page. That is the whole of what
/// "cut along its own baseline" costs.
///
/// `thousandth` is `Tfs · Th`: one thousandth of it is what a `TJ` number of
/// one moves the pen by. When it is zero no `TJ` number can express any
/// displacement at all — but a run with a zero font size or a zero horizontal
/// scale draws a glyph box of zero area, so there is nothing whose position
/// the lost gap could disturb.
fn emit_array(out: &mut Vec<u8>, runs: &[Run], thousandth: f64) {
    out.push(b'[');
    for run in runs {
        match run {
            Run::Text(bytes) if bytes.is_empty() => {}
            Run::Text(bytes) => {
                out.push(b'(');
                escape_into(out, bytes);
                out.extend_from_slice(b") ");
            }
            Run::Gap(shift) => {
                if thousandth.abs() <= f64::EPSILON || !shift.is_finite() {
                    continue;
                }
                let thousandths = -shift / thousandth * 1000.0;
                if thousandths.abs() >= 0.0005 {
                    out.extend_from_slice(format!("{thousandths:.3} ").as_bytes());
                }
            }
        }
    }
    out.extend_from_slice(b"] TJ\n");
}

/// The result of cutting one string.
struct Cut {
    runs: Vec<Run>,
    removed: usize,
    /// Why the string was left whole, when it was.
    warning: Option<RedactionWarning>,
}

/// Removes the glyphs of `bytes` that fall inside a redaction.
///
/// The glyph box is measured in the run's own frame — `x` along the baseline,
/// `rise` to `rise + size` across it — and carried into page space by
/// [`Pen::frame`], which is the text matrix and the transformation matrix
/// composed. A rotated or skewed matrix turns that box into a parallelogram
/// rather than making it unmeasurable, and [`quad_meets_rect`] answers
/// exactly.
///
/// A glyph the rectangle covers **partly** is removed. There is no third
/// option: a content stream can show a glyph or not show it, so the choice is
/// between removing a glyph that was partly visible and leaving one that was
/// partly covered — and only one of those two can leak. The visible cost is a
/// `mark`ed rectangle with a bite of blank page beside it where the
/// over-removed glyph was, which is a thing a reader can see rather than a
/// thing an extractor can find.
fn redact_string(bytes: &[u8], pen: &Pen, areas: &[Redaction]) -> Cut {
    let whole = |warning: Option<RedactionWarning>| Cut {
        runs: vec![Run::Text(bytes.to_vec())],
        removed: 0,
        warning,
    };

    // No rectangle, nothing to be uncertain about: a refusal warning says
    // "this run was not tested against your rectangles", and with no
    // rectangles that sentence has no content.
    if areas.is_empty() {
        return whole(None);
    }

    let font = || pen.font_name.clone();
    let left = bytes.len();

    let Some(selected) = pen.font.as_ref() else {
        return whole(Some(RedactionWarning::UnknownFont {
            font: font(),
            bytes: left,
        }));
    };
    if selected.font.is_vertical() {
        return whole(Some(RedactionWarning::VerticalRun {
            font: font(),
            bytes: left,
        }));
    }
    if selected.rescaled_type3 {
        return whole(Some(RedactionWarning::RescaledType3Font {
            font: font(),
            bytes: left,
        }));
    }

    let frame = pen.frame();
    if !frame.is_finite()
        || !pen.x.is_finite()
        || !pen.size.is_finite()
        || !pen.rise.is_finite()
        || !pen.horizontal_scale.is_finite()
    {
        return whole(Some(RedactionWarning::UnmeasurableFrame {
            font: font(),
            bytes: left,
        }));
    }

    // Measured in full before anything is cut, so that a run which turns out
    // to be unmeasurable part-way along is left whole rather than half-cut.
    let codes = selected.font.decode(bytes);
    let y0 = pen.rise;
    let y1 = pen.rise + pen.size;
    let mut boxes: Vec<([(f64, f64); 4], f64)> = Vec::with_capacity(codes.len());
    let mut x = pen.x;
    for code in &codes {
        let advance = pen.advance(code);
        // The glyph's box, approximated from its advance and the font size.
        // Approximating is right here: an exact outline would let a descender
        // poking one hundredth of a point into the box decide the redaction,
        // and erring towards removal is the safe direction anyway.
        let quad = [
            frame.apply(x, y0),
            frame.apply(x + advance, y0),
            frame.apply(x + advance, y1),
            frame.apply(x, y1),
        ];
        if !advance.is_finite() || quad.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
            return whole(Some(RedactionWarning::UnmeasurableFrame {
                font: font(),
                bytes: left,
            }));
        }
        boxes.push((quad, advance));
        x += advance;
    }

    let mut runs: Vec<Run> = Vec::new();
    let mut kept: Vec<u8> = Vec::new();
    let mut gap = 0.0f64;
    let mut removed = 0usize;

    for (code, (quad, advance)) in codes.iter().zip(&boxes) {
        let inside = areas
            .iter()
            .any(|redaction| quad_meets_rect(quad, redaction.area));

        if inside {
            removed += 1;
            if !kept.is_empty() {
                runs.push(Run::Text(std::mem::take(&mut kept)));
            }
            gap += advance;
        } else {
            if gap != 0.0 {
                runs.push(Run::Gap(std::mem::take(&mut gap)));
            }
            encode_code(&mut kept, code.code, code.bytes);
        }
    }

    if !kept.is_empty() {
        runs.push(Run::Text(kept));
    }
    if gap != 0.0 {
        // A trailing gap still matters: it holds the pen's position for
        // whatever the next operator shows.
        runs.push(Run::Gap(gap));
    }

    Cut {
        runs,
        removed,
        warning: None,
    }
}

/// Whether a convex quadrilateral and an axis-aligned rectangle overlap.
///
/// The separating-axis theorem: two convex shapes miss each other exactly when
/// some direction separates their projections. For a rectangle and a
/// parallelogram the candidate directions are four — the rectangle's two axes
/// and the parallelogram's two edge normals — and a direction along which one
/// shape's projection ends where the other's begins is a separation, so a
/// glyph box that merely *touches* the rectangle is not covered. That matches
/// the strict comparisons this test replaced, which is why the fixtures that
/// sit a boundary exactly on a glyph edge still mean what they meant.
///
/// Projections are taken over the corners as given, so a rectangle written
/// with its corners the wrong way round — `x0` greater than `x1` — describes
/// the same area rather than an empty one.
///
/// A degenerate quadrilateral is handled rather than special-cased: a zero
/// advance collapses one edge, whose normal is then skipped, and the test
/// reduces to a segment against the rectangle. Both edges collapsing reduces
/// it to a point.
fn quad_meets_rect(quad: &[(f64, f64); 4], area: Rect) -> bool {
    let corners = [
        (area.x0, area.y0),
        (area.x1, area.y0),
        (area.x1, area.y1),
        (area.x0, area.y1),
    ];

    let edges = [
        (quad[1].0 - quad[0].0, quad[1].1 - quad[0].1),
        (quad[2].0 - quad[1].0, quad[2].1 - quad[1].1),
    ];
    let axes = [
        (1.0, 0.0),
        (0.0, 1.0),
        (-edges[0].1, edges[0].0),
        (-edges[1].1, edges[1].0),
    ];

    for axis in axes {
        let length = (axis.0 * axis.0 + axis.1 * axis.1).sqrt();
        if !length.is_finite() || length <= 1e-9 {
            continue;
        }
        let unit = (axis.0 / length, axis.1 / length);
        let span = |points: &[(f64, f64)]| {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for point in points {
                let at = point.0 * unit.0 + point.1 * unit.1;
                lo = lo.min(at);
                hi = hi.max(at);
            }
            (lo, hi)
        };
        let (glyph_lo, glyph_hi) = span(quad);
        let (area_lo, area_hi) = span(&corners);
        if glyph_hi <= area_lo || area_hi <= glyph_lo {
            return false;
        }
    }

    true
}

/// Re-encodes a code as the bytes it was decoded from.
fn encode_code(out: &mut Vec<u8>, code: u32, width: u8) {
    let width = width.clamp(1, 4);
    for shift in (0..width).rev() {
        out.push(((code >> (u32::from(shift) * 8)) & 0xFF) as u8);
    }
}

fn escape_into(out: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
}

fn write_token(out: &mut Vec<u8>, token: &Token) {
    match token {
        Token::Number(v) => out.extend_from_slice(format!("{v}").as_bytes()),
        Token::String(s) => {
            out.push(b'(');
            escape_into(out, s);
            out.push(b')');
        }
        Token::Name(n) => {
            out.push(b'/');
            out.extend_from_slice(n);
        }
        Token::ArrayOpen => out.push(b'['),
        Token::ArrayClose => out.push(b']'),
        Token::DictOpen => out.extend_from_slice(b"<<"),
        Token::DictClose => out.extend_from_slice(b">>"),
        Token::Bool(true) => out.extend_from_slice(b"true"),
        Token::Bool(false) => out.extend_from_slice(b"false"),
        Token::Null => out.extend_from_slice(b"null"),
        Token::Operator(o) => out.extend_from_slice(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_cos::{DocumentBuilder, WriteMode, WriteOptions};

    fn document(text: &str) -> Arc<CosDocument> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, text);
        });
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    }

    /// Every stream in a document, decompressed. The acceptance test for a
    /// redaction is that the needle is absent from all of them — not from the
    /// one stream we happen to look at.
    fn all_streams(doc: &CosDocument) -> String {
        let mut out = Vec::new();
        for (number, _) in doc.xref().iter() {
            if let Ok(bytes) = doc.stream_decoded(ObjRef::new(number, 0)) {
                out.extend_from_slice(&bytes);
                out.push(b'\n');
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn redact(doc: Arc<CosDocument>, areas: &[Redaction]) -> (Vec<u8>, RedactionReport) {
        let mut editor = DocumentEditor::new(doc);
        let report = apply(&mut editor, 0, areas).expect("the page exists");
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        (bytes, report)
    }

    /// Redacts, reopens, and returns every stream the result contains.
    fn redact_to_streams(doc: Arc<CosDocument>, areas: &[Redaction]) -> (String, RedactionReport) {
        let (bytes, report) = redact(doc, areas);
        let reopened = CosDocument::open(bytes).expect("it reopens");
        (all_streams(&reopened), report)
    }

    /// Covers the second word of `PUBLIC SECRET` and nothing else.
    ///
    /// `PUBLIC` ends at about x=55.3 in 12pt Helvetica, so the band starts
    /// past that. A glyph that merely touches the band is removed, which is
    /// the safe direction but does mean the boundary is not arbitrary.
    fn second_word() -> Redaction {
        Redaction {
            area: Rect {
                x0: 57.0,
                y0: 45.0,
                x1: 400.0,
                y1: 70.0,
            },
            mark: false,
        }
    }

    /// The test that matters.
    #[test]
    fn redacted_bytes_are_absent_from_every_stream() {
        let doc = document("PUBLIC SECRET");
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts out present, or the test proves nothing"
        );

        let (streams, report) = redact_to_streams(doc, &[second_word()]);

        assert!(report.glyphs > 0, "glyphs were removed");
        assert!(
            !streams.contains("SECRET"),
            "the redacted bytes must be gone from the file, got: {streams}"
        );
    }

    #[test]
    fn text_outside_the_area_survives() {
        let doc = document("PUBLIC SECRET");
        let (streams, _) = redact_to_streams(doc, &[second_word()]);
        assert!(
            streams.contains("PUBLIC"),
            "text outside the rectangle is untouched"
        );
    }

    /// Reading the page back through the engine's own extractor, which is how
    /// anyone looking for the secret would read it.
    #[test]
    fn extraction_no_longer_finds_it() {
        let doc = document("PUBLIC SECRET");
        let (bytes, _) = redact(doc, &[second_word()]);

        let reopened = crate::Document::open(bytes).expect("it reopens");
        let page = reopened.page(0).expect("the page is there");
        let text = page.text().plain_text();
        assert!(
            !text.contains("SECRET"),
            "the extractor must not find it either, got: {text:?}"
        );
        assert!(
            text.contains("PUBLIC"),
            "but it still finds the rest, got: {text:?}"
        );
    }

    #[test]
    fn a_redaction_covering_nothing_removes_nothing() {
        let doc = document("PUBLIC SECRET");
        let (streams, report) = redact_to_streams(
            doc,
            &[Redaction {
                area: Rect {
                    x0: 1000.0,
                    y0: 1000.0,
                    x1: 1100.0,
                    y1: 1100.0,
                },
                mark: false,
            }],
        );

        assert_eq!(report, RedactionReport::default());
        assert!(streams.contains("PUBLIC SECRET"));
    }

    #[test]
    fn an_empty_redaction_list_is_a_no_op() {
        let doc = document("PUBLIC SECRET");
        let (streams, report) = redact_to_streams(doc, &[]);
        assert_eq!(report, RedactionReport::default());
        assert!(streams.contains("PUBLIC SECRET"));
    }

    #[test]
    fn a_marked_redaction_paints_the_area() {
        let doc = document("PUBLIC SECRET");
        let area = Redaction {
            mark: true,
            ..second_word()
        };
        let (streams, _) = redact_to_streams(doc, &[area]);

        assert!(streams.contains(" re f"), "a rectangle is painted");
        assert!(streams.contains("0 g"), "in black");
        assert!(!streams.contains("SECRET"), "and the text is still gone");
    }

    /// Builds a page whose text is placed by a scaling `Tm` rather than by
    /// `Td` with a sized font — `/F0 1 Tf` plus `12 0 0 12 x y Tm` is an
    /// ordinary way to write twelve-point text.
    fn scaled_document(text: &str) -> Arc<CosDocument> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 100.0, |page| {
            page.raw(
                format!(
                    "BT /F0 1 Tf 12 0 0 12 10 50 Tm ({text}) Tj ET
"
                )
                .as_bytes(),
            );
        });
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    }

    /// Reading only `Tm`'s translation left every advance twelve times too
    /// small, so the positions drifted further wrong across the line and the
    /// rectangle cut the wrong glyphs — or none at all.
    #[test]
    fn a_scaling_text_matrix_places_the_glyphs() {
        let doc = scaled_document("PUBLIC SECRET");
        assert!(all_streams(&doc).contains("SECRET"));

        let (streams, report) = redact_to_streams(doc, &[second_word()]);
        assert!(report.glyphs > 0, "something was found to cut");
        assert!(
            !streams.contains("SECRET"),
            "the second word is gone: {streams}"
        );
        assert!(streams.contains("PUBLIC"), "and the first survives");
    }

    /// A redaction rectangle is in page space. Content drawn under a `cm` is
    /// in some other space, and ignoring the transform measured every glyph in
    /// the wrong one — so the rectangle covered the wrong part of the line, or
    /// nothing at all.
    #[test]
    fn a_transform_is_mapped_into_page_space() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 200.0, |page| {
            // Everything at double scale: the text is written at (5, 25) in
            // its own space, which is (10, 50) on the page.
            page.raw(
                b"q 2 0 0 2 0 0 cm BT /F0 6 Tf 5 25 Td (PUBLIC SECRET) Tj ET Q
",
            );
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));
        assert!(all_streams(&doc).contains("SECRET"));

        // In page space the text starts at x = 10 and is 12pt tall. `PUBLIC`
        // ends near x = 65, so a band from 57 rightwards takes the second word.
        let band = Redaction {
            area: Rect {
                x0: 57.0,
                y0: 45.0,
                x1: 400.0,
                y1: 70.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[band]);
        assert!(report.glyphs > 0, "the transform was followed");
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(streams.contains("PUBLIC"), "and the first word survives");
    }

    /// The transform is restored by `Q`, so a rectangle over content outside
    /// the `q`/`Q` pair is not measured through it.
    #[test]
    fn a_transform_does_not_outlive_its_q() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 200.0, |page| {
            page.raw(
                b"q 2 0 0 2 0 0 cm BT /F0 6 Tf 5 60 Td (SCALED) Tj ET Q
                  BT /F0 12 Tf 10 30 Td (PLAIN) Tj ET
",
            );
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        // A band over the lower line only, in page space.
        let band = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 25.0,
                x1: 400.0,
                y1: 45.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[band]);
        assert!(report.glyphs > 0);
        assert!(!streams.contains("PLAIN"), "the lower line went");
        assert!(
            streams.contains("SCALED"),
            "and the scaled line, which sits higher, stayed: {streams}"
        );
    }

    /// A form XObject holds content like any other. A redaction that stopped
    /// at the page stream left everything a form drew exactly where it was —
    /// and forms are how most producers place repeated content, so this was a
    /// hole a redaction could drive through.
    #[test]
    fn text_inside_a_form_xobject_is_redacted() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 24 >>\nstream\n\
q 1 0 0 1 0 0 cm /Fm0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 200]\n\
   /Resources << /Font << /F0 6 0 R >> >> /Length 46 >>\nstream\n\
BT /F0 12 Tf 10 50 Td (PUBLIC SECRET) Tj ET\n\
endstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n";

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts present"
        );

        let (streams, report) = redact_to_streams(doc, &[second_word()]);
        assert!(report.glyphs > 0, "the form was followed into");
        assert!(
            !streams.contains("SECRET"),
            "and the text inside it is gone from every stream: {streams}"
        );
        assert!(streams.contains("PUBLIC"), "the rest survives");
    }

    /// **A pinned defect.** A form drawn twice is rewritten once, so only its
    /// first placement is ever measured against the rectangles.
    ///
    /// `visited` is there to stop a self-referential form recursing forever,
    /// and it makes the second `Do` a no-op as well. That is right when both
    /// invocations share a transform and wrong when they do not. The form
    /// below draws `SECRET` at page y 50 and again at page y 200; the
    /// rectangle covers the second placement only; the first pass finds
    /// nothing under it and marks the form done. The text is left whole, no
    /// warning names it, and the report is indistinguishable from a rectangle
    /// that covered nothing — which is the silent under-redaction the rest of
    /// this module exists to refuse, reached by a different road.
    ///
    /// Asserted as it behaves rather than as it should, because a test that
    /// fails is a test somebody turns off. The fix is a decision this row does
    /// not own — either each placement gets its own copy of the form, which
    /// changes what the saved file looks like, or a form invoked twice under
    /// different transforms refuses and reports — so it is a roadmap row of
    /// its own and this pin is its evidence.
    #[test]
    fn a_form_drawn_twice_is_cut_only_at_its_first_placement() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /Font << /F0 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 56 >>\nstream\n\
q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 150 cm /Fm0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 300]\n\
   /Resources << /Font << /F0 6 0 R >> >> /Length 39 >>\nstream\n\
BT /F0 12 Tf 10 50 Td (SECRET) Tj ET\n\
endstream\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n";

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts present"
        );

        // The second placement's baseline is page y 200; the first's is y 50.
        let over_the_second = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 190.0,
                x1: 400.0,
                y1: 230.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[over_the_second]);
        assert_eq!(
            report.glyphs, 0,
            "PINNED DEFECT: the second placement was never measured"
        );
        assert!(
            report.warnings.is_empty(),
            "PINNED DEFECT: and nothing told the caller: {:?}",
            report.warnings
        );
        assert!(
            streams.contains("SECRET"),
            "PINNED DEFECT: the text under the rectangle is still in the file"
        );
    }

    /// A form that invokes itself must not recurse forever.
    #[test]
    fn a_self_referential_form_terminates() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 10 >>\nstream\n\
/Fm0 Do\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 400 200]\n\
   /Resources << /XObject << /Fm0 5 0 R >> >> /Length 10 >>\nstream\n\
/Fm0 Do\n\
endstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        let (_, report) = redact_to_streams(doc, &[second_word()]);
        assert_eq!(report.glyphs, 0, "there is no text, and it terminated");
    }

    /// An image under a redaction is scrubbed, not covered. A rectangle
    /// painted over a photograph removes nothing at all: the samples stay in
    /// the file for anyone who decompresses it.
    #[test]
    fn an_image_under_a_redaction_is_scrubbed() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.7\n");
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        bytes.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        let content = b"q 100 0 0 100 60 50 cm /Im0 Do Q\n";
        bytes.extend_from_slice(
            format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        );
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(b"endstream\nendobj\n");

        // Recognisable samples, so their absence is checkable.
        let samples = b"SECRETPIXELDATA!";
        bytes.extend_from_slice(
            format!(
                "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4\n\
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                samples.len()
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(samples);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");
        bytes.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        assert!(
            all_streams(&doc).contains("SECRETPIXEL"),
            "the samples start present"
        );

        // The image occupies 60..160 by 50..150 on the page.
        let over_the_image = Redaction {
            area: Rect {
                x0: 80.0,
                y0: 70.0,
                x1: 120.0,
                y1: 110.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[over_the_image]);

        assert_eq!(report.images, 1, "the image was scrubbed");
        assert!(
            !streams.contains("SECRETPIXEL"),
            "and its samples are gone from every stream: {streams}"
        );
    }

    /// An image the redaction does not touch is left alone. Scrubbing every
    /// image on a page would destroy content nobody asked to remove.
    #[test]
    fn an_image_outside_the_redaction_survives() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.7\n");
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        bytes.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200]\n\
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
        );
        let content = b"q 20 0 0 20 5 5 cm /Im0 Do Q\n";
        bytes.extend_from_slice(
            format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        );
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(b"endstream\nendobj\n");

        let samples = b"KEEPTHESEPIXELS!";
        bytes.extend_from_slice(
            format!(
                "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4\n\
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                samples.len()
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(samples);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");
        bytes.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");

        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));

        // Far from the image, which sits at 5..25 on both axes.
        let elsewhere = Redaction {
            area: Rect {
                x0: 300.0,
                y0: 150.0,
                x1: 380.0,
                y1: 190.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[elsewhere]);

        assert_eq!(report.images, 0);
        assert!(
            streams.contains("KEEPTHESE"),
            "an untouched image keeps its samples"
        );
    }

    #[test]
    fn a_page_that_does_not_exist_is_refused() {
        let doc = document("text");
        let mut editor = DocumentEditor::new(doc);
        assert!(apply(&mut editor, 9, &[second_word()]).is_none());
    }

    /// Removing the middle of a line must not shift what follows it: the gap
    /// is re-emitted as a `TJ` displacement.
    #[test]
    fn surviving_text_keeps_its_position() {
        let doc = document("AAA BBB CCC");
        let (streams, report) = redact_to_streams(
            doc,
            &[Redaction {
                // A band over the middle word only. In 12pt Helvetica
                // `AAA ` ends at 37.35 and ` CCC` starts at 61.36, so this
                // takes all three Bs and neither space.
                area: Rect {
                    x0: 37.5,
                    y0: 45.0,
                    x1: 61.0,
                    y1: 70.0,
                },
                mark: false,
            }],
        );

        assert!(report.glyphs > 0);
        assert!(streams.contains("] TJ"), "re-emitted as an array");
        assert_eq!(report.glyphs, 3, "the three Bs and nothing else");
        assert!(
            streams.contains("AAA") && streams.contains("CCC"),
            "the outer words survive: {streams}"
        );
        assert!(!streams.contains("BBB"), "the middle word is gone");
    }

    /// A content stream the tokenizer cannot make sense of must come back
    /// intact rather than truncated — never fail the page (ruling 2).
    #[test]
    fn garbage_content_survives_the_rewrite() {
        let fonts = HashMap::new();
        let content = b"q 1 0 0 1 0 0 cm ) ) ) >> BI garbage EI Q";
        let (out, report, _) = rewrite(content, &[second_word()], &fonts, Matrix::IDENTITY);
        assert_eq!(report, RedactionReport::default());
        assert!(out.contains(&b'q'), "the operators survive");
    }
}

/// Runs whose text matrix rotates or skews, which this module used to refuse.
///
/// # What is adjudicated by what
///
/// Nothing here is checked against another implementation, and nothing needs
/// to be: the third-party data is **the standard itself** — ISO 32000-1's
/// 9.4.2 (the text and text line matrices), 9.4.3 (`TJ`'s displacement, and
/// what `'` and `\"` expand to) and 9.4.4 (the glyph displacement formula and
/// the text rendering matrix), all three carrying the same numbers in ISO
/// 32000-2 — and the fixtures are arithmetic done by hand from
/// those clauses and written into the assertions as expected page-space
/// coordinates. A fixture says "glyph six of this run occupies page x 90..100,
/// y 80..90", which is a claim about the specification that a reader can check
/// with a pencil, not a claim that two of this repository's own components
/// agree with each other.
///
/// The one link that is **self-consistency and is labelled as such**: the ink
/// assertions read this repository's own renderer. That the renderer draws
/// glyph six where the specification puts it is not proved here, and stating
/// where it *is* proved is part of labelling this link honestly:
/// `crates/tinker-pdf/tests/render_analytic.rs` holds the renderer to formulas
/// ISO 32000-1 publishes as expressions, and `render_differential.rs` holds
/// pairs of documents that must draw the same picture. The golden bitmaps
/// beside them are `UNREVIEWED` and the roadmap says so, which is why they are
/// not named here. What the ink assertions add is orthogonal to all of that
/// and is the
/// reason they exist: a redaction that removed the bytes from one stream while
/// leaving a second, unreferenced copy of them elsewhere passes every
/// byte-level check in this file and fails the ink check, because the page
/// still draws the glyph. Neither level is the safety property on its own.
#[cfg(test)]
mod rotated_runs {
    use super::tests_support::*;
    use super::*;

    /// A quarter turn: the run walks **up** the page.
    ///
    /// `0 1 -1 0 100 20 Tm` maps unscaled text space `(x, y)` to page
    /// `(100 - y, 20 + x)`. With a ten-point font whose every glyph is one em
    /// wide, glyph `k` therefore occupies page x 90..100 and page
    /// y `20 + 10k` .. `30 + 10k`. `PUBLIC` is glyphs 0..5, page y 20..80;
    /// `SECRET` is glyphs 6..11, page y 80..140.
    fn quarter_turn() -> Vec<u8> {
        boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 0 1 -1 0 100 20 Tm (PUBLICSECRET) Tj ET",
        )
    }

    /// The band over `SECRET` and nothing else.
    ///
    /// Two points of clearance below glyph six's box rather than a boundary
    /// sitting exactly on glyph five's edge, so that the ink assertion is not
    /// reading an antialiased pixel of the glyph that survived.
    fn upper_band() -> Redaction {
        Redaction {
            area: Rect {
                x0: 85.0,
                y0: 82.0,
                x1: 105.0,
                y1: 145.0,
            },
            mark: false,
        }
    }

    /// The headline, and the row's exit criterion: a rotated run is cut, and
    /// cut along its own baseline rather than along the page's.
    ///
    /// Both levels of the safety property are asserted here, because either
    /// alone passes a build the other fails: the bytes are gone from **every**
    /// decompressed stream in the file, and the redacted page **draws no ink**
    /// inside the rectangle.
    #[test]
    fn a_rotated_run_is_cut_along_its_own_baseline() {
        let doc = open(quarter_turn());
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts out present, or the test proves nothing"
        );

        let (bytes, report) = redact(doc, &[upper_band()]);
        assert_eq!(report.glyphs, 6, "the six glyphs of the second word");
        assert!(report.warnings.is_empty(), "nothing was refused");

        let reopened = CosDocument::open(bytes.clone()).expect("it reopens");
        let streams = all_streams(&reopened);
        assert!(
            !streams.contains("SECRET"),
            "the covered glyphs' codes are gone from every stream: {streams}"
        );
        assert!(
            streams.contains("PUBLIC"),
            "and the first word is still there: {streams}"
        );

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, upper_band().area),
            0,
            "no ink survives inside the redacted rectangle"
        );
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 90.0,
                    y0: 21.0,
                    x1: 100.0,
                    y1: 79.0,
                }
            ) > 100,
            "and the part of the run nobody redacted still draws"
        );
    }

    /// The run's matrix is the original bytes, untouched, and there is still
    /// exactly one of them.
    ///
    /// Re-anchoring each surviving sub-run with a `Tm` of its own is the
    /// plausible-looking wrong answer; [`emit_array`] gives the four reasons.
    #[test]
    fn a_cut_rotated_run_keeps_the_text_matrix_it_had() {
        let (bytes, _) = redact(open(quarter_turn()), &[upper_band()]);
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));

        assert!(
            streams.contains("0 1 -1 0 100 20 Tm"),
            "the original matrix is re-emitted verbatim: {streams}"
        );
        assert_eq!(
            streams.matches("Tm").count(),
            1,
            "and no second one was invented: {streams}"
        );
    }

    /// Removing a glyph from the middle must leave everything after it where
    /// it was on the page — which, for a rotated run, means further **up**,
    /// not further right.
    ///
    /// The band covers glyph three (`L`, page y 50..60) and nothing else. If
    /// the removed glyph's displacement were dropped, every later glyph would
    /// slide ten points back down the baseline toward the run's origin: the
    /// sample at the tail would go blank and the sample inside the cut would
    /// light up. Both are asserted, so that neither a shift nor a leak passes.
    #[test]
    fn removing_a_rotated_glyph_leaves_the_tail_where_it_was() {
        let band = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 51.0,
                x1: 105.0,
                y1: 59.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(open(quarter_turn()), &[band]);
        assert_eq!(report.glyphs, 1, "glyph three alone");

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, band.area),
            0,
            "the cut glyph is gone"
        );
        // Glyph four, which follows the cut, is still at page y 60..70.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 92.0,
                    y0: 62.0,
                    x1: 98.0,
                    y1: 68.0,
                }
            ) > 20,
            "the glyph after the cut did not slide back down the baseline"
        );
        // And the last glyph is still at page y 130..140.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 92.0,
                    y0: 132.0,
                    x1: 98.0,
                    y1: 138.0,
                }
            ) > 20,
            "nor did the end of the run"
        );
    }

    /// A glyph the rectangle covers only partly goes, because the only other
    /// choice is leaving a glyph that was partly covered — and only one of
    /// those two can leak.
    ///
    /// The band is a seven-by-four sliver straddling the boundary between
    /// glyphs three and four, clipping a corner of each. Neither is contained.
    #[test]
    fn a_partly_covered_rotated_glyph_is_removed() {
        let sliver = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 58.0,
                x1: 92.0,
                y1: 62.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(open(quarter_turn()), &[sliver]);
        assert_eq!(report.glyphs, 2, "both partly covered glyphs went");

        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(
            !streams.contains("LI"),
            "glyphs three and four are both gone: {streams}"
        );
    }

    /// A rotation that is not a multiple of a quarter turn, where every glyph
    /// box is a square standing on a corner and no axis-aligned reading of the
    /// run can come close.
    ///
    /// `0.6 0.8 -0.8 0.6 20 20 Tm` maps `(x, y)` to
    /// `(0.6x - 0.8y + 20, 0.8x + 0.6y + 20)`. Glyph `k` of a ten-point run
    /// therefore has corners at `(6k+20, 8k+20)`, `(6k+26, 8k+28)`,
    /// `(6k+18, 8k+34)` and `(6k+12, 8k+26)` — a square of side ten rotated
    /// 53 degrees, stepping ten points up the diagonal per glyph.
    ///
    /// The rectangle starts at `x = 73, y = 107`. Glyph eleven lies wholly
    /// inside it; glyph ten reaches it at the corner `(78, 114)`; glyph nine's
    /// highest corner is `(72, 106)`, a point below the rectangle's floor. So
    /// exactly two glyphs go, and the last of them only because the test
    /// followed the rotation.
    #[test]
    fn an_obliquely_rotated_run_is_cut_along_its_own_baseline() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 0.6 0.8 -0.8 0.6 20 20 Tm (PUBLICSECRET) Tj ET",
        ));
        assert!(all_streams(&doc).contains("PUBLICSECRET"));

        let corner = Redaction {
            area: Rect {
                x0: 73.0,
                y0: 107.0,
                x1: 200.0,
                y1: 200.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[corner]);
        assert_eq!(report.glyphs, 2, "the last two glyphs and no others");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(
            !streams.contains("PUBLICSECRET"),
            "the run was cut: {streams}"
        );
        assert!(
            streams.contains("PUBLICSECR"),
            "and cut in exactly one place: {streams}"
        );

        assert_eq!(
            ink_in(&render(bytes), 200.0, corner.area),
            0,
            "no ink survives inside the redacted rectangle"
        );
    }

    /// A skew, where the two edges of a glyph box are not perpendicular and
    /// the box's own extent is not its bounding box.
    ///
    /// `1 0 2 1 20 100 Tm` leans the em box two units right for every one up:
    /// glyph `k` of a ten-point run spans page x `10k+20 .. 10k+30` at its
    /// baseline and `10k+40 .. 10k+50` at its top. The rectangle is the
    /// quarter-plane `x >= 62, y >= 105`. Glyphs four and five reach it even
    /// if the lean is ignored; glyphs two and three reach it **only** through
    /// the lean — their upright boxes stop at x 50 and x 60. Four glyphs go,
    /// and a build that read the box upright would cut two.
    #[test]
    fn a_skewed_run_is_cut_along_its_own_baseline() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 1 0 2 1 20 100 Tm (SECRET) Tj ET",
        ));
        assert!(all_streams(&doc).contains("SECRET"));

        let leaning = Redaction {
            area: Rect {
                x0: 62.0,
                y0: 105.0,
                x1: 200.0,
                y1: 200.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[leaning]);
        assert_eq!(
            report.glyphs, 4,
            "the four glyphs whose leaning boxes reach the rectangle"
        );

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("CRET"), "got: {streams}");

        assert_eq!(
            ink_in(&render(bytes), 200.0, leaning.area),
            0,
            "no ink survives inside the redacted rectangle"
        );
    }

    /// A rotated run is measured through the `cm` in force as well as through
    /// its own matrix, because the rectangle is in page space and neither
    /// matrix alone gets there.
    ///
    /// The same quarter turn as above, drawn under `1 0 0 1 0 -60 cm`, which
    /// slides the whole run sixty points down the page. `SECRET` now occupies
    /// page y 20..80, and the band that used to take it takes nothing.
    #[test]
    fn a_rotated_run_is_measured_through_the_transform_too() {
        let shifted = boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "q 1 0 0 1 0 -60 cm BT /F0 10 Tf 0 1 -1 0 100 20 Tm (PUBLICSECRET) Tj ET Q",
        );

        let (_, high) = redact(open(shifted.clone()), &[upper_band()]);
        assert_eq!(high.glyphs, 0, "the run moved out from under the band");

        let lowered = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 22.0,
                x1: 105.0,
                y1: 85.0,
            },
            mark: false,
        };
        let (bytes, low) = redact(open(shifted), &[lowered]);
        assert_eq!(low.glyphs, 6, "and is under the band that followed it");
        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
    }

    /// The rotation can live in the `cm` while the `Tm` is perfectly ordinary,
    /// and the run is still rotated on the page.
    ///
    /// This is the hole the old refusal had. It carried one `skewed` flag,
    /// `cm` could only set it and `Tm` *assigned* it — so a rotated `cm`
    /// followed by an axis-aligned `Tm` cleared the flag and the run was cut
    /// by a pen walking along the page's x axis under a quarter turn. Not a
    /// refusal that was too broad: a refusal that was not there, on a run it
    /// then cut from positions ninety degrees out.
    ///
    /// `0 1 -1 0 200 0 cm` maps `(x, y)` to `(200 - y, x)` and the `Tm` puts
    /// the run's origin at text `(20, 20)`, so unscaled text space `(x, y)`
    /// lands at page `(180 - y, x + 20)`: the run climbs the page at
    /// x 170..180, glyph `k` at y `20 + 10k` .. `30 + 10k`, and `SECRET` is
    /// y 80..140 again.
    #[test]
    fn a_rotation_that_lives_in_the_transform_is_followed_too() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "q 0 1 -1 0 200 0 cm BT /F0 10 Tf 1 0 0 1 20 20 Tm (PUBLICSECRET) Tj ET Q",
        ));

        let band = Redaction {
            area: Rect {
                x0: 165.0,
                y0: 82.0,
                x1: 185.0,
                y1: 145.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[band]);
        assert_eq!(report.glyphs, 6, "the six glyphs of the second word");
        assert!(report.warnings.is_empty(), "nothing was refused");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(streams.contains("PUBLIC"), "got: {streams}");

        assert_eq!(
            ink_in(&render(bytes), 200.0, band.area),
            0,
            "no ink survives inside the redacted rectangle"
        );
    }

    /// A `TJ` number in a rotated run keeps its own meaning, and a `Tm` that
    /// carries the font size does not multiply into the replacement gap.
    ///
    /// `/F0 1 Tf` with `0 10 -10 0 100 20 Tm` is the same ten-point quarter
    /// turn written the other ordinary way: the size is in the matrix rather
    /// than in the `Tf`. 9.4.3 measures a `TJ` number in thousandths of
    /// `Tfs · Th` — which is one here, not ten — so a gap re-emitted with the
    /// matrix's scale folded in would be ten times too long and the tail of
    /// the run would fly off the page.
    #[test]
    fn a_gap_is_re_emitted_in_the_runs_own_units() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 1 Tf 0 10 -10 0 100 20 Tm (PUBLICSECRET) Tj ET",
        ));

        // Glyph three again: page y 50..60, exactly as in the `10 Tf` form.
        let band = Redaction {
            area: Rect {
                x0: 85.0,
                y0: 51.0,
                x1: 105.0,
                y1: 59.0,
            },
            mark: false,
        };
        let (bytes, report) = redact(doc, &[band]);
        assert_eq!(report.glyphs, 1, "the matrix placed the glyphs");

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, band.area),
            0,
            "the cut glyph is gone"
        );
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 92.0,
                    y0: 132.0,
                    x1: 98.0,
                    y1: 138.0,
                }
            ) > 20,
            "and the end of the run is still on the page where it was"
        );
    }

    /// The font is part of the graphics state `q` saves, so a `Tf` inside a
    /// `q`/`Q` pair does not survive the `Q`.
    ///
    /// The size was already being restored and the face was not, which meant
    /// the advances after a `Q` were computed from one font's widths at
    /// another font's size — positions that are wrong by however much the two
    /// faces differ, which is the "approximately right" cut this whole module
    /// refuses.
    ///
    /// Inside the pair the font is a Type 3 face with a non-default
    /// `/FontMatrix`, which redaction refuses to measure. The run after the
    /// `Q` names no font of its own — the `Tf` before the `q` is the one that
    /// still applies — so a selection that leaked past the `Q` would refuse
    /// the outer run too and cut nothing.
    #[test]
    fn a_font_selected_inside_a_q_does_not_outlive_it() {
        let doc = open(two_font_document(
            "/F0 10 Tf
             q /F1 10 Tf BT 10 150 Td (INSIDE) Tj ET Q
             BT 0 1 -1 0 100 20 Tm (PUBLICSECRET) Tj ET",
        ));

        let (_, report) = redact(doc, &[upper_band()]);
        assert_eq!(report.glyphs, 6, "the outer run was measured and cut");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::RescaledType3Font {
                font: b"F1".to_vec(),
                bytes: 6,
            }],
            "and only the inner run was refused"
        );
    }
}

/// Runs redaction still refuses to measure, each left whole and named.
///
/// A redaction that silently fails to redact is worse than one that refuses:
/// the caller believes the content is gone and distributes the file. So the
/// question each of these asks is the same — was the text left **and** was the
/// caller told — and both halves are asserted, because a refusal nobody hears
/// is an under-redaction with a clean report.
#[cfg(test)]
mod refusals {
    use super::tests_support::*;
    use super::*;

    /// Everything on the page, so that nothing escapes by being outside.
    fn everywhere() -> Redaction {
        Redaction {
            area: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 200.0,
                y1: 200.0,
            },
            mark: false,
        }
    }

    /// 9.4.4's vertical branch: the pen advances downward by `/W2`'s `w1`,
    /// and a `TJ` number displaces along that axis too.
    ///
    /// This is the class that arrived *with* the rotation cut rather than
    /// surviving it. A vertical run's text matrix is perfectly ordinary, so
    /// the old rotation guard never looked at it, and every vertical run on
    /// every page was being measured left to right and cut from the result.
    #[test]
    fn a_vertical_run_is_left_uncut_and_reported() {
        let doc = open(vertical_document("BT /F0 10 Tf 100 100 Td (SECRET) Tj ET"));
        assert!(all_streams(&doc).contains("SECRET"));

        let (bytes, report) = redact(doc, &[everywhere()]);
        assert_eq!(report.glyphs, 0, "nothing was cut");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::VerticalRun {
                font: b"F0".to_vec(),
                bytes: 6,
            }],
            "and the caller was told which font and how much"
        );

        let streams = all_streams(&CosDocument::open(bytes).expect("it reopens"));
        assert!(
            streams.contains("SECRET"),
            "the run is intact rather than half-removed: {streams}"
        );
    }

    /// 9.6.5: a Type 3 font's `/Widths` are in its own glyph space.
    ///
    /// `/FontMatrix [0.01 0 0 0.01 0 0]` makes every advance ten times what
    /// this module's `width / 1000` computes, so the third glyph is already a
    /// full em from where the rectangle thinks it is.
    #[test]
    fn a_rescaled_type3_font_is_left_uncut_and_reported() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            "[0.01 0 0 0.01 0 0]",
            "BT /F0 10 Tf 10 100 Td (SECRET) Tj ET",
        ));
        assert!(all_streams(&doc).contains("SECRET"));

        let (bytes, report) = redact(doc, &[everywhere()]);
        assert_eq!(report.glyphs, 0, "nothing was cut");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::RescaledType3Font {
                font: b"F0".to_vec(),
                bytes: 6,
            }]
        );
        assert!(all_streams(&CosDocument::open(bytes).expect("it reopens")).contains("SECRET"));
    }

    /// The same font with the conventional matrix is measured and cut, which
    /// is what says the refusal is about the matrix and not about Type 3.
    #[test]
    fn a_type3_font_with_the_conventional_matrix_is_cut() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 10 100 Td (SECRET) Tj ET",
        ));
        let (_, report) = redact(doc, &[everywhere()]);
        assert_eq!(report.glyphs, 6);
        assert!(report.warnings.is_empty());
    }

    /// A `Tf` naming a font the resource dictionary does not have leaves the
    /// run with no metrics at all.
    ///
    /// This was silent: the run was kept, nothing was counted, and the report
    /// was indistinguishable from a page where the rectangle covered nothing.
    #[test]
    fn a_run_whose_font_is_not_in_scope_is_left_uncut_and_reported() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /Missing 10 Tf 10 100 Td (SECRET) Tj ET",
        ));

        let (bytes, report) = redact(doc, &[everywhere()]);
        assert_eq!(report.glyphs, 0);
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::UnknownFont {
                font: b"Missing".to_vec(),
                bytes: 6,
            }]
        );
        assert!(all_streams(&CosDocument::open(bytes).expect("it reopens")).contains("SECRET"));
    }

    /// A text rendering matrix with an entry that is not a number.
    ///
    /// A single operand cannot deliver one: every operand this module reads is
    /// rejected unless it is finite. It arrives instead the way a real file
    /// would deliver it, out of the *composition* — a text matrix scaled by
    /// 10^300 under a `cm` scaled by 10^300, each perfectly readable on its
    /// own, whose product is not a number. Nothing can be measured against a
    /// rectangle from there, and the run is left **whole**: one abandoned
    /// part-way through would be a run cut from positions this module had
    /// already decided it could not trust.
    #[test]
    fn a_non_finite_text_matrix_is_left_uncut_and_reported() {
        let huge = format!("1{}", "0".repeat(300));
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            &format!(
                "q {huge} 0 0 {huge} 0 0 cm \
                 BT /F0 10 Tf {huge} 0 0 {huge} 10 100 Tm (SECRET) Tj ET Q"
            ),
        ));

        let (bytes, report) = redact(doc, &[everywhere()]);
        assert_eq!(report.glyphs, 0, "nothing was cut");
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::UnmeasurableFrame {
                font: b"F0".to_vec(),
                bytes: 6,
            }]
        );
        assert!(all_streams(&CosDocument::open(bytes).expect("it reopens")).contains("SECRET"));
    }

    /// A refusal is about a rectangle, so with no rectangles there is nothing
    /// to refuse. Warning on every vertical run of every page a caller merely
    /// opened would make the list say nothing.
    #[test]
    fn nothing_is_refused_when_there_is_nothing_to_redact() {
        let doc = open(vertical_document("BT /F0 10 Tf 100 100 Td (SECRET) Tj ET"));
        let (_, report) = redact(doc, &[]);
        assert_eq!(report, RedactionReport::default());
    }

    /// Warnings with the same cause and the same font merge, so a page of
    /// vertical text yields one entry rather than one per operator.
    #[test]
    fn refusals_of_the_same_cause_and_font_merge() {
        let doc = open(vertical_document(
            "BT /F0 10 Tf 100 100 Td (SECRET) Tj 0 -12 Td (AGAIN) Tj ET",
        ));
        let (_, report) = redact(doc, &[everywhere()]);
        assert_eq!(
            report.warnings,
            vec![RedactionWarning::VerticalRun {
                font: b"F0".to_vec(),
                bytes: 11,
            }],
            "one entry, carrying both runs' operand lengths"
        );
    }
}

/// A cut run is re-emitted as `TJ`, and what the original operator did
/// *besides* showing has to survive that.
///
/// Every failure here is the same one, and it is the failure the rest of this
/// module exists to refuse: a redaction that removed the right glyphs and
/// moved the wrong ones. The needle-bytes-absent check cannot see it, because
/// the bytes that had to go are gone; what catches it is where the rest of the
/// run lands, which each of these asserts twice — once in the rewritten
/// stream and once in the ink.
#[cfg(test)]
mod showing_operators {
    use super::tests_support::*;
    use super::*;

    /// `'` is `T*` then `Tj` (9.4.3). The line advance has to survive the cut.
    #[test]
    fn a_cut_quote_keeps_its_line_advance() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 20 TL 20 150 Td (PUBLIC) Tj (SECRET) ' ET",
        ));

        // Glyph zero of the second line: page x 20..30, y 130..140. The
        // first line's baseline is twenty points above it.
        let first_glyph = Redaction {
            area: Rect {
                x0: 18.0,
                y0: 131.0,
                x1: 29.0,
                y1: 139.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[first_glyph]);
        assert_eq!(report.glyphs, 1, "the S of the second line alone");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "the glyph went: {streams}");
        assert!(
            streams.contains("T*"),
            "and the line advance stayed: {streams}"
        );

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, first_glyph.area),
            0,
            "no ink where the cut glyph was"
        );
        // The E that followed it, still on the second line rather than
        // wherever the first line's pen had got to.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 31.0,
                    y0: 131.0,
                    x1: 39.0,
                    y1: 139.0,
                }
            ) > 20,
            "the rest of the run is still on its own line"
        );
    }

    /// `aw ac string "` is `aw Tw ac Tc` then `'` (9.4.3). Both spacings have
    /// to survive the cut, and the character spacing is visible in the
    /// geometry: at `5 Tc` the run's glyphs stand fifteen points apart rather
    /// than ten, so the last of six sits twenty points further along.
    #[test]
    fn a_cut_double_quote_keeps_both_spacings() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 20 TL 20 150 Td (PUBLIC) Tj 1 5 (SECRET) \" ET",
        ));

        let first_glyph = Redaction {
            area: Rect {
                x0: 18.0,
                y0: 131.0,
                x1: 29.0,
                y1: 139.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[first_glyph]);
        assert_eq!(report.glyphs, 1);

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(
            streams.contains("1 Tw 5 Tc"),
            "both spacings were re-emitted: {streams}"
        );

        // Glyph five of the second line: page x 95..105 at `5 Tc`, and
        // x 75..85 without it.
        assert!(
            ink_in(
                &render(bytes),
                200.0,
                Rect {
                    x0: 97.0,
                    y0: 131.0,
                    x1: 103.0,
                    y1: 139.0,
                }
            ) > 20,
            "the character spacing placed the tail of the run"
        );
    }

    /// An adjustment the original `TJ` array already carried is re-emitted
    /// with its own sign and its own magnitude.
    ///
    /// 9.4.3 makes a `TJ` number move the pen *backwards* by its value in
    /// thousandths, so `-2000` is twenty points forward at ten point. A
    /// rewrite that re-emits it as `2000` moves twenty points back instead,
    /// and every glyph after it lands forty points from where it was — which
    /// is a redaction that cut the right glyph and relocated the rest of the
    /// line.
    #[test]
    fn an_existing_tj_adjustment_keeps_its_sign_and_size() {
        let doc = open(boxed_glyph_document(
            200.0,
            200.0,
            DEFAULT_FONT_MATRIX,
            "BT /F0 10 Tf 20 100 Td [(PUB) -2000 (SECRET)] TJ ET",
        ));

        // `PUB` is page x 20..50; the adjustment carries the pen to x 70, so
        // `SECRET` is x 70..130 and its S is x 70..80.
        let first_glyph = Redaction {
            area: Rect {
                x0: 68.0,
                y0: 101.0,
                x1: 79.0,
                y1: 109.0,
            },
            mark: false,
        };

        let (bytes, report) = redact(doc, &[first_glyph]);
        assert_eq!(report.glyphs, 1, "the S alone");

        let streams = all_streams(&CosDocument::open(bytes.clone()).expect("it reopens"));
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(
            streams.contains("-2000"),
            "the original adjustment came back as it went in: {streams}"
        );

        let bitmap = render(bytes);
        assert_eq!(
            ink_in(&bitmap, 200.0, first_glyph.area),
            0,
            "no ink where the cut glyph was"
        );
        // The T that ends the run, still at page x 120..130.
        assert!(
            ink_in(
                &bitmap,
                200.0,
                Rect {
                    x0: 122.0,
                    y0: 102.0,
                    x1: 128.0,
                    y1: 108.0,
                }
            ) > 20,
            "the tail of the run did not move"
        );
    }
}

/// Fixtures whose glyphs are boxes, so that measured geometry and drawn ink
/// are the same rectangle.
///
/// A Type 3 font is what makes the ink assertions possible without a font
/// file: its glyphs are content streams, so `1000 0 d0 0 0 1000 1000 re f`
/// fills the whole em square and a ten-point glyph is exactly a ten-point
/// black box. With `/FontMatrix [0.001 0 0 0.001 0 0]` — 9.6.5's conventional
/// glyph space, and the one every other font kind has implicitly — a
/// `/Widths` entry of 1000 is one em, which is the same arithmetic redaction
/// does for a Type 1 or TrueType face. So the fixture is not a special case
/// of the code under test; it is the ordinary case with visible geometry.
///
/// Redaction rewrites the *page's* showing operators, so the glyph procedures
/// are never entered for a code that was removed. Text drawn **inside** a
/// glyph procedure is a different question and is still refused — see
/// `docs/features/editing.md`.
#[cfg(test)]
mod tests_support {
    use super::*;

    /// 9.6.5's conventional Type 3 glyph space, and the one every other font
    /// kind has implicitly.
    pub const DEFAULT_FONT_MATRIX: &str = "[0.001 0 0 0.001 0 0]";

    pub fn open(bytes: Vec<u8>) -> Arc<CosDocument> {
        Arc::new(CosDocument::open(bytes).expect("it opens"))
    }

    pub fn all_streams(doc: &CosDocument) -> String {
        let mut out = Vec::new();
        for (number, _) in doc.xref().iter() {
            if let Ok(bytes) = doc.stream_decoded(ObjRef::new(number, 0)) {
                out.extend_from_slice(&bytes);
                out.push(b'\n');
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    pub fn redact(doc: Arc<CosDocument>, areas: &[Redaction]) -> (Vec<u8>, RedactionReport) {
        let mut editor = DocumentEditor::new(doc);
        let report = apply(&mut editor, 0, areas).expect("the page exists");
        let bytes = editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            ..tinker_pdf_cos::WriteOptions::default()
        });
        (bytes, report)
    }

    pub fn render(bytes: Vec<u8>) -> crate::Bitmap {
        crate::Document::open(bytes)
            .expect("it reopens")
            .page(0)
            .expect("a page")
            .render(&crate::RenderOptions::default())
    }

    /// How many inked pixels fall inside a page-space rectangle.
    ///
    /// The render is one pixel per point with the page origin at the bottom
    /// left, so device rows count down from the page height. A pixel is
    /// counted by its centre, which keeps a rectangle's own edge from
    /// deciding the answer.
    pub fn ink_in(bitmap: &crate::Bitmap, page_height: f64, area: Rect) -> usize {
        let mut inked = 0;
        for y in 0..bitmap.height {
            let py = page_height - (f64::from(y) + 0.5);
            if py < area.y0 || py > area.y1 {
                continue;
            }
            for x in 0..bitmap.width {
                let px = f64::from(x) + 0.5;
                if px < area.x0 || px > area.x1 {
                    continue;
                }
                let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
                if bitmap.data.get(at).copied().unwrap_or(255) < 200 {
                    inked += 1;
                }
            }
        }
        inked
    }

    /// The `/Differences` and `/Widths` for codes 65..=90, every one of them
    /// the same box-filling procedure one em wide.
    fn every_letter() -> (String, String) {
        (
            format!("[65 {}]", ["/g"; 26].join(" ")),
            ["1000"; 26].join(" "),
        )
    }

    fn stream_object(number: u32, body: &str) -> String {
        format!(
            "{number} 0 obj\n<< /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
            body.len() + 1
        )
    }

    /// A one-page document with one Type 3 font whose every glyph fills its em
    /// square, and `content` as the page's content stream.
    pub fn boxed_glyph_document(
        width: f64,
        height: f64,
        font_matrix: &str,
        content: &str,
    ) -> Vec<u8> {
        let (differences, widths) = every_letter();
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(&format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n"
        ));
        out.push_str(&format!(
            "4 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
             /FontMatrix {font_matrix}\n\
             /CharProcs << /g 5 0 R >>\n\
             /Encoding << /Type /Encoding /Differences {differences} >>\n\
             /FirstChar 65 /LastChar 90 /Widths [{widths}]\n\
             /Resources << >> >>\nendobj\n"
        ));
        out.push_str(&stream_object(5, "1000 0 d0 0 0 1000 1000 re f"));
        out.push_str(&stream_object(7, content));
        out.push_str("trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// The same page with a second font, `/F1`, whose `/FontMatrix` redaction
    /// refuses to measure — so which font is in force is visible in the
    /// report rather than only in the geometry.
    pub fn two_font_document(content: &str) -> Vec<u8> {
        let (differences, widths) = every_letter();
        let font = |number: u32, matrix: &str| {
            format!(
                "{number} 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
                 /FontMatrix {matrix}\n\
                 /CharProcs << /g 5 0 R >>\n\
                 /Encoding << /Type /Encoding /Differences {differences} >>\n\
                 /FirstChar 65 /LastChar 90 /Widths [{widths}]\n\
                 /Resources << >> >>\nendobj\n"
            )
        };

        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
             /Resources << /Font << /F0 4 0 R /F1 8 0 R >> >> /Contents 7 0 R >>\nendobj\n",
        );
        out.push_str(&font(4, DEFAULT_FONT_MATRIX));
        out.push_str(&stream_object(5, "1000 0 d0 0 0 1000 1000 re f"));
        out.push_str(&stream_object(7, content));
        out.push_str(&font(8, "[0.01 0 0 0.01 0 0]"));
        out.push_str("trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    /// A one-page document whose font writes vertically (`/Identity-V`).
    ///
    /// A composite font, because writing mode is a property of the encoding
    /// CMap and only a composite font has one. `/DW` supplies the horizontal
    /// widths, which is exactly the trap: they are present and plausible, so
    /// a redaction that does not ask about the writing mode measures the run
    /// along the wrong axis and finds an answer.
    pub fn vertical_document(content: &str) -> Vec<u8> {
        let mut out = String::from("%PDF-1.7\n");
        out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.push_str(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n",
        );
        out.push_str(
            "4 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Boxed\n\
             /Encoding /Identity-V /DescendantFonts [5 0 R] >>\nendobj\n",
        );
        out.push_str(
            "5 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Boxed\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
             /DW 1000 >>\nendobj\n",
        );
        out.push_str(&stream_object(7, content));
        out.push_str("trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }
}

#[cfg(test)]
mod page_scope_tests {
    use super::*;
    use std::sync::Arc;
    use tinker_pdf_cos::CosDocument;

    /// Two pages, each with an image under the same resource name `/Im0`,
    /// and a redaction covering the whole of page one.
    fn two_pages() -> Vec<u8> {
        let content = "q 100 0 0 100 0 0 cm /Im0 Do Q";
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 2 /Kids [3 0 R 6 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {len} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
stream\nPAGE\nendstream\nendobj\n\
6 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << /XObject << /Im0 8 0 R >> >> /Contents 7 0 R >>\nendobj\n\
7 0 obj\n<< /Length {len} >>\nstream\n{content}\nendstream\nendobj\n\
8 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
stream\nSECR\nendstream\nendobj\n\
trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
            len = content.len()
        )
        .into_bytes()
    }

    /// Redacting page one must remove page one's image.
    ///
    /// This failed in the worst possible way: resource names were resolved
    /// against page *zero*, so redacting page one scrubbed page zero's `/Im0`
    /// — a name so common that the collision is the normal case — reported
    /// `images: 1`, and left the secret on page one untouched. It looked like
    /// it had worked.
    #[test]
    fn redacting_the_second_page_removes_the_second_pages_image() {
        let doc = Arc::new(CosDocument::open(two_pages()).expect("it opens"));
        let mut editor = DocumentEditor::new(doc);

        let report = apply(
            &mut editor,
            1,
            &[Redaction {
                area: tinker_pdf_cos::Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 100.0,
                    y1: 100.0,
                },
                mark: false,
            }],
        )
        .expect("it redacts");
        assert_eq!(report.images, 1, "it found page one's image");

        let saved = editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            compress: false,
            object_streams: false,
            ..tinker_pdf_cos::WriteOptions::default()
        });
        let text = String::from_utf8_lossy(&saved);

        assert!(
            !text.contains("SECR"),
            "page one's samples are gone from the file"
        );
        assert!(
            text.contains("PAGE"),
            "and page zero's image, which nobody asked about, is untouched"
        );
    }

    /// The mirror: redacting page zero must not reach page one.
    #[test]
    fn redacting_the_first_page_leaves_the_second_alone() {
        let doc = Arc::new(CosDocument::open(two_pages()).expect("it opens"));
        let mut editor = DocumentEditor::new(doc);

        apply(
            &mut editor,
            0,
            &[Redaction {
                area: tinker_pdf_cos::Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 100.0,
                    y1: 100.0,
                },
                mark: false,
            }],
        )
        .expect("it redacts");

        let saved = editor.save(&tinker_pdf_cos::WriteOptions {
            mode: tinker_pdf_cos::WriteMode::Rewrite,
            compress: false,
            object_streams: false,
            ..tinker_pdf_cos::WriteOptions::default()
        });
        let text = String::from_utf8_lossy(&saved);

        assert!(!text.contains("PAGE"), "page zero's samples are gone");
        assert!(text.contains("SECR"), "page one is untouched");
    }
}
