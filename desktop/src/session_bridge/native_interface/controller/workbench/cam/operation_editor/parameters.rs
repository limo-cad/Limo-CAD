use super::*;

fn add(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    path: &str,
    label: &str,
    kind: InputKind,
    options: Option<Vec<ChoiceOption>>,
) {
    let Some(value) = draft.record.pointer(path).cloned() else {
        return;
    };
    if let Some(field) = draft.fields.iter_mut().find(|field| field.path == path) {
        field.kind = kind;
        field.label = match kind {
            InputKind::Length | InputKind::OptionalLength => {
                format!("{label} ({})", cam.units.length_label())
            }
            InputKind::Feed | InputKind::OptionalFeed => {
                format!("{label} ({})", cam.units.feed_label())
            }
            _ => label.into(),
        };
        return;
    }
    form::push(draft, path, label, kind, value, cam.units, options);
}
fn choice(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    path: &str,
    label: &str,
    options: &[(&str, &str)],
) {
    add(
        draft,
        cam,
        path,
        label,
        InputKind::Choice,
        Some(form::options(options)),
    );
}
fn boolean(draft: &mut Draft, cam: &CamDocumentDto, path: &str, label: &str) {
    add(
        draft,
        cam,
        path,
        label,
        InputKind::Boolean,
        Some(form::options(&[("true", "Yes"), ("false", "No")])),
    );
}

pub(super) fn extend(draft: &mut Draft, cam: &CamDocumentDto) -> Result<(), String> {
    use InputKind::*;
    let kind = draft.record["kind"].as_str().unwrap_or("").to_owned();
    choice(
        draft,
        cam,
        "/cutting/coolant",
        "Coolant",
        &[("off", "Off"), ("mist", "Mist"), ("flood", "Flood")],
    );
    match kind.as_str() {
        "face" => {
            for (path, label) in [
                ("/bounds/min/x", "Facing min X"),
                ("/bounds/min/y", "Facing min Y"),
                ("/bounds/max/x", "Facing max X"),
                ("/bounds/max/y", "Facing max Y"),
                ("/safe_distance", "Facing entry clearance"),
            ] {
                add(draft, cam, path, label, Length, None);
            }
            choice(
                draft,
                cam,
                "/direction",
                "Facing direction",
                &[
                    ("both_ways", "Both ways"),
                    ("climb", "Climb"),
                    ("conventional", "Conventional"),
                ],
            );
        }
        "contour2d" => {
            choice(
                draft,
                cam,
                "/compensation",
                "Cutter side",
                &[
                    ("on", "On path"),
                    ("inside", "Inside"),
                    ("outside", "Outside"),
                    ("left", "Left of travel"),
                    ("right", "Right of travel"),
                ],
            );
            choice(
                draft,
                cam,
                "/compensation_mode",
                "Radius compensation",
                &[
                    ("in_control", "In the controller"),
                    ("in_software", "In the planner"),
                ],
            );
            for (path, label, kind) in [
                ("/lead_in", "Entry lead", Length),
                ("/lead_out", "Exit lead", Length),
                (
                    "/lead_arc_radius",
                    "Lead arc radius (optional)",
                    OptionalLength,
                ),
                ("/roughing_passes", "Radial passes", Integer),
                (
                    "/roughing_step_over",
                    "Radial step over (optional)",
                    OptionalLength,
                ),
                ("/finish_allowance", "Finish allowance", Length),
                ("/finish_feed", "Finish feed (optional)", OptionalFeed),
            ] {
                add(draft, cam, path, label, kind, None);
            }
            boolean(draft, cam, "/finishing_pass", "Separate finishing pass");
            boolean(draft, cam, "/spring_pass", "Repeat last lap");
        }
        "drill" => {
            choice(
                draft,
                cam,
                "/cycle",
                "Holemaking cycle",
                &[
                    ("drill", "Drill"),
                    ("chip_breaking", "Chip breaking"),
                    ("deep_hole", "Deep-hole peck"),
                    ("tapping_right", "Right-hand tapping"),
                    ("tapping_left", "Left-hand tapping"),
                    ("reaming", "Reaming"),
                    ("boring", "Boring"),
                ],
            );
            boolean(draft, cam, "/drill_tip_through", "Drill tip through");
            boolean(
                draft,
                cam,
                "/floating_tap_holder",
                "Suitable floating tap holder",
            );
            for (path, label, kind) in [
                ("/breakthrough_depth", "Extra breakthrough", Length),
                ("/peck_depth", "Peck depth (optional)", OptionalLength),
                (
                    "/peck_retract",
                    "Chip-break retract (optional)",
                    OptionalLength,
                ),
                ("/thread_pitch", "Tap pitch (optional)", OptionalLength),
                ("/feed_out", "Feed out (optional)", OptionalFeed),
                ("/dwell_seconds", "Dwell (seconds)", Number),
            ] {
                add(draft, cam, path, label, kind, None);
            }
        }
        "chamfer2d" => {
            if draft.record["modeled_chamfer"].is_null() {
                add(
                    draft,
                    cam,
                    "/chamfer_width",
                    "First chain chamfer width",
                    Length,
                    None,
                );
            } else {
                add(
                    draft,
                    cam,
                    "/modeled_chamfer/additional_width",
                    "First chain additional chamfer width",
                    Length,
                    None,
                );
            }
            add(draft, cam, "/tip_offset", "Tip offset", Length, None);
            choice(
                draft,
                cam,
                "/wall_side",
                "Material wall side",
                &[
                    ("inside", "Inside"),
                    ("outside", "Outside"),
                    ("left", "Left of travel"),
                    ("right", "Right of travel"),
                ],
            );
        }
        "thread" => {
            for (path, label, kind) in [
                ("/pitch", "Thread pitch", Length),
                ("/major_diameter", "Major diameter", Length),
                ("/minor_diameter", "Minor diameter", Length),
                ("/radial_passes", "Radial passes", Integer),
                ("/step_over", "Radial step over (optional)", OptionalLength),
            ] {
                add(draft, cam, path, label, kind, None);
            }
            choice(
                draft,
                cam,
                "/hand",
                "Thread hand",
                &[("right", "Right"), ("left", "Left")],
            );
        }
        "flat3d" => {
            for (path, label) in [
                ("step_over", "Step over"),
                ("radial_stock_to_leave", "Radial stock to leave"),
                ("axial_stock_to_leave", "Axial stock to leave"),
                ("tolerance", "Flat-detection tolerance"),
                ("stay_down_distance", "Stay-down distance"),
            ] {
                add(
                    draft,
                    cam,
                    &format!("/parameters/{path}"),
                    label,
                    Length,
                    None,
                );
            }
            choice(
                draft,
                cam,
                "/parameters/direction",
                "Milling direction",
                &[("climb", "Climb"), ("conventional", "Conventional")],
            );
        }
        "adaptive3d" => {
            for (path, label, kind) in [
                ("optimal_load", "Optimal load", Length),
                ("maximum_stepdown", "Maximum step down", Length),
                ("minimum_cutting_radius", "Minimum cutting radius", Length),
                ("radial_stock_to_leave", "Radial stock to leave", Length),
                ("axial_stock_to_leave", "Axial stock to leave", Length),
                ("tolerance", "Stock-envelope tolerance", Length),
                ("ramp_angle_degrees", "Ramp angle (degrees)", Number),
                ("maximum_ramp_stepdown", "Maximum ramp step down", Length),
                ("ramp_feed", "Ramp feed", Feed),
                ("linking_feed", "Linking feed", Feed),
                ("stay_down_distance", "Stay-down distance", Length),
            ] {
                add(
                    draft,
                    cam,
                    &format!("/parameters/{path}"),
                    label,
                    kind,
                    None,
                );
            }
            boolean(
                draft,
                cam,
                "/parameters/machine_cavities",
                "Machine cavities",
            );
        }
        _ => {}
    }
    if matches!(
        kind.as_str(),
        "contour2d" | "pocket2d" | "chamfer2d" | "thread"
    ) {
        choice(
            draft,
            cam,
            "/direction",
            "Milling direction",
            &[("climb", "Climb"), ("conventional", "Conventional")],
        );
    }
    Ok(())
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if path == "/wall_side" && !draft.record["modeled_chamfer"].is_null() {
        return false;
    }
    let value = |field: &str| {
        draft
            .fields
            .iter()
            .find(|f| f.path == field)
            .map(|f| f.text.as_str())
            .unwrap_or("")
    };
    if draft.record["kind"] == "drill" {
        let cycle = value("/cycle");
        if matches!(path, "/drill_tip_through" | "/breakthrough_depth") {
            return matches!(cycle, "drill" | "chip_breaking" | "deep_hole");
        }
        if path == "/peck_depth" {
            return matches!(cycle, "chip_breaking" | "deep_hole");
        }
        if path == "/peck_retract" {
            return cycle == "chip_breaking";
        }
        if matches!(path, "/thread_pitch" | "/floating_tap_holder") {
            return matches!(cycle, "tapping_right" | "tapping_left");
        }
        if path == "/feed_out" {
            return matches!(cycle, "reaming" | "boring");
        }
    }
    true
}

