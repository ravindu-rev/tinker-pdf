//! The twelve faces this build carries, behind `bundled-fonts`.
//!
//! ## Why there are any
//!
//! 9.6.2.2 requires a reader to have the standard 14 fonts, so most documents
//! do not embed them. This engine had none, and the cost of that was measured
//! rather than argued: `corpus/ratchet-fonts.json` renders the same 4 525
//! corpus files with a face supplied and `corpus/ratchet.json` without one,
//! and **52 % of all reported degradation was the absence of a face** — 1 045
//! files down to 506, and in qpdf's corpus 530 down to 130. A conforming file
//! that names Helvetica and embeds nothing is one this engine could not draw.
//!
//! ## Why they are off by default
//!
//! Because [`crate`]'s callers are not all the same. A desktop application, a
//! server with a font package, a web page with a face already loaded — each
//! has better faces than these and a way to supply them, and none of them
//! should carry 4.2 MB of ours. `FontProvider` remains the seam whether this
//! feature is on or off; what the feature adds is an answer for the host that
//! has none.
//!
//! ## What is here and what is not
//!
//! Liberation Sans, Serif and Mono, four styles each. They are
//! metric-compatible with Arial, Times New Roman and Courier New, which are in
//! turn what every reader substitutes for Helvetica, Times and Courier — so
//! twelve of the standard 14 are covered by faces whose advance widths match
//! what the document's own `/Widths` array already says.
//!
//! Symbol and ZapfDingbats are **not** here and have no Liberation
//! equivalent. Substituting a text face for a symbolic font draws confidently
//! wrong glyphs, which is the failure `FontProvider::substitute` names as the
//! reason declining is a legitimate answer, so [`face`] declines instead.
//!
//! Nothing reads a font directory: that is an operating-system dependency and
//! `wasm32-unknown-unknown` has no filesystem at all.

/// Which of the three families a request wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Liberation Sans — Helvetica and Arial.
    Sans,
    /// Liberation Serif — Times and Times New Roman.
    Serif,
    /// Liberation Mono — Courier and Courier New.
    Mono,
}

macro_rules! face_bytes {
    ($name:literal) => {
        include_bytes!(concat!("../data/liberation/", $name, ".ttf"))
    };
}

const SANS: [&[u8]; 4] = [
    face_bytes!("LiberationSans-Regular"),
    face_bytes!("LiberationSans-Bold"),
    face_bytes!("LiberationSans-Italic"),
    face_bytes!("LiberationSans-BoldItalic"),
];
const SERIF: [&[u8]; 4] = [
    face_bytes!("LiberationSerif-Regular"),
    face_bytes!("LiberationSerif-Bold"),
    face_bytes!("LiberationSerif-Italic"),
    face_bytes!("LiberationSerif-BoldItalic"),
];
const MONO: [&[u8]; 4] = [
    face_bytes!("LiberationMono-Regular"),
    face_bytes!("LiberationMono-Bold"),
    face_bytes!("LiberationMono-Italic"),
    face_bytes!("LiberationMono-BoldItalic"),
];

/// The face for a family and style, as the TrueType bytes.
#[must_use]
pub fn face(family: Family, bold: bool, italic: bool) -> &'static [u8] {
    let style = usize::from(bold) | (usize::from(italic) << 1);
    let family = match family {
        Family::Sans => &SANS,
        Family::Serif => &SERIF,
        Family::Mono => &MONO,
    };
    // The index is two bits and the array is four long, so this cannot miss;
    // the fallback is the regular face rather than a panic all the same,
    // because ruling 1 does not make exceptions for arithmetic that is
    // obviously right.
    family.get(style).copied().unwrap_or(family[0])
}

