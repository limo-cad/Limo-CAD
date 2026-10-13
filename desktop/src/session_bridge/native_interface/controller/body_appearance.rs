//! Native controls for the existing BodyAppearancePanel and shared catalog.
use super::*;
use chrome::{rect, Widgets};
use limo_cad_core::{BodyAppearance, BodyId};
use limo_cad_interface::{ChoiceOption, ControlInput, Field as ControlField, KeyChord};

mod draft;
mod panel;
pub(crate) mod preferences;
#[cfg(test)]
mod tests;
use draft::{Draft, Field};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Field(Field),
    Apply,
    Reset,
    SlicerTarget,
    Scroll(i32),
    Details,
    Info,
    PrintSettings,
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    owner: DocumentContext,
    revision: u64,
    body: u64,
}

#[derive(Resource, Default)]
struct State {
    key: Option<Key>,
    visible: bool,
    generation: u64,
    name: String,
    draft: Option<Draft>,
    scroll: usize,
    details: bool,
    errors: std::collections::BTreeMap<Field, (String, String)>,
    slicer_target: limo_cad_export::SlicerTarget,
    preference: preferences::Observer,
    preference_error: Option<String>,
    widgets: Widgets,
}

impl State {
    fn refresh_preference(&mut self, force: bool) {
        match self.preference.poll(std::time::Instant::now(), force) {
            Some(Ok(target)) => {
                self.slicer_target = target;
                self.preference_error = None;
            }
            Some(Err(error)) => self.preference_error = Some(error),
            None => {}
        }
    }
}

fn selected(world: &World) -> Option<u64> {
    let presentation = native_viewport::interface_view(world).2;
    (presentation.selected_body_ids.len() == 1
        && presentation.selected_face_ids.is_empty()
        && presentation.selected_edge_ids.is_empty())
    .then(|| presentation.selected_body_ids[0])
}

