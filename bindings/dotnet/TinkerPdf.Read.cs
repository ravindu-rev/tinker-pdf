// The read surface beyond pages, over the C ABI: /Info and the version, page
// labels, the outline, a page's links, attachments, the XMP packet and the
// warnings an open tolerated.
//
// The same arrangement as Signatures and Verdicts in TinkerPdf.cs: each list
// is a SafeHandle-owned engine copy with index accessors, so it outlives the
// Document it came from, and an index at or past Count throws PdfException
// rather than returning a plausible-looking nothing. A null answer from an
// accessor is the document not saying, which is a different fact from an
// empty one. No logic of its own (ruling 11).

using System;
using System.Runtime.InteropServices;

namespace TinkerPdf;

/// <summary>Which <c>/Info</c> text entry to read (14.3.3, Table 349).</summary>
public enum InfoKey
{
    /// <summary><c>/Title</c>.</summary>
    Title = 0,

    /// <summary><c>/Author</c>.</summary>
    Author = 1,

    /// <summary><c>/Subject</c>.</summary>
    Subject = 2,

    /// <summary><c>/Keywords</c>.</summary>
    Keywords = 3,

    /// <summary><c>/Creator</c>: the application that authored the original.</summary>
    Creator = 4,

    /// <summary><c>/Producer</c>: the application that wrote the PDF.</summary>
    Producer = 5,

    /// <summary><c>/CreationDate</c>, as written.</summary>
    CreationDate = 6,

    /// <summary><c>/ModDate</c>, as written.</summary>
    ModificationDate = 7,
}

/// <summary><c>/Trapped</c> (Table 349), with its absence spelled out.</summary>
public enum Trapped
{
    /// <summary>No <c>/Trapped</c> entry, or no <c>/Info</c> at all.</summary>
    Absent = 0,

    /// <summary><c>/True</c>.</summary>
    True = 1,

    /// <summary><c>/False</c>.</summary>
    False = 2,

    /// <summary><c>/Unknown</c>, or a name that is not one of the three.</summary>
    Unknown = 3,
}

/// <summary>Which of a destination's three arms (12.3.2); never collapsed (ruling 6).</summary>
public enum DestinationKind
{
    /// <summary>No destination: a heading, or an action that carries none.</summary>
    Absent = 0,

    /// <summary>A page in this document and a view of it.</summary>
    Explicit = 1,

    /// <summary>A name to look up in the document's own tables.</summary>
    Named = 2,

    /// <summary>A URI destination.</summary>
    Uri = 3,
}

/// <summary>Which action a link carries (12.6.4).</summary>
public enum ActionKind
{
    /// <summary>Neither <c>/Dest</c> nor a usable <c>/A</c>.</summary>
    Absent = 0,

    /// <summary><c>/GoTo</c>.</summary>
    GoTo = 1,

    /// <summary><c>/GoToR</c>: a destination in another file.</summary>
    GoToR = 2,

    /// <summary><c>/URI</c>.</summary>
    Uri = 3,

    /// <summary><c>/Named</c>: a viewer command.</summary>
    Named = 4,

    /// <summary><c>/Launch</c>: reported, never executed.</summary>
    Launch = 5,

    /// <summary>Any other action type, kept rather than dropped.</summary>
    Other = 6,
}

/// <summary>How a destination positions its page (12.3.2.2), both directions.</summary>
/// <remarks>
/// A null number is the file's <c>null</c>, "retain the current value"; the
/// C ABI spells it NaN, and this type is where that spelling stops. What the
/// kind does not use is null too.
/// </remarks>
public readonly record struct View(
    DestKind Kind,
    double? Left = null,
    double? Bottom = null,
    double? Right = null,
    double? Top = null,
    double? Zoom = null)
{
    internal DestinationRaw ToRaw() => new DestinationRaw
    {
        Kind = (int)Kind,
        Left = Left ?? double.NaN,
        Bottom = Bottom ?? double.NaN,
        Right = Right ?? double.NaN,
        Top = Top ?? double.NaN,
        Zoom = Zoom ?? double.NaN,
    };

    private static double? OrNull(double value) => double.IsNaN(value) ? (double?)null : value;

    internal static View FromRaw(DestinationRaw raw) => new View(
        (DestKind)raw.Kind,
        OrNull(raw.Left),
        OrNull(raw.Bottom),
        OrNull(raw.Right),
        OrNull(raw.Top),
        OrNull(raw.Zoom));
}

