//! Writing a page's command stream (`docs/design/command-stream.md`): its
//! calls, and the answers to every question a render of them asks, in the
//! scope it asks them in.
//!
//! [`Writer`] takes the calls one event at a time, as a retained page's list
//! holds them or as [`Interpreting`] builds them from an interpretation, and
//! for each call that asks the renderer's `GlyphSource` something it asks the
//! page's resources the same question — through resources of its own for
//! that one answer, so what the answer noted is that answer's — and keeps the
//! answer in the stream's tables. A tiling cell is a content stream a replay
//! cannot interpret, so it is interpreted here once, into a stream of its
//! own.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tinker_pdf_content::commands::{
    rendering_intent_number, write_stream, CommandKind, EventWriter, Field, Record,
};
use tinker_pdf_content::{
    interpret, Device, Event, Glyph, GraphicsState, Group, ImageRef, MarkedProps, MarkedScope,
    MaskGroup, Matrix, PathSegment, RenderingIntent, TextRenderMode,
};
use tinker_pdf_cos::pages as cos_pages;
use tinker_pdf_render::{Admission, GlyphSource, ImageRequest, MAX_PATTERN_DEPTH};

use super::tables::{
    Answered, CellEntry, DecodeEntry, Definitions, IdentityEntry, ImageAnswer, ImageEntry,
    ImageKey, Notes, OutlineEntry, PageRecord, PatternAnswer, PatternEntry, ScopeEntry,
    ShadingAnswer, ShadingEntry, Tables,
};
use crate::annots::{self, AppearanceDevice, Kept, Kinds};
use crate::resources::{Frame, PageResources};
use crate::{DisplayList, Usage};

/// A scope the writer resolves names in: one it borrows — the page's, a
/// cell's — or one it holds — a form's, an appearance's.
#[derive(Clone)]
enum Held<'a> {
    Borrowed(&'a PageResources),
    Owned(Arc<PageResources>),
}

impl core::ops::Deref for Held<'_> {
    type Target = PageResources;

    fn deref(&self) -> &PageResources {
        match self {
            Held::Borrowed(resources) => resources,
            Held::Owned(resources) => resources,
        }
    }
}

/// The scope in force, by its id in the stream and the resources it is.
#[derive(Clone)]
struct Scoped<'a> {
    id: u64,
    resources: Held<'a>,
}

/// Every reduction a draw can ask an image of `frame` for, with a request
/// that asks for it: the whole image alone where nothing reduces it.
fn reductions(frame: Option<Frame>) -> Vec<(u32, ImageRequest)> {
    frame.map_or_else(|| vec![(1, ImageRequest::FULL)], Frame::reductions)
}

/// Writes one stream: a page's, or a tiling cell's.
pub(crate) struct Writer<'a> {
    events: EventWriter,
    usage: Option<Usage>,
    /// How many tiling patterns stand above this stream: 0 for a page, a
    /// cell's renderer's `pattern_depth` for a cell.
    depth: u32,
    stack: Vec<Scoped<'a>>,
    /// How deep the stack stood when the list being written began, which an
    /// `EndForm` never pops below.
    floor: usize,
    tables: Tables,
    scope_ids: HashMap<(u64, Vec<u8>), u64>,
    outlines: HashSet<(u64, u64, u32)>,
    identities: HashSet<(u64, u64)>,
    images: HashSet<(u64, Vec<u8>, u64)>,
    inline: HashSet<(u64, Vec<u8>, Vec<u8>, u64)>,
    /// The inline images met so far in this stream's commands, the page's
    /// content and then each appearance's.
    inline_count: u64,
    shadings: HashSet<(u64, Vec<u8>)>,
    patterns: HashSet<(u64, Vec<u8>)>,
}

impl<'a> Writer<'a> {
    fn new(root: Held<'a>, usage: Option<Usage>, depth: u32) -> Writer<'a> {
        Writer {
            events: EventWriter::new(),
            usage,
            depth,
            stack: vec![Scoped {
                id: 0,
                resources: root,
            }],
            floor: 1,
            tables: Tables::default(),
            scope_ids: HashMap::new(),
            outlines: HashSet::new(),
            identities: HashSet::new(),
            images: HashSet::new(),
            inline: HashSet::new(),
            inline_count: 0,
            shadings: HashSet::new(),
            patterns: HashSet::new(),
        }
    }

    fn top(&self) -> &Scoped<'a> {
        // The stack holds the list's own scope from the start and is never
        // popped below it, so it is never empty.
        &self.stack[self.stack.len() - 1]
    }

