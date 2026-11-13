//! ICC transforms kept per document under the profile's bytes
//! (`docs/design/icc.md`, P-22, "The document cache: its key and its
//! charge"), counted through MS-01's tally: under a stated budget a profile
//! compiles once a document however many objects embed it and however many
//! pages, forms, appearances and renders name it, so the transforms built are
//! at most the distinct (profile, destination, conversion) keys; past the
//! budget every use compiles, and every page draws what a document with no
//! budget draws.
//!
//! Every count expected here is derived beside it from what the document
//! holds. A scope reads each name of its `/ColorSpace` once a render, so a
//! render of a page here asks for six profiles' transforms: the page's two,
//! its form's two and its annotation appearance's two.

mod icc_support;

use tinker_pdf::{Bitmap, CacheLimits, Document, OpenOptions, RenderOptions, Stats};

/// The pages of the generated document.
const PAGES: usize = 20;

/// A PDF of `objects`, numbered from 1 in order, with a cross-reference table
/// so that it opens without repair.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A stream object: `dict`'s entries, its `/Length`, and `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// The generated document: `PAGES` pages, each with eleven objects of its
/// own after the catalog and the page tree — the page, its content, its RGB
/// and its CMYK profile, its form and the form's own RGB and CMYK profile,
/// its annotation, the annotation's appearance and the appearance's own RGB
/// and CMYK profile. Every RGB profile is [`icc_support::rgb_profile`]'s
/// bytes and every CMYK one [`icc_support::cmyk_profile`]'s, each through an
/// object of its own: six objects a page, 120 in all, holding two profiles.
/// The page fills a square in each of its two spaces and draws its form,
/// which fills two more in its own, and its square annotation's appearance
/// fills two more in the appearance's.
fn many_objects_two_profiles() -> Vec<u8> {
    let first = 3;
    let kids: Vec<String> = (0..PAGES)
        .map(|page| format!("{} 0 R", first + 11 * page))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Count {PAGES} /Kids [{}] >>",
            kids.join(" ")
        )
        .into_bytes(),
    ];
    let rgb = icc_support::rgb_profile();
    let cmyk = icc_support::cmyk_profile();
    for page in 0..PAGES {
        let at = first + 11 * page;
        let (content, page_rgb, page_cmyk) = (at + 1, at + 2, at + 3);
        let (form, form_rgb, form_cmyk) = (at + 4, at + 5, at + 6);
        let (annotation, appearance, ap_rgb, ap_cmyk) = (at + 7, at + 8, at + 9, at + 10);
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
                 /Resources << /ColorSpace << /CS0 [/ICCBased {page_rgb} 0 R] \
                 /CS1 [/ICCBased {page_cmyk} 0 R] >> /XObject << /Fm {form} 0 R >> >> \
                 /Annots [{annotation} 0 R] /Contents {content} 0 R >>"
            )
            .into_bytes(),
        );
        objects.push(stream(
            "",
            b"/CS0 cs 0.9 0.1 0.1 sc 0 50 50 50 re f \
              /CS1 cs 0.1 0.6 0.3 0.2 sc 50 50 50 50 re f \
              q 1 0 0 1 10 10 cm /Fm Do Q",
        ));
        objects.push(stream("/N 3", &rgb));
        objects.push(stream("/N 4", &cmyk));
        objects.push(stream(
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 40 20] /Resources << /ColorSpace << \
                 /FormRGB [/ICCBased {form_rgb} 0 R] /FormCMYK [/ICCBased {form_cmyk} 0 R] >> >>"
            ),
            b"/FormRGB cs 0.2 0.4 0.6 sc 0 0 20 20 re f \
              /FormCMYK cs 0.1 0.2 0.3 0.4 sc 20 0 20 20 re f",
        ));
        objects.push(stream("/N 3", &rgb));
        objects.push(stream("/N 4", &cmyk));
        objects.push(
            format!(
                "<< /Type /Annot /Subtype /Square /Rect [60 0 100 30] /F 4 \
                 /AP << /N {appearance} 0 R >> >>"
            )
            .into_bytes(),
        );
        objects.push(stream(
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 40 30] /Resources << /ColorSpace << \
                 /ApRGB [/ICCBased {ap_rgb} 0 R] /ApCMYK [/ICCBased {ap_cmyk} 0 R] >> >>"
            ),
            b"/ApRGB cs 0.7 0.5 0.1 sc 0 0 20 30 re f \
              /ApCMYK cs 0.5 0.1 0.6 0.1 sc 20 0 20 30 re f",
        ));
        objects.push(stream("/N 3", &rgb));
        objects.push(stream("/N 4", &cmyk));
    }
    pdf(&objects)
}

