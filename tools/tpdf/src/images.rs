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
//! line beside each file: the geometry, the depth, the component count, the
//! codec that produced them and the space they are in. An `ICCBased` image's
//! profile is written beside its samples, since it *is* the space.

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
/// a stencil mask's, and `.icc` for the profile of an `ICCBased` image.
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
            if let Some(ImageSpace::Icc { profile, .. }) = &image.color_space {
                if !profile.is_empty() {
                    lines.extend(write(&format!("{base}.icc"), profile)?);
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
        ImageSpace::CalGray { .. } => "CalGray".to_string(),
        ImageSpace::CalRgb { .. } => "CalRGB".to_string(),
        ImageSpace::Lab { .. } => "Lab".to_string(),
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
        ImageSpace::Indexed { base, high, .. } => {
            format!(
                "Indexed over {}, {} entries",
                inner(base),
                u32::from(*high) + 1
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
                && text.contains("4x2, 1 bits x 1, Stream, Indexed over DeviceRGB, 2 entries")
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
        assert_eq!(read("doc-p0002-000-smask.raw"), ALPHA);
        assert_eq!(read("doc-p0002-001.raw"), TONES);
        assert_eq!(
            read("doc-p0002-001.icc"),
            PROFILE,
            "the space, beside its samples"
        );
        let written = std::fs::read_dir(&dir).expect("the directory").count();
        assert_eq!(
            written, 6,
            "one file a sample array or profile, and nothing else"
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
