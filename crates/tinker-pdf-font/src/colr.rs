//! `COLR`, the colour table: a colour glyph as layers of outline glyphs, or
//! as a graph of paints (OpenType 1.9.1, the COLR chapter).
//!
//! Version 0 maps a base glyph to a slice of layer records, the first the
//! bottom layer (*BaseGlyph and Layer records*). Each layer is an outline
//! glyph and a `CPAL` palette entry, or 0xFFFF for the text's own foreground
//! colour (*Colors and solid color fills*).
//!
//! Version 1 carries the same two arrays first, and then a graph of paint
//! tables per base glyph (*BaseGlyphList, LayerList and ClipList*, *Paint
//! tables*): fills, gradients, a glyph's outline as a clip, transforms and
//! composites. [`Colr::paint_graph`] reads a glyph's graph into a
//! [`PaintGraph`], an arena of [`Paint`] nodes each holding its table's
//! fields as written — the deltas of the variable formats are applied
//! elsewhere — and refuses what *Color glyphs as a directed acyclic graph*
//! calls not well formed by name, the rest of the graph read.
//!
//! Every offset and count is held to the table's own bytes (ruling 1), and
//! what comes back is glyph ids, palette indices, colours and numbers — no
//! PDF vocabulary (ruling 8). Drawing them is the writer's.
//!
//! Design: `docs/design/colour-fonts.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::cpal::{Cpal, Rgba};

fn byte(data: &[u8], at: usize) -> Option<u8> {
    data.get(at).copied()
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be24(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(3)?)?;
    Some(u32::from_be_bytes([0, b[0], b[1], b[2]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    be16(data, at).map(|v| v as i16)
}

/// The palette index that is no index: the text's foreground colour (*Colors
/// and solid color fills*).
pub const FOREGROUND: u16 = 0xFFFF;

/// The most paints a path from a glyph's root may hold
/// (`docs/design/colour-fonts.md`, *The limits a hostile graph meets*).
pub const MAX_PAINT_DEPTH: u32 = 64;

/// The most nodes one glyph's graph may hold.
pub const MAX_PAINT_NODES: usize = 16_384;

/// An `F2DOT14` as a number: its `i16` over 16 384, exactly.
#[must_use]
pub fn f2dot14(raw: i16) -> f64 {
    f64::from(raw) / 16384.0
}

/// A `Fixed` as a number: its `i32` over 65 536, exactly.
#[must_use]
pub fn fixed(raw: i32) -> f64 {
    f64::from(raw) / 65536.0
}

/// A colour reference as `COLR` writes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColourIndex {
    /// An entry of the palette the glyph is drawn in.
    Palette(u16),
    /// 0xFFFF: the colour the text is drawn in, which "must not be treated as
    /// an actual index into the CPAL ColorRecord array".
    Foreground,
}

impl ColourIndex {
    /// The reference a raw `paletteIndex` makes.
    #[must_use]
    pub fn from_raw(raw: u16) -> ColourIndex {
        if raw == FOREGROUND {
            ColourIndex::Foreground
        } else {
            ColourIndex::Palette(raw)
        }
    }
}

/// What a version 0 layer is filled with, its palette entry looked up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayerColour {
    /// The palette entry's colour, its own alpha included.
    Colour(Rgba),
    /// The text's colour.
    Foreground,
}

/// One version 0 layer: an outline glyph and what fills it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Layer {
    /// The glyph whose outline the layer fills.
    pub glyph: u16,
    /// Its colour.
    pub colour: LayerColour,
}

/// How a colour line continues past its first and last stops (*Color
/// references, ColorStop and ColorLine*, the `Extend` enumeration).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Extend {
    /// `EXTEND_PAD`: the nearest stop's colour.
    Pad,
    /// `EXTEND_REPEAT`: the line repeated.
    Repeat,
    /// `EXTEND_REFLECT`: the line mirrored from its nearest end.
    Reflect,
}

/// One stop of a colour line, as written: its offset and alpha `F2DOT14`s,
/// and for a `VarColorStop` its `varIndexBase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColourStop {
    /// `stopOffset`.
    pub offset: i16,
    /// `paletteIndex`.
    pub colour: ColourIndex,
    /// `alpha`, multiplied into the palette entry's.
    pub alpha: i16,
    /// `varIndexBase`, for a stop of a `VarColorLine`.
    pub var_index_base: Option<u32>,
}

/// A `ColorLine` or `VarColorLine`: its extend and its stops, in the order the
/// table writes them — *Color lines* has them used in offset order, which is
/// the drawing's to sort.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColourLine {
    /// The extend mode.
    pub extend: Extend,
    /// The stops.
    pub stops: Vec<ColourStop>,
}

