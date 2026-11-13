//! The plate plan a render on plates maps each colour's colorants by
//! (`docs/design/overprint-separations.md`, "Separations"): which colorant
//! each channel of a [`PixelFormat::Plates`] canvas is, and what a colour that
//! names colorants puts on each.
//!
//! The canvas knows its channels by index alone (ruling 8); the names are
//! here, and so is 8.6.6.4's and 8.6.6.5's reading of them: the four process
//! names are the process plates, a spot the plan gives a plate lands on it, a
//! colour naming a colorant the plan gives none reaches the process plates
//! through its alternate, `/All` tints every plate and `/None` nothing.
//!
//! [`PixelFormat::Plates`]: tinker_pdf_raster::PixelFormat::Plates

use std::collections::HashMap;
use std::sync::Arc;

use tinker_pdf_color::{ColorSpace, Colorant};
use tinker_pdf_content::{ColorantKind, ColorantTints};
use tinker_pdf_raster::{PlateInk, Spots, MAX_SPOT_PLATES};

/// The process colorants 8.6.6.4 reserves, in their channels' order: cyan 0,
/// magenta 1, yellow 2, black 3.
const PROCESS: [&[u8]; 4] = [b"Cyan", b"Magenta", b"Yellow", b"Black"];

/// What a render on plates does with one colorant: the behaviour the plate
/// plan gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlateUse {
    /// Its own plate, every paint naming it putting its tint there. Every
    /// colorant's default.
    #[default]
    Spot,
    /// No plate of its own: a paint naming it reaches the process plates
    /// through its alternate, as a page without the colorant draws it. Asked
    /// of a process colorant it is read as [`PlateUse::Spot`]: the process
    /// plates are what a composite lands on.
    Composite,
    /// Its own plate while the page is drawn — so it knocks out, and is
    /// knocked out by, every other paint exactly as a spot plate is — and
    /// left out of what is handed back.
    Disabled,
}

/// The plates a render on plates draws (RC-04.1's plate plan): the four
/// process plates, cyan, magenta, yellow and black, then a plate for each
/// spot colorant the plan gives one, in the plan's order — the channels of
/// the [`PixelFormat::Plates`] canvas it draws on, [`PlatePlan::spots`] of
/// them past the process four.
///
/// A spot named [`PlateUse::Spot`] or [`PlateUse::Disabled`] has a plate, at
/// most [`MAX_SPOT_PLATES`] of them; a spot named [`PlateUse::Composite`],
/// one past the cap, and a colorant the plan does not name have none, and a
/// colour naming one reaches the process plates through its alternate. A name
/// the plan gives twice keeps its first use.
///
/// [`PixelFormat::Plates`]: tinker_pdf_raster::PixelFormat::Plates
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlatePlan {
    /// Each spot colorant the plan names, once, in its order, with its use.
    named: Vec<(Arc<[u8]>, PlateUse)>,
    /// The channel each spot with a plate is on, by its name.
    channels: HashMap<Arc<[u8]>, usize>,
    /// The spot channels in order, each its colorant's index in `named`.
    spots: Vec<usize>,
    /// How many spots the plan asked a plate for, those past the cap among
    /// them.
    asked: usize,
    /// Which process plates the plan disables.
    process_disabled: [bool; 4],
}

/// By the colorants the plan names, in order, with their uses, and the process
/// plates it disables: every other field is a function of those two, which
/// [`PlatePlan::new`] derives, so two plans equal in them are equal.
impl std::hash::Hash for PlatePlan {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.named.hash(state);
        self.process_disabled.hash(state);
    }
}

impl PlatePlan {
    /// The plan giving each colorant of `colorants` its use, in the order
    /// given: each spot asking for a plate ([`PlateUse::Spot`] or
    /// [`PlateUse::Disabled`]) the next spot channel, while there are fewer
    /// than [`MAX_SPOT_PLATES`] of them, and drawn as [`PlateUse::Composite`]
    /// past them; a process name its process plate, disabled where it is
    /// asked to be.
    #[must_use]
    pub fn new<N: AsRef<[u8]>>(colorants: impl IntoIterator<Item = (N, PlateUse)>) -> PlatePlan {
        let mut plan = PlatePlan::default();
        for (name, behaviour) in colorants {
            let name = name.as_ref();
            if let Some(process) = process_channel(name) {
                if let Some(slot) = plan.process_disabled.get_mut(process) {
                    *slot = behaviour == PlateUse::Disabled;
                }
                continue;
            }
            if plan.named.iter().any(|(held, _)| **held == *name) {
                continue;
            }
            let name: Arc<[u8]> = Arc::from(name);
            plan.named.push((Arc::clone(&name), behaviour));
            if behaviour == PlateUse::Composite {
                continue;
            }
            plan.asked += 1;
            if plan.spots.len() < MAX_SPOT_PLATES {
                plan.channels.insert(name, 4 + plan.spots.len());
                plan.spots.push(plan.named.len() - 1);
            }
        }
        plan
    }

