//! A page's spot colorants (8.6.6.4, 8.6.6.5), behind
//! [`Page::spot_colorants`](crate::Page::spot_colorants), and its plates,
//! behind [`Page::separations`](crate::Page::separations).
//!
//! Feature documentation: `docs/features/rendering.md`, "Separations"; the
//! design is `docs/design/overprint-separations.md`.
//!
//! # What the inventory is
//!
//! Every colorant a `/Separation` or `/DeviceN` space on the page names that
//! is a spot: not one of the four names 8.6.6.4 reserves for a CMYK device's
//! process colorants, not `/All`, not `/None`, and not a process component an
//! `NChannel` space's `/Process` names. Each is listed once, in the order the
//! page's scopes first name it, and the order is the plan a separations
//! render lays its spot plates out in.
//!
//! # How it is read
//!
//! **One walk and one reader, both shared.** The scopes are the ones
//! [`Document::fonts`](crate::Document::fonts) walks — `fontlist`'s walk, a
//! page's `/Resources`, each scope it opens and each annotation's appearance
//! — with what a render draws: a soft mask's group entered, and an
//! annotation's normal appearances alone. A space's names are read as
//! `Page::images` and the renderer read them (`resources::separation_name`,
//! `resources::device_n_names`), so the three cannot disagree about what a
//! space is called.
//!
//! Within one `/Resources` dictionary the spaces are read in this order, each
//! subdictionary's entries in the order it holds them: its `/ColorSpace`
//! entries; the spaces of its `/XObject` entries — an image's `/ColorSpace`
//! and a form's transparency group's `/CS`; each `/Shading`'s space; each
//! shading pattern's shading's; and the group of each graphics state's soft
//! mask. A space's own spaces follow it: an `/Indexed` base, a `/Pattern`
//! space's underlying space. Within a `/DeviceN` its names come in array
//! order, then its `/Colorants` keys. Then the scopes the dictionary opens,
//! in the walk's order — Type 3 fonts, graphics states' fonts and soft masks'
//! groups, forms, tiling patterns — and last each annotation's appearance,
//! its group's space before its resources.
//!
//! # What it does not do
//!
//! **Nothing is decoded and nothing is evaluated.** No profile, palette or
//! tint transform stream is read, so listing adds nothing to
//! [`Document::warnings`](crate::Document::warnings), and asking twice gives
//! the same answer as asking once. A scope already walked is not walked again
//! — the cycle guard — the walk stops at its depth bound, and the listing at
//! [`MAX_LISTED_SPOTS`], each with what it had read (ruling 1).

use std::collections::BTreeSet;
use std::sync::Arc;

use tinker_pdf_cos::{pages as cos_pages, CosDocument, Dict, Name, ObjRef, Object};
use tinker_pdf_render::{PlatePlan, PlateUse};

use crate::fontlist::{self, ScopeVisitor};
use crate::resources::{device_n_attributes, device_n_names, separation_name};
use crate::{PixelFormat, RenderOptions, RenderWarning, Stats};

/// The colorant names 8.6.6.4 reserves for the process colorants of a CMYK
/// device: a `/Separation` or `/DeviceN` naming one paints a process plate,
/// never a spot plate of the same name.
pub(crate) const PROCESS_COLORANTS: [&[u8]; 4] = [b"Cyan", b"Magenta", b"Yellow", b"Black"];

/// How many spot colorants one page's inventory lists before it stops.
///
/// A bound rather than a budget, as `fontlist`'s is: each name is bytes the
/// file holds, so an honest page never comes near it, and a hostile one must
/// not make the listing allocate without limit (ruling 1). Far past what a
/// separations render carries as plates, which composite every spot past
/// their own cap through its alternate.
const MAX_LISTED_SPOTS: usize = 4096;

/// How deep a colour space's own spaces are followed: an `/Indexed` base and
/// a `/Pattern` space's underlying space are spaces too. The renderer's
/// reader and `Page::images`' stop where this does.
const MAX_SPACE_DEPTH: u32 = 8;

/// One spot colorant a page's resources name (8.6.6.4, 8.6.6.5).
///
/// A colorant of a `/Separation` or `/DeviceN` space that is not one of the
/// four process colorants 8.6.6.4 reserves — `Cyan`, `Magenta`, `Yellow`,
/// `Black` — nor `/All` or `/None`, nor a process component an `NChannel`
/// space's `/Process` names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SpotColorant {
    /// The colorant's name as the space writes it, `#` escapes decoded
    /// (7.3.5): `/PANTONE#20131` is `PANTONE 131`.
    pub name: Vec<u8>,
}

