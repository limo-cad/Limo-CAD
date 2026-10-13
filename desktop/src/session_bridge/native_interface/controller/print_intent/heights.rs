//! Bound print-Z requests reuse the existing guarded manufacturing editor.
use super::*;
#[cfg(test)]
mod tests;
use crate::native_forms::{DimensionKind, MeasurementInput};
use limo_cad_core::{
    PrintHeightBindingDto, PrintHeightLayoutDto, PrintHeightRangeDto, PrintIntentDocumentDto,
    PrintLayerHeightProfileDto,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Saved,
    Name,
    Enabled,
    Layout,
    Coordinate,
    Min,
    Max,
    Speed(usize),
    Point,
    PointZ,
    PointHeight,
    Replacement,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Create,
    Delete,
    AddPoint,
    RemovePoint,
    ReviewRebind,
    Rebind,
    ReviewGroup,
    CopyGroup,
    ToggleReplacement,
}
#[derive(Default)]
pub(super) struct Editor {
    pub profile: bool,
    selection: String,
    original: Option<Value>,
    draft: Option<Value>,
    layout: String,
    context: Value,
    point: usize,
    replacement: String,
    replacements: std::collections::BTreeSet<String>,
    changed: bool,
    review: Option<Review>,
}
struct Review {
    kind: Command,
    model: String,
    record: Value,
    layout: PrintHeightLayoutDto,
    value: Value,
}
fn collection(profile: bool) -> &'static str {
    if profile {
        "layer_height_profiles"
    } else {
        "height_ranges"
    }
}
pub(super) fn current_settings(
    state: &State,
    document: &PrintIntentDocumentDto,
) -> PrintSettingsDto {
    if state.height_editor.profile {
        // Layer-height profiles contain samples, not settings.
        PrintSettingsDto::default()
    } else {
        document
            .height_ranges
            .iter()
            .find(|record| {
                record.id == state.height_editor.selection && record.body_id.0 == state.body
            })
            .map(|record| record.settings.clone())
            .unwrap_or_default()
    }
}
fn records(document: &PrintIntentDocumentDto, profile: bool) -> Vec<Value> {
    // Only these records are editable here. Do not serialize unrelated part
    // settings/modifiers or duplicate their potentially large placement data.
    if profile {
        document
            .layer_height_profiles
            .iter()
            .map(|record| serde_json::to_value(record).unwrap_or_default())
            .collect()
    } else {
        document
            .height_ranges
            .iter()
            .map(|record| serde_json::to_value(record).unwrap_or_default())
            .collect()
    }
}
pub(super) fn reset(state: &mut State) {
    let profile = state.height_editor.profile;
    state.height_editor = Editor {
        profile,
        ..default()
    };
}
pub(super) fn dirty(state: &State) -> bool {
    state.height_scope && state.height_editor.changed
}
pub(super) fn canonical(state: &mut State, document: &PrintIntentDocumentDto) -> Option<Value> {
    if !state.height_scope {
        return None;
    }
    let records = records(document, state.height_editor.profile);
    if state.height_editor.selection.is_empty() {
        if let Some(v) = records
            .iter()
            .find(|v| v["body_id"].as_u64() == Some(state.body))
        {
            state.height_editor.selection = v["id"].as_str().unwrap_or_default().into();
        }
    }
    records.into_iter().find(|v| {
        v["id"].as_str() == Some(&state.height_editor.selection)
            && v["body_id"].as_u64() == Some(state.body)
    })
}
pub(super) fn settings(value: Option<&Value>) -> PrintSettingsDto {
    value
        .and_then(|v| serde_json::from_value(v["settings"].clone()).ok())
        .unwrap_or_default()
}
pub(super) fn accept(state: &mut State, value: Option<Value>) {
    let same_selection =
        state.height_editor.original.as_ref().map(|v| &v["id"]) == value.as_ref().map(|v| &v["id"]);
    state.height_editor.original = value.clone();
    state.height_editor.draft = value;
    state.height_editor.changed = false;
    state.height_editor.point = 0;
    state.height_editor.review = None;
    state.height_editor.replacement.clear();
    state.height_editor.replacements.clear();
    if !same_selection {
        if let Some(v) = &state.height_editor.draft {
            let layout = &v["binding"]["layout"];
            state.height_editor.layout = layout["id"].as_str().unwrap_or("").into();
        }
    }
}
pub(super) fn layout(state: &State) -> PrintHeightLayoutDto {
    if state.height_editor.layout.is_empty() {
        PrintHeightLayoutDto::Assembly
    } else {
        PrintHeightLayoutDto::NamedLayout {
            id: state.height_editor.layout.clone(),
        }
    }
}
pub(super) fn set_context(state: &mut State, value: Value) {
    state.height_editor.context = value;
}
pub(super) fn has_draft(state: &State) -> bool {
    state.height_editor.draft.is_some()
}
pub(super) fn field_key(field: Field) -> &'static str {
    match field {
        Field::Saved => "height_saved",
        Field::Name => "height_name",
        Field::Enabled => "height_enabled",
        Field::Layout => "height_layout",
        Field::Coordinate => "height_coordinate",
        Field::Min => "height_min",
        Field::Max => "height_max",
        Field::Speed(0) => "height_outer_speed",
        Field::Speed(1) => "height_inner_speed",
        Field::Speed(_) => "height_infill_speed",
        Field::Point => "height_point",
        Field::PointZ => "height_point_z",
        Field::PointHeight => "height_point_height",
        Field::Replacement => "height_replacement",
    }
}
pub(super) fn choices(state: &State, field: Field) -> Option<Vec<ChoiceOption>> {
    Some(match field {
        Field::Saved => std::iter::once(option("", "No saved height request"))
            .chain(
                records(state.document.as_ref()?, state.height_editor.profile)
                    .iter()
                    .filter(|v| v["body_id"].as_u64() == Some(state.body))
                    .map(|v| {
                        option(
                            v["id"].as_str().unwrap_or(""),
                            format!(
                                "{} - {}",
                                v["name"].as_str().unwrap_or(""),
                                if v["enabled"] == true {
                                    "enabled"
                                } else {
                                    "disabled"
                                }
                            ),
                        )
                    }),
            )
            .collect(),
        Field::Enabled => vec![option("true", "Enabled"), option("false", "Disabled")],
        Field::Coordinate => vec![
            option("object_bottom", "Object bottom - whole printable group"),
            option("build_plate", "Build plate Z"),
        ],
        Field::Layout => std::iter::once(option("", "Assembled source layout"))
            .chain(
                state.height_editor.context["views"]["views"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| {
                        Some(option(
                            v["id"].as_str()?,
                            format!("Saved {}", v["name"].as_str()?),
                        ))
                    }),
            )
            .collect(),
        Field::Replacement => std::iter::once(option("", "Choose explicit target UUID"))
            .chain(
                records(state.document.as_ref()?, state.height_editor.profile)
                    .into_iter()
                    .filter(|v| v["id"].as_str() != Some(&state.height_editor.selection))
                    .map(|v| {
                        option(
                            v["id"].as_str().unwrap_or(""),
                            format!(
                                "Body {}: {} ({})",
                                v["body_id"],
                                v["name"].as_str().unwrap_or(""),
                                v["id"].as_str().unwrap_or("")
                            ),
                        )
                    }),
            )
            .collect(),
        Field::Point => state.height_editor.draft.as_ref()?["points"]
            .as_array()?
            .iter()
            .enumerate()
            .map(|(i, p)| {
                option(
                    i.to_string(),
                    format!(
                        "Sample {} - Z {} mm, {} mm layers",
                        i + 1,
                        p["z_mm"],
                        p["height_mm"]
                    ),
                )
            })
            .collect(),
        _ => return None,
    })
}
fn speed_key(i: usize) -> &'static str {
    ["outer_wall_mm_s", "inner_wall_mm_s", "infill_mm_s"][i.min(2)]
}
pub(super) fn text(state: &State, field: Field) -> String {
    if let Some((text, _)) = state.errors.get(field_key(field)) {
        return text.clone();
    }
    match field {
        Field::Saved => return state.height_editor.selection.clone(),
        Field::Layout => return state.height_editor.layout.clone(),
        Field::Point => return state.height_editor.point.to_string(),
        Field::Replacement => return state.height_editor.replacement.clone(),
        _ => {}
    }
    let Some(v) = &state.height_editor.draft else {
        return String::new();
    };
    let value = match field {
        Field::Name => &v["name"],
        Field::Enabled => &v["enabled"],
        Field::Coordinate => &v["coordinate"],
        Field::Min => &v["min_z_mm"],
        Field::Max => &v["max_z_mm"],
        Field::Speed(i) => &v["speeds"][speed_key(i)],
        Field::PointZ => &v["points"][state.height_editor.point]["z_mm"],
        Field::PointHeight => &v["points"][state.height_editor.point]["height_mm"],
        _ => return String::new(),
    };
    if matches!(
        field,
        Field::Min | Field::Max | Field::PointZ | Field::PointHeight
    ) {
        if let Some(value) = value.as_f64() {
            return MeasurementInput::new(DimensionKind::Length, value, state.units)
                .text()
                .into();
        }
    }
    if value.is_null() {
        String::new()
    } else {
        value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string())
    }
}
fn measurement(text: &str, units: limo_cad_core::UnitSystem) -> Result<f64, String> {
    let mut input = MeasurementInput::new(DimensionKind::Length, 0., units);
    input.set_text(text.into());
    input.evaluate(units, &[])
}
pub(super) fn edit(state: &mut State, field: Field, text: &str) -> Result<Value, String> {
    if field == Field::Saved {
        if state.dirty() {
            return Err(
                "Apply or discard the height draft before selecting another request".into(),
            );
        }
        let selected = records(
            state.document.as_ref().ok_or("Wait for settings")?,
            state.height_editor.profile,
        )
        .into_iter()
        .find(|v| v["id"].as_str() == Some(text) && v["body_id"].as_u64() == Some(state.body));
        if !text.is_empty() && selected.is_none() {
            return Err("Choose a height request attached to the selected source body".into());
        }
        state.height_editor.selection = text.into();
        accept(state, selected.clone());
        state.original = settings(selected.as_ref());
        state.draft = state.original.clone();
        state.errors.clear();
        return Ok(json!({"selected":true}));
    }
    if field == Field::Layout {
        state.height_editor.layout = text.into();
        state.height_editor.review = None;
        state.loaded_revision = None;
        return Ok(json!({"layout_selected":true}));
    }
    if field == Field::Replacement {
        state.height_editor.replacement = text.into();
        return Ok(json!({"selected":true}));
    }
    if field == Field::Point {
        state.height_editor.point = text.parse().map_err(|_| "Choose a layer sample")?;
        return Ok(json!({"selected":true}));
    }
    let units = state.units;
    let mut candidate = state
        .height_editor
        .draft
        .clone()
        .ok_or("Create or select a height request")?;
    let result: Result<(), String> = (|| {
        match field {
            Field::Name => candidate["name"] = json!(text),
            Field::Enabled => candidate["enabled"] = json!(text == "true"),
            Field::Coordinate => candidate["coordinate"] = json!(text),
            Field::Min => candidate["min_z_mm"] = json!(measurement(text, units)?),
            Field::Max => candidate["max_z_mm"] = json!(measurement(text, units)?),
            Field::Speed(i) => {
                let value = if text.trim().is_empty() {
                    Value::Null
                } else {
                    let v = text
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| "Enter a speed in mm/s, or leave blank to inherit")?;
                    if !v.is_finite() || v < 0. || v > 1.0e6 {
                        return Err("Speed must be finite in 0..=1000000 mm/s".into());
                    }
                    json!(v)
                };
                candidate["speeds"][speed_key(i)] = value;
            }
            Field::PointZ => {
                candidate["points"][state.height_editor.point]["z_mm"] =
                    json!(measurement(text, units)?)
            }
            Field::PointHeight => {
                candidate["points"][state.height_editor.point]["height_mm"] =
                    json!(measurement(text, units)?)
            }
            _ => return Err("Unknown height field".into()),
        };
        Ok(())
    })();
    if let Err(error) = result {
        state
            .errors
            .insert(field_key(field), (text.into(), error.clone()));
        return Ok(json!({"valid":false,"error":error}));
    }
    state.height_editor.draft = Some(candidate);
    state.height_editor.changed = true;
    state.height_editor.review = None;
    state.errors.remove(field_key(field));
    state.error = None;
    Ok(json!({"valid":true}))
}
fn binding(state: &State) -> Result<PrintHeightBindingDto, String> {
    let value: PrintHeightBindingDto =
        serde_json::from_value(state.height_editor.context["binding"].clone()).map_err(|_| {
            state.height_editor.context["binding_error"]
                .as_str()
                .unwrap_or("Wait for the selected layout binding")
                .to_string()
        })?;
    if value.layout != layout(state) {
        return Err("Wait for the selected layout binding to refresh".to_string());
    }
    Ok(value)
}
pub(super) fn create(state: &mut State) -> Result<Value, String> {
    if state.dirty() {
        return Err("Apply or discard the existing draft first".into());
    }
    let binding = binding(state)?;
    let height = binding
        .occurrences
        .first()
        .map(|o| o.max_z_mm - o.min_z_mm)
        .ok_or("Selected layout has no visible occurrence of this body")?;
    let id = uuid::Uuid::new_v4().to_string();
    let value = if state.height_editor.profile {
        json!({"id":id,"name":"Variable layer profile","body_id":state.body,"enabled":true,"binding":binding,"points":[{"z_mm":0.,"height_mm":0.2},{"z_mm":height/2.,"height_mm":0.2},{"z_mm":height,"height_mm":0.2}]})
    } else {
        json!({"id":id,"name":"Print height range","body_id":state.body,"enabled":true,"binding":binding,"coordinate":"object_bottom","min_z_mm":0.,"max_z_mm":height,"settings":PrintSettingsDto::default(),"speeds":{"outer_wall_mm_s":null,"inner_wall_mm_s":null,"infill_mm_s":null}})
    };
    state.height_editor.selection = id;
    state.height_editor.original = None;
    state.height_editor.draft = Some(value);
    state.height_editor.changed = true;
    state.height_editor.review = None;
    state.height_editor.point = 0;
    state.original = Default::default();
    state.draft = Default::default();
    Ok(json!({"draft":true,"variable_layers":state.height_editor.profile}))
}
fn record(state: &State, inherit: bool) -> Result<Value, String> {
    let mut value = state
        .height_editor
        .draft
        .clone()
        .ok_or("Create or select a height request")?;
    if !state.height_editor.profile {
        value["settings"] = serde_json::to_value(if inherit {
            PrintSettingsDto::default()
        } else {
            state.draft.clone()
        })
        .map_err(|e| e.to_string())?;
        if inherit {
            value["speeds"] =
                json!({"outer_wall_mm_s":null,"inner_wall_mm_s":null,"infill_mm_s":null});
        }
    }
    Ok(value)
}
pub(super) fn write(
    state: &State,
    command: &super::Command,
) -> Result<(&'static str, Value), String> {
    let mut value = record(state, matches!(command, super::Command::Inherit))?;
    match command {
        super::Command::Apply | super::Command::Inherit => {
            validate_record(&value, state.height_editor.profile)?;
            if matches!(command, super::Command::Inherit) && state.height_editor.profile {
                return Err("Variable layer schedules have no inherited schedule. Disable or delete this profile explicitly".into());
            }
            let captured: PrintHeightLayoutDto =
                serde_json::from_value(value["binding"]["layout"].clone())
                    .map_err(|e| e.to_string())?;
            if captured != layout(state) {
                return Err("The selected layout differs from the captured binding. Review and apply Rebind explicitly".into());
            }
            if state.height_editor.original.is_none() {
                value.as_object_mut().unwrap().remove("id");
            }
            value.as_object_mut().unwrap().remove("binding");
            value["layout"] = json!(captured);
            Ok(if state.height_editor.profile {
                (
                    "print_intent_upsert_layer_profile",
                    json!({"profile":value}),
                )
            } else {
                ("print_intent_upsert_height_range", json!({"range":value}))
            })
        }
        super::Command::Height(Command::Delete) => {
            if state.dirty() {
                return Err("Apply or discard the draft before deleting".into());
            }
            if state.height_editor.original.is_none() {
                return Err("Discard the unsaved request instead".into());
            }
            Ok(("print_intent_remove_height", json!({"id":value["id"]})))
        }
        super::Command::Height(Command::Rebind | Command::CopyGroup) => {
            let review = state
                .height_editor
                .review
                .as_ref()
                .ok_or("Review the proposed changes first")?;
            let kind = if matches!(command, super::Command::Height(Command::Rebind)) {
                Command::ReviewRebind
            } else {
                Command::ReviewGroup
            };
            if review.kind != kind
                || review.model != state.expected_model
                || review.record != value
                || review.layout != layout(state)
            {
                return Err("The reviewed request changed. Review it again".into());
            }
            if kind == Command::ReviewGroup {
                Ok((
                    "print_intent_set_document",
                    json!({"document":review.value["document"]}),
                ))
            } else {
                let mut args = json!({"id":value["id"],"layout":review.layout});
                if state.height_editor.profile {
                    args["points"] = value["points"].clone();
                } else {
                    args["interval"] = json!({"coordinate":value["coordinate"],"min_z_mm":value["min_z_mm"],"max_z_mm":value["max_z_mm"]});
                }
                Ok(("print_intent_rebind_height", args))
            }
        }
        _ => Err("Unknown height command".into()),
    }
}
pub(super) fn edit_points(state: &mut State, command: Command) -> Result<Value, String> {
    if command == Command::ToggleReplacement {
        let id = state.height_editor.replacement.clone();
        if id.is_empty() {
            return Err("Choose a target request UUID to replace".into());
        }
        if !records(
            state.document.as_ref().ok_or("Wait for settings")?,
            state.height_editor.profile,
        )
        .iter()
        .any(|v| {
            v["id"].as_str() == Some(&id)
                && v["id"].as_str() != Some(&state.height_editor.selection)
        }) {
            return Err("The replacement request changed or is the source".into());
        }
        if !state.height_editor.replacements.remove(&id) {
            state.height_editor.replacements.insert(id);
        }
        state.height_editor.review = None;
        return Ok(json!({"replace_ids":state.height_editor.replacements}));
    }
    let draft = state
        .height_editor
        .draft
        .as_mut()
        .ok_or("Create or select a variable layer profile")?;
    let points = draft["points"]
        .as_array_mut()
        .ok_or("Choose Variable layer profile scope")?;
    let i = state
        .height_editor
        .point
        .min(points.len().saturating_sub(1));
    if command == Command::RemovePoint {
        if points.len() <= 3 || i == 0 || i + 1 == points.len() {
            return Err(
                "Keep at least three samples, including zero and exact object height".into(),
            );
        }
        points.remove(i);
        state.height_editor.point = i.min(points.len() - 1);
    } else {
        let next = i + 1;
        if next >= points.len() {
            return Err("Select a sample before the final endpoint to insert a midpoint".into());
        }
        let z = (points[i]["z_mm"].as_f64().ok_or("Invalid point")?
            + points[next]["z_mm"].as_f64().ok_or("Invalid point")?)
            / 2.;
        let h = points[i]["height_mm"].clone();
        points.insert(next, json!({"z_mm":z,"height_mm":h}));
        state.height_editor.point = next;
    }
    state.height_editor.changed = true;
    state.height_editor.review = None;
    Ok(json!({"samples":points.len()}))
}

