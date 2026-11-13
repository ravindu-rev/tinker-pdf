//! `@font-face` resolved against the container: real faces, or a named reason
//! there is none (gap 31, milestone 9).
//!
//! `tinker-pdf-css` reads the rule and says nothing about whether any of it
//! resolves, because it has no archive and is not acquiring one. This is the
//! file that opens the `url()`, undoes whatever obfuscation covers it, unpacks
//! the web containers, refuses what is left **by name**, and hands
//! [`super::paint`] a set of faces with a `cmap` in them.
//!
//! # Every failure is named, and there are seven of them
//!
//! A book whose fonts do not arrive is set in the standard 14, which looks like
//! a book that never asked for fonts. That is gap 31's whole subject one level
//! down from a CSS property, so each way a face can fail to arrive is its own
//! [`FaceDefect`]: a format this build cannot read, a `local()` this build
//! cannot answer, a `url()` that names nothing in the container, an entry that
//! will not inflate, a key the package cannot produce, bytes that are not a
//! font, and a `src` list every entry of which failed. Seven, and not one
//! "the font did not load", because a producer told the fourth cannot fix the
//! first.
//!
//! # `format()` is a hint and the bytes are the answer
//!
//! §4.3 lets a sheet say what a file is and browsers use it to skip a download.
//! This build does the same — a `format("svg")` is refused without reading the
//! entry — **and** sniffs the bytes of everything it does read, because a
//! `format()` that lies is a real file and a producer that omits it is commoner
//! still. The two checks catch different books and a build with only one of
//! them would be wrong about the other's.
//!
//! The hint list is the part that moves. `woff` and `woff2` sat on it until
//! this build could read them, and a sheet writing
//! `url(x.woff2) format("woff2"), url(x.otf) format("opentype")` — which is
//! what a modern producer writes — reached the second entry and now reaches
//! the first. What is left on it is `embedded-opentype` and `svg`, two formats
//! with no reader here and no plan for one.
//!
//! # WOFF and WOFF2 are unpacked
//!
//! Both are a container around an sfnt: WOFF 1.0 deflates each table, WOFF2
//! rewrites `glyf` into a transformed form and Brotli-compresses the lot.
//! `tinker_pdf_font::woff` undoes both, and this file calls it on any `src`
//! entry whose bytes carry either signature — **after** de-obfuscation, since a
//! WOFF2 a book obfuscated is `wOF2` only once the XOR is undone.
//!
//! What is embedded is the sfnt that came out, because a PDF cannot carry a
//! WOFF: 9.9's font file streams are `/FontFile2` and `/FontFile3`, and neither
//! has a subtype for a web container. So the unpacking is not an optimisation,
//! it is the only way the face reaches a page.
//!
//! A container that will not unpack keeps its own row. It is now the *damaged*
//! ones rather than all of them, and [`FaceDefect::PackedContainer`] carries
//! the reason the decoder gave rather than only the format (ruling 10) — a
//! producer told "the glyf table's checksum disagrees" can fix a file, where
//! one told "the file is a woff container" can only be told again.
//!
//! # A variable face is drawn at the instance each run asks for
//!
//! A face whose `fvar` names an axis is a family of faces, and a run picks
//! one: `FaceSet::instance_for` turns what it asks for (`Wanted`) into
//! user coordinates by `css-fonts-4` §7.2's precedence, lowest first —
//! `font-weight` as `wght`, held to the `@font-face` weight descriptor's
//! range; `font-style: italic` as `ital` 1 and `oblique` as `slnt` at §2.4's
//! fourteen degrees, negated, since CSS measures an angle clockwise and
//! `slnt` counter-clockwise; the rule's `font-variation-settings`
//! descriptor; `font-optical-sizing: auto` as `opsz` at the used size in CSS
//! pixels; and the run's `font-variation-settings` last — and normalises
//! them, which holds each to its axis. Where they land on the face's default
//! the face is drawn as it is. Anywhere else the set writes the static
//! instance there (`tinker_pdf_font::instantiate`) the first time a run asks
//! for it, and every run asking for the same location is measured, shaped,
//! drawn and embedded in that one instance, under a resource of its own.
//! `wdth` stays at the face's default, since this build reads no
//! `font-stretch`.
//!
//! The instances are bounded twice: `MAX_INSTANCES` of them a book and
//! `MAX_INSTANCE_BYTES` between them. A run asking past either, or for an
//! instance the instancer refuses, is drawn at the face's default and the
//! book says so ([`FaceSet::instance_defects`]).

