//! The separations inputs: pages whose spots a separations render lays on
//! plates (`docs/design/overprint-separations.md`), written byte by byte.
//!
//! `inputs.rs` takes these and re-exports them, so a group reads them where
//! it reads every other input, and a test that holds a page's plates takes
//! this file alone, by path, as a test that reads a count takes `samples.rs`.
//! Every page is a PDF of integers and literals written here, not by the
//! engine's writer, so it is the same bytes on every target and in every
//! build, the planning baseline's among them.

/// [`spots_page`]'s size in points: A4, a pixel a point at 72 dpi.
pub const SPOTS_PAGE: (u32, u32) = (595, 842);

/// The process ink a spots page lays over its whole page before its squares,
/// in 255ths of cyan, magenta, yellow and black: whole levels, so a process
/// plate holds each exactly where no square stands.
pub const SPOTS_BACKDROP: [u8; 4] = [51, 102, 153, 204];

/// How many names [`SpotSquare::numbered`] gives each of a spots page's
/// `/DeviceN` spaces: one for each process ink its alternate carries a
/// component onto.
pub const SPOTS_PER_SPACE: usize = 4;

/// The columns [`spot_squares`] lays its squares out in.
const COLUMNS: usize = 8;

/// One square a spots page fills: its colorant, the `/DeviceN` space that
/// paints it, its place among that space's names, its box and its tint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotSquare {
    /// The colorant's name, as the page's space writes it.
    pub name: String,
    /// Which of the page's `/DeviceN` spaces paints it: the resource
    /// `/D{space}`.
    pub space: usize,
    /// Its place among its space's names. The space's tint transform carries
    /// component `j` onto process ink `j` unchanged, so a square whose space
    /// is drawn through its alternate lands on that ink at its own tint.
    pub component: usize,
    /// Its box, `[x0, y0, x1, y1]` in points from the page's lower left.
    pub rect: [u32; 4],
    /// The tint it fills its box at, in 255ths.
    pub level: u8,
}

impl SpotSquare {
    /// The `index`th spot of a spots page, filling `rect`: `Spot01` onward,
    /// [`SPOTS_PER_SPACE`] to a `/DeviceN` in order, each at a tint three
    /// 255ths under the one before it from solid, so the first sixty-four are
    /// sixty-four tints and every one is a whole level.
    pub fn numbered(index: usize, rect: [u32; 4]) -> SpotSquare {
        SpotSquare {
            name: format!("Spot{:02}", index + 1),
            space: index / SPOTS_PER_SPACE,
            component: index % SPOTS_PER_SPACE,
            rect,
            level: u8::try_from(255 - 3 * (index % 64)).expect("at least 66"),
        }
    }
}

/// The squares [`spots_page`] fills for `count` spots: eight to a row from
/// the top left of A4, each 65 points wide with 5 between, the rows sharing
/// 800 points of the page's height with 4 points between them.
pub fn spot_squares(count: usize) -> Vec<SpotSquare> {
    let rows = u32::try_from(count.div_ceil(COLUMNS).max(1)).unwrap_or(u32::MAX);
    let pitch = 800 / rows;
    (0..count)
        .map(|index| {
            let column = u32::try_from(index % COLUMNS).expect("under eight");
            let row = u32::try_from(index / COLUMNS).unwrap_or(u32::MAX);
            let x0 = 20 + 70 * column;
            let top = 821u32.saturating_sub(pitch.saturating_mul(row));
            let bottom = top.saturating_sub(pitch.saturating_sub(4));
            SpotSquare::numbered(index, [x0, bottom, x0 + 65, top])
        })
        .collect()
}

/// A page of A4 naming `count` spots, [`SPOTS_PER_SPACE`] to a `/DeviceN`
/// space, each filling its square of [`spot_squares`] over a backdrop of
/// process ink: the render group's 64-spot page is `spots_page(64)`.
pub fn spots_page(count: usize) -> Vec<u8> {
    spots_page_of(SPOTS_PAGE, &spot_squares(count))
}