    /// Resources of the scope in force for one answer: its caches, its
    /// operation's optional-content states, and nothing noted yet.
    fn asker(&self) -> PageResources {
        self.top()
            .resources
            .for_one_render_counted(None, self.usage)
    }

    /// Writes one call, after the answers to the questions a render of it
    /// asks.
    pub(crate) fn event(&mut self, event: &Event) {
        match event {
            Event::BeginForm { name, .. } => self.enter_form(name),
            Event::EndForm { .. } => {
                if self.stack.len() > self.floor {
                    self.stack.pop();
                }
            }
            Event::ShowGlyph { glyph, state } => {
                self.glyph(glyph, state.as_deref());
                self.patterns(state.as_deref());
            }
            Event::DrawImage { image, state } => {
                self.image(image, state.as_deref());
                self.patterns(state.as_deref());
            }
            Event::DrawShading { name, .. } => self.shading(name),
            Event::FillPath { state, .. }
            | Event::StrokePath { state, .. }
            | Event::ClipPath { state, .. } => self.patterns(state.as_deref()),
            _ => {}
        }
        self.events.event(event);
    }

    /// Begins an annotation's appearance, a widget's where `widget` is true,
    /// drawn in `scope` until [`Writer::end_annotation`].
    fn begin_annotation(&mut self, scope: Held<'a>, widget: bool) {
        self.tables.scopes.push(ScopeEntry {
            parent: None,
            name: None,
        });
        let id = self.tables.scopes.len() as u64;
        self.events.body(
            Record::new(CommandKind::Annotation)
                .with(Field::Unsigned(id))
                .with(Field::Unsigned(u64::from(widget))),
        );
        self.stack.truncate(1);
        self.stack.push(Scoped {
            id,
            resources: scope,
        });
        self.floor = self.stack.len();
    }

    /// Ends the appearance [`Writer::begin_annotation`] began.
    fn end_annotation(&mut self) {
        self.stack.truncate(1);
        self.floor = 1;
    }

    /// A note the interpretation made, where the replay reaches it.
    fn note_missing(&mut self, name: &str) {
        self.events
            .body(Record::new(CommandKind::MissingFont).with(Field::bytes(name.as_bytes())));
    }

    /// A colour space the interpretation read with a repair, likewise.
    fn note_repaired(&mut self, name: &str, repair: &str) {
        self.events.body(
            Record::new(CommandKind::RepairedSpace)
                .with(Field::bytes(name.as_bytes()))
                .with(Field::bytes(repair.as_bytes())),
        );
    }

    /// `BeginForm`: the renderer asks the scope in force for the form's own
    /// resources and resolves in them until `EndForm`, or goes on in the
    /// scope it is in where the form brings none.
    fn enter_form(&mut self, name: &[u8]) {
        let parent = self.top().id;
        let child = self.top().resources.form_scope(name);
        let scoped = match child {
            Some(child) => {
                let key = (parent, name.to_vec());
                let id = match self.scope_ids.get(&key) {
                    Some(id) => *id,
                    None => {
                        self.tables.scopes.push(ScopeEntry {
                            parent: Some(parent),
                            name: Some(name.to_vec()),
                        });
                        let id = self.tables.scopes.len() as u64;
                        self.scope_ids.insert(key, id);
                        id
                    }
                };
                Scoped {
                    id,
                    resources: Held::Owned(child),
                }
            }
            None => self.top().clone(),
        };
        self.stack.push(scoped);
    }

