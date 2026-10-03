//! The font group: ISO 14289-1 7.21 and ISO 14289-2 8.4.5.
//!
//! # One rule, two standards
//!
//! Most of what ISO 14289 asks of a font, ISO 19005 asks too, and the PDF/A
//! font group in `crate::pdfa::fonts` already answers it over the fonts a
//! page draws with at a visible rendering mode. So this group **runs that
//! group** — milestone 2 of `docs/design/pdfua.md` — and re-numbers what it
//! finds by the PDF/UA clause that asks the same question, kind by kind, in
//! [`ua_clause_of`]. A kind with no PDF/UA clause is dropped rather than
//! reported under the nearest number: a subset tag's spelling, a program
//! under the wrong `/FontFile` key, a program this build's parser refuses,
//! and part 1's prohibition on `/Differences` are ISO 19005's sentences and
//! none of veraPDF's published PDF/UA rules states them.
//!
//! The PDF/A group is run under a part 2, level U flavour, which is the
//! reading the two standards share: the Unicode rule runs (it is level A and
//! U's in ISO 19005, and every PDF/UA file's in ISO 14289), and part 1's
//! `/Differences` prohibition does not. Its four `/ToUnicode` exemptions are
//! a superset of ISO 14289-1 7.21.7's four — it also admits a font naming
//! `StandardEncoding` and two more character collections — so a font it
//! reports is one 7.21.7 reports too; the converse is staged.
//!
//! # What is new here
//!
//! One rule no ISO 19005 group runs: the values a drawn code's `/ToUnicode`
//! maps to. The encoding-CMap rules (embedded unless predefined, `/WMode`
//! agreeing with the program, no reference outside Table 118, the collection
//! the CIDFont's) arrived here first and are the PDF/A group's now, under
//! the reading ISO 19005-2 and ISO 14289 share
//! (`crate::pdfa::fonts::CMapReading::TABLE_118`).
//!
//! # Which clause numbers
//!
//! veraPDF's published rules (`PDFUA-1.xml`, `PDFUA-2.xml`, read as data at
//! `veraPDF-validation-profiles` `070d39f`, and the wiki's statements of the
//! same rules at `109b482`).

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_content::Token;
use tinker_pdf_cos::{CosDocument, ObjRef, Object};

use super::{clauses, UaClauses, UaPart, UaRaw};
use crate::pdfa::FindingKind;
use crate::pdfa::{content, Flavour, Level, Machinery, Part, RuleGroup};

/// How many bytes of drawn strings the `/ToUnicode` value check holds
/// before it splits them into codes and lets them go, each string charged
/// its length and [`STRING_OVERHEAD`].
///
/// The rule is about codes, and a document with a million `Tj`s draws the
/// same few hundred codes over and over. Keeping the strings until the walk
/// ended made the check's memory the size of the text **once per font that
/// drew it**: one shared content stream holding a 512 KiB string, drawn on
/// 512 pages each mapping `/F1` to a font of its own, held 256 MiB from a
/// 3.2 MB file. Now the strings are a buffer and the codes are what is kept.
const MAX_PENDING_BYTES: usize = 1 << 20;

/// What one held string costs beyond its bytes: a set entry and a vector's
/// header, so that a million empty strings are not free.
const STRING_OVERHEAD: usize = 32;

/// How many fonts the `/ToUnicode` value check reads.
const MAX_FONTS: usize = 1 << 12;

/// How many codes of three or four bytes one font contributes. A code below
/// 2^16 — every simple font's, and a two-byte composite font's — is one bit
/// of a set that holds all 65 536 of them exactly, so only a font whose CMap
/// has wider codespaces is ever cut short, and only past this many.
const MAX_WIDE_CODES_PER_FONT: usize = 1 << 10;

/// How many `/ToUnicode` findings one font contributes.
const MAX_VALUE_FINDINGS: usize = 8;

/// The PDF/A reading the PDF/A font group is run under, for the reason the
/// module doc gives.
const SHARED_READING: Flavour = Flavour {
    part: Part::Two,
    level: Some(Level::U),
};

