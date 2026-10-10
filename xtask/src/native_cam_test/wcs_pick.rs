//! Real guarded Rust MCP / private Linux XTEST proof of the existing WCS form.
use super::*;
use crate::native_platform_test::Driver;
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub(super) fn datum(c: &mut Client) -> Result<Value> {
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_point",
        json!({"position":{"x":12.,"y":8.},"ctrl_held":true}),
    )?;
    let sketch = crate::native_fixture::sketch(c)?;
    let point = sketch["entities"]
        .as_array()
        .context("Standalone point entities")?
        .iter()
        .find(|entity| entity["kind"] == "point")
        .context("Standalone WCS point missing")?;
    let reference = json!({"sketch":sketch["name"],"entity_id":point["id"],"point":[12.,8.,0.]});
    control(c, "Finish sketch", None)?;
    Ok(reference)
}
fn row<'a>(state: &'a Value, label: &str) -> Option<&'a Value> {
    controls(state).find(|row| row["surface"] == "cam/document" && row["label"] == label)
}
fn wait(c: &mut Client, active: bool) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if row(&state, "Viewport WCS origin")
            .is_some_and(|row| row["selected"] == active && row["disabled"] == false)
        {
            return Ok(state);
        }
        ensure!(
            Instant::now() < deadline,
            "WCS picker did not reach active={active}: {state}"
        );
        thread::sleep(Duration::from_millis(30));
    }
}
fn picking(c: &mut Client) -> Result<Value> {
    panel_field(
        c,
        "Viewport WCS origin",
        None,
        "Previous fields",
        "More fields",
    )?;
    wait(c, true)
}
fn contains(bounds: &Value, point: [f64; 2]) -> bool {
    let Some(x) = bounds["x"].as_f64() else {
        return false;
    };
    let Some(y) = bounds["y"].as_f64() else {
        return false;
    };
    let Some(w) = bounds["width"].as_f64() else {
        return false;
    };
    let Some(h) = bounds["height"].as_f64() else {
        return false;
    };
    point[0] >= x && point[1] >= y && point[0] < x + w && point[1] < y + h
}
fn xyz(value: &Value) -> Result<[f64; 3]> {
    Ok([
        value["x"].as_f64().context("X")?,
        value["y"].as_f64().context("Y")?,
        value["z"].as_f64().context("Z")?,
    ])
}
fn point(value: [f64; 3]) -> Value {
    json!({"x":value[0],"y":value[1],"z":value[2]})
}
fn bounds(solid: &Value, ids: &Value) -> Result<Value> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for body in solid["bodies"].as_array().context("Solid bodies")? {
        if !ids
            .as_array()
            .context("Setup bodies")?
            .contains(&body["id"])
        {
            continue;
        }
        for p in body["mesh"]["positions"]
            .as_array()
            .context("Body mesh")?
            .as_chunks::<3>()
            .0
        {
            for axis in 0..3 {
                let v = p[axis].as_f64().context("Mesh coordinate")?;
                min[axis] = min[axis].min(v);
                max[axis] = max[axis].max(v);
            }
        }
    }
    ensure!(
        min.iter().chain(&max).all(|v| v.is_finite()),
        "Finite included model bounds"
    );
    Ok(json!({"min":point(min),"max":point(max)}))
}
fn stock_in_setup(bounds: &Value, wcs: &Value) -> Result<Value> {
    let lo = xyz(&bounds["min"])?;
    let hi = xyz(&bounds["max"])?;
    let origin = xyz(&wcs["origin"])?;
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for x in [lo[0], hi[0]] {
        for y in [lo[1], hi[1]] {
            for z in [lo[2], hi[2]] {
                let delta = [x - origin[0], y - origin[1], z - origin[2]];
                for (axis, name) in ["x_axis", "y_axis", "z_axis"].into_iter().enumerate() {
                    let value = (0..3)
                        .map(|i| delta[i] * wcs[name][i].as_f64().unwrap())
                        .sum::<f64>();
                    min[axis] = min[axis].min(value);
                    max[axis] = max[axis].max(value);
                }
            }
        }
    }
    Ok(json!({"min":point(min),"max":point(max)}))
}
fn gesture(driver: &Driver, state: &Value, point: [f64; 2], cancel: bool) -> Result<Value> {
    ensure!(
        !controls(state).any(|control| contains(&control["bounds"], point)),
        "WCS handle is covered by a control"
    );
    let response=driver.invoke("cam-row-drag",Some(&json!({"client":state["ui"]["client"],"x":point[0],"y":point[1],"points":[{"x":point[0],"y":point[1],"hold_ms":0}],"cancel":cancel}).to_string()))?;
    serde_json::from_str(&response).context("OS WCS picking omitted a valid input receipt")
}
pub(super) fn exercise(
    c: &mut Client,
    out: &Path,
    server: &str,
    solid: &Value,
    datum: &Value,
) -> Result<()> {
    ensure!(
        cfg!(target_os = "windows") || cfg!(target_os = "linux"),
        "Physical WCS proof supports Windows or private Linux Xvfb"
    );
    let state = ui(c, json!({"action":"inspect"}))?;
    let session = state["active_session_id"].as_str().context("CAM session")?;
    let pid = crate::native_drawing_navigation_test::owned_pid(out, session, server)?;
    let driver = Driver::new(pid, out)?;
    driver.event("focus")?;
    let incoming = project_model(c)?;
    let mut cases = vec![];
    for mode in ["stock_box_point", "model_box_point", "sketch_point"] {
        let before_raw = c.call("cad_project_model", json!({}))?;
        let before = project_model(c)?;
        let cam = document(c)?;
        let mut expected_cam = cam.clone();
        let expected_setup = &mut expected_cam["setups"][0];
        let mut stock_box = expected_setup["stock_model_box"].clone();
        if mode == "stock_box_point" {
            field(c, "Stock +X allowance (mm)", "3.25")?;
            let model = bounds(solid, &expected_setup["body_ids"])?;
            stock_box["max"]["x"] = json!(model["max"]["x"].as_f64().unwrap() + 3.25);
            expected_setup["stock_spec"]["offsets"]["x_max"] = json!(3.25);
        }
        field(c, "WCS origin", mode)?;
        let source = if mode == "model_box_point" {
            bounds(solid, &expected_setup["body_ids"])?
        } else {
            stock_box.clone()
        };
        let world = if mode == "sketch_point" {
            [12., 8., 0.]
        } else {
            xyz(&source["max"])?
        };
        let origin = if mode == "sketch_point" {
            json!({"mode":mode,"sketch":datum["sketch"],"entity_id":datum["entity_id"]})
        } else {
            json!({"mode":mode,"x":"max","y":"max","z":"max"})
        };
        expected_setup["wcs_origin"] = origin;
        expected_setup["wcs"]["origin"] = point(world);
        expected_setup["stock_model_box"] = stock_box.clone();
        expected_setup["stock"] = stock_in_setup(&stock_box, &expected_setup["wcs"])?;
        let view = ui(
            c,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
        )?;
        let ready = picking(c)?;
        let canvas = ready["ui"]["canvases"]
            .as_array()
            .context("Canvases")?
            .iter()
            .find(|canvas| canvas["name"] == "viewport")
            .context("Viewport")?;
        let screen = crate::native_move_test::project(&view["value"]["camera"], canvas, world);
        ensure!(
            contains(canvas, screen),
            "WCS point is outside the viewport"
        );
        capture(c, out, &format!("cam-wcs-{mode}-handles"))?;
        let input = gesture(&driver, &ready, screen, false)?;
        let picked = wait(c, false)?;
        ensure!(
            project_model(c)? == before,
            "Physical WCS pick committed before Apply"
        );
        capture(c, out, &format!("cam-wcs-{mode}-staged"))?;
        control(c, "Apply", None)?;
        ensure!(
            document(c)? == expected_cam,
            "Physical WCS Apply changed unexpected CAM data: {}",
            document(c)?
        );
        let after_raw = c.call("cad_project_model", json!({}))?;
        let after = project_model(c)?;
        let mut expected = before.clone();
        expected["cam"] = expected_cam;
        ensure!(
            after == expected,
            "WCS Apply changed unrelated project intent"
        );
        history(c, &before_raw, &after_raw)?;
        capture(c, out, &format!("cam-wcs-{mode}-applied"))?;
        let path = out.join(format!("cam-wcs-{mode}.limo"));
        ui(c, json!({"action":"file","command":"save","path":path}))?;
        let mut archive = zip::ZipArchive::new(fs::File::open(&path)?)?;
        let archived: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
        ensure!(
            archived == after,
            "Saved WCS archive differs from complete model"
        );
        control(c, "Undo", None)?;
        ensure!(project_model(c)? == before, "WCS Undo lost baseline");
        field(c, "WCS origin", mode)?;
        let ready = picking(c)?;
        let cancel = gesture(&driver, &ready, screen, true)?;
        wait(c, false)?;
        ensure!(
            project_model(c)? == before,
            "Canceled WCS pick changed intent"
        );
        control(c, "Reset", None)?;
        control(c, "Redo", None)?;
        ensure!(
            project_model(c)? == after,
            "Canceled WCS pick lost exact Redo"
        );
        control(c, "Undo", None)?;
        ensure!(
            project_model(c)? == incoming,
            "WCS fixture failed to restore incoming model"
        );
        cases.push(json!({"mode":mode,"point":world,"screen":screen,"input":input,"cancel_input":cancel,"ready":ready,"picked":picked,
            "before":before,"expected":expected,"after":after,"archive_model":archived,"draft_only":true,"exact_apply_undo_redo":true,"cancel_preserves_redo":true}));
    }
    fs::write(
        out.join("cam-wcs-os-picking.json"),
        serde_json::to_vec_pretty(
            &json!({"platform":std::env::consts::OS,"owned_pid":pid,"cases":cases,"incoming_restored":true,"pixel_review":"required"}),
        )?,
    )?;
    Ok(())
}
