//! Default colour spaces (ISO 32000-1 8.6.5.6, roadmap K-02.2): a scope's
//! `/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK` stand in for the device
//! space each is named for, on every path that selects one
//! (`docs/design/icc.md`, "Default colour spaces").
//!
//! Every case is a twin: a page painting in a device space under a default,
//! and the same page naming the default's space directly, compared byte for
//! byte. Beside each, a page it must **not** equal — the first without its
//! default, as a rule — because two twins that agree because neither applied
//! anything prove nothing. A JPEG's and a JPEG 2000 image's `/ColorSpace` is
//! read as a sampled image's is, so both are paths here like any other.

use tinker_pdf::{Bitmap, Document, ImageSpace, PixelFormat, RenderOptions, RenderWarning};
use tinker_pdf_filters::{jpeg_encode, JpegOptions, JpegSource, JpegSourceColour};

// ---- Profiles -------------------------------------------------------------

/// `value` as ICC.1's s15Fixed16Number.
fn s15(value: f64) -> [u8; 4] {
    (((value * 65536.0).round() as i32) as u32).to_be_bytes()
}

/// An `XYZType` tag.
fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut out = b"XYZ \0\0\0\0".to_vec();
    for value in [x, y, z] {
        out.extend_from_slice(&s15(value));
    }
    out
}

/// A `curv` tag with no points, which ICC.1 defines as the identity: the
/// profile's light is linear in its components, which no device space is.
fn linear() -> Vec<u8> {
    b"curv\0\0\0\0\0\0\0\0".to_vec()
}

/// A version 2.4 profile of the data space, connection space and tags given.
fn profile(space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut offsets = Vec::new();
    let mut at = 128 + 4 + tags.len() * 12;
    for (_, data) in tags {
        offsets.push(at);
        at = (at + data.len()).div_ceil(4) * 4;
    }
    let mut out = vec![0u8; 128];
    out[0..4].copy_from_slice(&(at as u32).to_be_bytes());
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
    out.resize(at, 0);
    out
}

/// sRGB's primaries adapted to D50, as ICC.1 writes a matrix profile's
/// columns.
const PRIMARIES: [[f64; 3]; 3] = [
    [0.436_065, 0.222_488, 0.013_916],
    [0.385_147, 0.716_873, 0.097_076],
    [0.143_066, 0.060_607, 0.714_096],
];

/// A grey profile with a linear tone curve: object 6.
fn grey_profile() -> Vec<u8> {
    profile(
        b"GRAY",
        &[(*b"kTRC", linear()), (*b"wtpt", xyz(0.9642, 1.0, 0.8249))],
    )
}

/// A matrix/TRC RGB profile, sRGB's primaries under linear curves: object 7.
fn rgb_profile() -> Vec<u8> {
    let [r, g, b] = PRIMARIES;
    profile(
        b"RGB ",
        &[
            (*b"rXYZ", xyz(r[0], r[1], r[2])),
            (*b"gXYZ", xyz(g[0], g[1], g[2])),
            (*b"bXYZ", xyz(b[0], b[1], b[2])),
            (*b"rTRC", linear()),
            (*b"gTRC", linear()),
            (*b"bTRC", linear()),
            (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
        ],
    )
}

/// A CMYK printer profile, object 8: one `mft2` table from four inks to XYZ
/// on a grid of two points an axis, each corner the light of its inks
/// squeezed into a tenth to nine tenths, so no colour is the one 8.6.4.4's
/// DeviceCMYK conversion gives it.
fn cmyk_profile() -> Vec<u8> {
    let mut tag = b"mft2\0\0\0\0".to_vec();
    tag.extend_from_slice(&[4, 3, 2, 0]);
    for row in 0..3 {
        for column in 0..3 {
            tag.extend_from_slice(&s15(if row == column { 1.0 } else { 0.0 }));
        }
    }
    tag.extend_from_slice(&2u16.to_be_bytes());
    tag.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..4 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    // The last axis, black, varies fastest.
    for corner in 0..16u32 {
        let ink = |bit: u32| f64::from((corner >> bit) & 1);
        let (c, m, y, k) = (ink(3), ink(2), ink(1), ink(0));
        let light = [
            0.1 + 0.8 * (1.0 - c) * (1.0 - k),
            0.1 + 0.8 * (1.0 - m) * (1.0 - k),
            0.1 + 0.8 * (1.0 - y) * (1.0 - k) * 0.7,
        ];
        let xyz: [f64; 3] = std::array::from_fn(|axis| {
            PRIMARIES
                .iter()
                .zip(light)
                .map(|(primary, amount)| primary[axis] * amount)
                .sum()
        });
        for value in xyz {
            // 1.0 is 0x8000 in the connection space's sixteen-bit encoding.
            tag.extend_from_slice(&((value * 32768.0).round() as u16).to_be_bytes());
        }
    }
    for _ in 0..3 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    profile(b"CMYK", &[(*b"A2B0", tag)])
}

// ---- Documents ------------------------------------------------------------

/// `<< dict /Length n >> stream … endstream`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn push_object(out: &mut Vec<u8>, number: u32, body: &[u8]) {
    out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendobj\n");
}

/// A 40 by 40 page before any default is put in: the entries of its
/// `/ColorSpace`, the rest of its `/Resources`, its content, keys of the page
/// dictionary itself, and objects from 10 on. Objects 6, 7 and 8 are always
/// the three profiles above.
#[derive(Clone, Default)]
struct Page {
    spaces: String,
    resources: String,
    content: String,
    keys: String,
    objects: Vec<(u32, Vec<u8>)>,
}

impl Page {
    fn new(content: impl Into<String>) -> Page {
        Page {
            content: content.into(),
            ..Page::default()
        }
    }

    fn spaces(mut self, spaces: impl AsRef<str>) -> Page {
        self.spaces.push(' ');
        self.spaces.push_str(spaces.as_ref());
        self
    }

    fn resources(mut self, resources: impl AsRef<str>) -> Page {
        self.resources.push(' ');
        self.resources.push_str(resources.as_ref());
        self
    }

    fn keys(mut self, keys: impl AsRef<str>) -> Page {
        self.keys.push(' ');
        self.keys.push_str(keys.as_ref());
        self
    }

    fn object(mut self, number: u32, body: Vec<u8>) -> Page {
        self.objects.push((number, body));
        self
    }

    /// The page as a document, its `/ColorSpace` carrying `defaults` — whole
    /// entries, `/DefaultRGB […]` — beside its own.
    fn document(&self, defaults: &str) -> Document {
        let mut out = b"%PDF-1.7\n".to_vec();
        push_object(&mut out, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
        push_object(&mut out, 2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>");
        let page = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Resources << \
             /ColorSpace << {defaults} {} >> {} >> /Contents 4 0 R {} >>",
            self.spaces, self.resources, self.keys
        );
        push_object(&mut out, 3, page.as_bytes());
        push_object(&mut out, 4, &stream("", self.content.as_bytes()));
        push_object(&mut out, 6, &stream("/N 1", &grey_profile()));
        push_object(&mut out, 7, &stream("/N 3", &rgb_profile()));
        push_object(&mut out, 8, &stream("/N 4", &cmyk_profile()));
        for (number, body) in &self.objects {
            push_object(&mut out, *number, body);
        }
        out.extend_from_slice(b"trailer\n<< /Size 32 /Root 1 0 R >>\n%%EOF\n");
        Document::open(out).expect("it opens")
    }
}

fn render(document: &Document, options: &RenderOptions) -> Bitmap {
    document.page(0).expect("a page").render(options)
}

/// A form XObject, object 12 wherever it is used.
fn form(keys: &str, content: &str) -> Vec<u8> {
    stream(
        &format!("/Type /XObject /Subtype /Form /BBox [0 0 40 40] {keys}"),
        content.as_bytes(),
    )
}

// ---- Families and substitutes ---------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Gray,
    Rgb,
    Cmyk,
}

