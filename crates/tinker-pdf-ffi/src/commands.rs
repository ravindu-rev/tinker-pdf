//! The command stream on the C ABI (`docs/design/command-stream.md`, "The C
//! ABI"): a page's drawing handed back as bytes, those bytes read into a
//! handle whose cursor fills one [`TpdfCommand`] per call and whose fields
//! read as plain values, accessors for the two tables a host draws from —
//! glyph outlines and image decodes — an encoder that takes commands back to
//! bytes, and the replay.
//!
//! Every function is a projection of a facade call (ruling 11):
//! `Page::command_stream`, `CommandStream::parse`, `records`, `to_bytes`,
//! `version`, `page_index`, `size`, `render` and `from_records`. The cursor,
//! the fields and the accessors read the records `CommandStream::records`
//! hands back, by the places `CommandKind::fields` names; nothing here reads
//! the format's bytes itself.
//!
//! The handles are engine-owned, as `TpdfBitmap` is: every pointer a field
//! or an accessor hands out borrows the stream handle's storage and is valid
//! until [`tpdf_command_stream_free`]. A command is read through a pointer
//! the caller passes, never returned by value, so a binding whose foreign
//! function interface passes no struct by value — Ruby's Fiddle — calls every
//! function here. No callback crosses the boundary.
//!
//! Bytes the reader refuses, and commands whose stream it would refuse, are
//! [`TpdfStatus::BadArgument`], with the reader's own sentence — which
//! refusal, and where — in [`crate::tpdf_last_error_message`].

use std::collections::HashMap;
use std::ffi::{c_int, c_void};
use std::ptr;

use tinker_pdf::{
    CommandField, CommandKind, CommandRecord, CommandStream, CommandStreamError, RenderOptions,
};

use crate::{count, required_bytes, set_error, TpdfBitmap, TpdfBuffer, TpdfDocument};
use crate::{TpdfPixelFormat, TpdfStatus};

/// Every kind of record a command stream holds, as the facade's
/// `CommandKind` lists them, numbered from zero in the order the kinds were
/// added. A kind's number on the wire is another number, which
/// [`TpdfCommand::number`] carries; a kind added by a later version is
/// appended here.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfCommandKind {
    /// `End`, kind 0 on the wire: the stream's end, which the cursor reads
    /// after its last record and which the encoder writes itself.
    End = 0,
    /// `Function`, kind 1: a function a state or a definition refers to.
    Function = 1,
    /// `Halftone`, kind 2.
    Halftone = 2,
    /// `Stored`, kind 3: a state's stored parameters.
    Stored = 3,
    /// `Colorants`, kind 4: what a colour paints by colorant.
    Colorants = 4,
    /// `Source`, kind 5: what a colour was resolved from.
    Source = 5,
    /// `State`, kind 6: a graphics state a command refers to.
    State = 6,
    /// `BeginText`, kind 16.
    BeginText = 7,
    /// `GlyphRun`, kind 17: consecutive glyphs of one font, size, writing
    /// mode and state.
    GlyphRun = 8,
    /// `EndText`, kind 18.
    EndText = 9,
    /// `FillPath`, kind 19.
    FillPath = 10,
    /// `StrokePath`, kind 20.
    StrokePath = 11,
    /// `ClipPath`, kind 21.
    ClipPath = 12,
    /// `SaveState`, kind 22.
    SaveState = 13,
    /// `RestoreState`, kind 23.
    RestoreState = 14,
    /// `DrawImage`, kind 24: an image XObject or an inline image.
    DrawImage = 15,
    /// `DrawShading`, kind 25: `sh`.
    DrawShading = 16,
    /// `BeginMarkedContent`, kind 26.
    BeginMarkedContent = 17,
    /// `EndMarkedContent`, kind 27.
    EndMarkedContent = 18,
    /// `BeginForm`, kind 28.
    BeginForm = 19,
    /// `EndForm`, kind 29.
    EndForm = 20,
    /// `BeginGroup`, kind 30: a transparency group.
    BeginGroup = 21,
    /// `EndGroup`, kind 31.
    EndGroup = 22,
    /// `BeginSoftMask`, kind 32.
    BeginSoftMask = 23,
    /// `EndSoftMask`, kind 33.
    EndSoftMask = 24,
    /// `ClearSoftMask`, kind 34.
    ClearSoftMask = 25,
    /// `GroupBudgetSpent`, kind 35: the interpretation offered no further
    /// group.
    GroupBudgetSpent = 26,
    /// `RecursionRefused`, kind 36: a stream not run again inside itself.
    RecursionRefused = 27,
    /// `NestedBudgetSpent`, kind 37: the interpretation ran no further
    /// nested content.
    NestedBudgetSpent = 28,
    /// `Page`, kind 64: the page's crop box, rotation, group space, usage
    /// and index, a page stream's first record.
    Page = 29,
    /// `Annotation`, kind 65: an appearance's calls follow, in its scope.
    Annotation = 30,
    /// `MissingFont`, kind 66: a font no name resolved, where the
    /// interpretation met it.
    MissingFont = 31,
    /// `RepairedSpace`, kind 67: a colour space read with a repair.
    RepairedSpace = 32,
    /// `Scope`, kind 68: a form's or an appearance's resource scope.
    Scope = 33,
    /// `Outline`, kind 69: a glyph's outline in a scope
    /// ([`tpdf_command_stream_outline`]).
    Outline = 34,
    /// `FontIdentity`, kind 70.
    FontIdentity = 35,
    /// `Profile`, kind 71: an ICC profile's bytes.
    Profile = 36,
    /// `ColorSpace`, kind 72.
    ColorSpace = 37,
    /// `Image`, kind 73: an image XObject a scope names.
    Image = 38,
    /// `InlineImage`, kind 74: an inline image of the stream's calls.
    InlineImage = 39,
    /// `ImageDecode`, kind 75: an image's decode at one reduction
    /// ([`tpdf_command_stream_image`]).
    ImageDecode = 40,
    /// `ShadingDefinition`, kind 76.
    ShadingDefinition = 41,
    /// `Shading`, kind 77: what a shading name answers in a scope.
    Shading = 42,
    /// `Pattern`, kind 78: what a pattern name answers in a scope.
    Pattern = 43,
    /// `Cell`, kind 79: a tiling cell, its own stream.
    Cell = 44,
}

