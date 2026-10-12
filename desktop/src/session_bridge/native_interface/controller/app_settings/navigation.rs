//! Draft navigation controls use the same retained widgets as other Settings.
use super::*;
use crate::app_preferences::navigation::{
    Action, Bindings, Button, DragSlot, Modifiers, ScrollSlot,
};
use limo_cad_interface::{ChoiceOption, KeyChord};
use preferences::locale::translate;
use serde::{de::DeserializeOwned, Serialize};

pub(super) const MIN_HEIGHT: f32 = 860.;
const PANEL_KEY: &str = "settings-navigation-bindings";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Button(DragSlot),
    Modifiers(DragSlot),
    Scroll(ScrollSlot),
}

pub(super) struct Draft {
    pub value: Bindings,
    baseline: Bindings,
    pub error: Option<String>,
}
impl Draft {
    fn new(bindings: Bindings) -> Self {
        Self {
            value: bindings,
            baseline: bindings,
            error: None,
        }
    }
    fn changed(&self) -> bool {
        self.value != self.baseline
    }
    pub(super) fn observe(&mut self, live: Bindings) {
        if !self.changed() && self.error.is_none() {
            *self = Self::new(live);
        }
    }
}

fn draft(settings: &mut Settings) -> &mut Draft {
    let live = settings.effective().navigation;
    settings.navigation.get_or_insert_with(|| Draft::new(live))
}

pub(super) fn reduce(
    settings: &mut Settings,
    command: Command,
    input: &ControlInput,
) -> Result<Value, String> {
    if let Command::NavigationField(field) = command {
        let ControlInput::SetValue(value) = input else {
            if super::super::super::is_activation(input) {
                return Ok(json!({"focused":true}));
            }
            return Err("Choose a navigation binding".into());
        };
        let draft = draft(settings);
        match field {
            Field::Button(slot) => draft.value.drag_mut(slot).button = parse(value)?,
            Field::Modifiers(slot) => draft.value.drag_mut(slot).modifiers = parse(value)?,
            Field::Scroll(slot) => *draft.value.scroll_mut(slot) = parse(value)?,
        }
        draft.error = None;
        return Ok(json!({"navigation_draft":draft.value,"applied":false}));
    }
    if !super::super::super::is_activation(input) {
        return Err("Activate the navigation Settings control".into());
    }
    match command {
        Command::CancelNavigation => {
            settings.navigation = Some(Draft::new(settings.effective().navigation));
            Ok(json!({"cancelled":true,"navigation":settings.effective().navigation}))
        }
        Command::ResetNavigation => {
            let draft = draft(settings);
            draft.value = Bindings::default();
            draft.error = None;
            Ok(json!({"navigation_draft":draft.value,"applied":false}))
        }
        Command::ApplyNavigation => {
            let (baseline, value) = {
                let draft = draft(settings);
                (draft.baseline, draft.value)
            };
            let result = settings
                .store
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|store| store.patch_navigation(baseline, value));
            match result {
                Ok(saved) => {
                    settings.saved = saved;
                    settings.navigation = Some(Draft::new(value));
                    Ok(json!({"navigation":value,"persisted":true}))
                }
                Err(error) => {
                    draft(settings).error = Some(error.clone());
                    Err(error)
                }
            }
        }
        _ => unreachable!("navigation reducer only receives navigation commands"),
    }
}

fn parse<T: DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(value.into()))
        .map_err(|_| "Choose an available navigation binding".into())
}
fn encoded<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("navigation enum serializes")
        .as_str()
        .expect("navigation enum is a string")
        .into()
}

