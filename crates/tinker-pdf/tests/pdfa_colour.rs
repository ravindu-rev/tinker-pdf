//! One fixture per colour rule, each built at the edge of its clause, and each
//! with the near-miss twin that must **not** fire (milestone 5 of
//! `docs/design/pdfa.md`).
//!
//! # The two facts a colour rule is a relation between
//!
//! ISO 19005 does not forbid `DeviceRGB`; it forbids a file whose colours
//! cannot be reproduced. So nearly every test here is a *pair* of edits — a
//! device colour in a content stream and an output intent in the catalog — and
//! the near-miss twin changes one of the two rather than removing both. A rule
//! shown only to fire on a file with no output intent has not been shown to
//! know what an output intent is for.
//!
//! # The ICC profiles here are real, and they have to be
//!
//! `pdfa_support` builds profiles that
//! `tinker_pdf_color::icc::Profile::parse` accepts, because the rules that
//! matter read the data colour space signature out of a parsed profile. A
//! filler profile would be refused at the header, the destination would come
//! back `Unreadable`, and every assertion below would be passing for the wrong
//! reason.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

mod pdfa_support;

use pdfa_support::{cmyk_like, grey_like, srgb_like};

// ---- building a document at the edge of a clause --------------------------

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

/// A document under construction, with every piece a colour rule looks at
/// exposed.
#[derive(Clone)]
struct Fixture {
    part: String,
    level: Option<String>,
    /// The catalog's entries beyond `/Type`, `/Pages` and `/Metadata`.
    catalog_extra: String,
    /// The page's entries beyond `/Type`, `/Parent`, `/MediaBox`,
    /// `/Resources` and `/Contents`.
    page_extra: String,
    /// The page's `/Resources`, as written.
    resources: String,
    /// The page's content stream.
    content: String,
    /// The output intent, object 5, without its enclosing `<< >>`. Empty means
    /// the file has none.
    intent: String,
    /// The destination profile's bytes, object 6.
    profile: Option<Vec<u8>>,
    /// Objects 7 and up.
    extra: Vec<(u32, Vec<u8>)>,
}

impl Fixture {
    /// A document this build finds nothing wrong with: one page painting in
    /// `DeviceRGB` under a PDF/A output intent whose profile is an RGB one.
    fn new(part: &str, level: Option<&str>) -> Fixture {
        Fixture {
            part: part.to_string(),
            level: level.map(str::to_string),
            catalog_extra: "/OutputIntents [5 0 R]".to_string(),
            page_extra: String::new(),
            resources: "<< >>".to_string(),
            content: "1 0 0 rg 10 10 50 50 re f".to_string(),
            intent: "/Type /OutputIntent /S /GTS_PDFA1 \
                     /OutputConditionIdentifier (Custom) /DestOutputProfile 6 0 R"
                .to_string(),
            profile: Some(srgb_like()),
            extra: Vec::new(),
        }
    }

    fn build(&self) -> Vec<u8> {
        let packet = packet(&self.part, self.level.as_deref());
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R {} >>",
                    self.catalog_extra
                )
                .into_bytes(),
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                     /Resources {} /Contents 9 0 R {} >>",
                    self.resources, self.page_extra
                )
                .into_bytes(),
            ),
            (
                4,
                stream("/Type /Metadata /Subtype /XML", packet.as_bytes()),
            ),
            (9, stream("", self.content.as_bytes())),
        ];
        if !self.intent.is_empty() {
            objects.push((5, format!("<< {} >>", self.intent).into_bytes()));
        }
        if let Some(profile) = &self.profile {
            objects.push((6, stream("/N 3", profile)));
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
        // 7.5.4: an object number the file does not define is a **free**
        // entry, not an in-use one pointing at offset zero. Writing `n` for
        // a gap names the file header as an object header, which the strict
        // structural validator reports as `ObjectHeaderAbsent` -- and it was
        // right: these fixtures number their objects sparsely, so every one
        // of them carried two such entries until the PDF/A group learned to
        // ask it.
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
}

