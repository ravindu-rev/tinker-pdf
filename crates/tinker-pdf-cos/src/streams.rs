//! Stream data: the `/Length` policy of 7.3.8.2 and the three-tier access API.
//!
//! The tiers exist because collapsing them is how a stream API goes vague.
//! [`CosDocument::stream_raw_encrypted`] is forensic — the exact bytes the
//! file holds, for signature checking and byte-identical revision copying.
//! [`CosDocument::stream_raw`] is extraction — decrypted, undecoded, so a JPEG
//! extracted is the JPEG embedded. [`CosDocument::stream_decoded`] is what an
//! interpreter eats, and it is the only tier with a decode ceiling.

use core::ops::Range;

use tinker_pdf_filters as filters;

use crate::doc::{CosDocument, CosError, DocNames};
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object, StreamObj};
use crate::repair::{find_from, next_object_header};
use crate::store::{LockExt, ResolveCtx};
use crate::warn::{WarningKind, WarningSink};

/// The bytes a `u64` range names, clamped to the buffer.
pub(crate) fn slice_range<'a>(buf: &'a [u8], range: &Range<u64>) -> &'a [u8] {
    let start = usize::try_from(range.start)
        .unwrap_or(usize::MAX)
        .min(buf.len());
    let end = usize::try_from(range.end)
        .unwrap_or(usize::MAX)
        .min(buf.len());
    buf.get(start..end).unwrap_or(&[])
}

/// True when `endstream` follows `at`, optionally after an end-of-line.
///
/// 7.3.8.2 requires an EOL before the keyword; blanks are tolerated because
/// producers pad. The match is anchored, so nothing is scanned for.
fn endstream_follows(buf: &[u8], at: u64) -> bool {
    let mut i = usize::try_from(at).unwrap_or(usize::MAX);
    let mut skipped = 0usize;
    while skipped < limits::MAX_STREAM_EOL_SKIP
        && buf.get(i).copied().is_some_and(crate::lexer::is_whitespace)
    {
        i += 1;
        skipped += 1;
    }
    buf.get(i..i + b"endstream".len()) == Some(&b"endstream"[..])
}

/// The data extent of a stream whose `/Length` has already been resolved.
///
/// 7.3.8.2, verbatim policy: trust `/Length` when the bytes it points at are
/// `endstream`; otherwise scan forward for the keyword and take that extent,
/// warning with both lengths; and if there is no `endstream` at all, truncate
/// at the next `N G obj` header or at end of buffer. Trusting the keyword over
/// the number is what every surviving reader does, because a declared-but-
/// wrong length is among the most common real-world damage.
pub(crate) fn resolve_extent(
    buf: &[u8],
    data_start: u64,
    declared: Option<u64>,
    object: Option<ObjRef>,
    sink: &mut WarningSink,
) -> Range<u64> {
    let len = buf.len() as u64;
    let start = data_start.min(len);
    if let Some(declared) = declared {
        let end = start.saturating_add(declared);
        if end <= len && endstream_follows(buf, end) {
            return start..end;
        }
    }

    let from = usize::try_from(start).unwrap_or(usize::MAX).min(buf.len());
    match find_from(buf, b"endstream", from) {
        Some(keyword) => {
            // One EOL before the keyword belongs to the file syntax, not to
            // the data (7.3.8.2).
            let mut end = keyword;
            if end > from && buf.get(end - 1) == Some(&b'\n') {
                end -= 1;
            }
            if end > from && buf.get(end - 1) == Some(&b'\r') {
                end -= 1;
            }
            let end = end as u64;
            sink.warn_at(
                start,
                object,
                WarningKind::StreamLengthRecovered {
                    declared,
                    actual: end - start,
                },
            );
            start..end
        }
        None => {
            let end = next_object_header(buf, start).unwrap_or(len);
            sink.warn_at(start, object, WarningKind::StreamEndstreamMissing);
            start..end.max(start)
        }
    }
}

