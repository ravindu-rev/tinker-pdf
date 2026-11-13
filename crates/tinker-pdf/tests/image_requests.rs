//! The decode-to-target-size handshake, held end to end
//! (`docs/design/image-edges.md`, "The handshake's drawing half").
//!
//! A decoder that answers a drawn size with fewer samples — a JPEG at a half,
//! a quarter or an eighth (`docs/design/jpeg-speed.md`, "The scale rule"), a
//! JPEG 2000 codestream with resolution levels dropped and the part of it a
//! draw shows (`docs/design/jpx-speed.md`, "The page's request" and "The
//! visible window") — hands the draw a smaller grid over the same unit square,
//! and the integer resampler draws it there. Every answer here is drawn at a
//! size that picks it — axis-aligned, flipped, quarter-turned and sheared, the
//! images odd in size — and held to the same draw of the full decode.
//!
//! The full decode is the same image under a `/Mask` nothing can apply: a
//! colour key whose ranges run backwards. The reducer keeps every image with a
//! `/Mask` whole, as a mask sampled against the grid it was drawn for needs
//! (8.9.6.3, 8.9.6.4), and a colour key that names no range of samples masks
//! nothing, so the two draws part in their decode alone.
//!
//! What a decode chose is read from the render's counted work (MS-01):
//! `Stats::jpeg_samples_transformed`, `N × N` samples for each block of a JPEG
//! decoded at `N/8`, held to the decoder's own count at each scale; and
//! `Stats::jpx_levels_discarded`, the levels a codestream's decode dropped.
//! Each axis's samples then follow from T.81's and T.800's size rules:
//! `⌈X·N/8⌉` for a JPEG (`JpegScale::length`), and `⌈X / 2^d⌉` for a
//! codestream whose image starts at the grid's origin (B.5).

use tinker_pdf::{Bitmap, Document, PixelFormat, PixelRegion, RenderOptions, Stats};
use tinker_pdf_filters::{
    jpeg_decode_strips, jpeg_encode, jpx_decode_request, jpx_header, JpegLayout, JpegOptions,
    JpegSampling, JpegScale, JpegSink, JpegSource, JpegSourceColour, JpxRequest, Limits,
};

/// The ceiling every decode the test makes itself is held to.
const CEILING: usize = 1 << 28;

// ---- the images ---------------------------------------------------------------

/// The scene a JPEG here holds, `channels` bytes a sample: a gradient that
/// darkens toward the bottom right, under a soft dark patch a third of the way
/// across and two fifths of the way down, so the picture's weight sits off its
/// centre and a grid placed wrong carries it somewhere else.
fn scene(width: u32, height: u32, channels: u32) -> Vec<u8> {
    let (w, h) = (i64::from(width), i64::from(height));
    let (cx, cy) = (w / 3, h * 2 / 5);
    let radius = (w.min(h) / 4).max(1);
    let mut samples = Vec::with_capacity((width * height * channels) as usize);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x - cx, y - cy);
            let inside = radius * radius - (dx * dx + dy * dy);
            let patch = if inside > 0 {
                inside * 140 / (radius * radius)
            } else {
                0
            };
            let base = 215 - x * 50 / w - y * 40 / h;
            for channel in 0..i64::from(channels) {
                let tint = channel * 12 - 12;
                samples.push((base - patch + tint).clamp(0, 255) as u8);
            }
        }
    }
    samples
}

/// [`scene`] as a JPEG the engine's own encoder writes, in `colour` with
/// `sampling`'s factors.
fn jpeg(width: u32, height: u32, colour: JpegSourceColour, sampling: JpegSampling) -> Vec<u8> {
    let channels = u32::from(colour.components());
    let samples = scene(width, height, channels);
    jpeg_encode(
        &JpegSource {
            width,
            height,
            colour,
            stride: (width * channels) as usize,
            data: &samples,
        },
        &JpegOptions {
            sampling,
            ..JpegOptions::default()
        },
    )
    .expect("the scene encodes")
}

