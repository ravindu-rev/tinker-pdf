//! A page and its content, generated: every operator of Annex A, with the
//! operands it takes or with wrong ones, over resources generated with the
//! page (ISO 32000-2 8 and 9), in a document the COS writer writes clean.
//!
//! `content_stream` holds the display list to the page it records. The page
//! is written with [`crate::cos::write`] over no choices, so every token of
//! the file is in its plainest spelling and the input's bytes go to what the
//! page draws.

use tinker_pdf::{PixelFormat, PixelRegion, RenderOptions};

use crate::cos::{self, Body, Entries, File, Indirect, Real, Section, Spell, Stream, Value, Xref};
use crate::Choices;

/// The most operators one content stream holds.
pub const MAX_OPS: usize = 32;
/// The most operators the document holds, every stream's together.
pub const MAX_TOTAL_OPS: usize = 192;
/// How deep forms, patterns, glyph procedures and appearances nest.
pub const MAX_DEPTH: u32 = 2;
/// The widest and the tallest an image is, in samples.
pub const MAX_IMAGE: usize = 8;
/// The longest edge of the page, in points.
pub const MAX_PAGE: i64 = 72;

/// A generated one-page document, and how it is to be rendered.
pub struct Page {
    /// The file.
    pub bytes: Vec<u8>,
    /// The render's options, generated with the page.
    pub options: RenderOptions,
}

/// The resource names content may name, each defined or not.
const FONTS: &[&[u8]] = &[b"F0", b"F1", b"F2", b"T3"];
const FORMS: &[&[u8]] = &[b"X0", b"X1", b"X2"];
const IMAGES: &[&[u8]] = &[b"I0", b"I1", b"I2", b"I3", b"I4"];
const STATES: &[&[u8]] = &[b"G0", b"G1"];
const SPACES: &[&[u8]] = &[b"C0", b"C1", b"C2", b"C3", b"C4", b"C5"];
const PATTERNS: &[&[u8]] = &[b"P0", b"P1", b"P2"];
const SHADINGS: &[&[u8]] = &[b"S0", b"S1"];
const PROPERTIES: &[&[u8]] = &[b"M0", b"M1"];

/// The standard 14 fonts the `F` names stand for (9.6.2.2).
const STANDARD: &[&[u8]] = &[b"Helvetica", b"Times-Roman", b"Courier-Bold"];

/// 8.6.5.8's device space names and the abbreviations 8.9.7 adds for an
/// inline image.
const DEVICE_SPACES: &[&[u8]] = &[b"DeviceGray", b"DeviceRGB", b"DeviceCMYK", b"Pattern"];

/// Table 134's blend modes.
const BLENDS: &[&[u8]] = &[
    b"Normal",
    b"Multiply",
    b"Screen",
    b"Overlay",
    b"Darken",
    b"Lighten",
    b"ColorDodge",
    b"ColorBurn",
    b"HardLight",
    b"SoftLight",
    b"Difference",
    b"Exclusion",
    b"Hue",
    b"Saturation",
    b"Color",
    b"Luminosity",
];

const INTENTS: &[&[u8]] = &[
    b"AbsoluteColorimetric",
    b"RelativeColorimetric",
    b"Saturation",
    b"Perceptual",
];

/// Every operator of Annex A but `BI`, which [`Gen::inline_image`] writes
/// with its dictionary and data.
const OPERATORS: &[&[u8]] = &[
    b"w", b"J", b"j", b"M", b"d", b"ri", b"i", b"gs", b"q", b"Q", b"cm", b"m", b"l", b"c", b"v",
    b"y", b"h", b"re", b"S", b"s", b"f", b"F", b"f*", b"B", b"B*", b"b", b"b*", b"n", b"W", b"W*",
    b"BT", b"ET", b"Tc", b"Tw", b"Tz", b"TL", b"Tf", b"Tr", b"Ts", b"Td", b"TD", b"Tm", b"T*",
    b"Tj", b"TJ", b"'", b"\"", b"d0", b"d1", b"CS", b"cs", b"SC", b"SCN", b"sc", b"scn", b"G",
    b"g", b"RG", b"rg", b"K", b"k", b"sh", b"Do", b"MP", b"DP", b"BMC", b"BDC", b"EMC", b"BX",
    b"EX",
];

/// The generator's state: the choices, the objects written so far, and what
/// the document has left to spend.
struct Gen<'c, 'a> {
    c: &'c mut Choices<'a>,
    objects: Vec<Indirect>,
    next: u32,
    budget: usize,
    width: i64,
    height: i64,
    /// Whether the content being written is the page's own.
    in_page: bool,
    /// What draws an image, counted where it is written.
    drawn: Drawn,
}

/// The operators that may draw an image, counted by where they are: `BI`,
/// and `Do` naming anything but a form. Over every stream a page's drawing
/// reaches, so a superset of the images it draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drawn {
    /// In the page's own content, where each operator runs once.
    pub page: usize,
    /// In a form, a pattern, a glyph procedure or an appearance, which may
    /// run any number of times.
    pub nested: usize,
}

/// Whether an operator with these operands may draw an image: `Do` naming
/// anything but one of the forms, the only XObjects that are not images.
fn draws_image(op: &[u8], operands: &[Value]) -> bool {
    op == b"Do"
        && operands
            .iter()
            .any(|v| matches!(v, Value::Name(name) if !FORMS.contains(&name.as_slice())))
}

/// Writes `value` after whatever separates it from what is already there.
fn put(out: &mut Vec<u8>, value: &Value) {
    let mut none = Choices::new(&[]);
    let lead = out.is_empty();
    Spell::new(&mut none).value(out, value, lead);
}

/// Writes an operator after whatever separates it from its operands.
fn operator(out: &mut Vec<u8>, name: &[u8]) {
    let mut none = Choices::new(&[]);
    let lead = out.is_empty();
    Spell::new(&mut none).token(out, name, lead);
    // Each operator on a line of its own keeps a stream readable.
    out.push(b'\n');
}

fn real(mantissa: i64, places: u8) -> Value {
    Value::Real(Real::new(mantissa, places))
}

fn names(list: &[&[u8]]) -> Vec<Value> {
    list.iter().map(|name| Value::name(name)).collect()
}

impl Gen<'_, '_> {
    fn add(&mut self, body: Body) -> u32 {
        let num = self.next;
        self.next += 1;
        self.objects.push(Indirect { num, gen: 0, body });
        num
    }

    /// A number in `lo..=hi`, an integer or a real of one or two places.
    fn number(&mut self, lo: i64, hi: i64) -> Value {
        match self.c.below(4) {
            0 => Value::Int(self.c.range(lo, hi)),
            1 => real(self.c.range(lo * 100, hi * 100), 2),
            _ => real(self.c.range(lo * 10, hi * 10), 1),
        }
    }

