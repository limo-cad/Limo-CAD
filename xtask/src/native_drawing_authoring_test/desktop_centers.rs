//! Genuine XTEST circle-center picks and extension-grip drags in private Xvfb.
use super::super::{centers, curved};
use super::chamfer_input::pointer;
use super::*;

fn view<'a>(model: &'a Value, annotation: &Value) -> Result<&'a Value> {
    let view = model["drawings"]["sheets"]
        .as_array()
        .context("Saved sheets")?
        .iter()
        .filter(|s| s["id"] == model["drawings"]["active_sheet_id"])
        .flat_map(|s| s["views"].as_array().into_iter().flatten())
        .find(|v| v["id"] == annotation["view_id"])
        .context("Created center annotation view")?;
    ensure!(
        view["direction"] == json!([0., 0., 1.])
            && view["up"] == json!([0., 1., 0.])
            && view["derivation"].is_null()
            && view["body_ids"].as_array().is_none_or(Vec::is_empty),
        "Center input fixture requires its unmodified complete Top projection"
    );
    Ok(view)
}

fn circle(projection: &Value, view: &Value, reference: &Value) -> Result<([f64; 2], f64)> {
    curved::circular_ref(projection, reference)?;
    let circle = projection["circles"]
        .as_array()
        .context("Actual circular projection")?
        .iter()
        .find(|c| {
            c["body_id"] == reference["body_id"]
                && c["edge_id"] == reference["edge_id"]
                && c["edge_key"] == reference["edge_key"]
                && c["occurrence_id"] == reference["occurrence_id"]
        })
        .context("Exact current circular occurrence")?;
    ensure!(
        circle["center_model"][2] == 10.,
        "Center pick did not select the fixture's visible frontmost boss edge"
    );
    let b = &projection["bounds"];
    let scale = view["scale"].as_f64().context("View scale")?;
    let point = [
        view["position"][0].as_f64().unwrap()
            + (circle["center"][0].as_f64().unwrap()
                - (b[0].as_f64().unwrap() + b[2].as_f64().unwrap()) * 0.5)
                * scale,
        view["position"][1].as_f64().unwrap()
            - (circle["center"][1].as_f64().unwrap()
                - (b[1].as_f64().unwrap() + b[3].as_f64().unwrap()) * 0.5)
                * scale,
    ];
    Ok((point, circle["radius"].as_f64().unwrap() * scale))
}

fn verify_pick(point: [f64; 2], actual: [f64; 2], paper: &Paper) -> Result<()> {
    ensure!(
        (point[0] - actual[0]).hypot(point[1] - actual[1]) <= 1. / paper.scale + 1e-4,
        "Physical circle pick differs from its resolved current paper center: {actual:?}, expected {point:?}"
    );
    Ok(())
}

fn check_extension(origin: [f64; 2], direction: [f64; 2], endpoint: [f64; 2]) -> f64 {
    ((endpoint[0] - origin[0]) * direction[0] + (endpoint[1] - origin[1]) * direction[1]).max(0.)
}

