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

// ---------------------------------------------------------------------------
// Milestone 3: isolation, and backdrop removal.
// ---------------------------------------------------------------------------

/// A page painting `under` first and then invoking a grouped form.
fn over_backdrop(group: &str, under: &str, form: &str, gs: &str) -> Vec<u8> {
    let content = format!("{under}\n/Fm0 Do");
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /XObject << /Fm0 5 0 R >> /ExtGState << {gs} >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {group}\n\
   /Length {} >>\nstream\n{form}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        form.len()
    )
    .into_bytes()
}

/// The isolation half of the exit criterion: a non-isolated group's contents
/// see the backdrop, an isolated group's do not.
///
/// 11.4.4 says the distinction is only observable when something inside the
/// group uses a blend mode other than Normal, so that is what the fixture
/// does. The shape multiplies. Against a transparent backdrop `Multiply` has
/// nothing to multiply by and the source passes through (11.3.6); against the
/// page's green it does not.
#[test]
fn an_isolated_group_ignores_its_backdrop_and_a_non_isolated_one_does_not() {
    let under = "0.4 0.8 0.4 rg 0 0 60 60 re f";
    let form = "/Mul gs 0.8 0.4 0.8 rg 10 10 40 40 re f";
    let gs = "/Mul << /BM /Multiply >>";

    let isolated = render(over_backdrop(
        "/Group << /S /Transparency /I true >>",
        under,
        form,
        gs,
    ));
    let joined = render(over_backdrop(
        "/Group << /S /Transparency /I false >>",
        under,
        form,
        gs,
    ));

    let iso = at(&isolated, 30.0, 30.0);
    assert!(
        iso.0 > 195 && iso.1 < 115 && iso.2 > 195,
        "an isolated group multiplies against nothing, so the source survives: {iso:?}"
    );

    let non = at(&joined, 30.0, 30.0);
    assert!(
        non.0 < 100 && non.1 < 100 && non.2 < 100,
        "a non-isolated one multiplies against the page underneath it: {non:?}"
    );

    // Outside the square both are the untouched backdrop, which is what says
    // the difference above is the blend and not the group's extent.
    assert_eq!(at(&isolated, 55.0, 55.0), at(&joined, 55.0, 55.0));
}

/// The backdrop-removal half, and the one that catches the failure the plan
/// calls "darker rather than broken".
///
/// A non-isolated group whose contents blend Normally must render *exactly*
/// as an isolated one does — 11.4.4's own note — and exactly as the same
/// content drawn with no group at all. The group starts from the backdrop, so
/// without 11.4.7.2's removal the backdrop is composited in once and then
/// again underneath, and a half-opaque red over blue arrives at a quarter of
/// the red instead of half.
///
/// The partial alpha inside the group is what makes this visible: at full
/// opacity the removal term is zero and a build with no removal at all passes.
/// That was checked by injection, not assumed.
#[test]
fn a_non_isolated_group_does_not_count_its_backdrop_twice() {
    let under = "0 0 1 rg 0 0 60 60 re f";
    let form = "/Half gs 1 0 0 rg 10 10 40 40 re f";
    let gs = "/Half << /ca 0.5 >>";

    let isolated = render(over_backdrop(
        "/Group << /S /Transparency /I true >>",
        under,
        form,
        gs,
    ));
    let joined = render(over_backdrop(
        "/Group << /S /Transparency /I false >>",
        under,
        form,
        gs,
    ));
    let ungrouped = render(over_backdrop("", under, form, gs));

    let want = at(&ungrouped, 30.0, 30.0);
    assert!(
        (120..=136).contains(&want.0) && (120..=136).contains(&want.2) && want.1 < 8,
        "half-opaque red over blue is halfway between them: {want:?}"
    );
    assert_eq!(
        at(&isolated, 30.0, 30.0),
        want,
        "an isolated group changes nothing under Normal blending"
    );
    let non = at(&joined, 30.0, 30.0);
    assert!(
        non.0.abs_diff(want.0) <= 2 && non.2.abs_diff(want.2) <= 2,
        "and neither does a non-isolated one: {non:?} against {want:?}. \
         A backdrop counted twice pulls this toward the blue -- about \
         (64, 0, 191) -- which is a plausible picture and not a broken one"
    );
}

