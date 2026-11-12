//! Merging identical streams on a rewrite.
//!
//! Feature documentation: `docs/features/writing.md`.
//!
//! # Identical means identical
//!
//! Two streams are merged only when their dictionaries are equal — every key,
//! every value, in order, `/Length` aside since the writer computes it — **and**
//! their contents are equal byte for byte. A hash only decides which streams
//! are worth comparing; it never decides a merge. A wrong merge is not a
//! larger file, it is a document that silently draws one font's glyphs with
//! another's program, and nothing downstream can tell.
//!
//! **Equal encodings merge undecoded.** Candidates are bucketed by their
//! dictionary's bytes, which carry the filter chain, and by the length and
//! the XXH64 of their stored bytes; two of one bucket whose stored bytes are
//! equal are one stream, since one chain over one input decodes to one
//! output, and nothing is decoded to say so.
//!
//! **Differing encodings under one dictionary are compared decoded**, so two
//! deflate levels over one font program are still found. That is ruling 23's
//! answer to H-137: the merges a comparison of encodings alone would give up
//! are kept, at the price of a decode of each stream whose dictionary another
//! shares under a different encoding, every page's content stream of a merge
//! among them. Each such stream is decoded at most once a pass, which a save
//! runs once, and its decoded bytes are kept until the pass ends; a
//! byte-identical copy merges without a decode, and no other stream is
//! decoded. Where decoding is anything less than exact — a filter this build
//! cannot run, an image codec the chain stops at, a decode that warned or hit
//! the output cap — the stream's stored bytes are compared instead, under a
//! different tag, so a truncated decode can never make two different streams
//! look alike.
//!
//! # What is never merged
//!
//! Only streams are candidates, so the objects whose *number* is what they
//! mean — pages, annotations, optional content groups, structure elements,
//! signature dictionaries, the `/Parent` a page points back at — are never
//! touched: they are dictionaries. Among streams, two kinds are held apart
//! whatever their bytes: a cross-reference or object stream (`/Type /XRef`,
//! `/ObjStm`), which a rewrite's source may carry as ordinary objects and
//! which describe the file they came from, and a stream an `/OBJR`'s `/Obj`
//! names (14.7.5.3), where the reference *is* the structure element's claim
//! on that object and two claims must stay two.
//!
//! Only on a rewrite: an incremental update appends, and merging would mean
//! rewriting objects a signature's revision covers. The caller is
//! [`crate::edit::DocumentEditor::save`], after garbage collection.
//!
//! # Merging to a fixed point
//!
//! Redirecting a duplicate's references can make two dictionaries equal that
//! were not — two form XObjects whose `/Resources` named two copies of one
//! image — so the pass repeats while it finds anything, at most
//! [`limits::MAX_NEST_DEPTH`] times: the depth of nesting any reader here
//! follows, and so the deepest chain of such dependencies worth resolving.
//! Stopping early misses a merge; it never makes a wrong one.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::limits;
use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object};
use crate::write::{write_object, ObjectSet, StreamData, Written};

/// What a stream's content is compared as: its decoded bytes, or — when
/// decoding is not exact — its stored bytes, told apart by the tag.
pub(crate) enum Content {
    /// The bytes after the whole `/Filter` chain, decoded without a warning.
    Decoded(Vec<u8>),
    /// The bytes as stored.
    Stored(Vec<u8>),
}

impl Content {
    fn tagged(&self) -> (u8, &[u8]) {
        match self {
            Content::Decoded(bytes) => (b'D', bytes),
            Content::Stored(bytes) => (b'S', bytes),
        }
    }
}

/// A stream's dictionary as it is compared: without `/Length`, which the
/// writer computes from the bytes it writes (7.3.8.2) and which a stream
/// the editor made may simply not carry yet.
fn comparable(dict: &Dict) -> Dict {
    crate::edit::without(dict, Name::LENGTH)
}

