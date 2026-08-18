//! `Glyphs`, and the font part it names (gap 30, milestone 7).
//!
//! # Why the de-obfuscation is asserted on bytes and not on a page
//!
//! Gap 30's row 7 says it in as many words: *"9.1.7.3's de-obfuscation asserted
//! on the **de-obfuscated bytes**, not on a page that drew"*. The reason is
//! milestone 1's, measured rather than argued — transcribing the clause's
//! B-notation instead of reading it as a reversal transposed two byte pairs,
//! and what came out was a font whose sfnt version was right, whose table tags
//! read `DSIG`, `GDEF`, `GPOS`, `GSUB`, and whose `searchRange`,
//! `entrySelector` and `rangeShift` were garbage. That font parses. A page
//! built on it draws *something*. Only the bytes say which something.
//!
//! So `the_real_part_de_obfuscates_to_the_font_milestone_one_measured` asserts
//! the sixteen bytes milestone 1 wrote down, and
//! `the_key_is_the_guid_reversed_and_not_its_b_names_transcribed` builds the
//! wrong key on purpose and asserts that it is caught — because a positive
//! assertion alone cannot catch a weakened check, and this is the one place in
//! this gap where the weakened version is known to look right.
//!
//! # The pairs
//!
//! Milestone 5 stated the rule the five milestones before this one each found
//! once: **when a thing has two independent consequences, a test for one of
//! them is not a test.** `Indices` is where this format keeps most of them.
//!
//! | one rule | its twin |
//! | --- | --- |
//! | the content type selects the de-obfuscation | the **extension** does not, in either direction |
//! | a stated `AdvanceWidth` moves the pen | a `uOffset` moves the glyph and **not** the pen |
//! | a cluster's `ClusterCodeUnitCount` decides how much text one glyph stands for | its `ClusterGlyphCount` decides how many glyphs share it |
//! | `Indices` names the glyph | `UnicodeString` names what it stands for, and the two are independent |
//! | a run draws | the same run **extracts**, through a different reader |
//! | an even `BidiLevel` draws | an odd one is refused by name |

mod xps_support;

use tinker_pdf::xps::font::{deobfuscate, obfuscation_key, OBFUSCATED_FONT_MEDIA_TYPE};
use tinker_pdf::xps::glyphs::{self, Cluster, Mapping};
use tinker_pdf::xps::opc::PartName;
use tinker_pdf::{
    ArchiveRefusal, ArchiveWarning, Bitmap, Document, RenderOptions, XpsElementDefect,
};
use xps_support::{
    archive, before_content_types, binary_part, collection, content_types_with, disagreeing_font,
    font_part_name, obfuscate, one_page_package, open, with, FONT_GUID, XPS_NS,
};

/// Table D-4's media type for a plain OpenType font part, which is what a
/// package that does not obfuscate its fonts writes.
const PLAIN_FONT_MEDIA_TYPE: &str = "application/vnd.ms-opentype";

// ---- fixtures -----------------------------------------------------------

/// A package whose one 200 x 200 fixed page carries `body` and whose font part
/// is named, typed and obfuscated exactly as asked.
///
/// The three are separate arguments because 9.1.7.3 makes them three
/// independent facts: a part's **name** may end in anything, its **media
/// type** is what selects the de-obfuscation, and its **bytes** are whatever
/// the producer put there. A fixture that tied any two together could not tell
/// a build that read the extension from one that read the content type.
fn package(body: &str, extension: &str, media_type: &str, program: &[u8]) -> Vec<u8> {
    let markup =
        format!(r#"<FixedPage xmlns="{XPS_NS}" Width="200" Height="200">{body}</FixedPage>"#);
    let parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &markup);
    let parts = before_content_types(
        parts,
        binary_part(&font_part_name(extension), program.to_vec()),
    );
    let types = content_types_with(&format!(
        r#"<Default Extension="{extension}" ContentType="{media_type}" />"#
    ));
    archive(with(parts, "[Content_Types].xml", &types))
}

/// The ordinary case: an obfuscated `.odttf` holding the disagreeing font.
fn obfuscated(body: &str) -> Vec<u8> {
    package(
        body,
        "odttf",
        OBFUSCATED_FONT_MEDIA_TYPE,
        &obfuscate(FONT_GUID, &disagreeing_font()),
    )
}

/// The `FontUri` a fixture's markup names, in XPS 1.0's absolute form.
fn font_uri(extension: &str) -> String {
    format!("/{}", font_part_name(extension))
}

/// A `Glyphs` element at origin `(10, 100)` and em size **100**, carrying
/// `extra` verbatim.
///
/// A hundred-unit em is chosen so that 12.1.3's hundredths of an em are whole
/// XPS units: an `AdvanceWidth` of `80` is eighty units, and a test can be
/// read against the markup rather than against arithmetic done twice — which
/// is the property gap 30's geometry section asks not to be thrown away.
fn run(extra: &str) -> String {
    format!(
        r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="{}" Fill="#000000" {extra} />"##,
        font_uri("odttf")
    )
}

/// One page's content stream, as text.
fn stream(document: &Document) -> String {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// The content stream a `Glyphs` element with these attributes produces.
fn drawn(extra: &str) -> String {
    let bytes = obfuscated(&run(extra));
    let document = open(&bytes).unwrap_or_else(|e| panic!("{extra}: {e:?}"));
    stream(&document)
}

/// The `TJ` array of the one run in a stream, whitespace collapsed.
fn array(stream: &str) -> String {
    let at = stream
        .find('[')
        .unwrap_or_else(|| panic!("a TJ array in {stream}"));
    let end = stream[at..]
        .find("] TJ")
        .unwrap_or_else(|| panic!("a TJ array in {stream}"));
    stream[at..at + end + 1]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every element-level defect a package reported, in order.
fn defects(bytes: &[u8]) -> Vec<XpsElementDefect> {
    let document = open(bytes).expect("an XPS");
    document
        .archive()
        .expect("a synthesised document")
        .warnings()
        .iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
            _ => None,
        })
        .collect()
}

/// The defects one `Glyphs` element produces.
fn run_defects(extra: &str) -> Vec<XpsElementDefect> {
    defects(&obfuscated(&run(extra)))
}

