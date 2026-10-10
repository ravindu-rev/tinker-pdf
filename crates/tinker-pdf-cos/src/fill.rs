//! Filling form fields and rebuilding their appearances (12.7.4.3).
//!
//! Setting `/V` is the easy half and the useless half: a value with no
//! matching appearance stream shows up in a viewer that regenerates
//! appearances and nowhere else, which is why filled forms so often print
//! blank. So every fill here rewrites the widget's `/AP` to match.
//!
//! The alternative — setting `/NeedAppearances` and hoping — is what produces
//! those files. This module clears that flag rather than setting it, because
//! once the appearances are right, asking viewers to rebuild them can only
//! make things worse.

use std::collections::HashMap;

use tinker_pdf_font::{Cff, Sfnt};
use tinker_pdf_shape::bidi::{reorder, BaseDirection, Paragraph};
use tinker_pdf_shape::shape::{itemize, Shaper};
use tinker_pdf_shape::{ShapedGlyph, Tag};

use crate::build::{close_array, number};
use crate::doc::CosDocument;
use crate::font::{self, Font, FontKind, ProgramKey};
use crate::form::{self, FieldKind};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages::Rect;
use crate::resolve::Resolve;
use crate::warn::{WarningKind, WarningSink};
use crate::write::StreamData;

/// Splits a `/DA` string into its font name, size, and everything else.
///
/// The remainder is replayed verbatim rather than interpreted, so a colour
/// this build does not understand still comes out right. The `Tf` operands
/// must be removed as *tokens*: dropping bytes leaves a stray number on the
/// stack, and the next operator consumes the wrong one.
fn operators(da: &[u8]) -> (Vec<u8>, f64, Vec<u8>) {
    let tokens: Vec<Vec<u8>> = da
        .split(|b| b.is_ascii_whitespace())
        .filter(|t| !t.is_empty())
        .map(<[u8]>::to_vec)
        .collect();

    let mut font = Vec::new();
    let mut size = 0.0;
    let mut rest: Vec<Vec<u8>> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == b"Tf" {
            if index >= 2 {
                font = tokens[index - 2]
                    .strip_prefix(b"/")
                    .unwrap_or(&tokens[index - 2])
                    .to_vec();
                size = std::str::from_utf8(&tokens[index - 1])
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(0.0);
                rest.truncate(rest.len().saturating_sub(2));
            }
            index += 1;
            continue;
        }
        rest.push(tokens[index].clone());
        index += 1;
    }

    if font.is_empty() {
        font = b"Helv".to_vec();
    }
    let mut joined = rest.join(&b' ');
    if joined.is_empty() {
        joined = b"0 g".to_vec();
    }
    (font, size, joined)
}

/// The width of a string in text-space units per 1000, using the font's own
/// metrics.
fn width_of(font: Option<&Font>, text: &str) -> f64 {
    let Some(font) = font else {
        // Half an em per character is close enough to keep auto-sizing sane
        // when the font cannot be read at all.
        return text.chars().count() as f64 * 500.0;
    };
    // A field's text is written as a single-byte string, so the code is the
    // byte, and characters outside the encoding are dropped by the writer too.
    text.chars()
        .map(|c| {
            let code = u32::from(c);
            font.width_of(code).0
        })
        .sum()
}

/// Escapes a string for a literal in a content stream (7.3.4.2), naming every
/// character it could not write.
///
/// A simple font addresses a *byte*, so a character above the single-byte
/// range has no code here at all. It is still drawn — ruling 2 degrades rather
/// than failing, and a field that vanished would be worse than one that shows
/// a mark — but the mark is now accompanied by a
/// [`WarningKind::FieldCharacterUnrepresentable`] naming the character, which
/// is the half ruling 10 adds. Before milestone 8 this function wrote `b'?'`
/// and said nothing, so a form whose Arabic value had become a row of question
/// marks was indistinguishable from one that had been filled correctly.
///
/// The shaped path is the *other* half of the answer, and where the `/DA`
/// font makes it available nothing reaches here; see [`Shapeable`].
pub(crate) fn escape(out: &mut Vec<u8>, text: &str, unwritable: &mut Vec<char>) {
    for c in text.chars() {
        let code = u32::from(c);
        let byte = if code < 256 {
            code as u8
        } else {
            if !unwritable.contains(&c) {
                unwritable.push(c);
            }
            b'?'
        };
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
}

/// A `/DA` font this build can shape a field's value against.
///
/// Milestone 8 of `docs/design/shaping.md`, and the reason the milestone was
/// blocked until now: `text_appearance` reaches its font through the AcroForm
/// `/DR`, and a [`Font`] that knew every width and no outline had nothing for
/// a shaper to work on. [`Font::program`] is the entry that changed.
///
/// # Four fonts, four ways back from a glyph to a code
///
/// Shaping answers in glyphs and a content stream carries codes, so what
/// decides whether a font can take the shaped path is whether its glyphs can
/// be read **backwards** into the codes that draw them. [`Path`] is that
/// answer per font:
///
/// - **A composite font over an sfnt, horizontal** — milestone 8 as it
///   shipped. The glyph goes back through `/CIDToGIDMap` to a CID
///   ([`Font::cid_for_gid`]) and the CID back through the encoding CMap to a
///   code ([`Font::code_for_cid`]), which gathers every code the CMap's own
///   tables could have meant and returns the first that maps **back**. That
///   covers `/Identity-H`, every embedded CMap stream, and — where this build
///   compiled the tables in — every registry CMap of 9.7.5.2.
/// - **The same under a vertical CMap** (9.7.4.3). The glyphs are found the
///   same way; what differs is where they go. The pen advances *down* by each
///   CID's own `/W2` displacement, so the value is written as a column at the
///   box's centre and every glyph sits where a reader's own vertical metrics
///   put it. `GSUB` runs `vert` and `vrt2` instead of the horizontal features
///   and no `GPOS` runs at all, since every positioning feature this crate
///   applies by default is horizontal.
/// - **A simple TrueType font.** A byte names at most 256 glyphs, but which
///   256 is decidable: each code the encoding gives a character (9.6.6, read
///   by [`Font::char_drawn_by`]) reaches the glyph the program's `cmap` gives
///   that character, and that table inverted is the way back. A shaped line
///   whose every glyph is in it is written as codes with `GPOS`'s positions;
///   a line that needs a glyph no byte reaches — a ligature, a joined form —
///   keeps the single-byte path whole, which is exactly what it drew before.
/// - **A composite font over a bare CFF** (`/FontFile3 /CIDFontType0C` or
///   `/Type1C`). A CFF has no `cmap`, no `hmtx` and no `GSUB` or `GPOS`, and
///   `tinker_pdf_shape::Shaper` takes an sfnt; so the program is **wrapped**,
///   per line, in the smallest sfnt that answers the shaper's two questions —
///   a `cmap` from each character of the line to a glyph, through the font's
///   own `/ToUnicode` read backwards ([`Font::code_for_char`]), and an
///   `hmtx` from `/W` — with each wrapper glyph standing for one CID the
///   program really carries. Nothing joins, because a CFF carries nothing to
///   join with; what the path buys is the characters, at the advances a
///   reader will use, in UAX #9's visual order.
///
/// Still refused, by the single-byte path and a warning per character: a
/// simple font that is not TrueType or is symbolic, a vertical CMap over a
/// CFF, a CFF font with no `/ToUnicode`, and a vertical comb field.
struct Shapeable {
    /// The embedded program, decoded once per appearance rather than once per
    /// line: a multiline field would otherwise inflate a megabyte per row.
    program: Vec<u8>,
    /// Which way back from a glyph to a code.
    path: Path,
}

/// See [`Shapeable`].
enum Path {
    /// A composite font over an sfnt, written along a baseline.
    Composite,
    /// A composite font over an sfnt under a vertical CMap, written as a
    /// column.
    Vertical,
    /// A simple TrueType font: each glyph a byte reaches, and the lowest byte
    /// that reaches it.
    Simple(HashMap<u16, u8>),
    /// A composite font over a bare CFF, wrapped per line.
    Cff,
}

/// Whether a field's value can be shaped against its `/DA` font, and where it
/// cannot, whether that is worth saying out loud.
enum Shaping {
    /// It can, against this program.
    Yes(Shapeable),
    /// It cannot, and the single-byte path is the right answer for this font
    /// — a standard-14 font, a symbolic one, one that embeds nothing this
    /// build can shape against. Every character that path cannot write is
    /// still named (ruling 10); there is just nothing to say about the *font*
    /// beyond what the file already says.
    No,
    /// It cannot, and the reason is this **build** rather than this document.
    ///
    /// The registry's code-to-CID tables are a megabyte and live behind the
    /// `cmap-predefined` cargo feature, so a `--no-default-features` build
    /// can read a `UniJIS-UCS2-H` field's widths and codespaces and still
    /// have nothing to invert. That refusal is typed and named against the
    /// field, because a capability that quietly depends on a feature is the
    /// failure this repository already named once in PDF/A: a verdict that
    /// depends on a feature is not a verdict, and neither is a fill.
    Refused(WarningKind),
}

/// One glyph of a shaped line, in the thousandths of an em [`width_of`]
/// answers in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    /// The character code to write. Under `/Identity-H` this is the CID;
    /// under any other CMap it is whatever code maps to it (9.7.5.2); in a
    /// simple font it is the byte.
    code: u32,
    /// How many bytes the code occupies, from the CMap's codespace ranges
    /// (9.7.6.2), or one in a simple font.
    ///
    /// Carried per glyph rather than per font because one CMap may have both
    /// widths: `90ms-RKSJ-H` takes one byte for ASCII and two for kanji, and
    /// writing `<0041>` where `<41>` was meant mis-splits every code after
    /// it.
    bytes: u8,
    /// Where the glyph's origin sits along the line — rightwards along a
    /// baseline, downwards along a column — from the line's start.
    x: f64,
    /// Its origin off the baseline — 9.4.3's `Ts`. Always zero in a column.
    rise: f64,
}

