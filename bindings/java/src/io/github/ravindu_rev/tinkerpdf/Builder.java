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

    private Builder(MemorySegment pointer) {
        this.pointer = pointer;
    }

    /** Starts a document whose header declares PDF major.minor (7.5.2). */
    public static Builder withVersion(int major, int minor) {
        return new Builder(Document.handle(Native.tpdf_builder_new_with_version, major, minor));
    }

    /** Stops later pages inheriting the images registered so far. */
    public void clearImageResources() {
        Native.check(Native.tpdf_builder_clear_image_resources, pointer);
    }

    /** One of the standard 14 under an /Encoding of glyph names from firstCode, with their widths. */
    public void addNamedFont(String resource, String baseFont, int firstCode, List<String> names, short[] widths) {
        byte[] r = ascii(resource);
        byte[] f = ascii(baseFont);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment w = Native.slot(arena, Math.max(2L, 2L * widths.length));
            for (int i = 0; i < widths.length; i++) {
                w.set(Native.SHORT, 2L * i, widths[i]);
            }
            Native.check(Native.tpdf_builder_add_named_font, pointer, Native.bytes(arena, r), (long) r.length,
                    Native.bytes(arena, f), (long) f.length, firstCode, FormData.strings(arena, names),
                    (long) names.size(), w, (long) widths.length);
        }
    }

    /**
     * A graphics state under a resource name, starting from
     * {@code tpdf_ext_gstate_init}. {@code TpdfExtGState}: fill_alpha f64 @0,
     * stroke_alpha @8, has_blend_mode i32 @16, blend_mode @20, soft_mask @24,
     * mask_kind @28, mask_form @32, its length @40, backdrop @48, its length
     * @56; 64 bytes.
     */
    public void addExtGState(String resource, TinkerPdf.ExtGState state) {
        byte[] r = ascii(resource);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment raw = Native.slot(arena, 64);
            Native.check(Native.tpdf_ext_gstate_init, raw);
            if (state.fillAlpha() != null) {
                raw.set(Native.DOUBLE, 0, state.fillAlpha());
            }
            if (state.strokeAlpha() != null) {
                raw.set(Native.DOUBLE, 8, state.strokeAlpha());
            }
            if (state.blendMode() != null) {
                raw.set(Native.INT, 16, 1);
                raw.set(Native.INT, 20, state.blendMode().ordinal());
            }
            if (state.softMask() != null) {
                raw.set(Native.INT, 24, state.softMask().ordinal());
            }
            if (state.maskKind() != null) {
                raw.set(Native.INT, 28, state.maskKind().ordinal());
            }
            if (state.maskForm() != null) {
                byte[] form = ascii(state.maskForm());
                raw.set(ADDRESS, 32, Native.bytes(arena, form));
                raw.set(Native.LONG, 40, form.length);
            }
            if (state.backdrop() != null) {
                double[] backdrop = state.backdrop();
                MemorySegment b = Native.slot(arena, Math.max(8L, 8L * backdrop.length));
                for (int i = 0; i < backdrop.length; i++) {
                    b.set(Native.DOUBLE, 8L * i, backdrop[i]);
                }
                raw.set(ADDRESS, 48, b);
                raw.set(Native.LONG, 56, backdrop.length);
            }
            Native.check(Native.tpdf_builder_add_ext_gstate, pointer, Native.bytes(arena, r), (long) r.length, raw);
        }
    }

    private static MemorySegment matrix(Arena arena, double[] matrix) {
        if (matrix == null) {
            return MemorySegment.NULL;
        }
        if (matrix.length != 6) {
            throw new IllegalArgumentException("a matrix is six numbers, not " + matrix.length);
        }
        MemorySegment m = Native.slot(arena, 48);
        for (int i = 0; i < 6; i++) {
            m.set(Native.DOUBLE, 8L * i, matrix[i]);
        }
        return m;
    }

    /**
     * A form XObject (8.10); a null matrix is the identity and a null group
     * none. {@code TpdfTransparencyGroup}: color_space @0, isolated @4,
     * knockout @8.
     */
    public void addForm(String resource, double x0, double y0, double x1, double y1, double[] matrix,
            TinkerPdf.TransparencyGroup group, byte[] content) {
        byte[] r = ascii(resource);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment g = MemorySegment.NULL;
            if (group != null) {
                g = Native.slot(arena, 12);
                g.set(Native.INT, 0, group.colorSpace().ordinal());
                g.set(Native.INT, 4, Native.flag(group.isolated()));
                g.set(Native.INT, 8, Native.flag(group.knockout()));
            }
            Native.check(Native.tpdf_builder_add_form, pointer, Native.bytes(arena, r), (long) r.length, x0, y0, x1,
                    y1, matrix(arena, matrix), g, Native.bytes(arena, content), (long) content.length);
        }
    }

    /** A coloured tiling pattern (8.7.3); a null matrix is the identity. */
    public void addTilingPattern(String resource, double x0, double y0, double x1, double y1, double xStep,
            double yStep, double[] matrix, TinkerPdf.TilingType tilingType, byte[] content) {
        byte[] r = ascii(resource);
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_builder_add_tiling_pattern, pointer, Native.bytes(arena, r), (long) r.length,
                    x0, y0, x1, y1, xStep, yStep, matrix(arena, matrix), tilingType.ordinal(),
                    Native.bytes(arena, content), (long) content.length);
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
