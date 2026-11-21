//! Differential kernel lab.
//!
//! Runs construction scripts through the real headless Limo CAD server once
//! per kernel and compares the resulting solids: how many bodies, what
//! each holds, its bounds and the make-up of its faces. Kernel time is read
//! from the adapter's opt-in timing log, so sketch solving and transport
//! are excluded.
//!
//! What a body holds is asked of the kernel that made it, which weighs it
//! as a solid (`LIMO_CAD_KERNEL_WEIGH`, see the adapter). The two kernels
//! have to agree on it to a part in a hundred million. Where they are known
//! not to and the reference kernel is the one that is out, the body is in
//! [`KNOWN`] with where the right value is worked out; any other difference
//! fails the recipe. The drawn skin of a body is compared as well, but two
//! kernels do not facet a round face alike, and that comparison can only
//! show that they made something alike.
//!
//! ```sh
//! cargo run --release -p limo-cad-kernel --example lab -- \
//!     --server mcp-server/target/release/limo-cad-mcp fillet-basics extrude-basics
//! cargo run --release -p limo-cad-kernel --example lab -- --server <bin> --all
//! ```
//!
//! `--plans <dir>` keeps the plan every recipe ends in on each kernel. The
//! adapter's `part` example makes a recipe's bodies again from those, a
//! feature at a time on both kernels, and says at which feature the two
//! first hold different amounts.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Client {
    child: Child,
    input: ChildStdin,
    replies: Receiver<Value>,
    id: u64,
}

impl Client {
    fn start(server: &Path, kernel: &str, timing: &Path, weigh: &Path) -> Client {
        let mut child = Command::new(server)
            .arg("--headless")
            .env("LIMO_CAD_KERNEL", kernel)
            .env("LIMO_CAD_KERNEL_TIMING", timing)
            .env("LIMO_CAD_KERNEL_WEIGH", weigh)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // What the kernel says of its decisions is worth reading when
            // it has been asked to say it.
            .stderr(if std::env::var_os("LIMO_CAD_KERNEL_TRACE").is_some() {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .spawn()
            .expect("could not start the Limo CAD MCP server");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if sender.send(value).is_err() {
                        break;
                    }
                }
            }
        });
        let mut client = Client {
            child,
            input,
            replies,
            id: 0,
        };
        client.rpc(
            "initialize",
            json!({"protocolVersion":"2025-06-18","capabilities":{},
                   "clientInfo":{"name":"kernel-lab","version":"1"}}),
        );
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(900);
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.replies.recv_timeout(wait) {
                Ok(reply) if reply["id"] == json!(self.id) => return reply,
                Ok(_) => continue,
                Err(_) => return json!({"error": "no reply from the server"}),
            }
        }
    }

    /// Call a tool; `Err` carries the tool's error text.
    fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let reply = self.rpc("tools/call", json!({"name": name, "arguments": arguments}));
        if let Some(error) = reply.get("error") {
            return Err(error.to_string());
        }
        let result = &reply["result"];
        let text = result["content"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["type"] == "text"))
            .and_then(|item| item["text"].as_str())
            .unwrap_or("");
        if result["isError"] == true {
            let most = if std::env::var_os("LAB_WHOLE_ERROR").is_some() {
                400_000
            } else {
                600
            };
            return Err(text.chars().take(most).collect());
        }
        serde_json::from_str(text).map_err(|e| format!("unreadable reply: {e}"))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default, Clone)]
struct BodySummary {
    id: u64,
    volume: f64,
    /// What the body holds, weighed as a solid by the kernel that made it,
    /// where it could be.
    held: Option<f64>,
    lo: [f64; 3],
    hi: [f64; 3],
    triangles: usize,
    faces: usize,
    planes: usize,
    cylinders: usize,
    edges: usize,
    plane_area: f64,
    /// Area of the whole display mesh.
    area: f64,
    /// Area and middle of every flat face.
    flats: Vec<Flat>,
}

