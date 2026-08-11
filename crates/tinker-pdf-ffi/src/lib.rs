//! C ABI over the `tinker-pdf` facade.
//!
//! Handle-based and thread-safe, because the core is: a `tpdf_document` boxes
//! a `Document`, which is `Send + Sync` and cheap to clone, so handles may be
//! used from any thread and freed independently.
//!
//! **Ownership, stated once.** The engine allocates and the matching
//! `tpdf_*_free` releases; nothing crosses this boundary as a caller-freed
//! buffer. Functions returning a pointer into a handle's storage borrow it,
//! and that borrow is valid only until the handle is freed.
//!
//! Nothing here contains logic. Every function is a projection of a facade
//! call (ruling 11); if a binding needs behaviour, the facade grows it first.
//!
//! Scope, design and exit criteria: `docs/plans/13-bindings.md`.

#![warn(missing_docs)]

use std::cell::RefCell;
use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

use tinker_pdf::{AuthLevel, Bitmap, Document, PixelFormat, RenderOptions, SimpleFontProvider};

/// How a call went.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfStatus {
    /// The call succeeded.
    Ok = 0,
    /// A pointer argument was null, or a length was nonsense.
    BadArgument = 1,
    /// The bytes are not a PDF.
    NotAPdf = 2,
    /// The document is encrypted and no password has been accepted.
    NeedsPassword = 3,
    /// The password did not match.
    WrongPassword = 4,
    /// The page index is past the end of the document.
    NoSuchPage = 5,
    /// The document is not encrypted, so there is nothing to authenticate.
    NotEncrypted = 6,
    /// The security handler is one this engine does not implement.
    UnsupportedHandler = 7,
}

/// How far a password got.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfAuthLevel {
    /// Unencrypted, or no password accepted yet.
    None = 0,
    /// The user password matched.
    User = 1,
    /// The owner password matched; restrictions are lifted.
    Owner = 2,
}

/// How a bitmap stores its pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfPixelFormat {
    /// One byte of grey.
    Gray8 = 0,
    /// Grey and alpha.
    GrayA8 = 1,
    /// Red, green, blue.
    Rgb8 = 2,
    /// Red, green, blue, alpha.
    Rgba8 = 3,
}

impl From<TpdfPixelFormat> for PixelFormat {
    fn from(value: TpdfPixelFormat) -> Self {
        match value {
            TpdfPixelFormat::Gray8 => PixelFormat::Gray8,
            TpdfPixelFormat::GrayA8 => PixelFormat::GrayA8,
            TpdfPixelFormat::Rgb8 => PixelFormat::Rgb8,
            TpdfPixelFormat::Rgba8 => PixelFormat::Rgba8,
        }
    }
}

/// An open document. Opaque to callers.
pub struct TpdfDocument {
    inner: Document,
}

/// A rendered page. Opaque to callers.
pub struct TpdfBitmap {
    inner: Bitmap,
}

thread_local! {
    /// The last error message, per thread so concurrent calls cannot
    /// overwrite each other's.
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_error(message: &str) {
    let text = CString::new(message).unwrap_or_default();
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(text));
}

/// The last error message on this thread, or null.
///
/// The pointer is valid until the next call that sets an error on this
/// thread. Copy it if it must outlive that.
///
/// # Safety
///
/// The returned pointer must not be freed by the caller and must not be used
/// after another fallible call on the same thread.
#[no_mangle]
pub unsafe extern "C" fn tpdf_last_error_message() -> *const c_char {
    LAST_ERROR.with(|slot| match slot.borrow().as_ref() {
        Some(text) => text.as_ptr(),
        None => ptr::null(),
    })
}

