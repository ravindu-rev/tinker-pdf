//! The C ABI's one build step: on Apple's platforms the dynamic library is
//! named `@rpath/libtinker_pdf_ffi.dylib`.
//!
//! **A dylib carries its own install name, and the linker's default is the
//! path it wrote the library to**: an absolute path into this build's target
//! directory, under the checkout and the home of whoever built it, which
//! every copy shipped in the NuGet package would carry. rustc's
//! `--remap-path-prefix` does not reach it, because the linker writes it.
//! `@rpath/` names the library by its file name alone, and a program linked
//! against it finds it through its own run paths, as the Go binding's
//! `-Wl,-rpath` to `target/release` does; a host that opens the library by
//! its path, as .NET, Ruby and Java do, never reads it.
//!
//! Only the dynamic library's link: the static library and the rlib have no
//! install name, and no other platform's library carries one.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libtinker_pdf_ffi.dylib");
    }
}
