//! The command stream's codec: a page's recorded events, and the state each
//! carries, as versioned bytes a host can keep, send and read without the
//! document (`docs/design/command-stream.md`, RC-18).
//!
//! # The form
//!
//! A stream is [`MAGIC`], a major and a minor version, then **records**, the
//! last of kind [`CommandKind::End`]. A record is its kind, the length of its
//! fields in bytes, and the fields, each a tag and a value ([`Field`]): so a
//! reader can step over a record it does not know, and every value has exactly
//! one encoding — a list of records read and written back is the same bytes.
//! Each kind's fields are positional, named by [`CommandKind::fields`].
//!
//! A newer minor may add kinds and append fields; a reader skips a kind it
//! does not know in a stream whose minor is newer than [`MINOR`], ignores the
//! fields past those it reads, and refuses an unknown kind in a stream of its
//! own minor or older. A newer major is refused by name.
//!
//! # What this module reads and writes
//!
//! The commands — every [`Event`] variant — and the definitions their states
//! are made of: functions, halftones, the stored parameters, colorants,
//! colour sources and the states themselves, each written once and referred
//! to by its place among the records of its kind. Consecutive glyphs of one
//! font, size, writing mode and state are one run ([`CommandKind::GlyphRun`]).
//! The page and the tables a replay asks for — outlines, images, shadings,
//! patterns, scopes — are the facade's, kinds 64 and up, which this module
//! frames and hands back untouched ([`Outcome::Foreign`]).
//!
//! # Budgets
//!
//! A reader is total over hostile bytes: every count is checked against the
//! bytes left before anything is allocated, and what a stream can make it
//! hold is bounded by [`Budgets`], whose every limit is refused by name
//! ([`CommandError::OverBudget`]) rather than read in part.

use std::collections::HashMap;
use std::sync::Arc;

use tinker_pdf_color::function::{parse_postscript, Function, PsOp};

use crate::device::{Glyph, ImageRef, MarkedProps, PathSegment};
use crate::interpret::{Form, Group, GroupSpace, MaskGroup};
use crate::record::{Event, MarkedScope};
use crate::state::{
    BlackPointCompensation, BlendMode, ColorFamily, ColorSource, ColorantKind, ColorantTint,
    ColorantTints, GraphicsState, Halftone, LineCap, LineJoin, Matrix, RenderingIntent, Rgb, Rgb16,
    SpotFunction, StoredParameters, TextRenderMode, TextState, Transfer,
};

/// The eight bytes a command stream opens with: `\x89TPCMD\r\n`, whose first
/// byte is no text and whose line ending a text-mode copy would change.
pub const MAGIC: [u8; 8] = *b"\x89TPCMD\r\n";

/// The major version this build writes and reads.
pub const MAJOR: u16 = 1;

/// The minor version this build writes, and the newest whose every kind it
/// knows. Minor 1 appends to [`CommandKind::Colorants`] the tints that are no
/// ink and not zero (`ColorantTint::zero`); a stream of minor 0 holds none.
pub const MINOR: u16 = 1;

/// The most glyphs one [`CommandKind::GlyphRun`] holds; a longer run of one
/// font and state is written as several.
pub const MAX_RUN: usize = 1 << 16;

// ---- fields and records ---------------------------------------------------------

/// One field of a record: its tag on the wire, and its value.
///
/// Equality is bitwise for the reals, so a NaN equals the NaN of the same bits
/// and `0.0` is not `-0.0`: two fields are equal exactly when they encode to
/// the same bytes.
#[derive(Clone, Debug)]
pub enum Field {
    /// Tag 0: an absent value.
    Nothing,
    /// Tag 1: an unsigned integer, a varint.
    Unsigned(u64),
    /// Tag 2: a signed integer, a zigzag varint.
    Signed(i64),
    /// Tag 3: an IEEE 754 binary64, its bits as written.
    Real(f64),
    /// Tag 4: bytes.
    Bytes(Arc<[u8]>),
    /// Tag 5: reals.
    Reals(Vec<f64>),
    /// Tag 6: unsigned integers.
    Naturals(Vec<u64>),
    /// Tag 7: byte strings.
    Strings(Vec<Vec<u8>>),
}

impl PartialEq for Field {
    fn eq(&self, other: &Field) -> bool {
        match (self, other) {
            (Field::Nothing, Field::Nothing) => true,
            (Field::Unsigned(a), Field::Unsigned(b)) => a == b,
            (Field::Signed(a), Field::Signed(b)) => a == b,
            (Field::Real(a), Field::Real(b)) => a.to_bits() == b.to_bits(),
            (Field::Bytes(a), Field::Bytes(b)) => a == b,
            (Field::Reals(a), Field::Reals(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
            }
            (Field::Naturals(a), Field::Naturals(b)) => a == b,
            (Field::Strings(a), Field::Strings(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Field {}

impl Field {
    /// The field's tag on the wire.
    #[must_use]
    pub fn tag(&self) -> u8 {
        match self {
            Field::Nothing => 0,
            Field::Unsigned(_) => 1,
            Field::Signed(_) => 2,
            Field::Real(_) => 3,
            Field::Bytes(_) => 4,
            Field::Reals(_) => 5,
            Field::Naturals(_) => 6,
            Field::Strings(_) => 7,
        }
    }

    /// Bytes, from a slice.
    #[must_use]
    pub fn bytes(bytes: &[u8]) -> Field {
        Field::Bytes(Arc::from(bytes))
    }
}

/// One record: its kind's number and its fields, in their places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The kind's number ([`CommandKind::number`]); a reader keeps a kind it
    /// does not know by its number.
    pub kind: u32,
    /// The fields, positional: [`CommandKind::fields`] names each place.
    pub fields: Vec<Field>,
}

impl Record {
    /// A record of `kind` with no fields yet.
    #[must_use]
    pub fn new(kind: CommandKind) -> Record {
        Record {
            kind: kind.number(),
            fields: Vec::new(),
        }
    }

    /// The record with `field` appended.
    #[must_use]
    pub fn with(mut self, field: Field) -> Record {
        self.fields.push(field);
        self
    }

    /// The kind, where this build knows the number.
    #[must_use]
    pub fn command_kind(&self) -> Option<CommandKind> {
        CommandKind::from_number(self.kind)
    }

    /// The record's fields, read as its kind's: what each typed reader of a
    /// record goes through, so a field of the wrong tag is named.
    #[must_use]
    pub fn read(&self) -> Fields<'_> {
        Fields {
            kind: self.kind,
            fields: &self.fields,
        }
    }
}

macro_rules! kinds {
    ($( $number:literal $name:ident $label:literal [$($field:literal),* $(,)?] )*) => {
        /// Every kind of record this build knows, by its number on the wire
        /// (`docs/design/command-stream.md`, "The records"): the codec's,
        /// 0 to 37, and the facade's page and tables, 64 to 79.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum CommandKind {
            $(
                #[doc = concat!("`", $label, "`: kind ", stringify!($number), ".")]
                $name,
            )*
        }

        impl CommandKind {
            /// Every kind, in the order of their numbers.
            pub const ALL: &'static [CommandKind] = &[$(CommandKind::$name),*];

            /// The kind's number on the wire.
            #[must_use]
            pub fn number(self) -> u32 {
                match self {
                    $(CommandKind::$name => $number,)*
                }
            }

            /// The kind a number names, where this build knows it.
            #[must_use]
            pub fn from_number(number: u32) -> Option<CommandKind> {
                match number {
                    $($number => Some(CommandKind::$name),)*
                    _ => None,
                }
            }

            /// The kind's name, as the design names it.
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $(CommandKind::$name => $label,)*
                }
            }

            /// What each of the kind's fields holds, in their order: a
            /// binding names a field by its place here rather than by
            /// knowing the format.
            #[must_use]
            pub fn fields(self) -> &'static [&'static str] {
                match self {
                    $(CommandKind::$name => &[$($field),*],)*
                }
            }
        }
    };
}

kinds! {
    0 End "End" []
    1 Function "Function" ["type", "domain", "range or c0 or functions", "size or c1 or bounds or code", "bits or n or encode or numbers", "encode or operators", "decode", "samples"]
    2 Halftone "Halftone" ["type", "frequency or names or width or xsquare", "angle or halftones or height or ysquare", "spot function or thresholds or second width", "spot names or transfer or second height", "accurate or thresholds", "transfer"]
    3 Stored "Stored" ["rendering intent", "black-point compensation", "stroke overprint", "fill overprint", "overprint mode", "alpha is shape", "text knockout", "flatness", "smoothness", "stroke adjustment", "black generation", "undercolour removal", "transfer", "halftone", "halftone origin"]
    4 Colorants "Colorants" ["kind", "names", "tints", "own inks", "inks", "alternate", "process", "no ink and not zero"]
    5 Source "Source" ["family", "components"]
    6 State "State" ["ctm", "text numbers", "render mode", "font", "fill alpha", "stroke alpha", "fill colour", "stroke colour", "sixteen bits", "fill ink", "stroke ink", "fill family", "stroke family", "fill colorants", "stroke colorants", "line width", "line cap", "line join", "miter limit", "dashes", "dash phase", "fill space", "stroke space", "fill pattern", "blend", "stroke pattern", "stored", "fill source", "stroke source"]
    16 BeginText "BeginText" []
    17 GlyphRun "GlyphRun" ["state", "font id", "font name", "size", "vertical", "codes", "text lengths", "text", "transforms", "advances", "baseline glyphs", "baselines"]
    18 EndText "EndText" []
    19 FillPath "FillPath" ["state", "even-odd", "operators", "numbers"]
    20 StrokePath "StrokePath" ["state", "operators", "numbers"]
    21 ClipPath "ClipPath" ["state", "even-odd", "operators", "numbers"]
    22 SaveState "SaveState" []
    23 RestoreState "RestoreState" []
    24 DrawImage "DrawImage" ["state", "name", "inline", "inline dictionary", "inline data"]
    25 DrawShading "DrawShading" ["state", "name"]
    26 BeginMarkedContent "BeginMarkedContent" ["tag", "visible", "hidden layer", "props", "mcid", "actual text", "alt", "lang", "expansion", "stream", "associated files"]
    27 EndMarkedContent "EndMarkedContent" []
    28 BeginForm "BeginForm" ["id", "name", "entered"]
    29 EndForm "EndForm" ["id"]
    30 BeginGroup "BeginGroup" ["state", "isolated", "knockout", "space", "entered"]
    31 EndGroup "EndGroup" []
    32 BeginSoftMask "BeginSoftMask" ["state", "content", "matrix", "bbox", "group", "isolated", "knockout", "space", "stream", "luminosity", "backdrop", "transfer", "bbox operators", "bbox numbers", "entered"]
    33 EndSoftMask "EndSoftMask" []
    34 ClearSoftMask "ClearSoftMask" []
    35 GroupBudgetSpent "GroupBudgetSpent" []
    36 RecursionRefused "RecursionRefused" []
    37 NestedBudgetSpent "NestedBudgetSpent" []
    64 Page "Page" ["crop box", "rotation", "group space", "usage", "page index"]
    65 Annotation "Annotation" ["scope", "widget"]
    66 MissingFont "MissingFont" ["name"]
    67 RepairedSpace "RepairedSpace" ["name", "repair"]
    68 Scope "Scope" ["id", "parent", "name"]
    69 Outline "Outline" ["scope", "font id", "code", "found", "operators", "numbers", "missing fonts", "damaged names", "damaged", "repaired names", "repaired"]
    70 FontIdentity "FontIdentity" ["scope", "font id", "identity", "object"]
    71 Profile "Profile" ["bytes"]
    72 ColorSpace "ColorSpace" ["type", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"]
    73 Image "Image" ["scope", "name", "intent", "frame", "width", "height", "origin x", "origin y", "levels", "opaque", "interpolate", "room"]
    74 InlineImage "InlineImage" ["scope", "inline image", "intent", "frame", "width", "height", "origin x", "origin y", "levels", "opaque", "interpolate", "room"]
    75 ImageDecode "ImageDecode" ["image", "reduction", "answer", "codec", "width", "height", "rgb", "alpha", "two-colour samples", "two colours", "stencil", "interpolate", "reduced", "family", "soft-masked", "window", "missing fonts", "damaged names", "damaged", "repaired names", "repaired"]
    76 ShadingDefinition "ShadingDefinition" ["type", "space", "function", "numbers", "extend", "mesh kind", "components", "positions", "values", "triangles", "patch points", "patch corners"]
    77 Shading "Shading" ["scope", "name", "answer", "type", "definition", "missing fonts", "damaged names", "damaged", "repaired names", "repaired"]
    78 Pattern "Pattern" ["scope", "name", "answer", "definition", "matrix", "bbox", "x step", "y step", "uncoloured", "missing fonts", "damaged names", "damaged", "repaired names", "repaired"]
    79 Cell "Cell" ["scope", "name", "own resources", "stream"]
}

impl CommandKind {
    /// Whether this module reads and writes the kind: the commands and the
    /// definitions their states are made of, 0 to 37. The rest are the
    /// facade's.
    #[must_use]
    pub fn is_codec(self) -> bool {
        self.number() < 64
    }

    /// Whether the kind is a command — one of a page's recorded calls — rather
    /// than a definition or a table.
    #[must_use]
    pub fn is_command(self) -> bool {
        (16..64).contains(&self.number())
    }
}

// ---- errors and budgets -----------------------------------------------------------

/// Why a stream was not read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandError {
    /// The bytes do not open with [`MAGIC`].
    NotACommandStream,
    /// The stream's major version is newer than the one this build reads.
    NewerMajor {
        /// The stream's major version.
        found: u16,
        /// The major version this build reads, [`MAJOR`].
        read: u16,
    },
    /// The stream ended inside its header, a record or a field, or before its
    /// `End` record: the offset at which more was needed.
    Truncated {
        /// The offset in the stream.
        at: usize,
    },
    /// Bytes follow the `End` record.
    TrailingBytes {
        /// The offset of the first one.
        at: usize,
    },
    /// A record of a kind this build does not know, in a stream whose minor
    /// version is not newer than [`MINOR`].
    UnknownRecord {
        /// The kind's number.
        kind: u32,
    },
    /// A record whose fields are not what its kind holds.
    Malformed {
        /// The kind's number.
        kind: u32,
        /// What is wrong, in words.
        what: String,
    },
    /// The stream asks for more than a budget allows ([`Budgets`]).
    OverBudget {
        /// The budget, by its field's name.
        budget: &'static str,
    },
}

impl core::fmt::Display for CommandError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CommandError::NotACommandStream => f.write_str("not a command stream"),
            CommandError::NewerMajor { found, read } => write!(
                f,
                "a command stream of major version {found}, newer than the {read} this build reads"
            ),
            CommandError::Truncated { at } => {
                write!(f, "the command stream is cut short at byte {at}")
            }
            CommandError::TrailingBytes { at } => {
                write!(f, "bytes follow the command stream's end, from byte {at}")
            }
            CommandError::UnknownRecord { kind } => write!(
                f,
                "a record of kind {kind}, which no version this build reads defines"
            ),
            CommandError::Malformed { kind, what } => {
                let name = CommandKind::from_number(*kind).map_or("unknown", CommandKind::name);
                write!(f, "a malformed {name} record (kind {kind}): {what}")
            }
            CommandError::OverBudget { budget } => {
                write!(
                    f,
                    "the command stream passes the reader's `{budget}` budget"
                )
            }
        }
    }
}

impl std::error::Error for CommandError {}

/// The most a stream may make a reader hold (`docs/design/command-stream.md`,
/// "The reader's budgets"). A stream past any of them is refused by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budgets {
    /// Records in a stream, nested streams' included.
    pub records: u64,
    /// Bytes the read holds: every field's values, every record, and every
    /// command's state and path as a display list counts them
    /// ([`Event::kept_bytes`]).
    pub bytes: u64,
    /// Streams nested in streams: a tiling cell's.
    pub depth: u32,
    /// Functions nested in functions, and a calculator's blocks in blocks.
    pub functions: u32,
    /// Colour spaces nested in colour spaces, and halftones in halftones.
    pub spaces: u32,
}

impl Budgets {
    /// The defaults the design gives.
    pub const DEFAULT: Budgets = Budgets {
        records: 1 << 24,
        bytes: 1 << 30,
        depth: 8,
        functions: 32,
        spaces: 8,
    };
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets::DEFAULT
    }
}

/// What a read has spent of its [`Budgets`], across every stream it reads —
/// a nested one's too.
#[derive(Clone, Debug)]
pub struct Meter {
    budgets: Budgets,
    records: u64,
    bytes: u64,
}

impl Meter {
    /// Nothing spent yet of `budgets`.
    #[must_use]
    pub fn new(budgets: Budgets) -> Meter {
        Meter {
            budgets,
            records: 0,
            bytes: 0,
        }
    }

    /// The budgets this meter holds a read to.
    #[must_use]
    pub fn budgets(&self) -> &Budgets {
        &self.budgets
    }

    /// Counts one record.
    pub fn record(&mut self) -> Result<(), CommandError> {
        self.records = self.records.saturating_add(1);
        if self.records > self.budgets.records {
            return Err(CommandError::OverBudget { budget: "records" });
        }
        Ok(())
    }

