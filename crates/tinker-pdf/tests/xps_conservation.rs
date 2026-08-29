//! The XPS conservation harness, and the harness's own tests (ruling 13,
//! roadmap step 4).
//!
//! `xps_support/conservation.rs` is the harness; this is where it is proved and
//! where it is run. It replaces `xps_mutool.rs`, whose four tests asked
//! `mutool draw -F trace` for a device trace of the package and a device trace
//! of the synthesised document and compared page boxes, fills, gradients,
//! glyphs and images. Every one of those comparisons is below. What is not
//! below, and does not come back, is that one of the two readings was written
//! by somebody else — `docs/verification.md` states that in its own voice.
//!
//! # An assertion that cannot fail is not an assertion
//!
//! All eight committed packages conserved on the first run of the sweep, so a
//! file that was only that sweep would pass with the comparator deleted. Three
//! legs answer that, and none of them is the sweep:
//!
//! 1. **The comparator, against censuses built by hand.** Every divergence the
//!    oracle could report is constructed here and asserted *detected*, by its
//!    own name and with its own size — a lost element, a page at the wrong
//!    size, a reversed gradient axis, a radius from the wrong stop, an image
//!    off the sheet, a run addressed through the wrong glyphs, three fills
//!    painted back to front.
//! 2. **The markup walk, against markup with something to get wrong.** Both
//!    `Data` spellings, both colour spellings, `{StaticResource}` against an
//!    inline brush, a relative reference against an absolute one, a comment
//!    between elements, a canvas composing a transform and an opacity.
//! 3. **The document census, against a document whose content is known** —
//!    built by `DocumentBuilder` rather than synthesised from a package, so the
//!    PDF side is proved on a document this test wrote.
//!
//! The corpus sweep is the fourth leg, and it asserts the two halves that are
//! true at every revision — **conservation holds, and the recorded figure** —
//! with the figure written down in `tests/xps/CONSERVATION.tsv` so a change
//! that moves it has to say so in the same commit.
//!
//! # What the third producer did to all of that
//!
//! Tier 4 added five Ghostscript packages, and they moved the sweep in three
//! ways worth naming here rather than leaving in a diff.
//!
//! **The scale.** The eight Microsoft packages state at most six facts each.
//! `gs-gradients.xps` states 1 115, `gs-rasterised-text.xps` 715 and
//! `gs-embedded-font.xps` 590, because `xpswrite` decomposes gradients and text
//! into one filled path per device unit. The comparator had never been run over
//! a page with more than three marks on it.
//!
//! **The defect it found, in this file's own scanner.** 11.2.3's fill-rule
//! prefix is `"F" wsp* ("0"|"1")`, and `data_bounds` stripped `F0` and `F1`
//! only — which is the whole of what WPF and the object model write, because
//! neither writes an `F` at all. Ghostscript writes `F 1`, the scanner met an
//! unknown command, and two of `gs-paths.xps`'s six marks censused as having no
//! bounds. The engine had it right; the independent walk did not. Corrected
//! against the clause, and it is exactly the class of hole a second producer
//! exists to find.
//!
//! **The one package the sweep cannot cover**, which is named below in a test
//! of its own rather than dropped from a list.
//!
//! # The injections that were counted before the oracle left
//!
//! Ruling 13's order is that nothing is deleted before the check replacing it
//! has been injection-counted. Each defect below was put back into the engine,
//! `cargo test -p tinker-pdf --no-fail-fast` was run, and the tests that caught
//! it were counted by name. Counts are against the 892-test package suite as it
//! stood before this file, except the third row, which is measured at 893.
//!
//! | Injected into | Caught by | Of which this file |
//! | --- | ---: | ---: |
//! | 18.1's unit scale, `0.75` to `1.0` | 29 | 1 |
//! | 18.1's flip, dropped | 6 | 1 |
//! | the painter's open scopes, composed outermost first | **1** | **1** |
//! | a `LinearGradientBrush` axis, reversed | 4 | 1 |
//! | a `RadialGradientBrush` radius, taken from the focal circle | 3 | 1 |
//! | a stated `Indices` advance, ignored | 6 | 1 |
//! | an empty `GlyphIndex` resolved to `.notdef` rather than through the `cmap` | 23 | 1 |
//! | the pages of a document, in reverse spine order | 10 | 1 |
//!
//! **The third row is why the exercise is run rather than argued.** Composing
//! the open scopes outermost-first was caught by **nothing at all** — 0 of 892
//! — because every `RenderTransform` in all eight committed packages is a
//! translation and two translations commute, and because the outermost scope is
//! the page, whose transform is the identity, so one canvas is not enough to
//! part the two orders either. The fixture that closes it —
//! `a_canvas_scale_over_a_child_translation_composes_the_child_first` — is two
//! nested canvases that do not commute with an image brush under them, and it
//! is the only assertion in the suite that fires.
//!
//! The other seven were caught by the suite before this file as well, which is
//! the honest reading of the table: what conservation adds is breadth over the
//! real packages and one hole nothing else covered, not sole custody of seven
//! defect classes.

mod xps_support;

use tinker_pdf::{DeviceSpace, Document, DocumentBuilder, Function, ImageData, Shading};
use xps_support::conservation::{
    conservation, conserve, document_census, markup_census, Census, Divergence, Gradient, Mark,
    PageCensus, Paint, Rect, Run,
};
use xps_support::{
    archive, before_content_types, binary_part, document, fixed_page_with, grey_jpeg,
    one_page_package, rgb_png, with, XPS_NS,
};

// ---- fixtures ---------------------------------------------------------------

fn corpus(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// A package whose one 816 x 1056 fixed page carries `body`.
fn package(body: &str) -> Vec<u8> {
    package_with_resources("", body)
}

/// The same, with a resource dictionary in front of the body.
fn package_with_resources(resources: &str, body: &str) -> Vec<u8> {
    let dictionary = if resources.is_empty() {
        String::new()
    } else {
        format!(
            "<FixedPage.Resources><ResourceDictionary>{resources}\
             </ResourceDictionary></FixedPage.Resources>"
        )
    };
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{dictionary}{body}</FixedPage>"#
    );
    archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &markup,
    ))
}

/// The resource-dictionary-key namespace all six `wpf-` packages carry.
const KEY_NS: &str = "http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key";

