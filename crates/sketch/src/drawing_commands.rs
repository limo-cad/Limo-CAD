//! Small drawing edits shared by native, browser and MCP hosts.
use crate::session::SessionError;
use crate::{drawing::*, SketchManager};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSheet {
    pub name: String,
    pub format: DrawingSheetFormat,
    pub orientation: DrawingSheetOrientation,
    #[serde(default)]
    pub standard: DrawingStandard,
    #[serde(default)]
    pub projection_method: DrawingProjectionMethod,
    #[serde(default)]
    pub tolerance_note: DrawingToleranceNoteDto,
    #[serde(default)]
    pub title_block: DrawingTitleBlockDto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetTarget {
    pub sheet_id: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddView {
    pub sheet_id: u64,
    pub view: DrawingViewDto,
    #[serde(default)]
    pub rescale_group: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddNote {
    pub sheet_id: u64,
    pub text: String,
    pub position: [f64; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddLinearDimension {
    pub sheet_id: u64,
    pub view_id: u64,
    pub first: DrawingTopologyAnchorRefDto,
    pub second: DrawingTopologyAnchorRefDto,
    pub mode: DrawingLinearDimensionMode,
    pub offset: f64,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default = "dimension_precision")]
    pub precision: u8,
    #[serde(default)]
    pub presentation: DrawingDimensionPresentationDto,
}
fn dimension_precision() -> u8 {
    2
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRadialDimension {
    pub sheet_id: u64,
    pub view_id: u64,
    pub feature: DrawingCircularRefDto,
    pub mode: DrawingRadialDimensionMode,
    pub leader_angle_deg: f64,
    pub offset: f64,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default = "dimension_precision")]
    pub precision: u8,
    #[serde(default)]
    pub presentation: DrawingDimensionPresentationDto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddAngularDimension {
    pub sheet_id: u64,
    pub view_id: u64,
    pub vertex: DrawingTopologyAnchorRefDto,
    pub first: DrawingTopologyAnchorRefDto,
    pub second: DrawingTopologyAnchorRefDto,
    pub radius: f64,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default = "dimension_precision")]
    pub precision: u8,
    #[serde(default)]
    pub presentation: DrawingDimensionPresentationDto,
}
/// IDs are allocated by the document. Replacing a BOM with attached balloons
/// is rejected rather than silently leaving balloons referring to other parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetBom {
    pub sheet_id: u64,
    pub items: Vec<BomItem>,
    #[serde(default)]
    pub position: Option<[f64; 2]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BomItem {
    pub item_number: String,
    #[serde(default)]
    pub body_id: Option<limo_cad_core::BodyId>,
    pub part_number: String,
    pub description: String,
    pub quantity: f64,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub finish: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationEdit {
    pub sheet_id: u64,
    pub annotation: DrawingAnnotationDto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationTarget {
    pub sheet_id: u64,
    pub annotation_id: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewTarget {
    pub sheet_id: u64,
    pub view_id: u64,
    /// Explicitly remove dependent views and their attached annotations.
    #[serde(default)]
    pub cascade: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTemplate {
    pub sheet_id: u64,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyTemplate {
    pub sheet_id: u64,
    pub template_id: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateTarget {
    pub template_id: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRevision {
    pub sheet_id: u64,
    pub revision: DrawingRevisionDto,
    #[serde(default)]
    pub position: Option<[f64; 2]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetRelease {
    pub sheet_id: u64,
    pub release: DrawingReleaseDto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "arguments", rename_all = "snake_case")]
pub enum DrawingCommand {
    CreateSheet(CreateSheet),
    SelectSheet(SheetTarget),
    DeleteSheet(SheetTarget),
    AddView(AddView),
    UpdateView(AddView),
    DeleteView(ViewTarget),
    AddAnnotation(AnnotationEdit),
    UpdateAnnotation(AnnotationEdit),
    DeleteAnnotation(AnnotationTarget),
    CreateTemplate(CreateTemplate),
    ApplyTemplate(ApplyTemplate),
    DeleteTemplate(TemplateTarget),
    AddRevision(AddRevision),
    SetRelease(SetRelease),
    AddNote(AddNote),
    AddLinearDimension(AddLinearDimension),
    AddRadialDimension(AddRadialDimension),
    AddAngularDimension(AddAngularDimension),
    SetBom(SetBom),
}
impl SketchManager {
    pub fn drawing_command(
        &mut self,
        command: DrawingCommand,
    ) -> Result<DrawingDocumentDto, SessionError> {
        if self.has_active_sketch() {
            return Err(SessionError::Solid(
                "Finish the active sketch before editing drawings.".into(),
            ));
        }
        let before = self.drawing_document_ref();
        let assembly = self.assembly_document_ref();
        let solution_cache = &std::cell::OnceCell::new();
        let mut next = before.clone();
        let updating_view = matches!(&command, DrawingCommand::UpdateView(_));
        let require_signature = !matches!(
            &command,
            DrawingCommand::AddLinearDimension(_)
                | DrawingCommand::AddRadialDimension(_)
                | DrawingCommand::AddAngularDimension(_)
        );
        let explicit_release = match &command {
            DrawingCommand::SetRelease(r) => Some(r.sheet_id),
            DrawingCommand::AddRevision(r) => Some(r.sheet_id),
            _ => None,
        };
        match command {
            DrawingCommand::CreateSheet(r) => {
                let sheet = DrawingSheetDto {
                    id: next.next_sheet_id,
                    name: r.name,
                    format: r.format,
                    orientation: r.orientation,
                    standard: r.standard,
                    projection_method: r.projection_method,
                    tolerance_note: r.tolerance_note,
                    title_block: r.title_block,
                    views: Vec::new(),
                    annotations: Vec::new(),
                    style: DrawingSheetStyleDto::default(),
                    template_name: "Limo CAD Default".into(),
                    revisions: Vec::new(),
                    bom: Vec::new(),
                    release: DrawingReleaseDto::default(),
                    revision_table_position: None,
                    bom_table_position: None,
                };
                next.active_sheet_id = Some(sheet.id);
                next.next_sheet_id = next
                    .next_sheet_id
                    .checked_add(1)
                    .ok_or_else(|| SessionError::Solid("Sheet IDs exhausted".into()))?;
                next.sheets.push(sheet);
            }
            DrawingCommand::SelectSheet(r) => {
                sheet(&mut next, r.sheet_id)?;
                next.active_sheet_id = Some(r.sheet_id);
            }
            DrawingCommand::DeleteSheet(r) => {
                sheet(&mut next, r.sheet_id)?;
                next.sheets.retain(|s| s.id != r.sheet_id);
                if next.active_sheet_id == Some(r.sheet_id) {
                    next.active_sheet_id = next.sheets.first().map(|s| s.id);
                }
            }
            DrawingCommand::AddView(mut r) | DrawingCommand::UpdateView(mut r) => {
                let scene = self.solid_scene_ref();
                if !scene.errors.is_empty() {
                    return Err(SessionError::Solid(
                        "Resolve timeline errors before adding a drawing view.".into(),
                    ));
                }
                if scene.bodies.is_empty() {
                    return Err(SessionError::Solid(
                        "Create a solid body before adding a drawing view.".into(),
                    ));
                }
                if r.view
                    .body_ids
                    .iter()
                    .any(|id| !scene.bodies.iter().any(|b| b.id == *id))
                {
                    return Err(SessionError::Solid(
                        "Drawing view references a missing body.".into(),
                    ));
                }
                if let Some(derivation) = &r.view.derivation {
                    match derivation {
                        DrawingViewDerivationDto::Section { first, second, .. }
                        | DrawingViewDerivationDto::RemovedSection { first, second, .. } => {
                            for a in [first, second] {
                                validate_instance(
                                    assembly,
                                    scene,
                                    solution_cache,
                                    &r.view,
                                    a.occurrence_id,
                                    a.body_id,
                                )?;
                                validate_edge(
                                    scene,
                                    &r.view,
                                    a.body_id,
                                    a.edge_id,
                                    &a.edge_key,
                                    a.circle_center,
                                )?;
                            }
                        }
                        DrawingViewDerivationDto::Detail { center, .. } => {
                            validate_instance(
                                assembly,
                                scene,
                                solution_cache,
                                &r.view,
                                center.occurrence_id,
                                center.body_id,
                            )?;
                            validate_edge(
                                scene,
                                &r.view,
                                center.body_id,
                                center.edge_id,
                                &center.edge_key,
                                center.circle_center,
                            )?;
                        }
                        DrawingViewDerivationDto::Auxiliary { reference, .. } => {
                            validate_instance(
                                assembly,
                                scene,
                                solution_cache,
                                &r.view,
                                reference.occurrence_id,
                                reference.body_id,
                            )?;
                            validate_edge(
                                scene,
                                &r.view,
                                reference.body_id,
                                reference.edge_id,
                                &reference.edge_key,
                                false,
                            )?;
                        }
                        DrawingViewDerivationDto::Broken { .. } => {}
                    }
                }
                validate_view_selection(assembly, scene, solution_cache, &r.view)?;
                r.view.visit_references(&mut |reference| {
                    let (body, _, _, _, signature, _) = reference.identity();
                    if signature.is_some() {
                        crate::drawing_topology::validate_drawing_reference_topology(
                            scene, body, signature,
                        )
                        .map_err(SessionError::Solid)?;
                    }
                    Ok::<_, SessionError>(())
                })?;
                if !updating_view {
                    r.view.id = next.next_view_id;
                    next.next_view_id = next
                        .next_view_id
                        .checked_add(1)
                        .ok_or_else(|| SessionError::Solid("View IDs exhausted".into()))?;
                }
                let target = sheet(&mut next, r.sheet_id)?;
                if r.rescale_group && !updating_view {
                    if let Some(parent) = r.view.parent_view_id {
                        let root = view_root(&target.views, parent);
                        let ids: Vec<u64> = target
                            .views
                            .iter()
                            .filter(|v| view_root(&target.views, v.id) == root)
                            .map(|v| v.id)
                            .collect();
                        for view in &mut target.views {
                            if ids.contains(&view.id) {
                                view.scale = r.view.scale;
                            }
                        }
                    }
                }
                if updating_view {
                    crate::update_drawing_view(target, r.view, r.rescale_group)
                        .map_err(SessionError::Solid)?;
                } else {
                    target.views.push(r.view);
                }
                if updating_view {
                    validate_sheet_associations(target, assembly, scene, solution_cache)?;
                }
            }
            DrawingCommand::DeleteView(r) => {
                let target = sheet(&mut next, r.sheet_id)?;
                dimension_view(target, r.view_id)?;
                let mut removed = std::collections::HashSet::from([r.view_id]);
                loop {
                    let count = removed.len();
                    for view in &target.views {
                        if view.parent_view_id.is_some_and(|id| removed.contains(&id))
                            || view
                                .derivation
                                .as_ref()
                                .is_some_and(|d| removed.contains(&derivation_parent(d)))
                        {
                            removed.insert(view.id);
                        }
                    }
                    if count == removed.len() {
                        break;
                    }
                }
                if !r.cascade
                    && (removed.len() > 1
                        || target
                            .annotations
                            .iter()
                            .any(|a| a.view_id().is_some_and(|id| removed.contains(&id))))
                {
                    return Err(SessionError::Solid("View has dependent views or annotations; explicitly request cascade to delete them.".into()));
                }
                target.views.retain(|v| !removed.contains(&v.id));
                target
                    .annotations
                    .retain(|a| a.view_id().is_none_or(|id| !removed.contains(&id)));
            }
            DrawingCommand::AddAnnotation(mut r) => {
                r.annotation.set_id(annotation_id(&mut next)?);
                sheet(&mut next, r.sheet_id)?.annotations.push(r.annotation);
            }
            DrawingCommand::UpdateAnnotation(r) => {
                let target = sheet(&mut next, r.sheet_id)?;
                let current = target
                    .annotations
                    .iter_mut()
                    .find(|a| a.id() == r.annotation.id())
                    .ok_or_else(|| {
                        SessionError::Solid("Drawing annotation does not exist".into())
                    })?;
                *current = r.annotation;
            }
            DrawingCommand::DeleteAnnotation(r) => {
                let target = sheet(&mut next, r.sheet_id)?;
                if !target.annotations.iter().any(|a| a.id() == r.annotation_id) {
                    return Err(SessionError::Solid(
                        "Drawing annotation does not exist".into(),
                    ));
                }
                target.annotations.retain(|a| a.id() != r.annotation_id);
            }
            DrawingCommand::CreateTemplate(r) => {
                if next.templates.iter().any(|t| t.name == r.name) {
                    return Err(SessionError::Solid(
                        "A drawing template with that name already exists".into(),
                    ));
                }
                let id = next.next_template_id;
                let source = sheet(&mut next, r.sheet_id)?;
                let template = DrawingTemplateDto {
                    id,
                    name: r.name,
                    standard: source.standard,
                    projection_method: source.projection_method,
                    tolerance_note: source.tolerance_note.clone(),
                    title_defaults: source.title_block.clone(),
                    style: source.style.clone(),
                };
                next.next_template_id = next
                    .next_template_id
                    .checked_add(1)
                    .ok_or_else(|| SessionError::Solid("Template IDs exhausted".into()))?;
                next.templates.push(template);
            }
            DrawingCommand::ApplyTemplate(r) => {
                let template = next
                    .templates
                    .iter()
                    .find(|t| t.id == r.template_id)
                    .cloned()
                    .ok_or_else(|| SessionError::Solid("Drawing template does not exist".into()))?;
                let target = sheet(&mut next, r.sheet_id)?;
                target.template_name = template.name;
                target.standard = template.standard;
                target.projection_method = template.projection_method;
                target.tolerance_note = template.tolerance_note;
                target.title_block = template.title_defaults;
                target.style = template.style;
            }
            DrawingCommand::DeleteTemplate(r) => {
                if !next.templates.iter().any(|t| t.id == r.template_id) {
                    return Err(SessionError::Solid(
                        "Drawing template does not exist".into(),
                    ));
                }
                next.templates.retain(|t| t.id != r.template_id);
            }
            DrawingCommand::AddRevision(mut r) => {
                r.revision.id = next.next_revision_id;
                next.next_revision_id = next
                    .next_revision_id
                    .checked_add(1)
                    .ok_or_else(|| SessionError::Solid("Revision IDs exhausted".into()))?;
                let target = sheet(&mut next, r.sheet_id)?;
                if target
                    .revisions
                    .iter()
                    .any(|entry| entry.revision == r.revision.revision)
                {
                    return Err(SessionError::Solid(
                        "Revision code already exists on this sheet".into(),
                    ));
                }
                target.title_block.revision = r.revision.revision.clone();
                if r.revision.status == DrawingReleaseStatus::Released {
                    target.release = DrawingReleaseDto {
                        status: r.revision.status,
                        released_revision: r.revision.revision.clone(),
                        released_at: r.revision.date.clone(),
                    };
                } else {
                    target.release.status = r.revision.status;
                }
                target.revisions.push(r.revision);
                if let Some(position) = r.position {
                    target.revision_table_position = Some(position);
                }
                validate_release(target, assembly, self.solid_scene_ref(), solution_cache)?;
            }
            DrawingCommand::SetRelease(r) => {
                let target = sheet(&mut next, r.sheet_id)?;
                target.release = r.release;
                validate_release(target, assembly, self.solid_scene_ref(), solution_cache)?;
            }
            DrawingCommand::AddLinearDimension(r) => {
                let target = sheet(&mut next, r.sheet_id)?;
                let view = target
                    .views
                    .iter()
                    .find(|v| v.id == r.view_id)
                    .ok_or_else(|| {
                        SessionError::Solid("Drawing dimension references a missing view.".into())
                    })?;
                let scene = self.solid_scene_ref();
                if !scene.errors.is_empty() {
                    return Err(SessionError::Solid(
                        "Resolve timeline errors before dimensioning.".into(),
                    ));
                }
                for anchor in [&r.first, &r.second] {
                    validate_edge(
                        scene,
                        view,
                        anchor.body_id,
                        anchor.edge_id,
                        &anchor.edge_key,
                        anchor.circle_center,
                    )?;
                    validate_instance(
                        assembly,
                        scene,
                        solution_cache,
                        view,
                        anchor.occurrence_id,
                        anchor.body_id,
                    )?;
                }
                if r.first.occurrence_id == r.second.occurrence_id
                    && r.first.body_id == r.second.body_id
                    && r.first.edge_id == r.second.edge_id
                    && r.first.edge_key == r.second.edge_key
                    && r.first.circle_center == r.second.circle_center
                    && (r.first.circle_center || r.first.endpoint == r.second.endpoint)
                {
                    return Err(SessionError::Solid(
                        "Dimension needs two distinct topology anchors.".into(),
                    ));
                }
                let id = next.next_annotation_id;
                next.next_annotation_id = id
                    .checked_add(1)
                    .ok_or_else(|| SessionError::Solid("Annotation IDs exhausted".into()))?;
                sheet(&mut next, r.sheet_id)?.annotations.push(
                    DrawingAnnotationDto::LinearDimension {
                        id,
                        view_id: r.view_id,
                        first: r.first,
                        second: r.second,
                        mode: r.mode,
                        offset: r.offset,
                        prefix: r.prefix,
                        suffix: r.suffix,
                        precision: r.precision,
                        presentation: r.presentation,
                    },
                );
            }
            DrawingCommand::AddRadialDimension(r) => {
                let target = sheet(&mut next, r.sheet_id)?;
                let view = dimension_view(target, r.view_id)?;
                let scene = self.solid_scene_ref();
                validate_edge(
                    scene,
                    view,
                    r.feature.body_id,
                    r.feature.edge_id,
                    &r.feature.edge_key,
                    true,
                )?;
                validate_instance(
                    assembly,
                    scene,
                    solution_cache,
                    view,
                    r.feature.occurrence_id,
                    r.feature.body_id,
                )?;
                let id = annotation_id(&mut next)?;
                sheet(&mut next, r.sheet_id)?.annotations.push(
                    DrawingAnnotationDto::RadialDimension {
                        id,
                        view_id: r.view_id,
                        feature: r.feature,
                        mode: r.mode,
                        leader_angle_deg: r.leader_angle_deg,
                        offset: r.offset,
                        prefix: r.prefix,
                        suffix: r.suffix,
                        precision: r.precision,
                        presentation: r.presentation,
                    },
                );
            }
            DrawingCommand::AddAngularDimension(r) => {
                let view = dimension_view(sheet(&mut next, r.sheet_id)?, r.view_id)?;
                let scene = self.solid_scene_ref();
                for a in [&r.vertex, &r.first, &r.second] {
                    validate_instance(
                        assembly,
                        scene,
                        solution_cache,
                        view,
                        a.occurrence_id,
                        a.body_id,
                    )?;
                    validate_edge(
                        scene,
                        view,
                        a.body_id,
                        a.edge_id,
                        &a.edge_key,
                        a.circle_center,
                    )?;
                }
                let id = annotation_id(&mut next)?;
                sheet(&mut next, r.sheet_id)?.annotations.push(
                    DrawingAnnotationDto::AngularDimension {
                        id,
                        view_id: r.view_id,
                        vertex: r.vertex,
                        first: r.first,
                        second: r.second,
                        radius: r.radius,
                        prefix: r.prefix,
                        suffix: r.suffix,
                        precision: r.precision,
                        presentation: r.presentation,
                    },
                );
            }
            DrawingCommand::SetBom(r) => {
                let target = sheet(&mut next, r.sheet_id)?;
                if target
                    .annotations
                    .iter()
                    .any(|a| matches!(a, DrawingAnnotationDto::ItemBalloon { .. }))
                {
                    return Err(SessionError::Solid(
                        "Remove attached balloons before replacing the bill of materials.".into(),
                    ));
                }
                let scene = self.solid_scene_ref();
                let mut items = Vec::with_capacity(r.items.len());
                for item in r.items {
                    if item
                        .body_id
                        .is_some_and(|id| !scene.bodies.iter().any(|b| b.id == id))
                    {
                        return Err(SessionError::Solid(
                            "BOM item references a missing body.".into(),
                        ));
                    }
                    let id = next.next_bom_item_id;
                    next.next_bom_item_id = id
                        .checked_add(1)
                        .ok_or_else(|| SessionError::Solid("BOM IDs exhausted".into()))?;
                    items.push(DrawingBomItemDto {
                        id,
                        item_number: item.item_number,
                        body_id: item.body_id,
                        part_number: item.part_number,
                        description: item.description,
                        quantity: item.quantity,
                        material: item.material,
                        finish: item.finish,
                    });
                }
                let target = sheet(&mut next, r.sheet_id)?;
                target.bom = items;
                if let Some(position) = r.position {
                    target.bom_table_position = Some(position);
                }
            }
            DrawingCommand::AddNote(r) => {
                let id = next.next_annotation_id;
                next.next_annotation_id = next
                    .next_annotation_id
                    .checked_add(1)
                    .ok_or_else(|| SessionError::Solid("Annotation IDs exhausted".into()))?;
                sheet(&mut next, r.sheet_id)?
                    .annotations
                    .push(DrawingAnnotationDto::Note {
                        id,
                        text: r.text,
                        position: r.position,
                    });
            }
        }

        for target in &next.sheets {
            for annotation in &target.annotations {
                if before
                    .sheets
                    .iter()
                    .find(|s| s.id == target.id)
                    .and_then(|s| s.annotations.iter().find(|a| a.id() == annotation.id()))
                    != Some(annotation)
                {
                    validate_annotation(
                        annotation,
                        target,
                        assembly,
                        self.solid_scene_ref(),
                        solution_cache,
                        require_signature,
                    )?;
                }
            }
        }
        for prior in &before.sheets {
            if let Some(current) = next.sheets.iter_mut().find(|s| s.id == prior.id) {
                if current != prior
                    && prior.release.status == DrawingReleaseStatus::Released
                    && explicit_release != Some(prior.id)
                {
                    current.release.status = DrawingReleaseStatus::Draft;
                }
            }
        }
        self.set_drawing_document(next)
    }
}
fn derivation_parent(derivation: &DrawingViewDerivationDto) -> u64 {
    match derivation {
        DrawingViewDerivationDto::Section { parent_view_id, .. }
        | DrawingViewDerivationDto::RemovedSection { parent_view_id, .. }
        | DrawingViewDerivationDto::Detail { parent_view_id, .. }
        | DrawingViewDerivationDto::Auxiliary { parent_view_id, .. }
        | DrawingViewDerivationDto::Broken { parent_view_id, .. } => *parent_view_id,
    }
}

fn validate_release(
    sheet: &DrawingSheetDto,
    assembly: &limo_cad_assembly::AssemblyDocumentDto,
    scene: &limo_cad_solid::SolidSceneDto,
    solution_cache: &std::cell::OnceCell<limo_cad_assembly::AssemblySolutionDto>,
) -> Result<(), SessionError> {
    if sheet.release.status != DrawingReleaseStatus::Released {
        return Ok(());
    }
    if sheet.release.released_at.trim().is_empty()
        || sheet.release.released_revision.trim().is_empty()
        || sheet.release.released_revision != sheet.title_block.revision
        || !sheet.revisions.iter().any(|revision| {
            revision.revision == sheet.release.released_revision
                && revision.status == DrawingReleaseStatus::Released
                && revision.date == sheet.release.released_at
        })
    {
        return Err(SessionError::Solid(
            "Release requires a matching issued revision and date".into(),
        ));
    }
    if !scene.errors.is_empty() {
        return Err(SessionError::Solid(
            "Resolve timeline errors before releasing a drawing".into(),
        ));
    }
    for view in &sheet.views {
        validate_view_selection(assembly, scene, solution_cache, view)?;
    }
    validate_sheet_associations(sheet, assembly, scene, solution_cache)?;
    crate::drawing_topology::validate_drawing_sheet_topology(sheet, scene)
        .map_err(SessionError::Solid)
}

fn validate_sheet_associations(
    sheet: &DrawingSheetDto,
    assembly: &limo_cad_assembly::AssemblyDocumentDto,
    scene: &limo_cad_solid::SolidSceneDto,
    solution_cache: &std::cell::OnceCell<limo_cad_assembly::AssemblySolutionDto>,
) -> Result<(), SessionError> {
    for annotation in &sheet.annotations {
        validate_annotation(annotation, sheet, assembly, scene, solution_cache, true)?;
    }
    Ok(())
}

fn validate_annotation(
    annotation: &DrawingAnnotationDto,
    sheet: &DrawingSheetDto,
    assembly: &limo_cad_assembly::AssemblyDocumentDto,
    scene: &limo_cad_solid::SolidSceneDto,
    solution_cache: &std::cell::OnceCell<limo_cad_assembly::AssemblySolutionDto>,
    require_signature: bool,
) -> Result<(), SessionError> {
    let Some(view_id) = annotation.view_id() else {
        return Ok(());
    };
    let view = dimension_view(sheet, view_id)?;
    annotation.visit_references(&mut |reference| {
        let (body, edge, key, occurrence, signature, circle) = reference.identity();
        if require_signature || signature.is_some() {
            crate::drawing_topology::validate_drawing_reference_topology(scene, body, signature)
                .map_err(SessionError::Solid)?;
        }
        validate_edge(scene, view, body, edge, key, circle)?;
        validate_instance(assembly, scene, solution_cache, view, occurrence, body)
    })
}
fn annotation_id(doc: &mut DrawingDocumentDto) -> Result<u64, SessionError> {
    let id = doc.next_annotation_id;
    doc.next_annotation_id = id
        .checked_add(1)
        .ok_or_else(|| SessionError::Solid("Annotation IDs exhausted".into()))?;
    Ok(id)
}
fn validate_view_selection(
    assembly: &limo_cad_assembly::AssemblyDocumentDto,
    scene: &limo_cad_solid::SolidSceneDto,
    solution_cache: &std::cell::OnceCell<limo_cad_assembly::AssemblySolutionDto>,
    view: &DrawingViewDto,
) -> Result<(), SessionError> {
    if view
        .body_ids
        .iter()
        .any(|id| !scene.bodies.iter().any(|body| body.id == *id))
    {
        return Err(SessionError::Solid(
            "Drawing view references a missing body.".into(),
        ));
    }
    if view.scope == DrawingViewScope::Definition {
        if !view.occurrence_ids.is_empty() {
            return Err(SessionError::Solid(
                "Definition drawing views cannot select occurrences.".into(),
            ));
        }
        return Ok(());
    }
    let solution = solution_cache.get_or_init(|| assembly.solve(scene));
    if !solution.solved {
        return Err(SessionError::Solid(
            "Resolve assembly diagnostics before accepting an assembly drawing.".into(),
        ));
    }
    if view.occurrence_ids.iter().any(|id| {
        !assembly
            .component_structure
            .occurrences
            .iter()
            .any(|o| o.id == *id)
    }) {
        return Err(SessionError::Solid(
            "Drawing view selects a missing occurrence.".into(),
        ));
    }
    if !solution.instance_body_poses.iter().any(|pose| {
        pose.visible
            && (view.body_ids.is_empty() || view.body_ids.contains(&pose.body_id))
            && validate_instance(
                assembly,
                scene,
                solution_cache,
                view,
                Some(pose.occurrence_id),
                pose.body_id,
            )
            .is_ok()
    }) {
        return Err(SessionError::Solid(
            "Assembly drawing view must select at least one visible body occurrence.".into(),
        ));
    }
    Ok(())
}
fn validate_instance(
    assembly: &limo_cad_assembly::AssemblyDocumentDto,
    scene: &limo_cad_solid::SolidSceneDto,
    solution_cache: &std::cell::OnceCell<limo_cad_assembly::AssemblySolutionDto>,
    view: &DrawingViewDto,
    occurrence: Option<limo_cad_assembly::OccurrenceId>,
    body: limo_cad_core::BodyId,
) -> Result<(), SessionError> {
    match (view.scope, occurrence) {
        (DrawingViewScope::Definition, None) => Ok(()),
        (DrawingViewScope::Assembly, Some(id)) => {
            let solution = solution_cache.get_or_init(|| assembly.solve(scene));
            if !solution.solved
                || !solution
                    .instance_body_poses
                    .iter()
                    .any(|p| p.occurrence_id == id && p.body_id == body && p.visible)
            {
                return Err(SessionError::Solid(
                    "Drawing anchor references a missing, hidden or unsolved occurrence.".into(),
                ));
            }
            let mut current = Some(id);
            for _ in 0..=assembly.component_structure.occurrences.len() {
                let Some(id) = current else {
                    break;
                };
                if view.occurrence_ids.is_empty() || view.occurrence_ids.contains(&id) {
                    return Ok(());
                }
                current = assembly
                    .component_structure
                    .occurrences
                    .iter()
                    .find(|o| o.id == id)
                    .and_then(|o| o.parent_occurrence_id);
            }
            Err(SessionError::Solid(
                "Drawing anchor occurrence is excluded from this view.".into(),
            ))
        }
        _ => Err(SessionError::Solid(
            "Dimension occurrence identity must match the drawing view scope.".into(),
        )),
    }
}
fn dimension_view(sheet: &DrawingSheetDto, id: u64) -> Result<&DrawingViewDto, SessionError> {
    sheet
        .views
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| SessionError::Solid("Drawing dimension references a missing view.".into()))
}
fn validate_edge(
    scene: &limo_cad_solid::SolidSceneDto,
    view: &DrawingViewDto,
    body: limo_cad_core::BodyId,
    edge: limo_cad_core::EdgeId,
    key: &str,
    circle: bool,
) -> Result<(), SessionError> {
    if !scene.errors.is_empty() {
        return Err(SessionError::Solid(
            "Resolve timeline errors before dimensioning.".into(),
        ));
    }
    let found = scene
        .bodies
        .iter()
        .find(|b| b.id == body)
        .and_then(|b| b.edges.iter().find(|e| e.id == edge && e.key == key));
    if found.is_none_or(|e| circle && e.circle.is_none())
        || (!view.body_ids.is_empty() && !view.body_ids.contains(&body))
    {
        return Err(SessionError::Solid(
            "Dimension anchor is missing, stale, or excluded from the view.".into(),
        ));
    }
    Ok(())
}
fn sheet(d: &mut DrawingDocumentDto, id: u64) -> Result<&mut DrawingSheetDto, SessionError> {
    d.sheets
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| SessionError::Solid(format!("Drawing sheet {id} does not exist")))
}

fn view_root(views: &[DrawingViewDto], mut id: u64) -> u64 {
    for _ in 0..views.len() {
        match views
            .iter()
            .find(|v| v.id == id)
            .and_then(|v| v.parent_view_id)
        {
            Some(parent) => id = parent,
            None => break,
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create(manager: &mut SketchManager) -> DrawingDocumentDto {
        manager.drawing_command(serde_json::from_value(serde_json::json!({"type":"create_sheet","arguments":{"name":"Fabrication","format":"a4","orientation":"landscape"}})).unwrap()).unwrap()
    }
    #[test]
    fn drawing_edits_are_atomic_and_failed_edits_do_not_consume_ids() {
        let mut manager = SketchManager::new();
        let first = create(&mut manager);
        assert!(manager
            .drawing_command(DrawingCommand::DeleteSheet(SheetTarget { sheet_id: 99 }))
            .is_err());
        assert_eq!(manager.drawing_document(), first);
        assert!(manager
            .drawing_command(DrawingCommand::AddNote(AddNote {
                sheet_id: 1,
                text: "x".repeat(4097),
                position: [10.0, 20.0]
            }))
            .is_err());
        assert_eq!(manager.drawing_document(), first);
        let next = manager
            .drawing_command(DrawingCommand::AddNote(AddNote {
                sheet_id: 1,
                text: "Deburr edges".into(),
                position: [10.0, 20.0],
            }))
            .unwrap();
        assert_eq!(next.next_annotation_id, 2);
        assert_eq!(next.sheets[0].annotations.len(), 1);
        let second = create(&mut manager);
        assert_eq!(second.active_sheet_id, Some(2));
        let deleted = manager
            .drawing_command(DrawingCommand::DeleteSheet(SheetTarget { sheet_id: 2 }))
            .unwrap();
        assert_eq!(deleted.active_sheet_id, Some(1));
    }
    #[test]
    fn content_edit_revokes_release_but_sheet_selection_does_not() {
        let mut manager = SketchManager::new();
        let mut doc = create(&mut manager);
        doc.sheets[0].release.status = DrawingReleaseStatus::Released;
        doc.sheets[0].release.released_revision = "A".into();
        doc.sheets[0].release.released_at = "2026-09-09".into();
        manager.set_drawing_document(doc).unwrap();
        let selected = manager
            .drawing_command(DrawingCommand::SelectSheet(SheetTarget { sheet_id: 1 }))
            .unwrap();
        assert_eq!(
            selected.sheets[0].release.status,
            DrawingReleaseStatus::Released
        );
        let edited = manager
            .drawing_command(DrawingCommand::AddNote(AddNote {
                sheet_id: 1,
                text: "New note".into(),
                position: [10.0, 20.0],
            }))
            .unwrap();
        assert_eq!(edited.sheets[0].release.status, DrawingReleaseStatus::Draft);
    }
}
