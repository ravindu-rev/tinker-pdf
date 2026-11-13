//! Re-rendering only the rectangle an incremental update changed
//! (`docs/design/performance.md`, P-17): [`crate::write::Appended::patch`].
//!
//! A page whose update changed nothing it draws but annotations is the page
//! before the update everywhere outside the rectangle the changed annotations
//! cover, before the edit and after it. That rectangle is rendered through
//! [`RenderOptions::region`] and written over the bitmap of the page before
//! the edit, and ruling 5 makes the region the full render's own pixels — but
//! for the one decision a canvas cannot see past its edge, whether an image
//! joins an image run, which the region is grown around until it sees it
//! ([`tinker_pdf_render::RunReach`]). The rectangle comes from each
//! annotation's `/Rect`, `/BBox` and matrix as the renderer places them
//! ([`annots::drawn`]), never from comparing pixels, so the bitmap is written
//! into and never read for what changed.

use std::collections::BTreeSet;

use tinker_pdf_cos::write::write_object;
use tinker_pdf_cos::{CosDocument, Dict, Name, ObjRef, Object, Rect};
use tinker_pdf_render::{PixelRegion, RunReach};

use crate::annots::{self, Drawn};
use crate::{frame_region, handed_back, Bitmap, Document, Page, RenderOptions, Stats};

/// Why [`crate::write::Appended::patch`] rendered the page whole rather than
/// the rectangle its update changed. Each is a test of the page, the update
/// or the bitmap handed in, and nothing else renders the page whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatchFallback {
    /// The document before the update has no page at the index asked for, or
    /// another page there: the update inserted, deleted or moved pages.
    OtherPage,
    /// The update changed what the page draws besides its annotations: its
    /// dictionary in anything but `/Annots` and `/Parent`, the boxes, turn or
    /// resources it inherits, an object its `/Contents`, `/Group` or
    /// resources reach, or the optional-content configuration its layers are
    /// shown by (8.11.4.3).
    ContentChanged,
    /// The annotations the update left alone are not in the same order on the
    /// page after it, so where two of them overlap the later is another.
    Reordered,
    /// An annotation the update changed, added or removed has an appearance
    /// with no `/BBox`, which is drawn where its own `/Matrix` puts it, with
    /// nothing bounding where.
    Unbounded,
    /// The bitmap handed in is not the whole page at these options: its
    /// width, height, format, premultiplication, stride or bytes are not the
    /// render's, or the options name a region, whose render is the region's
    /// and not the page's.
    Mismatch,
    /// The bitmap handed in carries warnings. A render of a rectangle raises
    /// what the part of the page it draws raises, and the part outside it is
    /// taken as clean.
    PreviousWarned,
}

/// What [`crate::write::Appended::patch`] did to the bitmap it was handed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Patch {
    /// The pixels of the page rendered and written over the bitmap: the
    /// rectangle the update's annotations changed, rounded outward, widened
    /// by a pixel and grown around any image run it cut. `None` when the
    /// update changed no pixel of the page, and when the page was rendered
    /// whole.
    pub region: Option<PixelRegion>,
    /// Why the page was rendered whole, when it was.
    pub fallback: Option<PatchFallback>,
}

/// Re-renders page `index` of `after`, which is `before` reopened over an
/// update that wrote `changed`, over `previous`, `before`'s render of the same
/// page under `options`. `None` when `after` has no page `index`, and then
/// `previous` is as it was.
pub(crate) fn patch(
    before: &Document,
    after: &Document,
    changed: &[u32],
    index: u32,
    previous: &mut Bitmap,
    options: &RenderOptions,
) -> Option<Patch> {
    let page = after.page(index)?;
    let changed: BTreeSet<u32> = changed.iter().copied().collect();
    let result = match plan(before, &page, &changed, previous, options) {
        Err(why) => {
            *previous = page.render(options);
            Patch {
                region: None,
                fallback: Some(why),
            }
        }
        // The update changed no pixel: the page before it is the page after.
        Ok(None) => {
            previous.stats = options.stats.then(Stats::default);
            Patch {
                region: None,
                fallback: None,
            }
        }
        Ok(Some(region)) => render_over(&page, region, previous, options),
    };
    Some(result)
}

