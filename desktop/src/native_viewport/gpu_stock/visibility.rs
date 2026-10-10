//! Set visibility even when a newly reserved grid has not been spawned yet.
use bevy::camera::visibility::Visibility;
use bevy_ecs::prelude::{Commands, Entity, Query};

pub(super) fn set_grid(
    entity: Entity,
    active: bool,
    commands: &mut Commands,
    visibility: &mut Query<&mut Visibility>,
) {
    let next = if active {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if let Ok(mut current) = visibility.get_mut(entity) {
        if *current != next {
            *current = next;
        }
    } else {
        commands.entity(entity).insert(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::{prelude::*, system::SystemState};

    #[test]
    fn newly_reserved_grid_gets_final_visibility_in_one_flush() {
        for active in [false, true] {
            let mut world = World::new();
            let mut state = SystemState::<(Commands, Query<&mut Visibility>)>::new(&mut world);
            let entity;
            {
                let (mut commands, mut visibility) = state.get_mut(&mut world).unwrap();
                entity = commands.spawn(Visibility::Hidden).id();
                assert!(visibility.get_mut(entity).is_err());
                set_grid(entity, active, &mut commands, &mut visibility);
            }
            state.apply(&mut world);
            assert_eq!(
                *world.get::<Visibility>(entity).unwrap(),
                if active {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                }
            );
        }
    }

    #[test]
    fn matching_existing_grid_visibility_does_not_mark_it_changed() {
        let mut world = World::new();
        let entity = world.spawn(Visibility::Visible).id();
        world.clear_trackers();
        let mut state = SystemState::<(Commands, Query<&mut Visibility>)>::new(&mut world);
        {
            let (mut commands, mut visibility) = state.get_mut(&mut world).unwrap();
            set_grid(entity, true, &mut commands, &mut visibility);
        }
        state.apply(&mut world);
        assert!(!world
            .entity(entity)
            .get_ref::<Visibility>()
            .unwrap()
            .is_changed());
    }

    #[test]
    fn fallback_hides_an_existing_cut_surface() {
        let mut world = World::new();
        let entity = world.spawn(Visibility::Visible).id();
        let mut state = SystemState::<(Commands, Query<&mut Visibility>)>::new(&mut world);
        {
            let (mut commands, mut visibility) = state.get_mut(&mut world).unwrap();
            set_grid(entity, false, &mut commands, &mut visibility);
        }
        state.apply(&mut world);
        assert_eq!(
            *world.get::<Visibility>(entity).unwrap(),
            Visibility::Hidden
        );
    }
}
