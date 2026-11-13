//! Colour glyphs into a document: a face's colour tables turned into the
//! Type 3 procedures `DocumentBuilder` writes (`docs/design/colour-fonts.md`).
//!
//! Feature documentation: `docs/features/fonts.md`.
//!
//! A [`ColourFace`] reads a face's `COLR` and `CPAL` once, under the palette
//! its caller chose, and turns each glyph a run draws into a procedure in the
//! face's own units: the first of the design's tables that describes the
//! glyph (*Which table draws a glyph*) — `COLR` version 0's layers, each
//! layer's outline filled in turn with its palette colour and alpha, a
//! foreground layer left to the text's own fill colour — and, for a glyph no
//! table describes, its outline unfilled by any colour of its own, so it
//! paints in the text's. [`crate::shaping::write_run`] writes a run through
//! one where the run names it, and the procedures become a glyph's form in
//! the face's Type 3 fonts (`DocumentBuilder::add_colour_glyph`).
//!
//! A glyph is drawn as one object: its form is a transparency group, so the
//! alpha of the text it is shown in fades the glyph whole. A layer's own alpha
//! is an `/ExtGState` the face registers once per value; under a profile that
//! forbids transparency (ISO 19005-1 6.4) such a glyph is drawn as its outline
//! instead and named, [`ColourWarning::TransparencyForbidden`] (ruling 10).
//!
//! Every number reaches the content through one rounding to a ten-thousandth
//! of a font unit, so the procedures are the same bytes on every target
//! (ruling 4).

use std::cell::RefCell;
use std::collections::BTreeSet;

use tinker_pdf_cos::build::{ColourGlyph, DocumentBuilder, ExtGState};
use tinker_pdf_cos::ArchivalPart;
use tinker_pdf_font::{
    glyf, Cff, Colr, Cpal, Layer, LayerColour, Outline, PaletteChoice, Segment, Sfnt,
};

/// `glyf`, whose outlines a TrueType face carries.
const GLYF: u32 = u32::from_be_bytes(*b"glyf");
/// `CFF `, whose outlines an `OTTO` face carries.
const CFF: u32 = u32::from_be_bytes(*b"CFF ");
/// `COLR`.
const COLR: u32 = u32::from_be_bytes(*b"COLR");
/// `CPAL`.
const CPAL: u32 = u32::from_be_bytes(*b"CPAL");

/// What a colour glyph could not be drawn as its tables describe it, and what
/// it was drawn as instead (ruling 10). Each names the glyph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ColourWarning {
    /// The glyph's description needs transparency — an alpha below one —
    /// and the document's profile forbids it (ISO 19005-1 6.4): drawn as its
    /// outline, in the text's colour.
    TransparencyForbidden {
        /// The glyph.
        glyph: u16,
    },
}

/// A face's colour glyphs, read under one palette, for one document.
///
/// One per face and palette, as the design has one set of Type 3 fonts per
/// face and palette: register it under its own resource name with
/// [`ColourFace::register`], and name it on every [`crate::shaping::Run`] in
/// that resource. It remembers the `/ExtGState`s it registered with the
/// builder it serves, so it serves one builder.
pub struct ColourFace<'a> {
    sfnt: &'a Sfnt<'a>,
    colr: Option<Colr<'a>>,
    cpal: Option<Cpal<'a>>,
    palette: u16,
    /// The layer alphas whose `/ExtGState` this face has registered.
    alphas: RefCell<BTreeSet<u8>>,
    /// What drawing its glyphs met, each once, in the order met.
    warnings: RefCell<Vec<ColourWarning>>,
}

impl core::fmt::Debug for ColourFace<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ColourFace")
            .field("colr", &self.colr.as_ref().map(Colr::version))
            .field("cpal", &self.cpal.is_some())
            .field("palette", &self.palette)
            .finish_non_exhaustive()
    }
}

/// One glyph's procedure: what it draws, in font units, and the box it draws
/// inside.
struct Drawn {
    content: Vec<u8>,
    bbox: Option<[f64; 4]>,
}

