//! A fixed page's markup, painted into a PDF content stream (gap 30,
//! milestone 6).
//!
//! # What is streamed and what is not
//!
//! `Canvas` and `FixedPage` are **streamed**: each opens a scope holding its
//! own content buffer, and a scope is closed and folded into its parent when
//! the end tag arrives. Nothing holds a drawable tree, which is gap 30's own
//! instruction — *"a tree would allocate the whole of a fixed page before
//! anything looked at it, and a fixed page is the one part of this format whose
//! size is chosen by the file"*.
//!
//! Everything else is a **value**, and a value is materialised: a `Path`'s
//! whole subtree is one call to [`super::markup::subtree`], because a `Path`
//! has no drawable children and its `Path.Data`, `Path.Fill` and `Path.Clip`
//! have to be complete before any of them can be used. The buffer is why a
//! property element may arrive after a drawable child and still apply: a
//! canvas's transform, clip and opacity are written **around** its buffered
//! content at the moment it closes, so document order inside the canvas cannot
//! change what the canvas does.
//!
//! # The refusal asymmetry, which is where most of this milestone's honesty is
//!
//! Gap 30's design section decides three cases before any file exists, and
//! they are deliberately not symmetric:
//!
//! - **geometry unreadable** — the element is *not painted*, and warns. A
//!   missing shape is visible as a missing shape.
//! - **paint unreadable** — the element *is* painted, in the neutral
//!   placeholder grey, and warns. The shape and its position are known and
//!   only the colour is not, and gap 07's headline defect was a
//!   gradient-stroked rule painting solid black *silently*: a default that
//!   could be right is worse than a default that is visibly a default.
//! - **a transform, a clip or an opacity that cannot be read** — the element
//!   and everything under it is *refused*, and warns. The identity matrix, the
//!   absent clip and full opacity are each a plausible wrong answer that draws
//!   the right content in the wrong place at the right size, which reads as a
//!   layout bug in the producer.
//!
//! One parser serves `Path.Data` and `Path.Clip`, and the same syntax error in
//! the two produces **different** answers — the first is a shape that is not
//! painted, the second is an element that does not draw at all. Two
//! consequences of one failure is exactly the shape gap 30's milestones 2 to 5
//! each found once, so the two are held apart by two fixtures rather than by
//! one that happens to exercise whichever came first.
//!
//! # `Opacity` on a `Canvas` is two rules
//!
//! 14.3's `Opacity` composites the canvas's rendering *as a whole*, which is a
//! transparency group. It is only observably a group when two of the canvas's
//! children **overlap**: where they do not, one alpha per child paints the
//! identical picture and costs no form XObject, no `/Group` and no second
//! buffer. So both are built, the overlap decides, and the two cases are two
//! fixtures — a suite that only ever drew non-overlapping children would pass
//! with the group deleted.

use std::collections::HashMap;

use tinker_pdf_cos::{
    DeviceSpace, DocumentBuilder, ExtGState, FormXObject, Glyph, MaskKind, PlacedGlyph, Shading,
    ShadingPattern, StateMask, TilingPattern, TilingType, TransparencyGroup,
};
use tinker_pdf_xml::{Doctype, Event, Limits as XmlLimits, Source};

use super::brush::{
    self, Brush, BrushError, ImageTile, Mask, Paint, Placement, TileMode, Units, VisualTile,
};
use super::font::Fonts;
use super::geometry::{self, Geometry, GeometryError};
use super::glyphs::{self, RunError};
use super::image::Images;
use super::markup::{self, Budget, Node, Trouble};
use super::opc::PartName;
use super::{XpsElementDefect, UNITS_TO_POINTS};
use crate::cbz::PLACEHOLDER_GREY;

/// How deep a chain of `{StaticResource}` aliases may run.
///
/// Declared in [`super::MAX_XPS_RESOURCE_DEPTH`], which carries the ledger.
use super::MAX_XPS_RESOURCE_DEPTH;

/// How deep a `VisualBrush` may nest inside another.
///
/// Declared in [`super::MAX_XPS_VISUAL_DEPTH`], which carries the ledger.
use super::MAX_XPS_VISUAL_DEPTH;

/// One fixed page, painted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawn {
    /// The content stream, **without** the page's own flip: the same markup
    /// may be shown on two pages and the flip is the page's height, so it is
    /// prepended per page rather than baked in per part.
    pub content: Vec<u8>,
    /// What was degraded, deduplicated, in first-occurrence order.
    ///
    /// Deduplicated because a page of ten thousand `Glyphs` runs has one thing
    /// wrong with it and not ten thousand, and a report nobody can read is a
    /// report nobody reads.
    pub defects: Vec<XpsElementDefect>,
}

/// The state that outlives one page: resource names, and the graphics states
/// already registered.
///
/// One per synthesised document. Resource names are unique across the
/// document, which they have to be — [`DocumentBuilder::add_page`] copies the
/// document's whole resource table into every page, so two pages naming one
/// `/GS0` for two different states would be one state.
#[derive(Default)]
pub struct Painter {
    next: usize,
    /// `(fill alpha, stroke alpha)` to the `/ExtGState` already written for
    /// it. A page of a thousand half-transparent shapes writes one dictionary.
    gstates: HashMap<(u64, u64), Vec<u8>>,
}

/// The step a `TileMode="None"` pattern gets.
///
/// PDF has no "draw this once" pattern — 8.7.3.1's cells always repeat — so the
/// spacing is made larger than any page can be and the shape's own extent does
/// the rest. A million points is about thirteen thousand inches; the largest
/// page PDF admits is 200.
const NO_TILE_STEP: f64 = 1.0e6;

impl Painter {
    fn name(&mut self, prefix: &str) -> Vec<u8> {
        let name = format!("{prefix}{}", self.next);
        self.next += 1;
        name.into_bytes()
    }

    /// Paints one fixed page's markup.
    ///
    /// # Errors
    /// [`Trouble::Markup`] when the part is not readable markup — the page
    /// becomes a named placeholder — and [`Trouble::Exhausted`] when one of
    /// the document's work totals is spent, which refuses the package.
    pub fn page(
        &mut self,
        builder: &mut DocumentBuilder,
        bytes: &[u8],
        around: &Surroundings<'_>,
        budget: &mut Budget,
    ) -> Result<Drawn, Trouble> {
        let source = Source::new(bytes).map_err(|_| Trouble::Markup)?;
        let mut reader = source.reader_with(around.xml, Doctype::Refuse);

        // The root has already been checked by the spine — it is a `FixedPage`
        // in one of the two dialects, or this part would never have become a
        // page — so this consumes it rather than re-deciding it. Its own
        // attributes carry nothing this milestone draws with; its
        // `FixedPage.Resources` arrives as a child.
        loop {
            let event = reader.next().ok_or(Trouble::Markup)??;
            if matches!(event, Event::Start(_)) {
                break;
            }
        }
        budget.element()?;

        let mut state = State {
            painter: self,
            builder,
            around,
            defects: Vec::new(),
            dictionaries: Vec::new(),
            visuals: Vec::new(),
        };
        let mut scopes = vec![Scope::root()];
        state.run(&mut reader, &mut scopes, budget)?;

        let content = scopes.pop().map(|scope| scope.content).unwrap_or_default();
        Ok(Drawn {
            content,
            defects: state.defects,
        })
    }
}

/// What a fixed page part needs from outside its own markup.
///
/// One value rather than three parameters, because every one of them is *the
/// part's* — its own name, the fonts resolved against that name, and what the
/// markup reader may spend on it — and a caller that had to thread three would
/// eventually thread one page's name past another page's fonts.
pub struct Surroundings<'a> {
    /// The fixed page part's own name, which every relative reference in its
    /// markup resolves against (OPC 8.1, and milestone 1 measured both forms:
    /// XPS 1.0 writes `/Resources/…` and OpenXPS writes `../../../Resources/…`
    /// for the same part).
    pub part: &'a PartName,
    /// The fonts [`super::font::Fonts::load`] resolved for this part.
    pub fonts: &'a Fonts,
    /// The images [`super::image::Images::load`] placed for this part.
    pub images: &'a Images,
    /// The page's own size, in XPS units.
    ///
    /// Needed for one thing and it is not the geometry: a tiling pattern's
    /// `/Matrix` maps pattern space into the page's **default** space (8.7.3.1),
    /// not into the transform in force when the pattern is set — so the painter
    /// has to be able to reconstruct 18.1's own `cm`, and that one carries the
    /// page height. Everything else in this module works in markup units on
    /// purpose, so the numbers in a content stream are the numbers in the file.
    pub page: (f64, f64),
    /// What the markup reader may spend on one part.
    pub xml: &'a XmlLimits,
}

/// One open element that has content of its own.
struct Scope {
    /// `Canvas`, or `FixedPage` for the root. Property elements are named
    /// `<owner>.<property>`, so this is what tells one from a drawable.
    owner: &'static str,
    content: Vec<u8>,
    transform: [f64; 6],
    clip: Option<Geometry>,
    opacity: f64,
    /// Each child's bounding box **in this scope's own space**, for the
    /// overlap test 14.3's opacity turns on.
    boxes: Vec<[f64; 4]>,
    /// A transform, clip or opacity that would not read. The scope still has
    /// to be tracked to its end tag — the markup below it is still markup —
    /// and nothing it holds is drawn.
    refused: bool,
    /// How many resource dictionaries this scope pushed, so closing it unbinds
    /// exactly those.
    dictionaries: usize,
    /// 14.3's `OpacityMask`, as the brush element that states it.
    ///
    /// Markup rather than a [`Mask`], because the box a
    /// `RelativeToBoundingBox` brush is stated in fractions of is the union of
    /// what the children drew, and that is not known until the end tag.
    mask: Option<Node>,
}

impl Scope {
    fn root() -> Scope {
        Scope {
            owner: "FixedPage",
            content: Vec::new(),
            transform: markup::IDENTITY,
            clip: None,
            opacity: 1.0,
            boxes: Vec::new(),
            refused: false,
            dictionaries: 0,
            mask: None,
        }
    }
}

