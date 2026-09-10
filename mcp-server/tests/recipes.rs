//! Exercise the authored recipes through the real MCP stdio binary. This is
//! only a transport/assertion harness; the Rust script interpreter builds CAD.
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Retain artifacts only when explicitly requested. Otherwise own a uniquely
/// created temporary directory, including cleanup during a failing assertion.
struct RecipeArtifacts {
    path: std::path::PathBuf,
    temporary: bool,
}
impl RecipeArtifacts {
    fn new() -> Self {
        if let Some(path) = std::env::var_os("NBCAD_RECIPE_ARTIFACT_DIR").filter(|v| !v.is_empty())
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
                "nbcad-recipe-{}-{epoch}-{}",
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
            // This exact directory was exclusively created by temporary().
            // Never remove the environment-provided destination or its parent.
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
}
impl Client {
    fn start() -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nbcad-mcp"));
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
            timeout: Duration::from_secs(60),
        };
        client.rpc("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"recipe-regression","version":"1"}}));
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
        loop {
            let reply = self
                .replies
                .recv_timeout(self.timeout)
                .unwrap_or_else(|error| panic!("MCP did not finish {method}: {error}"))
                .unwrap_or_else(|error| panic!("Invalid MCP response for {method}: {error}"));
            if reply["id"] != self.id {
                continue;
            }
            assert!(reply.get("error").is_none(), "{reply}");
            return reply["result"].clone();
        }
    }
    fn call(&mut self, operation: &str, arguments: Value) -> Value {
        let reply = self.rpc(
            "tools/call",
            json!({"name":operation,"arguments":arguments}),
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
                eprintln!("Set NBCAD_RECIPE_ARTIFACT_DIR to retain recipe diagnostics.");
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
        self.call(
            "cad_interface",
            json!({"action":"script","recipe":id,"mode":"fast","validate":true}),
        )
    }
    fn restore(model: &Value) -> Self {
        let mut client = Self::start();
        client.call(
            "cad_load_project_model",
            json!({"model_json":serde_json::to_string(model).unwrap()}),
        );
        client
    }
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
    assert!(!positions.is_empty() && positions.len() % 3 == 0);
    assert!(!indices.is_empty() && indices.len() % 3 == 0);
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for (i, coordinate) in positions.iter().enumerate() {
        let value = coordinate.as_f64().unwrap();
        assert!(value.is_finite());
        min[i % 3] = min[i % 3].min(value);
        max[i % 3] = max[i % 3].max(value);
    }
    let mut volume = 0.;
    for triangle in indices.chunks_exact(3) {
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
                    let triangles = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
                    assert!(triangles > 0);
                    assert_eq!(bytes.len(), 84 + 50 * triangles);
                }
                "3mf" => {
                    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
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
    assert_eq!(catalog, nbcad_recipes::catalog(false));
    assert!(catalog
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry.get("source").is_none()));
    assert_eq!(client.call("solid_scene", json!({}))["bodies"], json!([]));
}

#[test]
fn d_screw_vise_builds_editable_native_geometry() {
    let mut client = Client::start();
    let report = client.recipe("d-screw-vise");
    let exports = &report["exports"];
    let artifacts = RecipeArtifacts::new();
    let artifact_directory = &artifacts.path;
    std::fs::write(
        artifact_directory.join("replay-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    std::fs::write(
        artifact_directory.join("model.json"),
        serde_json::to_vec_pretty(&exports["final_model"]).unwrap(),
    )
    .unwrap();
    write_native_project(
        &artifact_directory.join("d-screw-vise.nbcad"),
        &exports["final_model"],
    );
    write_native_project(
        &artifact_directory.join("d-screw-vise-print.nbcad"),
        &exports["print_model"],
    );
    for (name, export) in exports.as_object().unwrap() {
        if name.ends_with("_svg") || name.ends_with("_dxf") {
            let format = export["format"].as_str().unwrap();
            let content = export["content"].as_str().unwrap();
            assert!(!content.is_empty());
            std::fs::write(artifact_directory.join(format!("{name}.{format}")), content).unwrap();
        }
    }
    let sheets = exports["final_model"]["drawings"]["sheets"]
        .as_array()
        .unwrap();
    assert_eq!(
        sheets.len(),
        6,
        "assembly and five printed-part sheets persist"
    );
    let assembly_sheet = sheets
        .iter()
        .find(|sheet| sheet["id"] == exports["vise_assembly_svg"]["sheet_id"])
        .unwrap();
    assert_eq!(
        assembly_sheet["bom"].as_array().unwrap().len(),
        exports["final_scene"]["bodies"].as_array().unwrap().len()
    );
    for sheet in sheets {
        assert_eq!(sheet["projection_method"], "third_angle");
    }
    for (part, measured) in [
        ("assembly", "48.00 at home"),
        ("frame", "150.00"),
        ("jaw", "5.60"),
        ("nut", "16.00"),
        ("screw", "R14.00"),
        ("keeper", "Ø12.80"),
    ] {
        assert!(
            exports[format!("vise_{part}_svg")]["content"]
                .as_str()
                .unwrap()
                .contains(&format!(">{measured}</text>")),
            "{part}: missing measured dimension {measured}"
        );
    }
    assert_eq!(exports["final_scene"]["errors"], json!([]));
    let pi = std::f64::consts::PI;
    let circular_strip = |radius: f64, half_width: f64| {
        2. * (half_width * (radius * radius - half_width * half_width).sqrt()
            + radius * radius * (half_width / radius).asin())
    };
    let jaw_retainer_relief = 3_f64.sqrt() / 2. * 5.9_f64.powi(2) * 3.8
        + pi * 1.7_f64.powi(2) * 1.8
        + 3. * (pi * 3.1_f64.powi(2) - circular_strip(3.1, 2.8));
    let keeper_retainer_relief = 35.4 * pi * 1.7_f64.powi(2) + 3. * circular_strip(3.1, 2.4);
    for (part, expected_min, expected_max, analytic, tolerance) in [
        (
            "frame",
            [0., -35., 0.],
            [150., 35., 52.],
            150. * 70. * 8. - 4. * 18. * 6. * 8. + 20. * 60. * 44. + 24. * 52. * 44.
                - 16.8 * 36.8 * 42.4
                - pi * 10.6_f64.powi(2) * 7.2
                - 7.2 * 10.6_f64.powi(2) * (1. - pi / 4.)
                - 2.8 * 3_f64.sqrt() / 2. * 5.9_f64.powi(2)
                - 4.4 * pi * 1.7_f64.powi(2)
                + 2. * 96. * 4. * 4.,
            0.0005,
        ),
        (
            "jaw",
            [60., -30., 8.],
            [82., 30., 52.],
            22. * 60. * 44.
                - 2. * 22. * 4.8 * 4.4
                - 5.6 * 40.8 * 38.4
                - 10.8 * pi * 14.4_f64.powi(2)
                - jaw_retainer_relief,
            0.002,
        ),
        (
            "keeper",
            [66.6, -20., 13.6],
            [71.4, 20., 52.],
            4.8 * (40. * 38.4 - 0.5 * pi * 6.4_f64.powi(2) - 12.8 * 14.4) - keeper_retainer_relief,
            0.001,
        ),
    ] {
        let body = exports["final_scene"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|body| body["id"] == exports[format!("{part}_body_id")])
            .unwrap();
        let (min, max, volume) = mesh_measurement(body);
        for axis in 0..3 {
            assert!((min[axis] - expected_min[axis]).abs() < 1e-4);
            assert!((max[axis] - expected_max[axis]).abs() < 1e-4);
        }
        assert!(
            (volume - analytic).abs() / analytic < tolerance,
            "{part}: measured {volume}, analytic {analytic}"
        );
    }
    let frame = exports["final_scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == exports["frame_body_id"])
        .unwrap();
    let roof_normals: Vec<_> = frame["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|face| face["plane"]["normal"].as_array())
        .filter(|normal| {
            normal[0].as_f64().unwrap().abs() < 1e-7
                && (normal[1].as_f64().unwrap().abs() - std::f64::consts::FRAC_1_SQRT_2).abs()
                    < 1e-7
                && (normal[2].as_f64().unwrap().abs() - std::f64::consts::FRAC_1_SQRT_2).abs()
                    < 1e-7
        })
        .collect();
    assert!(
        roof_normals.iter().any(|n| n[1].as_f64().unwrap() > 0.)
            && roof_normals.iter().any(|n| n[1].as_f64().unwrap() < 0.),
        "both45-degree roof sides exist in native geometry"
    );
    assert!(52. - (28. + 10.6 * 2_f64.sqrt()) > 9.);
    let female =
        nbcad_solid::iso_metric_grade6_envelope(20.5, 2.5, nbcad_solid::ThreadFit::Internal)
            .unwrap();
    let female_mean_radius_squared = internal_thread_mean_radius_squared(&female, 2.5);
    let nut = exports["final_scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == exports["nut_body_id"])
        .unwrap();
    let nut_analytic =
        16. * 36. * 36. - 16. * pi * female_mean_radius_squared - 16. * pi * 1.7_f64.powi(2);
    assert!(
        (mesh_measurement(nut).2 - nut_analytic).abs() / nut_analytic < 0.01,
        "threaded cartridge follows its60-degree radial profile integral"
    );
    let interference = exports["final_interference"].clone();
    std::fs::write(
        artifact_directory.join("interference.json"),
        serde_json::to_vec_pretty(&interference).unwrap(),
    )
    .unwrap();
    no_overlap(&interference);
    let support = interference["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| {
            (pair["body_a"] == exports["frame_body_id"] && pair["body_b"] == exports["jaw_body_id"])
                || (pair["body_b"] == exports["frame_body_id"]
                    && pair["body_a"] == exports["jaw_body_id"])
        })
        .unwrap();
    assert!(
        support["minimum_clearance_mm"].as_f64().unwrap() < 1e-6,
        "jaw must seat on the base support datum: {support}"
    );
    assert!(interference["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|pair| {
            pair["body_a"] == exports["screw_body_id"]
                && pair["body_b"] == exports["nut_body_id"]
                && pair["minimum_clearance_mm"].as_f64().unwrap() > 0.12
        }));
    validate_print_3mf(
        &exports["print_3mf"],
        5,
        [235.5, 256., 256.],
        &artifact_directory.join("d-screw-vise-print.3mf"),
    );
    std::fs::write(
        artifact_directory.join("print-model.json"),
        serde_json::to_vec_pretty(&exports["print_model"]).unwrap(),
    )
    .unwrap();

    // Command only the screw. The other loop coordinates and jaw pose must
    // follow the physical lead, including full turns and reversing direction.
    for angle in [90_f64, 360., 1440., 720., 48. / 2.5 * 360., 0.] {
        client.call("assembly_set_joint_motion",json!({"joint_id":exports["screw_joint_id"],"angle_offset_deg":angle,"linear_offset_mm":0}));
        let assembly = client.call("assembly_document", json!({}));
        let solution = client.call("assembly_solution", json!({}));
        assert_eq!(solution["solved"], true, "{solution}");
        assert_eq!(solution["diagnostics"], json!([]));
        let driven = assembly["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"] == exports["screw_joint_id"])
            .unwrap();
        assert!((driven["angle_offset_deg"].as_f64().unwrap() - angle).abs() < 1e-5);
        for part in ["jaw", "keeper", "screw"] {
            let pose = solution["instance_body_poses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|pose| pose["body_id"] == exports[format!("{part}_body_id")])
                .unwrap();
            assert!(
                (pose["translation"][0].as_f64().unwrap() - angle / 360. * 2.5).abs() < 1e-5,
                "{part}: {pose}"
            );
        }
        if angle == 90. || angle == 1440. || angle == 48. / 2.5 * 360. {
            // The end position also checks the fixed jaw contact and handle
            // clearance, rather than only the thread pair's bounding boxes.
            let arguments = if angle == 48. / 2.5 * 360. {
                json!({"clearance_threshold_mm":0.})
            } else {
                json!({"occurrence_ids":[exports["screw_occurrence_id"],exports["nut_occurrence_id"]],"clearance_threshold_mm":0.})
            };
            no_overlap(&client.call("assembly_interference_check", arguments));
        }
    }
    // Two full turns near the end of travel cover the paddle's entire sweep.
    // The rearward paddle also has an analytic separating plane: its forward
    // edge isX-49+travel, strictly before the base'sX0 for every allowed travel.
    for quarter_turn in 0..=8 {
        let travel = 43. + quarter_turn as f64 * 2.5 / 4.;
        assert!(-49. + travel < 0.);
        client.call("assembly_set_joint_motion",json!({"joint_id":exports["screw_joint_id"],"angle_offset_deg":travel/2.5*360.,"linear_offset_mm":0}));
        no_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[exports["screw_occurrence_id"],exports["frame_occurrence_id"]],"clearance_threshold_mm":0})));
    }
    client.call(
        "assembly_set_joint_motion",
        json!({"joint_id":exports["screw_joint_id"],"angle_offset_deg":0,"linear_offset_mm":0}),
    );

    // Remove the anti-lift screw first; the inverted-U keeper can then descend
    // over the neck through the actual jaw slot without intersecting either.
    let joint_id = |name: &str| {
        exports["final_assembly"]["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["name"] == name)
            .unwrap()["id"]
            .clone()
    };
    // Lower the jaw to the right of the head, then slide it over the head
    // through the open rear chamber. Its closed floor cannot drop over it.
    for name in ["jaw_guide", "thrust_retention"] {
        client.call(
            "assembly_set_joint_enabled",
            json!({"joint_id":joint_id(name),"enabled":false}),
        );
    }
    for translation in [
        [30., 0., 50.],
        [30., 0., 25.],
        [30., 0., 0.],
        [20., 0., 0.],
        [10., 0., 0.],
        [0., 0., 0.],
    ] {
        client.call("assembly_set_occurrence_pose", json!({"occurrence_id":exports["jaw_occurrence_id"],"local_pose":{"translation":translation,"rotation":[0,0,0,1]}}));
        no_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[exports["frame_occurrence_id"],exports["screw_occurrence_id"],exports["jaw_occurrence_id"]],"clearance_threshold_mm":0})));
    }
    for name in ["jaw_guide", "thrust_retention"] {
        client.call(
            "assembly_set_joint_enabled",
            json!({"joint_id":joint_id(name),"enabled":true}),
        );
    }
    // The cartridge has positive capture independent of the rotating D-thread.
    // Check its assembly sequence before the drive/jaw are installed, then its
    // free clearance and hard stops using only the actual retaining bodies.
    for name in [
        "nut_in_housing",
        "cartridge_screw_in_housing",
        "cartridge_nut_in_housing",
    ] {
        client.call(
            "assembly_set_joint_enabled",
            json!({"joint_id":joint_id(name),"enabled":false}),
        );
    }
    let cartridge_pose = |client: &mut Client, part: &str, translation: Value, rotation: Value| {
        client.call("assembly_set_occurrence_pose",json!({"occurrence_id":exports[format!("{part}_occurrence_id")],"local_pose":{"translation":translation,"rotation":rotation}}));
    };
    cartridge_pose(
        &mut client,
        "cartridge_screw",
        json!([60, 0, 0]),
        json!([0, 0, 0, 1]),
    );
    cartridge_pose(
        &mut client,
        "cartridge_nut",
        json!([-20, 0, 0]),
        json!([0, 0, 0, 1]),
    );
    for lift in [50., 30., 10., 0.] {
        cartridge_pose(&mut client, "nut", json!([0, 0, lift]), json!([0, 0, 0, 1]));
        no_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[exports["frame_occurrence_id"],exports["nut_occurrence_id"]],"clearance_threshold_mm":0})));
    }
    for retreat in [-12., -5., 0.] {
        cartridge_pose(
            &mut client,
            "cartridge_nut",
            json!([retreat, 0, 0]),
            json!([0, 0, 0, 1]),
        );
        no_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[exports["frame_occurrence_id"],exports["nut_occurrence_id"],exports["cartridge_nut_occurrence_id"]],"clearance_threshold_mm":0})));
    }
    for approach in [40., 20., 3., 0.] {
        cartridge_pose(
            &mut client,
            "cartridge_screw",
            json!([approach, 0, 0]),
            json!([0, 0, 0, 1]),
        );
        no_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[exports["frame_occurrence_id"],exports["nut_occurrence_id"],exports["cartridge_nut_occurrence_id"],exports["cartridge_screw_occurrence_id"]],"clearance_threshold_mm":0})));
    }
    // Two Ø3.4 holes around a Ø3 shaft permit at most0.4mm relative radial
    // offset in rigid geometry. At0.38mm the bolt can share that offset;
    // at0.42mm it cannot fit both holes. No friction/gravity assumption enters.
    cartridge_pose(
        &mut client,
        "cartridge_screw",
        json!([0, 0, 0.19]),
        json!([0, 0, 0, 1]),
    );
    for (lift, blocked) in [(0.38, false), (0.42, true)] {
        cartridge_pose(&mut client, "nut", json!([0, 0, lift]), json!([0, 0, 0, 1]));
        let result=client.call("assembly_interference_check",json!({"occurrence_ids":[exports["frame_occurrence_id"],exports["nut_occurrence_id"],exports["cartridge_screw_occurrence_id"]],"clearance_threshold_mm":0}));
        assert_eq!(
            result["pairs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["interfering"] == true),
            blocked,
            "cartridge lift{lift}: {result}"
        );
    }
    cartridge_pose(
        &mut client,
        "cartridge_screw",
        json!([0, 0, 0]),
        json!([0, 0, 0, 1]),
    );
    for angle in [-1_f64, 1.] {
        let radians = angle.to_radians();
        let (sin, cos) = radians.sin_cos();
        cartridge_pose(
            &mut client,
            "nut",
            json!([
                0.,
                12. - (12. * cos - 42. * sin),
                42. - (12. * sin + 42. * cos)
            ]),
            json!([(radians / 2.).sin(), 0., 0., (radians / 2.).cos()]),
        );
        let result=client.call("assembly_interference_check",json!({"occurrence_ids":[exports["frame_occurrence_id"],exports["nut_occurrence_id"]],"clearance_threshold_mm":0}));
        assert!(
            result["pairs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["interfering"] == true),
            "cartridge rock{angle}degree must meet a housing stop: {result}"
        );
    }
    cartridge_pose(&mut client, "nut", json!([0, 0, 0]), json!([0, 0, 0, 1]));
    for name in [
        "nut_in_housing",
        "cartridge_screw_in_housing",
        "cartridge_nut_in_housing",
    ] {
        client.call(
            "assembly_set_joint_enabled",
            json!({"joint_id":joint_id(name),"enabled":true}),
        );
    }
    for name in ["keeper_in_jaw", "retainer_screw_in_jaw"] {
        client.call(
            "assembly_set_joint_enabled",
            json!({"joint_id":joint_id(name),"enabled":false}),
        );
    }
    client.call("assembly_set_occurrence_pose",json!({"occurrence_id":exports["retainer_screw_occurrence_id"],"local_pose":{"translation":[0,0,60],"rotation":[0,0,0,1]}}));
    for lift in [40., 24., 16., 8., 0.] {
        client.call("assembly_set_occurrence_pose",json!({"occurrence_id":exports["keeper_occurrence_id"],"local_pose":{"translation":[0,0,lift],"rotation":[0,0,0,1]}}));
        no_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[exports["keeper_occurrence_id"],exports["jaw_occurrence_id"],exports["screw_occurrence_id"]],"clearance_threshold_mm":0})));
    }
    // Check shaft/head insertion after seating the keeper. These hardware
    // envelopes reserve access; simplified thread contact is deliberately zero.
    for lift in [45., 20., 3., 0.] {
        client.call("assembly_set_occurrence_pose",json!({"occurrence_id":exports["retainer_screw_occurrence_id"],"local_pose":{"translation":[0,0,lift],"rotation":[0,0,0,1]}}));
        no_overlap(&client.call("assembly_interference_check",json!({"occurrence_ids":[exports["retainer_screw_occurrence_id"],exports["retainer_nut_occurrence_id"],exports["keeper_occurrence_id"],exports["jaw_occurrence_id"]],"clearance_threshold_mm":0})));
    }
    for name in ["keeper_in_jaw", "retainer_screw_in_jaw"] {
        client.call(
            "assembly_set_joint_enabled",
            json!({"joint_id":joint_id(name),"enabled":true}),
        );
    }
    let before_rejection = client.call("cad_project_model", json!({}));
    let rejected=client.rpc("tools/call",json!({"name":"assembly_set_joint_motion","arguments":{"joint_id":exports["screw_joint_id"],"angle_offset_deg":49./2.5*360.,"linear_offset_mm":0}}));
    assert_eq!(rejected["isError"], true, "{rejected}");
    assert_eq!(
        client.call("cad_project_model", json!({})),
        before_rejection,
        "travel rejection is atomic"
    );

    let original_jaw = exports["final_scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == exports["jaw_body_id"])
        .unwrap();
    let original_jaw_volume = mesh_measurement(original_jaw).2;
    for (name, original, changed, expected_volume_delta) in [
        (
            "Moving jaw / 60 mm gripping face",
            60_f64,
            64_f64,
            4. * 22. * 44.,
        ),
        (
            "Jaw guide left / running clearance",
            4.8,
            5.2,
            -0.4 * 22. * 4.4,
        ),
    ] {
        let sketch = client.call("sketch_edit", json!({"name":name}));
        let dimension = sketch["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|dimension| (dimension["value"].as_f64().unwrap() - original).abs() < 1e-8)
            .unwrap();
        assert_eq!(dimension["mode"], "driving");
        let constraint_id = dimension["constraint_id"].clone();
        client.call(
            "sketch_edit_dimension",
            json!({"constraint_id":constraint_id,"text":changed.to_string()}),
        );
        assert_eq!(client.call("sketch_active", json!({}))["dof"]["value"], 0);
        client.call("sketch_finish", json!({}));
        let modified = client.call("solid_recompute", json!({}));
        assert_eq!(modified["scene"]["errors"], json!([]));
        let jaw = modified["scene"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|body| body["id"] == exports["jaw_body_id"])
            .unwrap();
        assert!(
            (mesh_measurement(jaw).2 - original_jaw_volume - expected_volume_delta).abs() < 0.03,
            "{name}: edit did not change the intended stock/clearance"
        );
        assert_eq!(client.call("assembly_solution", json!({}))["solved"], true);
        let updated_drawing = client.call(
            "drawing_export",
            json!({"sheet_id":exports["vise_jaw_svg"]["sheet_id"],"format":"svg"}),
        );
        assert!(
            updated_drawing["content"]
                .as_str()
                .unwrap()
                .contains(&format!(">{changed:.2}</text>")),
            "{name}: associative drawing did not measure the edited dimension"
        );
        client.call("sketch_edit", json!({"name":name}));
        client.call(
            "sketch_edit_dimension",
            json!({"constraint_id":constraint_id,"text":original.to_string()}),
        );
        client.call("sketch_finish", json!({}));
        let restored = client.call("solid_recompute", json!({}));
        let residual = restored_geometry_residual(
            &restored["scene"]["bodies"],
            &exports["final_scene"]["bodies"],
            String::new(),
        );
        eprintln!(
            "{name}: largest edit/restore floating residual = {} at {}",
            residual.0, residual.1
        );
        assert!(
            residual.0 < 1e-6,
            "{name}: restore residual {} at {}",
            residual.0,
            residual.1
        );
    }

    // Edit the actual persisted thread, replay its downstream D cut and
    // restore the modeled helix. This is not an annotation-only thread.
    let mut thread_request = exports["male_thread_request"].clone();
    thread_request["thread"]["representation"] = json!("simplified");
    let simplified = client.call(
        "solid_edit_external_thread",
        json!({"feature_id":exports["male_thread_feature_id"],"request":thread_request}),
    );
    assert_eq!(simplified["scene"]["errors"], json!([]));
    let screw_volume = |scene: &Value| {
        mesh_measurement(
            scene["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|body| body["id"] == exports["screw_body_id"])
                .unwrap(),
        )
        .2
    };
    assert!(screw_volume(&simplified["scene"]) > screw_volume(&exports["final_scene"]) + 100.);
    let invalidated = client.rpc("tools/call", json!({"name":"drawing_export","arguments":{"sheet_id":exports["vise_screw_svg"]["sheet_id"],"format":"svg"}}));
    assert_eq!(
        invalidated["isError"], true,
        "a topology-changing thread edit must not silently rebind dimensions"
    );
    assert!(invalidated["content"]
        .to_string()
        .contains("topology changed"));
    let restored_thread=client.call("solid_edit_external_thread",json!({"feature_id":exports["male_thread_feature_id"],"request":exports["male_thread_request"]}));
    assert!(
        restored_geometry_residual(
            &restored_thread["scene"]["bodies"],
            &exports["final_scene"]["bodies"],
            String::new()
        )
        .0 < 1e-6
    );
    assert_same_json(
        &client.call(
            "drawing_export",
            json!({"sheet_id":exports["vise_screw_svg"]["sheet_id"],"format":"svg"}),
        )["content"],
        &exports["vise_screw_svg"]["content"],
        "restored thread restores verified drawing references",
    );
    let mut restored = Client::restore(&exports["final_model"]);
    assert_same_json(
        &restored.call("solid_scene", json!({}))["bodies"],
        &exports["final_scene"]["bodies"],
        "native save/reload geometry",
    );
    assert_eq!(
        restored.call("assembly_solution", json!({}))["instance_body_poses"],
        exports["final_solution"]["instance_body_poses"]
    );
    let repeated = Client::start().recipe("d-screw-vise");
    for (key, value) in exports.as_object().unwrap() {
        if key.ends_with("_svg") || key.ends_with("_dxf") {
            assert_same_json(
                value,
                &repeated["exports"][key],
                &format!("independent drawing replay: {key}"),
            );
            assert_same_json(
                &restored.call(
                    "drawing_export",
                    json!({"sheet_id":value["sheet_id"],"format":value["format"]}),
                )["content"],
                &value["content"],
                &format!("native save/reload drawing: {key}"),
            );
        }
    }
    for key in [
        "final_model",
        "final_scene",
        "final_sketches",
        "final_assembly",
        "final_solution",
        "print_model",
        "print_solution",
    ] {
        assert_same_json(
            &exports[key],
            &repeated["exports"][key],
            &format!("independent vise replay: {key}"),
        );
    }
}

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

// Solver edit/restore can leave roundoff such as60.00000000000001 rather
// than60.0. Measure it; preserve exact topology/IDs/indices and keep independent
// cold replay byte-for-byte assertions separate from this geometric tolerance.
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

// An edit can flip a tessellation diagonal without changing the CAD surface.
// Compare the native topology separately; for changed mesh buffers compare the
// oriented boundary of each coplanar triangle patch, not corresponding indices.
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
    let quantize = |v: &[Value]| -> Vertex {
        std::array::from_fn(|i| (v[i].as_f64().unwrap() * 1e6).round() as i64)
    };
    let triangles = |mesh: &Value| -> (BTreeMap<Triangle, usize>, BTreeSet<Vertex>) {
        let vertices: Vec<_> = mesh["positions"]
            .as_array()
            .unwrap()
            .chunks_exact(3)
            .map(quantize)
            .collect();
        let normals = mesh["normals"].as_array().unwrap();
        assert_eq!(vertices.len() * 3, normals.len());
        // Averaged shading normals depend on the tessellation diagonal. Their
        // exact values belong to the independent replay check, not CAD extent.
        for normal in normals.chunks_exact(3) {
            let norm = normal
                .iter()
                .map(|v| v.as_f64().unwrap().powi(2))
                .sum::<f64>();
            assert!(norm.is_finite() && (norm - 1.).abs() < 1e-5);
        }
        let mut triangles = BTreeMap::new();
        for indices in mesh["indices"].as_array().unwrap().chunks_exact(3) {
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

fn internal_thread_mean_radius_squared(
    envelope: &nbcad_solid::IsoMetricThreadEnvelope,
    pitch: f64,
) -> f64 {
    let (minor, major, mid) = (
        envelope.modeled_minor / 2.,
        envelope.modeled_major / 2.,
        envelope.modeled_pitch / 2.,
    );
    let inner_half = pitch / 4. + (mid - minor) / 3_f64.sqrt();
    let outer_half = pitch / 4. - (major - mid) / 3_f64.sqrt();
    (2. * outer_half * major.powi(2)
        + 2. * (inner_half - outer_half) * (minor.powi(2) + minor * major + major.powi(2)) / 3.
        + (pitch - 2. * inner_half) * minor.powi(2))
        / pitch
}

fn write_native_project(path: &std::path::Path, model: &Value) {
    assert_eq!(model["format"], "nbcad-project");
    let manifest = json!({"format":"nbcad-project","container_version":1,"model":"model.json","model_schema_version":model["schema_version"],"application":"noBS CAD","application_version":env!("CARGO_PKG_VERSION"),"saved_at":"1970-01-01T00:00:00Z"});
    // Fixed epoch makes a rebuilt artifact reproducible; it is not the run date.
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
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("3D/3dmodel.model")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("unit=\"millimeter\""));
    fn attribute<'a>(tag: &'a str, name: &str) -> &'a str {
        tag.split(&format!("{name}=\""))
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
    }
    let mut bounds = Vec::new();
    for object in xml.split("<object ").skip(1) {
        let object = object.split("</object>").next().unwrap();
        let vertices: Vec<[f64; 3]> = object
            .split("<vertex ")
            .skip(1)
            .map(|tag| ["x", "y", "z"].map(|axis| attribute(tag, axis).parse().unwrap()))
            .collect();
        if vertices.is_empty() {
            continue;
        }
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
        let mut edges = std::collections::BTreeMap::<(usize, usize), usize>::new();
        let mut signed_volume = 0.;
        for triangle in object.split("<triangle ").skip(1) {
            let indices =
                ["v1", "v2", "v3"].map(|name| attribute(triangle, name).parse::<usize>().unwrap());
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
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        assert!(
            signed_volume > 1.,
            "non-positive print volume {signed_volume}"
        );
        assert!(
            !edges.is_empty() && edges.values().all(|incidence| *incidence == 2),
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
fn d_screw_vise_coupon_replays_real_threads_and_exports_printable_meshes() {
    let mut client = Client::start();
    let report = client.recipe("d-screw-vise-fit");
    let exports = &report["exports"];
    let envelope =
        nbcad_solid::iso_metric_grade6_envelope(20., 2.5, nbcad_solid::ThreadFit::External)
            .unwrap();
    let female =
        nbcad_solid::iso_metric_grade6_envelope(20.5, 2.5, nbcad_solid::ThreadFit::Internal)
            .unwrap();
    let radial_clearance_min = (female.pitch_min - envelope.pitch_max) / 2.;
    let radial_clearance_max = (female.pitch_max - envelope.pitch_min) / 2.;
    assert!(radial_clearance_min >= 0.25 && radial_clearance_max < 0.7);
    let (major, pitch, minor) = (
        envelope.modeled_major / 2.,
        envelope.modeled_pitch / 2.,
        envelope.modeled_minor / 2.,
    );
    let slope = 1. / 3_f64.sqrt();
    let root_half = 2.5 / 4. - (pitch - minor) * slope;
    let outer_half = 2.5 / 4. + (major - pitch) * slope;
    let mean_radius_squared = (2. * root_half * minor.powi(2)
        + 2. * (outer_half - root_half) * (minor.powi(2) + minor * major + major.powi(2)) / 3.
        + (2.5 - 2. * outer_half) * major.powi(2))
        / 2.5;
    let analytic_volume = std::f64::consts::PI * 25. * mean_radius_squared / 2.;
    let screw = exports["final_scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == exports["screw_body_id"])
        .unwrap();
    let (min, max, volume) = mesh_measurement(screw);
    assert!((min[0] - 0.).abs() < 1e-5 && (max[0] - 25.).abs() < 1e-5 && min[2].abs() < 1e-5);
    assert!(
        (volume - analytic_volume).abs() / analytic_volume < 0.01,
        "coupon measured {volume}, axial-profile integral {analytic_volume}"
    );
    let artifacts = RecipeArtifacts::new();
    let directory = &artifacts.path;
    let exported = client.call("solid_export_3mf", json!({"slicer_target":"standard"}));
    validate_print_3mf(
        &exported,
        2,
        [235.5, 256., 256.],
        &directory.join("d-screw-vise-fit.3mf"),
    );
    std::fs::write(
        directory.join("fit-model.json"),
        serde_json::to_vec_pretty(&exports["final_model"]).unwrap(),
    )
    .unwrap();
    write_native_project(
        &directory.join("d-screw-vise-fit.nbcad"),
        &exports["final_model"],
    );
    let repeated = Client::start().recipe("d-screw-vise-fit");
    for key in ["final_scene", "final_model", "final_sketches"] {
        assert_eq!(
            exports[key], repeated["exports"][key],
            "coupon replay {key}"
        );
    }
    assert_eq!(
        Client::restore(&exports["final_model"]).call("solid_scene", json!({}))["bodies"],
        exports["final_scene"]["bodies"]
    );
}

#[test]
fn turbine_replays_edits_restores_prints_and_drives_native_geometry() {
    let mut client = Client::start();
    // This is a full construction/drafting acceptance run, not a single-call
    // unit test. The deadline remains bounded and failures still stop at once.
    client.timeout = Duration::from_secs(240);
    let report = client.recipe("vertical-axis-turbine");
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
    assert_eq!(stage_instances[0]["translation"], json!([0., 0., 70.]));
    assert_eq!(stage_instances[1]["translation"], json!([0., 0., 170.]));
    assert!(
        (stage_instances[1]["rotation"][2].as_f64().unwrap() - std::f64::consts::FRAC_1_SQRT_2)
            .abs()
            < 1e-10
    );
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
        let print = client.call(
            "solid_export_3mf",
            json!({"body_ids":[part["body_id"]],"slicer_target":"standard"}),
        );
        let bytes = BASE64
            .decode(print["bytes_base64"].as_str().unwrap())
            .unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut model = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model)
            .unwrap();
        assert_eq!(
            model.matches("<object ").count(),
            1,
            "one selected printable definition per file"
        );
        assert!(model.contains("unit=\"millimeter\"") && model.contains("<triangle "));
        if let Some(directory) = std::env::var_os("NBCAD_RECIPE_ARTIFACT_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join(format!("{}.3mf", part["id"].as_str().unwrap())),
                bytes,
            )
            .unwrap();
        }
    }
    let interference = client.call("assembly_interference_check", json!({}));
    if let Some(directory) = std::env::var_os("NBCAD_RECIPE_ARTIFACT_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("turbine-run-1.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
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
    let mut repeat = Client::start();
    repeat.timeout = client.timeout;
    let repeated = repeat.recipe("vertical-axis-turbine");
    assert_eq!(
        exports, &repeated["exports"],
        "two independent native construction and drawing replays"
    );
    let mut restored = Client::restore(&exports["final_model"]);
    let restored_scene = restored.call("solid_scene", json!({}));
    assert_eq!(
        restored_scene["bodies"], scene["bodies"],
        "cold save/load preserves exact topology and mesh"
    );
    let old_volume = mesh_measurement(body(&stage["body_id"])).2;
    let changed=restored.call("solid_edit_extrude",json!({"feature_id":exports["stage_plate_feature"],"extent":{"type":"distance","distance":4.}}));
    assert_eq!(changed["scene"]["errors"], json!([]));
    let changed_body = changed["scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == stage["body_id"])
        .unwrap();
    let difference = mesh_measurement(changed_body).2 - old_volume;
    assert!((29_000.0..31_000.0).contains(&difference),"one mm of disc material must fill the free area around the hub and bucket walls: {difference}");
    let edited_model = restored.call("cad_project_model", json!({}));
    let mut edited = Client::restore(&edited_model);
    let edited_scene = edited.call("solid_scene", json!({}));
    assert_eq!(edited_scene["bodies"], changed["scene"]["bodies"]);
    let moved = client.call(
        "assembly_set_joint_motion",
        json!({"joint_id":exports["rotor_joint_id"],"angle_offset_deg":810.,"linear_offset_mm":0.}),
    );
    assert!(!moved.is_null());
    let assembly = client.call("assembly_document", json!({}));
    let driven = assembly["joints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == exports["generator_joint_id"])
        .unwrap();
    assert_eq!(driven["angle_offset_deg"], -3230.);
    assert_eq!(client.call("assembly_solution", json!({}))["solved"], true);
    client.call(
        "assembly_set_joint_motion",
        json!({"joint_id":exports["rotor_joint_id"],"angle_offset_deg":0.,"linear_offset_mm":0.}),
    );
    assert_eq!(
        client.call("assembly_document", json!({}))["joints"],
        exports["final_assembly"]["joints"]
    );
}
