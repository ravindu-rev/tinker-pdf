//! The COS model: the objects of 7.3 as a generator holds them, the spellings
//! 7.3 allows for each, and a file of sections written from them (7.5).
//!
//! `cos_syntax` generates files with [`generate`] and holds the reader to
//! what [`write`] wrote; a target that needs a document builds a [`File`] and
//! writes it with a [`Choices`] over no bytes, which spells every token in its
//! plainest form.
//!
//! Every spelling here is written from the clause it cites, not from the
//! reader's code: where the two agree only because both are this
//! repository's, a shared misreading passes (`docs/design/structured-fuzzing.md`,
//! *Risks*).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use tinker_pdf_cos::{NameTable, Object};

use crate::Choices;

/// A direct object (7.3).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null` (7.3.9).
    Null,
    /// `true` or `false` (7.3.2).
    Bool(bool),
    /// An integer (7.3.3).
    Int(i64),
    /// A real (7.3.3).
    Real(Real),
    /// A string's bytes and the form it is written in (7.3.4).
    String(Vec<u8>, Form),
    /// A name's bytes, never NUL (7.3.5).
    Name(Vec<u8>),
    /// An array (7.3.6).
    Array(Vec<Value>),
    /// A dictionary's entries in file order, every key once (7.3.7).
    Dict(Entries),
    /// An indirect reference `num gen R` (7.3.10).
    Ref(u32, u16),
}

/// A dictionary's entries, in the order they are written.
pub type Entries = Vec<(Vec<u8>, Value)>;

/// How a string is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// `( ... )`, 7.3.4.2.
    Literal,
    /// `< ... >`, 7.3.4.3.
    Hex,
}

/// A real number: `mantissa` divided by ten to the `places`.
///
/// Held that way so its value is one exact division — the mantissa and the
/// power are both exact in an `f64` — which is the correctly rounded value of
/// what is written, whatever leading or trailing zeros the spelling adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Real {
    /// The digits, at most [`Real::DIGITS`] of them, with the sign.
    pub mantissa: i64,
    /// How many of them follow the point, at most [`Real::PLACES`].
    pub places: u8,
}

/// The exact powers of ten a [`Real`] divides by.
const POW10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

impl Real {
    /// The most significant digits a real is written with, trailing zeros
    /// included: below 2^53, so the digits are an exact integer.
    pub const DIGITS: u32 = 15;
    /// The most places after the point.
    pub const PLACES: u8 = 10;

    /// `mantissa / 10^places`, its digits and places clamped to the bounds.
    pub fn new(mantissa: i64, places: u8) -> Real {
        let bound = 10i64.pow(Real::DIGITS) - 1;
        Real {
            mantissa: mantissa.clamp(-bound, bound),
            places: places.min(Real::PLACES),
        }
    }

    /// The value, as one rounding of the exact quotient.
    pub fn value(self) -> f64 {
        self.mantissa as f64 / POW10[usize::from(self.places)]
    }
}

impl Value {
    /// The name `/bytes`.
    pub fn name(bytes: &[u8]) -> Value {
        Value::Name(bytes.to_vec())
    }

    /// A dictionary of these entries, in this order.
    pub fn dict(entries: Vec<(&[u8], Value)>) -> Value {
        Value::Dict(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_vec(), value))
                .collect(),
        )
    }

    /// A literal string.
    pub fn literal(bytes: &[u8]) -> Value {
        Value::String(bytes.to_vec(), Form::Literal)
    }

    /// A reference to generation zero of `num`.
    pub fn reference(num: u32) -> Value {
        Value::Ref(num, 0)
    }
}

/// What may stand between the data and `endstream` (7.3.8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Nothing: 7.3.8.1's end-of-line is a "should".
    Bare,
    /// LINE FEED.
    Lf,
    /// CARRIAGE RETURN and LINE FEED.
    CrLf,
    /// CARRIAGE RETURN.
    Cr,
}

/// A filter a writer applies to a stream's data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// The data as it stands.
    None,
    /// `/FlateDecode` (7.4.4), compressed by the engine's own deflate.
    Flate,
    /// `/FlateDecode` over rows with the PNG Up predictor (7.4.4.4), as a
    /// cross-reference stream's rows usually are.
    FlateUp,
}

/// A stream (7.3.8): its dictionary, `/Length` among its entries, and the
/// bytes the file holds between `stream` and `endstream`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    /// The dictionary, `/Length` included and in its place.
    pub dict: Entries,
    /// The bytes between the keyword's end-of-line and the ending.
    pub data: Vec<u8>,
    /// The data once `/Filter` is undone, when there is one.
    pub decoded: Option<Vec<u8>>,
    /// CRLF rather than LF after `stream` (7.3.8.1 allows no CR alone).
    pub crlf: bool,
    /// What stands between the data and `endstream`.
    pub ending: Ending,
}

impl Stream {
    /// `data` behind `dict` and an honest, direct `/Length`, with an LF on
    /// either side.
    pub fn new(mut dict: Entries, data: Vec<u8>) -> Stream {
        dict.push((b"Length".to_vec(), Value::Int(data.len() as i64)));
        Stream {
            dict,
            data,
            decoded: None,
            crlf: false,
            ending: Ending::Lf,
        }
    }

    /// `decoded` compressed by the engine's own deflate, behind `dict` and
    /// an honest `/Filter /FlateDecode` and `/Length`.
    pub fn flate(mut dict: Entries, decoded: Vec<u8>) -> Stream {
        dict.push((b"Filter".to_vec(), Value::name(b"FlateDecode")));
        let mut stream = Stream::new(dict, tinker_pdf_filters::zlib_compress(&decoded));
        stream.decoded = Some(decoded);
        stream
    }

    /// The value `/Length` has in the dictionary.
    pub fn length(&self) -> Option<&Value> {
        self.dict
            .iter()
            .find(|(key, _)| key == b"Length")
            .map(|(_, value)| value)
    }
}

/// What an indirect object holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    /// A direct object.
    Value(Value),
    /// A stream.
    Stream(Stream),
}

/// An indirect object (7.3.10).
#[derive(Clone, Debug, PartialEq)]
pub struct Indirect {
    /// Its object number.
    pub num: u32,
    /// Its generation.
    pub gen: u16,
    /// What it holds.
    pub body: Body,
}

/// An object stream a cross-reference stream section packs objects into
/// (7.5.7).
#[derive(Clone, Debug, PartialEq)]
pub struct Packed {
    /// The object stream's own number.
    pub num: u32,
    /// The objects it holds, each of generation zero, in order.
    pub objects: Vec<(u32, Value)>,
    /// Whether its data is compressed.
    pub flate: bool,
}

/// How a section's cross-reference entries are written.
#[derive(Clone, Debug, PartialEq)]
pub enum Xref {
    /// A table (7.5.4) followed by `trailer`.
    Table {
        /// The two-byte end of each entry (7.5.4): 0 for SP LF, 1 for SP CR,
        /// 2 for CR LF.
        eol: u8,
    },
    /// A cross-reference stream (7.5.8), the trailer its dictionary.
    Stream {
        /// The stream's own number.
        num: u32,
        /// Bytes added to each field's narrowest width.
        wider: [u8; 3],
        /// Whether `/Index` is written where `[0 Size]` would do.
        index: bool,
        /// How its rows are compressed.
        filter: Filter,
        /// An object stream the section packs objects into.
        packed: Option<Packed>,
    },
}

