//! Milestone 8: the plan's refusal list, end to end, entry by entry.
//!
//! **The refusal path is the feature.** A wrong JPEG 2000 decode looks like a
//! photograph: tier-2 hands tier-1 a byte range, tier-1 hands the wavelet
//! coefficients, and the inverse wavelet is a smoothing operator, so a
//! decoder that skips what it does not understand produces a soft plausible
//! picture where the real image was a radiograph or a map, and nothing
//! downstream can tell. The placeholder is honest; that picture is not.
//!
//! So the list is not a default — it is enumerated, and this is where the
//! enumeration is checked. Every entry the plan names goes through
//! [`jpx_decode`] itself rather than through an internal parser, because the
//! claim being made is about the public boundary: `Unsupported(Jpx)`, so the
//! caller draws the placeholder (ruling 2), and exactly one [`Warning`]
//! naming the class (ruling 10).
//!
//! The counterpart claim is [`every_jpx_warning_is_reachable`]: a variant no
//! decode can produce is a public surface describing a distinction it does
//! not draw. Three were in that state before this milestone.

use crate::jpx::codestream::{cb_style, marker};
use crate::jpx::{jpx_decode, Refusal};
use crate::{Capability, FilterError, Limits, Warning};

use super::writer::{boxed, segment, tile_part, Spec, SIGNATURE};

/// The default fixture's two empty packets: one per resolution of a 4 x 4
/// image at one decomposition level.
const EMPTY_PACKETS: [u8; 2] = [0x00, 0x00];

fn refuse(bytes: &[u8]) -> Warning {
    let mut warnings = Vec::new();
    let got = jpx_decode(bytes, &Limits::new(1 << 20), &mut warnings);
    assert_eq!(
        got,
        Err(FilterError::Unsupported(Capability::Jpx)),
        "this refusal did not reach the caller as the named capability, so \
         the placeholder is not what gets drawn"
    );
    assert_eq!(
        warnings.len(),
        1,
        "a refusal leaves exactly one warning: {warnings:?}"
    );
    warnings[0]
}

/// A codestream whose main header is `Spec` and whose one tile carries
/// `packets`.
fn stream(spec: &Spec, packets: &[u8]) -> Vec<u8> {
    spec.codestream(&[(0, packets)])
}

