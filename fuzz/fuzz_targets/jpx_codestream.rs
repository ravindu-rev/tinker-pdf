//! JPEG 2000 codestreams, generated: markers that agree with one another and
//! packets in the progression they declare, decoded to the geometry they
//! were written with.
//!
//! `jpx` hands mutated bytes to the decoder, and a mutation lands in SIZ, COD
//! or a tile-part header far more often than in a packet, where it stops at a
//! refusal before tier-2 begins. This target reads its input as choices
//! (`tinker_pdf_fuzz::Choices`) and writes a codestream with
//! `tinker_pdf_fuzz::jpx` (T.800 Annexes A to D): SIZ with one to four
//! components, subsampled or not, over a grid of up to nine tiles with image
//! and tile offsets; COD and COC with every progression order, layers,
//! decomposition levels, code-block sizes and every Table A.19 style, either
//! wavelet, user precincts, SOP and EPH, and the RCT or the ICT; QCD and QCC
//! with no quantization, a derived one or an expounded one, each fitting the
//! tile-components it serves by A.6's precedence; POC volumes in the main
//! header and in a tile's first tile-part header, the last covering what the
//! others left; tile COD, COC, QCD and QCC; tiles cut into tile-parts at
//! packet boundaries, empty ones included, interleaved with other tiles',
//! `TNsot` stated or not and the last `Psot` zero or not; packet headers in
//! the bit stream, in PPT or in PPM, coded with B.10's tag trees, pass
//! counts, `Lblock` and one length per codeword segment; and code-block data
//! that is the input's own bytes or Annex D's coding of generated
//! coefficients through every coding style, segmentation symbols included.
//! One field may be broken, on a choice the generator records: set to a
//! value the standard rules out.
//!
//! The caps — images of 40 samples a side, four components, five levels,
//! four layers, three tiles across, five bit-planes and thirteen passes a
//! code-block — are `tinker_pdf_fuzz::jpx`'s constants.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A codestream written whole decodes to what was written.** Its width,
//!   height and component count are the generated ones, no tile is short,
//!   and the samples are the size that geometry implies. A refusal of one is
//!   a disagreement between the decoder and the clauses the generator spells
//!   from — in a packet's order, a tag tree, a length, a context — and fails
//!   the run.
//! - **A broken field is named.** A codestream with one field broken decodes
//!   to the same geometry, or is refused with a reason that names the field
//!   by its marker, by T.800's name for it or by the words for what it is
//!   (`Break::names`): a refusal that says something else has refused the
//!   codestream for the wrong reason, or for none a reader can act on
//!   (ruling 10).
//!
//! # What this target cannot find, and what covers it instead
//!
//! The generator and the decoder are both written here, so a clause both
//! read the same wrong way round-trips perfectly. The arithmetic coder is
//! held apart — its decisions read back through the MQ decoder, which its own
//! tests hold to T.88's published sequence — and a context the encoder forms
//! otherwise than the decoder shows as a segmentation symbol that is not
//! `1010`; but no assertion here says whether a decoded sample is right.
//! That is `crates/tinker-pdf-filters/tests/jpx_reference.rs`'s, against
//! decodes recorded once from codestreams another encoder wrote.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{jpx_decode_attributed, Limits};
use tinker_pdf_fuzz::jpx::{describe, generate, names};
use tinker_pdf_fuzz::Choices;

/// Far above anything the caps can write: 40 by 40 samples, four
/// components, two bytes each.
const CEILING: usize = 1 << 20;

fuzz_target!(|data: &[u8]| {
    let cs = generate(&mut Choices::new(data));
    match jpx_decode_attributed(&cs.bytes, &Limits::new(CEILING)) {
        Ok(image) => {
            assert_eq!(
                (image.width, image.height, image.components),
                (cs.width, cs.height, cs.components),
                "the decode's geometry is not the codestream's"
            );
            let wide = if image.precision > 8 { 2 } else { 1 };
            assert_eq!(
                image.samples.len(),
                cs.width as usize * cs.height as usize * usize::from(cs.components) * wide,
                "the samples are not the size the geometry implies"
            );
            if cs.broken.is_none() {
                assert!(!image.truncated, "a whole codestream decoded as short");
            }
        }
        Err(refusal) => match cs.broken {
            None => panic!(
                "a codestream written whole was refused: {}",
                describe(&refusal)
            ),
            Some(broken) => assert!(
                names(broken, &refusal),
                "{broken:?} was refused as \"{}\", which names none of {:?}",
                describe(&refusal),
                broken.names()
            ),
        },
    }
});
