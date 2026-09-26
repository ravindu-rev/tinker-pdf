// A safe C# wrapper over tinker-pdf's C ABI.
//
// The P/Invoke declarations below mirror `crates/tinker-pdf-ffi`. They are
// written out here rather than generated so the binding builds with nothing
// but the .NET SDK; regenerating them with csbindgen is a build-time
// convenience, not a requirement, and either way this file adds no logic of
// its own (ruling 11).
//
// Handle lifetime is the whole job of this wrapper. Every native handle lives
// in a SafeHandle, so a document or bitmap is released exactly once even if an
// exception unwinds past it, and the pixel span a caller gets is tied to the
// bitmap that owns it.

using System;
using System.IO;
using System.Runtime.InteropServices;

namespace TinkerPdf;

/// <summary>How far a password got.</summary>
public enum AuthLevel
{
    /// <summary>Unencrypted, or no password accepted yet.</summary>
    None = 0,

    /// <summary>The user password matched; the document's permissions apply.</summary>
    User = 1,

    /// <summary>The owner password matched; restrictions are lifted.</summary>
    Owner = 2,
}

/// <summary>How a bitmap stores its pixels.</summary>
public enum PixelFormat
{
    /// <summary>One byte of grey.</summary>
    Gray8 = 0,

    /// <summary>Grey and alpha.</summary>
    GrayA8 = 1,

    /// <summary>Red, green, blue.</summary>
    Rgb8 = 2,

    /// <summary>Red, green, blue, alpha.</summary>
    Rgba8 = 3,
}

/// <summary>What a signature's /ByteRange covers, checked against the file.</summary>
/// <remarks>
/// The engine's <c>Coverage</c> carries a revision index on one arm and a
/// named defect on another; neither crosses the C ABI, because a C enum has no
/// payload. The three-way answer does, and it is the one a caller acts on.
/// </remarks>
public enum Coverage
{
    /// <summary>Every byte of the file except the gap holding /Contents.</summary>
    WholeFile = 0,

    /// <summary>
    /// Every byte up to the end of an earlier revision, with later incremental
    /// updates outside it. Which revision does not cross.
    /// </summary>
    Revision = 1,

    /// <summary>Anything else. The reason does not cross.</summary>
    Suspicious = 2,
}

/// <summary>Whether the CMS blob could be read.</summary>
public enum CmsState
{
    /// <summary>It parsed.</summary>
    Read = 0,

    /// <summary>/Contents held no bytes to parse.</summary>
    Absent = 1,

    /// <summary>Bytes were there and would not parse.</summary>
    Unreadable = 2,
}

/// <summary>Whether the document still hashes to what the signature was made over.</summary>
public enum DocumentDigest
{
    /// <summary>The covered bytes digest to what the CMS says they did.</summary>
    Matches = 0,

    /// <summary>They do not: either the bytes changed or the signature was never over them.</summary>
    Differs = 1,

    /// <summary>
    /// Not checked, which is <b>not</b> a failure. Folding this into
    /// <see cref="Differs"/> confuses "we did not look" with "we looked and it
    /// was wrong", which is the shape of every convincing forgery.
    /// </summary>
    NotChecked = 2,
}

/// <summary>Whether the signature verifies against the signer's own public key.</summary>
public enum SignatureCheck
{
    /// <summary>It verifies.</summary>
    Verified = 0,

    /// <summary>The arithmetic ran and the signature is not the one that key would have made.</summary>
    Failed = 1,

    /// <summary>Not checked, and — as with <see cref="DocumentDigest.NotChecked"/> — not a failure.</summary>
    NotChecked = 2,
}

/// <summary>How far the certificate chain reached.</summary>
public enum Chain
{
    /// <summary>A path was built to a caller-supplied anchor and every link verified.</summary>
    AnchoredTo = 0,

    /// <summary>The path ends at a self-signed certificate that is not an anchor.</summary>
    SelfSigned = 1,

    /// <summary>No issuer for some certificate was found, so the path stops.</summary>
    Incomplete = 2,

    /// <summary>A link's signature did not verify, so the path is not a path.</summary>
    Broken = 3,

    /// <summary>
    /// No anchors were supplied, so no path was attempted. Distinct from
    /// <see cref="Incomplete"/>: the caller declined to say what it trusts.
    /// </summary>
    NoAnchors = 4,

    /// <summary>The signer's certificate was not in the blob.</summary>
    NoSignerCertificate = 5,
}

/// <summary>Something the verdict accepted that a caller should be told about.</summary>
public enum Weakness
{
    /// <summary>The document digest is SHA-1, which is not collision-resistant.</summary>
    Sha1Digest = 0,

    /// <summary>The signature algorithm is SHA-1 with RSA.</summary>
    Sha1Signature = 1,

    /// <summary>The signer's RSA modulus is under 2 048 bits. How far under does not cross.</summary>
    ShortRsaKey = 2,

    /// <summary>The signature covers an earlier revision, so later ones are outside it.</summary>
    CoversOnlyARevision = 3,

    /// <summary>The /ByteRange did not hold up.</summary>
    CoverageSuspicious = 4,

    /// <summary>A certificate's validity window does not contain the instant asked about.</summary>
    OutsideValidity = 5,
}

/// <summary>How a call went. These numbers are the ABI.</summary>
/// <remarks>
/// 0–7 are frozen, <see cref="NoSuchSignature"/> was appended at 8, and the
/// write surface's five at 9–13. A unit test in <c>tinker-pdf-ffi</c> pins
/// every one of them by number, because this enum is a hand transcription and
/// a reordered variant would compile on both sides and mean something
/// different on each.
/// </remarks>
public enum Status
{
    /// <summary>The call succeeded.</summary>
    Ok = 0,

    /// <summary>A pointer argument was null, or a length was nonsense.</summary>
    BadArgument = 1,

    /// <summary>The bytes are not a PDF.</summary>
    NotAPdf = 2,

    /// <summary>The document is encrypted and no password has been accepted.</summary>
    NeedsPassword = 3,

    /// <summary>The password did not match.</summary>
    WrongPassword = 4,

    /// <summary>The page index is past the end of the document.</summary>
    NoSuchPage = 5,

    /// <summary>The document is not encrypted, so there is nothing to authenticate.</summary>
    NotEncrypted = 6,

    /// <summary>The security handler is one this engine does not implement.</summary>
    UnsupportedHandler = 7,

    /// <summary>The signature index is past the last signature the document has.</summary>
    NoSuchSignature = 8,

    /// <summary>
    /// 12.7.3.2: no field carries that fully qualified name — or a field index
    /// is past the last field, which is the same sentence about a different
    /// way of asking.
    /// </summary>
    NoSuchField = 9,

    /// <summary>
    /// The field will not take that value: read-only (12.7.4.1 Table 227),
    /// longer than <c>/MaxLen</c>, or not offered by a non-editable list.
    /// Refusing beats truncating, which hides a data error inside a file that
    /// then looks correctly filled.
    /// </summary>
    ValueRefused = 10,

    /// <summary>
    /// The field's own object is not a dictionary, so there is nowhere to put
    /// <c>/V</c>. A damaged file rather than a rejected value.
    /// </summary>
    FieldUnreadable = 11,

    /// <summary>
    /// The handle was consumed by an earlier call, which the message names.
    /// <c>finish</c>, <c>push_page</c> and <c>add_child</c> consume in Rust;
    /// the handle stays live and stays yours to dispose, and asking it to work
    /// twice is this rather than a double free.
    /// </summary>
    SpentHandle = 12,

    /// <summary>
    /// The edit was refused, and the engine's own answer is a <c>bool</c>, so
    /// the reason does not cross. The message names the call and the argument;
    /// <see cref="Editor.PageCount"/> and <see cref="Editor.FieldCount"/> let
    /// you tell the bounds case apart before the call rather than after.
    /// </summary>
    EditRefused = 13,
}

/// <summary>What is wrong with a widget an appearance could not be written for.</summary>
public enum WidgetDefect
{
    /// <summary>
    /// 12.5.2 Table 164: <c>/Rect</c> is required for every annotation and
    /// this one has none, or none that is a usable rectangle.
    /// </summary>
    RectMissing = 0,
}

/// <summary>Which shape of output a save produces (7.5.6).</summary>
public enum WriteMode
{
    /// <summary>Emit every object afresh, renumbering from one.</summary>
    Rewrite = 0,

    /// <summary>
    /// Append changed objects to the original bytes, so the original survives
    /// as a prefix and a signature over it still covers what it covered
    /// (12.8.1).
    /// </summary>
    Incremental = 1,
}

