//! Drawing a command stream (`docs/design/command-stream.md`, "What a replay
//! asks, and what answers it"): its calls replayed through the engine's own
//! renderer, which asks [`StreamScope`] — the stream's tables, by scope —
//! every question it would ask a page's resources, and is answered with what
//! the page's resources answered, noting what they noted.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use tinker_pdf_content::commands::rendering_intent_number;
use tinker_pdf_content::{Cursor, Device, RenderingIntent, Replayed};
use tinker_pdf_font::Outline;
use tinker_pdf_render::{
    DecodedImage, FontIdentity, GlyphSource, ImageRequest, PatternPaint, Shading, Tile, TileRequest,
};

use super::read::{Layer, Note, Parsed};
use super::tables::{ImageAnswer, ImageKey, Notes};
use crate::resources::{paint_cell, Room};
use crate::stats::{self, Work};

/// One scope's answers, by the questions' keys: each an index into the
/// stream's tables.
#[derive(Default)]
struct ScopeAnswers {
    children: HashMap<Vec<u8>, u64>,
    outlines: HashMap<(u64, u32), usize>,
    identities: HashMap<u64, usize>,
    images: HashMap<(Vec<u8>, u64), usize>,
    /// By the first of the stream's inline images with the asked image's
    /// bytes ([`Replayable::first_inline`]) and the intent.
    inline: HashMap<(u64, u64), usize>,
    shadings: HashMap<Vec<u8>, usize>,
    patterns: HashMap<Vec<u8>, usize>,
    cells: HashMap<Vec<u8>, usize>,
}

/// A stream read and ready to draw: its parse, every scope's answers, each
/// image's decodes by reduction, the first of its inline images with each
/// one's bytes, and each cell's stream ready too.
pub(crate) struct Replayable {
    pub(crate) parsed: Arc<Parsed>,
    scopes: Vec<ScopeAnswers>,
    decodes: HashMap<(usize, u32), usize>,
    /// The ordinal of each first inline image, by [`inline_hash`] of its
    /// bytes: a key holds no copy of an image however many entries name it.
    inline: HashMap<u64, Vec<u64>>,
    cells: Vec<Arc<Replayable>>,
}

/// A hash of an inline image's dictionary and data, the bucket its first is
/// looked up in. Nothing a render draws depends on its value.
fn inline_hash(dict: &[u8], data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    dict.hash(&mut hasher);
    data.hash(&mut hasher);
    hasher.finish()
}

