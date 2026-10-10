use super::*;

pub(super) const COUNT: &str = "/native/geometry/chain_count";
const CURRENT: &str = "/native/ui/geometry_chain";

pub(super) fn prefix(index: usize) -> String {
    format!("{PREFIX}chains/{index}")
}
pub(super) fn chain(record: &Value, index: usize) -> Option<Value> {
    if index == 0 {
        Some(record.clone())
    } else {
        record["additional_chains"].get(index - 1).cloned()
    }
}
fn chain_count(record: &Value) -> usize {
    1 + record["additional_chains"].as_array().map_or(0, Vec::len)
}
pub(super) fn active(draft: &Draft) -> Option<usize> {
    let index = form::text(draft, CURRENT)
        .ok()?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    (index < count(draft, COUNT, 64).ok()?).then_some(index)
}
fn path_points(record: &Value) -> &[Value] {
    record
        .get("outline")
        .unwrap_or(&record["path"])
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}
pub(super) fn source(draft: &Draft, prefix: &str) -> String {
    form::text(draft, &format!("{prefix}/source"))
        .unwrap_or("manual")
        .into()
}
pub(super) fn keys(record: &Value) -> Vec<String> {
    record["chain_ref"]["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}
pub(super) fn key_cursor(prefix: &str) -> String {
    format!(
        "/native/ui/geometry_edge{}",
        prefix.strip_prefix(PREFIX).unwrap_or(prefix)
    )
}

pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    context: &Context,
) -> Result<(), String> {
    form::push(
        draft,
        COUNT,
        "Chamfer chain count",
        InputKind::Integer,
        json!(chain_count(&draft.record)),
        cam.units,
        None,
    );
    form::push(
        draft,
        CURRENT,
        "Chamfer chain number",
        InputKind::Integer,
        json!(1),
        cam.units,
        None,
    );
    extend_active(draft, cam, context)
}
pub(super) fn extend_active(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    context: &Context,
) -> Result<(), String> {
    let index = active(draft).ok_or("Choose an available chain number")?;
    let prefix = prefix(index);
    let stored = chain(&draft.record, index).unwrap_or_else(|| json!({
        "path":[],"closed":true,"chain_ref":null,"modeled_chamfer":null,
        "top_z":draft.record["top_z"],"chamfer_width":draft.record["chamfer_width"],"wall_side":draft.record["wall_side"]
    }));
    if !draft
        .fields
        .iter()
        .any(|field| field.path == format!("{prefix}/source"))
    {
        let current = stored["chain_ref"]["source"].as_str().unwrap_or("manual");
        form::push(
            draft,
            &format!("{prefix}/source"),
            "Geometry source",
            InputKind::Choice,
            json!(current),
            cam.units,
            Some(form::options(&[
                ("model", "Model edges"),
                ("sketch", "Sketch curves"),
                ("manual", "Manual setup XY"),
            ])),
        );
        form::push(
            draft,
            &format!("{prefix}/mode"),
            "Edge selection",
            InputKind::Choice,
            json!("manual"),
            cam.units,
            Some(form::options(&[
                ("manual", "Selected edges"),
                ("closed", "Closed loop from one edge"),
            ])),
        );
        form::push(
            draft,
            &picking::button_path(index),
            "Viewport geometry",
            InputKind::Name,
            json!(""),
            cam.units,
            None,
        );
        form::push(
            draft,
            &format!("{prefix}/key_count"),
            "Selected edge count",
            InputKind::Integer,
            json!(keys(&stored).len()),
            cam.units,
            None,
        );
        form::push(
            draft,
            &key_cursor(&prefix),
            "Selected edge number",
            InputKind::Integer,
            json!(1),
            cam.units,
            None,
        );
        boolean(
            draft,
            &format!("{prefix}/reversed"),
            "Reverse chain",
            stored["chain_ref"]["reversed"].as_bool().unwrap_or(false),
            cam.units,
        );
        boolean(
            draft,
            &format!("{prefix}/closed"),
            "Closed manual path",
            stored["closed"].as_bool().unwrap_or(true),
            cam.units,
        );
        if draft.record["kind"] == "chamfer2d" {
            boolean(
                draft,
                &format!("{prefix}/modeled"),
                "Use modeled bevel",
                !stored["modeled_chamfer"].is_null(),
                cam.units,
            );
            for (field, label, value) in [
                ("top_z", "Chain top Z", stored["top_z"].clone()),
                (
                    "chamfer_width",
                    "Chain chamfer width",
                    stored["chamfer_width"].clone(),
                ),
                (
                    "additional_width",
                    "Chain additional width",
                    stored["modeled_chamfer"]
                        .get("additional_width")
                        .cloned()
                        .unwrap_or(json!(0.)),
                ),
            ] {
                form::push(
                    draft,
                    &format!("{prefix}/{field}"),
                    label,
                    InputKind::Length,
                    value,
                    cam.units,
                    None,
                );
            }
            form::push(
                draft,
                &format!("{prefix}/wall_side"),
                "Chain material wall side",
                InputKind::Choice,
                stored.get("wall_side").cloned().unwrap_or(json!("outside")),
                cam.units,
                Some(form::options(&[
                    ("inside", "Inside"),
                    ("outside", "Outside"),
                    ("left", "Left of travel"),
                    ("right", "Right of travel"),
                ])),
            );
        }
    }
    picking::initialize(draft, index, cam.units);
    points::extend(
        draft,
        &format!("{prefix}/points"),
        "Path point",
        path_points(&stored),
        cam.units,
    );
    extend_key(draft, &prefix, &stored, cam.units, context)
}
fn extend_key(
    draft: &mut Draft,
    prefix: &str,
    stored: &Value,
    units: CamUnits,
    context: &Context,
) -> Result<(), String> {
    let count = count(draft, &format!("{prefix}/key_count"), 20_000)?;
    if count == 0 {
        picking::retain_key_options(draft, None);
        return Ok(());
    }
    let index = form::text(draft, &key_cursor(prefix))?
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|index| *index < count)
        .ok_or_else(|| format!("Choose an edge number from 1 to {count}"))?;
    let path = format!("{prefix}/keys/{index}");
    picking::retain_key_options(draft, Some(&path));
    let mut options = if source(draft, prefix) == "model" {
        context.model_options.clone()
    } else {
        context.sketch_options.clone()
    };
    let value = form::text(draft, &path)
        .map(str::to_owned)
        .unwrap_or_else(|_| keys(stored).get(index).cloned().unwrap_or_default());
    if !value.is_empty() && !options.iter().any(|option| option.value == value) {
        options.push(ChoiceOption {
            value: value.clone(),
            label: "Saved edge is unavailable".into(),
            disabled: true,
        });
    }
    if let Some(field) = draft.fields.iter_mut().find(|field| field.path == path) {
        field.options = Some(options);
    } else {
        form::push(
            draft,
            &path,
            &format!("Edge {}", index + 1),
            InputKind::Choice,
            json!(value),
            units,
            Some(options),
        );
    }
    Ok(())
}

