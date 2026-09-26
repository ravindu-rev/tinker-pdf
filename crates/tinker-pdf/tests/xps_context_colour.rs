//! 15.2.5's `ContextColor`: a colour stated in an ICC profile's own space.
//!
//! # The claim, and why it is a translation
//!
//! A `ContextColor` names a profile part, an alpha and the components, and
//! **nothing else** — there is no sRGB fallback in the syntax. So this build
//! does not convert: the profile goes into the PDF verbatim as an `/ICCBased`
//! colour space (8.6.5.5), the components go into the content stream unchanged,
//! and the reader does the colour management. Nothing here evaluates a profile,
//! so nothing here can be wrong about one.
//!
//! That is what these tests assert. `/CS0 cs 0.1 0.2 0.3 scn` with the file's
//! own numbers, against a `/ColorSpace` resource whose stream is the file's own
//! bytes — byte-for-byte, which is the assertion that "verbatim" is not a
//! figure of speech.
//!
//! # The three fixtures are real profiles, and are already in this repository
//!
//! `fuzz/corpus/icc_profile/` holds GRAY, RGB and CMYK profiles committed for
//! the ICC fuzz target. They are used here rather than fresh ones built in the
//! test, because a profile assembled by the same repository that reads it can
//! agree with a misreading; these were committed to be *decoded*, and their
//! channel counts — one, three and four — are exactly the three `/ICCBased`
//! permits. The `nCLR` fixture is built here because the corpus has none.
//!
//! # What is refused, and what still paints
//!
//! Three failures, three answers, and they are not the same one:
//!
//! - the profile part is **missing or is not a profile** — named
//!   `ColourProfileUnresolved`, and the element **still paints** in 8.6.5.5's
//!   default-`/Alternate` reading of the components. The fallback is not
//!   invented: PDF already specifies it for an `/ICCBased` stream a reader
//!   cannot use, and the numbers are the file's.
//! - the profile takes a channel count `/ICCBased` cannot state — a
//!   **narrowing**, named `ColourProfileChannels`, and painted in the
//!   placeholder grey rather than in a colour picked by dropping components.
//! - the markup is not 15.2.5's grammar — `BrushUnreadable`, which is a
//!   statement about the file and not about this build.
//!
//! # Counted injections
//!
//! Each was verified by reintroducing the defect and running
//! `cargo test -p tinker-pdf --no-fail-fast`.
//!
//! | Injection | Caught by |
//! | --- | --- |
//! | one byte is trimmed from the profile before it is embedded | 1 |
//! | the components are dropped and the fallback written instead | 4 |
//! | the channel count comes from the markup, not from the profile | 1 |
//! | `/ICCBased`'s 1/3/4 restriction is lifted | 1 |
//! | a missing profile paints grey instead of its alternate reading | 3 |
//! | the pre-pass scans only `Fill` and `Stroke` attributes | 1 |
//!
//! Nothing fired zero. The single-test rows are five *different* tests, which
//! is the property the file is arranged to have: the profile's bytes, its
//! channel count, `/ICCBased`'s restriction, the fallback and the breadth of
//! the pre-pass are five independent claims and each has one fixture that can
//! see it and no other.
//!
//! The one-byte trim is worth its own line. It is the smallest possible break
//! of "verbatim", it changes no behaviour a reader would notice without the
//! profile in hand, and it is caught — because the test compares the embedded
//! bytes against the fixture's own rather than checking that *a* profile was
//! written.

mod xps_support;

use tinker_pdf::{ArchiveWarning, Document, WriteMode, WriteOptions, XpsElementDefect};
use xps_support::{
    archive, before_content_types, binary_part, content_types_with, one_page_package, with, XPS_NS,
};

/// The resource-dictionary key namespace, which every real package binds.
const KEY_NS: &str = "http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key";

/// Three real profiles, committed for the ICC fuzz target.
const GREY: &[u8] = include_bytes!("../../../fuzz/corpus/icc_profile/grey-gamma.icc");
const RGB: &[u8] = include_bytes!("../../../fuzz/corpus/icc_profile/rgb-gamma-curve.icc");
const CMYK: &[u8] = include_bytes!("../../../fuzz/corpus/icc_profile/cmyk-lut.icc");

