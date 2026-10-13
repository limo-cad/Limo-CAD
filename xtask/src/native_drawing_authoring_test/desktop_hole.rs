//! Owned-window circle pick and hole-note leader drag.
//! A click in the ring interior is not a circle. The leader follows the label.
use super::super::hole;
use super::chamfer_input::pointer;
use super::*;

fn circle_bounds(state: &Value, label: &str) -> Result<[f64; 4]> {
    let found: Vec<_> = controls(state)
        .filter(|c| c["label"] == label && c["disabled"] == false)
        .collect();
    ensure!(
        found.len() == 1,
        "Expected one hole circle target {label}: {found:?}"
    );
    let bounds = &found[0]["bounds"];
    Ok([
        bounds["x"].as_f64().context("Circle x")?,
        bounds["y"].as_f64().context("Circle y")?,
        bounds["width"].as_f64().context("Circle width")?,
        bounds["height"].as_f64().context("Circle height")?,
    ])
}

fn interior_sample(bounds: [f64; 4]) -> [f64; 2] {
    [bounds[0] + bounds[2] * 0.5, bounds[1] + bounds[3] * 0.5]
}

fn ring_sample(bounds: [f64; 4]) -> [f64; 2] {
    [bounds[0] + bounds[2] - 1., bounds[1] + bounds[3] * 0.5]
}

fn pick_tolerance_mm(scale: f64) -> f64 {
    2_f64.max(3. / scale)
}

fn perimeter_gap_mm(center: [f64; 2], radius_mm: f64, point: [f64; 2], scale: f64) -> f64 {
    (((point[0] - center[0]) / scale).hypot((point[1] - center[1]) / scale) - radius_mm).abs()
}

fn require_distinct_ring(bounds: [f64; 4], scale: f64) -> Result<()> {
    ensure!(
        scale.is_finite()
            && scale > 0.
            && (bounds[2] - bounds[3]).abs() < 0.01
            && bounds[2] > 4. * scale,
        "Hole pointer fixture needs a complete unclipped ring larger than the pick tolerance"
    );
    let radius_mm = bounds[2] * 0.5 / scale;
    let center = interior_sample(bounds);
    let tolerance = pick_tolerance_mm(scale);
    let ring = ring_sample(bounds);
    ensure!(
        perimeter_gap_mm(center, radius_mm, ring, scale) <= 1. / scale + 1e-9,
        "Ring sample is more than one logical pixel off the circumference"
    );
    ensure!(
        perimeter_gap_mm(center, radius_mm, center, scale) > tolerance,
        "Circle center is inside the hole pick tolerance"
    );
    Ok(())
}

/// Saved label position after one pointer delta. Pinned to `Draft::move_hole`
/// in `drawing_authoring/draft/hole.rs`: replace from the saved anchor, then
/// clamp each axis to `5. ..= sheet - 5.`.
fn leader_position(original: [f64; 2], delta: [f64; 2], sheet: [f64; 2]) -> Result<[f64; 2]> {
    if delta.iter().any(|v| !v.is_finite()) || sheet.iter().any(|v| !v.is_finite() || *v < 10.) {
        anyhow::bail!("Invalid hole note paper position");
    }
    Ok(std::array::from_fn(|i| {
        (original[i] + delta[i]).clamp(5., sheet[i] - 5.)
    }))
}

fn paper_delta(from: [f64; 2], to: [f64; 2], scale: f64) -> Result<[f64; 2]> {
    ensure!(scale.is_finite() && scale > 0., "Invalid paper scale");
    Ok([(to[0] - from[0]) / scale, (to[1] - from[1]) / scale])
}

fn sheet_mm(model: &Value) -> Result<[f64; 2]> {
    let active = &model["drawings"]["active_sheet_id"];
    let sheet = model["drawings"]["sheets"]
        .as_array()
        .context("Sheets")?
        .iter()
        .find(|s| &s["id"] == active)
        .context("Active sheet")?;
    ensure!(
        sheet["format"] == "a4" && sheet["orientation"] == "landscape",
        "Hole pointer drag expects the fixture's landscape A4 sheet"
    );
    Ok([297., 210.])
}