raw_enum!(TpdfCommandKind {
    End,
    Function,
    Halftone,
    Stored,
    Colorants,
    Source,
    State,
    BeginText,
    GlyphRun,
    EndText,
    FillPath,
    StrokePath,
    ClipPath,
    SaveState,
    RestoreState,
    DrawImage,
    DrawShading,
    BeginMarkedContent,
    EndMarkedContent,
    BeginForm,
    EndForm,
    BeginGroup,
    EndGroup,
    BeginSoftMask,
    EndSoftMask,
    ClearSoftMask,
    GroupBudgetSpent,
    RecursionRefused,
    NestedBudgetSpent,
    Page,
    Annotation,
    MissingFont,
    RepairedSpace,
    Scope,
    Outline,
    FontIdentity,
    Profile,
    ColorSpace,
    Image,
    InlineImage,
    ImageDecode,
    ShadingDefinition,
    Shading,
    Pattern,
    Cell,
});

impl TpdfCommandKind {
    /// The facade's kind of the same name.
    pub(crate) fn kind(self) -> CommandKind {
        match self {
            TpdfCommandKind::End => CommandKind::End,
            TpdfCommandKind::Function => CommandKind::Function,
            TpdfCommandKind::Halftone => CommandKind::Halftone,
            TpdfCommandKind::Stored => CommandKind::Stored,
            TpdfCommandKind::Colorants => CommandKind::Colorants,
            TpdfCommandKind::Source => CommandKind::Source,
            TpdfCommandKind::State => CommandKind::State,
            TpdfCommandKind::BeginText => CommandKind::BeginText,
            TpdfCommandKind::GlyphRun => CommandKind::GlyphRun,
            TpdfCommandKind::EndText => CommandKind::EndText,
            TpdfCommandKind::FillPath => CommandKind::FillPath,
            TpdfCommandKind::StrokePath => CommandKind::StrokePath,
            TpdfCommandKind::ClipPath => CommandKind::ClipPath,
            TpdfCommandKind::SaveState => CommandKind::SaveState,
            TpdfCommandKind::RestoreState => CommandKind::RestoreState,
            TpdfCommandKind::DrawImage => CommandKind::DrawImage,
            TpdfCommandKind::DrawShading => CommandKind::DrawShading,
            TpdfCommandKind::BeginMarkedContent => CommandKind::BeginMarkedContent,
            TpdfCommandKind::EndMarkedContent => CommandKind::EndMarkedContent,
            TpdfCommandKind::BeginForm => CommandKind::BeginForm,
            TpdfCommandKind::EndForm => CommandKind::EndForm,
            TpdfCommandKind::BeginGroup => CommandKind::BeginGroup,
            TpdfCommandKind::EndGroup => CommandKind::EndGroup,
            TpdfCommandKind::BeginSoftMask => CommandKind::BeginSoftMask,
            TpdfCommandKind::EndSoftMask => CommandKind::EndSoftMask,
            TpdfCommandKind::ClearSoftMask => CommandKind::ClearSoftMask,
            TpdfCommandKind::GroupBudgetSpent => CommandKind::GroupBudgetSpent,
            TpdfCommandKind::RecursionRefused => CommandKind::RecursionRefused,
            TpdfCommandKind::NestedBudgetSpent => CommandKind::NestedBudgetSpent,
            TpdfCommandKind::Page => CommandKind::Page,
            TpdfCommandKind::Annotation => CommandKind::Annotation,
            TpdfCommandKind::MissingFont => CommandKind::MissingFont,
            TpdfCommandKind::RepairedSpace => CommandKind::RepairedSpace,
            TpdfCommandKind::Scope => CommandKind::Scope,
            TpdfCommandKind::Outline => CommandKind::Outline,
            TpdfCommandKind::FontIdentity => CommandKind::FontIdentity,
            TpdfCommandKind::Profile => CommandKind::Profile,
            TpdfCommandKind::ColorSpace => CommandKind::ColorSpace,
            TpdfCommandKind::Image => CommandKind::Image,
            TpdfCommandKind::InlineImage => CommandKind::InlineImage,
            TpdfCommandKind::ImageDecode => CommandKind::ImageDecode,
            TpdfCommandKind::ShadingDefinition => CommandKind::ShadingDefinition,
            TpdfCommandKind::Shading => CommandKind::Shading,
            TpdfCommandKind::Pattern => CommandKind::Pattern,
            TpdfCommandKind::Cell => CommandKind::Cell,
        }
    }