/// Turns `/Filter` and `/DecodeParms` into the filter crate's specs.
///
/// The translation happens here on purpose: `tinker-pdf-filters` never sees a
/// [`Name`], a [`Dict`] or an indirect reference, so it stays a byte-in,
/// byte-out crate with no PDF types.
pub(crate) fn build_chain(
    dict: &Dict,
    dn: &DocNames,
    resolve: &mut dyn FnMut(&Object) -> Object,
    at: u64,
    sink: &mut WarningSink,
) -> Vec<filters::FilterSpec> {
    let filter = match dict.get(Name::FILTER) {
        Some(object) => resolve(object),
        None => Object::Null,
    };
    let parms = match dict.get(Name::DECODE_PARMS).or_else(|| dict.get(dn.dp)) {
        Some(object) => resolve(object),
        None => Object::Null,
    };

    let named: Vec<Name> = match &filter {
        Object::Name(n) => vec![*n],
        Object::Array(items) => items.iter().filter_map(|o| resolve(o).as_name()).collect(),
        _ => Vec::new(),
    };
    let params: Vec<Option<Dict>> = match &parms {
        Object::Dict(d) => vec![Some(d.clone())],
        Object::Array(items) => items
            .iter()
            .map(|o| resolve(o).as_dict().cloned())
            .collect(),
        _ => Vec::new(),
    };

    let mut chain = Vec::with_capacity(named.len());
    for (i, name) in named.iter().enumerate() {
        // 7.4.10: a /Crypt filter is handled by the decryptor, and the only
        // one this crate can meet is /Identity. Either way it is not a byte
        // transform, so it never becomes a chain stage.
        if *name == dn.crypt {
            continue;
        }
        let Some(filter) = dn.filter(*name) else {
            sink.warn(at, WarningKind::FilterUnknown);
            break;
        };
        let parms = params.get(i).and_then(Option::as_ref);
        chain.push(spec(filter, parms, dn, resolve));
    }
    chain
}

fn spec(
    filter: filters::Filter,
    parms: Option<&Dict>,
    dn: &DocNames,
    resolve: &mut dyn FnMut(&Object) -> Object,
) -> filters::FilterSpec {
    let mut spec = filters::FilterSpec::new(filter);
    let Some(parms) = parms else { return spec };
    let int = |key: Name, resolve: &mut dyn FnMut(&Object) -> Object| -> Option<i64> {
        parms.get(key).and_then(|o| resolve(o).as_int())
    };

    if matches!(filter, filters::Filter::Flate | filters::Filter::Lzw) {
        // Table 10 defaults.
        let predictor = filters::PredictorParams {
            predictor: int(dn.predictor, resolve)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(1),
            colors: int(dn.colors, resolve)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(1),
            bits_per_component: int(dn.bits_per_component, resolve)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(8),
            columns: int(dn.columns, resolve)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(1),
        };
        if predictor.is_active() {
            spec = spec.with_predictor(predictor);
        }
    }
    if filter == filters::Filter::Lzw {
        // 7.4.4.3: /EarlyChange defaults to 1.
        spec = spec.with_early_change(int(dn.early_change, resolve) != Some(0));
    }
    spec
}

impl CosDocument {
    /// The exact bytes the file holds for this stream: encrypted if the file
    /// is, never decoded.
    ///
    /// This is the forensic tier — signature byte ranges and byte-identical
    /// revision copies read here and nowhere else.
    ///
    /// # Errors
    /// [`CosError::NotAStream`] when the object is not a stream (7.3.8),
    /// including when it does not exist.
    pub fn stream_raw_encrypted(&self, r: ObjRef) -> Result<&[u8], CosError> {
        let object = self.get(r)?;
        let stream = object.as_stream().ok_or(CosError::NotAStream(r))?;
        let range = self.stream_range(r, stream);
        Ok(slice_range(&self.buffer, &range))
    }

