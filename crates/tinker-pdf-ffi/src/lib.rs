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
//! Feature documentation: `docs/features/bindings.md`.

#![warn(missing_docs)]

use std::cell::RefCell;
use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

use tinker_pdf::{
    AuthLevel, Bitmap, Chain, CmsState, Coverage, DestKind, Document, DocumentBuilder,
    DocumentDigest, DocumentEditor, EditCheckpoint, Encryption, FillError, ImageData, OutlineEntry,
    PageBuilder, PixelFormat, RenderOptions, Signature, SignatureCheck, SimpleFontProvider,
    SkippedWidget, Target, TrustAnchors, Verdict, Weakness, WidgetDefect, WriteMode, WriteOptions,
};

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
    /// The signature index is past the last signature the document has.
    ///
    /// Appended at 8 rather than inserted: 0-7 are ABI a caller compares
    /// literals against, and a test pins every one of them.
    NoSuchSignature = 8,

    // ---- the write surface (gap 32), appended at 9 -------------------------
    //
    // Same rule as 8, and it is the only rule this enum has: **append**.
    // A released binding compiled against 0-8 must keep working, so nothing
    // above is renumbered and nothing is inserted between.
    /// 12.7.3.2: no field carries that fully qualified name.
    ///
    /// Also the answer when a field *index* is past the last field, which is
    /// the same sentence about a different way of asking.
    NoSuchField = 9,
    /// The field will not take that value: it is read-only (12.7.4.1 Table
    /// 227), the value is longer than `/MaxLen`, or a non-editable list does
    /// not offer it.
    ///
    /// Refusing beats truncating, which hides a data error inside a file that
    /// then looks correctly filled.
    ValueRefused = 10,
    /// The field's own object is not a dictionary, so there is nowhere to put
    /// `/V`. A damaged file rather than a rejected value.
    FieldUnreadable = 11,
    /// The handle was consumed by an earlier call, which
    /// [`tpdf_last_error_message`] names.
    ///
    /// `DocumentBuilder::finish` and `push_page` consume in Rust, and a
    /// consuming call across an ABI is a double-free factory. So the handle
    /// boxes an `Option` and the consuming call takes it; the handle stays
    /// live, `tpdf_*_free` stays required and safe, and asking it to work
    /// twice is this rather than undefined behaviour.
    SpentHandle = 12,
    /// The editor refused the edit, and the facade's own answer is a `bool`,
    /// so the reason does not cross.
    ///
    /// This is the [`TpdfCoverage`] decision applied to the write side. The
    /// operations behind it -- `delete_page`, `move_page`, `rotate_page`,
    /// `insert_page`, `set_crop_box`, `set_checkbox`, `select_radio`,
    /// `append_content` -- return `bool` or `Option` on the facade and name
    /// no reason. An index that does not exist and a page object that is not
    /// a dictionary are the same `false` there, and it is not this crate's
    /// place to invent a distinction the facade does not make (ruling 11):
    /// a C ABI that split them would be guessing, and a caller would believe
    /// the guess.
    ///
    /// What does cross is provenance, through [`tpdf_last_error_message`],
    /// which names the call and the argument it refused (ruling 10). And a
    /// caller that wants to tell "past the end" apart itself has
    /// [`tpdf_editor_page_count`] and [`tpdf_editor_field_count`] to do it
    /// with, before the call rather than after.
    EditRefused = 13,
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

/// What a signature's `/ByteRange` covers, checked against the file.
///
/// The facade's `Coverage` carries a revision index on one arm and a named
/// defect on another. Neither crosses: a C enum has no payload, and inventing
/// a struct to carry one would be this crate deciding how a defect is spelled
/// when the facade already spells it (ruling 11). The three-way answer is what
/// crosses, and it is the distinction a caller acts on — a signature over the
/// document, a signature over an earlier revision of it, or a `/ByteRange`
/// that did not hold up.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfCoverage {
    /// Every byte of the file except the one gap holding `/Contents`.
    WholeFile = 0,
    /// Every byte up to the end of an earlier revision, with later
    /// incremental updates outside it. Which revision does not cross.
    Revision = 1,
    /// Anything else. The reason does not cross.
    Suspicious = 2,
}

impl TpdfCoverage {
    /// The C spelling of a facade coverage.
    fn of(coverage: &Coverage) -> TpdfCoverage {
        match coverage {
            Coverage::WholeFile => TpdfCoverage::WholeFile,
            Coverage::Revision { .. } => TpdfCoverage::Revision,
            Coverage::Suspicious(_) => TpdfCoverage::Suspicious,
        }
    }
}

/// Whether the CMS blob could be read.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfCmsState {
    /// It parsed. How many `SignerInfo`s it holds does not cross; the verdict
    /// describes the first, exactly as the facade's does.
    Read = 0,
    /// `/Contents` held no bytes to parse.
    Absent = 1,
    /// Bytes were there and would not parse. The parser's own reason does not
    /// cross.
    Unreadable = 2,
}

impl TpdfCmsState {
    /// The C spelling of a facade CMS state.
    fn of(state: &CmsState) -> TpdfCmsState {
        match state {
            CmsState::Read { .. } => TpdfCmsState::Read,
            CmsState::Absent => TpdfCmsState::Absent,
            CmsState::Unreadable(_) => TpdfCmsState::Unreadable,
        }
    }
}

/// Whether the document still hashes to what the signature was made over.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfDocumentDigest {
    /// The covered bytes digest to what the CMS says they did.
    Matches = 0,
    /// They do not. Either the bytes changed or the signature was never over
    /// them.
    Differs = 1,
    /// Not checked. **This is not a failure.** The facade names a reason for
    /// every one of these and the reason does not cross; a caller that folds
    /// this into `Differs` has confused "we did not look" with "we looked and
    /// it was wrong", which is the shape of every convincing forgery.
    NotChecked = 2,
}

impl TpdfDocumentDigest {
    /// The C spelling of a facade digest answer.
    fn of(digest: &DocumentDigest) -> TpdfDocumentDigest {
        match digest {
            DocumentDigest::Matches => TpdfDocumentDigest::Matches,
            DocumentDigest::Differs => TpdfDocumentDigest::Differs,
            DocumentDigest::NotChecked(_) => TpdfDocumentDigest::NotChecked,
        }
    }
}

/// Whether the signature verifies against the signer's own public key.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfSignatureCheck {
    /// It verifies.
    Verified = 0,
    /// The arithmetic ran and the signature is not the one that key would
    /// have made.
    Failed = 1,
    /// Not checked, and — as with [`TpdfDocumentDigest::NotChecked`] — not a
    /// failure. The reason does not cross.
    NotChecked = 2,
}

impl TpdfSignatureCheck {
    /// The C spelling of a facade signature check.
    fn of(check: &SignatureCheck) -> TpdfSignatureCheck {
        match check {
            SignatureCheck::Verified => TpdfSignatureCheck::Verified,
            SignatureCheck::Failed => TpdfSignatureCheck::Failed,
            SignatureCheck::NotChecked(_) => TpdfSignatureCheck::NotChecked,
        }
    }
}

/// How far the certificate chain reached.
///
/// The subjects each arm names — the anchor, the self-signed certificate, the
/// missing issuer, the broken link — do not cross. They are strings about
/// certificates rather than answers about the document, and projecting six
/// arms' worth of payload would be six more accessors for one enum.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfChain {
    /// A path was built to an anchor the caller supplied and every link's
    /// signature verified.
    AnchoredTo = 0,
    /// The path ends at a self-signed certificate that is not an anchor.
    SelfSigned = 1,
    /// No issuer for some certificate was found, so the path stops.
    Incomplete = 2,
    /// A link's signature did not verify, so the path is not a path.
    Broken = 3,
    /// No anchors were supplied, so no path was attempted. Distinct from
    /// `Incomplete`: the caller declined to say what it trusts.
    NoAnchors = 4,
    /// The signer's certificate was not in the blob, so there was nothing to
    /// start from.
    NoSignerCertificate = 5,
}

impl TpdfChain {
    /// The C spelling of a facade chain result.
    fn of(chain: &Chain) -> TpdfChain {
        match chain {
            Chain::AnchoredTo { .. } => TpdfChain::AnchoredTo,
            Chain::SelfSigned { .. } => TpdfChain::SelfSigned,
            Chain::Incomplete { .. } => TpdfChain::Incomplete,
            Chain::Broken { .. } => TpdfChain::Broken,
            Chain::NoAnchors => TpdfChain::NoAnchors,
            Chain::NoSignerCertificate => TpdfChain::NoSignerCertificate,
        }
    }
}

/// Something the verdict accepted that a caller should be told about.
///
/// The payloads — how many bits the short key had, whose validity window the
/// instant fell outside — do not cross, for the same reason the chain's
/// subjects do not.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfWeakness {
    /// The document digest is SHA-1, which is not collision-resistant.
    Sha1Digest = 0,
    /// The signature algorithm is SHA-1 with RSA.
    Sha1Signature = 1,
    /// The signer's RSA modulus is under 2 048 bits. How far under does not
    /// cross.
    ShortRsaKey = 2,
    /// The signature covers an earlier revision, so later ones are outside it.
    CoversOnlyARevision = 3,
    /// The `/ByteRange` did not hold up.
    CoverageSuspicious = 4,
    /// A certificate's validity window does not contain the instant the caller
    /// asked about. Whose certificate does not cross.
    OutsideValidity = 5,
}

impl TpdfWeakness {
    /// The C spelling of a facade weakness.
    fn of(weakness: &Weakness) -> TpdfWeakness {
        match weakness {
            Weakness::Sha1Digest => TpdfWeakness::Sha1Digest,
            Weakness::Sha1Signature => TpdfWeakness::Sha1Signature,
            Weakness::ShortRsaKey { .. } => TpdfWeakness::ShortRsaKey,
            Weakness::CoversOnlyARevision => TpdfWeakness::CoversOnlyARevision,
            Weakness::CoverageSuspicious => TpdfWeakness::CoverageSuspicious,
            Weakness::OutsideValidity { .. } => TpdfWeakness::OutsideValidity,
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

/// Every signature a document carries, read. Opaque to callers.
///
/// The reading is owned rather than borrowed, so this outlives the
/// `TpdfDocument` it came from and is freed independently -- the same
/// arrangement `TpdfBitmap` has, and for the same reason.
pub struct TpdfSignatures {
    inner: Vec<Signature>,
}

/// Certificates the caller trusts, as DER. Opaque to callers.
pub struct TpdfTrustAnchors {
    inner: TrustAnchors,
}

/// One verdict per signature, in the order the signatures came in. Opaque to
/// callers.
pub struct TpdfVerdicts {
    inner: Vec<Verdict>,
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

/// What the strict structural validator found. Opaque to callers.
///
/// Owned rather than borrowed, so this outlives the [`TpdfDocument`] it came
/// from -- the [`TpdfBitmap`] and [`TpdfSignatures`] arrangement.
pub struct TpdfDefects {
    inner: Vec<tinker_pdf::Defect>,
}

/// Runs the strict structural validator over a document (ruling 13).
///
/// **This is the check that keeps four byte-identical outputs from being
/// identically wrong.** The write-parity suite compares four surfaces' bytes
/// to each other; agreement alone would be satisfied by four copies of a
/// broken file, so every saved artefact is re-opened and put through this. It
/// is first-party by ruling 13, which is precisely why it can be relied on
/// here rather than being an external step that might be skipped.
///
/// An empty result is a clean document. The caller frees the handle with
/// [`tpdf_defects_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_validate(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfDefects,
) -> TpdfStatus {
    let (Some(doc), false) = (unsafe { doc.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let handle = Box::new(TpdfDefects {
        inner: doc.inner.validate(),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// How many defects were found. Zero is a clean document, and is the answer
/// for a null handle too -- but a caller that never called
/// [`tpdf_document_validate`] has not validated anything, which is why the
/// call returns a status of its own.
///
/// # Safety
///
/// `defects` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_defects_count(defects: *const TpdfDefects) -> u32 {
    match unsafe { defects.as_ref() } {
        Some(defects) => count(defects.inner.len()),
        None => 0,
    }
}

/// One defect's rule name, such as `binary-comment-missing`.
///
/// The stable slug rather than the prose, because it is what a caller compares
/// and greps. The caller frees it with [`tpdf_string_free`].
///
/// # Safety
///
/// `defects` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_defect_rule(
    defects: *const TpdfDefects,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let Some(defects) = (unsafe { defects.as_ref() }) else {
        set_error("null defects");
        return TpdfStatus::BadArgument;
    };
    let Some(defect) = defects.inner.get(index as usize) else {
        set_error("no such defect");
        return TpdfStatus::BadArgument;
    };
    unsafe { hand_over_string(out, Some(defect.kind.as_str())) }
}

/// One defect rendered as a sentence, the facade's own wording.
///
/// The caller frees it with [`tpdf_string_free`].
///
/// # Safety
///
/// `defects` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_defect_message(
    defects: *const TpdfDefects,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let Some(defects) = (unsafe { defects.as_ref() }) else {
        set_error("null defects");
        return TpdfStatus::BadArgument;
    };
    let Some(defect) = defects.inner.get(index as usize) else {
        set_error("no such defect");
        return TpdfStatus::BadArgument;
    };
    unsafe { hand_over_string(out, Some(&defect.to_string())) }
}

/// Frees a validation result. Null is accepted and does nothing.
///
/// # Safety
///
/// `defects` must have come from [`tpdf_document_validate`] and must not be
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_defects_free(defects: *mut TpdfDefects) {
    if !defects.is_null() {
        drop(unsafe { Box::from_raw(defects) });
    }
}

// ---- signatures, read (12.8) ----------------------------------------------
//
// Reading only. The signing side is not here and is not coming here: a
// `Signer` is a host callback, and a callback across this boundary is an
// explicit non-goal of `docs/design/bindings-write.md`, which owns it.

/// Hands an optional string to C, or refuses it.
///
/// Absent is a real answer across this whole surface — a signature dictionary
/// with no `/Reason` is not an error — so absent writes a null pointer and
/// returns [`TpdfStatus::Ok`]. That is why a wrong *index* is
/// [`TpdfStatus::NoSuchSignature`] rather than a null: collapsing the two
/// would leave a caller unable to tell "the document does not say" from "you
/// asked about a signature that is not there".
unsafe fn hand_over_string(out: *mut *mut c_char, value: Option<&str>) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let Some(value) = value else {
        unsafe { *out = ptr::null_mut() };
        return TpdfStatus::Ok;
    };
    // A string in a signature dictionary may contain a null byte only if the
    // document put one there; replacing it keeps the C string well formed,
    // the same way `tpdf_page_text` does.
    let Ok(text) = CString::new(value.replace('\0', " ")) else {
        set_error("the value could not be represented as a C string");
        return TpdfStatus::BadArgument;
    };
    unsafe { *out = text.into_raw() };
    TpdfStatus::Ok
}

/// A count as C sees it, saturating rather than wrapping.
///
/// A `usize` that does not fit a `u32` cannot arise from any document this
/// engine can hold in memory; saturating is what keeps the impossible case
/// from being a silent wrap, and there is no panic on either path.
fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// One signature, or the refusal that says which argument was wrong.
unsafe fn signature_at<'a>(
    signatures: *const TpdfSignatures,
    index: u32,
) -> Result<&'a Signature, TpdfStatus> {
    let Some(signatures) = (unsafe { signatures.as_ref() }) else {
        set_error("null signatures handle");
        return Err(TpdfStatus::BadArgument);
    };
    match signatures.inner.get(index as usize) {
        Some(signature) => Ok(signature),
        None => {
            set_error("no such signature");
            Err(TpdfStatus::NoSuchSignature)
        }
    }
}

/// One verdict, or the refusal that says which argument was wrong.
unsafe fn verdict_at<'a>(
    verdicts: *const TpdfVerdicts,
    index: u32,
) -> Result<&'a Verdict, TpdfStatus> {
    let Some(verdicts) = (unsafe { verdicts.as_ref() }) else {
        set_error("null verdicts handle");
        return Err(TpdfStatus::BadArgument);
    };
    match verdicts.inner.get(index as usize) {
        Some(verdict) => Ok(verdict),
        None => {
            set_error("no such signature");
            Err(TpdfStatus::NoSuchSignature)
        }
    }
}

/// The document's digital signatures (12.8), in the facade's order.
///
/// Nothing here is verified; that is [`tpdf_document_verify_signatures`]. Each
/// entry says what the file claims and what checking that claim against the
/// file established.
///
/// The handle owns its own copy, so it outlives the document it came from and
/// is freed independently with [`tpdf_signatures_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer to write a handle to.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_signatures(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfSignatures,
) -> TpdfStatus {
    let (Some(doc), false) = (unsafe { doc.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let handle = Box::new(TpdfSignatures {
        inner: doc.inner.signatures(),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// How many signatures the handle holds, or zero if it is null.
///
/// This is the range every `index` below is checked against.
///
/// # Safety
///
/// `signatures` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signatures_count(signatures: *const TpdfSignatures) -> u32 {
    unsafe { signatures.as_ref() }.map_or(0, |s| count(s.inner.len()))
}

/// Frees a signatures handle. Null is accepted and does nothing.
///
/// # Safety
///
/// `signatures` must have come from [`tpdf_document_signatures`] and must not
/// be used afterwards. Strings it handed out are unaffected: each is the
/// caller's until [`tpdf_string_free`].
#[no_mangle]
pub unsafe extern "C" fn tpdf_signatures_free(signatures: *mut TpdfSignatures) {
    if !signatures.is_null() {
        drop(unsafe { Box::from_raw(signatures) });
    }
}

/// The fully qualified name of the field holding the signature (12.7.3.2).
///
/// Null on `Ok` when the signature was reached through the catalog's `/Perms`
/// rather than through a field, which is what a usage-rights signature
/// usually is. The caller frees a non-null result with [`tpdf_string_free`].
///
/// # Safety
///
/// `signatures` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_field_name(
    signatures: *const TpdfSignatures,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => unsafe { hand_over_string(out, signature.field.as_deref()) },
        Err(status) => status,
    }
}

/// `/SubFilter` exactly as the document wrote it, recognised or not.
///
/// As written rather than as recognised, because a scheme this build cannot
/// verify is still a scheme the document names, and rendering it through the
/// recognised set would turn an unknown into a silence. Null on `Ok` when the
/// dictionary has no `/SubFilter`. Freed with [`tpdf_string_free`].
///
/// # Safety
///
/// `signatures` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_sub_filter(
    signatures: *const TpdfSignatures,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => unsafe { hand_over_string(out, signature.sub_filter_name.as_deref()) },
        Err(status) => status,
    }
}

/// `/Reason`. Null on `Ok` when the dictionary has none. Freed with
/// [`tpdf_string_free`].
///
/// # Safety
///
/// `signatures` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_reason(
    signatures: *const TpdfSignatures,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => unsafe { hand_over_string(out, signature.reason.as_deref()) },
        Err(status) => status,
    }
}

/// `/Location`. Null on `Ok` when the dictionary has none. Freed with
/// [`tpdf_string_free`].
///
/// # Safety
///
/// `signatures` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_location(
    signatures: *const TpdfSignatures,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => unsafe { hand_over_string(out, signature.location.as_deref()) },
        Err(status) => status,
    }
}

/// `/Name`, the signer's own claim about who they are — not the certificate's.
///
/// Null on `Ok` when the dictionary has none. Freed with
/// [`tpdf_string_free`]. The certificate's answer to the same question is
/// [`tpdf_verdict_signer_subject`].
///
/// # Safety
///
/// `signatures` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_name(
    signatures: *const TpdfSignatures,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => unsafe { hand_over_string(out, signature.name.as_deref()) },
        Err(status) => status,
    }
}

/// What the signature's `/ByteRange` covers, checked against the file.
///
/// An out-parameter rather than a return value because a `TpdfCoverage` has no
/// spare arm to spend on "you asked wrongly", and adding one would put a
/// non-answer among three answers.
///
/// # Safety
///
/// `signatures` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_coverage(
    signatures: *const TpdfSignatures,
    index: u32,
    out: *mut TpdfCoverage,
) -> TpdfStatus {
    let signature = match unsafe { signature_at(signatures, index) } {
        Ok(signature) => signature,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfCoverage::of(&signature.coverage);
    TpdfStatus::Ok
}

/// Whether the signature covers every byte of the file it was read from.
///
/// Zero for a null handle and for an index past the end, which is the same
/// answer a caller gets for a signature that covers only a revision — read
/// [`tpdf_signatures_count`] first if the difference matters.
///
/// # Safety
///
/// `signatures` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_covers_whole_file(
    signatures: *const TpdfSignatures,
    index: u32,
) -> c_int {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => c_int::from(signature.covers_whole_file()),
        Err(_) => 0,
    }
}