    /// The variant naming the facade's `kind`.
    pub(crate) fn of(kind: CommandKind) -> Option<TpdfCommandKind> {
        Self::ALL
            .iter()
            .copied()
            .find(|variant| variant.kind() == kind)
    }
}

/// What a field holds: its tag on the wire, the facade's `CommandField`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfCommandFieldTag {
    /// An absent value.
    Nothing = 0,
    /// An unsigned integer, in [`TpdfCommandField::natural`]. A reference to
    /// a record is one of these.
    Unsigned = 1,
    /// A signed integer, in [`TpdfCommandField::integer`].
    Signed = 2,
    /// A real, in [`TpdfCommandField::real`], its bits as written.
    Real = 3,
    /// Bytes: [`TpdfCommandField::data`] points to `count` of them.
    Bytes = 4,
    /// Reals: `data` points to `count` `double`s.
    Reals = 5,
    /// Unsigned integers: `data` points to `count` `uint64_t`s.
    Naturals = 6,
    /// Byte strings: `data` points to `count` [`TpdfCommandBytes`].
    Strings = 7,
}

raw_enum!(TpdfCommandFieldTag {
    Nothing,
    Unsigned,
    Signed,
    Real,
    Bytes,
    Reals,
    Naturals,
    Strings,
});

/// What the page's resources answered for an image at one reduction
/// ([`TpdfCommandImage::answer`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpdfImageAnswer {
    /// No image: the name named none.
    Absent = 0,
    /// The image, decoded.
    Image = 1,
    /// A decode this build refused; the codec's name is in
    /// [`TpdfCommandImage::codec`].
    Refused = 2,
}

/// One record, as the cursor reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TpdfCommand {
    /// The record's kind, a [`TpdfCommandKind`], or -1 for a kind this build
    /// does not know, which a stream of a newer minor version holds.
    pub kind: c_int,
    /// The kind's number on the wire, known or not.
    pub number: u32,
    /// The record's place among the stream's records, from 0: what
    /// [`tpdf_command_stream_field`] and the accessors name it by. The end
    /// stands at the number of records.
    pub index: u32,
    /// How many fields the record holds.
    pub field_count: u32,
}

/// Bytes: a pointer and a length. Null with a length of 0 for none.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfCommandBytes {
    /// The first byte.
    pub data: *const u8,
    /// How many.
    pub len: usize,
}

impl TpdfCommandBytes {
    /// No bytes.
    const NONE: TpdfCommandBytes = TpdfCommandBytes {
        data: ptr::null(),
        len: 0,
    };

    /// `bytes`, borrowed.
    fn of(bytes: &[u8]) -> TpdfCommandBytes {
        if bytes.is_empty() {
            return TpdfCommandBytes::NONE;
        }
        TpdfCommandBytes {
            data: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
}

/// One field, as plain values: its tag, and the one of the values the tag
/// says holds it. The cursor's reads borrow the stream's storage; the
/// encoder copies what it is handed.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfCommandField {
    /// A [`TpdfCommandFieldTag`].
    pub tag: c_int,
    /// An unsigned field's value.
    pub natural: u64,
    /// A signed field's value.
    pub integer: i64,
    /// A real field's value.
    pub real: f64,
    /// A bytes field's bytes, a reals field's `double`s, a naturals field's
    /// `uint64_t`s, or a strings field's [`TpdfCommandBytes`]; null for any
    /// other tag and for an empty array.
    pub data: *const c_void,
    /// How many of them `data` points to.
    pub count: usize,
}

/// A glyph's outline record ([`TpdfCommandKind::Outline`]), as a host draws
/// it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfCommandOutline {
    /// The scope the glyph was asked in: 0 the page's, `n` the `n`th scope
    /// record's.
    pub scope: u64,
    /// The font the glyph is of, as the stream's glyph runs name it.
    pub font_id: u64,
    /// The glyph's code.
    pub code: u32,
    /// 1 when the font gave the glyph an outline, 0 when it gave none and
    /// both arrays are empty.
    pub found: c_int,
    /// The outline's segments, a byte each: 0 a move, 1 a line, 2 a
    /// quadratic, 3 a cubic, 4 a close.
    pub operators: TpdfCommandBytes,
    /// Their numbers in order, in a space where one unit is one em: two for
    /// a move or a line, four for a quadratic, six for a cubic, none for a
    /// close.
    pub numbers: *const f64,
    /// How many numbers.
    pub number_count: usize,
}

