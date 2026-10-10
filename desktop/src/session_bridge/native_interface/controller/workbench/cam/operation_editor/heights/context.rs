use super::*;
use limo_cad_cam::{CamChainRefDto, CamChainSource, CamHoleDto};
use limo_cad_core::edge_chain::{self, Edge, JOIN_TOLERANCE};

#[derive(Clone)]
pub(crate) struct Context {
    model: Result<(f64, f64), String>,
    stock: (f64, f64),
    holes: Result<(f64, f64), String>,
    selection: Result<f64, String>,
    geometry: HashMap<String, Result<f64, String>>,
    picker: Option<std::sync::Arc<picking::Source>>,
    staged: HashMap<String, limo_cad_cam::CamHeightGeometryDto>,
    pub(super) has_holes: bool,
    pub(super) has_selection: bool,
    pub(super) modeled_top: Option<f64>,
}
fn z(point: [f64; 3], wcs: WorkCoordinateSystemDto) -> f64 {
    [
        point[0] - wcs.origin.x,
        point[1] - wcs.origin.y,
        point[2] - wcs.origin.z,
    ]
    .into_iter()
    .zip(wcs.z_axis)
    .map(|(a, b)| a * b)
    .sum()
}
impl Context {
    pub(crate) fn with_picker(mut self, source: std::sync::Arc<picking::Source>) -> Self {
        self.picker = Some(source);
        self
    }

    pub(super) fn picker_source(&self) -> Result<std::sync::Arc<picking::Source>, String> {
        self.picker
            .clone()
            .ok_or("Reopen the operation editor".into())
    }

    pub(super) fn staged_geometry(&self, row: &str) -> Option<&limo_cad_cam::CamHeightGeometryDto> {
        self.staged.get(row)
    }

    pub(super) fn stage_geometry(
        &mut self,
        row: &str,
        geometry: limo_cad_cam::CamHeightGeometryDto,
        base: f64,
    ) -> Result<(), String> {
        let key = serde_json::to_string(&geometry).map_err(|error| error.to_string())?;
        self.geometry.insert(key, Ok(base));
        self.staged.insert(row.into(), geometry);
        Ok(())
    }

    pub(crate) fn with_picks_from(mut self, previous: &Self) -> Self {
        self.picker = previous.picker.clone();
        self.staged.clone_from(&previous.staged);
        self.geometry.extend(previous.geometry.clone());
        self
    }

    /// Resolve saved independent height references once with the editor's
    /// immutable setup/model receipt, using the same resolver as CAM planning.
    pub(crate) fn with_geometry(
        mut self,
        cam: &CamDocumentDto,
        setup: &CamSetupDto,
        operation_id: u64,
        scene: &SolidSceneDto,
        sketches: &[SketchDto],
    ) -> Self {
        for intent in cam
            .height_expressions
            .iter()
            .filter(|entry| entry.operation_id == operation_id)
        {
            for expression in [
                Some(&intent.clearance),
                Some(&intent.retract),
                Some(&intent.feed),
                Some(&intent.top),
                intent.bottom.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(geometry) = &expression.geometry {
                    if let Ok(key) = serde_json::to_string(geometry) {
                        self.geometry.insert(
                            key,
                            limo_cad_sketch::resolve_cam_height_geometry(
                                geometry, setup, scene, sketches,
                            ),
                        );
                    }
                }
            }
        }
        self
    }