impl Family {
    fn device(self) -> &'static str {
        match self {
            Family::Gray => "DeviceGray",
            Family::Rgb => "DeviceRGB",
            Family::Cmyk => "DeviceCMYK",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Family::Gray => "DefaultGray",
            Family::Rgb => "DefaultRGB",
            Family::Cmyk => "DefaultCMYK",
        }
    }

    /// Table 93's abbreviation, which `cs` reads as the device space where
    /// no resource has the name.
    fn abbreviation(self) -> &'static str {
        match self {
            Family::Gray => "G",
            Family::Rgb => "RGB",
            Family::Cmyk => "CMYK",
        }
    }

    fn fill(self) -> &'static str {
        match self {
            Family::Gray => "g",
            Family::Rgb => "rg",
            Family::Cmyk => "k",
        }
    }

    fn stroke(self) -> &'static str {
        match self {
            Family::Gray => "G",
            Family::Rgb => "RG",
            Family::Cmyk => "K",
        }
    }

    /// A colour in the family's components, away from the ends of every
    /// range so no substitute agrees with the device space on it.
    fn colour(self) -> &'static str {
        match self {
            Family::Gray => "0.3",
            Family::Rgb => "0.2 0.5 0.8",
            Family::Cmyk => "0.1 0.6 0.3 0.2",
        }
    }

    fn second(self) -> &'static str {
        match self {
            Family::Gray => "0.6",
            Family::Rgb => "0.7 0.3 0.1",
            Family::Cmyk => "0.5 0.1 0.2 0.3",
        }
    }

    fn zeros(self) -> &'static str {
        match self {
            Family::Gray => "0",
            Family::Rgb => "0 0 0",
            Family::Cmyk => "0 0 0 0",
        }
    }

    /// Four pixels of eight-bit samples, in hexadecimal.
    fn samples(self) -> &'static str {
        match self {
            Family::Gray => "2060A0E0",
            Family::Rgb => "3366991188DDCC4422FF8800",
            Family::Cmyk => "1A994D338019335033664D1A4D334D66",
        }
    }

    /// Two palette entries.
    fn palette(self) -> &'static str {
        match self {
            Family::Gray => "40C0",
            Family::Rgb => "4080C0C08040",
            Family::Cmyk => "1A994D3380193350",
        }
    }
}

/// A space a default may be, and the page text naming it.
struct Substitute {
    family: Family,
    label: &'static str,
    space: &'static str,
}

const SUBSTITUTES: [Substitute; 5] = [
    Substitute {
        family: Family::Gray,
        label: "CalGray",
        space: "[/CalGray << /WhitePoint [0.9505 1 1.089] /Gamma 2.2 >>]",
    },
    Substitute {
        family: Family::Gray,
        label: "ICCBased N=1",
        space: "[/ICCBased 6 0 R]",
    },
    Substitute {
        family: Family::Rgb,
        label: "CalRGB",
        space: "[/CalRGB << /WhitePoint [0.9505 1 1.089] /Gamma [1.8 1.8 1.8] \
                /Matrix [0.4124 0.2126 0.0193 0.3576 0.7152 0.1192 0.1805 0.0722 0.9505] >>]",
    },
    Substitute {
        family: Family::Rgb,
        label: "ICCBased N=3",
        space: "[/ICCBased 7 0 R]",
    },
    Substitute {
        family: Family::Cmyk,
        label: "ICCBased N=4",
        space: "[/ICCBased 8 0 R]",
    },
];

impl Substitute {
    /// The default entry that puts this space in for its family.
    fn default(&self) -> String {
        format!("/{} {}", self.family.key(), self.space)
    }
}

// ---- The painting paths ---------------------------------------------------
//
// Each path draws in its family's device space when given no space, and
// names the space it is given directly otherwise — the twin.

type Path = fn(Family, Option<&str>) -> Option<Page>;

/// `rest` painted after `/CS0 cs`, with `/CS0` the space given.
fn named(space: &str, rest: &str) -> Page {
    Page::new(format!("/CS0 cs {rest}")).spaces(format!("/CS0 {space}"))
}

fn operators(family: Family, twin: Option<&str>) -> Option<Page> {
    let (c, s) = (family.colour(), family.second());
    Some(match twin {
        None => Page::new(format!(
            "{c} {} 0 0 20 40 re f 6 w {s} {} 30 0 m 30 40 l S",
            family.fill(),
            family.stroke()
        )),
        Some(space) => Page::new(format!(
            "/CS0 cs {c} sc 0 0 20 40 re f 6 w /CS0 CS {s} SC 30 0 m 30 40 l S"
        ))
        .spaces(format!("/CS0 {space}")),
    })
}

fn cs_device(family: Family, twin: Option<&str>) -> Option<Page> {
    let (c, s) = (family.colour(), family.second());
    let paint = |fill: &str, stroke: &str| {
        format!("{fill} cs {c} sc 0 0 20 40 re f 6 w {stroke} CS {s} SC 30 0 m 30 40 l S")
    };
    let device = format!("/{}", family.device());
    Some(match twin {
        None => Page::new(paint(&device, &device)),
        Some(space) => Page::new(paint("/CS0", "/CS0")).spaces(format!("/CS0 {space}")),
    })
}

fn cs_entry(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    Some(
        Page::new(format!("/CS1 cs {} sc 0 0 40 40 re f", family.colour()))
            .spaces(format!("/CS1 {space}")),
    )
}

fn cs_abbreviation(family: Family, twin: Option<&str>) -> Option<Page> {
    let rest = format!("{} sc 0 0 40 40 re f", family.colour());
    Some(match twin {
        None => Page::new(format!("/{} cs {rest}", family.abbreviation())),
        Some(space) => named(space, &rest),
    })
}

fn cs_bare_cal(family: Family, twin: Option<&str>) -> Option<Page> {
    let bare = match family {
        Family::Gray => "CalGray",
        Family::Rgb => "CalRGB",
        Family::Cmyk => return None,
    };
    let rest = format!("{} sc 0 0 40 40 re f", family.colour());
    Some(match twin {
        None => Page::new(format!("/{bare} cs {rest}")),
        Some(space) => named(space, &rest),
    })
}

fn sc_by_count(family: Family, twin: Option<&str>) -> Option<Page> {
    let (c, s) = (family.colour(), family.second());
    Some(match twin {
        None => Page::new(format!("{c} sc 0 0 20 40 re f 6 w {s} SC 30 0 m 30 40 l S")),
        Some(space) => Page::new(format!(
            "/CS0 cs {c} sc 0 0 20 40 re f 6 w /CS0 CS {s} SC 30 0 m 30 40 l S"
        ))
        .spaces(format!("/CS0 {space}")),
    })
}