fn summarise(scene: &Value) -> Vec<BodySummary> {
    let mut out = Vec::new();
    for body in scene["bodies"].as_array().into_iter().flatten() {
        let positions: Vec<f64> = body["mesh"]["positions"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        let indices: Vec<usize> = body["mesh"]["indices"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_u64)
                    .map(|i| i as usize)
                    .collect()
            })
            .unwrap_or_default();
        let point = |i: usize| [positions[3 * i], positions[3 * i + 1], positions[3 * i + 2]];
        let mut summary = BodySummary {
            id: body["id"].as_u64().unwrap_or(0),
            lo: [f64::INFINITY; 3],
            hi: [f64::NEG_INFINITY; 3],
            triangles: indices.len() / 3,
            ..Default::default()
        };
        for t in indices.as_chunks::<3>().0 {
            let (a, b, c) = (point(t[0]), point(t[1]), point(t[2]));
            summary.volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
            let (u, v) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            summary.area += 0.5 * cross.iter().map(|x| x * x).sum::<f64>().sqrt();
            for p in [a, b, c] {
                for (k, x) in p.into_iter().enumerate() {
                    summary.lo[k] = summary.lo[k].min(x);
                    summary.hi[k] = summary.hi[k].max(x);
                }
            }
        }
        for face in body["faces"].as_array().into_iter().flatten() {
            summary.faces += 1;
            if !face["plane"].is_null() {
                summary.planes += 1;
                let area = face["signature"]["area"].as_f64().unwrap_or(0.0);
                summary.plane_area += area;
                let middle = &face["signature"]["centroid"];
                let at = |k: usize, name: &str| {
                    middle[k]
                        .as_f64()
                        .or_else(|| middle[name].as_f64())
                        .unwrap_or(0.0)
                };
                let detail = format!(
                    "perimeter {:.6}, {} loops, {} edges",
                    face["signature"]["perimeter"].as_f64().unwrap_or(0.0),
                    face["signature"]["wire_count"],
                    face["signature"]["edge_count"]
                );
                summary
                    .flats
                    .push((area, [at(0, "x"), at(1, "y"), at(2, "z")], detail));
            }
            if !face["cylinder"].is_null() {
                summary.cylinders += 1;
            }
        }
        summary.edges = body["edges"].as_array().map_or(0, Vec::len);
        out.push(summary);
    }
    out.sort_by_key(|b| b.id);
    out
}

struct Run {
    outcome: Result<Vec<BodySummary>, String>,
    kernel_ns: u128,
    recomputes: usize,
    wall: Duration,
}

fn run(server: &Path, kernel: &str, script: &Script, scratch: &Path, keep: Option<&Path>) -> Run {
    let timing = scratch.join(format!("timing-{kernel}-{}.tsv", std::process::id()));
    let _ = std::fs::remove_file(&timing);
    let weigh = scratch.join(format!("weigh-{kernel}-{}.tsv", std::process::id()));
    let _ = std::fs::remove_file(&weigh);
    let mut client = Client::start(server, kernel, &timing, &weigh);
    let started = Instant::now();
    let arguments = match script {
        Script::Recipe(id) => json!({"action":"script","recipe":id,"mode":"fast","validate":true}),
        Script::File(path) => json!({
            "action":"script","mode":"fast","validate":true,
            "source": std::fs::read_to_string(path).unwrap_or_default(),
            "include_base": std::fs::canonicalize(path)
                .ok()
                .and_then(|p| p.parent().map(|d| d.display().to_string())),
        }),
    };
    let ran = client.call("cad_interface", arguments);
    let wall = started.elapsed();
    let scene = client.call("solid_scene", json!({}));
    // What the script took of the kernel's time, before anything is asked
    // of it on the lab's own account.
    let spent = std::fs::read_to_string(&timing).unwrap_or_default();
    // Every body weighed as a solid: the server's kernel does that after a
    // recompute that finds this file empty.
    let mut held: HashMap<u64, f64> = HashMap::new();
    if ran.is_ok() && std::fs::write(&weigh, b"").is_ok() {
        let _ = client.call("solid_recompute", json!({}));
        for line in std::fs::read_to_string(&weigh).unwrap_or_default().lines() {
            let mut fields = line.split('\t');
            if let (Some(Ok(id)), Some(Ok(volume))) = (
                fields.next().map(str::parse::<u64>),
                fields.next().map(str::parse::<f64>),
            ) {
                held.insert(id, volume);
            }
        }
    }
    // The plan that made the bodies, kept for whoever wants to make them
    // again a feature at a time and see where the kernels part.
    let mut plan = weigh.as_os_str().to_owned();
    plan.push(".plan.json");
    if let Some(keep) = keep {
        let _ = std::fs::create_dir_all(keep);
        let _ = std::fs::copy(
            &plan,
            keep.join(format!("{}.{kernel}.plan.json", script.name())),
        );
    }
    let _ = std::fs::remove_file(&plan);
    let _ = std::fs::remove_file(&weigh);
    drop(client);
    let mut kernel_ns = 0;
    let mut recomputes = 0;
    for line in spent.lines() {
        if let Some(ns) = line
            .rsplit('\t')
            .next()
            .and_then(|v| v.parse::<u128>().ok())
        {
            kernel_ns += ns;
            recomputes += 1;
        }
    }
    let _ = std::fs::remove_file(&timing);
    let outcome = match (ran, scene) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(_), Ok(scene)) => {
            let errors: Vec<String> = scene["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["message"].as_str().map(str::to_owned))
                .collect();
            if errors.is_empty() {
                let mut bodies = summarise(&scene);
                for body in &mut bodies {
                    body.held = held.get(&body.id).copied();
                }
                Ok(bodies)
            } else {
                Err(errors.join("; "))
            }
        }
    };
    Run {
        outcome,
        kernel_ns,
        recomputes,
        wall,
    }
}