/// The engine's version, as a static null-terminated string.
///
/// # Safety
///
/// The returned pointer is static and must not be freed.
#[no_mangle]
pub unsafe extern "C" fn tpdf_version() -> *const c_char {
    // The crate version with a null appended at compile time.
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Opens a document from bytes.
///
/// The bytes are copied, so the caller may free theirs immediately.
///
/// # Safety
///
/// `bytes` must point to at least `len` readable bytes, and `out` must be a
/// valid pointer to write a handle to.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_open(
    bytes: *const u8,
    len: usize,
    out: *mut *mut TpdfDocument,
) -> TpdfStatus {
    if bytes.is_null() || out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }

    let data = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    match Document::open(data) {
        Ok(inner) => {
            let handle = Box::new(TpdfDocument { inner });
            unsafe { *out = Box::into_raw(handle) };
            TpdfStatus::Ok
        }
        Err(e) => {
            set_error(&e.to_string());
            TpdfStatus::NotAPdf
        }
    }
}

/// Frees a document handle. Null is accepted and does nothing.
///
/// # Safety
///
/// `doc` must have come from [`tpdf_document_open`] and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_free(doc: *mut TpdfDocument) {
    if !doc.is_null() {
        drop(unsafe { Box::from_raw(doc) });
    }
}

/// The number of pages, or zero if the handle is null.
///
/// # Safety
///
/// `doc` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_page_count(doc: *const TpdfDocument) -> u32 {
    match unsafe { doc.as_ref() } {
        Some(doc) => doc.inner.page_count(),
        None => 0,
    }
}

/// Whether the document is encrypted.
///
/// # Safety
///
/// `doc` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_is_encrypted(doc: *const TpdfDocument) -> c_int {
    match unsafe { doc.as_ref() } {
        Some(doc) => c_int::from(doc.inner.is_encrypted()),
        None => 0,
    }
}

/// Tries a password, reporting which one matched.
///
/// # Safety
///
/// `doc` must be a live handle, `password` a null-terminated string, and
/// `out_level` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_authenticate(
    doc: *mut TpdfDocument,
    password: *const c_char,
    out_level: *mut TpdfAuthLevel,
) -> TpdfStatus {
    let (Some(doc), false) = (unsafe { doc.as_mut() }, password.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };

    let Ok(password) = unsafe { CStr::from_ptr(password) }.to_str() else {
        set_error("password is not valid UTF-8");
        return TpdfStatus::BadArgument;
    };

    match doc.inner.authenticate(password) {
        Ok(level) => {
            if let Some(slot) = unsafe { out_level.as_mut() } {
                *slot = match level {
                    AuthLevel::None => TpdfAuthLevel::None,
                    AuthLevel::User => TpdfAuthLevel::User,
                    AuthLevel::Owner => TpdfAuthLevel::Owner,
                };
            }
            TpdfStatus::Ok
        }
        Err(tinker_pdf::AuthError::NotEncrypted) => {
            set_error("the document is not encrypted");
            TpdfStatus::NotEncrypted
        }
        Err(tinker_pdf::AuthError::UnsupportedHandler) => {
            set_error("unsupported security handler");
            TpdfStatus::UnsupportedHandler
        }
        Err(tinker_pdf::AuthError::WrongPassword) => {
            set_error("wrong password");
            TpdfStatus::WrongPassword
        }
    }
}

/// Whether the document permits printing, respecting the authentication
/// level. Note that PDF permissions are advisory.
///
/// # Safety
///
/// `doc` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_may_print(doc: *const TpdfDocument) -> c_int {
    match unsafe { doc.as_ref() } {
        Some(doc) => c_int::from(doc.inner.permissions().print()),
        None => 0,
    }
}

/// A page's size in points.
///
/// # Safety
///
/// `doc` must be a live handle and the out pointers valid.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_size(
    doc: *const TpdfDocument,
    index: u32,
    out_width: *mut f64,
    out_height: *mut f64,
) -> TpdfStatus {
    let Some(doc) = (unsafe { doc.as_ref() }) else {
        set_error("null document");
        return TpdfStatus::BadArgument;
    };
    let Some(page) = doc.inner.page(index) else {
        set_error("no such page");
        return TpdfStatus::NoSuchPage;
    };

    let (w, h) = page.size();
    if let Some(slot) = unsafe { out_width.as_mut() } {
        *slot = w;
    }
    if let Some(slot) = unsafe { out_height.as_mut() } {
        *slot = h;
    }
    TpdfStatus::Ok
}

