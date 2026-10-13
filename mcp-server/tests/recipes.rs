//! Exercise the authored recipes through the real MCP stdio binary. This is
//! only a transport/assertion harness; the Rust script interpreter builds CAD.
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Retain artifacts only when explicitly requested. Otherwise own a uniquely
/// created temporary directory, including cleanup during a failing assertion.
struct RecipeArtifacts {
    path: std::path::PathBuf,
    temporary: bool,
}
impl RecipeArtifacts {
    fn new() -> Self {
        if let Some(path) =
            std::env::var_os("LIMO_CAD_RECIPE_ARTIFACT_DIR").filter(|v| !v.is_empty())
        {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            return Self {
                path,
                temporary: false,
            };
        }
        Self::temporary()
    }
    fn temporary() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "limo-cad-recipe-{}-{epoch}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    return Self {
                        path,
                        temporary: true,
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("Cannot create recipe test temporary directory: {error}"),
            }
        }
        panic!("Cannot allocate a unique recipe test directory")
    }
}
impl Drop for RecipeArtifacts {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[test]
fn temporary_artifacts_remove_only_the_directory_they_own() {
    let first = RecipeArtifacts::temporary();
    let second = RecipeArtifacts::temporary();
    let owned = first.path.clone();
    std::fs::write(owned.join("checked-artifact"), b"fixture").unwrap();
    std::fs::write(second.path.join("separate-artifact"), b"keep").unwrap();
    drop(first);
    assert!(!owned.exists());
    assert!(second.path.join("separate-artifact").exists());
}

struct Client {
    child: Child,
    input: ChildStdin,
    replies: Receiver<Result<Value, String>>,
    id: u64,
    timeout: Duration,
    stage: String,
}
impl Client {
    fn start() -> Self {
        let binary = std::env::var_os("LIMO_CAD_RECIPE_MCP_BIN")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_limo-cad-mcp").into());
        let mut command = Command::new(binary);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines() {
                let reply = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str(&line).map_err(|error| error.to_string())
                });
                if sender.send(reply).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            replies,
            id: 0,

            timeout: Duration::from_secs(600),
            stage: "initialize MCP".into(),
        };
        client.rpc("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"recipe-regression","version":"1"}}));
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client.stage = "recipe acceptance".into();
        client
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.rpc_with_timeout(method, params, self.timeout)
    }
    fn rpc_with_timeout(&mut self, method: &str, params: Value, timeout: Duration) -> Value {
        self.id += 1;
        let started = Instant::now();
        let deadline = started + timeout;
        let request = request_summary(method, &params);
        self.record_request(&request, "pending", started, timeout, None);
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        match receive_request_reply(&self.replies, self.id, deadline) {
            Ok(reply) => {
                let error = reply
                    .get("error")
                    .map(|error| error.to_string().chars().take(2048).collect::<String>());
                self.record_request(
                    &request,
                    if error.is_some() {
                        "failed"
                    } else {
                        "completed"
                    },
                    started,
                    timeout,
                    error.as_deref(),
                );
                if started.elapsed() >= Duration::from_secs(1) {
                    eprintln!(
                        "MCP [{}] {} completed in {:.2}s",
                        self.stage,
                        request,
                        started.elapsed().as_secs_f64()
                    );
                }
                assert!(
                    reply.get("error").is_none(),
                    "stage {}: {request}: {reply}",
                    self.stage
                );
                reply["result"].clone()
            }
            Err(error) => {
                self.record_request(&request, "failed", started, timeout, Some(&error));
                panic!("MCP stage '{}' did not finish request {} (child {}, elapsed {:.2}s, limit {}s): {request}: {error}",
                    self.stage, self.id, self.child.id(), started.elapsed().as_secs_f64(), timeout.as_secs());
            }
        }
    }
    fn stage(&mut self, stage: impl Into<String>) {
        self.stage = stage.into();
        eprintln!("MCP child {}: {}", self.child.id(), self.stage);
    }
    fn record_request(
        &self,
        request: &str,
        status: &str,
        started: Instant,
        timeout: Duration,
        error: Option<&str>,
    ) {
        let Some(directory) =
            std::env::var_os("LIMO_CAD_RECIPE_ARTIFACT_DIR").filter(|value| !value.is_empty())
        else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        let result = std::fs::create_dir_all(&directory).and_then(|_| {
            std::fs::write(
                directory.join(format!("last-request-{}.json", self.child.id())),
                serde_json::to_vec_pretty(&json!({
                    "test":std::thread::current().name(), "child_pid":self.child.id(),
                    "stage":self.stage, "request_id":self.id, "request":request,
                    "status":status, "elapsed_ms":started.elapsed().as_millis(),
                    "deadline_seconds":timeout.as_secs(), "error":error,
                }))
                .unwrap(),
            )
        });
        if let Err(error) = result {
            eprintln!("Could not retain MCP request diagnostic: {error}");
        }
    }
    fn call(&mut self, operation: &str, arguments: Value) -> Value {
        self.call_with_timeout(operation, arguments, self.timeout)
    }
    fn call_with_timeout(&mut self, operation: &str, arguments: Value, timeout: Duration) -> Value {
        let reply = self.rpc_with_timeout(
            "tools/call",
            json!({"name":operation,"arguments":arguments}),
            timeout,
        );
        if reply["isError"] == true && operation == "cad_interface" {
            eprintln!("Recipe failed: {}", reply["content"]);
            let artifacts = RecipeArtifacts::new();
            let directory = &artifacts.path;
            let active = self.call("sketch_active", json!({}));
            std::fs::write(
                directory.join("failed-active-sketch.json"),
                serde_json::to_vec_pretty(&active).unwrap(),
            )
            .unwrap();
            for (name, operation) in [
                ("failed-model", "cad_project_model"),
                ("failed-scene", "solid_scene"),
                ("failed-drawings", "drawing_document"),
            ] {
                if operation == "cad_project_model" && !active.is_null() {
                    continue;
                }
                let value = self.call(operation, json!({}));
                std::fs::write(
                    directory.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(&value).unwrap(),
                )
                .unwrap();
            }
            if !artifacts.temporary {
                eprintln!(
                    "Failed recipe diagnostics retained in {}",
                    directory.display()
                );
            } else {
                eprintln!("Set LIMO_CAD_RECIPE_ARTIFACT_DIR to retain recipe diagnostics.");
            }
        }
        assert_ne!(reply["isError"], true, "{operation}: {}", reply["content"]);
        let text = reply["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "text")
            .unwrap()["text"]
            .as_str()
            .unwrap();
        serde_json::from_str(text).unwrap()
    }
    fn recipe(&mut self, id: &str) -> Value {
        self.call_with_timeout(
            "cad_interface",
            json!({"action":"script","recipe":id,"mode":"fast","validate":true}),
            recipe_timeout(id, self.timeout),
        )
    }
    fn compiled_recipe_source(&mut self, id: &str) -> Value {
        self.call_with_timeout(
            "cad_interface",
            json!({"action":"script",
            "source":limo_cad_recipes::find(id).unwrap().source,"mode":"fast","validate":true}),
            recipe_timeout(id, self.timeout),
        )
    }
    fn restore(model: &Value) -> Self {
        let mut client = Self::start();
        client.stage("restore saved native model");
        client.call(
            "cad_load_project_model",
            json!({"model_json":model.as_str().map(str::to_owned).unwrap_or_else(||serde_json::to_string(model).unwrap())}),
        );
        client
    }
}

fn recipe_timeout(id: &str, operation_timeout: Duration) -> Duration {
    match id {
        "d-screw-vise" | "vertical-axis-turbine" => operation_timeout.max(Duration::from_secs(900)),
        _ => operation_timeout,
    }
}

fn receive_request_reply(
    replies: &Receiver<Result<Value, String>>,
    id: u64,
    deadline: Instant,
) -> Result<Value, String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("absolute request deadline exceeded".into());
        }
        let reply = replies
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => "absolute request deadline exceeded".to_owned(),
                mpsc::RecvTimeoutError::Disconnected => {
                    "MCP stdout closed before the response".to_owned()
                }
            })??;
        if reply["id"] == id {
            return Ok(reply);
        }
    }
}