// ---------------------------------------------------------------------------
// Milestone 4: knockout.
// ---------------------------------------------------------------------------

/// 11.4.5: in a knockout group each element composites against the group's
/// *initial* backdrop, so overlapping elements do not accumulate.
///
/// Two half-opaque black squares that overlap. In an ordinary group the
/// crossing accumulates to three quarters and comes out darker; in a knockout
/// group the second square discards what the first left where they cross, so
/// the crossing is the same grey as either square on its own.
///
/// The far side of the *first* square is asserted too, and it is the assertion
/// that separates knockout from "the second element erased the group": the
/// part of the first square the second does not reach must survive untouched.
#[test]
fn overlapping_elements_in_a_knockout_group_do_not_accumulate() {
    let form = "/Half gs 0 0 0 rg 5 5 30 30 re f 25 25 30 30 re f";
    let gs = "/Half << /ca 0.5 >>";

    let ordinary = render(over_backdrop(
        "/Group << /S /Transparency /I true /K false >>",
        "",
        form,
        gs,
    ));
    let knockout = render(over_backdrop(
        "/Group << /S /Transparency /I true /K true >>",
        "",
        form,
        gs,
    ));

    let (first, cross) = ((15.0, 15.0), (30.0, 30.0));

    let ordinary_first = at(&ordinary, first.0, first.1).0;
    let ordinary_cross = at(&ordinary, cross.0, cross.1).0;
    assert!(
        (120..=136).contains(&ordinary_first),
        "one half-opaque square over white is mid grey, got {ordinary_first}"
    );
    assert!(
        ordinary_cross + 40 < ordinary_first,
        "without knockout the two accumulate where they cross: {ordinary_cross} \
         against {ordinary_first}"
    );

    let knockout_first = at(&knockout, first.0, first.1).0;
    let knockout_cross = at(&knockout, cross.0, cross.1).0;
    assert_eq!(
        knockout_first, ordinary_first,
        "the part of the first square the second never reaches is untouched"
    );
    assert_eq!(
        knockout_cross, knockout_first,
        "and where they cross the second knocked the first out rather than \
         compositing over it"
    );
}

/// A knockout group whose elements do not overlap renders identically to an
/// ordinary one, which is what says the restore is confined to the coverage
/// of the element that triggers it.
#[test]
fn knockout_changes_nothing_where_elements_do_not_overlap() {
    let form = "/Half gs 0 0 0 rg 5 5 20 20 re f 35 35 20 20 re f";
    let gs = "/Half << /ca 0.5 >>";
    let ordinary = render(over_backdrop(
        "/Group << /S /Transparency /K false >>",
        "",
        form,
        gs,
    ));
    let knockout = render(over_backdrop(
        "/Group << /S /Transparency /K true >>",
        "",
        form,
        gs,
    ));
    assert_eq!(ordinary.data, knockout.data);
}

/// A non-isolated knockout group knocks back to its *backdrop*, not to
/// transparency — the two initial states 11.4.5 can start from.
#[test]
fn a_non_isolated_knockout_group_knocks_back_to_the_page() {
    let under = "0 0 1 rg 0 0 60 60 re f";
    let form = "/Half gs 1 0 0 rg 5 5 30 30 re f 25 25 30 30 re f";
    let gs = "/Half << /ca 0.5 >>";
    let bitmap = render(over_backdrop(
        "/Group << /S /Transparency /I false /K true >>",
        under,
        form,
        gs,
    ));

    let single = at(&bitmap, 15.0, 15.0);
    let cross = at(&bitmap, 30.0, 30.0);
    assert!(
        (120..=136).contains(&single.0) && (120..=136).contains(&single.2),
        "half-opaque red over blue: {single:?}"
    );
    assert!(
        cross.0.abs_diff(single.0) <= 3 && cross.2.abs_diff(single.2) <= 3,
        "and the crossing knocked back to the blue rather than to nothing, \
         which would have left the red at a quarter strength over white: \
         {cross:?} against {single:?}"
    );
}

