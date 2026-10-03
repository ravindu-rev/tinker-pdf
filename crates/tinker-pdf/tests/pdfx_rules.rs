//! One fixture per PDF/X rule, each built at the edge of its clause, and each
//! with the near-miss twin that must **not** fire (`docs/design/pdfx.md`,
//! milestones 3 to 5).
//!
//! The discipline is `pdfua_rules.rs`'s: every test starts from [`X::new`] —
//! a document this build finds nothing wrong with under the level it claims —
//! makes one change, and asserts one finding of one kind under one clause.
//! [`the_baseline_is_clean_under_both_2003_levels`] fails first if the
//! baseline grows a finding, which is what keeps the single-finding
//! assertions honest.
//!
//! # Where each rule comes from, and what these fixtures cannot do
//!
//! ISO 15930 is sold and is not in this environment, and no annotated PDF/X
//! corpus exists anywhere this project could find. Each rule is a
//! transcription of the CGATS *Application Notes for PDF/X Standards*,
//! Version 4, as the design quotes it, and each test cites the note's
//! section. **These twins are the only thing that ever makes a PDF/X rule
//! fire**, and a twin built here is this project's reading of a secondary
//! source in both directions: a sentence read wrongly is read wrongly twice
//! and the pair agrees with itself. The census in `pdfx_census.rs` is the
//! false-positive bar, and for the 2003 levels it is empty until a file that
//! claims one is pinned.

use tinker_pdf::{
    ConformanceFinding, Document, FindingKind, PdfXAbstentionClass, PdfXCoverage, PdfXFlavour,
};

/// A 128-byte ICC header and nothing else: version 2, an output-device
/// profile, the data colour space given. The PDF/X rules read the header's
/// data colour space and nothing past it.
fn profile(space: &[u8; 4]) -> Vec<u8> {
    let mut bytes = vec![0u8; 128];
    bytes[0..4].copy_from_slice(&128u32.to_be_bytes());
    bytes[8] = 2;
    bytes[12..16].copy_from_slice(b"prtr");
    bytes[16..20].copy_from_slice(space);
    bytes[20..24].copy_from_slice(b"Lab ");
    bytes[36..40].copy_from_slice(b"acsp");
    bytes
}

/// A document under construction, with every piece a PDF/X rule looks at
/// exposed.
///
/// Objects: 1 catalog, 2 pages, 3 the page, 4 `/Info`, 5 the output intent,
/// 6 its profile, 9 the page's content. Objects 20 and up are a test's own.
struct X {
    /// `GTS_PDFXVersion`, or `None` for none.
    version: Option<String>,
    /// The other `/Info` entries.
    info: String,
    /// The catalog's `/OutputIntents` value, or `None` for none.
    intents: Option<String>,
    /// Object 5's entries.
    intent: String,
    /// Object 6's data colour space, or `None` for no object 6.
    profile: Option<[u8; 4]>,
    /// The page's own boxes, as arrays, or `None` for absent.
    media: Option<String>,
    crop: Option<String>,
    bleed: Option<String>,
    trim: Option<String>,
    art: Option<String>,
    /// Entries in the `/Pages` node.
    pages: String,
    /// Pages after object 3, by object number; a test writes their bodies
    /// into [`X::extra`].
    kids: Vec<u32>,
    /// Entries in the page dictionary.
    page: String,
    /// The page's `/Resources`.
    resources: String,
    /// The page's content stream.
    content: String,
    /// Objects 20 and up.
    extra: Vec<(u32, Vec<u8>)>,
}

impl X {
    /// A document claiming `version` that this build finds nothing wrong
    /// with: `/Trapped /False`, a `GTS_PDFX` intent embedding a CMYK
    /// profile, one page with a media box, a bleed box and a trim box inside
    /// it, painted in `DeviceCMYK`.
    fn new(version: &str) -> X {
        X {
            version: Some(version.to_string()),
            info: "/Trapped /False /Title (A title)".to_string(),
            intents: Some("[5 0 R]".to_string()),
            intent: "/Type /OutputIntent /S /GTS_PDFX \
                     /OutputConditionIdentifier (Custom) /DestOutputProfile 6 0 R"
                .to_string(),
            profile: Some(*b"CMYK"),
            media: Some("[0 0 200 200]".to_string()),
            crop: None,
            bleed: Some("[5 5 195 195]".to_string()),
            trim: Some("[10 10 190 190]".to_string()),
            art: None,
            pages: String::new(),
            kids: Vec::new(),
            page: String::new(),
            resources: "<< >>".to_string(),
            content: "0 0 0 1 k 20 20 100 100 re f".to_string(),
            extra: Vec::new(),
        }
    }

    fn x1a() -> X {
        X::new("PDF/X-1a:2003")
    }

    fn x3() -> X {
        X::new("PDF/X-3:2003")
    }

