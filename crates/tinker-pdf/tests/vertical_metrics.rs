//! Vertical text uses the vertical metrics the file supplies (gap 05, 9.7.4.3).
//!
//! A Japanese document set vertically was detected as vertical — the `-V`
//! suffix and `/WMode 1` both reached `Font`, and the pen moved down rather
//! than across — and then moved down by the *horizontal* width of each glyph,
//! because `/W2` and `/DW2` were parsed nowhere at all. Every vertical advance
//! was a horizontal number applied along the wrong axis, and the position
//! vector, which is the other half of 9.7.4.3, was not applied at any point.
//!
//! Two things about these fixtures are deliberate, and both come from the
//! gap's own risk table.
//!
//! **Positions are absolute, never deltas.** An inverted vertical displacement
//! produces a perfectly tidy column running upward: the gaps between the
//! glyphs are all correct and only the direction is wrong, so an assertion on
//! spacing accepts it. An assertion on where the glyphs actually *are* cannot.
//!
//! **The CIDs are non-adjacent — 5, 9 and 14.** `/W2` carries three numbers
//! per CID where `/W` carries one, and reading it with `/W`'s stride does not
//! fail; it puts each entry's metrics on some other CID and keeps going. Three
//! entries with gaps between them is what makes that visible, because a shift
//! lands in a gap rather than on another entry.
//!
//! And `/W` is the decoy. Every CID here is 1000 units wide horizontally and
//! none of them advances anything like 1000 vertically, so no test below can
//! pass through the horizontal path by accident.

use tinker_pdf::{Bitmap, Document, RenderOptions, RenderWarning, WritingMode};

// ---------------------------------------------------------------------------
// Building the font program.
// ---------------------------------------------------------------------------

/// One glyph's `glyf` entry: a box `size` font units on a side, from the
/// origin.
///
/// Boxes rather than letters, so the ink says *which* glyph drew — gap 01's
/// technique, and the only way to tell a right glyph from a wrong one of the
/// same width. Here it does double duty: a box anchored at the glyph origin
/// means the ink's position also says where the origin was put, which is what
/// the position vector moves.
fn box_glyph(size: i16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1i16.to_be_bytes()); // one contour
    out.extend_from_slice(&0i16.to_be_bytes()); // xMin
    out.extend_from_slice(&0i16.to_be_bytes()); // yMin
    out.extend_from_slice(&size.to_be_bytes()); // xMax
    out.extend_from_slice(&size.to_be_bytes()); // yMax
    out.extend_from_slice(&3u16.to_be_bytes()); // last point of the contour
    out.extend_from_slice(&0u16.to_be_bytes()); // no hinting instructions

    out.extend_from_slice(&[1, 1, 1, 1]); // every point on-curve
    for dx in [0i16, size, 0, -size] {
        out.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, size, 0] {
        out.extend_from_slice(&dy.to_be_bytes());
    }

    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// The four glyphs, glyph 1 upward. Glyph 0 is `.notdef` and draws nothing.
const SIZES: [i16; 4] = [300, 900, 600, 450];

/// A TrueType face whose glyph `g`, counting from 1, is a box `SIZES[g - 1]`
/// units on a side, and which carries no `cmap` — a composite font has no use
/// for one, and a subsetter drops it.
fn boxes_font() -> Vec<u8> {
    let glyphs = SIZES.len() as u16 + 1;

    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0]; // glyph 0 is an empty `loca` range.
    for size in SIZES {
        glyf.extend_from_slice(&box_glyph(size));
        loca.push(glyf.len() as u32);
    }
    let loca_bytes: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long `loca` offsets

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&glyphs.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&glyphs.to_be_bytes()); // numberOfHMetrics

    // The face's own horizontal advance, which nothing vertical may consult.
    let mut hmtx = Vec::new();
    for _ in 0..glyphs {
        hmtx.extend_from_slice(&600u16.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // left side bearing
    }

    let tables: [(&[u8; 4], &[u8]); 6] = [
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca_bytes),
        (b"maxp", &maxp),
    ];

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]); // search hints, unread

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