// ---------------------------------------------------------------------------
// Milestone 5: ExtGState /SMask.
// ---------------------------------------------------------------------------

/// A 60x60 page whose content is `content`, with one `/ExtGState` `/GS0`
/// carrying an `/SMask` whose group is `mask_content` inside `mask_bbox`.
///
/// `smask` is the mask dictionary's own entries beyond `/G`.
fn masked(content: &str, smask: &str, mask_bbox: &str, mask_content: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /ExtGState << /GS0 << /SMask << /G 5 0 R {smask} >> >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox {mask_bbox}\n\
   /Group << /S /Transparency /CS /DeviceGray >>\n\
   /Length {} >>\nstream\n{mask_content}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        mask_content.len()
    )
    .into_bytes()
}

/// Red over the whole page, through whatever mask `/GS0` installs.
const RED_PAGE: &str = "/GS0 gs 1 0 0 rg 0 0 60 60 re f";
/// A mask group painting white over the left half of the page and nothing
/// over the right half.
const LEFT_HALF_WHITE: &str = "1 g 0 0 30 60 re f";

/// **The first test to write.** A `/Luminosity` mask with no `/BC` masks
/// *fully* where its group painted nothing.
///
/// 11.6.5.2 composites the mask group against a fully opaque backdrop of the
/// colour `/BC` names, and `/BC` absent is the black of the group's colour
/// space. Black luminosity is zero, so a region the mask group did not reach
/// is **hidden**.
///
/// Defaulting the other way — white, or transparent read as unmasked —
/// inverts every drop shadow in a corpus, and on a light page the result
/// still looks reasonable: the shadow appears where the light should be,
/// which reads as a design choice. So the assertion is that the right half of
/// this page is the paper it started as, not that the left half is red.
#[test]
fn a_luminosity_mask_with_no_backdrop_colour_hides_what_its_group_did_not_reach() {
    let bitmap = render(masked(
        RED_PAGE,
        "/S /Luminosity",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    ));

    let lit = at(&bitmap, 15.0, 30.0);
    assert!(
        lit.0 > 245 && lit.1 < 10,
        "where the mask group painted white the red comes through: {lit:?}"
    );

    let dark = at(&bitmap, 45.0, 30.0);
    assert_eq!(
        dark,
        (255, 255, 255),
        "and where it painted nothing the default /BC black hides the red \
         completely -- if this is red, /BC defaulted to white or the absent \
         mask was read as no mask at all, and every drop shadow in every \
         document is inverted"
    );
}

/// The same page with `/BC [1]`: white, so the region the group never reached
/// is fully *un*masked. The pair is what says the default is a default rather
/// than the only thing implemented.
#[test]
fn a_white_backdrop_colour_leaves_the_rest_of_the_page_unmasked() {
    let bitmap = render(masked(
        RED_PAGE,
        "/S /Luminosity /BC [1]",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    ));

    let left = at(&bitmap, 15.0, 30.0);
    let right = at(&bitmap, 45.0, 30.0);
    assert!(left.0 > 245 && left.1 < 10, "the painted half: {left:?}");
    assert!(
        right.0 > 245 && right.1 < 10,
        "and the unpainted half, which /BC now lights: {right:?}"
    );
}

