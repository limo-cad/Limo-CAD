use super::*;
use limo_cad_cam::{CamLeadDto, CamLinkingDto};
const PREFIX: &str = "/native/linking/";

pub(super) fn initial(cam: &CamDocumentDto, operation: &CamOperationDto) -> Result<Value, String> {
    if let Some(linking) = cam
        .linking
        .iter()
        .find(|linking| linking.operation_id == operation.id())
    {
        return serde_json::to_value(linking).map_err(|e| e.to_string());
    }
    let tool = cam.tool(operation.tool_id());
    let diameter = tool.map_or(6., |tool| tool.diameter);
    let record = serde_json::to_value(operation).map_err(|e| e.to_string())?;
    let number = |path: &str, default: f64| {
        record
            .pointer(path)
            .and_then(Value::as_f64)
            .unwrap_or(default)
    };
    let kind = record["kind"].as_str().unwrap_or("");
    let cutting = operation.cutting();
    let lead = CamLeadDto {
        enabled: true,
        horizontal_radius: if kind == "face" {
            0.
        } else if kind == "contour2d" {
            number("/lead_arc_radius", 0.)
        } else {
            diameter * 0.1
        },
        sweep_degrees: 90.,
        linear_distance: if kind == "chamfer2d" {
            diameter * 0.1
        } else if kind == "contour2d" {
            number("/lead_in", diameter * 0.1)
        } else {
            0.
        },
        perpendicular: false,
        vertical_radius: 0.,
    };
    let linking = CamLinkingDto {
        operation_id: operation.id(),
        keep_tool_down: kind == "face",
        maximum_stay_down: number("/parameters/stay_down_distance", diameter * 5.),
        safe_distance: if kind == "face" {
            number("/safe_distance", 5.)
        } else {
            1_f64.min(diameter * 0.1)
        },
        lead_in: lead.clone(),
        lead_out: CamLeadDto {
            linear_distance: number("/lead_out", lead.linear_distance),
            ..lead
        },
        same_as_lead_in: kind != "contour2d" || number("/lead_in", 0.) == number("/lead_out", 0.),
        lead_in_feed: cutting.feed_xy,
        lead_out_feed: cutting.feed_xy,
        no_engagement_feed: number("/parameters/linking_feed", cutting.feed_xy),
        ramp_enabled: kind == "adaptive3d",
        ramp_angle: number("/parameters/ramp_angle_degrees", 3.),
        ramp_stepdown: number(
            "/parameters/maximum_ramp_stepdown",
            1_f64.min(diameter * 0.25),
        ),
        helix_diameter: diameter * 0.95,
        minimum_helix_diameter: diameter * 0.5,
        ramp_feed: number("/parameters/ramp_feed", cutting.feed_z),
        ..Default::default()
    };
    serde_json::to_value(linking).map_err(|e| e.to_string())
}
fn push(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    record: &Value,
    path: &str,
    label: &str,
    kind: InputKind,
    options: Option<Vec<ChoiceOption>>,
) {
    let Some(value) = record.pointer(&format!("/{path}")).cloned() else {
        return;
    };
    form::push(
        draft,
        &format!("{PREFIX}{path}"),
        label,
        kind,
        value,
        cam.units,
        options,
    );
}
pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    record: &Value,
) -> Result<(), String> {
    use InputKind::*;
    let Selection::Operation(id) = draft.selection else {
        return Ok(());
    };
    form::push(
        draft,
        &format!("{PREFIX}mode"),
        "Link programming",
        Choice,
        json!(if cam.linking.iter().any(|link| link.operation_id == id) {
            "custom"
        } else {
            "legacy"
        }),
        cam.units,
        Some(form::options(&[
            ("legacy", "Existing legacy linking"),
            ("custom", "Explicit linking controls"),
        ])),
    );
    for (path, label, options) in [
        (
            "high_feed_mode",
            "Rapid replacement",
            vec![
                ("preserve", "Preserve rapids"),
                ("axial_radial", "Axial and radial"),
                ("axial", "Axial"),
                ("radial", "Radial"),
                ("single_axis", "Single-axis"),
                ("always", "Always"),
            ],
        ),
        (
            "retraction_policy",
            "Retraction policy",
            vec![
                ("full", "Full clearance"),
                ("minimum", "Minimum clearance"),
                ("shortest", "Shortest safe"),
            ],
        ),
        (
            "transition",
            "Facing transition",
            vec![
                ("no_contact", "No contact"),
                ("straight", "Straight"),
                ("shortest", "Shortest"),
                ("smooth", "Smooth"),
            ],
        ),
        (
            "ramp_type",
            "Entry type",
            vec![
                ("predrill", "Predrilled entry"),
                ("plunge", "Plunge"),
                ("helix", "Helix"),
            ],
        ),
    ] {
        push(
            draft,
            cam,
            record,
            path,
            label,
            Choice,
            Some(form::options(&options)),
        );
    }
    for (path, label) in [
        ("allow_rapid_retract", "Allow rapid retract"),
        ("keep_tool_down", "Keep tool down"),
        ("extend_before_retract", "Extend before retract"),
        ("same_as_lead_in", "Match exit lead to entry"),
        ("ramp_enabled", "Use ramp entry"),
    ] {
        push(
            draft,
            cam,
            record,
            path,
            label,
            Boolean,
            Some(form::options(&[("true", "Yes"), ("false", "No")])),
        );
    }
    for (path, label, kind) in [
        ("high_feed", "High feed", Feed),
        ("maximum_stay_down", "Maximum stay-down distance", Length),
        ("minimum_clearance", "Minimum link clearance", Length),
        (
            "stay_down_level",
            "Stay-down search effort (0–100)",
            Integer,
        ),
        ("lift_height", "Link lift", Length),
        ("safe_distance", "Link safe distance", Length),
        ("lead_in_feed", "Entry lead feed", Feed),
        ("lead_out_feed", "Exit lead feed", Feed),
        ("no_engagement_feed", "No-engagement feed", Feed),
        ("ramp_angle", "Ramp angle (degrees)", Number),
        ("ramp_stepdown", "Ramp step down", Length),
        ("ramp_clearance", "Ramp clearance", Length),
        ("ramp_taper_angle", "Ramp taper (degrees)", Number),
        ("helix_diameter", "Tool-center helix diameter", Length),
        ("minimum_helix_diameter", "Minimum helix diameter", Length),
        ("ramp_feed", "Ramp feed", Feed),
    ] {
        push(draft, cam, record, path, label, kind, None);
    }
    for (prefix, label) in [("lead_in", "Entry"), ("lead_out", "Exit")] {
        for (field, suffix) in [
            ("enabled", "lead enabled"),
            ("perpendicular", "lead perpendicular"),
        ] {
            push(
                draft,
                cam,
                record,
                &format!("{prefix}/{field}"),
                &format!("{label} {suffix}"),
                Boolean,
                Some(form::options(&[("true", "Yes"), ("false", "No")])),
            );
        }
        for (field, suffix, kind) in [
            ("horizontal_radius", "horizontal lead radius", Length),
            ("sweep_degrees", "lead sweep (degrees)", Number),
            ("linear_distance", "straight lead", Length),
            ("vertical_radius", "vertical lead radius", Length),
        ] {
            push(
                draft,
                cam,
                record,
                &format!("{prefix}/{field}"),
                &format!("{label} {suffix}"),
                kind,
                None,
            );
        }
    }
    Ok(())
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if linking_points::handles(path) {
        return linking_points::visible(draft, path);
    }
    let Some(path) = path.strip_prefix(PREFIX) else {
        return true;
    };
    if path == "mode" {
        return true;
    }
    if form::text(draft, &format!("{PREFIX}mode")).unwrap_or("") != "custom" {
        return false;
    }
    let enabled =
        |name: &str| form::text(draft, &format!("{PREFIX}{name}")).unwrap_or("") == "true";
    if path == "transition" {
        return draft.record["kind"] == "face";
    }
    if path.starts_with("lead_out/") && path != "lead_out/enabled" && enabled("same_as_lead_in") {
        return false;
    }
    if matches!(
        path,
        "ramp_type"
            | "ramp_angle"
            | "ramp_stepdown"
            | "ramp_clearance"
            | "ramp_taper_angle"
            | "helix_diameter"
            | "minimum_helix_diameter"
            | "ramp_feed"
    ) {
        return enabled("ramp_enabled");
    }
    true
}

