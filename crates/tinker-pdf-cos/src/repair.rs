//! The repair scanner: one forward pass that finds every object the buffer
//! actually contains, whatever the cross-reference tables claim.
//!
//! Levels 2 and 3 of the leniency ladder both run on this index. It is built
//! at most once per document and only when something has already gone wrong,
//! because the pass parses every object it finds.
//!
//! Two things keep the index honest. Each `digits ws digits ws obj` hit is
//! matched at token boundaries, so `endobj` and `1 0 objx` are not hits; and
//! each hit that turns out to be a stream has its body skipped using the same
//! `endstream` recovery a normal read would use, so `N G obj` byte sequences
//! *inside* stream data never enter the index.
//!
//! The pass is linear in the buffer. It asks for the next `obj`, `trailer`
//! and `endstream` at a cursor that only moves forward, and each keyword's
//! search ([`Searches`]) remembers its next hit, so it looks again only once
//! the cursor has passed that hit. Asked afresh at every object, the search
//! for a `trailer` would look to the end of a file whose cross-reference
//! sections are streams once for every object the file holds.
//!
//! The pass parses into a scratch sink whose warnings are dropped. A scanned
//! object that is never read should not describe its own damage in the
//! document's warning list; the repairs that are actually used warn at the
//! point of use.

use std::collections::BTreeMap;

use crate::doc::DocNames;
use crate::lexer::{is_regular, is_whitespace};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::parse::parse_indirect_at;
use crate::streams::resolve_extent_in;
use crate::warn::{ParseSteps, WarningKind, WarningSink};

/// The first occurrence of `needle` in `hay` at or after `from`.
pub(crate) fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    first_from(hay, needle, from)
}

/// [`find_from`], adding the bytes it looked through to `examined`: from
/// `from` to the end of the hit, or to the end of `hay` when there is none.
///
/// **Counted in every search rather than in the pass**, so that a pass which
/// goes back to searching afresh at every hit is counted whoever writes it:
/// the pass searches only through [`Searches`], whose every look comes here,
/// and what it adds up is [`crate::work::Stats::bytes_scanned`] — the one
/// count the pass's own test and MS-01's tally both read.
fn find_counted(hay: &[u8], needle: &[u8], from: usize, examined: &mut u64) -> Option<usize> {
    let found = first_from(hay, needle, from);
    let end = found.map_or(hay.len(), |p| p.saturating_add(needle.len()));
    let looked = u64::try_from(end.saturating_sub(from)).unwrap_or(u64::MAX);
    *examined = examined.saturating_add(looked);
    found
}

fn first_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let Some(&first) = needle.first() else {
        return Some(from.min(hay.len()));
    };
    let mut i = from;
    loop {
        let rest = hay.get(i..)?;
        let k = rest.iter().position(|&b| b == first)?;
        let p = i + k;
        match hay.get(p..p.checked_add(needle.len())?) {
            Some(window) if window == needle => return Some(p),
            Some(_) => i = p + 1,
            None => return None,
        }
    }
}

/// The last occurrence of `needle` that starts at or after `from`.
pub(crate) fn rfind_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let mut found = None;
    let mut i = from;
    while let Some(p) = find_from(hay, needle, i) {
        found = Some(p);
        i = p + 1;
    }
    found
}

/// True when `at` is a token boundary in front of a keyword: the byte before
/// it, if any, must not be a regular character (7.2.2).
fn boundary_before(buf: &[u8], at: usize) -> bool {
    match at.checked_sub(1).and_then(|i| buf.get(i)) {
        Some(&b) => !is_regular(b),
        None => true,
    }
}

/// True when the keyword ending at `at` is followed by a boundary.
fn boundary_after(buf: &[u8], at: usize) -> bool {
    match buf.get(at) {
        Some(&b) => !is_regular(b),
        None => true,
    }
}