// ---------------------------------------------------------------------------
// Building the document around it.
// ---------------------------------------------------------------------------

/// A stream object holding raw bytes, with `prefix` written into its
/// dictionary verbatim.
fn stream(prefix: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {prefix} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// The vertical encoding CMap: `/WMode 1`, and three codes mapping to three
/// non-adjacent CIDs.
///
/// Embedded rather than `Identity-V` because the code and the CID must be
/// different numbers — under `Identity-V` they are the same one, and a reader
/// keying the metrics off the code would be indistinguishable from one keying
/// them off the CID.
fn vertical_cmap() -> Vec<u8> {
    stream(
        "/Type /CMap",
        b"/CIDInit /ProcSet findresource begin\n\
          begincmap\n\
          /CMapName /Fixture-V def\n\
          /WMode 1 def\n\
          1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
          3 begincidchar <0041> 5 <0042> 9 <0043> 14 endcidchar\n\
          endcmap end",
    )
}

/// The same CMap with `/WMode 0`, for the horizontal control.
fn horizontal_cmap() -> Vec<u8> {
    stream(
        "/Type /CMap",
        b"/CIDInit /ProcSet findresource begin\n\
          begincmap\n\
          /CMapName /Fixture-H def\n\
          /WMode 0 def\n\
          1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
          3 begincidchar <0041> 5 <0042> 9 <0043> 14 endcidchar\n\
          endcmap end",
    )
}

/// `/CIDToGIDMap`: CID 5 is glyph 3 (a 600-unit box), CID 9 is glyph 1 (300)
/// and CID 14 is glyph 4 (450).
fn cid_to_gid() -> Vec<u8> {
    let mut map = [0u16; 15];
    map[5] = 3;
    map[9] = 1;
    map[14] = 4;
    let bytes: Vec<u8> = map.iter().flat_map(|g| g.to_be_bytes()).collect();
    stream("", &bytes)
}

/// The decoy. Every CID the fixture uses is 1000 units wide, which is not
/// close to any vertical number below.
const HORIZONTAL_WIDTHS: &str = "/DW 250 /W [5 [1000] 9 [1000] 14 [1000]]";

/// The vertical metrics, in both of Table 117's forms and on three
/// non-adjacent CIDs.
///
/// CID 5 through the array form, CID 9 through the range form, CID 14 through
/// the array form again — so a stride error in either form desynchronises what
/// follows it and CID 14 reads back as something else.
const VERTICAL_METRICS: &str = "/W2 [5 [-400 250 700] 9 9 -600 300 800 14 [-900 350 900]]";

/// A one-page document: `objects` are numbered from 5, and object 5 is the
/// font the page's `/F0` names.
fn document(objects: &[Vec<u8>], content: &str, height: f64) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 {height}]\n\
             /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes());
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");
    for (i, object) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", i + 5).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            objects.len() + 5
        )
        .as_bytes(),
    );
    out
}

/// A composite font over the box face, showing `codes` at 48 point with the
/// pen started by `td`.
///
/// `metrics` is written into the descendant dictionary verbatim, so a test can
/// give `/W2`, `/DW2`, both or neither. `embed` decides whether the font
/// program is attached: text extraction needs no outlines, and a fixture that
/// carries none cannot accidentally be measuring the renderer.
fn vertical_document(
    encoding: fn() -> Vec<u8>,
    metrics: &str,
    codes: &[u16],
    td: (f64, f64),
    height: f64,
    embed: bool,
) -> Vec<u8> {
    let hex: String = codes.iter().map(|c| format!("{c:04X}")).collect();
    let descriptor = if embed { "/FontDescriptor 9 0 R" } else { "" };
    let mut objects = vec![
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding 7 0 R\n\
          /DescendantFonts [6 0 R] >>"
            .to_vec(),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture\n\
             {HORIZONTAL_WIDTHS} {metrics}\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
             /CIDToGIDMap 8 0 R {descriptor} >>"
        )
        .into_bytes(),
        encoding(),
        cid_to_gid(),
    ];
    if embed {
        objects.push(
            b"<< /Type /FontDescriptor /FontName /Fixture /Flags 4 /FontFile2 10 0 R >>".to_vec(),
        );
        objects.push(stream("", &boxes_font()));
    }

    let (x, y) = td;
    document(
        &objects,
        &format!("BT /F0 48 Tf {x} {y} Td <{hex}> Tj ET"),
        height,
    )
}

