//! Cross-layer golden job for the CAM stabilization gate.
//!
//! Focused unit tests in each module remain the first line of defense. This
//! fixture deliberately crosses the persistent document, neutral planner,
//! native 828D post, and volumetric simulator so a local change cannot make
//! those layers disagree while their individual tests still pass.

use crate::model::{
    CamHoleDto, CamResolvedStockDto, CamStockSpecDto, DrillCycle, WcsOriginSpecDto,
};
use crate::post::post_setup_unchecked as post_setup;
use crate::*;

fn cutting(feed_xy: f64, feed_z: f64) -> CuttingParametersDto {
    CuttingParametersDto {
        spindle_rpm: 8_000,
        feed_xy,
        feed_z,
        coolant: CoolantMode::Flood,
    }
}

fn tool(id: u64, number: u32, name: &str, kind: CamToolKind, diameter: f64) -> CamToolDto {
    CamToolDto {
        id,
        number: Some(number),
        name: name.to_string(),
        kind,
        diameter,
        flute_length: 30.0,
        overall_length: 60.0,
        center_cutting: true,
        flute_count: if kind == CamToolKind::Drill { 2 } else { 4 },
        point_angle_degrees: (kind == CamToolKind::Drill).then_some(118.0),
        corner_radius: None,
        corner_chamfer: None,
        cutting: CuttingParametersDto::default(),
        cutting_presets: vec![],
        maximum_axial_depth: None,
        default_step_down: None,
        default_step_over: None,
    }
}

fn golden_document() -> CamDocumentDto {
    let face = CamOperationDto::Face {
        id: 1,
        name: "Face datum".to_string(),
        enabled: true,
        tool_id: 1,
        bounds: Rect2Dto {
            min: Point2Dto::new(0.0, 0.0),
            max: Point2Dto::new(60.0, 40.0),
        },
        top_z: 2.0,
        target_z: 0.0,
        step_over: 4.0,
        step_down: 1.0,
        safe_distance: 5.0,
        direction: FaceDirection::BothWays,
        clearance_z: 12.0,
        retract_z: 4.0,
        feed_height_z: 2.5,
        cutting: cutting(900.0, 240.0),
    };
    let contour = CamOperationDto::Contour2d {
        id: 2,
        name: "Outside profile".to_string(),
        enabled: true,
        tool_id: 1,
        path: vec![
            Point2Dto::new(10.0, 10.0),
            Point2Dto::new(50.0, 10.0),
            Point2Dto::new(50.0, 30.0),
            Point2Dto::new(10.0, 30.0),
        ],
        closed: true,
        top_z: 0.0,
        bottom_z: -4.0,
        step_down: 2.0,
        compensation: ContourCompensation::Outside,
        compensation_mode: CompensationMode::InControl,
        lead_in: 5.0,
        lead_out: 5.0,
        lead_arc_radius: Some(2.0),
        direction: MillingDirection::Climb,
        roughing_passes: 2,
        roughing_step_over: Some(1.0),
        finishing_pass: true,
        finish_allowance: 0.3,
        finish_feed: Some(600.0),
        spring_pass: true,
        chain_ref: None,
        clearance_z: 12.0,
        retract_z: 4.0,
        feed_height_z: 1.0,
        cutting: cutting(800.0, 200.0),
    };
    let drill = CamOperationDto::Drill {
        id: 3,
        name: "Through holes".to_string(),
        enabled: true,
        tool_id: 2,
        points: vec![Point2Dto::new(20.0, 20.0), Point2Dto::new(40.0, 20.0)],
        top_z: 0.0,
        bottom_z: -8.0,
        retract_z: 4.0,
        clearance_z: 12.0,
        feed_height_z: 1.0,
        holes: Vec::<CamHoleDto>::new(),
        drill_tip_through: true,
        breakthrough_depth: 0.5,
        cycle: DrillCycle::Drill,
        peck_depth: None,
        peck_retract: None,
        thread_pitch: None,
        floating_tap_holder: false,
        feed_out: None,
        dwell_seconds: 0.0,
        cutting: cutting(300.0, 180.0),
    };
    CamDocumentDto {
        setups: vec![CamSetupDto {
            id: 1,
            name: "Golden setup".to_string(),
            wcs: WorkCoordinateSystemDto::default(),
            wcs_origin: WcsOriginSpecDto::Explicit,
            work_offset: WorkOffset::G54,
            work_offset_count: 1,
            stock_spec: CamStockSpecDto::LegacyBox,
            resolved_stock: CamResolvedStockDto::Box,
            stock: StockBoxDto {
                min: Point3Dto::new(0.0, 0.0, -12.0),
                max: Point3Dto::new(60.0, 40.0, 2.0),
            },
            stock_model_box: None,
            body_ids: vec![],
            machine: None,
            legacy_clearance_z: None,
            legacy_retract_z: None,
            operations: vec![face, contour, drill],
        }],
        active_setup_id: Some(1),
        tools: vec![
            tool(1, 1, "EM6", CamToolKind::FlatEndMill, 6.0),
            tool(2, 2, "DRILL5_5", CamToolKind::Drill, 5.5),
        ],
        toolpath_generations: vec![],
        height_expressions: vec![],
        linking: Vec::new(),
        load_warnings: vec![],
        units: CamUnits::Millimeters,
        post_defaults: CamPostConfigDto::default(),
        next_setup_id: 2,
        next_operation_id: 4,
        next_tool_id: 3,
    }
}

