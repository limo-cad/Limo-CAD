//! Presentation over two resident endpoints. No render request or texture allocation.
use super::Frame;
use crate::native_viewport::script_preview::PreviewCue;
use limo_cad_sketch::EntityDto;

pub(super) const STEPS: u8 = 6;

pub(super) struct Blend<H> {
    pub from: H,
    pub target: usize,
    progress: u8,
}
impl<H> Blend<H> {
    pub fn new(from: H, target: usize) -> Self {
        Self {
            from,
            target,
            progress: 0,
        }
    }
    pub fn alpha(&self) -> f32 {
        let t = f32::from(self.progress) / f32::from(STEPS);
        t * t * (3. - 2. * t)
    }
    /// Advance once per consumed UI deadline, including after a long pause.
    pub fn advance(&mut self) -> bool {
        self.progress = (self.progress + 1).min(STEPS);
        self.progress == STEPS
    }
}

/// Match the payloads supported by teaching_animation and teaching_scene before
/// moving them into the service. A cue alone does not establish animated geometry.
pub(super) fn geometry_motion(frames: &[Frame]) -> Vec<bool> {
    let extrusion = frames.iter().any(|f| f.cue == Some(PreviewCue::Extrude));
    frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let previous = index.checked_sub(1);
            match frame.cue {
                Some(PreviewCue::Plane) => frame.profile.is_some(),
                Some(PreviewCue::Profile) => frame.profile.as_ref().is_some_and(|profile| {
                    let before = previous.and_then(|i| frames[i].profile.as_ref());
                    profile.entities.iter().any(|entity| {
                        !matches!(entity, EntityDto::Point { .. })
                            && before
                                .is_none_or(|p| !p.entities.iter().any(|e| e.id() == entity.id()))
                    })
                }),
                Some(PreviewCue::Edges) => !frame.edges.is_empty(),
                Some(PreviewCue::Radius) => previous.is_some_and(|i| !frames[i].edges.is_empty()),
                Some(PreviewCue::Extrude | PreviewCue::Depth) if extrusion => frames
                    .iter()
                    .find_map(|f| f.profile.as_ref())
                    .is_some_and(|profile| {
                        profile
                            .basis
                            .normal
                            .iter()
                            .position(|n| n.abs() > 0.999999)
                            .is_some_and(|axis| {
                                let origin = profile.basis.origin[axis] as f32;
                                let extent = |i: usize| {
                                    frames[i]
                                        .scene
                                        .bodies
                                        .iter()
                                        .flat_map(|body| body.mesh.positions.chunks_exact(3))
                                        .map(|p| (p[axis] - origin).abs())
                                        .fold(0_f32, f32::max)
                                };
                                let to = extent(index);
                                to > 0.0001
                                    && (to - previous.map(extent).unwrap_or(0.)).abs() > 0.0001
                            })
                    }),
                Some(PreviewCue::Cut | PreviewCue::Depth) if !extrusion => {
                    let before = if frame.cue == Some(PreviewCue::Depth) {
                        (0..index)
                            .rev()
                            .find(|i| frames[*i].cue == Some(PreviewCue::Cut))
                    } else {
                        previous
                    };
                    let profile = frames[..=index]
                        .iter()
                        .rev()
                        .find_map(|f| f.profile.as_ref());
                    match (before, profile) {
                        (Some(before), Some(profile)) => {
                            circle_depth(frame, profile).is_some_and(|to| {
                                (to - circle_depth(&frames[before], profile).unwrap_or(0.)).abs()
                                    > 0.0001
                            })
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        })
        .collect()
}

// Same authored circular-rim witness as teaching_animation; other payloads use
// their exact static endpoints. This never infers or modifies CAD geometry.
fn circle_depth(frame: &Frame, profile: &limo_cad_sketch::SketchDto) -> Option<f32> {
    let (center, radius) = profile.entities.iter().find_map(|entity| match entity {
        EntityDto::Circle { center, radius, .. } => Some((*center, *radius)),
        _ => None,
    })?;
    let mut depth: Option<f32> = None;
    for point in frame
        .scene
        .bodies
        .iter()
        .flat_map(|body| body.mesh.positions.chunks_exact(3))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_preview_dissolve_is_six_bounded_monotonic_ui_samples() {
        let mut blend = Blend::new(17_u8, 3);
        assert_eq!((blend.from, blend.target, blend.alpha()), (17, 3, 0.));
        let mut previous = 0.;
        for step in 1..=STEPS {
            assert_eq!(blend.advance(), step == STEPS);
            let alpha = blend.alpha();
            assert!(alpha > previous && alpha <= 1.);
            previous = alpha;
        }
        assert_eq!(blend.alpha(), 1.);
        assert!(blend.advance());
    }

    #[test]
    fn native_preview_static_cues_and_absent_motion_payloads_do_not_schedule_geometry() {
        let frames: Vec<_> = [
            PreviewCue::Dimension,
            PreviewCue::Apply,
            PreviewCue::Body,
            PreviewCue::Profile,
            PreviewCue::Edges,
            PreviewCue::Plane,
            PreviewCue::Radius,
            PreviewCue::Extrude,
            PreviewCue::Depth,
            PreviewCue::Cut,
        ]
        .into_iter()
        .map(|cue| Frame {
            cue: Some(cue),
            ..Default::default()
        })
        .collect();
        assert!(geometry_motion(&frames).iter().all(|moving| !moving));
    }
}