// ---------------------------------------------------------------------------
// Measuring what came out.
// ---------------------------------------------------------------------------

fn open(bytes: Vec<u8>) -> Document {
    Document::open(bytes).expect("it opens")
}

/// The origin of every extracted character, in PDF user space.
fn origins(bytes: Vec<u8>) -> Vec<(f64, f64)> {
    let doc = open(bytes);
    let page = doc.page(0).expect("a page").text();
    page.lines()
        .iter()
        .flat_map(|l| l.chars.iter())
        .map(|c| c.origin)
        .collect()
}

fn render(bytes: Vec<u8>) -> Bitmap {
    open(bytes)
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// Everything the page painted, as `(x0, y0, x1, y1)` in device pixels — y
/// downward from the top, which is where the flip puts it.
fn ink_bounds(bitmap: &Bitmap) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let at = (y as usize * bitmap.stride) + x as usize * bitmap.components();
            let pixel = &bitmap.data[at..at + bitmap.components()];
            if pixel.iter().any(|v| *v != 255) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    assert!(x0 <= x1, "the page painted nothing: {:?}", bitmap.warnings);
    (f64::from(x0), f64::from(y0), f64::from(x1), f64::from(y1))
}

/// Font units at 48 point, in points.
fn pt(units: f64) -> f64 {
    units / 1000.0 * 48.0
}

#[track_caller]
fn close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: got {actual}, expected {expected}"
    );
}

#[track_caller]
fn near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= 2.0,
        "{what}: got {actual} pixels, expected {expected}"
    );
}

// ---------------------------------------------------------------------------
// Milestone 2: the displacement reaches the pen.
// ---------------------------------------------------------------------------

/// Milestone 2's exit criterion, on the text quads, so it holds without a
/// rasteriser: three glyphs of a vertical run advance by `/W2`'s numbers and
/// not by `/W`'s, and each one sits at an absolute position no other reading
/// of the file produces.
///
/// Hand-computed from 9.4.4 and 9.7.4.3, pen starting at (100, 700), 48pt:
///
/// | | v | -v at 48pt | drawn at | pen after |
/// | --- | --- | --- | --- | --- |
/// | A, CID 5 | (250, 700) | (-12, -33.6) | (88, 666.4) | (100, 680.8) |
/// | B, CID 9 | (300, 800) | (-14.4, -38.4) | (85.6, 642.4) | (100, 652) |
/// | C, CID 14 | (350, 900) | (-16.8, -43.2) | (83.2, 608.8) | (100, 608.8) |
///
/// Four wrong readings, and the y of the second glyph tells all of them apart
/// from the right one and from each other: `/W`'s 1000 units gives 613.6, an
/// inverted sign gives 701.6, `/W2` read with `/W`'s stride leaves CID 9 on
/// `/DW2`'s default and gives 638.56, and no position vector at all gives
/// 680.8.
#[test]
fn a_vertical_run_advances_by_w2_and_not_by_w() {
    let found = origins(vertical_document(
        vertical_cmap,
        VERTICAL_METRICS,
        &[0x0041, 0x0042, 0x0043],
        (100.0, 700.0),
        800.0,
        false,
    ));
    assert_eq!(found.len(), 3, "three glyphs came out: {found:?}");

    let expected = [
        (100.0 - pt(250.0), 700.0 - pt(700.0)),
        (100.0 - pt(300.0), 700.0 - pt(400.0) - pt(800.0)),
        (100.0 - pt(350.0), 700.0 - pt(400.0) - pt(600.0) - pt(900.0)),
    ];
    for (i, (want, got)) in expected.iter().zip(&found).enumerate() {
        close(got.0, want.0, &format!("glyph {i} x"));
        close(got.1, want.1, &format!("glyph {i} y"));
    }

    // The column runs downward, and every step is one of /W2's numbers rather
    // than /W's 1000. Stated separately from the positions above because this
    // is the sentence the gap is about.
    let ys: Vec<f64> = found.iter().map(|o| o.1).collect();
    assert!(ys[0] > ys[1] && ys[1] > ys[2], "downward: {ys:?}");
    assert!(
        ys.iter().all(|y| *y > 500.0),
        "and not by 48 points a glyph, which is what /W says: {ys:?}"
    );
}

