//! Real OS clicks and drags; semantic commands prepare tools and verify state.
use super::super::curved::{angular_triple, circular_ref, endpoint_ref, projection};
use super::*;
use anyhow::bail;

fn drag_and_restore(
    driver: &Driver,
    c: &mut Client,
    out: &Path,
    baseline: &Value,
    created: &Value,
    annotation: &Value,
    paper: &Paper,
) -> Result<()> {
    let stage = match annotation["kind"].as_str() {
        Some("radial_dimension") => "radial",
        Some("angular_dimension") => "angular",
        _ => bail!("Expected a radial or angular dimension"),
    };
    let id = annotation["id"]
        .as_u64()
        .context("Created curved dimension ID")?;
    history(c, baseline, created)?;
    capture(c, out, &format!("author-os-{stage}-created"))?;
    let state = inspect(c)?;
    let start = center(&state, &format!("Edit annotation {id}"))?;
    let moved = gesture(
        driver,
        c,
        out,
        &format!("author-os-{stage}-drag"),
        "drawing-drag",
        start,
        Some([start[0] + paper.scale * 7., start[1] + paper.scale * 6.]),
    )?;
    let from = observed_point(&moved, "logical_start")?;
    let to = observed_point(&moved, "logical_end")?;
    let delta = [
        (to[0] - from[0]) / paper.scale,
        (to[1] - from[1]) / paper.scale,
    ];
    let dragged = changed_model(c, created, out, &format!("author-os-{stage}-drag"))?;
    let row = annotations(&dragged)?
        .iter()
        .find(|a| a["id"] == id)
        .context("Dragged curved dimension")?;
    let expected = replace_expected(created, id, |a| {
        if stage == "radial" {
            a["leader_angle_deg"] = row["leader_angle_deg"].clone();
            a["offset"] = row["offset"].clone();
        } else {
            a["radius"] = row["radius"].clone();
        }
    });
    ensure!(
        dragged == expected,
        "Actual OS {stage} drag changed topology, presentation or unrelated model intent"
    );
    let view = created["drawings"]["sheets"]
        .as_array()
        .context("Saved sheets")?
        .iter()
        .flat_map(|s| s["views"].as_array().into_iter().flatten())
        .find(|v| v["id"] == annotation["view_id"])
        .context("Dimension view")?;
    if stage == "radial" {
        let paper_radius = annotation["feature"]["fallback_radius"]
            .as_f64()
            .context("Actual radius")?
            * view["scale"].as_f64().context("View scale")?;
        let radius = paper_radius + annotation["offset"].as_f64().unwrap();
        let angle = annotation["leader_angle_deg"]
            .as_f64()
            .unwrap()
            .to_radians();
        let vector = [
            radius * angle.cos() + delta[0],
            radius * angle.sin() + delta[1],
        ];
        let expected_angle = vector[1].atan2(vector[0]).to_degrees();
        let expected_offset = (vector[0].hypot(vector[1]) - paper_radius).max(2.);
        ensure!(
            (row["leader_angle_deg"].as_f64().unwrap() - expected_angle).abs() < 1e-5
                && (row["offset"].as_f64().unwrap() - expected_offset).abs() < 1e-5,
            "OS radial drag disagrees with actual logical pixel delta at this DPI"
        );
        ensure!(
            row["leader_angle_deg"] != annotation["leader_angle_deg"]
                && row["offset"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && v >= 2.),
            "Actual radial drag did not move its leader"
        );
    } else {
        ensure!(
            view["direction"] == json!([0., 0., 1.])
                && view["up"] == json!([0., 1., 0.])
                && view["derivation"].is_null(),
            "Physical angular fixture requires its unchanged Top basis"
        );
        let vertex = &annotation["vertex"]["fallback_point"];
        let ray = |key: &str| -> [f64; 2] {
            let p = &annotation[key]["fallback_point"];
            let v = [
                p[0].as_f64().unwrap() - vertex[0].as_f64().unwrap(),
                vertex[1].as_f64().unwrap() - p[1].as_f64().unwrap(),
            ];
            let length = v[0].hypot(v[1]);
            [v[0] / length, v[1] / length]
        };
        let a = ray("first");
        let b = ray("second");
        let bisector = [a[0] + b[0], a[1] + b[1]];
        let length = bisector[0].hypot(bisector[1]);
        ensure!(
            length > 0.5,
            "Expected the fixture's nondegenerate angular bisector"
        );
        let reach = annotation["radius"].as_f64().unwrap() + 4.;
        let vector = [
            bisector[0] / length * reach + delta[0],
            bisector[1] / length * reach + delta[1],
        ];
        let expected_radius = (vector[0].hypot(vector[1]) - 4.).max(2.);
        ensure!(
            (row["radius"].as_f64().unwrap() - expected_radius).abs() < 1e-5,
            "OS angular drag disagrees with actual logical pixel delta at this DPI"
        );
        ensure!(
            row["radius"] != annotation["radius"]
                && row["radius"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && v >= 2.),
            "Actual angular drag did not move its arc"
        );
    }
    history(c, created, &dragged)?;
    capture(c, out, &format!("author-os-{stage}-dragged"))?;
    control(c, "Undo", None)?;
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == baseline,
        "Actual OS {stage} gestures did not restore exact model and counters"
    );
    Ok(())
}