pub(in super::super) fn exercise(
    c: &mut Client,
    out: &Path,
    server: &str,
    baseline: &Value,
) -> Result<Value> {
    ensure!(
        cfg!(target_os = "linux"),
        "Center OS proof requires disposable private Linux Xvfb"
    );
    let initial = inspect(c)?;
    let session = initial["active_session_id"]
        .as_str()
        .context("Owned center session")?;
    let driver = Driver::new(owned_pid(out, session, server)?, out)?;
    control(c, "Fit sheet", None)?;
    let paper = Paper::fitted(&inspect(c)?)?;
    let projection = curved::projection(c)?;
    let mut reports = Vec::new();
    for (stage, line, reverse) in [
        ("mark", false, false),
        ("line", true, false),
        ("line-reversed", true, true),
    ] {
        centers::open(c, line)?;
        let mut labels = centers::picks(c)?;
        if reverse {
            labels.reverse();
        }
        capture(c, out, &format!("center-os-{stage}-targets"))?;
        if line {
            for suffix in ["first", "duplicate"] {
                let point = center(&inspect(c)?, &labels[0])?;
                gesture(
                    &driver,
                    c,
                    out,
                    &format!("center-os-{stage}-{suffix}"),
                    "drawing-click",
                    point,
                    None,
                )?;
                ensure!(
                    &observed_model(c)? == baseline,
                    "First/duplicate center click changed model or IDs"
                );
                ensure!(
                    controls(&inspect(c)?)
                        .any(|r| r["label"] == labels[0] && r["selected"] == true),
                    "Physical center click did not stage its actual target"
                );
            }
            let point = center(&inspect(c)?, &labels[0])?;
            pointer(
                &driver,
                c,
                out,
                &format!("center-os-{stage}-pick-cancel"),
                point,
                point,
                true,
            )?;
            ensure!(
                &observed_model(c)? == baseline,
                "Held Escape changed center staging model or IDs"
            );
            ensure!(
                !controls(&inspect(c)?).any(|r| r["surface"] == "drawing/centers"),
                "Escape did not retire center staging"
            );
            centers::open(c, true)?;
        }
        let mut observed_picks = Vec::new();
        for (index, label) in labels.iter().take(if line { 2 } else { 1 }).enumerate() {
            let point = center(&inspect(c)?, label)?;
            let evidence = gesture(
                &driver,
                c,
                out,
                &format!("center-os-{stage}-pick-{index}"),
                "drawing-click",
                point,
                None,
            )?;
            observed_picks.push(paper.paper(observed_point(&evidence, "logical_start")?));
            if line && index == 0 {
                ensure!(
                    &observed_model(c)? == baseline,
                    "First circle committed before the second pick"
                );
            }
        }
        let created = changed_model(c, baseline, out, &format!("center-os-{stage}-create"))?;
        let a = exact_one_added(
            baseline,
            &created,
            out,
            &format!("center-os-{stage}-created"),
        )?;
        ensure!(
            a["kind"] == if line { "center_line" } else { "center_mark" },
            "Physical center pick created the wrong annotation family"
        );
        ensure!(
            a["extension"] == 2.5,
            "Physical center creation changed default paper extension"
        );
        let id = a["id"].as_u64().context("Created center ID")?;
        let v = view(&created, &a)?;
        let (first, radius) = circle(&projection, v, &a[if line { "first" } else { "feature" }])?;
        verify_pick(first, observed_picks[0], &paper)?;
        let (origin, direction) = if line {
            let (second, second_radius) = circle(&projection, v, &a["second"])?;
            verify_pick(second, observed_picks[1], &paper)?;
            let d = [second[0] - first[0], second[1] - first[1]];
            let length = d[0].hypot(d[1]);
            ensure!(length > 1., "Center pair collapsed");
            let d = d.map(|n| n / length);
            (
                [
                    second[0] + d[0] * second_radius,
                    second[1] + d[1] * second_radius,
                ],
                d,
            )
        } else {
            ([first[0] + radius, first[1]], [1., 0.])
        };
        history(c, baseline, &created)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        capture(c, out, &format!("center-os-{stage}-created"))?;
        let grip = format!("Center extension {id} grip 2");
        let start = center(&inspect(c)?, &grip)?;
        let start_paper = paper.paper(start);
        let expected_grip = [
            origin[0] + direction[0] * 2.5,
            origin[1] + direction[1] * 2.5,
        ];
        ensure!(
            (start_paper[0] - expected_grip[0]).hypot(start_paper[1] - expected_grip[1]) < 1e-4,
            "Published center grip is not on the current extension"
        );
        let end = [
            start[0] + direction[0] * paper.scale * 7.,
            start[1] + direction[1] * paper.scale * 7.,
        ];
        pointer(
            &driver,
            c,
            out,
            &format!("center-os-{stage}-drag-cancel"),
            start,
            end,
            true,
        )?;
        ensure!(
            observed_model(c)? == created,
            "Held Escape committed a partial center extension drag"
        );
        control(c, &format!("Edit annotation {id}"), None)?;
        let start = center(&inspect(c)?, &grip)?;
        let end = [
            start[0] + direction[0] * paper.scale * 7.,
            start[1] + direction[1] * paper.scale * 7.,
        ];
        let moved = gesture(
            &driver,
            c,
            out,
            &format!("center-os-{stage}-drag"),
            "drawing-drag",
            start,
            Some(end),
        )?;
        let actual_end = paper.paper(observed_point(&moved, "logical_end")?);
        let expected_extension = check_extension(origin, direction, actual_end);
        let dragged = changed_model(c, &created, out, &format!("center-os-{stage}-drag"))?;
        let row = annotations(&dragged)?
            .iter()
            .find(|r| r["id"] == id)
            .context("Dragged center annotation")?;
        let extension = row["extension"]
            .as_f64()
            .context("Saved center extension")?;
        ensure!(expected_extension > 8. && (extension-expected_extension).abs()<2e-4, "Physical center extension differs from observed paper endpoint: {extension}, expected {expected_extension}");
        ensure!(
            dragged
                == replace_expected(&created, id, |a| a["extension"] = row["extension"].clone()),
            "Center drag changed topology, presentation, or unrelated model intent"
        );
        history(c, &created, &dragged)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        capture(c, out, &format!("center-os-{stage}-dragged"))?;
        curved::save_exact(c, out, &format!("center-os-{stage}-dragged"), &dragged)?;
        curved::delete_and_restore(c, id, &created, &dragged, baseline)?;
        reports.push(json!({"case":stage,"actual_circle_center_clicks":observed_picks,"extension_origin_paper":origin,"extension_direction":direction,"observed_drag_end_paper":actual_end,"expected_extension_mm":expected_extension,"saved_extension_mm":extension,"physical_grip_drag":true,"held_escape_keeps_exact_model":true,"one_commit_exact_history_delete_archive":true,"captures":[format!("center-os-{stage}-targets.png"),format!("center-os-{stage}-created.png"),format!("center-os-{stage}-dragged.png")]}));
    }
    Ok(
        json!({"source":"Private Xvfb X11 XTEST on exact fixture PID","center_cases":reports,"both_pair_orders":true,"duplicate_pick_and_escape_keep_model_and_counter":true,"not_proven":["Pixel review of retained PNGs","Windows or macOS center input","Wayland","Monitor DPI transitions","Physical touchscreen"]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_expectation_uses_absolute_observed_paper_endpoint_and_direction() {
        assert_eq!(check_extension([40., 25.], [1., 0.], [49.5, 90.]), 9.5);
        assert_eq!(check_extension([40., 25.], [-1., 0.], [30.5, 90.]), 9.5);
        assert!((check_extension([40., 25.], [0.6, -0.8], [45.7, 17.4]) - 9.5).abs() < 1e-12);
        assert_eq!(check_extension([40., 25.], [1., 0.], [39., 25.]), 0.);
    }

    #[test]
    fn physical_expectation_requires_exact_visible_current_occurrence() {
        let reference = json!({"body_id":1,"edge_id":4,"edge_key":"circle","occurrence_id":"visible-instance","topology_signature":"current","fallback_center":[10.,20.,10.],"fallback_normal":[0.,0.,1.],"fallback_radius":3.,"closed":true});
        let mut projected = json!({"bounds":[0.,0.,40.,30.],"topology_signatures":{"1":"current"},"circles":[{"body_id":1,"edge_id":4,"edge_key":"circle","occurrence_id":"other-instance","center":[90.,80.],"center_model":[90.,80.,10.],"normal_model":[0.,0.,1.],"radius":3.,"closed":true,"hidden":false},{"body_id":1,"edge_id":4,"edge_key":"circle","occurrence_id":"visible-instance","center":[10.,20.],"center_model":[10.,20.,10.],"normal_model":[0.,0.,1.],"radius":3.,"closed":true,"hidden":false}]});
        let view = json!({"position":[50.,58.],"scale":2.});
        assert_eq!(
            circle(&projected, &view, &reference).unwrap(),
            ([30., 48.], 6.)
        );
        let mut stale = reference.clone();
        stale["topology_signature"] = json!("stale");
        assert!(circle(&projected, &view, &stale).is_err());
        stale = reference.clone();
        stale["occurrence_id"] = json!("missing");
        assert!(circle(&projected, &view, &stale).is_err());
        projected["circles"][1]["hidden"] = json!(true);
        assert!(circle(&projected, &view, &reference).is_err());
    }
}
