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
//! Scope and packaging: `docs/plans/13-bindings.md`.

#![allow(clippy::new_without_default)]

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
                cancel: None,
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
    /// Growing the wasm heap moves it, and every reference JavaScript holds
    /// into the old memory then points at nothing. Use it to draw immediately
    /// and drop it; if the pixels must outlive the next engine call, use
    /// [`PdfBitmap::data`].
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
