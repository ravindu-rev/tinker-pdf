//! An imported page's words written over a PDF page as an invisible text
//! layer: the import seam's second half (`docs/design/ocr.md`, *The text
//! layer*).
//!
//! [`add_text_layer`] draws each word of an [`ImportedPage`] in render mode 3
//! (9.3.6), in a glyph-less font fitted to the word's box, inside one form
//! XObject stamped over the page — so the page's own content streams stay the
//! objects they were and what the page draws does not change.
//!
//! # The frame
//!
//! The file's page is mapped onto the page as it is displayed — its crop box
//! turned by `/Rotate` — or onto a rectangle the caller names there, scaled on
//! each axis. The form's space is that displayed page in points, origin at
//! its lower-left corner, `y` up: every number the layer writes is a point to
//! six decimals, which carries back to well inside a thousandth of any unit a
//! recogniser states, and the form's `/Matrix` turns that space back into the
//! page's own — the crop box's offset and the quarter turn `/Rotate` states,
//! whose entries are exact.
//!
//! # The font
//!
//! One composite font a layer (9.7): a Type 0 font under `/Identity-H` over a
//! CIDFontType2 whose embedded TrueType program has two glyphs, `.notdef` and
//! an empty one, each 1 000 units wide on a 1 000-unit em, its `/CIDToGIDMap`
//! sending every CID the layer draws to the empty glyph, and a `/ToUnicode`
//! giving each CID back its character. Each distinct character the layer
//! draws is a CID, numbered from 1 in the order the layer first draws it.

use std::collections::BTreeMap;

use tinker_pdf_content::{Matrix, WordLine};
use tinker_pdf_cos::{
    encode_text_string, Dict, DocumentEditor, FormXObject, ObjRef, Object, PdfString, Rect,
    StampPlacement, StreamData,
};
use tinker_pdf_shape::bidi::{order_units, BaseDirection};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

use super::import::{Direction, ImportedPage, LineSetting};

/// The most distinct characters one layer draws: the CIDs 1 to 65 535 a
/// two-byte `/Identity-H` code names, CID 0 never drawn.
pub const MAX_LAYER_CHARACTERS: usize = 65_535;

/// The name the layer's font has in its form's resources and as its
/// `/BaseFont`.
const FONT_NAME: &[u8] = b"OCRLayer";

/// What a layer drew.
#[derive(Clone, Debug, PartialEq)]
pub struct Layered {
    /// The form XObject stamped over the page, or `None` where no word could
    /// be drawn and nothing was added.
    pub form: Option<ObjRef>,
    /// How many words the layer drew.
    pub drawn: usize,
    /// The words it could not draw, each named.
    pub warnings: Vec<LayerWarning>,
}

/// A word the layer could not draw, by its line's position among the
/// model's lines (block by block, paragraph by paragraph) and its own in its
/// line.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LayerWarning {
    /// A word whose box, carried onto the page, has no width or no height,
    /// or is not finite: it cannot be fitted, and is skipped.
    WordWithoutArea {
        /// The line's position among the page's lines.
        line: usize,
        /// The word's position in its line.
        word: usize,
    },
    /// A word with no text: no glyph to fit, and skipped.
    EmptyWord {
        /// The line's position among the page's lines.
        line: usize,
        /// The word's position in its line.
        word: usize,
    },
}

/// Why no layer was written.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LayerError {
    /// The document the editor edits has no such page.
    NoSuchPage {
        /// The page asked for.
        page: u32,
    },
    /// The imported page's extent, the rectangle named, or the page's crop
    /// box has no area, or is not finite: nothing can be mapped onto it.
    NoArea,
    /// The page's words hold more distinct characters than
    /// [`MAX_LAYER_CHARACTERS`].
    TooManyCharacters {
        /// The cap.
        limit: usize,
    },
}

impl core::fmt::Display for LayerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LayerError::NoSuchPage { page } => write!(f, "the document has no page {page}"),
            LayerError::NoArea => f.write_str("the layer's frame has no area"),
            LayerError::TooManyCharacters { limit } => {
                write!(f, "the page draws more than {limit} distinct characters")
            }
        }
    }
}

impl std::error::Error for LayerError {}