    /// The bytes the plan holds beside its own fields: each colorant's name
    /// and its entries in the plan's tables, which a cache keeping a plan
    /// counts against its budget.
    #[must_use]
    pub fn held_bytes(&self) -> u64 {
        let named = self.named.iter().fold(0usize, |bytes, (name, _)| {
            bytes
                .saturating_add(name.len())
                .saturating_add(std::mem::size_of::<(Arc<[u8]>, PlateUse)>())
        });
        let tables = self
            .channels
            .len()
            .saturating_mul(std::mem::size_of::<(Arc<[u8]>, usize)>())
            .saturating_add(
                self.spots
                    .len()
                    .saturating_mul(std::mem::size_of::<usize>()),
            );
        u64::try_from(named.saturating_add(tables)).unwrap_or(u64::MAX)
    }

    /// How many spot channels the plan's canvas carries: one a spot with a
    /// plate.
    #[must_use]
    pub fn spots(&self) -> Spots {
        // At most the cap, which `new` holds.
        Spots::new(self.spots.len()).unwrap_or_default()
    }

    /// How many spot colorants the plan asked a plate for, those past
    /// [`MAX_SPOT_PLATES`] among them: more than [`PlatePlan::spots`] exactly
    /// where the cap left some to be drawn as composites.
    #[must_use]
    pub fn spots_asked(&self) -> usize {
        self.asked
    }

    /// The channel the colorant `name` paints: a process name its process
    /// plate, cyan 0 to black 3; a spot with a plate `4 + i`, `i` its place
    /// among the plan's plates; `None` for a colorant with no plate.
    #[must_use]
    pub fn channel(&self, name: &[u8]) -> Option<usize> {
        process_channel(name).or_else(|| self.channels.get(name).copied())
    }