use std::sync::OnceLock;

use tinker_pdf_css::font_face::{FontFace, FontFormat, FontSource};
use tinker_pdf_css::property::{FontOpticalSizing, FontStyle, VariationSetting};
use tinker_pdf_font::instance::instantiate;
use tinker_pdf_font::variation::{DesignSpace, Instance, Location, UserCoordinate, VariationError};
use tinker_pdf_font::woff;
use tinker_pdf_font::Sfnt;
use tinker_pdf_layout::metrics::FontRequest;
use tinker_pdf_shape::Plans;

/// Why a WOFF container did not unpack, re-exported so that a caller matching
/// on [`FaceDefect::PackedContainer`] does not have to name
/// `tinker-pdf-font` to do it (ruling 11).
pub use tinker_pdf_font::woff::WoffError;

/// Why the instancer refused an instance, re-exported for
/// [`FaceDefect::InstanceRefused`] as [`WoffError`] is for
/// [`FaceDefect::PackedContainer`] (ruling 11).
pub use tinker_pdf_font::instance::InstanceError;

use super::obfuscation::{deobfuscate, KeyDefect};
use super::ocf::Encryption;
use super::read::{Resources, Unavailable};
use super::Limits;

/// `hhea`, as a big-endian table tag.
const HHEA: u32 = 0x6868_6561;

/// The largest sfnt a `@font-face` container is allowed to unpack to.
///
/// Not advisory, and not the same number as the entry's own size. A WOFF2
/// table directory states its lengths in `UIntBase128`, which reaches
/// 2^32 - 1 in five bytes — so a forty-byte entry in a book's ZIP can ask a
/// decoder for four gigabytes, and the ZIP's own bound says nothing about it
/// (ruling 1).
///
/// Sixteen megabytes is roughly twice the largest face in the fetched corpora,
/// which are CJK, and about forty times the largest a `@font-face` in the
/// committed books carries. A book that wanted more gets the row and the
/// family it fell back to, which is ruling 2 working as intended.
const MAX_UNPACKED_FACE: usize = 16 << 20;

/// The most static instances one book makes of its variable faces.
///
/// A run's instance follows its weight, its slope and — with
/// `font-optical-sizing` at its initial `auto` — its size, so a book can ask
/// for as many as it has distinct runs. Sixty-four is four weights in two
/// slopes at eight sizes of one family, which is more than a book sets its
/// headings and text in. A run asking for one more is drawn at its face's
/// default (ruling 2).
const MAX_INSTANCES: usize = 64;

/// The most bytes a book's instances hold between them.
///
/// Each is a whole face, so the count alone would let sixty-four copies of a
/// sixteen-megabyte face stand; this is four such faces' worth. An instance
/// is charged its face's size before it is written, which it does not pass:
/// the instance drops the variation tables and keeps the rest.
const MAX_INSTANCE_BYTES: usize = 4 * MAX_UNPACKED_FACE;

/// `oblique`'s angle where the run states none, `css-fonts-4` §2.4's
/// fourteen degrees.
const OBLIQUE_DEGREES: f64 = 14.0;