/// A page `page` points wide and high naming the spot of each of `squares`
/// and filling its box: first [`SPOTS_BACKDROP`]'s process ink over the whole
/// page, then each square in its `/DeviceN`, its own component at its tint
/// and every other component of the space at none.
///
/// The page's `/ColorSpace` holds `/D0`, `/D1` and onward in order, each a
/// `/DeviceN` of the names its squares give in component order — `/None`
/// where no square takes a component — onto DeviceCMYK by a type 4 function
/// that carries component `j` onto ink `j`, so the page's inventory is the
/// squares' names in that order. Every edge is on a whole point.
///
/// # Panics
///
/// Where a square's component is past the four inks its space's alternate
/// carries its components onto.
pub fn spots_page_of(page: (u32, u32), squares: &[SpotSquare]) -> Vec<u8> {
    let spaces = squares
        .iter()
        .map(|square| square.space + 1)
        .max()
        .unwrap_or(0);
    let mut names: Vec<Vec<Option<&str>>> = vec![Vec::new(); spaces];
    for square in squares {
        assert!(
            square.component < 4,
            "{} is component {}, and a spots page's space carries its components onto the \
             four process inks",
            square.name,
            square.component
        );
        let slots = &mut names[square.space];
        if slots.len() <= square.component {
            slots.resize(square.component + 1, None);
        }
        slots[square.component] = Some(&square.name);
    }

    let (width, height) = page;
    let mut colour_spaces = String::new();
    let mut functions = Vec::new();
    for (space, slots) in names.iter().enumerate() {
        let listed: Vec<String> = slots
            .iter()
            .map(|slot| format!("/{}", slot.unwrap_or("None")))
            .collect();
        // A space no square paints in names nothing and is not written; its
        // function is, so each space's stays object `5 + space`.
        if !listed.is_empty() {
            colour_spaces.push_str(&format!(
                "/D{space} [/DeviceN [{}] /DeviceCMYK {} 0 R] ",
                listed.join(" "),
                5 + space
            ));
        }
        // Component `j` onto ink `j`: the inks past the space's names at
        // none, and a space of four its components as they are.
        let program = match slots.len() {
            4 => "0 add".to_string(),
            n => vec!["0"; 4 - n].join(" "),
        };
        let domain = vec!["0 1"; slots.len().max(1)].join(" ");
        functions.push(stream(
            &format!("/FunctionType 4 /Domain [{domain}] /Range [0 1 0 1 0 1 0 1]"),
            format!("{{ {program} }}").as_bytes(),
        ));
    }

    let [c, m, y, k] = SPOTS_BACKDROP.map(fraction);
    let mut content = format!("{c} {m} {y} {k} k 0 0 {width} {height} re f\n");
    for square in squares {
        let tints: Vec<String> = (0..names[square.space].len())
            .map(|component| {
                if component == square.component {
                    fraction(square.level)
                } else {
                    "0".to_string()
                }
            })
            .collect();
        let [x0, y0, x1, y1] = square.rect;
        content.push_str(&format!(
            "/D{} cs {} scn {x0} {y0} {} {} re f\n",
            square.space,
            tints.join(" "),
            x1.saturating_sub(x0),
            y1.saturating_sub(y0)
        ));
    }

    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /ColorSpace << {colour_spaces}>> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream("", content.as_bytes()),
    ];
    objects.extend(functions);
    pdf(&objects)
}

/// [`separations_page`]'s size in points: A4.
pub const SEPARATIONS_PAGE: (u32, u32) = (595, 842);

/// The spots [`separations_page`] names, in the order its resources name
/// them: its plates after the four process ones.
pub const SEPARATIONS_SPOTS: [&str; 4] = ["Gold", "Silver", "Teal", "Plum"];

/// The face [`separations_page`] sets its text in, embedded whole, so the
/// text draws alike in every build, `bundled-fonts` or not, and nothing is
/// read at run time.
const FACE: &[u8] =
    include_bytes!("../../../tinker-pdf-font/data/liberation/LiberationSans-Regular.ttf");

/// `/Gold`, a `/Separation` onto DeviceCMYK.
const GOLD: &str = "[/Separation /Gold /DeviceCMYK \
    << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0.25 0.9 0.1] /N 1 >>]";

/// `/Plum`, a `/Separation` onto DeviceCMYK.
const PLUM: &str = "[/Separation /Plum /DeviceCMYK \
    << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0.5 0.9 0 0.2] /N 1 >>]";

