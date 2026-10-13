//! Explicit opt-in real input on one verified fixture-owned window.
use super::super::chamfer;
use super::*;

pub(super) fn pointer(
    driver: &Driver,
    c: &mut Client,
    out: &Path,
    stage: &str,
    start: [f64; 2],
    end: [f64; 2],
    cancel: bool,
) -> Result<Value> {
    let state = inspect(c)?;
    let request = json!({"client":state["ui"]["client"],"x":start[0],"y":start[1],"points":[{"x":end[0],"y":end[1],"hold_ms":0}],"cancel":cancel});
    fs::write(
        out.join(format!("{stage}-surface.json")),
        serde_json::to_vec_pretty(&state)?,
    )?;
    fs::write(
        out.join(format!("{stage}-request.json")),
        serde_json::to_vec_pretty(&request)?,
    )?;
    let evidence: Value =
        serde_json::from_str(&driver.invoke("cam-row-drag", Some(&request.to_string()))?)?;
    fs::write(
        out.join(format!("{stage}-input.json")),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(evidence)
}
pub(in super::super) fn exercise(
    c: &mut Client,
    out: &Path,
    server: &str,
    baseline: &Value,
    projection: &Value,
) -> Result<Value> {
    ensure!(
        cfg!(target_os = "windows") || cfg!(target_os = "linux"),
        "Chamfer OS proof requires owned Windows or private Linux Xvfb"
    );
    let initial = inspect(c)?;
    let session = initial["active_session_id"]
        .as_str()
        .context("Owned chamfer session")?;
    let driver = Driver::new(owned_pid(out, session, server)?, out)?;
    control(c, "Fit sheet", None)?;
    let paper = Paper::fitted(&inspect(c)?)?;
    let label = chamfer::pick(c)?;
    let point = center(&inspect(c)?, &label)?;
    pointer(
        &driver,
        c,
        out,
        "chamfer-os-pick-cancel",
        point,
        point,
        true,
    )?;
    let state = inspect(c)?;
    ensure!(
        !controls(&state).any(|r| r["label"] == "Place note"),
        "OS Escape did not retire staged chamfer choice"
    );
    ensure!(
        &observed_model(c)? == baseline,
        "Canceled OS chamfer pick changed document/counter"
    );
    let label = chamfer::pick(c)?;
    let point = center(&inspect(c)?, &label)?;
    pointer(&driver, c, out, "chamfer-os-pick", point, point, false)?;
    ensure!(
        &observed_model(c)? == baseline,
        "OS chamfer candidate click placed before the paper click"
    );
    capture(c, out, "chamfer-os-staged")?;
    let place = paper.screen([213., 75.]);
    let placed = pointer(&driver, c, out, "chamfer-os-place", place, place, false)?;
    let created = changed_model(c, baseline, out, "chamfer-os-place")?;
    let a = exact_one_added(baseline, &created, out, "chamfer-os-created")?;
    chamfer::check_note(projection, &a)?;
    close_point(
        &a["position"],
        paper.paper(observed_point(&placed, "logical_start")?),
        "Chamfer placement",
    )?;
    let id = a["id"].as_u64().unwrap();
    history(c, baseline, &created)?;
    capture(c, out, "chamfer-os-created")?;
    let start = center(&inspect(c)?, &format!("Edit annotation {id}"))?;
    let end = [start[0] + paper.scale * 7., start[1] + paper.scale * 6.];
    pointer(&driver, c, out, "chamfer-os-drag-cancel", start, end, true)?;
    ensure!(
        observed_model(c)? == created,
        "Escape committed a partial chamfer drag"
    );
    let start = center(&inspect(c)?, &format!("Edit annotation {id}"))?;
    let end = [start[0] + paper.scale * 7., start[1] + paper.scale * 6.];
    let moved = pointer(&driver, c, out, "chamfer-os-drag", start, end, false)?;
    let from = observed_point(&moved, "logical_start")?;
    let to = observed_point(&moved, "logical_end")?;
    let expected_position = std::array::from_fn(|i| {
        a["position"][i].as_f64().unwrap() + (to[i] - from[i]) / paper.scale
    });
    let dragged = changed_model(c, &created, out, "chamfer-os-drag")?;
    let row = annotations(&dragged)?
        .iter()
        .find(|r| r["id"] == id)
        .context("Dragged chamfer record")?;
    close_point(
        &row["position"],
        expected_position,
        "Chamfer cumulative drag",
    )?;
    ensure!(
        dragged == replace_expected(&created, id, |a| a["position"] = row["position"].clone()),
        "OS chamfer drag altered references, measurements, or other intent"
    );
    history(c, &created, &dragged)?;
    capture(c, out, "chamfer-os-dragged")?;
    curved::save_exact(c, out, "chamfer-os-dragged", &dragged)?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == created,
        "OS drag Undo lost original annotation"
    );
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == baseline,
        "OS creation Undo did not restore complete baseline"
    );
    Ok(
        json!({"actual_os_pick_then_paper_placement":true,"actual_os_label_drag":true,"escape_cancels_pick_and_drag":true,
        "observed_logical_delta_matches_paper_position":true,"exact_model_history_archive":true,
        "captures":["chamfer-os-staged.png","chamfer-os-created.png","chamfer-os-dragged.png"]}),
    )
}