/// Every reference in `object` that `mapping` names, pointed at what it maps
/// to. Everything else — a dangling reference included — is left as it was.
fn redirect(object: &Object, mapping: &BTreeMap<u32, u32>, depth: u32) -> Object {
    if depth > limits::MAX_NEST_DEPTH {
        return object.clone();
    }
    match object {
        Object::Ref(r) => match mapping.get(&r.num) {
            Some(kept) => Object::Ref(ObjRef::new(*kept, 0)),
            None => object.clone(),
        },
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|item| redirect(item, mapping, depth + 1))
                .collect(),
        ),
        Object::Dict(dict) => Object::Dict(redirect_dict(dict, mapping, depth)),
        // A stream whose bytes a rewrite could not read travels as its parsed
        // object, and is written as its dictionary; that dictionary's
        // references move like any other.
        Object::Stream(stream) => {
            let mut stream = stream.clone();
            stream.dict = redirect_dict(&stream.dict, mapping, depth);
            Object::Stream(stream)
        }
        other => other.clone(),
    }
}

fn redirect_dict(dict: &Dict, mapping: &BTreeMap<u32, u32>, depth: u32) -> Dict {
    let mut out = Dict::with_capacity(dict.len());
    for (key, value) in dict.iter() {
        out.insert(*key, redirect(value, mapping, depth + 1));
    }
    out
}

/// The streams whose number carries identity (see the module documentation).
fn held_apart(objects: &ObjectSet, names: &NameTable) -> HashSet<u32> {
    let objr = names.intern(b"OBJR");
    let obj = names.intern(b"Obj");
    let identity_types = [names.intern(b"XRef"), names.intern(b"ObjStm")];

    fn claims(object: &Object, objr: Name, obj: Name, out: &mut HashSet<u32>, depth: u32) {
        if depth > limits::MAX_NEST_DEPTH {
            return;
        }
        match object {
            Object::Dict(dict) => {
                if dict.get_name(Name::TYPE) == Some(objr) {
                    if let Some(r) = dict.get_ref(obj) {
                        out.insert(r.num);
                    }
                }
                for (_, value) in dict.iter() {
                    claims(value, objr, obj, out, depth + 1);
                }
            }
            Object::Array(items) => {
                for item in items {
                    claims(item, objr, obj, out, depth + 1);
                }
            }
            _ => {}
        }
    }

    let mut out = HashSet::new();
    for (num, entry) in objects.iter() {
        match entry {
            Written::Object(object) => claims(object, objr, obj, &mut out, 0),
            Written::Stream(stream) => {
                if stream
                    .dict
                    .get_name(Name::TYPE)
                    .is_some_and(|t| identity_types.contains(&t))
                {
                    out.insert(*num);
                }
            }
        }
    }
    out
}

/// What one candidate is compared by once it is decoded: its content and
/// that content's hash, kept for the pass so no round decodes it again.
struct Decoded {
    content: Content,
    hash: u64,
}

/// Merges identical streams, returning the set and trailer with every
/// reference to a merged duplicate pointed at the stream kept in its place,
/// and each merge as `(duplicate, kept)`.
///
/// `content` gives a stream's content as it is compared decoded, and is
/// called at most once a stream; `digest` hashes bytes into buckets and is
/// XXH64 in every caller but the tests, which inject a colliding one to show
/// a shared hash is never a merge. The stream kept is the lowest-numbered of
/// its kind, so the output does not depend on map order.
pub(crate) fn deduplicate(
    objects: &ObjectSet,
    trailer: &Dict,
    names: &NameTable,
    content: &dyn Fn(u32, &StreamData) -> Content,
    digest: &dyn Fn(&[u8]) -> u64,
) -> (ObjectSet, Dict, Vec<(u32, u32)>) {
    deduplicate_comparing(objects, trailer, names, content, digest, true)
}

