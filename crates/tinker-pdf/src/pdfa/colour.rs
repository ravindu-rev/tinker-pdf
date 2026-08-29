//! The colour rule group: ISO 19005-1 6.2.2, 6.2.3, 6.2.9 and 6.4, and their
//! siblings in parts 2 to 4 (milestone 5 of `docs/design/pdfa.md`).
//!
//! # The clause this group is really about
//!
//! ISO 19005 does not forbid `DeviceRGB`. It forbids a file whose colours
//! cannot be reproduced, which is a different and much more specific thing: a
//! device colour space names a value with no meaning until something says what
//! device it is a value *for*, and the output intent is that something. So the
//! rule is a relation between two facts that live at opposite ends of the
//! file — an operator in a content stream, and an ICC profile embedded in the
//! catalog — and neither half is a finding on its own.
//!
//! That shape is why this group reaches for two machineries, both counted as
//! [`super::RuleGroup::Colour`]: [`super::content`]'s walk, for what the pages
//! actually paint with, and `tinker-pdf-color`'s ICC reader, for what the
//! destination profile says it is a profile *of*.
//!
//! # Where ICC staging actually falls, which is not where it was expected
//!
//! `docs/design/pdfa.md` staged "colour rules that need to look inside a
//! profile" behind `docs/design/icc.md`. That design has since landed
//! `icc::Profile::parse`, so the header fields these rules need — the data
//! colour space signature, and through it the channel count — are available
//! and are read. What is **not** available is a profile *validator*:
//! `Profile::parse` exists to build a colour transform and refuses a profile
//! it cannot build one from, including v4 profiles whose only route to the
//! connection space is an `mAB ` tag. Refusing to render is right; refusing to
//! conform is not, and a rule that reported every such profile would report
//! conforming files. So a destination profile this build cannot read makes the
//! colour space of the intent **unknown** rather than wrong, the rules that
//! depend on knowing it do not fire, and [`super::STAGED`] names the refusal.
//!
//! # Injections, counted, of the workspace's 3 599 tests
//!
//! Each of the three readings the corpus forced on this group was reverted and
//! the suite counted, because a rule the corpus taught and no test protects is
//! a rule the next change will quietly undo:
//!
//! - a transparency group's blending space no longer excusing a device space:
//!   **1** test, and thirteen conforming corpus files back in the
//!   false-positive column;
//! - an exact comparison of a constant alpha against one, instead of the
//!   tolerance: **1** test, and one corpus file;
//! - part 4's page-level output intents not read: **1** test, and three corpus
//!   files;
//! - the group entered without checking the request, which is the laziness
//!   requirement rather than a rule: **2** tests.

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_color::icc;
use tinker_pdf_cos::{pages, CosDocument, Dict, ObjRef, Object};

use super::{clauses, content, FindingKind, Flavour, Machinery, Part, Raw, RuleGroup};

/// How many pages the `/Group` sweep visits.
const MAX_PAGES: usize = 1 << 14;

/// How many output intents are read.
const MAX_INTENTS: usize = 64;

/// The output-intent subtype ISO 19005 gives its own intent, in every part.
///
/// The name is `GTS_PDFA1` in parts 2, 3 and 4 as well as in part 1 — the
/// suffix is part of the registered name rather than a version number, and a
/// build that looked for `GTS_PDFA2` would find nothing in any conforming
/// file.
const PDFA_OUTPUT_INTENT: &[u8] = b"GTS_PDFA1";

/// The four rendering intents ISO 32000-1 8.6.5.8 defines.
const RENDERING_INTENTS: &[&[u8]] = &[
    b"AbsoluteColorimetric",
    b"RelativeColorimetric",
    b"Perceptual",
    b"Saturation",
];

/// How far from 1 a constant alpha may be and still mean opaque.
///
/// Not a nicety and not invented here: `6-4-t03-pass-b.pdf` writes `/CA
/// 1.0000001` and `/ca 0.9999999` and is annotated `pass`, with a title that
/// says in those words what it is testing. A producer that wrote a real number
/// meaning "fully opaque" and landed one part in ten million away has not made
/// the file transparent, and a rule with an exact comparison reports it.
const ALPHA_TOLERANCE: f64 = 1e-5;