/// One of milestone 1's real packages, unmodified.
fn corpus(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The real ODTTF part of `wpf-image-and-text.xps`, still obfuscated.
fn real_font_part() -> (PartName, Vec<u8>) {
    let bytes = corpus("wpf-image-and-text.xps");
    let archive = tinker_pdf::cbz::open_archive(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
        .expect("a readable ZIP");
    let mut package = tinker_pdf::xps::opc::Package::open(
        archive,
        tinker_pdf_zip::limits::MAX_ZIP_NAME_LEN,
        tinker_pdf_xml::Limits::DEFAULT,
    );
    let name = PartName::from_absolute(&format!("/Resources/{FONT_GUID}.ODTTF")).expect("a part");
    let data = package.read_part(&name).expect("the font part").to_vec();
    (name, data)
}

/// A 200 x 200 fixed page rendered at one output pixel per PDF point.
fn render(bytes: &[u8]) -> Bitmap {
    open(bytes)
        .expect("an XPS")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// The pixel at an **XPS-unit** position on a 200 x 200 fixed page.
///
/// Stated in the markup's own units rather than the render's, so an assertion
/// reads against the `OriginX` and `FontRenderingEmSize` the fixture wrote.
fn ink(bitmap: &Bitmap, x: f64, y: f64) -> (u8, u8, u8) {
    let px = ((x * 0.75) as u32).min(bitmap.width - 1);
    let py = ((y * 0.75) as u32).min(bitmap.height - 1);
    let base = (py as usize) * bitmap.stride + (px as usize) * bitmap.components();
    let p = bitmap.data.get(base..base + 3).unwrap_or(&[255, 255, 255]);
    (p[0], p[1], p[2])
}

fn dark(pixel: (u8, u8, u8)) -> bool {
    pixel.0 < 128
}

/// The one composite font a page names, as a dictionary.
fn font_dict(document: &Document) -> tinker_pdf_cos::Dict {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    let resources = page.resources.as_ref().expect("the page has resources");
    let table = cos.resolve_key(resources, cos.intern(b"Font"));
    let table = table.as_dict().expect("a /Font sub-dictionary");
    let value = cos.resolve_key(table, cos.intern(b"XF0"));
    value.as_dict().cloned().expect("the font /XF0")
}

/// The `/ToUnicode` CMap the writer wrote, as text.
///
/// Read through the font dictionary rather than by searching the file, so the
/// assertion is about the CMap **this page's font names** and not about a
/// stream that happens to be in the document.
fn to_unicode(bytes: &[u8]) -> String {
    let document = open(bytes).expect("an XPS");
    let cos = document.cos();
    let font = font_dict(&document);
    let stream = font
        .get_ref(cos.intern(b"ToUnicode"))
        .expect("a /ToUnicode stream");
    let cmap = cos.stream_decoded(stream).expect("it decodes");
    String::from_utf8_lossy(&cmap).into_owned()
}

/// The descendant `CIDFont`'s `/W` array, flattened to text.
fn widths(bytes: &[u8]) -> String {
    let document = open(bytes).expect("an XPS");
    let cos = document.cos();
    let font = font_dict(&document);
    let descendants = cos.resolve_key(&font, cos.intern(b"DescendantFonts"));
    let descendants = descendants.as_array().expect("a /DescendantFonts array");
    let descendant = cos.resolve(descendants.first().expect("one descendant"));
    let descendant = descendant.as_dict().expect("a CIDFont");
    format!("{:?}", cos.resolve_key(descendant, cos.intern(b"W")))
}

// ---- 9.1.7.3, the de-obfuscation ----------------------------------------

/// **The real part's de-obfuscated bytes, out to its first table tag.**
///
/// Row 7's own criterion, and milestone 1's measurement is what it is checked
/// against: `Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF` in
/// `wpf-image-and-text.xps` begins `01 f4 52 a0 …` and comes out
/// `00 01 00 00 00 18 01 00 00 04 00 80 44 53 49 47` — sfnt version
/// `0x00010000`, twenty-four tables, `searchRange` 256, `entrySelector` 4,
/// `rangeShift` 128, and the tag `DSIG`.
///
/// The three fields after the table count are the whole point of asserting
/// bytes: a key with two pairs transposed gets the first eight of these right
/// and those three wrong, which is a font that still parses.
#[test]
fn the_real_part_de_obfuscates_to_the_font_milestone_one_measured() {
    let (name, obfuscated) = real_font_part();
    assert_eq!(
        &obfuscated[..16],
        &[
            0x01, 0xf4, 0x52, 0xa0, 0x77, 0xde, 0x33, 0xa0, 0xa5, 0x4c, 0xe8, 0x5b, 0xeb, 0x62,
            0x15, 0x1e
        ],
        "the corpus still holds the part milestone 1 measured"
    );

    let clear = deobfuscate(&name, &obfuscated).expect("a GUID part name");
    assert_eq!(
        &clear[..16],
        &[
            0x00, 0x01, 0x00, 0x00, 0x00, 0x18, 0x01, 0x00, 0x00, 0x04, 0x00, 0x80, 0x44, 0x53,
            0x49, 0x47
        ],
        "milestone 1's reference bytes, from tests/xps/README.md"
    );

    let be16 = |at: usize| u16::from_be_bytes([clear[at], clear[at + 1]]);
    assert_eq!(
        u32::from_be_bytes([clear[0], clear[1], clear[2], clear[3]]),
        0x0001_0000
    );
    assert_eq!(be16(4), 24, "twenty-four tables");
    assert_eq!(be16(6), 256, "searchRange");
    assert_eq!(be16(8), 4, "entrySelector");
    assert_eq!(be16(10), 128, "rangeShift");
    assert_eq!(&clear[12..16], b"DSIG", "the first table tag");

    // And the three after it, because one right tag is a coincidence a
    // transposed key can produce.
    assert_eq!(&clear[28..32], b"GDEF");
    assert_eq!(&clear[44..48], b"GPOS");
}

/// **The key is the GUID's sixteen bytes reversed, and transcribing the
/// clause's B-names instead is the trap milestone 1 fell into.**
///
/// This test builds the wrong key — the one that comes out of reading *"B37,
/// B36, B35, B34, B33, B32, B31, B30, B20, B21, B10, B11, B00, B01, B02,
/// B03"* as a list of names rather than as a reversal — and asserts it is
/// **not** what this code uses. A positive assertion alone cannot catch a
/// weakened check, and here the weakened version is known to look right: the
/// sfnt version and every table tag survive it.
#[test]
fn the_key_is_the_guid_reversed_and_not_its_b_names_transcribed() {
    let (name, obfuscated) = real_font_part();
    let key = obfuscation_key(&name).expect("a GUID part name");

    let digits: Vec<u8> = {
        let hex: String = FONT_GUID.chars().filter(|c| *c != '-').collect();
        (0..16)
            .map(|at| u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16).expect("a GUID"))
            .collect()
    };
    let mut reversed = digits.clone();
    reversed.reverse();
    assert_eq!(
        key.to_vec(),
        reversed,
        "the key is the sixteen bytes reversed"
    );

    // The transposition. `595c31af-dbe8-48a5-a032-c677a052f501` spells its
    // three two-byte groups `B11B10`, `B21B20` and `B30B31`, and the clause
    // asks for `B10, B11`, `B20, B21` and `B31, B30` — each of them the
    // group's own bytes the other way round. A transcriber who reverses the
    // two groups the clause spells out in full (`B37 … B32` and
    // `B00 … B03`) and leaves the three short ones as the name writes them
    // gets this, which is the key milestone 1 got on its first attempt.
    let b = |n: usize| digits[n];
    let transcribed = [
        b(15),
        b(14),
        b(13),
        b(12),
        b(11),
        b(10),
        b(8),
        b(9),
        b(6),
        b(7),
        b(4),
        b(5),
        b(3),
        b(2),
        b(1),
        b(0),
    ];
    assert_ne!(key, transcribed, "the two orders are not the same key");

    // And what the wrong one produces: the same sfnt version, the same table
    // count, the same table tags — and all three of the fields between them
    // garbage. That is a font that parses, which is why this milestone asserts
    // bytes rather than a page that drew.
    let mut wrong = obfuscated.clone();
    for (at, byte) in wrong.iter_mut().take(32).enumerate() {
        *byte ^= transcribed[at % 16];
    }
    let field = |at: usize| u16::from_be_bytes([wrong[at], wrong[at + 1]]);
    assert_eq!(
        &wrong[..6],
        &[0x00, 0x01, 0x00, 0x00, 0x00, 0x18],
        "the version and the table count survive it"
    );
    assert_eq!(&wrong[12..16], b"DSIG", "and so does the first table tag");
    assert_eq!(&wrong[28..32], b"GDEF", "and the second");
    assert_ne!(field(6), 256, "`searchRange` is where it shows");
    assert_ne!(field(8), 4, "and `entrySelector`");
    assert_ne!(field(10), 128, "and `rangeShift`");
}

/// **Only the first thirty-two bytes are obfuscated**, which is the other half
/// of 9.1.7.3 and the half a `for byte in bytes` would get wrong.
///
/// A part of two hundred bytes, where every byte past the thirty-second is a
/// counter: if the loop ran past thirty-two the tail would not read back.
#[test]
fn only_the_first_thirty_two_bytes_are_obfuscated() {
    let name = PartName::from_absolute(&format!("/{}", font_part_name("odttf"))).expect("a part");
    let plain: Vec<u8> = (0..200u32).map(|n| (n % 251) as u8).collect();
    let scrambled = obfuscate(FONT_GUID, &plain);

    assert_ne!(&scrambled[..32], &plain[..32], "the head is scrambled");
    assert_eq!(&scrambled[32..], &plain[32..], "and the tail is not");

    let back = deobfuscate(&name, &scrambled).expect("a GUID part name");
    assert_eq!(
        back, plain,
        "and the XOR is its own inverse over the whole part"
    );
}

/// A part name that is not a GUID has no key, in every shape a producer could
/// write one that is not.
///
/// The canonical spelling with hyphens and the bare thirty-two digits are
/// **both** read, because 9.1.7.3 says "the remaining characters are the GUID"
/// and neither form is excluded by that sentence.
#[test]
fn a_part_name_that_is_not_a_guid_has_no_key() {
    let key = |name: &str| obfuscation_key(&PartName::from_absolute(name).expect("a part name"));

    assert!(key("/Resources/595c31af-dbe8-48a5-a032-c677a052f501.odttf").is_some());
    assert!(
        key("/Resources/595c31afdbe848a5a032c677a052f501.odttf").is_some(),
        "the unhyphenated spelling is thirty-two hex digits and is still a GUID"
    );
    assert!(
        key("/Resources/595c31af-dbe8-48a5-a032-c677a052f501").is_some(),
        "9.1.7.3 removes the extension, and a segment with none has nothing to remove"
    );

    for not_a_guid in [
        "/Resources/font.odttf",
        "/Resources/595c31af-dbe8-48a5-a032-c677a052f50.odttf",
        "/Resources/595c31af-dbe8-48a5-a032-c677a052f5011.odttf",
        "/Resources/595c31af-dbe8-48a5-a032-c677a052fzzz.odttf",
        "/Resources/595-c31afdbe848a5a032c677a052f501.odttf",
    ] {
        assert!(key(not_a_guid).is_none(), "{not_a_guid} is not a GUID");
    }
}

/// **The content type selects the de-obfuscation and the extension does not,
/// in both directions.**
///
/// Row 7 asks for *"the content type selecting the path and the extension
/// **not** selecting it, from a fixture whose extension lies"*, and that is
/// one clause with two consequences — milestone 3's survivor exactly. So both
/// lies are told:
///
/// - a part named `.odttf` whose media type is plain OpenType, holding
///   **plain** bytes, draws — a build that read the extension would XOR a font
///   that was never obfuscated and read nothing;
/// - a part named `.ttf` whose media type is Table D-4's obfuscated OpenType,
///   holding **obfuscated** bytes, draws — a build that read the extension
///   would hand the scrambled bytes to the font parser.
#[test]
fn the_content_type_selects_the_de_obfuscation_and_the_extension_does_not() {
    let program = disagreeing_font();

    let lying_extension = package(
        &format!(
            r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="/{}" Fill="#000000" UnicodeString="A" />"##,
            font_part_name("odttf")
        ),
        "odttf",
        PLAIN_FONT_MEDIA_TYPE,
        &program,
    );
    assert_eq!(
        defects(&lying_extension),
        [],
        "a plain `.odttf` is not de-obfuscated"
    );
    assert!(
        stream(&open(&lying_extension).expect("an XPS")).contains("<0001>"),
        "and it drew"
    );

    let lying_the_other_way = package(
        &format!(
            r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="/{}" Fill="#000000" UnicodeString="A" />"##,
            font_part_name("ttf")
        ),
        "ttf",
        OBFUSCATED_FONT_MEDIA_TYPE,
        &obfuscate(FONT_GUID, &program),
    );
    assert_eq!(
        defects(&lying_the_other_way),
        [],
        "an obfuscated `.ttf` is de-obfuscated on the strength of its media type"
    );
    assert!(
        stream(&open(&lying_the_other_way).expect("an XPS")).contains("<0001>"),
        "and it drew too"
    );
}

/// The media type is compared **case-insensitively**, because 7.2.3.5 says so
/// and because milestone 1 measured a real `<Default Extension="ODTTF">`
/// against a part named `….ODTTF` in both dialects.
#[test]
fn the_obfuscated_media_type_is_compared_without_regard_to_case() {
    let shouted = package(
        &format!(
            r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="/{}" Fill="#000000" UnicodeString="A" />"##,
            font_part_name("odttf")
        ),
        "odttf",
        "APPLICATION/VND.MS-PACKAGE.OBFUSCATED-OPENTYPE",
        &obfuscate(FONT_GUID, &disagreeing_font()),
    );
    assert_eq!(defects(&shouted), []);
}

/// A part typed as obfuscated whose name carries no GUID is refused **by its
/// own name**, and never as a font that would not parse.
///
/// Two different things about a document: "the bytes were never de-obfuscated,
/// because there is no key" and "the bytes are not a font". A report that
/// could not tell them apart would send somebody looking at the font.
#[test]
fn an_obfuscated_part_with_no_guid_in_its_name_is_named_rather_than_unreadable() {
    let markup = r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="/Resources/plain.odttf" Fill="#000000" UnicodeString="A" />"##;
    let page =
        format!(r#"<FixedPage xmlns="{XPS_NS}" Width="200" Height="200">{markup}</FixedPage>"#);
    let parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &page);
    let parts = before_content_types(
        parts,
        binary_part(
            "Resources/plain.odttf",
            obfuscate(FONT_GUID, &disagreeing_font()),
        ),
    );
    let types = content_types_with(&format!(
        r#"<Default Extension="odttf" ContentType="{OBFUSCATED_FONT_MEDIA_TYPE}" />"#
    ));
    let bytes = archive(with(parts, "[Content_Types].xml", &types));
    assert_eq!(defects(&bytes), [XpsElementDefect::GlyphsFontObfuscation]);
}

/// Bytes that are not a font program are named as that, and not as a part that
/// did not resolve.
#[test]
fn a_font_part_that_is_not_a_font_is_named() {
    let bytes = package(
        &run(r#"UnicodeString="A""#),
        "odttf",
        OBFUSCATED_FONT_MEDIA_TYPE,
        &obfuscate(FONT_GUID, b"not a font at all, not even nearly one....."),
    );
    assert_eq!(defects(&bytes), [XpsElementDefect::GlyphsFontUnreadable]);
}

/// A `FontUri` naming no part at all is named as that.
#[test]
fn a_font_uri_naming_no_part_is_named() {
    let bytes = obfuscated(
        r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="/Resources/nowhere.odttf" Fill="#000000" UnicodeString="A" />"##,
    );
    assert_eq!(defects(&bytes), [XpsElementDefect::GlyphsFontUnresolved]);
}

// ---- 9.1.7, the face selector -------------------------------------------

/// **A collection with a `#n` fragment other than `#0` is refused by name**,
/// and `#0` draws.
///
/// Both halves, because a build that refused every fragment would pass the
/// first on its own — and `Sfnt::parse` takes the first face of a `ttcf`
/// whatever the fragment says, so the silent alternative is a page drawn in
/// the wrong face with nothing said about it.
#[test]
fn a_font_uri_naming_a_face_other_than_the_first_is_refused_by_name() {
    let first = disagreeing_font();
    // A second face that is a font in its own right, so nothing here turns on
    // the collection being malformed.
    let second = disagreeing_font();
    let program = obfuscate(FONT_GUID, &collection(&[&first, &second]));

    let with_fragment = |fragment: &str| {
        package(
            &format!(
                r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="/{}{fragment}" Fill="#000000" UnicodeString="A" />"##,
                font_part_name("odttf")
            ),
            "odttf",
            OBFUSCATED_FONT_MEDIA_TYPE,
            &program,
        )
    };

    assert_eq!(
        defects(&with_fragment("#0")),
        [],
        "the first face is the one this draws"
    );
    assert!(
        stream(&open(&with_fragment("#0")).expect("an XPS")).contains("<0001>"),
        "and it drew"
    );
    assert_eq!(
        defects(&with_fragment("")),
        [],
        "and no fragment is the first face too"
    );
    for fragment in ["#1", "#2", "#first"] {
        assert_eq!(
            defects(&with_fragment(fragment)),
            [XpsElementDefect::GlyphsFontFace],
            "{fragment}"
        );
    }
}

// ---- 12.1.3, the `Indices` grammar --------------------------------------

/// Every part of one `GlyphMapping`, read on its own.
///
/// The grammar is asserted here as well as through a page because a mapping is
/// four independent statements and a page can only show what they add up to:
/// an advance and a `uOffset` that were swapped draw the same glyphs in almost
/// the same places.
#[test]
fn one_glyph_mapping_is_a_cluster_an_index_an_advance_and_two_offsets() {
    let one = |text: &str| {
        let mut parsed = glyphs::indices(text).unwrap_or_else(|| panic!("{text} is 12.1.3's"));
        assert_eq!(parsed.len(), 1, "{text} is one mapping");
        parsed.pop().expect("one")
    };

    assert_eq!(
        one("(2:3)17,80,-5,10.5"),
        Mapping {
            cluster: Some(Cluster {
                code_units: 2,
                glyphs: 3
            }),
            index: Some(17),
            advance: Some(80.0),
            u_offset: Some(-5.0),
            v_offset: Some(10.5),
        }
    );

    // Each part on its own, so a reader that put a value in the wrong slot is
    // caught by the slot it left empty rather than by the one it filled.
    assert_eq!(
        one("17"),
        Mapping {
            index: Some(17),
            ..Mapping::default()
        }
    );
    assert_eq!(
        one(",80"),
        Mapping {
            advance: Some(80.0),
            ..Mapping::default()
        }
    );
    assert_eq!(
        one("17,,5"),
        Mapping {
            index: Some(17),
            u_offset: Some(5.0),
            ..Mapping::default()
        }
    );
    assert_eq!(
        one("17,,,5"),
        Mapping {
            index: Some(17),
            v_offset: Some(5.0),
            ..Mapping::default()
        }
    );
    assert_eq!(one(""), Mapping::default());
    assert_eq!(
        one("(4)9"),
        Mapping {
            cluster: Some(Cluster {
                code_units: 4,
                glyphs: 1
            }),
            index: Some(9),
            ..Mapping::default()
        },
        "`(m)` is `(m:1)`, which is 12.1.3's own default"
    );
}

/// **Exponents in the reals**, which row 7 names and which a hand-written
/// digit scanner would not accept.
///
/// Asserted as *two spellings of one file* rather than as a number that
/// parsed: `1e2` and `100` produce the identical content stream, which is a
/// claim about the drawing and not about the parser's return value.
#[test]
fn a_real_in_the_indices_may_carry_an_exponent() {
    assert_eq!(
        glyphs::indices("1,1e2,-5E-1"),
        glyphs::indices("1,100,-0.5"),
        "an exponent is a spelling of a number and not a different number"
    );
    assert_eq!(
        drawn(r#"Indices="1,1e2" UnicodeString="AA""#),
        drawn(r#"Indices="1,100" UnicodeString="AA""#),
        "and the two spellings are one file"
    );
}

/// `Indices` that is not the grammar is named and **not painted**, in each of
/// the shapes it can fail in.
///
/// Not painted rather than painted grey, because 12.1.3 is where the glyphs,
/// their widths and the text they stand for are all stated: a run read past a
/// mapping it did not understand draws the wrong glyphs at the wrong widths
/// for the wrong characters, which is the geometry rule said about text.
#[test]
fn indices_that_are_not_the_grammar_are_named_and_not_painted() {
    for bad in [
        "abc",       // not a number anywhere
        "1,2,3,4,5", // a fifth field
        "-1",        // a glyph index is not signed
        "65536",     // and it is sixteen bits
        "1.5",       // and it is an integer
        "(0:1)1",    // a cluster of no code units
        "(1:0)1",    // a cluster of no glyphs
        "(2:1",      // an unclosed cluster
        "1,inf",     // an advance a content stream cannot carry
    ] {
        let body = run(&format!(r#"Indices="{bad}" UnicodeString="AA""#));
        assert_eq!(
            defects(&obfuscated(&body)),
            [XpsElementDefect::GlyphsIndicesUnreadable],
            "{bad}"
        );
        assert!(
            !stream(&open(&obfuscated(&body)).expect("an XPS")).contains(" Tj")
                && !stream(&open(&obfuscated(&body)).expect("an XPS")).contains(" TJ"),
            "{bad} drew something"
        );
    }
}

/// **12.1.3's empty `GlyphIndex`**, which is the form the first real file this
/// repository ever opened is written in.
///
/// `Indices=",53"` — no index, an advance, and seven characters after it with
/// nothing said about them at all. The glyph comes from the font's own `cmap`
/// and the advance is the file's.
#[test]
fn an_empty_glyph_index_looks_the_code_unit_up_in_the_fonts_own_cmap() {
    // `A` maps to glyph 1, whose own advance is half an em — fifty units at
    // this em size. The mapping states eighty.
    let content = drawn(r#"Indices=",80" UnicodeString="AA""#);
    assert_eq!(
        array(&content),
        "[<0001> -300 <0001>]",
        "the cmap decided both glyphs and the file decided the first advance"
    );
}

/// **A stated `AdvanceWidth` moves the pen, and a `uOffset` moves the glyph
/// and not the pen.** Two consequences of one grammar, and one fixture each.
///
/// The advance is the whole of the first assertion: the second glyph sits
/// where the first mapping said the pen would be. The offset is the whole of
/// the second: the second glyph sits where the font's own advance puts it,
/// **not** twenty-five units further on, because 12.1.3's offset displaces the
/// glyph and leaves the pen alone.
#[test]
fn an_advance_moves_the_pen_and_an_offset_moves_only_the_glyph() {
    // Glyph 1's own advance is 500/1000 em, which is fifty units here.
    assert_eq!(
        array(&drawn(r#"UnicodeString="AA""#)),
        "[<0001><0001>]",
        "with nothing stated the font's own advance places the second glyph"
    );
    assert_eq!(
        array(&drawn(r#"Indices=",80" UnicodeString="AA""#)),
        "[<0001> -300 <0001>]",
        "a stated advance of eighty units moves the second glyph thirty further on"
    );
    assert_eq!(
        array(&drawn(r#"Indices="1,,25" UnicodeString="AA""#)),
        "[-250 <0001> 250 <0001>]",
        "and an offset of twenty-five moves the first glyph and puts the pen back"
    );
}

/// A `vOffset` is 9.4.3's rise: it lifts the glyph off the baseline and leaves
/// the pen where it was, and it is **turned off again** before the run ends.
///
/// The last part is not decoration. `Ts` is graphics state rather than
/// text-object state, so a run that left one set would tilt whatever the page
/// drew next.
#[test]
fn a_v_offset_is_a_rise_and_the_rise_is_put_back() {
    let content = drawn(r#"Indices="1,,,30;1" UnicodeString="AA""#);
    assert!(content.contains("30 Ts"), "the offset is a rise: {content}");
    assert!(
        content.contains("0 Ts"),
        "and the rise is put back before the run ends: {content}"
    );
    // The second glyph is back on the baseline and at the pen the first glyph
    // left, which says the rise moved the glyph and not the pen.
    assert_eq!(
        array(&content),
        "[<0001>]",
        "the run breaks where the rise changes"
    );
    assert!(
        content.matches("] TJ").count() == 2,
        "two arrays, one each side of the rise: {content}"
    );
}

/// **A cluster maps `m` code units to `n` glyphs, and both counts matter.**
///
/// Three spellings of one string, and all three differ:
///
/// - `(2:2)3;2` over `"ABC"` — two code units become two glyphs, the first of
///   which stands for `"AB"`, and `"C"` carries on one to one: **three**
///   glyphs;
/// - a build that ignored `ClusterCodeUnitCount` gives the first glyph `"A"`
///   and leaves `"BC"` to carry on: **four** glyphs;
/// - a build that ignored `ClusterGlyphCount` gives the second mapping `"B"`
///   of its own: **two** glyphs.
#[test]
fn a_cluster_maps_m_code_units_to_n_glyphs_and_both_counts_matter() {
    let content = drawn(r#"Indices="(2:2)3;2" UnicodeString="ABA""#);
    assert_eq!(
        array(&content),
        "[<0003><0002><0001>]",
        "three glyphs: the cluster's two and the one code unit left over"
    );

    let to_unicode = to_unicode(&obfuscated(&run(
        r#"Indices="(2:2)3;2" UnicodeString="ABA""#,
    )));
    assert!(
        to_unicode.contains("2 beginbfchar"),
        "two of the three glyphs stand for text: {to_unicode}"
    );
    assert!(
        to_unicode.contains("<0003> <00410042>"),
        "the cluster's whole text is on its first glyph: {to_unicode}"
    );
    assert!(
        !to_unicode.contains("<0002> <"),
        "and its second glyph stands for nothing, so the text is not extracted twice: {to_unicode}"
    );
    assert!(
        to_unicode.contains("<0001> <0041>"),
        "and the code unit past the cluster is its own glyph: {to_unicode}"
    );
}

/// Cluster counts that disagree with `UnicodeString` refuse the run, from
/// **both** sides.
///
/// A cluster claiming more code units than the string holds, and one claiming
/// more glyphs than the mapping list holds, are the same disagreement read
/// from its two ends — and a build that checked one would pass every fixture
/// written for the other.
#[test]
fn cluster_counts_that_do_not_add_up_refuse_the_run() {
    assert_eq!(
        run_defects(r#"Indices="(3:1)3" UnicodeString="AB""#),
        [XpsElementDefect::GlyphsIndicesUnreadable],
        "three code units out of a two-unit string"
    );
    assert_eq!(
        run_defects(r#"Indices="(1:3)3" UnicodeString="A""#),
        [XpsElementDefect::GlyphsIndicesUnreadable],
        "three glyphs out of a one-mapping list"
    );
}

/// **A trailing empty mapping is a separator, and not a ninth glyph out of
/// eight characters.**
///
/// Both halves, because they are two rules: `"1;"` over two characters is two
/// glyphs — the grammar's second mapping is a real one and there is a code
/// unit for it — and `"1;"` over one character is one, because a producer that
/// wrote a separator after its last item did not ask for a glyph there.
#[test]
fn a_trailing_empty_mapping_is_a_separator_and_not_a_glyph() {
    assert_eq!(
        array(&drawn(r#"Indices="1;" UnicodeString="AA""#)),
        "[<0001><0001>]",
        "the second mapping has a code unit and is a glyph"
    );
    assert_eq!(
        array(&drawn(r#"Indices="1;" UnicodeString="A""#)),
        "[<0001>]",
        "and with nothing left for it, it is a separator"
    );
    assert_eq!(
        run_defects(r#"Indices="1;" UnicodeString="A""#),
        [],
        "which is not a defect either"
    );
}

/// A mapping that names no glyph and has no code unit to look one up with is a
/// run that does not add up, and is refused rather than drawn as `.notdef`.
#[test]
fn a_mapping_with_no_index_and_no_code_unit_refuses_the_run() {
    assert_eq!(
        run_defects(r#"Indices="1;,80" UnicodeString="A""#),
        [XpsElementDefect::GlyphsIndicesUnreadable],
        "the second mapping states an advance and no glyph, and there is no `B` to look up"
    );
}

// ---- 12.1.1, `UnicodeString` --------------------------------------------

/// **`UnicodeString` alone**, with the font's own `cmap` doing every lookup.
///
/// The two characters map to two *different* glyphs with two *different*
/// advances, so a build that looked up only the first, or that used one
/// nominal width, is caught by the second.
#[test]
fn a_unicode_string_alone_addresses_glyphs_through_the_fonts_own_cmap() {
    // `A` is glyph 1 at half an em, `B` is glyph 3 at a quarter.
    assert_eq!(
        array(&drawn(r#"UnicodeString="ABA""#)),
        "[<0001><0003><0001>]"
    );
    let to_unicode = to_unicode(&obfuscated(&run(r#"UnicodeString="ABA""#)));
    assert!(to_unicode.contains("<0001> <0041>"), "{to_unicode}");
    assert!(to_unicode.contains("<0003> <0042>"), "{to_unicode}");
}

/// A character the font's `cmap` does not cover becomes glyph 0, which is
/// `.notdef` — the glyph a consumer draws for a character it has none for.
#[test]
fn a_character_the_cmap_does_not_cover_is_notdef() {
    assert_eq!(array(&drawn(r#"UnicodeString="AZ""#)), "[<0001><0000>]");
}

/// XAML's `{}` escape is stripped, so a string that begins with a brace draws
/// the text and not the braces.
///
/// 12.1.1 needs it because an attribute value beginning with `{` is a markup
/// extension — the same rule `{StaticResource}` is read under, one attribute
/// over.
#[test]
fn a_unicode_string_escaped_with_braces_draws_the_text_and_not_the_braces() {
    let to_unicode = to_unicode(&obfuscated(&run(r#"UnicodeString="{}AB""#)));
    assert!(to_unicode.contains("<0001> <0041>"), "{to_unicode}");
    assert!(to_unicode.contains("<0003> <0042>"), "{to_unicode}");
    assert_eq!(
        array(&drawn(r#"UnicodeString="{}AB""#)),
        "[<0001><0003>]",
        "two glyphs, not four"
    );
}

// ---- 12.1's three attributes that are never ignored ---------------------

/// **`IsSideways` is refused by name rather than drawn upright**, and a run
/// that says `false` — or says nothing — draws.
///
/// Both halves, because a build that refused every `IsSideways` attribute
/// would satisfy the first on its own, and every real producer writes the
/// attribute out with its default value.
#[test]
fn is_sideways_is_refused_by_name_rather_than_drawn_upright() {
    assert_eq!(
        run_defects(r#"IsSideways="true" UnicodeString="A""#),
        [XpsElementDefect::GlyphsSidewaysUnsupported]
    );
    assert!(
        !drawn(r#"IsSideways="true" UnicodeString="A""#).contains("TJ"),
        "and nothing is drawn"
    );
    assert_eq!(run_defects(r#"IsSideways="false" UnicodeString="A""#), []);
    assert_eq!(
        array(&drawn(r#"IsSideways="false" UnicodeString="A""#)),
        "[<0001>]"
    );
    assert_eq!(
        run_defects(r#"IsSideways="sideways" UnicodeString="A""#),
        [XpsElementDefect::GlyphsUnreadable],
        "and a value that is not a boolean is a different thing about the file"
    );
}

/// **An odd `BidiLevel` is refused by name and an even one draws.**
///
/// 12.1 makes an odd level a right-to-left run, whose origin is its *right*
/// edge — so drawing it left to right is the same glyphs somewhere else, which
/// is the wrong-place failure gap 30's refusal asymmetry exists for. An even
/// level is a left-to-right run at some embedding depth and is ordinary text,
/// so refusing every non-zero level would refuse text this build can draw
/// exactly.
#[test]
fn an_odd_bidi_level_is_refused_by_name_and_an_even_one_draws() {
    for level in ["0", "2", "4"] {
        let body = format!(r#"BidiLevel="{level}" UnicodeString="A""#);
        assert_eq!(run_defects(&body), [], "level {level} is left to right");
        assert_eq!(array(&drawn(&body)), "[<0001>]", "level {level}");
    }
    for level in ["1", "3"] {
        let body = format!(r#"BidiLevel="{level}" UnicodeString="A""#);
        assert_eq!(
            run_defects(&body),
            [XpsElementDefect::GlyphsBidiUnsupported],
            "level {level} is right to left"
        );
        assert!(!drawn(&body).contains("TJ"), "level {level} draws nothing");
    }
    assert_eq!(
        run_defects(r#"BidiLevel="rtl" UnicodeString="A""#),
        [XpsElementDefect::GlyphsUnreadable]
    );
}

/// **A `StyleSimulations` is named and the run still draws**, which is the one
/// of 12.1's three that is not a refusal.
///
/// The glyphs, their widths and the text they stand for are all exactly what
/// the file says; only the synthetic slant or weight is missing. That is the
/// *paint* side of gap 30's asymmetry — the shape and its place are known and
/// only its appearance is approximate — and dropping a page of text to avoid
/// drawing it unslanted would lose far more than it saved.
#[test]
fn a_style_simulation_is_named_and_the_run_still_draws() {
    for simulation in ["ItalicSimulation", "BoldSimulation", "BoldItalicSimulation"] {
        let body = format!(r#"StyleSimulations="{simulation}" UnicodeString="A""#);
        assert_eq!(
            run_defects(&body),
            [XpsElementDefect::GlyphsStyleSimulated],
            "{simulation}"
        );
        assert_eq!(
            array(&drawn(&body)),
            "[<0001>]",
            "{simulation}: the run still draws"
        );
    }
    assert_eq!(
        run_defects(r#"StyleSimulations="None" UnicodeString="A""#),
        [],
        "and the default says nothing"
    );
}

/// A `Glyphs` that states no usable origin or em size is not painted, and each
/// of the three required numbers is required on its own.
#[test]
fn a_glyphs_with_no_usable_origin_or_em_size_is_named_and_not_painted() {
    let bare = |extra: &str| {
        format!(
            r##"<Glyphs FontUri="{}" Fill="#000000" UnicodeString="A" {extra} />"##,
            font_uri("odttf")
        )
    };
    for extra in [
        r#"OriginY="100" FontRenderingEmSize="100""#,
        r#"OriginX="10" FontRenderingEmSize="100""#,
        r#"OriginX="10" OriginY="100""#,
        r#"OriginX="ten" OriginY="100" FontRenderingEmSize="100""#,
        r#"OriginX="10" OriginY="100" FontRenderingEmSize="-4""#,
    ] {
        assert_eq!(
            defects(&obfuscated(&bare(extra))),
            [XpsElementDefect::GlyphsUnreadable],
            "{extra}"
        );
    }
    // A `FontRenderingEmSize` of zero is invisible by arithmetic and is not a
    // defect: 12.1 permits it and a warning would be about the file rather
    // than about this build.
    let zero = bare(r#"OriginX="10" OriginY="100" FontRenderingEmSize="0""#);
    assert_eq!(defects(&obfuscated(&zero)), []);
    assert!(!stream(&open(&obfuscated(&zero)).expect("an XPS")).contains("TJ"));
}

// ---- the whole reason milestone 5 came first ----------------------------

/// **`Indices` names a glyph the font's `cmap` would never reach**, and a
/// fixture whose `cmap` and WinAnsi disagree is what proves it.
///
/// This is gap 30's own sentence about why the writer's missing half was built
/// before anything read a `Glyphs` run: *"the obvious fallback — write
/// `UnicodeString` through WinAnsi and drop `Indices` — produces text that is
/// readable, plausible and **wrong** exactly when the font's cmap and WinAnsi
/// disagree, and correct on every Latin fixture anybody would write."*
///
/// The fixture's `cmap` maps `A` to glyph 1, which paints the bottom-left
/// quarter of the em. Glyph 2 paints the top-right. `Indices="2"` over
/// `UnicodeString="A"` must paint the **top-right**, and the two are told
/// apart at both corners in both directions — because a build that drew
/// nothing at all would satisfy either half on its own.
#[test]
fn indices_names_a_glyph_the_cmap_would_never_reach() {
    // The em is a hundred units at origin (10, 100), so glyph 1's square is
    // 0 to 30 units right of the origin and 0 to 30 above the baseline, and
    // glyph 2's is 40 to 70 in both.
    let low = (25.0, 85.0);
    let high = (65.0, 45.0);

    let by_index = render(&obfuscated(&run(r#"Indices="2" UnicodeString="A""#)));
    assert!(
        dark(ink(&by_index, high.0, high.1)),
        "the run drew glyph 2, in the top-right of the em: {:?}",
        ink(&by_index, high.0, high.1)
    );
    assert!(
        !dark(ink(&by_index, low.0, low.1)),
        "and not glyph 1, which is what its cmap would have given: {:?}",
        ink(&by_index, low.0, low.1)
    );

    let by_character = render(&obfuscated(&run(r#"UnicodeString="A""#)));
    assert!(
        dark(ink(&by_character, low.0, low.1)),
        "a run with no `Indices` drew what the cmap says `A` is: {:?}",
        ink(&by_character, low.0, low.1)
    );
    assert!(
        !dark(ink(&by_character, high.0, high.1)),
        "and could not have reached glyph 2 at all: {:?}",
        ink(&by_character, high.0, high.1)
    );

    // And the glyph the run named is the code in the string, which is what
    // `/Identity-H` over `/CIDToGIDMap /Identity` makes possible and what a
    // simple font has no spelling for at all.
    assert_eq!(
        array(&drawn(r#"Indices="2" UnicodeString="A""#)),
        "[<0002>]"
    );
}

/// **The run is not drawn upside down**, which is the most plausible wrong
/// picture in this milestone.
///
/// 18.1's flip lives in the page's one `cm`, so user space under it has y
/// increasing *downward*. A text matrix that did not undo it would draw every
/// glyph mirrored about its own baseline — at exactly the right place, at
/// exactly the right size, in exactly the right colour.
///
/// Glyph 1's square sits **above** the baseline in the font, so it must land
/// above the baseline on the page: ink thirty units up, none thirty down.
#[test]
fn a_run_is_not_drawn_upside_down() {
    let page = render(&obfuscated(&run(r#"UnicodeString="A""#)));
    assert!(
        dark(ink(&page, 25.0, 85.0)),
        "the glyph is above the baseline: {:?}",
        ink(&page, 25.0, 85.0)
    );
    assert!(
        !dark(ink(&page, 25.0, 115.0)),
        "and not below it, which is what an unflipped text matrix draws: {:?}",
        ink(&page, 25.0, 115.0)
    );
}

/// **`/W` comes from the font's own `hmtx` for the glyphs the run drew**, and
/// the run's own positions agree with it.
///
/// Two independent statements about one font, and the second is what a reader
/// uses: the `TJ` array is computed from the same numbers `/W` publishes, so a
/// build whose two halves disagreed would move every glyph after the first.
#[test]
fn the_width_array_is_the_fonts_own_hmtx_for_the_glyphs_the_run_drew() {
    let widths = widths(&obfuscated(&run(r#"Indices="1;2;3" UnicodeString="AAA""#)));
    assert!(
        widths.contains("500") && widths.contains("1000") && widths.contains("250"),
        "the three advances the font states, in thousandths: {widths}"
    );
}

// ---- the test that says synthesis paid for text extraction --------------

/// **`Page::text()` returns the `UnicodeString`, through the `/ToUnicode`.**
///
/// Row 7's last clause, on a package this repository did not write.
/// Extraction drives a *second* device through the same content stream, so a
/// writer half the text extractor disagrees with is caught here and nowhere
/// else — and the `Indices=",53"` in that file means the extracted text and
/// the drawn glyphs come from two different attributes.
#[test]
fn page_text_comes_back_from_the_unicode_string_through_the_to_unicode() {
    for name in ["wpf-image-and-text.xps", "xpsom-image-and-text.oxps"] {
        let document = Document::open(corpus(name)).expect("a fixed document");
        let text = document.page(0).expect("a page").text().plain_text();
        assert!(
            text.contains("Page one"),
            "{name}: the run's own `UnicodeString`, got {text:?}"
        );
    }
}

/// The run in the real package draws the glyphs Cascadia Mono's `cmap` names
/// and honours the one advance the file states.
///
/// `Indices=",53"` is 12.1.3's empty `GlyphIndex` followed by an advance, for
/// the **first** of eight characters — so `P` is glyph 146 with an advance of
/// 53 hundredths of the em and the other seven are the font's own. The `TJ`
/// adjustment after the first glyph is the difference, in thousandths.
#[test]
fn the_real_run_draws_the_cmaps_glyphs_and_the_files_own_first_advance() {
    let document = Document::open(corpus("wpf-image-and-text.xps")).expect("a fixed document");
    let content = stream(&document);
    // 'P' is glyph 146 (0x92), 'a' 222 (0xDE), 'g' 284 (0x11C), 'e' 260
    // (0x104), the space 861 (0x35D), 'o' 345 (0x159), 'n' 336 (0x150).
    for code in [
        "<0092>", "<00DE>", "<011C>", "<0104>", "<035D>", "<0159>", "<0150>",
    ] {
        assert!(content.contains(code), "{code} in {content}");
    }
    assert!(
        content.contains("BT /XF0 24 Tf 1 0 0 -1 100 400 Tm"),
        "the markup's own `FontRenderingEmSize`, `OriginX` and `OriginY`: {content}"
    );
    // The font's own advance is 1200/2048 em, which `/W` rounds to 586
    // thousandths; the file states 53 hundredths. At an em of 24 that is
    // 14.064 against 12.72, and the adjustment is the difference in
    // thousandths of the em: 1000 x 1.344 / 24 = 56.
    assert!(
        content.contains("<0092> 56 <00DE>"),
        "the stated advance moved the pen by exactly the difference: {content}"
    );
}

/// Both dialects of the same document draw the same run.
///
/// The OpenXPS twin writes its `FontUri` as `../../../Resources/…` where XPS
/// 1.0 writes `/Resources/…`, so this is also the one assertion that says
/// 9.1.7's reference resolves against the **fixed page part's own name**.
#[test]
fn both_dialects_resolve_the_same_font_and_draw_the_same_run() {
    let xps = stream(&Document::open(corpus("wpf-image-and-text.xps")).expect("a document"));
    let oxps = stream(&Document::open(corpus("xpsom-image-and-text.oxps")).expect("a document"));
    let run_of = |content: &str| {
        let at = content.find("BT ").expect("a run");
        content[at..].to_string()
    };
    assert_eq!(
        run_of(&xps),
        run_of(&oxps),
        "one relative reference and one absolute, and one font"
    );
}

// ---- the element rules, on a `Glyphs` -----------------------------------

/// A gradient over text takes the placeholder grey and is named, which is the
/// answer a gradient **stroke** already gets and for the same reason: a
/// shading pattern is what one needs, and gap 30's milestone 5 records that it
/// writes none.
#[test]
fn a_gradient_over_text_is_the_placeholder_grey_and_named() {
    let body = format!(
        r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="{}" UnicodeString="A"><Glyphs.Fill><LinearGradientBrush StartPoint="0,0" EndPoint="100,0"><LinearGradientBrush.GradientStops><GradientStop Color="#FF0000" Offset="0" /><GradientStop Color="#0000FF" Offset="1" /></LinearGradientBrush.GradientStops></LinearGradientBrush></Glyphs.Fill></Glyphs>"##,
        font_uri("odttf")
    );
    assert_eq!(
        defects(&obfuscated(&body)),
        [XpsElementDefect::BrushUnsupported]
    );
    let content = stream(&open(&obfuscated(&body)).expect("an XPS"));
    assert!(
        content.contains("0.749 0.749 0.749 rg"),
        "the placeholder grey rather than the gradient's first stop: {content}"
    );
    assert!(
        content.contains("<0001>"),
        "and the text still drew: {content}"
    );
}

/// A `{StaticResource}` naming nothing paints the run grey and says so, which
/// is the brush rule reaching text unchanged.
#[test]
fn a_static_resource_fill_that_names_nothing_paints_the_run_grey() {
    let body = format!(
        r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="{}" Fill="{{StaticResource nowhere}}" UnicodeString="A" />"##,
        font_uri("odttf")
    );
    assert_eq!(
        defects(&obfuscated(&body)),
        [XpsElementDefect::BrushUnresolved]
    );
    assert!(stream(&open(&obfuscated(&body)).expect("an XPS")).contains("0.749 0.749 0.749 rg"));
}

/// A transform that is not six numbers refuses the run, and a `Clip` that will
/// not read refuses it too — the element rules gap 30's design section fixes,
/// applied to a `Glyphs` rather than to a `Path`.
#[test]
fn a_transform_or_a_clip_that_will_not_read_refuses_the_run() {
    assert_eq!(
        run_defects(r#"RenderTransform="1,0,0,1,5" UnicodeString="A""#),
        [XpsElementDefect::TransformUnreadable]
    );
    assert!(!drawn(r#"RenderTransform="1,0,0,1,5" UnicodeString="A""#).contains("TJ"));

    assert_eq!(
        run_defects(r#"Clip="M0,0Lnonsense" UnicodeString="A""#),
        [XpsElementDefect::ClipUnreadable]
    );
    assert_eq!(
        run_defects(r#"Opacity="2" UnicodeString="A""#),
        [XpsElementDefect::OpacityUnreadable]
    );
    assert_eq!(
        run_defects(r##"OpacityMask="#FF000000" UnicodeString="A""##),
        [XpsElementDefect::OpacityMaskUnsupported]
    );

    // And a transform that *does* read reaches the page as its own six
    // numbers, which is the other half of the pair.
    let content = drawn(r#"RenderTransform="1,0,0,1,5,7" UnicodeString="A""#);
    assert!(content.contains("1 0 0 1 5 7 cm"), "{content}");
}

/// An element `Opacity` and a brush's own alpha are one `/ExtGState` between
/// them, multiplied — 14.3's opacity is a factor and not a third alpha.
#[test]
fn an_element_opacity_multiplies_the_brushs_own_alpha() {
    let body = format!(
        r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="{}" Fill="#80000000" Opacity="0.5" UnicodeString="A" />"##,
        font_uri("odttf")
    );
    let content = stream(&open(&obfuscated(&body)).expect("an XPS"));
    assert!(content.contains(" gs"), "an /ExtGState is named: {content}");
    let document = open(&obfuscated(&body)).expect("an XPS");
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    let resources = page.resources.as_ref().expect("resources");
    let states = cos.resolve_key(resources, cos.intern(b"ExtGState"));
    let states = states.as_dict().expect("an /ExtGState sub-dictionary");
    let state = cos.resolve_key(states, cos.intern(b"GS0"));
    let state = state.as_dict().expect("a graphics state");
    // 128/255 x 0.5, which is neither number on its own.
    let alpha = cos.resolve_key(state, cos.intern(b"ca")).as_number();
    assert!(
        alpha.is_some_and(|a: f64| (a - (128.0 / 255.0) * 0.5).abs() < 1.0e-6),
        "the two multiply: {alpha:?}"
    );
}

// ---- the bound ----------------------------------------------------------

/// **`MAX_XPS_GLYPHS` fires by its own refusal**, at the shipped value,
/// against a real run built past it — and a run one glyph short opens.
///
/// A total rather than a per-run cap, for the reason the segment total is one:
/// `1;` is two bytes and one more glyph, so a single `Indices` attribute in a
/// part this build already admits holds a hundred million of them, and neither
/// the element count nor the segment count sees a single one. A `Glyphs` is
/// **one** element.
///
/// The pair is what says the cap is what stopped it rather than something else
/// about a large package.
#[test]
fn a_run_past_the_glyph_cap_is_refused_by_name() {
    let package_of = |count: usize| {
        let indices = "1;".repeat(count - 1) + "1";
        obfuscated(&run(&format!(r#"Indices="{indices}""#)))
    };

    let under = package_of(tinker_pdf::xps::MAX_XPS_GLYPHS);
    let opened = open(&under).expect("a run of exactly the cap is one this build draws");
    // And it **drew**: an `Ok` on its own would be satisfied by a placeholder
    // page, which is what a package refused one level down produces.
    assert_eq!(
        opened
            .archive()
            .expect("a synthesised document")
            .warnings()
            .len(),
        0,
        "nothing was degraded"
    );
    assert!(
        stream(&opened).contains("] TJ"),
        "and the run reached the page"
    );

    let over = package_of(tinker_pdf::xps::MAX_XPS_GLYPHS + 1);
    assert!(
        matches!(
            open(&over),
            Err(tinker_pdf::OpenError::UnsupportedArchive(
                ArchiveRefusal::TooLarge
            ))
        ),
        "one more is refused by name"
    );
}

/// The glyph total is a **document** total, so two pages of half the cap each
/// spend it between them.
///
/// A per-page cap set to the same number would never fire on this package, and
/// that is exactly `5adf502`'s finding — a per-item cap is not a total once
/// the file chooses how many items there are.
#[test]
fn the_glyph_total_is_a_total_and_not_a_per_run_cap() {
    let half = tinker_pdf::xps::MAX_XPS_GLYPHS / 2 + 1;
    let indices = "1;".repeat(half - 1) + "1";
    let body = format!(
        "{}{}",
        run(&format!(r#"Indices="{indices}""#)),
        run(&format!(r#"Indices="{indices}""#))
    );
    let over = obfuscated(&body);
    assert!(
        matches!(
            open(&over),
            Err(tinker_pdf::OpenError::UnsupportedArchive(
                ArchiveRefusal::TooLarge
            ))
        ),
        "two runs of half the cap each are past it together"
    );
}

// ---- the seven the injection matrix found ------------------------------

/// **A `FontUri` resolves against the fixed page part, not the package root.**
///
/// *A survivor.* The only relative `FontUri` in eight real packages is
/// OpenXPS's `../../../Resources/….ODTTF` on a page at
/// `/Documents/1/Pages/1.fpage` — which climbs **exactly** to the package root,
/// so resolving it against the root and against the page give the same answer
/// and `both_dialects_resolve_the_same_font_and_draw_the_same_run` cannot tell
/// the two apart. No producer will ever write the fixture that can.
///
/// Both directions, because a base is two rules: a reference relative to the
/// page **finds** a sibling part, and the same reference does **not** find the
/// part of that name at the package root.
#[test]
fn a_font_uri_resolves_against_the_fixed_page_part_and_not_the_package_root() {
    let program = obfuscate(FONT_GUID, &disagreeing_font());
    let beside = format!("Documents/1/Pages/{FONT_GUID}.odttf");

    let build = |uri: &str, at: &str| {
        let body = format!(
            r##"<Glyphs OriginX="10" OriginY="100" FontRenderingEmSize="100" FontUri="{uri}" Fill="#000000" UnicodeString="A" />"##
        );
        let markup =
            format!(r#"<FixedPage xmlns="{XPS_NS}" Width="200" Height="200">{body}</FixedPage>"#);
        let parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &markup);
        let parts = before_content_types(parts, binary_part(at, program.clone()));
        let types = content_types_with(&format!(
            r#"<Default Extension="odttf" ContentType="{OBFUSCATED_FONT_MEDIA_TYPE}" />"#
        ));
        archive(with(parts, "[Content_Types].xml", &types))
    };

    // The part sits beside the page, and the reference is relative to it.
    let sibling = build(&format!("{FONT_GUID}.odttf"), &beside);
    assert_eq!(defects(&sibling), [], "a sibling part resolves");
    assert!(stream(&open(&sibling).expect("an XPS")).contains("<0001>"));

    // The same reference, with the part at the package root instead: it must
    // **not** be found, because the page is three segments down.
    let at_root = build(&format!("{FONT_GUID}.odttf"), &font_part_name("odttf"));
    assert_eq!(
        defects(&at_root),
        [XpsElementDefect::GlyphsFontUnresolved],
        "a page-relative reference does not reach the package root"
    );
}

/// **A positive `vOffset` moves the glyph up the page.**
///
/// *A survivor.* `a_v_offset_is_a_rise_and_the_rise_is_put_back` asserts the
/// stream contains `30 Ts`, and `-30 Ts` contains that substring — so the
/// *direction* of 12.1.3's offset was asserted by nothing at all. The answer is
/// not a tighter string match: it is the picture, because "up the page" is what
/// the clause says and a rendered pixel is the only reading of it that cannot
/// be satisfied by a sign.
///
/// Glyph 1's square occupies the thirty units above the baseline, so a rise of
/// thirty puts it in the thirty above *that* — and a build with the sign the
/// other way puts it thirty **below** the baseline, where the third assertion
/// looks.
#[test]
fn a_v_offset_moves_the_glyph_up_the_page() {
    let risen = render(&obfuscated(&run(r#"Indices="1,,,30" UnicodeString="A""#)));
    assert!(
        dark(ink(&risen, 25.0, 55.0)),
        "thirty units above where the glyph would sit: {:?}",
        ink(&risen, 25.0, 55.0)
    );
    assert!(
        !dark(ink(&risen, 25.0, 85.0)),
        "and not where it would have sat without the offset: {:?}",
        ink(&risen, 25.0, 85.0)
    );
    assert!(
        !dark(ink(&risen, 25.0, 115.0)),
        "and not thirty units below, which is the other sign: {:?}",
        ink(&risen, 25.0, 115.0)
    );
}

/// **The glyph total is charged in two places, and a run reaches it through
/// `UnicodeString` alone.**
///
/// *A survivor.* `a_run_past_the_glyph_cap_is_refused_by_name` states
/// `Indices` and no `UnicodeString`, so it drives only the charge taken before
/// the mappings are parsed. The other charge — one per code unit the mappings
/// did not reach — is a second, independent statement, and a `Glyphs` carrying
/// two megabytes of `UnicodeString` and no `Indices` at all reaches it and
/// nothing else.
#[test]
fn a_run_past_the_glyph_cap_through_its_unicode_string_is_refused_by_name() {
    let string = "A".repeat(tinker_pdf::xps::MAX_XPS_GLYPHS + 1);
    let over = obfuscated(&run(&format!(r#"UnicodeString="{string}""#)));
    assert!(
        matches!(
            open(&over),
            Err(tinker_pdf::OpenError::UnsupportedArchive(
                ArchiveRefusal::TooLarge
            ))
        ),
        "a string past the cap is refused by name"
    );
}

/// **A run contributes its box to the canvas it sits in.**
///
/// *A survivor.* 14.3's opacity is a transparency group only where two of a
/// canvas's children cover the same place, and the overlap test reads the boxes
/// its children pushed. A `Glyphs` that pushed none leaves a canvas holding one
/// box, which never overlaps anything — so a canvas of a `Path` and a run drew
/// without a group and no test in this repository looked.
///
/// Both halves, for the reason milestone 6 built its own pair: a suite that
/// only ever overlapped would pass with the group deleted.
#[test]
fn a_run_contributes_its_box_to_its_canvass_overlap_test() {
    let canvas = |glyphs: &str| {
        let body = format!(
            r##"<Canvas Opacity="0.5"><Path Fill="#FF0000" Data="M0,0L60,0 60,60 0,60Z" />{glyphs}</Canvas>"##
        );
        obfuscated(&body)
    };

    // The run's box is its advance and one em above the baseline, so an origin
    // at (10, 50) with an em of 40 covers y from 10 to 50 — over the path.
    let overlapping = canvas(&format!(
        r##"<Glyphs OriginX="10" OriginY="50" FontRenderingEmSize="40" FontUri="{}" Fill="#000000" UnicodeString="A" />"##,
        font_uri("odttf")
    ));
    let document = open(&overlapping).expect("an XPS");
    let content = stream(&document);
    assert!(
        content.contains(" Do\n"),
        "the canvas is a transparency group: {content}"
    );
    let cos = document.cos();
    let raw = String::from_utf8_lossy(cos.bytes()).into_owned();
    assert!(
        raw.contains("/Transparency"),
        "and the form carries a /Group"
    );

    // The same canvas with the run well clear of the path needs no group at
    // all, which is what says the first assertion is about the overlap rather
    // than about a run being present.
    let apart = canvas(&format!(
        r##"<Glyphs OriginX="120" OriginY="180" FontRenderingEmSize="40" FontUri="{}" Fill="#000000" UnicodeString="A" />"##,
        font_uri("odttf")
    ));
    assert!(
        !stream(&open(&apart).expect("an XPS")).contains(" Do\n"),
        "no form XObject is needed"
    );
}

/// **A `Clip` that will not read refuses the run, and a `Data` that will not
/// read is a different rule.**
///
/// *A survivor.* `a_transform_or_a_clip_that_will_not_read_refuses_the_run`
/// asserted the warning's **name** and not its **consequence**, so a build that
/// warned and then drew the run unclipped passed it. That is milestone 6's own
/// finding — one parser, two answers — arriving on a `Glyphs`: an unreadable
/// clip that is ignored draws every glyph the clip existed to hide.
#[test]
fn a_clip_that_will_not_read_leaves_the_run_undrawn() {
    let content = drawn(r#"Clip="M0,0Lnonsense" UnicodeString="A""#);
    assert!(
        !content.contains(" TJ") && !content.contains("<0001>"),
        "nothing of the run reached the page: {content}"
    );
    // And a clip that **does** read leaves the run drawn, inside it — the
    // other half, because a build that drew nothing whatever the clip said
    // would pass the assertion above on its own.
    let clipped = drawn(r#"Clip="M0,0L200,0 200,200 0,200Z" UnicodeString="A""#);
    assert!(clipped.contains("<0001>"), "{clipped}");
    assert!(
        clipped.contains("W* n"),
        "and the clip is applied, in 11.2.1's default even-odd rule: {clipped}"
    );
}
