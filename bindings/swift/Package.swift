// swift-tools-version:5.9
//
// UNVERIFIED: no Swift toolchain was available where this was written, so
// this package has never been built or run. It is source in the shape the
// other bindings over the C ABI have, kept so that the first person with a
// toolchain starts from a binding rather than from nothing; see
// docs/features/bindings.md for what it covers and what is owed.
//
//   cargo build -p tinker-pdf-ffi --release
//   swift run -Xlinker -L../../target/release Smoke \
//     ../../testdata/simple-text.pdf /path/to/a/face.ttf
import PackageDescription

let package = Package(
    name: "TinkerPdf",
    products: [
        .library(name: "TinkerPdf", targets: ["TinkerPdf"]),
    ],
    targets: [
        // The committed header, as a Clang module; the library is linked by
        // name, from wherever -L points.
        .systemLibrary(name: "CTinkerPdf", path: "Sources/CTinkerPdf"),
        .target(name: "TinkerPdf", dependencies: ["CTinkerPdf"]),
        .executableTarget(name: "Smoke", dependencies: ["TinkerPdf"]),
    ]
)