    fn build(&self) -> Vec<u8> {
        let mut catalog = "/Type /Catalog /Pages 2 0 R".to_string();
        if let Some(intents) = &self.intents {
            catalog.push_str(&format!(" /OutputIntents {intents}"));
        }
        let mut page = "/Type /Page /Parent 2 0 R".to_string();
        for (key, value) in [
            ("MediaBox", &self.media),
            ("CropBox", &self.crop),
            ("BleedBox", &self.bleed),
            ("TrimBox", &self.trim),
            ("ArtBox", &self.art),
        ] {
            if let Some(value) = value {
                page.push_str(&format!(" /{key} {value}"));
            }
        }
        page.push_str(&format!(
            " /Resources {} /Contents 9 0 R {}",
            self.resources, self.page
        ));
        let mut info = self.info.clone();
        if let Some(version) = &self.version {
            info.push_str(&format!(" /GTS_PDFXVersion ({version})"));
        }

        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, format!("<< {catalog} >>").into_bytes()),
            (
                2,
                format!(
                    "<< /Type /Pages /Kids [3 0 R {}] /Count {} {} >>",
                    self.kids
                        .iter()
                        .map(|num| format!("{num} 0 R "))
                        .collect::<String>(),
                    1 + self.kids.len(),
                    self.pages
                )
                .into_bytes(),
            ),
            (3, format!("<< {page} >>").into_bytes()),
            (4, format!("<< {info} >>").into_bytes()),
            (5, format!("<< {} >>", self.intent).into_bytes()),
            (9, stream("", self.content.as_bytes())),
        ];
        if let Some(space) = &self.profile {
            let channels = if space == b"CMYK" { 4 } else { 3 };
            objects.push((6, stream(&format!("/N {channels}"), &profile(space))));
        }
        objects.extend(self.extra.iter().cloned());
        objects.sort_by_key(|(num, _)| *num);
        objects.dedup_by_key(|(num, _)| *num);
        assemble(&objects)
    }

    fn document(&self) -> Document {
        Document::open(self.build()).expect("the fixture opens")
    }

    /// Every finding a full validation reports.
    fn findings(&self) -> Vec<ConformanceFinding> {
        self.document().validate_pdfx().findings
    }

    /// The one finding this fixture is built to produce, as `(clause, kind)`.
    #[track_caller]
    fn one_finding(&self) -> (String, FindingKind) {
        let findings = self.findings();
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding, got {findings:#?}"
        );
        let finding = findings.into_iter().next().expect("one");
        (finding.clause.0, finding.kind)
    }

    #[track_caller]
    fn clean(&self) {
        let findings = self.findings();
        assert!(
            findings.is_empty(),
            "expected no finding, got {findings:#?}"
        );
    }
}

/// A stream object: `entries` inside the dictionary, `data` after it.
fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("<< {} /Length {} >>\nstream\n", entries, data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// A classic cross-reference table around `objects`, gaps written free, with
/// object 4 as `/Info`.
fn assemble(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let highest = objects.iter().map(|(n, _)| *n).max().unwrap_or(0) as usize;
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0u64; highest + 1];
    for (num, body) in objects {
        offsets[*num as usize] = out.len() as u64;
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len() as u64;
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", highest + 1).as_bytes());
    for entry in offsets.iter().skip(1) {
        if *entry == 0 {
            out.extend_from_slice(b"0000000000 65535 f \n");
        } else {
            out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
        }
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R \
             /ID [<0102030405060708090A0B0C0D0E0F10> <0102030405060708090A0B0C0D0E0F10>] \
             >>\nstartxref\n{xref_at}\n%%EOF\n",
            highest + 1
        )
        .as_bytes(),
    );
    out
}

/// `fixture`'s one finding under both 2003 levels: 15930-4's clause under
/// PDF/X-1a:2003, the note's section under PDF/X-3:2003.
#[track_caller]
fn under_both(change: impl Fn(&mut X), x1a: &str, x3: &str, kind: &FindingKind) {
    for (mut fixture, clause) in [(X::x1a(), x1a), (X::x3(), x3)] {
        change(&mut fixture);
        assert_eq!(
            fixture.one_finding(),
            (clause.to_string(), kind.clone()),
            "under {:?}",
            fixture.version
        );
    }
}

/// The twin: `change` leaves the document clean under both levels.
#[track_caller]
fn clean_under_both(change: impl Fn(&mut X)) {
    for mut fixture in [X::x1a(), X::x3()] {
        change(&mut fixture);
        fixture.clean();
    }
}

// ---- the baseline ---------------------------------------------------------

/// Without this, every `one_finding` below could be passing by accident.
#[test]
fn the_baseline_is_clean_under_both_2003_levels() {
    for (fixture, flavour) in [
        (X::x1a(), PdfXFlavour::X1a2003),
        (X::x3(), PdfXFlavour::X3_2003),
    ] {
        let verdict = fixture.document().validate_pdfx();
        assert_eq!(verdict.findings, vec![]);
        assert_eq!(verdict.flavour, Some(flavour));
        assert!(verdict.coverage.is_complete());
        for class in [PdfXAbstentionClass::Staged, PdfXAbstentionClass::Unread] {
            assert!(
                verdict.abstained.iter().any(|a| a.class == class),
                "a clean {flavour} verdict still names what it did not decide"
            );
        }
    }
}

