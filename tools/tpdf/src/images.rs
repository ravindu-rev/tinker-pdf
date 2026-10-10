//! `tpdf images`: the images each page draws, listed, and with `--out` their
//! samples written out.
//!
//! A wrapper over [`tinker_pdf::Page::images`] with no logic of its own
//! (ruling 11). The samples are written **as the facade hands them over** —
//! before any colour conversion, `/Decode` unapplied, rows from the top padded
//! to a byte, sixteen-bit values big-endian — and nothing here converts them
//! into a picture format, because doing so would mean evaluating the colour
//! space, and a second answer to what a sample looks like is a second
//! renderer. What a reader needs to interpret the bytes is on the listing
//! line beside each file — the geometry, the depth, the component count, the
//! codec that produced them and the space they are in — and in the files
//! written beside them: the space **whole**, as far as the facade hands it
//! over. A CIE space's parameters are a handful of numbers and go on the
//! listing line; what is bytes goes in a file of its own, wherever in the
//! space it sits — an `/Indexed` palette, and every `ICCBased` profile,
//! whether it is the space, an indexed base, a separation's alternate or
//! another profile's `/Alternate`. A `/Separation` or `/DeviceN` space's tint
//! transform is the one part not written: `ImageSpace` carries the colorants'
//! names and the alternate, not the function between them.

use std::path::Path;

use tinker_pdf::{Document, ImageMask, ImageSpace, PageImage};

use crate::Options;

/// How deep a colour space description follows `/Alternate` and `/Indexed`
/// bases before it stops.
///
/// 8.6.6.3 forbids an indexed base that is itself indexed or a pattern, and
/// an alternate is a device or CIE space, so a real space is two levels deep.
/// The bound is for a document that says otherwise: a description is not
/// worth a stack.
const SPACE_DEPTH: u32 = 8;

/// Lists the images each page draws and, with `--out DIR`, writes them.
pub(crate) fn images(options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    for line in image_lines(options, path, doc)? {
        println!("{line}");
    }
    Ok(())
}

/// What `images` prints, built rather than printed so it can be asserted on.
///
/// Files are named `<stem>-pNNNN-NNN` by page and by the image's position on
/// it: `.raw` for the samples, `-smask.raw` for a soft mask's, `-mask.raw` for
/// a stencil mask's, and for the space what [`space_files`] names.
pub(crate) fn image_lines(
    options: &Options,
    path: &str,
    doc: &Document,
) -> Result<Vec<String>, String> {
    let stem = Path::new(path)
        .file_stem()
        .map_or_else(|| "image".to_string(), |s| s.to_string_lossy().into_owned());
    if let Some(dir) = options.out.as_deref() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {dir}: {e}"))?;
    }

    let mut lines = vec![];
    let mut found = 0usize;
    for index in options.pages(doc) {
        let Some(page) = doc.page(index) else {
            continue;
        };
        for (position, image) in page.images().iter().enumerate() {
            found += 1;
            lines.push(format!(
                "  page {} #{position} {}",
                index + 1,
                describe(image)
            ));
            if let Some(refused) = &image.refused {
                lines.push(format!("    refused: {refused}"));
            }
            for warning in &image.warnings {
                lines.push(format!("    warning: {warning}"));
            }
            let Some(dir) = options.out.as_deref() else {
                continue;
            };
            let base = format!("{dir}/{stem}-p{:04}-{position:03}", index + 1);
            lines.extend(write(&format!("{base}.raw"), &image.samples)?);
            if let Some(space) = &image.color_space {
                for (suffix, bytes) in space_files(space, String::new(), 0) {
                    lines.extend(write(&format!("{base}{suffix}"), bytes)?);
                }
            }
            if let Some(ImageMask::Stencil(mask)) = &image.mask {
                lines.extend(write(&format!("{base}-mask.raw"), &mask.samples)?);
            }
            if let Some(soft) = &image.soft_mask {
                lines.extend(write(&format!("{base}-smask.raw"), &soft.samples)?);
            }
        }
    }
    lines.insert(0, format!("{path}: {found} images"));
    Ok(lines)
}

