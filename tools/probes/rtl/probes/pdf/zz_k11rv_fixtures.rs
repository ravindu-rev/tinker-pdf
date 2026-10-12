//! Scratch re-review probe (never committed): every local fixture's text,
//! in logical and content order, and every page's content stream bytes.
#![allow(missing_docs, clippy::all)]

use tinker_pdf::{Document, TextOptions};

fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn fixtures() {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let mut files = Vec::new();
    walk(&root.join("testdata"), &mut files);
    walk(&root.join("fuzz/corpus"), &mut files);
    let crates = std::fs::read_dir(root.join("crates")).unwrap();
    let mut crate_dirs: Vec<_> = crates.filter_map(Result::ok).map(|e| e.path()).collect();
    crate_dirs.sort();
    for c in crate_dirs {
        walk(&c.join("tests"), &mut files);
        walk(&c.join("testdata"), &mut files);
        walk(&c.join("fixtures"), &mut files);
    }
    let mut opened = 0;
    for path in files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let in_fuzz = path.to_string_lossy().contains("fuzz/corpus");
        if !in_fuzz && !matches!(ext.as_str(), "pdf" | "xps" | "oxps" | "epub" | "cbz" | "fb2" | "fdf") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.len() > 30_000_000 {
            continue;
        }
        let name = path.to_string_lossy().replace(&root.to_string_lossy().to_string(), "");
        let doc = match std::panic::catch_unwind(|| Document::open(bytes)) {
            Ok(Ok(doc)) => doc,
            _ => continue,
        };
        opened += 1;
        let mut logical = String::new();
        let mut content = String::new();
        let mut lines = 0usize;
        for at in 0..doc.page_count().min(400) {
            let Some(page) = doc.page(at) else { continue };
            logical.push_str(&page.text().plain_text());
            logical.push('\u{c}');
            let drawn = page.text_with(&TextOptions { content_order: true });
            lines += drawn.lines().len();
            content.push_str(&drawn.plain_text());
            content.push('\u{c}');
        }
        let cos = doc.cos();
        let mut all = Vec::new();
        for page in tinker_pdf_cos::pages::collect(cos) {
            all.extend_from_slice(&tinker_pdf_cos::pages::content_bytes(cos, &page));
        }
        let rtl = logical.chars().any(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFEFF));
        println!(
            "FIX\t{name}\tpages={}\tlines={lines}\trtl={}\tlogical={:016x}\tcontent={:016x}\tstreams={:016x}",
            doc.page_count(),
            rtl as u8,
            hash(logical.as_bytes()),
            hash(content.as_bytes()),
            hash(&all)
        );
    }
    println!("opened={opened}");
}
