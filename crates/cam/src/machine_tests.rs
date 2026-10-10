use super::*;
use crate::{plan_setup, post_setup, CamPostRequestDto, Siemens828dPostConfigDto};

fn fixture(dialect: PostDialect) -> CamDocumentDto {
    let mut doc = crate::post::tests::document(dialect);
    doc.setups[0].machine = Some(CamMachineAssignmentDto::three_axis(
        doc.post_defaults.clone(),
    ));
    if dialect == PostDialect::Siemens828d {
        crate::post::tests::bind_test_names(&mut doc, &[(1, "6_MM_FLAT")]);
    }
    doc
}

fn request(post: Option<CamPostConfigDto>) -> CamPostRequestDto {
    CamPostRequestDto {
        setup_id: 1,
        post,
        program_name: None,
    }
}

#[test]
fn legacy_setup_stays_generic_and_can_plan_but_cannot_export_nc() {
    let mut doc = crate::post::tests::document(PostDialect::Siemens828d);
    doc.setups[0].machine = None;
    let json = serde_json::to_value(&doc).unwrap();
    assert!(json["setups"][0].get("machine").is_none());
    let reopened: CamDocumentDto = serde_json::from_value(json).unwrap();
    assert!(reopened.setups[0].machine.is_none());
    assert!(plan_setup(&reopened, 1).is_ok());
    assert!(post_setup(&reopened, &request(None))
        .unwrap_err()
        .to_string()
        .contains("select a machine/controller"));
}

#[test]
fn selected_target_controls_post_default_and_rejects_silent_override() {
    for dialect in [
        PostDialect::Siemens828d,
        PostDialect::Fanuc,
        PostDialect::LinuxCnc,
        PostDialect::Grbl,
    ] {
        let mut doc = fixture(dialect);
        doc.post_defaults = CamPostConfigDto::default();
        let result = post_setup(&doc, &request(None)).unwrap();
        assert_eq!(result.dialect, dialect);
        assert!(result.warnings[0].contains("profile three-axis-starter revision 1"));
        let mut change = doc.setups[0].machine.as_ref().unwrap().profile.post.clone();
        change.dialect = if dialect == PostDialect::Grbl {
            PostDialect::Fanuc
        } else {
            PostDialect::Grbl
        };
        assert!(post_setup(&doc, &request(Some(change)))
            .unwrap_err()
            .to_string()
            .contains("does not match"));
    }
}

#[test]
fn post_can_change_numbering_but_not_machine_retract_or_preload_silently() {
    let doc = fixture(PostDialect::Siemens828d);
    let profile = &doc.setups[0].machine.as_ref().unwrap().profile;
    let mut post = profile.post.clone();
    post.program_number = Some(8123);
    post.sequence_numbers = true;
    assert!(post_setup(&doc, &request(Some(post.clone()))).is_ok());
    post.siemens_828d.as_mut().unwrap().preload_next_tool = true;
    assert!(post_setup(&doc, &request(Some(post)))
        .unwrap_err()
        .to_string()
        .contains("differ from the setup snapshot"));
}

#[test]
fn machine_snapshot_roundtrips_and_metadata_does_not_change_neutral_motion() {
    let a = fixture(PostDialect::Siemens828d);
    let value = serde_json::to_value(&a).unwrap();
    assert_eq!(value["setups"][0]["machine"]["mode"], "fixed3_axis");
    let mut b: CamDocumentDto = serde_json::from_value(value).unwrap();
    assert_eq!(a, b);
    b.setups[0].machine = Some(CamMachineAssignmentDto::three_axis(CamPostConfigDto {
        dialect: PostDialect::LinuxCnc,
        ..Default::default()
    }));
    assert_eq!(plan_setup(&a, 1).unwrap(), plan_setup(&b, 1).unwrap());
    assert_eq!(motion_document(&a), motion_document(&b));
    b.tools[0].diameter += 0.1;
    assert_ne!(motion_document(&a), motion_document(&b));
}

