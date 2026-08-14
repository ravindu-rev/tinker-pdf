//! Transparency groups and soft masks (clause 11).
//!
//! Everything here is about a failure mode that renders *plausibly*. A group
//! that fades element by element rather than as a unit is a picture with a
//! seam in it; a backdrop counted twice is a picture that is merely darker; a
//! luminosity mask defaulted the wrong way is a drop shadow that appears where
//! the light should be. None of those looks like a bug, which is why each one
//! here is asserted as a *number* against a control render that differs in one
//! key rather than as "it drew something".

use tinker_pdf::{Document, RenderOptions};

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// A pixel, by its position in *user* space on a 60x60 page.
///
/// The bitmap is whatever the default scale makes of sixty points, and device
/// space counts y downward from the top. Naming points and converting here is
/// the only way these tests stay readable next to the content streams they
/// assert about.
fn at(bitmap: &tinker_pdf::Bitmap, ux: f64, uy: f64) -> (u8, u8, u8) {
    let x = ((ux / 60.0) * f64::from(bitmap.width)) as u32;
    let y = (((60.0 - uy) / 60.0) * f64::from(bitmap.height)) as u32;
    let x = x.min(bitmap.width - 1);
    let y = y.min(bitmap.height - 1);
    let base = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(base..base + 3).unwrap_or(&[255, 255, 255]);
    (p[0], p[1], p[2])
}

/// A 60x60 page that invokes one form XObject.
///
/// `group` is the form's `/Group` entry, or empty for a plain form; `gs` is
/// the `/ExtGState` dictionary the page sets before the `Do`; `extra` is any
/// further objects the fixture needs, and `resources` any further resource
/// sub-dictionaries.
fn page(group: &str, gs: &str, form: &str, extra: &str, resources: &str) -> Vec<u8> {
    let content = "/GS0 gs /Fm0 Do";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /ExtGState << /GS0 << {gs} >> >>\n\
                 {resources} >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {group}\n\
   /Length {} >>\nstream\n{form}\nendstream\nendobj\n\
{extra}\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        form.len()
    )
    .into_bytes()
}

/// Two overlapping opaque black squares. The overlap is what tells
/// per-element alpha from group alpha.
const OVERLAP: &str = "0 0 0 rg 5 5 30 30 re f 25 25 30 30 re f";

const TRANSPARENCY: &str = "/Group << /S /Transparency >>";

// ---------------------------------------------------------------------------
// Milestone 2: a group fades as a unit.
// ---------------------------------------------------------------------------

/// The exit criterion, stated as the difference it makes.
///
/// Two overlapping opaque shapes at `ca 0.5`. Without a group each is faded on
/// its own, so where they cross the second is composited over the first's
/// result and the crossing comes out darker — a visible seam, and a perfectly
/// plausible picture. Inside a transparency group they are composited into a
/// buffer at full strength first and the *result* is faded once, so the
/// crossing is the same grey as everywhere else.
///
/// Both renders are asserted, and the control is the one that makes this test
/// mean anything: it pins that the seam is real and that the fixture can see
/// it.
#[test]
fn a_group_fades_as_a_unit_rather_than_element_by_element() {
    let plain = render(page("", "/ca 0.5", OVERLAP, "", ""));
    let grouped = render(page(TRANSPARENCY, "/ca 0.5", OVERLAP, "", ""));

    // Inside the first square only, inside both, inside the second only.
    let (first, cross, second) = ((15.0, 15.0), (30.0, 30.0), (45.0, 45.0));

    let plain_first = at(&plain, first.0, first.1).0;
    let plain_cross = at(&plain, cross.0, cross.1).0;
    assert!(
        (120..=136).contains(&plain_first),
        "half-strength black over white is mid grey, got {plain_first}"
    );
    assert!(
        plain_cross + 40 < plain_first,
        "without a group the crossing is visibly darker: {plain_cross} against \
         {plain_first} -- if these are equal the fixture cannot see the seam \
         and nothing below is evidence"
    );

    let grouped_first = at(&grouped, first.0, first.1).0;
    let grouped_cross = at(&grouped, cross.0, cross.1).0;
    let grouped_second = at(&grouped, second.0, second.1).0;
    assert_eq!(
        grouped_cross, grouped_first,
        "inside a group the crossing shows no seam"
    );
    assert_eq!(
        grouped_second, grouped_first,
        "and neither does the far side"
    );
    assert!(
        (120..=136).contains(&grouped_cross),
        "and the group as a whole is at half strength, got {grouped_cross}"
    );
}