/// Walks backwards from the `obj` keyword at `at` over `digits ws digits ws`.
///
/// Returns the offset of the first digit of the object number and the two
/// numbers. Digit runs are length-limited so a megabyte of zeroes in stream
/// data cannot turn into a scan hit.
pub(crate) fn header_before_obj(buf: &[u8], at: usize) -> Option<(u64, u32, u16)> {
    let mut i = at;
    let ws_end = i;
    while i > 0 && buf.get(i - 1).copied().is_some_and(is_whitespace) {
        i -= 1;
    }
    if i == ws_end {
        return None;
    }
    let gen_end = i;
    while i > 0 && buf.get(i - 1).is_some_and(u8::is_ascii_digit) {
        i -= 1;
    }
    let gen_start = i;
    if gen_start == gen_end || gen_end - gen_start > 10 {
        return None;
    }
    let ws_end = i;
    while i > 0 && buf.get(i - 1).copied().is_some_and(is_whitespace) {
        i -= 1;
    }
    if i == ws_end {
        return None;
    }
    let num_end = i;
    while i > 0 && buf.get(i - 1).is_some_and(u8::is_ascii_digit) {
        i -= 1;
    }
    let num_start = i;
    if num_start == num_end || num_end - num_start > 10 || !boundary_before(buf, num_start) {
        return None;
    }
    let num = digits(buf.get(num_start..num_end)?)?;
    let gen = digits(buf.get(gen_start..gen_end)?)?;
    Some((
        num_start as u64,
        u32::try_from(num).ok()?,
        u16::try_from(gen).ok()?,
    ))
}

fn digits(bytes: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    for &b in bytes {
        value = value.checked_mul(10)?.checked_add(u64::from(b - b'0'))?;
    }
    Some(value)
}

/// The next `N G obj` header at or after `from`, as an offset.
pub(crate) fn next_object_header(buf: &[u8], from: u64) -> Option<u64> {
    Searches::new(buf).next_object_header(from)
}

/// One keyword's forward search over one buffer, which remembers its last
/// answer.
///
/// A search from `from` that found the keyword at `hit` has shown that it
/// does not occur in `from..hit`, so it answers every later search from
/// anywhere in `from..=hit` without looking again; one that found nothing
/// answers every later search at or after `from`. The searches that answer a
/// cursor which only moves forward therefore look through the buffer about
/// once between them, however often it asks.
#[derive(Clone, Copy, Debug)]
struct NextHit {
    needle: &'static [u8],
    /// The last search that looked: where it started, and what it found.
    last: Option<(usize, Option<usize>)>,
    /// The bytes every search that looked has looked through.
    examined: u64,
}

impl NextHit {
    const fn new(needle: &'static [u8]) -> NextHit {
        NextHit {
            needle,
            last: None,
            examined: 0,
        }
    }

    /// What [`find_from`] answers for `from`.
    fn find(&mut self, buf: &[u8], from: usize) -> Option<usize> {
        if let Some((start, hit)) = self.last {
            if start <= from && hit.is_none_or(|at| from <= at) {
                return hit;
            }
        }
        let hit = find_counted(buf, self.needle, from, &mut self.examined);
        self.last = Some((from, hit));
        hit
    }
}

/// The keyword searches of a forward walk over one buffer: the next `obj`,
/// `trailer` and `endstream` at or after a position, each remembering its
/// next hit ([`NextHit`]).
///
/// Every answer is the one [`find_from`] gives; only the looking is saved.
pub(crate) struct Searches<'a> {
    buf: &'a [u8],
    obj: NextHit,
    trailer: NextHit,
    endstream: NextHit,
}

impl<'a> Searches<'a> {
    pub(crate) const fn new(buf: &'a [u8]) -> Searches<'a> {
        Searches {
            buf,
            obj: NextHit::new(b"obj"),
            trailer: NextHit::new(b"trailer"),
            endstream: NextHit::new(b"endstream"),
        }
    }

    /// The buffer searched.
    pub(crate) const fn buf(&self) -> &'a [u8] {
        self.buf
    }