/// Extracts a page's text as a null-terminated UTF-8 string.
///
/// The caller frees the result with [`tpdf_string_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_text(
    doc: *const TpdfDocument,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let (Some(doc), false) = (unsafe { doc.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let Some(page) = doc.inner.page(index) else {
        set_error("no such page");
        return TpdfStatus::NoSuchPage;
    };

    // A page's text may contain a null byte only if the document put one
    // there; replacing it keeps the C string well formed.
    let text = page.text().plain_text().replace('\0', " ");
    let Ok(text) = CString::new(text) else {
        set_error("text could not be represented as a C string");
        return TpdfStatus::BadArgument;
    };
    unsafe { *out = text.into_raw() };
    TpdfStatus::Ok
}

/// Frees a string returned by this library. Null is accepted.
///
/// # Safety
///
/// `text` must have come from a function in this library that says so.
#[no_mangle]
pub unsafe extern "C" fn tpdf_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(unsafe { CString::from_raw(text) });
    }
}

/// Supplies a font for documents that embed none.
///
/// Without one, such a document extracts its text perfectly and draws none of
/// it — the standard-14 metrics are built in, the outlines are not. The engine
/// bundles no faces and reads no font directories, so a host that wants text
/// drawn says where to find it.
///
/// The bytes are a TrueType or bare CFF program and are **copied**, so the
/// caller may free them on return. Passing the same document twice replaces
/// the previous face.
///
/// Weight and slope are chosen from the four faces given; a null pointer for
/// any of them falls back to `regular`, which is why that one is required.
///
/// # Safety
///
/// `doc` must be a live handle. Each non-null `*_data` pointer must be valid
/// for its stated length.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_set_fonts(
    doc: *mut TpdfDocument,
    regular_data: *const u8,
    regular_len: usize,
    bold_data: *const u8,
    bold_len: usize,
    italic_data: *const u8,
    italic_len: usize,
    bold_italic_data: *const u8,
    bold_italic_len: usize,
) -> TpdfStatus {
    let Some(doc) = (unsafe { doc.as_mut() }) else {
        set_error("null document");
        return TpdfStatus::BadArgument;
    };

    // Safety: each pointer is checked for null and paired with its own length,
    // which the contract above makes the caller's responsibility.
    let face = |data: *const u8, len: usize| -> Option<Vec<u8>> {
        if data.is_null() || len == 0 {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts(data, len) }.to_vec())
    };

    let Some(regular) = face(regular_data, regular_len) else {
        set_error("a regular face is required");
        return TpdfStatus::BadArgument;
    };

    let mut provider = SimpleFontProvider::new(regular);
    if let Some(bytes) = face(bold_data, bold_len) {
        provider = provider.with_bold(bytes);
    }
    if let Some(bytes) = face(italic_data, italic_len) {
        provider = provider.with_italic(bytes);
    }
    if let Some(bytes) = face(bold_italic_data, bold_italic_len) {
        provider = provider.with_bold_italic(bytes);
    }

    // `with_fonts` consumes and returns the document, and a Document is cheap
    // to clone — the bytes and the object store are shared — so replacing the
    // handle's contents costs an Arc bump rather than a reparse.
    doc.inner = doc.inner.clone().with_fonts(std::sync::Arc::new(provider));
    TpdfStatus::Ok
}