pub(super) fn apply(draft: &Draft, record: &mut Value) -> Result<(), String> {
    if draft.record["kind"] == "drill" && form::changed(draft, "/cycle") {
        let cycle = record["cycle"]
            .as_str()
            .ok_or("Choose a holemaking cycle")?;
        let pecking = matches!(cycle, "chip_breaking" | "deep_hole");
        let chip_breaking = cycle == "chip_breaking";
        let tapping = matches!(cycle, "tapping_right" | "tapping_left");
        let feed_out = matches!(cycle, "reaming" | "boring");
        let drilling = matches!(cycle, "drill" | "chip_breaking" | "deep_hole");
        if !pecking {
            record["peck_depth"] = Value::Null;
        }
        if !chip_breaking {
            record["peck_retract"] = Value::Null;
        }
        if !tapping {
            record["thread_pitch"] = Value::Null;
            record["floating_tap_holder"] = json!(false);
        } else {
            record["dwell_seconds"] = json!(0.);
        }
        if !feed_out {
            record["feed_out"] = Value::Null;
        }
        if !drilling {
            record["drill_tip_through"] = json!(false);
            record["breakthrough_depth"] = json!(0.);
        }
    }
    if form::changed(draft, "/modeled_chamfer/additional_width") {
        let old = draft.record["modeled_chamfer"]["additional_width"]
            .as_f64()
            .ok_or("Missing stored chamfer allowance")?;
        let next = record["modeled_chamfer"]["additional_width"]
            .as_f64()
            .ok_or("Enter the added chamfer width")?;
        let width = draft.record["chamfer_width"]
            .as_f64()
            .ok_or("Missing measured chamfer width")?
            + next
            - old;
        if !width.is_finite() {
            return Err("Chamfer width must be finite".into());
        }
        record["chamfer_width"] = json!(width);
    }
    Ok(())
}