/// What one line of a field's value shapes to.
struct ShapedLine {
    /// The glyphs, in the order they are **drawn** — left to right, whichever
    /// way the text reads, or top to bottom in a column.
    glyphs: Vec<Placed>,
    /// The line's whole advance, in thousandths of an em, which is what
    /// quadding and auto-sizing measure with: its width along a baseline, or
    /// its length down a column.
    advance: f64,
    /// Characters the face has no glyph for, or that the font's own
    /// `/CIDToGIDMap` cannot address. Each becomes a warning naming the field
    /// (ruling 10).
    unwritable: Vec<char>,
}

/// The `GSUB` features a column of text runs: the default set's two that are
/// not about horizontal setting, and the two vertical alternates.
const VERTICAL_GSUB: &[Tag] = &[
    Tag::new(b"locl"),
    Tag::new(b"ccmp"),
    Tag::new(b"vert"),
    Tag::new(b"vrt2"),
];

impl Shapeable {
    /// The font behind a `/DA`, if one of [`Path`]'s conditions holds, and
    /// the typed reason where one fails for a reason this build owns.
    fn of<R: Resolve + ?Sized>(doc: &R, dict: &Dict, font: &Font) -> Shaping {
        match font.kind() {
            FontKind::Type0 => {}
            FontKind::TrueType => return Self::simple(doc, font),
            _ => return Shaping::No,
        }
        // A composite font whose `/Encoding` names nothing 9.7.5.2 defines
        // has no codes to write at all. `font::read` has already said which
        // name it was (`PredefinedCMapUnknown`); repeating it here would only
        // double it, and the single-byte path still names every character.
        if !font.has_encoding_cmap() {
            return Shaping::No;
        }
        // 9.7.5.2: the registry defines this CMap and this build left its
        // code-to-CID table out, so there is nothing to invert. Declared
        // before the program is decoded, because the answer does not depend
        // on the program and a megabyte should not be inflated to reach it.
        if font.encoding_is_approximate() {
            // The name is what makes the warning actionable, and the
            // dictionary is where a name lives — `Font` models the CMap's
            // forward direction and has no name to give.
            return match doc.resolve_key(dict, doc.intern(b"Encoding")).as_name() {
                Some(name) => Shaping::Refused(WarningKind::PredefinedCMapApproximate(name)),
                // An embedded CMap stream that inherited approximateness up a
                // `usecmap` chain: real, and with no name of its own to
                // report.
                None => Shaping::No,
            };
        }
        let Some(program) = font
            .program()
            .and_then(|p| doc.stream_decoded(p.stream).ok())
        else {
            return Shaping::No;
        };
        // Parsed here and thrown away, so that a program which is neither an
        // sfnt nor a CFF declines now rather than per line.
        let path = if Sfnt::parse(&program).is_some() {
            if font.is_vertical() {
                Path::Vertical
            } else {
                Path::Composite
            }
        } else if Cff::parse(&program).is_some() && !font.is_vertical() && font.has_to_unicode() {
            Path::Cff
        } else {
            return Shaping::No;
        };
        Shaping::Yes(Shapeable { program, path })
    }

