//! The committed `pki_der` and `pki_cms` fuzz seeds, replayed on stable.
//!
//! `fuzz/corpus/pki_der/` and `fuzz/corpus/pki_cms/` are twenty-three inputs —
//! twenty-two written from the fixtures in this crate and one the first
//! `pki_der` session found — and the targets that consume them
//! need nightly and a sanitizer runtime. So the seeds were only ever exercised
//! when somebody ran `cargo fuzz`, which is not on every commit — and a seed
//! corpus nothing reads is a corpus that stops describing the parser without
//! anybody noticing. The same argument
//! `crates/tinker-pdf-filters/tests/jbig2_seeds.rs` makes, and the same
//! arrangement.
//!
//! This replays each seed through the same limits and the same assertions the
//! targets make, minus the mutation. It is not fuzzing and does not pretend to
//! be: it is a regression test over inputs that were once interesting, which is
//! what a seed corpus is. It prints `RAN` or `SKIPPED` for the reason every
//! check that can be absent does
//! ([verification](../../../docs/verification.md)).
//!
//! The six seeds that exist for X.690 §8.1.3.6's indefinite length are the
//! reason this file was written now: the form is read only under
//! `Limits::CMS`, so a target pass that leaves it off exercises none of the
//! end-of-contents scan, and a seed corpus nobody replays would not have said
//! so.

use std::path::{Path, PathBuf};

use tinker_pdf_pki::cms::ContentInfo;
use tinker_pdf_pki::der::{Budget, Cursor, DerError, Limits, Tag, Tlv};
use tinker_pdf_pki::x509::Certificate;

/// `fuzz/fuzz_targets/pki_der.rs`'s own ceilings.
const WALK: Limits = Limits::new(12, 4_096);
/// `fuzz/fuzz_targets/pki_cms.rs`'s.
const CMS_WALK: Limits = Limits::new(40, 16_384).allowing_indefinite_lengths();

fn seeds(target: &str) -> Option<Vec<(String, Vec<u8>)>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(target);
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let path: PathBuf = entry.path();
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((name, std::fs::read(&path).ok()?))
        })
        .collect();
    out.sort();
    (!out.is_empty()).then_some(out)
}

/// Whether `slice` is a subslice of `data`, by address — the target's own test
/// that a located encoding really is part of what was parsed.
fn inside(data: &[u8], slice: &[u8]) -> bool {
    let base = data.as_ptr().addr();
    let at = slice.as_ptr().addr();
    at >= base && at.saturating_add(slice.len()) <= base.saturating_add(data.len())
}