/// An image's decode at one reduction ([`TpdfCommandKind::ImageDecode`]),
/// as a host draws it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TpdfCommandImage {
    /// The image the decode is of: the `n`th image record, inline or not,
    /// from 0.
    pub image: u64,
    /// The reduction it is at: 1 the whole image, or what a draw at a
    /// smaller size asks a JPEG or a codestream for.
    pub reduction: u32,
    /// What the page's resources answered.
    pub answer: TpdfImageAnswer,
    /// The decode's width in samples.
    pub width: u32,
    /// Its height in samples.
    pub height: u32,
    /// 1 when the samples are a window of the decode, 0 when they are all
    /// of it.
    pub has_window: c_int,
    /// The window's left edge, in samples.
    pub window_x: u32,
    /// The window's top edge, in samples, counting down.
    pub window_y: u32,
    /// The window's width.
    pub window_width: u32,
    /// The window's height.
    pub window_height: u32,
    /// 1 for a stencil mask, which paints the fill colour where it is set.
    pub stencil: c_int,
    /// The image's `/Interpolate`.
    pub interpolate: c_int,
    /// A refused decode's codec, as UTF-8.
    pub codec: TpdfCommandBytes,
    /// Red, green and blue, three bytes a sample, row-major from the top;
    /// empty where `two_colour` holds the samples.
    pub rgb: TpdfCommandBytes,
    /// Opacity, a byte a sample; empty for an opaque image.
    pub alpha: TpdfCommandBytes,
    /// A byte a sample choosing between `colours`' two, zero the first;
    /// empty for an image held as RGB.
    pub two_colour: TpdfCommandBytes,
    /// Red, green, blue and alpha of a sample of zero, then of any other.
    pub colours: [u8; 8],
}

/// A command stream read. Opaque to callers.
pub struct TpdfCommandStream {
    inner: CommandStream,
    /// The place of the record the cursor reads next.
    cursor: usize,
    /// Each strings field's strings, as pointers into the records, by the
    /// record's place and the field's.
    strings: HashMap<(usize, usize), Vec<TpdfCommandBytes>>,
}

impl TpdfCommandStream {
    fn new(inner: CommandStream) -> TpdfCommandStream {
        let mut strings = HashMap::new();
        for (index, record) in inner.records().iter().enumerate() {
            for (at, field) in record.fields.iter().enumerate() {
                if let CommandField::Strings(list) = field {
                    let pointers = list.iter().map(|s| TpdfCommandBytes::of(s)).collect();
                    strings.insert((index, at), pointers);
                }
            }
        }
        TpdfCommandStream {
            inner,
            cursor: 0,
            strings,
        }
    }
}

/// Commands pushed to be written as a stream. Opaque to callers.
pub struct TpdfCommandEncoder {
    records: Vec<CommandRecord>,
}

/// A live stream handle, or the refusal.
unsafe fn stream_ref<'a>(
    stream: *const TpdfCommandStream,
) -> Result<&'a TpdfCommandStream, TpdfStatus> {
    match unsafe { stream.as_ref() } {
        Some(stream) => Ok(stream),
        None => {
            set_error("null command stream");
            Err(TpdfStatus::BadArgument)
        }
    }
}

/// Record `index` of `stream`, or the refusal naming it.
fn record_at(stream: &TpdfCommandStream, index: u32) -> Result<&CommandRecord, TpdfStatus> {
    let records = stream.inner.records();
    records.get(index as usize).ok_or_else(|| {
        set_error(&format!(
            "no such record: index {index} of a stream of {}",
            records.len()
        ));
        TpdfStatus::BadArgument
    })
}

/// Record `index` of `stream` where it is of `kind`, or the refusal naming
/// what it is.
fn record_of(
    stream: &TpdfCommandStream,
    index: u32,
    kind: CommandKind,
) -> Result<&CommandRecord, TpdfStatus> {
    let record = record_at(stream, index)?;
    if record.kind != kind.number() {
        let held = record
            .command_kind()
            .map_or("a kind this build does not know", CommandKind::name);
        set_error(&format!("record {index} is {held}, not {}", kind.name()));
        return Err(TpdfStatus::BadArgument);
    }
    Ok(record)
}

/// The command a record is, at `index`.
fn command(record: &CommandRecord, index: usize) -> TpdfCommand {
    TpdfCommand {
        kind: record
            .command_kind()
            .and_then(TpdfCommandKind::of)
            .map_or(-1, |kind| kind as c_int),
        number: record.kind,
        index: count(index),
        field_count: count(record.fields.len()),
    }
}