/// The parts of a space that are bytes, each with the suffix its file takes:
/// `.icc` for a profile and `-palette.raw` for an `/Indexed` lookup table,
/// after a `-base` or `-alternate` for each step down into the space that
/// reached it.
///
/// So an `ICCBased` image's profile is `.icc`, an indexed image's palette
/// `-palette.raw` and the profile that palette's entries are in `-base.icc`,
/// and a separation's fallback profile `-alternate.icc`. Every family names
/// at most one space inside it, so each path down is a suffix of its own and
/// no two files collide. An empty profile — a stream that would not decode —
/// or an empty palette writes nothing.
fn space_files(space: &ImageSpace, suffix: String, depth: u32) -> Vec<(String, &[u8])> {
    if depth > SPACE_DEPTH {
        return Vec::new();
    }
    let mut files = Vec::new();
    let inner = match space {
        ImageSpace::Icc {
            profile, alternate, ..
        } => {
            if !profile.is_empty() {
                files.push((format!("{suffix}.icc"), profile.as_slice()));
            }
            alternate.as_deref().map(|a| (a, "-alternate"))
        }
        ImageSpace::Indexed { base, lookup, .. } => {
            if !lookup.is_empty() {
                files.push((format!("{suffix}-palette.raw"), lookup.as_slice()));
            }
            Some((base.as_ref(), "-base"))
        }
        ImageSpace::Separation { alternate, .. } | ImageSpace::DeviceN { alternate, .. } => {
            Some((alternate.as_ref(), "-alternate"))
        }
        _ => None,
    };
    if let Some((inner, step)) = inner {
        files.extend(space_files(inner, format!("{suffix}{step}"), depth + 1));
    }
    files
}

/// Writes one file and says so.
fn write(file: &str, bytes: &[u8]) -> Result<Vec<String>, String> {
    std::fs::write(file, bytes).map_err(|e| format!("writing {file}: {e}"))?;
    Ok(vec![format!("    wrote {file} ({} bytes)", bytes.len())])
}

/// One image on one line: what it is called, its geometry and depth, the
/// codec and space its samples are in, and how it is masked and drawn.
fn describe(image: &PageImage) -> String {
    let name = match (image.reference, image.name.is_empty()) {
        (Some(r), false) => format!("/{} ({} {} R)", lossy(&image.name), r.num, r.gen),
        (Some(r), true) => format!("({} {} R)", r.num, r.gen),
        (None, _) => "inline".to_string(),
    };
    let space = match &image.color_space {
        Some(space) => space_name(space, 0),
        None if image.stencil => "a stencil mask".to_string(),
        None => "the space its codestream names".to_string(),
    };
    let mut text = format!(
        "{name} {}x{}, {} bits x {}, {:?}, {space}",
        image.width, image.height, image.bits_per_component, image.components, image.codec
    );
    if !image.decode.is_empty() {
        let pairs: Vec<String> = image
            .decode
            .iter()
            .map(|(min, max)| format!("{min} {max}"))
            .collect();
        text.push_str(&format!(", /Decode [{}]", pairs.join(" ")));
    }
    match &image.mask {
        Some(ImageMask::ColorKey(ranges)) => {
            let pairs: Vec<String> = ranges
                .iter()
                .map(|(min, max)| format!("{min} {max}"))
                .collect();
            text.push_str(&format!(", colour key [{}]", pairs.join(" ")));
        }
        Some(ImageMask::Stencil(mask)) => {
            text.push_str(&format!(", stencil mask {}x{}", mask.width, mask.height));
        }
        Some(_) => text.push_str(", a mask"),
        None => {}
    }
    if let Some(soft) = &image.soft_mask {
        text.push_str(&format!(
            ", soft mask {}x{} at {} bits",
            soft.width, soft.height, soft.bits_per_component
        ));
    }
    let times = image.placements.len();
    text.push_str(&format!(
        ", drawn {times} {}",
        if times == 1 { "time" } else { "times" }
    ));
    text
}

