//! Temporary probe: what happens to an OpenType/CFF ('OTTO') program?

use std::collections::BTreeSet;
use tinker_pdf_font::{glyf, Cff, Sfnt};

/// Assembles an sfnt directory with the given tag and tables.
fn assemble(tag: &[u8; 4], tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(tag);
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (t, data) in tables {
        out.extend_from_slice(*t);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

fn otto() -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&1u16.to_be_bytes()); // numberOfHMetrics
                                                       // A 'CFF ' table with plausible-looking bytes; nothing reads it.
    let cff = vec![1u8, 0, 4, 1, 0, 0, 0, 0];
    assemble(
        b"OTTO",
        &[
            (b"head", head),
            (b"hhea", hhea),
            (b"CFF ", cff),
            (b"cmap", vec![0u8; 4]),
        ],
    )
}

#[test]
fn probe_otto_goes_through_the_sfnt_branch_and_yields_an_empty_outline() {
    let data = otto();

    // 1. Does Sfnt::parse accept it? (the claim's first link)
    let sfnt = Sfnt::parse(&data).expect("PROBE: Sfnt::parse ACCEPTS an OTTO program");
    assert!(sfnt.table(0x676C_7966).is_none(), "PROBE: there is no glyf");
    assert!(
        sfnt.table(0x4346_4620).is_some(),
        "PROBE: there IS a CFF table"
    );

    // 2. Does glyf::outline return None (a signalled failure) or Some(empty)?
    let result = glyf::outline(&sfnt, 3);
    match &result {
        None => panic!("PROBE RESULT: glyf::outline returned None -- claim refuted"),
        Some(o) => {
            eprintln!(
                "PROBE RESULT: glyf::outline returned Some with {} segments; is_empty={}",
                o.segments.len(),
                o.is_empty()
            );
            assert!(o.is_empty(), "PROBE: the outline is EMPTY, not absent");
        }
    }

    // 3. Would the CFF branch even have worked, had it been reached?
    let via_cff = Cff::parse(&data);
    eprintln!(
        "PROBE RESULT: Cff::parse on the whole OTTO program -> {}",
        if via_cff.is_some() { "Some" } else { "None" }
    );

    // 4. And the subsetter, for contrast: does it refuse OTTO?
    eprintln!(
        "PROBE RESULT: subset() on OTTO -> {}",
        if tinker_pdf_font::subset(&data, &BTreeSet::new()).is_some() {
            "Some"
        } else {
            "None (refused)"
        }
    );
}
