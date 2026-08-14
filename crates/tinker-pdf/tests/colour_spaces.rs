//! Colour spaces and patterns produce the right pixels (8.6, 8.7).
//!
//! Four separate defects lived here, and each one rendered plausibly wrong
//! rather than obviously broken, which is why none had a test:
//!
//! - a `/Function` array was truncated to its first element, so an RGB
//!   gradient became a red ramp;
//! - a `/Separation` tint transform was hardcoded to the identity, so spot
//!   colours came out as whatever the alternate space made of a raw tint;
//! - `/Lab` was aliased to `/DeviceRGB`, which clamps L (0..100) into 0..1 and
//!   renders nearly the whole space black;
//! - a pattern fill painted `/Pattern`'s nominal black, so every gradient fill
//!   from a real design tool became an opaque black rectangle.

use tinker_pdf::{Document, RenderOptions};

/// A one-page document whose content and resources are given verbatim.
fn page(resources: &str, content: &str) -> tinker_pdf::Bitmap {
    page_with_objects(resources, content, "")
}

/// The same, with indirect objects appended after the content stream.
///
/// A tiling pattern is a *stream* (8.7.3.2, Table 75) — its cell is the
/// stream's bytes — so unlike a shading pattern it cannot be written inline in
/// the resource dictionary. Objects here start at 5.
fn page_with_objects(resources: &str, content: &str, objects: &str) -> tinker_pdf::Bitmap {
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << {resources} >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
{objects}\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1
    );

    Document::open(bytes.into_bytes())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// A tiling pattern as object 5: the keys that vary, and the cell's content.
fn tiling(keys: &str, cell: &str) -> String {
    format!(
        "5 0 obj\n<< /PatternType 1 /TilingType 1 /Resources << >> {keys}\n\
   /Length {} >>\nstream\n{cell}\nendstream\nendobj\n",
        cell.len() + 1
    )
}

/// Whether a render reported a pattern it could not paint.
fn reported_unpainted(bitmap: &tinker_pdf::Bitmap) -> bool {
    bitmap
        .warnings
        .iter()
        .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedPattern { .. }))
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

/// A pixel two renders disagree about: `(x, y, left, right)`.
type Difference = (u32, u32, (u8, u8, u8), (u8, u8, u8));

/// Where two renders of the same size first disagree, and what they say
/// there.
///
/// `assert_eq!` on the buffers is the assertion these tests want, but its
/// failure message is several thousand bytes of decimal with the interesting
/// pixel somewhere inside it. This says which pixel moved, which is the
/// question the person reading the failure has.
fn first_difference(a: &tinker_pdf::Bitmap, b: &tinker_pdf::Bitmap) -> Option<Difference> {
    if (a.width, a.height) != (b.width, b.height) {
        return Some((0, 0, pixel(a, 0, 0), pixel(b, 0, 0)));
    }
    for y in 0..a.height {
        for x in 0..a.width {
            let (left, right) = (pixel(a, x, y), pixel(b, x, y));
            if left != right {
                return Some((x, y, left, right));
            }
        }
    }
    None
}

/// Three one-output functions, one per component, must all be read. Taking
/// only the first leaves green and blue at zero for the whole ramp.
#[test]
fn a_function_array_supplies_every_component() {
    let bitmap = page(
        "/Shading << /S0 << /ShadingType 2 /ColorSpace /DeviceRGB \
           /Coords [0 0 40 0] /Extend [true true] \
           /Function [ \
             << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
             << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
             << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
           ] >> >>",
        "q 0 0 40 40 re W n /S0 sh Q",
    );

    // At the far end every component is 1, so the ramp ends white — not red.
    let (r, g, b) = pixel(&bitmap, 38, 20);
    assert!(r > 200, "red reaches full: {r}");
    assert!(
        g > 200 && b > 200,
        "and so do green and blue: {g}, {b} — a truncated array leaves them at 0"
    );
}

/// A one-ink Separation over DeviceCMYK. Full tint through the transform is
/// black; the identity would feed the tint in as cyan.
#[test]
fn a_separation_runs_its_tint_transform() {
    let bitmap = page(
        "/ColorSpace << /Spot [ /Separation /Black /DeviceCMYK \
           << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >> ] >>",
        "/Spot cs 1 scn 0 0 40 40 re f",
    );

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        r < 40 && g < 40 && b < 40,
        "full tint of a black ink is black, got ({r}, {g}, {b}) — \
         cyan means the transform was skipped"
    );
}

