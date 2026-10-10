// The editor's document operations over the C ABI: page labels, embedded
// files, the outline, the typed /Info setters and the XMP packet, the page
// boundaries, and Sanitise with its report.
//
// Transcribed from crates/tinker-pdf-ffi/include/tinker_pdf.h. Every struct
// that crosses is pointers, lengths and 32-bit integers only, so there is no
// packing to guess at. No logic of its own (ruling 11).

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace TinkerPdf;

/// <summary>How a page-label range writes its number (12.4.2, Table 159).</summary>
public enum LabelStyle
{
    /// <summary><c>/D</c>: 1, 2, 3.</summary>
    Decimal = 0,

    /// <summary><c>/R</c>: I, II, III.</summary>
    RomanUpper = 1,

    /// <summary><c>/r</c>: i, ii, iii.</summary>
    RomanLower = 2,

    /// <summary><c>/A</c>: A, B, ... Z, AA.</summary>
    LettersUpper = 3,

    /// <summary><c>/a</c>: a, b, ... z, aa.</summary>
    LettersLower = 4,

    /// <summary>No number: the prefix alone.</summary>
    None = 5,
}

/// <summary>What a metadata write did to the other statement of the same metadata.</summary>
public enum MetadataSync
{
    /// <summary>The other half does not exist, so nothing can disagree.</summary>
    Alone = 0,

    /// <summary>
    /// The other half exists and was not changed; it may still state the old
    /// value.
    /// </summary>
    OtherHalfUnchanged = 1,
}

/// <summary>One of a page's five boundaries (14.11.2).</summary>
public enum PageBoundary
{
    /// <summary><c>/MediaBox</c>.</summary>
    MediaBox = 0,

    /// <summary><c>/CropBox</c>.</summary>
    CropBox = 1,

    /// <summary><c>/BleedBox</c>.</summary>
    BleedBox = 2,

    /// <summary><c>/TrimBox</c>.</summary>
    TrimBox = 3,

    /// <summary><c>/ArtBox</c>.</summary>
    ArtBox = 4,
}

/// <summary>Why a sanitise removed something.</summary>
public enum Removal
{
    /// <summary>A JavaScript action.</summary>
    JavaScript = 0,

    /// <summary><c>/Names /JavaScript</c>.</summary>
    DocumentJavaScript = 1,

    /// <summary><c>/AcroForm /CO</c>.</summary>
    CalculationOrder = 2,

    /// <summary><c>/AcroForm /XFA</c>.</summary>
    XfaForm = 3,

    /// <summary>An outward-reaching action, whose <c>/S</c> is carried beside it.</summary>
    Action = 4,

    /// <summary><c>/Names /EmbeddedFiles</c>.</summary>
    EmbeddedFileTree = 5,

    /// <summary>A file specification's <c>/EF</c> or <c>/RF</c>, or an embedded file stream.</summary>
    EmbeddedFile = 6,

    /// <summary><c>/Info</c>.</summary>
    Info = 7,

    /// <summary>A <c>/Metadata</c> stream.</summary>
    Metadata = 8,
}

/// <summary>A date (7.9.4). A null offset is an unspecified zone.</summary>
public readonly record struct PdfDate(
    int Year,
    int Month,
    int Day,
    int Hour,
    int Minute,
    int Second,
    int? UtcOffsetMinutes = null)
{
    internal DateRaw ToRaw() => new DateRaw
    {
        Year = Year,
        Month = Month,
        Day = Day,
        Hour = Hour,
        Minute = Minute,
        Second = Second,
        HasUtcOffset = UtcOffsetMinutes.HasValue ? 1 : 0,
        UtcOffsetMinutes = UtcOffsetMinutes ?? 0,
    };
}

/// <summary>One run of page labels (12.4.2). A null prefix writes no <c>/P</c>.</summary>
public readonly record struct PageLabelRange(
    uint FirstPage,
    LabelStyle Style,
    string? Prefix,
    uint Start);

/// <summary>A file to embed (7.11.4).</summary>
public sealed record EmbeddedFile(
    string Name,
    string Filename,
    byte[] Data,
    string? Description = null,
    string? MimeType = null,
    PdfDate? Created = null,
    PdfDate? Modified = null);

/// <summary>What <see cref="Editor.Sanitise"/> takes out.</summary>
/// <remarks>All four false takes out nothing; all four true is the engine's <c>Sanitise::ALL</c>.</remarks>
public readonly record struct SanitiseOptions(
    bool JavaScript,
    bool Actions,
    bool EmbeddedFiles,
    bool Metadata);