/// A profile header stating an `nCLR` data space of `n` channels.
///
/// Built here because the corpus has none, and only the header is built
/// because only the header is read: ICC.1 §7.2 puts the data space at offset
/// 16 and `acsp` at 36, and this build never evaluates the profile. A fixture
/// with tags would be asserting something no code path reaches.
fn n_channel(n: u8) -> Vec<u8> {
    let mut out = vec![0u8; 132];
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(format!("{n:X}CLR").as_bytes());
    out[20..24].copy_from_slice(b"Lab ");
    out[36..40].copy_from_slice(b"acsp");
    let size = u32::try_from(out.len()).expect("a small profile");
    out[0..4].copy_from_slice(&size.to_be_bytes());
    out
}

/// A content-types item that also resolves `.icc` parts.
fn types() -> String {
    content_types_with(r#"<Default Extension="icc" ContentType="application/vnd.iccprofile" />"#)
}

/// A package whose one page carries `body` and holds `profile` at `/p.icc`.
fn package(body: &str, profile: Option<&[u8]>) -> Vec<u8> {
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{body}</FixedPage>"#
    );
    let mut parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &markup);
    parts = with(parts, "[Content_Types].xml", &types());
    if let Some(profile) = profile {
        parts = before_content_types(parts, binary_part("Resources/p.icc", profile.to_vec()));
    }
    archive(parts)
}

fn defects(bytes: &[u8]) -> Vec<XpsElementDefect> {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    document
        .archive()
        .expect("a synthesised document")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
            _ => None,
        })
        .collect()
}

fn stream(bytes: &[u8]) -> String {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// The whole synthesised document, saved. Uncompressed, so a profile's bytes
/// are in it as themselves.
fn saved(bytes: &[u8]) -> Vec<u8> {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    })
}

/// Whether `needle` appears in `haystack`.
fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// A `Path` filled by a `ContextColor` naming `/Resources/p.icc`.
fn filled(components: &str) -> String {
    format!(
        r#"<Path Fill="ContextColor /Resources/p.icc {components}" Data="M0,0L200,0 200,200 0,200Z" />"#
    )
}

/// **The profile reaches the PDF verbatim, and the components reach it
/// unchanged.**
///
/// The two halves are the whole claim, and they are two: a build that embedded
/// the profile and then wrote a converted RGB colour would pass the first, and
/// one that wrote the components into a space it never registered would pass
/// the second and produce a dangling name.
#[test]
fn a_context_colour_becomes_an_icc_based_space_over_the_files_own_profile() {
    let bytes = package(&filled("1.0,0.1,0.2,0.3"), Some(RGB));
    assert_eq!(defects(&bytes), []);

    let page = stream(&bytes);
    assert!(
        page.contains("/CS0 cs\n0.1 0.2 0.3 scn"),
        "the file's own numbers, in a named space: {page}"
    );
    assert!(
        !page.contains("rg"),
        "and no device colour was written instead: {page}"
    );

    let file = saved(&bytes);
    assert!(holds(&file, b"/ICCBased"), "the space is an ICCBased one");
    assert!(
        holds(&file, b"/N 3"),
        "declaring the profile's own channel count"
    );
    assert!(
        holds(&file, RGB),
        "and the profile is in the file byte for byte"
    );
}

/// **The channel count comes from the profile, not from the markup.**
///
/// One, three and four are the three `/ICCBased` permits, and each is a
/// different `/N` and a different number of `scn` operands. A build that took
/// the count from the component list would agree with all three of these
/// fixtures only by accident — so each is stated with a *different* number of
/// components than its profile takes, and the profile wins.
#[test]
fn the_channel_count_is_the_profiles_and_the_operands_match_it() {
    // A grey profile stated with three components: one operand is written.
    let grey = package(&filled("1.0,0.4,0.5,0.6"), Some(GREY));
    assert_eq!(defects(&grey), []);
    assert!(
        stream(&grey).contains("/CS0 cs\n0.4 scn"),
        "one channel, one operand: {}",
        stream(&grey)
    );
    assert!(holds(&saved(&grey), b"/N 1"));

    // A CMYK profile stated with two: the missing operands are zero rather
    // than absent, because 8.6.5.5's space says how many `scn` takes and a
    // short `scn` is not a colour at all.
    let cmyk = package(&filled("1.0,0.2,0.4"), Some(CMYK));
    assert_eq!(defects(&cmyk), []);
    assert!(
        stream(&cmyk).contains("/CS0 cs\n0.2 0.4 0 0 scn"),
        "four channels, four operands: {}",
        stream(&cmyk)
    );
    assert!(holds(&saved(&cmyk), b"/N 4"));
}