    /// A glyph shown: its outline, which a render asks of every glyph painted
    /// or clipped by, and its font's identity for the glyph-mask cache.
    fn glyph(&mut self, glyph: &Glyph, state: Option<&GraphicsState>) {
        let mode = state.map_or(TextRenderMode::Fill, |s| s.text.render_mode);
        if mode == TextRenderMode::Invisible {
            return;
        }
        let scope = self.top().id;
        if self.outlines.insert((scope, glyph.font_id, glyph.code)) {
            let asker = self.asker();
            let outline = asker.outline(glyph.font_id, glyph.code);
            self.tables.outlines.push(OutlineEntry {
                scope,
                font_id: glyph.font_id,
                code: glyph.code,
                answer: Answered {
                    value: outline,
                    notes: Notes::of(&asker),
                },
            });
        }
        if self.identities.insert((scope, glyph.font_id)) {
            let identity = self.top().resources.font_identity(glyph.font_id);
            self.tables.identities.push(IdentityEntry {
                scope,
                font_id: glyph.font_id,
                identity,
            });
        }
    }

    /// An image drawn: its decode at every reduction a draw can ask for, in
    /// the intent in force at the draw.
    fn image(&mut self, image: &ImageRef, state: Option<&GraphicsState>) {
        let intent = state.map_or(RenderingIntent::RelativeColorimetric, |s| {
            s.stored_parameters().rendering_intent
        });
        let number = rendering_intent_number(intent);
        let scope = self.top().id;
        if image.inline {
            let ordinal = self.inline_count;
            self.inline_count += 1;
            let key = (
                scope,
                image.inline_dict.clone(),
                image.inline_data.clone(),
                number,
            );
            if !self.inline.insert(key) {
                return;
            }
            let frame = self
                .asker()
                .inline_frame(&image.inline_dict, &image.inline_data, intent);
            let index = self.tables.images.len() as u64;
            // An inline image has no object the document keeps its decodes
            // under, so no budget reduces it.
            self.tables.images.push(ImageEntry {
                scope,
                key: ImageKey::Inline(ordinal),
                intent,
                frame,
                room: None,
            });
            for (reduction, request) in reductions(frame) {
                let asker = self.asker();
                let answer =
                    asker.inline_image(&image.inline_dict, &image.inline_data, &request, intent);
                self.tables.decodes.push(DecodeEntry {
                    image: index,
                    reduction,
                    answer: Answered {
                        value: ImageAnswer::of(answer),
                        notes: Notes::of(&asker),
                    },
                });
            }
            return;
        }
        if !self.images.insert((scope, image.name.clone(), number)) {
            return;
        }
        // The whole decode first: it fills the scope's entry for the name,
        // whose frame says what every other draw asks.
        let asker = self.asker();
        let whole = asker.image(&image.name, &ImageRequest::FULL, intent);
        let whole = Answered {
            value: ImageAnswer::of(whole),
            notes: Notes::of(&asker),
        };
        let frame = asker.image_frame(&image.name, intent);
        let index = self.tables.images.len() as u64;
        self.tables.images.push(ImageEntry {
            scope,
            key: ImageKey::Named(image.name.clone()),
            intent,
            frame,
            room: asker.image_room(&image.name).map(|room| room.0),
        });
        self.tables.decodes.push(DecodeEntry {
            image: index,
            reduction: 1,
            answer: whole,
        });
        for (reduction, request) in reductions(frame).into_iter().skip(1) {
            let asker = self.asker();
            let answer = asker.image(&image.name, &request, intent);
            self.tables.decodes.push(DecodeEntry {
                image: index,
                reduction,
                answer: Answered {
                    value: ImageAnswer::of(answer),
                    notes: Notes::of(&asker),
                },
            });
        }
    }

    /// `sh`: the shading the name selects in the scope in force.
    fn shading(&mut self, name: &[u8]) {
        let scope = self.top().id;
        if !self.shadings.insert((scope, name.to_vec())) {
            return;
        }
        let asker = self.asker();
        let value = match asker.shading(name) {
            Ok(None) => ShadingAnswer::None,
            Ok(Some(shading)) => ShadingAnswer::Shading(Arc::new(shading)),
            Err(kind) => ShadingAnswer::Unsupported(kind),
        };
        self.tables.shadings.push(ShadingEntry {
            scope,
            name: name.to_vec(),
            answer: Answered {
                value,
                notes: Notes::of(&asker),
            },
        });
    }

    /// The patterns a paint in `state` can fill or stroke with.
    fn patterns(&mut self, state: Option<&GraphicsState>) {
        let Some(state) = state else {
            return;
        };
        for name in [&state.fill_pattern, &state.stroke_pattern]
            .into_iter()
            .flatten()
        {
            self.pattern(name);
        }
    }