/// Whether this is a usage-rights signature (12.8.4).
///
/// One of these grants a reader capabilities and makes **no claim about the
/// document's content**. A host that reports it as "this document is signed"
/// is reporting something the file does not say.
///
/// # Safety
///
/// `signatures` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_is_usage_rights(
    signatures: *const TpdfSignatures,
    index: u32,
) -> c_int {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => c_int::from(signature.is_usage_rights()),
        Err(_) => 0,
    }
}

/// The `/DocMDP` certification level (12.8.2.2): 1, 2 or 3, or **0 for none**.
///
/// Zero is also what a null handle and an index past the end return, and it
/// is the honest value for both: an ordinary approval signature certifies
/// nothing, and so does a signature that is not there. Zero is additionally
/// what a `/P` value 12.8.2.2 does not define reads as, because the facade
/// reports that as no certification rather than rounding it to the strictest.
///
/// # Safety
///
/// `signatures` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_certification_level(
    signatures: *const TpdfSignatures,
    index: u32,
) -> u32 {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => signature
            .certification
            .map_or(0, |certification| u32::from(certification.level())),
        Err(_) => 0,
    }
}

/// How many spans `/ByteRange` names.
///
/// Zero when `/ByteRange` was missing or unreadable, which is also what a null
/// handle returns. 12.8.1 fixes the count at two spans for a well-formed
/// signature; anything else is why [`tpdf_signature_coverage`] exists.
///
/// # Safety
///
/// `signatures` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_span_count(
    signatures: *const TpdfSignatures,
    index: u32,
) -> u32 {
    match unsafe { signature_at(signatures, index) } {
        Ok(signature) => count(signature.spans.len()),
        Err(_) => 0,
    }
}

/// One span's start and length, as offsets into the file.
///
/// A span past the end of [`tpdf_signature_span_count`] is
/// [`TpdfStatus::BadArgument`] rather than `NoSuchSignature`: the signature is
/// there, the span index is not.
///
/// # Safety
///
/// `signatures` must be a live handle or null; the out pointers may each be
/// null, in which case that half of the answer is dropped.
#[no_mangle]
pub unsafe extern "C" fn tpdf_signature_span(
    signatures: *const TpdfSignatures,
    index: u32,
    span: u32,
    out_start: *mut u64,
    out_length: *mut u64,
) -> TpdfStatus {
    let signature = match unsafe { signature_at(signatures, index) } {
        Ok(signature) => signature,
        Err(status) => return status,
    };
    let Some(span) = signature.spans.get(span as usize) else {
        set_error("no such span");
        return TpdfStatus::BadArgument;
    };
    if let Some(slot) = unsafe { out_start.as_mut() } {
        *slot = span.start;
    }
    if let Some(slot) = unsafe { out_length.as_mut() } {
        *slot = span.end.saturating_sub(span.start);
    }
    TpdfStatus::Ok
}

// ---- trust anchors and verdicts --------------------------------------------

/// An empty set of trust anchors, or null if the allocation could not be made.
///
/// A handle rather than an array of pointers and lengths, for two reasons.
/// This file is handle-based throughout, and `TrustAnchors` is a facade type,
/// so a handle is a projection of something that exists rather than a shape
/// invented here. And [`tpdf_trust_anchors_add`] **refuses bytes that are not
/// a certificate**, per anchor, at the moment they are offered — a single call
/// taking an array could only report that as one aggregate failure, which
/// would leave the caller unable to say which of its certificates was the bad
/// one.
///
/// Empty is the honest default and the only one: an anchor set the engine
/// filled in would be the engine deciding what the caller trusts. A
/// verification run with no anchors reports [`TpdfChain::NoAnchors`].
///
/// # Safety
///
/// The result must be released with [`tpdf_trust_anchors_free`].
#[no_mangle]
pub unsafe extern "C" fn tpdf_trust_anchors_new() -> *mut TpdfTrustAnchors {
    Box::into_raw(Box::new(TpdfTrustAnchors {
        inner: TrustAnchors::new(),
    }))
}

/// Adds one DER certificate the caller trusts.
///
/// The bytes are copied, so the caller may free theirs immediately. Bytes that
/// do not parse as a certificate are [`TpdfStatus::BadArgument`] with the
/// parser's reason in [`tpdf_last_error_message`], and **are not kept** —
/// silently ignoring them would leave a caller believing it had supplied an
/// anchor it had not.
///
/// # Safety
///
/// `anchors` must be a live handle and `der` must point to at least `len`
/// readable bytes.
#[no_mangle]
pub unsafe extern "C" fn tpdf_trust_anchors_add(
    anchors: *mut TpdfTrustAnchors,
    der: *const u8,
    len: usize,
) -> TpdfStatus {
    let (Some(anchors), false) = (unsafe { anchors.as_mut() }, der.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let bytes = unsafe { std::slice::from_raw_parts(der, len) }.to_vec();
    match anchors.inner.add(bytes) {
        Ok(()) => TpdfStatus::Ok,
        Err(reason) => {
            set_error(&reason);
            TpdfStatus::BadArgument
        }
    }
}

/// How many anchors the handle holds, or zero if it is null.
///
/// # Safety
///
/// `anchors` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_trust_anchors_count(anchors: *const TpdfTrustAnchors) -> u32 {
    unsafe { anchors.as_ref() }.map_or(0, |a| count(a.inner.len()))
}

/// Frees a trust anchor handle. Null is accepted and does nothing.
///
/// # Safety
///
/// `anchors` must have come from [`tpdf_trust_anchors_new`] and must not be
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_trust_anchors_free(anchors: *mut TpdfTrustAnchors) {
    if !anchors.is_null() {
        drop(unsafe { Box::from_raw(anchors) });
    }
}

/// What every signature in the document turns out to prove (12.8).
///
/// One verdict per signature, in the order [`tpdf_document_signatures`]
/// returns them, so index *i* of one names the same signature as index *i* of
/// the other.
///
/// `anchors` is required and **null is refused**, not read as "no anchors":
/// an empty [`tpdf_trust_anchors_new`] handle is how a caller says it trusts
/// nothing, and reading a null pointer as that would be this crate supplying a
/// default the facade does not have (ruling 11).
///
/// `at` is the instant to judge certificate validity at, in seconds since the
/// Unix epoch, and it is read **only** when `judge_validity` is non-zero. The
/// facade takes an `Option<i64>` there and `Option` has no C spelling, so the
/// flag carries the `None`. With the flag clear, validity windows are reported
/// and nothing is judged — which is the facade's own default, because ruling 4
/// bans a clock from this engine and "expired" is a claim about now.
///
/// Two things the facade has here do not cross. A verdict repeats the
/// signature's coverage and this does not project it twice —
/// [`tpdf_signature_coverage`] at the same index is the same answer. And
/// `Verdict::is_trusted` is not here: it is a convenience the facade itself
/// calls deliberately conservative, it discards every distinction the fields
/// make, and one `int` saying `1` is exactly the shape this surface exists to
/// refuse.
///
/// The caller frees the result with [`tpdf_verdicts_free`].
///
/// # Safety
///
/// `doc` and `anchors` must be live handles and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_verify_signatures(
    doc: *const TpdfDocument,
    anchors: *const TpdfTrustAnchors,
    judge_validity: c_int,
    at: i64,
    out: *mut *mut TpdfVerdicts,
) -> TpdfStatus {
    let (Some(doc), Some(anchors), false) = (
        unsafe { doc.as_ref() },
        unsafe { anchors.as_ref() },
        out.is_null(),
    ) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let at = (judge_validity != 0).then_some(at);
    let handle = Box::new(TpdfVerdicts {
        inner: doc.inner.verify_signatures(&anchors.inner, at),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// How many verdicts the handle holds, or zero if it is null.
///
/// # Safety
///
/// `verdicts` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdicts_count(verdicts: *const TpdfVerdicts) -> u32 {
    unsafe { verdicts.as_ref() }.map_or(0, |v| count(v.inner.len()))
}

/// Frees a verdicts handle. Null is accepted and does nothing.
///
/// # Safety
///
/// `verdicts` must have come from [`tpdf_document_verify_signatures`] and must
/// not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdicts_free(verdicts: *mut TpdfVerdicts) {
    if !verdicts.is_null() {
        drop(unsafe { Box::from_raw(verdicts) });
    }
}

/// Whether the CMS blob could be read.
///
/// # Safety
///
/// `verdicts` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_cms_state(
    verdicts: *const TpdfVerdicts,
    index: u32,
    out: *mut TpdfCmsState,
) -> TpdfStatus {
    let verdict = match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => verdict,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfCmsState::of(&verdict.cms);
    TpdfStatus::Ok
}

/// Whether the covered bytes still hash to what was signed.
///
/// # Safety
///
/// `verdicts` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_document_digest(
    verdicts: *const TpdfVerdicts,
    index: u32,
    out: *mut TpdfDocumentDigest,
) -> TpdfStatus {
    let verdict = match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => verdict,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfDocumentDigest::of(&verdict.document_digest);
    TpdfStatus::Ok
}

/// Whether the signature verifies against the signer's own key.
///
/// # Safety
///
/// `verdicts` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_signature_check(
    verdicts: *const TpdfVerdicts,
    index: u32,
    out: *mut TpdfSignatureCheck,
) -> TpdfStatus {
    let verdict = match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => verdict,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfSignatureCheck::of(&verdict.signature);
    TpdfStatus::Ok
}

/// How far the certificate chain reached.
///
/// # Safety
///
/// `verdicts` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_chain(
    verdicts: *const TpdfVerdicts,
    index: u32,
    out: *mut TpdfChain,
) -> TpdfStatus {
    let verdict = match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => verdict,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfChain::of(&verdict.chain);
    TpdfStatus::Ok
}

/// The signer's certificate subject, rendered per RFC 4514.
///
/// Null on `Ok` when the verdict names no signer — the certificate was not in
/// the blob, or there was no readable CMS at all. Freed with
/// [`tpdf_string_free`]. This is what the *certificate* says; `/Name`
/// ([`tpdf_signature_name`]) is what the signer typed.
///
/// # Safety
///
/// `verdicts` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_signer_subject(
    verdicts: *const TpdfVerdicts,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => unsafe {
            hand_over_string(out, verdict.signer.as_ref().map(|s| s.subject.as_str()))
        },
        Err(status) => status,
    }
}

/// The signer's certificate issuer, rendered per RFC 4514.
///
/// Null on `Ok` when the verdict names no signer. Freed with
/// [`tpdf_string_free`].
///
/// # Safety
///
/// `verdicts` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_signer_issuer(
    verdicts: *const TpdfVerdicts,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => unsafe {
            hand_over_string(out, verdict.signer.as_ref().map(|s| s.issuer.as_str()))
        },
        Err(status) => status,
    }
}

/// The signer certificate's `notBefore` and `notAfter`, as seconds since the
/// Unix epoch.
///
/// Returns 1 when a signer was named and the window was written, and 0 for a
/// null handle, an index past the end, or a verdict with no signer — so this
/// doubles as "is there a signer certificate at all". It is a window, not a
/// judgement: whether the instant a caller cares about falls inside it is the
/// caller's to decide, unless it asked
/// [`tpdf_document_verify_signatures`] to judge, in which case a miss is
/// [`TpdfWeakness::OutsideValidity`].
///
/// # Safety
///
/// `verdicts` must be a live handle or null; either out pointer may be null,
/// in which case that half of the window is dropped.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_signer_validity(
    verdicts: *const TpdfVerdicts,
    index: u32,
    out_not_before: *mut i64,
    out_not_after: *mut i64,
) -> c_int {
    let Ok(verdict) = (unsafe { verdict_at(verdicts, index) }) else {
        return 0;
    };
    let Some(signer) = verdict.signer.as_ref() else {
        return 0;
    };
    if let Some(slot) = unsafe { out_not_before.as_mut() } {
        *slot = signer.validity.0;
    }
    if let Some(slot) = unsafe { out_not_after.as_mut() } {
        *slot = signer.validity.1;
    }
    1
}

/// How many weaknesses the verdict names, or zero if the handle is null or the
/// index is past the end.
///
/// # Safety
///
/// `verdicts` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_weakness_count(
    verdicts: *const TpdfVerdicts,
    index: u32,
) -> u32 {
    match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => count(verdict.weaknesses.len()),
        Err(_) => 0,
    }
}

/// One weakness by position.
///
/// A `weakness` past [`tpdf_verdict_weakness_count`] is
/// [`TpdfStatus::BadArgument`] rather than `NoSuchSignature`: the verdict is
/// there, the weakness index is not.
///
/// # Safety
///
/// `verdicts` must be a live handle or null, and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_verdict_weakness(
    verdicts: *const TpdfVerdicts,
    index: u32,
    weakness: u32,
    out: *mut TpdfWeakness,
) -> TpdfStatus {
    let verdict = match unsafe { verdict_at(verdicts, index) } {
        Ok(verdict) => verdict,
        Err(status) => return status,
    };
    let Some(found) = verdict.weaknesses.get(weakness as usize) else {
        set_error("no such weakness");
        return TpdfStatus::BadArgument;
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfWeakness::of(found);
    TpdfStatus::Ok
}

// ---- writing: the editor (gap 32 milestone 2) -----------------------------
//
// `docs/design/bindings-write.md`. The distance between the read surface above
// and this one was never capability -- `DocumentEditor` and `DocumentBuilder`
// have been on the facade since gap 26 -- but *shape*: their transactional and
// page-building APIs take closures, and a closure does not cross this
// boundary. Ruling 11 says the facade grows the closure-free equivalents
// first, so it did (`checkpoint`/`restore`, `begin_page`/`push_page`), and
// everything below is a mechanical wrapping of a Rust API that already exists.
//
// **Threading.** A `TpdfDocument` may be used from any thread because a
// `Document` is `Send + Sync` and every read borrows. An editor and a builder
// are *mutable state*: the calls below take `&mut`, so two threads calling
// into one handle at once is the same data race it would be in Rust, and the
// C ABI cannot stop it. One handle per thread, or the caller's own lock.
// Handles remain freeable from any thread.

/// An editor over an open document. Opaque to callers.
///
/// Independent of the [`TpdfDocument`] it came from: `Document::editor()`
/// clones the shared `Arc<CosDocument>`, so freeing the document first is
/// legal and the .NET `SafeHandle`s need no parent-child keep-alive. That is a
/// property of the facade rather than a promise this crate keeps, which is why
/// there is a test called
/// `an_editor_outlives_the_document_it_came_from`.
pub struct TpdfEditor {
    inner: DocumentEditor,
}

/// An editor's state, taken as a value. Opaque to callers.
///
/// The closure-free half of `DocumentEditor::transaction`. There is no "open"
/// state here to leave dangling: taking one changes nothing, freeing one
/// commits nothing because nothing was pending, and restoring is idempotent.
pub struct TpdfCheckpoint {
    inner: EditCheckpoint,
}

/// Bytes the engine produced. Opaque to callers.
///
/// [`tpdf_buffer_data`] borrows until [`tpdf_buffer_free`], exactly the
/// [`TpdfBitmap`] arrangement and for the same reason: the engine allocates
/// and the matching free releases.
pub struct TpdfBuffer {
    inner: Vec<u8>,
}

/// Widgets a fill wrote a value for and could not draw. Opaque to callers.
///
/// **The fourth outcome, and it must not flatten into failure.** A field that
/// appears on two pages has two widgets; ruling 2 degrades rather than failing,
/// so the value is written and the widgets that can be drawn are drawn. What
/// ruling 2 does not licence is silence, and ruling 10 says the degradation
/// names its object -- so [`tpdf_editor_fill_field`] returns
/// [`TpdfStatus::Ok`] *and* a report the caller iterates. An empty report is
/// the ordinary case and is not an error; a non-empty one is a document that
/// looks filled and is not wholly drawn.
pub struct TpdfFillReport {
    inner: Vec<SkippedWidget>,
}

/// What is wrong with a widget an appearance could not be written for.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfWidgetDefect {
    /// 12.5.2 Table 164: `/Rect` is required for every annotation and this one
    /// has none, or none that is a usable rectangle. There is no box to lay
    /// the value out in and nowhere on the page to draw it.
    RectMissing = 0,
}

impl TpdfWidgetDefect {
    /// The C spelling of a facade widget defect.
    fn of(defect: WidgetDefect) -> TpdfWidgetDefect {
        match defect {
            WidgetDefect::RectMissing => TpdfWidgetDefect::RectMissing,
        }
    }
}

/// Which shape of output a save produces (7.5.6).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfWriteMode {
    /// Emit every object afresh, renumbering from one.
    Rewrite = 0,
    /// Append changed objects to the original bytes, so the original survives
    /// as a prefix and a signature over it still covers what it covered
    /// (12.8.1).
    Incremental = 1,
}

impl From<TpdfWriteMode> for WriteMode {
    fn from(value: TpdfWriteMode) -> Self {
        match value {
            TpdfWriteMode::Rewrite => WriteMode::Rewrite,
            TpdfWriteMode::Incremental => WriteMode::Incremental,
        }
    }
}

/// How to encrypt on save, as C sees `Encryption`.
///
/// **No entropy default, ever.** `entropy` is 48 caller-supplied bytes -- the
/// 32-byte file key and two 8-byte salts -- because this engine has no opinion
/// about where randomness comes from and `wasm32-unknown-unknown` has no
/// source of it at all. A binding that invented one would violate ruling 11
/// and hide the single input that makes encrypted output non-reproducible.
/// Passing predictable bytes produces a predictably weak document, which is
/// the caller's decision and an honest one.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfEncryption {
    /// The password a reader needs to open the document, as a null-terminated
    /// UTF-8 string. Null or empty means none.
    pub user_password: *const c_char,
    /// The password that lifts the document's restrictions. Null or empty
    /// means none.
    pub owner_password: *const c_char,
    /// The permission bits, as `/P` stores them.
    pub permissions: i32,
    /// Exactly [`TPDF_ENTROPY_LEN`] bytes of randomness.
    pub entropy: *const u8,
    /// How many bytes `entropy` points at. Anything but
    /// [`TPDF_ENTROPY_LEN`] is [`TpdfStatus::BadArgument`]: a short buffer
    /// read to 48 would encrypt with whatever followed it in the caller's
    /// address space, which is the worst possible way to be "random".
    pub entropy_len: usize,
}

/// The number of entropy bytes an encrypted save requires: 32 for the file key
/// and 8 for each of the two salts.
pub const TPDF_ENTROPY_LEN: usize = 48;

/// Options for writing, as C sees `WriteOptions`.
///
/// Field for field, with the two `bool`s and the version pair widened to
/// integers so the layout has no packing surprises for a hand-written
/// P/Invoke. Fill it with [`tpdf_write_options_init`] and change what you
/// mean, rather than zeroing it: a zeroed struct is a *rewrite* at version
/// 0.0, which is not the facade's default and not a version any reader knows.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfWriteOptions {
    /// Rewrite or incremental.
    pub mode: TpdfWriteMode,
    /// Lay the file out for the first page to arrive first (Annex F). A
    /// request rather than a guarantee: it is quietly dropped for an
    /// incremental update and for a document with no catalog or no pages,
    /// because a file that claims `/Linearized` and is not is worse than an
    /// ordinary one.
    pub linearize: c_int,
    /// The major PDF version to declare in the header, on a rewrite.
    pub version_major: u32,
    /// The minor version. Above 255 either is [`TpdfStatus::BadArgument`];
    /// the facade holds them in a byte each.
    pub version_minor: u32,
    /// Pack eligible objects into object streams (7.5.7).
    pub object_streams: c_int,
    /// Compress content streams the caller has not already encoded.
    pub compress: c_int,
    /// Drop objects nothing reaches from the trailer, on a rewrite.
    pub garbage_collect: c_int,
    /// Encryption, or null for none. Borrowed for the duration of the call
    /// only; the passwords and entropy are copied out of it before it returns.
    pub encryption: *const TpdfEncryption,
}

/// Fills a [`TpdfWriteOptions`] with the facade's own defaults.
///
/// `WriteOptions::default()`, projected -- rewrite, not linearized, version
/// 1.7, no object streams, compressed, no encryption, no garbage collection.
/// A C caller should not have to know those, and a C caller who guesses them
/// wrong writes a different file than a Rust caller with the same intent,
/// which is the whole failure the write-parity suite exists to catch.
///
/// # Safety
///
/// `out` must be a valid pointer to a `TpdfWriteOptions` to write.
#[no_mangle]
pub unsafe extern "C" fn tpdf_write_options_init(out: *mut TpdfWriteOptions) -> TpdfStatus {
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let defaults = WriteOptions::default();
    *slot = TpdfWriteOptions {
        mode: match defaults.mode {
            WriteMode::Rewrite => TpdfWriteMode::Rewrite,
            WriteMode::Incremental => TpdfWriteMode::Incremental,
        },
        linearize: c_int::from(defaults.linearize),
        version_major: u32::from(defaults.version.0),
        version_minor: u32::from(defaults.version.1),
        object_streams: c_int::from(defaults.object_streams),
        compress: c_int::from(defaults.compress),
        garbage_collect: c_int::from(defaults.garbage_collect),
        encryption: ptr::null(),
    };
    TpdfStatus::Ok
}

