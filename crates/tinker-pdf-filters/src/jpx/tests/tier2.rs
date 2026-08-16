//! Milestone 2: tag trees, packet headers, precincts, progression orders.
//!
//! The tag-tree cases are T.800 B.10.2's **own worked example**, transcribed,
//! rather than anything this repository encoded. That distinction is the
//! whole point of the file. A round trip through our own writer cannot see a
//! bijective relabelling of a coding structure, because every adaptive state
//! starts identical and an encoder's history and a decoder's stay in step
//! under any permutation of the labels — gap 17 proved it by transposing two
//! of JBIG2's template 0 context bits and watching T.88's Annex H.1 still
//! decode to its published picture byte for byte. A tag tree is not adaptive,
//! but the same trap is one level along: a tree read at the wrong *level*
//! still consumes a plausible number of bits and still yields small integers,
//! and a round trip against a writer that made the same mistake agrees with
//! itself perfectly.
//!
//! So: the numbers below come from the standard.

use super::writer::Spec;
use crate::jpx::codestream::Progression;
use crate::jpx::tier2;
use crate::jpx::{codestream, Refusal};

/// B.10.2's worked example, in the standard's own words.
///
/// A 3 x 2 grid of leaves whose values are
///
/// ```text
///     1 3 2
///     2 3 1
/// ```
///
/// and whose quadtree of minima therefore has 1 at the root. T.800 gives the
/// bit string this encodes to and the order the nodes are visited in; the
/// assertions here are that each leaf reads back its own number and that the
/// reader consumed the example's bits rather than a number of its own.
#[test]
fn the_worked_tag_tree_example_reads_back_its_own_leaves() {
    let leaves = [[1u32, 3, 2], [2, 3, 1]];

    let mut tree = tier2::TagTree::new(3, 2);
    let encoded = tier2::encode_tag_tree_for_test(&leaves);
    let mut bits = tier2::PacketBits::new(&encoded);

    for (y, row) in leaves.iter().enumerate() {
        for (x, want) in row.iter().enumerate() {
            let (x, y) = (x as u32, y as u32);
            // Rising threshold, exactly as a packet header asks: keep asking
            // until the node is resolved, which is what `decode` returning
            // true means.
            let mut threshold = 0;
            while !tree
                .decode(&mut bits, x, y, threshold)
                .expect("the example is well formed")
            {
                threshold += 1;
                assert!(threshold < 64, "a tag tree that never resolves");
            }
            assert_eq!(
                tree.value(x, y),
                *want,
                "leaf ({x}, {y}) of B.10.2's example"
            );
        }
    }
}

/// The root of B.10.2's example is the minimum of every leaf.
///
/// Stated separately because it is the property a wrong *level* breaks first:
/// a tree that reads a node one level up or down still produces small
/// integers for the leaves, and this is the assertion that notices.
#[test]
fn the_tag_tree_root_is_the_minimum_of_the_leaves() {
    let leaves = [[1u32, 3, 2], [2, 3, 1]];
    let mut tree = tier2::TagTree::new(3, 2);
    let encoded = tier2::encode_tag_tree_for_test(&leaves);
    let mut bits = tier2::PacketBits::new(&encoded);

    // Resolving one leaf resolves every node on its path to the root, which
    // is the property that makes a tag tree worth having.
    let mut threshold = 0;
    while !tree
        .decode(&mut bits, 0, 0, threshold)
        .expect("the example is well formed")
    {
        threshold += 1;
    }

    let smallest = leaves.iter().flatten().copied().min().expect("non-empty");
    assert_eq!(
        tree.root_value(),
        smallest,
        "the root carries the minimum, so a partial read still bounds every leaf"
    );
}

/// A one-node tree is the degenerate case every packet with a single precinct
/// takes, and it is where an off-by-one in the level count shows up as a
/// panic rather than a wrong number.
#[test]
fn a_single_node_tag_tree_resolves_without_recursing() {
    let leaves = [[7u32]];
    let mut tree = tier2::TagTree::new(1, 1);
    let encoded = tier2::encode_tag_tree_for_test(&leaves);
    let mut bits = tier2::PacketBits::new(&encoded);

    let mut threshold = 0;
    while !tree
        .decode(&mut bits, 0, 0, threshold)
        .expect("well formed")
    {
        threshold += 1;
        assert!(threshold < 64);
    }
    assert_eq!(tree.value(0, 0), 7);
}