    /// A coordinate: mostly on the page, sometimes well off it.
    fn coord(&mut self, along: i64) -> Value {
        match self.c.below(8) {
            0 => self
                .c
                .pick(&[Value::Int(0), Value::Int(-10_000), Value::Int(10_000)])
                .clone(),
            _ => self.number(-along / 4, along + along / 4),
        }
    }

    fn x(&mut self) -> Value {
        let width = self.width;
        self.coord(width)
    }

    fn y(&mut self) -> Value {
        let height = self.height;
        self.coord(height)
    }

    /// A colour component, sometimes outside `0..=1`.
    fn unit(&mut self) -> Value {
        match self.c.below(8) {
            0 => self
                .c
                .pick(&[real(-5, 1), real(15, 1), Value::Int(2)])
                .clone(),
            _ => real(self.c.range(0, 10), 1),
        }
    }

    /// A transformation matrix: a scale or rotation-ish 2x2 and a shift.
    fn matrix(&mut self) -> Vec<Value> {
        let mut m = Vec::with_capacity(6);
        for _ in 0..4 {
            m.push(match self.c.below(4) {
                0 => Value::Int(0),
                1 => Value::Int(1),
                _ => real(self.c.range(-20, 20), 1),
            });
        }
        let (x, y) = (self.x(), self.y());
        m.push(x);
        m.push(y);
        m
    }

    /// A value of any type, for a wrong operand.
    fn any(&mut self) -> Value {
        match self.c.below(8) {
            0 => Value::Null,
            1 => Value::Bool(self.c.flag()),
            2 => Value::Int(self.c.range(-1000, 1000)),
            3 => real(self.c.range(-10_000, 10_000), 2),
            4 => Value::literal(&self.c.bytes(6)),
            5 => Value::name(self.c.pick(&[b"F0".as_slice(), b"X0", b"Foo", b""])),
            6 => Value::Array(vec![Value::Int(1), Value::name(b"A")]),
            _ => Value::dict(vec![(b"K", Value::Int(1))]),
        }
    }

    /// Text to show: the Type 3 font's codes, or any bytes.
    fn text(&mut self) -> Value {
        if self.c.flag() {
            let len = self.c.below(6);
            let bytes: Vec<u8> = (0..len)
                .map(|_| *self.c.pick(&[b'a', b'b', b'c', b' ', b'A']))
                .collect();
            Value::literal(&bytes)
        } else {
            Value::String(self.c.bytes(8), cos::Form::Hex)
        }
    }

    /// A property list: inline or named (14.6.2).
    fn property_list(&mut self) -> Value {
        if self.c.flag() {
            Value::name(self.c.pick(PROPERTIES))
        } else {
            Value::dict(vec![(b"MCID", Value::Int(self.c.range(0, 3)))])
        }
    }

    /// The operands `op` takes (Annex A).
    fn operands(&mut self, op: &[u8]) -> Vec<Value> {
        match op {
            b"w" => vec![self.number(0, 8)],
            b"J" | b"j" => vec![Value::Int(self.c.range(0, 2))],
            b"M" => vec![self.number(1, 10)],
            b"d" => {
                let dashes = (0..self.c.below(5)).map(|_| self.number(1, 8)).collect();
                vec![Value::Array(dashes), self.number(0, 8)]
            }
            b"ri" => vec![Value::name(self.c.pick(INTENTS))],
            b"i" => vec![self.number(0, 100)],
            b"gs" => vec![Value::name(self.c.pick(STATES))],
            b"cm" | b"Tm" => self.matrix(),
            b"m" | b"l" | b"Td" | b"TD" => vec![self.x(), self.y()],
            b"c" => vec![self.x(), self.y(), self.x(), self.y(), self.x(), self.y()],
            b"v" | b"y" => vec![self.x(), self.y(), self.x(), self.y()],
            b"re" => {
                let (x, y) = (self.x(), self.y());
                let (w, h) = (self.x(), self.y());
                vec![x, y, w, h]
            }
            b"Tc" | b"Tw" => vec![self.number(-2, 4)],
            b"Tz" => vec![self.number(10, 200)],
            b"TL" | b"Ts" => vec![self.number(-12, 12)],
            b"Tf" => vec![Value::name(self.c.pick(FONTS)), self.number(1, 24)],
            b"Tr" => vec![Value::Int(self.c.range(0, 7))],
            b"Tj" | b"'" => vec![self.text()],
            b"\"" => vec![self.number(-2, 4), self.number(-2, 4), self.text()],
            b"TJ" => {
                let items = (0..self.c.below(5))
                    .map(|_| {
                        if self.c.flag() {
                            self.text()
                        } else {
                            self.number(-500, 500)
                        }
                    })
                    .collect();
                vec![Value::Array(items)]
            }
            b"d0" => vec![self.number(0, 1000), Value::Int(0)],
            b"d1" => vec![
                self.number(0, 1000),
                Value::Int(0),
                self.number(-100, 100),
                self.number(-100, 100),
                self.number(100, 1000),
                self.number(100, 1000),
            ],
            b"CS" | b"cs" => {
                if self.c.flag() {
                    vec![Value::name(self.c.pick(DEVICE_SPACES))]
                } else {
                    vec![Value::name(self.c.pick(SPACES))]
                }
            }
            b"SC" | b"sc" => (0..1 + self.c.below(4)).map(|_| self.unit()).collect(),
            b"SCN" | b"scn" => {
                let mut values: Vec<Value> = (0..self.c.below(5)).map(|_| self.unit()).collect();
                if self.c.flag() {
                    values.push(Value::name(self.c.pick(PATTERNS)));
                }
                values
            }
            b"G" | b"g" => vec![self.unit()],
            b"RG" | b"rg" => vec![self.unit(), self.unit(), self.unit()],
            b"K" | b"k" => vec![self.unit(), self.unit(), self.unit(), self.unit()],
            b"sh" => vec![Value::name(self.c.pick(SHADINGS))],
            b"Do" => {
                if self.c.flag() {
                    vec![Value::name(self.c.pick(FORMS))]
                } else {
                    vec![Value::name(self.c.pick(IMAGES))]
                }
            }
            b"MP" | b"BMC" => vec![Value::name(self.c.pick(&[
                b"Tag".as_slice(),
                b"Artifact",
                b"Span",
            ]))],
            b"DP" | b"BDC" => vec![
                Value::name(
                    self.c
                        .pick(&[b"Tag".as_slice(), b"Artifact", b"Span", b"OC"]),
                ),
                self.property_list(),
            ],
            _ => Vec::new(),
        }
    }