    /// A simple TrueType font, if it is one the way back can be built for.
    fn simple<R: Resolve + ?Sized>(doc: &R, font: &Font) -> Shaping {
        // A symbolic font's codes name the program's own glyphs (9.6.6.4)
        // rather than characters, so there is no character to shape.
        if font.is_symbolic() {
            return Shaping::No;
        }
        let Some(program) = font
            .program()
            .filter(|p| p.key == ProgramKey::FontFile2)
            .and_then(|p| doc.stream_decoded(p.stream).ok())
        else {
            return Shaping::No;
        };
        let Some(face) = Sfnt::parse(&program) else {
            return Shaping::No;
        };
        // Ascending codes, so a glyph two codes reach is written with the
        // lower — the answer cannot depend on anything but the file.
        let mut codes: HashMap<u16, u8> = HashMap::new();
        for code in 0..=u8::MAX {
            let Some(c) = font.char_drawn_by(code) else {
                continue;
            };
            if let Some(glyph) = face.glyph_for_char(c).filter(|g| *g != 0) {
                codes.entry(glyph).or_insert(code);
            }
        }
        Shaping::Yes(Shapeable {
            program,
            path: Path::Simple(codes),
        })
    }

    /// Whether this font writes a column rather than a line.
    fn vertical(&self) -> bool {
        matches!(self.path, Path::Vertical)
    }

    /// Shapes one line and places every glyph, in drawing order.
    ///
    /// `None` is a line the shaped path cannot write and the single-byte path
    /// should — only ever a simple font's line that needs a glyph no byte
    /// reaches. A line with no glyphs at all comes back empty rather than as
    /// `None`: an empty field value is not a failure.
    fn line(&self, font: &Font, text: &str) -> Option<ShapedLine> {
        match &self.path {
            Path::Cff => Some(self.cff_line(font, text)),
            Path::Composite | Path::Vertical => {
                let face = Sfnt::parse(&self.program)?;
                Some(self.place(font, &face, text, |glyph| {
                    if glyph == 0 {
                        return None;
                    }
                    font.code_for_cid(font.cid_for_gid(glyph)?)
                }))
            }
            Path::Simple(codes) => {
                let face = Sfnt::parse(&self.program)?;
                let line = self.place(font, &face, text, |glyph| {
                    codes.get(&glyph).map(|code| (u32::from(*code), 1))
                });
                line.unwritable.is_empty().then_some(line)
            }
        }
    }

    /// One line through a face, with `code_for` the way back from a glyph.
    ///
    /// # Two steps along a baseline, and they are the consumer's two steps
    ///
    /// `docs/design/shaping.md`'s pipeline ends with the caller reordering:
    /// the shaper returns **logical** order, UAX #9's rule L2 orders the runs,
    /// and a right-to-left run's glyphs are then walked backwards. Both happen
    /// here, which is what puts an Arabic value's last letter at the left of
    /// the box where a reader of the script expects it.
    ///
    /// A column is not reordered: the text runs top to bottom in the order it
    /// was written, which is what vertical setting is.
    ///
    /// Glyph 0 is `.notdef`, and every `code_for` here refuses it rather than
    /// writing it: it is the face saying it has nothing for that character,
    /// and drawing the empty box while reporting success is the invisible
    /// failure ruling 10 exists to prevent.
    fn place(
        &self,
        font: &Font,
        face: &Sfnt<'_>,
        text: &str,
        code_for: impl Fn(u16) -> Option<(u32, u8)>,
    ) -> ShapedLine {
        let mut out = ShapedLine {
            glyphs: Vec::new(),
            advance: 0.0,
            unwritable: Vec::new(),
        };
        let upem = f64::from(face.units_per_em.max(1));
        let scale = |units: i32| f64::from(units) * 1000.0 / upem;
        let column = self.vertical();
        let shaper = if column {
            Shaper::new(face).with_features(VERTICAL_GSUB, &[])
        } else {
            Shaper::new(face)
        };
        let direction = if column {
            BaseDirection::LeftToRight
        } else {
            BaseDirection::Auto
        };
        let paragraph = Paragraph::new(text, direction);
        let runs = itemize(text, &paragraph);
        let shaped: Vec<_> = runs.iter().map(|run| shaper.shape(text, run)).collect();
        let levels: Vec<_> = runs.iter().map(|run| run.level).collect();
        let order: Vec<usize> = if column {
            (0..shaped.len()).collect()
        } else {
            reorder(&levels)
        };

        let mut pen = 0.0f64;
        for index in order {
            let Some(run) = shaped.get(index) else {
                continue;
            };
            let glyphs: Vec<ShapedGlyph> = if column || run.direction().is_forward() {
                run.glyphs().to_vec()
            } else {
                run.glyphs().iter().rev().copied().collect()
            };
            for glyph in glyphs {
                match code_for(glyph.glyph) {
                    Some((code, bytes)) => {
                        let (x, rise, advance) = if column {
                            // 9.7.4.3: `w1` is negative for text running
                            // down, and the column's length is its sum.
                            let (_, _, w1) = font.vertical_metrics(font.cid_of(code));
                            (pen, 0.0, -w1)
                        } else {
                            (
                                pen + scale(glyph.x_offset),
                                scale(glyph.y_offset),
                                scale(glyph.x_advance),
                            )
                        };
                        out.glyphs.push(Placed {
                            code,
                            bytes,
                            x,
                            rise,
                        });
                        pen += advance;
                    }
                    None => {
                        // The cluster is a byte offset into the text this run
                        // was shaped from, which is the line, so the
                        // character the reader typed is recoverable and is
                        // what the warning names.
                        let at = usize::try_from(glyph.cluster).unwrap_or(0);
                        if let Some(c) = text.get(at..).and_then(|rest| rest.chars().next()) {
                            if !out.unwritable.contains(&c) {
                                out.unwritable.push(c);
                            }
                        }
                        if !column {
                            pen += scale(glyph.x_advance);
                        }
                    }
                }
            }
        }
        out.advance = pen;
        out
    }

    /// One line against a bare CFF, through an sfnt wrapped around it.
    ///
    /// Each distinct character of the line is asked of `/ToUnicode` for the
    /// code that means it, the code is taken to a CID through the encoding,
    /// and the CID is kept only where the program carries it — a CID-keyed
    /// CFF's charset says, and a name-keyed one numbers its glyphs as CIDs.
    /// The wrapper gives each kept CID a glyph of its own, numbered from 1 in
    /// the order the line first meets them, and the way back is that table.
    fn cff_line(&self, font: &Font, text: &str) -> ShapedLine {
        let empty = || ShapedLine {
            glyphs: Vec::new(),
            advance: 0.0,
            unwritable: text.chars().fold(Vec::new(), |mut seen, c| {
                if !seen.contains(&c) {
                    seen.push(c);
                }
                seen
            }),
        };
        let Some(cff) = Cff::parse(&self.program) else {
            return empty();
        };
        // Glyph 0 is the wrapper's `.notdef` and stands for no CID.
        let mut cids: Vec<u32> = vec![0];
        let mut map: Vec<(char, u16, u16)> = Vec::new();
        for c in text.chars() {
            if map.iter().any(|(seen, _, _)| *seen == c) {
                continue;
            }
            let Some(code) = font.code_for_char(c) else {
                continue;
            };
            let cid = font.cid_of(code);
            let carried = if cff.is_cid() {
                cff.gid_for_cid(cid).is_some()
            } else {
                usize::try_from(cid).is_ok_and(|g| g < cff.glyph_count())
            };
            let Ok(glyph) = u16::try_from(cids.len()) else {
                break;
            };
            if !carried {
                continue;
            }
            // `/W` in thousandths of an em is the wrapper's `hmtx` at a
            // thousand units to the em, so what the shaper advances by is
            // exactly what a reader will.
            let advance = font
                .width_of(code)
                .0
                .round()
                .clamp(0.0, f64::from(u16::MAX)) as u16;
            cids.push(cid);
            map.push((c, glyph, advance));
        }
        let wrapped = wrap_for_shaping(&map);
        let Some(face) = Sfnt::parse(&wrapped) else {
            return empty();
        };
        self.place(font, &face, text, |glyph| {
            let cid = *cids.get(usize::from(glyph)).filter(|_| glyph != 0)?;
            font.code_for_cid(cid)
        })
    }
}

