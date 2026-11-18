//! Booleans between pairs of simple solids in random poses.
//!
//! `cargo run --release -p limo-cad-kernel --example pairs -- 500` builds the
//! first 500 pairs of `demo::random_pair`: a ring, cone, ball, rod or block
//! against another, turned and moved so they usually overlap. Each pair is
//! cut, intersected and united. What is cut away and what is kept must add
//! up to the whole, the union must count the shared part once, and every
//! result must be a closed solid whose edges lie in its faces, with a clean
//! display mesh. A pair called
//! apart is checked against the meshes. `pairs N FIRST` starts at pair
//! `FIRST`, to look at one failure again.
use std::collections::HashMap;

use limo_cad_kernel::audit::{audit, CLOSE};
use limo_cad_kernel::demo::random_pair;
use limo_cad_kernel::math::*;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{boolean, Body, Op};

/// Volume from two fine meshes: their chord error falls with the square of
/// the step, so the pair extrapolates most of it away. Both must be clean.
fn volume(body: &Body) -> Result<f64, String> {
    let at = |linear: f64, angular: f64| {
        let options = TessOptions { linear, angular };
        let mesh = tessellate(body, &options);
        mesh.check(body, &options)
            .map(|()| mesh.volume())
            .map_err(|flaw| format!("meshed to {linear}, {flaw}"))
    };
    Ok((4.0 * at(0.001, 0.03)? - at(0.004, 0.06)?) / 3.0)
}

/// What is wrong with the display mesh, if anything.
fn skin(body: &Body) -> Option<String> {
    let options = TessOptions::default();
    tessellate(body, &options).check(body, &options).err()
}

/// Whether any mesh vertex of `a` lies inside `b`, by counting how often a
/// ray from it leaves through `b`'s mesh.
fn pokes_into(a: &Body, b: &Body) -> bool {
    let options = TessOptions::default();
    let (ma, mb) = (tessellate(a, &options), tessellate(b, &options));
    let dir = v3(0.371_391, 0.557_086, 0.742_781);
    ma.positions.iter().step_by(7).any(|&o| {
        let mut crossings = 0;
        for t in mb.indices.as_chunks::<3>().0 {
            let p = t.map(|i| mb.positions[i as usize]);
            let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
            let h = dir.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1.0e-12 {
                continue;
            }
            let s = o - p[0];
            let u = s.dot(h) / det;
            let q = s.cross(e1);
            let v = dir.dot(q) / det;
            if u >= 0.0 && v >= 0.0 && u + v <= 1.0 && e2.dot(q) / det > 1.0e-9 {
                crossings += 1;
            }
        }
        crossings % 2 == 1
    })
}

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("pairs COUNT [FIRST]"))
        .collect();
    let count = args.first().copied().unwrap_or(200);
    let first = args.get(1).copied().unwrap_or(0);
    let (mut good, mut apart, mut refused, mut wrong) = (0, 0, 0, 0);
    let mut reasons: HashMap<String, usize> = HashMap::new();
    for index in first..first + count {
        let [(name_a, a), (name_b, b)] = random_pair(index as u64);
        let label = format!("pair {index} ({name_a} and {name_b})");
        let results: Vec<Result<Body, String>> = [Op::Cut, Op::Intersect, Op::Union]
            .into_iter()
            .map(|op| boolean(&a, &b, op))
            .collect();
        if let Some(reason) = results.iter().find_map(|r| r.as_ref().err()) {
            // "Apart" is checked against the meshes: no vertex of either
            // solid may lie inside the other.
            if reason.contains("do not overlap") && (pokes_into(&a, &b) || pokes_into(&b, &a)) {
                wrong += 1;
                println!("{label}: WRONG, called apart but one reaches into the other");
                continue;
            }
            // Solids that miss each other are the common case and only
            // counted; any other refusal is worth a line of its own.
            refused += 1;
            *reasons.entry(reason.clone()).or_insert(0) += 1;
            if !reason.contains("do not overlap") {
                println!("{label}: refused, {reason}");
            }
            continue;
        }
        let bodies: Vec<&Body> = results.iter().map(|r| r.as_ref().unwrap()).collect();
        let mut fault: Option<String> = None;
        for (body, op) in bodies.iter().zip(["cut", "common", "union"]) {
            if let Err(e) = body.check() {
                fault = Some(format!("{op} is not a solid: {e}"));
            } else if let Err(e) = audit(body, CLOSE) {
                fault = Some(format!("{op}: {e}"));
            } else if let Some(e) = skin(body) {
                fault = Some(format!("{op}: {e}"));
            }
            if fault.is_some() {
                break;
            }
        }
        if fault.is_none() {
            let volumes: Result<Vec<f64>, String> = [&a, &b, bodies[0], bodies[1], bodies[2]]
                .into_iter()
                .map(volume)
                .collect();
            match volumes {
                Err(flaw) => fault = Some(flaw),
                Ok(v) => {
                    let (va, vb, cut, common, union) = (v[0], v[1], v[2], v[3], v[4]);
                    let scale = va.max(vb);
                    if (cut + common - va).abs() > 5.0e-4 * scale {
                        fault = Some(format!("cut {cut:.4} + common {common:.4} is not {va:.4}"));
                    } else if (union + common - va - vb).abs() > 5.0e-4 * scale {
                        fault = Some(format!(
                            "union {union:.4} + common {common:.4} is not {va:.4} + {vb:.4}"
                        ));
                    } else if common < 1.0e-9 * scale {
                        apart += 1;
                    }
                }
            }
        }
        match fault {
            None => good += 1,
            Some(fault) => {
                wrong += 1;
                println!("{label}: WRONG, {fault}");
            }
        }
    }
    println!(
        "{count} pairs: {good} right ({apart} of them apart), {refused} refused, {wrong} wrong"
    );
    let mut reasons: Vec<_> = reasons.into_iter().collect();
    reasons.sort_by_key(|reason| std::cmp::Reverse(reason.1));
    for (reason, n) in reasons {
        println!("  {n} x {reason}");
    }
}
