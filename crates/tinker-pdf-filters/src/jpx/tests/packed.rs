//! Packed packet headers (T.800 A.7.4, A.7.5): the structure, and what is
//! refused.
//!
//! **What this file is not.** Nothing here adjudicates the *decoded picture*.
//! Every codestream below is written by this repository's own test writer and
//! read back by this repository's own decoder, so an agreement between them
//! is a statement about this repository. The picture is adjudicated in
//! `tests/jpx_annex_j.rs`, which moves T.800 J.10's **published** packet
//! headers — at the boundaries J.10.3 and J.10.4 publish — into a PPM and
//! into a PPT, and asserts J.10.5's nine published samples come back.
//!
//! What this file is for is the structure around those bytes, which J.10
//! cannot reach because its codestream has one tile, one tile-part and one
//! PPM segment: several segments, a series that runs out, an index that
//! repeats, the two markers together, and every seam the clauses name.

use super::writer::{nppm_run, ppm, ppt, segment, tile_part, Spec};
use crate::jpx::codestream::{marker, parse};
use crate::jpx::{jpx_decode, Refusal};
use crate::Limits;

/// The default 4 x 4 fixture's two empty packets, one per resolution: a
/// single `0x00` header byte each, and no body at all.
///
/// B.10.3, "Zero length packet": "The first bit in the packet header denotes
/// whether the packet has a length of zero (empty packet). The value 0
/// indicates a zero length; no code-blocks are included in this case."
const TWO_EMPTY_HEADERS: [u8; 2] = [0x00, 0x00];

/// The same picture with its headers where B.10 puts them by default.
fn in_bit_stream() -> Vec<u8> {
    Spec::default().codestream(&[(0, &TWO_EMPTY_HEADERS)])
}

/// The same picture with its headers in a PPM segment (A.7.4).
fn with_ppm(tail: &[u8]) -> Vec<u8> {
    let spec = Spec::default();
    let mut out = spec.main_header();
    out.extend_from_slice(&ppm(0, tail));
    out.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    out.extend_from_slice(&marker::EOC.to_be_bytes());
    out
}

/// The same picture with its headers in a PPT segment (A.7.5).
fn with_ppt(ippt: &[u8], data: &[u8]) -> Vec<u8> {
    let spec = Spec::default();
    let mut out = spec.main_header();
    out.extend_from_slice(&tile_part(0, 0, 1, &ppt(0, ippt), data));
    out.extend_from_slice(&marker::EOC.to_be_bytes());
    out
}

fn decode(bytes: &[u8]) -> Result<crate::JpxImage, crate::FilterError> {
    let mut warnings = Vec::new();
    jpx_decode(bytes, &Limits::new(1 << 20), &mut warnings)
}

/// B.10's three places for a packet header, over one picture.
///
/// Self-consistency, and labelled as such: all three codestreams are this
/// repository's. What it pins is that the *relocation* changes nothing
/// downstream — the geometry, the tag trees and the code-block lengths all
/// come out the same — which is the claim B.10 makes and the one a second
/// implementation of the packet-header reader would have broken.
#[test]
fn the_three_places_b10_allows_a_header_agree_on_this_writer_s_picture() {
    let plain = decode(&in_bit_stream()).expect("headers in the bit stream");
    let ppm = decode(&with_ppm(&nppm_run(&TWO_EMPTY_HEADERS))).expect("headers in a PPM");
    let ppt = decode(&with_ppt(&TWO_EMPTY_HEADERS, &[])).expect("headers in a PPT");
    assert_eq!(plain, ppm);
    assert_eq!(plain, ppt);
}

/// A.7.4: the `Nppm` series has "one value for each tile-part (not tile)".
///
/// Two tile-parts, two runs. The first carries the first packet's header and
/// the first tile-part carries no body; the second carries the second's.
#[test]
fn the_nppm_series_has_one_entry_per_tile_part() {
    let spec = Spec::default();
    let mut tail = nppm_run(&TWO_EMPTY_HEADERS[..1]);
    tail.extend_from_slice(&nppm_run(&TWO_EMPTY_HEADERS[1..]));
    let mut out = spec.main_header();
    out.extend_from_slice(&ppm(0, &tail));
    out.extend_from_slice(&tile_part(0, 0, 2, &[], &[]));
    out.extend_from_slice(&tile_part(0, 1, 2, &[], &[]));
    out.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        decode(&out).expect("two tile-parts, two Nppm runs"),
        decode(&in_bit_stream()).expect("the same picture")
    );
}