enum Script {
    Recipe(String),
    File(PathBuf),
}

impl Script {
    fn name(&self) -> String {
        match self {
            Script::Recipe(id) => id.clone(),
            Script::File(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().replace(".limo.jsonc", ""))
                .unwrap_or_default(),
        }
    }
}

/// Area, middle and description of a flat face.
type Flat = (f64, [f64; 3], String);

/// Flat faces of one kernel with no equal in the other.
fn lonely<'a>(mine: &'a [Flat], theirs: &[Flat]) -> Vec<&'a Flat> {
    mine.iter()
        .filter(|(area, at, _)| {
            !theirs.iter().any(|(other, there, _)| {
                (area - other).abs() <= 1.0e-7 * area.abs().max(1.0)
                    && (0..3).all(|k| (at[k] - there[k]).abs() < 1.0e-4)
            })
        })
        .collect()
}

fn describe((area, at, detail): &Flat) -> String {
    format!(
        "{area:.6} at ({:.4}, {:.4}, {:.4}), {detail}",
        at[0], at[1], at[2]
    )
}

/// Bodies the two kernels are known not to hold the same amount of, where
/// it is the reference kernel that is out: the recipe, the body, what this
/// kernel holds more than the reference does, and why. Each has been worked
/// out by hand, and the working is a test.
const KNOWN: &[(&str, u64, f64, &str)] = &[
    (
        "show-me-external-thread",
        1,
        -0.000_116,
        "the reference fits splines to the flanks of a thread; what the groove takes is worked \
         out in a_thread_takes_what_its_groove_holds (crates/occt/tests/corners.rs)",
    ),
    (
        "turbine-fit-coupons",
        16,
        0.002_095,
        "the round on the slotted rim takes the 6.134232 worked out for such a rim in \
         a_rim_that_ends_on_both_walls_of_a_slot (tests/blend.rs); the reference takes more",
    ),
    (
        "vertical-axis-turbine",
        78,
        0.002_095,
        "the round on the slotted rim takes the 6.134232 worked out for such a rim in \
         a_rim_that_ends_on_both_walls_of_a_slot (tests/blend.rs); the reference takes more",
    ),
];

