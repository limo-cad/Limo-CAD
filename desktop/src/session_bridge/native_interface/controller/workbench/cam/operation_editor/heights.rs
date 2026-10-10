//! Height reference controls preserve the engine's keyed associative intent.
use super::*;
use limo_cad_cam::{
    CamHeightReferenceDto, CamOperationHeightExpressionsDto, CamSetupDto, WorkCoordinateSystemDto,
};

mod context;
pub(in super::super) mod picking;
pub(super) use context::Context;
const PREFIX: &str = "/native/heights/";
const ROWS: [(&str, &str, &str); 5] = [
    ("bottom", "Bottom / target", "bottom_z"),
    ("top", "Cut top", "top_z"),
    ("feed", "Feed plane", "feed_height_z"),
    ("retract", "Retract", "retract_z"),
    ("clearance", "Clearance", "clearance_z"),
];

pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    context: &Context,
) -> Result<(), String> {
    let Selection::Operation(id) = draft.selection else {
        return Ok(());
    };
    let stored = cam.height_expressions.iter().find(|h| h.operation_id == id);
    let record = stored
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| e.to_string())?;
    form::push(
        draft,
        &format!("{PREFIX}mode"),
        "Height programming",
        InputKind::Choice,
        json!(if stored.is_some() {
            "associative"
        } else {
            "absolute"
        }),
        cam.units,
        Some(form::options(&[
            ("absolute", "Absolute setup Z"),
            ("associative", "Reference and offset"),
        ])),
    );
    for (index, (row, label, field)) in ROWS.into_iter().enumerate() {
        let label = if context.has_holes {
            match row {
                "bottom" => "Manual-center bottom",
                "top" => "Manual-center top",
                _ => label,
            }
        } else {
            label
        };
        let field = if row == "bottom" && draft.record["kind"] == "face" {
            "target_z"
        } else {
            field
        };
        let Some(baked) = draft.record.get(field).cloned() else {
            continue;
        };
        let expression = record.as_ref().and_then(|value| value.get(row));
        form::push(
            draft,
            &format!("{PREFIX}{row}/value"),
            &format!("{label} Z"),
            InputKind::Length,
            baked.clone(),
            cam.units,
            None,
        );
        let mut options = form::options(&[
            ("model_top", "Model top"),
            ("model_bottom", "Model bottom"),
            ("stock_top", "Stock top"),
            ("stock_bottom", "Stock bottom"),
            ("origin", "Setup origin"),
        ]);
        if context.has_holes {
            options.extend(form::options(&[
                ("hole_top", "Picked-hole top"),
                ("hole_bottom", "Picked-hole bottom"),
            ]));
        }
        if context.has_selection {
            options.extend(form::options(&[("selection", "Selected geometry")]));
        }
        for (lower, lower_label, _) in ROWS.iter().take(index) {
            if *lower == "bottom" && draft.record["kind"] == "chamfer2d" {
                continue;
            }
            options.extend(form::options(&[(lower, lower_label)]));
        }
        let reference = expression
            .and_then(|e| e.get("reference"))
            .cloned()
            .unwrap_or(json!("origin"));
        options.extend(form::options(&[("geometry", "Picked height geometry")]));
        if let Some(reference) = reference.as_str() {
            if !options.iter().any(|option| option.value == reference) {
                options.push(ChoiceOption {
                    value: reference.into(),
                    label: format!("{reference} (unavailable)"),
                    disabled: true,
                });
            }
        }
        form::push(
            draft,
            &format!("{PREFIX}{row}/reference"),
            &format!("{label} reference"),
            InputKind::Choice,
            reference,
            cam.units,
            Some(options),
        );
        let geometry = context.staged_geometry(row).cloned().or_else(|| {
            expression
                .and_then(|value| value.get("geometry"))
                .and_then(|value| serde_json::from_value(value.clone()).ok())
        });
        form::push(
            draft,
            &format!("/native/ui/height_kind/{row}"),
            "Geometry type",
            InputKind::Choice,
            json!(geometry.as_ref().map_or("face", picking::kind)),
            cam.units,
            Some(form::options(&[
                ("face", "Planar face"),
                ("edge", "Level edge"),
                ("vertex", "Vertex"),
                ("sketch_point", "Sketch point"),
                ("sketch_line", "Level sketch line"),
            ])),
        );
        let pick_label = geometry.as_ref().map(picking::label).map_or_else(
            || format!("{label} geometry"),
            |value| format!("{label} geometry: {value}"),
        );
        form::push(
            draft,
            &picking::button(row),
            &pick_label,
            InputKind::Boolean,
            geometry
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| error.to_string())?
                .map_or(json!(false), |key| json!(key)),
            cam.units,
            None,
        );
        form::push(
            draft,
            &format!("{PREFIX}{row}/offset"),
            &format!("{label} offset"),
            InputKind::Length,
            expression
                .and_then(|e| e.get("offset"))
                .cloned()
                .unwrap_or(baked),
            cam.units,
            None,
        );
    }
    Ok(())
}

