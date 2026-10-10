//! The real Windows print panel is observed and cancelled without submitting.
//! No product-only test hook or alternate drawing/print implementation is used.
use super::*;
use crate::native_fixture::begin_sketch;

pub(super) fn guard() -> Result<()> {
    ensure!(
        hosted::enabled(
            std::env::consts::OS,
            "windows",
            "Windows",
            ("LIMO_CAD_NATIVE_PRINT_TEST", "windows-cancel"),
            |key| std::env::var(key).ok(),
        ),
        "Print cancellation requires the explicitly opted-in disposable GitHub Windows runner"
    );
    Ok(())
}

fn retain(out: &Path, name: &str, value: &Value) -> Result<()> {
    fs::write(
        out.join(format!("{name}.json")),
        serde_json::to_vec_pretty(value)?,
    )?;
    Ok(())
}

fn status(client: &mut Client) -> Result<Value> {
    Ok(ui(client, json!({"action":"file","command":"print_status"}))?["value"]["printing"].clone())
}

fn wait_status(client: &mut Client, expected: &str) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let current = status(client)?;
        if current["state"] == expected {
            return Ok(current);
        }
        ensure!(
            current["state"] == "preparing" || current["state"] == "awaiting_print_dialog",
            "Print unexpectedly completed before {expected}: {current}"
        );
        ensure!(
            Instant::now() < deadline,
            "Print never reached {expected}: {current}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

pub(super) fn exercise(
    client: &mut Client,
    driver: &Driver,
    out: &Path,
    session: &str,
) -> Result<Value> {
    guard()?;
    begin_sketch(client, "XY")?;
    client.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},
        "p2":{"x":40.,"y":25.},"ctrl_held":true}),
    )?;
    control(client, "Finish sketch", None)?;
    client.call(
        "solid_extrude",
        json!({"sketch_name":"Sketch1","profile_indices":[0],
        "extent":{"type":"distance","distance":6.}}),
    )?;
    let scene = client.call("solid_scene", json!({}))?;
    ensure!(
        scene["bodies"].as_array().is_some_and(|b| b.len() == 1),
        "Real print solid is missing"
    );
    control(client, "Switch workspace", None)?;
    control(client, "Drawing", None)?;
    control(client, "New Sheet", None)?;
    let sheet = client.call("drawing_document", json!({}))?["active_sheet_id"]
        .as_u64()
        .context("Print sheet missing")?;
    client.call(
        "drawing_add_view",
        json!({"sheet_id":sheet,"view":{
        "name":"Print cancellation proof","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],
        "position":[100.,100.],"scale":1.,"show_hidden_lines":true}}),
    )?;
    let projected = client.call(
        "drawing_projection",
        json!({"direction":[0.,-1.,0.],"up":[0.,0.,1.],"include_hidden":true}),
    )?;
    ensure!(
        projected["visible"]
            .as_array()
            .is_some_and(|edges| !edges.is_empty()),
        "Print sheet has no real projected edges"
    );
    let before = client.call("cad_project_model", json!({}))?;
    retain(out, "project-before", &before)?;
    retain(out, "projection", &projected)?;
    capture(client, out, "real-sheet-before-print")?;
    let mut attempts = Vec::new();
    for attempt in 1..=2 {
        control(client, "File", None)?;
        control(client, "Print / Save as PDF", None)?;
        let pending = wait_status(client, "awaiting_print_dialog")?;
        ensure!(
            pending["sheet_id"] == sheet,
            "Print dialog has the wrong sheet: {pending}"
        );
        retain(out, &format!("print-{attempt}-pending"), &pending)?;
        let observed: Value = serde_json::from_str(&driver.invoke("print-cancel", None)?)?;
        retain(out, &format!("print-{attempt}-os-dialog"), &observed)?;
        ensure!(
            observed["dialog_closed"] == true && observed["action"] == "cancel",
            "OS print panel did not close by Cancel: {observed}"
        );
        let cancelled = wait_status(client, "cancelled")?;
        retain(out, &format!("print-{attempt}-completed"), &cancelled)?;
        let after = client.call("cad_project_model", json!({}))?;
        retain(out, &format!("project-after-{attempt}"), &after)?;
        ensure!(
            after == before,
            "Print cancellation changed the exact authored project"
        );
        let inspected = ui(client, json!({"action":"inspect"}))?;
        ensure!(
            inspected["active_session_id"] == session,
            "Print cancellation switched the owned document"
        );
        capture(client, out, &format!("real-sheet-after-cancel-{attempt}"))?;
        attempts.push(
            json!({"os_dialog":observed,"completion":cancelled,"exact_project_unchanged":true}),
        );
    }
    Ok(
        json!({"status":"passed","platform":"windows","owned_pid":driver.pid,"session_id":session,
        "source_sha":std::env::var("GITHUB_SHA").ok(),"run_id":std::env::var("GITHUB_RUN_ID").ok(),
        "sheet_id":sheet,"attempts":attempts,"submission_requested":false,
        "coverage":"Real solid and sheet, native File print action, owned OS dialog, Cancel, exact project retention, second-open cleanup",
        "limitations":["Windows cancellation does not submit printer output or a PDF","OS dialog pixels are not captured by Bevy window capture","macOS NSPrintOperation and the Linux print portal are not driven by this Windows cancellation; both write temp/Limo-CAD-print-*/sheet.pdf when no printer is installed"]}),
    )
}
