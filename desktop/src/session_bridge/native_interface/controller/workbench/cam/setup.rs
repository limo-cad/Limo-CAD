//! Stock/WCS form over CamSetupDto. The engine validates the resolved shared DTO.
use super::*;
use limo_cad_cam::{CamSetupDto, CamStockSpecDto, Point3Dto, StockBoxDto, WorkCoordinateSystemDto};
use limo_cad_sketch::{EntityDto, SketchDto};
use limo_cad_solid::SolidSceneDto;

pub(super) mod picking;
mod resolve;
const PREFIX: &str = "/native/setup/";
pub(super) struct Context {
    bodies: Vec<(u64, String, StockBoxDto)>,
    points: Vec<(String, String, u64, Point3Dto)>,
    model_valid: bool,
    source: Source,
    stamp: std::sync::Arc<()>,
}
struct Source {
    id: u64,
    body_ids: Vec<u64>,
    stock_spec: CamStockSpecDto,
    stock: StockBoxDto,
    model_box: Option<StockBoxDto>,
    wcs: WorkCoordinateSystemDto,
}
impl Context {
    fn new(scene: &SolidSceneDto, sketches: &[SketchDto], setup: &CamSetupDto) -> Self {
        let bodies =
            scene
                .bodies
                .iter()
                .filter_map(|body| {
                    let points =
                        body.mesh.positions.as_chunks::<3>().0.iter().map(|p| {
                            Point3Dto::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
                        });
                    resolve::bounds(points).map(|bounds| (body.id.0, body.name.clone(), bounds))
                })
                .collect();
        let points = sketches
            .iter()
            .flat_map(|sketch| {
                sketch.entities.iter().filter_map(|entity| {
                    let EntityDto::Point { id, position, .. } = entity else {
                        return None;
                    };
                    let p = sketch.basis.to_3d([position.x, position.y]);
                    Some((
                        format!("{}:{}", sketch.name, id.0),
                        sketch.name.clone(),
                        id.0,
                        Point3Dto::new(p[0], p[1], p[2]),
                    ))
                })
            })
            .collect();
        Self {
            bodies,
            points,
            model_valid: scene.errors.is_empty(),
            source: Source {
                id: setup.id,
                body_ids: setup.body_ids.iter().map(|id| id.0).collect(),
                stock_spec: setup.stock_spec.clone(),
                stock: setup.stock,
                model_box: setup
                    .stock_model_box
                    .or_else(|| resolve::box_to_model(setup.stock, setup.wcs).ok()),
                wcs: setup.wcs,
            },
            stamp: std::sync::Arc::new(()),
        }
    }
    fn model_bounds(&self, ids: &[u64]) -> Result<StockBoxDto, String> {
        if ids.is_empty() {
            return Err("Include at least one model body in the setup".into());
        }
        if ids
            .iter()
            .any(|id| !self.bodies.iter().any(|(body, _, _)| body == id))
        {
            return Err("A selected model body is no longer available".into());
        }
        resolve::bounds(
            self.bodies
                .iter()
                .filter(|(id, _, _)| ids.contains(id))
                .flat_map(|(_, _, b)| [b.min, b.max]),
        )
        .ok_or("Select a model body with current geometry".into())
    }
}

pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
) -> Result<(), String> {
    let Selection::Setup(id) = draft.selection else {
        return Ok(());
    };
    if id == 0 {
        return Ok(());
    }
    use InputKind::*;
    let setup = cam.setup(id).ok_or("Setup was removed")?;
    let context = Context::new(scene, sketches, setup);
    let record = serde_json::to_value(setup).map_err(|e| e.to_string())?;
    let value = |path: &str, fallback: Value| record.pointer(path).cloned().unwrap_or(fallback);
    for (body, name, _) in &context.bodies {
        form::push(
            draft,
            &format!("{PREFIX}body/{body}"),
            &format!("Model · {name}"),
            Boolean,
            json!(setup.body_ids.iter().any(|id| id.0 == *body)),
            cam.units,
            Some(form::options(&[
                ("true", "Included"),
                ("false", "Excluded"),
            ])),
        );
    }
    for body in setup
        .body_ids
        .iter()
        .filter(|id| !context.bodies.iter().any(|(body, _, _)| *body == id.0))
    {
        form::push(
            draft,
            &format!("{PREFIX}body/{}", body.0),
            "Missing model body",
            Boolean,
            json!(true),
            cam.units,
            Some(form::options(&[
                ("true", "Included (unavailable)"),
                ("false", "Excluded"),
            ])),
        );
    }
    form::push(
        draft,
        &format!("{PREFIX}mode"),
        "Stock definition",
        Choice,
        value("/stock_spec/mode", json!("legacy_box")),
        cam.units,
        Some(form::options(&[
            ("legacy_box", "Existing box envelope"),
            ("from_model", "Allowances from model"),
            ("fixed", "Fixed size"),
            ("model_body", "Modeled stock body"),
            ("rest_from_setup", "Remaining stock from setup"),
        ])),
    );
    form::push(
        draft,
        &format!("{PREFIX}shape"),
        "Stock shape",
        Choice,
        value("/stock_spec/shape", json!("box")),
        cam.units,
        Some(form::options(&[
            ("box", "Box"),
            ("cylinder", "Cylinder"),
            ("hex", "Hexagonal bar"),
        ])),
    );
    let model_box = match setup.stock_model_box {
        Some(bounds) => bounds,
        None => resolve::box_to_model(setup.stock, setup.wcs)?,
    };
    for (suffix, label, default) in [
        ("x_min", "Stock −X allowance", 2.),
        ("x_max", "Stock +X allowance", 2.),
        ("y_min", "Stock −Y allowance", 2.),
        ("y_max", "Stock +Y allowance", 2.),
        ("z_min", "Stock −Z allowance", 2.),
        ("z_max", "Stock +Z allowance", 1.),
    ] {
        form::push(
            draft,
            &format!("{PREFIX}offset/{suffix}"),
            label,
            Length,
            value(&format!("/stock_spec/offsets/{suffix}"), json!(default)),
            cam.units,
            None,
        );
    }
    let radial = ["x_min", "x_max", "y_min", "y_max"]
        .into_iter()
        .map(|axis| {
            value(&format!("/stock_spec/offsets/{axis}"), json!(2.))
                .as_f64()
                .unwrap_or(2.)
        })
        .fold(0., f64::max);
    form::push(
        draft,
        &format!("{PREFIX}offset/radial"),
        "Radial stock allowance",
        Length,
        json!(radial),
        cam.units,
        None,
    );
    for (axis, size, min, max) in [
        (
            "x",
            model_box.max.x - model_box.min.x,
            model_box.min.x,
            model_box.max.x,
        ),
        (
            "y",
            model_box.max.y - model_box.min.y,
            model_box.min.y,
            model_box.max.y,
        ),
        (
            "z",
            model_box.max.z - model_box.min.z,
            model_box.min.z,
            model_box.max.z,
        ),
    ] {
        let label = if axis == "x" {
            "Fixed X size / diameter / across flats".into()
        } else {
            format!("Fixed {} size", axis.to_uppercase())
        };
        form::push(
            draft,
            &format!("{PREFIX}size/{axis}"),
            &label,
            Length,
            value(&format!("/stock_spec/size/{axis}"), json!(size)),
            cam.units,
            None,
        );
        form::push(
            draft,
            &format!("{PREFIX}box/min/{axis}"),
            &format!("Stock model min {}", axis.to_uppercase()),
            Length,
            json!(min),
            cam.units,
            None,
        );
        form::push(
            draft,
            &format!("{PREFIX}box/max/{axis}"),
            &format!("Stock model max {}", axis.to_uppercase()),
            Length,
            json!(max),
            cam.units,
            None,
        );
    }
    let placement = if record["stock_spec"]["placement"]["center"] == false {
        value("/stock_spec/placement/face", json!("z_min"))
    } else {
        json!("center")
    };
    form::push(
        draft,
        &format!("{PREFIX}placement"),
        "Fixed stock placement",
        Choice,
        placement,
        cam.units,
        Some(form::options(&[
            ("center", "Centered XY, model floor"),
            ("x_min", "Against model −X"),
            ("x_max", "Against model +X"),
            ("y_min", "Against model −Y"),
            ("y_max", "Against model +Y"),
            ("z_min", "Against model bottom"),
            ("z_max", "Against model top"),
        ])),
    );
    form::push(
        draft,
        &format!("{PREFIX}gap"),
        "Gap to stock face",
        Length,
        value("/stock_spec/placement/offset", json!(0.)),
        cam.units,
        None,
    );
    form::push(
        draft,
        &format!("{PREFIX}stock_body"),
        "Stock body",
        Choice,
        value("/stock_spec/body_id", Value::Null),
        cam.units,
        Some(
            context
                .bodies
                .iter()
                .map(|(id, name, _)| ChoiceOption {
                    value: id.to_string(),
                    label: name.clone(),
                    disabled: false,
                })
                .collect(),
        ),
    );
    form::push(
        draft,
        &format!("{PREFIX}source_setup"),
        "Remaining stock source",
        Choice,
        value("/stock_spec/setup_id", Value::Null),
        cam.units,
        Some(
            cam.setups
                .iter()
                .filter(|s| s.id != id)
                .map(|s| ChoiceOption {
                    value: s.id.to_string(),
                    label: s.name.clone(),
                    disabled: false,
                })
                .collect(),
        ),
    );
    form::push(
        draft,
        &format!("{PREFIX}origin"),
        "WCS origin",
        Choice,
        value("/wcs_origin/mode", json!("explicit")),
        cam.units,
        Some(form::options(&[
            ("explicit", "Entered model coordinates"),
            ("stock_box_point", "Stock box point"),
            ("model_box_point", "Model box point"),
            ("sketch_point", "Sketch point"),
        ])),
    );
    for (axis, coordinate) in [
        ("x", setup.wcs.origin.x),
        ("y", setup.wcs.origin.y),
        ("z", setup.wcs.origin.z),
    ] {
        form::push(
            draft,
            &format!("{PREFIX}anchor/{axis}"),
            &format!("WCS {} anchor", axis.to_uppercase()),
            Choice,
            value(
                &format!("/wcs_origin/{axis}"),
                json!(if axis == "z" { "max" } else { "min" }),
            ),
            cam.units,
            Some(form::options(&[
                ("min", "Minimum"),
                ("center", "Center"),
                ("max", "Maximum"),
            ])),
        );
        form::push(
            draft,
            &format!("{PREFIX}origin/{axis}"),
            &format!("WCS model {}", axis.to_uppercase()),
            Length,
            json!(coordinate),
            cam.units,
            None,
        );
    }
    let point = match &setup.wcs_origin {
        limo_cad_cam::WcsOriginSpecDto::SketchPoint { sketch, entity_id } => {
            format!("{sketch}:{entity_id}")
        }
        _ => String::new(),
    };
    let mut options: Vec<_> = context
        .points
        .iter()
        .map(|(key, name, id, _)| ChoiceOption {
            value: key.clone(),
            label: format!("{name} · Point{id}"),
            disabled: false,
        })
        .collect();
    if !point.is_empty() && !options.iter().any(|option| option.value == point) {
        options.push(ChoiceOption {
            value: point.clone(),
            label: "Saved sketch point (unavailable)".into(),
            disabled: true,
        });
    }
    form::push(
        draft,
        &format!("{PREFIX}point"),
        "WCS sketch point",
        Choice,
        json!(point),
        cam.units,
        Some(options),
    );
    form::push(
        draft,
        picking::BUTTON,
        "Viewport WCS origin",
        Boolean,
        json!(false),
        cam.units,
        None,
    );
    form::push(
        draft,
        &format!("{PREFIX}orientation"),
        "WCS orientation",
        Choice,
        json!("keep"),
        cam.units,
        Some(form::options(&[
            ("keep", "Keep current axes"),
            ("up0", "Z up · XY 0°"),
            ("up90", "Z up · XY 90°"),
            ("up180", "Z up · XY 180°"),
            ("up270", "Z up · XY 270°"),
            ("down0", "Z down · XY 0°"),
            ("down90", "Z down · XY 90°"),
            ("down180", "Z down · XY 180°"),
            ("down270", "Z down · XY 270°"),
        ])),
    );
    draft.setup = Some(context);
    Ok(())
}