/// A colour space named as 8.6 names it, with what tells two of one family
/// apart — the profile's size, the palette's length, the colorants.
fn space_name(space: &ImageSpace, depth: u32) -> String {
    if depth > SPACE_DEPTH {
        return "...".to_string();
    }
    let inner = |space: &ImageSpace| space_name(space, depth + 1);
    match space {
        ImageSpace::DeviceGray => "DeviceGray".to_string(),
        ImageSpace::DeviceRgb => "DeviceRGB".to_string(),
        ImageSpace::DeviceCmyk => "DeviceCMYK".to_string(),
        // 8.6.5: a CIE space is its parameters, few enough to print in the
        // dictionary's own spelling.
        ImageSpace::CalGray { white, gamma } => {
            format!("CalGray /WhitePoint {} /Gamma {gamma}", numbers(white))
        }
        ImageSpace::CalRgb {
            white,
            gamma,
            matrix,
        } => format!(
            "CalRGB /WhitePoint {} /Gamma {} /Matrix {}",
            numbers(white),
            numbers(gamma),
            numbers(matrix)
        ),
        ImageSpace::Lab { white, range } => format!(
            "Lab /WhitePoint {} /Range {}",
            numbers(white),
            numbers(range)
        ),
        ImageSpace::Icc {
            components,
            profile,
            alternate,
        } => {
            let mut text = format!("ICCBased /N {components}, a {}-byte profile", profile.len());
            if let Some(alternate) = alternate {
                text.push_str(&format!(", alternate {}", inner(alternate)));
            }
            text
        }
        ImageSpace::Indexed { base, high, lookup } => {
            format!(
                "Indexed over {}, {} entries, a {}-byte palette",
                inner(base),
                u32::from(*high) + 1,
                lookup.len()
            )
        }
        ImageSpace::Separation {
            colorant,
            alternate,
        } => format!("Separation /{} over {}", lossy(colorant), inner(alternate)),
        ImageSpace::DeviceN {
            colorants,
            alternate,
        } => {
            let names: Vec<String> = colorants.iter().map(|c| format!("/{}", lossy(c))).collect();
            format!("DeviceN [{}] over {}", names.join(" "), inner(alternate))
        }
        ImageSpace::Unreadable { family } => format!("an unreadable space /{}", lossy(family)),
        // `#[non_exhaustive]`: a family the facade learns to describe later
        // is named by its debug form until this learns its words.
        other => format!("{other:?}"),
    }
}