    /// The bytes the three searches have looked through between them.
    pub(crate) fn examined(&self) -> u64 {
        self.obj
            .examined
            .saturating_add(self.trailer.examined)
            .saturating_add(self.endstream.examined)
    }

    /// The first `obj` at or after `from`.
    pub(crate) fn next_obj(&mut self, from: usize) -> Option<usize> {
        self.obj.find(self.buf, from)
    }

    /// The first `trailer` at or after `from`.
    pub(crate) fn next_trailer(&mut self, from: usize) -> Option<usize> {
        self.trailer.find(self.buf, from)
    }

    /// The first `endstream` at or after `from`.
    pub(crate) fn next_endstream(&mut self, from: usize) -> Option<usize> {
        self.endstream.find(self.buf, from)
    }

    /// The next `N G obj` header at or after `from`, as an offset.
    pub(crate) fn next_object_header(&mut self, from: u64) -> Option<u64> {
        let buf = self.buf;
        let mut i = usize::try_from(from).unwrap_or(usize::MAX).min(buf.len());
        while let Some(p) = self.next_obj(i) {
            if boundary_after(buf, p + 3) {
                if let Some((start, _, _)) = header_before_obj(buf, p) {
                    if start >= from {
                        return Some(start);
                    }
                }
            }
            i = p + 1;
        }
        None
    }
}

/// Where one object was found, and which generation the file spelled there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScanHit {
    pub offset: u64,
    pub gen: u16,
}

/// The result of the forward pass.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScanIndex {
    objects: BTreeMap<u32, ScanHit>,
    /// `trailer` dictionaries and `/Type /XRef` stream dictionaries, in the
    /// order found, which is ascending by offset.
    trailers: Vec<Dict>,
    /// Object numbers of every `/Type /ObjStm` stream found.
    objstms: Vec<u32>,
    catalog: Option<ObjRef>,
    info: Option<ObjRef>,
    /// What the pass read: the tokens its parses lexed, the objects they
    /// parsed and the bytes its searches looked through.
    steps: ParseSteps,
}

impl ScanIndex {
    /// Where object `num` lives, if the pass found it.
    pub(crate) fn get(&self, num: u32) -> Option<ScanHit> {
        self.objects.get(&num).copied()
    }

