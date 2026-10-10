// The Swift binding: Swift's own C interoperability over the C ABI in
// crates/tinker-pdf-ffi, whose committed header the CTinkerPdf module imports.
//
// UNVERIFIED. No Swift toolchain was available where this was written, so it
// has never been compiled, let alone run; it is written to the C importer's
// documented rules (an opaque `struct T *` is an OpaquePointer, a C enum is a
// RawRepresentable struct with one constant per case, `int` is Int32, `size_t`
// is Int) and to nothing that was observed. An enum the caller hands over
// crosses as an `int`, and the engine refuses a number its enum does not
// declare with BadArgument rather than reading it. It covers the core of the header -- open, text,
// render, validate, the form fill and save, and the builder -- and not the
// read surface, document operations, signatures or streaming, which the Go,
// Ruby and Java bindings carry. docs/features/bindings.md says what is owed.
//
// Ruling 11 is the whole design: the facade is the only public surface, and a
// binding projects it and adds no logic, caching or defaults of its own. Each
// method is one C call, or a loop of them over a list the engine hands back.
// Ownership is the C ABI's, carried by the classes: each frees its handle in
// deinit, and every String and [UInt8] handed back is a Swift copy.

import CTinkerPdf

/// A failure the engine reported: its TpdfStatus number, which is the ABI, and
/// the engine's own sentence, which names the call and the argument.
public struct TinkerPdfError: Error, CustomStringConvertible {
    public let status: Int
    public let message: String

    public var description: String { "\(message) (status \(status))" }
}

/// TpdfStatus numbers a caller branches on: the header's own constants, so
/// the C importer checks every one.
public enum Status {
    public static let badArgument = Int(TPDF_STATUS_BAD_ARGUMENT.rawValue)
    public static let notAPdf = Int(TPDF_STATUS_NOT_A_PDF.rawValue)
    public static let needsPassword = Int(TPDF_STATUS_NEEDS_PASSWORD.rawValue)
    public static let wrongPassword = Int(TPDF_STATUS_WRONG_PASSWORD.rawValue)
    public static let noSuchPage = Int(TPDF_STATUS_NO_SUCH_PAGE.rawValue)
    public static let notEncrypted = Int(TPDF_STATUS_NOT_ENCRYPTED.rawValue)
}

/// Runs one C call and throws on anything but Ok, with the engine's message,
/// read straight after on the same thread (it is per thread).
@discardableResult
func check(_ status: TpdfStatus) throws -> TpdfStatus {
    if status == TPDF_STATUS_OK {
        return status
    }
    let message = tpdf_last_error_message().map { String(cString: $0) } ?? "tinker-pdf error"
    throw TinkerPdfError(status: Int(status.rawValue), message: message)
}

/// An engine-allocated string, copied and freed; nil for null.
func take(_ pointer: UnsafeMutablePointer<CChar>?) -> String? {
    guard let pointer = pointer else { return nil }
    defer { tpdf_string_free(pointer) }
    return String(cString: pointer)
}

/// A TpdfBuffer, copied and freed.
func take(buffer: OpaquePointer?) -> [UInt8]? {
    guard let buffer = buffer else { return nil }
    defer { tpdf_buffer_free(buffer) }
    var length = 0
    guard let data = tpdf_buffer_data(buffer, &length) else { return [] }
    return Array(UnsafeBufferPointer(start: data, count: tpdf_buffer_len(buffer)))
}

/// Calls `body` with a pointer to `bytes` and their length. The C ABI refuses a
/// null pointer even with a zero length, so an empty array points at one byte
/// that is never read.
func withBytes<R>(_ bytes: [UInt8], _ body: (UnsafePointer<UInt8>, Int) throws -> R) rethrows -> R {
    let storage = bytes.isEmpty ? [0] : bytes
    return try storage.withUnsafeBufferPointer { buffer in
        try body(buffer.baseAddress!, bytes.count)
    }
}

/// The engine's version.
public func version() -> String {
    String(cString: tpdf_version())
}

/// One finding of the strict structural validator.
public struct Defect {
    public let rule: String
    public let message: String
}

/// A widget a fill wrote a value for and could not draw.
public struct SkippedWidget {
    public let object: UInt32
    public let generation: UInt16
    public let message: String
}