/// <summary><c>TpdfDate</c>, field for field: eight 32-bit integers.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct DateRaw
{
    internal int Year;
    internal int Month;
    internal int Day;
    internal int Hour;
    internal int Minute;
    internal int Second;
    internal int HasUtcOffset;
    internal int UtcOffsetMinutes;
}

/// <summary><c>TpdfPageLabelRange</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct PageLabelRangeRaw
{
    internal uint FirstPage;
    internal int Style;
    internal IntPtr Prefix;
    internal uint Start;
}

/// <summary><c>TpdfEmbeddedFile</c>, field for field: pointers and a length.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct EmbeddedFileRaw
{
    internal IntPtr Name;
    internal IntPtr Filename;
    internal IntPtr Description;
    internal IntPtr MimeType;
    internal IntPtr Created;
    internal IntPtr Modified;
    internal IntPtr Data;
    internal nuint DataLen;
}

/// <summary><c>TpdfSanitise</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct SanitiseRaw
{
    internal int JavaScript;
    internal int Actions;
    internal int EmbeddedFiles;
    internal int Metadata;
}

internal static partial class Native
{
    [DllImport(Library)]
    internal static extern int tpdf_editor_set_page_labels(
        IntPtr editor, PageLabelRangeRaw[] ranges, nuint count);

