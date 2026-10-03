//! The read surface beyond rendering, text and signatures: `/Info` and the
//! version, page labels, the outline, a page's links, attachments, the XMP
//! packet, and the warnings an open tolerated.
//!
//! Every function here is a projection of one facade call (ruling 11):
//! `Document::metadata`, `pdf_version`, `page_labels`, `outline`,
//! `Page::links`, `attachments`, `xmp_metadata` and `warnings`. The one place
//! two calls meet is an attachment's bytes, which the facade documents as
//! "the stream reference to read through `Document::cos`" — so
//! [`tpdf_attachment_data`] is that reference handed to
//! `CosDocument::stream_decoded`, the route the facade names, and nothing
//! else.
//!
//! Lists cross as owned handles on the [`crate::TpdfSignatures`] pattern: the
//! engine's own copy, so a handle outlives the document it came from and is
//! freed independently. An index past the end is [`TpdfStatus::BadArgument`],
//! and a null written on [`TpdfStatus::Ok`] means *the document does not say*
//! — the two are different answers and are kept apart, as they are across the
//! signature surface.

use std::ffi::{c_char, c_int};
use std::ptr;

use tinker_pdf::{Action, Attachment, DestKind, Destination, Document, Link, OutlineItem, Trapped};

use crate::{
    count, hand_over_string, set_error, TpdfBuffer, TpdfDestKind, TpdfDestination, TpdfDocument,
    TpdfStatus,
};

/// Which `/Info` entry to read (14.3.3, Table 349).
///
/// `/Trapped` is not here because it is a name rather than a text string; it
/// has [`tpdf_document_trapped`] and its own enum.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfInfoKey {
    /// `/Title`.
    Title = 0,
    /// `/Author`.
    Author = 1,
    /// `/Subject`.
    Subject = 2,
    /// `/Keywords`.
    Keywords = 3,
    /// `/Creator`: the application that authored the original document.
    Creator = 4,
    /// `/Producer`: the application that wrote the PDF.
    Producer = 5,
    /// `/CreationDate`, as written rather than parsed.
    CreationDate = 6,
    /// `/ModDate`, as written rather than parsed.
    ModificationDate = 7,
}

raw_enum!(TpdfInfoKey {
    Title,
    Author,
    Subject,
    Keywords,
    Creator,
    Producer,
    CreationDate,
    ModificationDate
});

/// `/Trapped` (Table 349), with its absence spelled out.
///
/// The facade's `Option<Trapped>` carries two facts that a three-arm enum
/// would merge: `Absent` is the key missing, and `Unknown` is the document
/// answering `/Unknown` (or a name outside the three, which reads the same).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfTrapped {
    /// No `/Trapped` entry, or no `/Info` at all.
    Absent = 0,
    /// `/True`.
    True = 1,
    /// `/False`.
    False = 2,
    /// `/Unknown`, or a name that is not one of the three.
    Unknown = 3,
}

raw_enum!(TpdfTrapped {
    Absent,
    True,
    False,
    Unknown
});

/// Which `Destination` arm an outline entry or a link names (12.3.2).
///
/// Ruling 6 is why three arms cross rather than a page number: a named
/// destination is not flattened to the page it resolves to, and a URI is
/// never read as a name.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfDestinationKind {
    /// No destination at all: an outline heading that points nowhere, or an
    /// action that carries none.
    Absent = 0,
    /// A page in this document and a view of it.
    Explicit = 1,
    /// A name to look up in the document's own tables; the bytes cross
    /// through the `_destination_bytes` accessor.
    Named = 2,
    /// A URI destination; the bytes cross through the `_destination_bytes`
    /// accessor.
    Uri = 3,
}

