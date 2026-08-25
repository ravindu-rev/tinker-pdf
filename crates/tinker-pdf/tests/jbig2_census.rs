//! What the corpus's JBIG2 actually uses (roadmap Tier 2, milestone 1).
//!
//! The symbol-dictionary-plus-text-region lineage is the engine's
//! highest-reachability refusal — 103 files in the pdf.js corpus — and it has
//! two coding variants and an optional refinement stage. Which of them to build
//! first is a question about real files rather than about the standard, so
//! ruling 3 says measure before staging: this walks every JBIG2 stream in the
//! fetched corpora and tallies the flags that decide it.
//!
//! **The parser here is written from ITU-T T.88 clause 7.2 and shares nothing
//! with `tinker-pdf-filters`.** It has to: a census taken with the decoder's own
//! header reader would agree with that reader by construction, including
//! wherever it is wrong, and the whole point is to find out what is out there.
//! It also has to survive segments the decoder refuses, which is most of them
//! today.
//!
//! Run it, and record what it says in `docs/design/jbig2-symbol-text.md`:
//!
//! ```sh
//! cargo test -p tinker-pdf --test jbig2_census -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};

/// Segment types this census names (T.88 Table 34).
mod kind {
    pub const SYMBOL_DICTIONARY: u8 = 0;
    pub const INTERMEDIATE_TEXT_REGION: u8 = 4;
    pub const IMMEDIATE_TEXT_REGION: u8 = 6;
    pub const IMMEDIATE_LOSSLESS_TEXT_REGION: u8 = 7;
    pub const PATTERN_DICTIONARY: u8 = 16;
    pub const INTERMEDIATE_HALFTONE_REGION: u8 = 20;
    pub const IMMEDIATE_HALFTONE_REGION: u8 = 22;
    pub const IMMEDIATE_LOSSLESS_HALFTONE_REGION: u8 = 23;
    pub const INTERMEDIATE_REFINEMENT_REGION: u8 = 40;
    pub const IMMEDIATE_REFINEMENT_REGION: u8 = 42;
    pub const IMMEDIATE_LOSSLESS_REFINEMENT_REGION: u8 = 43;
    pub const TABLES: u8 = 53;
}

/// A big-endian reader that answers `None` rather than panicking, because every
/// byte here is attacker-controlled and half of them are truncated.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    fn u8(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    fn u16(&mut self) -> Option<u16> {
        let slice = self.bytes.get(self.at..self.at + 2)?;
        self.at += 2;
        Some(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        let slice = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.at = self.at.checked_add(n)?;
        (self.at <= self.bytes.len()).then_some(())
    }
}

/// What one file's JBIG2 uses.
#[derive(Default, Clone)]
struct Tally {
    segments: BTreeMap<u8, u32>,
    /// Symbol dictionaries coded with Huffman tables (7.4.3.1.1 bit 0).
    sdhuff: u32,
    /// Symbol dictionaries using refinement/aggregate coding (bit 1).
    sdrefagg: u32,
    /// Symbol dictionaries that use a retained context (bit 8).
    context_used: u32,
    /// Symbol dictionaries that retain their context (bit 9).
    context_retained: u32,
    /// Text regions coded with Huffman tables (7.4.4.1.1 bit 0).
    sbhuff: u32,
    /// Text regions that refine their symbols (bit 1).
    sbrefine: u32,
    /// Headers that would not parse — a truncated or damaged stream.
    unparsable: u32,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        for (kind, count) in &other.segments {
            *self.segments.entry(*kind).or_default() += count;
        }
        self.sdhuff += other.sdhuff;
        self.sdrefagg += other.sdrefagg;
        self.context_used += other.context_used;
        self.context_retained += other.context_retained;
        self.sbhuff += other.sbhuff;
        self.sbrefine += other.sbrefine;
        self.unparsable += other.unparsable;
    }

    fn count(&self, kind: u8) -> u32 {
        self.segments.get(&kind).copied().unwrap_or(0)
    }

    fn symbol_dictionaries(&self) -> u32 {
        self.count(kind::SYMBOL_DICTIONARY)
    }

    fn text_regions(&self) -> u32 {
        self.count(kind::INTERMEDIATE_TEXT_REGION)
            + self.count(kind::IMMEDIATE_TEXT_REGION)
            + self.count(kind::IMMEDIATE_LOSSLESS_TEXT_REGION)
    }

    fn refinement_regions(&self) -> u32 {
        self.count(kind::INTERMEDIATE_REFINEMENT_REGION)
            + self.count(kind::IMMEDIATE_REFINEMENT_REGION)
            + self.count(kind::IMMEDIATE_LOSSLESS_REFINEMENT_REGION)
    }

    fn halftone(&self) -> u32 {
        self.count(kind::PATTERN_DICTIONARY)
            + self.count(kind::INTERMEDIATE_HALFTONE_REGION)
            + self.count(kind::IMMEDIATE_HALFTONE_REGION)
            + self.count(kind::IMMEDIATE_LOSSLESS_HALFTONE_REGION)
    }
}