/// The one page of a package, censused from its markup.
fn one_page(bytes: &[u8]) -> PageCensus {
    let census = markup_census(bytes);
    assert_eq!(census.pages.len(), 1, "one page: {census:?}");
    census.pages.into_iter().next().expect("one page")
}

fn solid(rgb: [f64; 3], bounds: Rect) -> Mark {
    Mark {
        paint: Paint::Solid { rgb },
        bounds,
        alpha: 1.0,
    }
}

fn one(marks: Vec<Mark>, runs: Vec<Run>) -> Census {
    Census {
        pages: vec![PageCensus {
            size: (612.0, 792.0),
            marks,
            runs,
        }],
    }
}

/// The three colours `make-corpus.ps1` painted the committed packages with,
/// written as the bytes the markup carries over 255 rather than as decimals.
///
/// A decimal here would be a *rounded* colour, and the markup walk states an
/// exact one — so an equality assertion on a paint would be comparing this
/// file's rounding rather than the walk's reading.
const RED: [f64; 3] = [220.0 / 255.0, 20.0 / 255.0, 60.0 / 255.0];
const GREEN: [f64; 3] = [46.0 / 255.0, 139.0 / 255.0, 87.0 / 255.0];
const BLUE: [f64; 3] = [25.0 / 255.0, 25.0 / 255.0, 112.0 / 255.0];

// ---- leg 1: the comparator, against censuses built by hand ------------------

/// A document that carries what the markup states conserves every fact.
#[test]
fn a_document_that_carries_the_markup_conserves_every_fact() {
    let census = one(
        vec![
            solid(RED, Rect::of(0.0, 0.0, 10.0, 10.0)),
            solid(GREEN, Rect::of(20.0, 0.0, 30.0, 10.0)),
        ],
        vec![Run {
            origin: (75.0, 492.0),
            em: 18.0,
            text: "Page one".to_owned(),
            glyphs: 8,
            rgb: [0.0, 0.0, 0.0],
            advances: Vec::new(),
        }],
    );
    let verdict = conserve(&census, &census);
    assert!(verdict.holds(), "{verdict:?}");
    // One fact for the page, one per mark, one per run.
    assert_eq!(verdict.figure(), (4, 4));
}

/// **A page at the wrong size.** The original defect, which is what
/// `xps_mutool.rs` opened with: before milestone 3 a package opened as a
/// one-page comic whose page was 32 x 32 points.
#[test]
fn a_page_at_the_wrong_size_is_reported_with_both_sizes() {
    let markup = one(Vec::new(), Vec::new());
    let mut document = markup.clone();
    document.pages[0].size = (32.0, 32.0);

    let verdict = conserve(&markup, &document);
    assert!(!verdict.holds());
    assert_eq!(verdict.conserved, 0, "the page is the only fact");
    assert!(
        matches!(
            verdict.divergences.first(),
            Some(Divergence::PageSize { markup, document, .. })
                if *markup == (612.0, 792.0) && *document == (32.0, 32.0)
        ),
        "{:?}",
        verdict.divergences
    );
}

/// **An element the document lost**, which is what an unimplemented brush or a
/// path that failed to read looks like from outside.
#[test]
fn an_element_the_document_lost_is_a_mark_count() {
    let markup = one(
        vec![
            solid(RED, Rect::of(0.0, 0.0, 10.0, 10.0)),
            solid(GREEN, Rect::of(20.0, 0.0, 30.0, 10.0)),
            solid(BLUE, Rect::of(40.0, 0.0, 50.0, 10.0)),
        ],
        Vec::new(),
    );
    let mut document = markup.clone();
    document.pages[0].marks.pop();

    let verdict = conserve(&markup, &document);
    assert!(!verdict.holds());
    assert!(
        verdict.divergences.iter().any(|d| matches!(
            d,
            Divergence::MarkCount {
                markup: 3,
                document: 2,
                ..
            }
        )),
        "{:?}",
        verdict.divergences
    );
    // The two that are there are still conserved, and the page is: a lost
    // element is one missing fact rather than a page nobody can measure.
    assert_eq!(verdict.figure(), (3, 4));
}

/// **An element nobody wrote**, which is the other direction and needs its own
/// assertion: a comparator that only walked the markup would call this page
/// perfect.
#[test]
fn an_element_the_document_invented_is_a_mark_count() {
    let markup = one(vec![solid(RED, Rect::of(0.0, 0.0, 10.0, 10.0))], Vec::new());
    let mut document = markup.clone();
    document.pages[0]
        .marks
        .push(solid(BLUE, Rect::of(0.0, 0.0, 10.0, 10.0)));

    let verdict = conserve(&markup, &document);
    assert!(!verdict.holds());
    assert!(
        verdict.divergences.iter().any(|d| matches!(
            d,
            Divergence::MarkCount {
                markup: 1,
                document: 2,
                ..
            }
        )),
        "{:?}",
        verdict.divergences
    );
}

/// **Three fills painted back to front.** Every colour is present, every
/// rectangle is where it should be, and the page is wrong — which is why the
/// comparison is in order rather than as a multiset.
#[test]
fn three_fills_painted_back_to_front_are_not_conserved() {
    let markup = one(
        vec![
            solid(RED, Rect::of(0.0, 0.0, 10.0, 10.0)),
            solid(GREEN, Rect::of(20.0, 0.0, 30.0, 10.0)),
            solid(BLUE, Rect::of(40.0, 0.0, 50.0, 10.0)),
        ],
        Vec::new(),
    );
    let mut document = markup.clone();
    document.pages[0].marks.reverse();

    let verdict = conserve(&markup, &document);
    assert!(!verdict.holds(), "a reversed page is not the same page");
    // The middle one is in the same place either way, so exactly two of the
    // three diverge — a count, not merely "something was reported".
    let colours = verdict
        .divergences
        .iter()
        .filter(|d| matches!(d, Divergence::Colour { .. }))
        .count();
    assert_eq!(colours, 2, "{:?}", verdict.divergences);
}

