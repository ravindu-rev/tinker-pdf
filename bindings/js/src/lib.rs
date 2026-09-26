//! The JavaScript and WebAssembly binding.
//!
//! wasm-bindgen directly over the facade, not through the C ABI — a C layer
//! would add a second error translation for nothing, since wasm-bindgen speaks
//! Rust types.
//!
//! **Copy is the default, views are opt-in.** `data()` returns a copied
//! `Uint8Array`; `view()` returns one aliasing wasm memory, which **any**
//! subsequent allocation may invalidate by growing that memory. The dangerous
//! call gets the warning in its name and its documentation; the safe one gets
//! the short name.
//!
//! Scope and packaging: `docs/features/bindings.md`.

#![allow(clippy::new_without_default)]

use core::ops::Range;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use wasm_bindgen::prelude::*;

/// An open PDF document.
#[wasm_bindgen]
pub struct PdfDocument {
    inner: tinker_pdf::Document,
}

/// A rendered page.
#[wasm_bindgen]
pub struct PdfBitmap {
    inner: tinker_pdf::Bitmap,
}

/// The engine's version.
#[wasm_bindgen(js_name = version)]
#[must_use]
pub fn version() -> String {
    tinker_pdf::VERSION.to_string()
}

/// Bytes the host has fetched, for a document opened by ranges.
///
/// **Transport, not engine.** The engine defines `ByteSource` and never
/// performs any I/O; this is the host side of that seam, exactly as a
/// `FontProvider` is the host side of the font seam. It holds what
/// JavaScript has handed it and answers from that; a range it has not been
/// given is a refusal, and the host is told which one so it can go and fetch
/// it.
///
/// # The loop
///
/// ```js
/// const source = new PdfSource(file.size);
/// let doc = null;
/// while (doc === null) {
///   try {
///     doc = PdfDocument.openStreaming(source);
///   } catch (e) {
///     for (const [start, end] of source.takeNeeded()) {
///       source.feed(start, await fetchRange(start, end));
///     }
///   }
/// }
/// ```
///
/// It terminates because every refusal names a range, the host feeds exactly
/// that, and the engine's caches keep what they have already parsed -- so
/// each turn strictly increases what is readable and no work is repeated.
#[wasm_bindgen]
pub struct PdfSource {
    inner: Arc<HostBytes>,
}

/// What the host has fed, and what it was asked for and could not answer.
struct HostBytes {
    len: u64,
    fed: Mutex<BTreeMap<u64, Vec<u8>>>,
    needed: Mutex<Vec<(u64, u64)>>,
}

