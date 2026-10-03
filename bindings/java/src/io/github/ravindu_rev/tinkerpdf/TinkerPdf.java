package io.github.ravindu_rev.tinkerpdf;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.List;

/**
 * The Java binding: the Foreign Function and Memory API over the C ABI in
 * crates/tinker-pdf-ffi, whose header crates/tinker-pdf-ffi/include/tinker_pdf.h
 * every declaration in {@link Native} is transcribed from.
 *
 * <p>Ruling 11 is the whole design: the facade is the only public surface, and
 * a binding projects it and adds no logic, caching or defaults of its own.
 * Every method is one C call, or a loop of them over a list the engine hands
 * back, and nothing else. Scope and packaging: docs/features/bindings.md.
 *
 * <p>This class holds the enums, transcribed in the header's order so that an
 * ordinal is the C number (which the C crate pins), the value types the calls
 * return, and the calls that belong to no handle. Ownership is the C ABI's:
 * every handle is {@link AutoCloseable}, closing twice is safe, and every
 * string and byte array handed back is a Java copy that outlives its handle.
 *
 * <p>Written against the part of {@code java.lang.foreign} that is the same in
 * JDK 21, where it is a preview API ({@code --enable-preview --release 21}),
 * and JDK 22, where it is final and the flag goes.
 */
public final class TinkerPdf {
    private TinkerPdf() {}

    /** {@code TpdfStatus}. Append only. */
    public enum Status {
        OK, BAD_ARGUMENT, NOT_A_PDF, NEEDS_PASSWORD, WRONG_PASSWORD, NO_SUCH_PAGE, NOT_ENCRYPTED,
        UNSUPPORTED_HANDLER, NO_SUCH_SIGNATURE, NO_SUCH_FIELD, VALUE_REFUSED, FIELD_UNREADABLE, SPENT_HANDLE,
        EDIT_REFUSED, SOURCE_MISS, SCRIPT_REFUSED, STREAM_UNREADABLE
    }

    /** {@code TpdfAuthLevel}. */
    public enum AuthLevel { NONE, USER, OWNER }

    /** {@code TpdfPixelFormat}. */
    public enum PixelFormat { GRAY8, GRAY_A8, RGB8, RGBA8 }

    /** {@code TpdfCoverage}. */
    public enum Coverage { WHOLE_FILE, REVISION, SUSPICIOUS }

    /** {@code TpdfCmsState}. */
    public enum CmsState { READ, ABSENT, UNREADABLE }

    /** {@code TpdfDocumentDigest}. {@code NOT_CHECKED} is not a failure. */
    public enum DocumentDigest { MATCHES, DIFFERS, NOT_CHECKED }

    /** {@code TpdfSignatureCheck}. {@code NOT_CHECKED} is not a failure. */
    public enum SignatureCheck { VERIFIED, FAILED, NOT_CHECKED }

    /** {@code TpdfChain}. */
    public enum Chain { ANCHORED_TO, SELF_SIGNED, INCOMPLETE, BROKEN, NO_ANCHORS, NO_SIGNER_CERTIFICATE }

    /** {@code TpdfWeakness}. */
    public enum Weakness {
        SHA1_DIGEST, SHA1_SIGNATURE, SHORT_RSA_KEY, COVERS_ONLY_A_REVISION, COVERAGE_SUSPICIOUS, OUTSIDE_VALIDITY
    }

    /** {@code TpdfWriteMode}. */
    public enum WriteMode { REWRITE, INCREMENTAL }

    /** {@code TpdfWidgetDefect}. */
    public enum WidgetDefect { RECT_MISSING }

    /** {@code TpdfDestKind}: how a destination positions its page (12.3.2.2). */
    public enum DestKind { XYZ, FIT, FIT_H, FIT_V, FIT_R, FIT_B, FIT_BH, FIT_BV }

    /** {@code TpdfImageKind}. */
    public enum ImageKind { JPEG, RGB8, GRAY8 }

