//! Bounded presentation motion over authored endpoints; never runs CAD operations.
use super::*;
use limo_cad_sketch::EntityDto;

#[derive(Default)]
pub(super) enum Motion {
    #[default]
    Still,
    Profile {
        existing: Vec<u64>,
    },
    Extrude {
        axis: usize,
        origin: f32,
        from: f32,
        to: f32,
    },
    Cut {
        before: usize,
        profile: Box<limo_cad_sketch::SketchDto>,
        from: f32,
        to: f32,
    },
    Fillet {
        before: usize,
    },
    Edges,
}
impl Motion {
    pub(super) fn new(document: &PreviewDocument, index: usize) -> Self {
        let frame = &document.frames[index];
        let previous = index.checked_sub(1);
        if frame.cue == Some(PreviewCue::Profile) {
            return Self::Profile {
                existing: previous
                    .and_then(|i| document.frames[i].profile.as_ref())
                    .map(|p| p.entities.iter().map(|e| e.id().0).collect())
                    .unwrap_or_default(),
            };
        }
        if frame.cue == Some(PreviewCue::Edges) {
            return Self::Edges;
        }
        if frame.cue == Some(PreviewCue::Radius) {
            return previous
                .map(|before| Self::Fillet { before })
                .unwrap_or_default();
        }
        let extrusion = document
            .frames
            .iter()
            .any(|f| f.cue == Some(PreviewCue::Extrude));
        if extrusion && matches!(frame.cue, Some(PreviewCue::Extrude | PreviewCue::Depth)) {
            if let Some(profile) = document.frames.iter().find_map(|f| f.profile.as_ref()) {
                // Transform supports the authored axis-aligned extrusion. Other
                // orientations retain the truthful endpoint rather than shear.
                if let Some(axis) = profile.basis.normal.iter().position(|n| n.abs() > 0.999999) {
                    let origin = profile.basis.origin[axis] as f32;
                    let extent = |i: usize| {
                        document.frames[i]
                            .scene
                            .bodies
                            .iter()
                            .flat_map(|b| b.mesh.positions.as_chunks::<3>().0.iter())
                            .map(|p| (p[axis] - origin).abs())
                            .fold(0_f32, f32::max)
                    };
                    let to = extent(index);
                    if to > 0.0001 {
                        return Self::Extrude {
                            axis,
                            origin,
                            from: previous.map(extent).unwrap_or(0.),
                            to,
                        };
                    }
                }
            }
        }
        if !extrusion && matches!(frame.cue, Some(PreviewCue::Cut | PreviewCue::Depth)) {
            // Scenario alternatives append after the through-cut endpoint. The
            // measured branch starts from Cut, not from that unrelated endpoint.
            let before = if frame.cue == Some(PreviewCue::Depth) {
                (0..index)
                    .rev()
                    .find(|i| document.frames[*i].cue == Some(PreviewCue::Cut))
            } else {
                previous
            };
            if let (Some(before), Some(profile)) = (
                before,
                document.frames[..=index]
                    .iter()
                    .rev()
                    .find_map(|f| f.profile.as_ref()),
            ) {
                if let Some(to) = circle_depth(&frame.scene, profile) {
                    let from = circle_depth(&document.frames[before].scene, profile).unwrap_or(0.);
                    return Self::Cut {
                        before,
                        profile: Box::new(profile.clone()),
                        from,
                        to,
                    };
                }
            }
        }
        Self::Still
    }
    pub(super) fn source(&self, index: usize, progress: f32) -> usize {
        match self {
            Self::Cut { before, .. } | Self::Fillet { before } if progress < 1. => *before,
            _ => index,
        }
    }
    pub(super) fn transform(&self, progress: f32) -> Transform {
        let mut transform = Transform::default();
        if let Self::Extrude {
            axis,
            origin,
            from,
            to,
        } = *self
        {
            let scale = ((from + (to - from) * progress) / to).max(0.0001);
            transform.scale[axis] = scale;
            transform.translation[axis] = origin * (1. - scale);
        }
        transform
    }
    pub(super) fn is_new_curve(&self, entity: &EntityDto) -> bool {
        matches!(self, Self::Profile { existing } if !existing.contains(&entity.id().0))
            && !matches!(entity, EntityDto::Point { .. })
    }
    pub(super) fn edge_visible(&self, id: u64, edges: &[u64], progress: f32) -> bool {
        !matches!(self, Self::Edges | Self::Fillet { .. })
            || edges
                .iter()
                .position(|v| *v == id)
                .is_some_and(|i| (i as f32) < progress * edges.len() as f32)
    }
    pub(super) fn draw_guide(
        &self,
        gizmos: &mut Gizmos<TeachingSelection>,
        progress: f32,
        color: Color,
    ) {
        let Self::Cut {
            profile, from, to, ..
        } = self
        else {
            return;
        };
        if progress >= 1. {
            return;
        }
        let depth = from + (to - from) * progress;
        let mut basis = profile.basis;
        for axis in 0..3 {
            basis.origin[axis] -= basis.normal[axis] * depth as f64;
        }
        for entity in &profile.entities {
            draw_sketch_curve_at_offset(gizmos, &basis, entity, color, DIRECT_PICK_FEEDBACK_OFFSET);
            if let EntityDto::Circle { center, radius, .. } = entity {
                // A separate depth indicator stays outside the workpiece. The
                // solid remains the previous exact state until Apply/reveal.
                let at = |d: f32| {
                    let p = profile.basis.to_3d([center.x, center.y + radius * 1.6]);
                    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
                        - Vec3::from_array(profile.basis.normal.map(|n| n as f32)) * d
                };
                gizmos.line(at(*from), at(depth), color);
                let side = Vec3::from_array(profile.basis.u.map(|n| n as f32)) * 1.5;
                gizmos.line(at(depth) - side, at(depth) + side, color);
            }
        }
    }
}

