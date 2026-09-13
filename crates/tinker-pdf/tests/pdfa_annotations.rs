//! The annotation rule group, one fixture per requirement.
//!
//! # Why every test here is a pair
//!
//! This group judges every `/Type /Annot` in the file rather than waiting to
//! be reached, and an annotation is the most ordinary thing a real document
//! carries. So the failure that matters is not a rule that misses — it is a
//! rule that reports a conforming file, and "something was found" is no
//! evidence against it. Every fixture below therefore comes with a twin that
//! differs by one key and must produce **nothing**.
//!
//! # The corpus wrote the exemptions, not this build
//!
//! Four of the rules here have carve-outs that no reading of the clause text
//! would give, and each is taken from a fixture the conformance suite
//! annotates `pass`:
//!
//! - a `Popup` needs no `/F` at all (`6-3-2-t01-pass-b`, under two parts);
//! - a `Popup`, a `Link` and part 4's `Projection` need no appearance
//!   (`6-3-3-t01-pass-b`, `-pass-c`, `-pass-d`);
//! - an annotation whose `/Rect` is a **point** needs none either
//!   (`6-3-3-t01-pass-a`), while one that is merely zero-width does
//!   (`6-3-3-t01-fail-p`, annotated fail);
//! - a push-button widget's `/N` is a sub-dictionary of states rather than a
//!   stream (`6-3-3-t02-pass-a`), and the `/FT` that says so may be inherited
//!   from the parent field (`6-4-1-t01-pass-b`, which this build reported
//!   until it followed `/Parent`).
//!
//! # Injection, counted
//!
//! Recorded in `docs/design/pdfa.md` under "What the annotation group
//! measured", with the bar as the unit for the two that move a number rather
//! than a verdict.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

mod pdfa_support;

use pdfa_support::{cmyk_like, srgb_like};

// ---- building a document with annotations on its page ---------------------