/// **A colour that moved**, and one that did not.
///
/// Both directions, because a tolerance that catches everything and a tolerance
/// that catches nothing both pass a one-sided test. The size is one part in ten
/// thousand and the reason is the one `xps_mutool.rs` measured: the writer
/// states four decimal places where the markup states a byte.
#[test]
fn a_colour_that_moved_is_reported_and_one_that_rounded_is_not() {
    let markup = one(vec![solid(RED, Rect::of(0.0, 0.0, 10.0, 10.0))], Vec::new());

    let mut rounded = markup.clone();
    rounded.pages[0].marks[0].paint = Paint::Solid {
        rgb: [0.8627, 0.0784, 0.2353],
    };
    assert!(
        conserve(&markup, &rounded).holds(),
        "four decimal places is the same colour as a byte"
    );

    let mut moved = markup.clone();
    moved.pages[0].marks[0].paint = Paint::Solid {
        rgb: [0.85, 0.078_431, 0.235_294],
    };
    let verdict = conserve(&markup, &moved);
    assert!(!verdict.holds());
    assert!(
        matches!(verdict.divergences.first(), Some(Divergence::Colour { .. })),
        "{:?}",
        verdict.divergences
    );
}

/// **A gradient whose axis is reversed.** The page is still a gradient and
/// still looks plausible; the numbers are what is not plausible-looking, which
/// is `xps_mutool.rs`'s own reason for comparing them.
#[test]
fn a_gradient_axis_the_wrong_way_round_is_reported() {
    let gradient = |geometry: Vec<f64>| Mark {
        paint: Paint::Gradient {
            kind: Gradient::Linear,
            geometry,
            stops: vec![(0.0, RED), (1.0, GREEN)],
        },
        bounds: Rect::of(0.0, 0.0, 400.0, 200.0),
        alpha: 1.0,
    };
    let markup = one(vec![gradient(vec![0.0, 0.0, 400.0, 200.0])], Vec::new());
    let document = one(vec![gradient(vec![400.0, 200.0, 0.0, 0.0])], Vec::new());

    let verdict = conserve(&markup, &document);
    assert!(!verdict.holds());
    assert!(
        matches!(
            verdict.divergences.first(),
            Some(Divergence::GradientGeometry { .. })
        ),
        "{:?}",
        verdict.divergences
    );
}

/// **A radial gradient is a different shading from an axial one**, and a
/// comparator that counted gradients would not know.
#[test]
fn a_radial_gradient_read_as_an_axial_one_is_a_paint_kind() {
    let markup = one(
        vec![Mark {
            paint: Paint::Gradient {
                kind: Gradient::Radial,
                geometry: vec![120.0, 120.0, 0.0, 150.0, 150.0, 150.0],
                stops: vec![(0.0, RED), (1.0, BLUE)],
            },
            bounds: Rect::of(0.0, 0.0, 300.0, 300.0),
            alpha: 1.0,
        }],
        Vec::new(),
    );
    let mut document = markup.clone();
    document.pages[0].marks[0].paint = Paint::Gradient {
        kind: Gradient::Linear,
        geometry: vec![120.0, 120.0, 0.0, 150.0, 150.0, 150.0],
        stops: vec![(0.0, RED), (1.0, BLUE)],
    };

    let verdict = conserve(&markup, &document);
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::PaintKind { .. })
    ));
}

/// **A stop the document lost, and a radius taken from the wrong one.**
///
/// A three-stop ramp read as two is a gradient that still runs from the right
/// colour to the right colour, past the wrong middle.
#[test]
fn a_gradient_that_lost_a_stop_is_reported_with_both_stop_lists() {
    let markup = one(
        vec![Mark {
            paint: Paint::Gradient {
                kind: Gradient::Linear,
                geometry: vec![0.0, 0.0, 400.0, 200.0],
                stops: vec![(0.0, RED), (0.5, GREEN), (1.0, BLUE)],
            },
            bounds: Rect::of(0.0, 0.0, 400.0, 200.0),
            alpha: 1.0,
        }],
        Vec::new(),
    );
    let mut document = markup.clone();
    if let Paint::Gradient { stops, .. } = &mut document.pages[0].marks[0].paint {
        stops.remove(1);
    }

    let verdict = conserve(&markup, &document);
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::Stops { markup, document, .. })
            if markup.len() == 3 && document.len() == 2
    ));
}

/// **An image at a different pixel count**, which is the pass-through claim:
/// the part reaches the page at the size it already was, never resampled.
#[test]
fn an_image_resampled_on_the_way_is_a_pixel_count() {
    let image = |pixels: (u32, u32)| Mark {
        paint: Paint::Image {
            pixels,
            tiled: false,
            copies: 1,
            area: Rect::of(75.0, 567.0, 225.0, 717.0),
        },
        bounds: Rect::of(75.0, 567.0, 225.0, 717.0),
        alpha: 1.0,
    };
    let verdict = conserve(
        &one(vec![image((32, 32))], Vec::new()),
        &one(vec![image((150, 150))], Vec::new()),
    );
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::Pixels {
            markup: (32, 32),
            document: (150, 150),
            ..
        })
    ));
}

/// **The image off the sheet.** Gap 30 milestone 8's own defect: an
/// `ImageBrush` matrix composed outermost first put the picture 4 336 points
/// down a 792-point page, which reads exactly like a feature that is not
/// finished yet and is a disagreement about a rectangle.
#[test]
fn an_image_off_the_page_is_a_placement_with_both_rectangles() {
    let image = |area: Rect| Mark {
        paint: Paint::Image {
            pixels: (32, 32),
            tiled: false,
            copies: 1,
            area,
        },
        bounds: area,
        alpha: 1.0,
    };
    let wanted = Rect::of(75.0, 567.0, 225.0, 717.0);
    let sunk = Rect::of(75.0, -4_336.0, 225.0, -4_186.0);
    let verdict = conserve(
        &one(vec![image(wanted)], Vec::new()),
        &one(vec![image(sunk)], Vec::new()),
    );
    assert!(!verdict.holds());
    assert!(
        verdict
            .divergences
            .iter()
            .any(|d| matches!(d, Divergence::Placement { .. })),
        "{:?}",
        verdict.divergences
    );
}