/// Which `Action` arm a link carries (12.6.4).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfActionKind {
    /// A `/Link` with neither `/Dest` nor a usable `/A` — legal, useless, and
    /// reported rather than hidden.
    Absent = 0,
    /// `/GoTo`: the destination is filled in.
    GoTo = 1,
    /// `/GoToR`: the destination is filled in when the action carried one,
    /// and the target file crosses through [`tpdf_link_action_bytes`].
    GoToR = 2,
    /// `/URI`; the URI crosses through [`tpdf_link_action_bytes`].
    Uri = 3,
    /// `/Named`, a viewer command; its name crosses through
    /// [`tpdf_link_action_bytes`].
    Named = 4,
    /// `/Launch`: reported, never executed. The file crosses through
    /// [`tpdf_link_action_bytes`].
    Launch = 5,
    /// Any other action type, kept rather than dropped; its `/S` crosses
    /// through [`tpdf_link_action_bytes`].
    Other = 6,
}

/// A destination as read, which is `Destination` in C.
///
/// The view is the write side's [`TpdfDestination`], NaN meaning `null`
/// exactly as it does there, so a destination written through
/// `tpdf_page_builder_link` and read back here is the same struct. For any
/// kind but [`TpdfDestinationKind::Explicit`] the view is `/Fit` with every
/// number NaN, which is [`crate::tpdf_destination_init_fit`]'s answer and
/// means nothing.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfDestinationRead {
    /// Which arm.
    pub kind: TpdfDestinationKind,
    /// 1 when the explicit destination's page resolved to an index.
    pub has_page_index: c_int,
    /// The zero-based page index, when `has_page_index` is 1.
    pub page_index: u32,
    /// 1 when the explicit destination named its page by reference. Kept
    /// whether or not the reference resolved, which is the facade's own
    /// arrangement: an unresolvable page is still the object the file named.
    pub has_page_ref: c_int,
    /// The page's object number, when `has_page_ref` is 1.
    pub page_object: u32,
    /// The page's generation, when `has_page_ref` is 1.
    pub page_generation: u16,
    /// How the page is positioned.
    pub view: TpdfDestination,
}

/// The NaN-for-`null` spelling of an `Option<f64>`.
fn nullable(value: Option<f64>) -> f64 {
    value.unwrap_or(f64::NAN)
}

/// The write side's [`TpdfDestination`], from a view the reader produced.
///
/// The inverse of `TpdfDestination::to_facade`, so a destination crosses the
/// same struct in both directions.
fn view_of(kind: &DestKind) -> TpdfDestination {
    let mut view = TpdfDestination {
        kind: TpdfDestKind::Fit as c_int,
        left: f64::NAN,
        bottom: f64::NAN,
        right: f64::NAN,
        top: f64::NAN,
        zoom: f64::NAN,
    };
    match *kind {
        DestKind::Xyz { left, top, zoom } => {
            view.kind = TpdfDestKind::Xyz as c_int;
            view.left = nullable(left);
            view.top = nullable(top);
            view.zoom = nullable(zoom);
        }
        DestKind::Fit => {}
        DestKind::FitH { top } => {
            view.kind = TpdfDestKind::FitH as c_int;
            view.top = nullable(top);
        }
        DestKind::FitV { left } => {
            view.kind = TpdfDestKind::FitV as c_int;
            view.left = nullable(left);
        }
        DestKind::FitR {
            left,
            bottom,
            right,
            top,
        } => {
            view.kind = TpdfDestKind::FitR as c_int;
            view.left = left;
            view.bottom = bottom;
            view.right = right;
            view.top = top;
        }
        DestKind::FitB => view.kind = TpdfDestKind::FitB as c_int,
        DestKind::FitBH { top } => {
            view.kind = TpdfDestKind::FitBH as c_int;
            view.top = nullable(top);
        }
        DestKind::FitBV { left } => {
            view.kind = TpdfDestKind::FitBV as c_int;
            view.left = nullable(left);
        }
    }
    view
}

/// A facade destination, as C sees it.
fn destination_read(destination: Option<&Destination>) -> TpdfDestinationRead {
    let mut read = TpdfDestinationRead {
        kind: TpdfDestinationKind::Absent,
        has_page_index: 0,
        page_index: 0,
        has_page_ref: 0,
        page_object: 0,
        page_generation: 0,
        view: view_of(&DestKind::Fit),
    };
    match destination {
        None => {}
        Some(Destination::Explicit {
            page_index,
            page_ref,
            kind,
        }) => {
            read.kind = TpdfDestinationKind::Explicit;
            if let Some(index) = page_index {
                read.has_page_index = 1;
                read.page_index = *index;
            }
            if let Some(reference) = page_ref {
                read.has_page_ref = 1;
                read.page_object = reference.num;
                read.page_generation = reference.gen;
            }
            read.view = view_of(kind);
        }
        Some(Destination::Named(_)) => read.kind = TpdfDestinationKind::Named,
        Some(Destination::Uri(_)) => read.kind = TpdfDestinationKind::Uri,
    }
    read
}