/// One section of a file: the original, or an incremental update (7.5.6).
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The objects it writes at the top level, in file order.
    pub objects: Vec<Indirect>,
    /// The objects it frees: each number and the generation its free entry
    /// gives the number's next use.
    pub freed: Vec<(u32, u16)>,
    /// How its entries are written.
    pub xref: Xref,
    /// Its trailer's entries beyond `/Size`, `/Prev` and a stream's own.
    pub trailer: Entries,
}

/// A whole file.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    /// The header's version (7.5.2).
    pub version: (u8, u8),
    /// Its sections, the original first.
    pub sections: Vec<Section>,
}

/// Where a written object's bytes are.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// At the top level of the file.
    Top {
        /// Its `N G obj` header, the offset its entry names.
        header: u64,
        /// Its direct object, or a stream's dictionary.
        value: Range<u64>,
        /// Where the token after the value starts.
        next: u64,
        /// A stream's data.
        data: Option<Range<u64>>,
        /// Just past its `endobj`.
        end: u64,
    },
    /// In an object stream.
    Packed {
        /// The object stream's number.
        container: u32,
        /// The object's bytes in the object stream's decoded data.
        value: Range<usize>,
        /// Where the token after it starts there.
        next: usize,
    },
}

/// One object as the file holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// Its number.
    pub num: u32,
    /// Its generation.
    pub gen: u16,
    /// The section that wrote it.
    pub section: usize,
    /// What it holds, the writer's own object streams and cross-reference
    /// streams included.
    pub body: Body,
    /// Where its bytes are.
    pub place: Place,
}

/// Where one section's own bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sectioned {
    /// Its cross-reference section, the offset `startxref` names.
    pub xref_at: u64,
    /// Just past its `%%EOF`.
    pub eof: u64,
}

/// What [`write`] wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    /// The file.
    pub bytes: Vec<u8>,
    /// Each section, in file order.
    pub sections: Vec<Sectioned>,
    /// Every object written, in file order.
    pub objects: Vec<Placed>,
    /// Each object stream's decoded data, by number.
    pub containers: BTreeMap<u32, Vec<u8>>,
    /// Each section's `/Size`, in file order.
    pub sizes: Vec<u32>,
}

impl Written {
    /// The objects the file defines once every section is read (7.5.6): each
    /// number's newest definition, less the numbers a later section freed.
    pub fn live(&self, file: &File) -> BTreeMap<u32, &Placed> {
        let mut live: BTreeMap<u32, &Placed> = BTreeMap::new();
        for (index, section) in file.sections.iter().enumerate() {
            for placed in self.objects.iter().filter(|p| p.section == index) {
                live.insert(placed.num, placed);
            }
            for (num, _) in &section.freed {
                live.remove(num);
            }
        }
        live
    }

    /// The numbers a section freed and no later section defined again.
    pub fn freed(&self, file: &File) -> BTreeSet<u32> {
        let mut freed = BTreeSet::new();
        for (index, section) in file.sections.iter().enumerate() {
            for placed in self.objects.iter().filter(|p| p.section == index) {
                freed.remove(&placed.num);
            }
            for (num, _) in &section.freed {
                freed.insert(*num);
            }
        }
        freed
    }
}

