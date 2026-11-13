//! Colour glyphs into a document: a face's colour tables turned into the
//! Type 3 procedures `DocumentBuilder` writes (`docs/design/colour-fonts.md`).
//!
//! Feature documentation: `docs/features/fonts.md`.
//!
//! A [`ColourFace`] reads a face's `COLR` and `CPAL` once, under the palette
//! its caller chose, and turns each glyph a run draws into a procedure in the
//! face's own units, from the first of the design's tables that describes the
//! glyph (*Which table draws a glyph*):
//!
//! 1. `COLR` version 1's paint graph, each paint as the design's construct:
//!    a layer list drawn bottom first, a solid fill of the clip region in its
//!    palette colour and alpha, a glyph's outline as the clip of its child, a
//!    `PaintColrGlyph` as the named glyph's graph in place, the transform
//!    family as `cm`, and the glyph's clip box clipping the whole. A gradient
//!    is refused by name and the glyph falls to the next table; a composite is
//!    drawn source-over and named with its mode.
//! 2. `COLR` version 0's layers, each layer's outline filled in turn with its
//!    palette colour and alpha.
//! 3. Its outline, unfilled by any colour of its own, so it paints in the
//!    text's.
//!
//! A foreground colour sets no colour at all, so it paints in whatever colour
//! the text is shown in. [`crate::shaping::write_run`] writes a run through a
//! face where the run names it, and the procedures become a glyph's form in
//! the face's Type 3 fonts (`DocumentBuilder::add_colour_glyph`).
//!
//! A glyph is drawn as one object: its form is a transparency group, so the
//! alpha of the text it is shown in fades the glyph whole. A paint's own alpha
//! is an `/ExtGState` the face registers once per eight-bit value; under a
//! profile that forbids transparency (ISO 19005-1 6.4) such a glyph is drawn
//! as its next table, or its outline, and named (ruling 10).
//!
//! A version 1 node with several parents that draws nothing outside its own
//! glyph outlines is written once, as a form each parent draws, so a graph
//! that fans one table out to many parents writes its tables and not its
//! paths; and every walk is held to [`MAX_DRAWN_PAINTS`] paints, past which
//! the glyph is drawn as its next table and named (ruling 1).
//!
//! Every number reaches the content through one rounding to a ten-thousandth
//! of a font unit, and every angle reaches a matrix through
//! `tinker_pdf_svg::transform`'s `tinker-pdf-math` helpers, so the procedures
//! are the same bytes on every target (ruling 4).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_cos::build::{ColourGlyph, DocumentBuilder, ExtGState, FormXObject};
use tinker_pdf_cos::ArchivalPart;
use tinker_pdf_font::colr::{
    f2dot14, fixed, ClipBox, ColrWarning, CompositeMode, Paint, PaintGraph, PaintId,
};
use tinker_pdf_font::{
    glyf, Cff, ColourIndex, Colr, Cpal, Layer, LayerColour, Outline, PaletteChoice, Segment, Sfnt,
};
use tinker_pdf_svg::transform;

/// `glyf`, whose outlines a TrueType face carries.
const GLYF: u32 = u32::from_be_bytes(*b"glyf");
/// `CFF `, whose outlines an `OTTO` face carries.
const CFF: u32 = u32::from_be_bytes(*b"CFF ");
/// `COLR`.
const COLR: u32 = u32::from_be_bytes(*b"COLR");
/// `CPAL`.
const CPAL: u32 = u32::from_be_bytes(*b"CPAL");

/// The most paints one glyph's version 1 graph is walked through, counting a
/// node each time a path reaches it outside a shared form.
///
/// The font crate holds a graph to `MAX_PAINT_NODES` tables and its paths to
/// `MAX_PAINT_DEPTH`, which bounds the tables and not the paths: a layer list
/// naming the same table twice at each of sixty-four levels is sixty-four
/// tables and 2^64 paths. A shared table that draws only inside its own
/// outlines is one form however many paths reach it; one that does not is
/// drawn where it stands, and this caps those walks.
pub const MAX_DRAWN_PAINTS: usize = 1 << 16;