/// The rectangle `page` must be rendered over, `None` for none, or why it
/// must be rendered whole.
fn plan(
    before: &Document,
    page: &Page,
    changed: &BTreeSet<u32>,
    previous: &Bitmap,
    options: &RenderOptions,
) -> Result<Option<PixelRegion>, PatchFallback> {
    if options.region.is_some() {
        return Err(PatchFallback::Mismatch);
    }
    let old = before
        .page(page.index())
        .filter(|old| old.inner.reference == page.inner.reference)
        .ok_or(PatchFallback::OtherPage)?;
    if content_changed(&old, page, changed) {
        return Err(PatchFallback::ContentChanged);
    }
    let mut drawn = Vec::new();
    if options.annotations {
        changed_annotations(&old, page, changed, &mut drawn)?;
    }
    fits(page, previous, options)?;
    Ok(rectangle(page, &drawn, options))
}

/// Whether the update changed anything `after` draws but its annotations, the
/// page being the same object on both sides.
fn content_changed(before: &Page, after: &Page, changed: &BTreeSet<u32>) -> bool {
    let (old_doc, new_doc) = (&*before.doc, &*after.doc);
    let (old, new) = (&before.inner, &after.inner);
    // What the page inherits, resolved: the boxes, the turn and the resources.
    if old.media_box != new.media_box
        || old.crop_box != new.crop_box
        || old.rotation != new.rotation
        || written(old_doc, old.resources.as_ref()) != written(new_doc, new.resources.as_ref())
    {
        return true;
    }
    let page = new.reference;
    let (Ok(old_object), Ok(new_object)) = (old_doc.get(page), new_doc.get(page)) else {
        return true;
    };
    let (Some(old_dict), Some(new_dict)) = (old_object.as_dict(), new_object.as_dict()) else {
        return true;
    };
    // A page the update did not write is the same dictionary on both sides.
    if changed.contains(&page.num)
        && entries_drawn(old_doc, old_dict) != entries_drawn(new_doc, new_dict)
    {
        return true;
    }
    // The optional-content configuration every layer the page draws is shown
    // or hidden by (8.11.4.3), which lives in the catalog.
    let (old_layers, new_layers) = (layers(old_doc), layers(new_doc));
    if written_object(old_doc, old_layers.as_ref()) != written_object(new_doc, new_layers.as_ref())
    {
        return true;
    }
    let mut roots: Vec<&Object> = Vec::new();
    for key in [Name::CONTENTS, new_doc.intern(b"Group")] {
        if let Some(value) = new_dict.get(key) {
            roots.push(value);
        }
    }
    let resources = new.resources.clone().map(Object::Dict);
    roots.extend(resources.as_ref());
    roots.extend(new_layers.as_ref());
    reaches(new_doc, &roots, changed, page.num)
}

/// The catalog's `/OCProperties`, as the catalog states it.
fn layers(doc: &CosDocument) -> Option<Object> {
    doc.catalog()?.get(doc.intern(b"OCProperties")).cloned()
}

/// `dict` written out, `None` as nothing: the same bytes for the same
/// dictionary in either document, whatever names each has interned.
fn written(doc: &CosDocument, dict: Option<&Dict>) -> Vec<u8> {
    written_object(doc, dict.map(|dict| Object::Dict(dict.clone())).as_ref())
}

/// `object` written out, `None` as nothing.
fn written_object(doc: &CosDocument, object: Option<&Object>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(object) = object {
        write_object(&mut out, object, doc.names_table());
    }
    out
}