// ---- identification -------------------------------------------------------

/// 15930-4 clause 5 and AN 2.3: the claim is `/Info`'s `GTS_PDFXVersion`. A
/// level this build identifies and does not validate runs no rule — the
/// baseline broken five ways claiming PDF/X-3:2002 or PDF/X-4 has no finding,
/// and its verdict names the whole part as not read. A claim made only in an
/// XMP packet is no claim for these levels.
#[test]
fn only_the_2003_levels_are_judged_and_the_others_say_so() {
    let broken = |version: &str| {
        let mut fixture = X::new(version);
        fixture.info = String::new();
        fixture.intents = None;
        fixture.trim = None;
        fixture.extra.push((20, stream("/Filter /LZWDecode", b"x")));
        fixture
    };
    assert_eq!(broken("PDF/X-1a:2003").findings().len(), 4);

    for (version, flavour, unread) in [
        ("PDF/X-3:2002", PdfXFlavour::X3_2002, 1),
        ("PDF/X-4", PdfXFlavour::X4, 27),
    ] {
        let verdict = broken(version).document().validate_pdfx();
        assert_eq!(verdict.flavour, Some(flavour));
        assert!(!flavour.is_validated());
        assert_eq!(verdict.findings, vec![], "{version}");
        assert_eq!(verdict.coverage, PdfXCoverage::default());
        assert_eq!(verdict.abstained.len(), unread, "{version}");
        assert!(verdict
            .abstained
            .iter()
            .all(|a| a.class == PdfXAbstentionClass::Unread));
    }

    let mut in_xmp_only = broken("unused");
    in_xmp_only.version = None;
    let verdict = in_xmp_only.document().validate_pdfx();
    assert_eq!(verdict.claim, None);
    assert_eq!(verdict.findings, vec![]);
    assert!(verdict.abstained.is_empty());
}

// ---- AN 2.11 encryption ---------------------------------------------------

/// AN 2.11: "None of the PDF/X standards covered by these application notes
/// permit the use of PDF-based encryption." The baseline re-saved by this
/// engine's own writer under an empty user password, which the reader is
/// then given — `Document::open` does not try the empty password, and the
/// claim is an encrypted string until it is. The twin is the baseline.
#[test]
fn an_encrypted_file_is_reported() {
    for (fixture, clause) in [(X::x1a(), "6.11"), (X::x3(), "AN 2.11")] {
        let bytes = fixture.document().editor().save(&tinker_pdf::WriteOptions {
            mode: tinker_pdf::WriteMode::Rewrite,
            encryption: Some(tinker_pdf::Encryption {
                user_password: String::new(),
                owner_password: "owner".to_string(),
                permissions: -1,
                entropy: [9; 48],
            }),
            ..tinker_pdf::WriteOptions::default()
        });
        let document = Document::open(bytes).expect("it opens");
        document
            .authenticate("")
            .expect("the empty user password authenticates");
        let findings = document.validate_pdfx().findings;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].clause.0, clause);
        assert_eq!(findings[0].kind, FindingKind::Encrypted);
        assert_eq!(findings[0].object, None, "a finding about the file");
        fixture.clean();
    }
}

// ---- AN 2.8 data compression ----------------------------------------------

/// AN 2.8: any lossless filter "other than LZW", and "JBIG2 compression may
/// not be used". Each forbidden filter is its own finding naming the stream;
/// the twins are Flate and DCT, which the note admits.
#[test]
fn lzw_and_jbig2_are_forbidden_and_flate_and_dct_are_not() {
    for filter in ["/LZWDecode", "/JBIG2Decode", "[/FlateDecode /LZWDecode]"] {
        let with = |fixture: &mut X| {
            fixture
                .extra
                .push((20, stream(&format!("/Filter {filter}"), b"x")));
        };
        let name = filter
            .trim_start_matches('[')
            .split_whitespace()
            .last()
            .unwrap_or_default()
            .trim_start_matches('/')
            .trim_end_matches(']');
        under_both(
            with,
            "6.5",
            "AN 2.8",
            &FindingKind::FilterForbidden {
                filter: name.to_string(),
            },
        );
        let mut fixture = X::x1a();
        with(&mut fixture);
        let findings = fixture.findings();
        assert_eq!(
            findings[0].object.map(|r| r.num),
            Some(20),
            "the stream is named"
        );
    }
    for filter in [
        "/FlateDecode",
        "/DCTDecode",
        "[/ASCII85Decode /FlateDecode]",
    ] {
        clean_under_both(|fixture| {
            fixture
                .extra
                .push((20, stream(&format!("/Filter {filter}"), b"x")));
        });
    }
}

// ---- AN 2.17 trapping -----------------------------------------------------