/// **A tile that lost its reflections.** `TileMode="FlipXY"` holds four copies
/// of the picture in one cell; a build that tiled it plainly draws a page that
/// is covered edge to edge and is not the page the markup asked for.
#[test]
fn a_flipping_tile_that_lost_its_reflections_is_a_copy_count() {
    let tile = |copies: usize| Mark {
        paint: Paint::Image {
            pixels: (32, 32),
            tiled: true,
            copies,
            area: Rect::of(75.0, 259.5, 225.0, 372.0),
        },
        bounds: Rect::of(75.0, 147.0, 375.0, 372.0),
        alpha: 0.5,
    };
    let verdict = conserve(
        &one(vec![tile(4)], Vec::new()),
        &one(vec![tile(1)], Vec::new()),
    );
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::Copies {
            markup: 4,
            document: 1,
            ..
        })
    ));
}

/// **An opacity that did not reach the page.** Nothing about the geometry
/// changes and the picture is drawn at full strength.
#[test]
fn an_opacity_that_did_not_reach_the_page_is_an_alpha() {
    let mut markup = one(vec![solid(RED, Rect::of(0.0, 0.0, 10.0, 10.0))], Vec::new());
    markup.pages[0].marks[0].alpha = 0.5;
    let document = one(vec![solid(RED, Rect::of(0.0, 0.0, 10.0, 10.0))], Vec::new());

    let verdict = conserve(&markup, &document);
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::Alpha { .. })
    ));
}

/// **A run addressed through the wrong glyphs.** `Indices=",53"` names a glyph
/// by index, and the only way a PDF addresses one is 9.7's Type 0 font with
/// `/Identity-H`; a build that went through the face's own `cmap` instead draws
/// letters that look right and are different glyphs. What that costs the census
/// is the glyph count when the two disagree in length, and the text when they
/// do not.
#[test]
fn a_run_that_lost_glyphs_and_one_that_changed_letters_are_both_reported() {
    let run = |text: &str, glyphs: usize| Run {
        origin: (75.0, 492.0),
        em: 18.0,
        text: text.to_owned(),
        glyphs,
        rgb: [0.0, 0.0, 0.0],
        advances: Vec::new(),
    };
    let markup = one(Vec::new(), vec![run("Page one", 8)]);

    let verdict = conserve(&markup, &one(Vec::new(), vec![run("Page one", 7)]));
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::GlyphCount {
            markup: 8,
            document: 7,
            ..
        })
    ));

    let verdict = conserve(&markup, &one(Vec::new(), vec![run("Rage one", 8)]));
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::Text { .. })
    ));

    // And a line break where the markup had a space is **not** a divergence:
    // the run states its own spacing and an extractor decides where a line
    // ends, so whitespace is the one thing that cannot be conserved.
    assert!(conserve(&markup, &one(Vec::new(), vec![run("Page\none", 8)])).holds());
}

/// **A run at the wrong place, and one at the wrong size.**
///
/// Two independent facts: 12.1's `Origin` puts the run somewhere and its
/// `FontRenderingEmSize` decides how big it is, and a build that scaled the
/// page instead of the text would get one right and the other wrong.
#[test]
fn a_run_at_the_wrong_origin_and_one_at_the_wrong_size_are_separate() {
    let run = |origin: (f64, f64), em: f64| Run {
        origin,
        em,
        text: "Page one".to_owned(),
        glyphs: 8,
        rgb: [0.0, 0.0, 0.0],
        advances: Vec::new(),
    };
    let markup = one(Vec::new(), vec![run((75.0, 492.0), 18.0)]);

    let moved = conserve(&markup, &one(Vec::new(), vec![run((100.0, 400.0), 18.0)]));
    assert!(matches!(
        moved.divergences.first(),
        Some(Divergence::Origin { .. })
    ));

    let resized = conserve(&markup, &one(Vec::new(), vec![run((75.0, 492.0), 24.0)]));
    assert!(matches!(
        resized.divergences.first(),
        Some(Divergence::EmSize { .. })
    ));
}

/// **A stated advance the document dropped.**
///
/// 12.1.3 lets a cluster override the advance the face would give, and
/// `Indices=",53"` — which every text run in the committed corpus carries —
/// overrides exactly one. A build that ignored it draws the same eight letters
/// of the same face at the same size, and puts seven of them in the wrong
/// place. Nothing about the page reads as broken.
#[test]
fn a_stated_advance_the_document_dropped_is_reported_per_glyph() {
    let run = |advance: f64| Run {
        origin: (75.0, 492.0),
        em: 18.0,
        text: "Page one".to_owned(),
        glyphs: 2,
        rgb: [0.0, 0.0, 0.0],
        advances: vec![Some(advance), None],
    };
    // 53 hundredths of a 24-unit em is 9.54 points; the face's own 586
    // thousandths is 10.548, which is what a build that dropped the override
    // uses.
    let verdict = conserve(
        &one(Vec::new(), vec![run(9.54)]),
        &one(Vec::new(), vec![run(10.548)]),
    );
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::Advance { glyph: 0, .. })
    ));

    // And a cluster that states nothing is not compared, because the width it
    // takes is the face's fact rather than the document's.
    let mut stated = one(Vec::new(), vec![run(9.54)]);
    let mut drawn = one(Vec::new(), vec![run(9.54)]);
    stated.pages[0].runs[0].advances[1] = None;
    drawn.pages[0].runs[0].advances[1] = Some(1_000.0);
    assert!(conserve(&stated, &drawn).holds());
}

/// **A page the document does not have at all**, which is a spine that lost a
/// `PageContent` rather than a page that drew badly.
#[test]
fn a_page_the_document_lost_is_a_page_count() {
    let markup = Census {
        pages: vec![PageCensus::default(), PageCensus::default()],
    };
    let verdict = conserve(
        &markup,
        &Census {
            pages: vec![PageCensus::default()],
        },
    );
    assert!(matches!(
        verdict.divergences.first(),
        Some(Divergence::PageCount {
            markup: 2,
            document: 1
        })
    ));
}

// ---- leg 2: the markup walk, against markup with something to get wrong -----

/// **18.1, applied once.** A path at a known place in a known page comes back
/// at points the test computes from the clause rather than from the reader:
/// 816 x 1056 units is 612 x 792 points, and a rectangle 120 units down the
/// page has its *top* 120 x 0.75 points below the top edge.
#[test]
fn the_unit_scale_and_the_flip_are_each_applied_exactly_once() {
    let page = one_page(&package(
        r##"<Path Fill="#FF000000" RenderTransform="1,0,0,1,100,120" Data="M0,0L300,0 300,4 0,4Z" />"##,
    ));
    assert_eq!(page.size, (612.0, 792.0));
    let bounds = page.marks[0].bounds;
    assert_eq!(bounds.x0, 75.0, "100 units from the left is 75 points");
    assert_eq!(bounds.x1, 300.0, "and 400 units is 300");
    assert_eq!(bounds.y1, 792.0 - 90.0, "120 units down is 90 points down");
    assert_eq!(bounds.y0, 792.0 - 93.0, "and the rectangle is 4 units tall");
}