/// Walks one embedded JBIG2 stream's segment headers (T.88 clause 7.2, in the
/// PDF organisation of ISO 32000-1 7.4.7: no file header, headers and data
/// together, in order).
fn census_stream(bytes: &[u8], tally: &mut Tally) {
    let mut reader = Reader::new(bytes);
    loop {
        let Some(segment) = read_segment(&mut reader) else {
            // The end of the stream, or a header that stopped making sense.
            // Both end the walk; only the second is worth counting, and the
            // difference is whether anything was left.
            if reader.at < bytes.len() {
                tally.unparsable += 1;
            }
            return;
        };
        *tally.segments.entry(segment.kind).or_default() += 1;

        let mut data = Reader::new(segment.data);
        match segment.kind {
            kind::SYMBOL_DICTIONARY => {
                // 7.4.3.1.1: two bytes of flags open the data part.
                if let Some(flags) = data.u16() {
                    if flags & 0x0001 != 0 {
                        tally.sdhuff += 1;
                    }
                    if flags & 0x0002 != 0 {
                        tally.sdrefagg += 1;
                    }
                    if flags & 0x0100 != 0 {
                        tally.context_used += 1;
                    }
                    if flags & 0x0200 != 0 {
                        tally.context_retained += 1;
                    }
                }
            }
            kind::INTERMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_LOSSLESS_TEXT_REGION => {
                // 7.4.1: the region segment information field is seventeen
                // bytes — width, height, x, y, then one byte of flags — and
                // 7.4.4.1.1's own flags follow it.
                if let Some(flags) = data.skip(17).and_then(|()| data.u16()) {
                    if flags & 0x0001 != 0 {
                        tally.sbhuff += 1;
                    }
                    if flags & 0x0002 != 0 {
                        tally.sbrefine += 1;
                    }
                }
            }
            _ => {}
        }
    }
}

struct Segment<'a> {
    kind: u8,
    data: &'a [u8],
}

/// One segment header and its data, per T.88 7.2.
fn read_segment<'a>(reader: &mut Reader<'a>) -> Option<Segment<'a>> {
    let number = reader.u32()?;
    let flags = reader.u8()?;
    let kind = flags & 0x3F;
    // 7.2.3 bit 6: the page association is four bytes rather than one.
    let long_page = flags & 0x40 != 0;

    // 7.2.4: the top three bits of the next byte hold the referred-to count,
    // unless all three are set — then the whole four bytes are the count and a
    // run of retain flags follows it.
    let first = reader.u8()?;
    let count = if first >> 5 == 7 {
        reader.at -= 1;
        let long = reader.u32()? & 0x1FFF_FFFF;
        reader.skip((long as usize).checked_add(1)?.div_ceil(8))?;
        long
    } else {
        u32::from(first >> 5)
    };

    // 7.2.5: the width of each referred-to number is decided by *this*
    // segment's number, not by the values being referred to.
    let width = if number <= 256 {
        1
    } else if number <= 65536 {
        2
    } else {
        4
    };
    reader.skip((count as usize).checked_mul(width)?)?;

    if long_page {
        reader.u32()?;
    } else {
        reader.u8()?;
    }

    let length = reader.u32()?;
    if length == u32::MAX {
        // 7.2.7: an unknown length is legal only for an immediate generic
        // region and nothing after it can be located. The census stops here
        // rather than guessing, and says so by leaving bytes unread.
        return None;
    }
    let start = reader.at;
    reader.skip(length as usize)?;
    let data = reader.bytes.get(start..start + length as usize)?;
    Some(Segment { kind, data })
}

