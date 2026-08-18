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
    DeviceSpace, DocumentBuilder, ExtGState, FormXObject, Shading, TransparencyGroup,
};
use tinker_pdf_xml::{Event, Limits as XmlLimits, Source};

use super::brush::{self, Brush, BrushError, Paint};
use super::geometry::{self, Geometry, GeometryError};
use super::markup::{self, Budget, Node, Trouble};
use super::XpsElementDefect;
use crate::cbz::PLACEHOLDER_GREY;

/// How deep a chain of `{StaticResource}` aliases may run.
///
/// Declared in [`super::MAX_XPS_RESOURCE_DEPTH`], which carries the ledger.
use super::MAX_XPS_RESOURCE_DEPTH;

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
        limits: &XmlLimits,
        budget: &mut Budget,
    ) -> Result<Drawn, Trouble> {
        let source = Source::new(bytes).map_err(|_| Trouble::Markup)?;
        let mut reader = source.reader(limits);

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
            defects: Vec::new(),
            dictionaries: Vec::new(),
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
        }
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
    defects: Vec<XpsElementDefect>,
    dictionaries: Vec<Dictionary>,
}

impl State<'_> {
    fn warn(&mut self, defect: XpsElementDefect) {
        if !self.defects.contains(&defect) {
            self.defects.push(defect);
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
                                // 12.1's text run, which is gap 30 milestone
                                // 7. Named, because a page whose words are
                                // missing and whose report says nothing is
                                // gap 17's blank page one element down.
                                self.warn(XpsElementDefect::GlyphsNotDrawn);
                                markup::skip_subtree(reader, budget)?;
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
                    self.close(scopes, done);
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
            "OpacityMask" => {
                // 14.3's mask is a brush used as an alpha, and this build
                // applies none. Ignoring it draws a whole shape where a sliver
                // was meant, which is the opacity case exactly, so it refuses
                // its element for the opacity case's reason.
                self.warn(XpsElementDefect::OpacityMaskUnsupported);
                if let Some(scope) = scopes.last_mut() {
                    scope.refused = true;
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
        if node.attr("OpacityMask").is_some() {
            self.warn(XpsElementDefect::OpacityMaskUnsupported);
            scope.refused = true;
        }
        Ok(scope)
    }

    /// Folds a closed canvas into its parent.
    fn close(&mut self, scopes: &mut [Scope], done: Scope) {
        let Some(parent) = scopes.last_mut() else {
            return;
        };
        if done.refused || done.content.is_empty() {
            return;
        }
        let inner_box = union(&done.boxes);
        // 14.3: the canvas's rendering is composited as a whole, which is a
        // transparency group — and observably one only where two of its
        // children cover the same place. Where they do not, one alpha applied
        // to each child paints the identical picture out of no form XObject at
        // all.
        let grouped = done.opacity < 1.0 && overlaps(&done.boxes);
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
        if done.opacity < 1.0 {
            if let Some(name) = self.gstate(done.opacity, done.opacity) {
                parent.content.push(b'/');
                parent.content.extend_from_slice(&name);
                parent.content.extend_from_slice(b" gs\n");
            }
        }
        parent.content.extend_from_slice(&body);
        parent.content.extend_from_slice(b"Q\n");

        if let Some(bbox) = inner_box {
            let bbox = clipped(bbox, done.clip.as_ref());
            if let Some(bbox) = bbox.and_then(|b| through(b, done.transform)) {
                parent.boxes.push(bbox);
            }
        }
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
        if node.attr("OpacityMask").is_some()
            || node.children.iter().any(|c| c.local == "Path.OpacityMask")
        {
            self.warn(XpsElementDefect::OpacityMaskUnsupported);
            return Ok(());
        }
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

        let fill_alpha = opacity * fill.as_ref().map_or(1.0, |brush| brush.alpha);
        let stroke_alpha = opacity * stroke.as_ref().map_or(1.0, |brush| brush.alpha);
        if fill_alpha < 1.0 || stroke_alpha < 1.0 {
            if let Some(name) = self.gstate(fill_alpha, stroke_alpha) {
                out.push(b'/');
                out.extend_from_slice(&name);
                out.extend_from_slice(b" gs\n");
            }
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
            }
        }
        if let Some(brush) = stroke {
            self.stroke(&mut out, node, &data, &brush);
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

    /// Writes the stroke, in the one shape this milestone can: a solid colour
    /// at a stated width.
    ///
    /// A gradient stroke would need a shading **pattern**, which gap 30's
    /// milestone 5 deliberately does not write — so it takes the placeholder
    /// grey rather than the first colour of the gradient, which is gap 07's
    /// defect said the other way round.
    fn stroke(&mut self, out: &mut Vec<u8>, node: &Node, data: &Geometry, brush: &Brush) {
        let width = node
            .attr("StrokeThickness")
            .map_or(Some(1.0), markup::number);
        let Some(width) = width.filter(|w| *w >= 0.0) else {
            self.warn(XpsElementDefect::GeometryUnreadable);
            return;
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
            Paint::Gradient { .. } => {
                self.warn(XpsElementDefect::BrushUnsupported);
                markup::op(out, &[PLACEHOLDER_GREY], "G");
            }
        }
        data.emit(out, false);
        out.extend_from_slice(b"S\n");
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
