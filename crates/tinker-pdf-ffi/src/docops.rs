//! The editor's document operations: page labels, embedded files, the
//! outline, the typed `/Info` setters and the XMP packet, the production page
//! boundaries, and `sanitise` with its report.
//!
//! Each function is one `DocumentEditor` call (ruling 11). Where the facade
//! answers `Result` the refusal's own sentence crosses through
//! [`crate::tpdf_last_error_message`] with [`TpdfStatus::EditRefused`]; where
//! it answers `bool` or `Option` the call and its argument are named, as for
//! every other edit. Where the facade answers with a [`MetadataSync`] -- what
//! a metadata write did to the *other* statement of the same metadata -- it
//! crosses as a [`TpdfMetadataSync`], because an `/Info` entry and an XMP
//! packet that disagree are a document that says two things, and a caller is
//! owed the warning.
//!
//! One read rides along: [`tpdf_page_boundary`], `Page::boundary`, so a
//! boundary written here can be read back through the same ABI.

use std::ffi::{c_char, c_int};
use std::ptr;

use tinker_pdf::{
    Date, DocumentEditor, EmbeddedFile, EntryHolder, LabelStyle, MetadataSync, PageBoundary,
    PageLabelRange, PathStep, Removal, Sanitise, SanitiseReport, Trapped,
};

use crate::read::{TpdfInfoKey, TpdfTrapped};
use crate::{
    count, refused, required_str, set_error, TpdfDocument, TpdfEditor, TpdfOutlineEntry, TpdfStatus,
};

/// How a page-label range writes its number (12.4.2, Table 159).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfLabelStyle {
    /// `/D`: 1, 2, 3.
    Decimal = 0,
    /// `/R`: I, II, III.
    RomanUpper = 1,
    /// `/r`: i, ii, iii.
    RomanLower = 2,
    /// `/A`: A, B, ... Z, AA.
    LettersUpper = 3,
    /// `/a`: a, b, ... z, aa.
    LettersLower = 4,
    /// No number: every page of the range is labelled with the prefix alone.
    None = 5,
}

raw_enum!(TpdfLabelStyle {
    Decimal,
    RomanUpper,
    RomanLower,
    LettersUpper,
    LettersLower,
    None
});

impl TpdfLabelStyle {
    fn to_facade(self) -> LabelStyle {
        match self {
            TpdfLabelStyle::Decimal => LabelStyle::Decimal,
            TpdfLabelStyle::RomanUpper => LabelStyle::RomanUpper,
            TpdfLabelStyle::RomanLower => LabelStyle::RomanLower,
            TpdfLabelStyle::LettersUpper => LabelStyle::LettersUpper,
            TpdfLabelStyle::LettersLower => LabelStyle::LettersLower,
            TpdfLabelStyle::None => LabelStyle::None,
        }
    }
}

/// One run of page labels, as C sees `PageLabelRange`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfPageLabelRange {
    /// The zero-based index of the range's first page.
    pub first_page: u32,
    /// How the number is written: a [`TpdfLabelStyle`]. Any other number is
    /// [`TpdfStatus::BadArgument`].
    pub style: c_int,
    /// `/P`, null-terminated UTF-8, or null for no `/P` -- which reads the
    /// same as an empty one and is not the same file.
    pub prefix: *const c_char,
    /// `/St`: the number of the range's first page; at least 1.
    pub start: u32,
}

/// A date (7.9.4), as C sees `Date`.
///
/// Every field an `int32_t` so a hand-written binding has no packing to
/// guess at; a field outside its range -- a month of 13, a minute of 60 --
/// is [`TpdfStatus::BadArgument`] rather than a byte truncated into another
/// date.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfDate {
    /// Four-digit year.
    pub year: i32,
    /// 1 to 12.
    pub month: i32,
    /// 1 to 31.
    pub day: i32,
    /// 0 to 23.
    pub hour: i32,
    /// 0 to 59.
    pub minute: i32,
    /// 0 to 59.
    pub second: i32,
    /// 1 when the date states its offset from UT; 0 for an unspecified zone.
    pub has_utc_offset: i32,
    /// The offset from UT in minutes, when `has_utc_offset` is 1.
    pub utc_offset_minutes: i32,
}

