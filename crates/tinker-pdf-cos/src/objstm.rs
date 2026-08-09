//! Object streams (7.5.7).
//!
//! An object stream holds `/N` pairs of (object number, offset relative to
//! `/First`) followed by the objects themselves. Both the decompressed bytes
//! and the parsed pair table are cached under the container's object number,
//! so fifty objects in one `ObjStm` cost one inflate.
//!
//! Two rules from 7.5.7 are enforced rather than assumed: contained objects
//! have generation 0, and none of them is itself a stream. A violation warns
//! and skips the entry instead of failing the container.
//!
//! Strings inside an object stream are **not** decrypted individually (7.6.2)
//! — the container stream was already decrypted as a whole, so decrypting its
//! contents again would corrupt them. That is the entire reason the loader
//! threads an `in_objstm` flag down to the string path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::lexer::{Lexer, TokenKind};
use crate::limits;
use crate::store::LockExt;
use crate::warn::{WarningKind, WarningSink};

/// One decompressed object stream and where each object starts in it.
#[derive(Debug, Default)]
pub(crate) struct ObjStm {
    pub(crate) data: Vec<u8>,
    /// (object number, absolute offset into `data`), in file order.
    pub(crate) entries: Vec<(u32, u64)>,
}

impl ObjStm {
    /// The entry a type-2 cross-reference entry names.
    ///
    /// The index is a hint that broken producers get wrong, so a mismatch
    /// falls back to searching by object number, which is the value that
    /// actually identifies the object.
    pub(crate) fn locate(
        &self,
        num: u32,
        idx: u32,
        at: u64,
        sink: &mut WarningSink,
    ) -> Option<u64> {
        if let Some((found, offset)) = self.entries.get(idx as usize) {
            if *found == num {
                return Some(*offset);
            }
        }
        // The index disagreed, or was past the end. Either way the object
        // number is what identifies the object, so search by that — and report
        // the disagreement whether or not the search succeeds, because a
        // type-2 entry naming an object its container does not hold is damage
        // even when nothing can be done about it.
        let found = self
            .entries
            .iter()
            .find(|(found, _)| *found == num)
            .map(|(_, offset)| *offset);
        sink.warn(at, WarningKind::ObjStmIndexMismatch);
        found
    }

    /// Reads the `/N` pairs of the `/First` prologue.
    ///
    /// `n` and `first` are what the dictionary claimed. When either is missing
    /// or disagrees with the bytes, pairs are lexed until they stop making
    /// sense and everything that parsed is kept — a truncated object stream is
    /// still worth most of its objects.
    pub(crate) fn parse(
        data: Vec<u8>,
        n: Option<u64>,
        first: Option<u64>,
        sink: &mut WarningSink,
        at: u64,
    ) -> ObjStm {
        let mut scratch = WarningSink::new();
        let mut lexer = Lexer::new(&data);
        let mut pairs: Vec<(u32, u64)> = Vec::new();
        let limit = n
            .unwrap_or(limits::MAX_OBJSTM_ENTRIES as u64)
            .min(limits::MAX_OBJSTM_ENTRIES as u64);

        let mut prologue_end = 0u64;
        while (pairs.len() as u64) < limit {
            let save = lexer.offset();
            // The prologue ends at /First; anything past it is object data.
            if first.is_some_and(|f| save >= f) {
                break;
            }
            let TokenKind::Int(num) = lexer.next_token(&mut scratch).kind else {
                lexer.seek(save);
                break;
            };
            let TokenKind::Int(offset) = lexer.next_token(&mut scratch).kind else {
                lexer.seek(save);
                break;
            };
            let (Ok(num), Ok(offset)) = (u32::try_from(num), u64::try_from(offset)) else {
                lexer.seek(save);
                break;
            };
            pairs.push((num, offset));
            prologue_end = lexer.offset();
        }

        // Without a usable /First the objects begin at the first token after
        // the pairs, which is where every producer puts them.
        let base = match first {
            Some(first) if first <= data.len() as u64 => first,
            _ => {
                sink.warn(at, WarningKind::ObjStmHeaderBad);
                lexer.seek(prologue_end);
                lexer.skip_space();
                lexer.offset()
            }
        };
        if n.is_none() {
            sink.warn(at, WarningKind::ObjStmHeaderBad);
        } else if n != Some(pairs.len() as u64) {
            sink.warn(at, WarningKind::ObjStmPairsTruncated);
        }

        let mut entries = Vec::with_capacity(pairs.len());
        let mut out_of_range = false;
        for (num, relative) in pairs {
            let absolute = base.saturating_add(relative);
            if absolute >= data.len() as u64 {
                out_of_range = true;
                continue;
            }
            entries.push((num, absolute));
        }
        if out_of_range {
            sink.warn(at, WarningKind::ObjStmEntryOutOfRange);
        }
        ObjStm { data, entries }
    }
}