/// AN 2.17: `/Trapped` is required, and is "a name object — /True or /False
/// — and not a boolean"; `/Unknown` is not permitted. The twins are the two
/// names the note admits.
#[test]
fn trapped_is_required_and_is_true_or_false_as_a_name() {
    under_both(
        |fixture| fixture.info = "/Title (A title)".to_string(),
        "6.6",
        "AN 2.17",
        &FindingKind::TrappedMissing,
    );
    for (value, found) in [
        ("/Unknown", "/Unknown"),
        ("true", "true"),
        ("false", "false"),
        ("(True)", "a string"),
    ] {
        under_both(
            |fixture| fixture.info = format!("/Trapped {value}"),
            "6.6",
            "AN 2.17",
            &FindingKind::TrappedInvalid {
                found: found.to_string(),
            },
        );
    }
    for value in ["/True", "/False"] {
        clean_under_both(|fixture| fixture.info = format!("/Trapped {value}"));
    }
    let mut fixture = X::x1a();
    fixture.info = "/Trapped /Unknown".to_string();
    assert_eq!(
        fixture.findings()[0].object.map(|r| r.num),
        Some(4),
        "the information dictionary is named"
    );
}

// ---- AN 2.10 the page boxes -----------------------------------------------

/// AN 2.10: `/MediaBox` required — its own or inherited, which is the twin —
/// and "each PDF/X page shall include either an ArtBox or TrimBox, but not
/// both".
#[test]
fn a_page_has_a_media_box_and_exactly_one_of_trim_and_art() {
    under_both(
        |fixture| fixture.media = None,
        "6.8",
        "AN 2.10",
        &FindingKind::PageBoxMissing {
            key: "MediaBox".to_string(),
        },
    );
    clean_under_both(|fixture| {
        fixture.media = None;
        fixture.pages = "/MediaBox [0 0 200 200]".to_string();
    });

    under_both(
        |fixture| fixture.trim = None,
        "6.8",
        "AN 2.10",
        &FindingKind::TrimOrArtBoxMissing,
    );
    under_both(
        |fixture| fixture.art = Some("[10 10 190 190]".to_string()),
        "6.8",
        "AN 2.10",
        &FindingKind::TrimAndArtBox,
    );
    clean_under_both(|fixture| {
        fixture.trim = None;
        fixture.art = Some("[10 10 190 190]".to_string());
    });
    // A trim box on the `/Pages` node is no trim box: Table 30 does not make
    // it inheritable.
    under_both(
        |fixture| {
            fixture.trim = None;
            fixture.pages = "/TrimBox [10 10 190 190]".to_string();
        },
        "6.8",
        "AN 2.10",
        &FindingKind::TrimOrArtBoxMissing,
    );
}

/// AN 2.10: neither the art nor the trim box may extend beyond the bleed box,
/// and the same for the crop box — the crop box inherited, as Table 30 makes
/// it. The twins: a trim box equal to the bleed box, and no bleed box at all.
#[test]
fn the_trim_or_art_box_stays_inside_the_bleed_and_crop_boxes() {
    under_both(
        |fixture| fixture.trim = Some("[4 10 190 190]".to_string()),
        "6.8",
        "AN 2.10",
        &FindingKind::PageBoxOutside {
            inner: "TrimBox".to_string(),
            outer: "BleedBox".to_string(),
        },
    );
    under_both(
        |fixture| {
            fixture.trim = None;
            fixture.art = Some("[10 10 196 190]".to_string());
        },
        "6.8",
        "AN 2.10",
        &FindingKind::PageBoxOutside {
            inner: "ArtBox".to_string(),
            outer: "BleedBox".to_string(),
        },
    );
    under_both(
        |fixture| fixture.pages = "/CropBox [12 0 200 200]".to_string(),
        "6.8",
        "AN 2.10",
        &FindingKind::PageBoxOutside {
            inner: "TrimBox".to_string(),
            outer: "CropBox".to_string(),
        },
    );
    clean_under_both(|fixture| fixture.trim = Some("[5 5 195 195]".to_string()));
    clean_under_both(|fixture| {
        fixture.bleed = None;
        fixture.trim = Some("[0 0 200 200]".to_string());
    });
}

// ---- AN 2.28 annotations --------------------------------------------------

/// An annotation of `subtype` with rectangle `rect`, on the page.
fn annotated(subtype: &str, rect: &str) -> impl Fn(&mut X) {
    let subtype = subtype.to_string();
    let rect = rect.to_string();
    move |fixture: &mut X| {
        fixture.page = "/Annots [20 0 R]".to_string();
        fixture.extra.push((
            20,
            format!("<< /Type /Annot /Subtype /{subtype} /Rect {rect} >>").into_bytes(),
        ));
    }
}

/// AN 2.28: annotations "must fall entirely outside the BleedBox". The
/// twins: one in the margin past the bleed, one touching the bleed's edge,
/// and the invisible signature widget's empty `[0 0 0 0]`.
#[test]
fn an_annotation_inside_the_bleed_box_is_reported() {
    under_both(
        annotated("Link", "[50 50 60 60]"),
        "6.13",
        "AN 2.28",
        &FindingKind::AnnotationInsideBox {
            subtype: "Link".to_string(),
            boundary: "BleedBox".to_string(),
        },
    );
    // Overlapping the edge is inside.
    under_both(
        annotated("Text", "[190 190 199 199]"),
        "6.13",
        "AN 2.28",
        &FindingKind::AnnotationInsideBox {
            subtype: "Text".to_string(),
            boundary: "BleedBox".to_string(),
        },
    );
    clean_under_both(annotated("Link", "[196 196 199 199]"));
    clean_under_both(annotated("Link", "[195 0 200 200]"));
    clean_under_both(annotated("Widget", "[0 0 0 0]"));

    let mut fixture = X::x1a();
    annotated("Link", "[50 50 60 60]")(&mut fixture);
    assert_eq!(
        fixture.findings()[0].object.map(|r| r.num),
        Some(20),
        "the annotation is named"
    );
}