/// The blend modes ISO 19005-1 6.4 leaves a part 1 file.
///
/// Part 1 has no transparency, so the only blend modes it admits are the two
/// that mean "do not blend". `/Compatible` is 11.3.5's deprecated spelling of
/// `/Normal` and is admitted for the same reason.
const OPAQUE_BLEND_MODES: &[&[u8]] = &[b"Normal", b"Compatible"];

/// Runs every colour rule that applies to `flavour`.
pub(super) fn rules(
    doc: &CosDocument,
    machinery: &Machinery,
    flavour: Option<Flavour>,
    out: &mut Vec<Raw>,
) {
    if !machinery.reach(RuleGroup::Colour) {
        return;
    }
    let part = flavour.map(|f| f.part);
    let destination = output_intents(doc, part, out);
    let mut used = Used::default();
    scan(doc, &mut used);
    gather_state_intents(doc, &mut used);
    gather_group_spaces(doc, &mut used);
    device_spaces(&destination, &used, out);
    icc_spaces(doc, &used, out);
    rendering_intents(&used, out);
    if part == Some(Part::One) {
        transparency(doc, &used, out);
    }
}

// ---- 6.2.2 / 6.2.3 The output intent --------------------------------------

/// What the file's PDF/A output intent says its destination device is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Destination {
    /// No output intent with `/S /GTS_PDFA1`.
    Absent,
    /// One, whose profile declares this data colour space (the ICC signature
    /// at header offset 16).
    Space([u8; 4]),
    /// One, whose profile this build could not read — see the module note on
    /// why that is not the same as a profile that is wrong.
    Unreadable,
}

/// ISO 19005-1 6.2.2, and its siblings.
///
/// Three requirements this build reads out of the clause, and the third is the
/// interesting one:
///
/// 1. `/OutputIntents`, when present, is an array of dictionaries;
/// 2. an entry whose `/S` is `/GTS_PDFA1` carries a `/DestOutputProfile` that
///    is a stream, and an `/OutputConditionIdentifier` (ISO 32000-1 14.11.5
///    Table 365 makes the identifier required of every output intent);
/// 3. if there is more than one such entry, **they all name the same
///    profile** — a file with two answers to "what device is this for" has
///    given none.
fn output_intents(doc: &CosDocument, part: Option<Part>, out: &mut Vec<Raw>) -> Destination {
    // Part 4's page-level fallback is applied at every exit, not only at the
    // end: the first version returned early when the catalog carried no
    // `/OutputIntents` at all, which is exactly the shape of the file the
    // fallback exists for. Three conforming fixtures were reported for it.
    let page_level = || {
        if part == Some(Part::Four) {
            page_destination(doc)
        } else {
            Destination::Absent
        }
    };

    let Some(catalog) = doc.catalog() else {
        return page_level();
    };
    let catalog_ref = doc.trailer().get_ref(doc.intern(b"Root"));
    let intents = doc.resolve_key(&catalog, doc.intern(b"OutputIntents"));
    if intents.is_null() {
        return page_level();
    }
    let Some(entries) = intents.as_array() else {
        out.push(Raw {
            rule: clauses::OUTPUT_INTENT,
            object: catalog_ref,
            kind: FindingKind::OutputIntentMalformed {
                key: "OutputIntents".to_string(),
            },
        });
        return page_level();
    };

    let mut profiles: BTreeSet<ObjRef> = BTreeSet::new();
    let mut destination = Destination::Absent;
    for entry in entries.iter().take(MAX_INTENTS) {
        let at = entry.as_objref().or(catalog_ref);
        let resolved = doc.resolve(entry);
        let Some(intent) = resolved.as_dict() else {
            out.push(Raw {
                rule: clauses::OUTPUT_INTENT,
                object: at,
                kind: FindingKind::OutputIntentMalformed {
                    key: "OutputIntents".to_string(),
                },
            });
            continue;
        };
        if name_of(doc, intent, b"S").as_deref() != Some(PDFA_OUTPUT_INTENT) {
            // ISO 32000-1 14.11.5 admits other output intents — `GTS_PDFX`,
            // `ISO_PDFE1` — beside the PDF/A one, and this clause is about the
            // PDF/A one. Judging the others would be enforcing a standard the
            // file did not claim.
            continue;
        }
        if doc
            .resolve_key(intent, doc.intern(b"OutputConditionIdentifier"))
            .as_string()
            .is_none()
        {
            out.push(Raw {
                rule: clauses::OUTPUT_INTENT,
                object: at,
                kind: FindingKind::OutputIntentMalformed {
                    key: "OutputConditionIdentifier".to_string(),
                },
            });
        }

        let Some(profile_ref) = intent.get_ref(doc.intern(b"DestOutputProfile")) else {
            out.push(Raw {
                rule: clauses::OUTPUT_INTENT,
                object: at,
                kind: FindingKind::DestOutputProfileMissing,
            });
            continue;
        };
        profiles.insert(profile_ref);
        destination = profile_space(doc, profile_ref);
    }

    if profiles.len() > 1 {
        out.push(Raw {
            rule: clauses::OUTPUT_INTENT,
            object: catalog_ref,
            kind: FindingKind::OutputIntentsDisagree,
        });
    }

    // **ISO 19005-4 moved the output intent onto the page.** Part 4's 6.2.3
    // admits an `/OutputIntents` array in a *page* dictionary as well as in
    // the catalog, and `6-2-3-t05-pass-a.pdf` is exactly that file: no
    // catalog-level intent, a PDF/A intent on the page, annotated `pass` with
    // a title saying so. Parts 1 to 3 have no such provision, so the page
    // sweep runs for part 4 alone rather than for whichever part happens to
    // have a page.
    //
    // Only the *destination* is taken from a page-level intent. The shape
    // rules above are not re-run over it: what part 4 requires of a
    // page-level intent is a reading this build has not established, and
    // reporting against a guess is what `super::STAGED` exists to avoid.
    if destination == Destination::Absent {
        destination = page_level();
    }

    destination
}