/// The XMP packet claiming `part` and `level`.
fn packet(part: &str, level: Option<&str>) -> String {
    let conformance = match level {
        Some(letter) => format!(r#" pdfaid:conformance="{letter}""#),
        None => String::new(),
    };
    let revision = if part == "4" {
        r#" pdfaid:rev="2020""#
    } else {
        ""
    };
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="{part}"{conformance}{revision}/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

fn stream(dict: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A conforming document with annotations on its one page.
#[derive(Clone)]
struct Fixture {
    part: String,
    level: Option<String>,
    /// Each annotation's dictionary body, without its enclosing `<< >>`.
    /// They become objects 10, 11, … and the page's `/Annots`.
    annotations: Vec<String>,
    /// Objects 20 and up, for an appearance stream or a parent field.
    extra: Vec<(u32, Vec<u8>)>,
    /// The destination profile, or `None` for a file with no output intent.
    profile: Option<Vec<u8>>,
}

impl Fixture {
    /// A document this build finds nothing wrong with, carrying `annotations`.
    ///
    /// Everything else conforms — an sRGB output intent, an embedded-free
    /// content stream, a `%PDF-2.0` header under part 4 — so any finding is
    /// the one the fixture was built to produce, and
    /// [`a_conforming_annotation_is_silent`] is what keeps that true.
    fn new(part: &str, level: Option<&str>, annotations: &[&str]) -> Fixture {
        Fixture {
            part: part.to_string(),
            level: level.map(str::to_string),
            annotations: annotations.iter().map(|a| (*a).to_string()).collect(),
            extra: Vec::new(),
            profile: Some(srgb_like()),
        }
    }

    fn with(mut self, num: u32, body: Vec<u8>) -> Fixture {
        self.extra.push((num, body));
        self
    }

    fn build(&self) -> Vec<u8> {
        let packet = packet(&self.part, self.level.as_deref());
        let references: Vec<String> = (0..self.annotations.len())
            .map(|index| format!("{} 0 R", 10 + index))
            .collect();
        let annots = if references.is_empty() {
            String::new()
        } else {
            format!("/Annots [{}]", references.join(" "))
        };
        // A fixture with no destination profile has no output intent at all,
        // so the catalog must not name one: a dangling `/OutputIntents [5 0 R]`
        // is its own finding and would drown the one under test.
        let intents = if self.profile.is_some() {
            "/OutputIntents [5 0 R]"
        } else {
            ""
        };
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!("<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R {intents} >>").into_bytes(),
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                     /Resources << >> /Contents 9 0 R {annots} >>"
                )
                .into_bytes(),
            ),
            (
                4,
                stream("/Type /Metadata /Subtype /XML", packet.as_bytes()),
            ),
            (9, stream("", b"0 g 10 10 50 50 re f")),
        ];
        if let Some(profile) = &self.profile {
            objects.push((
                5,
                b"<< /Type /OutputIntent /S /GTS_PDFA1 \
                   /OutputConditionIdentifier (Custom) /DestOutputProfile 6 0 R >>"
                    .to_vec(),
            ));
            objects.push((6, stream("/N 3", profile)));
        }
        for (index, body) in self.annotations.iter().enumerate() {
            objects.push((10 + index as u32, format!("<< {body} >>").into_bytes()));
        }
        objects.extend(self.extra.iter().cloned());
        objects.sort_by_key(|(num, _)| *num);

        let highest = objects.iter().map(|(n, _)| *n).max().unwrap_or(0) as usize;
        let header: &[u8] = if self.part == "4" {
            b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n"
        } else {
            b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n"
        };
        let mut out = header.to_vec();
        let mut offsets = vec![0u64; highest + 1];
        for (num, body) in &objects {
            offsets[*num as usize] = out.len() as u64;
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len() as u64;
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", highest + 1).as_bytes());
        // A number the file does not define is a free entry, not an in-use one
        // pointing at offset zero — these fixtures number sparsely, and the
        // strict structural validator reads the difference.
        for entry in offsets.iter().skip(1) {
            if *entry == 0 {
                out.extend_from_slice(b"0000000000 65535 f \n");
            } else {
                out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
            }
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /ID [<0102030405060708090A0B0C0D0E0F10> \
                 <0102030405060708090A0B0C0D0E0F10>] >>\nstartxref\n{xref_at}\n%%EOF\n",
                highest + 1
            )
            .as_bytes(),
        );
        out
    }

    fn findings(&self) -> Vec<FindingKind> {
        Document::open(self.build())
            .expect("the fixture opens")
            .validate_pdfa()
            .findings
            .into_iter()
            .map(|finding| finding.kind)
            .collect()
    }

    /// The findings, and an assertion that there is exactly one.
    #[track_caller]
    fn one_finding(&self) -> FindingKind {
        let findings = self.findings();
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding, got {findings:#?}"
        );
        findings.into_iter().next().expect("one")
    }

    #[track_caller]
    fn is_silent(&self) {
        assert_eq!(self.findings(), Vec::new(), "expected no findings");
    }
}

/// A conforming annotation: printable, opaque, with a normal appearance.
const CONFORMING: &str =
    "/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>";

/// The appearance stream `CONFORMING` points at.
fn appearance() -> (u32, Vec<u8>) {
    (
        20,
        stream("/Type /XObject /Subtype /Form /BBox [0 0 50 50]", b""),
    )
}

fn with_appearance(part: &str, level: Option<&str>, annotations: &[&str]) -> Fixture {
    let (num, body) = appearance();
    Fixture::new(part, level, annotations).with(num, body)
}

// ---- the baseline ---------------------------------------------------------

#[test]
fn a_conforming_annotation_is_silent() {
    for (part, level) in [("1", Some("B")), ("2", Some("B")), ("4", None)] {
        with_appearance(part, level, &[CONFORMING]).is_silent();
    }
}

/// A file with no annotations at all is silent, which is what says the group
/// judges annotations rather than every dictionary that happens to have a
/// `/Rect`.
#[test]
fn a_document_with_no_annotations_is_silent() {
    Fixture::new("2", Some("B"), &[]).is_silent();
}

// ---- 6.5.2 / 6.3.1: the permitted types -----------------------------------

/// Each part excludes a different set, and the fixture is the same file.
#[test]
fn a_forbidden_subtype_is_a_finding_and_the_set_is_per_part() {
    // `Movie` is excluded by every part.
    for (part, level) in [("1", Some("B")), ("2", Some("B")), ("4", None)] {
        let fixture = with_appearance(
            part,
            level,
            &["/Type /Annot /Subtype /Movie /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>"],
        );
        assert_eq!(
            fixture.one_finding(),
            FindingKind::AnnotationTypeForbidden {
                subtype: "Movie".to_string()
            },
            "part {part}"
        );
    }

    // `Polygon` is excluded by part 1 and permitted by part 2: it is an ISO
    // 32000-1 type that PDF 1.4 did not have.
    let polygon = "/Type /Annot /Subtype /Polygon /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>";
    assert_eq!(
        with_appearance("1", Some("B"), &[polygon]).one_finding(),
        FindingKind::AnnotationTypeForbidden {
            subtype: "Polygon".to_string()
        }
    );
    with_appearance("2", Some("B"), &[polygon]).is_silent();
}