/// What a colour glyph could not be drawn as its tables describe it, and what
/// it was drawn as instead (ruling 10). Each names the glyph.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ColourWarning {
    /// The glyph's description needs transparency — an alpha below one —
    /// and the document's profile forbids it (ISO 19005-1 6.4): drawn as its
    /// next table, or its outline in the text's colour.
    TransparencyForbidden {
        /// The glyph.
        glyph: u16,
    },
    /// A `COLR` version 1 gradient, which this build does not draw yet: the
    /// glyph is drawn as its next table, or its outline.
    GradientNotDrawn {
        /// The glyph.
        glyph: u16,
        /// The paint format: 4 to 9.
        format: u8,
    },
    /// A `COLR` version 1 composite drawn source-over — the backdrop, then the
    /// source over it — in place of the mode it names, which this build does
    /// not draw yet.
    CompositeDrawnSourceOver {
        /// The glyph.
        glyph: u16,
        /// The mode the table names.
        mode: CompositeMode,
    },
    /// A `COLR` version 1 graph with no clip box whose paints are not all
    /// bounded by a glyph's outline (*Metrics and boundedness of color glyphs
    /// using version 1 formats*): not drawn, so the glyph is drawn as its
    /// next table, or its outline.
    Unbounded {
        /// The glyph.
        glyph: u16,
    },
    /// A `COLR` version 1 graph whose walk passed [`MAX_DRAWN_PAINTS`], or a
    /// shared table's form that could not be written: the glyph is drawn as
    /// its next table, or its outline.
    GraphTooLarge {
        /// The glyph.
        glyph: u16,
    },
    /// A solid paint naming an entry the palette does not have: the glyph is
    /// drawn as its next table, or its outline, as a version 0 glyph missing
    /// a layer is.
    PaletteEntryMissing {
        /// The glyph.
        glyph: u16,
        /// The entry.
        entry: u16,
    },
    /// A transform whose matrix is no number — a skew at a right angle — or
    /// flattens the plane: what it holds is not drawn, the rest of the graph
    /// is.
    TransformUnusable {
        /// The glyph.
        glyph: u16,
    },
    /// What reading the glyph's graph tolerated or refused, as the font
    /// crate names it: a refused table is not drawn, the rest of the graph
    /// is (*Color glyphs as a directed acyclic graph*).
    Read {
        /// The glyph.
        glyph: u16,
        /// The reader's warning.
        warning: ColrWarning,
    },
}

