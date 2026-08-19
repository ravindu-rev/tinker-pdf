//! Gap 29's seven bounds, gap 30's and gap 31's, swept in one place.
//!
//! *Amended, 19 August 2026, gap 31 milestone 3.* One more row —
//! `MAX_OCF_PATH_LEN` — and it is the first here whose number is a
//! **specification's own**: OCF 3.3 §4.2.3 says a content path must not exceed
//! 65 535 bytes, so this cap is taken rather than invented. That makes
//! [`every_bound_can_fire`]'s check the interesting one: the ceiling in front
//! of it is an XML attribute value in a ZIP entry, 128 MiB, so the spec's own
//! figure sits two thousand times below what an attacker may ask for and the
//! cap is a cap.
//!
//! Two constants that could have joined it did not, and both absences are
//! argued where they are declared rather than here: nothing on container-path
//! **depth**, because nothing in this engine touches a filesystem so depth
//! bounds no allocation and no recursion; and nothing on a **segment**'s
//! length, because §4.2.3's 255-byte file-name limit is an interoperability
//! rule about somebody else's file system and enforcing it would refuse a path
//! that names an entry the container actually holds.
//!
//! *Amended, 19 August 2026, gap 30 milestone 9.* **No new row, and that is the
//! point of this amendment: what it adds is the missing half of the rows that
//! were already here.** Gap 30's yardstick — a 200-page fixed document at
//! roughly 2 000 drawable elements and 40 000 path segments a page — arrived in
//! milestone 2 with a figure for gap 30's own rows and `None` for gap 29's
//! seven, and milestone 2 wrote down that milestone 9 would fill them in. It
//! has, so [`Bound::document`] is no longer an `Option` and
//! [`no_bound_refuses_a_dense_fixed_document`] sweeps **seventeen** rows rather
//! than ten. Row 9 asks for *"three recorded numbers each"* and a row with a
//! `None` in it had two.
//!
//! Filling them in is not bookkeeping. Four of the seven are gap 29's ZIP
//! caps, and **every XPS part in this repository is a ZIP entry** — so those
//! four stand in front of the whole of gap 30 and had never been measured
//! against anything gap 30 produces. `MAX_PNG_SAMPLES` is the row that shows
//! why it was worth doing: gap 29's yardstick spends 24 000 000 against it and
//! gap 30's spends 33 660 000, because a 300 dpi RGBA page scan in a report is
//! a larger image than any page of a comic, and the margin under the cap is
//! half what the comic suggested.
//!
//! `MAX_XPS_VISUAL_DEPTH` is in gap 30's bounds table and is **not** here, and
//! the argument is milestone 8's rather than an omission: `VisualBrush` is
//! refused by name, so a cap over a walk nothing performs is a constant that
//! could never fire — [`every_bound_can_fire`]'s own failure, reached from the
//! direction that writes the constant first. The bound arrives with the walk or
//! not at all.
//!
//! *Amended, 18 August 2026, gap 30 milestone 7.* One more row —
//! `MAX_XPS_GLYPHS` — and it is here rather than absent because a `Glyphs` is
//! **one** element and **no** segments, so neither work cap milestone 6 added
//! sees a single glyph. `1;` is two bytes and one more glyph mapping, and one
//! mapping is eighty bytes of value, so a single `Indices` attribute in a part
//! this build already admits is a hundred million of them. It is charged
//! *before* the mappings are materialised for that reason, which is
//! `tinker-pdf-zip`'s own posture: a permit is what has been promised.
//!
//! *Amended, 18 August 2026, gap 30 milestone 6.* Three more rows —
//! `MAX_XPS_ELEMENTS`, `MAX_XPS_SEGMENTS` and `MAX_XPS_RESOURCE_DEPTH`. The
//! first two are the plan's own **work caps**, the numbers a file chooses when
//! it decides how much markup a page holds, and milestone 5 recorded that they
//! are *"where a file-chosen count is refused"*. The third arrives with them
//! rather than later because row 6's `{StaticResource}` criterion asks for *"a
//! depth cap and a cycle refused rather than recursed"*, and those are two
//! rules with two names in the report — a chain of twenty distinct keys is not
//! a cycle and a cycle of two is not deep.
//!
//! *Amended, 18 August 2026, gap 30 milestone 3.* Two more rows —
//! `MAX_XPS_PARTS` and `MAX_XPS_PAGES` — and the relation
//! `MAX_XPS_PAGES < MAX_XPS_PARTS < MAX_ZIP_ENTRIES`, written in a `const`
//! block beside the constants so a build that breaks it **does not compile**.
//! The pair is worth having as a pair: two hundred `PageContent` elements may
//! name one part between them, so the page count is not bounded by the part
//! count and neither cap could stand in for the other.
//!
//! *Amended, 18 August 2026, gap 30 milestone 2.* Four more rows —
//! `tinker-pdf-xml`'s — and one more relation. Gap 30's bounds section says the
//! new constants join **this** table rather than getting a sweep of their own,
//! *"because the whole value of it is that it is one table"*, and they inherit
//! all five checks below unchanged. What they add is a second yardstick: gap
//! 29's is a 200-page comic and gap 30's is a 200-page fixed document at
//! roughly 2 000 drawable elements and 40 000 path segments a page, and a bound
//! that refuses either is a missing feature wearing a `MAX_` prefix. The
//! yardstick is `Option`al per row because gap 29's seven have no figure for it
//! yet; gap 30's milestone 9 fills those in when it adds the rest of its own
//! table.
//!
//! Each of the four modules this gap added carries its own ledger beside its
//! own constants — `tinker-pdf-zip`'s `limits.rs`, `png.rs`'s module note,
//! `cbz.rs`'s bounds section — and each proves its own caps fire in its own
//! tests. What none of them can do from where it stands is check the **set**:
//! that every bound the plan's table names has a ledger at all, that the number
//! in the ledger is the number in the code, and that the cap sits below what
//! its own inputs can ask for.
//!
//! That last one is the whole reason this file exists. Gap 18a's milestone 8
//! found `MAX_JPX_WORK` set *above* the most its own inputs could reach, so it
//! could never fire; nothing failed, because a cap that cannot fire behaves
//! exactly like a cap that is never approached. The check that would have
//! caught it is arithmetic — compare the constant against the ceiling of what
//! stands in front of it — and it is done here for all seven at once.
//!
//! # What is asserted, and what is only recorded
//!
//! Asserted: the constant equals the number its ledger publishes; the cap is
//! **reachable**, so it can fire; a 200-page 2000 x 3000 comic fits under it,
//! so it does not refuse the thing the format is for; and the test named as
//! proving each one fires **exists**, in the file this table says it is in.
//!
//! Recorded but not asserted here: what each of those tests does. They are
//! read by name, not by behaviour, because a test in another crate is not
//! something this one can run — the discipline is that the name is checked, so
//! a ledger citing a test that was renamed or deleted fails here rather than
//! quietly becoming prose.
//!
//! # Why `fixtures <= comic` is not one of the relations
//!
//! It holds for six of the seven and cannot hold for the seventh.
//! `MAX_CBZ_PAGES` is proved to fire by building the real 4 097-entry archive
//! rather than by lowering the constant, so the most any fixture in this
//! repository spends against *that* bound is the cap itself — far more than a
//! comic. A relation that six rows satisfy and one does not is not a relation.