/// The same space at zero tint is white, which proves the transform is being
/// evaluated rather than the answer being black regardless.
#[test]
fn a_separation_at_zero_tint_is_blank() {
    let bitmap = page(
        "/ColorSpace << /Spot [ /Separation /Black /DeviceCMYK \
           << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >> ] >>",
        "/Spot cs 0 scn 0 0 40 40 re f",
    );

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(r > 220 && g > 220 && b > 220, "got ({r}, {g}, {b})");
}

/// L* of 100 with no chroma is white. Read as RGB it clamps to (1, 0, 0) after
/// the 0..1 clamp — and a mid-grey L* of 50 clamps to black.
#[test]
fn lab_lightness_is_not_clamped_into_zero_to_one() {
    let white = page(
        "/ColorSpace << /Lb [ /Lab << /WhitePoint [0.9642 1 0.8249] \
           /Range [-100 100 -100 100] >> ] >>",
        "/Lb cs 100 0 0 scn 0 0 40 40 re f",
    );
    let (r, g, b) = pixel(&white, 20, 20);
    assert!(
        r > 220 && g > 220 && b > 220,
        "L*=100 is white, got ({r}, {g}, {b})"
    );

    let mid = page(
        "/ColorSpace << /Lb [ /Lab << /WhitePoint [0.9642 1 0.8249] \
           /Range [-100 100 -100 100] >> ] >>",
        "/Lb cs 50 0 0 scn 0 0 40 40 re f",
    );
    let (r, g, b) = pixel(&mid, 20, 20);
    assert!(
        (90..=180).contains(&r) && (90..=180).contains(&g) && (90..=180).contains(&b),
        "L*=50 is a mid grey, got ({r}, {g}, {b})"
    );
}

/// A shading pattern fill paints the gradient, not black.
#[test]
fn a_shading_pattern_paints_its_shading() {
    let bitmap = page(
        "/Pattern << /P0 << /PatternType 2 /Shading \
           << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 40 0] \
              /Extend [true true] \
              /Function << /FunctionType 2 /Domain [0 1] \
                           /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >>",
        "/Pattern cs /P0 scn 0 0 40 40 re f",
    );

    let left = pixel(&bitmap, 2, 20);
    let right = pixel(&bitmap, 37, 20);
    assert!(
        left.0 > 180 && left.2 < 80,
        "the left end is red, got {left:?}"
    );
    assert!(
        right.2 > 180 && right.0 < 80,
        "the right end is blue, got {right:?} — black means the pattern was ignored"
    );
}

/// 8.7.3.1: a tiling pattern's matrix maps pattern space to the *default*
/// space of the page, so the CTM in force when the fill happens is not part of
/// it and the cell does not move with it.
///
/// *Rewritten August 2026, with gap 09.* This assertion used to be
/// `a_tiling_pattern_is_reported_rather_than_blacked_out`, and it was pinning
/// correct behaviour: the engine warned and left the area blank. Tiles paint
/// now, so it had to become something, and this is what gap 09's own "worse
/// than none" section names as the failure to guard against — a lattice
/// anchored to the paint-time CTM is *there*, just not where it belongs, which
/// reads as a small offset rather than as a defect and is therefore never
/// reported. Deleting the test would have left that hole unwatched.
///
/// All three pages put the same shape over the same device pixels and get
/// there differently: plainly, through a doubled CTM with halved coordinates,
/// and through a three-point translation. The scaled one catches a lattice
/// whose *cells* follow the transform; the translated one catches a lattice
/// whose **phase** does, which is the subtler and more common half — three
/// points is not a whole step, so the hatch simply sits three pixels over,
/// and that reads as a design choice rather than as a defect. The bytes are
/// the assertion in both.
#[test]
fn a_tiling_pattern_fill_does_not_move_with_the_transform() {
    let plain = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 4 4 32 32 re f",
        &tiling(HATCH_KEYS, HATCH),
    );
    let scaled = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn q 2 0 0 2 0 0 cm 2 2 16 16 re f Q",
        &tiling(HATCH_KEYS, HATCH),
    );
    let shifted = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn q 1 0 0 1 3 3 cm 1 1 32 32 re f Q",
        &tiling(HATCH_KEYS, HATCH),
    );

    // Three blank pages would agree and prove nothing, so the lattice has to
    // be on the page first — and it has to be a lattice rather than one cell,
    // or "the pattern moved" and "the pattern was scaled" are the same
    // picture.
    assert!(
        pixel(&plain, 6, 32).2 > 180 && pixel(&plain, 22, 16).2 > 180,
        "at least two cells paint, got {:?} and {:?}",
        pixel(&plain, 6, 32),
        pixel(&plain, 22, 16)
    );
    assert!(!reported_unpainted(&plain), "{:?}", plain.warnings);

    assert_eq!(
        first_difference(&plain, &scaled),
        None,
        "the lattice sits in the same place under a scaled CTM — \
         a difference here is the pattern following it"
    );
    assert_eq!(
        first_difference(&plain, &shifted),
        None,
        "and under a translated one, which moves the lattice's phase \
         rather than its scale"
    );
}

