//! The annotations group: ISO 14289-1 7.18, annotations against the logical
//! structure (milestone 4 of `docs/design/pdfua.md`, part 1's half).
//!
//! Every rule here is veraPDF's published statement of a 7.18 rule, read as
//! data (`PDFUA-1.xml` at `veraPDF-validation-profiles` `070d39f`, the wiki
//! at `109b482`) and never run (ruling 13), with the exemptions its test
//! conditions state and the corpus readings the design records:
//!
//! - **hidden or off the page** — an annotation whose `/F` sets the hidden
//!   bit, or whose `/Rect` shares no area with the page's crop box, is held
//!   to none of the tagging and description rules (7.18.1-t02-pass-c, -d);
//! - **a widget's `/TU` is its field's**: the field the widget belongs to
//!   carries it, and a `/TU` on two widget kids of a field with none does not
//!   count (7.18.1-t03-pass-e against -fail-d);
//! - **a subtype ISO 32000-1 Table 169 does not define is not judged**: the
//!   fixtures carrying an upper-case `FREETEXT` are annotated `pass`.
//!
//! # The enclosing element is the tree's `/OBJR`
//!
//! veraPDF finds an annotation's structure parent through its
//! `/StructParent` and the `/ParentTree`; this reads the element whose kid is
//! an `/OBJR` naming the annotation, which the structure reader already has
//! (`StructKid::Object`). The two agree on every well-formed file. Where they
//! do not — an `/OBJR` with no `/StructParent` back to it — this reading
//! finds an enclosing element veraPDF does not, and so errs towards silence.

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_cos::{decode_text_string, pages, CosDocument, Dict, ObjRef, Object, Rect};

use super::structure::MAX_FINDINGS_PER_RULE;
use super::{clauses, UaRaw};
use crate::pdfa::FindingKind;
use crate::structure::{StructKid, StructureTree};

/// How many pages the group visits.
const MAX_PAGES: usize = 1 << 14;

/// How many annotations one page contributes.
const MAX_ANNOTATIONS: usize = 4096;

/// How many `/Annots` entries the group reads across the whole document,
/// duplicates included. An annotation is judged once and a shared array read
/// once, which makes the walk's work what the file holds; this bounds what it
/// may hold, as `MAX_ANNOTATION_OBJECTS` bounds the PDF/A colour group's
/// annotation sweep. Without either, 1 024 pages sharing one `/Annots` of
/// 4 096 entries — a 166 KB file — cost 13.6 s after the last finding the
/// group would report.
const MAX_ANNOTATION_ENTRIES: usize = 1 << 18;

/// How many findings the group reports, across its rules.
const MAX_FINDINGS: usize = MAX_FINDINGS_PER_RULE * 4;

/// The annotation subtypes ISO 32000-1 Table 169 defines. A subtype outside
/// it is not one the rules are about.
const STANDARD_SUBTYPES: &[&[u8]] = &[
    b"Text",
    b"Link",
    b"FreeText",
    b"Line",
    b"Square",
    b"Circle",
    b"Polygon",
    b"PolyLine",
    b"Highlight",
    b"Underline",
    b"Squiggly",
    b"StrikeOut",
    b"Stamp",
    b"Caret",
    b"Ink",
    b"Popup",
    b"FileAttachment",
    b"Sound",
    b"Movie",
    b"Widget",
    b"Screen",
    b"PrinterMark",
    b"TrapNet",
    b"Watermark",
    b"3D",
];

/// The element an `/OBJR` places an annotation in: its standard type and its
/// `/Alt`.
struct Enclosing {
    standard_type: String,
    alt: Option<String>,
}

