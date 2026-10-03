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
//! For one, three and four channels nothing here evaluates a profile, so
//! nothing here can be wrong about one — only the profile's **header** is
//! read, for the channel count. The `nCLR` profiles are the exception, below:
//! `/ICCBased` cannot carry them, and the alternate a `/DeviceN` needs is the
//! profile evaluated.
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
//! # `nCLR`: a `/DeviceN` whose tint transform is the profile, evaluated
//!
//! Table 66 permits an `/ICCBased` space of **1, 3 or 4** components and no
//! others, so a profile of any other channel count — ECMA-388 15.2.5's
//! `2CLR` through `8CLR` — has no `/ICCBased` spelling at all. PDF's other
//! n-channel space is 8.6.6.5's `/DeviceN`: the components go into the content
//! stream unchanged, one colorant each, and a **tint transform** carries them
//! into an alternate space for a reader that has no such inks. Here that is
//! the one place this module evaluates a profile: `tinker-pdf-color`'s
//! transform, run at every point of a grid, written as a 7.10.2 sampled
//! function into `/DeviceRGB` (the sRGB the transform answers). The
//! translation of the components is as exact as for `/ICCBased`; the
//! alternate is as exact as the grid, which is as fine as
//! [`XPS_TINT_SAMPLES`] grid points allow — five a side for six channels.
//!
//! # What is narrowed, by name
//!
//! A channel count past [`MAX_XPS_DEVICE_N_CHANNELS`], or an `nCLR` profile
//! this build cannot evaluate — one with no `A2B*` table it reads — is
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
use tinker_pdf_cos::{DeviceSpace, DocumentBuilder, Function};
use tinker_pdf_xml::{Doctype, Event, Source};

use super::brush::ContextColour;
use super::markup::Trouble;
use super::opc::{Package, PartName};
use super::{dialect_of, Limits, XpsElementDefect};

/// The most channels an `nCLR` profile may have and still be placed as a
/// `/DeviceN` space.
///
/// **Eight**, which is ECMA-388's own: 15.2.5 permits `2CLR` through `8CLR`
/// for an n-channel colour. It also bounds the work the tint transform costs —
/// each grid point evaluates a lookup table's 2^n corners — which at eight is
/// 6 561 points of 256 corners each, and at ICC.1's fifteen would be 32 768 of
/// 32 768.
///
/// Reachable: `a_profile_past_eight_channels_is_a_named_narrowing`.
pub const MAX_XPS_DEVICE_N_CHANNELS: u8 = 8;

/// How many grid points the tint transform of an `nCLR` profile is sampled
/// at, at most.
///
/// Not a bound — nothing is refused at it — but a resolution: the grid is as
/// fine as this allows, the same count of points a side, never fewer than two.
/// Six channels are five a side (15 625 points), eight are three (6 561).
pub const XPS_TINT_SAMPLES: usize = 1 << 14;

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
    /// The profile, compiled, where `tinker-pdf-color` can evaluate it: what a
    /// gradient stop is converted to sRGB through (18.3.1.2), since a shading
    /// carries one colour space and a stop cannot bring its own. `None` for a
    /// profile embedded but not evaluable — a CMYK press profile with no
    /// `A2B*` table, say — which still embeds.
    pub transform: Option<icc::Transform>,
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

    /// A `ContextColor`'s sRGB, by its profile evaluated — `None` where the
    /// profile is not placed or cannot be evaluated.
    ///
    /// The components are padded or cut to the profile's own count, as the
    /// content stream writes them, and clamped to `[0, 1]` as 15.2.5 says.
    #[must_use]
    pub fn srgb(&self, page: &PartName, tint: &ContextColour) -> Option<[f64; 3]> {
        let placed = self.get(page, &tint.profile).ok()?;
        let transform = placed.transform.as_ref()?;
        let components: Vec<f64> = (0..usize::from(placed.channels))
            .map(|at| {
                tint.components
                    .get(at)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0)
            })
            .collect();
        let (r, g, b) = transform.apply(&components);
        Some([r, g, b].map(|v| f64::from(v) / 255.0))
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
        // Table 66 permits 1, 3 or 4 and no others; the rest are `nCLR`, and
        // a `/DeviceN` of their own.
        if !matches!(channels, 1 | 3 | 4) {
            let resource = format!("CS{}", self.next).into_bytes();
            self.next += 1;
            return device_n(bytes, channels, &resource, builder).map(|transform| Placed {
                resource,
                channels,
                transform: Some(transform),
            });
        }
        let profile = bytes.to_vec();
        let transform = icc::Profile::parse(bytes)
            .ok()
            .and_then(|profile| icc::Transform::compile(&profile))
            .filter(|transform| transform.inputs() == usize::from(channels));

        let resource = format!("CS{}", self.next).into_bytes();
        self.next += 1;
        if !builder.add_icc_color_space(&resource, &profile, channels) {
            // The writer refused it — an empty profile, or a count Table 66
            // does not allow. Nothing was written, and the element takes the
            // fallback rather than naming a space no page holds.
            return Err(XpsElementDefect::ColourProfileUnresolved);
        }
        Ok(Placed {
            resource,
            channels,
            transform,
        })
    }
}

