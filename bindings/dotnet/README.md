# .NET binding

A safe C# wrapper over `tinker-pdf-ffi`'s C ABI. Handle lifetime is the whole
job of it: every native handle lives in a `SafeHandle`, so a document or bitmap
is released exactly once even if an exception unwinds past it. Scope, design
and packaging: [`docs/features/bindings.md`](../../docs/features/bindings.md).

```csharp
using var document = Document.Open(File.ReadAllBytes("file.pdf"));
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
milestone is about. Its `nuget.config` clears the source list before adding the
folder, so a restore that cannot find the local package fails rather than
quietly taking something of the same name from nuget.org.

As in the Python and JavaScript smoke tests, the render is asserted twice —
blank without a face, inked with one — because `testdata/simple-text.pdf`
embeds no font program and this engine bundles no faces.

## Writing, and proving it is the same engine

A third argument turns on the write leg:

```bash
dotnet run --project bindings/dotnet/tests/Smoke -c Release -- \
  testdata/simple-text.pdf C:/Windows/Fonts/arial.ttf testdata/form-fields.pdf
```

Two scripts with every input pinned — fill a form and save incrementally, and
build a document from pages, a font and an image — printing one
`DOTNET-SMOKE: WROTE sha256=<hex>` line each. The same two run against the
facade in Rust, through the wheel and through the npm package, and
`cargo xtask bindings-parity` requires all four to be byte-identical.

Six more `SafeHandle`s carry the write surface — `Editor`, `Checkpoint`,
`Buffer`, `Builder`, `PageBuilder`, `OutlineEntry` — on exactly the pattern
`DocumentHandle` and `BitmapHandle` already follow. Three things about them are
asserted by the smoke rather than left as documentation:

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

## Nothing has been published

`dotnet add package TinkerPdf` does not work and is not meant to yet. The
pipeline exists and has been exercised as a dry run; the facade is not frozen
until 0.1.0 ([`docs/architecture.md`](../../docs/architecture.md)).