impl TpdfDate {
    /// The facade's own date, or a refusal naming the field out of range.
    fn to_facade(self) -> Result<Date, TpdfStatus> {
        let byte = |value: i32, what: &str| -> Result<u8, TpdfStatus> {
            u8::try_from(value).map_err(|_| {
                set_error(&format!("date {what} {value} is out of range"));
                TpdfStatus::BadArgument
            })
        };
        Ok(Date {
            year: self.year,
            month: byte(self.month, "month")?,
            day: byte(self.day, "day")?,
            hour: byte(self.hour, "hour")?,
            minute: byte(self.minute, "minute")?,
            second: byte(self.second, "second")?,
            utc_offset_minutes: (self.has_utc_offset != 0).then_some(self.utc_offset_minutes),
        })
    }
}

/// A file to embed, as C sees `EmbeddedFile` (7.11.4).
///
/// Pointers and a length only, so there is no padding to guess at. The
/// strings are null-terminated UTF-8; `description`, `mime_type`, `created`
/// and `modified` may be null for "none".
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfEmbeddedFile {
    /// The key it is filed under in `/Names /EmbeddedFiles`.
    pub name: *const c_char,
    /// The file name offered when it is saved out (`/UF`, and `/F`).
    pub filename: *const c_char,
    /// `/Desc`, or null.
    pub description: *const c_char,
    /// The stream's `/Subtype`, a MIME type such as `text/csv`, or null.
    pub mime_type: *const c_char,
    /// `/Params /CreationDate`, or null.
    pub created: *const TpdfDate,
    /// `/Params /ModDate`, or null.
    pub modified: *const TpdfDate,
    /// The file's bytes, unencoded; borrowed for the call and copied.
    pub data: *const u8,
    /// How many bytes `data` points at.
    pub data_len: usize,
}

/// What a metadata write did to the other statement of the same metadata.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfMetadataSync {
    /// The other half does not exist, so nothing can disagree.
    Alone = 0,
    /// The other half exists and was **not** changed; it may still state the
    /// old value.
    OtherHalfUnchanged = 1,
}

impl TpdfMetadataSync {
    fn of(sync: MetadataSync) -> TpdfMetadataSync {
        match sync {
            MetadataSync::Alone => TpdfMetadataSync::Alone,
            MetadataSync::OtherHalfUnchanged => TpdfMetadataSync::OtherHalfUnchanged,
        }
    }
}

/// One of a page's five boundaries (14.11.2).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfPageBoundary {
    /// `/MediaBox`.
    MediaBox = 0,
    /// `/CropBox`.
    CropBox = 1,
    /// `/BleedBox`.
    BleedBox = 2,
    /// `/TrimBox`.
    TrimBox = 3,
    /// `/ArtBox`.
    ArtBox = 4,
}

raw_enum!(TpdfPageBoundary {
    MediaBox,
    CropBox,
    BleedBox,
    TrimBox,
    ArtBox
});

impl TpdfPageBoundary {
    fn to_facade(self) -> PageBoundary {
        match self {
            TpdfPageBoundary::MediaBox => PageBoundary::MediaBox,
            TpdfPageBoundary::CropBox => PageBoundary::CropBox,
            TpdfPageBoundary::BleedBox => PageBoundary::BleedBox,
            TpdfPageBoundary::TrimBox => PageBoundary::TrimBox,
            TpdfPageBoundary::ArtBox => PageBoundary::ArtBox,
        }
    }
}

