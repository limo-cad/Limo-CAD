//! Small retained chrome shared by native panels, menus and history.
//! This only paints and binds controls; document behavior stays in reducers.
use super::*;
use interface_shell::{
    compact_label,
    ribbon::{self, Icon},
    InterfaceCaption, InterfaceOccluder,
};
use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_controls_survive_panel_styling_and_refresh_without_button_labels() {
        let mut world = World::new();
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn_empty().id();
        let mut widgets = Widgets::default();
        let mut control = InterfaceControl::button("assembly/joints", "Rotation");
        control.field = limo_cad_interface::Field::Range {
            value: 0.,
            min: -30.,
            max: 30.,
            step: 1.,
        };
        let mut previous = None;
        for value in [0., 20., -30.] {
            control.field = limo_cad_interface::Field::Range {
                value,
                min: -30.,
                max: 30.,
                step: 1.,
            };
            let entity = widgets
                .button(
                    &mut world,
                    camera,
                    "angle",
                    control.clone(),
                    None,
                    NativeCommand::ClearSelection,
                    rect(0., 0., 180., 28.),
                    None,
                    35,
                )
                .unwrap();
            interface_shell::caption_size(&mut world, entity, 10.);
            assert!(world
                .get::<interface_shell::ranges::NativeRange>(entity)
                .is_some());
            assert_eq!(
                world.get::<InterfaceControl>(entity).unwrap().role,
                "slider"
            );
            if let Some(previous) = previous {
                assert_eq!(entity, previous);
            }
            previous = Some(entity);
        }
    }
    #[test]
    fn text_controls_are_real_retained_editors_with_the_value_visible() {
        let mut world = World::new();
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn_empty().id();
        let mut widgets = Widgets::default();
        let mut control = InterfaceControl::button("assembly/joints", "Instance name");
        control.field = limo_cad_interface::Field::Text {
            value: "Bracket".into(),
            read_only: false,
            selection: None,
        };
        let entity = widgets
            .button(
                &mut world,
                camera,
                "name",
                control.clone(),
                None,
                NativeCommand::ClearSelection,
                rect(0., 0., 180., 28.),
                None,
                35,
            )
            .unwrap();
        assert!(world
            .get::<interface_shell::fields::NativeTextField>(entity)
            .is_some());
        assert_eq!(
            world
                .get::<bevy::text::EditableText>(entity)
                .unwrap()
                .value(),
            "Bracket"
        );
        assert!(world.get::<InterfaceControl>(entity).unwrap().text_editing);
        let again = widgets
            .button(
                &mut world,
                camera,
                "name",
                control,
                None,
                NativeCommand::ClearSelection,
                rect(0., 0., 180., 28.),
                None,
                35,
            )
            .unwrap();
        assert_eq!(
            entity, again,
            "Refreshing a panel must retain the text editor and focus"
        );
        assert!(world.get::<InterfaceControl>(entity).unwrap().text_editing);
    }
}

