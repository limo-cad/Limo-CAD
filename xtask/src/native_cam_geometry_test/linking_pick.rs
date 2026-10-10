//! Physical marker clicks preserve the existing operation-keyed linking DTO.
use super::super::viewport_pick::{click, contains, inspect, row, wait};
use super::*;
use crate::native_platform_test::Driver;

fn link<'a>(cam: &'a Value, id: &Value) -> Result<&'a Value> {
    cam["linking"]
        .as_array()
        .context("Linking records")?
        .iter()
        .find(|link| &link["operation_id"] == id)
        .context("Contour linking record")
}
fn link_mut<'a>(cam: &'a mut Value, id: &Value) -> Result<&'a mut Value> {
    cam["linking"]
        .as_array_mut()
        .context("Linking records")?
        .iter_mut()
        .find(|link| &link["operation_id"] == id)
        .context("Contour linking record")
}
fn xyz(value: &Value) -> [f64; 3] {
    ["x", "y", "z"].map(|axis| value[axis].as_f64().unwrap())
}
fn to_model(wcs: &Value, p: [f64; 3]) -> [f64; 3] {
    let origin = xyz(&wcs["origin"]);
    std::array::from_fn(|i| {
        origin[i]
            + p[0] * wcs["x_axis"][i].as_f64().unwrap()
            + p[1] * wcs["y_axis"][i].as_f64().unwrap()
            + p[2] * wcs["z_axis"][i].as_f64().unwrap()
    })
}
fn to_setup(wcs: &Value, p: [f64; 3]) -> Value {
    let origin = xyz(&wcs["origin"]);
    let dot = |axis: &str| {
        (0..3)
            .map(|i| (p[i] - origin[i]) * wcs[axis][i].as_f64().unwrap())
            .sum::<f64>()
    };
    json!({"x":dot("x_axis"),"y":dot("y_axis")})
}
fn button(target: &str) -> &'static str {
    match target {
        "predrill_positions" => "Viewport predrill position",
        "entry_positions" => "Viewport preferred entry position",
        _ => "Viewport preferred exit position",
    }
}
fn picking(c: &mut Client, target: &str) -> Result<Value> {
    panel_field(c, button(target), None, "Previous fields", "More fields")?;
    wait(
        c,
        |state| {
            row(state, button(target))
                .is_some_and(|row| row["selected"] == true && row["disabled"] == false)
        },
        "Linking picker did not become ready",
    )
}
fn finished(c: &mut Client, target: &str) -> Result<Value> {
    wait(
        c,
        |state| row(state, button(target)).is_none_or(|row| row["selected"] == false),
        "Linking picker did not finish the release",
    )
}
fn vertex(
    solid: &Value,
    setup: &Value,
    camera: &Value,
    canvas: &Value,
    state: &Value,
    previous: &Value,
) -> Result<([f64; 3], [f64; 2], Value, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut points = vec![];
    for body in solid["bodies"].as_array().context("Solid bodies")? {
        if !setup["body_ids"]
            .as_array()
            .context("Included bodies")?
            .contains(&body["id"])
        {
            continue;
        }
        for (index, point) in body["mesh"]["positions"]
            .as_array()
            .context("Mesh positions")?
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
        {
            if points.len() >= 2000 {
                break;
            }
            let p = std::array::from_fn(|i| point[i].as_f64().unwrap());
            if !seen.insert(format!("{:.5}:{:.5}:{:.5}", p[0], p[1], p[2])) {
                continue;
            }
            points.push((p, format!("vertex:{}:{index}", body["id"])));
        }
    }
    for (index, (p, key)) in points.iter().enumerate() {
        let screen = crate::native_move_test::project(camera, canvas, *p);
        let xy = to_setup(&setup["wcs"], *p);
        if !contains(canvas, screen)
            || controls(state).any(|row| contains(&row["bounds"], screen))
            || previous == &json!([xy.clone()])
        {
            continue;
        }
        if points.iter().enumerate().any(|(other, (p, _))| {
            other != index && {
                let q = crate::native_move_test::project(camera, canvas, *p);
                (q[0] - screen[0]).hypot(q[1] - screen[1]) < 3.
            }
        }) {
            continue;
        }
        return Ok((*p, screen, xy, key.clone()));
    }
    anyhow::bail!("No distinct uncovered linking vertex with an unambiguous physical pixel target")
}
pub(super) fn exercise(c: &mut Client, out: &Path, server: &str) -> Result<Value> {
    let incoming = model(c)?;
    let original = document(c)?;
    let contour = operation(&original)?.clone();
    let id = contour["id"].clone();
    let mut seeded = original.clone();
    let tool_id = seeded["next_tool_id"].as_u64().context("Next tool")?;
    let drill_id = seeded["next_operation_id"]
        .as_u64()
        .context("Next operation")?;
    let mut tool = seeded["tools"][0].clone();
    tool["id"] = json!(tool_id);
    tool["number"] = json!(99);
    tool["name"] = json!("Link picker pilot drill");
    tool["kind"] = json!("drill");
    tool["diameter"] = json!(3.);
    tool["point_angle_degrees"] = json!(118.);
    seeded["tools"].as_array_mut().unwrap().push(tool);
    seeded["next_tool_id"] = json!(tool_id + 1);
    seeded["next_operation_id"] = json!(drill_id + 1);
    let stock = &seeded["setups"][0]["stock"];
    let lo = xyz(&stock["min"]);
    let hi = xyz(&stock["max"]);
    let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, hi[2]];
    let drill = json!({"kind":"drill","id":drill_id,"name":"Earlier pilot drill","enabled":true,"tool_id":tool_id,
        "points":[{"x":center[0],"y":center[1]}],"holes":[],"top_z":hi[2],"bottom_z":lo[2],"clearance_z":hi[2]+8.,"retract_z":hi[2]+2.,"feed_height_z":hi[2]+1.,"cutting":contour["cutting"]});
    seeded["setups"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .insert(0, drill);
    c.call("cam_set_document", seeded.clone())?;
    let seeded = document(c)?;
    let mut retained = seeded.clone();
    retained["tools"]
        .as_array_mut()
        .unwrap()
        .retain(|tool| tool["id"] != tool_id);
    retained["setups"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .retain(|operation| operation["id"] != drill_id);
    retained["next_tool_id"] = original["next_tool_id"].clone();
    retained["next_operation_id"] = original["next_operation_id"].clone();
    ensure!(
        retained == original,
        "Adding the fixture's pilot drill changed unrelated CAM records"
    );
    ensure!(
        seeded["setups"][0]["operations"][0]["kind"] == "drill"
            && seeded["setups"][0]["operations"][0]["enabled"] == true
            && seeded["setups"][0]["operations"][0]["points"]
                == json!([{"x":center[0],"y":center[1]}]),
        "Canonical pilot drill differs from the requested source center"
    );
    control(c, "Toolpaths", None)?;
    control(
        c,
        &format!(
            "{} / {}",
            seeded["setups"][0]["name"].as_str().unwrap(),
            contour["name"].as_str().unwrap()
        ),
        None,
    )?;
    section(c, "linking")?;
    let state = inspect(c)?;
    let session = state["active_session_id"]
        .as_str()
        .context("Owned session")?;
    let pid = crate::native_drawing_navigation_test::owned_pid(out, session, server)?;
    let driver = Driver::new(pid, out)?;
    driver.event("focus")?;
    let baseline = model(c)?;
    let solid = scene(c)?;
    let mut cases = vec![];
    for target in ["predrill_positions", "entry_positions", "exit_positions"] {
        let view = ui(
            c,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
        )?;
        let ready = picking(c, target)?;
        let canvas = ready["ui"]["canvases"]
            .as_array()
            .context("Canvases")?
            .iter()
            .find(|canvas| canvas["name"] == "viewport")
            .context("Viewport")?;
        let setup = &seeded["setups"][0];
        let (world, screen, xy, key) = if target == "predrill_positions" {
            let p = to_model(&setup["wcs"], center);
            (
                p,
                crate::native_move_test::project(&view["value"]["camera"], canvas, p),
                json!({"x":center[0],"y":center[1]}),
                format!("drill:{drill_id}:0"),
            )
        } else {
            vertex(
                &solid,
                setup,
                &view["value"]["camera"],
                canvas,
                &ready,
                &link(&seeded, &id)?[target],
            )?
        };
        ensure!(contains(canvas, screen), "Linking station outside viewport");
        capture(c, out, &format!("geometry-linking-{target}-handles"))?;
        let input = click(&driver, c, screen)?;
        let picked = finished(c, target)?;
        ensure!(
            model(c)? == baseline,
            "Linking point pick committed before Apply"
        );
        capture(c, out, &format!("geometry-linking-{target}-staged"))?;
        let mut expected_cam = seeded.clone();
        let linked = link_mut(&mut expected_cam, &id)?;
        if target == "predrill_positions" {
            linked[target].as_array_mut().unwrap().push(xy.clone());
        } else {
            linked[target] = json!([xy]);
        }
        control(c, "Apply", None)?;
        ensure!(
            document(c)? == expected_cam,
            "Physical linking copy changed unrelated CAM records"
        );
        let after = model(c)?;
        let mut expected = baseline.clone();
        expected["cam"] = expected_cam;
        ensure!(
            after == expected,
            "Linking Apply changed unrelated project data"
        );
        history(c, &baseline, &after)?;
        capture(c, out, &format!("geometry-linking-{target}-applied"))?;
        let archived = save(
            c,
            &out.join(format!("geometry-linking-{target}-picked.limo")),
        )?;
        control(c, "Undo", None)?;
        ensure!(
            model(c)? == baseline,
            "Linking Undo did not restore exact baseline"
        );
        section(c, "linking")?;
        let ready = picking(c, target)?;
        let cancel=driver.invoke("cam-row-drag",Some(&json!({"client":ready["ui"]["client"],"x":screen[0],"y":screen[1],"points":[{"x":screen[0],"y":screen[1],"hold_ms":0}],"cancel":true}).to_string()))?;
        finished(c, target)?;
        ensure!(
            model(c)? == baseline,
            "Canceled linking pick changed the document"
        );
        control(c, "Reset", None)?;
        control(c, "Redo", None)?;
        ensure!(
            model(c)? == after,
            "Canceled linking picker lost exact Redo"
        );
        control(c, "Undo", None)?;
        ensure!(
            model(c)? == baseline,
            "Linking case did not restore its baseline"
        );
        cases.push(json!({"target":target,"identity":key,"world_point":world,"screen_point":screen,"input":input,"cancel_input":cancel,"picked":picked,"expected":expected,"after":after,"archive_model":archived,"draft_only":true,"exact_history":true,"cancel_preserves_redo":true}));
    }
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == incoming,
        "Linking fixture did not remove its prior-drill seed exactly"
    );
    let evidence = json!({"platform":std::env::consts::OS,"owned_pid":pid,"cases":cases,"baseline":baseline,"incoming_restored":true,"pixel_review":"required"});
    std::fs::write(
        out.join("geometry-linking-os-picking.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(evidence)
}