/// The geometry every node must satisfy, whatever the input said.
///
/// Restated from `pki_der.rs` rather than shared with it, so that a seed means
/// here exactly what it means there.
fn walk(name: &str, data: &[u8], limits: Limits) -> usize {
    let budget = Budget::new(limits);
    let mut stack: Vec<(Cursor<'_, '_>, Option<Tlv<'_>>)> =
        vec![(Cursor::new(data, &budget), None)];
    let mut nodes = 0usize;

    while let Some((mut cursor, parent)) = stack.pop() {
        let before = cursor.offset();
        let Ok(node) = cursor.read() else {
            continue;
        };
        nodes += 1;

        assert_eq!(node.start(), before, "{name}: a node moved");
        assert_eq!(node.range(), node.start()..node.end(), "{name}");
        assert_eq!(node.end() - node.start(), node.raw().len(), "{name}");
        assert!(node.raw().len() >= node.value().len(), "{name}");
        assert_eq!(cursor.offset(), node.end(), "{name}: the cursor overran");
        assert!(node.depth() <= limits.max_depth, "{name}: past the cap");
        assert!(inside(data, node.raw()), "{name}: a node outside the input");

        if node.is_indefinite() {
            assert!(
                limits.allow_indefinite_lengths,
                "{name}: an indefinite length read by a parse that forbade one"
            );
            assert!(node.is_constructed(), "{name}: X.690 §8.1.3.2");
            assert!(
                node.raw().len() >= node.value().len() + 4,
                "{name}: no room for a header and a terminator"
            );
            assert_eq!(
                node.raw().get(node.raw().len() - 2..),
                Some(&[0x00, 0x00][..]),
                "{name}: an indefinite node not ending in its terminator"
            );
        }
        // Where the sweep says a subtree is definite, no node in it may say
        // otherwise, read the other way — the property `cms.rs` refuses a BER
        // `signedAttrs` on, and the one the first session's crash was.
        if node.require_definite_lengths(&budget).is_ok() {
            assert!(!node.is_indefinite(), "{name}");
            if let Ok(mut inner) = node.children(&budget) {
                while let Ok(child) = inner.read() {
                    assert!(
                        !child.is_indefinite(),
                        "{name}: a subtree called definite holds an indefinite node"
                    );
                }
            }
        }
        if let Some(parent) = parent {
            assert!(
                node.start() >= parent.start() && node.end() <= parent.end(),
                "{name}: a child outside its parent"
            );
        }

        // Every reader on every node: a refusal path is as much of the
        // surface as an acceptance path.
        let _ = node.as_bool();
        let _ = node
            .as_integer()
            .map(|i| (i.as_u64(), i.as_i64(), i.to_hex()));
        let _ = node.as_octet_string();
        let _ = node.as_null();
        let _ = node.as_bit_string().map(|b| {
            let _ = b.whole_bytes();
            let _ = b.bit(usize::MAX);
            b.bit_len()
        });
        let _ = node.as_oid().map(|o| (o.to_dotted(), o.arcs().count()));
        let _ = node.as_string();
        let _ = node.as_time();

        stack.push((cursor, parent));
        if node.is_constructed() {
            if let Ok(inner) = node.children(&budget) {
                stack.push((inner, Some(node)));
            }
        }
    }
    nodes
}

/// **Every `pki_der` seed walks to a value, under four sets of ceilings.**
#[test]
fn the_der_seeds_walk_without_panicking_and_keep_their_geometry() {
    let Some(seeds) = seeds("pki_der") else {
        println!("SKIPPED (no fuzz/corpus/pki_der)");
        return;
    };
    println!("RAN over {} pki_der seeds", seeds.len());
    for (name, data) in &seeds {
        let der_only = walk(name, data, WALK);
        walk(name, data, Limits::new(1, 4_096));
        let ber = walk(name, data, WALK.allowing_indefinite_lengths());
        walk(
            name,
            data,
            Limits::new(2, 4_096).allowing_indefinite_lengths(),
        );
        println!("  {name:32} {der_only:4} nodes DER-only, {ber:4} with BER read");
        let _ = Certificate::parse(data);
    }

    // The six seeds that exist for the indefinite length must actually reach
    // it — a seed that no longer exercises what it was chosen for looks
    // exactly like one that does.
    let named: Vec<&str> = seeds.iter().map(|(name, _)| name.as_str()).collect();
    for wanted in [
        "indefinite-length",
        "indefinite-nested",
        "indefinite-unterminated",
        "indefinite-primitive",
        "indefinite-bad-end-of-contents",
        "indefinite-deep-nesting",
    ] {
        assert!(named.contains(&wanted), "the {wanted} seed is missing");
    }
}

/// **Each indefinite-length seed reaches the refusal it was written for.**
///
/// A seed corpus whose members all land in one error is a corpus that has
/// stopped covering anything, and that is invisible without naming the answers.
#[test]
fn each_indefinite_length_seed_reaches_its_own_answer() {
    let Some(seeds) = seeds("pki_der") else {
        println!("SKIPPED (no fuzz/corpus/pki_der)");
        return;
    };
    let ber = Limits::new(12, 4_096).allowing_indefinite_lengths();
    let read = |data: &[u8], limits: Limits| {
        let budget = Budget::new(limits);
        let mut cursor = Cursor::new(data, &budget);
        cursor.read().map(|node| node.range())
    };
    let find = |wanted: &str| {
        seeds
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, data)| data.clone())
            .unwrap_or_else(|| panic!("the {wanted} seed"))
    };

    println!("RAN");
    // With the form off, every one of them is the same refusal — which is what
    // makes "off by default" a guarantee rather than a hope.
    for name in [
        "indefinite-length",
        "indefinite-nested",
        "indefinite-unterminated",
        "indefinite-primitive",
        "indefinite-bad-end-of-contents",
        "indefinite-deep-nesting",
    ] {
        assert_eq!(
            read(&find(name), WALK),
            Err(DerError::IndefiniteLength),
            "{name} under DER-only ceilings"
        );
    }

    // And with it on, five different answers.
    assert_eq!(read(&find("indefinite-length"), ber), Ok(0..7));
    assert_eq!(
        read(&find("indefinite-nested"), ber),
        Ok(0..17),
        "the `00 00` inside the INTEGER's content is content, not a terminator"
    );
    assert_eq!(
        read(&find("indefinite-unterminated"), ber),
        Err(DerError::UnterminatedIndefiniteLength)
    );
    assert_eq!(
        read(&find("indefinite-primitive"), ber),
        Err(DerError::IndefinitePrimitive { tag: 4 })
    );
    assert_eq!(
        read(&find("indefinite-bad-end-of-contents"), ber),
        Err(DerError::MalformedEndOfContents { length_octet: 0x01 })
    );

    // Forty terminated levels in 160 octets: the depth ceiling has to be
    // applied *during* the scan, because the outermost node cannot say where
    // it ends without walking all forty. Read under ceilings that admit them
    // and it parses, so the refusal below is the cap and not the shape.
    let deep = find("indefinite-deep-nesting");
    assert_eq!(
        read(&deep, ber),
        Err(DerError::DepthExceeded),
        "an indefinite length moves the depth ceiling from descent time to \
         read time, and this is the seed that says so"
    );
    assert_eq!(
        read(&deep, Limits::new(64, 4_096).allowing_indefinite_lengths()),
        Ok(0..160),
        "and the same bytes read under ceilings that admit forty levels"
    );

    // The one seed here that a session found rather than a fixture wrote. Its
    // whole point is the verdict: a sweep bounded only by the outermost node
    // steps over the `30 80` at offset 23 and calls the subtree definite,
    // which is a BER `signedAttrs` inside RFC 5652 §5.4's digest.
    let stepped_over = find("sweep-over-an-indefinite-sibling");
    let budget = Budget::new(ber);
    let mut cursor = Cursor::new(&stepped_over, &budget);
    let outer = cursor.read().expect("the outermost SEQUENCE parses");
    assert_eq!(
        outer.require_definite_lengths(&budget),
        Err(DerError::IndefiniteLength),
        "the subtree holds an indefinite-length node that a flat sweep can \
         walk straight past"
    );
}

