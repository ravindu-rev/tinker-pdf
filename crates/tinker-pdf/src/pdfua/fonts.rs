//! The font group: ISO 14289-1 7.21 and ISO 14289-2 8.4.5.
//!
//! Milestone 1 of `docs/design/pdfua.md` moves the census's one font rule in
//! unchanged — every font a page's resources name carries an embedded program,
//! the standard 14 included (ISO 14289-1 7.21.4.1 exempts none of them).

use tinker_pdf_cos::{pages, CosDocument, ObjRef, Object};

use super::{clauses, UaPart, UaRaw};
use crate::pdfa::{FindingKind, Machinery, RuleGroup};

/// How many pages the scan visits.
const MAX_PAGES: usize = 1 << 14;

/// How many unembedded fonts one document contributes.
const MAX_FINDINGS: usize = 64;

/// Runs the font group.
pub(super) fn rules(doc: &CosDocument, machinery: &Machinery, _part: UaPart, out: &mut Vec<UaRaw>) {
    if !machinery.reach(RuleGroup::Fonts) {
        return;
    }
    let mut seen = std::collections::BTreeSet::new();
    for page in pages::collect_upto(doc, MAX_PAGES) {
        let Some(resources) = page.resources.as_ref() else {
            continue;
        };
        let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
        let Some(fonts) = fonts.as_dict() else {
            continue;
        };
        for (_, entry) in fonts.entries() {
            if seen.len() >= MAX_FINDINGS {
                return;
            }
            let reference = entry.as_objref();
            let font = doc.resolve(entry);
            let Some(font) = font.as_dict() else {
                continue;
            };
            if let Some((carrier, subtype)) = unembedded(doc, font, reference) {
                if seen.insert(carrier) {
                    out.push(UaRaw {
                        rule: clauses::FONT_EMBEDDING,
                        object: carrier,
                        kind: FindingKind::FontNotEmbedded { subtype },
                    });
                }
            }
        }
    }
}

/// The dictionary that should carry the program, and its `/Subtype`, when it
/// carries none.
///
/// 9.7.1: a composite font's program hangs off its descendant, so asking the
/// Type 0 dictionary alone would answer "unembedded" for every CID font. A
/// Type 3 font's glyphs are content streams and it has no program to embed.
fn unembedded(
    doc: &CosDocument,
    font: &tinker_pdf_cos::Dict,
    reference: Option<ObjRef>,
) -> Option<(Option<ObjRef>, String)> {
    let subtype_of = |dict: &tinker_pdf_cos::Dict| -> String {
        doc.resolve_key(dict, doc.intern(b"Subtype"))
            .as_name()
            .and_then(|name| doc.name_bytes(name))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    let subtype = subtype_of(font);
    let (carrier, carrier_ref) = if subtype == "Type0" {
        let descendants = doc.resolve_key(font, doc.intern(b"DescendantFonts"));
        let first = descendants.as_array().and_then(<[Object]>::first)?;
        let resolved = doc.resolve(first);
        let dict = resolved.as_dict()?.clone();
        (dict, first.as_objref().or(reference))
    } else if subtype == "Type3" {
        return None;
    } else {
        (font.clone(), reference)
    };
    let described = doc.resolve_key(&carrier, doc.intern(b"FontDescriptor"));
    let embedded = described.as_dict().is_some_and(|descriptor| {
        [&b"FontFile"[..], b"FontFile2", b"FontFile3"]
            .iter()
            .any(|key| !doc.resolve_key(descriptor, doc.intern(key)).is_null())
    });
    (!embedded).then(|| (carrier_ref, subtype_of(&carrier)))
}
