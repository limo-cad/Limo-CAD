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

#[test]
fn ordered_contact_motion_evaluation_releases_live_guards_and_rejects_stale_preview() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for case in [
        "live", "pause", "stop", "draft", "seek", "hide", "tab", "contact", "study", "edit",
        "replace", "native",
    ] {
        let f = Fixture::new();
        let owner = f.owner();
        let (study, contact) =
            crate::session_bridge::native_interface::contact_stock(&f.engine, &f.bridge, &owner);
        let assembly = document(&f.engine).unwrap();
        let before = f.engine.engine_call("project_export_model", "");
        let receipt = f.bridge.native_document_receipt(&f.engine, &owner).unwrap();
        let services = NativeServices {
            engine: f.engine.clone(),
            bridge: f.bridge.clone(),
        };
        let mut app = native_viewport::interface_scene_fixture();
        crate::session_bridge::native_interface::refresh_native_model(
            &f.engine,
            app.world_mut(),
            false,
        )
        .unwrap();
        let world = app.world_mut();
        let handle = NativeInterfaceHandle::new(|| {});
        worker::install(world, services.clone(), handle.clone()).unwrap();
        let original = native_viewport::interface_view_snapshot(world).2;
        let mut state = State::default();
        state.changed(&assembly);
        state.original = Some(original.clone());
        state.started = Some((Instant::now(), 0.));
        let serial = state.serial;
        world.insert_resource(Browser {
            owner: Some(owner.clone()),
            revision: receipt.revision,
            enabled: true,
            tab: Tab::Motion,
            studies: state,
            ..default()
        });
        let capture_services = services.clone();
        let capture_owner = owner.clone();
        let adoption_owner = owner.clone();
        let (sent, ready) = std::sync::mpsc::sync_channel(1);
        worker::enqueue_motion_evaluation(world,owner.clone(),receipt.revision,json!({"study_id":study.id,"time_seconds":1.,"previous_time_seconds":0.,"enforce_contacts":true}),
            move|finished| {
                capture_services.bridge.with_native_document_receipt(&capture_services.engine,&capture_owner,|_|{capture_services.engine.with_drawing(|_|());Ok(())}).unwrap();
                crate::session_bridge::native_interface::assert_contact_stop(finished.report(),contact);
                sent.send(()).unwrap();Ok((finished,()))
            },move|world,services,result|adopt_evaluation(world,services,&adoption_owner,receipt.revision,serial,study.id.0,result.map(|(finished,())|finished)),
        ).unwrap();
        ready.recv_timeout(Duration::from_secs(10)).unwrap();
        match case {
            "pause" | "stop" | "draft" | "seek" => {
                let mut state = std::mem::take(&mut world.resource_mut::<Browser>().studies);
                let (action, input) = match case {
                    "pause" => (Action::Play, ControlInput::Click),
                    "stop" => (Action::Stop, ControlInput::Click),
                    "draft" => (
                        Action::Field(Edit::Duration),
                        ControlInput::SetValue("2".into()),
                    ),
                    _ => (Action::Time, ControlInput::SetValue("0.1".into())),
                };
                let result = reduce(
                    world,
                    &handle,
                    (&f.engine, &f.bridge, &owner),
                    receipt.revision,
                    &mut state,
                    &assembly,
                    (&action, &input),
                );
                if case == "seek" {
                    assert!(
                        result.is_err(),
                        "One pending slot must refuse a second evaluation"
                    );
                } else {
                    result.unwrap();
                }
                world.resource_mut::<Browser>().studies = state;
            }
            "hide" | "tab" => {
                cancel(world, &owner, receipt.revision).unwrap();
                let mut browser = world.resource_mut::<Browser>();
                if case == "hide" {
                    browser.set_enabled(false);
                    browser.set_enabled(true);
                } else {
                    browser.tab = Tab::Structure;
                    browser.tab = Tab::Motion;
                }
            }
            "contact" => {
                let mut changed = assembly.contact_sets[0].clone();
                changed.clearance_mm = 1.;
                parse_engine_envelope(f.engine.engine_call(
                    "assembly_update_contact_set",
                    &serde_json::to_string(&changed).unwrap(),
                ))
                .unwrap();
            }
            "study" => {
                let mut changed = study.clone();
                changed.drivers[0].law = limo_cad_sketch::MotionDriverLawDto::Motor {
                    initial_value: 8.,
                    velocity_per_second: 0.,
                    acceleration_per_second2: 0.,
                };
                parse_engine_envelope(f.engine.engine_call(
                    "assembly_update_motion_study",
                    &serde_json::to_string(&changed).unwrap(),
                ))
                .unwrap();
            }
            "edit" => {
                f.rename(&owner, "Edited during contact evaluation")
                    .unwrap();
            }
            "replace" => {
                f.bridge
                    .apply_native_mutation(&f.engine, &owner, "cad_new_project", &json!({}), || {
                        Ok(())
                    })
                    .unwrap();
            }
            "native" => {
                let model = parse_engine_envelope(before.clone()).unwrap();
                parse_engine_envelope(
                    f.engine
                        .project_load(&serde_json::to_string(model.as_str().unwrap()).unwrap()),
                )
                .unwrap();
            }
            _ => {}
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(outcome) = worker::poll(world, &services) {
                if case == "live" {
                    assert!(outcome.value.is_ok());
                    let browser = world.resource::<Browser>();
                    crate::session_bridge::native_interface::assert_contact_stop(
                        browser.studies.evaluation.as_ref().unwrap(),
                        contact,
                    );
                    assert!(browser.studies.started.is_none());
                } else {
                    assert!(outcome.value.is_err());
                    assert!(world.resource::<Browser>().studies.evaluation.is_none());
                    let (_, _, current, _) = native_viewport::interface_view(world);
                    assert!(
                        current.instance_body_poses == original.instance_body_poses,
                        "Stale contact evaluation must not change retained viewport poses"
                    );
                }
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        if !matches!(case, "contact" | "study" | "edit" | "replace") {
            assert!(
                f.engine.engine_call("project_export_model", "") == before,
                "Contact preview must preserve complete authored model"
            );
        }
    }
}