/// A stream object: `entries` inside the dictionary, `data` after it.
fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("<< {} /Length {} >>\nstream\n", entries, data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// A conforming part 2 level B document.
fn conforming() -> Fixture {
    Fixture::new("2", Some("B"))
}

// ---- the baseline ---------------------------------------------------------

#[test]
fn the_baseline_is_clean() {
    assert_eq!(
        conforming().findings(),
        Vec::<FindingKind>::new(),
        "the fixture every other test mutates must itself be clean"
    );
    for (part, level) in [
        ("1", Some("B")),
        ("2", Some("U")),
        ("3", Some("B")),
        ("4", None),
    ] {
        assert_eq!(
            Fixture::new(part, level).findings(),
            Vec::<FindingKind>::new(),
            "clean baseline for part {part}"
        );
    }
}

#[test]
fn the_verdict_names_the_colour_group_as_having_run() {
    let document = Document::open(conforming().build()).expect("opens");
    let verdict = document.validate_pdfa();
    assert!(verdict.coverage.colour);
    assert!(
        verdict.coverage.is_complete(),
        "milestone 5 is the point at which an empty finding list can mean \
         conformance, and `is_complete` is what says so"
    );
    assert!(
        !document
            .validate_pdfa_with(PdfACoverage::SYNTAX)
            .coverage
            .colour
    );
}

// ---- 6.2.2 / 6.2.3 The output intent --------------------------------------

/// A `GTS_PDFA1` output intent with no `/DestOutputProfile`. The near-miss is
/// the baseline, which has one.
#[test]
fn a_pdfa_output_intent_without_a_destination_profile_is_a_finding() {
    let mut fixture = conforming();
    fixture.intent = "/Type /OutputIntent /S /GTS_PDFA1 \
                      /OutputConditionIdentifier (Custom)"
        .to_string();
    fixture.profile = None;
    // Two findings, and both are right: the intent has no profile, and the
    // page's `DeviceRGB` now has no intent to be reproduced under. That
    // second finding is the clause working, so this test asserts the pair
    // rather than pretending the rule fires alone.
    let findings = fixture.findings();
    assert!(
        findings.contains(&FindingKind::DestOutputProfileMissing),
        "{findings:#?}"
    );
    assert!(
        findings.contains(&FindingKind::DeviceColourWithoutOutputIntent {
            space: "DeviceRGB".to_string()
        }),
        "{findings:#?}"
    );
    assert_eq!(findings.len(), 2, "{findings:#?}");
}

/// ISO 32000-1 14.11.5 Table 365 requires the identifier of every output
/// intent. The near-miss is the baseline, which has one.
#[test]
fn an_output_intent_without_an_output_condition_identifier_is_a_finding() {
    let mut fixture = conforming();
    fixture.intent = "/Type /OutputIntent /S /GTS_PDFA1 /DestOutputProfile 6 0 R".to_string();
    assert_eq!(
        fixture.one_finding(),
        FindingKind::OutputIntentMalformed {
            key: "OutputConditionIdentifier".to_string()
        }
    );
}

/// Two PDF/A output intents naming two profiles is a file with two answers to
/// "what device is this for", which is no answer.
///
/// The near-miss twin is two intents naming **the same** profile, which the
/// clause explicitly permits and which must not fire.
#[test]
fn two_pdfa_output_intents_naming_different_profiles_is_a_finding() {
    let mut two = conforming();
    two.catalog_extra = "/OutputIntents [5 0 R 7 0 R]".to_string();
    two.extra = vec![
        (
            7,
            b"<< /Type /OutputIntent /S /GTS_PDFA1 \
              /OutputConditionIdentifier (Other) /DestOutputProfile 8 0 R >>"
                .to_vec(),
        ),
        (8, stream("/N 3", &srgb_like())),
    ];
    assert_eq!(two.one_finding(), FindingKind::OutputIntentsDisagree);

    let mut same = conforming();
    same.catalog_extra = "/OutputIntents [5 0 R 7 0 R]".to_string();
    same.extra = vec![(
        7,
        b"<< /Type /OutputIntent /S /GTS_PDFA1 \
          /OutputConditionIdentifier (Other) /DestOutputProfile 6 0 R >>"
            .to_vec(),
    )];
    assert_eq!(same.findings(), Vec::<FindingKind>::new());
}

/// An output intent for a different standard is not this clause's business.
///
/// ISO 32000-1 14.11.5 admits `GTS_PDFX` and others beside the PDF/A one, and
/// judging them would be enforcing a standard the file did not claim. The
/// device colour still has no PDF/A intent, so the *other* rule fires — which
/// is the assertion that shows the two rules are separate.
#[test]
fn an_output_intent_for_another_standard_is_not_a_pdfa_output_intent() {
    let mut fixture = conforming();
    fixture.intent = "/Type /OutputIntent /S /GTS_PDFX /DestOutputProfile 6 0 R".to_string();
    assert_eq!(
        fixture.one_finding(),
        FindingKind::DeviceColourWithoutOutputIntent {
            space: "DeviceRGB".to_string()
        }
    );
}

// ---- 6.2.3.3 / 6.2.4.3 Uncalibrated colour spaces -------------------------

/// A device colour with no output intent at all.
#[test]
fn a_device_colour_without_an_output_intent_is_a_finding() {
    for (content, space) in [
        ("0.5 g 10 10 50 50 re f", "DeviceGray"),
        ("1 0 0 rg 10 10 50 50 re f", "DeviceRGB"),
        ("0 0 0 1 k 10 10 50 50 re f", "DeviceCMYK"),
    ] {
        let mut fixture = conforming();
        fixture.catalog_extra = String::new();
        fixture.intent = String::new();
        fixture.profile = None;
        fixture.content = content.to_string();
        assert_eq!(
            fixture.one_finding(),
            FindingKind::DeviceColourWithoutOutputIntent {
                space: space.to_string()
            },
            "for {content}"
        );
    }
}

/// The relation the clause is really about: an RGB colour under a CMYK
/// destination profile.
///
/// Three assertions in one, and they are the three the reading turns on:
/// `DeviceRGB` needs an RGB profile, `DeviceCMYK` needs a CMYK one, and
/// `DeviceGray` is admitted under either — a grey value is a value on the
/// neutral axis of whatever device the intent names.
#[test]
fn a_device_colour_under_the_wrong_kind_of_profile_is_a_finding() {
    let mut rgb_under_cmyk = conforming();
    rgb_under_cmyk.profile = Some(cmyk_like());
    assert_eq!(
        rgb_under_cmyk.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceRGB".to_string(),
            profile: "CMYK".to_string()
        }
    );

    let mut cmyk_under_rgb = conforming();
    cmyk_under_rgb.content = "0 0 0 1 k 10 10 50 50 re f".to_string();
    assert_eq!(
        cmyk_under_rgb.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceCMYK".to_string(),
            profile: "RGB".to_string()
        }
    );

    // The near-miss on the other side, twice: grey under both.
    for destination in [srgb_like(), cmyk_like()] {
        let mut grey = conforming();
        grey.content = "0.5 g 10 10 50 50 re f".to_string();
        grey.profile = Some(destination);
        assert_eq!(grey.findings(), Vec::<FindingKind>::new());
    }

    // And an RGB colour under a grey profile is a finding, so that the grey
    // exemption is shown to be about the *colour* and not about the profile.
    let mut rgb_under_grey = conforming();
    rgb_under_grey.profile = Some(grey_like());
    assert_eq!(
        rgb_under_grey.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceRGB".to_string(),
            profile: "GRAY".to_string()
        }
    );
}

/// ISO 32000-1 8.6.5.6's `/Default…` space is the escape hatch the clause
/// itself offers, and the near-miss twin is the same document with the entry
/// named for a different device.
#[test]
fn a_default_colour_space_stands_in_for_the_device_space_it_is_named_for() {
    let mut matching = conforming();
    matching.catalog_extra = String::new();
    matching.intent = String::new();
    matching.profile = None;
    matching.resources = "<< /ColorSpace << /DefaultRGB [/CalRGB << /WhitePoint \
                          [0.95 1 1.09] >>] >> >>"
        .to_string();
    assert_eq!(matching.findings(), Vec::<FindingKind>::new());

    let mut mismatched = matching.clone();
    mismatched.resources = "<< /ColorSpace << /DefaultCMYK [/CalRGB << /WhitePoint \
                            [0.95 1 1.09] >>] >> >>"
        .to_string();
    assert_eq!(
        mismatched.one_finding(),
        FindingKind::DeviceColourWithoutOutputIntent {
            space: "DeviceRGB".to_string()
        },
        "a /DefaultCMYK says nothing about DeviceRGB"
    );
}

/// A transparency group's blending space is the second stand-in, and the
/// corpus is where this rule came from — `6-2-4-3-t04-pass-a.pdf` paints in
/// `DeviceRGB` with no matching intent and passes because its page group
/// declares an `ICCBased` RGB space.
///
/// The near-miss twin declares a **device** space in the group, which says
/// nothing and must not excuse anything.
#[test]
fn a_transparency_groups_blending_space_stands_in_for_a_device_space() {
    let mut independent = Fixture::new("4", None);
    independent.catalog_extra = String::new();
    independent.intent = String::new();
    independent.page_extra = "/Group << /S /Transparency /CS [/ICCBased 6 0 R] >>".to_string();
    assert_eq!(independent.findings(), Vec::<FindingKind>::new());

    let mut device = independent.clone();
    device.page_extra = "/Group << /S /Transparency /CS /DeviceRGB >>".to_string();
    device.profile = None;
    assert_eq!(
        device.one_finding(),
        FindingKind::DeviceColourWithoutOutputIntent {
            space: "DeviceRGB".to_string()
        },
        "a group compositing in DeviceRGB has not said what the values mean"
    );
}

