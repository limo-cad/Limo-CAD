//! Paper-space geometry and exact topology resolution shared by all annotations.
//! Corresponds to the existing drawing/annotations.ts renderer; fallback model
//! coordinates are diagnostic data and never substitute for missing topology.
use super::geometry::paper_point;
use crate::{DrawingProjectionAnchorEndpoint as Endpoint, DrawingProjectionDto};
use limo_cad_sketch::*;

use crate::drawing_presentation::geometry::*;

#[derive(Clone, Copy)]
pub struct Circle {
    pub center: P,
    pub radius: f64,
    pub model_radius: f64,
}
pub struct Resolver<'a> {
    pub view: &'a DrawingViewDto,
    pub projection: &'a DrawingProjectionDto,
}
impl Resolver<'_> {
    fn signature(&self, body: limo_cad_core::BodyId, expected: &Option<String>) -> Option<()> {
        (self.projection.topology_signatures.get(&body.0.to_string()) == expected.as_ref())
            .then_some(())
    }
    pub fn anchor(&self, r: &DrawingTopologyAnchorRefDto) -> Option<P> {
        self.signature(r.body_id, &r.topology_signature)?;
        if r.circle_center {
            let matches = |c: &&crate::DrawingProjectedCircleDto| {
                c.occurrence_id == r.occurrence_id
                    && c.body_id == r.body_id
                    && c.edge_key == r.edge_key
            };
            let c = self
                .projection
                .circles
                .iter()
                .filter(matches)
                .find(|c| c.edge_id == r.edge_id)
                .or_else(|| self.projection.circles.iter().find(matches))?;
            return Some(paper_point(self.view, c.center, self.projection));
        }
        let endpoint = match r.endpoint {
            DrawingEdgeEndpoint::Start => Endpoint::Start,
            DrawingEdgeEndpoint::End => Endpoint::End,
        };
        let matches = |c: &&crate::DrawingProjectionAnchorDto| {
            c.occurrence_id == r.occurrence_id
                && c.body_id == r.body_id
                && c.edge_key == r.edge_key
                && c.endpoint == endpoint
        };
        let c = self
            .projection
            .anchors
            .iter()
            .filter(matches)
            .find(|c| c.edge_id == r.edge_id)
            .or_else(|| self.projection.anchors.iter().find(matches))?;
        Some(paper_point(self.view, c.point, self.projection))
    }
    pub fn line(&self, r: &DrawingLineRefDto) -> Option<[P; 2]> {
        self.signature(r.body_id, &r.topology_signature)?;
        let endpoint = |endpoint| {
            let matches = |c: &&crate::DrawingProjectionAnchorDto| {
                c.occurrence_id == r.occurrence_id
                    && c.body_id == r.body_id
                    && c.edge_key == r.edge_key
                    && c.endpoint == endpoint
            };
            let c = self
                .projection
                .anchors
                .iter()
                .filter(matches)
                .find(|c| c.edge_id == r.edge_id)
                .or_else(|| self.projection.anchors.iter().find(matches))?;
            Some(paper_point(self.view, c.point, self.projection))
        };
        Some([endpoint(Endpoint::Start)?, endpoint(Endpoint::End)?])
    }
    pub fn circle(&self, r: &DrawingCircularRefDto) -> Option<Circle> {
        self.signature(r.body_id, &r.topology_signature)?;
        let matches = |c: &&crate::DrawingProjectedCircleDto| {
            c.occurrence_id == r.occurrence_id && c.body_id == r.body_id && c.edge_key == r.edge_key
        };
        let c = self
            .projection
            .circles
            .iter()
            .filter(matches)
            .find(|c| c.edge_id == r.edge_id)
            .or_else(|| self.projection.circles.iter().find(matches))?;
        Some(Circle {
            center: paper_point(self.view, c.center, self.projection),
            radius: c.radius * self.view.scale,
            model_radius: c.radius,
        })
    }
    pub fn attachment(&self, r: &DrawingAttachmentRefDto) -> Option<P> {
        match r {
            DrawingAttachmentRefDto::Anchor { reference } => self.anchor(reference),
            DrawingAttachmentRefDto::Circle { reference } => {
                self.circle(reference).map(|c| c.center)
            }
            DrawingAttachmentRefDto::Line { reference } => {
                self.line(reference).map(|[a, b]| midpoint(a, b))
            }
        }
    }
}

pub fn center_between(first: [P; 2], second: [P; 2], extension: f64) -> Option<[P; 2]> {
    let a = sub(first[1], first[0]);
    let b = sub(second[1], second[0]);
    let dir = unit(a)?;
    let other = unit(b)?;
    if cross(dir, other).abs() > 0.75_f64.to_radians().sin() {
        return None;
    }
    let normal = normal(dir);
    let first_mid = midpoint(first[0], first[1]);
    let second_mid = midpoint(second[0], second[1]);
    if dot(sub(second_mid, first_mid), normal).abs() < 1e-4 {
        return None;
    }
    let values = [
        dot(first[0], dir),
        dot(first[1], dir),
        dot(second[0], dir),
        dot(second[1], dir),
    ];
    if values[0].max(values[1]).min(values[2].max(values[3]))
        - values[0].min(values[1]).max(values[2].min(values[3]))
        < length(a).min(length(b)) * 0.05
    {
        return None;
    }
    let offset = dot(midpoint(first_mid, second_mid), normal);
    let low = values.iter().copied().fold(f64::INFINITY, f64::min) - extension.max(0.);
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max) + extension.max(0.);
    Some([
        add(scale(dir, low), scale(normal, offset)),
        add(scale(dir, high), scale(normal, offset)),
    ])
}