    /// The stream's bytes, decrypted, with no filter applied.
    ///
    /// This is the extraction tier: a `DCTDecode` stream comes back as the
    /// JPEG the file embedded, byte for byte.
    ///
    /// # Errors
    /// [`CosError::NotAStream`] when the object is not a stream.
    pub fn stream_raw(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        let object = self.get(r)?;
        let stream = object.as_stream().ok_or(CosError::NotAStream(r))?;
        Ok(self.decrypted_bytes(r, stream))
    }

    /// The stream's bytes, decrypted and run through its `/Filter` chain.
    ///
    /// Output is capped at [`limits::MAX_DECODED_STREAM`], so a decompression
    /// bomb costs bounded memory and reports
    /// [`WarningKind::Filter`]`(`[`filters::Warning::OutputCapHit`]`)`. A
    /// chain that ends in an image codec cannot be finished here: the bytes
    /// come back as the codec wants them, with
    /// [`WarningKind::ImageCodecNotDecoded`].
    ///
    /// # Errors
    /// [`CosError::NotAStream`] when the object is not a stream, and
    /// [`CosError::Filter`] when `/DecodeParms` cannot describe any stream.
    pub fn stream_decoded(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        let object = self.get(r)?;
        let stream = object.as_stream().ok_or(CosError::NotAStream(r))?;
        let mut sink = WarningSink::new();
        sink.set_context(Some(r));
        let data = self.decrypted_bytes(r, stream);
        let out = self.decode_with(&data, &stream.dict, r.num, &mut sink);
        self.absorb(sink);
        out
    }

    /// The bytes an image codec should be handed: decoded through every
    /// filter *before* the codec, and no further.
    ///
    /// `[/FlateDecode /DCTDecode]` is an ordinary shape — a producer
    /// compressing the JPEG bytes again — and the image path used to take
    /// [`CosDocument::stream_raw`] for these, which is undecoded bytes. The
    /// JPEG decoder was handed deflate output and refused it; the CCITT
    /// decoder has no refusal path and rendered noise.
    ///
    /// Distinct from [`CosDocument::stream_decoded`] only in not warning
    /// `ImageCodecNotDecoded`: that warning is for a caller who wanted pixels
    /// and did not get them, and here the caller is about to produce them.
    pub fn stream_image_input(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        let object = self.get(r)?;
        let stream = object.as_stream().ok_or(CosError::NotAStream(r))?;
        let mut sink = WarningSink::new();
        sink.set_context(Some(r));
        let data = self.decrypted_bytes(r, stream);
        let out = self.decode_chain(&data, &stream.dict, r.num, &mut sink, false);
        self.absorb(sink);
        out
    }

    /// Decodes already-decrypted bytes against a stream dictionary.
    pub(crate) fn decode_with(
        &self,
        data: &[u8],
        dict: &Dict,
        num: u32,
        sink: &mut WarningSink,
    ) -> Result<Vec<u8>, CosError> {
        self.decode_chain(data, dict, num, sink, true)
    }

    /// The chain, with `warn_undecoded` deciding whether stopping at an image
    /// codec is worth saying so.
    fn decode_chain(
        &self,
        data: &[u8],
        dict: &Dict,
        num: u32,
        sink: &mut WarningSink,
        warn_undecoded: bool,
    ) -> Result<Vec<u8>, CosError> {
        let mut ctx = ResolveCtx::new();
        ctx.push(num);
        // The resolver and the chain builder both want the sink; giving the
        // resolver its own and merging afterwards keeps one `&mut` each.
        let mut resolved = WarningSink::new();
        let chain = {
            let mut resolve = |o: &Object| self.resolve_in(o, &mut ctx, &mut resolved);
            build_chain(dict, &self.names, &mut resolve, 0, sink)
        };
        sink.extend(resolved.take());
        let limits = filters::Limits::new(limits::MAX_DECODED_STREAM);
        match filters::apply_chain(data, &chain, &limits) {
            Ok(filters::ChainOutput::Bytes(decoded)) => {
                for w in decoded.warnings {
                    sink.warn(0, WarningKind::Filter(w));
                }
                Ok(decoded.data)
            }
            Ok(filters::ChainOutput::EncodedImage { data, warnings, .. }) => {
                for w in warnings {
                    sink.warn(0, WarningKind::Filter(w));
                }
                if warn_undecoded {
                    sink.warn(0, WarningKind::ImageCodecNotDecoded);
                }
                Ok(data)
            }
            Err(e) => {
                sink.warn(0, WarningKind::FilterParamsBad);
                Err(CosError::Filter(e))
            }
        }
    }