/// Runs the font group.
pub(super) fn rules(
    doc: &std::sync::Arc<CosDocument>,
    machinery: &Machinery,
    _part: UaPart,
    out: &mut Vec<UaRaw>,
) {
    if !machinery.reach(RuleGroup::Fonts) {
        return;
    }

    let mut shared = Vec::new();
    crate::pdfa::fonts::run(doc, Some(SHARED_READING), &mut shared);
    for raw in shared {
        if let Some(rule) = ua_clause_of(&raw.kind) {
            out.push(UaRaw {
                rule,
                object: raw.object,
                kind: raw.kind,
            });
        }
    }

    for (reference, codes) in &drawn_codes(doc) {
        if let Some(codes) = codes {
            unicode_values(doc, *reference, codes, out);
        }
    }
}

/// The PDF/UA clause that asks what a font finding says, or `None` for a
/// finding no published PDF/UA rule states.
pub(super) fn ua_clause_of(kind: &FindingKind) -> Option<UaClauses> {
    Some(match kind {
        FindingKind::FontNotEmbedded { .. } => clauses::FONT_EMBEDDING,
        FindingKind::CidToGidMapMalformed { .. } => clauses::CID_TO_GID,
        FindingKind::EncodingNotStandard { .. } | FindingKind::SymbolicFontHasEncoding => {
            clauses::TRUETYPE_ENCODINGS
        }
        FindingKind::ToUnicodeMissing | FindingKind::ToUnicodeValueForbidden { .. } => {
            clauses::UNICODE_MAPPING
        }
        FindingKind::CMapNotEmbedded { .. }
        | FindingKind::CMapWritingModeMismatch { .. }
        | FindingKind::CMapReferenceNotStandard { .. } => clauses::CMAPS,
        FindingKind::CidSystemInfoMismatch { .. } => clauses::CID_SYSTEM_INFO,
        FindingKind::GlyphWidthInconsistent { .. } => clauses::FONT_WIDTHS,
        _ => return None,
    })
}

/// Every code a text-showing operator drew, by the font it drew with; `None`
/// for a font with no `/ToUnicode` stream, which the value rule has nothing
/// to judge in.
///
/// **At any rendering mode**, unlike the font group's own usage scan: the
/// `/ToUnicode` value rule is about text extraction, not about painting, and
/// veraPDF's statement of 7.21.7-2 says the requirement holds "regardless of
/// their rendering mode usage". Invisible OCR text is exactly the text a
/// screen reader reads.
fn drawn_codes(doc: &CosDocument) -> BTreeMap<ObjRef, Option<Codes>> {
    let mut drawn = Drawn::default();
    content::walk(doc, &mut |op| {
        if !matches!(op.operator, b"Tj" | b"TJ" | b"'" | b"\"") {
            return;
        }
        let (Some(name), Some(resources)) = (op.font, op.resources) else {
            return;
        };
        let Some(reference) = content::lookup(doc, resources, b"Font", name) else {
            return;
        };
        for token in op.operands {
            if let Token::String(bytes) = token {
                drawn.add(doc, reference, bytes);
            }
        }
    });
    drawn.flush(doc);
    drawn.codes
}

/// The walk's state: the codes found so far, and the strings drawn since
/// they were last split into codes.
#[derive(Default)]
struct Drawn {
    /// Every font met, up to [`MAX_FONTS`].
    codes: BTreeMap<ObjRef, Option<Codes>>,
    /// Distinct strings not yet split, by font.
    pending: BTreeMap<ObjRef, BTreeSet<Vec<u8>>>,
    /// What `pending` holds, as [`MAX_PENDING_BYTES`] counts it.
    pending_bytes: usize,
}

impl Drawn {
    /// One string drawn with the font at `reference`.
    fn add(&mut self, doc: &CosDocument, reference: ObjRef, bytes: &[u8]) {
        let judged = match self.codes.get(&reference) {
            Some(codes) => codes.is_some(),
            None => {
                if self.codes.len() >= MAX_FONTS {
                    return;
                }
                let judged = to_unicode_of(doc, reference).is_some();
                self.codes.insert(reference, judged.then(Codes::default));
                judged
            }
        };
        if !judged {
            return;
        }
        let strings = self.pending.entry(reference).or_default();
        if strings.contains(bytes) {
            return;
        }
        strings.insert(bytes.to_vec());
        self.pending_bytes = self
            .pending_bytes
            .saturating_add(bytes.len() + STRING_OVERHEAD);
        if self.pending_bytes >= MAX_PENDING_BYTES {
            self.flush(doc);
        }
    }