    /// The pattern `name` selects in the scope in force, and a tiling
    /// pattern's cell where a renderer this deep can draw one.
    fn pattern(&mut self, name: &[u8]) {
        let scope = self.top().id;
        if !self.patterns.insert((scope, name.to_vec())) {
            return;
        }
        let asker = self.asker();
        let value = match asker.pattern(name) {
            None => PatternAnswer::None,
            Some(tinker_pdf_render::PatternPaint::Unsupported) => PatternAnswer::Unsupported,
            Some(tinker_pdf_render::PatternPaint::Shading(shading, matrix)) => {
                PatternAnswer::Shading(Arc::new(*shading), matrix)
            }
            Some(tinker_pdf_render::PatternPaint::Tiling(tiling)) => PatternAnswer::Tiling(*tiling),
        };
        let tiling = matches!(value, PatternAnswer::Tiling(_));
        self.tables.patterns.push(PatternEntry {
            scope,
            name: name.to_vec(),
            answer: Answered {
                value,
                notes: Notes::of(&asker),
            },
        });
        // A renderer `MAX_PATTERN_DEPTH` patterns deep declines a pattern
        // before it asks for a cell.
        if tiling && self.depth < MAX_PATTERN_DEPTH {
            self.cell(name, scope);
        }
    }

    /// A tiling cell, interpreted once into a stream of its own in the
    /// resources it draws in: its own, or the asking scope's.
    fn cell(&mut self, name: &[u8], scope: u64) {
        let asker = self.asker();
        let Some((content, own)) = asker.cell(name) else {
            return;
        };
        let depth = self.depth + 1;
        let stream = match &own {
            Some(own) => write_cell(&content, own, self.usage, depth),
            None => write_cell(&content, &asker, self.usage, depth),
        };
        self.tables.cells.push(CellEntry {
            scope,
            name: name.to_vec(),
            own: own.is_some(),
            stream: Arc::from(stream),
        });
    }

    /// The stream's bytes, and the tables they hold: `page` first where the
    /// stream is a page's, then the states' definitions, then the colour
    /// spaces, profiles and shadings the tables refer to, then the tables,
    /// then every call — so every definition precedes what refers to it.
    pub(crate) fn finish(self, page: Option<PageRecord>) -> (Vec<u8>, Tables) {
        let Writer {
            mut events, tables, ..
        } = self;
        let mut definitions = Definitions::default();
        let table_records = tables.records(&mut definitions, &mut events);
        let (head, body) = events.finish();
        let mut records = Vec::with_capacity(head.len() + table_records.len() + body.len() + 1);
        if let Some(page) = page {
            records.push(page.record());
        }
        records.extend(head);
        records.extend(definitions.into_records());
        records.extend(table_records);
        records.extend(body);
        (write_stream(&records), tables)
    }
}

/// A tiling cell's stream: `content` interpreted in `resources` as
/// `PageResources::tile` interprets it, at `depth` patterns deep.
fn write_cell(
    content: &[u8],
    resources: &PageResources,
    usage: Option<Usage>,
    depth: u32,
) -> Vec<u8> {
    let mut writer = Writer::new(Held::Borrowed(resources), usage, depth);
    {
        let mut device = Interpreting::new(&mut writer, resources);
        interpret(content, Matrix::IDENTITY, &mut device, resources);
        device.note();
    }
    writer.finish(None).0
}

/// The page `list` is of, as a render reads it.
fn page_record(list: &DisplayList) -> PageRecord {
    let page = list.page();
    PageRecord {
        crop: page.crop_box(),
        rotation: page.rotation(),
        group: list.resources().page_group_space(&page.inner),
        usage: list.usage(),
        index: page.index(),
    }
}