    /// The raw bytes, run through the decryptor unless 7.6.2 exempts them.
    pub(crate) fn decrypted_bytes(&self, r: ObjRef, stream: &StreamObj) -> Vec<u8> {
        let range = self.stream_range(r, stream);
        let raw = slice_range(&self.buffer, &range);
        if !self.encrypted() || !self.stream_is_encrypted(&stream.dict) {
            return raw.to_vec();
        }
        self.decryptor().decrypt_stream(r, raw)
    }

    /// Whether a stream's bytes are ciphertext, given what the document says.
    ///
    /// Three exemptions, and each of them turns readable bytes into garbage if
    /// it is missed — a stream decrypted that should not have been comes out
    /// as noise, which reads as a corrupt file rather than as this mistake.
    fn stream_is_encrypted(&self, dict: &Dict) -> bool {
        // 7.6.2: a cross-reference stream carries the information needed to
        // find the /Encrypt dictionary, so it can never be encrypted itself.
        if dict.get_name(Name::TYPE) == Some(self.names.xref) {
            return false;
        }

        // 7.6.2: with /EncryptMetadata false the metadata stream is left in
        // the clear, so indexers can read it without the password. Decrypting
        // it anyway yields noise where the document's identity should be.
        if dict.get_name(Name::TYPE) == Some(self.names.metadata) && !self.encrypts_metadata() {
            return false;
        }

        // 7.4.10: a /Crypt filter names which crypt filter applies to this
        // stream, and /Identity means none. A stream marked that way is
        // already plaintext inside an encrypted document — an appearance
        // stream a signature covers, most often.
        if self.crypt_filter_is_identity(dict) {
            return false;
        }

        true
    }

    /// Whether a stream's `/Crypt` filter names `/Identity` (7.4.10).
    ///
    /// The filter's name lives in `/DecodeParms /Name`, positionally matched
    /// to the `/Crypt` entry in the filter array, and defaults to `/Identity`
    /// when absent — which is the case that matters, since that is how a
    /// producer marks a stream as already-plaintext.
    fn crypt_filter_is_identity(&self, dict: &Dict) -> bool {
        let filters = self.resolve_key(dict, Name::FILTER);
        let index = match filters.as_ref() {
            Object::Name(name) => (*name == self.names.crypt).then_some(0usize),
            Object::Array(items) => items
                .iter()
                .position(|item| self.resolve(item).as_name() == Some(self.names.crypt)),
            _ => None,
        };
        let Some(index) = index else {
            return false;
        };

        let parms = self.resolve_key(dict, Name::DECODE_PARMS);
        let entry = match parms.as_ref() {
            Object::Array(items) => items.get(index).map(|o| self.resolve(o)),
            Object::Dict(_) => Some(parms.clone()),
            _ => None,
        };

        // 7.4.10 Table 14: /Name defaults to /Identity.
        let Some(entry) = entry else {
            return true;
        };
        let Some(dict) = entry.as_dict() else {
            return true;
        };
        match dict.get_name(self.names.name_key) {
            Some(name) => name == self.names.identity,
            None => true,
        }
    }