/// **Both `Data` spellings bound the same rectangle.** WPF writes
/// `M0,0L200,0 200,100 0,100Z` and the XPS Object Model writes the same figure
/// with spaces around every command, and 11.2.3 makes them the same geometry.
#[test]
fn both_data_spellings_bound_the_same_rectangle() {
    let compact = one_page(&package(
        r##"<Path Fill="#FF000000" Data="M0,0L200,0 200,100 0,100Z" />"##,
    ));
    let spaced = one_page(&package(
        r##"<Path Fill="#FF000000" Data="M 0,0 L 200,0 200,100 0,100 Z" />"##,
    ));
    assert_eq!(compact.marks[0].bounds, spaced.marks[0].bounds);
    assert_eq!(compact.marks[0].bounds, Rect::of(0.0, 717.0, 150.0, 792.0));
}

/// **Both colour spellings are the same colour**, and the alpha digits are not
/// silently read as red.
#[test]
fn both_colour_spellings_are_the_same_colour() {
    let eight = one_page(&package(
        r##"<Path Fill="#FFDC143C" Data="M0,0L10,0 10,10 0,10Z" />"##,
    ));
    let six = one_page(&package(
        r##"<Path Fill="#dc143c" Data="M0,0L10,0 10,10 0,10Z" />"##,
    ));
    assert_eq!(eight.marks[0].paint, six.marks[0].paint);
    assert_eq!(eight.marks[0].paint, Paint::Solid { rgb: RED });
    assert_eq!(eight.marks[0].alpha, 1.0, "FF is opaque, not 255 of red");
}

/// **A `{StaticResource}` and an inline brush state the same brush.** One
/// indirection is what every real package in the corpus uses and none of the
/// hand-built fixtures did.
#[test]
fn a_static_resource_and_an_inline_brush_state_the_same_brush() {
    let keyed = one_page(&package_with_resources(
        r##"<SolidColorBrush x:Key="b0" Color="#FFDC143C" />"##,
        r##"<Path Fill="{StaticResource b0}" Data="M0,0L10,0 10,10 0,10Z" />"##,
    ));
    let inline = one_page(&package(concat!(
        r##"<Path Data="M0,0L10,0 10,10 0,10Z"><Path.Fill>"##,
        r##"<SolidColorBrush Color="#FFDC143C" /></Path.Fill></Path>"##,
    )));
    assert_eq!(keyed.marks[0].paint, Paint::Solid { rgb: RED });
    assert_eq!(keyed.marks[0].paint, inline.marks[0].paint);

    // And a key nothing defines is not a mark at all, rather than a mark
    // painted in whatever the last colour was.
    let dangling = one_page(&package(
        r##"<Path Fill="{StaticResource nothing}" Data="M0,0L10,0 10,10 0,10Z" />"##,
    ));
    assert!(dangling.marks.is_empty(), "{dangling:?}");
}

/// **A comment between elements is not an element.** Every page the XPS Object
/// Model writes carries one, and a scanner that read attributes out of it would
/// census markup nobody drew.
#[test]
fn a_comment_between_elements_is_not_an_element() {
    let page = one_page(&package(concat!(
        r##"<!-- Generated by: Microsoft XPS Object Model, Version: 1.0 -->"##,
        r##"<Path Fill="#FFDC143C" Data="M0,0L10,0 10,10 0,10Z" />"##,
        r##"<!-- <Path Fill="#FF000000" Data="M0,0L99,0 99,99 0,99Z" /> -->"##,
    )));
    assert_eq!(page.marks.len(), 1, "{page:?}");
    assert_eq!(page.marks[0].paint, Paint::Solid { rgb: RED });
}

/// **A canvas composes its transform and multiplies its opacity**, and a child
/// of one carries both.
#[test]
fn a_canvas_composes_its_transform_and_multiplies_its_opacity() {
    let page = one_page(&package(concat!(
        r##"<Canvas RenderTransform="1,0,0,1,100,100" Opacity="0.5">"##,
        r##"<Path Fill="#FFDC143C" RenderTransform="1,0,0,1,20,20" Data="M0,0L40,0 40,40 0,40Z" />"##,
        r##"</Canvas>"##,
    )));
    assert_eq!(page.marks.len(), 1);
    // 100 + 20 units across is 90 points, and the flip puts the top of the
    // rectangle 90 points below the top edge.
    assert_eq!(
        page.marks[0].bounds,
        Rect::of(90.0, 792.0 - 120.0, 120.0, 792.0 - 90.0)
    );
    assert_eq!(page.marks[0].alpha, 0.5);
}

