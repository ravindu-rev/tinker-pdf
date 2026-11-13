//! Code-blocks on a pool (P-10.10): a decode that hands tier-1's blocks to an
//! executor is the serial decode at every thread count — the same samples,
//! the same counts, the same warnings and, where it refuses, the same refusal
//! of the same block with the same budget left
//! (`docs/design/jpx-speed.md`, "Code-blocks and synthesis on P-15's pool").
//!
//! The executor here is the test's own, scoped threads with a fixed share
//! each ([`Spread`]), so the decoder is driven through the seam a caller
//! fills (`tinker_pdf_pool::Executor`) and never through the facade's pool,
//! and which thread ran which block is a fact the test can count.

use std::sync::Mutex;
use std::thread::{self, ThreadId};

use super::writer::{coded, At, Spec};
use crate::jpx::codestream::parse;
use crate::jpx::tier2::{self, Orientation, Tile};
use crate::jpx::{jpx_decode_on, jpx_decode_request, tier1, JpxRequest, JpxWindow, Refusal};
use crate::{Limits, Warning};
use tinker_pdf_pool::Executor;

/// The test's own executor: `threads` scoped threads, thread `k` running
/// jobs `k`, `k + threads`, … in order, each job recorded with the thread
/// that ran it.
struct Spread {
    threads: usize,
    ran: Mutex<Vec<(usize, ThreadId)>>,
}

impl Spread {
    fn new(threads: usize) -> Spread {
        Spread {
            threads,
            ran: Mutex::new(Vec::new()),
        }
    }
}

impl Executor for Spread {
    fn run(&self, len: usize, job: &(dyn Fn(usize) + Sync)) {
        thread::scope(|scope| {
            for k in 0..self.threads {
                scope.spawn(move || {
                    let mut i = k;
                    while i < len {
                        job(i);
                        if let Ok(mut ran) = self.ran.lock() {
                            ran.push((i, thread::current().id()));
                        }
                        i += self.threads;
                    }
                });
            }
        });
    }
}

fn limits() -> Limits {
    Limits::new(1 << 24)
}

