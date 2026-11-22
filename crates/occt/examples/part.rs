//! Where two kernels part.
//!
//! Given the plan a recipe ends in on each kernel, as the lab keeps them
//! with `--plans <dir>`, this makes the recipe's bodies again a feature at
//! a time on both kernels, weighs them as solids after each, and says at
//! which feature the two first hold different amounts.
//!
//! ```sh
//! lab --server <limo-cad-mcp> --plans plans fillet-basics
//! cargo run --release -p limo-cad-occt --features native-occt --example part -- \
//!     plans/fillet-basics.occt.plan.json plans/fillet-basics.limo.plan.json
//! ```
//!
//! `--within 1e-11` looks for a difference smaller than the part in a
//! hundred million that counts by default, and `--body 61` looks at that
//! body alone.
//!
//! `--each` weighs after every feature and lists them all; without it the
//! first feature that makes a difference is searched for by halving, and
//! shown with what it was asked. What that feature changed in each body it
//! touched is given for both kernels, so that it can be held against a
//! value worked out by hand.
use limo_cad_core::BodyId;
use limo_cad_occt::OcctKernel;
use limo_cad_solid::{KernelJobDto, RecomputePlanDto};
use std::collections::BTreeMap;

/// A part in a hundred million.
const CLOSE: f64 = 1.0e-8;

/// How close two volumes have to be to count as the same: [`CLOSE`] unless
/// asked otherwise.
static WITHIN: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

fn within() -> f64 {
    *WITHIN.get().unwrap_or(&CLOSE)
}

/// The one body to look at, when one was named.
static ONLY: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

fn looked_at(id: u64) -> bool {
    ONLY.get().is_none_or(|only| *only == id)
}