#[test]
fn controller_brands_and_iso_modes_are_not_post_aliases() {
    for (family, language) in [
        (
            CamControllerFamily::Siemens,
            CamControllerLanguage::SiemensIso,
        ),
        (CamControllerFamily::Haas, CamControllerLanguage::FanucStyle),
        (
            CamControllerFamily::Mitsubishi,
            CamControllerLanguage::FanucStyle,
        ),
        (CamControllerFamily::Mach, CamControllerLanguage::FanucStyle),
    ] {
        let mut doc = fixture(PostDialect::Fanuc);
        let controller = &mut doc.setups[0].machine.as_mut().unwrap().profile.controller;
        controller.family = family;
        controller.language = language;
        assert!(plan_setup(&doc, 1).is_ok());
        let error = post_setup(&doc, &request(None)).unwrap_err().to_string();
        assert!(
            error.contains("No supported built-in post") || error.contains("does not match"),
            "{error}"
        );
    }
}

#[test]
fn rotary_table_and_head_topology_roundtrips_but_never_executes_as_fixed_z() {
    let mut doc = fixture(PostDialect::Siemens828d);
    let machine = doc.setups[0].machine.as_mut().unwrap();
    machine.profile.axes.extend([
        CamMachineAxisDto {
            id: "A".into(),
            kind: CamAxisKind::Rotary,
            parent_axis_id: None,
            direction: Point3Dto::new(1.0, 0.0, 0.0),
            origin: Point3Dto::new(0.0, 0.0, 0.0),
            limits: Some([-120.0, 120.0]),
        },
        CamMachineAxisDto {
            id: "C".into(),
            kind: CamAxisKind::Rotary,
            parent_axis_id: Some("A".into()),
            direction: Point3Dto::new(0.0, 0.0, 1.0),
            origin: Point3Dto::new(0.0, 0.0, 0.0),
            limits: None,
        },
    ]);
    machine.profile.channels[0]
        .axis_ids
        .extend(["A".into(), "C".into()]);
    machine.workpiece_mount_axis_id = Some("C".into());
    for mode in [
        CamMachiningMode::Fixed3Axis,
        CamMachiningMode::Indexed,
        CamMachiningMode::Simultaneous,
    ] {
        doc.setups[0].machine.as_mut().unwrap().mode = mode;
        let reopened: CamDocumentDto =
            serde_json::from_slice(&serde_json::to_vec(&doc).unwrap()).unwrap();
        assert_eq!(doc, reopened);
        assert!(doc.validate().is_ok());
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .to_string()
            .contains("not implemented"));
        assert!(post_setup(&doc, &request(None)).is_err());
    }
}

#[test]
fn mill_turn_has_explicit_spindle_roles_channels_and_bindings() {
    let mut m = fixture(PostDialect::Siemens828d)
        .setups
        .remove(0)
        .machine
        .unwrap();
    m.profile.process = CamMachineProcess::MillTurn;
    m.mode = CamMachiningMode::MillTurn;
    m.profile.spindles.push(CamMachineSpindleDto {
        id: "main-chuck".into(),
        role: CamSpindleRole::Workpiece,
        parent_axis_id: None,
    });
    m.profile.channels[0].spindle_ids.push("main-chuck".into());
    m.workpiece_spindle_id = Some("main-chuck".into());
    m.profile.channels.push(CamMachineChannelDto {
        id: "lower-turret".into(),
        axis_ids: vec!["X".into(), "Z".into()],
        spindle_ids: vec!["main-chuck".into()],
    });
    assert!(m.validate().is_ok());
    let decoded: CamMachineAssignmentDto =
        serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(decoded, m);
    assert!(m.ensure_supported_motion().is_err());
    m.tool_spindle_id = Some("main-chuck".into());
    assert!(m.validate().unwrap_err().contains("wrong role"));
}

