use super::*;
use crate::native_fixture::{browser_select, control_in};

pub(super) fn run(client: &mut Client, out: &Path, mirror: bool) -> Result<Value> {
    let kind = if mirror { "Mirror" } else { "Split Body" };
    let tag = if mirror { "mirror" } else { "split-body" };
    for (i, x) in (if mirror { vec![20., 80.] } else { vec![20.] })
        .into_iter()
        .enumerate()
    {
        begin_sketch(client, "XY")?;
        client.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+40.,"y":20.},"ctrl_held":true}))?;
        control(client, "Finish sketch", None)?;
        client.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    }
    let mut planes = Vec::new();
    if !mirror {
        for distance in ["40", "45"] {
            control(client, "Offset Plane", None)?;
            browser_select(client, "Origin", "YZ")?;
            control(client, "Offset distance", Some(distance))?;
            control(client, "Apply Offset Plane", None)?;
        }
        planes = client
            .call("construction_plane_definitions", json!({}))?
            .as_array()
            .context("Plane definitions missing")?
            .clone();
    }
    control(client, "Isometric", None)?;
    control(client, kind, None)?;
    for _ in 0..3 {
        ui(
            client,
            json!({"action":"viewport","gesture":"move","world":[30.,10.,10.]}),
        )?;
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[30.,10.,10.]}),
        )?;
    }
    if mirror {
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[110.,10.,10.]}),
        )?;
    }
    control(client, "Click a planar face or reference plane", None)?;
    if mirror {
        browser_select(client, "Origin", "YZ")?;
    } else {
        browser_select(client, "Construction", planes[0]["name"].as_str().unwrap())?;
    }
    capture(client, out, &format!("{tag}-form"))?;
    control(client, &format!("Apply {kind}"), None)?;
    let created = client.call("solid_scene", json!({}))?;
    ensure!(
        created["errors"].as_array().is_some_and(Vec::is_empty),
        "Invalid {kind} result"
    );
    let bodies = created["bodies"].as_array().context("Bodies missing")?;
    ensure!(
        bodies.len() == if mirror { 4 } else { 2 },
        "Wrong {kind} body count"
    );
    let mut bounds = Vec::new();
    for body in bodies {
        let positions = body["mesh"]["positions"]
            .as_array()
            .context("Mesh positions missing")?;
        let min = positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[0].as_f64().unwrap())
            .fold(f64::INFINITY, f64::min);
        let max = positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[0].as_f64().unwrap())
            .fold(f64::NEG_INFINITY, f64::max);
        bounds.push((min, max));
    }
    bounds.sort_by(|a, b| a.0.total_cmp(&b.0));
    let expected = if mirror {
        vec![(-120., -80.), (-60., -20.), (20., 60.), (80., 120.)]
    } else {
        vec![(20., 40.), (40., 60.)]
    };
    ensure!(bounds == expected, "Wrong {kind} extents: {bounds:?}");
    let definitions = client.call("solid_body_feature_definitions", json!({}))?;
    let definition = &definitions[0];
    let name = definition["name"]
        .as_str()
        .context("Feature name missing")?;
    let before = client.call("cad_document", json!({}))?;
    edit_feature(client, name, true)?;
    ensure!(
        client.call("cad_document", json!({}))? == before,
        "Opening {kind} moved live history"
    );
    control(client, &format!("Close {kind}"), None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))? == definitions,
        "Cancel changed {kind}"
    );
    edit_feature(client, name, false)?;
    control_in(
        client,
        if mirror { "solid/repeat" } else { "solid/body" },
        if mirror {
            "YZ origin plane"
        } else {
            planes[0]["name"].as_str().unwrap()
        },
        None,
    )?;
    if mirror {
        browser_select(client, "Origin", "XZ")?;
    } else {
        browser_select(client, "Construction", planes[1]["name"].as_str().unwrap())?;
    }
    capture(client, out, &format!("{tag}-edit"))?;
    control(client, &format!("Apply {kind}"), None)?;
    let edited = client.call("solid_body_feature_definitions", json!({}))?;
    ensure!(
        edited != definitions,
        "{kind} edit did not change its reference"
    );
    control(client, "Undo", None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))? == definitions,
        "{kind} Undo failed"
    );
    control(client, "Redo", None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))? == edited,
        "{kind} Redo failed"
    );
    ensure!(
        client.call("solid_scene", json!({}))?["errors"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "{kind} edit left geometry errors"
    );
    control(client, "Isometric", None)?;
    capture(client, out, tag)?;
    let project = out.join(format!("{tag}.limo"));
    ui(
        client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    println!("PASS: {kind} native body/plane picking, exact extents, history edit, Cancel, Undo/Redo and Save");
    Ok(json!({"operation":tag,"project":project,"definitions":edited}))
}