use tinker_pdf::cbz::{zip_limits, MAX_CBZ_PAGES, MAX_SYNTHESISED_PDF, PAGE_OVERHEAD};
use tinker_pdf::epub::MAX_OCF_PATH_LEN;
use tinker_pdf::xps::{
    MAX_XPS_ELEMENTS, MAX_XPS_GLYPHS, MAX_XPS_PAGES, MAX_XPS_PARTS, MAX_XPS_RESOURCE_DEPTH,
    MAX_XPS_SEGMENTS,
};
use tinker_pdf_filters::MAX_PNG_SAMPLES;
use tinker_pdf_xml::limits as xml_limits;

/// The sources the ledgers live in, so a number here can be checked against
/// the number written beside the constant rather than against a memory of it.
const ZIP_LIMITS: &str = include_str!("../../tinker-pdf-zip/src/limits.rs");
const ZIP_TESTS: &str = include_str!("../../tinker-pdf-zip/src/tests.rs");
const PNG: &str = include_str!("../../tinker-pdf-filters/src/png.rs");
const PNG_TESTS: &str = include_str!("../../tinker-pdf-filters/src/png/tests.rs");
const CBZ: &str = include_str!("../src/cbz.rs");
const CBZ_TESTS: &str = include_str!("cbz.rs");
const XML_LIMITS: &str = include_str!("../../tinker-pdf-xml/src/limits.rs");
const XML_TESTS: &str = include_str!("../../tinker-pdf-xml/src/tests.rs");
const EPUB: &str = include_str!("../src/epub.rs");
const EPUB_TESTS: &str = include_str!("epub_ocf.rs");
const XPS: &str = include_str!("../src/xps.rs");
const XPS_TESTS: &str = include_str!("xps_opc.rs");
const XPS_MARKUP_TESTS: &str = include_str!("xps_markup.rs");
const XPS_GLYPH_TESTS: &str = include_str!("xps_glyphs.rs");

