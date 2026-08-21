//! `@font-face` and the `src` descriptor (`css-fonts-4` §4.1, §4.3), and the
//! parts of §5.2's matching that are a property of the rule (gap 31,
//! milestone 9).

use super::sheet;
use crate::font_face::{FontFormat, FontSource};
use crate::property::{Declaration, FontStyle, Property};
use crate::{parse, Budget, ImportResolver, Limits, Warning};

/// `@font-face` is read, and the name no longer appears in the unsupported
/// list.
///
/// Two assertions and they are not the same one: a build that parsed the rule
/// and *also* warned would produce a book with its fonts and a report saying
/// they were dropped, and a build that stopped warning without parsing would
/// produce the opposite.
#[test]
fn a_font_face_is_no_longer_an_unsupported_at_rule() {
    let parsed = sheet(r#"@font-face { font-family: "Deja"; src: url(deja.otf) }"#);
    assert_eq!(parsed.report.warnings, vec![], "no warning is left over");
    assert_eq!(parsed.font_faces.len(), 1);
    assert_eq!(parsed.font_faces[0].family, "deja");
}

/// §4.3's list is kept whole and in order, because the entry a build cannot
/// use is the one a modern producer writes first.
#[test]
fn the_src_fallback_list_keeps_every_entry_in_order() {
    let parsed = sheet(
        r#"@font-face { font-family: Deja;
             src: local("Deja Sans"),
                  url("deja.woff2") format("woff2"),
                  url(deja.otf) format(opentype) }"#,
    );
    assert_eq!(parsed.font_faces.len(), 1);
    assert_eq!(
        parsed.font_faces[0].sources,
        vec![
            FontSource::Local("Deja Sans".to_owned()),
            FontSource::Url {
                url: "deja.woff2".to_owned(),
                format: Some(FontFormat::Woff2),
            },
            FontSource::Url {
                url: "deja.otf".to_owned(),
                format: Some(FontFormat::OpenType),
            },
        ]
    );
}