fn plan(path: &str) -> RecomputePlanDto {
    let text = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_slice(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn first(plan: &RecomputePlanDto, jobs: usize) -> RecomputePlanDto {
    RecomputePlanDto {
        transaction_id: plan.transaction_id,
        errors: Vec::new(),
        jobs: plan.jobs[..jobs].to_vec(),
    }
}

/// What every body holds after the first `jobs` features, on each kernel,
/// or why one of them could not be made.
type Held = Result<BTreeMap<u64, f64>, String>;

fn weighed(reference: &RecomputePlanDto, limo: &RecomputePlanDto, jobs: usize) -> (Held, Held) {
    let theirs = (|| {
        let mut kernel = OcctKernel::new().map_err(|e| e.to_string())?;
        let scene = kernel
            .recompute(&first(reference, jobs))
            .map_err(|e| e.to_string())?;
        if let Some(error) = scene.errors.first() {
            return Err(error.message.clone());
        }
        scene
            .bodies
            .iter()
            .filter(|body| looked_at(body.body_id.0))
            .map(|body| Ok((body.body_id.0, kernel.weigh(body.body_id)?)))
            .collect()
    })();
    let ours = (|| {
        let mut kernel = limo_cad_kernel_session::Kernel::new();
        let scene = kernel.recompute(&first(limo, jobs));
        if let Some(error) = scene.errors.first() {
            return Err(error.message.clone());
        }
        Ok(scene
            .bodies
            .iter()
            .filter(|body| looked_at(body.body_id.0))
            .map(|body| {
                let BodyId(id) = body.body_id;
                let held = limo_cad_kernel::props::mass(kernel.body(id).unwrap()).volume;
                (id, held)
            })
            .collect())
    })();
    (theirs, ours)
}

/// The bodies the two kernels do not hold the same amount of.
fn apart(theirs: &Held, ours: &Held) -> Vec<String> {
    match (theirs, ours) {
        (Ok(theirs), Ok(ours)) => {
            let mut out = Vec::new();
            for (id, a) in theirs {
                match ours.get(id) {
                    Some(b) if (a - b).abs() <= within() * a.abs().max(1.0) => {}
                    Some(b) => out.push(format!(
                        "body {id}: {b:.9} where the reference holds {a:.9}, {:+.9}",
                        b - a
                    )),
                    None => out.push(format!("body {id} is the reference's alone")),
                }
            }
            out.extend(
                ours.keys()
                    .filter(|id| !theirs.contains_key(id))
                    .map(|id| format!("body {id} is this kernel's alone")),
            );
            out
        }
        (Err(why), Ok(_)) => vec![format!("the reference: {why}")],
        (Ok(_), Err(why)) => vec![format!("this kernel: {why}")],
        (Err(a), Err(b)) => vec![format!("the reference: {a}"), format!("this kernel: {b}")],
    }
}

fn kind(job: &KernelJobDto) -> String {
    serde_json::to_value(job)
        .ok()
        .and_then(|value| value["kind"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn main() {
    let mut each = false;
    let mut paths = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--each" => each = true,
            "--within" => {
                let close = args.next().and_then(|value| value.parse::<f64>().ok());
                let _ = WITHIN.set(close.expect("--within takes a number"));
            }
            "--body" => {
                let only = args.next().and_then(|value| value.parse::<u64>().ok());
                let _ = ONLY.set(only.expect("--body takes a body's number"));
            }
            _ => paths.push(arg),
        }
    }
    let [reference, limo] = paths.as_slice() else {
        eprintln!(
            "usage: part [--each] [--within <part>] [--body <number>] <recipe>.occt.plan.json <recipe>.limo.plan.json"
        );
        std::process::exit(2);
    };
    let (reference, limo) = (plan(reference), plan(limo));
    let jobs = reference.jobs.len().min(limo.jobs.len());
    if reference.jobs.len() != limo.jobs.len() {
        println!(
            "the plans are {} and {} features long; comparing the first {jobs}",
            reference.jobs.len(),
            limo.jobs.len()
        );
    }
    let name = |k: usize| {
        let job = &reference.jobs[k - 1];
        format!(
            "feature {k} of {jobs} ({}, id {})",
            kind(job),
            job.feature_id().0
        )
    };
    if each {
        for k in 1..=jobs {
            let (theirs, ours) = weighed(&reference, &limo, k);
            let apart = apart(&theirs, &ours);
            println!(
                "{}: {}",
                name(k),
                if apart.is_empty() {
                    "the same"
                } else {
                    "apart"
                }
            );
            for line in apart {
                println!("    {line}");
            }
        }
        return;
    }
    let (theirs, ours) = weighed(&reference, &limo, jobs);
    if apart(&theirs, &ours).is_empty() {
        println!("the two kernels hold the same after all {jobs} features");
        return;
    }
    // The first feature after which they differ, taking it that they then
    // go on differing.
    let (mut same, mut differ) = (0, jobs);
    while differ - same > 1 {
        let middle = (same + differ) / 2;
        let (theirs, ours) = weighed(&reference, &limo, middle);
        if apart(&theirs, &ours).is_empty() {
            same = middle;
        } else {
            differ = middle;
        }
    }
    let (theirs, ours) = weighed(&reference, &limo, differ);
    println!("the two kernels part at {}:", name(differ));
    for line in apart(&theirs, &ours) {
        println!("    {line}");
    }
    // What the feature changed, on each kernel, from what that kernel
    // held before it.
    if let ((Ok(theirs_before), Ok(ours_before)), (Ok(theirs), Ok(ours))) =
        (weighed(&reference, &limo, differ - 1), (&theirs, &ours))
    {
        for (id, a) in theirs {
            let (Some(b), Some(was_a), Some(was_b)) =
                (ours.get(id), theirs_before.get(id), ours_before.get(id))
            else {
                continue;
            };
            if a != was_a || b != was_b {
                println!(
                    "    body {id}: the feature changed it by {:+.9} on the reference (from {was_a:.9}) and by {:+.9} here (from {was_b:.9})",
                    a - was_a,
                    b - was_b
                );
            }
        }
    }
    for (whose, plan) in [("the reference", &reference), ("this kernel", &limo)] {
        println!("asked of {whose}:");
        println!(
            "{}",
            serde_json::to_string_pretty(&plan.jobs[differ - 1]).unwrap_or_default()
        );
    }
}
