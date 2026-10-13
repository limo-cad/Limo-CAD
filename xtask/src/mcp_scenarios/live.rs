use super::*;
pub(super) fn run(s: &mut Scenario) -> Result<()> {
    ensure!(
        s.options.contains_key("--desktop") || s.session.is_some(),
        "Use --desktop PATH or --session UUID"
    );
    s.attach_or_launch()?;
    let launched = s.session.clone().context("Live session missing")?;
    s.ui(json!({"action":"inspect","pace_ms":s.options.get("--pace").map(|v|v.parse::<u64>().unwrap()).unwrap_or(0)}))?;
    for mode in ["background", "foreground"] {
        let state = s.ui(json!({"action":"window","mode":mode}))?;
        ensure!(
            state["value"]["window_transition"] == mode,
            "Window mode was not applied"
        );
        if mode == "background" && s.options.contains_key("--idle") {
            std::thread::sleep(Duration::from_secs(35));
        }
        let view = s.ui(json!({"action":"view","view":"top"}))?;
        let mut offset = [0.; 3];
        for (i, n) in offset.iter_mut().enumerate() {
            *n = number(&view["value"]["camera"]["position"][i])?
                - number(&view["value"]["camera"]["target"][i])?;
        }
        ensure!(
            offset[2] > 0. && offset[0].hypot(offset[1]) < 1e-5,
            "Top view has wrong direction"
        );
    }
    let snapshot = s.ui(json!({"action":"inspect"}))?;
    ensure!(
        !array(&snapshot["ui"], "surfaces")?.is_empty(),
        "UI surfaces missing"
    );
    if let Some(row) = controls_in(&snapshot).find(|c| c["disabled"] == true) {
        ensure!(
            s.interface(json!({"action":"click","target":row["id"]}))?["status"] == "failed",
            "Disabled control was accepted"
        );
    }
    s.ui(json!({"action":"inspect"}))?;
    let stale = controls_in(&snapshot)
        .next()
        .context("No snapshot control")?["id"]
        .clone();
    ensure!(
        s.interface(json!({"action":"click","target":stale}))?["status"] == "failed",
        "Stale snapshot ID was accepted"
    );
    if !s.options.contains_key("--part") {
        s.check("live window state and revision-bound controls");
        return Ok(());
    }
    s.call("cad_attach", json!({"session_id":launched}))?;
    ensure!(
        array(&s.model()?["document"]["history"], "features")?.is_empty(),
        "Part demo requires an empty document"
    );
    for (name, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle_locked",
            json!({"mode":"two_point","anchor":{"x":-30,"y":-20},"corner_hint":{"x":30,"y":20},"width_mm":60,"height_mm":40,"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
    ] {
        s.call(name, args)?;
        let state = s.ui(json!({"action":"inspect"}))?;
        ensure!(
            controls_in(&state).any(|c| c["label"] == "Finish sketch") == (name != "sketch_finish"),
            "UI did not follow sketch lifecycle"
        );
    }
    s.call("cad_detach", json!({}))?;
    let replay_result = (|| -> Result<()> {
        let script = s.call("cad_script", json!({}))?;
        ensure!(
            !array(&script, "calls")?
                .iter()
                .any(|c| c["name"] == "cad_interface"),
            "Portable script contains desktop transport"
        );
        let mut replay = Client::start_command(
            Client::worker_command(&s.options["--server"]),
            Some(Duration::from_secs(60)),
        )?;
        for step in array(&script, "calls")? {
            replay.call(
                step["name"].as_str().context("Replay call name missing")?,
                step["arguments"].clone(),
            )?;
        }
        let text = replay.call("cad_project_model", json!({}))?;
        let model: Value = serde_json::from_str(text.as_str().context("Replay model missing")?)?;
        ensure!(
            array(&model, "sketches")?.len() == 1,
            "Live script replayed an edit more than once"
        );
        Ok(())
    })();
    let reattached = s.call("cad_attach", json!({"session_id":s.session}));
    replay_result?;
    reattached?;
    s.control("Extrude", None)?;
    s.ui(json!({"action":"viewport","world":[0,0,0]}))?;
    crate::native_fixture::field(&mut s.client, "Distance (mm)", Some("5"))?;
    s.control("Apply Extrude", None)?;
    s.call("cad_refresh", json!({}))?;
    let model = s.model()?;
    ensure!(
        array(&model, "sketches")?.len() == 1
            && array(&model["document"]["history"], "features")?
                .iter()
                .any(|f| f["kind"] == "extrude"),
        "UI failed to commit native extrusion"
    );
    let scene = s.call("solid_scene", json!({}))?;
    ensure!(
        array(&scene, "bodies")?.len() == 1 && array(&scene, "errors")?.is_empty(),
        "Extrusion geometry failed"
    );
    let z: Vec<_> = array(&scene["bodies"][0]["mesh"], "positions")?
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| number(&p[2]))
        .collect::<Result<_>>()?;
    ensure!(
        (z.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - z.iter().copied().fold(f64::INFINITY, f64::min)
            - 5.)
            .abs()
            < 1e-5,
        "Native distance field did not affect solid height"
    );
    if s.options.contains_key("--drawing") {
        s.control("Switch workspace", None)?;
        s.control("Drawing", None)?;
        s.control("New Sheet", None)?;
        s.control("Front View", None)?;
        let state = s.ui(json!({"action":"inspect"}))?;
        let canvas = array(&state["ui"], "canvases")?
            .iter()
            .find(|c| c["name"] == "drawing")
            .context("Drawing canvas missing")?;
        s.ui(json!({"action":"viewport","canvas":"drawing","point":[number(&canvas["x"])?+number(&canvas["width"])?*0.35,number(&canvas["y"])?+number(&canvas["height"])?*0.4]}))?;
        s.call("cad_refresh", json!({}))?;
        ensure!(
            array(&s.model()?["drawings"]["sheets"][0], "views")?.len() == 1,
            "UI placement did not create a drawing view"
        );
        s.control("Switch workspace", None)?;
        s.control("Solid Modeling", None)?;
    }
    if let Some(path) = s.options.get("--save").cloned() {
        s.ui(json!({"action":"file","command":"save","path":path}))?;
        ensure!(
            s.interface(json!({"action":"file","command":"save","path":path}))?["status"]
                == "failed",
            "Overwrite did not require explicit approval"
        );
        let opened = s.ui(json!({"action":"file","command":"open","path":path}))?;
        ensure!(
            opened["attached_session_id"] == opened["active_session_id"],
            "Open did not bind following calls"
        );
        ensure!(
            array(&s.call("solid_scene", json!({}))?, "bodies")?.len() == 1,
            "Reads did not follow reopened part"
        );
        s.ui(json!({"action":"inspect"}))?;
        let part_session = s.session.clone();
        let part_name = s.call("cad_document", json!({}))?["name"].clone();
        s.control("New design", None)?;
        ensure!(s.session != part_session, "New design reused resident tab");
        let empty_session = s.session.clone();
        let empty_path = format!("{path}.empty.limo");
        ensure!(
            !PathBuf::from(&empty_path).exists(),
            "Empty fixture save already exists"
        );
        s.ui(json!({"action":"file","command":"save","path":empty_path}))?;
        let empty_name = s.call("cad_document", json!({}))?["name"].clone();
        ensure!(
            array(&s.call("solid_scene", json!({}))?, "bodies")?.is_empty(),
            "New document is not empty"
        );
        s.ui(json!({"action":"file","command":"open","path":path}))?;
        ensure!(
            s.session == empty_session
                && array(&s.call("solid_scene", json!({}))?, "bodies")?.len() == 1
                && s.call("cad_document", json!({}))?["name"] == part_name,
            "Different-document open retained stale model"
        );
        s.ui(json!({"action":"file","command":"open","path":empty_path}))?;
        ensure!(
            s.session == empty_session
                && array(&s.call("solid_scene", json!({}))?, "bodies")?.is_empty()
                && s.call("cad_document", json!({}))?["name"] == empty_name,
            "Opening empty model retained old geometry"
        );
        let state = s.ui(json!({"action":"inspect"}))?;
        ensure!(
            controls_in(&state).filter(|c| c["role"] == "tab").count() == 2,
            "Fixture must have only its two tabs"
        );
        s.ui(json!({"action":"file","command":"close"}))?;
        ensure!(
            s.session == part_session
                && array(&s.call("solid_scene", json!({}))?, "bodies")?.len() == 1
                && s.call("cad_document", json!({}))?["name"] == part_name,
            "Close did not bind surviving tab"
        );
    }
    s.check("live native modeling, portable replay, file replacement and tab rebinding");
    Ok(())
}