/// `/BC` applies outside the mask group's **bounding box** as well as inside
/// it, because there is no group out there either — 11.6.5.2 makes the
/// backdrop the whole plane, not the box.
#[test]
fn the_backdrop_colour_reaches_beyond_the_groups_bounding_box() {
    // The group's box is the left half of the page and it fills it with white.
    let dark = render(masked(
        RED_PAGE,
        "/S /Luminosity",
        "[0 0 30 60]",
        "1 g 0 0 30 60 re f",
    ));
    let lit = render(masked(
        RED_PAGE,
        "/S /Luminosity /BC [1]",
        "[0 0 30 60]",
        "1 g 0 0 30 60 re f",
    ));

    assert!(
        at(&dark, 15.0, 30.0).0 > 245,
        "inside the box, both are lit"
    );
    assert!(at(&lit, 15.0, 30.0).0 > 245);
    assert_eq!(
        at(&dark, 45.0, 30.0),
        (255, 255, 255),
        "outside it, black /BC hides"
    );
    let outside = at(&lit, 45.0, 30.0);
    assert!(
        outside.0 > 245 && outside.1 < 10,
        "and white /BC does not: {outside:?}"
    );
}

/// An `/Alpha` mask reads the group's coverage rather than its colour, so a
/// *black* square unmasks exactly as a white one does — which is the whole
/// difference between the two kinds and the reason `/BC` is meaningless here.
#[test]
fn an_alpha_mask_reads_coverage_rather_than_colour() {
    let luminosity = render(masked(
        RED_PAGE,
        "/S /Luminosity",
        "[0 0 60 60]",
        "0 g 0 0 30 60 re f",
    ));
    let alpha = render(masked(
        RED_PAGE,
        "/S /Alpha",
        "[0 0 60 60]",
        "0 g 0 0 30 60 re f",
    ));

    assert_eq!(
        at(&luminosity, 15.0, 30.0),
        (255, 255, 255),
        "a black square has no luminosity, so it masks"
    );
    let covered = at(&alpha, 15.0, 30.0);
    assert!(
        covered.0 > 245 && covered.1 < 10,
        "but it is fully opaque, so under /Alpha it unmasks: {covered:?}"
    );
    assert_eq!(
        at(&alpha, 45.0, 30.0),
        (255, 255, 255),
        "and where nothing was drawn there is no alpha either"
    );
}

