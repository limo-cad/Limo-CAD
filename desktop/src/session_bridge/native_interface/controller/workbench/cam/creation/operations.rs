//! New toolpaths use the same typed operation editor as existing toolpaths.
//! Only tool/setup-derived defaults are seeded. Paths and hole centers require
//! an explicit geometry choice; thread dimensions require explicit entry.
use super::*;
use limo_cad_cam::{CamOperationHeightExpressionsDto, CamSetupDto, CamToolDto};

const KIND: &str = "/native/create/kind";
const SETUP: &str = "/native/create/setup_id";
pub(super) struct Context {
    setup_id: u64,
    kind: String,
    heights: CamOperationHeightExpressionsDto,
}

fn labels() -> Vec<ChoiceOption> {
    form::options(&[
        ("face", "Face"),
        ("contour2d", "2D contour"),
        ("pocket2d", "2D pocket"),
        ("chamfer2d", "Chamfer"),
        ("drill", "Holemaking"),
        ("thread", "Thread milling"),
        ("adaptive3d", "High-speed roughing"),
        ("flat3d", "Flat finishing"),
    ])
}
fn header(draft: &mut Draft, cam: &CamDocumentDto, kind: &str, setup: &str) {
    form::push(
        draft,
        KIND,
        "Toolpath type",
        InputKind::Choice,
        json!(kind),
        cam.units,
        Some(labels()),
    );
    form::push(
        draft,
        SETUP,
        "Setup · click to cycle",
        InputKind::Choice,
        json!(setup),
        cam.units,
        Some(
            cam.setups
                .iter()
                .map(|setup| ChoiceOption {
                    value: setup.id.to_string(),
                    label: setup.name.clone(),
                    disabled: false,
                })
                .collect(),
        ),
    );
}
pub(super) fn draft(cam: &CamDocumentDto, context: super::Context) -> Draft {
    let mut draft = Draft {
        copied_tool: false,
        creation: Some(context),
        setup: None,
        machine: None,
        presets: None,
        operation_edit: None,
        selection: Selection::Operation(0),
        record: Value::Null,
        fields: Vec::new(),
        enabled: None,
        original_enabled: None,
    };
    header(&mut draft, cam, "face", "");
    form::push(
        &mut draft,
        "/name",
        "Name",
        InputKind::Name,
        json!("Face 1"),
        cam.units,
        None,
    );
    form::push(
        &mut draft,
        "/tool_id",
        "Tool · click to cycle",
        InputKind::Integer,
        Value::Null,
        cam.units,
        choices(cam, "/tool_id"),
    );
    draft
}
pub(super) fn changed(draft: &mut Draft, cam: &CamDocumentDto, path: &str) -> Result<(), String> {
    if !matches!(path, KIND | SETUP | "/tool_id") {
        return Ok(());
    }
    let kind = form::text(draft, KIND)?.to_owned();
    let setup_id = form::text(draft, SETUP)?.parse::<u64>().ok();
    let tool_id = form::text(draft, "/tool_id")?.parse::<u64>().ok();
    let (Some(setup), Some(tool)) = (
        setup_id.and_then(|id| cam.setup(id)),
        tool_id.and_then(|id| cam.tool(id)),
    ) else {
        return Ok(());
    };
    let context = draft
        .creation
        .as_ref()
        .ok_or("Open a toolpath creation form")?;
    if context
        .operation
        .as_ref()
        .is_some_and(|seed| seed.setup_id == setup.id && seed.kind == kind)
    {
        return Ok(());
    }
    if !context.model_valid {
        return Err("Resolve model errors before creating a toolpath".into());
    }
    let geometry = operation_geometry::Context::new(setup, &context.scene, &context.sketches);
    let (record, heights) = blueprint(&kind, setup, tool, &geometry)?;
    let mut temporary = cam.clone();
    temporary
        .setups
        .iter_mut()
        .find(|item| item.id == setup.id)
        .unwrap()
        .operations
        .push(record);
    temporary.height_expressions.push(heights.clone());
    let mut next = Draft::new(&temporary, Selection::Operation(0))?;
    operation_editor::extend_shared(&mut next, &temporary, &context.scene, &context.sketches)?;
    let name = draft.fields.iter().find(|field| field.path == "/name");
    let keep_name = name
        .filter(|field| field.text != field.original)
        .map(|field| field.text.clone());
    if let Some(name) = keep_name {
        form::set(&mut next, "/name", &name);
    }
    let index = next.fields.len();
    header(&mut next, cam, &kind, &setup.id.to_string());
    let header = next.fields.drain(index..).collect::<Vec<_>>();
    next.fields.splice(1..1, header);
    if kind == "thread" {
        for path in ["/pitch", "/major_diameter", "/minor_diameter"] {
            if let Some(field) = next.fields.iter_mut().find(|field| field.path == path) {
                field.original.clear();
                field.text.clear();
            }
        }
    }
    next.creation = draft.creation.take();
    next.creation.as_mut().unwrap().operation = Some(Context {
        setup_id: setup.id,
        kind,
        heights,
    });
    *draft = next;
    Ok(())
}