/// Every entry of the plan's refusal list, reached and named.
///
/// The list is transcribed from
/// `docs/plans/gaps/18a-jpx-decoder.md`'s "Where a half-implementation is
/// worse than none" section, in its order, so that a reader can put the two
/// side by side. An entry that stops being reachable — because the feature
/// got built, or because a check moved — fails here rather than quietly
/// leaving the list a description of an older decoder.
#[test]
fn every_entry_of_the_refusal_list_is_reachable_and_named() {
    // "a progression order not implemented" — all five of B.12's are, so what
    // is left is a value Table A.16 does not define at all.
    assert_eq!(
        refuse(&stream(
            &Spec {
                progression: 5,
                ..Spec::default()
            },
            &EMPTY_PACKETS
        )),
        Warning::JpxFeatureUnsupported,
    );

    // "and any POC marker, which changes the order mid-stream"
    assert_eq!(
        refuse(&with_marker(marker::POC, &[0; 7])),
        Warning::JpxMarkerUnsupported,
    );

    // "any code-block style bit in COD/COC Table A.19 this build does not
    // implement" — five of the six. The sixth, segmentation symbols, is
    // implemented on purpose, because checking them is the only free
    // integrity check the format offers.
    for bit in [
        cb_style::BYPASS,
        cb_style::RESET,
        cb_style::TERMALL,
        cb_style::VERTICALLY_CAUSAL,
        cb_style::PREDICTABLE,
    ] {
        assert_eq!(
            refuse(&stream(
                &Spec {
                    cb_style: bit,
                    ..Spec::default()
                },
                &EMPTY_PACKETS
            )),
            Warning::JpxFeatureUnsupported,
            "Table A.19 bit {bit:#04x}",
        );
    }
    // And a bit Table A.19 does not define at all.
    assert_eq!(
        refuse(&stream(
            &Spec {
                cb_style: 0x40,
                ..Spec::default()
            },
            &EMPTY_PACKETS
        )),
        Warning::JpxFeatureUnsupported,
    );

    // "an RGN marker, a Part 2 marker, or a marker the standard defines and
    // this build does not". Three separate claims and three cases: RGN and
    // CRG are Table A.2's, and 0xFF74 is ISO/IEC 15444-2's MCT — Part 2 is a
    // non-goal, and every Part 2 marker lands in the same place, as a code
    // Table A.2 does not define.
    assert_eq!(
        refuse(&with_marker(marker::RGN, &[0, 0, 0])),
        Warning::JpxMarkerUnsupported,
    );
    assert_eq!(
        refuse(&with_marker(marker::CRG, &[0, 0])),
        Warning::JpxMarkerUnsupported,
    );
    assert_eq!(
        refuse(&with_marker(0xFF74, &[0])),
        Warning::JpxMarkerUnknown,
    );

    // "tile-parts out of order, or a tile whose parts do not cover it".
    let spec = Spec::default();
    let mut out_of_order = spec.main_header();
    out_of_order.extend_from_slice(&tile_part(0, 1, 2, &[], &EMPTY_PACKETS));
    out_of_order.extend_from_slice(&tile_part(0, 0, 2, &[], &[]));
    out_of_order.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(refuse(&out_of_order), Warning::JpxStructureInvalid);

    let mut uncovered = spec.main_header();
    uncovered.extend_from_slice(&tile_part(0, 0, 3, &[], &EMPTY_PACKETS));
    uncovered.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(refuse(&uncovered), Warning::JpxStructureInvalid);

    // "a packet whose declared length does not land where the next packet
    // begins" — the first of the two *integrity* refusals, and the one that
    // fires before a single coefficient exists. One byte too many in the
    // tile's data is a tile whose packets do not consume it.
    assert_eq!(
        refuse(&stream(&spec, &[0x00, 0x00, 0x00])),
        Warning::JpxPacketLength,
    );

    // "a `colr` method that cannot be mapped, when /ColorSpace is absent".
    assert_eq!(
        refuse(&jp2_with_colr(&[9, 0, 0])),
        Warning::JpxFeatureUnsupported
    );
    // ...and an enumerated space this build has no name for. 17 is greyscale
    // and 16 is sRGB; 42 is nothing.
    assert_eq!(
        refuse(&jp2_with_colr(&[1, 0, 0, 0, 0, 0, 42])),
        Warning::JpxFeatureUnsupported,
    );

    // "component precision above 16 bits" — refused, not truncated.
    assert_eq!(
        refuse(&stream(
            &Spec {
                components: vec![(24, false, 1, 1)],
                ..Spec::default()
            },
            &EMPTY_PACKETS
        )),
        Warning::JpxPrecisionUnsupported,
    );

    // "or a component count the colour pipeline cannot interpret".
    assert_eq!(
        refuse(&stream(
            &Spec {
                components: vec![(8, false, 1, 1); 65],
                ..Spec::default()
            },
            &EMPTY_PACKETS
        )),
        Warning::JpxFeatureUnsupported,
    );
    // And its sibling, which is what `Refusal::NotBuilt` used to be: three
    // channels at 8, 8 and 12 bits, which one output precision has no answer
    // for. Refused as the capability it is rather than as a missing stage,
    // since every stage now exists.
    assert_eq!(
        refuse(&stream(
            &Spec {
                components: vec![(8, false, 1, 1), (8, false, 1, 1), (12, false, 1, 1)],
                ..Spec::default()
            },
            &[0x00; 6]
        )),
        Warning::JpxFeatureUnsupported,
    );

    // "any budget in the previous section being spent."
    assert_eq!(
        refuse(&stream(
            &Spec {
                xsiz: 4096,
                ysiz: 4096,
                xtsiz: 4096,
                ytsiz: 4096,
                components: vec![(8, false, 1, 1); 64],
                ..Spec::default()
            },
            &EMPTY_PACKETS
        )),
        Warning::JpxBudgetSpent,
    );

    // Not on the plan's list, and the parser's own: a codestream that ends
    // inside something it was still reading. It is a refusal class of its
    // own because "the file stops here" and "this build cannot do that" are
    // different answers to a reader.
    let mut truncated = spec.main_header();
    truncated.truncate(truncated.len() - 3);
    assert_eq!(refuse(&truncated), Warning::JpxTruncated);

    // The list's remaining entry is the *second* integrity refusal — a
    // cleanup pass that did not end with T.800 D.5's `1010` — and it is
    // reached from `tier1::tests` rather than here, by
    // `a_segmentation_symbol_catches_a_decoder_out_of_step`, because it is
    // the one refusal a header cannot produce: it needs a code-block whose
    // MQ data is actually decoded and then corrupted. What is checked here
    // is that the refusal reaches the boundary as its own warning, which
    // `every_jpx_warning_is_reachable` below does.
}