/// The command stream of `list`'s page, and the tables it holds: a retained
/// list's calls as it holds them, what its interpretation met after the
/// content as its renders note it, and each appearance after; a list that
/// is not retained interprets the page into the writer as a render
/// interprets it.
pub(crate) fn write_list(list: &DisplayList) -> (Vec<u8>, Tables) {
    let page = page_record(list);
    if list.is_retained() {
        let mut writer = Writer::new(Held::Borrowed(list.resources()), list.usage(), 0);
        for event in list.content() {
            writer.event(event);
        }
        for name in list.interpreted_missing() {
            writer.note_missing(name);
        }
        for (name, repair) in list.interpreted_repairs() {
            writer.note_repaired(name, repair);
        }
        // A stream is the drawing of its page at magnification 1: an
        // appearance `/NoZoom` or `/NoRotate` places at render time is
        // interpreted into the writer placed so, as a render at 1 places it.
        let shown = list.page();
        let placed = PageResources::new_in(
            &shown.doc,
            &shown.inner,
            shown.fonts.as_ref(),
            shown.operation(None, list.usage(), list.layers(), list.magnification()),
        );
        for annotation in list.annotation_layers() {
            match &annotation.kept {
                Kept::Calls { scope, events } => {
                    writer.begin_annotation(Held::Owned(Arc::clone(scope)), annotation.widget);
                    for event in events {
                        writer.event(event);
                    }
                    writer.end_annotation();
                }
                Kept::Placed(appearance) => {
                    let mut device = Interpreting::new(&mut writer, &placed);
                    device.annotation(annotation.widget);
                    annots::draw_prepared(
                        &shown.doc,
                        appearance,
                        shown.fonts.as_ref(),
                        &mut device,
                        placed.operation(),
                        list.placement(1.0),
                    );
                    device.close();
                }
            }
        }
        return writer.finish(Some(page));
    }
    let shown = list.page();
    let content = cos_pages::content_bytes(&shown.doc, &shown.inner);
    let resources = PageResources::new_in(
        &shown.doc,
        &shown.inner,
        shown.fonts.as_ref(),
        shown.operation(None, list.usage(), list.layers(), list.magnification()),
    );
    let mut writer = Writer::new(Held::Borrowed(&resources), list.usage(), 0);
    {
        let mut device = Interpreting::new(&mut writer, &resources);
        interpret(&content, Matrix::IDENTITY, &mut device, &resources);
        device.note();
        annots::draw(
            &shown.doc,
            &shown.inner,
            shown.fonts.as_ref(),
            &mut device,
            resources.operation(),
            list.placement(1.0),
            Kinds::ALL,
        );
        device.close();
    }
    writer.finish(Some(page))
}

/// The device an interpretation is written through: the calls it is handed
/// made events, answered as the renderer answers them — through its own
/// [`Admission`], as `DisplayRecorder` answers — and what the interpretation
/// notes in `watched`, the resources it interprets in, written where the
/// replay reaches it: before the call it was met ahead of.
struct Interpreting<'w, 'a> {
    writer: &'w mut Writer<'a>,
    admission: Admission,
    watched: &'a PageResources,
    missing: usize,
    repaired: usize,
    /// The annotation [`AppearanceDevice::annotation`] announced, a widget's
    /// or not, whose layer its first call or its scope opens.
    pending: Option<bool>,
    /// Whether an annotation's layer is open.
    open: bool,
}

impl<'w, 'a> Interpreting<'w, 'a> {
    fn new(writer: &'w mut Writer<'a>, watched: &'a PageResources) -> Interpreting<'w, 'a> {
        Interpreting {
            writer,
            admission: Admission::new(),
            watched,
            missing: watched.missing_count(),
            repaired: watched.tolerated().repaired,
            pending: None,
            open: false,
        }
    }

    /// Ends the annotation's layer that is open, if one is.
    fn close(&mut self) {
        if self.open {
            self.writer.end_annotation();
            self.open = false;
        }
        self.pending = None;
    }

    /// Writes what the interpretation has noted since the last call.
    fn note(&mut self) {
        if self.watched.missing_count() > self.missing {
            let names = self.watched.missing_fonts();
            for name in names.iter().skip(self.missing) {
                self.writer.note_missing(name);
            }
            self.missing = names.len();
        }
        if self.watched.tolerated().repaired > self.repaired {
            let entries = self.watched.repaired_spaces();
            for (name, repair) in entries.iter().skip(self.repaired) {
                self.writer.note_repaired(name, repair);
            }
            self.repaired = entries.len();
        }
    }

    fn deliver(&mut self, event: Event) {
        self.note();
        // An annotation its `/OC` hides is told as content in that layer is,
        // in no scope of its own: its layer opens at its first call, in the
        // scope the interpretation watches, as a retained list keeps it in an
        // empty one.
        if let Some(widget) = self.pending.take() {
            self.writer
                .begin_annotation(Held::Borrowed(self.watched), widget);
            self.open = true;
        }
        self.writer.event(&event);
    }
}

