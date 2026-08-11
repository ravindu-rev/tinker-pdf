//! Blend modes (PDF 32000-1 11.3.5), in integers.
//!
//! A blend mode decides how a source colour combines with the backdrop it
//! lands on. `Normal` ignores the backdrop; every other mode is a function of
//! both, applied per channel for the twelve separable modes and across all
//! three at once for the four non-separable ones.
//!
//! **Everything here is integer arithmetic on 0..=255.** The spec writes these
//! in floating point over 0..=1, and translating them is where the mistakes
//! live — but ruling 4 wants byte-identical output across targets, and
//! `cargo xtask libm` cannot see a float that merely rounds differently. The
//! integers are not an optimisation; they are the determinism.
//!
//! This crate knows no PDF types (ruling 8), so the names here are its own
//! and the mapping from `/BM` lives in the content layer.

/// How a source colour combines with its backdrop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    /// The source replaces the backdrop. The default, and the only mode
    /// that does not look at what is underneath.
    #[default]
    Normal,
    /// Darkens: the product of the two.
    Multiply,
    /// Lightens: the complement of the product of the complements.
    Screen,
    /// `Multiply` or `Screen`, depending on the *backdrop*.
    Overlay,
    /// The darker of the two.
    Darken,
    /// The lighter of the two.
    Lighten,
    /// Brightens the backdrop to reflect the source.
    ColorDodge,
    /// Darkens the backdrop to reflect the source.
    ColorBurn,
    /// `Multiply` or `Screen`, depending on the *source*.
    HardLight,
    /// A softer `HardLight`.
    SoftLight,
    /// The absolute difference.
    Difference,
    /// Like `Difference`, lower contrast.
    Exclusion,
    /// The source's hue, the backdrop's saturation and luminosity.
    Hue,
    /// The source's saturation, the backdrop's hue and luminosity.
    Saturation,
    /// The source's hue and saturation, the backdrop's luminosity.
    Color,
    /// The source's luminosity, the backdrop's hue and saturation.
    Luminosity,
}

/// `a * b / 255`, rounded.
fn mul(a: u32, b: u32) -> u32 {
    let product = a * b + 128;
    (product + (product >> 8)) >> 8
}

impl BlendMode {
    /// Whether this mode needs all three channels at once (11.3.5.3).
    #[must_use]
    pub fn is_nonseparable(self) -> bool {
        matches!(
            self,
            BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity
        )
    }

    /// One channel of a separable mode (11.3.5.2).
    ///
    /// `cb` is the backdrop, `cs` the source, both 0..=255. A non-separable
    /// mode returns the source here and is handled by
    /// [`BlendMode::apply_nonseparable`].
    #[must_use]
    pub fn apply(self, cb: u32, cs: u32) -> u32 {
        let (cb, cs) = (cb.min(255), cs.min(255));
        match self {
            BlendMode::Normal => cs,
            BlendMode::Multiply => mul(cb, cs),
            BlendMode::Screen => cb + cs - mul(cb, cs),
            // 11.3.5.2: Overlay is HardLight with the arguments swapped, and
            // writing it that way is how the two stay consistent.
            BlendMode::Overlay => BlendMode::HardLight.apply(cs, cb),
            BlendMode::Darken => cb.min(cs),
            BlendMode::Lighten => cb.max(cs),
            BlendMode::ColorDodge => {
                if cb == 0 {
                    0
                } else if cs >= 255 {
                    255
                } else {
                    (cb * 255 / (255 - cs)).min(255)
                }
            }
            BlendMode::ColorBurn => {
                if cb >= 255 {
                    255
                } else if cs == 0 {
                    0
                } else {
                    255 - ((255 - cb) * 255 / cs).min(255)
                }
            }
            BlendMode::HardLight => {
                if cs <= 127 {
                    mul(cb, 2 * cs)
                } else {
                    let s = 2 * cs - 255;
                    cb + s - mul(cb, s)
                }
            }
            BlendMode::SoftLight => soft_light(cb, cs),
            BlendMode::Difference => cb.abs_diff(cs),
            BlendMode::Exclusion => cb + cs - 2 * mul(cb, cs),
            _ => cs,
        }
    }

    /// All three channels of a non-separable mode (11.3.5.3).
    #[must_use]
    pub fn apply_nonseparable(self, cb: [u32; 3], cs: [u32; 3]) -> [u32; 3] {
        match self {
            BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
            BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
            BlendMode::Color => set_lum(cs, lum(cb)),
            BlendMode::Luminosity => set_lum(cb, lum(cs)),
            _ => cs,
        }
    }
}

