//! 15.2.5's `ContextColor`: the ICC profile parts one fixed page names, read
//! before the drawing walk and embedded in the PDF as they stand.
//!
//! # A translation, not a conversion
//!
//! A `ContextColor` states a profile part, an alpha and the components — and
//! **nothing else**. There is no sRGB fallback in the syntax, so a consumer
//! without the profile has numbers and no statement about what they mean.
//!
//! This build does not guess at a colour. The profile goes into the PDF
//! verbatim as an `/ICCBased` colour space (8.6.5.5), the components go into
//! the content stream unchanged, and the *reader* does the colour management.
//! Nothing here evaluates a profile, so nothing here can be wrong about one —
//! which is why `tinker-pdf-color`'s transform machinery is not used at all and
//! only the profile's **header** is read, for the channel count.
//!
//! That distinction matters more than it looks: [`Profile::parse`] refuses a
//! profile it cannot build a transform out of, and a perfectly ordinary CMYK
//! press profile with no `A2B*` LUT is one of those. Embedding must not depend
//! on transforming, so [`icc::data_space`] reads §7.2's header and stops.
//!
//! # Why this is a pass and not a lookup
//!
//! `Package::read_part` hands back a borrow of the package and the drawing walk
//! is already holding one — the fixed page's own bytes. So the page is scanned
//! once for `ContextColor` attribute values, every part named is read, and the
//! walk does a pure lookup in the table this pass filled. That is
//! [`super::font::Fonts::load`]'s rule, [`super::image::Images::load`]'s rule
//! and [`super::resources::Remotes::load`]'s rule, for one reason.
//!
//! # What is narrowed, by name
//!
//! Table 66 permits an `/ICCBased` space of **1, 3 or 4** components and no
//! others. A profile with any other channel count — ICC.1's `nCLR` family runs
//! to fifteen — has no `/ICCBased` spelling at all, and PDF's other n-channel
//! space, `/DeviceN`, needs a tint transform into an alternate space that only
//! evaluating the profile could supply. So it is
//! [`XpsElementDefect::ColourProfileChannels`]: **named**, and painted in the
//! placeholder grey rather than in a colour picked by dropping components.
//!
//! A profile part that is missing or unreadable is
//! [`XpsElementDefect::ColourProfileUnresolved`], and the element still paints
//! — in 8.6.5.5's own default-`/Alternate` reading of the components, which is
//! what a PDF reader does with an `/ICCBased` stream it cannot use. Ruling 2:
//! the fallback is not invented here, it is the one PDF already specifies for
//! exactly this situation.

use std::collections::HashMap;

use tinker_pdf_color::icc;
use tinker_pdf_cos::DocumentBuilder;
use tinker_pdf_xml::{Doctype, Event, Source};

use super::markup::Trouble;
use super::opc::{Package, PartName};
use super::{dialect_of, Limits, XpsElementDefect};

/// One profile part, placed.
#[derive(Clone, Debug)]
pub struct Placed {
    /// The `/ColorSpace` resource name the space was registered under.
    pub resource: Vec<u8>,
    /// The number of components the profile's data space takes.
    ///
    /// Kept beside the resource because the content stream has to write
    /// exactly this many operands: 8.6.5.5's space says how many `scn` takes,
    /// and a `ContextColor` whose component count disagrees with its own
    /// profile is a file that contradicts itself.
    pub channels: u8,
}

/// Every ICC profile the `ContextColor`s of one document named.
#[derive(Default)]
pub struct Profiles {
    placed: HashMap<PartName, Result<Placed, XpsElementDefect>>,
    next: usize,
}

impl Profiles {
    /// Reads and registers every profile part one fixed page names.
    ///
    /// # Errors
    /// [`Trouble::Exhausted`] when one page names more distinct profiles than
    /// the package could hold parts, for [`super::image::Images::load`]'s
    /// reason: a page naming eight thousand profiles is not a page whose
    /// profiles eight thousand lookups would find.
    pub fn load(
        &mut self,
        package: &mut Package<'_>,
        page: &PartName,
        builder: &mut DocumentBuilder,
        limits: &Limits,
    ) -> Result<(), Trouble> {
        let wanted = match package.read_part(page) {
            Ok(bytes) => profiles_named(bytes, page, limits)?,
            // The page will not read at all. The painter reports that a moment
            // later, in its own words; this pass has nothing to add.
            Err(_) => return Ok(()),
        };
        if wanted.len() > limits.max_parts {
            return Err(Trouble::Exhausted);
        }
        for name in wanted {
            if self.placed.contains_key(&name) {
                continue;
            }
            let placed = self.place_one(package, &name, builder);
            self.placed.insert(name, placed);
        }
        Ok(())
    }