    /// One operator, its operands of the right shape or, one time in four,
    /// of a wrong one.
    fn op(&mut self, out: &mut Vec<u8>, depth: u32) {
        match self.c.below(12) {
            // A balanced block, so nesting is common rather than an accident.
            0 => {
                let (open, close): (&[u8], &[u8]) = *self.c.pick(&[
                    (b"q".as_slice(), b"Q".as_slice()),
                    (b"BT", b"ET"),
                    (b"BX", b"EX"),
                    (b"BMC", b"EMC"),
                    (b"BDC", b"EMC"),
                ]);
                let operands = self.operands(open);
                for value in &operands {
                    put(out, value);
                }
                operator(out, open);
                for _ in 0..self.c.below(6) {
                    if self.budget == 0 {
                        break;
                    }
                    self.budget -= 1;
                    self.op(out, depth);
                }
                operator(out, close);
            }
            1 => self.inline_image(out),
            _ => {
                let op = *self.c.pick(OPERATORS);
                let mut operands = self.operands(op);
                if self.c.below(4) == 3 {
                    match self.c.below(4) {
                        0 => {
                            operands.pop();
                        }
                        1 => {
                            let extra = self.any();
                            operands.push(extra);
                        }
                        2 if !operands.is_empty() => {
                            let at = self.c.below(operands.len());
                            operands[at] = self.any();
                        }
                        _ => operands.clear(),
                    }
                }
                if draws_image(op, &operands) {
                    self.count_image();
                }
                for value in &operands {
                    put(out, value);
                }
                operator(out, op);
            }
        }
    }

    fn count_image(&mut self) {
        if self.in_page {
            self.drawn.page += 1;
        } else {
            self.drawn.nested += 1;
        }
    }

    /// Image samples: `len` bytes from a few of the input's, repeated.
    fn samples(&mut self, len: usize) -> Vec<u8> {
        let seed = self.c.bytes(16);
        if seed.is_empty() {
            return vec![0x80; len];
        }
        (0..len).map(|at| seed[at % seed.len()]).collect()
    }

    /// An image's geometry and sample layout: width, height, components and
    /// bits a component, with the bytes its rows take (8.9.3).
    fn geometry(&mut self, components: usize, depths: &[i64]) -> (i64, i64, i64, usize) {
        let width = 1 + self.c.below(MAX_IMAGE) as i64;
        let height = 1 + self.c.below(MAX_IMAGE) as i64;
        let bits = *self.c.pick(depths);
        let row = (width as usize * components * bits as usize).div_ceil(8);
        (width, height, bits, row * height as usize)
    }