fn choices(draft: &Draft, field: Field) -> Option<Vec<ChoiceOption>> {
    let option = |value: String, label: String| ChoiceOption {
        value,
        label,
        disabled: false,
    };
    match field {
        Field::Brand => {
            let mut brands = vec![String::new()];
            brands.extend(limo_cad_export::brands().into_iter().map(str::to_string));
            if !brands.contains(&draft.value.brand) {
                brands.push(draft.value.brand.clone());
            }
            Some(
                brands
                    .into_iter()
                    .map(|brand| {
                        let label = if brand.is_empty() {
                            "Unspecified".into()
                        } else {
                            brand.clone()
                        };
                        option(brand, label)
                    })
                    .collect(),
            )
        }
        Field::Preset => {
            let mut choices = vec![option(String::new(), "Custom".into())];
            choices.extend(
                limo_cad_export::presets_for_brand(&draft.value.brand)
                    .into_iter()
                    .map(|p| {
                        option(
                            p.id.clone(),
                            format!(
                                "{} · {} · {}",
                                p.material
                                    .as_ref()
                                    .map_or("Plastic", |m| if m.kind == "metal" {
                                        "Metal"
                                    } else {
                                        "Plastic"
                                    }),
                                p.material_name,
                                p.color_name
                            ),
                        )
                    }),
            );
            if let Some(id) = draft
                .value
                .preset_id
                .as_ref()
                .filter(|id| !choices.iter().any(|c| &c.value == *id))
            {
                choices.push(ChoiceOption {
                    value: id.clone(),
                    label: format!("Saved profile · {id} (unavailable)"),
                    disabled: true,
                });
            }
            Some(choices)
        }
        _ => None,
    }
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    generation: u64,
    command: &Command,
) -> Result<Value, String> {
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    if matches!(command, Command::Field(_))
        && matches!(&action.control.input, ControlInput::Key(k) if k == &KeyChord::plain("Enter"))
    {
        return reduce(
            world,
            handle,
            engine,
            bridge,
            action,
            generation,
            &Command::Apply,
        );
    }
    let body = selected(world).ok_or("Select one solid body to edit its appearance")?;
    let key = Key {
        owner: receipt.owner.clone(),
        revision: receipt.revision,
        body,
    };
    let mut state = world
        .get_resource_mut::<State>()
        .ok_or("Select a body to edit its appearance")?;
    if !state.visible || state.key.as_ref() != Some(&key) || state.generation != generation {
        return Err("The body changed before the appearance edit was applied".into());
    }
    state.refresh_preference(true);
    if matches!(command, Command::Info) {
        return Ok(json!({"handled":true,"read_only":true}));
    }
    if matches!(command, Command::PrintSettings) {
        if !super::super::is_activation(&action.control.input) {
            return Err("Activate Print Settings".into());
        }
        if state.draft.as_ref().is_some_and(Draft::dirty) || !state.errors.is_empty() {
            return Err("Apply or reset the appearance draft before editing print settings".into());
        }

        return print_intent::open(world, engine, &receipt.owner, Some(body));
    }
    if matches!(command, Command::Details) {
        if !super::super::is_activation(&action.control.input) {
            return Err("Activate Material properties".into());
        }
        state.details = !state.details;
        state.scroll = 0;
        return Ok(json!({"handled":true,"properties_visible":state.details}));
    }
    if matches!(command, Command::SlicerTarget) {
        let options = slicer_choices();
        let selected = workbench::cam::choose(
            &options,
            state.slicer_target.as_str(),
            &action.control.input,
        )?;
        let target = serde_json::from_value(json!(selected)).map_err(|e| e.to_string())?;
        state.preference.write(target)?;
        state.slicer_target = target;
        state.preference_error = None;
        return Ok(json!({"handled":true,"slicer_target":target}));
    }
    if let Command::Scroll(delta) = command {
        if !super::super::is_activation(&action.control.input) {
            return Err("Activate the appearance scroll control".into());
        }
        state.scroll = state.scroll.saturating_add_signed(*delta as isize);
        return Ok(json!({"handled":true}));
    }
    let draft = state
        .draft
        .as_mut()
        .ok_or("The selected body was removed")?;
    match command {
        Command::Scroll(_)
        | Command::SlicerTarget
        | Command::Details
        | Command::Info
        | Command::PrintSettings => {
            unreachable!()
        }
        Command::Field(field) => {
            let value = if let Some(options) = choices(draft, *field) {
                workbench::cam::choose(&options, &draft.text(*field), &action.control.input)?
            } else if let ControlInput::SetValue(value) = &action.control.input {
                value.clone()
            } else if matches!(&action.control.input, ControlInput::Key(_)) {
                return Ok(json!({"handled":true}));
            } else if super::super::is_activation(&action.control.input) {
                return Ok(json!({"focused":true}));
            } else {
                return Err("Enter an appearance value".into());
            };
            let applies_catalog = *field == Field::Preset && !value.is_empty();
            let result = if value != draft.text(*field) || applies_catalog {
                draft.edit(*field, &value)
            } else {
                Ok(())
            };
            if let Err(error) = result {
                state.errors.insert(*field, (value, error.clone()));
                return Ok(json!({"handled":true,"valid":false,"error":error}));
            }
            if applies_catalog {
                state.errors.clear();
            } else {
                state.errors.remove(field);
            }
            return Ok(json!({"handled":true}));
        }
        Command::Reset => {
            if !super::super::is_activation(&action.control.input) {
                return Err("Activate Reset appearance".into());
            }
            draft.value = draft.original.clone();
            state.errors.clear();
            return Ok(json!({"handled":true}));
        }
        Command::Apply => {
            if !super::super::is_activation(&action.control.input) {
                return Err("Activate Apply appearance".into());
            }
            if let Some(error) = state.errors.values().next() {
                return Err(error.1.clone());
            }
            if !state.draft.as_ref().unwrap().dirty() {
                return Ok(json!({"handled":true}));
            }
        }
    }
    let arguments =
        serde_json::to_value(&state.draft.as_ref().unwrap().value).map_err(|e| e.to_string())?;
    worker::enqueue_transaction(
        world,
        "set_body_appearance".into(),
        move |services, guard| {
            services.bridge.apply_native_mutation_guarded(
                &services.engine,
                &receipt.owner,
                Some(receipt.revision),
                "set_body_appearance",
                || Ok(arguments),
                || guard.validate(),
            )
        },
        |world, services, result| {
            Ok(finish_mutation(
                &services.engine,
                &services.bridge,
                world,
                "set_body_appearance",
                result?,
            ))
        },
    )
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    width: f32,
    height: f32,
    active: bool,
) -> Result<(), String> {
    let body = selected(world);
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    let same_selection = state
        .key
        .as_ref()
        .is_some_and(|key| key.owner == *owner && Some(key.body) == body);
    if !state.visible && !same_selection {
        state.key = None;
        state.draft = None;
        state.errors.clear();
    }
    state.visible = false;
    state.widgets.begin();
    let result = (|| {
        // Temporarily hidden inspectors retain their draft only while the exact
        // document owner and body selection still match. Widgets retire below.
        if !active && same_selection {
            return Ok(());
        }
        let Some(body) = body.filter(|_| active) else {
            state.key = None;
            state.draft = None;
            state.errors.clear();
            return Ok(());
        };
        state.refresh_preference(false);
        let receipt = services
            .bridge
            .native_document_receipt(&services.engine, owner)?;
        let key = Key {
            owner: owner.clone(),
            revision: receipt.revision,
            body,
        };
        if state.key.as_ref() != Some(&key) {
            let (name, appearance) = services.bridge.with_native_document_receipt(
                &services.engine,
                owner,
                |revision| {
                    if revision != key.revision {
                        return Err("The body changed while opening its appearance".into());
                    }
                    let geometry = native_viewport::interface_geometry(world);
                    let name = geometry
                        .scene
                        .bodies
                        .iter()
                        .find(|b| b.id.0 == body)
                        .ok_or("The selected body was removed")?
                        .name
                        .clone();
                    let appearances: Vec<BodyAppearance> =
                        serde_json::from_value(crate::session_bridge::parse_engine_envelope(
                            services.engine.engine_call("body_appearances", ""),
                        )?)
                        .map_err(|e| e.to_string())?;
                    let appearance = appearances
                        .into_iter()
                        .find(|a| a.body_id.0 == body)
                        .unwrap_or_else(|| BodyAppearance::default_for(BodyId(body)));
                    Ok((name, appearance))
                },
            )?;
            state.name = name;
            state.generation = state
                .generation
                .checked_add(1)
                .ok_or("Appearance control generation exhausted")?;
            let canonical = serde_json::to_value(&appearance).map_err(|e| e.to_string())?;
            let retain = state.key.as_ref().is_some_and(|previous| {
                previous.owner.window_id == key.owner.window_id
                    && previous.owner.document_id == key.owner.document_id
                    && previous.body == key.body
            }) && state.draft.as_ref().is_some_and(|draft| {
                serde_json::to_value(&draft.original).ok().as_ref() == Some(&canonical)
            });
            if !retain {
                state.draft = Some(Draft::new(appearance));
                state.errors.clear();
            }
            state.key = Some(key);
        }
        panel::paint(world, camera, &mut state, width, height)?;
        state.visible = true;
        Ok(())
    })();
    state.widgets.finish(world);
    world.insert_resource(state);
    result
}

fn slicer_choices() -> Vec<ChoiceOption> {
    limo_cad_export::SlicerTarget::all()
        .iter()
        .filter(|target| {
            matches!(
                target,
                limo_cad_export::SlicerTarget::Standard
                    | limo_cad_export::SlicerTarget::PrusaSlicer
                    | limo_cad_export::SlicerTarget::Cura
            )
        })
        .map(|target| ChoiceOption {
            value: target.as_str().into(),
            label: preferences::label(*target).into(),
            disabled: false,
        })
        .collect()
}

pub(crate) fn caption(world: &World) -> Option<String> {
    let state = world.get_resource::<State>()?;
    if !state.visible {
        return None;
    }
    state.key.as_ref()?;
    let mut text = format!(
        "Body appearance: {}. 3MF target: {}.",
        state.name,
        preferences::label(state.slicer_target)
    );
    if let Ok(path) = preferences::path() {
        text.push_str(&format!("\nSlicer preference: {}", path.display()));
    }
    for (_, error) in state.errors.values() {
        text.push(' ');
        text.push_str(error);
    }
    if let Some(error) = &state.preference_error {
        text.push(' ');
        text.push_str(error);
    }
    Some(text)
}