/// `/TR` shifts the mask measurably. This one inverts it, which swaps the two
/// halves of the page.
#[test]
fn a_transfer_function_shifts_the_mask() {
    let plain = render(masked(
        RED_PAGE,
        "/S /Luminosity",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    ));
    // 7.10.3: an exponential with C0 = 1, C1 = 0 and N = 1 is `1 - x`.
    let inverted = render(masked(
        RED_PAGE,
        "/S /Luminosity /TR << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    ));

    assert!(at(&plain, 15.0, 30.0).0 > 245 && at(&plain, 15.0, 30.0).1 < 10);
    assert_eq!(at(&plain, 45.0, 30.0), (255, 255, 255));

    assert_eq!(
        at(&inverted, 15.0, 30.0),
        (255, 255, 255),
        "the transfer function turned the lit half dark"
    );
    let now_lit = at(&inverted, 45.0, 30.0);
    assert!(
        now_lit.0 > 245 && now_lit.1 < 10,
        "and the dark half lit, including outside the group where /BC was \
         read through it too: {now_lit:?}"
    );
}

/// A mask made of grey rather than black or white masks *partially*.
#[test]
fn a_grey_mask_group_masks_partially() {
    let bitmap = render(masked(
        "/GS0 gs 0 0 0 rg 0 0 60 60 re f",
        "/S /Luminosity",
        "[0 0 60 60]",
        "0.5 g 0 0 30 60 re f",
    ));
    let half = at(&bitmap, 15.0, 30.0).0;
    assert!(
        (110..=145).contains(&half),
        "mid-grey luminosity is about half coverage, got {half}"
    );
}

/// 11.6.5.2: the mask group is rendered with the transform in force at the
/// `gs`, not with the one in force when the masked content is painted.
///
/// The two documents here differ only in the *order* of `cm` and `gs`. The
/// transform at paint time is identical in both, so an implementation that
/// deferred the mask group to the first paint would render them the same
/// picture. A correct one puts the mask's edge at user x = 30 when the `cm`
/// comes after the `gs`, and at x = 45 when it comes before.
///
/// The failure this catches is a shadow a few points from where it belongs.
/// Nobody reports that as a bug.
#[test]
fn the_mask_group_uses_the_transform_in_force_at_the_gs() {
    let before = masked(
        "1 0 0 1 15 0 cm /GS0 gs 1 0 0 rg 0 0 60 60 re f",
        "/S /Luminosity",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    );
    let after = masked(
        "/GS0 gs 1 0 0 1 15 0 cm 1 0 0 rg 0 0 60 60 re f",
        "/S /Luminosity",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    );

    let before = render(before);
    let after = render(after);
    assert_ne!(
        before.data, after.data,
        "the two orders must not render the same picture -- if they do, the \
         mask is being rendered at paint time, where both have the same CTM"
    );

    // With the `cm` first the mask travelled with it: lit out to x = 45.
    assert!(at(&before, 40.0, 30.0).1 < 10, "lit at 40");
    assert_eq!(at(&before, 50.0, 30.0), (255, 255, 255), "dark at 50");
    // With the `cm` after, the mask stayed where the `gs` put it.
    assert!(at(&after, 25.0, 30.0).1 < 10, "lit at 25");
    assert_eq!(at(&after, 40.0, 30.0), (255, 255, 255), "dark at 40");
}

/// `/SMask /None` takes the mask off again (11.6.5.1), and an `/ExtGState`
/// that says nothing about `/SMask` leaves it alone.
#[test]
fn none_clears_the_mask_and_silence_leaves_it() {
    let content = "/GS0 gs /Off gs 1 0 0 rg 0 0 60 60 re f";
    let quiet = "/GS0 gs /Quiet gs 1 0 0 rg 0 0 60 60 re f";
    let build = |content: &str| {
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /ExtGState << /GS0 << /SMask << /G 5 0 R /S /Luminosity >> >>\n\
                                /Off << /SMask /None >>\n\
                                /Quiet << /ca 1 >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60]\n\
   /Group << /S /Transparency /CS /DeviceGray >>\n\
   /Length {} >>\nstream\n{LEFT_HALF_WHITE}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
            content.len(),
            LEFT_HALF_WHITE.len()
        )
        .into_bytes()
    };

    let cleared = render(build(content));
    let right = at(&cleared, 45.0, 30.0);
    assert!(
        right.0 > 245 && right.1 < 10,
        "/SMask /None removed the mask: {right:?}"
    );

    let kept = render(build(quiet));
    assert_eq!(
        at(&kept, 45.0, 30.0),
        (255, 255, 255),
        "an /ExtGState with no /SMask key at all leaves the mask in force -- \
         absent is not /None"
    );
}

// ---------------------------------------------------------------------------
// Milestone 6: the clip, the blend modes, and `q ... Q`.
// ---------------------------------------------------------------------------

/// The exit criterion. A soft mask set inside `q ... Q` does not survive the
/// `Q`.
///
/// The mask lives on the device rather than in the graphics state, because it
/// is pixels (ruling 8), so nothing about `Q` restores it for free — it has to
/// be stacked beside the clip deliberately. Getting it wrong leaves everything
/// drawn after the `Q` masked by a mask the file stopped asking for, which is
/// content quietly missing from the rest of the page.
#[test]
fn a_soft_mask_set_inside_q_does_not_survive_the_q() {
    // Inside the brackets, a red bar across the top; outside them, a second
    // red bar across the bottom. Only the first should be masked.
    let content = "q /GS0 gs 1 0 0 rg 0 40 60 20 re f Q\n\
                   1 0 0 rg 0 0 60 20 re f";
    let bitmap = render(masked(
        content,
        "/S /Luminosity",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    ));

    let inside_lit = at(&bitmap, 15.0, 50.0);
    assert!(
        inside_lit.0 > 245 && inside_lit.1 < 10,
        "inside the brackets and inside the mask: {inside_lit:?}"
    );
    assert_eq!(
        at(&bitmap, 45.0, 50.0),
        (255, 255, 255),
        "inside the brackets and outside the mask"
    );

    // After the `Q` the mask is gone, so both halves of the lower bar paint.
    let after_left = at(&bitmap, 15.0, 10.0);
    let after_right = at(&bitmap, 45.0, 10.0);
    assert!(
        after_left.0 > 245 && after_left.1 < 10,
        "after the Q, the left: {after_left:?}"
    );
    assert!(
        after_right.0 > 245 && after_right.1 < 10,
        "and the right, which the mask would still be hiding if `Q` did not \
         put it back: {after_right:?}"
    );
}