/// The same guarantee on a *stroke*, which gap 07 could only assert for a
/// shading pattern because a tiling one warned rather than painted.
///
/// *Rewritten August 2026, with gap 09.* This was
/// `a_tiling_pattern_stroke_is_reported_rather_than_blacked_out`. Gap 07
/// routed `stroke_path` through `fill_with_pattern`, so tiles started painting
/// on strokes the day this gap's cells did, with no separate wiring and
/// therefore nothing forcing a stroke-side anchoring test to exist. This is
/// that test, and it is deliberately the twin of the fill one above rather
/// than a weaker relative: the two routes share a function and the way they
/// diverge is by one of them growing a transform the other does not have.
#[test]
fn a_tiling_pattern_stroke_does_not_move_with_the_transform() {
    let plain = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern CS /P0 SCN 16 w 4 20 m 36 20 l S",
        &tiling(HATCH_KEYS, HATCH),
    );
    let scaled = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern CS /P0 SCN q 2 0 0 2 0 0 cm 8 w 2 10 m 18 10 l S Q",
        &tiling(HATCH_KEYS, HATCH),
    );
    let shifted = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern CS /P0 SCN q 1 0 0 1 3 3 cm 16 w 1 17 m 33 17 l S Q",
        &tiling(HATCH_KEYS, HATCH),
    );

    assert!(
        pixel(&plain, 6, 16).2 > 180 && pixel(&plain, 22, 16).2 > 180,
        "the rule carries at least two cells, got {:?} and {:?}",
        pixel(&plain, 6, 16),
        pixel(&plain, 22, 16)
    );
    assert!(!reported_unpainted(&plain), "{:?}", plain.warnings);

    assert_eq!(
        first_difference(&plain, &scaled),
        None,
        "the lattice sits in the same place under a scaled CTM — \
         a difference here is the stroke's transform reaching the pattern"
    );
    assert_eq!(
        first_difference(&plain, &shifted),
        None,
        "and under a translated one"
    );
}

/// The lattice belongs to the *pattern*, not to the shape being filled, so two
/// shapes filled with one pattern are in phase with each other.
///
/// This is what 8.7.3.1's anchoring buys a reader, and it is the assertion an
/// implementation that indexes the lattice from each shape's own bounding box
/// fails: each shape looks perfectly reasonable on its own, and the two
/// together do not line up. That is the failure nobody reports, on a page
/// where two adjacent panels carry the same hatch.
#[test]
fn two_shapes_share_one_pattern_lattice() {
    let together = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 2 2 12 36 re f 21 2 13 36 re f",
        &tiling(HATCH_KEYS, HATCH),
    );
    // The same lattice, painted in one go, then cut to the same two shapes by
    // the clip. If the lattice is the pattern's, these are the same picture.
    let whole = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "q 2 2 12 36 re 21 2 13 36 re W n \
         /Pattern cs /P0 scn 0 0 40 40 re f Q",
        &tiling(HATCH_KEYS, HATCH),
    );

    assert!(
        pixel(&together, 6, 32).2 > 180,
        "the first shape carries the hatch, got {:?}",
        pixel(&together, 6, 32)
    );
    assert_eq!(
        first_difference(&together, &whole),
        None,
        "two shapes filled separately are in phase with one lattice cut to \
         the same two shapes — a lattice indexed from each shape's own box \
         is not"
    );
}

