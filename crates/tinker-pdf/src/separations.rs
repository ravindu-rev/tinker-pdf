//! A page's spot colorants (8.6.6.4, 8.6.6.5), behind
//! [`Page::spot_colorants`](crate::Page::spot_colorants).
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

use crate::fontlist::{self, ScopeVisitor};
use crate::resources::{device_n_attributes, device_n_names, separation_name};

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