/// What painting one brush can go wrong with, at the two altitudes it can.
///
/// Every other paint failure in this module is one [`XpsElementDefect`] and
/// the element takes the placeholder grey. A `VisualBrush` is the first that
/// can also fail at the *page's* altitude, because painting it re-enters the
/// drawing walk — and that walk can exhaust one of gap 30's work totals, which
/// refuses the package, or find markup that will not read, which refuses the
/// page. Neither is a grey rectangle.
///
/// So the two are held apart in the type rather than by a convention: a
/// [`Trouble`] that arrived through a brush is still a [`Trouble`] when it
/// leaves, and there is no arm a caller can write that quietly turns an
/// exhausted budget into a tile it draws anyway.
#[derive(Debug)]
enum Refused {
    /// The brush is refused, named, and painted grey. The page continues.
    Brush(XpsElementDefect),
    /// The page or the package is refused, and nothing more is painted.
    Page(Trouble),
}

impl From<XpsElementDefect> for Refused {
    fn from(defect: XpsElementDefect) -> Refused {
        Refused::Brush(defect)
    }
}

impl From<Trouble> for Refused {
    fn from(trouble: Trouble) -> Refused {
        Refused::Page(trouble)
    }
}

/// 15.3's placement, with its two unit modes already spent.
///
/// [`Placement`] is what the markup **said**; this is what it **means** once
/// the source's own extent and the box being filled are known. Both brushes
/// that tile — an `ImageBrush` over a picture and a `VisualBrush` over a
/// subtree — differ in what goes in the cell and in nothing about where the
/// cell goes, so the arithmetic is done once here rather than twice.
///
/// The one place they are not identical is what a `RelativeToBoundingBox`
/// *viewbox* is a fraction of, and that is the `source` argument to
/// [`Placed::of`]: an image's pixels are a second space with an extent of its
/// own, and a visual's coordinates are already the brush's, so its source is
/// the unit square and a relative viewbox on one is the whole of it.
struct Placed {
    /// `Viewbox`, in the source's own units, absolute.
    viewbox: [f64; 4],
    /// `Viewport`, in the element's units, absolute.
    viewport: [f64; 4],
    /// `TileMode`, carried because the step depends on it.
    tile: TileMode,
    /// `Transform`, which maps the brush's space into the element's.
    transform: [f64; 6],
}

impl Placed {
    /// Spends 15.3's two unit modes.
    ///
    /// `source` is the source's own extent, which only a
    /// `RelativeToBoundingBox` viewbox needs. `bbox` is the box being filled,
    /// which only a `RelativeToBoundingBox` viewport needs — and a relative
    /// viewport with no box is [`XpsElementDefect::BrushUnreadable`] rather
    /// than a box invented here.
    fn of(
        placement: &Placement,
        source: (f64, f64),
        bbox: Option<[f64; 4]>,
    ) -> Result<Placed, XpsElementDefect> {
        let viewbox = match placement.viewbox_units {
            Units::Absolute => placement.viewbox,
            Units::RelativeToBoundingBox => [
                placement.viewbox[0] * source.0,
                placement.viewbox[1] * source.1,
                placement.viewbox[2] * source.0,
                placement.viewbox[3] * source.1,
            ],
        };
        let viewport = match placement.viewport_units {
            Units::Absolute => placement.viewport,
            Units::RelativeToBoundingBox => {
                let Some(b) = bbox else {
                    return Err(XpsElementDefect::BrushUnreadable);
                };
                let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
                [
                    b[0] + placement.viewport[0] * bw,
                    b[1] + placement.viewport[1] * bh,
                    placement.viewport[2] * bw,
                    placement.viewport[3] * bh,
                ]
            }
        };
        if viewbox[2] <= 0.0 || viewbox[3] <= 0.0 {
            return Err(XpsElementDefect::BrushUnreadable);
        }
        Ok(Placed {
            viewbox,
            viewport,
            tile: placement.tile,
            transform: placement.transform,
        })
    }

    /// The cell, in the source's own units, and how many sources across it is.
    ///
    /// The reflections are the only reason it is ever more than one: 8.7.3.1
    /// gives a cell and two steps and **no reflection at all**, so `FlipX`,
    /// `FlipY` and `FlipXY` are built by making the cell twice the source in
    /// the flipped direction and drawing the source into it twice.
    fn cell(&self) -> (f64, f64) {
        let (x, y) = match self.tile {
            TileMode::None | TileMode::Tile => (1.0, 1.0),
            TileMode::FlipX => (2.0, 1.0),
            TileMode::FlipY => (1.0, 2.0),
            TileMode::FlipXY => (2.0, 2.0),
        };
        (self.viewbox[2] * x, self.viewbox[3] * y)
    }

    /// One `(x, y)` sign pair per copy the cell holds.
    fn flips(&self) -> &'static [(f64, f64)] {
        match self.tile {
            TileMode::None | TileMode::Tile => &[(1.0, 1.0)],
            TileMode::FlipX => &[(1.0, 1.0), (-1.0, 1.0)],
            TileMode::FlipY => &[(1.0, 1.0), (1.0, -1.0)],
            TileMode::FlipXY => &[(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)],
        }
    }

    /// Where a mirrored copy's origin sits: on the far side of the cell,
    /// drawing backwards into its own half.
    fn mirror_origin(fx: f64, fy: f64, w: f64, h: f64) -> (f64, f64) {
        let x = if fx < 0.0 { 2.0 * w } else { 0.0 };
        let y = if fy < 0.0 { 2.0 * h } else { 0.0 };
        (x, y)
    }

    /// Pattern space into the element's space: the viewbox-to-viewport scale
    /// and the viewport's own offset, with the brush's `Transform` inside both.
    ///
    /// The two directions scale **independently**: 15.3 does not preserve the
    /// aspect ratio, and a build that took one scale for both would letterbox
    /// a picture the file said to stretch.
    fn place(&self) -> [f64; 6] {
        let sx = self.viewport[2] / self.viewbox[2];
        let sy = self.viewport[3] / self.viewbox[3];
        let fit = [
            sx,
            0.0,
            0.0,
            sy,
            self.viewport[0] - self.viewbox[0] * sx,
            self.viewport[1] - self.viewbox[1] * sy,
        ];
        // 15.3's `Transform` is stated in the brush's own space, so it runs
        // *before* the fit into the viewport — the other order would scale the
        // brush transform's translation by the viewbox-to-viewport factor.
        markup::concat(self.transform, fit)
    }
}

/// A resource dictionary and the scope depth it was declared at.
///
/// 14.2.5 resolves a reference in the dictionary of the element it is written
/// on and then outward, and a reference **inside** a dictionary entry resolves
/// from that dictionary outward and not from wherever the entry is used — so
/// the depth is carried rather than inferred from the top of the stack.
struct Dictionary {
    entries: HashMap<String, Node>,
}

struct State<'a> {
    painter: &'a mut Painter,
    builder: &'a mut DocumentBuilder,
    around: &'a Surroundings<'a>,
    defects: Vec<XpsElementDefect>,
    dictionaries: Vec<Dictionary>,
    /// The `VisualBrush`es currently being painted, innermost last.
    ///
    /// One entry per open brush whether or not it was keyed, so the length is
    /// the *nest's* depth and not the keyed nest's: an inline `VisualBrush`
    /// inside an inline one is exactly as deep as two keyed ones. An unkeyed
    /// brush pushes `None`, which is never equal to a key — an
    /// `Option<String>` rather than a sentinel string, so an `x:Key` that
    /// happens to be empty cannot be mistaken for one.
    visuals: Vec<Option<String>>,
}

