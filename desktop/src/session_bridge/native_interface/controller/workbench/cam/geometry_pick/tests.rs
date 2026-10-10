use super::*;
use crate::native_viewport::{ViewportCamera, ViewportModel};
use limo_cad_solid::SolidSceneDto;
#[path = "hole_tests.rs"]
mod holes_tests;
#[path = "linking_tests.rs"]
mod linking_tests;
#[path = "point_tests.rs"]
mod point_tests;

fn scene() -> SolidSceneDto {
    let vertices = [
        [2., 2., -1.],
        [20., 2., -1.],
        [20., 10., -1.],
        [2., 10., -1.],
    ];
    let edges = ["rim", "other"]
        .into_iter()
        .enumerate()
        .flat_map(|(loop_index, name)| {
            (0..4).map(move |index| {
                let points = [vertices[index], vertices[(index + 1) % 4]].map(
                    |point| json!({"x":point[0],"y":point[1]+loop_index as f64*30.,"z":point[2]}),
                );
                json!({"id":loop_index*4+index+1,"key":format!("{name}-{index}"),"points":points})
            })
        })
        .collect::<Vec<_>>();
    serde_json::from_value(
        json!({"bodies":[{"id":11,"name":"Pick block","feature_id":1,
        "mesh":{"positions":[0.,0.,-6.,30.,0.,-6.,30.,50.,-6.],"normals":[],"indices":[0,1,2]},
        "faces":[],"edges":edges}],"errors":[]}),
    )
    .unwrap()
}

fn cam(kind: &str) -> CamDocumentDto {
    let record = json!({"kind":kind,"id":7,"name":"Picked path","tool_id":5,"enabled":true,
        "top_z":-1.,"bottom_z":-4.,"step_down":1.,"step_over":2.,"clearance_z":8.,"retract_z":2.,"feed_height_z":1.,
        "cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.},
        "path":[{"x":2.,"y":2.},{"x":20.,"y":2.},{"x":20.,"y":10.},{"x":2.,"y":10.}],
        "outline":[{"x":2.,"y":2.},{"x":20.,"y":2.},{"x":20.,"y":10.},{"x":2.,"y":10.}],
        "closed":true,"compensation":"outside","chamfer_width":0.5,"wall_side":"inside","tip_offset":0.2,"additional_chains":[]});
    let mut cam: CamDocumentDto = serde_json::from_value(json!({
        "setups":[{"id":3,"name":"Top setup","body_ids":[11],"work_offset":"g55",
            "stock":{"min":{"x":0.,"y":0.,"z":-12.},"max":{"x":30.,"y":50.,"z":0.}},"operations":[record]}],
        "active_setup_id":3,"tools":[{"id":5,"number":1,"name":"End mill","kind":"flat_end_mill",
            "diameter":6.,"flute_length":20.,"overall_length":50.,"flute_count":4}],
        "next_setup_id":4,"next_tool_id":6,"next_operation_id":8})).unwrap();
    cam.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
        Default::default(),
    ));
    if kind == "chamfer2d" {
        cam.tools[0].kind = limo_cad_cam::CamToolKind::ChamferMill;
        cam.tools[0].point_angle_degrees = Some(90.);
    }
    cam.validate_for_editing().unwrap();
    cam
}

fn edit(draft: &mut Draft, cam: &CamDocumentDto, path: &str, value: &str) {
    assert!(
        draft.fields.iter().any(|field| field.path == path),
        "missing {path}"
    );
    form::set(draft, path, value);
    operation_editor::changed(draft, cam, path).unwrap();
}
fn draft(cam: &CamDocumentDto, closed: bool) -> Draft {
    let mut draft = Draft::new(cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, cam, &scene(), &[]).unwrap();
    edit(&mut draft, cam, "/native/ui/operation_section", "geometry");
    edit(&mut draft, cam, "/native/geometry/chains/0/source", "model");
    if closed {
        edit(&mut draft, cam, "/native/geometry/chains/0/mode", "closed");
    }
    draft
}
fn sketch() -> limo_cad_sketch::SketchDto {
    let points = [[2., 2.], [20., 2.], [20., 10.], [2., 10.]];
    let entities = (0..4)
        .map(|index| {
            json!({"kind":"line","id":index+1,
        "start_id":index+10,"end_id":(index+1)%4+10,
        "start":{"x":points[index][0],"y":points[index][1]},
        "end":{"x":points[(index+1)%4][0],"y":points[(index+1)%4][1]},
        "fully_defined":true})
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"name":"Pick profile","plane":{"type":"origin_plane","plane":"xy"},
        "basis":{"origin":[0.,0.,-1.],"u":[1.,0.,0.],"v":[0.,1.,0.],"normal":[0.,0.,1.]},
        "entities":entities,"constraints":[],"dimensions":[],"dimension_style":limo_cad_core::DimensionStyle::default(),
        "dof":{"value":0,"fully_defined":true},"can_undo":false,"can_redo":false})).unwrap()
}
fn draft_with_sketch(cam: &CamDocumentDto) -> Draft {
    let mut draft = Draft::new(cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, cam, &scene(), &[sketch()]).unwrap();
    edit(&mut draft, cam, "/native/ui/operation_section", "geometry");
    draft
}
fn fields(draft: &Draft) -> Vec<(String, String, String, Option<Vec<ChoiceOption>>)> {
    draft
        .fields
        .iter()
        .map(|field| {
            (
                field.path.clone(),
                field.original.clone(),
                field.text.clone(),
                field.options.clone(),
            )
        })
        .collect()
}
fn keys(name: &str) -> Vec<String> {
    (0..4)
        .map(|index| format!("edge:11:{name}-{index}"))
        .collect()
}
fn resolve(draft: &Draft, seed: &str) -> limo_cad_core::edge_chain::Chain {
    let selection = picking::snapshot(draft).unwrap();
    let context = operation_editor::geometry(draft).unwrap();
    limo_cad_sketch::resolve_edge_chain(
        &context.scene,
        &context.sketches,
        &limo_cad_sketch::EdgeChainRequest {
            source: selection.source,
            body_ids: context.setup.body_ids.clone(),
            normal: Some(context.setup.wcs.z_axis),
            keys: vec![seed.into()],
            mode: ChainMode::Closed,
            reversed: selection.reversed,
        },
    )
    .unwrap()
}

