//! Repository chores that need more than a cargo command.
//!
//! `cargo xtask <task>`. Every task exits non-zero on failure so CI can run it
//! without a wrapper.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xtask::version::{internal_dependencies, package_name};
use xtask::{corpus, fetch, parity, release, repo_root, version};

const USAGE: &str = "\
xtask — repository chores

usage:
  cargo xtask dag       check the crate dependency graph against the declared one
  cargo xtask libm      check that no pixel path calls the platform's libm
  cargo xtask oracles   check that every program this repository spawns is
                        written down with a reason (ruling 13)
  cargo xtask vendor    check vendored data against THIRDPARTY.md and deny.toml
  cargo xtask versions  check every manifest against the workspace version, and
                        `publish = false` where publishing would be wrong
  cargo xtask check     all five of the above

  cargo xtask release [options]  publish to crates.io, PyPI, npm and NuGet

release options:
  (none)          a DRY RUN. Every step is reported in dependency order and
                  the tools are invoked in whatever harmless form each has
  --dry-run       the same thing, said out loud
  --execute       actually publish. This cannot be undone on crates.io
  --plan          print the steps and run nothing at all
  --only STAGE    preflight | crates | wheel | npm | nuget; repeatable
  --tag vX.Y.Z    fail unless the tag matches the workspace version
  --local-registry
                  verify each crate against the packaged copies of the crates
                  before it, instead of against crates.io. Without it, ten of
                  the fifteen cannot be dry-run at all: `cargo publish
                  --dry-run` resolves against the live index and nothing of
                  this name has ever been published there

  cargo xtask nuget-stage        copy this machine's tinker-pdf-ffi cdylib into
                                 bindings/dotnet/runtimes/<rid>/native/

  cargo xtask bindings-parity [options]
                  run both write-parity scripts on all four surfaces and
                  require byte-identical output. Non-zero on a mismatch OR on
                  a surface that ran and printed no `WROTE sha256=` line at
                  all, which is the failure that gets shipped. A surface whose
                  artefact is not installed here is SKIPPED, by name and with
                  the reason -- never silently

bindings-parity options:
  --require-all   a skipped surface is a failure. What CI passes
  --python P      the interpreter that has the wheel installed (default:
                  python; skipped when `import tinker_pdf` fails in it)
  --node-dir D    a directory where the npm tarball has been installed
                  (default: target/js-parity)

  cargo xtask synth-face [--out PATH]    write the synthetic face `--fonts
                                 synthetic` measures with, so it can be looked at

  cargo xtask corpus-fetch [--record] [--force] [--corpus NAME]
                                         fetch and verify the pinned corpora
  cargo xtask corpus-licences [--check]  the corpus lock's licence table
  cargo xtask corpus-run [options]       open and render every corpus file

corpus-run options:
  --corpus NAME   only this one; repeatable
  --timeout N     seconds per file before the child is killed (default 20)
  --dpi D         render resolution (default 72)
  --fonts PATH    a face, or directory of faces, for documents embedding none
  --fonts synthetic
                  the face this repository writes for itself, so the second
                  bar needs no licence, no fetch and no runner image. The
                  keyword wins over a directory of that name
  --jobs N        files at once (default: the core count)
  --sample N      at most N files per corpus, recorded as a limit
  --child PATH    the program to spawn (default: tpdf beside this binary)
  --report PATH   write the full per-file report here
  --check         compare against the bar for this --fonts setting; fail on a
                  regression. corpus/ratchet.json without faces,
                  corpus/ratchet-fonts.json with them — never each other
  --record        rewrite that bar from this run
  --strict        with --check, a rise in the degradation rate also fails

  cargo xtask help
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let rest = args.get(1..).unwrap_or_default();

    match task.as_str() {
        "dag" => report("dag", check_dag()),
        "libm" => report("libm", check_libm()),
        "oracles" => report("oracles", check_oracles()),
        "vendor" => report("vendor", check_vendor()),
        "versions" => report("versions", version::check(&repo_root())),
        "check" => {
            let dag = check_dag();
            let libm = check_libm();
            let oracles = check_oracles();
            let vendor = check_vendor();
            let versions = version::check(&repo_root());
            let mut problems = dag.err().unwrap_or_default();
            problems.extend(libm.err().unwrap_or_default());
            problems.extend(oracles.err().unwrap_or_default());
            problems.extend(vendor.err().unwrap_or_default());
            problems.extend(versions.err().unwrap_or_default());
            report(
                "check",
                if problems.is_empty() {
                    Ok(())
                } else {
                    Err(problems)
                },
            )
        }
        "release" => one("release", release::run(&repo_root(), rest)),
        "nuget-stage" => one(
            "nuget-stage",
            release::stage_native_library(&repo_root()).map(|what| {
                println!("nuget-stage: {what}");
                println!(
                    "nuget-stage: RIDs now staged: {}",
                    release::staged_rids(&repo_root()).join(", ")
                );
            }),
        ),
        "bindings-parity" => one("bindings-parity", parity::run(&repo_root(), rest)),
        "corpus-licences" => one("corpus-licences", corpus::licences(&repo_root(), rest)),
        "corpus-fetch" => one("corpus-fetch", fetch::fetch(&repo_root(), rest)),
        "corpus-run" => one("corpus-run", corpus::run(&repo_root(), rest)),
        "synth-face" => one("synth-face", synth_face(rest)),
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("xtask: unknown task `{other}`");
            print!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `cargo xtask synth-face` — write the face the second corpus bar uses.
///
/// Exists so the face is a file somebody can open rather than an argument
/// nobody can inspect. `corpus-run --fonts synthetic` writes the same bytes to
/// the same place and does not need this to have been run.
fn synth_face(args: &[String]) -> Result<(), String> {
    let root = repo_root();
    let mut path = xtask::face::default_path(&root);
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                path = std::path::PathBuf::from(
                    args.get(index).ok_or("`--out` needs a path")?.clone(),
                );
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 1;
    }
    xtask::face::write(&path)?;
    println!(
        "synth-face: wrote {} ({} bytes, {})",
        path.display(),
        xtask::face::bytes().len(),
        xtask::face::SYNTHETIC
    );
    Ok(())
}