/// One bound, as its own ledger publishes it.
struct Bound {
    /// The constant's name, as written in the code.
    name: &'static str,
    /// Its value, read from the crate that declares it.
    cap: u128,
    /// The cap as its ledger's third row writes it, digit groups and all.
    published: &'static str,
    /// The most any fixture in this repository spends against it.
    fixtures: u128,
    /// The most gap 29's yardstick spends: a 200-page comic at 2000 x 3000.
    comic: u128,
    /// The most gap 30's yardstick spends: a 200-page fixed document at
    /// roughly 2 000 drawable elements and 40 000 path segments a page.
    ///
    /// Not an `Option` since milestone 9. It was one for seven milestones,
    /// because gap 29's rows arrived before gap 30 had a yardstick to measure
    /// them with — and a row that opts out of a check is a row that is not
    /// checked, which is the shape of every failure this file exists for.
    document: u128,
    /// The most this bound's own inputs can ask for, which must exceed the cap
    /// or the cap can never fire.
    reachable: u128,
    /// Why that is the ceiling in front of it.
    reachable_because: &'static str,
    /// The source the constant and its ledger live in.
    declared_in: &'static str,
    /// The test that proves it fires by its own refusal or warning, and the
    /// source that test lives in.
    fires_in: (&'static str, &'static str),
}

/// 4 GiB: a ZIP's central-directory offsets are 32-bit without Zip64, so this
/// is how large an ordinary archive can be.
const ARCHIVE_CEILING: u128 = 1 << 32;

