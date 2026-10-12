//! Review probe (round 5, code1): time `drawn_reading` on worst cases for the
//! sentence-end exception, doubling the line, at two trees.
use std::time::Instant;
use tinker_pdf_shape::bidi::{drawn_direction, drawn_reading};

fn line(kind: &str, n: usize) -> String {
    let he = "\u{5D5}\u{5D9} )\u{5D1}\u{5D9}\u{5D3}(";
    match kind {
        // a pair, then n stops
        "stops" => format!("{he}{}", ".".repeat(n)),
        // a pair, n stops, n closing quotes
        "stopsquotes" => format!("{he}{}{}", ".".repeat(n), "\u{201D}".repeat(n)),
        // n quotes and no stop: read past and refused
        "quotes" => format!("{he}{}", "\"".repeat(n)),
        // n pairs then an ellipsis
        "pairs" => format!("{}\u{2026}", he.repeat(n / 8 + 1)),
        // only stops
        "allstops" => "?!".repeat(n / 2),
        _ => unreachable!(),
    }
}

#[test]
fn zz_code1_time() {
    for kind in ["stops", "stopsquotes", "quotes", "pairs", "allstops"] {
        let mut row = format!("{kind}:");
        for n in [2_000usize, 4_000, 8_000, 16_000, 32_000] {
            let text = line(kind, n);
            let units: Vec<String> = text.chars().map(String::from).collect();
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            let t = Instant::now();
            let mut sink = 0usize;
            for _ in 0..5 {
                let r = drawn_reading(&units);
                sink += r.order.len();
                sink += usize::from(drawn_direction(&units) == r.direction);
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
            row.push_str(&format!(" n={n} {ms:.2}ms({sink})"));
        }
        eprintln!("ZZT {row}");
    }
}