/// **A device blending space is judged against the destination, in parts 2 to
/// 4.**
///
/// `6-2-10-t03` states it in its own titles: "DeviceRGB is used as blending
/// color space of the transparency group; the document has a CMYK-based output
/// ICC profile". Blending happens *in* a space, so compositing in `DeviceRGB`
/// under a CMYK destination is the same defect as painting in it there — and
/// it is reported with the same finding, because a reader learning two names
/// for one defect has learned nothing.
#[test]
fn a_device_blending_space_is_judged_against_the_destination() {
    // The page paints in grey, which every destination reproduces, so the only
    // device colour in question is the group's.
    let mut rgb_under_cmyk = Fixture::new("2", Some("B"));
    rgb_under_cmyk.content = "0.5 g 10 10 50 50 re f".to_string();
    rgb_under_cmyk.page_extra = "/Group << /S /Transparency /CS /DeviceRGB >>".to_string();
    rgb_under_cmyk.profile = Some(cmyk_like());
    assert_eq!(
        rgb_under_cmyk.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceRGB".to_string(),
            profile: "CMYK".to_string()
        }
    );

    // The twin: the same group under a destination that can reproduce it.
    let mut rgb_under_rgb = rgb_under_cmyk.clone();
    rgb_under_rgb.profile = Some(srgb_like());
    assert_eq!(rgb_under_rgb.findings(), Vec::<FindingKind>::new());

    // And the other direction, which the suite also states.
    let mut cmyk_under_rgb = rgb_under_cmyk.clone();
    cmyk_under_rgb.page_extra = "/Group << /S /Transparency /CS /DeviceCMYK >>".to_string();
    cmyk_under_rgb.profile = Some(srgb_like());
    assert_eq!(
        cmyk_under_rgb.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceCMYK".to_string(),
            profile: "RGB".to_string()
        }
    );
}

/// **Part 1 is untouched: it forbids transparency rather than conditioning
/// it.**
///
/// The same file under part 1 and part 2 gets two different findings, which is
/// what says the two readings are separate rather than one rule with a
/// tolerance. Part 1 reports the group itself; part 2 reports the space it
/// composites in.
#[test]
fn part_one_forbids_the_group_and_part_two_judges_its_space() {
    let group = "/Group << /S /Transparency /CS /DeviceRGB >>".to_string();

    let mut one = Fixture::new("1", Some("B"));
    one.content = "0.5 g 10 10 50 50 re f".to_string();
    one.page_extra = group.clone();
    one.profile = Some(cmyk_like());
    // The whole list, not a `contains`: part 1 reports the group and says
    // **nothing** about the space it composites in, because the group is
    // already forbidden. A build that ran the parts 2-to-4 gatherer here too
    // would add a second finding, and a `contains` would not have noticed —
    // a counted injection put that at zero failures until this was an
    // equality.
    assert_eq!(
        one.findings(),
        vec![FindingKind::TransparencyForbidden {
            feature: "Group".to_string()
        }]
    );

    let mut two = one.clone();
    two.part = "2".to_string();
    two.level = Some("B".to_string());
    assert_eq!(
        two.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceRGB".to_string(),
            profile: "CMYK".to_string()
        }
    );
}

/// **One defect, reported once.**
///
/// A page that both paints in `DeviceRGB` and composites in it says so a
/// single time. A first draft of this rule reported the blending space
/// separately and produced two findings for one defect on exactly this file,
/// which is the shape `docs/features/pdfa.md` already names as worse than
/// reporting it once.
#[test]
fn a_page_that_paints_and_blends_in_one_device_space_says_so_once() {
    let mut fixture = Fixture::new("2", Some("B"));
    fixture.content = "1 0 0 rg 10 10 50 50 re f".to_string();
    fixture.page_extra = "/Group << /S /Transparency /CS /DeviceRGB >>".to_string();
    fixture.profile = Some(cmyk_like());
    assert_eq!(
        fixture.findings(),
        vec![FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceRGB".to_string(),
            profile: "CMYK".to_string()
        }]
    );
}

/// ISO 19005-4 6.2.3 admits an output intent on a **page**, and parts 1 to 3
/// do not.
///
/// The twin is the same file claiming part 2, where the page-level intent is
/// not read and the device colour is therefore unaccounted for. Two documents
/// differing only in a digit of the XMP packet, opposite verdicts.
#[test]
fn a_page_level_output_intent_is_read_for_part_four_and_not_for_part_two() {
    let mut four = Fixture::new("4", None);
    four.catalog_extra = String::new();
    four.page_extra = "/OutputIntents [5 0 R]".to_string();
    assert_eq!(four.findings(), Vec::<FindingKind>::new());

    let mut two = four.clone();
    two.part = "2".to_string();
    two.level = Some("B".to_string());
    assert_eq!(
        two.one_finding(),
        FindingKind::DeviceColourWithoutOutputIntent {
            space: "DeviceRGB".to_string()
        }
    );
}

/// The alternate space of a `/Separation` is what the colour is reproduced in
/// (8.6.6.4), so a `/Separation` over `DeviceCMYK` uses `DeviceCMYK`.
#[test]
fn a_separations_alternate_space_is_the_space_it_uses() {
    let mut fixture = conforming();
    fixture.resources =
        "<< /ColorSpace << /Spot [/Separation /Ink /DeviceCMYK 7 0 R] >> >>".to_string();
    fixture.content = "/Spot cs 1 scn 10 10 50 50 re f".to_string();
    fixture.extra = vec![(
        7,
        b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >>".to_vec(),
    )];
    assert_eq!(
        fixture.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceCMYK".to_string(),
            profile: "RGB".to_string()
        }
    );

    // The near-miss: the same `/Separation` over an RGB alternate, which the
    // RGB destination profile does admit.
    let mut over_rgb = fixture;
    over_rgb.resources =
        "<< /ColorSpace << /Spot [/Separation /Ink /DeviceRGB 7 0 R] >> >>".to_string();
    assert_eq!(over_rgb.findings(), Vec::<FindingKind>::new());
}

// ---- 6.2.3.2 / 6.2.4.2 ICCBased colour spaces -----------------------------

/// The rule that reads inside a profile: `/N` shall agree with the number of
/// components the profile's data colour space has.
///
/// The near-miss twin is the same profile with the right `/N`, which is what
/// makes this a test of the *agreement* rather than of `/N` being present.
#[test]
fn an_icc_stream_whose_n_disagrees_with_its_profile_is_a_finding() {
    let mut wrong = conforming();
    wrong.resources = "<< /ColorSpace << /Cs [/ICCBased 7 0 R] >> >>".to_string();
    wrong.content = "/Cs cs 1 0 0 sc 10 10 50 50 re f".to_string();
    wrong.extra = vec![(7, stream("/N 4", &srgb_like()))];
    assert_eq!(
        wrong.one_finding(),
        FindingKind::IccStreamMalformed {
            key: "N".to_string()
        }
    );

    let mut right = wrong.clone();
    right.extra = vec![(7, stream("/N 3", &srgb_like()))];
    assert_eq!(right.findings(), Vec::<FindingKind>::new());

    // And `/N` absent, or a value ISO 32000-1 8.6.5.5 does not admit.
    for entries in ["", "/N 2"] {
        let mut malformed = wrong.clone();
        malformed.extra = vec![(7, stream(entries, &srgb_like()))];
        assert_eq!(
            malformed.one_finding(),
            FindingKind::IccStreamMalformed {
                key: "N".to_string()
            },
            "for {entries:?}"
        );
    }
}