fn request_summary(method: &str, params: &Value) -> String {
    let mut details = json!({"method":method});
    if let Some(name) = params.get("name") {
        details["operation"] = name.clone();
    }
    if let Some(arguments) = params.get("arguments").and_then(Value::as_object) {
        let mut summary = serde_json::Map::new();
        for key in [
            "action",
            "recipe",
            "name",
            "feature_id",
            "sheet_id",
            "joint_id",
            "occurrence_id",
            "occurrence_ids",
            "constraint_id",
            "text",
            "format",
            "clearance_threshold_mm",
            "angle_offset_deg",
            "linear_offset_mm",
        ] {
            if let Some(value) = arguments.get(key) {
                summary.insert(key.into(), value.clone());
            }
        }
        for key in ["source", "model_json", "data_base64", "bytes_base64"] {
            if let Some(value) = arguments.get(key).and_then(Value::as_str) {
                summary.insert(format!("{key}_bytes"), json!(value.len()));
            }
        }
        details["arguments"] = Value::Object(summary);
    }

    serde_json::to_string(&details)
        .unwrap()
        .chars()
        .take(2048)
        .collect()
}

#[test]
fn flagship_recipe_deadlines_do_not_change_ordinary_operation_budgets() {
    let ordinary = Duration::from_secs(600);
    for id in ["d-screw-vise", "vertical-axis-turbine"] {
        assert_eq!(recipe_timeout(id, ordinary), Duration::from_secs(900));
        let longer = Duration::from_secs(1200);
        assert_eq!(recipe_timeout(id, longer), longer);
    }
    for id in ["d-screw-vise-fit", "turbine-fit-coupons", "garden-bench"] {
        assert_eq!(recipe_timeout(id, ordinary), ordinary);
    }
}