    /** {@code TpdfLabelStyle}. */
    public enum LabelStyle { DECIMAL, ROMAN_UPPER, ROMAN_LOWER, LETTERS_UPPER, LETTERS_LOWER, NONE }

    /** {@code TpdfInfoKey}. */
    public enum InfoKey { TITLE, AUTHOR, SUBJECT, KEYWORDS, CREATOR, PRODUCER, CREATION_DATE, MODIFICATION_DATE }

    /** {@code TpdfMetadataSync}. */
    public enum MetadataSync { ALONE, OTHER_HALF_UNCHANGED }

    /** {@code TpdfTrapped}. */
    public enum Trapped { ABSENT, TRUE, FALSE, UNKNOWN }

    /** {@code TpdfPageBoundary}. */
    public enum PageBoundary { MEDIA_BOX, CROP_BOX, BLEED_BOX, TRIM_BOX, ART_BOX }

    /** {@code TpdfRemoval}. */
    public enum Removal {
        JAVA_SCRIPT, DOCUMENT_JAVA_SCRIPT, CALCULATION_ORDER, XFA_FORM, ACTION, EMBEDDED_FILE_TREE, EMBEDDED_FILE,
        INFO, METADATA
    }

    /** {@code TpdfDestinationKind}. */
    public enum DestinationKind { ABSENT, EXPLICIT, NAMED, URI }

    /** {@code TpdfActionKind}. */
    public enum ActionKind { ABSENT, GO_TO, GO_TO_R, URI, NAMED, LAUNCH, OTHER }

    /** The {@code TPDF_SCRIPT_*} policy bits. */
    public static final int SCRIPT_CALCULATE = 1;
    public static final int SCRIPT_FORMAT = 1 << 1;
    public static final int SCRIPT_KEYSTROKE = 1 << 2;
    public static final int SCRIPT_VALIDATE = 1 << 3;
    public static final int SCRIPT_DOCUMENT = 1 << 4;
    public static final int SCRIPT_CATALOG = 1 << 5;
    public static final int SCRIPT_DEFAULT = SCRIPT_CALCULATE | SCRIPT_FORMAT;

    /** {@code TPDF_ENTROPY_LEN}: the random bytes an encrypted save takes. */
    public static final int ENTROPY_LEN = 48;

    /** The engine's version. */
    public static String version() {
        return Native.readCString(Native.callAddress(Native.tpdf_version));
    }

    /** A destination's view; a null number is the file's null, "retain the current value". */
    public record View(DestKind kind, Double left, Double bottom, Double right, Double top, Double zoom) {
        public static View of(DestKind kind) {
            return new View(kind, null, null, null, null, null);
        }

        /** {@code TpdfDestination}: kind @0, then five doubles from @8; 48 bytes. */
        void write(MemorySegment segment, long offset) {
            segment.set(Native.INT, offset, kind.ordinal());
            segment.set(Native.DOUBLE, offset + 8, Native.nanFor(left));
            segment.set(Native.DOUBLE, offset + 16, Native.nanFor(bottom));
            segment.set(Native.DOUBLE, offset + 24, Native.nanFor(right));
            segment.set(Native.DOUBLE, offset + 32, Native.nanFor(top));
            segment.set(Native.DOUBLE, offset + 40, Native.nanFor(zoom));
        }

        static View read(MemorySegment segment, long offset) {
            return new View(DestKind.values()[segment.get(Native.INT, offset)],
                    Native.nullable(segment.get(Native.DOUBLE, offset + 8)),
                    Native.nullable(segment.get(Native.DOUBLE, offset + 16)),
                    Native.nullable(segment.get(Native.DOUBLE, offset + 24)),
                    Native.nullable(segment.get(Native.DOUBLE, offset + 32)),
                    Native.nullable(segment.get(Native.DOUBLE, offset + 40)));
        }
    }

