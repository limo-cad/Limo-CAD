use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

#[test]
fn every_moving_joint_previews_without_editing_and_saves_with_exact_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    joint::tests::stock(&f);
    let export =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    for (kind, _, _) in crate::native_forms::joint::KINDS {
        let a = document(&f.engine).unwrap();
        let mut form = Form::new(&a, None, UnitSystem::Mm);
        form.kind = kind;
        form.connectors = joint::tests::picked(&f, &a);
        let (op, args) = form.request(&a).unwrap();
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
            .unwrap();
        let a = document(&f.engine).unwrap();
        let before = export();
        let mut state = State {
            selected: Some(a.joints[0].id.0),
            ..default()
        };
        state.changed(&a);
        let form = state.form.as_mut().unwrap();
        let axes = form.axes();
        if !axes.is_empty() {
            for (i, _) in axes {
                form.coordinates[i].values[0]
                    .set_text(if matches!(i, 1 | 4) { "3 mm" } else { "20 deg" }.into());
            }
            let args = json!({"motion":state.motion().unwrap()});
            let preview: AssemblySolutionDto = serde_json::from_value(
                parse_engine_envelope(
                    f.engine
                        .engine_call("assembly_preview_joint_coordinates", &args.to_string()),
                )
                .unwrap(),
            )
            .unwrap();
            assert!(preview.solved, "{kind:?}: {:?}", preview.diagnostics);
            assert_eq!(export(), before, "{kind:?} preview changed source geometry");
            f.bridge
                .apply_native_mutation(
                    &f.engine,
                    &f.owner(),
                    "assembly_set_joint_coordinates",
                    &args,
                    || Ok(()),
                )
                .unwrap();
            let after = export();
            assert_ne!(after, before);
            f.bridge
                .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
                .unwrap();
            assert_eq!(export(), before, "{kind:?} undo");
            f.bridge
                .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
                .unwrap();
            assert_eq!(export(), after, "{kind:?} redo");
            f.bridge
                .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
                .unwrap();
        }
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
    }
}

#[test]
fn motion_limits_and_invalid_expressions_never_reach_preview() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    joint::tests::stock(&f);
    let a = document(&f.engine).unwrap();
    let mut form = Form::new(&a, None, UnitSystem::Mm);
    form.kind = limo_cad_sketch::JointKindDto::Revolute;
    form.connectors = joint::tests::picked(&f, &a);
    form.coordinates[0].limited = true;
    form.coordinates[0].values[1].set_text("-25 deg".into());
    form.coordinates[0].values[2].set_text("25 deg".into());
    let (op, args) = form.request(&a).unwrap();
    f.bridge
        .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
        .unwrap();
    let a = document(&f.engine).unwrap();
    let mut s = State {
        selected: Some(a.joints[0].id.0),
        ..default()
    };
    s.changed(&a);
    for value in ["NaN", "1 / 0", "26 deg", "-26 deg"] {
        s.form.as_mut().unwrap().coordinates[0].values[0].set_text(value.into());
        assert!(s.motion().is_err(), "{value}");
    }
    for value in ["25 deg", "-25 deg", "10 + 5"] {
        s.form.as_mut().unwrap().coordinates[0].values[0].set_text(value.into());
        assert!(s.motion().is_ok(), "{value}");
    }
}
