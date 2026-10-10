use super::*;
fn generation(s: &mut Scenario) -> Result<Value> {
    let sessions = s.call("cad_list_sessions", json!({}))?;
    array(&sessions, "session_details")?
        .iter()
        .find(|r| r["session_id"].as_str() == s.session.as_deref())
        .map(|r| r["heartbeat"]["generation"].clone())
        .filter(|g| g.is_u64())
        .context("Attached generation missing")
}
fn mutate(s: &mut Scenario, name: &str, args: Value) -> Result<()> {
    let generation = generation(s)?;
    let submitted = s.call(
        "cad_submit",
        json!({"name":name,"arguments":args,"base_generation":generation}),
    )?;
    let applied = s.call(
        "cad_await_apply",
        json!({"seq":submitted["seq"],"timeout_ms":10000}),
    )?;
    ensure!(applied["status"] == "applied", "Mutation failed: {applied}");
    s.call("cad_refresh", json!({}))?;
    Ok(())
}
pub(super) fn run(s: &mut Scenario) -> Result<()> {
    ensure!(
        s.session.is_some(),
        "Use --session UUID for an active disposable document"
    );
    s.attach_or_launch()?;
    if let Some(path) = s.options.get("--model").cloned() {
        mutate(
            s,
            "cad_load_project_model",
            json!({"model_json":fs::read_to_string(path)?}),
        )?;
    }
    let original = s.call("cad_project_model", json!({}))?;
    let before = generation(s)?;
    let home = s.ui(json!({"action":"view","view":"isometric","fit":false}))?;
    let fit = s.ui(json!({"action":"view","view":"current","fit":true}))?;
    for key in ["position", "target"] {
        for axis in 0..3 {
            ensure!(
                (number(&home["value"]["camera"][key][axis])?
                    - number(&fit["value"]["camera"][key][axis])?)
                .abs()
                    < 1e-6,
                "ISO does not already frame the geometry"
            );
        }
    }
    for (view, direction) in [
        ("current", None),
        ("top", Some([0., 0., 1.])),
        ("bottom", Some([0., 0., -1.])),
        ("front", Some([0., -1., 0.])),
        ("back", Some([0., 1., 0.])),
        ("left", Some([-1., 0., 0.])),
        ("right", Some([1., 0., 0.])),
        ("isometric", None),
    ] {
        let result = s.ui(json!({"action":"view","view":view,"fit":view!="current"}))?;
        let mut offset = [0.; 3];
        for axis in 0..3 {
            offset[axis] = number(&result["value"]["camera"]["position"][axis])?
                - number(&result["value"]["camera"]["target"][axis])?;
        }
        if let Some(direction) = direction {
            let length = offset.iter().map(|n| n * n).sum::<f64>().sqrt();
            ensure!(length > 0., "Camera coincides with target");
            for axis in 0..3 {
                ensure!(
                    (offset[axis] / length - direction[axis]).abs() < 1e-5,
                    "Wrong {view} direction: {result}"
                );
            }
        }
        ensure!(generation(s)? == before, "Camera changed engine generation");
        s.report["views"]
            .as_array_mut()
            .unwrap()
            .push(json!({"view":view,"result":result}));
    }
    s.call("cad_refresh", json!({}))?;
    let base: Value = serde_json::from_str(original.as_str().context("Model text missing")?)?;
    ensure!(s.model()? == base, "Camera changed model");
    let joint = array(&base["assembly"], "joints")?
        .first()
        .cloned()
        .context("Test document needs a joint")?;
    let result = (|| -> Result<()> {
        mutate(
            s,
            "assembly_set_joint_enabled",
            json!({"joint_id":joint["id"],"enabled":false}),
        )?;
        let mut expected = joint.clone();
        expected["enabled"] = json!(false);
        ensure!(
            s.model()?["assembly"]["joints"][0] == expected,
            "Joint disable changed definition"
        );
        mutate(
            s,
            "assembly_set_joint_enabled",
            json!({"joint_id":joint["id"],"enabled":true}),
        )?;
        let mut movable = joint.clone();
        for (key, value) in [
            ("kind", json!("revolute")),
            ("enabled", json!(true)),
            ("limits", Value::Null),
            ("angle_limits", Value::Null),
            ("linear_limits", Value::Null),
        ] {
            movable[key] = value;
        }
        mutate(s, "assembly_update_joint", json!({"joint":movable}))?;
        mutate(
            s,
            "assembly_set_joint_motion",
            json!({"joint_id":joint["id"],"angle_offset_deg":15,"linear_offset_mm":0}),
        )?;
        movable["angle_offset_deg"] = json!(15.0);
        movable["linear_offset_mm"] = json!(0.0);
        ensure!(
            s.model()?["assembly"]["joints"][0] == movable,
            "Joint motion changed unrelated fields"
        );
        mutate(s, "assembly_delete_joint", json!({"joint_id":joint["id"]}))?;
        ensure!(
            !array(&s.model()?["assembly"], "joints")?
                .iter()
                .any(|j| j["id"] == joint["id"]),
            "Joint deletion failed"
        );
        Ok(())
    })();
    let restored = mutate(s, "cad_load_project_model", json!({"model_json":original}));
    result?;
    restored.context("Restore original assembly")?;
    let solution = s.call("assembly_solution", json!({}))?;
    ensure!(
        solution["solved"] == true && array(&solution, "diagnostics")?.is_empty(),
        "Restored assembly did not solve"
    );
    s.check("live camera and joint controls; original assembly restored");
    Ok(())
}
