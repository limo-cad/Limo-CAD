//! Share Bevy's transformed clipping between rendered and inspectable controls.

use super::*;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct HitArea {
    pub bounds: InterfaceRect,
    clip: CalculatedClip,
    origin: [f64; 2],
    scale: f64,
}

impl HitArea {
    pub fn new(
        node: &ComputedNode,
        transform: &UiGlobalTransform,
        clip: Option<&CalculatedClip>,
        surface: InterfaceRect,
    ) -> Self {
        let rect = node.border_box();
        let vertices = [
            rect.min,
            Vec2::new(rect.max.x, rect.min.y),
            rect.max,
            Vec2::new(rect.min.x, rect.max.y),
        ]
        .map(|point| (transform.transform_point2(point), ()));
        let clip = clip.cloned().unwrap_or_default().with_rect(rect, transform);
        let vertices =
            bevy::ui_render::clipping::clip_polygon(Some(&clip), &vertices, |_, _, _| ());
        let scale = f64::from(node.inverse_scale_factor());
        let bounds = if vertices.is_empty() {
            InterfaceRect {
                width: 0.,
                height: 0.,
                ..surface
            }
        } else {
            let min = vertices
                .iter()
                .fold(Vec2::splat(f32::INFINITY), |min, (p, _)| min.min(*p));
            let max = vertices
                .iter()
                .fold(Vec2::splat(f32::NEG_INFINITY), |max, (p, _)| max.max(*p));
            intersection(
                InterfaceRect {
                    x: surface.x + f64::from(min.x) * scale,
                    y: surface.y + f64::from(min.y) * scale,
                    width: f64::from(max.x - min.x) * scale,
                    height: f64::from(max.y - min.y) * scale,
                },
                surface,
            )
        };
        Self {
            bounds,
            clip,
            origin: [surface.x, surface.y],
            scale,
        }
    }

    pub fn contains(&self, point: [f64; 2]) -> bool {
        contains_point(self.bounds, point)
            && self.clip.contains_point(Vec2::new(
                ((point[0] - self.origin[0]) / self.scale) as f32,
                ((point[1] - self.origin[1]) / self.scale) as f32,
            ))
    }
}