/// `PaintComposite`'s modes (*Format 32: PaintComposite*), each W3C
/// Compositing and Blending Level 1's of the same name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompositeMode {
    /// 0, `COMPOSITE_CLEAR`.
    Clear,
    /// 1, `COMPOSITE_SRC`.
    Src,
    /// 2, `COMPOSITE_DEST`.
    Dest,
    /// 3, `COMPOSITE_SRC_OVER`.
    SrcOver,
    /// 4, `COMPOSITE_DEST_OVER`.
    DestOver,
    /// 5, `COMPOSITE_SRC_IN`.
    SrcIn,
    /// 6, `COMPOSITE_DEST_IN`.
    DestIn,
    /// 7, `COMPOSITE_SRC_OUT`.
    SrcOut,
    /// 8, `COMPOSITE_DEST_OUT`.
    DestOut,
    /// 9, `COMPOSITE_SRC_ATOP`.
    SrcAtop,
    /// 10, `COMPOSITE_DEST_ATOP`.
    DestAtop,
    /// 11, `COMPOSITE_XOR`.
    Xor,
    /// 12, `COMPOSITE_PLUS`.
    Plus,
    /// 13, `COMPOSITE_SCREEN`.
    Screen,
    /// 14, `COMPOSITE_OVERLAY`.
    Overlay,
    /// 15, `COMPOSITE_DARKEN`.
    Darken,
    /// 16, `COMPOSITE_LIGHTEN`.
    Lighten,
    /// 17, `COMPOSITE_COLOR_DODGE`.
    ColorDodge,
    /// 18, `COMPOSITE_COLOR_BURN`.
    ColorBurn,
    /// 19, `COMPOSITE_HARD_LIGHT`.
    HardLight,
    /// 20, `COMPOSITE_SOFT_LIGHT`.
    SoftLight,
    /// 21, `COMPOSITE_DIFFERENCE`.
    Difference,
    /// 22, `COMPOSITE_EXCLUSION`.
    Exclusion,
    /// 23, `COMPOSITE_MULTIPLY`.
    Multiply,
    /// 24, `COMPOSITE_HSL_HUE`.
    HslHue,
    /// 25, `COMPOSITE_HSL_SATURATION`.
    HslSaturation,
    /// 26, `COMPOSITE_HSL_COLOR`.
    HslColor,
    /// 27, `COMPOSITE_HSL_LUMINOSITY`.
    HslLuminosity,
}

impl CompositeMode {
    /// Every mode, in the enumeration's order: index `n` is value `n`.
    pub const ALL: [CompositeMode; 28] = [
        CompositeMode::Clear,
        CompositeMode::Src,
        CompositeMode::Dest,
        CompositeMode::SrcOver,
        CompositeMode::DestOver,
        CompositeMode::SrcIn,
        CompositeMode::DestIn,
        CompositeMode::SrcOut,
        CompositeMode::DestOut,
        CompositeMode::SrcAtop,
        CompositeMode::DestAtop,
        CompositeMode::Xor,
        CompositeMode::Plus,
        CompositeMode::Screen,
        CompositeMode::Overlay,
        CompositeMode::Darken,
        CompositeMode::Lighten,
        CompositeMode::ColorDodge,
        CompositeMode::ColorBurn,
        CompositeMode::HardLight,
        CompositeMode::SoftLight,
        CompositeMode::Difference,
        CompositeMode::Exclusion,
        CompositeMode::Multiply,
        CompositeMode::HslHue,
        CompositeMode::HslSaturation,
        CompositeMode::HslColor,
        CompositeMode::HslLuminosity,
    ];

    /// The mode a raw `compositeMode` names, or `None` for a value the
    /// enumeration does not define.
    #[must_use]
    pub fn from_raw(raw: u8) -> Option<CompositeMode> {
        CompositeMode::ALL.get(usize::from(raw)).copied()
    }
}

/// A `ClipBox`, format 1 or 2 (*BaseGlyphList, LayerList and ClipList*): the
/// box outside which a colour glyph does not render, in font units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClipBox {
    /// `xMin`.
    pub x_min: i16,
    /// `yMin`.
    pub y_min: i16,
    /// `xMax`.
    pub x_max: i16,
    /// `yMax`.
    pub y_max: i16,
    /// `varIndexBase`, for format 2.
    pub var_index_base: Option<u32>,
}

/// One node of a [`PaintGraph`]: an index into its arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaintId(pub u32);

/// Why a paint table reads as no paint (*Color glyphs as a directed acyclic
/// graph*: the node is ignored, "considered to be visually bounded", and the
/// rest of the graph read).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColrRefusal {
    /// The paint table at this offset in `COLR` was reached again on its own
    /// path: the graph names itself, through a `PaintColrLayers` slice, a
    /// `PaintColrGlyph` or an offset of zero.
    Cycle {
        /// The table's offset in `COLR`.
        at: u32,
    },
    /// The path from the root passed [`MAX_PAINT_DEPTH`].
    TooDeep,
    /// The graph passed [`MAX_PAINT_NODES`].
    TooLarge,
    /// A paint format the clause does not define.
    UnknownFormat(u8),
    /// The table at this offset runs past `COLR`, names no child or colour
    /// line where it needs one, or names layers past the `LayerList`.
    Malformed {
        /// The table's offset in `COLR`.
        at: u32,
    },
    /// A `PaintColrGlyph` naming a glyph the `BaseGlyphList` does not hold.
    MissingColrGlyph(u16),
}

