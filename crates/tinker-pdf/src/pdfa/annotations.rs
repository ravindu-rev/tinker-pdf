//! The annotation rule group: ISO 19005-1 6.5, ISO 19005-2/3 6.3, ISO
//! 19005-4 6.3.
//!
//! # Why this rides the syntax group
//!
//! [`super::RuleGroup`] is a taxonomy of *machinery* rather than of clauses —
//! the metadata group is the XML parser, the font group is `tinker-pdf-font`,
//! the colour group is `tinker-pdf-color`. Every requirement here is decidable
//! from the COS document alone: a subtype name, an integer of flags, the
//! presence and shape of an `/AP`, and four numbers in a `/Rect`. Nothing
//! decodes a stream, parses a font program or builds a transform.
//!
//! So a group of its own would reach for nothing, its counter would prove
//! nothing, and [`super::Coverage::syntax`] — whose own documentation already
//! promises "encryption, version, filters, actions, annotations" — would have
//! become false by omission. The design doc put these rules in the syntax
//! group for the same reason, and this is where they are.
//!
//! **What the corpus only ever asks is whether an appearance stream
//! *exists*.** The staged entry these rules replace said the appearance rules
//! "need the annotation appearance machinery"; 164 annotated fixtures across
//! four parts say otherwise. Not one of them asks what an appearance draws.
//!
//! # The permitted set is a whitelist, and that is the safe direction
//!
//! A prohibited list passes every subtype nobody thought of. The clause is
//! written the other way round — an annotation type *defined in* the reference
//! specification and not among the excluded ones — so that is how it is
//! implemented: a subtype the part's reference specification does not define
//! is [`FindingKind::AnnotationTypeNonStandard`], and one it defines but
//! excludes is [`FindingKind::AnnotationTypeForbidden`]. The corpus
//! distinguishes them too, in its own fixtures' titles.

use std::sync::Arc;

use tinker_pdf_cos::{CosDocument, Dict, ObjRef};

use super::{clauses, FindingKind, Flavour, Level, Part, Raw};

/// The annotation types ISO 32000-1 12.5.6.1 defines, which is the reference
/// specification for parts 1 to 3.
///
/// Part 1's reference is PDF 1.4 and parts 2 and 3's is ISO 32000-1, so the
/// two differ — `Caret`, `Polygon`, `PolyLine` and `Redact` arrived after 1.4.
/// That difference is not modelled as two lists, because it does not change an
/// answer: every type in the gap is *excluded* by part 1 anyway, so a part 1
/// file carrying one is reported either way and under the same clause.
const STANDARD_TYPES: &[&[u8]] = &[
    b"3D",
    b"Caret",
    b"Circle",
    b"FileAttachment",
    b"FreeText",
    b"Highlight",
    b"Ink",
    b"Line",
    b"Link",
    b"Movie",
    b"Polygon",
    b"PolyLine",
    b"Popup",
    b"PrinterMark",
    b"Redact",
    b"Screen",
    b"Sound",
    b"Square",
    b"Squiggly",
    b"Stamp",
    b"StrikeOut",
    b"Text",
    b"TrapNet",
    b"Underline",
    b"Watermark",
    b"Widget",
];

/// The two types ISO 32000-2 12.5.6.1 adds, which part 4 is defined on.
const STANDARD_TYPES_2_0: &[&[u8]] = &[b"Projection", b"RichMedia"];

/// The types ISO 19005-1 6.5.2 excludes.
///
/// Everything ISO 32000-1 defines that PDF 1.4 did not, plus the three
/// multimedia types 1.4 had. The Isartor suite tests eleven of these by name
/// and every one of them is here.
const FORBIDDEN_PART_ONE: &[&[u8]] = &[
    b"3D",
    b"Caret",
    b"FileAttachment",
    b"Movie",
    b"Polygon",
    b"PolyLine",
    b"Redact",
    b"Screen",
    b"Sound",
    b"Watermark",
];

/// The types ISO 19005-2/3 6.3.1 excludes.
const FORBIDDEN_PART_TWO: &[&[u8]] = &[b"3D", b"Movie", b"Screen", b"Sound"];

/// The types ISO 19005-4 6.3.1 excludes at the plain level.
const FORBIDDEN_PART_FOUR: &[&[u8]] = &[
    b"3D",
    b"FileAttachment",
    b"Movie",
    b"RichMedia",
    b"Screen",
    b"Sound",
];

/// Annotations that need no appearance stream, whatever else they need.
///
/// Stated by the corpus rather than derived: `6-3-3-t01-pass-b` is a `Popup`
/// with no appearance dictionary and `-pass-c` is a `Link`, both annotated
/// pass under parts 2 and 4. A popup is drawn by its parent and a link is
/// drawn by the reader's own chrome, so neither carries an appearance.
const NO_APPEARANCE_NEEDED: &[&[u8]] = &[b"Link", b"Popup"];