fn sampled_image(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 4 /Height 1 /ColorSpace {space} \
             /BitsPerComponent 8 /Filter /ASCIIHexDecode"
        ),
        format!("{}>", family.samples()).as_bytes(),
    );
    Some(
        Page::new("q 40 0 0 40 0 0 cm /Im0 Do Q")
            .resources("/XObject << /Im0 10 0 R >>")
            .object(10, image),
    )
}

fn indexed_image(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 4 /Height 1 \
             /ColorSpace [/Indexed {space} 1 <{}>] /BitsPerComponent 8 /Filter /ASCIIHexDecode",
            family.palette()
        ),
        b"00010001>",
    );
    Some(
        Page::new("q 40 0 0 40 0 0 cm /Im0 Do Q")
            .resources("/XObject << /Im0 10 0 R >>")
            .object(10, image),
    )
}

fn inline_image(family: Family, twin: Option<&str>) -> Option<Page> {
    let image = |space: &str| {
        format!(
            "q 40 0 0 40 0 0 cm BI /W 4 /H 1 /CS {space} /BPC 8 /F /AHx ID {}> EI Q",
            family.samples()
        )
    };
    Some(match twin {
        None => Page::new(image(&format!("/{}", family.abbreviation()))),
        // 8.9.7: an inline image names any other space by its resource.
        Some(space) => Page::new(image("/CS0")).spaces(format!("/CS0 {space}")),
    })
}

/// The family's four pixels, as bytes.
fn sample_bytes(family: Family) -> Vec<u8> {
    family
        .samples()
        .as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII"), 16).expect("hex"))
        .collect()
}

/// The family's four pixels as a JPEG, which holds grey and RGB; the encoder
/// writes no CMYK.
fn jpeg(family: Family) -> Option<Vec<u8>> {
    let colour = match family {
        Family::Gray => JpegSourceColour::Gray,
        Family::Rgb => JpegSourceColour::Rgb,
        Family::Cmyk => return None,
    };
    let data = sample_bytes(family);
    let source = JpegSource {
        width: 4,
        height: 1,
        colour,
        stride: data.len(),
        data: &data,
    };
    Some(jpeg_encode(&source, &JpegOptions::default()).expect("four pixels encode"))
}

fn jpeg_image(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 4 /Height 1 /ColorSpace {space} \
             /BitsPerComponent 8 /Filter /DCTDecode"
        ),
        &jpeg(family)?,
    );
    Some(
        Page::new("q 40 0 0 40 0 0 cm /Im0 Do Q")
            .resources("/XObject << /Im0 10 0 R >>")
            .object(10, image),
    )
}

fn inline_jpeg(family: Family, twin: Option<&str>) -> Option<Page> {
    let hex: String = jpeg(family)?
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    let image = |space: &str| {
        format!("q 40 0 0 40 0 0 cm BI /W 4 /H 1 /CS {space} /BPC 8 /F [/AHx /DCT] ID {hex}> EI Q")
    };
    Some(match twin {
        None => Page::new(image(&format!("/{}", family.abbreviation()))),
        Some(space) => Page::new(image("/CS0")).spaces(format!("/CS0 {space}")),
    })
}

/// A codestream from the JPEG 2000 decoder's own fixtures: its grey ramp, or
/// its colour image; it has none in CMYK.
fn jpx_image(family: Family, twin: Option<&str>) -> Option<Page> {
    let fixture = match family {
        Family::Gray => "r1-2.jp2",
        Family::Rgb => "c1-2.jp2",
        Family::Cmyk => return None,
    };
    let codestream = std::fs::read(
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tinker-pdf-filters/tests/jpx"
        ))
        .join(fixture),
    )
    .expect("the decoder's fixture");
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 32 /Height 24 /ColorSpace {space} \
             /Filter /JPXDecode"
        ),
        &codestream,
    );
    Some(
        Page::new("q 40 0 0 40 0 0 cm /Im0 Do Q")
            .resources("/XObject << /Im0 10 0 R >>")
            .object(10, image),
    )
}

/// An axial shading in `space`, from the family's colour to its second.
fn axial(family: Family, space: &str) -> String {
    format!(
        "<< /ShadingType 2 /ColorSpace {space} /Coords [0 0 40 0] /Function \
         << /FunctionType 2 /Domain [0 1] /C0 [{}] /C1 [{}] /N 1 >> /Extend [true true] >>",
        family.colour(),
        family.second()
    )
}

fn shading(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    Some(Page::new("/Sh0 sh").resources(format!("/Shading << /Sh0 {} >>", axial(family, &space))))
}

fn shading_pattern(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    Some(
        Page::new("/Pattern cs /P0 scn 0 0 40 40 re f").resources(format!(
            "/Pattern << /P0 << /PatternType 2 /Shading {} >> >>",
            axial(family, &space)
        )),
    )
}

fn indexed_base(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    Some(
        Page::new("/CS1 cs 0 sc 0 0 20 40 re f /CS1 cs 1 sc 20 0 20 40 re f")
            .spaces(format!("/CS1 [/Indexed {space} 1 <{}>]", family.palette())),
    )
}

/// An uncoloured tiling pattern, whose cell is a shape and takes the colour
/// its `scn` operands give (8.7.3.3).
fn uncoloured_cell() -> Vec<u8> {
    stream(
        "/PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8 \
         /Resources << >>",
        b"0 0 4 8 re f",
    )
}

fn pattern_base(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    Some(
        Page::new(format!(
            "/CS1 cs {} /P0 scn 0 0 40 40 re f",
            family.colour()
        ))
        .spaces(format!("/CS1 [/Pattern {space}]"))
        .resources("/Pattern << /P0 11 0 R >>")
        .object(11, uncoloured_cell()),
    )
}

/// One colorant's tint transform into the family: none of it is the
/// family's zeros, all of it the family's colour.
fn tint(family: Family) -> String {
    format!(
        "<< /FunctionType 2 /Domain [0 1] /C0 [{}] /C1 [{}] /N 1 >>",
        family.zeros(),
        family.colour()
    )
}

fn separation_alternate(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    Some(
        Page::new("/CS1 cs 0.7 sc 0 0 40 40 re f")
            .spaces(format!("/CS1 [/Separation /Spot {space} {}]", tint(family))),
    )
}

fn device_n_alternate(family: Family, twin: Option<&str>) -> Option<Page> {
    let space = twin.map_or_else(|| format!("/{}", family.device()), str::to_string);
    Some(
        Page::new("/CS1 cs 0.7 sc 0 0 40 40 re f")
            .spaces(format!("/CS1 [/DeviceN [/Spot] {space} {}]", tint(family))),
    )
}

const PATHS: [(&str, Path); 18] = [
    ("the operators", operators),
    ("cs naming the device space", cs_device),
    ("a /ColorSpace entry that is the device space", cs_entry),
    ("cs by Table 93's abbreviation", cs_abbreviation),
    ("cs by a bare CalGray or CalRGB name", cs_bare_cal),
    ("sc with no space named", sc_by_count),
    ("a sampled image", sampled_image),
    ("an /Indexed image's base", indexed_image),
    ("an inline image", inline_image),
    ("a JPEG image", jpeg_image),
    ("an inline JPEG image", inline_jpeg),
    ("a JPEG 2000 image", jpx_image),
    ("a shading under sh", shading),
    ("a shading pattern", shading_pattern),
    ("an /Indexed base", indexed_base),
    ("an uncoloured pattern's base", pattern_base),
    ("a selected /Separation alternate", separation_alternate),
    ("a /DeviceN alternate", device_n_alternate),
];

