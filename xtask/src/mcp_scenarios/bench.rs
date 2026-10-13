use super::*;

struct Part {
    name: String,
    height: f64,
    body: Value,
    component: Value,
    occurrence: Value,
}
fn board(s: &mut Scenario, name: &str, width: f64, depth: f64, height: f64) -> Result<Part> {
    let before = s.call("solid_scene", json!({}))?;
    s.call(
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"xy"}}),
    )?;
    s.call("sketch_add_rectangle_locked", json!({"mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":width,"y":depth},"width_mm":width,"height_mm":depth,"ctrl_held":true}))?;
    s.call("sketch_finish", json!({}))?;
    let sketch = last(&s.model()?, "sketches")?["name"].clone();
    s.call("solid_extrude", json!({"sketch_name":sketch,"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":height},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}))?;
    let previous = array(&before, "bodies")?;
    let scene = s.call("solid_scene", json!({}))?;
    let body = array(&scene, "bodies")?
        .iter()
        .find(|b| !previous.iter().any(|old| old["id"] == b["id"]))
        .cloned()
        .context("Board did not create a body")?;
    s.call(
        "assembly_create_component",
        json!({"name":name,"body_ids":[body["id"]],"absorb_promoted_bodies":true}),
    )?;
    let assembly = s.call("assembly_document", json!({}))?;
    let component = array(&assembly["component_structure"], "definitions")?
        .iter()
        .find(|c| c["name"] == name)
        .cloned()
        .context("Component missing")?;
    let occurrence = array(&assembly["component_structure"], "occurrences")?
        .iter()
        .find(|o| o["component_id"] == component["id"])
        .cloned()
        .context("Occurrence missing")?;
    s.report["parts"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":name,"width":width,"depth":depth,"height":height}));
    Ok(Part {
        name: name.into(),
        height,
        body,
        component,
        occurrence,
    })
}
fn connector(part: &Part, axis: usize, positive: bool, origin: [f64; 3]) -> Result<Value> {
    let sign = if positive { 1. } else { -1. };
    let face = array(&part.body, "faces")?
        .iter()
        .find(|f| {
            f["plane"]["normal"][axis]
                .as_f64()
                .is_some_and(|n| n * sign > 0.99)
        })
        .context("Connector face missing")?;
    Ok(
        json!({"body_id":part.body["id"],"face_id":face["id"],"face_key":face["key"],"kind":"planar_face","frame":{"origin":origin,"primary_axis":face["plane"]["normal"],"secondary_axis":[1,0,0]},"source_surface_frame":{"origin":face["plane"]["origin"],"primary_axis":face["plane"]["normal"],"secondary_axis":face["plane"]["u"]}}),
    )
}
fn repeat(s: &mut Scenario, part: &Part, index: usize) -> Result<Value> {
    if index == 0 {
        return Ok(part.occurrence.clone());
    }
    let before = s.call("assembly_document", json!({}))?;
    s.call(
        "assembly_create_occurrence",
        json!({"component_id":part.component["id"],"name":format!("{} {}",part.name,index+1)}),
    )?;
    let after = s.call("assembly_document", json!({}))?;
    let previous = array(&before["component_structure"], "occurrences")?;
    array(&after["component_structure"], "occurrences")?
        .iter()
        .find(|o| !previous.iter().any(|old| old["id"] == o["id"]))
        .cloned()
        .context("Repeated occurrence missing")
}
pub(super) fn run(s: &mut Scenario) -> Result<()> {
    s.report["name"] = json!("Garden workshop bench");
    s.tools = s
        .client
        .call("cad_list_all_tools", json!({}))?
        .as_array()
        .context("Tool catalogue missing")?
        .clone();
    s.attach_or_launch()?;
    if s.session.is_some() {
        s.ui(json!({"action":"inspect","pace_ms":s.options.get("--pace").map(|p|p.parse::<u64>().unwrap()).unwrap_or(0)}))?;
    }
    let empty = s.model()?;
    ensure!(
        array(&empty["document"]["history"], "features")?.is_empty(),
        "Use a new empty document for the bench"
    );
    if s.options.contains_key("--workshop") {
        super::workshop::run(s, &serde_json::to_string(&empty)?)?;
    }
    let slat = board(s, "Seat slat", 1200., 85., 35.)?;
    s.call("assembly_set_occurrence_pose",json!({"occurrence_id":slat.occurrence["id"],"local_pose":{"translation":[0,0,415],"rotation":[0,0,0,1]}}))?;
    s.call(
        "assembly_set_occurrence_grounded",
        json!({"occurrence_id":slat.occurrence["id"],"grounded":true}),
    )?;
    let mut poses = vec![];
    let mut slats = vec![slat.occurrence.clone()];
    for (i, y) in [0., 90., 180., 270., 360.].into_iter().enumerate().skip(1) {
        let occurrence = repeat(s, &slat, i)?;
        horizontal_mount(s, &slat, &slat, &occurrence, [0., y], true)?;
        poses.push(json!({"occurrence_id":occurrence["id"],"translation":[0.,y,415.]}));
        slats.push(occurrence);
    }
    for (name, width, depth, height, positions) in [
        (
            "Leg",
            60.,
            60.,
            415.,
            vec![[60., 35.], [1080., 35.], [60., 340.], [1080., 340.]],
        ),
        ("Long apron", 1080., 25., 90., vec![[60., 50.], [60., 365.]]),
        ("Side rail", 25., 305., 90., vec![[78., 60.], [1097., 60.]]),
    ] {
        let part = board(s, name, width, depth, height)?;
        for (i, xy) in positions.into_iter().enumerate() {
            let occurrence = repeat(s, &part, i)?;
            horizontal_mount(s, &slat, &part, &occurrence, xy, false)?;
            poses.push(
                json!({"occurrence_id":occurrence["id"],"translation":[xy[0],xy[1],415.-height]}),
            );
        }
    }
    let post = board(s, "Back post", 50., 40., 740.)?;
    for (i, x) in [65., 1085.].into_iter().enumerate() {
        let occurrence = repeat(s, &post, i)?;
        side_mount(
            s,
            &slat,
            slats.last().unwrap(),
            &post,
            &occurrence,
            ([x, 85., 0.], [0., 40., 415.]),
            true,
        )?;
        poses.push(json!({"occurrence_id":occurrence["id"],"translation":[x,405.,0.]}));
    }
    for (name, z) in [("Back board", 550.), ("Upper back board", 655.)] {
        let part = board(s, name, 1200., 25., 85.)?;
        side_mount(
            s,
            &post,
            &post.occurrence,
            &part,
            &part.occurrence,
            ([0., 40., z], [65., 0., 0.]),
            false,
        )?;
        poses.push(json!({"occurrence_id":part.occurrence["id"],"translation":[0.,445.,z]}));
    }
    let assembly = s.call("assembly_document", json!({}))?;
    let solution = s.call("assembly_solution", json!({}))?;
    ensure!(
        solution["solved"] == true && array(&solution, "diagnostics")?.is_empty(),
        "Unsolved assembly: {solution}"
    );
    ensure!(
        array(&assembly, "joints")?.len() == poses.len()
            && array(&assembly["component_structure"], "occurrences")?.len() == poses.len() + 1,
        "Wrong joint/occurrence count"
    );
    for expected in &poses {
        let pose = array(&solution, "instance_body_poses")?
            .iter()
            .find(|p| p["occurrence_id"] == expected["occurrence_id"])
            .context("Missing solved pose")?;
        for axis in 0..3 {
            ensure!(
                (number(&pose["translation"][axis])? - number(&expected["translation"][axis])?)
                    .abs()
                    < 1e-5,
                "Misplaced bench part: {pose}, expected {expected}"
            );
        }
    }
    let model = s.model()?;
    ensure!(
        array(&model, "sketches")?.len() == array(&s.report, "parts")?.len(),
        "Sketch provenance count changed"
    );
    ensure!(
        !array(&model["document"]["history"], "features")?
            .iter()
            .any(|f| f["kind"] == "import_step"),
        "Bench lost native feature provenance"
    );
    s.call("solid_recompute", json!({}))?;
    ensure!(
        array(&s.call("solid_scene", json!({}))?, "errors")?.is_empty(),
        "History replay failed"
    );
    for name in [
        "native sketch/extrude provenance",
        "repeated component occurrences",
        "rigid joint solution",
        "history replay",
    ] {
        s.check(name);
    }
    if s.options.contains_key("--workshop") {
        let required: Vec<_> = s
            .tools
            .iter()
            .filter(|t| {
                t["group"].as_str().is_some_and(|g| {
                    g.starts_with("sketch/")
                        || t["mutates"] == true
                            && ["solid/build", "solid/refine", "solid/repeat", "solid/body"]
                                .contains(&g)
                })
            })
            .collect();
        let missing: Vec<_> = required
            .iter()
            .filter(|t| {
                !s.report["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["name"] == t["name"])
            })
            .map(|t| t["name"].clone())
            .collect();
        s.report["coverage"] = json!({"required":required.len(),"executed":required.len()-missing.len(),"missing":missing});
    }
    if s.session.is_some() {
        s.ui(json!({"action":"view","view":"isometric","fit":true}))?;
        if let Some(path) = s.options.get("--save").cloned() {
            s.ui(json!({"action":"file","command":"save","path":path}))?;
        }
    }
    if let Some(out) = s.options.get("--out").cloned() {
        fs::create_dir_all(&out)?;
        fs::write(
            PathBuf::from(out).join("bench.model.json"),
            serde_json::to_vec_pretty(&s.model()?)?,
        )?;
    }
    println!(
        "PASS bench: {} parts, {} occurrences, {} joints",
        array(&s.report, "parts")?.len(),
        poses.len() + 1,
        poses.len()
    );
    Ok(())
}
fn horizontal_mount(
    s: &mut Scenario,
    slat: &Part,
    part: &Part,
    occurrence: &Value,
    xy: [f64; 2],
    same: bool,
) -> Result<()> {
    s.call("assembly_create_joint",json!({"name":format!("{} mount {}",part.name,occurrence["id"]),"kind":"rigid","connector_a":connector(slat,2,same,[xy[0],xy[1],if same{35.}else{0.}])?,"connector_b":connector(part,2,true,[0.,0.,part.height])?,"flipped":same,"grounded_occurrence_id":slat.occurrence["id"],"advanced":{"connector_a_occurrence_id":slat.occurrence["id"],"connector_b_occurrence_id":occurrence["id"]}}))?;
    Ok(())
}
fn side_mount(
    s: &mut Scenario,
    parent: &Part,
    parent_occurrence: &Value,
    part: &Part,
    occurrence: &Value,
    frames: ([f64; 3], [f64; 3]),
    flipped: bool,
) -> Result<()> {
    s.call("assembly_create_joint",json!({"name":format!("{} mount {}",part.name,occurrence["id"]),"kind":"rigid","connector_a":connector(parent,1,true,frames.0)?,"connector_b":connector(part,1,flipped,frames.1)?,"flipped":flipped,"advanced":{"connector_a_occurrence_id":parent_occurrence["id"],"connector_b_occurrence_id":occurrence["id"]}}))?;
    Ok(())
}
