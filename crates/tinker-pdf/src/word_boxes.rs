//! A page's word boxes in the frame of a render: [`Page::word_boxes`] and
//! [`Document::word_boxes`].
//!
//! The model, the word rule and the writers are
//! `tinker_pdf_content::word_boxes`'. What is here is the frame, which only the
//! facade can make — the page's view transform is the renderer's — and the
//! fill from [`Page::text`]. The design is `docs/design/word-boxes.md`.
//!
//! # The frame
//!
//! The page as displayed, crop box and `/Rotate` applied, origin at the
//! top-left, `y` down, in pixels of `1 / resolution` inch: exactly the frame
//! [`Page::render`] draws in at `RenderOptions::scale = resolution / 72`, its
//! clamp for an enormous page included, so a box stands on the pixels a
//! render at the same resolution draws its word in. The page's size is
//! [`Page::pixel_size`] at that scale.
//!
//! # The fill
//!
//! One block per [`crate::TextBlock`] of [`Page::text`], each holding one
//! paragraph whose box is the block's — a born-digital page states no
//! paragraphs — and one line per [`crate::TextLine`], its words
//! `tinker_pdf_content::word_boxes::box_words`'. Every quad is carried into
//! the frame corner by corner. A word whose box does not meet the page is left
//! out, and a line or a block left empty goes with it. No word has a
//! confidence: the page states its text.

use tinker_pdf_content::word_boxes::box_words;
use tinker_pdf_content::{
    LineDirection, Matrix, Quad, WordBlock, WordBox, WordBoxes, WordLine, WordPage, WordParagraph,
    WritingMode,
};
use tinker_pdf_cos::{parse_date, Date};

use crate::{Document, Page, RenderOptions};

/// How word boxes are measured.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct WordBoxOptions {
    /// The frame's pixels per inch: the boxes are where a render at this
    /// resolution draws the words. 72 by default, which makes a pixel a point;
    /// a value that is not a finite positive number is read as 72.
    pub resolution: f64,
}

impl Default for WordBoxOptions {
    fn default() -> Self {
        WordBoxOptions { resolution: 72.0 }
    }
}

impl WordBoxOptions {
    /// Options measuring at `resolution` pixels per inch.
    #[must_use]
    pub fn at_dpi(resolution: f64) -> WordBoxOptions {
        WordBoxOptions { resolution }
    }
}

impl Page {
    /// The page's words with their boxes, in the frame of a render at
    /// `options.resolution` (see the module documentation of `word_boxes`).
    ///
    /// Read from [`Page::text`], so each line is in logical order and a span's
    /// stated text stands where its glyphs do; a word is one of
    /// [`crate::TextLine::words`] with the punctuation it touches. Write it with
    /// [`WordBoxes::tsv`] through [`Document::word_boxes`], or build a
    /// [`WordBoxes`] of the pages wanted.
    #[must_use]
    pub fn word_boxes(&self, options: &WordBoxOptions) -> WordPage {
        let asked = if options.resolution.is_finite() && options.resolution > 0.0 {
            options.resolution
        } else {
            72.0
        };
        let render = RenderOptions {
            scale: asked / 72.0,
            ..RenderOptions::default()
        };
        let (wanted, scale) = self.scales(&render);
        let (width, height) = self.pixel_size(&render);
        // An enormous page is drawn at a smaller scale than the one asked for,
        // and its frame is the one it is drawn in.
        let resolution = if scale < wanted { scale * 72.0 } else { asked };
        let to_frame =
            tinker_pdf_render::page_view_transform(self.crop_box(), self.rotation(), scale);

        let text = self.text();
        let mut blocks = Vec::new();
        for block in &text.blocks {
            let mut lines = Vec::new();
            for line in &block.lines {
                let words: Vec<WordBox> = box_words(line)
                    .into_iter()
                    .filter_map(|word| {
                        let quad = carried(&word.quad, &to_frame);
                        meets(&quad, width, height).then_some(WordBox {
                            text: word.text,
                            quad,
                            confidence: None,
                        })
                    })
                    .collect();
                if words.is_empty() {
                    continue;
                }
                let direction = if line.wmode == WritingMode::Vertical {
                    LineDirection::TopToBottom
                } else if line.rtl {
                    LineDirection::RightToLeft
                } else {
                    LineDirection::LeftToRight
                };
                lines.push(WordLine {
                    quad: carried(&line.quad, &to_frame),
                    direction,
                    words,
                });
            }
            if lines.is_empty() {
                continue;
            }
            let quad = carried(&block.quad, &to_frame);
            blocks.push(WordBlock {
                quad,
                paragraphs: vec![WordParagraph { quad, lines }],
            });
        }
        WordPage {
            index: self.index(),
            width,
            height,
            resolution,
            image: None,
            blocks,
        }
    }
}