    /// Each channel of the plan's canvas, in order — the four process plates,
    /// then each spot plate — as its colorant's name and its use, which is
    /// [`PlateUse::Spot`] or [`PlateUse::Disabled`]: what a caller hands back,
    /// and what it leaves out.
    pub fn channels(&self) -> impl Iterator<Item = (&[u8], PlateUse)> + '_ {
        let process = PROCESS
            .iter()
            .zip(self.process_disabled)
            .map(|(name, disabled)| {
                let behaviour = if disabled {
                    PlateUse::Disabled
                } else {
                    PlateUse::Spot
                };
                (*name, behaviour)
            });
        let spots = self.spots.iter().filter_map(|&index| {
            self.named
                .get(index)
                .map(|(name, behaviour)| (&name[..], *behaviour))
        });
        process.chain(spots)
    }

    /// What a colour naming `colorants` puts on a canvas of `spots` spot
    /// channels under this plan (`docs/design/overprint-separations.md`,
    /// "Colours on plates"), or `None` where it names nothing the plan can
    /// place and paints its light turned into ink, as a colour naming no
    /// colorant does.
    ///
    /// - DeviceCMYK as the current colour paints its own components on the
    ///   process plates.
    /// - A `/Separation` or a plain `/DeviceN` paints its colorants directly
    ///   when every name it gives is on a plate of the canvas — a process name,
    ///   or a spot with a plate — and otherwise the whole colour's process ink
    ///   through its alternate (8.6.6.4, 8.6.6.5).
    /// - An `NChannel` `/DeviceN` is read component by component (8.6.6.5):
    ///   each colorant on a plate puts its tint there, each other its own
    ///   process ink on the process plates, and the components of another
    ///   process space their ink there too; inks meeting on one plate add,
    ///   held at solid.
    /// - `/All` puts its tint on every plate of the canvas, a spot with no
    ///   plate getting none, since it is not a colorant the canvas has.
    /// - `/None`, and a `/DeviceN` of `/None` alone, paints nothing at all.
    ///
    /// Every channel a paint names no tint for takes none, so under `Normal`
    /// the paint knocks it out, as 8.6.7 has a paint do with overprint off.
    #[must_use]
    pub fn paint(&self, colorants: &ColorantTints, spots: Spots) -> Option<PlatePaint> {
        let width = 4 + spots.count();
        // A channel of this canvas: a plan's plate past it is no plate here.
        let placed = |name: &[u8]| self.channel(name).filter(|&channel| channel < width);
        let ink = |tints: &[u8]| {
            let channels: Vec<(usize, u8)> = tints.iter().copied().enumerate().collect();
            PlateInk::named(spots, &channels)
        };
        match colorants.kind {
            ColorantKind::None => Some(PlatePaint::Nothing),
            ColorantKind::All => {
                let tint = colorants.tints.first().map_or(0, |tint| tint.tint);
                Some(PlatePaint::Ink(ink(&vec![tint; width])))
            }
            ColorantKind::DirectCmyk | ColorantKind::Colorants => {
                let mut tints = vec![0u8; width];
                for colorant in &colorants.tints {
                    match placed(&colorant.name) {
                        Some(channel) => {
                            if let Some(slot) = tints.get_mut(channel) {
                                *slot = colorant.tint;
                            }
                        }
                        // 8.6.6.4, 8.6.6.5: a name the canvas has no plate
                        // for reverts the whole colour to its alternate.
                        None => {
                            let alternate = colorants.alternate?;
                            return Some(PlatePaint::Ink(PlateInk::process(spots, alternate)));
                        }
                    }
                }
                Some(PlatePaint::Ink(ink(&tints)))
            }
            ColorantKind::NChannel => {
                let mut tints = vec![0u8; width];
                let mut add = |channel: usize, tint: u8| {
                    if let Some(slot) = tints.get_mut(channel) {
                        *slot = slot.saturating_add(tint);
                    }
                };
                for colorant in &colorants.tints {
                    match (placed(&colorant.name), colorant.alone) {
                        (Some(channel), _) => add(channel, colorant.tint),
                        // A spot the canvas has no plate for: its own
                        // process ink, through its `/Colorants` space.
                        (None, Some(alone)) => {
                            for (channel, tint) in alone.into_iter().enumerate() {
                                add(channel, tint);
                            }
                        }
                        (None, None) => {}
                    }
                }
                if let Some(process) = colorants.process {
                    for (channel, tint) in process.into_iter().enumerate() {
                        add(channel, tint);
                    }
                }
                Some(PlatePaint::Ink(ink(&tints)))
            }
        }
    }

    /// The tints one sample of an image puts on each channel of a canvas of
    /// `spots` spot channels, into `out`, one a channel
    /// (`docs/design/overprint-separations.md`, "Images on plates"): a sample
    /// naming `colorants` puts what [`PlatePlan::paint`] answers for them,
    /// and one naming none the plan can place — or none at all — its `light`
    /// turned into ink on the process plates (8.6.4.4 inverted with maximum
    /// undercolour removal) and no ink on any spot, as a colour of light
    /// paints. `false` for a sample that paints nothing at all, 8.6.6.4's
    /// `/None`, with `out` left at no ink.
    pub fn sample_tints(
        &self,
        colorants: Option<&ColorantTints>,
        light: (u8, u8, u8),
        spots: Spots,
        out: &mut [u8],
    ) -> bool {
        out.fill(0);
        let tints = match colorants.and_then(|colorants| self.paint(colorants, spots)) {
            Some(PlatePaint::Nothing) => return false,
            Some(PlatePaint::Ink(ink)) => ink,
            None => PlateInk::from_light(
                spots,
                tinker_pdf_raster::Color::rgb(light.0, light.1, light.2),
            ),
        };
        for (slot, tint) in out.iter_mut().zip(tints.tints()) {
            *slot = *tint;
        }
        true
    }
}

/// How the colours of one shading's or mesh's space reach the plates of a
/// render by a plan (`docs/design/overprint-separations.md`, "Shadings on
/// plates"), chosen once a paint: each colour's tints are the ones a flat
/// colour of the same space and components puts on them
/// ([`crate::colorant_tints`], [`PlatePlan::sample_tints`]), read without
/// asking the answer for every pixel where the space allows.
pub(crate) struct SpaceTints {
    /// The plates canvas's spot channels.
    spots: Spots,
    road: Road,
    /// One colour's tints, a channel each, kept between pixels.
    out: Vec<u8>,
}