impl State<'_> {
    fn warn(&mut self, defect: XpsElementDefect) {
        if !self.defects.contains(&defect) {
            self.defects.push(defect);
        }
    }

    /// Splits a [`Refused`] at its two altitudes.
    ///
    /// The brush half is named and answered `Ok`, so the caller carries on and
    /// paints the placeholder. The page half is handed straight back, so a
    /// budget spent inside a `VisualBrush` still refuses the package rather
    /// than becoming a grey rectangle on a page that keeps drawing.
    fn refused(&mut self, error: Refused) -> Result<(), Trouble> {
        match error {
            Refused::Brush(defect) => {
                self.warn(defect);
                Ok(())
            }
            Refused::Page(trouble) => Err(trouble),
        }
    }

    /// The event loop. One pass, one scope stack, no drawable tree.
    fn run(
        &mut self,
        reader: &mut tinker_pdf_xml::Reader<'_>,
        scopes: &mut Vec<Scope>,
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        loop {
            let Some(event) = reader.next() else {
                // The page ended without closing its root. A well-formed
                // document cannot, so this is a part that will not read.
                return Err(Trouble::Markup);
            };
            match event? {
                Event::Start(element) => {
                    // One charge per start tag, here and nowhere else: the
                    // subtree readers charge what is *under* an element, so a
                    // caller that also charged the element would make one
                    // element cost two and a `MAX_XPS_ELEMENTS` that fires at
                    // half its published number.
                    budget.element()?;
                    let node = markup::leaf(&element);
                    let owner = scopes.last().map_or("FixedPage", |scope| scope.owner);
                    if !node.xps {
                        // Markup from another vocabulary. 14.1 lets a producer
                        // hang one on a fixed page and a consumer ignore it,
                        // and it is named rather than passed over in silence.
                        self.warn(XpsElementDefect::ElementUnknown);
                        markup::skip_subtree(reader, budget)?;
                    } else if node.property_of(owner).is_some() {
                        let subtree = markup::subtree(reader, &element, budget)?;
                        self.property(scopes, &subtree, budget)?;
                    } else {
                        match node.local.as_str() {
                            "Canvas" => {
                                let scope = self.canvas(scopes, &node, budget)?;
                                scopes.push(scope);
                                continue;
                            }
                            "Path" => {
                                let subtree = markup::subtree(reader, &element, budget)?;
                                self.path(scopes, &subtree, budget)?;
                            }
                            "Glyphs" => {
                                let subtree = markup::subtree(reader, &element, budget)?;
                                self.glyphs(scopes, &subtree, budget)?;
                            }
                            _ => {
                                self.warn(XpsElementDefect::ElementUnknown);
                                markup::skip_subtree(reader, budget)?;
                            }
                        }
                    }
                }
                Event::End(_) => {
                    let Some(done) = scopes.pop() else {
                        return Ok(());
                    };
                    self.dictionaries
                        .truncate(self.dictionaries.len().saturating_sub(done.dictionaries));
                    if scopes.is_empty() {
                        // The root closed. Put it back so the caller can take
                        // its content.
                        scopes.push(done);
                        return Ok(());
                    }
                    self.close(scopes, done, budget)?;
                }
                _ => {}
            }
        }
    }

    /// A `<owner>.<property>` element: a value, applied to the open scope.
    fn property(
        &mut self,
        scopes: &mut [Scope],
        node: &Node,
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        let owner = scopes.last().map_or("FixedPage", |scope| scope.owner);
        let Some(property) = node.property_of(owner) else {
            return Ok(());
        };
        match property {
            "Resources" => {
                let Some(dictionary) = node.child("ResourceDictionary") else {
                    self.warn(XpsElementDefect::ElementUnknown);
                    return Ok(());
                };
                // 14.2.4's remote dictionary names another part, which is gap
                // 30 milestone 8. Every key in it is unresolvable here, and
                // saying so once is better than one `BrushUnresolved` per use.
                if dictionary.attr("Source").is_some() {
                    self.warn(XpsElementDefect::ResourceDictionaryRemote);
                }
                let mut entries = HashMap::new();
                for entry in &dictionary.children {
                    let Some(key) = entry.key.clone() else {
                        // 14.2.2 makes `x:Key` mandatory on every entry; one
                        // without a key can never be referenced.
                        self.warn(XpsElementDefect::ElementUnknown);
                        continue;
                    };
                    entries.insert(key, entry.clone());
                }
                self.dictionaries.push(Dictionary { entries });
                if let Some(scope) = scopes.last_mut() {
                    scope.dictionaries += 1;
                }
            }
            "RenderTransform" | "Transform" => {
                let matrix = node
                    .child("MatrixTransform")
                    .and_then(|m| m.attr("Matrix"))
                    .and_then(markup::matrix);
                match (matrix, scopes.last_mut()) {
                    (Some(matrix), Some(scope)) => {
                        scope.transform = markup::concat(scope.transform, matrix);
                    }
                    (None, _) => {
                        self.warn(XpsElementDefect::TransformUnreadable);
                        if let Some(scope) = scopes.last_mut() {
                            scope.refused = true;
                        }
                    }
                    _ => {}
                }
            }
            "Clip" => {
                let geometry = node
                    .child("PathGeometry")
                    .ok_or(GeometryError::Syntax)
                    .and_then(|g| geometry::from_geometry_node(g, budget));
                match geometry {
                    Ok(geometry) => {
                        if let Some(scope) = scopes.last_mut() {
                            scope.clip = Some(geometry);
                        }
                    }
                    Err(GeometryError::Exhausted) => return Err(Trouble::Exhausted),
                    Err(GeometryError::Syntax) => {
                        self.warn(XpsElementDefect::ClipUnreadable);
                        if let Some(scope) = scopes.last_mut() {
                            scope.refused = true;
                        }
                    }
                }
            }
            "OpacityMask" if owner == "Canvas" => {
                // The brush is kept as **markup** and read as a mask when the
                // canvas closes: 14.3 composites the canvas's rendering *as a
                // whole*, so the mask covers the union of what the children
                // drew and that box is not known until the end tag — and a
                // `RelativeToBoundingBox` brush is stated in fractions of it.
                // Resolved to an element here rather than there, because a
                // `{StaticResource}` inside the canvas's own dictionary is out
                // of scope by the time the canvas closes.
                match node.children.iter().find(|child| child.xps) {
                    Some(brush) => {
                        if let Some(scope) = scopes.last_mut() {
                            scope.mask = Some(brush.clone());
                        }
                    }
                    None => {
                        self.warn(XpsElementDefect::BrushUnreadable);
                        if let Some(scope) = scopes.last_mut() {
                            scope.refused = true;
                        }
                    }
                }
            }
            // `FixedPage.NavigateUri`, `Canvas.Name` and anything else a
            // producer writes as a property element: not drawn, and named.
            _ => self.warn(XpsElementDefect::ElementUnknown),
        }
        Ok(())
    }

    /// Opens a `Canvas`.
    fn canvas(
        &mut self,
        scopes: &mut [Scope],
        node: &Node,
        budget: &mut Budget,
    ) -> Result<Scope, Trouble> {
        let mut scope = Scope {
            owner: "Canvas",
            content: Vec::new(),
            transform: markup::IDENTITY,
            clip: None,
            opacity: 1.0,
            boxes: Vec::new(),
            refused: scopes.last().is_some_and(|parent| parent.refused),
            dictionaries: 0,
            mask: None,
        };
        match geometry::transform_of(node, "Canvas") {
            Ok(Some(matrix)) => scope.transform = matrix,
            Ok(None) => {}
            Err(_) => {
                self.warn(XpsElementDefect::TransformUnreadable);
                scope.refused = true;
            }
        }
        match opacity_of(node) {
            Ok(value) => scope.opacity = value,
            Err(()) => {
                self.warn(XpsElementDefect::OpacityUnreadable);
                scope.refused = true;
            }
        }
        match geometry::property(node, "Canvas", "Clip", budget) {
            Ok(clip) => scope.clip = clip,
            Err(GeometryError::Exhausted) => return Err(Trouble::Exhausted),
            Err(GeometryError::Syntax) => {
                self.warn(XpsElementDefect::ClipUnreadable);
                scope.refused = true;
            }
        }
        // Resolved here rather than where the canvas closes, because a
        // `{StaticResource}` in the canvas's own dictionary is out of scope by
        // then — and kept as markup, because the box a
        // `RelativeToBoundingBox` brush is a fraction of is not known yet.
        if let Some(value) = node.attr("OpacityMask") {
            match self.attribute_brush(value) {
                Ok(brush) => scope.mask = Some(brush),
                Err(defect) => {
                    self.warn(defect);
                    scope.refused = true;
                }
            }
        }
        Ok(scope)
    }

    /// Folds a closed canvas into its parent.
    ///
    /// # Errors
    /// The canvas's `OpacityMask` may be a `VisualBrush`, whose painting can
    /// spend the page's budget — and that refuses the package rather than the
    /// canvas.
    fn close(
        &mut self,
        scopes: &mut [Scope],
        done: Scope,
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        if scopes.is_empty() || done.refused || done.content.is_empty() {
            return Ok(());
        }
        let inner_box = union(&done.boxes);

        // 14.3's mask, over the union of what the children drew — read here
        // rather than where the markup stated it, because that box is what a
        // `RelativeToBoundingBox` brush is a fraction of. A mask that will not
        // read refuses the canvas, which is what `Scope::refused` does for the
        // transform, the clip and the opacity.
        let mut mask_alpha = 1.0;
        let mut mask_gs = None;
        if let Some(brush) = &done.mask {
            let read = brush::mask_from_node(brush, inner_box)
                .map_err(|error| match error {
                    BrushError::Unsupported => XpsElementDefect::BrushUnsupported,
                    BrushError::Syntax => XpsElementDefect::BrushUnreadable,
                })
                .map_err(Refused::Brush)
                .and_then(|mask| self.mask_gstate(&mask, inner_box, budget));
            match read {
                Ok((alpha, gs)) => {
                    mask_alpha = alpha;
                    mask_gs = gs;
                }
                Err(error) => {
                    self.refused(error)?;
                    return Ok(());
                }
            }
        }

        let opacity = done.opacity * mask_alpha;
        // 14.3: the canvas's rendering is composited as a whole, which is a
        // transparency group — and observably one only where two of its
        // children cover the same place. Where they do not, one alpha applied
        // to each child paints the identical picture out of no form XObject at
        // all, and one soft mask applied to each child masks the identical
        // picture.
        let grouped = (opacity < 1.0 || mask_gs.is_some()) && overlaps(&done.boxes);
        let mut body = Vec::new();
        if grouped {
            if let Some(bbox) = inner_box {
                let name = self.painter.name("Fm");
                let registered = self.builder.add_form(
                    &name,
                    &FormXObject {
                        bbox,
                        matrix: None,
                        group: Some(TransparencyGroup {
                            color_space: DeviceSpace::Rgb,
                            isolated: true,
                            knockout: false,
                        }),
                        content: &done.content,
                    },
                );
                if registered {
                    body.push(b'/');
                    body.extend_from_slice(&name);
                    body.extend_from_slice(b" Do\n");
                }
            }
        }
        if body.is_empty() {
            body = done.content;
        }

        let Some(parent) = scopes.last_mut() else {
            return Ok(());
        };
        parent.content.extend_from_slice(b"q\n");
        if done.transform != markup::IDENTITY {
            markup::op(&mut parent.content, &done.transform, "cm");
        }
        if let Some(clip) = &done.clip {
            clip.emit(&mut parent.content, true);
            parent
                .content
                .extend_from_slice(clip.clip_operator().as_bytes());
            parent.content.push(b'\n');
        }
        if opacity < 1.0 {
            if let Some(name) = self.gstate(opacity, opacity) {
                parent.content.push(b'/');
                parent.content.extend_from_slice(&name);
                parent.content.extend_from_slice(b" gs\n");
            }
        }
        if let Some(name) = &mask_gs {
            parent.content.push(b'/');
            parent.content.extend_from_slice(name);
            parent.content.extend_from_slice(b" gs\n");
        }
        parent.content.extend_from_slice(&body);
        parent.content.extend_from_slice(b"Q\n");

        if let Some(bbox) = inner_box {
            let bbox = clipped(bbox, done.clip.as_ref());
            if let Some(bbox) = bbox.and_then(|b| through(b, done.transform)) {
                parent.boxes.push(bbox);
            }
        }
        Ok(())
    }

    /// Draws one `Path`.
    fn path(
        &mut self,
        scopes: &mut [Scope],
        node: &Node,
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        // A canvas that refused itself refuses everything under it, and says
        // so once rather than once per descendant.
        if scopes.last().is_some_and(|scope| scope.refused) {
            return Ok(());
        }

        // The three that refuse the element, read before anything is painted
        // — an element drawn and then discovered to be in the wrong place has
        // already been drawn.
        let transform = match geometry::transform_of(node, "Path") {
            Ok(matrix) => matrix.unwrap_or(markup::IDENTITY),
            Err(_) => {
                self.warn(XpsElementDefect::TransformUnreadable);
                return Ok(());
            }
        };
        let opacity = match opacity_of(node) {
            Ok(value) => value,
            Err(()) => {
                self.warn(XpsElementDefect::OpacityUnreadable);
                return Ok(());
            }
        };
        let clip = match geometry::property(node, "Path", "Clip", budget) {
            Ok(clip) => clip,
            Err(GeometryError::Exhausted) => return Err(Trouble::Exhausted),
            Err(GeometryError::Syntax) => {
                self.warn(XpsElementDefect::ClipUnreadable);
                return Ok(());
            }
        };

        // And the one that is not painted rather than refusing: a `Data` that
        // will not read is a missing shape, which is visible as a missing
        // shape.
        let data = match geometry::property(node, "Path", "Data", budget) {
            Ok(Some(data)) => data,
            Ok(None) => {
                // 11.1 makes `Data` mandatory. A `Path` without one describes
                // no shape at all.
                self.warn(XpsElementDefect::GeometryUnreadable);
                return Ok(());
            }
            Err(GeometryError::Exhausted) => return Err(Trouble::Exhausted),
            Err(GeometryError::Syntax) => {
                self.warn(XpsElementDefect::GeometryUnreadable);
                return Ok(());
            }
        };
        if data.is_empty() {
            return Ok(());
        }
        let bbox = data.bbox();

        // 14.3's mask, read after the geometry because a mask is a brush over
        // the element's own box and refused for `Opacity`'s reason: a mask
        // ignored draws a whole shape where a sliver was meant.
        let mask = match self.mask_of(node, "Path", bbox) {
            None => None,
            Some(Ok(mask)) => match self.mask_gstate(&mask, bbox, budget) {
                Ok(applied) => Some(applied),
                Err(error) => {
                    self.refused(error)?;
                    return Ok(());
                }
            },
            Some(Err(defect)) => {
                self.warn(defect);
                return Ok(());
            }
        };

        let fill = self.brush_of(node, "Path", "Fill", bbox);
        let stroke = self.brush_of(node, "Path", "Stroke", bbox);
        if fill.is_none() && stroke.is_none() {
            // 11.1: a `Path` with neither is a shape nobody asked to see.
            return Ok(());
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"q\n");
        if transform != markup::IDENTITY {
            markup::op(&mut out, &transform, "cm");
        }
        if let Some(clip) = &clip {
            clip.emit(&mut out, true);
            out.extend_from_slice(clip.clip_operator().as_bytes());
            out.push(b'\n');
        }

        // A uniform mask is one more constant alpha, which is what 11.6.4.4
        // already says; a varying one is a second `gs` naming the soft mask.
        let mask_alpha = mask.as_ref().map_or(1.0, |(alpha, _)| *alpha);
        let fill_alpha = opacity * mask_alpha * fill.as_ref().map_or(1.0, |brush| brush.alpha);
        let stroke_alpha = opacity * mask_alpha * stroke.as_ref().map_or(1.0, |brush| brush.alpha);
        if fill_alpha < 1.0 || stroke_alpha < 1.0 {
            if let Some(name) = self.gstate(fill_alpha, stroke_alpha) {
                out.push(b'/');
                out.extend_from_slice(&name);
                out.extend_from_slice(b" gs\n");
            }
        }
        if let Some((_, Some(name))) = &mask {
            out.push(b'/');
            out.extend_from_slice(name);
            out.extend_from_slice(b" gs\n");
        }

        if let Some(brush) = fill {
            match brush.paint {
                Paint::Solid(rgb) => {
                    markup::op(&mut out, &rgb, "rg");
                    data.emit(&mut out, true);
                    out.extend_from_slice(data.fill_operator().as_bytes());
                    out.push(b'\n');
                }
                Paint::Gradient { shading, matrix } => {
                    // 8.7.4.1: `sh` fills the **clip**, not a shape, and this
                    // writer emits no shading pattern — so the shape becomes
                    // the clip. The whole thing is bracketed so the clip does
                    // not outlive the fill and take the stroke with it.
                    if self.shading(&shading, &mut out, &data, matrix) {
                        // written
                    }
                }
                Paint::Image(tile) => {
                    let ctm = self.in_force(scopes, transform);
                    match self.tile(&tile, bbox, ctm) {
                        Ok(name) => {
                            fill_with_pattern(&mut out, &name, &data);
                        }
                        Err(error) => grey_fill(self, &mut out, &data, error)?,
                    }
                }
                Paint::Visual(tile) => {
                    let ctm = self.in_force(scopes, transform);
                    match self.visual(&tile, bbox, ctm, budget) {
                        Ok(name) => {
                            fill_with_pattern(&mut out, &name, &data);
                        }
                        Err(error) => grey_fill(self, &mut out, &data, error)?,
                    }
                }
            }
        }
        if let Some(brush) = stroke {
            let ctm = self.in_force(scopes, transform);
            self.stroke(&mut out, node, &data, &brush, ctm, budget)?;
        }
        out.extend_from_slice(b"Q\n");

        if let Some(scope) = scopes.last_mut() {
            scope.content.extend_from_slice(&out);
            if let Some(bbox) = bbox
                .and_then(|b| clipped(b, clip.as_ref()))
                .and_then(|b| through(b, transform))
            {
                scope.boxes.push(bbox);
            }
        }
        Ok(())
    }

    /// Draws one `Glyphs` run (12.1).
    ///
    /// # Three refusals that are not the geometry rule, and one that is not a
    /// refusal
    ///
    /// 12.1 gives a run three attributes this build cannot draw, and gap 30's
    /// row 7 asks that each be *implemented or refused by name, never
    /// ignored*. They do not get the same answer, and which one each gets is
    /// decided by what ignoring it would produce:
    ///
    /// - **`IsSideways`** rotates every glyph a quarter turn about its origin.
    ///   Drawn upright it is a different picture in the same place, so the run
    ///   is **not painted**.
    /// - **An odd `BidiLevel`** is a right-to-left run, whose origin is its
    ///   *right* edge. Drawn left to right it is the same picture somewhere
    ///   else, which is the wrong-place failure the whole refusal asymmetry
    ///   exists for, so the run is **not painted**. An even level is a
    ///   left-to-right run at any embedding depth and draws normally, which is
    ///   the implemented half.
    /// - **`StyleSimulations`** adds a synthetic slant or weight to glyphs
    ///   that are otherwise exactly the ones the file names, at exactly the
    ///   widths and positions it states. That is the *paint* side of the
    ///   asymmetry rather than the geometry side — the shape and its place are
    ///   known and only its appearance is approximate — so the run **is
    ///   painted** and says so. Refusing it would drop a page of text to avoid
    ///   drawing it upright.
    fn glyphs(
        &mut self,
        scopes: &mut [Scope],
        node: &Node,
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        if scopes.last().is_some_and(|scope| scope.refused) {
            return Ok(());
        }
        // Copied out before the builder is touched: the font is borrowed from
        // the surroundings and the writer is borrowed from `self`, and a
        // reference held through `self` would make the two one borrow.
        let around = self.around;

        // The three that refuse the element, in `path`'s own order and for
        // `path`'s own reason: a run drawn and then found to be in the wrong
        // place has already been drawn.
        let transform = match geometry::transform_of(node, "Glyphs") {
            Ok(matrix) => matrix.unwrap_or(markup::IDENTITY),
            Err(_) => {
                self.warn(XpsElementDefect::TransformUnreadable);
                return Ok(());
            }
        };
        let opacity = match opacity_of(node) {
            Ok(value) => value,
            Err(()) => {
                self.warn(XpsElementDefect::OpacityUnreadable);
                return Ok(());
            }
        };
        let clip = match geometry::property(node, "Glyphs", "Clip", budget) {
            Ok(clip) => clip,
            Err(GeometryError::Exhausted) => return Err(Trouble::Exhausted),
            Err(GeometryError::Syntax) => {
                self.warn(XpsElementDefect::ClipUnreadable);
                return Ok(());
            }
        };

        // 12.1's three required numbers. All three or none: a run at an origin
        // this reader invented is text in the wrong place, which is the
        // geometry rule said about a run rather than about a shape.
        let origin_x = node.attr("OriginX").and_then(markup::number);
        let origin_y = node.attr("OriginY").and_then(markup::number);
        let em = node
            .attr("FontRenderingEmSize")
            .and_then(markup::number)
            .filter(|size| *size >= 0.0);
        let (Some(origin_x), Some(origin_y), Some(em)) = (origin_x, origin_y, em) else {
            self.warn(XpsElementDefect::GlyphsUnreadable);
            return Ok(());
        };

        match node.attr("IsSideways").map(markup::boolean) {
            None | Some(Some(false)) => {}
            Some(Some(true)) => {
                self.warn(XpsElementDefect::GlyphsSidewaysUnsupported);
                return Ok(());
            }
            Some(None) => {
                self.warn(XpsElementDefect::GlyphsUnreadable);
                return Ok(());
            }
        }
        match node
            .attr("BidiLevel")
            .map(|text| text.trim().parse::<u32>())
        {
            None => {}
            Some(Ok(level)) if level % 2 == 0 => {}
            Some(Ok(_)) => {
                self.warn(XpsElementDefect::GlyphsBidiUnsupported);
                return Ok(());
            }
            Some(Err(_)) => {
                self.warn(XpsElementDefect::GlyphsUnreadable);
                return Ok(());
            }
        }
        if node
            .attr("StyleSimulations")
            .is_some_and(|value| value.trim() != "None")
        {
            self.warn(XpsElementDefect::GlyphsStyleSimulated);
        }

        // 9.1.7's font, resolved against **this part's** name.
        let Some(uri) = node.attr("FontUri") else {
            self.warn(XpsElementDefect::GlyphsFontUnresolved);
            return Ok(());
        };
        let font = match around.fonts.get(around.part, uri) {
            Ok(font) => font,
            Err(defect) => {
                self.warn(defect);
                return Ok(());
            }
        };

        let placed = match glyphs::run(node, font, em, budget) {
            Ok(placed) => placed,
            Err(RunError::Exhausted) => return Err(Trouble::Exhausted),
            Err(RunError::Indices) => {
                self.warn(XpsElementDefect::GlyphsIndicesUnreadable);
                return Ok(());
            }
        };
        // A run of no glyphs, and an em size of zero, are both things 12.1
        // permits and neither is a defect: a `Glyphs` with no `UnicodeString`
        // and no `Indices` names no glyph, and one at size zero is invisible
        // by arithmetic rather than by omission.
        if placed.is_empty() || em == 0.0 {
            return Ok(());
        }

        // The box the run occupies: its own advance along the baseline, and
        // one em above it. Not the ink — this reader does not read outlines —
        // and it is used for exactly two decisions that are about *where* a
        // thing is: 14.3's overlap test, and the box a
        // `RelativeToBoundingBox` brush is stated in fractions of.
        let (low, high) = glyphs::extent(&placed, font, em);
        let bbox = [origin_x + low, origin_y - em, origin_x + high, origin_y];

        // 14.3's mask, over the box the run occupies, and refused for `path`'s
        // reason where it will not read.
        let mask = match self.mask_of(node, "Glyphs", Some(bbox)) {
            None => None,
            Some(Ok(mask)) => match self.mask_gstate(&mask, Some(bbox), budget) {
                Ok(applied) => Some(applied),
                Err(error) => {
                    self.refused(error)?;
                    return Ok(());
                }
            },
            Some(Err(defect)) => {
                self.warn(defect);
                return Ok(());
            }
        };

        // 12.1 makes `Fill` required, and a run without one is text nobody
        // asked to see — `path`'s answer to a shape with neither brush, for
        // the same reason.
        let Some(fill) = self.brush_of(node, "Glyphs", "Fill", Some(bbox)) else {
            return Ok(());
        };

        let mut out = Vec::new();
        out.extend_from_slice(b"q\n");
        if transform != markup::IDENTITY {
            markup::op(&mut out, &transform, "cm");
        }
        if let Some(clip) = &clip {
            clip.emit(&mut out, true);
            out.extend_from_slice(clip.clip_operator().as_bytes());
            out.push(b'\n');
        }
        let alpha = opacity * mask.as_ref().map_or(1.0, |(alpha, _)| *alpha) * fill.alpha;
        if alpha < 1.0 {
            if let Some(name) = self.gstate(alpha, alpha) {
                out.push(b'/');
                out.extend_from_slice(&name);
                out.extend_from_slice(b" gs\n");
            }
        }
        if let Some((_, Some(name))) = &mask {
            out.push(b'/');
            out.extend_from_slice(name);
            out.extend_from_slice(b" gs\n");
        }
        // A brush over text is set as a *colour* and not as a region: 9.4's
        // glyphs are filled with whatever the non-stroking colour is when `Tj`
        // runs, and a glyph outline is not a clip a content stream can state —
        // which is why a gradient over text is a `/PatternType 2` here and an
        // `sh` over a clip on a `Path`.
        let ctm = self.in_force(scopes, transform);
        match &fill.paint {
            Paint::Solid(rgb) => markup::op(&mut out, rgb, "rg"),
            Paint::Gradient { shading, matrix } => match self.gradient(shading, *matrix, ctm) {
                Some(name) => fill_pattern_colour(&mut out, &name),
                None => {
                    self.warn(XpsElementDefect::BrushUnreadable);
                    markup::op(&mut out, &[PLACEHOLDER_GREY; 3], "rg");
                }
            },
            Paint::Image(tile) => match self.tile(tile, Some(bbox), ctm) {
                Ok(name) => fill_pattern_colour(&mut out, &name),
                Err(error) => {
                    self.refused(error)?;
                    markup::op(&mut out, &[PLACEHOLDER_GREY; 3], "rg");
                }
            },
            Paint::Visual(tile) => match self.visual(tile, Some(bbox), ctm, budget) {
                Ok(name) => fill_pattern_colour(&mut out, &name),
                Err(error) => {
                    self.refused(error)?;
                    markup::op(&mut out, &[PLACEHOLDER_GREY; 3], "rg");
                }
            },
        }

        let run: Vec<PlacedGlyph<'_>> = placed
            .iter()
            .map(|glyph| PlacedGlyph {
                glyph: Glyph {
                    id: glyph.id,
                    text: &glyph.text,
                },
                x: glyph.x,
                rise: glyph.rise,
            })
            .collect();
        // 18.1's flip lives in the page's one `cm`, so user space here has y
        // increasing **downward** — and a text matrix that did not undo it
        // would draw every glyph upside down at exactly the right place, which
        // is the most plausible wrong picture in this whole milestone.
        let matrix = [1.0, 0.0, 0.0, -1.0, origin_x, origin_y];
        if !self
            .builder
            .glyph_run(&mut out, &font.resource, em, matrix, &run)
        {
            // The writer refused the run: the resource is not a composite font
            // after all, or a number in it is not one a content stream can
            // carry. Nothing was written, and the run is named rather than
            // left as a `q Q` pair that draws nothing.
            self.warn(XpsElementDefect::GlyphsFontUnreadable);
            return Ok(());
        }
        out.extend_from_slice(b"Q\n");

        if let Some(scope) = scopes.last_mut() {
            scope.content.extend_from_slice(&out);
            if let Some(bbox) = clipped(bbox, clip.as_ref()).and_then(|b| through(b, transform)) {
                scope.boxes.push(bbox);
            }
        }
        Ok(())
    }

    /// 18.1's own `cm`, then every open scope's transform, then this element's.
    ///
    /// This is the *only* place in the painter that needs it, and it exists
    /// because 8.7.3.1 says a tiling pattern's `/Matrix` is relative to the
    /// page's default space and **ignores the transform in force**. Every other
    /// operator this module writes is inside the `q ... Q` that carries those
    /// transforms, which is why the numbers in a content stream are the numbers
    /// in the markup — a property gap 30's geometry section asks not to be
    /// thrown away, and one a pattern cannot have.
    fn in_force(&self, scopes: &[Scope], element: [f64; 6]) -> [f64; 6] {
        // `markup::concat(first, second)` is "first then second", so the
        // *innermost* transform is the first argument and this builds outward:
        // the element's own, then each open scope from the inside, then 18.1's.
        // Written this way round rather than the other because the other way
        // round compiles, runs, and puts the picture four thousand points down
        // the page — which is what it did before this comment existed.
        let mut matrix = element;
        for scope in scopes.iter().rev() {
            matrix = markup::concat(matrix, scope.transform);
        }
        // 18.1: one XPS unit is 1/96 inch against PDF's 1/72, origin top left
        // with y downward. The page's height arrives **already in points** —
        // `plan.size` is scaled by `UNITS_TO_POINTS` where it is built — so
        // scaling it again here is the same bug in a smaller place.
        let page = [
            UNITS_TO_POINTS,
            0.0,
            0.0,
            -UNITS_TO_POINTS,
            0.0,
            self.around.page.1,
        ];
        markup::concat(matrix, page)
    }

    /// Writes an image fill: a tiling pattern, then the shape filled with it.
    ///
    /// # The three spaces, and why the arithmetic is written out
    ///
    /// An `ImageBrush` states two rectangles in two different spaces and PDF
    /// wants a third. `viewbox` says which part of the *image* to show, in the
    /// image's own units — a 384-pixel PNG at 96 dpi is four units across, which
    /// is why [`super::image::Image::units`] exists and why 13.4.1's resolution
    /// is not decoration. `viewport` says where that goes in the *element's*
    /// space. PDF's pattern (8.7.3.1) wants a cell in pattern space, two steps,
    /// and a `/Matrix` into the page's **default** space — not into whatever
    /// transform is in force when the pattern is set, which is the trap this
    /// method exists to keep in one place.
    ///
    /// So the cell is built in image-unit space, one image across, and the
    /// matrix carries three things at once: the viewbox-to-viewport scale, the
    /// viewport's own offset, and the element's current transform. A build that
    /// left the last of those out would draw every tiled picture at the page's
    /// origin, which looks like a clipping bug and is not one.
    ///
    /// # Only the cell's content is this method's own
    ///
    /// Everything about *where* the tile goes — the two rectangles, their two
    /// unit modes, the five `TileMode`s including the reflections PDF has no
    /// operator for, and the `/Matrix` into default space — is [`Placed`]'s,
    /// and is shared verbatim with [`State::visual`]. What is here is the one
    /// thing an `ImageBrush` does that a `VisualBrush` does not: put a picture
    /// in the cell, once per reflection.
    fn tile(
        &mut self,
        tile: &ImageTile,
        bbox: Option<[f64; 4]>,
        ctm: [f64; 6],
    ) -> Result<Vec<u8>, Refused> {
        // The picture is not there, is a format this build refuses, or carries
        // a colour profile — each already named by `Images::get`. The *shape* is
        // still known, so the caller paints the placeholder and the rest of the
        // page is untouched, which is row 8's own requirement.
        let image = self.around.images.get(self.around.part, &tile.source)?;
        let resource = image.resource.clone();
        let source = image.units();
        let placed = Placed::of(&tile.placement, source, bbox)?;

        // The cell's content: the image drawn once per reflection. 8.9.5.2 puts
        // an image in the unit square, so the `cm` is the image's own extent
        // and a negative scale with a compensating translation is the mirror.
        let (w, h) = (placed.viewbox[2], placed.viewbox[3]);
        let mut cell = Vec::new();
        for (fx, fy) in placed.flips() {
            let (ox, oy) = Placed::mirror_origin(*fx, *fy, w, h);
            cell.extend_from_slice(b"q\n");
            markup::op(&mut cell, &[w * fx, 0.0, 0.0, h * fy, ox, oy], "cm");
            cell.push(b'/');
            cell.extend_from_slice(&resource);
            cell.extend_from_slice(b" Do\nQ\n");
        }
        self.pattern(&placed, &cell, ctm)
    }

    /// Writes a `VisualBrush`'s fill: a tiling pattern whose cell is a
    /// **drawing** (15.4).
    ///
    /// # Only the cell's content differs from [`State::tile`]
    ///
    /// Everything about where a tile goes — 15.3's two rectangles, their two
    /// unit modes, the five `TileMode`s including the reflections PDF has no
    /// operator for, and 8.7.3.1's `/Matrix` into the page's default space — is
    /// the same arithmetic, and it is done once in [`Placed`] rather than
    /// twice. What changes is that the cell holds the subtree instead of one
    /// `Do`.
    ///
    /// # Re-entering the drawing walk, and the two bounds on it
    ///
    /// The subtree is markup, so painting it means running the element handlers
    /// again from inside a brush. Two things can go wrong and they are **two
    /// rules**:
    ///
    /// - a `VisualBrush` whose subtree states another one is legal to any depth
    ///   the file chooses, and is bounded by [`MAX_XPS_VISUAL_DEPTH`];
    /// - a `VisualBrush` reached through a `{StaticResource}` whose own subtree
    ///   names that key again never terminates, and no single lookup chain can
    ///   see it because each lookup starts afresh — so the keys currently being
    ///   painted are carried and a repeat is refused.
    ///
    /// They answer under [`XpsElementDefect::BrushTooDeep`] and
    /// [`XpsElementDefect::BrushCyclic`], which are the names a
    /// `{StaticResource}` chain already uses for the same two failures.
    fn visual(
        &mut self,
        tile: &VisualTile,
        bbox: Option<[f64; 4]>,
        ctm: [f64; 6],
        budget: &mut Budget,
    ) -> Result<Vec<u8>, Refused> {
        // The cycle is checked before the depth, so a brush that is both is
        // reported as the cycle: a cycle is a statement about the file and a
        // depth is a statement about this build's cap, and the first is the
        // more useful of the two to be told.
        if let Some(key) = &tile.key {
            if self.visuals.iter().any(|open| open.as_ref() == Some(key)) {
                return Err(Refused::Brush(XpsElementDefect::BrushCyclic));
            }
        }
        if self.visuals.len() >= MAX_XPS_VISUAL_DEPTH {
            return Err(Refused::Brush(XpsElementDefect::BrushTooDeep));
        }
        // Pushed whether or not the brush was keyed, so the depth is the
        // nest's and not the keyed nest's.
        self.visuals.push(tile.key.clone());
        // One copy of the drawing, in the visual's own space, painted into a
        // scope stack of its own. The root scope is this brush's cell and not
        // the page's — a `Canvas` inside the visual still folds into its
        // parent the ordinary way, and the page's own scopes are not reachable
        // from here, which is what stops a visual's `Canvas` from closing one
        // of them.
        let mut scopes = vec![Scope::root()];
        let drawn = self.draw_subtree(&mut scopes, &tile.visual, budget);
        // Popped before the `?`, so a subtree that exhausted the budget still
        // leaves the depth where it found it.
        self.visuals.pop();
        drawn?;
        let root = scopes.pop();
        let extent = root.as_ref().and_then(|root| union(&root.boxes));
        let copy = root.map(|root| root.content).unwrap_or_default();

        // The drawing is measured **before** it is placed, because 15.3's
        // `ViewboxUnits` defaults to `RelativeToBoundingBox` and the box a
        // visual's viewbox is a fraction of is the visual's *own* — the
        // counterpart of the image's pixel extent, and the one thing about a
        // `VisualBrush` that cannot be known until the subtree has been drawn.
        // A visual that drew nothing has no extent; the unit square keeps the
        // arithmetic finite and the cell is empty either way.
        let source = extent.map_or((1.0, 1.0), |b| (b[2] - b[0], b[3] - b[1]));
        let placed = Placed::of(&tile.placement, source, bbox)?;

        // The reflections place that copy, mirrored, the way `tile` places the
        // image — and unlike 8.9.5.2's image there is no unit square to scale
        // out of, so the mirror is a bare `-1` about the far edge of the cell.
        let (w, h) = (placed.viewbox[2], placed.viewbox[3]);
        let mut cell = Vec::new();
        for (fx, fy) in placed.flips() {
            let (ox, oy) = Placed::mirror_origin(*fx, *fy, w, h);
            cell.extend_from_slice(b"q\n");
            markup::op(&mut cell, &[*fx, 0.0, 0.0, *fy, ox, oy], "cm");
            cell.extend_from_slice(&copy);
            cell.extend_from_slice(b"Q\n");
        }
        self.pattern(&placed, &cell, ctm)
    }

    /// Registers the tiling pattern a placed cell becomes.
    ///
    /// 8.7.3.1's `/Matrix` maps pattern space into the page's **default**
    /// space and ignores the transform in force, so the element's own transform
    /// arrives as `ctm` and is composed in here rather than left to the `cm`
    /// the operator sits inside.
    fn pattern(&mut self, placed: &Placed, cell: &[u8], ctm: [f64; 6]) -> Result<Vec<u8>, Refused> {
        // `place` maps pattern space (the source's own units) into the
        // element's space and the CTM maps that into the page's, so `place` is
        // the inner one. The other order scales the translation by the
        // viewbox-to-viewport factor, which puts a 200-unit picture four
        // thousand points down the page.
        let matrix = markup::concat(placed.place(), ctm);
        let (cell_w, cell_h) = placed.cell();
        let name = self.painter.name("P");
        let pattern = TilingPattern {
            bbox: [0.0, 0.0, cell_w, cell_h],
            // `TileMode::None` draws the source once. PDF has no such thing —
            // a pattern always repeats — so the step is made large enough that
            // no second cell can reach the shape, and the clip does the rest.
            x_step: if placed.tile == TileMode::None {
                NO_TILE_STEP
            } else {
                cell_w
            },
            y_step: if placed.tile == TileMode::None {
                NO_TILE_STEP
            } else {
                cell_h
            },
            matrix: Some(matrix),
            tiling_type: TilingType::ConstantSpacing,
            content: cell,
        };
        if !self.builder.add_tiling_pattern(&name, &pattern) {
            // A degenerate cell or a non-finite number: the writer refused it,
            // and the shape takes the placeholder rather than nothing.
            return Err(Refused::Brush(XpsElementDefect::BrushUnreadable));
        }
        Ok(name)
    }

    /// Draws a materialised subtree, which is what a `VisualBrush`'s cell is.
    ///
    /// [`State::run`]'s walk cannot serve here: its elements arrive as parser
    /// events, and a brush's visual has already been materialised by
    /// [`markup::subtree`] as part of the brush that carries it. Every
    /// *element* handler is shared — `canvas`, `path`, `glyphs` and `property`
    /// each take a [`Node`] — so what this adds is the dispatch over children,
    /// not a second reading of sections 11 to 15.
    ///
    /// # A `Canvas` is opened on its attributes alone, exactly as `run` does
    ///
    /// [`State::canvas`] reads a transform from the element's attribute **or**
    /// from its `Canvas.RenderTransform` child, because [`super::markup::leaf`]
    /// hands it a node with no children and the child arrives separately as a
    /// property event. Here the node is whole, so handing it over intact would
    /// have `canvas` read the property element *and* the loop below hand the
    /// same element to [`State::property`] — which composes rather than
    /// assigns, and would square every canvas transform inside a brush. So the
    /// canvas is opened on a leaf view and its children are dispatched, which
    /// is the streamed walk's own division and produces the same answer by
    /// construction rather than by agreement.
    fn draw_subtree(
        &mut self,
        scopes: &mut Vec<Scope>,
        node: &Node,
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        budget.element()?;
        if !node.xps {
            // 14.1's foreign markup, named rather than passed over in silence,
            // which is what `run` does with the same element.
            self.warn(XpsElementDefect::ElementUnknown);
            return Ok(());
        }
        let owner = scopes.last().map_or("FixedPage", |scope| scope.owner);
        if node.property_of(owner).is_some() {
            return self.property(scopes, node, budget);
        }
        match node.local.as_str() {
            "Canvas" => {
                let scope = self.canvas(scopes, &leaf_of(node), budget)?;
                scopes.push(scope);
                // A canvas's property elements are values and its drawables are
                // children, and document order between the two does not matter
                // — which is what the scope's own content buffer is for.
                for child in &node.children {
                    self.draw_subtree(scopes, child, budget)?;
                }
                if let Some(done) = scopes.pop() {
                    self.dictionaries
                        .truncate(self.dictionaries.len().saturating_sub(done.dictionaries));
                    self.close(scopes, done, budget)?;
                }
                Ok(())
            }
            "Path" => self.path(scopes, node, budget),
            "Glyphs" => self.glyphs(scopes, node, budget),
            _ => {
                self.warn(XpsElementDefect::ElementUnknown);
                Ok(())
            }
        }
    }

    /// A gradient as a `/PatternType 2` pattern (8.7.4.5.5), which is the one
    /// shape a **stroke** and a **glyph run** can take a gradient in.
    ///
    /// A *fill* keeps 8.7.4.1's `sh` over the shape as a clip: the two paint
    /// the same picture, `sh` says it in one operator with no colour space to
    /// change, and the clip is exactly the region being filled. Neither move is
    /// available to a stroke or to text — a stroke is not a region, and a glyph
    /// outline is not a clip a content stream can state — so those go through
    /// the pattern, where 8.7.3.2 makes a gradient a *colour*.
    ///
    /// 8.7.3.1's rule about the matrix is why `ctm` is a parameter, and it is
    /// [`State::tile`]'s trap one shading over: a pattern's `/Matrix` maps
    /// pattern space into the page's **default** space and ignores the
    /// transform in force, so the element's own transform is composed in here
    /// rather than left to the `cm` the operator sits inside.
    fn gradient(&mut self, shading: &Shading, matrix: [f64; 6], ctm: [f64; 6]) -> Option<Vec<u8>> {
        let name = self.painter.name("P");
        let pattern = ShadingPattern {
            shading: shading.clone(),
            matrix: Some(markup::concat(matrix, ctm)),
        };
        self.builder
            .add_shading_pattern(&name, &pattern)
            .then_some(name)
    }

    /// Writes a gradient fill: the shape as a clip, then `sh` over it.
    fn shading(
        &mut self,
        shading: &Shading,
        out: &mut Vec<u8>,
        data: &Geometry,
        matrix: [f64; 6],
    ) -> bool {
        let name = self.painter.name("Sh");
        if !self.builder.add_shading(&name, shading) {
            // The writer refused it — a zero-length axis, a negative radius, a
            // function whose arity is wrong. The shape is still known, so it
            // takes the placeholder rather than nothing.
            markup::op(out, &[PLACEHOLDER_GREY], "g");
            data.emit(out, true);
            out.extend_from_slice(data.fill_operator().as_bytes());
            out.push(b'\n');
            return false;
        }
        out.extend_from_slice(b"q\n");
        data.emit(out, true);
        out.extend_from_slice(data.clip_operator().as_bytes());
        out.push(b'\n');
        if matrix != markup::IDENTITY {
            markup::op(out, &matrix, "cm");
        }
        out.push(b'/');
        out.extend_from_slice(&name);
        out.extend_from_slice(b" sh\nQ\n");
        true
    }

    /// Writes the stroke: the line parameters 11.1 states, and a colour or a
    /// pattern.
    ///
    /// Every brush this build paints can stroke, because 8.7.3.2's pattern is a
    /// *colour* and `SCN` takes one — a gradient through
    /// [`State::gradient`]'s `/PatternType 2` and an `ImageBrush` through
    /// [`State::tile`]'s `/PatternType 1`. What is left grey is a brush that
    /// did not become paint at all, which is `brush_of`'s answer and not this
    /// one.
    fn stroke(
        &mut self,
        out: &mut Vec<u8>,
        node: &Node,
        data: &Geometry,
        brush: &Brush,
        ctm: [f64; 6],
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        // The box a `RelativeToBoundingBox` brush is a fraction of is the
        // shape's own, which is `data`'s — taken here rather than passed in,
        // because the only caller derived it from `data` and two ways to know
        // one thing is one of them eventually being wrong.
        let bbox = data.bbox();
        let width = node
            .attr("StrokeThickness")
            .map_or(Some(1.0), markup::number);
        let Some(width) = width.filter(|w| *w >= 0.0) else {
            self.warn(XpsElementDefect::GeometryUnreadable);
            return Ok(());
        };
        markup::op(out, &[width], "w");
        if let Some(cap) = node.attr("StrokeStartLineCap").and_then(line_cap) {
            markup::op(out, &[f64::from(cap)], "J");
        }
        if let Some(join) = node.attr("StrokeLineJoin").and_then(line_join) {
            markup::op(out, &[f64::from(join)], "j");
        }
        if let Some(limit) = node.attr("StrokeMiterLimit").and_then(markup::number) {
            markup::op(out, &[limit], "M");
        }
        match &brush.paint {
            Paint::Solid(rgb) => markup::op(out, rgb, "RG"),
            Paint::Gradient { shading, matrix } => match self.gradient(shading, *matrix, ctm) {
                Some(name) => stroke_with_pattern(out, &name),
                None => {
                    // The writer refused the shading: a zero-length axis, a
                    // negative radius, a function whose arity is wrong. The
                    // line is still known, so it takes the placeholder rather
                    // than nothing.
                    self.warn(XpsElementDefect::BrushUnreadable);
                    markup::op(out, &[PLACEHOLDER_GREY], "G");
                }
            },
            Paint::Image(tile) => match self.tile(tile, bbox, ctm) {
                Ok(name) => stroke_with_pattern(out, &name),
                Err(error) => {
                    self.refused(error)?;
                    markup::op(out, &[PLACEHOLDER_GREY], "G");
                }
            },
            Paint::Visual(tile) => match self.visual(tile, bbox, ctm, budget) {
                Ok(name) => stroke_with_pattern(out, &name),
                Err(error) => {
                    self.refused(error)?;
                    markup::op(out, &[PLACEHOLDER_GREY], "G");
                }
            },
        }
        data.emit(out, false);
        out.extend_from_slice(b"S\n");
        Ok(())
    }

    /// The brush an attribute names, as markup.
    ///
    /// Two spellings and both are real: 14.2.3's `{StaticResource key}`, and
    /// 15.2.4's bare colour, which XAML lets any brush-valued property take as
    /// shorthand for a `SolidColorBrush` of that colour. The colour form is
    /// synthesised into the element it stands for rather than answered
    /// separately, so every caller reads one kind of thing.
    fn attribute_brush(&mut self, value: &str) -> Result<Node, XpsElementDefect> {
        if let Some(key) = reference(value) {
            return self.lookup(key);
        }
        match brush::colour(value) {
            Ok(_) => Ok(Node {
                local: "SolidColorBrush".to_string(),
                xps: true,
                key: None,
                attrs: vec![("Color".to_string(), value.to_string())],
                children: Vec::new(),
            }),
            Err(BrushError::Unsupported) => Err(XpsElementDefect::BrushUnsupported),
            Err(BrushError::Syntax) => Err(XpsElementDefect::BrushUnreadable),
        }
    }

    /// Reads 14.3's `OpacityMask` off an element, in both spellings.
    ///
    /// `None` when the element states none, which is not a defect. Otherwise a
    /// mask or the name of what went wrong with it — and a mask that went wrong
    /// **refuses its element**, for the reason `Opacity` does: a mask ignored
    /// draws a whole shape where a sliver was meant.
    fn mask_of(
        &mut self,
        node: &Node,
        owner: &str,
        bbox: Option<[f64; 4]>,
    ) -> Option<Result<Mask, XpsElementDefect>> {
        let named = |defect: XpsElementDefect| Some(Err(defect));
        let from_node = |node: &Node| match brush::mask_from_node(node, bbox) {
            Ok(mask) => Some(Ok(mask)),
            Err(BrushError::Unsupported) => named(XpsElementDefect::BrushUnsupported),
            Err(BrushError::Syntax) => named(XpsElementDefect::BrushUnreadable),
        };

        if let Some(value) = node.attr("OpacityMask") {
            return match self.attribute_brush(value) {
                Ok(node) => from_node(&node),
                Err(defect) => named(defect),
            };
        }
        let wanted = format!("{owner}.OpacityMask");
        let child = node
            .children
            .iter()
            .find(|child| child.xps && child.local == wanted)?;
        let Some(brush) = child.children.iter().find(|node| node.xps) else {
            return named(XpsElementDefect::BrushUnreadable);
        };
        from_node(brush)
    }

    /// Turns a mask into what a content stream can say: an extra constant
    /// alpha, and an `/ExtGState` naming a soft mask where one is needed.
    ///
    /// # Why three constructions rather than one
    ///
    /// 14.3 makes the mask a **brush used as an alpha channel**, and where a
    /// brush keeps its alpha differs by brush:
    ///
    /// - a `SolidColorBrush`, and a gradient whose stops share one alpha, are
    ///   one number over the whole element — which is what 11.6.4.4's `/ca` and
    ///   `/CA` already say, so no form XObject is built at all;
    /// - a gradient whose stops' alphas **differ** varies across the element,
    ///   and 8.7.4.5's shading has nowhere to put an alpha — so the alphas are
    ///   painted as a grey and read back by 11.6.5.2's `/Luminosity`;
    /// - an `ImageBrush`'s alpha is the picture's own, which nothing but the
    ///   picture can supply — so the brush is painted as it stands and
    ///   `/Alpha` reads the alpha the painting produced.
    ///
    /// `Err` is a mask this build could not place: a degenerate shading, a
    /// picture that is not there, a form the writer refused. The element is
    /// refused rather than drawn unmasked.
    fn mask_gstate(
        &mut self,
        mask: &Mask,
        bbox: Option<[f64; 4]>,
        budget: &mut Budget,
    ) -> Result<(f64, Option<Vec<u8>>), Refused> {
        let Mask::Uniform(uniform) = mask else {
            // Everything but the uniform case is painted into a form, and a
            // form's `/BBox` is the element's own extent. An element with no
            // extent has nothing for a mask to vary across.
            let bbox = bbox.ok_or(Refused::Brush(XpsElementDefect::BrushUnreadable))?;
            return self.mask_form(mask, bbox, budget);
        };
        Ok((*uniform, None))
    }

    /// The two masks that need a form XObject and a soft mask.
    fn mask_form(
        &mut self,
        mask: &Mask,
        bbox: [f64; 4],
        budget: &mut Budget,
    ) -> Result<(f64, Option<Vec<u8>>), Refused> {
        let (opacity, kind, content, space) = match mask {
            // Answered by `mask_gstate`, which is the only caller.
            Mask::Uniform(alpha) => return Ok((*alpha, None)),
            Mask::Luminosity {
                shading,
                matrix,
                opacity,
            } => {
                let name = self.painter.name("Sh");
                if !self.builder.add_shading(&name, shading) {
                    return Err(Refused::Brush(XpsElementDefect::BrushUnreadable));
                }
                let mut content = Vec::new();
                if *matrix != markup::IDENTITY {
                    markup::op(&mut content, matrix, "cm");
                }
                // 8.7.4.1's `sh` floods the current clip, and inside a form
                // that is the form's own `/BBox` — which is the element being
                // masked. Nothing else has to be said.
                content.push(b'/');
                content.extend_from_slice(&name);
                content.extend_from_slice(b" sh\n");
                (*opacity, MaskKind::Luminosity, content, DeviceSpace::Gray)
            }
            Mask::Alpha { paint, opacity } => {
                // 8.7.3.1's pattern matrix maps into the default space of the
                // stream the pattern is used in, and this one is used inside
                // the mask form — whose space is the element's, because
                // 11.6.5.2 renders the group under the CTM in force when the
                // `gs` sets the mask. So the element's transform is *not*
                // composed in here, which is the one place in this module
                // where the answer differs from `tile`'s caller on a page.
                let name = match paint {
                    Paint::Image(tile) => self.tile(tile, Some(bbox), markup::IDENTITY)?,
                    Paint::Visual(tile) => {
                        self.visual(tile, Some(bbox), markup::IDENTITY, budget)?
                    }
                    // `brush::mask_from_node` sends only the two tiling
                    // brushes here; the other two carry their alpha somewhere
                    // a constant or a shading can hold it.
                    Paint::Solid(_) | Paint::Gradient { .. } => {
                        return Err(Refused::Brush(XpsElementDefect::BrushUnreadable))
                    }
                };
                let mut content = Vec::new();
                fill_pattern_colour(&mut content, &name);
                markup::op(
                    &mut content,
                    &[bbox[0], bbox[1], bbox[2] - bbox[0], bbox[3] - bbox[1]],
                    "re",
                );
                content.extend_from_slice(b"f\n");
                (*opacity, MaskKind::Alpha, content, DeviceSpace::Rgb)
            }
        };

        let form = self.painter.name("Fm");
        // 11.6.5.2 requires the mask's form to be a **transparency group** and
        // `add_ext_gstate` refuses one that is not, so the `/Group` here is not
        // decoration: without it the mask is silently absent and the element
        // draws whole.
        if !self.builder.add_form(
            &form,
            &FormXObject {
                bbox,
                matrix: None,
                group: Some(TransparencyGroup {
                    color_space: space,
                    isolated: true,
                    knockout: false,
                }),
                content: &content,
            },
        ) {
            return Err(Refused::Brush(XpsElementDefect::BrushUnreadable));
        }
        let gs = self.painter.name("GS");
        if !self.builder.add_ext_gstate(
            &gs,
            &ExtGState {
                soft_mask: Some(StateMask::Group {
                    kind,
                    form: &form,
                    // 11.6.5.2 defaults `/BC` to black, which is a luminosity
                    // of zero: outside what the mask brush painted, the element
                    // is hidden. That is 14.3's own answer and writing the
                    // default out would say it twice.
                    backdrop: None,
                }),
                ..ExtGState::default()
            },
        ) {
            return Err(Refused::Brush(XpsElementDefect::BrushUnreadable));
        }
        Ok((opacity, Some(gs)))
    }

    /// An `/ExtGState` for a pair of alphas, written once per distinct pair.
    fn gstate(&mut self, fill: f64, stroke: f64) -> Option<Vec<u8>> {
        let key = (fill.to_bits(), stroke.to_bits());
        if let Some(name) = self.painter.gstates.get(&key) {
            return Some(name.clone());
        }
        let name = self.painter.name("GS");
        let written = self.builder.add_ext_gstate(
            &name,
            &ExtGState {
                fill_alpha: Some(fill),
                stroke_alpha: Some(stroke),
                ..ExtGState::default()
            },
        );
        if !written {
            return None;
        }
        self.painter.gstates.insert(key, name.clone());
        Some(name)
    }

    /// Resolves one brush-valued property, in both spellings, through
    /// `{StaticResource}` where it is one.
    ///
    /// `None` means the element stated no brush for this property at all,
    /// which is not a defect: 11.1 lets a `Path` state only a `Fill`, only a
    /// `Stroke`, or both. Everything else answers with the placeholder grey
    /// and a name.
    fn brush_of(
        &mut self,
        node: &Node,
        owner: &str,
        property: &str,
        bbox: Option<[f64; 4]>,
    ) -> Option<Brush> {
        let grey = |defect: XpsElementDefect, state: &mut Self| {
            state.warn(defect);
            Some(Brush {
                paint: Paint::Solid([PLACEHOLDER_GREY; 3]),
                alpha: 1.0,
                approximated: false,
            })
        };

        if let Some(value) = node.attr(property) {
            return match reference(value) {
                None => match brush::from_attribute(value) {
                    Ok(brush) => Some(brush),
                    Err(BrushError::Unsupported) => grey(XpsElementDefect::BrushUnsupported, self),
                    Err(BrushError::Syntax) => grey(XpsElementDefect::BrushUnreadable, self),
                },
                Some(key) => match self.lookup(key) {
                    Ok(node) => self.brush_from_node(&node, bbox),
                    Err(defect) => grey(defect, self),
                },
            };
        }

        let wanted = format!("{owner}.{property}");
        let child = node
            .children
            .iter()
            .find(|child| child.xps && child.local == wanted)?;
        let Some(brush) = child.children.iter().find(|node| node.xps) else {
            return grey(XpsElementDefect::BrushUnreadable, self);
        };
        self.brush_from_node(brush, bbox)
    }

    fn brush_from_node(&mut self, node: &Node, bbox: Option<[f64; 4]>) -> Option<Brush> {
        match brush::from_node(node, bbox) {
            Ok(brush) => {
                if brush.approximated {
                    self.warn(XpsElementDefect::BrushApproximated);
                }
                Some(brush)
            }
            Err(BrushError::Unsupported) => {
                self.warn(XpsElementDefect::BrushUnsupported);
                Some(Brush {
                    paint: Paint::Solid([PLACEHOLDER_GREY; 3]),
                    alpha: 1.0,
                    approximated: false,
                })
            }
            Err(BrushError::Syntax) => {
                self.warn(XpsElementDefect::BrushUnreadable);
                Some(Brush {
                    paint: Paint::Solid([PLACEHOLDER_GREY; 3]),
                    alpha: 1.0,
                    approximated: false,
                })
            }
        }
    }

    /// 14.2.5's lookup, and the alias chain a dictionary entry may start.
    ///
    /// Two guards, and they are **two rules** rather than one: a chain of
    /// distinct keys twenty long is not a cycle and a cycle two long is not
    /// deep. Deleting either leaves the other passing every test written for
    /// it, so each has its own name in the report and its own fixture.
    fn lookup(&mut self, key: &str) -> Result<Node, XpsElementDefect> {
        let mut seen: Vec<String> = Vec::new();
        let mut wanted = key.to_string();
        let mut visible = self.dictionaries.len();
        for _ in 0..=MAX_XPS_RESOURCE_DEPTH {
            if seen.contains(&wanted) {
                return Err(XpsElementDefect::BrushCyclic);
            }
            seen.push(wanted.clone());
            // Innermost first: an inner dictionary's key hides an outer one's,
            // and a key an inner dictionary does not hold is still found
            // further out. Two halves of one clause, and one fixture for each.
            let found = self.dictionaries[..visible]
                .iter()
                .enumerate()
                .rev()
                .find_map(|(at, dictionary)| {
                    dictionary
                        .entries
                        .get(&wanted)
                        .map(|node| (at, node.clone()))
                });
            let Some((at, node)) = found else {
                return Err(XpsElementDefect::BrushUnresolved);
            };
            // An entry's own references resolve from **its** dictionary
            // outward, not from wherever it is used.
            visible = at + 1;
            match node.key.as_deref().and_then(|_| alias(&node)) {
                Some(next) => wanted = next.to_string(),
                None => return Ok(node),
            }
        }
        Err(XpsElementDefect::BrushTooDeep)
    }
}

