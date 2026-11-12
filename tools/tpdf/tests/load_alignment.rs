//! `tpdf`'s loadable segments ask for 64 KiB on Linux (`build.rs`, MS-27),
//! read from the binary cargo built for this test as the kernel reads them:
//! an ELF file's program headers, and each loadable segment's `p_align`.

/// The alignment `build.rs` asks the linker for: the kernel's fault-around
/// window.
const WINDOW: u64 = 64 * 1024;

/// `PT_LOAD`, the program header type of a loadable segment.
const PT_LOAD: u32 = 1;

/// Each loadable segment's alignment in a 64-bit little-endian ELF file, in
/// header order; `None` for a file that is not one, or whose program headers
/// run past its end.
fn load_alignments(elf: &[u8]) -> Option<Vec<u64>> {
    if elf.get(..4)? != b"\x7fELF" || *elf.get(4)? != 2 || *elf.get(5)? != 1 {
        return None;
    }
    let bytes = |at: usize, length: usize| elf.get(at..at.checked_add(length)?);
    let u16_at = |at: usize| Some(u16::from_le_bytes(bytes(at, 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(bytes(at, 4)?.try_into().ok()?));
    let u64_at = |at: usize| Some(u64::from_le_bytes(bytes(at, 8)?.try_into().ok()?));
    let table = usize::try_from(u64_at(0x20)?).ok()?;
    let size = usize::from(u16_at(0x36)?);
    let count = usize::from(u16_at(0x38)?);
    if size < 0x38 {
        return None;
    }
    let mut aligns = Vec::new();
    for index in 0..count {
        let header = table.checked_add(index.checked_mul(size)?)?;
        if u32_at(header)? == PT_LOAD {
            aligns.push(u64_at(header.checked_add(0x30)?)?);
        }
    }
    Some(aligns)
}

/// **Every loadable segment of the Linux binary asks for the fault-around
/// window**, so the kernel loads it at a 64 KiB boundary and each run maps
/// the same pages of its code.
#[test]
fn every_loadable_segment_of_the_linux_binary_asks_for_the_fault_around_window() {
    if !cfg!(target_os = "linux") {
        println!("SKIPPED: tpdf's load alignment, which its Linux build alone asks for");
        return;
    }
    let path = env!("CARGO_BIN_EXE_tpdf");
    let binary = std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let aligns = load_alignments(&binary)
        .unwrap_or_else(|| panic!("{path} is no 64-bit little-endian ELF file"));
    assert!(!aligns.is_empty(), "{path} has no loadable segment");
    for align in &aligns {
        assert!(
            *align >= WINDOW && align % WINDOW == 0,
            "{path}: a loadable segment aligned to {align} bytes, not to {WINDOW}"
        );
    }
    println!(
        "RAN: tpdf's load alignment, {} loadable segments each aligned to {WINDOW} bytes or more",
        aligns.len()
    );
}

/// The reading written out, on every target: the loadable segments of a
/// three-header table, the other header passed over, a file that is not ELF
/// refused and a table that runs past the file refused.
#[test]
fn the_loadable_segments_alignments_are_read_from_the_program_headers() {
    let mut elf = vec![0u8; 0x40 + 3 * 0x38];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes());
    elf[0x36..0x38].copy_from_slice(&0x38u16.to_le_bytes());
    elf[0x38..0x3a].copy_from_slice(&3u16.to_le_bytes());
    let headers = [(PT_LOAD, 0x1_0000u64), (6, 8), (PT_LOAD, 0x20_0000)];
    for (index, (kind, align)) in headers.into_iter().enumerate() {
        let at = 0x40 + index * 0x38;
        elf[at..at + 4].copy_from_slice(&kind.to_le_bytes());
        elf[at + 0x30..at + 0x38].copy_from_slice(&align.to_le_bytes());
    }
    assert_eq!(load_alignments(&elf), Some(vec![0x1_0000, 0x20_0000]));
    assert_eq!(load_alignments(b"MZ\x90\x00"), None, "not ELF");
    let mut cut = elf.clone();
    cut.truncate(0x40 + 0x38);
    assert_eq!(
        load_alignments(&cut),
        None,
        "a table past the end of the file"
    );
}