#[test]
fn closed_pick_stages_complete_keys_and_existing_apply_preserves_the_document() {
    let cam = cam("contour2d");
    let before = serde_json::to_value(&cam).unwrap();
    let mut draft = draft(&cam, true);
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/reversed",
        "true",
    );
    let resolved = resolve(&draft, "edge:11:rim-2");
    assert_eq!(resolved.keys.len(), 4);
    let snapshot = picking::snapshot(&draft).unwrap();
    let staged = picking::stage(&mut draft, &cam, &snapshot, resolved.keys.clone(), false).unwrap();
    assert_eq!(
        staged.mode,
        ChainMode::Closed,
        "automatic picking remains available for the next click"
    );
    assert_eq!(staged.keys, resolved.keys);
    assert!(draft.dirty());
    assert_eq!(
        serde_json::to_value(&cam).unwrap(),
        before,
        "picking is not a document mutation"
    );

    let next = draft.edited(&cam).unwrap();
    let mut expected = before;
    expected["setups"][0]["operations"][0]["chain_ref"] =
        json!({"source":"model","keys":resolved.keys,"reversed":true});
    expected["setups"][0]["operations"][0]["path"] = json!(resolved
        .points
        .iter()
        .map(|point| json!({"x":point[0],"y":point[1]}))
        .collect::<Vec<_>>());
    assert_eq!(
        serde_json::to_value(&next).unwrap(),
        expected,
        "the existing Apply preserves every unrelated shared field"
    );
    let mut reopened = Draft::new(&next, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut reopened, &next, &scene(), &[]).unwrap();
    edit(
        &mut reopened,
        &next,
        "/native/ui/operation_section",
        "geometry",
    );
    let selected = picking::snapshot(&reopened).unwrap();
    assert_eq!(selected.keys, staged.keys);
    assert_eq!(
        selected.mode,
        ChainMode::Manual,
        "saved complete keys reopen as exact selection, never multiple automatic seeds"
    );
    assert_eq!(
        serde_json::to_value(reopened.edited(&next).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn maximum_loop_toggle_compares_all_identities_without_order_or_duplicate_ambiguity() {
    let keys: Vec<_> = (0..20_000)
        .map(|index| format!("edge:11:segment-{index}"))
        .collect();
    let mut reordered = keys.clone();
    reordered.reverse();
    assert!(same_loop_keys(&keys, &reordered));
    reordered[10_000] = "edge:11:different-loop-edge".into();
    assert!(!same_loop_keys(&keys, &reordered));
    reordered[10_000] = reordered[10_001].clone();
    assert!(!same_loop_keys(&keys, &reordered));
    assert!(!same_loop_keys(&keys, &keys[..19_999]));
}

#[test]
fn repeated_picks_and_edge_paging_retain_one_catalog_without_losing_draft_intent() {
    let cam = cam("contour2d");
    let mut draft = draft(&cam, false);
    let all = keys("rim");
    let catalog = operation_editor::geometry(&draft)
        .unwrap()
        .model_options
        .clone();
    let key_prefix = "/native/geometry/chains/0/keys/";
    for count in 1..=all.len() {
        let before = picking::snapshot(&draft).unwrap();
        picking::stage(&mut draft, &cam, &before, all[..count].to_vec(), true).unwrap();
        let active = format!("{key_prefix}{}", count - 1);
        let catalogs: Vec<_> = draft
            .fields
            .iter()
            .filter(|field| field.path.contains("/keys/") && field.options.is_some())
            .collect();
        assert_eq!(
            catalogs.len(),
            1,
            "Prior pick fields must not retain whole catalog clones"
        );
        assert_eq!(catalogs[0].path, active);
        assert_eq!(catalogs[0].options.as_ref(), Some(&catalog));
        assert_eq!(picking::snapshot(&draft).unwrap().keys, all[..count]);
    }
    let expected = draft.edited(&cam).unwrap();
    let values: Vec<_> = draft
        .fields
        .iter()
        .map(|f| (f.path.clone(), f.original.clone(), f.text.clone()))
        .collect();
    let cursor = "/native/ui/geometry_edgechains/0";
    for slot in 1..=all.len() {
        edit(&mut draft, &cam, cursor, &slot.to_string());
        let catalogs: Vec<_> = draft
            .fields
            .iter()
            .filter(|field| field.path.contains("/keys/") && field.options.is_some())
            .collect();
        assert_eq!(catalogs.len(), 1);
        assert_eq!(catalogs[0].path, format!("{key_prefix}{}", slot - 1));
        assert_eq!(catalogs[0].options.as_ref(), Some(&catalog));
        assert_eq!(
            draft.edited(&cam).unwrap(),
            expected,
            "Paging a key is presentation only"
        );
        assert!(draft.dirty());
    }
    for (path, original, text) in values.iter().filter(|(path, _, _)| path != cursor) {
        let current = draft
            .fields
            .iter()
            .find(|field| &field.path == path)
            .unwrap();
        assert_eq!((&current.original, &current.text), (original, text));
    }
    let before = picking::snapshot(&draft).unwrap();
    picking::stage(&mut draft, &cam, &before, vec![], true).unwrap();
    assert!(draft
        .fields
        .iter()
        .filter(|field| field.path.contains("/keys/"))
        .all(|field| field.options.is_none()));
}

#[test]
fn stale_source_mode_selection_and_direction_cannot_publish_a_resolved_pick() {
    let cam = cam("contour2d");
    for change in ["source", "mode", "selection", "direction", "section"] {
        let mut draft = draft(&cam, true);
        let expected = picking::snapshot(&draft).unwrap();
        match change {
            "source" => edit(
                &mut draft,
                &cam,
                "/native/geometry/chains/0/source",
                "sketch",
            ),
            "mode" => edit(&mut draft, &cam, "/native/geometry/chains/0/mode", "manual"),
            "selection" => draft.selection = Selection::Operation(999),
            "direction" => edit(
                &mut draft,
                &cam,
                "/native/geometry/chains/0/reversed",
                "true",
            ),
            _ => edit(
                &mut draft,
                &cam,
                "/native/ui/operation_section",
                "parameters",
            ),
        }
        let before = fields(&draft);
        assert!(
            picking::stage(&mut draft, &cam, &expected, keys("rim"), false).is_err(),
            "{change}"
        );
        assert_eq!(
            fields(&draft),
            before,
            "stale {change} modified the current draft"
        );
    }
}

#[test]
fn source_switch_keeps_independent_pending_keys_modes_and_direction_without_replaying_old_results()
{
    let cam = cam("contour2d");
    let original = serde_json::to_value(&cam).unwrap();
    let mut draft = draft_with_sketch(&cam);
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/points/0/x",
        "unfinished-point",
    );
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "model",
    );
    edit(&mut draft, &cam, "/native/geometry/chains/0/mode", "closed");
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/reversed",
        "true",
    );
    let resolved = resolve(&draft, "edge:11:rim-2");
    let selection = picking::snapshot(&draft).unwrap();
    let model = picking::stage(&mut draft, &cam, &selection, resolved.keys.clone(), false).unwrap();

    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "sketch",
    );
    let empty = picking::snapshot(&draft).unwrap();
    assert_eq!(empty.source, ChainSource::Sketch);
    assert!(
        empty.keys.is_empty(),
        "first use must not inherit model keys"
    );
    assert_eq!(empty.mode, ChainMode::Manual);
    assert!(!empty.reversed);
    let before = fields(&draft);
    assert!(picking::stage(&mut draft, &cam, &model, model.keys.clone(), false).is_err());
    assert_eq!(fields(&draft), before);
    let first = picking::stage(
        &mut draft,
        &cam,
        &empty,
        vec!["sketch:Pick profile:1".into()],
        true,
    )
    .unwrap();
    assert_eq!(
        first.keys,
        vec!["sketch:Pick profile:1"],
        "individual picking works immediately after the source change"
    );
    let sketch_keys = (1..=4)
        .map(|id| format!("sketch:Pick profile:{id}"))
        .collect::<Vec<_>>();
    picking::stage(&mut draft, &cam, &first, sketch_keys.clone(), true).unwrap();
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/reversed",
        "true",
    );
    let sketch_selection = picking::snapshot(&draft).unwrap();

    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "model",
    );
    let restored = picking::snapshot(&draft).unwrap();
    assert_eq!(restored.keys, model.keys);
    assert_eq!(restored.mode, ChainMode::Closed);
    assert!(restored.reversed);
    assert!(restored.source_epoch > model.source_epoch);
    let before = fields(&draft);
    assert!(
        picking::stage(&mut draft, &cam, &model, keys("other"), false).is_err(),
        "round-tripping to the same keys cannot authorize a pre-switch worker result"
    );
    assert_eq!(fields(&draft), before);
    let applied = serde_json::to_value(draft.edited(&cam).unwrap()).unwrap();
    assert_eq!(
        applied["setups"][0]["operations"][0]["chain_ref"],
        json!({"source":"model","keys":model.keys,"reversed":true}),
        "the restored complete-loop marker must keep Apply in exact-key mode"
    );

    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "sketch",
    );
    let restored_sketch = picking::snapshot(&draft).unwrap();
    assert_eq!(restored_sketch.keys, sketch_keys);
    assert_eq!(restored_sketch.mode, sketch_selection.mode);
    assert_eq!(restored_sketch.reversed, sketch_selection.reversed);
    assert!(restored_sketch.source_epoch > sketch_selection.source_epoch);
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "manual",
    );
    assert_eq!(
        form::text(&draft, "/native/geometry/chains/0/points/0/x").unwrap(),
        "unfinished-point"
    );
    assert!(
        draft.edited(&cam).is_err(),
        "source changes must not sanitize unfinished manual geometry"
    );
    assert_eq!(serde_json::to_value(&cam).unwrap(), original);
}

