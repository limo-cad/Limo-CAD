//! Real OS row reorder in the owned host, including a held pointer across the
//! three-row pager. The fixture restores its exact incoming model on success.
use super::*;
use crate::{
    native_fixture::{control_in, inspect_after_gesture},
    native_platform_test::Driver,
};
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Snapshot {
    raw: Value,
    model: Value,
    cam: Value,
}
fn snapshot(c: &mut Client) -> Result<Snapshot> {
    let raw = c.call("cad_project_model", json!({}))?;
    Ok(Snapshot {
        model: serde_json::from_str(raw.as_str().context("Project JSON")?)?,
        raw,
        cam: document(c)?,
    })
}
fn unchanged(c: &mut Client, before: &Snapshot) -> Result<()> {
    ensure!(
        document(c)? == before.cam && c.call("cad_project_model", json!({}))? == before.raw,
        "OS row gesture changed CAM or unrelated project intent"
    );
    Ok(())
}
fn point(state: &Value, label: &str, fraction: f64) -> Result<[f64; 2]> {
    let matches: Vec<_> = controls(state)
        .filter(|r| r["surface"] == "cam/document" && r["label"] == label && r["disabled"] == false)
        .collect();
    ensure!(
        matches.len() == 1,
        "Expected one visible CAM row {label}: {matches:?}"
    );
    let b = &matches[0]["bounds"];
    let x = b["x"].as_f64().context("Row x")?;
    let y = b["y"].as_f64().context("Row y")?;
    let w = b["width"].as_f64().context("Row width")?;
    let h = b["height"].as_f64().context("Row height")?;
    ensure!(w > 16. && h > 16., "CAM row bounds are too small");
    Ok([x + w * 0.5, y + h * fraction])
}
fn first_page(c: &mut Client) -> Result<()> {
    for _ in 0..16 {
        let view = ui(c, json!({"action":"inspect"}))?;
        if !controls(&view).any(|r| {
            r["surface"] == "cam/document" && r["label"] == "Previous" && r["disabled"] == false
        }) {
            return Ok(());
        }
        control_in(c, "cam/document", "Previous", None)?;
    }
    anyhow::bail!("CAM row pager did not reach the first page")
}
fn operation_label(cam: &Value, setup: usize, operation: usize) -> Result<String> {
    Ok(format!(
        "{} / {}",
        cam["setups"][setup]["name"]
            .as_str()
            .context("Setup name")?,
        cam["setups"][setup]["operations"][operation]["name"]
            .as_str()
            .context("Operation name")?
    ))
}
fn inspect(c: &mut Client) -> Result<Value> {
    ui(c, json!({"action":"inspect"}))
}

fn selected(c: &mut Client, label: &str) -> Result<()> {
    let view = inspect(c)?;
    ensure!(
        controls(&view).any(|r| r["surface"] == "cam/document"
            && r["label"] == label
            && r["selected"] == true),
        "OS gesture did not retain the selected CAM identity {label}"
    );
    Ok(())
}