/// `name` in `directory`, under the crate's own directory.
fn read(directory: &str, name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(directory)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// What a reducer may answer an image's draw with.
#[derive(Clone, Copy, Debug)]
enum Codec {
    /// A JPEG: a half, a quarter or an eighth.
    Jpeg,
    /// A JPEG 2000 codestream whose image starts at the grid's origin, and
    /// that can drop `levels` resolution levels (its header's).
    Jpx { levels: u8 },
}

/// An image the suite draws.
struct Picture {
    /// The stream's bytes.
    data: Vec<u8>,
    /// Its dictionary's keys beside `/Subtype /Image`.
    keys: String,
    /// The `/Mask` that asks for its full decode: a colour key of backwards
    /// ranges, one a component.
    whole: &'static str,
    width: u32,
    height: u32,
    codec: Codec,
}

/// The images, by what each holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// A 203 by 157 RGB JPEG at 4:2:0: odd both ways, its sixteen-sample
    /// MCUs cut by both edges.
    Scene,
    /// A 99 by 61 grey JPEG, odd both ways.
    Grey,
    /// `c2-3.jp2`: 17 by 13 RGB, the reversible 5/3 and the RCT, two levels.
    Small,
    /// `r2-i3.jp2`: 17 by 13 grey, the irreversible 9/7, two levels.
    SmallGrey,
    /// `reversible-310x438.jp2`: 310 by 438 RGB, the 5/3, 64 by 64
    /// code-blocks, so a window decodes fewer of them.
    Reversible,
    /// `irreversible-620x877.jp2`: 620 by 877 RGB, the 9/7 and the ICT, five
    /// levels.
    Irreversible,
}

impl Source {
    /// The resource name the page of every answer draws it by.
    fn name(self) -> &'static str {
        match self {
            Source::Scene => "Scene",
            Source::Grey => "Grey",
            Source::Small => "Small",
            Source::SmallGrey => "SmallGrey",
            Source::Reversible => "Reversible",
            Source::Irreversible => "Irreversible",
        }
    }
}

/// The image `source` names.
fn picture(source: Source) -> Picture {
    let jpeg_picture = |width, height, colour, sampling, space: &str, whole| Picture {
        data: jpeg(width, height, colour, sampling),
        keys: format!(
            "/Width {width} /Height {height} /ColorSpace {space} /BitsPerComponent 8 \
             /Filter /DCTDecode"
        ),
        whole,
        width,
        height,
        codec: Codec::Jpeg,
    };
    let jpx_picture = |data: Vec<u8>, whole| {
        let header = jpx_header(&data, &Limits::new(CEILING)).expect("the header reads");
        assert_eq!(
            header.origin,
            (0, 0),
            "the image starts at the grid's origin"
        );
        Picture {
            keys: format!(
                "/Width {} /Height {} /Filter /JPXDecode",
                header.width, header.height
            ),
            whole,
            width: header.width,
            height: header.height,
            codec: Codec::Jpx {
                levels: header.levels,
            },
            data,
        }
    };
    let fixture = |name| read("../tinker-pdf-filters/tests/jpx", name);
    let committed = |name| read("benches/data", name);
    match source {
        Source::Scene => jpeg_picture(
            203,
            157,
            JpegSourceColour::Rgb,
            JpegSampling::FourTwoZero,
            "/DeviceRGB",
            "/Mask [1 0 1 0 1 0]",
        ),
        Source::Grey => jpeg_picture(
            99,
            61,
            JpegSourceColour::Gray,
            JpegSampling::FourFourFour,
            "/DeviceGray",
            "/Mask [1 0]",
        ),
        Source::Small => jpx_picture(fixture("c2-3.jp2"), "/Mask [1 0 1 0 1 0]"),
        Source::SmallGrey => jpx_picture(fixture("r2-i3.jp2"), "/Mask [1 0]"),
        Source::Reversible => {
            jpx_picture(committed("reversible-310x438.jp2"), "/Mask [1 0 1 0 1 0]")
        }
        Source::Irreversible => {
            jpx_picture(committed("irreversible-620x877.jp2"), "/Mask [1 0 1 0 1 0]")
        }
    }
}

// ---- the answers ----------------------------------------------------------------

/// One draw, at a size that picks one answer.
#[derive(Clone, Copy, Debug)]
struct Case {
    what: &'static str,
    source: Source,
    /// The draw's `cm`, `a b c d e f`, in points.
    matrix: [f64; 6],
    /// The rectangle the draw is clipped to, `x y width height`, in points.
    clip: Option<[f64; 4]>,
    /// The render's scale.
    scale: f64,
    /// The answer the drawn size picks: `N` of a JPEG's `N/8`, or the levels
    /// a codestream's decode drops.
    answer: u32,
    /// Whether the draw is the answer's own fraction of the image both ways,
    /// where A7's JPEG bound reads.
    exact: bool,
    /// Whether the clip leaves the decode a window of the image that holds
    /// fewer of its code-blocks than the whole.
    window: bool,
}