/// A group with no alpha and no blend mode changes nothing at all.
///
/// The commonest `/Group` in real files is exactly this — a producer marks a
/// form as a transparency group because its own model has one, and the page
/// looks the same either way. If it does not, every such file has moved, and
/// that is the widest possible regression for the narrowest possible reason.
#[test]
fn an_opaque_group_renders_the_same_as_no_group_at_all() {
    let plain = render(page("", "", OVERLAP, "", ""));
    let grouped = render(page(TRANSPARENCY, "", OVERLAP, "", ""));
    assert_eq!(
        plain.data, grouped.data,
        "an opaque, Normal-blended group is a no-op"
    );
}

/// `/Group` without `/S /Transparency` is not a transparency group.
///
/// 8.10.3's reference groups use the same key, and treating one as a
/// transparency group resets the alphas inside it — so a reference XObject at
/// `ca 0.5` would start painting at full strength.
#[test]
fn a_group_of_another_subtype_is_not_a_transparency_group() {
    let reference = "/Group << /S /Reference >>";
    let plain = render(page("", "/ca 0.5", OVERLAP, "", ""));
    let other = render(page(reference, "/ca 0.5", OVERLAP, "", ""));
    assert_eq!(
        plain.data, other.data,
        "only /S /Transparency makes a group"
    );
}

/// The group's blend mode applies to the group's result, and only where the
/// group actually painted.
///
/// This is the "renders darker" trap in its simplest form: a group buffer
/// composited back over its whole bounding box under `Multiply` blends the
/// page with itself everywhere the group drew nothing, and a white page
/// multiplied by itself is still white — which is why the assertion is on a
/// *coloured* page area rather than on the margin.
#[test]
fn a_groups_blend_mode_reaches_only_what_the_group_painted() {
    // A green bar across the page, then a group drawing one black square over
    // part of it under Multiply.
    let form = "0 0 0 rg 5 5 25 25 re f";
    let under = "0.2 0.8 0.4 rg 0 0 60 20 re f\n";
    let bytes = {
        let content = format!("{under}/GS0 gs /Fm0 Do");
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /XObject << /Fm0 5 0 R >>\n\
                 /ExtGState << /GS0 << /BM /Multiply >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {TRANSPARENCY}\n\
   /Length {} >>\nstream\n{form}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
            content.len(),
            form.len()
        )
        .into_bytes()
    };
    let bitmap = render(bytes);

    // Inside the square and over the bar: black multiplied by green is black.
    let inside = at(&bitmap, 15.0, 10.0);
    assert!(
        inside.1 < 40,
        "the group multiplied where it painted: {inside:?}"
    );

    // On the bar, inside the group's bounding box, where the group painted
    // nothing. The bar must be exactly the green it was drawn in. A group
    // whose whole box is composited back blends the bar with itself here and
    // the green drops by about a fifth, which looks like a design choice.
    let untouched = at(&bitmap, 45.0, 10.0);
    assert!(
        untouched.1 > 195 && untouched.0 > 40,
        "and left the rest of the bar alone: {untouched:?}"
    );
}

/// Groups nest, and the inner one's buffer is bounded by the outer one's.
///
/// Both XObjects are named in the *page's* resource dictionary. A form's own
/// `/Resources` are not consulted anywhere in this engine yet — a separate
/// gap, and not one this fixture should be the first to discover.
#[test]
fn groups_nest() {
    let inner = "0 0 0 rg 10 10 20 20 re f";
    let outer = "/Inner Do";
    let content = "/GS0 gs /Fm0 Do";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /XObject << /Fm0 5 0 R /Inner 6 0 R >>\n\
                 /ExtGState << /GS0 << /ca 0.5 >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {TRANSPARENCY}\n\
   /Length {} >>\nstream\n{outer}\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [5 5 35 35] {TRANSPARENCY}\n\
   /Length {} >>\nstream\n{inner}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        outer.len(),
        inner.len()
    )
    .into_bytes();

    let bitmap = render(bytes);
    let inside = at(&bitmap, 20.0, 20.0).0;
    assert!(
        (120..=136).contains(&inside),
        "a square inside two nested groups at ca 0.5 is mid grey, got {inside}"
    );
    // Outside the inner form's own bounding box, and inside the outer one.
    let outside = at(&bitmap, 45.0, 45.0);
    assert_eq!(outside, (255, 255, 255), "and nothing leaked out of either");
}
