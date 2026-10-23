//! Bounded, immutable inspection data. Never changes the live document or selection.
use super::Frame;
use limo_cad_occt::{
    measurement::{MeasurementReport, MeasurementRequest},
    section_review::{SectionOutcome, SectionReview},
};
use limo_cad_sketch::ProjectVisibilityDto;
use limo_cad_solid::DatumPlaneDefinitionDto;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(default)]
    pub body_ids: Vec<u64>,
    #[serde(default)]
    pub face_ids: Vec<u64>,
    #[serde(default)]
    pub sketch_entity_ids: Vec<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub request: MeasurementRequest,
    pub report: MeasurementReport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_poses: Vec<limo_cad_sketch::BodyPoseDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_body_poses: Vec<limo_cad_sketch::InstanceBodyPoseDto>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<ProjectVisibilityDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub datums: Vec<DatumPlaneDefinitionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<SectionReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement: Option<Measurement>,
}

fn bounded(values: &[u64]) -> bool {
    values.len() <= 16
        && values.iter().all(|id| *id != 0)
        && values
            .iter()
            .enumerate()
            .all(|(i, id)| !values[..i].contains(id))
}
fn point(p: &[f64]) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= 10_000.)
}

impl Inspection {
    pub fn profile_visible(&self, name: &str) -> bool {
        self.visibility
            .as_ref()
            .is_none_or(|v| !v.hidden_sketch_names.iter().any(|n| n == name))
    }