#[test]
fn request_deadline_is_not_extended_by_notifications() {
    let (sender, replies) = mpsc::channel();
    let deadline = Instant::now() + Duration::from_millis(60);
    let worker = std::thread::spawn(move || {
        for _ in 0..100 {
            if sender
                .send(Ok(
                    json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}),
                ))
                .is_err()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let error = receive_request_reply(&replies, 1, deadline).unwrap_err();
    assert!(error.contains("deadline"), "{error}");
    drop(replies);
    worker.join().unwrap();
}

#[test]
fn request_diagnostics_name_the_operation_without_copying_model_or_source() {
    let summary = request_summary(
        "tools/call",
        &json!({"name":"cad_interface","arguments":{
        "action":"script", "recipe":"d-screw-vise", "source":"private source".repeat(10000)}}),
    );
    assert!(summary.contains("cad_interface") && summary.contains("d-screw-vise"));
    assert!(summary.contains("source_bytes") && !summary.contains("private source"));
    assert!(summary.len() < 512);
}

#[test]
fn request_reply_distinguishes_completion_eof_and_invalid_output() {
    let (sender, replies) = mpsc::channel();
    sender
        .send(Ok(json!({"id":2,"result":"unrelated"})))
        .unwrap();
    sender
        .send(Ok(json!({"id":1,"result":"expected"})))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    assert_eq!(
        receive_request_reply(&replies, 1, deadline).unwrap()["result"],
        "expected"
    );
    sender.send(Err("invalid JSON on stdout".into())).unwrap();
    assert_eq!(
        receive_request_reply(&replies, 1, deadline).unwrap_err(),
        "invalid JSON on stdout"
    );
    drop(sender);
    assert!(receive_request_reply(&replies, 1, deadline)
        .unwrap_err()
        .contains("stdout closed"));
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn mesh_measurement(body: &Value) -> ([f64; 3], [f64; 3], f64) {
    let positions = body["mesh"]["positions"].as_array().unwrap();
    let indices = body["mesh"]["indices"].as_array().unwrap();
    assert!(!positions.is_empty() && positions.len().is_multiple_of(3));
    assert!(!indices.is_empty() && indices.len().is_multiple_of(3));
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for (i, coordinate) in positions.iter().enumerate() {
        let value = coordinate.as_f64().unwrap();
        assert!(value.is_finite());
        min[i % 3] = min[i % 3].min(value);
        max[i % 3] = max[i % 3].max(value);
    }
    let mut volume = 0.;
    for triangle in indices.as_chunks::<3>().0 {
        let point = |index: &Value| {
            let start = index.as_u64().unwrap() as usize * 3;
            assert!(start + 2 < positions.len());
            [
                positions[start].as_f64().unwrap(),
                positions[start + 1].as_f64().unwrap(),
                positions[start + 2].as_f64().unwrap(),
            ]
        };
        let [a, b, c] = [
            point(&triangle[0]),
            point(&triangle[1]),
            point(&triangle[2]),
        ];
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.;
    }
    (min, max, volume.abs())
}

fn part_geometry(
    scene: &Value,
    min: [f64; 3],
    max: [f64; 3],
    volume: f64,
    relative_tolerance: f64,
) {
    assert_eq!(scene["errors"], json!([]));
    let bodies = scene["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 1);
    let (actual_min, actual_max, actual_volume) = mesh_measurement(&bodies[0]);
    for axis in 0..3 {
        assert!(
            (actual_min[axis] - min[axis]).abs() < 0.1,
            "min: {actual_min:?}"
        );
        assert!(
            (actual_max[axis] - max[axis]).abs() < 0.1,
            "max: {actual_max:?}"
        );
    }
    assert!(
        (actual_volume - volume).abs() / volume < relative_tolerance,
        "measured {actual_volume}, analytic {volume}"
    );
}

fn three_mf_geometry(bytes: &[u8], min: [f64; 3], max: [f64; 3], volume: f64, tolerance: f64) {
    let meshes = limo_cad_export::test_reader::read_package(bytes)
        .expect("decode actual 3MF build geometry and component transforms");
    let bodies = meshes
        .into_iter()
        .map(|mesh| {
            json!({"mesh": {
                "positions": mesh.vertices.into_iter().flatten().collect::<Vec<_>>(),
                "indices": mesh.triangles.into_iter().flatten().collect::<Vec<_>>()
            }})
        })
        .collect::<Vec<_>>();
    part_geometry(
        &json!({"errors":[],"bodies":bodies}),
        min,
        max,
        volume,
        tolerance,
    );
}

fn stl_mesh(bytes: &[u8]) -> Value {
    let mut positions = Vec::new();
    if bytes.starts_with(b"solid ") {
        let text = std::str::from_utf8(bytes).expect("ASCII STL must be UTF-8");
        let body = text
            .strip_prefix("solid LimoCAD\n")
            .and_then(|text| text.strip_suffix("endsolid LimoCAD\n"))
            .expect("ASCII STL must have a complete solid envelope");
        let lines: Vec<_> = body.lines().map(str::trim).collect();
        assert!(!lines.is_empty() && lines.len().is_multiple_of(7));
        let vector = |line: &str, prefix: &str| {
            let numbers: Vec<f64> = line
                .strip_prefix(prefix)
                .expect("STL facet record")
                .split_whitespace()
                .map(|number| number.parse().expect("STL coordinate"))
                .collect();
            assert_eq!(numbers.len(), 3);
            assert!(numbers.iter().all(|number| number.is_finite()));
            numbers
        };
        for facet in lines.as_chunks::<7>().0 {
            let normal = vector(facet[0], "facet normal ");
            assert!((normal.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-4);
            assert_eq!(facet[1], "outer loop");
            for vertex in &facet[2..5] {
                positions.extend(vector(vertex, "vertex "));
            }
            assert_eq!(facet[5], "endloop");
            assert_eq!(facet[6], "endfacet");
        }
    } else {
        assert!(bytes.len() >= 84, "binary STL header");
        let triangles = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert!(triangles > 0);
        assert_eq!(bytes.len(), 84 + 50 * triangles);
        for facet in bytes[84..].as_chunks::<50>().0 {
            let values: Vec<_> = facet[..48]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|value| f64::from(f32::from_le_bytes(*value)))
                .collect();
            assert!(values.iter().all(|value| value.is_finite()));
            assert!((values[..3].iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-4);
            positions.extend_from_slice(&values[3..]);
        }
    }
    let indices: Vec<_> = (0..positions.len() / 3).collect();
    json!({"mesh":{"positions":positions,"indices":indices}})
}

#[test]
fn native_part_recipes_preserve_analytic_geometry_restore_and_export() {
    for (id, min, max, volume, tolerance) in [
        (
            "mounting-plate",
            [-30., -20., 0.],
            [30., 20., 5.],
            60. * 40. * 5. - 4. * std::f64::consts::PI * 2.5_f64.powi(2) * 5.,
            0.001,
        ),
        (
            "revolved-spacer",
            [-10., 0., -10.],
            [10., 12., 10.],
            std::f64::consts::PI * (10_f64.powi(2) - 5_f64.powi(2)) * 12.,
            0.01,
        ),
        (
            "angle-bracket",
            [0., 0., 0.],
            [40., 30., 20.],
            (40. * 5. + 5. * 25.) * 20.,
            0.001,
        ),
    ] {
        let mut client = Client::start();
        let report = client.recipe(id);
        let exports = &report["exports"];
        part_geometry(&exports["final_scene"], min, max, volume, tolerance);
        assert_eq!(exports["final_sketches"][0]["dof"]["value"], 0, "{id}");
        assert!(exports["final_model"]["document"]["history"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|feature| feature["kind"] != "import_step"));
        let mut replay = Client::start();
        let repeated = replay.recipe(id);
        for key in [
            "final_model",
            "final_scene",
            "final_sketches",
            "final_solution",
        ] {
            assert_eq!(
                exports[key], repeated["exports"][key],
                "{id}: independent replay {key}"
            );
        }
        let mut restored = Client::restore(&exports["final_model"]);
        part_geometry(
            &restored.call("solid_scene", json!({})),
            min,
            max,
            volume,
            tolerance,
        );
        for format in ["step", "stl", "3mf"] {
            let exported = client.call(
                &format!("solid_export_{format}"),
                if format == "3mf" {
                    json!({"slicer_target":"standard"})
                } else {
                    json!({})
                },
            );
            assert_eq!(exported["format"], format);
            assert_eq!(exported["encoding"], "base64");
            let bytes = BASE64
                .decode(exported["bytes_base64"].as_str().unwrap())
                .unwrap();
            assert!(bytes.len() > 100);
            match format {
                "step" => {
                    let text = std::str::from_utf8(&bytes).unwrap();
                    assert!(text.contains("ISO-10303-21;") && text.contains("END-ISO-10303-21;"));
                    let mut imported = Client::start();
                    imported.call("solid_import_step", json!({"file_name":format!("{id}.step"),"data_base64":BASE64.encode(&bytes)}));
                    part_geometry(
                        &imported.call("solid_scene", json!({})),
                        min,
                        max,
                        volume,
                        tolerance,
                    );
                }
                "stl" => {
                    // Native f64 coordinates use lossless ASCII whenever the
                    // binary STL f32 representation would change geometry.
                    part_geometry(
                        &json!({"errors":[],"bodies":[stl_mesh(&bytes)]}),
                        min,
                        max,
                        volume,
                        tolerance,
                    );
                }
                "3mf" => {
                    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
                    let mut model = String::new();
                    archive
                        .by_name("3D/3dmodel.model")
                        .unwrap()
                        .read_to_string(&mut model)
                        .unwrap();
                    assert!(
                        model.contains("unit=\"millimeter\"")
                            && model.contains("<triangle ")
                            && model.contains("<build>")
                    );
                    three_mf_geometry(&bytes, min, max, volume, tolerance);
                }
                _ => unreachable!(),
            }
        }
    }
}

fn assembly_geometry(
    scene: &Value,
    solution: &Value,
    assembly: &Value,
    body_id: &Value,
    width: f64,
) {
    assert_eq!(scene["errors"], json!([]));
    assert_eq!(scene["bodies"].as_array().unwrap().len(), 3);
    let bracket = scene["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == *body_id)
        .unwrap();
    let (min, max, volume) = mesh_measurement(bracket);
    assert!((max[2] - min[2] - width).abs() < 1e-5);
    assert!((volume - 325. * width).abs() < 0.01);
    assert_eq!(
        assembly["component_structure"]["definitions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        assembly["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(assembly["joints"].as_array().unwrap().len(), 3);
    assert_eq!(solution["solved"], true);
    assert_eq!(solution["diagnostics"], json!([]));
    let poses: Vec<_> = solution["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|pose| pose["body_id"] == *body_id)
        .collect();
    assert_eq!(poses.len(), 2);
    for (pose, expected) in poses.iter().zip([[-25., -15., 5.], [25., 15., 5.]]) {
        for (axis, coordinate) in expected.iter().enumerate() {
            assert!(
                (pose["translation"][axis].as_f64().unwrap() - coordinate).abs() < 1e-5,
                "{pose}"
            );
        }
        assert!(
            pose["rotation"][2].as_f64().unwrap().abs() > 0.999
                || pose["rotation"][3].as_f64().unwrap().abs() > 0.999
        );
    }
}

#[test]
fn repeated_brackets_edit_one_definition_and_restore_in_fresh_processes() {
    let mut client = Client::start();
    let report = client.recipe("repeated-bracket-assembly");
    let exports = &report["exports"];
    let body = &exports["bracket_body_id"];
    for (model, width) in [
        (&exports["before_edit_model"], 20.),
        (&exports["final_model"], 25.),
    ] {
        let mut restored = Client::restore(model);
        assembly_geometry(
            &restored.call("solid_scene", json!({})),
            &restored.call("assembly_solution", json!({})),
            &restored.call("assembly_document", json!({})),
            body,
            width,
        );
        let sketches = restored.call("sketch_finished", json!({}));
        assert_eq!(sketches.as_array().unwrap().len(), 3);
        assert!(sketches
            .as_array()
            .unwrap()
            .iter()
            .all(|sketch| sketch["dof"]["value"] == 0));
    }
    assembly_geometry(
        &exports["final_scene"],
        &exports["final_solution"],
        &exports["final_assembly"],
        body,
        25.,
    );
    let mut repeated = Client::start();
    let comparison = repeated.recipe("repeated-bracket-assembly");
    for key in [
        "final_model",
        "final_scene",
        "final_sketches",
        "final_solution",
    ] {
        assert_eq!(
            exports[key], comparison["exports"][key],
            "independent assembly replay {key}"
        );
    }
}

#[test]
fn recipe_discovery_is_shared_and_does_not_execute_construction() {
    let mut client = Client::start();
    let catalog = client.call("cad_interface", json!({"action":"recipes"}));
    assert_eq!(catalog, limo_cad_recipes::catalog(false));
    assert!(catalog
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry.get("source").is_none()));
    assert_eq!(client.call("solid_scene", json!({}))["bodies"], json!([]));
}

#[path = "recipes/vise.rs"]
mod vise;

fn assert_same_json(actual: &Value, expected: &Value, label: &str) {
    fn difference(actual: &Value, expected: &Value, path: String) -> Option<String> {
        if actual == expected {
            return None;
        }
        match (actual, expected) {
            (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
                .iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (a, b))| difference(a, b, format!("{path}/{i}"))),
            (Value::Object(a), Value::Object(b)) if a.len() == b.len() => {
                a.iter().find_map(|(key, value)| {
                    difference(
                        value,
                        b.get(key).unwrap_or(&Value::Null),
                        format!("{path}/{key}"),
                    )
                })
            }
            (a, b) => Some(format!(
                "{path}: actual {}, expected {}",
                a.to_string().chars().take(200).collect::<String>(),
                b.to_string().chars().take(200).collect::<String>()
            )),
        }
    }
    assert!(
        actual == expected,
        "{label}: {}",
        difference(actual, expected, String::new()).unwrap_or_default()
    );
}

fn geometry_restore_residual(actual: &Value, expected: &Value, path: String) -> (f64, String) {
    if actual == expected {
        return (0., path);
    }
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) if a.is_f64() || b.is_f64() => {
            ((a.as_f64().unwrap() - b.as_f64().unwrap()).abs(), path)
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .map(|(i, (a, b))| geometry_restore_residual(a, b, format!("{path}/{i}")))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or((0., path)),
        (Value::Object(a), Value::Object(b)) if a.len() == b.len() => a
            .iter()
            .map(|(key, value)| {
                geometry_restore_residual(
                    value,
                    b.get(key).unwrap_or(&Value::Null),
                    format!("{path}/{key}"),
                )
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or((0., path)),
        _ => panic!("edit/restore changed topology or non-numeric metadata at {path}"),
    }
}

fn restored_geometry_residual(actual: &Value, expected: &Value, path: String) -> (f64, String) {
    let mut actual_metadata = actual.clone();
    let mut expected_metadata = expected.clone();
    for (actual_body, expected_body) in actual
        .as_array()
        .unwrap()
        .iter()
        .zip(expected.as_array().unwrap())
    {
        let actual_mesh = &actual_body["mesh"];
        let expected_mesh = &expected_body["mesh"];
        if actual_mesh != expected_mesh {
            assert_equivalent_mesh(actual_mesh, expected_mesh);
            let (a_min, a_max, a_volume) = mesh_measurement(actual_body);
            let (b_min, b_max, b_volume) = mesh_measurement(expected_body);
            for axis in 0..3 {
                assert!((a_min[axis] - b_min[axis]).abs() < 1e-6);
                assert!((a_max[axis] - b_max[axis]).abs() < 1e-6);
            }
            assert!(
                (a_volume - b_volume).abs() < 1e-6,
                "restored mesh changed enclosed volume"
            );
        }
    }
    for bodies in [&mut actual_metadata, &mut expected_metadata] {
        for body in bodies.as_array_mut().unwrap() {
            body.as_object_mut().unwrap().remove("mesh");
        }
    }
    geometry_restore_residual(&actual_metadata, &expected_metadata, path)
}

fn assert_equivalent_mesh(actual: &Value, expected: &Value) {
    use std::collections::{BTreeMap, BTreeSet};
    type Vertex = [i64; 3];
    type Triangle = [Vertex; 3];
    type Boundary = BTreeMap<([i64; 4], Vertex, Vertex), i32>;
    let quantize = |v: &[Value; 3]| -> Vertex {
        std::array::from_fn(|i| (v[i].as_f64().unwrap() * 1e6).round() as i64)
    };
    let triangles = |mesh: &Value| -> (BTreeMap<Triangle, usize>, BTreeSet<Vertex>) {
        let vertices: Vec<_> = mesh["positions"]
            .as_array()
            .unwrap()
            .as_chunks::<3>()
            .0
            .iter()
            .map(quantize)
            .collect();
        let normals = mesh["normals"].as_array().unwrap();
        assert_eq!(vertices.len() * 3, normals.len());

        for normal in normals.as_chunks::<3>().0 {
            let norm = normal
                .iter()
                .map(|v| v.as_f64().unwrap().powi(2))
                .sum::<f64>();
            assert!(norm.is_finite() && (norm - 1.).abs() < 1e-5);
        }
        let mut triangles = BTreeMap::new();
        for indices in mesh["indices"].as_array().unwrap().as_chunks::<3>().0 {
            let points: Triangle =
                std::array::from_fn(|i| vertices[indices[i].as_u64().unwrap() as usize]);
            let triangle = (0..3)
                .map(|shift| std::array::from_fn(|i| points[(i + shift) % 3]))
                .min()
                .unwrap();
            *triangles.entry(triangle).or_default() += 1;
        }
        (triangles, vertices.into_iter().collect())
    };
    let (mut actual_triangles, actual_vertices) = triangles(actual);
    let (mut expected_triangles, expected_vertices) = triangles(expected);
    assert_eq!(
        actual_triangles.values().sum::<usize>(),
        expected_triangles.values().sum::<usize>(),
        "restored triangulation changed triangle count"
    );
    assert!(
        actual_vertices == expected_vertices,
        "restored vertex positions changed at 1e-6 mm resolution"
    );
    for (triangle, count) in &mut actual_triangles {
        if let Some(other) = expected_triangles.get_mut(triangle) {
            let common = (*count).min(*other);
            *count -= common;
            *other -= common;
        }
    }
    let boundary = |triangles: &BTreeMap<Triangle, usize>| -> Boundary {
        let mut result = BTreeMap::new();
        for (triangle, count) in triangles.iter().filter(|(_, count)| **count > 0) {
            let a: [f64; 3] = std::array::from_fn(|i| triangle[0][i] as f64 / 1e6);
            let u: [f64; 3] =
                std::array::from_fn(|i| (triangle[1][i] - triangle[0][i]) as f64 / 1e6);
            let v: [f64; 3] =
                std::array::from_fn(|i| (triangle[2][i] - triangle[0][i]) as f64 / 1e6);
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = cross.iter().map(|x| x * x).sum::<f64>().sqrt();
            assert!(length > 1e-12, "degenerate restored triangle");
            let n = cross.map(|x| x / length);
            let plane = [
                (n[0] * 1e6).round() as i64,
                (n[1] * 1e6).round() as i64,
                (n[2] * 1e6).round() as i64,
                ((n[0] * a[0] + n[1] * a[1] + n[2] * a[2]) * 1e5).round() as i64,
            ];
            for i in 0..3 {
                let (start, end) = (triangle[i], triangle[(i + 1) % 3]);
                let (low, high, sign) = if start < end {
                    (start, end, 1)
                } else {
                    (end, start, -1)
                };
                *result.entry((plane, low, high)).or_default() += sign * *count as i32;
            }
        }
        result.retain(|_, count| *count != 0);
        result
    };
    assert!(
        boundary(&actual_triangles) == boundary(&expected_triangles),
        "restored triangulation changed an oriented surface boundary"
    );
}

#[test]
fn mesh_equivalence_accepts_diagonal_flip_but_rejects_changed_surface() {
    let first = json!({"positions":[0,0,0, 2,0,0, 2,3,0, 0,3,0],
        "normals":[0,0,1, 0,0,1, 0,0,1, 0,0,1],"indices":[0,1,2,0,2,3]});
    let mut alternate = first.clone();
    alternate["indices"] = json!([0, 1, 3, 1, 2, 3]);
    assert_equivalent_mesh(&first, &alternate);
    let mut lifted = alternate.clone();
    lifted["positions"][11] = json!(0.1);
    assert!(std::panic::catch_unwind(|| assert_equivalent_mesh(&first, &lifted)).is_err());
    alternate["indices"] = json!([0, 3, 1, 1, 3, 2]);
    assert!(std::panic::catch_unwind(|| assert_equivalent_mesh(&first, &alternate)).is_err());
}

fn no_overlap(report: &Value) {
    assert_eq!(report["exact"], true);
    for pair in report["pairs"].as_array().unwrap() {
        assert_eq!(pair["interfering"], false, "{pair}");
        assert!(
            pair["overlap_volume_mm3"].as_f64().unwrap() < 1e-6,
            "{pair}"
        );
    }
}

fn write_native_project(path: &std::path::Path, model: &Value) {
    assert_eq!(model["format"], "limo-cad-project");
    let manifest = json!({"format":"limo-cad-project","container_version":1,"model":"model.json","model_schema_version":model["schema_version"],"application":"Limo CAD","application_version":env!("CARGO_PKG_VERSION"),"saved_at":"1970-01-01T00:00:00Z"});

    let mut archive = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, value) in [("manifest.json", &manifest), ("model.json", model)] {
        archive.start_file(name, options).unwrap();
        archive
            .write_all(&serde_json::to_vec_pretty(value).unwrap())
            .unwrap();
    }
    archive.finish().unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let roundtrip: Value = serde_json::from_reader(archive.by_name("model.json").unwrap()).unwrap();
    assert_same_json(&roundtrip, model, "native project ZIP payload");
}

/// Inspect the emitted package, including edge incidence of each welded
/// triangle mesh. Actual print geometry must be closed, positive and on-bed.
fn validate_print_3mf(export: &Value, count: usize, bed: [f64; 3], path: &std::path::Path) {
    let bytes = BASE64
        .decode(export["bytes_base64"].as_str().unwrap())
        .unwrap();
    std::fs::write(path, &bytes).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("3D/3dmodel.model")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("unit=\"millimeter\""));
    let mut bounds = Vec::new();
    for mesh in limo_cad_export::test_reader::read_package(&bytes)
        .expect("read the emitted 3MF build in world coordinates")
    {
        let vertices = mesh.vertices;
        assert!(!vertices.is_empty());
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for point in &vertices {
            for axis in 0..3 {
                assert!(point[axis].is_finite());
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
            }
        }
        assert!(min[2].abs() < 1e-4, "part is not on print bed: {min:?}");
        for axis in 0..3 {
            assert!(
                min[axis] >= -1e-4 && max[axis] <= bed[axis] + 1e-4,
                "outside bed: {min:?}..{max:?}"
            );
        }
        let mut edges = std::collections::BTreeMap::<(usize, usize), (usize, i32)>::new();
        let mut signed_volume = 0.;
        for indices in mesh.triangles {
            assert!(indices.iter().all(|index| *index < vertices.len()));
            let [a, b, c] = indices.map(|index| vertices[index]);
            signed_volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.;
            for [a, b] in [
                [indices[0], indices[1]],
                [indices[1], indices[2]],
                [indices[2], indices[0]],
            ] {
                assert_ne!(a, b);
                let edge = edges.entry((a.min(b), a.max(b))).or_default();
                edge.0 += 1;
                edge.1 += if a < b { 1 } else { -1 };
            }
        }
        assert!(
            signed_volume.is_finite() && signed_volume > 1.,
            "non-positive print volume {signed_volume}"
        );
        assert!(
            !edges.is_empty() && edges.values().all(|incidence| *incidence == (2, 0)),
            "non-manifold print mesh"
        );
        bounds.push((min, max));
    }
    assert_eq!(bounds.len(), count);
    for (i, (min, max)) in bounds.iter().enumerate() {
        for (other_min, other_max) in &bounds[i + 1..] {
            assert!(
                (0..2).any(|axis| max[axis] < other_min[axis] || other_max[axis] < min[axis]),
                "print bodies overlap"
            );
        }
    }
}

#[test]
fn print_acceptance_checks_the_build_pose_and_rejects_an_off_bed_build() {
    let artifacts = RecipeArtifacts::temporary();
    let package = |transform: &str| {
        let xml = format!(
            r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><object id="1"><mesh><vertices>
<vertex x="0" y="0" z="43"/><vertex x="3" y="0" z="43"/>
<vertex x="0" y="2" z="43"/><vertex x="0" y="0" z="48"/>
</vertices><triangles>
<triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/>
<triangle v1="0" v2="3" v3="2"/><triangle v1="1" v2="2" v3="3"/>
</triangles></mesh></object><object id="2"><components><component objectid="1"/></components></object></resources><build><item objectid="2" transform="{transform}"/></build></model>"#
        );
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(xml.as_bytes()).unwrap();
        json!({"bytes_base64":BASE64.encode(archive.finish().unwrap().into_inner())})
    };
    validate_print_3mf(
        &package("1 0 0 0 1 0 0 0 1 10 20 -43"),
        1,
        [256.; 3],
        &artifacts.path.join("on-bed.3mf"),
    );
    assert!(std::panic::catch_unwind(|| {
        validate_print_3mf(
            &package("1 0 0 0 1 0 0 0 1 10 20 0"),
            1,
            [256.; 3],
            &artifacts.path.join("off-bed.3mf"),
        );
    })
    .is_err());
}

#[test]
fn three_mf_part_acceptance_rejects_empty_build_and_wrong_component_placement() {
    let package = |item: &str| {
        let xml = format!(
            r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><object id="1"><mesh><vertices>
<vertex x="0" y="0" z="43"/><vertex x="3" y="0" z="43"/>
<vertex x="0" y="2" z="43"/><vertex x="0" y="0" z="48"/>
</vertices><triangles><triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/>
<triangle v1="0" v2="3" v3="2"/><triangle v1="1" v2="2" v3="3"/>
</triangles></mesh></object><object id="2"><components><component objectid="1"/></components></object></resources><build>{item}</build></model>"#
        );
        // Every negative fixture still passes the historical XML-presence checks.
        assert!(
            xml.contains("unit=\"millimeter\"")
                && xml.contains("<triangle ")
                && xml.contains("<build>")
        );
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(xml.as_bytes()).unwrap();
        archive.finish().unwrap().into_inner()
    };
    let placed = package(r#"<item objectid="2" transform="1 0 0 0 1 0 0 0 1 10 20 -43"/>"#);
    three_mf_geometry(&placed, [10., 20., 0.], [13., 22., 5.], 5., 0.0001);
    for incorrect in [package(""), package(r#"<item objectid="2"/>"#)] {
        assert!(std::panic::catch_unwind(|| three_mf_geometry(
            &incorrect,
            [10., 20., 0.],
            [13., 22., 5.],
            5.,
            0.0001
        ))
        .is_err());
    }
}

#[test]
fn d_screw_vise_coupon_replays_real_threads_and_exports_printable_meshes() {
    let mut client = Client::start();
    let report = client.recipe("d-screw-vise-fit");
    let exports = &report["exports"];
    let scene = &exports["final_scene"];
    assert_eq!(scene["errors"], json!([]));
    assert_eq!(exports["final_solution"]["solved"], true);
    assert_eq!(exports["final_solution"]["diagnostics"], json!([]));
    assert_retained_component_bodies(scene, &exports["final_assembly"]);
    no_overlap(&exports["final_interference"]);
    let parts = exports["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 4);
    assert!(exports["final_sketches"]
        .as_array()
        .unwrap()
        .iter()
        .all(|sketch| sketch["dof"]["value"] == 0));

    let production_source = limo_cad_recipes::find("d-screw-vise").unwrap().source;
    let production_json = production_source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let production: Value = serde_json::from_str(&production_json).unwrap();
    let inputs = &production["exports"]["design_inputs"];
    for (coupon, production) in [
        ("nominal_male_mm", "nominal_thread_mm"),
        ("pitch_mm", "lead_mm"),
        ("female_engagement_mm", "thread_engagement_mm"),
    ] {
        assert_eq!(exports[coupon], inputs[production], "production {coupon}");
    }
    for (coupon, production) in [
        ("radial_depth", "thread_radial_depth_mm"),
        ("corner_radius", "thread_corner_radius_mm"),
        ("radial_clearance", "female_radial_relief_mm"),
        ("axial_clearance", "female_axial_relief_mm"),
    ] {
        assert_eq!(
            exports["rounded_profile"][coupon], inputs[production],
            "production {coupon}"
        );
    }
    for (coupon, production) in [
        ("base_width_mm", "guide_base_width_mm"),
        ("head_width_mm", "guide_head_width_mm"),
        ("height_mm", "guide_height_mm"),
        ("clearance_mm", "guide_clearance_mm"),
    ] {
        assert_eq!(
            exports["guide_profile"][coupon], inputs[production],
            "production {coupon}"
        );
    }
    assert_eq!(exports["coupon_length_mm"], 40.);
    assert_eq!(exports["guide_profile"]["engagement_mm"], 40.);

    let external = client.call("solid_body_feature_definitions", json!({}));
    let male = external
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["type"] == "external_thread")
        .unwrap();
    let holes = client.call("solid_hole_definitions", json!({}));
    let female = holes
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["body_id"] == exports["nut_body_id"])
        .unwrap();
    assert_eq!(
        male["thread"], female["thread"],
        "both native features use the same mating profile"
    );
    assert_eq!(male["thread"]["standard"], "custom_trapezoidal");
    assert_eq!(male["thread"]["series"], "rounded");
    assert_eq!(male["thread"]["class"], "custom");
    assert_eq!(male["thread"]["representation"], "modeled");
    assert_eq!(
        male["thread"]["nominal_diameter"],
        exports["nominal_male_mm"]
    );
    assert_eq!(male["thread"]["pitch"], exports["pitch_mm"]);
    assert_eq!(
        male["thread"]["rounded_profile"],
        exports["rounded_profile"]
    );
    let thread: limo_cad_solid::HoleThreadDto =
        serde_json::from_value(male["thread"].clone()).unwrap();
    let female_diameters =
        limo_cad_solid::rounded_thread_diameters(&thread, limo_cad_solid::ThreadFit::Internal)
            .unwrap()
            .unwrap();
    assert_eq!(exports["nominal_female_mm"], female_diameters[0]);

    let body = |id: &str| {
        scene["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|body| body["id"] == exports[format!("{id}_body_id")])
            .unwrap()
    };
    let (min, max, volume) = mesh_measurement(body("screw"));
    assert!((min[0]).abs() < 1e-5 && (max[0] - 40.).abs() < 1e-5);
    assert!(
        (min[2] - inputs["flat_axis_z_mm"].as_f64().unwrap()).abs() < 1e-5,
        "coupon keeps the production shallow flat"
    );
    assert!(volume > 1.);
    let (_, nut_max, nut_volume) = mesh_measurement(body("nut"));
    assert!((nut_max[2] - exports["female_engagement_mm"].as_f64().unwrap()).abs() < 1e-5);
    assert!(nut_volume > 1.);
    assert!(
        body("nut")["faces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|face| face["cylinder"]["radius"]
                .as_f64()
                .is_some_and(|r| (2. * r - female_diameters[2]).abs() < 1e-6)),
        "native female minor bore matches the mating profile"
    );

    for (part, point) in [
        ("guide_male", [0., 6., 14.]),
        ("guide_male", [0., 14., 22.]),
        ("guide_female", [60., 6.4, 14.]),
        ("guide_female", [60., 14.8, 22.4]),
    ] {
        assert!(
            body(part)["mesh"]["positions"]
                .as_array()
                .unwrap()
                .as_chunks::<3>()
                .0
                .iter()
                .any(|p| (0..3).all(|axis| (p[axis].as_f64().unwrap() - point[axis]).abs() < 1e-5)),
            "{part} lacks captured profile vertex {point:?}"
        );
    }
    let artifacts = RecipeArtifacts::new();
    let directory = &artifacts.path;
    let exported = client.call("solid_export_3mf", json!({"slicer_target":"standard"}));
    validate_print_3mf(
        &exported,
        4,
        [235.5, 256., 256.],
        &directory.join("d-screw-vise-fit.3mf"),
    );
    for id in ["screw", "nut", "guide_male", "guide_female"] {
        let part = parts.iter().find(|part| part["id"] == id).unwrap();
        assert_eq!(part["printable"], true);
        let print = client.call(
            "solid_export_3mf",
            json!({
                "body_ids":[part["body_id"]], "scope":"assembly", "slicer_target":"standard"
            }),
        );
        validate_print_3mf(
            &print,
            1,
            [235.5, 256., 256.],
            &directory.join(format!("vise-fit-{id}.3mf")),
        );
    }
    std::fs::write(
        directory.join("fit-model.json"),
        serde_json::to_vec_pretty(&exports["final_model"]).unwrap(),
    )
    .unwrap();
    write_native_project(
        &directory.join("d-screw-vise-fit.limo"),
        &exports["final_model"],
    );
    let repeated = Client::start().recipe("d-screw-vise-fit");
    for key in [
        "final_scene",
        "final_model",
        "final_sketches",
        "final_assembly",
        "final_solution",
    ] {
        assert_same_json(
            &exports[key],
            &repeated["exports"][key],
            &format!("coupon replay {key}"),
        );
    }
    let mut restored = Client::restore(&exports["final_model"]);
    assert_same_json(
        &restored.call("solid_scene", json!({}))["bodies"],
        &scene["bodies"],
        "coupon native save/reload geometry",
    );
    assert_eq!(restored.call("solid_hole_definitions", json!({})), holes);
    assert_eq!(
        restored.call("solid_body_feature_definitions", json!({})),
        external
    );
    assert_eq!(
        restored.call("assembly_solution", json!({}))["instance_body_poses"],
        exports["final_solution"]["instance_body_poses"]
    );
    let restored_print = restored.call("solid_export_3mf", json!({"slicer_target":"standard"}));
    validate_print_3mf(
        &restored_print,
        4,
        [235.5, 256., 256.],
        &directory.join("vise-fit-restored.3mf"),
    );
}

#[test]
fn turbine_replays_edits_restores_prints_and_drives_native_geometry() {
    let mut client = Client::start();

    client.timeout = Duration::from_secs(900);
    eprintln!("turbine acceptance: complete catalog construction and drawing replay");
    let report = client.recipe("vertical-axis-turbine");
    eprintln!("turbine acceptance: first replay passed; native print plates");
    let exports = &report["exports"];
    assert_eq!(exports["final_solution"]["solved"], true);
    assert_eq!(exports["final_solution"]["diagnostics"], json!([]));
    assert!(exports["final_sketches"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["dof"]["value"] == 0));
    let parts = exports["parts"].as_array().unwrap();
    let scene = &exports["final_scene"];
    assert_eq!(scene["errors"], json!([]));
    assert_retained_component_bodies(scene, &exports["final_assembly"]);
    let body = |id: &Value| {
        scene["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == *id)
            .unwrap()
    };
    let stage = parts.iter().find(|p| p["id"] == "stage").unwrap();
    assert_eq!(stage["quantity"], 2);
    let stage_instances = exports["final_solution"]["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["body_id"] == stage["body_id"])
        .collect::<Vec<_>>();
    assert_eq!(stage_instances.len(), 2);
    assert!(
        (stage_instances[1]["translation"][2].as_f64().unwrap()
            - stage_instances[0]["translation"][2].as_f64().unwrap()
            - 100.)
            .abs()
            < 1e-8,
        "the repeated 100 mm stages must meet at their endplates"
    );
    assert!(
        (stage_instances[1]["rotation"][2].as_f64().unwrap() - std::f64::consts::FRAC_1_SQRT_2)
            .abs()
            < 1e-10
    );
    turbine::check_enclosure_clearance(exports);
    for part in parts {
        let (min, max, volume) = mesh_measurement(body(&part["body_id"]));
        assert!(volume.is_finite() && volume > 0., "{}", part["id"]);
        if part["printable"] != true {
            continue;
        }
        assert!(
            min[2].abs() < 1e-5,
            "{} print pose is not on the bed",
            part["id"]
        );
        assert!(
            (0..3).all(|i| max[i] - min[i] <= 200.001),
            "{} exceeds 200 mm print envelope",
            part["id"]
        );
    }
    let print_directory =
        std::env::var_os("LIMO_CAD_RECIPE_ARTIFACT_DIR").map(std::path::PathBuf::from);
    turbine::check_print_plates(exports, print_directory.as_deref());
    let interference = client.call("assembly_interference_check", json!({}));
    if let Some(directory) = std::env::var_os("LIMO_CAD_RECIPE_ARTIFACT_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("turbine-run-1.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        write_native_project(
            &directory.join("vertical-axis-turbine.limo"),
            &exports["final_model"],
        );
        std::fs::write(
            directory.join("model.json"),
            serde_json::to_vec_pretty(&exports["final_model"]).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("interference.json"),
            serde_json::to_vec_pretty(&interference).unwrap(),
        )
        .unwrap();
        for drawing in exports["drawings"].as_array().unwrap() {
            for format in ["svg", "dxf"] {
                std::fs::write(
                    directory.join(format!("{}.{}", drawing["part"].as_str().unwrap(), format)),
                    drawing[format].as_str().unwrap(),
                )
                .unwrap();
            }
        }
    }
    validate_turbine_edit_and_motion(&mut client, exports);
    validate_turbine_open_overlap(exports);
    eprintln!("turbine acceptance: physical assembly, hardware and driver paths");
    turbine::check_assembly(exports);
    eprintln!("turbine acceptance: mechanics passed; independent compiled-source replay");
    let mut repeat = Client::start();
    repeat.timeout = client.timeout;
    let repeated = repeat.compiled_recipe_source("vertical-axis-turbine");
    assert_eq!(
        exports, &repeated["exports"],
        "two independent native construction and drawing replays"
    );
}

fn validate_turbine_open_overlap(exports: &Value) {
    let mut probe = Client::restore(&exports["final_model"]);
    probe.call(
        "sketch_begin",
        json!({"name":"Overlap inspection witness","plane":{"type":"origin_plane","plane":"xy"}}),
    );
    probe.call("sketch_set_grid_snap", json!({"enabled":false}));
    probe.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":4.6,"y":-0.4},"p2":{"x":6.4,"y":0.4},"ctrl_held":true}),
    );
    probe.call("sketch_finish", json!({}));
    let created = probe.call("solid_extrude", json!({"sketch_name":"Overlap inspection witness","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":65.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]}));
    let original_ids = exports["final_scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| &body["id"])
        .collect::<Vec<_>>();
    let witness_body = &created["scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| !original_ids.contains(&&body["id"]))
        .unwrap()["id"];
    let component = probe.call("assembly_create_component", json!({"name":"Temporary overlap witness","body_ids":[witness_body],"absorb_promoted_bodies":true}));
    let assembly = probe.call("assembly_document", json!({}));
    let witness = &assembly["component_structure"]["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|occurrence| occurrence["component_id"] == component["id"])
        .unwrap()["id"];
    let parts = exports["parts"].as_array().unwrap();
    let stage_pose = exports["final_solution"]["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pose| {
            pose["occurrence_id"]
                == parts.iter().find(|part| part["id"] == "stage").unwrap()["occurrence_id"]
        })
        .unwrap();
    let corridor_bottom = stage_pose["translation"][2].as_f64().unwrap() + 25.;
    for x in [0., -11.] {
        probe.call("assembly_set_occurrence_pose", json!({"occurrence_id":witness,"local_pose":{"translation":[x,0.,corridor_bottom],"rotation":[0.,0.,0.,1.]}}));
        for part_name in ["stage", "shaft"] {
            let target =
                &parts.iter().find(|part| part["id"] == part_name).unwrap()["occurrence_id"];
            let report = probe.call(
                "assembly_interference_check",
                json!({"occurrence_ids":[witness,target],"clearance_threshold_mm":1.}),
            );
            assert_eq!(report["exact"], true);
            let pairs = report["pairs"].as_array().unwrap();
            assert_eq!(pairs.len(), 1);
            assert_eq!(
                pairs[0]["interfering"], false,
                "open overlap against {part_name}: {report}"
            );
            assert!(
                pairs[0]["minimum_clearance_mm"].as_f64().unwrap() >= 0.5,
                "continuous overlap corridor against {part_name}: {report}"
            );
        }
    }
}

fn assert_retained_component_bodies(scene: &Value, assembly: &Value) {
    let retained = scene["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| &body["id"])
        .collect::<Vec<_>>();
    for definition in assembly["component_structure"]["definitions"]
        .as_array()
        .unwrap()
    {
        for body_id in definition["body_ids"].as_array().unwrap() {
            assert!(retained.contains(&body_id),
                "consumed construction tools must not remain as phantom assembly parts: {definition}");
        }
    }
}

#[test]
fn turbine_fit_coupons_have_driving_fits_and_replay_as_closed_prints() {
    let mut client = Client::start();
    let report = client.recipe("turbine-fit-coupons");
    let exports = &report["exports"];
    assert_eq!(exports["final_scene"]["errors"], json!([]));
    assert_eq!(exports["final_solution"]["solved"], true);
    assert_eq!(
        exports["final_solution"]["diagnostics"],
        json!([]),
        "the supplied coupon layout must be a retained fixed arrangement"
    );
    assert_retained_component_bodies(
        &exports["final_scene"],
        &client.call("assembly_document", json!({})),
    );
    let parts = exports["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 4);
    let sketches = exports["final_sketches"].as_array().unwrap();
    assert!(sketches.iter().all(|sketch| sketch["dof"]["value"] == 0));
    let artifact_directory = std::env::var_os("LIMO_CAD_RECIPE_ARTIFACT_DIR")
        .map(|path| std::path::PathBuf::from(path).join("fit-coupons"));
    if let Some(directory) = &artifact_directory {
        std::fs::create_dir_all(directory).unwrap();
    }
    for (id, sketch_name, nominal, allowance) in [
        ("shaft_coupon", "shaft_coupon_fit", 8., 0.3),
        ("bearing_coupon", "bearing_coupon_fit", 22., 0.3),
        ("motor_mount", "motor_mount_cavity", 32., 0.6),
        ("pinion", "pinion_bore", 2., 0.2),
    ] {
        let diameter = nominal + allowance;
        let part = parts.iter().find(|part| part["id"] == id).unwrap();
        let body = exports["final_scene"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|body| body["id"] == part["body_id"])
            .unwrap();
        let (min, max, volume) = mesh_measurement(body);
        assert!(volume > 0. && min[2].abs() < 1e-5);
        assert!(
            (0..3).all(|axis| max[axis] - min[axis] <= 60.),
            "small printable coupon: {id}"
        );
        let sketch = sketches
            .iter()
            .find(|sketch| sketch["name"] == sketch_name)
            .unwrap();
        assert!(
            sketch["dimensions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|dimension| dimension["kind"] == "diameter"
                    && dimension["mode"] == "driving"
                    && (dimension["value"].as_f64().unwrap() - diameter).abs() < 1e-10),
            "editable fit diameter: {id}"
        );
        assert!(
            body["faces"]
                .as_array()
                .unwrap()
                .iter()
                .any(|face| face["cylinder"]["radius"]
                    .as_f64()
                    .is_some_and(|radius| (radius * 2. - diameter).abs() < 1e-8)),
            "actual retained native bore diameter: {id}"
        );
        let print = client.call(
            "solid_export_3mf",
            json!({"body_ids":[part["body_id"]],"scope":"assembly","slicer_target":"standard"}),
        );
        let pose = exports["final_solution"]["instance_body_poses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pose| pose["occurrence_id"] == part["occurrence_id"])
            .unwrap();
        turbine::check_print_placement(&print, body, pose);
        let bytes = BASE64
            .decode(print["bytes_base64"].as_str().unwrap())
            .unwrap();
        if let Some(directory) = &artifact_directory {
            std::fs::write(directory.join(format!("{id}.3mf")), &bytes).unwrap();
        }
    }
    let arranged_print = turbine::check_coupon_layout(&mut client, exports);
    if let Some(directory) = &artifact_directory {
        std::fs::write(
            directory.join("turbine-fit-coupons.3mf"),
            BASE64
                .decode(arranged_print["bytes_base64"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("run-1.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        write_native_project(
            &directory.join("turbine-fit-coupons.limo"),
            &exports["final_model"],
        );
        std::fs::write(
            directory.join("model.json"),
            serde_json::to_vec_pretty(&exports["final_model"]).unwrap(),
        )
        .unwrap();
        for drawing in exports["drawings"].as_array().unwrap() {
            for format in ["svg", "dxf"] {
                std::fs::write(
                    directory.join(format!("{}.{}", drawing["part"].as_str().unwrap(), format)),
                    drawing[format].as_str().unwrap(),
                )
                .unwrap();
            }
        }
    }
    let mut repeat = Client::start();
    assert_eq!(
        exports,
        &repeat.compiled_recipe_source("turbine-fit-coupons")["exports"],
        "exact independent native coupon replay"
    );
    let mut restored = Client::restore(&exports["final_model"]);
    assert_eq!(
        restored.call("solid_scene", json!({}))["bodies"],
        exports["final_scene"]["bodies"]
    );
}

#[path = "recipes/turbine.rs"]
mod turbine;

fn validate_turbine_edit_and_motion(client: &mut Client, exports: &Value) {
    turbine::check_edits_and_motion(client, exports);
}

/// Focused developer probe after a retained construction run:
/// set LIMO_CAD_TURBINE_SAVED_REPORT to that run's JSON report, then run
/// `cargo test --test recipes turbine_saved_edit_motion_probe -- --ignored --exact`.
/// LIMO_CAD_RECIPE_MCP_BIN may select an explicitly copied native test binary.
/// This reuses the normal checks; it does not replace independent construction,
/// installation, print or physical qualification of a changed design.
#[test]
#[ignore = "requires an explicitly selected retained turbine report"]
fn turbine_saved_edit_motion_probe() {
    let path = std::env::var_os("LIMO_CAD_TURBINE_SAVED_REPORT")
        .expect("set LIMO_CAD_TURBINE_SAVED_REPORT to a retained native turbine run report");
    let report: Value = serde_json::from_reader(
        std::fs::File::open(&path)
            .unwrap_or_else(|error| panic!("cannot open turbine report {path:?}: {error}")),
    )
    .unwrap();
    let exports = &report["exports"];
    assert_eq!(
        exports["final_model"]["format"], "limo-cad-project",
        "input must contain native recipe exports"
    );
    let mut client = Client::restore(&exports["final_model"]);
    validate_turbine_edit_and_motion(&mut client, exports);
}

/// Diagnose installation against a retained new-design native report using
/// the exact main acceptance helper. Set LIMO_CAD_TURBINE_SAVED_REPORT and run
/// `cargo test --test recipes turbine_saved_mechanical_probe -- --ignored --exact`.
/// This deliberately does not accept a bare model or legacy report without
/// actual hardware identities, ring envelopes and an explicit axial stack.
/// Optional LIMO_CAD_TURBINE_MECHANICAL_FOCUS=motor_adjuster_nuts selects only
/// those two shared access checks for diagnosis; normal acceptance never reads it.
#[test]
#[ignore = "requires an explicitly selected retained turbine report with hardware metadata"]
fn turbine_saved_mechanical_probe() {
    let path = std::env::var_os("LIMO_CAD_TURBINE_SAVED_REPORT")
        .expect("set LIMO_CAD_TURBINE_SAVED_REPORT to a retained native turbine run report");
    let report: Value = serde_json::from_reader(
        std::fs::File::open(&path)
            .unwrap_or_else(|error| panic!("cannot open turbine report {path:?}: {error}")),
    )
    .unwrap();
    let exports = &report["exports"];
    assert_eq!(exports["final_model"]["format"], "limo-cad-project");
    assert!(
        exports["hardware"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "full hardware metadata is required"
    );
    assert!(
        exports["occurrences"]["bearing_inner_upper"].is_number(),
        "separate bearing ring identities are required"
    );
    assert!(
        exports["design"]["axial_stack"]["endplay_mm"].is_number(),
        "explicit axial stack is required"
    );
    match std::env::var("LIMO_CAD_TURBINE_MECHANICAL_FOCUS")
        .ok()
        .as_deref()
    {
        None => turbine::check_assembly(exports),
        Some("motor_adjuster_nuts") => turbine::check_adjuster_access(exports),
        Some(other) => panic!("unknown explicit turbine mechanical diagnostic focus {other}"),
    }
}
