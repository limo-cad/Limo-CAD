//! Cold paper jobs own only selected exact definitions. Copying geometry costs
//! time/memory at capture, but HLR never retains or locks the live workspace.
//! Finished projections may cross annotation revisions only after coherent
//! scene, actual geometry generation and fully resolved request comparison.
use super::*;
use limo_cad_sketch::{AssemblyDocumentDto, DrawingDocumentDto, DrawingSheetDto, DrawingViewDto};
use std::collections::BTreeMap;
use std::sync::Weak;

/// Owned export input. Only capture locks the live workspace; HLR, paper
/// markup and downstream PDF preparation happen after that guard drops.
pub struct PreparedDrawingExport {
    drawing: Arc<DrawingDocumentDto>,
    snapshot: DetachedDrawingSnapshot,
    request: limo_cad_occt::drawing_export::DrawingExportRequest,
    units: limo_cad_core::UnitSystem,
    title: String,
    geometry_revision: u64,
}

impl PreparedDrawingExport {
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn sheet_id(&self) -> u64 {
        self.request.sheet_id
    }
    pub fn content(&self) -> Result<String, String> {
        self.content_observing(|_, _| {})
    }
    pub fn content_observing(
        &self,
        mut completed: impl FnMut(&DrawingProjectionRequest, &limo_cad_occt::DrawingProjectionDto),
    ) -> Result<String, String> {
        limo_cad_occt::drawing_export::export_sheet_with_units(
            &self.drawing,
            &self.snapshot.scene,
            &self.snapshot.assembly,
            &self.request,
            self.units,
            |request| {
                let resolved = limo_cad_occt::resolve_drawing_request(
                    &self.snapshot.scene,
                    &self.snapshot.assembly,
                    request,
                )
                .map_err(|e| e.to_string())?;
                let id = self
                    .snapshot
                    .requests
                    .iter()
                    .find(|(_, captured)| *captured == resolved)
                    .map(|(id, _)| *id)
                    .ok_or("Export projection differs from captured intent")?;
                let result = self.snapshot.project_captured(id, &resolved)?;
                completed(request, &result.projection);
                Ok((*result.projection).clone())
            },
        )
    }
}

#[derive(Clone)]
pub struct SharedDrawingProjection {
    pub projection: Arc<limo_cad_occt::DrawingProjectionDto>,
    pub basis: DrawingProjectionBasis,
}

/// In-process authority, never deserialized. Weak scene ownership prevents a
/// completed bundle from pinning retired mesh/topology while preventing ABA.
pub struct FinishedDrawingProjections {
    owner: String,
    scene: Weak<SolidSceneDto>,
    identities: BTreeMap<limo_cad_core::BodyId, limo_cad_occt::KernelBodyGeometryIdentity>,
    requests: Vec<(u64, DrawingProjectionRequest)>,
    projections: BTreeMap<u64, SharedDrawingProjection>,
}