/// The gradient this file's stroke tests paint with: red at x=0, blue at
/// x=40, extended past both ends so every pixel of a 40 pt page has a colour.
const GRADIENT: &str = "/Pattern << /P0 << /PatternType 2 /Shading \
       << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 40 0] \
          /Extend [true true] \
          /Function << /FunctionType 2 /Domain [0 1] \
                       /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >>";

/// One cell covering the whole page, so a placement test can measure the
/// cell's position without a lattice index confusing the picture.
const CELL_KEYS: &str = "/PaintType 1 /BBox [0 0 40 40] /XStep 40 /YStep 40";
const CELL: &str = "0 0 1 rg 8 8 24 24 re f";

/// A real lattice: an 8 pt cell with a 4 pt square in one corner, so the phase
/// of the lattice is visible and not only its scale.
const HATCH_KEYS: &str = "/PaintType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8";
const HATCH: &str = "0 0 1 rg 0 0 4 4 re f";

/// A tiling pattern with a cell that has no readable content stream at all.
///
/// It carries the keys and no stream, so there is nothing to rasterize. That
/// is still a pattern this build cannot paint, and it must still be reported
/// rather than blacked out — the same degradation, now reached by a different
/// route.
const CELLLESS: &str = "/Pattern << /P0 << /PatternType 1 /PaintType 1 \
       /TilingType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8 \
       /Resources << >> /Length 0 >> >>";

/// A pattern whose cell cannot be read is reported, not blacked out. Filling
/// it with black would hide the gap behind something that reads as content.
#[test]
fn a_pattern_with_no_cell_is_reported_rather_than_blacked_out() {
    let bitmap = page(CELLLESS, "/Pattern cs /P0 scn 0 0 40 40 re f");

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        r > 220 && g > 220 && b > 220,
        "the area is left alone, got ({r}, {g}, {b})"
    );
    assert!(
        reported_unpainted(&bitmap),
        "and the gap is reported: {:?}",
        bitmap.warnings
    );
}

/// The same on a stroke, because gap 07 made the two share one route and this
/// is the assertion that keeps them from diverging again.
#[test]
fn a_pattern_with_no_cell_is_reported_on_a_stroke_too() {
    let bitmap = page(CELLLESS, "/Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S");

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        r > 220 && g > 220 && b > 220,
        "the rule is left alone, got ({r}, {g}, {b}) — black is the defect"
    );
    assert!(
        reported_unpainted(&bitmap),
        "and the gap is reported: {:?}",
        bitmap.warnings
    );
}

/// Milestone 1: the cell is rasterized once into a buffer of its own, so a
/// one-cell pattern has to paint exactly what the same content painted
/// directly.
///
/// Byte-for-byte, because that is the only comparison that catches the cell
/// landing half a pixel out — which is what an offscreen buffer sized or
/// placed by a different rounding than the page's does, and which reads as
/// slightly soft edges rather than as a bug.
#[test]
fn a_one_cell_pattern_paints_what_its_cell_draws() {
    let tiled = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 0 0 40 40 re f",
        &tiling(CELL_KEYS, CELL),
    );
    let direct = page("", CELL);

    assert!(
        pixel(&tiled, 20, 20).2 > 180,
        "the cell painted, got {:?}",
        pixel(&tiled, 20, 20)
    );
    assert_eq!(
        first_difference(&tiled, &direct),
        None,
        "a cell blitted from its own buffer paints the pixels the same \
         content painted directly"
    );
}

/// 8.7.3.2: the cell is clipped to its `/BBox`, and the step is not the box.
///
/// The risk this closes is structural rather than arithmetic. A cell whose
/// box is smaller than its step leaves gaps between cells, and a neighbour
/// must not fill them — so the buffer is the box and nothing else, which is
/// what makes spill impossible rather than merely prevented. The cell here
/// draws a square four points larger than its box on every side, which is how
/// a real hatch is written: the overshoot is deliberate, so that neighbouring
/// cells join when the step *is* the box.
#[test]
fn a_cell_cannot_spill_into_the_gap_its_step_leaves() {
    let bitmap = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 0 0 40 40 re f",
        &tiling(
            "/PaintType 1 /BBox [0 0 8 8] /XStep 16 /YStep 16",
            "0 0 1 rg -4 -4 16 16 re f",
        ),
    );

    assert!(
        pixel(&bitmap, 4, 36).2 > 180,
        "the cell itself paints, got {:?}",
        pixel(&bitmap, 4, 36)
    );
    assert_eq!(
        pixel(&bitmap, 12, 36),
        (255, 255, 255),
        "and the 8 pt gap the step asks for stays paper — the overshoot is \
         clipped to the box rather than landing in the next cell's gap"
    );
    assert_eq!(pixel(&bitmap, 4, 28), (255, 255, 255), "vertically too");
}