#[cfg(test)]
thread_local! {
    /// Static instances `FaceSet::instance_for` wrote, on this thread.
    pub(crate) static INSTANCES_WRITTEN: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Why one `@font-face` did not become a face.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaceDefect {
    /// The `src` entry declared a `format()` this build has no reader for,
    /// with the keyword the sheet wrote.
    UnsupportedFormat(String),
    /// The bytes are a WOFF or WOFF2 container that would not unpack.
    ///
    /// Before the decoders existed this was *every* WOFF: the container was
    /// recognised and declined. Now it is only the damaged ones, so the reason
    /// the decoder gave travels with it — ruling 10, and the difference
    /// between a producer who can fix a file and one who can only be told the
    /// format again.
    PackedContainer {
        /// The `css-fonts-4` §4.3 keyword for the container the bytes are.
        format: &'static str,
        /// What the decoder said, unabridged.
        why: WoffError,
    },
    /// `local()`: a face installed on the reading system, which this one has
    /// none of.
    LocalUnavailable,
    /// The `url()` resolves to nothing in the container.
    ResourceMissing,
    /// The entry is there and will not inflate.
    Unreadable,
    /// `encryption.xml` covers the resource and the key could not be derived.
    KeyUnavailable(KeyDefect),
    /// The bytes are not an sfnt this build reads.
    NotAFont,
    /// Every entry of the `src` list failed, so the family has no file.
    ///
    /// Reported **beside** the entry defects rather than instead of them: the
    /// entries say what went wrong and this says the rule as a whole came to
    /// nothing, and a book whose first `src` entry failed and whose second
    /// worked produces the first and not this.
    NoUsableSource,
    /// The face varies and the instance a run asked of it is one the instancer
    /// refuses, by name; that run is drawn at the face's default.
    ///
    /// An `fvar` or `avar` the variation reader refuses is
    /// [`InstanceError::Variation`] here, for every run the face is asked for:
    /// a face whose design space cannot be read has no instance but its
    /// default.
    InstanceRefused(InstanceError),
    /// The book asked its variable faces for more instances, or more bytes of
    /// them, than one book is given; each run asking for one past the bound
    /// is drawn at its face's default.
    InstancesSpent,
}

impl FaceDefect {
    /// A short name for a report to print.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            FaceDefect::UnsupportedFormat(name) => format!("src format({name}) is not read here"),
            FaceDefect::PackedContainer { format, why } => {
                format!("the {format} container would not unpack: {why}")
            }
            FaceDefect::LocalUnavailable => {
                "local() names a face this build has none of".to_owned()
            }
            FaceDefect::ResourceMissing => "the src url is not in the container".to_owned(),
            FaceDefect::Unreadable => "the container entry will not inflate".to_owned(),
            FaceDefect::KeyUnavailable(_) => "the obfuscation key cannot be derived".to_owned(),
            FaceDefect::NotAFont => "the bytes are not a font this build reads".to_owned(),
            FaceDefect::NoUsableSource => "no src entry produced a face".to_owned(),
            FaceDefect::InstanceRefused(why) => {
                format!("an instance of the variable face was refused: {why}")
            }
            FaceDefect::InstancesSpent => {
                "the book asked for more instances of its variable faces than it is given"
                    .to_owned()
            }
        }
    }
}

/// One face a book embedded, with its program and the descriptors it was
/// declared under.
#[derive(Clone, Debug)]
pub struct EmbeddedFace {
    /// The `@font-face` family, lower-cased.
    pub family: String,
    /// The `font-weight` descriptor, as a range.
    pub weight: (u16, u16),
    /// The `font-style` descriptor.
    pub style: FontStyle,
    /// The `font-variation-settings` descriptor: what a variable face is
    /// drawn at before a run's own optical size and variation settings.
    /// Empty where the rule wrote none.
    pub variations: Vec<VariationSetting>,
    /// The container path the program came from, which is what a report names.
    pub path: String,
    /// The font program, **de-obfuscated**, exactly as it is handed to the
    /// writer: for an instance of a variable face, the static instance.
    pub program: Vec<u8>,
    /// The PDF resource name this face is registered under.
    pub resource: Vec<u8>,
    /// The face's shaping plans and coverage indexes, kept beside its
    /// program for every run the book sets in it: measuring, painting and
    /// SVG text all shape through this one, so a stage plan is resolved and
    /// a lookup indexed once per face per book. Built as the book is shaped;
    /// a clone of the face starts it empty.
    pub plans: Plans,
}

impl EmbeddedFace {
    /// The glyph this face has for a character, if it has one.
    ///
    /// Glyph 0 is `.notdef` and a `cmap` that answers with it is a `cmap`
    /// saying *no* — so a face whose table maps every unmapped code to zero,
    /// which is what a format 4 subtable with a wide segment does, does not
    /// claim to cover the whole plane.
    #[must_use]
    pub fn glyph(&self, ch: char) -> Option<u16> {
        let sfnt = Sfnt::parse(&self.program)?;
        sfnt.glyph_for_char(ch).filter(|id| *id != 0)
    }

    /// Whether this face can draw a character.
    #[must_use]
    pub fn covers(&self, ch: char) -> bool {
        self.glyph(ch).is_some()
    }

    /// A character's advance, as a fraction of the em.
    ///
    /// From the face's own `hmtx`, which is the only number that can agree
    /// with the glyphs a viewer will draw. A face with no `hmtx` — a bare CFF
    /// wrapped in an `OTTO` with the table dropped by a bad subsetter — gets
    /// half an em rather than nothing, because a run measured at zero collapses
    /// to a point and a run measured at nothing cannot be laid out at all.
    #[must_use]
    pub fn advance_em(&self, ch: char) -> f64 {
        let Some(sfnt) = Sfnt::parse(&self.program) else {
            return 0.5;
        };
        let units = f64::from(sfnt.units_per_em.max(1));
        let glyph = sfnt.glyph_for_char(ch).unwrap_or(0);
        match sfnt.advance(glyph) {
            Some(advance) => f64::from(advance) / units,
            None => 0.5,
        }
    }

