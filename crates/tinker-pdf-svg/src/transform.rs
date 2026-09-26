//! SVG 1.1 §7.6's `transform` attribute, and the §7.7 `viewBox` mapping.
//!
//! A matrix here is `[a, b, c, d, e, f]`, which is the order both SVG and PDF
//! write it in and the order `tinker_pdf_cos`'s builder takes — so a caller
//! never transposes one.

use tinker_pdf_math as math;

/// The identity.
pub const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `inner` then `outer`, which is the order a nested `transform` composes in:
/// a child's own matrix applies first, in its parent's space.
#[must_use]
pub fn concat(inner: [f64; 6], outer: [f64; 6]) -> [f64; 6] {
    [
        inner[0] * outer[0] + inner[1] * outer[2],
        inner[0] * outer[1] + inner[1] * outer[3],
        inner[2] * outer[0] + inner[3] * outer[2],
        inner[2] * outer[1] + inner[3] * outer[3],
        inner[4] * outer[0] + inner[5] * outer[2] + outer[4],
        inner[4] * outer[1] + inner[5] * outer[3] + outer[5],
    ]
}

/// A point through a matrix.
#[must_use]
pub fn apply(matrix: [f64; 6], point: [f64; 2]) -> [f64; 2] {
    [
        matrix[0] * point[0] + matrix[2] * point[1] + matrix[4],
        matrix[1] * point[0] + matrix[3] * point[1] + matrix[5],
    ]
}

/// Reads §7.6's transform list.
///
/// Returns `None` for anything that is not the grammar — a caller turns that
/// into [`crate::Warning::ValueUnreadable`] rather than into an identity,
/// because a transform silently dropped moves a shape somewhere the file did
/// not put it, which is a picture that looks right and is not.
///
/// The list is applied **left to right**, and §7.6 says the leftmost is the
/// outermost: `translate(10,0) scale(2)` scales first and then translates, so
/// each new function composes as the *outer* of what came before.
#[must_use]
pub fn list(text: &str) -> Option<[f64; 6]> {
    let mut out = IDENTITY;
    let mut rest = text.trim();
    if rest.is_empty() {
        return Some(out);
    }
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim();
        let close = rest[open..].find(')')? + open;
        let numbers = numbers(&rest[open + 1..close])?;
        let matrix = function(name, &numbers)?;
        out = concat(matrix, out);
        rest = rest[close + 1..].trim_start_matches([',', ' ', '\t', '\r', '\n']);
    }
    Some(out)
}

/// One transform function, as its own matrix.
fn function(name: &str, n: &[f64]) -> Option<[f64; 6]> {
    Some(match (name, n.len()) {
        ("matrix", 6) => [n[0], n[1], n[2], n[3], n[4], n[5]],
        ("translate", 1) => [1.0, 0.0, 0.0, 1.0, n[0], 0.0],
        ("translate", 2) => [1.0, 0.0, 0.0, 1.0, n[0], n[1]],
        // §7.6: one number scales both axes equally.
        ("scale", 1) => [n[0], 0.0, 0.0, n[0], 0.0, 0.0],
        ("scale", 2) => [n[0], 0.0, 0.0, n[1], 0.0, 0.0],
        ("rotate", 1) => {
            let (sin, cos) = (
                math::sin(math::to_radians(n[0])),
                math::cos(math::to_radians(n[0])),
            );
            [cos, sin, -sin, cos, 0.0, 0.0]
        }
        // §7.6's three-argument form is a rotation about a point, which is a
        // translate, a rotate and the inverse translate — written out rather
        // than composed, because composing it here would be three matrix
        // multiplications to produce four numbers that are already known.
        ("rotate", 3) => {
            let (sin, cos) = (
                math::sin(math::to_radians(n[0])),
                math::cos(math::to_radians(n[0])),
            );
            let (x, y) = (n[1], n[2]);
            [
                cos,
                sin,
                -sin,
                cos,
                x - cos * x + sin * y,
                y - sin * x - cos * y,
            ]
        }
        ("skewX", 1) => [1.0, 0.0, math::tan(math::to_radians(n[0])), 1.0, 0.0, 0.0],
        ("skewY", 1) => [1.0, math::tan(math::to_radians(n[0])), 0.0, 1.0, 0.0, 0.0],
        _ => return None,
    })
}

