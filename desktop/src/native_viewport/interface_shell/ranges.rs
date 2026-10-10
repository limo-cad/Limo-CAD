//! Retained horizontal range control using the shared pointer/key/value route.
use super::*;

#[derive(Component)]
pub(crate) struct NativeRange {
    track: Entity,
    fill: Entity,
    thumb: Entity,
    theme: ViewportUiTheme,
}
pub(super) fn refresh_theme(world: &mut World, theme: ViewportUiTheme) {
    for mut range in world.query::<&mut NativeRange>().iter_mut(world) {
        range.theme = theme;
    }
}
pub(crate) fn spawn(
    commands: &mut Commands,
    camera: Entity,
    node: Node,
    mut control: InterfaceControl,
    theme: ViewportUiTheme,
) -> Entity {
    control.role = "slider".into();
    control.text_editing = false;
    control.owned_keys = [
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
        "Home",
        "End",
    ]
    .map(KeyChord::plain)
    .into();
    let entity = commands
        .spawn((
            control,
            node,
            UiTargetCamera(camera),
            BackgroundColor(Color::NONE),
            Outline::new(px(2.), px(1.), Color::NONE),
        ))
        .id();
    let track = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(8.),
                right: px(8.),
                top: percent(50.),
                height: px(4.),
                margin: UiRect::top(px(-2.)),
                border_radius: BorderRadius::all(px(2.)),
                ..default()
            },
            BackgroundColor(theme.edge),
            ChildOf(entity),
        ))
        .id();
    let fill = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0.),
                top: px(0.),
                height: percent(100.),
                width: percent(0.),
                border_radius: BorderRadius::all(px(2.)),
                ..default()
            },
            BackgroundColor(theme.accent),
            ChildOf(track),
        ))
        .id();
    let thumb = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(0.),
                top: px(-5.),
                width: px(14.),
                height: px(14.),
                margin: UiRect::left(px(-7.)),
                border_radius: BorderRadius::MAX,
                border: UiRect::all(px(1.)),
                ..default()
            },
            BackgroundColor(theme.accent),
            BorderColor::all(theme.accent),
            ChildOf(track),
        ))
        .id();
    commands.entity(entity).insert(NativeRange {
        track,
        fill,
        thumb,
        theme,
    });
    entity
}
pub(crate) fn install(app: &mut App) {
    app.add_systems(Update, refresh.after(InterfaceReduction));
}
fn refresh(world: &mut World) {
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() else {
        return;
    };
    let focused = handle.shared.lock().ok().and_then(|s| s.focused);
    let changes = world
        .query::<(Entity, &InterfaceControl, &NativeRange)>()
        .iter(world)
        .filter_map(|(e, c, r)| {
            let Field::Range {
                value, min, max, ..
            } = c.field
            else {
                return None;
            };
            Some((
                e,
                r.track,
                r.fill,
                r.thumb,
                r.theme,
                c.disabled,
                ((value - min) / (max - min)).clamp(0., 1.) as f32,
            ))
        })
        .collect::<Vec<_>>();
    for (e, track, fill, thumb, theme, disabled, fraction) in changes {
        if let Some(mut node) = world.get_mut::<Node>(fill) {
            let width = percent(fraction * 100.);
            if node.width != width {
                node.width = width;
            }
        }
        if let Some(mut node) = world.get_mut::<Node>(thumb) {
            let left = percent(fraction * 100.);
            if node.left != left {
                node.left = left;
            }
        }
        for (part, color) in [
            (track, theme.edge),
            (fill, if disabled { theme.mute } else { theme.accent }),
            (thumb, if disabled { theme.mute } else { theme.accent }),
        ] {
            if world.get::<BackgroundColor>(part) != Some(&BackgroundColor(color)) {
                world.entity_mut(part).insert(BackgroundColor(color));
            }
        }
        let edge = BorderColor::all(if focused == Some(ControlKey(e.to_bits())) {
            theme.ink
        } else {
            theme.accent
        });
        if world.get::<BorderColor>(thumb) != Some(&edge) {
            world.entity_mut(thumb).insert(edge);
        }
        let focus_ring = if !disabled && focused == Some(ControlKey(e.to_bits())) {
            theme.accent
        } else {
            Color::NONE
        };
        if world
            .get::<Outline>(e)
            .is_none_or(|outline| outline.color != focus_ring)
        {
            world
                .entity_mut(e)
                .insert(Outline::new(px(2.), px(1.), focus_ring));
        }
    }
}

/// Both input bounds and the painted thumb use the same eight-pixel inset.
pub(super) fn pointer_value(control: &Control, x: f64) -> Option<f64> {
    let Field::Range { min, max, step, .. } = control.field else {
        return None;
    };
    let width = (control.bounds.width - 16.).max(1.);
    let fraction = ((x - control.bounds.x - 8.) / width).clamp(0., 1.);
    let value = min + fraction * (max - min);
    Some((min + ((value - min) / step).round() * step).clamp(min, max))
}