// ---- 6.2.9 / 6.2.6 Rendering intents --------------------------------------

/// Both spellings of a rendering intent, and the twin for each.
#[test]
fn a_rendering_intent_outside_the_four_is_a_finding() {
    let mut operator = conforming();
    operator.content = "/Wrong ri 1 0 0 rg 10 10 50 50 re f".to_string();
    assert_eq!(
        operator.one_finding(),
        FindingKind::RenderingIntentUnknown {
            declared: "Wrong".to_string()
        }
    );

    let mut good_operator = operator.clone();
    good_operator.content = "/Perceptual ri 1 0 0 rg 10 10 50 50 re f".to_string();
    assert_eq!(good_operator.findings(), Vec::<FindingKind>::new());

    let mut state = conforming();
    state.resources = "<< /ExtGState << /Gs 7 0 R >> >>".to_string();
    state.content = "/Gs gs 1 0 0 rg 10 10 50 50 re f".to_string();
    state.extra = vec![(
        7,
        b"<< /Type /ExtGState /RenderingIntent /Wrong >>".to_vec(),
    )];
    assert_eq!(
        state.one_finding(),
        FindingKind::RenderingIntentUnknown {
            declared: "Wrong".to_string()
        }
    );

    let mut good_state = state;
    good_state.extra = vec![(
        7,
        b"<< /Type /ExtGState /RenderingIntent /RelativeColorimetric >>".to_vec(),
    )];
    assert_eq!(good_state.findings(), Vec::<FindingKind>::new());
}

// ---- 6.4 Transparency, in part 1 only -------------------------------------

/// Part 1 forbids transparency outright, and each construct is its own
/// finding. Every one of these has the same near-miss twin: the identical
/// document claiming part 2, which permits transparency.
#[test]
fn transparency_is_a_part_one_finding_and_not_a_part_two_one() {
    // Object 9 is the page's own content stream, so the states start at 11.
    // Numbering one of them 9 made the page's `/Contents` resolve to a form
    // XObject with no `gs` in it, the walk saw no graphics state, and the test
    // failed for a reason that had nothing to do with the rule.
    let cases: [(&str, &str, &str, &str); 4] = [
        (
            "Group",
            "/Group << /S /Transparency >>",
            "<< >>",
            "1 0 0 rg 10 10 50 50 re f",
        ),
        (
            "SMask",
            "",
            "<< /ExtGState << /Gs 11 0 R >> >>",
            "/Gs gs 1 0 0 rg 10 10 50 50 re f",
        ),
        (
            "BM",
            "",
            "<< /ExtGState << /Gs 12 0 R >> >>",
            "/Gs gs 1 0 0 rg 10 10 50 50 re f",
        ),
        (
            "ca",
            "",
            "<< /ExtGState << /Gs 14 0 R >> >>",
            "/Gs gs 1 0 0 rg 10 10 50 50 re f",
        ),
    ];
    let states: Vec<(u32, Vec<u8>)> = vec![
        (
            11,
            b"<< /Type /ExtGState /SMask << /S /Alpha /G 13 0 R >> >>".to_vec(),
        ),
        (12, b"<< /Type /ExtGState /BM /Multiply >>".to_vec()),
        (
            13,
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
                b"0 0 10 10 re f",
            ),
        ),
        (14, b"<< /Type /ExtGState /ca 0.5 >>".to_vec()),
    ];

    for (feature, page_extra, resources, content) in cases {
        let mut one = Fixture::new("1", Some("B"));
        one.page_extra = page_extra.to_string();
        one.resources = resources.to_string();
        one.content = content.to_string();
        one.extra.clone_from(&states);
        assert_eq!(
            one.findings(),
            vec![FindingKind::TransparencyForbidden {
                feature: feature.to_string()
            }],
            "for {feature}"
        );

        let mut two = one;
        two.part = "2".to_string();
        assert_eq!(
            two.findings(),
            Vec::<FindingKind>::new(),
            "parts 2 to 4 permit transparency: {feature}"
        );
    }
}

/// The tolerance, and the fixture that forced it: `6-4-t03-pass-b.pdf` writes
/// `/CA 1.0000001` and `/ca 0.9999999` and is annotated `pass`.
///
/// The near-miss twin is `0.999`, which is a thousand times further from one
/// and is transparency.
#[test]
fn an_alpha_one_part_in_ten_million_from_opaque_is_opaque() {
    let mut near = Fixture::new("1", Some("B"));
    near.resources = "<< /ExtGState << /Gs 7 0 R >> >>".to_string();
    near.content = "/Gs gs 1 0 0 rg 10 10 50 50 re f".to_string();
    near.extra = vec![(
        7,
        b"<< /Type /ExtGState /CA 1.0000001 /ca 0.9999999 >>".to_vec(),
    )];
    assert_eq!(near.findings(), Vec::<FindingKind>::new());

    let mut far = near;
    far.extra = vec![(7, b"<< /Type /ExtGState /CA 1 /ca 0.999 >>".to_vec())];
    assert_eq!(
        far.one_finding(),
        FindingKind::TransparencyForbidden {
            feature: "ca".to_string()
        }
    );
}

// ---- the staged rules are named, not silent -------------------------------

/// Every colour rule this build does not run is in `PDFA_STAGED` with a clause
/// and a reason, and the one that matters most is 6.2.2's: a destination
/// profile this build cannot read leaves the intent's colour space unknown
/// rather than wrong.
///
/// This is milestone 5's exit criterion in a test. A staged rule nobody can
/// find is a silent pass with extra steps.
#[test]
fn every_staged_colour_rule_is_named_with_its_clause_and_its_reason() {
    // 6.2.3.4 left this list when 6.2.4.4's two rules landed
    // (`two_separations_of_one_name_with_different_transforms_are_a_finding`,
    // `a_devicen_spot_colorant_is_described_in_its_colorants`): the clause
    // defines the equality it asks for, which was the staged entry's reason.
    for clause in ["6.2.2", "6.2.4", "6.2.5", "6.2.8", "6.2.10", "6.4"] {
        let found = tinker_pdf::PDFA_STAGED
            .iter()
            .find(|rule| rule.clause == clause)
            .unwrap_or_else(|| panic!("no staged rule names clause {clause}"));
        assert!(
            found.because.len() >= 40,
            "a reason short enough to be a shrug is not a reason: {found:?}"
        );
    }

    let profile_rule = tinker_pdf::PDFA_STAGED
        .iter()
        .find(|rule| rule.clause == "6.2.2")
        .expect("the profile-conformance refusal is named");
    assert!(
        profile_rule.because.contains("transform"),
        "the reason has to say why the ICC reader is not a validator: {profile_rule:?}"
    );
}

