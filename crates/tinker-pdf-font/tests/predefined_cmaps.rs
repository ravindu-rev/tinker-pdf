//! The predefined CMaps of 9.7.5.2, checked against Adobe's own text.
//!
//! Every test here needs the compiled tables, so the file is compiled only
//! with `cmap-predefined`. What a build *without* it does is
//! `tests/predefined_cmaps_absent.rs`, and the two files together are why
//! `--no-default-features` is a leg of the gate rather than a build that
//! merely links.
//!
//! Gap 03's risk table names the failure this file exists to prevent: "a
//! generated table is wrong in a way no test notices", mitigated by testing
//! "against the registry's own published mappings, not against our own
//! output". A round trip through `build.rs`'s encoder and back proves that
//! the encoder is self-consistent and nothing else — an off-by-one in the
//! delta scheme survives it perfectly.
//!
//! So there are two kinds of test here and both read the registry.
//!
//! The first quotes it. A handful of lines are copied verbatim out of
//! `data/cmap-resources/...` into the test source, with the arithmetic done
//! by hand, so a reviewer can check a claim against the vendored file without
//! running anything.
//!
//! The second re-derives it. `declared` below is a second, deliberately
//! different reader of the same files — line-oriented, no tokenizer, no
//! delta encoding, no deflate — and every `cidrange` and `cidchar` in the
//! whole vendored set is checked through it. Two implementations that
//! disagree cannot both be the registry.

#![cfg(feature = "cmap-predefined")]

use std::path::PathBuf;

use tinker_pdf_font::CMap;

/// Where the vendored registry lives.
fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/cmap-resources")
}

/// Every CMap file in the vendored set, as `(name, source)`.
fn registry() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![data()];
    while let Some(dir) = stack.pop() {
        let is_cmap_dir = dir.file_name().is_some_and(|d| d == "CMap");
        for entry in std::fs::read_dir(&dir)
            .expect("the vendored tree is readable")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_cmap_dir {
                let name = path
                    .file_name()
                    .expect("a file has a name")
                    .to_string_lossy()
                    .into_owned();
                out.push((name, std::fs::read_to_string(&path).expect("ASCII text")));
            }
        }
    }
    out.sort();
    assert_eq!(out.len(), 202, "the vendored registry");
    out
}

/// Every `(code, cid)` pair a CMap source states *itself*, read a second way.
///
/// Deliberately not the build script's parser. This one works a line at a
/// time, splits on whitespace, and knows nothing about tokens, sections
/// nested in other sections, or hex strings spanning a newline — none of
/// which the registry uses. Where it and `build.rs` agree, two independent
/// readings of Adobe's text agree.
///
/// `notdefrange` is skipped, matching the compiled tables: it names a
/// substitute CID for codes the CMap maps to nothing, and honouring it would
/// dress "unmapped" up as an answer. `is_only_notdef` below pins that.
fn declared(source: &str) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    let mut section = "";
    for line in source.lines() {
        let line = line.split('%').next().unwrap_or("").trim();
        if let Some(word) = line.split_whitespace().last() {
            match word {
                "begincidrange" | "begincidchar" => {
                    section = word;
                    continue;
                }
                "endcidrange" | "endcidchar" | "beginnotdefrange" | "endnotdefrange" => {
                    section = "";
                    continue;
                }
                _ => {}
            }
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match (section, fields.as_slice()) {
            ("begincidrange", [low, high, cid]) => {
                out.push((hex(low), hex(high), cid.parse().expect("a CID is a number")));
            }
            ("begincidchar", [code, cid]) => {
                let code = hex(code);
                out.push((code, code, cid.parse().expect("a CID is a number")));
            }
            _ => {}
        }
    }
    out
}

/// `<8140>` as a number.
fn hex(text: &str) -> u32 {
    let body = text
        .strip_prefix('<')
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or_else(|| panic!("{text} is not a hex string"));
    u32::from_str_radix(body, 16).expect("hex digits")
}

// ---------------------------------------------------------------------------
// Milestone 2: the CIDs the registry lists
// ---------------------------------------------------------------------------