/// AN 2.28: a `PrinterMark` annotation may sit inside the bleed and must stay
/// outside the trim or art box. And with no bleed box the page's trim box is
/// the boundary every other annotation stays outside — the narrower of the two
/// readings, which the rule's documentation argues. A `TrapNet` annotation is
/// held to neither rule (staged by name).
#[test]
fn a_printers_mark_stays_outside_the_trim_box_and_trapnet_is_not_judged() {
    clean_under_both(annotated("PrinterMark", "[6 6 9 9]"));
    under_both(
        annotated("PrinterMark", "[6 6 20 20]"),
        "6.13",
        "AN 2.28",
        &FindingKind::AnnotationInsideBox {
            subtype: "PrinterMark".to_string(),
            boundary: "TrimBox".to_string(),
        },
    );
    under_both(
        |fixture| {
            fixture.bleed = None;
            annotated("Link", "[6 6 20 20]")(fixture);
        },
        "6.13",
        "AN 2.28",
        &FindingKind::AnnotationInsideBox {
            subtype: "Link".to_string(),
            boundary: "TrimBox".to_string(),
        },
    );
    clean_under_both(|fixture| {
        fixture.bleed = None;
        annotated("Link", "[6 6 9 9]")(fixture);
    });
    clean_under_both(annotated("TrapNet", "[0 0 200 200]"));
}

// ---- what one rule may cost and report ------------------------------------

/// `count` more pages like object 3 — its boxes, and `entries` besides —
/// numbered from `first`.
fn more_pages(fixture: &mut X, first: u32, count: u32, entries: &str) {
    for num in first..first + count {
        fixture.kids.push(num);
        fixture.extra.push((
            num,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                 /BleedBox [5 5 195 195] /TrimBox [10 10 190 190] {entries} >>"
            )
            .into_bytes(),
        ));
    }
}

/// An annotation of `subtype` at `rect`, as an object body.
fn annotation(subtype: &str, rect: &str) -> Vec<u8> {
    format!("<< /Type /Annot /Subtype /{subtype} /Rect {rect} >>").into_bytes()
}

/// Ruling 1, from the review of lane 7A: one annotation inside the bleed,
/// named 4 096 times from one `/Annots` array that sixty-four pages share,
/// was 262 144 findings — a million from a 60 KB file at 256 pages, and
/// about 67 million at the page cap. An annotation is judged once, on the
/// first page that names it, and an array once, on the first page that holds
/// it: a page names its annotations, and ISO 32000-1 12.5.2's `/P` gives an
/// annotation one page. The twin: the same array once more under a second
/// annotation inside the bleed, which is a second finding.
#[test]
fn an_annotation_named_from_many_pages_is_judged_once() {
    let mut fixture = X::x1a();
    fixture.page = "/Annots 21 0 R".to_string();
    fixture
        .extra
        .push((20, annotation("Text", "[50 50 60 60]")));
    fixture
        .extra
        .push((21, format!("[{}]", "20 0 R ".repeat(4096)).into_bytes()));
    more_pages(&mut fixture, 100, 63, "/Annots 21 0 R");
    let inside = FindingKind::AnnotationInsideBox {
        subtype: "Text".to_string(),
        boundary: "BleedBox".to_string(),
    };
    let findings = fixture.findings();
    assert_eq!(findings.len(), 1, "one annotation, one finding");
    assert_eq!(
        (findings[0].object.map(|r| r.num), &findings[0].kind),
        (Some(20), &inside)
    );

    if let Some(array) = fixture.extra.iter_mut().find(|(num, _)| *num == 21) {
        array.1 = format!("[22 0 R {}]", "20 0 R ".repeat(4095)).into_bytes();
    }
    fixture
        .extra
        .push((22, annotation("Text", "[70 70 80 80]")));
    assert_eq!(fixture.findings().len(), 2);
}

