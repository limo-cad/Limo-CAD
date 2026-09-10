//! Exercise the authored recipes through the real MCP stdio binary. This is
//! only a transport/assertion harness; the Rust script interpreter builds CAD.
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Client {
    child: Child,
    input: ChildStdin,
    replies: Receiver<Result<Value, String>>,
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
                .recv_timeout(Duration::from_secs(600))
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
        if reply["isError"]==true && operation=="cad_interface" {
            let sketches=self.call("sketch_finished",json!({}));
            eprintln!("Failed recipe's last sketch: {}",sketches.as_array().and_then(|s|s.last()).unwrap_or(&Value::Null));
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
    let artifact_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../native-target/vise-artifacts");
    std::fs::create_dir_all(&artifact_directory).unwrap();
    std::fs::write(artifact_directory.join("replay-report.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    std::fs::write(artifact_directory.join("model.json"), serde_json::to_vec_pretty(&exports["final_model"]).unwrap()).unwrap();
    assert_eq!(exports["final_scene"]["errors"], json!([]));
    let pi=std::f64::consts::PI;
    let circular_strip=|radius:f64,half_width:f64|2.*(half_width*(radius*radius-half_width*half_width).sqrt()+radius*radius*(half_width/radius).asin());
    let jaw_retainer_relief=3_f64.sqrt()/2.*5.9_f64.powi(2)*3.4+pi*1.7_f64.powi(2)*1.8+3.*(pi*3.1_f64.powi(2)-circular_strip(3.1,2.8));
    let keeper_retainer_relief=35.*pi*1.7_f64.powi(2)+3.*circular_strip(3.1,2.4);
    for (part,expected_min,expected_max,analytic,tolerance) in [
        ("frame",[0.,-35.,0.],[150.,35.,52.],150.*70.*8.-4.*18.*6.*8.+20.*60.*44.+24.*52.*44.-16.8*36.8*42.4-pi*10.6_f64.powi(2)*7.2+2.*96.*4.*4.,0.0005),
        ("jaw",[60.,-30.,8.4],[82.,30.,52.],22.*60.*43.6-2.*22.*4.8*4.-5.6*40.8*38.4-10.8*pi*14.4_f64.powi(2)-jaw_retainer_relief,0.002),
        ("keeper",[66.6,-20.,14.],[71.4,20.,52.],4.8*(40.*38.-0.5*pi*6.4_f64.powi(2)-12.8*14.)-keeper_retainer_relief,0.001),
    ] {
        let body=exports["final_scene"]["bodies"].as_array().unwrap().iter().find(|body|body["id"]==exports[format!("{part}_body_id")]).unwrap();
        let (min,max,volume) = mesh_measurement(body);
        for axis in 0..3 { assert!((min[axis]-expected_min[axis]).abs()<1e-4); assert!((max[axis]-expected_max[axis]).abs()<1e-4); }
        assert!((volume-analytic).abs()/analytic<tolerance,"{part}: measured {volume}, analytic {analytic}");
    }
    let interference = client.call("assembly_interference_check",json!({"clearance_threshold_mm":0.5}));
    std::fs::write(artifact_directory.join("interference.json"),serde_json::to_vec_pretty(&interference).unwrap()).unwrap();
    no_overlap(&interference);
    assert!(interference["pairs"].as_array().unwrap().iter().any(|pair| {
        pair["body_a"]==exports["screw_body_id"] && pair["body_b"]==exports["nut_body_id"] && pair["minimum_clearance_mm"].as_f64().unwrap()>0.12
    }));
    validate_print_3mf(&exports["print_3mf"],5,[235.5,256.,256.],&artifact_directory.join("d-screw-vise-print.3mf"));
    std::fs::write(artifact_directory.join("print-model.json"),serde_json::to_vec_pretty(&exports["print_model"]).unwrap()).unwrap();

    // Command only the screw. The other loop coordinates and jaw pose must
    // follow the physical lead, including full turns and reversing direction.
    for angle in [90_f64,360.,1440.,720.,48./2.5*360.,0.] {
        client.call("assembly_set_joint_motion",json!({"joint_id":exports["screw_joint_id"],"angle_offset_deg":angle,"linear_offset_mm":0}));
        let assembly=client.call("assembly_document",json!({}));
        let solution=client.call("assembly_solution",json!({}));
        assert_eq!(solution["solved"],true,"{solution}");
        assert_eq!(solution["diagnostics"],json!([]));
        let driven=assembly["joints"].as_array().unwrap().iter().find(|joint|joint["id"]==exports["screw_joint_id"]).unwrap();
        assert!((driven["angle_offset_deg"].as_f64().unwrap()-angle).abs()<1e-5);
        for part in ["jaw","keeper","screw"] {
            let pose=solution["instance_body_poses"].as_array().unwrap().iter().find(|pose|pose["body_id"]==exports[format!("{part}_body_id")]).unwrap();
            assert!((pose["translation"][0].as_f64().unwrap()-angle/360.*2.5).abs()<1e-5,"{part}: {pose}");
        }
        if angle==90. || angle==1440. || angle==48./2.5*360. {
            // The end position also checks the fixed jaw contact and handle
            // clearance, rather than only the thread pair's bounding boxes.
            let arguments=if angle==48./2.5*360. {json!({"clearance_threshold_mm":0.})} else {json!({"occurrence_ids":[exports["screw_occurrence_id"],exports["nut_occurrence_id"]],"clearance_threshold_mm":0.})};
            no_overlap(&client.call("assembly_interference_check",arguments));
        }
    }
    let before_rejection=client.call("cad_project_model",json!({}));
    let rejected=client.rpc("tools/call",json!({"name":"assembly_set_joint_motion","arguments":{"joint_id":exports["screw_joint_id"],"angle_offset_deg":49./2.5*360.,"linear_offset_mm":0}}));
    assert_eq!(rejected["isError"],true,"{rejected}");
    assert_eq!(client.call("cad_project_model",json!({})),before_rejection,"travel rejection is atomic");

    let original_jaw=exports["final_scene"]["bodies"].as_array().unwrap().iter().find(|body|body["id"]==exports["jaw_body_id"]).unwrap();
    let original_jaw_volume=mesh_measurement(original_jaw).2;
    for (name,original,changed,expected_volume_delta) in [
        ("Moving jaw / 60 mm gripping face",60_f64,64_f64,4.*22.*43.6),
        ("Jaw guide left / running clearance",4.8,5.2,-0.4*22.*4.),
    ] {
        let sketch=client.call("sketch_edit",json!({"name":name}));
        let dimension=sketch["dimensions"].as_array().unwrap().iter().find(|dimension|(dimension["value"].as_f64().unwrap()-original).abs()<1e-8).unwrap();
        assert_eq!(dimension["mode"],"driving");
        let constraint_id=dimension["constraint_id"].clone();
        client.call("sketch_edit_dimension",json!({"constraint_id":constraint_id,"text":changed.to_string()}));
        assert_eq!(client.call("sketch_active",json!({}))["dof"]["value"],0);
        client.call("sketch_finish",json!({}));
        let modified=client.call("solid_recompute",json!({}));
        assert_eq!(modified["scene"]["errors"],json!([]));
        let jaw=modified["scene"]["bodies"].as_array().unwrap().iter().find(|body|body["id"]==exports["jaw_body_id"]).unwrap();
        assert!((mesh_measurement(jaw).2-original_jaw_volume-expected_volume_delta).abs()<0.03,"{name}: edit did not change the intended stock/clearance");
        assert_eq!(client.call("assembly_solution",json!({}))["solved"],true);
        client.call("sketch_edit",json!({"name":name}));
        client.call("sketch_edit_dimension",json!({"constraint_id":constraint_id,"text":original.to_string()}));
        client.call("sketch_finish",json!({}));
        let restored=client.call("solid_recompute",json!({}));
        assert_eq!(restored["scene"]["bodies"],exports["final_scene"]["bodies"],"{name}: restore");
    }

    // Edit the actual persisted thread, replay its downstream D cut and
    // restore the modeled helix. This is not an annotation-only thread.
    let mut thread_request=exports["male_thread_request"].clone();
    thread_request["thread"]["representation"]=json!("simplified");
    let simplified=client.call("solid_edit_external_thread",json!({"feature_id":exports["male_thread_feature_id"],"request":thread_request}));
    assert_eq!(simplified["scene"]["errors"],json!([]));
    let screw_volume=|scene:&Value| mesh_measurement(scene["bodies"].as_array().unwrap().iter().find(|body|body["id"]==exports["screw_body_id"]).unwrap()).2;
    assert!(screw_volume(&simplified["scene"])>screw_volume(&exports["final_scene"])+100.);
    let restored_thread=client.call("solid_edit_external_thread",json!({"feature_id":exports["male_thread_feature_id"],"request":exports["male_thread_request"]}));
    assert_eq!(restored_thread["scene"]["bodies"],exports["final_scene"]["bodies"]);
    let mut restored=Client::restore(&exports["final_model"]);
    assert_eq!(restored.call("solid_scene",json!({}))["bodies"],exports["final_scene"]["bodies"]);
    assert_eq!(restored.call("assembly_solution",json!({}))["instance_body_poses"],exports["final_solution"]["instance_body_poses"]);
    let repeated=Client::start().recipe("d-screw-vise");
    for key in ["final_model","final_scene","final_sketches","final_assembly","final_solution","print_model","print_solution"] {
        assert_eq!(exports[key],repeated["exports"][key],"independent vise replay: {key}");
    }
}

fn no_overlap(report:&Value) {
    assert_eq!(report["exact"],true);
    for pair in report["pairs"].as_array().unwrap() {
        assert_eq!(pair["interfering"],false,"{pair}");
        assert!(pair["overlap_volume_mm3"].as_f64().unwrap()<1e-6,"{pair}");
    }
}

/// Inspect the emitted package, including edge incidence of each welded
/// triangle mesh. Actual print geometry must be closed, positive and on-bed.
fn validate_print_3mf(export:&Value,count:usize,bed:[f64;3],path:&std::path::Path) {
    let bytes=BASE64.decode(export["bytes_base64"].as_str().unwrap()).unwrap();
    std::fs::write(path,&bytes).unwrap();
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml=String::new();
    archive.by_name("3D/3dmodel.model").unwrap().read_to_string(&mut xml).unwrap();
    assert!(xml.contains("unit=\"millimeter\""));
    fn attribute<'a>(tag:&'a str,name:&str)->&'a str { tag.split(&format!("{name}=\"")).nth(1).unwrap().split('"').next().unwrap() }
    let mut bounds=Vec::new();
    for object in xml.split("<object ").skip(1) {
        let object=object.split("</object>").next().unwrap();
        let vertices:Vec<[f64;3]>=object.split("<vertex ").skip(1).map(|tag|["x","y","z"].map(|axis|attribute(tag,axis).parse().unwrap())).collect();
        if vertices.is_empty() { continue; }
        let mut min=[f64::INFINITY;3]; let mut max=[f64::NEG_INFINITY;3];
        for point in &vertices { for axis in 0..3 { assert!(point[axis].is_finite()); min[axis]=min[axis].min(point[axis]); max[axis]=max[axis].max(point[axis]); } }
        assert!(min[2].abs()<1e-4,"part is not on print bed: {min:?}");
        for axis in 0..3 { assert!(min[axis]>=-1e-4 && max[axis]<=bed[axis]+1e-4,"outside bed: {min:?}..{max:?}"); }
        let mut edges=std::collections::BTreeMap::<(usize,usize),usize>::new();
        let mut signed_volume=0.;
        for triangle in object.split("<triangle ").skip(1) {
            let indices=["v1","v2","v3"].map(|name|attribute(triangle,name).parse::<usize>().unwrap());
            assert!(indices.iter().all(|index|*index<vertices.len()));
            let [a,b,c]=indices.map(|index|vertices[index]);
            signed_volume+=(a[0]*(b[1]*c[2]-b[2]*c[1])+a[1]*(b[2]*c[0]-b[0]*c[2])+a[2]*(b[0]*c[1]-b[1]*c[0]))/6.;
            for [a,b] in [[indices[0],indices[1]],[indices[1],indices[2]],[indices[2],indices[0]]] { assert_ne!(a,b); *edges.entry((a.min(b),a.max(b))).or_default()+=1; }
        }
        assert!(signed_volume>1.,"non-positive print volume {signed_volume}");
        assert!(!edges.is_empty() && edges.values().all(|incidence|*incidence==2),"non-manifold print mesh");
        bounds.push((min,max));
    }
    assert_eq!(bounds.len(),count);
    for (i,(min,max)) in bounds.iter().enumerate() { for (other_min,other_max) in &bounds[i+1..] { assert!((0..2).any(|axis|max[axis]<other_min[axis] || other_max[axis]<min[axis]),"print bodies overlap"); } }
}

#[test]
fn d_screw_vise_coupon_replays_real_threads_and_exports_printable_meshes() {
    let mut client=Client::start();
    let report=client.recipe("d-screw-vise-fit");
    let exports=&report["exports"];
    let envelope=nbcad_solid::iso_metric_grade6_envelope(20.,2.5,nbcad_solid::ThreadFit::External).unwrap();
    let female=nbcad_solid::iso_metric_grade6_envelope(20.5,2.5,nbcad_solid::ThreadFit::Internal).unwrap();
    let radial_clearance_min=(female.pitch_min-envelope.pitch_max)/2.;
    let radial_clearance_max=(female.pitch_max-envelope.pitch_min)/2.;
    assert!(radial_clearance_min>=0.25 && radial_clearance_max<0.7);
    let (major,pitch,minor)=(envelope.modeled_major/2.,envelope.modeled_pitch/2.,envelope.modeled_minor/2.);
    let slope=1./3_f64.sqrt();
    let root_half=2.5/4.-(pitch-minor)*slope;
    let outer_half=2.5/4.+(major-pitch)*slope;
    let mean_radius_squared=(2.*root_half*minor.powi(2)+2.*(outer_half-root_half)*(minor.powi(2)+minor*major+major.powi(2))/3.+(2.5-2.*outer_half)*major.powi(2))/2.5;
    let analytic_volume=std::f64::consts::PI*25.*mean_radius_squared/2.;
    let screw=exports["final_scene"]["bodies"].as_array().unwrap().iter().find(|body|body["id"]==exports["screw_body_id"]).unwrap();
    let (min,max,volume)=mesh_measurement(screw);
    assert!((min[0]-0.).abs()<1e-5 && (max[0]-25.).abs()<1e-5 && min[2].abs()<1e-5);
    assert!((volume-analytic_volume).abs()/analytic_volume<0.01,"coupon measured {volume}, axial-profile integral {analytic_volume}");
    let directory=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../native-target/vise-artifacts");
    std::fs::create_dir_all(&directory).unwrap();
    let exported=client.call("solid_export_3mf",json!({"slicer_target":"standard"}));
    validate_print_3mf(&exported,2,[235.5,256.,256.],&directory.join("d-screw-vise-fit.3mf"));
    std::fs::write(directory.join("fit-model.json"),serde_json::to_vec_pretty(&exports["final_model"]).unwrap()).unwrap();
    let repeated=Client::start().recipe("d-screw-vise-fit");
    for key in ["final_scene","final_model","final_sketches"] { assert_eq!(exports[key],repeated["exports"][key],"coupon replay {key}"); }
    assert_eq!(Client::restore(&exports["final_model"]).call("solid_scene",json!({}))["bodies"],exports["final_scene"]["bodies"]);
}