/// [`deduplicate`], comparing differing encodings under one dictionary
/// decoded or not. Every save compares them decoded (ruling 23); only this
/// module's pricing test turns the comparison off, to price what it keeps.
fn deduplicate_comparing(
    objects: &ObjectSet,
    trailer: &Dict,
    names: &NameTable,
    content: &dyn Fn(u32, &StreamData) -> Content,
    digest: &dyn Fn(&[u8]) -> u64,
    decoded_comparison: bool,
) -> (ObjectSet, Dict, Vec<(u32, u32)>) {
    let apart = held_apart(objects, names);

    // Each candidate's stored bytes hashed, once: redirecting references
    // changes dictionaries and never bytes.
    let mut encoded: BTreeMap<u32, u64> = BTreeMap::new();
    for (num, entry) in objects.iter() {
        if let Written::Stream(stream) = entry {
            if !apart.contains(num) {
                encoded.insert(*num, digest(&stream.data));
            }
        }
    }
    let mut decoded: BTreeMap<u32, Decoded> = BTreeMap::new();

    let mut set = objects.clone();
    let mut trailer = trailer.clone();
    let mut merges: Vec<(u32, u32)> = Vec::new();

    for _ in 0..limits::MAX_NEST_DEPTH {
        // The streams that *might* be one share their dictionary's bytes.
        let mut groups: BTreeMap<Vec<u8>, Vec<u32>> = BTreeMap::new();
        for num in encoded.keys() {
            let Some(Written::Stream(stream)) = set.get(*num) else {
                continue;
            };
            let mut key = Vec::new();
            write_object(&mut key, &Object::Dict(comparable(&stream.dict)), names);
            groups.entry(key).or_default().push(*num);
        }

        let mut mapping: BTreeMap<u32, u32> = BTreeMap::new();
        for members in groups.values().filter(|m| m.len() > 1) {
            let distinct = merge_equal_encodings(&set, members, &encoded, &mut mapping);
            if decoded_comparison && distinct.len() > 1 {
                for &num in &distinct {
                    if let (false, Some(Written::Stream(stream))) =
                        (decoded.contains_key(&num), set.get(num))
                    {
                        let content = content(num, stream);
                        let hash = digest(content.tagged().1);
                        decoded.insert(num, Decoded { content, hash });
                    }
                }
                merge_equal_contents(&set, &distinct, &decoded, &mut mapping);
            }
        }
        if mapping.is_empty() {
            break;
        }

        let mut next = ObjectSet::new();
        for (num, entry) in set.iter() {
            if mapping.contains_key(num) {
                continue;
            }
            match entry {
                Written::Object(object) => next.insert(*num, redirect(object, &mapping, 0)),
                Written::Stream(stream) => next.insert_stream(
                    *num,
                    StreamData {
                        dict: redirect_dict(&stream.dict, &mapping, 0),
                        data: stream.data.clone(),
                    },
                ),
            }
        }
        trailer = redirect_dict(&trailer, &mapping, 0);
        for num in mapping.keys() {
            encoded.remove(num);
            decoded.remove(num);
        }
        merges.extend(mapping);
        set = next;
    }

    // A duplicate merged into one that was itself merged later points at the
    // survivor, so every pair names an object that is still there.
    let survivors: HashMap<u32, u32> = merges.iter().copied().collect();
    let merges = merges
        .iter()
        .map(|(duplicate, kept)| {
            let mut at = *kept;
            for _ in 0..limits::MAX_NEST_DEPTH {
                match survivors.get(&at) {
                    Some(further) => at = *further,
                    None => break,
                }
            }
            (*duplicate, at)
        })
        .collect();
    (set, trailer, merges)
}

/// `num`'s dictionary as it is compared, and its stored bytes.
fn compared(set: &ObjectSet, num: u32) -> Option<(Dict, &[u8])> {
    match set.get(num) {
        Some(Written::Stream(stream)) => Some((comparable(&stream.dict), &stream.data)),
        _ => None,
    }
}

/// `num`'s stored bytes.
fn stored(set: &ObjectSet, num: u32) -> Option<&[u8]> {
    match set.get(num) {
        Some(Written::Stream(stream)) => Some(&stream.data),
        _ => None,
    }
}