// ---- Cases ----------------------------------------------------------------

/// One comparison: a page, the page it must draw byte for byte, the pages it
/// must not, and the warning it must carry.
struct Case {
    name: String,
    drawn: Document,
    equals: Document,
    unlike: Vec<Document>,
    named: Option<RenderWarning>,
    options: RenderOptions,
}

impl Case {
    fn new(name: impl Into<String>, drawn: Document, equals: Document) -> Case {
        Case {
            name: name.into(),
            drawn,
            equals,
            unlike: Vec::new(),
            named: None,
            options: RenderOptions::default(),
        }
    }

    fn unlike(mut self, document: Document) -> Case {
        self.unlike.push(document);
        self
    }

    fn named(mut self, warning: RenderWarning) -> Case {
        self.named = Some(warning);
        self
    }

    fn with(mut self, options: RenderOptions) -> Case {
        self.options = options;
        self
    }

    /// What is wrong with the case, if anything.
    fn check(&self) -> Option<String> {
        let drawn = render(&self.drawn, &self.options);
        let equals = render(&self.equals, &self.options);
        if (drawn.width, drawn.height, &drawn.data) != (equals.width, equals.height, &equals.data) {
            return Some(format!(
                "{}: drew {} where its twin drew {}",
                self.name,
                first_pixel_that_differs(&drawn, &equals),
                first_pixel_that_differs(&equals, &drawn),
            ));
        }
        for (index, unlike) in self.unlike.iter().enumerate() {
            if render(unlike, &self.options).data == drawn.data {
                return Some(format!(
                    "{}: drew what page {index} it must differ from draws, so the twins \
                     agreeing says nothing",
                    self.name
                ));
            }
        }
        let repairs: Vec<&RenderWarning> = drawn
            .warnings
            .iter()
            .filter(|w| matches!(w, RenderWarning::RepairedColorSpace { .. }))
            .collect();
        let want: Vec<&RenderWarning> = self.named.iter().collect();
        (repairs != want).then(|| {
            format!(
                "{}: named {repairs:?} where it should name {want:?}",
                self.name
            )
        })
    }
}

/// The first pixel where `a` and `b` disagree, as `a` has it.
fn first_pixel_that_differs(a: &Bitmap, b: &Bitmap) -> String {
    let n = a.components();
    let at = a
        .data
        .chunks(n)
        .zip(b.data.chunks(n))
        .position(|(x, y)| x != y)
        .unwrap_or(0);
    let pixel = a.data.chunks(n).nth(at).unwrap_or_default();
    let width = a.width.max(1) as usize;
    format!("{pixel:?} at ({}, {})", at % width, at / width)
}