/// What [`tpdf_editor_sanitise`] takes out, as C sees `Sanitise`. Each field
/// is 0 or 1; all four 1 is `Sanitise::ALL`, all four 0 removes nothing.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfSanitise {
    /// Every JavaScript action, the document-level scripts, `/AcroForm /CO`
    /// and `/XFA`.
    pub javascript: c_int,
    /// Every action that reaches outside the document or plays media.
    pub actions: c_int,
    /// Every embedded file.
    pub embedded_files: c_int,
    /// `/Info` and every `/Metadata` stream.
    pub metadata: c_int,
}

/// Why [`tpdf_editor_sanitise`] removed something, as C sees `Removal`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfRemoval {
    /// A JavaScript action.
    JavaScript = 0,
    /// `/Names /JavaScript`.
    DocumentJavaScript = 1,
    /// `/AcroForm /CO`.
    CalculationOrder = 2,
    /// `/AcroForm /XFA`.
    XfaForm = 3,
    /// An outward-reaching action; its `/S` crosses through
    /// [`tpdf_sanitise_report_action`].
    Action = 4,
    /// `/Names /EmbeddedFiles`.
    EmbeddedFileTree = 5,
    /// A file specification's `/EF` or `/RF`, or an embedded file stream.
    EmbeddedFile = 6,
    /// `/Info`.
    Info = 7,
    /// A `/Metadata` stream.
    Metadata = 8,
}

impl TpdfRemoval {
    fn of(removal: &Removal) -> TpdfRemoval {
        match removal {
            Removal::JavaScript => TpdfRemoval::JavaScript,
            Removal::DocumentJavaScript => TpdfRemoval::DocumentJavaScript,
            Removal::CalculationOrder => TpdfRemoval::CalculationOrder,
            Removal::XfaForm => TpdfRemoval::XfaForm,
            Removal::Action(_) => TpdfRemoval::Action,
            Removal::EmbeddedFileTree => TpdfRemoval::EmbeddedFileTree,
            Removal::EmbeddedFile => TpdfRemoval::EmbeddedFile,
            Removal::Info => TpdfRemoval::Info,
            Removal::Metadata => TpdfRemoval::Metadata,
        }
    }
}

/// Which of a sanitise report's two lists an accessor reads.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfSanitiseList {
    /// Entries removed from objects that stay, and from the trailer.
    Removed = 0,
    /// Objects deleted because only removed entries reached them.
    Deleted = 1,
}

raw_enum!(TpdfSanitiseList { Removed, Deleted });

/// Everything a sanitise took out. Opaque to callers.
pub struct TpdfSanitiseReport {
    inner: SanitiseReport,
}