impl Replayable {
    /// `parsed`, its tables keyed by the questions they answer. A key two
    /// entries share answers as the first, which is the one the writer
    /// wrote; the writer writes each key once.
    pub(crate) fn of(parsed: Arc<Parsed>) -> Replayable {
        let tables = &parsed.tables;
        let mut scopes: Vec<ScopeAnswers> = (0..=tables.scopes.len())
            .map(|_| ScopeAnswers::default())
            .collect();
        let count = scopes.len();
        let at = move |scope: u64| usize::try_from(scope).ok().filter(|s| *s < count);
        for (index, entry) in tables.scopes.iter().enumerate() {
            if let (Some(parent), Some(name)) = (entry.parent, &entry.name) {
                if let Some(parent) = at(parent) {
                    scopes[parent]
                        .children
                        .entry(name.clone())
                        .or_insert(index as u64 + 1);
                }
            }
        }
        for (index, entry) in tables.outlines.iter().enumerate() {
            if let Some(scope) = at(entry.scope) {
                scopes[scope]
                    .outlines
                    .entry((entry.font_id, entry.code))
                    .or_insert(index);
            }
        }
        for (index, entry) in tables.identities.iter().enumerate() {
            if let Some(scope) = at(entry.scope) {
                scopes[scope]
                    .identities
                    .entry(entry.font_id)
                    .or_insert(index);
            }
        }
        // Each inline image by the first of the stream's with its bytes, so
        // that every entry naming an image keys it by one number.
        let mut inline: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut firsts: Vec<u64> = Vec::with_capacity(parsed.inline.len());
        for ordinal in 0..parsed.inline.len() {
            let own = ordinal as u64;
            let Some((dict, data)) = parsed.inline_image(ordinal) else {
                firsts.push(own);
                continue;
            };
            let bucket = inline.entry(inline_hash(dict, data)).or_default();
            let first = bucket
                .iter()
                .copied()
                .find(|&o| parsed.inline_image(o as usize) == Some((dict, data)));
            firsts.push(first.unwrap_or_else(|| {
                bucket.push(own);
                own
            }));
        }
        for (index, entry) in tables.images.iter().enumerate() {
            let Some(scope) = at(entry.scope) else {
                continue;
            };
            let intent = rendering_intent_number(entry.intent);
            match &entry.key {
                ImageKey::Named(name) => {
                    scopes[scope]
                        .images
                        .entry((name.clone(), intent))
                        .or_insert(index);
                }
                ImageKey::Inline(ordinal) => {
                    if let Some(&first) = usize::try_from(*ordinal).ok().and_then(|o| firsts.get(o))
                    {
                        scopes[scope].inline.entry((first, intent)).or_insert(index);
                    }
                }
            }
        }
        for (index, entry) in tables.shadings.iter().enumerate() {
            if let Some(scope) = at(entry.scope) {
                scopes[scope]
                    .shadings
                    .entry(entry.name.clone())
                    .or_insert(index);
            }
        }
        for (index, entry) in tables.patterns.iter().enumerate() {
            if let Some(scope) = at(entry.scope) {
                scopes[scope]
                    .patterns
                    .entry(entry.name.clone())
                    .or_insert(index);
            }
        }
        for (index, entry) in tables.cells.iter().enumerate() {
            if let Some(scope) = at(entry.scope) {
                scopes[scope]
                    .cells
                    .entry(entry.name.clone())
                    .or_insert(index);
            }
        }
        let mut decodes = HashMap::new();
        for (index, decode) in tables.decodes.iter().enumerate() {
            if let Ok(image) = usize::try_from(decode.image) {
                decodes.entry((image, decode.reduction)).or_insert(index);
            }
        }
        let cells = parsed
            .cells
            .iter()
            .map(|cell| Arc::new(Replayable::of(Arc::clone(cell))))
            .collect();
        Replayable {
            parsed,
            scopes,
            decodes,
            inline,
            cells,
        }
    }

    /// The ordinal of the first of the stream's inline images whose
    /// dictionary and data are `dict` and `data`.
    fn first_inline(&self, dict: &[u8], data: &[u8]) -> Option<u64> {
        self.inline
            .get(&inline_hash(dict, data))?
            .iter()
            .copied()
            .find(|&o| {
                usize::try_from(o)
                    .ok()
                    .and_then(|o| self.parsed.inline_image(o))
                    == Some((dict, data))
            })
    }
}

/// What a scope has noted while a render drew through it, as a page's
/// resources keep it (`PageResources`): the fonts it could not resolve, the
/// images it decoded with damage, the spaces it read with a repair and the
/// images it drew from a decode the budget reduced, each once and at most 64
/// of each.
#[derive(Debug, Default)]
pub(crate) struct Lists {
    missing: Mutex<Vec<String>>,
    damaged: Mutex<Vec<(String, String)>>,
    repaired: Mutex<Vec<(String, String)>>,
    reduced: Mutex<Vec<(String, u32)>>,
}

/// Adds `entry` to `list`, once, while it holds fewer than 64.
fn once<T: PartialEq>(list: &Mutex<Vec<T>>, entry: T) {
    if let Ok(mut list) = list.lock() {
        if list.len() < 64 && !list.contains(&entry) {
            list.push(entry);
        }
    }
}

impl Lists {
    fn notes(&self, notes: &Notes) {
        for name in &notes.missing {
            once(&self.missing, name.clone());
        }
        for entry in &notes.damaged {
            once(&self.damaged, entry.clone());
        }
        for entry in &notes.repaired {
            once(&self.repaired, entry.clone());
        }
    }

    fn note(&self, note: &Note) {
        match note {
            Note::Missing(name) => once(&self.missing, name.clone()),
            Note::Repaired(name, repair) => once(&self.repaired, (name.clone(), repair.clone())),
        }
    }