/// A series with the wrong number of entries is refused rather than
/// re-associated.
///
/// The kth entry is the kth tile-part's. A series one short leaves the last
/// tile-part reading another's headers, and a series one long describes a
/// tile-part that never arrived — in both cases every packet after the slip
/// reads a header that belongs to some other packet, which is the mis-parse
/// this whole decoder is written against.
#[test]
fn an_nppm_series_that_does_not_count_the_tile_parts_is_refused() {
    // One run, two tile-parts.
    let spec = Spec::default();
    let mut short = spec.main_header();
    short.extend_from_slice(&ppm(0, &nppm_run(&TWO_EMPTY_HEADERS)));
    short.extend_from_slice(&tile_part(0, 0, 2, &[], &[]));
    short.extend_from_slice(&tile_part(0, 1, 2, &[], &[]));
    short.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(matches!(parse(&short), Err(Refusal::Structure(_))));

    // Two runs, one tile-part.
    let mut tail = nppm_run(&TWO_EMPTY_HEADERS[..1]);
    tail.extend_from_slice(&nppm_run(&TWO_EMPTY_HEADERS[1..]));
    assert!(matches!(
        parse(&with_ppm(&tail)),
        Err(Refusal::Structure(_))
    ));
}

/// A.7.4: an `Nppm` that reaches past the joined stream is a truncation.
///
/// Nothing is allocated on it. It is a 32-bit count of bytes that are
/// already in hand, so the largest one T.800 permits costs a comparison.
#[test]
fn an_nppm_past_the_end_of_the_packed_stream_is_refused() {
    let mut tail = 0xFFFF_FFFFu32.to_be_bytes().to_vec();
    tail.extend_from_slice(&TWO_EMPTY_HEADERS);
    assert!(matches!(
        parse(&with_ppm(&tail)),
        Err(Refusal::Truncated(_))
    ));
}

/// A.7.4: "the series of Ippm parameters described by the Nppm does not have
/// to be complete in a given marker segment."
///
/// The run below is declared in one segment and finished in the next, which
/// is the case that makes joining-before-parsing necessary: a parser that
/// read each segment on its own would take the second segment's first four
/// bytes for a length.
#[test]
fn an_nppm_run_may_finish_in_a_later_segment() {
    // Four layers of two resolutions: eight empty packets, so the run is
    // long enough to be cut and still leave both segments at Table A.38's
    // minimum Lppm of 7.
    let spec = Spec {
        layers: 4,
        ..Spec::default()
    };
    let headers = [0u8; 8];
    let plain = spec.codestream(&[(0, &headers)]);

    let mut tail = nppm_run(&headers);
    let (first, rest) = tail.split_at_mut(8);
    let mut out = spec.main_header();
    out.extend_from_slice(&ppm(0, first));
    out.extend_from_slice(&ppm(1, rest));
    out.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    out.extend_from_slice(&marker::EOC.to_be_bytes());

    assert_eq!(
        decode(&out).expect("a run split across two PPM segments"),
        decode(&plain).expect("the same picture, headers in the bit stream")
    );
}

/// A.7.4 and A.7.5: two segments claiming one index are refused.
///
/// The join is ordered by `Zppm` and by nothing else, so a repeat leaves it
/// with no order at all — and picking either one silently hands every packet
/// after it the wrong header.
#[test]
fn a_repeated_packed_header_index_is_refused() {
    let spec = Spec::default();
    let mut two_zppm = spec.main_header();
    two_zppm.extend_from_slice(&ppm(0, &nppm_run(&TWO_EMPTY_HEADERS[..1])));
    two_zppm.extend_from_slice(&ppm(0, &nppm_run(&TWO_EMPTY_HEADERS[1..])));
    two_zppm.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    two_zppm.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(matches!(parse(&two_zppm), Err(Refusal::Structure(_))));

    let mut header = ppt(0, &TWO_EMPTY_HEADERS[..1]);
    header.extend_from_slice(&ppt(0, &TWO_EMPTY_HEADERS[1..]));
    let mut two_zppt = spec.main_header();
    two_zppt.extend_from_slice(&tile_part(0, 0, 1, &header, &[]));
    two_zppt.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(matches!(parse(&two_zppt), Err(Refusal::Structure(_))));
}

/// Table A.2, note c: "If the PPM marker segment is used then PPT marker
/// segments shall not be used, and vice versa."
///
/// A.7.4 says what is at stake: with PPM present "all the packet headers
/// shall be found in the main header", so a PPT alongside it is a second,
/// contradictory account of where this tile's headers are.
#[test]
fn a_codestream_carrying_both_ppm_and_ppt_is_refused() {
    let spec = Spec::default();
    let mut out = spec.main_header();
    out.extend_from_slice(&ppm(0, &nppm_run(&TWO_EMPTY_HEADERS)));
    out.extend_from_slice(&tile_part(0, 0, 1, &ppt(0, &TWO_EMPTY_HEADERS), &[]));
    out.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(matches!(parse(&out), Err(Refusal::Structure(_))));
}

