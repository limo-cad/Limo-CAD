//! Component dragging through the rendered canvas and shared joint solver.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui},
    replay::Client,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn drag(
    c: &mut Client,
    out: &Path,
    session: &str,
    server: &str,
    stage: &str,
    from: [f64; 2],
    to: [f64; 2],
) -> Result<()> {
    if std::env::var("LIMO_CAD_NATIVE_MECHANISM_INPUT").as_deref() != Ok("1") {
        ui(
            c,
            json!({"action":"viewport","gesture":"drag","point":from,"to":to}),
        )?;
        return Ok(());
    }
    let pid = crate::native_drawing_navigation_test::owned_pid(out, session, server)?;
    let driver = crate::native_platform_test::Driver::new(pid, out)?;
    driver.event("focus")?;
    let snapshot = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        snapshot["active_session_id"] == session && snapshot["attached_session_id"] == session,
        "Mechanism input no longer targets the acknowledged history session"
    );
    let request = json!({"x":from[0],"y":from[1],"to_x":to[0],"to_y":to[1],
        "client":snapshot["ui"]["client"]});
    let reply = driver.invoke("drawing-drag", Some(&request.to_string()))?;
    std::fs::write(out.join(format!("{stage}-os-input.json")), reply)?;
    crate::native_fixture::inspect_after_gesture(c, out, stage, &snapshot["active_session_id"])?;
    Ok(())
}

fn wait_pose(c: &mut Client, expected: impl Fn(f64) -> bool) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = assembly(c)?;
        if current["joints"][0]["linear_offset_mm"]
            .as_f64()
            .is_some_and(&expected)
        {
            return Ok(current);
        }
        ensure!(
            Instant::now() < deadline,
            "Mechanism pose did not reach the expected state: {current}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}
fn assembly(c: &mut Client) -> Result<Value> {
    c.call("assembly_document", json!({}))
}
fn frame(c: &mut Client, body: u64) -> Result<[f64; 2]> {
    frame_view(c, body, "front")
}
fn frame_view(c: &mut Client, body: u64, view: &str) -> Result<[f64; 2]> {
    ui(
        c,
        json!({"action":"view","view":view,"body_id":body,"fit":true,"duration_ms":0}),
    )?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let b = state["ui"]["canvases"]
        .as_array()
        .context("Canvas missing")?
        .iter()
        .find(|c| c["name"] == "viewport")
        .context("Viewport missing")?;
    Ok([
        b["x"].as_f64().unwrap() + b["width"].as_f64().unwrap() / 2.,
        b["y"].as_f64().unwrap() + b["height"].as_f64().unwrap() / 2.,
    ])
}
fn canvas_span(c: &mut Client) -> Result<f64> {
    let state = ui(c, json!({"action":"inspect"}))?;
    let b = state["ui"]["canvases"]
        .as_array()
        .context("Canvas missing")?
        .iter()
        .find(|c| c["name"] == "viewport")
        .context("Viewport missing")?;
    Ok(b["width"]
        .as_f64()
        .unwrap()
        .min(b["height"].as_f64().unwrap()))
}
fn viewport(c: &mut Client, request: Value) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match c.call("cad_interface", request.clone()) {
            Ok(result) if result["status"] == "applied" => return Ok(result),
            Ok(result) => bail!("Interface failed: {result}"),
            Err(error) => {
                let message = error.to_string();
                if Instant::now() < deadline && message.contains("still running") {
                    thread::sleep(Duration::from_millis(25));
                    continue;
                }
                return Err(error);
            }
        }
    }
}
struct BodyPose {
    occurrence_id: u64,
    body_id: u64,
    translation: [f64; 3],
    rotation: [f64; 4],
}

