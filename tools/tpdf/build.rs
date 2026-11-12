//! `tpdf`'s one build step: on Linux, its loadable segments are aligned to
//! 64 KiB, so the kernel loads the binary at a 64 KiB boundary (MS-27).
//!
//! **The binary's own pages are part of every peak a corpus child reports,
//! and how many of them are resident turned on where the binary was loaded.**
//! A page fault on a file-backed mapping maps the 64 KiB around it that the
//! page cache holds, a window aligned in the address space rather than in the
//! file, and the kernel places a binary whose segments ask for 4 KiB at a
//! random 4 KiB boundary. Each run's windows therefore took in other pages of
//! the same code, and the file-backed part of one document's peak moved by
//! hundreds of kilobytes from run to run, which on a small document is a
//! large share of the two percent its corpus's band allows. The kernel aligns
//! a binary's load address to its largest segment alignment, so with every
//! segment asking for 64 KiB each run loads the binary a whole number of
//! windows from the last, and the same code maps the same pages.
//!
//! Only the binary's own link: the shared libraries it loads keep the
//! placement their own builds ask for, and no test binary is touched.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-z,max-page-size=65536");
    }
}
