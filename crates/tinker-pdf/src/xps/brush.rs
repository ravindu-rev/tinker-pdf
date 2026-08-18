//! ECMA-388 15's colours and brushes (gap 30, milestone 6).
//!
//! # Two spellings of every colour, from one corpus
//!
//! Milestone 1 measured `Fill="#FF000000"` and `Color="#FFDC143C"` from WPF
//! against `Fill="#000000"` and `Color="#dc143c"` from the XPS object model —
//! six digits, lower case — for the **same page**. So a reader that takes one
//! length or one case refuses half of what Microsoft writes. 15.2.4 adds two
//! more forms nobody in the corpus uses and a real producer may:
//! `#RGB`/`#ARGB`, and `sc#` with floating-point scRGB components.
//!
//! # The asymmetry this module lives on
//!
//! Gap 30's design section splits an element's failure in two, and the halves
//! are not symmetric:
//!
//! - **geometry unreadable** — the element is *not painted*, because a missing
//!   shape is visible as a missing shape;
//! - **paint unreadable** — the element *is* painted, in the neutral
//!   placeholder grey, because the shape and its position are known and only
//!   the colour is not.
//!
//! That is [07](../../../../docs/plans/gaps/07-stroked-patterns.md)'s lesson:
//! that gap's headline defect was a gradient-stroked rule painting **solid
//! black, silently**, and black is a plausible colour where the placeholder
//! grey is not. Everything in this module that cannot be painted returns
//! [`BrushError`] and the caller paints grey and warns *by name*, and the
//! names are distinct — an unresolved `{StaticResource}`, an `ImageBrush` this
//! milestone does not draw and a colour string that is not one are three
//! different things about a document and collapsing them would make the report
//! useless for the one question it exists to answer.

use tinker_pdf_cos::{DeviceSpace, Function, Shading};

use super::markup::{self, Node};

/// How many periods of a repeating gradient are actually drawn.
///
/// PDF 8.7.4.5.3 has `/Extend` and no repeat, so `Reflect` and `Repeat` are
/// built by replicating the function along an extended axis. Eight periods
/// each way is 17 copies of a linear gradient and 9 of a radial, which is
/// past the edge of any page a gradient of that period could be stated on —
/// and past that the `/Extend` pads, which is what `Pad` does anyway.
const SPREAD_COPIES: usize = 8;

/// An sRGB colour with an alpha, which is every colour 15.2.4 can spell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour {
    /// Red, green and blue, each from zero to one.
    pub rgb: [f64; 3],
    /// Alpha, from zero to one. 15.2.4's `#RRGGBB` form has none and means
    /// one.
    pub alpha: f64,
}

/// Why a brush did not become paint.
///
/// Two variants and not one, because the caller reports them under different
/// names and a reader of the report has to be able to tell "this file says
/// something I cannot read" from "this file says something this build does not
/// draw yet".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushError {
    /// The markup is not 15's brush syntax.
    Syntax,
    /// It is a brush and this milestone does not paint it: an `ImageBrush` or
    /// a `VisualBrush` (both gap 30 milestone 8), or a `ContextColor` naming
    /// an ICC profile (a non-goal of the whole plan, because 15.2.5's syntax
    /// has nowhere to put an sRGB fallback).
    Unsupported,
}

/// What an element paints with.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// A colour, as `r g b`.
    Solid([f64; 3]),
    /// A gradient, and the matrix mapping its own space into the element's.
    Gradient {
        /// The shading, ready for [`tinker_pdf_cos::DocumentBuilder::add_shading`].
        shading: Shading,
        /// Shading space into the space the element draws in.
        matrix: [f64; 6],
    },
    /// An `ImageBrush` (15.3), which becomes a PDF tiling pattern.
    ///
    /// The `ImageSource` is carried **unresolved**, because resolving it needs
    /// the package and this module is pure -- [`super::image::Images`] holds the
    /// table and [`super::paint`] does the lookup. That split is not tidiness:
    /// `Package::read_part` hands back a borrow the painter is already holding,
    /// which is why images are resolved in a pass before the drawing walk.
    Image(Box<ImageTile>),
}

