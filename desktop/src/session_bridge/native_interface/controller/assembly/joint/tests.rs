use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

#[test]
fn joint_canvas_receipts_identify_shared_occurrences_without_committing() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    stock(&f);
    for (operation, arguments) in [
        ("assembly_duplicate_occurrence", json!({"occurrence_id":1})),
        (
            "assembly_set_occurrence_grounded",
            json!({"occurrence_id":1,"grounded":true}),
        ),
        (
            "assembly_update_occurrence",
            json!({"occurrence":{"id":3,"name":"Repeated stock","local_pose":{"translation":[80.,0.,0.],"rotation":[0.,0.,0.,1.]}}}),
        ),
    ] {
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), operation, &arguments, || Ok(()))
            .unwrap();
    }
    let services = NativeServices {
        engine: f.engine.clone(),
        bridge: f.bridge.clone(),
    };
    let owner = f.owner();
    let receipt = f.bridge.native_document_receipt(&f.engine, &owner).unwrap();
    let a = document(&f.engine).unwrap();
    let before = parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    for asynchronous in [false, true] {
        let mut app = native_viewport::interface_scene_fixture();
        crate::session_bridge::native_interface::refresh_native_model(
            &f.engine,
            app.world_mut(),
            false,
        )
        .unwrap();
        native_viewport::apply_interface_viewport(
            app.world_mut(),
            limo_cad_interface::Rect {
                x: 0.,
                y: 0.,
                width: 1280.,
                height: 720.,
            },
            1.,
        )
        .unwrap();
        let (_, mut camera, view, _) = native_viewport::interface_view(app.world());
        let original_view = view.clone();
        camera.position = [50., 5., 200.];
        camera.target = [50., 5., 10.];
        camera.up = [0., 1., 0.];
        native_viewport::apply_interface_view(
            app.world_mut(),
            &owner.document_id,
            Some(camera),
            None,
        )
        .unwrap();
        let editor = Editor {
            id: 1,
            owner: owner.clone(),
            revision: receipt.revision,
            assembly: Arc::new(a.clone()),
            form: Form::new(&a, None, UnitSystem::Mm),
            pick: Some(0),
            orientation: false,
            choice: false,
            scroll: 0.,
            max_scroll: 0.,
            error: None,
            original_view,
            original_preview: native_viewport::interface_preview_snapshot(app.world()),
            preview_revision: native_viewport::interface_preview_revision(app.world()),
        };
        app.world_mut().insert_resource(State {
            serial: 1,
            editor: Some(editor),
            ..default()
        });
        if asynchronous {
            worker::install(
                app.world_mut(),
                services.clone(),
                NativeInterfaceHandle::new(|| {}),
            )
            .unwrap();
        }
        for (slot, occurrence, point) in [("a", 1, [10., 5., 10.]), ("b", 3, [90., 5., 10.])] {
            let point =
                native_viewport::interface_world_point(app.world(), &owner.document_id, point)
                    .unwrap()
                    .unwrap();
            let mut result = canvas(app.world_mut(), &services, &owner, Some(point), true)
                .unwrap()
                .unwrap();
            if asynchronous && slot == "b" {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                loop {
                    if let Some(outcome) = worker::poll(app.world_mut(), &services) {
                        result = outcome.value.unwrap();
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Joint preview timed out"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            assert_eq!(result["handled"], true);
            assert_eq!(result["picked_connector"]["slot"], slot);
            assert_eq!(result["picked_connector"]["occurrence_id"], occurrence);
            assert_eq!(result["picked_connector"]["connector"]["body_id"], 1);
            assert_eq!(
                result["picked_connector"]["connector"]["kind"],
                "planar_face"
            );
            if slot == "a" {
                assert_eq!(result["valid"], false);
            } else {
                assert_eq!(
                    result["picked_connector"]["occurrence_name"],
                    "Repeated stock"
                );
                assert_eq!(result["solved"], true);
            }
            assert_eq!(
                parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap(),
                before,
                "Picking and previewing must preserve the model and history"
            );
        }
    }
}

#[test]
fn mechanism_drag_keeps_source_geometry_and_applies_one_reversible_joint_position() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    stock(&f);
    let a = document(&f.engine).unwrap();
    let mut form = Form::new(&a, None, UnitSystem::Mm);
    form.kind = limo_cad_sketch::JointKindDto::Slider;
    form.connectors = picked(&f, &a);
    form.coordinates[1].limited = true;
    form.coordinates[1].values[1].set_text("-20".into());
    form.coordinates[1].values[2].set_text("20".into());
    let (op, args) = form.request(&a).unwrap();
    f.bridge
        .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
        .unwrap();
    let a = document(&f.engine).unwrap();
    let scene = f.engine.viewport_snapshot().2;
    let solution = a.solve(&scene);
    let base = &solution.instance_body_poses[0];
    let moving = &solution.instance_body_poses[1];
    assert!(!a.can_drag_occurrence(base.body_id, base.occurrence_id, &scene));
    assert!(a.can_drag_occurrence(moving.body_id, moving.occurrence_id, &scene));
    let mut disabled = a.clone();
    disabled.joints[0].enabled = false;
    assert!(!disabled.can_drag_occurrence(moving.body_id, moving.occurrence_id, &scene));
    let mut rigid = a.clone();
    rigid.joints[0].kind = limo_cad_sketch::JointKindDto::Rigid;
    assert!(!rigid.can_drag_occurrence(moving.body_id, moving.occurrence_id, &scene));
    let before = parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let target = limo_cad_sketch::BodyPoseDto {
        body_id: moving.body_id,
        translation: [
            moving.translation[0],
            moving.translation[1],
            moving.translation[2] + 10.,
        ],
        rotation: moving.rotation,
    };
    let preview:limo_cad_sketch::MechanismPreviewDto=serde_json::from_value(parse_engine_envelope(f.engine.engine_call("assembly_preview_mechanism_drag",&json!({"body_id":moving.body_id,"occurrence_id":moving.occurrence_id,"target_pose":target,"maximum_iterations":12}).to_string())).unwrap()).unwrap();
    assert!(preview.solution.solved && preview.converged, "{preview:?}");
    assert_eq!(
        parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "assembly_apply_joint_motions",
            &json!({"motions":preview.joint_motions}),
            || Ok(()),
        )
        .unwrap();
    let after = parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    assert_ne!(before, after);
    assert_eq!(f.engine.viewport_snapshot().2, scene);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(
        parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap(),
        after
    );
}

pub(in super::super) fn stock(f: &Fixture) {
    for i in 0..2 {
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":i as f64*40.,"y":0.},"p2":{"x":i as f64*40.+20.,"y":10.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            ),
        ] {
            f.bridge
                .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
                .unwrap();
        }
    }
}
pub(in super::super) fn picked(f: &Fixture, a: &AssemblyDocumentDto) -> [Option<Connector>; 2] {
    let scene = f.engine.viewport_snapshot().2;
    std::array::from_fn(|i| {
        let b = &scene.bodies[i];
        let face = b
            .faces
            .iter()
            .find(|f| f.plane.is_some_and(|p| p.normal[2] > 0.99))
            .unwrap();
        let plane = face.plane.unwrap();
        let def = a
            .component_structure
            .definitions
            .iter()
            .find(|d| d.body_ids.contains(&b.id))
            .unwrap();
        let o = a
            .component_structure
            .occurrences
            .iter()
            .find(|o| o.component_id == def.id)
            .unwrap();
        let connector=serde_json::from_value(json!({"body_id":b.id,"face_id":face.id,"face_key":face.key,"frame":{"origin":plane.origin,"primary_axis":plane.normal,"secondary_axis":plane.u}})).unwrap();
        Some(Connector {
            connector,
            occurrence: o.id,
            label: o.name.clone(),
        })
    })
}
#[test]
fn all_joint_forms_solve_without_mutating_preview_and_commit_with_exact_undo() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    stock(&f);
    let export =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    for (kind, _, _) in form::KINDS {
        let a = document(&f.engine).unwrap();
        let before = export();
        let mut form = Form::new(&a, None, UnitSystem::Mm);
        form.kind = kind;
        form.connectors = picked(&f, &a);
        for (i, _) in form.axes() {
            form.coordinates[i].values[0]
                .set_text(if matches!(i, 1 | 4) { "2 mm" } else { "15 deg" }.into());
            form.coordinates[i].limited = true;
        }
        form.pitch.set_text("4 mm".into());
        let (op, args) = form.request(&a).unwrap();
        let solution: AssemblySolutionDto = serde_json::from_value(
            parse_engine_envelope(
                f.engine
                    .engine_call("assembly_preview_joint", &args.to_string()),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(solution.solved, "{kind:?}: {:?}", solution.diagnostics);
        assert_eq!(export(), before, "A preview changed the model for {kind:?}");
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
            .unwrap();
        let committed = export();
        assert_ne!(committed, before);
        let joint = document(&f.engine).unwrap().joints[0].clone();
        assert_eq!(joint.kind, kind);
        let mut edit = Form::new(&document(&f.engine).unwrap(), Some(joint), UnitSystem::Mm);
        edit.name = "Edited joint".into();
        edit.twists[0].set_text("10 deg".into());
        let (update, args) = edit.request(&document(&f.engine).unwrap()).unwrap();
        parse_engine_envelope(
            f.engine
                .engine_call("assembly_preview_joint_update", &args.to_string()),
        )
        .unwrap();
        assert_eq!(
            export(),
            committed,
            "Edit preview must be reversible without rebuilding geometry"
        );
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), update, &args, || Ok(()))
            .unwrap();
        let updated = export();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(export(), committed);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(export(), updated);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(export(), before, "Undo create {kind:?}");
    }
}
#[test]
fn joint_forms_reject_invalid_limits_names_pitch_and_same_instance() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    stock(&f);
    let a = document(&f.engine).unwrap();
    let mut form = Form::new(&a, None, UnitSystem::Cm);
    form.connectors = picked(&f, &a);
    form.kind = limo_cad_sketch::JointKindDto::Screw;
    form.pitch.set_text("0.4".into());
    assert_eq!(
        form.request(&a).unwrap().1["advanced"]["screw_pitch_mm_per_revolution"],
        4.
    );
    for value in ["NaN", "1/0", "-1", "0"] {
        form.pitch.set_text(value.into());
        assert!(form.request(&a).is_err());
    }
    form.pitch.set_text("4 mm".into());
    form.coordinates[0].limited = true;
    form.coordinates[0].values[0].set_text("180 deg".into());
    assert!(form.request(&a).is_err());
    form.coordinates[0].values[0].set_text("45 deg".into());
    assert!(form.request(&a).is_ok());
    form.name = "  ".into();
    assert!(form.request(&a).is_err());
    form.name = "Valid".into();
    form.connectors[1] = form.connectors[0].clone();
    assert!(form.request(&a).is_err());
}