#[test]
fn malformed_resource_graphs_and_versions_fail_closed() {
    let original = CamMachineAssignmentDto::three_axis(CamPostConfigDto::default());
    let mut m = original.clone();
    m.profile.axes[0].parent_axis_id = Some("X".into());
    assert!(m.validate().unwrap_err().contains("cycle"));
    let mut m = original.clone();
    m.profile.axes[0].parent_axis_id = Some("missing".into());
    assert!(m.validate().unwrap_err().contains("Missing"));
    let mut m = original.clone();
    m.profile.axes[0].limits = Some([2.0, 1.0]);
    assert!(m.validate().unwrap_err().contains("limits"));
    let mut m = original.clone();
    m.profile.axes[0].direction.x = 0.5;
    assert!(m.validate().unwrap_err().contains("unit direction"));
    let mut m = original.clone();
    m.profile.channels[0].axis_ids[1] = "X".into();
    assert!(m.validate().is_err());
    let mut m = original.clone();
    m.profile.channels[0].axis_ids[1] = "absent".into();
    assert!(m
        .validate()
        .unwrap_err()
        .contains("missing machine resources"));
    let mut m = original.clone();
    m.channel_id = "absent".into();
    assert!(m
        .validate()
        .unwrap_err()
        .contains("missing machine channel"));
    let mut m = original;
    m.profile.schema_version = u32::MAX;
    assert!(m.validate().unwrap_err().contains("version"));
}

fn comp_program(entry: CamCommandDto, exit_length: f64) -> CamProgramDto {
    let to = entry.endpoint().unwrap();
    CamProgramDto {
        setup_id: 1,
        name: "Compensation contract".into(),
        stats: Default::default(),
        per_operation: vec![],
        work_offsets: vec![],
        warnings: vec![],
        commands: vec![
            CamCommandDto::SectionStart {
                operation_id: 9,
                tool_id: 1,
                name: "Contour".into(),
            },
            CamCommandDto::Rapid {
                to: Point3Dto::new(0.0, 0.0, 5.0),
            },
            CamCommandDto::CutterCompensationOn { left: true },
            entry,
            CamCommandDto::Linear {
                to: Point3Dto::new(to.x + 10.0, to.y, to.z),
                feed: 100.0,
            },
            CamCommandDto::CutterCompensationOff,
            CamCommandDto::Linear {
                to: Point3Dto::new(to.x + 10.0 + exit_length, to.y, to.z),
                feed: 100.0,
            },
            CamCommandDto::SectionEnd,
        ],
    }
}

#[test]
fn controller_contract_checks_real_xy_motion_not_arcs_or_z_distance() {
    let doc = fixture(PostDialect::Siemens828d);
    let line = |x, y, z| CamCommandDto::Linear {
        to: Point3Dto::new(x, y, z),
        feed: 100.0,
    };
    let short = comp_program(line(0.2, 0.0, 5.0), 0.2);
    assert!(check_compensation_contract(&doc, &short, PostDialect::Siemens828d).is_ok());
    assert!(check_compensation_contract(&doc, &short, PostDialect::Fanuc).is_err());
    assert!(check_compensation_contract(&doc, &short, PostDialect::Grbl).is_err());
    let full = comp_program(line(3.0, 0.0, 5.0), 6.1);
    for dialect in [
        PostDialect::Siemens828d,
        PostDialect::Fanuc,
        PostDialect::LinuxCnc,
    ] {
        assert!(check_compensation_contract(&doc, &full, dialect).is_ok());
        let z_only = comp_program(line(0.0, 0.0, -25.0), 7.0);
        assert!(check_compensation_contract(&doc, &z_only, dialect).is_err());
        let mostly_z = comp_program(line(0.1, 0.0, -25.0), 7.0);
        if dialect != PostDialect::Siemens828d {
            assert!(check_compensation_contract(&doc, &mostly_z, dialect).is_err());
        }
    }
    assert!(check_compensation_contract(
        &doc,
        &comp_program(line(3.0, 0.0, 5.0), 6.0),
        PostDialect::LinuxCnc
    )
    .is_err());
    let rapid = comp_program(
        CamCommandDto::Rapid {
            to: Point3Dto::new(6.0, 0.0, 5.0),
        },
        7.0,
    );
    assert!(check_compensation_contract(&doc, &rapid, PostDialect::Siemens828d).is_err());
    let mut broken = full;
    broken.commands.remove(5);
    assert!(check_compensation_contract(&doc, &broken, PostDialect::Siemens828d).is_err());
}

