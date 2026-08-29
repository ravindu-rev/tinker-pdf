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
