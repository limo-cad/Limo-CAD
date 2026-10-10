//! Genuine owned XTEST endpoint clicks and drags of every series label part.
use super::super::{
    curved::{angular_triple, endpoint_ref, projection},
    series::select_tool,
};
use super::*;

pub(super) fn exercise(
    driver: &Driver,
    c: &mut Client,
    out: &Path,
    baseline: &Value,
    paper: &Paper,
) -> Result<Value> {
    let projection = projection(c)?;
    let mut captures = Vec::new();
    for (label, layout) in [
        ("Chain", Some("chain")),
        ("Baseline", Some("baseline")),
        ("Continued", Some("continued")),
        ("Ordinate", None),
    ] {
        let stage = layout.unwrap_or("ordinate");
        select_tool(c, label)?;
        let labels = angular_triple(c)?;
        let count = if layout.is_some() { 3 } else { 2 };
        for (index, label) in labels[..count].iter().enumerate() {
            let state = inspect(c)?;
            let point = center(&state, label)?;
            gesture(
                driver,
                c,
                out,
                &format!("author-os-{stage}-anchor-{index}"),
                "drawing-click",
                point,
                None,
            )?;
            if index + 1 < count {
                ensure!(
                    &observed_model(c)? == baseline,
                    "Partial OS {stage} selection mutated the model"
                );
            }
        }
        let created = changed_model(c, baseline, out, &format!("author-os-{stage}-anchors"))?;
        let a = exact_one_added(
            baseline,
            &created,
            out,
            &format!("author-os-{stage}-created"),
        )?;
        if let Some(layout) = layout {
            ensure!(
                a["kind"] == "chain_dimension" && a["layout"] == layout,
                "OS endpoints selected the wrong series"
            );
            for reference in a["anchors"].as_array().context("Series anchors")? {
                endpoint_ref(&projection, reference)?;
            }
        } else {
            ensure!(
                a["kind"] == "ordinate_dimension",
                "OS endpoints did not create an ordinate"
            );
            endpoint_ref(&projection, &a["origin"])?;
            endpoint_ref(&projection, &a["target"])?;
        }
        history(c, baseline, &created)?;
        let id = a["id"].as_u64().context("Dimension ID")?;
        let parts = if layout.is_some() { 2 } else { 1 };
        for part in 0..parts {
            let label = if part == 0 {
                format!("Edit annotation {id}")
            } else {
                format!("Edit annotation {id} part {}", part + 1)
            };
            let state = inspect(c)?;
            let start = center(&state, &label)?;
            let moved = gesture(
                driver,
                c,
                out,
                &format!("author-os-{stage}-part-{part}-drag"),
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
            let increment = if layout.is_some() {
                delta[1]
            } else if delta[0].abs() > delta[1].abs() {
                delta[0]
            } else {
                delta[1]
            };
            let dragged = changed_model(
                c,
                &created,
                out,
                &format!("author-os-{stage}-part-{part}-drag"),
            )?;
            let row = annotations(&dragged)?
                .iter()
                .find(|a| a["id"] == id)
                .context("Dragged dimension")?;
            ensure!(
                (row["offset"].as_f64().context("Offset")?
                    - a["offset"].as_f64().unwrap()
                    - increment)
                    .abs()
                    < 1e-5,
                "OS {stage} part {part} drag changed the wrong paper offset"
            );
            let expected = replace_expected(&created, id, |a| a["offset"] = row["offset"].clone());
            ensure!(
                dragged == expected,
                "OS {stage} part {part} drag changed anchors/spacing/metadata or unrelated intent"
            );
            history(c, &created, &dragged)?;
            let image = format!("author-os-{stage}-part-{part}-dragged");
            capture(c, out, &image)?;
            captures.push(format!("{image}.png"));
            control(c, "Undo", None)?;
            ensure!(
                model(c)? == created,
                "OS drag Undo did not restore the whole dimension"
            );
        }
        control(c, "Undo", None)?;
        ensure!(
            &model(c)? == baseline,
            "OS {stage} creation Undo lost the saved baseline"
        );
    }
    Ok(
        json!({"actual_input":"Owned Xvfb X11 XTEST", "chain_baseline_continued_every_label_drag":true,"ordinate_creation_and_drag":true,"observed_os_delta_matches_paper_offset":true,"one_commit_exact_history":true,"captures":captures}),
    )
}