/// The bytes a named or URI destination carries; `None` for any other.
fn destination_bytes(destination: Option<&Destination>) -> Option<&[u8]> {
    match destination {
        Some(Destination::Named(name)) => Some(name),
        Some(Destination::Uri(uri)) => Some(uri),
        _ => None,
    }
}

/// Hands a borrowed byte slice to C, or writes null for "the document does
/// not say".
///
/// # Safety
///
/// `out_data` and `out_len` must be valid pointers. The slice must live as
/// long as the handle it was borrowed from, which at every call site here is
/// the handle the caller passed in.
unsafe fn hand_over_bytes(
    out_data: *mut *const u8,
    out_len: *mut usize,
    value: Option<&[u8]>,
) -> TpdfStatus {
    if out_data.is_null() || out_len.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    match value {
        Some(bytes) => unsafe {
            *out_data = bytes.as_ptr();
            *out_len = bytes.len();
        },
        None => unsafe {
            *out_data = ptr::null();
            *out_len = 0;
        },
    }
    TpdfStatus::Ok
}

/// A live document, or the refusal.
unsafe fn document<'a>(doc: *const TpdfDocument) -> Result<&'a Document, TpdfStatus> {
    match unsafe { doc.as_ref() } {
        Some(doc) => Ok(&doc.inner),
        None => {
            set_error("null document");
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// One entry of an owned list, or the refusal that names which list.
unsafe fn entry<'a, H: 'a, T>(
    handle: *const H,
    list: impl FnOnce(&'a H) -> &'a [T],
    index: u32,
    what: &str,
) -> Result<&'a T, TpdfStatus> {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        set_error(&format!("null {what} handle"));
        return Err(TpdfStatus::BadArgument);
    };
    match list(handle).get(index as usize) {
        Some(item) => Ok(item),
        None => {
            set_error(&format!("no such {what}: index {index}"));
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// Writes `value` through `out` when `out` is not null.
unsafe fn put<T>(out: *mut T, value: T) {
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = value;
    }
}

// ---- /Info, the version, page labels, XMP -----------------------------------

/// One `/Info` text entry (14.3.3), decoded. `key` is a [`TpdfInfoKey`]; any
/// other number is [`TpdfStatus::BadArgument`].
///
/// **Null on `Ok` means the entry is absent**, and an empty string means the
/// producer wrote an empty one — the facade keeps those apart because a
/// viewer showing "(untitled)" needs to know which, and so does this. The
/// caller frees a non-null result with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_info(
    doc: *const TpdfDocument,
    key: c_int,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    let key = match TpdfInfoKey::checked(key, "info key") {
        Ok(key) => key,
        Err(status) => return status,
    };
    let metadata = doc.metadata();
    let value = match key {
        TpdfInfoKey::Title => metadata.title,
        TpdfInfoKey::Author => metadata.author,
        TpdfInfoKey::Subject => metadata.subject,
        TpdfInfoKey::Keywords => metadata.keywords,
        TpdfInfoKey::Creator => metadata.creator,
        TpdfInfoKey::Producer => metadata.producer,
        TpdfInfoKey::CreationDate => metadata.creation_date,
        TpdfInfoKey::ModificationDate => metadata.modification_date,
    };
    unsafe { hand_over_string(out, value.as_deref()) }
}

/// `/Info /Trapped` (Table 349), with absence as its own answer.
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_trapped(
    doc: *const TpdfDocument,
    out: *mut TpdfTrapped,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = match doc.metadata().trapped {
        None => TpdfTrapped::Absent,
        Some(Trapped::True) => TpdfTrapped::True,
        Some(Trapped::False) => TpdfTrapped::False,
        Some(Trapped::Unknown) => TpdfTrapped::Unknown,
    };
    TpdfStatus::Ok
}

/// The version, as "PDF 1.7": the later of the header's (7.5.2) and the
/// catalog's (7.7.2), never absent.
///
/// The caller frees it with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_pdf_version(
    doc: *const TpdfDocument,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    unsafe { hand_over_string(out, Some(&doc.pdf_version())) }
}

/// One page's label (12.4.2).
///
/// Null on `Ok` when the document defines no labels at all, which is
/// `Document::page_labels` answering with an empty list; a page past the end
/// is [`TpdfStatus::NoSuchPage`]. The caller frees a non-null result with
/// [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_page_label(
    doc: *const TpdfDocument,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    if index >= doc.page_count() {
        set_error(&format!("no such page: index {index}"));
        return TpdfStatus::NoSuchPage;
    }
    let labels = doc.page_labels();
    unsafe { hand_over_string(out, labels.get(index as usize).map(String::as_str)) }
}

/// The document's XMP packet (14.3.2), unparsed.
///
/// Null on `Ok` when the catalog names no metadata stream. Otherwise a
/// [`TpdfBuffer`] the caller frees with [`crate::tpdf_buffer_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_xmp_metadata(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let handle = match doc.xmp_metadata() {
        Some(bytes) => Box::into_raw(Box::new(TpdfBuffer { inner: bytes })),
        None => ptr::null_mut(),
    };
    unsafe { *out = handle };
    TpdfStatus::Ok
}