/// And the refusal is a refusal rather than a pass: a destination profile this
/// build cannot read makes the intent's colour space unknown, so the rules
/// that need it do not fire — in **either** direction.
///
/// The document below paints in `DeviceCMYK` under an intent whose profile is
/// a well-formed ICC header — an output-class CMYK profile, version 2.1 —
/// with no tags behind it, which the transform builder cannot read. A build
/// that guessed would report it; a build that treated an unreadable profile
/// as absent would report it too, under the other kind. Neither happens, and
/// that is what "staged, not guessed" means when you can see it.
///
/// The profile was thirteen bytes of rubbish until the header rule landed,
/// and thirteen bytes are a finding now in their own right
/// (`a_destination_profile_header_the_clause_does_not_admit_is_a_finding`);
/// a header with nothing behind it keeps this test about what it was about.
#[test]
fn an_unreadable_destination_profile_makes_the_intents_space_unknown() {
    let mut fixture = conforming();
    fixture.content = "0 0 0 1 k 10 10 50 50 re f".to_string();
    fixture.profile = Some(header_only(2, b"prtr", b"CMYK"));
    assert!(
        tinker_pdf_color::icc::Profile::parse(&header_only(2, b"prtr", b"CMYK")).is_err(),
        "the fixture is only about an unreadable profile if it is one"
    );
    assert_eq!(
        fixture.findings(),
        Vec::<FindingKind>::new(),
        "an unreadable profile is a staged refusal, not a finding either way"
    );

    // And the twin that shows the silence is about the profile and not about
    // the rule: the same document with a readable CMYK profile is clean, and
    // with a readable RGB one is a finding.
    let mut readable = fixture.clone();
    readable.profile = Some(cmyk_like());
    assert_eq!(readable.findings(), Vec::<FindingKind>::new());

    let mut wrong = fixture;
    wrong.profile = Some(srgb_like());
    assert_eq!(
        wrong.one_finding(),
        FindingKind::DeviceColourNotInOutputIntent {
            space: "DeviceCMYK".to_string(),
            profile: "RGB".to_string()
        }
    );
}

// ---- what a graphics object may not carry --------------------------------

/// A page drawing one image XObject, with `entries` in its dictionary.
fn drawing_an_image(entries: &str) -> Fixture {
    let mut fixture = conforming();
    fixture.resources = "<< /XObject << /Im 7 0 R >> >>".to_string();
    fixture.content = "q 50 0 0 50 10 10 cm /Im Do Q".to_string();
    fixture.extra = vec![(
        7,
        stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
                 /BitsPerComponent 8 /ColorSpace /DeviceGray {entries}"
            ),
            &[0x80],
        ),
    )];
    fixture
}

/// **An image may not name content held outside the file, or ask to be
/// smoothed.**
///
/// `/Alternates` names other versions of the same picture and `/OPI` links a
/// high-resolution original somewhere else — both make the page's appearance
/// depend on something the file does not carry, which is the whole of what
/// archiving forbids. `/Interpolate true` asks a reader to smooth on the way
/// up and 8.9.5.1 leaves how entirely to the reader.
///
/// Each has a twin, and `/Interpolate false` is the twin that matters: it is
/// the default, so a build that reported the *key* rather than the value would
/// report a great many conforming files.
#[test]
fn an_image_may_not_name_outside_content_or_ask_to_be_smoothed() {
    assert_eq!(
        drawing_an_image("/OPI << /F (elsewhere.tif) >>").one_finding(),
        FindingKind::ExternalContentForbidden {
            key: "OPI".to_string()
        }
    );
    assert_eq!(
        drawing_an_image("/Alternates [8 0 R]").one_finding(),
        FindingKind::ExternalContentForbidden {
            key: "Alternates".to_string()
        }
    );
    assert_eq!(
        drawing_an_image("/Interpolate true").one_finding(),
        FindingKind::ImageInterpolated
    );

    // The twins: the default value, and the plain image.
    assert_eq!(
        drawing_an_image("/Interpolate false").findings(),
        Vec::<FindingKind>::new()
    );
    assert_eq!(drawing_an_image("").findings(), Vec::<FindingKind>::new());
}

/// An image states its own rendering intent, and 8.6.5.8's four are all there
/// are — the same closed set the `ri` operator is held to, on the other
/// surface an intent reaches a page through.
#[test]
fn an_images_rendering_intent_is_the_same_closed_set_as_the_operators() {
    assert_eq!(
        drawing_an_image("/Intent /Wrong").one_finding(),
        FindingKind::RenderingIntentUnknown {
            declared: "Wrong".to_string()
        }
    );
    assert_eq!(
        drawing_an_image("/Intent /RelativeColorimetric").findings(),
        Vec::<FindingKind>::new()
    );
}

/// **A form may not carry `/OPI`, and a PostScript XObject may not be drawn at
/// all.**
///
/// 8.8.2 gives an XObject three subtypes and archiving admits two. The third
/// is a program, and `/Subtype2 /PS` is the spelling that makes a *form* one —
/// two ways to the same thing and both are reported.
#[test]
fn a_form_may_not_carry_opi_and_a_postscript_xobject_may_not_be_drawn() {
    let mut form = conforming();
    form.resources = "<< /XObject << /Fm 7 0 R >> >>".to_string();
    form.content = "/Fm Do".to_string();
    form.extra = vec![(
        7,
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /OPI << /F (x.tif) >>",
            b"",
        ),
    )];
    assert_eq!(
        form.one_finding(),
        FindingKind::ExternalContentForbidden {
            key: "OPI".to_string()
        }
    );

    let mut second = form.clone();
    second.extra = vec![(
        7,
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Subtype2 /PS",
            b"",
        ),
    )];
    assert_eq!(
        second.one_finding(),
        FindingKind::PostScriptXObjectForbidden
    );

    let mut postscript = form.clone();
    postscript.extra = vec![(7, stream("/Type /XObject /Subtype /PS", b""))];
    assert_eq!(
        postscript.one_finding(),
        FindingKind::PostScriptXObjectForbidden
    );

    // The twin: an ordinary form, drawn and silent.
    let mut plain = form;
    plain.extra = vec![(
        7,
        stream("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", b""),
    )];
    assert_eq!(plain.findings(), Vec::<FindingKind>::new());
}

/// **`/TR` is forbidden however it is spelled; `/TR2` only when it is not
/// `/Default`.**
///
/// That asymmetry is the Isartor suite's, not a reading: it has four `/TR`
/// fixtures — an array, a function, `/Identity` and **`/Default`** — and all
/// four fail, against three `/TR2` fixtures where only `/Default` is admitted.
/// So `/TR /Default` is a finding and `/TR2 /Default` is not, which "other
/// than Default" alone would not have given.
#[test]
fn a_transfer_function_is_forbidden_and_tr2_default_is_the_one_exception() {
    let state = |entries: &str| {
        let mut fixture = conforming();
        fixture.resources = "<< /ExtGState << /Gs 7 0 R >> >>".to_string();
        fixture.content = "/Gs gs 0.5 g 10 10 50 50 re f".to_string();
        fixture.extra = vec![
            (7, format!("<< /Type /ExtGState {entries} >>").into_bytes()),
            // Object 8 is the function the indirect spellings point at. It has
            // to exist: a reference to a missing object resolves to null, and
            // a rule reading null would find nothing to report — which is how
            // the first version of this test passed for the wrong reason.
            (
                8,
                stream("/FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1", b""),
            ),
        ];
        fixture
    };

    for spelling in ["/TR /Identity", "/TR /Default", "/TR [8 0 R]", "/TR 8 0 R"] {
        assert_eq!(
            state(spelling).one_finding(),
            FindingKind::TransferFunctionForbidden {
                key: "TR".to_string()
            },
            "{spelling}"
        );
    }

    for spelling in ["/TR2 /Identity", "/TR2 [8 0 R]"] {
        assert_eq!(
            state(spelling).one_finding(),
            FindingKind::TransferFunctionForbidden {
                key: "TR2".to_string()
            },
            "{spelling}"
        );
    }

    // The one exception, and the plain state beside it.
    assert_eq!(state("/TR2 /Default").findings(), Vec::<FindingKind>::new());
    assert_eq!(state("").findings(), Vec::<FindingKind>::new());
}