/// Writes `imported`'s words over page `page` of the document `editor` edits
/// as an invisible text layer (see the module documentation).
///
/// `onto` is the rectangle the file's page is mapped onto, in points on the
/// page as it is displayed — origin at the displayed page's lower-left
/// corner, `x` to the right, `y` up — and `None` maps it onto the whole
/// displayed page. Words that cannot be fitted are skipped and named; a page
/// is never refused for one word.
///
/// # Errors
/// [`LayerError`]: a page the document does not have, a frame with no area,
/// or more distinct characters than a layer can name.
pub fn add_text_layer(
    editor: &mut DocumentEditor,
    page: u32,
    imported: &ImportedPage,
    onto: Option<Rect>,
) -> Result<Layered, LayerError> {
    let view = editor.view().map_err(|_| LayerError::NoSuchPage { page })?;
    let geometry = tinker_pdf_cos::pages::at(&view, page).ok_or(LayerError::NoSuchPage { page })?;
    let crop = geometry.crop_box;
    let displayed = geometry.display_size();
    let frame = Frame::new(imported, onto, displayed)?;

    let mut characters = Characters::default();
    let mut content = String::from("3 Tr\n");
    let mut warnings = Vec::new();
    let mut drawn = 0usize;
    let lines = imported
        .page
        .blocks
        .iter()
        .flat_map(|block| &block.paragraphs)
        .flat_map(|paragraph| &paragraph.lines);
    for (index, line) in lines.enumerate() {
        let setting = imported.lines.get(index).copied().unwrap_or(LineSetting {
            turn: 0.0,
            direction: None,
        });
        drawn += draw_line(
            &mut content,
            &mut characters,
            &mut warnings,
            &frame,
            line,
            index,
            setting,
        )?;
    }
    if drawn == 0 {
        return Ok(Layered {
            form: None,
            drawn,
            warnings,
        });
    }

    // The form's space is the displayed page in points, `y` up; its matrix
    // takes that space to the page's device frame at scale 1 (`y` down, from
    // the top-left) and back through the page's view transform to the page's
    // own user space.
    let flip = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: -1.0,
        e: 0.0,
        f: displayed.1,
    };
    let view_transform = tinker_pdf_render::page_view_transform(
        (crop.x0, crop.y0, crop.x1, crop.y1),
        geometry.rotation,
        1.0,
    );
    let to_user = inverse(&view_transform).ok_or(LayerError::NoArea)?;
    let matrix = flip.then(&to_user);

    // One edit: the font, the form and the stamp land together or not at all.
    editor.transaction(|editor| {
        let font = write_font(editor, &characters);
        let mut fonts = Dict::new();
        fonts.insert(editor.intern(FONT_NAME), Object::Ref(font));
        let mut resources = Dict::new();
        resources.insert(editor.intern(b"Font"), Object::Dict(fonts));
        let form = editor
            .add_form(
                &FormXObject {
                    bbox: [
                        frame.left,
                        frame.top - frame.height,
                        frame.left + frame.width,
                        frame.top,
                    ],
                    matrix: Some([matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f]),
                    group: None,
                    content: content.as_bytes(),
                },
                resources,
            )
            .ok_or(LayerError::NoArea)?;
        editor
            .stamp(page, form, StampPlacement::Over)
            .ok_or(LayerError::NoSuchPage { page })?;
        Ok(Layered {
            form: Some(form),
            drawn,
            warnings,
        })
    })
}

/// The file's page mapped onto the displayed page: where its extent lands,
/// in points, `y` up.
struct Frame {
    /// The file's extent.
    from_left: f64,
    from_top: f64,
    /// The rectangle it lands on: its left and top edges, and its size.
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    /// Points a unit of the file, on each axis.
    sx: f64,
    sy: f64,
}

impl Frame {
    /// The imported page's extent onto `onto`, or onto the whole displayed
    /// page of `displayed` points: refused where either has no area.
    fn new(
        imported: &ImportedPage,
        onto: Option<Rect>,
        displayed: (f64, f64),
    ) -> Result<Frame, LayerError> {
        let extent = imported.extent;
        let (from_width, from_height) = (extent.width(), extent.height());
        let (left, top, width, height) = match onto {
            None => (0.0, displayed.1, displayed.0, displayed.1),
            Some(rect) => {
                let (x0, x1) = (rect.x0.min(rect.x1), rect.x0.max(rect.x1));
                let (y0, y1) = (rect.y0.min(rect.y1), rect.y0.max(rect.y1));
                (x0, y1, x1 - x0, y1 - y0)
            }
        };
        let finite = [
            extent.left,
            extent.top,
            from_width,
            from_height,
            left,
            top,
            width,
            height,
            displayed.0,
            displayed.1,
        ]
        .iter()
        .all(|v| v.is_finite());
        if !finite
            || from_width <= 0.0
            || from_height <= 0.0
            || width <= 0.0
            || height <= 0.0
            || displayed.0 <= 0.0
            || displayed.1 <= 0.0
        {
            return Err(LayerError::NoArea);
        }
        Ok(Frame {
            from_left: extent.left,
            from_top: extent.top,
            left,
            top,
            width,
            height,
            sx: width / from_width,
            sy: height / from_height,
        })
    }

