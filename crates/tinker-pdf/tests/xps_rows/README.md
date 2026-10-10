# XPS packages for the rows that left the refusal table, derived

Each `XpsElementDefect` row that closed after tier 4 owes a fixture in the
conservation sweep, and no producer on hand writes any of the features those
rows are about — WPF, the XPS object model and Ghostscript write none of
12.1.5's simulations, no n-channel `ContextColor`, no `ContextColor` gradient
stop and no per-stop alpha. So these packages are **derived** rather than
produced, and they live here rather than in [`../xps`](../xps/README.md)
because that directory's first claim is that nothing in this repository wrote
a byte of anything in it.

Every package is `../xps/wpf-image-and-text.xps` — WPF's one-page package with
an obfuscated ODTTF font — with its **fixed page replaced**. The font, the
picture, the relationships, the content types and the containers are WPF's
bytes; the page markup, and where a row needs one a profile part, are this
repository's.

| File | Bytes | SHA-256 | Row |
| --- | ---: | --- | --- |
| `wpf-style-simulations.xps` | 78 764 | `3a31d48215aea021cb0a5853533656cf0f9821ddaa494bf84d279df6d310426a` | 12.1.5's `StyleSimulations` |
| `wpf-stop-alphas.xps` | 78 925 | `c9ca7d2ddbe3c862c9046da29485ee06c11a34bb210b138d72d62a35b988263f` | 18.3.2's per-stop alpha |
| `wpf-colour-interpolation.xps` | 78 997 | `873e0f42f3fa3610eaa9c5dc95e3a60d1839951392cfa360d5bed43170207684` | 18.3.1.2's `ColorInterpolationMode` |
| `wpf-n-channel.xps` | 78 977 | `95a46000c0d33b0d296e9279a815a4a4a033c2a3440a370cf290232ec5afbcc6` | 15.2.5's n-channel `ContextColor` |
| `wpf-context-stops.xps` | 79 050 | `12905033c02364b96bcbc76625755018cc1712f603221272e970ee7d641069d4` | 18.3.1.2's `ContextColor` gradient stop |

**`wpf-style-simulations.xps`** sets WPF's own run — `"Page one"`,
`Indices=",53"`, in the package's font — four times at a 48-unit em, a hundred
units apart: as designed, `BoldSimulation`, `ItalicSimulation` and
`BoldItalicSimulation`.

**`wpf-stop-alphas.xps`** fills three shapes from the page's resource
dictionary: a linear gradient over `wpf-gradients.xps`'s three colours at
alphas `FF`, `80` and `00`; a radial one fading from opaque to clear; and a
linear one whose stops share the alpha `80`, which is a constant alpha and no
ramp. The census reads the stops' alphas out of the markup and, out of the
document, the `/DeviceGray` ramp of the `/Luminosity` soft mask in force when
each gradient is painted.

**`wpf-colour-interpolation.xps`** states `wpf-gradients.xps`'s three stops
twice, in `SRgbLinearInterpolation` and in `ScRgbLinearInterpolation`, then in
linear light a hard edge (two stops at one offset), a radial gradient whose
stops stop short of both ends, and a ramp that fades its alpha. The census
reads the colour halfway along every interval: out of the markup by 18.3.1.2
— the mean in sRGB, the mean of the linear light re-encoded in scRGB, both
written out from IEC 61966-2-1 — and out of the document by evaluating the
shading's function there.

**`wpf-n-channel.xps`** adds a `6CLR` profile part — the one
`xps_context_colour.rs` builds as `n_channel_lut`, a two-point `mft2` grid
whose answer is arithmetic, written again here by `make-rows.py` — with its
content type and the page's required-resource relationship (M2.10), and
fills four shapes in it, three through `Fill` and one through a keyed
`SolidColorBrush`. The census reads each `ContextColor`'s components out of
the markup and the `scn` operands under the `/DeviceN` space out of the
document.

**`wpf-context-stops.xps`** adds the committed fuzz corpus's
`grey-gamma.icc` — a `GRAY` profile, a gamma of 461/256 over XYZ — and two
gradients in it: one whose both stops are `ContextColor`s, and one with a
`ContextColor` stop beside an sRGB one. The census converts each such stop to
sRGB itself, from ICC.1 (`Y = v^γ`) and IEC 61966-2-1, without the reader's
evaluator, and holds the stops and middles beside it to a byte, because the
engine's conversion answers eight-bit sRGB.

**How they were obtained**, on Linux x86_64 with CPython 3.11.15's `zipfile`,
on 3 October 2026:

```
cd crates/tinker-pdf/tests/xps_rows && python3 make-rows.py
```

`make-rows.py` stamps every item with one fixed time, so a rerun under the
same CPython writes the same bytes; each hash above was measured twice.

**How they are checked.** `xps_conservation.rs` sweeps them with the thirteen
real packages, and each has its row in `../xps/CONSERVATION.tsv`. The census
reads what each row is about out of the markup with its own scanners — for the
simulations, `StyleSimulations` and 12.1.5's S5.6 offset written out from the
clause — and out of the document from the content stream: Table 106's
fill-and-stroke mode at a line width of 2% of the em, and a text matrix whose
second axis leans 20°. `xps_glyphs.rs` renders the simulation fixture and
measures the ink.

**What they do not buy** is a producer's idea of these features: the markup is
this repository's reading of ECMA-388, and a package from a producer that
writes them would be the file that closes that.