/// `format()` takes a string in `css-fonts-3` and a keyword in `css-fonts-4`,
/// and both are in the wild.
///
/// A build reading only the string would see no hint at all on a sheet written
/// to the newer grammar, which is not the same as an absent `format()` — an
/// absent one means *try the bytes*, and this build does. So the difference
/// only shows as a WOFF2 that gets opened before being refused, which is a
/// defect no page could reveal.
#[test]
fn a_format_hint_is_read_as_a_string_and_as_a_keyword() {
    let quoted = sheet(r#"@font-face { font-family: A; src: url(a.ttf) format("truetype") }"#);
    let bare = sheet("@font-face { font-family: A; src: url(a.ttf) format(truetype) }");
    let hint = |sheet: &crate::Stylesheet| match &sheet.font_faces[0].sources[0] {
        FontSource::Url { format, .. } => format.clone(),
        FontSource::Local(_) => None,
    };
    assert_eq!(hint(&quoted), Some(FontFormat::TrueType));
    assert_eq!(hint(&bare), Some(FontFormat::TrueType));
}

/// A `format()` naming something no specification here lists keeps its text,
/// so a warning one crate up can name it.
#[test]
fn an_unknown_format_hint_keeps_the_word_the_sheet_wrote() {
    let parsed = sheet(r#"@font-face { font-family: A; src: url(a.xx) format("bitmap") }"#);
    let FontSource::Url { format, .. } = &parsed.font_faces[0].sources[0] else {
        panic!("a url source")
    };
    assert_eq!(format.as_ref().map(FontFormat::name), Some("bitmap"));
}

/// §4.1: no `font-family`, or no `src`, and the rule is invalid.
///
/// Counted as a discarded rule rather than dropped silently, and the count is
/// two rather than one — a build that stopped at the first invalid rule would
/// pass an assertion that only checked the count was non-zero.
#[test]
fn a_font_face_without_a_family_or_a_src_is_invalid_and_counted() {
    let parsed = sheet(
        "@font-face { src: url(a.otf) } @font-face { font-family: A } \
         @font-face { font-family: B; src: url(b.otf) }",
    );
    assert_eq!(parsed.report.discarded_rules, 2);
    assert_eq!(parsed.font_faces.len(), 1);
    assert_eq!(parsed.font_faces[0].family, "b");
}

/// An `src` whose every entry is unreadable is an `src` that is not there.
///
/// §4.1 then makes the rule invalid, and the face must not be registered with
/// an empty list: a family declared with no file shadows nothing and a build
/// that kept it would answer "this family exists" to a matching question whose
/// only honest answer is no.
#[test]
fn a_src_that_parses_to_nothing_makes_the_rule_invalid() {
    let parsed = sheet("@font-face { font-family: A; src: 12px }");
    assert_eq!(parsed.font_faces, vec![]);
    assert_eq!(parsed.report.discarded_rules, 1);
}

/// The descriptor takes **one** family, and the property takes a list.
///
/// A build that reused the property's parser here would accept
/// `font-family: A, B` and register the face under one of them; §4.2's grammar
/// is a single `<family-name>` and a list is invalid, which takes the rule with
/// it.
#[test]
fn the_family_descriptor_is_one_name_and_not_a_list() {
    let list = sheet("@font-face { font-family: A, B; src: url(a.otf) }");
    assert_eq!(list.font_faces, vec![], "a list invalidates the descriptor");
    assert_eq!(list.report.discarded_rules, 1);

    let unquoted = sheet("@font-face { font-family: Deja Sans Mono; src: url(a.otf) }");
    assert_eq!(unquoted.font_faces[0].family, "deja sans mono");
}

/// §4.5's weight descriptor: one value is a range whose ends are equal, and
/// two values are the range they say.
#[test]
fn the_weight_descriptor_is_always_a_range() {
    let single = sheet("@font-face { font-family: A; src: url(a.otf); font-weight: bold }");
    assert_eq!(single.font_faces[0].weight, (700, 700));

    let variable = sheet("@font-face { font-family: A; src: url(a.otf); font-weight: 200 900 }");
    assert_eq!(variable.font_faces[0].weight, (200, 900));

    let missing = sheet("@font-face { font-family: A; src: url(a.otf) }");
    assert_eq!(missing.font_faces[0].weight, (400, 400), "§4.5's initial");
}

/// A weight outside §4.5's 1-to-1000 range invalidates the **descriptor** and
/// not the rule.
///
/// The face keeps its family and its `src` and falls back to the initial
/// weight, which is what a browser does; clamping to 1000 instead would make a
/// typo the heaviest face in the book.
#[test]
fn a_weight_outside_the_range_leaves_the_face_at_its_initial() {
    let parsed = sheet("@font-face { font-family: A; src: url(a.otf); font-weight: 5000 }");
    assert_eq!(parsed.font_faces.len(), 1);
    assert_eq!(parsed.font_faces[0].weight, (400, 400));
}

/// §4.6's style descriptor, and an `oblique` angle read past rather than
/// refused.
#[test]
fn the_style_descriptor_reads_past_an_oblique_angle() {
    let plain = sheet("@font-face { font-family: A; src: url(a.otf); font-style: italic }");
    assert_eq!(plain.font_faces[0].style, FontStyle::Italic);

    let angled = sheet("@font-face { font-family: A; src: url(a.otf); font-style: oblique 14deg }");
    assert_eq!(angled.font_faces[0].style, FontStyle::Oblique);
}

/// §5.2's weight distance, at the two facts a single-number test cannot
/// separate.
///
/// **Inside the range is zero**, which is what makes a variable face declared
/// `200 900` answer every request exactly; and **the direction matters**, so a
/// request at 400 with a 300 and a 500 available takes the 300 rather than
/// whichever was declared first. A build with the comparison but not the
/// direction sets a book in bold and passes a test that only checks the
/// nearest number.
#[test]
fn weight_matching_prefers_the_lighter_face_below_the_boundary() {
    let parsed = sheet(
        "@font-face { font-family: A; src: url(l.otf); font-weight: 300 }
         @font-face { font-family: A; src: url(h.otf); font-weight: 500 }
         @font-face { font-family: A; src: url(v.otf); font-weight: 200 900 }",
    );
    let [light, heavy, variable] = &parsed.font_faces[..] else {
        panic!("three faces")
    };
    assert_eq!(variable.weight_distance(400), 0, "inside the range");
    assert!(
        light.weight_distance(400) < heavy.weight_distance(400),
        "at 400 the lighter face wins: {} against {}",
        light.weight_distance(400),
        heavy.weight_distance(400)
    );
    assert!(
        heavy.weight_distance(600) < light.weight_distance(600),
        "above 500 the heavier one does"
    );
}

/// §5.2 makes italic and oblique substitutes for one another and neither a
/// substitute for upright.
#[test]
fn italic_and_oblique_are_nearer_to_each_other_than_to_upright() {
    let parsed = sheet(
        "@font-face { font-family: A; src: url(i.otf); font-style: italic }
         @font-face { font-family: A; src: url(o.otf); font-style: oblique }
         @font-face { font-family: A; src: url(r.otf) }",
    );
    let [italic, oblique, upright] = &parsed.font_faces[..] else {
        panic!("three faces")
    };
    assert_eq!(italic.style_distance(FontStyle::Italic), 0);
    assert_eq!(oblique.style_distance(FontStyle::Italic), 1);
    assert_eq!(upright.style_distance(FontStyle::Italic), 2);
}

/// The family name matches case-insensitively, per §5.2.
#[test]
fn a_family_name_matches_whatever_case_the_property_wrote() {
    let parsed = sheet(r#"@font-face { font-family: "Deja Serif"; src: url(a.otf) }"#);
    assert!(parsed.font_faces[0].matches_family("DEJA SERIF"));
    assert!(!parsed.font_faces[0].matches_family("Deja Sans"));
}

/// A resolver over a fixed table, so an `@import`ed sheet has an address of its
/// own.
struct Table(&'static [(&'static str, &'static str)]);

impl ImportResolver for Table {
    fn resolve(&self, href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
        self.0
            .iter()
            .find(|(name, _)| *name == href)
            .map(|(name, body)| ((*name).to_owned(), body.as_bytes().to_vec()))
    }
}

/// A face declared in an `@import`ed sheet resolves against **that** sheet's
/// address.
///
/// Two facts and both are asserted, because a build that collected the faces
/// but carried the importing sheet's base would find every font in the book
/// and open none of them: the list reaching the caller, and the base being the
/// nested sheet's.
#[test]
fn an_imported_sheets_face_carries_the_imported_sheets_base() {
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let table = Table(&[(
        "css/fonts.css",
        "@font-face { font-family: Deja; src: url(../fonts/deja.otf) }",
    )]);
    let parsed = parse(
        b"@import url(\"css/fonts.css\"); p { color: red }",
        Some("EPUB/style.css"),
        &table,
        &crate::media::MediaContext::screen(432.0, 648.0),
        &limits,
        &mut budget,
    )
    .expect("under every cap");

    assert_eq!(parsed.font_faces.len(), 1, "the face reaches the caller");
    assert_eq!(
        parsed.font_faces[0].base.as_deref(),
        Some("css/fonts.css"),
        "and its base is the sheet it was written in"
    );
    assert_eq!(parsed.rules.len(), 1, "the importing sheet's own rule");
}

/// A `@font-face` inside a `@media` block that matched is read; one inside a
/// block that did not is not.
///
/// The second half is the one that matters: a build that read every nested
/// `@font-face` regardless would load a book's print-only faces on screen, and
/// a build that read none would lose the fonts of every producer that wraps
/// them in a query. `@media` still refuses every other nested at-rule by name,
/// which the third assertion holds.
#[test]
fn a_font_face_inside_a_media_block_follows_the_query() {
    let matched = sheet("@media screen { @font-face { font-family: A; src: url(a.otf) } }");
    assert_eq!(matched.font_faces.len(), 1);
    assert_eq!(matched.report.warnings, vec![]);

    let unmatched = sheet("@media print { @font-face { font-family: A; src: url(a.otf) } }");
    assert_eq!(unmatched.font_faces, vec![]);

    let other = sheet("@media screen { @page { margin: 0 } p { color: red } }");
    assert_eq!(
        other.report.warnings,
        vec![(Warning::AtRuleUnsupported("page".to_owned()), 1)]
    );
    assert_eq!(other.rules.len(), 1, "and the rule after it still parses");
}

/// `@font-face;` with no block is a parse error, not a face.
#[test]
fn a_font_face_with_no_block_is_discarded() {
    let parsed = sheet("@font-face; p { color: red }");
    assert_eq!(parsed.font_faces, vec![]);
    assert_eq!(parsed.report.discarded_rules, 1);
    assert_eq!(
        parsed.rules[0].declarations[0].declaration,
        Declaration::Known(Property::Color(crate::property::Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255
        }))
    );
}