/// 7.2.2's white space.
fn is_white(byte: u8) -> bool {
    matches!(byte, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}

/// 7.2.2's delimiters.
fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Neither: a byte two tokens of which run together.
fn is_regular(byte: u8) -> bool {
    !is_white(byte) && !is_delimiter(byte)
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// Spells tokens, each spelling chosen from the [`Choices`] it holds; over
/// no bytes every choice is the first, the plainest spelling.
pub struct Spell<'c, 'a> {
    choices: &'c mut Choices<'a>,
}

impl<'c, 'a> Spell<'c, 'a> {
    /// A speller choosing from `choices`.
    pub fn new(choices: &'c mut Choices<'a>) -> Spell<'c, 'a> {
        Spell { choices }
    }

    /// An end-of-line marker (7.2.3): LF, CRLF or CR.
    fn eol(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(match self.choices.below(3) {
            0 => b"\n".as_slice(),
            1 => b"\r\n",
            _ => b"\r",
        });
    }

    /// What separates two tokens: none where `needed` is false and the first
    /// choice is taken, else white space of each kind 7.2.3 names, or a
    /// comment, which is one white-space character (7.2.4).
    fn gap(&mut self, out: &mut Vec<u8>, needed: bool) {
        match self.choices.below(9) {
            0 if !needed => {}
            0 | 1 => out.push(b' '),
            2 => out.push(b'\n'),
            3 => out.extend_from_slice(b"\r\n"),
            4 => out.push(b'\t'),
            5 => out.push(0x0c),
            6 => out.push(0x00),
            7 => out.extend_from_slice(b"  \n "),
            _ => {
                out.push(b'%');
                for byte in self.choices.bytes(8) {
                    out.push(if byte == b'\r' || byte == b'\n' {
                        b'x'
                    } else {
                        byte
                    });
                }
                self.eol(out);
            }
        }
    }

    /// Writes `token` after whatever separation the bytes before it need:
    /// two regular characters side by side would read as one token, and so
    /// would the empty name `/` and a regular character after it (7.3.5).
    pub fn token(&mut self, out: &mut Vec<u8>, token: &[u8], lead: bool) {
        if !lead {
            let needed = match (out.last(), token.first()) {
                (Some(&before), Some(&first)) => {
                    (is_regular(before) || before == b'/') && is_regular(first)
                }
                _ => false,
            };
            self.gap(out, needed);
        }
        out.extend_from_slice(token);
    }

    /// An integer (7.3.3): a sign or none before a non-negative one, and
    /// leading zeros.
    fn int(&mut self, value: i64) -> Vec<u8> {
        let mut text = Vec::new();
        if value < 0 {
            text.push(b'-');
        } else if self.choices.below(4) == 1 {
            text.push(b'+');
        }
        for _ in 0..self.choices.below(4).saturating_sub(1) {
            text.push(b'0');
        }
        text.extend_from_slice(value.unsigned_abs().to_string().as_bytes());
        text
    }

    /// A real (7.3.3): a sign, leading zeros, a leading, trailing or embedded
    /// point, and trailing zeros that keep it inside the lexer's exact digits.
    fn real(&mut self, real: Real) -> Vec<u8> {
        let mut text = Vec::new();
        if real.mantissa < 0 {
            text.push(b'-');
        } else if self.choices.below(4) == 1 {
            text.push(b'+');
        }
        let digits = real.mantissa.unsigned_abs().to_string();
        let places = usize::from(real.places);
        let padded = if digits.len() <= places {
            format!("{}{digits}", "0".repeat(places + 1 - digits.len()))
        } else {
            digits
        };
        let (whole, fraction) = padded.split_at(padded.len() - places);
        // `.5` rather than `0.5`, where the whole part is zero.
        if whole == "0" && !fraction.is_empty() && self.choices.below(3) == 1 {
            // Nothing before the point.
        } else {
            for _ in 0..self.choices.below(3).saturating_sub(1) {
                text.push(b'0');
            }
            text.extend_from_slice(whole.as_bytes());
        }
        text.push(b'.');
        text.extend_from_slice(fraction.as_bytes());
        // Trailing zeros, within DIGITS significant digits and 22 places,
        // the bound of the exact path.
        let significant = real.mantissa.unsigned_abs().to_string().len() as u32;
        let room = (Real::DIGITS.saturating_sub(significant))
            .min(22 - u32::from(real.places))
            .min(3) as usize;
        for _ in 0..self.choices.below(room + 1) {
            text.push(b'0');
        }
        text
    }

    /// A name (7.3.5): `#xx` for any byte, required for those that are not
    /// regular characters and for `#` itself, either case of hex digit.
    fn name(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut text = vec![b'/'];
        for &byte in bytes {
            let plain = is_regular(byte) && byte != b'#';
            if plain && self.choices.below(4) != 3 {
                text.push(byte);
            } else {
                let digits = if self.choices.flag() {
                    HEX_LOWER
                } else {
                    HEX_UPPER
                };
                text.push(b'#');
                text.push(digits[usize::from(byte >> 4)]);
                text.push(digits[usize::from(byte & 15)]);
            }
        }
        text
    }

    /// A literal string (7.3.4.2), each byte spelled as one of the forms the
    /// clause gives it, and balanced parentheses left unescaped.
    fn literal(&mut self, bytes: &[u8]) -> Vec<u8> {
        // Which parentheses pair, so a pair can be written bare.
        let mut bare = vec![false; bytes.len()];
        let mut open: Vec<usize> = Vec::new();
        for (at, &byte) in bytes.iter().enumerate() {
            if byte == b'(' {
                open.push(at);
            } else if byte == b')' {
                if let Some(start) = open.pop() {
                    if self.choices.below(4) != 3 {
                        bare[start] = true;
                        bare[at] = true;
                    }
                }
            }
        }

        let mut text = vec![b'('];
        // What the bytes just written forbid the next spelling to begin
        // with: a bare CR swallows an LF after it into one end-of-line, and a
        // short octal escape swallows an octal digit after it.
        let mut after_cr = false;
        let mut after_short_octal = false;
        for (at, &byte) in bytes.iter().enumerate() {
            // A line continuation: a reverse solidus and an end-of-line,
            // both dropped.
            if self.choices.below(8) == 7 {
                match self.choices.below(3) {
                    0 => text.extend_from_slice(b"\\\n"),
                    1 => text.extend_from_slice(b"\\\r\n"),
                    _ => {
                        if after_cr {
                            text.extend_from_slice(b"\\\n");
                        } else {
                            text.extend_from_slice(b"\\\r");
                        }
                    }
                }
                after_cr = text.last() == Some(&b'\r');
                after_short_octal = false;
            }
            // A parenthesis of a bare pair is written bare, or the pair would
            // no longer balance; every other one is escaped.
            if bare[at] {
                text.push(byte);
                after_cr = false;
                after_short_octal = false;
                continue;
            }
            let mut forms: Vec<Vec<u8>> = Vec::with_capacity(6);
            let raw_ok = match byte {
                b'\\' | b'\r' | b'(' | b')' => false,
                b'\n' => !after_cr,
                b'0'..=b'7' => !after_short_octal,
                _ => true,
            };
            if raw_ok {
                forms.push(vec![byte]);
            }
            let named: Option<&[u8]> = match byte {
                b'\n' => Some(b"\\n"),
                b'\r' => Some(b"\\r"),
                b'\t' => Some(b"\\t"),
                0x08 => Some(b"\\b"),
                0x0c => Some(b"\\f"),
                b'(' => Some(b"\\("),
                b')' => Some(b"\\)"),
                b'\\' => Some(b"\\\\"),
                _ => None,
            };
            if let Some(named) = named {
                forms.push(named.to_vec());
            }
            // Three octal digits always; fewer only for a byte that fits.
            forms.push(format!("\\{byte:03o}").into_bytes());
            forms.push(format!("\\{:03o}", u32::from(byte) + 256).into_bytes());
            if byte < 64 {
                forms.push(format!("\\{byte:o}").into_bytes());
            }
            // A reverse solidus before a byte Table 3 does not name is
            // ignored.
            let ignorable = !matches!(
                byte,
                b'n' | b'r' | b't' | b'b' | b'f' | b'(' | b')' | b'\\' | b'0'
                    ..=b'7' | b'\r' | b'\n'
            );
            if ignorable {
                forms.push(vec![b'\\', byte]);
            }
            // A bare LF may also be spelled CR LF or CR, which read as LF.
            if byte == b'\n' && !after_cr {
                forms.push(b"\r\n".to_vec());
                forms.push(b"\r".to_vec());
            }
            let form = forms.swap_remove(self.choices.below(forms.len()));
            after_cr = form == b"\r";
            after_short_octal = form.len() < 4
                && form.first() == Some(&b'\\')
                && form[1..].iter().all(u8::is_ascii_digit);
            text.extend_from_slice(&form);
        }
        text.push(b')');
        text
    }

    /// A hexadecimal string (7.3.4.3): either case, white space between the
    /// digits, and the final 0 left off where it is one.
    fn hex(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut text = vec![b'<'];
        let digits = if self.choices.flag() {
            HEX_LOWER
        } else {
            HEX_UPPER
        };
        for (at, &byte) in bytes.iter().enumerate() {
            for (half, nibble) in [(0, byte >> 4), (1, byte & 15)] {
                let last = at + 1 == bytes.len() && half == 1;
                if last && nibble == 0 && self.choices.below(3) == 2 {
                    continue;
                }
                match self.choices.below(10) {
                    7 => text.push(b' '),
                    8 => text.push(b'\n'),
                    9 => text.push(0x00),
                    _ => {}
                }
                text.push(digits[usize::from(nibble)]);
            }
        }
        if self.choices.below(4) == 3 {
            text.push(b'\r');
        }
        text.push(b'>');
        text
    }

    /// A direct object. `lead` says whether it is the first thing written
    /// here, so nothing separates it from what the caller wrote before.
    pub fn value(&mut self, out: &mut Vec<u8>, value: &Value, lead: bool) {
        match value {
            Value::Null => self.token(out, b"null", lead),
            Value::Bool(true) => self.token(out, b"true", lead),
            Value::Bool(false) => self.token(out, b"false", lead),
            Value::Int(int) => {
                let text = self.int(*int);
                self.token(out, &text, lead);
            }
            Value::Real(real) => {
                let text = self.real(*real);
                self.token(out, &text, lead);
            }
            Value::String(bytes, Form::Literal) => {
                let text = self.literal(bytes);
                self.token(out, &text, lead);
            }
            Value::String(bytes, Form::Hex) => {
                let text = self.hex(bytes);
                self.token(out, &text, lead);
            }
            Value::Name(bytes) => {
                let text = self.name(bytes);
                self.token(out, &text, lead);
            }
            Value::Array(items) => {
                self.token(out, b"[", lead);
                for item in items {
                    self.value(out, item, false);
                }
                self.token(out, b"]", false);
            }
            Value::Dict(entries) => {
                self.token(out, b"<<", lead);
                for (key, item) in entries {
                    let text = self.name(key);
                    self.token(out, &text, false);
                    self.value(out, item, false);
                }
                self.token(out, b">>", false);
            }
            Value::Ref(num, gen) => {
                let num = self.int(i64::from(*num));
                self.token(out, &num, lead);
                let gen = self.int(i64::from(*gen));
                self.token(out, &gen, false);
                self.token(out, b"R", false);
            }
        }
    }
}

/// The narrowest big-endian width that holds `value`.
fn width(value: u64) -> u8 {
    let mut bytes = 0u8;
    let mut rest = value;
    while rest > 0 {
        bytes += 1;
        rest >>= 8;
    }
    bytes
}

/// One cross-reference row.
#[derive(Clone, Copy, Debug)]
enum Row {
    Free(u16),
    At(u64, u16),
    In(u32, u32),
}

/// Writes `file`, every spelling chosen from `choices`.
pub fn write(file: &File, choices: &mut Choices<'_>) -> Written {
    let mut spell = Spell::new(choices);
    let mut out = format!("%PDF-{}.{}", file.version.0, file.version.1).into_bytes();
    spell.eol(&mut out);
    // 7.5.2: a comment of four bytes above 127, so a transfer treats the file
    // as binary.
    out.push(b'%');
    for _ in 0..4 + spell.choices.below(4) {
        out.push(0x80 | spell.choices.byte());
    }
    spell.eol(&mut out);

    let mut written = Written {
        bytes: Vec::new(),
        sections: Vec::new(),
        objects: Vec::new(),
        containers: BTreeMap::new(),
        sizes: Vec::new(),
    };
    let mut highest = 0u32;
    let mut prev: Option<u64> = None;
    for (index, section) in file.sections.iter().enumerate() {
        let mut rows: BTreeMap<u32, Row> = BTreeMap::new();
        if index == 0 {
            rows.insert(0, Row::Free(u16::MAX));
        }
        for object in &section.objects {
            let placed = indirect(&mut spell, &mut out, object, index);
            if let Place::Top { header, .. } = placed.place {
                rows.insert(object.num, Row::At(header, object.gen));
            }
            highest = highest.max(object.num);
            written.objects.push(placed);
        }
        for (num, gen) in &section.freed {
            rows.insert(*num, Row::Free(*gen));
            highest = highest.max(*num);
        }

        let xref_at;
        match &section.xref {
            Xref::Table { eol } => {
                let size = highest + 1;
                xref_at = out.len() as u64;
                out.extend_from_slice(b"xref");
                spell.eol(&mut out);
                for (first, count) in runs(&rows) {
                    out.extend_from_slice(format!("{first} {count}").as_bytes());
                    spell.eol(&mut out);
                    for num in first..first + count {
                        let (field, gen, kind) = match rows[&num] {
                            Row::Free(gen) => (0, gen, b'f'),
                            Row::At(offset, gen) => (offset, gen, b'n'),
                            // A table has no third type; the generator packs
                            // only in stream sections.
                            Row::In(..) => (0, 0, b'f'),
                        };
                        out.extend_from_slice(format!("{field:010} {gen:05} ").as_bytes());
                        out.push(kind);
                        out.extend_from_slice(match eol % 3 {
                            0 => b" \n".as_slice(),
                            1 => b" \r",
                            _ => b"\r\n",
                        });
                    }
                }
                let trailer = trailer_entries(section, size, prev);
                out.extend_from_slice(b"trailer");
                spell.eol(&mut out);
                spell.value(&mut out, &Value::Dict(trailer), true);
                spell.eol(&mut out);
                written.sizes.push(size);
            }
            Xref::Stream {
                num,
                wider,
                index: explicit,
                filter,
                packed,
            } => {
                if let Some(packed) = packed {
                    let (placed, members, data) = container(&mut spell, &mut out, packed, index);
                    if let Place::Top { header, .. } = placed.place {
                        rows.insert(packed.num, Row::At(header, 0));
                    }
                    highest = highest.max(packed.num);
                    written.objects.push(placed);
                    for (member_index, placed) in members.into_iter().enumerate() {
                        rows.insert(placed.num, Row::In(packed.num, member_index as u32));
                        highest = highest.max(placed.num);
                        written.objects.push(placed);
                    }
                    written.containers.insert(packed.num, data);
                }
                highest = highest.max(*num);
                let size = highest + 1;
                xref_at = out.len() as u64;
                rows.insert(*num, Row::At(xref_at, 0));

                let all_offsets = rows.values().all(|row| matches!(row, Row::At(..)));
                let mut second = 0u64;
                let mut third = 0u64;
                for row in rows.values() {
                    let (field, last) = match *row {
                        Row::Free(gen) => (0, u64::from(gen)),
                        Row::At(offset, gen) => (offset, u64::from(gen)),
                        Row::In(container, index) => (u64::from(container), u64::from(index)),
                    };
                    second = second.max(field);
                    third = third.max(last);
                }
                // 7.5.8.2: a type field of width zero means type 1 throughout.
                let widths = [
                    if all_offsets { 0 } else { 1 } + wider[0].min(1),
                    width(second).max(1) + wider[1].min(2),
                    width(third) + wider[2].min(2),
                ];
                let mut data = Vec::new();
                for row in rows.values() {
                    let (kind, field, last) = match *row {
                        Row::Free(gen) => (0u64, 0u64, u64::from(gen)),
                        Row::At(offset, gen) => (1, offset, u64::from(gen)),
                        Row::In(container, index) => (2, u64::from(container), u64::from(index)),
                    };
                    for (value, bytes) in [(kind, widths[0]), (field, widths[1]), (last, widths[2])]
                    {
                        for shift in (0..bytes).rev() {
                            data.push(value.checked_shr(u32::from(shift) * 8).unwrap_or(0) as u8);
                        }
                    }
                }
                let columns = usize::from(widths[0] + widths[1] + widths[2]);
                let runs = runs(&rows);
                let mut dict: Entries = vec![
                    (b"Type".to_vec(), Value::name(b"XRef")),
                    (b"Size".to_vec(), Value::Int(i64::from(size))),
                    (
                        b"W".to_vec(),
                        Value::Array(widths.iter().map(|w| Value::Int(i64::from(*w))).collect()),
                    ),
                ];
                if *explicit || runs != [(0, size)] {
                    let mut index_array = Vec::new();
                    for (first, count) in &runs {
                        index_array.push(Value::Int(i64::from(*first)));
                        index_array.push(Value::Int(i64::from(*count)));
                    }
                    dict.push((b"Index".to_vec(), Value::Array(index_array)));
                }
                dict.extend(trailer_entries(section, size, prev).into_iter().skip(1));
                let (data, decoded) = match filter {
                    Filter::None => (data, None),
                    Filter::Flate => {
                        dict.push((b"Filter".to_vec(), Value::name(b"FlateDecode")));
                        (tinker_pdf_filters::zlib_compress(&data), Some(data))
                    }
                    Filter::FlateUp => {
                        dict.push((b"Filter".to_vec(), Value::name(b"FlateDecode")));
                        dict.push((
                            b"DecodeParms".to_vec(),
                            Value::dict(vec![
                                (b"Predictor".as_slice(), Value::Int(12)),
                                (b"Columns".as_slice(), Value::Int(columns as i64)),
                            ]),
                        ));
                        (
                            tinker_pdf_filters::zlib_compress(&up(&data, columns)),
                            Some(data),
                        )
                    }
                };
                dict.push((b"Length".to_vec(), Value::Int(data.len() as i64)));
                let object = Indirect {
                    num: *num,
                    gen: 0,
                    body: Body::Stream(Stream {
                        dict,
                        data,
                        decoded,
                        crlf: false,
                        ending: Ending::Lf,
                    }),
                };
                let placed = indirect(&mut spell, &mut out, &object, index);
                written.objects.push(placed);
                written.sizes.push(size);
            }
        }

        out.extend_from_slice(b"startxref");
        spell.eol(&mut out);
        out.extend_from_slice(xref_at.to_string().as_bytes());
        spell.eol(&mut out);
        out.extend_from_slice(b"%%EOF");
        let eof = out.len() as u64;
        spell.eol(&mut out);
        written.sections.push(Sectioned { xref_at, eof });
        prev = Some(xref_at);
    }
    written.bytes = out;
    written
}

/// The PNG Up predictor's rows (7.4.4.4): each row prefixed with filter type
/// 2 and stored as its difference from the row above.
fn up(data: &[u8], columns: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / columns.max(1) + 1);
    let mut above = vec![0u8; columns];
    for row in data.chunks(columns.max(1)) {
        out.push(2);
        for (at, &byte) in row.iter().enumerate() {
            out.push(byte.wrapping_sub(above[at]));
            above[at] = byte;
        }
    }
    out
}

/// The contiguous runs of numbers `rows` holds, as `(first, count)`.
fn runs(rows: &BTreeMap<u32, Row>) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for &num in rows.keys() {
        match runs.last_mut() {
            Some((first, count)) if *first + *count == num => *count += 1,
            _ => runs.push((num, 1)),
        }
    }
    runs
}

