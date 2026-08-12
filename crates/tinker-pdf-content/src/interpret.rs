//! The content-stream interpreter (8.2, 9.4).
//!
//! Operands accumulate on a stack until an operator consumes them. An operator
//! that arrives with the wrong operand count is skipped and the stack cleared,
//! which is what keeps one malformed instruction from desynchronizing the rest
//! of the page.

use crate::device::{Device, Glyph, ImageRef, PathSegment};
use crate::state::{GraphicsState, LineCap, LineJoin, Matrix, Rgb, TextRenderMode};
use crate::tokenizer::{Token, Tokenizer};

/// Everything the interpreter needs from a page's resources.
///
/// This is the seam that keeps the crate free of PDF dictionaries: the
/// interpreter knows *which* font or colour space a stream selected, and the
/// caller says what that name means. Every method has a default, so an
/// implementation supplies only what it has.
/// A form XObject the interpreter is about to run (8.10).
///
/// The bounding box travels with the content rather than being fetched
/// separately, and `form` returns this struct rather than a tuple, so that
/// adding it forced every implementor to supply it. A defaulted accessor
/// would have compiled everywhere and quietly clipped nothing — which is
/// exactly how `GlyphSource::image` once stopped drawing every image in every
/// document with the whole suite green.
#[derive(Clone, Debug)]
pub struct Form {
    /// The decoded content stream.
    pub content: Vec<u8>,
    /// `/Matrix`, mapping form space into the space that invoked it.
    pub matrix: Matrix,
    /// `/BBox` in form space, as `[x0, y0, x1, y1]`.
    ///
    /// 8.10.2 makes it required and says the form is clipped to it. `None`
    /// means the file omitted it, in which case there is nothing to clip to
    /// and the form is drawn unclipped rather than dropped.
    pub bbox: Option<[f64; 4]>,
}

/// An optional-content layer, as the resource seam resolved it (8.11).
///
/// The interpreter cannot decide this: `/OCProperties` lives in the catalog
/// and the groups are indirect objects, neither of which this crate can see.
/// So it asks, and gets back the two things it has any use for — whether to
/// paint, and what to call the layer if it does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// Whether the document's default configuration paints this layer.
    pub visible: bool,
    /// What to call it: the group's `/Name`, which is the string a viewer's
    /// layer panel shows, falling back to the resource name.
    ///
    /// Ruling 10 — a warning that cannot say *which* layer it hid is not
    /// actionable, and a hidden layer is exactly the leniency a reader cannot
    /// see for themselves.
    pub label: String,
}

pub trait FontSource {
    /// Splits a string into `(code, text, advance in 1/1000 em)`.
    fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)>;

    /// Whether the named font writes vertically.
    fn is_vertical(&self, font: &[u8]) -> bool {
        let _ = font;
        false
    }

    /// The vertical metrics of one code, in 1/1000 em: `(v_x, v_y, w1_y)`.
    ///
    /// 9.7.4.3. `w1_y` is the displacement — how far the pen moves along the
    /// column, negative because text space has y upward — and `(v_x, v_y)` is
    /// the position vector, which is where within its em box the glyph is
    /// drawn: a comma belongs in a different corner vertically than
    /// horizontally, and the offset is what puts it there.
    ///
    /// Only consulted when [`FontSource::is_vertical`] is true, and it has no
    /// default on purpose. A defaulted one would compile in every
    /// implementation that ever grows a vertical font and quietly advance
    /// every column by nothing — which is the shape of the bug this method
    /// exists to close, where the horizontal advance was applied downward
    /// because nothing else was on offer.
    fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64);

    /// A stable identifier for the named font.
    fn font_id(&self, font: &[u8]) -> u64 {
        let _ = font;
        0
    }

    /// A form XObject, when the interpreter should recurse into one.
    /// Returning `None` skips it.
    fn form(&self, name: &[u8]) -> Option<Form> {
        let _ = name;
        None
    }

    /// A Type 3 glyph procedure and the font's `/FontMatrix` (9.6.5).
    ///
    /// A Type 3 glyph is a content stream, not an outline: it can fill, stroke,
    /// draw images, do anything a page can. So it is *run*, the way a form
    /// XObject is, rather than handed to a glyph source that only knows how to
    /// return curves.
    ///
    /// `None` for every other kind of font, which is what keeps the ordinary
    /// path unchanged.
    fn type3_glyph(&self, font: &[u8], code: u32) -> Option<(Vec<u8>, Matrix)> {
        let _ = (font, code);
        None
    }

    /// The RGB a named colour space gives these components.
    ///
    /// The interpreter cannot know what `/CS0 0.2 0.9 0.1 scn` means — RGB, a
    /// separation, an indexed palette — because only the resource dictionary
    /// says. `None` leaves the colour unchanged, which is what a viewer does
    /// with a space it cannot resolve.
    fn resolve_color(&self, space: &[u8], components: &[f64]) -> Option<Rgb> {
        let _ = (space, components);
        None
    }

    /// The fill and stroke alphas an `/ExtGState` sets, if it sets them.
    fn ext_g_state_alpha(&self, name: &[u8]) -> Option<(Option<f64>, Option<f64>)> {
        let _ = name;
        None
    }

    /// `/BM` from a named external graphics state (11.3.5).
    ///
    /// Separate from the alphas rather than folded in with them because a
    /// dictionary may set either without the other, and `None` here has to
    /// mean "unchanged" rather than "Normal" — a `gs` that sets only `/ca`
    /// must not silently reset the blend mode.
    fn ext_g_state_blend(&self, name: &[u8]) -> Option<crate::state::BlendMode> {
        let _ = name;
        None
    }

    /// How many components a named colour space takes.
    ///
    /// Used to tell `scn`'s optional trailing pattern name from its numeric
    /// operands.
    fn color_components(&self, space: &[u8]) -> Option<usize> {
        let _ = space;
        None
    }

    /// The optional-content layer a name in the `/Properties` resource
    /// sub-dictionary selects (8.11.3.2).
    ///
    /// `None` means the name is not optional content at all — an ordinary
    /// `/Span` or `/Artifact` property list, or a name the resources do not
    /// define — and that is not the same as a hidden layer. Every `None`
    /// path here ends in content being painted, which is the direction a
    /// reader can recover from.
    fn optional_content(&self, name: &[u8]) -> Option<Layer> {
        let _ = name;
        None
    }

    /// The `/OC` entry on a form or image XObject (8.11.4.4).
    ///
    /// Separate from [`FontSource::optional_content`] because the two look in
    /// different dictionaries: one in `/Properties`, the other on the XObject
    /// itself.
    fn xobject_optional_content(&self, name: &[u8]) -> Option<Layer> {
        let _ = name;
        None
    }
}

/// How deep form XObjects may nest before recursion is refused (8.10).
const MAX_FORM_DEPTH: u32 = 16;

/// How many marked-content scopes may be open at once (14.6.2).
///
/// Past the cap a scope is not reported to the device at all, and `EMC`
/// balances against the same counter so the nesting the device sees stays
/// correct. Unreported means *visible*, which is the direction a runaway
/// stream should fail in: the alternative is a file that hides a page by
/// nesting `BDC` four thousand deep.
const MAX_MARKED_CONTENT_DEPTH: u32 = 4096;

/// Runs a content stream against a device.
pub fn interpret<D: Device, F: FontSource>(
    content: &[u8],
    initial: Matrix,
    device: &mut D,
    fonts: &F,
) {
    let mut interp = Interpreter {
        device,
        fonts,
        stack: Vec::new(),
        gs: GraphicsState::new(initial),
        saved: Vec::new(),
        text_matrix: Matrix::IDENTITY,
        line_matrix: Matrix::IDENTITY,
        path: Vec::new(),
        current: (0.0, 0.0),
        start: (0.0, 0.0),
        depth: 0,
        pending_clip: None,
        marked: 0,
        marked_over_cap: 0,
    };
    interp.run(content);
    // 14.6.2: a stream may end with scopes still open, and a device left
    // inside one would treat everything after it as part of that layer.
    interp.close_open_marked_content();
}

struct Interpreter<'d, D: Device, F: FontSource> {
    device: &'d mut D,
    fonts: &'d F,
    stack: Vec<Token>,
    gs: GraphicsState,
    saved: Vec<GraphicsState>,
    text_matrix: Matrix,
    line_matrix: Matrix,
    path: Vec<PathSegment>,
    current: (f64, f64),
    start: (f64, f64),
    depth: u32,
    pending_clip: Option<bool>,
    /// Marked-content scopes open in the stream being run, and reported to
    /// the device (14.6.2).
    marked: u32,
    /// Scopes refused by [`MAX_MARKED_CONTENT_DEPTH`], so that an `EMC`
    /// closes the one it belongs to rather than an outer one.
    marked_over_cap: u32,
}