/// A draw with no clip, at 72 dpi.
const fn case(
    what: &'static str,
    source: Source,
    matrix: [f64; 6],
    answer: u32,
    exact: bool,
) -> Case {
    Case {
        what,
        source,
        matrix,
        clip: None,
        scale: 1.0,
        answer,
        exact,
        window: false,
    }
}

/// Every answer, drawn every way. The 203 by 157 JPEG covers a draw at an
/// eighth up to 25.375 by 19.625 pixels, a quarter up to 50.75 by 39.25 and a
/// half up to 101.5 by 78.5; the 99 by 61 one at an eighth up to 12.375 by
/// 7.625 and a half up to 49.5 by 30.5. A codestream `W` by `H` drops `d`
/// levels while `W >> d` by `H >> d` covers the draw: 8 by 6 and 4 by 3 for
/// the 17 by 13 fixtures, 77 by 109 at two for the 310 by 438 one, and 77 by
/// 109 at three for the 620 by 877 one.
const CASES: [Case; 18] = [
    case(
        "a JPEG at a half, axis-aligned",
        Source::Scene,
        [101.5, 0.0, 0.0, 78.5, 3.25, 4.5],
        4,
        true,
    ),
    case(
        "a JPEG at a quarter, flipped left to right",
        Source::Scene,
        [-50.75, 0.0, 0.0, 39.25, 56.6, 3.7],
        2,
        true,
    ),
    case(
        "a JPEG at an eighth, quarter-turned",
        Source::Scene,
        [0.0, 25.375, -19.625, 0.0, 22.4, 2.2],
        1,
        true,
    ),
    case(
        "a JPEG at a quarter, sheared",
        Source::Scene,
        [50.0, 0.0, 12.0, 37.0, 2.5, 3.2],
        2,
        false,
    ),
    case(
        "a JPEG at a half, flipped both ways",
        Source::Scene,
        [-101.5, 0.0, 0.0, -78.5, 104.9, 81.7],
        4,
        true,
    ),
    Case {
        scale: 2.0,
        ..case(
            "a JPEG at an eighth at twice the scale",
            Source::Scene,
            [12.6875, 0.0, 0.0, 9.8125, 2.1, 3.3],
            1,
            true,
        )
    },
    case(
        "a JPEG a hair past half its width, whole",
        Source::Scene,
        [101.6, 0.0, 0.0, 78.5, 3.25, 4.5],
        8,
        false,
    ),
    case(
        "a grey JPEG at a half, flipped top to bottom",
        Source::Grey,
        [49.5, 0.0, 0.0, -30.5, 3.3, 34.2],
        4,
        true,
    ),
    case(
        "a grey JPEG at an eighth, sheared",
        Source::Grey,
        [12.0, 3.0, 0.0, 7.5, 2.1, 2.4],
        1,
        false,
    ),
    case(
        "a codestream with one level dropped, axis-aligned",
        Source::Small,
        [8.0, 0.0, 0.0, 6.0, 2.6, 3.1],
        1,
        false,
    ),
    case(
        "a codestream with two levels dropped, quarter-turned",
        Source::Small,
        [0.0, 4.0, -3.0, 0.0, 5.3, 2.7],
        2,
        false,
    ),
    case(
        "a 9/7 codestream with one level dropped, flipped",
        Source::SmallGrey,
        [-8.0, 0.0, 0.0, 6.0, 10.4, 2.9],
        1,
        false,
    ),
    case(
        "a 9/7 codestream with two levels dropped, sheared",
        Source::SmallGrey,
        [4.0, 0.0, 1.0, 2.5, 2.2, 3.4],
        2,
        false,
    ),
    case(
        "a codestream with two levels dropped, axis-aligned",
        Source::Reversible,
        [77.0, 0.0, 0.0, 109.0, 2.3, 3.6],
        2,
        false,
    ),
    // Every subband this discard keeps is one code-block, so the window the
    // clip leaves saves none of them; the draw is held to the full decode
    // all the same.
    Case {
        clip: Some([2.3, 60.1, 31.5, 52.5]),
        ..case(
            "a codestream with two levels dropped, clipped to its top left",
            Source::Reversible,
            [77.0, 0.0, 0.0, 109.0, 2.3, 3.6],
            2,
            false,
        )
    },
    Case {
        clip: Some([100.3, 10.2, 40.5, 35.7]),
        window: true,
        ..case(
            "a codestream with one level dropped and a window",
            Source::Reversible,
            [155.0, 0.0, 0.0, 219.0, 2.5, 2.5],
            1,
            false,
        )
    },
    Case {
        clip: Some([20.2, 300.4, 60.6, 50.3]),
        window: true,
        ..case(
            "a codestream at its own size with a window",
            Source::Reversible,
            [310.0, 0.0, 0.0, 438.0, 1.5, 1.5],
            0,
            false,
        )
    },
    case(
        "a 9/7 codestream with three levels dropped, flipped top to bottom",
        Source::Irreversible,
        [77.0, 0.0, 0.0, -109.0, 3.3, 112.7],
        3,
        false,
    ),
];