/// The smallest sfnt [`Shaper`] can shape against: `head` for the em, a
/// format 12 `cmap` from each character to its glyph, and `hhea` and `hmtx`
/// for the advances, with glyph 0 as an empty `.notdef`.
///
/// `map` is `(character, glyph, advance)`, glyphs numbered from 1 in order.
/// It carries no outline table at all: the shaper reads none, and nothing
/// draws from this — the appearance names the document's own font.
fn wrap_for_shaping(map: &[(char, u16, u16)]) -> Vec<u8> {
    let glyphs = map.len().saturating_add(1);
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    let metrics = u16::try_from(glyphs).unwrap_or(u16::MAX);
    hhea[34..36].copy_from_slice(&metrics.to_be_bytes());
    let mut hmtx = vec![0u8; 4];
    for (_, _, advance) in map {
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0u16.to_be_bytes());
    }
    let mut sorted: Vec<(u32, u16)> = map.iter().map(|(c, g, _)| (u32::from(*c), *g)).collect();
    sorted.sort_unstable();
    let groups = u32::try_from(sorted.len()).unwrap_or(0);
    let mut sub = Vec::new();
    sub.extend_from_slice(&12u16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&(16 + groups * 12).to_be_bytes());
    sub.extend_from_slice(&0u32.to_be_bytes());
    sub.extend_from_slice(&groups.to_be_bytes());
    for (c, g) in &sorted {
        sub.extend_from_slice(&c.to_be_bytes());
        sub.extend_from_slice(&c.to_be_bytes());
        sub.extend_from_slice(&u32::from(*g).to_be_bytes());
    }
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&3u16.to_be_bytes());
    cmap.extend_from_slice(&10u16.to_be_bytes());
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);

    let tables: [(&[u8; 4], &[u8]); 4] = [
        (b"cmap", &cmap),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&[0u8; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// Writes one shaped line as a text run along a baseline at `(x, y)`.
///
/// The `TJ` numbers are the difference between where the *reader's* pen will
/// be — which advances by `/W`, not by the shaper's own `hmtx` figure — and
/// where this crate put each glyph. Absorbing the difference glyph by glyph
/// rather than letting it accumulate is what `DocumentBuilder::glyph_run`
/// does for the same reason, and it is `docs/design/shaping.md`'s answer to
/// its own third risk: what was measured is what is drawn.
fn write_shaped(out: &mut Vec<u8>, font: &Font, line: &ShapedLine, x: f64, y: f64) {
    out.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
    let mut pen = 0.0f64;
    let mut rise = 0.0f64;
    let mut open = false;
    for placed in &line.glyphs {
        if placed.rise != rise {
            close_array(out, &mut open);
            rise = placed.rise;
            out.extend_from_slice(format!("{} Ts\n", number(rise / 1000.0)).as_bytes());
        }
        if !open {
            out.push(b'[');
            open = true;
        }
        // 9.4.3: a `TJ` number moves the pen **backwards** by that many
        // thousandths of a text space unit. Both sides here are already in
        // thousandths of an em, so the number is the gap outright.
        let adjust = pen - placed.x;
        if number(adjust) != "0" {
            if out.last() == Some(&b'>') {
                out.push(b' ');
            }
            out.extend_from_slice(number(adjust).as_bytes());
            out.push(b' ');
        }
        write_code(out, placed);
        pen = placed.x + font.width_of(placed.code).0;
    }
    close_array(out, &mut open);
    if rise != 0.0 {
        // `Ts` is graphics state and outlives the text object, so a run that
        // left one set would tilt whatever a viewer drew next.
        out.extend_from_slice(b"0 Ts\n");
    }
}

/// Writes one shaped column at `(x, y)`, the pen's start at the top.
///
/// No `TJ` numbers, by construction: every glyph was placed at the
/// displacement 9.7.4.3 gives its own CID, which is the one a reader will
/// advance by, so the pen and the placement cannot part.
fn write_column(out: &mut Vec<u8>, line: &ShapedLine, x: f64, y: f64) {
    out.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
    if line.glyphs.is_empty() {
        return;
    }
    out.push(b'[');
    for placed in &line.glyphs {
        write_code(out, placed);
    }
    out.extend_from_slice(b"] TJ\n");
}

/// One code as a hex string as wide as its codespace says.
///
/// 9.7.6.2: a code is as many bytes as its codespace range says, and the
/// string carries no separators — so the width is what tells a reader where
/// this code ends. Two hex digits per byte, zero-padded, whatever the code's
/// magnitude; `/Identity-H`'s two bytes are the common case rather than the
/// only one.
fn write_code(out: &mut Vec<u8>, placed: &Placed) {
    let digits = usize::from(placed.bytes).clamp(1, 4) * 2;
    let mask = u32::MAX >> (32 - digits * 4);
    out.extend_from_slice(
        format!("<{:0digits$X}>", placed.code & mask, digits = digits).as_bytes(),
    );
}

/// How a text field lays its value out.
///
/// A struct rather than five more parameters: the call had grown to eight,
/// which is the point at which the order of `bool, i64, Option<i64>` stops
/// being checkable by eye and a transposition compiles.
pub struct TextLayout<'a> {
    /// The default appearance string: font, size and colour.
    pub da: &'a [u8],
    /// `/Q`: 0 left, 1 centred, 2 right.
    pub quadding: i64,
    /// `/Ff` bit 13: the field wraps.
    pub multiline: bool,
    /// `/MaxLen`, when `/Ff` bit 25 makes the field a comb.
    pub comb: Option<i64>,
    /// The form's `/DR`, where the `/DA` finds its font.
    pub resources: Option<&'a Dict>,
    /// The field object this appearance is for, so that a character the font
    /// cannot draw is reported against something (ruling 10).
    ///
    /// `None` is a caller building an appearance outside a document's field
    /// tree — the tests below do it — and costs only the address on the
    /// warning, never the warning itself.
    pub field: Option<ObjRef>,
}

/// Builds the appearance stream for a text field's widget.
///
/// `rect` is the widget's rectangle; the stream is written in a box of the
/// same size anchored at the origin, which is what 12.5.5's mapping expects
/// and what every other producer writes.
#[must_use]
pub fn text_appearance(
    doc: &CosDocument,
    rect: Rect,
    value: &str,
    layout: &TextLayout<'_>,
) -> StreamData {
    text_appearance_in(doc, rect, value, layout)
}

/// [`text_appearance`] through a view.
///
/// The `/DA` font is found **through the view**, so an editor that has just
/// added a font to `/DR` — which is what
/// [`crate::edit::DocumentEditor::add_field`] does for `/Helv` when a form
/// has none — draws with its metrics rather than with the half-an-em guess a
/// font that cannot be found gets. It used to be read from the file, so a
/// field created and filled in one editor was laid out against a font the
/// file did not have yet.
///
/// What is still read from the file is the font's own subsidiary objects —
/// its descriptor, widths array and program — because [`font::read`] takes a
/// [`CosDocument`]. A standard-14 font, which is what a created field names,
/// has none of those; a composite font an editor added together with its
/// program is the case that remains (see `docs/features/forms.md`).
pub(crate) fn text_appearance_in<R: Resolve + ?Sized>(
    doc: &R,
    rect: Rect,
    value: &str,
    layout: &TextLayout<'_>,
) -> StreamData {
    let TextLayout {
        da,
        quadding,
        multiline,
        comb,
        resources,
        field,
    } = *layout;
    let (font_name, mut size, colour) = operators(da);
    let (w, h) = (rect.x1 - rect.x0, rect.y1 - rect.y0);

    let font_dict = resources
        .and_then(|dr| doc.resolve_key(dr, doc.intern(b"Font")).as_dict().cloned())
        .and_then(|fonts| fonts.get_ref(doc.intern(&font_name)))
        .and_then(|r| doc.get(r).ok())
        .and_then(|object| object.as_dict().cloned());
    let font = font_dict
        .as_ref()
        .map(|dict| font::read(doc.document(), dict));
    // Milestone 8: whether this field's value can be shaped and written as
    // glyphs rather than as bytes. `None` keeps every line on the single-byte
    // path this module has always had, which is still right for the `/Helv`
    // most forms name.
    let mut refusals: Vec<WarningKind> = Vec::new();
    let shapeable = match font_dict
        .as_ref()
        .zip(font.as_ref())
        .map_or(Shaping::No, |(dict, font)| Shapeable::of(doc, dict, font))
    {
        Shaping::Yes(shapeable) => Some(shapeable),
        Shaping::No => None,
        Shaping::Refused(kind) => {
            refusals.push(kind);
            None
        }
    };
    // A vertical comb would need its cells down the box rather than across
    // it, and 12.7.4.3 describes cells across; so a comb field keeps the
    // single-byte path under a vertical font, as it did before columns.
    let shapeable = shapeable.filter(|s| !(s.vertical() && comb.is_some_and(|n| n > 0)));
    let column = shapeable.as_ref().is_some_and(Shapeable::vertical);

    // 12.7.3.3: two units of padding on each side is the convention, and
    // matching it is what keeps a regenerated appearance from jumping.
    const PAD: f64 = 2.0;
    let inner_w = (w - PAD * 2.0).max(0.0);
    let inner_h = (h - PAD * 2.0).max(0.0);

    let lines: Vec<&str> = if multiline {
        value.split('\n').collect()
    } else {
        // A single-line field shows one line whatever the value contains.
        vec![value.lines().next().unwrap_or(value)]
    };

    // Shaped **once**, before auto-sizing, because auto-sizing measures the
    // same runs that are about to be drawn. Measuring one way and drawing
    // another is the two-paths-disagree failure `metrics.rs` warns about, and
    // a joined Arabic word is narrower than its letters by enough to see.
    //
    // A line the shaped path declines — only ever a simple font's, needing a
    // glyph no byte reaches — is `None` here and keeps the single-byte path.
    let shaped: Vec<Option<ShapedLine>> = lines
        .iter()
        .map(|line| {
            shapeable
                .as_ref()
                .zip(font.as_ref())
                .and_then(|(shapeable, font)| shapeable.line(font, line))
        })
        .collect();
    let widths: Vec<f64> = lines
        .iter()
        .zip(shaped.iter())
        .map(|(line, shaped)| match shaped {
            Some(shaped) => shaped.advance,
            None => width_of(font.as_ref(), line),
        })
        .collect();
    let mut unwritable: Vec<char> = Vec::new();
    for line in shaped.iter().flatten() {
        for c in &line.unwritable {
            if !unwritable.contains(c) {
                unwritable.push(*c);
            }
        }
    }

    if size <= 0.0 {
        // Auto-size. The height budget is what makes it legible; the width
        // budget is what keeps it inside the box.
        let widest = widths.iter().copied().fold(0.0f64, f64::max) / 1000.0;
        size = if column {
            // A column's length is its advance and its breadth is an em, so
            // the two budgets swap axes: the longest column must fit the
            // height, and the columns side by side the width.
            let by_length = if widest > 0.0 {
                inner_h / widest
            } else {
                inner_h
            };
            let by_breadth = inner_w / (lines.len() as f64).max(1.0) / 1.15;
            by_length.min(by_breadth).clamp(1.0, 12.0)
        } else {
            let by_height = if multiline {
                inner_h / (lines.len() as f64).max(1.0) / 1.15
            } else {
                inner_h * 0.72
            };
            let by_width = if widest > 0.0 {
                inner_w / widest
            } else {
                by_height
            };
            by_height.min(by_width).clamp(1.0, 12.0)
        };
    }

    let leading = size * 1.15;
    let mut content = Vec::new();
    // 12.7.4.3: the marked-content pair is how a viewer recognises a
    // regenerated field appearance as its own rather than as page content.
    content.extend_from_slice(b"/Tx BMC\nq\n");
    // Clipped to the box, so an over-long value is cut off rather than
    // spilling across the page.
    content.extend_from_slice(format!("{PAD} {PAD} {inner_w:.2} {inner_h:.2} re W n\n").as_bytes());
    content.extend_from_slice(b"BT\n");
    content.extend_from_slice(b"/");
    content.extend_from_slice(&font_name);
    content.extend_from_slice(format!(" {size:.2} Tf\n").as_bytes());
    content.extend_from_slice(&colour);
    content.push(b'\n');

    // 12.7.4.3: a comb field divides its width into /MaxLen equal cells and
    // centres one character in each. Laid out as ordinary text it drifts out
    // of the printed boxes it exists to sit inside, character by character,
    // which is the whole reason the flag is there.
    if let Some(cells) = comb.filter(|n| *n > 0) {
        let cell = inner_w / cells as f64;
        let line = lines.first().copied().unwrap_or("");
        for (index, ch) in line.chars().take(cells as usize).enumerate() {
            let mut text = String::new();
            text.push(ch);
            // Each cell is shaped on its own, and that is the right answer
            // rather than a shortcut: a comb field draws one character per
            // printed box, so a joining script's letters are in the isolated
            // form in one whatever their neighbours are.
            let cell_shaped = shapeable
                .as_ref()
                .zip(font.as_ref())
                .and_then(|(shapeable, font)| shapeable.line(font, &text));
            let width = cell_shaped
                .as_ref()
                .map_or_else(|| width_of(font.as_ref(), &text), |line| line.advance)
                / 1000.0
                * size;
            // Centred in its cell, which is what makes the column line up
            // whatever the character is.
            let x = PAD + cell * index as f64 + (cell - width) / 2.0;
            let y = (h - size * 0.72) / 2.0;

            match (&cell_shaped, font.as_ref()) {
                (Some(cell_shaped), Some(font)) => {
                    for c in &cell_shaped.unwritable {
                        if !unwritable.contains(c) {
                            unwritable.push(*c);
                        }
                    }
                    write_shaped(&mut content, font, cell_shaped, x, y);
                }
                _ => {
                    content.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
                    content.push(b'(');
                    escape(&mut content, &text, &mut unwritable);
                    content.extend_from_slice(b") Tj\n");
                }
            }
        }

        content.extend_from_slice(b"ET\nQ\nEMC\n");
        report(doc.document(), field, &refusals, &unwritable);
        return finish_appearance(doc.document(), content, w, h, resources);
    }

    if column {
        for (index, line) in shaped.iter().enumerate() {
            let Some(line) = line else {
                continue;
            };
            let length = line.advance / 1000.0 * size;
            // Columns run right to left, the first at the right, which is
            // how vertical text is set; a single line is centred across the
            // box. A glyph is drawn displaced by minus its position vector
            // (9.7.4.3), whose horizontal half is half its width, so a pen on
            // the column's centre line centres every glyph on it.
            let x = if multiline {
                PAD + inner_w - (index as f64 + 0.5) * leading
            } else {
                w / 2.0
            };
            // /Q, read down the column: 0 starts at the top, 1 centres the
            // column in the box, 2 ends it at the bottom.
            let top = match quadding {
                1 => (h + length) / 2.0,
                2 => PAD + length,
                _ => h - PAD,
            }
            .min(h - PAD);
            write_column(&mut content, line, x, top);
        }
        content.extend_from_slice(b"ET\nQ\nEMC\n");
        report(doc.document(), field, &refusals, &unwritable);
        return finish_appearance(doc.document(), content, w, h, resources);
    }

    for (index, line) in lines.iter().enumerate() {
        let line_width = widths.get(index).copied().unwrap_or(0.0) / 1000.0 * size;
        // 12.7.4.3: /Q is 0 left, 1 centred, 2 right.
        let x = match quadding {
            1 => PAD + (inner_w - line_width) / 2.0,
            2 => PAD + inner_w - line_width,
            _ => PAD,
        }
        .max(PAD);

        let y = if multiline {
            // Multiline runs from the top down.
            rect_top_baseline(inner_h, size) - index as f64 * leading + PAD
        } else {
            // A single line is centred vertically, which is what a viewer
            // does and what makes a regenerated field sit where it did.
            (h - size * 0.72) / 2.0
        };

        match shaped
            .get(index)
            .and_then(Option::as_ref)
            .zip(font.as_ref())
        {
            Some((shaped, font)) => write_shaped(&mut content, font, shaped, x, y),
            None => {
                content.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
                content.push(b'(');
                escape(&mut content, line, &mut unwritable);
                content.extend_from_slice(b") Tj\n");
            }
        }
    }

    content.extend_from_slice(b"ET\nQ\nEMC\n");
    report(doc.document(), field, &refusals, &unwritable);
    finish_appearance(doc.document(), content, w, h, resources)
}

/// What this appearance could not do, against the field it could not do it to
/// (ruling 10).
///
/// Two kinds, and the order is the order a reader wants them in: the
/// **refusal** first, because it is the cause and it names the font or the
/// build, then one warning per character the appearance could not draw.
///
/// Distinct characters and not occurrences: a value of two hundred Devanagari
/// letters in a Latin face is one problem with one fix, and two hundred
/// warnings would spend the sink's whole budget saying so.
///
/// Both carry the field as their object, which is what makes a
/// `cmap-predefined`-off build's `predefined-cmap-approximate` here
/// distinguishable from the one `font::read` emits when the same font is
/// merely *read*: this one names a field, and it means a fill was refused.
fn report(doc: &CosDocument, field: Option<ObjRef>, refusals: &[WarningKind], unwritable: &[char]) {
    if refusals.is_empty() && unwritable.is_empty() {
        return;
    }
    let mut sink = WarningSink::new();
    sink.set_context(field);
    for kind in refusals {
        sink.warn(0, *kind);
    }
    for c in unwritable {
        sink.warn(
            0,
            WarningKind::FieldCharacterUnrepresentable { character: *c },
        );
    }
    doc.absorb(sink);
}

/// Wraps a field's operators in the form XObject that carries them.
fn finish_appearance(
    doc: &CosDocument,
    content: Vec<u8>,
    w: f64,
    h: f64,
    resources: Option<&Dict>,
) -> StreamData {
    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(doc.intern(b"XObject")));
    dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Form")));
    dict.insert(
        doc.intern(b"BBox"),
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Real(w),
            Object::Real(h),
        ]),
    );
    dict.insert(
        doc.intern(b"Matrix"),
        Object::Array(vec![
            Object::Int(1),
            Object::Int(0),
            Object::Int(0),
            Object::Int(1),
            Object::Int(0),
            Object::Int(0),
        ]),
    );
    if let Some(dr) = resources {
        dict.insert(Name::RESOURCES, Object::Dict(dr.clone()));
    }

    StreamData {
        dict,
        data: content,
    }
}