/// A prohibited entry on an XObject **nothing draws** is not reported.
///
/// "Used for rendering" is this group's own qualifier and it decides answers
/// elsewhere — a form's `/DR` names standard-14 fonts nothing paints with. An
/// image carrying `/OPI` in a resource dictionary no content stream names is
/// the same case, and reporting it would be reporting a picture that does not
/// reach the page.
#[test]
fn a_prohibited_entry_on_an_xobject_nothing_draws_is_not_reported() {
    let mut unused = drawing_an_image("/OPI << /F (elsewhere.tif) >>");
    unused.content = "0.5 g 10 10 50 50 re f".to_string();
    assert_eq!(unused.findings(), Vec::<FindingKind>::new());
}

// ---- 6.2.10 / 6.2.2: the operators a content stream may use -----------------

/// Every finding as `(clause, object number, kind)`.
fn located(fixture: &Fixture) -> Vec<(String, Option<u32>, FindingKind)> {
    Document::open(fixture.build())
        .expect("the fixture opens")
        .validate_pdfa()
        .findings
        .into_iter()
        .map(|finding| {
            (
                finding.clause.0,
                finding.object.map(|r| r.num),
                finding.kind,
            )
        })
        .collect()
}

/// ISO 19005-2 6.2.2, in veraPDF's statement of rule 6.2.2-1: "Content
/// streams shall not contain any operators not defined in ISO 32000-1 even if
/// such operators are bracketed by the BX/EX compatibility operators". Part 1
/// numbers it 6.2.10 and says "PDF Reference". One finding per distinct
/// operator, naming the page whose content used it.
#[test]
fn an_operator_iso_32000_does_not_define_is_a_finding_inside_bx_ex_too() {
    for (part, clause) in [("1", "6.2.10"), ("2", "6.2.2")] {
        let mut bracketed = Fixture::new(part, Some("B"));
        bracketed.content = "1 0 0 rg BX 5 xyz xyz EX 10 10 50 50 re f".to_string();
        assert_eq!(
            located(&bracketed),
            [(
                clause.to_string(),
                Some(3),
                FindingKind::OperatorUndefined {
                    operator: "xyz".to_string()
                }
            )],
            "part {part}"
        );
    }
    // `PS`, which earlier PDF defined and ISO 32000 does not: veraPDF's note
    // reads its prohibition out of this same rule.
    let mut postscript = conforming();
    postscript.content = "1 0 0 rg (showpage) PS 10 10 50 50 re f".to_string();
    assert_eq!(
        postscript.one_finding(),
        FindingKind::OperatorUndefined {
            operator: "PS".to_string()
        }
    );
}

/// The twin: Table A.1's less common operators — the compatibility pair,
/// the four marked-content ones, the graphics-state and text-state setters,
/// `T*` — are admitted, and so is an inline image, whose data is not
/// operators at all.
#[test]
fn the_operators_table_a1_defines_are_admitted() {
    let mut rare = conforming();
    rare.content = "BX EX /Span BMC EMC /P << /MCID 0 >> BDC EMC /X MP /X << >> DP \
                    q 1 0 0 1 0 0 cm 0 i 1 j 1 J 4 M [] 0 d 1 w Q \
                    BT /F1 1 Tf 0 Tc 0 Tw 100 Tz 0 TL 0 Ts 0 Tr 1 0 0 1 0 0 Tm \
                    0 0 Td 0 0 TD T* ET \
                    BI /W 1 /H 1 /BPC 8 /CS /G ID \u{1} EI \
                    1 0 0 rg 10 10 50 50 re f"
        .to_string();
    assert_eq!(located(&rare), []);
}

/// A form XObject's content is a content stream too, and the finding names the
/// form rather than the page that invoked it (ruling 10).
#[test]
fn an_undefined_operator_in_a_form_names_the_form() {
    let mut fixture = conforming();
    fixture.resources = "<< /XObject << /X1 7 0 R >> >>".to_string();
    fixture.content = "/X1 Do".to_string();
    fixture.extra.push((
        7,
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
            b"1 0 0 rg 0 0 5 5 re f zzz",
        ),
    ));
    assert_eq!(
        located(&fixture),
        [(
            "6.2.2".to_string(),
            Some(7),
            FindingKind::OperatorUndefined {
                operator: "zzz".to_string()
            }
        )]
    );
}

// ---- 6.2.2 / 6.2.3 and 6.2.3.2 / 6.2.4.2: a profile's own header ------------

/// An ICC profile that is a 128-byte header and an empty tag table: version
/// `major.1`, device class `class`, data colour space `space`, PCS `Lab `,
/// and the `acsp` signature where ICC.1 puts it.
fn header_only(major: u8, class: &[u8; 4], space: &[u8; 4]) -> Vec<u8> {
    let mut profile = vec![0u8; 132];
    profile[0..4].copy_from_slice(&132u32.to_be_bytes());
    profile[8] = major;
    profile[9] = 0x10;
    profile[12..16].copy_from_slice(class);
    profile[16..20].copy_from_slice(space);
    profile[20..24].copy_from_slice(b"Lab ");
    profile[36..40].copy_from_slice(b"acsp");
    profile
}

/// Every finding as `(clause, kind)`.
fn by_clause(fixture: &Fixture) -> Vec<(String, FindingKind)> {
    Document::open(fixture.build())
        .expect("the fixture opens")
        .validate_pdfa()
        .findings
        .into_iter()
        .map(|finding| (finding.clause.0, finding.kind))
        .collect()
}

fn header(field: &str, found: &str) -> FindingKind {
    FindingKind::IccProfileHeader {
        field: field.to_string(),
        found: found.to_string(),
    }
}

