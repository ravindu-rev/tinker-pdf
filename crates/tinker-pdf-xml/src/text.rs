//! Bytes to characters, and characters to the ones XML 1.0 allows.
//!
//! Three jobs, all of them decided before a single tag is looked at: which
//! encoding the bytes are in (§4.3.3 and Appendix F), whether every character
//! they decode to is one the Char production admits (§2.2), and what a
//! reference stands for (§4.6 and §4.1).
//!
//! The whole-document character sweep happens once, here, rather than in each
//! construct the parser meets. Every one of them is bound by the same
//! production, so checking in one place is both cheaper and impossible to get
//! partially right — an illegal byte inside a comment is exactly as illegal as
//! one inside an attribute value, and a reader that checked only the second
//! would pass a fixture nobody thought to write.

use std::borrow::Cow;

use crate::{Construct, Encoding, Error, Warning};

/// XML 1.0 §2.2, the `Char` production.
///
/// Not "any Unicode scalar value": `NUL` is not a character, the C0 controls
/// other than tab, newline and carriage return are not characters, and neither
/// are `U+FFFE` and `U+FFFF`. `char` already excludes the surrogates and
/// anything past `U+10FFFF`, which is why those two do not appear here — they
/// appear in [`character_reference`], where a file gets to name a number
/// directly.
pub(crate) const fn is_xml_char(c: char) -> bool {
    matches!(c,
        '\u{9}' | '\u{A}' | '\u{D}'
        | '\u{20}'..='\u{D7FF}'
        | '\u{E000}'..='\u{FFFD}'
        | '\u{10000}'..='\u{10FFFF}')
}

/// XML 1.0 §2.3, `NameStartChar`, as the fifth edition writes it.
pub(crate) const fn is_name_start(c: char) -> bool {
    matches!(c,
        ':' | 'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}')
}

/// XML 1.0 §2.3, `NameChar`.
///
/// `.` is one of these, which is not a detail: `FixedPage.Resources` and
/// `LinearGradientBrush.GradientStops` are element names in every real XPS
/// package, and a scanner that stopped at a dot would read the first half of
/// each of them.
pub(crate) const fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c,
            '-' | '.' | '0'..='9' | '\u{B7}'
            | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// XML 1.0 §2.3, `S`.
pub(crate) const fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// What the first bytes say the encoding is, and the text they decode to.
///
/// §4.3.3 requires a UTF-16 entity to carry a byte order mark and lets a UTF-8
/// one carry the signature form. Milestone 1 measured what producers actually
/// do: WPF writes the UTF-8 signature on **every** part and Microsoft's XPS
/// object model writes **none**, so detection has to mean detection rather than
/// expectation — a reader requiring a mark refuses every OpenXPS file Windows
/// writes, and one requiring its absence refuses every XPS file it writes.
///
/// The unmarked UTF-16 case is a leniency and says so: Appendix F's `3C 00` /
/// `00 3C` shape is recognised and warned about, because §4.3.3 makes the mark
/// mandatory there and a file without one is a file this reader is guessing at.
pub(crate) fn decode(bytes: &[u8]) -> Result<(Cow<'_, str>, Encoding, Option<Warning>), Error> {
    // Before UTF-16: a UTF-32 mark begins with the same two bytes as a UTF-16
    // one in the little-endian case, so it has to be tested first or it decodes
    // as UTF-16 text full of NULs and is refused for the wrong reason.
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
    {
        return Err(Error::UnsupportedEncoding);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((utf8(rest)?, Encoding::Utf8, None));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok((
            Cow::Owned(utf16(rest, false)?),
            Encoding::Utf16LittleEndian,
            None,
        ));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok((
            Cow::Owned(utf16(rest, true)?),
            Encoding::Utf16BigEndian,
            None,
        ));
    }
    match bytes.get(..2) {
        Some([0x3C, 0x00]) => Ok((
            Cow::Owned(utf16(bytes, false)?),
            Encoding::Utf16LittleEndian,
            Some(Warning::Utf16WithoutByteOrderMark),
        )),
        Some([0x00, 0x3C]) => Ok((
            Cow::Owned(utf16(bytes, true)?),
            Encoding::Utf16BigEndian,
            Some(Warning::Utf16WithoutByteOrderMark),
        )),
        // §4.3.3: in the absence of any of the above, UTF-8.
        _ => Ok((utf8(bytes)?, Encoding::Utf8, None)),
    }
}

