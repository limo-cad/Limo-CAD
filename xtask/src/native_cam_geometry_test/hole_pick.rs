//! Physical cylindrical-wall selection against the existing real cut solid.
//! All CAM changes use native fields; complete model/history checks isolate it.
use super::viewport_pick::{click, contains, inspect, row, wait};
use super::*;
use crate::native_platform_test::Driver;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

fn canvas(state: &Value) -> Result<&Value> {
    state["ui"]["canvases"]
        .as_array()
        .context("Native canvases")?
        .iter()
        .find(|canvas| canvas["name"] == "viewport")
        .context("Native model canvas")
}
fn count(state: &Value) -> Option<usize> {
    row(state, "Hole count")
        .and_then(|row| row["value"].as_str())
        .and_then(|value| value.parse().ok())
}
fn picking(c: &mut Client) -> Result<()> {
    panel_field(
        c,
        "Viewport geometry",
        None,
        "Previous fields",
        "More fields",
    )?;
    wait(
        c,
        |state| {
            row(state, "Viewport geometry")
                .is_some_and(|row| row["selected"] == true && row["disabled"] == false)
        },
        "Hole picker did not become ready",
    )?;
    Ok(())
}
fn done(c: &mut Client) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match control(c, "Viewport geometry", None) {
            Ok(_) => return Ok(()),
            Err(error)
                if error.to_string().contains("Wait for the selected face")
                    && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(30))
            }
            Err(error) => return Err(error),
        }
    }
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn triangle(body: &Value, indices: &[Value]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            body["mesh"]["positions"][indices[i].as_u64().unwrap() as usize * 3 + j]
                .as_f64()
                .unwrap()
        })
    })
}
/// Fixture-only visibility witness: find the frontmost triangle along a ray
/// to an interior wall point, avoiding a brittle fixed screen coordinate.
fn nearest(solid: &Value, eye: [f64; 3], point: [f64; 3]) -> Option<(u64, u64)> {
    let direction = sub(point, eye);
    let mut best = f64::INFINITY;
    let mut key = None;
    for body in solid["bodies"].as_array()? {
        let indices = body["mesh"]["indices"].as_array()?;
        for face in body["faces"].as_array()? {
            let start = face["first_index"].as_u64()? as usize;
            let end = start + face["index_count"].as_u64()? as usize;
            for chunk in indices.get(start..end)?.as_chunks::<3>().0 {
                let [a, b, c] = triangle(body, chunk);
                let e1 = sub(b, a);
                let e2 = sub(c, a);
                let p = cross(direction, e2);
                let det = dot(e1, p);
                if det.abs() < 1e-12 {
                    continue;
                }
                let t = sub(eye, a);
                let u = dot(t, p) / det;
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let q = cross(t, e1);
                let v = dot(direction, q) / det;
                if v < 0. || u + v > 1. {
                    continue;
                }
                let distance = dot(e2, q) / det;
                if distance >= 0. && distance < best {
                    best = distance;
                    key = Some((body["id"].as_u64()?, face["id"].as_u64()?));
                }
            }
        }
    }
    key
}
fn wall_point(
    solid: &Value,
    state: &Value,
    camera: &Value,
    key: (u64, u64),
) -> Result<([f64; 3], [f64; 2])> {
    let body = solid["bodies"]
        .as_array()
        .context("Bodies")?
        .iter()
        .find(|body| body["id"] == key.0)
        .context("Picked body")?;
    let face = body["faces"]
        .as_array()
        .context("Faces")?
        .iter()
        .find(|face| face["id"] == key.1)
        .context("Picked face")?;
    let eye = std::array::from_fn(|i| camera["position"][i].as_f64().unwrap());
    let indices = body["mesh"]["indices"].as_array().context("Mesh indices")?;
    let start = face["first_index"].as_u64().context("Face range")? as usize;
    let end = start + face["index_count"].as_u64().context("Face range")? as usize;
    let bounds = canvas(state)?;
    let mut candidates = Vec::new();
    for indices in indices[start..end].as_chunks::<3>().0 {
        let tri = triangle(body, indices);
        let point = std::array::from_fn(|i| (tri[0][i] + tri[1][i] + tri[2][i]) / 3.);
        let screen = crate::native_move_test::project(camera, bounds, point);
        if contains(bounds, screen)
            && !controls(state).any(|control| contains(&control["bounds"], screen))
            && nearest(solid, eye, point) == Some(key)
        {
            candidates.push((point, screen));
        }
    }
    candidates.sort_by(|a, b| b.0[2].total_cmp(&a.0[2]));
    candidates
        .into_iter()
        .next()
        .context("No unobscured physical cylindrical-wall triangle")
}
pub(super) fn exercise(c: &mut Client, out: &Path, server: &str, kind: &str) -> Result<Value> {
    ensure!(
        cfg!(target_os = "windows") || cfg!(target_os = "linux"),
        "Physical hole proof requires Windows or private Linux Xvfb"
    );
    let initial = inspect(c)?;
    let session = initial["active_session_id"]
        .as_str()
        .context("CAM session")?;
    let pid = crate::native_drawing_navigation_test::owned_pid(out, session, server)?;
    let driver = Driver::new(pid, out)?;
    driver.event("focus")?;
    let incoming = model(c)?;
    let incoming_cam = document(c)?;
    let saved_hole = operation(&incoming_cam)?["holes"][0].clone();
    let reference = saved_hole["face_key"]
        .as_str()
        .context("Existing canonical hole")?;
    let (body, face) = reference.split_once(':').context("Canonical body:face")?;
    let key = (body.parse::<u64>()?, face.parse::<u64>()?);
    section(c, "geometry")?;
    field(c, "Hole count", "0")?;
    field(c, "Manual center count", "1")?;
    field(c, "Manual center 1 X (mm)", "4")?;
    field(c, "Manual center 1 Y (mm)", "3")?;
    section(c, "heights")?;
    field(c, "Height programming", "absolute")?;
    control(c, "Apply", None)?;
    section(c, "geometry")?;
    let before = model(c)?;
    let cam_before = document(c)?;
    let operation_before = operation(&cam_before)?.clone();
    ensure!(
        operation_before["holes"].is_null()
            || operation_before["holes"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "Baseline still has associated holes"
    );
    ensure!(
        operation_before["points"] == json!([{"x":4.,"y":3.}]),
        "Baseline manual center differs"
    );
    capture(c, out, &format!("geometry-{kind}-holes-baseline"))?;
    let solid = scene(c)?;
    let view = ui(
        c,
        json!({"action":"view","view":"top","fit":true,"duration_ms":0}),
    )?;
    let mut negatives = Vec::new();
    for (name, point) in [("opening", [20., 15., 8.]), ("planar", [3., 3., 8.])] {
        picking(c)?;
        let state = inspect(c)?;
        let screen =
            crate::native_move_test::project(&view["value"]["camera"], canvas(&state)?, point);
        let input = click(&driver, c, screen)?;
        done(c)?;
        let surface = inspect(c)?;
        ensure!(
            count(&surface) == Some(0),
            "{name} selected a nonphysical or occluded cylindrical face"
        );
        ensure!(
            model(c)? == before,
            "Negative physical hole pick mutated the model"
        );
        capture(c, out, &format!("geometry-{kind}-holes-{name}-rejected"))?;
        negatives.push(json!({"name":name,"world_point":point,"projected_point":screen,"input":input,"surface":surface}));
    }
    let view = ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    picking(c)?;
    let state = inspect(c)?;
    let (point, screen) = wall_point(&solid, &state, &view["value"]["camera"], key)?;
    let mut inputs = Vec::new();
    let mut selected = Value::Null;
    for expected in [1, 0, 1] {
        inputs.push(click(&driver, c, screen)?);
        selected = wait(
            c,
            |state| count(state) == Some(expected),
            "Physical wall click did not toggle the associated hole",
        )?;
        ensure!(
            model(c)? == before,
            "Hole selection wrote model intent before Apply"
        );
    }
    capture(c, out, &format!("geometry-{kind}-holes-picked"))?;
    done(c)?;
    ensure!(model(c)? == before, "Done picking committed the hole draft");
    control(c, "Apply", None)?;
    let after = model(c)?;
    let cam_after = document(c)?;
    let actual = operation(&cam_after)?;
    let mut expected_operation = operation_before;
    expected_operation["holes"] = json!([saved_hole]);
    ensure!(
        *actual == expected_operation,
        "Physical hole Apply changed manual centers or unrelated operation fields: {actual}"
    );
    let mut expected = before.clone();
    expected["cam"]["setups"][0]["operations"][0] = actual.clone();
    ensure!(
        after == expected,
        "Physical hole Apply changed unrelated project data"
    );
    history(c, &before, &after)?;
    let applied = inspect(c)?;
    ensure!(
        count(&applied) == Some(1),
        "Post-Redo hole count differs from saved associations"
    );
    capture(c, out, &format!("geometry-{kind}-holes-applied"))?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == before,
        "Hole fixture Undo lost the manual baseline"
    );
    picking(c)?;
    let cancel_state = inspect(c)?;
    let bounds = canvas(&cancel_state)?;
    let blank = [
        bounds["x"].as_f64().unwrap() + 8.,
        bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap() - 8.,
    ];
    let cancelled=driver.invoke("cam-row-drag",Some(&json!({"client":cancel_state["ui"]["client"],"x":blank[0],"y":blank[1],"points":[{"x":blank[0],"y":blank[1],"hold_ms":0}],"cancel":true}).to_string()))?;
    wait(
        c,
        |state| row(state, "Viewport geometry").is_some_and(|row| row["selected"] == false),
        "Physical Escape did not retire the hole picker",
    )?;
    ensure!(
        model(c)? == before,
        "Cancelled hole picker changed the document"
    );
    control(c, "Redo", None)?;
    ensure!(model(c)? == after, "Cancelled hole picker lost exact Redo");
    generate(c)?;
    let generated = model(c)?;
    ensure!(
        generated != after,
        "Hole generation did not record fresh evidence"
    );
    capture(c, out, &format!("geometry-{kind}-holes-generated"))?;
    let saved = save(c, &out.join(format!("geometry-{kind}-holes-picked.limo")))?;
    history(c, &after, &generated)?;
    control(c, "Undo", None)?;
    ensure!(model(c)? == after, "Generation Undo lost the applied hole");
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == before,
        "Generated hole Undo did not restore the baseline"
    );
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == incoming,
        "Physical hole fixture did not restore its incoming complete model"
    );
    let evidence = json!({"platform":std::env::consts::OS,"owned_pid":pid,"face_key":reference,"world_point":point,"projected_point":screen,
        "inputs":inputs,"negative_picks":negatives,"cancel_input":cancelled,"selected_surface":selected,"applied_surface":applied,
        "before":before,"expected":expected,"after":after,"generated":generated,"generation_exact_undo_redo":true,"archive_model":saved,"draft_only":true,
        "manual_centers_preserved":true,"toggle_identity":true,"one_apply_exact_undo_redo":true,"cancel_preserves_redo":true,"archive_exact":true,"incoming_restored":true});
    fs::write(
        out.join(format!("geometry-{kind}-holes-os-picking.json")),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(evidence)
}