    /// The colour space a `ContextColor` on `page` named.
    ///
    /// Pure: resolution is arithmetic over two strings and the lookup is the
    /// table [`Profiles::load`] filled, so the drawing walk never needs the
    /// package.
    ///
    /// # Errors
    /// One [`XpsElementDefect`] per way a profile can fail to be a colour
    /// space, each by its own name.
    pub fn get(&self, page: &PartName, uri: &str) -> Result<&Placed, XpsElementDefect> {
        let name = page
            .resolve(uri)
            .ok_or(XpsElementDefect::ColourProfileUnresolved)?;
        match self.placed.get(&name) {
            Some(Ok(placed)) => Ok(placed),
            Some(Err(defect)) => Err(*defect),
            None => Err(XpsElementDefect::ColourProfileUnresolved),
        }
    }

    fn place_one(
        &mut self,
        package: &mut Package<'_>,
        name: &PartName,
        builder: &mut DocumentBuilder,
    ) -> Result<Placed, XpsElementDefect> {
        if !package.has(name) {
            return Err(XpsElementDefect::ColourProfileUnresolved);
        }
        let bytes = package
            .read_part(name)
            .map_err(|_| XpsElementDefect::ColourProfileUnresolved)?;
        // §7.2's header and nothing else. A profile whose header does not say
        // `acsp` is not a profile, and one whose data space this build cannot
        // name has no component count to write — both are the part failing to
        // be a profile rather than this build failing to transform one.
        let channels = icc::data_space(bytes)
            .and_then(icc::channels)
            .ok_or(XpsElementDefect::ColourProfileUnresolved)?;
        // Table 66 permits 1, 3 or 4 and no others.
        if !matches!(channels, 1 | 3 | 4) {
            return Err(XpsElementDefect::ColourProfileChannels);
        }
        let profile = bytes.to_vec();

        let resource = format!("CS{}", self.next).into_bytes();
        self.next += 1;
        if !builder.add_icc_color_space(&resource, &profile, channels) {
            // The writer refused it — an empty profile, or a count Table 66
            // does not allow. Nothing was written, and the element takes the
            // fallback rather than naming a space no page holds.
            return Err(XpsElementDefect::ColourProfileUnresolved);
        }
        Ok(Placed { resource, channels })
    }
}

/// Every profile part the `ContextColor`s of one page name.
///
/// Scanned with the streaming reader rather than the materialised tree, which
/// is [`super::image::Images::load`]'s own choice and for its reason: this runs
/// before the drawing walk and has no tree to read.
///
/// **Every attribute is looked at**, not a fixed list of names. 15.2.5's colour
/// is a value and not an element, so it may stand wherever a brush-valued
/// property does — `Fill` and `Stroke` on a `Path`, `Fill` on a `Glyphs`,
/// `Color` on a `SolidColorBrush` or a `GradientStop`, and an `OpacityMask` —
/// and a pass that listed those would be a list to forget the next one from.
fn profiles_named(
    bytes: &[u8],
    page: &PartName,
    limits: &Limits,
) -> Result<Vec<PartName>, Trouble> {
    let Ok(source) = Source::new(bytes) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<PartName> = Vec::new();
    for event in source.reader_with(&limits.xml, Doctype::Refuse) {
        let element = match event {
            Ok(Event::Start(element)) => element,
            Ok(_) => continue,
            // Markup that will not read is the painter's to report, and it
            // will: this pass answers with what it found and the drawing walk
            // meets the same failure a moment later.
            Err(_) => break,
        };
        if dialect_of(element.namespace()).is_none() {
            continue;
        }
        for attribute in element.attributes() {
            let Some(profile) = profile_of(attribute.value()) else {
                continue;
            };
            let Some(name) = page.resolve(profile) else {
                continue;
            };
            if !out.contains(&name) {
                out.push(name);
                if out.len() > limits.max_parts {
                    return Err(Trouble::Exhausted);
                }
            }
        }
    }
    Ok(out)
}

/// The profile URI of a `ContextColor` attribute value, if it is one.
///
/// The same shape [`super::brush::context_colour`] parses, read here only as
/// far as the URI: this pass decides *which parts to read* and the brush module
/// decides what the value means. Two readings of one grammar, and the narrow
/// one cannot accept something the full one rejects — it answers a part name,
/// and a value the brush module then refuses simply leaves a part read and
/// unused.
fn profile_of(value: &str) -> Option<&str> {
    let rest = value.trim().strip_prefix("ContextColor")?;
    if !rest.starts_with([' ', '\t', '\r', '\n']) {
        return None;
    }
    let (profile, _) = rest.trim_start().split_once(char::is_whitespace)?;
    (!profile.is_empty()).then_some(profile)
}