    pub(crate) fn objects(&self) -> impl Iterator<Item = (u32, ScanHit)> + '_ {
        self.objects.iter().map(|(n, h)| (*n, *h))
    }

    pub(crate) fn object_streams(&self) -> &[u32] {
        &self.objstms
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// What the pass that built this index read, for the operation it ran in.
    pub(crate) fn steps(&self) -> ParseSteps {
        self.steps
    }

    /// `base` filled in from every trailer-shaped dictionary the pass found,
    /// plus a synthesized `/Root` when nothing named one.
    ///
    /// Ascending offset means later in the file, and later means newer in an
    /// incrementally updated file (7.5.6), so the merge runs in reverse and
    /// keeps the first value it sees for each key. `base` is whatever the
    /// cross-reference chain managed to read before it was discarded — a
    /// trailer can be perfectly good while every offset around it lies.
    pub(crate) fn synthesized_trailer(&self, base: Dict, sink: &mut WarningSink) -> Dict {
        let mut trailer = base;
        let was_empty = trailer.is_empty();
        for dict in self.trailers.iter().rev() {
            for (key, value) in dict.iter() {
                if !trailer.contains_key(*key) {
                    trailer.insert(*key, value.clone());
                }
            }
        }
        if was_empty && !trailer.is_empty() {
            sink.warn(0, WarningKind::TrailerSynthesized);
        }
        if !matches!(
            trailer.get(Name::ROOT),
            Some(Object::Ref(_) | Object::Dict(_))
        ) {
            match self.catalog {
                Some(r) => {
                    trailer.insert(Name::ROOT, Object::Ref(r));
                    sink.warn(0, WarningKind::RootSynthesized);
                }
                None => sink.warn(0, WarningKind::RootMissing),
            }
        }
        if !trailer.contains_key(Name::INFO) {
            if let Some(r) = self.info {
                trailer.insert(Name::INFO, Object::Ref(r));
            }
        }
        trailer
    }

    /// The forward pass.
    pub(crate) fn build(buf: &[u8], dn: &DocNames) -> ScanIndex {
        let mut index = ScanIndex::default();
        let mut scratch = WarningSink::new();
        let mut search = Searches::new(buf);
        let mut i = 0usize;

        loop {
            let obj_hit = search.next_obj(i);
            let trailer_hit = search.next_trailer(i);
            let next = match (obj_hit, trailer_hit) {
                (None, None) => break,
                (Some(a), None) => Hit::Object(a),
                (None, Some(b)) => Hit::Trailer(b),
                (Some(a), Some(b)) if a <= b => Hit::Object(a),
                (Some(_), Some(b)) => Hit::Trailer(b),
            };
            let advanced = match next {
                Hit::Object(p) => index.take_object(&mut search, p, dn, &mut scratch),
                Hit::Trailer(p) => index.take_trailer(buf, p, dn, &mut scratch),
            };
            // Always advance: a hit that produced nothing must not be retried.
            i = advanced.max(i + 1);
            scratch.take();
        }
        index.steps = scratch.steps();
        index.steps.scanned = search.examined();
        index
    }

    fn take_object(
        &mut self,
        search: &mut Searches<'_>,
        at: usize,
        dn: &DocNames,
        scratch: &mut WarningSink,
    ) -> usize {
        let buf = search.buf();
        if !boundary_after(buf, at + 3) {
            return at + 3;
        }
        let Some((start, num, gen)) = header_before_obj(buf, at) else {
            return at + 3;
        };
        let Some(parsed) = parse_indirect_at(buf, start, &dn.table, scratch) else {
            return at + 3;
        };

        // 7.5.6: duplicates keep the higher generation, then the higher
        // offset. The pass runs forward, so a later hit always has the higher
        // offset and a same-generation duplicate simply replaces.
        let keep = match self.objects.get(&num) {
            Some(existing) => gen >= existing.gen,
            None => true,
        };
        if keep {
            self.objects.insert(num, ScanHit { offset: start, gen });
        }

        let dict = parsed.object.as_dict();
        let type_name = dict.and_then(|d| d.get_name(Name::TYPE));
        if type_name == Some(dn.catalog) {
            self.catalog = Some(ObjRef::new(num, gen));
        } else if type_name == Some(dn.obj_stm) {
            self.objstms.push(num);
        } else if type_name.is_none() && dict.is_some_and(|d| dn.looks_like_info(d)) {
            self.info = Some(ObjRef::new(num, gen));
        }
        if type_name == Some(dn.xref) {
            if let Some(d) = dict {
                self.trailers.push(d.clone());
            }
        }

        match parsed.object.as_stream() {
            Some(stream) => {
                let range =
                    resolve_extent_in(search, stream.data_start, stream.len_hint, None, scratch);
                let end = usize::try_from(range.end)
                    .unwrap_or(usize::MAX)
                    .min(buf.len());
                // Past the data and past the `endstream` that closed it, so a
                // second copy of the keyword inside the next object's data is
                // never mistaken for this one's.
                match search.next_endstream(end) {
                    Some(p) => p + b"endstream".len(),
                    None => end,
                }
            }
            None => usize::try_from(parsed.end_offset)
                .unwrap_or(usize::MAX)
                .min(buf.len()),
        }
    }

    fn take_trailer(
        &mut self,
        buf: &[u8],
        at: usize,
        dn: &DocNames,
        scratch: &mut WarningSink,
    ) -> usize {
        let end = at + b"trailer".len();
        if !boundary_before(buf, at) || !boundary_after(buf, end) {
            return end;
        }
        let parsed = crate::parse::parse_object_at(buf, end as u64, &dn.table, scratch);
        if let Object::Dict(dict) = parsed.object {
            self.trailers.push(dict);
        }
        usize::try_from(parsed.end_offset)
            .unwrap_or(usize::MAX)
            .min(buf.len())
    }
}