/// A face's colour glyphs, read under one palette, for one document.
///
/// One per face and palette, as the design has one set of Type 3 fonts per
/// face and palette: register it under its own resource name with
/// [`ColourFace::register`], and name it on every [`crate::shaping::Run`] in
/// that resource. It remembers the `/ExtGState`s and forms it registered with
/// the builder it serves, so it serves one builder.
pub struct ColourFace<'a> {
    sfnt: &'a Sfnt<'a>,
    colr: Option<Colr<'a>>,
    cpal: Option<Cpal<'a>>,
    palette: u16,
    /// The paint alphas whose `/ExtGState` this face has registered.
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
            if let Some(graph) = colr.paint_graph(glyph) {
                if let Some(drawn) = self.graph(builder, resource, glyph, &graph, colr) {
                    return drawn;
                }
            }
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

    /// COLR version 1's graph (*COLR version 1 rendering algorithm*), each
    /// paint as the design's construct, under the glyph's clip box.
    ///
    /// `None` — the glyph then drawn as its next table — for a graph holding
    /// a gradient, one with no clip box that is not bounded, one past
    /// [`MAX_DRAWN_PAINTS`], one naming a palette entry the palette lacks,
    /// and one needing an alpha the profile forbids; each named.
    fn graph(
        &self,
        builder: &mut DocumentBuilder,
        resource: &[u8],
        glyph: u16,
        graph: &PaintGraph,
        colr: &Colr<'_>,
    ) -> Option<Drawn> {
        for warning in graph.warnings() {
            self.warn(ColourWarning::Read {
                glyph,
                warning: *warning,
            });
        }
        let survey = Survey::of(graph, self.sfnt);
        if let Some(format) = survey.gradient {
            self.warn(ColourWarning::GradientNotDrawn { glyph, format });
            return None;
        }
        let clip = colr.clip_box(glyph).map(clip_rect);
        let bounds = match clip {
            Some(rect) => Some(rect),
            None if survey.bounded(graph.root()) => survey.extent(graph.root()),
            None => {
                self.warn(ColourWarning::Unbounded { glyph });
                return None;
            }
        };
        let mut walk = Walk {
            face: self,
            builder,
            resource,
            glyph,
            graph,
            survey: &survey,
            forms: BTreeMap::new(),
            drawn: 0,
            failed: None,
        };
        let mut content = Vec::new();
        if let Some(rect) = clip {
            content.extend_from_slice(b"q\n");
            operator(&mut content, &rect_operands(rect), b"re");
            content.extend_from_slice(b"W n\n");
        }
        walk.paint(&mut content, graph.root(), bounds);
        if clip.is_some() {
            content.extend_from_slice(b"Q\n");
        }
        if let Some(failure) = walk.failed {
            self.warn(failure);
            return None;
        }
        Some(Drawn {
            content,
            bbox: bounds,
        })
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

/// What a version 1 graph is, found before it is drawn: the first gradient a
/// path from the root reaches, how many parents each node has, which nodes
/// paint outside every outline below them, and each node's extent.
struct Survey {
    /// The format of the first gradient reachable from the root.
    gradient: Option<u8>,
    /// How many edges reach each reachable node.
    parents: BTreeMap<PaintId, usize>,
    /// Whether a node paints outside every glyph outline below it: a solid
    /// or a gradient with no `PaintGlyph` between it and the node.
    exposed: BTreeMap<PaintId, bool>,
    /// Whether a node is bounded (*Metrics and boundedness of color glyphs
    /// using version 1 formats*).
    bounded: BTreeMap<PaintId, bool>,
    /// The box of a node's outlines in its own space: what a bounded node
    /// can draw inside.
    extent: BTreeMap<PaintId, Option<[f64; 4]>>,
}

impl Survey {
    /// Surveys every node a path from the root reaches, each once, children
    /// before parents.
    fn of(graph: &PaintGraph, sfnt: &Sfnt<'_>) -> Survey {
        let mut survey = Survey {
            gradient: None,
            parents: BTreeMap::new(),
            exposed: BTreeMap::new(),
            bounded: BTreeMap::new(),
            extent: BTreeMap::new(),
        };
        // Depth-first from the root, without recursion: a node is finished
        // once its children are. The reader holds the graph acyclic.
        let mut order = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack = vec![(graph.root(), false)];
        survey.parents.insert(graph.root(), 0);
        while let Some((id, finished)) = stack.pop() {
            if finished {
                order.push(id);
                continue;
            }
            if !seen.insert(id) {
                continue;
            }
            stack.push((id, true));
            let children = graph.paint(id).map(Paint::children).unwrap_or_default();
            for child in children {
                *survey.parents.entry(child).or_insert(0) += 1;
                if !seen.contains(&child) {
                    stack.push((child, false));
                }
            }
        }
        for id in order {
            let Some(paint) = graph.paint(id) else {
                continue;
            };
            let children = paint.children();
            let any = |map: &BTreeMap<PaintId, bool>| {
                children
                    .iter()
                    .any(|c| map.get(c).copied().unwrap_or(false))
            };
            let all = |map: &BTreeMap<PaintId, bool>| {
                children.iter().all(|c| map.get(c).copied().unwrap_or(true))
            };
            let child_extent =
                |survey: &Survey, child: &PaintId| survey.extent.get(child).copied().flatten();
            let (exposed, bounded, extent) = match paint {
                Paint::Solid { .. } => (true, false, None),
                Paint::LinearGradient { .. }
                | Paint::RadialGradient { .. }
                | Paint::SweepGradient { .. } => {
                    survey.gradient = survey.gradient.or(paint.format());
                    (true, false, None)
                }
                Paint::Glyph { glyph, .. } => {
                    let outline = outline_of(sfnt, *glyph).as_ref().and_then(box_of);
                    (false, true, outline)
                }
                Paint::Layers(_) => (
                    any(&survey.exposed),
                    all(&survey.bounded),
                    children
                        .iter()
                        .fold(None, |held, c| union(held, child_extent(&survey, c))),
                ),
                Paint::ColrGlyph { paint, .. } => (
                    survey.exposed.get(paint).copied().unwrap_or(false),
                    survey.bounded.get(paint).copied().unwrap_or(true),
                    child_extent(&survey, paint),
                ),
                Paint::Composite {
                    source,
                    mode,
                    backdrop,
                } => {
                    let bounded = |c: &PaintId| survey.bounded.get(c).copied().unwrap_or(true);
                    let (s, b) = (bounded(source), bounded(backdrop));
                    // *Format 32: PaintComposite*'s list, mode by mode.
                    let composite_bounded = match mode {
                        CompositeMode::Clear => true,
                        CompositeMode::Src | CompositeMode::SrcOut => s,
                        CompositeMode::Dest | CompositeMode::DestOut => b,
                        CompositeMode::SrcIn | CompositeMode::DestIn => s || b,
                        _ => s && b,
                    };
                    (
                        any(&survey.exposed),
                        composite_bounded,
                        union(
                            child_extent(&survey, source),
                            child_extent(&survey, backdrop),
                        ),
                    )
                }
                Paint::Refused(_) => (false, true, None),
                // The transform family: the child's, through the transform.
                _ => {
                    let child = children.first().copied();
                    let extent = match (child, transform_of(paint)) {
                        (Some(child), Some(matrix)) => {
                            child_extent(&survey, &child).map(|rect| mapped(matrix, rect))
                        }
                        _ => None,
                    };
                    (
                        child.is_some_and(|c| survey.exposed.get(&c).copied().unwrap_or(false)),
                        child.is_none_or(|c| survey.bounded.get(&c).copied().unwrap_or(true)),
                        extent,
                    )
                }
            };
            survey.exposed.insert(id, exposed);
            survey.bounded.insert(id, bounded);
            survey.extent.insert(id, extent);
        }
        survey
    }

    fn bounded(&self, id: PaintId) -> bool {
        self.bounded.get(&id).copied().unwrap_or(false)
    }

    fn extent(&self, id: PaintId) -> Option<[f64; 4]> {
        self.extent.get(&id).copied().flatten()
    }

    /// Whether a node is written once as a form its parents draw: it has
    /// several, and paints nothing outside its own outlines, so what it draws
    /// does not depend on where it is drawn.
    fn shared(&self, id: PaintId) -> bool {
        self.parents.get(&id).copied().unwrap_or(0) > 1
            && !self.exposed.get(&id).copied().unwrap_or(true)
    }
}

/// One glyph's version 1 graph being written.
struct Walk<'w, 'a> {
    face: &'w ColourFace<'a>,
    builder: &'w mut DocumentBuilder,
    resource: &'w [u8],
    glyph: u16,
    graph: &'w PaintGraph,
    survey: &'w Survey,
    /// Each shared node's form, by node, once written.
    forms: BTreeMap<PaintId, Vec<u8>>,
    /// Paints walked so far.
    drawn: usize,
    /// Why the glyph cannot be drawn from its graph, once something says so.
    failed: Option<ColourWarning>,
}

impl Walk<'_, '_> {
    fn fail(&mut self, warning: ColourWarning) {
        if self.failed.is_none() {
            self.failed = Some(warning);
        }
    }

    /// Writes node `id` into `out`; `clip` is the box of the region it paints
    /// inside, in its own space, which a solid fills.
    fn paint(&mut self, out: &mut Vec<u8>, id: PaintId, clip: Option<[f64; 4]>) {
        if self.failed.is_some() {
            return;
        }
        self.drawn += 1;
        if self.drawn > MAX_DRAWN_PAINTS {
            self.fail(ColourWarning::GraphTooLarge { glyph: self.glyph });
            return;
        }
        if self.survey.shared(id) {
            if let Some(name) = self.form(id) {
                crate::form_data::write_name(out, &name);
                out.extend_from_slice(b" Do\n");
            }
            return;
        }
        self.inline(out, id, clip);
    }

    /// Node `id`'s own operators.
    fn inline(&mut self, out: &mut Vec<u8>, id: PaintId, clip: Option<[f64; 4]>) {
        let Some(paint) = self.graph.paint(id) else {
            return;
        };
        match paint {
            Paint::Layers(layers) => {
                // *Format 1: PaintColrLayers*: bottom first, each composited
                // source-over, which is drawing them in turn.
                for layer in layers.clone() {
                    self.paint(out, layer, clip);
                }
            }
            Paint::Solid { colour, alpha, .. } => self.solid(out, *colour, *alpha, clip),
            Paint::Glyph { glyph, paint } => {
                let child = *paint;
                let Some(outline) = outline_of(self.face.sfnt, *glyph).filter(|o| !o.is_empty())
                else {
                    // An empty outline clips everything away.
                    return;
                };
                out.extend_from_slice(b"q\n");
                path(out, &outline);
                // *Filling shapes*: the outline as a clip, under the nonzero
                // rule `glyf` and `CFF ` outlines are filled by.
                out.extend_from_slice(b"W n\n");
                self.paint(out, child, box_of(&outline));
                out.extend_from_slice(b"Q\n");
            }
            // *Re-use using PaintColrGlyph*: "a continuation of the graph",
            // so the named glyph's own clip box does not apply.
            Paint::ColrGlyph { paint, .. } => self.paint(out, *paint, clip),
            Paint::Composite {
                source,
                mode,
                backdrop,
            } => {
                if *mode != CompositeMode::SrcOver {
                    self.face.warn(ColourWarning::CompositeDrawnSourceOver {
                        glyph: self.glyph,
                        mode: *mode,
                    });
                }
                let (source, backdrop) = (*source, *backdrop);
                self.paint(out, backdrop, clip);
                self.paint(out, source, clip);
            }
            // Named by the reader's warning; the rest of the graph is drawn.
            Paint::Refused(_) => {}
            // The survey refused a graph that reaches one.
            Paint::LinearGradient { .. }
            | Paint::RadialGradient { .. }
            | Paint::SweepGradient { .. } => {}
            transformed => {
                let child = transformed.children().first().copied();
                let Some(child) = child else {
                    return;
                };
                let matrix = transform_of(transformed);
                let inverse = matrix.and_then(transform::invert);
                let (Some(matrix), Some(inverse)) = (matrix, inverse) else {
                    self.face
                        .warn(ColourWarning::TransformUnusable { glyph: self.glyph });
                    return;
                };
                out.extend_from_slice(b"q\n");
                operator(out, &matrix, b"cm");
                self.paint(out, child, clip.map(|rect| mapped(inverse, rect)));
                out.extend_from_slice(b"Q\n");
            }
        }
    }

    /// *Format 2: PaintSolid*: the clip region filled in the palette colour,
    /// its alpha the entry's over 255 times the paint's, clamped to `[0, 1]`
    /// (*Colors and solid color fills*); a foreground fill sets no colour.
    fn solid(
        &mut self,
        out: &mut Vec<u8>,
        colour: ColourIndex,
        alpha: i16,
        clip: Option<[f64; 4]>,
    ) {
        let Some(rect) = clip else {
            return;
        };
        let paint_alpha = f2dot14(alpha).clamp(0.0, 1.0);
        let (components, entry_alpha) = match colour {
            ColourIndex::Foreground => (None, 1.0),
            ColourIndex::Palette(entry) => {
                let rgba = self
                    .face
                    .cpal
                    .as_ref()
                    .and_then(|cpal| cpal.colour(self.face.palette, entry));
                let Some(rgba) = rgba else {
                    self.fail(ColourWarning::PaletteEntryMissing {
                        glyph: self.glyph,
                        entry,
                    });
                    return;
                };
                (
                    Some([rgba.red, rgba.green, rgba.blue]),
                    f64::from(rgba.alpha) / 255.0,
                )
            }
        };
        // Eight bits, as every alpha this engine composites with.
        let alpha = (entry_alpha * paint_alpha * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        out.extend_from_slice(b"q\n");
        if alpha < 255 {
            let Some(state) = self
                .face
                .alpha_state(self.builder, self.resource, self.glyph, alpha)
            else {
                self.fail(ColourWarning::TransparencyForbidden { glyph: self.glyph });
                return;
            };
            crate::form_data::write_name(out, &state);
            out.extend_from_slice(b" gs\n");
        }
        if let Some(components) = components {
            rgb(out, components);
        }
        operator(out, &rect_operands(rect), b"re");
        out.extend_from_slice(b"f\nQ\n");
    }

    /// The name of shared node `id`'s form, written and registered with the
    /// builder the first time a path reaches it: its own operators in its own
    /// space, its `/BBox` its outlines' box.
    fn form(&mut self, id: PaintId) -> Option<Vec<u8>> {
        if let Some(name) = self.forms.get(&id) {
            return Some(name.clone());
        }
        let Some(bbox) = self.survey.extent(id).filter(is_area) else {
            // Nothing to draw inside: a shared node of empty outlines.
            return None;
        };
        let mut content = Vec::new();
        self.inline(&mut content, id, None);
        if self.failed.is_some() {
            return None;
        }
        let mut name = self.resource.to_vec();
        name.extend_from_slice(format!(".g{}n{}", self.glyph, id.0).as_bytes());
        let registered = self.builder.add_form(
            &name,
            &FormXObject {
                bbox,
                matrix: None,
                group: None,
                content: &content,
            },
        );
        if !registered {
            self.fail(ColourWarning::GraphTooLarge { glyph: self.glyph });
            return None;
        }
        self.forms.insert(id, name.clone());
        Some(name)
    }
}

/// The matrix a transform-family paint applies to its child, `None` for any
/// other paint and for one that is no number.
///
/// *Formats 12 to 31*: `Affine2x3`'s `xx yx xy yy dx dy` as `cm`'s `a b c d e
/// f`; a rotation by θ, 180° per 1.0 counter-clockwise, `[cos θ, sin θ, −sin
/// θ, cos θ, 0, 0]`; a skew by φ along x and ψ along y `[1, tan ψ, −tan φ, 1,
/// 0, 0]`; each around-centre form between a translation to the centre and
/// its inverse.
fn transform_of(paint: &Paint) -> Option<[f64; 6]> {
    let (matrix, centre) = match paint {
        Paint::Transform { matrix, .. } => {
            let [xx, yx, xy, yy, dx, dy] = matrix.map(fixed);
            ([xx, yx, xy, yy, dx, dy], None)
        }
        Paint::Translate { dx, dy, .. } => {
            ([1.0, 0.0, 0.0, 1.0, f64::from(*dx), f64::from(*dy)], None)
        }
        Paint::Scale {
            scale_x,
            scale_y,
            centre,
            ..
        } => (
            [f2dot14(*scale_x), 0.0, 0.0, f2dot14(*scale_y), 0.0, 0.0],
            *centre,
        ),
        Paint::ScaleUniform { scale, centre, .. } => {
            let s = f2dot14(*scale);
            ([s, 0.0, 0.0, s, 0.0, 0.0], *centre)
        }
        Paint::Rotate { angle, centre, .. } => {
            (transform::rotation(f2dot14(*angle) * 180.0), *centre)
        }
        Paint::Skew {
            x_angle,
            y_angle,
            centre,
            ..
        } => (
            [
                1.0,
                transform::tangent(f2dot14(*y_angle) * 180.0),
                -transform::tangent(f2dot14(*x_angle) * 180.0),
                1.0,
                0.0,
                0.0,
            ],
            *centre,
        ),
        _ => return None,
    };
    let matrix = match centre {
        Some([cx, cy]) => {
            let (cx, cy) = (f64::from(cx), f64::from(cy));
            // To the centre's origin, the transform, and back.
            transform::concat(
                transform::concat([1.0, 0.0, 0.0, 1.0, -cx, -cy], matrix),
                [1.0, 0.0, 0.0, 1.0, cx, cy],
            )
        }
        None => matrix,
    };
    matrix.iter().all(|v| v.is_finite()).then_some(matrix)
}

/// A `ClipBox` as a rectangle.
fn clip_rect(clip: ClipBox) -> [f64; 4] {
    [
        f64::from(clip.x_min),
        f64::from(clip.y_min),
        f64::from(clip.x_max),
        f64::from(clip.y_max),
    ]
}

/// `re`'s operands for a rectangle `[x0 y0 x1 y1]`.
fn rect_operands(rect: [f64; 4]) -> [f64; 4] {
    [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]]
}

/// Whether a rectangle encloses an area.
fn is_area(rect: &[f64; 4]) -> bool {
    rect.iter().all(|v| v.is_finite()) && rect[2] > rect[0] && rect[3] > rect[1]
}

/// The box of a rectangle's four corners through a matrix.
fn mapped(matrix: [f64; 6], rect: [f64; 4]) -> [f64; 4] {
    let corners = [
        [rect[0], rect[1]],
        [rect[2], rect[1]],
        [rect[2], rect[3]],
        [rect[0], rect[3]],
    ]
    .map(|corner| transform::apply(matrix, corner));
    let mut out = [corners[0][0], corners[0][1], corners[0][0], corners[0][1]];
    for [x, y] in corners {
        out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A centred transform is the transform carried to the centre: the centre
    /// stays where it is, and a point a unit right of it lands where the
    /// transform puts a unit vector.
    #[test]
    fn an_around_centre_transform_fixes_its_centre() {
        let paint = Paint::Rotate {
            angle: 8192,
            centre: Some([500, 500]),
            paint: PaintId(1),
            var_index_base: None,
        };
        let matrix = transform_of(&paint).expect("a number");
        let centre = transform::apply(matrix, [500.0, 500.0]);
        assert!((centre[0] - 500.0).abs() < 1e-9 && (centre[1] - 500.0).abs() < 1e-9);
        // A quarter turn counter-clockwise takes (+1, 0) from the centre to
        // (0, +1).
        let right = transform::apply(matrix, [501.0, 500.0]);
        assert!((right[0] - 500.0).abs() < 1e-9 && (right[1] - 501.0).abs() < 1e-9);
    }

    /// The skew of *Formats 28 to 31*: `[1, tan ψ, −tan φ, 1, 0, 0]`.
    #[test]
    fn a_skew_is_the_formula_with_its_signs() {
        let paint = Paint::Skew {
            x_angle: 4096,
            y_angle: 0,
            centre: None,
            paint: PaintId(1),
            var_index_base: None,
        };
        let matrix = transform_of(&paint).expect("a number");
        assert!((matrix[2] + 1.0).abs() < 1e-12, "{matrix:?}");
        assert_eq!([matrix[0], matrix[1], matrix[3]], [1.0, 0.0, 1.0]);
        // A right angle has no tangent a matrix can hold.
        let right = Paint::Skew {
            x_angle: 8192,
            y_angle: 0,
            centre: None,
            paint: PaintId(1),
            var_index_base: None,
        };
        let matrix = transform_of(&right);
        assert!(
            matrix.is_none() || matrix.is_some_and(|m| m[2].abs() > 1e12),
            "{matrix:?}"
        );
    }

    /// `Affine2x3` goes to `cm` in its own order, its `Fixed`s divided by
    /// 65 536.
    #[test]
    fn an_affine_is_written_in_its_own_order() {
        let paint = Paint::Transform {
            matrix: [65536, 32768, -65536, 131072, 655360, -327680],
            paint: PaintId(1),
            var_index_base: None,
        };
        assert_eq!(
            transform_of(&paint),
            Some([1.0, 0.5, -1.0, 2.0, 10.0, -5.0])
        );
        let at = transform::apply(transform_of(&paint).expect("a number"), [1.0, 1.0]);
        // x' = xx x + xy y + dx, y' = yx x + yy y + dy.
        assert_eq!(at, [1.0 - 1.0 + 10.0, 0.5 + 2.0 - 5.0]);
    }
}
