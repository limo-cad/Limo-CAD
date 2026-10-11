use super::*;
use limo_cad_core::EdgeId;
use limo_cad_occt::measurement::MeasurementProperties;

fn selected(occurrence: Option<u64>) -> MeasurementSelection {
    MeasurementSelection {
        body_id: BodyId(1),
        occurrence_id: occurrence,
        target: MeasurementTarget::Edge { edge_id: EdgeId(2) },
    }
}
#[test]
fn measure_units_scale_area_volume_and_signed_deltas_without_rounding_source_values() {
    let mut state = State {
        secondary: "mm".into(),
        precision: "4".into(),
        ..default()
    };
    assert_eq!(
        readout::format_value(25.4, 1, UnitSystem::In, &state),
        "1.0000 in (25.4000 mm)"
    );
    assert_eq!(
        readout::format_value(25.4f64.powi(2), 2, UnitSystem::In, &state),
        "1.0000 in² (645.1600 mm²)"
    );
    assert_eq!(
        readout::format_value(25.4f64.powi(3), 3, UnitSystem::In, &state),
        "1.0000 in³ (16387.0640 mm³)"
    );
    assert_eq!(
        readout::format_value(-0.00001, 1, UnitSystem::Mm, &state),
        "0.0000 mm"
    );
    assert_eq!(
        readout::format_value(32.125, 0, UnitSystem::Mm, &state),
        "32.1250°"
    );
    state.selections = vec![selected(Some(3)), selected(Some(4))];
    state.instances = Arc::new(vec![limo_cad_sketch::InstanceBodyPoseDto {
        occurrence_id: limo_cad_sketch::OccurrenceId(3),
        component_id: limo_cad_sketch::ComponentId(1),
        body_id: BodyId(1),
        translation: [0.; 3],
        rotation: DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2).to_array(),
        visible: true,
    }]);
    state.delta = "first".into();
    state.precision = "3".into();
    state.report = Some(MeasurementReport {
        distance: Some(5.),
        closest_points: Some([[0., 0., 0.], [3., 4., 0.]]),
        selections: vec![MeasurementProperties::default(); 2],
        ..default()
    });
    let before = state.report.clone();
    let values = rows(&state, UnitSystem::Mm);
    assert_eq!(
        values.iter().find(|r| r.label == "Δ X").unwrap().value,
        "4.000 mm"
    );
    assert_eq!(
        values.iter().find(|r| r.label == "Δ Y").unwrap().value,
        "-3.000 mm"
    );
    assert_eq!(
        state.report, before,
        "Formatting must preserve canonical results"
    );
}

#[test]
fn measure_selections_do_not_count_parent_bodies_or_alias_repeated_occurrences() {
    let scene: limo_cad_solid::SolidSceneDto=serde_json::from_value(json!({"bodies":[{"id":1,"feature_id":1,"name":"Part","mesh":{"positions":[],"normals":[],"indices":[]},"faces":[],"edges":[{"id":2,"key":"edge:0","points":[]}]}],"errors":[]})).unwrap();
    let view = ViewportPresentation {
        selected_body_ids: vec![1],
        selected_edge_ids: vec![2],
        selected_occurrence_id: Some(3),
        ..default()
    };
    let mut state = State {
        selections: initial(&view, &scene),
        ..default()
    };
    assert_eq!(state.selections, vec![selected(Some(3))]);
    state.select(selected(Some(4)));
    assert_eq!(
        state.selections.len(),
        2,
        "Two occurrences of one source edge are distinct measurements"
    );
    state.report = Some(MeasurementReport::default());
    state.select(selected(Some(3)));
    assert_eq!(state.selections, vec![selected(Some(4))]);
    assert!(state.report.is_none());
    state.select(selected(Some(5)));
    state.select(selected(Some(6)));
    assert_eq!(
        state.selections,
        vec![selected(Some(6))],
        "The third distinct pick starts a new measurement"
    );
}

#[test]
fn measure_panel_retains_controls_exposes_copy_values_and_scrolls_within_its_card() {
    let mut world = World::new();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn_empty().id();
    let mut state = State {
        visible: true,
        selections: vec![selected(None), selected(None)],
        secondary: "in".into(),
        ..default()
    };
    let properties = MeasurementProperties {
        length: Some(25.4),
        radius: Some(12.7),
        center: Some([0., 0., 3.]),
        ..default()
    };
    state.report = Some(MeasurementReport {
        selections: vec![properties.clone(), properties],
        distance: Some(3.),
        closest_points: Some([[0., 0., 0.], [0., 0., 3.]]),
        center_distance: Some(3.),
        maximum_distance: Some(25.576942741698785),
        ..default()
    });
    state.widgets.begin();
    paint(&mut world, camera, &mut state, UnitSystem::Mm, 1000., 600.).unwrap();
    state.widgets.finish(&mut world);
    let precision = state.widgets.entity("measure-field-1").unwrap();
    let binding = world.get::<InterfaceControl>(precision).unwrap().binding;
    let distance = state.widgets.entity("measure-row-0").unwrap();
    assert_eq!(
        world.get::<InterfaceControl>(distance).unwrap().label,
        "Copy Minimum distance: 3.000 mm (0.118 in)"
    );
    assert!(
        state.widgets.entity("measure-down").is_some(),
        "Long results need scrolling"
    );
    state.scroll = usize::MAX;
    state.generation += 1;
    state.widgets.begin();
    paint(&mut world, camera, &mut state, UnitSystem::Mm, 1000., 600.).unwrap();
    state.widgets.finish(&mut world);
    assert_eq!(state.widgets.entity("measure-field-1"), Some(precision));
    assert!(
        world.get::<InterfaceControl>(precision).unwrap().binding > binding,
        "Retained controls must reject their previous result generation"
    );
    assert!(state.widgets.entity("measure-row-0").is_none());
    let last = rows(&state, UnitSystem::Mm).len() - 1;
    let row = state
        .widgets
        .entity(&format!("measure-row-{last}"))
        .unwrap();
    let bounds = world.get::<Node>(row).unwrap();
    let Val::Px(top) = bounds.top else {
        panic!("pixel top")
    };
    let Val::Px(h) = bounds.height else {
        panic!("pixel height")
    };
    assert!(
        top + h <= 600. - 72. - 56.,
        "Results must stay above the footer and scroll controls"
    );
}