/// Renders a page.
///
/// The caller frees the result with [`tpdf_bitmap_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_render(
    doc: *const TpdfDocument,
    index: u32,
    scale: f64,
    format: TpdfPixelFormat,
    out: *mut *mut TpdfBitmap,
) -> TpdfStatus {
    let (Some(doc), false) = (unsafe { doc.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let Some(page) = doc.inner.page(index) else {
        set_error("no such page");
        return TpdfStatus::NoSuchPage;
    };

    let bitmap = page.render(&RenderOptions {
        scale,
        format: format.into(),
        ..RenderOptions::default()
    });
    unsafe { *out = Box::into_raw(Box::new(TpdfBitmap { inner: bitmap })) };
    TpdfStatus::Ok
}

/// A bitmap's width in pixels.
///
/// # Safety
///
/// `bitmap` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_bitmap_width(bitmap: *const TpdfBitmap) -> u32 {
    unsafe { bitmap.as_ref() }.map_or(0, |b| b.inner.width)
}

/// A bitmap's height in pixels.
///
/// # Safety
///
/// `bitmap` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_bitmap_height(bitmap: *const TpdfBitmap) -> u32 {
    unsafe { bitmap.as_ref() }.map_or(0, |b| b.inner.height)
}

/// A bitmap's bytes per row.
///
/// # Safety
///
/// `bitmap` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_bitmap_stride(bitmap: *const TpdfBitmap) -> usize {
    unsafe { bitmap.as_ref() }.map_or(0, |b| b.inner.stride)
}

/// A borrowed pointer to a bitmap's pixels, with its length.
///
/// The pointer is valid until the bitmap is freed. It is **not** the caller's
/// to release.
///
/// # Safety
///
/// `bitmap` must be a live handle; `out_len` may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_bitmap_data(
    bitmap: *const TpdfBitmap,
    out_len: *mut usize,
) -> *const u8 {
    let Some(bitmap) = (unsafe { bitmap.as_ref() }) else {
        return ptr::null();
    };
    if let Some(slot) = unsafe { out_len.as_mut() } {
        *slot = bitmap.inner.data.len();
    }
    bitmap.inner.data.as_ptr()
}

/// Frees a bitmap. Null is accepted.
///
/// # Safety
///
/// `bitmap` must have come from [`tpdf_page_render`] and must not be used
/// afterwards, nor must any pointer [`tpdf_bitmap_data`] returned for it.
#[no_mangle]
pub unsafe extern "C" fn tpdf_bitmap_free(bitmap: *mut TpdfBitmap) {
    if !bitmap.is_null() {
        drop(unsafe { Box::from_raw(bitmap) });
    }
}

#[cfg(test)]
mod tests {

