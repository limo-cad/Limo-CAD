//! Cold paper jobs own only selected exact definitions. Copying geometry costs
//! time/memory at capture, but HLR never retains or locks the live workspace.
//! A fresh snapshot does not inherit the live kernel's HLR cache; exact source
//! revision fences conservatively favor correctness over cross-revision reuse.
use super::*;
use limo_cad_sketch::{AssemblyDocumentDto, DrawingDocumentDto, DrawingSheetDto, DrawingViewDto};

pub struct DetachedDrawingSnapshot {
    kernel: limo_cad_occt::DetachedDrawingKernel,
    scene: Arc<SolidSceneDto>,
    assembly: AssemblyDocumentDto,
}

impl NativeEngineHost {
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
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
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
            bodies.extend(
                limo_cad_occt::drawing_required_body_ids(&scene, &assembly, &request)
                    .map_err(|e| e.to_string())?,
            );
        }
        let kernel = inner
            .kernel
            .detached_drawing_snapshot(&bodies.into_iter().collect::<Vec<_>>())
            .map_err(|e| e.to_string())?;
        Ok(DetachedDrawingSnapshot {
            kernel,
            scene,
            assembly,
        })
    }
}

impl DetachedDrawingSnapshot {
    pub fn project(
        &self,
        view: &DrawingViewDto,
        views: &[DrawingViewDto],
    ) -> Result<ResolvedDrawingProjection, String> {
        let request = limo_cad_occt::drawing_export::projection_request(
            view,
            views,
            &self.scene,
            &self.assembly,
        )?;
        let basis = limo_cad_occt::drawing_projection_basis(request.direction, request.up)
            .map_err(|e| e.to_string())?;
        let projection = self
            .kernel
            .project(&self.scene, &self.assembly, &request)
            .map_err(|e| e.to_string())?;
        Ok(ResolvedDrawingProjection { projection, basis })
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
