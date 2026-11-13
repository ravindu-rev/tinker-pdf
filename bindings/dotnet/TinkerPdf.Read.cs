// The read surface beyond pages, over the C ABI: /Info and the version, page
// labels, the outline, a page's links, attachments, the XMP packet, the
// warnings an open tolerated, and a page's command stream.
//
// The same arrangement as Signatures and Verdicts in TinkerPdf.cs: each list
// is a SafeHandle-owned engine copy with index accessors, so it outlives the
// Document it came from, and an index at or past Count throws PdfException
// rather than returning a plausible-looking nothing. A null answer from an
// accessor is the document not saying, which is a different fact from an
// empty one. No logic of its own (ruling 11).

using System;
using System.Collections.Generic;
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
    internal static extern int tpdf_document_page_labels(IntPtr doc, out IntPtr labels);

    [DllImport(Library)]
    internal static extern uint tpdf_page_labels_count(IntPtr labels);

    [DllImport(Library)]
    internal static extern int tpdf_page_label_text(IntPtr labels, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern void tpdf_page_labels_free(IntPtr labels);

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

/// <summary>
/// A document's page labels (12.4.2), one per page in page order: the engine's
/// own copy, built by one walk, so it outlives its document and every indexed
/// read is a lookup.
/// </summary>
public sealed class PageLabels : IDisposable
{
    private readonly ReadHandle _handle;

    internal PageLabels(IntPtr raw) => _handle = new ReadHandle(raw, Native.tpdf_page_labels_free);

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>How many labels: the page count, or zero when the document has none.</summary>
    public uint Count => Native.tpdf_page_labels_count(Raw);

    /// <summary>The label of the page at a zero-based index.</summary>
    public string Label(uint index)
    {
        Native.Check(Native.tpdf_page_label_text(Raw, index, out var text));
        return Native.TakeString(text) ?? string.Empty;
    }

    /// <summary>Releases the list.</summary>
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

    /// <summary>
    /// The page labels (12.4.2), read with one walk: one per page, or none when
    /// the document defines none.
    /// </summary>
    public PageLabels ReadPageLabels()
    {
        Native.Check(Native.tpdf_document_page_labels(ReadRaw, out var raw));
        return new PageLabels(raw);
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

    /// <summary>
    /// A page's drawing as a command stream's versioned bytes
    /// (docs/design/command-stream.md), which
    /// <see cref="TinkerPdf.CommandStream.Parse"/> reads.
    /// </summary>
    public byte[] CommandStream(uint index)
    {
        Native.Check(Native.tpdf_page_command_stream(ReadRaw, index, out var buffer));
        return Buffers.Take(buffer);
    }
}

// ---- the command stream (docs/design/command-stream.md) ---------------------
//
// Over crates/tinker-pdf-ffi's src/commands.rs, call for call: the cursor and
// a record read by its place, each field as plain values copied out of the
// stream's storage before its handle is freed, the two tables a host draws
// from, the encoder, the stream's own facts and its render. Replay is a loop
// of those reads, handing each record to the device method its kind names.
// The structs are the header's, field for field, at the offsets
// `tests/layout.rs` pins.

/// <summary><c>TpdfCommand</c>: one record as the cursor reads it.</summary>
/// <param name="Kind">
/// The record's kind, or null for a kind this build does not know, which a
/// stream of a newer minor version holds.
/// </param>
/// <param name="Number">The kind's number on the wire, known or not.</param>
/// <param name="Index">
/// The record's place, from 0, which the stream's accessors name it by; the
/// end stands at the number of records.
/// </param>
/// <param name="FieldCount">How many fields the record holds.</param>
public readonly record struct Command(CommandKind? Kind, uint Number, uint Index, uint FieldCount);

/// <summary>One field of a command-stream record: its tag beside the value the tag says it holds.</summary>
/// <remarks>
/// <see cref="Value"/> is null for <see cref="CommandFieldTag.Nothing"/>, a
/// <see cref="ulong"/> for <see cref="CommandFieldTag.Unsigned"/>, a
/// <see cref="long"/> for <see cref="CommandFieldTag.Signed"/>, a
/// <see cref="double"/> for <see cref="CommandFieldTag.Real"/>, and a
/// <c>byte[]</c>, <c>double[]</c>, <c>ulong[]</c> or
/// <c>IReadOnlyList&lt;byte[]&gt;</c> for bytes, reals, naturals and strings,
/// each a copy. Two fields are equal when their tags and values are, a real by
/// its bits.
/// </remarks>
public sealed class CommandField : IEquatable<CommandField>
{
    private readonly object? _value;

    private CommandField(CommandFieldTag tag, object? value)
    {
        Tag = tag;
        _value = value;
    }

    /// <summary>What the field holds.</summary>
    public CommandFieldTag Tag { get; }

    /// <summary>The value, as the tag says it is held; an array is a copy.</summary>
    public object? Value => _value switch
    {
        byte[] bytes => bytes.Clone(),
        double[] reals => reals.Clone(),
        ulong[] naturals => naturals.Clone(),
        IReadOnlyList<byte[]> strings => Copies(strings),
        _ => _value,
    };

    /// <summary>An absent value.</summary>
    public static CommandField Nothing() => new(CommandFieldTag.Nothing, null);

    /// <summary>An unsigned integer. A reference to a record is one.</summary>
    public static CommandField Unsigned(ulong value) => new(CommandFieldTag.Unsigned, value);

    /// <summary>A signed integer.</summary>
    public static CommandField Signed(long value) => new(CommandFieldTag.Signed, value);

    /// <summary>A real, its bits as given.</summary>
    public static CommandField Real(double value) => new(CommandFieldTag.Real, value);

    /// <summary>Bytes, copied.</summary>
    public static CommandField Bytes(byte[] value) =>
        new(CommandFieldTag.Bytes, (byte[])value.Clone());

    /// <summary>Reals, copied.</summary>
    public static CommandField Reals(double[] value) =>
        new(CommandFieldTag.Reals, (double[])value.Clone());

    /// <summary>Unsigned integers, copied.</summary>
    public static CommandField Naturals(ulong[] value) =>
        new(CommandFieldTag.Naturals, (ulong[])value.Clone());

    /// <summary>Byte strings, copied.</summary>
    public static CommandField Strings(IReadOnlyList<byte[]> value) =>
        new(CommandFieldTag.Strings, Copies(value));

    private static byte[][] Copies(IReadOnlyList<byte[]> strings)
    {
        var copies = new byte[strings.Count][];
        for (var at = 0; at < copies.Length; at++)
        {
            copies[at] = (byte[])strings[at].Clone();
        }
        return copies;
    }

    private static long[] Bits(double[] reals) => Array.ConvertAll(reals, BitConverter.DoubleToInt64Bits);

    /// <summary>Whether both fields hold one tag and one value, a real by its bits.</summary>
    public bool Equals(CommandField? other)
    {
        if (other is null || other.Tag != Tag)
        {
            return false;
        }
        switch (_value, other._value)
        {
            case (null, null):
                return true;
            case (ulong a, ulong b):
                return a == b;
            case (long a, long b):
                return a == b;
            case (double a, double b):
                return BitConverter.DoubleToInt64Bits(a) == BitConverter.DoubleToInt64Bits(b);
            case (byte[] a, byte[] b):
                return a.AsSpan().SequenceEqual(b);
            case (double[] a, double[] b):
                return Bits(a).AsSpan().SequenceEqual(Bits(b));
            case (ulong[] a, ulong[] b):
                return a.AsSpan().SequenceEqual(b);
            case (byte[][] a, byte[][] b):
                if (a.Length != b.Length)
                {
                    return false;
                }
                for (var at = 0; at < a.Length; at++)
                {
                    if (!a[at].AsSpan().SequenceEqual(b[at]))
                    {
                        return false;
                    }
                }
                return true;
            default:
                return false;
        }
    }

    /// <inheritdoc/>
    public override bool Equals(object? obj) => Equals(obj as CommandField);

    /// <inheritdoc/>
    public override int GetHashCode()
    {
        var hash = new HashCode();
        hash.Add(Tag);
        switch (_value)
        {
            case byte[] bytes:
                foreach (var value in bytes)
                {
                    hash.Add(value);
                }
                break;
            case double[] reals:
                foreach (var value in reals)
                {
                    hash.Add(BitConverter.DoubleToInt64Bits(value));
                }
                break;
            case ulong[] naturals:
                foreach (var value in naturals)
                {
                    hash.Add(value);
                }
                break;
            case byte[][] strings:
                foreach (var value in strings)
                {
                    hash.Add(value.Length);
                }
                break;
            case double real:
                hash.Add(BitConverter.DoubleToInt64Bits(real));
                break;
            case not null:
                hash.Add(_value);
                break;
        }
        return hash.ToHashCode();
    }

    /// <inheritdoc/>
    public override string ToString() => _value switch
    {
        null => Tag.ToString(),
        byte[] bytes => $"{Tag} [{string.Join(", ", bytes)}]",
        double[] reals => $"{Tag} [{string.Join(", ", reals)}]",
        ulong[] naturals => $"{Tag} [{string.Join(", ", naturals)}]",
        byte[][] strings => $"{Tag} of {strings.Length}",
        _ => $"{Tag} {_value}",
    };
}

/// <summary>A record and its fields, in their places.</summary>
/// <remarks>
/// What each place holds is the kind's (docs/design/command-stream.md, "The
/// records"): a command's state is its field 0, a reference to the
/// <see cref="CommandKind.State"/> records before it in their order.
/// </remarks>
public sealed class CommandRecord
{
    internal CommandRecord(Command command, IReadOnlyList<CommandField> fields)
    {
        Kind = command.Kind;
        Number = command.Number;
        Index = command.Index;
        Fields = fields;
    }

    /// <summary>The record's kind, or null for a kind this build does not know.</summary>
    public CommandKind? Kind { get; }

    /// <summary>The kind's number on the wire.</summary>
    public uint Number { get; }

    /// <summary>The record's place in its stream, from 0.</summary>
    public uint Index { get; }

    /// <summary>The record's fields, copied out of the stream.</summary>
    public IReadOnlyList<CommandField> Fields { get; }
}

/// <summary><c>TpdfCommandOutline</c>: a glyph's outline record.</summary>
/// <param name="Scope">The scope the glyph was asked in: 0 the page's, <c>n</c> the <c>n</c>th scope record's.</param>
/// <param name="FontId">The font the glyph is of, as the stream's glyph runs name it.</param>
/// <param name="Code">The glyph's code.</param>
/// <param name="Found">Whether the font gave the glyph an outline; both arrays are empty where it did not.</param>
/// <param name="Operators">A byte a segment: 0 a move, 1 a line, 2 a quadratic, 3 a cubic, 4 a close.</param>
/// <param name="Numbers">The segments' numbers in order, in a space where a unit is an em.</param>
public sealed record CommandOutline(ulong Scope, ulong FontId, uint Code, bool Found, byte[] Operators, double[] Numbers);

/// <summary><c>TpdfCommandImage</c>: an image's decode at one reduction.</summary>
/// <param name="Image">The image the decode is of: the <c>n</c>th image record, inline or not, from 0.</param>
/// <param name="Reduction">1 the whole image, or what a draw at a smaller size asks a JPEG or a codestream for.</param>
/// <param name="Answer">What the page's resources answered.</param>
/// <param name="Width">The decode's width in samples.</param>
/// <param name="Height">Its height in samples.</param>
/// <param name="Window">The window the samples are, its top edge counting down; null where they are all of the decode.</param>
/// <param name="Stencil">A stencil mask, which paints the fill colour where it is set.</param>
/// <param name="Interpolate">The image's <c>/Interpolate</c>.</param>
/// <param name="Codec">A refused decode's codec; empty otherwise.</param>
/// <param name="Rgb">Three bytes a sample, row-major from the top; empty where <paramref name="TwoColour"/> holds the samples.</param>
/// <param name="Alpha">Opacity, a byte a sample; empty for an opaque image.</param>
/// <param name="TwoColour">A byte a sample choosing between <paramref name="Colours"/>' two, zero the first.</param>
/// <param name="Colours">Red, green, blue and alpha of a sample of zero, then of any other.</param>
public sealed record CommandImage(
    ulong Image,
    uint Reduction,
    ImageAnswer Answer,
    uint Width,
    uint Height,
    (uint X, uint Y, uint Width, uint Height)? Window,
    bool Stencil,
    bool Interpolate,
    string Codec,
    byte[] Rgb,
    byte[] Alpha,
    byte[] TwoColour,
    byte[] Colours);

/// <summary>
/// A device a replay hands records to: one method a kind, each handed the
/// record, and <see cref="Unknown"/> for a kind this build does not know.
/// </summary>
/// <remarks>
/// Every method does nothing unless a device implements it, so a device names
/// the kinds it draws and passes over the rest. A record's tables, its glyphs'
/// outlines and its images' decodes, are the stream's
/// <see cref="CommandStream.OutlineAt"/> and <see cref="CommandStream.ImageAt"/>,
/// which a device that draws them holds the stream for. A replay to a platform
/// canvas is a device over this projection and no part of it; the README
/// replays a stream to <c>System.Drawing.Graphics</c> on Windows.
/// </remarks>
public interface ICommandDevice
{
    /// <summary>A <see cref="CommandKind.Function"/> record.</summary>
    void Function(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Halftone"/> record.</summary>
    void Halftone(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Stored"/> record.</summary>
    void Stored(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Colorants"/> record.</summary>
    void Colorants(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Source"/> record.</summary>
    void Source(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.State"/> record.</summary>
    void State(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.BeginText"/> record.</summary>
    void BeginText(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.GlyphRun"/> record.</summary>
    void GlyphRun(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.EndText"/> record.</summary>
    void EndText(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.FillPath"/> record.</summary>
    void FillPath(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.StrokePath"/> record.</summary>
    void StrokePath(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.ClipPath"/> record.</summary>
    void ClipPath(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.SaveState"/> record.</summary>
    void SaveState(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.RestoreState"/> record.</summary>
    void RestoreState(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.DrawImage"/> record.</summary>
    void DrawImage(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.DrawShading"/> record.</summary>
    void DrawShading(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.BeginMarkedContent"/> record.</summary>
    void BeginMarkedContent(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.EndMarkedContent"/> record.</summary>
    void EndMarkedContent(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.BeginForm"/> record.</summary>
    void BeginForm(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.EndForm"/> record.</summary>
    void EndForm(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.BeginGroup"/> record.</summary>
    void BeginGroup(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.EndGroup"/> record.</summary>
    void EndGroup(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.BeginSoftMask"/> record.</summary>
    void BeginSoftMask(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.EndSoftMask"/> record.</summary>
    void EndSoftMask(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.ClearSoftMask"/> record.</summary>
    void ClearSoftMask(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.GroupBudgetSpent"/> record.</summary>
    void GroupBudgetSpent(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.RecursionRefused"/> record.</summary>
    void RecursionRefused(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.NestedBudgetSpent"/> record.</summary>
    void NestedBudgetSpent(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Page"/> record.</summary>
    void Page(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.Annotation"/> record.</summary>
    void Annotation(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.MissingFont"/> record.</summary>
    void MissingFont(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.RepairedSpace"/> record.</summary>
    void RepairedSpace(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Scope"/> record.</summary>
    void Scope(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.Outline"/> record.</summary>
    void Outline(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.FontIdentity"/> record.</summary>
    void FontIdentity(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Profile"/> record.</summary>
    void Profile(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.ColorSpace"/> record.</summary>
    void ColorSpace(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.Image"/> record.</summary>
    void Image(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.InlineImage"/> record.</summary>
    void InlineImage(CommandRecord record) { }

    /// <summary>An <see cref="CommandKind.ImageDecode"/> record.</summary>
    void ImageDecode(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.ShadingDefinition"/> record.</summary>
    void ShadingDefinition(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Shading"/> record.</summary>
    void Shading(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Pattern"/> record.</summary>
    void Pattern(CommandRecord record) { }

    /// <summary>A <see cref="CommandKind.Cell"/> record.</summary>
    void Cell(CommandRecord record) { }

    /// <summary>A record of a kind this build does not know.</summary>
    void Unknown(CommandRecord record) { }
}

/// <summary><c>TpdfCommand</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct CommandRaw
{
    internal int Kind;
    internal uint Number;
    internal uint Index;
    internal uint FieldCount;
}

/// <summary><c>TpdfCommandField</c>, field for field: 48 bytes, padded after its tag.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct CommandFieldRaw
{
    internal int Tag;
    internal ulong Natural;
    internal long Integer;
    internal double Real;
    internal IntPtr Data;
    internal nuint Count;
}

/// <summary><c>TpdfCommandBytes</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct CommandBytesRaw
{
    internal IntPtr Data;
    internal nuint Len;
}

/// <summary><c>TpdfCommandOutline</c>, field for field: 56 bytes.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct CommandOutlineRaw
{
    internal ulong Scope;
    internal ulong FontId;
    internal uint Code;
    internal int Found;
    internal CommandBytesRaw Operators;
    internal IntPtr Numbers;
    internal nuint NumberCount;
}

/// <summary><c>TpdfCommandImage</c>, field for field: 128 bytes.</summary>
[StructLayout(LayoutKind.Sequential)]
internal unsafe struct CommandImageRaw
{
    internal ulong Image;
    internal uint Reduction;
    internal int Answer;
    internal uint Width;
    internal uint Height;
    internal int HasWindow;
    internal uint WindowX;
    internal uint WindowY;
    internal uint WindowWidth;
    internal uint WindowHeight;
    internal int Stencil;
    internal int Interpolate;
    internal CommandBytesRaw Codec;
    internal CommandBytesRaw Rgb;
    internal CommandBytesRaw Alpha;
    internal CommandBytesRaw TwoColour;
    internal fixed byte Colours[8];
}

internal static partial class Native
{
    // ---- the command stream -------------------------------------------------
    //
    // Transcribed from crates/tinker-pdf-ffi/include/tinker_pdf.h. A field's
    // and an accessor's pointers borrow the stream handle's storage; the
    // wrapper copies them out before it can be freed.

    [DllImport(Library)]
    internal static extern int tpdf_page_command_stream(IntPtr doc, uint index, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_parse(byte[] bytes, nuint len, out IntPtr stream);

    [DllImport(Library)]
    internal static extern uint tpdf_command_stream_count(IntPtr stream);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_next(IntPtr stream, out CommandRaw command);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_rewind(IntPtr stream);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_record(IntPtr stream, uint index, out CommandRaw command);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_field(
        IntPtr stream, uint index, uint field, out CommandFieldRaw value);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_outline(
        IntPtr stream, uint index, out CommandOutlineRaw outline);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_image(
        IntPtr stream, uint index, out CommandImageRaw image);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_to_bytes(IntPtr stream, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_version(
        IntPtr stream, out ushort major, out ushort minor);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_page_index(IntPtr stream, out uint index);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_size(
        IntPtr stream, out double width, out double height);

    [DllImport(Library)]
    internal static extern int tpdf_command_stream_render(
        IntPtr stream, double scale, int format, out IntPtr bitmap);

    [DllImport(Library)]
    internal static extern void tpdf_command_stream_free(IntPtr stream);

    [DllImport(Library)]
    internal static extern int tpdf_command_encoder_new(out IntPtr encoder);

    [DllImport(Library)]
    internal static extern int tpdf_command_encoder_push(
        IntPtr encoder, int kind, [In] CommandFieldRaw[] fields, nuint count);

    [DllImport(Library)]
    internal static extern int tpdf_command_encoder_finish(IntPtr encoder, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern void tpdf_command_encoder_free(IntPtr encoder);
}

/// <summary>
/// A command stream read (docs/design/command-stream.md), held by the engine:
/// a page's drawing as versioned bytes, from
/// <see cref="Document.CommandStream"/>.
/// </summary>
/// <remarks>
/// Every member is a call of the C ABI's, or a loop of them, and everything
/// it hands back is a copy that outlives the stream. Bytes the reader refuses
/// throw <see cref="PdfException"/> with <see cref="Status.BadArgument"/> and
/// the reader's own sentence.
/// </remarks>
public sealed class CommandStream : IDisposable
{
    private readonly ReadHandle _handle;

    private CommandStream(IntPtr raw) => _handle = new ReadHandle(raw, Native.tpdf_command_stream_free);

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Reads a stream's bytes, which are read and not kept.</summary>
    public static CommandStream Parse(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        Native.Check(Native.tpdf_command_stream_parse(bytes, (nuint)bytes.Length, out var raw));
        return new CommandStream(raw);
    }

    /// <summary>How many records the stream holds before its end.</summary>
    public uint Count => Native.tpdf_command_stream_count(Raw);

    /// <summary>
    /// The cursor: the next record's command, moving past it. After the last
    /// record it reads <see cref="CommandKind.End"/>, at the number of
    /// records, until <see cref="Rewind"/>.
    /// </summary>
    public Command Next()
    {
        Native.Check(Native.tpdf_command_stream_next(Raw, out var raw));
        return Of(raw);
    }

    /// <summary>Puts the cursor back on the stream's first record.</summary>
    public void Rewind() => Native.Check(Native.tpdf_command_stream_rewind(Raw));

    /// <summary>Record <paramref name="index"/>'s command, wherever the cursor stands.</summary>
    public Command CommandAt(uint index)
    {
        Native.Check(Native.tpdf_command_stream_record(Raw, index, out var raw));
        return Of(raw);
    }

    /// <summary>Field <paramref name="field"/> of record <paramref name="index"/>, copied.</summary>
    public CommandField FieldAt(uint index, uint field)
    {
        Native.Check(Native.tpdf_command_stream_field(Raw, index, field, out var raw));
        return (CommandFieldTag)raw.Tag switch
        {
            CommandFieldTag.Nothing => CommandField.Nothing(),
            CommandFieldTag.Unsigned => CommandField.Unsigned(raw.Natural),
            CommandFieldTag.Signed => CommandField.Signed(raw.Integer),
            CommandFieldTag.Real => CommandField.Real(raw.Real),
            CommandFieldTag.Bytes => CommandField.Bytes(Copy<byte>(raw.Data, raw.Count)),
            CommandFieldTag.Reals => CommandField.Reals(Copy<double>(raw.Data, raw.Count)),
            CommandFieldTag.Naturals => CommandField.Naturals(Copy<ulong>(raw.Data, raw.Count)),
            CommandFieldTag.Strings => CommandField.Strings(Array.ConvertAll(
                Copy<CommandBytesRaw>(raw.Data, raw.Count), each => Copy<byte>(each.Data, each.Len))),
            _ => throw new PdfException($"field {field} of record {index} is of tag {raw.Tag}"),
        };
    }

    /// <summary>Record <paramref name="index"/>: its command and every one of its fields.</summary>
    public CommandRecord RecordAt(uint index)
    {
        var command = CommandAt(index);
        var fields = new CommandField[command.FieldCount];
        for (uint at = 0; at < command.FieldCount; at++)
        {
            fields[at] = FieldAt(index, at);
        }
        return new CommandRecord(command, fields);
    }

    /// <summary>Every record, in the stream's order.</summary>
    public IReadOnlyList<CommandRecord> Records()
    {
        var count = Count;
        var records = new List<CommandRecord>(checked((int)count));
        for (uint index = 0; index < count; index++)
        {
            records.Add(RecordAt(index));
        }
        return records;
    }

    /// <summary>Outline record <paramref name="index"/>'s glyph. A record of another kind is refused, naming both.</summary>
    public CommandOutline OutlineAt(uint index)
    {
        Native.Check(Native.tpdf_command_stream_outline(Raw, index, out var raw));
        return new CommandOutline(
            raw.Scope,
            raw.FontId,
            raw.Code,
            raw.Found != 0,
            Copy<byte>(raw.Operators.Data, raw.Operators.Len),
            Copy<double>(raw.Numbers, raw.NumberCount));
    }

    /// <summary>Image decode record <paramref name="index"/>. A record of another kind is refused, naming both.</summary>
    public CommandImage ImageAt(uint index)
    {
        Native.Check(Native.tpdf_command_stream_image(Raw, index, out var raw));
        var colours = new byte[8];
        unsafe
        {
            for (var at = 0; at < colours.Length; at++)
            {
                colours[at] = raw.Colours[at];
            }
        }
        return new CommandImage(
            raw.Image,
            raw.Reduction,
            (ImageAnswer)raw.Answer,
            raw.Width,
            raw.Height,
            raw.HasWindow != 0
                ? (raw.WindowX, raw.WindowY, raw.WindowWidth, raw.WindowHeight)
                : ((uint, uint, uint, uint)?)null,
            raw.Stencil != 0,
            raw.Interpolate != 0,
            System.Text.Encoding.UTF8.GetString(Copy<byte>(raw.Codec.Data, raw.Codec.Len)),
            Copy<byte>(raw.Rgb.Data, raw.Rgb.Len),
            Copy<byte>(raw.Alpha.Data, raw.Alpha.Len),
            Copy<byte>(raw.TwoColour.Data, raw.TwoColour.Len),
            colours);
    }

    /// <summary>The stream's bytes, written back at its own version.</summary>
    public byte[] ToBytes()
    {
        Native.Check(Native.tpdf_command_stream_to_bytes(Raw, out var buffer));
        return Buffers.Take(buffer);
    }

    /// <summary>The stream's major and minor version.</summary>
    public (ushort Major, ushort Minor) Version
    {
        get
        {
            Native.Check(Native.tpdf_command_stream_version(Raw, out var major, out var minor));
            return (major, minor);
        }
    }

    /// <summary>Which page of its document the stream is of.</summary>
    public uint PageIndex
    {
        get
        {
            Native.Check(Native.tpdf_command_stream_page_index(Raw, out var index));
            return index;
        }
    }

    /// <summary>The page's size in points after rotation.</summary>
    public (double Width, double Height) Size
    {
        get
        {
            Native.Check(Native.tpdf_command_stream_size(Raw, out var width, out var height));
            return (width, height);
        }
    }

    /// <summary>Draws the stream as a render draws its page, through the engine's renderer. A scale of 1.0 is 72 dpi.</summary>
    public Bitmap Render(double scale = 1.0, PixelFormat format = PixelFormat.Rgb8)
    {
        Native.Check(Native.tpdf_command_stream_render(Raw, scale, (int)format, out var raw));
        return new Bitmap(raw);
    }

    /// <summary>
    /// Hands every record, in the stream's order, to the method of
    /// <paramref name="device"/> named for its kind, and a record of a kind
    /// this build does not know to <see cref="ICommandDevice.Unknown"/>. An
    /// exception a method throws ends the replay and is thrown.
    /// </summary>
    public void Replay(ICommandDevice device)
    {
        ArgumentNullException.ThrowIfNull(device);
        var count = Count;
        for (uint index = 0; index < count; index++)
        {
            var record = RecordAt(index);
            switch (record.Kind)
            {
                case null: device.Unknown(record); break;
                // The end is where a stream stops: no record is of its kind.
                case CommandKind.End: break;
                case CommandKind.Function: device.Function(record); break;
                case CommandKind.Halftone: device.Halftone(record); break;
                case CommandKind.Stored: device.Stored(record); break;
                case CommandKind.Colorants: device.Colorants(record); break;
                case CommandKind.Source: device.Source(record); break;
                case CommandKind.State: device.State(record); break;
                case CommandKind.BeginText: device.BeginText(record); break;
                case CommandKind.GlyphRun: device.GlyphRun(record); break;
                case CommandKind.EndText: device.EndText(record); break;
                case CommandKind.FillPath: device.FillPath(record); break;
                case CommandKind.StrokePath: device.StrokePath(record); break;
                case CommandKind.ClipPath: device.ClipPath(record); break;
                case CommandKind.SaveState: device.SaveState(record); break;
                case CommandKind.RestoreState: device.RestoreState(record); break;
                case CommandKind.DrawImage: device.DrawImage(record); break;
                case CommandKind.DrawShading: device.DrawShading(record); break;
                case CommandKind.BeginMarkedContent: device.BeginMarkedContent(record); break;
                case CommandKind.EndMarkedContent: device.EndMarkedContent(record); break;
                case CommandKind.BeginForm: device.BeginForm(record); break;
                case CommandKind.EndForm: device.EndForm(record); break;
                case CommandKind.BeginGroup: device.BeginGroup(record); break;
                case CommandKind.EndGroup: device.EndGroup(record); break;
                case CommandKind.BeginSoftMask: device.BeginSoftMask(record); break;
                case CommandKind.EndSoftMask: device.EndSoftMask(record); break;
                case CommandKind.ClearSoftMask: device.ClearSoftMask(record); break;
                case CommandKind.GroupBudgetSpent: device.GroupBudgetSpent(record); break;
                case CommandKind.RecursionRefused: device.RecursionRefused(record); break;
                case CommandKind.NestedBudgetSpent: device.NestedBudgetSpent(record); break;
                case CommandKind.Page: device.Page(record); break;
                case CommandKind.Annotation: device.Annotation(record); break;
                case CommandKind.MissingFont: device.MissingFont(record); break;
                case CommandKind.RepairedSpace: device.RepairedSpace(record); break;
                case CommandKind.Scope: device.Scope(record); break;
                case CommandKind.Outline: device.Outline(record); break;
                case CommandKind.FontIdentity: device.FontIdentity(record); break;
                case CommandKind.Profile: device.Profile(record); break;
                case CommandKind.ColorSpace: device.ColorSpace(record); break;
                case CommandKind.Image: device.Image(record); break;
                case CommandKind.InlineImage: device.InlineImage(record); break;
                case CommandKind.ImageDecode: device.ImageDecode(record); break;
                case CommandKind.ShadingDefinition: device.ShadingDefinition(record); break;
                case CommandKind.Shading: device.Shading(record); break;
                case CommandKind.Pattern: device.Pattern(record); break;
                case CommandKind.Cell: device.Cell(record); break;
            }
        }
    }

    /// <summary>Releases the stream. What was read from it stays valid.</summary>
    public void Dispose() => _handle.Dispose();

    private static Command Of(CommandRaw raw) => new(
        raw.Kind < 0 ? (CommandKind?)null : (CommandKind)raw.Kind,
        raw.Number,
        raw.Index,
        raw.FieldCount);

    /// <summary><paramref name="count"/> values the stream lends at <paramref name="data"/>, copied.</summary>
    private static unsafe T[] Copy<T>(IntPtr data, nuint count)
        where T : unmanaged =>
        data == IntPtr.Zero || count == 0
            ? Array.Empty<T>()
            : new ReadOnlySpan<T>(data.ToPointer(), checked((int)count)).ToArray();
}

/// <summary>
/// <c>TpdfCommandEncoder</c>: records pushed by kind and fields, copied, and
/// finished into a stream's bytes at this build's version, read back as
/// <see cref="CommandStream.Parse"/> reads them.
/// </summary>
/// <remarks>
/// A kind or a tag the C ABI does not declare, the end pushed, and records
/// whose stream the reader refuses throw <see cref="PdfException"/> with
/// <see cref="Status.BadArgument"/> naming them. The encoder keeps its
/// records, so it finishes again.
/// </remarks>
public sealed class CommandEncoder : IDisposable
{
    private readonly ReadHandle _handle;

    /// <summary>An encoder holding no records.</summary>
    public CommandEncoder()
    {
        Native.Check(Native.tpdf_command_encoder_new(out var raw));
        _handle = new ReadHandle(raw, Native.tpdf_command_encoder_free);
    }

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Appends a record of <paramref name="kind"/> and its fields.</summary>
    public void Push(CommandKind kind, IReadOnlyList<CommandField> fields)
    {
        ArgumentNullException.ThrowIfNull(fields);
        var pins = new List<GCHandle>();
        try
        {
            var raw = new CommandFieldRaw[fields.Count];
            for (var at = 0; at < raw.Length; at++)
            {
                raw[at] = Lay(fields[at], pins);
            }
            Native.Check(Native.tpdf_command_encoder_push(Raw, (int)kind, raw, (nuint)raw.Length));
        }
        finally
        {
            foreach (var pin in pins)
            {
                pin.Free();
            }
        }
    }

    /// <summary>
    /// Appends <paramref name="record"/>'s kind and fields; a kind this build
    /// does not know crosses as the C ABI's -1, which it refuses.
    /// </summary>
    public void Push(CommandRecord record)
    {
        ArgumentNullException.ThrowIfNull(record);
        Push(record.Kind ?? (CommandKind)(-1), record.Fields);
    }

    /// <summary>The records pushed so far as a stream's bytes.</summary>
    public byte[] Finish()
    {
        Native.Check(Native.tpdf_command_encoder_finish(Raw, out var buffer));
        return Buffers.Take(buffer);
    }

    /// <summary>Releases the encoder.</summary>
    public void Dispose() => _handle.Dispose();

    /// <summary><paramref name="field"/> as plain values, its arrays pinned until the push returns.</summary>
    private static CommandFieldRaw Lay(CommandField field, List<GCHandle> pins)
    {
        var raw = new CommandFieldRaw { Tag = (int)field.Tag };
        switch (field.Value)
        {
            case ulong natural:
                raw.Natural = natural;
                break;
            case long integer:
                raw.Integer = integer;
                break;
            case double real:
                raw.Real = real;
                break;
            case byte[] bytes:
                raw.Data = Pin(bytes, pins);
                raw.Count = (nuint)bytes.Length;
                break;
            case double[] reals:
                raw.Data = Pin(reals, pins);
                raw.Count = (nuint)reals.Length;
                break;
            case ulong[] naturals:
                raw.Data = Pin(naturals, pins);
                raw.Count = (nuint)naturals.Length;
                break;
            case byte[][] strings:
                var list = new CommandBytesRaw[strings.Length];
                for (var at = 0; at < list.Length; at++)
                {
                    list[at] = new CommandBytesRaw { Data = Pin(strings[at], pins), Len = (nuint)strings[at].Length };
                }
                raw.Data = Pin(list, pins);
                raw.Count = (nuint)list.Length;
                break;
        }
        return raw;
    }

    private static IntPtr Pin(object array, List<GCHandle> pins)
    {
        var pin = GCHandle.Alloc(array, GCHandleType.Pinned);
        pins.Add(pin);
        return pin.AddrOfPinnedObject();
    }
}