    /// A face whose every glyph from 32 up is a filled box, so "did anything
    /// draw" has an unambiguous answer.
    fn boxy_font() -> Vec<u8> {
        let mut glyph = Vec::new();
        glyph.extend_from_slice(&1i16.to_be_bytes());
        for value in [0i16, 0, 700, 700] {
            glyph.extend_from_slice(&value.to_be_bytes());
        }
        glyph.extend_from_slice(&3u16.to_be_bytes());
        glyph.extend_from_slice(&0u16.to_be_bytes());
        glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
        for dx in [0i16, 700, 0, -700] {
            glyph.extend_from_slice(&dx.to_be_bytes());
        }
        for dy in [0i16, 0, 700, 0] {
            glyph.extend_from_slice(&dy.to_be_bytes());
        }

        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        head[50..52].copy_from_slice(&1i16.to_be_bytes());

        const FIRST: usize = 32;
        const LAST: usize = 255;
        let size = glyph.len() as u32;
        let mut glyf = Vec::new();
        for _ in FIRST..=LAST {
            glyf.extend_from_slice(&glyph);
        }
        let mut loca = Vec::new();
        for index in 0..=LAST + 1 {
            loca.extend_from_slice(&((index.saturating_sub(FIRST)) as u32 * size).to_be_bytes());
        }

        let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    /// A page of base-14 text with nothing embedded — the shape of most simple
    /// documents, and the one no binding could draw.
    fn text_document() -> Vec<u8> {
        // Written by hand rather than with the builder: ruling 11 keeps a
        // binding on the facade alone, and reaching into the COS crate for a
        // test fixture is still an edge. The DAG check caught exactly that.
        let content = b"BT /F0 24 Tf 10 20 Td (HELLO) Tj ET
";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"%PDF-1.7
",
        );
        bytes.extend_from_slice(
            b"1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
",
        );
        bytes.extend_from_slice(
            b"2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
",
        );
        bytes.extend_from_slice(
            b"3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 60]
              /Resources << /Font << /F0 4 0 R >> >> /Contents 5 0 R >>
endobj
",
        );
        bytes.extend_from_slice(
            b"4 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
",
        );
        bytes.extend_from_slice(
            format!(
                "5 0 obj
<< /Length {} >>
stream
",
                content.len()
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(content);
        bytes.extend_from_slice(
            b"endstream
endobj
",
        );
        bytes.extend_from_slice(
            b"trailer
<< /Size 6 /Root 1 0 R >>
%%EOF
",
        );
        bytes
    }

    fn inked(bitmap: &TpdfBitmap) -> usize {
        bitmap
            .inner
            .data
            .chunks_exact(bitmap.inner.components())
            .filter(|p| p[0] < 250)
            .count()
    }

    /// The font seam was Rust-only, so no binding could draw text for a
    /// document that embeds no fonts — which is most simple documents.
    #[test]
    fn a_supplied_face_reaches_the_c_abi() {
        let bytes = text_document();
        let mut doc: *mut TpdfDocument = std::ptr::null_mut();
        let status = unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) };
        assert_eq!(status, TpdfStatus::Ok);

        let mut before: *mut TpdfBitmap = std::ptr::null_mut();
        let status = unsafe { tpdf_page_render(doc, 0, 1.0, TpdfPixelFormat::Rgb8, &mut before) };
        assert_eq!(status, TpdfStatus::Ok);
        assert_eq!(
            inked(unsafe { &*before }),
            0,
            "without a face there is nothing to draw"
        );

        let face = boxy_font();
        let status = unsafe {
            tpdf_document_set_fonts(
                doc,
                face.as_ptr(),
                face.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(status, TpdfStatus::Ok);

        let mut after: *mut TpdfBitmap = std::ptr::null_mut();
        let status = unsafe { tpdf_page_render(doc, 0, 1.0, TpdfPixelFormat::Rgb8, &mut after) };
        assert_eq!(status, TpdfStatus::Ok);
        assert!(inked(unsafe { &*after }) > 0, "and with one, text draws");

        unsafe {
            tpdf_bitmap_free(before);
            tpdf_bitmap_free(after);
            tpdf_document_free(doc);
        }
    }

    /// A regular face is the one that cannot be omitted, since the others fall
    /// back to it.
    #[test]
    fn setting_fonts_without_a_regular_face_is_refused() {
        let bytes = text_document();
        let mut doc: *mut TpdfDocument = std::ptr::null_mut();
        unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) };

        let status = unsafe {
            tpdf_document_set_fonts(
                doc,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(status, TpdfStatus::BadArgument);
        unsafe { tpdf_document_free(doc) };
    }

    #[test]
    fn setting_fonts_on_a_null_document_is_refused() {
        let face = boxy_font();
        let status = unsafe {
            tpdf_document_set_fonts(
                std::ptr::null_mut(),
                face.as_ptr(),
                face.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(status, TpdfStatus::BadArgument);
    }
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    }

    fn open(name: &str) -> *mut TpdfDocument {
        let bytes = fixture(name);
        let mut doc: *mut TpdfDocument = ptr::null_mut();
        let status = unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) };
        assert_eq!(status, TpdfStatus::Ok);
        assert!(!doc.is_null());
        doc
    }

    #[test]
    fn a_document_opens_and_reports_its_pages() {
        let doc = open("simple-text.pdf");
        assert_eq!(unsafe { tpdf_document_page_count(doc) }, 3);
        assert_eq!(unsafe { tpdf_document_is_encrypted(doc) }, 0);
        unsafe { tpdf_document_free(doc) };
    }

    #[test]
    fn page_size_and_text_cross_the_boundary() {
        let doc = open("simple-text.pdf");

        let (mut w, mut h) = (0.0, 0.0);
        assert_eq!(
            unsafe { tpdf_page_size(doc, 0, &mut w, &mut h) },
            TpdfStatus::Ok
        );
        assert!((w - 595.0).abs() < 1.0 && (h - 842.0).abs() < 1.0);

        let mut text: *mut c_char = ptr::null_mut();
        assert_eq!(unsafe { tpdf_page_text(doc, 0, &mut text) }, TpdfStatus::Ok);
        let extracted = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        assert!(extracted.contains("Tinker"), "got {extracted:?}");
        unsafe { tpdf_string_free(text) };

        unsafe { tpdf_document_free(doc) };
    }

    #[test]
    fn rendering_crosses_the_boundary_with_its_pixels() {
        let doc = open("simple-text.pdf");
        let mut bitmap: *mut TpdfBitmap = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_page_render(doc, 0, 1.0, TpdfPixelFormat::Rgb8, &mut bitmap) },
            TpdfStatus::Ok
        );