/// An open PDF.
public final class Document {
    let pointer: OpaquePointer

    /// Opens a document from bytes, which are copied.
    public init(bytes: [UInt8]) throws {
        var out: OpaquePointer? = nil
        try withBytes(bytes) { data, length in
            try check(tpdf_document_open(data, length, &out))
        }
        pointer = out!
    }

    deinit {
        tpdf_document_free(pointer)
    }

    public var pageCount: UInt32 { tpdf_document_page_count(pointer) }
    public var isEncrypted: Bool { tpdf_document_is_encrypted(pointer) != 0 }
    public var mayPrint: Bool { tpdf_document_may_print(pointer) != 0 }

    /// Tries a password; returns the TpdfAuthLevel number it reached.
    public func authenticate(_ password: String) throws -> Int {
        var level = TPDF_AUTH_LEVEL_NONE
        try check(tpdf_document_authenticate(pointer, password, &level))
        return Int(level.rawValue)
    }

    /// (width, height) in points.
    public func pageSize(_ index: UInt32) throws -> (Double, Double) {
        var width = 0.0
        var height = 0.0
        try check(tpdf_page_size(pointer, index, &width, &height))
        return (width, height)
    }

    /// A page's text, in logical order (ruling 14).
    public func pageText(_ index: UInt32) throws -> String {
        var out: UnsafeMutablePointer<CChar>? = nil
        try check(tpdf_page_text(pointer, index, &out))
        return take(out) ?? ""
    }

    /// The face a document that embeds none is drawn with. The engine bundles
    /// no faces and reads no font directories.
    public func setFonts(regular: [UInt8]) throws {
        try withBytes(regular) { data, length in
            try check(tpdf_document_set_fonts(pointer, data, length, nil, 0, nil, 0, nil, 0))
        }
    }

    /// Draws a page at a scale, 1.0 being 72 dots per inch, as RGB.
    public func render(_ index: UInt32, scale: Double = 1.0) throws -> Bitmap {
        var out: OpaquePointer? = nil
        try check(tpdf_page_render(pointer, index, scale, Int32(TPDF_PIXEL_FORMAT_RGB8.rawValue), &out))
        return Bitmap(pointer: out!)
    }

    /// The strict structural validator (ruling 13); empty is clean.
    public func validate() throws -> [Defect] {
        var defects: OpaquePointer? = nil
        try check(tpdf_document_validate(pointer, &defects))
        defer { tpdf_defects_free(defects) }
        return try (0..<tpdf_defects_count(defects)).map { index in
            var rule: UnsafeMutablePointer<CChar>? = nil
            var message: UnsafeMutablePointer<CChar>? = nil
            try check(tpdf_defect_rule(defects, index, &rule))
            let ruleText = take(rule) ?? ""
            try check(tpdf_defect_message(defects, index, &message))
            return Defect(rule: ruleText, message: take(message) ?? "")
        }
    }

    /// One /Info text entry by its TpdfInfoKey number; nil when absent. A
    /// number that is not a key throws BadArgument.
    public func info(_ key: Int32) throws -> String? {
        var out: UnsafeMutablePointer<CChar>? = nil
        try check(tpdf_document_info(pointer, key, &out))
        return take(out)
    }

    public func pdfVersion() throws -> String {
        var out: UnsafeMutablePointer<CChar>? = nil
        try check(tpdf_document_pdf_version(pointer, &out))
        return take(out) ?? ""
    }

    /// An editor over this document; it holds its own reference to the store.
    public func editor() throws -> Editor {
        var out: OpaquePointer? = nil
        try check(tpdf_document_editor(pointer, &out))
        return Editor(pointer: out!)
    }
}

/// A rendered page.
public final class Bitmap {
    let pointer: OpaquePointer

    init(pointer: OpaquePointer) {
        self.pointer = pointer
    }

    deinit {
        tpdf_bitmap_free(pointer)
    }

    public var width: UInt32 { tpdf_bitmap_width(pointer) }
    public var height: UInt32 { tpdf_bitmap_height(pointer) }
    public var stride: Int { tpdf_bitmap_stride(pointer) }

    /// A copy of the pixels.
    public var pixels: [UInt8] {
        var length = 0
        guard let data = tpdf_bitmap_data(pointer, &length) else { return [] }
        return Array(UnsafeBufferPointer(start: data, count: length))
    }
}