pub(super) fn exercise(
    driver: &Driver,
    c: &mut Client,
    out: &Path,
    baseline: &Value,
    paper: &Paper,
) -> Result<Value> {
    let projected = projection(c)?;
    control(c, "Radius", None)?;
    let state = inspect(c)?;
    let target = controls(&state)
        .find(|a| a["surface"] == "drawing/circles" && a["disabled"] == false)
        .context("Actual circular paper target")?;
    let bounds = &target["bounds"];
    let x = bounds["x"].as_f64().context("Circle x")?;
    let y = bounds["y"].as_f64().context("Circle y")?;
    let width = bounds["width"].as_f64().context("Circle width")?;
    let height = bounds["height"].as_f64().context("Circle height")?;
    ensure!(
        (width - height).abs() < 0.01 && width > 4. * paper.scale,
        "Fixture requires a complete unclipped ring larger than picking tolerance"
    );
    gesture(
        driver,
        c,
        out,
        "author-os-radial-empty-center",
        "drawing-click",
        [x + width * 0.5, y + height * 0.5],
        None,
    )?;
    thread::sleep(Duration::from_millis(180));
    ensure!(
        &observed_model(c)? == baseline,
        "Empty ring center activated a rectangular circular target"
    );
    gesture(
        driver,
        c,
        out,
        "author-os-radial-ring",
        "drawing-click",
        [x + width - 0.5, y + height * 0.5],
        None,
    )?;
    let created = changed_model(c, baseline, out, "author-os-radial-ring")?;
    let radial = exact_one_added(baseline, &created, out, "author-os-radial-created")?;
    ensure!(
        radial["kind"] == "radial_dimension"
            && radial["mode"] == "radius"
            && radial["offset"] == 14.,
        "OS circumference click did not create the shared radius dimension"
    );
    circular_ref(&projected, &radial["feature"])?;
    ensure!(
        radial["feature"]["fallback_center"][2] == 10.,
        "Coincident OS circle hit did not select the real frontmost cylinder edge"
    );
    drag_and_restore(driver, c, out, baseline, &created, &radial, paper)?;

    control(c, "Angle", None)?;
    let labels = angular_triple(c)?;
    for (index, label) in labels.iter().enumerate() {
        let state = inspect(c)?;
        let point = center(&state, label)?;
        gesture(
            driver,
            c,
            out,
            &format!("author-os-angular-pick-{index}"),
            "drawing-click",
            point,
            None,
        )?;
        if index < 2 {
            ensure!(
                &observed_model(c)? == baseline,
                "Incomplete actual OS angular picks changed the model"
            );
        }
    }
    let created = changed_model(c, baseline, out, "author-os-angular-picks")?;
    let angular = exact_one_added(baseline, &created, out, "author-os-angular-created")?;
    ensure!(
        angular["kind"] == "angular_dimension" && angular["radius"] == 12.,
        "Actual OS three-anchor picks did not create the shared angular dimension"
    );
    for key in ["vertex", "first", "second"] {
        endpoint_ref(&projected, &angular[key])?;
    }
    drag_and_restore(driver, c, out, baseline, &created, &angular, paper)?;
    Ok(
        json!({"actual_ring_click_passed":true,"empty_rectangle_center_rejected":true,
        "actual_three_anchor_clicks_passed":true,"radial_angular_drag_passed":true,
        "exact_whole_model_history_passed":true,"observed_os_pixel_to_paper_delta_passed":true,
        "captures":["author-os-radial-created.png","author-os-radial-dragged.png",
            "author-os-angular-created.png","author-os-angular-dragged.png"]}),
    )
}