/// **A canvas scale over a child translation composes the child first**, end
/// to end through the whole pipeline.
///
/// The one composition order in this format that does not commute, and the
/// reason this fixture exists rather than reusing the corpus: every
/// `RenderTransform` in all eight committed packages is a **translation**, and
/// two translations compose to the same matrix either way round. So a build
/// that composed its open scopes outermost-first drew every committed package
/// correctly — measured, by injecting exactly that and watching the whole
/// suite stay green.
///
/// **The brush is load-bearing and the plain path is not.** A `<Canvas>`
/// writes its own `cm` and its children draw inside it, so a solid fill
/// reaches the page through the content stream's own nesting whatever the
/// composition code does. A brush cannot: 8.7.3.1 puts a pattern matrix in the
/// page's *default* space, so the painter composes every open scope by hand,
/// and that is the code the order belongs to. This fixture carries both,
/// because the two routes are two claims.
///
/// **And the canvases are nested two deep, which is also measured rather than
/// assumed.** The outermost open scope is the page itself and its transform is
/// the identity, so with a single canvas the two orders compose to the same
/// matrix and the defect is invisible — injecting it against a one-canvas
/// fixture left the whole suite green. Two canvases that do not commute are
/// the smallest fixture that can tell them apart.
///
/// Child first: the viewport moves by `(50,50)` and then doubles, so `(0,0)`
/// lands at `(100,100)` and 18.1 puts it at `(75, 717)`. Outermost first: it
/// doubles to `(0,0)` and then moves to `(50,50)`, landing at `(37.5, 754.5)`
/// — a picture half the size in the wrong corner, which is gap 30 milestone
/// 8's own defect in miniature.
#[test]
fn a_canvas_scale_over_a_child_translation_composes_the_child_first() {
    let markup = format!(
        r##"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056"><FixedPage.Resources><ResourceDictionary><ImageBrush x:Key="b0" ViewboxUnits="Absolute" ViewportUnits="Absolute" Viewbox="0,0,4,4" Viewport="0,0,100,100" TileMode="None" ImageSource="/Resources/p.png" /></ResourceDictionary></FixedPage.Resources><Canvas RenderTransform="2,0,0,2,0,0"><Canvas RenderTransform="1,0,0,1,50,50"><Path Fill="{{StaticResource b0}}" Data="M0,0L100,0 100,100 0,100Z" /><Path Fill="#FFDC143C" RenderTransform="1,0,0,1,0,150" Data="M0,0L100,0 100,50 0,50Z" /></Canvas></Canvas></FixedPage>"##
    );
    let parts = before_content_types(
        with(one_page_package(), "Documents/1/Pages/1.fpage", &markup),
        binary_part("Resources/p.png", rgb_png(4, 4, &[128; 4 * 4 * 3])),
    );
    let bytes = archive(parts);

    let page = one_page(&bytes);
    assert_eq!(page.marks.len(), 2, "{page:?}");
    assert!(
        matches!(
            page.marks[0].paint,
            Paint::Image { area, .. } if area == Rect::of(75.0, 567.0, 225.0, 717.0)
        ),
        "the child's translation runs before the canvas's scale: {:?}",
        page.marks[0].paint
    );

    // And the document agrees, which is the half a census of the markup alone
    // cannot say.
    let verdict = conservation(&bytes);
    assert!(
        verdict.holds(),
        "the document composes the two transforms differently: {:?}",
        verdict.divergences
    );
}

/// **A relative fixed-representation target resolves against the package**,
/// not against the folder the relationships part lives in.
///
/// OPC 8.1.1.1 resolves a target against its **source part**, and the source of
/// `/_rels/.rels` is the package itself. WPF writes this absolute and the XPS
/// Object Model writes it relative, so a harness that got this wrong reads one
/// dialect and finds no pages at all in the other — which is exactly what the
/// first draft of this walk did, on both committed `.oxps` files.
#[test]
fn a_relative_fixed_representation_target_resolves_against_the_package() {
    let relative = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="{XPS_NS}/fixedrepresentation" Target="FixedDocumentSequence.fdseq" Id="R0" /></Relationships>"#
    );
    let parts = with(one_page_package(), "_rels/.rels", &relative);
    let census = markup_census(&archive(parts));
    assert_eq!(census.pages.len(), 1, "{census:?}");
    assert_eq!(census.pages[0].size, (612.0, 792.0));
}

/// **A page that states its own size states it**, which is what a corpus of
/// eight Letter-sized packages cannot check.
#[test]
fn a_page_states_its_own_size_and_not_a_letter_one() {
    let parts = with(
        with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage", "Pages/2.fpage"]),
        ),
        "Documents/1/Pages/1.fpage",
        &fixed_page_with("400", "600", ""),
    );
    let parts = with(
        parts,
        "Documents/1/Pages/2.fpage",
        &fixed_page_with("200", "300", ""),
    );
    let census = markup_census(&archive(parts));
    assert_eq!(census.pages.len(), 2);
    assert_eq!(census.pages[0].size, (300.0, 450.0));
    assert_eq!(census.pages[1].size, (150.0, 225.0));
}

/// **A PNG and a JPEG each state their own pixels**, read from the part rather
/// than from a decoder — which is the half that makes "the picture reaches the
/// page at the size it already was" two readings instead of one.
#[test]
fn a_png_and_a_jpeg_each_state_their_own_pixels() {
    for (name, bytes, wanted) in [
        ("Resources/p.png", rgb_png(7, 5, &[255; 7 * 5 * 3]), (7, 5)),
        ("Resources/p.jpg", grey_jpeg(24, 16), (24, 16)),
    ] {
        let source = format!("/{name}");
        let markup = format!(
            r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">\
               <FixedPage.Resources><ResourceDictionary>\
               <ImageBrush x:Key="b0" ViewboxUnits="Absolute" ViewportUnits="Absolute" \
               Viewbox="0,0,32,32" Viewport="0,0,200,200" TileMode="None" ImageSource="{source}" />\
               </ResourceDictionary></FixedPage.Resources>\
               <Path Fill="{{StaticResource b0}}" Data="M0,0L200,0 200,200 0,200Z" /></FixedPage>"#
        );
        let parts = before_content_types(
            with(
                one_page_package(),
                "Documents/1/Pages/1.fpage",
                &markup.replace('\\', ""),
            ),
            binary_part(name, bytes),
        );
        let parts = with(
            parts,
            "[Content_Types].xml",
            &xps_support::content_types_with(
                r#"<Default Extension="jpg" ContentType="image/jpeg" />"#,
            ),
        );
        let page = one_page(&archive(parts));
        assert_eq!(page.marks.len(), 1, "{name}: {page:?}");
        assert!(
            matches!(page.marks[0].paint, Paint::Image { pixels, .. } if pixels == wanted),
            "{name}: {:?}",
            page.marks[0].paint
        );
    }
}