/// Every JPX warning this crate defines can be produced by decoding
/// something.
///
/// A `Warning` variant nothing can emit is a public surface claiming a
/// distinction it does not draw, and three were in that state until this
/// milestone: `JpxStageNotBuilt`, which named a decoder stage after the last
/// stage was built, and `JpxPacketLength` and `JpxSegmentationSymbol`, whose
/// refusals were reporting themselves as `JpxStructureInvalid`. That mapping
/// lost the distinction at exactly the boundary where it is worth something —
/// a capability refusal says a better build would draw this picture, an
/// integrity refusal says no build should.
///
/// The set is transcribed here a second time rather than iterated, for the
/// reason the Table A.2 test gives: a list derived from the enum agrees with
/// the enum whatever is in it.
///
/// **What this proves and what it does not.** Nine of the ten are refusals,
/// and every one of them is a `Refusal` variant's `warning()` — which is the
/// only route by which it can arise — with
/// `every_entry_of_the_refusal_list_is_reachable_and_named` above showing
/// each of those refusals coming out of a real decode. The tenth,
/// `JpxCoefficientClamped`, is the one *leniency*: `jpx_decode` pushes it
/// directly when E.1's dyadic clamp fires, and no fixture in this repository
/// reaches that clamp — every one of them asserts it is **not** reached,
/// because a legitimate 8-bit image's coefficients live three bits below it.
/// So its entry here is a claim about the wiring and not about a decode, and
/// saying so is better than letting the row look like the other nine.
#[test]
fn every_jpx_warning_is_reachable() {
    let expected = [
        Warning::JpxMarkerUnsupported,
        Warning::JpxMarkerUnknown,
        Warning::JpxStructureInvalid,
        Warning::JpxFeatureUnsupported,
        Warning::JpxPrecisionUnsupported,
        Warning::JpxTruncated,
        Warning::JpxPacketLength,
        Warning::JpxSegmentationSymbol,
        Warning::JpxBudgetSpent,
        Warning::JpxCoefficientClamped,
    ];

    // Every `Refusal` variant's `warning()`, which is the only way one of the
    // nine refusal warnings can arise, plus the one leniency.
    let produced = [
        Refusal::Marker("RGN").warning(),
        Refusal::UnknownMarker(0xFF74).warning(),
        Refusal::Structure("").warning(),
        Refusal::Feature("").warning(),
        Refusal::Precision(24).warning(),
        Refusal::Truncated("").warning(),
        Refusal::Budget("").warning(),
        Refusal::PacketLength.warning(),
        Refusal::SegmentationSymbol.warning(),
        Warning::JpxCoefficientClamped,
    ];

    for want in expected {
        assert!(
            produced.contains(&want),
            "{want:?} is a JPX warning nothing can emit. Either a refusal \
             should be reporting it, or it should not be in the public set."
        );
    }
    assert_eq!(
        expected.len(),
        produced.len(),
        "the two lists have drifted: a refusal class was added without a \
         warning, or the other way round"
    );
}

/// A refusal is recorded at most once however many times it is provoked.
///
/// Ruling 10 keeps the warning set closed and deduplicated, so a stream of a
/// million bad markers cannot turn leniency into an allocation attack.
#[test]
fn a_decode_leaves_at_most_one_warning() {
    let bytes = with_marker(marker::RGN, &[0, 0, 0]);
    let mut warnings = Vec::new();
    let _ = jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings);
    let _ = jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings);
    assert_eq!(warnings, vec![Warning::JpxMarkerUnsupported]);
}

/// The default main header with one extra marker segment before EOC.
fn with_marker(code: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Spec::default().main_header();
    out.extend_from_slice(&segment(code, body));
    out.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    out.extend_from_slice(&marker::EOC.to_be_bytes());
    out
}

/// A JP2 file whose `colr` box carries `body`.
fn jp2_with_colr(colr: &[u8]) -> Vec<u8> {
    let spec = Spec::default();
    let mut out = SIGNATURE.to_vec();
    out.extend_from_slice(&boxed(*b"ftyp", b"jp2 \x00\x00\x00\x00jp2 "));

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&spec.ysiz.to_be_bytes());
    ihdr.extend_from_slice(&spec.xsiz.to_be_bytes());
    ihdr.extend_from_slice(&1u16.to_be_bytes());
    ihdr.push(7);
    ihdr.extend_from_slice(&[7, 0, 0]);

    let mut jp2h = boxed(*b"ihdr", &ihdr);
    jp2h.extend_from_slice(&boxed(*b"colr", colr));
    out.extend_from_slice(&boxed(*b"jp2h", &jp2h));
    out.extend_from_slice(&boxed(*b"jp2c", &stream(&spec, &EMPTY_PACKETS)));
    out
}
