//! Exercise the authored recipes through the real MCP stdio binary. This is
//! only a transport/assertion harness; the Rust script interpreter builds CAD.
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
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
        let mut client = Self {
            child,
            input,
            output,
            id: 0,
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
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "MCP exited while waiting for {method}"
            );
            let reply: Value = serde_json::from_str(&line).unwrap();
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