/// Runs every case and fails with all that fail, not the first.
fn check_all(cases: &[Case]) {
    let failures: Vec<String> = cases.iter().filter_map(Case::check).collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// Every painting path, under every substitute: the page under the default
/// draws as its twin and not as the device space.
fn path_cases(cases: &mut Vec<Case>) {
    for substitute in &SUBSTITUTES {
        for (path, build) in PATHS {
            let family = substitute.family;
            let (Some(device), Some(twin)) =
                (build(family, None), build(family, Some(substitute.space)))
            else {
                continue;
            };
            cases.push(
                Case::new(
                    format!("{} under /{} {}", path, family.key(), substitute.label),
                    device.document(&substitute.default()),
                    twin.document(""),
                )
                .unlike(device.document("")),
            );
        }
    }
}

/// 8.6.5.6's "current resource dictionary", scope by scope.
fn scope_cases(cases: &mut Vec<Case>) {
    for substitute in &SUBSTITUTES {
        let family = substitute.family;
        let (c, fill, key, space) = (
            family.colour(),
            family.fill(),
            family.key(),
            substitute.space,
        );
        let label = |what: &str| format!("{what}, /{key} {}", substitute.label);
        let device = format!("{c} {fill} 0 0 20 40 re f");
        let named_in_form = format!("/CS0 cs {c} sc 0 0 20 40 re f");
        let page = |form_body: Vec<u8>, content: &str| {
            Page::new(content)
                .resources("/XObject << /Fm0 12 0 R >>")
                .object(12, form_body)
        };
        let beside = format!("/Fm0 Do {c} {fill} 20 0 20 40 re f");

        // A form's own default applies inside it, and not to the page.
        let own = |spaces: &str, content: &str| {
            form(
                &format!("/Resources << /ColorSpace << {spaces} >> >>"),
                content,
            )
        };
        cases.push(
            Case::new(
                label("a default in a form applies inside it only"),
                page(own(&substitute.default(), &device), &beside).document(""),
                page(own(&format!("/CS0 {space}"), &named_in_form), &beside).document(""),
            )
            .unlike(page(own("", &device), &beside).document("")),
        );

        // A page's default stops at a form with `/Resources` of its own,
        // empty ones included (8.10.1).
        let own_empty = form("/Resources << >>", &device);
        cases.push(
            Case::new(
                label("a page's default does not reach a form with resources of its own"),
                page(own_empty.clone(), &beside).document(&substitute.default()),
                page(
                    own_empty,
                    &format!("/Fm0 Do /CS0 cs {c} sc 20 0 20 40 re f"),
                )
                .spaces(format!("/CS0 {space}"))
                .document(""),
            )
            .unlike(page(form("", &device), &beside).document(&substitute.default())),
        );

        // A form with none paints in the invoking scope, defaults and all.
        cases.push(
            Case::new(
                label("a form with no resources paints under the page's default"),
                page(form("", &device), &beside).document(&substitute.default()),
                page(
                    form("", &named_in_form),
                    &format!("/Fm0 Do /CS0 cs {c} sc 20 0 20 40 re f"),
                )
                .spaces(format!("/CS0 {space}"))
                .document(""),
            )
            .unlike(page(form("", &device), &beside).document("")),
        );

        // A coloured cell paints under its own `/Resources`' defaults.
        let cell = |spaces: &str, content: &str| {
            stream(
                &format!(
                    "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 8 8] /XStep 8 \
                     /YStep 8 /Resources << /ColorSpace << {spaces} >> >>"
                ),
                content.as_bytes(),
            )
        };
        let patterned = |body: Vec<u8>| {
            Page::new("/Pattern cs /P0 scn 0 0 40 40 re f")
                .resources("/Pattern << /P0 13 0 R >>")
                .object(13, body)
        };
        cases.push(
            Case::new(
                label("a coloured cell paints under its own resources' default"),
                patterned(cell(
                    &substitute.default(),
                    &format!("{c} {fill} 0 0 4 8 re f"),
                ))
                .document(""),
                patterned(cell(
                    &format!("/CS0 {space}"),
                    &format!("/CS0 cs {c} sc 0 0 4 8 re f"),
                ))
                .document(""),
            )
            .unlike(patterned(cell("", &format!("{c} {fill} 0 0 4 8 re f"))).document("")),
        );

        // An appearance stream is a form XObject (12.5.5) and paints under
        // its own `/Resources`' default, beside a page that names none.
        let annotated = |spaces: &str, content: &str| {
            Page::new(format!("{c} {fill} 20 0 20 40 re f"))
                .keys("/Annots [20 0 R]")
                .object(
                    20,
                    b"<< /Type /Annot /Subtype /Square /Rect [0 0 20 40] /AP << /N 21 0 R >> >>"
                        .to_vec(),
                )
                .object(
                    21,
                    stream(
                        &format!(
                            "/Type /XObject /Subtype /Form /BBox [0 0 20 40] \
                             /Resources << /ColorSpace << {spaces} >> >>"
                        ),
                        content.as_bytes(),
                    ),
                )
        };
        cases.push(
            Case::new(
                label("an appearance stream paints under its own resources' default"),
                annotated(&substitute.default(), &device).document(""),
                annotated(&format!("/CS0 {space}"), &named_in_form).document(""),
            )
            .unlike(annotated("", &device).document("")),
        );

        // 11.6.6: a group's device `/CS` remaps, read in the scope its
        // contents run in. A CMYK group is left out: a `k` colour keeps its
        // ink under `/DefaultCMYK` (H-135's default), and a CMYK group's
        // buffer composites that ink where its twin's ICC colour has none —
        // the ink test below holds what such a group draws.
        if family != Family::Cmyk {
            let group = |cs: &str, content: &str| {
                form(
                    &format!("/Group << /S /Transparency /CS {cs} /I true >>"),
                    content,
                )
            };
            cases.push(
                Case::new(
                    label("a group's /CS and its contents remap together"),
                    page(group(&format!("/{}", family.device()), &device), "/Fm0 Do")
                        .document(&substitute.default()),
                    page(group(space, &named_in_form), "/Fm0 Do")
                        .spaces(format!("/CS0 {space}"))
                        .document(""),
                )
                .unlike(
                    page(group(&format!("/{}", family.device()), &device), "/Fm0 Do").document(""),
                ),
            );
        }

        // 11.6.5.2: a luminosity mask's `/BC` is in its group's `/CS`, which
        // remaps — so the default moves the backdrop the mask is measured
        // against. The masked fill is painted in a family this default
        // leaves alone.
        let masked = if family == Family::Rgb {
            "0.2 g"
        } else {
            "0 0 1 rg"
        };
        let mask = |cs: &str| {
            Page::new(format!("/GS0 gs {masked} 0 0 40 40 re f"))
                .resources(format!(
                    "/ExtGState << /GS0 << /SMask << /S /Luminosity /G 14 0 R /BC [{c}] >> >> >>"
                ))
                .object(
                    14,
                    form(&format!("/Group << /S /Transparency /CS {cs} >>"), ""),
                )
        };
        cases.push(
            Case::new(
                label("a luminosity mask's /BC reads in its remapped group space"),
                mask(&format!("/{}", family.device())).document(&substitute.default()),
                mask(space).document(""),
            )
            .unlike(mask(&format!("/{}", family.device())).document("")),
        );
    }
}

/// `cs` resets to the device space's initial colour, carried into the
/// default: `/DeviceCMYK cs` and `0 0 0 1 k` paint one colour.
fn initial_colour_cases(cases: &mut Vec<Case>) {
    for substitute in &SUBSTITUTES {
        let family = substitute.family;
        let black = match family {
            Family::Gray => "0 g",
            Family::Rgb => "0 0 0 rg",
            Family::Cmyk => "0 0 0 1 k",
        };
        let selected = Page::new(format!("/{} cs 0 0 40 40 re f", family.device()));
        let mut case = Case::new(
            format!(
                "/{} cs alone paints what {black} paints, /{} {}",
                family.device(),
                family.key(),
                substitute.label
            ),
            selected.document(&substitute.default()),
            Page::new(format!("{black} 0 0 40 40 re f")).document(&substitute.default()),
        );
        // Where the default's own initial value is not the device space's —
        // an `ICCBased` CMYK's is no ink at all — the two readings part, and
        // the default is seen: these are the cases that can fail.
        if family == Family::Cmyk {
            case = case
                .unlike(selected.document(""))
                .unlike(Page::new("0 0 0 0 k 0 0 40 40 re f").document(&substitute.default()));
        }
        cases.push(case);
    }
}

/// A default 8.6.5.6 does not allow is refused: the device space is drawn,
/// and the refusal is named under the default's own name.
fn refusal_cases(cases: &mut Vec<Case>) {
    let refused = [
        (
            Family::Rgb,
            "[/Lab << /WhitePoint [0.9505 1 1.089] >>]",
            "DefaultSpaceFamily",
        ),
        (
            Family::Gray,
            "[/Indexed /DeviceRGB 1 <4080C0C08040>]",
            "DefaultSpaceFamily",
        ),
        (Family::Cmyk, "/Pattern", "DefaultSpaceFamily"),
        (
            Family::Rgb,
            "[/CalGray << /WhitePoint [0.9505 1 1.089] >>]",
            "DefaultSpaceComponents",
        ),
        (Family::Cmyk, "[/ICCBased 7 0 R]", "DefaultSpaceComponents"),
        (Family::Gray, "/Nonsense", "DefaultSpaceUnreadable"),
        (Family::Rgb, "42", "DefaultSpaceUnreadable"),
    ];
    for (family, value, reason) in refused {
        let page = everything_at_once(family);
        cases.push(
            Case::new(
                format!("/{} {value} is refused as {reason}", family.key()),
                page.document(&format!("/{} {value}", family.key())),
                page.document(""),
            )
            .named(RenderWarning::RepairedColorSpace {
                name: family.key().to_string(),
                reason: reason.to_string(),
            }),
        );
    }
    // The device space itself is a default 8.6.5.6 allows, and paints what
    // no default paints, with nothing to say.
    for (family, value) in [
        (Family::Rgb, "/DeviceRGB"),
        (Family::Gray, "/CalGray"),
        (Family::Cmyk, "/CMYK"),
    ] {
        let page = everything_at_once(family);
        cases.push(Case::new(
            format!("/{} {value} is the device space", family.key()),
            page.document(&format!("/{} {value}", family.key())),
            page.document(""),
        ));
    }
}

/// The operators, `cs` and an image in one family, side by side.
fn everything_at_once(family: Family) -> Page {
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 4 /Height 1 /ColorSpace /{} \
             /BitsPerComponent 8 /Filter /ASCIIHexDecode",
            family.device()
        ),
        format!("{}>", family.samples()).as_bytes(),
    );
    Page::new(format!(
        "{} {} 0 0 10 40 re f /{} cs {} sc 10 0 10 40 re f q 20 0 0 40 20 0 cm /Im0 Do Q",
        family.colour(),
        family.fill(),
        family.device(),
        family.second()
    ))
    .resources("/XObject << /Im0 10 0 R >>")
    .object(10, image)
}

/// What a default does not reach: the alternate of `/All` and `/None`,
/// which 8.6.6.4 is read here as never selecting.
fn unselected_alternate_cases(cases: &mut Vec<Case>) {
    for substitute in &SUBSTITUTES {
        let family = substitute.family;
        for colorant in ["All", "None"] {
            let page = Page::new("/CS1 cs 0.7 sc 0 0 40 40 re f").spaces(format!(
                "/CS1 [/Separation /{colorant} /{} {}]",
                family.device(),
                tint(family)
            ));
            cases.push(Case::new(
                format!(
                    "a /Separation /{colorant}'s alternate is not remapped by /{} {}",
                    family.key(),
                    substitute.label
                ),
                page.document(&substitute.default()),
                page.document(""),
            ));
        }
    }
}