impl tinker_pdf::ByteSource for HostBytes {
    fn len(&self) -> u64 {
        self.len
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, tinker_pdf::SourceMiss> {
        let end = range.end.min(self.len);
        if range.start >= end {
            return Ok(Arc::from(&[][..]));
        }
        let fed = self.fed.lock().unwrap_or_else(|e| e.into_inner());
        // The block that starts at or before the wanted byte, which is the
        // only one that can answer it.
        if let Some((start, bytes)) = fed.range(..=range.start).next_back() {
            let offset = (range.start - start) as usize;
            if offset < bytes.len() {
                let take = bytes.len() - offset;
                let take = take.min((end - range.start) as usize);
                return Ok(Arc::from(&bytes[offset..offset + take]));
            }
        }
        drop(fed);
        self.needed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((range.start, end));
        Err(tinker_pdf::SourceMiss::at(range.start..end))
    }
}

#[wasm_bindgen]
impl PdfSource {
    /// A source for a document of `length` bytes, with nothing fetched yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(length: f64) -> PdfSource {
        PdfSource {
            inner: Arc::new(HostBytes {
                len: length.max(0.0) as u64,
                fed: Mutex::new(BTreeMap::new()),
                needed: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Supplies `bytes` as the document's content starting at `offset`.
    pub fn feed(&self, offset: f64, bytes: &[u8]) {
        self.inner
            .fed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(offset.max(0.0) as u64, bytes.to_vec());
    }

    /// The ranges refused since this was last called, as `[start, end, ...]`,
    /// and forgets them.
    ///
    /// A flat array of pairs rather than objects: it crosses the boundary as
    /// one `Float64Array` and needs no allocation per range on either side.
    #[wasm_bindgen(js_name = takeNeeded)]
    #[must_use]
    pub fn take_needed(&self) -> Vec<f64> {
        let mut needed = self.inner.needed.lock().unwrap_or_else(|e| e.into_inner());
        let taken = std::mem::take(&mut *needed);
        let mut out = Vec::with_capacity(taken.len() * 2);
        for (start, end) in taken {
            out.push(start as f64);
            out.push(end as f64);
        }
        out
    }

    /// How many bytes the host has fed so far.
    #[wasm_bindgen(getter, js_name = bytesFed)]
    #[must_use]
    pub fn bytes_fed(&self) -> f64 {
        self.inner
            .fed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|b| b.len() as f64)
            .sum()
    }
}

#[wasm_bindgen]
impl PdfDocument {
    /// Opens a document from bytes.
    ///
    /// The bytes are copied into the wasm heap, so the caller's buffer may be
    /// released immediately.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<PdfDocument, JsError> {
        tinker_pdf::Document::open(bytes.to_vec())
            .map(|inner| PdfDocument { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Opens a document whose bytes the host supplies by range.
    ///
    /// Throws when a range the open path needs has not been fed. That is not
    /// a failure: call [`PdfSource::take_needed`], feed what it names, and
    /// call this again. See [`PdfSource`] for the loop and why it terminates.
    ///
    /// The document that comes out is the same document `new PdfDocument(all
    /// the bytes)` would have produced -- same pages, same warnings, same
    /// pixels. Where the bytes came from is not an input to what they mean.
    #[wasm_bindgen(js_name = openStreaming)]
    pub fn open_streaming(source: &PdfSource) -> Result<PdfDocument, JsError> {
        let handle: Arc<dyn tinker_pdf::ByteSource> = Arc::clone(&source.inner) as _;
        tinker_pdf::Document::open_streaming(handle)
            .map(|inner| PdfDocument { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// The number of pages.
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> u32 {
        self.inner.page_count()
    }

    /// Whether the document is encrypted.
    #[wasm_bindgen(getter, js_name = isEncrypted)]
    pub fn is_encrypted(&self) -> bool {
        self.inner.is_encrypted()
    }

    /// Tries a password, returning "none", "user" or "owner".
    #[wasm_bindgen]
    pub fn authenticate(&mut self, password: &str) -> Result<String, JsError> {
        match self.inner.authenticate(password) {
            Ok(tinker_pdf::AuthLevel::Owner) => Ok("owner".to_string()),
            Ok(tinker_pdf::AuthLevel::User) => Ok("user".to_string()),
            Ok(tinker_pdf::AuthLevel::None) => Ok("none".to_string()),
            Err(e) => Err(JsError::new(&format!("{e:?}"))),
        }
    }

    /// Whether the document permits printing. PDF permissions are advisory.
    #[wasm_bindgen(js_name = mayPrint)]
    pub fn may_print(&self) -> bool {
        self.inner.permissions().print()
    }

    /// A page's width in points.
    #[wasm_bindgen(js_name = pageWidth)]
    pub fn page_width(&self, index: u32) -> f64 {
        self.inner.page(index).map_or(0.0, |p| p.size().0)
    }

    /// A page's height in points.
    #[wasm_bindgen(js_name = pageHeight)]
    pub fn page_height(&self, index: u32) -> f64 {
        self.inner.page(index).map_or(0.0, |p| p.size().1)
    }

    /// A page's text.
    #[wasm_bindgen(js_name = pageText)]
    pub fn page_text(&self, index: u32) -> String {
        self.inner
            .page(index)
            .map(|p| p.text().plain_text())
            .unwrap_or_default()
    }

    /// Supplies a font for documents that embed none.
    ///
    /// Without one such a document extracts its text perfectly and draws none
    /// of it: the standard-14 metrics are built in, the outlines are not. The
    /// engine bundles no faces, and a browser has no font directory to read,
    /// so the page supplies the bytes.
    #[wasm_bindgen(js_name = setFonts)]
    pub fn set_fonts(
        &mut self,
        regular: Vec<u8>,
        bold: Option<Vec<u8>>,
        italic: Option<Vec<u8>>,
        bold_italic: Option<Vec<u8>>,
    ) {
        let mut provider = tinker_pdf::SimpleFontProvider::new(regular);
        if let Some(bytes) = bold {
            provider = provider.with_bold(bytes);
        }
        if let Some(bytes) = italic {
            provider = provider.with_italic(bytes);
        }
        if let Some(bytes) = bold_italic {
            provider = provider.with_bold_italic(bytes);
        }
        self.inner = self.inner.clone().with_fonts(std::sync::Arc::new(provider));
    }

    /// Renders a page at a scale, where 1.0 is 72 dpi.
    #[wasm_bindgen(js_name = renderPage)]
    pub fn render_page(&self, index: u32, scale: f64) -> Result<PdfBitmap, JsError> {
        let page = self
            .inner
            .page(index)
            .ok_or_else(|| JsError::new("no such page"))?;
        Ok(PdfBitmap {
            inner: page.render(&tinker_pdf::RenderOptions {
                scale,
                format: tinker_pdf::PixelFormat::Rgba8,
                ..tinker_pdf::RenderOptions::default()
            }),
        })
    }

    /// The strict structural validator's findings, as rule names (ruling 13).
    ///
    /// An empty array is a clean document. This is the check that keeps four
    /// byte-identical outputs from being identically wrong: the write-parity
    /// suite compares four surfaces' bytes to each other, and agreement alone
    /// would be satisfied by four copies of a broken file.
    #[wasm_bindgen]
    pub fn validate(&self) -> Vec<String> {
        self.inner
            .validate()
            .into_iter()
            .map(|defect| defect.kind.as_str().to_string())
            .collect()
    }

    /// An editor over this document.
    ///
    /// Independent of the document it came from: the editor holds its own
    /// reference to the shared object store, so this one may be freed first
    /// and the editor still saves correctly.
    #[wasm_bindgen]
    pub fn editor(&self) -> PdfEditor {
        PdfEditor {
            inner: self.inner.editor(),
        }
    }
}

// ---- writing (gap 32 milestone 4) ------------------------------------------
//
// Facade-direct, like the read side above. `DocumentEditor::transaction` and
// `DocumentBuilder::add_page` take closures and a closure does not cross a
// language boundary, so ruling 11 had the facade grow the closure-free
// equivalents first; these are projections of those.
//
// **There is deliberately no `editor.transaction(callback)` here**, and the
// reason is mechanical rather than a matter of taste. An exported method
// borrows its `this` for the whole call, so JavaScript running inside one that
// touched the same editor would hit wasm-bindgen's "recursive use of an object
// detected which would lead to unsafe aliasing in Rust" -- a panic, from the
// one shape a caller would most want. What the design asks for is
// checkpoint, host-language control flow, restore; in JavaScript that *is*
// three lines the caller writes, and `tests/node_smoke.mjs` writes them:
//
// ```js
// const mark = editor.checkpoint();
// try { editor.fillField("name", "Ada"); }
// catch (e) { editor.restore(mark); throw e; }
// finally { mark.free(); }
// ```
//
// Wrapping those three lines in a shipped JavaScript helper was the
// alternative and was rejected: it would be the first logic any binding here
// carries, and ruling 11's whole point is that there is none to diverge with.

/// A widget a fill wrote a value for and could not draw.
///
/// **The fourth outcome.** `fillField` throws when nothing was written,
/// returns an empty array when the value was written and every widget drawn,
/// and returns a non-empty one when the value was written and these widgets
/// were left showing whatever they showed before, because 12.5.2's required
/// `/Rect` is missing from them (ruling 2 degrades, ruling 10 names).
#[wasm_bindgen]
pub struct PdfSkippedWidget {
    inner: tinker_pdf::SkippedWidget,
}

#[wasm_bindgen]
impl PdfSkippedWidget {
    /// The widget annotation's object number.
    #[wasm_bindgen(getter, js_name = objectNumber)]
    pub fn object_number(&self) -> u32 {
        self.inner.widget.num
    }

    /// Its generation number.
    #[wasm_bindgen(getter)]
    pub fn generation(&self) -> u16 {
        self.inner.widget.gen
    }

    /// What is wrong with it: `"rect-missing"`.
    #[wasm_bindgen(getter)]
    pub fn reason(&self) -> String {
        match self.inner.reason {
            tinker_pdf::WidgetDefect::RectMissing => "rect-missing".to_string(),
        }
    }

    /// The whole thing as one sentence, the facade's own wording.
    #[wasm_bindgen(js_name = toString)]
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.inner.to_string()
    }
}

/// An editor's state, taken as a value.
///
/// Not an open transaction: taking one changes nothing, freeing one commits
/// nothing because nothing was pending, and `restore` is idempotent.
#[wasm_bindgen]
pub struct PdfCheckpoint {
    inner: tinker_pdf::EditCheckpoint,
}

/// Options for writing, starting from the facade's own defaults.
///
/// A binding invents no defaults (ruling 11), so `new PdfWriteOptions()` is
/// `WriteOptions::default()` and every setter overrides one field of it. A
/// JavaScript caller who sets nothing writes the file a Rust caller who sets
/// nothing writes, byte for byte.
#[wasm_bindgen]
pub struct PdfWriteOptions {
    inner: tinker_pdf::WriteOptions,
}

#[wasm_bindgen]
impl PdfWriteOptions {
    /// The facade's defaults: rewrite, not linearized, version 1.7, no object
    /// streams, compressed, no encryption, no garbage collection.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PdfWriteOptions {
        PdfWriteOptions {
            inner: tinker_pdf::WriteOptions::default(),
        }
    }

    /// `"rewrite"` or `"incremental"` (7.5.6).
    #[wasm_bindgen(js_name = setMode)]
    pub fn set_mode(&mut self, mode: &str) -> Result<(), JsError> {
        self.inner.mode = match mode {
            "rewrite" => tinker_pdf::WriteMode::Rewrite,
            "incremental" => tinker_pdf::WriteMode::Incremental,
            other => {
                return Err(JsError::new(&format!(
                    "mode must be 'rewrite' or 'incremental', not {other:?}"
                )))
            }
        };
        Ok(())
    }

    /// Lay the file out for the first page to arrive first (Annex F). A
    /// request rather than a guarantee.
    #[wasm_bindgen(js_name = setLinearize)]
    pub fn set_linearize(&mut self, linearize: bool) {
        self.inner.linearize = linearize;
    }

    /// The PDF version to declare in the header, on a rewrite.
    #[wasm_bindgen(js_name = setVersion)]
    pub fn set_version(&mut self, major: u8, minor: u8) {
        self.inner.version = (major, minor);
    }

    /// Pack eligible objects into object streams (7.5.7).
    #[wasm_bindgen(js_name = setObjectStreams)]
    pub fn set_object_streams(&mut self, object_streams: bool) {
        self.inner.object_streams = object_streams;
    }

    /// Compress content streams the caller has not already encoded.
    #[wasm_bindgen(js_name = setCompress)]
    pub fn set_compress(&mut self, compress: bool) {
        self.inner.compress = compress;
    }

    /// Drop objects nothing reaches from the trailer, on a rewrite.
    #[wasm_bindgen(js_name = setGarbageCollect)]
    pub fn set_garbage_collect(&mut self, garbage_collect: bool) {
        self.inner.garbage_collect = garbage_collect;
    }

    /// Encrypt on save.
    ///
    /// `entropy` is **48 caller-supplied bytes** — the 32-byte file key and
    /// two 8-byte salts. There is no default and there will not be one: this
    /// engine has no opinion about where randomness comes from, and
    /// `wasm32-unknown-unknown` has no source of it at all. A binding that
    /// invented one would violate ruling 11 and hide the single input that
    /// makes encrypted output non-reproducible.
    #[wasm_bindgen(js_name = setEncryption)]
    pub fn set_encryption(
        &mut self,
        user_password: &str,
        owner_password: &str,
        permissions: i32,
        entropy: &[u8],
    ) -> Result<(), JsError> {
        let entropy: [u8; 48] = entropy.try_into().map_err(|_| {
            JsError::new(&format!(
                "entropy must be exactly 48 bytes, not {}",
                entropy.len()
            ))
        })?;
        self.inner.encryption = Some(tinker_pdf::Encryption {
            user_password: user_password.to_string(),
            owner_password: owner_password.to_string(),
            permissions,
            entropy,
        });
        Ok(())
    }
}

impl Default for PdfWriteOptions {
    fn default() -> Self {
        PdfWriteOptions::new()
    }
}

/// Edits layered over an open document.
#[wasm_bindgen]
pub struct PdfEditor {
    inner: tinker_pdf::DocumentEditor,
}

/// The one place a `bool` or `None` from the facade becomes a JavaScript
/// error.
///
/// The facade names no reason, so neither does this; what it can still say is
/// which call refused and what it was given.
fn refused(call: &str, detail: &str) -> JsError {
    JsError::new(&format!("{call} refused: {detail}"))
}

#[wasm_bindgen]
impl PdfEditor {
    /// Whether anything has been changed.
    #[wasm_bindgen(getter, js_name = isDirty)]
    pub fn is_dirty(&self) -> bool {
        self.inner.is_dirty()
    }

    /// How many pages the document has as this editor sees it.
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> u32 {
        u32::try_from(self.inner.page_refs().len()).unwrap_or(u32::MAX)
    }

    /// The form's field names, in document order.
    #[wasm_bindgen(js_name = fieldNames)]
    pub fn field_names(&self) -> Vec<String> {
        self.inner.fields().into_iter().map(|f| f.name).collect()
    }

    /// One field's value as text, or the empty string when it has none.
    #[wasm_bindgen(js_name = fieldValue)]
    pub fn field_value(&self, name: &str) -> String {
        self.inner
            .fields()
            .into_iter()
            .find(|f| f.name == name)
            .map(|f| f.value.as_text())
            .unwrap_or_default()
    }

    /// Removes a page.
    #[wasm_bindgen(js_name = deletePage)]
    pub fn delete_page(&mut self, index: u32) -> Result<(), JsError> {
        if self.inner.delete_page(index) {
            Ok(())
        } else {
            Err(refused("deletePage", &format!("index {index}")))
        }
    }

    /// Moves a page to a new position.
    #[wasm_bindgen(js_name = movePage)]
    pub fn move_page(&mut self, from: u32, to: u32) -> Result<(), JsError> {
        if self.inner.move_page(from, to) {
            Ok(())
        } else {
            Err(refused("movePage", &format!("from {from} to {to}")))
        }
    }

    /// Rotates a page by a quarter-turn multiple, relative to its current
    /// rotation.
    #[wasm_bindgen(js_name = rotatePage)]
    pub fn rotate_page(&mut self, index: u32, degrees: i64) -> Result<(), JsError> {
        if self.inner.rotate_page(index, degrees) {
            Ok(())
        } else {
            Err(refused(
                "rotatePage",
                &format!("index {index}, {degrees} degrees"),
            ))
        }
    }

    /// Inserts a blank page at `index`, which may equal the page count to
    /// append.
    #[wasm_bindgen(js_name = insertPage)]
    pub fn insert_page(&mut self, index: u32, width: f64, height: f64) -> Result<(), JsError> {
        if self.inner.insert_page(index, width, height).is_some() {
            Ok(())
        } else {
            Err(refused(
                "insertPage",
                &format!("index {index}, {width} by {height}"),
            ))
        }
    }

    /// Sets a page's `/CropBox` (14.11.2).
    #[wasm_bindgen(js_name = setCropBox)]
    pub fn set_crop_box(
        &mut self,
        index: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<(), JsError> {
        if self.inner.set_crop_box(index, x0, y0, x1, y1) {
            Ok(())
        } else {
            Err(refused(
                "setCropBox",
                &format!("index {index}, [{x0} {y0} {x1} {y1}]"),
            ))
        }
    }

    /// Appends operators to a page's content stream.
    #[wasm_bindgen(js_name = appendContent)]
    pub fn append_content(&mut self, page: u32, operators: &[u8]) -> Result<(), JsError> {
        if self.inner.append_content(page, operators) {
            Ok(())
        } else {
            Err(refused("appendContent", &format!("page {page}")))
        }
    }

    /// Fills a text or choice field, returning the widgets it could not draw.
    ///
    /// Throws when **nothing** was written; returns an array — empty or not —
    /// when the value was written. See [`PdfSkippedWidget`].
    #[wasm_bindgen(js_name = fillField)]
    pub fn fill_field(
        &mut self,
        name: &str,
        value: &str,
    ) -> Result<Vec<PdfSkippedWidget>, JsError> {
        match self.inner.fill_field(name, value) {
            Ok(skipped) => Ok(skipped
                .into_iter()
                .map(|inner| PdfSkippedWidget { inner })
                .collect()),
            Err(error) => Err(JsError::new(&format!("{name}: {error}"))),
        }
    }

    /// Ticks or clears a checkbox.
    #[wasm_bindgen(js_name = setCheckbox)]
    pub fn set_checkbox(&mut self, name: &str, on: bool) -> Result<(), JsError> {
        if self.inner.set_checkbox(name, on) {
            Ok(())
        } else {
            Err(refused("setCheckbox", &format!("field {name:?}")))
        }
    }

    /// Selects one option of a radio group (12.7.4.2).
    #[wasm_bindgen(js_name = selectRadio)]
    pub fn select_radio(&mut self, name: &str, option: &str) -> Result<(), JsError> {
        if self.inner.select_radio(name, option) {
            Ok(())
        } else {
            Err(refused(
                "selectRadio",
                &format!("field {name:?}, option {option:?}"),
            ))
        }
    }

    /// Takes this editor's state as a value, for `restore` to put back.
    #[wasm_bindgen]
    pub fn checkpoint(&self) -> PdfCheckpoint {
        PdfCheckpoint {
            inner: self.inner.checkpoint(),
        }
    }

    /// Puts this editor back to what a checkpoint recorded.
    ///
    /// Idempotent: restoring twice is restoring once, which is what a
    /// `finally` running after its own `catch` needs. The checkpoint is
    /// borrowed rather than consumed, so one can undo several attempts.
    #[wasm_bindgen]
    pub fn restore(&mut self, checkpoint: &PdfCheckpoint) {
        self.inner.restore(&checkpoint.inner);
    }

    /// Saves the edited document, as a copy.
    ///
    /// A copy rather than a view into wasm memory, deliberately: the read
    /// side's `viewUnsafeUntilNextAllocation` is the only aliasing view on
    /// this surface, and a write API that handed one back would hand it back
    /// at exactly the moment the caller is about to allocate again.
    #[wasm_bindgen]
    pub fn save(&self, options: &PdfWriteOptions) -> Vec<u8> {
        self.inner.save(&options.inner)
    }
}

/// A page being drawn, owned until it is pushed.
///
/// Born from a builder or not at all: there is no constructor, because a page
/// whose resource names were never resolved against a builder is a page whose
/// names mean nothing.
#[wasm_bindgen]
pub struct PdfPageBuilder {
    /// `None` once pushed: `push_page` consumes in Rust, and a JavaScript
    /// object that had been consumed would otherwise be a live handle to
    /// nothing.
    inner: Option<tinker_pdf::PageBuilder>,
}

impl PdfPageBuilder {
    fn get(&mut self) -> Result<&mut tinker_pdf::PageBuilder, JsError> {
        self.inner.as_mut().ok_or_else(|| {
            JsError::new(
                "this page was already pushed; its drawing is in the document \
                 now, so drawing on it again would write into nothing",
            )
        })
    }
}

#[wasm_bindgen]
impl PdfPageBuilder {
    /// Draws text with a registered font.
    #[wasm_bindgen]
    pub fn text(
        &mut self,
        font: &[u8],
        size: f64,
        x: f64,
        y: f64,
        text: &str,
    ) -> Result<(), JsError> {
        self.get()?.text(font, size, x, y, text);
        Ok(())
    }

    /// Fills a rectangle in device grey, from black (0) to white (1).
    #[wasm_bindgen(js_name = fillRect)]
    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, grey: f64) -> Result<(), JsError> {
        self.get()?.fill_rect(x, y, w, h, grey);
        Ok(())
    }

    /// Draws a registered image into the given rectangle.
    #[wasm_bindgen]
    pub fn image(
        &mut self,
        resource: &[u8],
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<(), JsError> {
        self.get()?.image(resource, x, y, w, h);
        Ok(())
    }

    /// Sets the non-stroking colour.
    #[wasm_bindgen(js_name = setFillRgb)]
    pub fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) -> Result<(), JsError> {
        self.get()?.set_fill_rgb(r, g, b);
        Ok(())
    }

    /// Sets the **stroking** colour. `RG`, not `rg`.
    #[wasm_bindgen(js_name = setStrokeRgb)]
    pub fn set_stroke_rgb(&mut self, r: f64, g: f64, b: f64) -> Result<(), JsError> {
        self.get()?.set_stroke_rgb(r, g, b);
        Ok(())
    }

    /// Sets this page's `/CropBox` (7.7.3.3).
    #[wasm_bindgen(js_name = setCropBox)]
    pub fn set_crop_box(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<(), JsError> {
        self.get()?.set_crop_box(x0, y0, x1, y1);
        Ok(())
    }

    /// Appends content-stream operators verbatim.
    #[wasm_bindgen]
    pub fn raw(&mut self, operators: &[u8]) -> Result<(), JsError> {
        self.get()?.raw(operators);
        Ok(())
    }

    /// Adds a link annotation over a rectangle, to a page in this document
    /// (12.5.6.5).
    #[wasm_bindgen(js_name = linkToPage)]
    pub fn link_to_page(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        page: u32,
    ) -> Result<(), JsError> {
        let target = tinker_pdf::Target::Page {
            index: page,
            view: tinker_pdf::DestKind::Fit,
        };
        if self.get()?.link(x0, y0, x1, y1, &target) {
            Ok(())
        } else {
            Err(refused("linkToPage", &format!("[{x0} {y0} {x1} {y1}]")))
        }
    }

    /// The same, to a URI (12.6.4.7). 7-bit ASCII per that clause; anything
    /// else the writer refuses rather than mangles.
    #[wasm_bindgen(js_name = linkToUri)]
    pub fn link_to_uri(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        uri: &str,
    ) -> Result<(), JsError> {
        let target = tinker_pdf::Target::Uri(uri.to_string());
        if self.get()?.link(x0, y0, x1, y1, &target) {
            Ok(())
        } else {
            Err(refused("linkToUri", &format!("{uri:?}")))
        }
    }
}

/// One outline entry under construction (12.3.3).
///
/// A tree built by handles rather than described by a plain object, because
/// the nesting is what a flat description cannot carry. `addChild` **consumes**
/// its argument, the way `DocumentBuilder::push_page` consumes a page.
#[wasm_bindgen]
pub struct PdfOutlineEntry {
    inner: Option<tinker_pdf::OutlineEntry>,
}

impl PdfOutlineEntry {
    fn get(&mut self) -> Result<&mut tinker_pdf::OutlineEntry, JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("this outline entry was already added to a tree"))
    }
}

#[wasm_bindgen]
impl PdfOutlineEntry {
    /// An entry with a title and no destination.
    ///
    /// 12.3.3 makes `/Dest` optional, and an entry without one is a real shape
    /// rather than a degraded one: a part title above three chapters often
    /// points nowhere itself.
    #[wasm_bindgen(constructor)]
    pub fn new(title: &str) -> PdfOutlineEntry {
        PdfOutlineEntry {
            inner: Some(tinker_pdf::OutlineEntry {
                title: title.to_string(),
                target: None,
                open: false,
                children: Vec::new(),
            }),
        }
    }

    /// Points the entry at a page in this document.
    #[wasm_bindgen(js_name = setPageTarget)]
    pub fn set_page_target(&mut self, index: u32) -> Result<(), JsError> {
        self.get()?.target = Some(tinker_pdf::Target::Page {
            index,
            view: tinker_pdf::DestKind::Fit,
        });
        Ok(())
    }

    /// Points the entry at a URI.
    #[wasm_bindgen(js_name = setUriTarget)]
    pub fn set_uri_target(&mut self, uri: &str) -> Result<(), JsError> {
        self.get()?.target = Some(tinker_pdf::Target::Uri(uri.to_string()));
        Ok(())
    }

    /// Whether the entry is shown expanded. Ignored for an entry with no
    /// children, which 12.3.3 leaves neither open nor closed.
    #[wasm_bindgen(js_name = setOpen)]
    pub fn set_open(&mut self, open: bool) -> Result<(), JsError> {
        self.get()?.open = open;
        Ok(())
    }

    /// Nests one entry under another, **consuming** the child.
    #[wasm_bindgen(js_name = addChild)]
    pub fn add_child(&mut self, child: &mut PdfOutlineEntry) -> Result<(), JsError> {
        let Some(taken) = child.inner.take() else {
            return Err(JsError::new(
                "this outline entry was already added to a tree",
            ));
        };
        self.get()?.children.push(taken);
        Ok(())
    }
}

/// Assembles a document from pages, fonts and images.
#[wasm_bindgen]
pub struct PdfBuilder {
    /// `None` once finished: `finish` consumes in Rust, so a second call is a
    /// refusal rather than a second document.
    inner: Option<tinker_pdf::DocumentBuilder>,
}

impl PdfBuilder {
    fn get(&mut self) -> Result<&mut tinker_pdf::DocumentBuilder, JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("this builder was already finished"))
    }
}

#[wasm_bindgen]
impl PdfBuilder {
    /// Starts a document.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PdfBuilder {
        PdfBuilder {
            inner: Some(tinker_pdf::DocumentBuilder::new()),
        }
    }

    /// Registers one of the standard 14 fonts under a resource name (9.6.2.2).
    #[wasm_bindgen(js_name = addBaseFont)]
    pub fn add_base_font(&mut self, resource: &[u8], base_font: &[u8]) -> Result<(), JsError> {
        self.get()?.add_base_font(resource, base_font);
        Ok(())
    }

    /// Embeds a TrueType or CFF font program under a resource name.
    #[wasm_bindgen(js_name = addEmbeddedFont)]
    pub fn add_embedded_font(
        &mut self,
        resource: &[u8],
        base_font: &[u8],
        program: &[u8],
    ) -> Result<(), JsError> {
        if self.get()?.add_embedded_font(resource, base_font, program) {
            Ok(())
        } else {
            Err(refused(
                "addEmbeddedFont",
                "the font program was not usable",
            ))
        }
    }

    /// Whether embedded fonts are subsetted to the glyphs actually drawn.
    #[wasm_bindgen(js_name = setSubsetFonts)]
    pub fn set_subset_fonts(&mut self, subset: bool) -> Result<(), JsError> {
        self.get()?.set_subset_fonts(subset);
        Ok(())
    }

    /// Registers an image under a resource name.
    ///
    /// `kind` is `"jpeg"`, `"rgb8"` or `"gray8"`. JPEG bytes are placed as
    /// they are and never re-encoded, so `width` and `height` are read from
    /// the bytes and ignored here.
    #[wasm_bindgen(js_name = addImage)]
    pub fn add_image(
        &mut self,
        resource: &[u8],
        data: &[u8],
        kind: &str,
        width: u32,
        height: u32,
    ) -> Result<(), JsError> {
        let described = match kind {
            "jpeg" => tinker_pdf::ImageData::Jpeg(data),
            "rgb8" => tinker_pdf::ImageData::Rgb8 {
                width,
                height,
                data,
            },
            "gray8" => tinker_pdf::ImageData::Gray8 {
                width,
                height,
                data,
            },
            other => {
                return Err(JsError::new(&format!(
                    "kind must be 'jpeg', 'rgb8' or 'gray8', not {other:?}"
                )))
            }
        };
        if self.get()?.add_image(resource, &described) {
            Ok(())
        } else {
            Err(refused(
                "addImage",
                &format!("{kind}, {width} by {height}, {} bytes", data.len()),
            ))
        }
    }

    /// Sets an `/Info` field, such as `Title` or `Author`.
    #[wasm_bindgen(js_name = setInfo)]
    pub fn set_info(&mut self, key: &[u8], value: &str) -> Result<(), JsError> {
        self.get()?.set_info(key, value);
        Ok(())
    }

    /// Sets the document outline from top-level entries, **consuming** each
    /// (12.3.3).
    #[wasm_bindgen(js_name = setOutline)]
    pub fn set_outline(&mut self, entries: Vec<PdfOutlineEntry>) -> Result<(), JsError> {
        let mut taken = Vec::with_capacity(entries.len());
        for mut entry in entries {
            let Some(one) = entry.inner.take() else {
                return Err(JsError::new(
                    "this outline entry was already added to a tree",
                ));
            };
            taken.push(one);
        }
        if self.get()?.set_outline(taken) {
            Ok(())
        } else {
            Err(refused(
                "setOutline",
                "the tree is deeper or wider than this engine's own reader walks",
            ))
        }
    }

    /// Starts a page, owned by the caller until `pushPage` takes it.
    ///
    /// **The resource snapshot happens here.** A font or image registered
    /// after this call is invisible to this page — the same timing the Rust
    /// closure form imposes, because `add_page` calls this.
    #[wasm_bindgen(js_name = beginPage)]
    pub fn begin_page(&mut self, width: f64, height: f64) -> Result<PdfPageBuilder, JsError> {
        Ok(PdfPageBuilder {
            inner: Some(self.get()?.begin_page(width, height)),
        })
    }

    /// Adds a page the caller has finished drawing, **consuming** it.
    ///
    /// A second push of the same page is a refusal rather than a second page.
    /// Pages arrive in the order they are pushed.
    #[wasm_bindgen(js_name = pushPage)]
    pub fn push_page(&mut self, page: &mut PdfPageBuilder) -> Result<(), JsError> {
        let Some(drawn) = page.inner.take() else {
            return Err(JsError::new("this page was already pushed"));
        };
        self.get()?.push_page(drawn);
        Ok(())
    }

    /// Finishes the document and returns its bytes, **consuming** the builder.
    #[wasm_bindgen]
    pub fn finish(&mut self) -> Result<Vec<u8>, JsError> {
        let Some(document) = self.inner.take() else {
            return Err(JsError::new("this builder was already finished"));
        };
        Ok(document.finish())
    }
}

#[wasm_bindgen]
impl PdfBitmap {
    /// Width in pixels.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.inner.width
    }