    /// Spends `bytes` of the byte budget.
    pub fn spend(&mut self, bytes: u64) -> Result<(), CommandError> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > self.budgets.bytes {
            return Err(CommandError::OverBudget { budget: "bytes" });
        }
        Ok(())
    }

    /// Whether `bytes` more would still be inside the byte budget: asked before
    /// a copy a reference makes is allocated.
    #[must_use]
    pub fn can_hold(&self, bytes: u64) -> bool {
        self.bytes.saturating_add(bytes) <= self.budgets.bytes
    }
}

fn malformed(kind: u32, what: impl Into<String>) -> CommandError {
    CommandError::Malformed {
        kind,
        what: what.into(),
    }
}

// ---- the wire -----------------------------------------------------------------------

/// Appends `value` as a varint.
fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// `value` as a zigzag integer.
fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// A zigzag integer's value.
fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

/// Appends `field`.
fn put_field(out: &mut Vec<u8>, field: &Field) {
    out.push(field.tag());
    match field {
        Field::Nothing => {}
        Field::Unsigned(value) => put_varint(out, *value),
        Field::Signed(value) => put_varint(out, zigzag(*value)),
        Field::Real(value) => out.extend_from_slice(&value.to_bits().to_le_bytes()),
        Field::Bytes(bytes) => {
            put_varint(out, bytes.len() as u64);
            out.extend_from_slice(bytes);
        }
        Field::Reals(reals) => {
            put_varint(out, reals.len() as u64);
            for value in reals {
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        Field::Naturals(naturals) => {
            put_varint(out, naturals.len() as u64);
            for value in naturals {
                put_varint(out, *value);
            }
        }
        Field::Strings(strings) => {
            put_varint(out, strings.len() as u64);
            for string in strings {
                put_varint(out, string.len() as u64);
                out.extend_from_slice(string);
            }
        }
    }
}

/// Appends `record`: its kind, its fields' length, its fields.
pub fn put_record(out: &mut Vec<u8>, record: &Record) {
    let mut fields = Vec::new();
    for field in &record.fields {
        put_field(&mut fields, field);
    }
    put_varint(out, u64::from(record.kind));
    put_varint(out, fields.len() as u64);
    out.extend_from_slice(&fields);
}

/// A stream of `records`, written at this build's version: the header, each
/// record, and `End`. A record of kind `End` among them is written as it is,
/// so a caller passes the records before the end.
#[must_use]
pub fn write_stream(records: &[Record]) -> Vec<u8> {
    write_stream_at(MAJOR, MINOR, records)
}

/// [`write_stream`] under a version of the caller's: what a test of a newer
/// minor or major writes.
#[must_use]
pub fn write_stream_at(major: u16, minor: u16, records: &[Record]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&major.to_le_bytes());
    out.extend_from_slice(&minor.to_le_bytes());
    for record in records {
        put_record(&mut out, record);
    }
    put_record(&mut out, &Record::new(CommandKind::End));
    out
}

/// A stream read into its records: its version, and every record before
/// `End`, unknown kinds kept by number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    /// The stream's major version.
    pub major: u16,
    /// The stream's minor version.
    pub minor: u16,
    /// Every record before `End`, in order.
    pub records: Vec<Record>,
}

/// A cursor over a stream's bytes.
struct Wire<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Wire<'_> {
    fn left(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn byte(&mut self) -> Result<u8, CommandError> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or(CommandError::Truncated { at: self.at })?;
        self.at += 1;
        Ok(byte)
    }

    fn take(&mut self, n: usize) -> Result<&[u8], CommandError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(CommandError::Truncated {
                at: self.bytes.len(),
            })?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    /// A varint: at most ten bytes, the tenth no more than 1, and the
    /// shortest that holds its value — a longer one would be a second
    /// encoding of the same value.
    fn varint(&mut self, kind: u32) -> Result<u64, CommandError> {
        let mut value = 0u64;
        for index in 0..10 {
            let byte = self.byte()?;
            if index == 9 && byte > 1 {
                return Err(malformed(kind, "a varint past 64 bits"));
            }
            value |= u64::from(byte & 0x7f) << (7 * index);
            if byte & 0x80 == 0 {
                if byte == 0 && index > 0 {
                    return Err(malformed(kind, "a varint longer than its value"));
                }
                return Ok(value);
            }
        }
        Err(malformed(kind, "a varint past ten bytes"))
    }

    fn count(&mut self, kind: u32) -> Result<usize, CommandError> {
        let count = self.varint(kind)?;
        usize::try_from(count).map_err(|_| CommandError::Truncated {
            at: self.bytes.len(),
        })
    }

    fn real(&mut self) -> Result<f64, CommandError> {
        let bytes = self.take(8)?;
        let mut eight = [0u8; 8];
        eight.copy_from_slice(bytes);
        Ok(f64::from_bits(u64::from_le_bytes(eight)))
    }

    /// One field, its values charged to `meter` before they are allocated.
    fn field(&mut self, kind: u32, meter: &mut Meter) -> Result<Field, CommandError> {
        let tag = self.byte()?;
        Ok(match tag {
            0 => Field::Nothing,
            1 => Field::Unsigned(self.varint(kind)?),
            2 => Field::Signed(unzigzag(self.varint(kind)?)),
            3 => Field::Real(self.real()?),
            4 => {
                let n = self.count(kind)?;
                if n > self.left() {
                    return Err(CommandError::Truncated {
                        at: self.bytes.len(),
                    });
                }
                meter.spend(n as u64)?;
                Field::Bytes(Arc::from(self.take(n)?))
            }
            5 => {
                let n = self.count(kind)?;
                if n.checked_mul(8).is_none_or(|bytes| bytes > self.left()) {
                    return Err(CommandError::Truncated {
                        at: self.bytes.len(),
                    });
                }
                meter.spend((n as u64).saturating_mul(8))?;
                let mut reals = Vec::with_capacity(n);
                for _ in 0..n {
                    reals.push(self.real()?);
                }
                Field::Reals(reals)
            }
            6 => {
                let n = self.count(kind)?;
                // Each takes a byte at least.
                if n > self.left() {
                    return Err(CommandError::Truncated {
                        at: self.bytes.len(),
                    });
                }
                meter.spend((n as u64).saturating_mul(8))?;
                let mut naturals = Vec::with_capacity(n);
                for _ in 0..n {
                    naturals.push(self.varint(kind)?);
                }
                Field::Naturals(naturals)
            }
            7 => {
                let n = self.count(kind)?;
                if n > self.left() {
                    return Err(CommandError::Truncated {
                        at: self.bytes.len(),
                    });
                }
                let each = core::mem::size_of::<Vec<u8>>() as u64;
                meter.spend((n as u64).saturating_mul(each))?;
                let mut strings = Vec::with_capacity(n);
                for _ in 0..n {
                    let len = self.count(kind)?;
                    if len > self.left() {
                        return Err(CommandError::Truncated {
                            at: self.bytes.len(),
                        });
                    }
                    meter.spend(len as u64)?;
                    strings.push(self.take(len)?.to_vec());
                }
                Field::Strings(strings)
            }
            other => return Err(malformed(kind, format!("a field of tag {other}"))),
        })
    }
}

/// Reads `bytes` into its records, charging `meter`: the header, every
/// record to `End`, and nothing after it. Refuses what [`CommandError`]
/// names; a kind it does not know is kept by its number, for the caller's
/// rule ([`unknown_kind`]).
pub fn read_stream(bytes: &[u8], meter: &mut Meter) -> Result<Stream, CommandError> {
    if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
        return Err(CommandError::NotACommandStream);
    }
    let mut wire = Wire {
        bytes,
        at: MAGIC.len(),
    };
    let version = wire.take(4)?;
    let major = u16::from_le_bytes([version[0], version[1]]);
    let minor = u16::from_le_bytes([version[2], version[3]]);
    if major > MAJOR {
        return Err(CommandError::NewerMajor {
            found: major,
            read: MAJOR,
        });
    }
    if major < MAJOR {
        return Err(CommandError::NotACommandStream);
    }
    let mut records = Vec::new();
    loop {
        if wire.left() == 0 {
            return Err(CommandError::Truncated { at: wire.at });
        }
        let kind = wire.varint(0)?;
        let kind = u32::try_from(kind).map_err(|_| malformed(u32::MAX, "a kind past 32 bits"))?;
        let length = wire.count(kind)?;
        if length > wire.left() {
            return Err(CommandError::Truncated { at: bytes.len() });
        }
        meter.record()?;
        let end = wire.at + length;
        if kind == CommandKind::End.number() {
            if length != 0 {
                return Err(malformed(kind, "an End record with fields"));
            }
            if wire.at < bytes.len() {
                return Err(CommandError::TrailingBytes { at: wire.at });
            }
            break;
        }
        let mut body = Wire {
            bytes: &bytes[..end],
            at: wire.at,
        };
        meter.spend(core::mem::size_of::<Record>() as u64)?;
        let mut fields = Vec::new();
        while body.at < end {
            // Each field is charged before it is held, so a record of
            // empty fields costs the budget what it costs the heap.
            meter.spend(core::mem::size_of::<Field>() as u64)?;
            let field = body.field(kind, meter)?;
            fields.push(field);
        }
        wire.at = end;
        records.push(Record { kind, fields });
    }
    Ok(Stream {
        major,
        minor,
        records,
    })
}

/// What a reader does with a record of kind `kind` this build does not know,
/// in a stream of minor version `minor`: skips it where the stream is newer
/// than [`MINOR`], and refuses it otherwise.
pub fn unknown_kind(kind: u32, minor: u16) -> Result<(), CommandError> {
    if minor > MINOR {
        Ok(())
    } else {
        Err(CommandError::UnknownRecord { kind })
    }
}

// ---- reading a record's fields ----------------------------------------------------

/// A record's fields, read as its kind's: each accessor names the field it
/// could not read.
#[derive(Clone, Copy)]
pub struct Fields<'a> {
    kind: u32,
    fields: &'a [Field],
}

impl<'a> Fields<'a> {
    /// A malformed error for this record.
    pub fn wrong(&self, what: impl Into<String>) -> CommandError {
        malformed(self.kind, what)
    }

    /// The field at `at`; a record with fewer fields than its kind holds is
    /// malformed.
    pub fn field(&self, at: usize) -> Result<&'a Field, CommandError> {
        self.fields
            .get(at)
            .ok_or_else(|| self.wrong(format!("no field {at}")))
    }

    fn mistyped(&self, at: usize, wanted: &str) -> CommandError {
        self.wrong(format!("field {at} is not {wanted}"))
    }

    /// An unsigned field.
    pub fn unsigned(&self, at: usize) -> Result<u64, CommandError> {
        match self.field(at)? {
            Field::Unsigned(value) => Ok(*value),
            _ => Err(self.mistyped(at, "unsigned")),
        }
    }

    /// An unsigned field of at most `max`.
    pub fn bounded(&self, at: usize, max: u64) -> Result<u64, CommandError> {
        let value = self.unsigned(at)?;
        if value > max {
            return Err(self.wrong(format!("field {at} is {value}, past {max}")));
        }
        Ok(value)
    }

    /// An unsigned field that fits a `u32`.
    pub fn unsigned32(&self, at: usize) -> Result<u32, CommandError> {
        u32::try_from(self.unsigned(at)?)
            .map_err(|_| self.wrong(format!("field {at} past 32 bits")))
    }

    /// A flag: unsigned 0 or 1.
    pub fn flag(&self, at: usize) -> Result<bool, CommandError> {
        match self.unsigned(at)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(self.wrong(format!("field {at} is {other}, not a flag"))),
        }
    }

    /// A signed field.
    pub fn signed(&self, at: usize) -> Result<i64, CommandError> {
        match self.field(at)? {
            Field::Signed(value) => Ok(*value),
            _ => Err(self.mistyped(at, "signed")),
        }
    }

    /// A signed field, or nothing.
    pub fn optional_signed(&self, at: usize) -> Result<Option<i64>, CommandError> {
        match self.field(at)? {
            Field::Nothing => Ok(None),
            Field::Signed(value) => Ok(Some(*value)),
            _ => Err(self.mistyped(at, "signed or nothing")),
        }
    }

    /// A real field.
    pub fn real(&self, at: usize) -> Result<f64, CommandError> {
        match self.field(at)? {
            Field::Real(value) => Ok(*value),
            _ => Err(self.mistyped(at, "a real")),
        }
    }

    /// A real field, or nothing.
    pub fn optional_real(&self, at: usize) -> Result<Option<f64>, CommandError> {
        match self.field(at)? {
            Field::Nothing => Ok(None),
            Field::Real(value) => Ok(Some(*value)),
            _ => Err(self.mistyped(at, "a real or nothing")),
        }
    }

    /// A bytes field.
    pub fn bytes(&self, at: usize) -> Result<&'a Arc<[u8]>, CommandError> {
        match self.field(at)? {
            Field::Bytes(bytes) => Ok(bytes),
            _ => Err(self.mistyped(at, "bytes")),
        }
    }

    /// A bytes field of exactly `len` bytes.
    pub fn exact_bytes(&self, at: usize, len: usize) -> Result<&'a [u8], CommandError> {
        let bytes = self.bytes(at)?;
        if bytes.len() != len {
            return Err(self.wrong(format!("field {at} holds {} bytes, not {len}", bytes.len())));
        }
        Ok(bytes)
    }

    /// A bytes field, or nothing.
    pub fn optional_bytes(&self, at: usize) -> Result<Option<&'a Arc<[u8]>>, CommandError> {
        match self.field(at)? {
            Field::Nothing => Ok(None),
            Field::Bytes(bytes) => Ok(Some(bytes)),
            _ => Err(self.mistyped(at, "bytes or nothing")),
        }
    }

    /// A bytes field of UTF-8.
    pub fn text(&self, at: usize) -> Result<String, CommandError> {
        String::from_utf8(self.bytes(at)?.to_vec())
            .map_err(|_| self.wrong(format!("field {at} is not UTF-8")))
    }

    /// A bytes field of UTF-8, or nothing.
    pub fn optional_text(&self, at: usize) -> Result<Option<String>, CommandError> {
        match self.optional_bytes(at)? {
            None => Ok(None),
            Some(bytes) => String::from_utf8(bytes.to_vec())
                .map(Some)
                .map_err(|_| self.wrong(format!("field {at} is not UTF-8"))),
        }
    }

    /// A reals field.
    pub fn reals(&self, at: usize) -> Result<&'a [f64], CommandError> {
        match self.field(at)? {
            Field::Reals(reals) => Ok(reals),
            _ => Err(self.mistyped(at, "reals")),
        }
    }

    /// A reals field of exactly `len` reals.
    pub fn exact_reals(&self, at: usize, len: usize) -> Result<&'a [f64], CommandError> {
        let reals = self.reals(at)?;
        if reals.len() != len {
            return Err(self.wrong(format!("field {at} holds {} reals, not {len}", reals.len())));
        }
        Ok(reals)
    }

    /// A reals field of exactly `len` reals, or nothing.
    pub fn optional_reals(&self, at: usize, len: usize) -> Result<Option<&'a [f64]>, CommandError> {
        match self.field(at)? {
            Field::Nothing => Ok(None),
            Field::Reals(_) => self.exact_reals(at, len).map(Some),
            _ => Err(self.mistyped(at, "reals or nothing")),
        }
    }

    /// A naturals field.
    pub fn naturals(&self, at: usize) -> Result<&'a [u64], CommandError> {
        match self.field(at)? {
            Field::Naturals(naturals) => Ok(naturals),
            _ => Err(self.mistyped(at, "naturals")),
        }
    }

    /// A naturals field a later minor appended to its kind: the field where
    /// the record carries it, and none where the record, written by an
    /// earlier minor, ends before it.
    pub fn appended_naturals(&self, at: usize) -> Result<&'a [u64], CommandError> {
        if at >= self.fields.len() {
            return Ok(&[]);
        }
        self.naturals(at)
    }

    /// A naturals field, or nothing.
    pub fn optional_naturals(&self, at: usize) -> Result<Option<&'a [u64]>, CommandError> {
        match self.field(at)? {
            Field::Nothing => Ok(None),
            Field::Naturals(naturals) => Ok(Some(naturals)),
            _ => Err(self.mistyped(at, "naturals or nothing")),
        }
    }

    /// A strings field.
    pub fn strings(&self, at: usize) -> Result<&'a [Vec<u8>], CommandError> {
        match self.field(at)? {
            Field::Strings(strings) => Ok(strings),
            _ => Err(self.mistyped(at, "strings")),
        }
    }

    /// A reference: an unsigned field, or nothing for none.
    pub fn reference(&self, at: usize) -> Result<Option<u64>, CommandError> {
        match self.field(at)? {
            Field::Nothing => Ok(None),
            Field::Unsigned(value) => Ok(Some(*value)),
            _ => Err(self.mistyped(at, "a reference")),
        }
    }

    /// An unsigned field, or nothing.
    pub fn optional_unsigned(&self, at: usize) -> Result<Option<u64>, CommandError> {
        self.reference(at)
    }

    /// A flag, or nothing.
    pub fn optional_flag(&self, at: usize) -> Result<Option<bool>, CommandError> {
        match self.field(at)? {
            Field::Nothing => Ok(None),
            _ => self.flag(at).map(Some),
        }
    }
}

/// `values` as a reals field.
#[must_use]
pub fn reals(values: &[f64]) -> Field {
    Field::Reals(values.to_vec())
}

/// `value` as a flag.
#[must_use]
pub fn flag(value: bool) -> Field {
    Field::Unsigned(u64::from(value))
}