/// A trailer's entries (7.5.5): `/Size` first, then the section's own, then
/// `/Prev` back to the section before.
fn trailer_entries(section: &Section, size: u32, prev: Option<u64>) -> Entries {
    let mut entries: Entries = vec![(b"Size".to_vec(), Value::Int(i64::from(size)))];
    entries.extend(section.trailer.iter().cloned());
    if let Some(prev) = prev {
        entries.push((b"Prev".to_vec(), Value::Int(prev as i64)));
    }
    entries
}

/// Writes one indirect object at the top level.
fn indirect(
    spell: &mut Spell<'_, '_>,
    out: &mut Vec<u8>,
    object: &Indirect,
    section: usize,
) -> Placed {
    let header = out.len() as u64;
    // 7.3.10's object number is a positive integer and its generation a
    // non-negative one, written here as digits, with leading zeros or none.
    for (field, lead) in [
        (u64::from(object.num), true),
        (u64::from(object.gen), false),
    ] {
        let mut digits = vec![b'0'; spell.choices.below(3).saturating_sub(1)];
        digits.extend_from_slice(field.to_string().as_bytes());
        spell.token(out, &digits, lead);
    }
    spell.token(out, b"obj", false);
    let (value, next, data) = match &object.body {
        Body::Value(value) => {
            let start = out.len();
            spell.value(out, value, false);
            // The value's first byte, past whatever separated it from `obj`.
            let first = first_token(out, start);
            let end = out.len() as u64;
            spell.token(out, b"endobj", false);
            let next = (out.len() - b"endobj".len()) as u64;
            (first..end, next, None)
        }
        Body::Stream(stream) => {
            let start = out.len();
            spell.value(out, &Value::Dict(stream.dict.clone()), false);
            let first = first_token(out, start);
            let end = out.len() as u64;
            spell.token(out, b"stream", false);
            let next = (out.len() - b"stream".len()) as u64;
            out.extend_from_slice(if stream.crlf { b"\r\n" } else { b"\n" });
            let data_start = out.len() as u64;
            out.extend_from_slice(&stream.data);
            let data_end = out.len() as u64;
            out.extend_from_slice(match stream.ending {
                Ending::Bare => b"".as_slice(),
                Ending::Lf => b"\n",
                Ending::CrLf => b"\r\n",
                Ending::Cr => b"\r",
            });
            out.extend_from_slice(b"endstream");
            spell.token(out, b"endobj", false);
            (first..end, next, Some(data_start..data_end))
        }
    };
    let end = out.len() as u64;
    // Something must separate `endobj` from what follows it.
    spell.eol(out);
    Placed {
        num: object.num,
        gen: object.gen,
        section,
        body: object.body.clone(),
        place: Place::Top {
            header,
            value,
            next,
            data,
            end,
        },
    }
}