impl crate::Page {
    /// The spot colorants this page's resources name (8.6.6.4, 8.6.6.5),
    /// each once, in the order its scopes first name them: the plan a
    /// separations render lays its spot plates out in.
    ///
    /// A spot is a colorant of a `/Separation` or `/DeviceN` space that is
    /// not one of the four process colorants 8.6.6.4 reserves, nor `/All`
    /// or `/None`, nor a process component an `NChannel` space's `/Process`
    /// names. The scopes are every one a render of the page enters — its
    /// `/Resources`, each form's, tiling pattern's, Type 3 font's and soft
    /// mask's group's within them, and each annotation's normal appearances
    /// — and the spaces every one its `/ColorSpace`, its images, shadings,
    /// shading patterns and groups are in, with an `/Indexed` base, a
    /// `/Pattern` space's underlying space and a `/DeviceN`'s `/Colorants`.
    ///
    /// Read, never evaluated: no stream is decoded, so listing adds nothing
    /// to [`crate::Document::warnings`].
    #[must_use]
    pub fn spot_colorants(&self) -> Vec<SpotColorant> {
        of_page(&self.doc, &self.inner)
    }

    /// The page's separations: an image of ink for each colorant
    /// (`docs/design/overprint-separations.md`, "Plates handed out") — the
    /// four process plates of a CMYK device, cyan, magenta, yellow and
    /// black, then a plate for each spot colorant the page's inventory names
    /// ([`crate::Page::spot_colorants`]), in its order.
    ///
    /// Each colorant is [`PlateUse::Spot`], its own plate, unless
    /// [`SeparationOptions::plates`] says otherwise. [`PlateUse::Composite`]
    /// draws a spot through its alternate on the process plates, as a page
    /// without the colorant draws it, and hands back no plate for it;
    /// [`PlateUse::Disabled`] keeps its plate while the page is drawn — so it
    /// knocks out, and is knocked out by, every other paint exactly as a spot
    /// plate is — and leaves it out of what is handed back. A process
    /// colorant asked to be composite is a plate still: the process plates
    /// are what a composite lands on.
    ///
    /// Drawn through the pipeline [`crate::Page::render`] draws through
    /// (ruling 5): the region, the scale and its clamp, the rotation, the
    /// crop box, the antialiasing and the warnings are that render's, so a
    /// region's plates are the rectangle of the page's plates under it, byte
    /// for byte. Every paint knocks out the plates it names no ink for, as
    /// 8.6.7 has a paint do with overprint off. A page whose transparency
    /// group declares a space other than a four-component subtractive one
    /// composites in that space, every spot through its alternate (11.7.3),
    /// and its plates are that light turned into ink.
    ///
    /// At most [`crate::MAX_SPOT_PLATES`] spot plates, and at most as many as
    /// keep the page's plates — every channel and the alpha, a byte each, at
    /// each pixel of the whole page — inside [`crate::MAX_PLATE_BYTES`]: the
    /// spots past either, in inventory order, are drawn as composites, and
    /// the render says so ([`RenderWarning::PlatesCapped`]).
    #[must_use]
    pub fn separations(&self, options: &SeparationOptions) -> Separations {
        let render = &options.render;
        // The page's plates, whatever region is asked for, so a tile keeps
        // the plates the page keeps (ruling 5).
        let (width, height) = self.pixel_size(render);
        let planned = plan(
            &self.spot_colorants(),
            &options.plates,
            u64::from(width) * u64::from(height),
        );
        let op = self.render_operation(render, render.usage);
        let content = if render.content {
            crate::stats::content(&self.doc, &self.inner, op.work())
        } else {
            Vec::new()
        };
        let resources = crate::resources::PageResources::new_in(
            &self.doc,
            &self.inner,
            self.fonts.as_ref(),
            op,
        );
        let crate::Sheeted { canvas, met, .. } = self.draw_plates(
            render,
            &resources,
            Arc::clone(&planned.plan),
            |renderer, resources| self.paint_page(render, &content, renderer, resources),
        );
        // A page group of another space composited in it: its light, turned
        // into ink on the process plates, every spot at no ink.
        let format = PixelFormat::Plates {
            spots: planned.plan.spots(),
        };
        let canvas = if canvas.format == format {
            canvas
        } else {
            canvas.extract(canvas.origin(), canvas.width, canvas.height, format)
        };
        let plates = planned
            .plan
            .channels()
            .enumerate()
            .filter(|(_, (_, behaviour))| *behaviour != PlateUse::Disabled)
            .map(|(channel, (name, _))| Plate {
                name: name.to_vec(),
                kind: if channel < 4 {
                    PlateKind::Process
                } else {
                    PlateKind::Spot
                },
                tints: canvas.channel(channel).unwrap_or_default(),
            })
            .collect();
        let alpha = render.transparent.then(|| {
            let components = canvas.format.components();
            canvas
                .data
                .chunks_exact(components)
                .map(|pixel| pixel.last().copied().unwrap_or(255))
                .collect()
        });
        let mut warnings = met.warnings();
        if planned.kept < planned.named {
            let capped = RenderWarning::PlatesCapped {
                named: planned.named,
                kept: planned.kept,
            };
            if !warnings.contains(&capped) {
                warnings.push(capped);
            }
        }
        Separations {
            width: canvas.width,
            height: canvas.height,
            plates,
            alpha,
            warnings,
            stats: resources.work().map(|work| work.stats()),
        }
    }
}

