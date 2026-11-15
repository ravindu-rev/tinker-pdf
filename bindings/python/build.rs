//! The Python binding's one build step: on Windows, a reproducible link.
//!
//! **The MSVC linker writes the time of the link into the module it links**,
//! in its PE header and its debug directory, beside a random identifier that
//! pairs the module with its PDB, so no two builds of one commit are the same
//! bytes. `/Brepro` derives each of them from the module's own contents
//! instead, which is what makes the wheel's extension module the same bytes
//! from any build of the commit, wherever its checkout sits (ruling 25).
//!
//! Only the extension module's link, and only on the MSVC targets, the ones
//! a Windows wheel is built for: the linkers of the other targets write no
//! time into their output.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-cdylib-link-arg=/Brepro");
    }
}