/// A materialised element with its children dropped, which is what
/// [`super::markup::leaf`] produces from a start tag.
///
/// One caller — [`State::draw_subtree`] opening a `Canvas` — and it is there
/// so the buffered walk and the streamed walk hand [`State::canvas`] the same
/// shape of node. See that method's own note for what handing it the whole
/// subtree would do.
fn leaf_of(node: &Node) -> Node {
    Node {
        local: node.local.clone(),
        xps: node.xps,
        key: node.key.clone(),
        attrs: node.attrs.clone(),
        children: Vec::new(),
    }
}

/// 8.7.3.2's two operators, which are written together and never apart: a
/// `scn` naming a pattern in a colour space that is not `/Pattern` is a name
/// the reader is entitled to read as a number.
fn fill_pattern_colour(out: &mut Vec<u8>, name: &[u8]) {
    out.extend_from_slice(b"/Pattern cs /");
    out.extend_from_slice(name);
    out.extend_from_slice(b" scn\n");
}

/// The same pair for the **stroking** colour, which Table 74 spells in capitals.
fn stroke_with_pattern(out: &mut Vec<u8>, name: &[u8]) {
    out.extend_from_slice(b"/Pattern CS /");
    out.extend_from_slice(name);
    out.extend_from_slice(b" SCN\n");
}

