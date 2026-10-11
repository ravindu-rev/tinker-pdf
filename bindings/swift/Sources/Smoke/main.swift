// Proves the Swift binding is the engine -- once somebody runs it.
//
// UNVERIFIED: never compiled or run; see Package.swift.
//
//   swift run -Xlinker -L../../target/release Smoke \
//     ../../testdata/simple-text.pdf /path/to/a/face.ttf
//
// The same trap every smoke test here is written against: simple-text.pdf
// embeds no font and the engine bundles none, so a render without a face is
// a correctly sized blank bitmap, and "a bitmap came back" passes on a
// renderer that does nothing. So it asserts blank without a face and inked
// with one, and prints SWIFT-SMOKE: RAN only when every check passed.

import Foundation
import TinkerPdf

func fail(_ message: String) -> Never {
    FileHandle.standardError.write("SWIFT-SMOKE: FAILED: \(message)\n".data(using: .utf8)!)
    exit(1)
}

func ink(_ pixels: [UInt8]) -> Int {
    stride(from: 0, to: pixels.count - 2, by: 3).filter { pixels[$0] != 0xFF }.count
}

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    FileHandle.standardError.write("usage: Smoke <file.pdf> <face.ttf>\n".data(using: .utf8)!)
    exit(2)
}

do {
    print("engine version \(TinkerPdf.version())")
    let data = [UInt8](try Data(contentsOf: URL(fileURLWithPath: arguments[1])))
    let face = [UInt8](try Data(contentsOf: URL(fileURLWithPath: arguments[2])))
    let document = try Document(bytes: data)
    guard document.pageCount == 3 else { fail("pageCount \(document.pageCount)") }
    let text = try document.pageText(0)
    guard text.contains("Tinker fixture") else { fail("text \(text)") }

    let bare = try document.render(0)
    guard ink(bare.pixels) == 0 else { fail("this fixture embeds no font, so it must draw nothing yet") }
    try document.setFonts(regular: face)
    let drawn = try document.render(0)
    let painted = ink(drawn.pixels)
    print("bitmap \(drawn.width)x\(drawn.height) stride=\(drawn.stride) ink=\(painted)")
    guard painted >= 100 else { fail("only \(painted) pixels of ink with a face supplied") }

    do {
        _ = try document.pageText(99)
        fail("a page past the end must be refused")
    } catch let error as TinkerPdfError {
        guard error.status == Status.noSuchPage else { fail("a page past the end is \(error)") }
    }

    let builder = try Builder()
    try builder.addBaseFont(resource: "F1", baseFont: "Helvetica")
    let page = try builder.beginPage(width: 200, height: 200)
    try page.text(font: "F1", size: 14, x: 20, y: 170, "Swift")
    try builder.pushPage(page)
    let built = try Document(bytes: try builder.finish())
    guard try built.validate().isEmpty else { fail("the built document has defects") }
    guard try built.pageText(0).contains("Swift") else { fail("the built text") }
    print("SWIFT-SMOKE: RAN, rendered and inked")
} catch {
    fail("\(error)")
}