    /// Height in pixels.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.inner.height
    }

    /// The pixels, copied out of wasm memory.
    ///
    /// Safe to keep: nothing the engine does afterwards can disturb it.
    #[wasm_bindgen(js_name = data)]
    pub fn data(&self) -> Vec<u8> {
        self.inner.data.clone()
    }

    /// The pixels as a view into wasm memory, **invalidated by any later
    /// allocation**.
    ///
    /// Growing the wasm heap *detaches* the `ArrayBuffer` this array wraps, so
    /// the view — and every other view anybody is holding — becomes **zero
    /// length**. It does not throw, and it does not happen on every call: only
    /// when an allocation crosses a page boundary. A page that holds a view
    /// across one render therefore works until the day a document is large
    /// enough that it does not.
    ///
    /// That is measured rather than asserted from memory:
    /// `tests/node_smoke.mjs` takes a view, renders the same page at four
    /// times the scale, and requires the view's length to have become 0.
    ///
    /// Use it to draw immediately and drop it; if the pixels must outlive the
    /// next engine call, use [`PdfBitmap::data`], which copies.
    ///
    /// # Safety
    ///
    /// The returned array aliases wasm linear memory and must not be held
    /// across any call that allocates.
    #[wasm_bindgen(js_name = viewUnsafeUntilNextAllocation)]
    pub unsafe fn view(&self) -> js_sys::Uint8Array {
        unsafe { js_sys::Uint8Array::view(&self.inner.data) }
    }
}