fn ledger() -> Vec<Bound> {
    vec![
        Bound {
            name: "MAX_ZIP_ENTRIES",
            cap: zip_limits::MAX_ZIP_ENTRIES as u128,
            published: "16 384",
            fixtures: 6,
            comic: 202,
            // An OPC package of the yardstick's shape: the package
            // relationships part and `[Content_Types].xml`, one
            // `FixedDocumentSequence`, one `FixedDocument`, two hundred fixed
            // pages with a `_rels` part each, and about a hundred fonts and
            // images between them. The same 505 `MAX_XPS_PARTS` counts, because
            // in this format a part *is* an entry.
            document: 505,
            // A central directory record is 46 bytes plus a name, so a name of
            // one byte is the densest an archive can be.
            reachable: ARCHIVE_CEILING / 47,
            reachable_because: "47-byte central directory records in a 4 GiB archive",
            declared_in: ZIP_LIMITS,
            fires_in: (
                "an_archive_with_more_entries_than_the_cap_is_refused_by_name",
                ZIP_TESTS,
            ),
        },
        Bound {
            name: "MAX_ZIP_ENTRY_BYTES",
            cap: zip_limits::MAX_ZIP_ENTRY_BYTES as u128,
            published: "128 MiB",
            fixtures: 1_024,
            comic: 48_000_000,
            // The largest single entry, which for this yardstick is one fixed
            // page's markup rather than an image: 40 000 segments at about
            // fourteen bytes of `L 1234,5678 ` apiece, plus 2 000 elements at
            // about sixty. A comic's largest entry is a scan and is seventy
            // times this, which is why the two yardsticks disagree most here.
            document: 40_000 * 14 + 2_000 * 60,
            reachable: u32::MAX as u128,
            reachable_because: "the uncompressed-size field is 32 bits, and Zip64's is 64",
            declared_in: ZIP_LIMITS,
            fires_in: (
                "an_entry_declaring_more_than_the_per_entry_cap_is_refused_before_it_allocates",
                ZIP_TESTS,
            ),
        },
        Bound {
            name: "MAX_ZIP_INFLATED",
            cap: zip_limits::MAX_ZIP_INFLATED as u128,
            published: "1 GiB",
            fixtures: 1_024,
            comic: 300_000_000,
            // Two hundred of those pages, plus about twenty megabytes of fonts
            // and images. This is the row the whole of gap 30 stands behind:
            // an XPS part reaches this engine as a ZIP entry, so nothing gap 30
            // reads is admitted without being charged here first.
            document: 200 * (40_000 * 14 + 2_000 * 60) + 20_000_000,
            // The work cap's whole argument: a per-entry ceiling times a
            // file-chosen entry count is not a bound, and this is the product
            // it would otherwise be.
            reachable: zip_limits::MAX_ZIP_ENTRIES as u128
                * zip_limits::MAX_ZIP_ENTRY_BYTES as u128,
            reachable_because: "every entry the entry cap allows, each at the per-entry cap",
            declared_in: ZIP_LIMITS,
            fires_in: (
                "a_zip_bomb_is_refused_by_name_rather_than_by_running_out_of_memory",
                ZIP_TESTS,
            ),
        },
        Bound {
            name: "MAX_ZIP_NAME_LEN",
            cap: zip_limits::MAX_ZIP_NAME_LEN as u128,
            published: "1 024",
            fixtures: 24,
            comic: 42,
            // The longest part name a real package in this repository holds,
            // measured off milestone 1's committed inventory rather than
            // imagined: `Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF`,
            // which is `Resources/` plus a GUID plus an extension. Doubled for
            // headroom, because the yardstick is a *plausible* document rather
            // than the corpus.
            document: 104,
            reachable: u16::MAX as u128,
            reachable_because: "the name-length field is 16 bits",
            declared_in: ZIP_LIMITS,
            fires_in: ("a_name_past_the_cap_is_truncated_and_says_so", ZIP_TESTS),
        },
        Bound {
            name: "MAX_PNG_SAMPLES",
            cap: MAX_PNG_SAMPLES as u128,
            published: "67 108 864",
            fixtures: 4_096,
            comic: 24_000_000,
            // **The row that made filling these in worth doing.** A full-page
            // 300 dpi RGBA scan — 2 550 x 3 300 x 4 — which is an ordinary
            // thing for a report to carry and is *larger* than any page of a
            // 2000 x 3000 comic. The margin under this cap is half what gap
            // 29's yardstick alone suggested, and RGBA is not academic here:
            // it is what WPF's serialiser writes, so it is the colour type
            // every real package in this repository uses.
            document: 2_550 * 3_300 * 4,
            // Thirteen bytes of IHDR: two 31-bit dimensions, charged at the
            // widest layout the colour type can produce.
            reachable: 0x7FFF_FFFFu128 * 0x7FFF_FFFF * 4,
            reachable_because: "IHDR's two 31-bit dimensions, times four components",
            declared_in: PNG,
            fires_in: (
                "an_image_past_the_sample_cap_is_refused_before_it_allocates",
                PNG_TESTS,
            ),
        },
        Bound {
            name: "MAX_CBZ_PAGES",
            cap: MAX_CBZ_PAGES as u128,
            published: "4 096",
            // The archive built past this cap holds 4 097 entries; 4 096 is
            // the most any fixture here spends and is *allowed*.
            fixtures: 4_096,
            comic: 200,
            // Zero, and it is an answer rather than a blank: a fixed document
            // has no comic pages at all, exactly as a comic archive has no XML
            // and every gap 30 row above says `comic: 0`. The symmetry is the
            // check — a yardstick that measured something here would mean the
            // two paths had been confused, which is the defect this whole gap
            // exists to fix.
            document: 0,
            reachable: zip_limits::MAX_ZIP_ENTRIES as u128,
            reachable_because: "every entry the archive reader will hand over could be an image",
            declared_in: CBZ,
            fires_in: ("a_page_count_past_the_cap_is_refused_by_name", CBZ_TESTS),
        },
        Bound {
            name: "MAX_SYNTHESISED_PDF",
            cap: MAX_SYNTHESISED_PDF as u128,
            published: "512 MiB",
            fixtures: 70_000,
            comic: 300_000_000,
            // The document gap 30 synthesises from that package: 40 000
            // segments a page at about twenty bytes of content-stream operator,
            // two hundred times, plus the fonts and the images. Shared with
            // gap 29 rather than duplicated — `cbz.rs` declares this cap and
            // `xps.rs` reuses it, which is the ledger entry the plan asked for
            // in as many words.
            document: 200 * 40_000 * 20 + 20_000_000,
            reachable: MAX_CBZ_PAGES as u128
                * (PAGE_OVERHEAD as u128 + zip_limits::MAX_ZIP_ENTRY_BYTES as u128),
            reachable_because: "every page the page cap allows, each carrying a whole entry",
            declared_in: CBZ,
            fires_in: (
                "a_synthesis_past_the_byte_cap_is_refused_by_name",
                CBZ_TESTS,
            ),
        },
        // ---- gap 30, milestone 2 ---------------------------------------
        //
        // The ceiling in front of all four is the same and it is worth stating
        // once: an XML part reaches this engine as a ZIP entry, so
        // `MAX_ZIP_ENTRY_BYTES` — 128 MiB — is the most text any of them can be
        // handed. Every `reachable` below is that number divided by what one
        // unit of the thing being counted costs in bytes.
        Bound {
            name: "MAX_XML_DEPTH",
            cap: xml_limits::MAX_XML_DEPTH as u128,
            published: "256",
            // The fixture that proves it fires nests 257 and is 771 bytes, so
            // the most any fixture *spends* is the cap. Real markup reaches 6,
            // measured by `xml_real_packages.rs`.
            fixtures: xml_limits::MAX_XML_DEPTH as u128,
            // A comic archive holds no XML at all: gap 29's `ComicInfo.xml` is
            // still nobody's scope and gap 30 says so in as many words.
            comic: 0,
            // ECMA-388 18.2 recommends 16 canvases; a path geometry adds four
            // and a resource dictionary two.
            document: 24,
            reachable: (zip_limits::MAX_ZIP_ENTRY_BYTES as u128) / 3,
            reachable_because: "`<a>` is three bytes, in a part of at most MAX_ZIP_ENTRY_BYTES",
            declared_in: XML_LIMITS,
            fires_in: ("nesting_past_the_depth_cap_is_refused_by_name", XML_TESTS),
        },
        Bound {
            name: "MAX_XML_ATTRIBUTES",
            cap: xml_limits::MAX_XML_ATTRIBUTES as u128,
            published: "256",
            fixtures: xml_limits::MAX_XML_ATTRIBUTES as u128,
            comic: 0,
            // A `Glyphs` with every optional attribute ECMA-388 12.1 gives it.
            document: 24,
            reachable: (zip_limits::MAX_ZIP_ENTRY_BYTES as u128) / 5,
            reachable_because: "` a=\"\"` is five bytes, and they may all sit on one element",
            declared_in: XML_LIMITS,
            fires_in: ("more_attributes_than_the_cap_is_refused_by_name", XML_TESTS),
        },
        Bound {
            name: "MAX_XML_NAME_LEN",
            cap: xml_limits::MAX_XML_NAME_LEN as u128,
            published: "1 024",
            fixtures: xml_limits::MAX_XML_NAME_LEN as u128,
            comic: 0,
            // `LinearGradientBrush.GradientStops` is 33, plus room for a prefix.
            document: 48,
            reachable: zip_limits::MAX_ZIP_ENTRY_BYTES as u128,
            reachable_because: "a name may be as long as the part that holds it",
            declared_in: XML_LIMITS,
            fires_in: (
                "a_name_past_the_cap_is_refused_rather_than_truncated",
                XML_TESTS,
            ),
        },
        Bound {
            name: "MAX_XML_TOKENS",
            cap: xml_limits::MAX_XML_TOKENS as u128,
            published: "1 048 576",
            fixtures: xml_limits::MAX_XML_TOKENS as u128,
            comic: 0,
            // 2 000 drawable elements at three elements of markup each, plus
            // 40 000 path segments as `PolyLineSegment` children, at two events
            // an element.
            document: 2_000 * 3 * 2 + 40_000 * 2,
            // The work cap's argument, in this format's terms: `<a/>` is four
            // bytes and produces two events, so a per-element cap times a
            // file-chosen element count is not a bound and this is the product
            // it would otherwise be.
            reachable: (zip_limits::MAX_ZIP_ENTRY_BYTES as u128) / 2,
            reachable_because: "`<a/>` is four bytes and two events, across a whole part",
            declared_in: XML_LIMITS,
            fires_in: (
                "more_events_than_the_token_cap_is_refused_by_name",
                XML_TESTS,
            ),
        },
        // ---- gap 30, milestone 3 ---------------------------------------
        Bound {
            name: "MAX_XPS_PARTS",
            cap: MAX_XPS_PARTS as u128,
            published: "8 192",
            // The package built *past* this cap holds 8 193 parts; 8 192 is the
            // most any fixture here spends and is allowed. The largest real
            // package in the corpus holds 7.
            fixtures: MAX_XPS_PARTS as u128,
            // A comic archive is not a package at all.
            comic: 0,
            // One sequence, one document, two hundred pages, two hundred page
            // relationships parts, the package relationships part and about a
            // hundred fonts and images.
            document: 505,
            reachable: zip_limits::MAX_ZIP_ENTRIES as u128,
            reachable_because: "every entry the archive reader will hand over could be a part",
            declared_in: XPS,
            fires_in: ("a_package_past_the_part_cap_is_refused_by_name", XPS_TESTS),
        },
        Bound {
            name: "MAX_XPS_PAGES",
            cap: MAX_XPS_PAGES as u128,
            published: "4 096",
            fixtures: MAX_XPS_PAGES as u128,
            comic: 0,
            document: 200,
            // **Not** the part cap, and this is the row that says why: two
            // hundred `PageContent` elements may name one part between them, so
            // the page count is not bounded by the part count at all. What
            // stands in front of it is the markup reader's own total.
            reachable: xml_limits::MAX_XML_TOKENS as u128,
            reachable_because: "every event one fixed document part may produce could be a page",
            declared_in: XPS,
            fires_in: (
                "a_page_count_past_the_xps_cap_is_refused_by_name",
                XPS_TESTS,
            ),
        },
        Bound {
            name: "MAX_XPS_ELEMENTS",
            cap: MAX_XPS_ELEMENTS as u128,
            published: "1 048 576",
            // One short of the cap, which is what
            // `a_page_past_the_element_cap_is_refused_by_name`'s *opening*
            // package spends; the one built past it spends three more.
            fixtures: MAX_XPS_ELEMENTS as u128 - 1,
            comic: 0,
            document: 400_000,
            // No **single** part can reach this, which is the whole reason it
            // is a total: `MAX_XML_TOKENS` bounds one part at a million events,
            // of which at most half can be start tags. The ceiling is that,
            // once per part the package may hold.
            reachable: MAX_XPS_PARTS as u128 * (xml_limits::MAX_XML_TOKENS as u128 / 2),
            reachable_because: "half a million elements in each of the parts a package may hold",
            declared_in: XPS,
            fires_in: (
                "a_page_past_the_element_cap_is_refused_by_name",
                XPS_MARKUP_TESTS,
            ),
        },
        Bound {
            name: "MAX_XPS_SEGMENTS",
            cap: MAX_XPS_SEGMENTS as u128,
            published: "8 388 608",
            fixtures: MAX_XPS_SEGMENTS as u128,
            comic: 0,
            document: 8_000_000,
            // `H1` is two bytes and one segment, in a part this build already
            // admits at 128 MiB — and that is one path, before the file has
            // chosen how many paths or how many pages.
            reachable: zip_limits::MAX_ZIP_ENTRY_BYTES as u128 / 2,
            reachable_because: "a two-byte `H` command per segment in one 128 MiB part",
            declared_in: XPS,
            fires_in: (
                "a_geometry_past_the_segment_cap_is_refused_by_name",
                XPS_MARKUP_TESTS,
            ),
        },
        Bound {
            name: "MAX_XPS_GLYPHS",
            cap: MAX_XPS_GLYPHS as u128,
            published: "2 097 152",
            fixtures: MAX_XPS_GLYPHS as u128,
            comic: 0,
            document: 1_000_000,
            // `1;` is two bytes and one more mapping, in one attribute of one
            // element, in a part this build already admits at 128 MiB — and a
            // `Glyphs` costs one element and no segments, so neither cap
            // milestone 6 added stands in front of this one.
            reachable: zip_limits::MAX_ZIP_ENTRY_BYTES as u128 / 2,
            reachable_because: "a two-byte `1;` per glyph in one `Indices` attribute",
            declared_in: XPS,
            fires_in: (
                "a_run_past_the_glyph_cap_is_refused_by_name",
                XPS_GLYPH_TESTS,
            ),
        },
        Bound {
            name: "MAX_XPS_RESOURCE_DEPTH",
            cap: MAX_XPS_RESOURCE_DEPTH as u128,
            published: "16",
            fixtures: MAX_XPS_RESOURCE_DEPTH as u128,
            comic: 0,
            document: 2,
            // One dictionary may hold as many entries as its part has events,
            // and a chain through all of them is not a cycle.
            reachable: xml_limits::MAX_XML_TOKENS as u128,
            reachable_because: "every entry one resource dictionary part could declare, chained",
            declared_in: XPS,
            fires_in: (
                "a_static_resource_chain_past_the_depth_cap_is_named",
                XPS_MARKUP_TESTS,
            ),
        },
        // ---- gap 31, milestone 3 ---------------------------------------
        Bound {
            name: "MAX_OCF_PATH_LEN",
            cap: MAX_OCF_PATH_LEN as u128,
            published: "65 535",
            // The container built *past* this cap names a path of 65 536; the
            // longest path in any of the twenty-six real books milestone 1
            // measured is 39 bytes.
            fixtures: MAX_OCF_PATH_LEN as u128,
            // A comic archive has no container paths at all, and a fixed
            // document has OPC part names rather than OCF content paths — two
            // grammars that disagree about case folding and about `..`, which
            // is why `PartName` is not reused. Both are a zero that is an
            // answer rather than a blank, in the shape every gap 30 row above
            // writes `comic: 0`.
            comic: 0,
            document: 0,
            // An XML attribute value may be as long as the part that holds it,
            // and a `META-INF/container.xml` reaches this engine as a ZIP entry
            // — so the ceiling is the per-entry cap, and the specification's own
            // number sits two thousand times below it.
            reachable: zip_limits::MAX_ZIP_ENTRY_BYTES as u128,
            reachable_because: "a `full-path` attribute may be as long as the entry that holds it",
            declared_in: EPUB,
            fires_in: (
                "a_content_path_past_the_length_cap_is_refused_by_name",
                EPUB_TESTS,
            ),
        },
    ]
}