/// A type the reference specification does not define at all is a different
/// finding from a standard type the part excludes.
#[test]
fn a_type_nobody_defines_is_not_the_same_finding_as_one_a_part_excludes() {
    assert_eq!(
        with_appearance(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /Doodle /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>"],
        )
        .one_finding(),
        FindingKind::AnnotationTypeNonStandard {
            subtype: "Doodle".to_string()
        }
    );

    // `RichMedia` is ISO 32000-2's, so it is non-standard under part 2 and a
    // standard type part 4 excludes. Same file, two different defects.
    let rich = "/Type /Annot /Subtype /RichMedia /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>";
    assert_eq!(
        with_appearance("2", Some("B"), &[rich]).one_finding(),
        FindingKind::AnnotationTypeNonStandard {
            subtype: "RichMedia".to_string()
        }
    );
    assert_eq!(
        with_appearance("4", None, &[rich]).one_finding(),
        FindingKind::AnnotationTypeForbidden {
            subtype: "RichMedia".to_string()
        }
    );
}

/// Part 4's levels are permissions: E admits 3D and rich media, F admits
/// attachments, and neither admits the other's.
#[test]
fn the_part_four_levels_admit_exactly_what_they_are_for() {
    let three_d = "/Type /Annot /Subtype /3D /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>";
    let attachment =
        "/Type /Annot /Subtype /FileAttachment /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>";

    with_appearance("4", Some("E"), &[three_d]).is_silent();
    with_appearance("4", Some("F"), &[attachment]).is_silent();

    assert_eq!(
        with_appearance("4", Some("F"), &[three_d]).one_finding(),
        FindingKind::AnnotationTypeForbidden {
            subtype: "3D".to_string()
        }
    );
    assert_eq!(
        with_appearance("4", Some("E"), &[attachment]).one_finding(),
        FindingKind::AnnotationTypeForbidden {
            subtype: "FileAttachment".to_string()
        }
    );
}

/// Level E admits 3D artwork and then says what the artwork may be.
#[test]
fn level_e_artwork_is_u3d_or_prc_and_nothing_else() {
    let annotation =
        "/Type /Annot /Subtype /3D /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >> /3DD 21 0 R";
    let (num, body) = appearance();

    for format in ["U3D", "PRC"] {
        Fixture::new("4", Some("E"), &[annotation])
            .with(num, body.clone())
            .with(21, stream(&format!("/Type /3D /Subtype /{format}"), b""))
            .is_silent();
    }

    let fixture = Fixture::new("4", Some("E"), &[annotation])
        .with(num, body)
        .with(21, stream("/Type /3D /Subtype /STEP", b""));
    assert_eq!(
        fixture.one_finding(),
        FindingKind::ThreeDArtworkFormat {
            declared: "STEP".to_string()
        }
    );
}

// ---- 6.5.3 / 6.3.2: the flag word -----------------------------------------

#[test]
fn an_annotation_with_no_flags_is_a_finding() {
    assert_eq!(
        with_appearance(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /AP << /N 20 0 R >>"],
        )
        .one_finding(),
        FindingKind::AnnotationFlagsMissing
    );
}

/// The corpus's own carve-out, asserted in both directions: a popup needs no
/// `/F`, and a square in the same file does.
#[test]
fn a_popup_needs_no_flags_and_a_square_does() {
    let popup = "/Type /Annot /Subtype /Popup /Rect [10 10 60 60]";
    with_appearance("2", Some("B"), &[popup]).is_silent();

    assert_eq!(
        with_appearance(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /AP << /N 20 0 R >>"],
        )
        .one_finding(),
        FindingKind::AnnotationFlagsMissing
    );
}