/// The road a [`SpaceTints`] reads a colour's tints by.
enum Road {
    /// DeviceCMYK: each component onto its process plate.
    Cmyk,
    /// A `/Separation` or plain `/DeviceN` every name of which has a plate:
    /// each component onto its colorant's plate, `/None`'s onto none.
    Direct(Vec<Option<usize>>),
    /// Every other space naming colorants: the colorants' answer and the
    /// plan, colour by colour.
    General(Arc<PlatePlan>),
    /// A space naming only `/None`: no effect on the page (8.6.6.4).
    Nothing,
}

impl SpaceTints {
    /// How colours of `space` reach a plates canvas of `spots` spot channels
    /// by `plan`, or `None` where the space names no colorant and its colours
    /// are their light, which the canvas turns into ink.
    pub(crate) fn of(
        space: &ColorSpace,
        plan: &Arc<PlatePlan>,
        spots: Spots,
    ) -> Option<SpaceTints> {
        let initial = space.initial();
        let tints = crate::colorant_tints(space, &initial, (255, 255, 255))?;
        let width = 4 + spots.count();
        let road = match (tints.kind, space) {
            (ColorantKind::None, _) => Road::Nothing,
            (ColorantKind::DirectCmyk, ColorSpace::DeviceCmyk) => Road::Cmyk,
            (ColorantKind::Colorants, ColorSpace::Separation { colorants, .. }) => {
                let channels: Option<Vec<Option<usize>>> = colorants
                    .names
                    .iter()
                    .map(|name| match name {
                        Colorant::Named(name) => plan
                            .channel(name)
                            .filter(|&channel| channel < width)
                            .map(Some),
                        Colorant::None => Some(None),
                        Colorant::All => None,
                    })
                    .collect();
                channels.map_or_else(|| Road::General(Arc::clone(plan)), Road::Direct)
            }
            _ => Road::General(Arc::clone(plan)),
        };
        Some(SpaceTints {
            spots,
            road,
            out: vec![0; width],
        })
    }

    /// Whether colours of the space paint nothing at all.
    pub(crate) fn paints_nothing(&self) -> bool {
        matches!(self.road, Road::Nothing)
    }

    /// The tints a colour of `space` with `components` and light `light`
    /// puts on each plate, or `None` where it paints nothing.
    pub(crate) fn tints(
        &mut self,
        space: &ColorSpace,
        components: &[f64],
        light: (u8, u8, u8),
    ) -> Option<&[u8]> {
        let at = |i: usize| crate::tint_byte(components.get(i).copied().unwrap_or(0.0));
        match &self.road {
            Road::Nothing => return None,
            Road::Cmyk => {
                self.out.fill(0);
                for (channel, slot) in self.out.iter_mut().enumerate().take(4) {
                    *slot = at(channel);
                }
            }
            Road::Direct(channels) => {
                self.out.fill(0);
                for (component, channel) in channels.iter().enumerate() {
                    if let Some(slot) = channel.and_then(|channel| self.out.get_mut(channel)) {
                        *slot = at(component);
                    }
                }
            }
            Road::General(plan) => {
                let colorants = crate::colorant_tints(space, components, light);
                if !plan.sample_tints(colorants.as_ref(), light, self.spots, &mut self.out) {
                    return None;
                }
            }
        }
        Some(&self.out)
    }
}

/// What a colour puts on a plates canvas ([`PlatePlan::paint`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatePaint {
    /// A tint for each channel, every channel it names none for knocked out.
    Ink(PlateInk),
    /// Nothing at all: 8.6.6.4's `/None`, whose paints have no effect on the
    /// page — no ink, and no channel knocked out.
    Nothing,
}

/// The process ink of a plate ink that puts no tint on any spot: its four
/// process tints, which the ink paths every canvas takes paint as a plate
/// ink paints them. `None` where it tints a spot.
pub(crate) fn spotless(ink: &PlateInk) -> Option<[u8; 4]> {
    let tints = ink.tints();
    if tints.iter().skip(4).any(|tint| *tint != 0) {
        return None;
    }
    let at = |channel: usize| tints.get(channel).copied().unwrap_or(0);
    Some([at(0), at(1), at(2), at(3)])
}