/// The stride risk, in a document. `/W2`'s three-number entries read with
/// `/W`'s one-number stride would leave CID 9 and CID 14 on `/DW2`'s default,
/// so the run would step -1000 twice after its first glyph.
///
/// Asserted as the gaps between glyphs here rather than as positions, because
/// what a stride error changes is which CID got which entry — and the three
/// entries have three different displacements precisely so that reading them
/// off by one is visible.
#[test]
fn each_cid_gets_its_own_entry_and_not_its_neighbour_s() {
    let found = origins(vertical_document(
        vertical_cmap,
        VERTICAL_METRICS,
        &[0x0041, 0x0042, 0x0043],
        (100.0, 700.0),
        800.0,
        false,
    ));

    // The pen positions, recovered by undoing each glyph's own position
    // vector — so what is compared is the displacement and nothing else.
    let pen: Vec<f64> = [700.0, 800.0, 900.0]
        .iter()
        .zip(&found)
        .map(|(v_y, origin)| origin.1 + pt(*v_y))
        .collect();
    close(pen[1] - pen[0], -pt(400.0), "CID 5's displacement");
    close(pen[2] - pen[1], -pt(600.0), "CID 9's displacement");

    // Three different numbers, so no arrangement of them is accidental.
    assert_ne!(pen[1] - pen[0], pen[2] - pen[1]);
}

/// A font with no `/W2` at all still writes vertically, by 9.7.4.3's
/// `[880 -1000]` — one em down per glyph, and a position vector of half the
/// horizontal width sideways. Falling back to `/W` is not one of the options
/// the spec offers, and it is what this engine did.
#[test]
fn a_font_without_w2_uses_the_dw2_default_rather_than_its_widths() {
    let found = origins(vertical_document(
        vertical_cmap,
        "",
        &[0x0041, 0x0042],
        (100.0, 700.0),
        800.0,
        false,
    ));

    // /W is 1000 for both CIDs, so v_x is 500 and both glyphs sit 24 points
    // left of the pen — a derived vector, not a defaulted one.
    close(found[0].0, 100.0 - pt(500.0), "A's derived v_x");
    close(found[1].0, 100.0 - pt(500.0), "B's derived v_x");

    close(found[0].1, 700.0 - pt(880.0), "A, at the default v_y");
    close(
        found[1].1,
        700.0 - pt(1000.0) - pt(880.0),
        "B, one default em further down",
    );
}

/// `/DW2` when the file gives one. Both components have to arrive, and in
/// Table 116's order: the first is the position vector's y and the second the
/// displacement. Reading them the other way round would advance the column
/// 123 units *upward*.
#[test]
fn dw2_is_read_in_the_order_the_table_gives() {
    let found = origins(vertical_document(
        vertical_cmap,
        "/DW2 [123 -1234]",
        &[0x0041, 0x0042],
        (100.0, 700.0),
        800.0,
        false,
    ));
    close(found[0].1, 700.0 - pt(123.0), "the position vector's y");
    close(
        found[1].1,
        700.0 - pt(1234.0) - pt(123.0),
        "and the displacement",
    );
}