// Derive the authored circular cavity depth from its rim vertices. Never infer
// depth from the bracket's overall bounding box (the base extends past the hole).
fn circle_depth(scene: &SolidSceneDto, profile: &limo_cad_sketch::SketchDto) -> Option<f32> {
    let (center, radius) = profile.entities.iter().find_map(|e| match e {
        EntityDto::Circle { center, radius, .. } => Some((*center, *radius)),
        _ => None,
    })?;
    let mut depth: Option<f32> = None;
    for point in scene
        .bodies
        .iter()
        .flat_map(|b| b.mesh.positions.as_chunks::<3>().0.iter())
    {
        let p = [point[0] as f64, point[1] as f64, point[2] as f64];
        let uv = profile.basis.to_2d(p);
        let radial = ((uv[0] - center.x).powi(2) + (uv[1] - center.y).powi(2)).sqrt();
        if (radial - radius).abs() > (radius * 0.001).max(0.001) {
            continue;
        }
        let distance = (0..3)
            .map(|a| (profile.basis.origin[a] - p[a]) * profile.basis.normal[a])
            .sum::<f64>() as f32;
        if distance >= -0.001 {
            depth = Some(depth.unwrap_or(0.).max(distance.max(0.)));
        }
    }
    depth
}

pub(super) fn partial_curve(entity: &EntityDto, fraction: f32) -> Option<EntityDto> {
    if fraction <= 0. {
        return None;
    }
    if fraction >= 1. {
        return Some(entity.clone());
    }
    let mut entity = entity.clone();
    match &mut entity {
        EntityDto::Line { start, end, .. } => {
            end.x = start.x + (end.x - start.x) * fraction as f64;
            end.y = start.y + (end.y - start.y) * fraction as f64;
        }
        EntityDto::Arc {
            start_angle,
            end_angle,
            ..
        } => {
            let mut sweep = *end_angle - *start_angle;
            while sweep <= 0. {
                sweep += std::f64::consts::TAU;
            }
            *end_angle = *start_angle + sweep * fraction as f64;
        }
        EntityDto::Circle {
            id,
            center,
            radius,
            fully_defined,
        } => {
            entity = EntityDto::Arc {
                id: *id,
                center: *center,
                radius: *radius,
                fully_defined: *fully_defined,
                start_angle: 0.,
                end_angle: std::f64::consts::TAU * fraction as f64,
            };
        }
        EntityDto::Spline { tessellation, .. } => {
            let count = tessellation.len().saturating_sub(1) as f32 * fraction;
            let index = count.floor() as usize;
            if index + 1 < tessellation.len() {
                let a = tessellation[index];
                let b = tessellation[index + 1];
                let t = (count - index as f32) as f64;
                tessellation.truncate(index + 2);
                tessellation[index + 1].x = a.x + (b.x - a.x) * t;
                tessellation[index + 1].y = a.y + (b.y - a.y) * t;
            }
        }
        EntityDto::Point { .. } => {}
    }
    Some(entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(bytes: &[u8]) -> PreviewDocument {
        #[derive(serde::Deserialize)]
        struct Envelope {
            frames: Vec<crate::native_viewport::script_preview::Frame>,
        }
        let value: Envelope = ciborium::de::from_reader(bytes).unwrap();
        PreviewDocument::new(value.frames).unwrap()
    }
    #[test]
    fn teaching_animation_uses_authored_extrusion_axis_and_exact_endpoints() {
        let doc = document(include_bytes!("../../../assets/show-me/extrude.cbor"));
        for index in 1..=3 {
            let motion = Motion::new(&doc, index);
            let Motion::Extrude {
                axis,
                from,
                to,
                origin,
            } = motion
            else {
                panic!("missing extrusion motion");
            };
            assert_eq!(axis, 1);
            assert!((origin).abs() < 0.001);
            assert!(to > from);
            assert_eq!(motion.transform(1.), Transform::default());
            assert!((motion.transform(0.5).scale[axis] * to - (from + to) * 0.5).abs() < 0.001);
        }
    }
    #[test]
    fn teaching_animation_cut_guides_match_authored_cavity_depth_and_branch() {
        let doc = document(include_bytes!("../../../assets/show-me/cut.cbor"));
        for (index, expected_from, expected_to, previous) in
            [(1, 0., 2., 0), (2, 2., 8., 1), (4, 2., 4., 1)]
        {
            let motion = Motion::new(&doc, index);
            let Motion::Cut {
                from, to, before, ..
            } = &motion
            else {
                panic!("missing cut motion at {index}");
            };
            assert!((from - expected_from).abs() < 0.01, "from {from}");
            assert!((to - expected_to).abs() < 0.01, "to {to}");
            assert_eq!(*before, previous);
            assert_eq!(motion.source(index, 0.5), previous);
            assert_eq!(motion.source(index, 1.), index);
        }
    }
    #[test]
    fn teaching_animation_draws_only_new_sketch_curves_and_preserves_endpoints() {
        let doc = document(include_bytes!("../../../assets/show-me/sketch.cbor"));
        let motion = Motion::new(&doc, 2);
        let old = doc.frames[1].profile.as_ref().unwrap();
        let full = doc.frames[2].profile.as_ref().unwrap();
        assert!(old.entities.iter().all(|e| !motion.is_new_curve(e)));
        let line = full
            .entities
            .iter()
            .find(|e| motion.is_new_curve(e))
            .unwrap();
        assert!(partial_curve(line, 0.).is_none());
        assert_eq!(partial_curve(line, 1.).as_ref(), Some(line));
        let EntityDto::Line { start, end, .. } = line else {
            panic!("expected line");
        };
        let Some(EntityDto::Line { end: middle, .. }) = partial_curve(line, 0.5) else {
            panic!("expected partial line");
        };
        assert!((middle.x - (start.x + end.x) * 0.5).abs() < 1e-9);
        assert!((middle.y - (start.y + end.y) * 0.5).abs() < 1e-9);
    }
}