impl<'a> ColourFace<'a> {
    /// The colour glyphs of `sfnt`, in the palette `palette` chooses.
    ///
    /// A `COLR` with no `CPAL` beside it is ignored, as the COLR chapter's
    /// *Overview* says, and a face with neither draws every glyph as its
    /// outline.
    #[must_use]
    pub fn new(sfnt: &'a Sfnt<'a>, palette: PaletteChoice) -> ColourFace<'a> {
        let cpal = sfnt.table(CPAL).and_then(Cpal::parse);
        let colr = cpal.as_ref().and(sfnt.table(COLR)).and_then(Colr::parse);
        let palette = cpal.as_ref().map_or(0, |cpal| cpal.choose(palette));
        ColourFace {
            sfnt,
            colr,
            cpal,
            palette,
            alphas: RefCell::new(BTreeSet::new()),
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Whether the face carries colour glyphs this module draws.
    #[must_use]
    pub fn has_colour(&self) -> bool {
        self.colr.is_some()
    }

    /// The palette the glyphs are drawn in.
    #[must_use]
    pub fn palette(&self) -> u16 {
        self.palette
    }

    /// Registers the face with `builder` as a colour face under `resource`,
    /// its descriptors naming `name` (`DocumentBuilder::add_colour_font`).
    pub fn register(&self, builder: &mut DocumentBuilder, resource: &[u8], name: &[u8]) -> bool {
        builder.add_colour_font(resource, name, self.sfnt.units_per_em.max(1))
    }

    /// What drawing the face's glyphs met so far, each once, in the order
    /// met.
    #[must_use]
    pub fn warnings(&self) -> Vec<ColourWarning> {
        self.warnings.borrow().clone()
    }

    fn warn(&self, warning: ColourWarning) {
        let mut warnings = self.warnings.borrow_mut();
        if !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }

    /// Adds the procedure of every glyph of `glyphs` the face registered
    /// under `resource` has none for yet, so a run of them can be written.
    ///
    /// Returns false where the builder refused a procedure — `resource` is no
    /// colour face of it.
    pub fn add_glyphs(
        &self,
        builder: &mut DocumentBuilder,
        resource: &[u8],
        glyphs: impl IntoIterator<Item = u16>,
    ) -> bool {
        for glyph in glyphs {
            if builder.has_colour_glyph(resource, glyph) {
                continue;
            }
            let drawn = self.procedure(builder, resource, glyph);
            let advance = self.sfnt.advance(glyph).map_or(0.0, f64::from);
            let added = builder.add_colour_glyph(
                resource,
                &ColourGlyph {
                    id: glyph,
                    advance,
                    bbox: drawn.bbox.unwrap_or([0.0; 4]),
                    content: if drawn.bbox.is_some() {
                        &drawn.content
                    } else {
                        b""
                    },
                },
            );
            if !added {
                return false;
            }
        }
        true
    }

    /// `glyph`'s procedure, from the first table that describes it.
    fn procedure(&self, builder: &mut DocumentBuilder, resource: &[u8], glyph: u16) -> Drawn {
        if let (Some(colr), Some(cpal)) = (&self.colr, &self.cpal) {
            if let Some(layers) = colr.layers(glyph, cpal, self.palette) {
                if let Some(drawn) = self.layers(builder, resource, glyph, &layers) {
                    return drawn;
                }
            }
        }
        self.plain(glyph)
    }

    /// The glyph's outline, filled in whatever colour the text is shown in.
    fn plain(&self, glyph: u16) -> Drawn {
        let mut content = Vec::new();
        let Some(outline) = outline_of(self.sfnt, glyph).filter(|o| !o.is_empty()) else {
            return Drawn {
                content,
                bbox: None,
            };
        };
        path(&mut content, &outline);
        content.extend_from_slice(b"f\n");
        Drawn {
            content,
            bbox: box_of(&outline),
        }
    }

    /// COLR version 0's layers, bottom first (*BaseGlyph and Layer records*):
    /// each layer's outline filled under `q`/`Q` in its palette colour, its
    /// alpha through an `/ExtGState`, a foreground layer setting no colour.
    /// Layers composite by simple alpha compositing (*Compositing and
    /// blending*), which is ISO 32000-2 11.3.3's formula under `Normal`.
    ///
    /// `None` where an alpha is needed and the profile forbids it, which the
    /// caller draws as the outline.
    fn layers(
        &self,
        builder: &mut DocumentBuilder,
        resource: &[u8],
        glyph: u16,
        layers: &[Layer],
    ) -> Option<Drawn> {
        let mut content = Vec::new();
        let mut bbox: Option<[f64; 4]> = None;
        for layer in layers {
            let Some(outline) = outline_of(self.sfnt, layer.glyph).filter(|o| !o.is_empty()) else {
                continue;
            };
            content.extend_from_slice(b"q\n");
            if let LayerColour::Colour(colour) = layer.colour {
                if colour.alpha < 255 {
                    let state = self.alpha_state(builder, resource, glyph, colour.alpha)?;
                    crate::form_data::write_name(&mut content, &state);
                    content.extend_from_slice(b" gs\n");
                }
                rgb(&mut content, [colour.red, colour.green, colour.blue]);
            }
            path(&mut content, &outline);
            content.extend_from_slice(b"f\nQ\n");
            bbox = union(bbox, box_of(&outline));
        }
        Some(Drawn { content, bbox })
    }

    /// The `/ExtGState` setting the non-stroking alpha to `alpha` over 255,
    /// registered once per value; `None`, warning, where the profile forbids
    /// transparency.
    fn alpha_state(
        &self,
        builder: &mut DocumentBuilder,
        resource: &[u8],
        glyph: u16,
        alpha: u8,
    ) -> Option<Vec<u8>> {
        if forbids_transparency(builder) {
            self.warn(ColourWarning::TransparencyForbidden { glyph });
            return None;
        }
        let mut name = resource.to_vec();
        name.extend_from_slice(format!(".ca{alpha}").as_bytes());
        if self.alphas.borrow().contains(&alpha) {
            return Some(name);
        }
        let state = ExtGState {
            fill_alpha: Some(f64::from(alpha) / 255.0),
            ..ExtGState::default()
        };
        if !builder.add_ext_gstate(&name, &state) {
            return None;
        }
        self.alphas.borrow_mut().insert(alpha);
        Some(name)
    }
}

/// Whether `builder` writes under a profile that admits no transparency:
/// ISO 19005-1's, part 1.
fn forbids_transparency(builder: &DocumentBuilder) -> bool {
    builder
        .profile()
        .is_some_and(|profile| profile.part == ArchivalPart::One)
}

/// A glyph's outline, from `glyf` or, in a face without one, from `CFF `.
fn outline_of(sfnt: &Sfnt<'_>, glyph: u16) -> Option<Outline> {
    if sfnt.table(GLYF).is_some() {
        return glyf::outline(sfnt, glyph);
    }
    Cff::parse(sfnt.table(CFF)?)?.outline(glyph)
}

/// An outline's box, as `[x0 y0 x1 y1]`, where it has an area.
fn box_of(outline: &Outline) -> Option<[f64; 4]> {
    let (x0, y0, x1, y1) = outline.bounds()?;
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

/// The union of two boxes, either of which may be absent.
fn union(left: Option<[f64; 4]>, right: Option<[f64; 4]>) -> Option<[f64; 4]> {
    match (left, right) {
        (Some(a), Some(b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]),
        (one, None) | (None, one) => one,
    }
}

/// One number, rounded to a ten-thousandth, as the builder writes a content
/// stream's: no exponent, and no `-0`.
fn number(out: &mut Vec<u8>, value: f64) {
    let rounded = (value * 1.0e4).round() / 1.0e4;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    out.extend_from_slice(format!("{rounded}").as_bytes());
}

/// Numbers and an operator, space-separated.
fn operator(out: &mut Vec<u8>, values: &[f64], op: &[u8]) {
    for value in values {
        number(out, *value);
        out.push(b' ');
    }
    out.extend_from_slice(op);
    out.push(b'\n');
}

/// A non-stroking colour, from eight-bit components, as `rg`.
fn rgb(out: &mut Vec<u8>, components: [u8; 3]) {
    operator(out, &components.map(|c| f64::from(c) / 255.0), b"rg");
}

/// An outline as path construction operators in font units (8.5.2): a
/// TrueType quadratic raised to the cubic `c` draws, its controls two thirds
/// of the way from each end to the quadratic's.
fn path(out: &mut Vec<u8>, outline: &Outline) {
    let mut at = (0.0f64, 0.0f64);
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                operator(out, &[x, y], b"m");
                at = (x, y);
            }
            Segment::LineTo { x, y } => {
                operator(out, &[x, y], b"l");
                at = (x, y);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                let c1 = (
                    at.0 + 2.0 / 3.0 * (cx - at.0),
                    at.1 + 2.0 / 3.0 * (cy - at.1),
                );
                let c2 = (x + 2.0 / 3.0 * (cx - x), y + 2.0 / 3.0 * (cy - y));
                operator(out, &[c1.0, c1.1, c2.0, c2.1, x, y], b"c");
                at = (x, y);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                operator(out, &[c1x, c1y, c2x, c2y, x, y], b"c");
                at = (x, y);
            }
            Segment::Close => out.extend_from_slice(b"h\n"),
        }
    }
}