/// Gap 29's bounds table has five rows and its code has seven, because two of
/// the plan's "per-item caps sit beside them" were built as named constants.
/// Gap 30's milestone 2 adds four, its milestone 3 adds two and its milestone
/// 6 adds three; gap 31's milestone 3 adds one. All eighteen are here, and a
/// bound added without a row fails this.
#[test]
fn the_sweep_covers_every_bound_these_three_gaps_added() {
    let names: Vec<&str> = ledger().iter().map(|b| b.name).collect();
    assert_eq!(
        names,
        [
            "MAX_ZIP_ENTRIES",
            "MAX_ZIP_ENTRY_BYTES",
            "MAX_ZIP_INFLATED",
            "MAX_ZIP_NAME_LEN",
            "MAX_PNG_SAMPLES",
            "MAX_CBZ_PAGES",
            "MAX_SYNTHESISED_PDF",
            "MAX_XML_DEPTH",
            "MAX_XML_ATTRIBUTES",
            "MAX_XML_NAME_LEN",
            "MAX_XML_TOKENS",
            "MAX_XPS_PARTS",
            "MAX_XPS_PAGES",
            "MAX_XPS_ELEMENTS",
            "MAX_XPS_SEGMENTS",
            "MAX_XPS_GLYPHS",
            "MAX_XPS_RESOURCE_DEPTH",
            "MAX_OCF_PATH_LEN",
        ],
        "a bound was added or renamed without a row in this sweep"
    );

    // Every one of them is a `pub const` where its ledger says it is, which is
    // what makes the numbers above readable from outside their own crate.
    for bound in ledger() {
        assert!(
            bound
                .declared_in
                .contains(&format!("pub const {}", bound.name)),
            "{} is not declared where this sweep says it is",
            bound.name
        );
    }
}