/// A shape filled with a pattern.
fn fill_with_pattern(out: &mut Vec<u8>, name: &[u8], data: &Geometry) {
    fill_pattern_colour(out, name);
    data.emit(out, true);
    out.extend_from_slice(data.fill_operator().as_bytes());
    out.push(b'\n');
}

/// A shape filled in the neutral placeholder grey, and the defect named.
///
/// `rg` and not `g`, to match every other placeholder this module paints: a
/// reader diffing two content streams should not have to know that one grey is
/// three numbers and another is one.
fn grey_fill(
    state: &mut State<'_>,
    out: &mut Vec<u8>,
    data: &Geometry,
    error: Refused,
) -> Result<(), Trouble> {
    state.refused(error)?;
    markup::op(out, &[PLACEHOLDER_GREY; 3], "rg");
    data.emit(out, true);
    out.extend_from_slice(data.fill_operator().as_bytes());
    out.push(b'\n');
    Ok(())
}

/// A `{StaticResource key}` reference, or `None` for an ordinary value.
///
/// 14.2.3's syntax, and the braces are the whole of it: an attribute value
/// that begins with `{` is a markup extension and a colour never is.
fn reference(value: &str) -> Option<&str> {
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?;
    let rest = inner.trim().strip_prefix("StaticResource")?;
    let key = rest.trim();
    (!key.is_empty()).then_some(key)
}

