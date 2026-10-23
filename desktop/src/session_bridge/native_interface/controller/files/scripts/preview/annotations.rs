//! Noninteractive annotations aligned with the currently published teaching image.
use super::super::super::super::chrome::{self, rect};
use super::*;
use crate::native_editor::annotations::teaching::Annotations;

#[derive(Component)]
pub(super) struct DimensionLabel;

#[derive(Resource, Default)]
struct Cache {
    key: Option<(String, u64, [u32; 2])>,
    projected: Annotations,
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    model: Entity,
    size: [f32; 2],
    current: bool,
) -> Result<(), String> {
    let preview = &world.resource::<Files>().script.preview;
    let Some(retained) = preview.retained.as_ref().filter(|retained| {
        current
            && preview.geometry_animation_complete()
            && retained.descriptor.cues[preview.frame_index()]
                == Some(crate::native_viewport::script_preview::PreviewCue::Dimension)
    }) else {
        return Ok(());
    };
    let unchanged = world.get_resource::<Cache>().is_some_and(|cache| {
        cache
            .key
            .as_ref()
            .is_some_and(|(id, revision, dimensions)| {
                id == &retained.descriptor.preview_id
                    && *revision == preview.revision
                    && *dimensions == size.map(f32::to_bits)
            })
    });
    let request = (!unchanged).then(|| RenderRequest {
        palette: Some(preview.palette),
        preview_id: retained.descriptor.preview_id.clone(),
        view_id: retained.view.clone(),
        revision: preview.revision,
        frame_index: preview.frame_index(),
        width: preview.size[0],
        height: preview.size[1],
        yaw: preview.yaw,
        pitch: preview.pitch,
    });
    let service = retained.service.clone();
    let alpha = preview.blend.as_ref().map_or(1., |blend| blend.alpha());
    let mut cache = world.remove_resource::<Cache>().unwrap_or_default();
    if let Some(request) = request {
        cache.projected = service.dimension_annotations(&request, size)?;
        cache.key = Some((request.preview_id, request.revision, size.map(f32::to_bits)));
    }
    let palette = crate::native_viewport::ui::palette(world);
    let color = Color::srgb(
        palette.dimension[0],
        palette.dimension[1],
        palette.dimension[2],
    )
    .with_alpha(alpha);
    let background = Color::srgb(
        palette.background[0],
        palette.background[1],
        palette.background[2],
    )
    .with_alpha(alpha);
    for (index, [a, b]) in cache.projected.lines.iter().enumerate() {
        let delta = Vec2::from_array(*b) - Vec2::from_array(*a);
        let key = format!("lesson-dimension-line-{index}");
        widgets.panel(
            world,
            camera,
            &key,
            rect(
                (a[0] + b[0] - delta.length()) / 2.,
                (a[1] + b[1]) / 2. - 0.5,
                delta.length(),
                1.,
            ),
            color,
            62,
        );
        world
            .entity_mut(widgets.entity(&key).unwrap())
            .remove::<interface_shell::InterfaceOccluder>()
            .insert((
                ChildOf(model),
                interface_shell::InterfacePointerPassthrough,
                UiTransform::from_rotation(Rot2::radians(delta.y.atan2(delta.x))),
            ));
    }
    for (index, label) in cache.projected.labels.iter().enumerate() {
        let key = format!("lesson-dimension-label-{index}");
        widgets.colored_text(
            world,
            camera,
            &key,
            rect(
                label.center[0] - label.width / 2.,
                label.center[1] - 10.,
                label.width,
                20.,
            ),
            &label.text,
            12.,
            63,
            color,
        );
        world.entity_mut(widgets.entity(&key).unwrap()).insert((
            DimensionLabel,
            ChildOf(model),
            interface_shell::InterfacePointerPassthrough,
            BackgroundColor(background),
            TextLayout::justify(Justify::Center),
            UiTransform::from_rotation(Rot2::radians(label.angle)),
        ));
    }
    world.insert_resource(cache);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_script_preview_dimensions_follow_authored_values_and_camera() {
        let source = &catalog::examples()
            .iter()
            .find(|example| example.id == lesson::RECIPE)
            .unwrap()
            .source;
        let frames = prepared::prepare(
            &Mutex::new(prepared::Cache::default()),
            source,
            Some(lesson::Id::Sketch),
        )
        .unwrap();
        let index = frames
            .iter()
            .position(|frame| {
                frame.cue == Some(crate::native_viewport::script_preview::PreviewCue::Dimension)
            })
            .unwrap();
        let expected: Vec<_> = frames[index]
            .profile
            .as_ref()
            .unwrap()
            .dimensions
            .iter()
            .map(|dimension| dimension.text.clone())
            .collect();
        assert!(!expected.is_empty());
        let service = PreviewService::default();
        let descriptor = service.retain(frames).unwrap();
        let mut request = RenderRequest {
            palette: None,
            preview_id: descriptor.preview_id,
            view_id: uuid::Uuid::new_v4().to_string(),
            revision: 1,
            frame_index: index,
            width: 592,
            height: 400,
            yaw: HOME.0,
            pitch: HOME.1,
        };
        let size = [296., 200.];
        let original = service.dimension_annotations(&request, size).unwrap();
        assert_eq!(
            original
                .labels
                .iter()
                .map(|label| &label.text)
                .collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert!(!original.lines.is_empty());
        for label in &original.labels {
            let (sin, cos) = label.angle.sin_cos();
            let half = [
                cos.abs() * label.width / 2. + sin.abs() * 10.,
                sin.abs() * label.width / 2. + cos.abs() * 10.,
            ];
            for axis in 0..2 {
                assert!(label.center[axis] - half[axis] >= 0.);
                assert!(label.center[axis] + half[axis] <= size[axis]);
            }
        }
        request.yaw += 0.25;
        let orbit = service.dimension_annotations(&request, size).unwrap();
        assert_eq!(orbit.labels.len(), expected.len());
        assert_ne!(
            orbit.lines, original.lines,
            "Leaders must track the same camera as geometry"
        );
        request.frame_index = 0;
        let plane = service.dimension_annotations(&request, size).unwrap();
        assert!(plane.labels.is_empty() && plane.lines.is_empty());
        request.frame_index = index + 1;
        let finish = service.dimension_annotations(&request, size).unwrap();
        assert!(finish.labels.is_empty() && finish.lines.is_empty());
    }
}