/// Each bit is its own finding, and each is checked in the direction the
/// clause states it.
#[test]
fn each_flag_bit_is_its_own_finding() {
    // Print set and nothing else is the conforming word.
    with_appearance(
        "2",
        Some("B"),
        &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 /AP << /N 20 0 R >>"],
    )
    .is_silent();

    // Bit 1 Invisible (1), bit 2 Hidden (2), bit 6 NoView (32), bit 9
    // ToggleNoView (256) — each written *with* Print set, so the only defect
    // is the bit under test.
    for (value, flag) in [
        (4 | 1, "Invisible"),
        (4 | 2, "Hidden"),
        (4 | 32, "NoView"),
        (4 | 256, "ToggleNoView"),
    ] {
        let fixture = with_appearance(
            "2",
            Some("B"),
            &[&format!(
                "/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F {value} /AP << /N 20 0 R >>"
            )],
        );
        assert_eq!(
            fixture.one_finding(),
            FindingKind::AnnotationFlag { flag, set: true },
            "/F {value}"
        );
    }

    // And Print clear, which is the one that must be *set*.
    assert_eq!(
        with_appearance(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 0 /AP << /N 20 0 R >>"],
        )
        .one_finding(),
        FindingKind::AnnotationFlag {
            flag: "Print",
            set: false
        }
    );
}

/// ToggleNoView is bit **9**, and this is the test that says so.
///
/// The corpus fixture is `6-3-2-t02-fail-e`, which writes `/F 268` — bits 3,
/// 4 and 9, so Print set, NoZoom set and ToggleNoView set. A build reading bit
/// 10 finds nothing to say about it, and bit 4 is not a bit this clause cares
/// about, so the file must produce exactly one finding.
#[test]
fn the_corpus_flag_word_produces_exactly_the_toggle_finding() {
    assert_eq!(
        with_appearance(
            "4",
            None,
            &["/Type /Annot /Subtype /Line /Rect [0 0 612 792] /F 268 /AP << /N 20 0 R >>"],
        )
        .one_finding(),
        FindingKind::AnnotationFlag {
            flag: "ToggleNoView",
            set: true
        }
    );
}

// ---- 6.5.3 / 6.3.2: opacity -----------------------------------------------

#[test]
fn an_annotation_that_is_not_opaque_is_a_finding() {
    assert_eq!(
        with_appearance(
            "2",
            Some("B"),
            &[
                "/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 /CA 0.5 \
               /AP << /N 20 0 R >>"
            ],
        )
        .one_finding(),
        FindingKind::AnnotationNotOpaque
    );

    // The twin, and the tolerance with it: a producer that wrote a number
    // meaning opaque and landed a part in ten million away has not made the
    // annotation transparent.
    for alpha in ["1", "1.0", "0.9999999"] {
        with_appearance(
            "2",
            Some("B"),
            &[&format!(
                "/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 /CA {alpha} \
                 /AP << /N 20 0 R >>"
            )],
        )
        .is_silent();
    }
}

// ---- 6.5.3 / 6.3.3: the appearance ----------------------------------------

#[test]
fn an_annotation_with_no_appearance_is_a_finding() {
    assert_eq!(
        Fixture::new(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4"],
        )
        .one_finding(),
        FindingKind::AnnotationAppearanceMissing
    );
}

/// `Popup`, `Link` and part 4's `Projection` need none, and a `Square` does.
#[test]
fn the_three_subtypes_that_need_no_appearance() {
    Fixture::new(
        "2",
        Some("B"),
        &["/Type /Annot /Subtype /Link /Rect [10 10 60 60] /F 4"],
    )
    .is_silent();
    Fixture::new(
        "2",
        Some("B"),
        &["/Type /Annot /Subtype /Popup /Rect [10 10 60 60]"],
    )
    .is_silent();
    Fixture::new(
        "4",
        None,
        &["/Type /Annot /Subtype /Projection /Rect [10 10 60 60] /F 4"],
    )
    .is_silent();

    // `Projection` is ISO 32000-2's, so under part 2 it is not a type at all
    // and the appearance exemption does not reach it either. **Both** findings
    // are right, and asserting the pair is what says the exemption is written
    // against part 4 rather than against the name alone.
    assert_eq!(
        Fixture::new(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /Projection /Rect [10 10 60 60] /F 4"],
        )
        .findings(),
        vec![
            FindingKind::AnnotationTypeNonStandard {
                subtype: "Projection".to_string()
            },
            FindingKind::AnnotationAppearanceMissing,
        ]
    );
}

