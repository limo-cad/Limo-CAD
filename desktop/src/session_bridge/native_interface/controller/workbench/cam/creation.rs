//! Creation forms over shared CAM DTOs, with explicit setup, tool and face choices.
use super::*;
use limo_cad_cam::{CamUnits, Point3Dto, StockBoxDto};
use limo_cad_sketch::SketchDto;
use limo_cad_solid::SolidSceneDto;
use std::sync::Arc;

mod operations;

pub(super) struct Context {
    bodies: Vec<(u64, String, StockBoxDto)>,
    model_valid: bool,
    scene: Arc<SolidSceneDto>,
    sketches: Arc<[SketchDto]>,
    operation: Option<operations::Context>,
}
impl Context {
    #[cfg(test)]
    pub(super) fn new(scene: &SolidSceneDto, cam: &CamDocumentDto) -> Result<Self, String> {
        Self::shared(&Arc::new(scene.clone()), cam)
    }
    pub(super) fn shared(
        scene: &Arc<SolidSceneDto>,
        _cam: &CamDocumentDto,
    ) -> Result<Self, String> {
        let bodies = scene
            .bodies
            .iter()
            .filter_map(|body| {
                let mut min = [f64::INFINITY; 3];
                let mut max = [f64::NEG_INFINITY; 3];
                for p in body.mesh.positions.as_chunks::<3>().0 {
                    for i in 0..3 {
                        min[i] = min[i].min(f64::from(p[i]));
                        max[i] = max[i].max(f64::from(p[i]));
                    }
                }
                min.into_iter().chain(max).all(f64::is_finite).then(|| {
                    (
                        body.id.0,
                        body.name.clone(),
                        StockBoxDto {
                            min: Point3Dto::new(min[0], min[1], min[2]),
                            max: Point3Dto::new(max[0], max[1], max[2]),
                        },
                    )
                })
            })
            .collect();
        Ok(Self {
            bodies,
            model_valid: scene.errors.is_empty(),
            scene: Arc::clone(scene),
            sketches: Arc::from([]),
            operation: None,
        })
    }
    pub(super) fn with_sketches(mut self, sketches: &[SketchDto]) -> Self {
        self.sketches = sketches.to_vec().into();
        self
    }
    pub(super) fn choices(&self, cam: &CamDocumentDto, path: &str) -> Option<Vec<ChoiceOption>> {
        match path {
            "/body_id" => Some(
                self.bodies
                    .iter()
                    .map(|(id, name, _)| ChoiceOption {
                        value: id.to_string(),
                        label: name.clone(),
                        disabled: false,
                    })
                    .collect(),
            ),
            "/setup_id" => Some(
                cam.setups
                    .iter()
                    .map(|setup| ChoiceOption {
                        value: setup.id.to_string(),
                        label: setup.name.clone(),
                        disabled: false,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
}
fn field(
    path: &'static str,
    label: &str,
    kind: InputKind,
    value: Option<f64>,
    units: CamUnits,
) -> DraftField {
    let text = value
        .map(|v| match kind {
            InputKind::Length | InputKind::Feed => units.from_mm(v).to_string(),
            _ => v.to_string(),
        })
        .unwrap_or_default();
    let label = match kind {
        InputKind::Length => format!("{label} ({})", units.length_label()),
        InputKind::Feed => format!("{label} ({})", units.feed_label()),
        _ => label.into(),
    };
    DraftField {
        path: path.into(),
        label,
        kind,
        original: text.clone(),
        text,
        options: None,
    }
}
pub(super) fn draft(tab: Tab, cam: &CamDocumentDto, context: Context) -> Draft {
    if tab == Tab::Toolpaths {
        return operations::draft(cam, context);
    }
    use InputKind::*;
    let mut fields = vec![field("/name", "Name", Name, None, cam.units)];
    let selection = match tab {
        Tab::Setups => {
            fields[0].text = format!("Setup {}", cam.setups.len() + 1);
            fields.push(field(
                "/body_id",
                "Model body · click to cycle",
                Integer,
                None,
                cam.units,
            ));
            fields.push(field(
                "/work_offset",
                "Work offset · click to cycle",
                Offset,
                None,
                cam.units,
            ));
            fields.last_mut().unwrap().text = "g54".into();
            for (path, label, value) in [
                ("/x_min", "Stock −X allowance", 2.),
                ("/x_max", "Stock +X allowance", 2.),
                ("/y_min", "Stock −Y allowance", 2.),
                ("/y_max", "Stock +Y allowance", 2.),
                ("/z_min", "Stock −Z allowance", 2.),
                ("/z_max", "Stock +Z allowance", 1.),
            ] {
                fields.push(field(path, label, Length, Some(value), cam.units));
            }
            Selection::Setup(0)
        }
        Tab::Tools => {
            fields.push(field(
                "/number",
                "Tool number (optional)",
                OptionalInteger,
                Some(f64::from(cam.tools.iter().filter_map(|t| t.number).max().unwrap_or(0)) + 1.),
                cam.units,
            ));
            for (path, label, kind, value) in [
                ("/diameter", "Diameter", Length, None),
                ("/flute_length", "Flute length", Length, None),
                ("/overall_length", "Overall length", Length, None),
                ("/flute_count", "Flutes", Integer, Some(4.)),
                ("/spindle_rpm", "Default spindle (rpm)", Integer, None),
                ("/feed_xy", "Default cutting feed", Feed, None),
                ("/feed_z", "Default plunge feed", Feed, None),
            ] {
                fields.push(field(path, label, kind, value, cam.units));
            }
            Selection::Tool(0)
        }
        Tab::Toolpaths => unreachable!("Toolpath creation uses the shared operation editor"),
    };
    let mut draft = Draft {
        creation: Some(context),
        copied_tool: false,
        setup: None,
        machine: None,
        presets: None,
        operation_edit: None,
        selection,
        record: Value::Null,
        fields,
        enabled: None,
        original_enabled: None,
    };
    if tab == Tab::Tools {
        tool::extend(&mut draft, cam, true).expect("New cutter form is valid");
        presets::extend_tool(&mut draft, cam.units).expect("New cutter profiles are valid");
    }
    draft
}
fn text<'a>(draft: &'a Draft, path: &str) -> Result<&'a str, String> {
    draft
        .fields
        .iter()
        .find(|f| f.path == path)
        .map(|f| f.text.trim())
        .ok_or_else(|| format!("Missing {path}"))
}
fn integer(draft: &Draft, path: &str) -> Result<u64, String> {
    text(draft, path)?.parse().map_err(|_| {
        format!(
            "Choose or enter {}",
            draft.fields.iter().find(|f| f.path == path).unwrap().label
        )
    })
}
fn number(draft: &Draft, path: &str, units: CamUnits) -> Result<f64, String> {
    let field = draft
        .fields
        .iter()
        .find(|f| f.path == path)
        .ok_or("Missing CAM field")?;
    let value = field
        .text
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("Enter {}", field.label))?;
    let value = if matches!(field.kind, InputKind::Length | InputKind::Feed) {
        units.to_mm(value)
    } else {
        value
    };
    if !value.is_finite() {
        return Err(format!("{} must be finite", field.label));
    }
    Ok(value)
}
pub(super) fn seed_choices(draft: &mut Draft, cam: &CamDocumentDto) -> Result<(), String> {
    if draft.selection == Selection::Operation(0) {
        operations::changed(draft, cam, "/native/create/setup_id")?;
    }
    Ok(())
}
pub(super) fn create(
    draft: &Draft,
    cam: &CamDocumentDto,
) -> Result<(CamDocumentDto, Selection), String> {
    if draft.selection == Selection::Operation(0) {
        return operations::create(draft, cam);
    }
    let context = draft.creation.as_ref().ok_or("Open a creation form")?;
    let name = text(draft, "/name")?;
    if name.is_empty() {
        return Err("Enter a name".into());
    }
    let mut next = cam.clone();
    let selection = match draft.selection {
        Selection::Setup(0) => {
            if !context.model_valid {
                return Err("Resolve model errors before creating a setup".into());
            }
            let id = integer(draft, "/body_id")?;
            let bounds = context
                .bodies
                .iter()
                .find(|(body, _, _)| *body == id)
                .map(|(_, _, b)| b)
                .ok_or("Choose a model body")?;
            let mut offsets = [0.; 6];
            for (i, path) in ["/x_min", "/x_max", "/y_min", "/y_max", "/z_min", "/z_max"]
                .into_iter()
                .enumerate()
            {
                offsets[i] = number(draft, path, cam.units)?;
                if offsets[i] < 0. {
                    return Err("Stock allowances must be nonnegative".into());
                }
            }
            let min = [
                bounds.min.x - offsets[0],
                bounds.min.y - offsets[2],
                bounds.min.z - offsets[4],
            ];
            let max = [
                bounds.max.x + offsets[1],
                bounds.max.y + offsets[3],
                bounds.max.z + offsets[5],
            ];
            let origin = [min[0], min[1], max[2]];
            let setup_id = next_id(cam.next_setup_id, cam.setups.iter().map(|s| s.id))?;
            let setup = serde_json::from_value(json!({"id":setup_id,"name":name,"body_ids":[id],
                "wcs":{"origin":{"x":origin[0],"y":origin[1],"z":origin[2]},"x_axis":[1.,0.,0.],"y_axis":[0.,1.,0.],"z_axis":[0.,0.,1.]},
                "wcs_origin":{"mode":"stock_box_point","x":"min","y":"min","z":"max"},
                "work_offset":text(draft,"/work_offset")?,"work_offset_count":1,
                "stock_spec":{"mode":"from_model","shape":"box","offsets":{"x_min":offsets[0],"x_max":offsets[1],"y_min":offsets[2],"y_max":offsets[3],"z_min":offsets[4],"z_max":offsets[5]}},
                "resolved_stock":{"shape":"box"},
                "stock_model_box":{"min":{"x":min[0],"y":min[1],"z":min[2]},"max":{"x":max[0],"y":max[1],"z":max[2]}},
                "stock":{"min":{"x":0.,"y":0.,"z":min[2]-origin[2]},"max":{"x":max[0]-origin[0],"y":max[1]-origin[1],"z":0.}},"operations":[]
            })).map_err(|e|format!("Invalid setup: {e}"))?;
            next.setups.push(setup);
            next.next_setup_id = setup_id + 1;
            next.active_setup_id = Some(setup_id);
            Selection::Setup(setup_id)
        }
        Selection::Tool(0) => {
            let id = next_id(cam.next_tool_id, cam.tools.iter().map(|t| t.id))?;
            let mut record = json!({"id":id,"name":name,"kind":"flat_end_mill",
                "number":if text(draft,"/number")?.is_empty(){None}else{Some(integer(draft,"/number")?)},
                "diameter":number(draft,"/diameter",cam.units)?,"flute_length":number(draft,"/flute_length",cam.units)?,
                "overall_length":number(draft,"/overall_length",cam.units)?,"flute_count":integer(draft,"/flute_count")?,
                "center_cutting":true,"cutting":{"spindle_rpm":integer(draft,"/spindle_rpm")?,
                    "feed_xy":number(draft,"/feed_xy",cam.units)?,"feed_z":number(draft,"/feed_z",cam.units)?,"coolant":"off"}
            });
            tool::apply(draft, &mut record, cam.units)?;
            presets::apply_tool(draft, &mut record, cam.units)?;
            let tool = serde_json::from_value(record).map_err(|e| format!("Invalid tool: {e}"))?;
            next.tools.push(tool);
            next.next_tool_id = id + 1;
            Selection::Tool(id)
        }
        Selection::Operation(0) => {
            unreachable!("Toolpath creation uses the shared operation editor")
        }
        _ => return Err("Open a new CAM item form".into()),
    };
    next.validate_for_editing()?;
    Ok((next, selection))
}
