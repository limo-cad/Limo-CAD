use super::*;

pub(super) fn run(client: &mut Client, out: &Path, circular: bool) -> Result<Value> {
    let kind = if circular {
        "Circular Pattern"
    } else {
        "Rectangular Pattern"
    };
    let tag = if circular {
        "circular-pattern"
    } else {
        "rectangular-pattern"
    };
    begin_sketch(client, "XY")?;
    client.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":20.,"y":0.},"p2":{"x":40.,"y":20.},"ctrl_held":true}),
    )?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    control(client, "Isometric", None)?;
    let original = client.call("cad_document", json!({}))?;
    control(client, kind, None)?;
    for _ in 0..3 {
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[30.,10.,10.]}),
        )?;
    }
    for value in ["1", "2.5", "10001", "4 mm"] {
        control(client, "Count", Some(value))?;
        let state = ui(client, json!({"action":"inspect"}))?;
        ensure!(
            controls(&state)
                .any(|c| c["label"] == format!("Apply {kind}") && c["disabled"] == true),
            "{kind} accepted invalid count {value}"
        );
    }
    control(client, "Count", Some(if circular { "4" } else { "3" }))?;
    control(
        client,
        if circular {
            "Pick an edge or enter an axis"
        } else {
            "Pick an edge or enter XYZ"
        },
        None,
    )?;
    let edge_point = if circular {
        [40., 0., 5.]
    } else {
        [30., 0., 10.]
    };
    ui(
        client,
        json!({"action":"viewport","gesture":"move","world":edge_point}),
    )?;
    ui(
        client,
        json!({"action":"viewport","gesture":"click","world":edge_point}),
    )?;
    let picked = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&picked).any(|c| c["label"] == "Straight edge selected"),
        "Straight edge did not populate {kind}"
    );
    if circular {
        for axis in ["X", "Y", "Z"] {
            control(client, &format!("Axis origin {axis}"), Some("0 mm"))?;
            control(
                client,
                &format!("Axis direction {axis}"),
                Some(if axis == "Z" { "1" } else { "0" }),
            )?;
        }
        control(client, "Total angle", Some("360 deg"))?;
    } else {
        control(client, "First direction X", Some("1"))?;
        control(client, "Spacing", Some("3 cm"))?;
        capture(client, out, &format!("{tag}-one-direction"))?;
        control(client, "Add a second direction", None)?;
        let state = ui(client, json!({"action":"inspect"}))?;
        if controls(&state).any(|c| c["label"] == "Scroll feature down" && c["disabled"] == false) {
            control(client, "Scroll feature down", None)?;
        }
        control(client, "Pick a second edge or enter XYZ", None)?;
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[40.,10.,10.]}),
        )?;
        control(client, "Second direction X", Some("0"))?;
        control(client, "Second direction Y", Some("1"))?;
        control(client, "Second direction Z", Some("0"))?;
        control(client, "Second spacing", Some("30"))?;
        control(client, "Second count", Some("10000"))?;
        let state = ui(client, json!({"action":"inspect"}))?;
        ensure!(
            controls(&state)
                .any(|c| c["label"] == format!("Apply {kind}") && c["disabled"] == true),
            "Unbounded pattern should be rejected"
        );
        control(client, "Second count", Some("2"))?;
    }
    ensure!(
        client.call("cad_document", json!({}))? == original,
        "The pattern draft changed history"
    );
    capture(client, out, &format!("{tag}-form"))?;
    control(client, &format!("Apply {kind}"), None)?;
    let created = client.call("solid_scene", json!({}))?;
    ensure!(
        created["errors"].as_array().is_some_and(Vec::is_empty),
        "Invalid pattern geometry"
    );
    let bodies = created["bodies"].as_array().context("Bodies missing")?;
    let mut centers = Vec::new();
    for b in bodies {
        let positions = b["mesh"]["positions"]
            .as_array()
            .context("Mesh positions missing")?;
        let center = |i: usize| {
            let values: Vec<_> = positions
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| p[i].as_f64().unwrap())
                .collect();
            ((values.iter().copied().fold(f64::INFINITY, f64::min)
                + values.iter().copied().fold(f64::NEG_INFINITY, f64::max))
                * 0.5)
                .round() as i32
        };
        centers.push((center(0), center(1)));
    }
    centers.sort_unstable();
    let expected = if circular {
        vec![(-30, -10), (-10, 30), (10, -30), (30, 10)]
    } else {
        vec![(30, 10), (30, 40), (60, 10), (60, 40), (90, 10), (90, 40)]
    };
    ensure!(centers == expected, "Wrong {kind} centers: {centers:?}");
    let definitions = client.call("solid_body_feature_definitions", json!({}))?;
    let name = definitions[0]["name"]
        .as_str()
        .context("Pattern name missing")?;
    let before = client.call("cad_document", json!({}))?;
    edit_feature(client, name, true)?;
    control(client, "Count", Some("5"))?;
    control(client, &format!("Close {kind}"), None)?;
    ensure!(
        client.call("cad_document", json!({}))? == before,
        "Pattern Cancel changed history"
    );
    edit_feature(client, name, false)?;
    control(
        client,
        if circular { "Total angle" } else { "Spacing" },
        Some(if circular { "-180" } else { "-30" }),
    )?;
    capture(client, out, &format!("{tag}-edit"))?;
    control(client, &format!("Apply {kind}"), None)?;
    let edited = client.call("solid_body_feature_definitions", json!({}))?;
    ensure!(
        edited != definitions,
        "Pattern edit did not change its values"
    );
    control(client, "Undo", None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))? == definitions,
        "Pattern Undo failed"
    );
    control(client, "Redo", None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))? == edited,
        "Pattern Redo failed"
    );
    ensure!(
        client.call("solid_scene", json!({}))?["errors"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Pattern edit left geometry errors"
    );
    control(client, "Isometric", None)?;
    capture(client, out, tag)?;
    let project = out.join(format!("{tag}.limo"));
    ui(
        client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    println!("PASS: {kind} native edge picking, typed vectors/units, invalid counts, exact placement, history edit, Cancel, Undo/Redo and Save");
    Ok(json!({"operation":tag,"project":project,"definitions":edited}))
}