/// The process plate a reserved name paints (8.6.6.4), or `None` for any
/// other name.
fn process_channel(name: &[u8]) -> Option<usize> {
    PROCESS.iter().position(|process| *process == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_content::ColorantTint;

    fn spots(count: usize) -> Spots {
        Spots::new(count).expect("under the cap")
    }

    /// A colour of `kind` naming `named`, each at its tint, its alternate
    /// `alternate` and its other process space's ink `process`.
    fn colour(
        kind: ColorantKind,
        named: &[(&[u8], u8, Option<[u8; 4]>)],
        alternate: Option<[u8; 4]>,
        process: Option<[u8; 4]>,
    ) -> ColorantTints {
        ColorantTints {
            kind,
            tints: named
                .iter()
                .map(|(name, tint, alone)| ColorantTint {
                    name: Arc::from(*name),
                    tint: *tint,
                    alone: *alone,
                })
                .collect(),
            alternate,
            process,
        }
    }

    /// The tints a paint puts on each channel, or `None` for nothing.
    fn tints(plan: &PlatePlan, colorants: &ColorantTints, count: usize) -> Option<Vec<u8>> {
        match plan.paint(colorants, spots(count)) {
            Some(PlatePaint::Ink(ink)) => Some(ink.tints().to_vec()),
            Some(PlatePaint::Nothing) => None,
            None => panic!("the plan places every colour here: {colorants:?}"),
        }
    }

    fn plan() -> PlatePlan {
        PlatePlan::new([
            (&b"Gold"[..], PlateUse::Spot),
            (b"Violet", PlateUse::Composite),
            (b"Silver", PlateUse::Disabled),
            (b"Orange", PlateUse::Spot),
        ])
    }

    /// **The plan lays its plates out in its order**: process plates 0 to
    /// 3, then a channel for each spot with a plate — a disabled one among
    /// them — and none for a composite one or one the plan does not name; a
    /// name given twice keeps its first use, and a process name asked to be
    /// composite is its process plate still.
    #[test]
    fn the_plan_lays_its_plates_out_in_its_order() {
        let plan = PlatePlan::new([
            (&b"Gold"[..], PlateUse::Spot),
            (b"Violet", PlateUse::Composite),
            (b"Silver", PlateUse::Disabled),
            (b"Gold", PlateUse::Composite),
            (b"Black", PlateUse::Composite),
            (b"Cyan", PlateUse::Disabled),
            (b"Orange", PlateUse::Spot),
        ]);
        assert_eq!(plan.spots(), spots(3));
        assert_eq!(plan.spots_asked(), 3);
        for (name, channel) in [
            (&b"Cyan"[..], Some(0)),
            (b"Magenta", Some(1)),
            (b"Yellow", Some(2)),
            (b"Black", Some(3)),
            (b"Gold", Some(4)),
            (b"Silver", Some(5)),
            (b"Orange", Some(6)),
            (b"Violet", None),
            (b"Teal", None),
        ] {
            assert_eq!(
                plan.channel(name),
                channel,
                "{}",
                String::from_utf8_lossy(name)
            );
        }
        let channels: Vec<(Vec<u8>, PlateUse)> = plan
            .channels()
            .map(|(name, behaviour)| (name.to_vec(), behaviour))
            .collect();
        assert_eq!(
            channels,
            [
                (b"Cyan".to_vec(), PlateUse::Disabled),
                (b"Magenta".to_vec(), PlateUse::Spot),
                (b"Yellow".to_vec(), PlateUse::Spot),
                (b"Black".to_vec(), PlateUse::Spot),
                (b"Gold".to_vec(), PlateUse::Spot),
                (b"Silver".to_vec(), PlateUse::Disabled),
                (b"Orange".to_vec(), PlateUse::Spot),
            ]
        );
        assert_eq!(PlatePlan::default().spots(), Spots::NONE);
        assert_eq!(PlatePlan::default().channels().count(), 4);
    }

    /// **Past the cap a spot is drawn as a composite**: a plan asking more
    /// plates than [`MAX_SPOT_PLATES`] keeps the first sixty-four in its
    /// order, and says how many it was asked for.
    #[test]
    fn spots_past_the_cap_have_no_plate() {
        let names: Vec<Vec<u8>> = (0..MAX_SPOT_PLATES + 3)
            .map(|i| format!("Spot{i}").into_bytes())
            .collect();
        let plan = PlatePlan::new(names.iter().map(|name| (name, PlateUse::Spot)));
        assert_eq!(plan.spots().count(), MAX_SPOT_PLATES);
        assert_eq!(plan.spots_asked(), MAX_SPOT_PLATES + 3);
        assert_eq!(plan.channel(b"Spot0"), Some(4));
        assert_eq!(
            plan.channel(format!("Spot{}", MAX_SPOT_PLATES - 1).as_bytes()),
            Some(3 + MAX_SPOT_PLATES)
        );
        assert_eq!(
            plan.channel(format!("Spot{MAX_SPOT_PLATES}").as_bytes()),
            None
        );
    }

    /// **Each kind of colour lands as 8.6.6.4 and 8.6.6.5 read it**: DeviceCMYK
    /// on the process plates; a `/Separation` and a `/DeviceN` on their
    /// plates while every name has one, a process name among them, and the
    /// whole colour through its alternate once one has none; `/All` on every
    /// plate the canvas has; `/None` nothing.
    #[test]
    fn each_kind_of_colour_lands_as_the_clauses_read_it() {
        let plan = plan();
        let cmyk = ColorantTints::device_cmyk([10, 20, 30, 40]);
        assert_eq!(tints(&plan, &cmyk, 3), Some(vec![10, 20, 30, 40, 0, 0, 0]));

        let gold = colour(
            ColorantKind::Colorants,
            &[(b"Gold", 153, None)],
            Some([0, 9, 99, 0]),
            None,
        );
        assert_eq!(tints(&plan, &gold, 3), Some(vec![0, 0, 0, 0, 153, 0, 0]));
        let mixed = colour(
            ColorantKind::Colorants,
            &[(b"Orange", 64, None), (b"Cyan", 200, None)],
            Some([1, 2, 3, 4]),
            None,
        );
        assert_eq!(tints(&plan, &mixed, 3), Some(vec![200, 0, 0, 0, 0, 0, 64]));
        // Disabled has a plate, so a colour naming it is placed.
        let silver = colour(
            ColorantKind::Colorants,
            &[(b"Silver", 77, None)],
            Some([5, 5, 5, 5]),
            None,
        );
        assert_eq!(tints(&plan, &silver, 3), Some(vec![0, 0, 0, 0, 0, 77, 0]));
        // A composite spot, and one the plan does not name, revert the
        // whole colour, the placed names with it.
        for missing in [&b"Violet"[..], b"Teal"] {
            let reverting = colour(
                ColorantKind::Colorants,
                &[(b"Gold", 153, None), (missing, 40, None)],
                Some([12, 0, 240, 3]),
                None,
            );
            assert_eq!(
                tints(&plan, &reverting, 3),
                Some(vec![12, 0, 240, 3, 0, 0, 0]),
                "{}",
                String::from_utf8_lossy(missing)
            );
        }
        // A plate of the plan the canvas does not carry is no plate.
        assert_eq!(tints(&plan, &mixed, 2), Some(vec![1, 2, 3, 4, 0, 0]));
        // With no alternate to revert to, the colour is its light.
        let lost = colour(ColorantKind::Colorants, &[(b"Teal", 40, None)], None, None);
        assert_eq!(plan.paint(&lost, spots(3)), None);

        let all = colour(ColorantKind::All, &[(b"All", 102, None)], None, None);
        assert_eq!(tints(&plan, &all, 3), Some(vec![102; 7]));
        let none = colour(ColorantKind::None, &[], None, None);
        assert_eq!(tints(&plan, &none, 3), None);
    }

    /// **An `NChannel` colour lands component by component**: each colorant
    /// with a plate puts its tint there, each without its own process ink on
    /// the process plates, and another process space's components their ink;
    /// inks meeting on one plate add, held at solid.
    #[test]
    fn an_n_channel_colour_lands_component_by_component() {
        let plan = plan();
        let n_channel = colour(
            ColorantKind::NChannel,
            &[
                (b"Cyan", 100, None),
                (b"Gold", 50, Some([0, 0, 255, 0])),
                (b"Violet", 80, Some([60, 10, 0, 0])),
                (b"Teal", 30, Some([200, 0, 0, 70])),
            ],
            None,
            Some([0, 250, 0, 10]),
        );
        assert_eq!(
            tints(&plan, &n_channel, 3),
            Some(vec![255, 255, 0, 80, 50, 0, 0]),
            "cyan 100 + 60 + 200, magenta 10 + 250, black 70 + 10"
        );
    }

    /// **A spotless ink is its process tints**, and an ink tinting a spot is
    /// not.
    #[test]
    fn a_spotless_ink_is_its_process_tints() {
        assert_eq!(
            spotless(&PlateInk::process(spots(2), [1, 2, 3, 4])),
            Some([1, 2, 3, 4])
        );
        assert_eq!(
            spotless(&PlateInk::named(spots(2), &[(0, 9), (5, 0)])),
            Some([9, 0, 0, 0])
        );
        assert_eq!(spotless(&PlateInk::named(spots(2), &[(5, 1)])), None);
    }

    /// A `/Separation` of the one name in `names`, or a plain `/DeviceN` of
    /// them, `None` and `All` the reserved names, onto DeviceCMYK through the
    /// PostScript `program`.
    fn separation(names: &[&[u8]], program: &[u8]) -> ColorSpace {
        let names: Vec<Colorant> = names
            .iter()
            .map(|name| match *name {
                b"None" => Colorant::None,
                b"All" => Colorant::All,
                name => Colorant::Named(name.to_vec()),
            })
            .collect();
        ColorSpace::Separation {
            components: names.len(),
            alternate: Box::new(ColorSpace::DeviceCmyk),
            tint: Box::new(tinker_pdf_color::Function::PostScript {
                domain: vec![(0.0, 1.0); names.len()],
                range: vec![(0.0, 1.0); 4],
                program: tinker_pdf_color::function::parse_postscript(program),
            }),
            colorants: tinker_pdf_color::Colorants {
                names,
                ..tinker_pdf_color::Colorants::default()
            },
        }
    }

    /// **A space's road paints as a flat colour of it does** (RC-04.13):
    /// DeviceCMYK, a `/Separation` of a spot and of a process colorant, a
    /// `/DeviceN` of spots, a process name and `/None` every name of which
    /// has a plate, `/All`, and a `/DeviceN` naming a spot the plan
    /// composites and one naming a spot the plan does not name — at every
    /// colour of a lattice, the tints a shading's colour of the space takes
    /// are the ones `PlatePlan::sample_tints` puts for the colorants' answer
    /// on the same components; a space of `/None` alone paints nothing, and
    /// one naming no colorant has no road, its colours being their light.
    #[test]
    fn a_space_s_road_paints_as_a_flat_colour_of_it_does() {
        let plan = Arc::new(plan());
        let spots = spots(3);
        let light = (90, 120, 200);
        let spaces = [
            ("DeviceCMYK", ColorSpace::DeviceCmyk),
            ("a spot", separation(&[b"Gold"], b"{ 0 0 0 }")),
            (
                "a process colorant",
                separation(&[b"Magenta"], b"{ 0 exch 0 0 }"),
            ),
            (
                "spots, a process name and /None",
                separation(
                    &[b"Orange", b"Cyan", b"None", b"Silver"],
                    b"{ pop pop pop 0 0 0 }",
                ),
            ),
            ("/All", separation(&[b"All"], b"{ dup dup dup }")),
            (
                "a composited spot",
                separation(&[b"Gold", b"Violet"], b"{ 0 0 }"),
            ),
            (
                "a spot of no plate",
                separation(&[b"Teal", b"Gold"], b"{ 0 0 }"),
            ),
        ];
        let levels = [0.0, 0.3, 0.7, 1.0];
        for (what, space) in &spaces {
            let mut road = SpaceTints::of(space, &plan, spots).expect("a space naming colorants");
            assert!(!road.paints_nothing(), "{what}");
            let count = space.components();
            for index in 0..levels.len().pow(count as u32) {
                let components: Vec<f64> = (0..count)
                    .map(|c| levels[index / levels.len().pow(c as u32) % levels.len()])
                    .collect();
                let mut expected = vec![0; 4 + spots.count()];
                let colorants = crate::colorant_tints(space, &components, light);
                let paints = plan.sample_tints(colorants.as_ref(), light, spots, &mut expected);
                assert_eq!(
                    road.tints(space, &components, light).map(<[u8]>::to_vec),
                    paints.then_some(expected),
                    "{what} at {components:?}"
                );
            }
        }

        let none = separation(&[b"None"], b"{ 0 0 0 }");
        let mut road = SpaceTints::of(&none, &plan, spots).expect("a space naming /None");
        assert!(road.paints_nothing());
        assert_eq!(road.tints(&none, &[0.5], light), None);
        assert!(SpaceTints::of(&ColorSpace::DeviceRgb, &plan, spots).is_none());
    }
}
