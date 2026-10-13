//! Real motion-studio controls with geometry isolation and exact saved intent.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
fn field(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    panel_field(
        c,
        label,
        value,
        "Scroll assembly up",
        "Scroll assembly down",
    )
}
fn assembly(c: &mut Client) -> Result<Value> {
    c.call("assembly_document", json!({}))
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-studies")?;
    let c = &mut fixture.client;
    if controls(&ui(c, json!({"action":"inspect"}))?).any(|v| v["label"] == "Back to model browser")
    {
        control(c, "Back to model browser", None)?;
    }
    for i in 0..2 {
        begin_sketch(c, "XY")?;
        c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":i*40,"y":0},"p2":{"x":i*40+20,"y":10},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    }
    crate::native_joint_test::open(c, "slider")?;
    control(c, "Apply joint", None)?;
    let source = c.call("solid_scene", json!({}))?;
    control(c, "Assembly Motion", None)?;
    field(c, "Create first motion study", None)?;
    let initial = assembly(c)?;
    field(c, "Study duration seconds", Some("0"))?;
    ensure!(
        controls(&ui(c, json!({"action":"inspect"}))?)
            .any(|v| v["label"] == "Apply motion study" && v["disabled"] == true),
        "Invalid study duration accepted"
    );
    field(c, "Study duration seconds", Some("2"))?;
    field(c, "Motion study name", Some("Native travel"))?;
    field(c, "Study playback speed", Some("5"))?;
    field(c, "Add motion driver", None)?;
    field(c, "Driver 1 name", Some("Slide travel"))?;
    field(c, "Driver 1 keyframe 1 interpolation", Some("linear"))?;
    field(c, "Driver 1 keyframe 2 value", Some("12"))?;
    field(c, "Driver 1 keyframe 2 interpolation", Some("linear"))?;
    ensure!(
        assembly(c)? == initial,
        "Study drafts changed model before Apply"
    );
    field(c, "Apply motion study", None)?;
    let keyed = assembly(c)?;
    ensure!(
        keyed["motion_studies"][0]["duration_seconds"] == 2.
            && keyed["motion_studies"][0]["drivers"][0]["law"]["keyframes"][1]["time_seconds"]
                == 2.,
        "Duration did not resize keyframes"
    );
    control(c, "Undo", None)?;
    ensure!(
        assembly(c)? == initial,
        "Study Undo changed original intent"
    );
    control(c, "Redo", None)?;
    ensure!(assembly(c)? == keyed, "Study Redo lost drivers");
    field(c, "Motion study time", Some("1"))?;
    ensure!(
        assembly(c)? == keyed,
        "Timeline preview mutated saved model"
    );
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    capture(c, &fixture.out, "study-keyframes")?;
    field(c, "Capture assembly position", None)?;
    let posed = assembly(c)?;
    ensure!(
        (posed["positions"][0]["motions"][0]["linear_offset_mm"]
            .as_f64()
            .context("Captured motion absent")?
            - 6.)
            .abs()
            < 1e-8,
        "Capture did not retain preview coordinates"
    );
    field(c, "Position 1 name", Some("Half travel"))?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let id = controls(&state)
        .find(|v| v["label"] == "Position 1 name")
        .context("Position name missing")?["id"]
        .clone();
    ui(c, json!({"action":"key","target":id,"key":"Enter"}))?;
    ensure!(
        assembly(c)?["positions"][0]["name"] == "Half travel",
        "Position name Enter did not apply"
    );
    let before = assembly(c)?;
    field(c, "Apply position 1", None)?;
    let applied = assembly(c)?;
    ensure!(
        (applied["joints"][0]["linear_offset_mm"].as_f64().unwrap() - 6.).abs() < 1e-8,
        "Named position did not solve"
    );
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == before, "Apply position Undo failed");
    control(c, "Redo", None)?;
    ensure!(assembly(c)? == applied, "Apply position Redo failed");
    field(c, "Driver 1 Motor", None)?;
    field(c, "Driver 1 motor Start", Some("0"))?;
    field(c, "Driver 1 motor Speed / s", Some("4"))?;
    field(c, "Driver 1 motor Accel / s²", Some("2"))?;
    field(c, "Apply motion study", None)?;
    let motor = assembly(c)?;
    let sample = c.call(
        "cad_interface",
        json!({"action":"execute","group":"assembly/motion","operation":"assembly_evaluate_motion_study","arguments":{"study_id":1,"time_seconds":1}}),
    )?;
    ensure!(
        (sample["sample"]["joint_motions"][0]["linear_offset_mm"]
            .as_f64()
            .context("Motor sample missing")?
            - 5.)
            .abs()
            < 1e-8,
        "Motor law lost acceleration"
    );
    field(c, "Motion study time", Some("1"))?;
    capture(c, &fixture.out, "study-motor")?;
    field(c, "Stop motion study", None)?;
    field(c, "Play motion study", None)?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state).any(|v| v["label"] == "Play motion study") {
            break;
        }
        ensure!(
            std::time::Instant::now() < until,
            "Playback failed to finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    ensure!(
        assembly(c)? == motor,
        "Playback changed saved joint design state"
    );
    field(c, "Stop motion study", None)?;
    field(c, "Loop playback", None)?;
    field(c, "Study playback speed", Some("1"))?;
    field(c, "Apply motion study", None)?;
    field(c, "Play motion study", None)?;
    field(c, "Pause motion study", None)?;
    field(c, "Stop motion study", None)?;
    field(c, "Driver 1 Keyframes", None)?;
    field(c, "Add driver 1 keyframe", None)?;
    field(c, "Revert motion study changes", None)?;
    let csv = c.call(
        "cad_interface",
        json!({"action":"execute","group":"assembly/motion","operation":"assembly_export_motion_path_csv","arguments":{"study_id":1,"sample_rate_hz":10,"occurrence_ids":[]}}),
    )?;
    let csv = csv.as_str().context("CSV result missing")?;
    ensure!(csv.lines().count() > 10, "Motion path empty");
    std::fs::write(fixture.out.join("motion-path.csv"), csv)?;
    field(c, "Create motion study", None)?;
    field(c, "Motion study", Some("2"))?;
    field(c, "Delete motion study", None)?;
    field(c, "Motion study", Some("1"))?;
    let before = assembly(c)?;
    field(c, "Delete position 1", None)?;
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == before, "Position delete Undo failed");
    field(c, "Delete driver 1", None)?;
    field(c, "Apply motion study", None)?;
    ensure!(
        assembly(c)?["motion_studies"][0]["drivers"]
            .as_array()
            .unwrap()
            .is_empty(),
        "Driver delete failed"
    );
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == before, "Driver delete Undo failed");
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "Motion studio changed source geometry"
    );
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    capture(c, &fixture.out, "study-final")?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({"passed":true,"assembly":assembly(c)?}))?,
    )?;
    println!("PASS native motion studio: typed drivers/keyframes, duration scaling, named positions, preview/playback/pause/loop/stop, CSV, validation, exact history and saved source geometry");
    Ok(())
}
