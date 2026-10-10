use super::*;
use crate::gcode::{simulate_gcode, CamGcodeDialectDto};
use crate::model::{
    CamPostConfigDto, CamResolvedStockDto, CamSetupDto, CamStockPlacementDto, CamStockShape,
    CamStockSpecDto, CamToolDto, CamToolKind, CuttingParametersDto, StockBoxDto, WcsOriginSpecDto,
    WorkCoordinateSystemDto, WorkOffset,
};

fn document() -> CamDocumentDto {
    let mut document = CamDocumentDto {
        setups: vec![CamSetupDto {
            id: 1,
            name: "NC workpiece".into(),
            work_offset: WorkOffset::G54,
            work_offset_count: 1,
            wcs: WorkCoordinateSystemDto::default(),
            wcs_origin: WcsOriginSpecDto::Explicit,
            stock_spec: CamStockSpecDto::Fixed {
                shape: CamStockShape::Box,
                size: Point3Dto::new(20., 20., 8.),
                placement: CamStockPlacementDto::default(),
            },
            resolved_stock: CamResolvedStockDto::Box,
            stock: StockBoxDto {
                min: Point3Dto::new(-10., -10., -8.),
                max: Point3Dto::new(10., 10., 0.),
            },
            stock_model_box: None,
            body_ids: vec![],
            machine: None,
            legacy_clearance_z: None,
            legacy_retract_z: None,
            operations: vec![],
        }],
        tools: vec![CamToolDto {
            id: 1,
            number: Some(3),
            name: "6 mm flat".into(),
            kind: CamToolKind::FlatEndMill,
            diameter: 6.,
            flute_length: 20.,
            overall_length: 55.,
            center_cutting: true,
            flute_count: 2,
            point_angle_degrees: None,
            corner_radius: None,
            corner_chamfer: None,
            cutting: CuttingParametersDto::default(),
            cutting_presets: vec![],
            maximum_axial_depth: None,
            default_step_down: None,
            default_step_over: None,
        }],
        active_setup_id: Some(1),
        next_setup_id: 2,
        next_tool_id: 2,
        ..CamDocumentDto::default()
    };
    let mut machine = crate::CamMachineAssignmentDto::three_axis(CamPostConfigDto::default());
    machine.tool_calls = vec![crate::CamMachineToolBindingDto {
        tool_id: 1,
        call: crate::CamMachineToolCallDto::Number { number: 3 },
    }];
    document.setups[0].machine = Some(machine);
    document.validate().unwrap();
    document
}

fn request(source: &str) -> CamGcodeSimulationRequestDto {
    CamGcodeSimulationRequestDto {
        setup_id: 1,
        source: source.into(),
        file_name: Some("playback.nc".into()),
        dialect: CamGcodeDialectDto::Iso,
        voxel_size: Some(0.5),
        max_voxels: Some(100_000),
        stock_mesh: None,
        target: None,
        completed_steps: None,
    }
}

fn face() -> crate::CamOperationDto {
    serde_json::from_value(serde_json::json!({
        "kind":"face", "id":1, "name":"Different generated path", "enabled":true,
        "tool_id":1, "bounds":{"min":{"x":-9.,"y":-9.},"max":{"x":9.,"y":9.}},
        "top_z":0.,"target_z":-1.,"step_over":3.,"step_down":1.,"safe_distance":5.,
        "clearance_z":5.,"retract_z":2.,"feed_height_z":1.,
        "cutting":{"spindle_rpm":5000,"feed_xy":600.,"feed_z":300.,"coolant":"off"}
    }))
    .unwrap()
}

fn stock_mesh() -> CamStockMeshDto {
    CamStockMeshDto {
        positions: vec![
            -10., -10., -8., 10., -10., -8., 10., 10., -8., -10., 10., -8., -10., -10., 0., 10.,
            -10., 0., 10., 10., 0., -10., 10., 0.,
        ],
        indices: vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7,
            6, 3, 0, 4, 3, 4, 7,
        ],
    }
}