fn utf8(bytes: &[u8]) -> Result<Cow<'_, str>, Error> {
    std::str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .map_err(|_| Error::NotUtf8)
}

fn utf16(bytes: &[u8], big_endian: bool) -> Result<String, Error> {
    if bytes.len() % 2 != 0 {
        return Err(Error::NotUtf16);
    }
    let mut out = String::with_capacity(bytes.len() / 2);
    let mut at = 0;
    while at + 1 < bytes.len() {
        let unit = unit_at(bytes, at, big_endian).ok_or(Error::NotUtf16)?;
        at += 2;
        let scalar = match unit {
            0xD800..=0xDBFF => {
                let low = unit_at(bytes, at, big_endian).ok_or(Error::NotUtf16)?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(Error::NotUtf16);
                }
                at += 2;
                0x1_0000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00)
            }
            // A low surrogate with no high one in front of it. Nothing here
            // repairs it: a lone surrogate is not a character and pretending it
            // is U+FFFD would put a character in the document that the file
            // does not contain.
            0xDC00..=0xDFFF => return Err(Error::NotUtf16),
            other => u32::from(other),
        };
        out.push(char::from_u32(scalar).ok_or(Error::NotUtf16)?);
    }
    Ok(out)
}

fn unit_at(bytes: &[u8], at: usize, big_endian: bool) -> Option<u16> {
    let pair = [*bytes.get(at)?, *bytes.get(at + 1)?];
    Some(if big_endian {
        u16::from_be_bytes(pair)
    } else {
        u16::from_le_bytes(pair)
    })
}

/// The first character of `text` that §2.2 does not admit, and where it is.
pub(crate) fn illegal_character(text: &str) -> Option<usize> {
    text.char_indices()
        .find(|(_, c)| !is_xml_char(*c))
        .map(|(at, _)| at)
}

/// What a run of character data means: references resolved, line ends
/// normalised, and — for an attribute value — §3.3.3's whitespace mapping
/// applied.
///
/// The two callers differ in exactly one way and it is worth stating rather
/// than inferring from the flag. §2.11 turns `\r\n` and a lone `\r` into `\n`
/// everywhere, and §3.3.3 then turns every *literal* tab, newline and carriage
/// return in an attribute value into a space. A character **reference** to one
/// of them survives both, which is the whole reason `&#xA;` exists — so the
/// mapping is applied to what the source held, never to what a reference
/// produced.
///
/// Nothing here can expand: every reference is at least four source bytes and
/// yields exactly one character, so the result is never longer than the input.
/// That is the structural half of this crate's answer to entity expansion; the
/// other half is that `<!DOCTYPE` never gets parsed at all.
pub(crate) fn value(raw: &str, attribute: bool) -> Result<Cow<'_, str>, Error> {
    let plain = !raw.as_bytes().iter().any(|b| match b {
        b'&' | b'\r' => true,
        b'\n' | b'\t' => attribute,
        _ => false,
    });
    if plain {
        return Ok(Cow::Borrowed(raw));
    }

    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(c) = rest.chars().next() {
        match c {
            '&' => {
                let (produced, used) = reference(rest)?;
                out.push(produced);
                rest = rest.get(used..).unwrap_or("");
            }
            // §2.11 first, for both callers: the two-character sequence is one
            // line end, and a reader that mapped each byte separately would
            // turn one newline in an attribute value into two spaces.
            '\r' => {
                let after_cr = rest.get(1..).unwrap_or("");
                let two = after_cr.starts_with('\n');
                out.push(if attribute { ' ' } else { '\n' });
                rest = if two {
                    after_cr.get(1..).unwrap_or("")
                } else {
                    after_cr
                };
            }
            '\n' | '\t' if attribute => {
                out.push(' ');
                rest = rest.get(1..).unwrap_or("");
            }
            other => {
                out.push(other);
                rest = rest.get(other.len_utf8()..).unwrap_or("");
            }
        }
    }
    Ok(Cow::Owned(out))
}

