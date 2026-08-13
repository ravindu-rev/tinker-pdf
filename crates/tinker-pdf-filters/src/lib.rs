//! Hand-rolled PDF stream filters. Bytes in, bytes out; no PDF types.
//!
//! Scope, design and exit criteria: `docs/plans/02-filters.md`.
//!
//! Wave 1 is the byte-filter set: Flate (RFC 1950/1951), LZW (7.4.4),
//! ASCIIHex (7.4.2), ASCII85 (7.4.3), RunLength (7.4.5) and the predictors of
//! Table 10. The image codecs (DCTDecode, CCITTFaxDecode) land in wave 2 and
//! are terminal in a filter chain until then: [`apply_chain`] hands their
//! still-encoded bytes back to the caller.
//!
//! The output contract, stated once: **corrupt input truncates and warns; it
//! never errors.** [`Decoded::complete`] is false whenever the input ended
//! early, was damaged, or hit [`Limits::max_output`], and every tolerated
//! condition leaves a typed [`Warning`]. [`FilterError`] is reserved for the
//! caller holding the API wrong ([`FilterError::BadParams`]) and for deferred
//! capabilities ([`FilterError::Unsupported`]) — decisions, not data.

#![forbid(unsafe_code)]

mod ascii;
mod ccitt;
pub mod deflate;
mod inflate;
mod jpeg;
mod lzw;
mod predictors;
mod runlength;

use core::fmt;

pub use ccitt::{decode as ccitt_decode, CcittParams, T6Rows};
pub use deflate::{deflate, zlib_compress};
pub use jpeg::{decode as jpeg_decode, JpegColor, JpegError, JpegImage};
pub use predictors::PredictorParams;

/// Resource ceilings. Mandatory: a 1 KB flate stream can legally expand to
/// gigabytes, and a lenient decoder without a ceiling is a denial-of-service
/// primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Hard ceiling on produced bytes. Hitting it truncates and warns.
    pub max_output: usize,
}

impl Limits {
    #[must_use]
    pub const fn new(max_output: usize) -> Self {
        Self { max_output }
    }
}

/// The result of one decode: what came out, whether it came out whole, and
/// what had to be tolerated to get it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub data: Vec<u8>,
    /// false: input ended early, was damaged, or hit `max_output`.
    pub complete: bool,
    /// Typed leniency records (ruling 10). This crate reports *what* it
    /// tolerated; the caller attaches *which object* it happened in.
    pub warnings: Vec<Warning>,
}

/// The two conditions that are decisions rather than data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterError {
    /// Parameters that cannot describe any stream (zero `/Columns`, a
    /// `/BitsPerComponent` outside 1/2/4/8/16, an unknown `/Predictor`).
    BadParams(&'static str),
    /// A codec deferred behind a capability gate (ruling 3).
    Unsupported(Capability),
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadParams(m) => write!(f, "bad filter parameters: {m}"),
            Self::Unsupported(c) => write!(f, "unsupported codec: {c:?}"),
        }
    }
}

impl std::error::Error for FilterError {}

/// Codecs that exist behind a capability gate rather than in the crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    Jbig2,
    Jpx,
    JpegArithmetic,
    Jpeg12Bit,
}