/// A coefficient for every position from a seeded hash of where it sits,
/// image-scale in the lowest band and small elsewhere, as `reduced.rs`'s.
fn noise(at: At) -> i32 {
    let band = match at.orientation {
        Orientation::Ll => 0u64,
        Orientation::Hl => 1,
        Orientation::Lh => 2,
        Orientation::Hh => 3,
    };
    let mut h = (at.component as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((at.resolution as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(band.wrapping_mul(0x94D0_49BB_1331_11EB))
        .wrapping_add(u64::from(at.x).wrapping_mul(0x2545_F491_4F6C_DD1D))
        .wrapping_add(u64::from(at.y).wrapping_mul(0x6A09_E667_F3BC_C909));
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    if band == 0 {
        return (h % 201) as i32 - 100;
    }
    (h % 61) as i32 - 30
}

/// Many tiles of many code-blocks: a 40 by 34 image at an odd origin in 16
/// by 16 tiles, three levels, 8 by 8 blocks, over `components`, with
/// `cb_style`'s Table A.19 bits.
fn many_blocks(components: &[(u8, bool, u8, u8)], mct: bool, cb_style: u8) -> Spec {
    Spec {
        xsiz: 40,
        ysiz: 34,
        xosiz: 3,
        yosiz: 5,
        xtsiz: 16,
        ytsiz: 16,
        xtosiz: 0,
        ytosiz: 0,
        components: components.to_vec(),
        levels: 3,
        xcb: 1,
        ycb: 1,
        cb_style,
        mct: u8::from(mct),
        ..Spec::default()
    }
}

/// What a decode came to: the image and its counts, or the refusal's
/// warnings; and the warnings either way.
type Outcome = (
    Result<(crate::jpx::JpxImage, crate::jpx::JpxCounts), ()>,
    Vec<Warning>,
);

fn on(bytes: &[u8], request: &JpxRequest, executor: &dyn Executor) -> Outcome {
    let mut warnings = Vec::new();
    let decoded = jpx_decode_on(bytes, &limits(), request, executor, &mut warnings).map_err(|_| ());
    (decoded, warnings)
}

fn serial(bytes: &[u8], request: &JpxRequest) -> Outcome {
    let mut warnings = Vec::new();
    let decoded = jpx_decode_request(bytes, &limits(), request, &mut warnings).map_err(|_| ());
    (decoded, warnings)
}

/// Tier-1 alone over `bytes`'s tiles on `executor` from a budget of
/// `budget`: what it came to, the budget left, and every block's
/// coefficients, by tile, component, resolution, band, precinct and block.
fn tier1_on(
    bytes: &[u8],
    executor: &dyn Executor,
    budget: u64,
) -> Option<(Result<u64, Refusal>, u64, Vec<Vec<i32>>)> {
    let stream = parse(bytes).ok()?;
    let mut tiles: Vec<Tile> = tier2::decode_tiles(&stream).ok()?;
    let (decoded, left) = tier1::decode_tiles_within(&stream, &mut tiles, 0, executor, budget);
    let coefficients = tiles
        .iter()
        .flat_map(|tile| tile.components.iter())
        .flat_map(|component| component.resolutions.iter())
        .flat_map(|resolution| resolution.bands.iter())
        .flat_map(|band| band.precincts.iter())
        .flat_map(|precinct| precinct.blocks.iter())
        .map(|block| block.coefficients.clone())
        .collect();
    Some((decoded, left, coefficients))
}

/// The whole cost tier-1 charges `bytes`'s blocks: each block's area times
/// its passes, as `account` charges it.
fn total_charge(bytes: &[u8]) -> u64 {
    let stream = parse(bytes).expect("it parses");
    let tiles = tier2::decode_tiles(&stream).expect("tier-2 reads it");
    tiles
        .iter()
        .flat_map(|tile| tile.components.iter())
        .flat_map(|component| component.resolutions.iter())
        .flat_map(|resolution| resolution.bands.iter())
        .flat_map(|band| band.precincts.iter())
        .flat_map(|precinct| precinct.blocks.iter())
        .map(|block| u64::from(block.width()) * u64::from(block.height()) * u64::from(block.passes))
        .sum()
}

/// **Code-blocks on 2, 3, 4 and 7 threads are the serial decode.** Every
/// codestream under `tests/jpx/` decoded whole, and writer-made codestreams
/// of many tiles and blocks — both filters, G.2's transform, segmentation
/// symbols — whole, with levels dropped and over a window, give the serial
/// decode's samples, counts and warnings at every thread count. A budget
/// spent part way through tier-1 refuses the same block with the same budget
/// left and the same blocks decoded before it; and damaged codestreams under
/// segmentation symbols, which refuse at a block whose D.5 symbol has
/// drifted, refuse it alike, the budget ending as the serial decode leaves
/// it.
#[test]
fn code_blocks_on_2_3_4_and_7_threads_are_the_serial_decode() {
    let threads = [2usize, 3, 4, 7];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jpx");
    let fixtures: Vec<Vec<u8>> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jp2"))
        .map(|path| std::fs::read(path).expect("a fixture"))
        .collect();
    assert!(fixtures.len() > 30, "the committed codestreams");
    let made = [
        coded(&many_blocks(&[(8, false, 1, 1); 3], true, 0), &noise),
        coded(
            &Spec {
                transform: 0,
                quant_style: 2,
                ..many_blocks(&[(8, false, 1, 1); 3], true, 0)
            },
            &noise,
        ),
        coded(&many_blocks(&[(8, false, 1, 1)], false, 0x20), &noise),
    ];
    let whole = JpxRequest::default();
    let requests = [
        whole,
        JpxRequest {
            discard: 1,
            window: None,
        },
        JpxRequest {
            discard: 0,
            window: Some(JpxWindow {
                x: 5,
                y: 3,
                width: 17,
                height: 9,
            }),
        },
    ];
    let mut checked = 0;
    for fixture in &fixtures {
        let want = serial(fixture, &whole);
        assert!(want.0.is_ok(), "every committed codestream decodes");
        for n in threads {
            assert!(on(fixture, &whole, &Spread::new(n)) == want, "{n} threads");
            checked += 1;
        }
    }
    for bytes in &made {
        for request in &requests {
            let want = serial(bytes, request);
            assert!(want.0.is_ok(), "{request:?}");
            for n in threads {
                assert!(
                    on(bytes, request, &Spread::new(n)) == want,
                    "{n} threads, {request:?}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, fixtures.len() * 4 + 3 * 3 * 4);

    // Half the budget the whole of tier-1 needs: it runs out part way.
    let bytes = &made[0];
    let budget = total_charge(bytes) / 2;
    let want = tier1_on(bytes, &tinker_pdf_pool::Parallelism::Serial, budget).expect("it parses");
    assert!(
        matches!(want.0, Err(Refusal::Budget(_))),
        "the budget runs out part way: {:?}",
        want.0
    );
    assert!(
        want.2.iter().any(|block| !block.is_empty()) && want.2.iter().any(Vec::is_empty),
        "some blocks decoded before it ran out and some not"
    );
    for n in threads {
        assert!(
            tier1_on(bytes, &Spread::new(n), budget) == Some(want.clone()),
            "{n} threads: the same refusal, budget and blocks"
        );
    }

    // Damage under segmentation symbols: every byte of the third codestream
    // overwritten in turn, and each that tier-1 refuses for a drifted symbol
    // refused alike.
    let marked = &made[2];
    let mut drifted = 0;
    for at in (0..marked.len()).step_by(3) {
        let mut damaged = marked.clone();
        damaged[at] ^= 0x3C;
        let Some(want) = tier1_on(&damaged, &tinker_pdf_pool::Parallelism::Serial, u64::MAX) else {
            continue;
        };
        if want.0 == Err(Refusal::SegmentationSymbol) {
            drifted += 1;
        }
        for n in threads {
            assert!(
                tier1_on(&damaged, &Spread::new(n), u64::MAX) == Some(want.clone()),
                "byte {at}, {n} threads"
            );
        }
        let request = serial(&damaged, &whole);
        assert!(
            on(&damaged, &whole, &Spread::new(3)) == request,
            "byte {at}, the whole decode"
        );
    }
    assert!(
        drifted > 0,
        "some damage drifts a block's segmentation symbol"
    );
}

/// **A decode on four workers shares its code-blocks among them.** The
/// writer-made codestream of many tiles and blocks decoded on four threads of
/// the test's own: one job a decoded block, as many jobs as the decode says it
/// decoded, each run once, and every one of the four threads — none of them
/// the caller — ran some.
#[test]
fn a_decode_on_four_workers_shares_its_code_blocks() {
    let bytes = coded(&many_blocks(&[(8, false, 1, 1); 3], true, 0), &noise);
    let spread = Spread::new(4);
    let mut warnings = Vec::new();
    let (_, counts) = jpx_decode_on(
        &bytes,
        &limits(),
        &JpxRequest::default(),
        &spread,
        &mut warnings,
    )
    .expect("it decodes");
    let ran = spread.ran.into_inner().expect("the record");
    assert_eq!(
        ran.len() as u64,
        counts.code_blocks_decoded,
        "one job a block"
    );
    let mut jobs: Vec<usize> = ran.iter().map(|(job, _)| *job).collect();
    jobs.sort_unstable();
    jobs.dedup();
    assert_eq!(jobs.len(), ran.len(), "each block decoded once");
    let caller = thread::current().id();
    let mut workers: Vec<ThreadId> = ran.iter().map(|(_, thread)| *thread).collect();
    workers.sort_unstable_by_key(|thread| format!("{thread:?}"));
    workers.dedup();
    assert_eq!(workers.len(), 4, "every worker took some");
    assert!(!workers.contains(&caller), "and none is the caller");
}