pub(super) fn height(world: &World, settings: &Settings) -> f32 {
    settings
        .widgets
        .entity(PANEL_KEY)
        .and_then(|entity| world.get::<ComputedNode>(entity))
        .map_or(MIN_HEIGHT, |node| node.size.y * node.inverse_scale_factor)
        .max(MIN_HEIGHT)
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    settings: &mut Settings,
    parent: Entity,
    width: f32,
) -> Result<(), String> {
    let locale = settings.effective().locale;
    let t = |key| translate(locale, key);
    let value = draft(settings).value;
    settings.widgets.panel(
        world,
        camera,
        PANEL_KEY,
        Node {
            height: Val::Auto,
            min_height: px(MIN_HEIGHT),
            flex_direction: FlexDirection::Column,
            row_gap: px(6.),
            flex_shrink: 0.,
            ..rect(16., 680. - 94., width, 0.)
        },
        Color::NONE,
        74,
    );
    settings.widgets.parent(world, PANEL_KEY, parent);
    let panel = settings.widgets.entity(PANEL_KEY).unwrap();
    text(
        world,
        camera,
        settings,
        panel,
        "nav-title",
        t("navigation.title"),
        12.,
    );
    text(
        world,
        camera,
        settings,
        panel,
        "nav-scope",
        t("navigation.scope"),
        10.,
    );
    text(
        world,
        camera,
        settings,
        panel,
        "nav-drag-hint",
        t("navigation.dragHint"),
        10.,
    );
    for slot in DragSlot::ALL {
        let key = format!("nav-drag-{slot:?}");
        text(
            world,
            camera,
            settings,
            panel,
            &format!("{key}-label"),
            slot.label(locale),
            11.,
        );
        settings.widgets.panel(
            world,
            camera,
            &key,
            Node {
                width: percent(100.),
                height: px(28.),
                min_height: px(28.),
                flex_direction: FlexDirection::Row,
                column_gap: px(6.),
                flex_shrink: 0.,
                ..default()
            },
            Color::NONE,
            75,
        );
        settings.widgets.parent(world, &key, panel);
        let row = settings.widgets.entity(&key).unwrap();
        let binding = value.drag(slot);
        choice(
            world,
            camera,
            settings,
            row,
            &format!("{key}-button"),
            &t("navigation.buttonField").replace("{action}", slot.label(locale)),
            Field::Button(slot),
            encoded(binding.button),
            Button::ALL
                .into_iter()
                .map(|button| ChoiceOption {
                    value: encoded(button),
                    label: button.label(locale).into(),
                    disabled: button == Button::Left,
                })
                .collect(),
            Node {
                width: percent(35.),
                min_width: px(0.),
                flex_shrink: 0.,
                height: px(28.),
                ..default()
            },
            false,
        )?;
        choice(
            world,
            camera,
            settings,
            row,
            &format!("{key}-modifiers"),
            &t("navigation.modifiersField").replace("{action}", slot.label(locale)),
            Field::Modifiers(slot),
            encoded(binding.modifiers),
            Modifiers::ALL
                .into_iter()
                .map(|modifiers| ChoiceOption {
                    value: encoded(modifiers),
                    label: modifiers.label(locale),
                    disabled: false,
                })
                .collect(),
            Node {
                flex_grow: 1.,
                min_width: px(0.),
                height: px(28.),
                ..default()
            },
            binding.button == Button::Disabled,
        )?;
    }
    text(
        world,
        camera,
        settings,
        panel,
        "nav-scroll-title",
        t("navigation.scrollTitle"),
        12.,
    );
    text(
        world,
        camera,
        settings,
        panel,
        "nav-scroll-hint",
        t("navigation.scrollHint"),
        10.,
    );
    for slot in ScrollSlot::ALL {
        let key = format!("nav-scroll-{slot:?}");
        text(
            world,
            camera,
            settings,
            panel,
            &format!("{key}-label"),
            slot.label(locale),
            11.,
        );
        choice(
            world,
            camera,
            settings,
            panel,
            &key,
            slot.label(locale),
            Field::Scroll(slot),
            encoded(value.scroll(slot)),
            Action::ALL
                .into_iter()
                .filter(|action| !slot.is_base() || *action != Action::Inherit)
                .map(|action| ChoiceOption {
                    value: encoded(action),
                    label: action.label(locale).into(),
                    disabled: false,
                })
                .collect(),
            Node {
                width: percent(100.),
                min_height: px(28.),
                height: px(28.),
                flex_shrink: 0.,
                ..default()
            },
            false,
        )?;
    }
    let navigation_draft = draft(settings);
    let conflicts = navigation_draft.value.conflicts();
    let valid = conflicts.is_empty();
    let changed = navigation_draft.changed();
    let has_error = navigation_draft.error.is_some();
    let status = if !valid {
        conflicts
            .into_iter()
            .map(|conflict| conflict.message(locale))
            .collect::<Vec<_>>()
            .join("\n")
    } else if let Some(error) = &navigation_draft.error {
        error.clone()
    } else {
        t(if changed {
            "navigation.unsaved"
        } else {
            "navigation.applied"
        })
        .into()
    };
    text(world, camera, settings, panel, "nav-status", &status, 10.);
    for (key, label, command, disabled) in [
        (
            "nav-apply",
            "navigation.apply",
            Command::ApplyNavigation,
            !valid || !changed,
        ),
        (
            "nav-cancel",
            "navigation.cancel",
            Command::CancelNavigation,
            !changed && !has_error,
        ),
        (
            "nav-defaults",
            "navigation.defaults",
            Command::ResetNavigation,
            value == Bindings::default(),
        ),
    ] {
        let mut control = InterfaceControl::button("document/appearance", t(label));
        control.modal_scope = Some("app-settings".into());
        control.disabled = disabled;
        let entity = settings.widgets.button(
            world,
            camera,
            key,
            control,
            None,
            NativeCommand::AppSettings(command),
            Node {
                width: percent(100.),
                min_height: px(28.),
                height: px(28.),
                flex_shrink: 0.,
                ..default()
            },
            None,
            76,
        )?;
        settings.widgets.parent(world, key, panel);
        interface_shell::center_caption(world, entity);
        interface_shell::caption_size(world, entity, 11.);
    }
    Ok(())
}