enum Hit {
    Object(usize),
    Trailer(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_and_rfind_agree_with_the_buffer() {
        let hay = b"aXbXcX";
        assert_eq!(find_from(hay, b"X", 0), Some(1));
        assert_eq!(find_from(hay, b"X", 2), Some(3));
        assert_eq!(find_from(hay, b"Z", 0), None);
        assert_eq!(rfind_from(hay, b"X", 0), Some(5));
        assert_eq!(rfind_from(hay, b"X", 4), Some(5));
        assert_eq!(find_from(hay, b"cX", 0), Some(4));
        assert_eq!(find_from(hay, b"cXq", 0), None);
        assert_eq!(find_from(hay, b"", 3), Some(3));
        assert_eq!(find_from(hay, b"a", 99), None);
    }

    #[test]
    fn header_match_needs_token_boundaries() {
        // `obj` inside `endobj` has a regular character before it.
        let buf = b"1 0 endobj";
        let at = find_from(buf, b"obj", 0).expect("a hit");
        assert_eq!(header_before_obj(buf, at), None);

        let buf = b"12 3 obj";
        let at = find_from(buf, b"obj", 0).expect("a hit");
        assert_eq!(header_before_obj(buf, at), Some((0, 12, 3)));

        // No separation between the numbers.
        let buf = b"12 obj";
        let at = find_from(buf, b"obj", 0).expect("a hit");
        assert_eq!(header_before_obj(buf, at), None);

        // A regular character in front of the object number.
        let buf = b"x1 0 obj";
        let at = find_from(buf, b"obj", 0).expect("a hit");
        assert_eq!(header_before_obj(buf, at), None);

        // Absurd digit runs are not object numbers.
        let buf = b"000000000000 0 obj";
        let at = find_from(buf, b"obj", 0).expect("a hit");
        assert_eq!(header_before_obj(buf, at), None);
    }

    #[test]
    fn next_object_header_skips_endobj() {
        let buf = b"1 0 obj (a) endobj\n2 0 obj (b) endobj";
        assert_eq!(next_object_header(buf, 0), Some(0));
        assert_eq!(next_object_header(buf, 1), Some(19));
        assert_eq!(next_object_header(buf, 20), None);
    }

    #[test]
    fn a_remembered_search_answers_as_a_fresh_one() {
        let buf: &[u8] = b"1 0 obj << /Length 2 >> stream\nobjtrailer\nendstream endobj\n\
            trailer << /Size 3 >> 2 0 obj (endstream) endobj trailer";
        let fresh = |from: usize| {
            [
                find_from(buf, b"obj", from),
                find_from(buf, b"trailer", from),
                find_from(buf, b"endstream", from),
            ]
        };
        let asked = |search: &mut Searches<'_>, from: usize| {
            [
                search.next_obj(from),
                search.next_trailer(from),
                search.next_endstream(from),
            ]
        };
        // Forward, where the remembered hits answer; then backward from past
        // the buffer's end, where every search looks again.
        let forward: Vec<usize> = (0..buf.len() + 2).collect();
        let back: Vec<usize> = forward.iter().rev().copied().collect();
        for order in [forward, back] {
            let mut search = Searches::new(buf);
            for &from in &order {
                assert_eq!(
                    asked(&mut search, from),
                    fresh(from),
                    "searching from {from}"
                );
            }
            let mut headers = Searches::new(buf);
            for &from in &order {
                let from = from as u64;
                assert_eq!(
                    headers.next_object_header(from),
                    Searches::new(buf).next_object_header(from),
                    "the next header from {from}"
                );
            }
        }
    }