impl<D: Device, F: FontSource> Interpreter<'_, D, F> {
    fn run(&mut self, content: &[u8]) {
        let mut tokens = Tokenizer::new(content);

        while let Some(token) = tokens.next_token() {
            if self.device.is_cancelled() {
                return;
            }

            let Token::Operator(op) = token else {
                self.stack.push(token);
                // A runaway operand list means a stream with no operators;
                // bound it rather than accumulate forever.
                if self.stack.len() > 512 {
                    self.stack.remove(0);
                }
                continue;
            };

            match op.as_slice() {
                b"BI" => {
                    // 8.9.7: an inline image's data is not tokenizable, so the
                    // span from here to `EI` is taken whole and handed to the
                    // device, which is the layer that can read a dictionary
                    // and run a filter chain.
                    let rest = tokens.rest();
                    let consumed = skip_inline_image(rest);
                    let at = tokens.position();
                    tokens.seek(at + consumed);

                    let span = rest.get(..consumed).unwrap_or(rest);
                    let (dict, data) = split_inline_image(span);
                    self.device.draw_image(
                        &ImageRef {
                            name: Vec::new(),
                            inline: true,
                            inline_dict: dict.to_vec(),
                            inline_data: data.to_vec(),
                        },
                        &self.gs,
                    );
                }
                _ => self.operator(&op),
            }
            self.stack.clear();
        }
    }

    fn num(&self, from_end: usize) -> Option<f64> {
        match self
            .stack
            .get(self.stack.len().checked_sub(from_end + 1)?)?
        {
            Token::Number(v) if v.is_finite() => Some(*v),
            _ => None,
        }
    }

    /// Reads `d`: a dash array and a phase (8.4.3.6).
    ///
    /// An array of all zeros, or one containing a negative, is invalid and
    /// means a solid line rather than an invisible one — the alternative is a
    /// stroke that vanishes, which reads as missing content.
    fn set_dashes(&mut self) {
        let phase = self.num(0).filter(|v| v.is_finite() && *v >= 0.0);
        let Some(phase) = phase else { return };

        // The array is the operand before the phase, and the tokenizer leaves
        // its elements on the stack between the brackets.
        let mut dashes: Vec<f64> = Vec::new();
        let mut seen_open = false;
        for token in self.stack.iter().rev().skip(1) {
            match token {
                Token::Number(v) if v.is_finite() && *v >= 0.0 => dashes.push(*v),
                Token::ArrayClose => {}
                Token::ArrayOpen => {
                    seen_open = true;
                    break;
                }
                _ => return,
            }
        }
        if !seen_open {
            return;
        }
        dashes.reverse();

        if dashes.iter().all(|v| *v == 0.0) {
            dashes.clear();
        }
        self.gs.dashes = dashes;
        self.gs.dash_phase = phase;
    }

    fn operator(&mut self, op: &[u8]) {
        match op {
            // 8.4.4 graphics state
            b"q" => {
                if self.saved.len() < 64 {
                    self.saved.push(self.gs.clone());
                    self.device.save_state();
                }
            }
            b"Q" => {
                if let Some(prev) = self.saved.pop() {
                    self.gs = prev;
                    self.device.restore_state();
                }
            }
            b"cm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    self.num(5),
                    self.num(4),
                    self.num(3),
                    self.num(2),
                    self.num(1),
                    self.num(0),
                ) {
                    let m = Matrix { a, b, c, d, e, f };
                    let next = m.then(&self.gs.ctm);
                    // A degenerate matrix voids everything after it, so keep
                    // the previous one and carry on.
                    if next.is_finite() {
                        self.gs.ctm = next;
                    }
                }
            }

            // 9.4.1 text objects
            b"BT" => {
                self.text_matrix = Matrix::IDENTITY;
                self.line_matrix = Matrix::IDENTITY;
                self.device.begin_text();
            }
            b"ET" => self.device.end_text(),

            // 9.3 text state
            b"Tf" => {
                if let (Some(Token::Name(name)), Some(size)) = (
                    self.stack.get(self.stack.len().wrapping_sub(2)).cloned(),
                    self.num(0),
                ) {
                    self.gs.text.font = Some(name);
                    self.gs.text.size = size;
                }
            }
            b"Tc" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.char_spacing = v;
                }
            }
            b"Tw" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.word_spacing = v;
                }
            }
            b"Tz" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.horizontal_scale = v / 100.0;
                }
            }
            b"TL" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.leading = v;
                }
            }
            b"Ts" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.rise = v;
                }
            }
            b"Tr" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.render_mode = TextRenderMode::from_i64(v as i64);
                }
            }

            // 9.4.2 text positioning
            b"Td" => {
                if let (Some(tx), Some(ty)) = (self.num(1), self.num(0)) {
                    self.line_matrix = Matrix::translate(tx, ty).then(&self.line_matrix);
                    self.text_matrix = self.line_matrix;
                }
            }
            b"TD" => {
                if let (Some(tx), Some(ty)) = (self.num(1), self.num(0)) {
                    self.gs.text.leading = -ty;
                    self.line_matrix = Matrix::translate(tx, ty).then(&self.line_matrix);
                    self.text_matrix = self.line_matrix;
                }
            }
            b"Tm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    self.num(5),
                    self.num(4),
                    self.num(3),
                    self.num(2),
                    self.num(1),
                    self.num(0),
                ) {
                    let m = Matrix { a, b, c, d, e, f };
                    if m.is_finite() {
                        self.line_matrix = m;
                        self.text_matrix = m;
                    }
                }
            }
            b"T*" => self.next_line(),

            // 9.4.3 text showing
            b"Tj" => {
                if let Some(Token::String(s)) = self.stack.last().cloned() {
                    self.show(&s);
                }
            }
            b"'" => {
                self.next_line();
                if let Some(Token::String(s)) = self.stack.last().cloned() {
                    self.show(&s);
                }
            }
            b"\"" => {
                if let (Some(aw), Some(ac)) = (self.num(2), self.num(1)) {
                    self.gs.text.word_spacing = aw;
                    self.gs.text.char_spacing = ac;
                }
                self.next_line();
                if let Some(Token::String(s)) = self.stack.last().cloned() {
                    self.show(&s);
                }
            }
            b"TJ" => self.show_array(),

            // 8.5.2 path construction, accumulated in user space and
            // transformed as it is built.
            b"m" => {
                if let (Some(x), Some(y)) = (self.num(1), self.num(0)) {
                    let p = self.gs.ctm.apply(x, y);
                    self.current = p;
                    self.start = p;
                    self.push_segment(PathSegment::MoveTo { x: p.0, y: p.1 });
                }
            }
            b"l" => {
                if let (Some(x), Some(y)) = (self.num(1), self.num(0)) {
                    let p = self.gs.ctm.apply(x, y);
                    self.current = p;
                    self.push_segment(PathSegment::LineTo { x: p.0, y: p.1 });
                }
            }
            b"c" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x3), Some(y3)) = (
                    self.num(5),
                    self.num(4),
                    self.num(3),
                    self.num(2),
                    self.num(1),
                    self.num(0),
                ) {
                    let a = self.gs.ctm.apply(x1, y1);
                    let b = self.gs.ctm.apply(x2, y2);
                    let c = self.gs.ctm.apply(x3, y3);
                    self.current = c;
                    self.push_segment(PathSegment::CurveTo {
                        x1: a.0,
                        y1: a.1,
                        x2: b.0,
                        y2: b.1,
                        x3: c.0,
                        y3: c.1,
                    });
                }
            }
            b"v" => {
                if let (Some(x2), Some(y2), Some(x3), Some(y3)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    let a = self.current;
                    let b = self.gs.ctm.apply(x2, y2);
                    let c = self.gs.ctm.apply(x3, y3);
                    self.current = c;
                    self.push_segment(PathSegment::CurveTo {
                        x1: a.0,
                        y1: a.1,
                        x2: b.0,
                        y2: b.1,
                        x3: c.0,
                        y3: c.1,
                    });
                }
            }
            b"y" => {
                if let (Some(x1), Some(y1), Some(x3), Some(y3)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    let a = self.gs.ctm.apply(x1, y1);
                    let c = self.gs.ctm.apply(x3, y3);
                    self.current = c;
                    self.push_segment(PathSegment::CurveTo {
                        x1: a.0,
                        y1: a.1,
                        x2: c.0,
                        y2: c.1,
                        x3: c.0,
                        y3: c.1,
                    });
                }
            }
            b"h" => {
                self.current = self.start;
                self.push_segment(PathSegment::Close);
            }
            b"re" => {
                if let (Some(x), Some(y), Some(w), Some(h)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    let p0 = self.gs.ctm.apply(x, y);
                    let p1 = self.gs.ctm.apply(x + w, y);
                    let p2 = self.gs.ctm.apply(x + w, y + h);
                    let p3 = self.gs.ctm.apply(x, y + h);
                    self.push_segment(PathSegment::MoveTo { x: p0.0, y: p0.1 });
                    self.push_segment(PathSegment::LineTo { x: p1.0, y: p1.1 });
                    self.push_segment(PathSegment::LineTo { x: p2.0, y: p2.1 });
                    self.push_segment(PathSegment::LineTo { x: p3.0, y: p3.1 });
                    self.push_segment(PathSegment::Close);
                    self.current = p0;
                    self.start = p0;
                }
            }

            // 8.5.3 path painting
            b"S" | b"s" => {
                if op == b"s" {
                    self.push_segment(PathSegment::Close);
                }
                let path = std::mem::take(&mut self.path);
                self.device.stroke_path(&path, &self.gs);
                self.apply_pending_clip(&path);
            }
            b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                let even_odd = op.ends_with(b"*");
                let path = std::mem::take(&mut self.path);
                self.device.fill_path(&path, &self.gs, even_odd);
                if matches!(op, b"B" | b"B*" | b"b" | b"b*") {
                    self.device.stroke_path(&path, &self.gs);
                }
                self.apply_pending_clip(&path);
            }
            b"n" => {
                let path = std::mem::take(&mut self.path);
                self.apply_pending_clip(&path);
            }
            // 8.5.4: W and W* mark the current path as the new clip, but it
            // does not take effect until the painting operator that ends the
            // path — which is why this only records the intent.
            b"W" => self.pending_clip = Some(false),
            b"W*" => self.pending_clip = Some(true),

            // 8.10 XObjects
            b"Do" => {
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    self.do_xobject(&name);
                }
            }

            // 8.4.3 line parameters. All of them reach the device: a stroke
            // drawn with the wrong cap, or without its dashes, is wrong in a
            // way that looks deliberate.
            b"w" => {
                if let Some(v) = self.num(0) {
                    if v.is_finite() && v >= 0.0 {
                        self.gs.line_width = v;
                    }
                }
            }
            b"J" => {
                if let Some(v) = self.num(0) {
                    self.gs.line_cap = LineCap::from_operand(v);
                }
            }
            b"j" => {
                if let Some(v) = self.num(0) {
                    self.gs.line_join = LineJoin::from_operand(v);
                }
            }
            b"M" => {
                if let Some(v) = self.num(0) {
                    // 8.4.3.5: a limit below 1 is meaningless — the ratio it
                    // bounds is never less than 1 — so it is ignored rather
                    // than turning every join into a bevel.
                    if v.is_finite() && v >= 1.0 {
                        self.gs.miter_limit = v;
                    }
                }
            }
            b"d" => self.set_dashes(),
            b"ri" | b"i" => {}

            // 8.6.8 colour. The device operators resolve immediately; the
            // named ones go through the resource seam.
            b"g" | b"G" => {
                if let Some(v) = self.num(0) {
                    let value = to_byte(v);
                    let color = Rgb {
                        r: value,
                        g: value,
                        b: value,
                    };
                    self.set_color(op == b"g", color, None);
                }
            }
            b"rg" | b"RG" => {
                if let (Some(r), Some(g), Some(b)) = (self.num(2), self.num(1), self.num(0)) {
                    let color = Rgb {
                        r: to_byte(r),
                        g: to_byte(g),
                        b: to_byte(b),
                    };
                    self.set_color(op == b"rg", color, None);
                }
            }
            b"k" | b"K" => {
                if let (Some(c), Some(m), Some(y), Some(k)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    self.set_color(op == b"k", cmyk_to_rgb(c, m, y, k), None);
                }
            }
            b"cs" | b"CS" => {
                let Some(Token::Name(space)) = self.stack.last().cloned() else {
                    return;
                };
                let fill = op == b"cs";
                // 8.6.8: selecting a space resets the colour to that space's
                // initial value, which is black in every device space.
                let initial = self
                    .fonts
                    .color_components(&space)
                    .and_then(|n| self.fonts.resolve_color(&space, &vec![0.0; n]))
                    .unwrap_or(Rgb::BLACK);
                self.set_color(fill, initial, Some(space));
            }
            b"sc" | b"SC" | b"scn" | b"SCN" => {
                let fill = op == b"sc" || op == b"scn";
                let components: Vec<f64> = self
                    .stack
                    .iter()
                    .filter_map(|t| match t {
                        Token::Number(v) if v.is_finite() => Some(*v),
                        _ => None,
                    })
                    .collect();
                let space = if fill {
                    self.gs.fill_space.clone()
                } else {
                    self.gs.stroke_space.clone()
                };

                // 8.6.8.2: `scn` may end with a pattern name, with or without
                // preceding components (an uncoloured pattern takes both).
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    // 8.7.3.2: those preceding components are the colour a
                    // `PaintType 2` pattern paints with — it supplies shape
                    // and nothing else — and they are measured in the
                    // underlying space of `[/Pattern base]`, which is the
                    // space this slot is already in. Dropping them, which is
                    // what happened before, left the slot holding whatever
                    // `cs` had reset it to and the pattern with no colour to
                    // take.
                    if !components.is_empty() {
                        if let Some(color) = space
                            .as_ref()
                            .and_then(|s| self.fonts.resolve_color(s, &components))
                        {
                            self.set_color(fill, color, None);
                        }
                    }
                    self.set_pattern(fill, name);
                    return;
                }
                if components.is_empty() {
                    return;
                }

                let resolved = match &space {
                    Some(space) => self.fonts.resolve_color(space, &components),
                    // Without a named space the component count is the only
                    // clue, and it is a reliable one.
                    None => Some(components_to_rgb(&components)),
                };
                if let Some(color) = resolved {
                    self.set_color(fill, color, space);
                }
            }

            // 8.4.5 external graphics state. Only the alphas are modelled;
            // the rest of an /ExtGState needs the resource dictionary.
            b"gs" => {
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    if let Some((fill, stroke)) = self.fonts.ext_g_state_alpha(&name) {
                        if let Some(alpha) = fill {
                            self.gs.fill_alpha = alpha.clamp(0.0, 1.0);
                        }
                        if let Some(alpha) = stroke {
                            self.gs.stroke_alpha = alpha.clamp(0.0, 1.0);
                        }
                    }
                    if let Some(blend) = self.fonts.ext_g_state_blend(&name) {
                        self.gs.blend = blend;
                    }
                }
            }

            // 8.7.4.2: `sh` paints a shading over the current clip.
            b"sh" => {
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    self.device.draw_shading(&name, &self.gs);
                }
            }

            // 14.6.2 marked content, and 8.11.3.2's use of it for optional
            // content. `BDC` is the only operator in the language that can
            // stop a page drawing, so it is the only one that looks at its
            // property list here.
            b"BDC" => {
                let layer = self
                    .optional_content_name()
                    .and_then(|name| self.fonts.optional_content(&name));
                self.open_marked_content(layer);
            }
            // 14.6.1: `BMC` has a tag and no property list, so it can name
            // no layer and always paints.
            b"BMC" => self.open_marked_content(None),
            b"EMC" => self.close_marked_content(),
            // 14.6.1's marked-content *points*, which mark a position rather
            // than open a scope, and 7.8.2's compatibility brackets. All
            // four do nothing here, and are named rather than left to the
            // arm below so that a reader can see the difference between `MP`
            // and `BMC` was considered: reading `DP` as a scope would leave
            // the device inside a layer with no `EMC` ever to close it.
            b"MP" | b"DP" | b"BX" | b"EX" => {}

            _ => {}
        }
    }

    /// The name a `BDC`'s `/OC` property list gave, if it gave one (14.6.2).
    ///
    /// `BDC` takes `tag properties`. Only the `/OC` tag selects optional
    /// content — every other tag, `/Span` and `/Artifact` and the structure
    /// element names, opens a scope that is structure and nothing else — and
    /// only the name form of the property list can reach a layer.
    ///
    /// **The inline form cannot, and there is nothing to reassemble.**
    /// 8.11.3.2 makes an `/OC` entry a reference to an optional content
    /// group or a membership dictionary; 7.3.10 puts indirect references in
    /// the file structure, where a content stream cannot write one. So an
    /// inline `<< … >>` property list names no group in any document, and
    /// this returns `None` for it — which means visible, the direction a
    /// parse failure must fail in.
    ///
    /// The scan that reassembled 7.8.2's flattened `DictOpen`/`DictClose`
    /// tokens to find the tag before the `<<` was written and then deleted:
    /// a deliberate defect injection could not make it change a single
    /// answer, because both it and this produce a visible scope for every
    /// inline list, well formed or not. `docs/plans/gaps/06-optional-content
    /// .md` asks for the reassembly; the plan is amended there instead.
    fn optional_content_name(&self) -> Option<Vec<u8>> {
        let Some(Token::Name(property)) = self.stack.last() else {
            return None;
        };
        match self.stack.iter().rev().nth(1) {
            Some(Token::Name(tag)) if tag.as_slice() == b"OC" => Some(property.clone()),
            _ => None,
        }
    }

    /// Opens a marked-content scope and tells the device whether to paint
    /// what follows.
    fn open_marked_content(&mut self, layer: Option<Layer>) {
        if self.marked >= MAX_MARKED_CONTENT_DEPTH {
            self.marked_over_cap = self.marked_over_cap.saturating_add(1);
            return;
        }
        self.marked += 1;
        match layer {
            Some(layer) if !layer.visible => {
                self.device.begin_marked_content(false, Some(&layer.label));
            }
            _ => self.device.begin_marked_content(true, None),
        }
    }

    /// Closes the innermost scope, if there is one.
    ///
    /// An `EMC` with nothing open is damage, and is dropped. Acting on it
    /// would let a form XObject — or a stray one halfway down a page — close
    /// a scope it never opened, which reaches out of the stream that wrote
    /// it and un-hides a layer somewhere else entirely.
    fn close_marked_content(&mut self) {
        if self.marked_over_cap > 0 {
            self.marked_over_cap -= 1;
            return;
        }
        if self.marked > 0 {
            self.marked -= 1;
            self.device.end_marked_content();
        }
    }

    /// Closes every scope the stream just run left open.
    fn close_open_marked_content(&mut self) {
        self.marked_over_cap = 0;
        while self.marked > 0 {
            self.marked -= 1;
            self.device.end_marked_content();
        }
    }

    /// Hands a recorded `W`/`W*` to the device, now that the path has ended.
    fn apply_pending_clip(&mut self, path: &[PathSegment]) {
        if let Some(even_odd) = self.pending_clip.take() {
            self.device.clip_path(path, &self.gs, even_odd);
        }
    }

    /// Applies a colour to the fill or stroke slot.
    fn set_color(&mut self, fill: bool, color: Rgb, space: Option<Vec<u8>>) {
        if fill {
            self.gs.fill_color = color;
            self.gs.fill_pattern = None;
            if let Some(space) = space {
                self.gs.fill_space = Some(space);
            }
        } else {
            self.gs.stroke_color = color;
            self.gs.stroke_pattern = None;
            if let Some(space) = space {
                self.gs.stroke_space = Some(space);
            }
        }
    }

    /// Records the pattern a `scn`/`SCN` selected (8.7.3).
    fn set_pattern(&mut self, fill: bool, name: Vec<u8>) {
        if fill {
            self.gs.fill_pattern = Some(name);
        } else {
            self.gs.stroke_pattern = Some(name);
        }
    }

    fn push_segment(&mut self, segment: PathSegment) {
        // A page can describe an unbounded path; cap it so one hostile stream
        // cannot exhaust memory.
        if self.path.len() < 1 << 20 {
            self.path.push(segment);
        }
    }

    fn next_line(&mut self) {
        let leading = self.gs.text.leading;
        self.line_matrix = Matrix::translate(0.0, -leading).then(&self.line_matrix);
        self.text_matrix = self.line_matrix;
    }

    fn do_xobject(&mut self, name: &[u8]) {
        let Some(form) = self.fonts.form(name) else {
            self.device.draw_image(
                &ImageRef {
                    name: name.to_vec(),
                    inline: false,
                    inline_dict: Vec::new(),
                    inline_data: Vec::new(),
                },
                &self.gs,
            );
            return;
        };

        if self.depth >= MAX_FORM_DEPTH {
            return;
        }
        let id = self.fonts.font_id(name);
        if !self.device.begin_form(id) {
            return;
        }

        // 8.10.2: a form's /Matrix maps its space into the current one, and
        // its content runs with the surrounding state saved.
        let saved_gs = self.gs.clone();
        let saved_stack = std::mem::take(&mut self.saved);
        let saved_path = std::mem::take(&mut self.path);
        let saved_text = self.text_matrix;
        let saved_line = self.line_matrix;
        // 14.6.2: marked content nests within one stream. A form that leaves
        // a scope open must not leave its caller inside it, and an `EMC` it
        // has no `BDC` for must not close one of the caller's — so the count
        // starts at zero here and is emptied below.
        let saved_marked = std::mem::replace(&mut self.marked, 0);
        let saved_over_cap = std::mem::replace(&mut self.marked_over_cap, 0);

        let combined = form.matrix.then(&self.gs.ctm);
        if combined.is_finite() {
            self.gs.ctm = combined;
        }

        // 8.10.2: the form is clipped to its bounding box, which is expressed
        // in form space — so with `/Matrix` already folded into the CTM, the
        // rectangle goes in unmodified. Without this a form that draws
        // outside its box paints across the rest of the page, and forms are
        // how most producers place repeated content, so it is not a rare
        // shape. `begin_form` saved the clip and `end_form` restores it.
        if let Some([x0, y0, x1, y1]) = form.bbox {
            if [x0, y0, x1, y1].iter().all(|v| v.is_finite()) && x1 > x0 && y1 > y0 {
                // A `Device` receives paths already in device space — `re`
                // and friends apply the CTM as they build them — so the
                // corners are transformed here too. Passing the rectangle in
                // form space instead clips somewhere else entirely, which
                // with an identity `/Matrix` happens to look almost right and
                // with any other matrix erases the form completely.
                let p0 = self.gs.ctm.apply(x0, y0);
                let p1 = self.gs.ctm.apply(x1, y0);
                let p2 = self.gs.ctm.apply(x1, y1);
                let p3 = self.gs.ctm.apply(x0, y1);
                let box_path = [
                    PathSegment::MoveTo { x: p0.0, y: p0.1 },
                    PathSegment::LineTo { x: p1.0, y: p1.1 },
                    PathSegment::LineTo { x: p2.0, y: p2.1 },
                    PathSegment::LineTo { x: p3.0, y: p3.1 },
                    PathSegment::Close,
                ];
                self.device.clip_path(&box_path, &self.gs, false);
            }
        }

        self.depth += 1;
        self.run(&form.content);
        self.depth -= 1;

        self.close_open_marked_content();
        self.marked = saved_marked;
        self.marked_over_cap = saved_over_cap;
        self.gs = saved_gs;
        self.saved = saved_stack;
        self.path = saved_path;
        self.text_matrix = saved_text;
        self.line_matrix = saved_line;
        self.device.end_form(id);
    }

    fn show_array(&mut self) {
        // TJ's operand is an array of strings and numbers; the numbers move
        // the pen backwards in thousandths of text space (9.4.3).
        let items: Vec<Token> = self
            .stack
            .iter()
            .rev()
            .take_while(|t| !matches!(t, Token::ArrayOpen))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // 9.4.3: the adjustment is subtracted from "the current horizontal or
        // vertical coordinate, depending on the writing mode" — so which axis
        // it moves along is a property of the font, not of the operator. A
        // vertical run whose adjustments went sideways would step out of its
        // own column, one kern at a time. `Tf` cannot appear inside a `TJ`
        // array, so the font is fixed for the whole of it.
        let vertical = self
            .gs
            .text
            .font
            .as_ref()
            .is_some_and(|name| self.fonts.is_vertical(name));

        for item in items {
            match item {
                Token::String(s) => self.show(&s),
                Token::Number(adjust) if adjust.is_finite() => {
                    let ts = &self.gs.text;
                    // 9.4.4's tx carries Th and its ty does not: Th is a
                    // horizontal scaling and there is no horizontal motion to
                    // scale in a vertical run.
                    self.text_matrix = if vertical {
                        let shift = -adjust / 1000.0 * ts.size;
                        Matrix::translate(0.0, shift).then(&self.text_matrix)
                    } else {
                        let shift = -adjust / 1000.0 * ts.size * ts.horizontal_scale;
                        Matrix::translate(shift, 0.0).then(&self.text_matrix)
                    };
                }
                _ => {}
            }
        }
    }

    fn show(&mut self, bytes: &[u8]) {
        let Some(font_name) = self.gs.text.font.clone() else {
            return;
        };
        let vertical = self.fonts.is_vertical(&font_name);
        let font_id = self.fonts.font_id(&font_name);

        for (code, text, width) in self.fonts.decode(&font_name, bytes) {
            let ts = self.gs.text.clone();

            // 9.4.4: the glyph is placed by the text matrix scaled by the font
            // size, horizontal scaling and rise.
            let scale = Matrix {
                a: ts.size * ts.horizontal_scale,
                b: 0.0,
                c: 0.0,
                d: ts.size,
                e: 0.0,
                f: ts.rise,
            };

            // 9.7.4.3: a vertical font displaces the pen by w1 rather than by
            // the horizontal w0, and draws the glyph at the current point
            // offset by *minus* the position vector v — v points from the
            // glyph's horizontal origin to the one vertical setting uses, so
            // undoing it is what puts the glyph in the column. Both come out
            // of one call, because a build where the advance and the placement
            // read different numbers is the failure 9.7.4.3 is easiest to get
            // half right about.
            let (v_x, v_y, w1_y) = if vertical {
                self.fonts.vertical_metrics(&font_name, code)
            } else {
                (0.0, 0.0, 0.0)
            };

            // v is in glyph space, so the offset goes *inside* the size
            // scaling — 9.4.4 sends the position vector through Trm like every
            // other glyph-space coordinate, which is what makes it scale with
            // the font size and with Th.
            let placed = if vertical {
                Matrix::translate(-v_x / 1000.0, -v_y / 1000.0).then(&scale)
            } else {
                scale
            };
            let transform = placed.then(&self.text_matrix).then(&self.gs.ctm);

            // 9.6.5: a Type 3 glyph is a content stream. Its procedure runs in
            // glyph space, which the font matrix maps into text space — so the
            // matrix goes *inside* the transform that places the glyph, not
            // outside it.
            if let Some((proc_bytes, font_matrix)) = self.fonts.type3_glyph(&font_name, code) {
                if self.depth < MAX_FORM_DEPTH {
                    let saved_gs = self.gs.clone();
                    let saved_text = self.text_matrix;
                    let saved_line = self.line_matrix;
                    // A glyph procedure is a content stream like a form's,
                    // and 14.6.2's scopes are per stream in the same way.
                    let saved_marked = std::mem::replace(&mut self.marked, 0);
                    let saved_over_cap = std::mem::replace(&mut self.marked_over_cap, 0);

                    self.gs.ctm = font_matrix.then(&transform);
                    self.depth += 1;
                    self.device.save_state();
                    let inner = proc_bytes.clone();
                    self.run(&inner);
                    self.close_open_marked_content();
                    self.device.restore_state();
                    self.depth -= 1;

                    self.marked = saved_marked;
                    self.marked_over_cap = saved_over_cap;
                    self.gs = saved_gs;
                    self.text_matrix = saved_text;
                    self.line_matrix = saved_line;
                }

                // The advance still comes from /Widths, which are in glyph
                // space and so scale by the font matrix rather than by 1/1000.
                let advance = width * font_matrix.a * ts.size * ts.horizontal_scale;
                let shift =
                    advance + (ts.char_spacing + word_spacing(code, &ts)) * ts.horizontal_scale;
                self.text_matrix = Matrix::translate(shift, 0.0).then(&self.text_matrix);
                continue;
            }

            let glyph = Glyph {
                code,
                text: text.clone(),
                transform,
                advance: if vertical { w1_y } else { width } / 1000.0 * ts.size,
                size: ts.size,
                vertical,
                font_id,
            };
            self.device.show_glyph(&glyph, &self.gs);

            // 9.4.4: word spacing applies to a single-byte code 32 only, which
            // is why it must not be added for a two-byte CID font.
            let word = if code == 32 && !vertical {
                ts.word_spacing
            } else {
                0.0
            };

            // 9.4.4, in full:
            //   tx = ((w0 - Tj/1000) * Tfs + Tc + Tw) * Th
            //   ty =  (w1 - Tj/1000) * Tfs + Tc + Tw
            // Two differences, and both of them matter. The displacement is w1
            // and not w0 — a downward number, already signed, so it is added
            // rather than subtracted and an engine that negates a negative
            // runs the column upward. And Th does not appear in ty at all:
            // horizontal scaling scales horizontal motion, of which a vertical
            // run has none.
            self.text_matrix = if vertical {
                let ty = w1_y / 1000.0 * ts.size + ts.char_spacing + word;
                Matrix::translate(0.0, ty).then(&self.text_matrix)
            } else {
                let tx = (width / 1000.0 * ts.size + ts.char_spacing + word) * ts.horizontal_scale;
                Matrix::translate(tx, 0.0).then(&self.text_matrix)
            };
        }
    }
}