/// Runs the group over every page, given the tree when there is one.
pub(super) fn rules(doc: &CosDocument, tree: Option<&StructureTree>, out: &mut Vec<UaRaw>) {
    let mut enclosing: BTreeMap<ObjRef, Enclosing> = BTreeMap::new();
    if let Some(tree) = tree {
        for element in tree.elements() {
            for kid in &element.kids {
                if let StructKid::Object(reference) = kid {
                    enclosing.entry(*reference).or_insert_with(|| Enclosing {
                        standard_type: element.standard_type.clone(),
                        alt: element.alt.clone(),
                    });
                }
            }
        }
    }
    let mut reported = Reported(0);
    // An annotation is judged once, on the first page that names it, and an
    // indirect `/Annots` array is read once, on the first page that holds
    // it: ISO 32000-1 12.5.2's `/P` gives an annotation one page, and a file
    // that names one from many has not placed it many times.
    let mut arrays: BTreeSet<ObjRef> = BTreeSet::new();
    let mut judged: BTreeSet<ObjRef> = BTreeSet::new();
    let mut entries = 0usize;

    for page in pages::collect_upto(doc, MAX_PAGES) {
        if reported.full() {
            break;
        }
        let Ok(object) = doc.get(page.reference) else {
            continue;
        };
        let Some(page_dict) = object.as_dict() else {
            continue;
        };
        let key = doc.intern(b"Annots");
        let annots = doc.resolve_key(page_dict, key);
        let Some(annots) = annots.as_array() else {
            continue;
        };
        if annots.is_empty() {
            continue;
        }
        let read_before = page_dict
            .get(key)
            .and_then(Object::as_objref)
            .is_some_and(|array| !arrays.insert(array));

        // 7.18.3-1: "Every page on which there is an annotation shall
        // contain in its page dictionary the key Tabs, and its value shall
        // be S."
        let tabs = doc
            .resolve_key(page_dict, doc.intern(b"Tabs"))
            .as_name()
            .and_then(|name| doc.name_bytes(name))
            .map(|name| String::from_utf8_lossy(&name).into_owned());
        if tabs.as_deref() != Some("S") {
            reported.push(
                UaRaw {
                    rule: clauses::TAB_ORDER,
                    object: Some(page.reference),
                    kind: FindingKind::TabOrderNotStructure { found: tabs },
                },
                out,
            );
        }

        if read_before {
            continue;
        }
        for entry in annots.iter().take(MAX_ANNOTATIONS) {
            if entries >= MAX_ANNOTATION_ENTRIES {
                break;
            }
            entries += 1;
            // An annotation written in place has no object an /OBJR can name,
            // so nothing encloses it; it is judged by the same rules, under
            // the page's number.
            let reference = entry.as_objref();
            if reference.is_some_and(|r| !judged.insert(r)) {
                continue;
            }
            let resolved = doc.resolve(entry);
            let Some(annot) = resolved.as_dict() else {
                continue;
            };
            let subtype = name_of(doc, annot, b"Subtype").unwrap_or_default();
            if !STANDARD_SUBTYPES.contains(&subtype.as_slice()) {
                continue;
            }
            if hidden(doc, annot) || outside(doc, annot, &page.crop_box) {
                continue;
            }
            let name = String::from_utf8_lossy(&subtype).into_owned();
            let parent = reference.and_then(|r| enclosing.get(&r));
            let parent_type = parent.map(|p| p.standard_type.as_str());
            let parent_alt = parent
                .and_then(|p| p.alt.as_deref())
                .is_some_and(|alt| !alt.is_empty());
            let contents = text(doc, annot, b"Contents").is_some_and(|t| !t.is_empty());
            let object = reference.or(Some(page.reference));

            // 7.18.1-1, 7.18.4-1, 7.18.5-1: the element each kind sits in.
            let expected = match subtype.as_slice() {
                b"Widget" => Some(("Form", clauses::WIDGET_FORM)),
                b"Link" => Some(("Link", clauses::LINKS)),
                // A printer's mark is an artifact (7.18.8 below), and a popup
                // is its parent's window: staged by name rather than read
                // as an annotation of its own that wants an Annot.
                b"PrinterMark" | b"Popup" => None,
                _ => Some(("Annot", clauses::ANNOTATION_TAGGING)),
            };
            // With no tree at all nothing encloses anything, and the missing
            // tree is the one finding worth having; each annotation reported
            // unenclosed beside it would be the same defect again.
            if let Some((expected, rule)) = expected.filter(|_| tree.is_some()) {
                if parent_type != Some(expected) {
                    reported.push(
                        UaRaw {
                            rule,
                            object,
                            kind: FindingKind::AnnotationNotEnclosed {
                                subtype: name.clone(),
                                expected: expected.to_string(),
                                enclosing: parent_type.map(str::to_string),
                            },
                        },
                        out,
                    );
                }
            }

            // 7.18.1-2 and 7.18.1-3: a description — the annotation's own
            // /Contents or the enclosing element's /Alt; for a widget, its
            // field's /TU or the enclosing /Alt.
            let described = match subtype.as_slice() {
                b"Widget" => field_tooltip(doc, annot) || parent_alt,
                b"Popup" => true,
                _ => contents || parent_alt,
            };
            if !described {
                reported.push(
                    UaRaw {
                        rule: clauses::ANNOTATION_TAGGING,
                        object,
                        kind: FindingKind::AnnotationDescriptionMissing {
                            subtype: name.clone(),
                        },
                    },
                    out,
                );
            }

            match subtype.as_slice() {
                // 7.18.2-1: "Annotations of subtype TrapNet shall not be
                // permitted."
                b"TrapNet" => reported.push(
                    UaRaw {
                        rule: clauses::TRAPNET,
                        object,
                        kind: FindingKind::AnnotationForbidden { subtype: name },
                    },
                    out,
                ),
                // 7.18.5-2: "Links shall contain an alternate description via
                // their Contents key" — the enclosing /Alt does not stand in.
                b"Link" if !contents => reported.push(
                    UaRaw {
                        rule: clauses::LINKS,
                        object,
                        kind: FindingKind::LinkContentsMissing,
                    },
                    out,
                ),
                // 7.18.8-1: a printer's mark is an incidental artifact, so it
                // is in no structure element.
                b"PrinterMark" if parent.is_some() => reported.push(
                    UaRaw {
                        rule: clauses::PRINTER_MARK,
                        object,
                        kind: FindingKind::PrinterMarkInStructure,
                    },
                    out,
                ),
                _ => {}
            }
        }
    }
}