/// <summary>Where an outline entry or a link goes (12.3.2).</summary>
/// <param name="Kind">Which arm.</param>
/// <param name="PageIndex">The zero-based page, when the reference resolved.</param>
/// <param name="PageRef">The page object the file named, resolved or not.</param>
/// <param name="View">How the page is positioned, for an explicit destination.</param>
/// <param name="Bytes">The name of a named destination or the URI of a URI one.</param>
public sealed record Destination(
    DestinationKind Kind,
    uint? PageIndex,
    (uint Object, ushort Generation)? PageRef,
    View? View,
    byte[]? Bytes);

/// <summary><c>TpdfDestinationRead</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct DestinationReadRaw
{
    internal int Kind;
    internal int HasPageIndex;
    internal uint PageIndex;
    internal int HasPageRef;
    internal uint PageObject;
    internal ushort PageGeneration;
    internal DestinationRaw View;

    internal Destination? ToDestination(byte[]? bytes)
    {
        var kind = (DestinationKind)Kind;
        if (kind == DestinationKind.Absent)
        {
            return null;
        }
        return new Destination(
            kind,
            HasPageIndex != 0 ? (uint?)PageIndex : null,
            HasPageRef != 0 ? ((uint, ushort)?)(PageObject, PageGeneration) : null,
            kind == DestinationKind.Explicit ? (TinkerPdf.View?)TinkerPdf.View.FromRaw(View) : null,
            bytes);
    }
}

internal static partial class Native
{
    // ---- the read surface ---------------------------------------------------
    //
    // Transcribed from crates/tinker-pdf-ffi/include/tinker_pdf.h. Byte strings
    // cross as a borrowed pointer and length, valid until their handle is
    // freed; ReadBytes copies them out before that.