    /// An inline image (8.9.7), its keys abbreviated or not, its data as long
    /// as its dictionary says.
    fn inline_image(&mut self, out: &mut Vec<u8>) {
        let abbreviate = self.c.flag();
        let key = |full: &'static [u8], short: &'static [u8]| -> &'static [u8] {
            if abbreviate {
                short
            } else {
                full
            }
        };
        let mask = self.c.below(4) == 3;
        let (space, components, depths): (Value, usize, &[i64]) = if mask {
            (Value::Null, 1, &[1])
        } else {
            match self.c.below(4) {
                0 => (Value::name(key(b"DeviceGray", b"G")), 1, &[1, 2, 4, 8]),
                1 => (Value::name(key(b"DeviceRGB", b"RGB")), 3, &[8]),
                2 => (Value::name(key(b"DeviceCMYK", b"CMYK")), 4, &[8]),
                _ => (
                    Value::Array(vec![
                        Value::name(key(b"Indexed", b"I")),
                        Value::name(key(b"DeviceRGB", b"RGB")),
                        Value::Int(1),
                        Value::String(vec![255, 0, 0, 0, 0, 255], cos::Form::Hex),
                    ]),
                    1,
                    &[1, 8],
                ),
            }
        };
        let (width, height, bits, len) = self.geometry(components, depths);
        self.count_image();
        operator(out, b"BI");
        let mut entries: Vec<(&[u8], Value)> = vec![
            (key(b"Width", b"W"), Value::Int(width)),
            (key(b"Height", b"H"), Value::Int(height)),
        ];
        if mask {
            entries.push((key(b"ImageMask", b"IM"), Value::Bool(true)));
        } else {
            entries.push((key(b"BitsPerComponent", b"BPC"), Value::Int(bits)));
            entries.push((key(b"ColorSpace", b"CS"), space));
        }
        if self.c.flag() {
            entries.push((key(b"Interpolate", b"I"), Value::Bool(true)));
        }
        if mask && self.c.flag() {
            entries.push((
                key(b"Decode", b"D"),
                Value::Array(vec![Value::Int(1), Value::Int(0)]),
            ));
        }
        for (name, value) in entries {
            put(out, &Value::name(name));
            put(out, &value);
        }
        operator(out, b"ID");
        // `operator` ended the line: that LF is the one white-space byte
        // after `ID`.
        let data = self.samples(len);
        out.extend_from_slice(&data);
        out.push(b'\n');
        operator(out, b"EI");
    }

    /// A content stream's bytes: for a glyph procedure, `d0` or `d1` first.
    fn content(&mut self, depth: u32, glyph: bool, page: bool) -> Vec<u8> {
        let outer = std::mem::replace(&mut self.in_page, page);
        let out = self.content_of(depth, glyph, page);
        self.in_page = outer;
        out
    }

    fn content_of(&mut self, depth: u32, glyph: bool, page: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if page {
            // A filled rectangle first, so the page draws something unless
            // its own content paints over it.
            let colour = vec![self.unit(), self.unit(), self.unit()];
            for value in &colour {
                put(&mut out, value);
            }
            operator(&mut out, b"rg");
            let (w, h) = (self.width, self.height);
            for value in [
                Value::Int(w / 8),
                Value::Int(h / 8),
                Value::Int(w / 2),
                Value::Int(h / 2),
            ] {
                put(&mut out, &value);
            }
            operator(&mut out, b"re");
            operator(&mut out, b"f");
        }
        if glyph {
            let first: &[u8] = if self.c.flag() { b"d0" } else { b"d1" };
            for value in self.operands(first) {
                put(&mut out, &value);
            }
            operator(&mut out, first);
        }
        for _ in 0..self.c.below(MAX_OPS + 1) {
            if self.budget == 0 {
                break;
            }
            self.budget -= 1;
            self.op(&mut out, depth);
        }
        out
    }

    /// A stream of `content`, compressed one time in four.
    fn stream(&mut self, dict: Entries, content: Vec<u8>) -> Body {
        Body::Stream(if self.c.below(4) == 3 {
            Stream::flate(dict, content)
        } else {
            Stream::new(dict, content)
        })
    }

    fn rect(&mut self) -> Value {
        let (x0, y0) = (self.c.range(-8, 8), self.c.range(-8, 8));
        let (x1, y1) = (
            x0 + 1 + self.c.range(0, self.width),
            y0 + 1 + self.c.range(0, self.height),
        );
        Value::Array(vec![
            Value::Int(x0),
            Value::Int(y0),
            Value::Int(x1),
            Value::Int(y1),
        ])
    }

    /// A form XObject (8.10): a box, perhaps a matrix and a transparency
    /// group, the shared resources or none, and generated content.
    fn form(&mut self, depth: u32, resources: u32, group: bool) -> u32 {
        let mut dict: Entries = vec![
            (b"Type".to_vec(), Value::name(b"XObject")),
            (b"Subtype".to_vec(), Value::name(b"Form")),
            (b"BBox".to_vec(), self.rect()),
        ];
        if self.c.flag() {
            dict.push((b"Matrix".to_vec(), Value::Array(self.matrix())));
        }
        if group || self.c.below(4) == 3 {
            let space: &[u8] = if group { b"DeviceGray" } else { b"DeviceRGB" };
            dict.push((
                b"Group".to_vec(),
                Value::dict(vec![
                    (b"S", Value::name(b"Transparency")),
                    (b"CS", Value::name(space)),
                    (b"I", Value::Bool(self.c.flag())),
                    (b"K", Value::Bool(self.c.flag())),
                ]),
            ));
        }
        if self.c.below(4) != 0 {
            dict.push((b"Resources".to_vec(), Value::reference(resources)));
        }
        let content = self.content(depth + 1, false, false);
        let body = self.stream(dict, content);
        self.add(body)
    }

    /// An image XObject (8.9.5) of `components` per sample, or a stencil mask.
    fn image(&mut self, space: Option<Value>, components: usize, depths: &[i64]) -> u32 {
        let (width, height, bits, len) = self.geometry(components, depths);
        let mut dict: Entries = vec![
            (b"Type".to_vec(), Value::name(b"XObject")),
            (b"Subtype".to_vec(), Value::name(b"Image")),
            (b"Width".to_vec(), Value::Int(width)),
            (b"Height".to_vec(), Value::Int(height)),
        ];
        match space {
            Some(space) => {
                dict.push((b"ColorSpace".to_vec(), space));
                dict.push((b"BitsPerComponent".to_vec(), Value::Int(bits)));
            }
            None => {
                dict.push((b"ImageMask".to_vec(), Value::Bool(true)));
                if self.c.flag() {
                    dict.push((
                        b"Decode".to_vec(),
                        Value::Array(vec![Value::Int(1), Value::Int(0)]),
                    ));
                }
            }
        }
        if self.c.flag() {
            dict.push((b"Interpolate".to_vec(), Value::Bool(true)));
        }
        let data = self.samples(len);
        self.add(Body::Stream(Stream::new(dict, data)))
    }

    /// A Type 2 function from `inputs` 0..1 to `c1`'s components (7.10.3).
    fn exponential(&mut self, c0: Vec<Value>, c1: Vec<Value>) -> Value {
        Value::dict(vec![
            (b"FunctionType", Value::Int(2)),
            (b"Domain", Value::Array(vec![Value::Int(0), Value::Int(1)])),
            (b"C0", Value::Array(c0)),
            (b"C1", Value::Array(c1)),
            (b"N", self.number(1, 3)),
        ])
    }

    /// An axial or a radial shading (8.7.4.5.3, 8.7.4.5.4).
    fn shading(&mut self, radial: bool) -> Value {
        let mut coords = vec![self.x(), self.y()];
        if radial {
            coords.push(self.number(0, 20));
        }
        coords.push(self.x());
        coords.push(self.y());
        if radial {
            coords.push(self.number(0, 40));
        }
        let c0 = vec![self.unit(), self.unit(), self.unit()];
        let c1 = vec![self.unit(), self.unit(), self.unit()];
        let function = self.exponential(c0, c1);
        Value::dict(vec![
            (b"ShadingType", Value::Int(if radial { 3 } else { 2 })),
            (b"ColorSpace", Value::name(b"DeviceRGB")),
            (b"Coords", Value::Array(coords)),
            (b"Function", function),
            (
                b"Extend",
                Value::Array(vec![Value::Bool(self.c.flag()), Value::Bool(self.c.flag())]),
            ),
        ])
    }

    /// A tiling pattern (8.7.3): coloured or uncoloured, its cell generated
    /// content.
    fn tiling(&mut self, depth: u32, resources: u32, coloured: bool) -> u32 {
        let dict: Entries = vec![
            (b"Type".to_vec(), Value::name(b"Pattern")),
            (b"PatternType".to_vec(), Value::Int(1)),
            (
                b"PaintType".to_vec(),
                Value::Int(if coloured { 1 } else { 2 }),
            ),
            (b"TilingType".to_vec(), Value::Int(self.c.range(1, 3))),
            (b"BBox".to_vec(), self.rect()),
            (b"XStep".to_vec(), self.number(2, 40)),
            (b"YStep".to_vec(), self.number(2, 40)),
            (b"Resources".to_vec(), Value::reference(resources)),
            (b"Matrix".to_vec(), Value::Array(self.matrix())),
        ];
        let content = self.content(depth + 1, false, false);
        let body = self.stream(dict, content);
        self.add(body)
    }

    /// A Type 3 font (9.6.4) of three glyphs, each a generated procedure.
    fn type3(&mut self, resources: u32) -> Value {
        let mut procs: Entries = Vec::new();
        let mut widths = Vec::new();
        for name in [b"a".as_slice(), b"b", b"c"] {
            let content = self.content(1, true, false);
            let body = self.stream(Vec::new(), content);
            let num = self.add(body);
            procs.push((name.to_vec(), Value::reference(num)));
            widths.push(self.number(0, 1000));
        }
        let mut font: Vec<(&[u8], Value)> = vec![
            (b"Type", Value::name(b"Font")),
            (b"Subtype", Value::name(b"Type3")),
            (
                b"FontBBox",
                Value::Array(vec![
                    Value::Int(0),
                    Value::Int(0),
                    Value::Int(1000),
                    Value::Int(1000),
                ]),
            ),
            (
                b"FontMatrix",
                Value::Array(vec![
                    real(1, 3),
                    Value::Int(0),
                    Value::Int(0),
                    real(1, 3),
                    Value::Int(0),
                    Value::Int(0),
                ]),
            ),
            (b"CharProcs", Value::Dict(procs)),
            (
                b"Encoding",
                Value::dict(vec![
                    (b"Type", Value::name(b"Encoding")),
                    (
                        b"Differences",
                        Value::Array(vec![
                            Value::Int(97),
                            Value::name(b"a"),
                            Value::name(b"b"),
                            Value::name(b"c"),
                        ]),
                    ),
                ]),
            ),
            (b"FirstChar", Value::Int(97)),
            (b"LastChar", Value::Int(99)),
            (b"Widths", Value::Array(widths)),
        ];
        if self.c.flag() {
            font.push((b"Resources", Value::reference(resources)));
        }
        Value::dict(font)
    }

    /// The resource dictionary every stream of the page shares, each entry
    /// present or not.
    fn resources(&mut self, depth: u32, resources: u32) -> Entries {
        let mut out: Entries = Vec::new();

        let mut fonts: Entries = Vec::new();
        for (name, base) in FONTS.iter().zip(STANDARD) {
            if self.c.below(4) != 0 {
                fonts.push((
                    name.to_vec(),
                    Value::dict(vec![
                        (b"Type", Value::name(b"Font")),
                        (b"Subtype", Value::name(b"Type1")),
                        (b"BaseFont", Value::name(base)),
                    ]),
                ));
            }
        }
        if depth < MAX_DEPTH && self.c.below(3) != 0 {
            let font = self.type3(resources);
            fonts.push((b"T3".to_vec(), font));
        }
        out.push((b"Font".to_vec(), Value::Dict(fonts)));

        let mut xobjects: Entries = Vec::new();
        if depth < MAX_DEPTH {
            for (at, name) in FORMS.iter().enumerate() {
                if self.c.flag() {
                    // The last form carries a luminosity group, for a soft
                    // mask to name.
                    let num = self.form(depth, resources, at == FORMS.len() - 1);
                    xobjects.push((name.to_vec(), Value::reference(num)));
                }
            }
        }
        for name in IMAGES {
            if !self.c.flag() {
                continue;
            }
            let num = match *name {
                b"I0" => self.image(Some(Value::name(b"DeviceRGB")), 3, &[8]),
                b"I1" => self.image(None, 1, &[1]),
                b"I2" => self.image(
                    Some(Value::Array(vec![
                        Value::name(b"Indexed"),
                        Value::name(b"DeviceRGB"),
                        Value::Int(3),
                        Value::String(
                            vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
                            cos::Form::Hex,
                        ),
                    ])),
                    1,
                    &[1, 2, 8],
                ),
                b"I3" => self.image(Some(Value::name(b"DeviceGray")), 1, &[1, 2, 4, 8, 16]),
                _ => self.image(Some(Value::name(b"DeviceCMYK")), 4, &[8]),
            };
            xobjects.push((name.to_vec(), Value::reference(num)));
        }
        out.push((b"XObject".to_vec(), Value::Dict(xobjects.clone())));

        let mut states: Entries = Vec::new();
        for name in STATES {
            if !self.c.flag() {
                continue;
            }
            let mut state: Entries = vec![
                (b"Type".to_vec(), Value::name(b"ExtGState")),
                (b"LW".to_vec(), self.number(0, 6)),
                (b"LC".to_vec(), Value::Int(self.c.range(0, 2))),
                (b"LJ".to_vec(), Value::Int(self.c.range(0, 2))),
                (b"CA".to_vec(), self.unit()),
                (b"ca".to_vec(), self.unit()),
                (b"BM".to_vec(), Value::name(self.c.pick(BLENDS))),
            ];
            if self.c.flag() {
                state.push((
                    b"D".to_vec(),
                    Value::Array(vec![
                        Value::Array(vec![self.number(1, 6), self.number(1, 6)]),
                        Value::Int(0),
                    ]),
                ));
            }
            let mask = xobjects.iter().find(|(name, _)| name == b"X2").cloned();
            let kind: &[u8] = if self.c.flag() {
                b"Luminosity"
            } else {
                b"Alpha"
            };
            match (mask, self.c.below(3)) {
                (Some((_, form)), 0) => state.push((
                    b"SMask".to_vec(),
                    Value::dict(vec![
                        (b"Type", Value::name(b"Mask")),
                        (b"S", Value::name(kind)),
                        (b"G", form),
                    ]),
                )),
                (_, 1) => state.push((b"SMask".to_vec(), Value::name(b"None"))),
                _ => {}
            }
            states.push((name.to_vec(), Value::Dict(state)));
        }
        out.push((b"ExtGState".to_vec(), Value::Dict(states)));

        let mut spaces: Entries = Vec::new();
        for name in SPACES {
            if !self.c.flag() {
                continue;
            }
            let space = match *name {
                b"C0" => Value::Array(vec![
                    Value::name(b"Indexed"),
                    Value::name(b"DeviceRGB"),
                    Value::Int(1),
                    Value::String(vec![0, 128, 255, 255, 128, 0], cos::Form::Hex),
                ]),
                b"C1" => {
                    let c0 = vec![Value::Int(0), Value::Int(0), Value::Int(0), Value::Int(0)];
                    let c1 = vec![self.unit(), self.unit(), self.unit(), self.unit()];
                    Value::Array(vec![
                        Value::name(b"Separation"),
                        Value::name(b"Spot"),
                        Value::name(b"DeviceCMYK"),
                        self.exponential(c0, c1),
                    ])
                }
                b"C2" => Value::Array(vec![Value::name(b"Pattern"), Value::name(b"DeviceRGB")]),
                b"C3" => {
                    // 7.10.5: two inputs to three outputs, a calculator.
                    let program = b"{ 2 copy add 2 div }".to_vec();
                    let dict: Entries = vec![
                        (b"FunctionType".to_vec(), Value::Int(4)),
                        (
                            b"Domain".to_vec(),
                            Value::Array(vec![
                                Value::Int(0),
                                Value::Int(1),
                                Value::Int(0),
                                Value::Int(1),
                            ]),
                        ),
                        (
                            b"Range".to_vec(),
                            Value::Array(vec![
                                Value::Int(0),
                                Value::Int(1),
                                Value::Int(0),
                                Value::Int(1),
                                Value::Int(0),
                                Value::Int(1),
                            ]),
                        ),
                    ];
                    let function = self.add(Body::Stream(Stream::new(dict, program)));
                    Value::Array(vec![
                        Value::name(b"DeviceN"),
                        Value::Array(names(&[b"A", b"B"])),
                        Value::name(b"DeviceRGB"),
                        Value::reference(function),
                    ])
                }
                b"C4" => Value::Array(vec![
                    Value::name(b"CalRGB"),
                    Value::dict(vec![
                        (
                            b"WhitePoint",
                            Value::Array(vec![real(9505, 4), Value::Int(1), real(1089, 3)]),
                        ),
                        (
                            b"Gamma",
                            Value::Array(vec![real(22, 1), real(22, 1), real(22, 1)]),
                        ),
                    ]),
                ]),
                _ => Value::Array(vec![
                    Value::name(b"Lab"),
                    Value::dict(vec![
                        (
                            b"WhitePoint",
                            Value::Array(vec![real(9505, 4), Value::Int(1), real(1089, 3)]),
                        ),
                        (
                            b"Range",
                            Value::Array(vec![
                                Value::Int(-100),
                                Value::Int(100),
                                Value::Int(-100),
                                Value::Int(100),
                            ]),
                        ),
                    ]),
                ]),
            };
            spaces.push((name.to_vec(), space));
        }
        out.push((b"ColorSpace".to_vec(), Value::Dict(spaces)));

        let mut patterns: Entries = Vec::new();
        if depth < MAX_DEPTH {
            for (name, coloured) in [(b"P0", true), (b"P1", false)] {
                if self.c.flag() {
                    let num = self.tiling(depth, resources, coloured);
                    patterns.push((name.to_vec(), Value::reference(num)));
                }
            }
        }
        if self.c.flag() {
            let radial = self.c.flag();
            let shading = self.shading(radial);
            patterns.push((
                b"P2".to_vec(),
                Value::dict(vec![
                    (b"PatternType", Value::Int(2)),
                    (b"Shading", shading),
                    (b"Matrix", Value::Array(self.matrix())),
                ]),
            ));
        }
        out.push((b"Pattern".to_vec(), Value::Dict(patterns)));

        let mut shadings: Entries = Vec::new();
        for (name, radial) in [(b"S0", false), (b"S1", true)] {
            if self.c.flag() {
                let shading = self.shading(radial);
                shadings.push((name.to_vec(), shading));
            }
        }
        out.push((b"Shading".to_vec(), Value::Dict(shadings)));

        out.push((
            b"Properties".to_vec(),
            Value::dict(vec![
                (b"M0", Value::dict(vec![(b"MCID", Value::Int(0))])),
                (
                    b"M1",
                    Value::dict(vec![(b"Alt", Value::literal(b"a property list"))]),
                ),
            ]),
        ));
        out
    }
}