    /// A damaged file of `objects` objects, and where each one starts: the
    /// damage that makes each of the pass's searches look to the end of the
    /// file when it is asked afresh at every hit.
    ///
    /// Its first seven-eighths are an original revision's objects in four
    /// shapes — a dictionary, a string, a stream whose `/Length` is right and
    /// one whose `/Length` is indirect — with no `trailer` among them, so the
    /// next `trailer` from any of them is past them all. An eighth as many
    /// `trailer` dictionaries follow, so the next `obj` from any of them is
    /// past them all. The last eighth is an update whose cross-reference
    /// section was a stream and whose streams were cut short of their
    /// `endstream`, so the next `endstream` from any of them is none at all.
    /// Objects are numbered from 100 000, so that each shape is the same
    /// length in every file and twenty times the objects is twenty times the
    /// bytes.
    fn damaged(objects: u32) -> (Vec<u8>, Vec<(u32, u64)>) {
        const SHAPES: [&[u8]; 4] = [
            b"<< /Type /Annot /Subtype /Text /Rect [0 0 1 1] >>\nendobj\n",
            b"(a string)\nendobj\n",
            b"<< /Length 18 >>\nstream\nq 1 0 0 1 0 0 cm Q\nendstream\nendobj\n",
            b"<< /Length 100000 0 R >>\nstream\nq 1 0 0 1 0 0 cm Q\nendstream\nendobj\n",
        ];
        const CUT: &[u8] = b"<< /Length 18 >>\nstream\nq 1 0 0 1 0 0 cm Q\n";
        const TRAILER: &[u8] = b"trailer\n<< /Size 120001 /Root 100000 0 R >>\n";
        let original = objects - objects / 8;
        let mut file = b"%PDF-1.7\n".to_vec();
        let mut starts = Vec::new();
        for k in 0..objects {
            if k == original {
                for _ in original..objects {
                    file.extend_from_slice(TRAILER);
                }
            }
            let num = 100_000 + k;
            starts.push((num, file.len() as u64));
            file.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            let shape = SHAPES[k as usize % SHAPES.len()];
            file.extend_from_slice(if k < original { shape } else { CUT });
        }
        (file, starts)
    }

    /// The index over `file`, and the bytes its searches looked through.
    fn counted(file: &[u8]) -> (ScanIndex, usize) {
        let names = DocNames::new();
        let index = ScanIndex::build(file, &names);
        let examined = usize::try_from(index.steps().scanned).unwrap_or(usize::MAX);
        (index, examined)
    }

    #[test]
    fn the_index_costs_work_linear_in_the_objects() {
        let work = |objects: u32| {
            let (file, starts) = damaged(objects);
            let (index, examined) = counted(&file);
            // What the pass finds is what the file holds: every object where
            // it starts, so each cut stream ended at the next header, and
            // every trailer read.
            let found: Vec<(u32, u64)> = index.objects().map(|(n, h)| (n, h.offset)).collect();
            assert_eq!(found, starts, "the objects of a file of {objects}");
            assert_eq!(index.trailers.len(), objects as usize / 8);
            // Three keywords, each looked for through the file once at most.
            assert!(
                examined <= 3 * file.len(),
                "the index over {objects} objects looked through {examined} bytes of a \
                 file of {}",
                file.len()
            );
            examined
        };
        let small = work(1_000);
        let large = work(20_000);
        // Twenty times the objects, at most about twenty times the work. A
        // pass that searched for the next `trailer` afresh at every object
        // would make twenty times the searches over twenty times the bytes.
        assert!(
            large <= 21 * small,
            "twenty times the objects cost {large} bytes looked through against {small}"
        );
    }
}
