package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.ADDRESS;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.util.List;

/** Assembles a document from pages, fonts and images. */
public final class Builder implements AutoCloseable {
    private MemorySegment pointer;

    public Builder() {
        pointer = Document.handle(Native.tpdf_builder_new);
    }

    private static byte[] ascii(String name) {
        return name.getBytes(StandardCharsets.UTF_8);
    }

    /** One of the standard 14 fonts under a resource name. */
    public void addBaseFont(String resource, String baseFont) {
        byte[] r = ascii(resource);
        byte[] f = ascii(baseFont);
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_builder_add_base_font, pointer, Native.bytes(arena, r), (long) r.length,
                    Native.bytes(arena, f), (long) f.length);
        }
    }

    /** A TrueType or CFF program, embedded under a resource name. */
    public void addEmbeddedFont(String resource, String baseFont, byte[] program) {
        byte[] r = ascii(resource);
        byte[] f = ascii(baseFont);
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_builder_add_embedded_font, pointer, Native.bytes(arena, r), (long) r.length,
                    Native.bytes(arena, f), (long) f.length, Native.bytes(arena, program), (long) program.length);
        }
    }

    /** Whether embedded fonts are subset to the glyphs drawn. */
    public void setSubsetFonts(boolean subset) {
        Native.check(Native.tpdf_builder_set_subset_fonts, pointer, Native.flag(subset));
    }

    /** An image under a resource name. {@code TpdfImage}: kind @0, width @4, height @8, data @16, len @24. */
    public void addImage(String resource, TinkerPdf.ImageKind kind, int width, int height, byte[] data) {
        byte[] r = ascii(resource);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment image = Native.slot(arena, 32);
            image.set(Native.INT, 0, kind.ordinal());
            image.set(Native.INT, 4, width);
            image.set(Native.INT, 8, height);
            image.set(ADDRESS, 16, Native.bytes(arena, data));
            image.set(Native.LONG, 24, data.length);
            Native.check(Native.tpdf_builder_add_image, pointer, Native.bytes(arena, r), (long) r.length, image);
        }
    }

    /** An /Info entry such as Title. */
    public void setInfo(String key, String value) {
        byte[] k = ascii(key);
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_builder_set_info, pointer, Native.bytes(arena, k), (long) k.length,
                    Native.cString(arena, value));
        }
    }

    /** Starts a page. The resource snapshot happens here. */
    public PageBuilder beginPage(double width, double height) {
        return new PageBuilder(Document.handle(Native.tpdf_builder_begin_page, pointer, width, height));
    }

    /** Adds a finished page, consuming it. */
    public void pushPage(PageBuilder page) {
        Native.check(Native.tpdf_builder_push_page, pointer, page.pointer());
    }

    /** The outline, from top-level entries, consuming each. */
    public void setOutline(List<OutlineEntry> entries) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_builder_set_outline, pointer, OutlineEntry.array(arena, entries),
                    (long) entries.size());
        }
    }

    /** The document's bytes, consuming the builder. */
    public byte[] finish() {
        return Native.takeBuffer(Document.handle(Native.tpdf_builder_finish, pointer));
    }

    /** Releases the builder, finished or not. */
    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_builder_free, pointer);
            pointer = null;
        }
    }
}