/// A generated page, its document written clean, and render options chosen
/// with it.
pub fn page(c: &mut Choices) -> Page {
    let (objects, options, _) = model(c);
    Page {
        bytes: plainly(objects),
        options,
    }
}

/// A page's objects in one table, every token in its plainest spelling.
fn plainly(objects: Vec<Indirect>) -> Vec<u8> {
    let file = File {
        version: (1, 7),
        sections: vec![Section {
            objects,
            freed: Vec::new(),
            xref: Xref::Table { eol: 0 },
            trailer: vec![(b"Root".to_vec(), Value::reference(1))],
        }],
    };
    cos::write(&file, &mut Choices::new(&[])).bytes
}

/// A generated page to draw whole and in regions (ruling 5).
pub struct Tiled {
    /// The file, as [`page`] writes it.
    pub bytes: Vec<u8>,
    /// The whole render's options, generated with the page, with no region.
    pub options: RenderOptions,
    /// The regions to render at the same options, each anywhere from inside
    /// the page to past its far edges.
    pub regions: Vec<PixelRegion>,
    /// What may draw an image, and where.
    pub drawn: Drawn,
}

/// A page and the regions to draw it in, built from `c`: [`model`]'s page,
/// its options' own region set aside, then one to four regions at its scale.
pub fn tiled(c: &mut Choices) -> Tiled {
    let (objects, mut options, drawn) = model(c);
    options.region = None;
    // Past the widest page this scale draws, rotated or not, by a margin, so
    // a region can run off the far edges as the lattice's last tiles do.
    let reach = (MAX_PAGE as f64 * options.scale).ceil() as usize + 8;
    let regions = (0..1 + c.below(4))
        .map(|_| {
            let x = c.below(reach) as u32;
            let y = c.below(reach) as u32;
            let w = 1 + c.below(reach) as u32;
            let h = 1 + c.below(reach) as u32;
            PixelRegion::new(x, y, w, h)
        })
        .collect();
    Tiled {
        bytes: plainly(objects),
        options,
        regions,
        drawn,
    }
}