/// A task whose failure is one message rather than a list of problems.
fn one(task: &str, outcome: Result<(), String>) -> ExitCode {
    match outcome {
        Ok(()) => {
            println!("{task}: ok");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{task}: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The dependency graph the architecture plan declares, as
/// `crate -> everything it may depend on`.
///
/// Ruling 8 and plan 00: the leaves take bytes and return values and know no
/// PDF types, which is what makes each of them independently fuzzable and
/// keeps the layering honest. Nothing here is checked by the compiler — a new
/// edge compiles perfectly well — so it is checked here instead.
///
/// **`cos -> font` is a deliberate amendment to the plan's DAG.** The plan
/// listed `cos -> {filters, crypto}`. Reading a font *dictionary* — its
/// encoding, its `/ToUnicode`, its standard-14 metrics — is COS work, and it
/// needs the leaf font crate's CMap and encoding tables to do it. The
/// alternative is a third crate between them whose only job is to hold two
/// tables, which is worse. The edge points from a higher layer to a leaf, so
/// it does not invert the layering.
///
/// **`math` is a second-order leaf.** Deterministic `sin`, `ln` and `pow`
/// belong *under* the rasteriser and the colour code rather than inside
/// either, because both need them and neither may depend on the other. It has
/// no dependencies of its own — not even `std` — so it adds a layer below the
/// leaves rather than an edge between them.
///
/// **`font -> filters` is the second amendment, and it is one leaf to
/// another.** Plan 05's build-time data table says the predefined CMaps ship
/// "delta-encoded ranges, deflated with our own filter code", and its
/// dependency section already says the phase "needs ... the filters phase
/// (FlateDecode for FontFile2/3 **and for the bundled-asset pipeline**)".
/// This is that edge, arriving where the plan said it would: `build.rs`
/// deflates with `tinker-pdf-filters` and the crate inflates with it. Ruling 3
/// is untouched — a sibling workspace crate is not a third-party dependency —
/// and the graph cannot cycle, because `filters` depends on nothing. The
/// runtime half is optional and disappears with `cmap-predefined`; the
/// build-time half is unconditional and never reaches a binary.
///
/// **`zip -> filters` is the third amendment, and the second leaf-to-leaf
/// edge.** A ZIP entry stored with method 8 is raw RFC 1951 by definition
/// (APPNOTE 4.4.5), and every entry carries a CRC-32 the format requires be
/// checked. Both already exist in `filters` — `inflate_raw` was made public
/// for exactly this caller, and `crc32` was written there because PNG needs
/// the identical routine for its chunk checksums. The alternatives are worse
/// in the two available directions: re-implementing DEFLATE inside the ZIP
/// crate is a second copy of the most intricate code in the tree, and moving
/// the archive reader into `filters` puts a container format inside a crate
/// whose entire subject is PDF stream filters.
///
/// The same three properties that made `font -> filters` acceptable hold
/// here. It points from one leaf to another rather than upward, so the
/// layering is not inverted. It cannot cycle, because `filters` depends on
/// nothing. And a sibling workspace crate is not a third-party dependency, so
/// ruling 3 and CONTRIBUTING rule 1 are untouched — which is the whole point:
/// gap 29 exists so that reading a comic archive adds no crate from outside
/// this repository.
///
/// **`zip` is a leaf despite having a dependency**, on the same reading that
/// makes `font` and `color` leaves: what a leaf means here is bytes in,
/// values out, no PDF types, independently fuzzable. `tinker-pdf-zip` knows
/// nothing of documents, pages or objects; it turns an archive into names and
/// byte ranges. Gap 29's page semantics live in the facade for precisely that
/// reason.
///
/// **`xml` is the fourth amendment, and the argument is that it needs
/// nothing.** The other three each added an edge and had to say why the edge
/// was safe; this one adds a node with an empty allow-list, and the thing worth
/// writing down is that the empty list is a *finding* rather than an omission.
/// `tinker-pdf-xml` was checked against what it might have wanted and wants
/// none of it: not `filters`, because nothing in XML is compressed and the five
/// predefined entities cannot expand — a reference is at least four bytes and
/// produces exactly one character, so decoded text is never longer than its
/// source and there is no decoder to reach for; not `zip`, because a part
/// arrives as a byte slice and where it came from is the caller's business
/// (gap 30's package layer lives in the facade for exactly that reason); not
/// `math`, because there is no arithmetic here beyond a checked multiply in a
/// character reference; and not `cos`, which is the edge that would invert the
/// layering. It is the third crate in the workspace with no internal
/// dependency at all, beside `filters` and `crypto`.
///
/// What it does add is a name to the facade's row, and the reason that is
/// listed rather than left to arrive with the code is the commentary directly
/// above this one: a crate appearing in a manifest without its argument is the
/// failure this file's own history records. Gap 31, EPUB, reuses this crate and
/// reuses **none** of gap 30's package layer, because EPUB's container is OCF
/// rather than OPC — which is the argument for the parser being a crate and the
/// package layer not being one, and it is recorded in both places.
///
/// **`css` is the fifth amendment, and it is the second whose argument is that
/// it needs nothing.** `xml` set that register — an empty allow-list is a
/// *finding* rather than an omission — and `tinker-pdf-css` was put through the
/// same interrogation with the same answer. Not `xml`, because a stylesheet is
/// not markup: `css-syntax-3` has its own tokenizer, and a `<style>` element's
/// contents arrive here as a byte slice with no idea what element they came
/// out of. Not `math`, because there is no transcendental in CSS at all — the
/// arithmetic is unit conversion, percentage resolution and `css-color-4` §7's
/// HSL-to-RGB, every one of which is multiply, divide, compare and subtract,
/// so ruling 4's `cargo xtask libm` rule has nothing here to object to even
/// though this crate is not on `PIXEL_PATHS`. Not `font`, because `@font-face`
/// **names** a face and does not read one, and because the element side of
/// matching arrives through a five-method trait rather than through anything
/// that knows what a glyph is. Not `filters`, because nothing in a stylesheet
/// is compressed and nothing in it expands. Not `zip`, for `xml`'s reason
/// exactly: an `@import` is resolved by a caller-supplied `ImportResolver`,
/// because the container is the caller's business and a leaf that could open
/// one would be a leaf with a direction it did not need. And not `cos`, which
/// is the edge that would invert the layering.
///
/// The empty list is load-bearing in a way the other four were not, and this
/// is the sentence worth keeping: because `tinker-pdf-css` has **no**
/// dependency, internal or third-party, and no build script, the whole crate
/// compiles with a bare `rustc` and no dependency resolution — which is what
/// lets `tests/unimplemented_property_does_not_build.rs` compile the *real*
/// source with a defect injected and assert that the **build** fails. Gap 31's
/// decision 5 is the device its whole reflowable scope was accepted on, and the
/// proof of it is possible here only because this row is empty.
///
/// It adds a name to the facade's row too, ahead of the manifest edge that
/// arrives with gap 31's milestone 8 — the same order gap 30 used for `xml`,
/// and for the same reason the commentary above gives: a crate appearing in a
/// manifest without its argument is the failure this file's own history
/// records.
///
/// **`layout` is the sixth amendment, and it is the first whose argument had to
/// answer a question the plan left open rather than defend an edge somebody
/// wanted.** Gap 31's design section predicts
/// `("tinker-pdf-layout", &["tinker-pdf-math"])` and then says, in as many
/// words, that *"whether layout needs one at all is an open question milestone
/// 7 answers: if it does not, the edge is dropped and the crate joins the
/// empty-list group"*. **It does not, and the edge is dropped.**
///
/// The interrogation, because the answer is only worth as much as the search
/// that produced it. `tinker-pdf-math` exists for ruling 4's reason: a
/// pixel-path crate may not call a platform transcendental, because glibc,
/// musl, the MSVC runtime, Apple's libm and the wasm shim each round `sin`,
/// `exp` and `ln` their own way and a page would then differ by target. Every
/// arithmetic operation in `tinker-pdf-layout` was listed and checked against
/// that rule:
///
/// - **the box model** is addition and subtraction of used widths, and a
///   percentage is one multiply and one divide (§8.3, §10.2);
/// - **margin collapsing** is `max` and `min` over `f64`, which IEEE 754
///   specifies exactly;
/// - **line-height half-leading** is `(line_height - (ascent + descent)) / 2`;
/// - **justification** distributes slack as `slack / spaces`, one divide;
/// - **fragmentation** compares a running `y` against a page height.
///
/// Not one of them is transcendental, and not one is even a `sqrt` — which
/// would have been fine anyway, since IEEE 754 requires `sqrt` to be correctly
/// rounded and every target agrees about it. `cargo xtask libm` would have
/// nothing to object to here even if this crate were on `PIXEL_PATHS`, which
/// it is not, and the same sentence was true of `css` one milestone earlier.
/// Taking the edge anyway would have been the failure this file's own history
/// records from the other direction: an edge in a manifest that nothing needs.
///
/// **What it does take is `tinker-pdf-css`, which the plan did not predict**,
/// and that is the third leaf-to-leaf edge after `font -> filters` and
/// `zip -> filters`. The argument is the plan's own ordering argument read to
/// its conclusion. Gap 31 puts milestone 6 before milestone 7 because *"a
/// layout engine built first has to invent its own input representation, and
/// that representation then becomes what the cascade must produce — which is
/// how a cascade acquires shortcuts"*. A `tinker-pdf-layout` with no edge to
/// `css` would have to declare a second style type and the facade would
/// convert between them, which is exactly the second representation that
/// paragraph refuses; and decision 5's compile-time device would stop at the
/// cascade, because the thing layout matched on would no longer be the
/// parser's own output.
///
/// With the edge, the device goes one level further than the crate that
/// invented it: `style::consume` destructures `ComputedStyle` with **no `..`**,
/// so a property that is parsed, cascaded and then laid out by nobody is
/// `error[E0027]` rather than a page that looks like the book.
/// `tests/uncascaded_field_does_not_build.rs` injects that defect and asserts
/// the build fails.
///
/// The three properties that made the other leaf-to-leaf edges acceptable hold
/// here. It points from one leaf to another rather than upward. It cannot
/// cycle, because `tinker-pdf-css` depends on nothing at all. And a sibling
/// workspace crate is not a third-party dependency, so ruling 3 and
/// CONTRIBUTING rule 1 are untouched — which is the whole point of a gap whose
/// deliverable is that reading a book adds no crate from outside this
/// repository.
///
/// It is a leaf despite having a dependency, on the reading that makes `font`,
/// `zip` and `color` leaves: bytes and plain parameters in, values out, no PDF
/// types, independently fuzzable. A tree of plain structs is plain parameters,
/// and `layout` is the twenty-fourth fuzz target precisely because it can be
/// driven with no file of any kind in front of it.
///
/// **`pki` is the seventh amendment, and it is the fourth leaf-to-leaf edge.**
/// `tinker-pdf-pki` holds ASN.1 DER, X.509 and — from milestone 3 —
/// CMS, and it takes `tinker-pdf-crypto`. Two questions have to be answered
/// separately: why the crate exists at all rather than being part of `crypto`,
/// and why the edge between them is safe.
///
/// **Why it is not inside `tinker-pdf-crypto`, which is where a signature
/// feature would obviously put it.** The two fail differently, and the way a
/// thing fails is what decides how it has to be reviewed. `crypto` is
/// *arithmetic*: its failure mode is a wrong number, and a wrong number is
/// caught completely by published vectors — FIPS 197, RFC 6229, FIPS 180-4 are
/// already merge gates there. ASN.1 is *untrusted-input structure walking*:
/// its failure mode is a panic or a read past the end of a buffer on bytes an
/// attacker chose, which no known-answer vector detects and which ruling 1's
/// per-format fuzzers exist for. Merging them would put the largest new attack
/// surface in the tree inside the one crate whose review story is "small,
/// vector-gated arithmetic", and — the concrete cost — would leave DER
/// reachable by a fuzzer only through `crypto`'s API, so a malformed
/// certificate could be fuzzed only by first constructing a plausible
/// `HandlerParams` around it. Apart, it is the twenty-fifth fuzz target and
/// `fuzz/fuzz_targets/pki_der.rs` points straight at raw DER, which
/// `docs/design/signatures.md:75-92` argues for and its risk table names as the
/// mitigation for the largest risk it records.
///
/// **It is a leaf on ruling 8's definition, which is about public APIs.** X.509
/// and CMS are not PDF concepts: bytes and plain parameters in, values out, no
/// COS types, no `/ByteRange`, no signature dictionary. Everything PDF about a
/// signature — which bytes a `/ByteRange` covers, what `/DocMDP` permits, what
/// a verdict says — lives in the facade, the same split that keeps `zip`
/// ignorant of what an archive entry is *for*.
///
/// **The edge itself.** The three properties that made `font -> filters`,
/// `zip -> filters` and `layout -> css` acceptable hold unchanged. It points
/// sideways from one leaf to another rather than upward, so the layering is not
/// inverted. It cannot cycle, because `tinker-pdf-crypto` depends on nothing at
/// all — the same sentence that carried the first two amendments, and it is
/// still the whole of the cycle argument. And a sibling workspace crate is not
/// a third-party dependency, so ruling 3 and CONTRIBUTING rule 1 are untouched.
///
/// What is taken across it *today* is one function: SHA-1, for RFC 5280
/// §4.2.1.2's method (1) key identifier, which is the SHA-1 of a certificate's
/// `subjectPublicKey` bits and which chain building needs when a certificate
/// carries no `subjectKeyIdentifier` of its own. That is deliberately a real
/// use rather than a placeholder, because this file's own history records the
/// failure in the other direction — an edge in a manifest that nothing needs —
/// and because RFC 5280's Appendix C.1 certificate states the identifier its
/// own key produces, so the edge arrives with a published vector behind it.
/// Milestones 4 and 5 of the design widen what crosses to RSA and ECDSA
/// verification; the edge is declared once, here, and does not move when they
/// land.
///
/// **`shape -> font` is the eighth amendment, and the fifth leaf-to-leaf
/// edge.** `docs/design/shaping.md` makes the argument in two halves, and the
/// interesting half is the one about a table that is *not* taken.
///
/// The edge itself is small and obvious. `tinker_pdf_font::Sfnt` already
/// parses the table directory, and `tinker-pdf-shape` needs exactly that —
/// the twelve-byte header and `table(tag)` — to find `GDEF`, `GSUB` and
/// `GPOS`. A second reader of those twelve bytes in this workspace would be a
/// second place for the same bug to live, and the design says so: *"one sfnt
/// parser in the tree rather than two"*.
///
/// **What is not taken is the parsing of the layout tables themselves**, and
/// that is the decision worth recording. The obvious move was to add `GSUB`
/// and `GPOS` to `tinker-pdf-font` beside `cmap` and `hmtx`, and it was
/// refused on the font crate's charter: that crate holds *the tables metrics
/// need*, and a lookup is not a metric. Nothing in `tinker-pdf-content`,
/// `tinker-pdf-render` or the facade asks a font what its ligatures are —
/// they ask how wide a glyph is and what its outline looks like — so putting
/// substitution rules there would widen a crate every layer above depends on,
/// for one caller. It would also put the lookup fuzz target in the wrong
/// crate: `fuzz_targets/shape.rs` drives the code it exercises directly,
/// which it could not if that code lived behind `tinker-pdf-font`'s API.
///
/// The three properties that made the earlier leaf-to-leaf edges acceptable
/// hold here, and the third of them is what makes the direction safe. It
/// points **sideways to a leaf** rather than upward, so the layering is not
/// inverted. It **cannot cycle**: `tinker-pdf-font` depends on
/// `tinker-pdf-filters`, `tinker-pdf-filters` on nothing, and neither has any
/// reason to know that shaping exists — an edge into a subtree with no path
/// back is a tree, whatever else it is. And a sibling workspace crate is not a
/// third-party dependency, so ruling 3 and CONTRIBUTING rule 1 are untouched:
/// setting Arabic adds no crate from outside this repository.
///
/// It is a leaf on ruling 8's definition rather than on any list: face table
/// bytes and glyph indices in, glyph indices and six integers out. There is no
/// PDF vocabulary in its API and no CSS vocabulary either — the consumers in
/// milestones 6 to 8 convert at their own boundaries.
/// **`cos -> pki` is the ninth amendment, and it is not a leaf-to-leaf edge
/// at all** — it is the same shape as `cos -> font`, which this file already
/// argues for, applied to a second leaf.
///
/// ISO 32000-1 7.6.5's public-key security handler derives its file key from a
/// seed sealed inside a CMS `EnvelopedData` in `/Recipients`. Two facts decide
/// where that lives. A security handler is `tinker-pdf-cos`'s charter: it owns
/// `/Encrypt`, `security.rs`, and the decryptor a document installs. And an
/// `EnvelopedData` is DER, which is `tinker-pdf-pki`'s, for the reasons the
/// seventh amendment gives about how structure-walking and arithmetic fail
/// differently.
///
/// The alternative was putting the handler in the facade, which already
/// depends on both. It was rejected on what it would have cost: installing a
/// decryptor is `CosDocument`'s own operation, so a facade-level handler needs
/// `set_decryptor_with_key` to become public — and a public "install this
/// decryptor" on an opened document is a hole with no floor under it, offered
/// so that a dependency edge could be avoided. An edge is cheaper than a
/// footgun.
///
/// It points downward from a non-leaf to a leaf and cannot cycle, because
/// `pki` depends only on `crypto` and `crypto` on nothing.
const ALLOWED: &[(&str, &[&str])] = &[
    // The bottom: nothing at all, internal or otherwise.
    ("tinker-pdf-math", &[]),
    // Leaves: nothing internal beyond the maths.
    ("tinker-pdf-filters", &[]),
    ("tinker-pdf-crypto", &[]),
    ("tinker-pdf-font", &["tinker-pdf-filters"]),
    // The fourth leaf-to-leaf edge: ASN.1 is structure walking on hostile
    // bytes, `crypto` is vector-gated arithmetic, and they are reviewed and
    // fuzzed differently. See the seventh amendment above.
    ("tinker-pdf-pki", &["tinker-pdf-crypto"]),
    ("tinker-pdf-zip", &["tinker-pdf-filters"]),
    // The eighth leaf, and the first with nothing under it since `crypto`.
    ("tinker-pdf-xml", &[]),
    // The ninth, and the fourth crate here with no internal dependency at all.
    // The empty list is what makes the compile-time proof of decision 5
    // possible; see the fifth amendment above.
    ("tinker-pdf-css", &[]),
    // The tenth, and the third leaf-to-leaf edge. **Not** `tinker-pdf-math`,
    // which gap 31's plan predicted and milestone 7 answered: nothing here is
    // transcendental. See the sixth amendment above.
    ("tinker-pdf-layout", &["tinker-pdf-css"]),
    // The eleventh, and the fourth leaf-to-leaf edge. It takes the sfnt table
    // directory and nothing else; the OpenType Layout tables are parsed here
    // rather than in `font` because that crate's charter is the tables
    // metrics need. See the eighth amendment above.
    ("tinker-pdf-shape", &["tinker-pdf-font"]),
    ("tinker-pdf-color", &["tinker-pdf-math"]),
    ("tinker-pdf-raster", &["tinker-pdf-math"]),
    // File syntax and the object model.
    (
        "tinker-pdf-cos",
        &[
            "tinker-pdf-filters",
            "tinker-pdf-crypto",
            "tinker-pdf-font",
            // The public-key security handler's envelope is DER. See the
            // ninth amendment above.
            "tinker-pdf-pki",
        ],
    ),
    // Content interpretation emits to a `Device`; it never rasterizes.
    (
        "tinker-pdf-content",
        &["tinker-pdf-cos", "tinker-pdf-font", "tinker-pdf-color"],
    ),
    (
        "tinker-pdf-render",
        &[
            "tinker-pdf-content",
            "tinker-pdf-raster",
            "tinker-pdf-font",
            "tinker-pdf-color",
        ],
    ),
    // The facade may reach anything; it is what users depend on.
    (
        "tinker-pdf",
        &[
            "tinker-pdf-cos",
            "tinker-pdf-font",
            "tinker-pdf-crypto",
            "tinker-pdf-content",
            "tinker-pdf-render",
            "tinker-pdf-raster",
            "tinker-pdf-filters",
            "tinker-pdf-color",
            "tinker-pdf-zip",
            "tinker-pdf-xml",
            "tinker-pdf-css",
            "tinker-pdf-layout",
            // Signatures milestone 3. The facade is where a signature verdict
            // is assembled (milestone 6), so it is the one crate that must be
            // able to turn a `/Contents` blob into a `SignedData` — and the
            // edge goes *down* into a leaf, which is the direction ruling 8
            // allows without argument. What needed the argument was the other
            // half: `tinker-pdf-pki` still has no PDF vocabulary and still
            // does not know what a document is, so adding this edge did not
            // buy the leaf a reason to acquire one.
            "tinker-pdf-pki",
            // Shaping milestone 6. The facade is where a *producing* path
            // meets a face: `BookMetrics` implements the layout crate's
            // `Shaper` seam over a book's own `@font-face` faces, and
            // `DocumentBuilder::glyph_run` writes what comes back. The edge
            // goes down into a leaf, which ruling 8 allows without argument,
            // and the direction matters: `tinker-pdf-layout` gains **no** edge
            // for this, because the trait is plain structs and `f64` and a
            // leaf that had acquired a shaper would have stopped being one.
            //
            // Nothing in `tinker-pdf-render` reaches it. Shaping while
            // *reading* a PDF stays the permanent non-goal it was — the
            // producer positioned every glyph and re-shaping them would be
            // wrong — and `docs/features/fonts.md` keeps that row.
            "tinker-pdf-shape",
        ],
    ),
    // Ruling 11: bindings sit on the facade only.
    ("tinker-pdf-ffi", &["tinker-pdf"]),
];

/// Crates outside `crates/` and the internal edges each may have, by path from
/// the repository root.
///
/// Paths rather than bare names because `xtask` does not live under `tools/`.
/// It was listed as `"xtask"` and looked for at `tools/xtask/Cargo.toml`,
/// which does not exist, so the manifest read failed, the loop moved on, and
/// xtask's own dependencies were never checked at all — by a check whose
/// entire purpose is that the compiler cannot do this.
///
/// Crates outside `crates/`, and the internal edges each may have.
///
/// The rule is "the facade only", and it exists so that a tool exercises what
/// a user gets rather than reaching past the API into an implementation
/// detail. `xtask` is the one exception and it is spelled out rather than
/// waved through: it is not a tool users get, it is repository automation, and
/// it is the *only* thing here that never touches a PDF. What it needs from
/// the workspace is a SHA-256 to verify a fetched corpus archive against the
/// digit string in the lock, and taking a third-party one — or writing a
/// second one — in a project premised on implementing its own primitives would
/// be the wrong way round twice over.
///
/// It deliberately does not depend on `tinker-pdf`. `cargo xtask check` reads
/// manifests and counts lines; making it link the engine would put a full
/// engine build in front of every dependency-graph check, and the corpus
/// runner does not need it either, because it spawns `tpdf` rather than
/// opening documents itself.
const TOOLS: &[(&str, &[&str])] = &[
    ("tools/pdfcmp", &["tinker-pdf"]),
    ("tools/tpdf", &["tinker-pdf"]),
    ("xtask", &["tinker-pdf-crypto"]),
];

/// Prints a task's outcome and turns it into an exit code.
fn report(task: &str, outcome: Result<(), Vec<String>>) -> ExitCode {
    match outcome {
        Ok(()) => {
            println!("{task}: ok");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            for problem in &problems {
                eprintln!("{task}: {problem}");
            }
            eprintln!("{task}: {} problem(s)", problems.len());
            ExitCode::FAILURE
        }
    }
}

/// The crates whose output is pixels, and which therefore may not call the
/// platform's transcendental functions.
///
/// Ruling 4 wants byte-identical rendering across targets. `sqrt` and the
/// rounding family are safe — IEEE 754 requires them to be correctly rounded,
/// so every platform agrees. The functions below are not, and glibc, musl,
/// the MSVC runtime, Apple's libm and the wasm shim each round them their own
/// way. `tinker-pdf-math` exists to replace them; this makes sure nobody
/// quietly goes back.
const PIXEL_PATHS: &[&str] = &[
    "tinker-pdf-raster",
    "tinker-pdf-color",
    "tinker-pdf-render",
    "tinker-pdf-content",
];

/// Method calls that are not correctly rounded, and so differ between
/// platforms. Spelled with the dot so `f.exp()` matches and a local named
/// `exp` does not.
const FORBIDDEN: &[&str] = &[
    ".sin()",
    ".cos()",
    ".tan()",
    ".asin()",
    ".acos()",
    ".atan()",
    ".atan2(",
    ".sinh()",
    ".cosh()",
    ".tanh()",
    ".exp()",
    ".exp2()",
    ".exp_m1()",
    ".ln()",
    ".ln_1p()",
    ".log(",
    ".log2()",
    ".log10()",
    ".powf(",
    ".cbrt()",
    ".hypot(",
    ".to_radians()",
    ".to_degrees()",
    ".mul_add(",
];

fn check_libm() -> Result<(), Vec<String>> {
    let root = repo_root();
    let mut problems = Vec::new();

    for crate_name in PIXEL_PATHS {
        let src = root.join("crates").join(crate_name).join("src");
        let mut files = Vec::new();
        collect_rust_files(&src, &mut files);

        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            // Tests may compare against the platform — that is how the maths
            // crate proves it agrees with one. Only shipped code is bound.
            let shipped = text.split("#[cfg(test)]").next().unwrap_or(&text);

            for (number, line) in shipped.lines().enumerate() {
                let code = line.split("//").next().unwrap_or(line);
                for call in FORBIDDEN {
                    if code.contains(call) {
                        let shown = file
                            .strip_prefix(&root)
                            .unwrap_or(&file)
                            .display()
                            .to_string();
                        problems.push(format!(
                            "{shown}:{}: `{call}` is not correctly rounded, so it differs between platforms; use tinker_pdf_math (ruling 4)",
                            number + 1
                        ));
                    }
                }
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every file permitted to spawn a program, and the reason it may.
///
/// Ruling 13: nothing outside this repository renders, parses, validates or
/// measures a document as evidence. A third-party program may host this code,
/// execute it, fetch bytes for it or generate inputs for it — it may never be
/// the thing that says whether the output is right.
///
/// **The reason is a field, not a comment, and no row opts out.** That is the
/// `bounds_ledger.rs` discipline, for the same reason: a row that opts out of
/// saying why is a row nobody re-reads. Rows below fall into two classes, and
/// the difference is the whole point of this check:
///
/// - **Permanent.** The program supplies or hosts. `curl` and `tar` move
///   bytes this repository then verifies against its own SHA-256; `cargo`
///   builds and publishes; the child the corpus runner spawns is a workspace
///   binary built from the same revision.
/// - **A debt, with a milestone against it.** The XPS render comparison, the
///   browser and epubcheck. Each is an oracle of retired ruling 9, still
///   running because ruling 13's order is fixed: nothing is deleted before the
///   first-party check replacing it exists and has been injection-counted.
///   Each row names the step of the roadmap's first-party-verification item
///   that removes it. The four qpdf tests left in step 3 with the strict
///   validator, and `xps_mutool.rs` left in step 4 with the conservation
///   suite — every one of their rows leaving in the same commit as the test it
///   allowed, which is the half of this check that catches a stale allowance.
///
/// The check runs both ways. A file that spawns something and is not here is
/// a build failure — that is the boundary. And a row here whose file no
/// longer spawns anything is *also* a failure, because that is what makes an
/// allowance leave in the same commit as the thing it allowed, instead of
/// standing for a year after the debt is paid.
const SPAWNERS: &[(&str, &str)] = &[
    (
        "crates/tinker-pdf-css/tests/unimplemented_property_does_not_build.rs",
        "PERMANENT: spawns `rustc` on a snippet that must fail to compile. It \
         adjudicates nothing about a document — it asks this repository's own \
         compiler whether this repository's own type refuses a state",
    ),
    (
        "crates/tinker-pdf-layout/tests/uncascaded_field_does_not_build.rs",
        "PERMANENT: the same compile-refusal proof for a layout field",
    ),
    (
        "xtask/src/fetch.rs",
        "PERMANENT: `curl` and `tar` fetch and unpack the pinned corpora. \
         Supplying, not adjudicating — and nothing fetched is trusted: the \
         archive is verified against this project's own SHA-256 before it is \
         unpacked",
    ),
    (
        "xtask/src/release.rs",
        "PERMANENT: `cargo`, and the packaging tools, to build and publish. \
         Build machinery, which reads no document",
    ),
    (
        "xtask/src/runner.rs",
        "PERMANENT: spawns the corpus child, which is `tpdf` built from this \
         revision into the same directory as the runner. A workspace binary, \
         resolved as a sibling rather than from PATH for exactly that reason",
    ),
    (
        "xtask/src/corpus.rs",
        "PERMANENT: asks that same sibling `tpdf` what record format it writes, \
         once, before a run spawns it per file. It adjudicates nothing about a \
         document — the question is what this repository's own binary is, and \
         the answer only decides whether to refuse the run",
    ),
    (
        "xtask/src/parity.rs",
        "PERMANENT: spawns `cargo`, `python`, `node` and `dotnet` as *hosts* \
         for this repository's own code. Each loads this engine — as a cargo \
         example, an installed wheel, an installed npm package, an installed \
         NuGet package — and runs a script this repository wrote. Ruling 13's \
         line is between hosting and adjudicating, and every judgement here is \
         first-party: the bytes are hashed against a number recorded in \
         parity.rs, and each surface re-opens its own artefact through this \
         engine's strict structural validator before reporting one. No \
         third-party program reads, writes or judges a document",
    ),
];

/// Ruling 13's boundary, held by a build failure rather than by habit.
///
/// The rule this enforces is not "do not call `Command::new`". It is that
/// every place this repository starts a program is written down with a reason,
/// in one list, so that adding an outside adjudicator is a diff somebody
/// reviews instead of a line nobody notices. The oracles it is retiring
/// arrived one plausible commit at a time.
///
/// Deliberately a text scan rather than anything cleverer: an `#[ignore]`d
/// test, a `cfg`-ed module and a helper behind three layers of indirection all
/// spawn just as effectively as a plain call, and a check that only sees what
/// compiles on this target is a check with holes on the others.
///
/// **What it does not see, said here rather than discovered later:** Rust is
/// the only language it reads. `bindings/js/demo/verify.mjs` drives a headless
/// browser through Playwright and this check cannot see it — legitimately, as
/// it happens, because that browser *hosts* the wasm demo and reports whether
/// ink landed, which is executing rather than adjudicating. But the reason it
/// passes is a reading of what it does, not a thing this check established,
/// and the same would be true of a `.mjs` that did adjudicate. A gate that
/// covers one language and is described as covering the boundary is the kind
/// of claim ruling 13's own sweep was written to find.
fn check_oracles() -> Result<(), Vec<String>> {
    let root = repo_root();
    let mut problems = Vec::new();

    let mut files = Vec::new();
    for tree in ["crates", "tools", "xtask", "fuzz/fuzz_targets", "bindings"] {
        collect_rust_files(&root.join(tree), &mut files);
    }
    files.sort();

    // Spelled in two pieces so that this function does not match itself. The
    // alternative is to add this file to the allowance list, which would be a
    // lie about what it does — and a scanner that reports its own needle is
    // one nobody trusts the second time.
    let needle = concat!("Command", "::new");

    let mut spawns = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            // Never a silent skip. A file this cannot read is a file this
            // cannot vouch for, and vouching is the entire job.
            problems.push(format!(
                "{}: could not be read, so it cannot be checked",
                shown(&root, file)
            ));
            continue;
        };
        // Comments are stripped so that a doc comment *about* spawning — the
        // one this very function has — does not register as spawning.
        if !text
            .lines()
            .filter_map(|line| line.split("//").next())
            .any(|code| code.contains(needle))
        {
            continue;
        }
        spawns.push(shown(&root, file));
    }

    let allowed: BTreeMap<&str, &str> = SPAWNERS.iter().copied().collect();
    for file in &spawns {
        if !allowed.contains_key(file.as_str()) {
            problems.push(format!(
                "{file}: spawns a program, and ruling 13 says nothing outside \
                 this repository may adjudicate. If it supplies or hosts \
                 rather than adjudicates, add it to `SPAWNERS` in \
                 xtask/src/main.rs with the reason it may",
            ));
        }
    }
    for (file, _) in SPAWNERS {
        if !spawns.iter().any(|found| found == file) {
            problems.push(format!(
                "{file}: is allowed to spawn a program and no longer spawns \
                 one. Remove the row — an allowance that outlives its need is \
                 how the next one gets in",
            ));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        problems.sort();
        Err(problems)
    }
}

/// A path as this repository names it: relative to the root, forward slashes,
/// so a problem reads the same on Windows as in CI.
fn shown(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Vendored data, checked against the two files that are supposed to describe
/// it.
///
/// `cargo deny check licenses` reads the *crate* graph. A directory of text
/// files is not in that graph, so an eight-megabyte BSD-3-Clause asset can sit
/// inside a crate declaring `MIT OR Apache-2.0` and every licence check in the
/// repository passes. That is the code/data distinction, and this is the half
/// of it cargo-deny cannot do.
///
/// Three rules, each of which has been somebody's incident somewhere:
///
/// - a vendored tree that nothing declares — the licence arrives in the
///   repository and no released artefact mentions it;
/// - a declared tree with no licence text beside the data, so the copy is not
///   self-describing once the file it was named in moves;
/// - an SPDX identifier `deny.toml` does not allow, which is the whole point:
///   data the project could not ship must fail the same allowlist a crate
///   licence would.
fn check_vendor() -> Result<(), Vec<String>> {
    let root = repo_root();
    let mut problems = Vec::new();

    let manifest = root.join("THIRDPARTY.md");
    let text = match std::fs::read_to_string(&manifest) {
        Ok(text) => text,
        Err(error) => {
            return Err(vec![format!(
                "THIRDPARTY.md could not be read ({error}); vendored data has \
                 nowhere to be declared"
            )])
        }
    };
    let allowed = allowed_licenses(&root);
    let declared = declared_vendor_trees(&text);

    for (path, spdx) in &declared {
        let dir = root.join(path);
        if !dir.is_dir() {
            problems.push(format!(
                "THIRDPARTY.md declares {path}, which is not a directory"
            ));
            continue;
        }
        if !has_license_file(&dir) {
            problems.push(format!(
                "{path} carries no LICENSE file, so the copy does not describe \
                 its own terms"
            ));
        }
        if !allowed.contains(spdx) {
            problems.push(format!(
                "{path} is {spdx}, which deny.toml does not allow — data is \
                 held to the same allowlist as a crate"
            ));
        }
    }

    for tree in vendor_trees(&root) {
        if !declared.iter().any(|(path, _)| *path == tree) {
            problems.push(format!(
                "{tree} is vendored and not declared in THIRDPARTY.md"
            ));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every `crates/<crate>/data/<tree>` directory: one vendored upstream each.
fn vendor_trees(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(crates) = std::fs::read_dir(root.join("crates")) else {
        return out;
    };
    for entry in crates.flatten() {
        let Ok(trees) = std::fs::read_dir(entry.path().join("data")) else {
            continue;
        };
        for tree in trees.flatten() {
            if !tree.path().is_dir() {
                continue;
            }
            if let Ok(rel) = tree.path().strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

fn has_license_file(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_name()
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("LICENSE")
    })
}

/// The `| path | upstream | SPDX |` rows of THIRDPARTY.md's table.
fn declared_vendor_trees(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().trim_matches('`').trim())
            .collect();
        if cells.len() < 3 {
            continue;
        }
        let (path, spdx) = (cells[0], cells[cells.len() - 1]);
        // The header row and the `---` separator are shaped like data rows.
        if !path.starts_with("crates/") {
            continue;
        }
        out.push((path.to_string(), spdx.to_string()));
    }
    out
}

/// `deny.toml`'s `allow = [...]`, as identifiers.
///
/// Read line by line rather than as TOML for the same reason `package_name`
/// is: adding a dependency in order to check the dependency rules would be
/// its own kind of funny. Entries carry trailing comments, so the identifier
/// is whatever sits inside the quotes.
fn allowed_licenses(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("deny.toml")) else {
        return Vec::new();
    };
    allowed_licenses_in(&text)
}

/// The same reading, over text, so it can be tested on inputs this repository
/// does not happen to contain.
///
/// Split out when the allowlist stopped containing a commented-out entry: the
/// test for "a comment is not an allowance" had been written against
/// `deny.toml`'s own OFL-1.1 line, so the day that line became real the test
/// asserted the project's font policy rather than the parser's behaviour, and
/// failed for the change it should have been indifferent to.
fn allowed_licenses_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_allow = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("allow") && line.contains('[') {
            in_allow = true;
            continue;
        }
        if !in_allow {
            continue;
        }
        if line.starts_with(']') {
            break;
        }
        // A commented-out entry is not an allowance, and this file has one.
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.split_once('"') {
            if let Some((id, _)) = rest.1.split_once('"') {
                out.push(id.to_string());
            }
        }
    }
    out
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn check_dag() -> Result<(), Vec<String>> {
    let root = repo_root();
    let crates_dir = root.join("crates");

    let mut problems = Vec::new();
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let entries = match std::fs::read_dir(&crates_dir) {
        Ok(entries) => entries,
        Err(e) => return Err(vec![format!("cannot read {}: {e}", crates_dir.display())]),
    };

    for entry in entries.flatten() {
        let manifest = entry.path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Some(name) = package_name(&text) else {
            problems.push(format!("{} has no [package] name", manifest.display()));
            continue;
        };
        seen.insert(name, internal_dependencies(&text));
    }

    for (name, deps) in &seen {
        let Some((_, allowed)) = ALLOWED.iter().find(|(n, _)| n == name) else {
            problems.push(format!(
                "{name} is not in the declared graph — add it to ALLOWED with the \
                 reason, or it is an accident"
            ));
            continue;
        };
        for dep in deps {
            if !allowed.contains(&dep.as_str()) {
                problems.push(format!(
                    "{name} -> {dep} is not a declared edge (plan 00, ruling 8)"
                ));
            }
        }
    }

    for (name, _) in ALLOWED {
        if !seen.contains_key(*name) {
            problems.push(format!("{name} is declared but no such crate exists"));
        }
    }

    // The tools and bindings are checked only for the one rule that matters
    // for them: ruling 11 keeps a binding on the facade alone.
    for (tool, allowed) in TOOLS {
        let manifest = root.join(tool).join("Cargo.toml");
        let text = match std::fs::read_to_string(&manifest) {
            Ok(text) => text,
            // Not a skip. A check that quietly passes over what it cannot
            // find is a check that does not run, and this one already spent
            // its whole life doing exactly that to `xtask`.
            Err(error) => {
                problems.push(format!(
                    "{tool}/Cargo.toml could not be read ({error}), so its \
                     dependencies went unchecked"
                ));
                continue;
            }
        };
        for dep in internal_dependencies(&text) {
            if !allowed.contains(&dep.as_str()) {
                problems.push(format!(
                    "{tool} -> {dep} is not one of its declared edges ({}); \
                     tools use the facade, so that they exercise what users get",
                    allowed.join(", ")
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The check runs against this repository, which is the point of it.
    ///
    /// Both directions at once: a file that spawns something and is not on
    /// the list fails here, and so does a row on the list whose file has
    /// stopped spawning. The second half is what makes an allowance leave in
    /// the same commit as the oracle it allowed.
    #[test]
    fn this_repository_writes_down_every_program_it_spawns() {
        if let Err(problems) = check_oracles() {
            panic!(
                "ruling 13's boundary has moved:
{problems:#?}"
            );
        }
    }

    /// Every row says why, and says which kind of why.
    ///
    /// The `bounds_ledger.rs` doctrine: a row that opts out of a check is a
    /// row that is not checked. `PERMANENT` means the program supplies or
    /// hosts and stays; `DEBT` means it adjudicates, is on its way out, and
    /// names the roadmap step that removes it. A row that is neither has not
    /// been thought about.
    #[test]
    fn every_spawner_says_why_and_says_which_kind() {
        for (file, reason) in SPAWNERS {
            assert!(
                reason.starts_with("PERMANENT: ") || reason.starts_with("DEBT ("),
                "{file}: a reason must begin `PERMANENT: ` or `DEBT (step N): `, so that a debt cannot be filed as a fact of life: {reason}"
            );
            assert!(
                reason.len() > 40,
                "{file}: `{reason}` does not say enough to be re-read in a year"
            );
        }
    }

    /// A file named twice would let one row be deleted and the allowance
    /// survive, which is precisely the failure the second direction of the
    /// check exists to prevent.
    #[test]
    fn no_spawner_is_listed_twice() {
        let mut seen = std::collections::BTreeSet::new();
        for (file, _) in SPAWNERS {
            assert!(seen.insert(*file), "{file} appears twice in SPAWNERS");
        }
    }

    #[test]
    fn this_repository_obeys_its_own_graph() {
        if let Err(problems) = check_dag() {
            panic!("the declared graph and the manifests disagree:\n{problems:#?}");
        }
    }

    #[test]
    fn a_vendor_row_yields_its_path_and_licence() {
        let text = "| Path | Upstream | SPDX |\n\
                    | --- | --- | --- |\n\
                    | `crates/tinker-pdf-font/data/cmap-resources` | [x](y) | `BSD-3-Clause` |\n";
        assert_eq!(
            declared_vendor_trees(text),
            vec![(
                "crates/tinker-pdf-font/data/cmap-resources".to_string(),
                "BSD-3-Clause".to_string()
            )]
        );
    }

    /// The header and the separator are shaped exactly like data rows, and
    /// prose tables elsewhere in the file are not vendor declarations at all.
    #[test]
    fn only_rows_naming_a_crate_path_are_declarations() {
        let text = "| Data | Source license | Handling |\n| --- | --- | --- |\n\
                    | AGL + AGLFN | BSD-3-Clause | table |\n";
        assert!(declared_vendor_trees(text).is_empty());
    }

    /// A commented-out allowlist entry is not an allowance.
    ///
    /// Reading one as live would let a licence the project has decided against
    /// pass this gate on the strength of a paragraph explaining why it does
    /// not ship — and `deny.toml`'s allowlist is mostly paragraphs.
    #[test]
    fn a_commented_allowlist_entry_does_not_allow() {
        let text = "[licenses]
allow = [
  \"MIT\",
  # \"GPL-3.0\",
                      \"Zlib\",
]
";
        let allowed = allowed_licenses_in(text);
        assert_eq!(allowed, vec!["MIT".to_string(), "Zlib".to_string()]);
    }

    /// And this repository's own allowlist reads as it looks.
    ///
    /// A separate test from the one above, because they answer different
    /// questions: that one is about the parser and stays true whatever is
    /// allowed, this one is about what is allowed and changes when the project
    /// changes its mind. Merging them is how the parser's test came to fail on
    /// the day the fonts arrived.
    #[test]
    fn this_repositorys_allowlist_reads_as_it_looks() {
        let allowed = allowed_licenses(&repo_root());
        for spdx in [
            "MIT",
            "Apache-2.0",
            "BSD-3-Clause",
            "Unicode-3.0",
            "OFL-1.1",
        ] {
            assert!(allowed.contains(&spdx.to_string()), "{spdx}: {allowed:?}");
        }
        assert!(!allowed.iter().any(|id| id.contains("GPL")), "{allowed:?}");
    }

    /// The same rule as the graph check: it runs against this repository.
    #[test]
    fn this_repository_declares_the_data_it_vendors() {
        if let Err(problems) = check_vendor() {
            panic!("vendored data and THIRDPARTY.md disagree:\n{problems:#?}");
        }
    }
}