/// `value`, or nothing.
#[must_use]
pub fn optional_bytes(value: Option<&[u8]>) -> Field {
    value.map_or(Field::Nothing, Field::bytes)
}

/// `value`, or nothing.
#[must_use]
pub fn optional_real(value: Option<f64>) -> Field {
    value.map_or(Field::Nothing, Field::Real)
}

/// `value` as a reference, or nothing.
#[must_use]
pub fn reference(value: Option<u64>) -> Field {
    value.map_or(Field::Nothing, Field::Unsigned)
}

// ---- paths --------------------------------------------------------------------------

/// A path as its operators and numbers (`docs/design/command-stream.md`).
fn put_path(path: &[PathSegment]) -> (Field, Field) {
    let mut operators = Vec::with_capacity(path.len());
    let mut numbers = Vec::with_capacity(path.len() * 2);
    for segment in path {
        match *segment {
            PathSegment::MoveTo { x, y } => {
                operators.push(0);
                numbers.extend_from_slice(&[x, y]);
            }
            PathSegment::LineTo { x, y } => {
                operators.push(1);
                numbers.extend_from_slice(&[x, y]);
            }
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                operators.push(2);
                numbers.extend_from_slice(&[x1, y1, x2, y2, x3, y3]);
            }
            PathSegment::Close => operators.push(3),
        }
    }
    (Field::Bytes(Arc::from(operators)), Field::Reals(numbers))
}

/// A path read back from its operators and numbers, which must agree.
fn read_path(
    fields: &Fields<'_>,
    operators: usize,
    numbers: usize,
) -> Result<Vec<PathSegment>, CommandError> {
    let ops = fields.bytes(operators)?;
    let values = fields.reals(numbers)?;
    let mut path = Vec::with_capacity(ops.len());
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[f64], CommandError> {
        let taken = values
            .get(at..at + n)
            .ok_or_else(|| fields.wrong("a path with fewer numbers than its operators take"))?;
        at += n;
        Ok(taken)
    };
    for &op in ops.iter() {
        path.push(match op {
            0 => {
                let v = take(2)?;
                PathSegment::MoveTo { x: v[0], y: v[1] }
            }
            1 => {
                let v = take(2)?;
                PathSegment::LineTo { x: v[0], y: v[1] }
            }
            2 => {
                let v = take(6)?;
                PathSegment::CurveTo {
                    x1: v[0],
                    y1: v[1],
                    x2: v[2],
                    y2: v[3],
                    x3: v[4],
                    y3: v[5],
                }
            }
            3 => PathSegment::Close,
            other => return Err(fields.wrong(format!("a path operator {other}"))),
        });
    }
    if at != values.len() {
        return Err(fields.wrong("a path with more numbers than its operators take"));
    }
    Ok(path)
}

// ---- the enumerations ------------------------------------------------------------

fn render_mode_number(mode: TextRenderMode) -> u64 {
    match mode {
        TextRenderMode::Fill => 0,
        TextRenderMode::Stroke => 1,
        TextRenderMode::FillStroke => 2,
        TextRenderMode::Invisible => 3,
        TextRenderMode::FillClip => 4,
        TextRenderMode::StrokeClip => 5,
        TextRenderMode::FillStrokeClip => 6,
        TextRenderMode::Clip => 7,
    }
}

fn blend_number(blend: BlendMode) -> u64 {
    match blend {
        BlendMode::Normal => 0,
        BlendMode::Multiply => 1,
        BlendMode::Screen => 2,
        BlendMode::Overlay => 3,
        BlendMode::Darken => 4,
        BlendMode::Lighten => 5,
        BlendMode::ColorDodge => 6,
        BlendMode::ColorBurn => 7,
        BlendMode::HardLight => 8,
        BlendMode::SoftLight => 9,
        BlendMode::Difference => 10,
        BlendMode::Exclusion => 11,
        BlendMode::Hue => 12,
        BlendMode::Saturation => 13,
        BlendMode::Color => 14,
        BlendMode::Luminosity => 15,
    }
}

const BLENDS: [BlendMode; 16] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

fn family_number(family: ColorFamily) -> u64 {
    match family {
        ColorFamily::Gray => 0,
        ColorFamily::Rgb => 1,
        ColorFamily::Cmyk => 2,
        ColorFamily::Other => 3,
    }
}

const FAMILIES: [ColorFamily; 4] = [
    ColorFamily::Gray,
    ColorFamily::Rgb,
    ColorFamily::Cmyk,
    ColorFamily::Other,
];

/// A colour family's number on the wire: 0 grey, 1 RGB, 2 CMYK, 3 other.
#[must_use]
pub fn color_family_number(family: ColorFamily) -> u64 {
    family_number(family)
}

/// The colour family a number names.
#[must_use]
pub fn color_family_of(number: u64) -> Option<ColorFamily> {
    usize::try_from(number)
        .ok()
        .and_then(|at| FAMILIES.get(at))
        .copied()
}

fn intent_number(intent: RenderingIntent) -> u64 {
    match intent {
        RenderingIntent::AbsoluteColorimetric => 0,
        RenderingIntent::RelativeColorimetric => 1,
        RenderingIntent::Saturation => 2,
        RenderingIntent::Perceptual => 3,
        RenderingIntent::Unrecognised => 4,
    }
}

const INTENTS: [RenderingIntent; 5] = [
    RenderingIntent::AbsoluteColorimetric,
    RenderingIntent::RelativeColorimetric,
    RenderingIntent::Saturation,
    RenderingIntent::Perceptual,
    RenderingIntent::Unrecognised,
];

/// A rendering intent's number on the wire: Table 69's order, then the
/// unrecognised one.
#[must_use]
pub fn rendering_intent_number(intent: RenderingIntent) -> u64 {
    intent_number(intent)
}

/// The rendering intent a number names.
#[must_use]
pub fn rendering_intent_of(number: u64) -> Option<RenderingIntent> {
    usize::try_from(number)
        .ok()
        .and_then(|at| INTENTS.get(at))
        .copied()
}

fn group_space_number(space: GroupSpace) -> u64 {
    match space {
        GroupSpace::Gray => 0,
        GroupSpace::Rgb => 1,
        GroupSpace::Cmyk => 2,
        GroupSpace::Lab => 3,
    }
}

/// A group space's field: 0 grey, 1 RGB, 2 CMYK, 3 Lab, or nothing for none.
#[must_use]
pub fn group_space_field(space: Option<GroupSpace>) -> Field {
    space.map_or(Field::Nothing, |space| {
        Field::Unsigned(group_space_number(space))
    })
}

/// The group space a field names.
pub fn group_space_at(fields: &Fields<'_>, at: usize) -> Result<Option<GroupSpace>, CommandError> {
    Ok(match fields.optional_unsigned(at)? {
        None => None,
        Some(0) => Some(GroupSpace::Gray),
        Some(1) => Some(GroupSpace::Rgb),
        Some(2) => Some(GroupSpace::Cmyk),
        Some(3) => Some(GroupSpace::Lab),
        Some(other) => return Err(fields.wrong(format!("a group space {other}"))),
    })
}

fn matrix_field(m: &Matrix) -> Field {
    Field::Reals(vec![m.a, m.b, m.c, m.d, m.e, m.f])
}

/// A matrix's field: its six numbers.
#[must_use]
pub fn matrix_to_field(m: &Matrix) -> Field {
    matrix_field(m)
}

/// The matrix a field of six reals holds.
pub fn matrix_at(fields: &Fields<'_>, at: usize) -> Result<Matrix, CommandError> {
    let v = fields.exact_reals(at, 6)?;
    Ok(Matrix {
        a: v[0],
        b: v[1],
        c: v[2],
        d: v[3],
        e: v[4],
        f: v[5],
    })
}

fn rgb_field(c: Rgb) -> Field {
    Field::Bytes(Arc::from(&[c.r, c.g, c.b][..]))
}

fn rgb_at(fields: &Fields<'_>, at: usize) -> Result<Rgb, CommandError> {
    let v = fields.exact_bytes(at, 3)?;
    Ok(Rgb {
        r: v[0],
        g: v[1],
        b: v[2],
    })
}

fn ink_field(ink: Option<[u8; 4]>) -> Field {
    ink.map_or(Field::Nothing, |ink| Field::Bytes(Arc::from(&ink[..])))
}

fn ink_at(fields: &Fields<'_>, at: usize) -> Result<Option<[u8; 4]>, CommandError> {
    match fields.field(at)? {
        Field::Nothing => Ok(None),
        _ => {
            let v = fields.exact_bytes(at, 4)?;
            Ok(Some([v[0], v[1], v[2], v[3]]))
        }
    }
}

// ---- the definitions: functions ---------------------------------------------------

/// A calculator's program as its flat code, numbers and operators.
fn put_program(
    program: &[PsOp],
    code: &mut Vec<u8>,
    numbers: &mut Vec<f64>,
    names: &mut Vec<Vec<u8>>,
) {
    for op in program {
        match op {
            PsOp::Number(value) => {
                code.push(0);
                numbers.push(*value);
            }
            PsOp::Op(name) => {
                code.push(1);
                names.push(name.as_bytes().to_vec());
            }
            PsOp::Block(body) => {
                code.push(2);
                put_program(body, code, numbers, names);
                code.push(3);
            }
        }
    }
}

/// A calculator's program read back from its flat code, each operator by its
/// name in 7.10.5 and blocks nested at most `depth` deep.
fn read_program(
    fields: &Fields<'_>,
    code: &[u8],
    numbers: &[f64],
    names: &[Vec<u8>],
    depth: u32,
) -> Result<Vec<PsOp>, CommandError> {
    let mut stack: Vec<Vec<PsOp>> = vec![Vec::new()];
    let (mut number, mut name) = (0usize, 0usize);
    for &op in code {
        match op {
            0 => {
                let value = *numbers
                    .get(number)
                    .ok_or_else(|| fields.wrong("a calculator short of numbers"))?;
                number += 1;
                stack
                    .last_mut()
                    .ok_or_else(|| fields.wrong("a calculator block closed twice"))?
                    .push(PsOp::Number(value));
            }
            1 => {
                let spelled = names
                    .get(name)
                    .ok_or_else(|| fields.wrong("a calculator short of operators"))?;
                name += 1;
                // The name as the colour crate spells it, so the operator is
                // its own static name and the program evaluates as parsed.
                let parsed = parse_postscript(spelled);
                let op = match parsed.as_slice() {
                    [PsOp::Op(op)] if op.as_bytes() == spelled.as_slice() => *op,
                    _ => return Err(fields.wrong("a calculator operator 7.10.5 does not name")),
                };
                stack
                    .last_mut()
                    .ok_or_else(|| fields.wrong("a calculator block closed twice"))?
                    .push(PsOp::Op(op));
            }
            2 => {
                if stack.len() > depth as usize {
                    return Err(CommandError::OverBudget {
                        budget: "functions",
                    });
                }
                stack.push(Vec::new());
            }
            3 => {
                if stack.len() < 2 {
                    return Err(fields.wrong("a calculator block closed and never opened"));
                }
                let body = stack.pop().unwrap_or_default();
                stack
                    .last_mut()
                    .ok_or_else(|| fields.wrong("a calculator block closed twice"))?
                    .push(PsOp::Block(body));
            }
            other => return Err(fields.wrong(format!("a calculator code {other}"))),
        }
    }
    if stack.len() != 1 || number != numbers.len() || name != names.len() {
        return Err(fields.wrong("a calculator whose code, numbers and operators disagree"));
    }
    Ok(stack.pop().unwrap_or_default())
}

fn pairs(values: &[(f64, f64)]) -> Field {
    Field::Reals(values.iter().flat_map(|&(a, b)| [a, b]).collect())
}

fn pairs_at(fields: &Fields<'_>, at: usize) -> Result<Vec<(f64, f64)>, CommandError> {
    let v = fields.reals(at)?;
    if v.len() % 2 != 0 {
        return Err(fields.wrong(format!("field {at} holds an odd count of reals")));
    }
    Ok(v.chunks_exact(2).map(|p| (p[0], p[1])).collect())
}

fn pair_at(fields: &Fields<'_>, at: usize) -> Result<(f64, f64), CommandError> {
    let v = fields.exact_reals(at, 2)?;
    Ok((v[0], v[1]))
}

/// The bytes one function holds, its pieces' included: what copying it costs.
fn function_weight(function: &Function) -> u64 {
    let n = |len: usize| u64::try_from(len).unwrap_or(u64::MAX);
    let own = n(core::mem::size_of::<Function>());
    let parts = match function {
        Function::Sampled {
            domain,
            range,
            size,
            encode,
            decode,
            samples,
            ..
        } => n(domain.len() + range.len() + encode.len() + decode.len())
            .saturating_mul(16)
            .saturating_add(n(size.len()).saturating_mul(8))
            .saturating_add(n(samples.len())),
        Function::Exponential { c0, c1, .. } => n(c0.len() + c1.len()).saturating_mul(8),
        Function::Stitching {
            functions,
            bounds,
            encode,
            ..
        } => functions
            .iter()
            .map(function_weight)
            .fold(0u64, u64::saturating_add)
            .saturating_add(n(bounds.len()).saturating_mul(8))
            .saturating_add(n(encode.len()).saturating_mul(16)),
        Function::PostScript {
            domain,
            range,
            program,
        } => n(domain.len() + range.len())
            .saturating_mul(16)
            .saturating_add(n(program.len()).saturating_mul(n(core::mem::size_of::<PsOp>()))),
        Function::Array(functions) => functions
            .iter()
            .map(function_weight)
            .fold(0u64, u64::saturating_add),
        Function::Identity => 0,
    };
    own.saturating_add(parts)
}

// ---- writing -------------------------------------------------------------------------

/// The glyph run being gathered: its key and its glyphs.
struct Run {
    state: Option<u64>,
    font_id: u64,
    font_name: Option<Arc<str>>,
    size: u64,
    vertical: bool,
    glyphs: Vec<Glyph>,
}

/// A table of definitions written once each, by their encoded bytes.
#[derive(Default)]
struct Table {
    by_bytes: HashMap<Vec<u8>, u64>,
    count: u64,
}

impl Table {
    /// The index of `record` among this table's, writing it to `head` where it
    /// is new.
    fn index(&mut self, record: Record, head: &mut Vec<Record>) -> u64 {
        let mut bytes = Vec::new();
        put_record(&mut bytes, &record);
        if let Some(&index) = self.by_bytes.get(&bytes) {
            return index;
        }
        let index = self.count;
        self.count += 1;
        self.by_bytes.insert(bytes, index);
        head.push(record);
        index
    }
}

/// Writes a page's events, and the definitions their states are made of, as
/// records (`docs/design/command-stream.md`).
///
/// Definitions go to the head and commands to the body, so a stream written
/// as head then body has every definition before the first command that
/// refers to it; a caller adds its own records to either.
#[derive(Default)]
pub struct EventWriter {
    head: Vec<Record>,
    body: Vec<Record>,
    functions: Table,
    halftones: Table,
    stored: Table,
    colorants: Table,
    sources: Table,
    states: Table,
    /// The shared definitions already written, by the address of what they
    /// share, kept alive so an address is never another's.
    stored_by_address: HashMap<usize, (Arc<StoredParameters>, u64)>,
    colorants_by_address: HashMap<usize, (Arc<ColorantTints>, u64)>,
    sources_by_address: HashMap<usize, (Arc<ColorSource>, u64)>,
    run: Option<Run>,
}

impl EventWriter {
    /// A writer with nothing written.
    #[must_use]
    pub fn new() -> EventWriter {
        EventWriter::default()
    }

    /// Appends `record` to the head, among the definitions and tables.
    pub fn head(&mut self, record: Record) {
        self.head.push(record);
    }

    /// Appends `record` to the body, among the commands, after the glyph run
    /// being gathered.
    pub fn body(&mut self, record: Record) {
        self.flush();
        self.body.push(record);
    }

    /// The head and the body, the last glyph run written.
    #[must_use]
    pub fn finish(mut self) -> (Vec<Record>, Vec<Record>) {
        self.flush();
        (self.head, self.body)
    }

    /// How many commands the body holds so far, the glyph run being gathered
    /// counted as its glyphs: the event index a caller's record stands
    /// after.
    #[must_use]
    pub fn body_len(&self) -> usize {
        self.body.len()
    }

    /// The index of `function` among the stream's functions, written where
    /// it is new.
    pub fn function(&mut self, function: &Function) -> u64 {
        let record = self.function_record(function);
        self.functions.index(record, &mut self.head)
    }