/// The offset of the first byte of a token written at or after `start`:
/// past white space and comments.
fn first_token(out: &[u8], start: usize) -> u64 {
    let mut at = start;
    while at < out.len() {
        match out[at] {
            byte if is_white(byte) => at += 1,
            b'%' => {
                while at < out.len() && out[at] != b'\r' && out[at] != b'\n' {
                    at += 1;
                }
            }
            _ => break,
        }
    }
    at as u64
}

/// Writes an object stream (7.5.7) and the objects it holds.
fn container(
    spell: &mut Spell<'_, '_>,
    out: &mut Vec<u8>,
    packed: &Packed,
    section: usize,
) -> (Placed, Vec<Placed>, Vec<u8>) {
    let mut bodies = Vec::new();
    let mut spans = Vec::new();
    for (_, value) in &packed.objects {
        if !bodies.is_empty() {
            // White space between two objects, so two integers cannot run
            // together.
            spell.gap(&mut bodies, true);
        }
        let start = bodies.len();
        spell.value(&mut bodies, value, true);
        spans.push(start..bodies.len());
    }
    let mut head = Vec::new();
    for ((num, _), span) in packed.objects.iter().zip(&spans) {
        if !head.is_empty() {
            head.push(b' ');
        }
        head.extend_from_slice(format!("{num} {}", span.start).as_bytes());
    }
    head.push(b'\n');
    let first = head.len();
    let mut data = head;
    data.extend_from_slice(&bodies);
    let mut members = Vec::new();
    for (at, ((num, value), span)) in packed.objects.iter().zip(&spans).enumerate() {
        let next = spans
            .get(at + 1)
            .map_or(data.len(), |following| first + following.start);
        members.push(Placed {
            num: *num,
            gen: 0,
            section,
            body: Body::Value(value.clone()),
            place: Place::Packed {
                container: packed.num,
                value: first + span.start..first + span.end,
                next,
            },
        });
    }
    let dict = vec![
        (b"Type".to_vec(), Value::name(b"ObjStm")),
        (b"N".to_vec(), Value::Int(packed.objects.len() as i64)),
        (b"First".to_vec(), Value::Int(first as i64)),
    ];
    let stream = if packed.flate {
        Stream::flate(dict, data.clone())
    } else {
        Stream::new(dict, data.clone())
    };
    let object = Indirect {
        num: packed.num,
        gen: 0,
        body: Body::Stream(stream),
    };
    (indirect(spell, out, &object, section), members, data)
}

/// Whether `read` is the object `model` describes, by the names in `names`;
/// the first difference, with where it is, when it is not.
///
/// # Errors
/// The first difference.
pub fn same(model: &Value, read: &Object, names: &NameTable) -> Result<(), String> {
    match (model, read) {
        (Value::Null, Object::Null) => Ok(()),
        (Value::Bool(a), Object::Bool(b)) if a == b => Ok(()),
        (Value::Int(a), Object::Int(b)) if a == b => Ok(()),
        (Value::Real(a), Object::Real(b)) if a.value() == *b => Ok(()),
        (Value::String(bytes, form), Object::String(string))
            if *bytes == string.bytes && (*form == Form::Hex) == string.hex =>
        {
            Ok(())
        }
        (Value::Name(bytes), Object::Name(name))
            if names.bytes(*name).as_deref() == Some(bytes.as_slice()) =>
        {
            Ok(())
        }
        (Value::Array(items), Object::Array(read)) if items.len() == read.len() => {
            for (at, (item, read)) in items.iter().zip(read).enumerate() {
                same(item, read, names).map_err(|why| format!("[{at}] {why}"))?;
            }
            Ok(())
        }
        (Value::Dict(entries), Object::Dict(dict)) => same_entries(entries, dict, names),
        (Value::Ref(num, gen), Object::Ref(r)) if r.num == *num && r.gen == *gen => Ok(()),
        _ => Err(format!("wrote {model:?}, read {read:?}")),
    }
}