/// A paint table, its fields as written: `F2DOT14`s and `Fixed`s as their
/// integers ([`f2dot14`], [`fixed`]), coordinates in font units, and each
/// variable format's `varIndexBase` as `Some` where the static format of the
/// pair has `None`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Paint {
    /// Format 1, `PaintColrLayers`: its layers, bottom first.
    Layers(Vec<PaintId>),
    /// Formats 2 and 3, `PaintSolid` and `PaintVarSolid`.
    Solid {
        /// `paletteIndex`.
        colour: ColourIndex,
        /// `alpha`.
        alpha: i16,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 4 and 5: the line runs from `p0` to `p1`, its colours carried
    /// along lines parallel to `p0p2`.
    LinearGradient {
        /// The colour line.
        line: ColourLine,
        /// `x0`, `y0`.
        p0: [i16; 2],
        /// `x1`, `y1`.
        p1: [i16; 2],
        /// `x2`, `y2`.
        p2: [i16; 2],
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 6 and 7: two circles.
    RadialGradient {
        /// The colour line.
        line: ColourLine,
        /// `x0`, `y0`.
        c0: [i16; 2],
        /// `radius0`.
        r0: u16,
        /// `x1`, `y1`.
        c1: [i16; 2],
        /// `radius1`.
        r1: u16,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 8 and 9: angles biased by 1.0, so `(angle + 1) × 180°`
    /// counter-clockwise.
    SweepGradient {
        /// The colour line.
        line: ColourLine,
        /// `centerX`, `centerY`.
        centre: [i16; 2],
        /// `startAngle`.
        start_angle: i16,
        /// `endAngle`.
        end_angle: i16,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Format 10, `PaintGlyph`: an outline glyph as the child's clip.
    Glyph {
        /// `glyphID`.
        glyph: u16,
        /// The child.
        paint: PaintId,
    },
    /// Format 11, `PaintColrGlyph`: another base glyph's graph, its root the
    /// child.
    ColrGlyph {
        /// `glyphID`.
        glyph: u16,
        /// That glyph's root.
        paint: PaintId,
    },
    /// Formats 12 and 13: `xx`, `yx`, `xy`, `yy`, `dx`, `dy` as `Fixed`s.
    Transform {
        /// The matrix.
        matrix: [i32; 6],
        /// The child.
        paint: PaintId,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 14 and 15.
    Translate {
        /// `dx`.
        dx: i16,
        /// `dy`.
        dy: i16,
        /// The child.
        paint: PaintId,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 16 to 19; 18 and 19 about a centre.
    Scale {
        /// `scaleX`.
        scale_x: i16,
        /// `scaleY`.
        scale_y: i16,
        /// `centerX`, `centerY`.
        centre: Option<[i16; 2]>,
        /// The child.
        paint: PaintId,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 20 to 23; 22 and 23 about a centre.
    ScaleUniform {
        /// `scale`.
        scale: i16,
        /// `centerX`, `centerY`.
        centre: Option<[i16; 2]>,
        /// The child.
        paint: PaintId,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 24 to 27, 180° per 1.0 counter-clockwise; 26 and 27 about a
    /// centre.
    Rotate {
        /// `angle`.
        angle: i16,
        /// `centerX`, `centerY`.
        centre: Option<[i16; 2]>,
        /// The child.
        paint: PaintId,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Formats 28 to 31, 180° per 1.0; 30 and 31 about a centre.
    Skew {
        /// `xSkewAngle`.
        x_angle: i16,
        /// `ySkewAngle`.
        y_angle: i16,
        /// `centerX`, `centerY`.
        centre: Option<[i16; 2]>,
        /// The child.
        paint: PaintId,
        /// `varIndexBase`.
        var_index_base: Option<u32>,
    },
    /// Format 32: the source composited onto the backdrop.
    Composite {
        /// The source.
        source: PaintId,
        /// The mode.
        mode: CompositeMode,
        /// The backdrop.
        backdrop: PaintId,
    },
    /// A table read as no paint, and why.
    Refused(ColrRefusal),
}

impl Paint {
    /// The format this node was read from, `None` for a refusal.
    #[must_use]
    pub fn format(&self) -> Option<u8> {
        let var = |base: &Option<u32>| u8::from(base.is_some());
        let around = |centre: &Option<[i16; 2]>| 2 * u8::from(centre.is_some());
        Some(match self {
            Paint::Layers(_) => 1,
            Paint::Solid { var_index_base, .. } => 2 + var(var_index_base),
            Paint::LinearGradient { var_index_base, .. } => 4 + var(var_index_base),
            Paint::RadialGradient { var_index_base, .. } => 6 + var(var_index_base),
            Paint::SweepGradient { var_index_base, .. } => 8 + var(var_index_base),
            Paint::Glyph { .. } => 10,
            Paint::ColrGlyph { .. } => 11,
            Paint::Transform { var_index_base, .. } => 12 + var(var_index_base),
            Paint::Translate { var_index_base, .. } => 14 + var(var_index_base),
            Paint::Scale {
                centre,
                var_index_base,
                ..
            } => 16 + around(centre) + var(var_index_base),
            Paint::ScaleUniform {
                centre,
                var_index_base,
                ..
            } => 20 + around(centre) + var(var_index_base),
            Paint::Rotate {
                centre,
                var_index_base,
                ..
            } => 24 + around(centre) + var(var_index_base),
            Paint::Skew {
                centre,
                var_index_base,
                ..
            } => 28 + around(centre) + var(var_index_base),
            Paint::Composite { .. } => 32,
            Paint::Refused(_) => return None,
        })
    }

    /// The nodes this one draws, in the order it draws them: a composite's
    /// backdrop before its source.
    #[must_use]
    pub fn children(&self) -> Vec<PaintId> {
        match self {
            Paint::Layers(layers) => layers.clone(),
            Paint::Glyph { paint, .. }
            | Paint::ColrGlyph { paint, .. }
            | Paint::Transform { paint, .. }
            | Paint::Translate { paint, .. }
            | Paint::Scale { paint, .. }
            | Paint::ScaleUniform { paint, .. }
            | Paint::Rotate { paint, .. }
            | Paint::Skew { paint, .. } => vec![*paint],
            Paint::Composite {
                source, backdrop, ..
            } => vec![*backdrop, *source],
            Paint::Solid { .. }
            | Paint::LinearGradient { .. }
            | Paint::RadialGradient { .. }
            | Paint::SweepGradient { .. }
            | Paint::Refused(_) => Vec::new(),
        }
    }
}

/// Something the reader tolerated in a table it still reads (ruling 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColrWarning {
    /// The `BaseGlyph` records are not in increasing glyph order, which
    /// *BaseGlyph and Layer records* requires: they are scanned, and the
    /// first record for a glyph answers for it.
    BaseGlyphsOutOfOrder,
    /// The `BaseGlyphPaintRecord`s are not in increasing glyph order: scanned
    /// the same way.
    BaseGlyphPaintsOutOfOrder,
    /// The `Clip` records are not sorted by `startGlyphID` with ranges that do
    /// not overlap: scanned, the first range holding a glyph answering.
    ClipsOutOfOrder,
    /// A colour line's extend value the enumeration does not define, read as
    /// `EXTEND_PAD`, as the clause says.
    ExtendRead(u8),
    /// A `compositeMode` the enumeration does not define, read as
    /// `COMPOSITE_CLEAR`, as the clause says.
    CompositeModeRead(u8),
    /// A paint table read as a refusal, the graph around it read.
    PaintRefused(ColrRefusal),
}

/// One glyph's version 1 graph: an arena of nodes, each paint table one node
/// however many parents reach it, and the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintGraph {
    nodes: Vec<Paint>,
    root: PaintId,
    warnings: Vec<ColrWarning>,
}

impl PaintGraph {
    /// The root, the paint the glyph's `BaseGlyphPaintRecord` names.
    #[must_use]
    pub fn root(&self) -> PaintId {
        self.root
    }

    /// The node `id`.
    #[must_use]
    pub fn paint(&self, id: PaintId) -> Option<&Paint> {
        self.nodes.get(usize::try_from(id.0).ok()?)
    }

    /// Every node, in the order read; a node's index is its [`PaintId`].
    #[must_use]
    pub fn nodes(&self) -> &[Paint] {
        &self.nodes
    }

    /// What reading the graph tolerated and refused, each once, in the order
    /// first met.
    #[must_use]
    pub fn warnings(&self) -> &[ColrWarning] {
        &self.warnings
    }
}

/// A parsed `COLR` table over borrowed bytes.
#[derive(Clone, Debug)]
pub struct Colr<'a> {
    data: &'a [u8],
    version: u16,
    /// The `BaseGlyph` records: where they start and how many, where the whole
    /// array lies inside the table.
    base: Option<(usize, u16)>,
    /// Whether `base` is in increasing glyph order, so a lookup bisects.
    base_ordered: bool,
    /// The layer records: where they start and how many.
    layers_at: usize,
    layer_count: u16,
    /// Version 1's `BaseGlyphList`: where it starts and its record count,
    /// where every record lies inside the table.
    paints: Option<(usize, u32)>,
    paints_ordered: bool,
    /// Version 1's `LayerList`: where it starts and its offset count.
    layer_list: Option<(usize, u32)>,
    /// Version 1's `ClipList`, format 1: where it starts and its clip count.
    clips: Option<(usize, u32)>,
    clips_ordered: bool,
    warnings: Vec<ColrWarning>,
}

/// Where a list a header offset names starts, and its count, where the offset
/// is not zero and every one of `count` records of `size` bytes after a
/// `count_size`-byte count lies inside `data`.
fn list(
    data: &[u8],
    offset: u32,
    count_at: usize,
    count_size: usize,
    size: usize,
) -> Option<(usize, u32)> {
    let at = usize::try_from(offset).ok().filter(|&at| at != 0)?;
    let count = be32(data, at.checked_add(count_at)?)?;
    let end = u64::try_from(at).ok()?
        + u64::try_from(count_at + count_size).ok()?
        + u64::from(count) * u64::try_from(size).ok()?;
    (end <= u64::try_from(data.len()).ok()?).then_some((at, count))
}

impl<'a> Colr<'a> {
    /// Parses the header of a version 0 or version 1 table, and checks the
    /// order of each sorted array once.
    ///
    /// `None` for another version and for a header that runs past the data.
    /// A `BaseGlyph` array, a `BaseGlyphList`, a `LayerList` or a `ClipList`
    /// whose records run past the table is read as none; the layer records
    /// are checked a lookup at a time.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Colr<'a>> {
        let version = be16(data, 0)?;
        let header = match version {
            0 => 14,
            1 => 34,
            _ => return None,
        };
        if data.len() < header {
            return None;
        }
        let base_count = be16(data, 2)?;
        let base_at = usize::try_from(be32(data, 4)?).ok()?;
        let layers_at = usize::try_from(be32(data, 8)?).ok()?;
        let layer_count = be16(data, 12)?;

        let mut warnings = Vec::new();
        let base_end = base_at.checked_add(usize::from(base_count) * 6);
        let base = (base_count > 0 && base_end.is_some_and(|end| end <= data.len()))
            .then_some((base_at, base_count));
        let base_ordered = base.is_none_or(|(at, count)| {
            ascending((0..usize::from(count)).map(|i| be16(data, at + i * 6)))
        });
        if !base_ordered {
            warnings.push(ColrWarning::BaseGlyphsOutOfOrder);
        }

        let (mut paints, mut layer_list, mut clips) = (None, None, None);
        let (mut paints_ordered, mut clips_ordered) = (true, true);
        if version == 1 {
            paints = list(data, be32(data, 14)?, 0, 4, 6);
            paints_ordered = paints.is_none_or(|(at, count)| {
                ascending((0..count as usize).map(|i| be16(data, at + 4 + i * 6)))
            });
            if !paints_ordered {
                warnings.push(ColrWarning::BaseGlyphPaintsOutOfOrder);
            }
            layer_list = list(data, be32(data, 18)?, 0, 4, 4);
            clips =
                list(data, be32(data, 22)?, 1, 4, 7).filter(|&(at, _)| byte(data, at) == Some(1));
            clips_ordered = clips.is_none_or(|(at, count)| {
                let mut previous_end: Option<u16> = None;
                for i in 0..count as usize {
                    let record = at + 5 + i * 7;
                    let (Some(start), Some(end)) = (be16(data, record), be16(data, record + 2))
                    else {
                        return false;
                    };
                    if previous_end.is_some_and(|previous| start <= previous) {
                        return false;
                    }
                    previous_end = Some(end.max(start));
                }
                true
            });
            if !clips_ordered {
                warnings.push(ColrWarning::ClipsOutOfOrder);
            }
        }
        Some(Colr {
            data,
            version,
            base,
            base_ordered,
            layers_at,
            layer_count,
            paints,
            paints_ordered,
            layer_list,
            clips,
            clips_ordered,
            warnings,
        })
    }

    /// The table's version, 0 or 1.
    #[must_use]
    pub fn version(&self) -> u16 {
        self.version
    }

    /// What the reader tolerated in the table's arrays, in the order it met
    /// it; a glyph's graph carries its own ([`PaintGraph::warnings`]).
    #[must_use]
    pub fn warnings(&self) -> &[ColrWarning] {
        &self.warnings
    }

    /// The `BaseGlyph` record for `glyph`: its first layer and layer count.
    fn base_record(&self, glyph: u16) -> Option<(u16, u16)> {
        let (at, count) = self.base?;
        let data = self.data;
        let record = move |i: usize| -> Option<(u16, (u16, u16))> {
            let r = at + i * 6;
            Some((be16(data, r)?, (be16(data, r + 2)?, be16(data, r + 4)?)))
        };
        find(usize::from(count), self.base_ordered, glyph, record)
    }

    /// Whether `glyph` has a version 0 record.
    #[must_use]
    pub fn has_layers(&self, glyph: u16) -> bool {
        self.base_record(glyph).is_some()
    }

    /// `glyph`'s version 0 layers as the table writes them, bottom first: each
    /// outline glyph and its colour reference.
    ///
    /// `None` where the glyph has no record, and where its slice runs past
    /// `numLayerRecords` or past the table. A record of no layers answers an
    /// empty list.
    #[must_use]
    pub fn layer_records(&self, glyph: u16) -> Option<Vec<(u16, ColourIndex)>> {
        let (first, count) = self.base_record(glyph)?;
        let end = u32::from(first) + u32::from(count);
        if end > u32::from(self.layer_count) {
            return None;
        }
        (usize::from(first)..usize::from(first) + usize::from(count))
            .map(|i| {
                let at = self.layers_at.checked_add(i.checked_mul(4)?)?;
                let glyph = be16(self.data, at)?;
                let index = be16(self.data, at.checked_add(2)?)?;
                Some((glyph, ColourIndex::from_raw(index)))
            })
            .collect()
    }

    /// `glyph`'s version 0 layers, bottom first, each palette entry looked up in
    /// palette `palette` of `cpal`.
    ///
    /// `None` where [`Colr::layer_records`] answers `None`, and where any
    /// layer names an entry the palette does not have: a glyph drawn with a
    /// layer missing is a different picture, and its outline is the honest
    /// one.
    #[must_use]
    pub fn layers(&self, glyph: u16, cpal: &Cpal<'_>, palette: u16) -> Option<Vec<Layer>> {
        self.layer_records(glyph)?
            .into_iter()
            .map(|(glyph, index)| {
                let colour = match index {
                    ColourIndex::Foreground => LayerColour::Foreground,
                    ColourIndex::Palette(entry) => {
                        LayerColour::Colour(cpal.colour(palette, entry)?)
                    }
                };
                Some(Layer { glyph, colour })
            })
            .collect()
    }

    /// The offset in `COLR` of `glyph`'s root paint, through its
    /// `BaseGlyphPaintRecord`.
    fn root_paint(&self, glyph: u16) -> Option<usize> {
        let (at, count) = self.paints?;
        let data = self.data;
        let record = move |i: usize| -> Option<(u16, u32)> {
            let r = at + 4 + i * 6;
            Some((be16(data, r)?, be32(data, r + 2)?))
        };
        let offset = find(count as usize, self.paints_ordered, glyph, record)?;
        if offset == 0 {
            return None;
        }
        at.checked_add(usize::try_from(offset).ok()?)
    }

    /// Whether `glyph` has a version 1 `BaseGlyphPaintRecord`.
    #[must_use]
    pub fn has_paint(&self, glyph: u16) -> bool {
        self.root_paint(glyph).is_some()
    }

    /// `glyph`'s version 1 graph.
    ///
    /// `None` where the glyph has no `BaseGlyphPaintRecord`, and where its
    /// root paint itself reads as a refusal — nothing of the glyph is then left
    /// to draw, and *Color glyphs as a directed acyclic graph* lets its outline
    /// be drawn instead. A refusal below the root is a [`Paint::Refused`] node
    /// with a warning, and the rest of the graph is read.
    #[must_use]
    pub fn paint_graph(&self, glyph: u16) -> Option<PaintGraph> {
        let root_at = self.root_paint(glyph)?;
        let mut reader = GraphReader {
            colr: self,
            nodes: Vec::new(),
            memo: BTreeMap::new(),
            refused: BTreeMap::new(),
            path: Vec::new(),
            warnings: Vec::new(),
            warned: BTreeSet::new(),
        };
        let (root, _) = reader.read(root_at, 0);
        let refused_root = matches!(
            reader.nodes.get(usize::try_from(root.0).ok()?),
            Some(Paint::Refused(_)) | None
        );
        if refused_root {
            return None;
        }
        Some(PaintGraph {
            nodes: reader.nodes,
            root,
            warnings: reader.warnings,
        })
    }

    /// The clip box a `Clip` record gives `glyph`'s range.
    ///
    /// `None` where no record's range holds the glyph, where the `ClipList`
    /// is absent or not format 1, and where the box runs past the table or is
    /// a format the clause does not define. A record whose end is below its
    /// start holds no glyph.
    #[must_use]
    pub fn clip_box(&self, glyph: u16) -> Option<ClipBox> {
        let (at, count) = self.clips?;
        let data = self.data;
        let record = |i: usize| -> Option<(u16, u16, u32)> {
            let r = at + 5 + i * 7;
            Some((be16(data, r)?, be16(data, r + 2)?, be24(data, r + 4)?))
        };
        let holds = |&(start, end, _): &(u16, u16, u32)| start <= glyph && glyph <= end;
        let found = if self.clips_ordered {
            // The last record starting at or below the glyph is the only one
            // that can hold it, the ranges being sorted and disjoint.
            let (mut low, mut high) = (0usize, count as usize);
            while low < high {
                let middle = low + (high - low) / 2;
                if record(middle)?.0 <= glyph {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            low.checked_sub(1).and_then(record).filter(holds)
        } else {
            (0..count as usize).filter_map(record).find(holds)
        };
        let (_, _, offset) = found?;
        let box_at = at.checked_add(usize::try_from(offset).ok()?)?;
        let var_index_base = match byte(data, box_at)? {
            1 => None,
            2 => Some(be32(data, box_at + 9)?),
            _ => return None,
        };
        Some(ClipBox {
            x_min: i16_at(data, box_at + 1)?,
            y_min: i16_at(data, box_at + 3)?,
            x_max: i16_at(data, box_at + 5)?,
            y_max: i16_at(data, box_at + 7)?,
            var_index_base,
        })
    }
}

/// Whether every value of `values` reads and each exceeds the one before.
fn ascending(values: impl Iterator<Item = Option<u16>>) -> bool {
    let mut previous: Option<u16> = None;
    for value in values {
        let Some(value) = value else {
            return false;
        };
        if previous.is_some_and(|previous| value <= previous) {
            return false;
        }
        previous = Some(value);
    }
    true
}

/// The value of the record whose glyph is `glyph`, among `count` records
/// `record` reads: by bisection where they are in increasing glyph order, and
/// otherwise by a scan from the start, so the first record for a glyph
/// answers.
fn find<T>(
    count: usize,
    ordered: bool,
    glyph: u16,
    record: impl Fn(usize) -> Option<(u16, T)>,
) -> Option<T> {
    if !ordered {
        return (0..count)
            .filter_map(&record)
            .find(|(id, _)| *id == glyph)
            .map(|(_, value)| value);
    }
    let (mut low, mut high) = (0usize, count);
    while low < high {
        let middle = low + (high - low) / 2;
        let (id, value) = record(middle)?;
        match id.cmp(&glyph) {
            core::cmp::Ordering::Less => low = middle + 1,
            core::cmp::Ordering::Greater => high = middle,
            core::cmp::Ordering::Equal => return Some(value),
        }
    }
    None
}

/// The walk that reads one glyph's graph into an arena.
struct GraphReader<'c, 'a> {
    colr: &'c Colr<'a>,
    nodes: Vec<Paint>,
    /// Each paint table read, by its offset in `COLR`: its node and its height,
    /// the most paints below it on a path.
    memo: BTreeMap<u32, (PaintId, u32)>,
    /// One node for each refusal, however many edges it stands for.
    refused: BTreeMap<ColrRefusal, PaintId>,
    /// The tables on the path being read, by offset.
    path: Vec<u32>,
    warnings: Vec<ColrWarning>,
    warned: BTreeSet<ColrWarning>,
}

impl GraphReader<'_, '_> {
    fn warn(&mut self, warning: ColrWarning) {
        if self.warned.insert(warning) {
            self.warnings.push(warning);
        }
    }

    fn push(&mut self, paint: Paint) -> PaintId {
        let id = PaintId(u32::try_from(self.nodes.len()).unwrap_or(u32::MAX));
        self.nodes.push(paint);
        id
    }

    /// The node standing for `why`, its warning given once.
    fn refuse(&mut self, why: ColrRefusal) -> (PaintId, u32) {
        self.warn(ColrWarning::PaintRefused(why));
        if let Some(&id) = self.refused.get(&why) {
            return (id, 0);
        }
        let id = self.push(Paint::Refused(why));
        self.refused.insert(why, id);
        (id, 0)
    }

    /// The node for the paint table at `at` in `COLR`, reached at `depth`
    /// paints below the root, and its height.
    fn read(&mut self, at: usize, depth: u32) -> (PaintId, u32) {
        let Ok(key) = u32::try_from(at) else {
            return self.refuse(ColrRefusal::Malformed { at: u32::MAX });
        };
        if self.path.contains(&key) {
            return self.refuse(ColrRefusal::Cycle { at: key });
        }
        if let Some(&(id, height)) = self.memo.get(&key) {
            if depth.saturating_add(height) >= MAX_PAINT_DEPTH {
                return self.refuse(ColrRefusal::TooDeep);
            }
            return (id, height);
        }
        if depth >= MAX_PAINT_DEPTH {
            return self.refuse(ColrRefusal::TooDeep);
        }
        if self.nodes.len() >= MAX_PAINT_NODES {
            return self.refuse(ColrRefusal::TooLarge);
        }
        self.path.push(key);
        let read = self.table(at, key, depth);
        self.path.pop();
        let (id, height) = match read {
            Ok((paint, height)) => (self.push(paint), height),
            Err(why) => self.refuse(why),
        };
        self.memo.insert(key, (id, height));
        (id, height)
    }

    /// The child at `offset` from the table at `at`.
    fn child(&mut self, at: usize, offset: u32, depth: u32) -> (PaintId, u32) {
        match at.checked_add(offset as usize) {
            Some(child) => self.read(child, depth + 1),
            None => self.refuse(ColrRefusal::Malformed { at: u32::MAX }),
        }
    }

    /// A colour line at `at`, a `VarColorLine` where `variable`; `None` where
    /// its stops run past the table.
    fn colour_line(&mut self, at: usize, variable: bool) -> Option<ColourLine> {
        let data = self.colr.data;
        let raw = byte(data, at)?;
        let count = usize::from(be16(data, at.checked_add(1)?)?);
        let size = if variable { 10 } else { 6 };
        let end = at.checked_add(3)?.checked_add(count.checked_mul(size)?)?;
        if end > data.len() {
            return None;
        }
        let extend = match raw {
            0 => Extend::Pad,
            1 => Extend::Repeat,
            2 => Extend::Reflect,
            other => {
                self.warn(ColrWarning::ExtendRead(other));
                Extend::Pad
            }
        };
        let mut stops = Vec::with_capacity(count);
        for i in 0..count {
            let stop = at + 3 + i * size;
            stops.push(ColourStop {
                offset: i16_at(data, stop)?,
                colour: ColourIndex::from_raw(be16(data, stop + 2)?),
                alpha: i16_at(data, stop + 4)?,
                var_index_base: if variable {
                    Some(be32(data, stop + 6)?)
                } else {
                    None
                },
            });
        }
        Some(ColourLine { extend, stops })
    }

    /// The paint table at `at`, whose offset is `key`: its node and height, or
    /// why it is none.
    fn table(&mut self, at: usize, key: u32, depth: u32) -> Result<(Paint, u32), ColrRefusal> {
        let data = self.colr.data;
        let bad = ColrRefusal::Malformed { at: key };
        // Saturating: an offset that would wrap reads past the data and is
        // refused, on a 32-bit target as on a 64-bit one.
        let field = |o: usize| at.saturating_add(o);
        let u8_at = |o: usize| byte(data, field(o)).ok_or(bad);
        let u16_at = |o: usize| be16(data, field(o)).ok_or(bad);
        let s16_at = |o: usize| i16_at(data, field(o)).ok_or(bad);
        let u32_at = |o: usize| be32(data, field(o)).ok_or(bad);
        // A child or subtable offset: never zero, which would be the table
        // itself and is no child (*Filling shapes*).
        let offset_at = |o: usize| match be24(data, field(o)) {
            Some(0) | None => Err(bad),
            Some(offset) => Ok(offset),
        };
        let point =
            |o: usize| -> Result<[i16; 2], ColrRefusal> { Ok([s16_at(o)?, s16_at(o + 2)?]) };

        let format = u8_at(0)?;
        // Each variable format is the odd one of its pair (3, 5, 7, 9, 13,
        // 15 and on to 31), its `varIndexBase` after the static form's fields.
        let variable = format % 2 == 1;
        Ok(match format {
            1 => {
                let count = u8_at(1)?;
                let first = u32_at(2)?;
                let (list_at, listed) = self.colr.layer_list.ok_or(bad)?;
                if u64::from(first) + u64::from(count) > u64::from(listed) {
                    return Err(bad);
                }
                let mut offsets = Vec::with_capacity(usize::from(count));
                for i in 0..usize::from(count) {
                    let slot = list_at + 4 + (first as usize + i) * 4;
                    offsets.push(be32(data, slot).ok_or(bad)?);
                }
                let mut layers = Vec::with_capacity(offsets.len());
                let mut height = 0;
                for offset in offsets {
                    let (id, below) = if offset == 0 {
                        self.refuse(ColrRefusal::Malformed { at: key })
                    } else {
                        self.child(list_at, offset, depth)
                    };
                    layers.push(id);
                    height = height.max(below + 1);
                }
                (Paint::Layers(layers), height)
            }
            2 | 3 => (
                Paint::Solid {
                    colour: ColourIndex::from_raw(u16_at(1)?),
                    alpha: s16_at(3)?,
                    var_index_base: if variable { Some(u32_at(5)?) } else { None },
                },
                0,
            ),
            // The table's own fields are read, and its colour line after them.
            4 | 5 => {
                let line_at = field(offset_at(1)? as usize);
                let (p0, p1, p2) = (point(4)?, point(8)?, point(12)?);
                let var_index_base = if variable { Some(u32_at(16)?) } else { None };
                let line = self.colour_line(line_at, variable).ok_or(bad)?;
                (
                    Paint::LinearGradient {
                        line,
                        p0,
                        p1,
                        p2,
                        var_index_base,
                    },
                    0,
                )
            }
            6 | 7 => {
                let line_at = field(offset_at(1)? as usize);
                let (c0, r0, c1, r1) = (point(4)?, u16_at(8)?, point(10)?, u16_at(14)?);
                let var_index_base = if variable { Some(u32_at(16)?) } else { None };
                let line = self.colour_line(line_at, variable).ok_or(bad)?;
                (
                    Paint::RadialGradient {
                        line,
                        c0,
                        r0,
                        c1,
                        r1,
                        var_index_base,
                    },
                    0,
                )
            }
            8 | 9 => {
                let line_at = field(offset_at(1)? as usize);
                let (centre, start_angle, end_angle) = (point(4)?, s16_at(8)?, s16_at(10)?);
                let var_index_base = if variable { Some(u32_at(12)?) } else { None };
                let line = self.colour_line(line_at, variable).ok_or(bad)?;
                (
                    Paint::SweepGradient {
                        line,
                        centre,
                        start_angle,
                        end_angle,
                        var_index_base,
                    },
                    0,
                )
            }
            10 => {
                let offset = offset_at(1)?;
                let glyph = u16_at(4)?;
                let (paint, below) = self.child(at, offset, depth);
                (Paint::Glyph { glyph, paint }, below + 1)
            }
            11 => {
                let glyph = u16_at(1)?;
                let root = self
                    .colr
                    .root_paint(glyph)
                    .ok_or(ColrRefusal::MissingColrGlyph(glyph))?;
                let (paint, below) = self.read(root, depth + 1);
                (Paint::ColrGlyph { glyph, paint }, below + 1)
            }
            12 | 13 => {
                let offset = offset_at(1)?;
                let affine = field(offset_at(4)? as usize);
                let mut matrix = [0i32; 6];
                for (i, value) in matrix.iter_mut().enumerate() {
                    *value = be32(data, affine.saturating_add(4 * i)).ok_or(bad)? as i32;
                }
                let var_index_base = if variable {
                    Some(be32(data, affine.saturating_add(24)).ok_or(bad)?)
                } else {
                    None
                };
                let (paint, below) = self.child(at, offset, depth);
                (
                    Paint::Transform {
                        matrix,
                        paint,
                        var_index_base,
                    },
                    below + 1,
                )
            }
            14..=31 => {
                let offset = offset_at(1)?;
                // Each family's fields after the child offset: the static
                // form's count of 16-bit values, two more for a centre, and
                // the variable form's index base after them.
                let (base, values) = match format {
                    14 | 15 => (14, 2),
                    16..=19 => (16, 2),
                    20..=23 => (20, 1),
                    24..=27 => (24, 1),
                    _ => (28, 2),
                };
                let centred = (format - base) >= 2;
                let count = values + if centred { 2 } else { 0 };
                let mut fields = [0i16; 4];
                for (i, field) in fields.iter_mut().enumerate().take(count) {
                    *field = s16_at(4 + 2 * i)?;
                }
                let var_index_base = if variable {
                    Some(u32_at(4 + 2 * count)?)
                } else {
                    None
                };
                let centre = centred.then(|| [fields[values], fields[values + 1]]);
                let (paint, below) = self.child(at, offset, depth);
                let node = match base {
                    14 => Paint::Translate {
                        dx: fields[0],
                        dy: fields[1],
                        paint,
                        var_index_base,
                    },
                    16 => Paint::Scale {
                        scale_x: fields[0],
                        scale_y: fields[1],
                        centre,
                        paint,
                        var_index_base,
                    },
                    20 => Paint::ScaleUniform {
                        scale: fields[0],
                        centre,
                        paint,
                        var_index_base,
                    },
                    24 => Paint::Rotate {
                        angle: fields[0],
                        centre,
                        paint,
                        var_index_base,
                    },
                    _ => Paint::Skew {
                        x_angle: fields[0],
                        y_angle: fields[1],
                        centre,
                        paint,
                        var_index_base,
                    },
                };
                (node, below + 1)
            }
            32 => {
                let source_offset = offset_at(1)?;
                let raw = u8_at(4)?;
                let backdrop_offset = offset_at(5)?;
                let mode = CompositeMode::from_raw(raw).unwrap_or_else(|| {
                    self.warn(ColrWarning::CompositeModeRead(raw));
                    CompositeMode::Clear
                });
                let (source, source_below) = self.child(at, source_offset, depth);
                let (backdrop, backdrop_below) = self.child(at, backdrop_offset, depth);
                (
                    Paint::Composite {
                        source,
                        mode,
                        backdrop,
                    },
                    source_below.max(backdrop_below) + 1,
                )
            }
            other => return Err(ColrRefusal::UnknownFormat(other)),
        })
    }
}