// ---- the outline (12.3.3) ----------------------------------------------------

/// A document's outline, flattened to reading order. Opaque to callers.
///
/// Flattened rather than a tree of handles because a tree of handles is a
/// tree of frees: each entry carries its depth, which is
/// `OutlineItem::flatten`'s own answer, and the nesting is recovered from it.
pub struct TpdfOutline {
    inner: Vec<(u32, OutlineItem)>,
}

/// Reads the outline tree; an empty handle when the document has none, which
/// is not an error.
///
/// The caller frees the handle with [`tpdf_outline_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_outline(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfOutline,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let tree = doc.outline();
    let inner = OutlineItem::flatten(&tree)
        .into_iter()
        .map(|(depth, item)| {
            // The children are the entries that follow at a greater depth,
            // so each flattened entry carries none of its own: copying them
            // would make the handle quadratic in the outline's depth.
            let entry = OutlineItem {
                title: item.title.clone(),
                destination: item.destination.clone(),
                open: item.open,
                children: Vec::new(),
            };
            (depth, entry)
        })
        .collect();
    unsafe { *out = Box::into_raw(Box::new(TpdfOutline { inner })) };
    TpdfStatus::Ok
}

/// How many entries the outline holds, at every depth, or zero for null.
///
/// # Safety
///
/// `outline` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_count(outline: *const TpdfOutline) -> u32 {
    unsafe { outline.as_ref() }.map_or(0, |o| count(o.inner.len()))
}