pub(super) fn review(
    world: &mut World,
    receipt: workspace::DocumentReceipt,
    command: Command,
) -> Result<Value, String> {
    let state = world.resource::<State>();
    let value = record(state, false)?;
    if state.height_editor.original.is_none() {
        return Err("Save the source request before reviewing Rebind or group copies".into());
    }
    if command == Command::ReviewRebind {
        let mut original = state.height_editor.original.clone().unwrap();
        let mut proposed = value.clone();
        for field in ["binding", "coordinate", "min_z_mm", "max_z_mm", "points"] {
            original.as_object_mut().unwrap().remove(field);
            proposed.as_object_mut().unwrap().remove(field);
        }
        if original != proposed {
            return Err("Apply or discard name, enable and setting edits before Rebind. Rebind reviews only placement and numeric height corrections".into());
        }
    }
    if command == Command::ReviewGroup && state.dirty() {
        return Err("Apply or discard the source draft before reviewing group copies".into());
    }
    let model = state.expected_model.clone();
    let selected_layout = layout(state);
    let generation = state.generation;
    let profile = state.height_editor.profile;
    let document = state.document.clone().ok_or("Wait for settings")?;
    let replacements = state.height_editor.replacements.clone();
    let source = value.clone();
    let input_layout = selected_layout.clone();
    let owner = receipt.owner.clone();
    let revision = receipt.revision;
    worker::enqueue_document_io(
        world,
        "print-height-review".into(),
        move |services, guard| {
            services
                .bridge
                .with_native_document_receipt(&services.engine, &owner, |current| {
                    if current != revision {
                        return Err("Print height review changed documents or revision".into());
                    }
                    guard.validate()?;
                    let captured = capture_binding(
                        &services.engine,
                        source["body_id"].as_u64().ok_or("Missing body")?,
                        &input_layout,
                    )?;
                    let result = if command == Command::ReviewRebind {
                        let mut proposed = source.clone();
                        proposed["binding"] = json!(captured);
                        validate_record(&proposed, profile)?;
                        json!({"previous_binding":source["binding"],"binding":captured})
                    } else {
                        review_group_document(
                            &services.engine,
                            document,
                            source,
                            profile,
                            &input_layout,
                            &replacements,
                        )?
                    };
                    Ok(NativeMutationResult {
                        context: owner.clone(),
                        engine_revision: revision,
                        value: result,
                    })
                })
        },
        move |world, services, result| {
            let result = result?;
            services.bridge.with_native_document_receipt(
                &services.engine,
                &result.context,
                |current| {
                    let mut state = world
                        .get_resource_mut::<State>()
                        .ok_or("Print Settings closed")?;
                    if current != revision
                        || state.owner.as_ref() != Some(&result.context)
                        || state.generation != generation
                        || state.expected_model != model
                        || record(&state, false)? != value
                        || layout(&state) != selected_layout
                    {
                        return Err("The height request changed during review".into());
                    }
                    state.height_editor.review = Some(Review {
                        kind: command,
                        model,
                        record: value,
                        layout: selected_layout,
                        value: result.value.clone(),
                    });
                    Ok(json!({"review":result.value}))
                },
            )
        },
    )
}
fn capture_binding(
    engine: &AppState,
    body: u64,
    layout: &PrintHeightLayoutDto,
) -> Result<PrintHeightBindingDto, String> {
    serde_json::from_value(parse_engine_envelope(engine.engine_call(
        "print_intent_height_binding",
        &json!({"body_id":body,"layout":layout}).to_string(),
    ))?)
    .map_err(|e| e.to_string())
}
fn review_group_document(
    engine: &AppState,
    mut document: PrintIntentDocumentDto,
    source: Value,
    profile: bool,
    layout: &PrintHeightLayoutDto,
    replacements: &std::collections::BTreeSet<String>,
) -> Result<Value, String> {
    let source_body = source["body_id"].as_u64().ok_or("Missing source body")?;
    let mut bindings = std::collections::BTreeMap::new();
    let mut pending = std::collections::BTreeSet::from([source_body]);
    while let Some(body) = pending.pop_first() {
        if bindings.contains_key(&body) {
            continue;
        }
        let binding = capture_binding(engine, body, layout)?;
        for member in binding.groups.iter().flat_map(|g| &g.members) {
            if !bindings.contains_key(&member.body_id.0) {
                pending.insert(member.body_id.0);
            }
        }
        bindings.insert(body, binding);
    }
    let captured = serde_json::from_value::<PrintHeightBindingDto>(source["binding"].clone())
        .map_err(|e| e.to_string())?;
    if bindings.get(&source_body) != Some(&captured) {
        return Err("Source binding is stale. Review and Rebind it before copying".into());
    }
    for id in replacements {
        let original = records(&document, profile)
            .into_iter()
            .find(|v| v["id"].as_str() == Some(id))
            .ok_or("A selected replacement UUID was removed")?;
        let body = original["body_id"]
            .as_u64()
            .ok_or("Replacement omitted body")?;
        if body == source_body || !bindings.contains_key(&body) {
            return Err(format!(
                "Replacement {id} is outside the reviewed target members or is the source request"
            ));
        }
    }
    if profile {
        document
            .layer_height_profiles
            .retain(|v| !replacements.contains(&v.id));
    } else {
        document
            .height_ranges
            .retain(|v| !replacements.contains(&v.id));
    }
    let mut copies = vec![];
    for (&body, binding) in &bindings {
        if body == source_body {
            continue;
        }
        let mut copy = source.clone();
        copy["id"] = json!(uuid::Uuid::new_v4().to_string());
        copy["body_id"] = json!(body);
        copy["binding"] = json!(binding);
        if profile {
            let v: PrintLayerHeightProfileDto =
                serde_json::from_value(copy.clone()).map_err(|e| e.to_string())?;
            v.validate()
                .map_err(|e| format!("Body {body} needs explicit profile correction: {e}"))?;
            document.layer_height_profiles.push(v);
        } else {
            let v: PrintHeightRangeDto =
                serde_json::from_value(copy.clone()).map_err(|e| e.to_string())?;
            v.validate()
                .map_err(|e| format!("Body {body} needs explicit interval correction: {e}"))?;
            document.height_ranges.push(v);
        }
        copies.push(copy);
    }
    document.validate()?;
    let groups: std::collections::BTreeSet<_> = bindings
        .values()
        .flat_map(|b| b.groups.iter().map(|g| g.root_occurrence_id))
        .collect();
    let occurrences: std::collections::BTreeSet<_> = bindings
        .values()
        .flat_map(|b| b.occurrences.iter().map(|o| o.occurrence_id))
        .collect();
    Ok(
        json!({"document":document,"source_body_id":source_body,"body_ids":bindings.keys().collect::<Vec<_>>(),"groups":groups,"occurrence_ids":occurrences,"copies":copies,"replaced_ids":replacements,"layout":layout,"coordinate":source["coordinate"],"min_z_mm":source["min_z_mm"],"max_z_mm":source["max_z_mm"],"points":source["points"],"settings":source["settings"],"speeds":source["speeds"],"bindings":bindings}),
    )
}
pub(super) fn load_context(
    engine: &AppState,
    body: u64,
    layout: &PrintHeightLayoutDto,
) -> Result<Value, String> {
    let views = parse_engine_envelope(engine.engine_call("named_views", "{}"))?;
    let names: std::collections::BTreeMap<_, _> = engine
        .solid_scene_snapshot()
        .bodies
        .iter()
        .map(|b| (b.id.0.to_string(), b.name.clone()))
        .collect();
    Ok(match capture_binding(engine, body, layout) {
        Ok(binding) => json!({"views":views,"body_names":names,"binding":binding}),
        Err(error) => json!({"views":views,"body_names":names,"binding_error":error}),
    })
}
type Row = (
    String,
    Option<super::Field>,
    Option<super::Command>,
    Option<String>,
);
fn field_row(label: &str, field: Field) -> Row {
    (label.into(), Some(super::Field::Height(field)), None, None)
}
fn command_row(label: &str, command: Command) -> Row {
    (
        label.into(),
        None,
        Some(super::Command::Height(command)),
        None,
    )
}
fn info(label: impl Into<String>, text: impl Into<String>) -> Row {
    (label.into(), None, None, Some(text.into()))
}
pub(super) fn rows(state: &State) -> Vec<Row> {
    let profile = state.height_editor.profile;
    let mut rows = vec![
        field_row("Saved height request", Field::Saved),
        field_row("Height request layout", Field::Layout),
        command_row(
            if profile {
                "Create variable layer profile"
            } else {
                "Create print height range"
            },
            Command::Create,
        ),
        field_row("Height request name", Field::Name),
        field_row("Height request enabled", Field::Enabled),
    ];
    if profile {
        rows.push(info("Variable layers opt-in","Separate object-bottom layer schedule; no wall-strength guarantee. The saved slicer template validates every nozzle's permitted heights."));
        rows.extend([
            field_row("Variable layer sample", Field::Point),
            field_row("Sample object-bottom Z", Field::PointZ),
            field_row("Sample layer height", Field::PointHeight),
            command_row("Add layer sample midpoint", Command::AddPoint),
            command_row("Remove layer sample", Command::RemovePoint),
        ]);
    } else {
        rows.extend([
            field_row("Height range coordinate", Field::Coordinate),
            field_row("Height range minimum Z", Field::Min),
            field_row("Height range maximum Z", Field::Max),
            field_row("Requested outer wall speed (mm/s)", Field::Speed(0)),
            field_row("Requested inner wall speed (mm/s)", Field::Speed(1)),
            field_row("Requested infill speed (mm/s)", Field::Speed(2)),
        ]);
    }
    rows.extend([
        command_row("Delete height request", Command::Delete),
        command_row("Review height rebind", Command::ReviewRebind),
        command_row("Apply reviewed height rebind", Command::Rebind),
        field_row("Group copy target replacement", Field::Replacement),
        command_row(
            "Toggle explicit target replacement",
            Command::ToggleReplacement,
        ),
        command_row(
            "Review copies to printable group members",
            Command::ReviewGroup,
        ),
        command_row("Apply reviewed group copies", Command::CopyGroup),
    ]);
    if state.height_editor.context["views"]["views"]
        .as_array()
        .is_some_and(|views| views.iter().any(|v| v["id"].is_null()))
    {
        rows.push(info("Legacy named layout identity","Save a legacy named view once to assign its stable identity before binding height requests."));
    }
    rows.push(info(
        "Selected replacement UUIDs",
        format!(
            "{} explicitly selected target requests",
            state.height_editor.replacements.len()
        ),
    ));
    for (i, id) in state.height_editor.replacements.iter().enumerate() {
        rows.push(info(format!("Replacement target UUID {}", i + 1), id));
    }
    if let Some(error) = state.height_editor.context["binding_error"].as_str() {
        rows.push(info("Height binding unavailable", error));
    }
    if let Some(draft) = &state.height_editor.draft {
        let current = &state.height_editor.context["binding"];
        if current.is_object() && current != &draft["binding"] {
            rows.push(info("Selected layout current bounds","Current engine capture; adjust interval or sample endpoints explicitly before reviewing Rebind."));
            binding_rows(&mut rows, "Current selected layout", current, state);
        }
    }
    if let Some(draft) = &state.height_editor.draft {
        rows.push(info(
            "Captured print-Z binding",
            format!(
                "{}; {} groups; {} occurrences",
                layout_label(state, &draft["binding"]["layout"]),
                draft["binding"]["groups"].as_array().map_or(0, Vec::len),
                draft["binding"]["occurrences"]
                    .as_array()
                    .map_or(0, Vec::len)
            ),
        ));
        binding_rows(&mut rows, "Captured", &draft["binding"], state);
    }
    if let Some(review) = &state.height_editor.review {
        rows.push(info(
            "Reviewed height change",
            if review.kind == Command::ReviewGroup {
                "Explicit copies to every affected printable-group definition"
            } else {
                "Explicit update of captured placement; source settings remain unchanged"
            },
        ));
        rows.push(info(
            "Reviewed layout",
            layout_label(state, &json!(review.layout)),
        ));
        if let PrintHeightLayoutDto::NamedLayout { id } = &review.layout {
            rows.push(info("Reviewed saved layout ID", id));
        }
        if profile {
            for (i, point) in review.record["points"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                rows.push(info(
                    format!("Reviewed layer sample {}", i + 1),
                    format!(
                        "Object-bottom Z {} mm; layer height {} mm",
                        point["z_mm"], point["height_mm"]
                    ),
                ));
            }
        } else {
            let coordinate = if review.record["coordinate"] == "object_bottom" {
                "Whole object bottom"
            } else {
                "Build plate"
            };
            rows.push(info(
                "Reviewed interval",
                format!(
                    "{coordinate}: {}..{} mm",
                    review.record["min_z_mm"], review.record["max_z_mm"]
                ),
            ));
            for (key, label) in [
                ("wall_count", "Walls"),
                ("infill_density_percent", "Infill (%)"),
                ("infill_pattern", "Infill pattern"),
                ("top_shell_layers", "Top shell layers"),
                ("bottom_shell_layers", "Bottom shell layers"),
            ] {
                let v = &review.record["settings"][key];
                rows.push(info(
                    format!("Reviewed {label}"),
                    if v.is_null() {
                        "Inherit".into()
                    } else {
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string())
                    },
                ));
            }
            for (key, label) in [
                ("outer_wall_mm_s", "outer wall"),
                ("inner_wall_mm_s", "inner wall"),
                ("infill_mm_s", "infill"),
            ] {
                let v = &review.record["speeds"][key];
                rows.push(info(
                    format!("Reviewed {label} speed"),
                    if v.is_null() {
                        "Inherit".into()
                    } else {
                        format!("{v} mm/s")
                    },
                ));
            }
        }
        if review.kind == Command::ReviewGroup {
            rows.push(info(
                "Reviewed group members",
                format!(
                    "Body IDs {}; groups {}; occurrences {}",
                    review.value["body_ids"],
                    review.value["groups"],
                    review.value["occurrence_ids"]
                ),
            ));
            rows.push(info("Group copy policy","Source UUID and unrelated records stay intact. Copies use new UUIDs. Only explicitly selected target UUIDs are replaced."));
            for (body, binding) in review.value["bindings"].as_object().into_iter().flatten() {
                let name = state.height_editor.context["body_names"][body]
                    .as_str()
                    .unwrap_or("Retained source");
                rows.push(info(
                    format!("Reviewed body {body}"),
                    format!(
                        "{name}; groups {}; occurrences {}",
                        binding["groups"].as_array().map_or(0, Vec::len),
                        binding["occurrences"].as_array().map_or(0, Vec::len)
                    ),
                ));
                binding_rows(&mut rows, &format!("Reviewed body {body}"), binding, state);
            }
        } else {
            binding_rows(
                &mut rows,
                "Previous",
                &review.value["previous_binding"],
                state,
            );
            binding_rows(&mut rows, "Proposed", &review.value["binding"], state);
        }
    }
    let effective = &state.effective[collection(profile)];
    for v in effective.as_array().into_iter().flatten().filter(|v| {
        v[if profile { "profile" } else { "range" }]["id"].as_str()
            == Some(&state.height_editor.selection)
    }) {
        rows.push(info(
            "Height binding status",
            format!("{} - current {}", v["binding"], v["binding_current"]),
        ));
        for (index, issue) in v["issues"].as_array().into_iter().flatten().enumerate() {
            rows.push(info(
                format!("Height binding issue {}", index + 1),
                issue.as_str().unwrap_or(""),
            ));
        }
        rows.push(info(
            "Height target support",
            if profile {
                format!("Variable schedule supported: {}", v["target_supported"])
            } else {
                format!(
                    "Unsupported settings {}; unsupported speeds {}",
                    v["unsupported"], v["unsupported_speeds"]
                )
            },
        ));
    }
    for (label, field, _, _) in &mut rows {
        if matches!(
            field,
            Some(super::Field::Height(
                Field::Min | Field::Max | Field::PointZ | Field::PointHeight
            ))
        ) {
            let unit = match state.units {
                limo_cad_core::UnitSystem::Mm => "mm",
                limo_cad_core::UnitSystem::Cm => "cm",
                limo_cad_core::UnitSystem::In => "in",
            };
            *label = format!("{label} ({unit})");
        }
    }
    rows
}
pub(super) fn disabled(
    state: &State,
    field: Option<super::Field>,
    command: Option<super::Command>,
) -> bool {
    if !state.height_scope {
        return false;
    }
    if matches!(
        field,
        Some(super::Field::Height(Field::Saved | Field::Layout))
    ) || matches!(command, Some(super::Command::Height(Command::Create)))
    {
        return false;
    }
    if state.height_editor.draft.is_none() {
        return matches!(field, Some(super::Field::Height(_)))
            || matches!(
                command,
                Some(super::Command::Height(_) | super::Command::Apply | super::Command::Inherit)
            );
    }
    if state.height_editor.profile && matches!(command, Some(super::Command::Inherit)) {
        return true;
    }
    false
}

