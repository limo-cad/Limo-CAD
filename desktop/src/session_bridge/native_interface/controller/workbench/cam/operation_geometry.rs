//! Native geometry forms over the canonical CAM paths and reference records.
//! The shared sketch adapters resolve selected geometry; the editor never
//! infers a loop, bevel or hole from its display silhouette.
use super::*;
use limo_cad_cam::{CamChainRefDto, CamHoleDto, CamSetupDto, CamUnits, Point2Dto};
use limo_cad_sketch::{ChainMode, ChainSource, EdgeChainRequest, SketchDto};
use limo_cad_solid::SolidSceneDto;
use std::sync::Arc;

mod chains;
pub(super) mod hole_picking;
mod holes;
pub(super) mod picking;
pub(super) mod points;

const PREFIX: &str = "/native/geometry/";

#[derive(Clone)]
pub(super) struct Context {
    pub(super) setup: CamSetupDto,
    pub(super) scene: Arc<SolidSceneDto>,
    pub(super) sketches: Arc<[SketchDto]>,
    pub(super) model_options: Vec<ChoiceOption>,
    pub(super) sketch_options: Vec<ChoiceOption>,
    pub(super) holes: Vec<(String, CamHoleDto)>,
}

impl Context {
    pub(super) fn new(
        setup: &CamSetupDto,
        scene: &Arc<SolidSceneDto>,
        sketches: &[SketchDto],
    ) -> Self {
        let options = |source| {
            limo_cad_sketch::edge_chain_candidates(scene, sketches, source, &setup.body_ids)
                .into_iter()
                .filter(|edge| {
                    let first = project(edge.points[0], setup);
                    edge.points.iter().any(|point| {
                        let next = project(*point, setup);
                        (next[0] - first[0]).hypot(next[1] - first[1])
                            > limo_cad_core::edge_chain::JOIN_TOLERANCE
                    })
                })
                .enumerate()
                .map(|(index, edge)| {
                    let owner = edge
                        .key
                        .strip_prefix("edge:")
                        .and_then(|key| key.split_once(':'))
                        .and_then(|(body, _)| body.parse::<u64>().ok())
                        .and_then(|id| scene.bodies.iter().find(|body| body.id.0 == id))
                        .map(|body| body.name.clone())
                        .or_else(|| {
                            edge.key.strip_prefix("sketch:").and_then(|key| {
                                key.rsplit_once(':')
                                    .map(|(name, id)| format!("{name} · curve {id}"))
                            })
                        })
                        .unwrap_or_else(|| "Model".into());
                    ChoiceOption {
                        value: edge.key,
                        label: if source == ChainSource::Model {
                            format!("{owner} · edge {}", index + 1)
                        } else {
                            owner
                        },
                        disabled: false,
                    }
                })
                .collect()
        };
        Self {
            setup: setup.clone(),
            scene: Arc::clone(scene),
            sketches: sketches.to_vec().into(),
            model_options: options(ChainSource::Model),
            sketch_options: options(ChainSource::Sketch),
            holes: holes::candidates(setup, scene),
        }
    }
}

pub(super) fn supports(record: &Value) -> bool {
    matches!(
        record["kind"].as_str(),
        Some("contour2d" | "pocket2d" | "chamfer2d" | "drill" | "thread")
    )
}

pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    context: &Context,
) -> Result<(), String> {
    match draft.record["kind"].as_str().unwrap_or("") {
        "contour2d" | "pocket2d" | "chamfer2d" => chains::extend(draft, cam, context),
        "drill" | "thread" => holes::extend(draft, cam, context),
        _ => Ok(()),
    }
}

pub(super) fn changed(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    path: &str,
    context: &Context,
) -> Result<(), String> {
    if !path.starts_with(PREFIX) && !path.starts_with("/native/ui/geometry_") {
        return Ok(());
    }
    if matches!(draft.record["kind"].as_str(), Some("drill" | "thread")) {
        holes::changed(draft, cam, path, context)
    } else {
        chains::changed(draft, cam, path, context)
    }
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if !path.starts_with(PREFIX) && !path.starts_with("/native/ui/geometry_") {
        return false;
    }
    if matches!(draft.record["kind"].as_str(), Some("drill" | "thread")) {
        holes::visible(draft, path)
    } else {
        chains::visible(draft, path)
    }
}

pub(super) fn apply(
    draft: &Draft,
    record: &mut Value,
    units: CamUnits,
    context: &Context,
) -> Result<bool, String> {
    if matches!(record["kind"].as_str(), Some("drill" | "thread")) {
        let before = (record["holes"].clone(), record["points"].clone());
        holes::apply(draft, record, units, context)?;
        return Ok(before != (record["holes"].clone(), record["points"].clone()));
    }
    if !form::changed(draft, PREFIX) {
        return Ok(false);
    }
    if !context.scene.errors.is_empty() {
        return Err("Resolve model errors before editing CAM geometry".into());
    }
    match record["kind"].as_str().unwrap_or("") {
        "contour2d" | "pocket2d" | "chamfer2d" => chains::apply(draft, record, units, context)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn boolean(draft: &mut Draft, path: &str, label: &str, value: bool, units: CamUnits) {
    form::push(
        draft,
        path,
        label,
        InputKind::Boolean,
        json!(value),
        units,
        Some(form::options(&[("true", "Yes"), ("false", "No")])),
    );
}
fn count(draft: &Draft, path: &str, maximum: usize) -> Result<usize, String> {
    let value = form::text(draft, path)?
        .parse::<usize>()
        .map_err(|_| "Enter a whole item count")?;
    if value > maximum {
        return Err(format!("This collection supports at most {maximum} items"));
    }
    Ok(value)
}
fn number(
    draft: &Draft,
    path: &str,
    original: Option<f64>,
    units: CamUnits,
) -> Result<f64, String> {
    if !form::changed(draft, path) {
        if let Some(original) = original {
            return Ok(original);
        }
    }
    form::number(draft, path, units)
}
pub(super) fn project(point: [f64; 3], setup: &CamSetupDto) -> [f64; 3] {
    let p = [
        point[0] - setup.wcs.origin.x,
        point[1] - setup.wcs.origin.y,
        point[2] - setup.wcs.origin.z,
    ];
    [setup.wcs.x_axis, setup.wcs.y_axis, setup.wcs.z_axis]
        .map(|axis| p.into_iter().zip(axis).map(|(a, b)| a * b).sum())
}