fn to_byte(value: f64) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 8.6.4.4: the additive complement, with black applied.
fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> Rgb {
    let k = k.clamp(0.0, 1.0);
    Rgb {
        r: to_byte((1.0 - c.clamp(0.0, 1.0)) * (1.0 - k)),
        g: to_byte((1.0 - m.clamp(0.0, 1.0)) * (1.0 - k)),
        b: to_byte((1.0 - y.clamp(0.0, 1.0)) * (1.0 - k)),
    }
}

/// Reads components as a device colour, by how many there are.
///
/// `sc` without a preceding `cs` is malformed but occurs; the count is the
/// only signal available and it is an unambiguous one.
fn components_to_rgb(components: &[f64]) -> Rgb {
    let at = |i: usize| components.get(i).copied().unwrap_or(0.0);
    match components.len() {
        1 => {
            let v = to_byte(at(0));
            Rgb { r: v, g: v, b: v }
        }
        4 => cmyk_to_rgb(at(0), at(1), at(2), at(3)),
        _ => Rgb {
            r: to_byte(at(0)),
            g: to_byte(at(1)),
            b: to_byte(at(2)),
        },
    }
}

/// Finds the end of an inline image, returning how many bytes to skip.
///
/// 8.9.7 ends the data at `EI` surrounded by whitespace, and binary data can
/// contain those bytes, so the match is only accepted when what follows looks
/// like a content stream again.
/// 9.4.4: word spacing applies to a single-byte code 32 and to nothing else.
fn word_spacing(code: u32, ts: &crate::state::TextState) -> f64 {
    if code == 32 {
        ts.word_spacing
    } else {
        0.0
    }
}