fn pose_rows(poses: &Value) -> Result<Vec<BodyPose>> {
    let mut rows = Vec::new();
    for pose in poses.as_array().context("Displayed poses missing")? {
        let numbers = |value: &Value, n: usize| -> Result<Vec<f64>> {
            let items = value.as_array().context("Pose component missing")?;
            ensure!(items.len() == n, "Pose component has the wrong length");
            items
                .iter()
                .map(|item| item.as_f64().context("Pose component is not numeric"))
                .collect()
        };
        let translation = numbers(&pose["translation"], 3)?;
        let rotation = numbers(&pose["rotation"], 4)?;
        rows.push(BodyPose {
            occurrence_id: pose["occurrence_id"]
                .as_u64()
                .context("Occurrence missing")?,
            body_id: pose["body_id"].as_u64().context("Body missing")?,
            translation: [translation[0], translation[1], translation[2]],
            rotation: [rotation[0], rotation[1], rotation[2], rotation[3]],
        });
    }
    rows.sort_by_key(|row| (row.occurrence_id, row.body_id));
    Ok(rows)
}
fn same_poses(left: &Value, right: &Value) -> Result<bool> {
    let left = pose_rows(left)?;
    let right = pose_rows(right)?;
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-3;
    Ok(left.len() == right.len()
        && left.iter().zip(&right).all(|(a, b)| {
            a.occurrence_id == b.occurrence_id
                && a.body_id == b.body_id
                && a.translation
                    .iter()
                    .zip(b.translation)
                    .all(|(a, b)| close(*a, b))
                && a.rotation.iter().zip(b.rotation).all(|(a, b)| close(*a, b))
        }))
}
fn poses_moved(preview: &Value, solved: &Value) -> Result<bool> {
    let preview = pose_rows(preview)?;
    let solved = pose_rows(solved)?;
    ensure!(
        preview.len() == solved.len() && !preview.is_empty(),
        "Displayed poses missing"
    );
    Ok(preview.iter().zip(&solved).any(|(preview, solved)| {
        preview
            .translation
            .iter()
            .zip(solved.translation)
            .any(|(preview, solved)| (preview - solved).abs() > 0.05)
            || preview
                .rotation
                .iter()
                .zip(solved.rotation)
                .any(|(preview, solved)| (preview - solved).abs() > 1e-3)
    }))
}
fn displayed_poses(c: &mut Client, at: [f64; 2]) -> Result<Value> {
    let value = viewport(
        c,
        json!({"action":"viewport","gesture":"move","point":at,"poses":true}),
    )?;
    Ok(value["value"]["instance_body_poses"].clone())
}
/// Drawing-drag is one atomic press, move, and release, so it cannot deliver
/// the window events `observe()` already cancels on. This gesture reaches that
/// path: hold a preview, then inject unfocus or scale before release.
fn restore_during_drag(c: &mut Client, baseline: &Value, lifecycle: &str) -> Result<()> {
    let solved = c.call("assembly_solution", json!({}))?["instance_body_poses"].clone();
    let point = frame(c, 2)?;
    let end = [point[0], point[1] - 60.];
    viewport(
        c,
        json!({"action":"viewport","gesture":"drag","point":point,"to":end,"release":false}),
    )?;
    let preview = displayed_poses(c, end)?;
    ensure!(
        poses_moved(&preview, &solved)?,
        "Drag preview did not change the displayed poses: {preview}"
    );
    let restored = viewport(
        c,
        json!({"action":"viewport","gesture":"move","point":end,"lifecycle":lifecycle,"poses":true}),
    )?;
    let restored = &restored["value"]["instance_body_poses"];
    ensure!(
        same_poses(restored, &solved)?,
        "Cancelling the drag did not restore the original poses: {restored} vs {solved}"
    );
    ensure!(
        assembly(c)? == *baseline,
        "Cancelling the drag committed a joint motion"
    );
    Ok(())
}
fn limit_axis(c: &mut Client, axis: &str, min: &str, max: &str) -> Result<()> {
    panel_field(
        c,
        &format!("Limit {axis}"),
        None,
        "Scroll joint up",
        "Scroll joint down",
    )?;
    panel_field(
        c,
        &format!("{axis} Minimum"),
        Some(min),
        "Scroll joint up",
        "Scroll joint down",
    )?;
    panel_field(
        c,
        &format!("{axis} Maximum"),
        Some(max),
        "Scroll joint up",
        "Scroll joint down",
    )?;
    Ok(())
}
fn blank_blocks(c: &mut Client) -> Result<String> {
    let new = control(c, "New design", None)?;
    let session = new["active_session_id"]
        .as_str()
        .context("Blank session missing")?
        .to_owned();
    c.call("cad_attach", json!({"session_id": session}))?;
    if controls(&ui(c, json!({"action":"inspect"}))?).any(|v| v["label"] == "Back to model browser")
    {
        control(c, "Back to model browser", None)?;
    }
    for i in 0..2 {
        begin_sketch(c, "XY")?;
        c.call("sketch_add_rectangle", json!({"mode":"two_point","p1":{"x":i*40,"y":0},"p2":{"x":i*40+20,"y":10},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call(
            "solid_extrude",
            json!({"sketch_name":format!("Sketch{}", i + 1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
        )?;
    }
    Ok(session)
}
fn wait_coordinate(c: &mut Client, field: &str, expected: impl Fn(f64) -> bool) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = assembly(c)?;
        if current["joints"][0][field].as_f64().is_some_and(&expected) {
            return Ok(current);
        }
        ensure!(
            Instant::now() < deadline,
            "Mechanism pose did not reach the expected state: {current}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}
fn coordinate_within(value: f64, min: f64, max: f64) -> bool {
    value.is_finite() && value >= min && value <= max
}
fn exercise_joint(c: &mut Client, out: &Path, server: &str, kind: &str) -> Result<()> {
    let session = blank_blocks(c)?;
    crate::native_joint_test::open(c, kind)?;
    if matches!(kind, "revolute" | "cylindrical") {
        limit_axis(c, "Primary rotation", "-12 deg", "20 deg")?;
    }
    if matches!(kind, "slider" | "cylindrical") {
        limit_axis(c, "Slide", "-6", "14")?;
    }
    control(c, "Apply joint", None)?;
    let source = c.call("solid_scene", json!({}))?;
    let before = assembly(c)?;
    let (from, to, field, travel) = if kind == "revolute" {
        let center = frame_view(c, 2, "top")?;
        let offset = canvas_span(c)? * 0.08;
        let from = [center[0] + offset, center[1]];
        (
            from,
            [from[0], from[1] - 50.],
            "angle_offset_deg",
            Box::new(|value: f64| value.abs() > 0.05 && coordinate_within(value, -12., 20.))
                as Box<dyn Fn(f64) -> bool>,
        )
    } else {
        let from = frame(c, 2)?;
        (
            from,
            [from[0], from[1] - 60.],
            "linear_offset_mm",
            Box::new(|value: f64| value > 0.05 && coordinate_within(value, -6., 14.))
                as Box<dyn Fn(f64) -> bool>,
        )
    };
    drag(c, out, &session, server, &format!("{kind}-drag"), from, to)?;
    let moved = wait_coordinate(c, field, travel)?;
    let value = moved["joints"][0][field]
        .as_f64()
        .with_context(|| format!("{kind} coordinate missing"))?;
    ensure!(
        (kind == "revolute" && value.abs() > 0.05 && coordinate_within(value, -12., 20.))
            || (kind == "cylindrical"
                && value > 0.05
                && coordinate_within(value, -6., 14.)
                && coordinate_within(
                    moved["joints"][0]["angle_offset_deg"]
                        .as_f64()
                        .context("Cylindrical angle missing")?,
                    -12.,
                    20.,
                )),
        "{kind} drag left its limits: {moved}"
    );
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "{kind} drag modified source geometry"
    );
    let undo = control(c, "Undo", None)?;
    ensure!(
        undo["active_session_id"].is_string(),
        "{kind} drag Undo lost the history session"
    );
    ensure!(
        assembly(c)? == before,
        "{kind} drag did not commit as one history step"
    );
    Ok(())
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut f = start(args, "native-mechanism")?;
    let c = &mut f.client;
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
    panel_field(
        c,
        "Limit Slide",
        None,
        "Scroll joint up",
        "Scroll joint down",
    )?;
    panel_field(
        c,
        "Slide Minimum",
        Some("-5"),
        "Scroll joint up",
        "Scroll joint down",
    )?;
    panel_field(
        c,
        "Slide Maximum",
        Some("20"),
        "Scroll joint up",
        "Scroll joint down",
    )?;
    control(c, "Apply joint", None)?;
    let source = c.call("solid_scene", json!({}))?;
    let before = assembly(c)?;
    let point = frame(c, 2)?;
    drag(
        c,
        &f.out,
        &f.session,
        &f.server,
        "first-drag",
        point,
        [point[0], point[1] - 60.],
    )?;
    let moved = wait_pose(c, |offset| offset > 0.05 && offset <= 20.)?;
    let offset = moved["joints"][0]["linear_offset_mm"]
        .as_f64()
        .context("Joint coordinate missing")?;
    ensure!(
        offset > 0.05 && offset <= 20.,
        "Drag did not move the joint within its limits: {offset}"
    );
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "Drag modified source geometry"
    );
    capture(c, &f.out, "mechanism-first-drag")?;
    let undo = control(c, "Undo", None)?;
    std::fs::write(
        f.out.join("mechanism-first-undo.json"),
        serde_json::to_vec_pretty(&undo)?,
    )?;
    ensure!(
        assembly(c)? == before,
        "Drag Undo did not restore original coordinates"
    );
    let redo = control(c, "Redo", None)?;
    std::fs::write(
        f.out.join("mechanism-first-redo.json"),
        serde_json::to_vec_pretty(&redo)?,
    )?;
    let resumed_session = redo["active_session_id"]
        .as_str()
        .context("Redo session receipt")?;
    ensure!(
        assembly(c)? == moved,
        "Drag Redo did not restore exact coordinates"
    );
    let point = frame(c, 2)?;
    drag(
        c,
        &f.out,
        resumed_session,
        &f.server,
        "second-drag",
        point,
        [point[0], point[1] + 30.],
    )?;
    let second = wait_pose(c, |position| position < offset)?;
    ensure!(
        second["joints"][0]["linear_offset_mm"].as_f64().unwrap() < offset,
        "Consecutive drag did not follow the displayed pose"
    );
    let undo = control(c, "Undo", None)?;
    std::fs::write(
        f.out.join("mechanism-second-undo.json"),
        serde_json::to_vec_pretty(&undo)?,
    )?;
    let resumed_session = undo["active_session_id"]
        .as_str()
        .context("Undo session receipt")?;
    ensure!(
        assembly(c)? == moved,
        "Consecutive drag created multiple history entries"
    );
    let point = frame(c, 1)?;
    if std::env::var("LIMO_CAD_NATIVE_MECHANISM_INPUT").as_deref() == Ok("1") {
        drag(
            c,
            &f.out,
            resumed_session,
            &f.server,
            "grounded-drag",
            point,
            [point[0] + 40., point[1]],
        )?;
        for _ in 0..10 {
            ensure!(assembly(c)? == moved, "Grounded component moved");
            thread::sleep(Duration::from_millis(25));
        }
    } else {
        let _ = c.call("cad_interface", json!({"action":"viewport","gesture":"drag","point":point,"to":[point[0]+40.,point[1]]}));
        ensure!(assembly(c)? == moved, "Grounded component moved");
    }
    restore_during_drag(c, &moved, "unfocus")?;
    restore_during_drag(c, &moved, "scale")?;
    exercise_joint(c, &f.out, &f.server, "revolute")?;
    exercise_joint(c, &f.out, &f.server, "cylindrical")?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    capture(c, &f.out, "mechanism-final")?;
    ui(
        c,
        json!({"action":"file","command":"save","path":f.project}),
    )?;
    std::fs::write(
        &f.report,
        serde_json::to_string_pretty(
            &json!({"passed":true,"first_travel_mm":offset,"assembly":assembly(c)?,
                "joints":["slider","revolute","cylindrical"],
                "os_input":std::env::var("LIMO_CAD_NATIVE_MECHANISM_INPUT").as_deref() == Ok("1"),
                "pixel_review":"required","not_proven":["physical hardware"]}),
        )?,
    )?;
    println!("PASS native mechanism dragging: slider, revolute, and cylindrical limits, one commit, focus-loss restore, scale cancellation, grounded rejection");
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Limit and pose checks shared with `run`. The rendered drags, the single
    //! undo, and `observe()` cancellation execute in that fixture.
    use super::{coordinate_within, poses_moved, same_poses};
    use serde_json::json;

    fn pose(body: u64, translation: [f64; 3]) -> serde_json::Value {
        json!([{
            "occurrence_id": 1,
            "body_id": body,
            "translation": translation,
            "rotation": [0.0, 0.0, 0.0, 1.0]
        }])
    }

    #[test]
    fn revolute_angle_limit_is_a_closed_interval() {
        assert!(coordinate_within(20.0, -12.0, 20.0));
        assert!(coordinate_within(-12.0, -12.0, 20.0));
        assert!(!coordinate_within(20.1, -12.0, 20.0));
        assert!(!coordinate_within(-12.1, -12.0, 20.0));
    }

    #[test]
    fn cylindrical_slide_and_angle_limits_are_closed_intervals() {
        assert!(coordinate_within(14.0, -6.0, 14.0));
        assert!(coordinate_within(-6.0, -6.0, 14.0));
        assert!(!coordinate_within(14.1, -6.0, 14.0));
        assert!(coordinate_within(0.0, -12.0, 20.0));
    }

    #[test]
    fn restored_poses_match_and_a_preview_does_not() {
        let original = pose(2, [50.0, 5.0, 15.0]);
        let preview = pose(2, [50.0, 5.0, 22.0]);
        assert!(poses_moved(&preview, &original).unwrap());
        assert!(same_poses(&original, &original).unwrap());
        assert!(!same_poses(&preview, &original).unwrap());
    }

    #[test]
    fn scale_cancel_discards_a_shifted_pose() {
        let original = pose(2, [50.0, 5.0, 15.0]);
        let scaled = pose(2, [62.0, 5.0, 15.0]);
        assert!(poses_moved(&scaled, &original).unwrap());
        assert!(same_poses(&original, &original).unwrap());
    }
}