/// **A tile mode states how many copies one cell holds.** 15.3's four modes
/// differ only in that number, and a census that recorded the mode as a string
/// would compare a word rather than a picture.
#[test]
fn a_tile_mode_states_how_many_copies_one_cell_holds() {
    let png = rgb_png(4, 4, &[128; 4 * 4 * 3]);
    for (mode, copies, tiled) in [
        ("None", 1, false),
        ("Tile", 1, true),
        ("FlipX", 2, true),
        ("FlipXY", 4, true),
    ] {
        let markup = format!(
            r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056"><FixedPage.Resources><ResourceDictionary><ImageBrush x:Key="b0" ViewboxUnits="Absolute" ViewportUnits="Absolute" Viewbox="0,0,4,4" Viewport="0,0,40,40" TileMode="{mode}" ImageSource="/Resources/p.png" /></ResourceDictionary></FixedPage.Resources><Path Fill="{{StaticResource b0}}" Data="M0,0L200,0 200,200 0,200Z" /></FixedPage>"#
        );
        let parts = before_content_types(
            with(one_page_package(), "Documents/1/Pages/1.fpage", &markup),
            binary_part("Resources/p.png", png.clone()),
        );
        let page = one_page(&archive(parts));
        assert!(
            matches!(
                page.marks[0].paint,
                Paint::Image { copies: c, tiled: t, .. } if c == copies && t == tiled
            ),
            "{mode}: {:?}",
            page.marks[0].paint
        );
    }
}

/// **A glyph run states its origin, its size and its text**, each read from the
/// markup rather than from the run this engine drew.
#[test]
fn a_glyph_run_states_its_origin_its_size_and_its_text() {
    let page = one_page(&package(
        r##"<Glyphs OriginX="100" OriginY="400" FontRenderingEmSize="24" FontUri="/Resources/f.odttf" UnicodeString="Page one" Indices=",53" Fill="#FF000000" />"##,
    ));
    assert_eq!(page.runs.len(), 1, "{page:?}");
    let run = &page.runs[0];
    assert_eq!(run.origin, (75.0, 492.0), "18.1 on both axes");
    assert_eq!(run.em, 18.0, "24 units of em is 18 points");
    assert_eq!(run.text, "Page one");
    assert_eq!(run.glyphs, 8, "eight characters, one glyph each");
    // `Indices=",53"` states the first cluster's advance and no other: 53
    // hundredths of the 24-unit em, which is 9.54 points at 18.1's scale.
    let stated = run.advances[0].expect("the first cluster states an advance");
    assert!(
        (stated - 53.0 / 100.0 * 24.0 * 0.75).abs() < 1e-9,
        "53 hundredths of a 24-unit em is 9.54 points: {stated}"
    );
    assert!(
        run.advances[1..].iter().all(Option::is_none),
        "the other seven take the face's own width: {:?}",
        run.advances
    );
}

// ---- leg 3: the document census, against a document whose content is known --

/// The half a sweep over synthesised documents never exercises on its own.
///
/// A `DocumentBuilder` page carrying a known fill, a known shading, a known
/// image and a known run, censused by the same [`document_census`] the sweep
/// uses. A build that read the content stream backwards, lost the shading
/// behind its clip, or took an image rectangle from the wrong matrix would pass
/// every assertion in this file without it — because both sides of the sweep
/// would move together.
#[test]
fn the_document_census_reads_a_document_this_test_wrote() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 6,
            height: 3,
            data: &[200; 6 * 3 * 3],
        }
    ));
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [10.0, 0.0, 50.0, 0.0],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![1.0, 0.0, 0.0],
                c1: vec![0.0, 0.0, 1.0],
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    builder.add_page(200.0, 100.0, |page| {
        page.set_fill_rgb(0.25, 0.5, 0.75);
        page.raw(b"10 10 30 20 re f");
        page.raw(b"q 60 10 40 20 re W n");
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
        page.image(b"Im0", 120.0, 10.0, 60.0, 30.0);
        page.text(b"F1", 12.0, 20.0, 60.0, "HELLO");
    });

    let document = Document::open(builder.finish()).expect("a document");
    let census = document_census(&document);
    assert_eq!(census.pages.len(), 1);
    let page = &census.pages[0];
    assert_eq!(page.size, (200.0, 100.0));

    // Three marks, in the order they were drawn.
    assert_eq!(page.marks.len(), 3, "{page:?}");
    assert_eq!(
        page.marks[0].paint,
        Paint::Solid {
            rgb: [0.25, 0.5, 0.75]
        }
    );
    assert_eq!(page.marks[0].bounds, Rect::of(10.0, 10.0, 40.0, 30.0));
    assert!(
        matches!(
            &page.marks[1].paint,
            Paint::Gradient { kind: Gradient::Linear, geometry, stops }
                if *geometry == vec![10.0, 0.0, 50.0, 0.0] && stops.len() == 2
        ),
        "{:?}",
        page.marks[1].paint
    );
    assert_eq!(
        page.marks[1].bounds,
        Rect::of(60.0, 10.0, 100.0, 30.0),
        "a shading is painted over its clip"
    );
    assert!(
        matches!(
            page.marks[2].paint,
            Paint::Image { pixels: (6, 3), area, .. } if area == Rect::of(120.0, 10.0, 180.0, 40.0)
        ),
        "{:?}",
        page.marks[2].paint
    );

    // And the run, with its text through the `/ToUnicode` road.
    assert_eq!(page.runs.len(), 1, "{page:?}");
    assert_eq!(page.runs[0].glyphs, 5);
    assert_eq!(page.runs[0].origin, (20.0, 60.0));
    assert_eq!(page.runs[0].em, 12.0);
    assert_eq!(page.runs[0].text, "HELLO");
}

/// And the pages come back **in page order**, which a one-page fixture cannot
/// say and a document read backwards would pass.
#[test]
fn the_document_census_reads_the_pages_in_page_order() {
    let mut builder = DocumentBuilder::new();
    for (index, grey) in [0.0_f64, 0.5, 1.0].into_iter().enumerate() {
        builder.add_page(100.0, 100.0, |page| {
            page.fill_rect(10.0, 10.0, 10.0 * (index as f64 + 1.0), 10.0, grey);
        });
    }
    let document = Document::open(builder.finish()).expect("a document");
    let census = document_census(&document);
    assert_eq!(census.pages.len(), 3);
    let widths: Vec<f64> = census
        .pages
        .iter()
        .map(|page| page.marks[0].bounds.x1 - page.marks[0].bounds.x0)
        .collect();
    assert_eq!(widths, vec![10.0, 20.0, 30.0], "{census:?}");
}

// ---- leg 4: the corpus, and the figure this milestone records ---------------

