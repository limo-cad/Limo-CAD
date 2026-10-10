//! Optional physical pointer proof with the desktop paper fixtures' owned PID,
//! guarded Rust MCP on Windows, or private Linux XTEST transport.
use super::*;
use crate::native_platform_test::Driver;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

pub(super) fn inspect(c: &mut Client) -> Result<Value> {
    ui(c, json!({"action":"inspect"}))
}
pub(super) fn wait(
    c: &mut Client,
    predicate: impl Fn(&Value) -> bool,
    message: &str,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = inspect(c)?;
        if predicate(&state) {
            return Ok(state);
        }
        ensure!(Instant::now() < deadline, "{message}: {state}");
        thread::sleep(Duration::from_millis(30));
    }
}
pub(super) fn row<'a>(state: &'a Value, label: &str) -> Option<&'a Value> {
    controls(state).find(|row| row["surface"] == "cam/document" && row["label"] == label)
}
pub(super) fn contains(bounds: &Value, point: [f64; 2]) -> bool {
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
    point[0] >= x && point[0] < x + w && point[1] >= y && point[1] < y + h
}
pub(super) fn click(driver: &Driver, c: &mut Client, point: [f64; 2]) -> Result<Value> {
    let state = inspect(c)?;
    ensure!(
        !controls(&state).any(|control| contains(&control["bounds"], point)),
        "Projected CAM edge is covered by a control"
    );
    let raw = driver.invoke(
        "cam-row-drag",
        Some(
            &json!({"x":point[0],"y":point[1],"client":state["ui"]["client"],
        "points":[{"x":point[0],"y":point[1],"hold_ms":0}],"cancel":false})
            .to_string(),
        ),
    )?;
    serde_json::from_str(&raw).context("OS picking omitted a valid input receipt")
}
pub(super) fn exercise(c: &mut Client, out: &Path, server: &str, kind: &str) -> Result<Value> {
    ensure!(
        cfg!(target_os = "windows") || cfg!(target_os = "linux"),
        "CAM OS picking proof supports owned Windows or private Linux Xvfb"
    );
    let initial = inspect(c)?;
    let session = initial["active_session_id"]
        .as_str()
        .context("CAM document session missing")?;
    let pid = crate::native_drawing_navigation_test::owned_pid(out, session, server)?;
    let driver = Driver::new(pid, out)?;
    driver.event("focus")?;
    let before = model(c)?;
    let cam_before = document(c)?;
    let operation_before = operation(&cam_before)?.clone();
    section(c, "geometry")?;
    field(c, "Geometry source", "model")?;
    field(c, "Edge selection", "closed")?;
    field(c, "Selected edge count", "0")?;
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
        "Geometry picker did not become ready",
    )?;
    let view = ui(
        c,
        json!({"action":"view","view":"top","fit":true,"duration_ms":0}),
    )?;
    let state = inspect(c)?;
    let canvas = state["ui"]["canvases"]
        .as_array()
        .context("Native canvases")?
        .iter()
        .find(|canvas| canvas["name"] == "viewport")
        .context("Native model canvas")?;
    let scene = scene(c)?;
    let body = scene["bodies"]
        .as_array()
        .context("Solid bodies")?
        .first()
        .context("Real solid missing")?;
    let circle = kind == "contour2d";
    let mut points = Vec::new();
    for edge in body["edges"].as_array().context("Real solid edges")? {
        if edge["circle"]["closed"].as_bool().unwrap_or(false) != circle {
            continue;
        }
        let samples = edge["points"].as_array().context("Edge sample points")?;
        if !samples
            .iter()
            .all(|point| point["z"].as_f64().is_some_and(|z| (z - 8.).abs() < 1e-5))
        {
            continue;
        }
        for pair in samples.windows(2) {
            let point = ["x", "y", "z"].map(|axis| {
                (pair[0][axis].as_f64().unwrap() + pair[1][axis].as_f64().unwrap()) * 0.5
            });
            let screen = crate::native_move_test::project(&view["value"]["camera"], canvas, point);
            if contains(canvas, screen)
                && !controls(&state).any(|control| contains(&control["bounds"], screen))
            {
                points.push((
                    screen,
                    format!(
                        "edge:{}:{}",
                        body["id"],
                        edge["key"].as_str().context("Edge identity")?
                    ),
                ));
            }
        }
    }
    points.sort_by(|a, b| b.0[1].total_cmp(&a.0[1]));
    let (point, key) = points
        .first()
        .context("No unobscured projected edge midpoint")?;
    let input = click(&driver, c, *point)?;
    let selected = wait(
        c,
        |state| {
            row(state, "Selected edge count")
                .and_then(|row| row["value"].as_str())
                .and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|count| count == if circle { 1 } else { 4 })
        },
        "Physical edge click did not stage its complete closed chain",
    )?;
    ensure!(
        model(c)? == before,
        "Physical geometry pick committed before Apply"
    );
    capture(c, out, &format!("geometry-{kind}-os-picked"))?;
    control(c, "Viewport geometry", None)?;
    ensure!(model(c)? == before, "Done picking changed model/history");
    control(c, "Apply", None)?;
    let after = model(c)?;
    let cam_after = document(c)?;
    let actual = operation(&cam_after)?;
    ensure!(
        actual["chain_ref"]["source"] == "model"
            && actual["chain_ref"]["keys"]
                .as_array()
                .is_some_and(|keys| keys.iter().any(|value| value == key)),
        "Physical pick lost stable model-edge provenance"
    );
    let mut expected_operation = operation_before;
    for field in [
        "chain_ref",
        if kind == "pocket2d" {
            "outline"
        } else {
            "path"
        },
        "closed",
    ] {
        if !actual[field].is_null() {
            expected_operation[field] = actual[field].clone();
        }
    }
    ensure!(
        *actual == expected_operation && actual != operation(&cam_before)?,
        "Viewport picking changed unrelated operation fields or did not change geometry"
    );
    let mut expected = before.clone();
    expected["cam"]["setups"][0]["operations"][0] = actual.clone();
    ensure!(
        after == expected,
        "Geometry Apply changed unrelated project data"
    );
    history(c, &before, &after)?;
    let applied = inspect(c)?;
    let applied_key_count = actual["chain_ref"]["keys"]
        .as_array()
        .context("Applied stable model-edge keys")?
        .len();
    fs::write(
        out.join(format!("geometry-{kind}-os-applied-inspect.json")),
        serde_json::to_vec_pretty(&json!({
            "expected_selected_edge_count": applied_key_count,
            "saved_chain_ref": actual["chain_ref"],
            "surface": applied,
        }))?,
    )?;
    ensure!(
        row(&applied, "Selected edge count")
            .and_then(|row| row["value"].as_str())
            .and_then(|value| value.parse::<usize>().ok())
            == Some(applied_key_count),
        "Post-Redo retained edge count differs from saved chain: expected {applied_key_count}, control {:?}",
        row(&applied, "Selected edge count")
    );
    capture(c, out, &format!("geometry-{kind}-os-applied"))?;
    save(c, &out.join(format!("geometry-{kind}-os-picked.limo")))?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == before && document(c)? == cam_before,
        "OS picker fixture did not restore its complete incoming model"
    );
    section(c, "geometry")?;
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
        "Reopened picker did not become ready",
    )?;
    let cancel_state = inspect(c)?;
    let blank = [
        canvas["x"].as_f64().unwrap() + 8.,
        canvas["y"].as_f64().unwrap() + canvas["height"].as_f64().unwrap() - 8.,
    ];
    ensure!(
        !controls(&cancel_state).any(|control| contains(&control["bounds"], blank)),
        "Cancellation point is covered by a control"
    );
    let cancelled = driver.invoke("cam-row-drag", Some(&json!({"client":cancel_state["ui"]["client"],"x":blank[0],"y":blank[1],"points":[{"x":blank[0],"y":blank[1],"hold_ms":0}],"cancel":true}).to_string()))?;
    wait(
        c,
        |state| row(state, "Viewport geometry").is_some_and(|row| row["selected"] == false),
        "Physical Escape did not end picking",
    )?;
    ensure!(model(c)? == before, "Cancelled picker changed model intent");
    control(c, "Redo", None)?;
    ensure!(
        model(c)? == after,
        "Cancelled picker added history or lost the pending Redo"
    );
    control(c, "Undo", None)?;
    ensure!(model(c)? == before, "Exact fixture restore failed");
    let evidence = json!({"platform":std::env::consts::OS,"input":input,"cancel_input":cancelled,
        "projected_point":point,"edge_key":key,"selected_surface":selected,"applied_surface":applied,
        "draft_only":true,"one_apply_exact_undo_redo":true,"cancel_preserves_redo":true,"archive_exact":true});
    fs::write(
        out.join(format!("geometry-{kind}-os-picking.json")),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(evidence)
}
