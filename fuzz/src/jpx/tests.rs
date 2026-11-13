use super::*;
use tinker_pdf_filters::{jpx_decode_attributed, Limits, MqContexts, MqDecoder};

/// Every decision the coder here writes, the engine's MQ decoder reads back
/// — the engine's decoder being held to T.88's published sequence by its own
/// tests — under both of D.4's terminations and from Table D.7's states.
#[test]
fn the_arithmetic_coder_reads_back_through_the_mq_decoder() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for round in 0..300 {
        let n = (rng.next() % 700) as usize;
        let bias: Vec<u32> = (0..CONTEXTS).map(|_| rng.next() % 17).collect();
        let decisions: Vec<(usize, u8)> = (0..n)
            .map(|_| {
                let cx = (rng.next() % CONTEXTS as u32) as usize;
                (cx, u8::from(rng.next() % 16 < bias[cx]))
            })
            .collect();
        for predictable in [false, true] {
            let mut states = States::initial();
            let mut mq = Mq::new();
            for &(cx, d) in &decisions {
                mq.encode(&mut states, cx, d);
            }
            let bytes = mq.finish(predictable);
            assert_ne!(bytes.last(), Some(&0xFF), "a segment ends in 0xFF");
            for pair in bytes.windows(2) {
                assert!(pair[0] != 0xFF || pair[1] < 0x90, "a marker in the data");
            }
            let mut contexts = MqContexts::new(CONTEXTS);
            for (i, &(state, mps)) in States::initial().0.iter().enumerate() {
                contexts.set_state(i, state, mps);
            }
            let mut decoder = MqDecoder::new(&bytes);
            for (k, &(cx, d)) in decisions.iter().enumerate() {
                assert_eq!(
                    decoder.decode_at(&mut contexts, cx),
                    d,
                    "round {round}, decision {k}, predictable {predictable}"
                );
            }
        }
    }
}

/// The bits a [`Bits`] holds that it has not yet packed into a byte.
fn pending(bits: &Bits) -> String {
    (0..bits.bits)
        .rev()
        .map(|i| if (bits.byte >> i) & 1 == 1 { '1' } else { '0' })
        .collect()
}

/// B.10.2's note, over Figure B.12's array: `01111` for q3(0, 0), `001` for
/// q3(1, 0) once the nodes above it are known, `101` for q3(2, 0).
#[test]
fn a_tag_tree_codes_figure_b12_as_its_note_reads() {
    let leaves = [1, 3, 2, 3, 2, 3, 2, 2, 1, 4, 3, 2, 2, 2, 2, 2, 1, 2];
    let mut tree = TagTree::new(6, 3, &leaves);
    for (x, want) in [(0, "01111"), (1, "001"), (2, "101")] {
        let mut bits = Bits::new();
        assert!(tree.encode(x, 0, u32::MAX, &mut bits));
        assert_eq!(pending(&bits), want, "q3({x}, 0)");
    }
}

/// A threshold stops the questions, and a later, higher one picks them up
/// where they stopped (B.10.4).
#[test]
fn a_tag_tree_stops_at_its_threshold_and_resumes_past_it() {
    let mut tree = TagTree::new(2, 1, &[2, 0]);
    let mut bits = Bits::new();
    assert!(
        !tree.encode(0, 0, 1, &mut bits),
        "a value of 2 is not below 1"
    );
    // The root (0) is resolved, then the leaf answers "above 0".
    assert_eq!(pending(&bits), "10");
    let mut bits = Bits::new();
    assert!(tree.encode(1, 0, 1, &mut bits));
    assert_eq!(pending(&bits), "1", "the root is known; the leaf is 0");
    let mut bits = Bits::new();
    assert!(tree.encode(0, 0, 3, &mut bits));
    assert_eq!(pending(&bits), "01", "1 is passed, 2 is reached");
}

/// The bits after a 0xFF byte are seven, so no marker can be spelled, and
/// the last byte is never 0xFF (B.10.1).
#[test]
fn packet_header_bits_are_stuffed_after_0xff() {
    let mut bits = Bits::new();
    for _ in 0..15 {
        bits.put(true);
    }
    assert_eq!(bits.finish(), vec![0xFF, 0x7F]);
    let mut bits = Bits::new();
    bits.put_value(0xFF, 8);
    assert_eq!(
        bits.finish(),
        vec![0xFF, 0x00],
        "the stuffed bit's byte is kept"
    );
}

/// What one generated codestream decodes to, held to what it was written as.
fn reads_back(input: &[u8]) -> Result<Codestream, String> {
    let cs = generate(&mut Choices::new(input));
    match jpx_decode_attributed(&cs.bytes, &Limits::new(1 << 22)) {
        Ok(image) => {
            let shape = (image.width, image.height, image.components);
            if shape != (cs.width, cs.height, cs.components) {
                return Err(format!(
                    "decoded {shape:?}, written {:?}",
                    (cs.width, cs.height, cs.components)
                ));
            }
            if cs.broken.is_none() && image.truncated {
                return Err("a whole codestream decoded as truncated".into());
            }
            Ok(cs)
        }
        Err(refusal) => match cs.broken {
            Some(broken) if names(broken, &refusal) => Ok(cs),
            Some(broken) => Err(format!("{broken:?} was refused as {refusal:?}")),
            None => Err(format!("refused: {refusal:?}")),
        },
    }
}

fn inputs(count: usize, longest: usize) -> impl Iterator<Item = Vec<u8>> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..count).map(move |n| {
        let len = (n * 131) % longest;
        (0..len)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    })
}