/// A C string as an owned `String`, or a refusal.
///
/// Null and non-UTF-8 are both [`TpdfStatus::BadArgument`], because the facade
/// takes `&str` and there is no lossy answer that is not a guess about what
/// the caller meant.
unsafe fn required_str(value: *const c_char, what: &str) -> Result<String, TpdfStatus> {
    if value.is_null() {
        set_error(&format!("{what} is null"));
        return Err(TpdfStatus::BadArgument);
    }
    match unsafe { CStr::from_ptr(value) }.to_str() {
        Ok(text) => Ok(text.to_string()),
        Err(_) => {
            set_error(&format!("{what} is not valid UTF-8"));
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// The same, for a pointer that is allowed to be absent.
unsafe fn optional_str(value: *const c_char, what: &str) -> Result<String, TpdfStatus> {
    if value.is_null() {
        return Ok(String::new());
    }
    unsafe { required_str(value, what) }
}

/// The facade's `WriteOptions` from the C struct, or a refusal.
unsafe fn write_options(options: *const TpdfWriteOptions) -> Result<WriteOptions, TpdfStatus> {
    let Some(options) = (unsafe { options.as_ref() }) else {
        set_error("null write options");
        return Err(TpdfStatus::BadArgument);
    };

    let (Ok(major), Ok(minor)) = (
        u8::try_from(options.version_major),
        u8::try_from(options.version_minor),
    ) else {
        set_error("a PDF version component does not fit in a byte");
        return Err(TpdfStatus::BadArgument);
    };

    let encryption = match unsafe { options.encryption.as_ref() } {
        None => None,
        Some(encryption) => {
            if encryption.entropy.is_null() || encryption.entropy_len != TPDF_ENTROPY_LEN {
                set_error(&format!(
                    "encryption needs exactly {TPDF_ENTROPY_LEN} bytes of caller-supplied entropy"
                ));
                return Err(TpdfStatus::BadArgument);
            }
            // Safety: the length was just checked against the one value this
            // field is allowed to hold, and the pointer against null.
            let bytes = unsafe { std::slice::from_raw_parts(encryption.entropy, TPDF_ENTROPY_LEN) };
            let mut entropy = [0u8; TPDF_ENTROPY_LEN];
            entropy.copy_from_slice(bytes);
            Some(Encryption {
                user_password: unsafe { optional_str(encryption.user_password, "user password") }?,
                owner_password: unsafe {
                    optional_str(encryption.owner_password, "owner password")
                }?,
                permissions: encryption.permissions,
                entropy,
            })
        }
    };

    Ok(WriteOptions {
        mode: options.mode.into(),
        linearize: options.linearize != 0,
        version: (major, minor),
        object_streams: options.object_streams != 0,
        compress: options.compress != 0,
        garbage_collect: options.garbage_collect != 0,
        encryption,
    })
}

/// The status a `FillError` crosses as.
fn fill_status(error: FillError) -> TpdfStatus {
    match error {
        FillError::NoSuchField => TpdfStatus::NoSuchField,
        FillError::ValueRefused => TpdfStatus::ValueRefused,
        FillError::FieldUnreadable => TpdfStatus::FieldUnreadable,
    }
}

/// A refused edit, with the provenance ruling 10 asks for.
///
/// The facade said `false` or `None` and named no reason; what this can still
/// say is which call refused and what it was given, which is the difference
/// between a debuggable failure and a number.
fn refused(call: &str, detail: &str) -> TpdfStatus {
    set_error(&format!("{call} refused: {detail}"));
    TpdfStatus::EditRefused
}

/// Opens an editor over a document.
///
/// The editor is independent of `doc`: freeing the document first is legal,
/// because the editor holds its own reference to the shared object store.
/// Edits are held in the editor until [`tpdf_editor_save`] and never touch
/// the document handle.
///
/// The caller frees the result with [`tpdf_editor_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_editor(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfEditor,
) -> TpdfStatus {
    let (Some(doc), false) = (unsafe { doc.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let handle = Box::new(TpdfEditor {
        inner: doc.inner.editor(),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// Frees an editor. Null is accepted and does nothing.
///
/// Pending edits are discarded, because an editor holds them and nothing else
/// does. Nothing is written to any document.
///
/// # Safety
///
/// `editor` must have come from [`tpdf_document_editor`] and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_free(editor: *mut TpdfEditor) {
    if !editor.is_null() {
        drop(unsafe { Box::from_raw(editor) });
    }
}

/// Whether anything has been changed, or zero if the handle is null.
///
/// # Safety
///
/// `editor` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_is_dirty(editor: *const TpdfEditor) -> c_int {
    match unsafe { editor.as_ref() } {
        Some(editor) => c_int::from(editor.inner.is_dirty()),
        None => 0,
    }
}

/// How many pages the document has *as this editor sees it*, or zero if the
/// handle is null.
///
/// Not the same as [`tpdf_document_page_count`] once a page has been inserted
/// or deleted here, which is exactly why it exists: a caller checking an index
/// before an edit must check it against the edit's own view.
///
/// # Safety
///
/// `editor` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_page_count(editor: *const TpdfEditor) -> u32 {
    match unsafe { editor.as_ref() } {
        Some(editor) => count(editor.inner.page_refs().len()),
        None => 0,
    }
}

/// Removes a page.
///
/// # Safety
///
/// `editor` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_delete_page(
    editor: *mut TpdfEditor,
    index: u32,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    if editor.inner.delete_page(index) {
        TpdfStatus::Ok
    } else {
        refused("delete_page", &format!("index {index}"))
    }
}

/// Moves a page to a new position.
///
/// # Safety
///
/// `editor` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_move_page(
    editor: *mut TpdfEditor,
    from: u32,
    to: u32,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    if editor.inner.move_page(from, to) {
        TpdfStatus::Ok
    } else {
        refused("move_page", &format!("from {from} to {to}"))
    }
}

/// Rotates a page by a quarter-turn multiple, relative to its current
/// rotation.
///
/// # Safety
///
/// `editor` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_rotate_page(
    editor: *mut TpdfEditor,
    index: u32,
    degrees: i64,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    if editor.inner.rotate_page(index, degrees) {
        TpdfStatus::Ok
    } else {
        refused("rotate_page", &format!("index {index}, {degrees} degrees"))
    }
}

/// Inserts a blank page of the given size at `index`, which may equal the page
/// count to append.
///
/// # Safety
///
/// `editor` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_insert_page(
    editor: *mut TpdfEditor,
    index: u32,
    width: f64,
    height: f64,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    if editor.inner.insert_page(index, width, height).is_some() {
        TpdfStatus::Ok
    } else {
        refused(
            "insert_page",
            &format!("index {index}, {width} by {height}"),
        )
    }
}

/// Sets a page's `/CropBox` (14.11.2), in the page's own user space.
///
/// # Safety
///
/// `editor` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_crop_box(
    editor: *mut TpdfEditor,
    index: u32,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    if editor.inner.set_crop_box(index, x0, y0, x1, y1) {
        TpdfStatus::Ok
    } else {
        refused(
            "set_crop_box",
            &format!("index {index}, [{x0} {y0} {x1} {y1}]"),
        )
    }
}

/// Appends operators to a page's content stream.
///
/// # Safety
///
/// `editor` must be a live handle and `operators` must point to at least
/// `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_append_content(
    editor: *mut TpdfEditor,
    page: u32,
    operators: *const u8,
    len: usize,
) -> TpdfStatus {
    let (Some(editor), false) = (unsafe { editor.as_mut() }, operators.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let operators = unsafe { std::slice::from_raw_parts(operators, len) };
    if editor.inner.append_content(page, operators) {
        TpdfStatus::Ok
    } else {
        refused("append_content", &format!("page {page}"))
    }
}

/// How many form fields the document has, or zero if the handle is null.
///
/// # Safety
///
/// `editor` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_field_count(editor: *const TpdfEditor) -> u32 {
    match unsafe { editor.as_ref() } {
        Some(editor) => count(editor.inner.fields().len()),
        None => 0,
    }
}

/// A field's fully qualified name (12.7.3.2), as a null-terminated UTF-8
/// string the caller frees with [`tpdf_string_free`].
///
/// # Safety
///
/// `editor` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_field_name(
    editor: *const TpdfEditor,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_ref() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    let fields = editor.inner.fields();
    let Some(field) = fields.get(index as usize) else {
        set_error("no such field");
        return TpdfStatus::NoSuchField;
    };
    unsafe { hand_over_string(out, Some(&field.name)) }
}

/// A field's current value as text, as a null-terminated UTF-8 string the
/// caller frees with [`tpdf_string_free`].
///
/// A field with no value yields an empty string rather than null: "the field
/// is empty" is an answer, and the absent answer on this surface is reserved
/// for things a document genuinely does not say.
///
/// # Safety
///
/// `editor` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_field_value(
    editor: *const TpdfEditor,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_ref() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    let fields = editor.inner.fields();
    let Some(field) = fields.get(index as usize) else {
        set_error("no such field");
        return TpdfStatus::NoSuchField;
    };
    let text = field.value.as_text();
    unsafe { hand_over_string(out, Some(&text)) }
}

/// Fills a text or choice field, reporting the widgets it could not draw.
///
/// Three outcomes and not two:
///
/// - a non-`Ok` status -- **nothing was written at all**, and the status says
///   why ([`TpdfStatus::NoSuchField`], [`TpdfStatus::ValueRefused`],
///   [`TpdfStatus::FieldUnreadable`]);
/// - `Ok` with a report of zero entries -- `/V` was written and every widget's
///   appearance was regenerated;
/// - `Ok` with a report of some entries -- `/V` was written, and those widgets
///   were left showing whatever they were showing before, because 12.5.2's
///   required `/Rect` is missing from them.
///
/// The third is the one a `bool` cannot express and the one this exists for.
/// A caller that wants all-or-nothing checks the count is zero and restores a
/// checkpoint if it is not, which is what
/// `DocumentEditor::set_field_value` does in Rust.
///
/// `out_report` is always written on `Ok`, never null on `Ok`, and is freed
/// with [`tpdf_fill_report_free`]. A caller that does not want it may pass
/// null for `out_report`, in which case the report is dropped -- but a caller
/// who does that has chosen not to know, which is the silence ruling 10 is
/// against.
///
/// # Safety
///
/// `editor` must be a live handle, `name` and `value` null-terminated UTF-8,
/// and `out_report` a valid pointer or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_fill_field(
    editor: *mut TpdfEditor,
    name: *const c_char,
    value: *const c_char,
    out_report: *mut *mut TpdfFillReport,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    let name = match unsafe { required_str(name, "field name") } {
        Ok(name) => name,
        Err(status) => return status,
    };
    let value = match unsafe { required_str(value, "field value") } {
        Ok(value) => value,
        Err(status) => return status,
    };

    match editor.inner.fill_field(&name, &value) {
        Ok(skipped) => {
            if let Some(slot) = unsafe { out_report.as_mut() } {
                *slot = Box::into_raw(Box::new(TpdfFillReport { inner: skipped }));
            }
            TpdfStatus::Ok
        }
        Err(error) => {
            set_error(&format!("{name}: {error}"));
            fill_status(error)
        }
    }
}

/// Ticks or clears a checkbox by its fully qualified name.
///
/// # Safety
///
/// `editor` must be a live handle and `name` null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_checkbox(
    editor: *mut TpdfEditor,
    name: *const c_char,
    on: c_int,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    let name = match unsafe { required_str(name, "field name") } {
        Ok(name) => name,
        Err(status) => return status,
    };
    if editor.inner.set_checkbox(&name, on != 0) {
        TpdfStatus::Ok
    } else {
        refused("set_checkbox", &format!("field {name:?}"))
    }
}

/// Selects one option of a radio group (12.7.4.2).
///
/// # Safety
///
/// `editor` must be a live handle, and `name` and `option` null-terminated
/// UTF-8.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_select_radio(
    editor: *mut TpdfEditor,
    name: *const c_char,
    option: *const c_char,
) -> TpdfStatus {
    let Some(editor) = (unsafe { editor.as_mut() }) else {
        set_error("null editor");
        return TpdfStatus::BadArgument;
    };
    let name = match unsafe { required_str(name, "field name") } {
        Ok(name) => name,
        Err(status) => return status,
    };
    let option = match unsafe { required_str(option, "option") } {
        Ok(option) => option,
        Err(status) => return status,
    };
    if editor.inner.select_radio(&name, &option) {
        TpdfStatus::Ok
    } else {
        refused(
            "select_radio",
            &format!("field {name:?}, option {option:?}"),
        )
    }
}

/// Takes this editor's state as a value, for [`tpdf_editor_restore`] to put
/// back.
///
/// The closure-free half of `DocumentEditor::transaction`, which is
/// checkpoint -> body -> restore on failure. Taking one changes nothing about
/// the editor, and holding one across any number of further edits is fine: it
/// is a copy, not a borrow, and the copy is of the *edits* rather than of the
/// document.
///
/// The caller frees the result with [`tpdf_checkpoint_free`]. Freeing one
/// commits nothing, because nothing was pending.
///
/// # Safety
///
/// `editor` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_checkpoint(
    editor: *const TpdfEditor,
    out: *mut *mut TpdfCheckpoint,
) -> TpdfStatus {
    let (Some(editor), false) = (unsafe { editor.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let handle = Box::new(TpdfCheckpoint {
        inner: editor.inner.checkpoint(),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// Puts an editor back to what a checkpoint recorded.
///
/// Restores objects written, objects deleted, the page order and the
/// object-number counter -- the same four a rolled-back
/// `DocumentEditor::transaction` restores, because it is the same function.
///
/// **Idempotent**, which is what makes it safe here: a host language's
/// `finally` may run after its own `catch` has already restored, and
/// restoring twice is restoring once.
///
/// The checkpoint is borrowed, not consumed, so one checkpoint can undo
/// several attempts and the handle stays the caller's to free.
///
/// # Safety
///
/// `editor` and `checkpoint` must be live handles.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_restore(
    editor: *mut TpdfEditor,
    checkpoint: *const TpdfCheckpoint,
) -> TpdfStatus {
    let (Some(editor), Some(checkpoint)) =
        (unsafe { editor.as_mut() }, unsafe { checkpoint.as_ref() })
    else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    editor.inner.restore(&checkpoint.inner);
    TpdfStatus::Ok
}

/// Frees a checkpoint. Null is accepted and does nothing.
///
/// # Safety
///
/// `checkpoint` must have come from [`tpdf_editor_checkpoint`] and must not be
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_checkpoint_free(checkpoint: *mut TpdfCheckpoint) {
    if !checkpoint.is_null() {
        drop(unsafe { Box::from_raw(checkpoint) });
    }
}

/// Saves the edited document.
///
/// The caller frees the result with [`tpdf_buffer_free`].
///
/// An incremental save (7.5.6) appends to the original bytes, so the original
/// survives as a prefix and the property signatures depend on (12.8.1) holds.
/// A rewrite emits everything afresh.
///
/// # Safety
///
/// `editor` must be a live handle, `options` a valid pointer, and `out` a
/// valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_save(
    editor: *const TpdfEditor,
    options: *const TpdfWriteOptions,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    let (Some(editor), false) = (unsafe { editor.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let options = match unsafe { write_options(options) } {
        Ok(options) => options,
        Err(status) => return status,
    };
    let bytes = editor.inner.save(&options);
    unsafe { *out = Box::into_raw(Box::new(TpdfBuffer { inner: bytes })) };
    TpdfStatus::Ok
}

/// A borrowed pointer to a buffer's bytes, with its length.
///
/// The pointer is valid until the buffer is freed. It is **not** the caller's
/// to release, and this is the [`tpdf_bitmap_data`] arrangement exactly.
///
/// # Safety
///
/// `buffer` must be a live handle or null; `out_len` may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_buffer_data(
    buffer: *const TpdfBuffer,
    out_len: *mut usize,
) -> *const u8 {
    let Some(buffer) = (unsafe { buffer.as_ref() }) else {
        if let Some(slot) = unsafe { out_len.as_mut() } {
            *slot = 0;
        }
        return ptr::null();
    };
    if let Some(slot) = unsafe { out_len.as_mut() } {
        *slot = buffer.inner.len();
    }
    buffer.inner.as_ptr()
}

/// A buffer's length in bytes, or zero if the handle is null.
///
/// # Safety
///
/// `buffer` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_buffer_len(buffer: *const TpdfBuffer) -> usize {
    unsafe { buffer.as_ref() }.map_or(0, |b| b.inner.len())
}

/// Frees a buffer. Null is accepted and does nothing.
///
/// # Safety
///
/// `buffer` must have come from a function that says so, and neither it nor
/// any pointer [`tpdf_buffer_data`] returned for it may be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_buffer_free(buffer: *mut TpdfBuffer) {
    if !buffer.is_null() {
        drop(unsafe { Box::from_raw(buffer) });
    }
}

/// How many widgets a fill could not draw. Zero is the ordinary answer and is
/// not an error.
///
/// # Safety
///
/// `report` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_fill_report_count(report: *const TpdfFillReport) -> u32 {
    match unsafe { report.as_ref() } {
        Some(report) => count(report.inner.len()),
        None => 0,
    }
}

/// One entry rendered as text, naming the widget and the defect.
///
/// `"7 0 R: no usable /Rect (12.5.2)"` and the like -- the facade's own
/// `Display`, so the C ABI and Rust say the same sentence about the same
/// document. The caller frees it with [`tpdf_string_free`].
///
/// # Safety
///
/// `report` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_fill_report_message(
    report: *const TpdfFillReport,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let Some(report) = (unsafe { report.as_ref() }) else {
        set_error("null fill report");
        return TpdfStatus::BadArgument;
    };
    let Some(entry) = report.inner.get(index as usize) else {
        set_error("no such fill report entry");
        return TpdfStatus::BadArgument;
    };
    unsafe { hand_over_string(out, Some(&entry.to_string())) }
}

/// One entry's widget, as an object number and generation.
///
/// The `ObjRef` ruling 10 requires a warning to carry, in the two integers it
/// is made of -- so a caller can go and look at the object rather than parse a
/// sentence about it.
///
/// # Safety
///
/// `report` must be a live handle; the out pointers may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_fill_report_widget(
    report: *const TpdfFillReport,
    index: u32,
    out_number: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let Some(report) = (unsafe { report.as_ref() }) else {
        set_error("null fill report");
        return TpdfStatus::BadArgument;
    };
    let Some(entry) = report.inner.get(index as usize) else {
        set_error("no such fill report entry");
        return TpdfStatus::BadArgument;
    };
    if let Some(slot) = unsafe { out_number.as_mut() } {
        *slot = entry.widget.num;
    }
    if let Some(slot) = unsafe { out_generation.as_mut() } {
        *slot = entry.widget.gen;
    }
    TpdfStatus::Ok
}