#[test]
fn saved_source_round_trip_is_clean_and_keeps_lazy_original_keys_and_missing_references() {
    let mut cam = cam("contour2d");
    let mut record = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    record["chain_ref"] = json!({"source":"model","keys":keys("rim"),"reversed":false});
    cam.setups[0].operations[0] = serde_json::from_value(record).unwrap();
    let mut draft = draft_with_sketch(&cam);
    assert!(!draft.dirty());
    assert!(!draft
        .fields
        .iter()
        .any(|field| field.path == "/native/geometry/chains/0/keys/3"));
    let saved = picking::snapshot(&draft).unwrap();
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "sketch",
    );
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "model",
    );
    assert_eq!(picking::snapshot(&draft).unwrap().keys, saved.keys);
    assert!(
        !draft.dirty(),
        "source caches must not alter original form baselines"
    );
    assert_eq!(draft.edited(&cam).unwrap(), cam);

    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/keys/0",
        "edge:11:missing",
    );
    let broken = picking::snapshot(&draft).unwrap();
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "sketch",
    );
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "model",
    );
    assert_eq!(picking::snapshot(&draft).unwrap().keys, broken.keys);
    assert!(
        draft.edited(&cam).is_err(),
        "unresolved same-source keys must not be silently removed or replaced"
    );
    let field = draft
        .fields
        .iter()
        .find(|field| field.path == "/native/geometry/chains/0/keys/0")
        .unwrap();
    assert!(field
        .options
        .as_ref()
        .unwrap()
        .iter()
        .any(|option| option.value == "edge:11:missing" && option.disabled));
}

