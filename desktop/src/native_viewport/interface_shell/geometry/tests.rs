use super::*;
use crate::native_viewport::interface_shell::tests::fixture;

#[test]
fn fully_clipped_controls_lose_focus_and_do_not_block_the_viewport() {
    let (mut app, handle, entity, _) = fixture();
    handle
        .pointer(PointerPhase::Down, [140., 140.], PointerButton::Primary)
        .unwrap();
    app.world_mut()
        .entity_mut(entity)
        .insert(CalculatedClip::FullyClipped);
    app.update();
    let shared = handle.shared.lock().unwrap();
    assert!(!shared.registry.frame().controls[0].visible);
    assert!(shared.focused.is_none());
    assert!(shared.capture.is_none());
    drop(shared);
    assert!(!handle.owns_pointer([140., 140.]));
}

#[test]
fn nested_translated_clips_preserve_dpi_and_unbounded_overflow_axes() {
    let (mut app, handle, entity, _) = fixture();
    app.world_mut()
        .get_mut::<ComputedNode>(entity)
        .unwrap()
        .inverse_scale_factor = 0.5;
    let clip = CalculatedClip::default()
        .with_rect(
            Rect::from_corners(
                Vec2::new(-10., -f32::INFINITY),
                Vec2::new(10., f32::INFINITY),
            ),
            &UiGlobalTransform::from_xy(60., 0.),
        )
        .with_rect(
            Rect::from_corners(Vec2::new(0., 32.), Vec2::new(90., 48.)),
            &UiGlobalTransform::default(),
        );
    app.world_mut().entity_mut(entity).insert(clip);
    app.update();
    let bounds = handle.shared.lock().unwrap().registry.frame().controls[0].bounds;
    assert_eq!(
        bounds,
        InterfaceRect {
            x: 125.,
            y: 116.,
            width: 10.,
            height: 8.
        }
    );
    assert!(handle.owns_pointer([130., 120.]));
    assert!(!handle.owns_pointer([130., 114.]));
    assert!(!handle.owns_pointer([124., 120.]));
}

#[test]
fn rotated_clips_use_the_visible_polygon_for_controls_and_panel_occlusion() {
    let (mut app, handle, entity, _) = fixture();
    let clip = CalculatedClip::default().with_rect(
        Rect::from_center_size(Vec2::ZERO, Vec2::splat(20.)),
        &UiGlobalTransform::from(bevy::math::Affine2::from_scale_angle_translation(
            Vec2::ONE,
            std::f32::consts::FRAC_PI_4,
            Vec2::new(60., 42.),
        )),
    );
    app.world_mut().entity_mut(entity).insert(clip.clone());
    app.update();
    let shared = handle.shared.lock().unwrap();
    let bounds = shared.registry.frame().controls[0].bounds;
    assert!(contains_point(bounds, [170., 152.]));
    assert_eq!(
        hit(&shared, [160., 142.]),
        Some(ControlKey(entity.to_bits()))
    );
    assert_eq!(hit(&shared, [170., 152.]), None);
    drop(shared);
    assert!(!handle.owns_pointer([170., 152.]));

    app.world_mut()
        .entity_mut(entity)
        .remove::<CalculatedClip>();
    app.world_mut().spawn((
        InterfaceOccluder,
        ComputedNode {
            size: Vec2::new(80., 24.),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_xy(60., 42.),
        ComputedStackIndex(2),
        InheritedVisibility::VISIBLE,
        clip,
    ));
    app.update();
    let shared = handle.shared.lock().unwrap();
    assert_eq!(hit(&shared, [160., 142.]), None);
    assert_eq!(
        hit(&shared, [170., 152.]),
        Some(ControlKey(entity.to_bits()))
    );
}