/// One entry's defect.
///
/// # Safety
///
/// `report` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_fill_report_defect(
    report: *const TpdfFillReport,
    index: u32,
    out: *mut TpdfWidgetDefect,
) -> TpdfStatus {
    let Some(report) = (unsafe { report.as_ref() }) else {
        set_error("null fill report");
        return TpdfStatus::BadArgument;
    };
    let Some(entry) = report.inner.get(index as usize) else {
        set_error("no such fill report entry");
        return TpdfStatus::BadArgument;
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfWidgetDefect::of(entry.reason);
    TpdfStatus::Ok
}

/// Frees a fill report. Null is accepted and does nothing.
///
/// # Safety
///
/// `report` must have come from [`tpdf_editor_fill_field`] and must not be
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_fill_report_free(report: *mut TpdfFillReport) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

// ---- writing: the builder (gap 32 milestone 3) ----------------------------
//
// **Consuming calls are a double-free factory**, and this is the whole design
// problem of this section. `DocumentBuilder::finish(self)` and
// `push_page(page)` consume in Rust; a C caller has a pointer, and a pointer
// that has been "consumed" is one the caller will still pass to free, and may
// still pass to another call.
//
// So a consumable handle boxes an `Option`. The consuming call `take()`s the
// value and records which call took it. The handle stays live and stays the
// caller's to free -- so free remains symmetric with allocation and remains
// null-tolerant, exactly as it is everywhere else on this boundary -- and a
// second call on a spent handle is `TpdfStatus::SpentHandle` with a message
// naming the call that spent it, rather than undefined behaviour.

/// A handle that a call may consume.
///
/// Written once rather than three times, because the failure this prevents is
/// the same failure in each: a `take()` without a record of who took it gives
/// a caller "spent" with no way to find out where.
struct Consumable<T> {
    inner: Option<T>,
    /// The call that took the value, for the error message. Empty until then.
    spent_by: &'static str,
}

impl<T> Consumable<T> {
    fn new(value: T) -> Consumable<T> {
        Consumable {
            inner: Some(value),
            spent_by: "",
        }
    }

    /// The value, still in place, or a refusal naming who took it.
    fn borrow_mut(&mut self, what: &str) -> Result<&mut T, TpdfStatus> {
        if self.inner.is_none() {
            set_error(&format!(
                "{what}: this handle was consumed by {}",
                self.spent_by
            ));
            return Err(TpdfStatus::SpentHandle);
        }
        Ok(self.inner.as_mut().expect("just checked"))
    }

    /// The value, taken. A second take is refused rather than repeated.
    fn take(&mut self, by: &'static str) -> Result<T, TpdfStatus> {
        match self.inner.take() {
            Some(value) => {
                self.spent_by = by;
                Ok(value)
            }
            None => {
                set_error(&format!(
                    "{by}: this handle was already consumed by {}",
                    self.spent_by
                ));
                Err(TpdfStatus::SpentHandle)
            }
        }
    }
}

/// A document being assembled. Opaque to callers.
///
/// Spent by [`tpdf_builder_finish`], which is the only call that consumes it.
pub struct TpdfBuilder {
    inner: Consumable<DocumentBuilder>,
}

/// A page being drawn, owned by the caller until it is pushed. Opaque.
///
/// Spent by [`tpdf_builder_push_page`]. A page begun and never pushed is
/// simply freed and the document is what it would have been -- there is no
/// half-added page and no counter to unwind, which is the property that makes
/// abandoning a handle safe.
pub struct TpdfPageBuilder {
    inner: Consumable<PageBuilder>,
}

/// One outline entry under construction. Opaque.
///
/// Spent by [`tpdf_outline_entry_add_child`] or [`tpdf_builder_set_outline`],
/// each of which takes it into the tree it is joining.
pub struct TpdfOutlineEntry {
    inner: Consumable<OutlineEntry>,
}

/// How a destination positions the page it names (12.3.2.2 Table 151).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfDestKind {
    /// `/XYZ left top zoom`.
    Xyz = 0,
    /// `/Fit`: fit the whole page.
    Fit = 1,
    /// `/FitH top`: fit the width.
    FitH = 2,
    /// `/FitV left`: fit the height.
    FitV = 3,
    /// `/FitR left bottom right top`: fit a rectangle.
    FitR = 4,
    /// `/FitB`: fit the bounding box of the page's contents.
    FitB = 5,
    /// `/FitBH top`: fit the bounding box's width.
    FitBH = 6,
    /// `/FitBV left`: fit the bounding box's height.
    FitBV = 7,
}

/// A destination, as C sees `DestKind`.
///
/// All eight arms cross, rather than a convenient subset, because 12.3.2.2
/// gives them different meanings and a binding that could only write `/Fit`
/// would make every other one unreachable from three languages.
///
/// **A component that is NaN is `null`** -- 12.3.2.2's "retain the current
/// value" -- which is how `Option<f64>` crosses without a parallel presence
/// mask to fall out of step with the values it describes. It is unambiguous
/// because the writer refuses a non-finite number anywhere else: there is no
/// legitimate destination in which NaN means a coordinate. Fields the named
/// kind does not use are ignored, so a caller may leave them at anything.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfDestination {
    /// Which of the eight.
    pub kind: TpdfDestKind,
    /// `/XYZ`'s and `/FitV`'s and `/FitBV`'s left edge.
    pub left: f64,
    /// `/FitR`'s bottom edge.
    pub bottom: f64,
    /// `/FitR`'s right edge.
    pub right: f64,
    /// `/XYZ`'s, `/FitH`'s, `/FitBH`'s and `/FitR`'s top edge.
    pub top: f64,
    /// `/XYZ`'s magnification.
    pub zoom: f64,
}

/// `Some` unless the value is NaN, which is this ABI's spelling of `null`.
fn optional_number(value: f64) -> Option<f64> {
    if value.is_nan() {
        None
    } else {
        Some(value)
    }
}

impl TpdfDestination {
    /// The facade's own destination.
    fn to_facade(self) -> DestKind {
        match self.kind {
            TpdfDestKind::Xyz => DestKind::Xyz {
                left: optional_number(self.left),
                top: optional_number(self.top),
                zoom: optional_number(self.zoom),
            },
            TpdfDestKind::Fit => DestKind::Fit,
            TpdfDestKind::FitH => DestKind::FitH {
                top: optional_number(self.top),
            },
            TpdfDestKind::FitV => DestKind::FitV {
                left: optional_number(self.left),
            },
            TpdfDestKind::FitR => DestKind::FitR {
                left: self.left,
                bottom: self.bottom,
                right: self.right,
                top: self.top,
            },
            TpdfDestKind::FitB => DestKind::FitB,
            TpdfDestKind::FitBH => DestKind::FitBH {
                top: optional_number(self.top),
            },
            TpdfDestKind::FitBV => DestKind::FitBV {
                left: optional_number(self.left),
            },
        }
    }
}

/// Fills a [`TpdfDestination`] with `/Fit`, which is the one that needs no
/// numbers.
///
/// Here for the reason [`tpdf_write_options_init`] is: a caller who zeroes the
/// struct instead gets `/XYZ 0 0 0`, which is a *different* destination that
/// happens to look like a default.
///
/// # Safety
///
/// `out` must be a valid pointer to a `TpdfDestination` to write.
#[no_mangle]
pub unsafe extern "C" fn tpdf_destination_init_fit(out: *mut TpdfDestination) -> TpdfStatus {
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = TpdfDestination {
        kind: TpdfDestKind::Fit,
        left: f64::NAN,
        bottom: f64::NAN,
        right: f64::NAN,
        top: f64::NAN,
        zoom: f64::NAN,
    };
    TpdfStatus::Ok
}

/// Where a link or an outline entry goes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfTargetKind {
    /// A page in this document, positioned as `view` says.
    Page = 0,
    /// A URI, written as the `/URI` action of 12.6.4.7. 7-bit ASCII per that
    /// clause; anything else is refused by the writer rather than mangled.
    Uri = 1,
}

/// A target, as C sees `Target`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfTarget {
    /// Which of the two.
    pub kind: TpdfTargetKind,
    /// The zero-based page index, for [`TpdfTargetKind::Page`].
    pub page_index: u32,
    /// How that page is positioned, for [`TpdfTargetKind::Page`].
    pub view: TpdfDestination,
    /// The URI as a null-terminated UTF-8 string, for
    /// [`TpdfTargetKind::Uri`].
    pub uri: *const c_char,
}

impl TpdfTarget {
    /// The facade's own target, or a refusal.
    ///
    /// # Safety
    ///
    /// `self.uri` must be a null-terminated string when `kind` is `Uri`.
    unsafe fn to_facade(self) -> Result<Target, TpdfStatus> {
        match self.kind {
            TpdfTargetKind::Page => Ok(Target::Page {
                index: self.page_index,
                view: self.view.to_facade(),
            }),
            TpdfTargetKind::Uri => Ok(Target::Uri(unsafe { required_str(self.uri, "uri") }?)),
        }
    }
}

/// Which of `ImageData`'s arms a [`TpdfImage`] carries.
///
/// `ImageData::Compressed` does **not** cross, and that is a decision rather
/// than an omission: it carries a `CompressedImage` with a nested colour space
/// that itself holds a palette slice and a filter with its own parameters, so
/// projecting it is a sub-surface rather than a struct. It exists because a
/// CBZ synthesises every page at open and must not decode each one
/// (`docs/features/*`, gap 29); that is an engine-internal path with no host
/// on the other end. A host with already-compressed bytes has
/// [`TpdfImageKind::Jpeg`], which is the same idea for the one codec hosts
/// actually hold bytes in.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfImageKind {
    /// JPEG bytes, placed **as they are** and never re-encoded, because
    /// recompression is generational quality loss the caller cannot undo.
    /// `width` and `height` are read from the bytes and the struct's are
    /// ignored.
    Jpeg = 0,
    /// Eight-bit RGB, three bytes per pixel, row-major from the top.
    Rgb8 = 1,
    /// Eight-bit greyscale, one byte per pixel.
    Gray8 = 2,
}

/// An image to register, as C sees `ImageData`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfImage {
    /// Which arm.
    pub kind: TpdfImageKind,
    /// Width in pixels; ignored for [`TpdfImageKind::Jpeg`].
    pub width: u32,
    /// Height in pixels; ignored for [`TpdfImageKind::Jpeg`].
    pub height: u32,
    /// The bytes, borrowed for the duration of the call and copied into the
    /// document before it returns.
    pub data: *const u8,
    /// How many bytes `data` points at.
    pub data_len: usize,
}

/// A borrowed byte slice from C, or a refusal.
///
/// # Safety
///
/// `data` must point to at least `len` readable bytes that outlive the
/// returned slice's use, which at every call site here is the current call.
unsafe fn required_bytes<'a>(
    data: *const u8,
    len: usize,
    what: &str,
) -> Result<&'a [u8], TpdfStatus> {
    if data.is_null() {
        set_error(&format!("{what} is null"));
        return Err(TpdfStatus::BadArgument);
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

/// Starts a document.
///
/// The caller frees the result with [`tpdf_builder_free`], whether or not it
/// was finished.
///
/// # Safety
///
/// `out` must be a valid pointer to write a handle to.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_new(out: *mut *mut TpdfBuilder) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let handle = Box::new(TpdfBuilder {
        inner: Consumable::new(DocumentBuilder::new()),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// Frees a builder. Null is accepted and does nothing.
///
/// Required whether or not [`tpdf_builder_finish`] was called: finishing takes
/// the *document* out of the handle and leaves the handle, which is what makes
/// free symmetric with allocation here as everywhere else.
///
/// # Safety
///
/// `builder` must have come from [`tpdf_builder_new`] and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_free(builder: *mut TpdfBuilder) {
    if !builder.is_null() {
        drop(unsafe { Box::from_raw(builder) });
    }
}

/// The builder behind a handle, or a refusal.
///
/// # Safety
///
/// `builder` must be a live handle or null.
unsafe fn builder_mut<'a>(
    builder: *mut TpdfBuilder,
    what: &str,
) -> Result<&'a mut DocumentBuilder, TpdfStatus> {
    let Some(handle) = (unsafe { builder.as_mut() }) else {
        set_error("null builder");
        return Err(TpdfStatus::BadArgument);
    };
    handle.inner.borrow_mut(what)
}

/// The page behind a handle, or a refusal.
///
/// # Safety
///
/// `page` must be a live handle or null.
unsafe fn page_mut<'a>(
    page: *mut TpdfPageBuilder,
    what: &str,
) -> Result<&'a mut PageBuilder, TpdfStatus> {
    let Some(handle) = (unsafe { page.as_mut() }) else {
        set_error("null page builder");
        return Err(TpdfStatus::BadArgument);
    };
    handle.inner.borrow_mut(what)
}

/// Registers one of the standard 14 fonts under a resource name (9.6.2.2).
///
/// # Safety
///
/// `builder` must be a live handle, and each pointer must be valid for its
/// stated length.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_add_base_font(
    builder: *mut TpdfBuilder,
    resource: *const u8,
    resource_len: usize,
    base_font: *const u8,
    base_font_len: usize,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "add_base_font") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let (Ok(resource), Ok(base_font)) = (
        unsafe { required_bytes(resource, resource_len, "resource name") },
        unsafe { required_bytes(base_font, base_font_len, "base font name") },
    ) else {
        return TpdfStatus::BadArgument;
    };
    builder.add_base_font(resource, base_font);
    TpdfStatus::Ok
}

/// Embeds a TrueType or CFF font program under a resource name.
///
/// # Safety
///
/// `builder` must be a live handle, and each pointer must be valid for its
/// stated length.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_add_embedded_font(
    builder: *mut TpdfBuilder,
    resource: *const u8,
    resource_len: usize,
    base_font: *const u8,
    base_font_len: usize,
    program: *const u8,
    program_len: usize,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "add_embedded_font") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let (Ok(resource), Ok(base_font), Ok(program)) = (
        unsafe { required_bytes(resource, resource_len, "resource name") },
        unsafe { required_bytes(base_font, base_font_len, "base font name") },
        unsafe { required_bytes(program, program_len, "font program") },
    ) else {
        return TpdfStatus::BadArgument;
    };
    if builder.add_embedded_font(resource, base_font, program) {
        TpdfStatus::Ok
    } else {
        refused("add_embedded_font", "the font program was not usable")
    }
}

/// Whether embedded fonts are subsetted to the glyphs actually drawn.
///
/// # Safety
///
/// `builder` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_set_subset_fonts(
    builder: *mut TpdfBuilder,
    subset: c_int,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "set_subset_fonts") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    builder.set_subset_fonts(subset != 0);
    TpdfStatus::Ok
}

/// Registers an image under a resource name, for a page to draw.
///
/// # Safety
///
/// `builder` must be a live handle, `image` a valid pointer, and the image's
/// `data` valid for `data_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_add_image(
    builder: *mut TpdfBuilder,
    resource: *const u8,
    resource_len: usize,
    image: *const TpdfImage,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "add_image") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let Ok(resource) = (unsafe { required_bytes(resource, resource_len, "resource name") }) else {
        return TpdfStatus::BadArgument;
    };
    let Some(image) = (unsafe { image.as_ref() }) else {
        set_error("null image");
        return TpdfStatus::BadArgument;
    };
    let Ok(data) = (unsafe { required_bytes(image.data, image.data_len, "image data") }) else {
        return TpdfStatus::BadArgument;
    };

    let described = match image.kind {
        TpdfImageKind::Jpeg => ImageData::Jpeg(data),
        TpdfImageKind::Rgb8 => ImageData::Rgb8 {
            width: image.width,
            height: image.height,
            data,
        },
        TpdfImageKind::Gray8 => ImageData::Gray8 {
            width: image.width,
            height: image.height,
            data,
        },
    };
    if builder.add_image(resource, &described) {
        TpdfStatus::Ok
    } else {
        refused(
            "add_image",
            &format!(
                "{:?}, {} by {}, {} bytes",
                image.kind,
                image.width,
                image.height,
                data.len()
            ),
        )
    }
}

/// Sets an `/Info` field, such as `Title` or `Author`.
///
/// # Safety
///
/// `builder` must be a live handle, `key` valid for `key_len` bytes, and
/// `value` a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_set_info(
    builder: *mut TpdfBuilder,
    key: *const u8,
    key_len: usize,
    value: *const c_char,
) -> TpdfStatus {
    let builder = match unsafe { builder_mut(builder, "set_info") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let Ok(key) = (unsafe { required_bytes(key, key_len, "info key") }) else {
        return TpdfStatus::BadArgument;
    };
    let value = match unsafe { required_str(value, "info value") } {
        Ok(value) => value,
        Err(status) => return status,
    };
    builder.set_info(key, &value);
    TpdfStatus::Ok
}

/// Starts a page, owned by the caller until [`tpdf_builder_push_page`] takes
/// it.
///
/// **The resource snapshot happens here.** A font or image registered on the
/// builder after this call is invisible to this page -- the same timing the
/// closure form imposes, because `add_page` calls this. A caller that wants a
/// late resource on a page must begin that page after registering it.
///
/// The caller frees the result with [`tpdf_page_builder_free`], whether or not
/// it was pushed.
///
/// # Safety
///
/// `builder` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_begin_page(
    builder: *mut TpdfBuilder,
    width: f64,
    height: f64,
    out: *mut *mut TpdfPageBuilder,
) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let builder = match unsafe { builder_mut(builder, "begin_page") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    let handle = Box::new(TpdfPageBuilder {
        inner: Consumable::new(builder.begin_page(width, height)),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// Adds a page the caller has finished drawing.
///
/// **Consumes the page**: the drawing is taken out of the handle and into the
/// document. The handle stays live and stays the caller's to free, and a
/// second push of the same page is [`TpdfStatus::SpentHandle`] rather than a
/// second page or a double free.
///
/// Pages arrive in the order they are pushed, which is the order they are
/// numbered.
///
/// # Safety
///
/// `builder` and `page` must be live handles.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_push_page(
    builder: *mut TpdfBuilder,
    page: *mut TpdfPageBuilder,
) -> TpdfStatus {
    let Some(page_handle) = (unsafe { page.as_mut() }) else {
        set_error("null page builder");
        return TpdfStatus::BadArgument;
    };
    // The builder is checked before the page is taken, so a push into a spent
    // builder does not swallow the page on the way to refusing.
    if let Err(status) = unsafe { builder_mut(builder, "push_page") } {
        return status;
    }
    let drawn = match page_handle.inner.take("tpdf_builder_push_page") {
        Ok(drawn) => drawn,
        Err(status) => return status,
    };
    let builder = match unsafe { builder_mut(builder, "push_page") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    builder.push_page(drawn);
    TpdfStatus::Ok
}

/// Frees a page builder. Null is accepted and does nothing.
///
/// A page begun and never pushed is simply dropped and the document is what it
/// would have been.
///
/// # Safety
///
/// `page` must have come from [`tpdf_builder_begin_page`] and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_free(page: *mut TpdfPageBuilder) {
    if !page.is_null() {
        drop(unsafe { Box::from_raw(page) });
    }
}

/// Draws text with a registered font.
///
/// # Safety
///
/// `page` must be a live handle, `font` valid for `font_len` bytes, and `text`
/// a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_text(
    page: *mut TpdfPageBuilder,
    font: *const u8,
    font_len: usize,
    size: f64,
    x: f64,
    y: f64,
    text: *const c_char,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "text") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    let Ok(font) = (unsafe { required_bytes(font, font_len, "font resource name") }) else {
        return TpdfStatus::BadArgument;
    };
    let text = match unsafe { required_str(text, "text") } {
        Ok(text) => text,
        Err(status) => return status,
    };
    page.text(font, size, x, y, &text);
    TpdfStatus::Ok
}

/// Fills a rectangle in device grey, from black (0) to white (1).
///
/// # Safety
///
/// `page` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_fill_rect(
    page: *mut TpdfPageBuilder,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    grey: f64,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "fill_rect") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    page.fill_rect(x, y, w, h, grey);
    TpdfStatus::Ok
}

/// Draws a registered image into the given rectangle.
///
/// # Safety
///
/// `page` must be a live handle and `resource` valid for `resource_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_image(
    page: *mut TpdfPageBuilder,
    resource: *const u8,
    resource_len: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "image") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    let Ok(resource) = (unsafe { required_bytes(resource, resource_len, "resource name") }) else {
        return TpdfStatus::BadArgument;
    };
    page.image(resource, x, y, w, h);
    TpdfStatus::Ok
}

/// Sets the non-stroking colour, as red, green and blue from zero to one.
///
/// # Safety
///
/// `page` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_set_fill_rgb(
    page: *mut TpdfPageBuilder,
    r: f64,
    g: f64,
    b: f64,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "set_fill_rgb") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    page.set_fill_rgb(r, g, b);
    TpdfStatus::Ok
}

/// Sets the **stroking** colour. `RG`, not `rg`: the two are different
/// parameters of the graphics state.
///
/// # Safety
///
/// `page` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_set_stroke_rgb(
    page: *mut TpdfPageBuilder,
    r: f64,
    g: f64,
    b: f64,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "set_stroke_rgb") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    page.set_stroke_rgb(r, g, b);
    TpdfStatus::Ok
}

/// Sets `/CropBox` for this page, as `[x0 y0 x1 y1]` in points (7.7.3.3).
///
/// # Safety
///
/// `page` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_set_crop_box(
    page: *mut TpdfPageBuilder,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "set_crop_box") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    page.set_crop_box(x0, y0, x1, y1);
    TpdfStatus::Ok
}

/// Appends content-stream operators verbatim.
///
/// # Safety
///
/// `page` must be a live handle and `operators` valid for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_raw(
    page: *mut TpdfPageBuilder,
    operators: *const u8,
    len: usize,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "raw") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    let Ok(operators) = (unsafe { required_bytes(operators, len, "operators") }) else {
        return TpdfStatus::BadArgument;
    };
    page.raw(operators);
    TpdfStatus::Ok
}

/// Adds a link annotation over a rectangle (12.5.6.5).
///
/// # Safety
///
/// `page` must be a live handle and `target` a valid pointer whose `uri` is a
/// null-terminated string when its kind says so.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_builder_link(
    page: *mut TpdfPageBuilder,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    target: *const TpdfTarget,
) -> TpdfStatus {
    let page = match unsafe { page_mut(page, "link") } {
        Ok(page) => page,
        Err(status) => return status,
    };
    let Some(target) = (unsafe { target.as_ref() }) else {
        set_error("null target");
        return TpdfStatus::BadArgument;
    };
    let facade = match unsafe { target.to_facade() } {
        Ok(target) => target,
        Err(status) => return status,
    };
    if page.link(x0, y0, x1, y1, &facade) {
        TpdfStatus::Ok
    } else {
        refused("link", &format!("[{x0} {y0} {x1} {y1}]"))
    }
}

