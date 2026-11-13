# .NET binding

A safe C# wrapper over `tinker-pdf-ffi`'s C ABI. Handle lifetime is the whole
job of it: every native handle lives in a `SafeHandle`, so a document or bitmap
is released exactly once even if an exception unwinds past it. Scope, design
and packaging: [`docs/features/bindings.md`](../../docs/features/bindings.md).

```csharp
// Read on demand where an open of the file's bytes would answer the same, and
// read whole otherwise; document.IsStreamed says which.
using var document = Document.OpenFile("file.pdf");
Console.WriteLine(document.PageText(0));

// The engine bundles no font faces and reads no font directories, so a
// document that embeds none extracts its text perfectly and draws none of it.
document.SetFonts(File.ReadAllBytes(@"C:\Windows\Fonts\arial.ttf"));

using var bitmap = document.Render(0, scale: 2.0);
ReadOnlySpan<byte> pixels = bitmap.Pixels;   // zero-copy, valid while alive

// Signatures, read (12.8). There is no "is it valid": reading a signature is
// four independent questions and any one can hold while another does not.
using var signatures = document.ReadSignatures();
using var anchors = new TrustAnchors();      // empty -> Chain.NoAnchors, honestly
using var verdicts = document.VerifySignatures(anchors);
for (uint i = 0; i < signatures.Count; i++)
{
    Console.WriteLine($"{signatures.FieldName(i)}: {signatures.CoverageOf(i)}, " +
                      $"{verdicts.DocumentDigestOf(i)}, {verdicts.SignatureCheckOf(i)}, " +
                      $"{verdicts.ChainOf(i)}");
}
```

The P/Invoke declarations in `TinkerPdf.cs` are written out rather than
generated, so the binding builds with nothing but the .NET SDK. csbindgen is a
build-time convenience, not a requirement; either way this layer adds no logic
of its own (ruling 11).

## Per-RID natives, which is the part that can be wrong

A NuGet package carries platform binaries as `runtimes/<rid>/native/`, and the
.NET host picks one at run time. Building the package is two steps:

```bash
cargo build --release -p tinker-pdf-ffi
cargo run -p xtask -- nuget-stage      # -> runtimes/<this machine's rid>/native/
dotnet pack bindings/dotnet/TinkerPdf.csproj -c Release -o target/nuget
```

`nuget-stage` reads the host OS and architecture and maps them to a RID —
`win-x64`, `linux-x64`, `osx-arm64` and the three arm/x64 counterparts — and to
the platform's library name. That mapping is in `xtask` with a unit test rather
than in three `cp` lines, because it is the thing that fails silently: a
package built with the wrong RID restores, compiles, and throws
`DllNotFoundException` the first time anybody calls it.

A single machine can build only its own RID. The full package is assembled by
[`.github/workflows/release.yml`](../../.github/workflows/release.yml), which
builds the cdylib on three runners and gathers them before packing — and then
greps the `.nupkg` for all three, because `dotnet pack` on an empty
`runtimes/` produces a perfectly valid managed-only package.

## Proving an installed package works

```bash
dotnet run --project bindings/dotnet/tests/Smoke -c Release -- \
  testdata/simple-text.pdf C:/Windows/Fonts/arial.ttf
```

The smoke project takes a `PackageReference` on `TinkerPdf` from a local
folder feed — **not** a `ProjectReference`. A project reference resolves the
managed assembly and finds the cdylib wherever cargo left it, so it passes on
a package that carries no native library at all, which is the one failure this
smoke exists to catch. Its `nuget.config` clears the source list before adding the
folder, so a restore that cannot find the local package fails rather than
quietly taking something of the same name from nuget.org.

As in the Python and JavaScript smoke tests, the render is asserted twice —
blank without a face, inked with one — because `testdata/simple-text.pdf`
embeds no font program and this engine bundles no faces. Then, as
`wheel_smoke.py` does, it holds `OpenFile` to `Open(bytes)`: a generated
four-megabyte file is streamed and reads the same, and a damaged file and a
linearized one updated since are read whole and warn what their bytes warn.

## Writing, and proving it is the same engine

A third argument turns on the write leg:

