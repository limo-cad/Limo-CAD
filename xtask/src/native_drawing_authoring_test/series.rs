//! The release three-endpoint series and origin/target ordinate tools.
use super::*;

pub(super) fn select_tool(c: &mut Client, label: &str) -> Result<()> {
    control(c, "More dimensions", None)?;
    control(c, label, None)?;
    Ok(())
}

fn paper_interior_triple(c: &mut Client) -> Result<[String; 3]> {
    let state = ui(c, json!({"action":"inspect"}))?;
    let mut views = std::collections::BTreeMap::<&str, Vec<(&str, [f64; 2])>>::new();
    for anchor in
        controls(&state).filter(|a| a["surface"] == "drawing/anchors" && a["disabled"] == false)
    {
        let label = anchor["label"].as_str().context("Endpoint label")?;
        let (view, _) = label.split_once(" anchor ").context("Endpoint view")?;
        let point = [
            anchor["bounds"]["x"].as_f64().context("Endpoint x")?
                + anchor["bounds"]["width"]
                    .as_f64()
                    .context("Endpoint width")?
                    / 2.,
            anchor["bounds"]["y"].as_f64().context("Endpoint y")?
                + anchor["bounds"]["height"]
                    .as_f64()
                    .context("Endpoint height")?
                    / 2.,
        ];
        views.entry(view).or_default().push((label, point));
    }
    let mut candidates = Vec::new();
    for anchors in views.values() {
        let min_x = anchors
            .iter()
            .map(|(_, p)| p[0])
            .fold(f64::INFINITY, f64::min);
        let min_y = anchors
            .iter()
            .map(|(_, p)| p[1])
            .fold(f64::INFINITY, f64::min);
        let max_x = anchors
            .iter()
            .map(|(_, p)| p[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = anchors
            .iter()
            .map(|(_, p)| p[1])
            .fold(f64::NEG_INFINITY, f64::max);
        if max_x - min_x <= 60. || max_y - min_y <= 30. {
            continue;
        }
        let corner = |x: f64, y: f64| {
            anchors
                .iter()
                .find(|(_, p)| (p[0] - x).abs() < 0.02 && (p[1] - y).abs() < 0.02)
                .map(|(label, _)| (*label).to_owned())
        };
        if let (Some(top_left), Some(bottom_left), Some(bottom_right)) = (
            corner(min_x, min_y),
            corner(min_x, max_y),
            corner(max_x, max_y),
        ) {
            candidates.push(([min_x, min_y], [top_left, bottom_left, bottom_right]));
        }
    }
    candidates.sort_by(|(a, _), (b, _)| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
    candidates
        .into_iter()
        .next()
        .map(|(_, labels)| labels)
        .context("Real projection has no visible same-view rectangular endpoint triple")
}

pub(super) fn exercise(c: &mut Client, out: &Path, baseline: &Value) -> Result<Value> {
    let projection = curved::projection(c)?;
    let mut images = Vec::new();
    for (label, layout) in [
        ("Chain", Some("chain")),
        ("Baseline", Some("baseline")),
        ("Continued", Some("continued")),
        ("Ordinate", None),
    ] {
        let stage = layout.unwrap_or("ordinate");
        select_tool(c, label)?;
        let labels = paper_interior_triple(c)?;
        let count = if layout.is_some() { 3 } else { 2 };
        for anchor in &labels[..count - 1] {
            control(c, anchor, None)?;
            ensure!(
                &model(c)? == baseline,
                "Partial {stage} selection mutated the model"
            );
            control(c, anchor, None)?;
            ensure!(
                &model(c)? == baseline,
                "Repeated {stage} endpoint advanced the selection"
            );
        }
        capture(c, out, &format!("author-{stage}-targets"))?;
        images.push(format!("author-{stage}-targets.png"));
        control(c, &labels[count - 1], None)?;
        let created = model(c)?;
        let a = exact_one_added(baseline, &created, out, &format!("author-{stage}-created"))?;
        let references = if let Some(layout) = layout {
            ensure!(
                a["kind"] == "chain_dimension"
                    && a["layout"] == layout
                    && a["mode"] == "aligned"
                    && a["spacing"] == 7.
                    && a["offset"] == 12.
                    && a["precision"] == 2,
                "Release series defaults changed"
            );
            a["anchors"]
                .as_array()
                .context("Series references")?
                .clone()
        } else {
            ensure!(
                a["kind"] == "ordinate_dimension"
                    && a["axis"] == "both"
                    && a["offset"] == 10.
                    && a["precision"] == 2,
                "Release ordinate defaults changed"
            );
            vec![a["origin"].clone(), a["target"].clone()]
        };
        ensure!(references.len() == count, "Saved endpoint count changed");
        for reference in &references {
            ensure!(
                reference["circle_center"] == false,
                "Series/ordinate creation exposed circle centers"
            );
            curved::endpoint_ref(&projection, reference)?;
        }
        let id = a["id"].as_u64().unwrap();
        history(c, baseline, &created)?;
        capture(c, out, &format!("author-{stage}-created"))?;
        images.push(format!("author-{stage}-created.png"));
        if layout.is_some() {
            let state = ui(c, json!({"action":"inspect"}))?;
            let first = controls(&state)
                .find(|a| a["label"] == format!("Edit annotation {id}"))
                .context("First series label")?;
            let second = controls(&state)
                .find(|a| a["label"] == format!("Edit annotation {id} part 2"))
                .context("Second series label")?;
            ensure!(
                first["bounds"] != second["bounds"],
                "Series label parts share one giant target"
            );
            control(c, &format!("Edit annotation {id} part 2"), None)?;
        } else {
            control(c, &format!("Edit annotation {id}"), None)?;
        }
        if layout.is_some() {
            field(c, "Baseline spacing (mm)", "11")?;
            field(c, "Prefix", "SER ")?;
            field(c, "Suffix", " exact")?;
        } else {
            field(c, "Axis", "y")?;
        }
        field(
            c,
            if layout.is_some() {
                "Offset (paper mm)"
            } else {
                "Leader offset (paper mm)"
            },
            "20",
        )?;
        for (name, value) in [
            ("Precision", "3"),
            ("Tolerance mode", "deviation"),
            ("Upper tolerance", "0.2"),
            ("Lower tolerance", "-0.1"),
            ("Reference dimension", "true"),
            ("Fit class", "H7"),
            ("Dual units", "true"),
            ("Secondary unit", "inch"),
            ("Dual precision", "3"),
            ("Dual placement", "bracketed"),
        ] {
            field(c, name, value)?;
        }
        ensure!(model(c)? == created, "{stage} field edits applied early");
        control(c, "Apply annotation", None)?;
        let edited = model(c)?;
        let expected = replace_expected(&created, id, |a| {
            a["offset"] = json!(20.);
            a["precision"] = json!(3);
            if layout.is_some() {
                a["spacing"] = json!(11.);
                a["prefix"] = json!("SER ");
                a["suffix"] = json!(" exact");
            } else {
                a["axis"] = json!("y");
            }
            a["presentation"] = json!({"tolerance":{"mode":"deviation","upper":0.2,"lower":-0.1},"basic":false,"reference":true,"fit_class":"H7","dual_units":{"unit":"inch","precision":3,"placement":"bracketed"}});
        });
        ensure!(
            edited == expected,
            "{stage} inspector lost references, metadata or unrelated intent"
        );
        history(c, &created, &edited)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        capture(c, out, &format!("author-{stage}-edited"))?;
        images.push(format!("author-{stage}-edited.png"));
        curved::save_exact(c, out, stage, &edited)?;
        curved::delete_and_restore(c, id, &created, &edited, baseline)?;
    }
    Ok(
        json!({"endpoint_creation_and_presentation":true,"every_series_label_published":true,"exact_history_archive":true,"all_saved_intent_preserved":true,"captures":images,"not_proven":["Actual OS series/ordinate gestures (separate opt-in fixture)"]}),
    )
}