/// The exit criterion, quoted.
///
/// From `data/cmap-resources/Adobe-Japan1-7/CMap/90ms-RKSJ-H`, verbatim:
///
/// ```text
/// 100 begincidrange
/// <20> <7d>      231
/// <7e> <7e>      631
/// <8140> <817e>  633
/// <8180> <81ac>  696
/// ```
///
/// and, 100 lines further down and in a *second* `begincidrange` section, so
/// that a reader stopping at the first one fails here:
///
/// ```text
/// 71 begincidrange
/// <9480> <94fc> 3350
/// ```
#[test]
fn ninety_ms_rksj_h_maps_the_cids_the_registry_lists() {
    let cmap = CMap::predefined(b"90ms-RKSJ-H").expect("the registry defines it");

    // `<20> <7d> 231`: a range's base belongs to its low code, and the rest
    // count up from there. 0x7d is 93 past 0x20.
    assert_eq!(cmap.cid(0x20), Some(231));
    assert_eq!(cmap.cid(0x21), Some(232));
    assert_eq!(cmap.cid(0x7d), Some(231 + 93));

    // `<7e> <7e> 631`: the single code immediately after it, which is not
    // 231 + 94. An off-by-one that ran ranges together would say 325.
    assert_eq!(cmap.cid(0x7e), Some(631));

    // `<8140> <817e> 633`, the first two-byte range.
    assert_eq!(cmap.cid(0x8140), Some(633));
    assert_eq!(cmap.cid(0x8150), Some(633 + 0x10));
    assert_eq!(cmap.cid(0x817e), Some(633 + 0x3e));

    // `<8180> <81ac> 696`. 0x817f is between the two ranges and in neither,
    // which is also what the codespace `<8140> <9FFC>` and Shift-JIS agree
    // on: 0x7f is not a lead byte's partner.
    assert_eq!(cmap.cid(0x8180), Some(696));
    assert_eq!(cmap.cid(0x817f), None, "between two ranges, in neither");

    // `<9480> <94fc> 3350`, from the second section.
    assert_eq!(cmap.cid(0x9480), Some(3350));
    assert_eq!(cmap.cid(0x94fc), Some(3350 + 0x7c));

    assert!(!cmap.is_vertical());
    assert!(!cmap.is_approximate(), "the table is compiled in");
}

/// The registry supplies its own decoy.
///
/// `90ms-RKSJ-V` is a differential CMap: `/90ms-RKSJ-H usecmap` and then 78
/// `cidrange`s that restate the codes whose glyphs rotate. Every code below
/// is answered by *both* files, with different numbers, so a merge that let
/// the parent win, or that dropped the parent entirely, produces a value this
/// test names.
///
/// From `.../90ms-RKSJ-V`, verbatim:
///
/// ```text
/// /90ms-RKSJ-H usecmap
/// /WMode 1 def
/// 78 begincidrange
/// <8141> <8142> 7887
/// <8160> <8164> 7894
/// ```
#[test]
fn ninety_ms_rksj_v_overrides_its_parent_and_inherits_the_rest() {
    let h = CMap::predefined(b"90ms-RKSJ-H").expect("the parent");
    let v = CMap::predefined(b"90ms-RKSJ-V").expect("the child");

    assert!(v.is_vertical(), "/WMode 1 def");
    assert!(!h.is_vertical());

    // Restated by the child. The parent's answer for 0x8141 is 634, from
    // `<8140> <817e> 633`, so neither number could arrive by accident.
    assert_eq!(h.cid(0x8141), Some(634));
    assert_eq!(v.cid(0x8141), Some(7887));
    assert_eq!(v.cid(0x8142), Some(7888), "and it offsets from the child's");
    assert_eq!(h.cid(0x8160), Some(633 + 0x20));
    assert_eq!(v.cid(0x8160), Some(7894));

    // Not restated by the child, so the parent's answer survives the merge.
    // Without `usecmap` this is `None`, which is the whole defect gap 04
    // named and this is where a real registry parent finally proves it.
    assert_eq!(v.cid(0x8140), Some(633), "inherited from 90ms-RKSJ-H");
    assert_eq!(v.cid(0x9480), Some(3350), "and from its second section");
    assert_eq!(v.cid(0x20), Some(231), "including the one-byte half");
}