/// How [`Page::separations`](crate::Page::separations) draws a page's
/// plates.
#[derive(Clone, Debug, Default)]
pub struct SeparationOptions {
    /// The render, as [`crate::Page::render`] takes it: the region, the scale,
    /// the antialiasing, the levels and the thinnest line, the transparency,
    /// the cancel token, what is drawn, the optional-content states and the
    /// counting. Its `format`, `allow_cmyk`, `premultiplied` and
    /// `forced_colours` say how light comes back and are not read, since
    /// plates are tints; the page is drawn on the caller's thread whatever its
    /// `parallelism` asks.
    pub render: RenderOptions,
    /// What each colorant's plate does, by name — a process colorant
    /// (`Cyan`, `Magenta`, `Yellow`, `Black`) or a spot of the page's
    /// inventory — a name's first entry deciding. A colorant it does not name
    /// is [`PlateUse::Spot`], and a name the page has no colorant of is passed
    /// over.
    pub plates: Vec<(Vec<u8>, PlateUse)>,
}

/// What kind of colorant a [`Plate`] carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlateKind {
    /// One of the four process colorants of a CMYK device, which 8.6.6.4
    /// reserves the names `Cyan`, `Magenta`, `Yellow` and `Black` for.
    Process,
    /// A spot colorant a `/Separation` or `/DeviceN` space names.
    Spot,
}

/// One plate of a page's separations: an image of one colorant's ink, a
/// byte a pixel.
///
/// Not a [`crate::Bitmap`]: a one-channel bitmap reads as grey, which is
/// light, and a plate read as grey is its own negative.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Plate {
    /// The colorant's name, as the page's spaces write it, `#` escapes
    /// decoded (7.3.5).
    pub name: Vec<u8>,
    /// Whether the colorant is a process one or a spot.
    pub kind: PlateKind,
    /// The colorant's tint at each pixel, row by row from the top, the
    /// separations' width by their height: 0 no ink, 255 solid.
    pub tints: Vec<u8>,
}

/// A page's separations, behind [`Page::separations`](crate::Page::separations).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Separations {
    /// Width in pixels: the page's at the render's scale, or the region's.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Each plate the plan keeps: the process plates, then the spots in
    /// inventory order, a disabled one left out.
    pub plates: Vec<Plate>,
    /// With `RenderOptions::transparent`, how much of each pixel the page
    /// covered, row by row, 255 wholly; `None` otherwise, where the plates
    /// stand on paper and every pixel is whole.
    pub alpha: Option<Vec<u8>>,
    /// What the render met, as [`crate::Bitmap::warnings`] says it, and
    /// [`RenderWarning::PlatesCapped`] where spots past a cap were drawn as
    /// composites.
    pub warnings: Vec<RenderWarning>,
    /// What the render did, where `RenderOptions::stats` asked
    /// ([`crate::Bitmap::stats`]).
    pub stats: Option<Stats>,
}

/// The plate plan a separations render draws by, and what its caps left:
/// how many spots asked for a plate, and how many kept one.
struct Planned {
    plan: Arc<PlatePlan>,
    named: usize,
    kept: usize,
}

