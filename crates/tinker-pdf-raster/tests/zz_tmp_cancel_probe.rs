use std::time::Instant;
use tinker_pdf_raster::fill::fill;
use tinker_pdf_raster::geom::{FillRule, Path};

#[test]
fn how_long_is_one_unguarded_fill() {
    // A canvas at the page-area ceiling: 1 << 26 pixels, 8192 x 8192.
    let (w, h) = (8192u32, 8192u32);

    // A path with many edges, of the kind a real map or chart produces.
    let mut path = Path::new();
    let n = 20_000;
    path.move_to(0.0, 0.0);
    for i in 0..n {
        let t = i as f64;
        path.line_to((t * 7.0) % 8192.0, (t * 13.0) % 8192.0);
    }
    path.close();

    let start = Instant::now();
    let mask = fill(&path, FillRule::NonZero, 0, 0, w, h, 0.2);
    let elapsed = start.elapsed();
    println!(
        "fill of {n} edges over {w}x{h} took {:?} (mask {} bytes)",
        elapsed,
        mask.data.len()
    );
    assert!(elapsed.as_millis() < 1, "deliberately failing to print timing");
}