    /// What a cell with resources of its own met, said as this scope's, as
    /// `PageResources::absorb` says it.
    fn absorb(&self, cell: &Lists) {
        let missing = cell.missing.lock().map(|l| l.clone()).unwrap_or_default();
        for name in missing {
            once(&self.missing, name);
        }
        let damaged = cell.damaged.lock().map(|l| l.clone()).unwrap_or_default();
        if let Ok(mut mine) = self.damaged.lock() {
            for entry in damaged {
                if !mine.contains(&entry) {
                    mine.push(entry);
                }
            }
        }
        let repaired = cell.repaired.lock().map(|l| l.clone()).unwrap_or_default();
        for entry in repaired {
            once(&self.repaired, entry);
        }
        let reduced = cell.reduced.lock().map(|l| l.clone()).unwrap_or_default();
        for entry in reduced {
            once(&self.reduced, entry);
        }
    }
}

/// One scope of a stream, as a renderer draws through it: the stream, which
/// of its scopes, what this scope has noted, and the tally the render counts
/// into.
pub(crate) struct StreamScope {
    stream: Arc<Replayable>,
    scope: u64,
    lists: Arc<Lists>,
    work: Option<Arc<Work>>,
}

impl StreamScope {
    /// The stream's page scope, with nothing noted, counting into `work`.
    pub(crate) fn root(stream: Arc<Replayable>, work: Option<Arc<Work>>) -> StreamScope {
        StreamScope {
            stream,
            scope: 0,
            lists: Arc::new(Lists::default()),
            work,
        }
    }

    /// Another scope of the same stream, with nothing noted: what a form's
    /// scope and an appearance's are each time a render enters one, as a
    /// page's resources bind a kept scope afresh.
    pub(crate) fn scoped(&self, scope: u64) -> StreamScope {
        StreamScope {
            stream: Arc::clone(&self.stream),
            scope,
            lists: Arc::new(Lists::default()),
            work: self.work.clone(),
        }
    }

    fn answers(&self) -> Option<&ScopeAnswers> {
        self.stream.scopes.get(usize::try_from(self.scope).ok()?)
    }

    /// The decode of image entry `image` a draw that asks as `request` does
    /// is handed, and what it noted.
    ///
    /// The reduction is the frame's for the request under the room the
    /// writing document's budget left the image, as the page's resources
    /// read it; one past what the drawn size asks is noted under `name`, the
    /// image's resource name, by every draw that takes it, as theirs note it
    /// (`RenderWarning::ReducedImage`).
    fn decode(
        &self,
        image: usize,
        request: &ImageRequest,
        name: &[u8],
    ) -> Option<Result<Option<DecodedImage>, String>> {
        let tables = &self.stream.parsed.tables;
        let entry = tables.images.get(image)?;
        let room = entry.room.map(Room);
        let reduction = entry
            .frame
            .map_or(1, |frame| frame.reduction(request, room));
        let decode = tables
            .decodes
            .get(*self.stream.decodes.get(&(image, reduction))?)?;
        self.lists.notes(&decode.answer.notes);
        let own = entry
            .frame
            .map_or(1, |frame| frame.reduction(request, None));
        if room.is_some() && reduction > own && matches!(decode.answer.value, ImageAnswer::Image(_))
        {
            once(
                &self.lists.reduced,
                (String::from_utf8_lossy(name).into_owned(), reduction),
            );
        }
        Some(decode.answer.value.handed())
    }
}

impl GlyphSource for StreamScope {
    fn outline(&self, font_id: u64, code: u32) -> Option<Outline> {
        let index = *self.answers()?.outlines.get(&(font_id, code))?;
        let entry = self.stream.parsed.tables.outlines.get(index)?;
        self.lists.notes(&entry.answer.notes);
        entry.answer.value.clone()
    }

    fn font_identity(&self, font_id: u64) -> Option<FontIdentity> {
        let index = *self.answers()?.identities.get(&font_id)?;
        self.stream.parsed.tables.identities.get(index)?.identity
    }