/// The lattice repeats end to end, through a real content stream: three
/// columns and three rows of an 8 pt cell stepped by 16 across a 40 pt page.
#[test]
fn a_hatch_repeats_across_what_it_fills() {
    let bitmap = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 0 0 40 40 re f",
        &tiling(
            "/PaintType 1 /BBox [0 0 8 8] /XStep 16 /YStep 16",
            "0 0 1 rg 0 0 8 8 re f",
        ),
    );

    for (x, y) in [(4, 36), (20, 36), (36, 36), (4, 20), (20, 20), (36, 4)] {
        assert!(
            pixel(&bitmap, x, y).2 > 180,
            "a cell covers ({x}, {y}), got {:?}",
            pixel(&bitmap, x, y)
        );
    }
    for (x, y) in [(12, 36), (28, 36), (4, 28), (12, 28)] {
        assert_eq!(
            pixel(&bitmap, x, y),
            (255, 255, 255),
            "and ({x}, {y}) is between cells"
        );
    }
}

/// Nothing paints outside the filled path, through the same route.
#[test]
fn a_tiled_fill_stops_at_its_path() {
    let bitmap = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 10 10 20 20 re f",
        &tiling(
            "/PaintType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8",
            "0 0 1 rg 0 0 8 8 re f",
        ),
    );

    assert!(pixel(&bitmap, 20, 20).2 > 180, "inside the rectangle");
    for (x, y) in [(4, 20), (36, 20), (20, 4), (20, 36)] {
        assert_eq!(
            pixel(&bitmap, x, y),
            (255, 255, 255),
            "({x}, {y}) is outside the path the pattern filled"
        );
    }
}

/// 8.7.3.3: a `PaintType 2` cell is a *shape*. Colour operators inside it
/// mean nothing, and the paint is the colour the `SCN` operands supplied.
///
/// The cell here paints green, loudly, and the operands say red. Green
/// anywhere on this page is the cell's own colour surviving.
#[test]
fn an_uncoloured_cell_takes_the_operands_colour_and_ignores_its_own() {
    let bitmap = page_with_objects(
        "/Pattern << /P0 5 0 R >> /ColorSpace << /Cs1 [/Pattern /DeviceRGB] >>",
        "/Cs1 cs 1 0 0 /P0 scn 0 0 40 40 re f",
        &tiling(
            "/PaintType 2 /BBox [0 0 8 8] /XStep 16 /YStep 16",
            "0 1 0 rg 0 0 8 8 re f",
        ),
    );

    let (r, g, b) = pixel(&bitmap, 4, 36);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the cell paints in the operands' red, got ({r}, {g}, {b}) — \
         green is the cell's own `rg` surviving"
    );
    assert_eq!(
        pixel(&bitmap, 12, 36),
        (255, 255, 255),
        "and it is still a shape: the step's gap is untouched, which a \
         recoloured *rectangle* rather than a recoloured shape would fill"
    );
}

/// The same on the stroke slot, and this is the assertion nothing in this
/// engine has ever been able to make.
///
/// Gap 07 stored the `SCN` components for the stroking slot as well as the
/// filling one, and nothing ever rendered them: a shading pattern supplies its
/// own colour, so the slot's colour was dead data on both routes. An
/// uncoloured tiling pattern is the first thing that reads it, and a stroke is
/// the first thing that can read the *wrong* one.
///
/// Both slots are set, to different colours, and only one of them is correct
/// here. Every other pattern assertion in this file is satisfied by a cell
/// that paints in *some* colour.
#[test]
fn an_uncoloured_pattern_stroke_takes_the_stroking_slots_colour() {
    let bitmap = page_with_objects(
        "/Pattern << /P0 5 0 R >> /ColorSpace << /Cs1 [/Pattern /DeviceRGB] >>",
        "/Cs1 cs 1 0 0 /P0 scn /Cs1 CS 0 0 1 /P0 SCN \
         16 w 4 20 m 36 20 l S",
        &tiling(
            "/PaintType 2 /BBox [0 0 8 8] /XStep 8 /YStep 8",
            "0 1 0 rg 0 0 8 8 re f",
        ),
    );

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        b > 200 && r < 60,
        "the rule takes the stroking slot's blue, got ({r}, {g}, {b}) — \
         red is the fill slot's colour on a stroke, green is the cell's own"
    );
}

