use super::*;
use crate::session_bridge::native_interface::tests::Fixture;
#[test]
fn contact_playback_stops_at_the_earliest_contact_regardless_of_record_order() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    joint::tests::stock(&f);
    let a = document(&f.engine).unwrap();
    let mut form = crate::native_forms::joint::Form::new(&a, None, UnitSystem::Mm);
    form.kind = limo_cad_sketch::JointKindDto::Slider;
    form.connectors = joint::tests::picked(&f, &a);
    let (op, args) = form.request(&a).unwrap();
    f.bridge
        .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
        .unwrap();
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "assembly_create_motion_study",
            &json!({"name":"Approach","duration_seconds":4.}),
            || Ok(()),
        )
        .unwrap();
    let a = document(&f.engine).unwrap();
    let mut form = Form::new(a.motion_studies[0].clone());
    form.add_driver(&a).unwrap();
    let driver = &mut form.drivers[0];
    driver.is_motor = true;
    driver.motor = ["40".into(), "-10".into(), "0".into()];
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "assembly_update_motion_study",
            &json!(form.value(&a).unwrap()),
            || Ok(()),
        )
        .unwrap();
    let poses = f.engine.viewport_snapshot().9;
    for clearance in [5., 15.] {
        f.bridge.apply_native_mutation(&f.engine,&f.owner(),"assembly_create_contact_set",&json!({"name":format!("Stop {clearance}"),"occurrence_a":poses[0].occurrence_id,"body_a":poses[0].body_id,"occurrence_b":poses[1].occurrence_id,"body_b":poses[1].body_id,"clearance_mm":clearance,"stop_motion":true}),||Ok(())).unwrap();
    }
    let query = || {
        parse_engine_envelope(f.engine.engine_call(
            "assembly_evaluate_motion_study",
            r#"{"study_id":1,"time_seconds":4,"previous_time_seconds":0}"#,
        ))
        .unwrap()
    };
    let result = query();
    assert_eq!(result["stopped_by_contact"], 2, "{result}");
    assert!(
        result["sample"]["time_seconds"].as_f64().unwrap() < 3.5,
        "{result}"
    );
    assert_eq!(query(), result, "Contact playback must be deterministic");
}
#[test]
fn study_forms_validate_drivers_resize_keys_and_keep_exact_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    joint::tests::stock(&f);
    let a = document(&f.engine).unwrap();
    let mut joint = crate::native_forms::joint::Form::new(&a, None, UnitSystem::Mm);
    joint.kind = limo_cad_sketch::JointKindDto::Slider;
    joint.connectors = joint::tests::picked(&f, &a);
    let (op, args) = joint.request(&a).unwrap();
    f.bridge
        .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
        .unwrap();
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "assembly_create_motion_study",
            &json!({"name":"Slide","duration_seconds":2.}),
            || Ok(()),
        )
        .unwrap();
    let a = document(&f.engine).unwrap();
    let mut form = Form::new(a.motion_studies[0].clone());
    form.add_driver(&a).unwrap();
    assert!(form.add_driver(&a).is_err());
    let id = form.drivers[0].record.id.0;
    form.driver_mut(id).unwrap().keys[1].value = "10".into();
    form.duration = "4".into();
    let resized = form.value(&a).unwrap();
    if let limo_cad_sketch::MotionDriverLawDto::Keyframes { keyframes } = &resized.drivers[0].law {
        assert_eq!(keyframes[1].time_seconds, 4.);
    } else {
        panic!("Not keyframes");
    }
    form.driver_mut(id).unwrap().keys[1].time = "0".into();
    assert!(form.value(&a).is_err());
    form.driver_mut(id).unwrap().keys[1].time = "2".into();
    form.driver_mut(id).unwrap().is_motor = true;
    form.driver_mut(id).unwrap().motor = ["0".into(), "2 + 3".into(), "0".into()];
    let study = form.value(&a).unwrap();
    let export =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let before = export();
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "assembly_update_motion_study",
            &json!(study),
            || Ok(()),
        )
        .unwrap();
    let saved = export();
    let evaluation: MotionStudyEvaluationDto = serde_json::from_value(
        parse_engine_envelope(f.engine.engine_call(
            "assembly_evaluate_motion_study",
            &json!({"study_id":study.id,"time_seconds":1.}).to_string(),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(evaluation.sample.joint_motions[0].linear_offset_mm, 5.);
    assert!(evaluation.sample.solution.solved);
    assert_eq!(export(), saved);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(), saved);
    let a = document(&f.engine).unwrap();
    let mut form = Form::new(a.motion_studies[0].clone());
    for value in ["NaN", "1 / 0"] {
        form.driver_mut(id).unwrap().motor[1] = value.into();
        assert!(form.value(&a).is_err());
    }
}