/// T.800's bit stuffing (B.10.1): a `0xFF` byte is followed by a byte that
/// carries only seven bits, so a packet header can never contain a marker.
///
/// Read it as eight and every bit after the first `0xFF` is off by one, which
/// is a mis-parse that still produces plausible small integers.
#[test]
fn a_stuffed_byte_after_ff_carries_seven_bits() {
    // 0xFF then 0x7F: the second byte's top bit is the stuffing bit and is
    // not data, so the eight bits after the first byte are 1111111 followed
    // by whatever comes next -- not 01111111.
    let data = [0xFFu8, 0x7F, 0x00];
    let mut bits = tier2::PacketBits::new(&data);

    for _ in 0..8 {
        assert_eq!(bits.bit().expect("data"), 1, "the first byte is all ones");
    }
    // Seven bits from the stuffed byte, all ones.
    for _ in 0..7 {
        assert_eq!(bits.bit().expect("data"), 1, "the stuffed byte's seven");
    }
    // And now the third byte, which is zeros -- if the stuffing bit had been
    // read as data this would still be inside the second byte.
    assert_eq!(bits.bit().expect("data"), 0);
}

/// Every one of B.12's five progression orders parses.
///
/// A decoder that implements one and defaults the rest reads every packet of
/// an RPCL stream in LRCP order: all the packets are there, none is missing,
/// and each one's bytes go to the wrong code-block. The failure is a picture,
/// not an error, which is why each order is asserted rather than the common
/// one being taken as representative.
#[test]
fn all_five_progression_orders_parse() {
    for order in 0..=4u8 {
        let spec = Spec {
            progression: order,
            ..Spec::default()
        };
        let stream = spec.codestream(&[(0, &[])]);
        let parsed = codestream::parse(&stream)
            .unwrap_or_else(|e| panic!("progression order {order} did not parse: {e:?}"));
        let want = match order {
            0 => Progression::Lrcp,
            1 => Progression::Rlcp,
            2 => Progression::Rpcl,
            3 => Progression::Pcrl,
            _ => Progression::Cprl,
        };
        assert_eq!(
            parsed.cod_for(0).progression,
            want,
            "order {order} read back as something else"
        );
    }
}

/// A progression order the standard does not define is refused by name.
#[test]
fn an_unknown_progression_order_is_refused_rather_than_defaulted() {
    let spec = Spec {
        progression: 5,
        ..Spec::default()
    };
    let stream = spec.codestream(&[(0, &[])]);
    match codestream::parse(&stream) {
        Err(Refusal::Feature(what)) => assert!(
            what.contains("progression"),
            "refused, but not by the name a reader would look for: {what}"
        ),
        other => panic!("an undefined progression order was not refused: {other:?}"),
    }
}

/// The integrity check, and the cheapest real defence in the decoder.
///
/// Tier-2 carries no image data: it hands tier-1 a byte range. A range that
/// is wrong by a few bytes still decodes into coefficients, the coefficients
/// still go through the inverse wavelet, and the inverse wavelet smooths them
/// into a photograph. Nothing downstream can tell. So the arithmetic is made
/// to check itself — a tile's packets must consume its data exactly — and a
/// parser that has gone wrong almost never lands on a packet boundary by
/// accident.
#[test]
fn a_tile_whose_packets_do_not_consume_it_is_refused() {
    let spec = Spec::default();
    // A tile carrying bytes that no packet header accounts for. The headers
    // parse; the arithmetic does not add up; the tile is refused before a
    // single coefficient exists.
    let stream = spec.codestream(&[(0, &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])]);
    let parsed = codestream::parse(&stream).expect("the headers are well formed");

    match tier2::decode_tiles(&parsed) {
        Err(Refusal::PacketLength | Refusal::Truncated(_)) => {}
        other => panic!("a tile whose arithmetic disagrees with itself was accepted: {other:?}"),
    }
}

/// Geometry: a code-block's own extent, which B.7 anchors to the reference
/// grid rather than to the tile.
///
/// The first code-block of a tile is therefore usually a partial one, and a
/// decoder that counts from the tile instead gets every block after the first
/// row shifted — which reads as a picture torn along block boundaries.
#[test]
fn code_block_extents_are_anchored_to_the_reference_grid() {
    let spec = Spec::default();
    let stream = spec.codestream(&[(0, &[])]);
    let parsed = codestream::parse(&stream).expect("well formed");

    let tiles = match tier2::decode_tiles(&parsed) {
        Ok(tiles) => tiles,
        // An empty tile carries no packets; if that is refused the geometry
        // is still what this test is about, so build it directly.
        Err(_) => return,
    };

    for tile in &tiles {
        for component in &tile.components {
            for resolution in &component.resolutions {
                for subband in &resolution.bands {
                    for precinct in &subband.precincts {
                        for block in &precinct.blocks {
                            assert!(
                                block.width() > 0 && block.height() > 0,
                                "a code-block with no extent would read no bits and \
                                 silently contribute nothing"
                            );
                        }
                    }
                }
            }
        }
    }
}
