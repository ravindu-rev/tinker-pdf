//! What a colour paints by colorant (8.6.6.4, 8.6.6.5;
//! `docs/design/overprint-separations.md`, "Names on plates"): the answer a
//! page's resources give the interpreter for a flat colour
//! (`FontSource::colorants`), and the one the renderer reads an image's,
//! a shading's and a mesh's colours by on plates — one road for every
//! paint.

use std::sync::Arc;

use tinker_pdf_color::{ColorSpace, Colorant, Colorants, Function};
use tinker_pdf_content::{ColorantKind, ColorantTint, ColorantTints};

/// The four process colorants of a CMYK device, by the names 8.6.6.4 reserves
/// for them, in DeviceCMYK's order.
pub(crate) const PROCESS_NAMES: [&[u8]; 4] = [b"Cyan", b"Magenta", b"Yellow", b"Black"];

/// A tint as a byte, `round(255 × clamp(tint, 0, 1))` as the interpreter
/// makes `k`'s, and 0 for one that is not finite.
#[must_use]
pub fn tint_byte(tint: f64) -> u8 {
    if tint.is_finite() {
        (tint.clamp(0.0, 1.0) * 255.0).round() as u8
    } else {
        0
    }
}

/// `values`' first four as bytes: a DeviceCMYK colour's ink.
fn ink_bytes(values: &[f64]) -> [u8; 4] {
    let at = |i: usize| tint_byte(values.get(i).copied().unwrap_or(0.0));
    [at(0), at(1), at(2), at(3)]
}

/// The process ink `alternate` paints for `values` of its own: a DeviceCMYK
/// alternate's own components (an ink render keeps them, H-135), and any
/// other's light — `light` — turned into ink as an ink canvas turns light,
/// with maximum undercolour removal.
fn process_ink(alternate: &ColorSpace, values: &[f64], light: (u8, u8, u8)) -> [u8; 4] {
    if *alternate == ColorSpace::DeviceCmyk {
        return ink_bytes(values);
    }
    let (c, m, y, k) = tinker_pdf_raster::canvas::rgb_to_cmyk(light.0, light.1, light.2);
    [c, m, y, k]
}

/// Whether a process space is a CMYK one, whose components paint the
/// process colorants themselves (8.6.6.5): DeviceCMYK, or a four-component
/// space — a CMYK `ICCBased` profile, read or approximated.
fn is_cmyk_process(space: &ColorSpace) -> bool {
    matches!(
        space,
        ColorSpace::DeviceCmyk
            | ColorSpace::Icc { components: 4, .. }
            | ColorSpace::Approximated { components: 4 }
    )
}

/// What a colour of `space`, as selected, at `components` paints by colorant
/// (8.6.6.4, 8.6.6.5; `docs/design/overprint-separations.md`, "Names on
/// plates"), `light` the colour as painted; `None` for a space that names no
/// colorant of its own — a device space but DeviceCMYK, a CIE-based or ICC
/// one, a coloured pattern's — and for a separation whose names could not
/// all be read, both painting their light.
///
/// DeviceCMYK is its own four process colorants. An `/Indexed` colour is
/// its palette entry in the base (8.6.6.3), and an uncoloured pattern's
/// components are its underlying space's (8.7.3.2), each answered as that
/// space answers. A `/Separation` or plain `/DeviceN` names its colorants,
/// `/None` discarded, each at its tint, beside the process ink its alternate
/// paints for the whole colour; `/All` is one tint for every colorant, and a
/// space naming only `/None` paints nothing and never reverts. An `NChannel`
/// space is read component by component: a reserved process name, or a
/// component a CMYK `/Process` names, paints its process colorant; a spot
/// carries its own process ink, through its `/Colorants` space or, without
/// one, the space's tint transform at its tint alone; and the components of
/// any other process space convert together through it, those the names
/// leave out at its initial values, to light and the light to ink.
#[must_use]
pub fn colorant_tints(
    space: &ColorSpace,
    components: &[f64],
    light: (u8, u8, u8),
) -> Option<ColorantTints> {
    tints_at(space, components, light, 0)
}

/// [`colorant_tints`] `depth` spaces down an `/Indexed` or `/Pattern`
/// space's bases, which ends past eight.
fn tints_at(
    space: &ColorSpace,
    components: &[f64],
    light: (u8, u8, u8),
    depth: u32,
) -> Option<ColorantTints> {
    if depth > 8 {
        return None;
    }
    match space {
        ColorSpace::DeviceCmyk => Some(ColorantTints::device_cmyk(ink_bytes(components))),
        ColorSpace::Indexed { base, lookup, high } => {
            // 8.6.6.3, as `to_rgb` reads the palette.
            let index = components
                .first()
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, f64::from(*high))
                .round() as usize;
            let n = base.components();
            let start = index.saturating_mul(n);
            let entry: Vec<f64> = (0..n)
                .map(|i| lookup.get(start + i).map_or(0.0, |b| f64::from(*b) / 255.0))
                .collect();
            tints_at(base, &entry, light, depth + 1)
        }
        ColorSpace::Pattern { base: Some(base) } => tints_at(base, components, light, depth + 1),
        ColorSpace::Separation {
            components: count,
            alternate,
            tint,
            colorants,
        } => separation_tints(*count, alternate, tint, colorants, components, light),
        _ => None,
    }
}