    /** The engine's own {@code /Fit} view, from {@code tpdf_destination_init_fit}. */
    public static View fitView() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment raw = Native.slot(arena, 48);
            Native.check(Native.tpdf_destination_init_fit, raw);
            return View.read(raw, 0);
        }
    }

    /** Where a link or an outline entry goes: a page with a view, or a URI. */
    public record Target(Integer page, View view, String uri) {
        public static Target page(int index, View view) {
            return new Target(index, view, null);
        }

        public static Target uri(String uri) {
            return new Target(null, null, uri);
        }

        /** {@code TpdfTarget}: kind @0, page_index @4, view @8, uri @56; 64 bytes. */
        MemorySegment write(Arena arena) {
            MemorySegment raw = Native.slot(arena, 64);
            raw.set(Native.INT, 0, uri != null ? 1 : 0);
            raw.set(Native.INT, 4, page == null ? 0 : page);
            (view == null ? fitView() : view).write(raw, 8);
            raw.set(Native.ADDRESS, 56, Native.cStringOrNull(arena, uri));
            return raw;
        }
    }

    /** An indirect reference. */
    public record Ref(int object, int generation) {}

    /** A destination as read: a page and a view, a name, or a URI. */
    public record Destination(DestinationKind kind, Integer pageIndex, Ref pageRef, View view, byte[] bytes) {
        /**
         * {@code TpdfDestinationRead}: kind @0, has_page_index @4, page_index @8,
         * has_page_ref @12, page_object @16, page_generation @20, view @24; 72 bytes.
         */
        static Destination read(MemorySegment raw, byte[] bytes) {
            DestinationKind kind = DestinationKind.values()[raw.get(Native.INT, 0)];
            if (kind == DestinationKind.ABSENT) {
                return null;
            }
            return new Destination(kind,
                    raw.get(Native.INT, 4) == 0 ? null : raw.get(Native.INT, 8),
                    raw.get(Native.INT, 12) == 0 ? null
                            : new Ref(raw.get(Native.INT, 16), Short.toUnsignedInt(raw.get(Native.SHORT, 20))),
                    View.read(raw, 24), bytes);
        }
    }

    public record OutlineItem(int depth, boolean open, String title, Destination destination) {}

    public record Link(double x0, double y0, double x1, double y1, Ref reference, ActionKind action,
            Destination destination, byte[] actionBytes) {}

    public record Warning(long offset, Ref object, String kind, String message) {}

    public record Defect(String rule, String message) {}

    public record SkippedWidget(int object, int generation, WidgetDefect defect, String message) {}

    public record Span(long start, long length) {}

    public record Signature(String fieldName, String subFilter, String reason, String location, String name,
            Coverage coverage, boolean coversWholeFile, boolean usageRights, int certificationLevel,
            List<Span> spans) {}

    /** {@code signerValidity} is {notBefore, notAfter}, or null with no signer. */
    public record Verdict(CmsState cms, DocumentDigest documentDigest, SignatureCheck signature, Chain chain,
            String signerSubject, String signerIssuer, long[] signerValidity, List<Weakness> weaknesses) {}

    public record PageLabelRange(int firstPage, LabelStyle style, String prefix, int start) {}

    /** A date; a null offset is a date that names no zone. */
    public record Date(int year, int month, int day, int hour, int minute, int second, Integer utcOffsetMinutes) {
        /** {@code TpdfDate}: eight int32s; 32 bytes. */
        MemorySegment write(Arena arena) {
            MemorySegment raw = Native.slot(arena, 32);
            int[] fields = {year, month, day, hour, minute, second, utcOffsetMinutes == null ? 0 : 1,
                utcOffsetMinutes == null ? 0 : utcOffsetMinutes};
            for (int i = 0; i < fields.length; i++) {
                raw.set(Native.INT, i * 4L, fields[i]);
            }
            return raw;
        }
    }

    public record EmbeddedFile(String name, String filename, String description, String mimeType, Date created,
            Date modified, byte[] data) {}

    public record Sanitise(boolean javascript, boolean actions, boolean embeddedFiles, boolean metadata) {}

    /** One step of a path: a dictionary key, or an array index. */
    public record PathStep(byte[] key, Long index) {}

    public record RemovedEntry(Ref holder, List<PathStep> path, Removal what, byte[] action) {}

    public record DeletedObject(Ref object, Removal what, byte[] action) {}

    public record SanitiseReport(List<RemovedEntry> removed, List<DeletedObject> deleted) {}

    public record ChangedField(String name, String value) {}

    public record Recalculation(List<ChangedField> changed, int skipped, List<String> cascadesCut,
            List<String> refused) {}

    /** What a keystroke or validate action answered; a refusal is the form working, not an error. */
    public record ScriptAnswer(boolean accepted, String value) {}

    public record Rect(double x0, double y0, double x1, double y1) {}

    /**
     * How to encrypt on save. {@code entropy} is exactly {@link #ENTROPY_LEN}
     * caller-supplied bytes; there is no default, because the engine has no
     * opinion about where randomness comes from.
     */
    public record Encryption(String userPassword, String ownerPassword, int permissions, byte[] entropy) {}

    /**
     * {@code TpdfWriteOptions}, field for field. Start from {@link #writeOptions()},
     * the engine's own defaults, and change what you mean.
     */
    public static final class WriteOptions {
        public WriteMode mode;
        public boolean linearize;
        public int versionMajor;
        public int versionMinor;
        public boolean objectStreams;
        public boolean compress;
        public boolean garbageCollect;
        public Encryption encryption;

        WriteOptions() {}

        /**
         * mode @0, linearize @4, version_major @8, version_minor @12,
         * object_streams @16, compress @20, garbage_collect @24, encryption @32;
         * 40 bytes. {@code TpdfEncryption}: user_password @0, owner_password @8,
         * permissions @16, entropy @24, entropy_len @32; 40 bytes.
         */
        MemorySegment write(Arena arena) {
            MemorySegment raw = Native.slot(arena, 40);
            raw.set(Native.INT, 0, mode.ordinal());
            raw.set(Native.INT, 4, Native.flag(linearize));
            raw.set(Native.INT, 8, versionMajor);
            raw.set(Native.INT, 12, versionMinor);
            raw.set(Native.INT, 16, Native.flag(objectStreams));
            raw.set(Native.INT, 20, Native.flag(compress));
            raw.set(Native.INT, 24, Native.flag(garbageCollect));
            if (encryption != null) {
                MemorySegment lock = Native.slot(arena, 40);
                lock.set(Native.ADDRESS, 0, Native.cStringOrNull(arena, encryption.userPassword()));
                lock.set(Native.ADDRESS, 8, Native.cStringOrNull(arena, encryption.ownerPassword()));
                lock.set(Native.INT, 16, encryption.permissions());
                byte[] entropy = encryption.entropy() == null ? new byte[0] : encryption.entropy();
                lock.set(Native.ADDRESS, 24, Native.bytes(arena, entropy));
                lock.set(Native.LONG, 32, entropy.length);
                raw.set(Native.ADDRESS, 32, lock);
            }
            return raw;
        }
    }

    /** What {@code tpdf_write_options_init} fills in. */
    public static WriteOptions writeOptions() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment raw = Native.slot(arena, 40);
            Native.check(Native.tpdf_write_options_init, raw);
            WriteOptions options = new WriteOptions();
            options.mode = WriteMode.values()[raw.get(Native.INT, 0)];
            options.linearize = raw.get(Native.INT, 4) != 0;
            options.versionMajor = raw.get(Native.INT, 8);
            options.versionMinor = raw.get(Native.INT, 12);
            options.objectStreams = raw.get(Native.INT, 16) != 0;
            options.compress = raw.get(Native.INT, 20) != 0;
            options.garbageCollect = raw.get(Native.INT, 24) != 0;
            return options;
        }
    }
}