/// [`same`] for a dictionary's entries, in order.
///
/// # Errors
/// The first difference.
pub fn same_entries(
    entries: &Entries,
    dict: &tinker_pdf_cos::Dict,
    names: &NameTable,
) -> Result<(), String> {
    if entries.len() != dict.len() {
        return Err(format!(
            "wrote {} entries, read {}: {entries:?} against {dict:?}",
            entries.len(),
            dict.len()
        ));
    }
    for ((key, value), (name, read)) in entries.iter().zip(dict.entries()) {
        if names.bytes(*name).as_deref() != Some(key.as_slice()) {
            return Err(format!(
                "wrote the key {:?}, read {:?}",
                String::from_utf8_lossy(key),
                names.bytes(*name)
            ));
        }
        same(value, read, names)
            .map_err(|why| format!("/{} {why}", String::from_utf8_lossy(key)))?;
    }
    Ok(())
}

// ---- the generator `cos_syntax` drives -------------------------------------

/// The most sections a file has: the original and three updates.
pub const MAX_SECTIONS: usize = 4;
/// The most objects a section defines afresh.
pub const MAX_NEW: usize = 6;
/// How deep arrays and dictionaries nest.
pub const MAX_DEPTH: u32 = 4;
/// The most items an array or a dictionary holds.
pub const MAX_ITEMS: usize = 6;
/// The longest string.
pub const MAX_STRING: usize = 24;
/// The longest name.
pub const MAX_NAME: usize = 10;
/// The longest stream data.
pub const MAX_DATA: usize = 96;

/// What `generate` knows of each number while it builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    /// A direct object at this generation.
    Value(u16),
    /// A stream at this generation.
    Stream(u16),
    /// Kept as written: the catalog, the page tree, a `/Length`, an object
    /// stream or a cross-reference stream.
    Kept,
}

/// The name bytes a generated name is drawn from: regular characters, the
/// delimiters and white space a name must escape, `#`, and bytes above 127.
const NAME_BYTES: &[u8] = b"AZaz09_.-+#()<>[]{}/% \t\n\r\x01\x7f\x80\xff";

fn name(c: &mut Choices) -> Vec<u8> {
    let len = c.below(MAX_NAME + 1);
    (0..len)
        .map(|_| {
            if c.below(4) == 3 {
                // Any byte but NUL, which 7.3.5 keeps out of names.
                1 + c.below(255) as u8
            } else {
                *c.pick(NAME_BYTES)
            }
        })
        .collect()
}

fn int(c: &mut Choices) -> i64 {
    match c.below(6) {
        0 => c.range(-128, 127),
        1 => c.range(0, 65_535),
        2 => c.range(i64::from(i32::MIN), i64::from(i32::MAX)),
        3 => *c.pick(&[i64::MIN, i64::MAX, 1 << 53, -(1 << 53), 0, -1]),
        _ => c.range(i64::MIN, i64::MAX),
    }
}

fn real(c: &mut Choices) -> Real {
    let bound = 10i64.pow(Real::DIGITS) - 1;
    let mantissa = match c.below(3) {
        0 => c.range(-9_999, 9_999),
        1 => c.range(-999_999_999, 999_999_999),
        _ => c.range(-bound, bound),
    };
    Real::new(mantissa, c.below(usize::from(Real::PLACES) + 1) as u8)
}

/// A direct object, its references drawn from the numbers below `numbers`
/// and a few past them that nothing defines.
fn value(c: &mut Choices, depth: u32, numbers: u32, gens: &BTreeMap<u32, u16>) -> Value {
    let kinds = if depth >= MAX_DEPTH { 8 } else { 10 };
    match c.below(kinds) {
        0 => Value::Null,
        1 => Value::Bool(c.flag()),
        2 => Value::Int(int(c)),
        3 => Value::Real(real(c)),
        4 => Value::String(c.bytes(MAX_STRING), Form::Literal),
        5 => Value::String(c.bytes(MAX_STRING), Form::Hex),
        6 => Value::Name(name(c)),
        7 => {
            let num = match c.below(4) {
                0 => *c.pick(&[0, u32::MAX]),
                1 => numbers.saturating_add(c.below(4) as u32),
                _ => c.below(numbers as usize + 1) as u32,
            };
            let gen = match c.below(4) {
                0 => *c.pick(&[0, u16::MAX]),
                _ => gens.get(&num).copied().unwrap_or(0),
            };
            Value::Ref(num, gen)
        }
        8 => Value::Array(
            (0..c.below(MAX_ITEMS + 1))
                .map(|_| value(c, depth + 1, numbers, gens))
                .collect(),
        ),
        _ => Value::Dict(entries(c, depth + 1, numbers, gens, &[])),
    }
}

/// A dictionary's entries, every key once (7.3.7), none of them in `kept`.
fn entries(
    c: &mut Choices,
    depth: u32,
    numbers: u32,
    gens: &BTreeMap<u32, u16>,
    kept: &[&[u8]],
) -> Entries {
    let mut out: Entries = Vec::new();
    for _ in 0..c.below(MAX_ITEMS + 1) {
        let key = name(c);
        let item = value(c, depth, numbers, gens);
        if !kept.contains(&key.as_slice()) && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, item));
        }
    }
    out
}

/// The keys of Table 5 that would change where a stream's data is or what it
/// decodes to, which a generated stream dictionary leaves to the generator.
const STREAM_KEYS: &[&[u8]] = &[
    b"Length",
    b"Filter",
    b"DecodeParms",
    b"F",
    b"FFilter",
    b"FDecodeParms",
    b"DL",
];

/// What [`generate`] made, beside the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    /// Every stream whose `/Length` lies, by number and generation.
    pub lying: BTreeSet<(u32, u16)>,
}

