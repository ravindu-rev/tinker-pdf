//! The OCR import seam (ROADMAP row OC-01): hOCR, ALTO and PAGE XML read into
//! FT-24's word-box model, over arbitrary bytes.
//!
//! The input is the file, whole. The format is told from the file, so a
//! mutation that renames a root element or its namespace moves the input from
//! one reader to another, and one that breaks the markup reaches the
//! refusals. No control byte: every seed is a file the import reads.
//!
//! # What this target checks
//!
//! **That the import never panics, hangs or exhausts memory** (ruling 1) — the
//! XML reader's tree, the HTML parser's, the `title` grammar, the reading
//! orders' walks over names a file chooses, and the word cap — and, on every
//! file it imports, what the model promises:
//!
//! - every word has text, with no ASCII white space at either end and no
//!   character that is no text, and a box of finite numbers;
//! - no page keeps more than `MAX_IMPORTED_WORDS` words;
//! - every confidence is on the model's scale, 0 to 100;
//! - every line of the model has its setting beside it, and every turn is
//!   from 0 up to a whole turn;
//! - every page has an extent with area.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Whether a file is read as its format states it. That is
//! `crates/tinker-pdf/tests/ocr_import.rs`, which holds the import to
//! hand-written files whose words, boxes and order it states.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::ocr::{import, MAX_IMPORTED_WORDS};

fuzz_target!(|data: &[u8]| {
    let Ok(imported) = import(data) else {
        return;
    };
    for page in &imported.pages {
        assert!(
            page.extent.width() > 0.0 && page.extent.height() > 0.0,
            "a page with no area was imported"
        );
        let mut words = 0usize;
        let mut lines = 0usize;
        for block in &page.page.blocks {
            assert!(block.quad.is_finite());
            for paragraph in &block.paragraphs {
                assert!(paragraph.quad.is_finite());
                for line in &paragraph.lines {
                    assert!(line.quad.is_finite());
                    lines += 1;
                    for word in &line.words {
                        words += 1;
                        assert!(word.quad.is_finite(), "a word's box is not finite");
                        assert!(!word.text.is_empty(), "an empty word was kept");
                        assert_eq!(
                            word.text.trim_matches(|c: char| c.is_ascii_whitespace()),
                            word.text,
                            "a word keeps white space at an end"
                        );
                        assert!(
                            !word.text.chars().any(|c| c.is_control() || c == '\u{FEFF}'),
                            "a word keeps a character that is no text"
                        );
                        if let Some(confidence) = word.confidence {
                            assert!((0.0..=100.0).contains(&confidence));
                        }
                    }
                }
            }
        }
        assert!(words <= MAX_IMPORTED_WORDS, "a page past the word cap");
        assert_eq!(page.lines.len(), lines, "a line without its setting");
        assert!(page.lines.iter().all(|l| (0.0..360.0).contains(&l.turn)));
    }
});