/// A live editor, or the refusal.
unsafe fn editor_mut<'a>(editor: *mut TpdfEditor) -> Result<&'a mut DocumentEditor, TpdfStatus> {
    match unsafe { editor.as_mut() } {
        Some(editor) => Ok(&mut editor.inner),
        None => {
            set_error("null editor");
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// Writes a sync answer through `out` when `out` is not null.
unsafe fn hand_over_sync(out: *mut TpdfMetadataSync, sync: MetadataSync) -> TpdfStatus {
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = TpdfMetadataSync::of(sync);
    }
    TpdfStatus::Ok
}

/// An optional C string, `None` for null.
unsafe fn nullable_str(value: *const c_char, what: &str) -> Result<Option<String>, TpdfStatus> {
    if value.is_null() {
        Ok(None)
    } else {
        unsafe { required_str(value, what) }.map(Some)
    }
}

/// Sets the document's page labels (12.4.2), replacing any it had.
///
/// Refused as a whole -- [`TpdfStatus::EditRefused`] with the facade's own
/// reason -- when no range starts at page 0, a range starts past the last
/// page or numbers from 0, two ranges start at one page, or there is no
/// catalog.
///
/// # Safety
///
/// `editor` must be a live handle and `ranges` must point to `count` ranges,
/// each `prefix` null or a null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_page_labels(
    editor: *mut TpdfEditor,
    ranges: *const TpdfPageLabelRange,
    count: usize,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    if ranges.is_null() && count != 0 {
        set_error("null page-label range array");
        return TpdfStatus::BadArgument;
    }
    let mut facade = Vec::with_capacity(count);
    for index in 0..count {
        let range = unsafe { *ranges.add(index) };
        let prefix = match unsafe { nullable_str(range.prefix, "prefix") } {
            Ok(prefix) => prefix,
            Err(status) => return status,
        };
        let style = match TpdfLabelStyle::checked(range.style, "page-label style") {
            Ok(style) => style.to_facade(),
            Err(status) => return status,
        };
        facade.push(PageLabelRange {
            first_page: range.first_page,
            style,
            prefix,
            start: range.start,
        });
    }
    match editor.set_page_labels(&facade) {
        Ok(()) => TpdfStatus::Ok,
        Err(error) => refused("set_page_labels", &error.to_string()),
    }
}

/// Embeds a file (7.11.4), filed under its name in `/Names /EmbeddedFiles`.
///
/// The new file specification's object number and generation are written
/// through the out pointers, either of which may be null. Refused --
/// [`TpdfStatus::EditRefused`] with the facade's own reason -- when the name
/// is taken, the MIME type cannot be written as a name, a date cannot be
/// spelled, or there is no catalog.
///
/// # Safety
///
/// `editor` must be a live handle and `file` a valid pointer whose strings,
/// dates and bytes are valid for the call.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_attach_file(
    editor: *mut TpdfEditor,
    file: *const TpdfEmbeddedFile,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let Some(file) = (unsafe { file.as_ref() }) else {
        set_error("null embedded file");
        return TpdfStatus::BadArgument;
    };
    let date = |value: *const TpdfDate| -> Result<Option<Date>, TpdfStatus> {
        match unsafe { value.as_ref() } {
            Some(date) => date.to_facade().map(Some),
            None => Ok(None),
        }
    };
    let built = (|| -> Result<EmbeddedFile, TpdfStatus> {
        let data = if file.data_len == 0 {
            Vec::new()
        } else {
            unsafe { crate::required_bytes(file.data, file.data_len, "data") }?.to_vec()
        };
        Ok(EmbeddedFile {
            name: unsafe { required_str(file.name, "name") }?,
            filename: unsafe { required_str(file.filename, "filename") }?,
            description: unsafe { nullable_str(file.description, "description") }?,
            mime_type: unsafe { nullable_str(file.mime_type, "mime_type") }?,
            created: date(file.created)?,
            modified: date(file.modified)?,
            data,
        })
    })();
    let built = match built {
        Ok(built) => built,
        Err(status) => return status,
    };
    match editor.attach_file(&built) {
        Ok(reference) => {
            if let Some(slot) = unsafe { out_object.as_mut() } {
                *slot = reference.num;
            }
            if let Some(slot) = unsafe { out_generation.as_mut() } {
                *slot = reference.gen;
            }
            TpdfStatus::Ok
        }
        Err(error) => refused("attach_file", &error.to_string()),
    }
}

/// Replaces the document's outline (12.3.3) with an array of top-level
/// entries built with `tpdf_outline_entry_new`.
///
/// **Consumes every entry in `entries`**, in order, exactly as
/// `tpdf_builder_set_outline` does: each handle stays live and stays the
/// caller's to free. Refused when the tree is one this repository could not
/// read back.
///
/// # Safety
///
/// `editor` must be a live handle and `entries` must point to `count` live
/// entry handles.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_outline(
    editor: *mut TpdfEditor,
    entries: *const *mut TpdfOutlineEntry,
    count: usize,
) -> TpdfStatus {
    if let Err(status) = unsafe { editor_mut(editor) } {
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
        match handle.inner.take("tpdf_editor_set_outline") {
            Ok(entry) => taken.push(entry),
            Err(status) => return status,
        }
    }
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    if editor.set_outline(&taken) {
        TpdfStatus::Ok
    } else {
        refused(
            "set_outline",
            "the tree is deeper or wider than this engine's own reader walks, \
             or the document has no catalog",
        )
    }
}