/// Starts an outline entry with a title and no destination (12.3.3).
///
/// An entry without a destination is a real shape rather than a degraded one:
/// a part title above three chapters often points nowhere itself.
///
/// The caller frees it with [`tpdf_outline_entry_free`], whether or not it was
/// added to anything.
///
/// # Safety
///
/// `title` must be a null-terminated UTF-8 string and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_entry_new(
    title: *const c_char,
    out: *mut *mut TpdfOutlineEntry,
) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let title = match unsafe { required_str(title, "outline title") } {
        Ok(title) => title,
        Err(status) => return status,
    };
    let handle = Box::new(TpdfOutlineEntry {
        inner: Consumable::new(OutlineEntry {
            title,
            target: None,
            open: false,
            children: Vec::new(),
        }),
    });
    unsafe { *out = Box::into_raw(handle) };
    TpdfStatus::Ok
}

/// The entry behind a handle, or a refusal.
///
/// # Safety
///
/// `entry` must be a live handle or null.
unsafe fn entry_mut<'a>(
    entry: *mut TpdfOutlineEntry,
    what: &str,
) -> Result<&'a mut OutlineEntry, TpdfStatus> {
    let Some(handle) = (unsafe { entry.as_mut() }) else {
        set_error("null outline entry");
        return Err(TpdfStatus::BadArgument);
    };
    handle.inner.borrow_mut(what)
}

/// Points an outline entry somewhere.
///
/// # Safety
///
/// `entry` must be a live handle and `target` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_entry_set_target(
    entry: *mut TpdfOutlineEntry,
    target: *const TpdfTarget,
) -> TpdfStatus {
    let entry = match unsafe { entry_mut(entry, "set_target") } {
        Ok(entry) => entry,
        Err(status) => return status,
    };
    let Some(target) = (unsafe { target.as_ref() }) else {
        set_error("null target");
        return TpdfStatus::BadArgument;
    };
    entry.target = Some(match unsafe { target.to_facade() } {
        Ok(target) => target,
        Err(status) => return status,
    });
    TpdfStatus::Ok
}

/// Whether the entry is shown expanded when the document is opened.
///
/// 12.3.3 spells this as the *sign* of `/Count` and only for an entry that has
/// descendants, so it is ignored for an entry with no children: one with
/// nothing beneath it is neither open nor closed.
///
/// # Safety
///
/// `entry` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_entry_set_open(
    entry: *mut TpdfOutlineEntry,
    open: c_int,
) -> TpdfStatus {
    let entry = match unsafe { entry_mut(entry, "set_open") } {
        Ok(entry) => entry,
        Err(status) => return status,
    };
    entry.open = open != 0;
    TpdfStatus::Ok
}

/// Nests one entry under another.
///
/// **Consumes `child`**: it moves into the parent's list, the child handle
/// stays live and stays the caller's to free, and adding it a second time is
/// [`TpdfStatus::SpentHandle`] rather than two copies of one entry.
///
/// # Safety
///
/// `parent` and `child` must be live handles, and must not be the same handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_entry_add_child(
    parent: *mut TpdfOutlineEntry,
    child: *mut TpdfOutlineEntry,
) -> TpdfStatus {
    if parent.is_null() || child.is_null() {
        set_error("null outline entry");
        return TpdfStatus::BadArgument;
    }
    if std::ptr::eq(parent, child) {
        set_error("an outline entry cannot be its own child");
        return TpdfStatus::BadArgument;
    }
    // The parent is checked before the child is taken, so a failed add does
    // not swallow the child.
    if let Err(status) = unsafe { entry_mut(parent, "add_child") } {
        return status;
    }
    let Some(child_handle) = (unsafe { child.as_mut() }) else {
        set_error("null outline entry");
        return TpdfStatus::BadArgument;
    };
    let taken = match child_handle.inner.take("tpdf_outline_entry_add_child") {
        Ok(taken) => taken,
        Err(status) => return status,
    };
    let parent = match unsafe { entry_mut(parent, "add_child") } {
        Ok(parent) => parent,
        Err(status) => return status,
    };
    parent.children.push(taken);
    TpdfStatus::Ok
}

/// Frees an outline entry. Null is accepted and does nothing.
///
/// # Safety
///
/// `entry` must have come from [`tpdf_outline_entry_new`] and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_entry_free(entry: *mut TpdfOutlineEntry) {
    if !entry.is_null() {
        drop(unsafe { Box::from_raw(entry) });
    }
}

/// Sets the document outline from an array of top-level entries (12.3.3).
///
/// **Consumes every entry in `entries`**, in order. Each handle stays live and
/// stays the caller's to free.
///
/// Refused as a whole when the tree is one this repository could not read back
/// -- deeper than the reader's own nesting limit, or wider than its sibling
/// limit -- because a writer whose output its own reader silently truncates is
/// not a writer. Entries taken before the refusal stay taken; the outline is
/// simply not set.
///
/// # Safety
///
/// `builder` must be a live handle and `entries` must point to `count` live
/// entry handles.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_set_outline(
    builder: *mut TpdfBuilder,
    entries: *const *mut TpdfOutlineEntry,
    count: usize,
) -> TpdfStatus {
    if let Err(status) = unsafe { builder_mut(builder, "set_outline") } {
        return status;
    }
    if entries.is_null() && count != 0 {
        set_error("null outline entry array");
        return TpdfStatus::BadArgument;
    }

    let mut taken = Vec::with_capacity(count);
    for index in 0..count {
        let handle = unsafe { *entries.add(index) };
        let Some(handle) = (unsafe { handle.as_mut() }) else {
            set_error(&format!("outline entry {index} is null"));
            return TpdfStatus::BadArgument;
        };
        match handle.inner.take("tpdf_builder_set_outline") {
            Ok(entry) => taken.push(entry),
            Err(status) => return status,
        }
    }

    let builder = match unsafe { builder_mut(builder, "set_outline") } {
        Ok(builder) => builder,
        Err(status) => return status,
    };
    if builder.set_outline(taken) {
        TpdfStatus::Ok
    } else {
        refused(
            "set_outline",
            "the tree is deeper or wider than this engine's own reader walks",
        )
    }
}