    fn function_record(&mut self, function: &Function) -> Record {
        let record = Record::new(CommandKind::Function);
        match function {
            Function::Sampled {
                domain,
                range,
                size,
                bits,
                encode,
                decode,
                samples,
            } => record
                .with(Field::Unsigned(0))
                .with(pairs(domain))
                .with(pairs(range))
                .with(Field::Naturals(size.iter().map(|&s| s as u64).collect()))
                .with(Field::Unsigned(u64::from(*bits)))
                .with(pairs(encode))
                .with(pairs(decode))
                .with(Field::bytes(samples)),
            Function::Exponential { domain, c0, c1, n } => record
                .with(Field::Unsigned(2))
                .with(pairs(&[*domain]))
                .with(reals(c0))
                .with(reals(c1))
                .with(Field::Real(*n)),
            Function::Stitching {
                domain,
                functions,
                bounds,
                encode,
            } => {
                let pieces: Vec<u64> = functions.iter().map(|f| self.function(f)).collect();
                record
                    .with(Field::Unsigned(3))
                    .with(pairs(&[*domain]))
                    .with(Field::Naturals(pieces))
                    .with(reals(bounds))
                    .with(pairs(encode))
            }
            Function::PostScript {
                domain,
                range,
                program,
            } => {
                let (mut code, mut numbers, mut names) = (Vec::new(), Vec::new(), Vec::new());
                put_program(program, &mut code, &mut numbers, &mut names);
                record
                    .with(Field::Unsigned(4))
                    .with(pairs(domain))
                    .with(pairs(range))
                    .with(Field::Bytes(Arc::from(code)))
                    .with(Field::Reals(numbers))
                    .with(Field::Strings(names))
            }
            Function::Array(functions) => {
                let pieces: Vec<u64> = functions.iter().map(|f| self.function(f)).collect();
                record
                    .with(Field::Unsigned(5))
                    .with(Field::Naturals(pieces))
            }
            Function::Identity => record.with(Field::Unsigned(6)),
        }
    }

    fn transfer_field(&mut self, transfer: Option<&Transfer>) -> Field {
        match transfer {
            None => Field::Nothing,
            Some(Transfer::Identity) => Field::Naturals(Vec::new()),
            Some(Transfer::One(f)) => Field::Naturals(vec![self.function(f)]),
            Some(Transfer::Four(fs)) => {
                Field::Naturals(fs.iter().map(|f| self.function(f)).collect())
            }
        }
    }

    fn halftone(&mut self, halftone: &Halftone) -> u64 {
        let record = Record::new(CommandKind::Halftone);
        let record = match halftone {
            Halftone::Screen {
                frequency,
                angle,
                spot,
                accurate,
                transfer,
            } => {
                let (function, names) = match spot {
                    SpotFunction::Function(f) => (Field::Unsigned(self.function(f)), Vec::new()),
                    SpotFunction::Names(names) => (Field::Nothing, names.clone()),
                };
                let transfer = self.transfer_field(transfer.as_ref());
                record
                    .with(Field::Unsigned(1))
                    .with(Field::Real(*frequency))
                    .with(Field::Real(*angle))
                    .with(function)
                    .with(Field::Strings(names))
                    .with(flag(*accurate))
                    .with(transfer)
            }
            Halftone::Colorants(entries) => {
                let names: Vec<Vec<u8>> = entries.iter().map(|(name, _)| name.clone()).collect();
                let tones: Vec<u64> = entries.iter().map(|(_, h)| self.halftone(h)).collect();
                record
                    .with(Field::Unsigned(5))
                    .with(Field::Strings(names))
                    .with(Field::Naturals(tones))
            }
            Halftone::Threshold {
                width,
                height,
                thresholds,
                transfer,
            } => {
                let transfer = self.transfer_field(transfer.as_ref());
                record
                    .with(Field::Unsigned(6))
                    .with(Field::Signed(*width))
                    .with(Field::Signed(*height))
                    .with(Field::Unsigned(*thresholds))
                    .with(transfer)
            }
            Halftone::Squares {
                xsquare,
                ysquare,
                thresholds,
                transfer,
            } => {
                let transfer = self.transfer_field(transfer.as_ref());
                record
                    .with(Field::Unsigned(10))
                    .with(Field::Signed(*xsquare))
                    .with(Field::Signed(*ysquare))
                    .with(Field::Unsigned(*thresholds))
                    .with(transfer)
            }
            Halftone::Threshold16 {
                width,
                height,
                second,
                thresholds,
                transfer,
            } => {
                let transfer = self.transfer_field(transfer.as_ref());
                let (w2, h2) = match second {
                    Some((w, h)) => (Field::Signed(*w), Field::Signed(*h)),
                    None => (Field::Nothing, Field::Nothing),
                };
                record
                    .with(Field::Unsigned(16))
                    .with(Field::Signed(*width))
                    .with(Field::Signed(*height))
                    .with(w2)
                    .with(h2)
                    .with(Field::Unsigned(*thresholds))
                    .with(transfer)
            }
        };
        self.halftones.index(record, &mut self.head)
    }

    fn stored(&mut self, stored: &Arc<StoredParameters>) -> u64 {
        let address = Arc::as_ptr(stored) as usize;
        if let Some((_, index)) = self.stored_by_address.get(&address) {
            return *index;
        }
        let s = &**stored;
        let black = s.black_generation.as_ref().map(|f| self.function(f));
        let removal = s.undercolor_removal.as_ref().map(|f| self.function(f));
        let transfer = self.transfer_field(s.transfer.as_ref());
        let halftone = s.halftone.as_ref().map(|h| self.halftone(h));
        let bpc = match s.black_point_compensation {
            BlackPointCompensation::On => 0,
            BlackPointCompensation::Off => 1,
            BlackPointCompensation::Default => 2,
        };
        let record = Record::new(CommandKind::Stored)
            .with(Field::Unsigned(intent_number(s.rendering_intent)))
            .with(Field::Unsigned(bpc))
            .with(flag(s.stroke_overprint))
            .with(flag(s.fill_overprint))
            .with(Field::Signed(s.overprint_mode))
            .with(flag(s.alpha_is_shape))
            .with(flag(s.text_knockout))
            .with(optional_real(s.flatness))
            .with(optional_real(s.smoothness))
            .with(s.stroke_adjustment.map_or(Field::Nothing, flag))
            .with(reference(black))
            .with(reference(removal))
            .with(transfer)
            .with(reference(halftone))
            .with(s.halftone_origin.map_or(Field::Nothing, |o| reals(&o)));
        let index = self.stored.index(record, &mut self.head);
        self.stored_by_address
            .insert(address, (Arc::clone(stored), index));
        index
    }

    fn colorants(&mut self, colorants: &Arc<ColorantTints>) -> u64 {
        let address = Arc::as_ptr(colorants) as usize;
        if let Some((_, index)) = self.colorants_by_address.get(&address) {
            return *index;
        }
        let c = &**colorants;
        let kind = match c.kind {
            ColorantKind::DirectCmyk => 0,
            ColorantKind::Colorants => 1,
            ColorantKind::NChannel => 2,
            ColorantKind::All => 3,
            ColorantKind::None => 4,
        };
        let names: Vec<Vec<u8>> = c.tints.iter().map(|t| t.name.to_vec()).collect();
        let tints: Vec<u8> = c.tints.iter().map(|t| t.tint).collect();
        let own: Vec<u64> = c
            .tints
            .iter()
            .enumerate()
            .filter(|(_, t)| t.alone.is_some())
            .map(|(i, _)| i as u64)
            .collect();
        let inks: Vec<u8> = c.tints.iter().filter_map(|t| t.alone).flatten().collect();
        // A tint that is zero is the byte 0, so a reader takes every byte 0
        // for zero but these: the tints of no ink whose file value was not
        // zero (8.6.7).
        let not_zero: Vec<u64> = c
            .tints
            .iter()
            .enumerate()
            .filter(|(_, t)| t.tint == 0 && !t.zero)
            .map(|(i, _)| i as u64)
            .collect();
        let record = Record::new(CommandKind::Colorants)
            .with(Field::Unsigned(kind))
            .with(Field::Strings(names))
            .with(Field::Bytes(Arc::from(tints)))
            .with(Field::Naturals(own))
            .with(Field::Bytes(Arc::from(inks)))
            .with(ink_field(c.alternate))
            .with(ink_field(c.process))
            .with(Field::Naturals(not_zero));
        let index = self.colorants.index(record, &mut self.head);
        self.colorants_by_address
            .insert(address, (Arc::clone(colorants), index));
        index
    }

    fn source(&mut self, source: &Arc<ColorSource>) -> u64 {
        let address = Arc::as_ptr(source) as usize;
        if let Some((_, index)) = self.sources_by_address.get(&address) {
            return *index;
        }
        let record = Record::new(CommandKind::Source)
            .with(optional_bytes(source.family))
            .with(reals(&source.components));
        let index = self.sources.index(record, &mut self.head);
        self.sources_by_address
            .insert(address, (Arc::clone(source), index));
        index
    }

    /// The index of `state` among the stream's states, written where it is
    /// new, with every definition it refers to.
    pub fn state(&mut self, state: &GraphicsState) -> u64 {
        let fill_colorants = state.fill_colorants.as_ref().map(|c| self.colorants(c));
        let stroke_colorants = state.stroke_colorants.as_ref().map(|c| self.colorants(c));
        let stored = state.stored.as_ref().map(|s| self.stored(s));
        let fill_source = state.fill_source.as_ref().map(|s| self.source(s));
        let stroke_source = state.stroke_source.as_ref().map(|s| self.source(s));
        let t = &state.text;
        let record = Record::new(CommandKind::State)
            .with(matrix_field(&state.ctm))
            .with(reals(&[
                t.size,
                t.char_spacing,
                t.word_spacing,
                t.horizontal_scale,
                t.leading,
                t.rise,
            ]))
            .with(Field::Unsigned(render_mode_number(t.render_mode)))
            .with(optional_bytes(t.font.as_deref()))
            .with(Field::Real(state.fill_alpha))
            .with(Field::Real(state.stroke_alpha))
            .with(rgb_field(state.fill_color))
            .with(rgb_field(state.stroke_color))
            .with(Field::Naturals(vec![
                u64::from(state.fill_wide.r),
                u64::from(state.fill_wide.g),
                u64::from(state.fill_wide.b),
                u64::from(state.stroke_wide.r),
                u64::from(state.stroke_wide.g),
                u64::from(state.stroke_wide.b),
            ]))
            .with(ink_field(state.fill_ink))
            .with(ink_field(state.stroke_ink))
            .with(Field::Unsigned(family_number(state.fill_family)))
            .with(Field::Unsigned(family_number(state.stroke_family)))
            .with(reference(fill_colorants))
            .with(reference(stroke_colorants))
            .with(Field::Real(state.line_width))
            .with(Field::Unsigned(match state.line_cap {
                LineCap::Butt => 0,
                LineCap::Round => 1,
                LineCap::Square => 2,
            }))
            .with(Field::Unsigned(match state.line_join {
                LineJoin::Miter => 0,
                LineJoin::Round => 1,
                LineJoin::Bevel => 2,
            }))
            .with(Field::Real(state.miter_limit))
            .with(reals(&state.dashes))
            .with(Field::Real(state.dash_phase))
            .with(optional_bytes(state.fill_space.as_deref()))
            .with(optional_bytes(state.stroke_space.as_deref()))
            .with(optional_bytes(state.fill_pattern.as_deref()))
            .with(Field::Unsigned(blend_number(state.blend)))
            .with(optional_bytes(state.stroke_pattern.as_deref()))
            .with(reference(stored))
            .with(reference(fill_source))
            .with(reference(stroke_source));
        self.states.index(record, &mut self.head)
    }

    fn state_field(&mut self, state: &Option<Box<GraphicsState>>) -> Field {
        reference(state.as_deref().map(|s| self.state(s)))
    }

    /// Writes the glyph run being gathered.
    fn flush(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        let n = run.glyphs.len();
        let mut codes = Vec::with_capacity(n);
        let mut lengths = Vec::with_capacity(n);
        let mut text = Vec::new();
        let mut transforms = Vec::with_capacity(n * 6);
        let mut advances = Vec::with_capacity(n);
        let mut lifted = Vec::new();
        let mut baselines = Vec::new();
        for (at, glyph) in run.glyphs.iter().enumerate() {
            codes.push(u64::from(glyph.code));
            lengths.push(glyph.text.len() as u64);
            text.extend_from_slice(glyph.text.as_bytes());
            let m = &glyph.transform;
            transforms.extend_from_slice(&[m.a, m.b, m.c, m.d, m.e, m.f]);
            advances.push(glyph.advance);
            if let Some((x, y)) = glyph.baseline {
                lifted.push(at as u64);
                baselines.extend_from_slice(&[x, y]);
            }
        }
        self.body.push(
            Record::new(CommandKind::GlyphRun)
                .with(reference(run.state))
                .with(Field::Unsigned(run.font_id))
                .with(optional_bytes(run.font_name.as_deref().map(str::as_bytes)))
                .with(Field::Real(f64::from_bits(run.size)))
                .with(flag(run.vertical))
                .with(Field::Naturals(codes))
                .with(Field::Naturals(lengths))
                .with(Field::Bytes(Arc::from(text)))
                .with(Field::Reals(transforms))
                .with(Field::Reals(advances))
                .with(Field::Naturals(lifted))
                .with(Field::Reals(baselines)),
        );
    }

    /// Writes one event, gathering glyphs into runs.
    pub fn event(&mut self, event: &Event) {
        if let Event::ShowGlyph { glyph, state } = event {
            let state = state.as_deref().map(|s| self.state(s));
            let size = glyph.size.to_bits();
            let joins = self.run.as_ref().is_some_and(|run| {
                run.state == state
                    && run.font_id == glyph.font_id
                    && run.font_name == glyph.font_name
                    && run.size == size
                    && run.vertical == glyph.vertical
                    && run.glyphs.len() < MAX_RUN
            });
            if !joins {
                self.flush();
                self.run = Some(Run {
                    state,
                    font_id: glyph.font_id,
                    font_name: glyph.font_name.clone(),
                    size,
                    vertical: glyph.vertical,
                    glyphs: Vec::new(),
                });
            }
            if let Some(run) = self.run.as_mut() {
                run.glyphs.push(glyph.clone());
            }
            return;
        }
        self.flush();
        let record = match event {
            Event::ShowGlyph { .. } => return,
            Event::BeginText => Record::new(CommandKind::BeginText),
            Event::EndText => Record::new(CommandKind::EndText),
            Event::FillPath {
                path,
                even_odd,
                state,
            } => {
                let state = self.state_field(state);
                let (ops, numbers) = put_path(path);
                Record::new(CommandKind::FillPath)
                    .with(state)
                    .with(flag(*even_odd))
                    .with(ops)
                    .with(numbers)
            }
            Event::StrokePath { path, state } => {
                let state = self.state_field(state);
                let (ops, numbers) = put_path(path);
                Record::new(CommandKind::StrokePath)
                    .with(state)
                    .with(ops)
                    .with(numbers)
            }
            Event::ClipPath {
                path,
                even_odd,
                state,
            } => {
                let state = self.state_field(state);
                let (ops, numbers) = put_path(path);
                Record::new(CommandKind::ClipPath)
                    .with(state)
                    .with(flag(*even_odd))
                    .with(ops)
                    .with(numbers)
            }
            Event::SaveState => Record::new(CommandKind::SaveState),
            Event::RestoreState => Record::new(CommandKind::RestoreState),
            Event::DrawImage { image, state } => {
                let state = self.state_field(state);
                Record::new(CommandKind::DrawImage)
                    .with(state)
                    .with(Field::bytes(&image.name))
                    .with(flag(image.inline))
                    .with(Field::bytes(&image.inline_dict))
                    .with(Field::bytes(&image.inline_data))
            }
            Event::DrawShading { name, state } => {
                let state = self.state_field(state);
                Record::new(CommandKind::DrawShading)
                    .with(state)
                    .with(Field::bytes(name))
            }
            Event::BeginMarkedContent(scope) => {
                let record = Record::new(CommandKind::BeginMarkedContent)
                    .with(Field::bytes(&scope.tag))
                    .with(flag(scope.visible))
                    .with(optional_bytes(
                        scope.hidden_layer.as_deref().map(str::as_bytes),
                    ));
                match &scope.props {
                    None => record
                        .with(flag(false))
                        .with(Field::Nothing)
                        .with(Field::Nothing)
                        .with(Field::Nothing)
                        .with(Field::Nothing)
                        .with(Field::Nothing)
                        .with(Field::Unsigned(0))
                        .with(Field::Nothing),
                    Some(props) => {
                        let text =
                            |s: &Option<String>| optional_bytes(s.as_deref().map(str::as_bytes));
                        record
                            .with(flag(true))
                            .with(reference(props.mcid.map(u64::from)))
                            .with(text(&props.actual_text))
                            .with(text(&props.alt))
                            .with(text(&props.lang))
                            .with(text(&props.expansion))
                            .with(Field::Unsigned(props.stream))
                            .with(optional_bytes(props.associated_files.as_deref()))
                    }
                }
            }
            Event::EndMarkedContent => Record::new(CommandKind::EndMarkedContent),
            Event::BeginForm { id, name, entered } => Record::new(CommandKind::BeginForm)
                .with(Field::Unsigned(*id))
                .with(Field::bytes(name))
                .with(flag(*entered)),
            Event::EndForm { id } => Record::new(CommandKind::EndForm).with(Field::Unsigned(*id)),
            Event::BeginGroup {
                group,
                entered,
                state,
            } => {
                let state = self.state_field(state);
                Record::new(CommandKind::BeginGroup)
                    .with(state)
                    .with(flag(group.isolated))
                    .with(flag(group.knockout))
                    .with(group_space_field(group.space))
                    .with(flag(*entered))
            }
            Event::EndGroup => Record::new(CommandKind::EndGroup),
            Event::BeginSoftMask {
                mask,
                bbox,
                entered,
                state,
            } => {
                let state = self.state_field(state);
                let form = &mask.form;
                let (group, isolated, knockout, space) = match form.group {
                    Some(g) => (
                        flag(true),
                        flag(g.isolated),
                        flag(g.knockout),
                        group_space_field(g.space),
                    ),
                    None => (flag(false), flag(false), flag(false), Field::Nothing),
                };
                let (ops, numbers) = put_path(bbox);
                Record::new(CommandKind::BeginSoftMask)
                    .with(state)
                    .with(Field::bytes(&form.content))
                    .with(matrix_field(&form.matrix))
                    .with(form.bbox.map_or(Field::Nothing, |b| reals(&b)))
                    .with(group)
                    .with(isolated)
                    .with(knockout)
                    .with(space)
                    .with(Field::Unsigned(form.stream))
                    .with(flag(mask.luminosity))
                    .with(mask.backdrop.map_or(Field::Nothing, rgb_field))
                    .with(
                        mask.transfer
                            .map_or(Field::Nothing, |t| Field::Bytes(Arc::from(&t[..]))),
                    )
                    .with(ops)
                    .with(numbers)
                    .with(flag(*entered))
            }
            Event::EndSoftMask => Record::new(CommandKind::EndSoftMask),
            Event::ClearSoftMask => Record::new(CommandKind::ClearSoftMask),
            Event::GroupBudgetSpent => Record::new(CommandKind::GroupBudgetSpent),
            Event::RecursionRefused => Record::new(CommandKind::RecursionRefused),
            Event::NestedBudgetSpent => Record::new(CommandKind::NestedBudgetSpent),
        };
        self.body.push(record);
    }
}