/// And the fill slot's twin, with both slots set the other way round, so
/// neither test can pass by reading a slot that happens to hold the right
/// answer.
#[test]
fn an_uncoloured_pattern_fill_takes_the_filling_slots_colour() {
    let bitmap = page_with_objects(
        "/Pattern << /P0 5 0 R >> /ColorSpace << /Cs1 [/Pattern /DeviceRGB] >>",
        "/Cs1 cs 0 0 1 /P0 scn /Cs1 CS 1 0 0 /P0 SCN 4 4 32 32 re f",
        &tiling(
            "/PaintType 2 /BBox [0 0 8 8] /XStep 8 /YStep 8",
            "0 1 0 rg 0 0 8 8 re f",
        ),
    );

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        b > 200 && r < 60,
        "the fill takes the filling slot's blue, got ({r}, {g}, {b})"
    );
}

/// A `PaintType 1` cell keeps its own colours, which is the other half of the
/// same decision: recolouring a coloured pattern would flatten every
/// multi-colour logo watermark in the corpus to one ink.
#[test]
fn a_coloured_cell_keeps_the_colours_it_drew_with() {
    let bitmap = page_with_objects(
        "/Pattern << /P0 5 0 R >> /ColorSpace << /Cs1 [/Pattern /DeviceRGB] >>",
        "/Cs1 cs 1 0 0 /P0 scn 0 0 40 40 re f",
        &tiling(
            "/PaintType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8",
            "0 1 0 rg 0 0 4 8 re f 0 0 1 rg 4 0 4 8 re f",
        ),
    );

    let left = pixel(&bitmap, 2, 36);
    let right = pixel(&bitmap, 6, 36);
    assert!(
        left.1 > 200 && left.0 < 60,
        "the cell's own green survives, got {left:?}"
    );
    assert!(
        right.2 > 200 && right.0 < 60,
        "and so does its blue, got {right:?} — one flat colour is a coloured \
         cell being recoloured"
    );
}

/// A cell drawn through a pattern `/Matrix` lands where the matrix puts it,
/// not where the cell's own coordinates would.
#[test]
fn a_pattern_matrix_moves_the_cell() {
    let moved = page_with_objects(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 0 0 40 40 re f",
        &tiling(
            "/PaintType 1 /BBox [0 0 40 40] /XStep 400 /YStep 400 \
             /Matrix [1 0 0 1 6 6]",
            CELL,
        ),
    );

    // The cell draws 8..32; the matrix shifts it to 14..38 in PDF space, which
    // is y 2..26 from the top.
    assert!(
        pixel(&moved, 20, 20).2 > 180,
        "inside the moved cell, got {:?}",
        pixel(&moved, 20, 20)
    );
    assert_eq!(
        pixel(&moved, 10, 34),
        (255, 255, 255),
        "and where the cell would have been without the matrix, nothing"
    );
}

/// A shading pattern named by `SCN` paints the stroke, not `/Pattern`'s
/// nominal black.
#[test]
fn a_shading_pattern_strokes_with_its_shading() {
    let bitmap = page(GRADIENT, "/Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S");

    let left = pixel(&bitmap, 2, 20);
    let right = pixel(&bitmap, 37, 20);
    assert!(
        left.0 > 180 && left.2 < 80,
        "the left end of the rule is red, got {left:?} — \
         black means the stroke ignored the pattern"
    );
    assert!(
        right.2 > 180 && right.0 < 80,
        "the right end is blue, got {right:?}"
    );

    // The rule is 8 pt of a 40 pt page, so everything outside it is untouched.
    // Without this a pattern painted over the whole canvas would pass above.
    let above = pixel(&bitmap, 20, 4);
    assert_eq!(
        above,
        (255, 255, 255),
        "outside the rule nothing is painted"
    );
}