impl FinishedDrawingProjections {
    /// The cache charges this bundle alongside independently owned paper
    /// sources, including actual capacities rather than just serialized length.
    pub fn retained_bytes(&self) -> usize {
        let mut bytes = self
            .identities
            .len()
            .saturating_mul(128)
            .saturating_add(self.owner.capacity())
            .saturating_add(self.projections.len().saturating_mul(128))
            .saturating_add(
                self.requests
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(u64, DrawingProjectionRequest)>()),
            );
        for (_, request) in &self.requests {
            bytes = bytes
                .saturating_add(
                    request
                        .body_ids
                        .capacity()
                        .saturating_mul(std::mem::size_of::<limo_cad_core::BodyId>()),
                )
                .saturating_add(
                    request
                        .occurrence_ids
                        .capacity()
                        .saturating_mul(std::mem::size_of::<u64>()),
                );
            if let Some(poses) = &request.resolved_occurrences {
                bytes = bytes.saturating_add(
                    poses
                        .capacity()
                        .saturating_mul(std::mem::size_of::<InstanceBodyPoseDto>()),
                );
            }
        }
        for resolved in self.projections.values() {
            let projection = &resolved.projection;
            bytes =
                bytes.saturating_add(std::mem::size_of::<limo_cad_occt::DrawingProjectionDto>());
            for lines in [&projection.visible, &projection.hidden, &projection.section] {
                bytes = bytes.saturating_add(
                    lines
                        .capacity()
                        .saturating_mul(std::mem::size_of::<limo_cad_occt::DrawingPolylineDto>()),
                );
                for line in lines {
                    bytes = bytes.saturating_add(
                        line.points
                            .capacity()
                            .saturating_mul(std::mem::size_of::<[f64; 2]>()),
                    );
                }
            }
            bytes =
                bytes
                    .saturating_add(projection.anchors.capacity().saturating_mul(
                        std::mem::size_of::<limo_cad_occt::DrawingProjectionAnchorDto>(),
                    ))
                    .saturating_add(projection.circles.capacity().saturating_mul(
                        std::mem::size_of::<limo_cad_occt::DrawingProjectedCircleDto>(),
                    ));
            for anchor in &projection.anchors {
                bytes = bytes.saturating_add(anchor.edge_key.capacity());
            }
            for circle in &projection.circles {
                bytes = bytes.saturating_add(circle.edge_key.capacity());
            }
            for (key, signature) in &projection.topology_signatures {
                bytes = bytes
                    .saturating_add(128)
                    .saturating_add(key.capacity())
                    .saturating_add(signature.capacity());
            }
        }
        bytes
    }
}

pub struct DetachedDrawingSnapshot {
    kernel: Option<limo_cad_occt::DetachedDrawingKernel>,
    scene: Arc<SolidSceneDto>,
    assembly: AssemblyDocumentDto,
    identities: BTreeMap<limo_cad_core::BodyId, limo_cad_occt::KernelBodyGeometryIdentity>,
    requests: Vec<(u64, DrawingProjectionRequest)>,
    projections: Mutex<BTreeMap<u64, SharedDrawingProjection>>,
    reused: Option<Arc<FinishedDrawingProjections>>,
    owner: String,
}

impl NativeEngineHost {
    pub fn prepare_drawing_export(
        &self,
        request: &limo_cad_occt::drawing_export::DrawingExportRequest,
    ) -> Result<PreparedDrawingExport, String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let drawing = inner.manager.drawing_document_snapshot();
        drawing.validate()?;
        drawing.ensure_projected_sheet_supported(request.sheet_id)?;
        let sheet = drawing
            .sheets
            .iter()
            .find(|sheet| sheet.id == request.sheet_id)
            .ok_or("Drawing sheet does not exist")?;
        let title = format!("{} — {}", inner.manager.document().name(), sheet.name);
        let snapshot = capture_snapshot(
            inner,
            workspace.verification_owner_key(),
            (&drawing, inner.geometry_revision),
            sheet,
            (&sheet.views, false),
            None,
        )?;
        Ok(PreparedDrawingExport {
            drawing,
            snapshot,
            request: request.clone(),
            units: inner.manager.document().settings().units,
            title,
            geometry_revision: inner.geometry_revision,
        })
    }
    /// Read-only proof check. Attached callers must separately retain their
    /// original publisher/ticket receipt through final response publication.
    pub fn validate_drawing_export(&self, prepared: &PreparedDrawingExport) -> Result<(), String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        if workspace.verification_owner_key() != prepared.snapshot.owner {
            return Err("The owning drawing document was replaced during export".into());
        }
        let inner = workspace.active();
        if inner.geometry_revision != prepared.geometry_revision
            || !Arc::ptr_eq(
                &prepared.drawing,
                &inner.manager.drawing_document_snapshot(),
            )
            || !Arc::ptr_eq(
                &prepared.snapshot.scene,
                &inner.manager.solid_scene_snapshot(),
            )
            || &prepared.snapshot.assembly != inner.manager.assembly_document_ref()
            || prepared.units != inner.manager.document().settings().units
        {
            return Err("The drawing or its source changed during export".into());
        }
        let identities = inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        if prepared
            .snapshot
            .identities
            .iter()
            .any(|(id, identity)| identities.get(id) != Some(identity))
        {
            return Err("The native geometry changed or was replaced during drawing export".into());
        }
        Ok(())
    }
    pub fn drawing_presentation_snapshot(
        &self,
    ) -> (Arc<DrawingDocumentDto>, u64, limo_cad_core::UnitSystem) {
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active();
        (
            inner.manager.drawing_document_snapshot(),
            inner.geometry_revision,
            inner.manager.document().settings().units,
        )
    }
    pub fn detached_drawing_snapshot(
        &self,
        expected: &Arc<DrawingDocumentDto>,
        geometry_revision: u64,
        sheet: &DrawingSheetDto,
        original_views: &[DrawingViewDto],
        repairing: bool,
    ) -> Result<DetachedDrawingSnapshot, String> {
        self.detached_drawing_snapshot_reusing(
            expected,
            geometry_revision,
            sheet,
            (original_views, repairing),
            None,
        )
    }

    pub fn detached_drawing_snapshot_reusing(
        &self,
        expected: &Arc<DrawingDocumentDto>,
        geometry_revision: u64,
        sheet: &DrawingSheetDto,
        intent: (&[DrawingViewDto], bool),
        previous: Option<&Arc<FinishedDrawingProjections>>,
    ) -> Result<DetachedDrawingSnapshot, String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        capture_snapshot(
            workspace.active(),
            workspace.verification_owner_key(),
            (expected, geometry_revision),
            sheet,
            intent,
            previous,
        )
    }
}

