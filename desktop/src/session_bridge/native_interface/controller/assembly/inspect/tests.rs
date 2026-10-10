use super::*;
use crate::session_bridge::native_interface::tests::Fixture;
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