/// An editor over a document.
public final class Editor {
    let pointer: OpaquePointer

    init(pointer: OpaquePointer) {
        self.pointer = pointer
    }

    deinit {
        tpdf_editor_free(pointer)
    }

    /// Sets a text or choice field. A throw means nothing was written; a
    /// non-empty answer means the value was written and those widgets were
    /// left showing what they showed before.
    public func fillField(_ name: String, _ value: String) throws -> [SkippedWidget] {
        var report: OpaquePointer? = nil
        try check(tpdf_editor_fill_field(pointer, name, value, &report))
        defer { tpdf_fill_report_free(report) }
        return try (0..<tpdf_fill_report_count(report)).map { index in
            var number: UInt32 = 0
            var generation: UInt16 = 0
            var message: UnsafeMutablePointer<CChar>? = nil
            try check(tpdf_fill_report_widget(report, index, &number, &generation))
            try check(tpdf_fill_report_message(report, index, &message))
            return SkippedWidget(object: number, generation: generation, message: take(message) ?? "")
        }
    }

    public func setCheckbox(_ name: String, _ on: Bool) throws {
        try check(tpdf_editor_set_checkbox(pointer, name, on ? 1 : 0))
    }

    public func selectRadio(_ name: String, _ option: String) throws {
        try check(tpdf_editor_select_radio(pointer, name, option))
    }

    /// Saves with the engine's default options (tpdf_write_options_init) but
    /// the mode: 0 rewrite, 1 incremental, as TpdfWriteMode numbers them; any
    /// other number throws BadArgument.
    public func save(mode: Int32) throws -> [UInt8] {
        var options = TpdfWriteOptions()
        try check(tpdf_write_options_init(&options))
        options.mode = mode
        var out: OpaquePointer? = nil
        try check(tpdf_editor_save(pointer, &options, &out))
        return take(buffer: out) ?? []
    }
}

/// A page being drawn, owned until it is pushed.
public final class PageBuilder {
    let pointer: OpaquePointer

    init(pointer: OpaquePointer) {
        self.pointer = pointer
    }

    deinit {
        tpdf_page_builder_free(pointer)
    }

    public func text(font: String, size: Double, x: Double, y: Double, _ text: String) throws {
        let name = Array(font.utf8)
        try withBytes(name) { data, length in
            try check(tpdf_page_builder_text(pointer, data, length, size, x, y, text))
        }
    }

    public func fillRect(x: Double, y: Double, width: Double, height: Double, grey: Double) throws {
        try check(tpdf_page_builder_fill_rect(pointer, x, y, width, height, grey))
    }
}

/// Assembles a document from pages and fonts.
public final class Builder {
    let pointer: OpaquePointer

    public init() throws {
        var out: OpaquePointer? = nil
        try check(tpdf_builder_new(&out))
        pointer = out!
    }

    deinit {
        tpdf_builder_free(pointer)
    }

    /// One of the standard 14 fonts under a resource name.
    public func addBaseFont(resource: String, baseFont: String) throws {
        try withBytes(Array(resource.utf8)) { r, rl in
            try withBytes(Array(baseFont.utf8)) { f, fl in
                try check(tpdf_builder_add_base_font(pointer, r, rl, f, fl))
            }
        }
    }

    public func setInfo(key: String, _ value: String) throws {
        try withBytes(Array(key.utf8)) { k, kl in
            try check(tpdf_builder_set_info(pointer, k, kl, value))
        }
    }

    /// Starts a page. The resource snapshot happens here.
    public func beginPage(width: Double, height: Double) throws -> PageBuilder {
        var out: OpaquePointer? = nil
        try check(tpdf_builder_begin_page(pointer, width, height, &out))
        return PageBuilder(pointer: out!)
    }

    /// Adds a finished page, consuming it; the PageBuilder still frees its
    /// (now spent) handle.
    public func pushPage(_ page: PageBuilder) throws {
        try check(tpdf_builder_push_page(pointer, page.pointer))
    }

    /// The document's bytes, consuming the builder.
    public func finish() throws -> [UInt8] {
        var out: OpaquePointer? = nil
        try check(tpdf_builder_finish(pointer, &out))
        return take(buffer: out) ?? []
    }
}