/// One entry's depth (0 for a top-level entry) and whether it was saved
/// expanded (`/Count` positive).
///
/// # Safety
///
/// `outline` must be a live handle; either out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_item(
    outline: *const TpdfOutline,
    index: u32,
    out_depth: *mut u32,
    out_open: *mut c_int,
) -> TpdfStatus {
    match unsafe { entry(outline, |o: &TpdfOutline| &o.inner, index, "outline entry") } {
        Ok((depth, item)) => {
            unsafe {
                put(out_depth, *depth);
                put(out_open, c_int::from(item.open));
            }
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// One entry's title, decoded. The caller frees it with
/// [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `outline` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_title(
    outline: *const TpdfOutline,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { entry(outline, |o: &TpdfOutline| &o.inner, index, "outline entry") } {
        Ok((_, item)) => unsafe { hand_over_string(out, Some(&item.title)) },
        Err(status) => status,
    }
}

/// Where one entry goes. [`TpdfDestinationKind::Absent`] is an entry that is
/// only a heading.
///
/// # Safety
///
/// `outline` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_destination(
    outline: *const TpdfOutline,
    index: u32,
    out: *mut TpdfDestinationRead,
) -> TpdfStatus {
    let (_, item) =
        match unsafe { entry(outline, |o: &TpdfOutline| &o.inner, index, "outline entry") } {
            Ok(found) => found,
            Err(status) => return status,
        };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = destination_read(item.destination.as_ref());
    TpdfStatus::Ok
}

/// The bytes of a named or URI destination, borrowed until the outline is
/// freed; null on `Ok` for an explicit destination or none.
///
/// Bytes rather than a string because 12.3.2.3 makes a name a byte string,
/// and a URI is 7-bit ASCII only when the producer obeyed 12.6.4.7.
///
/// # Safety
///
/// `outline` must be a live handle and both out pointers valid.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_destination_bytes(
    outline: *const TpdfOutline,
    index: u32,
    out_data: *mut *const u8,
    out_len: *mut usize,
) -> TpdfStatus {
    match unsafe { entry(outline, |o: &TpdfOutline| &o.inner, index, "outline entry") } {
        Ok((_, item)) => unsafe {
            hand_over_bytes(
                out_data,
                out_len,
                destination_bytes(item.destination.as_ref()),
            )
        },
        Err(status) => status,
    }
}

/// Frees an outline. Null is accepted and does nothing.
///
/// # Safety
///
/// `outline` must have come from [`tpdf_document_outline`] and must not be
/// used afterwards, nor any bytes borrowed from it.
#[no_mangle]
pub unsafe extern "C" fn tpdf_outline_free(outline: *mut TpdfOutline) {
    if !outline.is_null() {
        drop(unsafe { Box::from_raw(outline) });
    }
}

// ---- a page's links (12.5.6.5) ----------------------------------------------

/// A page's link annotations, in `/Annots` order. Opaque to callers.
pub struct TpdfLinks {
    inner: Vec<Link>,
}

/// Reads a page's link annotations, with their targets resolved.
///
/// Only `/Link`: every other subtype belongs to the annotation model. The
/// caller frees the handle with [`tpdf_links_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_links(
    doc: *const TpdfDocument,
    index: u32,
    out: *mut *mut TpdfLinks,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let Some(page) = doc.page(index) else {
        set_error("no such page");
        return TpdfStatus::NoSuchPage;
    };
    unsafe {
        *out = Box::into_raw(Box::new(TpdfLinks {
            inner: page.links(),
        }));
    }
    TpdfStatus::Ok
}

/// How many links the handle holds, or zero for null.
///
/// # Safety
///
/// `links` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_links_count(links: *const TpdfLinks) -> u32 {
    unsafe { links.as_ref() }.map_or(0, |l| count(l.inner.len()))
}