/// The other half of the exit criterion: a masked group under `Multiply`
/// blends *and* masks.
///
/// The two are independent mechanisms — the blend decides how the group's
/// colour meets the page, the mask decides where — so a build that applied
/// one and dropped the other would look right on half the page. Four samples,
/// one per combination of inside/outside the mask and over/off the bar.
#[test]
fn a_masked_group_under_multiply_blends_and_masks() {
    // A green bar across the lower half; then a grouped white-ish square
    // painted through a left-half mask under Multiply.
    let content = "0.2 0.8 0.4 rg 0 0 60 30 re f\n\
                   /GS0 gs /Fm0 Do";
    let form = "0.5 0.5 0.5 rg 0 0 60 60 re f";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /XObject << /Fm0 6 0 R >>\n\
                 /ExtGState << /GS0 << /BM /Multiply\n\
                                       /SMask << /G 5 0 R /S /Luminosity >> >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60]\n\
   /Group << /S /Transparency /CS /DeviceGray >>\n\
   /Length {} >>\nstream\n{LEFT_HALF_WHITE}\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] {TRANSPARENCY}\n\
   /Length {} >>\nstream\n{form}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        LEFT_HALF_WHITE.len(),
        form.len()
    )
    .into_bytes();
    let bitmap = render(bytes);

    // Masked in, over the green bar: mid grey multiplied by the green.
    let blended = at(&bitmap, 15.0, 15.0);
    assert!(
        blended.1 > 90 && blended.1 < 130 && blended.0 < 60,
        "the group both multiplied and showed through: {blended:?}"
    );
    // Masked out, over the bar: the bar untouched.
    let bar = at(&bitmap, 45.0, 15.0);
    assert!(
        bar.1 > 195 && bar.0 < 80,
        "the mask kept the group off this half entirely: {bar:?}"
    );
    // Masked in, off the bar: mid grey multiplied by white paper is mid grey.
    let paper = at(&bitmap, 15.0, 45.0);
    assert!(
        (120..=136).contains(&paper.0),
        "multiplying against the page leaves the group's own grey: {paper:?}"
    );
    // Masked out, off the bar: nothing at all.
    assert_eq!(at(&bitmap, 45.0, 45.0), (255, 255, 255));
}

/// A clip and a soft mask multiply, rather than one replacing the other.
#[test]
fn a_clip_and_a_soft_mask_both_apply() {
    // The clip keeps the lower half; the mask keeps the left half. Only the
    // lower-left quarter paints.
    let content = "q 0 0 60 30 re W n /GS0 gs 1 0 0 rg 0 0 60 60 re f Q";
    let bitmap = render(masked(
        content,
        "/S /Luminosity",
        "[0 0 60 60]",
        LEFT_HALF_WHITE,
    ));

    let quarter = at(&bitmap, 15.0, 15.0);
    assert!(
        quarter.0 > 245 && quarter.1 < 10,
        "inside both: {quarter:?}"
    );
    assert_eq!(
        at(&bitmap, 45.0, 15.0),
        (255, 255, 255),
        "clip yes, mask no"
    );
    assert_eq!(
        at(&bitmap, 15.0, 45.0),
        (255, 255, 255),
        "mask yes, clip no"
    );
    assert_eq!(at(&bitmap, 45.0, 45.0), (255, 255, 255), "neither");
}