/// A page dictionary's entries, but for `/Annots` and `/Parent`, written out
/// in the order of their keys: what the page draws by, for comparing the page
/// before an update and after it. `/Annots` is compared annotation by
/// annotation, and `/Parent` is the tree, whose inherited attributes are
/// compared resolved.
fn entries_drawn(doc: &CosDocument, dict: &Dict) -> Vec<(Vec<u8>, Vec<u8>)> {
    let skipped = [doc.intern(b"Annots"), Name::PARENT];
    let mut entries: Vec<(Vec<u8>, Vec<u8>)> = dict
        .iter()
        .filter(|(key, _)| !skipped.contains(key))
        .map(|(key, value)| {
            let name = doc.name_bytes(*key).map(|bytes| bytes.to_vec());
            (name.unwrap_or_default(), written_object(doc, Some(value)))
        })
        .collect();
    entries.sort();
    entries
}

/// Every reference `object` holds, through its arrays, dictionaries and a
/// stream's dictionary, onto `out`.
fn references_in(object: &Object, out: &mut Vec<ObjRef>) {
    let mut pending = vec![object];
    while let Some(object) = pending.pop() {
        match object {
            Object::Ref(r) => out.push(*r),
            Object::Array(items) => pending.extend(items.iter()),
            Object::Dict(dict) => pending.extend(dict.iter().map(|(_, value)| value)),
            Object::Stream(stream) => pending.extend(stream.dict.iter().map(|(_, value)| value)),
            _ => {}
        }
    }
}

/// Whether anything `roots` reach in `doc`, by reference, is an object the
/// update wrote — `page`, the page itself, passed over, since what of it is
/// drawn is compared entry by entry.
fn reaches(doc: &CosDocument, roots: &[&Object], changed: &BTreeSet<u32>, page: u32) -> bool {
    let mut pending = Vec::new();
    for root in roots {
        references_in(root, &mut pending);
    }
    let mut seen = BTreeSet::new();
    while let Some(r) = pending.pop() {
        if r.num == page || !seen.insert(r.num) {
            continue;
        }
        if changed.contains(&r.num) {
            return true;
        }
        if let Ok(object) = doc.get(r) {
            references_in(&object, &mut pending);
        }
    }
    false
}

/// One entry of a page's `/Annots`: the annotation's reference, when the
/// array names one, and its dictionary, when it is one.
struct Entry {
    reference: Option<ObjRef>,
    dict: Option<Dict>,
}

/// A page's `/Annots`, entry by entry, and the reference of the array when it
/// is an object of its own.
fn entries(doc: &CosDocument, page: ObjRef) -> (Vec<Entry>, Option<ObjRef>) {
    let Ok(object) = doc.get(page) else {
        return (Vec::new(), None);
    };
    let Some(dict) = object.as_dict() else {
        return (Vec::new(), None);
    };
    let key = doc.intern(b"Annots");
    let list = dict.get(key).and_then(Object::as_objref);
    let array = doc.resolve_key(dict, key);
    let entries = array
        .as_array()
        .unwrap_or_default()
        .iter()
        .map(|item| match item {
            Object::Ref(r) => Entry {
                reference: Some(*r),
                dict: doc.get(*r).ok().and_then(|o| o.as_dict().cloned()),
            },
            Object::Dict(dict) => Entry {
                reference: None,
                dict: Some(dict.clone()),
            },
            _ => Entry {
                reference: None,
                dict: None,
            },
        })
        .collect();
    (entries, list)
}

