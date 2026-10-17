//! Thin layout adapter for Bevy dialogs. Bevy owns title dragging/pointer capture;
//! the application supplies only resize constraints and its existing layer order.
use super::*;
use bevy::{
    picking::{cursor::EntityCursor, events::PointerDrag, pointer::PointerButton, Pickable},
    ui::{GlobalZIndex, UiScale, UiTransform, Val2},
    ui_widgets::{Dialog, DialogDragHandle, DialogPlugin},
    window::{PrimaryWindow, SystemCursorIcon},
};

#[derive(Component)]
struct FloatingDialog {
    minimum: Vec2,
    resized: Option<Vec2>,
}
#[derive(Component)]
struct ResizeHandle;

pub(super) fn install(app: &mut App) {
    app.init_resource::<UiScale>()
        .add_plugins(DialogPlugin)
        .add_observer(resize)
        .add_systems(PostUpdate, keep_in_window.before(UiSystems::Layout));
}

pub(crate) fn size(world: &mut World, root: Option<Entity>, initial: Vec2) -> Vec2 {
    let requested = root
        .and_then(|e| world.get::<FloatingDialog>(e))
        .and_then(|dialog| dialog.resized)
        .unwrap_or(initial);
    let maximum = window_ui_size(world)
        .map(|size| size - Vec2::splat(16.))
        .unwrap_or(initial);
    requested.min(maximum.max(Vec2::ONE))
}

pub(crate) fn origin(world: &World, root: Option<Entity>, initial: Vec2) -> Vec2 {
    root.filter(|_| resized(world, root))
        .and_then(|entity| world.get::<Node>(entity))
        .and_then(pixels)
        .map_or(initial, |(position, _)| position)
}

pub(crate) fn resized(world: &World, root: Option<Entity>) -> bool {
    root.and_then(|e| world.get::<FloatingDialog>(e))
        .is_some_and(|dialog| dialog.resized.is_some())
}

pub(crate) fn attach(world: &mut World, root: Entity, header: Entity, minimum: Vec2) {
    if world.get::<FloatingDialog>(root).is_none() {
        world.entity_mut(root).insert((
            Dialog,
            FloatingDialog {
                minimum,
                resized: None,
            },
        ));
        let color = crate::native_viewport::ui::theme(world).mute;
        world.spawn((
            Name::new("Resize dialog"),
            ResizeHandle,
            Pickable::default(),
            EntityCursor::System(SystemCursorIcon::NwseResize),
            Node {
                position_type: PositionType::Absolute,
                right: px(2.),
                bottom: px(2.),
                width: px(12.),
                height: px(12.),
                border: UiRect {
                    right: px(2.),
                    bottom: px(2.),
                    ..default()
                },
                ..default()
            },
            BorderColor::all(color),
            ZIndex(100),
            ChildOf(root),
        ));
    }
    world.entity_mut(header).insert((
        DialogDragHandle,
        Pickable::default(),
        EntityCursor::System(SystemCursorIcon::Move),
    ));
}

/// Existing scroll/annotation adapters need the translated window coordinates.
pub(crate) fn area(world: &World, root: Entity, mut bounds: InterfaceRect) -> InterfaceRect {
    if let Some(transform) = world.get::<UiTransform>(root) {
        let translation = translation(transform);
        bounds.x += f64::from(translation.x);
        bounds.y += f64::from(translation.y);
    }
    bounds
}

fn translation(transform: &UiTransform) -> Vec2 {
    match transform.translation {
        Val2 {
            x: Val::Px(x),
            y: Val::Px(y),
        } => Vec2::new(x, y),
        _ => Vec2::ZERO,
    }
}
fn pixels(node: &Node) -> Option<(Vec2, Vec2)> {
    match (node.left, node.top, node.width, node.height) {
        (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) => {
            Some((Vec2::new(x, y), Vec2::new(w, h)))
        }
        _ => None,
    }
}

fn resize(
    mut drag: On<PointerDrag>,
    handles: Query<&ChildOf, With<ResizeHandle>>,
    mut dialogs: Query<(&Node, &UiTransform, &mut FloatingDialog)>,
    scale: Res<UiScale>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok(parent) = handles.get(drag.entity) else {
        return;
    };
    let Ok((node, transform, mut dialog)) = dialogs.get_mut(parent.parent()) else {
        return;
    };
    let Some((position, size)) = pixels(node) else {
        return;
    };
    drag.propagate(false);
    let maximum = (window.size() / scale.0 - position - translation(transform) - Vec2::splat(8.))
        .max(Vec2::ONE);
    let minimum = dialog.minimum.min(maximum);
    dialog.resized =
        Some((dialog.resized.unwrap_or(size) + drag.delta / scale.0).clamp(minimum, maximum));
}

fn keep_in_window(
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    scale: Res<UiScale>,
    mut dialogs: Query<(&Node, &mut UiTransform, Option<&mut GlobalZIndex>), With<FloatingDialog>>,
) {
    let Some(window) = window else {
        return;
    };
    let available = window.size() / scale.0;
    for (node, mut transform, global_z) in &mut dialogs {
        // CAD already orders feature panels, menus and modal barriers. Keep that
        // order; DialogPlugin's separate floating-window stack must not cover a modal.
        if let Some(mut global_z) = global_z {
            global_z.set_if_neq(GlobalZIndex(0));
        }
        let Some((position, size)) = pixels(node) else {
            continue;
        };
        let margin = Vec2::splat(8.);
        let maximum = (available - size - margin).max(margin);
        let fitted = (position + translation(&transform)).clamp(margin, maximum) - position;
        let next = Val2::px(fitted.x, fitted.y);
        if transform.translation != next {
            transform.translation = next;
        }
    }
}

#[cfg(test)]
mod tests;