/// A soft mask reaches every kind of paint, not only path fills.
///
/// A mask wired into `fill_path` alone leaves strokes, glyphs, images and
/// shadings unmasked, and a page whose shadow is right and whose logo is
/// opaque reads as a font or an image problem rather than as a mask one.
#[test]
fn a_soft_mask_reaches_strokes_shadings_and_images() {
    // A stroke, an inline image and a shading, all through the same mask.
    let content = "/GS0 gs\n\
                   0 0 1 RG 12 w 0 50 m 60 50 l S\n\
                   q 0 0 60 20 re W n /Sh0 sh Q\n\
                   q 60 0 0 20 0 25 cm BI /W 1 /H 1 /CS /RGB /BPC 8 /F /AHx ID ff0000> EI Q";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /ExtGState << /GS0 << /SMask << /G 5 0 R /S /Luminosity >> >> >>\n\
                 /Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB\n\
                    /Coords [0 0 60 0] /Extend [true true]\n\
                    /Function << /FunctionType 2 /Domain [0 1]\n\
                                 /C0 [0 0.6 0] /C1 [0 0.6 0] /N 1 >> >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60]\n\
   /Group << /S /Transparency /CS /DeviceGray >>\n\
   /Length {} >>\nstream\n{LEFT_HALF_WHITE}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        LEFT_HALF_WHITE.len()
    )
    .into_bytes();
    let bitmap = render(bytes);

    for (label, x, y) in [
        ("stroke", 15.0, 50.0),
        ("shading", 15.0, 10.0),
        ("image", 15.0, 35.0),
    ] {
        let lit = at(&bitmap, x, y);
        assert_ne!(lit, (255, 255, 255), "the {label} drew inside the mask");
    }
    for (label, x, y) in [
        ("stroke", 45.0, 50.0),
        ("shading", 45.0, 10.0),
        ("image", 45.0, 35.0),
    ] {
        assert_eq!(
            at(&bitmap, x, y),
            (255, 255, 255),
            "and the {label} was masked away outside it"
        );
    }
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

/// A soft mask whose `/G` names the page's own content stream.
///
/// Found by the first real fuzzing campaign (gap 24 milestone 5) in
/// forty-seven seconds, as a libFuzzer timeout rather than a panic. The page
/// is 1 851 bytes and 120 by 80 points -- 9 600 pixels -- and it took 19.3
/// seconds to render.
///
/// The mechanism is worth stating, because the nesting cap that was already
/// there does not stop it. `MAX_GROUP_DEPTH` bounds how *deep* groups nest.
/// This page's ExtGState `/SMask` names `/G 4 0 R`, and object 4 is the page
/// content stream itself, which runs three `gs` operators that each open
/// another mask group. So the recursion **branches** rather than descends:
/// depth stays inside sixteen the whole way down while the number of buffers
/// is three to the sixteenth. A depth cap is not a work cap, which is the
/// same lesson `MAX_TILE_WORK` records one layer down for tiling patterns.
///
/// The assertion is the warning rather than a clock. A timing test would fail
/// on a slow machine and pass on a fast one with the budget removed; the
/// warning says the budget was reached, which is the thing that bounds the
/// work.
#[test]
fn a_soft_mask_that_names_its_own_page_is_bounded_and_says_so() {
    let bytes = include_bytes!("../../../fuzz/corpus/render_page/soft-mask-names-its-own-page.pdf");
    let doc = tinker_pdf::Document::open(bytes.to_vec()).expect("it opens, damaged as it is");
    let page = doc.page(0).expect("a page");
    let bitmap = page.render(&tinker_pdf::RenderOptions::default());

    assert!(
        bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::GroupBudgetSpent { .. })),
        "the budget is what stops this page, so it has to say it stopped: {:?}",
        bitmap.warnings
    );

    // And it is still a page rather than a refusal: ruling 2 degrades.
    assert_eq!((bitmap.width, bitmap.height), (120, 80));
    assert!(
        bitmap.data.iter().any(|&b| b != bitmap.data[0]),
        "something was drawn"
    );
}