/// The value of `dict`'s `key`, when it is a name.
fn name(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Arc<[u8]>> {
    doc.resolve_key(dict, doc.intern(key))
        .as_name()
        .and_then(|name| doc.name_bytes(name))
}

/// Runs every annotation rule that applies to `flavour`, on one dictionary.
///
/// Called from the syntax group's object walk, which reaches every dictionary
/// in the file — so an annotation no page references is judged too. The clause
/// is about what the file contains rather than about what a page draws, which
/// is the opposite of 6.3's "used for rendering" qualifier and is why this
/// group is not a visitor over the content walk.
pub(super) fn rules(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    let Some(flavour) = flavour else {
        return;
    };
    // `/Type /Annot` and nothing looser. An annotation dictionary may omit
    // `/Type` and readers cope, but a rule that guessed from `/Subtype` and
    // `/Rect` would judge dictionaries that are not annotations at all, and
    // what guessing wrong costs here is a conforming file reported.
    match name(doc, dict, b"Type") {
        Some(kind) if kind.as_ref() == b"Annot" => {}
        _ => return,
    }
    let subtype = name(doc, dict, b"Subtype");
    let subtype = subtype.as_deref();

    annotation_type(doc, flavour, subtype, dict, at, out);
    flags(doc, subtype, dict, at, out);
    opacity(doc, dict, at, out);
    appearance(doc, flavour, subtype, dict, at, out);
}

/// ISO 19005-1 6.5.2, ISO 19005-2/3 6.3.1, ISO 19005-4 6.3.1.
///
/// **Part 4's two levels are permissions rather than levels.** Level E is the
/// engineering level and exists to carry 3D artwork and rich media; level F
/// exists to carry attachments. Each permits exactly what its own fixtures say
/// it permits and nothing more, so `6-3-1-t01-pass-c` (a `3D` under 4E) and
/// `6-3-1-t01-pass-a` (a `FileAttachment` under 4F) are silent while the same
/// subtypes under plain part 4 are findings.
fn annotation_type(
    doc: &CosDocument,
    flavour: Flavour,
    subtype: Option<&[u8]>,
    dict: &Dict,
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    let Some(subtype) = subtype else {
        // No `/Subtype` at all. ISO 32000-1 12.5.2 makes it required, and a
        // type nobody stated is not one the reference specification defines.
        out.push(Raw {
            rule: clauses::ANNOTATION_TYPES,
            object: Some(at),
            kind: FindingKind::AnnotationTypeNonStandard {
                subtype: String::new(),
            },
        });
        return;
    };

    let mut standard = STANDARD_TYPES.contains(&subtype);
    if flavour.part == Part::Four {
        standard = standard || STANDARD_TYPES_2_0.contains(&subtype);
    }
    if !standard {
        out.push(Raw {
            rule: clauses::ANNOTATION_TYPES,
            object: Some(at),
            kind: FindingKind::AnnotationTypeNonStandard {
                subtype: String::from_utf8_lossy(subtype).into_owned(),
            },
        });
        return;
    }

    let forbidden: &[&[u8]] = match flavour.part {
        Part::One => FORBIDDEN_PART_ONE,
        Part::Two | Part::Three => FORBIDDEN_PART_TWO,
        Part::Four => FORBIDDEN_PART_FOUR,
    };
    let permitted_by_level: &[&[u8]] = match (flavour.part, flavour.level) {
        (Part::Four, Some(Level::E)) => &[b"3D", b"RichMedia"],
        (Part::Four, Some(Level::F)) => &[b"FileAttachment"],
        _ => &[],
    };
    if forbidden.contains(&subtype) && !permitted_by_level.contains(&subtype) {
        out.push(Raw {
            rule: clauses::ANNOTATION_TYPES,
            object: Some(at),
            kind: FindingKind::AnnotationTypeForbidden {
                subtype: String::from_utf8_lossy(subtype).into_owned(),
            },
        });
        return;
    }

    // Level E admits 3D artwork and then says what the artwork may be. The
    // subject is the *stream* the annotation points at rather than the
    // annotation, and it is the one rule in this group that follows a
    // reference out of the dictionary it was handed.
    if subtype == b"3D" && matches!(flavour.level, Some(Level::E)) {
        let artwork = doc.resolve_key(dict, doc.intern(b"3DD"));
        if let Some(stream) = artwork.as_stream() {
            let format = name(doc, &stream.dict, b"Subtype");
            let declared = format.as_deref().unwrap_or_default();
            if declared != b"U3D" && declared != b"PRC" {
                out.push(Raw {
                    rule: clauses::ANNOTATION_TYPES,
                    object: Some(at),
                    kind: FindingKind::ThreeDArtworkFormat {
                        declared: String::from_utf8_lossy(declared).into_owned(),
                    },
                });
            }
        }
    }
}

/// The `/F` flag word: ISO 19005-1 6.5.3, ISO 19005-2/3 6.3.2, ISO 19005-4
/// 6.3.2.
///
/// `/F` must be present, its Print bit set, and its Hidden, Invisible, NoView
/// and ToggleNoView bits clear. An archival file is one that prints the same
/// way everywhere, and every one of those bits is a way of making what is seen
/// depend on which reader is looking.
///
/// **`Popup` is exempt from the whole requirement**, and that is the corpus's
/// statement rather than a reading of ours: `6-3-2-t01-pass-b` is titled "The
/// F key is missing in a Popup annotation dictionary" and is annotated
/// **pass**, under part 2 and again under part 4. A popup is opened by its
/// parent rather than printed on its own.
fn flags(doc: &CosDocument, subtype: Option<&[u8]>, dict: &Dict, at: ObjRef, out: &mut Vec<Raw>) {
    if subtype == Some(&b"Popup"[..]) {
        return;
    }
    let Some(flags) = doc.resolve_key(dict, doc.intern(b"F")).as_int() else {
        out.push(Raw {
            rule: clauses::ANNOTATION_DICTS,
            object: Some(at),
            kind: FindingKind::AnnotationFlagsMissing,
        });
        return;
    };
    // ISO 32000-1 table 165, counted from 1 at the low end: bit 1 Invisible,
    // 2 Hidden, 3 Print, 4 NoZoom, 5 NoRotate, 6 NoView, 7 ReadOnly, 8 Locked,
    // **9 ToggleNoView**, 10 LockedContents. Nine rather than ten is the one
    // number here that was wrong first: `6-3-2-t02-fail-e` writes `/F 268`,
    // which is bits 3, 4 and 9, and a rule reading bit 10 found nothing to say
    // about a file the suite annotates fail.
    for (bit, flag, required) in [
        (1u32, "Invisible", false),
        (2, "Hidden", false),
        (3, "Print", true),
        (6, "NoView", false),
        (9, "ToggleNoView", false),
    ] {
        let set = flags & (1 << (bit - 1)) != 0;
        if set != required {
            out.push(Raw {
                rule: clauses::ANNOTATION_DICTS,
                object: Some(at),
                kind: FindingKind::AnnotationFlag { flag, set },
            });
        }
    }
}

/// `/CA` is an annotation's constant opacity, and the clause requires 1.0.
///
/// The same tolerance the transparency rule uses, for the same reason a corpus
/// fixture taught it there: a producer that wrote a real number meaning "fully
/// opaque" and landed one part in ten million away has not made the annotation
/// transparent, and an exact comparison reports it.
fn opacity(doc: &CosDocument, dict: &Dict, at: ObjRef, out: &mut Vec<Raw>) {
    let Some(alpha) = doc.resolve_key(dict, doc.intern(b"CA")).as_number() else {
        return;
    };
    if (alpha - 1.0).abs() > super::colour::ALPHA_TOLERANCE {
        out.push(Raw {
            rule: clauses::ANNOTATION_DICTS,
            object: Some(at),
            kind: FindingKind::AnnotationNotOpaque,
        });
    }
}

/// The `/AP` dictionary: ISO 19005-1 6.5.3, ISO 19005-2/3 6.3.3, ISO 19005-4
/// 6.3.3.
///
/// Four requirements, each its own finding, because "the appearance is wrong"
/// is not something a caller can act on:
///
/// 1. every annotation that needs one has an `/AP`;
/// 2. the `/AP` carries an `/N` and **nothing else** — a `/D` or an `/R` is a
///    second appearance, and which of them a reader shows depends on the
///    reader;
/// 3. `/N` is a stream;
/// 4. except on a push-button widget, where `/N` is a sub-dictionary of states
///    and `/AS` says which is current. `6-3-3-t02-pass-a` and `-fail-b` are
///    that pair and they differ by nothing else.
///
/// **Three exemptions, every one of them the corpus's own.** `Popup` and
/// `Link` need no appearance at all; part 4 adds `Projection`; and an
/// annotation whose `/Rect` encloses no area needs none either —
/// `6-3-3-t01-pass-a` is a `Text` annotation with no `/AP` whose title says
/// "value 1 is equal to value 3 and value 2 is equal to value 4". A rectangle
/// with no area displays nothing, so there is nothing for an appearance to
/// say.
fn appearance(
    doc: &CosDocument,
    flavour: Flavour,
    subtype: Option<&[u8]>,
    dict: &Dict,
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    let exempt = subtype.is_some_and(|name| {
        NO_APPEARANCE_NEEDED.contains(&name)
            || (flavour.part == Part::Four && name == b"Projection")
    });

    let appearances = doc.resolve_key(dict, doc.intern(b"AP"));
    let Some(states) = appearances.as_dict() else {
        if !exempt && !rect_is_a_point(doc, dict) {
            out.push(Raw {
                rule: clauses::ANNOTATION_APPEARANCES,
                object: Some(at),
                kind: FindingKind::AnnotationAppearanceMissing,
            });
        }
        return;
    };

    for (key, _) in states.entries() {
        let Some(key) = doc.name_bytes(*key) else {
            continue;
        };
        if key.as_ref() != b"N" {
            out.push(Raw {
                rule: clauses::ANNOTATION_APPEARANCES,
                object: Some(at),
                kind: FindingKind::AnnotationAppearanceExtraState {
                    key: String::from_utf8_lossy(&key).into_owned(),
                },
            });
        }
    }

    let normal = doc.resolve_key(states, doc.intern(b"N"));
    if normal.is_null() {
        out.push(Raw {
            rule: clauses::ANNOTATION_APPEARANCES,
            object: Some(at),
            kind: FindingKind::AnnotationAppearanceMissing,
        });
        return;
    }

    // A push-button widget's appearance is one state per button state, so `/N`
    // is a dictionary there and a stream everywhere else. The two are not
    // interchangeable and the corpus asserts it in both directions.
    let is_button = subtype == Some(&b"Widget"[..])
        && field_type(doc, dict).is_some_and(|kind| kind.as_ref() == b"Btn");
    if is_button {
        if normal.as_stream().is_some() || normal.as_dict().is_none() {
            out.push(Raw {
                rule: clauses::ANNOTATION_APPEARANCES,
                object: Some(at),
                kind: FindingKind::AnnotationAppearanceNotStates,
            });
        }
    } else if normal.as_stream().is_none() {
        out.push(Raw {
            rule: clauses::ANNOTATION_APPEARANCES,
            object: Some(at),
            kind: FindingKind::AnnotationAppearanceNotAStream,
        });
    }
}

/// The field type governing a widget annotation, which may be inherited.
///
/// ISO 32000-1 12.7.3.1 makes `/FT` inheritable through `/Parent`, and a
/// radio button is exactly the case that uses it: each kid widget carries the
/// appearance states and the parent field carries the `/FT /Btn`. Reading
/// `/FT` off the widget alone reported `6-4-1-t01-pass-b`, a conforming part 4
/// file whose two radio widgets have their `/FT` one level up.
///
/// Bounded rather than followed to the end, because a `/Parent` chain in an
/// untrusted file can be a cycle and ruling 1 forbids a rule that never
/// returns. Four is past any real field tree's depth.
fn field_type(doc: &CosDocument, dict: &Dict) -> Option<Arc<[u8]>> {
    const MAX_FIELD_DEPTH: usize = 4;
    let mut current = doc.resolve_key(dict, doc.intern(b"FT"));
    if let Some(kind) = current.as_name().and_then(|kind| doc.name_bytes(kind)) {
        return Some(kind);
    }
    let mut parent = doc.resolve_key(dict, doc.intern(b"Parent"));
    for _ in 0..MAX_FIELD_DEPTH {
        let field = parent.as_dict()?;
        current = doc.resolve_key(field, doc.intern(b"FT"));
        if let Some(kind) = current.as_name().and_then(|kind| doc.name_bytes(kind)) {
            return Some(kind);
        }
        parent = doc.resolve_key(field, doc.intern(b"Parent"));
    }
    None
}

/// Whether the annotation's `/Rect` is a single point.
///
/// **Both pairs, not either**, and the corpus is what settles it. The
/// conforming fixture `6-3-3-t01-pass-a` writes `/Rect [50 110 50 110]` and
/// its title says "value 1 is equal to value 3 **and** value 2 is equal to
/// value 4"; `6-3-3-t01-fail-p` writes `[50 600 50 50]`, which is zero *width*
/// and 550 units tall, and the suite annotates it fail. So a rectangle with no
/// area is not the exemption — a rectangle that is a point is. Reading it as
/// "no area" passed a file the clause fails, which is the direction that costs
/// a real defect rather than a false positive, and it is still wrong.
fn rect_is_a_point(doc: &CosDocument, dict: &Dict) -> bool {
    let rect = doc.resolve_key(dict, doc.intern(b"Rect"));
    let Some(values) = rect.as_array() else {
        // No `/Rect` at all is a different defect and not this rule's, and it
        // is not a licence to skip the appearance requirement either.
        return false;
    };
    if values.len() != 4 {
        return false;
    }
    let mut corners = [0.0f64; 4];
    for (slot, value) in corners.iter_mut().zip(values) {
        let Some(number) = doc.resolve(value).as_number() else {
            return false;
        };
        *slot = number;
    }
    corners[0] == corners[2] && corners[1] == corners[3]
}