        assert_eq!(unsafe { tpdf_bitmap_width(bitmap) }, 595);
        assert_eq!(unsafe { tpdf_bitmap_height(bitmap) }, 842);

        let mut len = 0usize;
        let data = unsafe { tpdf_bitmap_data(bitmap, &mut len) };
        assert!(!data.is_null());
        assert_eq!(len, 595 * 842 * 3);

        unsafe { tpdf_bitmap_free(bitmap) };
        unsafe { tpdf_document_free(doc) };
    }

    #[test]
    fn authentication_reports_which_password_matched() {
        let doc = open("encrypted-aes256.pdf");
        assert_eq!(unsafe { tpdf_document_is_encrypted(doc) }, 1);

        let mut level = TpdfAuthLevel::None;
        let wrong = CString::new("nope").unwrap_or_default();
        assert_eq!(
            unsafe { tpdf_document_authenticate(doc, wrong.as_ptr(), &mut level) },
            TpdfStatus::WrongPassword
        );

        let owner = CString::new("owner-secret").unwrap_or_default();
        assert_eq!(
            unsafe { tpdf_document_authenticate(doc, owner.as_ptr(), &mut level) },
            TpdfStatus::Ok
        );
        assert_eq!(level, TpdfAuthLevel::Owner);

        unsafe { tpdf_document_free(doc) };
    }

    #[test]
    fn null_and_nonsense_arguments_are_refused_rather_than_dereferenced() {
        let mut doc: *mut TpdfDocument = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_open(ptr::null(), 0, &mut doc) },
            TpdfStatus::BadArgument
        );

        let bytes = b"not a pdf";
        assert_eq!(
            unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) },
            TpdfStatus::NotAPdf
        );
        assert!(!unsafe { tpdf_last_error_message() }.is_null());

        // Every accessor tolerates null.
        assert_eq!(unsafe { tpdf_document_page_count(ptr::null()) }, 0);
        assert_eq!(unsafe { tpdf_bitmap_width(ptr::null()) }, 0);
        assert!(unsafe { tpdf_bitmap_data(ptr::null(), ptr::null_mut()) }.is_null());
        // And freeing null is a no-op rather than a crash.
        unsafe { tpdf_document_free(ptr::null_mut()) };
        unsafe { tpdf_bitmap_free(ptr::null_mut()) };
        unsafe { tpdf_string_free(ptr::null_mut()) };
    }

    #[test]
    fn a_page_past_the_end_is_reported() {
        let doc = open("simple-text.pdf");
        let (mut w, mut h) = (0.0, 0.0);
        assert_eq!(
            unsafe { tpdf_page_size(doc, 99, &mut w, &mut h) },
            TpdfStatus::NoSuchPage
        );
        unsafe { tpdf_document_free(doc) };
    }

    #[test]
    fn the_version_string_is_readable() {
        let version = unsafe { CStr::from_ptr(tpdf_version()) };
        assert_eq!(version.to_string_lossy(), tinker_pdf::VERSION);
    }
}
