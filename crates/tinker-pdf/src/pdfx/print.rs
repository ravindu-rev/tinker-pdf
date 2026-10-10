//! The PDF/X print group, and its fonts: what needs the content walk.
//!
//! The output intent (AN 2.16), device colour against it (AN 2.16), the
//! profile device-independent colour needs under PDF/X-3 (AN 2.16),
//! transparency (AN 2.25), PostScript (AN 2.26) and font embedding (AN 2.18).
//!
//! # One walk, PDF/A's
//!
//! Every rule here that asks what a page *does* asks it of
//! [`crate::pdfa::colour::scan`], the walk ISO 19005's colour group already
//! runs — the same visitor, the same caps, the same reading of "used" — and
//! where ISO 15930 asks exactly what ISO 19005-1 asks the PDF/A rule runs and
//! only the clause differs: "no transparency" is
//! [`crate::pdfa::colour::transparency`], and "every font embedded" is
//! [`crate::pdfa::fonts::embedding`]. The design asks that the kernel be
//! shared rather than forked, and a second reading of "no transparency" would
//! be a fork that drifts.

use tinker_pdf_cos::{CosDocument, ObjRef};

use super::{clauses, XFlavour, XRaw};
use crate::pdfa::colour::{self, Used};
use crate::pdfa::{fonts as pdfa_fonts, FindingKind, Machinery, RuleGroup};

/// How many output intents are read.
const MAX_INTENTS: usize = 64;

/// The output-intent subtype ISO 15930 gives its own intent.
const PDFX_OUTPUT_INTENT: &[u8] = b"GTS_PDFX";

/// Runs the print rules for `flavour`; returns whether the group ran.
pub(super) fn rules(
    doc: &CosDocument,
    machinery: &Machinery,
    flavour: XFlavour,
    out: &mut Vec<XRaw>,
) -> bool {
    if !machinery.reach(RuleGroup::Colour) {
        return false;
    }
    let mut used = Used::default();
    colour::scan(doc, &mut used);
    let intent = output_intent(doc, out);
    device_rgb(&intent, &used, out);
    if flavour == XFlavour::X3_2003 {
        independent_colour(&intent, &used, out);
    }

    // AN 2.25, through ISO 19005-1 6.4's own rule: the same five constructs,
    // the same reading of "used", renumbered.
    let mut raw = Vec::new();
    colour::transparency(doc, &used, &mut raw);
    out.extend(raw.into_iter().map(|raw| XRaw {
        rule: clauses::TRANSPARENCY,
        object: raw.object,
        kind: raw.kind,
    }));

    postscript(doc, &used, out);
    true
}

/// Runs the font rule; returns whether the group ran.
///
/// AN 2.18: every font used is embedded, subsets "neither required nor
/// prohibited". "Used" is read as ISO 19005 reads "used for rendering": a font
/// only invisible text (rendering mode 3) is shown in paints nothing, and the
/// notes as quoted do not say whether such a font is used. The reading errs
/// towards silence, the direction a validator with no false-negative bar
/// should err in, and `super::STAGED` names it.
pub(super) fn fonts(doc: &CosDocument, machinery: &Machinery, out: &mut Vec<XRaw>) -> bool {
    if !machinery.reach(RuleGroup::Fonts) {
        return false;
    }
    let mut raw = Vec::new();
    pdfa_fonts::embedding(doc, &mut raw);
    out.extend(raw.into_iter().filter_map(|raw| {
        matches!(raw.kind, FindingKind::FontNotEmbedded { .. }).then_some(XRaw {
            rule: clauses::FONTS,
            object: raw.object,
            kind: raw.kind,
        })
    }));
    true
}

/// What the `GTS_PDFX` output intent says.
struct Intent {
    /// The intent's dictionary is there at all.
    present: bool,
    /// It embeds a destination profile.
    profile: bool,
    /// The profile's data colour space, the ICC signature at header offset
    /// 16, when the header could be read.
    space: Option<[u8; 4]>,
    /// The object a finding about the intent names.
    at: Option<ObjRef>,
}