pub(in super::super) fn exercise(
    c: &mut Client,
    out: &Path,
    server: &str,
    baseline: &Value,
    projection: &Value,
    definition: &Value,
) -> Result<Value> {
    ensure!(
        cfg!(target_os = "linux"),
        "Hole OS proof requires disposable private Linux Xvfb"
    );
    let initial = inspect(c)?;
    let session = initial["active_session_id"]
        .as_str()
        .context("Owned hole session")?;
    let driver = Driver::new(owned_pid(out, session, server)?, out)?;
    control(c, "Fit sheet", None)?;
    let paper = Paper::fitted(&inspect(c)?)?;
    hole::open(c)?;
    let state = inspect(c)?;
    let label = hole::entry_label(projection, &state)?;
    let bounds = circle_bounds(&state, &label)?;
    require_distinct_ring(bounds, paper.scale)?;
    capture(c, out, "hole-os-targets")?;
    gesture(
        &driver,
        c,
        out,
        "hole-os-empty-center",
        "drawing-click",
        interior_sample(bounds),
        None,
    )?;
    thread::sleep(Duration::from_millis(180));
    ensure!(
        &observed_model(c)? == baseline,
        "Circle-center click created a hole note"
    );
    ensure!(
        controls(&inspect(c)?)
            .any(|row| row["label"] == label && row["surface"] == "drawing/circles"),
        "Interior click retired the hole circle targets"
    );
    let ring = ring_sample(bounds);
    let picked = gesture(&driver, c, out, "hole-os-ring", "drawing-click", ring, None)?;
    let observed = observed_point(&picked, "logical_start")?;
    let radius_mm = bounds[2] * 0.5 / paper.scale;
    let gap = perimeter_gap_mm(interior_sample(bounds), radius_mm, observed, paper.scale);
    ensure!(
        gap <= 1.5 / paper.scale,
        "Physical circle pick was not on the drilled circumference: gap {gap} mm, radius {radius_mm} mm"
    );
    let created = changed_model(c, baseline, out, "hole-os-ring")?;
    let note = exact_one_added(baseline, &created, out, "hole-os-created")?;
    curved::circular_ref(projection, &note["feature"])?;
    ensure!(
        note["kind"] == "hole_note"
            && note["quantity"] == 2
            && note["diameter"] == 6.
            && note["depth"].is_null()
            && note["through_all"] == true
            && note["note"] == "THRU"
            && note["source_feature_id"] == definition["feature_id"]
            && note["feature_name"] == definition["name"]
            && note["pattern_note"] == "2 HOLES"
            && note["hole_style"] == "simple",
        "Pointer hole pick lost exact canonical metadata: {note}"
    );
    let feature_center = &note["feature"]["fallback_center"];
    ensure!(
        (feature_center[0].as_f64().context("Hole X")? - 20.).abs() < 1e-6
            && (feature_center[1].as_f64().context("Hole Y")? - 20.).abs() < 1e-6
            && (feature_center[2].as_f64().context("Hole Z")? - 10.).abs() < 1e-6,
        "Pointer pick selected a different or bottom circle"
    );
    let id = note["id"].as_u64().context("Hole note ID")?;
    history(c, baseline, &created)?;
    capture(c, out, "hole-os-created")?;
    let start = center(&inspect(c)?, &format!("Edit annotation {id}"))?;
    let end = [start[0] + paper.scale * 7., start[1] + paper.scale * 6.];
    pointer(&driver, c, out, "hole-os-drag-cancel", start, end, true)?;
    ensure!(
        observed_model(c)? == created,
        "Escape committed a partial hole-leader drag"
    );
    let start = center(&inspect(c)?, &format!("Edit annotation {id}"))?;
    let sheet = sheet_mm(&created)?;
    let end = [start[0] + paper.scale * (sheet[0] + 40.), start[1]];
    let moved = pointer(&driver, c, out, "hole-os-drag", start, end, false)?;
    let from = observed_point(&moved, "logical_start")?;
    let to = observed_point(&moved, "logical_end")?;
    let expected = leader_position(
        [
            note["position"][0].as_f64().context("Saved hole X")?,
            note["position"][1].as_f64().context("Saved hole Y")?,
        ],
        paper_delta(from, to, paper.scale)?,
        sheet,
    )?;
    ensure!(
        (expected[0] - (sheet[0] - 5.)).abs() < 1e-6,
        "Hole leader drag did not reach the sheet clamp: {expected:?}"
    );
    let dragged = changed_model(c, &created, out, "hole-os-drag")?;
    let row = annotations(&dragged)?
        .iter()
        .find(|r| r["id"] == id)
        .context("Dragged hole note")?;
    close_point(&row["position"], expected, "Hole leader drag")?;
    ensure!(
        row["position"] != note["position"],
        "Pointer drag did not move the hole leader"
    );
    ensure!(
        dragged == replace_expected(&created, id, |a| a["position"] = row["position"].clone()),
        "Hole leader drag changed the circle, callout, or unrelated model intent"
    );
    history(c, &created, &dragged)?;
    capture(c, out, "hole-os-dragged")?;
    curved::save_exact(c, out, "hole-os-dragged", &dragged)?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == created,
        "Leader-drag Undo lost the pointer-created hole note"
    );
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == baseline,
        "Hole pointer gestures did not restore the complete baseline"
    );
    Ok(json!({
        "source": "Private Xvfb X11 XTEST on exact fixture PID",
        "actual_circumference_click": true,
        "center_click_does_not_create": true,
        "actual_leader_drag": true,
        "escape_cancels_leader_drag": true,
        "observed_logical_delta_matches_clamped_paper_position": true,
        "front_entry_circle": [20., 20., 10.],
        "exact_model_history_archive": true,
        "captures": ["hole-os-targets.png", "hole-os-created.png", "hole-os-dragged.png"]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hole_pointer_leader_position_follows_delta_and_clamp() {
        assert_eq!(
            leader_position([40., 30.], [7., -6.], [297., 210.]).unwrap(),
            [47., 24.]
        );
        assert_eq!(
            leader_position([100., 40.], [1000., -1000.], [297., 210.]).unwrap(),
            [292., 5.]
        );
        assert_eq!(paper_delta([10., 20.], [24., 8.], 2.).unwrap(), [7., -6.]);
        assert!(leader_position([10., 10.], [f64::NAN, 0.], [297., 210.]).is_err());
        assert!(leader_position([10., 10.], [1., 1.], [9., 210.]).is_err());
        assert!(paper_delta([0., 0.], [1., 1.], 0.).is_err());
    }

    #[test]
    fn hole_pointer_circumference_is_a_pick_and_center_is_not() {
        let bounds = [100., 80., 48., 48.];
        let scale = 4.;
        let center = interior_sample(bounds);
        let radius_mm = bounds[2] * 0.5 / scale;
        let tolerance = pick_tolerance_mm(scale);
        assert!(perimeter_gap_mm(center, radius_mm, ring_sample(bounds), scale) <= tolerance);
        assert!(perimeter_gap_mm(center, radius_mm, center, scale) > tolerance);
        require_distinct_ring(bounds, scale).unwrap();
        assert!(require_distinct_ring([100., 80., 8., 8.], scale).is_err());
    }
}