/// **The check gap 18a milestone 8 was missing.**
///
/// A cap is only a cap if its own inputs can ask for more than it allows. Each
/// row computes the ceiling of what stands in front of it — the field widths
/// the format gives, or the product of the caps below it — and the assertion
/// is that the constant sits underneath. A row that fails this has a constant
/// that no input can ever reach, which is decoration with a `MAX_` prefix.
#[test]
fn every_bound_can_fire() {
    for bound in ledger() {
        assert!(
            bound.reachable > bound.cap,
            "{} is {} and the most its own inputs can ask for is {} ({}): it \
             can never fire, which is gap 18a milestone 8's failure exactly",
            bound.name,
            bound.cap,
            bound.reachable,
            bound.reachable_because,
        );
    }
}

/// And the other direction: a cap that refuses the thing the format is for is
/// not a bound, it is a missing feature.
///
/// The yardstick is gap 29's own — a 200-page comic at 2000 x 3000 — and the
/// margin between it and the cap is the safety margin the plan asks to be
/// written down. Writing it down is what would have caught `MAX_JPX_WORK`.
#[test]
fn no_bound_refuses_a_two_hundred_page_comic() {
    for bound in ledger() {
        assert!(
            bound.comic < bound.cap,
            "{} is {} and a 200-page comic spends {}: this cap refuses a real \
             archive",
            bound.name,
            bound.cap,
            bound.comic,
        );
        assert!(
            bound.fixtures <= bound.cap,
            "{}: a fixture spends {} against a cap of {}",
            bound.name,
            bound.fixtures,
            bound.cap,
        );
    }
}