/// Splits an inline image's span into its dictionary and its data (8.9.7).
///
/// `ID` is followed by exactly one whitespace byte before the data starts, and
/// the span ends at `EI`. A span with no `ID` at all is all dictionary and no
/// data, which is what a truncated stream leaves behind.
fn split_inline_image(span: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0usize;
    while i + 1 < span.len() {
        if span[i] == b'I' && span[i + 1] == b'D' {
            let before_ok =
                i == 0 || span[i - 1].is_ascii_whitespace() || is_delimiter(span[i - 1]);
            if before_ok {
                let dict = &span[..i];
                // One whitespace byte separates `ID` from the samples.
                let start = (i + 3).min(span.len());
                let end = span.len().saturating_sub(2).max(start);
                return (dict, &span[start..end]);
            }
        }
        i += 1;
    }
    (span, &[])
}

fn skip_inline_image(rest: &[u8]) -> usize {
    let mut i = 0usize;
    while i + 1 < rest.len() {
        if rest.get(i) == Some(&b'E') && rest.get(i + 1) == Some(&b'I') {
            let before_ok = i == 0
                || rest
                    .get(i - 1)
                    .is_some_and(|b| b.is_ascii_whitespace() || *b == 0);
            let after = rest.get(i + 2);
            let after_ok = after.is_none_or(|b| b.is_ascii_whitespace() || is_delimiter(*b));
            if before_ok && after_ok {
                return i + 2;
            }
        }
        i += 1;
    }
    rest.len()
}

