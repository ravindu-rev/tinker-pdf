package io.github.ravindu_rev.tinkerpdf;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;

/** A page being drawn, owned until it is pushed; one never pushed leaves no trace. */
public final class PageBuilder implements AutoCloseable {
    private MemorySegment pointer;

    PageBuilder(MemorySegment pointer) {
        this.pointer = pointer;
    }

    MemorySegment pointer() {
        return pointer;
    }

    /** Text in a registered font. */
    public void text(String font, double size, double x, double y, String text) {
        byte[] f = font.getBytes(StandardCharsets.UTF_8);
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_page_builder_text, pointer, Native.bytes(arena, f), (long) f.length, size, x, y,
                    Native.cString(arena, text));
        }
    }

    /** A rectangle filled in device grey. */
    public void fillRect(double x, double y, double width, double height, double grey) {
        Native.check(Native.tpdf_page_builder_fill_rect, pointer, x, y, width, height, grey);
    }

    /** A registered image, drawn into a rectangle. */
    public void image(String resource, double x, double y, double width, double height) {
        byte[] r = resource.getBytes(StandardCharsets.UTF_8);
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_page_builder_image, pointer, Native.bytes(arena, r), (long) r.length, x, y, width,
                    height);
        }
    }

    /** A link annotation over a rectangle (12.5.6.5). */
    public void link(double x0, double y0, double x1, double y1, TinkerPdf.Target target) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_page_builder_link, pointer, x0, y0, x1, y1, target.write(arena));
        }
    }

    public void setFillRgb(double red, double green, double blue) {
        Native.check(Native.tpdf_page_builder_set_fill_rgb, pointer, red, green, blue);
    }

    public void setStrokeRgb(double red, double green, double blue) {
        Native.check(Native.tpdf_page_builder_set_stroke_rgb, pointer, red, green, blue);
    }

    public void setCropBox(double x0, double y0, double x1, double y1) {
        Native.check(Native.tpdf_page_builder_set_crop_box, pointer, x0, y0, x1, y1);
    }

    /** Content-stream operators, written as they are. */
    public void raw(byte[] operators) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_page_builder_raw, pointer, Native.bytes(arena, operators),
                    (long) operators.length);
        }
    }

    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_page_builder_free, pointer);
            pointer = null;
        }
    }
}