/// **Each default draws as the space it substitutes, named directly** —
/// CalGray and ICCBased N=1 for `/DefaultGray`, CalRGB and ICCBased N=3 for
/// `/DefaultRGB`, ICCBased N=4 for `/DefaultCMYK` — through the operators,
/// `cs` and `CS` (by the device space's name, a `/ColorSpace` entry, Table
/// 93's abbreviation and a bare Cal name), `sc` and `SC` with no space named,
/// a sampled image and an `/Indexed` one, an inline image, a shading under
/// `sh` and in a pattern, an `/Indexed` base, an uncoloured pattern's base,
/// and a selected `/Separation` or `/DeviceN` alternate. Then the scopes: a
/// default in a form applies inside it only, a page's stops at a form with
/// resources of its own and reaches one without, and a coloured cell and an
/// appearance stream paint under their own; a group's `/CS` and a luminosity
/// mask's `/BC` remap as K-02.1 reads
/// 11.6.6; `/DeviceCMYK cs` alone paints what `0 0 0 1 k` paints; a Lab,
/// Indexed, Pattern, wrong-count or unreadable default is refused, drawn as
/// the device space and named; and `/All` and `/None` keep their alternate.
#[test]
fn each_default_space_draws_as_its_substitute_named_directly() {
    let mut cases = Vec::new();
    path_cases(&mut cases);
    scope_cases(&mut cases);
    initial_colour_cases(&mut cases);
    refusal_cases(&mut cases);
    unselected_alternate_cases(&mut cases);
    // 14 paths for CMYK and 18 for the rest, under five substitutes; seven
    // scope cases and six for CMYK; five initial colours, ten refusals and
    // guards, and two unselected alternates under each substitute.
    assert_eq!(cases.len(), 86 + 34 + 5 + 10 + 10, "the table lost a row");
    check_all(&cases);
}