    /// The face's ascent and descent, as fractions of the em, both positive.
    ///
    /// `hhea`'s own numbers, with the descender's sign flipped because
    /// [`tinker_pdf_layout::metrics::Vertical`] measures a depth below the
    /// baseline as a positive number. A face with no readable `hhea` gets
    /// `None` and the caller uses the standard face's proportions, which is a
    /// worse answer than the file's and a much better one than zero.
    ///
    /// **`ascender` is at byte 4 and `descender` at byte 6.** Bytes 0 to 3 are
    /// the table's version, which is `0x0001_0000` in every font there has ever
    /// been — so a build that read the first two fields from the front of the
    /// table gets an ascent of **one unit** and a descent of zero for every
    /// face alike, which is a line height of a thousandth of an em and looks
    /// from a distance like a working build with tight leading. Milestone 9
    /// shipped that bug and `an_embedded_faces_line_height_is_its_own_hhea`
    /// found it, which is why the offsets are named here rather than left to
    /// the specification: the wrong ones are *plausible* and the right ones
    /// have to be argued for.
    #[must_use]
    pub fn vertical_fractions(&self) -> Option<(f64, f64)> {
        let sfnt = Sfnt::parse(&self.program)?;
        let hhea = sfnt.table(HHEA)?;
        let units = f64::from(sfnt.units_per_em.max(1));
        let ascender = i16::from_be_bytes([*hhea.get(4)?, *hhea.get(5)?]);
        let descender = i16::from_be_bytes([*hhea.get(6)?, *hhea.get(7)?]);
        if ascender <= 0 {
            return None;
        }
        Some((
            f64::from(ascender) / units,
            f64::from(descender.saturating_neg().max(0)) / units,
        ))
    }
}

/// Every face a book brought with it, and every reason one is missing.
#[derive(Clone, Debug, Default)]
pub struct FaceSet {
    faces: Vec<EmbeddedFace>,
    defects: Vec<(String, FaceDefect)>,
    /// The instances runs ask of the faces that vary, written as they are
    /// asked for; none for a set with no faces. Behind a box so that
    /// [`FaceSet::EMPTY`] holds no cell a `const` may not point at.
    instances: Option<Box<Instances>>,
}

/// What a run asks of a variable face: the inputs `css-fonts-4` §7.2 turns
/// into coordinates, kept by a plan that may choose the face for any of its
/// characters.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Wanted {
    weight: u16,
    style: FontStyle,
    /// `opsz` at the used size in CSS pixels, where `font-optical-sizing` is
    /// `auto`.
    optical_size: Option<f64>,
    variations: Vec<VariationSetting>,
}

impl Wanted {
    /// What `font` asks of a variable face.
    pub(crate) fn of(font: &FontRequest<'_>) -> Wanted {
        Wanted {
            weight: font.weight,
            style: font.style,
            optical_size: (font.optical_sizing == FontOpticalSizing::Auto)
                .then(|| font.size / super::read::PX_TO_PT),
            variations: font.variations.to_vec(),
        }
    }

    /// The user coordinates this asks of `face`, lowest precedence first, so
    /// that the instancer, which takes a tag's last value, gives each axis
    /// the one §7.2 lets stand: the weight held to the rule's descriptor,
    /// the slope, the rule's own settings, the optical size, then the run's
    /// settings.
    fn coordinates(&self, face: &EmbeddedFace) -> Vec<UserCoordinate> {
        let mut out = Vec::with_capacity(3 + face.variations.len() + self.variations.len());
        let (low, high) = face.weight;
        let weight = self.weight.max(low).min(high);
        out.push(UserCoordinate::new(*b"wght", f64::from(weight)));
        match self.style {
            FontStyle::Italic => out.push(UserCoordinate::new(*b"ital", 1.0)),
            FontStyle::Oblique => out.push(UserCoordinate::new(*b"slnt", -OBLIQUE_DEGREES)),
            FontStyle::Normal => {}
        }
        out.extend(
            face.variations
                .iter()
                .map(|setting| UserCoordinate::new(setting.tag, setting.value)),
        );
        if let Some(size) = self.optical_size {
            out.push(UserCoordinate::new(*b"opsz", size));
        }
        out.extend(
            self.variations
                .iter()
                .map(|setting| UserCoordinate::new(setting.tag, setting.value)),
        );
        out
    }
}