/// Sets one `/Info` text entry (14.3.3) -- the typed setters `set_title`,
/// `set_author`, `set_subject`, `set_keywords`, `set_creator` and
/// `set_producer` -- creating `/Info` when there is none.
///
/// `key` is a [`TpdfInfoKey`]; the two date keys are
/// [`tpdf_editor_set_info_date`]'s, and passing one here, or a number that is
/// not a key, is [`TpdfStatus::BadArgument`]. What the write did to the XMP
/// packet is written through `out_sync`, which may be null.
///
/// # Safety
///
/// `editor` must be a live handle and `value` a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_info(
    editor: *mut TpdfEditor,
    key: c_int,
    value: *const c_char,
    out_sync: *mut TpdfMetadataSync,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let value = match unsafe { required_str(value, "value") } {
        Ok(value) => value,
        Err(status) => return status,
    };
    let key = match TpdfInfoKey::checked(key, "info key") {
        Ok(key) => key,
        Err(status) => return status,
    };
    let sync = match key {
        TpdfInfoKey::Title => editor.set_title(&value),
        TpdfInfoKey::Author => editor.set_author(&value),
        TpdfInfoKey::Subject => editor.set_subject(&value),
        TpdfInfoKey::Keywords => editor.set_keywords(&value),
        TpdfInfoKey::Creator => editor.set_creator(&value),
        TpdfInfoKey::Producer => editor.set_producer(&value),
        TpdfInfoKey::CreationDate | TpdfInfoKey::ModificationDate => {
            set_error("a date entry is set with tpdf_editor_set_info_date");
            return TpdfStatus::BadArgument;
        }
    };
    unsafe { hand_over_sync(out_sync, sync) }
}

/// Sets `/CreationDate` or `/ModDate` (14.3.3), spelled as 7.9.4 spells a
/// date. Any other key is [`TpdfStatus::BadArgument`]; a date the document's
/// version cannot spell is [`TpdfStatus::EditRefused`], writing nothing.
///
/// # Safety
///
/// `editor` must be a live handle and `date` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_info_date(
    editor: *mut TpdfEditor,
    key: c_int,
    date: *const TpdfDate,
    out_sync: *mut TpdfMetadataSync,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let Some(date) = (unsafe { date.as_ref() }) else {
        set_error("null date");
        return TpdfStatus::BadArgument;
    };
    let date = match date.to_facade() {
        Ok(date) => date,
        Err(status) => return status,
    };
    let key = match TpdfInfoKey::checked(key, "info key") {
        Ok(key) => key,
        Err(status) => return status,
    };
    let sync = match key {
        TpdfInfoKey::CreationDate => editor.set_creation_date(date),
        TpdfInfoKey::ModificationDate => editor.set_modification_date(date),
        _ => {
            set_error("only CreationDate and ModificationDate are dates");
            return TpdfStatus::BadArgument;
        }
    };
    match sync {
        Some(sync) => unsafe { hand_over_sync(out_sync, sync) },
        None => refused("set_info_date", &format!("{date:?}")),
    }
}

/// Sets `/Info /Trapped` (Table 349). `trapped` is a [`TpdfTrapped`];
/// [`TpdfTrapped::Absent`] is [`TpdfStatus::BadArgument`] -- the facade sets a
/// value, it does not remove one -- and so is a number that is not one.
///
/// # Safety
///
/// `editor` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_trapped(
    editor: *mut TpdfEditor,
    trapped: c_int,
    out_sync: *mut TpdfMetadataSync,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let trapped = match TpdfTrapped::checked(trapped, "trapped") {
        Ok(trapped) => trapped,
        Err(status) => return status,
    };
    let value = match trapped {
        TpdfTrapped::True => Trapped::True,
        TpdfTrapped::False => Trapped::False,
        TpdfTrapped::Unknown => Trapped::Unknown,
        TpdfTrapped::Absent => {
            set_error("Absent is not a /Trapped value to write");
            return TpdfStatus::BadArgument;
        }
    };
    unsafe { hand_over_sync(out_sync, editor.set_trapped(value)) }
}

