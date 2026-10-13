use super::*;

fn line(points: [f32; 6], label: Option<&str>) -> ViewportMeasurementLine {
    ViewportMeasurementLine {
        points,
        color: [1., 0.78, 0.12, 1.],
        label: label.map(str::to_string),
    }
}

fn label(world: &mut World, text: &str) -> (Entity, Node) {
    world
        .query_filtered::<(Entity, &Text, &Node), With<MeasurementLabel>>()
        .iter(world)
        .find_map(|(entity, value, node)| (value.0 == text).then(|| (entity, node.clone())))
        .expect("rendered measurement label")
}

fn pixels(node: &Node) -> Vec2 {
    let (Val::Px(x), Val::Px(y)) = (node.left, node.top) else {
        panic!("absolute logical pixel position")
    };
    Vec2::new(x, y)
}

#[test]
fn measurement_labels_follow_camera_resize_and_visibility_without_replacing_preview_annotations() {
    let mut app = App::new();
    app.init_resource::<PresentationResource>()
        .init_resource::<CameraResource>()
        .init_resource::<ViewportSizeResource>()
        .init_resource::<PaletteResource>()
        .init_resource::<ViewportUiAssets>()
        .add_systems(Update, update);
    app.world_mut().spawn(NativeOverlayCamera);
    let preview = app
        .world_mut()
        .spawn((NativeAnnotationRoot, Text::new("Preview")))
        .id();
    app.world_mut().resource_mut::<CameraResource>().camera = ViewportCamera {
        position: [0., 0., 10.],
        target: [0.; 3],
        up: [0., 1., 0.],
        vertical_fov_degrees: 90.,
    };
    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 200.,
        logical_height: 100.,
    };
    app.world_mut()
        .resource_mut::<PresentationResource>()
        .0
        .measurement_lines = Arc::new(vec![
        line([-1., 0., 0., 1., 0., 0.], Some("2.000 mm")),
        line([-1., 0., 0., 1., 0., 0.], None),
        line([-1., 0., 0., 1., 0., 0.], Some("Δ X: 2.000 mm")),
    ]);
    app.update();
    let (entity, first) = label(app.world_mut(), "2.000 mm");
    let (_, axis) = label(app.world_mut(), "Δ X: 2.000 mm");
    assert_ne!(
        first.top, axis.top,
        "Coincident guides need separate readouts"
    );
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<MeasurementLabel>>()
            .iter(app.world())
            .count(),
        2
    );
    assert_eq!(
        *app.world().get::<bevy::picking::Pickable>(entity).unwrap(),
        bevy::picking::Pickable::IGNORE
    );
    app.update();
    assert_eq!(
        label(app.world_mut(), "2.000 mm").0,
        entity,
        "Idle labels must be retained"
    );
    app.world_mut()
        .resource_mut::<CameraResource>()
        .camera
        .position[0] = 2.;
    app.world_mut()
        .resource_mut::<CameraResource>()
        .camera
        .target[0] = 2.;
    app.update();
    let (same, moved) = label(app.world_mut(), "2.000 mm");
    assert_eq!(same, entity);
    assert!((pixels(&moved) - pixels(&first) - Vec2::new(-10., 0.)).length() < 0.001);
    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 400.,
        logical_height: 200.,
    };
    app.update();
    let (same, resized) = label(app.world_mut(), "2.000 mm");
    assert_eq!(same, entity);
    assert!((pixels(&resized) - pixels(&moved) - Vec2::new(90., 50.)).length() < 0.001);
    app.world_mut()
        .resource_mut::<PresentationResource>()
        .0
        .measurement_lines = Arc::new(vec![
        line([-1., 0., 0., 1., 0., 0.], Some("2.000 mm")),
        line([-1., 0., 0., 1., 0., 0.], Some("Δ X: 2.000 mm")),
        line([-1., 0., 0., 1., 0., 0.], Some("Δ Y: 0.000 mm")),
        line([-1., 0., 0., 1., 0., 0.], Some("Δ Z: 0.000 mm")),
    ]);
    app.update();
    let mut heights: Vec<_> = app
        .world_mut()
        .query_filtered::<&Node, With<MeasurementLabel>>()
        .iter(app.world())
        .map(|node| pixels(node).y)
        .collect();
    heights.sort_by(f32::total_cmp);
    assert_eq!(heights.len(), 4);
    assert!(
        heights.windows(2).all(|pair| pair[1] - pair[0] >= 24.),
        "All coincident metric badges must remain readable"
    );
    app.world_mut()
        .resource_mut::<CameraResource>()
        .camera
        .target[2] = 20.;
    app.update();
    assert_eq!(label(app.world_mut(), "2.000 mm").1.display, Display::None);
    app.world_mut()
        .resource_mut::<PresentationResource>()
        .0
        .measurement_lines = Arc::default();
    app.update();
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<MeasurementLabel>>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(app.world().get::<Text>(preview).unwrap().0, "Preview");
}

#[test]
fn measurement_label_anchor_uses_visible_segment_and_handles_touching_and_invalid_guides() {
    let basis = camera_projection(
        ViewportCamera {
            position: [0., 0., 10.],
            target: [0.; 3],
            up: [0., 1., 0.],
            vertical_fov_degrees: 90.,
        },
        (200., 100.),
    )
    .unwrap();
    let projected = anchor(
        &basis,
        (200., 100.),
        &line([-100., 0., 0., 0., 0., 0.], Some("100 mm")),
    )
    .unwrap();
    assert!((projected - Vec2::new(50., 50.)).length() < 0.001);
    assert_eq!(
        anchor(&basis, (200., 100.), &line([0.; 6], Some("0 mm"))),
        Some(Vec2::new(100., 50.))
    );
    assert!(anchor(
        &basis,
        (200., 100.),
        &line([100., 0., 0., 110., 0., 0.], Some("10 mm"))
    )
    .is_none());
    assert!(anchor(
        &basis,
        (200., 100.),
        &line([0., 0., 20., 1., 0., 20.], Some("1 mm"))
    )
    .is_none());
    assert!(anchor(&basis, (200., 100.), &line([f32::NAN; 6], Some("invalid"))).is_none());
    let through_camera = anchor(
        &basis,
        (200., 100.),
        &line([-1., 0., 20., 1., 0., 0.], Some("distance")),
    )
    .unwrap();
    assert!((0.0..=200.).contains(&through_camera.x));
}
