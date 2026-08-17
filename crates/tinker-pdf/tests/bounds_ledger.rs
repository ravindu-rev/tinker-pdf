//! Gap 29's seven bounds, swept in one place (milestone 6).
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
use tinker_pdf_filters::MAX_PNG_SAMPLES;

/// The sources the ledgers live in, so a number here can be checked against
/// the number written beside the constant rather than against a memory of it.
const ZIP_LIMITS: &str = include_str!("../../tinker-pdf-zip/src/limits.rs");
const ZIP_TESTS: &str = include_str!("../../tinker-pdf-zip/src/tests.rs");
const PNG: &str = include_str!("../../tinker-pdf-filters/src/png.rs");
const PNG_TESTS: &str = include_str!("../../tinker-pdf-filters/src/png/tests.rs");
const CBZ: &str = include_str!("../src/cbz.rs");
const CBZ_TESTS: &str = include_str!("cbz.rs");

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
            reachable: MAX_CBZ_PAGES as u128
                * (PAGE_OVERHEAD as u128 + zip_limits::MAX_ZIP_ENTRY_BYTES as u128),
            reachable_because: "every page the page cap allows, each carrying a whole entry",
            declared_in: CBZ,
            fires_in: (
                "a_synthesis_past_the_byte_cap_is_refused_by_name",
                CBZ_TESTS,
            ),
        },
    ]
}

/// The plan's bounds table has five rows and the code has seven, because two
/// of the plan's "per-item caps sit beside them" were built as named
/// constants. All seven are here, and a bound added without a row fails this.
#[test]
fn the_sweep_covers_every_bound_gap_twenty_nine_added() {
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
    for source in [ZIP_TESTS, PNG_TESTS, CBZ_TESTS] {
        assert!(
            !source.contains("Instant::now"),
            "a bound in this gap is being proved by a clock"
        );
    }
}
