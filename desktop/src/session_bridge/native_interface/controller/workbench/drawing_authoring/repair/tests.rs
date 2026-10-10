use super::super::{anchors, runtime, technical_runtime, Stamp};
use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};

/// No window or renderer is created. Real OCCT projections and the production
/// cached paper path exercise the previously unreachable broken-child repair.
#[test]
fn broken_detail_keeps_its_real_parent_pickable_and_repairs_in_one_history_entry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let owner = f.owner();
    let mutate = |operation, arguments| {
        f.bridge
            .apply_native_mutation(&f.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap()
    };
    for (op, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
        ),
        (
            "drawing_create_sheet",
            json!({"name":"Repair","format":"a4","orientation":"landscape"}),
        ),
        (
            "drawing_add_view",
            json!({"sheet_id":1,"view":{"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[90.,90.],"scale":1.}}),
        ),
    ] {
        mutate(op, args);
    }
    let mut document = f.engine.drawing_snapshot();
    let parent = document.sheets[0].views[0].clone();
    let projection = f
        .engine
        .project_sheet_view(&parent, &document.sheets[0].views)
        .unwrap();
    let mut center = anchors::endpoint_ref(&projection.anchors[0], &projection);
    center.edge_key = "missing-derived-reference".into();
    let mut detail = parent.clone();
    detail.id = document.next_view_id;
    document.next_view_id += 1;
    detail.name = "Broken detail".into();
    detail.kind = DrawingViewKind::Detail;
    detail.position = [190., 100.];
    detail.derivation = Some(DrawingViewDerivationDto::Detail {
        parent_view_id: parent.id,
        center,
        radius: 12.,
        label: "A".into(),
    });
    document.sheets[0].views.push(detail.clone());
    document.sheets[0].release.status = DrawingReleaseStatus::Released;
    document.sheets[0].release.released_revision = "A".into();
    mutate("drawing_set_document", json!(document));
    let saved = f.engine.drawing_snapshot();
    let broken_projection = f
        .engine
        .project_sheet_view(&detail, &saved.sheets[0].views)
        .unwrap();
    assert!(
        limo_cad_occt::drawing_export::detail_clip_circle(&detail, &broken_projection).is_err()
    );
    let exported =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let before = exported();
    let receipt = f.bridge.native_document_receipt(&f.engine, &owner).unwrap();
    let services = NativeServices {
        engine: f.engine.clone(),
        bridge: f.bridge.clone(),
    };
    let mut app = crate::native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let mut workbench = Workbench::default();
    workbench.refresh_owner(&owner);
    workbench.widgets.begin();
    drawing_paper::paint(
        world,
        camera,
        &services,
        &mut workbench,
        (1200., 800., 240.),
        &HashMap::new(),
    )
    .unwrap();
    workbench.widgets.finish(world);
    assert!(
        workbench.paper_key.is_none(),
        "Normal sheet must disclose the broken child instead of publishing a partial drawing"
    );
    world.insert_resource(workbench);
    let stamp = Stamp {
        owner: owner.clone(),
        revision: receipt.revision,
        sheet_id: 1,
    };
    let mut editor = Editor {
        stamp: Some(stamp.clone()),
        document: Arc::new(saved.clone()),
        tool: Some(Tool::Technical(technical::Tool::Repair)),
        ..Default::default()
    };
    select(world, &mut editor, format!("view:{}", detail.id)).unwrap();
    assert_eq!(editor.repair.view_id, parent.id);
    world.insert_resource(editor);
    assert!(runtime::repair_view(world, &saved.sheets[0], &owner, receipt.revision + 1).is_none());
    let mut foreign = owner.clone();
    foreign.epoch += 1;
    assert!(runtime::repair_view(world, &saved.sheets[0], &foreign, receipt.revision).is_none());
    let mut workbench = world.remove_resource::<Workbench>().unwrap();
    workbench.widgets.begin();
    drawing_paper::paint(
        world,
        camera,
        &services,
        &mut workbench,
        (1200., 800., 240.),
        &HashMap::new(),
    )
    .unwrap();
    workbench.widgets.finish(world);
    assert!(
        workbench.paper_key.is_some(),
        "Repair must publish the valid parent even though its child cannot resolve its clip"
    );
    assert!(workbench.widgets.entity("drawing-repair-preview").is_some());
    assert_eq!(
        drawing_paper::with_projections(world, &workbench, |p, _| p
            .keys()
            .copied()
            .collect::<Vec<_>>())
        .unwrap(),
        vec![parent.id]
    );
    let mut editor = world.remove_resource::<Editor>().unwrap();
    technical_runtime::synchronize(world, &workbench, &mut editor).unwrap();
    world.insert_resource(workbench);
    assert!(!editor.targets.is_empty());
    let original_target = editor.targets[0].clone();
    editor.targets[0].view_id = 999;
    assert!(pick(&mut editor, &Command::Anchor(0)).is_err());
    editor.targets[0] = original_target;
    technical_runtime::pick(world, &mut editor, &stamp, &Command::Anchor(0)).unwrap();
    assert_eq!(
        exported(),
        before,
        "A picked replacement is only a disposable preview"
    );
    assert!(editor.dirty());
    assert!(choose(
        world,
        &mut editor,
        &Command::RepairReference,
        &ControlInput::Click
    )
    .is_err());
    let next = apply(&editor).unwrap();
    assert_eq!(next.sheets[0].views[0], saved.sheets[0].views[0]);
    assert_eq!(next.sheets[0].annotations, saved.sheets[0].annotations);
    f.bridge
        .apply_native_mutation_at(
            &f.engine,
            &owner,
            receipt.revision,
            "drawing_set_document",
            &json!(next),
            || Ok(()),
        )
        .unwrap();
    let after = exported();
    let repaired = f.engine.drawing_snapshot();
    let repaired_projection = f
        .engine
        .project_sheet_view(&repaired.sheets[0].views[1], &repaired.sheets[0].views)
        .unwrap();
    assert!(limo_cad_occt::drawing_export::detail_clip_circle(
        &repaired.sheets[0].views[1],
        &repaired_projection,
    )
    .unwrap()
    .is_some());
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(exported(), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(exported(), after);
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &owner,
            receipt.revision,
            "drawing_set_document",
            &json!(saved),
            || Ok(())
        )
        .is_err());
    assert_eq!(exported(), after);
}