/// The group's findings so far, which stop at [`MAX_FINDINGS`].
struct Reported(usize);

impl Reported {
    fn push(&mut self, raw: UaRaw, out: &mut Vec<UaRaw>) {
        if !self.full() {
            self.0 += 1;
            out.push(raw);
        }
    }

    fn full(&self) -> bool {
        self.0 >= MAX_FINDINGS
    }
}

/// `/F` sets bit 2, Hidden (12.5.3 Table 165).
fn hidden(doc: &CosDocument, annot: &Dict) -> bool {
    doc.resolve_key(annot, doc.intern(b"F"))
        .as_int()
        .is_some_and(|flags| flags & 2 == 2)
}

/// `/Rect` shares no area with the page's crop box. An annotation with no
/// readable rectangle is not outside anything.
fn outside(doc: &CosDocument, annot: &Dict, crop: &Rect) -> bool {
    let rect = doc.resolve_key(annot, doc.intern(b"Rect"));
    let Some(values) = rect.as_array() else {
        return false;
    };
    let resolved: Vec<Object> = values
        .iter()
        .take(4)
        .map(|v| doc.resolve(v).as_ref().clone())
        .collect();
    Rect::from_array(&resolved).is_some_and(|rect| rect.intersect(crop).is_none())
}

/// The widget's field carries a non-empty `/TU`: the widget itself when it is
/// merged with its field (it has a `/T`), its `/Parent` otherwise.
fn field_tooltip(doc: &CosDocument, widget: &Dict) -> bool {
    let tooltip = |dict: &Dict| text(doc, dict, b"TU").is_some_and(|t| !t.is_empty());
    if widget.contains_key(doc.intern(b"T")) {
        return tooltip(widget);
    }
    let parent = doc.resolve_key(widget, doc.intern(b"Parent"));
    match parent.as_dict() {
        Some(field) => tooltip(field),
        None => tooltip(widget),
    }
}

/// A text-string entry, decoded.
fn text(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<String> {
    doc.resolve_key(dict, doc.intern(key))
        .as_string()
        .map(|string| decode_text_string(&string.bytes))
}

/// A name entry's bytes.
fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    doc.resolve_key(dict, doc.intern(key))
        .as_name()
        .and_then(|name| doc.name_bytes(name))
        .map(|name| name.to_vec())
}