/// Gap 30's yardstick, for the rows that have one: a 200-page fixed document at
/// roughly 2 000 drawable elements and 40 000 path segments a page.
///
/// A separate test from the comic rather than a second assertion inside it,
/// because the two yardsticks measure different documents and a row's two
/// figures are genuinely different numbers — a comic holds no XML and a fixed
/// document holds no comic page, and both of those are recorded as a zero
/// rather than as an absence. What must not happen is a row acquiring a bound
/// that refuses the format it was written for, which is the failure this pair
/// of tests exists to make impossible in both directions.
///
/// *Amended by milestone 9.* It swept ten rows of seventeen until then, because
/// gap 29's seven arrived before gap 30 had a yardstick and opted out with a
/// `None`. **A row that opts out of a check is a row that is not checked**, and
/// four of those seven are the ZIP caps every XPS part in this repository is
/// admitted through — so the seven rows standing in front of gap 30's whole
/// input path were the seven this test could not see.
#[test]
fn no_bound_refuses_a_dense_fixed_document() {
    let mut measured = 0usize;
    for bound in ledger() {
        measured += 1;
        assert!(
            bound.document < bound.cap,
            "{} is {} and a dense 200-page fixed document spends {}: this cap \
             refuses a real document",
            bound.name,
            bound.cap,
            bound.document,
        );
    }
    // A sweep that found nothing to sweep is a sweep that does not run, and a
    // sweep that found *some* of it is what this test was until milestone 9.
    assert_eq!(
        measured,
        ledger().len(),
        "gap 30's yardstick covers {measured} rows and the ledger has {}",
        ledger().len(),
    );
    assert_eq!(measured, 18, "the ledger is eighteen rows");
}