    [DllImport(Library)]
    internal static extern int tpdf_editor_attach_file(
        IntPtr editor, ref EmbeddedFileRaw file, out uint objectNumber, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_outline(
        IntPtr editor, IntPtr[] entries, nuint count);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_info(
        IntPtr editor, int key, byte[] value, out int sync);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_info_date(
        IntPtr editor, int key, ref DateRaw date, out int sync);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_trapped(IntPtr editor, int trapped, out int sync);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_xmp_metadata(
        IntPtr editor, byte[] data, nuint len, out int sync);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_page_boundary(
        IntPtr editor, uint index, int boundary, double x0, double y0, double x1, double y1);

    [DllImport(Library)]
    internal static extern int tpdf_page_boundary(
        IntPtr doc, uint index, int boundary,
        out double x0, out double y0, out double x1, out double y1);

    [DllImport(Library)]
    internal static extern int tpdf_editor_sanitise(
        IntPtr editor, ref SanitiseRaw what, out IntPtr report);

    [DllImport(Library)]
    internal static extern uint tpdf_sanitise_report_count(IntPtr report, int list);

    [DllImport(Library)]
    internal static extern int tpdf_sanitise_report_entry(
        IntPtr report, int list, uint index,
        out int what, out int hasObject, out uint objectNumber, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_sanitise_report_action(
        IntPtr report, int list, uint index, out IntPtr data, out nuint len);

    [DllImport(Library)]
    internal static extern uint tpdf_sanitise_report_path_count(IntPtr report, uint index);

    [DllImport(Library)]
    internal static extern int tpdf_sanitise_report_path_step(
        IntPtr report, uint index, uint step,
        out int isIndex, out ulong position, out IntPtr keyData, out nuint keyLen);

    [DllImport(Library)]
    internal static extern void tpdf_sanitise_report_free(IntPtr report);
}

/// <summary>One entry a sanitise removed from an object that stays, or from the trailer.</summary>
/// <param name="Holder">The object it was removed from, or null for the trailer.</param>
/// <param name="Path">Keys (<c>byte[]</c>) and array positions (<c>ulong</c>, counted in
/// the array as it was) from the holder down to the removed value.</param>
/// <param name="What">Why.</param>
/// <param name="Action">The <c>/S</c> of a <see cref="Removal.Action"/>.</param>
public sealed record RemovedEntry(
    (uint Object, ushort Generation)? Holder,
    IReadOnlyList<object> Path,
    Removal What,
    byte[]? Action);

/// <summary>One object a sanitise deleted, because only removed entries reached it.</summary>
public sealed record DeletedObject(
    (uint Object, ushort Generation) Object,
    Removal What,
    byte[]? Action);

/// <summary>Everything a sanitise took out; together the lists account for every change.</summary>
public sealed record SanitiseReport(
    IReadOnlyList<RemovedEntry> Removed,
    IReadOnlyList<DeletedObject> Deleted);

public sealed partial class Editor
{
    private static int SyncOf(int status, int sync)
    {
        Native.Check(status);
        return sync;
    }

    /// <summary>Sets the page labels (12.4.2), replacing any.</summary>
    /// <remarks>Throws <see cref="Status.EditRefused"/> with the engine's own reason,
    /// writing nothing, when the ranges are refused.</remarks>
    public void SetPageLabels(params PageLabelRange[] ranges)
    {
        ArgumentNullException.ThrowIfNull(ranges);
        var prefixes = new List<GCHandle>();
        try
        {
            var raw = new PageLabelRangeRaw[ranges.Length];
            for (var i = 0; i < ranges.Length; i++)
            {
                var prefix = IntPtr.Zero;
                if (ranges[i].Prefix is { } text)
                {
                    var handle = GCHandle.Alloc(Native.Utf8(text), GCHandleType.Pinned);
                    prefixes.Add(handle);
                    prefix = handle.AddrOfPinnedObject();
                }
                raw[i] = new PageLabelRangeRaw
                {
                    FirstPage = ranges[i].FirstPage,
                    Style = (int)ranges[i].Style,
                    Prefix = prefix,
                    Start = ranges[i].Start,
                };
            }
            Native.Check(Native.tpdf_editor_set_page_labels(Raw, raw, (nuint)raw.Length));
        }
        finally
        {
            foreach (var handle in prefixes)
            {
                handle.Free();
            }
        }
    }

    /// <summary>Embeds a file (7.11.4) and returns its file specification's reference.</summary>
    public (uint Object, ushort Generation) AttachFile(EmbeddedFile file)
    {
        ArgumentNullException.ThrowIfNull(file);
        ArgumentNullException.ThrowIfNull(file.Data);
        var pinned = new List<GCHandle>();
        IntPtr Pin(object value)
        {
            var handle = GCHandle.Alloc(value, GCHandleType.Pinned);
            pinned.Add(handle);
            return handle.AddrOfPinnedObject();
        }
        try
        {
            var created = file.Created?.ToRaw();
            var modified = file.Modified?.ToRaw();
            var raw = new EmbeddedFileRaw
            {
                Name = Pin(Native.Utf8(file.Name)),
                Filename = Pin(Native.Utf8(file.Filename)),
                Description = file.Description is null ? IntPtr.Zero : Pin(Native.Utf8(file.Description)),
                MimeType = file.MimeType is null ? IntPtr.Zero : Pin(Native.Utf8(file.MimeType)),
                Created = created is null ? IntPtr.Zero : Pin(new[] { created.Value }),
                Modified = modified is null ? IntPtr.Zero : Pin(new[] { modified.Value }),
                Data = Pin(file.Data),
                DataLen = (nuint)file.Data.Length,
            };
            Native.Check(Native.tpdf_editor_attach_file(Raw, ref raw, out var number, out var generation));
            return (number, generation);
        }
        finally
        {
            foreach (var handle in pinned)
            {
                handle.Free();
            }
        }
    }

    /// <summary>Replaces the outline (12.3.3), <b>consuming</b> each entry.</summary>
    public void SetOutline(params OutlineEntry[] entries)
    {
        ArgumentNullException.ThrowIfNull(entries);
        var raws = new IntPtr[entries.Length];
        for (var i = 0; i < entries.Length; i++)
        {
            raws[i] = entries[i].Raw;
        }
        Native.Check(Native.tpdf_editor_set_outline(Raw, raws, (nuint)raws.Length));
    }

    /// <summary>
    /// Sets an <c>/Info</c> text entry — the typed setters <c>set_title</c>
    /// to <c>set_producer</c>. The two date keys are <see cref="SetInfoDate"/>'s.
    /// </summary>
    public MetadataSync SetInfo(InfoKey key, string value)
    {
        ArgumentNullException.ThrowIfNull(value);
        var status = Native.tpdf_editor_set_info(Raw, (int)key, Native.Utf8(value), out var sync);
        return (MetadataSync)SyncOf(status, sync);
    }

    /// <summary>Sets <c>/CreationDate</c> or <c>/ModDate</c>.</summary>
    public MetadataSync SetInfoDate(InfoKey key, PdfDate date)
    {
        var raw = date.ToRaw();
        var status = Native.tpdf_editor_set_info_date(Raw, (int)key, ref raw, out var sync);
        return (MetadataSync)SyncOf(status, sync);
    }

    /// <summary>Sets <c>/Info /Trapped</c>. <see cref="Trapped.Absent"/> is refused.</summary>
    public MetadataSync SetTrapped(Trapped trapped)
    {
        var status = Native.tpdf_editor_set_trapped(Raw, (int)trapped, out var sync);
        return (MetadataSync)SyncOf(status, sync);
    }

    /// <summary>Makes <paramref name="packet"/> the XMP metadata (14.3.2), verbatim.</summary>
    public MetadataSync SetXmpMetadata(byte[] packet)
    {
        ArgumentNullException.ThrowIfNull(packet);
        var status = Native.tpdf_editor_set_xmp_metadata(
            Raw, packet, (nuint)packet.Length, out var sync);
        return (MetadataSync)SyncOf(status, sync);
    }

    /// <summary>Sets one of a page's boundaries (14.11.2).</summary>
    public void SetPageBoundary(uint index, PageBoundary boundary, double x0, double y0, double x1, double y1) =>
        Native.Check(Native.tpdf_editor_set_page_boundary(Raw, index, (int)boundary, x0, y0, x1, y1));

    /// <summary>Sets a page's <c>/BleedBox</c>.</summary>
    public void SetBleedBox(uint index, double x0, double y0, double x1, double y1) =>
        SetPageBoundary(index, PageBoundary.BleedBox, x0, y0, x1, y1);

    /// <summary>Sets a page's <c>/TrimBox</c>.</summary>
    public void SetTrimBox(uint index, double x0, double y0, double x1, double y1) =>
        SetPageBoundary(index, PageBoundary.TrimBox, x0, y0, x1, y1);

    /// <summary>Sets a page's <c>/ArtBox</c>.</summary>
    public void SetArtBox(uint index, double x0, double y0, double x1, double y1) =>
        SetPageBoundary(index, PageBoundary.ArtBox, x0, y0, x1, y1);

    /// <summary>Takes out what <paramref name="what"/> names and reports every change.</summary>
    public SanitiseReport Sanitise(SanitiseOptions what)
    {
        var raw = new SanitiseRaw
        {
            JavaScript = what.JavaScript ? 1 : 0,
            Actions = what.Actions ? 1 : 0,
            EmbeddedFiles = what.EmbeddedFiles ? 1 : 0,
            Metadata = what.Metadata ? 1 : 0,
        };
        Native.Check(Native.tpdf_editor_sanitise(Raw, ref raw, out var report));
        try
        {
            var removed = new List<RemovedEntry>();
            var removedCount = Native.tpdf_sanitise_report_count(report, 0);
            for (uint i = 0; i < removedCount; i++)
            {
                Native.Check(Native.tpdf_sanitise_report_entry(
                    report, 0, i, out var kind, out var hasObject, out var number, out var generation));
                var path = new List<object>();
                var steps = Native.tpdf_sanitise_report_path_count(report, i);
                for (uint step = 0; step < steps; step++)
                {
                    Native.Check(Native.tpdf_sanitise_report_path_step(
                        report, i, step, out var isIndex, out var position, out var key, out var keyLen));
                    path.Add(isIndex != 0 ? position : (object)(Native.ReadBytes(key, keyLen) ?? Array.Empty<byte>()));
                }
                Native.Check(Native.tpdf_sanitise_report_action(report, 0, i, out var data, out var len));
                removed.Add(new RemovedEntry(
                    hasObject != 0 ? ((uint, ushort)?)(number, generation) : null,
                    path,
                    (Removal)kind,
                    Native.ReadBytes(data, len)));
            }
            var deleted = new List<DeletedObject>();
            var deletedCount = Native.tpdf_sanitise_report_count(report, 1);
            for (uint i = 0; i < deletedCount; i++)
            {
                Native.Check(Native.tpdf_sanitise_report_entry(
                    report, 1, i, out var kind, out _, out var number, out var generation));
                Native.Check(Native.tpdf_sanitise_report_action(report, 1, i, out var data, out var len));
                deleted.Add(new DeletedObject((number, generation), (Removal)kind, Native.ReadBytes(data, len)));
            }
            return new SanitiseReport(removed, deleted);
        }
        finally
        {
            Native.tpdf_sanitise_report_free(report);
        }
    }
}

public sealed partial class Document
{
    /// <summary>One of a page's boundaries, resolved the way the reader resolves an absent one.</summary>
    public (double X0, double Y0, double X1, double Y1) PageBox(uint index, PageBoundary boundary)
    {
        Native.Check(Native.tpdf_page_boundary(
            _handle.DangerousGetHandle(), index, (int)boundary,
            out var x0, out var y0, out var x1, out var y1));
        return (x0, y0, x1, y1);
    }
}