/// Every JBIG2 stream in one document: its bytes, and its globals.
fn jbig2_streams(bytes: Vec<u8>) -> Vec<Vec<u8>> {
    let Ok(doc) = Document::open(bytes) else {
        return Vec::new();
    };
    let cos = doc.cos();
    let filter = cos.intern(b"Filter");
    let parms = cos.intern(b"DecodeParms");
    let globals = cos.intern(b"JBIG2Globals");

    let names = |object: &Object| -> bool {
        let named = |o: &Object| {
            o.as_name()
                .and_then(|n| cos.name_bytes(n))
                .is_some_and(|b| b.as_ref() == b"JBIG2Decode")
        };
        match object {
            Object::Array(items) => items.iter().any(named),
            other => named(other),
        }
    };

    let mut out = Vec::new();
    for (number, entry) in cos.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let generation = match entry {
            XrefEntry::Offset { gen, .. } => gen,
            _ => 0,
        };
        let reference = ObjRef::new(number, generation);
        let Ok(object) = cos.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        if !names(&cos.resolve_key(dict, filter)) {
            continue;
        }
        // The globals stream is a separate object the image points at, and it
        // carries the shared symbol dictionaries — which is exactly what this
        // census is counting, so it has to be walked too.
        let parms_value = cos.resolve_key(dict, parms);
        if let Some(parms_dict) = parms_value.as_dict() {
            if let Some(reference) = parms_dict.get_ref(globals) {
                if let Ok(data) = cos.stream_decoded(reference) {
                    out.push(data);
                }
            }
        }
        if let Ok(data) = cos.stream_raw(reference) {
            out.push(data);
        }
    }
    out
}

/// The corpora, if they have been fetched.
fn corpus_root() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("TINKER_CORPUS") {
        let path = PathBuf::from(named);
        return path.is_dir().then_some(path);
    }
    // `cargo test` runs with the crate root as the working directory.
    let guess = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()?;
    guess.is_dir().then_some(guess)
}

fn pdfs_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

