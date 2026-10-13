//! Actual owned OS edge/point clicks, separate paper placement and label drag.
use super::super::{curved, straight};
use super::*;

pub(super) fn exercise(
    driver: &Driver,
    c: &mut Client,
    out: &Path,
    baseline: &Value,
    paper: &Paper,
) -> Result<Value> {
    let projection = curved::projection(c)?;
    let sheet = baseline["drawings"]["sheets"]
        .as_array()
        .context("Sheets")?
        .iter()
        .find(|s| s["id"] == baseline["drawings"]["active_sheet_id"])
        .context("Active sheet")?;
    let view = &sheet["views"][0];
    let view_id = view["id"].as_u64().context("View ID")?;
    let position = [
        view["position"][0].as_f64().unwrap() + 40.,
        view["position"][1].as_f64().unwrap() + 33.,
    ];
    let mut captures = Vec::new();
    for stage in ["edge-length", "point-line"] {
        control(c, "Linear dimension", None)?;
        let (edge, _, _, anchor) = straight::picks(c, &projection, view_id)?;
        let labels = if stage == "point-line" {
            vec![edge.label, anchor]
        } else {
            vec![edge.label]
        };
        for (index, label) in labels.iter().enumerate() {
            let state = inspect(c)?;
            let point = center(&state, label)?;
            gesture(
                driver,
                c,
                out,
                &format!("author-os-{stage}-pick-{index}"),
                "drawing-click",
                point,
                None,
            )?;
            ensure!(
                &observed_model(c)? == baseline,
                "Actual OS edge/point picks committed before placement"
            );
        }
        let placed = gesture(
            driver,
            c,
            out,
            &format!("author-os-{stage}-place"),
            "drawing-click",
            paper.screen(position),
            None,
        )?;
        let created = changed_model(c, baseline, out, &format!("author-os-{stage}-place"))?;
        let annotation = exact_one_added(
            baseline,
            &created,
            out,
            &format!("author-os-{stage}-created"),
        )?;
        let observed = paper.paper(observed_point(&placed, "logical_start")?);
        for axis in 0..2 {
            ensure!(
                (annotation["position"][axis]
                    .as_f64()
                    .context("Paper position")?
                    - observed[axis])
                    .abs()
                    < 1e-4,
                "Actual OS placement does not match observed paper coordinates"
            );
        }
        if stage == "edge-length" {
            ensure!(
                annotation["kind"] == "line_dimension" && annotation["mode"] == "length",
                "OS edge created wrong annotation"
            );
            straight::line_reference(&projection, &annotation["first"])?;
        } else {
            ensure!(
                annotation["kind"] == "point_line_dimension",
                "OS point and edge created wrong annotation"
            );
            straight::line_reference(&projection, &annotation["line"])?;
            curved::endpoint_ref(&projection, &annotation["point"])?;
        }
        let id = annotation["id"].as_u64().unwrap();
        history(c, baseline, &created)?;
        capture(c, out, &format!("author-os-{stage}-created"))?;
        captures.push(format!("author-os-{stage}-created.png"));
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
        let expected_position: [f64; 2] = std::array::from_fn(|axis| {
            annotation["position"][axis].as_f64().unwrap() + (to[axis] - from[axis]) / paper.scale
        });
        let dragged = changed_model(c, &created, out, &format!("author-os-{stage}-drag"))?;
        let row = annotations(&dragged)?
            .iter()
            .find(|a| a["id"] == id)
            .context("Dragged straight annotation")?;
        for axis in 0..2 {
            ensure!(
                (row["position"][axis].as_f64().unwrap() - expected_position[axis]).abs() < 1e-4,
                "Actual OS drag did not use cumulative paper delta"
            );
        }
        let expected = replace_expected(&created, id, |a| a["position"] = row["position"].clone());
        ensure!(
            dragged == expected,
            "OS straight drag changed references/presentation or unrelated intent"
        );
        history(c, &created, &dragged)?;
        capture(c, out, &format!("author-os-{stage}-dragged"))?;
        captures.push(format!("author-os-{stage}-dragged.png"));
        curved::save_exact(c, out, &format!("os-{stage}"), &dragged)?;
        control(c, "Undo", None)?;
        ensure!(model(c)? == created, "Drag Undo changed unrelated intent");
        control(c, "Undo", None)?;
        ensure!(
            &model(c)? == baseline,
            "Creation Undo did not restore baseline"
        );
    }
    Ok(
        json!({"actual_os_edge_point_pick_and_paper_placement":true,"actual_os_label_drag":true,
        "observed_logical_delta_matches_paper_position":true,"one_commit_exact_history_archive":true,"captures":captures}),
    )
}