    /// The stream's data extent, computed once per object and remembered.
    ///
    /// Recovery scans the buffer and warns, and neither should happen twice
    /// for the same stream just because a caller read it twice.
    pub(crate) fn stream_range(&self, r: ObjRef, stream: &StreamObj) -> Range<u64> {
        if let Some(range) = self.stream_ranges.read_lock().get(&r.num) {
            return range.clone();
        }
        let mut sink = WarningSink::new();
        let range = resolve_extent(
            &self.buffer,
            stream.data_start,
            stream.len_hint,
            Some(r),
            &mut sink,
        );
        let mut cache = self.stream_ranges.write_lock();
        // Another thread may have computed the same extent meanwhile; its
        // warnings are already recorded, so this one's are dropped.
        if let Some(existing) = cache.get(&r.num) {
            return existing.clone();
        }
        cache.insert(r.num, range.clone());
        drop(cache);
        self.absorb(sink);
        range
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extent(buf: &[u8], start: u64, declared: Option<u64>) -> (Range<u64>, Vec<WarningKind>) {
        let mut sink = WarningSink::new();
        let range = resolve_extent(buf, start, declared, None, &mut sink);
        let kinds = sink.warnings().iter().map(|w| w.kind).collect();
        (range, kinds)
    }

    #[test]
    fn a_correct_length_is_trusted() {
        let buf = b"HELLO\nendstream";
        let (range, warnings) = extent(buf, 0, Some(5));
        assert_eq!(range, 0..5);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_correct_length_with_no_eol_is_trusted() {
        let buf = b"HELLOendstream";
        let (range, warnings) = extent(buf, 0, Some(5));
        assert_eq!(range, 0..5);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_short_length_is_recovered_from_the_keyword() {
        let buf = b"HELLO WORLD\nendstream";
        let (range, warnings) = extent(buf, 0, Some(5));
        assert_eq!(range, 0..11);
        assert_eq!(
            warnings,
            [WarningKind::StreamLengthRecovered {
                declared: Some(5),
                actual: 11
            }]
        );
    }

    #[test]
    fn a_long_length_is_recovered_from_the_keyword() {
        let buf = b"HELLO\nendstream";
        let (range, warnings) = extent(buf, 0, Some(9999));
        assert_eq!(range, 0..5);
        assert_eq!(
            warnings,
            [WarningKind::StreamLengthRecovered {
                declared: Some(9999),
                actual: 5
            }]
        );
    }

    #[test]
    fn an_unresolved_length_is_recovered_from_the_keyword() {
        let buf = b"HELLO\r\nendstream";
        let (range, warnings) = extent(buf, 0, None);
        assert_eq!(range, 0..5);
        assert_eq!(
            warnings,
            [WarningKind::StreamLengthRecovered {
                declared: None,
                actual: 5
            }]
        );
    }

    #[test]
    fn a_missing_endstream_truncates_at_the_next_header() {
        let buf = b"HELLO\n7 0 obj (x) endobj";
        let (range, warnings) = extent(buf, 0, Some(200));
        assert_eq!(range, 0..6);
        assert_eq!(warnings, [WarningKind::StreamEndstreamMissing]);
    }

    #[test]
    fn a_missing_endstream_at_eof_takes_the_rest() {
        let buf = b"HELLO";
        let (range, warnings) = extent(buf, 0, Some(200));
        assert_eq!(range, 0..5);
        assert_eq!(warnings, [WarningKind::StreamEndstreamMissing]);
    }

    #[test]
    fn only_one_eol_is_trimmed_before_the_keyword() {
        let buf = b"A\n\nendstream";
        let (range, _) = extent(buf, 0, None);
        assert_eq!(range, 0..2);
    }

    #[test]
    fn a_start_past_the_buffer_is_an_empty_extent() {
        let buf = b"HELLO";
        let (range, _) = extent(buf, 999, Some(3));
        assert_eq!(range, 5..5);
    }
}