/// The same file with `/WMode 0` is untouched by any of this: it advances by
/// `/W`, along x, and no position vector is applied. Vertical metrics that
/// leaked into a horizontal run would move every existing document.
#[test]
fn a_horizontal_run_of_the_same_font_does_not_move() {
    let found = origins(vertical_document(
        horizontal_cmap,
        VERTICAL_METRICS,
        &[0x0041, 0x0042],
        (100.0, 700.0),
        800.0,
        false,
    ));
    close(found[0].0, 100.0, "A, at the pen exactly");
    close(found[0].1, 700.0, "and on the baseline");
    close(found[1].0, 100.0 + pt(1000.0), "B, one /W further along");
    close(found[1].1, 700.0, "still on the baseline");
}

/// A vertical line is vertical and is not right-to-left, which is the property
/// these two fields exist to keep apart. Asserted here because the glyph
/// events these fixtures produce are the ones that carry it.
#[test]
fn the_extracted_line_reports_vertical_writing() {
    let doc = open(vertical_document(
        vertical_cmap,
        VERTICAL_METRICS,
        &[0x0041, 0x0042, 0x0043],
        (100.0, 700.0),
        800.0,
        false,
    ));
    let page = doc.page(0).expect("a page").text();
    let lines = page.lines();
    assert_eq!(lines.len(), 1, "one column, one line");
    assert_eq!(lines[0].wmode, WritingMode::Vertical);
    assert!(!lines[0].rtl);
    assert_eq!(lines[0].text, "ABC");
}

/// `TJ`'s numbers move the pen along the writing direction (9.4.3), which for
/// a vertical run is its own column. They went on the horizontal coordinate
/// whatever the mode, so every kern in a vertical run pushed the column
/// sideways and none of them changed the spacing.
#[test]
fn a_tj_adjustment_moves_a_vertical_pen_down_its_column() {
    let plain = origins(vertical_document(
        vertical_cmap,
        VERTICAL_METRICS,
        &[0x0041, 0x0042],
        (100.0, 700.0),
        800.0,
        false,
    ));

    let bytes = document(
        &[
            b"<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding 7 0 R\n\
              /DescendantFonts [6 0 R] >>"
                .to_vec(),
            format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture\n\
                 {HORIZONTAL_WIDTHS} {VERTICAL_METRICS}\n\
                 /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
                 /CIDToGIDMap 8 0 R >>"
            )
            .into_bytes(),
            vertical_cmap(),
            cid_to_gid(),
        ],
        "BT /F0 48 Tf 100 700 Td [<0041> 500 <0042>] TJ ET",
        800.0,
    );
    let kerned = origins(bytes);

    close(kerned[0].0, plain[0].0, "the first glyph is unmoved");
    close(kerned[0].1, plain[0].1, "in both coordinates");
    close(
        kerned[1].0,
        plain[1].0,
        "and the column has not drifted sideways, which is what the \
         adjustment used to do",
    );
    // 9.4.3 subtracts the number from the coordinate the writing mode names,
    // so +500 thousandths takes the pen 24 points further *down*.
    close(
        kerned[1].1,
        plain[1].1 - pt(500.0),
        "the adjustment landed on the vertical coordinate",
    );
}

// ---------------------------------------------------------------------------
// Milestone 3: the position vector reaches the placement.
// ---------------------------------------------------------------------------