/// **Every committed package, against the figure written down beside it.**
///
/// Two halves, and only one of them is a number that moves:
///
/// - **conservation holds**, for every package: what the markup states is what
///   the document has, in order, at the place 18.1 puts it.
/// - **the recorded census**, in `tests/xps/CONSERVATION.tsv`. A change that
///   moves any of those counts has to re-measure in the same commit, which is
///   `INVENTORY.tsv`'s discipline applied to what is drawn rather than to what
///   is stored.
#[test]
fn every_committed_package_conserves_the_figure_the_record_states() {
    let recorded = record();
    let mut measured: Vec<String> = Vec::new();
    for name in COMMITTED {
        let bytes = corpus(name);
        let markup = markup_census(&bytes);
        let verdict = conservation(&bytes);

        assert!(
            verdict.holds(),
            "{name}: the document does not carry what the markup states: {:?}",
            verdict.divergences
        );
        assert_eq!(
            verdict.figure(),
            (markup.facts(), markup.facts()),
            "{name}: every fact conserved"
        );
        assert!(
            markup.facts() > 0,
            "{name}: a package that states nothing is not evidence"
        );

        measured.push(format!(
            "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            markup.pages.len(),
            markup.solids(),
            markup.gradients(),
            markup.images(),
            markup.pages.iter().map(|p| p.runs.len()).sum::<usize>(),
            markup.glyphs(),
            verdict.conserved,
            verdict.facts,
        ));
        println!(
            "  {name:28} {}/{} facts over {} page(s)",
            verdict.conserved,
            verdict.facts,
            markup.pages.len()
        );
    }

    assert_eq!(
        measured, recorded,
        "tests/xps/CONSERVATION.tsv is out of date"
    );
    assert_eq!(recorded.len(), 12, "the sweep covers twelve packages");
}

/// The packages the sweep covers, which is the list rather than the record of
/// what they measured.
///
/// **Twelve of the thirteen committed packages**, and the thirteenth is named
/// below rather than left out quietly.
const COMMITTED: &[&str] = &[
    "gs-embedded-font.xps",
    "gs-gradients.xps",
    "gs-paths.xps",
    "gs-rasterised-text.xps",
    "wpf-gradients.xps",
    "wpf-image-and-text.xps",
    "wpf-jpeg-image.xps",
    "wpf-shapes-only.xps",
    "wpf-three-pages.xps",
    "wpf-tiled-brush.xps",
    "xpsom-gradients.oxps",
    "xpsom-image-and-text.oxps",
];

/// **The one package the sweep above cannot cover, and why — asserted.**
///
/// `gs-images.xps` states two pictures its document does not carry, and both
/// sides are right. Ghostscript writes every image as a **TIFF** part named
/// through a `{ColorConvertedBitmap …}` wrapper carrying an ICC profile, and
/// this build refuses that wrapper by name (`ImageProfileUnsupported`) because
/// the syntax has nowhere to put an sRGB fallback — so ruling 2's grey
/// placeholder is what reaches the page, and a census of *what the markup
/// states* can never equal a census of *what a refusal drew*.
///
/// The alternative was to widen the markup walk until the two agreed, which
/// would have made the harness agree with the engine about a picture neither of
/// them drew. So the exclusion is a test instead: the divergence is pinned to
/// its exact shape, and the day this build learns to read a TIFF, **this test
/// fails** and the package joins the sweep in the same commit.
#[test]
fn the_one_package_the_sweep_excludes_diverges_only_by_its_refusal() {
    let bytes = corpus("gs-images.xps");
    let markup = markup_census(&bytes);
    let verdict = conservation(&bytes);

    // One page, one mark stated: the white background. The two `<Path>`
    // elements whose fill is an `ImageBrush` state a source the markup walk
    // cannot resolve to a part, because `{ColorConvertedBitmap /…/0.tif
    // /…/Profile_0.icc}` is a wrapper and not a reference.
    assert_eq!(markup.pages.len(), 1);
    assert_eq!(markup.solids(), 1, "the page's white background");
    assert_eq!(markup.images(), 0, "no picture the markup walk can address");

    assert!(
        !verdict.holds(),
        "this is the package that does not conserve"
    );
    assert_eq!(
        verdict.divergences.len(),
        1,
        "one divergence and no others: {:?}",
        verdict.divergences
    );
    assert!(
        matches!(
            verdict.divergences[0],
            Divergence::MarkCount {
                page: 0,
                markup: 1,
                document: 3
            }
        ),
        "the document carries the background and two grey placeholders: {:?}",
        verdict.divergences
    );

    // And the refusal is named, once, at the element — not swallowed, and not
    // repeated per picture.
    let document = Document::open(bytes).expect("gs-images.xps opens");
    let report = document.archive().expect("a report");
    let refusals: Vec<&tinker_pdf::ArchiveWarning> = report
        .warnings()
        .iter()
        .filter(|w| {
            matches!(
                w,
                tinker_pdf::ArchiveWarning::XpsElement {
                    defect: tinker_pdf::XpsElementDefect::ImageProfileUnsupported,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        refusals.len(),
        1,
        "one deduplicated element refusal: {:?}",
        report.warnings()
    );
}

/// **The two dialects of one document conserve the same census.**
///
/// `xpsom-image-and-text.oxps` is `wpf-image-and-text.xps` written back out by
/// a second Microsoft component, so the two state the same page. A reader that
/// discriminated on anything but the namespace, or a walk that resolved one
/// dialect's references and not the other's, makes these two censuses differ.
#[test]
fn the_two_dialects_of_one_document_state_the_same_census() {
    for (one, other) in [
        ("wpf-image-and-text.xps", "xpsom-image-and-text.oxps"),
        ("wpf-gradients.xps", "xpsom-gradients.oxps"),
    ] {
        let xps = markup_census(&corpus(one));
        let oxps = markup_census(&corpus(other));
        assert_eq!(xps.facts(), oxps.facts(), "{one} against {other}");
        assert!(
            conserve(&xps, &oxps).holds(),
            "{one} and {other} state different pages: {:?}",
            conserve(&xps, &oxps).divergences
        );
    }
}

fn record() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("xps")
        .join("CONSERVATION.tsv");
    let text = std::fs::read_to_string(&path).expect("tests/xps/CONSERVATION.tsv");
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("package\tpages\tsolids\tgradients\timages\truns\tglyphs\tconserved\tfacts"),
        "the record's header changed"
    );
    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            assert_eq!(
                line.split('\t').count(),
                9,
                "a row of CONSERVATION.tsv has nine cells"
            );
            line.to_owned()
        })
        .collect()
}