pub(super) fn create(
    draft: &Draft,
    cam: &CamDocumentDto,
) -> Result<(CamDocumentDto, Selection), String> {
    let seed = draft
        .creation
        .as_ref()
        .and_then(|context| context.operation.as_ref())
        .ok_or("Choose a setup and project tool")?;
    if form::text(draft, SETUP)?.parse::<u64>().ok() != Some(seed.setup_id)
        || form::text(draft, KIND)? != seed.kind
    {
        return Err(
            "Resolve the selected setup and toolpath type before creating the toolpath".into(),
        );
    }
    let mut next = cam.clone();
    let id = next_id(
        cam.next_operation_id,
        cam.setups
            .iter()
            .flat_map(|setup| &setup.operations)
            .map(CamOperationDto::id),
    )?;
    next.setups
        .iter_mut()
        .find(|setup| setup.id == seed.setup_id)
        .ok_or("The setup was removed")?
        .operations
        .push(serde_json::from_value(draft.record.clone()).map_err(|e| e.to_string())?);
    next.height_expressions.push(seed.heights.clone());
    next = draft.edited_without_validation(&next)?;
    let setup = next
        .setups
        .iter_mut()
        .find(|setup| setup.id == seed.setup_id)
        .unwrap();
    let index = setup
        .operations
        .iter()
        .position(|operation| operation.id() == 0)
        .ok_or("New toolpath draft missing")?;
    let mut record = serde_json::to_value(&setup.operations[index]).map_err(|e| e.to_string())?;
    record["id"] = json!(id);
    setup.operations[index] = serde_json::from_value(record).map_err(|e| e.to_string())?;
    setup.operations[index].validate(setup, &next.tools)?;
    for height in next
        .height_expressions
        .iter_mut()
        .filter(|height| height.operation_id == 0)
    {
        height.operation_id = id;
    }
    for links in next
        .linking
        .iter_mut()
        .filter(|links| links.operation_id == 0)
    {
        links.operation_id = id;
    }
    next.next_operation_id = id + 1;
    next.active_setup_id = Some(seed.setup_id);
    next.validate_for_editing()?;
    Ok((next, Selection::Operation(id)))
}