/// The plan for a page of `inventory` and `pixels` pixels, each colorant's
/// use `asked`'s first entry for its name or [`PlateUse::Spot`]: the four
/// process plates, then each spot of the inventory, in its order, that asks
/// for a plate — Spot or Disabled — while fewer than
/// [`tinker_pdf_render::spots_within_bytes`] allows have one, the rest drawn as
/// composites.
fn plan(inventory: &[SpotColorant], asked: &[(Vec<u8>, PlateUse)], pixels: u64) -> Planned {
    let behaviour = |name: &[u8]| {
        asked
            .iter()
            .find(|(named, _)| named.as_slice() == name)
            .map_or(PlateUse::Spot, |(_, behaviour)| *behaviour)
    };
    let cap = tinker_pdf_render::spots_within_bytes(pixels);
    let mut colorants: Vec<(Vec<u8>, PlateUse)> = PROCESS_COLORANTS
        .iter()
        .map(|name| (name.to_vec(), behaviour(name)))
        .collect();
    let (mut named, mut kept) = (0, 0);
    for spot in inventory {
        let mut behaviour = behaviour(&spot.name);
        if behaviour != PlateUse::Composite {
            named += 1;
            if kept < cap {
                kept += 1;
            } else {
                behaviour = PlateUse::Composite;
            }
        }
        colorants.push((spot.name.clone(), behaviour));
    }
    Planned {
        plan: Arc::new(PlatePlan::new(colorants)),
        named,
        kept,
    }
}

/// The spot colorants `page`'s scopes name, each once, in the order the walk
/// first meets it (the module comment).
pub(crate) fn of_page(doc: &Arc<CosDocument>, page: &cos_pages::Page) -> Vec<SpotColorant> {
    let mut inventory = Inventory::default();
    let mut scopes = BTreeSet::new();
    fontlist::walk_page(doc, page, &mut scopes, &mut inventory);
    inventory.spots
}

/// The inventory's side of the walk.
#[derive(Default)]
struct Inventory {
    /// What has been listed, in order.
    spots: Vec<SpotColorant>,
    /// The names in `spots`, so a name is listed once.
    names: BTreeSet<Vec<u8>>,
    /// Every colour space object already read, so a space a hundred scopes
    /// name is read once.
    spaces: BTreeSet<ObjRef>,
    /// Every Type 3 font whose resources the walk has entered.
    fonts: BTreeSet<ObjRef>,
}

impl ScopeVisitor for Inventory {
    fn full(&self) -> bool {
        self.spots.len() >= MAX_LISTED_SPOTS
    }

    fn scope(&mut self, doc: &Arc<CosDocument>, resources: &Dict) {
        // 8.6.3: the spaces the scope names, which `cs`, `CS` and an inline
        // image's `/CS` select by name.
        let table = doc.resolve_key(resources, Name::COLOR_SPACE);
        for (_, space) in table.as_dict().into_iter().flat_map(Dict::iter) {
            self.space(doc, space, 0);
        }
        // An image's own space (8.9.5), and a form's group's (11.6.6).
        let table = doc.resolve_key(resources, Name::XOBJECT);
        for (_, entry) in table.as_dict().into_iter().flat_map(Dict::iter) {
            let object = doc.resolve(entry);
            if let Some(dict) = object.as_dict() {
                self.xobject(doc, dict);
            }
        }
        // A shading's (8.7.4.3), painted by `sh`.
        let table = doc.resolve_key(resources, Name::SHADING);
        for (_, entry) in table.as_dict().into_iter().flat_map(Dict::iter) {
            let object = doc.resolve(entry);
            if let Some(dict) = object.as_dict() {
                self.shading(doc, dict);
            }
        }
        // A shading pattern's shading (8.7.4.1); a tiling pattern's cell is a
        // scope of its own, which the walk enters.
        let table = doc.resolve_key(resources, Name::PATTERN);
        for (_, entry) in table.as_dict().into_iter().flat_map(Dict::iter) {
            let object = doc.resolve(entry);
            let Some(pattern) = object.as_dict() else {
                continue;
            };
            let kind = doc.resolve_key(pattern, doc.intern(b"PatternType"));
            if kind.as_int() != Some(2) {
                continue;
            }
            let shading = doc.resolve_key(pattern, Name::SHADING);
            if let Some(dict) = shading.as_dict() {
                self.shading(doc, dict);
            }
        }
        // A soft mask's group (11.6.5.2), whose contents the walk enters too.
        let table = doc.resolve_key(resources, Name::EXT_G_STATE);
        for (_, entry) in table.as_dict().into_iter().flat_map(Dict::iter) {
            let state = doc.resolve(entry);
            let Some(state) = state.as_dict() else {
                continue;
            };
            if let Some((_, group)) = fontlist::soft_mask_group(doc, state) {
                if let Some(form) = group.as_dict() {
                    self.group(doc, form);
                }
            }
        }
    }