pub(super) fn changed(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    path: &str,
    context: &Context,
) -> Result<(), String> {
    let length = count(draft, COUNT, 64)?;
    if length == 0 {
        return Err("A toolpath needs at least one chain".into());
    }
    if path == COUNT {
        let current = active(draft).map_or(length, |index| index + 1).min(length);
        let next = if length > chain_count(&draft.record) {
            length
        } else {
            current
        };
        form::set(draft, CURRENT, &next.to_string());
    }
    let index = active(draft).ok_or_else(|| format!("Choose a chain number from 1 to {length}"))?;
    let prefix = prefix(index);
    picking::changed(draft, cam, index, path)?;
    if path == format!("{prefix}/mode") && form::text(draft, path)? == "closed" {
        form::set(draft, &format!("{prefix}/key_count"), "1");
        form::set(draft, &key_cursor(&prefix), "1");
    }
    if path == format!("{prefix}/source") && source(draft, &prefix) != "model" {
        form::set(draft, &format!("{prefix}/modeled"), "false");
    }
    if path == format!("{prefix}/key_count") {
        let count = count(draft, path, 20_000)?;
        let next = count.max(1);
        form::set(draft, &key_cursor(&prefix), &next.to_string());
    }
    let stored = chain(&draft.record, index).unwrap_or(Value::Null);
    let point_prefix = format!("{prefix}/points");
    if path.starts_with(&point_prefix) || path == points::cursor(&point_prefix) {
        points::changed(
            draft,
            &point_prefix,
            "Path point",
            path,
            path_points(&stored),
            cam.units,
        )?;
    }
    extend_active(draft, cam, context)
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if path == COUNT || path == CURRENT {
        return draft.record["kind"] == "chamfer2d";
    }
    let Some(index) = active(draft) else {
        return false;
    };
    let prefix = prefix(index);
    let source = source(draft, &prefix);
    if path == picking::button_path(index) {
        return source != "manual";
    }
    let point_prefix = format!("{prefix}/points");
    if path.starts_with(&point_prefix) || path == points::cursor(&point_prefix) {
        return source == "manual" && points::visible(draft, &point_prefix, path);
    }
    if path == key_cursor(&prefix) {
        return source != "manual"
            && count(draft, &format!("{prefix}/key_count"), 20_000).unwrap_or(0) > 0;
    }
    let Some(suffix) = path.strip_prefix(&format!("{prefix}/")) else {
        return false;
    };
    let modeled = form::text(draft, &format!("{prefix}/modeled")).is_ok_and(|s| s == "true");
    match suffix {
        "source" => true,
        "closed" => source == "manual" && draft.record["kind"] != "pocket2d",
        "mode" | "key_count" | "reversed" => source != "manual",
        "modeled" => source == "model",
        "additional_width" => modeled,
        "top_z" => {
            !modeled
                && form::text(draft, "/native/heights/mode").is_ok_and(|mode| mode == "absolute")
        }
        "chamfer_width" | "wall_side" => !modeled,
        _ if suffix.starts_with("keys/") => {
            source != "manual"
                && suffix
                    .strip_prefix("keys/")
                    .and_then(|s| s.parse::<usize>().ok())
                    == form::text(draft, &key_cursor(&prefix))
                        .ok()
                        .and_then(|s| s.parse::<usize>().ok())
                        .and_then(|n| n.checked_sub(1))
        }
        _ => false,
    }
}