/// Finishes the document and hands back its bytes.
///
/// **Consumes the builder**: the document is taken out of the handle, so a
/// second finish is [`TpdfStatus::SpentHandle`] rather than a second document
/// or a double free. The handle stays live and must still be freed with
/// [`tpdf_builder_free`].
///
/// The caller frees the result with [`tpdf_buffer_free`].
///
/// # Safety
///
/// `builder` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_builder_finish(
    builder: *mut TpdfBuilder,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let Some(handle) = (unsafe { builder.as_mut() }) else {
        set_error("null builder");
        return TpdfStatus::BadArgument;
    };
    let document = match handle.inner.take("tpdf_builder_finish") {
        Ok(document) => document,
        Err(status) => return status,
    };
    let bytes = document.finish();
    unsafe { *out = Box::into_raw(Box::new(TpdfBuffer { inner: bytes })) };
    TpdfStatus::Ok
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
<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>
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
            "without a face there is nothing to draw. `Symbol` rather than              `Helvetica`, so this holds in a `bundled-fonts` build too: the              bundled faces decline symbolic fonts by name, and a face the              caller hands over is still used"
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

    // ---- signatures, read (12.8) -------------------------------------------

    use tinker_pdf::{
        Certification, DigestAlgorithm, SignRefused, Signer, SigningRequest, SigningTarget,
        WriteMode, WriteOptions,
    };

    /// A signer that holds no key and returns a recognisable blob.
    ///
    /// The shape is `crates/tinker-pdf/tests/signing.rs`'s, and it is here for
    /// the same reason: a signed fixture built in the test runs without the
    /// fetched corpus, and what this crate has to prove is that the *reading*
    /// surface projects — not that anybody's cryptography works.
    struct StubSigner;

    impl Signer for StubSigner {
        fn digest_algorithm(&self) -> DigestAlgorithm {
            DigestAlgorithm::Sha256
        }

        fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
            let mut blob = vec![0x30, 0x82];
            blob.extend_from_slice(digest);
            blob.resize(300, 0xAB);
            Ok(blob)
        }
    }

    fn incremental() -> WriteOptions {
        WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        }
    }

    /// `testdata/simple-text.pdf` signed twice.
    ///
    /// Twice rather than once because the second signature is what makes the
    /// first cover only a revision, and that is the one shape that gives a
    /// verdict a weakness to carry — so `tpdf_signature_coverage`,
    /// `tpdf_verdict_weakness_count` and `tpdf_verdict_weakness` have
    /// something other than an empty answer to project.
    fn signed_twice() -> Vec<u8> {
        let signer = StubSigner;

        let mut first = SigningRequest::new(
            SigningTarget::NewInvisibleField {
                name: "Signature1".to_string(),
            },
            &signer,
        );
        first.reserve = 2048;
        first.reason = Some("Because the test says so".to_string());
        first.location = Some("Nowhere in particular".to_string());
        first.name = Some("A Signer".to_string());
        first.certification = Some(Certification::FormFillAndSigning);

        let once = Document::open(fixture("simple-text.pdf"))
            .expect("the fixture opens")
            .editor()
            .save_signed(&incremental(), &first)
            .expect("the first signature");

        // No `/Location` on this one, so an absent string has a case too. And
        // no certification: 12.8.2.2 lets only the first signature certify.
        let mut second = SigningRequest::new(
            SigningTarget::NewInvisibleField {
                name: "Signature2".to_string(),
            },
            &signer,
        );
        second.reserve = 2048;
        second.name = Some("Another Signer".to_string());

        Document::open(once)
            .expect("the once-signed file opens")
            .editor()
            .save_signed(&incremental(), &second)
            .expect("the second signature")
    }

    fn open_bytes(bytes: &[u8]) -> *mut TpdfDocument {
        let mut doc: *mut TpdfDocument = ptr::null_mut();
        let status = unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) };
        assert_eq!(status, TpdfStatus::Ok);
        doc
    }

    fn signature_string(
        signatures: *const TpdfSignatures,
        index: u32,
        accessor: unsafe extern "C" fn(*const TpdfSignatures, u32, *mut *mut c_char) -> TpdfStatus,
    ) -> Option<String> {
        let mut text: *mut c_char = ptr::null_mut();
        assert_eq!(
            unsafe { accessor(signatures, index, &mut text) },
            TpdfStatus::Ok
        );
        if text.is_null() {
            return None;
        }
        let value = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        unsafe { tpdf_string_free(text) };
        Some(value)
    }

    fn verdict_string(
        verdicts: *const TpdfVerdicts,
        index: u32,
        accessor: unsafe extern "C" fn(*const TpdfVerdicts, u32, *mut *mut c_char) -> TpdfStatus,
    ) -> Option<String> {
        let mut text: *mut c_char = ptr::null_mut();
        assert_eq!(
            unsafe { accessor(verdicts, index, &mut text) },
            TpdfStatus::Ok
        );
        if text.is_null() {
            return None;
        }
        let value = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        unsafe { tpdf_string_free(text) };
        Some(value)
    }

    /// `TpdfStatus`'s numbers *are* the ABI — a C caller compares them against
    /// literals and the .NET binding against an `int` — so renumbering one
    /// silently changes what every existing caller believes. The original
    /// eight are pinned here one by one, and `NoSuchSignature` is at 8 because
    /// appending is the only way to add.
    #[test]
    fn the_status_codes_are_frozen_and_were_only_appended_to() {
        assert_eq!(TpdfStatus::Ok as i32, 0);
        assert_eq!(TpdfStatus::BadArgument as i32, 1);
        assert_eq!(TpdfStatus::NotAPdf as i32, 2);
        assert_eq!(TpdfStatus::NeedsPassword as i32, 3);
        assert_eq!(TpdfStatus::WrongPassword as i32, 4);
        assert_eq!(TpdfStatus::NoSuchPage as i32, 5);
        assert_eq!(TpdfStatus::NotEncrypted as i32, 6);
        assert_eq!(TpdfStatus::UnsupportedHandler as i32, 7);
        assert_eq!(TpdfStatus::NoSuchSignature as i32, 8);

        // The write surface's five, appended at 9 with gap 32. Once a binding
        // has shipped against these they are as frozen as the eight above; the
        // reason they are pinned the day they are added is that renumbering is
        // easiest, and least noticed, before anybody depends on them.
        assert_eq!(TpdfStatus::NoSuchField as i32, 9);
        assert_eq!(TpdfStatus::ValueRefused as i32, 10);
        assert_eq!(TpdfStatus::FieldUnreadable as i32, 11);
        assert_eq!(TpdfStatus::SpentHandle as i32, 12);
        assert_eq!(TpdfStatus::EditRefused as i32, 13);
    }

    /// A `TpdfStatus` crosses as an `int`, and the three hand-written bindings
    /// each transcribe these numbers. A variant this test does not name is one
    /// nobody promised to keep, which is why the count is pinned too: adding a
    /// variant without adding its line here is caught by the number, not by
    /// somebody remembering.
    #[test]
    fn every_status_the_abi_carries_is_pinned_by_number() {
        // Nothing in Rust enumerates a `#[repr(C)]` enum's variants, so this
        // is the list, written out. It is exactly the fourteen pinned above.
        const EVERY: &[(TpdfStatus, i32)] = &[
            (TpdfStatus::Ok, 0),
            (TpdfStatus::BadArgument, 1),
            (TpdfStatus::NotAPdf, 2),
            (TpdfStatus::NeedsPassword, 3),
            (TpdfStatus::WrongPassword, 4),
            (TpdfStatus::NoSuchPage, 5),
            (TpdfStatus::NotEncrypted, 6),
            (TpdfStatus::UnsupportedHandler, 7),
            (TpdfStatus::NoSuchSignature, 8),
            (TpdfStatus::NoSuchField, 9),
            (TpdfStatus::ValueRefused, 10),
            (TpdfStatus::FieldUnreadable, 11),
            (TpdfStatus::SpentHandle, 12),
            (TpdfStatus::EditRefused, 13),
        ];
        for (status, number) in EVERY {
            assert_eq!(*status as i32, *number, "{status:?}");
        }
        assert_eq!(EVERY.len(), 14, "append only, and say how many there are");
    }

    /// The write surface's other two enums, pinned for the reason
    /// `the_signature_enums_have_the_numbers_the_bindings_transcribe` pins
    /// its own: `bindings/dotnet/TinkerPdf.cs` writes these numbers out by
    /// hand, and a reordered variant would compile on both sides and mean
    /// something different on each.
    #[test]
    fn the_write_enums_have_the_numbers_the_bindings_transcribe() {
        assert_eq!(TpdfWriteMode::Rewrite as i32, 0);
        assert_eq!(TpdfWriteMode::Incremental as i32, 1);
        assert_eq!(TpdfWidgetDefect::RectMissing as i32, 0);
    }

    /// The signature enums' numbers are transcribed by hand into
    /// `bindings/dotnet/TinkerPdf.cs`, so they are pinned on this side too:
    /// a reordered variant would compile everywhere and mean something else
    /// on the other side of the boundary.
    #[test]
    fn the_signature_enums_have_the_numbers_the_bindings_transcribe() {
        assert_eq!(TpdfCoverage::WholeFile as i32, 0);
        assert_eq!(TpdfCoverage::Revision as i32, 1);
        assert_eq!(TpdfCoverage::Suspicious as i32, 2);

        assert_eq!(TpdfCmsState::Read as i32, 0);
        assert_eq!(TpdfCmsState::Absent as i32, 1);
        assert_eq!(TpdfCmsState::Unreadable as i32, 2);

        assert_eq!(TpdfDocumentDigest::Matches as i32, 0);
        assert_eq!(TpdfDocumentDigest::Differs as i32, 1);
        assert_eq!(TpdfDocumentDigest::NotChecked as i32, 2);

        assert_eq!(TpdfSignatureCheck::Verified as i32, 0);
        assert_eq!(TpdfSignatureCheck::Failed as i32, 1);
        assert_eq!(TpdfSignatureCheck::NotChecked as i32, 2);

        assert_eq!(TpdfChain::AnchoredTo as i32, 0);
        assert_eq!(TpdfChain::SelfSigned as i32, 1);
        assert_eq!(TpdfChain::Incomplete as i32, 2);
        assert_eq!(TpdfChain::Broken as i32, 3);
        assert_eq!(TpdfChain::NoAnchors as i32, 4);
        assert_eq!(TpdfChain::NoSignerCertificate as i32, 5);

        assert_eq!(TpdfWeakness::Sha1Digest as i32, 0);
        assert_eq!(TpdfWeakness::Sha1Signature as i32, 1);
        assert_eq!(TpdfWeakness::ShortRsaKey as i32, 2);
        assert_eq!(TpdfWeakness::CoversOnlyARevision as i32, 3);
        assert_eq!(TpdfWeakness::CoverageSuspicious as i32, 4);
        assert_eq!(TpdfWeakness::OutsideValidity as i32, 5);
    }

    /// Most documents have no signatures, and "none" must be an answer rather
    /// than a handle nobody can ask anything of.
    #[test]
    fn a_document_with_no_signatures_says_so_rather_than_failing() {
        let bytes = fixture("simple-text.pdf");
        let facade = Document::open(bytes.clone()).expect("the fixture opens");
        assert!(facade.signatures().is_empty(), "the fixture is unsigned");

        let doc = open_bytes(&bytes);
        let mut signatures: *mut TpdfSignatures = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_signatures(doc, &mut signatures) },
            TpdfStatus::Ok,
            "no signatures is not an error"
        );
        assert!(!signatures.is_null());
        assert_eq!(unsafe { tpdf_signatures_count(signatures) }, 0);

        let anchors = unsafe { tpdf_trust_anchors_new() };
        let mut verdicts: *mut TpdfVerdicts = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(doc, anchors, 0, 0, &mut verdicts) },
            TpdfStatus::Ok
        );
        assert_eq!(unsafe { tpdf_verdicts_count(verdicts) }, 0);

        unsafe {
            tpdf_verdicts_free(verdicts);
            tpdf_trust_anchors_free(anchors);
            tpdf_signatures_free(signatures);
            tpdf_document_free(doc);
        }
    }

    /// Ruling 1: an index a caller made up is refused, never dereferenced and
    /// never a panic. Two signatures exist, so index 2 is the first one past
    /// the end.
    #[test]
    fn an_index_past_the_last_signature_is_refused() {
        let bytes = signed_twice();
        let doc = open_bytes(&bytes);
        let mut signatures: *mut TpdfSignatures = ptr::null_mut();
        unsafe { tpdf_document_signatures(doc, &mut signatures) };
        assert_eq!(unsafe { tpdf_signatures_count(signatures) }, 2);

        let mut text: *mut c_char = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_signature_reason(signatures, 2, &mut text) },
            TpdfStatus::NoSuchSignature
        );
        assert!(text.is_null(), "nothing was allocated to leak");

        let mut coverage = TpdfCoverage::WholeFile;
        assert_eq!(
            unsafe { tpdf_signature_coverage(signatures, 2, &mut coverage) },
            TpdfStatus::NoSuchSignature
        );
        assert_eq!(
            unsafe { tpdf_signature_span(signatures, 2, 0, ptr::null_mut(), ptr::null_mut()) },
            TpdfStatus::NoSuchSignature
        );
        assert_eq!(
            unsafe { tpdf_signature_covers_whole_file(signatures, 2) },
            0
        );
        assert_eq!(unsafe { tpdf_signature_is_usage_rights(signatures, 2) }, 0);
        assert_eq!(
            unsafe { tpdf_signature_certification_level(signatures, 2) },
            0
        );
        assert_eq!(unsafe { tpdf_signature_span_count(signatures, 2) }, 0);

        // A span index past the end of a signature that *is* there is a bad
        // argument, not a missing signature — the two are different mistakes.
        assert_eq!(
            unsafe { tpdf_signature_span(signatures, 0, 99, ptr::null_mut(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );

        let anchors = unsafe { tpdf_trust_anchors_new() };
        let mut verdicts: *mut TpdfVerdicts = ptr::null_mut();
        unsafe { tpdf_document_verify_signatures(doc, anchors, 0, 0, &mut verdicts) };
        let mut chain = TpdfChain::NoAnchors;
        assert_eq!(
            unsafe { tpdf_verdict_chain(verdicts, 2, &mut chain) },
            TpdfStatus::NoSuchSignature
        );
        assert_eq!(unsafe { tpdf_verdict_weakness_count(verdicts, 2) }, 0);
        assert_eq!(
            unsafe { tpdf_verdict_signer_validity(verdicts, 2, ptr::null_mut(), ptr::null_mut()) },
            0
        );
        let mut weakness = TpdfWeakness::Sha1Digest;
        assert_eq!(
            unsafe { tpdf_verdict_weakness(verdicts, 0, 99, &mut weakness) },
            TpdfStatus::BadArgument,
            "a weakness index past the end is a bad argument, not a missing verdict"
        );

        unsafe {
            tpdf_verdicts_free(verdicts);
            tpdf_trust_anchors_free(anchors);
            tpdf_signatures_free(signatures);
            tpdf_document_free(doc);
        }
    }

    /// Ruling 1 again, for the handles this milestone added: every one of them
    /// tolerates a null rather than dereferencing it, and freeing null is a
    /// no-op.
    #[test]
    fn null_handles_across_the_signature_surface_are_refused() {
        let bytes = fixture("simple-text.pdf");
        let doc = open_bytes(&bytes);

        let mut signatures: *mut TpdfSignatures = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_signatures(ptr::null(), &mut signatures) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_document_signatures(doc, ptr::null_mut()) },
            TpdfStatus::BadArgument
        );

        let mut text: *mut c_char = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_signature_field_name(ptr::null(), 0, &mut text) },
            TpdfStatus::BadArgument
        );
        let mut coverage = TpdfCoverage::WholeFile;
        assert_eq!(
            unsafe { tpdf_signature_coverage(ptr::null(), 0, &mut coverage) },
            TpdfStatus::BadArgument
        );
        assert_eq!(unsafe { tpdf_signatures_count(ptr::null()) }, 0);
        assert_eq!(
            unsafe { tpdf_signature_covers_whole_file(ptr::null(), 0) },
            0
        );
        assert_eq!(unsafe { tpdf_signature_is_usage_rights(ptr::null(), 0) }, 0);
        assert_eq!(
            unsafe { tpdf_signature_certification_level(ptr::null(), 0) },
            0
        );
        assert_eq!(unsafe { tpdf_signature_span_count(ptr::null(), 0) }, 0);

        assert_eq!(unsafe { tpdf_trust_anchors_count(ptr::null()) }, 0);
        let der = [0x30u8, 0x00];
        assert_eq!(
            unsafe { tpdf_trust_anchors_add(ptr::null_mut(), der.as_ptr(), der.len()) },
            TpdfStatus::BadArgument
        );

        // A null anchor set is refused rather than read as "no anchors": an
        // empty handle is how a caller says it trusts nothing, and inventing
        // that from a null pointer would be a default the facade has not got.
        let mut verdicts: *mut TpdfVerdicts = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(doc, ptr::null(), 0, 0, &mut verdicts) },
            TpdfStatus::BadArgument
        );
        let anchors = unsafe { tpdf_trust_anchors_new() };
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(ptr::null(), anchors, 0, 0, &mut verdicts) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(doc, anchors, 0, 0, ptr::null_mut()) },
            TpdfStatus::BadArgument
        );

        assert_eq!(unsafe { tpdf_verdicts_count(ptr::null()) }, 0);
        let mut state = TpdfCmsState::Absent;
        assert_eq!(
            unsafe { tpdf_verdict_cms_state(ptr::null(), 0, &mut state) },
            TpdfStatus::BadArgument
        );
        assert_eq!(unsafe { tpdf_verdict_weakness_count(ptr::null(), 0) }, 0);
        assert_eq!(
            unsafe {
                tpdf_verdict_signer_validity(ptr::null(), 0, ptr::null_mut(), ptr::null_mut())
            },
            0
        );

        unsafe {
            tpdf_trust_anchors_free(anchors);
            tpdf_signatures_free(ptr::null_mut());
            tpdf_verdicts_free(ptr::null_mut());
            tpdf_trust_anchors_free(ptr::null_mut());
            tpdf_document_free(doc);
        }
    }

    /// Bytes that are not a certificate are refused where they are offered,
    /// which is the reason this surface takes anchors one at a time.
    #[test]
    fn an_anchor_that_is_not_a_certificate_is_refused_and_not_kept() {
        let anchors = unsafe { tpdf_trust_anchors_new() };
        let empty_sequence = [0x30u8, 0x00];
        assert_eq!(
            unsafe {
                tpdf_trust_anchors_add(anchors, empty_sequence.as_ptr(), empty_sequence.len())
            },
            TpdfStatus::BadArgument
        );
        assert!(!unsafe { tpdf_last_error_message() }.is_null());
        assert_eq!(
            unsafe { tpdf_trust_anchors_count(anchors) },
            0,
            "and it was not kept"
        );
        unsafe { tpdf_trust_anchors_free(anchors) };
    }

    /// Ruling 11, as an equality.
    ///
    /// Every answer the C ABI gives about a signed document is the answer the
    /// facade gives about the same bytes. A projection that agreed with itself
    /// and not with the facade would be a second implementation, and this is
    /// the assertion that says it is not one.
    #[test]
    fn a_signed_document_reads_the_same_through_the_abi_as_through_the_facade() {
        let bytes = signed_twice();
        let facade = Document::open(bytes.clone()).expect("the signed file reopens");
        let expected = facade.signatures();
        assert_eq!(expected.len(), 2, "two signatures were written");

        let doc = open_bytes(&bytes);
        let mut signatures: *mut TpdfSignatures = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_signatures(doc, &mut signatures) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_signatures_count(signatures) } as usize,
            expected.len()
        );

        for (position, signature) in expected.iter().enumerate() {
            let index = u32::try_from(position).expect("two signatures");

            assert_eq!(
                signature_string(signatures, index, tpdf_signature_field_name),
                signature.field,
                "field name at {index}"
            );
            assert_eq!(
                signature_string(signatures, index, tpdf_signature_sub_filter),
                signature.sub_filter_name,
                "sub-filter at {index}"
            );
            assert_eq!(
                signature_string(signatures, index, tpdf_signature_reason),
                signature.reason,
                "reason at {index}"
            );
            assert_eq!(
                signature_string(signatures, index, tpdf_signature_location),
                signature.location,
                "location at {index}"
            );
            assert_eq!(
                signature_string(signatures, index, tpdf_signature_name),
                signature.name,
                "name at {index}"
            );

            // Re-derived here rather than run through the crate's own
            // conversion, so the test compares two readings instead of one.
            let coverage = match &signature.coverage {
                Coverage::WholeFile => TpdfCoverage::WholeFile,
                Coverage::Revision { .. } => TpdfCoverage::Revision,
                Coverage::Suspicious(_) => TpdfCoverage::Suspicious,
            };
            let mut through_c = TpdfCoverage::Suspicious;
            assert_eq!(
                unsafe { tpdf_signature_coverage(signatures, index, &mut through_c) },
                TpdfStatus::Ok
            );
            assert_eq!(through_c, coverage, "coverage at {index}");

            assert_eq!(
                unsafe { tpdf_signature_covers_whole_file(signatures, index) } != 0,
                signature.covers_whole_file(),
                "covers-whole-file at {index}"
            );
            assert_eq!(
                unsafe { tpdf_signature_is_usage_rights(signatures, index) } != 0,
                signature.is_usage_rights(),
                "usage rights at {index}"
            );

            assert_eq!(
                unsafe { tpdf_signature_span_count(signatures, index) } as usize,
                signature.spans.len(),
                "span count at {index}"
            );
            for (place, span) in signature.spans.iter().enumerate() {
                let place = u32::try_from(place).expect("two spans");
                let (mut start, mut length) = (0u64, 0u64);
                assert_eq!(
                    unsafe {
                        tpdf_signature_span(signatures, index, place, &mut start, &mut length)
                    },
                    TpdfStatus::Ok
                );
                assert_eq!(start, span.start, "span {place} start at {index}");
                assert_eq!(
                    length,
                    span.end - span.start,
                    "span {place} length at {index}"
                );
            }
        }

        // The concrete facts the two requests asked for, so the equality above
        // is an equality between two right answers rather than two wrong ones.
        assert_eq!(
            signature_string(signatures, 0, tpdf_signature_field_name).as_deref(),
            Some("Signature1")
        );
        assert_eq!(
            signature_string(signatures, 0, tpdf_signature_reason).as_deref(),
            Some("Because the test says so")
        );
        assert_eq!(
            signature_string(signatures, 0, tpdf_signature_location).as_deref(),
            Some("Nowhere in particular")
        );
        assert_eq!(
            signature_string(signatures, 1, tpdf_signature_location),
            None,
            "the second request set no /Location, and absent is a null on Ok"
        );
        assert_eq!(
            unsafe { tpdf_signature_certification_level(signatures, 0) },
            2,
            "the first request certified at /P 2"
        );
        assert_eq!(
            expected[0].certification,
            Some(Certification::FormFillAndSigning)
        );
        assert_eq!(
            unsafe { tpdf_signature_certification_level(signatures, 1) },
            0,
            "and the second certified nothing"
        );

        // ---- the verdicts ------------------------------------------------
        let anchors = unsafe { tpdf_trust_anchors_new() };
        let mut verdicts: *mut TpdfVerdicts = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(doc, anchors, 0, 0, &mut verdicts) },
            TpdfStatus::Ok
        );

        let expected_verdicts = facade.verify_signatures(&TrustAnchors::new(), None);
        assert_eq!(
            unsafe { tpdf_verdicts_count(verdicts) } as usize,
            expected_verdicts.len()
        );

        for (position, verdict) in expected_verdicts.iter().enumerate() {
            let index = u32::try_from(position).expect("two verdicts");

            let cms = match &verdict.cms {
                CmsState::Read { .. } => TpdfCmsState::Read,
                CmsState::Absent => TpdfCmsState::Absent,
                CmsState::Unreadable(_) => TpdfCmsState::Unreadable,
            };
            let mut through_c = TpdfCmsState::Absent;
            assert_eq!(
                unsafe { tpdf_verdict_cms_state(verdicts, index, &mut through_c) },
                TpdfStatus::Ok
            );
            assert_eq!(through_c, cms, "cms state at {index}");

            let digest = match &verdict.document_digest {
                DocumentDigest::Matches => TpdfDocumentDigest::Matches,
                DocumentDigest::Differs => TpdfDocumentDigest::Differs,
                DocumentDigest::NotChecked(_) => TpdfDocumentDigest::NotChecked,
            };
            let mut through_c = TpdfDocumentDigest::Matches;
            assert_eq!(
                unsafe { tpdf_verdict_document_digest(verdicts, index, &mut through_c) },
                TpdfStatus::Ok
            );
            assert_eq!(through_c, digest, "document digest at {index}");

            let check = match &verdict.signature {
                SignatureCheck::Verified => TpdfSignatureCheck::Verified,
                SignatureCheck::Failed => TpdfSignatureCheck::Failed,
                SignatureCheck::NotChecked(_) => TpdfSignatureCheck::NotChecked,
            };
            let mut through_c = TpdfSignatureCheck::Verified;
            assert_eq!(
                unsafe { tpdf_verdict_signature_check(verdicts, index, &mut through_c) },
                TpdfStatus::Ok
            );
            assert_eq!(through_c, check, "signature check at {index}");

            let chain = match &verdict.chain {
                Chain::AnchoredTo { .. } => TpdfChain::AnchoredTo,
                Chain::SelfSigned { .. } => TpdfChain::SelfSigned,
                Chain::Incomplete { .. } => TpdfChain::Incomplete,
                Chain::Broken { .. } => TpdfChain::Broken,
                Chain::NoAnchors => TpdfChain::NoAnchors,
                Chain::NoSignerCertificate => TpdfChain::NoSignerCertificate,
            };
            let mut through_c = TpdfChain::NoAnchors;
            assert_eq!(
                unsafe { tpdf_verdict_chain(verdicts, index, &mut through_c) },
                TpdfStatus::Ok
            );
            assert_eq!(through_c, chain, "chain at {index}");

            assert_eq!(
                verdict_string(verdicts, index, tpdf_verdict_signer_subject),
                verdict.signer.as_ref().map(|s| s.subject.clone()),
                "signer subject at {index}"
            );
            assert_eq!(
                verdict_string(verdicts, index, tpdf_verdict_signer_issuer),
                verdict.signer.as_ref().map(|s| s.issuer.clone()),
                "signer issuer at {index}"
            );

            let (mut not_before, mut not_after) = (0i64, 0i64);
            let described = unsafe {
                tpdf_verdict_signer_validity(verdicts, index, &mut not_before, &mut not_after)
            };
            assert_eq!(
                described != 0,
                verdict.signer.is_some(),
                "signer presence at {index}"
            );
            if let Some(signer) = verdict.signer.as_ref() {
                assert_eq!((not_before, not_after), signer.validity);
            }

            assert_eq!(
                unsafe { tpdf_verdict_weakness_count(verdicts, index) } as usize,
                verdict.weaknesses.len(),
                "weakness count at {index}"
            );
            for (place, weakness) in verdict.weaknesses.iter().enumerate() {
                let place = u32::try_from(place).expect("a short list");
                let expected_weakness = match weakness {
                    Weakness::Sha1Digest => TpdfWeakness::Sha1Digest,
                    Weakness::Sha1Signature => TpdfWeakness::Sha1Signature,
                    Weakness::ShortRsaKey { .. } => TpdfWeakness::ShortRsaKey,
                    Weakness::CoversOnlyARevision => TpdfWeakness::CoversOnlyARevision,
                    Weakness::CoverageSuspicious => TpdfWeakness::CoverageSuspicious,
                    Weakness::OutsideValidity { .. } => TpdfWeakness::OutsideValidity,
                };
                let mut through_c = TpdfWeakness::Sha1Digest;
                assert_eq!(
                    unsafe { tpdf_verdict_weakness(verdicts, index, place, &mut through_c) },
                    TpdfStatus::Ok
                );
                assert_eq!(through_c, expected_weakness, "weakness {place} at {index}");
            }
        }

        // And the shape the second signing produced, named rather than left to
        // whatever the loop happened to compare: the first signature now
        // covers only the revision it was made over, and the verdict says so.
        let first = expected
            .iter()
            .position(|s| s.field.as_deref() == Some("Signature1"))
            .expect("Signature1 is there");
        let first = u32::try_from(first).expect("two signatures");
        let mut coverage = TpdfCoverage::WholeFile;
        unsafe { tpdf_signature_coverage(signatures, first, &mut coverage) };
        assert_eq!(coverage, TpdfCoverage::Revision);
        assert_eq!(
            unsafe { tpdf_signature_covers_whole_file(signatures, first) },
            0
        );
        assert_eq!(unsafe { tpdf_verdict_weakness_count(verdicts, first) }, 1);
        let mut weakness = TpdfWeakness::Sha1Digest;
        unsafe { tpdf_verdict_weakness(verdicts, first, 0, &mut weakness) };
        assert_eq!(weakness, TpdfWeakness::CoversOnlyARevision);

        // No anchors were supplied, and the honest answer to "whose key is it"
        // is that nobody said what to trust. It reads `NoSignerCertificate`
        // here rather than `NoAnchors` because the stub signer's blob is not
        // CMS, so the walk never had a certificate to start from — which is
        // the facade's answer too, and the loop above already asserted it.
        assert_eq!(unsafe { tpdf_trust_anchors_count(anchors) }, 0);

        unsafe {
            tpdf_verdicts_free(verdicts);
            tpdf_trust_anchors_free(anchors);
            tpdf_signatures_free(signatures);
            tpdf_document_free(doc);
        }
    }

    /// The instant is a separate argument from the flag that says whether to
    /// use it, because `Option<i64>` has no C spelling. With the flag clear
    /// the instant is not read at all, so a caller that passes rubbish there
    /// gets the same verdicts as one that passes zero.
    #[test]
    fn the_validity_instant_is_read_only_when_the_flag_says_to() {
        let bytes = signed_twice();
        let doc = open_bytes(&bytes);
        let anchors = unsafe { tpdf_trust_anchors_new() };

        let mut ignored: *mut TpdfVerdicts = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(doc, anchors, 0, i64::MIN, &mut ignored) },
            TpdfStatus::Ok
        );
        let mut zeroed: *mut TpdfVerdicts = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(doc, anchors, 0, 0, &mut zeroed) },
            TpdfStatus::Ok
        );
        for index in 0..unsafe { tpdf_verdicts_count(ignored) } {
            assert_eq!(
                unsafe { tpdf_verdict_weakness_count(ignored, index) },
                unsafe { tpdf_verdict_weakness_count(zeroed, index) }
            );
        }

        // And with the flag set the facade is asked to judge, which for these
        // signatures changes nothing — there is no certificate to be outside
        // the validity of — but the call is the one a host makes.
        let mut judged: *mut TpdfVerdicts = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_verify_signatures(doc, anchors, 1, 1_700_000_000, &mut judged) },
            TpdfStatus::Ok
        );
        let facade = Document::open(bytes).expect("reopens");
        let expected = facade.verify_signatures(&TrustAnchors::new(), Some(1_700_000_000));
        assert_eq!(
            unsafe { tpdf_verdicts_count(judged) } as usize,
            expected.len()
        );
        for (position, verdict) in expected.iter().enumerate() {
            let index = u32::try_from(position).expect("two verdicts");
            assert_eq!(
                unsafe { tpdf_verdict_weakness_count(judged, index) } as usize,
                verdict.weaknesses.len()
            );
        }

        unsafe {
            tpdf_verdicts_free(ignored);
            tpdf_verdicts_free(zeroed);
            tpdf_verdicts_free(judged);
            tpdf_trust_anchors_free(anchors);
            tpdf_document_free(doc);
        }
    }

    /// The handle owns its own reading, so a host that frees the document and
    /// keeps the signatures still has them. Stated in the doc comment; asserted
    /// here, because a borrow would be a use-after-free rather than a wrong
    /// answer.
    #[test]
    fn signatures_outlive_the_document_they_came_from() {
        let bytes = signed_twice();
        let doc = open_bytes(&bytes);
        let mut signatures: *mut TpdfSignatures = ptr::null_mut();
        unsafe { tpdf_document_signatures(doc, &mut signatures) };
        unsafe { tpdf_document_free(doc) };

        assert_eq!(unsafe { tpdf_signatures_count(signatures) }, 2);
        assert_eq!(
            signature_string(signatures, 0, tpdf_signature_sub_filter).as_deref(),
            Some("adbe.pkcs7.detached")
        );
        unsafe { tpdf_signatures_free(signatures) };
    }

    // -- writing: the editor surface (gap 32 milestone 2) -------------------
    //
    // The claim every test below serves is one sentence: **the C ABI writes
    // the same bytes the facade does**. Not similar bytes and not a valid
    // document -- the same bytes, because anything less means the four
    // bindings are four writers and the parity suite is comparing them to each
    // other rather than to the engine.

    /// A null-terminated C string from a Rust one, kept alive by the caller.
    fn c(text: &str) -> CString {
        CString::new(text).expect("no interior nul in a test string")
    }

    /// The committed form fixture, opened through the C ABI.
    fn open_form() -> *mut TpdfDocument {
        open("form-fields.pdf")
    }

    /// An editor over it, through the C ABI.
    fn editor_over(doc: *mut TpdfDocument) -> *mut TpdfEditor {
        let mut editor: *mut TpdfEditor = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_document_editor(doc, &mut editor) },
            TpdfStatus::Ok
        );
        assert!(!editor.is_null());
        editor
    }

    /// Incremental save options, as both sides of every comparison use them.
    fn incremental_options() -> TpdfWriteOptions {
        let mut options = TpdfWriteOptions {
            mode: TpdfWriteMode::Rewrite,
            linearize: 0,
            version_major: 0,
            version_minor: 0,
            object_streams: 0,
            compress: 0,
            garbage_collect: 0,
            encryption: ptr::null(),
        };
        assert_eq!(
            unsafe { tpdf_write_options_init(&mut options) },
            TpdfStatus::Ok
        );
        options.mode = TpdfWriteMode::Incremental;
        options
    }

    /// A saved buffer's bytes, copied out and the handle freed.
    fn take_buffer(buffer: *mut TpdfBuffer) -> Vec<u8> {
        assert!(!buffer.is_null());
        let mut len = 0usize;
        let data = unsafe { tpdf_buffer_data(buffer, &mut len) };
        assert!(!data.is_null());
        assert_eq!(len, unsafe { tpdf_buffer_len(buffer) });
        let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
        unsafe { tpdf_buffer_free(buffer) };
        bytes
    }

    /// The fill-and-save script, driven entirely through the C ABI.
    ///
    /// Kept beside the facade-direct twin below so the two read as the same
    /// program written twice, which is what makes a byte difference between
    /// them a difference in the boundary rather than in the script.
    fn fill_and_save_through_the_abi() -> (Vec<u8>, Vec<String>) {
        let doc = open_form();
        let editor = editor_over(doc);
        // Freed here, before a single edit, because the editor holds its own
        // reference to the object store. If that were untrue the rest of this
        // function would be a use-after-free rather than a test.
        unsafe { tpdf_document_free(doc) };

        let mut report: *mut TpdfFillReport = ptr::null_mut();
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(
                    editor,
                    c("name").as_ptr(),
                    c("Ada Lovelace").as_ptr(),
                    &mut report,
                )
            },
            TpdfStatus::Ok,
            "a widget that cannot be drawn is not a failed fill"
        );

        let mut messages = Vec::new();
        for index in 0..unsafe { tpdf_fill_report_count(report) } {
            let mut text: *mut c_char = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_fill_report_message(report, index, &mut text) },
                TpdfStatus::Ok
            );
            messages.push(
                unsafe { CStr::from_ptr(text) }
                    .to_string_lossy()
                    .into_owned(),
            );
            unsafe { tpdf_string_free(text) };
        }
        unsafe { tpdf_fill_report_free(report) };

        // The control field, filled through the same call: its report must
        // come back empty, so this script exercises both legs rather than one.
        let mut clean: *mut TpdfFillReport = ptr::null_mut();
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(
                    editor,
                    c("notes").as_ptr(),
                    c("every surface writes this").as_ptr(),
                    &mut clean,
                )
            },
            TpdfStatus::Ok
        );
        assert_eq!(unsafe { tpdf_fill_report_count(clean) }, 0);
        unsafe { tpdf_fill_report_free(clean) };

        assert_eq!(
            unsafe { tpdf_editor_set_checkbox(editor, c("agree").as_ptr(), 1) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_editor_select_radio(editor, c("colour").as_ptr(), c("red").as_ptr()) },
            TpdfStatus::Ok
        );

        let options = incremental_options();
        let mut buffer: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_editor_save(editor, &options, &mut buffer) },
            TpdfStatus::Ok
        );
        let bytes = take_buffer(buffer);
        unsafe { tpdf_editor_free(editor) };
        (bytes, messages)
    }

    /// The same script against the facade, in Rust.
    fn fill_and_save_through_the_facade() -> (Vec<u8>, Vec<String>) {
        let document = Document::open(fixture("form-fields.pdf")).expect("the fixture opens");
        let mut editor = document.editor();
        let skipped = editor
            .fill_field("name", "Ada Lovelace")
            .expect("the value is taken");
        let messages = skipped.iter().map(ToString::to_string).collect();
        assert!(editor
            .fill_field("notes", "every surface writes this")
            .expect("the value is taken")
            .is_empty());
        assert!(editor.set_checkbox("agree", true));
        assert!(editor.select_radio("colour", "red"));
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        });
        (bytes, messages)
    }

    /// The milestone's own exit criterion, and the one the whole design rests
    /// on: **byte-equal**.
    #[test]
    fn filling_and_saving_through_the_abi_is_byte_equal_to_the_facade() {
        let (through_abi, abi_messages) = fill_and_save_through_the_abi();
        let (through_facade, facade_messages) = fill_and_save_through_the_facade();

        assert_eq!(
            through_abi.len(),
            through_facade.len(),
            "the two saves are not even the same length"
        );
        assert_eq!(
            through_abi, through_facade,
            "the C ABI wrote a different document than the facade did"
        );
        assert_eq!(
            abi_messages, facade_messages,
            "and it reported the skipped widget in the same words"
        );
        assert_eq!(
            abi_messages,
            vec!["7 0 R: no usable /Rect (12.5.2)".to_string()],
            "exactly one widget, named"
        );
    }

    /// 7.5.6: an incremental save appends. The original bytes must be a
    /// prefix, because that is what keeps a signature over them covering what
    /// it covered (12.8.1) -- and it is a property of *this boundary's* output
    /// rather than an inherited one, since the C ABI is what chose the mode.
    #[test]
    fn an_incremental_save_through_the_abi_keeps_the_original_as_a_prefix() {
        let original = fixture("form-fields.pdf");
        let (saved, _) = fill_and_save_through_the_abi();
        assert!(saved.len() > original.len());
        assert_eq!(&saved[..original.len()], &original[..]);

        // And the artefact is a document this engine reads back, at the
        // ladder's top rung: ruling 13 makes the check that the bytes are
        // right a first-party one, and this is that check on the C ABI's own
        // output.
        let reopened = Document::open(saved).expect("the C ABI's output opens");
        assert!(
            reopened.validate().is_empty(),
            "and the strict validator finds nothing in it: {:?}",
            reopened.validate()
        );
    }

    /// The fourth outcome, entry by entry: the widget's `ObjRef` and its
    /// defect, not only a sentence about them.
    #[test]
    fn the_fill_report_carries_the_widget_and_the_defect() {
        let doc = open_form();
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        let mut report: *mut TpdfFillReport = ptr::null_mut();
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(editor, c("name").as_ptr(), c("Ada").as_ptr(), &mut report)
            },
            TpdfStatus::Ok
        );
        assert_eq!(unsafe { tpdf_fill_report_count(report) }, 1);

        let (mut number, mut generation) = (0u32, 0u16);
        assert_eq!(
            unsafe { tpdf_fill_report_widget(report, 0, &mut number, &mut generation) },
            TpdfStatus::Ok
        );
        assert_eq!((number, generation), (7, 0));

        let mut defect = TpdfWidgetDefect::RectMissing;
        assert_eq!(
            unsafe { tpdf_fill_report_defect(report, 0, &mut defect) },
            TpdfStatus::Ok
        );
        assert_eq!(defect, TpdfWidgetDefect::RectMissing);

        // Past the end is refused rather than answered.
        assert_eq!(
            unsafe { tpdf_fill_report_widget(report, 1, &mut number, &mut generation) },
            TpdfStatus::BadArgument
        );

        unsafe { tpdf_fill_report_free(report) };
        unsafe { tpdf_editor_free(editor) };
    }

    /// A field whose every widget can be drawn reports an **empty** report,
    /// which is a success rather than an absence of one.
    ///
    /// This is the control for
    /// [`the_fill_report_carries_the_widget_and_the_defect`]: without it, "the
    /// report was non-empty" cannot be told apart from "the report is always
    /// non-empty", which is what a binding that inverted the condition would
    /// look like from the outside.
    #[test]
    fn a_field_that_draws_cleanly_reports_an_empty_report() {
        let doc = open("form-fields.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        let mut report: *mut TpdfFillReport = ptr::null_mut();
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(
                    editor,
                    c("notes").as_ptr(),
                    c("all drawable").as_ptr(),
                    &mut report,
                )
            },
            TpdfStatus::Ok
        );
        assert!(!report.is_null(), "Ok always writes a report");
        assert_eq!(
            unsafe { tpdf_fill_report_count(report) },
            0,
            "and for an undamaged field it is empty"
        );
        unsafe { tpdf_fill_report_free(report) };
        assert_eq!(unsafe { tpdf_editor_is_dirty(editor) }, 1);

        unsafe { tpdf_editor_free(editor) };
    }

    /// A fill that is refused outright writes **no report at all**, because
    /// nothing was written and there is nothing to report about.
    #[test]
    fn a_refused_fill_writes_no_report_at_all() {
        let doc = open("form-fields.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        let mut report: *mut TpdfFillReport = ptr::null_mut();
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(
                    editor,
                    c("no-such-field").as_ptr(),
                    c("x").as_ptr(),
                    &mut report,
                )
            },
            TpdfStatus::NoSuchField
        );
        assert!(report.is_null());
        assert_eq!(unsafe { tpdf_editor_is_dirty(editor) }, 0);

        // And the report accessors tolerate the null they were just handed.
        assert_eq!(unsafe { tpdf_fill_report_count(report) }, 0);
        unsafe { tpdf_fill_report_free(report) };
        unsafe { tpdf_editor_free(editor) };
    }

    /// Every `FillError` variant reaches C as its own status, so a caller can
    /// tell "there is no such field" from "the field will not take that".
    #[test]
    fn each_fill_refusal_crosses_as_its_own_status() {
        let doc = open("form-fields.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(editor, c("nope").as_ptr(), c("x").as_ptr(), ptr::null_mut())
            },
            TpdfStatus::NoSuchField
        );

        // /MaxLen is 32 on this field, so 40 characters is refused rather
        // than truncated -- truncation hides a data error inside a file that
        // then looks correctly filled.
        let long = "x".repeat(40);
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(
                    editor,
                    c("name").as_ptr(),
                    c(&long).as_ptr(),
                    ptr::null_mut(),
                )
            },
            TpdfStatus::ValueRefused
        );
        assert_eq!(
            unsafe { tpdf_editor_is_dirty(editor) },
            0,
            "and a refusal wrote nothing"
        );
        unsafe { tpdf_editor_free(editor) };
    }

    /// Checkpoint, edit, restore -- the round trip, through the boundary.
    #[test]
    fn a_checkpoint_round_trips_through_the_abi() {
        let doc = open("form-fields.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        let options = incremental_options();
        let save = || {
            let mut buffer: *mut TpdfBuffer = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_editor_save(editor, &options, &mut buffer) },
                TpdfStatus::Ok
            );
            take_buffer(buffer)
        };

        assert_eq!(
            unsafe { tpdf_editor_set_checkbox(editor, c("agree").as_ptr(), 1) },
            TpdfStatus::Ok
        );
        let before = save();

        let mut mark: *mut TpdfCheckpoint = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_editor_checkpoint(editor, &mut mark) },
            TpdfStatus::Ok
        );
        assert!(!mark.is_null());

        assert_eq!(
            unsafe { tpdf_editor_select_radio(editor, c("colour").as_ptr(), c("blue").as_ptr()) },
            TpdfStatus::Ok
        );
        assert_ne!(save(), before, "the second edit changed the file");

        assert_eq!(unsafe { tpdf_editor_restore(editor, mark) }, TpdfStatus::Ok);
        assert_eq!(save(), before, "and the restore undid exactly it");

        // Idempotent, which is the property a host language's `finally` needs:
        // it may run after its own `catch` has already restored.
        for _ in 0..3 {
            assert_eq!(unsafe { tpdf_editor_restore(editor, mark) }, TpdfStatus::Ok);
            assert_eq!(save(), before);
        }

        // The checkpoint is borrowed, not consumed, so it is still the
        // caller's to free -- and freeing it commits nothing.
        unsafe { tpdf_checkpoint_free(mark) };
        assert_eq!(save(), before);
        unsafe { tpdf_editor_free(editor) };
    }

    /// The lifetime claim, made explicitly rather than relied on: the editor
    /// holds its own reference, so the document handle may go first.
    ///
    /// This is what lets the .NET `SafeHandle`s stay independent instead of
    /// needing a parent-child keep-alive, so it is worth a test of its own
    /// rather than being a side effect of the tests above.
    #[test]
    fn an_editor_outlives_the_document_it_came_from() {
        let doc = open("form-fields.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        assert_eq!(unsafe { tpdf_editor_page_count(editor) }, 1);
        assert_eq!(unsafe { tpdf_editor_field_count(editor) }, 4);

        let mut name: *mut c_char = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_editor_field_name(editor, 0, &mut name) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { CStr::from_ptr(name) }.to_string_lossy(),
            "name",
            "the field list is readable after the document handle is gone"
        );
        unsafe { tpdf_string_free(name) };

        assert_eq!(
            unsafe { tpdf_editor_field_name(editor, 4, &mut name) },
            TpdfStatus::NoSuchField,
            "and an index past the end is named rather than guessed at"
        );
        unsafe { tpdf_editor_free(editor) };
    }

    /// The page operations, and what a refusal says.
    #[test]
    fn page_operations_cross_and_a_refusal_names_the_call() {
        let doc = open("simple-text.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        assert_eq!(unsafe { tpdf_editor_page_count(editor) }, 3);
        assert_eq!(
            unsafe { tpdf_editor_rotate_page(editor, 0, 90) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_editor_insert_page(editor, 3, 200.0, 100.0) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_editor_page_count(editor) },
            4,
            "the editor's own view of the page count moved with the edit"
        );
        assert_eq!(
            unsafe { tpdf_editor_move_page(editor, 3, 0) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_editor_delete_page(editor, 0) },
            TpdfStatus::Ok
        );
        assert_eq!(unsafe { tpdf_editor_page_count(editor) }, 3);
        assert_eq!(
            unsafe { tpdf_editor_set_crop_box(editor, 0, 10.0, 10.0, 100.0, 100.0) },
            TpdfStatus::Ok
        );

        // Refusals. The facade answers `bool`, so the status is the same for
        // each and the *message* is what tells them apart (ruling 10).
        assert_eq!(
            unsafe { tpdf_editor_delete_page(editor, 99) },
            TpdfStatus::EditRefused
        );
        let message = unsafe { CStr::from_ptr(tpdf_last_error_message()) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(message, "delete_page refused: index 99");

        assert_eq!(
            unsafe { tpdf_editor_set_crop_box(editor, 0, 10.0, 10.0, 10.0, 100.0) },
            TpdfStatus::EditRefused,
            "a crop box of no area is refused rather than written"
        );
        assert!(unsafe { CStr::from_ptr(tpdf_last_error_message()) }
            .to_string_lossy()
            .starts_with("set_crop_box refused:"));

        assert_eq!(
            unsafe { tpdf_editor_insert_page(editor, 0, f64::NAN, 100.0) },
            TpdfStatus::EditRefused,
            "and a dimension that is not a number reaches no file"
        );

        unsafe { tpdf_editor_free(editor) };
    }

    /// `tpdf_write_options_init` is the facade's defaults and not a second
    /// opinion about them, which is what stops a C caller writing a different
    /// file than a Rust caller with the same intent.
    #[test]
    fn the_default_write_options_are_the_facades_own() {
        let mut options = TpdfWriteOptions {
            mode: TpdfWriteMode::Incremental,
            linearize: 1,
            version_major: 9,
            version_minor: 9,
            object_streams: 1,
            compress: 0,
            garbage_collect: 1,
            encryption: ptr::null(),
        };
        assert_eq!(
            unsafe { tpdf_write_options_init(&mut options) },
            TpdfStatus::Ok
        );

        let facade = WriteOptions::default();
        assert_eq!(options.mode, TpdfWriteMode::Rewrite);
        assert_eq!(options.linearize, c_int::from(facade.linearize));
        assert_eq!(options.version_major, u32::from(facade.version.0));
        assert_eq!(options.version_minor, u32::from(facade.version.1));
        assert_eq!(options.object_streams, c_int::from(facade.object_streams));
        assert_eq!(options.compress, c_int::from(facade.compress));
        assert_eq!(options.garbage_collect, c_int::from(facade.garbage_collect));
        assert!(options.encryption.is_null());

        // And a rewrite through the C ABI equals a rewrite through the facade
        // with those same defaults -- the pair of assertions above says the
        // struct matches, and this says the struct is what is used.
        let doc = open("simple-text.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };
        let mut buffer: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_editor_save(editor, &options, &mut buffer) },
            TpdfStatus::Ok
        );
        let through_abi = take_buffer(buffer);
        unsafe { tpdf_editor_free(editor) };

        let document = Document::open(fixture("simple-text.pdf")).expect("it opens");
        let through_facade = document.editor().save(&WriteOptions::default());
        assert_eq!(through_abi, through_facade);
    }

    /// Encryption crosses field for field, and the 48 entropy bytes are the
    /// caller's -- refused rather than invented when they are the wrong
    /// length, because a short buffer read to 48 would encrypt with whatever
    /// followed it in the caller's address space.
    #[test]
    fn encryption_crosses_and_a_wrong_entropy_length_is_refused() {
        let entropy: Vec<u8> = (0..TPDF_ENTROPY_LEN).map(|i| (i * 7) as u8).collect();
        let user = c("open-sesame");
        let owner = c("owner-secret");

        let doc = open("simple-text.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };

        let mut options = incremental_options();
        options.mode = TpdfWriteMode::Rewrite;

        // Short entropy first: the refusal must come before anything is
        // written.
        let short = TpdfEncryption {
            user_password: user.as_ptr(),
            owner_password: owner.as_ptr(),
            permissions: -1,
            entropy: entropy.as_ptr(),
            entropy_len: TPDF_ENTROPY_LEN - 1,
        };
        options.encryption = &short;
        let mut buffer: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_editor_save(editor, &options, &mut buffer) },
            TpdfStatus::BadArgument
        );
        assert!(buffer.is_null(), "and nothing was allocated for it");

        let good = TpdfEncryption {
            user_password: user.as_ptr(),
            owner_password: owner.as_ptr(),
            permissions: -1,
            entropy: entropy.as_ptr(),
            entropy_len: TPDF_ENTROPY_LEN,
        };
        options.encryption = &good;
        assert_eq!(
            unsafe { tpdf_editor_save(editor, &options, &mut buffer) },
            TpdfStatus::Ok
        );
        let through_abi = take_buffer(buffer);
        unsafe { tpdf_editor_free(editor) };

        let mut fixed = [0u8; TPDF_ENTROPY_LEN];
        fixed.copy_from_slice(&entropy);
        let document = Document::open(fixture("simple-text.pdf")).expect("it opens");
        let through_facade = document.editor().save(&WriteOptions {
            encryption: Some(Encryption {
                user_password: "open-sesame".to_string(),
                owner_password: "owner-secret".to_string(),
                permissions: -1,
                entropy: fixed,
            }),
            ..WriteOptions::default()
        });
        assert_eq!(
            through_abi, through_facade,
            "fixed entropy is the one input that would otherwise vary, so with \
             it pinned the two sides are byte-identical"
        );

        let reopened = Document::open(through_abi).expect("the encrypted output opens");
        assert!(reopened.is_encrypted());
    }

    /// Null handles are refused rather than dereferenced, and every new free
    /// accepts null. The read surface has this test; the write surface needs
    /// its own, because none of these functions existed when that one was
    /// written.
    #[test]
    fn null_handles_across_the_write_surface_are_refused() {
        let name = c("name");
        let value = c("Ada");

        assert_eq!(
            unsafe { tpdf_document_editor(ptr::null(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(unsafe { tpdf_editor_is_dirty(ptr::null()) }, 0);
        assert_eq!(unsafe { tpdf_editor_page_count(ptr::null()) }, 0);
        assert_eq!(unsafe { tpdf_editor_field_count(ptr::null()) }, 0);
        assert_eq!(
            unsafe { tpdf_editor_delete_page(ptr::null_mut(), 0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_move_page(ptr::null_mut(), 0, 1) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_rotate_page(ptr::null_mut(), 0, 90) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_insert_page(ptr::null_mut(), 0, 10.0, 10.0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_set_crop_box(ptr::null_mut(), 0, 0.0, 0.0, 1.0, 1.0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_append_content(ptr::null_mut(), 0, ptr::null(), 0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(
                    ptr::null_mut(),
                    name.as_ptr(),
                    value.as_ptr(),
                    ptr::null_mut(),
                )
            },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_set_checkbox(ptr::null_mut(), name.as_ptr(), 1) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_select_radio(ptr::null_mut(), name.as_ptr(), value.as_ptr()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_checkpoint(ptr::null(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_restore(ptr::null_mut(), ptr::null()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_editor_save(ptr::null(), ptr::null(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_write_options_init(ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert!(unsafe { tpdf_buffer_data(ptr::null(), ptr::null_mut()) }.is_null());
        assert_eq!(unsafe { tpdf_buffer_len(ptr::null()) }, 0);
        assert_eq!(unsafe { tpdf_fill_report_count(ptr::null()) }, 0);
        assert_eq!(
            unsafe { tpdf_fill_report_message(ptr::null(), 0, ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_fill_report_widget(ptr::null(), 0, ptr::null_mut(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_fill_report_defect(ptr::null(), 0, ptr::null_mut()) },
            TpdfStatus::BadArgument
        );

        // A non-UTF-8 name is refused too, rather than lossily converted into
        // a name the caller did not ask for.
        let doc = open("form-fields.pdf");
        let editor = editor_over(doc);
        unsafe { tpdf_document_free(doc) };
        let invalid: [c_char; 3] = [-1i8 as c_char, -2i8 as c_char, 0];
        assert_eq!(
            unsafe {
                tpdf_editor_fill_field(editor, invalid.as_ptr(), value.as_ptr(), ptr::null_mut())
            },
            TpdfStatus::BadArgument
        );
        unsafe { tpdf_editor_free(editor) };

        // Every new free takes null and does nothing.
        unsafe { tpdf_editor_free(ptr::null_mut()) };
        unsafe { tpdf_checkpoint_free(ptr::null_mut()) };
        unsafe { tpdf_buffer_free(ptr::null_mut()) };
        unsafe { tpdf_fill_report_free(ptr::null_mut()) };
    }

    // -- writing: the builder surface (gap 32 milestone 3) ------------------

    /// The image the build-a-document script draws: eight by eight grey,
    /// generated from a formula so that four languages can produce the same 64
    /// bytes without a fixture file between them.
    ///
    /// That is not a convenience. A parity suite whose four surfaces read the
    /// same image *file* proves they can read a file; one whose four surfaces
    /// compute the same bytes proves the bytes are the same, which is what the
    /// hash is about.
    fn parity_image() -> Vec<u8> {
        (0..64u32).map(|i| ((i * 7) % 256) as u8).collect()
    }

    /// The build-a-document script, driven entirely through the C ABI.
    fn build_through_the_abi() -> Vec<u8> {
        let samples = parity_image();
        let mut builder: *mut TpdfBuilder = ptr::null_mut();
        assert_eq!(unsafe { tpdf_builder_new(&mut builder) }, TpdfStatus::Ok);

        let font = b"F1";
        let helvetica = b"Helvetica";
        assert_eq!(
            unsafe {
                tpdf_builder_add_base_font(
                    builder,
                    font.as_ptr(),
                    font.len(),
                    helvetica.as_ptr(),
                    helvetica.len(),
                )
            },
            TpdfStatus::Ok
        );

        let resource = b"Im1";
        let image = TpdfImage {
            kind: TpdfImageKind::Gray8,
            width: 8,
            height: 8,
            data: samples.as_ptr(),
            data_len: samples.len(),
        };
        assert_eq!(
            unsafe { tpdf_builder_add_image(builder, resource.as_ptr(), resource.len(), &image) },
            TpdfStatus::Ok
        );

        let mut one: *mut TpdfPageBuilder = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_begin_page(builder, 200.0, 200.0, &mut one) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe {
                tpdf_page_builder_text(
                    one,
                    font.as_ptr(),
                    font.len(),
                    14.0,
                    20.0,
                    170.0,
                    c("Page one").as_ptr(),
                )
            },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_page_builder_fill_rect(one, 20.0, 40.0, 60.0, 60.0, 0.25) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe {
                tpdf_page_builder_image(
                    one,
                    resource.as_ptr(),
                    resource.len(),
                    100.0,
                    40.0,
                    60.0,
                    60.0,
                )
            },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_builder_push_page(builder, one) },
            TpdfStatus::Ok
        );
        unsafe { tpdf_page_builder_free(one) };

        let mut two: *mut TpdfPageBuilder = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_begin_page(builder, 200.0, 200.0, &mut two) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe {
                tpdf_page_builder_text(
                    two,
                    font.as_ptr(),
                    font.len(),
                    14.0,
                    20.0,
                    170.0,
                    c("Page two").as_ptr(),
                )
            },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_builder_push_page(builder, two) },
            TpdfStatus::Ok
        );
        unsafe { tpdf_page_builder_free(two) };

        let title = b"Title";
        assert_eq!(
            unsafe {
                tpdf_builder_set_info(
                    builder,
                    title.as_ptr(),
                    title.len(),
                    c("tinker-pdf write parity").as_ptr(),
                )
            },
            TpdfStatus::Ok
        );

        let mut fit = TpdfDestination {
            kind: TpdfDestKind::Fit,
            left: 0.0,
            bottom: 0.0,
            right: 0.0,
            top: 0.0,
            zoom: 0.0,
        };
        assert_eq!(
            unsafe { tpdf_destination_init_fit(&mut fit) },
            TpdfStatus::Ok
        );

        let mut entries: Vec<*mut TpdfOutlineEntry> = Vec::new();
        for (index, label) in [(0u32, "Page one"), (1, "Page two")] {
            let mut entry: *mut TpdfOutlineEntry = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_outline_entry_new(c(label).as_ptr(), &mut entry) },
                TpdfStatus::Ok
            );
            let target = TpdfTarget {
                kind: TpdfTargetKind::Page,
                page_index: index,
                view: fit,
                uri: ptr::null(),
            };
            assert_eq!(
                unsafe { tpdf_outline_entry_set_target(entry, &target) },
                TpdfStatus::Ok
            );
            entries.push(entry);
        }
        assert_eq!(
            unsafe { tpdf_builder_set_outline(builder, entries.as_ptr(), entries.len()) },
            TpdfStatus::Ok
        );
        for entry in entries {
            unsafe { tpdf_outline_entry_free(entry) };
        }

        let mut buffer: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_finish(builder, &mut buffer) },
            TpdfStatus::Ok
        );
        let bytes = take_buffer(buffer);
        unsafe { tpdf_builder_free(builder) };
        bytes
    }

    /// The same script against the facade, in Rust.
    fn build_through_the_facade() -> Vec<u8> {
        let samples = parity_image();
        let mut builder = tinker_pdf::DocumentBuilder::new();
        builder.add_base_font(b"F1", b"Helvetica");
        assert!(builder.add_image(
            b"Im1",
            &ImageData::Gray8 {
                width: 8,
                height: 8,
                data: &samples,
            }
        ));

        let mut one = builder.begin_page(200.0, 200.0);
        one.text(b"F1", 14.0, 20.0, 170.0, "Page one");
        one.fill_rect(20.0, 40.0, 60.0, 60.0, 0.25);
        one.image(b"Im1", 100.0, 40.0, 60.0, 60.0);
        builder.push_page(one);

        let mut two = builder.begin_page(200.0, 200.0);
        two.text(b"F1", 14.0, 20.0, 170.0, "Page two");
        builder.push_page(two);

        builder.set_info(b"Title", "tinker-pdf write parity");
        assert!(builder.set_outline(
            [(0u32, "Page one"), (1, "Page two")]
                .into_iter()
                .map(|(index, title)| OutlineEntry {
                    title: title.to_string(),
                    target: Some(Target::Page {
                        index,
                        view: DestKind::Fit,
                    }),
                    open: false,
                    children: Vec::new(),
                })
                .collect()
        ));
        builder.finish()
    }

    /// The milestone's own exit criterion: **byte-equal**.
    #[test]
    fn building_a_document_through_the_abi_is_byte_equal_to_the_facade() {
        let through_abi = build_through_the_abi();
        let through_facade = build_through_the_facade();
        assert_eq!(
            through_abi.len(),
            through_facade.len(),
            "the two documents are not even the same length"
        );
        assert_eq!(through_abi, through_facade);

        // And it is a document rather than merely a matching pile of bytes.
        // Ruling 13: the check that the output is right is this engine's own.
        let document = Document::open(through_abi).expect("the built document opens");
        assert_eq!(document.page_count(), 2);
        assert!(
            document.validate().is_empty(),
            "the strict validator finds nothing: {:?}",
            document.validate()
        );
    }

    /// A consuming call takes the value and leaves the handle, so free stays
    /// symmetric -- and a second call says which call spent it.
    #[test]
    fn a_spent_builder_refuses_and_names_the_call_that_spent_it() {
        let mut builder: *mut TpdfBuilder = ptr::null_mut();
        assert_eq!(unsafe { tpdf_builder_new(&mut builder) }, TpdfStatus::Ok);
        let font = b"F1";
        let helvetica = b"Helvetica";
        assert_eq!(
            unsafe {
                tpdf_builder_add_base_font(
                    builder,
                    font.as_ptr(),
                    font.len(),
                    helvetica.as_ptr(),
                    helvetica.len(),
                )
            },
            TpdfStatus::Ok
        );

        let mut first: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_finish(builder, &mut first) },
            TpdfStatus::Ok
        );
        assert!(!first.is_null());
        drop(take_buffer(first));

        // The second finish is refused rather than producing a second
        // document or freeing the first one again.
        let mut second: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_finish(builder, &mut second) },
            TpdfStatus::SpentHandle
        );
        assert!(second.is_null(), "and nothing was written to the out slot");
        assert_eq!(
            unsafe { CStr::from_ptr(tpdf_last_error_message()) }.to_string_lossy(),
            "tpdf_builder_finish: this handle was already consumed by \
             tpdf_builder_finish"
        );

        // Every other call on the spent handle refuses the same way, naming
        // the call that spent it rather than the call that failed.
        assert_eq!(
            unsafe {
                tpdf_builder_add_base_font(
                    builder,
                    font.as_ptr(),
                    font.len(),
                    helvetica.as_ptr(),
                    helvetica.len(),
                )
            },
            TpdfStatus::SpentHandle
        );
        assert_eq!(
            unsafe { CStr::from_ptr(tpdf_last_error_message()) }.to_string_lossy(),
            "add_base_font: this handle was consumed by tpdf_builder_finish"
        );

        let mut page: *mut TpdfPageBuilder = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_begin_page(builder, 10.0, 10.0, &mut page) },
            TpdfStatus::SpentHandle
        );
        assert!(page.is_null());

        // And free is still required, still safe, and still the caller's job.
        unsafe { tpdf_builder_free(builder) };
    }

    /// Pushing a page twice is refused rather than duplicating it.
    #[test]
    fn a_spent_page_refuses_a_second_push() {
        let mut builder: *mut TpdfBuilder = ptr::null_mut();
        assert_eq!(unsafe { tpdf_builder_new(&mut builder) }, TpdfStatus::Ok);
        let font = b"F1";
        let helvetica = b"Helvetica";
        unsafe {
            tpdf_builder_add_base_font(
                builder,
                font.as_ptr(),
                font.len(),
                helvetica.as_ptr(),
                helvetica.len(),
            )
        };

        let mut page: *mut TpdfPageBuilder = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_begin_page(builder, 100.0, 100.0, &mut page) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_page_builder_fill_rect(page, 0.0, 0.0, 10.0, 10.0, 0.5) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_builder_push_page(builder, page) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_builder_push_page(builder, page) },
            TpdfStatus::SpentHandle
        );
        assert_eq!(
            unsafe { CStr::from_ptr(tpdf_last_error_message()) }.to_string_lossy(),
            "tpdf_builder_push_page: this handle was already consumed by \
             tpdf_builder_push_page"
        );
        // Drawing on a pushed page is refused too: the drawing is in the
        // document now, and a call that appeared to work would silently write
        // into nothing.
        assert_eq!(
            unsafe { tpdf_page_builder_fill_rect(page, 0.0, 0.0, 1.0, 1.0, 0.0) },
            TpdfStatus::SpentHandle
        );

        let mut buffer: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_finish(builder, &mut buffer) },
            TpdfStatus::Ok
        );
        let bytes = take_buffer(buffer);
        unsafe { tpdf_page_builder_free(page) };
        unsafe { tpdf_builder_free(builder) };

        let document = Document::open(bytes).expect("it opens");
        assert_eq!(document.page_count(), 1, "one page, not two");
    }

    /// A page begun and never pushed leaves no trace, so abandoning a handle
    /// is safe rather than merely non-fatal.
    #[test]
    fn a_page_that_is_never_pushed_leaves_no_trace_through_the_abi() {
        let build = |abandon: bool| {
            let mut builder: *mut TpdfBuilder = ptr::null_mut();
            assert_eq!(unsafe { tpdf_builder_new(&mut builder) }, TpdfStatus::Ok);
            let font = b"F1";
            let helvetica = b"Helvetica";
            unsafe {
                tpdf_builder_add_base_font(
                    builder,
                    font.as_ptr(),
                    font.len(),
                    helvetica.as_ptr(),
                    helvetica.len(),
                )
            };

            let mut kept: *mut TpdfPageBuilder = ptr::null_mut();
            unsafe { tpdf_builder_begin_page(builder, 100.0, 100.0, &mut kept) };
            unsafe {
                tpdf_page_builder_text(
                    kept,
                    font.as_ptr(),
                    font.len(),
                    12.0,
                    10.0,
                    50.0,
                    c("kept").as_ptr(),
                )
            };
            unsafe { tpdf_builder_push_page(builder, kept) };
            unsafe { tpdf_page_builder_free(kept) };

            if abandon {
                let mut lost: *mut TpdfPageBuilder = ptr::null_mut();
                unsafe { tpdf_builder_begin_page(builder, 400.0, 400.0, &mut lost) };
                unsafe {
                    tpdf_page_builder_text(
                        lost,
                        font.as_ptr(),
                        font.len(),
                        12.0,
                        10.0,
                        50.0,
                        c("abandoned").as_ptr(),
                    )
                };
                unsafe { tpdf_page_builder_free(lost) };
            }

            let mut buffer: *mut TpdfBuffer = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_builder_finish(builder, &mut buffer) },
                TpdfStatus::Ok
            );
            let bytes = take_buffer(buffer);
            unsafe { tpdf_builder_free(builder) };
            bytes
        };

        let with = build(true);
        let without = build(false);
        assert_eq!(with, without, "byte for byte, as if it had never begun");
        assert!(!String::from_utf8_lossy(&with).contains("abandoned"));
    }

    /// An outline entry is consumed by whichever call takes it into a tree,
    /// and the nesting the facade allows is the nesting that crosses.
    #[test]
    fn outline_entries_nest_and_are_consumed_once() {
        let mut builder: *mut TpdfBuilder = ptr::null_mut();
        assert_eq!(unsafe { tpdf_builder_new(&mut builder) }, TpdfStatus::Ok);
        let font = b"F1";
        let helvetica = b"Helvetica";
        unsafe {
            tpdf_builder_add_base_font(
                builder,
                font.as_ptr(),
                font.len(),
                helvetica.as_ptr(),
                helvetica.len(),
            )
        };
        let mut page: *mut TpdfPageBuilder = ptr::null_mut();
        unsafe { tpdf_builder_begin_page(builder, 100.0, 100.0, &mut page) };
        unsafe { tpdf_builder_push_page(builder, page) };
        unsafe { tpdf_page_builder_free(page) };

        let new = |title: &str| {
            let mut entry: *mut TpdfOutlineEntry = ptr::null_mut();
            assert_eq!(
                unsafe { tpdf_outline_entry_new(c(title).as_ptr(), &mut entry) },
                TpdfStatus::Ok
            );
            entry
        };

        let parent = new("Part one");
        let child = new("Chapter one");
        assert_eq!(
            unsafe { tpdf_outline_entry_set_open(parent, 1) },
            TpdfStatus::Ok
        );
        assert_eq!(
            unsafe { tpdf_outline_entry_add_child(parent, child) },
            TpdfStatus::Ok
        );
        // Adding it again is refused rather than making two of it.
        assert_eq!(
            unsafe { tpdf_outline_entry_add_child(parent, child) },
            TpdfStatus::SpentHandle
        );
        // And an entry cannot be its own child, which would otherwise be a
        // take from a handle that is being borrowed.
        assert_eq!(
            unsafe { tpdf_outline_entry_add_child(parent, parent) },
            TpdfStatus::BadArgument
        );

        let tops = [parent];
        assert_eq!(
            unsafe { tpdf_builder_set_outline(builder, tops.as_ptr(), tops.len()) },
            TpdfStatus::Ok
        );
        unsafe { tpdf_outline_entry_free(parent) };
        unsafe { tpdf_outline_entry_free(child) };

        let mut buffer: *mut TpdfBuffer = ptr::null_mut();
        assert_eq!(
            unsafe { tpdf_builder_finish(builder, &mut buffer) },
            TpdfStatus::Ok
        );
        let bytes = take_buffer(buffer);
        unsafe { tpdf_builder_free(builder) };

        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("(Part one)"), "the parent is in the outline");
        assert!(text.contains("(Chapter one)"), "and so is the child");
    }

    /// All eight destination kinds cross, and NaN is `null` -- 12.3.2.2's
    /// "retain the current value" -- rather than a coordinate.
    #[test]
    fn every_destination_kind_crosses_and_nan_means_null() {
        let each = [
            (TpdfDestKind::Xyz, 0),
            (TpdfDestKind::Fit, 1),
            (TpdfDestKind::FitH, 2),
            (TpdfDestKind::FitV, 3),
            (TpdfDestKind::FitR, 4),
            (TpdfDestKind::FitB, 5),
            (TpdfDestKind::FitBH, 6),
            (TpdfDestKind::FitBV, 7),
        ];
        for (kind, number) in each {
            assert_eq!(kind as i32, number, "{kind:?}");
        }

        let with = |kind, left: f64, top: f64, zoom: f64| {
            TpdfDestination {
                kind,
                left,
                bottom: 1.0,
                right: 2.0,
                top,
                zoom,
            }
            .to_facade()
        };

        assert_eq!(
            with(TpdfDestKind::Xyz, 10.0, 20.0, f64::NAN),
            DestKind::Xyz {
                left: Some(10.0),
                top: Some(20.0),
                zoom: None,
            },
            "a NaN zoom is /XYZ's null, which is not the same as a zoom of 0"
        );
        assert_eq!(
            with(TpdfDestKind::Xyz, f64::NAN, f64::NAN, f64::NAN),
            DestKind::Xyz {
                left: None,
                top: None,
                zoom: None,
            }
        );
        assert_eq!(
            with(TpdfDestKind::Fit, f64::NAN, f64::NAN, f64::NAN),
            DestKind::Fit
        );
        assert_eq!(
            with(TpdfDestKind::FitH, f64::NAN, 5.0, f64::NAN),
            DestKind::FitH { top: Some(5.0) }
        );
        assert_eq!(
            with(TpdfDestKind::FitV, 5.0, f64::NAN, f64::NAN),
            DestKind::FitV { left: Some(5.0) }
        );
        assert_eq!(
            with(TpdfDestKind::FitR, 3.0, 4.0, f64::NAN),
            DestKind::FitR {
                left: 3.0,
                bottom: 1.0,
                right: 2.0,
                top: 4.0,
            }
        );
        assert_eq!(
            with(TpdfDestKind::FitB, f64::NAN, f64::NAN, f64::NAN),
            DestKind::FitB
        );
        assert_eq!(
            with(TpdfDestKind::FitBH, f64::NAN, 7.0, f64::NAN),
            DestKind::FitBH { top: Some(7.0) }
        );
        assert_eq!(
            with(TpdfDestKind::FitBV, 7.0, f64::NAN, f64::NAN),
            DestKind::FitBV { left: Some(7.0) }
        );

        // And the initialiser is /Fit rather than a zeroed struct, which would
        // be /XYZ 0 0 0 -- a different destination that merely looks default.
        let mut fit = TpdfDestination {
            kind: TpdfDestKind::Xyz,
            left: 0.0,
            bottom: 0.0,
            right: 0.0,
            top: 0.0,
            zoom: 0.0,
        };
        assert_eq!(
            unsafe { tpdf_destination_init_fit(&mut fit) },
            TpdfStatus::Ok
        );
        assert_eq!(fit.to_facade(), DestKind::Fit);
    }

    /// Null handles across the builder surface, and every new free taking
    /// null.
    #[test]
    fn null_handles_across_the_builder_surface_are_refused() {
        let name = b"F1";
        let target = TpdfTarget {
            kind: TpdfTargetKind::Page,
            page_index: 0,
            view: TpdfDestination {
                kind: TpdfDestKind::Fit,
                left: f64::NAN,
                bottom: f64::NAN,
                right: f64::NAN,
                top: f64::NAN,
                zoom: f64::NAN,
            },
            uri: ptr::null(),
        };
        let image = TpdfImage {
            kind: TpdfImageKind::Gray8,
            width: 1,
            height: 1,
            data: ptr::null(),
            data_len: 1,
        };

        assert_eq!(
            unsafe { tpdf_builder_new(ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe {
                tpdf_builder_add_base_font(
                    ptr::null_mut(),
                    name.as_ptr(),
                    name.len(),
                    name.as_ptr(),
                    name.len(),
                )
            },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe {
                tpdf_builder_add_embedded_font(
                    ptr::null_mut(),
                    name.as_ptr(),
                    name.len(),
                    name.as_ptr(),
                    name.len(),
                    name.as_ptr(),
                    name.len(),
                )
            },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_builder_set_subset_fonts(ptr::null_mut(), 1) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_builder_add_image(ptr::null_mut(), name.as_ptr(), name.len(), &image) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe {
                tpdf_builder_set_info(ptr::null_mut(), name.as_ptr(), name.len(), ptr::null())
            },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_builder_begin_page(ptr::null_mut(), 1.0, 1.0, ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_builder_push_page(ptr::null_mut(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_builder_set_outline(ptr::null_mut(), ptr::null(), 0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_builder_finish(ptr::null_mut(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe {
                tpdf_page_builder_text(
                    ptr::null_mut(),
                    name.as_ptr(),
                    name.len(),
                    1.0,
                    0.0,
                    0.0,
                    ptr::null(),
                )
            },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_page_builder_fill_rect(ptr::null_mut(), 0.0, 0.0, 1.0, 1.0, 0.5) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe {
                tpdf_page_builder_image(
                    ptr::null_mut(),
                    name.as_ptr(),
                    name.len(),
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                )
            },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_page_builder_set_fill_rgb(ptr::null_mut(), 0.0, 0.0, 0.0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_page_builder_set_stroke_rgb(ptr::null_mut(), 0.0, 0.0, 0.0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_page_builder_set_crop_box(ptr::null_mut(), 0.0, 0.0, 1.0, 1.0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_page_builder_raw(ptr::null_mut(), ptr::null(), 0) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_page_builder_link(ptr::null_mut(), 0.0, 0.0, 1.0, 1.0, &target) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_outline_entry_new(ptr::null(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_outline_entry_set_target(ptr::null_mut(), &target) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_outline_entry_set_open(ptr::null_mut(), 1) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_outline_entry_add_child(ptr::null_mut(), ptr::null_mut()) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_destination_init_fit(ptr::null_mut()) },
            TpdfStatus::BadArgument
        );

        // A null image payload is refused rather than read.
        let mut builder: *mut TpdfBuilder = ptr::null_mut();
        assert_eq!(unsafe { tpdf_builder_new(&mut builder) }, TpdfStatus::Ok);
        assert_eq!(
            unsafe { tpdf_builder_add_image(builder, name.as_ptr(), name.len(), &image) },
            TpdfStatus::BadArgument
        );
        assert_eq!(
            unsafe { tpdf_builder_add_image(builder, name.as_ptr(), name.len(), ptr::null()) },
            TpdfStatus::BadArgument
        );
        unsafe { tpdf_builder_free(builder) };

        unsafe { tpdf_builder_free(ptr::null_mut()) };
        unsafe { tpdf_page_builder_free(ptr::null_mut()) };
        unsafe { tpdf_outline_entry_free(ptr::null_mut()) };
    }
}