/// AN 2.16: an output intent with `/S /GTS_PDFX` is required. A
/// characterization in the ICC registry may be named by
/// `/OutputConditionIdentifier` with `/RegistryName`; otherwise
/// `/DestOutputProfile` is required.
///
/// The first `GTS_PDFX` entry is the one judged: the notes as quoted say
/// nothing about a second, and the registry identifier is checked for shape
/// and not looked up — the registry is data with a date on it, and the design
/// makes vendoring it a decision rather than a rule.
fn output_intent(doc: &CosDocument, out: &mut Vec<XRaw>) -> Intent {
    let catalog_at = doc.trailer().get_ref(doc.intern(b"Root"));
    let mut intent = Intent {
        present: false,
        profile: false,
        space: None,
        at: catalog_at,
    };
    let entries = doc
        .catalog()
        .map(|catalog| doc.resolve_key(&catalog, doc.intern(b"OutputIntents")));
    let found = entries.as_deref().and_then(|entries| {
        entries.as_array().and_then(|entries| {
            entries.iter().take(MAX_INTENTS).find_map(|entry| {
                let resolved = doc.resolve(entry);
                let dict = resolved.as_dict()?;
                let subtype = doc
                    .resolve_key(dict, doc.intern(b"S"))
                    .as_name()
                    .and_then(|name| doc.name_bytes(name))?;
                (subtype.as_ref() == PDFX_OUTPUT_INTENT)
                    .then(|| (dict.clone(), entry.as_objref().or(catalog_at)))
            })
        })
    });
    let Some((dict, at)) = found else {
        out.push(XRaw {
            rule: clauses::COLOUR,
            object: catalog_at,
            kind: FindingKind::OutputIntentMissing {
                subtype: "GTS_PDFX".to_string(),
            },
        });
        return intent;
    };
    intent.present = true;
    intent.at = at;

    if let Some(profile) = dict.get_ref(doc.intern(b"DestOutputProfile")) {
        if doc
            .get(profile)
            .is_ok_and(|object| object.as_stream().is_some())
        {
            intent.profile = true;
            // The header alone: the data colour space sits at a fixed offset
            // and needs no transform built, which is the reading
            // `pdfa::colour` came to for the same field.
            intent.space = doc
                .stream_decoded(profile)
                .ok()
                .and_then(|bytes| bytes.get(16..20).and_then(|s| <[u8; 4]>::try_from(s).ok()));
        }
    }
    if !intent.profile {
        let text = |key: &[u8]| {
            doc.resolve_key(&dict, doc.intern(key))
                .as_string()
                .is_some()
        };
        if !text(b"RegistryName") {
            out.push(XRaw {
                rule: clauses::COLOUR,
                object: at,
                kind: FindingKind::DestOutputProfileMissing,
            });
        } else if !text(b"OutputConditionIdentifier") {
            out.push(XRaw {
                rule: clauses::COLOUR,
                object: at,
                kind: FindingKind::OutputIntentMalformed {
                    key: "OutputConditionIdentifier".to_string(),
                },
            });
        }
    }
    intent
}

/// AN 2.16: under a CMYK output intent `DeviceRGB` is not allowed, and must go
/// through a `/DefaultRGB`; the rule reaches the alternate spaces of
/// `Separation`, `DeviceN`, `Indexed` and `Pattern`, which is how the shared
/// walk already records a space.
///
/// Only when the intent's profile says CMYK. An intent that names a registered
/// characterization without embedding a profile does not say what device it
/// is for in any form this build reads, and the rule is silent there rather
/// than guessing from an identifier.
fn device_rgb(intent: &Intent, used: &Used, out: &mut Vec<XRaw>) {
    if intent.space != Some(*b"CMYK") {
        return;
    }
    let Some(at) = used.devices.get(colour::DEVICE_RGB) else {
        return;
    };
    if used.defaulted.contains(colour::DEVICE_RGB) {
        return;
    }
    out.push(XRaw {
        rule: clauses::COLOUR,
        object: *at,
        kind: FindingKind::DeviceColourNotInOutputIntent {
            space: colour::DEVICE_RGB.to_string(),
            profile: "CMYK".to_string(),
        },
    });
}

/// AN 2.16, PDF/X-3 only: device-independent colour anywhere makes the
/// embedded destination profile mandatory, registry or not.
///
/// "Device-independent" is what the walk records as such: an `ICCBased`
/// space, a `CalGray`, `CalRGB` or `Lab` one, or a `/Default…` space standing
/// in for a device space the pages paint with. Reported once, as the missing
/// profile — and not at all when the intent's own shape rule already reported
/// that the profile is missing, which would be one defect said twice.
fn independent_colour(intent: &Intent, used: &Used, out: &mut Vec<XRaw>) {
    if !intent.present || intent.profile {
        return;
    }
    let defaulted = used
        .defaulted
        .iter()
        .any(|space| used.devices.contains_key(space));
    if used.icc_streams.is_empty() && used.independent.is_empty() && !defaulted {
        return;
    }
    let already = out.iter().any(|raw| {
        raw.rule == clauses::COLOUR && raw.kind == FindingKind::DestOutputProfileMissing
    });
    if !already {
        out.push(XRaw {
            rule: clauses::COLOUR,
            object: intent.at,
            kind: FindingKind::DestOutputProfileMissing,
        });
    }
}

/// AN 2.26: no PostScript XObject — by `/Subtype /PS`, or a form's
/// `/Subtype2 /PS` — and no `PS` operator.
///
/// The XObjects are the ones the pages draw, the walk's reading of "used";
/// the operator is the one ISO 32000 no longer defines, which the shared walk
/// already collects among the operators Table A.1 does not list.
fn postscript(doc: &CosDocument, used: &Used, out: &mut Vec<XRaw>) {
    for reference in &used.postscript {
        out.push(XRaw {
            rule: clauses::POSTSCRIPT,
            object: Some(*reference),
            kind: FindingKind::PostScriptXObjectForbidden,
        });
    }
    for reference in &used.forms {
        let Ok(object) = doc.get(*reference) else {
            continue;
        };
        let Some(stream) = object.as_stream() else {
            continue;
        };
        let second = doc
            .resolve_key(&stream.dict, doc.intern(b"Subtype2"))
            .as_name()
            .and_then(|name| doc.name_bytes(name));
        if second.as_deref() == Some(b"PS") {
            out.push(XRaw {
                rule: clauses::POSTSCRIPT,
                object: Some(*reference),
                kind: FindingKind::PostScriptXObjectForbidden,
            });
        }
    }
    if let Some(container) = used.undefined.get(&b"PS"[..]) {
        out.push(XRaw {
            rule: clauses::POSTSCRIPT,
            object: Some(*container),
            kind: FindingKind::PostScriptOperatorForbidden,
        });
    }
}