    /// The decode the page's resources handed a draw asking as `request`
    /// does, under `intent`: its frame decides the reduction, as theirs does.
    /// A name the page drew no image by is no image, as theirs answer.
    fn image(
        &self,
        name: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> Result<Option<DecodedImage>, String> {
        let key = (name.to_vec(), rendering_intent_number(intent));
        self.answers()
            .and_then(|answers| answers.images.get(&key))
            .and_then(|&image| self.decode(image, request, name))
            .unwrap_or_else(|| Err(String::from_utf8_lossy(name).into_owned()))
    }

    fn inline_image(
        &self,
        dict: &[u8],
        data: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> Result<Option<DecodedImage>, String> {
        let intent = rendering_intent_number(intent);
        self.stream
            .first_inline(dict, data)
            .and_then(|first| self.answers()?.inline.get(&(first, intent)).copied())
            .and_then(|image| self.decode(image, request, b""))
            .unwrap_or(Ok(None))
    }

    fn shading(&self, name: &[u8]) -> Result<Option<Shading>, i64> {
        let Some(entry) = self
            .answers()
            .and_then(|answers| answers.shadings.get(name))
            .and_then(|&index| self.stream.parsed.tables.shadings.get(index))
        else {
            return Ok(None);
        };
        self.lists.notes(&entry.answer.notes);
        entry.answer.value.handed()
    }

    fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
        let index = *self.answers()?.patterns.get(name)?;
        let entry = self.stream.parsed.tables.patterns.get(index)?;
        self.lists.notes(&entry.answer.notes);
        entry.answer.value.handed()
    }

    /// The cell replayed at the request's geometry, as `PageResources::tile`
    /// interprets it: in a scope of its own that this one absorbs where the
    /// cell brings resources of its own, and in this one's notes where it
    /// does not.
    fn tile(&self, name: &[u8], request: &TileRequest) -> Option<Tile> {
        let index = *self.answers()?.cells.get(name)?;
        let own = self.stream.parsed.tables.cells.get(index)?.own;
        let stream = Arc::clone(self.stream.cells.get(index)?);
        let lists = if own {
            Arc::new(Lists::default())
        } else {
            Arc::clone(&self.lists)
        };
        let root = StreamScope {
            stream,
            scope: 0,
            lists,
            work: self.work.clone(),
        };
        let (canvas, warnings, counts, opaque) = paint_cell(request, &root, |renderer| {
            replay_layer(&root.stream.parsed.content, renderer, &root);
        });
        // The cell's renderer is one of this render's own, so the work it did
        // is; the calls it drew are a paint of the fill that names the
        // pattern, not the page's.
        stats::add_render(self.work.as_deref(), counts, stats::Draws::Elsewhere);
        if own {
            self.lists.absorb(&root.lists);
        }
        Some(Tile {
            canvas: Arc::new(canvas),
            warnings,
            opaque,
        })
    }

    fn form_scope(&self, name: &[u8]) -> Option<Arc<StreamScope>> {
        let child = *self.answers()?.children.get(name)?;
        Some(Arc::new(self.scoped(child)))
    }
}

impl crate::Drawn for StreamScope {
    fn met_missing_fonts(&self) -> bool {
        self.lists.missing.lock().is_ok_and(|list| !list.is_empty())
    }

    fn damaged_images(&self) -> Vec<(String, String)> {
        self.lists
            .damaged
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default()
    }

    fn repaired_spaces(&self) -> Vec<(String, String)> {
        self.lists
            .repaired
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default()
    }

    fn reduced_images(&self) -> Vec<(String, u32)> {
        self.lists
            .reduced
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default()
    }

    fn work(&self) -> Option<&Arc<Work>> {
        self.work.as_ref()
    }
}

/// `layer`'s calls onto `device`, each note the stream holds noted in
/// `scope` once the replay has reached the call it was met ahead of — after
/// the last for a note at the end — and none once the device has said it is
/// cancelled, as a direct render notes nothing its interpreter did not reach.
pub(crate) fn replay_layer<D: Device + ?Sized>(
    layer: &Layer,
    device: &mut D,
    scope: &StreamScope,
) -> Replayed {
    let mut cursor = Cursor::new();
    let mut done = Replayed::default();
    for (position, note) in &layer.notes {
        while cursor.position() < *position
            && !cursor.is_finished(&layer.events)
            && !cursor.is_cancelled()
        {
            done += cursor.step(&layer.events, device, position - cursor.position());
        }
        if cursor.is_cancelled() {
            break;
        }
        scope.lists.note(note);
    }
    done += cursor.step(&layer.events, device, usize::MAX);
    done
}