/// `page` of `document` rendered at 72 dpi, counting.
fn counted(document: &Document, page: usize) -> (Bitmap, Stats) {
    let bitmap = document
        .page(page as u32)
        .expect("a page")
        .render(&RenderOptions {
            stats: true,
            ..RenderOptions::default()
        });
    let stats = bitmap.stats.expect("a counted render hands its stats back");
    (bitmap, stats)
}

/// `bytes` opened under a budget of `budget` bytes, stated at open.
fn budgeted(bytes: Vec<u8>, budget: u64) -> Document {
    Document::open_with(
        bytes,
        &OpenOptions::default().with_cache_limits(CacheLimits::new(budget)),
    )
    .expect("the document opens")
}

/// Asserts two renders are one picture: size, every byte and every warning.
#[track_caller]
fn same_render(kept: &Bitmap, plain: &Bitmap, what: &str) {
    assert_eq!(
        (kept.width, kept.height, kept.format),
        (plain.width, plain.height, plain.format),
        "{what}: a different shape"
    );
    assert!(kept.data == plain.data, "{what}: different pixels");
    assert_eq!(kept.warnings, plain.warnings, "{what}: different warnings");
}

/// **The transforms built are at most the distinct keys.** The generated
/// document embeds two profiles through 120 objects, six a page, named by
/// its pages, their forms and their annotations' appearances. Rendered twice
/// under a budget with room for both profiles' transforms, its forty renders
/// build two transforms between them — the first render's first question
/// about each profile's bytes — and find them kept at every other question,
/// six a render; each page draws what a document with no budget draws, which
/// builds six a render, one for each of its scopes' names.
#[test]
fn transforms_built_are_at_most_the_distinct_keys() {
    let bytes = many_objects_two_profiles();
    let document = budgeted(bytes.clone(), 16 << 20);
    let plain = Document::open(bytes).expect("the document opens");
    let (mut built, mut found) = (0, 0);
    for pass in 0..2 {
        for page in 0..PAGES {
            let (bitmap, stats) = counted(&document, page);
            built += stats.icc_transform_misses;
            found += stats.icc_transform_hits;
            let (alone, alone_stats) = counted(&plain, page);
            assert_eq!(
                (
                    alone_stats.icc_transform_misses,
                    alone_stats.icc_transform_hits
                ),
                (6, 0),
                "pass {pass}, page {page}: with no budget each name builds its own"
            );
            same_render(&bitmap, &alone, &format!("pass {pass}, page {page}"));
        }
    }
    assert_eq!(built, 2, "two profiles, two transforms built");
    assert_eq!(
        found,
        6 * 2 * PAGES as u64 - 2,
        "every other question found its profile's transforms kept"
    );
}

/// **Past the budget each use compiles, and draws the same page.** Under a
/// budget of a kilobyte, below what one profile's transforms are charged,
/// no transform is kept: every page's every question builds its own, six a
/// render, the bytes held never pass the budget, and every page draws what a
/// document with no budget draws, byte for byte, twice over.
#[test]
fn under_a_budget_below_one_grid_every_page_is_the_same() {
    let bytes = many_objects_two_profiles();
    let budget = 1 << 10;
    let document = budgeted(bytes.clone(), budget);
    let plain = Document::open(bytes).expect("the document opens");
    for pass in 0..2 {
        for page in 0..PAGES {
            let (bitmap, stats) = counted(&document, page);
            assert_eq!(
                (stats.icc_transform_misses, stats.icc_transform_hits),
                (6, 0),
                "pass {pass}, page {page}: nothing kept to find"
            );
            assert!(stats.cache_bytes_held <= budget, "{stats:?}");
            let (alone, _) = counted(&plain, page);
            same_render(&bitmap, &alone, &format!("pass {pass}, page {page}"));
        }
    }
}