const LINEAR: &str = "G21 G90 G94\nT3 M6\nG0 X-6 Y0 Z2\nG1 Z-2 F300\nG1 X6 F600\nM30";

#[test]
fn nc_playback_without_operations_retains_source_lines_and_warnings() {
    let document = document();
    assert!(document.setups[0].operations.is_empty());
    let full = simulate_gcode(&document, &request(LINEAR)).unwrap();
    assert!(full.removed_voxels > 0);
    let mut input = request(LINEAR);
    input.completed_steps = Some(0);
    let mut player = CamPlayback::from_gcode(document, input, 0., None).unwrap();
    assert_eq!(player.timeline, full.steps);
    assert!(player
        .timeline
        .iter()
        .any(|step| step.source_line == Some(5)));
    assert_eq!(player.metadata.source, CamSimulationSourceDto::GCode);
    assert_eq!(player.metadata.warnings, full.warnings);
    assert!(player
        .metadata
        .warnings
        .iter()
        .any(|warning| warning.contains("workpiece-only")));
    let end = player.sample(full.estimated_seconds, None).unwrap();
    assert_eq!(end.remaining_voxels, full.remaining_voxels);
    assert_eq!(end.stock_mesh, full.stock_mesh);
    assert!(end.steps.is_empty());
    assert_eq!(end.source, CamSimulationSourceDto::GCode);
}

#[test]
fn nc_preparation_runs_verification_once_and_does_not_reuse_cam_stage_stock() {
    let mut document = document();
    document.setups[0].operations = vec![face()];
    document.next_operation_id = 2;
    let mut input = request(LINEAR);
    input.target = Some(CamSimulationTargetDto {
        cache_key: Some("nc-playback-stage-isolation".into()),
        meshes: vec![stock_mesh()],
        tolerance_mm: 0.1,
    });
    let cam_request = CamSimulationRequestDto {
        setup_id: input.setup_id,
        voxel_size: input.voxel_size,
        max_voxels: input.max_voxels,
        stock_mesh: None,
        target: input.target.clone(),
        through_operation_id: None,
        completed_steps: None,
        playback_time_seconds: None,
    };
    let generated = simulate_setup(&document, &cam_request).unwrap();
    let before = MESH_EXTRACTIONS.with(|count| count.get());
    let expected = simulate_gcode(&document, &input).unwrap();
    let extractions = MESH_EXTRACTIONS.with(|count| count.get()) - before;
    assert_ne!(generated.remaining_voxels, expected.remaining_voxels);
    let before = MESH_EXTRACTIONS.with(|count| count.get());
    let (mut player, complete) =
        CamPlayback::prepare_gcode(document.clone(), input.clone(), 0., None).unwrap();
    assert_eq!(
        MESH_EXTRACTIONS.with(|count| count.get()) - before,
        extractions
    );
    assert_eq!(complete, expected);
    assert!(complete.comparison.is_some());
    assert!(player.metadata.comparison.is_none());
    assert!(player.metadata.collisions.is_empty());
    assert!(player.metadata.steps.is_empty());
    assert!(player.metadata.stock_mesh.is_none());
    let initial = player.sample(0., None).unwrap();
    assert_eq!(initial.remaining_voxels, expected.initial_voxels);
    for (index, step) in expected.steps.iter().enumerate() {
        let mut boundary = input.clone();
        boundary.completed_steps = Some(index + 1);
        let independent = simulate_gcode(&document, &boundary).unwrap();
        let sample = player.sample(step.cumulative_seconds, None).unwrap();
        assert_eq!(sample.remaining_voxels, independent.remaining_voxels);
        assert_eq!(sample.source, CamSimulationSourceDto::GCode);
    }
    assert_eq!(
        player.sample(player.end, None).unwrap().remaining_voxels,
        expected.remaining_voxels
    );
}