/// The device pixels a draw spans along the image's own two axes: the
/// lengths of the two columns of its matrix in the page frame, which is what
/// a draw asks a decode for (`ImageRequest::drawn_under`).
fn drawn(case: &Case) -> (f64, f64) {
    let [a, b, c, d, ..] = case.matrix;
    (
        case.scale * (a * a + b * b).sqrt(),
        case.scale * (c * c + d * d).sqrt(),
    )
}

/// The answer the scale rule gives a JPEG of `width` by `height` drawn over
/// `drawn` pixels: the smallest `N` of 1, 2 and 4 for which `W·N/8` and
/// `H·N/8` cover the draw both ways, and 8, its own size, where none does.
fn jpeg_rule((width, height): (u32, u32), (across, down): (f64, f64)) -> u32 {
    [1u32, 2, 4]
        .into_iter()
        .find(|&n| {
            f64::from(width) * f64::from(n) / 8.0 >= across
                && f64::from(height) * f64::from(n) / 8.0 >= down
        })
        .unwrap_or(8)
}

/// The answer the discard rule gives a codestream of `width` by `height`
/// that can drop `levels` levels, drawn over `drawn` pixels: the most `d` for
/// which `W >> d` and `H >> d` cover the draw both ways, and at least one
/// sample.
fn jpx_rule((width, height): (u32, u32), levels: u8, (across, down): (f64, f64)) -> u32 {
    let holds = |samples: u32, d: u32, drawn: f64| f64::from(samples >> d) >= drawn.max(1.0);
    let mut d = 0u32;
    while d < u32::from(levels).min(31) && holds(width, d + 1, across) && holds(height, d + 1, down)
    {
        d += 1;
    }
    d
}

/// The JPEG scale whose `N` is `n`.
fn scale_of(n: u32) -> JpegScale {
    match n {
        1 => JpegScale::Eighth,
        2 => JpegScale::Quarter,
        4 => JpegScale::Half,
        _ => JpegScale::Full,
    }
}

/// A strip sink that keeps nothing: the decode is made for its count.
struct Nowhere;

impl JpegSink for Nowhere {
    fn begin(&mut self, _layout: JpegLayout) {}

    fn strip(&mut self, _y: u32, _rows: u32, _data: &[u8]) {}
}

/// The samples the JPEG `data`'s inverse transforms output at `N/8`, as the
/// decoder itself counts them.
fn transformed_at(data: &[u8], n: u32) -> u64 {
    jpeg_decode_strips(data, CEILING, scale_of(n), &mut Nowhere)
        .expect("the JPEG decodes")
        .transformed
}

/// The answer a render's decode of `picture` chose, read from the render's
/// counted work: the `N` whose decode transforms exactly the samples the
/// render counted, or the levels it counted dropped. Each scale of a JPEG
/// transforms `N × N` samples a block, so no two scales count alike.
fn chosen(picture: &Picture, stats: &Stats) -> u32 {
    match picture.codec {
        Codec::Jpeg => [1u32, 2, 4, 8]
            .into_iter()
            .find(|&n| transformed_at(&picture.data, n) == stats.jpeg_samples_transformed)
            .unwrap_or_else(|| {
                panic!(
                    "no scale transforms the {} samples the render counted",
                    stats.jpeg_samples_transformed
                )
            }),
        Codec::Jpx { .. } => u32::try_from(stats.jpx_levels_discarded).expect("a level count"),
    }
}