/// A field as plain values, `strings` its strings' pointers where it holds
/// strings.
fn field_of(value: &CommandField, strings: Option<&Vec<TpdfCommandBytes>>) -> TpdfCommandField {
    let mut out = TpdfCommandField {
        tag: c_int::from(value.tag()),
        natural: 0,
        integer: 0,
        real: 0.0,
        data: ptr::null(),
        count: 0,
    };
    match value {
        CommandField::Nothing => {}
        CommandField::Unsigned(v) => out.natural = *v,
        CommandField::Signed(v) => out.integer = *v,
        CommandField::Real(v) => out.real = *v,
        CommandField::Bytes(bytes) => {
            out.data = bytes.as_ptr().cast();
            out.count = bytes.len();
        }
        CommandField::Reals(reals) => {
            out.data = reals.as_ptr().cast();
            out.count = reals.len();
        }
        CommandField::Naturals(naturals) => {
            out.data = naturals.as_ptr().cast();
            out.count = naturals.len();
        }
        CommandField::Strings(list) => {
            out.data = strings.map_or(ptr::null(), |s| s.as_ptr().cast());
            out.count = list.len();
        }
    }
    if out.count == 0 {
        out.data = ptr::null();
    }
    out
}

/// `count` values of `T` at `data`, or none.
///
/// # Safety
///
/// Where `count` is not zero, `data` must point to `count` readable values
/// of `T`, aligned for it.
unsafe fn values<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, count) }
    }
}

/// The facade's field a caller's plain values spell, or the refusal naming
/// `what` and why, its message set.
///
/// # Safety
///
/// `field.data` must point to what its tag says, `count` of them.
unsafe fn field_from(field: &TpdfCommandField, what: &str) -> Result<CommandField, TpdfStatus> {
    let tag = TpdfCommandFieldTag::checked(field.tag, &format!("the tag of {what}"))?;
    let arrayed = matches!(
        tag,
        TpdfCommandFieldTag::Bytes
            | TpdfCommandFieldTag::Reals
            | TpdfCommandFieldTag::Naturals
            | TpdfCommandFieldTag::Strings
    );
    if arrayed && field.data.is_null() && field.count > 0 {
        set_error(&format!(
            "{what}: its data is null and its count {}",
            field.count
        ));
        return Err(TpdfStatus::BadArgument);
    }
    Ok(match tag {
        TpdfCommandFieldTag::Nothing => CommandField::Nothing,
        TpdfCommandFieldTag::Unsigned => CommandField::Unsigned(field.natural),
        TpdfCommandFieldTag::Signed => CommandField::Signed(field.integer),
        TpdfCommandFieldTag::Real => CommandField::Real(field.real),
        TpdfCommandFieldTag::Bytes => {
            CommandField::bytes(unsafe { values(field.data.cast::<u8>(), field.count) })
        }
        TpdfCommandFieldTag::Reals => {
            CommandField::Reals(unsafe { values(field.data.cast::<f64>(), field.count) }.to_vec())
        }
        TpdfCommandFieldTag::Naturals => CommandField::Naturals(
            unsafe { values(field.data.cast::<u64>(), field.count) }.to_vec(),
        ),
        TpdfCommandFieldTag::Strings => {
            let list = unsafe { values(field.data.cast::<TpdfCommandBytes>(), field.count) };
            let mut strings = Vec::with_capacity(list.len());
            for (at, string) in list.iter().enumerate() {
                if string.data.is_null() && string.len > 0 {
                    set_error(&format!(
                        "{what}: its string {at} is null and {} long",
                        string.len
                    ));
                    return Err(TpdfStatus::BadArgument);
                }
                strings.push(unsafe { values(string.data, string.len) }.to_vec());
            }
            CommandField::Strings(strings)
        }
    })
}

/// Hands `bytes` over as a buffer.
///
/// # Safety
///
/// `out` must be a valid pointer.
unsafe fn hand_over_buffer(out: *mut *mut TpdfBuffer, bytes: Vec<u8>) {
    unsafe { *out = Box::into_raw(Box::new(TpdfBuffer { inner: bytes })) };
}

// ---- writing and reading ------------------------------------------------------

