//! Replay a captured Boolean: `replay a.body b.body cut|union|intersect`.
use limo_cad_kernel::{boolean, Body, Op};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let load = |p: &str| Body::from_text(&std::fs::read_to_string(p).unwrap()).unwrap();
    let (a, b) = (load(&args[1]), load(&args[2]));
    let op = match args.get(3).map(String::as_str) {
        Some("union") => Op::Union,
        Some("intersect") => Op::Intersect,
        _ => Op::Cut,
    };
    a.check().expect("operand A is invalid");
    b.check().expect("operand B is invalid");
    let started = std::time::Instant::now();
    match boolean(&a, &b, op) {
        Ok(r) => println!(
            "ok in {:?}: {} faces, {} edges, {} vertices",
            started.elapsed(),
            r.face_count(),
            r.edge_count(),
            r.verts.len()
        ),
        Err(e) => println!("failed: {e}"),
    }
}
