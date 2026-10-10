use super::*;

const POINTS: &str = "/native/geometry/centers";
pub(super) const COUNT: &str = "/native/geometry/hole_count";
pub(super) const CURRENT: &str = "/native/ui/geometry_hole";

pub(super) fn candidates(setup: &CamSetupDto, scene: &SolidSceneDto) -> Vec<(String, CamHoleDto)> {
    let mut result = Vec::new();
    for body in scene
        .bodies
        .iter()
        .filter(|body| setup.body_ids.contains(&body.id))
    {
        for face in body.faces.iter().filter(|face| face.cylinder.is_some()) {
            let reference = format!("{}:{}", body.id.0, face.id.0);
            let mut hole = CamHoleDto {
                point: Point2Dto::new(0., 0.),
                top_z: 0.,
                bottom_z: 0.,
                axis: [0., 0., 1.],
                face_key: Some(reference.clone()),
            };
            if limo_cad_sketch::resolve_cam_hole_reference(&reference, &mut hole, setup, scene)
                .is_ok()
            {
                let diameter = face.cylinder.unwrap().radius * 2.;
                result.push((
                    format!("{} · cylinder {} · Ø{diameter:.3} mm", body.name, face.id.0),
                    hole,
                ));
            }
        }
    }
    result
}
pub(super) fn prefix(index: usize) -> String {
    format!("{PREFIX}holes/{index}")
}
fn stored_points(record: &Value) -> &[Value] {
    record["points"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn stored_holes(record: &Value) -> &[Value] {
    record["holes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn active(draft: &Draft) -> Option<usize> {
    let index = form::text(draft, CURRENT)
        .ok()?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    (index < count(draft, COUNT, 250_000).ok()?).then_some(index)
}
pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    context: &Context,
) -> Result<(), String> {
    let points = stored_points(&draft.record).to_vec();
    points::extend(draft, POINTS, "Manual center", &points, cam.units);
    form::push(
        draft,
        COUNT,
        "Hole count",
        InputKind::Integer,
        json!(stored_holes(&draft.record).len()),
        cam.units,
        None,
    );
    form::push(
        draft,
        CURRENT,
        "Hole number",
        InputKind::Integer,
        json!(1),
        cam.units,
        None,
    );
    form::push(
        draft,
        hole_picking::BUTTON,
        "Viewport geometry",
        InputKind::Name,
        json!(""),
        cam.units,
        None,
    );
    extend_active(draft, cam.units, context)
}
fn extend_active(draft: &mut Draft, units: CamUnits, context: &Context) -> Result<(), String> {
    let index = active(draft);
    let active_face = index.map(|index| format!("{}/face", prefix(index)));
    for field in &mut draft.fields {
        if field.path.starts_with("/native/geometry/holes/")
            && field.path.ends_with("/face")
            && Some(field.path.as_str()) != active_face.as_deref()
        {
            field.options = None;
        }
    }
    let Some(index) = index else {
        return Ok(());
    };
    let prefix = prefix(index);
    if let Some(field) = draft
        .fields
        .iter_mut()
        .find(|field| Some(field.path.as_str()) == active_face.as_deref())
    {
        if field.options.is_none() {
            field.options = Some(face_options(context, field.text.trim()));
        }
        return Ok(());
    }
    let original = hole_picking::baseline(draft, context, index)?;
    let source = if original.is_null() || original["face_key"].is_string() {
        "face"
    } else {
        "manual"
    };
    form::push(
        draft,
        &format!("{prefix}/source"),
        "Hole source",
        InputKind::Choice,
        json!(source),
        units,
        Some(form::options(&[
            ("face", "Picked cylindrical face"),
            ("manual", "Explicit hole span"),
        ])),
    );
    let reference = original["face_key"].as_str().unwrap_or("");
    let options = face_options(context, reference);
    form::push(
        draft,
        &format!("{prefix}/face"),
        "Cylindrical face",
        InputKind::Choice,
        json!(reference),
        units,
        Some(options),
    );
    for (path, label, value) in [
        ("x", "Hole center X", original["point"]["x"].clone()),
        ("y", "Hole center Y", original["point"]["y"].clone()),
        ("top_z", "Hole top Z", original["top_z"].clone()),
        ("bottom_z", "Hole bottom Z", original["bottom_z"].clone()),
    ] {
        form::push(
            draft,
            &format!("{prefix}/{path}"),
            label,
            InputKind::Length,
            value,
            units,
            None,
        );
    }
    form::push(
        draft,
        &format!("{prefix}/axis"),
        "Hole axis",
        InputKind::Choice,
        json!(if original["axis"][2].as_f64().unwrap_or(1.) < 0. {
            "down"
        } else {
            "up"
        }),
        units,
        Some(form::options(&[("up", "Setup +Z"), ("down", "Setup −Z")])),
    );
    Ok(())
}
fn face_options(context: &Context, reference: &str) -> Vec<ChoiceOption> {
    let mut options = context
        .holes
        .iter()
        .map(|(label, hole)| ChoiceOption {
            value: hole.face_key.clone().unwrap(),
            label: label.clone(),
            disabled: false,
        })
        .collect::<Vec<_>>();
    if !reference.is_empty() && !options.iter().any(|option| option.value == reference) {
        options.push(ChoiceOption {
            value: reference.into(),
            label: "Saved cylindrical face is unavailable".into(),
            disabled: true,
        });
    }
    options
}
pub(super) fn changed(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    path: &str,
    context: &Context,
) -> Result<(), String> {
    if path.starts_with(POINTS) || path == points::cursor(POINTS) {
        let original = stored_points(&draft.record).to_vec();
        return points::changed(draft, POINTS, "Manual center", path, &original, cam.units);
    }
    let length = count(draft, COUNT, 250_000)?;
    if path == COUNT {
        let next = if length > stored_holes(&draft.record).len() {
            length
        } else {
            active(draft).map_or(1, |n| n + 1).min(length.max(1))
        };
        form::set(draft, CURRENT, &next.to_string());
    }
    if length > 0 && active(draft).is_none() {
        return Err(format!("Choose a hole number from 1 to {length}"));
    }
    extend_active(draft, cam.units, context)
}
pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if path.starts_with(POINTS) || path == points::cursor(POINTS) {
        return points::visible(draft, POINTS, path);
    }
    if path == COUNT || path == hole_picking::BUTTON {
        return true;
    }
    if path == CURRENT {
        return count(draft, COUNT, 250_000).unwrap_or(0) > 0;
    }
    let Some(index) = active(draft) else {
        return false;
    };
    let prefix = prefix(index);
    let Some(suffix) = path.strip_prefix(&format!("{prefix}/")) else {
        return false;
    };
    let picked = form::text(draft, &format!("{prefix}/source")).is_ok_and(|s| s == "face");
    match suffix {
        "source" => true,
        "face" => picked,
        "x" | "y" | "top_z" | "bottom_z" | "axis" => !picked,
        _ => false,
    }
}
pub(super) fn apply(
    draft: &Draft,
    record: &mut Value,
    units: CamUnits,
    context: &Context,
) -> Result<(), String> {
    if form::changed(draft, POINTS) {
        record["points"] =
            serde_json::to_value(points::read(draft, POINTS, stored_points(record), units)?)
                .map_err(|e| e.to_string())?;
    }
    let length = count(draft, COUNT, 250_000)?;
    let baselines = hole_picking::baselines(draft, context)?;
    let canonical = canonical_holes(context);
    let mut holes = Vec::with_capacity(length);
    for (index, original) in baselines.into_iter().enumerate() {
        let prefix = prefix(index);
        if !form::changed(draft, &format!("{prefix}/")) && !original.is_null() {
            holes.push(resolve_association(original, context, &canonical, index)?);
            continue;
        }
        match form::text(draft, &format!("{prefix}/source"))? {
            "face" => {
                let reference = form::text(draft, &format!("{prefix}/face"))?;
                let mut value = if original.is_null() {
                    json!({"point":{"x":0.,"y":0.},"top_z":0.,"bottom_z":0.,
                        "axis":[0.,0.,1.],"face_key":reference})
                } else {
                    original.clone()
                };
                value["face_key"] = json!(reference);
                holes.push(resolve_association(value, context, &canonical, index)?);
            }
            "manual" => {
                let value = |key, old| number(draft, &format!("{prefix}/{key}"), old, units);
                let axis = if !form::changed(draft, &format!("{prefix}/axis"))
                    && original["axis"].is_array()
                {
                    original["axis"].clone()
                } else {
                    match form::text(draft, &format!("{prefix}/axis"))? {
                        "up" => json!([0., 0., 1.]),
                        "down" => json!([0., 0., -1.]),
                        _ => return Err("Choose a hole axis".into()),
                    }
                };
                holes.push(json!({"point":{"x":value("x",original["point"]["x"].as_f64())?,"y":value("y",original["point"]["y"].as_f64())?},
                    "top_z":value("top_z",original["top_z"].as_f64())?,"bottom_z":value("bottom_z",original["bottom_z"].as_f64())?,"axis":axis,"face_key":null}));
            }
            _ => return Err("Choose a hole source".into()),
        }
    }
    record["holes"] = json!(holes);
    Ok(())
}

pub(super) fn canonical_holes(context: &Context) -> HashMap<hole_picking::FaceKey, &CamHoleDto> {
    context
        .holes
        .iter()
        .filter_map(|(_, hole)| {
            Some((
                hole_picking::FaceKey::parse(hole.face_key.as_deref()?).ok()?,
                hole,
            ))
        })
        .collect()
}
fn resolve_association(
    value: Value,
    context: &Context,
    canonical: &HashMap<hole_picking::FaceKey, &CamHoleDto>,
    index: usize,
) -> Result<Value, String> {
    let Some(reference) = value["face_key"].as_str() else {
        return Ok(value);
    };
    let key = hole_picking::FaceKey::parse(reference)?;
    if !context
        .setup
        .body_ids
        .iter()
        .any(|body| body.0 == key.body_id)
    {
        return Err(format!(
            "Hole {} references a body outside this setup; remove or reselect {reference}",
            index + 1
        ));
    }
    if !context.scene.errors.is_empty() {
        return Err("Resolve model errors before applying associated CAM holes".into());
    }
    if let Some(hole) = canonical.get(&key) {
        let mut resolved = (**hole).clone();
        resolved.face_key = Some(reference.into());
        return serde_json::to_value(resolved).map_err(|error| error.to_string());
    }
    let mut hole: CamHoleDto = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    limo_cad_sketch::resolve_cam_hole_reference(
        reference,
        &mut hole,
        &context.setup,
        &context.scene,
    )
    .map_err(|error| {
        format!(
            "Hole {}: {error}; remove or reselect {reference}",
            index + 1
        )
    })?;
    serde_json::to_value(hole).map_err(|error| error.to_string())
}