fn assert_retract_then_clearance(program: &CamProgramDto, retract_z: f64, clearance_z: f64) {
    let mut section_start = None;
    for (index, command) in program.commands.iter().enumerate() {
        if matches!(command, CamCommandDto::SectionStart { .. }) {
            section_start = Some(index);
        }
        if matches!(command, CamCommandDto::SectionEnd) {
            let start = section_start.take().expect("section start");
            let exit_rapids: Vec<f64> = program.commands[start..index]
                .iter()
                .rev()
                .filter_map(|command| match command {
                    CamCommandDto::Rapid { to } => Some(to.z),
                    _ => None,
                })
                .take(2)
                .collect();
            assert_eq!(exit_rapids.len(), 2, "section has two exit rapids");
            assert!((exit_rapids[0] - clearance_z).abs() < 1.0e-9);
            assert!((exit_rapids[1] - retract_z).abs() < 1.0e-9);
        }
    }
    assert!(section_start.is_none(), "every section is closed");
}

#[test]
fn golden_job_stays_aligned_across_save_plan_post_and_simulation() {
    let mut document = golden_document();
    crate::post::tests::bind_test_names(&mut document, &[(1, "EM6"), (2, "DRILL5_5")]);
    document.validate().expect("golden document validates");

    let encoded = serde_json::to_vec(&document).expect("serialize CAM document");
    let mut reopened: CamDocumentDto = serde_json::from_slice(&encoded).expect("reopen document");
    reopened.soften_for_load();
    reopened.validate().expect("reopened document validates");
    assert!(reopened.load_warnings.is_empty());
    let program = plan_setup(&document, 1).expect("plan original");
    assert_eq!(program, plan_setup(&reopened, 1).expect("plan reopened"));

    let section_names: Vec<&str> = program
        .commands
        .iter()
        .filter_map(|command| match command {
            CamCommandDto::SectionStart { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        section_names,
        ["Face datum", "Outside profile", "Through holes"]
    );
    assert_retract_then_clearance(&program, 4.0, 12.0);
    assert!(program
        .commands
        .iter()
        .any(|command| matches!(command, CamCommandDto::CutterCompensationOn { left: true })));
    assert!(program
        .commands
        .iter()
        .any(|command| matches!(command, CamCommandDto::CutterCompensationOff)));

    crate::post::tests::bind_test_names(&mut reopened, &[(1, "EM6"), (2, "DRILL5_5")]);
    let posted = post_setup(
        &reopened,
        &CamPostRequestDto {
            setup_id: 1,
            post: Some(CamPostConfigDto {
                machine_retract_z: Some(0.0),
                tool_call_mode: crate::CamToolCallMode::Automatic,
                dialect: PostDialect::Siemens828d,
                program_number: None,
                sequence_numbers: false,
                siemens_828d: Some(Siemens828dPostConfigDto {
                    supa_retract_z: 0.0,
                    preload_next_tool: false,
                    ..Siemens828dPostConfigDto::default()
                }),
            }),
            program_name: Some("stabilization gate".to_string()),
        },
    )
    .expect("post native 828D program");
    assert_eq!(posted.program, program);
    assert!(posted.nc.starts_with("; %_N_STABILIZATION_GATE_MPF\n"));
    assert!(posted.nc.contains("G17 G710 G90 G94"));
    assert!(posted.nc.contains("G0 SUPA Z0 D0"));
    assert!(posted.nc.contains("T=\"EM6\""));
    assert!(posted.nc.contains("T=\"DRILL5_5\""));
    assert!(posted.nc.contains("G1 G41 NORM"));
    assert!(posted.nc.contains("G1 G40"));
    assert!(posted
        .nc
        .lines()
        .any(|line| line.ends_with(" M5") || line == "M5"));

    let simulation = simulate_setup(
        &reopened,
        &CamSimulationRequestDto {
            setup_id: 1,
            voxel_size: Some(0.5),
            max_voxels: None,
            stock_mesh: None,
            target: None,
            through_operation_id: None,
            completed_steps: None,
            playback_time_seconds: None,
        },
    )
    .expect("simulate golden job");
    assert!(simulation.removed_voxels > 0);
    assert!(simulation.remaining_voxels < simulation.initial_voxels);
    assert!(
        simulation.collisions.is_empty(),
        "unexpected rapid collision"
    );
    assert_eq!(simulation.through_operation_id, None);
    assert!(simulation
        .warnings
        .iter()
        .any(|warning| warning.contains("0.500 × 0.500 × 0.500 mm per cell")));
}