/// **One profile's bytes are one key, and two profiles' two**: two objects
/// holding the same profile, named by two pages, build one transform; an
/// object holding another profile — the same one written with a different
/// white point, so its bytes and colours differ — builds its own; and a
/// stream that will not decode and one whose bytes are no profile are each
/// refused once a document, every later question finding the refusal.
#[test]
fn a_profile_is_one_key_by_its_bytes_and_a_refusal_is_kept() {
    let rgb = icc_support::rgb_profile();
    let other = icc_support::icc_profile(
        b"mntr",
        b"RGB ",
        &[
            (
                *b"rXYZ",
                icc_support::xyz_tag(0.436_065, 0.222_488, 0.013_916),
            ),
            (
                *b"gXYZ",
                icc_support::xyz_tag(0.385_147, 0.716_873, 0.097_076),
            ),
            (
                *b"bXYZ",
                icc_support::xyz_tag(0.143_066, 0.060_607, 0.714_096),
            ),
            (*b"rTRC", icc_support::gamma_tag(2.2)),
            (*b"gTRC", icc_support::gamma_tag(2.2)),
            (*b"bTRC", icc_support::gamma_tag(2.2)),
            (*b"wtpt", icc_support::xyz_tag(0.9505, 1.0, 1.089)),
        ],
    );
    assert_ne!(rgb, other);
    // 3 and 4 the same profile, 5 another, 6 a stream whose filter is
    // damaged, 7 bytes that are no profile; pages from 8.
    let shared = [
        stream("/N 3", &rgb),
        stream("/N 3", &rgb),
        stream("/N 3", &other),
        stream("/N 3 /Filter /FlateDecode", b"\x78\x9c not deflate"),
        stream("/N 3", b"not an ICC profile"),
    ];
    let names = [3, 4, 5, 6, 7];
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            names.len(),
            (0..names.len())
                .map(|page| format!("{} 0 R", 8 + 2 * page))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes(),
    ];
    objects.extend(shared);
    for (page, profile) in names.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 50 50] \
                 /Resources << /ColorSpace << /CS0 [/ICCBased {profile} 0 R] >> >> \
                 /Contents {} 0 R >>",
                9 + 2 * page
            )
            .into_bytes(),
        );
        objects.push(stream("", b"/CS0 cs 0.9 0.5 0.1 sc 0 0 50 50 re f"));
    }
    let bytes = pdf(&objects);
    let document = budgeted(bytes.clone(), 16 << 20);
    let plain = Document::open(bytes).expect("the document opens");
    // Page by page: the first profile builds, its twin finds it, the other
    // builds its own, and each refusal is reached once.
    let want = [(1, 0), (0, 1), (1, 0), (1, 0), (1, 0)];
    for (page, &want) in want.iter().enumerate() {
        let (bitmap, stats) = counted(&document, page);
        assert_eq!(
            (stats.icc_transform_misses, stats.icc_transform_hits),
            want,
            "page {page}, its profile object {}",
            names[page]
        );
        let (alone, _) = counted(&plain, page);
        same_render(&bitmap, &alone, &format!("page {page}"));
    }
    // Again: every question finds what it asked before.
    for page in 0..names.len() {
        let (bitmap, stats) = counted(&document, page);
        assert_eq!(
            (stats.icc_transform_misses, stats.icc_transform_hits),
            (0, 1),
            "page {page} again"
        );
        let (alone, _) = counted(&plain, page);
        same_render(&bitmap, &alone, &format!("page {page} again"));
    }
    let (first, _) = counted(&plain, 0);
    let (third, _) = counted(&plain, 2);
    assert!(
        first.data != third.data,
        "the other profile draws other colours, so it is no hit of the first's"
    );
}