/// Every leniency this crate performs, as data rather than a log line.
///
/// The set is closed and each variant is recorded at most once per decode:
/// a stream of a million bad bytes yields one `NonHexByte`, not a million.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Warning {
    /// The zlib header was absent or invalid; the same bytes were decoded as
    /// raw deflate (RFC 1951 with no wrapper).
    RawDeflateFallback,
    /// FDICT was set. Preset dictionaries do not occur in PDFs; the
    /// dictionary id was skipped and decoding attempted anyway.
    FdictIgnored,
    /// The zlib Adler-32 did not match the decoded bytes.
    AdlerMismatch,
    /// Input ended in the middle of a code, block or run.
    TruncatedInput,
    /// The stream ended before its EOD marker (`>`, `~>`, `128`, code 257 or
    /// a final deflate block) appeared.
    EarlyEod,
    /// Bytes remained after the stream's end; they were ignored.
    TrailingGarbage,
    /// `Limits::max_output` was reached; the output is truncated there.
    OutputCapHit,
    /// A deflate block was structurally invalid (over-subscribed Huffman
    /// table, reserved block type, distance past the window); decoding
    /// stopped at that point.
    CorruptDeflateStream,
    /// ASCIIHex: an odd number of digits; a trailing `0` was implied.
    OddHexDigit,
    /// ASCIIHex: a byte that is neither a hex digit nor white space was
    /// skipped.
    NonHexByte,
    /// ASCII85: a byte outside `!`..=`u` (`y` included — 7.4.3 has no `y`
    /// shortcut) was skipped.
    BadAscii85Byte,
    /// ASCII85: `z` appeared inside a group rather than at its start; it was
    /// skipped.
    Ascii85ZInGroup,
    /// ASCII85: a group decoded above 2^32-1; it was clamped.
    Ascii85Overflow,
    /// LZW: a code outside the current table; decoding stopped there.
    BadLzwCode,
    /// LZW: the table filled without a Clear code; it was frozen and
    /// decoding continued.
    LzwTableFull,
    /// Predictor: the last row was short; the bytes present were unfiltered.
    PredictorRowShort,
    /// Predictor: a PNG row tag above 4; the row was taken unfiltered.
    BadPredictorTag,
    /// A chain entry followed an image codec; the tail was ignored.
    ChainTailIgnored,
    /// CCITT: `/EndOfLine true` says every line is preceded by an EOL and a
    /// line was not; it was decoded from the cursor anyway.
    MissingEndOfLine,
}

impl Warning {
    /// A short stable identifier, suitable for a debug dump or a test name.
    /// Callers that re-export these as their own typed warnings need a name
    /// that does not change when the prose does.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RawDeflateFallback => "raw-deflate-fallback",
            Self::FdictIgnored => "fdict-ignored",
            Self::AdlerMismatch => "adler-mismatch",
            Self::TruncatedInput => "truncated-input",
            Self::EarlyEod => "early-eod",
            Self::TrailingGarbage => "trailing-garbage",
            Self::OutputCapHit => "output-cap-hit",
            Self::CorruptDeflateStream => "corrupt-deflate-stream",
            Self::OddHexDigit => "odd-hex-digit",
            Self::NonHexByte => "non-hex-byte",
            Self::BadAscii85Byte => "bad-ascii85-byte",
            Self::Ascii85ZInGroup => "ascii85-z-in-group",
            Self::Ascii85Overflow => "ascii85-overflow",
            Self::BadLzwCode => "bad-lzw-code",
            Self::LzwTableFull => "lzw-table-full",
            Self::PredictorRowShort => "predictor-row-short",
            Self::BadPredictorTag => "bad-predictor-tag",
            Self::ChainTailIgnored => "chain-tail-ignored",
            Self::MissingEndOfLine => "missing-end-of-line",
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::RawDeflateFallback => "zlib header invalid, decoded as raw deflate",
            Self::FdictIgnored => "zlib preset dictionary ignored",
            Self::AdlerMismatch => "zlib Adler-32 mismatch",
            Self::TruncatedInput => "input ended mid-symbol",
            Self::EarlyEod => "stream ended before its EOD marker",
            Self::TrailingGarbage => "bytes after end of stream ignored",
            Self::OutputCapHit => "output limit reached, decode truncated",
            Self::CorruptDeflateStream => "corrupt deflate block",
            Self::OddHexDigit => "odd hex digit count, trailing zero implied",
            Self::NonHexByte => "non-hex byte skipped",
            Self::BadAscii85Byte => "invalid ASCII85 byte skipped",
            Self::Ascii85ZInGroup => "ASCII85 'z' inside a group skipped",
            Self::Ascii85Overflow => "ASCII85 group above 2^32-1 clamped",
            Self::BadLzwCode => "LZW code out of range",
            Self::LzwTableFull => "LZW table full without a clear code",
            Self::PredictorRowShort => "short predictor row",
            Self::BadPredictorTag => "unknown PNG predictor tag",
            Self::ChainTailIgnored => "filters after an image codec ignored",
            Self::MissingEndOfLine => "expected end-of-line code absent",
        };
        f.write_str(s)
    }
}

/// Deduplicating warning sink. Keeps first-occurrence order and bounds the
/// vector by the size of the `Warning` set, so a pathological stream cannot
/// turn leniency into an allocation attack.
#[derive(Clone, Debug, Default)]
pub(crate) struct Warnings {
    list: Vec<Warning>,
}