/// ISO 19005-2 6.2.3, in veraPDF's statement of rule 6.2.3-1: "The profile
/// stream that is the value of the DestOutputProfile key shall either be an
/// output profile (Device Class = "prtr") or a monitor profile (Device Class
/// = "mntr"). The profiles shall have a colour space of either "GRAY", "RGB",
/// or "CMYK"" — and its test condition bounds the version below 5.0, and part
/// 1's (6.2.2-1) below 3.0. Each of the three is read from the header and
/// nothing else, so a profile the transform builder refuses is judged all
/// the same.
#[test]
fn a_destination_profile_header_the_clause_does_not_admit_is_a_finding() {
    let with = |part: &str, profile: Vec<u8>| {
        let mut fixture = Fixture::new(part, Some("B"));
        fixture.content = "0 g 10 10 50 50 re f".to_string();
        fixture.profile = Some(profile);
        by_clause(&fixture)
    };
    assert_eq!(
        with("2", header_only(2, b"scnr", b"GRAY")),
        [("6.2.3".to_string(), header("device class", "scnr"))]
    );
    assert_eq!(
        with("2", header_only(2, b"prtr", b"Lab ")),
        [("6.2.3".to_string(), header("colour space", "Lab "))]
    );
    assert_eq!(
        with("2", header_only(5, b"mntr", b"GRAY")),
        [("6.2.3".to_string(), header("version", "5.1"))]
    );
    // Version 4 is below part 2's bound and above part 1's.
    assert_eq!(with("2", header_only(4, b"mntr", b"GRAY")), []);
    assert_eq!(
        with("1", header_only(4, b"mntr", b"GRAY")),
        [("6.2.2".to_string(), header("version", "4.1"))]
    );
    // Thirteen bytes say nothing a header must, which is a finding by itself.
    assert_eq!(
        with("2", b"not a profile".to_vec()),
        [("6.2.3".to_string(), header("header", "13 bytes"))]
    );
}

/// ISO 19005-2 6.2.4.2 for an `ICCBased` colour space's profile: the input
/// and colour-space classes are admitted too (`scnr`, `spac`), and `Lab ` is
/// a colour space it may have, which the destination profile may not — the
/// test conditions of veraPDF's 6.2.3.2-1 and 6.2.4.2-1. An abstract profile
/// (`abst`) is not.
#[test]
fn an_icc_based_profile_header_is_judged_against_its_own_list() {
    let with = |profile: Vec<u8>| {
        let mut fixture = conforming();
        fixture.resources = "<< /ColorSpace << /Cs [/ICCBased 7 0 R] >> >>".to_string();
        fixture.content = "/Cs cs 0.5 sc 10 10 50 50 re f".to_string();
        fixture.extra.push((7, stream("/N 1", &profile)));
        by_clause(&fixture)
    };
    assert_eq!(with(header_only(2, b"scnr", b"GRAY")), []);
    assert_eq!(
        with(header_only(2, b"abst", b"GRAY")),
        [("6.2.4.2".to_string(), header("device class", "abst"))]
    );
}

// ---- 6.2.4.4: Separation and DeviceN colour spaces ---------------------------

/// A type 2 function from 0 to `c1` in RGB, as a dictionary's entries.
fn tint(c1: &str) -> String {
    format!("/FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [{c1}] /N 1")
}

/// The baseline painting with two colour spaces, `/A` and `/B`, as written.
fn two_spaces(part: &str, a: &str, b: &str) -> Fixture {
    let mut fixture = Fixture::new(part, Some("B"));
    fixture.resources = format!("<< /ColorSpace << /A {a} /B {b} >> >>");
    fixture.content = "/A cs 1 scn 10 10 20 20 re f /B cs 1 scn 40 40 20 20 re f".to_string();
    fixture
}

/// ISO 19005-2 6.2.4.4, in veraPDF's statement of rule 6.2.4.4-2: "All
/// Separation arrays … that have the same name shall have the same
/// tintTransform and alternateSpace. In evaluating equivalence, the PDF
/// objects shall be compared, rather than the computational result of the use
/// of those PDF objects. Compression and whether or not an object is direct
/// or indirect shall be ignored."
#[test]
fn two_separations_of_one_name_with_different_transforms_are_a_finding() {
    let mut differing = two_spaces(
        "2",
        "[/Separation /Ink /DeviceRGB 7 0 R]",
        &format!("[/Separation /Ink /DeviceRGB << {} >>]", tint("0 0 1")),
    );
    differing
        .extra
        .push((7, format!("<< {} >>", tint("1 0 0")).into_bytes()));
    assert_eq!(
        differing.one_finding(),
        FindingKind::SeparationsDisagree {
            colorant: "Ink".to_string()
        }
    );
    // Part 1 states no such rule.
    let mut part_one = differing.clone();
    part_one.part = "1".to_string();
    assert_eq!(part_one.findings(), Vec::<FindingKind>::new());

    // A different alternate space under one tint transform is the other half
    // of the sentence.
    let one_function = "<< /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>";
    let alternates = two_spaces(
        "2",
        &format!("[/Separation /Ink /DeviceRGB {one_function}]"),
        &format!("[/Separation /Ink /DeviceGray {one_function}]"),
    );
    assert_eq!(
        alternates.one_finding(),
        FindingKind::SeparationsDisagree {
            colorant: "Ink".to_string()
        }
    );
}

/// The twins the clause's own last sentence makes: the same function once by
/// reference and once written in place, and a sampled function once plain and
/// once hex-encoded, are one function. Two names are two colorants.
#[test]
fn the_same_transform_direct_or_indirect_or_encoded_is_the_same() {
    let mut direct_and_indirect = two_spaces(
        "2",
        "[/Separation /Ink /DeviceRGB 7 0 R]",
        &format!("[/Separation /Ink /DeviceRGB << {} >>]", tint("1 0 0")),
    );
    direct_and_indirect
        .extra
        .push((7, format!("<< {} >>", tint("1.0 0 0")).into_bytes()));
    assert_eq!(direct_and_indirect.findings(), Vec::<FindingKind>::new());

    let sampled = "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] \
                   /BitsPerSample 8";
    let mut encoded = two_spaces(
        "2",
        "[/Separation /Ink /DeviceRGB 7 0 R]",
        "[/Separation /Ink /DeviceRGB 8 0 R]",
    );
    encoded
        .extra
        .push((7, stream(sampled, &[255, 255, 255, 255, 0, 0])));
    encoded.extra.push((
        8,
        stream(
            &format!("{sampled} /Filter /ASCIIHexDecode"),
            b"FFFFFFFF0000>",
        ),
    ));
    assert_eq!(encoded.findings(), Vec::<FindingKind>::new());

    let two_names = two_spaces(
        "2",
        &format!("[/Separation /Ink /DeviceRGB << {} >>]", tint("1 0 0")),
        &format!("[/Separation /Other /DeviceRGB << {} >>]", tint("0 0 1")),
    );
    assert_eq!(two_names.findings(), Vec::<FindingKind>::new());
}

/// Two parallel chains of stitching functions, one per side. The top of a
/// side, object 100 or 200, names its chain and then a type 2 function
/// ending at `after`; level `j` of the chain names level `j + 1` thirty-two
/// times, and the bottom, six levels down, is a type 2 function ending at
/// `bottom`.
fn parallel_chains(bottom: [&str; 2], after: [&str; 2]) -> Fixture {
    const LEVELS: u32 = 6;
    let mut fixture = two_spaces(
        "2",
        "[/Separation /Ink /DeviceRGB 100 0 R]",
        "[/Separation /Ink /DeviceRGB 200 0 R]",
    );
    for (side, base) in [100, 200].into_iter().enumerate() {
        fixture.extra.push((
            base,
            format!(
                "<< /FunctionType 3 /Domain [0 1] /Functions [{} 0 R << {} >>] >>",
                base + 1,
                tint(after[side])
            )
            .into_bytes(),
        ));
        for level in 1..=LEVELS {
            let next = format!("{} 0 R ", base + level + 1);
            fixture.extra.push((
                base + level,
                format!(
                    "<< /FunctionType 3 /Domain [0 1] /Functions [{}] >>",
                    next.repeat(32)
                )
                .into_bytes(),
            ));
        }
        fixture.extra.push((
            base + LEVELS + 1,
            format!("<< {} >>", tint(bottom[side])).into_bytes(),
        ));
    }
    fixture
}

