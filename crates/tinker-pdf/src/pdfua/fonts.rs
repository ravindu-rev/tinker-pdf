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

/// How many distinct strings one font contributes to the `/ToUnicode` value
/// check.
///
/// The rule is about codes, and a document with a million `Tj`s draws the
/// same few hundred codes over and over; keeping every string would make the
/// check's memory the size of the text.
const MAX_STRINGS_PER_FONT: usize = 1 << 12;

/// How many fonts the `/ToUnicode` value check reads.
const MAX_FONTS: usize = 1 << 12;

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

    let drawn = drawn_strings(doc);
    for (reference, strings) in drawn.iter().take(MAX_FONTS) {
        unicode_values(doc, *reference, strings, out);
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

/// Every string a text-showing operator drew, by the font it drew with.
///
/// **At any rendering mode**, unlike the font group's own usage scan: the
/// `/ToUnicode` value rule is about text extraction, not about painting, and
/// veraPDF's statement of 7.21.7-2 says the requirement holds "regardless of
/// their rendering mode usage". Invisible OCR text is exactly the text a
/// screen reader reads.
fn drawn_strings(doc: &CosDocument) -> BTreeMap<ObjRef, BTreeSet<Vec<u8>>> {
    let mut drawn: BTreeMap<ObjRef, BTreeSet<Vec<u8>>> = BTreeMap::new();
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
        if !drawn.contains_key(&reference) && drawn.len() >= MAX_FONTS {
            return;
        }
        let strings = drawn.entry(reference).or_default();
        for token in op.operands {
            if let Token::String(bytes) = token {
                if strings.len() >= MAX_STRINGS_PER_FONT {
                    return;
                }
                strings.insert(bytes.clone());
            }
        }
    });
    drawn
}

/// ISO 14289-1 7.21.7, second sentence, as veraPDF's 7.21.7-2 states it:
/// "The Unicode values specified in the ToUnicode CMap shall all be greater
/// than zero (0), but not equal to either U+FEFF or U+FFFE." Judged over the
/// codes a page draws, which is the object veraPDF's rule is stated over —
/// a `/ToUnicode` may carry entries for codes nothing shows.
fn unicode_values(
    doc: &CosDocument,
    reference: ObjRef,
    strings: &BTreeSet<Vec<u8>>,
    out: &mut Vec<UaRaw>,
) {
    let Ok(object) = doc.get(reference) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };
    let key = doc.intern(b"ToUnicode");
    let Some(to_unicode) = dict.get_ref(key) else {
        return;
    };
    if !matches!(doc.get(to_unicode).as_deref(), Ok(Object::Stream(_))) {
        return;
    }
    let Ok(bytes) = doc.stream_decoded(to_unicode) else {
        return;
    };
    let cmap = tinker_pdf_font::cmap::parse(&bytes);
    let font = tinker_pdf_cos::font::read(doc, dict);
    let mut reported: BTreeSet<u32> = BTreeSet::new();
    for string in strings {
        for decoded in font.decode(string) {
            if reported.len() >= MAX_VALUE_FINDINGS || reported.contains(&decoded.code) {
                continue;
            }
            let Some(text) = cmap.to_unicode_string(decoded.code) else {
                continue;
            };
            if let Some(bad) = text
                .chars()
                .find(|c| matches!(*c, '\u{0}' | '\u{FEFF}' | '\u{FFFE}'))
            {
                reported.insert(decoded.code);
                out.push(UaRaw {
                    rule: clauses::UNICODE_MAPPING,
                    object: Some(reference),
                    kind: FindingKind::ToUnicodeValueForbidden {
                        code: decoded.code,
                        value: u32::from(bad),
                    },
                });
            }
        }
    }
}