fn gesture(
    driver: &Driver,
    c: &mut Client,
    out: &Path,
    name: &str,
    start: [f64; 2],
    points: &[([f64; 2], u64)],
    cancel: bool,
) -> Result<()> {
    let view = inspect(c)?;
    let request = json!({"client":view["ui"]["client"],"x":start[0],"y":start[1],
        "points":points.iter().map(|(p,hold)| json!({"x":p[0],"y":p[1],"hold_ms":hold})).collect::<Vec<_>>(),"cancel":cancel});
    fs::write(
        out.join(format!("{name}-surface.json")),
        serde_json::to_vec_pretty(&view)?,
    )?;
    fs::write(
        out.join(format!("{name}-request.json")),
        serde_json::to_vec_pretty(&request)?,
    )?;
    let evidence: Value =
        serde_json::from_str(&driver.invoke("cam-row-drag", Some(&request.to_string()))?)?;
    ensure!(
        evidence["operation"] == "cam-row-drag" && evidence["cancel"] == cancel,
        "OS helper did not acknowledge the requested row gesture"
    );
    fs::write(
        out.join(format!("{name}-input.json")),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    inspect_after_gesture(c, out, name, &view["active_session_id"])?;
    Ok(())
}
fn wait_expected(c: &mut Client, expected: &Snapshot) -> Result<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = snapshot(c)?;
        if current.model == expected.model && current.cam == expected.cam {
            return Ok(current);
        }
        ensure!(Instant::now() < deadline,
            "Actual OS row drag did not produce the exact expected list permutation; current CAM={}, expected={}", current.cam, expected.cam);
        thread::sleep(Duration::from_millis(40));
    }
}
fn moved(before: &Snapshot, operation: bool, from: usize, to: usize) -> Snapshot {
    let mut result = before.clone();
    for cam in [&mut result.cam, &mut result.model["cam"]] {
        let rows = if operation {
            &mut cam["setups"][0]["operations"]
        } else {
            &mut cam["setups"]
        };
        let rows = rows.as_array_mut().expect("known fixture list");
        let row = rows.remove(from);
        rows.insert(to, row);
    }
    result
}
fn archive(c: &mut Client, out: &Path, expected: &Snapshot) -> Result<()> {
    let file = out.join("cam-os-row-reordered.limo");
    ensure!(
        !file.exists(),
        "Preserve previous row-drag archive evidence"
    );
    ui(c, json!({"action":"file","command":"save","path":file}))?;
    let mut zip = zip::ZipArchive::new(fs::File::open(&file)?)?;
    let saved: Value = serde_json::from_reader(zip.by_name("model.json")?)?;
    ensure!(
        saved == expected.model,
        "Saved row-drag archive changed keyed intent or model data"
    );
    Ok(())
}