    /// A box of the file's frame on the displayed page: left, bottom, right
    /// and top in points, `y` up.
    fn place(&self, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        (
            self.left + (x0 - self.from_left) * self.sx,
            self.top - (y1 - self.from_top) * self.sy,
            self.left + (x1 - self.from_left) * self.sx,
            self.top - (y0 - self.from_top) * self.sy,
        )
    }
}

/// A two-dimensional affine map's inverse, or `None` for one with no inverse.
fn inverse(m: &Matrix) -> Option<Matrix> {
    let det = m.a * m.d - m.b * m.c;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    Some(Matrix {
        a: m.d / det,
        b: -m.b / det,
        c: -m.c / det,
        d: m.a / det,
        e: (m.c * m.f - m.d * m.e) / det,
        f: (m.b * m.e - m.a * m.f) / det,
    })
}

/// The characters a layer draws, each a CID from 1 in the order first drawn.
#[derive(Default)]
struct Characters {
    cids: BTreeMap<char, u16>,
    /// The characters in CID order, CID 1 first.
    order: Vec<char>,
}

impl Characters {
    fn cid(&mut self, c: char) -> Result<u16, LayerError> {
        if let Some(&cid) = self.cids.get(&c) {
            return Ok(cid);
        }
        if self.order.len() >= MAX_LAYER_CHARACTERS {
            return Err(LayerError::TooManyCharacters {
                limit: MAX_LAYER_CHARACTERS,
            });
        }
        self.order.push(c);
        // At most `MAX_LAYER_CHARACTERS`, which is `u16::MAX`, by the check
        // above.
        let cid = u16::try_from(self.order.len()).unwrap_or(u16::MAX);
        self.cids.insert(c, cid);
        Ok(cid)
    }

    /// The codes of `chars` as a hexadecimal string's digits.
    fn codes(&mut self, chars: &[char]) -> Result<String, LayerError> {
        let mut out = String::with_capacity(chars.len() * 4);
        for &c in chars {
            out.push_str(&format!("{:04X}", self.cid(c)?));
        }
        Ok(out)
    }
}

