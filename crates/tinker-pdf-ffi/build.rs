//! The C ABI's build step: the dynamic library's link, made to name no
//! machine and to be the same bytes from any build of a commit.
//!
//! **On Apple's platforms a dylib carries its own install name, and the
//! linker's default is the path it wrote the library to**: an absolute path
//! into this build's target directory, under the checkout and the home of
//! whoever built it, which every copy shipped in the NuGet package would
//! carry. rustc's `--remap-path-prefix` does not reach it, because the linker
//! writes it. `@rpath/` names the library by its file name alone, and a
//! program linked against it finds it through its own run paths, as the Go
//! binding's `-Wl,-rpath` to `target/release` does; a host that opens the
//! library by its path, as .NET, Ruby and Java do, never reads it.
//!
//! **On the MSVC targets the linker writes the time of the link into the
//! library**, in its PE header and its debug directory, beside a random
//! identifier that pairs it with its PDB. `/Brepro` derives each from the
//! library's own contents instead.
//!
//! Only the dynamic library's link: the static library and the rlib are not
//! linked, and the other targets' linkers do neither.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libtinker_pdf_ffi.dylib");
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-cdylib-link-arg=/Brepro");
    }
}
