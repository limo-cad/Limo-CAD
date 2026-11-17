//! Time the sample parts and, on request, model them at random dimensions.
//!
//! `cargo run --release -p limo-cad-kernel --example bench` prints how long
//! each part takes to model and to mesh. With `--sweep N` every part is also
//! built at `N` random points of its dimension ranges and checked.
use std::time::Instant;

use limo_cad_kernel::demo::PARTS;
use limo_cad_kernel::tess::{tessellate, TessOptions};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sweep: usize = match args.as_slice() {
        [] => 0,
        [flag, n] if flag == "--sweep" => n.parse().expect("--sweep takes a count"),
        _ => panic!("usage: bench [--sweep N]"),
    };
    println!(
        "{:<22} {:>9} {:>9} {:>6} {:>6} {:>10}",
        "part", "model ms", "mesh ms", "faces", "edges", "triangles"
    );
    for part in &PARTS {
        let mut model = f64::INFINITY;
        let mut body = part.default_body().unwrap();
        for _ in 0..50 {
            let clock = Instant::now();
            body = part.default_body().unwrap();
            model = model.min(clock.elapsed().as_secs_f64() * 1.0e3);
        }
        let mut mesh = f64::INFINITY;
        let mut triangles = 0;
        for _ in 0..50 {
            let clock = Instant::now();
            triangles = tessellate(&body, &TessOptions::default()).triangle_count();
            mesh = mesh.min(clock.elapsed().as_secs_f64() * 1.0e3);
        }
        println!(
            "{:<22} {model:>9.3} {mesh:>9.3} {:>6} {:>6} {triangles:>10}",
            part.name,
            body.face_count(),
            body.edge_count()
        );
    }
    if sweep == 0 {
        return;
    }
    // A fixed sequence, so a failure can be found again.
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut unit = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for part in &PARTS {
        let (mut built, mut refused) = (0, 0);
        let clock = Instant::now();
        for _ in 0..sweep {
            let values: Vec<f64> = part
                .dimensions
                .iter()
                // In steps of 0.1 mm, as the sliders of the browser demo move.
                .map(|d| ((d.min + (d.max - d.min) * unit()) * 10.0).round() / 10.0)
                .collect();
            match (part.build)(&values) {
                Ok(body) => {
                    body.check()
                        .unwrap_or_else(|e| panic!("{} {values:?}: {e}", part.name));
                    built += 1;
                }
                Err(reason) => {
                    refused += 1;
                    if refused <= 5 {
                        println!("  {} {values:.3?}: {reason}", part.name);
                    }
                }
            }
        }
        println!(
            "{}: {built} built, {refused} refused, {:.2} ms each",
            part.name,
            clock.elapsed().as_secs_f64() * 1.0e3 / sweep as f64
        );
    }
}