/// Makes `packet` the document's XMP metadata (14.3.2), written verbatim and
/// never compressed. [`TpdfStatus::EditRefused`] when there is no catalog.
///
/// # Safety
///
/// `editor` must be a live handle and `data` must point to `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_xmp_metadata(
    editor: *mut TpdfEditor,
    data: *const u8,
    len: usize,
    out_sync: *mut TpdfMetadataSync,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let packet = match unsafe { crate::required_bytes(data, len, "packet") } {
        Ok(packet) => packet,
        Err(status) => return status,
    };
    match editor.set_xmp_metadata(packet) {
        Some(sync) => unsafe { hand_over_sync(out_sync, sync) },
        None => refused("set_xmp_metadata", "the document has no catalog"),
    }
}

/// Sets one of a page's boundaries (14.11.2) -- `set_page_boundary`, and
/// with it `set_bleed_box`, `set_trim_box` and `set_art_box`, which are that
/// call with the boundary named. `boundary` is a [`TpdfPageBoundary`]; any
/// other number is [`TpdfStatus::BadArgument`]. Refused for a rectangle with
/// no area, a non-finite number, or a page that does not exist.
///
/// # Safety
///
/// `editor` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_set_page_boundary(
    editor: *mut TpdfEditor,
    index: u32,
    boundary: c_int,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let boundary = match TpdfPageBoundary::checked(boundary, "page boundary") {
        Ok(boundary) => boundary,
        Err(status) => return status,
    };
    if editor.set_page_boundary(index, boundary.to_facade(), x0, y0, x1, y1) {
        TpdfStatus::Ok
    } else {
        refused(
            "set_page_boundary",
            &format!("page {index}, [{x0} {y0} {x1} {y1}]"),
        )
    }
}

/// A page's boundary as the reader resolves it -- its own entry, or the
/// default 14.11.2 gives an absent one -- as `x0 y0 x1 y1`. `boundary` is a
/// [`TpdfPageBoundary`]; any other number is [`TpdfStatus::BadArgument`].
///
/// # Safety
///
/// `doc` must be a live handle; any out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_boundary(
    doc: *const TpdfDocument,
    index: u32,
    boundary: c_int,
    out_x0: *mut f64,
    out_y0: *mut f64,
    out_x1: *mut f64,
    out_y1: *mut f64,
) -> TpdfStatus {
    let Some(doc) = (unsafe { doc.as_ref() }) else {
        set_error("null document");
        return TpdfStatus::BadArgument;
    };
    let boundary = match TpdfPageBoundary::checked(boundary, "page boundary") {
        Ok(boundary) => boundary,
        Err(status) => return status,
    };
    let Some(page) = doc.inner.page(index) else {
        set_error("no such page");
        return TpdfStatus::NoSuchPage;
    };
    let (x0, y0, x1, y1) = page.boundary(boundary.to_facade());
    for (out, value) in [(out_x0, x0), (out_y0, y0), (out_x1, x1), (out_y1, y1)] {
        if let Some(slot) = unsafe { out.as_mut() } {
            *slot = value;
        }
    }
    TpdfStatus::Ok
}