/// <summary>How a destination positions the page it names (12.3.2.2 Table 151).</summary>
public enum DestKind
{
    /// <summary><c>/XYZ left top zoom</c>.</summary>
    Xyz = 0,

    /// <summary><c>/Fit</c>: fit the whole page.</summary>
    Fit = 1,

    /// <summary><c>/FitH top</c>: fit the width.</summary>
    FitH = 2,

    /// <summary><c>/FitV left</c>: fit the height.</summary>
    FitV = 3,

    /// <summary><c>/FitR left bottom right top</c>: fit a rectangle.</summary>
    FitR = 4,

    /// <summary><c>/FitB</c>: fit the bounding box of the page's contents.</summary>
    FitB = 5,

    /// <summary><c>/FitBH top</c>: fit the bounding box's width.</summary>
    FitBH = 6,

    /// <summary><c>/FitBV left</c>: fit the bounding box's height.</summary>
    FitBV = 7,
}

/// <summary>Which arm of the engine's image description a payload carries.</summary>
public enum ImageKind
{
    /// <summary>
    /// JPEG bytes, placed <b>as they are</b> and never re-encoded, because
    /// recompression is generational quality loss the caller cannot undo.
    /// Width and height are read from the bytes.
    /// </summary>
    Jpeg = 0,

    /// <summary>Eight-bit RGB, three bytes per pixel, row-major from the top.</summary>
    Rgb8 = 1,

    /// <summary>Eight-bit greyscale, one byte per pixel.</summary>
    Gray8 = 2,
}

/// <summary>Thrown when the engine reports a failure.</summary>
public sealed class PdfException : Exception
{
    internal PdfException(string message, Status status = Status.BadArgument)
        : base(message)
    {
        Status = status;
    }

    /// <summary>
    /// Which failure, as the C ABI numbered it.
    /// </summary>
    /// <remarks>
    /// Carried rather than folded into the message so a caller can branch on
    /// it — telling <see cref="Status.NoSuchField"/> from
    /// <see cref="Status.ValueRefused"/> is the difference between "your form
    /// changed" and "your data is wrong", and a string comparison is not a way
    /// to make that distinction.
    /// </remarks>
    public Status Status { get; }
}

internal static class Native
{
    private const string Library = "tinker_pdf_ffi";

    [DllImport(Library)]
    internal static extern IntPtr tpdf_last_error_message();

    [DllImport(Library)]
    internal static extern IntPtr tpdf_version();

    [DllImport(Library)]
    internal static extern int tpdf_document_open(byte[] bytes, nuint len, out IntPtr doc);