#[test]
fn saved_unmaterialized_chamfer_chain_owns_its_keys_and_keeps_independent_values() {
    let mut cam = cam("chamfer2d");
    let mut record = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    record["additional_chains"] = json!([{"path":[{"x":2.,"y":2.},{"x":20.,"y":2.},{"x":20.,"y":10.},{"x":2.,"y":10.}],
        "closed":true,"chain_ref":{"source":"model","keys":keys("rim"),"reversed":true},
        "modeled_chamfer":null,"top_z":-3.,"chamfer_width":0.123456789,"wall_side":"outside"}]);
    cam.setups[0].operations[0] = serde_json::from_value(record.clone()).unwrap();
    cam.validate_for_editing().unwrap();
    let mut draft = draft(&cam, true);
    assert!(!draft
        .fields
        .iter()
        .any(|field| field.path.starts_with("/native/geometry/chains/1/")));
    let selection = picking::snapshot(&draft).unwrap();
    assert_eq!(selection.claimed, keys("rim"));
    let before = fields(&draft);
    assert!(
        picking::stage(&mut draft, &cam, &selection, keys("rim"), false)
            .unwrap_err()
            .contains("another chain")
    );
    assert_eq!(fields(&draft), before);

    let resolved = resolve(&draft, "edge:11:other-1");
    picking::stage(&mut draft, &cam, &selection, resolved.keys, false).unwrap();
    let next = draft.edited(&cam).unwrap();
    let actual = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(actual["additional_chains"], record["additional_chains"]);
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.height_expressions, cam.height_expressions);
    assert_eq!(next.toolpath_generations, cam.toolpath_generations);
}

