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

use tinker_pdf::{AuthLevel, Bitmap, Document, PixelFormat, RenderOptions};

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
        cancel: None,
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