#[test]
fn nc_playback_preserves_modeled_and_prior_setup_incoming_stock() {
    let mut modeled = document();
    modeled.setups[0].stock_spec = CamStockSpecDto::ModelBody { body_id: 9 };
    modeled.setups[0].resolved_stock = CamResolvedStockDto::ModelBody { body_id: 9 };
    let mut input = request(LINEAR);
    input.stock_mesh = Some(stock_mesh());
    let complete = simulate_gcode(&modeled, &input).unwrap();
    let mut player = CamPlayback::from_gcode(modeled, input, 0., None).unwrap();
    assert_eq!(
        player.sample(0., None).unwrap().remaining_voxels,
        complete.initial_voxels
    );
    assert_eq!(
        player.sample(player.end, None).unwrap().remaining_voxels,
        complete.remaining_voxels
    );

    let mut rest = document();
    let mut second = rest.setups[0].clone();
    second.id = 2;
    second.name = "NC on prior stock".into();
    second.stock_spec = CamStockSpecDto::RestFromSetup { setup_id: 1 };
    second.resolved_stock = CamResolvedStockDto::Rest { source_setup_id: 1 };
    rest.setups[0].operations = vec![face()];
    rest.setups.push(second);
    rest.active_setup_id = Some(2);
    rest.next_setup_id = 3;
    rest.next_operation_id = 2;
    let mut input = request(LINEAR);
    input.setup_id = 2;
    let complete = simulate_gcode(&rest, &input).unwrap();
    assert!(complete.initial_voxels < 40 * 40 * 16);
    let mut player = CamPlayback::from_gcode(rest, input, 0., None).unwrap();
    let initial = player.sample(0., None).unwrap();
    assert_eq!(initial.remaining_voxels, complete.initial_voxels);
    assert_eq!(
        player.sample(player.end, None).unwrap().remaining_voxels,
        complete.remaining_voxels
    );
    assert_eq!(
        player.sample(0., None).unwrap().stock_mesh,
        initial.stock_mesh
    );
}

#[test]
fn nc_linear_partial_sweep_matches_stopped_nc_and_rewinds_exactly() {
    let document = document();
    let full = simulate_gcode(&document, &request(LINEAR)).unwrap();
    let cut = full
        .steps
        .iter()
        .find(|step| step.source_line == Some(5))
        .unwrap();
    let time = cut.cumulative_seconds - cut.duration_seconds * 0.5;
    let stopped = simulate_gcode(&document, &request(&LINEAR.replace("G1 X6", "G1 X0"))).unwrap();
    let mut player = CamPlayback::from_gcode(document, request(LINEAR), 0., None).unwrap();
    let initial = player.sample(0., None).unwrap();
    let middle = player.sample(time, None).unwrap();
    assert_eq!(middle.remaining_voxels, stopped.remaining_voxels);
    let actual = middle.stock_mesh.as_ref().unwrap();
    let expected = stopped.stock_mesh.as_ref().unwrap();
    assert_eq!(actual.triangle_count, expected.triangle_count);
    assert_eq!(actual.positions, expected.positions);
    assert_eq!(actual.normals.len(), expected.normals.len());
    for (index, (a, b)) in actual.normals.iter().zip(&expected.normals).enumerate() {
        assert!((a - b).abs() <= 1.0e-12, "normal {index}: {a} != {b}");
    }
    assert!(middle.remaining_voxels < initial.remaining_voxels);
    assert!(middle.remaining_voxels > full.remaining_voxels);
    assert!(player.sample(time, None).unwrap().stock_mesh.is_none());
    let end = player.sample(full.estimated_seconds, None).unwrap();
    assert_eq!(end.remaining_voxels, full.remaining_voxels);
    assert_eq!(end.stock_mesh, full.stock_mesh);
    let backward = player.sample(time, None).unwrap();
    assert_eq!(backward.remaining_voxels, middle.remaining_voxels);
    assert_eq!(backward.stock_mesh, middle.stock_mesh);
    let restart = player.sample(0., None).unwrap();
    assert_eq!(restart.remaining_voxels, initial.remaining_voxels);
    assert_eq!(restart.stock_mesh, initial.stock_mesh);
}