/// A rule reports at most sixty-four findings, the PDF/UA group's figure for
/// its reason: a file with a thousand pages without a trim box has one
/// defect, and a verdict carrying a thousand copies of it is not provenance.
/// Each rule is capped on its own, so the box rule's copies do not hide a
/// filter. The twins: sixty-three of each are all reported.
#[test]
fn a_rule_reports_at_most_sixty_four_findings() {
    let crowded = |count: u32| {
        let mut fixture = X::x1a();
        // The box rule: pages with no trim box.
        more_pages(&mut fixture, 100, count, "");
        for page in &mut fixture.extra {
            page.1 = String::from_utf8_lossy(&page.1)
                .replace("/TrimBox [10 10 190 190] ", "")
                .into_bytes();
        }
        // The annotation rule: distinct annotations inside the bleed.
        fixture.page = format!(
            "/Annots [{}]",
            (0..count)
                .map(|i| format!("{} 0 R ", 1000 + i))
                .collect::<String>()
        );
        for i in 0..count {
            fixture
                .extra
                .push((1000 + i, annotation("Text", "[50 50 60 60]")));
        }
        // The filter rule: LZW streams.
        for i in 0..count {
            fixture
                .extra
                .push((2000 + i, stream("/Filter /LZWDecode", b"x")));
        }
        let mut counts = std::collections::BTreeMap::new();
        for finding in fixture.findings() {
            let rule = match finding.kind {
                FindingKind::TrimOrArtBoxMissing => "boxes",
                FindingKind::AnnotationInsideBox { .. } => "annotations",
                FindingKind::FilterForbidden { .. } => "filters",
                other => panic!("an unexpected finding: {other:?}"),
            };
            *counts.entry(rule).or_insert(0usize) += 1;
        }
        counts
    };
    let expected = |n: usize| {
        [("annotations", n), ("boxes", n), ("filters", n)]
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(crowded(63), expected(63));
    assert_eq!(crowded(200), expected(64));
}

/// The walk's own budget, for the files the two rules above do not reach:
/// distinct `/Annots` arrays, each naming one annotation outside every box
/// 4 096 times, reported nothing and cost a resolution each. The rule reads
/// at most 2^18 entries across the document — sixty-four such pages — and a
/// page past them is not looked at; the twin is the same page with one array
/// fewer before it.
#[test]
fn the_annotation_walk_stops_at_its_budget() {
    let walk = |arrays: u32| {
        let mut fixture = X::x1a();
        fixture.extra.push((20, annotation("Text", "[0 0 2 2]")));
        for array in 0..arrays {
            fixture.extra.push((
                3000 + array,
                format!("[{}]", "20 0 R ".repeat(4096)).into_bytes(),
            ));
            more_pages(
                &mut fixture,
                100 + array,
                1,
                &format!("/Annots {} 0 R", 3000 + array),
            );
        }
        // The last page holds an annotation inside the bleed.
        fixture
            .extra
            .push((21, annotation("Text", "[50 50 60 60]")));
        more_pages(&mut fixture, 2000, 1, "/Annots [21 0 R]");
        fixture.findings().len()
    };
    assert_eq!(walk(63), 1);
    assert_eq!(walk(64), 0);
}

// ---- AN 2.29 private keys -------------------------------------------------

/// AN 2.29: a private `/Info` key's value is a text string. The twins: the
/// same key with a string, and the keys ISO 32000 and ISO 15930 define.
#[test]
fn a_private_info_key_is_a_text_string() {
    for value in ["/Name", "3", "<< >>"] {
        under_both(
            |fixture| fixture.info = format!("/Trapped /False /Custom {value}"),
            "AN 2.29",
            "AN 2.29",
            &FindingKind::InfoValueNotText {
                key: "Custom".to_string(),
            },
        );
    }
    clean_under_both(|fixture| {
        fixture.info = "/Trapped /False /Custom (text) /Producer (p) \
                        /GTS_PDFXConformance (PDF/X-1a:2003)"
            .to_string();
    });
}

// ---- AN 2.16 the output intent --------------------------------------------

/// AN 2.16: an output intent with `/S /GTS_PDFX` is required. A PDF/A intent
/// is not one — the twin that keeps the rule from reading any intent as the
/// right one.
#[test]
fn a_gts_pdfx_output_intent_is_required() {
    let missing = FindingKind::OutputIntentMissing {
        subtype: "GTS_PDFX".to_string(),
    };
    under_both(|fixture| fixture.intents = None, "6.2", "AN 2.16", &missing);
    under_both(
        |fixture| fixture.intent = fixture.intent.replace("GTS_PDFX", "GTS_PDFA1"),
        "6.2",
        "AN 2.16",
        &missing,
    );
}

/// AN 2.16: a registered characterization may be named by
/// `/OutputConditionIdentifier` with `/RegistryName`; otherwise
/// `/DestOutputProfile` is required. The twin is the registry form.
#[test]
fn an_intent_embeds_a_profile_or_names_a_registered_condition() {
    let registry = "/Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (CGATS TR 001) \
                    /RegistryName (http://www.color.org)";
    clean_under_both(|fixture| {
        fixture.intent = registry.to_string();
        fixture.profile = None;
    });
    under_both(
        |fixture| {
            fixture.intent = "/Type /OutputIntent /S /GTS_PDFX \
                              /OutputConditionIdentifier (CGATS TR 001)"
                .to_string();
            fixture.profile = None;
        },
        "6.2",
        "AN 2.16",
        &FindingKind::DestOutputProfileMissing,
    );
    under_both(
        |fixture| {
            fixture.intent =
                "/Type /OutputIntent /S /GTS_PDFX /RegistryName (http://www.color.org)".to_string();
            fixture.profile = None;
        },
        "6.2",
        "AN 2.16",
        &FindingKind::OutputIntentMalformed {
            key: "OutputConditionIdentifier".to_string(),
        },
    );
}

/// AN 2.16: under a CMYK output intent `DeviceRGB` is not allowed and must go
/// through a `/DefaultRGB`, and the rule reaches a `Separation`'s alternate
/// space. The twins: the same page with a `/DefaultRGB`, under an RGB
/// profile, and under an intent that embeds no profile at all.
#[test]
fn device_rgb_under_a_cmyk_intent_is_reported() {
    let rgb = FindingKind::DeviceColourNotInOutputIntent {
        space: "DeviceRGB".to_string(),
        profile: "CMYK".to_string(),
    };
    under_both(
        |fixture| fixture.content = "1 0 0 rg 20 20 100 100 re f".to_string(),
        "6.2",
        "AN 2.16",
        &rgb,
    );
    under_both(
        |fixture| {
            fixture.resources =
                "<< /ColorSpace << /Spot [/Separation /Gold /DeviceRGB 20 0 R] >> >>".to_string();
            fixture.content = "/Spot cs 1 sc 20 20 100 100 re f".to_string();
            fixture.extra.push((
                20,
                b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 0] /N 1 >>".to_vec(),
            ));
        },
        "6.2",
        "AN 2.16",
        &rgb,
    );
    clean_under_both(|fixture| {
        fixture.content = "1 0 0 rg 20 20 100 100 re f".to_string();
        fixture.resources =
            "<< /ColorSpace << /DefaultRGB [/CalRGB << /WhitePoint [0.9505 1 1.089] >>] >> >>"
                .to_string();
        // The Default space is device-independent colour, which PDF/X-3
        // admits with the embedded profile the baseline has.
    });
    clean_under_both(|fixture| {
        fixture.content = "1 0 0 rg 20 20 100 100 re f".to_string();
        fixture.profile = Some(*b"RGB ");
    });
    clean_under_both(|fixture| {
        fixture.content = "1 0 0 rg 20 20 100 100 re f".to_string();
        fixture.intent =
            "/Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (CGATS TR 001) \
                          /RegistryName (http://www.color.org)"
                .to_string();
        fixture.profile = None;
    });
}

