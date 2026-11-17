//! Differential kernel lab.
//!
//! Runs construction scripts through the real headless Limo CAD server once
//! per kernel and compares the resulting solids: body count, volume, bounds
//! and face make-up. Kernel time is read from the adapter's opt-in timing
//! log, so sketch solving and transport are excluded.
//!
//! ```sh
//! cargo run --release -p limo-cad-kernel --example lab -- \
//!     --server mcp-server/target/release/limo-cad-mcp fillet-basics extrude-basics
//! cargo run --release -p limo-cad-kernel --example lab -- --server <bin> --all
//! ```

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
    fn start(server: &Path, kernel: &str, timing: &Path) -> Client {
        let mut child = Command::new(server)
            .arg("--headless")
            .env("LIMO_CAD_KERNEL", kernel)
            .env("LIMO_CAD_KERNEL_TIMING", timing)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
            return Err(text.chars().take(600).collect());
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
    lo: [f64; 3],
    hi: [f64; 3],
    triangles: usize,
    faces: usize,
    planes: usize,
    cylinders: usize,
    edges: usize,
    plane_area: f64,
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
            .map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect())
            .unwrap_or_default();
        let point = |i: usize| [positions[3 * i], positions[3 * i + 1], positions[3 * i + 2]];
        let mut summary = BodySummary {
            id: body["id"].as_u64().unwrap_or(0),
            lo: [f64::INFINITY; 3],
            hi: [f64::NEG_INFINITY; 3],
            triangles: indices.len() / 3,
            ..Default::default()
        };
        for t in indices.chunks_exact(3) {
            let (a, b, c) = (point(t[0]), point(t[1]), point(t[2]));
            summary.volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
            for p in [a, b, c] {
                for k in 0..3 {
                    summary.lo[k] = summary.lo[k].min(p[k]);
                    summary.hi[k] = summary.hi[k].max(p[k]);
                }
            }
        }
        for face in body["faces"].as_array().into_iter().flatten() {
            summary.faces += 1;
            if !face["plane"].is_null() {
                summary.planes += 1;
                summary.plane_area += face["signature"]["area"].as_f64().unwrap_or(0.0);
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

fn run(server: &Path, kernel: &str, script: &Script, scratch: &Path) -> Run {
    let timing = scratch.join(format!("timing-{kernel}-{}.tsv", std::process::id()));
    let _ = std::fs::remove_file(&timing);
    let mut client = Client::start(server, kernel, &timing);
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
    drop(client);
    let mut kernel_ns = 0;
    let mut recomputes = 0;
    for line in std::fs::read_to_string(&timing).unwrap_or_default().lines() {
        if let Some(ns) = line.rsplit('\t').next().and_then(|v| v.parse::<u128>().ok()) {
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
                Ok(summarise(&scene))
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

/// Differences between the reference and candidate scenes, empty when they
/// agree within tessellation error.
fn compare(reference: &[BodySummary], candidate: &[BodySummary]) -> Vec<String> {
    let mut issues = Vec::new();
    if reference.len() != candidate.len() {
        issues.push(format!(
            "{} bodies instead of {}",
            candidate.len(),
            reference.len()
        ));
        return issues;
    }
    for (r, c) in reference.iter().zip(candidate) {
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
        if r.planes != c.planes {
            issues.push(format!(
                "body {} has {} planar faces instead of {}",
                r.id, c.planes, r.planes
            ));
        } else if (r.plane_area - c.plane_area).abs() > 1.0e-6 * r.plane_area.abs().max(1.0) {
            issues.push(format!(
                "body {} planar area {:.6} instead of {:.6}",
                r.id, c.plane_area, r.plane_area
            ));
        }
    }
    issues
}

fn main() {
    let mut server: Option<PathBuf> = std::env::var_os("LIMO_CAD_MCP_BIN").map(PathBuf::from);
    let mut scripts: Vec<Script> = Vec::new();
    let mut all = false;
    let mut verbose = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => server = args.next().map(PathBuf::from),
            "--all" => all = true,
            "--verbose" => verbose = true,
            other if other.ends_with(".jsonc") => scripts.push(Script::File(PathBuf::from(other))),
            other => scripts.push(Script::Recipe(other.to_owned())),
        }
    }
    let Some(server) = server else {
        eprintln!("usage: lab --server <limo-cad-mcp> [--all] [--verbose] [recipe-id | script.limo.jsonc]...");
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
        let reference = run(&server, "occt", script, &scratch);
        let name = script.name();
        let Ok(expected) = &reference.outcome else {
            skipped += 1;
            println!(
                "{name:<38} {:>7} {:>10} {:>10} {:>8}  reference failed: {}",
                "-",
                "-",
                "-",
                "-",
                reference.outcome.as_ref().err().unwrap().lines().next().unwrap_or("")
            );
            continue;
        };
        let candidate = run(&server, "limo", script, &scratch);
        let occt_ms = reference.kernel_ns as f64 / 1.0e6;
        let limo_ms = candidate.kernel_ns as f64 / 1.0e6;
        let verdict = match &candidate.outcome {
            Err(error) => {
                unsupported += 1;
                format!("limo: {}", error.lines().next().unwrap_or(""))
            }
            Ok(actual) => {
                let issues = compare(expected, actual);
                if issues.is_empty() {
                    same += 1;
                    occt_total += reference.kernel_ns;
                    limo_total += candidate.kernel_ns;
                    "match".to_owned()
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
                        "    body {}: volume {:.3} / {:.3}, faces {} / {}, edges {} / {}, triangles {} / {}",
                        r.id, r.volume, c.volume, r.faces, c.faces, r.edges, c.edges, r.triangles, c.triangles
                    );
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