/// **Every `pki_cms` seed parses or refuses, and what it digests is DER.**
#[test]
fn the_cms_seeds_parse_or_refuse_and_never_digest_ber() {
    let Some(seeds) = seeds("pki_cms") else {
        println!("SKIPPED (no fuzz/corpus/pki_cms)");
        return;
    };
    println!("RAN over {} pki_cms seeds", seeds.len());
    let mut ber_seeds = 0usize;

    for (name, data) in &seeds {
        for limits in [
            CMS_WALK,
            Limits::new(40, 16_384),
            Limits::new(2, 16_384).allowing_indefinite_lengths(),
            Limits::new(40, 8).allowing_indefinite_lengths(),
        ] {
            let Ok(info) = ContentInfo::parse_with(data, limits) else {
                continue;
            };
            assert!(inside(data, info.der()), "{name}");
            for signer in info.signed_data().signer_infos() {
                assert!(inside(data, signer.signature()), "{name}");
                let Some(attributes) = signer.signed_attrs() else {
                    assert!(signer.signed_attrs_to_digest().is_none(), "{name}");
                    continue;
                };
                // RFC 5652 §5.4, which is the property widening the walker to
                // BER could have broken silently.
                let budget = Budget::new(Limits::new(64, 1 << 20).allowing_indefinite_lengths());
                let mut cursor = Cursor::new(attributes.stored_der(), &budget);
                let stored = cursor.read().expect("the set parsed once");
                assert!(!stored.is_indefinite(), "{name}: a BER signedAttrs");
                assert_eq!(
                    stored.require_definite_lengths(&budget),
                    Ok(()),
                    "{name}: an indefinite length inside what §5.4 digests"
                );

                let digested = signer.signed_attrs_to_digest().expect("there are some");
                assert_eq!(digested.len(), attributes.stored_der().len(), "{name}");
                assert_eq!(digested.first(), Some(&0x31), "{name}");
                assert_eq!(
                    digested.get(1..),
                    attributes.stored_der().get(1..),
                    "{name}: something other than the tag octet moved"
                );
                let mut cursor = Cursor::new(&digested, &budget);
                let node = cursor.expect(Tag::Set).expect("a universal SET");
                assert!(cursor.finish().is_ok(), "{name}");
                assert_eq!(node.raw().len(), digested.len(), "{name}");
                assert!(!node.is_indefinite(), "{name}");
            }
        }

        // The BER seed reads under `Limits::CMS` and refuses under the DER
        // ceilings every other caller uses. Both halves, or the opt-in is not
        // one.
        if name == "ber-indefinite-length" {
            ber_seeds += 1;
            assert!(
                ContentInfo::parse_with(data, CMS_WALK).is_ok(),
                "{name} must read under CMS ceilings"
            );
            assert!(
                ContentInfo::parse_with(data, Limits::new(40, 16_384)).is_err(),
                "{name} must refuse under DER-only ceilings"
            );
        }
        if name == "ber-indefinite-signed-attrs" {
            ber_seeds += 1;
            let error = ContentInfo::parse_with(data, CMS_WALK).expect_err("refused");
            assert_eq!(
                format!("{error:?}"),
                "IndefiniteSignedAttributes",
                "{name} must be refused for its attributes, not its envelope"
            );
        }
        println!("  {name}");
    }
    assert_eq!(ber_seeds, 2, "both BER seeds are present and checked");
}