    /// Splits every held string into codes, through its font's own encoding
    /// (a simple font's bytes, a composite font's CMap), and lets it go. A
    /// font is read once a flush, and only a font with strings held.
    fn flush(&mut self, doc: &CosDocument) {
        for (reference, strings) in std::mem::take(&mut self.pending) {
            let Some(Some(codes)) = self.codes.get_mut(&reference) else {
                continue;
            };
            let Ok(object) = doc.get(reference) else {
                continue;
            };
            let Some(dict) = object.as_dict() else {
                continue;
            };
            let font = tinker_pdf_cos::font::read(doc, dict);
            for string in &strings {
                for decoded in font.decode(string) {
                    codes.insert(decoded.code);
                }
            }
        }
        self.pending_bytes = 0;
    }
}

/// The codes one font drew.
#[derive(Default)]
struct Codes {
    /// Codes below 2^16, one bit each, grown as far as the highest one.
    narrow: Vec<u64>,
    /// Codes from 2^16 up, at most [`MAX_WIDE_CODES_PER_FONT`].
    wide: BTreeSet<u32>,
}

impl Codes {
    fn insert(&mut self, code: u32) {
        match u16::try_from(code) {
            Ok(code) => {
                let word = usize::from(code / 64);
                if self.narrow.len() <= word {
                    self.narrow.resize(word + 1, 0);
                }
                if let Some(bits) = self.narrow.get_mut(word) {
                    *bits |= 1 << (code % 64);
                }
            }
            Err(_) => {
                if self.wide.len() < MAX_WIDE_CODES_PER_FONT {
                    self.wide.insert(code);
                }
            }
        }
    }

    /// Every code, in ascending order.
    fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        (0u32..)
            .zip(&self.narrow)
            .flat_map(|(word, bits)| {
                (0..64u32)
                    .filter(move |bit| bits >> bit & 1 == 1)
                    .map(move |bit| word * 64 + bit)
            })
            .chain(self.wide.iter().copied())
    }
}

/// The font's `/ToUnicode` stream, when it has one by reference.
fn to_unicode_of(doc: &CosDocument, reference: ObjRef) -> Option<ObjRef> {
    let object = doc.get(reference).ok()?;
    let dict = object.as_dict()?;
    let to_unicode = dict.get_ref(doc.intern(b"ToUnicode"))?;
    matches!(doc.get(to_unicode).as_deref(), Ok(Object::Stream(_))).then_some(to_unicode)
}

/// ISO 14289-1 7.21.7, second sentence, as veraPDF's 7.21.7-2 states it:
/// "The Unicode values specified in the ToUnicode CMap shall all be greater
/// than zero (0), but not equal to either U+FEFF or U+FFFE." Judged over the
/// codes a page draws, which is the object veraPDF's rule is stated over —
/// a `/ToUnicode` may carry entries for codes nothing shows.
fn unicode_values(doc: &CosDocument, reference: ObjRef, codes: &Codes, out: &mut Vec<UaRaw>) {
    let Some(to_unicode) = to_unicode_of(doc, reference) else {
        return;
    };
    let Ok(bytes) = doc.stream_decoded(to_unicode) else {
        return;
    };
    let cmap = tinker_pdf_font::cmap::parse(&bytes);
    let mut reported = 0usize;
    for code in codes.iter() {
        if reported >= MAX_VALUE_FINDINGS {
            return;
        }
        let Some(text) = cmap.to_unicode_string(code) else {
            continue;
        };
        if let Some(bad) = text
            .chars()
            .find(|c| matches!(*c, '\u{0}' | '\u{FEFF}' | '\u{FFFE}'))
        {
            reported += 1;
            out.push(UaRaw {
                rule: clauses::UNICODE_MAPPING,
                object: Some(reference),
                kind: FindingKind::ToUnicodeValueForbidden {
                    code,
                    value: u32::from(bad),
                },
            });
        }
    }
}