// ---- reading -------------------------------------------------------------------------

/// What [`EventReader::read`] did with a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A definition, kept for the records after it.
    Defined,
    /// A command, its events appended.
    Commanded,
    /// A kind this module does not read: the page and its tables, the
    /// facade's.
    Foreign(CommandKind),
    /// A kind no version this build knows defines, in a stream of a newer
    /// minor, skipped.
    Skipped,
}

/// A definition read back, with what nesting it holds and what copying it
/// costs.
struct Defined<T> {
    value: Arc<T>,
    depth: u32,
    weight: u64,
}

/// Reads a stream's records back into events, keeping the definitions their
/// states refer to.
pub struct EventReader {
    minor: u16,
    functions: Vec<Defined<Function>>,
    halftones: Vec<Defined<Halftone>>,
    stored: Vec<Arc<StoredParameters>>,
    colorants: Vec<Arc<ColorantTints>>,
    sources: Vec<Arc<ColorSource>>,
    /// Each state, with the bytes a copy of it owns: a command copies its
    /// state, so a reference is checked against the budget before it is.
    states: Vec<(Arc<GraphicsState>, u64)>,
}

/// The bytes a copy of `state` owns beside what it shares: its own size, its
/// dashes and the names it copies.
fn state_weight(state: &GraphicsState) -> u64 {
    let n = |len: usize| u64::try_from(len).unwrap_or(u64::MAX);
    let name = |name: &Option<Vec<u8>>| name.as_ref().map_or(0, |name| n(name.len()));
    n(core::mem::size_of::<GraphicsState>())
        .saturating_add(n(state.dashes.len()).saturating_mul(8))
        .saturating_add(name(&state.text.font))
        .saturating_add(name(&state.fill_space))
        .saturating_add(name(&state.stroke_space))
        .saturating_add(name(&state.fill_pattern))
        .saturating_add(name(&state.stroke_pattern))
}

impl EventReader {
    /// A reader for a stream of minor version `minor`, with nothing defined.
    #[must_use]
    pub fn new(minor: u16) -> EventReader {
        EventReader {
            minor,
            functions: Vec::new(),
            halftones: Vec::new(),
            stored: Vec::new(),
            colorants: Vec::new(),
            sources: Vec::new(),
            states: Vec::new(),
        }
    }

    /// The function a reference names, with what copying it costs.
    #[must_use]
    pub fn function(&self, index: u64) -> Option<(&Arc<Function>, u64)> {
        let defined = self.functions.get(usize::try_from(index).ok()?)?;
        Some((&defined.value, defined.weight))
    }

    /// How deep the function a reference names nests.
    #[must_use]
    pub fn function_depth(&self, index: u64) -> Option<u32> {
        let defined = self.functions.get(usize::try_from(index).ok()?)?;
        Some(defined.depth)
    }

    /// How many functions the stream has defined so far.
    #[must_use]
    pub fn functions_defined(&self) -> usize {
        self.functions.len()
    }

    /// The state a reference names.
    #[must_use]
    pub fn state(&self, index: u64) -> Option<&Arc<GraphicsState>> {
        self.states
            .get(usize::try_from(index).ok()?)
            .map(|(state, _)| state)
    }

    /// Reads `record`: a definition kept, a command's events appended to
    /// `events`, a foreign kind handed back, an unknown one skipped or
    /// refused by the minor's rule ([`unknown_kind`]).
    pub fn read(
        &mut self,
        record: &Record,
        events: &mut Vec<Event>,
        meter: &mut Meter,
    ) -> Result<Outcome, CommandError> {
        let Some(kind) = CommandKind::from_number(record.kind) else {
            unknown_kind(record.kind, self.minor)?;
            return Ok(Outcome::Skipped);
        };
        if !kind.is_codec() {
            return Ok(Outcome::Foreign(kind));
        }
        let fields = record.read();
        match kind {
            CommandKind::End => Err(fields.wrong("an End record among the records")),
            CommandKind::Function => self
                .read_function(&fields, meter)
                .map(|()| Outcome::Defined),
            CommandKind::Halftone => self
                .read_halftone(&fields, meter)
                .map(|()| Outcome::Defined),
            CommandKind::Stored => self.read_stored(&fields, meter).map(|()| Outcome::Defined),
            CommandKind::Colorants => self
                .read_colorants(&fields, meter)
                .map(|()| Outcome::Defined),
            CommandKind::Source => self.read_source(&fields, meter).map(|()| Outcome::Defined),
            CommandKind::State => self.read_state(&fields, meter).map(|()| Outcome::Defined),
            _ => {
                self.read_command(kind, &fields, events, meter)?;
                Ok(Outcome::Commanded)
            }
        }
    }

    fn function_at(
        &self,
        fields: &Fields<'_>,
        index: u64,
    ) -> Result<&Defined<Function>, CommandError> {
        usize::try_from(index)
            .ok()
            .and_then(|at| self.functions.get(at))
            .ok_or_else(|| {
                fields.wrong(format!("function {index}, which no record before defines"))
            })
    }

    fn read_function(
        &mut self,
        fields: &Fields<'_>,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let limit = meter.budgets().functions;
        // A piece is copied into the function that holds it, so what the
        // copies cost and how deep they nest are checked before any is made:
        // a chain of functions each holding the last twice doubles.
        let pieces =
            |reader: &EventReader, at: usize| -> Result<(Vec<Function>, u32), CommandError> {
                let mut held = Vec::new();
                let (mut depth, mut weight) = (0u32, 0u64);
                for &index in fields.naturals(at)? {
                    let piece = reader.function_at(fields, index)?;
                    depth = depth.max(piece.depth);
                    weight = weight.saturating_add(piece.weight);
                    held.push(piece);
                }
                if depth >= limit {
                    return Err(CommandError::OverBudget {
                        budget: "functions",
                    });
                }
                if !meter.can_hold(weight) {
                    return Err(CommandError::OverBudget { budget: "bytes" });
                }
                Ok((
                    held.iter().map(|piece| (*piece.value).clone()).collect(),
                    depth,
                ))
            };
        let (function, depth) = match fields.unsigned(0)? {
            0 => {
                let size = fields
                    .naturals(3)?
                    .iter()
                    .map(|&s| {
                        usize::try_from(s).map_err(|_| fields.wrong("a sample count past usize"))
                    })
                    .collect::<Result<Vec<usize>, _>>()?;
                (
                    Function::Sampled {
                        domain: pairs_at(fields, 1)?,
                        range: pairs_at(fields, 2)?,
                        size,
                        bits: fields.unsigned32(4)?,
                        encode: pairs_at(fields, 5)?,
                        decode: pairs_at(fields, 6)?,
                        samples: fields.bytes(7)?.to_vec(),
                    },
                    0,
                )
            }
            2 => (
                Function::Exponential {
                    domain: pair_at(fields, 1)?,
                    c0: fields.reals(2)?.to_vec(),
                    c1: fields.reals(3)?.to_vec(),
                    n: fields.real(4)?,
                },
                0,
            ),
            3 => {
                let (functions, depth) = pieces(self, 2)?;
                (
                    Function::Stitching {
                        domain: pair_at(fields, 1)?,
                        functions,
                        bounds: fields.reals(3)?.to_vec(),
                        encode: pairs_at(fields, 4)?,
                    },
                    depth + 1,
                )
            }
            4 => {
                let program = read_program(
                    fields,
                    fields.bytes(3)?,
                    fields.reals(4)?,
                    fields.strings(5)?,
                    limit,
                )?;
                (
                    Function::PostScript {
                        domain: pairs_at(fields, 1)?,
                        range: pairs_at(fields, 2)?,
                        program,
                    },
                    0,
                )
            }
            5 => {
                let (functions, depth) = pieces(self, 1)?;
                (Function::Array(functions), depth + 1)
            }
            6 => (Function::Identity, 0),
            other => return Err(fields.wrong(format!("a function of type {other}"))),
        };
        if depth > limit {
            return Err(CommandError::OverBudget {
                budget: "functions",
            });
        }
        let weight = function_weight(&function);
        meter.spend(weight)?;
        self.functions.push(Defined {
            value: Arc::new(function),
            depth,
            weight,
        });
        Ok(())
    }

    fn shared_function(
        &self,
        fields: &Fields<'_>,
        index: u64,
    ) -> Result<Arc<Function>, CommandError> {
        Ok(Arc::clone(&self.function_at(fields, index)?.value))
    }

    fn transfer_at(
        &self,
        fields: &Fields<'_>,
        at: usize,
    ) -> Result<Option<Transfer>, CommandError> {
        let Some(refs) = fields.optional_naturals(at)? else {
            return Ok(None);
        };
        Ok(Some(match refs {
            [] => Transfer::Identity,
            [one] => Transfer::One(self.shared_function(fields, *one)?),
            [a, b, c, d] => Transfer::Four([
                self.shared_function(fields, *a)?,
                self.shared_function(fields, *b)?,
                self.shared_function(fields, *c)?,
                self.shared_function(fields, *d)?,
            ]),
            _ => return Err(fields.wrong("a transfer of neither one nor four functions")),
        }))
    }