impl Warnings {
    pub(crate) fn push(&mut self, w: Warning) {
        if !self.list.contains(&w) {
            self.list.push(w);
        }
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        for w in &other.list {
            self.push(*w);
        }
    }

    pub(crate) fn into_vec(self) -> Vec<Warning> {
        self.list
    }
}

/// One filter of a `/Filter` chain. Image codecs are members so a chain can
/// name them, but they terminate the chain — see [`ChainOutput`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    Flate,
    Lzw,
    AsciiHex,
    Ascii85,
    RunLength,
    Dct,
    Ccitt,
    Jbig2,
    Jpx,
}

impl Filter {
    /// `Some` when this filter produces an image rather than bytes.
    #[must_use]
    pub const fn image_codec(self) -> Option<ImageCodec> {
        match self {
            Self::Dct => Some(ImageCodec::Dct),
            Self::Ccitt => Some(ImageCodec::Ccitt),
            Self::Jbig2 => Some(ImageCodec::Jbig2),
            Self::Jpx => Some(ImageCodec::Jpx),
            _ => None,
        }
    }
}

/// The filters whose output is an image, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageCodec {
    Dct,
    Ccitt,
    Jbig2,
    Jpx,
}

impl ImageCodec {
    /// The capability gate this codec sits behind, if it has one. `None`
    /// means the codec is scheduled to be decoded in-crate (wave 2); `Some`
    /// means the caller substitutes the neutral placeholder (ruling 2).
    #[must_use]
    pub const fn capability(self) -> Option<Capability> {
        match self {
            Self::Jbig2 => Some(Capability::Jbig2),
            Self::Jpx => Some(Capability::Jpx),
            Self::Dct | Self::Ccitt => None,
        }
    }
}

/// A chain entry: which filter, plus the parameters COS decoded for it.
///
/// Parameters that do not apply to the named filter are ignored, so a caller
/// can forward a `/DecodeParms` entry without first deciding whether it is
/// meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterSpec {
    pub filter: Filter,
    /// Flate and LZW only.
    pub predictor: Option<PredictorParams>,
    /// LZW only. `/EarlyChange`, whose PDF default is 1 (true).
    pub early_change: bool,
}

impl FilterSpec {
    /// A filter with PDF-default parameters.
    #[must_use]
    pub const fn new(filter: Filter) -> Self {
        Self {
            filter,
            predictor: None,
            early_change: true,
        }
    }

    #[must_use]
    pub const fn with_predictor(mut self, p: PredictorParams) -> Self {
        self.predictor = Some(p);
        self
    }

    #[must_use]
    pub const fn with_early_change(mut self, early_change: bool) -> Self {
        self.early_change = early_change;
        self
    }
}

/// What a chain produced: bytes, or the still-encoded payload of an image
/// codec that terminated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainOutput {
    Bytes(Decoded),
    EncodedImage {
        kind: ImageCodec,
        /// The bytes as the codec wants them: every byte filter ahead of the
        /// codec has been applied, the codec itself has not.
        data: Vec<u8>,
        warnings: Vec<Warning>,
    },
}

/// FlateDecode (7.4.4): zlib or raw deflate, then the predictor if one was
/// given.
///
/// # Errors
/// [`FilterError::BadParams`] if the predictor parameters cannot describe a
/// row. Damaged *data* never errors.
pub fn flate_decode(
    input: &[u8],
    limits: &Limits,
    predictor: Option<&PredictorParams>,
) -> Result<Decoded, FilterError> {
    let mut w = Warnings::default();
    let (data, complete) = inflate::flate_bytes(input, limits, &mut w);
    finish(data, complete, predictor, limits, w)
}

/// LZWDecode (7.4.4), then the predictor if one was given.
///
/// `early_change` is `/EarlyChange`, whose default is 1 (`true`).
///
/// # Errors
/// [`FilterError::BadParams`] if the predictor parameters cannot describe a
/// row. Damaged *data* never errors.
pub fn lzw_decode(
    input: &[u8],
    limits: &Limits,
    early_change: bool,
    predictor: Option<&PredictorParams>,
) -> Result<Decoded, FilterError> {
    let mut w = Warnings::default();
    let (data, complete) = lzw::lzw_bytes(input, limits, early_change, &mut w);
    finish(data, complete, predictor, limits, w)
}