#[test]
fn rejected_keys_are_atomic_and_successful_picks_preserve_other_invalid_raw_fields() {
    let cam = cam("contour2d");
    let mut draft = draft(&cam, true);
    edit(&mut draft, &cam, "/cutting/feed_xy", "unfinished-");
    let selection = picking::snapshot(&draft).unwrap();
    for rejected in [
        vec!["edge:11:missing".into()],
        vec!["edge:11:rim-0".into(); 20_001],
    ] {
        let before = fields(&draft);
        assert!(picking::stage(&mut draft, &cam, &selection, rejected, false).is_err());
        assert_eq!(fields(&draft), before);
    }
    picking::stage(&mut draft, &cam, &selection, keys("rim"), false).unwrap();
    assert_eq!(
        form::text(&draft, "/cutting/feed_xy").unwrap(),
        "unfinished-"
    );
    assert!(
        draft.edited(&cam).is_err(),
        "a pick cannot sanitize another unfinished field"
    );
}

fn owner() -> DocumentContext {
    DocumentContext {
        window_id: "pick-window".into(),
        document_id: "pick-document".into(),
        epoch: 1,
    }
}
fn session(selection: SelectionState) -> Session {
    Session {
        receipt: DocumentReceipt {
            owner: owner(),
            revision: 1,
        },
        selection,
        candidates: vec![],
        loaded: true,
        projection: None,
        projected: vec![],
        hover: None,
        hover_keys: vec![],
        hover_resolved: false,
        pending: None,
        clicks: VecDeque::new(),
        captured: false,
        direction: None,
        transforms: HashMap::new(),
        cursor: None,
        individual: false,
    }
}

#[test]
fn individual_click_queue_toggles_exact_keys_and_pocket_apply_rejects_an_open_chain() {
    let cam = cam("pocket2d");
    let draft = draft(&cam, true);
    let mut session = session(picking::snapshot(&draft).unwrap());
    let context = operation_editor::geometry(&draft).unwrap().clone();
    let mut worker = worker::start(
        context,
        session.selection.clone(),
        HashMap::new(),
        NativeInterfaceHandle::new(|| {}),
    )
    .unwrap();
    let mut world = World::new();
    world.insert_resource(Editor {
        cam: cam.clone(),
        draft: Some(draft),
        ..default()
    });
    session.clicks.push_back(("edge:11:rim-0".into(), true));
    pump(&mut world, &mut session, &worker).unwrap();
    assert_eq!(session.selection.mode, ChainMode::Manual);
    assert_eq!(session.selection.keys, vec!["edge:11:rim-0"]);
    assert!(world
        .resource::<Editor>()
        .draft
        .as_ref()
        .unwrap()
        .edited(&cam)
        .is_err());
    session.clicks.push_back(("edge:11:rim-0".into(), true));
    pump(&mut world, &mut session, &worker).unwrap();
    assert!(session.selection.keys.is_empty());
    assert!(world
        .resource::<Editor>()
        .draft
        .as_ref()
        .unwrap()
        .edited(&cam)
        .is_err());
    session
        .clicks
        .extend(keys("rim").into_iter().map(|key| (key, true)));
    pump(&mut world, &mut session, &worker).unwrap();
    let next = world
        .resource::<Editor>()
        .draft
        .as_ref()
        .unwrap()
        .edited(&cam)
        .unwrap();
    let record = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(record["chain_ref"]["keys"], json!(keys("rim")));
    assert_eq!(
        record["outline"],
        serde_json::to_value(&cam.setups[0].operations[0]).unwrap()["outline"]
    );
    assert_eq!(
        world.resource::<Editor>().cam,
        cam,
        "the queue only stages draft changes"
    );
    worker.cancel();
}

#[test]
fn explicit_closed_mode_after_an_individual_pick_re_resolves_the_seed() {
    let cam = cam("pocket2d");
    let mut draft = draft(&cam, false);
    let selection = picking::snapshot(&draft).unwrap();
    picking::stage(
        &mut draft,
        &cam,
        &selection,
        vec!["edge:11:rim-0".into()],
        true,
    )
    .unwrap();
    assert!(
        draft.edited(&cam).is_err(),
        "one manually picked open edge cannot bound a pocket"
    );
    edit(&mut draft, &cam, "/native/geometry/chains/0/mode", "closed");
    let next = draft.edited(&cam).unwrap();
    let record = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(
        record["chain_ref"]["keys"].as_array().unwrap().len(),
        4,
        "an old full-selection receipt must not override the user's explicit seed mode"
    );
}

fn candidate(points: Vec<[f64; 3]>, closed: bool, planar: bool) -> worker::Candidate {
    worker::Candidate {
        key: "candidate".into(),
        points,
        closed,
        planar,
    }
}
fn projected(points: &[[f32; 2]]) -> hit::Projected {
    hit::Projected {
        depths: vec![100.; points.len()],
        points: points
            .iter()
            .map(|point| Some(Vec2::from_array(*point)))
            .collect(),
    }
}

#[test]
fn overlapping_silhouettes_pick_the_front_edge_in_either_source_order() {
    let candidates = vec![
        candidate(vec![[0.; 3]; 2], false, true),
        candidate(vec![[0.; 3]; 2], false, true),
    ];
    for front in [0, 1] {
        let mut projection = vec![
            projected(&[[0., 0.], [100., 0.]]),
            projected(&[[0., 0.], [100., 0.]]),
        ];
        projection[front].depths = vec![50.; 2];
        assert_eq!(
            hit::closest(&candidates, &projection, Vec2::new(50., 0.), true, false),
            Some(front)
        );
    }
}