/// AN 2.16, PDF/X-3 only: device-independent colour anywhere makes the
/// embedded profile mandatory, registry or not. The twins: the same page
/// with the profile embedded, and the same bytes claiming PDF/X-1a:2003,
/// where the rule is not the note's (and what X-1a:2003 says of an
/// `ICCBased` space is unread, by name).
#[test]
fn device_independent_colour_needs_an_embedded_profile_under_pdfx_3() {
    let registry_only = |fixture: &mut X, space: &str| {
        fixture.intent =
            "/Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (CGATS TR 001) \
                          /RegistryName (http://www.color.org)"
                .to_string();
        fixture.profile = None;
        fixture.resources = format!("<< /ColorSpace << /CS0 {space} >> >>");
        fixture.content = "/CS0 cs 0.5 0.5 0.5 sc 20 20 100 100 re f".to_string();
        fixture.extra.push((20, stream("/N 3", &profile(b"RGB "))));
    };
    for space in [
        "[/ICCBased 20 0 R]",
        "[/Lab << /WhitePoint [0.9505 1 1.089] >>]",
    ] {
        let mut x3 = X::x3();
        registry_only(&mut x3, space);
        assert_eq!(
            x3.one_finding(),
            ("AN 2.16".to_string(), FindingKind::DestOutputProfileMissing),
            "{space}"
        );
        let mut x1a = X::x1a();
        registry_only(&mut x1a, space);
        x1a.clean();

        let mut embedded = X::x3();
        registry_only(&mut embedded, space);
        embedded.intent = X::x3().intent;
        embedded.profile = Some(*b"CMYK");
        embedded.clean();
    }
}

// ---- AN 2.25 transparency -------------------------------------------------

/// AN 2.25: transparency is prohibited in the 2003 levels — read exactly as
/// ISO 19005-1 6.4 reads it, through the same function. The twins: an alpha
/// of one, and a soft mask of `/None`.
#[test]
fn transparency_is_forbidden() {
    let with_state = |state: &'static str| {
        move |fixture: &mut X| {
            fixture.resources = "<< /ExtGState << /G 20 0 R >> >>".to_string();
            fixture.content = "/G gs 0 0 0 1 k 20 20 100 100 re f".to_string();
            fixture.extra.push((20, state.as_bytes().to_vec()));
        }
    };
    for (state, feature) in [
        ("<< /Type /ExtGState /ca 0.5 >>", "ca"),
        ("<< /Type /ExtGState /BM /Multiply >>", "BM"),
    ] {
        under_both(
            with_state(state),
            "6.16",
            "AN 2.25",
            &FindingKind::TransparencyForbidden {
                feature: feature.to_string(),
            },
        );
    }
    under_both(
        |fixture| fixture.page = "/Group << /S /Transparency >>".to_string(),
        "6.16",
        "AN 2.25",
        &FindingKind::TransparencyForbidden {
            feature: "Group".to_string(),
        },
    );
    clean_under_both(with_state(
        "<< /Type /ExtGState /ca 1 /CA 1 /SMask /None >>",
    ));
}

