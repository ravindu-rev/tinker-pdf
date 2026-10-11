//! The PDF/X syntax group: what the COS document alone decides.
//!
//! Encryption (AN 2.11), the forbidden filters (AN 2.8), `/Trapped` (AN
//! 2.17), the page boxes (AN 2.10), annotations against them (AN 2.28) and the
//! private `/Info` keys (AN 2.29) — each a transcription of the CGATS
//! application notes as `docs/design/pdfx.md` quotes them, each held to a
//! fixture and its near-miss twin in `tests/pdfx_rules.rs`.

use std::collections::BTreeSet;

use tinker_pdf_cos::{pages, CosDocument, Dict, ObjRef, Object, Rect, XrefEntry};

use super::{clauses, info, XRaw};
use crate::pdfa::FindingKind;

/// How many objects the filter sweep visits, so a damaged cross-reference
/// table cannot turn a validation into a sweep (ruling 1) — the bound the
/// PDF/A syntax group's own sweep has.
const MAX_OBJECTS: usize = 1 << 20;

/// How many pages the box and annotation rules visit.
const MAX_PAGES: usize = 1 << 14;

/// How many annotations one page contributes.
const MAX_ANNOTATIONS: usize = 4096;

/// How many filter names one stream contributes.
const MAX_FILTER_NAMES: usize = 32;

/// How many `/Info` entries are read.
const MAX_INFO_ENTRIES: usize = 4096;

/// How far up the page tree an inherited box is looked for.
const MAX_PARENT_DEPTH: u32 = 64;

/// How many findings one rule contributes: the PDF/UA group's figure, for
/// its reason. A file with a thousand pages without a trim box has one
/// defect, and a verdict carrying a thousand copies of it is not provenance
/// (ruling 10) — and one annotation named 4 096 times from an `/Annots` array
/// every page shares was 67 million of them at the page cap (ruling 1).
const MAX_FINDINGS_PER_RULE: usize = 64;

/// How many `/Annots` entries the annotation rule reads across the whole
/// document, duplicates included. An annotation is judged once and a shared
/// array read once, which makes the walk's work what the file holds; this
/// bounds what it may hold, as `MAX_ANNOTATION_OBJECTS` bounds the PDF/A
/// colour group's annotation sweep.
const MAX_ANNOTATION_ENTRIES: usize = 1 << 18;

