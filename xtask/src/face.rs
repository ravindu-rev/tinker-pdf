//! A TrueType face this repository writes for itself.
//!
//! ## Why there is one at all
//!
//! The corpus's degradation rate is measured **without** font faces, because
//! this engine bundles none: a document that names Helvetica and embeds
//! nothing extracts its text perfectly and draws none of it, and every one of
//! those files counts as degraded. So the figure measures the font policy as
//! much as it measures the engine, `corpus/ratchet.json` says so in its own
//! note, and the roadmap asks for the other number.
//!
//! ## Why it is synthesised rather than fetched
//!
//! `crates/tinker-pdf/tests/substitute_fonts.rs` already builds a face on the
//! spot, and says why: "the test is identical on every platform and the
//! repository carries no font anyone has to licence". The same two reasons
//! apply to a corpus bar and a third joins them — a bar measured against
//! whatever face a runner image happened to ship is not a bar, for exactly the
//! reason `corpus-run` spawns a `tpdf` built from its own revision rather than
//! one found on `PATH`.
//!
//! ## What it can and cannot say
//!
//! Every glyph from 32 up is the same filled box. That answers **"was a face
//! available"** and nothing else: the shapes are wrong, and a bar measured
//! with this face says how much of the degradation was the absence of a face
//! rather than a defect in the engine. It cannot say that the text was set
//! correctly, and the ratchet's note says so where somebody reading the number
//! will see it.
//!
//! The face is versioned in that note, as `synthetic-1`. Changing what this
//! writes invalidates the bar rather than silently moving it.

use std::path::{Path, PathBuf};

/// The name the ratchet records for a run measured with this face.
///
/// Versioned on purpose. A different face is a different measurement, and the
/// comparator already refuses to compare two runs whose `--fonts` differ — so
/// the version has to be *in* the setting for that refusal to catch a change
/// to the face itself.
pub const SYNTHETIC: &str = "synthetic-1";

/// Where `--fonts synthetic` puts the face.
///
/// Under `target/`, because it is built rather than committed, and the same
/// path every time so a re-run does not accumulate copies.
#[must_use]
pub fn default_path(root: &Path) -> PathBuf {
    root.join("target/fonts/tinker-synthetic-1.ttf")
}

/// Writes the face, creating its directory.
pub fn write(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The lowest character code that gets a glyph. Below 32 are the controls.
const FIRST: usize = 32;
/// The highest. A simple font's codes are bytes, so this is all of them.
const LAST: usize = 255;
/// Design units per em, and the side of the box in them.
const UNITS_PER_EM: u16 = 1000;
/// The box's side, leaving a little bearing inside the em.
const SIDE: i16 = 700;

/// The face, as bytes.
///
/// Three tables and no more: `head` for the units per em and the `loca`
/// format, `loca` for where each glyph is, and `glyf` for the glyph itself.
/// A `cmap` is deliberately absent — a simple font's code *is* its glyph index
/// through this path, and a `cmap` would add a second opinion about that.
/// `hmtx` is absent for the same kind of reason: a PDF carries its own
/// `/Widths`, and a face that disagreed with them would move the text.
#[must_use]
pub fn bytes() -> Vec<u8> {
    // A filled square, as one contour of four on-curve points.
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes()); // one contour
    glyph.extend_from_slice(&0i16.to_be_bytes()); // xMin
    glyph.extend_from_slice(&0i16.to_be_bytes()); // yMin
    glyph.extend_from_slice(&SIDE.to_be_bytes()); // xMax
    glyph.extend_from_slice(&SIDE.to_be_bytes()); // yMax
    glyph.extend_from_slice(&3u16.to_be_bytes()); // last point of contour 0
    glyph.extend_from_slice(&0u16.to_be_bytes()); // no instructions
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on-curve, word deltas
    for dx in [0i16, SIDE, 0, -SIDE] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, SIDE, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&UNITS_PER_EM.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    // Each glyph needs its own slice of `glyf`, so the shape is repeated
    // rather than pointed at: a glyph is empty precisely when its `loca` entry
    // equals the next one, and offsets that merely repeat make every glyph
    // empty — which draws nothing and looks exactly like a face that failed to
    // load.
    let size = glyph.len() as u32;
    let mut glyf = Vec::with_capacity(glyph.len() * (LAST + 1 - FIRST));
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }

    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        let offset = (index.saturating_sub(FIRST)) as u32 * size;
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The face is the same bytes every time.
    ///
    /// Not decoration: the bar recorded against it is only a bar if the thing
    /// it was recorded against does not drift. A clock, a hash-map iteration
    /// or an environment lookup in here would make two runs on one machine
    /// disagree, and the ratchet would report it as an engine regression.
    #[test]
    fn the_face_is_the_same_bytes_every_time() {
        assert_eq!(bytes(), bytes());
        assert!(bytes().len() > 4096, "it has glyphs in it");
    }

    /// A TrueType face, by its own header.
    #[test]
    fn it_is_a_truetype_face_with_the_three_tables() {
        let face = bytes();
        assert_eq!(&face[0..4], &0x0001_0000u32.to_be_bytes(), "sfnt version");
        assert_eq!(u16::from_be_bytes([face[4], face[5]]), 3, "three tables");
        for tag in [b"head", b"loca", b"glyf"] {
            assert!(
                face.windows(4).any(|w| w == tag),
                "{} is in the table directory",
                String::from_utf8_lossy(tag)
            );
        }
    }
}