/// Differences between the reference and candidate scenes, empty when they
/// agree, and remarks on what agrees only nearly or differs for a reason
/// that is known.
fn compare(
    recipe: &str,
    reference: &[BodySummary],
    candidate: &[BodySummary],
) -> (Vec<String>, Vec<String>) {
    let mut issues = Vec::new();
    let mut notes = Vec::new();
    if reference.len() != candidate.len() {
        issues.push(format!(
            "{} bodies instead of {}",
            candidate.len(),
            reference.len()
        ));
        return (issues, notes);
    }
    for (r, c) in reference.iter().zip(candidate) {
        // Weighed as solids, the two have to hold the same to a part in a
        // hundred million. This is the test of whether the kernels made
        // the same thing; the rest of what follows compares drawings of
        // it, which can only show that they made something alike.
        if let (Some(theirs), Some(mine)) = (r.held, c.held) {
            if (theirs - mine).abs() > 1.0e-8 * theirs.abs().max(1.0) {
                let said = format!(
                    "body {} holds {mine:.6} where the reference holds {theirs:.6}: {:+.6}, {:+.1e} of it",
                    r.id,
                    mine - theirs,
                    (mine - theirs) / theirs
                );
                let known = KNOWN.iter().find(|(name, body, more, _)| {
                    *name == recipe && *body == r.id && (mine - theirs - more).abs() < 5.0e-6
                });
                match known {
                    Some((.., why)) => notes.push(format!("{said}; {why}")),
                    None => issues.push(said),
                }
            }
        }
        let scale = r.volume.abs().max(1.0);
        // Both meshes approximate curved faces from the inside or outside by
        // the display tolerance; planar bodies agree to rounding.
        let curved = r.cylinders > 0 || r.faces != r.planes;
        let tolerance = if curved { 2.0e-2 } else { 1.0e-6 };
        if (r.volume - c.volume).abs() > tolerance * scale {
            issues.push(format!(
                "body {} volume {:.4} instead of {:.4}",
                r.id, c.volume, r.volume
            ));
        }
        // A faceted circle reaches its radius only where a vertex happens to
        // point along the axis.
        let slack = if curved { 1.0e-2 } else { 1.0e-6 };
        for k in 0..3 {
            let span = (r.hi[k] - r.lo[k]).abs().max(1.0);
            if (r.lo[k] - c.lo[k]).abs() > slack * span || (r.hi[k] - c.hi[k]).abs() > slack * span
            {
                issues.push(format!("body {} bounds differ on axis {k}", r.id));
                break;
            }
        }
        // The reference kernel lofts straight-sided sections into ruled
        // surfaces and does not call them planes, flat as they are. Faces
        // this kernel knows to be flat count as the same when the number
        // of faces and their whole area agree.
        let flat_by_another_name = c.planes > r.planes
            && c.faces == r.faces
            && (r.area - c.area).abs() <= 1.0e-6 * r.area.abs().max(1.0);
        if flat_by_another_name {
            continue;
        }
        if r.planes != c.planes {
            issues.push(format!(
                "body {} has {} planar faces instead of {}",
                r.id, c.planes, r.planes
            ));
        } else if (r.plane_area - c.plane_area).abs() > 1.0e-6 * r.plane_area.abs().max(1.0) {
            // The reference walks along its blends in steps, and a flat
            // face that a blend ends on can come out a few parts in a
            // hundred thousand off (the face in `turbine-fit-coupons` that
            // first showed this has an area known in closed form, and it
            // is this kernel's). Such faces are remarked on, not failed:
            // each has to pair with one of the other kernel's in the same
            // place, with the same loops, and nearly the same area.
            let (theirs, mine) = (lonely(&r.flats, &c.flats), lonely(&c.flats, &r.flats));
            let near = |a: &Flat, b: &Flat| {
                (a.0 - b.0).abs() <= 1.0e-4 * a.0.abs().max(1.0)
                    && (0..3).all(|k| (a.1[k] - b.1[k]).abs() < 1.0e-2)
            };
            let paired = theirs.len() == mine.len()
                && theirs.len() <= 2
                && theirs.iter().all(|a| mine.iter().any(|b| near(a, b)));
            if paired {
                for (a, b) in theirs.iter().zip(&mine) {
                    notes.push(format!(
                        "body {}: a flat face of {:.6} where the reference has {:.6}",
                        r.id, b.0, a.0
                    ));
                }
            } else {
                issues.push(format!(
                    "body {} planar area {:.6} instead of {:.6}",
                    r.id, c.plane_area, r.plane_area
                ));
            }
        }
    }
    (issues, notes)
}