/// The samples along each of the image's axes an answer leaves: `⌈X·N/8⌉`
/// by `⌈Y·N/8⌉` of a JPEG (T.81), and `⌈X / 2^d⌉` by `⌈Y / 2^d⌉` of a
/// codestream at the grid's origin (T.800 B.5).
fn samples_at(picture: &Picture, answer: u32) -> (u32, u32) {
    match picture.codec {
        Codec::Jpeg => {
            let scale = scale_of(answer);
            (scale.length(picture.width), scale.length(picture.height))
        }
        Codec::Jpx { .. } => (
            picture.width.div_ceil(1 << answer),
            picture.height.div_ceil(1 << answer),
        ),
    }
}

// ---- the renders ------------------------------------------------------------------

/// A page of `size` points drawing `content`, its resources the images
/// `images`, each `(name, keys, bytes)`.
fn page(size: (f64, f64), content: &str, images: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let names: String = images
        .iter()
        .enumerate()
        .map(|(at, (name, ..))| format!("/{name} {} 0 R ", at + 5))
        .collect();
    let mut file = format!(
        "%PDF-1.7\n\
         1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
         2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
         3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}]\n\
         /Resources << /XObject << {names}>> >> /Contents 4 0 R >>\nendobj\n\
         4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        size.0,
        size.1,
        content.len()
    )
    .into_bytes();
    for (at, (_, keys, data)) in images.iter().enumerate() {
        file.extend_from_slice(
            format!(
                "{} 0 obj\n<< /Type /XObject /Subtype /Image {keys} /Length {} >>\nstream\n",
                at + 5,
                data.len()
            )
            .as_bytes(),
        );
        file.extend_from_slice(data);
        file.extend_from_slice(b"\nendstream\nendobj\n");
    }
    file.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            images.len() + 5
        )
        .as_bytes(),
    );
    file
}

/// The corners of the unit square under `matrix`, and of `clip`.
fn corners(case: &Case) -> Vec<(f64, f64)> {
    let [a, b, c, d, e, f] = case.matrix;
    let mut points = vec![
        (e, f),
        (a + e, b + f),
        (c + e, d + f),
        (a + c + e, b + d + f),
    ];
    if let Some([x, y, w, h]) = case.clip {
        points.extend([(x, y), (x + w, y + h)]);
    }
    points
}

/// The page a case is drawn on alone: whatever it reaches, and a margin.
fn page_size(case: &Case) -> (f64, f64) {
    let points = corners(case);
    let right = points.iter().map(|p| p.0).fold(0.0, f64::max);
    let top = points.iter().map(|p| p.1).fold(0.0, f64::max);
    (right.ceil() + 6.0, top.ceil() + 5.0)
}

/// The draw a case makes of the image named `name`, clipped as it says.
fn draw(case: &Case, name: &str) -> String {
    let clip = case.clip.map_or(String::new(), |[x, y, w, h]| {
        format!("{x} {y} {w} {h} re W n ")
    });
    let [a, b, c, d, e, f] = case.matrix;
    format!("q {clip}{a} {b} {c} {d} {e} {f} cm /{name} Do Q")
}

/// One draw rendered over the page's white and over black, and what the
/// first render did, counted.
struct Pair {
    white: Bitmap,
    black: Bitmap,
    stats: Stats,
}

/// `case` drawn alone, its image `picture` under `keys`.
fn rendered(case: &Case, picture: &Picture, keys: &str) -> Pair {
    let size = page_size(case);
    let content = draw(case, "Im0");
    let render = |content: &str| {
        Document::open(page(size, content, &[("Im0", keys, &picture.data[..])]))
            .expect("it opens")
            .page(0)
            .expect("a page")
            .render(&RenderOptions {
                scale: case.scale,
                stats: true,
                ..RenderOptions::default()
            })
    };
    let white = render(&content);
    let black = render(&format!("0 g 0 0 {} {} re f {content}", size.0, size.1));
    assert_eq!(white.format, PixelFormat::Rgb8, "{}", case.what);
    let stats = white.stats.expect("the render counted");
    Pair {
        white,
        black,
        stats,
    }
}

/// Each channel of each pixel over white less over black: `255 − c` for the
/// coverage `c` the draw composited the pixel at, whatever its colour — over
/// white a channel is `mul255(s, c) + (255 − c)` and over black `mul255(s, c)`
/// — so it reads the image's ink and the coverage of its edge, the samples
/// entering only through the composite's rounding: 255 exactly where the image
/// does not reach, 0 exactly where it covers the pixel wholly.
fn uncovered(pair: &Pair) -> Vec<i16> {
    pair.white
        .data
        .iter()
        .zip(&pair.black.data)
        .map(|(&white, &black)| i16::from(white) - i16::from(black))
        .collect()
}