/// Where the first baseline of a multiline field sits.
fn rect_top_baseline(inner_h: f64, size: f64) -> f64 {
    (inner_h - size * 0.85).max(0.0)
}

/// The rectangle of a widget annotation.
#[must_use]
pub fn widget_rect(doc: &CosDocument, widget: ObjRef) -> Option<Rect> {
    widget_rect_in(doc, widget)
}

/// [`widget_rect`] through a view.
pub(crate) fn widget_rect_in<R: Resolve + ?Sized>(doc: &R, widget: ObjRef) -> Option<Rect> {
    let object = doc.get(widget).ok()?;
    let dict = object.as_dict()?;
    doc.resolve_key(dict, doc.intern(b"Rect"))
        .as_array()
        .and_then(Rect::from_array)
        .filter(|r| !r.is_empty())
}

/// The `/DA` a widget should use: its own, its field's, or the form's.
#[must_use]
pub fn appearance_string(doc: &CosDocument, field: &form::Field, widget: ObjRef) -> Vec<u8> {
    appearance_string_in(doc, field, widget)
}

/// [`appearance_string`] through a view.
pub(crate) fn appearance_string_in<R: Resolve + ?Sized>(
    doc: &R,
    field: &form::Field,
    widget: ObjRef,
) -> Vec<u8> {
    if let Ok(object) = doc.get(widget) {
        if let Some(da) = object
            .as_dict()
            .and_then(|d| d.get(doc.intern(b"DA")))
            .and_then(Object::as_string)
        {
            return da.bytes.clone();
        }
    }
    field
        .default_appearance
        .clone()
        .unwrap_or_else(|| b"/Helv 0 Tf 0 g".to_vec())
}

