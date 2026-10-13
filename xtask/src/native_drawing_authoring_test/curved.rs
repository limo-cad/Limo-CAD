//! Actual real-solid circle/endpoint controls and complete shared history.
use super::*;

pub(super) fn projection(c: &mut Client) -> Result<Value> {
    let value = c.call(
        "drawing_projection",
        json!({"direction":[0.,0.,1.],"up":[0.,1.,0.],"include_hidden":true}),
    )?;
    if let Some(s) = value.as_str() {
        Ok(serde_json::from_str(s)?)
    } else {
        Ok(value)
    }
}
pub(super) fn circular_ref(projection: &Value, feature: &Value) -> Result<()> {
    let circle = projection["circles"]
        .as_array()
        .context("Projected circles")?
        .iter()
        .find(|r| {
            r["body_id"] == feature["body_id"]
                && r["edge_id"] == feature["edge_id"]
                && r["occurrence_id"] == feature["occurrence_id"]
        })
        .context("Created radial reference does not resolve to the actual projection")?;
    ensure!(
        circle["hidden"] == false && circle["closed"] == true,
        "Fixture picked hidden or open circular geometry"
    );
    for (saved, projected) in [
        ("edge_key", "edge_key"),
        ("fallback_center", "center_model"),
        ("fallback_normal", "normal_model"),
        ("fallback_radius", "radius"),
        ("closed", "closed"),
    ] {
        ensure!(
            feature[saved] == circle[projected],
            "Radial creation lost exact {saved}"
        );
    }
    ensure!(
        feature["topology_signature"]
            == projection["topology_signatures"]
                [feature["body_id"].as_u64().context("Body id")?.to_string()],
        "Radial creation lost current topology signature"
    );
    Ok(())
}
pub(super) fn endpoint_ref(projection: &Value, reference: &Value) -> Result<()> {
    let anchor = projection["anchors"]
        .as_array()
        .context("Projected endpoints")?
        .iter()
        .find(|r| {
            r["body_id"] == reference["body_id"]
                && r["edge_id"] == reference["edge_id"]
                && r["endpoint"] == reference["endpoint"]
                && r["occurrence_id"] == reference["occurrence_id"]
        })
        .context("Created angular reference does not resolve to an actual endpoint")?;
    ensure!(
        anchor["hidden"] == false
            && reference["fallback_point"] == anchor["model_point"]
            && reference["edge_key"] == anchor["edge_key"],
        "Angular creation lost exact visible endpoint intent"
    );
    ensure!(
        reference["topology_signature"]
            == projection["topology_signatures"][reference["body_id"]
                .as_u64()
                .context("Body id")?
                .to_string()],
        "Angular creation lost topology signature"
    );
    Ok(())
}
pub(super) fn save_exact(c: &mut Client, out: &Path, stage: &str, expected: &Value) -> Result<()> {
    let path = out.join(format!("author-{stage}.limo"));
    ui(c, json!({"action":"file","command":"save","path":path}))?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        &saved == expected && &model(c)? == expected,
        "Curved dimension archive changed exact shared intent"
    );
    Ok(())
}
pub(super) fn delete_and_restore(
    c: &mut Client,
    id: u64,
    before: &Value,
    edited: &Value,
    baseline: &Value,
) -> Result<()> {
    control(c, &format!("Edit annotation {id}"), None)?;
    control(c, "Delete annotation", None)?;
    let deleted = model(c)?;
    let mut expected = edited.clone();
    let active = edited["drawings"]["active_sheet_id"].clone();
    let sheet = expected["drawings"]["sheets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["id"] == active)
        .unwrap();
    sheet["annotations"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["id"] != id);
    ensure!(
        deleted == expected,
        "Curved deletion changed unrelated saved intent"
    );
    history(c, edited, &deleted)?;
    control(c, "Undo", None)?;
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == before,
        "Undo edit did not restore the exact new curved dimension"
    );
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == baseline,
        "Curved authoring did not restore all 24 saved annotations"
    );
    Ok(())
}
pub(super) fn angular_triple(c: &mut Client) -> Result<[String; 3]> {
    let state = ui(c, json!({"action":"inspect"}))?;
    let anchors: Vec<_> = controls(&state)
        .filter(|a| a["surface"] == "drawing/anchors" && a["disabled"] == false)
        .collect();
    let xy = |a: &Value| -> Result<[f64; 2]> {
        Ok([
            a["bounds"]["x"].as_f64().context("Anchor x")?
                + a["bounds"]["width"].as_f64().context("Anchor width")? / 2.,
            a["bounds"]["y"].as_f64().context("Anchor y")?
                + a["bounds"]["height"].as_f64().context("Anchor height")? / 2.,
        ])
    };
    let view = |a: &Value| {
        a["label"]
            .as_str()
            .unwrap_or("")
            .split(" anchor ")
            .next()
            .unwrap_or("")
            .to_owned()
    };
    for a in &anchors {
        for b in &anchors {
            for d in &anchors {
                if view(a) != view(b) || view(a) != view(d) {
                    continue;
                }
                let [p, q, r] = [xy(a)?, xy(b)?, xy(d)?];
                if (p[1] - q[1]).abs() < 0.02
                    && q[0] - p[0] > 60.
                    && (p[0] - r[0]).abs() < 0.02
                    && (r[1] - p[1]).abs() > 30.
                {
                    return Ok([a, b, d].map(|c| c["label"].as_str().unwrap().to_owned()));
                }
            }
        }
    }
    anyhow::bail!("Real projection has no visible same-view right-angle triple: {anchors:?}")
}
pub(super) fn exercise(c: &mut Client, out: &Path, baseline: &Value) -> Result<Value> {
    let projection = projection(c)?;
    let mut images = Vec::new();
    for (stage, label, mode, next_mode) in [
        ("radius", "Radius", "radius", "diameter"),
        ("diameter", "Diameter", "diameter", "radius"),
    ] {
        control(c, label, None)?;
        let state = ui(c, json!({"action":"inspect"}))?;
        let circle = controls(&state)
            .find(|a| a["surface"] == "drawing/circles" && a["disabled"] == false)
            .context("No visible actual circular edge controls")?["label"]
            .as_str()
            .context("Circle label")?
            .to_owned();
        ensure!(
            &model(c)? == baseline,
            "Selecting radial tool changed the model"
        );
        capture(c, out, &format!("author-{stage}-targets"))?;
        images.push(format!("author-{stage}-targets.png"));
        control(c, &circle, None)?;
        let created = model(c)?;
        let a = exact_one_added(baseline, &created, out, &format!("author-{stage}-created"))?;
        ensure!(
            a["kind"] == "radial_dimension"
                && a["mode"] == mode
                && a["leader_angle_deg"] == -35.
                && a["offset"] == 14.
                && a["precision"] == 2,
            "Existing radial creation defaults changed"
        );
        circular_ref(&projection, &a["feature"])?;
        let id = a["id"].as_u64().unwrap();
        history(c, baseline, &created)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        for (name, value) in [
            ("Dimension type", next_mode),
            ("Leader angle (degrees)", "25"),
            ("Leader offset (paper mm)", "24"),
            ("Precision", "3"),
            ("Prefix", "RAD "),
            ("Suffix", " exact"),
            ("Tolerance mode", "symmetric"),
            ("Upper tolerance", "0.1"),
            ("Reference dimension", "true"),
            ("Fit class", "H7"),
            ("Dual units", "true"),
            ("Secondary unit", "inch"),
            ("Dual precision", "3"),
            ("Dual placement", "bracketed"),
        ] {
            field(c, name, value)?;
        }
        ensure!(
            model(c)? == created,
            "Radial inspector applied before Apply"
        );
        control(c, "Apply annotation", None)?;
        let edited = model(c)?;
        let expected = replace_expected(&created, id, |a| {
            a["mode"] = json!(next_mode);
            a["leader_angle_deg"] = json!(25.);
            a["offset"] = json!(24.);
            a["precision"] = json!(3);
            a["prefix"] = json!("RAD ");
            a["suffix"] = json!(" exact");
            a["presentation"] = json!({"tolerance":{"mode":"symmetric","upper":0.1,"lower":0.},"basic":false,"reference":true,"fit_class":"H7","dual_units":{"unit":"inch","precision":3,"placement":"bracketed"}});
        });
        ensure!(
            edited == expected,
            "Radial inspector lost full reference/presentation intent"
        );
        history(c, &created, &edited)?;
        control(c, &format!("Edit annotation {id}"), None)?;
        capture(c, out, &format!("author-{stage}-edited"))?;
        images.push(format!("author-{stage}-edited.png"));
        save_exact(c, out, stage, &edited)?;
        delete_and_restore(c, id, &created, &edited, baseline)?;
    }
    control(c, "Angle", None)?;
    let labels = angular_triple(c)?;
    capture(c, out, "author-angular-targets")?;
    images.push("author-angular-targets.png".into());
    for label in labels.iter().take(2) {
        control(c, label, None)?;
        ensure!(
            &model(c)? == baseline,
            "Partial angular picks mutated the shared drawing"
        );
        control(c, label, None)?;
        ensure!(
            &model(c)? == baseline,
            "Duplicate angular pick created an annotation"
        );
    }
    control(c, &labels[2], None)?;
    let created = model(c)?;
    let a = exact_one_added(baseline, &created, out, "author-angular-created")?;
    ensure!(
        a["kind"] == "angular_dimension" && a["radius"] == 12. && a["precision"] == 1,
        "Existing angular creation defaults changed"
    );
    for key in ["vertex", "first", "second"] {
        endpoint_ref(&projection, &a[key])?;
    }
    let point = |k: &str| -> [f64; 3] {
        std::array::from_fn(|i| a[k]["fallback_point"][i].as_f64().unwrap())
    };
    let p = point("vertex");
    let q = point("first");
    let r = point("second");
    let dot = (0..3).map(|i| (q[i] - p[i]) * (r[i] - p[i])).sum::<f64>();
    ensure!(
        dot.abs() < 1e-8,
        "Projected right angle did not preserve actual orthogonal model rays"
    );
    let id = a["id"].as_u64().unwrap();
    history(c, baseline, &created)?;
    control(c, &format!("Edit annotation {id}"), None)?;
    for (name, value) in [
        ("Arc radius (paper mm)", "18"),
        ("Precision", "2"),
        ("Prefix", "ANGLE "),
        ("Suffix", " exact"),
        ("Tolerance mode", "deviation"),
        ("Upper tolerance", "0.2"),
        ("Lower tolerance", "-0.1"),
        ("Basic dimension", "true"),
    ] {
        field(c, name, value)?;
    }
    ensure!(
        model(c)? == created,
        "Angular inspector committed before Apply"
    );
    control(c, "Apply annotation", None)?;
    let edited = model(c)?;
    let expected = replace_expected(&created, id, |a| {
        a["radius"] = json!(18.);
        a["precision"] = json!(2);
        a["prefix"] = json!("ANGLE ");
        a["suffix"] = json!(" exact");
        a["presentation"] = json!({"tolerance":{"mode":"deviation","upper":0.2,"lower":-0.1},"basic":true,"reference":false,"fit_class":"","dual_units":null});
    });
    ensure!(
        edited == expected,
        "Angular inspector lost full anchor/presentation intent"
    );
    history(c, &created, &edited)?;
    control(c, &format!("Edit annotation {id}"), None)?;
    capture(c, out, "author-angular-edited")?;
    images.push("author-angular-edited.png".into());
    save_exact(c, out, "angular", &edited)?;
    delete_and_restore(c, id, &created, &edited, baseline)?;
    Ok(
        json!({"radial_angular_controls_passed":true,"exact_history_archive_passed":true,"shared_topology_preserved":true,"captures":images,"not_proven":["Actual OS radial ring click", "Actual OS radial/angular drag"]}),
    )
}