/// A whitespace- or comma-separated number list.
///
/// SVG's number grammar is not Rust's: `1e3`, `.5`, `+2` and `-.5e-2` are all
/// numbers, and `1-2` is *two* of them because a sign begins a new one where a
/// separator would.
#[must_use]
pub fn numbers(text: &str) -> Option<Vec<f64>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        while at < bytes.len() && matches!(bytes[at], b' ' | b'\t' | b'\r' | b'\n' | b',') {
            at += 1;
        }
        if at >= bytes.len() {
            return Some(out);
        }
        let start = at;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let mut digits = 0usize;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
            digits += 1;
        }
        if bytes.get(at) == Some(&b'.') {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        if matches!(bytes.get(at), Some(b'e' | b'E')) {
            let mark = at;
            at += 1;
            if matches!(bytes.get(at), Some(b'+' | b'-')) {
                at += 1;
            }
            let mut exponent = 0usize;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
                exponent += 1;
            }
            // `1e` is the number one followed by rubbish, not a bad number:
            // the exponent is only part of the token if it has digits.
            if exponent == 0 {
                at = mark;
            }
        }
        let value: f64 = text.get(start..at)?.parse().ok()?;
        if !value.is_finite() {
            return None;
        }
        out.push(value);
    }
}

/// §7.7's `viewBox` mapping, with `preserveAspectRatio` read.
///
/// Returns the matrix that maps the view box onto a viewport of `width` by
/// `height`. `None` when the view box is degenerate, which §7.7 says disables
/// rendering of the element entirely — a different answer from an identity.
#[must_use]
pub fn view_box(
    box_: [f64; 4],
    width: f64,
    height: f64,
    preserve: Option<&str>,
) -> Option<[f64; 6]> {
    let [min_x, min_y, box_width, box_height] = box_;
    if box_width <= 0.0 || box_height <= 0.0 || !width.is_finite() || !height.is_finite() {
        return None;
    }
    let (align, slice) = aspect(preserve);
    let (scale_x, scale_y) = (width / box_width, height / box_height);
    if let Some(uniform) = align {
        // `meet` fits the whole box inside the viewport, `slice` fills the
        // viewport and lets the box overflow. One is the smaller scale and the
        // other the larger, which is the whole of the difference.
        let scale = if slice {
            scale_x.max(scale_y)
        } else {
            scale_x.min(scale_y)
        };
        let extra_x = width - box_width * scale;
        let extra_y = height - box_height * scale;
        let (fraction_x, fraction_y) = uniform;
        return Some([
            scale,
            0.0,
            0.0,
            scale,
            -min_x * scale + extra_x * fraction_x,
            -min_y * scale + extra_y * fraction_y,
        ]);
    }
    Some([
        scale_x,
        0.0,
        0.0,
        scale_y,
        -min_x * scale_x,
        -min_y * scale_y,
    ])
}

/// `preserveAspectRatio`, as the fraction of the leftover to put before the
/// box on each axis, and whether the meaning is `slice`.
///
/// `None` for the alignment means `none` — stretch each axis independently,
/// which is the only value that does not preserve the ratio at all.
fn aspect(text: Option<&str>) -> (Option<(f64, f64)>, bool) {
    // §7.8: the initial value is `xMidYMid meet`.
    let Some(text) = text else {
        return (Some((0.5, 0.5)), false);
    };
    let mut words = text.split_ascii_whitespace();
    let mut align = words.next().unwrap_or("xMidYMid");
    // The `defer` keyword is only meaningful on `<image>`, and this crate does
    // not resolve one — so it is skipped rather than refused.
    if align == "defer" {
        align = words.next().unwrap_or("xMidYMid");
    }
    let slice = words.next() == Some("slice");
    if align == "none" {
        return (None, slice);
    }
    let fraction = |which: &str| match which {
        "Min" => 0.0,
        "Max" => 1.0,
        _ => 0.5,
    };
    let x = align.get(1..4).unwrap_or("Mid");
    let y = align.get(5..8).unwrap_or("Mid");
    (Some((fraction(x), fraction(y))), slice)
}