/// A face's design space, as the first run asking for an instance of it read
/// it.
#[derive(Clone, Debug)]
enum Space {
    /// No `fvar`, or one of no axes: the face is its only instance.
    Static,
    /// The axes, instances and segment maps.
    Varies(DesignSpace),
    /// An `fvar` or `avar` the reader refuses: the face is drawn at its
    /// default.
    Refused(VariationError),
}

/// One instance a run asked for.
#[derive(Clone, Debug)]
struct Instanced {
    /// The face it is an instance of, by index into [`FaceSet::faces`].
    base: usize,
    /// Where in that face's design space, normalised: what two runs share an
    /// instance by.
    location: Location,
    /// The instance, or why there is none.
    face: Result<EmbeddedFace, FaceDefect>,
}

/// The instances of a book's variable faces.
#[derive(Clone, Debug)]
struct Instances {
    /// Each face's design space, by its index, read once.
    spaces: Vec<OnceLock<Space>>,
    /// The instances, in the order runs first asked for them, each slot
    /// written once: [`MAX_INSTANCES`] of them.
    slots: Vec<OnceLock<Instanced>>,
    /// The family of the first face a run asked for an instance of with every
    /// slot written.
    spent: OnceLock<String>,
}

impl Instances {
    /// The store for a book's faces, or none for a book with none.
    fn over(faces: &[EmbeddedFace]) -> Option<Box<Instances>> {
        (!faces.is_empty()).then(|| {
            Box::new(Instances {
                spaces: faces.iter().map(|_| OnceLock::new()).collect(),
                slots: (0..MAX_INSTANCES).map(|_| OnceLock::new()).collect(),
                spent: OnceLock::new(),
            })
        })
    }
}

/// The static instance of `face` at `instance`, registered under `resource`:
/// the face it varies from in every descriptor, with the instance's program
/// and plans of its own.
fn write_instance(
    face: &EmbeddedFace,
    instance: &Instance,
    resource: Vec<u8>,
) -> Result<EmbeddedFace, FaceDefect> {
    #[cfg(test)]
    INSTANCES_WRITTEN.with(|n| n.set(n.get() + 1));
    let program = instantiate(&face.program, instance).map_err(FaceDefect::InstanceRefused)?;
    Ok(EmbeddedFace {
        family: face.family.clone(),
        weight: face.weight,
        style: face.style,
        variations: face.variations.clone(),
        path: face.path.clone(),
        program,
        resource,
        plans: Plans::new(),
    })
}

/// A face's design space, for [`Instances::spaces`].
fn space_of(program: &[u8]) -> Space {
    let Some(sfnt) = Sfnt::parse(program) else {
        return Space::Static;
    };
    match DesignSpace::read(&sfnt) {
        Ok(space) if space.is_variable() => Space::Varies(space),
        Ok(_) => Space::Static,
        Err(error) => Space::Refused(error),
    }
}