/// A text field's quadding, inherited from the form when it says nothing.
#[must_use]
pub fn quadding(doc: &CosDocument, field: &form::Field) -> i64 {
    quadding_in(doc, field)
}

/// [`quadding`] through a view.
pub(crate) fn quadding_in<R: Resolve + ?Sized>(doc: &R, field: &form::Field) -> i64 {
    doc.get(field.reference)
        .ok()
        .and_then(|o| o.as_dict().and_then(|d| d.get_int(doc.intern(b"Q"))))
        .or_else(|| form::acro_form_in(doc).and_then(|f| f.get_int(doc.intern(b"Q"))))
        .unwrap_or(0)
}

/// 12.7.4.3 table 231: a text field that wraps.
pub const MULTILINE: i64 = 1 << 12;
/// A text field whose characters sit in fixed cells.
pub const COMB: i64 = 1 << 24;

/// Whether a value is one the field will accept from a user.
///
/// Refusing is better than truncating silently: a form filled with a value the
/// field rejects is a data error, and writing half of it hides that.
#[must_use]
pub fn accepts(field: &form::Field, value: &str) -> bool {
    if field.is_read_only() {
        return false;
    }
    accepts_value(field, value)
}

/// Whether a value fits the field, leaving aside who is writing it.
///
/// 12.7.4.1 table 227 says ReadOnly means the field "shall not be modified by
/// **the user**", and a calculate action is the document's own script rather
/// than a user — a calculated total is read-only precisely so that nothing
/// *but* the script writes it. So the read-only test lives in [`accepts`],
/// which is the user's door, and everything that is true of a value whoever
/// writes it lives here, where both doors share it. Splitting it any other way
/// gives the calculation path a second copy of the rules to drift from.
#[must_use]
pub fn accepts_value(field: &form::Field, value: &str) -> bool {
    match field.kind {
        FieldKind::Text => field
            .max_len
            .is_none_or(|max| max <= 0 || value.chars().count() as i64 <= max),
        FieldKind::ComboBox | FieldKind::ListBox => {
            // An editable combo takes anything; a list takes what it offers.
            const EDIT: i64 = 1 << 18;
            field.flags & EDIT != 0
                || field.options.is_empty()
                || field.options.iter().any(|o| o == value)
        }
        _ => false,
    }
}