fn is_delimiter(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        glyphs: Vec<(String, f64, f64)>,
        advances: Vec<f64>,
        fills: usize,
        strokes: usize,
    }

    impl Device for Recorder {
        fn show_glyph(&mut self, glyph: &Glyph, _state: &GraphicsState) {
            self.glyphs
                .push((glyph.text.clone(), glyph.transform.e, glyph.transform.f));
            self.advances.push(glyph.advance);
        }
        fn fill_path(&mut self, _path: &[PathSegment], _state: &GraphicsState, _even_odd: bool) {
            self.fills += 1;
        }
        fn stroke_path(&mut self, _path: &[PathSegment], _state: &GraphicsState) {
            self.strokes += 1;
        }
    }

    /// Every byte is one code, 500/1000 em wide, mapping to itself.
    struct Simple;

    impl FontSource for Simple {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        /// Never vertical, so nothing here is ever asked. Written out because
        /// the trait has no default for it: 9.7.4.3's metrics have no safe
        /// one, and a source that grows a vertical font is made to say so.
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
    }

    /// The same font written vertically, with metrics that are all different
    /// numbers.
    ///
    /// Every component is distinct and none is a multiple of another, so an
    /// assertion below can only hold for one arrangement of them: `w1_y` is
    /// -800 where the *horizontal* width is 500, so a pen still advancing by
    /// `w0` — the defect this closes — lands somewhere none of these tests
    /// accept, and so does one that negates the sign.
    ///
    /// `A` and `B` differ from each other as well, so a run of two glyphs
    /// cannot pass by applying either one's metrics twice.
    struct Vertical;

    impl Vertical {
        /// `(v_x, v_y, w1_y)` per code, in 1/1000 em.
        fn metrics(code: u32) -> (f64, f64, f64) {
            match code {
                0x41 => (250.0, 700.0, -800.0),
                0x42 => (300.0, 650.0, -900.0),
                _ => (0.0, 880.0, -1000.0),
            }
        }
    }

    impl FontSource for Vertical {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn is_vertical(&self, _font: &[u8]) -> bool {
            true
        }
        fn vertical_metrics(&self, _font: &[u8], code: u32) -> (f64, f64, f64) {
            Vertical::metrics(code)
        }
    }

    fn run(src: &[u8]) -> Recorder {
        let mut d = Recorder::default();
        interpret(src, Matrix::IDENTITY, &mut d, &Simple);
        d
    }

    fn run_vertical(src: &[u8]) -> Recorder {
        let mut d = Recorder::default();
        interpret(src, Matrix::IDENTITY, &mut d, &Vertical);
        d
    }

    #[test]
    fn glyphs_advance_by_their_width() {
        let d = run(b"BT /F0 10 Tf 0 0 Td (AB) Tj ET");
        let text: String = d.glyphs.iter().map(|g| g.0.as_str()).collect();
        assert_eq!(text, "AB");
        // 500/1000 em at 10pt is 5 points per glyph.
        assert_eq!(d.glyphs.first().map(|g| g.1), Some(0.0));
        assert_eq!(d.glyphs.get(1).map(|g| g.1), Some(5.0));
    }

    #[test]
    fn tj_adjustments_move_the_pen_backwards() {
        let d = run(b"BT /F0 10 Tf [(A) -1000 (B)] TJ ET");
        // -1000 thousandths at 10pt closes 10 points, so B lands at 5 + 10.
        assert_eq!(d.glyphs.get(1).map(|g| g.1), Some(15.0));
    }

    #[test]
    fn character_and_word_spacing_apply_where_the_spec_says() {
        let spaced = run(b"BT /F0 10 Tf 2 Tc (AB) Tj ET");
        assert_eq!(spaced.glyphs.get(1).map(|g| g.1), Some(7.0));

        // Word spacing applies to code 32 only.
        let worded = run(b"BT /F0 10 Tf 100 Tw (A B) Tj ET");
        let xs: Vec<f64> = worded.glyphs.iter().map(|g| g.1).collect();
        assert_eq!(xs.first(), Some(&0.0), "A");
        assert_eq!(xs.get(1), Some(&5.0), "the space itself is not shifted");
        assert_eq!(xs.get(2), Some(&110.0), "B follows the word space");
    }

    // -----------------------------------------------------------------------
    // Vertical writing (gap 05, 9.4.4 and 9.7.4.3).
    // -----------------------------------------------------------------------

    /// Absolute positions after two glyphs, which is the whole point of the
    /// assertion: an inverted vertical displacement still draws a tidy column,
    /// just one running upward, and a test on the *gaps* between glyphs would
    /// accept it. A test on where the glyphs actually are cannot.
    ///
    /// Every number below is hand-computed from 9.4.4 with the pen starting at
    /// (100, 700) and 10pt text:
    ///
    /// - `A` is drawn at the pen offset by -v = (-250, -700)/1000 x 10, so at
    ///   (100 - 2.5, 700 - 7) = (97.5, 693).
    /// - the pen then moves by w1 = -800/1000 x 10 = -8, to (100, 692).
    /// - `B` is drawn at that pen offset by its own -v = (-3, -6.5), so at
    ///   (97, 685.5).
    ///
    /// The horizontal width is 500 for both, so a pen still advancing by `w0`
    /// puts `B` at y = 688.5 and a sign inversion puts it at 701.5. Neither is
    /// 685.5.
    #[test]
    fn a_vertical_run_advances_by_w1_and_is_placed_by_v() {
        let d = run_vertical(b"BT /F0 10 Tf 100 700 Td (AB) Tj ET");

        assert_eq!(
            d.glyphs.first().map(|g| (g.1, g.2)),
            Some((97.5, 693.0)),
            "A, at the pen minus its position vector"
        );
        assert_eq!(
            d.glyphs.get(1).map(|g| (g.1, g.2)),
            Some((97.0, 685.5)),
            "B, one w1 further down and offset by its own v"
        );

        // The glyph event carries the same displacement, signed: a consumer
        // measuring a column reads it rather than recomputing it.
        assert_eq!(d.advances, vec![-8.0, -9.0]);
    }

    /// The position vector is the half of 9.7.4.3 that has nothing to do with
    /// the advance, so it is asserted on its own: two codes with the same
    /// displacement and different vectors sit in different places.
    ///
    /// `A` and `B` are drawn one after another here, and the difference in
    /// their x is `(250 - 300)/1000 x 10 = -0.5` — a sideways shift inside the
    /// em box, in a writing mode where nothing else moves sideways at all.
    #[test]
    fn the_position_vector_shifts_the_glyph_within_its_em_box() {
        let d = run_vertical(b"BT /F0 10 Tf 100 700 Td (AB) Tj ET");
        let xs: Vec<f64> = d.glyphs.iter().map(|g| g.1).collect();
        assert_eq!(xs, vec![97.5, 97.0]);
        assert!(
            xs.iter().all(|x| *x != 100.0),
            "neither glyph sits at the pen itself, which is what applying no \
             position vector would look like"
        );
    }

    /// 9.4.4's `ty` has no `Th` in it, and its `Tc` is added to a negative
    /// displacement rather than scaling it. `Tz 200` doubles the *glyph*
    /// horizontally — so the position vector's x doubles with it, since v is a
    /// glyph-space coordinate — and leaves the column's step alone.
    #[test]
    fn horizontal_scaling_does_not_reach_a_vertical_advance() {
        let d = run_vertical(b"BT /F0 10 Tf 2 Tc 200 Tz 100 700 Td (AB) Tj ET");

        assert_eq!(
            d.glyphs.first().map(|g| (g.1, g.2)),
            Some((95.0, 693.0)),
            "A: v_x doubled by Tz, v_y untouched by it"
        );
        // The step is -8 + 2 = -6, and 9.4.4 does not multiply it by Th.
        // Character spacing shortens a downward step because the spec adds it
        // to a negative number; scaling it by 2 would put B at y = 682.
        assert_eq!(
            d.glyphs.get(1).map(|g| (g.1, g.2)),
            Some((94.0, 687.5)),
            "B: one step of -6 down, not -12"
        );
    }

    /// 9.4.3: a `TJ` number is subtracted from "the current horizontal or
    /// vertical coordinate, depending on the writing mode". It went on the
    /// horizontal one whatever the mode, so a vertical run drifted sideways by
    /// every kern in it while the column itself never noticed.
    #[test]
    fn tj_adjustments_move_a_vertical_pen_along_its_own_column() {
        let d = run_vertical(b"BT /F0 10 Tf 100 700 Td [(A) -1000 (B)] TJ ET");

        // A is where it always was; the pen is then at (100, 692), and -1000
        // thousandths at 10pt subtracts -10 from y, taking it to 702.
        assert_eq!(d.glyphs.get(1).map(|g| (g.1, g.2)), Some((97.0, 695.5)));
        assert_eq!(
            d.glyphs.get(1).map(|g| g.1),
            Some(97.0),
            "and the column has not moved sideways, which is what the \
             adjustment used to do"
        );
    }

    #[test]
    fn horizontal_scaling_multiplies_the_advance() {
        let d = run(b"BT /F0 10 Tf 50 Tz (AB) Tj ET");
        assert_eq!(d.glyphs.get(1).map(|g| g.1), Some(2.5));
    }

    #[test]
    fn td_and_t_star_move_by_the_leading() {
        let d = run(b"BT /F0 10 Tf 12 TL 5 700 Td (A) Tj T* (B) Tj ET");
        assert_eq!(d.glyphs.first().map(|g| (g.1, g.2)), Some((5.0, 700.0)));
        assert_eq!(d.glyphs.get(1).map(|g| (g.1, g.2)), Some((5.0, 688.0)));
    }

    #[test]
    fn invisible_text_is_still_shown_to_the_device() {
        // Mode 3 is what a scanned page's OCR layer uses; extraction must see
        // it even though a renderer paints nothing.
        let d = run(b"BT /F0 10 Tf 3 Tr (hidden) Tj ET");
        let text: String = d.glyphs.iter().map(|g| g.0.as_str()).collect();
        assert_eq!(text, "hidden");
    }

    #[test]
    fn the_transform_stack_restores_state() {
        let d = run(b"q 2 0 0 2 0 0 cm BT /F0 10 Tf (A) Tj ET Q BT /F0 10 Tf (B) Tj ET");
        // Inside the q/Q the CTM doubles the font size's effect on advance,
        // but both glyphs start at the origin.
        assert_eq!(d.glyphs.len(), 2);
        assert_eq!(d.glyphs.first().map(|g| g.1), Some(0.0));
        assert_eq!(d.glyphs.get(1).map(|g| g.1), Some(0.0));
    }

    #[test]
    fn paths_reach_the_device() {
        let d = run(b"0 0 m 10 10 l S 0 0 100 100 re f");
        assert_eq!(d.strokes, 1);
        assert_eq!(d.fills, 1);
    }

    #[test]
    fn an_inline_image_does_not_derail_the_stream() {
        let d = run(b"BI /W 2 /H 2 ID \x00\x01\x02\x03 EI BT /F0 10 Tf (A) Tj ET");
        let text: String = d.glyphs.iter().map(|g| g.0.as_str()).collect();
        assert_eq!(text, "A", "text after an inline image still runs");
    }

    #[test]
    fn malformed_streams_do_not_panic() {
        for src in [
            b"".as_slice(),
            b"Tj",
            b"BT ET ET ET",
            b"Q Q Q",
            b"1 0 0 1 cm",
            b"BT /F0 Tf (x) Tj ET",
            b"[ ( unclosed",
            &[0xFF; 512],
        ] {
            let _ = run(src);
        }
    }

    #[test]
    fn a_non_finite_matrix_is_refused() {
        // Without the guard everything after this would be at NaN.
        let d = run(b"BT /F0 10 Tf 0 0 Td (A) Tj 1 0 0 0 0 0 Tm (B) Tj ET");
        assert!(d.glyphs.iter().all(|g| g.1.is_finite() && g.2.is_finite()));
    }

    // -----------------------------------------------------------------------
    // Patterns (gap 07, 8.7.3).
    // -----------------------------------------------------------------------

    /// The state as the device saw it when the path was painted.
    #[derive(Default)]
    struct Painted {
        states: Vec<GraphicsState>,
    }

    impl Device for Painted {
        fn fill_path(&mut self, _path: &[PathSegment], state: &GraphicsState, _even_odd: bool) {
            self.states.push(state.clone());
        }
        fn stroke_path(&mut self, _path: &[PathSegment], state: &GraphicsState) {
            self.states.push(state.clone());
        }
    }

    /// Two uncoloured pattern spaces (8.7.3.2), over different underlying
    /// spaces so a test cannot pass by resolving against the wrong one:
    /// `/Prgb` reads three components as RGB, `/Pgray` reads one as grey.
    struct PatternSpaces;

    impl FontSource for PatternSpaces {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn color_components(&self, space: &[u8]) -> Option<usize> {
            match space {
                b"Prgb" => Some(3),
                b"Pgray" => Some(1),
                _ => None,
            }
        }
        fn resolve_color(&self, space: &[u8], components: &[f64]) -> Option<Rgb> {
            let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            match (space, components) {
                (b"Prgb", [r, g, b]) => Some(Rgb {
                    r: byte(*r),
                    g: byte(*g),
                    b: byte(*b),
                }),
                (b"Pgray", [v]) => Some(Rgb {
                    r: byte(*v),
                    g: byte(*v),
                    b: byte(*v),
                }),
                _ => None,
            }
        }
    }

    fn run_patterned(src: &[u8]) -> Painted {
        let mut d = Painted::default();
        interpret(src, Matrix::IDENTITY, &mut d, &PatternSpaces);
        d
    }

    /// 8.7.3.2: a `PaintType 2` pattern paints with the colour the operands
    /// before its name supply, in the pattern space's underlying space. The
    /// operands were being collected and then dropped, so the slot held
    /// whatever `cs`/`CS` had reset it to and the pattern had no colour at all
    /// to take.
    ///
    /// The two slots take different spaces *and* different colours here, so
    /// neither can pass by being written twice from the other's operands.
    #[test]
    fn an_uncoloured_pattern_keeps_the_colour_its_operands_gave() {
        let d = run_patterned(
            b"/Prgb cs 1 0 0 /P0 scn \
              /Pgray CS 0.5 /P1 SCN \
              0 0 m 10 10 l S",
        );

        let state = d.states.first().expect("the stroke reached the device");
        assert_eq!(
            state.fill_color,
            Rgb { r: 255, g: 0, b: 0 },
            "the fill slot took its three operands as RGB"
        );
        assert_eq!(
            state.stroke_color,
            Rgb {
                r: 128,
                g: 128,
                b: 128
            },
            "and the stroke slot took its one operand as grey"
        );
        assert_eq!(
            state.fill_pattern.as_deref(),
            Some(b"P0".as_slice()),
            "with both patterns still selected"
        );
        assert_eq!(state.stroke_pattern.as_deref(), Some(b"P1".as_slice()));
    }

    /// A coloured pattern names itself and nothing else. Whatever colour the
    /// slot held stays put — there is nothing to overwrite it with, and the
    /// pattern supplies its own paint.
    #[test]
    fn a_pattern_named_alone_leaves_the_colour_where_it_was() {
        let d = run_patterned(b"0 1 0 RG /P1 SCN 0 0 m 10 10 l S");

        let state = d.states.first().expect("the stroke reached the device");
        assert_eq!(
            state.stroke_color,
            Rgb { r: 0, g: 255, b: 0 },
            "no operands, so no new colour"
        );
        assert_eq!(state.stroke_pattern.as_deref(), Some(b"P1".as_slice()));
    }

    /// Choosing an ordinary colour has to clear the pattern on the stroke slot
    /// as it does on the fill slot, or every later stroke keeps painting it.
    #[test]
    fn an_ordinary_stroking_colour_clears_the_stroke_pattern() {
        let d = run_patterned(b"/P1 SCN 0 0 1 RG 0 0 m 10 10 l S");

        let state = d.states.first().expect("the stroke reached the device");
        assert_eq!(state.stroke_pattern, None);
        assert_eq!(state.stroke_color, Rgb { r: 0, g: 0, b: 255 });
    }

    // -----------------------------------------------------------------------
    // Marked content and optional content (gap 06, 14.6.2 and 8.11.3.2).
    // -----------------------------------------------------------------------

    /// A device that keeps the nesting the interpreter reports, so a test can
    /// ask what was in force when something was painted.
    ///
    /// The counter is exactly what a drawing device has to keep — each scope's
    /// *own* visibility arrives, so the enclosing answer is this stack — which
    /// is the point: a test here fails for the same reason the renderer would.
    #[derive(Default)]
    struct Scopes {
        /// Whether each open scope hid its contents, innermost last.
        open: Vec<bool>,
        /// Every `begin`, in order: `(visible, layer)`.
        begins: Vec<(bool, Option<String>)>,
        /// How many `end`s arrived.
        ends: usize,
        /// One entry per fill: whether any scope was hiding it.
        fills: Vec<bool>,
        /// One entry per glyph: its text, and whether any scope was hiding it.
        glyphs: Vec<(String, bool)>,
    }

    impl Scopes {
        fn hidden(&self) -> bool {
            self.open.iter().any(|h| *h)
        }
        /// The fills that were painted with nothing hiding them.
        fn painted(&self) -> usize {
            self.fills.iter().filter(|hidden| !**hidden).count()
        }
    }

    impl Device for Scopes {
        fn begin_marked_content(&mut self, visible: bool, hidden_layer: Option<&str>) {
            self.open.push(!visible);
            self.begins
                .push((visible, hidden_layer.map(str::to_string)));
        }
        fn end_marked_content(&mut self) {
            self.ends += 1;
            // Counted separately from the stack, so a pop with nothing open
            // is visible as `ends > begins` rather than silently absorbed —
            // which is what a real device's `saturating_sub` would do.
            self.open.pop();
        }
        fn fill_path(&mut self, _path: &[PathSegment], _state: &GraphicsState, _even_odd: bool) {
            let hidden = self.hidden();
            self.fills.push(hidden);
        }
        fn show_glyph(&mut self, glyph: &Glyph, _state: &GraphicsState) {
            let hidden = self.hidden();
            self.glyphs.push((glyph.text.clone(), hidden));
        }
    }

    /// `/Off` names a layer the configuration hides, `/On` one it shows, and
    /// `/Plain` is a property list that is not optional content at all.
    ///
    /// Three answers rather than two, because "hidden" and "not a layer" are
    /// different `None`s and a seam that collapsed them would hide every
    /// `/Span`.
    struct Layers;

    impl FontSource for Layers {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn optional_content(&self, name: &[u8]) -> Option<Layer> {
            match name {
                b"Off" => Some(Layer {
                    visible: false,
                    label: "Construction lines".to_string(),
                }),
                b"On" => Some(Layer {
                    visible: true,
                    label: "Base".to_string(),
                }),
                _ => None,
            }
        }
        fn form(&self, name: &[u8]) -> Option<Form> {
            (name == b"Frm").then(|| Form {
                // Opens a scope and never closes it, and closes one it never
                // opened. Both are what a real damaged form does.
                content: b"EMC /OC /Off BDC 0 0 5 5 re f".to_vec(),
                matrix: Matrix::IDENTITY,
                bbox: None,
            })
        }
    }

    fn run_layers(src: &[u8]) -> Scopes {
        let mut d = Scopes::default();
        interpret(src, Matrix::IDENTITY, &mut d, &Layers);
        d
    }

    /// M2's first exit criterion: a nested `BDC` inside a hidden scope stays
    /// hidden.
    ///
    /// The inner scope names a layer that is *on*, so a build that reported
    /// only the innermost answer — or that let a nested `BDC` reset the
    /// state — would paint the middle rectangle. Three rectangles, one
    /// outside the scopes entirely, so "nothing painted" cannot pass either.
    #[test]
    fn a_scope_nested_inside_a_hidden_one_stays_hidden() {
        let d = run_layers(
            b"0 0 1 1 re f \
              /OC /Off BDC \
                0 0 2 2 re f \
                /OC /On BDC 0 0 3 3 re f EMC \
                /Span /Plain BDC 0 0 4 4 re f EMC \
              EMC \
              0 0 5 5 re f",
        );

        assert_eq!(d.fills.len(), 5, "every fill reached the device");
        assert_eq!(
            d.fills,
            vec![false, true, true, true, false],
            "the three inside the /Off scope are hidden, including the two \
             nested ones"
        );
        assert_eq!(d.painted(), 2);
        assert_eq!(d.ends, 3, "one EMC per BDC");
    }

    /// M2's second exit criterion: an unbalanced `EMC` does not underflow.
    ///
    /// Four strays before anything opens, and two more after the one scope
    /// closes. A build that let them through would pop past the bottom of the
    /// device's stack; one that counted them would report more `end`s than
    /// `begin`s, which is the same defect one level up.
    #[test]
    fn a_stray_emc_is_dropped_rather_than_underflowing() {
        let d = run_layers(b"EMC EMC EMC EMC /OC /Off BDC 0 0 2 2 re f EMC EMC EMC 0 0 3 3 re f");

        assert_eq!(d.begins.len(), 1);
        assert_eq!(d.ends, 1, "six of the seven EMCs had nothing to close");
        assert_eq!(d.fills, vec![true, false]);
    }

    /// 14.6.2: a stream may end inside a scope, and the device must not be
    /// left there. The page's own content is the outer case; a form XObject
    /// is the one that would otherwise reach out of itself.
    #[test]
    fn an_unclosed_scope_is_closed_at_the_end_of_its_stream() {
        let d = run_layers(b"/OC /Off BDC 0 0 2 2 re f");
        assert_eq!(d.begins.len(), 1);
        assert_eq!(d.ends, 1, "the page's own trailing scope was closed");
        assert!(d.open.is_empty());

        // `Frm` opens one scope it never closes and closes one it never
        // opened. Neither may reach the page: the fill after the form is
        // outside every layer.
        let d = run_layers(b"/Frm Do 0 0 9 9 re f");
        assert_eq!(
            d.fills,
            vec![true, false],
            "the form's own fill is hidden and the page's is not"
        );
        assert_eq!(d.begins.len(), 1);
        assert_eq!(d.ends, 1);
    }

    /// 8.11.3.2: only an `/OC` tag selects optional content.
    #[test]
    fn a_tag_that_is_not_oc_hides_nothing() {
        // `/Off` is a hidden layer, but the tag here is `/Span`, so the name
        // is an ordinary property list and the fill paints.
        let d = run_layers(b"/Span /Off BDC 0 0 2 2 re f EMC");
        assert_eq!(d.fills, vec![false]);
        assert_eq!(d.begins, vec![(true, None)]);
    }

    /// Ruling 10: the scope that hides carries the layer's name, so a device
    /// can say what it hid rather than only that something was hidden.
    #[test]
    fn a_hidden_scope_names_its_layer() {
        let d = run_layers(b"/OC /Off BDC 0 0 2 2 re f EMC");
        assert_eq!(
            d.begins,
            vec![(false, Some("Construction lines".to_string()))]
        );
    }

    /// An inline property list (7.8.2 tokenizes it flat) renders.
    ///
    /// 7.3.10 keeps indirect references out of content streams and 8.11.3.2
    /// makes an `/OC` entry a reference, so an inline list cannot name a
    /// layer. The tag still has to be found, though — it sits before the
    /// `<<`, not before the operator — and a malformed one must not turn into
    /// a hidden scope. Every case here paints.
    #[test]
    fn an_inline_property_list_renders() {
        for src in [
            b"/OC << /Type /OCMD >> BDC 0 0 2 2 re f EMC".as_slice(),
            b"/OC << /OCGs [ ] /P /AllOff >> BDC 0 0 2 2 re f EMC",
            b"/OC << /A << /B 1 >> >> BDC 0 0 2 2 re f EMC",
            // Unbalanced: no `<<` for the `>>` to match.
            b"/OC /Type /OCMD >> BDC 0 0 2 2 re f EMC",
            // No tag at all.
            b"<< /Type /OCMD >> BDC 0 0 2 2 re f EMC",
            b"BDC 0 0 2 2 re f EMC",
        ] {
            let d = run_layers(src);
            assert_eq!(
                d.fills,
                vec![false],
                "{} should paint",
                String::from_utf8_lossy(src)
            );
        }
    }

    /// The operators that used to fall into the catch-all, in one stream that
    /// also draws. None of them opens a scope, and none of them may swallow
    /// the operators after it.
    #[test]
    fn the_remaining_marked_content_operators_are_no_ops() {
        let d = run_layers(
            b"BX /Weird1 op EX \
              /Point MP \
              /Point << /K 1 >> DP \
              BMC 0 0 2 2 re f EMC \
              0 0 3 3 re f",
        );

        assert_eq!(d.begins.len(), 1, "only the BMC opened a scope");
        assert_eq!(d.begins, vec![(true, None)]);
        assert_eq!(d.ends, 1);
        assert_eq!(d.fills, vec![false, false], "both fills painted");
    }

    /// Hidden content still runs. The pen advances, `q`/`Q` balance, and the
    /// glyphs reach the device — the whole reason suppression belongs at the
    /// paint rather than here.
    #[test]
    fn a_hidden_scope_still_interprets_everything_inside_it() {
        let d = run_layers(
            b"/OC /Off BDC q 2 0 0 2 0 0 cm BT /F0 10 Tf (AB) Tj ET Q EMC \
              BT /F0 10 Tf (C) Tj ET",
        );

        let text: String = d.glyphs.iter().map(|g| g.0.as_str()).collect();
        assert_eq!(text, "ABC", "the hidden glyphs were still shown");
        assert_eq!(
            d.glyphs.iter().map(|g| g.1).collect::<Vec<_>>(),
            vec![true, true, false],
            "and only their painting was marked hidden"
        );
    }

    /// Depth is bounded, and running out of it shows content rather than
    /// hiding it — the direction a hostile stream should fail in.
    ///
    /// The arrangement is the one that can tell a build that *counts* the
    /// scopes it refused from one that silently drops them. A hidden scope
    /// is opened first, the cap is then filled with visible ones, and three
    /// more are opened past it. The three `EMC`s that follow belong to those
    /// three; a build that let them close reported scopes instead would be
    /// three short by the time the unwinding reaches the outer hidden one,
    /// close it early, and paint the last rectangle. Every simpler ordering
    /// — nest to the cap, then unwind completely — gives the same pixels
    /// either way, which is how this test came to be written twice.
    #[test]
    fn runaway_nesting_is_bounded_and_stays_balanced() {
        let cap = MAX_MARKED_CONTENT_DEPTH as usize;
        let mut src = Vec::from(b"/OC /Off BDC ".as_slice());
        for _ in 0..cap - 1 {
            src.extend_from_slice(b"/OC /On BDC ");
        }
        for _ in 0..3 {
            src.extend_from_slice(b"/OC /Off BDC ");
        }
        src.extend_from_slice(b"0 0 2 2 re f ");
        // The three that belong to the scopes past the cap.
        src.extend_from_slice(b"EMC EMC EMC ");
        // Then the cap's worth, less the outermost.
        for _ in 0..cap - 1 {
            src.extend_from_slice(b"EMC ");
        }
        src.extend_from_slice(b"0 0 3 3 re f EMC 0 0 4 4 re f");

        let d = run_layers(&src);
        assert_eq!(d.begins.len(), cap, "the cap held");
        assert_eq!(d.begins.len(), d.ends, "and every one was closed");
        assert!(d.open.is_empty());
        assert_eq!(
            d.fills,
            vec![true, true, false],
            "the fill inside is hidden; so is the one after the unwinding, \
             because the outermost scope is still open; only the fill after \
             its own EMC paints"
        );
        assert!(
            d.begins.iter().filter(|(visible, _)| !*visible).count() == 1,
            "the three scopes past the cap were refused, and a refused scope \
             is a visible one"
        );
    }
}