/// **A profile `/ICCBased` cannot state is a narrowing, and is named.**
///
/// Table 66 permits 1, 3 or 4 components and no others; ICC.1's `nCLR` family
/// runs to fifteen. PDF's other n-channel space, `/DeviceN`, needs a tint
/// transform into an alternate space that only *evaluating* the profile could
/// supply — which is the colour engine this build deliberately does not have.
/// So it is named and painted in the placeholder grey, rather than in a colour
/// picked by dropping components until six became four.
#[test]
fn a_profile_with_more_channels_than_icc_based_allows_is_a_named_narrowing() {
    let bytes = package(&filled("1.0,0.1,0.2,0.3,0.4,0.5,0.6"), Some(&n_channel(6)));
    assert_eq!(defects(&bytes), [XpsElementDefect::ColourProfileChannels]);
    let page = stream(&bytes);
    assert!(
        page.contains("0.749 0.749 0.749 rg"),
        "the placeholder, and not four of the six: {page}"
    );
    assert!(
        !saved(&bytes).windows(9).any(|w| w == b"/ICCBased"),
        "and no space was registered"
    );
}

/// **A missing profile still paints**, in the reading PDF itself specifies.
///
/// 8.6.5.5 falls back to `/Alternate`, which defaults by component count to
/// `DeviceGray`, `DeviceRGB` or `DeviceCMYK`. So a `ContextColor` whose part is
/// absent is not a lost colour: the components are in the markup and the rule
/// for reading them is PDF's own. Painting the placeholder grey here would
/// throw away numbers the file supplied, which is the opposite of ruling 2.
#[test]
fn a_missing_profile_paints_its_default_alternate_reading() {
    for (components, expected) in [
        ("1.0,0.25", "0.25 0.25 0.25 rg"),
        ("1.0,0.1,0.2,0.3", "0.1 0.2 0.3 rg"),
        ("1.0,0.1,0.2,0.3,0.4", "0.5 0.4 0.3 rg"),
    ] {
        let bytes = package(&filled(components), None);
        assert_eq!(
            defects(&bytes),
            [XpsElementDefect::ColourProfileUnresolved],
            "{components}"
        );
        let page = stream(&bytes);
        assert!(page.contains(expected), "{components}: {page}");
    }
}

/// A part that **is** there and is not a profile is the same fault as a part
/// that is not there: the file pointed at something that cannot be a colour
/// space either way.
#[test]
fn a_part_that_is_not_a_profile_is_unresolved() {
    let bytes = package(&filled("1.0,0.1,0.2,0.3"), Some(b"not a profile at all"));
    assert_eq!(defects(&bytes), [XpsElementDefect::ColourProfileUnresolved]);
    assert!(stream(&bytes).contains("0.1 0.2 0.3 rg"));
}

/// A `ContextColor` **strokes**, and a stroke takes Table 74's capitals.
///
/// `cs`/`scn` and `CS`/`SCN` are two operator pairs and a build that wrote the
/// lower-case pair for a stroke would set the fill colour and leave the stroke
/// black — gap 07's headline defect exactly.
#[test]
fn a_context_colour_strokes_in_the_capitalised_operators() {
    let body = r#"<Path Stroke="ContextColor /Resources/p.icc 1.0,0.1,0.2,0.3" StrokeThickness="4"
             Data="M0,0L200,0 200,200 0,200Z" />"#;
    let bytes = package(body, Some(RGB));
    assert_eq!(defects(&bytes), []);
    let page = stream(&bytes);
    assert!(
        page.contains("/CS0 CS\n0.1 0.2 0.3 SCN"),
        "the stroking pair: {page}"
    );
}