/// **What the corpus's JBIG2 actually uses.** Ignored by default: it reads
/// thousands of fetched files and answers a scheduling question rather than
/// asserting a property.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_jbig2() {
    let Some(root) = corpus_root() else {
        println!("jbig2-census: SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!(
        "jbig2-census: RAN over {} files under {}",
        files.len(),
        root.display()
    );

    let mut total = Tally::default();
    let mut carriers = 0u32;
    let mut per_file: Vec<(String, Tally)> = Vec::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let streams = jbig2_streams(bytes);
        if streams.is_empty() {
            continue;
        }
        let mut tally = Tally::default();
        for stream in &streams {
            census_stream(stream, &mut tally);
        }
        if tally.segments.is_empty() {
            continue;
        }
        carriers += 1;
        total.add(&tally);
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        per_file.push((name, tally));
    }

    println!("\n{carriers} files carry JBIG2\n");
    println!(
        "{:<58} {:>4} {:>4} {:>5} {:>5} {:>5} {:>5} {:>4}",
        "file", "sdic", "text", "sdhuf", "sbhuf", "refag", "refin", "ctx"
    );
    for (name, t) in &per_file {
        println!(
            "{:<58} {:>4} {:>4} {:>5} {:>5} {:>5} {:>5} {:>4}",
            if name.len() > 58 {
                &name[name.len() - 58..]
            } else {
                name
            },
            t.symbol_dictionaries(),
            t.text_regions(),
            t.sdhuff,
            t.sbhuff,
            t.sdrefagg,
            t.sbrefine,
            t.context_used + t.context_retained
        );
    }

    let files_with = |f: fn(&Tally) -> u32| per_file.iter().filter(|(_, t)| f(t) > 0).count();

    println!("\n--- totals over {carriers} files ---");
    println!(
        "symbol dictionaries      {:>6}   in {:>3} files",
        total.symbol_dictionaries(),
        files_with(Tally::symbol_dictionaries)
    );
    println!(
        "text regions             {:>6}   in {:>3} files",
        total.text_regions(),
        files_with(Tally::text_regions)
    );
    println!(
        "refinement regions       {:>6}   in {:>3} files",
        total.refinement_regions(),
        files_with(Tally::refinement_regions)
    );
    println!(
        "halftone / pattern       {:>6}   in {:>3} files",
        total.halftone(),
        files_with(Tally::halftone)
    );
    println!(
        "custom tables (type 53)  {:>6}   in {:>3} files",
        total.count(kind::TABLES),
        files_with(|t| t.count(kind::TABLES))
    );
    println!();
    println!(
        "SDHUFF   (Huffman symbol dict) {:>6}   in {:>3} files",
        total.sdhuff,
        files_with(|t| t.sdhuff)
    );
    println!(
        "SBHUFF   (Huffman text region) {:>6}   in {:>3} files",
        total.sbhuff,
        files_with(|t| t.sbhuff)
    );
    println!(
        "SDREFAGG (refine/aggregate)    {:>6}   in {:>3} files",
        total.sdrefagg,
        files_with(|t| t.sdrefagg)
    );
    println!(
        "SBREFINE (text refinement)     {:>6}   in {:>3} files",
        total.sbrefine,
        files_with(|t| t.sbrefine)
    );
    println!(
        "context used                   {:>6}   in {:>3} files",
        total.context_used,
        files_with(|t| t.context_used)
    );
    println!(
        "context retained               {:>6}   in {:>3} files",
        total.context_retained,
        files_with(|t| t.context_retained)
    );
    println!(
        "unparsable headers             {:>6}   in {:>3} files",
        total.unparsable,
        files_with(|t| t.unparsable)
    );

    // The scheduling question milestone 1 exists to answer is not how many
    // segments use a feature but how many *files* a stage would unlock, and
    // that is a question about overlap: a file needing both Huffman and
    // refinement is unlocked by neither alone.
    let needs_huff = |t: &Tally| t.sdhuff > 0 || t.sbhuff > 0;
    let needs_refine = |t: &Tally| t.sdrefagg > 0 || t.sbrefine > 0;
    let lineage = |t: &Tally| t.symbol_dictionaries() > 0 || t.text_regions() > 0;

    let carrying: Vec<&Tally> = per_file
        .iter()
        .map(|(_, t)| t)
        .filter(|t| lineage(t))
        .collect();
    let plain = carrying
        .iter()
        .filter(|t| !needs_huff(t) && !needs_refine(t))
        .count();
    let huff_only = carrying
        .iter()
        .filter(|t| needs_huff(t) && !needs_refine(t))
        .count();
    let refine_only = carrying
        .iter()
        .filter(|t| !needs_huff(t) && needs_refine(t))
        .count();
    let both = carrying
        .iter()
        .filter(|t| needs_huff(t) && needs_refine(t))
        .count();
    let halftone_too = carrying.iter().filter(|t| t.halftone() > 0).count();
    let halftone_only = per_file
        .iter()
        .filter(|(_, t)| !lineage(t) && t.halftone() > 0)
        .count();

    println!();
    println!(
        "--- what each stage unlocks, of the {} files in this lineage ---",
        carrying.len()
    );
    println!("arithmetic alone (milestones 3-4)   {plain:>3}");
    println!(
        "+ Huffman     (milestone 5)         {huff_only:>3}   cumulative {:>3}",
        plain + huff_only
    );
    println!(
        "+ refinement  (milestone 6)         {refine_only:>3}   cumulative {:>3}",
        plain + refine_only
    );
    println!("needs both                          {both:>3}");
    println!("also carry halftone, still degrade  {halftone_too:>3}");
    println!("halftone or pattern only            {halftone_only:>3}");
    println!();

    println!("\nsegment types seen, by T.88 Table 34 number:");
    for (kind, count) in &total.segments {
        println!("  type {kind:>3}  {count:>6}");
    }
}
