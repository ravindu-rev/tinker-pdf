package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.ADDRESS;
import static io.github.ravindu_rev.tinkerpdf.Native.DOUBLE;
import static io.github.ravindu_rev.tinkerpdf.Native.INT;
import static io.github.ravindu_rev.tinkerpdf.Native.LONG;
import static io.github.ravindu_rev.tinkerpdf.Native.SHORT;

import io.github.ravindu_rev.tinkerpdf.TinkerPdf.ActionKind;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.AuthLevel;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Chain;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CmsState;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Coverage;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Defect;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Destination;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.DocumentDigest;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.InfoKey;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Link;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.OutlineItem;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.PageBoundary;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.PixelFormat;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Rect;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Ref;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Signature;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.SignatureCheck;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Span;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Trapped;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Verdict;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Warning;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Weakness;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandle;
import java.util.ArrayList;
import java.util.List;

/** An open PDF. */
public final class Document implements AutoCloseable {
    private MemorySegment pointer;
    /** The upcall stubs of a streamed document, released after the document. */
    private final Arena streams;

    private Document(MemorySegment pointer, Arena streams) {
        this.pointer = pointer;
        this.streams = streams;
    }

    /** Opens a document from bytes, which are copied. */
    public static Document open(byte[] data) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, ADDRESS);
            Native.check(Native.tpdf_document_open, Native.bytes(arena, data), (long) data.length, out);
            return new Document(out.get(ADDRESS, 0), null);
        }
    }

    /**
     * Opens a document whose bytes come from {@code source}. The source is
     * released when the document is closed, or at once if the open fails.
     */
    public static Document openStreaming(Source source) {
        Arena streams = Arena.ofShared();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment vtable = Source.vtable(source, streams, arena);
            MemorySegment out = Native.slot(arena, ADDRESS);
            Native.check(Native.tpdf_document_open_streaming, vtable, MemorySegment.NULL, out);
            return new Document(out.get(ADDRESS, 0), streams);
        } catch (RuntimeException | Error e) {
            streams.close();
            throw e;
        }
    }

    MemorySegment pointer() {
        return pointer;
    }

    /** Releases the document. Handles taken from it stay valid. */
    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_document_free, pointer);
            pointer = null;
            if (streams != null) {
                streams.close();
            }
        }
    }

    public int pageCount() {
        return Native.callInt(Native.tpdf_document_page_count, pointer);
    }

    public boolean isStreamed() {
        return Native.callInt(Native.tpdf_document_is_streamed, pointer) != 0;
    }

    public boolean isEncrypted() {
        return Native.callInt(Native.tpdf_document_is_encrypted, pointer) != 0;
    }

    /** Whether the document permits printing at the level reached. PDF permissions are advisory. */
    public boolean mayPrint() {
        return Native.callInt(Native.tpdf_document_may_print, pointer) != 0;
    }

    /** Tries a password; a wrong one throws WRONG_PASSWORD, an unencrypted document NOT_ENCRYPTED. */
    public AuthLevel authenticate(String password) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment level = Native.slot(arena, INT);
            Native.check(Native.tpdf_document_authenticate, pointer, Native.cString(arena, password), level);
            return AuthLevel.values()[level.get(INT, 0)];
        }
    }

    /** {width, height} in points. */
    public double[] pageSize(int index) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment width = Native.slot(arena, DOUBLE);
            MemorySegment height = Native.slot(arena, DOUBLE);
            Native.check(Native.tpdf_page_size, pointer, index, width, height);
            return new double[] {width.get(DOUBLE, 0), height.get(DOUBLE, 0)};
        }
    }

    /** A page's text, in logical order (ruling 14). */
    public String pageText(int index) {
        return text(Native.tpdf_page_text, pointer, index);
    }

    /** The face a document that embeds none is drawn with. The engine bundles none. */
    public void setFonts(byte[] regular) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_document_set_fonts, pointer, Native.bytes(arena, regular), (long) regular.length,
                    MemorySegment.NULL, 0L, MemorySegment.NULL, 0L, MemorySegment.NULL, 0L);
        }
    }

    /** Draws a page at a scale, 1.0 being 72 dots per inch. */
    public Bitmap render(int index, double scale, PixelFormat format) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, ADDRESS);
            Native.check(Native.tpdf_page_render, pointer, index, scale, format.ordinal(), out);
            return new Bitmap(out.get(ADDRESS, 0));
        }
    }

    /** The strict structural validator (ruling 13); an empty list is a clean document. */
    public List<Defect> validate() {
        MemorySegment defects = handle(Native.tpdf_document_validate, pointer);
        try {
            int count = Native.callInt(Native.tpdf_defects_count, defects);
            List<Defect> found = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                found.add(new Defect(text(Native.tpdf_defect_rule, defects, i),
                        text(Native.tpdf_defect_message, defects, i)));
            }
            return found;
        } finally {
            Native.call(Native.tpdf_defects_free, defects);
        }
    }

    /** An editor over this document; it holds its own reference to the object store. */
    public Editor editor() {
        return new Editor(handle(Native.tpdf_document_editor, pointer));
    }

    /** One /Info text entry; null when absent, "" when empty. */
    public String info(InfoKey key) {
        return text(Native.tpdf_document_info, pointer, key.ordinal());
    }

    public Trapped trapped() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, INT);
            Native.check(Native.tpdf_document_trapped, pointer, out);
            return Trapped.values()[out.get(INT, 0)];
        }
    }

    public String pdfVersion() {
        return text(Native.tpdf_document_pdf_version, pointer);
    }

    /** A page's label; null when the document has no labels. */
    public String pageLabel(int index) {
        return text(Native.tpdf_document_page_label, pointer, index);
    }

    /** The XMP packet's bytes; null when there is none. */
    public byte[] xmpMetadata() {
        return Native.takeBuffer(handle(Native.tpdf_document_xmp_metadata, pointer));
    }

    /** The outline flattened to reading order, each item with its depth. */
    public List<OutlineItem> outline() {
        MemorySegment outline = handle(Native.tpdf_document_outline, pointer);
        try (Arena arena = Arena.ofConfined()) {
            int count = Native.callInt(Native.tpdf_outline_count, outline);
            List<OutlineItem> items = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                MemorySegment depth = Native.slot(arena, INT);
                MemorySegment open = Native.slot(arena, INT);
                Native.check(Native.tpdf_outline_item, outline, i, depth, open);
                String title = text(Native.tpdf_outline_title, outline, i);
                MemorySegment raw = Native.slot(arena, 72);
                Native.check(Native.tpdf_outline_destination, outline, i, raw);
                MemorySegment data = Native.slot(arena, ADDRESS);
                MemorySegment length = Native.slot(arena, LONG);
                Native.check(Native.tpdf_outline_destination_bytes, outline, i, data, length);
                items.add(new OutlineItem(depth.get(INT, 0), open.get(INT, 0) != 0, title,
                        Destination.read(raw, Native.borrowed(data, length))));
            }
            return items;
        } finally {
            Native.call(Native.tpdf_outline_free, outline);
        }
    }

    /** The link annotations on a page. */
    public List<Link> links(int page) {
        MemorySegment links = handle(Native.tpdf_page_links, pointer, page);
        try (Arena arena = Arena.ofConfined()) {
            int count = Native.callInt(Native.tpdf_links_count, links);
            List<Link> found = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                MemorySegment rect = Native.slot(arena, 32);
                Native.check(Native.tpdf_link_rect, links, i, rect, rect.asSlice(8), rect.asSlice(16),
                        rect.asSlice(24));
                MemorySegment present = Native.slot(arena, INT);
                MemorySegment object = Native.slot(arena, INT);
                MemorySegment generation = Native.slot(arena, SHORT);
                Native.check(Native.tpdf_link_reference, links, i, present, object, generation);
                MemorySegment kind = Native.slot(arena, INT);
                MemorySegment raw = Native.slot(arena, 72);
                Native.check(Native.tpdf_link_action, links, i, kind, raw);
                MemorySegment actionData = Native.slot(arena, ADDRESS);
                MemorySegment actionLength = Native.slot(arena, LONG);
                Native.check(Native.tpdf_link_action_bytes, links, i, actionData, actionLength);
                MemorySegment destinationData = Native.slot(arena, ADDRESS);
                MemorySegment destinationLength = Native.slot(arena, LONG);
                Native.check(Native.tpdf_link_destination_bytes, links, i, destinationData, destinationLength);
                found.add(new Link(rect.get(DOUBLE, 0), rect.get(DOUBLE, 8), rect.get(DOUBLE, 16),
                        rect.get(DOUBLE, 24),
                        present.get(INT, 0) == 0 ? null : ref(object, generation),
                        ActionKind.values()[kind.get(INT, 0)],
                        Destination.read(raw, Native.borrowed(destinationData, destinationLength)),
                        Native.borrowed(actionData, actionLength)));
            }
            return found;
        } finally {
            Native.call(Native.tpdf_links_free, links);
        }
    }

    /** Every file attached to the document (7.11.4). */
    public Attachments attachments() {
        return new Attachments(handle(Native.tpdf_document_attachments, pointer));
    }

    /** What the engine recovered from while reading. */
    public List<Warning> warnings() {
        MemorySegment warnings = handle(Native.tpdf_document_warnings, pointer);
        try (Arena arena = Arena.ofConfined()) {
            int count = Native.callInt(Native.tpdf_warnings_count, warnings);
            List<Warning> found = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                MemorySegment offset = Native.slot(arena, LONG);
                MemorySegment has = Native.slot(arena, INT);
                MemorySegment object = Native.slot(arena, INT);
                MemorySegment generation = Native.slot(arena, SHORT);
                Native.check(Native.tpdf_warning_location, warnings, i, offset, has, object, generation);
                found.add(new Warning(offset.get(LONG, 0), has.get(INT, 0) == 0 ? null : ref(object, generation),
                        text(Native.tpdf_warning_kind, warnings, i), text(Native.tpdf_warning_message, warnings, i)));
            }
            return found;
        } finally {
            Native.call(Native.tpdf_warnings_free, warnings);
        }
    }

    /** A page boundary, with the inheritance and defaults 14.11.2 gives it. */
    public Rect pageBox(int index, PageBoundary boundary) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment box = Native.slot(arena, 32);
            Native.check(Native.tpdf_page_boundary, pointer, index, boundary.ordinal(), box, box.asSlice(8),
                    box.asSlice(16), box.asSlice(24));
            return new Rect(box.get(DOUBLE, 0), box.get(DOUBLE, 8), box.get(DOUBLE, 16), box.get(DOUBLE, 24));
        }
    }

    /** The signatures, as the document states them. */
    public List<Signature> signatures() {
        MemorySegment signatures = handle(Native.tpdf_document_signatures, pointer);
        try (Arena arena = Arena.ofConfined()) {
            int count = Native.callInt(Native.tpdf_signatures_count, signatures);
            List<Signature> found = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                MemorySegment coverage = Native.slot(arena, INT);
                Native.check(Native.tpdf_signature_coverage, signatures, i, coverage);
                int spanCount = Native.callInt(Native.tpdf_signature_span_count, signatures, i);
                List<Span> spans = new ArrayList<>(spanCount);
                for (int s = 0; s < spanCount; s++) {
                    MemorySegment start = Native.slot(arena, LONG);
                    MemorySegment length = Native.slot(arena, LONG);
                    Native.check(Native.tpdf_signature_span, signatures, i, s, start, length);
                    spans.add(new Span(start.get(LONG, 0), length.get(LONG, 0)));
                }
                found.add(new Signature(text(Native.tpdf_signature_field_name, signatures, i),
                        text(Native.tpdf_signature_sub_filter, signatures, i),
                        text(Native.tpdf_signature_reason, signatures, i),
                        text(Native.tpdf_signature_location, signatures, i),
                        text(Native.tpdf_signature_name, signatures, i),
                        Coverage.values()[coverage.get(INT, 0)],
                        Native.callInt(Native.tpdf_signature_covers_whole_file, signatures, i) != 0,
                        Native.callInt(Native.tpdf_signature_is_usage_rights, signatures, i) != 0,
                        Native.callInt(Native.tpdf_signature_certification_level, signatures, i), spans));
            }
            return found;
        } finally {
            Native.call(Native.tpdf_signatures_free, signatures);
        }
    }

    /**
     * Verifies every signature against the anchors the caller trusts.
     * {@code at} is the instant, in seconds since the Unix epoch, to judge
     * validity at, or null to judge nothing: "expired" is a claim about a
     * moment the caller names.
     */
    public List<Verdict> verifySignatures(TrustAnchors anchors, Long at) {
        MemorySegment verdicts = handle(Native.tpdf_document_verify_signatures, pointer, anchors.pointer(),
                at == null ? 0 : 1, at == null ? 0L : at);
        try (Arena arena = Arena.ofConfined()) {
            int count = Native.callInt(Native.tpdf_verdicts_count, verdicts);
            List<Verdict> found = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                int[] answers = new int[4];
                MethodHandle[] accessors = {Native.tpdf_verdict_cms_state, Native.tpdf_verdict_document_digest,
                    Native.tpdf_verdict_signature_check, Native.tpdf_verdict_chain};
                for (int a = 0; a < accessors.length; a++) {
                    MemorySegment out = Native.slot(arena, INT);
                    Native.check(accessors[a], verdicts, i, out);
                    answers[a] = out.get(INT, 0);
                }
                MemorySegment notBefore = Native.slot(arena, LONG);
                MemorySegment notAfter = Native.slot(arena, LONG);
                boolean signer = Native.callInt(Native.tpdf_verdict_signer_validity, verdicts, i, notBefore,
                        notAfter) != 0;
                int weaknessCount = Native.callInt(Native.tpdf_verdict_weakness_count, verdicts, i);
                List<Weakness> weaknesses = new ArrayList<>(weaknessCount);
                for (int w = 0; w < weaknessCount; w++) {
                    MemorySegment out = Native.slot(arena, INT);
                    Native.check(Native.tpdf_verdict_weakness, verdicts, i, w, out);
                    weaknesses.add(Weakness.values()[out.get(INT, 0)]);
                }
                found.add(new Verdict(CmsState.values()[answers[0]], DocumentDigest.values()[answers[1]],
                        SignatureCheck.values()[answers[2]], Chain.values()[answers[3]],
                        text(Native.tpdf_verdict_signer_subject, verdicts, i),
                        text(Native.tpdf_verdict_signer_issuer, verdicts, i),
                        signer ? new long[] {notBefore.get(LONG, 0), notAfter.get(LONG, 0)} : null, weaknesses));
            }
            return found;
        } finally {
            Native.call(Native.tpdf_verdicts_free, verdicts);
        }
    }

    static Ref ref(MemorySegment object, MemorySegment generation) {
        return new Ref(object.get(INT, 0), Short.toUnsignedInt(generation.get(SHORT, 0)));
    }

    /** A call whose last argument is a {@code char **}: the string it wrote, or null. */
    static String text(MethodHandle handle, Object... leading) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, ADDRESS);
            Object[] arguments = java.util.Arrays.copyOf(leading, leading.length + 1);
            arguments[leading.length] = out;
            Native.check(handle, arguments);
            return Native.takeString(out);
        }
    }

    /** A call whose last argument is a handle out-pointer: the handle it wrote. */
    static MemorySegment handle(MethodHandle handle, Object... leading) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, ADDRESS);
            Object[] arguments = java.util.Arrays.copyOf(leading, leading.length + 1);
            arguments[leading.length] = out;
            Native.check(handle, arguments);
            return out.get(ADDRESS, 0);
        }
    }
}
