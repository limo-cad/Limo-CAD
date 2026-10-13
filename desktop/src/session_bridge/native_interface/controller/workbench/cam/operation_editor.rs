//! Existing-operation forms write one shared operation and its keyed intent.
use super::*;
use limo_cad_sketch::SketchDto;
use limo_cad_solid::SolidSceneDto;

pub(super) mod heights;
mod linking;
pub(super) mod linking_points;
mod parameters;

pub(super) struct Context {
    heights: heights::Context,
    linking: Value,
    linking_points: linking_points::Context,
    geometry: Option<operation_geometry::Context>,
}

pub(super) fn geometry(draft: &Draft) -> Option<&operation_geometry::Context> {
    draft.operation_edit.as_ref()?.geometry.as_ref()
}

#[cfg(test)]
pub(super) fn extend(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
) -> Result<(), String> {
    extend_shared(draft, cam, &Arc::new(scene.clone()), sketches)
}

pub(super) fn extend_shared(
    draft: &mut Draft,
    cam: &CamDocumentDto,
    scene: &Arc<SolidSceneDto>,
    sketches: &[SketchDto],
) -> Result<(), String> {
    let Selection::Operation(id) = draft.selection else {
        return Ok(());
    };
    let setup = cam
        .setups
        .iter()
        .find(|setup| setup.operations.iter().any(|op| op.id() == id))
        .ok_or("Toolpath was removed")?;
    let operation = setup.operations.iter().find(|op| op.id() == id).unwrap();
    let geometry = operation_geometry::supports(&draft.record)
        .then(|| operation_geometry::Context::new(setup, scene, sketches));
    let source = Arc::new(heights::picking::Source {
        setup: setup.clone(),
        scene: Arc::clone(scene),
        sketches: geometry
            .as_ref()
            .map_or_else(|| Arc::from(sketches), |geometry| geometry.sketches.clone()),
    });
    let context = Context {
        heights: heights::Context::new(setup, operation, scene, sketches)
            .with_geometry(cam, setup, operation.id(), scene, sketches)
            .with_picker(source),
        linking: linking::initial(cam, operation)?,
        linking_points: linking_points::Context::new(setup, operation, scene),
        geometry,
    };
    let index = draft.fields.len();
    let supports_linking = matches!(
        operation,
        CamOperationDto::Face { .. }
            | CamOperationDto::Contour2d { .. }
            | CamOperationDto::Chamfer2d { .. }
            | CamOperationDto::Adaptive3d { .. }
    );
    let mut sections = form::options(&[("parameters", "Parameters"), ("heights", "Heights")]);
    if context.geometry.is_some() {
        sections.extend(form::options(&[("geometry", "Geometry")]));
    }
    if supports_linking {
        sections.extend(form::options(&[("linking", "Linking")]));
    }
    form::push(
        draft,
        "/native/ui/operation_section",
        "Operation section",
        InputKind::Choice,
        json!("parameters"),
        cam.units,
        Some(sections),
    );
    let section = draft.fields.remove(index);
    draft.fields.insert(0, section);
    parameters::extend(draft, cam)?;
    heights::extend(draft, cam, &context.heights)?;
    if supports_linking {
        linking::extend(draft, cam, &context.linking)?;
        linking_points::extend(draft, cam, &context.linking, &context.linking_points);
    }
    if let Some(geometry) = context.geometry.as_ref() {
        operation_geometry::extend(draft, cam, geometry)?;
    }
    draft.operation_edit = Some(context);
    Ok(())
}

pub(super) fn visible(draft: &Draft, path: &str) -> bool {
    if draft.operation_edit.is_none() {
        return true;
    }
    if path == "/native/ui/operation_section" {
        return true;
    }
    match form::text(draft, "/native/ui/operation_section").unwrap_or("parameters") {
        "heights" => heights::handles(path) && heights::visible(draft, path),
        "linking" => {
            (path.starts_with("/native/linking/") || linking_points::handles(path))
                && linking::visible(draft, path)
        }
        "geometry" => {
            !heights::handles(path)
                && !linking_points::handles(path)
                && operation_geometry::visible(draft, path)
        }
        _ => {
            !heights::handles(path)
                && !path.starts_with("/native/linking/")
                && !path.starts_with("/native/geometry/")
                && !path.starts_with("/native/ui/geometry_")
                && parameters::visible(draft, path)
        }
    }
}