/// Takes out what `what` names -- scripts, outward actions, embedded files,
/// metadata -- and reports every change it made (`DocumentEditor::sanitise`).
///
/// The caller frees the report with [`tpdf_sanitise_report_free`]. An empty
/// report is a document that had nothing to take out, not a failure.
///
/// # Safety
///
/// `editor` must be a live handle, `what` a valid pointer and `out` a valid
/// pointer to write a handle to.
#[no_mangle]
pub unsafe extern "C" fn tpdf_editor_sanitise(
    editor: *mut TpdfEditor,
    what: *const TpdfSanitise,
    out: *mut *mut TpdfSanitiseReport,
) -> TpdfStatus {
    let editor = match unsafe { editor_mut(editor) } {
        Ok(editor) => editor,
        Err(status) => return status,
    };
    let (Some(what), false) = (unsafe { what.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let report = editor.sanitise(&Sanitise {
        javascript: what.javascript != 0,
        actions: what.actions != 0,
        embedded_files: what.embedded_files != 0,
        metadata: what.metadata != 0,
    });
    unsafe { *out = Box::into_raw(Box::new(TpdfSanitiseReport { inner: report })) };
    TpdfStatus::Ok
}

/// How many entries one of the report's lists holds, or zero for null --
/// and zero for a `list` that is not a [`TpdfSanitiseList`], which names no
/// list and so holds nothing; [`tpdf_sanitise_report_entry`] refuses the same
/// number with [`TpdfStatus::BadArgument`].
///
/// # Safety
///
/// `report` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_sanitise_report_count(
    report: *const TpdfSanitiseReport,
    list: c_int,
) -> u32 {
    match (unsafe { report.as_ref() }, TpdfSanitiseList::from_raw(list)) {
        (Some(report), Some(TpdfSanitiseList::Removed)) => count(report.inner.removed.len()),
        (Some(report), Some(TpdfSanitiseList::Deleted)) => count(report.inner.deleted.len()),
        _ => 0,
    }
}

/// One entry: why it was removed, and where. For [`TpdfSanitiseList::Removed`]
/// the object is the holder the entry was removed from, and
/// `out_has_object` is 0 when that holder is the trailer; for
/// [`TpdfSanitiseList::Deleted`] it is the deleted object. A `list` that is
/// not a [`TpdfSanitiseList`] is [`TpdfStatus::BadArgument`].
///
/// # Safety
///
/// `report` must be a live handle; any out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_sanitise_report_entry(
    report: *const TpdfSanitiseReport,
    list: c_int,
    index: u32,
    out_what: *mut TpdfRemoval,
    out_has_object: *mut c_int,
    out_object: *mut u32,
    out_generation: *mut u16,
) -> TpdfStatus {
    let Some(report) = (unsafe { report.as_ref() }) else {
        set_error("null sanitise report");
        return TpdfStatus::BadArgument;
    };
    let list = match TpdfSanitiseList::checked(list, "sanitise list") {
        Ok(list) => list,
        Err(status) => return status,
    };
    let (what, object) = match list {
        TpdfSanitiseList::Removed => match report.inner.removed.get(index as usize) {
            Some(entry) => (
                &entry.what,
                match entry.holder {
                    EntryHolder::Trailer => None,
                    EntryHolder::Object(reference) => Some(reference),
                },
            ),
            None => return no_such_entry(index),
        },
        TpdfSanitiseList::Deleted => match report.inner.deleted.get(index as usize) {
            Some(entry) => (&entry.what, Some(entry.object)),
            None => return no_such_entry(index),
        },
    };
    unsafe {
        if let Some(slot) = out_what.as_mut() {
            *slot = TpdfRemoval::of(what);
        }
        if let Some(slot) = out_has_object.as_mut() {
            *slot = c_int::from(object.is_some());
        }
        if let Some(slot) = out_object.as_mut() {
            *slot = object.map_or(0, |r| r.num);
        }
        if let Some(slot) = out_generation.as_mut() {
            *slot = object.map_or(0, |r| r.gen);
        }
    }
    TpdfStatus::Ok
}

fn no_such_entry(index: u32) -> TpdfStatus {
    set_error(&format!("no such sanitise report entry: index {index}"));
    TpdfStatus::BadArgument
}