/// Hook in native_cam_test after the generated operation checks and before
/// its existing Duplicate/Delete checks. Caller must prove the GUI PID belongs
/// to its disposable host before constructing Driver; this is an opt-in test.
pub(super) fn exercise(c: &mut Client, out: &Path, driver: &Driver) -> Result<Value> {
    ensure!(
        cfg!(target_os = "windows") || cfg!(target_os = "linux"),
        "OS CAM row proof currently supports Windows and private Linux Xvfb"
    );
    driver.event("focus")?;
    let incoming = snapshot(c)?;
    ensure!(
        incoming.cam["setups"]
            .as_array()
            .is_some_and(|s| s.len() == 1)
            && incoming.cam["setups"][0]["operations"]
                .as_array()
                .is_some_and(|s| s.len() == 1),
        "Run OS row fixture with the existing single-setup/single-operation CAM seed"
    );
    control_in(c, "cam/document", "Toolpaths", None)?;
    first_page(c)?;
    control_in(
        c,
        "cam/document",
        &operation_label(&incoming.cam, 0, 0)?,
        None,
    )?;
    for _ in 0..4 {
        control_in(c, "cam/document", "Duplicate", None)?;
    }
    control_in(c, "cam/document", "Setups", None)?;
    control_in(c, "cam/document", "Duplicate", None)?;
    let seeded = snapshot(c)?;
    ensure!(
        seeded.cam["setups"]
            .as_array()
            .is_some_and(|s| s.len() == 2)
            && seeded.cam["setups"][0]["operations"]
                .as_array()
                .is_some_and(|s| s.len() == 5),
        "OS fixture needs five operations followed by another setup"
    );
    let labels = (0..5)
        .map(|i| operation_label(&seeded.cam, 0, i))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        labels
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == 5,
        "Seeded operation names must be unique for exact UI identity"
    );
    control_in(c, "cam/document", "Toolpaths", None)?;
    first_page(c)?;
    let view = inspect(c)?;
    let start = point(&view, &labels[0], 0.5)?;
    let edge = point(&view, &labels[2], 0.92)?;
    let destination = point(&view, &labels[1], 0.78)?;
    gesture(
        driver,
        c,
        out,
        "cam-os-operation-across-page",
        start,
        &[(edge, 420), (destination, 0)],
        false,
    )?;
    let expected = moved(&seeded, true, 0, 4);
    let reordered = wait_expected(c, &expected)?;
    history(c, &seeded.raw, &reordered.raw)?;
    selected(c, &labels[0])?;
    capture(c, out, "cam-os-operation-reordered")?;

    for (name, kind) in [
        ("cam-os-below-threshold", 0),
        ("cam-os-same-slot", 1),
        ("cam-os-escape", 2),
        ("cam-os-outside-list", 3),
    ] {
        first_page(c)?;
        let view = inspect(c)?;
        let first = operation_label(&reordered.cam, 0, 0)?;
        let third = operation_label(&reordered.cam, 0, 2)?;
        let start = point(&view, &first, 0.5)?;
        let target = point(&view, &third, 0.65)?;
        let (points, cancel) = match kind {
            0 => (vec![([start[0] + 2., start[1]], 0)], false),
            1 => (vec![([start[0] + 12., start[1]], 0), (start, 0)], false),
            2 => (vec![(target, 0)], true),
            _ => {
                let client = &view["ui"]["client"];
                let outside = [
                    client["x"].as_f64().context("Client x")?
                        + client["width"].as_f64().context("Client width")? * 0.75,
                    client["y"].as_f64().context("Client y")?
                        + client["height"].as_f64().context("Client height")? * 0.55,
                ];
                (vec![(outside, 0)], false)
            }
        };
        gesture(driver, c, out, name, start, &points, cancel)?;
        unchanged(c, &reordered)?;
        if kind == 0 {
            selected(c, &first)?;
        }
        history(c, &seeded.raw, &reordered.raw)?;
    }
    first_page(c)?;
    let view = inspect(c)?;
    let start = point(&view, &operation_label(&reordered.cam, 0, 0)?, 0.5)?;
    let edge = point(&view, &operation_label(&reordered.cam, 0, 2)?, 0.92)?;
    let foreign_slot = point(&view, &operation_label(&reordered.cam, 0, 2)?, 0.5)?;
    gesture(
        driver,
        c,
        out,
        "cam-os-cross-setup-rejected",
        start,
        &[(edge, 420), (foreign_slot, 0)],
        false,
    )?;
    unchanged(c, &reordered)?;
    history(c, &seeded.raw, &reordered.raw)?;
    control(c, "Undo", None)?;
    unchanged(c, &seeded)?;

    control_in(c, "cam/document", "Setups", None)?;
    first_page(c)?;
    let view = inspect(c)?;
    let first = seeded.cam["setups"][0]["name"].as_str().unwrap();
    let second = seeded.cam["setups"][1]["name"].as_str().unwrap();
    let start = point(&view, second, 0.5)?;
    let target = point(&view, first, 0.25)?;
    gesture(
        driver,
        c,
        out,
        "cam-os-setup-drag",
        start,
        &[(target, 0)],
        false,
    )?;
    let setups = wait_expected(c, &moved(&seeded, false, 1, 0))?;
    history(c, &seeded.raw, &setups.raw)?;
    selected(c, second)?;
    capture(c, out, "cam-os-setup-reordered")?;
    archive(c, out, &setups)?;
    control(c, "Undo", None)?;
    unchanged(c, &seeded)?;
    for _ in 0..5 {
        control(c, "Undo", None)?;
    }
    unchanged(c, &incoming)?;
    control_in(c, "cam/document", "Toolpaths", None)?;
    first_page(c)?;
    control_in(
        c,
        "cam/document",
        &operation_label(&incoming.cam, 0, 0)?,
        None,
    )?;
    let report = json!({"status":"passed","actual_input":if cfg!(target_os="windows") {"Rust/Enigo MCP OS input"}else{"X11 XTEST in owned Xvfb"},
        "operation_across_three_row_page":true,"setup_drag":true,"below_threshold_click":true,"same_slot_noop":true,
        "escape_cancel":true,"outside_list_cancel":true,"cross_setup_rejected":true,"single_commit_exact_undo_redo":true,
        "exact_model_and_cam_permutations":true,"saved_archive_equal":true,"incoming_model_restored":true,
        "not_proven":["macOS row drag","physical mouse hardware","focus loss mid-drag","monitor DPI transition","pixel correctness without capture review"]});
    fs::write(
        out.join("cam-os-row-reorder.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