/// Whether a dictionary entry is itself a reference to another entry.
///
/// 14.2.3 lets **any** attribute-settable property carry a static resource
/// reference, and `Color` is the one where the reference names a value this
/// reader would otherwise have read out of the entry itself — so
/// `<SolidColorBrush x:Key="a" Color="{StaticResource b}"/>` is an alias for
/// whatever `b` names. That is what makes the depth cap and the cycle guard
/// reachable at all, and both are reachable from two lines of markup, which is
/// gap 30's own description of the hazard.
///
/// Deliberately **only** `Color`. A reference on a `Transform` or a `Viewbox`
/// is a reference to that property and not to the whole brush, and treating
/// one as an alias would hand back a `MatrixTransform` where a brush was
/// asked for.
fn alias(node: &Node) -> Option<&str> {
    node.attr("Color").and_then(reference)
}

/// 14.3's `Opacity`, which every drawable may carry.
fn opacity_of(node: &Node) -> Result<f64, ()> {
    match node.attr("Opacity") {
        None => Ok(1.0),
        Some(text) => markup::number(text)
            .filter(|value| (0.0..=1.0).contains(value))
            .ok_or(()),
    }
}

fn line_cap(text: &str) -> Option<i32> {
    match text.trim() {
        "Flat" => Some(0),
        "Round" => Some(1),
        "Square" => Some(2),
        // 11.1's `Triangle` has no PDF spelling; the nearest is the projecting
        // square cap, and taking it silently would be a different shape drawn
        // as though it were the one asked for.
        _ => None,
    }
}

