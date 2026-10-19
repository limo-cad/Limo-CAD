//! Menu behavior for the feature forms' existing expandable choice lists.
//! The form reducer owns open/selected state; Bevy owns menu input and focus.
use super::*;
use bevy::ui_widgets::{MenuAction, MenuEvent, MenuFocusState, MenuPopup};
use std::collections::HashSet;

#[derive(Component)]
pub(super) struct ChoicePopup(pub Entity);
#[derive(Component)]
struct Opening(NavAction);
#[derive(Resource, Default)]
pub(super) struct MenuRequests {
    requests: Vec<(Entity, bool, NativeInterfaceAction)>,
    pub(super) selected: HashSet<Entity>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<MenuRequests>().add_observer(on_menu);
}

pub(super) fn attach(world: &mut World, group: Entity, owner: Entity) {
    if world.get::<ChoicePopup>(group).is_none() {
        let nav = world
            .get::<Opening>(owner)
            .map_or(NavAction::First, |o| o.0);
        world.entity_mut(owner).remove::<Opening>();
        world.entity_mut(group).insert((
            ChoicePopup(owner),
            MenuPopup::default(),
            MenuFocusState::Closed,
            Opening(nav),
        ));
    }
}

fn on_menu(
    mut event: On<MenuEvent>,
    owners: Query<(&WidgetAction, &InterfaceControl), With<MenuButton>>,
    popups: Query<&ChoicePopup>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
    mut requests: ResMut<MenuRequests>,
    mut commands: Commands,
) {
    let owner = if owners.contains(event.source) {
        Some(event.source)
    } else {
        popups
            .get(event.source)
            .ok()
            .or_else(|| {
                parents
                    .iter_ancestors(event.source)
                    .find_map(|e| popups.get(e).ok())
            })
            .map(|p| p.0)
    };
    let Some(owner) = owner else {
        return;
    };
    let Ok((action, control)) = owners.get(owner) else {
        return;
    };
    event.propagate(false);
    let open = control.expanded == Some(true);
    match event.action {
        MenuAction::FocusRoot => focus.set(owner, FocusCause::Navigated),
        MenuAction::Open(nav) if !open => {
            commands.entity(owner).insert(Opening(nav));
            requests.requests.push((owner, false, action.0.clone()));
        }
        MenuAction::Toggle => requests.requests.push((owner, false, action.0.clone())),
        MenuAction::CloseAll if open => requests.requests.push((owner, true, action.0.clone())),
        _ => {}
    }
}

pub(super) fn enqueue(world: &mut World) {
    let MenuRequests { requests, selected } =
        std::mem::take(&mut *world.resource_mut::<MenuRequests>());
    let mut pending = world.resource_mut::<WidgetActions>();
    let mut closed = HashSet::new();
    for (owner, closing, action) in requests {
        // Choosing an option already closes its list in the form reducer.
        // A subsequent CloseAll must not toggle that list back open.
        if closing && (selected.contains(&owner) || !closed.insert(owner)) {
            continue;
        }
        pending.0.push(action);
    }
}

// Menus acquire focus in PostUpdate, after ordinary input dispatch. Publish that
// focus only after the new option entities have entered the CAD control frame.
pub(super) fn publish_focus(world: &mut World) {
    let menu_focused = world
        .resource::<InputFocus>()
        .get()
        .is_some_and(|entity| world.get::<MenuItem>(entity).is_some());
    if menu_focused {
        if let Err(error) = world.run_system_cached(super::publish_focus) {
            eprintln!("Menu focus publication failed: {error}");
        }
    }
}

// The retained painter creates choices before their availability is published.
// Arm Bevy's initial focus only after the real option bindings and TabIndex exist.
pub(super) fn activate_ready(world: &mut World) {
    let ready = world
        .query::<(Entity, &Opening, &Children)>()
        .iter(world)
        .filter(|(entity, _, _)| world.get::<ChoicePopup>(*entity).is_some())
        .filter(|(_, _, children)| {
            children.iter().any(|child| {
                world.get::<WidgetAction>(child).is_some()
                    && world
                        .get::<TabIndex>(child)
                        .is_some_and(|index| index.0 >= 0)
            })
        })
        .map(|(entity, opening, _)| (entity, opening.0))
        .collect::<Vec<_>>();
    for (entity, nav) in ready {
        world
            .entity_mut(entity)
            .remove::<Opening>()
            .insert(MenuFocusState::Opening(nav));
    }
}