impl FaceSet {
    /// The shared empty set.
    ///
    /// A `const` reference rather than a `Default` call so that
    /// [`super::paint::BookMetrics::STANDARD`] can be a `const` too — which is
    /// what lets a caller name *the standard 14 and nothing else* without
    /// having a book to hand.
    pub const EMPTY: &'static FaceSet = &FaceSet {
        faces: Vec::new(),
        defects: Vec::new(),
        instances: None,
    };

    /// A book with no `@font-face` at all.
    #[must_use]
    pub fn new() -> FaceSet {
        FaceSet::default()
    }

    /// A set of the faces given, for the tests that hold font matching to
    /// faces of their own.
    #[cfg(test)]
    pub(crate) fn of(faces: Vec<EmbeddedFace>) -> FaceSet {
        let instances = Instances::over(&faces);
        FaceSet {
            faces,
            defects: Vec::new(),
            instances,
        }
    }

    /// The faces, in declaration order.
    #[must_use]
    pub fn faces(&self) -> &[EmbeddedFace] {
        &self.faces
    }

    /// The face a [`super::paint::Chosen::Embedded`] index names: one of
    /// [`FaceSet::faces`], or past them an instance of one a run asked for.
    /// `None` for an index that names neither.
    #[must_use]
    pub fn face(&self, index: usize) -> Option<&EmbeddedFace> {
        if let Some(face) = self.faces.get(index) {
            return Some(face);
        }
        let slot = index.checked_sub(self.faces.len())?;
        self.instances
            .as_deref()?
            .slots
            .get(slot)?
            .get()?
            .face
            .as_ref()
            .ok()
    }

    /// How many indices [`FaceSet::face`] may answer for: the declared faces
    /// and a slot for every instance the set may write.
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.faces.len()
            + self
                .instances
                .as_deref()
                .map_or(0, |instances| instances.slots.len())
    }

    /// Every face a page may draw in, by its index: the declared faces, then
    /// each instance written so far.
    pub fn drawn(&self) -> impl Iterator<Item = (usize, &EmbeddedFace)> {
        (0..self.face_count()).filter_map(|index| self.face(index).map(|face| (index, face)))
    }

    /// The face `index` is drawn in for a run asking `wanted`: the instance of
    /// it there where it varies and `wanted` lands off its default, written
    /// the first time a run asks for it and shared by every run after;
    /// `index` itself for a face that does not vary, at its default, past the
    /// bounds, or whose instance is refused.
    pub(crate) fn instance_for(&self, index: usize, wanted: &Wanted) -> usize {
        let (Some(face), Some(instances)) = (self.faces.get(index), self.instances.as_deref())
        else {
            return index;
        };
        let Some(space) = instances
            .spaces
            .get(index)
            .map(|slot| slot.get_or_init(|| space_of(&face.program)))
        else {
            return index;
        };
        let Space::Varies(space) = space else {
            return index;
        };
        let instance = Instance::User(wanted.coordinates(face));
        let Ok(location) = space.location(&instance) else {
            return index;
        };
        if location == space.default_location() {
            return index;
        }
        let mut held = 0usize;
        for (slot, cell) in instances.slots.iter().enumerate() {
            let entry = cell.get_or_init(|| {
                let made = if held.saturating_add(face.program.len()) > MAX_INSTANCE_BYTES {
                    Err(FaceDefect::InstancesSpent)
                } else {
                    let resource = format!("Bf{}", self.faces.len() + slot).into_bytes();
                    write_instance(face, &instance, resource)
                };
                Instanced {
                    base: index,
                    location: location.clone(),
                    face: made,
                }
            });
            if entry.base == index && entry.location == location {
                return match entry.face {
                    Ok(_) => self.faces.len() + slot,
                    Err(_) => index,
                };
            }
            if let Ok(made) = &entry.face {
                held = held.saturating_add(made.program.len());
            }
        }
        let _ = instances.spent.set(face.family.clone());
        index
    }

    /// What drawing the variable faces at the instances runs asked for could
    /// not do, as `(family, defect)` pairs: an instance refused or past the
    /// bytes, once for each location asked for, in the order runs asked; a
    /// design space refused, once for its face; and the instance count spent,
    /// once.
    #[must_use]
    pub fn instance_defects(&self) -> Vec<(String, FaceDefect)> {
        let Some(instances) = self.instances.as_deref() else {
            return Vec::new();
        };
        let family = |base: usize| {
            self.faces
                .get(base)
                .map(|face| face.family.clone())
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        for (base, space) in instances.spaces.iter().enumerate() {
            if let Some(Space::Refused(error)) = space.get() {
                out.push((
                    family(base),
                    FaceDefect::InstanceRefused(InstanceError::Variation(*error)),
                ));
            }
        }
        for cell in &instances.slots {
            if let Some(Instanced {
                base,
                face: Err(defect),
                ..
            }) = cell.get()
            {
                out.push((family(*base), defect.clone()));
            }
        }
        if let Some(spent) = instances.spent.get() {
            out.push((spent.clone(), FaceDefect::InstancesSpent));
        }
        out
    }

    /// What could not be loaded, as `(family, defect)` pairs in the order the
    /// rules were written.
    #[must_use]
    pub fn defects(&self) -> &[(String, FaceDefect)] {
        &self.defects
    }

    /// Whether this book has any embedded face at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The best face for a family at a weight and a slope **that covers a
    /// character**, or `None`.
    ///
    /// `css-fonts-4` §5.2's ordering — slope first, then weight — with §5.3's
    /// per-character step folded in: a family whose faces exist but none of
    /// which has the glyph is not a match for *this character*, and the caller
    /// moves to the next family. That is the difference between per-character
    /// fallback and a single face with holes in it, and it is why coverage is
    /// a parameter here rather than a filter the caller applies afterwards.
    ///
    /// `ch` of `None` asks the same question ignoring coverage, which is what a
    /// line height needs: a run's leading is a property of its nominal face
    /// and does not change part way along because one character fell through.
    #[must_use]
    pub fn best(
        &self,
        family: &str,
        weight: u16,
        style: FontStyle,
        ch: Option<char>,
    ) -> Option<usize> {
        let mut best: Option<(u32, u32, usize)> = None;
        for (index, face) in self.faces.iter().enumerate() {
            if !face.family.eq_ignore_ascii_case(family) {
                continue;
            }
            if let Some(ch) = ch {
                if !face.covers(ch) {
                    continue;
                }
            }
            let key = (
                style_distance(face.style, style),
                weight_distance(face.weight, weight),
                index,
            );
            // §5.2 sorts on slope before weight, and the index is the last
            // criterion so that two faces a book declared identically resolve
            // to the first — which makes the choice a function of the book
            // rather than of a hash order.
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best.map(|(_, _, index)| index)
    }

    /// Any face at all that covers a character, in declaration order.
    ///
    /// §5.3's system fallback, for a reading system whose "system" is the book:
    /// once the author's `font-family` list is exhausted a browser looks
    /// through the faces it has, and the faces this build has are the standard
    /// 14 and whatever the book embedded. Trying the book's own faces before
    /// giving up is what puts a CJK line on the page when the book shipped a
    /// CJK face under a family its `body` rule never mentions.
    #[must_use]
    pub fn any_covering(&self, ch: char) -> Option<usize> {
        self.faces.iter().position(|face| face.covers(ch))
    }
}

/// §5.2's slope distance: italic and oblique substitute for one another.
pub(crate) fn style_distance(have: FontStyle, want: FontStyle) -> u32 {
    match (have, want) {
        (a, b) if a == b => 0,
        (FontStyle::Italic, FontStyle::Oblique) | (FontStyle::Oblique, FontStyle::Italic) => 1,
        _ => 2,
    }
}

/// §5.2's weight distance, with the direction the specification prefers.
pub(crate) fn weight_distance(range: (u16, u16), wanted: u16) -> u32 {
    let (low, high) = range;
    if wanted >= low && wanted <= high {
        return 0;
    }
    let (distance, heavier) = if wanted < low {
        (u32::from(low - wanted), true)
    } else {
        (u32::from(wanted - high), false)
    };
    let deprioritised = if wanted <= 450 { heavier } else { !heavier };
    distance + u32::from(deprioritised) * 10_000
}

/// Whether a `format()` hint names a container this build has no reader for.
///
/// `None` means *read the bytes and find out*, which is what an absent hint and
/// a hint naming a plain sfnt both mean.
fn refused_by_hint(format: Option<&FontFormat>) -> Option<String> {
    match format? {
        // `Woff` and `Woff2` joined this list the day the decoders landed.
        // They are the reason the hint is checked at all — a sheet that writes
        // `url(x.woff2) format("woff2"), url(x.otf) format("opentype")` used to
        // reach the second entry because the first was refused unread, and now
        // reaches the first.
        FontFormat::OpenType
        | FontFormat::TrueType
        | FontFormat::Collection
        | FontFormat::Woff
        | FontFormat::Woff2 => None,
        // An unrecognised keyword is **not** refused on the hint: §4.3 makes
        // `format()` advisory, and a sheet that wrote a vendor keyword over a
        // perfectly ordinary OpenType file should still get its font. The
        // bytes decide.
        FontFormat::Other(_) => None,
        other => Some(other.name().to_owned()),
    }
}

/// Loads every `@font-face` a book declared.
///
/// `identifier` is the package's unique identifier, which is the only input
/// either obfuscation key has; `encryption` is what `META-INF/encryption.xml`
/// said covers which resource.
///
/// Faces are registered in declaration order and resource names follow that
/// order, so a document's font resources are a function of the book rather
/// than of the order the reader happened to meet its stylesheets in.
///
/// # One face per file, and the caller cannot do this part
///
/// The caller deduplicates the **rules** before calling, which collapses the
/// thirteen chapters that `<link>` one stylesheet into one rule: an imported
/// sheet's faces carry that sheet's address, and thirteen chapters linking it
/// produce thirteen identical [`FontFace`]s. A `<style>` element has no address
/// of its own, so its rules carry the **content document's** — which is a
/// different string in every chapter, and thirteen chapters with the same
/// `<style>` block are thirteen rules the caller cannot tell apart.
///
/// They resolve to one container entry, and that is what this sees. So a rule
/// whose family, weight, slope and resolved path match a face already loaded
/// adds no second face: a thirteen-chapter book with a two-megabyte CJK face in
/// a `<style>` block inflates and parses it **once**. The defect list is not
/// collapsed with it — thirteen rules that all failed are thirteen rules, and
/// [`super::ArchiveWarning::FontFace`]'s count is where that is said.
pub fn load<R: Resources + ?Sized>(
    book: &mut R,
    faces: &[FontFace],
    identifier: Option<&str>,
    encryption: &Encryption,
    limits: &Limits,
) -> FaceSet {
    let mut out = FaceSet::default();
    for rule in faces {
        match load_one(book, rule, identifier, encryption, limits, &mut out.defects) {
            Some((path, program)) => {
                if out.faces.iter().any(|face| {
                    face.path == path
                        && face.family == rule.family
                        && face.weight == rule.weight
                        && face.style == rule.style
                        && face.variations == rule.variations
                }) {
                    continue;
                }
                let resource = format!("Bf{}", out.faces.len()).into_bytes();
                out.faces.push(EmbeddedFace {
                    family: rule.family.clone(),
                    weight: rule.weight,
                    style: rule.style,
                    variations: rule.variations.clone(),
                    path,
                    program,
                    resource,
                    plans: Plans::new(),
                });
            }
            None => out
                .defects
                .push((rule.family.clone(), FaceDefect::NoUsableSource)),
        }
    }
    out.instances = Instances::over(&out.faces);
    out
}

/// Walks one rule's `src` list and returns the first entry that produced a
/// face.
///
/// **The list is walked to the end.** §4.3 makes it a preference order and a
/// build that stopped at the first failure would lose every book that writes
/// `url(x.woff2) format("woff2"), url(x.otf) format("opentype")` — which is
/// what a modern producer writes, with the entry this build cannot use first.
fn load_one<R: Resources + ?Sized>(
    book: &mut R,
    rule: &FontFace,
    identifier: Option<&str>,
    encryption: &Encryption,
    limits: &Limits,
    defects: &mut Vec<(String, FaceDefect)>,
) -> Option<(String, Vec<u8>)> {
    for source in &rule.sources {
        let (url, format) = match source {
            FontSource::Local(_) => {
                defects.push((rule.family.clone(), FaceDefect::LocalUnavailable));
                continue;
            }
            FontSource::Url { url, format } => (url, format.as_ref()),
        };
        if let Some(name) = refused_by_hint(format) {
            defects.push((rule.family.clone(), FaceDefect::UnsupportedFormat(name)));
            continue;
        }
        // A `<style>` element has no address of its own, and the content
        // document that holds it is the base the caller put in `base`.
        let base = rule.base.as_deref().unwrap_or("");
        let (path, bytes) = match book.fetch(base, url, limits) {
            Ok(found) => found,
            Err(Unavailable::Missing) => {
                defects.push((rule.family.clone(), FaceDefect::ResourceMissing));
                continue;
            }
            Err(Unavailable::Unreadable) => {
                defects.push((rule.family.clone(), FaceDefect::Unreadable));
                continue;
            }
        };
        let mut program = bytes;
        if let Some(entry) = encryption.entries().iter().find(|e| e.path == path) {
            if let Err(defect) =
                deobfuscate(entry.algorithm, identifier.unwrap_or(""), &mut program)
            {
                defects.push((rule.family.clone(), FaceDefect::KeyUnavailable(defect)));
                continue;
            }
        }
        // After de-obfuscation and not before: a WOFF2 that a book obfuscated
        // is `wOF2` only once the XOR is undone, and a build that sniffed the
        // obfuscated bytes would see neither the packed container nor a font.
        // §4.3's `format()` is a hint and these bytes are the answer, so the
        // signature decides here as it decides everywhere else in this file.
        if let Some(packing) = woff::packaging(&program) {
            match woff::decode(&program, MAX_UNPACKED_FACE) {
                Ok(sfnt) => program = sfnt,
                Err(why) => {
                    defects.push((
                        rule.family.clone(),
                        FaceDefect::PackedContainer {
                            format: packing.name(),
                            why,
                        },
                    ));
                    continue;
                }
            }
        }
        if Sfnt::parse(&program).is_none() {
            defects.push((rule.family.clone(), FaceDefect::NotAFont));
            continue;
        }
        return Some((path, program));
    }
    None
}