/// The same band filled and stroked must come out the same. A stroke is a
/// fill of its outline, so anything that makes the two disagree — a different
/// fill rule, a different anchoring, a different alpha — shows up here and
/// nowhere else.
#[test]
fn a_pattern_fills_and_strokes_the_same_band_identically() {
    let filled = page(GRADIENT, "/Pattern cs /P0 scn 0 16 40 8 re f");
    let stroked = page(GRADIENT, "/Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S");

    // Proof the comparison is about something: the band is painted and it is
    // not one flat colour.
    let left = pixel(&stroked, 2, 20);
    let right = pixel(&stroked, 37, 20);
    assert!(
        left.0 > 180 && right.2 > 180,
        "the band carries the gradient"
    );

    assert_eq!(
        first_difference(&filled, &stroked),
        None,
        "the same 8 pt band filled and stroked paints the same pixels"
    );
}

/// 8.4.5: painting has two alphas, and a stroke uses `CA`. Reaching the
/// pattern paint through the fill path is the obvious way to build this, and
/// the obvious way carries `ca` with it — so a page that sets them apart is
/// the only thing that can tell the two apart.
#[test]
fn a_pattern_stroke_takes_the_stroking_alpha() {
    let resources = format!("{GRADIENT} /ExtGState << /GS0 << /ca 1 /CA 0.4 >> >>");
    let bitmap = page(
        &resources,
        "/GS0 gs /Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S",
    );

    // The ramp is red at this end, so 40 % of it over white leaves green and
    // blue at about 153. Full opacity — which is what `ca` would give — leaves
    // them at 0.
    let (r, g, b) = pixel(&bitmap, 2, 20);
    assert!(r > 220, "red stays saturated over white, got {r}");
    assert!(
        (120..=185).contains(&g) && (120..=185).contains(&b),
        "the stroke is 40 % opaque, got ({r}, {g}, {b}) — \
         near 0 means it took `ca` instead of `CA`"
    );
}

/// 8.7.3.1: a pattern's matrix maps pattern space to the *default* space of
/// the page, so the CTM in force when the stroke happens is not part of it.
///
/// Both pages draw the same 8 pt rule across the same pixels; the second gets
/// there through a doubled CTM and halved coordinates. Anchoring the pattern
/// to the CTM would stretch the second page's gradient by two — its right end
/// would reach only the middle of the ramp — so the bytes are the assertion.
#[test]
fn a_stroked_pattern_does_not_move_with_the_transform() {
    let plain = page(GRADIENT, "/Pattern CS /P0 SCN 8 w 4 20 m 36 20 l S");
    let scaled = page(
        GRADIENT,
        "/Pattern CS /P0 SCN q 2 0 0 2 0 0 cm 4 w 2 10 m 18 10 l S Q",
    );

    // The ramp has to actually run across the rule, or two blank pages would
    // agree and prove nothing.
    let left = pixel(&plain, 5, 20);
    let right = pixel(&plain, 34, 20);
    assert!(
        left.0 > 180 && left.2 < 80,
        "the left end is red, got {left:?}"
    );
    assert!(
        right.2 > 150 && right.0 < 110,
        "the right end is blue, got {right:?}"
    );

    assert_eq!(
        first_difference(&plain, &scaled),
        None,
        "the gradient sits in the same place under both transforms — \
         a difference here is the pattern following the CTM"
    );
}

/// Choosing an ordinary colour after a pattern has to clear it, or every
/// later fill keeps painting the gradient.
#[test]
fn setting_a_colour_clears_the_pattern() {
    let bitmap = page(
        "/Pattern << /P0 << /PatternType 2 /Shading \
           << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 40 0] \
              /Extend [true true] \
              /Function << /FunctionType 2 /Domain [0 1] \
                           /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >>",
        "/Pattern cs /P0 scn 0 0 40 20 re f 0 1 0 rg 0 20 40 20 re f",
    );

    let patterned = pixel(&bitmap, 2, 30);
    let plain = pixel(&bitmap, 2, 10);
    assert!(
        patterned.0 > 180,
        "the lower half is the gradient's red end"
    );
    assert!(
        plain.1 > 180 && plain.0 < 80,
        "the upper half is plain green, got {plain:?}"
    );
}