impl Document {
    /// Every page's words with their boxes, as [`Page::word_boxes`] measures
    /// them, and the document's dates for PAGE XML's metadata: the
    /// information dictionary's `/CreationDate` and `/ModDate` (14.3.3), each
    /// standing for the other where only one reads as a date.
    #[must_use]
    pub fn word_boxes(&self, options: &WordBoxOptions) -> WordBoxes {
        let metadata = self.metadata();
        let created = metadata.creation_date.as_deref().and_then(date_time);
        let modified = metadata.modification_date.as_deref().and_then(date_time);
        WordBoxes {
            pages: self
                .pages()
                .iter()
                .map(|page| page.word_boxes(options))
                .collect(),
            created: created.clone().or_else(|| modified.clone()),
            modified: modified.or(created),
        }
    }
}

/// `quad` carried into the frame by `to_frame`, corner by corner, so each
/// corner keeps its name.
fn carried(quad: &Quad, to_frame: &Matrix) -> Quad {
    let at = |(x, y): (f64, f64)| to_frame.apply(x, y);
    Quad {
        ul: at(quad.ul),
        ur: at(quad.ur),
        ll: at(quad.ll),
        lr: at(quad.lr),
    }
}

/// Whether some of `quad`'s box is on a page of `width` by `height`.
fn meets(quad: &Quad, width: u32, height: u32) -> bool {
    let (x0, y0, x1, y1) = quad.bounds();
    x0 < f64::from(width) && x1 > 0.0 && y0 < f64::from(height) && y1 > 0.0
}

/// A PDF date (7.9.4) as an XML Schema `dateTime`, or `None` where it names
/// no day the type can state: a year outside 1 to 9999, a day past its
/// month's last, or a zone further than fourteen hours from UT.
fn date_time(text: &str) -> Option<String> {
    let Date {
        year,
        month,
        day,
        hour,
        minute,
        second,
        utc_offset_minutes,
    } = parse_date(text)?;
    if !(1..=9999).contains(&year) || day > days_in(year, month) {
        return None;
    }
    let mut out = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    match utc_offset_minutes {
        None => {}
        Some(0) => out.push('Z'),
        Some(offset) if offset.unsigned_abs() <= 14 * 60 => {
            let sign = if offset < 0 { '-' } else { '+' };
            let offset = offset.unsigned_abs();
            out.push_str(&format!("{sign}{:02}:{:02}", offset / 60, offset % 60));
        }
        Some(_) => return None,
    }
    Some(out)
}

/// The days in `month` of `year`, by the Gregorian calendar XML Schema reads
/// dates in.
fn days_in(year: i32, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF date reads as the `dateTime` it names, its zone kept, and a date
    /// no `dateTime` can state is none.
    #[test]
    fn a_pdf_date_reads_as_its_date_time() {
        assert_eq!(
            date_time("D:20260102030405Z").as_deref(),
            Some("2026-01-02T03:04:05Z")
        );
        assert_eq!(
            date_time("D:19991231235959+05'30'").as_deref(),
            Some("1999-12-31T23:59:59+05:30")
        );
        assert_eq!(
            date_time("D:20260102030405-08'00'").as_deref(),
            Some("2026-01-02T03:04:05-08:00")
        );
        // A date with no zone states none.
        assert_eq!(date_time("D:2026").as_deref(), Some("2026-01-01T00:00:00"));
        assert_eq!(
            date_time("D:20240229").as_deref(),
            Some("2024-02-29T00:00:00")
        );
        assert_eq!(date_time("D:20230229"), None, "no 29 February in 2023");
        assert_eq!(
            date_time("D:00001231"),
            None,
            "XML Schema 1.0 has no year 0"
        );
        assert_eq!(date_time("D:20260102030405+20'00'"), None);
        assert_eq!(date_time("not a date"), None);
    }

    /// A box meets the page when some of it is inside, and a box wholly past
    /// an edge does not.
    #[test]
    fn a_box_meets_the_page_when_some_of_it_is_on_it() {
        let quad = |x0: f64, y0: f64, x1: f64, y1: f64| {
            tinker_pdf_content::word_boxes::upright(x0, y0, x1, y1)
        };
        assert!(meets(&quad(10.0, 10.0, 20.0, 20.0), 100, 100));
        assert!(meets(&quad(-5.0, 10.0, 1.0, 20.0), 100, 100));
        assert!(!meets(&quad(-5.0, 10.0, 0.0, 20.0), 100, 100));
        assert!(!meets(&quad(100.0, 10.0, 120.0, 20.0), 100, 100));
        assert!(!meets(&quad(10.0, 100.5, 20.0, 120.0), 100, 100));
    }
}
