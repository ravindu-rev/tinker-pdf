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
//! [`srgb_like`] and [`cmyk_like`] build profiles that
//! `tinker_pdf_color::icc::Profile::parse` accepts, because the rules that
//! matter read the data colour space signature out of a parsed profile. A
//! filler profile would be refused at the header, the destination would come
//! back `Unreadable`, and every assertion below would be passing for the wrong
//! reason.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

// ---- building an ICC profile the reader accepts ---------------------------

/// A profile with the given data colour space signature and tag set.
///
/// `icc::Profile::parse` is a **transform builder**, so a profile it accepts
/// has to carry enough tags to build a transform from — which is exactly the
/// property the colour rules depend on, and exactly what a filler profile
/// would not have. The first version of these fixtures wrote three `XYZ`
/// columns and no tone curves; every profile came back unreadable, the
/// destination came back `Unreadable`, and five tests passed by finding
/// nothing for the wrong reason. Building the real thing is what makes them
/// tests.
fn profile(space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let header = 128usize;
    let table = 4 + tags.len() * 12;
    let mut offsets = Vec::with_capacity(tags.len());
    let mut at = header + table;
    for (_, data) in tags {
        offsets.push(at);
        at += data.len();
        // ICC.1 aligns tag data to four bytes. Not enforced by this reader,
        // written anyway, because a fixture that is legal in one respect and
        // not in another tests the reader's leniency rather than the rule.
        at = at.div_ceil(4) * 4;
    }
    let total = at;

    let mut out = vec![0u8; header];
    out[0..4].copy_from_slice(&(total as u32).to_be_bytes());
    // Version 2.4.0, in the byte layout ICC.1 gives the field.
    out[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(b"XYZ ");
    out[36..40].copy_from_slice(b"acsp");

    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    for ((signature, data), offset) in tags.iter().zip(&offsets) {
        out.extend_from_slice(signature);
        out.extend_from_slice(&(*offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    for ((_, data), offset) in tags.iter().zip(&offsets) {
        out.resize(*offset, 0);
        out.extend_from_slice(data);
    }
    out.resize(total, 0);
    out
}

/// An `XYZType` tag holding one colourant column.
fn xyz_tag(x: u32, y: u32, z: u32) -> Vec<u8> {
    let mut out = b"XYZ ".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    for value in [x, y, z] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out
}

/// A `curv` tag with no points, which ICC.1 defines as the identity.
fn identity_curve() -> Vec<u8> {
    let mut out = b"curv".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out
}

/// An `mft1` lookup table from `inputs` channels to the three of the
/// connection space, on the smallest grid the format admits.
///
/// This is what a CMYK profile carries and why one cannot be faked with a
/// matrix: the relation between ink and light is not linear, so ICC.1 gives a
/// four-channel profile a sampled table and `icc::Profile::parse` refuses a
/// four-channel profile that has no `A2B*` tag at all.
fn lut_tag(inputs: usize) -> Vec<u8> {
    let outputs = 3usize;
    let grid = 2usize;
    let mut out = b"mft1".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    out.push(inputs as u8);
    out.push(outputs as u8);
    out.push(grid as u8);
    out.push(0);
    // The 3x3 matrix, which an `A2B*` tag never applies. Written as the
    // identity because the field exists and a reader may look at it.
    for row in 0..3usize {
        for column in 0..3usize {
            let value: u32 = if row == column { 0x0001_0000 } else { 0 };
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
    // `mft1`'s input and output tables are fixed at 256 entries of one byte.
    out.extend(std::iter::repeat_n(0u8, inputs * 256));
    out.extend(std::iter::repeat_n(0u8, grid.pow(inputs as u32) * outputs));
    out.extend(std::iter::repeat_n(0u8, outputs * 256));
    out
}

/// An RGB destination profile: three colourant columns and three tone curves,
/// which is what `Profile::parse` calls a matrix/TRC model.
fn srgb_like() -> Vec<u8> {
    profile(
        b"RGB ",
        &[
            (*b"rXYZ", xyz_tag(0x0000_6FA2, 0x0000_38F5, 0x0000_0390)),
            (*b"gXYZ", xyz_tag(0x0000_6299, 0x0000_B785, 0x0000_18DA)),
            (*b"bXYZ", xyz_tag(0x0000_24A0, 0x0000_0F84, 0x0000_B6CF)),
            (*b"rTRC", identity_curve()),
            (*b"gTRC", identity_curve()),
            (*b"bTRC", identity_curve()),
        ],
    )
}

/// A CMYK destination profile: a four-channel `A2B1` lookup table.
fn cmyk_like() -> Vec<u8> {
    profile(b"CMYK", &[(*b"A2B1", lut_tag(4))])
}

/// A grey destination profile: one tone curve and a white point.
fn grey_like() -> Vec<u8> {
    profile(
        b"GRAY",
        &[
            (*b"kTRC", identity_curve()),
            (*b"wtpt", xyz_tag(0x0000_F6D6, 0x0001_0000, 0x0000_D32D)),
        ],
    )
}

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
        for entry in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
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
    for clause in [
        "6.2.2", "6.2.3.4", "6.2.4", "6.2.5", "6.2.8", "6.2.10", "6.4",
    ] {
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
/// twelve bytes of rubbish. A build that guessed would report it; a build that
/// treated an unreadable profile as absent would report it too, under the
/// other kind. Neither happens, and that is what "staged, not guessed" means
/// when you can see it.
#[test]
fn an_unreadable_destination_profile_makes_the_intents_space_unknown() {
    let mut fixture = conforming();
    fixture.content = "0 0 0 1 k 10 10 50 50 re f".to_string();
    fixture.profile = Some(b"not a profile".to_vec());
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