/// Ruling 1, from the review of the PDF/A staged rules: the comparison
/// 6.2.4.4 asks for walked a pair of objects once per path to it, so two
/// chains of equal stitching functions, each level naming the next thirty-two
/// times, asked for 32^6 comparisons of the bottom from a file of a few
/// kilobytes and never finished. A pair is now compared once per question:
/// the chains cost a few hundred comparisons, and a difference *after* them
/// is still within the rule's budget and found. And a difference *beneath*
/// them is found too, because remembering a pair is assuming it equal only
/// while the answer is a conjunction that any difference ends.
#[test]
fn a_tint_transform_reached_by_many_paths_is_compared_once() {
    let red = "1 0 0";
    let blue = "0 0 1";
    let equal = parallel_chains([red, red], [red, red]);
    assert_eq!(equal.findings(), Vec::<FindingKind>::new());

    let ink = FindingKind::SeparationsDisagree {
        colorant: "Ink".to_string(),
    };
    assert_eq!(parallel_chains([red, red], [red, blue]).one_finding(), ink);
    assert_eq!(parallel_chains([red, blue], [red, red]).one_finding(), ink);
}

/// The comparison's work budget, which is what bounds it once no pair is
/// compared twice: side A's level of `fan` functions names the leaves in a
/// different order from each function, side B's in one order, so every leaf
/// of A meets every leaf of B — `fan`² pairs, each a leaf's worth of work —
/// before the last entry of the top-level `/Functions`, where the two sides
/// differ. Sixteen leaves is well inside the budget and the difference is
/// found; a hundred and sixty is past it, and a comparison that cannot finish
/// answers "the same", as a nesting past the depth cap always has: a finding
/// has to be one the file shows.
#[test]
fn a_comparison_past_its_work_budget_answers_the_same() {
    let rotated = |fan: u32| {
        let mut fixture = two_spaces(
            "2",
            "[/Separation /Ink /DeviceRGB 100 0 R]",
            "[/Separation /Ink /DeviceRGB 200 0 R]",
        );
        let leaf = format!(
            "<< /FunctionType 0 /Domain [0 1] /Range [{}] >>",
            "0 1 ".repeat(32)
        );
        // Side A is object 100, its middle level 1000.., its leaves 3000..;
        // side B is 200, 2000.. and 4000...
        for (top, middle, leaves, rotate, last) in [
            (100, 1000, 3000, true, "1 0 0"),
            (200, 2000, 4000, false, "0 0 1"),
        ] {
            let names: String = (0..fan).map(|j| format!("{} 0 R ", middle + j)).collect();
            fixture.extra.push((
                top,
                format!(
                    "<< /FunctionType 3 /Functions [{names} << {} >>] >>",
                    tint(last)
                )
                .into_bytes(),
            ));
            for j in 0..fan {
                let names: String = (0..fan)
                    .map(|m| {
                        let leaf = if rotate { (j + m) % fan } else { m };
                        format!("{} 0 R ", leaves + leaf)
                    })
                    .collect();
                fixture.extra.push((
                    middle + j,
                    format!("<< /FunctionType 3 /Functions [{names}] >>").into_bytes(),
                ));
            }
            for m in 0..fan {
                fixture.extra.push((leaves + m, leaf.clone().into_bytes()));
            }
        }
        fixture
    };
    assert_eq!(
        rotated(16).one_finding(),
        FindingKind::SeparationsDisagree {
            colorant: "Ink".to_string()
        }
    );
    assert_eq!(rotated(160).findings(), Vec::<FindingKind>::new());
}

/// Rule 6.2.4.4-1: "For any spot colour used in a DeviceN or NChannel colour
/// space, an entry in the Colorants dictionary shall be present". The process
/// colorants need none, and a `/Colorants` entry is the twin — whose own
/// `/Separation` the consistency rule then reads ("including those in
/// Colorants dictionaries").
#[test]
fn a_devicen_spot_colorant_is_described_in_its_colorants() {
    let device_n = |attributes: &str| {
        let mut fixture = conforming();
        fixture.resources = format!(
            "<< /ColorSpace << /N [/DeviceN [/Cyan /Spot] /DeviceRGB 7 0 R {attributes}] >> >>"
        );
        fixture.content = "/N cs 0.5 0.5 scn 10 10 50 50 re f".to_string();
        fixture.extra.push((
            7,
            stream(
                "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1]",
                b"{ pop pop 0 0 0 }",
            ),
        ));
        fixture
    };
    assert_eq!(
        device_n("").one_finding(),
        FindingKind::ColorantUndescribed {
            colorant: "Spot".to_string()
        }
    );
    let described = device_n(&format!(
        "<< /Colorants << /Spot [/Separation /Spot /DeviceRGB << {} >>] >> >>",
        tint("0 0 1")
    ));
    assert_eq!(described.findings(), Vec::<FindingKind>::new());

    // The Colorants entry against a page-level Separation of the same name.
    let mut against = described;
    against.resources = against.resources.replace(
        "/ColorSpace << /N",
        &format!(
            "/ColorSpace << /S [/Separation /Spot /DeviceRGB << {} >>] /N",
            tint("1 0 0")
        ),
    );
    against.content = format!("/S cs 1 scn 0 0 5 5 re f {}", against.content);
    assert_eq!(
        against.one_finding(),
        FindingKind::SeparationsDisagree {
            colorant: "Spot".to_string()
        }
    );
}

/// Rule 6.4-2, veraPDF's statement of ISO 19005-1 6.4 (with Cor.2:2011): "An
/// XObject dictionary shall not contain the SMask key." The image the page
/// draws carries a soft mask; the twins are the same image without one, and
/// the same document claiming part 2, which permits transparency.
#[test]
fn an_xobject_soft_mask_is_part_one_transparency() {
    let mut masked = drawing_an_image("/SMask 8 0 R");
    masked.part = "1".to_string();
    masked.extra.push((
        8,
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
             /BitsPerComponent 8 /ColorSpace /DeviceGray",
            &[0xFF],
        ),
    ));
    assert_eq!(
        masked.one_finding(),
        FindingKind::TransparencyForbidden {
            feature: "SMask".to_string()
        }
    );

    let mut plain = drawing_an_image("");
    plain.part = "1".to_string();
    assert_eq!(plain.findings(), Vec::<FindingKind>::new());

    let mut two = masked;
    two.part = "2".to_string();
    assert_eq!(two.findings(), Vec::<FindingKind>::new());
}
