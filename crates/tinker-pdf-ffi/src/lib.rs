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
    AuthLevel, Bitmap, Chain, CmsState, Coverage, Document, DocumentDigest, PixelFormat,
    RenderOptions, Signature, SignatureCheck, SimpleFontProvider, TrustAnchors, Verdict, Weakness,
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
}