    fn read_halftone(
        &mut self,
        fields: &Fields<'_>,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let (halftone, depth, weight) = match fields.unsigned(0)? {
            1 => {
                let spot = match fields.reference(3)? {
                    Some(index) => SpotFunction::Function(self.shared_function(fields, index)?),
                    None => SpotFunction::Names(fields.strings(4)?.to_vec()),
                };
                (
                    Halftone::Screen {
                        frequency: fields.real(1)?,
                        angle: fields.real(2)?,
                        spot,
                        accurate: fields.flag(5)?,
                        transfer: self.transfer_at(fields, 6)?,
                    },
                    0,
                    0,
                )
            }
            5 => {
                let names = fields.strings(1)?;
                let tones = fields.naturals(2)?;
                if names.len() != tones.len() {
                    return Err(fields.wrong("colorant halftones and names of two lengths"));
                }
                // Each colorant's halftone is copied in, so the copies are
                // checked before they are made, as a function's pieces are.
                let mut held = Vec::with_capacity(names.len());
                let (mut depth, mut weight) = (0u32, 0u64);
                for &index in tones {
                    let tone = usize::try_from(index)
                        .ok()
                        .and_then(|at| self.halftones.get(at))
                        .ok_or_else(|| {
                            fields
                                .wrong(format!("halftone {index}, which no record before defines"))
                        })?;
                    depth = depth.max(tone.depth);
                    weight = weight.saturating_add(tone.weight);
                    held.push(tone);
                }
                if depth >= meter.budgets().spaces {
                    return Err(CommandError::OverBudget { budget: "spaces" });
                }
                if !meter.can_hold(weight) {
                    return Err(CommandError::OverBudget { budget: "bytes" });
                }
                let entries = names
                    .iter()
                    .zip(&held)
                    .map(|(name, tone)| (name.clone(), (*tone.value).clone()))
                    .collect();
                (Halftone::Colorants(entries), depth + 1, weight)
            }
            6 => (
                Halftone::Threshold {
                    width: fields.signed(1)?,
                    height: fields.signed(2)?,
                    thresholds: fields.unsigned(3)?,
                    transfer: self.transfer_at(fields, 4)?,
                },
                0,
                0,
            ),
            10 => (
                Halftone::Squares {
                    xsquare: fields.signed(1)?,
                    ysquare: fields.signed(2)?,
                    thresholds: fields.unsigned(3)?,
                    transfer: self.transfer_at(fields, 4)?,
                },
                0,
                0,
            ),
            16 => {
                let second = match (fields.optional_signed(3)?, fields.optional_signed(4)?) {
                    (Some(w), Some(h)) => Some((w, h)),
                    (None, None) => None,
                    _ => return Err(fields.wrong("a second rectangle with one side")),
                };
                (
                    Halftone::Threshold16 {
                        width: fields.signed(1)?,
                        height: fields.signed(2)?,
                        second,
                        thresholds: fields.unsigned(5)?,
                        transfer: self.transfer_at(fields, 6)?,
                    },
                    0,
                    0,
                )
            }
            other => return Err(fields.wrong(format!("a halftone of type {other}"))),
        };
        if depth > meter.budgets().spaces {
            return Err(CommandError::OverBudget { budget: "spaces" });
        }
        let weight = weight.saturating_add(core::mem::size_of::<Halftone>() as u64);
        meter.spend(weight)?;
        self.halftones.push(Defined {
            value: Arc::new(halftone),
            depth,
            weight,
        });
        Ok(())
    }

    fn read_stored(&mut self, fields: &Fields<'_>, meter: &mut Meter) -> Result<(), CommandError> {
        let intent = rendering_intent_of(fields.unsigned(0)?)
            .ok_or_else(|| fields.wrong("a rendering intent past Table 69's"))?;
        let bpc = match fields.unsigned(1)? {
            0 => BlackPointCompensation::On,
            1 => BlackPointCompensation::Off,
            2 => BlackPointCompensation::Default,
            other => return Err(fields.wrong(format!("a black-point compensation {other}"))),
        };
        let function = |at: usize| -> Result<Option<Arc<Function>>, CommandError> {
            fields
                .reference(at)?
                .map(|index| self.shared_function(fields, index))
                .transpose()
        };
        let halftone = match fields.reference(13)? {
            None => None,
            Some(index) => Some(Arc::clone(
                &usize::try_from(index)
                    .ok()
                    .and_then(|at| self.halftones.get(at))
                    .ok_or_else(|| {
                        fields.wrong(format!("halftone {index}, which no record before defines"))
                    })?
                    .value,
            )),
        };
        let origin = fields.optional_reals(14, 2)?.map(|o| [o[0], o[1]]);
        let stored = StoredParameters {
            rendering_intent: intent,
            black_point_compensation: bpc,
            stroke_overprint: fields.flag(2)?,
            fill_overprint: fields.flag(3)?,
            overprint_mode: fields.signed(4)?,
            alpha_is_shape: fields.flag(5)?,
            text_knockout: fields.flag(6)?,
            flatness: fields.optional_real(7)?,
            smoothness: fields.optional_real(8)?,
            stroke_adjustment: fields.optional_flag(9)?,
            black_generation: function(10)?,
            undercolor_removal: function(11)?,
            transfer: self.transfer_at(fields, 12)?,
            halftone,
            halftone_origin: origin,
        };
        meter.spend(core::mem::size_of::<StoredParameters>() as u64)?;
        self.stored.push(Arc::new(stored));
        Ok(())
    }

    fn read_colorants(
        &mut self,
        fields: &Fields<'_>,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let kind = match fields.unsigned(0)? {
            0 => ColorantKind::DirectCmyk,
            1 => ColorantKind::Colorants,
            2 => ColorantKind::NChannel,
            3 => ColorantKind::All,
            4 => ColorantKind::None,
            other => return Err(fields.wrong(format!("a colorant kind {other}"))),
        };
        let names = fields.strings(1)?;
        let tints = fields.bytes(2)?;
        let own = fields.naturals(3)?;
        let inks = fields.bytes(4)?;
        if names.len() != tints.len() || inks.len() != own.len() * 4 {
            return Err(fields.wrong("colorants whose names, tints and inks disagree"));
        }
        let mut tints: Vec<ColorantTint> = names
            .iter()
            .zip(tints.iter())
            .map(|(name, &tint)| ColorantTint {
                name: Arc::from(name.as_slice()),
                tint,
                alone: None,
                zero: tint == 0,
            })
            .collect();
        let mut last: Option<u64> = None;
        for (at, &index) in own.iter().enumerate() {
            if last.is_some_and(|last| index <= last) {
                return Err(fields.wrong("own inks out of order"));
            }
            last = Some(index);
            let slot = usize::try_from(index)
                .ok()
                .and_then(|i| tints.get_mut(i))
                .ok_or_else(|| fields.wrong("an own ink past the colorants"))?;
            let ink = &inks[at * 4..at * 4 + 4];
            slot.alone = Some([ink[0], ink[1], ink[2], ink[3]]);
        }
        // Minor 1: the tints of no ink that are not zero, in order; a tint
        // of ink is never zero, so one listed is malformed.
        let mut last: Option<u64> = None;
        for &index in fields.appended_naturals(7)? {
            if last.is_some_and(|last| index <= last) {
                return Err(fields.wrong("tints not zero out of order"));
            }
            last = Some(index);
            let slot = usize::try_from(index)
                .ok()
                .and_then(|i| tints.get_mut(i))
                .filter(|slot| slot.tint == 0)
                .ok_or_else(|| fields.wrong("a tint not zero that is no tint of no ink"))?;
            slot.zero = false;
        }
        let colorants = ColorantTints {
            kind,
            tints,
            alternate: ink_at(fields, 5)?,
            process: ink_at(fields, 6)?,
        };
        meter.spend(core::mem::size_of::<ColorantTints>() as u64)?;
        self.colorants.push(Arc::new(colorants));
        Ok(())
    }

    fn read_source(&mut self, fields: &Fields<'_>, meter: &mut Meter) -> Result<(), CommandError> {
        let family: Option<&'static [u8]> = match fields.optional_bytes(0)? {
            None => None,
            Some(name) => Some(match &name[..] {
                b"DeviceGray" => b"DeviceGray",
                b"DeviceRGB" => b"DeviceRGB",
                b"DeviceCMYK" => b"DeviceCMYK",
                _ => return Err(fields.wrong("a colour source in no device space")),
            }),
        };
        let source = ColorSource {
            family,
            components: fields.reals(1)?.to_vec(),
        };
        meter.spend(core::mem::size_of::<ColorSource>() as u64)?;
        self.sources.push(Arc::new(source));
        Ok(())
    }

    fn read_state(&mut self, fields: &Fields<'_>, meter: &mut Meter) -> Result<(), CommandError> {
        let text = fields.exact_reals(1, 6)?;
        let render_mode = match fields.unsigned(2)? {
            n @ 0..=7 => TextRenderMode::from_i64(n as i64),
            other => return Err(fields.wrong(format!("a render mode {other}"))),
        };
        let wide = fields.naturals(8)?;
        if wide.len() != 6 {
            return Err(fields.wrong("sixteen-bit colours of other than six channels"));
        }
        let wide: Vec<u16> = wide
            .iter()
            .map(|&v| {
                u16::try_from(v).map_err(|_| fields.wrong("a sixteen-bit channel past 16 bits"))
            })
            .collect::<Result<_, _>>()?;
        let family = |at: usize| -> Result<ColorFamily, CommandError> {
            color_family_of(fields.unsigned(at)?)
                .ok_or_else(|| fields.wrong("a colour family past 3"))
        };
        let shared = |table: &[Arc<ColorantTints>],
                      at: usize|
         -> Result<Option<Arc<ColorantTints>>, CommandError> {
            fields
                .reference(at)?
                .map(|index| {
                    usize::try_from(index)
                        .ok()
                        .and_then(|i| table.get(i))
                        .cloned()
                        .ok_or_else(|| {
                            fields
                                .wrong(format!("colorants {index}, which no record before defines"))
                        })
                })
                .transpose()
        };
        let source = |at: usize| -> Result<Option<Arc<ColorSource>>, CommandError> {
            fields
                .reference(at)?
                .map(|index| {
                    usize::try_from(index)
                        .ok()
                        .and_then(|i| self.sources.get(i))
                        .cloned()
                        .ok_or_else(|| {
                            fields.wrong(format!("source {index}, which no record before defines"))
                        })
                })
                .transpose()
        };
        let stored = fields
            .reference(26)?
            .map(|index| {
                usize::try_from(index)
                    .ok()
                    .and_then(|i| self.stored.get(i))
                    .cloned()
                    .ok_or_else(|| {
                        fields.wrong(format!(
                            "stored parameters {index}, which no record before defines"
                        ))
                    })
            })
            .transpose()?;
        let state = GraphicsState {
            ctm: matrix_at(fields, 0)?,
            text: TextState {
                size: text[0],
                char_spacing: text[1],
                word_spacing: text[2],
                horizontal_scale: text[3],
                leading: text[4],
                rise: text[5],
                render_mode,
                font: fields.optional_bytes(3)?.map(|b| b.to_vec()),
            },
            fill_alpha: fields.real(4)?,
            stroke_alpha: fields.real(5)?,
            fill_color: rgb_at(fields, 6)?,
            stroke_color: rgb_at(fields, 7)?,
            fill_wide: Rgb16 {
                r: wide[0],
                g: wide[1],
                b: wide[2],
            },
            stroke_wide: Rgb16 {
                r: wide[3],
                g: wide[4],
                b: wide[5],
            },
            fill_ink: ink_at(fields, 9)?,
            stroke_ink: ink_at(fields, 10)?,
            fill_family: family(11)?,
            stroke_family: family(12)?,
            fill_colorants: shared(&self.colorants, 13)?,
            stroke_colorants: shared(&self.colorants, 14)?,
            line_width: fields.real(15)?,
            line_cap: match fields.unsigned(16)? {
                0 => LineCap::Butt,
                1 => LineCap::Round,
                2 => LineCap::Square,
                other => return Err(fields.wrong(format!("a line cap {other}"))),
            },
            line_join: match fields.unsigned(17)? {
                0 => LineJoin::Miter,
                1 => LineJoin::Round,
                2 => LineJoin::Bevel,
                other => return Err(fields.wrong(format!("a line join {other}"))),
            },
            miter_limit: fields.real(18)?,
            dashes: fields.reals(19)?.to_vec(),
            dash_phase: fields.real(20)?,
            fill_space: fields.optional_bytes(21)?.map(|b| b.to_vec()),
            stroke_space: fields.optional_bytes(22)?.map(|b| b.to_vec()),
            fill_pattern: fields.optional_bytes(23)?.map(|b| b.to_vec()),
            blend: usize::try_from(fields.unsigned(24)?)
                .ok()
                .and_then(|at| BLENDS.get(at))
                .copied()
                .ok_or_else(|| fields.wrong("a blend mode past 11.3.5's"))?,
            stroke_pattern: fields.optional_bytes(25)?.map(|b| b.to_vec()),
            stored,
            fill_source: source(27)?,
            stroke_source: source(28)?,
        };
        let weight = state_weight(&state);
        meter.spend(weight)?;
        self.states.push((Arc::new(state), weight));
        Ok(())
    }

    /// The state a command's field names, copied for the command once the
    /// copy is known to fit the budget.
    fn state_at(
        &self,
        fields: &Fields<'_>,
        at: usize,
        meter: &Meter,
    ) -> Result<Option<Box<GraphicsState>>, CommandError> {
        let Some(index) = fields.reference(at)? else {
            return Ok(None);
        };
        let (state, weight) = usize::try_from(index)
            .ok()
            .and_then(|i| self.states.get(i))
            .ok_or_else(|| {
                fields.wrong(format!("state {index}, which no record before defines"))
            })?;
        if !meter.can_hold(*weight) {
            return Err(CommandError::OverBudget { budget: "bytes" });
        }
        Ok(Some(Box::new((**state).clone())))
    }

    /// Appends `event`, charging what it holds as a display list counts it.
    fn push(events: &mut Vec<Event>, event: Event, meter: &mut Meter) -> Result<(), CommandError> {
        meter.spend(event.kept_bytes() as u64)?;
        events.push(event);
        Ok(())
    }

    fn read_command(
        &self,
        kind: CommandKind,
        fields: &Fields<'_>,
        events: &mut Vec<Event>,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let event = match kind {
            CommandKind::GlyphRun => return self.read_run(fields, events, meter),
            CommandKind::BeginText => Event::BeginText,
            CommandKind::EndText => Event::EndText,
            CommandKind::FillPath => Event::FillPath {
                state: self.state_at(fields, 0, meter)?,
                even_odd: fields.flag(1)?,
                path: read_path(fields, 2, 3)?,
            },
            CommandKind::StrokePath => Event::StrokePath {
                state: self.state_at(fields, 0, meter)?,
                path: read_path(fields, 1, 2)?,
            },
            CommandKind::ClipPath => Event::ClipPath {
                state: self.state_at(fields, 0, meter)?,
                even_odd: fields.flag(1)?,
                path: read_path(fields, 2, 3)?,
            },
            CommandKind::SaveState => Event::SaveState,
            CommandKind::RestoreState => Event::RestoreState,
            CommandKind::DrawImage => Event::DrawImage {
                state: self.state_at(fields, 0, meter)?,
                image: ImageRef {
                    name: fields.bytes(1)?.to_vec(),
                    inline: fields.flag(2)?,
                    inline_dict: fields.bytes(3)?.to_vec(),
                    inline_data: fields.bytes(4)?.to_vec(),
                },
            },
            CommandKind::DrawShading => Event::DrawShading {
                state: self.state_at(fields, 0, meter)?,
                name: fields.bytes(1)?.to_vec(),
            },
            CommandKind::BeginMarkedContent => {
                let props = if fields.flag(3)? {
                    let mcid = fields
                        .optional_unsigned(4)?
                        .map(|mcid| {
                            u32::try_from(mcid).map_err(|_| fields.wrong("an mcid past 32 bits"))
                        })
                        .transpose()?;
                    Some(MarkedProps {
                        mcid,
                        actual_text: fields.optional_text(5)?,
                        alt: fields.optional_text(6)?,
                        lang: fields.optional_text(7)?,
                        expansion: fields.optional_text(8)?,
                        stream: fields.unsigned(9)?,
                        associated_files: fields.optional_bytes(10)?.map(|b| b.to_vec()),
                    })
                } else {
                    None
                };
                Event::BeginMarkedContent(MarkedScope {
                    tag: fields.bytes(0)?.to_vec(),
                    visible: fields.flag(1)?,
                    hidden_layer: fields.optional_text(2)?,
                    props,
                })
            }
            CommandKind::EndMarkedContent => Event::EndMarkedContent,
            CommandKind::BeginForm => Event::BeginForm {
                id: fields.unsigned(0)?,
                name: fields.bytes(1)?.to_vec(),
                entered: fields.flag(2)?,
            },
            CommandKind::EndForm => Event::EndForm {
                id: fields.unsigned(0)?,
            },
            CommandKind::BeginGroup => Event::BeginGroup {
                state: self.state_at(fields, 0, meter)?,
                group: Group {
                    isolated: fields.flag(1)?,
                    knockout: fields.flag(2)?,
                    space: group_space_at(fields, 3)?,
                },
                entered: fields.flag(4)?,
            },
            CommandKind::EndGroup => Event::EndGroup,
            CommandKind::BeginSoftMask => {
                let group = if fields.flag(4)? {
                    Some(Group {
                        isolated: fields.flag(5)?,
                        knockout: fields.flag(6)?,
                        space: group_space_at(fields, 7)?,
                    })
                } else {
                    None
                };
                let bbox = fields
                    .optional_reals(3, 4)?
                    .map(|b| [b[0], b[1], b[2], b[3]]);
                let backdrop = match fields.field(10)? {
                    Field::Nothing => None,
                    _ => Some(rgb_at(fields, 10)?),
                };
                let transfer = match fields.field(11)? {
                    Field::Nothing => None,
                    _ => {
                        let t = fields.exact_bytes(11, 256)?;
                        let mut table = [0u8; 256];
                        table.copy_from_slice(t);
                        Some(table)
                    }
                };
                Event::BeginSoftMask {
                    state: self.state_at(fields, 0, meter)?,
                    mask: Box::new(MaskGroup {
                        form: Form {
                            content: fields.bytes(1)?.to_vec(),
                            matrix: matrix_at(fields, 2)?,
                            bbox,
                            group,
                            stream: fields.unsigned(8)?,
                        },
                        luminosity: fields.flag(9)?,
                        backdrop,
                        transfer,
                    }),
                    bbox: read_path(fields, 12, 13)?,
                    entered: fields.flag(14)?,
                }
            }
            CommandKind::EndSoftMask => Event::EndSoftMask,
            CommandKind::ClearSoftMask => Event::ClearSoftMask,
            CommandKind::GroupBudgetSpent => Event::GroupBudgetSpent,
            CommandKind::RecursionRefused => Event::RecursionRefused,
            CommandKind::NestedBudgetSpent => Event::NestedBudgetSpent,
            other => return Err(fields.wrong(format!("{} is no command", other.name()))),
        };
        Self::push(events, event, meter)
    }

    fn read_run(
        &self,
        fields: &Fields<'_>,
        events: &mut Vec<Event>,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let state = self.state_at(fields, 0, meter)?;
        let font_id = fields.unsigned(1)?;
        let font_name: Option<Arc<str>> = fields.optional_text(2)?.map(Arc::from);
        let size = fields.real(3)?;
        let vertical = fields.flag(4)?;
        let codes = fields.naturals(5)?;
        let lengths = fields.naturals(6)?;
        let text = fields.bytes(7)?;
        let transforms = fields.reals(8)?;
        let advances = fields.reals(9)?;
        let lifted = fields.naturals(10)?;
        let baselines = fields.reals(11)?;
        let n = codes.len();
        if n == 0
            || n > MAX_RUN
            || lengths.len() != n
            || advances.len() != n
            || transforms.len() != n * 6
            || baselines.len() != lifted.len() * 2
        {
            return Err(fields.wrong("a glyph run whose fields disagree on its glyphs"));
        }
        let mut lifts = lifted.iter().copied().enumerate().peekable();
        let mut last: Option<u64> = None;
        let mut offset = 0usize;
        for at in 0..n {
            let code = u32::try_from(codes[at]).map_err(|_| fields.wrong("a code past 32 bits"))?;
            let len =
                usize::try_from(lengths[at]).map_err(|_| fields.wrong("a text past usize"))?;
            let end = offset
                .checked_add(len)
                .filter(|end| *end <= text.len())
                .ok_or_else(|| fields.wrong("glyph texts past the run's text"))?;
            let glyph_text = std::str::from_utf8(&text[offset..end])
                .map_err(|_| fields.wrong("a glyph's text that is not UTF-8"))?
                .to_string();
            offset = end;
            let baseline = match lifts.peek() {
                Some(&(place, index)) if index == at as u64 => {
                    lifts.next();
                    if last.is_some_and(|last| index <= last) {
                        return Err(fields.wrong("baselines out of order"));
                    }
                    last = Some(index);
                    Some((baselines[place * 2], baselines[place * 2 + 1]))
                }
                _ => None,
            };
            let m = &transforms[at * 6..at * 6 + 6];
            // Each glyph holds a copy of the run's state, checked before it
            // is made as a command's is.
            if let Some(state) = &state {
                if !meter.can_hold(state_weight(state)) {
                    return Err(CommandError::OverBudget { budget: "bytes" });
                }
            }
            let glyph_event = Event::ShowGlyph {
                glyph: Glyph {
                    code,
                    text: glyph_text,
                    transform: Matrix {
                        a: m[0],
                        b: m[1],
                        c: m[2],
                        d: m[3],
                        e: m[4],
                        f: m[5],
                    },
                    baseline,
                    advance: advances[at],
                    size,
                    vertical,
                    font_id,
                    font_name: font_name.clone(),
                    // A command stream is a drawing's, and a drawing asks
                    // nothing of its glyphs (`Device::glyph_queries`).
                    glyph_id: None,
                    extent: None,
                },
                state: state.clone(),
            };
            Self::push(events, glyph_event, meter)?;
        }
        if offset != text.len() || lifts.next().is_some() {
            return Err(fields.wrong("a glyph run with text or baselines left over"));
        }
        Ok(())
    }
}

// ---- nesting ------------------------------------------------------------------------

/// Holds one list of commands to the nesting a recording has: each form,
/// group and soft mask entered closes, inside the ones open around it, and
/// none is left open at the list's end.
#[derive(Clone, Debug, Default)]
pub struct Nesting {
    open: Vec<CommandKind>,
}

impl Nesting {
    /// Nothing open.
    #[must_use]
    pub fn new() -> Nesting {
        Nesting::default()
    }

    /// Takes `event`, refusing an end that closes nothing open, or something
    /// other than the innermost bracket.
    pub fn event(&mut self, event: &Event) -> Result<(), CommandError> {
        let (opens, closes) = match event {
            Event::BeginForm { entered: true, .. } => (Some(CommandKind::BeginForm), None),
            Event::BeginGroup { entered: true, .. } => (Some(CommandKind::BeginGroup), None),
            Event::BeginSoftMask { entered: true, .. } => (Some(CommandKind::BeginSoftMask), None),
            Event::EndForm { .. } => (None, Some((CommandKind::BeginForm, CommandKind::EndForm))),
            Event::EndGroup => (None, Some((CommandKind::BeginGroup, CommandKind::EndGroup))),
            Event::EndSoftMask => (
                None,
                Some((CommandKind::BeginSoftMask, CommandKind::EndSoftMask)),
            ),
            _ => (None, None),
        };
        if let Some(kind) = opens {
            self.open.push(kind);
        }
        if let Some((begun, end)) = closes {
            if self.open.pop() != Some(begun) {
                return Err(malformed(
                    end.number(),
                    "an end of a bracket not innermost open",
                ));
            }
        }
        Ok(())
    }

    /// Ends the list, refusing one that leaves a bracket open.
    pub fn close(&self) -> Result<(), CommandError> {
        match self.open.last() {
            None => Ok(()),
            Some(kind) => Err(malformed(kind.number(), "a bracket left open at the end")),
        }
    }
}

// ---- events alone ---------------------------------------------------------------------