pub(super) fn changed(draft: &mut Draft, cam: &CamDocumentDto, path: &str) -> Result<(), String> {
    let linking_point = linking_points::handles(path);
    if !linking_point
        && !path.starts_with("/native/geometry/")
        && !path.starts_with("/native/ui/geometry_")
    {
        return Ok(());
    }
    let Some(mut context) = draft.operation_edit.take() else {
        return Ok(());
    };
    let result = (|| {
        if linking_point {
            return linking_points::changed(
                draft,
                cam,
                path,
                &context.linking,
                &mut context.linking_points,
            );
        }
        let Some(geometry) = context.geometry.as_ref() else {
            return Ok(());
        };
        operation_geometry::changed(draft, cam, path, geometry)?;
        let mut record = draft.record.clone();
        if operation_geometry::apply(draft, &mut record, cam.units, geometry).is_ok() {
            if let Ok(operation) = serde_json::from_value::<CamOperationDto>(record) {
                let heights = context
                    .heights
                    .with_resolved_holes(&operation)
                    .unwrap_or_else(|| {
                        heights::Context::new(
                            &geometry.setup,
                            &operation,
                            &geometry.scene,
                            &geometry.sketches,
                        )
                        .with_geometry(
                            cam,
                            &geometry.setup,
                            operation.id(),
                            &geometry.scene,
                            &geometry.sketches,
                        )
                    });
                let mut saved = HashMap::new();
                draft.fields.retain(|field| {
                    if heights::handles(&field.path) {
                        saved.insert(
                            field.path.clone(),
                            (field.original.clone(), field.text.clone()),
                        );
                        false
                    } else {
                        true
                    }
                });
                let heights = heights.with_picks_from(&context.heights);
                heights::extend(draft, cam, &heights)?;
                for field in &mut draft.fields {
                    if let Some((original, text)) = saved.remove(&field.path) {
                        field.original = original;
                        field.text = text;
                    }
                }
                context.heights = heights;
            }
        }
        Ok(())
    })();
    draft.operation_edit = Some(context);
    result
}

pub(super) fn retain_section(previous: Option<&Draft>, next: &mut Draft) {
    let Some(previous) = previous.filter(|previous| previous.selection == next.selection) else {
        return;
    };
    if let Ok(section) = form::text(previous, "/native/ui/operation_section") {
        form::set(next, "/native/ui/operation_section", section);
    }
}

pub(super) fn apply(
    draft: &Draft,
    record: &mut Value,
    cam: &mut CamDocumentDto,
) -> Result<(), String> {
    let context = draft
        .operation_edit
        .as_ref()
        .ok_or("Reopen the operation editor")?;
    parameters::apply(draft, record)?;
    let changed_geometry = context
        .geometry
        .as_ref()
        .map(|geometry| operation_geometry::apply(draft, record, cam.units, geometry))
        .transpose()?
        .unwrap_or(false);
    if let Some(geometry) = context.geometry.as_ref().filter(|_| changed_geometry) {
        let operation: CamOperationDto =
            serde_json::from_value(record.clone()).map_err(|e| e.to_string())?;
        let heights = context
            .heights
            .with_resolved_holes(&operation)
            .unwrap_or_else(|| {
                heights::Context::new(
                    &geometry.setup,
                    &operation,
                    &geometry.scene,
                    &geometry.sketches,
                )
                .with_geometry(
                    cam,
                    &geometry.setup,
                    operation.id(),
                    &geometry.scene,
                    &geometry.sketches,
                )
            });
        let heights = heights.with_picks_from(&context.heights);
        heights::apply(draft, record, cam, &heights, true)?;
    } else {
        heights::apply(draft, record, cam, &context.heights, false)?;
    }
    linking::apply(
        draft,
        record,
        cam,
        &context.linking,
        &context.linking_points,
    )?;
    Ok(())
}