/// The target's property, on stable: every codestream written whole decodes
/// to its geometry, and every one with a field broken decodes to it too or
/// is refused by a name of that field.
#[test]
fn every_codestream_decodes_to_its_geometry_or_names_its_break() {
    let mut broken = 0;
    for (n, input) in inputs(600, 2400).enumerate() {
        match reads_back(&input) {
            Ok(cs) => broken += usize::from(cs.broken.is_some()),
            Err(why) => panic!("input {n} ({} bytes): {why}", input.len()),
        }
    }
    assert!(broken > 0, "no input broke a field");
}

type Feature = fn(&Codestream) -> bool;

/// Whether some tile's packets follow `order`, over a few filled packets.
fn follows(cs: &Codestream, order: Order) -> bool {
    cs.orders & (1 << order.byte()) != 0 && cs.filled >= 4
}

/// Rewrites `fuzz/corpus/jpx_codestream/`: one seed per feature, each the
/// first input of `first_input`'s sequence whose codestream has it and reads
/// back as the target asks.
#[test]
#[ignore = "writes fuzz/corpus/jpx_codestream"]
fn write_the_fuzz_seeds() {
    let features: &[(&str, Feature)] = &[
        ("lrcp", |cs| {
            follows(cs, Order::Lrcp) && cs.layers >= 2 && cs.broken.is_none()
        }),
        ("rlcp", |cs| {
            follows(cs, Order::Rlcp) && cs.layers >= 2 && cs.broken.is_none()
        }),
        ("rpcl", |cs| {
            follows(cs, Order::Rpcl) && cs.precincts && cs.broken.is_none()
        }),
        ("pcrl", |cs| {
            follows(cs, Order::Pcrl) && cs.precincts && cs.broken.is_none()
        }),
        ("cprl", |cs| {
            follows(cs, Order::Cprl) && cs.precincts && cs.broken.is_none()
        }),
        ("poc-volumes", |cs| {
            cs.volumes >= 3 && cs.filled >= 2 && cs.broken.is_none()
        }),
        ("tile-poc", |cs| {
            cs.tile_pocs >= 1 && cs.tiles >= 2 && cs.broken.is_none()
        }),
        ("tiles-interleaved", |cs| {
            cs.tiles >= 4 && cs.interleaved && cs.tnsot_zero && cs.broken.is_none()
        }),
        ("tile-overrides", |cs| {
            cs.tile_cods >= 1 && cs.overrides >= 3 && cs.broken.is_none()
        }),
        ("subsampled", |cs| {
            cs.subsampled && cs.components >= 3 && cs.filled >= 4 && cs.broken.is_none()
        }),
        ("rct", |cs| cs.rct && cs.filled >= 4 && cs.broken.is_none()),
        ("ict", |cs| cs.ict && cs.filled >= 4 && cs.broken.is_none()),
        ("sop-eph", |cs| {
            cs.sop && cs.eph && cs.filled >= 4 && cs.broken.is_none()
        }),
        ("bypass-termall", |cs| {
            cs.styles & (BYPASS | TERMALL) == BYPASS | TERMALL
                && cs.filled >= 4
                && cs.broken.is_none()
        }),
        ("bypass-raw", |cs| {
            cs.raw >= 2 && cs.encoded && cs.broken.is_none()
        }),
        ("reset-causal-predictable", |cs| {
            cs.styles & (RESET | CAUSAL | PREDICTABLE) == RESET | CAUSAL | PREDICTABLE
                && cs.encoded
                && cs.filled >= 4
                && cs.broken.is_none()
        }),
        ("segmentation-symbols", |cs| {
            cs.styles & SEGMENTATION != 0 && cs.encoded && cs.filled >= 6 && cs.broken.is_none()
        }),
        ("packed-ppm", |cs| {
            cs.headers == Headers::Ppm && cs.filled >= 3 && cs.broken.is_none()
        }),
        ("packed-ppt", |cs| {
            cs.headers == Headers::Ppt && cs.filled >= 3 && cs.broken.is_none()
        }),
        ("psot-zero", |cs| {
            cs.psot_zero && cs.parts >= 3 && cs.broken.is_none()
        }),
        ("sixteen-bit-signed", |cs| {
            cs.precision == 16 && cs.signed && cs.filled >= 3 && cs.broken.is_none()
        }),
        ("broken-siz", |cs| {
            matches!(
                cs.broken,
                Some(Break::TileGrid | Break::TileOffset | Break::Csiz)
            )
        }),
        ("broken-cod", |cs| {
            matches!(
                cs.broken,
                Some(Break::Precinct | Break::CodeBlockSize | Break::Layers)
            )
        }),
        ("broken-sot", |cs| {
            matches!(cs.broken, Some(Break::TPsot | Break::TNsot | Break::Psot))
        }),
    ];
    let read = |data: &[u8]| {
        let mut choices = Choices::new(data);
        let _ = generate(&mut choices);
        choices.consumed()
    };
    let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
    for (name, has) in features {
        let taken: Vec<Vec<u8>> = seeds.iter().map(|(_, seed)| seed.clone()).collect();
        let seed = crate::first_input(
            3000,
            200_000,
            |data| {
                let cs = generate(&mut Choices::new(data));
                has(&cs) && reads_back(data).is_ok()
            },
            read,
            &taken,
        )
        .unwrap_or_else(|| panic!("no input has {name}"));
        let cs = reads_back(&seed).unwrap_or_else(|why| panic!("{name}: {why}"));
        assert!(has(&cs), "{name} lost its feature when cut");
        seeds.push((name, seed));
    }
    crate::write_seeds("jpx_codestream", &seeds);
}