/// Builds the `/V` object for a text or choice value, in a document declaring
/// PDF 1.7 or earlier.
///
/// [`value_object_in`] with the version fixed below 2.0, so the value is
/// never written in the UTF-8 form a 1.x reader cannot read.
#[must_use]
pub fn value_object(value: &str) -> Object {
    value_object_in(value, (1, 7))
}

/// Builds the `/V` object for a text or choice value, in a document declaring
/// PDF `version`.
///
/// 12.7.4.3 Table 229 makes a text field's value a text string, so it is
/// written by [`crate::text_string::encode_text_string`], the same writer
/// `/Info` and outline titles use: ASCII, and any value PDFDocEncoding
/// carries, as a literal; anything else behind a byte-order mark — UTF-16BE,
/// or UTF-8 when the document declares 2.0 or later.
#[must_use]
pub fn value_object_in(value: &str, version: (u8, u8)) -> Object {
    Object::String(crate::text_string::encode_text_string(value, version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_string::decode_text_string;

    fn doc() -> CosDocument {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n";
        CosDocument::open(bytes).expect("it opens")
    }

    fn rect() -> Rect {
        Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 20.0,
        }
    }

    fn text_of(stream: &StreamData) -> String {
        String::from_utf8_lossy(&stream.data).into_owned()
    }

    #[test]
    fn a_default_appearance_splits_into_font_size_and_colour() {
        let (font, size, colour) = operators(b"/Helv 12 Tf 0 0 1 rg");
        assert_eq!(font, b"Helv");
        assert_eq!(size, 12.0);
        assert_eq!(colour, b"0 0 1 rg", "the colour is replayed verbatim");
    }

    /// The operands of the `Tf` must not be replayed as though they were
    /// colour operators, which would push a stray number onto the stack.
    #[test]
    fn the_font_operands_are_not_replayed() {
        let (_, _, colour) = operators(b"0 g /Helv 9 Tf");
        assert_eq!(colour, b"0 g");
    }

    #[test]
    fn a_missing_or_broken_appearance_string_still_yields_something_usable() {
        let (font, size, colour) = operators(b"");
        assert_eq!(font, b"Helv");
        assert_eq!(size, 0.0, "which means auto-size");
        assert_eq!(colour, b"0 g");

        let (font, size, _) = operators(b"/ 1e999 Tf");
        assert_eq!(font, b"Helv", "an empty name falls back");
        assert!(size.is_finite());
    }

    #[test]
    fn an_appearance_is_marked_as_a_field_appearance() {
        let doc = doc();
        let stream = text_appearance(
            &doc,
            rect(),
            "Ada",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        );
        let content = text_of(&stream);
        assert!(content.starts_with("/Tx BMC"), "got: {content}");
        assert!(content.ends_with("EMC\n"));
        assert!(content.contains("(Ada) Tj"));
    }

    /// The box is anchored at the origin whatever the widget's rectangle is,
    /// because 12.5.5 maps it onto that rectangle.
    #[test]
    fn the_box_is_the_widgets_size_at_the_origin() {
        let doc = doc();
        let offset = Rect {
            x0: 300.0,
            y0: 400.0,
            x1: 400.0,
            y1: 420.0,
        };
        let stream = text_appearance(
            &doc,
            offset,
            "x",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        );
        let bbox: Vec<f64> = stream
            .dict
            .get_array(doc.intern(b"BBox"))
            .expect("a box")
            .iter()
            .filter_map(Object::as_number)
            .collect();
        assert_eq!(bbox, vec![0.0, 0.0, 100.0, 20.0]);
    }

    #[test]
    fn the_value_is_clipped_to_the_box() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "a very long value indeed",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert!(content.contains(" re W n"), "a clip is set: {content}");
    }

    #[test]
    fn quadding_moves_the_line() {
        let doc = doc();
        let left = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let centre = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 1,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let right = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 2,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));

        let x = |content: &str| -> f64 {
            content
                .lines()
                .find(|l| l.ends_with(" Tm"))
                .and_then(|l| l.split_whitespace().nth(4))
                .and_then(|v| v.parse().ok())
                .expect("a text matrix")
        };
        assert!(x(&left) < x(&centre), "centred sits right of left");
        assert!(x(&centre) < x(&right), "and right of centred");
    }

    #[test]
    fn a_multiline_field_writes_a_line_each() {
        let doc = doc();
        let tall = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 60.0,
        };
        let content = text_of(&text_appearance(
            &doc,
            tall,
            "one\ntwo\nthree",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: true,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 3);
        assert!(content.contains("(one)") && content.contains("(three)"));
    }

    #[test]
    fn a_single_line_field_shows_one_line_of_a_multiline_value() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "one\ntwo",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 1);
        assert!(content.contains("(one)") && !content.contains("(two)"));
    }

    /// Size zero means auto, and it has to come out as a real number a
    /// tokenizer accepts rather than a zero that draws nothing.
    #[test]
    fn an_auto_sized_field_picks_a_real_size() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "Ada",
            &TextLayout {
                da: b"/Helv 0 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let size: f64 = content
            .lines()
            .find(|l| l.ends_with(" Tf"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .expect("a font size");
        assert!(size > 0.0 && size <= 12.0, "got {size}");
    }

    /// A value far too long for its box must shrink rather than run out of it.
    #[test]
    fn a_long_auto_sized_value_shrinks_to_fit() {
        let doc = doc();
        let short = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 0 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let long = text_of(&text_appearance(
            &doc,
            rect(),
            "a value far longer than the box it has to fit inside of",
            &TextLayout {
                da: b"/Helv 0 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let size = |content: &str| -> f64 {
            content
                .lines()
                .find(|l| l.ends_with(" Tf"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
                .expect("a size")
        };
        assert!(size(&long) < size(&short), "the long value shrank");
    }

    #[test]
    fn parentheses_in_a_value_are_escaped() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "a (b) \\ c",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert!(content.contains("(a \\(b\\) \\\\ c) Tj"), "got: {content}");
    }

    /// 12.7.4.3: a comb field divides its width into /MaxLen cells and centres
    /// one character in each. Laid out as ordinary text it drifts out of the
    /// printed boxes it exists to sit inside, character by character.
    #[test]
    fn a_comb_field_spreads_its_characters_across_cells() {
        let doc = doc();
        let wide = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 20.0,
        };
        let content = text_of(&text_appearance(
            &doc,
            wide,
            "ABCD",
            &TextLayout {
                da: b"/Helv 10 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: Some(4),
                resources: None,
                field: None,
            },
        ));

        let xs: Vec<f64> = content
            .lines()
            .filter(|l| l.ends_with(" Tm"))
            .filter_map(|l| l.split_whitespace().nth(4)?.parse().ok())
            .collect();

        assert_eq!(xs.len(), 4, "one placement per character: {content}");
        for pair in xs.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (40.0..=58.0).contains(&gap),
                "cells are the field width over /MaxLen apart, got {gap}"
            );
        }
    }

    /// More characters than cells cannot be laid out, so the overflow is
    /// dropped rather than drawn outside the last box.
    #[test]
    fn a_comb_field_stops_at_its_cell_count() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "ABCDEFGH",
            &TextLayout {
                da: b"/Helv 10 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: Some(3),
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 3);
    }

    /// The flag means nothing without /MaxLen, so the field lays out as
    /// ordinary text rather than as one enormous cell.
    #[test]
    fn a_comb_field_without_maxlen_lays_out_normally() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "ABCD",
            &TextLayout {
                da: b"/Helv 10 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 1, "one run, not four");
    }

    /// A value comes back as the text it was: PDFDocEncoded where that
    /// carries it, and behind a byte-order mark where it does not.
    ///
    /// This used to assert that *any* non-ASCII value was UTF-16, which was
    /// this function's own rule; the rule is now the shared text-string
    /// writer's, and "naïve" is three PDFDocEncoding bytes short of needing a
    /// mark. What the assertion protected -- that the value reads back -- is
    /// asserted for every form directly.
    #[test]
    fn non_ascii_values_are_written_in_a_form_that_reads_back() {
        let Object::String(ascii) = value_object("plain") else {
            panic!("a string");
        };
        assert!(!ascii.hex && ascii.bytes == b"plain");

        let Object::String(latin) = value_object("naïve") else {
            panic!("a string");
        };
        assert_eq!(latin.bytes, b"na\xEFve", "PDFDocEncoding carries it");
        assert_eq!(decode_text_string(&latin.bytes), "naïve");

        let Object::String(wide) = value_object("日本") else {
            panic!("a string");
        };
        assert_eq!(&wide.bytes[..2], &[0xFE, 0xFF], "a byte-order mark");
        assert_eq!(decode_text_string(&wide.bytes), "日本");

        let Object::String(utf8) = value_object_in("日本", (2, 0)) else {
            panic!("a string");
        };
        assert_eq!(&utf8.bytes[..3], &[0xEF, 0xBB, 0xBF], "2.0's mark");
        assert_eq!(decode_text_string(&utf8.bytes), "日本");
    }

    #[test]
    fn a_value_over_maxlen_is_refused() {
        let field = form::Field {
            reference: ObjRef::new(1, 0),
            name: "x".to_string(),
            kind: FieldKind::Text,
            value: form::FieldValue::None,
            default: form::FieldValue::None,
            flags: 0,
            widgets: Vec::new(),
            options: Vec::new(),
            max_len: Some(3),
            default_appearance: None,
            scripts: form::FieldScripts::default(),
        };
        assert!(accepts(&field, "abc"));
        assert!(
            !accepts(&field, "abcd"),
            "silently truncating hides an error"
        );
    }

    #[test]
    fn a_read_only_field_is_refused() {
        let field = form::Field {
            reference: ObjRef::new(1, 0),
            name: "x".to_string(),
            kind: FieldKind::Text,
            value: form::FieldValue::None,
            default: form::FieldValue::None,
            flags: 1,
            widgets: Vec::new(),
            options: Vec::new(),
            scripts: form::FieldScripts::default(),
            max_len: None,
            default_appearance: None,
        };
        assert!(!accepts(&field, "anything"));
    }
}