    pub(super) fn geometry_base(
        &self,
        geometry: &limo_cad_cam::CamHeightGeometryDto,
    ) -> Result<f64, String> {
        self.model.as_ref().map_err(Clone::clone)?;
        let key = serde_json::to_string(geometry).map_err(|error| error.to_string())?;
        self.geometry
            .get(&key)
            .ok_or("The picked height reference is no longer available")?
            .as_ref()
            .copied()
            .map_err(Clone::clone)
    }
    /// Use only after the geometry adapter has resolved every association.
    /// Its immutable scene/setup receipt already owns the model/stock bases;
    /// changing hole rows must not rescan those meshes on the UI thread.
    pub(crate) fn with_resolved_holes(&self, operation: &CamOperationDto) -> Option<Self> {
        let holes = match operation {
            CamOperationDto::Drill { holes, .. } | CamOperationDto::Thread { holes, .. } => holes,
            _ => return None,
        };
        let mut next = self.clone();
        next.has_holes = holes.iter().any(|hole| hole.face_key.is_some());
        next.holes = resolved_hole_levels(holes);
        Some(next)
    }
    pub(crate) fn new(
        setup: &CamSetupDto,
        operation: &CamOperationDto,
        scene: &SolidSceneDto,
        sketches: &[SketchDto],
    ) -> Self {
        let mut top = f64::NEG_INFINITY;
        let mut bottom = f64::INFINITY;
        for body in scene
            .bodies
            .iter()
            .filter(|body| setup.body_ids.contains(&body.id))
        {
            for p in body.mesh.positions.as_chunks::<3>().0 {
                let level = z(
                    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])],
                    setup.wcs,
                );
                top = top.max(level);
                bottom = bottom.min(level);
            }
        }
        if !top.is_finite() {
            top = setup.stock.max.z;
        }
        if !bottom.is_finite() {
            bottom = setup.stock.min.z;
        }
        let model = if scene.errors.is_empty() {
            Ok((top, bottom))
        } else {
            Err("Resolve model errors before editing model-relative heights".into())
        };
        let holes = match operation {
            CamOperationDto::Drill { holes, .. } | CamOperationDto::Thread { holes, .. } => {
                holes.as_slice()
            }
            _ => &[],
        };
        let has_holes = holes.iter().any(|hole| hole.face_key.is_some());
        let holes = hole_levels(holes, setup, scene);
        let has_selection = match operation {
            CamOperationDto::Contour2d { chain_ref, .. }
            | CamOperationDto::Pocket2d { chain_ref, .. }
            | CamOperationDto::Chamfer2d { chain_ref, .. } => {
                chain_ref.as_ref().is_some_and(|r| !r.keys.is_empty())
            }
            _ => false,
        };
        let selection = selection_level(operation, setup, scene, sketches);
        let modeled_top = if matches!(
            operation,
            CamOperationDto::Chamfer2d {
                modeled_chamfer: Some(_),
                ..
            }
        ) {
            operation
                .chamfer_chains()
                .iter()
                .map(|chain| chain.top_z)
                .reduce(f64::max)
        } else {
            None
        };
        Self {
            model,
            stock: (setup.stock.max.z, setup.stock.min.z),
            holes,
            selection,
            geometry: HashMap::new(),
            picker: None,
            staged: HashMap::new(),
            has_holes,
            has_selection,
            modeled_top,
        }
    }
    pub(super) fn base(
        &self,
        reference: CamHeightReferenceDto,
        values: &HashMap<&str, f64>,
    ) -> Result<f64, String> {
        use CamHeightReferenceDto::*;
        Ok(match reference {
            ModelTop => self.model.as_ref().map_err(Clone::clone)?.0,
            ModelBottom => self.model.as_ref().map_err(Clone::clone)?.1,
            StockTop => self.stock.0,
            StockBottom => self.stock.1,
            Origin => 0.,
            HoleTop => self.holes.as_ref().map_err(Clone::clone)?.0,
            HoleBottom => self.holes.as_ref().map_err(Clone::clone)?.1,
            Selection => *self.selection.as_ref().map_err(Clone::clone)?,
            Geometry => return Err("A picked height needs its saved geometry identity".into()),
            Bottom | Top | Feed | Retract => {
                let name = match reference {
                    Bottom => "bottom",
                    Top => "top",
                    Feed => "feed",
                    _ => "retract",
                };
                *values
                    .get(name)
                    .ok_or("A height may only reference an earlier available height")?
            }
        })
    }
}
fn resolved_hole_levels(holes: &[CamHoleDto]) -> Result<(f64, f64), String> {
    let mut top = f64::NEG_INFINITY;
    let mut bottom = f64::INFINITY;
    for hole in holes {
        if !hole.top_z.is_finite() || !hole.bottom_z.is_finite() || hole.top_z <= hole.bottom_z {
            return Err("A hole has no valid resolved axial span".into());
        }
        top = top.max(hole.top_z);
        bottom = bottom.min(hole.bottom_z);
    }
    if top.is_finite() && bottom.is_finite() {
        Ok((top, bottom))
    } else {
        Err("Select associated hole geometry before using hole heights".into())
    }
}
fn hole_levels(
    holes: &[CamHoleDto],
    setup: &CamSetupDto,
    scene: &SolidSceneDto,
) -> Result<(f64, f64), String> {
    let mut top = f64::NEG_INFINITY;
    let mut bottom = f64::INFINITY;
    for hole in holes {
        let (hi, lo) = if let Some(reference) = &hole.face_key {
            let (body, face) = reference
                .split_once(':')
                .ok_or("Invalid picked-hole reference")?;
            let body_id = body.parse::<u64>().map_err(|_| "Invalid hole body")?;
            let face_id = face.parse::<u64>().map_err(|_| "Invalid hole face")?;
            let body = scene
                .bodies
                .iter()
                .find(|body| body.id.0 == body_id)
                .ok_or("A picked-hole body is missing")?;
            let face = body
                .faces
                .iter()
                .find(|face| face.id.0 == face_id)
                .ok_or("A picked-hole face is missing")?;
            let cylinder = face
                .cylinder
                .ok_or("A picked-hole face is no longer cylindrical")?;
            let axis = [cylinder.axis.x, cylinder.axis.y, cylinder.axis.z];
            let length = axis.into_iter().map(|v| v * v).sum::<f64>().sqrt();
            let alignment = axis
                .into_iter()
                .zip(setup.wcs.z_axis)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                / length;
            if !alignment.is_finite() || alignment.abs() < 1. - 1e-6 {
                return Err("A picked hole is no longer aligned with setup Z".into());
            }
            let start = face.first_index as usize;
            let end = start.saturating_add(face.index_count as usize);
            let mut hi = f64::NEG_INFINITY;
            let mut lo = f64::INFINITY;
            for index in body.mesh.indices.get(start..end).unwrap_or_default() {
                let i = *index as usize * 3;
                let Some(p) = body.mesh.positions.get(i..i.saturating_add(3)) else {
                    continue;
                };
                let level = z(
                    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])],
                    setup.wcs,
                );
                hi = hi.max(level);
                lo = lo.min(level);
            }
            if !hi.is_finite() || !lo.is_finite() || hi <= lo + 1e-9 {
                return Err("A picked hole has no current axial span".into());
            }
            (hi, lo)
        } else {
            (hole.top_z, hole.bottom_z)
        };
        top = top.max(hi);
        bottom = bottom.min(lo);
    }
    if top.is_finite() && bottom.is_finite() {
        Ok((top, bottom))
    } else {
        Err("Select associated hole geometry before using hole heights".into())
    }
}
fn selection_level(
    operation: &CamOperationDto,
    setup: &CamSetupDto,
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
) -> Result<f64, String> {
    if matches!(operation,CamOperationDto::Chamfer2d{additional_chains,..}if !additional_chains.is_empty())
    {
        return operation
            .chamfer_chains()
            .into_iter()
            .map(|chain| {
                selection_level(&operation.with_chamfer_chain(chain), setup, scene, sketches)
            })
            .try_fold(f64::NEG_INFINITY, |highest, next| {
                next.map(|next| highest.max(next))
            });
    }
    let reference = match operation {
        CamOperationDto::Contour2d { chain_ref, .. }
        | CamOperationDto::Pocket2d { chain_ref, .. }
        | CamOperationDto::Chamfer2d { chain_ref, .. } => chain_ref.as_ref(),
        _ => None,
    }
    .ok_or("Select associated edge or sketch geometry before using Selection height")?;
    match reference.source {
        CamChainSource::Sketch => {
            let key = reference
                .keys
                .first()
                .and_then(|key| key.strip_prefix("sketch:"))
                .ok_or("Missing sketch reference")?;
            let (name, id) = key.rsplit_once(':').ok_or("Invalid sketch reference")?;
            id.parse::<u64>()
                .map_err(|_| "Invalid sketch entity reference")?;
            let sketch = sketches
                .iter()
                .find(|sketch| sketch.name == name)
                .ok_or("The selected sketch is missing")?;
            Ok(z(sketch.basis.origin, setup.wcs))
        }
        CamChainSource::Model => model_selection_level(reference, setup, scene),
    }
}
fn model_selection_level(
    reference: &CamChainRefDto,
    setup: &CamSetupDto,
    scene: &SolidSceneDto,
) -> Result<f64, String> {
    let all: Vec<_> = scene
        .bodies
        .iter()
        .filter(|body| setup.body_ids.is_empty() || setup.body_ids.contains(&body.id))
        .flat_map(|body| {
            body.edges.iter().filter_map(|edge| {
                let mut points: Vec<_> = edge.points.iter().map(|p| [p.x, p.y, p.z]).collect();
                if points.len() < 2 {
                    return None;
                }
                let closed = edge.circle.is_some_and(|circle| circle.closed)
                    || edge_chain::distance(points[0], *points.last().unwrap()) <= JOIN_TOLERANCE;
                if closed
                    && edge_chain::distance(points[0], *points.last().unwrap()) <= JOIN_TOLERANCE
                {
                    points.pop();
                }
                Some(Edge {
                    key: format!("edge:{}:{}", body.id.0, edge.key),
                    scope: format!("body:{}", body.id.0),
                    points,
                    closed,
                })
            })
        })
        .collect();
    let chain = edge_chain::resolve(&all, &reference.keys, false)?;
    let first = chain.points.first().ok_or("Selected geometry is empty")?;
    let level = z(*first, setup.wcs);
    if chain
        .points
        .iter()
        .any(|point| (z(*point, setup.wcs) - level).abs() > JOIN_TOLERANCE)
    {
        return Err("Selection height needs a chain in one setup-Z plane".into());
    }
    Ok(level)
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