/// ASCIIHexDecode (7.4.2).
#[must_use]
pub fn ascii_hex_decode(input: &[u8], limits: &Limits) -> Decoded {
    let mut w = Warnings::default();
    let (data, complete) = ascii::hex_bytes(input, limits, &mut w);
    Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    }
}

/// ASCII85Decode (7.4.3).
#[must_use]
pub fn ascii85_decode(input: &[u8], limits: &Limits) -> Decoded {
    let mut w = Warnings::default();
    let (data, complete) = ascii::a85_bytes(input, limits, &mut w);
    Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    }
}

/// RunLengthDecode (7.4.5).
#[must_use]
pub fn run_length_decode(input: &[u8], limits: &Limits) -> Decoded {
    let mut w = Warnings::default();
    let (data, complete) = runlength::rle_bytes(input, limits, &mut w);
    Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    }
}

/// The Table 10 predictors, applied on their own. Flate and LZW streams
/// normally reach this through their own entry points.
///
/// # Errors
/// [`FilterError::BadParams`] if the parameters cannot describe a row.
pub fn predictor_decode(
    input: &[u8],
    params: &PredictorParams,
    limits: &Limits,
) -> Result<Decoded, FilterError> {
    let mut w = Warnings::default();
    let (data, complete) = predictors::unpredict(input, params, limits, &mut w)?;
    Ok(Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    })
}

/// Runs a `/Filter` chain in order.
///
/// An image codec terminates the chain: its input bytes come back tagged
/// with [`ImageCodec`] for the caller to hand to a codec entry point. A
/// filter named after an image codec is nonsense, so the tail is dropped
/// with [`Warning::ChainTailIgnored`].
///
/// # Errors
/// [`FilterError::BadParams`] if a stage's predictor parameters cannot
/// describe a row.
pub fn apply_chain(
    input: &[u8],
    chain: &[FilterSpec],
    limits: &Limits,
) -> Result<ChainOutput, FilterError> {
    let mut w = Warnings::default();
    let mut complete = true;
    let mut carried: Option<Vec<u8>> = None;

    for (i, spec) in chain.iter().enumerate() {
        let cur: &[u8] = match carried.as_deref() {
            Some(d) => d,
            None => input,
        };

        if let Some(kind) = spec.filter.image_codec() {
            if i + 1 < chain.len() {
                w.push(Warning::ChainTailIgnored);
            }
            let (data, _) = cap_slice(cur, limits, &mut w);
            return Ok(ChainOutput::EncodedImage {
                kind,
                data,
                warnings: w.into_vec(),
            });
        }

        let (data, ok) = match spec.filter {
            Filter::Flate => {
                let (d, ok) = inflate::flate_bytes(cur, limits, &mut w);
                stage_predictor(d, ok, spec.predictor.as_ref(), limits, &mut w)?
            }
            Filter::Lzw => {
                let (d, ok) = lzw::lzw_bytes(cur, limits, spec.early_change, &mut w);
                stage_predictor(d, ok, spec.predictor.as_ref(), limits, &mut w)?
            }
            Filter::AsciiHex => ascii::hex_bytes(cur, limits, &mut w),
            Filter::Ascii85 => ascii::a85_bytes(cur, limits, &mut w),
            Filter::RunLength => runlength::rle_bytes(cur, limits, &mut w),
            // Image codecs returned above.
            Filter::Dct | Filter::Ccitt | Filter::Jbig2 | Filter::Jpx => (cur.to_vec(), true),
        };

        complete &= ok;
        carried = Some(data);
    }

    let data = match carried {
        Some(d) => d,
        // An empty chain is the identity, and the identity is still bounded.
        None => {
            let (d, ok) = cap_slice(input, limits, &mut w);
            complete &= ok;
            d
        }
    };
    Ok(ChainOutput::Bytes(Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    }))
}

fn cap_slice(data: &[u8], limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    match data.get(..limits.max_output) {
        Some(head) if head.len() < data.len() => {
            w.push(Warning::OutputCapHit);
            (head.to_vec(), false)
        }
        _ => (data.to_vec(), true),
    }
}