fn main() {
    let mut server: Option<PathBuf> = std::env::var_os("LIMO_CAD_MCP_BIN").map(PathBuf::from);
    let mut scripts: Vec<Script> = Vec::new();
    let mut all = false;
    let mut verbose = false;
    let mut plans: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => server = args.next().map(PathBuf::from),
            "--all" => all = true,
            "--verbose" => verbose = true,
            "--plans" => plans = args.next().map(PathBuf::from),
            other if other.ends_with(".jsonc") => scripts.push(Script::File(PathBuf::from(other))),
            other => scripts.push(Script::Recipe(other.to_owned())),
        }
    }
    let Some(server) = server else {
        eprintln!("usage: lab --server <limo-cad-mcp> [--all] [--verbose] [--plans <dir>] [recipe-id | script.limo.jsonc]...");
        std::process::exit(2);
    };
    if all {
        let mut found: Vec<PathBuf> = std::fs::read_dir("examples/scripts")
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with(".limo.jsonc"))
            .collect();
        found.sort();
        scripts.extend(found.into_iter().map(Script::File));
    }
    let scratch = std::env::temp_dir();
    println!(
        "{:<38} {:>7} {:>10} {:>10} {:>8}  result",
        "script", "bodies", "occt ms", "limo ms", "speedup"
    );
    let (mut same, mut different, mut unsupported, mut skipped) = (0, 0, 0, 0);
    let (mut occt_total, mut limo_total) = (0u128, 0u128);
    for script in &scripts {
        let reference = run(&server, "occt", script, &scratch, plans.as_deref());
        let name = script.name();
        let Ok(expected) = &reference.outcome else {
            skipped += 1;
            println!(
                "{name:<38} {:>7} {:>10} {:>10} {:>8}  reference failed: {}",
                "-",
                "-",
                "-",
                "-",
                reference
                    .outcome
                    .as_ref()
                    .err()
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap_or("")
            );
            continue;
        };
        let candidate = run(&server, "limo", script, &scratch, plans.as_deref());
        let occt_ms = reference.kernel_ns as f64 / 1.0e6;
        let limo_ms = candidate.kernel_ns as f64 / 1.0e6;
        let mut remarks = Vec::new();
        let verdict = match &candidate.outcome {
            Err(error) => {
                unsupported += 1;
                format!("limo: {}", error.lines().next().unwrap_or(""))
            }
            Ok(actual) => {
                let (issues, notes) = compare(&name, expected, actual);
                if issues.is_empty() {
                    same += 1;
                    occt_total += reference.kernel_ns;
                    limo_total += candidate.kernel_ns;
                    remarks = notes;
                    if remarks.is_empty() {
                        "match".to_owned()
                    } else {
                        "match, nearly".to_owned()
                    }
                } else {
                    different += 1;
                    format!("DIFFERENT: {}", issues.join("; "))
                }
            }
        };
        let speedup = if candidate.outcome.is_ok() && limo_ms > 0.0 {
            format!("{:.0}x", occt_ms / limo_ms)
        } else {
            "-".into()
        };
        println!(
            "{name:<38} {:>7} {occt_ms:>10.2} {limo_ms:>10.3} {speedup:>8}  {verdict}",
            expected.len()
        );
        for remark in &remarks {
            println!("    {remark}");
        }
        if verbose {
            println!(
                "    recomputes {} / {}, script wall time {:.0} ms / {:.0} ms",
                reference.recomputes,
                candidate.recomputes,
                reference.wall.as_secs_f64() * 1e3,
                candidate.wall.as_secs_f64() * 1e3
            );
            if let Ok(actual) = &candidate.outcome {
                for (r, c) in expected.iter().zip(actual) {
                    println!(
                        "    body {}: holds {} / {}, drawn {:.3} / {:.3}, faces {} / {}, edges {} / {}, triangles {} / {}",
                        r.id,
                        r.held.map_or("?".to_owned(), |v| format!("{v:.9}")),
                        c.held.map_or("?".to_owned(), |v| format!("{v:.9}")),
                        r.volume, c.volume, r.faces, c.faces, r.edges, c.edges, r.triangles, c.triangles
                    );
                    for face in lonely(&r.flats, &c.flats) {
                        println!(
                            "        only the reference has a flat face of {}",
                            describe(face)
                        );
                    }
                    for face in lonely(&c.flats, &r.flats) {
                        println!("        only Limo has a flat face of {}", describe(face));
                    }
                }
            }
        }
    }
    println!(
        "\n{same} match, {different} differ, {unsupported} not yet supported by the Limo kernel, {skipped} skipped"
    );
    if limo_total > 0 {
        println!(
            "kernel time over matching scripts: OCCT {:.1} ms, Limo {:.2} ms ({:.0}x)",
            occt_total as f64 / 1e6,
            limo_total as f64 / 1e6,
            occt_total as f64 / limo_total as f64
        );
    }
    if different > 0 {
        std::process::exit(1);
    }
}
