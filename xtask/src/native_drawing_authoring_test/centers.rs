//! Published center controls over the established real OCCT boss fixture.
use super::*;

pub(super) fn open(c: &mut Client, line: bool) -> Result<()> {
    control(c, "More dimensions", None)?;
    control(c, if line { "Centerline" } else { "Center Mark" }, None)?;
    Ok(())
}
pub(super) fn picks(c: &mut Client) -> Result<[String; 2]> {
    let state = ui(c, json!({"action":"inspect"}))?;
    let mut by_view = std::collections::BTreeMap::<String, Vec<(String, f64, f64)>>::new();
    for row in
        controls(&state).filter(|r| r["surface"] == "drawing/centers" && r["disabled"] == false)
    {
        let label = row["label"].as_str().context("Circular center label")?;
        let view = label
            .split(" circular center ")
            .next()
            .context("Center view label")?
            .to_owned();
        let x = row["bounds"]["x"].as_f64().context("Center X")?;
        let y = row["bounds"]["y"].as_f64().context("Center Y")?;
        by_view.entry(view).or_default().push((label.into(), x, y));
    }
    let rows = by_view
        .values_mut()
        .filter(|r| r.len() >= 2)
        .min_by(|a, b| a[0].2.total_cmp(&b[0].2))
        .context("Two real same-view circular centers")?;
    rows.sort_by(|a, b| a.1.total_cmp(&b.1));
    ensure!(
        (rows[0].1 - rows[1].1).hypot(rows[0].2 - rows[1].2) > 2.,
        "Projected centers coincide"
    );
    Ok([rows[0].0.clone(), rows[1].0.clone()])
}
pub(super) fn exercise(c: &mut Client, out: &Path, baseline: &Value) -> Result<Value> {
    let projection = curved::projection(c)?;
    std::fs::write(
        out.join("center-real-projection.json"),
        serde_json::to_vec_pretty(&projection)?,
    )?;
    let mut captures = Vec::new();
    for (stage, line, reverse) in [
        ("center-mark", false, false),
        ("center-line", true, false),
        ("center-line-reversed", true, true),
    ] {
        open(c, line)?;
        let mut labels = picks(c)?;
        if reverse {
            labels.reverse();
        }
        capture(c, out, &format!("author-{stage}-targets"))?;
        captures.push(format!("author-{stage}-targets.png"));
        if line {
            control(c, &labels[0], None)?;
            control(c, &labels[0], None)?;
            ensure!(
                &model(c)? == baseline,
                "First/duplicate circular center committed the model"
            );
            control(c, "Sheet setup", None)?;
            ensure!(
                &model(c)? == baseline,
                "Center cancellation changed the model"
            );
            open(c, true)?;
            control(c, &labels[0], None)?;
        }
        control(c, &labels[if line { 1 } else { 0 }], None)?;
        let created = model(c)?;
        let a = exact_one_added(baseline, &created, out, &format!("author-{stage}-created"))?;
        ensure!(
            a["kind"] == (if line { "center_line" } else { "center_mark" })
                && a["extension"] == 2.5,
            "Center defaults changed"
        );
        for key in if line {
            vec!["first", "second"]
        } else {
            vec!["feature"]
        } {
            curved::circular_ref(&projection, &a[key])?;
        }
        let id = a["id"].as_u64().context("Center annotation ID")?;
        history(c, baseline, &created)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        capture(c, out, &format!("author-{stage}-created"))?;
        captures.push(format!("author-{stage}-created.png"));
        field(c, "Extension (paper mm)", "13.125")?;
        ensure!(
            model(c)? == created,
            "Center extension committed before Apply"
        );
        control(c, "Reset annotation", None)?;
        ensure!(
            model(c)? == created,
            "Center Reset mutated the shared document"
        );
        field(c, "Extension (paper mm)", "13.125")?;
        control(c, "Apply annotation", None)?;
        let edited = model(c)?;
        let expected = replace_expected(&created, id, |a| a["extension"] = json!(13.125));
        ensure!(
            edited == expected,
            "Center extension edit changed associations or unrelated intent"
        );
        history(c, &created, &edited)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        let state = ui(c, json!({"action":"inspect"}))?;
        let grip_count = controls(&state)
            .filter(|r| {
                r["label"]
                    .as_str()
                    .is_some_and(|label| label.starts_with(&format!("Center extension {id} grip ")))
            })
            .count();
        ensure!(
            grip_count == if line { 2 } else { 4 },
            "Expected visible center extension grips"
        );
        capture(c, out, &format!("author-{stage}-edited"))?;
        captures.push(format!("author-{stage}-edited.png"));
        curved::save_exact(c, out, stage, &edited)?;
        curved::delete_and_restore(c, id, &created, &edited, baseline)?;
    }
    Ok(
        json!({"center_mark_and_two_circle_centerline_controls_passed":true,"current_projection_refs_passed":true,"both_pair_orders_passed":true,"cancel_reset_no_mutation_passed":true,"exact_history_archive_and_preservation_passed":true,"captures":captures,"not_proven":["Physical OS circle selection","Physical extension-grip drag","Linux/macOS center annotation pixels"]}),
    )
}