/// Each constant's ledger publishes its value in prose, and prose does not
/// compile.
///
/// So the published rendering is asserted to be in the file that declares the
/// constant, beside a `**This cap**` row. A constant changed without its table
/// fails here — which is the drift `tinker-pdf-math` demonstrated for the leaf
/// count, where a number written in three documents moved in none of them.
#[test]
fn every_bound_publishes_the_number_it_is() {
    for bound in ledger() {
        assert!(
            bound
                .declared_in
                .contains(&format!("**This cap** | **{}**", bound.published)),
            "{}'s ledger does not publish {} as its cap",
            bound.name,
            bound.published,
        );
    }

    // The published renderings are the values, not decoration: every one of
    // them parses back to the constant it names.
    for bound in ledger() {
        let published = bound.published.replace(' ', "");
        let value = match published.strip_suffix("MiB") {
            Some(n) => n.parse::<u128>().expect("a number") * (1 << 20),
            None => match published.strip_suffix("GiB") {
                Some(n) => n.parse::<u128>().expect("a number") * (1 << 30),
                None => published.parse::<u128>().expect("a number"),
            },
        };
        assert_eq!(
            value, bound.cap,
            "{}'s ledger publishes {} and the constant is {}",
            bound.name, bound.published, bound.cap,
        );
    }
}

/// Every bound names a test that fires it, and that test exists.
///
/// The plan's exit criterion is that each fires "by its own warning or
/// refusal, not by a clock" — `5adf502`'s method, taken for its stated reason:
/// a timing assertion fails on a slow machine and passes on a fast one with
/// the budget removed. What this can check from here is that the named test is
/// still in the file it is claimed to be in; what it cannot check is that the
/// test still asserts a refusal, which is why each of those tests carries a
/// doc comment saying which refusal it is waiting for.
#[test]
fn every_bound_names_a_test_that_exists() {
    for bound in ledger() {
        let (test, source) = bound.fires_in;
        assert!(
            source.contains(&format!("fn {test}(")),
            "{} is proved to fire by {test}, and there is no such test",
            bound.name,
        );
        // And it is a test rather than a helper that happens to be named like
        // one, and not an `#[ignore]`d one either: the attribute immediately
        // before it is `#[test]` and nothing else. Whitespace is trimmed rather
        // than matched, because a checkout with `core.autocrlf` on ends these
        // lines with `\r\n` — which the injection matrix found by rewriting
        // these files through a tool that translates them.
        let at = source.find(&format!("fn {test}(")).expect("just found");
        assert!(
            source[..at].trim_end().ends_with("#[test]"),
            "{test} is not a `#[test]`, or is ignored",
        );
    }

    // Not one of them may be a timing assertion. `5adf502` is the scar: a
    // budget proved by a clock passes on a fast machine with the budget
    // removed.
    // `XPS_GLYPH_TESTS` joined this list in milestone 9 and was missing from it
    // from the moment milestone 7 added the row it holds: a source named by a
    // `fires_in` and left out here is a file where a clock could be introduced
    // without this sweep noticing, which is half of the fifth check absent for
    // exactly one bound.
    for source in [
        ZIP_TESTS,
        PNG_TESTS,
        CBZ_TESTS,
        XML_TESTS,
        XPS_TESTS,
        XPS_MARKUP_TESTS,
        XPS_GLYPH_TESTS,
        EPUB_TESTS,
    ] {
        assert!(
            !source.contains("Instant::now"),
            "a bound in this gap is being proved by a clock"
        );
    }
}