fn blueprint(
    kind: &str,
    setup: &CamSetupDto,
    tool: &CamToolDto,
    geometry: &operation_geometry::Context,
) -> Result<(CamOperationDto, CamOperationHeightExpressionsDto), String> {
    let mut top = f64::NEG_INFINITY;
    let mut bottom = f64::INFINITY;
    for body in geometry
        .scene
        .bodies
        .iter()
        .filter(|body| setup.body_ids.contains(&body.id))
    {
        for point in body.mesh.positions.as_chunks::<3>().0 {
            let relative = [
                f64::from(point[0]) - setup.wcs.origin.x,
                f64::from(point[1]) - setup.wcs.origin.y,
                f64::from(point[2]) - setup.wcs.origin.z,
            ];
            let z = relative
                .into_iter()
                .zip(setup.wcs.z_axis)
                .map(|(a, b)| a * b)
                .sum::<f64>();
            top = top.max(z);
            bottom = bottom.min(z);
        }
    }
    if !top.is_finite() || !bottom.is_finite() {
        return Err("The setup needs a current model body".into());
    }
    let safe = top.max(setup.stock.max.z);
    let (top_value, bottom_value, top_reference, bottom_reference) = match kind {
        "face" => (setup.stock.max.z, top, "stock_top", "model_top"),
        "contour2d" | "adaptive3d" => (
            setup.stock.max.z,
            setup.stock.min.z,
            "stock_top",
            "stock_bottom",
        ),
        "pocket2d" | "flat3d" => (top, bottom, "model_top", "model_bottom"),
        "drill" | "thread" => (top, setup.stock.min.z, "model_top", "stock_bottom"),
        "chamfer2d" => (top, bottom, "model_top", "model_bottom"),
        _ => return Err("Choose a toolpath type".into()),
    };
    let title = labels()
        .into_iter()
        .find(|option| option.value == kind)
        .unwrap()
        .label;
    let count = setup
        .operations
        .iter()
        .filter(|operation| {
            serde_json::to_value(operation).is_ok_and(|value| value["kind"] == kind)
        })
        .count()
        + 1;
    let step = tool.default_step_down.unwrap_or_else(|| {
        if kind == "face" {
            (top_value - bottom_value).abs().max(0.001)
        } else {
            (tool.diameter * 0.5).min(tool.flute_length * 0.5)
        }
    });
    let mut record = json!({"id":0,"name":format!("{title} {count}"),"kind":kind,"tool_id":tool.id,"enabled":true,
        "top_z":top_value,"bottom_z":bottom_value,"clearance_z":safe+10.,"retract_z":safe+5.,"feed_height_z":safe+5.,"cutting":tool.cutting});
    let extra = match kind {
        "face" => {
            json!({"bounds":setup.stock.xy_bounds(),"target_z":bottom_value,"step_down":step,"step_over":tool.default_step_over.unwrap_or(tool.diameter*0.5),"safe_distance":5.,"direction":"both_ways"})
        }
        "contour2d" => {
            json!({"path":[],"closed":true,"chain_ref":null,"step_down":step,"compensation":"outside","compensation_mode":"in_software","direction":"climb","lead_in":tool.diameter*0.75,"lead_out":tool.diameter*0.75})
        }
        "pocket2d" => {
            json!({"outline":[],"chain_ref":null,"step_down":step,"step_over":tool.default_step_over.unwrap_or(tool.diameter*0.5),"direction":"climb"})
        }
        "chamfer2d" => {
            json!({"path":[],"closed":true,"chain_ref":null,"modeled_chamfer":null,"additional_chains":[],"chamfer_width":0.5,"tip_offset":0.5,"wall_side":"inside","direction":"climb"})
        }
        "drill" => {
            json!({"points":[],"holes":[],"cycle":"drill","drill_tip_through":true,"breakthrough_depth":1.})
        }
        "thread" => {
            json!({"points":[],"holes":[],"pitch":0.,"major_diameter":0.,"minor_diameter":0.,"hand":"right","direction":"climb","radial_passes":1})
        }
        "adaptive3d" => {
            json!({"geometry":null,"parameters":{"optimal_load":tool.diameter*0.2,"maximum_stepdown":tool.diameter.min(tool.flute_length*0.5),"minimum_cutting_radius":tool.diameter*0.2,
            "radial_stock_to_leave":0.2,"axial_stock_to_leave":0.2,"tolerance":0.2,"ramp_angle_degrees":3.,"maximum_ramp_stepdown":1_f64.min(tool.diameter*0.25),"ramp_feed":tool.cutting.feed_z,"linking_feed":tool.cutting.feed_xy,"stay_down_distance":tool.diameter*5.,"machine_cavities":true}})
        }
        "flat3d" => {
            let flat_width = tool.diameter - 2.0 * tool.corner_radius.unwrap_or(0.0);
            json!({"geometry":null,"parameters":{"step_over":tool.default_step_over.unwrap_or(tool.diameter*0.5).min(flat_width),
                "radial_stock_to_leave":0.,"axial_stock_to_leave":0.,"tolerance":0.05,"direction":"climb","stay_down_distance":tool.diameter*5.}})
        }
        _ => unreachable!(),
    };
    record
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let expression = |reference: &str, offset: f64| json!({"reference":reference,"offset":offset});
    let mut heights = json!({"operation_id":0,"top":expression(top_reference,0.),"bottom":expression(bottom_reference,0.),
        "feed":expression("model_top",safe+5.-top),"retract":expression("model_top",safe+5.-top),"clearance":expression("retract",5.)});
    if kind == "face" {
        heights["clearance"] = expression("model_top", safe + 10. - top);
    }
    if kind == "chamfer2d" {
        heights["bottom"] = Value::Null;
    }
    Ok((
        serde_json::from_value(record).map_err(|e| e.to_string())?,
        serde_json::from_value(heights).map_err(|e| e.to_string())?,
    ))
}