/// 11.3.5.2's `B(cb, cs)` for SoftLight.
fn soft_light(cb: u32, cs: u32) -> u32 {
    // The spec's D(x) has a square root in one branch. Integer Newton is used
    // rather than `f64::sqrt` — which *is* correctly rounded and would be
    // legal — so that this file has no floating point at all and cannot
    // acquire any by accident.
    let d = |x: u32| -> u32 {
        if x <= 63 {
            // ((16x − 12)x + 4)x, with x in 0..=1 scaled to 0..=255.
            let x1 = x;
            let a = (16 * x1).saturating_sub(12 * 255);
            let b = mul(a, x1) + 4 * 255;
            mul(b, x1)
        } else {
            isqrt(x * 255)
        }
    };

    if cs <= 127 {
        let s = 255 - 2 * cs;
        cb.saturating_sub(mul(mul(s, cb), 255 - cb))
    } else {
        let s = 2 * cs - 255;
        cb + mul(s, d(cb).saturating_sub(cb))
    }
}

/// Integer square root, so this module needs no floating point.
fn isqrt(value: u32) -> u32 {
    if value == 0 {
        return 0;
    }
    let mut guess = value;
    let mut next = guess.div_ceil(2);
    while next < guess {
        guess = next;
        next = (guess + value / guess) / 2;
    }
    guess
}

/// 11.3.5.3 `Lum`.
///
/// The coefficients are the spec's own 0.3 / 0.59 / 0.11, not Rec.601's
/// 0.299 / 0.587 / 0.114. They differ in the third digit, which is invisible
/// on any one pixel and is the difference between matching a reference
/// renderer and not.
fn lum(c: [u32; 3]) -> u32 {
    (300 * c[0] + 590 * c[1] + 110 * c[2] + 500) / 1000
}

