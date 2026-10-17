//! Real Bevy groups around existing controls. GhostNode preserves their layout.
use super::*;
use bevy::ui_widgets::{RadioGroup, SelectedTab, TabActivation, TabList};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SelectionGroup {
    Radio,
    Tabs,
    Choices,
}

fn group(world: &mut World, members: &[Entity], kind: SelectionGroup) -> Option<Entity> {
    let first = *members.first()?;
    let parent = world.get::<ChildOf>(first).map(ChildOf::parent);
    let existing = members.iter().find_map(|entity| {
        let parent = world.get::<ChildOf>(*entity)?.parent();
        (world.get::<SelectionGroup>(parent) == Some(&kind)).then_some(parent)
    });
    let group = existing.unwrap_or_else(|| {
        let entity = world.spawn((GhostNode, kind)).id();
        if let Some(parent) = parent {
            world.entity_mut(entity).insert(ChildOf(parent));
        }
        entity
    });
    for &entity in members {
        if world.get::<ChildOf>(entity).map(ChildOf::parent) != Some(group) {
            world.entity_mut(entity).insert(ChildOf(group));
        }
    }
    Some(group)
}

pub(crate) fn radio_group(world: &mut World, members: &[Entity]) {
    if let Some(group) = group(world, members, SelectionGroup::Radio) {
        if world.get::<RadioGroup>(group).is_none() {
            world.entity_mut(group).insert(RadioGroup);
        }
    }
}

pub(crate) fn tab_group(world: &mut World, members: &[Entity], enabled: bool) {
    if let Some(group) = group(world, members, SelectionGroup::Tabs) {
        if !enabled {
            // The existing overflow strip can navigate to documents that have no
            // visible tab entity. Preserve that route instead of rebuilding it.
            world.entity_mut(group).remove::<(TabList, SelectedTab)>();
        } else if world.get::<TabList>(group).is_none() {
            world.entity_mut(group).insert(TabList {
                activation: TabActivation::Automatic,
                ..default()
            });
        }
    }
}

pub(crate) fn choice_group(world: &mut World, owner: Entity, members: &[Entity]) {
    if let Some(group) = group(world, members, SelectionGroup::Choices) {
        choices::attach(world, group, owner);
    }
}

/// Retained painters can keep assigning the original layout parent without
/// removing the transparent interaction group between it and the control.
pub(crate) fn preserves_parent(world: &World, entity: Entity, parent: Entity) -> bool {
    world.get::<ChildOf>(entity).is_some_and(|child| {
        world.get::<SelectionGroup>(child.parent()).is_some()
            && world.get::<ChildOf>(child.parent()).map(ChildOf::parent) == Some(parent)
    })
}

pub(super) fn synchronize(world: &mut World) {
    let groups = world
        .query_filtered::<Entity, With<SelectionGroup>>()
        .iter(world)
        .collect::<Vec<_>>();
    for group in groups {
        let members = world
            .get::<Children>(group)
            .map(|c| c.to_vec())
            .unwrap_or_default();
        if members.is_empty() {
            world.despawn(group);
            continue;
        }
        if world.get::<TabList>(group).is_some() {
            let selected = SelectedTab(members.iter().copied().find(|entity| {
                world
                    .get::<InterfaceControl>(*entity)
                    .is_some_and(|c| c.selected == Some(true))
                    && world.get::<InteractionDisabled>(*entity).is_none()
            }));
            if world.get::<SelectedTab>(group) != Some(&selected) {
                world.entity_mut(group).insert(selected);
            }
        }
    }
}