/// One link's `/Rect`, corners ordered.
///
/// # Safety
///
/// `links` must be a live handle; any out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_link_rect(
    links: *const TpdfLinks,
    index: u32,
    out_x0: *mut f64,
    out_y0: *mut f64,
    out_x1: *mut f64,
    out_y1: *mut f64,
) -> TpdfStatus {
    match unsafe { entry(links, |l: &TpdfLinks| &l.inner, index, "link") } {
        Ok(link) => {
            unsafe {
                put(out_x0, link.rect.x0);
                put(out_y0, link.rect.y0);
                put(out_x1, link.rect.x1);
                put(out_y1, link.rect.y1);
            }
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// The annotation object a link is, when `/Annots` named it indirectly
/// (ruling 10: what was read is addressable). `out_present` is 0 for a link
/// written inline in the array.
///
/// # Safety
///
/// `links` must be a live handle; any out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_link_reference(
    links: *const TpdfLinks,
    index: u32,
    out_present: *mut c_int,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    match unsafe { entry(links, |l: &TpdfLinks| &l.inner, index, "link") } {
        Ok(link) => {
            unsafe {
                put(out_present, c_int::from(link.reference.is_some()));
                put(out_object, link.reference.map_or(0, |r| r.num));
                put(out_generation, link.reference.map_or(0, |r| r.gen));
            }
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// What a link does: the action's arm, and the destination when the action
/// carries one (`/GoTo` always, `/GoToR` when it says).
///
/// # Safety
///
/// `links` must be a live handle and both out pointers valid.
#[no_mangle]
pub unsafe extern "C" fn tpdf_link_action(
    links: *const TpdfLinks,
    index: u32,
    out_kind: *mut TpdfActionKind,
    out_destination: *mut TpdfDestinationRead,
) -> TpdfStatus {
    let link = match unsafe { entry(links, |l: &TpdfLinks| &l.inner, index, "link") } {
        Ok(link) => link,
        Err(status) => return status,
    };
    let (Some(kind), Some(destination)) = (unsafe { out_kind.as_mut() }, unsafe {
        out_destination.as_mut()
    }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let (arm, carried) = match &link.target {
        None => (TpdfActionKind::Absent, None),
        Some(Action::GoTo(dest)) => (TpdfActionKind::GoTo, Some(dest)),
        Some(Action::GoToR { dest, .. }) => (TpdfActionKind::GoToR, dest.as_ref()),
        Some(Action::Uri(_)) => (TpdfActionKind::Uri, None),
        Some(Action::Named(_)) => (TpdfActionKind::Named, None),
        Some(Action::Launch { .. }) => (TpdfActionKind::Launch, None),
        Some(Action::Other { .. }) => (TpdfActionKind::Other, None),
    };
    *kind = arm;
    *destination = destination_read(carried);
    TpdfStatus::Ok
}

/// The action's own bytes, borrowed until the links are freed: the URI of a
/// `/URI`, the name of a `/Named`, the `/S` of any other type, and the file
/// of a `/GoToR` or `/Launch`. Null on `Ok` when the action has none.
///
/// # Safety
///
/// `links` must be a live handle and both out pointers valid.
#[no_mangle]
pub unsafe extern "C" fn tpdf_link_action_bytes(
    links: *const TpdfLinks,
    index: u32,
    out_data: *mut *const u8,
    out_len: *mut usize,
) -> TpdfStatus {
    let link = match unsafe { entry(links, |l: &TpdfLinks| &l.inner, index, "link") } {
        Ok(link) => link,
        Err(status) => return status,
    };
    let bytes = match &link.target {
        Some(Action::Uri(uri)) => Some(uri.as_slice()),
        Some(Action::Named(name)) => Some(name.as_slice()),
        Some(Action::Other { subtype }) => Some(subtype.as_slice()),
        Some(Action::GoToR { file, .. }) | Some(Action::Launch { file }) => file.as_deref(),
        Some(Action::GoTo(_)) | None => None,
    };
    unsafe { hand_over_bytes(out_data, out_len, bytes) }
}

/// The bytes of the link's named or URI destination, borrowed until the
/// links are freed; null on `Ok` for an explicit destination or none.
///
/// # Safety
///
/// `links` must be a live handle and both out pointers valid.
#[no_mangle]
pub unsafe extern "C" fn tpdf_link_destination_bytes(
    links: *const TpdfLinks,
    index: u32,
    out_data: *mut *const u8,
    out_len: *mut usize,
) -> TpdfStatus {
    let link = match unsafe { entry(links, |l: &TpdfLinks| &l.inner, index, "link") } {
        Ok(link) => link,
        Err(status) => return status,
    };
    let carried = match &link.target {
        Some(Action::GoTo(dest)) => Some(dest),
        Some(Action::GoToR { dest, .. }) => dest.as_ref(),
        _ => None,
    };
    unsafe { hand_over_bytes(out_data, out_len, destination_bytes(carried)) }
}

/// Frees a links handle. Null is accepted and does nothing.
///
/// # Safety
///
/// `links` must have come from [`tpdf_page_links`] and must not be used
/// afterwards, nor any bytes borrowed from it.
#[no_mangle]
pub unsafe extern "C" fn tpdf_links_free(links: *mut TpdfLinks) {
    if !links.is_null() {
        drop(unsafe { Box::from_raw(links) });
    }
}

// ---- attachments (7.11.4) ---------------------------------------------------

/// Every file attached to a document, in name order. Opaque to callers.
///
/// Holds its own clone of the document — an `Arc` bump, not a copy — because
/// listing what a document carries does not read the bytes, and
/// [`tpdf_attachment_data`] reads them only when asked. So this, too,
/// outlives the [`TpdfDocument`] it came from.
pub struct TpdfAttachments {
    document: Document,
    inner: Vec<Attachment>,
}

/// Lists the document's attachments; an empty handle when it has none.
///
/// The caller frees the handle with [`tpdf_attachments_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_attachments(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfAttachments,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let handle = TpdfAttachments {
        document: doc.clone(),
        inner: doc.attachments(),
    };
    unsafe { *out = Box::into_raw(Box::new(handle)) };
    TpdfStatus::Ok
}

/// How many attachments the handle holds, or zero for null.
///
/// # Safety
///
/// `attachments` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_attachments_count(attachments: *const TpdfAttachments) -> u32 {
    unsafe { attachments.as_ref() }.map_or(0, |a| count(a.inner.len()))
}

/// The name an attachment is filed under in `/Names /EmbeddedFiles`. The
/// caller frees it with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `attachments` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_attachment_name(
    attachments: *const TpdfAttachments,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe {
        entry(
            attachments,
            |a: &TpdfAttachments| &a.inner,
            index,
            "attachment",
        )
    } {
        Ok(attachment) => unsafe { hand_over_string(out, Some(&attachment.name)) },
        Err(status) => status,
    }
}

/// `/UF` or `/F`: the filename to offer when saving it out. The caller frees
/// it with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `attachments` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_attachment_filename(
    attachments: *const TpdfAttachments,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe {
        entry(
            attachments,
            |a: &TpdfAttachments| &a.inner,
            index,
            "attachment",
        )
    } {
        Ok(attachment) => unsafe { hand_over_string(out, Some(&attachment.filename)) },
        Err(status) => status,
    }
}

/// `/Desc`; null on `Ok` when the producer wrote none. The caller frees a
/// non-null result with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `attachments` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_attachment_description(
    attachments: *const TpdfAttachments,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe {
        entry(
            attachments,
            |a: &TpdfAttachments| &a.inner,
            index,
            "attachment",
        )
    } {
        Ok(attachment) => unsafe { hand_over_string(out, attachment.description.as_deref()) },
        Err(status) => status,
    }
}