/// The destination profile of a part 4 page-level output intent, if there is
/// one.
fn page_destination(doc: &CosDocument) -> Destination {
    for page in pages::collect_upto(doc, MAX_PAGES) {
        let Ok(object) = doc.get(page.reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let intents = doc.resolve_key(dict, doc.intern(b"OutputIntents"));
        let Some(entries) = intents.as_array() else {
            continue;
        };
        for entry in entries.iter().take(MAX_INTENTS) {
            let resolved = doc.resolve(entry);
            let Some(intent) = resolved.as_dict() else {
                continue;
            };
            if name_of(doc, intent, b"S").as_deref() != Some(PDFA_OUTPUT_INTENT) {
                continue;
            }
            if let Some(profile) = intent.get_ref(doc.intern(b"DestOutputProfile")) {
                return profile_space(doc, profile);
            }
        }
    }
    Destination::Absent
}

/// The data colour space of the profile behind `reference`.
///
/// The reach into `tinker-pdf-color`. A profile that will not parse is
/// [`Destination::Unreadable`] rather than a finding, for the reason the module
/// note gives: this parser refuses profiles it cannot build a transform from,
/// and "cannot render" is not "does not conform".
fn profile_space(doc: &CosDocument, reference: ObjRef) -> Destination {
    let Ok(bytes) = doc.stream_decoded(reference) else {
        return Destination::Unreadable;
    };
    match icc::Profile::parse(&bytes) {
        Ok(profile) => Destination::Space(profile.space),
        Err(_) => Destination::Unreadable,
    }
}

// ---- what the pages paint with --------------------------------------------

/// What one walk of the content streams found, for every colour rule at once.
#[derive(Default)]
struct Used {
    /// Device colour spaces painted with, and one object that did.
    devices: BTreeMap<&'static str, Option<ObjRef>>,
    /// Device colour spaces a `/Default…` entry in the resources standing over
    /// them excuses (ISO 32000-1 8.6.5.6).
    defaulted: BTreeSet<&'static str>,
    /// `ICCBased` streams the pages selected.
    icc_streams: BTreeSet<ObjRef>,
    /// Rendering intents named, by the `ri` operator or an `/ExtGState`.
    intents: BTreeMap<Vec<u8>, Option<ObjRef>>,
    /// Extended graphics states the pages selected.
    ext_g_states: BTreeSet<ObjRef>,
    /// Form XObjects the pages invoked.
    forms: BTreeSet<ObjRef>,
    /// Device-independent blending colour spaces a transparency group named,
    /// by the device family each stands in for (11.6.6).
    group_spaces: BTreeSet<&'static str>,
}

/// The three uncalibrated spaces, spelled as the operators and the names spell
/// them.
const DEVICE_GRAY: &str = "DeviceGray";
const DEVICE_RGB: &str = "DeviceRGB";
const DEVICE_CMYK: &str = "DeviceCMYK";

/// One walk, filling [`Used`].
fn scan(doc: &CosDocument, used: &mut Used) {
    content::walk(doc, &mut |op| {
        match op.operator {
            // 8.6.8: the colour operators that select a device space and a
            // value in it, in one operator.
            b"g" | b"G" => {
                used.devices.entry(DEVICE_GRAY).or_insert(None);
            }
            b"rg" | b"RG" => {
                used.devices.entry(DEVICE_RGB).or_insert(None);
            }
            b"k" | b"K" => {
                used.devices.entry(DEVICE_CMYK).or_insert(None);
            }
            // 8.6.8: `cs` names a space, either one of the four built-in names
            // or a resource.
            b"cs" | b"CS" => {
                let Some(name) = op.first_name() else {
                    return;
                };
                if let Some(space) = device_name(name) {
                    used.devices.entry(space).or_insert(None);
                    return;
                }
                let Some(resources) = op.resources else {
                    return;
                };
                colour_space_resource(doc, resources, name, used);
            }
            b"ri" => {
                if let Some(name) = op.first_name() {
                    used.intents.entry(name.to_vec()).or_insert(None);
                }
            }
            b"gs" => {
                let (Some(name), Some(resources)) = (op.first_name(), op.resources) else {
                    return;
                };
                if let Some(reference) = content::lookup(doc, resources, b"ExtGState", name) {
                    used.ext_g_states.insert(reference);
                }
            }
            b"Do" => {
                let (Some(name), Some(resources)) = (op.first_name(), op.resources) else {
                    return;
                };
                let Some(reference) = content::lookup(doc, resources, b"XObject", name) else {
                    return;
                };
                let Ok(object) = doc.get(reference) else {
                    return;
                };
                let Some(stream) = object.as_stream() else {
                    return;
                };
                match name_of(doc, &stream.dict, b"Subtype").as_deref() {
                    Some(b"Form") => {
                        used.forms.insert(reference);
                    }
                    Some(b"Image") => {
                        image_space(doc, &stream.dict, reference, used);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        // 8.6.5.6: a `/Default…` entry in the resources' `/ColorSpace`
        // sub-dictionary replaces the device space it is named for, which is
        // the escape hatch the clause itself offers. Recorded per walk rather
        // than per operator scope, which is a simplification and named as one:
        // a `/DefaultRGB` on one page excuses `DeviceRGB` on all of them here,
        // where the clause scopes it to the resource dictionary it is in.
        if let Some(resources) = op.resources {
            let spaces = doc.resolve_key(resources, doc.intern(b"ColorSpace"));
            if let Some(spaces) = spaces.as_dict() {
                for (key, space) in [
                    (&b"DefaultGray"[..], DEVICE_GRAY),
                    (b"DefaultRGB", DEVICE_RGB),
                    (b"DefaultCMYK", DEVICE_CMYK),
                ] {
                    if spaces.contains_key(doc.intern(key)) {
                        used.defaulted.insert(space);
                    }
                }
            }
        }
    });
}

/// The device space a built-in colour-space name denotes, if it is one.
fn device_name(name: &[u8]) -> Option<&'static str> {
    match name {
        b"DeviceGray" | b"G" => Some(DEVICE_GRAY),
        b"DeviceRGB" | b"RGB" => Some(DEVICE_RGB),
        b"DeviceCMYK" | b"CMYK" => Some(DEVICE_CMYK),
        _ => None,
    }
}

/// A colour space named in the resources: a device name, an `ICCBased` array,
/// or one of the families whose base or alternate space is what matters.
fn colour_space_resource(doc: &CosDocument, resources: &Dict, name: &[u8], used: &mut Used) {
    let spaces = doc.resolve_key(resources, doc.intern(b"ColorSpace"));
    let Some(spaces) = spaces.as_dict() else {
        return;
    };
    let value = doc.resolve_key(spaces, doc.intern(name));
    let at = spaces.get_ref(doc.intern(name));
    colour_space(doc, &value, at, used, 0);
}

/// How deep a colour space's base or alternate chain is followed.
///
/// `/Indexed` over `/Separation` over `/DeviceN` over an alternate is legal and
/// nests; a file that nests it a thousand deep is not describing a colour.
const MAX_SPACE_DEPTH: u32 = 8;

/// One colour space object, recorded for whichever rule is about it.
fn colour_space(
    doc: &CosDocument,
    value: &Object,
    at: Option<ObjRef>,
    used: &mut Used,
    depth: u32,
) {
    if depth > MAX_SPACE_DEPTH {
        return;
    }
    match value {
        Object::Name(name) => {
            if let Some(space) = doc.name_bytes(*name).and_then(|n| device_name(&n)) {
                used.devices.entry(space).or_insert(at);
            }
        }
        Object::Array(members) => {
            let Some(family) = members
                .first()
                .and_then(|first| doc.resolve(first).as_name())
                .and_then(|name| doc.name_bytes(name))
            else {
                return;
            };
            match family.as_ref() {
                b"ICCBased" => {
                    if let Some(stream) = members.get(1).and_then(Object::as_objref) {
                        used.icc_streams.insert(stream);
                    }
                }
                // 8.6.6.3 and 8.6.6.4: the value a `/Separation` or `/DeviceN`
                // produces lives in its **alternate** space, and 8.6.6.2 says
                // the same of an `/Indexed` space's base. So a `/Separation`
                // over `DeviceCMYK` uses `DeviceCMYK`, which is the reading
                // that makes 6.2.3.4's fixtures make sense.
                b"Separation" => {
                    if let Some(alternate) = members.get(2) {
                        let resolved = doc.resolve(alternate);
                        colour_space(doc, &resolved, at, used, depth + 1);
                    }
                }
                b"DeviceN" => {
                    if let Some(alternate) = members.get(2) {
                        let resolved = doc.resolve(alternate);
                        colour_space(doc, &resolved, at, used, depth + 1);
                    }
                }
                b"Indexed" | b"I" => {
                    if let Some(base) = members.get(1) {
                        let resolved = doc.resolve(base);
                        colour_space(doc, &resolved, at, used, depth + 1);
                    }
                }
                b"Pattern" => {
                    if let Some(under) = members.get(1) {
                        let resolved = doc.resolve(under);
                        colour_space(doc, &resolved, at, used, depth + 1);
                    }
                }
                _ => {
                    if let Some(space) = device_name(&family) {
                        used.devices.entry(space).or_insert(at);
                    }
                }
            }
        }
        _ => {}
    }
}

/// An image XObject's `/ColorSpace`, which is a colour space like any other.
fn image_space(doc: &CosDocument, dict: &Dict, at: ObjRef, used: &mut Used) {
    // 8.9.5.2: an image mask has no colour space, it has a stencil. Reading
    // one as `DeviceGray` would report every stencil in the corpus.
    if doc
        .resolve_key(dict, doc.intern(b"ImageMask"))
        .as_bool()
        .unwrap_or(false)
    {
        return;
    }
    let value = doc.resolve_key(dict, doc.intern(b"ColorSpace"));
    colour_space(doc, &value, Some(at), used, 0);
}

/// The blending colour space of every transparency group the file renders.
///
/// **The corpus taught this rule, and it is the second stand-in the clause
/// admits.** `6-2-4-3-t04-pass-a.pdf` paints in `DeviceRGB` with no matching
/// output intent and is annotated `pass`; its own title says why — *"the
/// transparency group on page level defines ICCBased RGB colour space"*.
/// 11.6.6 makes a group's `/CS` the space its contents are composited in, so a
/// device-independent one pins down what the device values in it mean, exactly
/// as `/DefaultRGB` does.
///
/// **A simplification, named as one.** A group's `/CS` is recorded for the
/// whole document rather than for the scope it encloses, so an RGB group on
/// page one excuses `DeviceRGB` on page two. The clause scopes it to the group
/// itself. The error is towards accepting, which is the direction a validator
/// should err in when it cannot tell — and `6-2-4-3-t04-pass-i.pdf`, whose
/// page group is `DeviceRGB` and whose form group is `ICCBased` CMYK, is the
/// fixture that shows the difference between the two readings.
fn gather_group_spaces(doc: &CosDocument, used: &mut Used) {
    let mut dicts: Vec<Dict> = Vec::new();
    for page in pages::collect_upto(doc, MAX_PAGES) {
        if let Some(dict) = doc
            .get(page.reference)
            .ok()
            .and_then(|o| o.as_dict().cloned())
        {
            dicts.push(dict);
        }
    }
    for reference in &used.forms {
        if let Some(stream) = doc
            .get(*reference)
            .ok()
            .and_then(|o| o.as_stream().cloned())
        {
            dicts.push(stream.dict);
        }
    }
    for dict in &dicts {
        let group = doc.resolve_key(dict, doc.intern(b"Group"));
        let Some(group) = group.as_dict() else {
            continue;
        };
        if name_of(doc, group, b"S").as_deref() != Some(b"Transparency") {
            continue;
        }
        let space = doc.resolve_key(group, doc.intern(b"CS"));
        if let Some(family) = independent_family(doc, &space) {
            used.group_spaces.insert(family);
        }
    }
}

/// The device family a **device-independent** colour space stands in for, if
/// it is one.
///
/// A device space returns `None` and that is the point: a transparency group
/// whose `/CS` is `DeviceRGB` has not said what the values mean, so it excuses
/// nothing.
fn independent_family(doc: &CosDocument, space: &Object) -> Option<&'static str> {
    let members = space.as_array()?;
    let family = members
        .first()
        .and_then(|first| doc.resolve(first).as_name())
        .and_then(|name| doc.name_bytes(name))?;
    match family.as_ref() {
        b"CalGray" => Some(DEVICE_GRAY),
        b"CalRGB" => Some(DEVICE_RGB),
        b"ICCBased" => {
            let stream = members.get(1).and_then(Object::as_objref)?;
            let object = doc.get(stream).ok()?;
            let components = doc
                .resolve_key(&object.as_stream()?.dict, doc.intern(b"N"))
                .as_int()?;
            match components {
                1 => Some(DEVICE_GRAY),
                3 => Some(DEVICE_RGB),
                4 => Some(DEVICE_CMYK),
                _ => None,
            }
        }
        _ => None,
    }
}

// ---- 6.2.3.3 / 6.2.4.3 Uncalibrated colour spaces -------------------------

/// ISO 19005-1 6.2.3.3, and its siblings.
///
/// **Two readings, named as such**, because the clause distinguishes the three
/// device spaces and this build's reading of how is worth being explicit
/// about:
///
/// - **any** device space needs a PDF/A output intent to be present at all, or
///   a `/Default…` space standing in for it;
/// - `DeviceRGB` additionally needs the destination profile to be an RGB
///   profile and `DeviceCMYK` a CMYK one, while `DeviceGray` is admitted under
///   any of them — a grey value is a value on the neutral axis of whatever
///   device the intent names, and does not need the intent to be grey.
///
/// One finding per space rather than per operator: a page that sets
/// `DeviceRGB` five hundred times has one defect, and five hundred findings
/// saying so is a list nobody reads.
fn device_spaces(destination: &Destination, used: &Used, out: &mut Vec<Raw>) {
    for (space, at) in &used.devices {
        if used.defaulted.contains(space) || used.group_spaces.contains(space) {
            continue;
        }
        match destination {
            Destination::Absent => out.push(Raw {
                rule: clauses::DEVICE_SPACES,
                object: *at,
                kind: FindingKind::DeviceColourWithoutOutputIntent {
                    space: (*space).to_string(),
                },
            }),
            // The profile is there and this build cannot read it, so it cannot
            // say whether the space matches. Staged, not guessed.
            Destination::Unreadable => {}
            Destination::Space(signature) => {
                let admitted = match *space {
                    DEVICE_RGB => signature == b"RGB ",
                    DEVICE_CMYK => signature == b"CMYK",
                    _ => true,
                };
                if !admitted {
                    out.push(Raw {
                        rule: clauses::DEVICE_SPACES,
                        object: *at,
                        kind: FindingKind::DeviceColourNotInOutputIntent {
                            space: (*space).to_string(),
                            profile: String::from_utf8_lossy(signature).trim().to_string(),
                        },
                    });
                }
            }
        }
    }
}

// ---- 6.2.3.2 / 6.2.4.2 ICCBased colour spaces -----------------------------

/// ISO 19005-1 6.2.3.2: an `ICCBased` stream's `/N` says how many components
/// the space has, and it shall agree with the profile.
///
/// The second half is the one that needs the profile's own header, and it is
/// the clearest example of what `docs/design/icc.md` landing bought this
/// group: the channel count follows from the data colour space signature, so a
/// three-component profile declared `/N 4` is a finding rather than a guess. A
/// profile this build cannot read leaves the second half unasked; the first —
/// `/N` present and one of the three values ISO 32000-1 8.6.5.5 admits — is
/// asked either way.
fn icc_spaces(doc: &CosDocument, used: &Used, out: &mut Vec<Raw>) {
    for reference in &used.icc_streams {
        let Ok(object) = doc.get(*reference) else {
            continue;
        };
        let Some(stream) = object.as_stream() else {
            continue;
        };
        let declared = doc.resolve_key(&stream.dict, doc.intern(b"N")).as_int();
        let Some(declared) = declared.filter(|n| matches!(n, 1 | 3 | 4)) else {
            out.push(Raw {
                rule: clauses::ICC_SPACES,
                object: Some(*reference),
                kind: FindingKind::IccStreamMalformed {
                    key: "N".to_string(),
                },
            });
            continue;
        };
        let Ok(bytes) = doc.stream_decoded(*reference) else {
            continue;
        };
        let Ok(profile) = icc::Profile::parse(&bytes) else {
            continue;
        };
        let components = match &profile.space {
            b"GRAY" => 1,
            b"RGB " | b"Lab " | b"XYZ " | b"YCbr" | b"HSV " | b"HLS " | b"CMY " => 3,
            b"CMYK" => 4,
            // A signature this build has not tabulated says nothing about the
            // count, and a rule that assumed one would be reporting its own
            // table rather than the file.
            _ => continue,
        };
        if declared != components {
            out.push(Raw {
                rule: clauses::ICC_SPACES,
                object: Some(*reference),
                kind: FindingKind::IccStreamMalformed {
                    key: "N".to_string(),
                },
            });
        }
    }
}

// ---- 6.2.9 / 6.2.6 Rendering intents --------------------------------------

/// ISO 19005-1 6.2.9: a rendering intent shall be one of the four ISO 32000-1
/// 8.6.5.8 defines.
///
/// Both spellings are gathered by the same walk — the `ri` operator and an
/// `/ExtGState`'s `/RenderingIntent` — because they are the same statement
/// written in two places and a rule that knew only one would pass the other.
fn rendering_intents(used: &Used, out: &mut Vec<Raw>) {
    for (intent, at) in &used.intents {
        if RENDERING_INTENTS.contains(&intent.as_slice()) {
            continue;
        }
        out.push(Raw {
            rule: clauses::RENDERING_INTENTS,
            object: *at,
            kind: FindingKind::RenderingIntentUnknown {
                declared: String::from_utf8_lossy(intent).into_owned(),
            },
        });
    }
}

// ---- 6.4 Transparency, in part 1 only -------------------------------------

/// ISO 19005-1 6.4: a part 1 file has no transparency at all.
///
/// Four ways to have some, and each is its own finding because they are
/// different things to fix: a transparency group on a page or a form, a soft
/// mask in a graphics state, a blend mode that is not one of the two meaning
/// "do not blend", and a constant alpha below one.
///
/// **Parts 2 to 4 permit transparency** and constrain it instead — the group's
/// colour space, the blend modes, the mask — and none of that runs here.
/// `super::STAGED` names it.
fn transparency(doc: &CosDocument, used: &Used, out: &mut Vec<Raw>) {
    for page in pages::collect_upto(doc, MAX_PAGES) {
        let Ok(object) = doc.get(page.reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        if is_transparency_group(doc, dict) {
            out.push(Raw {
                rule: clauses::TRANSPARENCY,
                object: Some(page.reference),
                kind: FindingKind::TransparencyForbidden {
                    feature: "Group".to_string(),
                },
            });
        }
    }

    for reference in &used.forms {
        let Ok(object) = doc.get(*reference) else {
            continue;
        };
        let Some(stream) = object.as_stream() else {
            continue;
        };
        if is_transparency_group(doc, &stream.dict) {
            out.push(Raw {
                rule: clauses::TRANSPARENCY,
                object: Some(*reference),
                kind: FindingKind::TransparencyForbidden {
                    feature: "Group".to_string(),
                },
            });
        }
    }

    for reference in &used.ext_g_states {
        let Ok(object) = doc.get(*reference) else {
            continue;
        };
        let Some(state) = object.as_dict() else {
            continue;
        };
        let mask = doc.resolve_key(state, doc.intern(b"SMask"));
        let none = mask
            .as_name()
            .and_then(|name| doc.name_bytes(name))
            .is_some_and(|name| name.as_ref() == b"None");
        if !mask.is_null() && !none {
            out.push(Raw {
                rule: clauses::TRANSPARENCY,
                object: Some(*reference),
                kind: FindingKind::TransparencyForbidden {
                    feature: "SMask".to_string(),
                },
            });
        }

        // 11.6.3: `/BM` is a name or an array of names, the array being the
        // reader's fallback list. Every member has to be admitted, because a
        // reader that supported the first one would blend.
        let modes = doc.resolve_key(state, doc.intern(b"BM"));
        let mut blend: Vec<Vec<u8>> = Vec::new();
        match modes.as_ref() {
            Object::Name(name) => {
                if let Some(bytes) = doc.name_bytes(*name) {
                    blend.push(bytes.to_vec());
                }
            }
            Object::Array(members) => {
                for member in members.iter().take(MAX_INTENTS) {
                    if let Some(bytes) = doc
                        .resolve(member)
                        .as_name()
                        .and_then(|name| doc.name_bytes(name))
                    {
                        blend.push(bytes.to_vec());
                    }
                }
            }
            _ => {}
        }
        if blend
            .iter()
            .any(|mode| !OPAQUE_BLEND_MODES.contains(&mode.as_slice()))
        {
            out.push(Raw {
                rule: clauses::TRANSPARENCY,
                object: Some(*reference),
                kind: FindingKind::TransparencyForbidden {
                    feature: "BM".to_string(),
                },
            });
        }

        for key in [&b"CA"[..], b"ca"] {
            let alpha = doc.resolve_key(state, doc.intern(key)).as_number();
            if let Some(alpha) = alpha {
                if alpha < 1.0 - ALPHA_TOLERANCE {
                    out.push(Raw {
                        rule: clauses::TRANSPARENCY,
                        object: Some(*reference),
                        kind: FindingKind::TransparencyForbidden {
                            feature: String::from_utf8_lossy(key).into_owned(),
                        },
                    });
                }
            }
        }
    }
}

/// Whether a page or form dictionary carries a transparency group (11.6.6).
fn is_transparency_group(doc: &CosDocument, dict: &Dict) -> bool {
    let group = doc.resolve_key(dict, doc.intern(b"Group"));
    group
        .as_dict()
        .and_then(|group| name_of(doc, group, b"S"))
        .is_some_and(|subtype| subtype == b"Transparency")
}

// ---- shared ---------------------------------------------------------------

/// A dictionary entry that is a name, as bytes.
fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    doc.resolve_key(dict, doc.intern(key))
        .as_name()
        .and_then(|name| doc.name_bytes(name))
        .map(|bytes| bytes.to_vec())
}

/// Whether an `/ExtGState` naming a rendering intent is gathered by the walk.
///
/// Called from [`scan`] through the extended-graphics-state resources, so that
/// [`rendering_intents`] sees both spellings. Kept beside the graphics-state
/// rules rather than inside the walk, because what a state dictionary means is
/// this group's business and not the walker's.
fn gather_state_intents(doc: &CosDocument, used: &mut Used) {
    let states: Vec<ObjRef> = used.ext_g_states.iter().copied().collect();
    for reference in states {
        let Ok(object) = doc.get(reference) else {
            continue;
        };
        let Some(state) = object.as_dict() else {
            continue;
        };
        if let Some(intent) = name_of(doc, state, b"RenderingIntent") {
            used.intents.entry(intent).or_insert(Some(reference));
        }
    }
}