fn text(
    world: &mut World,
    camera: Entity,
    settings: &mut Settings,
    parent: Entity,
    key: &str,
    value: &str,
    size: f32,
) {
    settings.widgets.text(
        world,
        camera,
        key,
        Node {
            width: percent(100.),
            min_width: px(0.),
            flex_shrink: 0.,
            ..default()
        },
        value,
        size,
        75,
    );
    settings.widgets.parent(world, key, parent);
    let entity = settings.widgets.entity(key).unwrap();
    let layout = TextLayout::new(Justify::Left, bevy::text::LineBreak::WordOrCharacter);
    if world.get::<TextLayout>(entity).is_none_or(|current| {
        current.justify != layout.justify || current.linebreak != layout.linebreak
    }) {
        world.entity_mut(entity).insert(layout);
    }
}

#[allow(clippy::too_many_arguments)]
fn choice(
    world: &mut World,
    camera: Entity,
    settings: &mut Settings,
    parent: Entity,
    key: &str,
    label: &str,
    field: Field,
    value: String,
    options: Vec<ChoiceOption>,
    bounds: Node,
    disabled: bool,
) -> Result<(), String> {
    let caption = options
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label.clone())
        .unwrap_or_default();
    let mut control = InterfaceControl::button("document/appearance", label);
    control.modal_scope = Some("app-settings".into());
    control.role = "combobox".into();
    control.disabled = disabled;
    control.owned_keys = [
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Home",
        "End",
    ]
    .map(KeyChord::plain)
    .into();
    control.field = limo_cad_interface::Field::Choice { value, options };
    let entity = settings.widgets.button(
        world,
        camera,
        key,
        control,
        Some(&caption),
        NativeCommand::AppSettings(Command::NavigationField(field)),
        bounds,
        None,
        76,
    )?;
    settings.widgets.parent(world, key, parent);
    interface_shell::center_caption(world, entity);
    interface_shell::compact_label(world, entity, 10.);
    Ok(())
}