/// The annotations the update changed, added or removed, each one's drawn
/// rectangle on the side it is drawn on pushed onto `drawn` — or why the page
/// must be rendered whole.
///
/// An annotation the array names by reference changed when the update wrote
/// its dictionary or an object its `/AP` reaches; one written into the array
/// changed when the update could have rewritten the array — the page or the
/// array's own object — or wrote an object its `/AP` reaches. One the array
/// names on one side only was added or removed.
fn changed_annotations(
    before: &Page,
    after: &Page,
    changed: &BTreeSet<u32>,
    drawn: &mut Vec<Rect>,
) -> Result<(), PatchFallback> {
    let (old_doc, new_doc) = (&*before.doc, &*after.doc);
    let page = after.inner.reference.num;
    let (old, old_list) = entries(old_doc, before.inner.reference);
    let (new, new_list) = entries(new_doc, after.inner.reference);
    let relisted = changed.contains(&page)
        || [old_list, new_list]
            .into_iter()
            .flatten()
            .any(|list| changed.contains(&list.num));
    let appearance_changed = |doc: &CosDocument, dict: &Option<Dict>| {
        dict.as_ref()
            .and_then(|dict| dict.get(doc.intern(b"AP")))
            .is_some_and(|ap| reaches(doc, &[ap], changed, page))
    };
    let old_refs: BTreeSet<ObjRef> = old.iter().filter_map(|entry| entry.reference).collect();
    let new_refs: BTreeSet<ObjRef> = new.iter().filter_map(|entry| entry.reference).collect();
    let altered: BTreeSet<ObjRef> = new
        .iter()
        .filter_map(|entry| entry.reference.map(|r| (r, entry)))
        .filter(|(r, entry)| {
            old_refs.contains(r)
                && (changed.contains(&r.num) || appearance_changed(new_doc, &entry.dict))
        })
        .map(|(r, _)| r)
        .collect();

    // The annotations left alone draw in the same order on both pages.
    let kept = |entries: &[Entry], other: &BTreeSet<ObjRef>| -> Vec<ObjRef> {
        entries
            .iter()
            .filter_map(|entry| entry.reference)
            .filter(|r| other.contains(r) && !altered.contains(r))
            .collect()
    };
    if kept(&old[..], &new_refs) != kept(&new[..], &old_refs) {
        return Err(PatchFallback::Reordered);
    }

    // Each changed, added or removed annotation where it is drawn, on the
    // side it is drawn on.
    let sides = [(old_doc, &old, &new_refs), (new_doc, &new, &old_refs)];
    for (doc, entries, other) in sides {
        for entry in entries {
            let differs = match entry.reference {
                Some(r) => !other.contains(&r) || altered.contains(&r),
                None => relisted || appearance_changed(doc, &entry.dict),
            };
            if !differs {
                continue;
            }
            match entry.dict.as_ref().map(|dict| annots::drawn(doc, dict)) {
                Some(Drawn::Within(rect)) => drawn.push(rect),
                Some(Drawn::Unbounded) => return Err(PatchFallback::Unbounded),
                Some(Drawn::Nothing) | None => {}
            }
        }
    }
    Ok(())
}

/// Whether `previous` is the whole page at `options`, as [`Page::render`]
/// hands it back, and carries no warnings.
fn fits(page: &Page, previous: &Bitmap, options: &RenderOptions) -> Result<(), PatchFallback> {
    let (width, height) = page.pixel_size(options);
    let format = handed_back(options);
    let premultiplied = options.premultiplied && format.has_alpha();
    let row = (width as usize).checked_mul(format.components());
    let needed = row.and_then(|row| {
        previous
            .stride
            .checked_mul((height as usize).saturating_sub(1))?
            .checked_add(row)
    });
    let whole = previous.width == width
        && previous.height == height
        && previous.format == format
        && previous.premultiplied == premultiplied
        && row.is_some_and(|row| previous.stride >= row)
        && needed.is_some_and(|needed| previous.data.len() >= needed);
    if !whole {
        return Err(PatchFallback::Mismatch);
    }
    if !previous.warnings.is_empty() {
        return Err(PatchFallback::PreviousWarned);
    }
    Ok(())
}