/// The page's drawing as a command stream: `Page::command_stream`, its
/// display list's for no usage.
///
/// The caller frees the result with [`crate::tpdf_buffer_free`].
///
/// # Safety
///
/// `doc` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_page_command_stream(
    doc: *const TpdfDocument,
    index: u32,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    let (Some(doc), false) = (unsafe { doc.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let Some(page) = doc.inner.page(index) else {
        set_error("no such page");
        return TpdfStatus::NoSuchPage;
    };
    unsafe { hand_over_buffer(out, page.command_stream()) };
    TpdfStatus::Ok
}

/// Reads a command stream: `CommandStream::parse`, under the design's
/// budgets.
///
/// The bytes are read, not kept: the caller may free theirs at once. Bytes
/// that are no stream this build reads — no command stream, a newer major
/// version, cut short or running on, a kind this minor does not define, a
/// record whose fields are not its kind's, no page record, past a budget —
/// are [`TpdfStatus::BadArgument`] with the reader's sentence. The caller
/// frees the result with [`tpdf_command_stream_free`].
///
/// # Safety
///
/// `bytes` must point to at least `len` readable bytes and `out` must be a
/// valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_parse(
    bytes: *const u8,
    len: usize,
    out: *mut *mut TpdfCommandStream,
) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let bytes = match unsafe { required_bytes(bytes, len, "the stream's bytes") } {
        Ok(bytes) => bytes,
        Err(status) => return status,
    };
    match CommandStream::parse(bytes) {
        Ok(stream) => {
            let handle = Box::new(TpdfCommandStream::new(stream));
            unsafe { *out = Box::into_raw(handle) };
            TpdfStatus::Ok
        }
        Err(error) => {
            set_error(&format!("the command stream was refused: {error}"));
            TpdfStatus::BadArgument
        }
    }
}

/// How many records the stream holds before its end: the records the
/// cursor reads, and 0 for a null handle.
///
/// # Safety
///
/// `stream` must be a live handle or null.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_count(stream: *const TpdfCommandStream) -> u32 {
    unsafe { stream.as_ref() }.map_or(0, |stream| count(stream.inner.records().len()))
}

/// The cursor: fills `out` with the next record's command and moves past
/// it. After the last record it fills [`TpdfCommandKind::End`], at the
/// number of records, and stays there until
/// [`tpdf_command_stream_rewind`].
///
/// # Safety
///
/// `stream` must be a live handle, used from one thread at a time, and
/// `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_next(
    stream: *mut TpdfCommandStream,
    out: *mut TpdfCommand,
) -> TpdfStatus {
    let (Some(stream), Some(slot)) = (unsafe { stream.as_mut() }, unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let records = stream.inner.records();
    *slot = match records.get(stream.cursor) {
        Some(record) => {
            let read = command(record, stream.cursor);
            stream.cursor += 1;
            read
        }
        None => TpdfCommand {
            kind: TpdfCommandKind::End as c_int,
            number: CommandKind::End.number(),
            index: count(records.len()),
            field_count: 0,
        },
    };
    TpdfStatus::Ok
}

/// Puts the cursor back on the stream's first record.
///
/// # Safety
///
/// `stream` must be a live handle, used from one thread at a time.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_rewind(stream: *mut TpdfCommandStream) -> TpdfStatus {
    let Some(stream) = (unsafe { stream.as_mut() }) else {
        set_error("null command stream");
        return TpdfStatus::BadArgument;
    };
    stream.cursor = 0;
    TpdfStatus::Ok
}

/// Fills `out` with record `index`'s command, wherever the cursor stands.
///
/// # Safety
///
/// `stream` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_record(
    stream: *const TpdfCommandStream,
    index: u32,
    out: *mut TpdfCommand,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    match record_at(stream, index) {
        Ok(record) => {
            *slot = command(record, index as usize);
            TpdfStatus::Ok
        }
        Err(status) => status,
    }
}

/// Fills `out` with field `field` of record `index`, as plain values; a
/// pointer in it borrows the stream until [`tpdf_command_stream_free`].
/// `CommandKind::fields` names each place, as the design does.
///
/// # Safety
///
/// `stream` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_field(
    stream: *const TpdfCommandStream,
    index: u32,
    field: u32,
    out: *mut TpdfCommandField,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let record = match record_at(stream, index) {
        Ok(record) => record,
        Err(status) => return status,
    };
    let Some(value) = record.fields.get(field as usize) else {
        set_error(&format!(
            "no such field: {field} of record {index}, which holds {}",
            record.fields.len()
        ));
        return TpdfStatus::BadArgument;
    };
    let strings = stream.strings.get(&(index as usize, field as usize));
    *slot = field_of(value, strings);
    TpdfStatus::Ok
}

/// Fills `out` with outline record `index`'s glyph: its scope, font and
/// code, and its segments' operators and numbers, borrowed from the stream.
/// A record of another kind is [`TpdfStatus::BadArgument`] naming it.
///
/// # Safety
///
/// `stream` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_outline(
    stream: *const TpdfCommandStream,
    index: u32,
    out: *mut TpdfCommandOutline,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let record = match record_of(stream, index, CommandKind::Outline) {
        Ok(record) => record,
        Err(status) => return status,
    };
    let fields = record.read();
    let read = || -> Result<TpdfCommandOutline, CommandStreamError> {
        let numbers = fields.reals(5)?;
        Ok(TpdfCommandOutline {
            scope: fields.unsigned(0)?,
            font_id: fields.unsigned(1)?,
            code: fields.unsigned32(2)?,
            found: c_int::from(fields.flag(3)?),
            operators: TpdfCommandBytes::of(fields.bytes(4)?),
            numbers: if numbers.is_empty() {
                ptr::null()
            } else {
                numbers.as_ptr()
            },
            number_count: numbers.len(),
        })
    };
    match read() {
        Ok(outline) => {
            *slot = outline;
            TpdfStatus::Ok
        }
        Err(error) => {
            set_error(&error.to_string());
            TpdfStatus::BadArgument
        }
    }
}