/// A `/Rect` that is a point needs no appearance; one that is merely
/// zero-width does.
///
/// Both halves are the corpus's: `6-3-3-t01-pass-a` writes `[50 110 50 110]`
/// and passes, `6-3-3-t01-fail-p` writes `[50 600 50 50]` and fails. Reading
/// the exemption as "no area" would pass the second, which is a real defect
/// going unreported rather than a conforming file reported — the quieter of
/// the two failures and still a failure.
#[test]
fn a_point_needs_no_appearance_and_a_zero_width_rectangle_does() {
    Fixture::new(
        "2",
        Some("B"),
        &["/Type /Annot /Subtype /Text /Rect [50 110 50 110] /F 4"],
    )
    .is_silent();

    assert_eq!(
        Fixture::new(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /FileAttachment /Rect [50 600 50 50] /F 4"],
        )
        .one_finding(),
        FindingKind::AnnotationAppearanceMissing
    );
}

/// The `/AP` carries an `/N` and nothing else.
#[test]
fn an_appearance_dictionary_carries_only_the_normal_appearance() {
    let (num, body) = appearance();
    let fixture = Fixture::new(
        "2",
        Some("B"),
        &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 \
           /AP << /N 20 0 R /D 20 0 R >>"],
    )
    .with(num, body);
    assert_eq!(
        fixture.one_finding(),
        FindingKind::AnnotationAppearanceExtraState {
            key: "D".to_string()
        }
    );
}

/// An `/AP` with entries and no `/N` is the appearance-missing finding, not a
/// third kind: there is no normal appearance either way.
#[test]
fn an_appearance_dictionary_with_no_normal_entry() {
    let (num, body) = appearance();
    let findings = Fixture::new(
        "2",
        Some("B"),
        &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 /AP << /D 20 0 R >>"],
    )
    .with(num, body)
    .findings();
    assert_eq!(
        findings,
        vec![
            FindingKind::AnnotationAppearanceExtraState {
                key: "D".to_string()
            },
            FindingKind::AnnotationAppearanceMissing,
        ]
    );
}

/// `/N` is a stream everywhere except on a push-button widget.
#[test]
fn a_normal_appearance_is_a_stream() {
    assert_eq!(
        Fixture::new(
            "2",
            Some("B"),
            &["/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 \
               /AP << /N << /Off 20 0 R >> >>"],
        )
        .with(appearance().0, appearance().1)
        .one_finding(),
        FindingKind::AnnotationAppearanceNotAStream
    );
}

/// A push-button widget's `/N` is a sub-dictionary of states, and the `/FT`
/// that says so may be inherited from the parent field.
///
/// The inherited case is the one that reported a conforming corpus file:
/// `6-4-1-t01-pass-b` is a radio group whose two kid widgets carry the states
/// and whose parent carries `/FT /Btn`.
#[test]
fn a_push_button_widget_carries_states_and_its_field_type_may_be_inherited() {
    let states = "/Type /Annot /Subtype /Widget /Rect [10 10 60 60] /F 4 \
                  /AS /Off /AP << /N << /Off 20 0 R /On 20 0 R >> >>";

    // Stated on the widget itself.
    Fixture::new("2", Some("B"), &[&format!("{states} /FT /Btn")])
        .with(appearance().0, appearance().1)
        .is_silent();

    // Inherited through `/Parent`.
    Fixture::new("2", Some("B"), &[&format!("{states} /Parent 21 0 R")])
        .with(appearance().0, appearance().1)
        .with(21, b"<< /FT /Btn /T (group) >>".to_vec())
        .is_silent();

    // A text field's `/N` is a stream, and states there are a finding.
    assert_eq!(
        Fixture::new("2", Some("B"), &[&format!("{states} /FT /Tx")])
            .with(appearance().0, appearance().1)
            .one_finding(),
        FindingKind::AnnotationAppearanceNotAStream
    );

    // And the other direction: a button whose `/N` is a stream.
    assert_eq!(
        Fixture::new(
            "2",
            Some("B"),
            &[
                "/Type /Annot /Subtype /Widget /Rect [10 10 60 60] /F 4 /FT /Btn \
               /AP << /N 20 0 R >>"
            ],
        )
        .with(appearance().0, appearance().1)
        .one_finding(),
        FindingKind::AnnotationAppearanceNotStates
    );
}

// ---- 6.5.3: the annotation's own colour, which is the colour group's ------