pub(super) fn selected(state: &State, v: &Value) -> bool {
    v["id"].as_str() == Some(&state.height_editor.selection)
        && v["body_id"].as_u64() == Some(state.body)
}
pub(super) fn unchanged(state: &State, v: Option<&Value>) -> bool {
    state.height_editor.original.as_ref() == v
}

fn validate_record(value: &Value, profile: bool) -> Result<(), String> {
    if profile {
        serde_json::from_value::<PrintLayerHeightProfileDto>(value.clone())
            .map_err(|e| e.to_string())?
            .validate()
    } else {
        serde_json::from_value::<PrintHeightRangeDto>(value.clone())
            .map_err(|e| e.to_string())?
            .validate()
    }
}

fn binding_rows(rows: &mut Vec<Row>, prefix: &str, binding: &Value, state: &State) {
    for group in binding["groups"].as_array().into_iter().flatten() {
        rows.push(info(
            format!("{prefix} group {}", group["root_occurrence_id"]),
            format!(
                "Whole object: bed Z {}..{} mm",
                bound_text(&group["min_z_mm"]),
                bound_text(&group["max_z_mm"])
            ),
        ));
        for member in group["members"].as_array().into_iter().flatten() {
            let body = member["body_id"].to_string();
            let name = state.height_editor.context["body_names"][&body]
                .as_str()
                .unwrap_or("Source body");
            rows.push(info(
                format!(
                    "{prefix} group {} body {body} occurrence {}",
                    group["root_occurrence_id"], member["occurrence_id"]
                ),
                format!(
                    "{name}; body {body}; occurrence {}",
                    member["occurrence_id"]
                ),
            ));
        }
    }
    for pose in binding["occurrences"].as_array().into_iter().flatten() {
        if let Ok(pose_value) =
            serde_json::from_value::<limo_cad_core::PrintLocalPoseDto>(pose["pose"].clone())
        {
            let placement = assembly::TransformDraft::new(
                limo_cad_sketch::AssemblyTransformDto {
                    translation: pose_value.translation_mm,
                    rotation: pose_value.rotation,
                },
                limo_cad_core::UnitSystem::Mm,
            );
            let angles: Vec<_> = placement
                .rotation
                .iter()
                .map(|a| format!("{:.2}", a.text().parse::<f64>().unwrap_or_default()))
                .collect();
            let [x, y, z] = pose_value.translation_mm;
            rows.push(info(
                format!("{prefix} occurrence {} placement", pose["occurrence_id"]),
                format!("Body {}; X {x:.3}, Y {y:.3}, Z {z:.3} mm", pose["body_id"]),
            ));
            rows.push(info(
                format!("{prefix} occurrence {} rotation", pose["occurrence_id"]),
                format!("XYZ degrees: {}", angles.join(", ")),
            ));
        }
    }
}
fn layout_label(state: &State, layout: &Value) -> String {
    if layout["kind"] == "assembly" {
        return "Assembled source".into();
    }
    state.height_editor.context["views"]["views"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["id"] == layout["id"])
        .and_then(|v| v["name"].as_str())
        .map(|name| format!("Saved {name}"))
        .unwrap_or_else(|| "Saved layout (missing)".into())
}
pub(super) fn context_layout_mismatch(state: &State) -> bool {
    serde_json::from_value::<PrintHeightBindingDto>(state.height_editor.context["binding"].clone())
        .is_ok_and(|b| b.layout != layout(state))
}

pub(super) fn select_created(state: &mut State, document: &Value, previous_ids: &[String]) {
    if let Some(created) = document[collection(state.height_editor.profile)]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| {
            v["body_id"].as_u64() == Some(state.body)
                && v["id"]
                    .as_str()
                    .is_some_and(|id| !previous_ids.iter().any(|old| old == id))
        })
    {
        state.height_editor.selection = created["id"].as_str().unwrap().into();
    }
}

fn bound_text(value: &Value) -> String {
    value
        .as_f64()
        .map(|value| format!("{value:.3}"))
        .unwrap_or_else(|| "Unresolved".into())
}
