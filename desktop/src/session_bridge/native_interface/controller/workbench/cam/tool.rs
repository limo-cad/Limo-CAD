//! Cutter controls edit the project tool, including its existing cutting defaults.
use super::*;
use limo_cad_cam::CamUnits;

pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    creating: bool,
) -> Result<(), String> {
    use InputKind::*;
    let record = draft.record.clone();
    let value = |path: &str, fallback: Value| record.pointer(path).cloned().unwrap_or(fallback);
    form::push(
        draft,
        "/kind",
        "Cutter type",
        Choice,
        value("/kind", json!("flat_end_mill")),
        cam.units,
        Some(form::options(&[
            ("flat_end_mill", "Flat end mill"),
            ("ball_end_mill", "Ball end mill"),
            ("bull_nose_end_mill", "Bull-nose end mill"),
            ("face_mill", "Face mill"),
            ("drill", "Drill"),
            ("chamfer_mill", "Chamfer mill"),
            ("tap", "Tap"),
            ("reamer", "Reamer"),
            ("boring_bar", "Boring bar"),
            ("thread_mill", "Thread mill"),
            ("turning_general", "Turning tool (reserved)"),
        ])),
    );
    if let Some(options) = draft
        .fields
        .last_mut()
        .and_then(|field| field.options.as_mut())
    {
        for option in options
            .iter_mut()
            .filter(|option| option.value == "turning_general")
        {
            option.disabled = true;
        }
    }
    form::push(
        draft,
        "/center_cutting",
        "Center cutting",
        Boolean,
        value("/center_cutting", json!(true)),
        cam.units,
        Some(form::options(&[("true", "Yes"), ("false", "No")])),
    );
    form::push(
        draft,
        "/point_angle_degrees",
        "Point angle (degrees)",
        OptionalNumber,
        value("/point_angle_degrees", Value::Null),
        cam.units,
        None,
    );
    let shape = if record.get("corner_chamfer").is_some_and(|v| !v.is_null()) {
        "chamfer"
    } else if record.get("corner_radius").is_some_and(|v| !v.is_null())
        || record["kind"] == "bull_nose_end_mill"
    {
        "radius"
    } else {
        "sharp"
    };
    form::push(
        draft,
        "/native/corner/shape",
        "Cutting corner",
        Choice,
        json!(shape),
        cam.units,
        Some(form::options(&[
            ("sharp", "Sharp"),
            ("radius", "Radius"),
            ("chamfer", "Chamfer"),
        ])),
    );
    form::push(
        draft,
        "/native/corner/radius",
        "Corner radius",
        OptionalLength,
        value("/corner_radius", Value::Null),
        cam.units,
        None,
    );
    form::push(
        draft,
        "/native/corner/width",
        "Corner chamfer width",
        OptionalLength,
        value("/corner_chamfer/width", Value::Null),
        cam.units,
        None,
    );
    form::push(
        draft,
        "/native/corner/angle",
        "Corner chamfer angle (degrees)",
        Number,
        value("/corner_chamfer/angle_degrees", json!(45.)),
        cam.units,
        None,
    );
    for (path, label) in [
        ("/default_step_down", "Default step down (optional)"),
        ("/default_step_over", "Default step over (optional)"),
    ] {
        form::push(
            draft,
            path,
            label,
            OptionalLength,
            value(path, Value::Null),
            cam.units,
            None,
        );
    }
    if !creating {
        for (path, label, kind) in [
            ("/cutting/spindle_rpm", "Default spindle (rpm)", Integer),
            ("/cutting/feed_xy", "Default cutting feed", Feed),
            ("/cutting/feed_z", "Default plunge feed", Feed),
        ] {
            form::push(
                draft,
                path,
                label,
                kind,
                value(path, Value::Null),
                cam.units,
                None,
            );
        }
    }
    form::push(
        draft,
        "/cutting/coolant",
        "Default coolant",
        Choice,
        value("/cutting/coolant", json!("off")),
        cam.units,
        Some(form::options(&[
            ("off", "Off"),
            ("mist", "Mist"),
            ("flood", "Flood"),
        ])),
    );
    Ok(())
}

fn radius_capable(kind: &str) -> bool {
    matches!(kind, "flat_end_mill" | "bull_nose_end_mill" | "face_mill")
}
pub(super) fn changed(draft: &mut Draft, path: &str) {
    if path != "/kind" {
        return;
    }
    let kind = form::text(draft, "/kind").unwrap_or("").to_owned();
    form::set(
        draft,
        "/point_angle_degrees",
        match kind.as_str() {
            "drill" => "118",
            "chamfer_mill" => "90",
            _ => "",
        },
    );
    form::set(
        draft,
        "/native/corner/shape",
        if kind == "bull_nose_end_mill" {
            "radius"
        } else {
            "sharp"
        },
    );
    if matches!(
        kind.as_str(),
        "drill" | "tap" | "reamer" | "boring_bar" | "thread_mill"
    ) {
        form::set(draft, "/center_cutting", "false");
    }
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if !matches!(draft.selection, Selection::Tool(_)) {
        return true;
    }
    let kind = form::text(draft, "/kind").unwrap_or("");
    if path == "/point_angle_degrees" {
        return matches!(kind, "drill" | "chamfer_mill");
    }
    if path.starts_with("/native/corner/") {
        if !radius_capable(kind) {
            return false;
        }
        let shape = form::text(draft, "/native/corner/shape").unwrap_or("");
        return match path {
            "/native/corner/radius" => shape == "radius",
            "/native/corner/width" | "/native/corner/angle" => shape == "chamfer",
            _ => true,
        };
    }
    true
}

pub(super) fn apply(draft: &Draft, record: &mut Value, units: CamUnits) -> Result<(), String> {
    if draft.creation.is_some() {
        record["kind"] = json!(form::text(draft, "/kind")?);
        record["center_cutting"] = json!(form::text(draft, "/center_cutting")?
            .parse::<bool>()
            .map_err(|_| "Choose center cutting")?);
        for path in [
            "/point_angle_degrees",
            "/default_step_down",
            "/default_step_over",
        ] {
            record[path.trim_start_matches('/')] = if form::text(draft, path)?.is_empty() {
                Value::Null
            } else {
                json!(form::number(draft, path, units)?)
            };
        }
        record["cutting"]["coolant"] = json!(form::text(draft, "/cutting/coolant")?);
    }
    if draft.creation.is_some()
        || form::changed(draft, "/native/corner/")
        || form::changed(draft, "/kind")
    {
        let kind = record["kind"].as_str().unwrap_or("");
        let shape = if radius_capable(kind) {
            form::text(draft, "/native/corner/shape")?
        } else {
            "sharp"
        };
        record["corner_radius"] = if shape == "radius" {
            json!(form::number(draft, "/native/corner/radius", units)?)
        } else {
            Value::Null
        };
        record["corner_chamfer"] = if shape == "chamfer" {
            json!({"width":form::number(draft, "/native/corner/width", units)?, "angle_degrees":form::number(draft, "/native/corner/angle", units)?})
        } else {
            Value::Null
        };
    }
    Ok(())
}