/// Fills `out` with image decode record `index`: what the page's resources
/// answered, and for an image its size, its window, its samples and its
/// colours, borrowed from the stream. A record of another kind is
/// [`TpdfStatus::BadArgument`] naming it.
///
/// # Safety
///
/// `stream` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_image(
    stream: *const TpdfCommandStream,
    index: u32,
    out: *mut TpdfCommandImage,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    let record = match record_of(stream, index, CommandKind::ImageDecode) {
        Ok(record) => record,
        Err(status) => return status,
    };
    let fields = record.read();
    let read = || -> Result<TpdfCommandImage, CommandStreamError> {
        let mut image = TpdfCommandImage {
            image: fields.unsigned(0)?,
            reduction: fields.unsigned32(1)?,
            answer: TpdfImageAnswer::Absent,
            width: 0,
            height: 0,
            has_window: 0,
            window_x: 0,
            window_y: 0,
            window_width: 0,
            window_height: 0,
            stencil: 0,
            interpolate: 0,
            codec: TpdfCommandBytes::NONE,
            rgb: TpdfCommandBytes::NONE,
            alpha: TpdfCommandBytes::NONE,
            two_colour: TpdfCommandBytes::NONE,
            colours: [0; 8],
        };
        match fields.unsigned(2)? {
            0 => {}
            1 => {
                image.answer = TpdfImageAnswer::Image;
                image.width = fields.unsigned32(4)?;
                image.height = fields.unsigned32(5)?;
                image.rgb = TpdfCommandBytes::of(fields.bytes(6)?);
                image.alpha = TpdfCommandBytes::of(fields.bytes(7)?);
                if let Some(samples) = fields.optional_bytes(8)? {
                    image.two_colour = TpdfCommandBytes::of(samples);
                    image.colours.copy_from_slice(fields.exact_bytes(9, 8)?);
                }
                image.stencil = c_int::from(fields.flag(10)?);
                image.interpolate = c_int::from(fields.flag(11)?);
                if let Some([x, y, w, h]) = fields.optional_naturals(15)? {
                    let fit = |v: &u64| u32::try_from(*v).unwrap_or(u32::MAX);
                    image.has_window = 1;
                    image.window_x = fit(x);
                    image.window_y = fit(y);
                    image.window_width = fit(w);
                    image.window_height = fit(h);
                }
            }
            2 => {
                image.answer = TpdfImageAnswer::Refused;
                image.codec = TpdfCommandBytes::of(fields.bytes(3)?);
            }
            other => return Err(fields.wrong(format!("an image answer {other}"))),
        }
        Ok(image)
    };
    match read() {
        Ok(image) => {
            *slot = image;
            TpdfStatus::Ok
        }
        Err(error) => {
            set_error(&error.to_string());
            TpdfStatus::BadArgument
        }
    }
}

/// The stream's bytes, written back at its own version: for a stream this
/// build wrote, the bytes it was read from. The caller frees the result
/// with [`crate::tpdf_buffer_free`].
///
/// # Safety
///
/// `stream` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_to_bytes(
    stream: *const TpdfCommandStream,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    unsafe { hand_over_buffer(out, stream.inner.to_bytes()) };
    TpdfStatus::Ok
}

/// The stream's major and minor version.
///
/// # Safety
///
/// `stream` must be a live handle; `major` and `minor` valid pointers.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_version(
    stream: *const TpdfCommandStream,
    major: *mut u16,
    minor: *mut u16,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    let (Some(major), Some(minor)) = (unsafe { major.as_mut() }, unsafe { minor.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    (*major, *minor) = stream.inner.version();
    TpdfStatus::Ok
}

/// Which page of its document the stream is of.
///
/// # Safety
///
/// `stream` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_page_index(
    stream: *const TpdfCommandStream,
    out: *mut u32,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    let Some(slot) = (unsafe { out.as_mut() }) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    *slot = stream.inner.page_index();
    TpdfStatus::Ok
}

/// The page's size in points after rotation, as [`crate::tpdf_page_size`]
/// gives it for the page the stream is of.
///
/// # Safety
///
/// `stream` must be a live handle; `width` and `height` valid pointers.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_size(
    stream: *const TpdfCommandStream,
    width: *mut f64,
    height: *mut f64,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    let (Some(width), Some(height)) = (unsafe { width.as_mut() }, unsafe { height.as_mut() })
    else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    (*width, *height) = stream.inner.size();
    TpdfStatus::Ok
}