// ---- AN 2.26 PostScript ---------------------------------------------------

/// AN 2.26: no PostScript XObject, by `/Subtype /PS` or a form's
/// `/Subtype2 /PS`, and no `PS` operator. The twin is a plain form.
#[test]
fn postscript_xobjects_and_the_ps_operator_are_forbidden() {
    let drawing = |dict: &'static str| {
        move |fixture: &mut X| {
            fixture.resources = "<< /XObject << /X0 20 0 R >> >>".to_string();
            fixture.content = "/X0 Do".to_string();
            fixture
                .extra
                .push((20, stream(dict, b"0 0 0 1 k 0 0 1 1 re f")));
        }
    };
    under_both(
        drawing("/Type /XObject /Subtype /PS"),
        "6.10",
        "AN 2.26",
        &FindingKind::PostScriptXObjectForbidden,
    );
    under_both(
        drawing("/Type /XObject /Subtype /Form /Subtype2 /PS /BBox [0 0 1 1]"),
        "6.10",
        "AN 2.26",
        &FindingKind::PostScriptXObjectForbidden,
    );
    under_both(
        |fixture| fixture.content = "0 0 0 1 k (showpage) PS 20 20 100 100 re f".to_string(),
        "6.10",
        "AN 2.26",
        &FindingKind::PostScriptOperatorForbidden,
    );
    clean_under_both(drawing("/Type /XObject /Subtype /Form /BBox [0 0 1 1]"));
}

// ---- AN 2.18 fonts --------------------------------------------------------

/// A page drawing `A` in font object 20, `font` its dictionary entries.
fn drawing_text(font: &str, mode: u8) -> impl Fn(&mut X) {
    let font = font.to_string();
    move |fixture: &mut X| {
        fixture.resources = "<< /Font << /F1 20 0 R >> >>".to_string();
        fixture.content = format!("BT /F1 12 Tf {mode} Tr 20 20 Td (A) Tj ET");
        fixture
            .extra
            .push((20, format!("<< {font} >>").into_bytes()));
        fixture.extra.push((
            21,
            b"<< /Type /FontDescriptor /FontName /Acme /Flags 32 /FontBBox [0 0 1000 1000] \
              /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 \
              /FontFile2 22 0 R >>"
                .to_vec(),
        ));
        fixture
            .extra
            .push((22, stream("/Length1 4", b"\0\x01\0\0")));
    }
}

/// AN 2.18: every font used is embedded. The PDF/A embedding rule, run for a
/// PDF/X claim with no PDF/A claim and renumbered. The twins: the same font
/// embedded, a Type 3 font (whose glyphs are content), and the unembedded
/// font used only for invisible text — the reading the font clause's staged
/// entry names.
#[test]
fn every_font_used_is_embedded() {
    let bare = "/Type /Font /Subtype /TrueType /BaseFont /Acme";
    under_both(
        drawing_text(bare, 0),
        "6.3",
        "AN 2.18",
        &FindingKind::FontNotEmbedded {
            subtype: "TrueType".to_string(),
        },
    );
    clean_under_both(drawing_text(
        "/Type /Font /Subtype /TrueType /BaseFont /Acme /FontDescriptor 21 0 R",
        0,
    ));
    clean_under_both(drawing_text(
        "/Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
         /CharProcs << >> /Encoding << /Differences [65 /a] >> /FirstChar 65 /LastChar 65 \
         /Widths [1]",
        0,
    ));
    clean_under_both(drawing_text(bare, 3));
}

// ---- the citations ----------------------------------------------------------

/// Under PDF/X-3:2003 every finding cites the application note it was
/// transcribed from, because no number of ISO 15930-6's is in hand; under
/// PDF/X-1a:2003 every finding but the private-key rule cites a clause of
/// 15930-4's contents. One document broken eight ways, read both ways.
#[test]
fn each_level_cites_what_is_in_hand_for_it() {
    let broken = |version: &str| {
        let mut fixture = X::new(version);
        fixture.info = "/Custom 3".to_string();
        fixture.trim = None;
        fixture.content = "1 0 0 rg (x) PS 20 20 100 100 re f".to_string();
        fixture.extra.push((20, stream("/Filter /LZWDecode", b"x")));
        fixture.page = "/Group << /S /Transparency >>".to_string();
        fixture
    };
    let x3 = broken("PDF/X-3:2003").findings();
    assert!(x3.len() >= 7, "{x3:#?}");
    assert!(x3.iter().all(|f| f.clause.0.starts_with("AN ")), "{x3:#?}");
    let x1a = broken("PDF/X-1a:2003").findings();
    assert_eq!(x1a.len(), x3.len());
    for finding in &x1a {
        let note = matches!(finding.kind, FindingKind::InfoValueNotText { .. });
        assert_eq!(finding.clause.0.starts_with("AN "), note, "{finding:?}");
    }
}