/// Tables A.38 and A.39: `Lppm` is 7 to 65 535 and `Lppt` is 4 to 65 535.
///
/// The minima are not decoration — 7 is the shortest segment that can carry
/// a `Zppm` and one 32-bit `Nppm`, and 4 the shortest that can carry a
/// `Zppt` and one byte of header.
#[test]
fn a_packed_segment_below_its_table_s_minimum_length_is_refused() {
    let spec = Spec::default();
    let mut short_ppm = spec.main_header();
    short_ppm.extend_from_slice(&segment(marker::PPM, &[0, 0, 0, 0]));
    short_ppm.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    short_ppm.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(matches!(parse(&short_ppm), Err(Refusal::Structure(_))));

    let mut short_ppt = spec.main_header();
    short_ppt.extend_from_slice(&tile_part(0, 0, 1, &segment(marker::PPT, &[0]), &[]));
    short_ppt.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(matches!(parse(&short_ppt), Err(Refusal::Structure(_))));
}

/// The packed stream is consumed exactly, or the tile is refused.
///
/// With the headers moved out, the bit stream's own exact-consumption check
/// only covers the bodies. A packed stream with a byte left over is a tile
/// whose packets and whose headers disagree about how many there are, and it
/// is refused for the same reason a long bit stream is: tier-2 carries no
/// image data, so the disagreement would otherwise become a picture.
#[test]
fn a_packed_stream_the_packets_do_not_consume_is_refused() {
    let mut extra = TWO_EMPTY_HEADERS.to_vec();
    extra.push(0x00);
    assert_eq!(
        decode(&with_ppm(&nppm_run(&extra))),
        Err(crate::FilterError::Unsupported(crate::Capability::Jpx))
    );
    assert_eq!(
        decode(&with_ppt(&extra, &[])),
        Err(crate::FilterError::Unsupported(crate::Capability::Jpx))
    );
    // And one short.
    assert!(decode(&with_ppt(&TWO_EMPTY_HEADERS[..1], &[])).is_err());
}

// A.7.4 and A.7.5's two remaining clauses — "Every marker segment in this
// series shall end with a completed packet header" and "concatenated, in
// the order of increasing Zppt" — are checked in `tests/jpx_annex_j.rs`
// rather than here, and deliberately. This module's packets are all empty,
// so every packet header in it is one byte: there is no interior offset for
// a segment to end at wrongly, and two segments of one zero byte each are
// the same bytes in either order. Both tests would have passed against a
// decoder that ignored the seam and the index entirely. J.10's published
// headers are three bytes and four, so there both clauses have something to
// fail on — see `a_packed_segment_that_ends_inside_a_published_packet_header_is_refused`
// and `a_ppm_series_joins_by_zppm_across_segments_before_it_is_read`.

/// A.8.2: EPH moves with the header it delimits.
///
/// "If the packet headers are moved to a PPM or PPT marker segments (see
/// A.7.4 and A.7.5), then the EPH markers shall appear after the packet
/// headers in the PPM or PPT marker segments", and, in the same clause, "If
/// packet headers are not in-bit stream (i.e., PPM or PPT marker segments
/// are used), this marker shall not be used in the bit stream."
#[test]
fn eph_follows_the_header_into_the_packed_stream() {
    let spec = Spec {
        // Scod bit 2: EPH used.
        scod: 0x04,
        ..Spec::default()
    };
    let packed = [0x00, 0xFF, 0x92, 0x00, 0xFF, 0x92];
    let mut ok = spec.main_header();
    ok.extend_from_slice(&tile_part(0, 0, 1, &ppt(0, &packed), &[]));
    ok.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(decode(&ok).is_ok(), "EPH inside the PPT, as A.8.2 requires");

    // The same headers with the EPH markers left in the bit stream, which
    // A.8.2 forbids, and which would be a decoder reading two streams'
    // worth of delimiters out of one.
    let mut wrong = spec.main_header();
    wrong.extend_from_slice(&tile_part(
        0,
        0,
        1,
        &ppt(0, &TWO_EMPTY_HEADERS),
        &[0xFF, 0x92, 0xFF, 0x92],
    ));
    wrong.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(decode(&wrong).is_err(), "EPH in the bit stream is refused");
}

/// A.7.4: the choice is per tile, and A.7.5's arm of it is the one that can
/// vary between tiles.
///
/// "The packet headers shall not be in both a PPT marker segment and the
/// codestream for the same tile" — *for the same tile*. One tile packed and
/// its neighbour not is a conforming codestream, and refusing it would
/// refuse a file the standard allows.
#[test]
fn one_tile_may_use_ppt_while_another_keeps_its_headers_in_the_bit_stream() {
    let spec = Spec {
        xsiz: 8,
        ysiz: 4,
        xtsiz: 4,
        ytsiz: 4,
        ..Spec::default()
    };
    let mut out = spec.main_header();
    out.extend_from_slice(&tile_part(0, 0, 1, &ppt(0, &TWO_EMPTY_HEADERS), &[]));
    out.extend_from_slice(&tile_part(1, 0, 1, &[], &TWO_EMPTY_HEADERS));
    out.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(decode(&out).is_ok());
}