/// Milestone 3's exit criterion: a glyph with a non-default `v` draws offset
/// within its em box, and a rendered fixture differs from the same run without
/// the vector.
///
/// One code, CID 5, glyph 3 — a 600-unit box anchored at the glyph origin, so
/// the ink's own corner is the origin and the offset is measurable in pixels.
/// The pen is at (100, 150) on a 200-point page, so with `v = (250, 700)` the
/// box's lower-left corner lands at (88, 116.4) in user space, which is
/// (88, 54.8) from the top of the canvas.
#[test]
fn the_position_vector_moves_the_glyph_on_the_page() {
    let with_vector = render(vertical_document(
        vertical_cmap,
        "/W2 [5 [-400 250 700]]",
        &[0x0041],
        (100.0, 150.0),
        200.0,
        true,
    ));
    let without = render(vertical_document(
        vertical_cmap,
        "/W2 [5 [-400 0 0]]",
        &[0x0041],
        (100.0, 150.0),
        200.0,
        true,
    ));

    for page in [&with_vector, &without] {
        assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));
    }

    // The right glyph drew: a 600-unit box is 28.8 points on a side, and no
    // other glyph in this face is that size.
    let (ax0, ay0, ax1, ay1) = ink_bounds(&with_vector);
    near(ax1 - ax0 + 1.0, pt(600.0), "the box's width");
    near(ay1 - ay0 + 1.0, pt(600.0), "and its height");

    // And it drew where -v put it, not at the pen.
    near(ax0, 100.0 - pt(250.0), "the box's left edge");
    near(
        ay0,
        200.0 - 150.0 - pt(600.0) + pt(700.0),
        "and its top edge",
    );

    // The two renders differ, and by exactly the vector.
    let (bx0, by0, ..) = ink_bounds(&without);
    near(bx0, 100.0, "with no vector the box sits at the pen");
    near(ax0 - bx0, -pt(250.0), "the horizontal half of v");
    near(
        ay0 - by0,
        pt(700.0),
        "and the vertical half, flipped by the page",
    );
    assert_ne!(
        with_vector.data, without.data,
        "the same run with and without the position vector renders the same \
         bytes, so the vector reaches nothing"
    );
}

/// The derived vector is applied too, not only an explicit one. With no `/W2`
/// the glyph still moves — by half its horizontal width sideways and by
/// `/DW2`'s 880 down, which 9.7.4.3 specifies rather than leaving to the
/// implementation.
#[test]
fn a_derived_position_vector_moves_the_glyph_as_well() {
    let page = render(vertical_document(
        vertical_cmap,
        "",
        &[0x0041],
        (100.0, 150.0),
        200.0,
        true,
    ));
    assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));

    let (x0, y0, ..) = ink_bounds(&page);
    // /W gives CID 5 a width of 1000, so v_x is 500.
    near(x0, 100.0 - pt(500.0), "half the horizontal width");
    near(
        y0,
        200.0 - 150.0 - pt(600.0) + pt(880.0),
        "/DW2's default v_y",
    );
}

/// The column in pixels. Two glyphs one `/W2` displacement apart span the
/// glyph plus the step; the horizontal `/W` would make that span half again as
/// tall, and an inverted sign would put the second box above the first rather
/// than below it — which this catches because the top edge is asserted
/// absolutely, not the height alone.
#[test]
fn two_glyphs_stack_downward_by_the_vertical_displacement() {
    let page = render(vertical_document(
        vertical_cmap,
        "/W2 [5 [-400 250 700]]",
        &[0x0041, 0x0041],
        (100.0, 150.0),
        200.0,
        true,
    ));
    assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));

    let (x0, y0, x1, y1) = ink_bounds(&page);
    near(
        x1 - x0 + 1.0,
        pt(600.0),
        "one box wide, since the column is straight",
    );
    near(
        y1 - y0 + 1.0,
        pt(600.0) + pt(400.0),
        "one box plus one displacement tall, not one box plus /W's 1000",
    );
    // The first box's top edge, in device pixels. An upward column would put
    // the ink 19.2 points higher than this.
    near(
        y0,
        200.0 - 150.0 - pt(600.0) + pt(700.0),
        "the top of the run",
    );
}

// ---------------------------------------------------------------------------
// The decoy, and the dependencies.
// ---------------------------------------------------------------------------

