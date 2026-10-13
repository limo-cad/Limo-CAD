//! Published exact straight edges over the existing real-solid drawing fixture.
use super::*;

pub(super) struct Edge {
    pub(super) label: String,
    first: [f64; 2],
    second: [f64; 2],
}
pub(super) fn line_reference(projection: &Value, reference: &Value) -> Result<()> {
    let mut points = Vec::new();
    for (endpoint, fallback) in [("start", "fallback_start"), ("end", "fallback_end")] {
        let anchor = projection["anchors"]
            .as_array()
            .context("Projection anchors")?
            .iter()
            .find(|a| {
                a["body_id"] == reference["body_id"]
                    && a["edge_id"] == reference["edge_id"]
                    && a["edge_key"] == reference["edge_key"]
                    && a["occurrence_id"] == reference["occurrence_id"]
                    && a["endpoint"] == endpoint
            })
            .context("Straight reference does not resolve exactly")?;
        ensure!(
            reference[fallback] == anchor["model_point"],
            "Straight reference lost placed diagnostic endpoints"
        );
        points.push(anchor["point"].clone());
    }
    ensure!(
        points[0] != points[1],
        "Straight reference has zero projected length"
    );
    ensure!(
        reference["topology_signature"]
            == projection["topology_signatures"][reference["body_id"]
                .as_u64()
                .context("Body ID")?
                .to_string()],
        "Straight topology signature changed"
    );
    Ok(())
}
pub(super) fn picks(
    c: &mut Client,
    projection: &Value,
    view: u64,
) -> Result<(Edge, Edge, Edge, String)> {
    let state = ui(c, json!({"action":"inspect"}))?;
    let prefix = format!("View {view} straight edge ");
    let mut edges = Vec::new();
    for control in
        controls(&state).filter(|a| a["surface"] == "drawing/edges" && a["disabled"] == false)
    {
        let label = control["label"].as_str().context("Straight edge label")?;
        if !label.starts_with(&prefix) {
            continue;
        }
        let words: Vec<_> = label.split_whitespace().collect();
        ensure!(words.len() == 9, "Unexpected exact edge label: {label}");
        let edge = words[4].parse::<u64>()?;
        let body = words[6].parse::<u64>()?;
        let occurrence = if words[8] == "definition" {
            Value::Null
        } else {
            json!(words[8].parse::<u64>()?)
        };
        let point = |endpoint: &str| -> Result<[f64; 2]> {
            let a = projection["anchors"]
                .as_array()
                .context("Projected anchors")?
                .iter()
                .find(|a| {
                    a["edge_id"] == edge
                        && a["body_id"] == body
                        && a["occurrence_id"] == occurrence
                        && a["endpoint"] == endpoint
                })
                .context("Published edge missing exact projection endpoints")?;
            Ok([
                a["point"][0].as_f64().context("Projected x")?,
                a["point"][1].as_f64().context("Projected y")?,
            ])
        };
        edges.push(Edge {
            label: label.into(),
            first: point("start")?,
            second: point("end")?,
        });
    }
    let horizontal = |e: &Edge| {
        (e.first[1] - e.second[1]).abs() < 1e-7 && (e.first[0] - e.second[0]).abs() > 39.9
    };
    let take = |predicate: &dyn Fn(&Edge) -> bool| -> Result<Edge> {
        let e = edges
            .iter()
            .find(|e| predicate(e))
            .context("Real rectangle projected edge missing")?;
        Ok(Edge {
            label: e.label.clone(),
            first: e.first,
            second: e.second,
        })
    };
    let first = take(&|e| horizontal(e) && e.first[1].abs() < 1e-7)?;
    let parallel = take(&|e| horizontal(e) && (e.first[1] - 30.).abs() < 1e-7)?;
    let perpendicular = take(&|e| {
        (e.first[0] - e.second[0]).abs() < 1e-7 && (e.first[1] - e.second[1]).abs() > 29.9
    })?;
    let anchor_prefix = format!("View {view} anchor ");
    let mut anchors: Vec<_> = controls(&state)
        .filter(|a| {
            a["surface"] == "drawing/anchors"
                && a["label"]
                    .as_str()
                    .is_some_and(|label| label.starts_with(&anchor_prefix))
        })
        .collect();
    anchors.sort_by(|a, b| {
        a["bounds"]["y"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["bounds"]["y"].as_f64().unwrap())
            .then(
                a["bounds"]["x"]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b["bounds"]["x"].as_f64().unwrap()),
            )
    });
    let point = anchors.first().context("Published point-line anchor")?["label"]
        .as_str()
        .unwrap()
        .to_owned();
    Ok((first, parallel, perpendicular, point))
}
pub(super) fn exercise(c: &mut Client, out: &Path, baseline: &Value) -> Result<Value> {
    let projection = curved::projection(c)?;
    let sheet = baseline["drawings"]["sheets"]
        .as_array()
        .context("Sheets")?
        .iter()
        .find(|s| s["id"] == baseline["drawings"]["active_sheet_id"])
        .context("Active sheet")?;
    let view = &sheet["views"][0];
    let view_id = view["id"].as_u64().context("View ID")?;
    let mut captures = Vec::new();
    for stage in [
        "edge-length",
        "edge-distance",
        "edge-angle",
        "point-line",
        "point-first",
    ] {
        control(c, "Linear dimension", None)?;
        let (first, parallel, perpendicular, point) = picks(c, &projection, view_id)?;
        if stage == "point-first" {
            control(c, &point, None)?;
        }
        control(
            c,
            if stage == "edge-distance" {
                &parallel.label
            } else {
                &first.label
            },
            None,
        )?;
        match stage {
            "edge-distance" => {
                control(c, &first.label, None)?;
            }
            "edge-angle" => {
                control(c, &perpendicular.label, None)?;
            }
            "point-line" => {
                control(c, &point, None)?;
            }
            _ => {}
        }
        ensure!(
            &model(c)? == baseline,
            "Straight picks mutated the document before placement"
        );
        control(c, "Place dimension", None)?;
        let created = model(c)?;
        let a = exact_one_added(baseline, &created, out, stage)?;
        if stage.starts_with("edge-") {
            ensure!(
                a["kind"] == "line_dimension" && a["mode"] == stage.trim_start_matches("edge-"),
                "Wrong line relationship"
            );
            line_reference(&projection, &a["first"])?;
            if stage != "edge-length" {
                line_reference(&projection, &a["second"])?;
            } else {
                ensure!(a["second"].is_null(), "Length gained a second edge");
            }
        } else {
            ensure!(
                a["kind"] == "point_line_dimension",
                "Wrong point-line record"
            );
            curved::endpoint_ref(&projection, &a["point"])?;
            line_reference(&projection, &a["line"])?;
        }
        let id = a["id"].as_u64().context("Annotation ID")?;
        history(c, baseline, &created)?;
        capture(c, out, &format!("author-{stage}-created"))?;
        captures.push(format!("author-{stage}-created.png"));
        control(c, &format!("Edit annotation {id}"), None)?;
        let x = view["position"][0].as_f64().unwrap()
            + if matches!(stage, "edge-distance" | "point-line" | "point-first") {
                50.
            } else {
                30.
            };
        let y = view["position"][1].as_f64().unwrap() + 28.;
        for (name, value) in [
            ("Paper X (mm)", x.to_string()),
            ("Paper Y (mm)", y.to_string()),
            ("Precision", "3".into()),
            ("Prefix", "EDGE ".into()),
            ("Suffix", " exact".into()),
            ("Tolerance mode", "deviation".into()),
            ("Upper tolerance", "0.2".into()),
            ("Lower tolerance", "-0.1".into()),
            ("Reference dimension", "true".into()),
            ("Fit class", "H7".into()),
            ("Dual units", "true".into()),
            ("Secondary unit", "inch".into()),
            ("Dual precision", "3".into()),
            ("Dual placement", "bracketed".into()),
        ] {
            field(c, name, &value)?;
        }
        ensure!(
            model(c)? == created,
            "Straight fields mutated the document before Apply"
        );
        control(c, "Apply annotation", None)?;
        let edited = model(c)?;
        let expected = replace_expected(&created, id, |a| {
            a["position"] = json!([x, y]);
            a["precision"] = json!(3);
            a["prefix"] = json!("EDGE ");
            a["suffix"] = json!(" exact");
            a["presentation"] = json!({"tolerance":{"mode":"deviation","upper":0.2,"lower":-0.1},"basic":false,"reference":true,
                "fit_class":"H7","dual_units":{"unit":"inch","precision":3,"placement":"bracketed"}});
        });
        ensure!(
            edited == expected,
            "Straight edit lost exact references/presentation or unrelated intent"
        );
        history(c, &created, &edited)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        capture(c, out, &format!("author-{stage}-edited"))?;
        captures.push(format!("author-{stage}-edited.png"));
        curved::save_exact(c, out, stage, &edited)?;
        curved::delete_and_restore(c, id, &created, &edited, baseline)?;
    }
    Ok(
        json!({"line_modes_and_both_point_pick_orders":true,"exact_projected_references":true,
        "all_saved_intent_preserved":true,"exact_history_archive":true,"captures":captures,
        "not_proven":["Actual OS straight-edge placement/drag","Chamfer authoring","Native shared sheet export for these annotations"]}),
    )
}