```bash
dotnet run --project bindings/dotnet/tests/Smoke -c Release -- \
  testdata/simple-text.pdf C:/Windows/Fonts/arial.ttf testdata/form-fields.pdf
```

Scripts with every input pinned — those that write a document (fill a
form and save incrementally; build a document from pages, a font and an image;
the document operations; a sanitise; two saves with every write option set;
form fields; the builder's graphics resources; tagged writing) and those that
write down what a sanitise reported, what the read surface and the signature
verdicts say, and what form data says, all listed in
[docs/features/bindings.md](../../docs/features/bindings.md) —
print one
`DOTNET-SMOKE: WROTE sha256=<hex>` or `READ sha256=<hex>` line each. The same scripts run against the
facade in Rust, through the wheel, the npm package and the Go, Ruby and Java
bindings, and `cargo xtask bindings-parity` requires every one to be
byte-identical.

The write surface's native handles — `Editor`, `Checkpoint`,
`Buffer`, `Builder`, `PageBuilder`, `OutlineEntry` — are `SafeHandle`s on
exactly the pattern `DocumentHandle` and `BitmapHandle` follow. Three things
about them are asserted by the smoke rather than left as documentation:

- **The editor outlives the document.** The engine's editor holds its own
  reference to the shared object store, so the smoke disposes the `Document`
  *before* using the editor. That is why `EditorHandle` needs no keep-alive on
  its parent.
- **A consuming call is not a double free.** `Finish`, `PushPage` and
  `AddChild` consume in the engine, so the native handle boxes an option and
  takes it; calling one twice is `PdfException` with
  `Status.SpentHandle` and a message naming the call that spent it, and
  `Dispose` stays required and safe either way.
- **Finalizer-only teardown.** One document is built with no `using` at all and
  collected under two GC passes, because the forgot-to-dispose path is the one
  a caller actually takes and the one nothing else exercises.

`PdfException` carries a `Status`. Folding it into the message would leave you
comparing strings to make the one distinction that matters:
`Status.NoSuchField` means your form changed and `Status.ValueRefused` means
your data is wrong, and those are different bugs in different places.

## The command stream

`Document.CommandStream(index)` hands back a page's drawing as versioned bytes
([`docs/design/command-stream.md`](../../docs/design/command-stream.md)), and
`CommandStream.Parse` reads them: `Records()` is every record in order, each
its `CommandKind`, its number on the wire and its `CommandField`s, a tag beside
a value — a `ulong`, a `long`, a `double`, a `byte[]`, a `double[]`, a
`ulong[]` or a list of `byte[]` — and `OutlineAt` and `ImageAt` read the two
tables a host draws glyphs and images from. `CommandEncoder` writes records
back, `ToBytes` hands over the bytes, and `Render` replays the stream through
the engine.

`Replay(device)` hands each record, in the stream's order, to the method of an
`ICommandDevice` named for its kind (`GlyphRun`, `FillPath`, `BeginGroup`; a
kind the build does not know goes to `Unknown`), and every method does nothing
until the device implements it. A replay to `System.Drawing.Graphics` is a
device over that projection, written by the host and not shipped — GDI+ runs
on Windows only, through the `System.Drawing.Common` package — and here it
fills, strokes and clips. A command's state is its field 0, a reference to the
`State` records before it in their order; its path is its operators (a byte a
segment: 0 a move, 1 a line, 2 a curve, 3 a close) and its numbers, in the
page's space with `y` up, the transformation already applied. A state's ctm is
its field 0, its fill and stroke alpha its fields 4 and 5, its fill and stroke
colour its fields 6 and 7, and its line width its field 15.

```csharp
using System.Drawing;
using System.Drawing.Drawing2D;
using TinkerPdf;

using var document = Document.Open(File.ReadAllBytes("page.pdf"));
using var stream = CommandStream.Parse(document.CommandStream(0));
var (width, height) = stream.Size;
const float scale = 2;
using var bitmap = new System.Drawing.Bitmap(
    (int)Math.Ceiling(width * scale), (int)Math.Ceiling(height * scale));
using (var graphics = Graphics.FromImage(bitmap))
{
    graphics.Clear(Color.White);
    graphics.SmoothingMode = SmoothingMode.AntiAlias;
    graphics.Transform = new Matrix(scale, 0, 0, -scale, 0, (float)(height * scale));
    stream.Replay(new GdiDevice(graphics));
}
bitmap.Save("page.png");

sealed class GdiDevice : ICommandDevice
{
    private readonly Graphics _graphics;
    private readonly List<CommandRecord> _states = new();
    private readonly Stack<GraphicsState> _saved = new();

    public GdiDevice(Graphics graphics) => _graphics = graphics;

    private CommandRecord? StateOf(CommandRecord record) =>
        record.Fields[0].Value is ulong state ? _states[(int)state] : null;

    private static Color ColourOf(CommandRecord state, int colour, int alpha)
    {
        var rgb = (byte[])state.Fields[colour].Value!;
        var opacity = (double)state.Fields[alpha].Value!;
        return Color.FromArgb((int)Math.Round(opacity * 255), rgb[0], rgb[1], rgb[2]);
    }

    private static GraphicsPath PathOf(CommandRecord record, int operators, bool evenOdd)
    {
        var segments = (byte[])record.Fields[operators].Value!;
        var n = (double[])record.Fields[operators + 1].Value!;
        PointF At(int i) => new((float)n[i], (float)n[i + 1]);
        var path = new GraphicsPath(evenOdd ? FillMode.Alternate : FillMode.Winding);
        PointF start = default, current = default;
        var next = 0;
        foreach (var segment in segments)
        {
            switch (segment)
            {
                case 0:
                    path.StartFigure();
                    start = current = At(next);
                    next += 2;
                    break;
                case 1:
                    path.AddLine(current, At(next));
                    current = At(next);
                    next += 2;
                    break;
                case 2:
                    path.AddBezier(current, At(next), At(next + 2), At(next + 4));
                    current = At(next + 4);
                    next += 6;
                    break;
                default:
                    path.CloseFigure();
                    current = start;
                    break;
            }
        }
        return path;
    }

    public void State(CommandRecord record) => _states.Add(record);

    public void SaveState(CommandRecord record) => _saved.Push(_graphics.Save());

    public void RestoreState(CommandRecord record) => _graphics.Restore(_saved.Pop());

    public void FillPath(CommandRecord record)
    {
        if (StateOf(record) is { } state)
        {
            using var brush = new SolidBrush(ColourOf(state, 6, 4));
            using var path = PathOf(record, 2, record.Fields[1].Value is 1UL);
            _graphics.FillPath(brush, path);
        }
    }

    public void StrokePath(CommandRecord record)
    {
        if (StateOf(record) is { } state)
        {
            var ctm = (double[])state.Fields[0].Value!;
            var lineWidth = (double)state.Fields[15].Value!
                * Math.Sqrt(Math.Abs(ctm[0] * ctm[3] - ctm[1] * ctm[2]));
            using var pen = new Pen(ColourOf(state, 7, 5), (float)lineWidth);
            using var path = PathOf(record, 1, false);
            _graphics.DrawPath(pen, path);
        }
    }

    public void ClipPath(CommandRecord record)
    {
        using var path = PathOf(record, 2, record.Fields[1].Value is 1UL);
        _graphics.SetClip(path, CombineMode.Intersect);
    }
}
```

A glyph run's outlines are the stream's `Outline` records (`OutlineAt`), an
image's samples its `ImageDecode` records (`ImageAt`), a shading's its
`ShadingDefinition`; a device that draws them holds the stream and reads those
as it reads the paths above. The smoke program decodes the stream of a page
holding every kind of record, writes it back to the facade's bytes, and
drives a device that hears every kind in the stream's order.

## Nothing is published

`dotnet add package TinkerPdf` does not work: nothing is published.
[`release.yml`](../../.github/workflows/release.yml) pushes the package to
NuGet when the owner merges `develop` into `main` at a workspace version that
has no `v<version>` tag, and a merge that leaves the version as it is
publishes nothing; the facade does not freeze before 0.1.0
([`docs/architecture.md`](../../docs/architecture.md)).