/// A file of one to [`MAX_SECTIONS`] sections, built from `c`.
pub fn generate(c: &mut Choices) -> (File, Generated) {
    let count = 1 + c.below(MAX_SECTIONS);
    let streams: Vec<bool> = (0..count).map(|_| c.flag()).collect();
    let version = if streams.iter().any(|s| *s) {
        *c.pick(&[(1, 5), (1, 6), (1, 7), (2, 0)])
    } else {
        *c.pick(&[(1, 4), (1, 5), (1, 6), (1, 7), (2, 0)])
    };
    // A quarter of the files may carry lying lengths; the rest are clean.
    let clean = c.below(4) != 0;
    // Table 15's identifiers, sixteen bytes each as 14.4 computes them.
    let identifier = |c: &mut Choices| -> Vec<u8> { (0..16).map(|_| c.byte()).collect() };
    let id = c.flag().then(|| identifier(c));

    let mut generated = Generated::default();
    let mut uses: BTreeMap<u32, Use> = BTreeMap::new();
    let mut freed: BTreeMap<u32, u16> = BTreeMap::new();
    let mut next = 3u32;
    let mut sections = Vec::new();

    for (index, &stream) in streams.iter().enumerate() {
        let mut objects: Vec<Indirect> = Vec::new();
        let mut gens: BTreeMap<u32, u16> = uses
            .iter()
            .map(|(num, u)| {
                (
                    *num,
                    match u {
                        Use::Value(g) | Use::Stream(g) => *g,
                        Use::Kept => 0,
                    },
                )
            })
            .collect();
        gens.extend(freed.iter().map(|(n, g)| (*n, *g)));

        let mut defined: Vec<(u32, u16)> = Vec::new();
        if index == 0 {
            objects.push(Indirect {
                num: 1,
                gen: 0,
                body: Body::Value(Value::dict(vec![
                    (b"Type", Value::name(b"Catalog")),
                    (b"Pages", Value::reference(2)),
                ])),
            });
            objects.push(Indirect {
                num: 2,
                gen: 0,
                body: Body::Value(Value::dict(vec![
                    (b"Type", Value::name(b"Pages")),
                    (b"Kids", Value::Array(Vec::new())),
                    (b"Count", Value::Int(0)),
                ])),
            });
            uses.insert(1, Use::Kept);
            uses.insert(2, Use::Kept);
        } else {
            // Redefinitions, at the generation the number has.
            let changeable: Vec<(u32, u16)> = uses
                .iter()
                .filter_map(|(num, u)| match u {
                    Use::Value(g) | Use::Stream(g) => Some((*num, *g)),
                    Use::Kept => None,
                })
                .collect();
            for _ in 0..c.below(3) {
                if changeable.is_empty() {
                    break;
                }
                let (num, gen) = *c.pick(&changeable);
                if !defined.iter().any(|(n, _)| *n == num) {
                    defined.push((num, gen));
                }
            }
            // A freed number used again, at the generation its entry gave it.
            let reusable: Vec<(u32, u16)> = freed.iter().map(|(n, g)| (*n, *g)).collect();
            if !reusable.is_empty() && c.below(3) == 2 {
                let (num, gen) = *c.pick(&reusable);
                defined.push((num, gen));
                freed.remove(&num);
            }
        }
        for _ in 0..c.below(MAX_NEW + 1) {
            defined.push((next, 0));
            next += 1;
        }

        for (num, gen) in defined {
            let is_stream = c.below(3) == 2;
            if !is_stream {
                let v = value(c, 0, next, &gens);
                uses.insert(num, Use::Value(gen));
                objects.push(Indirect {
                    num,
                    gen,
                    body: Body::Value(v),
                });
                continue;
            }
            let mut dict = entries(c, 1, next, &gens, STREAM_KEYS);
            let data = c.bytes(MAX_DATA);
            let flate = c.below(4) == 3;
            let encoded = if flate {
                tinker_pdf_filters::zlib_compress(&data)
            } else {
                data.clone()
            };
            let lies = !clean && c.flag();
            let declared = if lies {
                let delta = 1 + c.below(encoded.len() + 16) as i64;
                if c.flag() || (encoded.len() as i64) < delta {
                    encoded.len() as i64 + delta
                } else {
                    encoded.len() as i64 - delta
                }
            } else {
                encoded.len() as i64
            };
            if lies {
                generated.lying.insert((num, gen));
            }
            let length = if c.flag() {
                // 7.3.8.2: an indirect `/Length`, kept so it stays this
                // stream's length.
                let length_num = next;
                next += 1;
                uses.insert(length_num, Use::Kept);
                objects.push(Indirect {
                    num: length_num,
                    gen: 0,
                    body: Body::Value(Value::Int(declared)),
                });
                Value::Ref(length_num, 0)
            } else {
                Value::Int(declared)
            };
            let at = c.below(dict.len() + 1);
            dict.insert(at, (b"Length".to_vec(), length));
            if flate {
                let at = c.below(dict.len() + 1);
                dict.insert(at, (b"Filter".to_vec(), Value::name(b"FlateDecode")));
            }
            uses.insert(num, Use::Stream(gen));
            objects.push(Indirect {
                num,
                gen,
                body: Body::Stream(Stream {
                    dict,
                    data: encoded,
                    decoded: flate.then_some(data),
                    crlf: c.flag(),
                    ending: *c.pick(&[Ending::Lf, Ending::CrLf, Ending::Cr, Ending::Bare]),
                }),
            });
        }
        // The objects of a section in an order of the generator's choosing,
        // so a forward reference is as common as a backward one.
        if objects.len() > 1 {
            let rotate = c.below(objects.len());
            objects.rotate_left(rotate);
        }

        // Frees: a number defined before this section and not kept.
        let mut frees = Vec::new();
        if index > 0 {
            let freeable: Vec<(u32, u16)> = uses
                .iter()
                .filter_map(|(num, u)| match u {
                    Use::Value(g) | Use::Stream(g) => Some((*num, *g)),
                    Use::Kept => None,
                })
                .filter(|(num, _)| !objects.iter().any(|o| o.num == *num))
                .collect();
            for _ in 0..c.below(3) {
                if freeable.is_empty() {
                    break;
                }
                let (num, gen) = *c.pick(&freeable);
                if gen < u16::MAX - 1 && !frees.iter().any(|(n, _)| *n == num) {
                    frees.push((num, gen + 1));
                }
            }
            for (num, gen) in &frees {
                uses.remove(num);
                freed.insert(*num, *gen);
            }
        }

        let mut trailer: Entries = vec![(b"Root".to_vec(), Value::reference(1))];
        if let Some(first) = &id {
            let second = identifier(c);
            trailer.push((
                b"ID".to_vec(),
                Value::Array(vec![
                    Value::String(first.clone(), Form::Hex),
                    Value::String(second, Form::Hex),
                ]),
            ));
        }

        let xref = if stream {
            let packed = if c.flag() {
                // 7.5.7: no stream, and nothing of a generation but zero.
                let mut members = Vec::new();
                let mut kept = Vec::new();
                for object in objects {
                    match &object.body {
                        Body::Value(v) if object.gen == 0 && c.below(3) != 0 => {
                            members.push((object.num, v.clone()));
                        }
                        _ => kept.push(object),
                    }
                }
                objects = kept;
                if members.is_empty() {
                    None
                } else {
                    let num = next;
                    next += 1;
                    uses.insert(num, Use::Kept);
                    Some(Packed {
                        num,
                        objects: members,
                        flate: c.flag(),
                    })
                }
            } else {
                None
            };
            let num = next;
            next += 1;
            uses.insert(num, Use::Kept);
            Xref::Stream {
                num,
                wider: [c.below(2) as u8, c.below(3) as u8, c.below(3) as u8],
                index: c.flag(),
                filter: *c.pick(&[Filter::None, Filter::Flate, Filter::FlateUp]),
                packed,
            }
        } else {
            Xref::Table {
                eol: c.below(3) as u8,
            }
        };
        sections.push(Section {
            objects,
            freed: frees,
            xref,
            trailer,
        });
    }
    (File { version, sections }, generated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_cos::{parse_object_at, WarningSink};

    fn read(text: &[u8]) -> (Object, NameTable, usize) {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(text, 0, &names, &mut sink);
        (parsed.object, names, sink.warnings().len())
    }

    #[test]
    fn the_plainest_spellings_are_the_clauses_examples() {
        let mut none = Choices::new(&[]);
        let mut spell = Spell::new(&mut none);
        let mut out = Vec::new();
        spell.value(
            &mut out,
            &Value::Array(vec![
                Value::Int(549),
                Value::Real(Real::new(314, 2)),
                Value::Bool(false),
                Value::literal(b"Ralph"),
                Value::name(b"SomeName"),
            ]),
            true,
        );
        assert_eq!(out, b"[549 3.14 false(Ralph)/SomeName]");
    }

    #[test]
    fn every_byte_of_a_literal_string_reads_back_in_every_form() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        for choice in 0..=255u8 {
            let script = vec![choice; 4096];
            let mut choices = Choices::new(&script);
            let mut spell = Spell::new(&mut choices);
            let text = spell.literal(&bytes);
            let (object, _, warnings) = read(&text);
            assert_eq!(warnings, 0, "choice {choice}");
            assert_eq!(
                object,
                Object::String(tinker_pdf_cos::PdfString::literal(bytes.clone())),
                "choice {choice}: {:?}",
                String::from_utf8_lossy(&text)
            );
        }
    }

    #[test]
    fn a_real_reads_back_as_its_exact_quotient() {
        for (mantissa, places) in [
            (5, 10),
            (-314, 2),
            (0, 3),
            (999_999_999_999_999, 10),
            (7, 0),
        ] {
            for choice in 0..=255u8 {
                let script = vec![choice; 64];
                let mut choices = Choices::new(&script);
                let mut spell = Spell::new(&mut choices);
                let real = Real::new(mantissa, places);
                let text = spell.real(real);
                let (object, _, warnings) = read(&text);
                assert_eq!(warnings, 0, "{}", String::from_utf8_lossy(&text));
                assert_eq!(
                    object,
                    Object::Real(real.value()),
                    "{}",
                    String::from_utf8_lossy(&text)
                );
            }
        }
    }

    /// What one input makes: the file, what the generator said of it, the
    /// bytes written, and how much of the input that read.
    fn made(data: &[u8]) -> (File, Generated, Written, usize) {
        let mut choices = Choices::new(data);
        let (file, generated) = generate(&mut choices);
        let written = write(&file, &mut choices);
        (file, generated, written, choices.consumed())
    }

    /// Every kind of value a body holds, by name.
    fn kinds(value: &Value, out: &mut BTreeSet<&'static str>) {
        let kind = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Int(_) => "integer",
            Value::Real(_) => "real",
            Value::String(_, Form::Literal) => "literal",
            Value::String(_, Form::Hex) => "hex",
            Value::Name(_) => "name",
            Value::Array(items) => {
                for item in items {
                    kinds(item, out);
                }
                "array"
            }
            Value::Dict(entries) => {
                for (_, item) in entries {
                    kinds(item, out);
                }
                "dictionary"
            }
            Value::Ref(..) => "reference",
        };
        out.insert(kind);
    }

    fn stream_sections(file: &File) -> usize {
        file.sections
            .iter()
            .filter(|s| matches!(s.xref, Xref::Stream { .. }))
            .count()
    }

    /// Whether a stream whose `/Length` lies has it direct, or indirect.
    fn lying_length(file: &File, generated: &Generated, indirect: bool) -> bool {
        file.sections.iter().flat_map(|s| &s.objects).any(|o| {
            matches!(&o.body, Body::Stream(stream)
                if generated.lying.contains(&(o.num, o.gen))
                    && matches!(stream.length(), Some(Value::Ref(..))) == indirect)
        })
    }

    fn depth(value: &Value) -> u32 {
        match value {
            Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
            Value::Dict(entries) => 1 + entries.iter().map(|(_, v)| depth(v)).max().unwrap_or(0),
            _ => 0,
        }
    }

    type Feature = Box<dyn Fn(&File, &Generated, &Written) -> bool>;

    /// Rewrites `fuzz/corpus/cos_syntax/`: one seed per feature, each the
    /// first input of `first_input`'s sequence whose file has it.
    #[test]
    #[ignore = "writes fuzz/corpus/cos_syntax"]
    fn write_the_fuzz_seeds() {
        let features: Vec<(&str, Feature)> = vec![
            (
                "one-table",
                Box::new(|file: &File, generated: &Generated, written: &Written| {
                    file.sections.len() == 1
                        && stream_sections(file) == 0
                        && generated.lying.is_empty()
                        && written.objects.len() >= 5
                }),
            ),
            (
                "one-stream-section",
                Box::new(|file: &File, generated: &Generated, _: &Written| {
                    file.sections.len() == 1
                        && stream_sections(file) == 1
                        && generated.lying.is_empty()
                        && matches!(&file.sections[0].xref, Xref::Stream { packed: None, .. })
                }),
            ),
            (
                "object-stream",
                Box::new(|file: &File, _: &Generated, _: &Written| {
                    file.sections.iter().any(|s| {
                        matches!(&s.xref, Xref::Stream { packed: Some(p), .. }
                            if p.objects.len() >= 3 && !p.flate)
                    })
                }),
            ),
            (
                "object-stream-compressed",
                Box::new(|file: &File, _: &Generated, _: &Written| {
                    file.sections
                        .iter()
                        .any(|s| matches!(&s.xref, Xref::Stream { packed: Some(p), .. } if p.flate))
                }),
            ),
            (
                "rows-predicted",
                Box::new(|file: &File, _: &Generated, _: &Written| {
                    file.sections.iter().any(|s| {
                        matches!(
                            s.xref,
                            Xref::Stream {
                                filter: Filter::FlateUp,
                                ..
                            }
                        )
                    })
                }),
            ),
            (
                "four-sections",
                Box::new(|file: &File, generated: &Generated, _: &Written| {
                    file.sections.len() == 4
                        && (1..4).contains(&stream_sections(file))
                        && generated.lying.is_empty()
                }),
            ),
            (
                "freed-then-reused",
                Box::new(|_: &File, _: &Generated, written: &Written| {
                    written.objects.iter().any(|p| p.gen > 0 && p.num > 2)
                }),
            ),
            (
                "lying-direct-length",
                Box::new(|file: &File, generated: &Generated, _: &Written| {
                    lying_length(file, generated, false)
                }),
            ),
            (
                "lying-indirect-length",
                Box::new(|file: &File, generated: &Generated, _: &Written| {
                    lying_length(file, generated, true)
                }),
            ),
            (
                "every-kind-of-value",
                Box::new(|_: &File, _: &Generated, written: &Written| {
                    let mut seen = BTreeSet::new();
                    let mut flate = false;
                    for placed in &written.objects {
                        match &placed.body {
                            Body::Value(value) => kinds(value, &mut seen),
                            Body::Stream(stream) => {
                                kinds(&Value::Dict(stream.dict.clone()), &mut seen);
                                flate |= stream.decoded.is_some() && placed.num > 2;
                            }
                        }
                    }
                    flate && seen.len() == 10
                }),
            ),
            (
                "deep-nesting",
                Box::new(|_: &File, _: &Generated, written: &Written| {
                    written.objects.iter().any(|placed| match &placed.body {
                        Body::Value(value) => depth(value) >= MAX_DEPTH,
                        Body::Stream(_) => false,
                    })
                }),
            ),
        ];
        let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
        for (name, has) in &features {
            let seed = crate::first_input(
                640,
                200_000,
                |data| {
                    let (file, generated, written, _) = made(data);
                    has(&file, &generated, &written)
                },
                |data| made(data).3,
                &seeds
                    .iter()
                    .map(|(_, seed)| seed.clone())
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|| panic!("no input has {name}"));
            let (file, generated, written, _) = made(&seed);
            assert!(
                has(&file, &generated, &written),
                "{name} lost its feature when cut"
            );
            seeds.push((name, seed));
        }
        crate::write_seeds("cos_syntax", &seeds);
    }

    #[test]
    fn a_plain_file_is_the_same_file_every_time() {
        let (file, _) = generate(&mut Choices::new(b"seed bytes for one file"));
        let once = write(&file, &mut Choices::new(&[]));
        let again = write(&file, &mut Choices::new(&[]));
        assert_eq!(once, again);
        assert!(once.bytes.starts_with(b"%PDF-"));
    }
}