    pub fn validate(&self, frame: &Frame) -> Result<(), String> {
        // Exact inspection endpoints permit only static views and genuine edge
        // highlights. They must never use the extrusion/cut presentation motion.
        if !matches!(
            frame.cue,
            None | Some(
                super::PreviewCue::Body | super::PreviewCue::Dimension | super::PreviewCue::Edges
            )
        ) {
            return Err("Inspection frames cannot animate substitute solid geometry".into());
        }
        if let Some(s) = &self.selection {
            if !bounded(&s.body_ids)
                || !bounded(&s.face_ids)
                || !bounded(&s.sketch_entity_ids)
                || s.body_ids
                    .iter()
                    .any(|id| !frame.scene.bodies.iter().any(|b| b.id.0 == *id))
                || s.face_ids.iter().any(|id| {
                    !frame
                        .scene
                        .bodies
                        .iter()
                        .any(|b| b.faces.iter().any(|f| f.id.0 == *id))
                })
                || s.sketch_entity_ids.iter().any(|id| {
                    !frame
                        .profile
                        .as_ref()
                        .is_some_and(|p| p.entities.iter().any(|e| e.id().0 == *id))
                })
            {
                return Err("Inspection selection must reference distinct owned topology".into());
            }
        }
        if self.datums.len() > 4
            || self.datums.iter().any(|p| {
                p.datum_id.0 == 0
                    || p.name.len() > 256
                    || !point(&p.basis.origin)
                    || [&p.basis.u, &p.basis.v, &p.basis.normal]
                        .iter()
                        .any(|axis| {
                            !point(axis.as_slice())
                                || (axis.iter().map(|v| v * v).sum::<f64>() - 1.).abs() > 1e-6
                        })
                    || p.basis
                        .u
                        .iter()
                        .zip(p.basis.v)
                        .map(|(a, b)| a * b)
                        .sum::<f64>()
                        .abs()
                        > 1e-6
            })
        {
            return Err("Inspection datums must have bounded resolved bases".into());
        }
        if let Some(v) = &self.visibility {
            if !bounded(&v.hidden_body_ids)
                || !bounded(&v.hidden_datum_plane_ids)
                || v.hidden_sketch_names.len() > 16
                || v.hidden_sketch_names.iter().any(|n| n.len() > 256)
                || v.hidden_body_ids
                    .iter()
                    .any(|id| !frame.scene.bodies.iter().any(|b| b.id.0 == *id))
                || v.hidden_datum_plane_ids
                    .iter()
                    .any(|id| !self.datums.iter().any(|d| d.datum_id.0 == *id))
                || v.hidden_sketch_names
                    .iter()
                    .any(|n| !frame.profile.as_ref().is_some_and(|p| &p.name == n))
            {
                return Err(
                    "Inspection visibility references must be included in this frame".into(),
                );
            }
        }
        if let Some(s) = &self.section {
            s.request.validate()?;
            if s.outcome != SectionOutcome::MaterialSection
                || !s.source_errors.is_empty()
                || !frame.scene.bodies.iter().any(|b| b.id == s.request.body_id)
                || !s.request.include_cutaway
                || s.svg.len() > 256 * 1024
                || s.section.iter().map(|l| l.points.len()).sum::<usize>() > 10_000
                || s.section.iter().flat_map(|l| &l.points).any(|p| !point(p))
                || s.probe_spans.len() > 4
                || s.probe_spans.iter().any(|p| {
                    !point(&[p.start_mm, p.end_mm, p.length_mm])
                        || p.length_mm <= 0.
                        || (p.end_mm - p.start_mm - p.length_mm).abs() > 1e-6
                })
                || s.bounds_mm
                    .is_none_or(|b| !point(&b) || b[2] <= b[0] || b[3] <= b[1])
            {
                return Err(
                    "Teaching sections require a successful bounded native material section".into(),
                );
            }
            let b = s
                .cutaway
                .as_ref()
                .ok_or("Teaching section has no native cutaway")?;
            let n = b.positions.len();
            if b.body_id != s.request.body_id
                || !b.display_warnings.is_empty()
                || n < 9
                || n > 300_000
                || n % 3 != 0
                || b.normals.len() != n
                || b.indices.len() < 3
                || b.indices.len() > 300_000
                || b.indices.len() % 3 != 0
                || b.indices.iter().any(|i| *i as usize >= n / 3)
                || b.positions
                    .iter()
                    .chain(&b.normals)
                    .any(|v| !v.is_finite() || v.abs() > 10_000.)
                || b.faces.len() > 10_000
                || b.edges.len() > 10_000
                || b.edges.iter().map(|e| e.points.len()).sum::<usize>() > 100_000
                || b.edges
                    .iter()
                    .flat_map(|e| &e.points)
                    .any(|p| !point(&[p.x, p.y, p.z]))
            {
                return Err("Invalid or excessive native section mesh".into());
            }
            let mut ranges = b
                .faces
                .iter()
                .map(|f| (f.first_index as usize, f.index_count as usize))
                .collect::<Vec<_>>();
            ranges.sort_unstable();
            let mut end = 0;
            for (start, count) in ranges {
                if start != end
                    || count % 3 != 0
                    || start.checked_add(count).is_none_or(|e| e > b.indices.len())
                {
                    return Err("Native section faces must partition the native mesh".into());
                }
                end = start + count;
            }
            if end != b.indices.len() {
                return Err("Incomplete native section face coverage".into());
            }
            let axis = s.request.plane.axis();
            if b.positions.chunks_exact(3).any(|p| {
                let delta = f64::from(p[axis]) - s.request.offset_mm;
                if s.request.keep_positive {
                    delta < -1e-4
                } else {
                    delta > 1e-4
                }
            }) {
                return Err("Native cutaway does not match its retained side".into());
            }
        }
        if let Some(m) = &self.measurement {
            // Same exact topology resolver as the native Measure controller; no mesh approximation.
            if m.body_poses.len() > 16
                || m.instance_body_poses.len() > 16
                || m.body_poses
                    .iter()
                    .any(|p| p.translation != [0.; 3] || p.rotation != [0., 0., 0., 1.])
                || m.instance_body_poses
                    .iter()
                    .any(|p| p.translation != [0.; 3] || p.rotation != [0., 0., 0., 1.])
            {
                return Err("Teaching measurements require bounded original placements".into());
            }
            limo_cad_occt::measurement::resolve(
                &frame.scene,
                &m.body_poses,
                &m.instance_body_poses,
                &m.request,
            )?;
            let distance = m
                .report
                .distance
                .ok_or("Distance measurement result missing")?;
            let endpoints = m
                .report
                .closest_points
                .ok_or("Native measurement endpoints missing")?;
            if m.request.selections.len() != 2
                || m.report.selections.len() != 2
                || !point(&[distance])
                || distance <= 0.
                || endpoints.iter().any(|p| !point(p))
                || ((0..3)
                    .map(|i| (endpoints[0][i] - endpoints[1][i]).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    - distance)
                    .abs()
                    > 1e-5
            {
                return Err("Invalid native distance measurement".into());
            }
        }
        // Avoid silently layering mutually unrelated inspection modes.
        if self.section.is_some()
            && (self.measurement.is_some() || self.visibility.is_some() || !self.datums.is_empty())
        {
            return Err(
                "Keep section teaching frames independent of other inspection modes".into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_rejects_missing_ownership_duplicate_ids_and_pending_native_results() {
        let frame = Frame::default();
        for ids in [vec![1], vec![1, 1], vec![0], vec![1; 17]] {
            let value = Inspection {
                selection: Some(Selection {
                    body_ids: ids,
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert!(value.validate(&frame).is_err());
        }
        assert!(
            serde_json::from_str::<Inspection>(r#"{"native_measurement_pending":true}"#).is_err()
        );
        assert!(Inspection::default().validate(&frame).is_ok());
    }
}
