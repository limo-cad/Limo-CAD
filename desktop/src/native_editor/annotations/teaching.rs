//! Read-only lesson annotations use the editor's authoritative leaders and layout.
use super::*;

pub(crate) struct Label {
    pub text: String,
    pub center: [f32; 2],
    pub width: f32,
    pub angle: f32,
}

#[derive(Default)]
pub(crate) struct Annotations {
    pub lines: Vec<[[f32; 2]; 2]>,
    pub labels: Vec<Label>,
}

pub(crate) fn project(
    sketch: &SketchDto,
    camera: native_viewport::ViewportCamera,
    size: [f32; 2],
    raster_size: [f32; 2],
) -> Result<Annotations, String> {
    let project = |point: Point| {
        native_viewport::project_world_point(
            camera,
            raster_size,
            sketch.basis.to_3d([point.x, point.y]),
        )
        .map(|point| {
            point.map(|point| {
                [
                    point[0] * size[0] / raster_size[0],
                    point[1] * size[1] / raster_size[1],
                ]
            })
        })
    };
    let mut result = Annotations::default();
    let mut layout = layout::Layout::new(size);
    for dim in &sketch.dimensions {
        let Some(screen) = project(dim.text_pos)? else {
            continue;
        };
        let mut unit = 0.01_f32;
        for axis in [Point::new(1., 0.), Point::new(0., 1.)] {
            if let Some(point) = project(dim.text_pos + axis)? {
                unit = unit.max((point[0] - screen[0]).hypot(point[1] - screen[1]));
            }
        }
        let leaders = dimension_leaders(dim, sketch, 6. / f64::from(unit));
        let mut lines = Vec::new();
        for [a, b] in leaders {
            if let (Some(a), Some(b)) = (project(a)?, project(b)?) {
                lines.push([a, b]);
            }
        }
        if lines.is_empty() {
            continue;
        }
        let angle = if sketch.dimension_style == limo_cad_core::DimensionStyle::Aligned
            && dim.kind != "angle"
        {
            lines.last().map_or(0., |[a, b]| readable_angle(*a, *b))
        } else {
            0.
        };
        let width = (dim.text.chars().count() as f32 * 7. + 12.).max(36.);
        let preferred = [screen[0] + angle.sin() * 12., screen[1] - angle.cos() * 12.];
        let Some(center) = layout.place(preferred, [width, 20.], angle) else {
            continue;
        };
        if center != preferred {
            lines.push([
                screen,
                [
                    screen[0] + center[0] - preferred[0],
                    screen[1] + center[1] - preferred[1],
                ],
            ]);
        }
        result.lines.extend(lines);
        result.labels.push(Label {
            text: dim.text.clone(),
            center,
            width,
            angle,
        });
    }
    Ok(result)
}