/// One decompressed object stream per container object number.
#[derive(Debug, Default)]
pub(crate) struct ObjStmCache {
    streams: RwLock<HashMap<u32, Arc<ObjStm>>>,
    decodes: AtomicU64,
}

impl ObjStmCache {
    pub(crate) fn new() -> ObjStmCache {
        ObjStmCache::default()
    }

    pub(crate) fn get(&self, num: u32) -> Option<Arc<ObjStm>> {
        self.streams.read_lock().get(&num).cloned()
    }

    /// Publishes a decoded stream, first writer winning as everywhere else.
    pub(crate) fn publish(&self, num: u32, stream: Arc<ObjStm>) -> Arc<ObjStm> {
        let mut streams = self.streams.write_lock();
        Arc::clone(streams.entry(num).or_insert(stream))
    }

    /// Counts one decode. Exposed so the cache can be proven to work: a
    /// document that resolves fifty objects out of one container must report
    /// one decode, not fifty.
    pub(crate) fn count_decode(&self) {
        self.decodes.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn decodes(&self) -> u64 {
        self.decodes.load(Ordering::Relaxed)
    }
}

/// Whether the object that ended at `end` is followed by the `stream`
/// keyword, which 7.5.7 forbids inside an object stream.
pub(crate) fn is_stream_at(data: &[u8], end: u64) -> bool {
    let mut scratch = WarningSink::new();
    let mut lexer = Lexer::at(data, end);
    matches!(
        lexer.next_token(&mut scratch).kind,
        TokenKind::Keyword(crate::lexer::Keyword::Stream)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &[u8], n: Option<u64>, first: Option<u64>) -> (ObjStm, Vec<WarningKind>) {
        let mut sink = WarningSink::new();
        let stm = ObjStm::parse(data.to_vec(), n, first, &mut sink, 0);
        let kinds = sink.warnings().iter().map(|w| w.kind).collect();
        (stm, kinds)
    }

    #[test]
    fn a_well_formed_prologue_parses() {
        let data = b"7 0 8 6 9 12 (aaa)(bbb)(ccc)";
        let (stm, warnings) = parse(data, Some(3), Some(15));
        assert_eq!(stm.entries, [(7, 15), (8, 21), (9, 27)]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_missing_first_is_recovered_from_the_pairs() {
        let data = b"7 0 8 6 (aaa)(bbb)";
        let (stm, warnings) = parse(data, Some(2), None);
        assert_eq!(stm.entries, [(7, 8), (8, 14)]);
        assert_eq!(warnings, [WarningKind::ObjStmHeaderBad]);
    }

    #[test]
    fn a_missing_n_lexes_pairs_until_they_stop() {
        let data = b"7 0 8 6 (aaa)(bbb)";
        let (stm, warnings) = parse(data, None, Some(8));
        assert_eq!(stm.entries, [(7, 8), (8, 14)]);
        assert_eq!(warnings, [WarningKind::ObjStmHeaderBad]);
    }

    #[test]
    fn too_few_pairs_keeps_what_parsed() {
        let data = b"7 0 8 6 (aaa)(bbb)";
        let (stm, warnings) = parse(data, Some(9), Some(8));
        assert_eq!(stm.entries, [(7, 8), (8, 14)]);
        assert_eq!(warnings, [WarningKind::ObjStmPairsTruncated]);
    }

    #[test]
    fn offsets_past_the_data_are_dropped() {
        let data = b"7 0 8 900 (aaa)";
        let (stm, warnings) = parse(data, Some(2), Some(10));
        assert_eq!(stm.entries, [(7, 10)]);
        assert_eq!(warnings, [WarningKind::ObjStmEntryOutOfRange]);
    }

    #[test]
    fn locate_prefers_the_index_and_falls_back_to_the_number() {
        let mut sink = WarningSink::new();
        let stm = ObjStm {
            data: Vec::new(),
            entries: vec![(7, 10), (8, 20)],
        };
        assert_eq!(stm.locate(8, 1, 0, &mut sink), Some(20));
        assert!(sink.is_empty());
        assert_eq!(stm.locate(8, 0, 0, &mut sink), Some(20));
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::ObjStmIndexMismatch]
        );
        // An object the container simply does not hold is reported too.
        assert_eq!(stm.locate(9, 0, 0, &mut sink), None);
        assert_eq!(sink.len(), 2);
        // An index past the end still finds the object by number.
        assert_eq!(stm.locate(7, 99, 0, &mut sink), Some(10));
        assert_eq!(sink.len(), 3);
    }

    #[test]
    fn the_decode_counter_counts_decodes() {
        let cache = ObjStmCache::new();
        assert_eq!(cache.decodes(), 0);
        cache.count_decode();
        let first = cache.publish(3, Arc::new(ObjStm::default()));
        let second = cache.publish(3, Arc::new(ObjStm::default()));
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.decodes(), 1);
    }
}