/// An `ImageBrush`, parsed but not resolved (15.3).
///
/// Both rectangles are kept, and they are **two different spaces**. `viewbox`
/// is stated in the image's own units, so a 384-pixel PNG at 96 dpi is four
/// units across; `viewport` is stated in the element's. The mapping between
/// them is what becomes the pattern's `/Matrix`, and a build that swapped them
/// would draw the picture at the reciprocal of its intended size -- which is why
/// they are separate fields with separate tests rather than one `[f64; 8]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageTile {
    /// `ImageSource`, verbatim. Resolved against the fixed page part's own name.
    pub source: String,
    /// `Viewbox`, as `x y width height` in the image's own units.
    pub viewbox: [f64; 4],
    /// `Viewport`, as `x y width height` in the element's units.
    pub viewport: [f64; 4],
    /// Whether `Viewbox` is absolute or a fraction of the image.
    pub viewbox_units: Units,
    /// Whether `Viewport` is absolute or a fraction of the filled box.
    pub viewport_units: Units,
    /// `TileMode` (15.3.1).
    pub tile: TileMode,
    /// `Transform`, which maps the brush's space into the element's.
    pub transform: [f64; 6],
}

/// 15.3's `ViewboxUnits` and `ViewportUnits`.
///
/// Two independent attributes with one grammar. Milestone 1 measured
/// `Absolute` on both in all eight real packages, so the relative form is
/// exercised only by fixtures this repository wrote -- recorded rather than
/// left for a reader to assume otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    /// The rectangle is stated in the space it names -- image units for a
    /// viewbox, element units for a viewport.
    Absolute,
    /// The rectangle is a fraction of that space's own extent.
    RelativeToBoundingBox,
}

/// 15.3.1's `TileMode`, in all five spellings.
///
/// Five values and **five rules**: `None` draws the image once and leaves the
/// rest of the shape unpainted, `Tile` repeats it, and the three flips reflect
/// alternate cells. PDF's tiling patterns have no flip at all (8.7.3.1 gives a
/// cell, two steps and a matrix), so a flip has to be built by making the
/// *cell* four times the image and drawing the image into it four ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileMode {
    /// Draw the image once and leave the rest of the shape unpainted.
    None,
    /// Repeat it in both directions.
    Tile,
    /// Repeat it, mirroring alternate columns.
    FlipX,
    /// Repeat it, mirroring alternate rows.
    FlipY,
    /// Repeat it, mirroring alternate columns and rows.
    FlipXY,
}

/// A brush, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    /// What it paints.
    pub paint: Paint,
    /// The constant alpha to apply, from the colour's own alpha and the
    /// brush's `Opacity` multiplied together.
    pub alpha: f64,
    /// Whether something about the brush reached the page **approximately**:
    /// gradient stops whose alphas differ from each other, which one constant
    /// alpha cannot express, or a `ColorInterpolationMode` this build does not
    /// interpolate in. Reported by name rather than left silent.
    pub approximated: bool,
}

impl Brush {
    /// An opaque solid colour.
    fn solid(colour: Colour) -> Brush {
        Brush {
            paint: Paint::Solid(colour.rgb),
            alpha: colour.alpha,
            approximated: false,
        }
    }
}

// ---- 15.2.4, colours --------------------------------------------------------

/// Reads a colour.
///
/// # Errors
/// [`BrushError::Syntax`] for anything that is not 15.2.4's grammar,
/// [`BrushError::Unsupported`] for `ContextColor`.
pub fn colour(text: &str) -> Result<Colour, BrushError> {
    let text = text.trim();
    // 15.2.5's `ContextColor <uri> a,c1,…` carries a profile part and channel
    // values and **no sRGB fallback**, so there is no cheap approximation to
    // take: the shape degrades under ruling 2 rather than being guessed at.
    if text.starts_with("ContextColor") {
        return Err(BrushError::Unsupported);
    }
    if let Some(values) = text.strip_prefix("sc#") {
        return sc_rgb(values);
    }
    let Some(digits) = text.strip_prefix('#') else {
        return Err(BrushError::Syntax);
    };
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(BrushError::Syntax);
    }
    let nibble = |at: usize| -> f64 {
        f64::from(
            digits
                .as_bytes()
                .get(at)
                .and_then(|b| char::from(*b).to_digit(16))
                .unwrap_or(0),
        )
    };
    let short = |at: usize| nibble(at) * 17.0 / 255.0;
    let long = |at: usize| (nibble(at) * 16.0 + nibble(at + 1)) / 255.0;
    match digits.len() {
        3 => Ok(Colour {
            rgb: [short(0), short(1), short(2)],
            alpha: 1.0,
        }),
        4 => Ok(Colour {
            rgb: [short(1), short(2), short(3)],
            alpha: short(0),
        }),
        6 => Ok(Colour {
            rgb: [long(0), long(2), long(4)],
            alpha: 1.0,
        }),
        8 => Ok(Colour {
            rgb: [long(2), long(4), long(6)],
            alpha: long(0),
        }),
        _ => Err(BrushError::Syntax),
    }
}

