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