pub(super) fn apply(
    draft: &Draft,
    record: &Value,
    cam: &mut CamDocumentDto,
    original: &Value,
    points: &linking_points::Context,
) -> Result<(), String> {
    if !form::changed(draft, PREFIX) {
        return Ok(());
    }
    let Selection::Operation(id) = draft.selection else {
        return Err("Select a toolpath".into());
    };
    if form::text(draft, &format!("{PREFIX}mode"))? == "legacy" {
        cam.linking.retain(|link| link.operation_id != id);
        return Ok(());
    }
    let mut next = original.clone();
    for field in draft.fields.iter().filter(|field| {
        field.path.starts_with(PREFIX)
            && field.path != format!("{PREFIX}mode")
            && !linking_points::handles(&field.path)
            && field.text != field.original
    }) {
        let pointer = format!("/{}", field.path.strip_prefix(PREFIX).unwrap());
        let value = match field.kind {
            InputKind::Choice => json!(field.text),
            InputKind::Boolean => json!(field
                .text
                .parse::<bool>()
                .map_err(|_| format!("Choose {}", field.label))?),
            InputKind::Integer => json!(field
                .text
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("Enter a whole number for {}", field.label))?),
            _ => json!(form::number(draft, &field.path, cam.units)?),
        };
        *next
            .pointer_mut(&pointer)
            .ok_or("Linking field is unavailable")? = value;
    }
    linking_points::apply(draft, &mut next, cam.units, points)?;
    let next: CamLinkingDto = serde_json::from_value(next).map_err(|e| e.to_string())?;
    let operation: CamOperationDto =
        serde_json::from_value(record.clone()).map_err(|e| e.to_string())?;
    next.validate(&operation)?;
    if let Some(index) = cam.linking.iter().position(|link| link.operation_id == id) {
        cam.linking[index] = next;
    } else {
        cam.linking.push(next);
    }
    Ok(())
}