/// Every `cidrange` and `cidchar` in the vendored registry, re-read.
///
/// This is the test that would catch a delta-encoding bug the quoted
/// constants above happen to miss: 402 816 ranges across 202 CMaps, each
/// checked at both ends against a second reader of the same file.
#[test]
fn every_declared_mapping_survives_the_build() {
    let mut checked = 0usize;
    for (name, source) in registry() {
        let Some(cmap) = CMap::predefined(name.as_bytes()) else {
            panic!("{name} is vendored and `predefined` does not know it");
        };
        for (low, high, cid) in declared(&source) {
            // A CMap's own declarations always win over anything it
            // inherits, so every one of them must come back exactly.
            assert_eq!(cmap.cid(low), Some(cid), "{name}: <{low:04X}> low bound");
            let top = cid + (high - low);
            assert_eq!(cmap.cid(high), Some(top), "{name}: <{high:04X}> high bound");
            checked += 2;
        }
    }
    assert_eq!(
        checked, 805_632,
        "the registry's whole declared mapping set, both bounds of each range"
    );
}

/// The registry's `Identity-H` and this crate's are the same CMap.
///
/// `CMap::predefined` answers that name from `CMap::identity`, a rule rather
/// than a table, because it is the one name a build with `cmap-predefined`
/// off still has to get right. So the rule has to be checked against the
/// file, and it is not the file a reader would guess: Adobe writes the
/// identity as 256 `cidrange`s of 256 codes each — `<0000> <00ff> 0`,
/// `<0100> <01ff> 256`, and so on — rather than as one range of 65 536,
/// because the format's own `<lo> <hi>` bounds are per-byte.
#[test]
fn the_built_in_identity_agrees_with_the_registry_file() {
    let source =
        std::fs::read_to_string(data().join("Adobe-Identity-0/CMap/Identity-H")).expect("vendored");
    let ranges = declared(&source);
    assert_eq!(ranges.len(), 256);
    assert_eq!(ranges.first(), Some(&(0x0000, 0x00FF, 0)));
    assert_eq!(ranges.last(), Some(&(0xFF00, 0xFFFF, 0xFF00)));
    for (low, high, cid) in &ranges {
        assert_eq!(low, cid, "every range maps a code to itself");
        assert_eq!(high - low, 0xFF);
    }

    let cmap = CMap::predefined(b"Identity-H").expect("built in");
    for code in [0u32, 1, 0x41, 0x3042, 0xFFFF] {
        assert_eq!(cmap.cid(code), Some(code));
    }
    assert!(!cmap.is_vertical());
    assert!(CMap::predefined(b"Identity-V")
        .expect("built in")
        .is_vertical());
}

/// `notdefrange` is read by nothing, deliberately.
///
/// `90ms-RKSJ-H` opens `1 beginnotdefrange <00> <1f> 231`, and 231 is the
/// same CID `<20>` maps to — the collection's space. Honouring it would turn
/// "this code means nothing" into an answer that looks exactly like a real
/// one. The decision is in `build.rs`'s doc comment; this is what pins it.
#[test]
fn a_notdefrange_maps_nothing() {
    let cmap = CMap::predefined(b"90ms-RKSJ-H").expect("the registry defines it");
    for code in [0x00u32, 0x01, 0x1f] {
        assert_eq!(
            cmap.cid(code),
            None,
            "<{code:02X}> is only in a notdefrange"
        );
    }
    assert_eq!(cmap.cid(0x20), Some(231), "and 231 is not unreachable");
}

/// A name the registry does not define is refused rather than guessed.
///
/// Every name below used to produce a CMap: fourteen prefixes matched with
/// `starts_with`, so anything beginning `90ms` or `Ext-` was answered with a
/// blanket two-byte identity. The list also carried `RKSJ`, which no registry
/// name begins with, so it matched nothing at all.
#[test]
fn a_name_outside_the_registry_is_refused() {
    for name in [
        &b"90ms-RKSJ-Q"[..],
        b"UniJIS-Nonsense",
        b"Ext-",
        b"RKSJ-Whatever",
        b"",
        b"H ",
        b"identity-h",
    ] {
        assert!(
            CMap::predefined(name).is_none(),
            "{} was answered with something",
            String::from_utf8_lossy(name)
        );
    }
}

/// The names the gap plan calls out as matching nothing under the old prefix
/// list, all of which the registry does define.
#[test]
fn the_names_the_prefix_list_missed_now_resolve() {
    for name in [
        &b"83pv-RKSJ-H"[..],
        b"78-RKSJ-H",
        b"Hankaku",
        b"Roman",
        b"HKscs-B5-H",
        b"UniHojo-UCS2-H",
        b"Hiragana",
        b"Katakana",
        b"WP-Symbol",
        b"H",
        b"V",
    ] {
        assert!(
            CMap::predefined(name).is_some(),
            "{} is in the registry",
            String::from_utf8_lossy(name)
        );
    }
}