    [DllImport(Library)]
    internal static extern int tpdf_document_info(IntPtr doc, int key, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_document_trapped(IntPtr doc, out int trapped);

    [DllImport(Library)]
    internal static extern int tpdf_document_pdf_version(IntPtr doc, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_document_page_label(IntPtr doc, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_document_xmp_metadata(IntPtr doc, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern int tpdf_document_outline(IntPtr doc, out IntPtr outline);

    [DllImport(Library)]
    internal static extern uint tpdf_outline_count(IntPtr outline);

    [DllImport(Library)]
    internal static extern int tpdf_outline_item(
        IntPtr outline, uint index, out uint depth, out int open);

    [DllImport(Library)]
    internal static extern int tpdf_outline_title(IntPtr outline, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_outline_destination(
        IntPtr outline, uint index, out DestinationReadRaw destination);

    [DllImport(Library)]
    internal static extern int tpdf_outline_destination_bytes(
        IntPtr outline, uint index, out IntPtr data, out nuint len);

    [DllImport(Library)]
    internal static extern void tpdf_outline_free(IntPtr outline);

    [DllImport(Library)]
    internal static extern int tpdf_page_links(IntPtr doc, uint index, out IntPtr links);

    [DllImport(Library)]
    internal static extern uint tpdf_links_count(IntPtr links);

    [DllImport(Library)]
    internal static extern int tpdf_link_rect(
        IntPtr links, uint index, out double x0, out double y0, out double x1, out double y1);

    [DllImport(Library)]
    internal static extern int tpdf_link_reference(
        IntPtr links, uint index, out int present, out uint objectNumber, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_link_action(
        IntPtr links, uint index, out int kind, out DestinationReadRaw destination);

    [DllImport(Library)]
    internal static extern int tpdf_link_action_bytes(
        IntPtr links, uint index, out IntPtr data, out nuint len);

    [DllImport(Library)]
    internal static extern int tpdf_link_destination_bytes(
        IntPtr links, uint index, out IntPtr data, out nuint len);

    [DllImport(Library)]
    internal static extern void tpdf_links_free(IntPtr links);

    [DllImport(Library)]
    internal static extern int tpdf_document_attachments(IntPtr doc, out IntPtr attachments);

    [DllImport(Library)]
    internal static extern uint tpdf_attachments_count(IntPtr attachments);

    [DllImport(Library)]
    internal static extern int tpdf_attachment_name(IntPtr attachments, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_attachment_filename(
        IntPtr attachments, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_attachment_description(
        IntPtr attachments, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_attachment_size(
        IntPtr attachments, uint index, out int present, out long size);

    [DllImport(Library)]
    internal static extern int tpdf_attachment_data(
        IntPtr attachments, uint index, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern void tpdf_attachments_free(IntPtr attachments);

    [DllImport(Library)]
    internal static extern int tpdf_document_warnings(IntPtr doc, out IntPtr warnings);

    [DllImport(Library)]
    internal static extern uint tpdf_warnings_count(IntPtr warnings);

    [DllImport(Library)]
    internal static extern int tpdf_warning_location(
        IntPtr warnings,
        uint index,
        out ulong offset,
        out int hasObject,
        out uint objectNumber,
        out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_warning_kind(IntPtr warnings, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_warning_message(IntPtr warnings, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern void tpdf_warnings_free(IntPtr warnings);

    /// <summary>A borrowed byte string, copied; null when the engine wrote null.</summary>
    internal static byte[]? ReadBytes(IntPtr data, nuint len)
    {
        if (data == IntPtr.Zero)
        {
            return null;
        }
        var bytes = new byte[checked((int)len)];
        Marshal.Copy(data, bytes, 0, bytes.Length);
        return bytes;
    }

    /// <summary>An engine buffer, copied and freed; null when the engine wrote null.</summary>
    internal static byte[]? TakeBuffer(IntPtr buffer) =>
        buffer == IntPtr.Zero ? null : Buffers.Take(buffer);
}

/// <summary>A handle whose release is one of the read surface's frees.</summary>
internal sealed class ReadHandle : SafeHandle
{
    private readonly Action<IntPtr> _free;

    internal ReadHandle(IntPtr raw, Action<IntPtr> free) : base(IntPtr.Zero, ownsHandle: true)
    {
        _free = free;
        SetHandle(raw);
    }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        _free(handle);
        return true;
    }
}

/// <summary>A document's outline (12.3.3), flattened to reading order.</summary>
/// <remarks>
/// Each entry carries its depth, 0 for a top-level entry, and the nesting is
/// the entries that follow at a greater depth — the C ABI's shape, because a
/// tree of handles would be a tree of frees.
/// </remarks>
public sealed class Outline : IDisposable
{
    private readonly ReadHandle _handle;

    internal Outline(IntPtr raw) => _handle = new ReadHandle(raw, Native.tpdf_outline_free);

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>How many entries, at every depth.</summary>
    public uint Count => Native.tpdf_outline_count(Raw);

    /// <summary>The entry's depth and whether it was saved expanded.</summary>
    public (uint Depth, bool Open) Item(uint index)
    {
        Native.Check(Native.tpdf_outline_item(Raw, index, out var depth, out var open));
        return (depth, open != 0);
    }

    /// <summary>The entry's title, decoded.</summary>
    public string Title(uint index)
    {
        Native.Check(Native.tpdf_outline_title(Raw, index, out var text));
        return Native.TakeString(text) ?? string.Empty;
    }

    /// <summary>Where the entry goes, or null for one that is only a heading.</summary>
    public Destination? DestinationOf(uint index)
    {
        Native.Check(Native.tpdf_outline_destination(Raw, index, out var raw));
        Native.Check(Native.tpdf_outline_destination_bytes(Raw, index, out var data, out var len));
        return raw.ToDestination(Native.ReadBytes(data, len));
    }

    /// <summary>Releases the outline.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>A page's link annotations (12.5.6.5), in <c>/Annots</c> order.</summary>
public sealed class Links : IDisposable
{
    private readonly ReadHandle _handle;

    internal Links(IntPtr raw) => _handle = new ReadHandle(raw, Native.tpdf_links_free);

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>How many links.</summary>
    public uint Count => Native.tpdf_links_count(Raw);

    /// <summary>The link's <c>/Rect</c>, corners ordered.</summary>
    public (double X0, double Y0, double X1, double Y1) Rect(uint index)
    {
        Native.Check(Native.tpdf_link_rect(Raw, index, out var x0, out var y0, out var x1, out var y1));
        return (x0, y0, x1, y1);
    }

    /// <summary>The annotation object, when <c>/Annots</c> named it indirectly.</summary>
    public (uint Object, ushort Generation)? Reference(uint index)
    {
        Native.Check(Native.tpdf_link_reference(
            Raw, index, out var present, out var number, out var generation));
        return present != 0 ? ((uint, ushort)?)(number, generation) : null;
    }

    /// <summary>The action's kind, and its destination when it carries one.</summary>
    public (ActionKind Kind, Destination? Destination) Action(uint index)
    {
        Native.Check(Native.tpdf_link_action(Raw, index, out var kind, out var raw));
        Native.Check(Native.tpdf_link_destination_bytes(Raw, index, out var data, out var len));
        return ((ActionKind)kind, raw.ToDestination(Native.ReadBytes(data, len)));
    }

    /// <summary>
    /// The action's own bytes: a <c>/URI</c>'s URI, a <c>/Named</c>'s name,
    /// another type's <c>/S</c>, a <c>/GoToR</c>'s or <c>/Launch</c>'s file.
    /// </summary>
    public byte[]? ActionBytes(uint index)
    {
        Native.Check(Native.tpdf_link_action_bytes(Raw, index, out var data, out var len));
        return Native.ReadBytes(data, len);
    }

    /// <summary>Releases the links.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>Every file attached to a document (7.11.4), in name order.</summary>
/// <remarks>
/// Listing reads no bytes; <see cref="Data"/> does, through the stream the
/// facade names. The engine's copy holds its own document, so this outlives
/// the <see cref="Document"/> it came from.
/// </remarks>
public sealed class Attachments : IDisposable
{
    private readonly ReadHandle _handle;

    internal Attachments(IntPtr raw) => _handle = new ReadHandle(raw, Native.tpdf_attachments_free);

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>How many attachments.</summary>
    public uint Count => Native.tpdf_attachments_count(Raw);

    /// <summary>The name it is filed under.</summary>
    public string Name(uint index)
    {
        Native.Check(Native.tpdf_attachment_name(Raw, index, out var text));
        return Native.TakeString(text) ?? string.Empty;
    }

    /// <summary><c>/UF</c> or <c>/F</c>: the filename to offer when saving it out.</summary>
    public string Filename(uint index)
    {
        Native.Check(Native.tpdf_attachment_filename(Raw, index, out var text));
        return Native.TakeString(text) ?? string.Empty;
    }

    /// <summary><c>/Desc</c>, or null.</summary>
    public string? Description(uint index)
    {
        Native.Check(Native.tpdf_attachment_description(Raw, index, out var text));
        return Native.TakeString(text);
    }

    /// <summary><c>/Params /Size</c>, or null. Advisory: the stream is the truth.</summary>
    public long? Size(uint index)
    {
        Native.Check(Native.tpdf_attachment_size(Raw, index, out var present, out var size));
        return present != 0 ? (long?)size : null;
    }

    /// <summary>
    /// The file's bytes, decoded; null when the specification names no stream.
    /// A stream that is named and cannot be read throws with
    /// <see cref="Status.StreamUnreadable"/>.
    /// </summary>
    public byte[]? Data(uint index)
    {
        Native.Check(Native.tpdf_attachment_data(Raw, index, out var buffer));
        return Native.TakeBuffer(buffer);
    }

    /// <summary>Releases the list.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>Everything the engine tolerated, in the order it happened (ruling 10).</summary>
public sealed class Warnings : IDisposable
{
    private readonly ReadHandle _handle;

    internal Warnings(IntPtr raw) => _handle = new ReadHandle(raw, Native.tpdf_warnings_free);

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>How many warnings.</summary>
    public uint Count => Native.tpdf_warnings_count(Raw);

    /// <summary>The byte offset that triggered it, and the object being read.</summary>
    public (ulong Offset, (uint Object, ushort Generation)? Object) Location(uint index)
    {
        Native.Check(Native.tpdf_warning_location(
            Raw, index, out var offset, out var hasObject, out var number, out var generation));
        return (offset, hasObject != 0 ? ((uint, ushort)?)(number, generation) : null);
    }

    /// <summary>The stable identifier, such as <c>header-not-at-start</c>.</summary>
    public string Kind(uint index)
    {
        Native.Check(Native.tpdf_warning_kind(Raw, index, out var text));
        return Native.TakeString(text) ?? string.Empty;
    }

    /// <summary>The engine's own sentence for it.</summary>
    public string Message(uint index)
    {
        Native.Check(Native.tpdf_warning_message(Raw, index, out var text));
        return Native.TakeString(text) ?? string.Empty;
    }

    /// <summary>Releases the list.</summary>
    public void Dispose() => _handle.Dispose();
}

public sealed partial class Document
{
    private IntPtr ReadRaw => _handle.DangerousGetHandle();

    /// <summary>One <c>/Info</c> text entry; null when absent, "" when empty.</summary>
    public string? Info(InfoKey key)
    {
        Native.Check(Native.tpdf_document_info(ReadRaw, (int)key, out var text));
        return Native.TakeString(text);
    }

    /// <summary><c>/Info /Trapped</c>, with absence as its own answer.</summary>
    public TinkerPdf.Trapped Trapped
    {
        get
        {
            Native.Check(Native.tpdf_document_trapped(ReadRaw, out var trapped));
            return (TinkerPdf.Trapped)trapped;
        }
    }

    /// <summary>The version, as "PDF 1.7", never absent.</summary>
    public string PdfVersion
    {
        get
        {
            Native.Check(Native.tpdf_document_pdf_version(ReadRaw, out var text));
            return Native.TakeString(text) ?? string.Empty;
        }
    }

    /// <summary>One page's label (12.4.2); null when the document defines none.</summary>
    public string? PageLabel(uint index)
    {
        Native.Check(Native.tpdf_document_page_label(ReadRaw, index, out var text));
        return Native.TakeString(text);
    }

    /// <summary>The XMP packet (14.3.2), unparsed, or null.</summary>
    public byte[]? XmpMetadata()
    {
        Native.Check(Native.tpdf_document_xmp_metadata(ReadRaw, out var buffer));
        return Native.TakeBuffer(buffer);
    }

    /// <summary>The outline, flattened; empty when the document has none.</summary>
    public Outline ReadOutline()
    {
        Native.Check(Native.tpdf_document_outline(ReadRaw, out var raw));
        return new Outline(raw);
    }

    /// <summary>A page's link annotations.</summary>
    public Links ReadLinks(uint page)
    {
        Native.Check(Native.tpdf_page_links(ReadRaw, page, out var raw));
        return new Links(raw);
    }

    /// <summary>The document's attachments.</summary>
    public Attachments ReadAttachments()
    {
        Native.Check(Native.tpdf_document_attachments(ReadRaw, out var raw));
        return new Attachments(raw);
    }

    /// <summary>
    /// The warnings so far. Reading a page can tolerate more, so asking again
    /// later may answer with more.
    /// </summary>
    public Warnings ReadWarnings()
    {
        Native.Check(Native.tpdf_document_warnings(ReadRaw, out var raw));
        return new Warnings(raw);
    }
}