    fn font(
        &mut self,
        doc: &Arc<CosDocument>,
        _label: Option<String>,
        reference: Option<ObjRef>,
        dict: &Dict,
    ) -> bool {
        // 9.6.5: only a Type 3 font's glyphs run in resources of their own.
        // Its `/Subtype` as written, the test the font listing makes.
        let subtype = doc.resolve_key(dict, Name::SUBTYPE);
        let type3 = subtype
            .as_name()
            .and_then(|name| doc.name_bytes(name))
            .is_some_and(|name| name.as_ref() == b"Type3");
        type3 && reference.is_none_or(|reference| self.fonts.insert(reference))
    }

    fn appearance(&mut self, doc: &Arc<CosDocument>, stream: &Dict) {
        // An appearance is a form (12.5.5), and may be a group.
        self.group(doc, stream);
    }

    fn drawn(&self) -> bool {
        true
    }
}

impl Inventory {
    /// The space an image XObject is drawn in, or a form's group's.
    fn xobject(&mut self, doc: &CosDocument, dict: &Dict) {
        let subtype = doc.resolve_key(dict, Name::SUBTYPE);
        let Some(subtype) = subtype.as_name().and_then(|name| doc.name_bytes(name)) else {
            return;
        };
        match subtype.as_ref() {
            b"Image" => {
                // 8.9.6.2: a stencil mask has no colour space; its samples
                // say where the fill colour paints.
                let stencil = doc
                    .resolve_key(dict, doc.intern(b"ImageMask"))
                    .as_bool()
                    .unwrap_or(false);
                if !stencil {
                    if let Some(space) = dict.get(Name::COLOR_SPACE) {
                        self.space(doc, space, 0);
                    }
                }
            }
            b"Form" => self.group(doc, dict),
            _ => {}
        }
    }

    /// A form's transparency group's space (11.6.6): `/Group /CS`, where
    /// `/Group /S` is `/Transparency`. 11.6.6 keeps a group's space to device
    /// and CIE-based ones, so this names a spot only where a file breaks that.
    fn group(&mut self, doc: &CosDocument, form: &Dict) {
        let group = doc.resolve_key(form, doc.intern(b"Group"));
        let Some(group) = group.as_dict() else {
            return;
        };
        let kind = doc.resolve_key(group, doc.intern(b"S"));
        let transparency = kind
            .as_name()
            .and_then(|name| doc.name_bytes(name))
            .is_some_and(|name| name.as_ref() == b"Transparency");
        if !transparency {
            return;
        }
        if let Some(space) = group.get(doc.intern(b"CS")) {
            self.space(doc, space, 0);
        }
    }

    /// A shading dictionary's space (8.7.4.3).
    fn shading(&mut self, doc: &CosDocument, shading: &Dict) {
        if let Some(space) = shading.get(Name::COLOR_SPACE) {
            self.space(doc, space, 0);
        }
    }