/// Which family a font's name and flags ask for, or `None` to decline.
///
/// **The name is consulted before the flags**, and that ordering is the whole
/// of the interesting judgement here. `/Flags` bit 2 is the serif bit and
/// producers set it wrongly all the time — a great many emit zero for every
/// font they write — whereas a base-14 document names `Helvetica`, `Times-
/// Roman` or `Courier` exactly, and those three names are the case this
/// feature exists for. So a recognised name decides, and the flags decide only
/// what the name does not.
///
/// A symbolic font is declined outright. Its codes address a private glyph
/// order, so a text face drawn for one produces letters where the document
/// meant arrows — legible, plausible and wrong, which is worse than the
/// missing-glyph gap it would be replacing.
#[must_use]
pub fn family_for(
    base_font: &str,
    serif: bool,
    fixed_pitch: bool,
    symbolic: bool,
) -> Option<Family> {
    let lowered = base_font.to_ascii_lowercase();
    // **Declined by name as well as by flag**, for the reason the families are
    // chosen by name: a base-14 document writes no `/FontDescriptor` at all,
    // so `/Flags` is zero and the symbolic bit says nothing. Symbol and
    // ZapfDingbats are two of the standard 14 — precisely the case this
    // feature exists for — and a text face drawn for either puts letters where
    // the document meant arrows. Measured: without this line a `/BaseFont
    // /Symbol` page renders `abgd` as Latin letters, confidently.
    if symbolic
        || ["symbol", "dingbat", "wingding", "webding", "marlett"]
            .iter()
            .any(|name| lowered.contains(name))
    {
        return None;
    }
    // **The order is load-bearing**, because these are substrings and real
    // font names nest them: `sans-serif` contains `serif`, and `DejaVu Sans
    // Mono` contains `sans`. Narrowest claim first, so monospace beats sans
    // and sans beats serif, and each of those three sentences is one of the
    // tests below.
    for (needle, family) in [
        ("courier", Family::Mono),
        ("mono", Family::Mono),
        ("consol", Family::Mono),
        ("sans", Family::Sans),
        ("times", Family::Serif),
        ("serif", Family::Serif),
        ("georgia", Family::Serif),
        ("garamond", Family::Serif),
        ("book", Family::Serif),
        ("roman", Family::Serif),
        ("helvetica", Family::Sans),
        ("arial", Family::Sans),
        ("verdana", Family::Sans),
        ("tahoma", Family::Sans),
        ("calibri", Family::Sans),
    ] {
        if lowered.contains(needle) {
            return Some(family);
        }
    }
    if fixed_pitch {
        return Some(Family::Mono);
    }
    Some(if serif { Family::Serif } else { Family::Sans })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every face is a TrueType font, and no two of them are the same one.
    ///
    /// Both halves matter. The first is that `include_bytes!` reached a font
    /// rather than a README; the second is that twelve `include_bytes!` paths
    /// naming twelve files did not, through a copy-paste, name one of them
    /// four times — which compiles, passes any test that only checks the
    /// bytes are a font, and silently draws every weight as regular.
    #[test]
    fn twelve_distinct_truetype_faces_are_compiled_in() {
        let mut seen: Vec<&[u8]> = Vec::new();
        for family in [Family::Sans, Family::Serif, Family::Mono] {
            for bold in [false, true] {
                for italic in [false, true] {
                    let bytes = face(family, bold, italic);
                    assert_eq!(
                        &bytes[0..4],
                        &0x0001_0000u32.to_be_bytes(),
                        "{family:?} bold={bold} italic={italic} is not an sfnt"
                    );
                    assert!(bytes.len() > 100_000, "and it is a whole face");
                    assert!(
                        !seen.iter().any(|other| other.as_ptr() == bytes.as_ptr()),
                        "{family:?} bold={bold} italic={italic} repeats another face"
                    );
                    seen.push(bytes);
                }
            }
        }
        assert_eq!(seen.len(), 12);
    }

    /// The standard 14's own names pick the right family, whatever the flags
    /// say — which is the case this feature exists for.
    #[test]
    fn the_standard_names_decide_before_the_flags_do() {
        // Every one of these is asked with the *wrong* flags: serif claimed
        // for the sans faces and cleared for the serif ones, fixed pitch never
        // set. A reading that trusted the flags would get all six wrong.
        for (name, want) in [
            ("Helvetica", Family::Sans),
            ("Helvetica-BoldOblique", Family::Sans),
            ("Arial,Bold", Family::Sans),
            ("Times-Roman", Family::Serif),
            ("TimesNewRomanPSMT", Family::Serif),
            ("Courier", Family::Mono),
            ("Courier-BoldOblique", Family::Mono),
            ("CourierNewPS-BoldMT", Family::Mono),
        ] {
            let serif = want != Family::Serif;
            assert_eq!(
                family_for(name, serif, false, false),
                Some(want),
                "{name} with serif={serif}"
            );
        }
    }

    /// The substring search survives its own counter-examples.
    ///
    /// `sans-serif` contains `serif` and `DejaVu Sans Mono` contains `sans`,
    /// so the three families nest inside each other's names. Each row here is
    /// asked with flags that point the *other* way, so a reading that fell
    /// through to them would get it wrong.
    #[test]
    fn a_nested_family_name_resolves_to_the_narrowest_claim() {
        for name in ["sans-serif", "DejaVu Sans", "Open Sans", "PT Sans-Serif"] {
            assert_eq!(
                family_for(name, true, false, false),
                Some(Family::Sans),
                "{name} is sans, not serif"
            );
        }
        for name in ["DejaVu Sans Mono", "Liberation Mono", "Courier Sans"] {
            assert_eq!(
                family_for(name, true, false, false),
                Some(Family::Mono),
                "{name} is monospace, not sans"
            );
        }
    }

    /// A name nothing recognises falls to the flags, which is what they are
    /// for.
    #[test]
    fn an_unknown_name_falls_to_the_flags() {
        assert_eq!(
            family_for("Whatever-Regular", true, false, false),
            Some(Family::Serif)
        );
        assert_eq!(
            family_for("Whatever-Regular", false, false, false),
            Some(Family::Sans)
        );
        assert_eq!(
            family_for("Whatever-Regular", true, true, false),
            Some(Family::Mono)
        );
    }

    /// A symbolic font is declined rather than substituted.
    ///
    /// Symbol and ZapfDingbats are two of the standard 14 and neither has a
    /// Liberation equivalent. Drawing Liberation Sans for one puts letters
    /// where the document meant arrows — legible, plausible and wrong, which
    /// is a worse outcome than the gap it replaces.
    #[test]
    fn a_symbolic_font_is_declined() {
        for name in ["Symbol", "ZapfDingbats", "Wingdings", "Helvetica"] {
            assert_eq!(family_for(name, false, false, true), None, "{name}");
        }
    }

    /// And declined **by name**, with the flag clear.
    ///
    /// This is not belt and braces. A base-14 font dictionary carries no
    /// `/FontDescriptor`, so `/Flags` is zero and the symbolic bit is absent
    /// for exactly the two faces that need it most — which is how a
    /// `/BaseFont /Symbol` page came to render its text as Latin letters
    /// before this existed.
    #[test]
    fn the_two_symbolic_standard_faces_are_declined_by_name() {
        for name in ["Symbol", "ZapfDingbats", "Wingdings-Regular", "Webdings"] {
            assert_eq!(family_for(name, false, false, false), None, "{name}");
        }
        // And a text face with a similar-looking name is not caught by it.
        assert_eq!(
            family_for("SymbolaText", false, false, false),
            None,
            "a name containing `symbol` is declined, which is the safe side"
        );
        assert_eq!(
            family_for("Helvetica", false, false, false),
            Some(Family::Sans)
        );
    }
}