fn capture_snapshot(
    inner: &NativeEngine,
    owner: String,
    expected: (&Arc<DrawingDocumentDto>, u64),
    sheet: &DrawingSheetDto,
    intent: (&[DrawingViewDto], bool),
    previous: Option<&Arc<FinishedDrawingProjections>>,
) -> Result<DetachedDrawingSnapshot, String> {
    let (expected, geometry_revision) = expected;
    let (original_views, repairing) = intent;
    if inner.geometry_revision != geometry_revision
        || !Arc::ptr_eq(expected, &inner.manager.drawing_document_snapshot())
    {
        return Err("The drawing changed before projection capture".into());
    }
    let scene = inner.manager.solid_scene_snapshot();
    if !scene.errors.is_empty() {
        return Err("Resolve timeline errors before generating a drawing view.".into());
    }
    let assembly = inner.manager.assembly_document_ref().clone();

    let mut bodies = BTreeSet::new();
    let mut requests = Vec::new();
    // Derived planes resolve against full saved intent, but only projected
    // views' selected definitions need an independent native shape.
    for view in &sheet.views {
        let (views, view) = if repairing {
            (
                original_views,
                original_views
                    .iter()
                    .find(|saved| saved.id == view.id)
                    .ok_or("Repair view was removed")?,
            )
        } else {
            (sheet.views.as_slice(), view)
        };
        let request =
            limo_cad_occt::drawing_export::projection_request(view, views, &scene, &assembly)?;
        let request = limo_cad_occt::resolve_drawing_request(&scene, &assembly, &request)
            .map_err(|e| e.to_string())?;
        bodies.extend(
            limo_cad_occt::drawing_required_body_ids(&scene, &assembly, &request)
                .map_err(|e| e.to_string())?,
        );
        if requests.iter().any(|(id, _)| *id == view.id) {
            return Err("Drawing sheet contains duplicate view identities".into());
        }
        requests.push((view.id, request));
    }
    let live_identities = inner
        .kernel
        .body_geometry_identities()
        .map_err(|e| e.to_string())?;
    let identities = bodies
        .iter()
        .map(|id| {
            live_identities
                .get(id)
                .copied()
                .map(|identity| (*id, identity))
                .ok_or_else(|| "Drawing body geometry identity is unavailable".to_string())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let reused = previous
        .filter(|previous| {
            previous.owner == owner
                && previous
                    .scene
                    .upgrade()
                    .is_some_and(|old| Arc::ptr_eq(&old, &scene))
                && previous.identities == identities
                && previous.requests == requests
                && previous.projections.len() == requests.len()
        })
        .cloned();
    let kernel = if reused.is_none() {
        Some(
            inner
                .kernel
                .detached_drawing_snapshot(&bodies.into_iter().collect::<Vec<_>>())
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    Ok(DetachedDrawingSnapshot {
        kernel,
        scene,
        assembly,
        identities,
        requests,
        projections: Mutex::new(BTreeMap::new()),
        reused,
        owner,
    })
}

impl DetachedDrawingSnapshot {
    pub fn project(
        &self,
        view: &DrawingViewDto,
        views: &[DrawingViewDto],
    ) -> Result<ResolvedDrawingProjection, String> {
        let resolved = self.project_shared(view, views)?;
        Ok(ResolvedDrawingProjection {
            projection: (*resolved.projection).clone(),
            basis: resolved.basis,
        })
    }

    pub fn project_shared(
        &self,
        view: &DrawingViewDto,
        views: &[DrawingViewDto],
    ) -> Result<SharedDrawingProjection, String> {
        let request = limo_cad_occt::drawing_export::projection_request(
            view,
            views,
            &self.scene,
            &self.assembly,
        )?;
        let request = limo_cad_occt::resolve_drawing_request(&self.scene, &self.assembly, &request)
            .map_err(|e| e.to_string())?;
        if !self
            .requests
            .iter()
            .any(|(id, captured)| *id == view.id && *captured == request)
        {
            return Err("Drawing projection differs from its captured intent".into());
        }
        self.project_captured(view.id, &request)
    }

    fn project_captured(
        &self,
        id: u64,
        request: &DrawingProjectionRequest,
    ) -> Result<SharedDrawingProjection, String> {
        if let Some(reused) = &self.reused {
            return reused
                .projections
                .get(&id)
                .cloned()
                .ok_or_else(|| "Drawing projection bundle is incomplete".into());
        }
        if let Some(projected) = self
            .projections
            .lock()
            .map_err(|_| "Drawing projection cache poisoned")?
            .get(&id)
        {
            return Ok(projected.clone());
        }
        let basis = limo_cad_occt::drawing_projection_basis(request.direction, request.up)
            .map_err(|e| e.to_string())?;
        let projection = self
            .kernel
            .as_ref()
            .ok_or("Drawing projection kernel is unavailable")?
            .project(&self.scene, &self.assembly, request)
            .map_err(|e| e.to_string())?;
        let resolved = SharedDrawingProjection {
            projection: Arc::new(projection),
            basis,
        };
        self.projections
            .lock()
            .map_err(|_| "Drawing projection cache poisoned")?
            .insert(id, resolved.clone());
        Ok(resolved)
    }

    pub fn finished_bundle(&self) -> Result<Option<Arc<FinishedDrawingProjections>>, String> {
        if let Some(reused) = &self.reused {
            return Ok(Some(Arc::clone(reused)));
        }
        let projections = self
            .projections
            .lock()
            .map_err(|_| "Drawing projection cache poisoned")?;
        if projections.len() != self.requests.len() {
            return Ok(None);
        }
        Ok(Some(Arc::new(FinishedDrawingProjections {
            scene: Arc::downgrade(&self.scene),
            owner: self.owner.clone(),
            identities: self.identities.clone(),
            requests: self.requests.clone(),
            projections: projections.clone(),
        })))
    }

    pub fn source_graphics<'a>(
        &self,
        sheet: &DrawingSheetDto,
        projection: impl Fn(u64) -> Option<&'a limo_cad_occt::DrawingProjectionDto>,
        budget: &mut limo_cad_occt::drawing_export::PaperGraphicsBudget,
    ) -> Result<Vec<limo_cad_occt::drawing_export::PaperPrimitive>, String> {
        let mut graphics = Vec::new();
        for view in &sheet.views {
            if view.derivation.is_some() {
                let marks = limo_cad_occt::drawing_export::derived_source_graphics(
                    view,
                    sheet,
                    &projection,
                    &self.scene,
                    &self.assembly,
                    budget,
                )?;
                budget.append(&mut graphics, marks)?;
            }
        }
        Ok(graphics)
    }
}
