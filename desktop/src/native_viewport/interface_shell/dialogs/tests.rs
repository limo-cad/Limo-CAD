use super::*;
use bevy::{
    input::{mouse::MouseButtonInput, ButtonState, InputPlugin},
    picking::{
        backend::{HitData, PointerHits},
        pointer::{PointerId, PointerLocation},
        DefaultPickingPlugins, PickingSystems,
    },
    time::TimePlugin,
    window::{CursorMoved, WindowEvent},
};

type Handles<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static ChildOf, Has<ResizeHandle>),
    Or<(With<DialogDragHandle>, With<ResizeHandle>)>,
>;

// Feed actual window input through Bevy picking. This backend supplies only the
// two visible rectangles; Bevy recognizes the drag, capture and release.
fn hits(
    pointers: Query<(&PointerId, &PointerLocation)>,
    handles: Handles,
    dialogs: Query<(&Node, &UiTransform), With<FloatingDialog>>,
    scale: Res<UiScale>,
    mut output: MessageWriter<PointerHits>,
) {
    for (id, location) in &pointers {
        let Some(location) = location.location() else {
            continue;
        };
        let mut hits = Vec::new();
        for (entity, parent, resize) in &handles {
            let (node, transform) = dialogs.get(parent.parent()).unwrap();
            let (position, size) = pixels(node).unwrap();
            let origin = position + translation(transform);
            let (min, extent) = if resize {
                (origin + size - Vec2::splat(14.), Vec2::splat(12.))
            } else {
                (origin, Vec2::new(size.x - 35., 40.))
            };
            let point = location.position / scale.0;
            if point.cmpge(min).all() && point.cmple(min + extent).all() {
                hits.push((entity, HitData::new(entity, 0., None, None)));
            }
        }
        output.write(PointerHits::new(*id, hits, 10.));
    }
}

fn fixture() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((TimePlugin, InputPlugin, DefaultPickingPlugins))
        .add_message::<WindowEvent>()
        .add_systems(PreUpdate, hits.in_set(PickingSystems::Backend));
    install(&mut app);
    app.world_mut().resource_mut::<UiScale>().0 = 1.25;
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: (1000, 800).into(),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    app.world_mut().spawn(PointerId::Mouse);
    let root = app
        .world_mut()
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(100.),
            top: px(80.),
            width: px(320.),
            height: px(400.),
            ..default()
        })
        .id();
    let header = app.world_mut().spawn((Node::default(), ChildOf(root))).id();
    attach(app.world_mut(), root, header, Vec2::new(280., 180.));
    app.update();
    (app, window, root)
}
fn move_to(app: &mut App, window: Entity, point: Vec2) {
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: point,
            delta: None,
        }));
    app.update();
}
fn button(app: &mut App, window: Entity, state: ButtonState) {
    app.world_mut()
        .write_message(WindowEvent::MouseButtonInput(MouseButtonInput {
            window,
            button: MouseButton::Left,
            state,
        }));
    app.update();
}

#[test]
fn standard_title_drag_respects_ui_scale_window_bounds_and_native_layers() {
    let (mut app, window, root) = fixture();
    move_to(&mut app, window, Vec2::new(180., 120.));
    button(&mut app, window, ButtonState::Pressed);
    move_to(&mut app, window, Vec2::new(305., 182.5));
    button(&mut app, window, ButtonState::Released);
    assert_eq!(
        translation(app.world().get::<UiTransform>(root).unwrap()),
        Vec2::new(100., 50.)
    );
    assert_eq!(
        app.world().get::<GlobalZIndex>(root),
        Some(&GlobalZIndex(0))
    );
    move_to(&mut app, window, Vec2::new(305., 182.5));
    button(&mut app, window, ButtonState::Pressed);
    // Begin within the window before continuing a captured drag past its edge.
    move_to(&mut app, window, Vec2::new(310., 187.5));
    move_to(&mut app, window, Vec2::new(2000., 1600.));
    button(&mut app, window, ButtonState::Released);
    let translated = translation(app.world().get::<UiTransform>(root).unwrap());
    assert_eq!(
        Vec2::new(100., 80.) + translated + Vec2::new(320., 400.),
        Vec2::new(792., 632.)
    );
    // A released drag no longer moves the dialog.
    move_to(&mut app, window, Vec2::new(100., 100.));
    assert_eq!(
        translation(app.world().get::<UiTransform>(root).unwrap()),
        translated
    );
}

#[test]
fn corner_resize_preserves_origin_and_has_readable_minimums() {
    let (mut app, window, root) = fixture();
    let start = Vec2::new(412., 472.) * 1.25;
    move_to(&mut app, window, start);
    button(&mut app, window, ButtonState::Pressed);
    move_to(&mut app, window, start + Vec2::new(125., -125.));
    assert_eq!(
        size(app.world_mut(), Some(root), Vec2::ZERO),
        Vec2::new(420., 300.)
    );
    assert_eq!(
        origin(app.world(), Some(root), Vec2::ZERO),
        Vec2::new(100., 80.)
    );
    move_to(&mut app, window, Vec2::ZERO);
    button(&mut app, window, ButtonState::Released);
    assert_eq!(
        size(app.world_mut(), Some(root), Vec2::ZERO),
        Vec2::new(280., 180.)
    );
    assert_eq!(
        translation(app.world().get::<UiTransform>(root).unwrap()),
        Vec2::ZERO
    );
    app.world_mut().despawn(root);
    assert!(
        !resized(app.world(), Some(root)),
        "Closing retires geometry with its dialog"
    );
}