fn snapshot(state: &GraphicsState) -> Option<Box<GraphicsState>> {
    Some(Box::new(state.clone()))
}

impl Device for Interpreting<'_, '_> {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.deliver(Event::ShowGlyph {
            glyph: glyph.clone(),
            state: snapshot(state),
        });
    }

    fn begin_text(&mut self) {
        self.deliver(Event::BeginText);
    }

    fn end_text(&mut self) {
        self.deliver(Event::EndText);
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.deliver(Event::FillPath {
            path: path.to_vec(),
            even_odd,
            state: snapshot(state),
        });
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        self.deliver(Event::StrokePath {
            path: path.to_vec(),
            state: snapshot(state),
        });
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.deliver(Event::ClipPath {
            path: path.to_vec(),
            even_odd,
            state: snapshot(state),
        });
    }

    fn save_state(&mut self) {
        self.deliver(Event::SaveState);
    }

    fn restore_state(&mut self) {
        self.deliver(Event::RestoreState);
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        self.deliver(Event::DrawImage {
            image: image.clone(),
            state: snapshot(state),
        });
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        self.deliver(Event::DrawShading {
            name: name.to_vec(),
            state: snapshot(state),
        });
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        self.admission.begin_marked_content(visible);
        self.deliver(Event::BeginMarkedContent(MarkedScope {
            tag: tag.to_vec(),
            visible,
            hidden_layer: hidden_layer.map(str::to_string),
            props: props.cloned(),
        }));
    }

    fn end_marked_content(&mut self) {
        self.admission.end_marked_content();
        self.deliver(Event::EndMarkedContent);
    }

    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        // The renderer enters every form it is not cancelled out of.
        self.deliver(Event::BeginForm {
            id,
            name: name.to_vec(),
            entered: true,
        });
        true
    }

    fn end_form(&mut self, id: u64) {
        self.deliver(Event::EndForm { id });
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        let entered = self.admission.begin_group();
        self.deliver(Event::BeginGroup {
            group,
            entered,
            state: snapshot(state),
        });
        entered
    }

    fn end_group(&mut self) {
        self.admission.end_group();
        self.deliver(Event::EndGroup);
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        let entered = self.admission.begin_soft_mask();
        self.deliver(Event::BeginSoftMask {
            mask: Box::new(mask.clone()),
            bbox: bbox.to_vec(),
            entered,
            state: snapshot(state),
        });
        entered
    }

    fn end_soft_mask(&mut self) {
        self.admission.end_soft_mask();
        self.deliver(Event::EndSoftMask);
    }

    fn clear_soft_mask(&mut self) {
        self.deliver(Event::ClearSoftMask);
    }

    fn group_budget_spent(&mut self) {
        self.admission.note_budget_spent();
        self.deliver(Event::GroupBudgetSpent);
    }

    fn recursion_refused(&mut self) {
        self.deliver(Event::RecursionRefused);
    }

    fn nested_budget_spent(&mut self) {
        self.deliver(Event::NestedBudgetSpent);
    }

    /// Never: a stream holds the whole page, and cancellation is a replay's.
    fn is_cancelled(&self) -> bool {
        false
    }

    /// The renderer's admission's answer, as `DisplayRecorder` gives it, so a
    /// kept drawing of a form is replayed into the writer where it would be
    /// replayed into the recorder.
    fn answers_alike(&self, events: &[Event]) -> bool {
        self.admission.answers_alike(events)
    }
}

impl<'w, 'a> AppearanceDevice for Interpreting<'w, 'a> {
    fn push_scope(&mut self, scope: Arc<PageResources>) {
        self.note();
        let widget = self.pending.take().unwrap_or(false);
        if self.open {
            self.writer.end_annotation();
        }
        self.writer.begin_annotation(Held::Owned(scope), widget);
        self.open = true;
    }

    fn pop_scope(&mut self) {
        self.writer.end_annotation();
        self.open = false;
    }

    fn annotation(&mut self, widget: bool) {
        self.note();
        self.close();
        self.pending = Some(widget);
    }
}