/// A number to six decimals, trailing zeros dropped, never `-0`.
fn number(out: &mut String, value: f64) {
    let text = format!("{value:.6}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" || trimmed == "-0" {
        out.push('0');
    } else {
        out.push_str(trimmed);
    }
}

fn numbers(out: &mut String, values: &[f64], operator: &str) {
    for value in values {
        number(out, *value);
        out.push(' ');
    }
    out.push_str(operator);
    out.push('\n');
}

/// A word ready to draw: its box on the displayed page and its characters in
/// the order they are drawn.
struct Placed {
    /// Left, bottom, right and top, in points.
    rect: (f64, f64, f64, f64),
    chars: Vec<char>,
}

/// The quarter turn nearest `turn`, as the line's run and up directions: the
/// unit vectors its glyphs advance along and stand along.
fn quarter(turn: f64) -> ((f64, f64), (f64, f64)) {
    let quarters = if turn.is_finite() {
        (turn / 90.0).round().rem_euclid(4.0)
    } else {
        0.0
    };
    if quarters == 1.0 {
        ((0.0, 1.0), (-1.0, 0.0))
    } else if quarters == 2.0 {
        ((-1.0, 0.0), (0.0, -1.0))
    } else if quarters == 3.0 {
        ((0.0, -1.0), (1.0, 0.0))
    } else {
        ((1.0, 0.0), (0.0, 1.0))
    }
}

/// A word's fit along a run: its start on its own baseline, its size, and
/// how long its advances run.
///
/// Text extraction boxes a glyph from a fifth of its size below its baseline
/// to four fifths above it (`TextDevice::show_glyph`), so a word set at its
/// box's thickness across the run, with its baseline a fifth of that in from
/// the box's edge, fills the box across; its advances, scaled to the box's
/// length, fill it along.
fn fit(rect: (f64, f64, f64, f64), run: (f64, f64)) -> ((f64, f64), f64, f64) {
    let (left, bottom, right, top) = rect;
    let (width, height) = (right - left, top - bottom);
    match run {
        // Up: the glyphs stand towards -x, so the baseline is a fifth in from
        // the right edge.
        (x, y) if x == 0.0 && y > 0.0 => ((right - width / 5.0, bottom), width, height),
        // Down: they stand towards +x.
        (x, y) if x == 0.0 && y < 0.0 => ((left + width / 5.0, top), width, height),
        // Leftwards: upside down, the baseline a fifth below the top edge.
        (x, _) if x < 0.0 => ((right, top - height / 5.0), height, width),
        _ => ((left, bottom + height / 5.0), height, width),
    }
}

fn dot(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

/// Whether a line holds a right-to-left character (UAX #9's `R` or `AL`).
fn holds_right_to_left(words: &[Placed]) -> bool {
    words
        .iter()
        .flat_map(|w| &w.chars)
        .any(|&c| matches!(bidi_class(c), BidiClass::R | BidiClass::AL))
}

/// Draws one line of the model as one text object: its words in reading
/// order, or — for a line holding a right-to-left character — in the visual
/// order UAX #9 gives the line's text, inside a span stating that text.
/// Returns how many words it drew.
#[allow(clippy::too_many_arguments)]
fn draw_line(
    content: &mut String,
    characters: &mut Characters,
    warnings: &mut Vec<LayerWarning>,
    frame: &Frame,
    line: &WordLine,
    index: usize,
    setting: LineSetting,
) -> Result<usize, LayerError> {
    let mut placed: Vec<Placed> = Vec::with_capacity(line.words.len());
    for (word_index, word) in line.words.iter().enumerate() {
        let rect = frame.place(word.quad.bounds());
        let (left, bottom, right, top) = rect;
        let finite = [left, bottom, right, top].iter().all(|v| v.is_finite());
        if !finite || right - left <= 0.0 || top - bottom <= 0.0 {
            warnings.push(LayerWarning::WordWithoutArea {
                line: index,
                word: word_index,
            });
            continue;
        }
        let chars: Vec<char> = word.text.chars().collect();
        if chars.is_empty() {
            warnings.push(LayerWarning::EmptyWord {
                line: index,
                word: word_index,
            });
            continue;
        }
        placed.push(Placed { rect, chars });
    }
    if placed.is_empty() {
        return Ok(0);
    }

    // The line's imported text, its drawn words with one space between each
    // two; and, for a right-to-left line, the order its words and their
    // characters are drawn in, left to right along the run.
    let text: String = placed
        .iter()
        .map(|w| w.chars.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    let rtl = holds_right_to_left(&placed);
    let order: Vec<usize> = if rtl {
        let direction = match setting.direction {
            Some(Direction::LeftToRight) => BaseDirection::LeftToRight,
            Some(Direction::RightToLeft) => BaseDirection::RightToLeft,
            None => BaseDirection::Auto,
        };
        // Each character of the text, and the word and the place in it it
        // holds: `None` for a space between two words.
        let mut owners: Vec<Option<(usize, usize)>> = Vec::new();
        let mut units: Vec<String> = Vec::new();
        for (w, word) in placed.iter().enumerate() {
            if w > 0 {
                owners.push(None);
                units.push(" ".to_string());
            }
            for (at, c) in word.chars.iter().enumerate() {
                owners.push(Some((w, at)));
                units.push(c.to_string());
            }
        }
        let unit_refs: Vec<&str> = units.iter().map(String::as_str).collect();
        let visual = order_units(&unit_refs, direction);
        let mut word_order: Vec<usize> = Vec::with_capacity(placed.len());
        let mut ordered = vec![false; placed.len()];
        let mut drawn_at: Vec<Vec<usize>> = vec![Vec::new(); placed.len()];
        for at in visual {
            let Some(Some((w, place))) = owners.get(at).copied() else {
                continue;
            };
            if let Some(seen) = ordered.get_mut(w) {
                if !*seen {
                    *seen = true;
                    word_order.push(w);
                }
            }
            if let Some(places) = drawn_at.get_mut(w) {
                places.push(place);
            }
        }
        for (word, mut places) in placed.iter_mut().zip(drawn_at) {
            // A character X9 takes out of the order — a formatting
            // character — is still the word's, and is drawn after the rest.
            let mut drawn = vec![false; word.chars.len()];
            for &place in &places {
                if let Some(seen) = drawn.get_mut(place) {
                    *seen = true;
                }
            }
            places.extend(
                drawn
                    .iter()
                    .enumerate()
                    .filter(|(_, seen)| !**seen)
                    .map(|(place, _)| place),
            );
            word.chars = places
                .into_iter()
                .filter_map(|place| word.chars.get(place).copied())
                .collect();
        }
        // A word UAX #9 placed nowhere is drawn after the rest.
        for (w, seen) in ordered.iter().enumerate() {
            if !seen {
                word_order.push(w);
            }
        }
        word_order
    } else {
        (0..placed.len()).collect()
    };

    let (run, up) = quarter(setting.turn);
    let mut fits = Vec::with_capacity(order.len());
    for &w in &order {
        let Some(word) = placed.get(w) else {
            continue;
        };
        let (start, size, length) = fit(word.rect, run);
        fits.push((w, start, size, length));
    }
    let Some(&(_, first_start, _, _)) = fits.first() else {
        return Ok(0);
    };
    let baseline = dot(first_start, up);

    if rtl {
        content.push_str("/Span << /ActualText <");
        let stated = encode_text_string(&text, (1, 7));
        for byte in &stated.bytes {
            content.push_str(&format!("{byte:02X}"));
        }
        content.push_str("> >> BDC\n");
    }
    content.push_str("BT\n");
    for (at, &(w, start, size, length)) in fits.iter().enumerate() {
        let Some(word) = placed.get(w) else {
            continue;
        };
        // On the line's baseline, raised or lowered onto its own box.
        let rise = dot(start, up) - baseline;
        let origin = (start.0 - rise * up.0, start.1 - rise * up.1);
        numbers(
            content,
            &[run.0, run.1, up.0, up.1, origin.0, origin.1],
            "Tm",
        );
        content.push('/');
        content.push_str(core::str::from_utf8(FONT_NAME).unwrap_or("OCRLayer"));
        content.push(' ');
        numbers(content, &[size], "Tf");
        let count = word.chars.len() as f64;
        numbers(content, &[100.0 * length / (count * size)], "Tz");
        numbers(content, &[rise], "Ts");
        content.push('<');
        content.push_str(&characters.codes(&word.chars)?);
        content.push_str("> Tj\n");
        // A space after every word but the last: as long as the gap to the
        // next word's start where it starts further along the run, and a
        // hundredth of an em where it does not.
        if let Some(&(_, next_start, _, _)) = fits.get(at + 1) {
            let next_rise = dot(next_start, up) - baseline;
            let next_origin = (
                next_start.0 - next_rise * up.0,
                next_start.1 - next_rise * up.1,
            );
            let gap = dot((next_origin.0 - origin.0, next_origin.1 - origin.1), run) - length;
            let scale = if gap > 0.0 { 100.0 * gap / size } else { 1.0 };
            numbers(content, &[scale], "Tz");
            content.push('<');
            content.push_str(&characters.codes(&[' '])?);
            content.push_str("> Tj\n");
        }
    }
    content.push_str("ET\n");
    if rtl {
        content.push_str("EMC\n");
    }
    Ok(fits.len())
}

/// Writes the layer's font — the Type 0 font, its CIDFontType2, the
/// descriptor, the program, the CID-to-glyph map and the `/ToUnicode` — and
/// returns the Type 0 font.
fn write_font(editor: &mut DocumentEditor, characters: &Characters) -> ObjRef {
    let name = |editor: &DocumentEditor, bytes: &[u8]| Object::Name(editor.intern(bytes));
    let program = glyphless_program();

    let file = editor.allocate();
    let mut dict = Dict::new();
    dict.insert(
        editor.intern(b"Length1"),
        Object::Int(i64::try_from(program.len()).unwrap_or(i64::MAX)),
    );
    editor.put_stream(
        file,
        StreamData {
            dict,
            data: program,
        },
    );

    let descriptor = editor.allocate();
    let mut dict = Dict::new();
    dict.insert(editor.intern(b"Type"), name(editor, b"FontDescriptor"));
    dict.insert(editor.intern(b"FontName"), name(editor, FONT_NAME));
    dict.insert(editor.intern(b"Flags"), Object::Int(4));
    dict.insert(
        editor.intern(b"FontBBox"),
        Object::Array(vec![
            Object::Int(0),
            Object::Int(-200),
            Object::Int(1000),
            Object::Int(800),
        ]),
    );
    dict.insert(editor.intern(b"ItalicAngle"), Object::Int(0));
    dict.insert(editor.intern(b"Ascent"), Object::Int(800));
    dict.insert(editor.intern(b"Descent"), Object::Int(-200));
    dict.insert(editor.intern(b"CapHeight"), Object::Int(800));
    dict.insert(editor.intern(b"StemV"), Object::Int(0));
    dict.insert(editor.intern(b"FontFile2"), Object::Ref(file));
    editor.put(descriptor, Object::Dict(dict));

    // 9.7.4.2: two bytes a CID, the glyph it draws — the empty glyph for
    // every CID the layer draws, and `.notdef` for CID 0, which it never does.
    let map = editor.allocate();
    let mut data = vec![0u8, 0];
    for _ in &characters.order {
        data.extend_from_slice(&1u16.to_be_bytes());
    }
    editor.put_stream(
        map,
        StreamData {
            dict: Dict::new(),
            data,
        },
    );

    let cid_font = editor.allocate();
    let mut info = Dict::new();
    info.insert(
        editor.intern(b"Registry"),
        Object::String(PdfString::literal(b"Adobe".to_vec())),
    );
    info.insert(
        editor.intern(b"Ordering"),
        Object::String(PdfString::literal(b"Identity".to_vec())),
    );
    info.insert(editor.intern(b"Supplement"), Object::Int(0));
    let mut dict = Dict::new();
    dict.insert(editor.intern(b"Type"), name(editor, b"Font"));
    dict.insert(editor.intern(b"Subtype"), name(editor, b"CIDFontType2"));
    dict.insert(editor.intern(b"BaseFont"), name(editor, FONT_NAME));
    dict.insert(editor.intern(b"CIDSystemInfo"), Object::Dict(info));
    dict.insert(editor.intern(b"FontDescriptor"), Object::Ref(descriptor));
    dict.insert(editor.intern(b"DW"), Object::Int(1000));
    dict.insert(editor.intern(b"CIDToGIDMap"), Object::Ref(map));
    editor.put(cid_font, Object::Dict(dict));

    let to_unicode = editor.allocate();
    editor.put_stream(
        to_unicode,
        StreamData {
            dict: Dict::new(),
            data: to_unicode_cmap(&characters.order).into_bytes(),
        },
    );

    let font = editor.allocate();
    let mut dict = Dict::new();
    dict.insert(editor.intern(b"Type"), name(editor, b"Font"));
    dict.insert(editor.intern(b"Subtype"), name(editor, b"Type0"));
    dict.insert(editor.intern(b"BaseFont"), name(editor, FONT_NAME));
    dict.insert(editor.intern(b"Encoding"), name(editor, b"Identity-H"));
    dict.insert(
        editor.intern(b"DescendantFonts"),
        Object::Array(vec![Object::Ref(cid_font)]),
    );
    dict.insert(editor.intern(b"ToUnicode"), Object::Ref(to_unicode));
    editor.put(font, Object::Dict(dict));
    font
}

/// 9.10.3's `/ToUnicode` CMap: each CID, from 1, its character, as UTF-16BE.
fn to_unicode_cmap(order: &[char]) -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    // A `bfchar` block holds at most a hundred mappings (5014, 5.2).
    for (block, chunk) in order.chunks(100).enumerate() {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (at, c) in chunk.iter().enumerate() {
            let cid = block * 100 + at + 1;
            let mut units = [0u16; 2];
            let target: String = c
                .encode_utf16(&mut units)
                .iter()
                .map(|unit| format!("{unit:04X}"))
                .collect();
            out.push_str(&format!("<{cid:04X}> <{target}>\n"));
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

/// The layer's TrueType program: `.notdef` and one empty glyph, each 1 000
/// units wide on a 1 000-unit em, in the tables 9.9 requires of a
/// CIDFontType2's program — `glyf`, `head`, `hhea`, `hmtx`, `loca` and
/// `maxp` — with `post`, and no `cmap`, which a CIDFont's program does not
/// carry.
fn glyphless_program() -> Vec<u8> {
    let be16 = |v: u16| v.to_be_bytes();
    let be32 = |v: u32| v.to_be_bytes();

    let mut head = Vec::with_capacity(54);
    head.extend_from_slice(&be32(0x0001_0000)); // version
    head.extend_from_slice(&be32(0x0001_0000)); // fontRevision
    head.extend_from_slice(&be32(0)); // checkSumAdjustment, set below
    head.extend_from_slice(&be32(0x5F0F_3CF5)); // magicNumber
    head.extend_from_slice(&be16(0x0003)); // flags: baseline and lsb at zero
    head.extend_from_slice(&be16(1000)); // unitsPerEm
    head.extend_from_slice(&[0; 16]); // created, modified
    head.extend_from_slice(&be16(0)); // xMin
    head.extend_from_slice(&(-200i16).to_be_bytes()); // yMin
    head.extend_from_slice(&be16(1000)); // xMax
    head.extend_from_slice(&be16(800)); // yMax
    head.extend_from_slice(&be16(0)); // macStyle
    head.extend_from_slice(&be16(8)); // lowestRecPPEM
    head.extend_from_slice(&be16(2)); // fontDirectionHint
    head.extend_from_slice(&be16(0)); // indexToLocFormat: short
    head.extend_from_slice(&be16(0)); // glyphDataFormat

    let mut hhea = Vec::with_capacity(36);
    hhea.extend_from_slice(&be32(0x0001_0000)); // version
    hhea.extend_from_slice(&be16(800)); // ascender
    hhea.extend_from_slice(&(-200i16).to_be_bytes()); // descender
    hhea.extend_from_slice(&be16(0)); // lineGap
    hhea.extend_from_slice(&be16(1000)); // advanceWidthMax
    hhea.extend_from_slice(&[0; 6]); // minLeftSideBearing, minRightSideBearing, xMaxExtent
    hhea.extend_from_slice(&be16(1)); // caretSlopeRise
    hhea.extend_from_slice(&[0; 14]); // caretSlopeRun, caretOffset, four reserved, metricDataFormat
    hhea.extend_from_slice(&be16(2)); // numberOfHMetrics

    let mut hmtx = Vec::with_capacity(8);
    for _ in 0..2 {
        hmtx.extend_from_slice(&be16(1000)); // advanceWidth
        hmtx.extend_from_slice(&be16(0)); // leftSideBearing
    }

    // Short offsets, halved: both glyphs empty, so all three are zero.
    let loca = vec![0u8; 6];

    let mut maxp = Vec::with_capacity(32);
    maxp.extend_from_slice(&be32(0x0001_0000)); // version 1.0
    maxp.extend_from_slice(&be16(2)); // numGlyphs
    maxp.extend_from_slice(&[0; 8]); // maxPoints, maxContours, maxCompositePoints, maxCompositeContours
    maxp.extend_from_slice(&be16(2)); // maxZones
    maxp.extend_from_slice(&[0; 16]); // the rest

    let mut post = Vec::with_capacity(32);
    post.extend_from_slice(&be32(0x0003_0000)); // version 3: no glyph names
    post.extend_from_slice(&be32(0)); // italicAngle
    post.extend_from_slice(&(-100i16).to_be_bytes()); // underlinePosition
    post.extend_from_slice(&be16(50)); // underlineThickness
    post.extend_from_slice(&[0; 20]); // isFixedPitch and the four memory hints

    let tables: [(&[u8; 4], Vec<u8>); 7] = [
        (b"glyf", Vec::new()),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
        (b"post", post),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(&be32(0x0001_0000));
    out.extend_from_slice(&be16(tables.len() as u16));
    out.extend_from_slice(&be16(64)); // searchRange: 16 x 4
    out.extend_from_slice(&be16(2)); // entrySelector: log2 of 4
    out.extend_from_slice(&be16(48)); // rangeShift: 7 x 16 - 64
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    let mut head_at = 0usize;
    for (tag, data) in &tables {
        if *tag == b"head" {
            head_at = offset;
        }
        out.extend_from_slice(*tag);
        out.extend_from_slice(&be32(checksum(data)));
        out.extend_from_slice(&be32(offset as u32));
        out.extend_from_slice(&be32(data.len() as u32));
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend_from_slice(&body);
    // `head`'s checkSumAdjustment makes the whole font sum to 0xB1B0AFBA.
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    if let Some(slot) = out.get_mut(head_at + 8..head_at + 12) {
        slot.copy_from_slice(&adjustment.to_be_bytes());
    }
    out
}

/// The OpenType table checksum: the sum of the data's big-endian 32-bit
/// words, the last padded with zeros.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_is_a_font_this_engine_reads() {
        let program = glyphless_program();
        let face = tinker_pdf_font::Sfnt::parse(&program).expect("the program parses");
        assert_eq!(face.units_per_em, 1000);
        assert_eq!(face.advance(0), Some(1000));
        assert_eq!(face.advance(1), Some(1000));
        assert_eq!(
            checksum(&program),
            0xB1B0_AFBA,
            "the whole font sums to the magic"
        );
        for tag in [
            b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp", b"post",
        ] {
            assert!(
                face.table(u32::from_be_bytes(*tag)).is_some(),
                "{}",
                String::from_utf8_lossy(tag)
            );
        }
        assert!(
            face.table(u32::from_be_bytes(*b"cmap")).is_none(),
            "a CIDFont's program carries no cmap"
        );
    }

    #[test]
    fn numbers_are_written_to_six_decimals() {
        let mut out = String::new();
        for value in [0.0, -0.0, 1.5, -2.25, 1.0 / 3.0, -0.000_000_1, 1e7] {
            number(&mut out, value);
            out.push(' ');
        }
        assert_eq!(out, "0 0 1.5 -2.25 0.333333 0 10000000 ");
    }

    #[test]
    fn the_nearest_quarter_turn_is_drawn() {
        assert_eq!(quarter(0.0), ((1.0, 0.0), (0.0, 1.0)));
        assert_eq!(quarter(44.0), ((1.0, 0.0), (0.0, 1.0)));
        assert_eq!(quarter(90.0), ((0.0, 1.0), (-1.0, 0.0)));
        assert_eq!(quarter(181.0), ((-1.0, 0.0), (0.0, -1.0)));
        assert_eq!(quarter(270.0), ((0.0, -1.0), (1.0, 0.0)));
        assert_eq!(quarter(359.0), ((1.0, 0.0), (0.0, 1.0)));
        assert_eq!(quarter(f64::NAN), ((1.0, 0.0), (0.0, 1.0)));
    }

    /// A word's fit fills its box: its glyph boxes, a fifth of the size below
    /// the baseline to four fifths above, run from its start for its length.
    #[test]
    fn a_fitted_word_fills_its_box_at_every_quarter_turn() {
        let rect = (10.0, 20.0, 50.0, 30.0);
        for turn in [0.0, 90.0, 180.0, 270.0] {
            let (run, up) = quarter(turn);
            let (start, size, length) = fit(rect, run);
            let corners = [
                (start.0 - 0.2 * size * up.0, start.1 - 0.2 * size * up.1),
                (start.0 + 0.8 * size * up.0, start.1 + 0.8 * size * up.1),
                (
                    start.0 + length * run.0 + 0.8 * size * up.0,
                    start.1 + length * run.1 + 0.8 * size * up.1,
                ),
            ];
            let xs = corners.iter().map(|c| c.0);
            let ys = corners.iter().map(|c| c.1);
            let bounds = (
                xs.clone().fold(f64::INFINITY, f64::min),
                ys.clone().fold(f64::INFINITY, f64::min),
                xs.fold(f64::NEG_INFINITY, f64::max),
                ys.fold(f64::NEG_INFINITY, f64::max),
            );
            assert_eq!(bounds, rect, "turned {turn}");
        }
    }

    #[test]
    fn a_cmap_names_every_cid_in_blocks_of_a_hundred() {
        let order: Vec<char> = (0..150u32)
            .map(|n| char::from_u32(0x4E00 + n).unwrap_or(' '))
            .chain(['\u{1F600}'])
            .collect();
        let cmap = to_unicode_cmap(&order);
        assert!(cmap.contains("100 beginbfchar\n<0001> <4E00>\n"));
        assert!(cmap.contains("51 beginbfchar\n<0065> <4E64>\n"));
        assert!(cmap.contains("<0097> <D83DDE00>\n"), "{cmap}");
        assert!(cmap.contains("begincmap"));
    }

    #[test]
    fn an_affine_inverse_undoes_its_map() {
        let m = Matrix {
            a: 0.0,
            b: -1.0,
            c: 1.0,
            d: 0.0,
            e: 3.0,
            f: 5.0,
        };
        let inverse = inverse(&m).expect("invertible");
        let (x, y) = m.apply(7.0, 11.0);
        assert_eq!(inverse.apply(x, y), (7.0, 11.0));
        assert!(super::inverse(&Matrix {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 1.0,
            f: 1.0
        })
        .is_none());
    }

    #[test]
    fn characters_are_numbered_from_one_as_first_drawn() {
        let mut characters = Characters::default();
        assert_eq!(
            characters.codes(&['b', 'a', 'b']).ok(),
            Some("000100020001".into())
        );
        assert_eq!(characters.cid(' ').ok(), Some(3));
        assert_eq!(characters.order, ['b', 'a', ' ']);
    }
}