/// A generated page's objects — 1 the catalog, 2 the page tree, 3 the page,
/// 4 its resources, then every object they name, by number — render options
/// chosen with it, and the operators that may draw an image, counted where
/// they are written: what [`page`] writes, for a writer that lays the objects
/// out otherwise or draws the page otherwise.
pub fn model(c: &mut Choices) -> (Vec<Indirect>, RenderOptions, Drawn) {
    let width = 8 + c.range(0, MAX_PAGE - 8);
    let height = 8 + c.range(0, MAX_PAGE - 8);
    let mut generator = Gen {
        c,
        objects: Vec::new(),
        next: 5,
        budget: MAX_TOTAL_OPS,
        width,
        height,
        in_page: false,
        drawn: Drawn::default(),
    };
    // 1 the catalog, 2 the page tree, 3 the page, 4 the shared resources.
    let resources = 4;
    let shared = generator.resources(0, resources);
    generator.objects.push(Indirect {
        num: resources,
        gen: 0,
        body: Body::Value(Value::Dict(shared)),
    });

    let content = generator.content(0, false, true);
    let contents = if generator.c.flag() && content.len() > 2 {
        // 7.8.2: an array of streams is one stream, split at a line.
        let mid = content.len() / 2;
        let at = content[..mid]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(content.len(), |at| at + 1);
        let (first, second) = content.split_at(at);
        let one = generator.stream(Vec::new(), first.to_vec());
        let two = generator.stream(Vec::new(), second.to_vec());
        let one = generator.add(one);
        let two = generator.add(two);
        Value::Array(vec![Value::reference(one), Value::reference(two)])
    } else {
        let body = generator.stream(Vec::new(), content);
        Value::reference(generator.add(body))
    };

    let mut page: Entries = vec![
        (b"Type".to_vec(), Value::name(b"Page")),
        (b"Parent".to_vec(), Value::reference(2)),
        (
            b"MediaBox".to_vec(),
            Value::Array(vec![
                Value::Int(0),
                Value::Int(0),
                Value::Int(width),
                Value::Int(height),
            ]),
        ),
        (b"Resources".to_vec(), Value::reference(resources)),
        (b"Contents".to_vec(), contents),
    ];
    let rotate = *generator.c.pick(&[0, 0, 90, 180, 270]);
    if rotate != 0 {
        page.push((b"Rotate".to_vec(), Value::Int(rotate)));
    }
    if generator.c.below(4) == 3 {
        page.push((
            b"Group".to_vec(),
            Value::dict(vec![
                (b"S", Value::name(b"Transparency")),
                (b"CS", Value::name(b"DeviceRGB")),
            ]),
        ));
    }
    if generator.c.flag() {
        // An annotation whose appearance is a generated form, and one the
        // engine draws from its own dictionary (12.5.5).
        let appearance = generator.form(0, resources, false);
        let rect = generator.rect();
        let mut annots = vec![Value::dict(vec![
            (b"Type", Value::name(b"Annot")),
            (b"Subtype", Value::name(b"Square")),
            (b"Rect", rect),
            (
                b"AP",
                Value::dict(vec![(b"N", Value::reference(appearance))]),
            ),
        ])];
        if generator.c.flag() {
            let rect = generator.rect();
            annots.push(Value::dict(vec![
                (b"Type", Value::name(b"Annot")),
                (b"Subtype", Value::name(b"Square")),
                (b"Rect", rect),
                (
                    b"C",
                    Value::Array(vec![Value::Int(1), Value::Int(0), Value::Int(0)]),
                ),
            ]));
        }
        page.push((b"Annots".to_vec(), Value::Array(annots)));
    }

    let mut objects = vec![
        Indirect {
            num: 1,
            gen: 0,
            body: Body::Value(Value::dict(vec![
                (b"Type", Value::name(b"Catalog")),
                (b"Pages", Value::reference(2)),
            ])),
        },
        Indirect {
            num: 2,
            gen: 0,
            body: Body::Value(Value::dict(vec![
                (b"Type", Value::name(b"Pages")),
                (b"Kids", Value::Array(vec![Value::reference(3)])),
                (b"Count", Value::Int(1)),
            ])),
        },
        Indirect {
            num: 3,
            gen: 0,
            body: Body::Value(Value::Dict(page)),
        },
    ];
    let options = options(generator.c, width, height);
    let mut rest = std::mem::take(&mut generator.objects);
    rest.sort_by_key(|object| object.num);
    objects.extend(rest);
    (objects, options, generator.drawn)
}