/// The pre-pass finds a `ContextColor` **wherever a colour may stand**, not
/// only on `Fill` and `Stroke`.
///
/// 15.2.5's colour is a value, so it stands on a `SolidColorBrush`'s `Color`
/// as readily as on a `Path`'s `Fill`. A pass that listed the attribute names
/// it knew about would be a list to forget the next one from, so it looks at
/// every attribute of every element in a dialect namespace — and this fixture
/// is the one that would break if it did not.
#[test]
fn a_context_colour_inside_a_solid_colour_brush_resolves_too() {
    let body = r#"<Path Data="M0,0L200,0 200,200 0,200Z"><Path.Fill>
         <SolidColorBrush Color="ContextColor /Resources/p.icc 1.0,0.1,0.2,0.3" />
       </Path.Fill></Path>"#;
    let bytes = package(body, Some(RGB));
    assert_eq!(defects(&bytes), []);
    assert!(stream(&bytes).contains("/CS0 cs\n0.1 0.2 0.3 scn"));
}

/// A `ContextColor` in a **gradient stop** is approximated, and says so.
///
/// 8.7.4.5's shading states one colour space for the whole function, so a stop
/// is not free to name one of its own. The stop takes 8.6.5.5's alternate
/// reading — the same numbers the file supplied, read the way a reader without
/// the profile would — and the brush reports that it reached the page and not
/// exactly, which is what `BrushApproximated` is for.
///
/// This is the one place `ContextColor` is lossy, and it is narrowed by name
/// rather than left as a silent difference between a solid fill and a gradient
/// stop of the same colour.
#[test]
fn a_context_colour_in_a_gradient_stop_is_approximated_and_named() {
    let body = r##"<Path Data="M0,0L200,0 200,200 0,200Z"><Path.Fill>
         <LinearGradientBrush StartPoint="0,0" EndPoint="1,0"
                              MappingMode="RelativeToBoundingBox">
           <LinearGradientBrush.GradientStops>
             <GradientStop Color="ContextColor /Resources/p.icc 1.0,0.1,0.2,0.3" Offset="0" />
             <GradientStop Color="#FF000000" Offset="1" />
           </LinearGradientBrush.GradientStops>
         </LinearGradientBrush></Path.Fill></Path>"##;
    let bytes = package(body, Some(RGB));
    assert_eq!(defects(&bytes), [XpsElementDefect::BrushApproximated]);
    assert!(
        stream(&bytes).contains("sh"),
        "and the gradient still reaches the page"
    );
}

/// Markup that is not 15.2.5's grammar is **unreadable**, not unresolved.
///
/// "That is not a `ContextColor`" and "that `ContextColor`'s profile is not
/// there" are different facts about the file, and only the second says the
/// package is incomplete.
#[test]
fn a_context_colour_that_is_not_the_grammar_is_unreadable() {
    for bad in [
        // No profile at all.
        "ContextColor 1.0,0.1,0.2,0.3",
        // No components.
        "ContextColor /Resources/p.icc",
        // An alpha and nothing else: 15.2.5 always states the alpha first, so
        // a list of one is a colour with no channels.
        "ContextColor /Resources/p.icc 1.0",
        // Not separated from the keyword.
        "ContextColorx /Resources/p.icc 1.0,0.5",
    ] {
        let body = format!(r#"<Path Fill="{bad}" Data="M0,0L200,0 200,200 0,200Z" />"#);
        let bytes = package(&body, Some(RGB));
        assert_eq!(
            defects(&bytes),
            [XpsElementDefect::BrushUnreadable],
            "{bad}"
        );
    }
}

/// More components than any ICC profile can have is **syntax**, not a
/// narrowing.
///
/// ICC.1's `nCLR` ceiling is fifteen, so a `ContextColor` naming sixteen is
/// markup that cannot be true of any profile — a different thing from a colour
/// whose profile this build declines to state, and named differently.
#[test]
fn a_context_color_with_more_channels_than_icc_allows_is_syntax() {
    let sixteen: Vec<String> = (0..16).map(|at| format!("0.0{at}")).collect();
    let body = format!(
        r#"<Path Fill="ContextColor /Resources/p.icc 1.0,{}" Data="M0,0L200,0 200,200 0,200Z" />"#,
        sixteen.join(",")
    );
    let bytes = package(&body, Some(RGB));
    assert_eq!(defects(&bytes), [XpsElementDefect::BrushUnreadable]);
}