/// Runs every syntax rule. The level is not an argument: every rule here
/// runs under both 2003 levels, and the clause table numbers it.
pub(super) fn rules(doc: &CosDocument, out: &mut Vec<XRaw>) {
    // AN 2.11: "None of the PDF/X standards covered by these application
    // notes permit the use of PDF-based encryption." A file-level finding,
    // as ISO 19005's is.
    if doc.is_encrypted() {
        out.push(XRaw {
            rule: clauses::ENCRYPTION,
            object: None,
            kind: FindingKind::Encrypted,
        });
    }
    information(doc, out);
    filters(doc, out);
    let mut boxes_reported = 0;
    let mut annotations = AnnotationWalk::default();
    for page in pages::collect_upto(doc, MAX_PAGES) {
        let Ok(object) = doc.get(page.reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let boxes = Boxes::of(doc, dict);
        if boxes_reported < MAX_FINDINGS_PER_RULE {
            let mut found = Vec::new();
            boxes.judge(page.reference, &mut found);
            let room = MAX_FINDINGS_PER_RULE - boxes_reported;
            boxes_reported += found.len().min(room);
            out.extend(found.into_iter().take(room));
        }
        annotations.page(doc, dict, &boxes, page.reference, out);
    }
}

// ---- AN 2.17 trapping and AN 2.29 private keys -----------------------------

/// The keys ISO 32000-1 14.3.3 Table 317 defines in the document information
/// dictionary, and the two ISO 15930 adds: everything else is a private key.
const DEFINED_INFO_KEYS: &[&[u8]] = &[
    b"Title",
    b"Author",
    b"Subject",
    b"Keywords",
    b"Creator",
    b"Producer",
    b"CreationDate",
    b"ModDate",
    b"Trapped",
    b"GTS_PDFXVersion",
    b"GTS_PDFXConformance",
];

/// `/Trapped` and the private keys.
///
/// AN 2.17: `/Trapped` is required, and is "a name object — /True or /False —
/// and not a boolean"; `/Unknown`, which ISO 32000-1 Table 317 admits and
/// makes the default, is not permitted. So an absent key is one finding, and
/// a key that is present as anything but those two names is another, with
/// what was there named — a boolean `true` is the mistake the note calls out
/// by name.
///
/// AN 2.29: a private key's value is a text string. A value that is a name, a
/// number or a dictionary is reported with its key.
fn information(doc: &CosDocument, out: &mut Vec<XRaw>) {
    // The claim was read from this dictionary, so it is there; but a rule
    // that assumed so would be one refactor from a panic.
    let Some((info, at)) = info(doc) else {
        return;
    };
    let trapped = doc.resolve_key(&info, doc.intern(b"Trapped"));
    let found = match trapped.as_ref() {
        Object::Null => Some(None),
        Object::Name(name) => match doc.name_bytes(*name).as_deref() {
            Some(b"True" | b"False") => None,
            Some(other) => Some(Some(format!("/{}", String::from_utf8_lossy(other)))),
            None => Some(Some("a name".to_string())),
        },
        Object::Bool(value) => Some(Some(value.to_string())),
        Object::String(_) => Some(Some("a string".to_string())),
        _ => Some(Some("neither a name nor a boolean".to_string())),
    };
    match found {
        None => {}
        Some(None) => out.push(XRaw {
            rule: clauses::TRAPPING,
            object: at,
            kind: FindingKind::TrappedMissing,
        }),
        Some(Some(found)) => out.push(XRaw {
            rule: clauses::TRAPPING,
            object: at,
            kind: FindingKind::TrappedInvalid { found },
        }),
    }

    for (key, value) in info.entries().iter().take(MAX_INFO_ENTRIES) {
        let Some(name) = doc.name_bytes(*key) else {
            continue;
        };
        if DEFINED_INFO_KEYS.contains(&name.as_ref()) {
            continue;
        }
        if !matches!(doc.resolve(value).as_ref(), Object::String(_)) {
            out.push(XRaw {
                rule: clauses::INFO_KEYS,
                object: at,
                kind: FindingKind::InfoValueNotText {
                    key: String::from_utf8_lossy(&name).into_owned(),
                },
            });
        }
    }
}

// ---- AN 2.8 data compression ------------------------------------------------

/// AN 2.8: any lossless filter "other than LZW", and "JBIG2 compression may
/// not be used".
///
/// Every stream the cross-reference table reaches, used for rendering or not,
/// because the note is about the file's compression rather than about the
/// page: an LZW-encoded embedded file is LZW in the file. The abbreviated
/// spelling is the inline-image one and a stream dictionary should not carry
/// it, but a rule that knew only the long name would pass the same filter
/// written short. An inline image's own dictionary is not reached — the
/// content walk steps over it — and `super::STAGED` says so.
fn filters(doc: &CosDocument, out: &mut Vec<XRaw>) {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut reported = 0;
    for (num, entry) in doc.xref().iter().take(MAX_OBJECTS) {
        if reported >= MAX_FINDINGS_PER_RULE {
            return;
        }
        let gen = match entry {
            XrefEntry::Free { .. } => continue,
            XrefEntry::Offset { gen, .. } => gen,
            // 7.5.7: a stream is never inside an object stream.
            XrefEntry::InStream { .. } => continue,
        };
        if !seen.insert(num) {
            continue;
        }
        let reference = ObjRef::new(num, gen);
        let Ok(object) = doc.get(reference) else {
            continue;
        };
        let Some(stream) = object.as_stream() else {
            continue;
        };
        for name in filter_names(doc, &stream.dict) {
            if matches!(name.as_slice(), b"LZWDecode" | b"LZW" | b"JBIG2Decode")
                && reported < MAX_FINDINGS_PER_RULE
            {
                reported += 1;
                out.push(XRaw {
                    rule: clauses::COMPRESSION,
                    object: Some(reference),
                    kind: FindingKind::FilterForbidden {
                        filter: String::from_utf8_lossy(&name).into_owned(),
                    },
                });
            }
        }
    }
}

/// The names a stream's `/Filter` holds: one name, or an array of them.
fn filter_names(doc: &CosDocument, dict: &Dict) -> Vec<Vec<u8>> {
    let value = doc.resolve_key(dict, doc.intern(b"Filter"));
    let mut names = Vec::new();
    let mut push = |object: &Object| {
        if let Some(bytes) = object.as_name().and_then(|name| doc.name_bytes(name)) {
            names.push(bytes.to_vec());
        }
    };
    match value.as_ref() {
        Object::Array(values) => {
            for value in values.iter().take(MAX_FILTER_NAMES) {
                push(&doc.resolve(value));
            }
        }
        other => push(other),
    }
    names
}

// ---- AN 2.10 the page boxes -------------------------------------------------

/// A page's boxes as the page dictionary states them, **before** ISO 32000's
/// defaults: the rules are about what the producer wrote, and a trim box
/// that defaulted to the crop box is a trim box nobody stated.
struct Boxes {
    /// `/MediaBox`, the page's own or inherited (7.7.3.3 Table 30).
    media: Option<Rect>,
    /// `/CropBox`, the page's own or inherited.
    crop: Option<Rect>,
    /// `/BleedBox`, the page's own only: Table 30 does not make it
    /// inheritable.
    bleed: Option<Rect>,
    /// `/TrimBox`, the page's own only.
    trim: Option<Rect>,
    /// `/ArtBox`, the page's own only.
    art: Option<Rect>,
}

impl Boxes {
    fn of(doc: &CosDocument, page: &Dict) -> Boxes {
        Boxes {
            media: inherited(doc, page, b"MediaBox"),
            crop: inherited(doc, page, b"CropBox"),
            bleed: own(doc, page, b"BleedBox"),
            trim: own(doc, page, b"TrimBox"),
            art: own(doc, page, b"ArtBox"),
        }
    }

    /// AN 2.10, three requirements:
    ///
    /// 1. `/MediaBox` is required;
    /// 2. "each PDF/X page shall include either an ArtBox or TrimBox, but not
    ///    both";
    /// 3. a `/BleedBox` is optional, and neither the art nor the trim box may
    ///    extend beyond it — and the same for the `/CropBox`.
    ///
    /// The third is containment, compared exactly: a trim box whose edge sits
    /// a thousandth of a point past the bleed box's extends beyond it, and no
    /// source in hand offers a tolerance to excuse that.
    fn judge(&self, at: ObjRef, out: &mut Vec<XRaw>) {
        if self.media.is_none() {
            out.push(XRaw {
                rule: clauses::BOXES,
                object: Some(at),
                kind: FindingKind::PageBoxMissing {
                    key: "MediaBox".to_string(),
                },
            });
        }
        match (self.trim, self.art) {
            (None, None) => out.push(XRaw {
                rule: clauses::BOXES,
                object: Some(at),
                kind: FindingKind::TrimOrArtBoxMissing,
            }),
            (Some(_), Some(_)) => out.push(XRaw {
                rule: clauses::BOXES,
                object: Some(at),
                kind: FindingKind::TrimAndArtBox,
            }),
            _ => {}
        }
        for (inner_key, inner) in [("TrimBox", self.trim), ("ArtBox", self.art)] {
            let Some(inner) = inner else {
                continue;
            };
            for (outer_key, outer) in [("BleedBox", self.bleed), ("CropBox", self.crop)] {
                let Some(outer) = outer else {
                    continue;
                };
                if !contains(&outer, &inner) {
                    out.push(XRaw {
                        rule: clauses::BOXES,
                        object: Some(at),
                        kind: FindingKind::PageBoxOutside {
                            inner: inner_key.to_string(),
                            outer: outer_key.to_string(),
                        },
                    });
                }
            }
        }
    }
}

/// Whether `inner` lies inside `outer`, edges included.
fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

/// A box the page dictionary itself carries, as a rectangle.
///
/// A value that is not four numbers is no box: [`Rect::from_array`] refuses
/// it, and the rule reads it as absent rather than guessing at a shape.
fn own(doc: &CosDocument, page: &Dict, key: &[u8]) -> Option<Rect> {
    let value = doc.resolve_key(page, doc.intern(key));
    let values = value.as_array()?;
    let resolved: Vec<Object> = values
        .iter()
        .take(4)
        .map(|v| doc.resolve(v).as_ref().clone())
        .collect();
    Rect::from_array(&resolved)
}

/// A box the page carries or inherits from a `/Pages` node above it, for the
/// two Table 30 makes inheritable.
fn inherited(doc: &CosDocument, page: &Dict, key: &[u8]) -> Option<Rect> {
    if let Some(rect) = own(doc, page, key) {
        return Some(rect);
    }
    let parent_key = doc.intern(b"Parent");
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut next = page.get_ref(parent_key);
    for _ in 0..MAX_PARENT_DEPTH {
        let reference = next?;
        if !seen.insert(reference.num) {
            return None;
        }
        let object = doc.get(reference).ok()?;
        let node = object.as_dict()?;
        if let Some(rect) = own(doc, node, key) {
            return Some(rect);
        }
        next = node.get_ref(parent_key);
    }
    None
}

// ---- AN 2.28 annotations ----------------------------------------------------

/// AN 2.28: annotations "must fall entirely outside the BleedBox"; a
/// `PrinterMark` annotation may sit inside the bleed and must stay outside
/// the trim or art box.
///
/// **Which box stands in when the page has no `/BleedBox`** is a reading, and
/// it is the narrower one. ISO 32000-1 14.11.2 would default the bleed box to
/// the crop box; the notes as the design quotes them do not say, and a reader
/// that used the crop box would report every annotation in the margin of a
/// page that states no bleed. So without a bleed box the page's own trim or
/// art box is the boundary, and without either the rule is silent — the box
/// rule has already reported that page.
///
/// "Entirely outside" is read as sharing no area: an annotation whose
/// rectangle touches the boundary's edge, or has no area at all — the
/// invisible signature widget's `[0 0 0 0]` — is outside it.
///
/// A `TrapNet` annotation is held to neither rule: the design names a rule of
/// its own for it and quotes none of it, and `super::UNREAD` says so.
///
/// **An annotation is judged once, on the first page that names it**, and an
/// indirect `/Annots` array is read once, on the first page that holds it:
/// ISO 32000-1 12.5.2's `/P` gives an annotation one page, and a file that
/// names one from many has not drawn it many times. Without that, one
/// annotation named 4 096 times from an array every page shared was one
/// finding per page per entry. The walk reads at most
/// [`MAX_ANNOTATION_ENTRIES`] entries and reports at most
/// [`MAX_FINDINGS_PER_RULE`] findings, and stops at whichever comes first.
#[derive(Default)]
struct AnnotationWalk {
    /// Indirect `/Annots` arrays already read.
    arrays: BTreeSet<ObjRef>,
    /// Indirect annotations already judged.
    judged: BTreeSet<ObjRef>,
    /// Entries read so far, duplicates included.
    entries: usize,
    /// Findings reported so far.
    reported: usize,
}

impl AnnotationWalk {
    /// Whether the walk has spent its budget or its findings.
    fn done(&self) -> bool {
        self.entries >= MAX_ANNOTATION_ENTRIES || self.reported >= MAX_FINDINGS_PER_RULE
    }

    /// Judges the annotations `page` names against its `boxes`.
    fn page(
        &mut self,
        doc: &CosDocument,
        page: &Dict,
        boxes: &Boxes,
        at: ObjRef,
        out: &mut Vec<XRaw>,
    ) {
        if self.done() {
            return;
        }
        let key = doc.intern(b"Annots");
        if let Some(array) = page.get(key).and_then(Object::as_objref) {
            if !self.arrays.insert(array) {
                return;
            }
        }
        let annots = doc.resolve_key(page, key);
        let Some(annots) = annots.as_array() else {
            return;
        };
        for entry in annots.iter().take(MAX_ANNOTATIONS) {
            if self.done() {
                return;
            }
            self.entries += 1;
            if let Some(reference) = entry.as_objref() {
                if !self.judged.insert(reference) {
                    continue;
                }
            }
            let object = entry.as_objref().or(Some(at));
            let resolved = doc.resolve(entry);
            let Some(annot) = resolved.as_dict() else {
                continue;
            };
            let subtype = doc
                .resolve_key(annot, doc.intern(b"Subtype"))
                .as_name()
                .and_then(|name| doc.name_bytes(name))
                .map(|name| name.to_vec())
                .unwrap_or_default();
            let rect = doc.resolve_key(annot, doc.intern(b"Rect"));
            let Some(rect) = rect.as_array().and_then(|values| {
                let resolved: Vec<Object> = values
                    .iter()
                    .take(4)
                    .map(|v| doc.resolve(v).as_ref().clone())
                    .collect();
                Rect::from_array(&resolved)
            }) else {
                continue;
            };
            let boundary = match subtype.as_slice() {
                b"TrapNet" => continue,
                b"PrinterMark" => trim_or_art(boxes),
                _ => boxes
                    .bleed
                    .map(|bleed| ("BleedBox", bleed))
                    .or_else(|| trim_or_art(boxes)),
            };
            let Some((name, boundary)) = boundary else {
                continue;
            };
            if rect.intersect(&boundary).is_some() {
                self.reported += 1;
                out.push(XRaw {
                    rule: clauses::ANNOTATIONS,
                    object,
                    kind: FindingKind::AnnotationInsideBox {
                        subtype: String::from_utf8_lossy(&subtype).into_owned(),
                        boundary: name.to_string(),
                    },
                });
            }
        }
    }
}

/// The trim box the page states, or failing that its art box.
fn trim_or_art(boxes: &Boxes) -> Option<(&'static str, Rect)> {
    boxes
        .trim
        .map(|trim| ("TrimBox", trim))
        .or_else(|| boxes.art.map(|art| ("ArtBox", art)))
}