fn stage_predictor(
    data: Vec<u8>,
    complete: bool,
    predictor: Option<&PredictorParams>,
    limits: &Limits,
    w: &mut Warnings,
) -> Result<(Vec<u8>, bool), FilterError> {
    match predictor {
        Some(p) if p.is_active() => {
            let (out, ok) = predictors::unpredict(&data, p, limits, w)?;
            Ok((out, complete && ok))
        }
        _ => Ok((data, complete)),
    }
}

fn finish(
    data: Vec<u8>,
    complete: bool,
    predictor: Option<&PredictorParams>,
    limits: &Limits,
    mut w: Warnings,
) -> Result<Decoded, FilterError> {
    let (data, complete) = stage_predictor(data, complete, predictor, limits, &mut w)?;
    Ok(Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_dedup_and_keep_order() {
        let mut w = Warnings::default();
        w.push(Warning::EarlyEod);
        w.push(Warning::NonHexByte);
        w.push(Warning::EarlyEod);
        assert_eq!(w.into_vec(), vec![Warning::EarlyEod, Warning::NonHexByte]);
    }

    #[test]
    fn warning_merge_dedups_across_sinks() {
        let mut a = Warnings::default();
        a.push(Warning::OutputCapHit);
        let mut b = Warnings::default();
        b.push(Warning::OutputCapHit);
        b.push(Warning::TruncatedInput);
        a.merge(&b);
        assert_eq!(
            a.into_vec(),
            vec![Warning::OutputCapHit, Warning::TruncatedInput]
        );
    }

    #[test]
    fn empty_chain_is_identity() {
        let out = apply_chain(b"abc", &[], &Limits::new(64)).expect("no params to reject");
        match out {
            ChainOutput::Bytes(d) => {
                assert_eq!(d.data, b"abc");
                assert!(d.complete);
                assert!(d.warnings.is_empty());
            }
            ChainOutput::EncodedImage { .. } => panic!("no codec in chain"),
        }
    }

    #[test]
    fn image_codec_terminates_chain() {
        // ASCIIHex then DCT: the hex is decoded, the JPEG bytes come back raw.
        let chain = [
            FilterSpec::new(Filter::AsciiHex),
            FilterSpec::new(Filter::Dct),
        ];
        let out = apply_chain(b"FFD8FFE0>", &chain, &Limits::new(64)).expect("params fine");
        match out {
            ChainOutput::EncodedImage {
                kind,
                data,
                warnings,
            } => {
                assert_eq!(kind, ImageCodec::Dct);
                assert_eq!(data, vec![0xFF, 0xD8, 0xFF, 0xE0]);
                assert!(warnings.is_empty());
            }
            ChainOutput::Bytes(_) => panic!("codec should terminate"),
        }
    }

    #[test]
    fn chain_tail_after_codec_is_dropped_with_a_warning() {
        let chain = [
            FilterSpec::new(Filter::Jpx),
            FilterSpec::new(Filter::AsciiHex),
        ];
        let out = apply_chain(b"\x00\x01", &chain, &Limits::new(64)).expect("params fine");
        match out {
            ChainOutput::EncodedImage {
                kind,
                data,
                warnings,
            } => {
                assert_eq!(kind, ImageCodec::Jpx);
                assert_eq!(data, vec![0, 1]);
                assert_eq!(warnings, vec![Warning::ChainTailIgnored]);
            }
            ChainOutput::Bytes(_) => panic!("codec should terminate"),
        }
    }

    #[test]
    fn capability_gates_name_the_deferred_codecs() {
        assert_eq!(ImageCodec::Jbig2.capability(), Some(Capability::Jbig2));
        assert_eq!(ImageCodec::Jpx.capability(), Some(Capability::Jpx));
        assert_eq!(ImageCodec::Dct.capability(), None);
        assert_eq!(ImageCodec::Ccitt.capability(), None);
        assert_eq!(Filter::Flate.image_codec(), None);
        assert_eq!(Filter::Ccitt.image_codec(), Some(ImageCodec::Ccitt));
    }

    #[test]
    fn filter_error_displays() {
        assert_eq!(
            FilterError::BadParams("bad").to_string(),
            "bad filter parameters: bad"
        );
        assert_eq!(
            FilterError::Unsupported(Capability::Jpeg12Bit).to_string(),
            "unsupported codec: Jpeg12Bit"
        );
        assert_eq!(
            Warning::EarlyEod.to_string(),
            "stream ended before its EOD marker"
        );
    }
}