pub(super) fn handles(path: &str) -> bool {
    path.starts_with(PREFIX) || path.starts_with("/native/ui/height_kind/")
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    let kind = path.strip_prefix("/native/ui/height_kind/");
    let normalized = kind.map(|row| format!("{row}/kind"));
    let path = normalized.as_deref().unwrap_or(path);
    let Some(path) = path.strip_prefix(PREFIX).or_else(|| kind.map(|_| path)) else {
        return true;
    };
    if path == "mode" {
        return true;
    }
    if path.starts_with("top/")
        && draft
            .operation_edit
            .as_ref()
            .is_some_and(|context| context.heights.modeled_top.is_some())
    {
        return false;
    }
    if form::text(draft, &format!("{PREFIX}mode")).unwrap_or("") == "absolute" {
        path.ends_with("/value")
    } else if path.ends_with("/pick") || path.ends_with("/kind") {
        let Some((row, _)) = path.split_once('/') else {
            return false;
        };
        form::text(draft, &format!("{PREFIX}{row}/reference")).unwrap_or("") == "geometry"
    } else {
        !path.ends_with("/value")
    }
}

pub(super) fn apply(
    draft: &Draft,
    record: &mut Value,
    cam: &mut CamDocumentDto,
    context: &Context,
    force: bool,
) -> Result<(), String> {
    if !force && !form::changed(draft, PREFIX) {
        return Ok(());
    }
    let Selection::Operation(id) = draft.selection else {
        return Err("Select a toolpath".into());
    };
    let mode = form::text(draft, &format!("{PREFIX}mode"))?;
    let saved_intent = cam
        .height_expressions
        .iter()
        .find(|entry| entry.operation_id == id)
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| error.to_string())?;
    let number = |path: &str, unchanged: f64| -> Result<f64, String> {
        if form::changed(draft, path) {
            form::number(draft, path, cam.units)
        } else {
            Ok(unchanged)
        }
    };
    let mut values = HashMap::<&str, f64>::new();
    let mut intent = json!({"operation_id":id});
    for (row, _, field) in ROWS {
        let field = if row == "bottom" && record["kind"] == "face" {
            "target_z"
        } else {
            field
        };
        if record.get(field).is_none() {
            continue;
        }
        let value = if mode == "absolute" {
            number(
                &format!("{PREFIX}{row}/value"),
                record[field].as_f64().ok_or("Missing saved height")?,
            )?
        } else if mode == "associative" {
            let reference = form::text(draft, &format!("{PREFIX}{row}/reference"))?;
            let saved_offset = saved_intent
                .as_ref()
                .and_then(|intent| intent[row]["offset"].as_f64())
                .or_else(|| draft.record[field].as_f64())
                .ok_or("Missing saved height offset")?;
            let offset = number(&format!("{PREFIX}{row}/offset"), saved_offset)?;
            intent[row] = json!({"reference":reference,"offset":offset});
            let parsed: CamHeightReferenceDto = serde_json::from_value(json!(reference))
                .map_err(|_| "Choose a height reference")?;
            let geometry = if parsed == CamHeightReferenceDto::Geometry {
                let geometry = if let Some(geometry) = context.staged_geometry(row) {
                    geometry.clone()
                } else {
                    let saved = saved_intent
                        .as_ref()
                        .and_then(|intent| intent[row].get("geometry"))
                        .filter(|value| !value.is_null())
                        .ok_or("Pick geometry for this height before applying")?;
                    serde_json::from_value::<limo_cad_cam::CamHeightGeometryDto>(saved.clone())
                        .map_err(|error| error.to_string())?
                };
                intent[row]["geometry"] =
                    serde_json::to_value(&geometry).map_err(|error| error.to_string())?;
                Some(geometry)
            } else {
                None
            };
            if let Some(modeled_top) = context.modeled_top.filter(|_| row == "top") {
                modeled_top
            } else {
                let base = match geometry {
                    Some(geometry) => context.geometry_base(&geometry)?,
                    None => context.base(parsed, &values)?,
                };
                base + offset
            }
        } else {
            return Err("Choose a height programming mode".into());
        };
        if !value.is_finite() {
            return Err("Resolved height must be finite".into());
        }
        values.insert(row, value);
        if row != "top" || record["modeled_chamfer"].is_null() {
            record[field] = json!(value);
        }
        if row == "top"
            && (mode == "associative" || form::changed(draft, &format!("{PREFIX}top/value")))
        {
            if let Some(chains) = record
                .get_mut("additional_chains")
                .and_then(Value::as_array_mut)
            {
                for chain in chains
                    .iter_mut()
                    .filter(|chain| chain["modeled_chamfer"].is_null())
                {
                    chain["top_z"] = json!(value);
                }
            }
        }
    }
    let next_intent = if mode == "associative" {
        let entry: CamOperationHeightExpressionsDto =
            serde_json::from_value(intent).map_err(|e| e.to_string())?;
        let op: CamOperationDto =
            serde_json::from_value(record.clone()).map_err(|e| e.to_string())?;
        entry.validate_for_operation(&op)?;
        Some(entry)
    } else {
        None
    };
    let index = cam
        .height_expressions
        .iter()
        .position(|entry| entry.operation_id == id);
    match (index, next_intent) {
        (Some(index), Some(entry)) => cam.height_expressions[index] = entry,
        (None, Some(entry)) => cam.height_expressions.push(entry),
        (Some(_), None) => cam
            .height_expressions
            .retain(|entry| entry.operation_id != id),
        (None, None) => {}
    }
    Ok(())
}
