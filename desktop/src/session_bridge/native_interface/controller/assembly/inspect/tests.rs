use super::*;
use crate::session_bridge::native_interface::tests::Fixture;
#[test]
fn ordered_interference_review_releases_live_guards_and_rejects_stale_reports() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for case in [
        "overlap",
        "clearance",
        "draft",
        "hide",
        "tab",
        "edit",
        "pose",
        "replace",
        "native",
    ] {
        let f = Fixture::new();
        let owner = f.owner();
        for (op, args) in [
            (
                "sketch_begin",
                json!({"name":"Repeated stock","plane":{"type":"origin_plane","plane":"xy"}}),
            ),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":10.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Repeated stock","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}),
            ),
        ] {
            f.bridge
                .apply_native_mutation(&f.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        let body = f.engine.solid_scene_snapshot().bodies[0].id;
        let assembly = document(&f.engine).unwrap();
        let definition = assembly
            .component_structure
            .definitions
            .iter()
            .find(|d| d.body_ids.contains(&body))
            .unwrap()
            .id;
        f.bridge.apply_native_mutation(&f.engine,&owner,"assembly_create_occurrence",
            &json!({"component_id":definition,"name":"Repeated native body","local_pose":{"translation":[if case=="clearance" {12.}else{5.},0.,0.],"rotation":[0.,0.,0.,1.]}}),||Ok(())).unwrap();
        let assembly = document(&f.engine).unwrap();
        let repeated = assembly
            .component_structure
            .occurrences
            .iter()
            .find(|o| o.name == "Repeated native body")
            .unwrap()
            .id;
        let before = f.engine.engine_call("project_export_model", "");
        let receipt = f.bridge.native_document_receipt(&f.engine, &owner).unwrap();
        let services = NativeServices {
            engine: f.engine.clone(),
            bridge: f.bridge.clone(),
        };
        let mut world = World::new();
        worker::install(
            &mut world,
            services.clone(),
            NativeInterfaceHandle::new(|| {}),
        )
        .unwrap();
        world.insert_resource(Browser {
            owner: Some(owner.clone()),
            revision: receipt.revision,
            enabled: true,
            tab: Tab::Inspect,
            ..Default::default()
        });
        let capture_services = services.clone();
        let capture_owner = owner.clone();
        let adoption_owner = owner.clone();
        let (sent, ready) = std::sync::mpsc::sync_channel(1);
        worker::enqueue_interference_review(
            &mut world,
            owner.clone(),
            receipt.revision,
            json!({"occurrence_ids":[],"clearance_threshold_mm":3.}),
            move |finished| {
                capture_services
                    .bridge
                    .with_native_document_receipt(&capture_services.engine, &capture_owner, |_| {
                        capture_services.engine.with_drawing(|_| ());
                        Ok(())
                    })
                    .unwrap();
                let report = finished.report();
                assert!(report.exact);
                assert_eq!(report.pairs.len(), 1);
                let pair = &report.pairs[0];
                assert_eq!(
                    pair.body_a, pair.body_b,
                    "Repeated occurrences must use the same actual definition"
                );
                assert_ne!(pair.occurrence_a, pair.occurrence_b);
                if case == "clearance" {
                    assert!(!pair.interfering && pair.below_clearance);
                    assert!((pair.minimum_clearance_mm - 2.).abs() < 1e-6);
                } else {
                    assert!(pair.interfering && (pair.overlap_volume_mm3 - 500.).abs() < 1e-6);
                }
                let prepared = prepare_interference_report(finished)?;
                sent.send(()).unwrap();
                Ok(prepared)
            },
            move |world, services, result| {
                adopt_interference_report(
                    world,
                    services,
                    &adoption_owner,
                    receipt.revision,
                    0,
                    result,
                )
            },
        )
        .unwrap();
        ready
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        match case {
            "draft" => world.resource_mut::<Browser>().inspect.invalidate_report(),
            "hide" => {
                let mut browser = world.resource_mut::<Browser>();
                browser.set_enabled(false);
                browser.set_enabled(true);
            }
            "tab" => {
                let mut browser = world.resource_mut::<Browser>();
                browser.tab = Tab::Structure;
                browser.inspect.invalidate_report();
            }
            "edit" => {
                f.rename(&owner, "Edited during check").unwrap();
            }
            "pose" => {
                parse_engine_envelope(f.engine.engine_call("assembly_update_occurrence",
                &json!({"occurrence":{"id":repeated,"local_pose":{"translation":[20.,0.,0.],"rotation":[0.,0.,0.,1.]}}}).to_string())).unwrap();
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(outcome) = worker::poll(&mut world, &services) {
                if matches!(case, "overlap" | "clearance") {
                    assert!(outcome.value.is_ok());
                    assert!(world.resource::<Browser>().inspect.report.is_some());
                } else {
                    assert!(outcome.value.is_err());
                    assert!(world.resource::<Browser>().inspect.report.is_none());
                }
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if matches!(
            case,
            "overlap" | "clearance" | "draft" | "hide" | "tab" | "native"
        ) {
            assert!(
                f.engine.engine_call("project_export_model", "") == before,
                "Interference check must preserve complete model"
            );
        }
    }
}
#[test]
fn ordered_swept_review_releases_live_guards_and_rejects_stale_motion_reports() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for case in [
        "live",
        "stop",
        "sample",
        "study",
        "stop_draft",
        "clearance_draft",
        "hide",
        "tab",
        "edit",
        "pose",
        "replace",
        "native",
    ] {
        let f = Fixture::new();
        let owner = f.owner();
        let study = crate::session_bridge::native_interface::tests::swept_stock(
            &f.engine, &f.bridge, &owner,
        );
        let selected_study = if case == "study" {
            f.bridge
                .apply_native_mutation(
                    &f.engine,
                    &owner,
                    "assembly_create_motion_study",
                    &json!({"name":"Other actual study","duration_seconds":1.}),
                    || Ok(()),
                )
                .unwrap()
                .value["id"]
                .as_u64()
                .unwrap()
        } else {
            study.id.0
        };
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
        worker::install(world, services.clone(), NativeInterfaceHandle::new(|| {})).unwrap();
        world.insert_resource(Browser {
            owner: Some(owner.clone()),
            revision: receipt.revision,
            enabled: true,
            tab: Tab::Inspect,
            ..Default::default()
        });
        let capture_services = services.clone();
        let capture_owner = owner.clone();
        let adoption_owner = owner.clone();
        let (sent, ready) = std::sync::mpsc::sync_channel(1);
        worker::enqueue_swept_collision_review(
            world,
            owner.clone(),
            receipt.revision,
            json!({"study_id":study.id,"sample_rate_hz":4.,"clearance_threshold_mm":0.5,"stop_at_first":case=="stop"}),
            move |finished| {
                capture_services
                    .bridge
                    .with_native_document_receipt(&capture_services.engine, &capture_owner, |_| {
                        capture_services.engine.with_drawing(|_| ());
                        Ok(())
                    })
                    .unwrap();
                let report = finished.report();
                assert!(report.exact);
                assert_eq!(report.sample_count, if case=="stop" {3} else {5});
                assert_eq!(report.events.len(),1);
                let event=&report.events[0];
                assert_eq!(event.body_a,event.body_b);
                assert_ne!(event.occurrence_a,event.occurrence_b);
                assert!((event.first_time_seconds-0.5).abs()<1e-9);
                assert!((event.last_time_seconds-if case=="stop" {0.5}else{1.}).abs()<1e-9);
                assert!((event.maximum_overlap_volume_mm3-if case=="stop" {30.}else{180.}).abs()<1e-6);
                let prepared = prepare_swept_report(finished)?;
                sent.send(()).unwrap();
                Ok(prepared)
            },
            move |world, services, result| {
                adopt_swept_report(
                    world,
                    services,
                    &adoption_owner,
                    receipt.revision,
                    0,
                    result,
                )
            },
        )
        .unwrap();
        ready
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        match case {
            "sample" | "study" | "stop_draft" | "clearance_draft" => {
                let mut state = std::mem::take(&mut world.resource_mut::<Browser>().inspect);
                state.study = Some(study.id.0);
                let (action, input) = match case {
                    "sample" => (
                        Action::Field(Edit::SampleRate),
                        ControlInput::SetValue("8".into()),
                    ),
                    "study" => (
                        Action::Choice(Choice::Study),
                        ControlInput::SetValue(selected_study.to_string()),
                    ),
                    "stop_draft" => (Action::StopFirst, ControlInput::Click),
                    _ => (
                        Action::Field(Edit::Clearance),
                        ControlInput::SetValue("1".into()),
                    ),
                };
                reduce(
                    world,
                    (&f.engine, &f.bridge, &owner),
                    receipt.revision,
                    &mut state,
                    &assembly,
                    UnitSystem::Mm,
                    (&action, &input),
                )
                .unwrap();
                assert_eq!(
                    state.report_generation,
                    u64::from(case == "clearance_draft"),
                    "Swept-only options must not invalidate static review"
                );
                world.resource_mut::<Browser>().inspect = state;
            }
            "hide" => {
                let mut browser = world.resource_mut::<Browser>();
                browser.set_enabled(false);
                browser.set_enabled(true);
            }
            "tab" => {
                let mut browser = world.resource_mut::<Browser>();
                browser.tab = Tab::Structure;
                browser.inspect.invalidate_reviews();
                browser.tab = Tab::Inspect;
            }
            "edit" => {
                f.rename(&owner, "Edited during check").unwrap();
            }
            "pose" => {
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(outcome) = worker::poll(world, &services) {
                if matches!(case, "live" | "stop") {
                    assert!(outcome.value.is_ok());
                    assert!(world.resource::<Browser>().inspect.swept.is_some());
                } else {
                    assert!(outcome.value.is_err());
                    assert!(world.resource::<Browser>().inspect.swept.is_none());
                }
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if matches!(
            case,
            "live"
                | "stop"
                | "sample"
                | "study"
                | "stop_draft"
                | "clearance_draft"
                | "hide"
                | "tab"
                | "native"
        ) {
            assert!(
                f.engine.engine_call("project_export_model", "") == before,
                "Swept check must preserve complete model"
            );
        }
    }
}
#[test]
fn native_inspection_is_exact_read_only_and_contact_edits_have_exact_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    for i in 0..2 {
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":i*5,"y":0},"p2":{"x":i*5+10,"y":10},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10}}),
            ),
        ] {
            f.bridge
                .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
                .unwrap();
        }
    }
    let export =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let before = export();
    let report: InterferenceReportDto = serde_json::from_value(
        parse_engine_envelope(f.engine.engine_call(
            "assembly_interference_check",
            r#"{"clearance_threshold_mm":0}"#,
        ))
        .unwrap(),
    )
    .unwrap();
    assert!(
        report.exact,
        "The native query must never silently use mesh bounds"
    );
    assert_eq!(report.pairs.len(), 1);
    assert!(report.pairs[0].interfering);
    assert!((report.pairs[0].overlap_volume_mm3 - 500.).abs() < 1e-6);
    assert_eq!(export(), before);
    let a = document(&f.engine).unwrap();
    let poses = f.engine.viewport_snapshot().9;
    let ca = &poses[0];
    let cb = &poses[1];
    let create = json!({"name":"Physical stop","occurrence_a":ca.occurrence_id,"body_a":ca.body_id,"occurrence_b":cb.occurrence_id,"body_b":cb.body_id,"clearance_mm":0.2,"stop_motion":true});
    assert!(a.contact_sets.is_empty());
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "assembly_create_contact_set",
            &create,
            || Ok(()),
        )
        .unwrap();
    let made = export();
    let mut contact = document(&f.engine).unwrap().contact_sets[0].clone();
    contact.name = "Renamed contact".into();
    contact.enabled = false;
    contact.stop_motion = false;
    contact.clearance_mm = 0.4;
    for (operation, args) in [
        (
            "assembly_update_contact_set",
            serde_json::to_value(&contact).unwrap(),
        ),
        (
            "assembly_delete_contact_set",
            json!({"contact_id":contact.id}),
        ),
    ] {
        let before = export();
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), operation, &args, || Ok(()))
            .unwrap();
        let after = export();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(export(), before);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(export(), after);
    }
    for _ in 0..2 {
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
    }
    assert_eq!(export(), made);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(), before);
    parse_engine_envelope(f.engine.engine_call(
        "assembly_create_motion_study",
        r#"{"name":"Static sample","duration_seconds":0.1}"#,
    ))
    .unwrap();
    let swept: SweptCollisionReportDto = serde_json::from_value(
        parse_engine_envelope(f.engine.engine_call(
            "assembly_swept_collision_check",
            r#"{"study_id":1,"sample_rate_hz":10,"stop_at_first":true}"#,
        ))
        .unwrap(),
    )
    .unwrap();
    assert!(swept.exact);
    assert!(!swept.events.is_empty());
}
#[test]
fn inspection_inputs_validate_units_nonfinite_values_and_sample_bounds() {
    let mut state = State::default();
    state.clearance.set_text("0.1".into());
    assert_eq!(state.clearance(UnitSystem::Cm).unwrap(), 1.);
    for invalid in ["-1", "NaN", "1/0"] {
        state.clearance.set_text(invalid.into());
        assert!(state.clearance(UnitSystem::Mm).is_err());
    }
    for invalid in ["0", "241", "NaN", "inf", "bad"] {
        state.sample_rate = invalid.into();
        assert!(state.sample_rate().is_err());
    }
    for value in ["1", "120", "240"] {
        state.sample_rate = value.into();
        assert!(state.sample_rate().is_ok());
    }
}