    [DllImport(Library)]
    internal static extern void tpdf_document_free(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_document_set_fonts(
        IntPtr doc,
        byte[]? regular, nuint regularLen,
        byte[]? bold, nuint boldLen,
        byte[]? italic, nuint italicLen,
        byte[]? boldItalic, nuint boldItalicLen);

    [DllImport(Library)]
    internal static extern uint tpdf_document_page_count(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_document_is_encrypted(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_document_authenticate(
        IntPtr doc, byte[] password, out int level);

    [DllImport(Library)]
    internal static extern int tpdf_document_may_print(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_page_size(
        IntPtr doc, uint index, out double width, out double height);

    [DllImport(Library)]
    internal static extern int tpdf_page_text(IntPtr doc, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern void tpdf_string_free(IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_page_render(
        IntPtr doc, uint index, double scale, int format, out IntPtr bitmap);

    [DllImport(Library)]
    internal static extern uint tpdf_bitmap_width(IntPtr bitmap);

    [DllImport(Library)]
    internal static extern uint tpdf_bitmap_height(IntPtr bitmap);

    [DllImport(Library)]
    internal static extern nuint tpdf_bitmap_stride(IntPtr bitmap);

    [DllImport(Library)]
    internal static extern IntPtr tpdf_bitmap_data(IntPtr bitmap, out nuint len);

    [DllImport(Library)]
    internal static extern void tpdf_bitmap_free(IntPtr bitmap);

    // ---- signatures, read (12.8) ------------------------------------------
    //
    // Reading only. `save_signed` and the `Signer` callback are not projected
    // through the C ABI at all, so there is nothing here to declare for them.

    [DllImport(Library)]
    internal static extern int tpdf_document_signatures(IntPtr doc, out IntPtr signatures);

    [DllImport(Library)]
    internal static extern uint tpdf_signatures_count(IntPtr signatures);

    [DllImport(Library)]
    internal static extern void tpdf_signatures_free(IntPtr signatures);

    [DllImport(Library)]
    internal static extern int tpdf_signature_field_name(
        IntPtr signatures, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_signature_sub_filter(
        IntPtr signatures, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_signature_reason(
        IntPtr signatures, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_signature_location(
        IntPtr signatures, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_signature_name(
        IntPtr signatures, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_signature_coverage(
        IntPtr signatures, uint index, out int coverage);

    [DllImport(Library)]
    internal static extern int tpdf_signature_covers_whole_file(IntPtr signatures, uint index);

    [DllImport(Library)]
    internal static extern int tpdf_signature_is_usage_rights(IntPtr signatures, uint index);

    [DllImport(Library)]
    internal static extern uint tpdf_signature_certification_level(IntPtr signatures, uint index);

    [DllImport(Library)]
    internal static extern uint tpdf_signature_span_count(IntPtr signatures, uint index);

    [DllImport(Library)]
    internal static extern int tpdf_signature_span(
        IntPtr signatures, uint index, uint span, out ulong start, out ulong length);

    [DllImport(Library)]
    internal static extern IntPtr tpdf_trust_anchors_new();

    [DllImport(Library)]
    internal static extern int tpdf_trust_anchors_add(IntPtr anchors, byte[] der, nuint len);

    [DllImport(Library)]
    internal static extern uint tpdf_trust_anchors_count(IntPtr anchors);

    [DllImport(Library)]
    internal static extern void tpdf_trust_anchors_free(IntPtr anchors);

    [DllImport(Library)]
    internal static extern int tpdf_document_verify_signatures(
        IntPtr doc, IntPtr anchors, int judgeValidity, long at, out IntPtr verdicts);

    [DllImport(Library)]
    internal static extern uint tpdf_verdicts_count(IntPtr verdicts);

    [DllImport(Library)]
    internal static extern void tpdf_verdicts_free(IntPtr verdicts);

    [DllImport(Library)]
    internal static extern int tpdf_verdict_cms_state(IntPtr verdicts, uint index, out int state);

    [DllImport(Library)]
    internal static extern int tpdf_verdict_document_digest(
        IntPtr verdicts, uint index, out int digest);

    [DllImport(Library)]
    internal static extern int tpdf_verdict_signature_check(
        IntPtr verdicts, uint index, out int check);

    [DllImport(Library)]
    internal static extern int tpdf_verdict_chain(IntPtr verdicts, uint index, out int chain);

    [DllImport(Library)]
    internal static extern int tpdf_verdict_signer_subject(
        IntPtr verdicts, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_verdict_signer_issuer(
        IntPtr verdicts, uint index, out IntPtr text);

    // `ref` rather than `out`, and the one place this file departs from the
    // shape of the others: this call returns a flag rather than a status, and
    // when the flag is 0 the engine writes nothing. An `out` would leave the
    // caller's slots holding whatever was on the stack while the compiler
    // believed them assigned; `ref` forces the caller to initialise them, so a
    // "there is no signer" answer reads back as the zeros it was given.
    [DllImport(Library)]
    internal static extern int tpdf_verdict_signer_validity(
        IntPtr verdicts, uint index, ref long notBefore, ref long notAfter);

    [DllImport(Library)]
    internal static extern uint tpdf_verdict_weakness_count(IntPtr verdicts, uint index);

    [DllImport(Library)]
    internal static extern int tpdf_verdict_weakness(
        IntPtr verdicts, uint index, uint weakness, out int value);

    internal static string? TakeString(IntPtr raw)
    {
        if (raw == IntPtr.Zero)
        {
            return null;
        }
        try
        {
            return Marshal.PtrToStringUTF8(raw);
        }
        finally
        {
            tpdf_string_free(raw);
        }
    }

    internal static void Check(int status)
    {
        if (status == 0)
        {
            return;
        }
        var pointer = tpdf_last_error_message();
        var message = pointer == IntPtr.Zero
            ? $"tinker-pdf error {status}"
            : Marshal.PtrToStringUTF8(pointer) ?? $"tinker-pdf error {status}";
        throw new PdfException(message, (Status)status);
    }

    // ---- the write surface (gap 32) --------------------------------------
    //
    // Transcribed by hand from `crates/tinker-pdf-ffi/src/lib.rs`, which is
    // the contract: the crate ships no generated header. A unit test on the
    // Rust side pins every enum discriminant these declarations name, because
    // a reordered variant would compile on both sides and mean something
    // different on each.
    //
    // Resource and font *names* cross as pointer+length rather than as
    // strings, matching the facade's `&[u8]`: a PDF name is bytes, and a
    // marshalled string would decide an encoding on the caller's behalf.

    [DllImport(Library)]
    internal static extern int tpdf_document_validate(IntPtr doc, out IntPtr defects);

    [DllImport(Library)]
    internal static extern uint tpdf_defects_count(IntPtr defects);

    [DllImport(Library)]
    internal static extern int tpdf_defect_rule(IntPtr defects, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_defect_message(IntPtr defects, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern void tpdf_defects_free(IntPtr defects);

    [DllImport(Library)]
    internal static extern int tpdf_write_options_init(out WriteOptionsRaw options);

    [DllImport(Library)]
    internal static extern int tpdf_document_editor(IntPtr doc, out IntPtr editor);

    [DllImport(Library)]
    internal static extern void tpdf_editor_free(IntPtr editor);

    [DllImport(Library)]
    internal static extern int tpdf_editor_is_dirty(IntPtr editor);

    [DllImport(Library)]
    internal static extern uint tpdf_editor_page_count(IntPtr editor);

    [DllImport(Library)]
    internal static extern int tpdf_editor_delete_page(IntPtr editor, uint index);

    [DllImport(Library)]
    internal static extern int tpdf_editor_move_page(IntPtr editor, uint from, uint to);

    [DllImport(Library)]
    internal static extern int tpdf_editor_rotate_page(IntPtr editor, uint index, long degrees);

    [DllImport(Library)]
    internal static extern int tpdf_editor_insert_page(
        IntPtr editor, uint index, double width, double height);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_crop_box(
        IntPtr editor, uint index, double x0, double y0, double x1, double y1);

    [DllImport(Library)]
    internal static extern int tpdf_editor_append_content(
        IntPtr editor, uint page, byte[] operators, nuint len);

    [DllImport(Library)]
    internal static extern uint tpdf_editor_field_count(IntPtr editor);

    [DllImport(Library)]
    internal static extern int tpdf_editor_field_name(IntPtr editor, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_editor_field_value(IntPtr editor, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_editor_fill_field(
        IntPtr editor, byte[] name, byte[] value, out IntPtr report);

    [DllImport(Library)]
    internal static extern int tpdf_editor_set_checkbox(IntPtr editor, byte[] name, int on);

    [DllImport(Library)]
    internal static extern int tpdf_editor_select_radio(
        IntPtr editor, byte[] name, byte[] option);

    [DllImport(Library)]
    internal static extern int tpdf_editor_checkpoint(IntPtr editor, out IntPtr checkpoint);

    [DllImport(Library)]
    internal static extern int tpdf_editor_restore(IntPtr editor, IntPtr checkpoint);

    [DllImport(Library)]
    internal static extern void tpdf_checkpoint_free(IntPtr checkpoint);

    [DllImport(Library)]
    internal static extern int tpdf_editor_save(
        IntPtr editor, ref WriteOptionsRaw options, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern IntPtr tpdf_buffer_data(IntPtr buffer, out nuint len);

    [DllImport(Library)]
    internal static extern nuint tpdf_buffer_len(IntPtr buffer);

    [DllImport(Library)]
    internal static extern void tpdf_buffer_free(IntPtr buffer);

    [DllImport(Library)]
    internal static extern uint tpdf_fill_report_count(IntPtr report);

    [DllImport(Library)]
    internal static extern int tpdf_fill_report_message(
        IntPtr report, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_fill_report_widget(
        IntPtr report, uint index, out uint number, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_fill_report_defect(IntPtr report, uint index, out int defect);

    [DllImport(Library)]
    internal static extern void tpdf_fill_report_free(IntPtr report);

    [DllImport(Library)]
    internal static extern int tpdf_builder_new(out IntPtr builder);

    [DllImport(Library)]
    internal static extern void tpdf_builder_free(IntPtr builder);

    [DllImport(Library)]
    internal static extern int tpdf_builder_add_base_font(
        IntPtr builder, byte[] resource, nuint resourceLen, byte[] baseFont, nuint baseFontLen);

    [DllImport(Library)]
    internal static extern int tpdf_builder_add_embedded_font(
        IntPtr builder,
        byte[] resource, nuint resourceLen,
        byte[] baseFont, nuint baseFontLen,
        byte[] program, nuint programLen);

    [DllImport(Library)]
    internal static extern int tpdf_builder_set_subset_fonts(IntPtr builder, int subset);

    [DllImport(Library)]
    internal static extern int tpdf_builder_add_image(
        IntPtr builder, byte[] resource, nuint resourceLen, ref ImageRaw image);

    [DllImport(Library)]
    internal static extern int tpdf_builder_set_info(
        IntPtr builder, byte[] key, nuint keyLen, byte[] value);

    [DllImport(Library)]
    internal static extern int tpdf_builder_begin_page(
        IntPtr builder, double width, double height, out IntPtr page);

    [DllImport(Library)]
    internal static extern int tpdf_builder_push_page(IntPtr builder, IntPtr page);

    [DllImport(Library)]
    internal static extern int tpdf_builder_set_outline(
        IntPtr builder, IntPtr[] entries, nuint count);

    [DllImport(Library)]
    internal static extern int tpdf_builder_finish(IntPtr builder, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern void tpdf_page_builder_free(IntPtr page);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_text(
        IntPtr page, byte[] font, nuint fontLen,
        double size, double x, double y, byte[] text);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_fill_rect(
        IntPtr page, double x, double y, double w, double h, double grey);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_image(
        IntPtr page, byte[] resource, nuint resourceLen,
        double x, double y, double w, double h);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_set_fill_rgb(
        IntPtr page, double r, double g, double b);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_set_stroke_rgb(
        IntPtr page, double r, double g, double b);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_set_crop_box(
        IntPtr page, double x0, double y0, double x1, double y1);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_raw(IntPtr page, byte[] operators, nuint len);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_link(
        IntPtr page, double x0, double y0, double x1, double y1, ref TargetRaw target);

    [DllImport(Library)]
    internal static extern int tpdf_destination_init_fit(out DestinationRaw destination);

    [DllImport(Library)]
    internal static extern int tpdf_outline_entry_new(byte[] title, out IntPtr entry);

    [DllImport(Library)]
    internal static extern int tpdf_outline_entry_set_target(IntPtr entry, ref TargetRaw target);

    [DllImport(Library)]
    internal static extern int tpdf_outline_entry_set_open(IntPtr entry, int open);

    [DllImport(Library)]
    internal static extern int tpdf_outline_entry_add_child(IntPtr parent, IntPtr child);

    [DllImport(Library)]
    internal static extern void tpdf_outline_entry_free(IntPtr entry);

    /// <summary>A null-terminated UTF-8 copy, which is what every string
    /// argument on this boundary is.</summary>
    internal static byte[] Utf8(string text) =>
        System.Text.Encoding.UTF8.GetBytes(text + '\0');
}

/// <summary>
/// <c>TpdfWriteOptions</c>, field for field.
/// </summary>
/// <remarks>
/// Sequential layout with every field an <c>int</c>, <c>uint</c> or pointer,
/// because the Rust side widened its booleans and version pair for exactly
/// this: a hand-written P/Invoke should have no packing to guess at.
///
/// Never zero one of these and use it. A zeroed struct is a <i>rewrite at
/// version 0.0</i>, which is not the engine's default and not a version any
/// reader knows; <see cref="WriteOptions.ToRaw"/> starts from
/// <c>tpdf_write_options_init</c> instead.
/// </remarks>
[StructLayout(LayoutKind.Sequential)]
internal struct WriteOptionsRaw
{
    internal int Mode;
    internal int Linearize;
    internal uint VersionMajor;
    internal uint VersionMinor;
    internal int ObjectStreams;
    internal int Compress;
    internal int GarbageCollect;
    internal IntPtr Encryption;
}

/// <summary><c>TpdfEncryption</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct EncryptionRaw
{
    internal IntPtr UserPassword;
    internal IntPtr OwnerPassword;
    internal int Permissions;
    internal IntPtr Entropy;
    internal nuint EntropyLen;
}

/// <summary><c>TpdfDestination</c>, field for field.</summary>
/// <remarks>
/// A component that is <c>double.NaN</c> is 12.3.2.2's <c>null</c> — "retain
/// the current value" — rather than a coordinate.
/// </remarks>
[StructLayout(LayoutKind.Sequential)]
internal struct DestinationRaw
{
    internal int Kind;
    internal double Left;
    internal double Bottom;
    internal double Right;
    internal double Top;
    internal double Zoom;
}

/// <summary><c>TpdfTarget</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct TargetRaw
{
    internal int Kind;
    internal uint PageIndex;
    internal DestinationRaw View;
    internal IntPtr Uri;
}

/// <summary><c>TpdfImage</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct ImageRaw
{
    internal int Kind;
    internal uint Width;
    internal uint Height;
    internal IntPtr Data;
    internal nuint DataLen;
}

internal sealed class DocumentHandle : SafeHandle
{
    internal DocumentHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_document_free(handle);
        return true;
    }
}

internal sealed class BitmapHandle : SafeHandle
{
    internal BitmapHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_bitmap_free(handle);
        return true;
    }
}

/// <summary>A rendered page.</summary>
public sealed class Bitmap : IDisposable
{
    private readonly BitmapHandle _handle;

    internal Bitmap(IntPtr raw)
    {
        _handle = new BitmapHandle();
        Marshal.InitHandle(_handle, raw);
    }

    /// <summary>Width in pixels.</summary>
    public uint Width => Native.tpdf_bitmap_width(_handle.DangerousGetHandle());

    /// <summary>Height in pixels.</summary>
    public uint Height => Native.tpdf_bitmap_height(_handle.DangerousGetHandle());

    /// <summary>Bytes per row.</summary>
    public nuint Stride => Native.tpdf_bitmap_stride(_handle.DangerousGetHandle());

    /// <summary>
    /// The pixels, as a span over the engine's own memory.
    /// </summary>
    /// <remarks>
    /// Zero-copy, and therefore only valid while this bitmap is alive. Copy it
    /// with <c>ToArray</c> if it must outlive the bitmap.
    /// </remarks>
    public ReadOnlySpan<byte> Pixels
    {
        get
        {
            var pointer = Native.tpdf_bitmap_data(_handle.DangerousGetHandle(), out var len);
            if (pointer == IntPtr.Zero)
            {
                return ReadOnlySpan<byte>.Empty;
            }
            unsafe
            {
                return new ReadOnlySpan<byte>(pointer.ToPointer(), checked((int)len));
            }
        }
    }

    /// <summary>Releases the bitmap.</summary>
    public void Dispose() => _handle.Dispose();
}


internal sealed class SignaturesHandle : SafeHandle
{
    internal SignaturesHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_signatures_free(handle);
        return true;
    }
}

internal sealed class TrustAnchorsHandle : SafeHandle
{
    internal TrustAnchorsHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_trust_anchors_free(handle);
        return true;
    }
}

internal sealed class VerdictsHandle : SafeHandle
{
    internal VerdictsHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_verdicts_free(handle);
        return true;
    }
}

/// <summary>An accessor that hands back an engine-allocated string or null.</summary>
internal delegate int StringAccessor(IntPtr handle, uint index, out IntPtr text);

/// <summary>Every digital signature a document carries (12.8), read.</summary>
/// <remarks>
/// Nothing here is verified; that is <see cref="Document.VerifySignatures"/>.
/// Each entry says what the file claims and what checking that claim against
/// the file established.
///
/// The reading is the engine's own copy, so this outlives the
/// <see cref="Document"/> it came from and is disposed independently.
///
/// Every accessor takes an index below <see cref="Count"/>; anything at or
/// past it throws <see cref="PdfException"/> rather than returning a
/// plausible-looking nothing.
/// </remarks>
public sealed class Signatures : IDisposable
{
    private readonly SignaturesHandle _handle;

    internal Signatures(IntPtr raw)
    {
        _handle = new SignaturesHandle();
        Marshal.InitHandle(_handle, raw);
    }

    /// <summary>How many signatures there are.</summary>
    public uint Count => Native.tpdf_signatures_count(_handle.DangerousGetHandle());

    /// <summary>
    /// The fully qualified name of the field holding the signature (12.7.3.2),
    /// or null when it was reached through the catalog's /Perms instead.
    /// </summary>
    public string? FieldName(uint index) => Text(Native.tpdf_signature_field_name, index);

    /// <summary>
    /// /SubFilter exactly as the document wrote it, recognised or not, or null
    /// when the dictionary has none.
    /// </summary>
    public string? SubFilter(uint index) => Text(Native.tpdf_signature_sub_filter, index);

    /// <summary>/Reason, or null.</summary>
    public string? Reason(uint index) => Text(Native.tpdf_signature_reason, index);

    /// <summary>/Location, or null.</summary>
    public string? Location(uint index) => Text(Native.tpdf_signature_location, index);

    /// <summary>
    /// /Name — who the signer <i>claims</i> to be, which is not the
    /// certificate's answer. That one is
    /// <see cref="Verdicts.SignerSubject"/>.
    /// </summary>
    public string? SignerName(uint index) => Text(Native.tpdf_signature_name, index);

    /// <summary>What the signature's /ByteRange covers, checked against the file.</summary>
    public Coverage CoverageOf(uint index)
    {
        Native.Check(Native.tpdf_signature_coverage(
            _handle.DangerousGetHandle(), index, out var value));
        return (Coverage)value;
    }

    /// <summary>Whether the signature covers every byte of the file.</summary>
    public bool CoversWholeFile(uint index) =>
        Native.tpdf_signature_covers_whole_file(_handle.DangerousGetHandle(), index) != 0;

    /// <summary>
    /// Whether this is a usage-rights signature (12.8.4), which grants a
    /// reader capabilities and makes <b>no claim about the document's
    /// content</b>.
    /// </summary>
    public bool IsUsageRights(uint index) =>
        Native.tpdf_signature_is_usage_rights(_handle.DangerousGetHandle(), index) != 0;

    /// <summary>
    /// The /DocMDP certification level (12.8.2.2): 1, 2 or 3, or 0 for none —
    /// which is what an ordinary approval signature reads as, and also what a
    /// /P value the specification does not define reads as.
    /// </summary>
    public uint CertificationLevel(uint index) =>
        Native.tpdf_signature_certification_level(_handle.DangerousGetHandle(), index);

    /// <summary>How many spans /ByteRange names. Zero when it was unreadable.</summary>
    public uint SpanCount(uint index) =>
        Native.tpdf_signature_span_count(_handle.DangerousGetHandle(), index);

    /// <summary>One span's start and length, as offsets into the file.</summary>
    public (ulong Start, ulong Length) Span(uint index, uint span)
    {
        Native.Check(Native.tpdf_signature_span(
            _handle.DangerousGetHandle(), index, span, out var start, out var length));
        return (start, length);
    }

    private string? Text(StringAccessor accessor, uint index)
    {
        Native.Check(accessor(_handle.DangerousGetHandle(), index, out var raw));
        return Native.TakeString(raw);
    }

    /// <summary>Releases the reading.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>Certificates the caller trusts, as DER.</summary>
/// <remarks>
/// Empty by default and never populated by the engine: there is no bundled
/// root store and there will not be one. A verification run against an empty
/// set reports <see cref="Chain.NoAnchors"/>, which is honest — without
/// something trusted to reach, a chain proves that a key signed something and
/// not whose key it was.
///
/// Anchors go in one at a time because <see cref="Add"/> refuses bytes that
/// are not a certificate at the moment they are offered; a single call taking
/// an array could only say that one of them was bad.
/// </remarks>
public sealed class TrustAnchors : IDisposable
{
    private readonly TrustAnchorsHandle _handle;

    /// <summary>An empty set.</summary>
    public TrustAnchors()
    {
        _handle = new TrustAnchorsHandle();
        Marshal.InitHandle(_handle, Native.tpdf_trust_anchors_new());
    }

    internal IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>How many anchors there are.</summary>
    public uint Count => Native.tpdf_trust_anchors_count(Raw);

    /// <summary>
    /// Adds one DER certificate. Bytes that do not parse throw
    /// <see cref="PdfException"/> and are not kept.
    /// </summary>
    public void Add(byte[] der)
    {
        ArgumentNullException.ThrowIfNull(der);
        Native.Check(Native.tpdf_trust_anchors_add(Raw, der, (nuint)der.Length));
    }

    /// <summary>Releases the anchor set.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>What every signature in a document turns out to prove (12.8).</summary>
/// <remarks>
/// One verdict per signature, in the order <see cref="Signatures"/> returns
/// them, so index <i>i</i> of one names the same signature as index <i>i</i> of
/// the other.
///
/// There is no "is it valid" here, deliberately. Reading a signature is four
/// independent questions — what it covers, whether the covered bytes still
/// hash to what was signed, whether the signature was made by the key in the
/// certificate, and how far that certificate's chain reaches — and any one of
/// them can hold while another does not.
/// </remarks>
public sealed class Verdicts : IDisposable
{
    private readonly VerdictsHandle _handle;

    internal Verdicts(IntPtr raw)
    {
        _handle = new VerdictsHandle();
        Marshal.InitHandle(_handle, raw);
    }

    /// <summary>How many verdicts there are.</summary>
    public uint Count => Native.tpdf_verdicts_count(_handle.DangerousGetHandle());

    /// <summary>Whether the CMS blob could be read.</summary>
    public CmsState CmsStateOf(uint index)
    {
        Native.Check(Native.tpdf_verdict_cms_state(
            _handle.DangerousGetHandle(), index, out var value));
        return (CmsState)value;
    }

    /// <summary>Whether the covered bytes still hash to what was signed.</summary>
    public DocumentDigest DocumentDigestOf(uint index)
    {
        Native.Check(Native.tpdf_verdict_document_digest(
            _handle.DangerousGetHandle(), index, out var value));
        return (DocumentDigest)value;
    }

    /// <summary>Whether the signature verifies against the signer's own key.</summary>
    public SignatureCheck SignatureCheckOf(uint index)
    {
        Native.Check(Native.tpdf_verdict_signature_check(
            _handle.DangerousGetHandle(), index, out var value));
        return (SignatureCheck)value;
    }

    /// <summary>How far the certificate chain reached.</summary>
    public Chain ChainOf(uint index)
    {
        Native.Check(Native.tpdf_verdict_chain(
            _handle.DangerousGetHandle(), index, out var value));
        return (Chain)value;
    }

    /// <summary>
    /// The signer certificate's subject, rendered per RFC 4514, or null when
    /// the verdict names no signer.
    /// </summary>
    public string? SignerSubject(uint index) => Text(Native.tpdf_verdict_signer_subject, index);

    /// <summary>The signer certificate's issuer, or null when there is no signer.</summary>
    public string? SignerIssuer(uint index) => Text(Native.tpdf_verdict_signer_issuer, index);

    /// <summary>
    /// The signer certificate's notBefore and notAfter as seconds since the
    /// Unix epoch, or null when the verdict names no signer.
    /// </summary>
    /// <remarks>
    /// A window, not a judgement. Whether an instant falls inside it is the
    /// caller's to decide, unless it asked
    /// <see cref="Document.VerifySignatures"/> to judge — in which case a miss
    /// is <see cref="Weakness.OutsideValidity"/>.
    /// </remarks>
    public (long NotBefore, long NotAfter)? SignerValidity(uint index)
    {
        long notBefore = 0;
        long notAfter = 0;
        var described = Native.tpdf_verdict_signer_validity(
            _handle.DangerousGetHandle(), index, ref notBefore, ref notAfter);
        return described != 0 ? (notBefore, notAfter) : null;
    }

    /// <summary>How many weaknesses the verdict names.</summary>
    public uint WeaknessCount(uint index) =>
        Native.tpdf_verdict_weakness_count(_handle.DangerousGetHandle(), index);

    /// <summary>One weakness by position.</summary>
    public Weakness WeaknessAt(uint index, uint weakness)
    {
        Native.Check(Native.tpdf_verdict_weakness(
            _handle.DangerousGetHandle(), index, weakness, out var value));
        return (Weakness)value;
    }

    private string? Text(StringAccessor accessor, uint index)
    {
        Native.Check(accessor(_handle.DangerousGetHandle(), index, out var raw));
        return Native.TakeString(raw);
    }

    /// <summary>Releases the verdicts.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>An open PDF document.</summary>
public sealed class Document : IDisposable
{
    private readonly DocumentHandle _handle;

    private Document(IntPtr raw)
    {
        _handle = new DocumentHandle();
        Marshal.InitHandle(_handle, raw);
    }

    /// <summary>The engine's version.</summary>
    public static string Version =>
        Marshal.PtrToStringUTF8(Native.tpdf_version()) ?? "unknown";

    /// <summary>Opens a document from bytes.</summary>
    public static Document Open(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        Native.Check(Native.tpdf_document_open(bytes, (nuint)bytes.Length, out var raw));
        return new Document(raw);
    }

    /// <summary>Opens a document from a file.</summary>
    public static Document OpenFile(string path) => Open(File.ReadAllBytes(path));

    /// <summary>The number of pages.</summary>
    public uint PageCount => Native.tpdf_document_page_count(_handle.DangerousGetHandle());

    /// <summary>Whether the document is encrypted.</summary>
    public bool IsEncrypted =>
        Native.tpdf_document_is_encrypted(_handle.DangerousGetHandle()) != 0;

    /// <summary>
    /// Whether the document permits printing.
    /// </summary>
    /// <remarks>
    /// PDF permissions are advisory: a document that says printing is denied
    /// is asking, not enforcing.
    /// </remarks>
    public bool MayPrint => Native.tpdf_document_may_print(_handle.DangerousGetHandle()) != 0;

    /// <summary>Tries a password, reporting which one matched.</summary>
    public AuthLevel Authenticate(string password)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(password + '\0');
        Native.Check(Native.tpdf_document_authenticate(
            _handle.DangerousGetHandle(), bytes, out var level));
        return (AuthLevel)level;
    }

    /// <summary>A page's size in points.</summary>
    public (double Width, double Height) PageSize(uint index)
    {
        Native.Check(Native.tpdf_page_size(
            _handle.DangerousGetHandle(), index, out var w, out var h));
        return (w, h);
    }

    /// <summary>A page's text.</summary>
    public string PageText(uint index)
    {
        Native.Check(Native.tpdf_page_text(_handle.DangerousGetHandle(), index, out var raw));
        try
        {
            return Marshal.PtrToStringUTF8(raw) ?? string.Empty;
        }
        finally
        {
            Native.tpdf_string_free(raw);
        }
    }

    /// <summary>
    /// Supplies a font for documents that embed none.
    /// </summary>
    /// <remarks>
    /// Without one, such a document extracts its text perfectly and draws none
    /// of it: the standard-14 metrics are built in, the outlines are not. The
    /// engine bundles no faces and reads no font directories, so a host that
    /// wants text drawn says where to find it — on .NET, usually a face read
    /// from the system font folder or shipped beside the application.
    ///
    /// The bytes are copied, so the arrays may be reused or collected after
    /// the call. <paramref name="regular"/> is required; the others fall back
    /// to it.
    /// </remarks>
    public void SetFonts(
        byte[] regular,
        byte[]? bold = null,
        byte[]? italic = null,
        byte[]? boldItalic = null)
    {
        ArgumentNullException.ThrowIfNull(regular);

        Native.Check(Native.tpdf_document_set_fonts(
            _handle.DangerousGetHandle(),
            regular, (nuint)regular.Length,
            bold, (nuint)(bold?.Length ?? 0),
            italic, (nuint)(italic?.Length ?? 0),
            boldItalic, (nuint)(boldItalic?.Length ?? 0)));
    }

    /// <summary>Renders a page. A scale of 1.0 is 72 dpi.</summary>
    public Bitmap Render(uint index, double scale = 1.0, PixelFormat format = PixelFormat.Rgb8)
    {
        Native.Check(Native.tpdf_page_render(
            _handle.DangerousGetHandle(), index, scale, (int)format, out var raw));
        return new Bitmap(raw);
    }


    /// <summary>The document's digital signatures (12.8), read.</summary>
    /// <remarks>
    /// Nothing here is verified; that is <see cref="VerifySignatures"/>. The
    /// result owns its own reading, so it may outlive this document.
    /// </remarks>
    public Signatures ReadSignatures()
    {
        Native.Check(Native.tpdf_document_signatures(
            _handle.DangerousGetHandle(), out var raw));
        return new Signatures(raw);
    }

    /// <summary>What every signature in this document turns out to prove (12.8).</summary>
    /// <remarks>
    /// <paramref name="anchors"/> are the certificates the <i>caller</i>
    /// trusts, and it is required: an empty <see cref="TrustAnchors"/> is how a
    /// caller says it trusts nothing, and passing null instead would be this
    /// binding inventing a default the engine has not got.
    ///
    /// <paramref name="at"/> is the instant to judge certificate validity at,
    /// in seconds since the Unix epoch. Null — the default — reports the
    /// validity windows and judges nothing, because "expired" is a claim about
    /// now and a library that invents a now gives a different answer on a
    /// different day for the same bytes.
    /// </remarks>
    public Verdicts VerifySignatures(TrustAnchors anchors, long? at = null)
    {
        ArgumentNullException.ThrowIfNull(anchors);
        Native.Check(Native.tpdf_document_verify_signatures(
            _handle.DangerousGetHandle(),
            anchors.Raw,
            at.HasValue ? 1 : 0,
            at ?? 0,
            out var raw));
        return new Verdicts(raw);
    }

    /// <summary>
    /// The strict structural validator's findings, as rule names (ruling 13).
    /// </summary>
    /// <remarks>
    /// An empty array is a clean document. This is the check that keeps four
    /// byte-identical outputs from being identically wrong: the write-parity
    /// suite compares four surfaces' bytes to each other, and agreement alone
    /// would be satisfied by four copies of a broken file.
    /// </remarks>
    public string[] Validate()
    {
        Native.Check(Native.tpdf_document_validate(
            _handle.DangerousGetHandle(), out var raw));
        try
        {
            var count = Native.tpdf_defects_count(raw);
            var found = new string[count];
            for (uint i = 0; i < count; i++)
            {
                Native.Check(Native.tpdf_defect_rule(raw, i, out var text));
                found[i] = Native.TakeString(text) ?? string.Empty;
            }
            return found;
        }
        finally
        {
            Native.tpdf_defects_free(raw);
        }
    }

    /// <summary>An editor over this document.</summary>
    /// <remarks>
    /// Independent of this <see cref="Document"/>: the engine's editor holds
    /// its own reference to the shared object store, so disposing the document
    /// first is legal and the editor still saves correctly. That is why
    /// <c>EditorHandle</c> needs no keep-alive on its parent — and it is
    /// asserted by the smoke test rather than left inferred.
    /// </remarks>
    public Editor CreateEditor()
    {
        Native.Check(Native.tpdf_document_editor(_handle.DangerousGetHandle(), out var raw));
        return new Editor(raw);
    }

    /// <summary>Releases the document.</summary>
    public void Dispose() => _handle.Dispose();
}

// ---- writing (gap 32 milestone 5) -----------------------------------------
//
// Five more SafeHandles, on the DocumentHandle/BitmapHandle pattern exactly:
// a native handle is released once even if an exception unwinds past it, and
// a finalizer releases one the caller never disposed.

internal sealed class EditorHandle : SafeHandle
{
    internal EditorHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_editor_free(handle);
        return true;
    }
}

internal sealed class CheckpointHandle : SafeHandle
{
    internal CheckpointHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_checkpoint_free(handle);
        return true;
    }
}

internal sealed class BufferHandle : SafeHandle
{
    internal BufferHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_buffer_free(handle);
        return true;
    }
}

internal sealed class BuilderHandle : SafeHandle
{
    internal BuilderHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_builder_free(handle);
        return true;
    }
}

internal sealed class PageBuilderHandle : SafeHandle
{
    internal PageBuilderHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_page_builder_free(handle);
        return true;
    }
}

internal sealed class OutlineEntryHandle : SafeHandle
{
    internal OutlineEntryHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_outline_entry_free(handle);
        return true;
    }
}

/// <summary>A widget a fill wrote a value for and could not draw.</summary>
/// <remarks>
/// <b>The fourth outcome.</b> A fill has three answers, not two: it throws
/// when nothing was written, returns an empty array when the value was written
/// and every widget drawn, and returns a non-empty one when the value was
/// written and these widgets were left showing whatever they showed before,
/// because 12.5.2's required <c>/Rect</c> is missing from them. Ruling 2
/// degrades rather than failing; ruling 10 makes the degradation name the
/// object it happened to.
/// </remarks>
public sealed record SkippedWidget(uint ObjectNumber, ushort Generation, WidgetDefect Reason, string Message)
{
    /// <summary>The engine's own wording, so .NET and Rust say the same
    /// sentence about the same document.</summary>
    public override string ToString() => Message;
}

/// <summary>An editor's state, taken as a value.</summary>
/// <remarks>
/// Not an open transaction: taking one changes nothing, disposing one commits
/// nothing because nothing was pending, and <see cref="Editor.Restore"/> is
/// idempotent — which is what a <c>finally</c> running after its own
/// <c>catch</c> needs.
/// </remarks>
public sealed class Checkpoint : IDisposable
{
    private readonly CheckpointHandle _handle;

    internal Checkpoint(IntPtr raw)
    {
        _handle = new CheckpointHandle();
        Marshal.InitHandle(_handle, raw);
    }

    internal IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Releases the checkpoint. Nothing is committed.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>How to encrypt on save.</summary>
/// <remarks>
/// <paramref name="Entropy"/> is <b>48 caller-supplied bytes</b> — the 32-byte
/// file key and two 8-byte salts. There is no default and there will not be
/// one: this engine has no opinion about where randomness comes from, and a
/// binding that invented one would violate ruling 11 and hide the single input
/// that makes encrypted output non-reproducible.
/// </remarks>
public sealed record Encryption(
    string UserPassword,
    string OwnerPassword,
    int Permissions,
    byte[] Entropy);

/// <summary>Options for writing, starting from the engine's own defaults.</summary>
/// <remarks>
/// A binding invents no defaults (ruling 11), so a new instance is whatever
/// <c>tpdf_write_options_init</c> fills in and every property overrides one
/// field of it. A .NET caller who sets nothing writes the file a Rust caller
/// who sets nothing writes, byte for byte.
/// </remarks>
public sealed class WriteOptions
{
    /// <summary>Rewrite or incremental (7.5.6).</summary>
    public WriteMode Mode { get; set; } = WriteMode.Rewrite;

    /// <summary>Lay the file out for the first page to arrive first (Annex F).
    /// A request rather than a guarantee.</summary>
    public bool? Linearize { get; set; }

    /// <summary>The PDF version to declare in the header, on a rewrite.</summary>
    public (uint Major, uint Minor)? Version { get; set; }

    /// <summary>Pack eligible objects into object streams (7.5.7).</summary>
    public bool? ObjectStreams { get; set; }

    /// <summary>Compress content streams the caller has not already encoded.</summary>
    public bool? Compress { get; set; }

    /// <summary>Drop objects nothing reaches from the trailer, on a rewrite.</summary>
    public bool? GarbageCollect { get; set; }

    /// <summary>Encrypt on save, or null for a plain file.</summary>
    public Encryption? Encryption { get; set; }

    /// <summary>
    /// The engine's defaults with this object's overrides applied.
    /// </summary>
    /// <remarks>
    /// Starts from <c>tpdf_write_options_init</c> rather than from a zeroed
    /// struct, because a zeroed one is a rewrite at version 0.0 — not the
    /// engine's default and not a version any reader knows.
    /// </remarks>
    internal WriteOptionsRaw ToRaw()
    {
        Native.Check(Native.tpdf_write_options_init(out var raw));
        raw.Mode = (int)Mode;
        if (Linearize.HasValue) raw.Linearize = Linearize.Value ? 1 : 0;
        if (Version.HasValue)
        {
            raw.VersionMajor = Version.Value.Major;
            raw.VersionMinor = Version.Value.Minor;
        }
        if (ObjectStreams.HasValue) raw.ObjectStreams = ObjectStreams.Value ? 1 : 0;
        if (Compress.HasValue) raw.Compress = Compress.Value ? 1 : 0;
        if (GarbageCollect.HasValue) raw.GarbageCollect = GarbageCollect.Value ? 1 : 0;
        return raw;
    }
}

/// <summary>Edits layered over an open document.</summary>
/// <remarks>
/// Independent of the <see cref="Document"/> it came from, so the document may
/// be disposed first.
///
/// <b>Not thread-safe.</b> The read surface is, because every read borrows an
/// immutable shared document; an editor is mutable state, so two threads in
/// one instance is a data race no wrapper can prevent. One per thread, or your
/// own lock.
/// </remarks>
public sealed class Editor : IDisposable
{
    private readonly EditorHandle _handle;

    internal Editor(IntPtr raw)
    {
        _handle = new EditorHandle();
        Marshal.InitHandle(_handle, raw);
    }

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Whether anything has been changed.</summary>
    public bool IsDirty => Native.tpdf_editor_is_dirty(Raw) != 0;

    /// <summary>
    /// How many pages the document has as this editor sees it, which is not
    /// the document's own count once a page has been inserted or deleted here.
    /// </summary>
    public uint PageCount => Native.tpdf_editor_page_count(Raw);

    /// <summary>How many form fields the document has.</summary>
    public uint FieldCount => Native.tpdf_editor_field_count(Raw);

    /// <summary>A field's fully qualified name (12.7.3.2).</summary>
    public string FieldName(uint index)
    {
        Native.Check(Native.tpdf_editor_field_name(Raw, index, out var raw));
        return Native.TakeString(raw) ?? string.Empty;
    }

    /// <summary>A field's current value as text, empty when it has none.</summary>
    public string FieldValue(uint index)
    {
        Native.Check(Native.tpdf_editor_field_value(Raw, index, out var raw));
        return Native.TakeString(raw) ?? string.Empty;
    }

    /// <summary>Removes a page.</summary>
    public void DeletePage(uint index) => Native.Check(Native.tpdf_editor_delete_page(Raw, index));

    /// <summary>Moves a page to a new position.</summary>
    public void MovePage(uint from, uint to) =>
        Native.Check(Native.tpdf_editor_move_page(Raw, from, to));

    /// <summary>Rotates a page by a quarter-turn multiple, relative to its
    /// current rotation.</summary>
    public void RotatePage(uint index, long degrees) =>
        Native.Check(Native.tpdf_editor_rotate_page(Raw, index, degrees));

    /// <summary>Inserts a blank page at <paramref name="index"/>, which may
    /// equal the page count to append.</summary>
    public void InsertPage(uint index, double width, double height) =>
        Native.Check(Native.tpdf_editor_insert_page(Raw, index, width, height));

    /// <summary>Sets a page's <c>/CropBox</c> (14.11.2).</summary>
    public void SetCropBox(uint index, double x0, double y0, double x1, double y1) =>
        Native.Check(Native.tpdf_editor_set_crop_box(Raw, index, x0, y0, x1, y1));

    /// <summary>Appends operators to a page's content stream.</summary>
    public void AppendContent(uint page, byte[] operators)
    {
        ArgumentNullException.ThrowIfNull(operators);
        Native.Check(Native.tpdf_editor_append_content(
            Raw, page, operators, (nuint)operators.Length));
    }

    /// <summary>
    /// Fills a text or choice field, returning the widgets it could not draw.
    /// </summary>
    /// <remarks>
    /// Throws when <b>nothing</b> was written; returns an array — empty or not
    /// — when the value was written. See <see cref="SkippedWidget"/> for why
    /// an empty one and a non-empty one are both successes.
    /// </remarks>
    public SkippedWidget[] FillField(string name, string value)
    {
        Native.Check(Native.tpdf_editor_fill_field(
            Raw, Native.Utf8(name), Native.Utf8(value), out var report));
        if (report == IntPtr.Zero)
        {
            return Array.Empty<SkippedWidget>();
        }
        try
        {
            var count = Native.tpdf_fill_report_count(report);
            var skipped = new SkippedWidget[count];
            for (uint i = 0; i < count; i++)
            {
                Native.Check(Native.tpdf_fill_report_widget(
                    report, i, out var number, out var generation));
                Native.Check(Native.tpdf_fill_report_defect(report, i, out var defect));
                Native.Check(Native.tpdf_fill_report_message(report, i, out var text));
                skipped[i] = new SkippedWidget(
                    number, generation, (WidgetDefect)defect,
                    Native.TakeString(text) ?? string.Empty);
            }
            return skipped;
        }
        finally
        {
            Native.tpdf_fill_report_free(report);
        }
    }

    /// <summary>Ticks or clears a checkbox.</summary>
    public void SetCheckbox(string name, bool on) =>
        Native.Check(Native.tpdf_editor_set_checkbox(Raw, Native.Utf8(name), on ? 1 : 0));

    /// <summary>Selects one option of a radio group (12.7.4.2).</summary>
    public void SelectRadio(string name, string option) =>
        Native.Check(Native.tpdf_editor_select_radio(
            Raw, Native.Utf8(name), Native.Utf8(option)));

    /// <summary>Takes this editor's state as a value, for
    /// <see cref="Restore"/> to put back.</summary>
    public Checkpoint Checkpoint()
    {
        Native.Check(Native.tpdf_editor_checkpoint(Raw, out var raw));
        return new Checkpoint(raw);
    }

    /// <summary>Puts this editor back to what a checkpoint recorded.</summary>
    /// <remarks>
    /// Idempotent: restoring twice is restoring once. The checkpoint is
    /// borrowed rather than consumed, so one can undo several attempts.
    /// </remarks>
    public void Restore(Checkpoint checkpoint)
    {
        ArgumentNullException.ThrowIfNull(checkpoint);
        Native.Check(Native.tpdf_editor_restore(Raw, checkpoint.Raw));
    }

    /// <summary>Runs <paramref name="body"/> as one edit: it lands together or
    /// not at all.</summary>
    /// <remarks>
    /// <b>Nothing but checkpoint, host-language control flow, restore.</b> The
    /// semantics is the engine's — the same two calls its own
    /// <c>transaction</c> makes — and C# supplies only the <c>try</c>. An
    /// exception is restored from and then <i>rethrown</i>: a rollback that
    /// also hid the reason would be the worst of both.
    /// </remarks>
    public void Transaction(Action body)
    {
        ArgumentNullException.ThrowIfNull(body);
        using var mark = Checkpoint();
        try
        {
            body();
        }
        catch
        {
            Restore(mark);
            throw;
        }
    }

    /// <summary>Saves the edited document.</summary>
    public byte[] Save(WriteOptions? options = null)
    {
        var raw = (options ?? new WriteOptions()).ToRaw();
        var encryption = options?.Encryption;
        if (encryption is null)
        {
            return SaveWith(ref raw);
        }

        ArgumentNullException.ThrowIfNull(encryption.Entropy);
        // Pinned by hand rather than marshalled: the engine borrows these for
        // the duration of the call and copies out of them before returning, so
        // they only have to outlive the call — but they do have to.
        var user = Native.Utf8(encryption.UserPassword);
        var owner = Native.Utf8(encryption.OwnerPassword);
        var entropy = encryption.Entropy;
        unsafe
        {
            fixed (byte* userPtr = user)
            fixed (byte* ownerPtr = owner)
            fixed (byte* entropyPtr = entropy)
            {
                var block = new EncryptionRaw
                {
                    UserPassword = (IntPtr)userPtr,
                    OwnerPassword = (IntPtr)ownerPtr,
                    Permissions = encryption.Permissions,
                    Entropy = (IntPtr)entropyPtr,
                    EntropyLen = (nuint)entropy.Length,
                };
                raw.Encryption = (IntPtr)(&block);
                return SaveWith(ref raw);
            }
        }
    }

    private byte[] SaveWith(ref WriteOptionsRaw raw)
    {
        Native.Check(Native.tpdf_editor_save(Raw, ref raw, out var buffer));
        return Buffers.Take(buffer);
    }

    /// <summary>Releases the editor. Pending edits are discarded; nothing is
    /// written to any document.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>Copying an engine buffer out and releasing it.</summary>
internal static class Buffers
{
    /// <summary>The bytes, copied, with the handle released either way.</summary>
    internal static byte[] Take(IntPtr buffer)
    {
        var handle = new BufferHandle();
        Marshal.InitHandle(handle, buffer);
        using (handle)
        {
            var pointer = Native.tpdf_buffer_data(handle.DangerousGetHandle(), out var len);
            if (pointer == IntPtr.Zero)
            {
                return Array.Empty<byte>();
            }
            var bytes = new byte[checked((int)len)];
            Marshal.Copy(pointer, bytes, 0, bytes.Length);
            return bytes;
        }
    }
}

/// <summary>A page being drawn, owned until it is pushed.</summary>
/// <remarks>
/// Born from a <see cref="DocumentBuilder"/> or not at all: there is no public
/// constructor, because a page whose resource names were never resolved
/// against a builder is a page whose names mean nothing.
///
/// A page begun and never pushed is simply disposed, and the document is
/// byte-for-byte what it would have been.
/// </remarks>
public sealed class PageBuilder : IDisposable
{
    private readonly PageBuilderHandle _handle;

    internal PageBuilder(IntPtr raw)
    {
        _handle = new PageBuilderHandle();
        Marshal.InitHandle(_handle, raw);
    }

    internal IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Draws text with a registered font.</summary>
    public void Text(byte[] font, double size, double x, double y, string text)
    {
        ArgumentNullException.ThrowIfNull(font);
        Native.Check(Native.tpdf_page_builder_text(
            Raw, font, (nuint)font.Length, size, x, y, Native.Utf8(text)));
    }

    /// <summary>Fills a rectangle in device grey, from black (0) to white (1).</summary>
    public void FillRect(double x, double y, double w, double h, double grey) =>
        Native.Check(Native.tpdf_page_builder_fill_rect(Raw, x, y, w, h, grey));

    /// <summary>Draws a registered image into the given rectangle.</summary>
    public void Image(byte[] resource, double x, double y, double w, double h)
    {
        ArgumentNullException.ThrowIfNull(resource);
        Native.Check(Native.tpdf_page_builder_image(
            Raw, resource, (nuint)resource.Length, x, y, w, h));
    }

    /// <summary>Sets the non-stroking colour.</summary>
    public void SetFillRgb(double r, double g, double b) =>
        Native.Check(Native.tpdf_page_builder_set_fill_rgb(Raw, r, g, b));

    /// <summary>Sets the <b>stroking</b> colour. <c>RG</c>, not <c>rg</c>.</summary>
    public void SetStrokeRgb(double r, double g, double b) =>
        Native.Check(Native.tpdf_page_builder_set_stroke_rgb(Raw, r, g, b));

    /// <summary>Sets this page's <c>/CropBox</c> (7.7.3.3).</summary>
    public void SetCropBox(double x0, double y0, double x1, double y1) =>
        Native.Check(Native.tpdf_page_builder_set_crop_box(Raw, x0, y0, x1, y1));

    /// <summary>Appends content-stream operators verbatim.</summary>
    public void Raw_(byte[] operators)
    {
        ArgumentNullException.ThrowIfNull(operators);
        Native.Check(Native.tpdf_page_builder_raw(Raw, operators, (nuint)operators.Length));
    }

    /// <summary>Adds a link annotation over a rectangle, to a page in this
    /// document (12.5.6.5).</summary>
    public void LinkToPage(double x0, double y0, double x1, double y1, uint page)
    {
        Native.Check(Native.tpdf_destination_init_fit(out var view));
        var target = new TargetRaw
        {
            Kind = 0,
            PageIndex = page,
            View = view,
            Uri = IntPtr.Zero,
        };
        Native.Check(Native.tpdf_page_builder_link(Raw, x0, y0, x1, y1, ref target));
    }

    /// <summary>Releases the page. One never pushed leaves no trace.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>One outline entry under construction (12.3.3).</summary>
/// <remarks>
/// A tree built by handles rather than described by an object graph, because
/// the nesting is what a flat description cannot carry.
/// <see cref="AddChild"/> and <see cref="DocumentBuilder.SetOutline"/> each
/// <b>consume</b> what they take; the handle stays yours to dispose, and using
/// it again is <see cref="Status.SpentHandle"/> rather than two copies of one
/// entry.
/// </remarks>
public sealed class OutlineEntry : IDisposable
{
    private readonly OutlineEntryHandle _handle;

    /// <summary>An entry with a title and no destination.</summary>
    /// <remarks>
    /// 12.3.3 makes <c>/Dest</c> optional, and an entry without one is a real
    /// shape rather than a degraded one: a part title above three chapters
    /// often points nowhere itself.
    /// </remarks>
    public OutlineEntry(string title)
    {
        Native.Check(Native.tpdf_outline_entry_new(Native.Utf8(title), out var raw));
        _handle = new OutlineEntryHandle();
        Marshal.InitHandle(_handle, raw);
    }

    internal IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Points the entry at a page in this document.</summary>
    public void SetPageTarget(uint index)
    {
        Native.Check(Native.tpdf_destination_init_fit(out var view));
        var target = new TargetRaw
        {
            Kind = 0,
            PageIndex = index,
            View = view,
            Uri = IntPtr.Zero,
        };
        Native.Check(Native.tpdf_outline_entry_set_target(Raw, ref target));
    }

    /// <summary>Whether the entry is shown expanded. Ignored for an entry with
    /// no children, which 12.3.3 leaves neither open nor closed.</summary>
    public void SetOpen(bool open) =>
        Native.Check(Native.tpdf_outline_entry_set_open(Raw, open ? 1 : 0));

    /// <summary>Nests one entry under another, <b>consuming</b> the child.</summary>
    public void AddChild(OutlineEntry child)
    {
        ArgumentNullException.ThrowIfNull(child);
        Native.Check(Native.tpdf_outline_entry_add_child(Raw, child.Raw));
    }

    /// <summary>Releases the entry.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>Assembles a document from pages, fonts and images.</summary>
public sealed class DocumentBuilder : IDisposable
{
    private readonly BuilderHandle _handle;

    /// <summary>Starts a document.</summary>
    public DocumentBuilder()
    {
        Native.Check(Native.tpdf_builder_new(out var raw));
        _handle = new BuilderHandle();
        Marshal.InitHandle(_handle, raw);
    }

    private IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Registers one of the standard 14 fonts under a resource name
    /// (9.6.2.2).</summary>
    public void AddBaseFont(byte[] resource, byte[] baseFont)
    {
        ArgumentNullException.ThrowIfNull(resource);
        ArgumentNullException.ThrowIfNull(baseFont);
        Native.Check(Native.tpdf_builder_add_base_font(
            Raw, resource, (nuint)resource.Length, baseFont, (nuint)baseFont.Length));
    }

    /// <summary>Embeds a TrueType or CFF font program under a resource name.</summary>
    public void AddEmbeddedFont(byte[] resource, byte[] baseFont, byte[] program)
    {
        ArgumentNullException.ThrowIfNull(resource);
        ArgumentNullException.ThrowIfNull(baseFont);
        ArgumentNullException.ThrowIfNull(program);
        Native.Check(Native.tpdf_builder_add_embedded_font(
            Raw,
            resource, (nuint)resource.Length,
            baseFont, (nuint)baseFont.Length,
            program, (nuint)program.Length));
    }

    /// <summary>Whether embedded fonts are subsetted to the glyphs actually
    /// drawn.</summary>
    public void SetSubsetFonts(bool subset) =>
        Native.Check(Native.tpdf_builder_set_subset_fonts(Raw, subset ? 1 : 0));

    /// <summary>Registers an image under a resource name.</summary>
    /// <remarks>
    /// JPEG bytes are placed as they are and never re-encoded, so
    /// <paramref name="width"/> and <paramref name="height"/> are read from the
    /// bytes and ignored for <see cref="ImageKind.Jpeg"/>.
    /// </remarks>
    public void AddImage(byte[] resource, byte[] data, ImageKind kind, uint width, uint height)
    {
        ArgumentNullException.ThrowIfNull(resource);
        ArgumentNullException.ThrowIfNull(data);
        unsafe
        {
            fixed (byte* dataPtr = data)
            {
                var image = new ImageRaw
                {
                    Kind = (int)kind,
                    Width = width,
                    Height = height,
                    Data = (IntPtr)dataPtr,
                    DataLen = (nuint)data.Length,
                };
                Native.Check(Native.tpdf_builder_add_image(
                    Raw, resource, (nuint)resource.Length, ref image));
            }
        }
    }

    /// <summary>Sets an <c>/Info</c> field, such as <c>Title</c>.</summary>
    public void SetInfo(byte[] key, string value)
    {
        ArgumentNullException.ThrowIfNull(key);
        Native.Check(Native.tpdf_builder_set_info(
            Raw, key, (nuint)key.Length, Native.Utf8(value)));
    }

    /// <summary>Sets the document outline from top-level entries,
    /// <b>consuming</b> each (12.3.3).</summary>
    public void SetOutline(params OutlineEntry[] entries)
    {
        ArgumentNullException.ThrowIfNull(entries);
        var raws = new IntPtr[entries.Length];
        for (var i = 0; i < entries.Length; i++)
        {
            raws[i] = entries[i].Raw;
        }
        Native.Check(Native.tpdf_builder_set_outline(Raw, raws, (nuint)raws.Length));
    }

    /// <summary>Starts a page, owned by the caller until
    /// <see cref="PushPage"/> takes it.</summary>
    /// <remarks>
    /// <b>The resource snapshot happens here.</b> A font or image registered
    /// after this call is invisible to this page — the same timing the engine's
    /// own closure form imposes, because it calls this.
    /// </remarks>
    public PageBuilder BeginPage(double width, double height)
    {
        Native.Check(Native.tpdf_builder_begin_page(Raw, width, height, out var raw));
        return new PageBuilder(raw);
    }

    /// <summary>Adds a page the caller has finished drawing,
    /// <b>consuming</b> it.</summary>
    /// <remarks>
    /// A second push of the same page is <see cref="Status.SpentHandle"/>
    /// rather than a second page. Disposing it is still required and still
    /// safe.
    /// </remarks>
    public void PushPage(PageBuilder page)
    {
        ArgumentNullException.ThrowIfNull(page);
        Native.Check(Native.tpdf_builder_push_page(Raw, page.Raw));
    }

    /// <summary>Finishes the document, <b>consuming</b> the builder.</summary>
    /// <remarks>
    /// A second call is <see cref="Status.SpentHandle"/> rather than a second
    /// document. Disposing is still required and still safe.
    /// </remarks>
    public byte[] Finish()
    {
        Native.Check(Native.tpdf_builder_finish(Raw, out var buffer));
        return Buffers.Take(buffer);
    }

    /// <summary>Releases the builder.</summary>
    public void Dispose() => _handle.Dispose();
}