/// The pixels a draw inked, as `(x0, y0, x1, y1)` with the far edges
/// exclusive: every pixel any channel of which the image reached.
fn ink_bounds(pair: &Pair) -> Option<(u32, u32, u32, u32)> {
    let bitmap = &pair.white;
    let uncovered = uncovered(pair);
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let at = y as usize * bitmap.stride + x as usize * 3;
            if uncovered[at..at + 3].iter().all(|&left| left == 255) {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x + 1, y + 1),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
            });
        }
    }
    bounds
}

/// The pixels a draw covers wholly — the same over white and over black —
/// and their colours.
fn wholly(pair: &Pair) -> Vec<(u32, u32, [u8; 3])> {
    let (white, black) = (&pair.white, &pair.black);
    let mut out = Vec::new();
    for y in 0..white.height {
        for x in 0..white.width {
            let at = y as usize * white.stride + x as usize * 3;
            let pixel = [white.data[at], white.data[at + 1], white.data[at + 2]];
            if pixel[..] == black.data[at..at + 3] {
                out.push((x, y, pixel));
            }
        }
    }
    out
}

/// The fewest wholly covered pixels whose colours [`lands`] weighs: below it
/// a draw is mostly its own edge, which [`uncovered`] holds exactly.
const WEIGHED: usize = 150;

/// **The samples land where the full decode's do**: over the pixels both
/// cover wholly, the weight of the picture's darkness — 255 less its green —
/// sits within half a pixel of the full decode's on both axes, and the colours
/// are near the full decode's, the decoder's low band standing in for the
/// resampler's box filter. A grid placed a sample out, or stretched over the
/// whole square rather than the part its samples span, moves the weight by a
/// drawn pixel.
fn lands(case: &Case, reduced: &Pair, whole: &Pair) {
    let (got, want) = (wholly(reduced), wholly(whole));
    assert_eq!(
        got.len(),
        want.len(),
        "{}: the same pixels covered wholly",
        case.what
    );
    if got.len() < WEIGHED {
        return;
    }
    let centroid = |pixels: &[(u32, u32, [u8; 3])]| {
        let (mut sx, mut sy, mut total) = (0.0, 0.0, 0.0);
        for &(x, y, [_, g, _]) in pixels {
            let weight = f64::from(255 - g);
            sx += weight * (f64::from(x) + 0.5);
            sy += weight * (f64::from(y) + 0.5);
            total += weight;
        }
        (total > 0.0).then(|| (sx / total, sy / total))
    };
    if let (Some(at), Some(there)) = (centroid(&got), centroid(&want)) {
        assert!(
            (at.0 - there.0).abs() <= 0.5 && (at.1 - there.1).abs() <= 0.5,
            "{}: the picture's weight lands at ({:.3}, {:.3}), the full decode's at \
             ({:.3}, {:.3})",
            case.what,
            at.0,
            at.1,
            there.0,
            there.1
        );
    }
    let mut apart = 0u64;
    for (&(x, y, a), &(wx, wy, b)) in got.iter().zip(&want) {
        assert_eq!((x, y), (wx, wy), "{}: the same pixels", case.what);
        apart += a
            .iter()
            .zip(&b)
            .map(|(&p, &q)| u64::from(p.abs_diff(q)))
            .sum::<u64>();
    }
    let pixels = got.len() as u64;
    assert!(
        apart <= 3 * 12 * pixels,
        "{}: {apart} levels apart over {pixels} pixels",
        case.what
    );
}

// ---- the guards ----------------------------------------------------------------------