#[test]
fn production_post_gate_applies_compensation_contract_to_generated_blocks() {
    let mut doc = crate::post::tests::contour_document(PostDialect::Siemens828d);
    doc.setups[0].machine = Some(CamMachineAssignmentDto::three_axis(
        doc.post_defaults.clone(),
    ));
    crate::post::tests::bind_test_names(&mut doc, &[(1, "6_MM_FLAT")]);
    for op in &mut doc.setups[0].operations {
        if let crate::CamOperationDto::Contour2d {
            lead_in, lead_out, ..
        } = op
        {
            *lead_in = 0.5;
            *lead_out = 0.5;
        }
    }
    assert!(post_setup(&doc, &request(None)).is_ok());
    doc.setups[0].machine = Some(CamMachineAssignmentDto::three_axis(CamPostConfigDto {
        dialect: PostDialect::LinuxCnc,
        ..Default::default()
    }));
    doc.tools[0].number = Some(1);
    let error = post_setup(&doc, &request(None)).unwrap_err().to_string();
    assert!(
        error.contains("Operation 1:") && error.contains("longer than 6.000"),
        "{error}"
    );
    doc.setups[0].machine = None;
    assert!(post_setup(&doc, &request(None)).is_err());
    let mut missing = CamMachineAssignmentDto::three_axis(CamPostConfigDto {
        dialect: PostDialect::Siemens828d,
        ..Default::default()
    });
    assert!(missing
        .ensure_post_matches(&missing.profile.post)
        .unwrap_err()
        .contains("explicit tool-change"));
    missing.profile.post.siemens_828d = Some(Siemens828dPostConfigDto::default());
    assert!(missing.ensure_post_matches(&missing.profile.post).is_ok());
}

#[test]
fn compensation_checks_post_rounding_in_inches_not_only_neutral_mm() {
    let mut doc = fixture(PostDialect::Fanuc);
    doc.units = crate::CamUnits::Inches;

    let program = comp_program(
        CamCommandDto::Linear {
            to: Point3Dto::new(3.0, 0.0, 5.0),
            feed: 100.0,
        },
        7.0,
    );
    assert!(check_compensation_contract(&doc, &program, PostDialect::Fanuc).is_err());
    let program = comp_program(
        CamCommandDto::Linear {
            to: Point3Dto::new(3.03, 0.0, 5.0),
            feed: 100.0,
        },
        7.0,
    );
    assert!(check_compensation_contract(&doc, &program, PostDialect::Fanuc).is_ok());
}

#[test]
fn unsupported_rest_source_is_checked_before_reusing_neutral_stock_cache() {
    let mut doc = fixture(PostDialect::Siemens828d);
    let mut rest = doc.setups[0].clone();
    rest.id = 2;
    rest.resolved_stock = crate::CamResolvedStockDto::Rest { source_setup_id: 1 };
    rest.stock_spec = crate::CamStockSpecDto::RestFromSetup { setup_id: 1 };
    rest.operations.clear();
    doc.setups.push(rest);
    assert!(ensure_setup_machines_supported(&doc, 2).is_ok());
    doc.setups[0].machine.as_mut().unwrap().mode = CamMachiningMode::Indexed;
    assert!(ensure_setup_machines_supported(&doc, 2)
        .unwrap_err()
        .to_string()
        .contains("not implemented"));
}