/// §2.11 alone, for the three constructs that hold literal text and no
/// references at all: a CDATA section, a comment and a processing instruction.
///
/// Running [`value`] over any of them would be wrong rather than merely
/// wasteful — `&` inside a CDATA section is an ampersand, which is the entire
/// reason the construct exists.
pub(crate) fn line_ends(raw: &str) -> Cow<'_, str> {
    if !raw.contains('\r') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(c) = rest.chars().next() {
        if c == '\r' {
            let after = rest.get(1..).unwrap_or("");
            out.push('\n');
            rest = if after.starts_with('\n') {
                after.get(1..).unwrap_or("")
            } else {
                after
            };
        } else {
            out.push(c);
            rest = rest.get(c.len_utf8()..).unwrap_or("");
        }
    }
    Cow::Owned(out)
}

/// One reference at the head of `rest`, and how many bytes it took.
///
/// The five predefined entities are the only names admitted. There is no table
/// to look a sixth up in, because building one would mean having parsed a
/// document type declaration, which this crate refuses before it reads a byte
/// past `<!DOCTYPE` (ECMA-388 9.3.2 [M2.71]). So `&nbsp;` is
/// [`Error::UnknownEntity`] — refused rather than guessed at, and refused by a
/// name that says the entity was never declared rather than one that says the
/// markup is broken.
fn reference(rest: &str) -> Result<(char, usize), Error> {
    let body = rest.get(1..).unwrap_or("");
    let Some(end) = body.find(';') else {
        // An unterminated reference and a stray ampersand are the same input.
        return Err(Error::Unterminated(Construct::Reference));
    };
    let name = body.get(..end).unwrap_or("");
    let used = 1 + end + 1;
    let produced = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "apos" => '\'',
        "quot" => '"',
        numeric if numeric.starts_with('#') => character_reference(numeric)?,
        _ => return Err(Error::UnknownEntity),
    };
    Ok((produced, used))
}

/// `&#38;` and `&#x26;`, in both radixes, refusing what §4.1 does not allow.
///
/// Three refusals rather than one, and the reason each is here is that a
/// character reference is the one place a file names a code point *directly*
/// instead of encoding one. `char::from_u32` catches the surrogates and
/// anything past `U+10FFFF`; it does **not** catch `&#0;` or `&#xFFFE;`, which
/// are perfectly good scalar values and are not characters, so §2.2 is applied
/// again here after the conversion.
fn character_reference(body: &str) -> Result<char, Error> {
    let (digits, radix) = match body.get(1..2) {
        Some("x") | Some("X") => (body.get(2..).unwrap_or(""), 16u32),
        _ => (body.get(1..).unwrap_or(""), 10u32),
    };
    if digits.is_empty() {
        return Err(Error::BadCharacterReference);
    }
    let mut scalar: u32 = 0;
    for c in digits.chars() {
        let digit = c.to_digit(radix).ok_or(Error::BadCharacterReference)?;
        // Checked, and not because overflow would be wrong twice: `&#` and
        // three hundred digits is eight bytes of markup, and a wrapping
        // multiply would land on a legal code point roughly one time in a
        // million tries.
        scalar = scalar
            .checked_mul(radix)
            .and_then(|s| s.checked_add(digit))
            .ok_or(Error::BadCharacterReference)?;
    }
    let produced = char::from_u32(scalar).ok_or(Error::BadCharacterReference)?;
    if !is_xml_char(produced) {
        return Err(Error::BadCharacterReference);
    }
    Ok(produced)
}