fn text<'a>(draft: &'a Draft, suffix: &str) -> Result<&'a str, String> {
    form::text(draft, &format!("{PREFIX}{suffix}"))
}
fn number(draft: &Draft, suffix: &str, cam: &CamDocumentDto) -> Result<f64, String> {
    form::number(draft, &format!("{PREFIX}{suffix}"), cam.units)
}
fn precise_number(
    draft: &Draft,
    suffix: &str,
    baseline: f64,
    cam: &CamDocumentDto,
) -> Result<f64, String> {
    let path = format!("{PREFIX}{suffix}");
    if draft
        .fields
        .iter()
        .any(|field| field.path == path && field.text == field.original)
    {
        Ok(baseline)
    } else {
        number(draft, suffix, cam)
    }
}
pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    let Some(path) = path.strip_prefix(PREFIX) else {
        return true;
    };
    let mode = text(draft, "mode").unwrap_or("");
    let shape = text(draft, "shape").unwrap_or("");
    let origin = text(draft, "origin").unwrap_or("");
    if path == "pick_origin" {
        return mode != "rest_from_setup"
            && matches!(
                origin,
                "stock_box_point" | "model_box_point" | "sketch_point"
            );
    }
    if path == "shape" {
        return matches!(mode, "fixed" | "from_model");
    }
    if path.starts_with("offset/") {
        return mode == "from_model"
            && if path == "offset/radial" {
                shape != "box"
            } else {
                shape == "box" || path.starts_with("offset/z_")
            };
    }
    if path.starts_with("size/") {
        return mode == "fixed" && (shape == "box" || path != "size/y");
    }
    if path.starts_with("box/") {
        return mode == "legacy_box";
    }
    if path == "placement" {
        return mode == "fixed";
    }
    if path == "gap" {
        return mode == "fixed" && text(draft, "placement").unwrap_or("") != "center";
    }
    if path == "stock_body" {
        return mode == "model_body";
    }
    if path == "source_setup" {
        return mode == "rest_from_setup";
    }
    if path.starts_with("origin")
        || path.starts_with("anchor/")
        || path == "point"
        || path == "orientation"
    {
        if mode == "rest_from_setup" {
            return false;
        }
        if path.starts_with("anchor/") {
            return matches!(origin, "stock_box_point" | "model_box_point");
        }
        if path.starts_with("origin/") {
            return origin == "explicit";
        }
        if path == "point" {
            return origin == "sketch_point";
        }
    }
    true
}