/// Draws the stream as [`crate::tpdf_page_render`] draws its page, at
/// `scale` in `format` (a [`TpdfPixelFormat`]; any other number is
/// [`TpdfStatus::BadArgument`]), through the engine's own renderer with no
/// document behind it. The caller frees the result with
/// [`crate::tpdf_bitmap_free`].
///
/// # Safety
///
/// `stream` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_render(
    stream: *const TpdfCommandStream,
    scale: f64,
    format: c_int,
    out: *mut *mut TpdfBitmap,
) -> TpdfStatus {
    let stream = match unsafe { stream_ref(stream) } {
        Ok(stream) => stream,
        Err(status) => return status,
    };
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let format = match TpdfPixelFormat::checked(format, "pixel format") {
        Ok(format) => format,
        Err(status) => return status,
    };
    let bitmap = stream.inner.render(&RenderOptions {
        scale,
        format: format.into(),
        ..RenderOptions::default()
    });
    unsafe { *out = Box::into_raw(Box::new(TpdfBitmap { inner: bitmap })) };
    TpdfStatus::Ok
}

/// Frees a command stream. Null is accepted and does nothing.
///
/// # Safety
///
/// `stream` must have come from [`tpdf_command_stream_parse`], and neither
/// it nor any pointer borrowed from it may be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_stream_free(stream: *mut TpdfCommandStream) {
    if !stream.is_null() {
        drop(unsafe { Box::from_raw(stream) });
    }
}

// ---- the encoder ----------------------------------------------------------------

/// Starts an encoder, holding no commands. The caller frees it with
/// [`tpdf_command_encoder_free`].
///
/// # Safety
///
/// `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_encoder_new(out: *mut *mut TpdfCommandEncoder) -> TpdfStatus {
    if out.is_null() {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    }
    let encoder = Box::new(TpdfCommandEncoder {
        records: Vec::new(),
    });
    unsafe { *out = Box::into_raw(encoder) };
    TpdfStatus::Ok
}

/// Appends a record: `kind` (a [`TpdfCommandKind`] other than `End`, which
/// [`tpdf_command_encoder_finish`] writes) and its `count` fields, copied.
/// A kind or a tag its enum does not declare, `End`, and null data with a
/// count are [`TpdfStatus::BadArgument`] naming them, and nothing is
/// appended. Whether the fields are their kind's is the parse's question,
/// which the finish asks.
///
/// # Safety
///
/// `encoder` must be a live handle, used from one thread at a time, and
/// `fields` must point to `count` fields, each `data` to what its tag says,
/// aligned for it.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_encoder_push(
    encoder: *mut TpdfCommandEncoder,
    kind: c_int,
    fields: *const TpdfCommandField,
    count: usize,
) -> TpdfStatus {
    let Some(encoder) = (unsafe { encoder.as_mut() }) else {
        set_error("null command encoder");
        return TpdfStatus::BadArgument;
    };
    let kind = match TpdfCommandKind::checked(kind, "command kind") {
        Ok(kind) => kind,
        Err(status) => return status,
    };
    if kind == TpdfCommandKind::End {
        set_error("the end is no command to push: tpdf_command_encoder_finish writes it");
        return TpdfStatus::BadArgument;
    }
    if fields.is_null() && count > 0 {
        set_error(&format!("the fields are null and their count {count}"));
        return TpdfStatus::BadArgument;
    }
    let mut record = CommandRecord::new(kind.kind());
    for (at, field) in unsafe { values(fields, count) }.iter().enumerate() {
        let what = format!("field {at} of the {} record pushed", kind.kind().name());
        match unsafe { field_from(field, &what) } {
            Ok(value) => record.fields.push(value),
            Err(status) => return status,
        }
    }
    encoder.records.push(record);
    TpdfStatus::Ok
}

/// The commands pushed so far as a stream's bytes at this build's version,
/// read back as [`tpdf_command_stream_parse`] reads them:
/// `CommandStream::from_records`. Commands whose stream the reader refuses
/// are [`TpdfStatus::BadArgument`] with its sentence. The encoder keeps its
/// commands, so more may be pushed and the stream finished again. The
/// caller frees the result with [`crate::tpdf_buffer_free`].
///
/// # Safety
///
/// `encoder` must be a live handle and `out` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_encoder_finish(
    encoder: *const TpdfCommandEncoder,
    out: *mut *mut TpdfBuffer,
) -> TpdfStatus {
    let (Some(encoder), false) = (unsafe { encoder.as_ref() }, out.is_null()) else {
        set_error("null pointer");
        return TpdfStatus::BadArgument;
    };
    match CommandStream::from_records(&encoder.records) {
        Ok(stream) => {
            unsafe { hand_over_buffer(out, stream.to_bytes()) };
            TpdfStatus::Ok
        }
        Err(error) => {
            set_error(&format!("the commands pushed are no stream: {error}"));
            TpdfStatus::BadArgument
        }
    }
}

/// Frees an encoder. Null is accepted and does nothing.
///
/// # Safety
///
/// `encoder` must have come from [`tpdf_command_encoder_new`] and must not
/// be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn tpdf_command_encoder_free(encoder: *mut TpdfCommandEncoder) {
    if !encoder.is_null() {
        drop(unsafe { Box::from_raw(encoder) });
    }
}

#[cfg(test)]
mod tests;