/// **A refused default is named on every render of the scope**: a direct
/// render, each replay of a retained page, and a page written as SVG — once
/// each, under the default's own name (ruling 10).
#[test]
fn a_refused_default_is_named_on_every_render() {
    let document = everything_at_once(Family::Rgb)
        .document("/DefaultRGB [/Lab << /WhitePoint [0.9505 1 1.089] >>]");
    let want = RenderWarning::RepairedColorSpace {
        name: "DefaultRGB".to_string(),
        reason: "DefaultSpaceFamily".to_string(),
    };
    let count = |warnings: &[RenderWarning]| warnings.iter().filter(|w| **w == want).count();
    let page = document.page(0).expect("a page");

    let direct = page.render(&RenderOptions::default());
    assert_eq!(count(&direct.warnings), 1, "{:?}", direct.warnings);
    let list = page.display_list();
    assert!(list.is_retained());
    for replay in 0..2 {
        let bitmap = list.render(&RenderOptions::default());
        assert_eq!(
            count(&bitmap.warnings),
            1,
            "replay {replay}: {:?}",
            bitmap.warnings
        );
        assert_eq!(bitmap.data, direct.data, "replay {replay}");
    }
    let svg = page.to_svg(&tinker_pdf::SvgOptions::default());
    let rendered: Vec<RenderWarning> = svg
        .warnings
        .iter()
        .filter_map(|w| match w {
            tinker_pdf::SvgWarning::Render(w) => Some(w.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(count(&rendered), 1, "{:?}", svg.warnings);
}

// ---- The selections a default rides on -------------------------------------

/// **The selections a default rides on read what the file names**, with no
/// default anywhere: `sc` after `g`, `rg` or `k` (and `SC` after `G`, `RG`,
/// `K`) reads in that operator's family, where it read in the space the last
/// `cs` named (8.6.8); an inline image's `[/I …]` is `/Indexed` (Table 94),
/// beside the `/I` that is `/Interpolate` (Table 93) and whatever bytes a
/// literal-string palette holds; and an inline image naming a `/ColorSpace`
/// resource (8.9.7), or an `/Indexed` one over a resource, draws in that
/// space where it drew in DeviceGray.
#[test]
fn the_selections_a_default_rides_on_read_what_the_file_names() {
    let mut cases = Vec::new();
    // A space that reads any operand count as something else entirely.
    let indexed = "/CS1 [/Indexed /DeviceRGB 1 <FF0000 0000FF>]";
    for (fill, stroke, set, operands) in [
        ("g", "G", "0.5", "0.7"),
        ("rg", "RG", "0.1 0.1 0.1", "0.2 0.5 0.8"),
        ("k", "K", "0 0 0 0", "0.1 0.6 0.3 0.2"),
    ] {
        cases.push(
            Case::new(
                format!("sc after {fill}"),
                Page::new(format!(
                    "/CS1 cs 1 sc {set} {fill} {operands} sc 0 0 40 40 re f"
                ))
                .spaces(indexed)
                .document(""),
                Page::new(format!("{operands} {fill} 0 0 40 40 re f")).document(""),
            )
            .unlike(
                Page::new("/CS1 cs 1 sc 0 0 40 40 re f")
                    .spaces(indexed)
                    .document(""),
            ),
        );
        cases.push(
            Case::new(
                format!("SC after {stroke}"),
                Page::new(format!(
                    "8 w /CS1 CS 1 SC {set} {stroke} {operands} SC 20 0 m 20 40 l S"
                ))
                .spaces(indexed)
                .document(""),
                Page::new(format!("8 w {operands} {stroke} 20 0 m 20 40 l S")).document(""),
            )
            .unlike(
                Page::new("8 w /CS1 CS 1 SC 20 0 m 20 40 l S")
                    .spaces(indexed)
                    .document(""),
            ),
        );
    }

    let inline = |keys: &str| {
        Page::new(format!(
            "q 40 0 0 40 0 0 cm BI /W 2 /H 1 {keys} /BPC 8 /F /AHx ID 0001> EI Q"
        ))
    };
    // What the inline image drew while its space was read as DeviceGray.
    let gray = || inline("/CS /G").document("");
    let palette = "<4080C0C08040>";
    cases.push(
        Case::new(
            "an inline [/I …] is /Indexed",
            inline(&format!("/CS [/I /RGB 1 {palette}]")).document(""),
            inline(&format!("/CS [/Indexed /DeviceRGB 1 {palette}]")).document(""),
        )
        .unlike(gray()),
    );
    cases.push(
        Case::new(
            "an inline [/I …] beside the /I that is /Interpolate",
            inline(&format!("/I true /CS [/I /RGB 1 {palette}]")).document(""),
            inline(&format!(
                "/Interpolate true /CS [/Indexed /DeviceRGB 1 {palette}]"
            ))
            .document(""),
        )
        .unlike(inline(&format!("/CS [/Indexed /DeviceRGB 1 {palette}]")).document("")),
    );
    // A palette whose first entry is the bytes `/`, `I` and 0x80.
    cases.push(
        Case::new(
            "an inline palette written as a literal string is read as written",
            inline(r"/CS [/I /RGB 1 (/I\200 \240\140)]").document(""),
            inline("/CS [/Indexed /DeviceRGB 1 <2F498020A060>]").document(""),
        )
        .unlike(gray()),
    );
    cases.push(
        Case::new(
            "an inline image naming an /Indexed resource",
            inline("/CS /CS0")
                .spaces(format!("/CS0 [/Indexed /DeviceRGB 1 {palette}]"))
                .document(""),
            inline(&format!("/CS [/Indexed /DeviceRGB 1 {palette}]")).document(""),
        )
        .unlike(gray()),
    );

    // A space no inline image may spell out, so its twin is an XObject.
    let cal = "[/CalRGB << /WhitePoint [0.9505 1 1.089] /Gamma [1.8 1.8 1.8] >>]";
    let xobject = |space: &str, samples: &str| {
        Page::new("q 40 0 0 40 0 0 cm /Im0 Do Q")
            .resources("/XObject << /Im0 10 0 R >>")
            .object(
                10,
                stream(
                    &format!(
                        "/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace {space} \
                         /BitsPerComponent 8 /Filter /ASCIIHexDecode"
                    ),
                    format!("{samples}>").as_bytes(),
                ),
            )
            .document("")
    };
    cases.push(
        Case::new(
            "an inline image naming a CalRGB resource",
            Page::new(
                "q 40 0 0 40 0 0 cm BI /W 2 /H 1 /CS /CS0 /BPC 8 /F /AHx ID 336699CC4422> EI Q",
            )
            .spaces(format!("/CS0 {cal}"))
            .document(""),
            xobject(cal, "336699CC4422"),
        )
        .unlike(
            Page::new(
                "q 40 0 0 40 0 0 cm BI /W 2 /H 1 /CS /RGB /BPC 8 /F /AHx ID 336699CC4422> EI Q",
            )
            .document(""),
        ),
    );
    cases.push(
        Case::new(
            "an inline /Indexed over a CalRGB resource",
            inline(&format!("/CS [/I /CS0 1 {palette}]"))
                .spaces(format!("/CS0 {cal}"))
                .document(""),
            xobject(&format!("[/Indexed {cal} 1 {palette}]"), "0001"),
        )
        .unlike(gray()),
    );
    check_all(&cases);

    // Extraction reads the same dictionary through the same expansion.
    let listed = inline(&format!("/CS [/I /RGB 1 {palette}]"))
        .document("")
        .page(0)
        .expect("a page")
        .images();
    assert!(
        matches!(
            listed.first().and_then(|image| image.color_space.as_ref()),
            Some(ImageSpace::Indexed { .. })
        ),
        "{:?}",
        listed.first().map(|image| &image.color_space)
    );
}

// ---- Extraction and SVG ------------------------------------------------------

/// The painting paths that draw an image, each one image a page.
const IMAGE_PATHS: [(&str, Path); 6] = [
    ("a sampled image", sampled_image),
    ("an /Indexed image", indexed_image),
    ("an inline image", inline_image),
    ("a JPEG image", jpeg_image),
    ("an inline JPEG image", inline_jpeg),
    ("a JPEG 2000 image", jpx_image),
];

fn images(document: &Document) -> Vec<tinker_pdf::PageImage> {
    document.page(0).expect("a page").images()
}

/// **An extracted image names the space its samples are in** (K-02.3): an
/// image under each default keeps the space it states and reports the one
/// put in for it, its twin naming that space directly reports the same
/// `sample_space`, and the page without the default reports nothing put in;
/// one XObject drawn on a page under a default and in a form without one is
/// two entries, each with its own placements; and a refused default puts
/// nothing in and is named in the warnings of an image in its family alone.
#[test]
fn an_extracted_image_names_the_space_its_samples_are_in() {
    let mut wrong = Vec::new();
    let mut compared = 0;
    for substitute in &SUBSTITUTES {
        for (path, build) in IMAGE_PATHS {
            let family = substitute.family;
            let (Some(device), Some(twin)) =
                (build(family, None), build(family, Some(substitute.space)))
            else {
                continue;
            };
            let case = format!("{path} under /{} {}", family.key(), substitute.label);
            compared += 1;
            let under = images(&device.document(&substitute.default()));
            let without = images(&device.document(""));
            let direct = images(&twin.document(""));
            let ([under], [without], [direct]) = (&under[..], &without[..], &direct[..]) else {
                wrong.push(format!(
                    "{case}: {} {} {} entries, where each page draws one image",
                    under.len(),
                    without.len(),
                    direct.len()
                ));
                continue;
            };
            if under.color_space != without.color_space {
                wrong.push(format!(
                    "{case}: states {:?} where the page without the default states {:?}",
                    under.color_space, without.color_space
                ));
            }
            if under.substituted_space.is_none() {
                wrong.push(format!("{case}: reports nothing put in"));
            }
            if under.sample_space() != direct.sample_space() {
                wrong.push(format!(
                    "{case}: samples in {:?} where its twin's are in {:?}",
                    under.sample_space(),
                    direct.sample_space()
                ));
            }
            if without.substituted_space.is_some() || direct.substituted_space.is_some() {
                wrong.push(format!("{case}: a page with no default reports one put in"));
            }
            if under.samples != without.samples || !under.warnings.is_empty() {
                wrong.push(format!(
                    "{case}: other samples, or warnings {:?}",
                    under.warnings
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // Six image paths in grey and RGB under two substitutes each, and the
    // three with a CMYK image under one.
    assert_eq!(compared, 27, "image pages compared");

    // One XObject, drawn twice on a page under /DefaultRGB and once in a form
    // whose own resources name no default.
    let image = sampled_image(Family::Rgb, None)
        .expect("an RGB image")
        .objects
        .remove(0)
        .1;
    let page = Page::new("q 40 0 0 40 0 0 cm /Im0 Do Q q 20 0 0 20 0 0 cm /Im0 Do Q /Fm0 Do")
        .resources("/XObject << /Im0 10 0 R /Fm0 12 0 R >>")
        .object(10, image)
        .object(
            12,
            form(
                "/Resources << /XObject << /Im0 10 0 R >> >>",
                "q 10 0 0 10 0 0 cm /Im0 Do Q",
            ),
        );
    let rgb = &SUBSTITUTES[3];
    let listed = images(&page.document(&rgb.default()));
    assert_eq!(listed.len(), 2, "one entry for each space");
    assert_eq!(listed[0].placements.len(), 2, "the page's two drawings");
    assert!(listed[0].substituted_space.is_some());
    assert_eq!(listed[1].placements.len(), 1, "the form's drawing");
    assert_eq!(listed[1].substituted_space, None);
    assert_eq!(listed[1].sample_space(), Some(&ImageSpace::DeviceRgb));
    assert_eq!(listed[0].reference, listed[1].reference);
    let plain = images(&page.document(""));
    assert_eq!(plain.len(), 1, "with no default the XObject is one entry");
    assert_eq!(plain[0].placements.len(), 3);

    // A refused default puts nothing in, and an image in its family, or over
    // it, names the refusal; an image in another family does not.
    let lab = "/DefaultRGB [/Lab << /WhitePoint [0.9505 1 1.089] >>]";
    for (case, page, named) in [
        ("an RGB image", sampled_image(Family::Rgb, None), true),
        (
            "an /Indexed image over RGB",
            indexed_image(Family::Rgb, None),
            true,
        ),
        ("an inline RGB image", inline_image(Family::Rgb, None), true),
        ("a grey image", sampled_image(Family::Gray, None), false),
    ] {
        let listed = images(&page.expect("a page").document(lab));
        let [image] = &listed[..] else {
            panic!("{case}: {} entries", listed.len());
        };
        assert_eq!(image.substituted_space, None, "{case}");
        let want: &[&str] = if named { &["DefaultSpaceFamily"] } else { &[] };
        assert_eq!(image.warnings, want, "{case}");
    }
}

/// **A default-space page writes the SVG its twin writes** (K-02.3):
/// `to_svg` writes the interpreter's colour and decodes images and shadings
/// as the renderer does, so every painting path under each default writes
/// the markup its twin naming the substitute writes, and not the markup of
/// the page without the default.
#[test]
fn a_default_space_page_writes_the_svg_its_twin_writes() {
    let svg = |document: &Document| {
        document
            .page(0)
            .expect("a page")
            .to_svg(&tinker_pdf::SvgOptions::default())
            .markup
    };
    let mut wrong = Vec::new();
    let mut compared = 0;
    for substitute in &SUBSTITUTES {
        for (path, build) in PATHS {
            let family = substitute.family;
            let (Some(device), Some(twin)) =
                (build(family, None), build(family, Some(substitute.space)))
            else {
                continue;
            };
            let case = format!("{path} under /{} {}", family.key(), substitute.label);
            let under = svg(&device.document(&substitute.default()));
            if under != svg(&twin.document("")) {
                wrong.push(format!("{case}: not its twin's markup"));
            }
            if under == svg(&device.document("")) {
                wrong.push(format!(
                    "{case}: the markup of the page without the default"
                ));
            }
            compared += 1;
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(compared > 60, "{compared} pages compared");
}

// ---- An ink render ---------------------------------------------------------

/// The ink an ink render put at `(x, y)`.
fn ink_at(bitmap: &Bitmap, x: u32, y: u32) -> [u8; 4] {
    let at = y as usize * bitmap.stride + x as usize * bitmap.components();
    let mut ink = [0u8; 4];
    if let Some(pixel) = bitmap.data.get(at..at + 4) {
        ink.copy_from_slice(pixel);
    }
    ink
}

/// **An ink render keeps a DeviceCMYK colour's ink under `/DefaultCMYK`,
/// while the light render paints the default** — H-135's default until the
/// owner decides. `k`, `/DeviceCMYK cs … sc`, a `/ColorSpace` entry that is
/// `/DeviceCMYK`, four components with no space named, a stencil mask
/// painted in one and a `K` stroke each arrive on an ink page as the
/// document's own components, byte for byte the page without the default;
/// the light render of the same page is its twin's naming the profile
/// directly, and not 8.6.4.4's. Where an ink page needs light rather than
/// ink — inside a `/DeviceRGB` group — the light is the default's; and where
/// a light render composites in ink — inside a `/DeviceCMYK` group — the ink
/// is the document's, as it is on an ink page.
#[test]
fn an_ink_render_keeps_a_device_cmyk_colour_s_ink_under_default_cmyk() {
    let ink = RenderOptions {
        format: PixelFormat::CmykA8,
        allow_cmyk: true,
        ..RenderOptions::default()
    };
    let stencil = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ImageMask true /BitsPerComponent 1",
        &[0x00],
    );
    let bands = |paint: [&str; 6]| {
        Page::new(format!(
            "{} 0 0 5 40 re f {} 5 0 5 40 re f {} 10 0 5 40 re f {} 15 0 5 40 re f \
             {} q 5 0 0 40 20 0 cm /Im0 Do Q 4 w {} 27 0 m 27 40 l S",
            paint[0], paint[1], paint[2], paint[3], paint[4], paint[5]
        ))
        .spaces("/CS1 /DeviceCMYK")
        .resources("/XObject << /Im0 10 0 R >>")
        .object(10, stencil.clone())
    };
    let page = bands([
        "0.1 0.6 0.3 0.2 k",
        "/DeviceCMYK cs 0.5 0.1 0.2 0.3 sc",
        "/CS1 cs 0.3 0.3 0.6 0.1 sc",
        "0 0 0 0 k 0.2 0.1 0.7 0.2 sc",
        "0.2 0.3 0.4 0.1 k",
        "0.6 0.2 0.1 0.3 K",
    ]);
    let twin = bands([
        "/CS0 cs 0.1 0.6 0.3 0.2 sc",
        "/CS0 cs 0.5 0.1 0.2 0.3 sc",
        "/CS0 cs 0.3 0.3 0.6 0.1 sc",
        "/CS0 cs 0.2 0.1 0.7 0.2 sc",
        "/CS0 cs 0.2 0.3 0.4 0.1 sc",
        "/CS0 CS 0.6 0.2 0.1 0.3 SC",
    ])
    .spaces("/CS0 [/ICCBased 8 0 R]");
    let default = "/DefaultCMYK [/ICCBased 8 0 R]";

    let inked = render(&page.document(default), &ink);
    assert_eq!(inked.format, PixelFormat::CmykA8);
    for (x, want, what) in [
        (2, [26, 153, 77, 51], "k"),
        (7, [128, 26, 51, 77], "/DeviceCMYK cs … sc"),
        (
            12,
            [77, 77, 153, 26],
            "a /ColorSpace entry that is /DeviceCMYK",
        ),
        (17, [51, 26, 179, 51], "four components with no space named"),
        (22, [51, 77, 102, 26], "a stencil mask painted in one"),
        (27, [153, 51, 26, 77], "K"),
    ] {
        assert_eq!(ink_at(&inked, x, 20), want, "{what} lost its ink");
    }
    assert_eq!(
        inked.data,
        render(&page.document(""), &ink).data,
        "the ink is the document's own, as it is without the default"
    );

    // A page whose one form is a transparency group in `cs`, painting
    // `content`, beside the `/ColorSpace` entries `spaces`.
    let grouped = |cs: &str, spaces: &str, content: &str| {
        Page::new("/Fm0 Do")
            .spaces(spaces)
            .resources("/XObject << /Fm0 12 0 R >>")
            .object(
                12,
                form(
                    &format!("/Group << /S /Transparency /CS {cs} /I true >>"),
                    content,
                ),
            )
    };
    let flat = "0.1 0.6 0.3 0.2 k 0 0 40 40 re f";
    let (profile, named) = (
        "/CS0 [/ICCBased 8 0 R]",
        "/CS0 cs 0.1 0.6 0.3 0.2 sc 0 0 40 40 re f",
    );
    check_all(&[
        Case::new(
            "the light render of the same page paints the default",
            page.document(default),
            twin.document(""),
        )
        .unlike(page.document("")),
        Case::new(
            "inside a /DeviceRGB group an ink page reads the default's light",
            grouped("/DeviceRGB", "", flat).document(default),
            grouped("/DeviceRGB", profile, named).document(""),
        )
        .unlike(grouped("/DeviceRGB", "", flat).document(""))
        .with(ink.clone()),
        // A guard on what H-135's default does to a light render: a group
        // whose buffer is CMYK composites a colour's ink, as an ink page
        // does, so inside a `/DeviceCMYK` group the kept ink is what the
        // light render paints — the page without the default, and not its
        // twin naming the profile. Choosing conversion would move this row.
        Case::new(
            "inside a /DeviceCMYK group a light render composites the document's ink",
            grouped("/DeviceCMYK", "", flat).document(default),
            grouped("/DeviceCMYK", "", flat).document(""),
        )
        .unlike(grouped("/DeviceCMYK", profile, named).document("")),
    ]);
}