pub(super) fn apply(
    draft: &Draft,
    record: &mut Value,
    units: CamUnits,
    context: &Context,
) -> Result<(), String> {
    let count = count(draft, COUNT, 64)?;
    if count == 0 || (record["kind"] != "chamfer2d" && count != 1) {
        return Err("Choose a valid chain count".into());
    }
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let prefix = prefix(index);
        let original = chain(record, index);
        if !form::changed(draft, &format!("{prefix}/")) {
            if let Some(original) = original {
                result.push(original);
                continue;
            }
        }
        let mut next=original.clone().unwrap_or_else(||json!({"path":[],"closed":true,"chain_ref":null,"modeled_chamfer":null,
            "top_z":record["top_z"],"chamfer_width":record["chamfer_width"],"wall_side":record["wall_side"]}));
        let source = source(draft, &prefix);
        let modeled = record["kind"] == "chamfer2d"
            && form::text(draft, &format!("{prefix}/modeled"))? == "true";
        if source == "manual" {
            let points = points::read(
                draft,
                &format!("{prefix}/points"),
                path_points(&next),
                units,
            )?;
            let key = if record["kind"] == "pocket2d" {
                "outline"
            } else {
                "path"
            };
            next[key] = serde_json::to_value(points).map_err(|e| e.to_string())?;
            next["closed"] = json!(
                record["kind"] == "pocket2d"
                    || form::text(draft, &format!("{prefix}/closed"))? == "true"
            );
            next["chain_ref"] = Value::Null;
        } else {
            let chain_source = match source.as_str() {
                "model" => ChainSource::Model,
                "sketch" => ChainSource::Sketch,
                _ => return Err("Choose a geometry source".into()),
            };
            let n = count_value(draft, &format!("{prefix}/key_count"))?;
            let stored = keys(&next);
            let keys = (0..n)
                .map(|i| {
                    form::text(draft, &format!("{prefix}/keys/{i}"))
                        .map(str::to_owned)
                        .or_else(|_| {
                            stored
                                .get(i)
                                .cloned()
                                .ok_or_else(|| "Choose every selected edge".to_string())
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let reversed = form::text(draft, &format!("{prefix}/reversed"))? == "true";
            let mode = if form::text(draft, &format!("{prefix}/mode"))? == "closed"
                && !picking::resolved(draft, index, &keys)
            {
                ChainMode::Closed
            } else {
                ChainMode::Manual
            };
            let resolved = limo_cad_sketch::resolve_edge_chain(
                &context.scene,
                &context.sketches,
                &EdgeChainRequest {
                    source: chain_source,
                    body_ids: context.setup.body_ids.clone(),
                    normal: Some(context.setup.wcs.z_axis),
                    keys,
                    mode,
                    reversed,
                },
            )?;
            let projected = resolved
                .points
                .iter()
                .map(|point| project(*point, &context.setup))
                .collect::<Vec<_>>();
            if record["kind"] == "pocket2d" && !resolved.closed {
                return Err("A pocket needs a closed boundary".into());
            }
            if (record["kind"] == "pocket2d" || (record["kind"] == "chamfer2d" && !modeled))
                && projected.iter().any(|point| {
                    (point[2] - projected[0][2]).abs() > limo_cad_core::edge_chain::JOIN_TOLERANCE
                })
            {
                return Err(
                    "A pocket or sharp chamfer boundary must lie in one setup-Z plane".into(),
                );
            }
            let reference: CamChainRefDto = serde_json::from_value(
                json!({"source":source,"keys":resolved.keys,"reversed":reversed}),
            )
            .map_err(|e| e.to_string())?;
            if modeled {
                let geometry = limo_cad_sketch::resolve_cam_chamfer_geometry(
                    &context.scene,
                    &context.setup,
                    &reference,
                )?;
                let allowance = number(
                    draft,
                    &format!("{prefix}/additional_width"),
                    next["modeled_chamfer"]["additional_width"]
                        .as_f64()
                        .or(Some(0.)),
                    units,
                )?;
                next["path"] = serde_json::to_value(geometry.path).map_err(|e| e.to_string())?;
                next["closed"] = json!(geometry.closed);
                next["top_z"] = json!(geometry.top_z);
                next["chamfer_width"] = json!(geometry.width + allowance);
                next["wall_side"] =
                    serde_json::to_value(geometry.wall_side).map_err(|e| e.to_string())?;
                next["modeled_chamfer"] = json!({"additional_width":allowance});
            } else {
                let key = if record["kind"] == "pocket2d" {
                    "outline"
                } else {
                    "path"
                };
                next[key] = json!(projected
                    .into_iter()
                    .map(|p| Point2Dto::new(p[0], p[1]))
                    .collect::<Vec<_>>());
                next["closed"] = json!(resolved.closed);
            }
            next["chain_ref"] = serde_json::to_value(reference).map_err(|e| e.to_string())?;
        }
        if record["kind"] == "chamfer2d" && !modeled {
            next["modeled_chamfer"] = Value::Null;
            for key in ["top_z", "chamfer_width"] {
                next[key] = json!(number(
                    draft,
                    &format!("{prefix}/{key}"),
                    next[key].as_f64(),
                    units
                )?);
            }
            next["wall_side"] = json!(form::text(draft, &format!("{prefix}/wall_side"))?);
        }
        result.push(next);
    }
    let first = result.remove(0);
    for key in [
        "path",
        "outline",
        "closed",
        "chain_ref",
        "top_z",
        "chamfer_width",
        "wall_side",
        "modeled_chamfer",
    ] {
        if let Some(value) = first.get(key) {
            if record.get(key).is_some() {
                record[key] = value.clone();
            }
        }
    }
    if record["kind"] == "chamfer2d" {
        let chains = result
            .into_iter()
            .map(serde_json::from_value::<limo_cad_cam::CamChamferChainDto>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        record["additional_chains"] = serde_json::to_value(chains).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn count_value(draft: &Draft, path: &str) -> Result<usize, String> {
    count(draft, path, 20_000)
}