#[derive(Default)]
pub(crate) struct Widgets {
    controls: HashMap<String, (Entity, NativeCommand, Option<Icon>)>,
    decoration: HashMap<String, Entity>,
    live: HashSet<String>,
}
pub(crate) fn rect(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(w.max(0.)),
        height: px(h.max(0.)),
        align_items: AlignItems::Center,
        border_radius: BorderRadius::all(px(3.)),
        overflow: Overflow::clip(),
        ..default()
    }
}
impl Widgets {
    pub(crate) fn entity(&self, key: &str) -> Option<Entity> {
        self.controls
            .get(key)
            .map(|(entity, _, _)| *entity)
            .or_else(|| self.decoration.get(key).copied())
    }
    pub(crate) fn parent(&self, world: &mut World, key: &str, parent: Entity) {
        if let Some(entity) = self.entity(key) {
            if world.get::<ChildOf>(entity).map(ChildOf::parent) != Some(parent) {
                world.entity_mut(entity).insert(ChildOf(parent));
            }
        }
    }
    pub(crate) fn begin(&mut self) {
        self.live.clear();
    }
    /// Retain a fixed decorative SVG alongside the panel's controls.
    pub(crate) fn glyph(
        &mut self,
        (world, camera): (&mut World, Entity),
        key: &str,
        bounds: Node,
        icon: Icon,
        color: Color,
        z: i32,
    ) {
        self.live.insert(key.into());
        let entity = *self
            .decoration
            .entry(key.into())
            .or_insert_with(|| ribbon::decoration(world, camera, icon, color));
        ribbon::refresh_decoration(world, entity, icon, color);
        if world.get::<Node>(entity) != Some(&bounds) {
            world.entity_mut(entity).insert(bounds);
        }
        if world.get::<ZIndex>(entity) != Some(&ZIndex(z)) {
            world.entity_mut(entity).insert(ZIndex(z));
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn button(
        &mut self,
        world: &mut World,
        camera: Entity,
        key: &str,
        mut control: InterfaceControl,
        caption: Option<&str>,
        command: NativeCommand,
        bounds: Node,
        icon: Option<Icon>,
        z: i32,
    ) -> Result<Entity, String> {
        self.live.insert(key.into());
        let assets = world.resource::<ViewportUiAssets>().clone();
        let theme = crate::native_viewport::ui::theme(world);
        let text_field = matches!(control.field, limo_cad_interface::Field::Text { .. });
        let range = matches!(control.field, limo_cad_interface::Field::Range { .. });
        if text_field {
            control.text_editing = true;
            control.role = "textbox".into();
        }
        if range {
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
            .map(limo_cad_interface::KeyChord::plain)
            .into();
        }
        let entity = if let Some((entity, _, _)) = self.controls.get(key) {
            *entity
        } else {
            let entity = if range {
                interface_shell::ranges::spawn(
                    &mut world.commands(),
                    camera,
                    bounds.clone(),
                    control.clone(),
                    theme,
                )
            } else if text_field {
                interface_shell::fields::spawn_text_field(
                    &mut world.commands(),
                    camera,
                    bounds.clone(),
                    control.clone(),
                    theme,
                    &assets,
                )?
            } else {
                spawn_button(
                    &mut world.commands(),
                    camera,
                    bounds.clone(),
                    control.clone(),
                    theme,
                    &assets,
                )
            };
            world.flush();
            bind_command(world, entity, command.clone())?;
            if !text_field && !range {
                compact_label(
                    world,
                    entity,
                    if caption == Some("") {
                        0.
                    } else if icon.is_some() {
                        24.
                    } else {
                        8.
                    },
                );
            }
            if let Some(icon) = icon {
                ribbon::compact_glyph(world, entity, icon, 4., 13.);
                if caption == Some("") {
                    ribbon::center_glyph(world, entity);
                }
            }
            self.controls
                .insert(key.into(), (entity, command.clone(), icon));
            entity
        };
        let entry = self.controls.get_mut(key).unwrap();
        if bounds.border != UiRect::default() {
            world
                .entity_mut(entity)
                .remove::<interface_shell::InterfaceFlat>();
        }
        if entry.1 != command {
            bind_command(world, entity, command.clone())?;
            entry.1 = command;
        }
        if entry.2 != icon {
            if let Some(icon) = icon {
                ribbon::replace_compact_glyph(world, entity, icon);
            }
            entry.2 = icon;
        }
        control.binding = world.get::<InterfaceControl>(entity).unwrap().binding;
        if world.get::<InterfaceControl>(entity) != Some(&control) {
            world.entity_mut(entity).insert(control.clone());
        }
        let caption = InterfaceCaption(caption.unwrap_or(&control.label).into());
        if world.get::<InterfaceCaption>(entity) != Some(&caption) {
            world.entity_mut(entity).insert(caption);
        }
        if world.get::<Node>(entity) != Some(&bounds) {
            world.entity_mut(entity).insert(bounds);
        }
        if world.get::<ZIndex>(entity) != Some(&ZIndex(z)) {
            world.entity_mut(entity).insert(ZIndex(z));
        }
        if world
            .get::<Node>(entity)
            .is_some_and(|node| node.justify_content == JustifyContent::Center)
            && icon.is_none()
            && !text_field
            && !range
            && world.get::<ribbon::RibbonButton>(entity).is_none()
        {
            interface_shell::center_caption(world, entity);
        }
        Ok(entity)
    }
    pub(crate) fn backdrop(
        &mut self,
        (world, camera): (&mut World, Entity),
        key: &str,
        scope: &str,
        command: NativeCommand,
        bounds: Node,
        z: i32,
    ) -> Result<(), String> {
        self.live.insert(key.into());
        let entity = if let Some((entity, _, _)) = self.controls.get(key) {
            *entity
        } else {
            let mut control = InterfaceControl::button(
                scope,
                if scope.ends_with("menu") {
                    "Close menu"
                } else {
                    "Close dialog"
                },
            );
            control.modal_scope = Some(scope.into());
            let entity = world
                .spawn((control, UiTargetCamera(camera), bounds.clone(), ZIndex(z)))
                .id();
            bind_command(world, entity, command.clone())?;
            self.controls.insert(key.into(), (entity, command, None));
            entity
        };
        if world.get::<Node>(entity) != Some(&bounds) {
            world.entity_mut(entity).insert(bounds);
        }
        Ok(())
    }
    pub(crate) fn panel(
        &mut self,
        world: &mut World,
        camera: Entity,
        key: &str,
        bounds: Node,
        color: Color,
        z: i32,
    ) {
        self.live.insert(key.into());
        let entity = *self.decoration.entry(key.into()).or_insert_with(|| {
            world
                .spawn((UiTargetCamera(camera), InterfaceOccluder))
                .id()
        });
        if bounds.border != UiRect::default() {
            let border = BorderColor::all(crate::native_viewport::ui::theme(world).edge);
            if world.get::<BorderColor>(entity) != Some(&border) {
                world.entity_mut(entity).insert(border);
            }
        }
        if world.get::<Node>(entity) != Some(&bounds) {
            world.entity_mut(entity).insert(bounds);
        }
        if world.get::<BackgroundColor>(entity) != Some(&BackgroundColor(color)) {
            world.entity_mut(entity).insert(BackgroundColor(color));
        }
        if world.get::<ZIndex>(entity) != Some(&ZIndex(z)) {
            world.entity_mut(entity).insert(ZIndex(z));
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn text(
        &mut self,
        world: &mut World,
        camera: Entity,
        key: &str,
        bounds: Node,
        value: &str,
        size: f32,
        z: i32,
    ) {
        self.live.insert(key.into());
        let entity = *self.decoration.entry(key.into()).or_insert_with(|| {
            let assets = world.resource::<ViewportUiAssets>().clone();
            let theme = crate::native_viewport::ui::theme(world);
            world
                .spawn((
                    UiTargetCamera(camera),
                    Text::default(),
                    theme.text(&assets, size, FontWeight::NORMAL),
                    TextColor(theme.ink),
                ))
                .id()
        });
        if world.get::<Text>(entity).is_none_or(|text| text.0 != value) {
            world.entity_mut(entity).insert(Text::new(value));
        }
        let ink = crate::native_viewport::ui::theme(world).ink;
        if world.get::<TextColor>(entity) != Some(&TextColor(ink)) {
            world.entity_mut(entity).insert(TextColor(ink));
        }
        if world.get::<Node>(entity) != Some(&bounds) {
            world.entity_mut(entity).insert(bounds);
        }
        if world.get::<ZIndex>(entity) != Some(&ZIndex(z)) {
            world.entity_mut(entity).insert(ZIndex(z));
        }
    }
    pub(crate) fn finish(&mut self, world: &mut World) {
        self.controls.retain(|key, (entity, _, _)| {
            if self.live.contains(key) {
                true
            } else {
                world.despawn(*entity);
                false
            }
        });
        self.decoration.retain(|key, entity| {
            if self.live.contains(key) {
                true
            } else {
                world.despawn(*entity);
                false
            }
        });
    }
}