/// An `nCLR` profile as 8.6.6.5's `/DeviceN`: one colorant a channel, and the
/// profile evaluated over a grid as the tint transform into `/DeviceRGB`.
///
/// The colorants are named `nCLR.1` through `nCLR.n` — `6CLR.3` for the third
/// of six — which no process or spot ink is called, so a reader with real
/// separations matches none of them and uses the transform, which is the
/// profile's own answer. The grid is 7.10.2's, the first channel varying
/// fastest, each point the sRGB `tinker-pdf-color` computes for it, the byte
/// widened to sixteen bits exactly (`v × 257`).
///
/// # Errors
/// [`XpsElementDefect::ColourProfileChannels`] for a profile past
/// [`MAX_XPS_DEVICE_N_CHANNELS`] or one with no transform this build reads,
/// and [`XpsElementDefect::ColourProfileUnresolved`] where the writer refused
/// the space.
fn device_n(
    bytes: &[u8],
    channels: u8,
    resource: &[u8],
    builder: &mut DocumentBuilder,
) -> Result<icc::Transform, XpsElementDefect> {
    if !(2..=MAX_XPS_DEVICE_N_CHANNELS).contains(&channels) {
        return Err(XpsElementDefect::ColourProfileChannels);
    }
    let transform = icc::Profile::parse(bytes)
        .ok()
        .and_then(|profile| icc::Transform::compile(&profile))
        .filter(|transform| transform.inputs() == usize::from(channels))
        .ok_or(XpsElementDefect::ColourProfileChannels)?;
    let n = usize::from(channels);
    // The finest grid of one count a side that fits: `side^n` points.
    let mut side = 2usize;
    while (side + 1)
        .checked_pow(u32::from(channels))
        .is_some_and(|points| points <= XPS_TINT_SAMPLES)
    {
        side += 1;
    }
    let points = side.pow(u32::from(channels));
    let last = (side - 1) as f64;
    let mut samples = Vec::with_capacity(points * 3);
    let mut components = vec![0.0f64; n];
    for index in 0..points {
        let mut rest = index;
        for slot in &mut components {
            *slot = (rest % side) as f64 / last;
            rest /= side;
        }
        let (r, g, b) = transform.apply(&components);
        samples.extend([r, g, b].map(|v| u16::from(v) * 257));
    }
    let tint = Function::Sampled {
        domain: vec![[0.0, 1.0]; n],
        range: vec![[0.0, 1.0]; 3],
        size: vec![u32::try_from(side).unwrap_or(2); n],
        samples,
    };
    let names: Vec<Vec<u8>> = (1..=n)
        .map(|k| format!("{channels}CLR.{k}").into_bytes())
        .collect();
    let names: Vec<&[u8]> = names.iter().map(Vec::as_slice).collect();
    if builder.add_device_n_color_space(resource, &names, DeviceSpace::Rgb, &tint, None) {
        Ok(transform)
    } else {
        Err(XpsElementDefect::ColourProfileUnresolved)
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
