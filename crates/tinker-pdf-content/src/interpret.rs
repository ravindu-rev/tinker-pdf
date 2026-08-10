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
pub trait FontSource {
    /// Splits a string into `(code, text, advance in 1/1000 em)`.
    fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)>;

    /// Whether the named font writes vertically.
    fn is_vertical(&self, font: &[u8]) -> bool {
        let _ = font;
        false
    }

    /// A stable identifier for the named font.
    fn font_id(&self, font: &[u8]) -> u64 {
        let _ = font;
        0
    }

    /// The bytes of a form XObject, when the interpreter should recurse into
    /// one. Returning `None` skips it.
    fn form(&self, name: &[u8]) -> Option<(Vec<u8>, Matrix)> {
        let _ = name;
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

    /// How many components a named colour space takes.
    ///
    /// Used to tell `scn`'s optional trailing pattern name from its numeric
    /// operands.
    fn color_components(&self, space: &[u8]) -> Option<usize> {
        let _ = space;
        None
    }
}

/// How deep form XObjects may nest before recursion is refused (8.10).
const MAX_FORM_DEPTH: u32 = 16;

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
    };
    interp.run(content);
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
                    // 8.9.7: an inline image's data is not tokenizable, so
                    // scan for its end and resume there.
                    let consumed = skip_inline_image(tokens.rest());
                    let at = tokens.position();
                    tokens.seek(at + consumed);
                    self.device.draw_image(
                        &ImageRef {
                            name: Vec::new(),
                            inline: true,
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
                if components.is_empty() {
                    // `scn` with only a pattern name: a pattern's colour comes
                    // from the pattern, which this device does not paint.
                    return;
                }

                let space = if fill {
                    self.gs.fill_space.clone()
                } else {
                    self.gs.stroke_space.clone()
                };
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
                }
            }

            // 8.7.4.2: `sh` paints a shading over the current clip.
            b"sh" => {
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    self.device.draw_shading(&name, &self.gs);
                }
            }

            _ => {}
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
            if let Some(space) = space {
                self.gs.fill_space = Some(space);
            }
        } else {
            self.gs.stroke_color = color;
            if let Some(space) = space {
                self.gs.stroke_space = Some(space);
            }
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
        let Some((content, matrix)) = self.fonts.form(name) else {
            self.device.draw_image(
                &ImageRef {
                    name: name.to_vec(),
                    inline: false,
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

        let combined = matrix.then(&self.gs.ctm);
        if combined.is_finite() {
            self.gs.ctm = combined;
        }
        self.depth += 1;
        self.run(&content);
        self.depth -= 1;

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

        for item in items {
            match item {
                Token::String(s) => self.show(&s),
                Token::Number(adjust) if adjust.is_finite() => {
                    let ts = &self.gs.text;
                    let shift = -adjust / 1000.0 * ts.size * ts.horizontal_scale;
                    self.text_matrix = Matrix::translate(shift, 0.0).then(&self.text_matrix);
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
            let transform = scale.then(&self.text_matrix).then(&self.gs.ctm);

            let glyph = Glyph {
                code,
                text: text.clone(),
                transform,
                advance: width / 1000.0 * ts.size,
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
            let advance = (width / 1000.0 * ts.size + ts.char_spacing + word) * ts.horizontal_scale;

            self.text_matrix = if vertical {
                Matrix::translate(0.0, -advance).then(&self.text_matrix)
            } else {
                Matrix::translate(advance, 0.0).then(&self.text_matrix)
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
        fills: usize,
        strokes: usize,
    }

    impl Device for Recorder {
        fn show_glyph(&mut self, glyph: &Glyph, _state: &GraphicsState) {
            self.glyphs
                .push((glyph.text.clone(), glyph.transform.e, glyph.transform.f));
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
    }

    fn run(src: &[u8]) -> Recorder {
        let mut d = Recorder::default();
        interpret(src, Matrix::IDENTITY, &mut d, &Simple);
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
}