/// **Every reduced answer lands where the full decode does.** Each case
/// drawn from its reduced decode and from its full one, over white and over
/// black: the decodes are the ones asked for — the reduced one the case's
/// answer, read from the render's counted work, and the full one whole — and
/// over every pixel and channel the two leave the same part of the page
/// uncovered ([`uncovered`]): the image reaches the same pixels and covers the
/// same ones wholly, and each pixel of its unit square's edge is covered alike
/// but for the composite's rounding, the reduced grid cut where the image ends
/// rather than stretched to the square's edge, flipped, turned and sheared as
/// the full image is. The samples land where the full decode's do ([`lands`]). A clip
/// that leaves a codestream's decode a window decodes fewer of its
/// code-blocks than the same draw unclipped, and still covers what the full
/// decode covers.
#[test]
fn every_reduced_answer_lands_where_the_full_decode_does() {
    for case in CASES {
        let picture = picture(case.source);
        let reduced = rendered(&case, &picture, &picture.keys);
        assert!(
            reduced.white.warnings.is_empty(),
            "{}: {:?}",
            case.what,
            reduced.white.warnings
        );
        let whole_keys = format!("{} {}", picture.keys, picture.whole);
        let whole = rendered(&case, &picture, &whole_keys);
        assert_eq!(
            chosen(&picture, &reduced.stats),
            case.answer,
            "{}: the reduced decode's answer",
            case.what
        );
        let full = match picture.codec {
            Codec::Jpeg => 8,
            Codec::Jpx { .. } => 0,
        };
        assert_eq!(
            chosen(&picture, &whole.stats),
            full,
            "{}: the mask keeps the image whole",
            case.what
        );

        let (got, want) = (uncovered(&reduced), uncovered(&whole));
        assert_eq!(got.len(), want.len(), "{}: the same page", case.what);
        let inked = |g: i16, w: i16| (g == 255) == (w == 255);
        let whole_alike = |g: i16, w: i16| (g == 0) == (w == 0);
        let apart = got
            .iter()
            .zip(&want)
            .filter(|&(&g, &w)| !inked(g, w) || !whole_alike(g, w))
            .count();
        assert_eq!(
            apart, 0,
            "{}: {apart} channels of the page are reached or covered wholly otherwise than \
             under the full decode",
            case.what
        );
        let worst = got
            .iter()
            .zip(&want)
            .map(|(&g, &w)| (g - w).abs())
            .max()
            .unwrap_or(0);
        assert!(
            worst <= 1,
            "{}: an edge pixel covered {worst} levels otherwise than under the full decode",
            case.what
        );
        let bounds = ink_bounds(&reduced);
        assert!(bounds.is_some(), "{}: the draw inks the page", case.what);
        assert_eq!(
            bounds,
            ink_bounds(&whole),
            "{}: the ink's bounds",
            case.what
        );

        lands(&case, &reduced, &whole);

        if case.window {
            let open = Case { clip: None, ..case };
            let unclipped = rendered(&open, &picture, &picture.keys);
            assert_eq!(
                chosen(&picture, &unclipped.stats),
                case.answer,
                "{}: the clip leaves the levels as they were",
                case.what
            );
            assert!(
                reduced.stats.jpx_code_blocks_decoded < unclipped.stats.jpx_code_blocks_decoded,
                "{}: the window decoded {} code-blocks, the whole draw {}",
                case.what,
                reduced.stats.jpx_code_blocks_decoded,
                unclipped.stats.jpx_code_blocks_decoded
            );
        }
    }
}

/// **A reduced answer is never drawn magnified.** Each case is drawn at a size
/// whose rule picks its answer — a JPEG's smallest scale that covers the draw
/// both ways, a codestream's most levels that do — and its render chose that
/// answer, read from the counted work: the decode's samples along each of the
/// image's axes, from T.81's and T.800's size rules, are at least the device
/// pixels the draw spans along it, flipped, turned, sheared and at twice the
/// scale alike. Guard A7 holds beside it: a JPEG drawn at its answer's own
/// fraction transforms at most four samples a drawn pixel, and a codestream
/// synthesises exactly the levels its decoder synthesises for that discard,
/// no more; each image is decoded once.
#[test]
fn a_reduced_answer_is_never_magnified() {
    let limits = Limits::new(CEILING);
    for case in CASES {
        let picture = picture(case.source);
        let size = (picture.width, picture.height);
        let spans = drawn(&case);
        let rule = match picture.codec {
            Codec::Jpeg => jpeg_rule(size, spans),
            Codec::Jpx { levels } => jpx_rule(size, levels, spans),
        };
        assert_eq!(
            rule, case.answer,
            "{}: the case is drawn at the size that picks its answer",
            case.what
        );

        let Pair { white, stats, .. } = rendered(&case, &picture, &picture.keys);
        assert!(
            white.warnings.is_empty(),
            "{}: {:?}",
            case.what,
            white.warnings
        );
        assert_eq!(stats.images_decoded, 1, "{}: one decode", case.what);
        let answer = chosen(&picture, &stats);
        assert_eq!(
            answer, rule,
            "{}: the decode chose the rule's answer",
            case.what
        );
        let (across, down) = samples_at(&picture, answer);
        assert!(
            f64::from(across) >= spans.0 && f64::from(down) >= spans.1,
            "{}: {across} by {down} samples drawn over {:.3} by {:.3} pixels",
            case.what,
            spans.0,
            spans.1
        );

        match picture.codec {
            Codec::Jpeg => {
                if case.exact {
                    let pixels = spans.0 * spans.1;
                    assert!(
                        stats.jpeg_samples_transformed as f64 <= 4.0 * pixels,
                        "A7, {}: {} samples transformed for {pixels} drawn pixels",
                        case.what,
                        stats.jpeg_samples_transformed
                    );
                }
            }
            Codec::Jpx { .. } => {
                let (_, counts) = jpx_decode_request(
                    &picture.data,
                    &limits,
                    &JpxRequest {
                        discard: u8::try_from(answer).expect("a level count"),
                        window: None,
                    },
                    &mut Vec::new(),
                )
                .expect("the codestream decodes");
                assert_eq!(
                    stats.jpx_levels_synthesised, counts.levels_synthesised,
                    "A7, {}: the levels needed and no more",
                    case.what
                );
            }
        }
    }
}