#[test]
fn measure_overlay_highlights_the_correct_extremum_reuses_idle_guides_and_preserves_preview() {
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<ViewportUiAssets>();
    let session = native_viewport::interface_view(world).0.to_string();
    let owner = DocumentContext {
        window_id: "qa".into(),
        document_id: session.clone(),
        epoch: 0,
    };
    native_viewport::apply_interface_preview(
        world,
        &session,
        native_viewport::ViewportPreview {
            marker: Some(native_viewport::ViewportSnapMarker {
                position: [7., 8., 9.],
                kind: native_viewport::ViewportSnapKind::Point,
            }),
            ..default()
        },
    )
    .unwrap();
    let preview = native_viewport::interface_preview_snapshot(world);
    let camera = world.spawn_empty().id();
    let mut state = State {
        visible: true,
        selections: vec![selected(None), selected(None)],
        ..default()
    };
    state.report = Some(MeasurementReport {
        selections: vec![MeasurementProperties::default(); 2],
        distance: Some(3.),
        closest_points: Some([[0., 0., 0.], [0., 0., 3.]]),
        center_distance: Some(3.),
        center_points: Some([[5., 0., 0.], [5., 0., 3.]]),
        maximum_distance: Some(5.),
        maximum_points: Some([[3., 0., 0.], [7., 0., 3.]]),
        ..default()
    });
    state.widgets.begin();
    paint(world, camera, &mut state, UnitSystem::Mm, 1200., 900.).unwrap();
    state.widgets.finish(world);
    let maximum = state.widgets.entity("measure-row-2").unwrap();
    world
        .entity_mut(maximum)
        .insert(bevy::picking::hover::Hovered(true));
    world.insert_resource(state);
    present(world, &owner).unwrap();
    let first = native_viewport::interface_view(world)
        .2
        .measurement_lines
        .clone();
    assert!(first
        .iter()
        .any(|line| line.points == [3., 0., 0., 7., 0., 3.]));
    assert!(
        !first
            .iter()
            .any(|line| line.points == [5., 0., 0., 5., 0., 3.]),
        "Maximum distance must not highlight the center distance"
    );
    present(world, &owner).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &native_viewport::interface_view(world).2.measurement_lines
    ));
    world.resource_mut::<State>().owner = Some(owner.clone());
    clear_current(world, &owner).unwrap();
    present(world, &owner).unwrap();
    assert!(native_viewport::interface_view(world)
        .2
        .measurement_lines
        .is_empty());
    assert!(
        Arc::ptr_eq(
            &preview,
            &native_viewport::interface_preview_snapshot(world)
        ),
        "Read-only measurement guides must not replace modeling previews"
    );
    let original = ViewportPresentation {
        selected_body_ids: vec![1],
        selected_occurrence_id: Some(3),
        ..default()
    };
    world.resource_mut::<State>().original = Some(original);
    assert!(escape(world, &owner).unwrap());
    assert!(!active(world));
    assert_eq!(
        native_viewport::interface_view(world).2.selected_body_ids,
        vec![1]
    );
    assert_eq!(
        native_viewport::interface_view(world)
            .2
            .selected_occurrence_id,
        Some(3)
    );
    let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    view.body_poses = Arc::new(vec![limo_cad_sketch::BodyPoseDto {
        body_id: BodyId(1),
        translation: [10., 0., 0.],
        rotation: [0., 0., 0., 1.],
    }]);
    super::super::super::clear_selection(&mut view);
    native_viewport::apply_interface_view(world, &session, None, Some(view)).unwrap();
    let mut state = world.resource_mut::<State>();
    state.visible = true;
    state.original = Some(ViewportPresentation {
        selected_body_ids: vec![1],
        ..default()
    });
    assert!(escape(world, &owner).unwrap());
    assert!(
        native_viewport::interface_view(world)
            .2
            .selected_body_ids
            .is_empty(),
        "Closing must not restore preselection after a placement change"
    );
}
