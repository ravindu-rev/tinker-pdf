//! What a character costs the structured-text writer in allocations
//! (`docs/design/performance.md`, P-02, "The structured-text writer's
//! numbers"): its ten numbers — its quad's eight and its origin's two — are
//! written straight into the output, as are its line's sizes compared, so a
//! page's JSON, XML or HTML allocates for its output's growth and its lines,
//! and nothing for each character.
//!
//! No tally sees an allocation, so this binary installs the guard suite's
//! counting allocator (`guard_support::Counting`), which counts each
//! allocation against the thread that made it: one test, writing on its own
//! thread. `#![forbid(unsafe_code)]` binds the library this tests and is
//! untouched; the allocator forwards every call to `System` and reads no
//! document byte.

mod guard_support;
mod render_support;

use std::sync::Arc;

use guard_support::{allocations_of, Counting};
use tinker_pdf::{
    PageFrame, Quad, TextBlock, TextChar, TextFormat, TextLine, TextPage, WritingMode,
};

#[global_allocator]
static COUNTING: Counting = Counting;

/// The characters of the smaller page, and of the larger.
const SIZES: [usize; 2] = [1_000, 4_000];

/// A page of one line of `chars` characters in one font at one size, each set
/// an odd fraction of a point after the last, so every number it carries has
/// places to write, and some a tie at the fourth decimal. Built outside the
/// count.
fn line_page(chars: usize) -> TextPage {
    let font: Arc<str> = Arc::from("Curvy");
    let advance = 5.4375;
    let quad = |x: f64, width: f64| Quad {
        ul: (x, 750.913),
        ur: (x + width, 750.913),
        ll: (x, 739.0625),
        lr: (x + width, 739.0625),
    };
    let characters: Vec<TextChar> = (0..chars)
        .map(|at| {
            let x = 36.25 + at as f64 * advance;
            TextChar {
                text: char::from(b'a' + (at % 26) as u8).to_string(),
                quad: quad(x, advance),
                size: 11.3,
                origin: (x, 742.017),
                mcid: None,
                stream: 0,
                font: Some(Arc::clone(&font)),
                stated: None,
                artifact: false,
            }
        })
        .collect();
    let text: String = characters.iter().map(|c| c.text.as_str()).collect();
    let bounds = quad(36.25, chars as f64 * advance);
    TextPage {
        blocks: vec![TextBlock {
            quad: bounds,
            lines: vec![TextLine {
                chars: characters,
                text,
                quad: bounds,
                wmode: WritingMode::Horizontal,
                rtl: false,
                size: 11.3,
            }],
        }],
        mcid_props: Default::default(),
        stated: Vec::new(),
        warnings: Vec::new(),
    }
}

/// **P-02.6**: a character writes its numbers without allocating. Each
/// format writes the page of 4 000 characters with fewer than one allocation
/// for every hundred characters more than the page of 1 000 takes — what the
/// output's growth costs; a character whose ten numbers each made a `String`
/// cost ten — and the whole page with fewer than one for every hundred
/// characters it holds.
#[test]
fn a_character_writes_its_numbers_without_allocating() {
    let frame = PageFrame {
        index: 0,
        bounds: (0.0, 0.0, 612.0, 792.0),
        rotation: 0,
    };
    let pages = SIZES.map(line_page);
    for format in [TextFormat::Json, TextFormat::Xml, TextFormat::Html] {
        let [small, large] = [&pages[0], &pages[1]]
            .map(|page| allocations_of(|| drop(page.serialize(format, &frame))) as usize);
        let added = SIZES[1] - SIZES[0];
        assert!(
            large.saturating_sub(small) * 100 < added,
            "{format:?}: {small} allocations for {} characters and {large} for {}: \
             a character allocates",
            SIZES[0],
            SIZES[1]
        );
        assert!(
            large * 100 < SIZES[1],
            "{format:?}: {large} allocations for {} characters",
            SIZES[1]
        );
        println!(
            "text-allocations: RAN {format:?}, {small} allocations for {} characters \
             and {large} for {}",
            SIZES[0], SIZES[1]
        );
    }
}