/// The `/S` of an [`TpdfRemoval::Action`] removal, borrowed until the report
/// is freed; null on `Ok` for any other removal. A `list` that is not a
/// [`TpdfSanitiseList`] is [`TpdfStatus::BadArgument`].
///
/// # Safety
///
/// `report` must be a live handle and both out pointers valid.
#[no_mangle]
pub unsafe extern "C" fn tpdf_sanitise_report_action(
    report: *const TpdfSanitiseReport,
    list: c_int,
    index: u32,
    out_data: *mut *const u8,
    out_len: *mut usize,
) -> TpdfStatus {
    let Some(report) = (unsafe { report.as_ref() }) else {
        set_error("null sanitise report");
        return TpdfStatus::BadArgument;
    };
    if out_data.is_null() || out_len.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let list = match TpdfSanitiseList::checked(list, "sanitise list") {
        Ok(list) => list,
        Err(status) => return status,
    };
    let what = match list {
        TpdfSanitiseList::Removed => report.inner.removed.get(index as usize).map(|e| &e.what),
        TpdfSanitiseList::Deleted => report.inner.deleted.get(index as usize).map(|e| &e.what),
    };
    let Some(what) = what else {
        return no_such_entry(index);
    };
    match what {
        Removal::Action(subtype) => unsafe {
            *out_data = subtype.as_ptr();
            *out_len = subtype.len();
        },
        _ => unsafe {
            *out_data = ptr::null();
            *out_len = 0;
        },
    }
    TpdfStatus::Ok
}

/// How many steps lead from a removed entry's holder to it: the keys and
/// array positions, the last being the one removed.
///
/// # Safety
///
/// `report` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_sanitise_report_path_count(
    report: *const TpdfSanitiseReport,
    index: u32,
) -> u32 {
    unsafe { report.as_ref() }
        .and_then(|report| report.inner.removed.get(index as usize))
        .map_or(0, |entry| count(entry.path.len()))
}

/// One step of a removed entry's path: a dictionary key, whose bytes are
/// borrowed until the report is freed (`out_is_index` 0), or an array
/// position counted in the array as it was (`out_is_index` 1).
///
/// # Safety
///
/// `report` must be a live handle; any out pointer may be null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_sanitise_report_path_step(
    report: *const TpdfSanitiseReport,
    index: u32,
    step: u32,
    out_is_index: *mut c_int,
    out_position: *mut u64,
    out_key_data: *mut *const u8,
    out_key_len: *mut usize,
) -> TpdfStatus {
    let Some(report) = (unsafe { report.as_ref() }) else {
        set_error("null sanitise report");
        return TpdfStatus::BadArgument;
    };
    let Some(entry) = report.inner.removed.get(index as usize) else {
        return no_such_entry(index);
    };
    let Some(found) = entry.path.get(step as usize) else {
        set_error(&format!("no such path step: {step}"));
        return TpdfStatus::BadArgument;
    };
    let (is_index, position, key): (bool, u64, Option<&[u8]>) = match found {
        PathStep::Key(key) => (false, 0, Some(key)),
        PathStep::Index(position) => (true, u64::try_from(*position).unwrap_or(u64::MAX), None),
    };
    unsafe {
        if let Some(slot) = out_is_index.as_mut() {
            *slot = c_int::from(is_index);
        }
        if let Some(slot) = out_position.as_mut() {
            *slot = position;
        }
        if let Some(slot) = out_key_data.as_mut() {
            *slot = key.map_or(ptr::null(), <[u8]>::as_ptr);
        }
        if let Some(slot) = out_key_len.as_mut() {
            *slot = key.map_or(0, <[u8]>::len);
        }
    }
    TpdfStatus::Ok
}

/// Frees a sanitise report. Null is accepted and does nothing.
///
/// # Safety
///
/// `report` must have come from [`tpdf_editor_sanitise`] and must not be used
/// afterwards, nor any bytes borrowed from it.
#[no_mangle]
pub unsafe extern "C" fn tpdf_sanitise_report_free(report: *mut TpdfSanitiseReport) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

#[cfg(test)]
mod tests;