/// `/DeviceN [/Silver /Teal]` onto DeviceCMYK through object 5.
const SILVER_TEAL: &str = "[/DeviceN [/Silver /Teal] /DeviceCMYK 5 0 R]";

/// `/DeviceN [/Gold /Plum]` onto DeviceCMYK through object 14.
const GOLD_PLUM: &str = "[/DeviceN [/Gold /Plum] /DeviceCMYK 14 0 R]";

/// The separations fixture: an A4 page putting four spots on their plates by
/// every road a separations render takes, over a backdrop of process ink.
///
/// - **Flat colours**: a `/Separation` of Gold, a `/DeviceN` of Silver and
///   Teal and a `/Separation` of Plum filled over one another, a DeviceRGB
///   fill turned into ink beside them, and a line stroked in Teal.
/// - **Text** in Liberation Sans embedded whole: a line in Gold, one in Silver
///   and Teal, one in process black.
/// - **Groups**: an isolated knockout group of Gold and Silver-and-Teal at a
///   constant alpha of 0.7, drawn under `Multiply` at 0.6; and an isolated
///   group of DeviceRGB, in which Gold and Plum composite through their
///   alternates (11.7.3) and reach the process plates as light turned into
///   ink.
/// - **Images**: 32 x 16 samples in a `/DeviceN` of Gold and Plum, drawn
///   eight times their size, and a 24 x 12 stencil mask painting Silver and
///   Teal.
/// - **A shading**: an axial ramp of Plum under a clip, by `sh`.
/// - **A mesh**: two free-form triangles in Silver and Teal, by `sh`.
///
/// Every spot is a plate of a separations render's default plan, so its
/// plates are the four process ones and [`SEPARATIONS_SPOTS`]'s.
pub fn separations_page() -> Vec<u8> {
    let (width, height) = SEPARATIONS_PAGE;
    let content = format!(
        "0.08 0.04 0 0.06 k 0 0 {width} {height} re f\n\
         0.2 0.6 0.4 rg 420 700 140 110 re f\n\
         /G cs 1 scn 30 720 160 90 re f\n\
         /DN cs 0.6 0.4 scn 120 690 200 70 re f\n\
         /P cs 0.75 scn 240 740 150 60 re f\n\
         /DN CS 0 1 SCN 4 w 30 670 m 300 670 l 560 655 l S\n\
         BT /F0 30 Tf /G cs 0.85 scn 30 605 Td (Separations, plate by plate) Tj ET\n\
         BT /F0 18 Tf /DN cs 0.9 0.5 scn 30 575 Td (Silver and teal through one space) Tj ET\n\
         BT /F0 14 Tf 0 0 0 1 k 30 550 Td (Process black beside them) Tj ET\n\
         q /GS0 gs /Fm0 Do Q\n\
         q /Fm1 Do Q\n\
         q 260 0 0 130 30 260 cm /Im0 Do Q\n\
         /DN cs 0.3 0.9 scn q 230 0 0 120 330 265 cm /Mk0 Do Q\n\
         q 30 40 260 190 re W n /Sh0 sh Q\n\
         q /Sh1 sh Q\n"
    );

    // Silver s and Teal t onto cyan 0.8t, no magenta, yellow 0.3t and black
    // 0.4s; Gold g and Plum p onto cyan 0.5p, magenta 0.9p, yellow 0.9g and
    // black 0.1g. Each program leaves its four inks over its inputs and rolls
    // the inputs off.
    let silver_teal = stream(
        "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
        b"{ dup 0.8 mul 0 2 index 0.3 mul 4 index 0.4 mul 6 4 roll pop pop }",
    );
    let gold_plum = stream(
        "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
        b"{ dup 0.5 mul 1 index 0.9 mul 3 index 0.9 mul 4 index 0.1 mul 6 4 roll pop pop }",
    );

    let widths = vec!["556"; 95].join(" ");
    let font = format!(
        "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSans \
         /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] \
         /FontDescriptor 7 0 R >>"
    );
    let descriptor = "<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 \
                      /FontBBox [-1000 -300 2000 1000] /ItalicAngle 0 /Ascent 905 \
                      /Descent -212 /CapHeight 716 /StemV 80 /FontFile2 8 0 R >>";
    let program = stream(&format!("/Length1 {}", FACE.len()), FACE);

    // Gold across, Plum down: every sample its own pair.
    let mut samples = Vec::with_capacity(32 * 16 * 2);
    for y in 0..16u8 {
        for x in 0..32u8 {
            samples.extend_from_slice(&[x * 8, y * 16]);
        }
    }
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 32 /Height 16 /BitsPerComponent 8 \
             /ColorSpace {GOLD_PLUM}"
        ),
        &samples,
    );
    // A checker of four-sample cells; a sample of 0 paints (8.9.6.2).
    let mut bits = Vec::with_capacity(3 * 12);
    for y in 0..12usize {
        for byte in 0..3usize {
            let mut packed = 0u8;
            for bit in 0..8usize {
                let x = byte * 8 + bit;
                if (x / 4 + y / 4) % 2 == 1 {
                    packed |= 0x80 >> bit;
                }
            }
            bits.push(packed);
        }
    }
    let stencil = stream(
        "/Type /XObject /Subtype /Image /Width 24 /Height 12 /ImageMask true \
         /BitsPerComponent 1",
        &bits,
    );

    let knockout = stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 {width} {height}] \
             /Group << /S /Transparency /I true /K true >> \
             /Resources << /ColorSpace << /G {GOLD} /DN {SILVER_TEAL} >> \
             /ExtGState << /GS1 << /ca 0.7 >> >> >>"
        ),
        b"/GS1 gs /G cs 0.9 scn 30 410 180 110 re f /DN cs 0.8 0.3 scn 120 430 180 90 re f",
    );
    let light = stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 {width} {height}] \
             /Group << /S /Transparency /CS /DeviceRGB /I true >> \
             /Resources << /ColorSpace << /G {GOLD} /P {PLUM} >> >>"
        ),
        b"/G cs 1 scn 360 410 90 110 re f /P cs 0.5 scn 410 430 150 80 re f",
    );

    let axial = format!(
        "<< /ShadingType 2 /ColorSpace {PLUM} /Coords [30 0 290 0] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
         /Extend [true true] >>"
    );
    // Two triangles, each vertex a flag, two 16-bit coordinates decoded as
    // points and a byte of each spot.
    let mut mesh = Vec::with_capacity(6 * 7);
    for (x, y, silver, teal) in [
        (320u16, 40u16, 0u8, 255u8),
        (565, 40, 255, 0),
        (565, 230, 128, 255),
        (320, 40, 0, 255),
        (565, 230, 128, 255),
        (320, 230, 255, 128),
    ] {
        mesh.push(0);
        mesh.extend_from_slice(&x.to_be_bytes());
        mesh.extend_from_slice(&y.to_be_bytes());
        mesh.extend_from_slice(&[silver, teal]);
    }
    let triangles = stream(
        &format!(
            "/ShadingType 4 /ColorSpace {SILVER_TEAL} /BitsPerCoordinate 16 \
             /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 65535 0 65535 0 1 0 1]"
        ),
        &mesh,
    );

    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /ColorSpace << /G {GOLD} /DN {SILVER_TEAL} /P {PLUM} >> \
             /Font << /F0 6 0 R >> \
             /XObject << /Im0 9 0 R /Mk0 10 0 R /Fm0 11 0 R /Fm1 12 0 R >> \
             /Shading << /Sh0 13 0 R /Sh1 15 0 R >> \
             /ExtGState << /GS0 << /BM /Multiply /ca 0.6 /CA 0.6 >> >> >> \
             /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream("", content.as_bytes()),
        silver_teal,
        font.into_bytes(),
        descriptor.as_bytes().to_vec(),
        program,
        image,
        stencil,
        knockout,
        light,
        axial.into_bytes(),
        gold_plum,
        triangles,
    ];
    pdf(&objects)
}

/// `level` 255ths as a content stream writes a tint: six decimals, which
/// `round(255 × t)` takes back to the level.
fn fraction(level: u8) -> String {
    format!("{:.6}", f64::from(level) / 255.0)
}

/// A stream object: `entries` and its `/Length`, then `data`.
fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A document of `objects`, numbered from one, the first its catalog, with
/// its cross-reference table, so nothing about opening it is repaired.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
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