#[test]
fn projected_hits_include_the_closing_segment_threshold_and_planar_tie_preference() {
    let triangle = candidate(vec![[0.; 3]; 3], true, true);
    assert_eq!(
        hit::closest(
            &[triangle],
            &[projected(&[[0., 0.], [100., 0.], [100., 100.]])],
            Vec2::new(50., 50.),
            true,
            false
        ),
        Some(0)
    );
    let open = candidate(vec![[0.; 3]; 3], false, true);
    assert_eq!(
        hit::closest(
            &[open],
            &[projected(&[[0., 0.], [100., 0.], [100., 100.]])],
            Vec2::new(50., 50.),
            true,
            false
        ),
        None
    );
    let candidates = vec![candidate(vec![[0.; 3]; 2], false, true)];
    let projection = vec![projected(&[[0., 0.], [100., 0.]])];
    assert_eq!(
        hit::closest(&candidates, &projection, Vec2::new(50., 10.), false, false),
        Some(0)
    );
    assert_eq!(
        hit::closest(
            &candidates,
            &projection,
            Vec2::new(50., 10.01),
            false,
            false
        ),
        None
    );
    let candidates = vec![
        candidate(vec![[0.; 3]; 2], false, true),
        candidate(vec![[0.; 3]; 2], false, false),
    ];
    let projection = vec![
        projected(&[[0., 0.], [100., 0.]]),
        projected(&[[0., 0.], [100., 0.]]),
    ];
    assert_eq!(
        hit::closest(&candidates, &projection, Vec2::new(50., 0.), true, false),
        Some(0)
    );
    assert_eq!(
        hit::closest(&candidates, &projection, Vec2::new(50., 0.), true, true),
        Some(1)
    );
    assert_eq!(
        hit::closest(&candidates, &projection, Vec2::new(50., 0.), false, false),
        Some(1)
    );
    assert_eq!(hit::distance(Vec2::new(3., 4.), Vec2::ZERO, Vec2::ZERO), 5.);
}