    /// Lists each spot `space` names, and the spots of the spaces it holds.
    fn space(&mut self, doc: &CosDocument, space: &Object, depth: u32) {
        if depth > MAX_SPACE_DEPTH || self.full() {
            return;
        }
        if let Some(reference) = space.as_objref() {
            if !self.spaces.insert(reference) {
                return;
            }
        }
        let resolved = doc.resolve(space);
        // A name is a device or CIE-based family, or `/Pattern`: none of them
        // names a colorant.
        let Some(items) = resolved.as_array() else {
            return;
        };
        let Some(family) = items
            .first()
            .and_then(Object::as_name)
            .and_then(|name| doc.name_bytes(name))
        else {
            return;
        };
        match family.as_ref() {
            // 8.6.6.3 and 8.7.3.2: the base an index or an uncoloured
            // pattern's components are in.
            b"Indexed" | b"I" | b"Pattern" => {
                if let Some(base) = items.get(1) {
                    self.space(doc, base, depth + 1);
                }
            }
            b"Separation" => {
                if let Some(name) = separation_name(doc, items) {
                    self.name(name);
                }
            }
            b"DeviceN" => {
                let attributes = device_n_attributes(doc, items);
                let process = |name: &[u8]| {
                    attributes
                        .as_ref()
                        .is_some_and(|attributes| attributes.is_process_component(name))
                };
                for name in device_n_names(doc, items) {
                    if !process(&name) {
                        self.name(name);
                    }
                }
                // 8.6.6.5: `/Colorants` describes each colorant alone, and may
                // name colorants this space does not use; a definition of a
                // process component is ignored.
                let keys: Vec<Vec<u8>> = attributes
                    .as_ref()
                    .map(|attributes| {
                        attributes
                            .colorants
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for name in keys {
                    if !process(&name) {
                        self.name(name);
                    }
                }
            }
            _ => {}
        }
    }

    /// Lists `name` if it is a spot not yet listed.
    fn name(&mut self, name: Vec<u8>) {
        if self.full()
            || PROCESS_COLORANTS.contains(&name.as_slice())
            || matches!(name.as_slice(), b"All" | b"None")
        {
            return;
        }
        if self.names.insert(name.clone()) {
            self.spots.push(SpotColorant { name });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page document whose page carries `resources` and whose object
    /// numbers 6 upward are `extra`.
    fn page(extra: &str, resources: &str) -> crate::Page {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
/Resources << {resources} >> >>\nendobj\n\
{extra}\
trailer\n<< /Size 60 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        doc.page(0).expect("a page")
    }

    fn names(page: &crate::Page) -> Vec<String> {
        page.spot_colorants()
            .into_iter()
            .map(|spot| String::from_utf8_lossy(&spot.name).into_owned())
            .collect()
    }

    /// 8.6.6.4 and 8.6.6.5: the four reserved process names, `/All` and
    /// `/None` are not spots, wherever they stand — a `/Separation` of each,
    /// and every one of them inside a `/DeviceN` — and a spot both spaces
    /// name is listed once.
    #[test]
    fn process_names_all_and_none_are_not_spots() {
        let mut resources = String::from("/ColorSpace << ");
        for name in ["Cyan", "Magenta", "Yellow", "Black", "All", "None", "Gold"] {
            resources.push_str(&format!(
                "/S{name} [/Separation /{name} /DeviceGray << /FunctionType 2 /Domain [0 1] /N 1 >>] "
            ));
        }
        resources.push_str(
            "/N1 [/DeviceN [/Cyan /Gold /None /All /Black /Silver] /DeviceCMYK \
             << /FunctionType 2 /Domain [0 1] /N 1 >>] >>",
        );
        assert_eq!(names(&page("", &resources)), vec!["Gold", "Silver"]);
    }

    /// Table 70 and 71: an `NChannel` space's `/Process` names its process
    /// components, which are not spots; a plain `/DeviceN`'s `/Process` is
    /// not read (8.6.6.5: it supports what it did before PDF 1.6), so the
    /// same names there are spots. `/Colorants` keys follow the names array,
    /// a process component's among them passed over.
    #[test]
    fn an_n_channel_process_component_is_not_a_spot() {
        let process = "/Process << /ColorSpace /DeviceRGB \
                       /Components [/ProcessRed /ProcessGreen /ProcessBlue] >>";
        let n_channel = format!(
            "/NC [/DeviceN [/ProcessRed /ProcessGreen /ProcessBlue /Red] /DeviceRGB \
             << /FunctionType 2 /Domain [0 1] /N 1 >> \
             << /Subtype /NChannel {process} \
             /Colorants << /Red [/Separation /Red /DeviceRGB << /FunctionType 2 /Domain [0 1] /N 1 >>] \
             /ProcessRed [/Separation /ProcessRed /DeviceRGB << /FunctionType 2 /Domain [0 1] /N 1 >>] \
             /Orange [/Separation /Orange /DeviceRGB << /FunctionType 2 /Domain [0 1] /N 1 >>] >> >>]"
        );
        let plain = format!(
            "/DN [/DeviceN [/ProcessRed /Lilac] /DeviceRGB \
             << /FunctionType 2 /Domain [0 1] /N 1 >> << {process} >>]"
        );
        assert_eq!(
            names(&page("", &format!("/ColorSpace << {n_channel} >>"))),
            vec!["Red", "Orange"]
        );
        assert_eq!(
            names(&page("", &format!("/ColorSpace << {plain} >>"))),
            vec!["ProcessRed", "Lilac"]
        );
    }

    /// A name is its bytes with `#` escapes decoded (7.3.5), so a name
    /// written two ways is one colorant.
    #[test]
    fn a_name_is_its_decoded_bytes() {
        let resources = "/ColorSpace << \
            /A [/Separation /PANTONE#20131 /DeviceGray << /FunctionType 2 /Domain [0 1] /N 1 >>] \
            /B [/DeviceN [/PANTONE#20131 /Teal] /DeviceGray << /FunctionType 2 /Domain [0 1] /N 1 >>] >>";
        let spots = page("", resources).spot_colorants();
        assert_eq!(spots.len(), 2);
        assert_eq!(spots[0].name, b"PANTONE 131");
        assert_eq!(spots[1].name, b"Teal");
    }

    /// Spaces that name no colorant list nothing, and malformed ones —
    /// a `/Separation` with no name, a `/DeviceN` whose names are no array,
    /// a family that is not a name — are passed over without a panic
    /// (ruling 1).
    #[test]
    fn spaces_with_no_colorants_list_nothing() {
        let resources = "/ColorSpace << /G /DeviceGray /R [/CalRGB << >>] \
            /I [/ICCBased 6 0 R] /L [/Indexed /DeviceRGB 1 <000000FFFFFF>] /P /Pattern \
            /S1 [/Separation] /S2 [/Separation 7] /D1 [/DeviceN /Gold /DeviceGray 1] \
            /D2 [/DeviceN [1 2 3]] /X [5 /Separation /Gold] /E [] /F 42 >>";
        let extra = "6 0 obj\n<< /N 3 /Length 0 >>\nstream\n\nendstream\nendobj\n";
        assert!(names(&page(extra, resources)).is_empty());
    }

    /// An indexed space over an indexed space over a `/Separation`, deeper
    /// than any reader follows, ends where the readers end.
    #[test]
    fn a_space_nested_past_its_bound_ends_where_the_readers_do() {
        let mut space = String::from(
            "[/Separation /Deep /DeviceGray << /FunctionType 2 /Domain [0 1] /N 1 >>]",
        );
        for _ in 0..=MAX_SPACE_DEPTH {
            space = format!("[/Indexed {space} 0 <00>]");
        }
        assert!(names(&page("", &format!("/ColorSpace << /X {space} >>"))).is_empty());

        let mut space = String::from(
            "[/Separation /Shallow /DeviceGray << /FunctionType 2 /Domain [0 1] /N 1 >>]",
        );
        for _ in 0..MAX_SPACE_DEPTH {
            space = format!("[/Indexed {space} 0 <00>]");
        }
        assert_eq!(
            names(&page("", &format!("/ColorSpace << /X {space} >>"))),
            vec!["Shallow"]
        );
    }

    /// A space that holds itself as its base ends at its first repeat.
    #[test]
    fn a_space_holding_itself_ends() {
        let extra = "6 0 obj\n[/Indexed 6 0 R 1 <0001>]\nendobj\n";
        assert!(names(&page(extra, "/ColorSpace << /X 6 0 R >>")).is_empty());
    }

    /// Forms nested past the walk's depth bound: the scopes it reaches, the
    /// page's and one for each level to the bound, list their spots, and
    /// none deeper does.
    #[test]
    fn the_walk_lists_the_scopes_to_its_depth_bound() {
        let depth = fontlist::MAX_RESOURCE_DEPTH as usize;
        let mut extra = String::new();
        for level in 1..=depth + 4 {
            extra.push_str(&format!(
                "{} 0 obj\n<< /Subtype /Form /BBox [0 0 1 1] /Resources << /ColorSpace << \
                 /C [/Separation /L{level} /DeviceGray << /FunctionType 2 /Domain [0 1] /N 1 >>] \
                 >> /XObject << /X {} 0 R >> >> /Length 0 >>\nstream\n\nendstream\nendobj\n",
                level + 5,
                level + 6
            ));
        }
        let page = page(
            &extra,
            "/ColorSpace << /C [/Separation /L0 /DeviceGray \
             << /FunctionType 2 /Domain [0 1] /N 1 >>] >> /XObject << /X 6 0 R >>",
        );
        let expected: Vec<String> = (0..=depth).map(|level| format!("L{level}")).collect();
        assert_eq!(names(&page), expected);
    }

    /// An inventory of `count` spots, `Spot0` onward.
    fn inventory(count: usize) -> Vec<SpotColorant> {
        (0..count)
            .map(|i| SpotColorant {
                name: format!("Spot{i}").into_bytes(),
            })
            .collect()
    }

    /// The plan's plates, by name and use.
    fn channels(planned: &Planned) -> Vec<(String, PlateUse)> {
        planned
            .plan
            .channels()
            .map(|(name, behaviour)| (String::from_utf8_lossy(name).into_owned(), behaviour))
            .collect()
    }

    /// **The plan is the process plates, then each spot asking for a plate
    /// in inventory order**: every spot is a plate by default; one asked to
    /// be composite has none and is neither named nor kept; a disabled one
    /// keeps its plate, which comes back marked; a process colorant asked to
    /// be composite is a plate still, and one asked to be disabled is marked;
    /// a name's first entry decides, and a name the inventory lacks adds
    /// nothing.
    #[test]
    fn the_plan_lays_out_the_inventory_by_the_callers_uses() {
        let asked: Vec<(Vec<u8>, PlateUse)> = vec![
            (b"Spot1".to_vec(), PlateUse::Composite),
            (b"Spot2".to_vec(), PlateUse::Disabled),
            (b"Spot2".to_vec(), PlateUse::Spot),
            (b"Cyan".to_vec(), PlateUse::Composite),
            (b"Black".to_vec(), PlateUse::Disabled),
            (b"Elsewhere".to_vec(), PlateUse::Spot),
        ];
        let planned = plan(&inventory(4), &asked, 100);
        assert_eq!((planned.named, planned.kept), (3, 3));
        assert_eq!(
            channels(&planned),
            [
                ("Cyan".to_string(), PlateUse::Spot),
                ("Magenta".to_string(), PlateUse::Spot),
                ("Yellow".to_string(), PlateUse::Spot),
                ("Black".to_string(), PlateUse::Disabled),
                ("Spot0".to_string(), PlateUse::Spot),
                ("Spot2".to_string(), PlateUse::Disabled),
                ("Spot3".to_string(), PlateUse::Spot),
            ]
        );
        assert_eq!(
            planned.plan.channel(b"Spot1"),
            None,
            "a composite spot has no plate"
        );
        assert_eq!(planned.plan.channel(b"Elsewhere"), None);
    }

    /// **The caps keep the spots that fit, in inventory order**: past
    /// sixty-four spot plates, or past the bytes a page's plates may hold at
    /// its size, the spots past the cap are drawn as composites, the plan
    /// saying how many asked for a plate and how many kept one; a composite
    /// spot counts toward neither.
    #[test]
    fn the_caps_keep_the_spots_that_fit() {
        let many = inventory(tinker_pdf_raster::MAX_SPOT_PLATES + 6);
        let planned = plan(&many, &[], 100);
        assert_eq!(
            (planned.named, planned.kept),
            (
                tinker_pdf_raster::MAX_SPOT_PLATES + 6,
                tinker_pdf_raster::MAX_SPOT_PLATES
            )
        );
        assert_eq!(
            planned.plan.spots().count(),
            tinker_pdf_raster::MAX_SPOT_PLATES
        );
        let last = format!("Spot{}", tinker_pdf_raster::MAX_SPOT_PLATES - 1);
        assert!(planned.plan.channel(last.as_bytes()).is_some());
        let past = format!("Spot{}", tinker_pdf_raster::MAX_SPOT_PLATES);
        assert_eq!(planned.plan.channel(past.as_bytes()), None);

        // A page at the pixel cap keeps sixteen bytes a pixel: eleven spots.
        let planned = plan(&inventory(20), &[], tinker_pdf_render::MAX_PAGE_PIXELS);
        assert_eq!((planned.named, planned.kept), (20, 11));
        assert!(planned.plan.channel(b"Spot10").is_some());
        assert_eq!(planned.plan.channel(b"Spot11"), None);

        // A composite spot asks for nothing, so the cap reaches past it.
        let asked = vec![(b"Spot0".to_vec(), PlateUse::Composite)];
        let planned = plan(&inventory(20), &asked, tinker_pdf_render::MAX_PAGE_PIXELS);
        assert_eq!((planned.named, planned.kept), (19, 11));
        assert!(planned.plan.channel(b"Spot11").is_some());
        assert_eq!(planned.plan.channel(b"Spot12"), None);

        // Within both caps, nothing is left.
        let planned = plan(&inventory(5), &[], 2480 * 3508);
        assert_eq!((planned.named, planned.kept), (5, 5));
    }

    /// The listing stops at its bound, with what it had read.
    #[test]
    fn the_listing_stops_at_its_bound() {
        let mut names_array = String::new();
        for i in 0..MAX_LISTED_SPOTS + 10 {
            names_array.push_str(&format!("/S{i} "));
        }
        let resources = format!(
            "/ColorSpace << /D [/DeviceN [{names_array}] /DeviceGray \
             << /FunctionType 2 /Domain [0 1] /N 1 >>] >>"
        );
        let spots = page("", &resources).spot_colorants();
        assert_eq!(spots.len(), MAX_LISTED_SPOTS);
        assert_eq!(spots[0].name, b"S0");
        assert_eq!(
            spots[MAX_LISTED_SPOTS - 1].name,
            format!("S{}", MAX_LISTED_SPOTS - 1).into_bytes()
        );
    }
}