/// `/C` and `/IC` are device colours and the output intent is what makes them
/// reproducible — part 1 only, which is where the corpus tests it.
#[test]
fn an_annotation_colour_is_judged_against_the_output_intent() {
    let coloured = "/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 \
                    /C [1 0 0] /AP << /N 20 0 R >>";

    // An RGB border under an RGB intent.
    with_appearance("1", Some("B"), &[coloured]).is_silent();

    // The same border with no output intent at all. The page paints in grey,
    // which is reported too — grey is only admitted *against* a destination —
    // so this one asserts by containment and names why.
    let mut fixture = with_appearance("1", Some("B"), &[coloured]);
    fixture.profile = None;
    let findings = fixture.findings();
    assert!(
        findings.contains(&FindingKind::DeviceColourWithoutOutputIntent {
            space: "DeviceRGB".to_string()
        }),
        "{findings:#?}"
    );

    // And under a CMYK intent, where RGB is a colour nobody can reproduce.
    let mut fixture = with_appearance("1", Some("B"), &[coloured]);
    fixture.profile = Some(cmyk_like());
    assert_eq!(
        fixture.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceRGB".to_string(),
            profile: "CMYK".to_string()
        }
    );
}

/// A one-component colour is grey, and grey is on the neutral axis.
#[test]
fn a_grey_annotation_colour_is_admitted_under_any_intent() {
    let mut fixture = with_appearance(
        "1",
        Some("B"),
        &[
            "/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 /C [0] \
           /AP << /N 20 0 R >>",
        ],
    );
    fixture.profile = Some(cmyk_like());
    fixture.is_silent();
}

/// Outside part 1 the rule does not run, and the silence is a decision.
///
/// `6-3-3-t01-pass-d` is a part 4 `Projection` carrying `/C [1 0 0]` in a file
/// with no output intent, annotated `pass`. Whatever ISO 19005-4 does here it
/// is not what part 1 does, and `PDFA_STAGED` names the gap.
#[test]
fn an_annotation_colour_is_not_judged_outside_part_one() {
    let mut fixture = with_appearance(
        "4",
        None,
        &[
            "/Type /Annot /Subtype /Square /Rect [10 10 60 60] /F 4 /C [1 0 0] \
           /AP << /N 20 0 R >>",
        ],
    );
    fixture.profile = None;
    // Exactly one finding, and it is the *page's* grey rather than the
    // annotation's red: with no output intent every device colour a content
    // stream paints is reported, and the annotation's `/C` is not. Asserting
    // the whole list is what makes that a claim about the annotation rule
    // instead of a claim that nothing happened.
    assert_eq!(
        fixture.findings(),
        vec![FindingKind::DeviceColourWithoutOutputIntent {
            space: "DeviceGray".to_string()
        }]
    );
}

// ---- what the group does not reach for ------------------------------------

/// The annotation rules ride the syntax group, so a syntax-only sweep runs
/// them and builds nothing.
///
/// This is the assertion behind the placement argument: if these rules ever
/// need machinery, this test is what fails.
#[test]
fn the_annotation_rules_run_in_a_syntax_only_sweep() {
    let bytes = Fixture::new(
        "2",
        Some("B"),
        &["/Type /Annot /Subtype /Movie /Rect [10 10 60 60] /F 4"],
    )
    .build();
    let verdict = Document::open(bytes)
        .expect("the fixture opens")
        .validate_pdfa_with(PdfACoverage::SYNTAX);
    let kinds: Vec<FindingKind> = verdict.findings.into_iter().map(|f| f.kind).collect();
    assert!(
        kinds.contains(&FindingKind::AnnotationTypeForbidden {
            subtype: "Movie".to_string()
        }),
        "{kinds:#?}"
    );
    assert!(verdict.coverage.syntax);
    assert!(!verdict.coverage.colour);
}

/// A dictionary that is not an annotation is not judged as one.
///
/// The group matches `/Type /Annot` and nothing looser, because a rule that
/// guessed from `/Subtype` and `/Rect` would judge form XObjects, page groups
/// and anything else that carries a rectangle.
#[test]
fn a_dictionary_that_is_not_an_annotation_is_not_judged() {
    Fixture::new(
        "2",
        Some("B"),
        &["/Subtype /Movie /Rect [10 10 60 60] /Contents (not an annotation)"],
    )
    .is_silent();
}