fn projection_app(owner: &DocumentContext, bounds: InterfaceRect) -> App {
    let mut app = native_viewport::interface_scene_fixture();
    native_viewport::apply_interface_model(
        app.world_mut(),
        ViewportModel {
            session_id: owner.document_id.clone(),
            geometry_revision: 1,
            body_poses: std::sync::Arc::default(),
            instance_body_poses: std::sync::Arc::default(),
            document: std::sync::Arc::new(limo_cad_native_engine::NativeViewportDocument {
                scene: std::sync::Arc::new(SolidSceneDto::default()),
                active_sketch: None,
                finished_sketches: vec![],
                datum_planes: vec![],
                profile_catalog: vec![],
                body_appearances: vec![],
                ..Default::default()
            }),
        },
    )
    .unwrap();
    native_viewport::apply_interface_viewport(app.world_mut(), bounds, 2.).unwrap();
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        Some(ViewportCamera {
            position: [0., 0., 100.],
            target: [0., 0., 0.],
            up: [0., 1., 0.],
            vertical_fov_degrees: 45.,
        }),
        None,
    )
    .unwrap();
    app
}
fn publish(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    owner: DocumentContext,
    bounds: InterfaceRect,
) {
    handle
        .present(InterfaceFrame {
            context: owner,
            client: InterfaceRect {
                x: 0.,
                y: 0.,
                width: 1360.,
                height: 860.,
            },
            surface: bounds,
            canvases: vec![limo_cad_interface::Canvas {
                name: "viewport".into(),
                bounds,
            }],
            surfaces: vec![],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    interface_shell::tests::publish_layout_once(world, handle.clone());
}

#[test]
fn actual_camera_projection_uses_canvas_origin_and_rebuilds_only_after_camera_changes() {
    let owner = owner();
    let bounds = InterfaceRect {
        x: 240.,
        y: 120.,
        width: 1120.,
        height: 640.,
    };
    let mut app = projection_app(&owner, bounds);
    let handle = NativeInterfaceHandle::new(|| {});
    publish(app.world_mut(), &handle, owner.clone(), bounds);
    let cam = cam("contour2d");
    let mut session = session(picking::snapshot(&draft(&cam, true)).unwrap());
    session.candidates = vec![candidate(
        vec![[-10., -10., 0.], [10., -10., 0.], [10., 10., 0.]],
        true,
        true,
    )];
    project(app.world(), &handle, &mut session).unwrap();
    let center = Vec2::new(800., 440.);
    assert_eq!(
        hit::closest(&session.candidates, &session.projected, center, true, false),
        Some(0)
    );
    let saved = session.projected[0].points.clone();
    let allocation = session.projected[0].points.as_ptr();
    project(app.world(), &handle, &mut session).unwrap();
    assert_eq!(
        session.projected[0].points.as_ptr(),
        allocation,
        "idle frames must reuse projected data"
    );
    assert_eq!(session.projected[0].points, saved);
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        Some(ViewportCamera {
            position: [20., 0., 100.],
            target: [20., 0., 0.],
            up: [0., 1., 0.],
            vertical_fov_degrees: 45.,
        }),
        None,
    )
    .unwrap();
    project(app.world(), &handle, &mut session).unwrap();
    assert_ne!(session.projected[0].points, saved);
    assert_eq!(
        hit::closest(&session.candidates, &session.projected, center, true, false),
        None
    );
    session.receipt.owner.document_id = "retired".into();
    assert!(project(app.world(), &handle, &mut session).is_err());
}

#[test]
fn worker_candidates_are_bounded_and_apply_body_transforms_without_changing_planarity() {
    let cam = cam("contour2d");
    let draft = draft(&cam, true);
    let selection = picking::snapshot(&draft).unwrap();
    let context = operation_editor::geometry(&draft).unwrap().clone();
    let transforms = HashMap::from([(11, Transform::from_translation(Vec3::new(10., 20., 30.)))]);
    let candidates = worker::candidates(&context, &selection, &transforms).unwrap();
    assert_eq!(candidates.len(), 8);
    assert_eq!(candidates[0].points[0], [12., 22., 29.]);
    assert!(candidates.iter().all(|candidate| candidate.planar));

    let mut oversized = context.clone();
    oversized
        .model_options
        .resize(20_001, context.model_options[0].clone());
    assert!(worker::candidates(&oversized, &selection, &HashMap::new())
        .err()
        .unwrap()
        .contains("20000"));
    let mut too_many_points = context.clone();
    let edge = &mut Arc::make_mut(&mut too_many_points.scene).bodies[0].edges[0];
    edge.points = vec![edge.points[0]; worker::MAX_POINTS + 1];
    edge.points.last_mut().unwrap().x += 1.;
    assert!(
        worker::candidates(&too_many_points, &selection, &HashMap::new())
            .err()
            .unwrap()
            .contains("point budget")
    );
    let mut invalid = context.clone();
    Arc::make_mut(&mut invalid.scene).bodies[0].edges[0].points[1].z = f64::NAN;
    assert!(worker::candidates(&invalid, &selection, &HashMap::new()).is_err());
    let mut overflow = context.clone();
    Arc::make_mut(&mut overflow.scene).bodies[0].edges[0].points[1].x = f64::MAX;
    assert!(
        worker::candidates(&overflow, &selection, &HashMap::new()).is_err(),
        "finite f64 values that overflow the renderer's f32 coordinates must be rejected"
    );
    for translation in [Vec3::splat(f32::NAN), Vec3::splat(f32::INFINITY)] {
        let transforms = HashMap::from([(11, Transform::from_translation(translation))]);
        assert!(
            worker::candidates(&context, &selection, &transforms).is_err(),
            "transformed preview points must remain finite"
        );
    }
}

#[test]
fn published_picker_escape_and_retired_owner_leave_engine_and_history_unchanged() {
    use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
    use bevy::input::keyboard::{KeyCode, KeyboardInput};
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let cam = cam("contour2d");
    let draft = draft(&cam, true);
    let original_fields = fields(&draft);
    let bounds = InterfaceRect {
        x: 240.,
        y: 120.,
        width: 1120.,
        height: 640.,
    };
    let mut app = projection_app(&owner, bounds);
    let handle = NativeInterfaceHandle::new(|| {});
    publish(app.world_mut(), &handle, owner.clone(), bounds);
    app.world_mut()
        .insert_resource(super::super::super::Workbench {
            owner: Some(owner.clone()),
            workspace: Workspace::Cam,
            ..default()
        });
    let editor = Editor {
        owner: Some(owner.clone()),
        revision: receipt.revision,
        cam,
        tab: Tab::Toolpaths,
        draft: Some(draft),
        ..default()
    };
    toggle(app.world_mut(), &handle, &receipt, &editor).unwrap();
    app.world_mut().insert_resource(editor);
    assert!(active(app.world()));
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let selected = app.world().resource::<State>().session.as_ref().unwrap();
    current(app.world(), &handle, &services, selected).unwrap();

    use crate::session_bridge::native_interface::controller;
    app.init_resource::<Messages<NativeHostInput>>();
    controller::worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    let mut controller = controller::Controller::new(
        owner.window_id.clone(),
        None,
        Arc::new(AtomicBool::new(false)),
    );
    controller.synchronized = Some((owner.clone(), receipt.revision));
    for action in ["inspect", "capture"] {
        let session_id = fixture
            .bridge
            .session_id_for_window(&owner.window_id)
            .unwrap()
            .unwrap();
        let id = format!("988-{}", if action == "inspect" { 1 } else { 2 });
        let directory = crate::session_bridge::session_root()
            .join(&session_id)
            .join("controls");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{id}.request.json")),
            json!({"id":id,"expires_ms":now_ms()+30_000,"ui":{"action":action}}).to_string(),
        )
        .unwrap();
        controller::worker::enqueue_control_poll(app.world_mut(), owner.clone(), id.clone())
            .unwrap();
        controller.polled_control = Some(controller::PolledControl {
            owner: owner.clone(),
            session: session_id,
            id,
            interface_only: true,
        });
        controller::maintain_busy_window(app.world_mut(), &handle, &mut controller).unwrap();
        assert!(active(app.world()), "{action} claim cancelled the picker");
        let cursor = Vec2::new(260., 140.);
        for pressed in [true, false] {
            app.world_mut().write_message(NativeHostInput {
                ui_scale: 1.,
                context: Some(owner.clone()),
                cursor: Some(cursor),
                modifiers: default(),
                consumed: false,
                actions: vec![],
                event: WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                    window: Entity::PLACEHOLDER,
                    button: MouseButton::Left,
                    state: if pressed {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                }),
            });
        }
        controller::maintain_busy_window(app.world_mut(), &handle, &mut controller).unwrap();
        assert!(active(app.world()));
        assert_eq!(
            controller.deferred_pointer.as_ref().unwrap().events.len(),
            2
        );
        assert!(controller::take_deferred_pointer_input(
            app.world_mut(),
            &handle,
            &services,
            &mut controller
        )
        .unwrap()
        .is_empty());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let outcome = loop {
            if let Some(outcome) = controller::worker::poll(app.world_mut(), &services) {
                break outcome;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            outcome.value.unwrap()["control_request"]["ui"]["action"],
            action
        );
        controller.polled_control = None;
        let events = controller::take_deferred_pointer_input(
            app.world_mut(),
            &handle,
            &services,
            &mut controller,
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        for event in events {
            input(app.world_mut(), &handle, &services, &event).unwrap();
        }
        assert!(active(app.world()));
        assert!(
            !app.world()
                .resource::<State>()
                .session
                .as_ref()
                .unwrap()
                .captured
        );
        assert_eq!(
            fields(app.world().resource::<Editor>().draft.as_ref().unwrap()),
            original_fields
        );
    }
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );

    let escape = NativeHostInput {
        ui_scale: 1.,
        context: Some(owner.clone()),
        cursor: None,
        modifiers: default(),
        consumed: false,
        actions: vec![],
        event: WindowEvent::KeyboardInput(KeyboardInput {
            window: Entity::PLACEHOLDER,
            logical_key: Key::Escape,
            key_code: KeyCode::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
        }),
    };
    assert!(input(app.world_mut(), &handle, &services, &escape).unwrap());
    assert!(!active(app.world()));
    assert_eq!(
        fields(app.world().resource::<Editor>().draft.as_ref().unwrap()),
        original_fields
    );
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap(),
        receipt
    );
    assert!(
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
            .is_err(),
        "opening/cancelling the picker cannot create Undo history"
    );

    for lifecycle in [
        WindowEvent::WindowFocused(bevy::window::WindowFocused {
            window: Entity::PLACEHOLDER,
            focused: false,
        }),
        WindowEvent::WindowResized(bevy::window::WindowResized {
            window: Entity::PLACEHOLDER,
            width: 1440.,
            height: 900.,
        }),
        WindowEvent::WindowScaleFactorChanged(bevy::window::WindowScaleFactorChanged {
            window: Entity::PLACEHOLDER,
            scale_factor: 2.,
        }),
    ] {
        let mut pending = session(
            picking::snapshot(app.world().resource::<Editor>().draft.as_ref().unwrap()).unwrap(),
        );
        pending.receipt = receipt.clone();
        pending.captured = true;
        pending.clicks.push_back(("edge:11:rim-0".into(), false));
        pending.pending = Some(Pending {
            key: "edge:11:rim-1".into(),
            click: true,
        });
        app.world_mut().resource_mut::<State>().session = Some(pending);
        assert!(settled(app.world()).is_err());
        let event = NativeHostInput {
            ui_scale: 1.,
            context: None,
            event: lifecycle,
            ..escape.clone()
        };
        assert!(!input(app.world_mut(), &handle, &services, &event).unwrap());
        assert!(!active(app.world()));
        assert!(settled(app.world()).is_ok());
        assert_eq!(
            fields(app.world().resource::<Editor>().draft.as_ref().unwrap()),
            original_fields
        );
        assert_eq!(
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
            before
        );
    }

    let mut stale = session(
        picking::snapshot(app.world().resource::<Editor>().draft.as_ref().unwrap()).unwrap(),
    );
    stale.receipt = receipt.clone();
    let mut replacement = owner.clone();
    replacement.epoch += 1;
    publish(app.world_mut(), &handle, replacement, bounds);
    assert!(
        current(app.world(), &handle, &services, &stale).is_err(),
        "a frame replacement retires the entire old pick session"
    );
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
}