/// The page of the answers drawn at 72 dpi: each case of the smaller images
/// drawn where a row of them leaves room, every image once in the page's
/// resources.
fn answers_page() -> Vec<u8> {
    let mut sources: Vec<Source> = Vec::new();
    let mut content = String::new();
    let (mut x, mut y, mut row, mut width) = (2.0f64, 2.0f64, 0.0f64, 0.0f64);
    // The draws at 72 dpi of the smaller images: the larger codestreams'
    // tile by tile are `jpx_images.rs`'s and `render_regions.rs`'s.
    for case in CASES
        .iter()
        .filter(|case| case.scale == 1.0 && !matches!(case.source, Source::Irreversible))
    {
        let (w, h) = page_size(case);
        if w > 120.0 || h > 120.0 {
            continue;
        }
        if x + w > 330.0 {
            x = 2.0;
            y += row;
            row = 0.0;
        }
        content.push_str(&format!(
            "q 1 0 0 1 {x} {y} cm {} Q\n",
            draw(case, case.source.name())
        ));
        if !sources.contains(&case.source) {
            sources.push(case.source);
        }
        x += w;
        row = row.max(h);
        width = width.max(x);
    }
    let pictures: Vec<(Source, Picture)> = sources
        .iter()
        .map(|&source| (source, picture(source)))
        .collect();
    let images: Vec<(&str, &str, &[u8])> = pictures
        .iter()
        .map(|(source, picture)| (source.name(), picture.keys.as_str(), &picture.data[..]))
        .collect();
    page((width + 3.0, y + row + 3.0), &content, &images)
}

/// **Every tile of the answers is the page under it** (ruling 5). The page of
/// the answers — both JPEGs at each scale and transform, both small
/// codestreams with each discard, the 310 by 438 one with two levels dropped,
/// whole and clipped — at 72 dpi and at one and a half times it, tiled by
/// lattices of 37 and 23 pixels: each tile asks for the page frame's size, so
/// it chooses the page's answers, decodes the window its own canvas reaches
/// and is the page's pixels byte for byte.
#[test]
fn every_tile_of_the_answers_is_the_page_under_it() {
    let document = Document::open(answers_page()).expect("it opens");
    let page = document.page(0).expect("a page");
    for (scale, tile) in [(1.0, 37u32), (1.0, 23), (1.5, 37)] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        let whole = page.render(&options);
        assert!(whole.warnings.is_empty(), "{:?}", whole.warnings);
        assert_eq!(whole.format, PixelFormat::Rgb8);
        let (width, height) = (whole.width, whole.height);
        for top in (0..height).step_by(tile as usize) {
            for left in (0..width).step_by(tile as usize) {
                let asked = PixelRegion::new(left, top, tile, tile);
                let part = page.render(&RenderOptions {
                    region: Some(asked),
                    ..options.clone()
                });
                let expected = asked.clamped_to(width, height);
                assert_eq!(
                    (part.width, part.height),
                    (expected.width, expected.height),
                    "the {tile}px tile at ({left}, {top}) at {scale}x"
                );
                let bytes = part.width as usize * 3;
                for row in 0..part.height as usize {
                    let from = (expected.y as usize + row) * whole.stride + expected.x as usize * 3;
                    assert!(
                        part.data[row * part.stride..][..bytes] == whole.data[from..from + bytes],
                        "the {tile}px tile at ({left}, {top}) at {scale}x: row {row} is not the \
                         page's"
                    );
                }
            }
        }
    }
}