struct ResolvedStock<'a> {
    ids: Vec<u64>,
    model: StockBoxDto,
    model_box: StockBoxDto,
    spec: CamStockSpecDto,
    wcs: WorkCoordinateSystemDto,
    shape: resolve::Shape,
    source: Option<&'a CamSetupDto>,
}
fn included_model(draft: &Draft) -> Result<(Vec<u64>, StockBoxDto), String> {
    let context = draft.setup.as_ref().ok_or("Reopen the setup editor")?;
    if !context.model_valid {
        return Err("Resolve model errors before changing setup geometry".into());
    }
    let original = &context.source;
    let chosen: Vec<_> = draft
        .fields
        .iter()
        .filter_map(|f| {
            f.path
                .strip_prefix(&format!("{PREFIX}body/"))
                .filter(|_| f.text == "true")
                .and_then(|id| id.parse::<u64>().ok())
        })
        .collect();
    let mut ids: Vec<_> = original
        .body_ids
        .iter()
        .copied()
        .filter(|id| chosen.contains(id))
        .collect();
    for id in chosen {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let model = context.model_bounds(&ids)?;
    Ok((ids, model))
}
fn resolved_stock<'a>(draft: &Draft, cam: &'a CamDocumentDto) -> Result<ResolvedStock<'a>, String> {
    let context = draft.setup.as_ref().ok_or("Reopen the setup editor")?;
    let original = &context.source;
    let (ids, model) = included_model(draft)?;
    let mode = text(draft, "mode")?;
    let offset = |name: &str| -> Result<f64, String> {
        if text(draft, "shape")? != "box" && !name.starts_with('z') {
            number(draft, "offset/radial", cam)
        } else {
            number(draft, &format!("offset/{name}"), cam)
        }
    };
    let spec = match mode {
        "legacy_box" => json!({"mode":mode}),
        "from_model" => json!({"mode":mode,"shape":text(draft,"shape")?,"offsets": {
            "x_min":offset("x_min")?,"x_max":offset("x_max")?,
            "y_min":offset("y_min")?,"y_max":offset("y_max")?,
            "z_min":offset("z_min")?,"z_max":offset("z_max")?}}),
        "fixed" => {
            let face = text(draft, "placement")?;
            json!({"mode":mode,"shape":text(draft,"shape")?,
            "size":{"x":number(draft,"size/x",cam)?,"y":if text(draft,"shape")?=="box"{number(draft,"size/y",cam)?}else{0.},"z":number(draft,"size/z",cam)?},
            "placement":{"center":face=="center","face":if face=="center" {Value::Null}else{json!(face)},"offset":if face=="center"{0.}else{number(draft,"gap",cam)?}}})
        }
        "model_body" => {
            json!({"mode":mode,"body_id":text(draft,"stock_body")?.parse::<u64>().map_err(|_|"Choose a stock body")?})
        }
        "rest_from_setup" => {
            json!({"mode":mode,"setup_id":text(draft,"source_setup")?.parse::<u64>().map_err(|_|"Choose a source setup")?})
        }
        _ => return Err("Choose a stock definition".into()),
    };
    let stock_changed = [
        "mode",
        "shape",
        "offset/",
        "size/",
        "placement",
        "gap",
        "stock_body",
        "source_setup",
    ]
    .into_iter()
    .any(|suffix| form::changed(draft, &format!("{PREFIX}{suffix}")));
    let spec: CamStockSpecDto = if stock_changed {
        serde_json::from_value(spec).map_err(|e| e.to_string())?
    } else {
        original.stock_spec.clone()
    };
    let orientation = text(draft, "orientation")?;
    let wcs = resolve::orientation(orientation, original.wcs)?;
    let legacy = if mode == "legacy_box" {
        let original_box = original
            .model_box
            .ok_or("The saved stock envelope is invalid")?;
        StockBoxDto {
            min: Point3Dto::new(
                precise_number(draft, "box/min/x", original_box.min.x, cam)?,
                precise_number(draft, "box/min/y", original_box.min.y, cam)?,
                precise_number(draft, "box/min/z", original_box.min.z, cam)?,
            ),
            max: Point3Dto::new(
                precise_number(draft, "box/max/x", original_box.max.x, cam)?,
                precise_number(draft, "box/max/y", original_box.max.y, cam)?,
                precise_number(draft, "box/max/z", original_box.max.z, cam)?,
            ),
        }
    } else {
        original.stock
    };
    let source = match &spec {
        CamStockSpecDto::RestFromSetup { setup_id } => Some(
            cam.setup(*setup_id)
                .filter(|s| s.id != original.id)
                .ok_or("Choose a different source setup")?,
        ),
        _ => None,
    };
    let stock_bounds = match &spec {
        CamStockSpecDto::ModelBody { body_id } => context.model_bounds(&[*body_id])?,
        _ => model,
    };
    let (model_box, shape) = resolve::stock(&spec, stock_bounds, legacy, source, wcs)?;
    model_box.validate()?;
    Ok(ResolvedStock {
        ids,
        model,
        model_box,
        spec,
        wcs,
        shape,
        source,
    })
}
fn selected_point(draft: &Draft) -> Result<&(String, String, u64, Point3Dto), String> {
    let key = text(draft, "point")?;
    draft.setup.as_ref().ok_or("Reopen the setup editor")?.points.iter()
        .find(|(candidate, _, _, point)| candidate == key && [point.x, point.y, point.z].iter().all(|v| v.is_finite()))
        .ok_or_else(|| "Saved WCS sketch point is unavailable; select a current point or change the origin mode".into())
}
pub(super) fn apply(draft: &Draft, record: &mut Value, cam: &CamDocumentDto) -> Result<(), String> {
    let moved_point =
        if text(draft, "mode")? != "rest_from_setup" && text(draft, "origin")? == "sketch_point" {
            selected_point(draft)?.3
                != draft
                    .setup
                    .as_ref()
                    .ok_or("Reopen the setup editor")?
                    .source
                    .wcs
                    .origin
        } else {
            false
        };
    if !form::changed(draft, PREFIX) && !moved_point {
        return Ok(());
    }
    let ResolvedStock {
        ids,
        model,
        model_box,
        spec,
        mut wcs,
        shape,
        source,
    } = resolved_stock(draft, cam)?;
    let origin_mode = text(draft, "origin")?;
    let origin_spec = if let Some(source) = source {
        wcs = source.wcs;
        serde_json::to_value(&source.wcs_origin).map_err(|e| e.to_string())?
    } else if origin_mode == "explicit" {
        let original = draft
            .setup
            .as_ref()
            .ok_or("Reopen the setup editor")?
            .source
            .wcs
            .origin;
        wcs.origin = Point3Dto::new(
            precise_number(draft, "origin/x", original.x, cam)?,
            precise_number(draft, "origin/y", original.y, cam)?,
            precise_number(draft, "origin/z", original.z, cam)?,
        );
        json!({"mode":"explicit"})
    } else if origin_mode == "sketch_point" {
        let (_, sketch, id, point) = selected_point(draft)?;
        wcs.origin = *point;
        json!({"mode":"sketch_point","sketch":sketch,"entity_id":id})
    } else {
        let b = if origin_mode == "stock_box_point" {
            model_box
        } else if origin_mode == "model_box_point" {
            model
        } else {
            return Err("Choose a WCS origin".into());
        };
        let anchor = |axis: &str, min: f64, max: f64| -> Result<f64, String> {
            Ok(match text(draft, &format!("anchor/{axis}"))? {
                "min" => min,
                "center" => (min + max) * 0.5,
                "max" => max,
                _ => return Err("Choose a WCS box anchor".into()),
            })
        };
        wcs.origin = Point3Dto::new(
            anchor("x", b.min.x, b.max.x)?,
            anchor("y", b.min.y, b.max.y)?,
            anchor("z", b.min.z, b.max.z)?,
        );
        json!({"mode":origin_mode,"x":text(draft,"anchor/x")?,"y":text(draft,"anchor/y")?,"z":text(draft,"anchor/z")?})
    };
    let resolved = resolve::resolved(shape, wcs);
    record["body_ids"] = json!(ids);
    record["stock_spec"] = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    record["wcs_origin"] = origin_spec;
    record["wcs"] = serde_json::to_value(wcs).map_err(|e| e.to_string())?;
    record["stock_model_box"] = serde_json::to_value(model_box).map_err(|e| e.to_string())?;
    let stock = if let Some(source) = source {
        source.stock
    } else {
        resolve::box_to_setup(model_box, wcs)?
    };
    record["stock"] = serde_json::to_value(stock).map_err(|e| e.to_string())?;
    record["resolved_stock"] = resolved;
    Ok(())
}

pub(super) fn preview(draft: &Draft, cam: &CamDocumentDto) -> Option<String> {
    draft.setup.as_ref()?;
    let mut record = draft.record.clone();
    if let Err(error) = apply(draft, &mut record, cam) {
        return Some(error);
    }
    let setup: CamSetupDto = serde_json::from_value(record).ok()?;
    let display = |v| cam.units.from_mm(v);
    Some(format!(
        "WCS {:.2}, {:.2}, {:.2} {}\nStock {:.2} × {:.2} × {:.2} {}",
        display(setup.wcs.origin.x),
        display(setup.wcs.origin.y),
        display(setup.wcs.origin.z),
        cam.units.length_label(),
        display(setup.stock.max.x - setup.stock.min.x),
        display(setup.stock.max.y - setup.stock.min.y),
        display(setup.stock.max.z - setup.stock.min.z),
        cam.units.length_label()
    ))
}