/// 15.2.4's `sc#` form: three or four **scRGB** components as reals.
///
/// scRGB is linear-light and its components legitimately run outside `[0, 1]`,
/// so a reader that clamped first and converted second would move every
/// out-of-gamut colour to a different one than the transfer function does.
/// The alpha is *not* transferred — it is a coverage and not a light level.
fn sc_rgb(values: &str) -> Result<Colour, BrushError> {
    let numbers = markup::numbers(values).ok_or(BrushError::Syntax)?;
    let (alpha, components) = match numbers.as_slice() {
        [r, g, b] => (1.0, [*r, *g, *b]),
        [a, r, g, b] => (a.clamp(0.0, 1.0), [*r, *g, *b]),
        _ => return Err(BrushError::Syntax),
    };
    Ok(Colour {
        rgb: components.map(srgb_transfer),
        alpha,
    })
}

/// IEC 61966-2-1's transfer function, linear light to sRGB.
fn srgb_transfer(linear: f64) -> f64 {
    let value = linear.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

// ---- 15.1, brushes ----------------------------------------------------------

/// Reads a brush from an attribute value — the `Fill="#FF000000"` form.
///
/// A `{StaticResource …}` never reaches here: the caller resolves it first,
/// because the answer is an *element* and not a string.
///
/// # Errors
/// As [`colour`].
pub fn from_attribute(text: &str) -> Result<Brush, BrushError> {
    colour(text).map(Brush::solid)
}

/// Reads a brush from an element — the `<SolidColorBrush …/>` form and the
/// two gradients.
///
/// `bbox` is the box a `MappingMode="RelativeToBoundingBox"` gradient is
/// stated in fractions of, which is the geometry the brush is about to fill.
/// `None` where there is no such box, which makes a relative gradient
/// unreadable rather than a gradient over a box this reader invented.
///
/// # Errors
/// [`BrushError::Syntax`] for markup that is not 15's, and
/// [`BrushError::Unsupported`] for a brush kind this milestone does not paint.
pub fn from_node(node: &Node, bbox: Option<[f64; 4]>) -> Result<Brush, BrushError> {
    if !node.xps {
        return Err(BrushError::Syntax);
    }
    match node.local.as_str() {
        "SolidColorBrush" => {
            let mut brush = Brush::solid(colour(node.attr("Color").ok_or(BrushError::Syntax)?)?);
            brush.alpha *= opacity_of(node)?;
            Ok(brush)
        }
        "LinearGradientBrush" | "RadialGradientBrush" => gradient(node, bbox),
        "ImageBrush" => image_brush(node, bbox),
        // `VisualBrush` is row 8's and is not built. Its cell is a *subtree* of
        // markup rather than a part, so painting one means re-entering the
        // drawing walk from inside a brush -- with 18.2's cross-part depth to
        // carry -- and this module is deliberately pure. Refused by name rather
        // than drawn as its first child, which would be a picture the file
        // never described. See the plan's amended row 8.
        "VisualBrush" => Err(BrushError::Unsupported),
        _ => Err(BrushError::Syntax),
    }
}

/// Reads an `ImageBrush` (15.3).
///
/// `bbox` is the box a `RelativeToBoundingBox` viewport is stated in fractions
/// of -- the geometry about to be filled. `None` makes a relative viewport
/// unreadable rather than a picture placed over a box this reader invented,
/// which is `gradient`'s rule and for the same reason.
fn image_brush(node: &Node, bbox: Option<[f64; 4]>) -> Result<Brush, BrushError> {
    let source = node.attr("ImageSource").ok_or(BrushError::Syntax)?;
    // 15.3 makes both rectangles required on an `ImageBrush`; there is no
    // sensible default for "which part of the image" or "where it goes".
    let viewbox = rect(node.attr("Viewbox").ok_or(BrushError::Syntax)?)?;
    let viewport = rect(node.attr("Viewport").ok_or(BrushError::Syntax)?)?;
    let viewbox_units = units_of(node, "ViewboxUnits")?;
    let viewport_units = units_of(node, "ViewportUnits")?;
    if viewport_units == Units::RelativeToBoundingBox && bbox.is_none() {
        return Err(BrushError::Syntax);
    }
    let tile = match node.attr("TileMode") {
        None | Some("None") => TileMode::None,
        Some("Tile") => TileMode::Tile,
        Some("FlipX") => TileMode::FlipX,
        Some("FlipY") => TileMode::FlipY,
        Some("FlipXY") => TileMode::FlipXY,
        Some(_) => return Err(BrushError::Syntax),
    };
    let transform = transform_of(node)?;

    // A viewbox or viewport with no extent paints nothing at all, and a
    // pattern whose cell is degenerate is one the writer refuses anyway. Said
    // here so the refusal names the brush rather than the pattern.
    if viewbox[2] <= 0.0 || viewbox[3] <= 0.0 || viewport[2] <= 0.0 || viewport[3] <= 0.0 {
        return Err(BrushError::Syntax);
    }

    Ok(Brush {
        paint: Paint::Image(Box::new(ImageTile {
            source: source.to_string(),
            viewbox,
            viewport,
            viewbox_units,
            viewport_units,
            tile,
            transform,
        })),
        alpha: opacity_of(node)?,
        approximated: false,
    })
}

/// `x,y,width,height`, which 15.3 writes as four numbers.
fn rect(text: &str) -> Result<[f64; 4], BrushError> {
    let values = markup::numbers(text).ok_or(BrushError::Syntax)?;
    if values.len() != 4 || !values.iter().all(|v| v.is_finite()) {
        return Err(BrushError::Syntax);
    }
    Ok([values[0], values[1], values[2], values[3]])
}

/// One of the two `*Units` attributes.
fn units_of(node: &Node, attribute: &str) -> Result<Units, BrushError> {
    match node.attr(attribute) {
        None | Some("RelativeToBoundingBox") => Ok(Units::RelativeToBoundingBox),
        Some("Absolute") => Ok(Units::Absolute),
        Some(_) => Err(BrushError::Syntax),
    }
}

/// A brush's own `Transform`, in either the attribute or the element form.
fn transform_of(node: &Node) -> Result<[f64; 6], BrushError> {
    if let Some(text) = node.attr("Transform") {
        return markup::matrix(text).ok_or(BrushError::Syntax);
    }
    let wanted = format!("{}.Transform", node.local);
    let Some(property) = node
        .children
        .iter()
        .find(|child| child.xps && child.local == wanted)
    else {
        return Ok([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    };
    let matrix = property
        .children
        .iter()
        .find(|child| child.xps && child.local == "MatrixTransform")
        .ok_or(BrushError::Syntax)?;
    markup::matrix(matrix.attr("Matrix").ok_or(BrushError::Syntax)?).ok_or(BrushError::Syntax)
}

/// 15.1's `Opacity`, which every brush may carry and which defaults to one.
fn opacity_of(node: &Node) -> Result<f64, BrushError> {
    match node.attr("Opacity") {
        None => Ok(1.0),
        Some(text) => {
            let value = markup::number(text).ok_or(BrushError::Syntax)?;
            (0.0..=1.0)
                .contains(&value)
                .then_some(value)
                .ok_or(BrushError::Syntax)
        }
    }
}

/// 15.4.1's `SpreadMethod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spread {
    Pad,
    Reflect,
    Repeat,
}

fn spread_of(node: &Node) -> Result<Spread, BrushError> {
    // The object model drops the attribute when it is the default and WPF
    // writes it out, so both packages of one page disagree about whether it is
    // there at all. Absent means `Pad`.
    match node.attr("SpreadMethod") {
        None => Ok(Spread::Pad),
        Some(text) => match text.trim() {
            "Pad" => Ok(Spread::Pad),
            "Reflect" => Ok(Spread::Reflect),
            "Repeat" => Ok(Spread::Repeat),
            _ => Err(BrushError::Syntax),
        },
    }
}

/// One `GradientStop`.
#[derive(Clone, Copy, Debug)]
struct Stop {
    offset: f64,
    colour: Colour,
}

/// 15.4.2's stop list, from either spelling of the property element.
fn stops_of(node: &Node) -> Result<Vec<Stop>, BrushError> {
    let property = format!("{}.GradientStops", node.local);
    let mut out = Vec::new();
    let mut push = |stop: &Node| -> Result<(), BrushError> {
        if !stop.xps || stop.local != "GradientStop" {
            return Ok(());
        }
        let colour = colour(stop.attr("Color").ok_or(BrushError::Syntax)?)?;
        let offset = markup::number(stop.attr("Offset").ok_or(BrushError::Syntax)?)
            .ok_or(BrushError::Syntax)?;
        out.push(Stop {
            offset: offset.clamp(0.0, 1.0),
            colour,
        });
        Ok(())
    };
    for child in &node.children {
        if child.xps && child.local == property {
            for stop in &child.children {
                push(stop)?;
            }
        } else {
            push(child)?;
        }
    }
    if out.is_empty() {
        return Err(BrushError::Syntax);
    }
    // A stable sort, so two stops at one offset keep the order the file wrote
    // them in — which is what decides which side of a hard stop each colour
    // lands on.
    out.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    Ok(out)
}

fn gradient(node: &Node, bbox: Option<[f64; 4]>) -> Result<Brush, BrushError> {
    let stops = stops_of(node)?;
    let spread = spread_of(node)?;
    let approximated = stops
        .iter()
        .any(|stop| stop.colour.alpha != stops[0].colour.alpha)
        // 15.4's `ScRgbLinearInterpolation` interpolates in linear light and
        // this writer interpolates in the shading's own `/DeviceRGB`, which is
        // sRGB. The endpoints are right and the middle is not, so it is
        // reported as approximate rather than either refused or left silent.
        || node
            .attr("ColorInterpolationMode")
            .is_some_and(|mode| mode.trim() == "ScRgbLinearInterpolation");
    let alpha = stops[0].colour.alpha * opacity_of(node)?;

    let relative = match node.attr("MappingMode") {
        None | Some("Absolute") => false,
        Some("RelativeToBoundingBox") => true,
        Some(_) => return Err(BrushError::Syntax),
    };
    // A relative gradient is stated in fractions of the box it fills, so
    // without a box there is nothing to be a fraction of — and inventing one
    // would put the gradient somewhere this file never said.
    let unit = if relative {
        let [x0, y0, x1, y1] = bbox.ok_or(BrushError::Syntax)?;
        if x1 <= x0 || y1 <= y0 {
            return Err(BrushError::Syntax);
        }
        [x1 - x0, 0.0, 0.0, y1 - y0, x0, y0]
    } else {
        markup::IDENTITY
    };
    let brush_transform = super::geometry::transform_of(node, &node.local)
        .map_err(|_| BrushError::Syntax)?
        .unwrap_or(markup::IDENTITY);

    let (shading, inner) = match node.local.as_str() {
        "LinearGradientBrush" => linear(node, &stops, spread)?,
        _ => radial(node, &stops, spread)?,
    };
    let matrix = markup::concat(inner, markup::concat(unit, brush_transform));
    if !matrix.iter().all(|v| markup::usable(*v)) {
        return Err(BrushError::Syntax);
    }

    Ok(Brush {
        paint: Paint::Gradient { shading, matrix },
        alpha,
        approximated,
    })
}

/// 15.4.3's `LinearGradientBrush`, as a type 2 shading.
fn linear(node: &Node, stops: &[Stop], spread: Spread) -> Result<(Shading, [f64; 6]), BrushError> {
    let start = node
        .attr("StartPoint")
        .and_then(markup::pair)
        .ok_or(BrushError::Syntax)?;
    let end = node
        .attr("EndPoint")
        .and_then(markup::pair)
        .ok_or(BrushError::Syntax)?;
    if start == end {
        return Err(BrushError::Syntax);
    }

    let base = ramp(stops);
    let (function, coords) = match spread {
        Spread::Pad => (base, [start.0, start.1, end.0, end.1]),
        _ => {
            // The axis grows by `SPREAD_COPIES` periods each way and the
            // function repeats over it, so the whole extended axis is still
            // `t` from zero to one and the sub-function boundaries land
            // exactly on the period boundaries.
            let (dx, dy) = (end.0 - start.0, end.1 - start.1);
            let copies = SPREAD_COPIES * 2 + 1;
            let from = (
                start.0 - dx * SPREAD_COPIES as f64,
                start.1 - dy * SPREAD_COPIES as f64,
            );
            let to = (
                end.0 + dx * SPREAD_COPIES as f64,
                end.1 + dy * SPREAD_COPIES as f64,
            );
            let mirrored =
                |index: usize| spread == Spread::Reflect && (index % 2) != (SPREAD_COPIES % 2);
            (
                replicate(&base, copies, &mirrored),
                [from.0, from.1, to.0, to.1],
            )
        }
    };
    if !coords.iter().all(|v| markup::usable(*v)) {
        return Err(BrushError::Syntax);
    }
    Ok((
        Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords,
            function,
            extend: (true, true),
        },
        markup::IDENTITY,
    ))
}

/// 15.4.4's `RadialGradientBrush`, as a type 3 shading between the
/// `GradientOrigin` at radius zero and the `Center` at `RadiusX`.
///
/// An XPS radial gradient is an **ellipse** and a PDF type 3 shading blends
/// between **circles**, so the difference is carried in the matrix rather than
/// in the geometry: the shading is stated over a circle of radius `RadiusX`
/// and the space it lives in is scaled about the centre so that circle becomes
/// the ellipse the file stated. The focal point is un-scaled on the way in for
/// the same reason.
fn radial(node: &Node, stops: &[Stop], spread: Spread) -> Result<(Shading, [f64; 6]), BrushError> {
    let centre = node
        .attr("Center")
        .and_then(markup::pair)
        .ok_or(BrushError::Syntax)?;
    let rx = markup::number(node.attr("RadiusX").ok_or(BrushError::Syntax)?)
        .ok_or(BrushError::Syntax)?;
    let ry = markup::number(node.attr("RadiusY").ok_or(BrushError::Syntax)?)
        .ok_or(BrushError::Syntax)?;
    if rx <= 0.0 || ry <= 0.0 {
        return Err(BrushError::Syntax);
    }
    // 15.4.4: the origin defaults to the centre, which makes the gradient
    // concentric.
    let origin = match node.attr("GradientOrigin") {
        Some(text) => markup::pair(text).ok_or(BrushError::Syntax)?,
        None => centre,
    };

    // Into the space in which the outer circle is a circle of radius `rx`.
    let unscale = [1.0, 0.0, 0.0, rx / ry, 0.0, 0.0];
    let focal = markup::apply(
        markup::concat(
            [1.0, 0.0, 0.0, 1.0, -centre.0, -centre.1],
            markup::concat(unscale, [1.0, 0.0, 0.0, 1.0, centre.0, centre.1]),
        ),
        origin,
    );
    let ellipse = markup::concat(
        [1.0, 0.0, 0.0, 1.0, -centre.0, -centre.1],
        markup::concat(
            [1.0, 0.0, 0.0, ry / rx, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0, centre.0, centre.1],
        ),
    );

    let base = ramp(stops);
    let (function, coords) = match spread {
        Spread::Pad => (base, [focal.0, focal.1, 0.0, centre.0, centre.1, rx]),
        _ => {
            // Only outward: the inner circle has radius zero, so there is
            // nothing between it and the focal point to repeat.
            let copies = SPREAD_COPIES + 1;
            let far = rx * copies as f64;
            let centre_out = (
                focal.0 + (centre.0 - focal.0) * copies as f64,
                focal.1 + (centre.1 - focal.1) * copies as f64,
            );
            let mirrored = |index: usize| spread == Spread::Reflect && index % 2 == 1;
            (
                replicate(&base, copies, &mirrored),
                [focal.0, focal.1, 0.0, centre_out.0, centre_out.1, far],
            )
        }
    };
    if !coords.iter().all(|v| markup::usable(*v)) || !ellipse.iter().all(|v| markup::usable(*v)) {
        return Err(BrushError::Syntax);
    }
    Ok((
        Shading::Radial {
            color_space: DeviceSpace::Rgb,
            coords,
            function,
            extend: (true, true),
        },
        ellipse,
    ))
}

/// One period of the gradient, as a function over `[0, 1]`.
///
/// Stops that do not reach the ends are extended flat, because 15.4.2 makes
/// the colour before the first stop and after the last one that stop's own —
/// which is a statement about the gradient and not about `/Extend`.
fn ramp(stops: &[Stop]) -> Function {
    let mut offsets: Vec<f64> = Vec::with_capacity(stops.len() + 2);
    let mut colours: Vec<[f64; 3]> = Vec::with_capacity(stops.len() + 2);
    if stops[0].offset > 0.0 {
        offsets.push(0.0);
        colours.push(stops[0].colour.rgb);
    }
    for stop in stops {
        offsets.push(stop.offset);
        colours.push(stop.colour.rgb);
    }
    let last = stops[stops.len() - 1];
    if last.offset < 1.0 {
        offsets.push(1.0);
        colours.push(last.colour.rgb);
    }

    // 7.10.4 wants `/Bounds` strictly increasing and strictly inside the
    // domain, and 15.4.2 permits two stops at one offset — that is how a hard
    // edge is spelled. The edge is kept by separating the two by the smallest
    // step that is still a distinct PDF real rather than by dropping one of
    // them, which would turn a hard edge into a ramp across the whole
    // gradient.
    const NUDGE: f64 = 1.0e-6;
    for at in 1..offsets.len() {
        if offsets[at] <= offsets[at - 1] {
            offsets[at] = (offsets[at - 1] + NUDGE).min(1.0);
        }
    }

    let mut pieces: Vec<Function> = Vec::new();
    let mut bounds: Vec<f64> = Vec::new();
    for at in 1..offsets.len() {
        pieces.push(Function::Exponential {
            domain: [0.0, 1.0],
            c0: colours[at - 1].to_vec(),
            c1: colours[at].to_vec(),
            n: 1.0,
        });
        if at + 1 < offsets.len() {
            bounds.push(offsets[at]);
        }
    }
    if pieces.len() == 1 {
        return pieces.remove(0);
    }
    if pieces.is_empty() {
        // One stop, or several at one offset: a gradient with no extent is a
        // flat colour, which is what 15.4.2 makes it.
        return Function::Exponential {
            domain: [0.0, 1.0],
            c0: colours[0].to_vec(),
            c1: colours[0].to_vec(),
            n: 1.0,
        };
    }
    let encode = vec![[0.0, 1.0]; pieces.len()];
    Function::Stitching {
        domain: [0.0, 1.0],
        functions: pieces,
        bounds,
        encode,
    }
}

/// `copies` periods of `base` end to end, each forward or mirrored.
///
/// This is the whole of `Repeat` and `Reflect`: the two differ in nothing but
/// which copies run backwards, so building them from one function with one
/// predicate is what stops them from being two implementations that agree by
/// accident.
fn replicate(base: &Function, copies: usize, mirrored: &dyn Fn(usize) -> bool) -> Function {
    let step = 1.0 / copies as f64;
    Function::Stitching {
        domain: [0.0, 1.0],
        functions: vec![base.clone(); copies],
        bounds: (1..copies).map(|index| index as f64 * step).collect(),
        encode: (0..copies)
            .map(|index| {
                if mirrored(index) {
                    [1.0, 0.0]
                } else {
                    [0.0, 1.0]
                }
            })
            .collect(),
    }
}