/// A stream of `events` and nothing else: the definitions their states are
/// made of, then the commands.
#[must_use]
pub fn encode_events(events: &[Event]) -> Vec<u8> {
    let mut writer = EventWriter::new();
    for event in events {
        writer.event(event);
    }
    let (mut records, body) = writer.finish();
    records.extend(body);
    write_stream(&records)
}

/// The events of a stream [`encode_events`] wrote, read under `budgets`: a
/// foreign kind — the page or a table, which a stream of events alone does
/// not hold — is refused, as is a list that does not nest as a recording's.
pub fn decode_events(bytes: &[u8], budgets: &Budgets) -> Result<Vec<Event>, CommandError> {
    let mut meter = Meter::new(*budgets);
    let stream = read_stream(bytes, &mut meter)?;
    let mut reader = EventReader::new(stream.minor);
    let mut events = Vec::new();
    for record in &stream.records {
        if let Outcome::Foreign(kind) = reader.read(record, &mut events, &mut meter)? {
            return Err(malformed(
                kind.number(),
                "a page or table record in a stream of events alone",
            ));
        }
    }
    let mut nesting = Nesting::new();
    for event in &events {
        nesting.event(event)?;
    }
    nesting.close()?;
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bit of a real, compared as bits.
    fn same_bits(a: f64, b: f64) -> bool {
        a.to_bits() == b.to_bits()
    }

    /// Two states equal field for field, every real by its bits.
    fn states_alike(a: &GraphicsState, b: &GraphicsState) -> bool {
        let m = |x: &Matrix, y: &Matrix| {
            [x.a, x.b, x.c, x.d, x.e, x.f]
                .iter()
                .zip([y.a, y.b, y.c, y.d, y.e, y.f])
                .all(|(p, q)| same_bits(*p, q))
        };
        let reals = |x: &[f64], y: &[f64]| {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_bits(*p, *q))
        };
        m(&a.ctm, &b.ctm)
            && reals(
                &[
                    a.text.size,
                    a.text.char_spacing,
                    a.text.word_spacing,
                    a.text.horizontal_scale,
                    a.text.leading,
                    a.text.rise,
                ],
                &[
                    b.text.size,
                    b.text.char_spacing,
                    b.text.word_spacing,
                    b.text.horizontal_scale,
                    b.text.leading,
                    b.text.rise,
                ],
            )
            && a.text.render_mode == b.text.render_mode
            && a.text.font == b.text.font
            && same_bits(a.fill_alpha, b.fill_alpha)
            && same_bits(a.stroke_alpha, b.stroke_alpha)
            && a.fill_color == b.fill_color
            && a.stroke_color == b.stroke_color
            && a.fill_wide == b.fill_wide
            && a.stroke_wide == b.stroke_wide
            && a.fill_ink == b.fill_ink
            && a.stroke_ink == b.stroke_ink
            && a.fill_family == b.fill_family
            && a.stroke_family == b.stroke_family
            && a.fill_colorants == b.fill_colorants
            && a.stroke_colorants == b.stroke_colorants
            && same_bits(a.line_width, b.line_width)
            && a.line_cap == b.line_cap
            && a.line_join == b.line_join
            && same_bits(a.miter_limit, b.miter_limit)
            && reals(&a.dashes, &b.dashes)
            && same_bits(a.dash_phase, b.dash_phase)
            && a.fill_space == b.fill_space
            && a.stroke_space == b.stroke_space
            && a.fill_pattern == b.fill_pattern
            && a.blend == b.blend
            && a.stroke_pattern == b.stroke_pattern
            && stored_alike(a.stored.as_deref(), b.stored.as_deref())
            && sources_alike(a.fill_source.as_deref(), b.fill_source.as_deref())
            && sources_alike(a.stroke_source.as_deref(), b.stroke_source.as_deref())
    }

    fn sources_alike(a: Option<&ColorSource>, b: Option<&ColorSource>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.family == b.family
                    && a.components.len() == b.components.len()
                    && a.components
                        .iter()
                        .zip(&b.components)
                        .all(|(p, q)| same_bits(*p, *q))
            }
            _ => false,
        }
    }

    fn stored_alike(a: Option<&StoredParameters>, b: Option<&StoredParameters>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                // The functions and halftones hold no NaN in the fixtures, so
                // value equality is bit equality there.
                a.rendering_intent == b.rendering_intent
                    && a.black_point_compensation == b.black_point_compensation
                    && a.stroke_overprint == b.stroke_overprint
                    && a.fill_overprint == b.fill_overprint
                    && a.overprint_mode == b.overprint_mode
                    && a.alpha_is_shape == b.alpha_is_shape
                    && a.text_knockout == b.text_knockout
                    && a.flatness.map(f64::to_bits) == b.flatness.map(f64::to_bits)
                    && a.smoothness.map(f64::to_bits) == b.smoothness.map(f64::to_bits)
                    && a.stroke_adjustment == b.stroke_adjustment
                    && a.black_generation == b.black_generation
                    && a.undercolor_removal == b.undercolor_removal
                    && a.transfer == b.transfer
                    && a.halftone == b.halftone
                    && a.halftone_origin.map(|o| o.map(f64::to_bits))
                        == b.halftone_origin.map(|o| o.map(f64::to_bits))
            }
            _ => false,
        }
    }

    /// A NaN whose payload a canonicalising copy would lose.
    fn odd_nan() -> f64 {
        f64::from_bits(0x7ff8_dead_beef_0001)
    }

    /// A state with every field away from its initial value, the reals at
    /// edges: negative zero, the infinities, a NaN with a payload, the
    /// smallest subnormal and the largest finite.
    fn edge_state() -> GraphicsState {
        let mut state = GraphicsState::new(Matrix {
            a: -0.0,
            b: f64::INFINITY,
            c: f64::NEG_INFINITY,
            d: odd_nan(),
            e: f64::MIN_POSITIVE / 4.0,
            f: f64::MAX,
        });
        state.text = TextState {
            size: 1e-300,
            char_spacing: -0.0,
            word_spacing: f64::MIN,
            horizontal_scale: 1.5,
            leading: odd_nan(),
            rise: -7.25,
            render_mode: TextRenderMode::FillStrokeClip,
            font: Some(b"F\x00\xff".to_vec()),
        };
        state.fill_alpha = 0.5;
        state.stroke_alpha = -0.0;
        state.fill_color = Rgb {
            r: 0,
            g: 128,
            b: 255,
        };
        state.stroke_color = Rgb { r: 255, g: 1, b: 2 };
        state.fill_wide = Rgb16 {
            r: 0,
            g: 32_896,
            b: 65_535,
        };
        state.stroke_wide = Rgb16 {
            r: 65_535,
            g: 257,
            b: 514,
        };
        state.fill_ink = Some([0, 1, 254, 255]);
        state.stroke_ink = Some([255, 255, 255, 255]);
        state.fill_family = ColorFamily::Cmyk;
        state.stroke_family = ColorFamily::Other;
        state.fill_colorants = Some(Arc::new(ColorantTints {
            kind: ColorantKind::NChannel,
            tints: vec![
                ColorantTint {
                    name: Arc::from(&b"Cyan"[..]),
                    tint: 0,
                    alone: None,
                    zero: false,
                },
                ColorantTint {
                    name: Arc::from(&b"PANTONE 131"[..]),
                    tint: 200,
                    alone: Some([1, 2, 3, 4]),
                    zero: false,
                },
                ColorantTint {
                    name: Arc::from(&b"Magenta"[..]),
                    tint: 0,
                    alone: None,
                    zero: true,
                },
            ],
            alternate: Some([5, 6, 7, 8]),
            process: Some([9, 10, 11, 12]),
        }));
        state.stroke_colorants = Some(Arc::new(ColorantTints::device_cmyk([1, 2, 3, 4])));
        state.line_width = 0.0;
        state.line_cap = LineCap::Square;
        state.line_join = LineJoin::Bevel;
        state.miter_limit = f64::INFINITY;
        state.dashes = vec![3.0, -0.0, odd_nan()];
        state.dash_phase = 1e308;
        state.fill_space = Some(b"CS0".to_vec());
        state.stroke_space = Some(Vec::new());
        state.fill_pattern = Some(b"P1".to_vec());
        state.blend = BlendMode::Luminosity;
        state.stroke_pattern = Some(b"P2".to_vec());
        let curve = Arc::new(Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0],
            c1: vec![1.0],
            n: 2.2,
        });
        let stitched = Arc::new(Function::Stitching {
            domain: (0.0, 1.0),
            functions: vec![(*curve).clone(), Function::Identity],
            bounds: vec![0.5],
            encode: vec![(0.0, 1.0), (1.0, 0.0)],
        });
        let sampled = Arc::new(Function::Sampled {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0)],
            size: vec![2],
            bits: 8,
            encode: vec![(0.0, 1.0)],
            decode: vec![(0.0, 1.0)],
            samples: vec![0, 255],
        });
        let calculator = Arc::new(Function::PostScript {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0)],
            program: parse_postscript(b"{ dup 0.5 gt { 1 exch sub } if }"),
        });
        state.set_stored(|s| {
            s.rendering_intent = RenderingIntent::Unrecognised;
            s.black_point_compensation = BlackPointCompensation::On;
            s.stroke_overprint = true;
            s.fill_overprint = true;
            s.overprint_mode = i64::MIN;
            s.alpha_is_shape = true;
            s.text_knockout = false;
            s.flatness = Some(-0.0);
            s.smoothness = Some(odd_nan());
            s.stroke_adjustment = Some(true);
            s.black_generation = Some(Arc::clone(&stitched));
            s.undercolor_removal = Some(Arc::clone(&calculator));
            s.transfer = Some(Transfer::Four([
                Arc::clone(&curve),
                Arc::clone(&sampled),
                Arc::new(Function::Array(vec![Function::Identity, (*curve).clone()])),
                Arc::clone(&curve),
            ]));
            s.halftone = Some(Arc::new(Halftone::Colorants(vec![
                (
                    b"Default".to_vec(),
                    Halftone::Screen {
                        frequency: 150.0,
                        angle: 45.0,
                        spot: SpotFunction::Names(vec![b"Round".to_vec(), b"Ellipse".to_vec()]),
                        accurate: true,
                        transfer: Some(Transfer::Identity),
                    },
                ),
                (
                    b"Cyan".to_vec(),
                    Halftone::Screen {
                        frequency: 0.0,
                        angle: -0.0,
                        spot: SpotFunction::Function(Arc::clone(&curve)),
                        accurate: false,
                        transfer: None,
                    },
                ),
                (
                    b"Magenta".to_vec(),
                    Halftone::Threshold {
                        width: i64::MAX,
                        height: -1,
                        thresholds: u64::MAX,
                        transfer: Some(Transfer::One(Arc::clone(&sampled))),
                    },
                ),
                (
                    b"Yellow".to_vec(),
                    Halftone::Squares {
                        xsquare: 3,
                        ysquare: 4,
                        thresholds: 7 << 16,
                        transfer: None,
                    },
                ),
                (
                    b"Black".to_vec(),
                    Halftone::Threshold16 {
                        width: 16,
                        height: 16,
                        second: Some((i64::MIN, 0)),
                        thresholds: 1,
                        transfer: None,
                    },
                ),
                (
                    b"Spot".to_vec(),
                    Halftone::Threshold16 {
                        width: 1,
                        height: 2,
                        second: None,
                        thresholds: 2,
                        transfer: None,
                    },
                ),
            ])));
            s.halftone_origin = Some([f64::NEG_INFINITY, -0.0]);
        });
        state.fill_source = Some(Arc::new(ColorSource {
            family: Some(b"DeviceCMYK"),
            components: vec![0.0, -0.0, 1.0, odd_nan()],
        }));
        state.stroke_source = Some(Arc::new(ColorSource {
            family: None,
            components: Vec::new(),
        }));
        state
    }

    fn glyph(code: u32, text: &str, baseline: Option<(f64, f64)>, font: u64) -> Glyph {
        Glyph {
            code,
            text: text.to_string(),
            transform: Matrix {
                a: 12.0,
                b: -0.0,
                c: 0.0,
                d: 12.0,
                e: f64::from(code),
                f: odd_nan(),
            },
            baseline,
            advance: -0.5,
            size: 12.0,
            vertical: font == 2,
            font_id: font,
            font_name: (font != 0).then(|| Arc::from("ABCDEF+Liberation Serif")),
            glyph_id: None,
            extent: None,
        }
    }

    fn every_path() -> Vec<PathSegment> {
        vec![
            PathSegment::MoveTo {
                x: -0.0,
                y: f64::INFINITY,
            },
            PathSegment::LineTo {
                x: odd_nan(),
                y: 1.0,
            },
            PathSegment::CurveTo {
                x1: 1.0,
                y1: 2.0,
                x2: 3.0,
                y2: 4.0,
                x3: f64::MIN,
                y3: f64::MAX,
            },
            PathSegment::Close,
        ]
    }

    /// A recording holding every `Event` variant, and states at edges.
    fn every_event() -> Vec<Event> {
        let edge = Some(Box::new(edge_state()));
        let plain = Some(Box::new(GraphicsState::new(Matrix::IDENTITY)));
        let mut transfer = [0u8; 256];
        for (i, slot) in transfer.iter_mut().enumerate() {
            *slot = 255 - i as u8;
        }
        vec![
            Event::SaveState,
            Event::BeginText,
            Event::ShowGlyph {
                glyph: glyph(65, "A", None, 1),
                state: edge.clone(),
            },
            Event::ShowGlyph {
                glyph: glyph(66, "ﬁ", Some((1.0, -0.0)), 1),
                state: edge.clone(),
            },
            Event::ShowGlyph {
                glyph: glyph(67, "", None, 1),
                state: edge.clone(),
            },
            // Another font, the same font in another state, and no state at
            // all: four runs in all.
            Event::ShowGlyph {
                glyph: glyph(u32::MAX, "\u{10ffff}", None, 2),
                state: edge.clone(),
            },
            Event::ShowGlyph {
                glyph: glyph(0, "x", None, 2),
                state: plain.clone(),
            },
            Event::ShowGlyph {
                glyph: glyph(1, "y", Some((odd_nan(), 2.0)), 0),
                state: None,
            },
            Event::EndText,
            Event::FillPath {
                path: every_path(),
                even_odd: true,
                state: edge.clone(),
            },
            Event::StrokePath {
                path: every_path(),
                state: plain.clone(),
            },
            Event::ClipPath {
                path: Vec::new(),
                even_odd: false,
                state: None,
            },
            Event::DrawImage {
                image: ImageRef {
                    name: b"Im0".to_vec(),
                    inline: false,
                    inline_dict: Vec::new(),
                    inline_data: Vec::new(),
                },
                state: edge.clone(),
            },
            Event::DrawImage {
                image: ImageRef {
                    name: Vec::new(),
                    inline: true,
                    inline_dict: b"/W 1 /H 1 /BPC 8 /CS /G".to_vec(),
                    inline_data: vec![0, 0xff, 0x80],
                },
                state: plain.clone(),
            },
            Event::DrawShading {
                name: b"Sh0".to_vec(),
                state: edge.clone(),
            },
            Event::BeginMarkedContent(MarkedScope {
                tag: b"OC".to_vec(),
                visible: false,
                hidden_layer: Some("Construction lines".to_string()),
                props: None,
            }),
            Event::BeginMarkedContent(MarkedScope {
                tag: Vec::new(),
                visible: true,
                hidden_layer: None,
                props: Some(MarkedProps {
                    mcid: Some(u32::MAX),
                    actual_text: Some("fi".to_string()),
                    alt: Some(String::new()),
                    lang: Some("ar-EG".to_string()),
                    expansion: Some("for example".to_string()),
                    stream: (12 << 16) | 3,
                    associated_files: Some(b"MC0".to_vec()),
                }),
            }),
            Event::BeginMarkedContent(MarkedScope {
                tag: b"Span".to_vec(),
                visible: true,
                hidden_layer: None,
                props: Some(MarkedProps::default()),
            }),
            Event::EndMarkedContent,
            Event::EndMarkedContent,
            Event::EndMarkedContent,
            Event::BeginForm {
                id: u64::MAX,
                name: b"Fm0".to_vec(),
                entered: true,
            },
            Event::BeginGroup {
                group: Group {
                    isolated: true,
                    knockout: true,
                    space: Some(GroupSpace::Lab),
                },
                entered: true,
                state: edge.clone(),
            },
            Event::BeginSoftMask {
                mask: Box::new(MaskGroup {
                    form: Form {
                        content: b"0 g 0 0 1 1 re f".to_vec(),
                        matrix: Matrix {
                            a: -0.0,
                            ..Matrix::IDENTITY
                        },
                        bbox: Some([0.0, -0.0, f64::INFINITY, 1.0]),
                        group: Some(Group {
                            isolated: false,
                            knockout: true,
                            space: None,
                        }),
                        stream: 7 << 16,
                    },
                    luminosity: true,
                    backdrop: Some(Rgb { r: 1, g: 2, b: 3 }),
                    transfer: Some(transfer),
                }),
                bbox: every_path(),
                entered: true,
                state: edge.clone(),
            },
            Event::EndSoftMask,
            Event::BeginSoftMask {
                mask: Box::new(MaskGroup {
                    form: Form {
                        content: Vec::new(),
                        matrix: Matrix::IDENTITY,
                        bbox: None,
                        group: None,
                        stream: 0,
                    },
                    luminosity: false,
                    backdrop: None,
                    transfer: None,
                }),
                bbox: Vec::new(),
                entered: false,
                state: None,
            },
            Event::ClearSoftMask,
            Event::EndGroup,
            Event::BeginGroup {
                group: Group {
                    isolated: false,
                    knockout: false,
                    space: None,
                },
                entered: false,
                state: None,
            },
            Event::EndForm { id: u64::MAX },
            Event::BeginForm {
                id: 0,
                name: Vec::new(),
                entered: false,
            },
            Event::GroupBudgetSpent,
            Event::RecursionRefused,
            Event::NestedBudgetSpent,
            Event::RestoreState,
        ]
    }

    /// Two event lists alike event for event, every real by its bits.
    fn events_alike(a: &[Event], b: &[Event]) -> Result<(), String> {
        if a.len() != b.len() {
            return Err(format!("{} events and {}", a.len(), b.len()));
        }
        for (at, (x, y)) in a.iter().zip(b).enumerate() {
            if x.kind() != y.kind() {
                return Err(format!("event {at}: {:?} and {:?}", x.kind(), y.kind()));
            }
            match (x.state(), y.state()) {
                (None, None) => {}
                (Some(p), Some(q)) if states_alike(p, q) => {}
                _ => return Err(format!("event {at}: its state")),
            }
            // Past the state, the events' own fields: compared through their
            // records, which encode every real by its bits.
            let mut one = EventWriter::new();
            one.event(x);
            let mut two = EventWriter::new();
            two.event(y);
            let (_, first) = one.finish();
            let (_, second) = two.finish();
            if first != second {
                return Err(format!("event {at}: {x:?} and {y:?}"));
            }
        }
        Ok(())
    }

    /// **Every event and every state field round-trips to the bit**: a
    /// recording holding every `Event` variant and every `GraphicsState`
    /// field at edge values — negative zero, the infinities, a NaN with a
    /// payload, the extremes of each integer — reads back equal, writes back
    /// to the same bytes, and keeps its glyph runs, its states' shared
    /// definitions and their order.
    #[test]
    fn every_event_round_trips_to_the_bit() {
        let events = every_event();
        let bytes = encode_events(&events);
        let read = decode_events(&bytes, &Budgets::DEFAULT).expect("the stream reads");
        events_alike(&events, &read).unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(encode_events(&read), bytes, "written back, the same bytes");

        // Every variant is in the recording, so a variant the codec dropped
        // would fail here and not pass by absence.
        let kinds: std::collections::HashSet<_> = events.iter().map(Event::kind).collect();
        assert_eq!(kinds.len(), 22, "every Event variant is recorded");

        // The three glyph runs: one font and state, then another font, then
        // the same font in a plain state, and a glyph with no state at all.
        let mut meter = Meter::new(Budgets::DEFAULT);
        let stream = read_stream(&bytes, &mut meter).expect("it frames");
        let runs = stream
            .records
            .iter()
            .filter(|r| r.kind == CommandKind::GlyphRun.number())
            .count();
        assert_eq!(runs, 4, "glyphs gathered into runs by font and state");
        let states = stream
            .records
            .iter()
            .filter(|r| r.kind == CommandKind::State.number())
            .count();
        assert_eq!(states, 2, "each distinct state is written once");
        assert_eq!(stream.major, MAJOR);
        assert_eq!(stream.minor, MINOR);
        // Every definition precedes the first command.
        let first_command = stream
            .records
            .iter()
            .position(|r| r.command_kind().is_some_and(CommandKind::is_command))
            .expect("commands");
        assert!(stream.records[first_command..]
            .iter()
            .all(|r| r.command_kind().is_some_and(CommandKind::is_command)));
    }

    /// The edge state's NaN payloads and negative zeros are the bits read
    /// back, not merely equal values.
    #[test]
    fn a_state_s_reals_come_back_as_their_bits() {
        let state = edge_state();
        let event = Event::FillPath {
            path: Vec::new(),
            even_odd: false,
            state: Some(Box::new(state.clone())),
        };
        let read = decode_events(&encode_events(&[event]), &Budgets::DEFAULT).expect("it reads");
        let back = read[0].state().expect("a state");
        assert_eq!(back.ctm.a.to_bits(), (-0.0f64).to_bits());
        assert_eq!(back.ctm.d.to_bits(), odd_nan().to_bits());
        assert_eq!(back.text.leading.to_bits(), odd_nan().to_bits());
        assert_eq!(back.dashes[2].to_bits(), odd_nan().to_bits());
        assert_eq!(
            back.stored_parameters().smoothness.map(f64::to_bits),
            Some(odd_nan().to_bits())
        );
        assert!(states_alike(&state, back));
    }

    /// **A tint of no ink that is not zero reads back** (minor 1, RC-03.5): a
    /// fill whose colorants hold `0.001`'s cyan, the byte 0 and not zero, and
    /// a zero black reads back with each as it was written; the same
    /// colorants' record as minor 0 wrote it, with no list, reads every byte
    /// 0 as zero; and a list naming a tint of ink, or out of order, is
    /// refused by name.
    #[test]
    fn a_tint_of_no_ink_that_is_not_zero_reads_back() {
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.fill_colorants = Some(Arc::new(ColorantTints::device_cmyk_components([
            0.001, 0.5, 0.0, 0.0,
        ])));
        let fill = |state: &GraphicsState| Event::FillPath {
            path: every_path(),
            even_odd: false,
            state: Some(Box::new(state.clone())),
        };
        let events = vec![fill(&state)];
        let read = decode_events(&encode_events(&events), &Budgets::DEFAULT).expect("it reads");
        let zeros = |events: &[Event]| -> Vec<(u8, bool)> {
            match events.first() {
                Some(Event::FillPath {
                    state: Some(state), ..
                }) => state
                    .fill_colorants
                    .iter()
                    .flat_map(|c| c.tints.iter().map(|t| (t.tint, t.zero)))
                    .collect(),
                _ => Vec::new(),
            }
        };
        assert_eq!(
            zeros(&read),
            vec![(0, false), (128, false), (0, true), (0, true)],
            "cyan is no ink and not zero, yellow and black zero"
        );

        // The same stream with the appended field taken off, as minor 0
        // wrote it: every byte 0 reads as zero.
        let mut writer = EventWriter::new();
        writer.event(&events[0]);
        let (head, body) = writer.finish();
        let mut records: Vec<Record> = head;
        records.extend(body);
        let colorants = CommandKind::Colorants.number();
        let mut older = records.clone();
        for record in older.iter_mut().filter(|r| r.kind == colorants) {
            record.fields.truncate(7);
        }
        let read = decode_events(&write_stream_at(MAJOR, 0, &older), &Budgets::DEFAULT)
            .expect("minor 0 reads");
        assert_eq!(
            zeros(&read),
            vec![(0, true), (128, false), (0, true), (0, true)]
        );

        for (list, why) in [
            (vec![1u64], "a tint not zero that is no tint of no ink"),
            (vec![2, 0], "tints not zero out of order"),
            (vec![9], "a tint not zero that is no tint of no ink"),
        ] {
            let mut bad = records.clone();
            for record in bad.iter_mut().filter(|r| r.kind == colorants) {
                if let Some(field) = record.fields.get_mut(7) {
                    *field = Field::Naturals(list.clone());
                }
            }
            match decode_events(&write_stream(&bad), &Budgets::DEFAULT) {
                Err(CommandError::Malformed { what, .. }) => assert_eq!(what, why, "{list:?}"),
                other => panic!("{list:?}: {other:?}"),
            }
        }
    }

    /// **A newer minor's unknown record is skipped**, wherever it stands, and
    /// a field a newer minor appends to a known kind is ignored; the same
    /// record in a stream of this minor is refused.
    #[test]
    fn a_newer_minor_s_unknown_record_is_skipped() {
        let events = vec![
            Event::SaveState,
            Event::FillPath {
                path: every_path(),
                even_odd: false,
                state: None,
            },
            Event::RestoreState,
        ];
        let mut writer = EventWriter::new();
        for event in &events {
            writer.event(event);
        }
        let (head, mut body) = writer.finish();
        let unknown = Record {
            kind: 63,
            fields: vec![Field::Unsigned(1), Field::bytes(b"from a later minor")],
        };
        body.insert(1, unknown.clone());
        // A later minor's field appended to a known kind.
        if let Some(fill) = body
            .iter_mut()
            .find(|r| r.kind == CommandKind::FillPath.number())
        {
            fill.fields.push(Field::Real(1.5));
        }
        let mut records = head;
        records.extend(body);
        records.push(Record {
            kind: 4_000_000,
            fields: Vec::new(),
        });

        let newer = write_stream_at(MAJOR, MINOR + 1, &records);
        let read = decode_events(&newer, &Budgets::DEFAULT).expect("a newer minor reads");
        events_alike(&events, &read).unwrap_or_else(|why| panic!("{why}"));

        let same = write_stream_at(MAJOR, MINOR, &records);
        assert_eq!(
            decode_events(&same, &Budgets::DEFAULT),
            Err(CommandError::UnknownRecord { kind: 63 }),
            "an unknown kind of this minor is refused"
        );
    }

    /// **A newer major is refused by name**, and so is anything that is not a
    /// stream.
    #[test]
    fn a_newer_major_is_refused_by_name() {
        let newer = write_stream_at(MAJOR + 1, 0, &[]);
        let refused = decode_events(&newer, &Budgets::DEFAULT);
        assert_eq!(
            refused,
            Err(CommandError::NewerMajor {
                found: MAJOR + 1,
                read: MAJOR
            })
        );
        let message = refused.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("major version 2"), "{message}");
        assert_eq!(
            decode_events(&write_stream_at(0, 0, &[]), &Budgets::DEFAULT),
            Err(CommandError::NotACommandStream)
        );
        assert_eq!(
            decode_events(b"%PDF-1.7", &Budgets::DEFAULT),
            Err(CommandError::NotACommandStream)
        );
        assert_eq!(
            decode_events(b"", &Budgets::DEFAULT),
            Err(CommandError::NotACommandStream)
        );
        // An empty recording is a stream of its own.
        assert_eq!(
            decode_events(&encode_events(&[]), &Budgets::DEFAULT),
            Ok(Vec::new())
        );
    }

    /// A stream cut short anywhere, or carrying bytes past its end, is
    /// refused rather than read in part.
    #[test]
    fn a_stream_cut_short_or_running_on_is_refused() {
        let bytes = encode_events(&every_event());
        for cut in 0..bytes.len() {
            assert!(
                decode_events(&bytes[..cut], &Budgets::DEFAULT).is_err(),
                "a stream cut at {cut} of {}",
                bytes.len()
            );
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            decode_events(&longer, &Budgets::DEFAULT),
            Err(CommandError::TrailingBytes { at: bytes.len() })
        );
    }

    /// A varint has one encoding: a longer spelling of a value is refused.
    #[test]
    fn a_varint_longer_than_its_value_is_refused() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&MAJOR.to_le_bytes());
        bytes.extend_from_slice(&MINOR.to_le_bytes());
        // SaveState, its length spelt in two bytes.
        bytes.extend_from_slice(&[22, 0x80, 0x00, 0, 0]);
        assert!(matches!(
            decode_events(&bytes, &Budgets::DEFAULT),
            Err(CommandError::Malformed { .. })
        ));
        // The same with the length's one byte reads.
        let mut good = MAGIC.to_vec();
        good.extend_from_slice(&MAJOR.to_le_bytes());
        good.extend_from_slice(&MINOR.to_le_bytes());
        good.extend_from_slice(&[22, 0, 0, 0]);
        assert_eq!(
            decode_events(&good, &Budgets::DEFAULT),
            Ok(vec![Event::SaveState])
        );
    }

    /// Each budget is refused by name, and a reference ahead of its
    /// definition is malformed.
    #[test]
    fn each_budget_and_each_reference_is_held() {
        let bytes = encode_events(&every_event());
        let tight = |budgets: Budgets| decode_events(&bytes, &budgets);
        assert_eq!(
            tight(Budgets {
                records: 3,
                ..Budgets::DEFAULT
            }),
            Err(CommandError::OverBudget { budget: "records" })
        );
        assert_eq!(
            tight(Budgets {
                bytes: 64,
                ..Budgets::DEFAULT
            }),
            Err(CommandError::OverBudget { budget: "bytes" })
        );
        assert_eq!(
            tight(Budgets {
                functions: 0,
                ..Budgets::DEFAULT
            }),
            Err(CommandError::OverBudget {
                budget: "functions"
            })
        );
        assert_eq!(
            tight(Budgets {
                spaces: 0,
                ..Budgets::DEFAULT
            }),
            Err(CommandError::OverBudget { budget: "spaces" })
        );

        // A fill naming state 0 before any state is defined.
        let forward = write_stream(&[Record::new(CommandKind::FillPath)
            .with(Field::Unsigned(0))
            .with(flag(false))
            .with(Field::bytes(&[]))
            .with(Field::Reals(Vec::new()))]);
        assert!(matches!(
            decode_events(&forward, &Budgets::DEFAULT),
            Err(CommandError::Malformed { .. })
        ));

        // Functions copied into functions are charged as copies: a chain of
        // stitchings each holding the last twice doubles, and passes the
        // byte budget long before memory would.
        let mut records = vec![Record::new(CommandKind::Function).with(Field::Unsigned(6))];
        for at in 0..30u64 {
            records.push(
                Record::new(CommandKind::Function)
                    .with(Field::Unsigned(5))
                    .with(Field::Naturals(vec![at, at])),
            );
        }
        let doubling = write_stream(&records);
        // Under a byte budget of 16 MiB, so the copies the budget does allow
        // stay small; the default's gigabyte bounds them the same way.
        let refused = decode_events(
            &doubling,
            &Budgets {
                functions: 64,
                bytes: 1 << 24,
                ..Budgets::DEFAULT
            },
        );
        assert_eq!(refused, Err(CommandError::OverBudget { budget: "bytes" }));
    }

    /// A list of commands nests as a recording's: an end with nothing open,
    /// or closing another kind, and a bracket left open, are refused.
    #[test]
    fn commands_nest_as_a_recording_s() {
        let refused =
            |events: &[Event]| decode_events(&encode_events(events), &Budgets::DEFAULT).is_err();
        assert!(refused(&[Event::EndGroup]));
        assert!(refused(&[Event::BeginForm {
            id: 1,
            name: b"F".to_vec(),
            entered: true
        }]));
        assert!(refused(&[
            Event::BeginForm {
                id: 1,
                name: b"F".to_vec(),
                entered: true
            },
            Event::EndGroup,
        ]));
        assert!(!refused(&[
            Event::BeginForm {
                id: 1,
                name: b"F".to_vec(),
                entered: false
            },
            Event::RestoreState,
            Event::EndMarkedContent,
        ]));
    }

    /// The kind list is one list: each number names one kind, each kind's
    /// number reads back as it, and the codec's kinds are below 64.
    #[test]
    fn every_kind_has_one_number_and_named_fields() {
        let mut seen = std::collections::HashSet::new();
        for &kind in CommandKind::ALL {
            assert!(seen.insert(kind.number()), "{} twice", kind.number());
            assert_eq!(CommandKind::from_number(kind.number()), Some(kind));
            assert!(!kind.name().is_empty());
            if kind.is_command() {
                assert!(kind.is_codec());
            }
        }
        assert_eq!(CommandKind::from_number(38), None);
        assert_eq!(CommandKind::from_number(80), None);
        assert_eq!(CommandKind::GlyphRun.fields().len(), 12);
        assert_eq!(CommandKind::State.fields().len(), 29);
    }

    /// The xorshift a seeded campaign draws from: the same choices on every
    /// target.
    struct Seeded(u64);

    impl Seeded {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    /// **Truncations and mutations never panic**: every prefix of a stream,
    /// every byte replaced by a seeded value at every offset, and seeded runs
    /// of bytes inserted and deleted, each read under the default budgets
    /// and under tight ones, and every stream that does read is written back
    /// to bytes that read the same.
    #[test]
    fn truncations_and_mutations_never_panic() {
        let bytes = encode_events(&every_event());
        let tight = Budgets {
            records: 64,
            bytes: 1 << 12,
            depth: 1,
            functions: 2,
            spaces: 1,
        };
        let read = |candidate: &[u8]| {
            for budgets in [Budgets::DEFAULT, tight] {
                if let Ok(events) = decode_events(candidate, &budgets) {
                    let again = encode_events(&events);
                    let twice = decode_events(&again, &Budgets::DEFAULT)
                        .expect("what was read reads again");
                    assert_eq!(encode_events(&twice), again, "and writes the same bytes");
                }
            }
        };
        for cut in 0..=bytes.len() {
            read(&bytes[..cut]);
        }
        let mut seeded = Seeded(0x9e37_79b9_7f4a_7c15);
        for at in 0..bytes.len() {
            let mut mutated = bytes.clone();
            mutated[at] = (seeded.next() & 0xff) as u8;
            read(&mutated);
        }
        for _ in 0..512 {
            let mut mutated = bytes.clone();
            let at = (seeded.next() as usize) % (mutated.len() + 1);
            if seeded.next() & 1 == 0 {
                let run = (seeded.next() % 16) as usize;
                for _ in 0..run {
                    mutated.insert(at.min(mutated.len()), (seeded.next() & 0xff) as u8);
                }
            } else {
                let end = (at + (seeded.next() % 16) as usize).min(mutated.len());
                mutated.drain(at..end);
            }
            read(&mutated);
        }
    }
}