/// The pixels the rectangles in `drawn` cover on `page` at `options`: each
/// carried through the page's view transform and rounded outward, their hull
/// widened by a pixel on every side — the slack a fill's region keeps, so
/// that a mask bounded by a rectangle changes no pixel — and trimmed to the
/// page. `None` when they cover none.
fn rectangle(page: &Page, drawn: &[Rect], options: &RenderOptions) -> Option<PixelRegion> {
    let (width, height) = page.pixel_size(options);
    let full = PixelRegion::new(0, 0, width, height);
    let applied = page.scales(options).1;
    let mut hull: Option<(u32, u32, u32, u32)> = None;
    for rect in drawn {
        let covered = frame_region(
            page.crop_box(),
            page.rotation(),
            applied,
            (rect.x0, rect.y0, rect.x1, rect.y1),
            full,
        );
        if covered.is_empty() {
            continue;
        }
        let (x0, y0) = (covered.x, covered.y);
        let (x1, y1) = (
            covered.x.saturating_add(covered.width),
            covered.y.saturating_add(covered.height),
        );
        hull = Some(hull.map_or((x0, y0, x1, y1), |(a, b, c, d)| {
            (a.min(x0), b.min(y0), c.max(x1), d.max(y1))
        }));
    }
    let (x0, y0, x1, y1) = hull?;
    let (x0, y0) = (x0.saturating_sub(1), y0.saturating_sub(1));
    let (x1, y1) = (
        x1.saturating_add(1).min(width),
        y1.saturating_add(1).min(height),
    );
    Some(PixelRegion::new(x0, y0, x1 - x0, y1 - y0))
}

/// `region` and `more`, a rectangle of the page's pixels, as one rectangle.
fn grown(region: PixelRegion, more: (i32, i32, u32, u32)) -> PixelRegion {
    let at = |v: i32| u32::try_from(v).unwrap_or(0);
    let (mx, my) = (at(more.0), at(more.1));
    let (mr, mb) = (mx.saturating_add(more.2), my.saturating_add(more.3));
    let x0 = region.x.min(mx);
    let y0 = region.y.min(my);
    let x1 = region.x.saturating_add(region.width).max(mr);
    let y1 = region.y.saturating_add(region.height).max(mb);
    PixelRegion::new(x0, y0, x1 - x0, y1 - y0)
}

/// Renders `region` of `page` and writes it over `previous`, growing it
/// first until the region's render sees every image run the page decides.
///
/// A round that reports a place an image met a run outside the region
/// ([`RunReach::unseen`]) grows the region to hold it and renders again; from
/// the second round on, the region is grown to hold every image a run could
/// hold as well ([`RunReach::cut`]), after which every image meets every run
/// inside it and nothing is reported. Each round reports only what lies
/// outside its region, so the region grows every round, and stops growing at
/// the page.
fn render_over(
    page: &Page,
    mut region: PixelRegion,
    previous: &mut Bitmap,
    options: &RenderOptions,
) -> Patch {
    let mut stats: Option<Stats> = None;
    let mut round = 0u32;
    let bitmap = loop {
        let (bitmap, reach): (Bitmap, RunReach) = page.render_reaching(&RenderOptions {
            region: Some(region),
            ..options.clone()
        });
        if options.stats {
            stats = Some(stats.unwrap_or_default() + bitmap.stats.unwrap_or_default());
        }
        let Some(unseen) = reach.unseen else {
            break bitmap;
        };
        region = grown(region, unseen);
        if round > 0 {
            if let Some(cut) = reach.cut {
                region = grown(region, cut);
            }
        }
        round = round.saturating_add(1);
    };
    let components = previous.format.components();
    let row = (bitmap.width as usize).saturating_mul(components);
    for y in 0..bitmap.height as usize {
        let from = y.saturating_mul(bitmap.stride);
        let to = (region.y as usize)
            .saturating_add(y)
            .saturating_mul(previous.stride)
            .saturating_add((region.x as usize).saturating_mul(components));
        let (Some(source), Some(target)) = (
            bitmap.data.get(from..from.saturating_add(row)),
            previous.data.get_mut(to..to.saturating_add(row)),
        ) else {
            continue;
        };
        target.copy_from_slice(source);
    }
    previous.warnings = bitmap.warnings;
    previous.stats = stats;
    Patch {
        region: Some(region),
        fallback: None,
    }
}