fn line_join(text: &str) -> Option<i32> {
    match text.trim() {
        "Miter" => Some(0),
        "Round" => Some(1),
        "Bevel" => Some(2),
        _ => None,
    }
}

fn union(boxes: &[[f64; 4]]) -> Option<[f64; 4]> {
    boxes.iter().copied().reduce(|a, b| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    })
}

/// Whether any two boxes cover the same place.
///
/// Quadratic in the child count, which is bounded by
/// [`super::MAX_XPS_ELEMENTS`] — and *that* is the reason it is written down
/// here: `5adf502`'s finding was that a per-item cap is not a total, and a
/// canvas with a hundred thousand children would be ten billion comparisons
/// under a cap that only counted elements. So the test stops at the first
/// overlap and the answer for a large canvas is decided by the first pair that
/// covers each other, which any real drawing reaches immediately.
fn overlaps(boxes: &[[f64; 4]]) -> bool {
    for (at, one) in boxes.iter().enumerate() {
        for other in &boxes[at + 1..] {
            if one[0] < other[2] && other[0] < one[2] && one[1] < other[3] && other[1] < one[3] {
                return true;
            }
        }
    }
    false
}

fn clipped(bbox: [f64; 4], clip: Option<&Geometry>) -> Option<[f64; 4]> {
    let Some(clip) = clip.and_then(Geometry::bbox) else {
        return Some(bbox);
    };
    let out = [
        bbox[0].max(clip[0]),
        bbox[1].max(clip[1]),
        bbox[2].min(clip[2]),
        bbox[3].min(clip[3]),
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

/// A box through a matrix, as the box of its four transformed corners.
fn through(bbox: [f64; 4], matrix: [f64; 6]) -> Option<[f64; 4]> {
    let corners = [
        markup::apply(matrix, (bbox[0], bbox[1])),
        markup::apply(matrix, (bbox[2], bbox[1])),
        markup::apply(matrix, (bbox[0], bbox[3])),
        markup::apply(matrix, (bbox[2], bbox[3])),
    ];
    if !corners
        .iter()
        .all(|(x, y)| markup::usable(*x) && markup::usable(*y))
    {
        return None;
    }
    let xs = corners.map(|(x, _)| x);
    let ys = corners.map(|(_, y)| y);
    Some([
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ])
}