#[test]
fn nc_arc_partial_sweep_uses_physical_curve_and_source_line() {
    let document = document();
    let source = LINEAR.replace("G1 X6 F600", "G2 X6 Y0 I6 J0 F600");
    let full = simulate_gcode(&document, &request(&source)).unwrap();
    let arc = full
        .steps
        .iter()
        .find(|step| step.source_line == Some(5))
        .unwrap();
    assert_eq!(arc.kind, CamSimulationStepKind::Circular);
    let time = arc.cumulative_seconds - arc.duration_seconds * 0.5;
    let stopped =
        simulate_gcode(&document, &request(&source.replace("G2 X6 Y0", "G2 X0 Y6"))).unwrap();
    let mut player = CamPlayback::from_gcode(document, request(&source), 0., None).unwrap();
    let partial = player.sample(time, None).unwrap();
    assert_eq!(partial.remaining_voxels, stopped.remaining_voxels);
    assert_eq!(partial.stock_mesh, stopped.stock_mesh);
    assert_eq!(
        player
            .sample(full.estimated_seconds, None)
            .unwrap()
            .remaining_voxels,
        full.remaining_voxels
    );
}

#[test]
fn nc_cancellation_rejects_preparation_and_preserves_unstarted_sample() {
    let document = document();
    let cancel = CamSimulationCancellation::default();
    cancel.cancel();
    let error =
        simulate_gcode_with_cancellation(&document, &request(LINEAR), Some(&cancel)).unwrap_err();
    assert!(error.0.contains("superseded"));
    assert!(CamPlayback::from_gcode(document.clone(), request(LINEAR), 0., Some(&cancel)).is_err());
    let mut player = CamPlayback::from_gcode(document, request(LINEAR), 0., None).unwrap();
    let initial = player.stock.occupied.clone();
    assert!(player
        .sample(player.end, Some(&cancel))
        .unwrap_err()
        .0
        .contains("superseded"));
    assert_eq!(player.stock.occupied, initial);
    assert_eq!(player.completed, 0);
    let complete = player.sample(player.end, None).unwrap();
    assert!(complete.removed_voxels > 0);
}

#[test]
fn nc_playback_validates_scope_times_and_propagates_parser_and_stock_errors() {
    let document = document();
    for start in [-1., f64::NAN, f64::INFINITY] {
        assert!(CamPlayback::from_gcode(document.clone(), request(LINEAR), start, None).is_err());
    }
    let mut player =
        CamPlayback::from_gcode(document.clone(), request(LINEAR), f64::MAX, None).unwrap();
    assert_eq!(player.start, player.end);
    for time in [-1., 0., f64::NAN, f64::INFINITY] {
        assert!(player.sample(time, None).is_err());
    }
    assert!(player.sample(player.end, None).is_ok());
    let mut input = request(LINEAR);
    input.setup_id = 42;
    assert!(simulate_gcode_with_cancellation(&document, &input, None)
        .unwrap_err()
        .0
        .contains("42"));
    input = request(&LINEAR.replace("G1 X6", "G1 X6 A90"));
    assert!(simulate_gcode_with_cancellation(&document, &input, None)
        .unwrap_err()
        .0
        .contains("A-axis"));
    input = request(&LINEAR.replace("T3", "T999"));
    assert!(CamPlayback::from_gcode(document.clone(), input, 0., None).is_err());
    input = request(LINEAR);
    input.voxel_size = Some(f64::NAN);
    assert!(CamPlayback::from_gcode(document, input, 0., None).is_err());
}