/// Numbers as a PDF array spells them: `[0.9505 1 1.089]`.
fn numbers(values: &[f64]) -> String {
    let spelled: Vec<String> = values.iter().map(f64::to_string).collect();
    format!("[{}]", spelled.join(" "))
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf::{
        CompressedImage, DeviceSpace, DocumentBuilder, ImageColorSpace, ImageData, SoftMask,
    };

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("tpdf-images-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir.to_string_lossy().replace('\\', "/")
    }

    fn parse(args: &[&str]) -> Options {
        let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        Options::parse(&args).expect("the arguments parse")
    }

    const RGB: &[u8] = &[
        255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 8, 7, 1, 2, 3, 250, 251, 252,
    ];
    const GREY: &[u8] = &[0, 64, 128, 255];
    const PALETTE: &[u8] = &[0, 0, 0, 255, 255, 255];
    const INDICES: &[u8] = &[0b1010_0000, 0b0101_0000];
    const ALPHA: &[u8] = &[255, 128, 64, 0];
    /// A profile's bytes. Nothing reads inside it here: the listing reports
    /// its size and `--out` writes it, which is all the test asks.
    const PROFILE: &[u8] = b"not a real ICC profile";
    const TONES: &[u8] = &[10, 20];

    /// Two pages: an RGB image drawn twice and a grey one on the first, and
    /// on the second an indexed one-bit image with a soft mask and a
    /// one-component ICC-based image. Built from known arrays, so the right
    /// answer is the array (ruling 13).
    fn document() -> Document {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 3,
                height: 2,
                data: RGB
            }
        ));
        assert!(builder.add_image(
            b"Im1",
            &ImageData::Gray8 {
                width: 2,
                height: 2,
                data: GREY
            }
        ));
        assert!(builder.add_image(
            b"Im2",
            &ImageData::Compressed(CompressedImage {
                width: 4,
                height: 2,
                bits_per_component: 1,
                color_space: ImageColorSpace::Indexed {
                    base: DeviceSpace::Rgb,
                    lookup: PALETTE,
                },
                filter: None,
                data: INDICES,
                color_key_mask: None,
                soft_mask: Some(SoftMask {
                    width: 2,
                    height: 2,
                    bits_per_component: 8,
                    filter: None,
                    data: ALPHA,
                }),
            })
        ));
        builder.add_page(100.0, 100.0, |page| {
            page.image(b"Im0", 0.0, 0.0, 30.0, 20.0);
            page.image(b"Im1", 40.0, 0.0, 20.0, 20.0);
            page.image(b"Im0", 0.0, 50.0, 30.0, 20.0);
        });
        assert!(builder.add_icc_color_space(b"CS0", PROFILE, 1));
        assert!(builder.add_image(
            b"Im3",
            &ImageData::Compressed(CompressedImage {
                width: 2,
                height: 1,
                bits_per_component: 8,
                color_space: ImageColorSpace::Icc {
                    resource: b"CS0",
                    components: 1,
                },
                filter: None,
                data: TONES,
                color_key_mask: None,
                soft_mask: None,
            })
        ));
        builder.add_page(100.0, 100.0, |page| {
            page.image(b"Im2", 0.0, 0.0, 40.0, 20.0);
            page.image(b"Im3", 50.0, 0.0, 20.0, 10.0);
        });
        Document::open(builder.finish()).expect("the built document opens")
    }

    /// The listing names each image once with its geometry, depth, codec and
    /// space, and `--out` writes exactly the samples the document holds.
    #[test]
    fn each_image_is_listed_and_its_samples_written_as_the_facade_hands_them() {
        let dir = scratch("listing");
        let doc = document();
        let lines =
            image_lines(&parse(&["doc.pdf", "--out", &dir]), "doc.pdf", &doc).expect("lists");
        let text = lines.join("\n");
        assert_eq!(lines[0], "doc.pdf: 4 images", "{text}");
        assert!(
            text.contains("page 1 #0 /Im0") && text.contains("3x2, 8 bits x 3, Stream, DeviceRGB"),
            "{text}"
        );
        assert!(
            text.contains("drawn 2 times"),
            "an image drawn twice is listed once: {text}"
        );
        assert!(
            text.contains("page 1 #1 /Im1") && text.contains("DeviceGray"),
            "{text}"
        );
        assert!(
            text.contains("page 2 #0 /Im2")
                && text.contains(
                    "4x2, 1 bits x 1, Stream, Indexed over DeviceRGB, 2 entries, a 6-byte palette"
                )
                && text.contains("soft mask 2x2 at 8 bits"),
            "{text}"
        );
        assert!(
            text.contains("page 2 #1 /Im3")
                && text.contains(&format!(
                    "2x1, 8 bits x 1, Stream, ICCBased /N 1, a {}-byte profile, drawn 1 time",
                    PROFILE.len()
                )),
            "{text}"
        );

        let read = |name: &str| std::fs::read(format!("{dir}/{name}")).expect(name);
        assert_eq!(read("doc-p0001-000.raw"), RGB);
        assert_eq!(read("doc-p0001-001.raw"), GREY);
        assert_eq!(read("doc-p0002-000.raw"), INDICES, "indices, not colours");
        assert_eq!(
            read("doc-p0002-000-palette.raw"),
            PALETTE,
            "and beside them the colours they index"
        );
        assert_eq!(read("doc-p0002-000-smask.raw"), ALPHA);
        assert_eq!(read("doc-p0002-001.raw"), TONES);
        assert_eq!(
            read("doc-p0002-001.icc"),
            PROFILE,
            "the space, beside its samples"
        );
        let written = std::fs::read_dir(&dir).expect("the directory").count();
        assert_eq!(
            written, 7,
            "one file a sample array, palette or profile, and nothing else"
        );
    }

    /// `--page` and `--pages` choose the pages, as they do for `text`; with
    /// no `--out` nothing is written.
    #[test]
    fn the_page_flags_choose_the_pages_and_no_out_writes_nothing() {
        let doc = document();
        let lines =
            image_lines(&parse(&["doc.pdf", "--page", "2"]), "doc.pdf", &doc).expect("lists");
        assert_eq!(lines[0], "doc.pdf: 2 images");
        assert!(lines.iter().all(|l| !l.contains("wrote")), "{lines:?}");
        let lines =
            image_lines(&parse(&["doc.pdf", "--pages", "1-2"]), "doc.pdf", &doc).expect("lists");
        assert_eq!(lines[0], "doc.pdf: 4 images");
    }

    /// One page over hand-written objects, for what `DocumentBuilder` does not
    /// write: `resources` is the page's `/XObject` dictionary's body, the
    /// content draws each name in `names` once, and `objects` follow the
    /// catalog, the page tree, the page and its content, which are 1 to 4.
    fn written(names: &[&str], resources: &str, objects: &[u8]) -> Document {
        let content: String = names
            .iter()
            .enumerate()
            .map(|(at, name)| format!("q 10 0 0 10 {} 0 cm /{name} Do Q ", 10 * at))
            .collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
        out.extend_from_slice(
            format!(
                "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
                 /Resources << /XObject << {resources} >> >> /Contents 4 0 R >>\nendobj\n\
                 4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
                content.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(objects);
        out.extend_from_slice(b"trailer\n<< /Size 40 /Root 1 0 R >>\n%%EOF\n");
        Document::open(out).expect("the written document opens")
    }

    /// An image XObject `number` with `dict` beside the geometry and `data`
    /// as its unfiltered samples.
    fn image_object(number: u32, dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "{number} 0 obj\n<< /Type /XObject /Subtype /Image {dict} /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out
    }

    /// A stream object `number` with `dict` and `data`: a profile here.
    fn stream_object(number: u32, dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "{number} 0 obj\n<< {dict} /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out
    }

    const UNDER_THE_PALETTE: &[u8] = b"the profile an indexed image's palette is in";
    const UNDER_THE_TINT: &[u8] = b"the profile a separation falls back to";
    const OUTER: &[u8] = b"an ICC-based image's own profile";
    const ITS_ALTERNATE: &[u8] = b"the profile its /Alternate names";
    const TWO_COLOURS: &[u8] = &[255, 0, 0, 0, 255, 0];

    /// **The space is written out whole**, not only a top-level profile: a
    /// palette beside its indices, a profile wherever in the space it sits —
    /// under an `/Indexed` base, a `/Separation`'s alternate, an `ICCBased`
    /// space's own `/Alternate` — and a CIE space's parameters on the listing
    /// line. Every number and every byte is one this test wrote.
    #[test]
    fn every_part_of_the_space_is_written_beside_the_samples() {
        let dir = scratch("spaces");
        let white = "/WhitePoint [0.875 1 1.125]";
        let mut objects = Vec::new();
        objects.extend(image_object(
            5,
            &format!(
                "/Width 2 /Height 1 /BitsPerComponent 8 \
                 /ColorSpace [/CalGray << {white} /Gamma 2.25 >>]"
            ),
            &[1, 2],
        ));
        objects.extend(image_object(
            6,
            &format!(
                "/Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/CalRGB << {white} \
                 /Gamma [1.75 2 2.5] /Matrix [0.5 0.25 0.125 0.25 0.5 0.125 0.125 0.25 0.5] >>]"
            ),
            &[3, 4, 5],
        ));
        objects.extend(image_object(
            7,
            &format!(
                "/Width 1 /Height 1 /BitsPerComponent 8 \
                 /ColorSpace [/Lab << {white} /Range [-128 127 -64 63] >>]"
            ),
            &[6, 7, 8],
        ));
        objects.extend(image_object(
            8,
            "/Width 2 /Height 1 /BitsPerComponent 8 \
             /ColorSpace [/Indexed [/ICCBased 20 0 R] 1 <FF000000FF00>]",
            &[0, 1],
        ));
        objects.extend(image_object(
            9,
            "/Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/Separation /Gold \
             [/ICCBased 21 0 R] << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 0] /N 1 >>]",
            &[9],
        ));
        objects.extend(image_object(
            10,
            "/Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/ICCBased 22 0 R]",
            &[10],
        ));
        objects.extend(stream_object(20, "/N 3", UNDER_THE_PALETTE));
        objects.extend(stream_object(21, "/N 3", UNDER_THE_TINT));
        objects.extend(stream_object(
            22,
            "/N 1 /Alternate [/ICCBased 23 0 R]",
            OUTER,
        ));
        objects.extend(stream_object(23, "/N 1", ITS_ALTERNATE));
        let doc = written(
            &["Im0", "Im1", "Im2", "Im3", "Im4", "Im5"],
            "/Im0 5 0 R /Im1 6 0 R /Im2 7 0 R /Im3 8 0 R /Im4 9 0 R /Im5 10 0 R",
            &objects,
        );

        let lines =
            image_lines(&parse(&["doc.pdf", "--out", &dir]), "doc.pdf", &doc).expect("lists");
        let text = lines.join("\n");
        assert_eq!(lines[0], "doc.pdf: 6 images", "{text}");
        for described in [
            "/Im0 (5 0 R) 2x1, 8 bits x 1, Stream, \
             CalGray /WhitePoint [0.875 1 1.125] /Gamma 2.25,",
            "/Im1 (6 0 R) 1x1, 8 bits x 3, Stream, CalRGB /WhitePoint [0.875 1 1.125] \
             /Gamma [1.75 2 2.5] /Matrix [0.5 0.25 0.125 0.25 0.5 0.125 0.125 0.25 0.5],",
            "/Im2 (7 0 R) 1x1, 8 bits x 3, Stream, \
             Lab /WhitePoint [0.875 1 1.125] /Range [-128 127 -64 63],",
            &format!(
                "/Im3 (8 0 R) 2x1, 8 bits x 1, Stream, Indexed over ICCBased /N 3, \
                 a {}-byte profile, 2 entries, a 6-byte palette,",
                UNDER_THE_PALETTE.len()
            ),
            &format!(
                "/Im4 (9 0 R) 1x1, 8 bits x 1, Stream, Separation /Gold over ICCBased /N 3, \
                 a {}-byte profile,",
                UNDER_THE_TINT.len()
            ),
            &format!(
                "/Im5 (10 0 R) 1x1, 8 bits x 1, Stream, ICCBased /N 1, a {}-byte profile, \
                 alternate ICCBased /N 1, a {}-byte profile,",
                OUTER.len(),
                ITS_ALTERNATE.len()
            ),
        ] {
            assert!(text.contains(described), "{described}\n{text}");
        }

        let read = |name: &str| std::fs::read(format!("{dir}/{name}")).expect(name);
        assert_eq!(read("doc-p0001-000.raw"), [1, 2]);
        assert_eq!(read("doc-p0001-001.raw"), [3, 4, 5]);
        assert_eq!(read("doc-p0001-002.raw"), [6, 7, 8]);
        assert_eq!(read("doc-p0001-003.raw"), [0, 1], "indices");
        assert_eq!(
            read("doc-p0001-003-palette.raw"),
            TWO_COLOURS,
            "and the colours they index"
        );
        assert_eq!(
            read("doc-p0001-003-base.icc"),
            UNDER_THE_PALETTE,
            "and the space those colours are in"
        );
        assert_eq!(read("doc-p0001-004.raw"), [9]);
        assert_eq!(read("doc-p0001-004-alternate.icc"), UNDER_THE_TINT);
        assert_eq!(read("doc-p0001-005.raw"), [10]);
        assert_eq!(read("doc-p0001-005.icc"), OUTER);
        assert_eq!(read("doc-p0001-005-alternate.icc"), ITS_ALTERNATE);
        let written = std::fs::read_dir(&dir).expect("the directory").count();
        assert_eq!(written, 11, "six sample arrays, a palette, four profiles");
    }

    /// A stencil mask's samples are its own, written beside the image's; an
    /// image that will not decode says why; one decoded with a leniency says
    /// what was forgiven (ruling 10).
    #[test]
    fn a_stencil_mask_a_refusal_and_a_leniency_are_each_reported() {
        let dir = scratch("masks");
        // `page_images.rs`'s damaged fax: one good row, then a code T.4 does
        // not have, replicated from the row above.
        let fax = [0b0010_0110, 0b1010_1111, 0b1000_0000, 0b1000_0000];
        let mut objects = Vec::new();
        objects.extend(image_object(
            5,
            "/Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Mask 8 0 R",
            &[11, 12],
        ));
        objects.extend(image_object(
            6,
            "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /DCTDecode",
            b"not a JPEG",
        ));
        objects.extend(image_object(
            7,
            "/Width 8 /Height 4 /BitsPerComponent 1 /ColorSpace /DeviceGray \
             /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 8 /Rows 4 >>",
            &fax,
        ));
        objects.extend(image_object(
            8,
            "/Width 8 /Height 1 /BitsPerComponent 1 /ImageMask true",
            &[0b1010_0101],
        ));
        let doc = written(
            &["Im0", "Im1", "Im2"],
            "/Im0 5 0 R /Im1 6 0 R /Im2 7 0 R",
            &objects,
        );

        let lines =
            image_lines(&parse(&["doc.pdf", "--out", &dir]), "doc.pdf", &doc).expect("lists");
        let text = lines.join("\n");
        let at = |prefix: &str| {
            lines
                .iter()
                .position(|l| l.starts_with(prefix))
                .unwrap_or_else(|| panic!("{prefix}\n{text}"))
        };
        assert!(
            lines[at("  page 1 #0 /Im0")].contains("stencil mask 8x1"),
            "{text}"
        );
        let refused = at("  page 1 #1 /Im1");
        assert!(
            lines[refused + 1].starts_with("    refused: "),
            "the refusal is under its image: {text}"
        );
        let forgiven = at("  page 1 #2 /Im2");
        assert!(
            lines[forgiven + 1].starts_with("    warning: "),
            "the leniency is under its image: {text}"
        );

        let read = |name: &str| std::fs::read(format!("{dir}/{name}")).expect(name);
        assert_eq!(read("doc-p0001-000.raw"), [11, 12]);
        assert_eq!(
            read("doc-p0001-000-mask.raw"),
            [0b1010_0101],
            "the mask's samples, not the image's"
        );
        assert_eq!(
            read("doc-p0001-002.raw").len(),
            4,
            "every row, one replicated"
        );
    }

    /// A page with no images says so rather than printing nothing.
    #[test]
    fn a_document_with_no_images_counts_none() {
        let path = format!(
            "{}/../../testdata/simple-text.pdf",
            env!("CARGO_MANIFEST_DIR")
        );
        let doc = Document::open(std::fs::read(&path).expect("the fixture")).expect("it opens");
        let lines = image_lines(&parse(&[&path]), &path, &doc).expect("lists");
        assert_eq!(lines, vec![format!("{path}: 0 images")]);
    }
}