/// 11.3.5.3 `ClipColor`, which pulls an out-of-range colour back without
/// changing its luminosity.
fn clip_color(mut c: [i32; 3]) -> [u32; 3] {
    let l = (300 * c[0] + 590 * c[1] + 110 * c[2] + 500) / 1000;
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);

    if n < 0 && l != n {
        for v in &mut c {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 255 && x != l {
        for v in &mut c {
            *v = l + (*v - l) * (255 - l) / (x - l);
        }
    }
    [
        c[0].clamp(0, 255) as u32,
        c[1].clamp(0, 255) as u32,
        c[2].clamp(0, 255) as u32,
    ]
}

/// 11.3.5.3 `SetLum`.
fn set_lum(c: [u32; 3], l: u32) -> [u32; 3] {
    let d = l as i32 - lum(c) as i32;
    clip_color([c[0] as i32 + d, c[1] as i32 + d, c[2] as i32 + d])
}

/// 11.3.5.3 `Sat`: the spread between the largest and smallest channel.
fn sat(c: [u32; 3]) -> u32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// 11.3.5.3 `SetSat`.
fn set_sat(c: [u32; 3], s: u32) -> [u32; 3] {
    // The spec names the channels by rank rather than by index, so the
    // indices are recovered first and written back at the end.
    let mut order = [0usize, 1, 2];
    order.sort_by_key(|&i| c[i]);
    let (min, mid, max) = (order[0], order[1], order[2]);

    let mut out = [0u32; 3];
    if c[max] > c[min] {
        out[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        out[max] = s;
    }
    // `out[min]` stays zero, which is what the spec sets it to.
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Normal` must be exactly the source, or every existing golden moves.
    #[test]
    fn normal_ignores_the_backdrop() {
        for cb in [0u32, 1, 127, 254, 255] {
            for cs in [0u32, 1, 127, 254, 255] {
                assert_eq!(BlendMode::Normal.apply(cb, cs), cs);
            }
        }
    }

    /// The endpoints every separable mode has to honour: blending with white
    /// or black is where a mistranslated formula shows up as a visible
    /// inversion rather than as a subtle shift.
    #[test]
    fn the_separable_modes_hold_at_their_endpoints() {
        use BlendMode::*;
        assert_eq!(Multiply.apply(255, 128), 128, "white backdrop is neutral");
        assert_eq!(Multiply.apply(0, 200), 0, "black backdrop absorbs");
        assert_eq!(Screen.apply(0, 128), 128, "black backdrop is neutral");
        assert_eq!(Screen.apply(255, 40), 255, "white backdrop saturates");

        assert_eq!(Darken.apply(60, 200), 60);
        assert_eq!(Lighten.apply(60, 200), 200);
        assert_eq!(Difference.apply(200, 60), 140);
        assert_eq!(Difference.apply(60, 200), 140, "and it is symmetric");
        assert_eq!(Exclusion.apply(255, 255), 0);
        assert_eq!(Exclusion.apply(0, 0), 0);

        assert_eq!(ColorDodge.apply(0, 200), 0, "a black backdrop stays black");
        assert_eq!(ColorDodge.apply(100, 255), 255, "a white source blows out");
        assert_eq!(ColorBurn.apply(255, 100), 255, "a white backdrop stays");
        assert_eq!(ColorBurn.apply(100, 0), 0, "a black source burns to black");
    }

    /// 11.3.5.2 defines Overlay as HardLight with the arguments swapped. If
    /// the two ever disagree, one of them has been rewritten by hand.
    #[test]
    fn overlay_is_hardlight_with_its_arguments_swapped() {
        for cb in (0..=255).step_by(5) {
            for cs in (0..=255).step_by(5) {
                assert_eq!(
                    BlendMode::Overlay.apply(cb, cs),
                    BlendMode::HardLight.apply(cs, cb),
                    "at backdrop {cb}, source {cs}"
                );
            }
        }
    }

    /// Every mode must stay inside the representable range for every input.
    /// An overflow here is a wrapped channel, which looks like a completely
    /// different colour rather than a slightly wrong one.
    #[test]
    fn no_mode_leaves_the_range() {
        let modes = [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
        ];
        for mode in modes {
            for cb in 0..=255u32 {
                for cs in 0..=255u32 {
                    let out = mode.apply(cb, cs);
                    assert!(out <= 255, "{mode:?} at ({cb}, {cs}) gave {out}");
                }
            }
        }
    }

    /// A grey source blended into a grey backdrop with SoftLight must stay
    /// grey and must not invert — the branch boundary at 127 is where the
    /// spec's two cases meet and where an off-by-one shows as a seam.
    #[test]
    fn soft_light_is_continuous_across_its_branch() {
        let below = BlendMode::SoftLight.apply(128, 127);
        let above = BlendMode::SoftLight.apply(128, 128);
        assert!(
            below.abs_diff(above) <= 2,
            "no seam at the branch: {below} then {above}"
        );
        assert_eq!(
            BlendMode::SoftLight.apply(128, 127),
            128,
            "a mid source leaves the backdrop alone"
        );
    }

    #[test]
    fn luminosity_uses_the_specs_coefficients_not_rec_601() {
        // Pure blue: 0.11 rather than 0.114.
        assert_eq!(lum([0, 0, 255]), 28);
        assert_eq!(lum([255, 255, 255]), 255);
        assert_eq!(lum([0, 0, 0]), 0);
    }

    /// The four non-separable modes each take some attributes from the source
    /// and the rest from the backdrop. Taking the wrong ones produces a
    /// plausible image in the wrong colours.
    #[test]
    fn the_non_separable_modes_take_what_they_should() {
        let backdrop = [200u32, 100, 50];
        let source = [50u32, 100, 200];

        let luminosity = BlendMode::Luminosity.apply_nonseparable(backdrop, source);
        assert_eq!(
            lum(luminosity),
            lum(source),
            "Luminosity takes the source's brightness"
        );

        let color = BlendMode::Color.apply_nonseparable(backdrop, source);
        assert_eq!(
            lum(color),
            lum(backdrop),
            "Color keeps the backdrop's brightness"
        );

        let saturation = BlendMode::Saturation.apply_nonseparable(backdrop, source);
        assert_eq!(
            lum(saturation),
            lum(backdrop),
            "Saturation keeps it as well"
        );
    }

    #[test]
    fn non_separable_modes_stay_in_range() {
        for &cb in &[[0u32, 0, 0], [255, 255, 255], [200, 30, 90], [1, 254, 128]] {
            for &cs in &[
                [0u32, 0, 0],
                [255, 255, 255],
                [10, 240, 60],
                [128, 128, 128],
            ] {
                for mode in [
                    BlendMode::Hue,
                    BlendMode::Saturation,
                    BlendMode::Color,
                    BlendMode::Luminosity,
                ] {
                    let out = mode.apply_nonseparable(cb, cs);
                    assert!(
                        out.iter().all(|v| *v <= 255),
                        "{mode:?} on {cb:?} / {cs:?} gave {out:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_integer_square_root_is_the_square_root() {
        for value in [0u32, 1, 2, 3, 4, 100, 255, 65_025, 1_000_000] {
            let root = isqrt(value);
            assert!(root * root <= value, "{root}^2 <= {value}");
            assert!(
                (root + 1) * (root + 1) > value,
                "and ({root}+1)^2 > {value}"
            );
        }
    }
}