/// [`colorant_tints`] for a `/Separation` or `/DeviceN` space.
fn separation_tints(
    count: usize,
    alternate: &ColorSpace,
    tint: &Function,
    colorants: &Colorants,
    components: &[f64],
    light: (u8, u8, u8),
) -> Option<ColorantTints> {
    let names = &colorants.names;
    if names.len() != count {
        return None;
    }
    let at = |i: usize| components.get(i).copied().unwrap_or(0.0);
    let named = |name: &[u8], tint: f64, alone: Option<[u8; 4]>| ColorantTint {
        name: Arc::from(name),
        tint: tint_byte(tint),
        alone,
    };
    let none = || ColorantTints {
        kind: ColorantKind::None,
        tints: Vec::new(),
        alternate: None,
        process: None,
    };
    match names.as_slice() {
        // 8.6.6.4: every colorant of the device at the one tint.
        [Colorant::All] => {
            return Some(ColorantTints {
                kind: ColorantKind::All,
                tints: vec![named(b"All", at(0), None)],
                alternate: None,
                process: None,
            })
        }
        // 8.6.6.4 and 8.6.6.5: nothing, and never the alternate.
        _ if names.iter().all(|name| *name == Colorant::None) => return Some(none()),
        _ => {}
    }
    if !colorants.n_channel {
        let tints = names
            .iter()
            .enumerate()
            .filter_map(|(i, name)| match name {
                Colorant::Named(name) => Some(named(name, at(i), None)),
                Colorant::All | Colorant::None => None,
            })
            .collect();
        return Some(ColorantTints {
            kind: ColorantKind::Colorants,
            tints,
            alternate: Some(process_ink(alternate, &tint.eval(components), light)),
            process: None,
        });
    }

    // NChannel: each component by itself (8.6.6.5).
    let mut tints = Vec::new();
    let process = colorants.process.as_ref();
    let cmyk = process.is_none_or(|process| is_cmyk_process(&process.space));
    let mut process_values = match process {
        Some(process) if !cmyk => Some(process.space.initial()),
        _ => None,
    };
    let mut converts = false;
    for (i, name) in names.iter().enumerate() {
        let Colorant::Named(name) = name else {
            continue;
        };
        if let Some(reserved) = PROCESS_NAMES
            .iter()
            .find(|reserved| **reserved == name.as_slice())
        {
            tints.push(named(reserved, at(i), None));
            continue;
        }
        if let Some(index) = colorants.process_component(name) {
            match process_values.as_mut() {
                // A component of another process space, converted with the
                // rest through it.
                Some(values) => {
                    if let Some(slot) = values.get_mut(index) {
                        *slot = at(i);
                        converts = true;
                    }
                }
                // A CMYK process space's component paints its process
                // colorant.
                None => {
                    if let Some(process_name) = PROCESS_NAMES.get(index) {
                        tints.push(named(process_name, at(i), None));
                    }
                }
            }
            continue;
        }
        // A spot: the colorant alone, through its own `/Colorants` space, or
        // the space's tint transform at its tint with every other component
        // at zero.
        let alone = match colorants.separation(name) {
            Some(ColorSpace::Separation {
                alternate: own,
                tint: own_tint,
                ..
            }) => {
                let values = own_tint.eval(&[at(i)]);
                process_ink(own, &values, own.to_rgb(&values))
            }
            _ => {
                let mut only = vec![0.0; count];
                if let Some(slot) = only.get_mut(i) {
                    *slot = at(i);
                }
                let values = tint.eval(&only);
                process_ink(alternate, &values, alternate.to_rgb(&values))
            }
        };
        tints.push(named(name, at(i), Some(alone)));
    }
    let process = match (process, process_values) {
        (Some(process), Some(values)) if converts => {
            let (r, g, b) = process.space.to_rgb(&values);
            let (c, m, y, k) = tinker_pdf_raster::canvas::rgb_to_cmyk(r, g, b);
            Some([c, m, y, k])
        }
        _ => None,
    };
    Some(ColorantTints {
        kind: ColorantKind::NChannel,
        tints,
        alternate: None,
        process,
    })
}