/// The decoy is really a decoy: the horizontal metrics say something entirely
/// different from the vertical ones for every CID the fixtures use. Without
/// this, "the vertical metrics were used" would be a claim about numbers that
/// might have agreed all along.
#[test]
fn the_horizontal_widths_would_be_visibly_wrong_if_used() {
    let doc = open(vertical_document(
        vertical_cmap,
        VERTICAL_METRICS,
        &[0x0041],
        (100.0, 700.0),
        800.0,
        false,
    ));
    let cos = doc.cos();
    let page = tinker_pdf_cos::pages::collect(cos);
    let resources = page
        .first()
        .and_then(|p| p.resources.clone())
        .expect("the page has resources");
    let font = tinker_pdf_cos::font::from_resources(cos, &resources)
        .into_values()
        .next()
        .expect("the page has one font");

    assert!(font.is_vertical(), "the CMap declared /WMode 1");
    for (code, cid, w1_y) in [
        (0x41u32, 5u32, -400.0),
        (0x42, 9, -600.0),
        (0x43, 14, -900.0),
    ] {
        let decoded = font.decode(&u16::try_from(code).expect("a two-byte code").to_be_bytes());
        assert_eq!(decoded[0].cid, cid, "code {code:#x} names CID {cid}");
        assert_eq!(font.width_of(code).0, 1000.0, "the horizontal decoy");
        assert_eq!(font.vertical_metrics(cid).2, w1_y, "the vertical truth");
    }
}

/// Gap 03's dependency, made concrete: a `-V` name out of Adobe's registry is
/// a real CMap, so a file using one — rather than `Identity-V` — reaches these
/// metrics through its own CIDs.
///
/// `90ms-RKSJ-V` maps `<8141>` to CID 7887 through its own `cidrange`, over a
/// `90ms-RKSJ-H` parent that says 634. `/W2` names 7887, so a build resolving
/// the parent's CID would take `/DW2`'s default instead and the assertion
/// would fail rather than pass differently.
#[cfg(feature = "cmap-predefined")]
#[test]
fn a_registry_vertical_cmap_reaches_the_vertical_metrics() {
    let bytes = document(
        &[
            b"<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding /90ms-RKSJ-V\n\
              /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>"
                .to_vec(),
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture /DW 1000\n\
              /W2 [7887 [-500 260 710]]\n\
              /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >>\n\
              /CIDToGIDMap /Identity >>"
                .to_vec(),
            stream(
                "/Type /CMap",
                b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                  1 beginbfchar <8141> <3001> endbfchar",
            ),
        ],
        "BT /F0 48 Tf 100 700 Td <81418141> Tj ET",
        800.0,
    );

    let found = origins(bytes);
    assert_eq!(found.len(), 2, "two codes: {found:?}");
    // The ideographic comma, which is exactly the character 9.7.4.3's position
    // vector exists for.
    close(found[0].0, 100.0 - pt(260.0), "v_x from /W2");
    close(found[0].1, 700.0 - pt(710.0), "v_y from /W2");
    close(
        found[1].1,
        700.0 - pt(500.0) - pt(710.0),
        "and the second glyph one /W2 displacement further down",
    );
}

/// A hostile file may say anything about its vertical metrics. None of these
/// may panic (ruling 1) and none may stop the page laying out.
#[test]
fn a_malformed_w2_still_lays_the_page_out() {
    for metrics in [
        "/W2 [5]",
        "/W2 [5 [-400 250]]",
        "/W2 [5 9 -600]",
        "/W2 [5 [(text) (more) (still)]]",
        "/W2 << /A /B >>",
        "/DW2 [880]",
        "/DW2 [(a) (b)]",
        "/W2 [0 65535 -1000 0 880]",
    ] {
        let found = origins(vertical_document(
            vertical_cmap,
            metrics,
            &[0x0041, 0x0042],
            (100.0, 700.0),
            800.0,
            false,
        ));
        assert_eq!(found.len(), 2, "{metrics} lost a glyph");
        assert!(
            found[1].1 < found[0].1,
            "{metrics} stopped the column running downward: {found:?}"
        );
    }
}