/// Within `members`, which share their dictionary's bytes, each stream whose
/// dictionary and stored bytes equal a lower-numbered one's is mapped to it,
/// undecoded: bucketed by the bytes' length and hash, and confirmed byte for
/// byte. Returns the first of each distinct encoding, in number order.
fn merge_equal_encodings(
    set: &ObjectSet,
    members: &[u32],
    encoded: &BTreeMap<u32, u64>,
    mapping: &mut BTreeMap<u32, u32>,
) -> Vec<u32> {
    let mut buckets: BTreeMap<(usize, u64), Vec<(u32, Dict)>> = BTreeMap::new();
    let mut distinct = Vec::new();
    for &num in members {
        let (Some((dict, data)), Some(&hash)) = (compared(set, num), encoded.get(&num)) else {
            continue;
        };
        let kept = buckets.entry((data.len(), hash)).or_default();
        let same = kept
            .iter()
            .find(|(other, other_dict)| *other_dict == dict && stored(set, *other) == Some(data));
        match same {
            Some((canonical, _)) => {
                mapping.insert(num, *canonical);
            }
            None => {
                kept.push((num, dict));
                distinct.push(num);
            }
        }
    }
    distinct
}

/// Among `distinct`, the first of each encoding under one dictionary's bytes,
/// each stream whose dictionary and decoded content equal a lower-numbered
/// one's is mapped to it: bucketed by the content's tag, length and hash, and
/// confirmed byte for byte.
fn merge_equal_contents(
    set: &ObjectSet,
    distinct: &[u32],
    decoded: &BTreeMap<u32, Decoded>,
    mapping: &mut BTreeMap<u32, u32>,
) {
    let mut buckets: BTreeMap<(u8, usize, u64), Vec<(u32, Dict)>> = BTreeMap::new();
    for &num in distinct {
        let (Some((dict, _)), Some(mine)) = (compared(set, num), decoded.get(&num)) else {
            continue;
        };
        let (tag, bytes) = mine.content.tagged();
        let kept = buckets.entry((tag, bytes.len(), mine.hash)).or_default();
        let same = kept.iter().find(|(other, other_dict)| {
            *other_dict == dict
                && decoded
                    .get(other)
                    .is_some_and(|other| other.content.tagged() == (tag, bytes))
        });
        match same {
            Some((canonical, _)) => {
                mapping.insert(num, *canonical);
            }
            None => kept.push((num, dict)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PdfString;

    fn stream(names: &NameTable, entries: &[(&[u8], Object)], data: &[u8]) -> StreamData {
        let mut dict = Dict::new();
        for (key, value) in entries {
            dict.insert(names.intern(key), value.clone());
        }
        StreamData {
            dict,
            data: data.to_vec(),
        }
    }

    fn raw(_: u32, stream: &StreamData) -> Content {
        Content::Decoded(stream.data.clone())
    }

    fn xxh(bytes: &[u8]) -> u64 {
        tinker_pdf_filters::xxh64(bytes)
    }

    /// Two copies of one program under two font descriptors: one survives
    /// and both descriptors name it.
    #[test]
    fn two_identical_streams_become_one_and_every_reference_follows() {
        let names = NameTable::new();
        let mut set = ObjectSet::new();
        set.insert_stream(1, stream(&names, &[], b"program"));
        set.insert_stream(2, stream(&names, &[], b"program"));
        let file = names.intern(b"FontFile2");
        let descriptor = |num| {
            let mut dict = Dict::new();
            dict.insert(file, Object::Ref(ObjRef::new(num, 0)));
            Object::Dict(dict)
        };
        set.insert(3, descriptor(1));
        set.insert(4, descriptor(2));
        let (out, _, merges) = deduplicate(&set, &Dict::new(), &names, &raw, &xxh);
        assert_eq!(merges, [(2, 1)]);
        assert!(out.get(2).is_none());
        for num in [3, 4] {
            let Some(Written::Object(Object::Dict(dict))) = out.get(num) else {
                panic!("descriptor {num} survives");
            };
            assert_eq!(dict.get_ref(file), Some(ObjRef::new(1, 0)));
        }
    }

    /// Equal bytes, unequal dictionaries: two predictors over one sample
    /// run are two images.
    #[test]
    fn equal_bytes_under_different_parameters_stay_two() {
        let names = NameTable::new();
        let parms = |columns: i64| {
            let mut dict = Dict::new();
            dict.insert(names.intern(b"Columns"), Object::Int(columns));
            Object::Dict(dict)
        };
        let mut set = ObjectSet::new();
        set.insert_stream(1, stream(&names, &[(b"DecodeParms", parms(4))], b"same"));
        set.insert_stream(2, stream(&names, &[(b"DecodeParms", parms(8))], b"same"));
        let (out, _, merges) = deduplicate(&set, &Dict::new(), &names, &raw, &xxh);
        assert!(merges.is_empty());
        assert_eq!(out.len(), 2);
    }

    /// A digest that collides on everything puts every stream in one
    /// bucket, and the byte comparison still keeps different content apart:
    /// a shared digest is never a merge.
    #[test]
    fn a_colliding_digest_never_merges_different_bytes() {
        let names = NameTable::new();
        let collide = |_: &[u8]| 0u64;
        let mut set = ObjectSet::new();
        set.insert_stream(1, stream(&names, &[], b"alpha"));
        set.insert_stream(2, stream(&names, &[], b"omega"));
        set.insert_stream(3, stream(&names, &[], b"alpha"));
        let (out, _, merges) = deduplicate(&set, &Dict::new(), &names, &raw, &collide);
        assert_eq!(merges, [(3, 1)], "only the true duplicate merges");
        assert!(out.get(2).is_some(), "the collision survives");
    }

    /// Two encodings under one dictionary, one decoding exactly to what the
    /// other stores and could not decode exactly: a stored-bytes comparison
    /// never meets a decoded one, so they stay two.
    #[test]
    fn decoded_and_stored_content_are_never_compared_with_each_other() {
        let names = NameTable::new();
        let mut set = ObjectSet::new();
        set.insert_stream(1, stream(&names, &[], b"x, encoded once"));
        set.insert_stream(2, stream(&names, &[], b"x, encoded twice"));
        let tagged = |num: u32, _: &StreamData| {
            if num == 1 {
                Content::Decoded(b"x".to_vec())
            } else {
                Content::Stored(b"x".to_vec())
            }
        };
        let (_, _, merges) = deduplicate(&set, &Dict::new(), &names, &tagged, &xxh);
        assert!(merges.is_empty());
    }

    /// A stream an `/OBJR` names, and a cross-reference stream, are held
    /// apart from their twins.
    #[test]
    fn a_stream_with_identity_is_never_merged() {
        let names = NameTable::new();
        let mut set = ObjectSet::new();
        set.insert_stream(1, stream(&names, &[], b"figure"));
        set.insert_stream(2, stream(&names, &[], b"figure"));
        let mut objr = Dict::new();
        objr.insert(Name::TYPE, Object::Name(names.intern(b"OBJR")));
        objr.insert(names.intern(b"Obj"), Object::Ref(ObjRef::new(2, 0)));
        set.insert(3, Object::Dict(objr));
        let xref = (b"Type".as_slice(), Object::Name(names.intern(b"XRef")));
        set.insert_stream(4, stream(&names, std::slice::from_ref(&xref), b"t"));
        set.insert_stream(5, stream(&names, std::slice::from_ref(&xref), b"t"));
        let (_, _, merges) = deduplicate(&set, &Dict::new(), &names, &raw, &xxh);
        assert!(merges.is_empty(), "{merges:?}");
    }

    /// Two forms that differ only in naming two copies of one image become
    /// equal once the image is merged, and merge in the next round.
    #[test]
    fn merging_runs_to_a_fixed_point() {
        let names = NameTable::new();
        let mut set = ObjectSet::new();
        set.insert_stream(1, stream(&names, &[], b"image"));
        set.insert_stream(2, stream(&names, &[], b"image"));
        let form = |image: u32| {
            let mut xobjects = Dict::new();
            xobjects.insert(names.intern(b"Im0"), Object::Ref(ObjRef::new(image, 0)));
            let mut resources = Dict::new();
            resources.insert(names.intern(b"XObject"), Object::Dict(xobjects));
            stream(
                &names,
                &[(b"Resources", Object::Dict(resources))],
                b"/Im0 Do",
            )
        };
        set.insert_stream(3, form(1));
        set.insert_stream(4, form(2));
        let mut trailer = Dict::new();
        trailer.insert(
            names.intern(b"Probe"),
            Object::Array(vec![
                Object::Ref(ObjRef::new(4, 0)),
                Object::String(PdfString::literal(b"kept".to_vec())),
            ]),
        );
        let (out, trailer, merges) = deduplicate(&set, &trailer, &names, &raw, &xxh);
        assert_eq!(merges, [(2, 1), (4, 3)]);
        assert_eq!(out.len(), 2);
        assert_eq!(
            trailer
                .get_array(names.intern(b"Probe"))
                .and_then(|a| a.first())
                .and_then(Object::as_objref),
            Some(ObjRef::new(3, 0)),
            "the trailer's references are redirected too"
        );
    }

    /// Four copies of a document whose streams' dictionaries are pairwise
    /// distinct: every copy merges into the first, and nothing is decoded.
    #[test]
    fn byte_identical_encodings_merge_undecoded() {
        let names = NameTable::new();
        let mut set = ObjectSet::new();
        let kinds: [&[u8]; 3] = [b"FontFile2", b"Subtype", b"Width"];
        for copy in 0..4u32 {
            for (at, kind) in kinds.iter().enumerate() {
                let num = 1 + copy * 3 + at as u32;
                let marker = (*kind, Object::Int(at as i64));
                set.insert_stream(num, stream(&names, &[marker], b"the same bytes"));
            }
        }
        let decodes = std::cell::Cell::new(0u32);
        let counted = |num: u32, s: &StreamData| {
            decodes.set(decodes.get() + 1);
            raw(num, s)
        };
        let (out, _, merges) = deduplicate(&set, &Dict::new(), &names, &counted, &xxh);
        assert_eq!(decodes.get(), 0, "nothing is decoded");
        assert_eq!(out.len(), 3, "one stream of each kind survives");
        let mut want: Vec<(u32, u32)> = (1..4u32)
            .flat_map(|copy| (0..3u32).map(move |at| (1 + copy * 3 + at, 1 + at)))
            .collect();
        want.sort_unstable();
        let mut got = merges.clone();
        got.sort_unstable();
        assert_eq!(got, want, "each copy merges into the first");
    }
    // ---- H-137's price (P-03.5) -------------------------------------------------

    /// SplitMix64, the stream MS-04's generator draws its words from.
    struct Seeded(u64);

    impl Seeded {
        fn below(&mut self, bound: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) % bound as u64) as usize
        }
    }

    /// The workspace root, where the committed fixtures and faces are.
    fn root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// `pages` pages as MS-04's `dense` builds them: A4, seventy lines of
    /// about eighty characters a page, in Liberation Serif embedded whole.
    fn dense(pages: usize, seed: u64, face: &[u8]) -> Vec<u8> {
        const WORDS: [&str; 24] = [
            "the", "of", "and", "a", "to", "in", "is", "that", "it", "was", "for", "on", "with",
            "from", "which", "their", "page", "line", "glyph", "reader", "order", "about", "1984",
            "2026",
        ];
        let mut words = Seeded(seed);
        let mut builder = crate::build::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"LiberationSerif", face));
        for _ in 0..pages {
            builder.add_page(595.0, 842.0, |page| {
                for at in 0..70 {
                    let mut line = String::new();
                    while line.len() < 78 {
                        if !line.is_empty() {
                            line.push(' ');
                        }
                        line.push_str(WORDS[words.below(WORDS.len())]);
                    }
                    let y = 806.0 - 11.0 * f64::from(at);
                    page.encoded_text(b"F0", 9.0, 36.0, y, (0.0, 0.0), line.as_bytes(), &line);
                }
            });
        }
        builder.finish()
    }

    /// Four of [`dense`]'s 100-page documents merged as `tpdf merge` merges
    /// them, the first the base and every page of the rest imported, and
    /// written without deduplication: the set a merge's save deduplicates.
    fn merged_four_by_a_hundred() -> Vec<u8> {
        let face = std::fs::read(
            root().join("crates/tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf"),
        )
        .expect("the committed face");
        let parts: Vec<std::sync::Arc<crate::doc::CosDocument>> = (0..4)
            .map(|seed| {
                std::sync::Arc::new(
                    crate::doc::CosDocument::open(dense(100, seed, &face)).expect("a part opens"),
                )
            })
            .collect();
        let mut editor = crate::edit::DocumentEditor::new(std::sync::Arc::clone(&parts[0]));
        for part in &parts[1..] {
            for index in 0..crate::pages::count(part) {
                let at = editor.page_count();
                editor
                    .import_page(part, index, at)
                    .expect("the page imports");
            }
        }
        editor.save(&crate::write::WriteOptions {
            mode: crate::write::WriteMode::Rewrite,
            ..crate::write::WriteOptions::default()
        })
    }

    /// The fixture `stream_dedup.rs`'s `two_encodings_of_one_content_merge`
    /// builds: one content under one dictionary, deflated by the encoder and
    /// stored in a zlib stream of stored blocks.
    fn two_encodings_of_one_content() -> (crate::doc::CosDocument, ObjectSet) {
        let mut builder = crate::build::DocumentBuilder::new();
        builder.add_page(50.0, 50.0, |_| {});
        let doc = crate::doc::CosDocument::open(builder.finish()).expect("opens");
        let names = doc.names_table();
        let content = b"one content, two encodings, one object".repeat(20);
        let packed = tinker_pdf_filters::zlib_compress(&content);
        let mut stored = vec![0x78, 0x01, 0x01];
        stored.extend_from_slice(&(content.len() as u16).to_le_bytes());
        stored.extend_from_slice(&(!(content.len() as u16)).to_le_bytes());
        stored.extend_from_slice(&content);
        let mut a = 1u32;
        let mut b = 0u32;
        for &byte in &content {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        stored.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let flate = (
            b"Filter".as_slice(),
            Object::Name(names.intern(b"FlateDecode")),
        );
        let mut set = ObjectSet::new();
        set.insert_stream(1, stream(names, std::slice::from_ref(&flate), &packed));
        set.insert_stream(2, stream(names, std::slice::from_ref(&flate), &stored));
        (doc, set)
    }

    /// The objects and trailer a rewrite of `doc` hands deduplication.
    fn rewrite_set(doc: &crate::doc::CosDocument) -> (ObjectSet, Dict) {
        let mut set = ObjectSet::new();
        for (num, _) in doc.xref().iter() {
            if num == 0 {
                continue;
            }
            let reference = ObjRef::new(num, 0);
            let Ok(object) = doc.get(reference) else {
                continue;
            };
            match object.as_ref() {
                Object::Null => {}
                Object::Stream(parsed) => {
                    if let Ok(data) = doc.stream_raw(reference) {
                        let dict = parsed.dict.clone();
                        set.insert_stream(num, StreamData { dict, data });
                    }
                }
                other => set.insert(num, other.clone()),
            }
        }
        (set, doc.trailer().clone())
    }

    /// What one mode of the pass did to one input: the decodes it ran, the
    /// merges it made, the rewrite's bytes, and the median time of the pass
    /// and the rewrite over five runs.
    struct Priced {
        decodes: u32,
        merges: usize,
        bytes: usize,
        millis: f64,
    }

    /// The pass over `set`, comparing differing encodings decoded or not,
    /// decoding through `doc` as a save decodes through its document.
    fn price(
        set: &ObjectSet,
        trailer: &Dict,
        names: &NameTable,
        doc: &crate::doc::CosDocument,
        decoded: bool,
    ) -> Priced {
        let decodes = std::cell::Cell::new(0u32);
        let content = |num: u32, stream: &StreamData| {
            if stream.dict.get(Name::FILTER).is_none() {
                return Content::Decoded(stream.data.clone());
            }
            decodes.set(decodes.get() + 1);
            let mut sink = crate::warn::WarningSink::new();
            match doc.decode_with(&stream.data, &stream.dict, num, &mut sink) {
                Ok(bytes) if sink.is_empty() => Content::Decoded(bytes),
                _ => Content::Stored(stream.data.clone()),
            }
        };
        let options = crate::write::WriteOptions {
            mode: crate::write::WriteMode::Rewrite,
            ..crate::write::WriteOptions::default()
        };
        let mut times = Vec::new();
        let mut last = None;
        for _ in 0..5 {
            decodes.set(0);
            let start = std::time::Instant::now();
            let (out, out_trailer, merges) =
                deduplicate_comparing(set, trailer, names, &content, &xxh, decoded);
            let bytes = crate::write::rewrite(&out, &out_trailer, &options, names).len();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            last = Some((merges.len(), bytes));
        }
        times.sort_by(f64::total_cmp);
        let (merges, bytes) = last.expect("five runs");
        Priced {
            decodes: decodes.get(),
            merges,
            bytes,
            millis: times[2],
        }
    }

    /// One input priced both ways, as a row: decodes, merges and bytes kept
    /// and dropped, the merges dropping loses, and each mode's time; and
    /// whether the two modes differ.
    fn row(
        name: &str,
        set: &ObjectSet,
        trailer: &Dict,
        names: &NameTable,
        doc: &crate::doc::CosDocument,
    ) -> (String, bool) {
        let kept = price(set, trailer, names, doc, true);
        let dropped = price(set, trailer, names, doc, false);
        let differ = kept.merges != dropped.merges || kept.decodes != dropped.decodes;
        (
            format!(
                "| {name} | {} / {} | {} / {} | {} | {} / {} | {:.2} / {:.2} |",
                kept.decodes,
                dropped.decodes,
                kept.merges,
                dropped.merges,
                kept.merges - dropped.merges,
                kept.bytes,
                dropped.bytes,
                kept.millis,
                dropped.millis
            ),
            differ,
        )
    }

    /// Every committed PDF under the workspace, outside `target` and `.git`.
    fn committed_pdfs(at: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(at) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "target" && name != ".git" {
                    committed_pdfs(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "pdf") {
                out.push(path);
            }
        }
    }

    /// **H-137's price** (P-03.5): the decoded comparison of differing
    /// encodings under one dictionary kept, as today, and dropped, as merging
    /// on encoded bytes alone would, over a merge of four 100-page documents
    /// built as MS-04's generator builds them and over every committed fixture
    /// whose two modes differ. Prints a row an input: decodes, merges, the
    /// merges dropping loses, the rewrite's bytes, and the median time of the
    /// pass and the rewrite, each kept / dropped. Holds nothing.
    ///
    /// `cargo test -p tinker-pdf-cos --lib h137 -- --ignored --nocapture`,
    /// and again with `--release`.
    #[test]
    #[ignore = "a measurement for the owner: prints H-137's price and holds nothing"]
    fn h137_prices_the_decoded_comparison_kept_and_dropped() {
        println!("RAN h137_prices_the_decoded_comparison_kept_and_dropped");
        println!("| Input | Decodes | Merges | Lost | Bytes | ms |");
        println!("| --- | ---: | ---: | ---: | ---: | ---: |");
        let merged = crate::doc::CosDocument::open(merged_four_by_a_hundred()).expect("opens");
        let (set, trailer) = rewrite_set(&merged);
        let (line, _) = row(
            "merge of 4 x 100 pages",
            &set,
            &trailer,
            merged.names_table(),
            &merged,
        );
        println!("{line}");
        let (doc, set) = two_encodings_of_one_content();
        let name = "`two_encodings_of_one_content_merge`";
        println!(
            "{}",
            row(name, &set, &Dict::new(), doc.names_table(), &doc).0
        );
        let mut pdfs = Vec::new();
        committed_pdfs(&root(), &mut pdfs);
        pdfs.sort();
        let scanned = pdfs.len();
        for path in pdfs {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(doc) = crate::doc::CosDocument::open(bytes) else {
                continue;
            };
            let (set, trailer) = rewrite_set(&doc);
            let name = path
                .strip_prefix(root())
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            let (line, differ) = row(
                &format!("`{name}`"),
                &set,
                &trailer,
                doc.names_table(),
                &doc,
            );
            if differ {
                println!("{line}");
            }
        }
        println!("{scanned} committed PDFs scanned; a row for each whose two modes differ");
    }
}