/// `/Params /Size`, when declared; `out_present` is 0 when it is not.
/// Advisory: the stream is the truth.
///
/// # Safety
///
/// `attachments` must be a live handle; either out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_attachment_size(
    attachments: *const TpdfAttachments,
    index: u32,
    out_present: *mut c_int,
    out_size: *mut i64,
) -> TpdfStatus {
    match unsafe {
        entry(
            attachments,
            |a: &TpdfAttachments| &a.inner,
            index,
            "attachment",
        )
    } {
        Ok(attachment) => {
            unsafe {
                put(out_present, c_int::from(attachment.size.is_some()));
                put(out_size, attachment.size.unwrap_or(0));
            }
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// An attachment's bytes, decoded through the stream's filters.
///
/// Null on `Ok` when the file specification names no embedded stream at all.
/// A stream that is named and cannot be read is
/// [`TpdfStatus::StreamUnreadable`], with the engine's reason in
/// [`crate::tpdf_last_error_message`] — "there is nothing here" and "there is
/// something here I could not read" are different answers. The caller frees a
/// non-null buffer with [`crate::tpdf_buffer_free`].
///
/// # Safety
///
/// `attachments` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_attachment_data(
    attachments: *const TpdfAttachments,
    index: u32,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    let Some(handle) = (unsafe { attachments.as_ref() }) else {
        set_error("null attachment handle");
        return TpdfStatus::BadArgument;
    };
    let attachment = match unsafe {
        entry(
            attachments,
            |a: &TpdfAttachments| &a.inner,
            index,
            "attachment",
        )
    } {
        Ok(attachment) => attachment,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let Some(stream) = attachment.stream else {
        unsafe { *out = ptr::null_mut() };
        return TpdfStatus::Ok;
    };
    match handle.document.cos().stream_decoded(stream) {
        Ok(bytes) => {
            unsafe { *out = Box::into_raw(Box::new(TpdfBuffer { inner: bytes })) };
            TpdfStatus::Ok
        }
        Err(error) => {
            set_error(&format!(
                "attachment {:?}: {} {} R: {error}",
                attachment.name, stream.num, stream.gen
            ));
            unsafe { *out = ptr::null_mut() };
            TpdfStatus::StreamUnreadable
        }
    }
}

/// Frees an attachments handle. Null is accepted and does nothing.
///
/// # Safety
///
/// `attachments` must have come from [`tpdf_document_attachments`] and must
/// not be used afterwards. Buffers it handed out are unaffected.
#[no_mangle]
pub unsafe extern "C" fn tpdf_attachments_free(attachments: *mut TpdfAttachments) {
    if !attachments.is_null() {
        drop(unsafe { Box::from_raw(attachments) });
    }
}

// ---- warnings (ruling 10) -----------------------------------------------------

/// Everything the engine tolerated, in the order it happened. Opaque to
/// callers.
pub struct TpdfWarnings {
    inner: Vec<tinker_pdf::Warning>,
}

/// The warnings the document has accumulated so far.
///
/// A snapshot: reading pages can tolerate more, and asking again afterwards
/// may answer with more. The caller frees the handle with
/// [`tpdf_warnings_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_document_warnings(
    doc: *const TpdfDocument,
    out: *mut *mut TpdfWarnings,
) -> TpdfStatus {
    let doc = match unsafe { document(doc) } {
        Ok(doc) => doc,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    unsafe {
        *out = Box::into_raw(Box::new(TpdfWarnings {
            inner: doc.warnings(),
        }));
    }
    TpdfStatus::Ok
}

/// How many warnings the handle holds, or zero for null.
///
/// # Safety
///
/// `warnings` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_warnings_count(warnings: *const TpdfWarnings) -> u32 {
    unsafe { warnings.as_ref() }.map_or(0, |w| count(w.inner.len()))
}

/// Where a warning happened: the byte offset that triggered it, and the
/// indirect object being read when it did, if known (`out_has_object` 0 when
/// not).
///
/// # Safety
///
/// `warnings` must be a live handle; any out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_warning_location(
    warnings: *const TpdfWarnings,
    index: u32,
    out_offset: *mut u64,
    out_has_object: *mut c_int,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    match unsafe { entry(warnings, |w: &TpdfWarnings| &w.inner, index, "warning") } {
        Ok(warning) => {
            unsafe {
                put(out_offset, warning.offset);
                put(out_has_object, c_int::from(warning.object.is_some()));
                put(out_object, warning.object.map_or(0, |r| r.num));
                put(out_generation, warning.object.map_or(0, |r| r.gen));
            }
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// A warning's stable identifier, such as `header-not-at-start` — the slug a
/// caller compares and greps. The caller frees it with
/// [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `warnings` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_warning_kind(
    warnings: *const TpdfWarnings,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { entry(warnings, |w: &TpdfWarnings| &w.inner, index, "warning") } {
        Ok(warning) => unsafe { hand_over_string(out, Some(warning.kind.as_str())) },
        Err(status) => status,
    }
}

/// A warning as a sentence, the facade's own wording with whatever the kind
/// carries. The caller frees it with [`crate::tpdf_string_free`].
///
/// # Safety
///
/// `warnings` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_warning_message(
    warnings: *const TpdfWarnings,
    index: u32,
    out: *mut *mut c_char,
) -> TpdfStatus {
    match unsafe { entry(warnings, |w: &TpdfWarnings| &w.inner, index, "warning") } {
        Ok(warning) => unsafe { hand_over_string(out, Some(&warning.kind.to_string())) },
        Err(status) => status,
    }
}

/// Frees a warnings handle. Null is accepted and does nothing.
///
/// # Safety
///
/// `warnings` must have come from [`tpdf_document_warnings`] and must not be
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_warnings_free(warnings: *mut TpdfWarnings) {
    if !warnings.is_null() {
        drop(unsafe { Box::from_raw(warnings) });
    }
}

#[cfg(test)]
mod tests;