/// Render options for a page of `width` by `height` points.
fn options(c: &mut Choices, width: i64, height: i64) -> RenderOptions {
    let scale = *c.pick(&[1.0, 0.5, 2.0, 1.5, 0.25, 3.0]);
    let format = *c.pick(&[
        PixelFormat::Rgb8,
        PixelFormat::Rgba8,
        PixelFormat::Gray8,
        PixelFormat::GrayA8,
        PixelFormat::CmykA8,
    ]);
    let mut options = RenderOptions {
        scale,
        format,
        ..RenderOptions::default()
    };
    options.allow_cmyk = format == PixelFormat::CmykA8 && c.flag();
    options.antialias = c.below(4) != 3;
    options.annotations = c.below(4) != 3;
    options.transparent = c.flag();
    options.premultiplied = c.flag();
    if c.flag() {
        let across = (width as f64 * scale).ceil() as u32 + 4;
        let down = (height as f64 * scale).ceil() as u32 + 4;
        let x = c.below(across as usize) as u32;
        let y = c.below(down as usize) as u32;
        let w = 1 + c.below(across as usize) as u32;
        let h = 1 + c.below(down as usize) as u32;
        options.region = Some(PixelRegion::new(x, y, w, h));
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_input_makes_a_page_that_opens() {
        for input in [&b""[..], b"\x01\x02\x03", &[0xff; 64], &[7; 300]] {
            let page = page(&mut Choices::new(input));
            let doc = tinker_pdf::Document::open(page.bytes).expect("a generated page opens");
            assert_eq!(doc.page_count(), 1);
        }
    }

    fn holds(bytes: &[u8], needle: &[u8]) -> bool {
        bytes.windows(needle.len()).any(|w| w == needle)
    }

    /// Rewrites `fuzz/corpus/content_stream/`: one seed per feature, each the
    /// first input of `first_input`'s sequence whose page has it.
    #[test]
    #[ignore = "writes fuzz/corpus/content_stream"]
    fn write_the_fuzz_seeds() {
        type Feature = fn(&Page) -> bool;
        let features: &[(&str, Feature)] = &[
            ("type3-text", |p| {
                holds(&p.bytes, b"/T3 ") && holds(&p.bytes, b"/Type3") && holds(&p.bytes, b"Tj\n")
            }),
            ("forms-in-groups", |p| {
                holds(&p.bytes, b"/Subtype/Form")
                    && holds(&p.bytes, b"/S/Transparency")
                    && holds(&p.bytes, b"Do\n")
            }),
            ("inline-images", |p| {
                holds(&p.bytes, b"BI\n") && holds(&p.bytes, b"/IM true")
            }),
            ("image-xobjects", |p| {
                [b"/I0 ".as_slice(), b"/I1 ", b"/I2 ", b"/I3 ", b"/I4 "]
                    .iter()
                    .filter(|name| holds(&p.bytes, name))
                    .count()
                    >= 4
            }),
            ("patterns-and-shadings", |p| {
                holds(&p.bytes, b"/PatternType 1")
                    && holds(&p.bytes, b"/PatternType 2")
                    && holds(&p.bytes, b"sh\n")
            }),
            ("soft-mask", |p| holds(&p.bytes, b"/SMask<<")),
            ("annotations", |p| {
                holds(&p.bytes, b"/Annots") && holds(&p.bytes, b"/AP<<")
            }),
            ("marked-content", |p| {
                holds(&p.bytes, b"BDC\n") && holds(&p.bytes, b"EMC\n")
            }),
            ("two-contents", |p| holds(&p.bytes, b"/Contents[")),
            ("region-in-ink", |p| {
                p.options.region.is_some()
                    && p.options.allow_cmyk
                    && p.options.format == PixelFormat::CmykA8
            }),
            ("colour-spaces", |p| {
                holds(&p.bytes, b"/DeviceN")
                    && holds(&p.bytes, b"/Lab")
                    && holds(&p.bytes, b"/Separation")
            }),
        ];
        let read = |data: &[u8]| {
            let mut choices = Choices::new(data);
            let _ = page(&mut choices);
            choices.consumed()
        };
        let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
        for (name, has) in features {
            let seed = crate::first_input(
                1500,
                100_000,
                |data| has(&page(&mut Choices::new(data))),
                read,
                &seeds
                    .iter()
                    .map(|(_, seed)| seed.clone())
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|| panic!("no input has {name}"));
            assert!(
                has(&page(&mut Choices::new(&seed))),
                "{name} lost its feature when cut"
            );
            seeds.push((name, seed));
        }
        crate::write_seeds("content_stream", &seeds);
    }

    #[test]
    fn the_plainest_page_paints_its_rectangle() {
        let page = page(&mut Choices::new(&[]));
        let doc = tinker_pdf::Document::open(page.bytes).expect("it opens");
        let bitmap = doc
            .page(0)
            .expect("one page")
            .render(&RenderOptions::default());
        assert!(
            bitmap.data.iter().any(|b| *b != 255),
            "the opening rectangle drew nothing"
        );
    }

    fn inputs(count: usize, longest: usize) -> impl Iterator<Item = Vec<u8>> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        (0..count).map(move |n| {
            let len = (n * 101) % longest;
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

    /// `Do` may draw an image unless what it names is one of the forms, and
    /// no other operator draws one through its operands.
    #[test]
    fn an_operator_may_draw_an_image_only_as_do_naming_no_form() {
        assert!(draws_image(b"Do", &[Value::name(b"I0")]));
        assert!(
            draws_image(b"Do", &[Value::name(b"Foo")]),
            "unknown names count"
        );
        assert!(draws_image(
            b"Do",
            &[Value::name(b"X0"), Value::name(b"I3")]
        ));
        assert!(!draws_image(b"Do", &[Value::name(b"X2")]));
        assert!(!draws_image(b"Do", &[Value::Int(1)]));
        assert!(!draws_image(b"Do", &[]));
        assert!(!draws_image(b"Tj", &[Value::name(b"I0")]));
    }

    /// The plainest input tiles the plainest page, in one region of one pixel
    /// at its corner, and draws no image.
    #[test]
    fn the_empty_input_tiles_its_plainest_page() {
        let tiled = tiled(&mut Choices::new(&[]));
        assert_eq!(tiled.bytes, page(&mut Choices::new(&[])).bytes);
        assert!(tiled.options.region.is_none());
        assert_eq!(tiled.regions, vec![PixelRegion::new(0, 0, 1, 1)]);
        assert_eq!(tiled.drawn, Drawn::default());
    }

    /// Images are counted in the page's own content and in what it reaches,
    /// and enough pages draw at most one, in the page, to tile.
    #[test]
    fn image_draws_are_counted_where_they_are_written() {
        let all: Vec<Drawn> = inputs(800, 2000)
            .map(|input| tiled(&mut Choices::new(&input)).drawn)
            .collect();
        assert!(all.iter().any(|d| d.page > 1));
        assert!(all.iter().any(|d| d.nested > 0));
        let tiling = all.iter().filter(|d| d.page <= 1 && d.nested == 0).count();
        assert!(tiling >= 80, "only {tiling} of 800 pages tile");
    }

    /// `render_tile`'s relation, on stable: every region of a page that draws
    /// no image run is the whole render's rectangle there, byte for byte.
    #[test]
    fn every_region_is_the_page_under_it() {
        let mut compared = 0;
        for (n, input) in inputs(200, 2000).enumerate() {
            let tiled = tiled(&mut Choices::new(&input));
            if tiled.drawn.page > 1 || tiled.drawn.nested > 0 {
                continue;
            }
            let doc = tinker_pdf::Document::open(tiled.bytes).expect("it opens");
            let page = doc.page(0).expect("one page");
            let whole = page.render(&tiled.options);
            let (width, height) = page.pixel_size(&tiled.options);
            assert_eq!((whole.width, whole.height), (width, height), "input {n}");
            let components = whole.components();
            for region in &tiled.regions {
                let tile = page.render(&RenderOptions {
                    region: Some(*region),
                    ..tiled.options.clone()
                });
                let at = region.clamped_to(width, height);
                assert_eq!(
                    (tile.width, tile.height),
                    (at.width, at.height),
                    "input {n}"
                );
                let span = tile.width as usize * components;
                for y in 0..tile.height as usize {
                    let mine = &tile.data[y * tile.stride..y * tile.stride + span];
                    let from = (at.y as usize + y) * whole.stride + at.x as usize * components;
                    assert!(mine == &whole.data[from..from + span], "input {n}, row {y}");
                }
            }
            compared += 1;
        }
        assert!(compared >= 20, "only {compared} pages were compared");
    }

    /// `render_tile`'s seeds, beside `content_stream`'s, under the writers'
    /// one name.
    mod render_tile {
        use super::*;

        /// Rewrites `fuzz/corpus/render_tile/`: one seed per feature, each the
        /// first input of `first_input`'s sequence whose page has it and tiles.
        #[test]
        #[ignore = "writes fuzz/corpus/render_tile"]
        fn write_the_fuzz_seeds() {
            type Feature = fn(&Tiled) -> bool;
            let features: &[(&str, Feature)] = &[
                ("four-regions", |t| t.regions.len() == 4),
                ("one-image", |t| t.drawn.page == 1),
                ("rotated", |t| holds(&t.bytes, b"/Rotate")),
                ("transparency-group", |t| holds(&t.bytes, b"/Transparency")),
                ("annotations", |t| {
                    holds(&t.bytes, b"/Annots") && t.options.annotations
                }),
                ("type3-text", |t| {
                    holds(&t.bytes, b"/Type3") && holds(&t.bytes, b"Tj")
                }),
                ("patterns-and-shadings", |t| {
                    holds(&t.bytes, b"/PatternType") && holds(&t.bytes, b"/ShadingType")
                }),
                ("quarter-scale", |t| t.options.scale < 0.5),
                ("triple-scale", |t| t.options.scale > 2.5),
                ("cmyk-ink", |t| t.options.allow_cmyk),
                ("no-anti-aliasing", |t| !t.options.antialias),
                ("transparent-premultiplied", |t| {
                    t.options.transparent && t.options.premultiplied
                }),
            ];
            let tiles = |t: &Tiled| t.drawn.page <= 1 && t.drawn.nested == 0;
            let read = |data: &[u8]| {
                let mut choices = Choices::new(data);
                let _ = tiled(&mut choices);
                choices.consumed()
            };
            let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
            for (name, has) in features {
                let seed = crate::first_input(
                    2000,
                    200_000,
                    |data| {
                        let t = tiled(&mut Choices::new(data));
                        has(&t) && tiles(&t)
                    },
                    read,
                    &seeds
                        .iter()
                        .map(|(_, seed)| seed.clone())
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_else(|| panic!("no input has {name}"));
                assert!(
                    has(&tiled(&mut Choices::new(&seed))),
                    "{name} lost its feature when cut"
                );
                seeds.push((name, seed));
            }
            crate::write_seeds("render_tile", &seeds);
        }
    }
}
