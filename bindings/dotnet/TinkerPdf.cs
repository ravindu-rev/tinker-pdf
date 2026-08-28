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

/// <summary>Thrown when the engine reports a failure.</summary>
public sealed class PdfException : Exception
{
    internal PdfException(string message) : base(message) { }
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
        throw new PdfException(message);
    }
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

    /// <summary>Releases the document.</summary>
    public void Dispose() => _handle.Dispose();
}
