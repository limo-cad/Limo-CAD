//! Revision-cloud placement uses the existing paper polygon, never view geometry.
use super::Stamp;
use limo_cad_sketch::*;

#[derive(Default)]
pub(super) struct Placement {
    owner: Option<Stamp>,
    pub points: Vec<[f64; 2]>,
}
impl Placement {
    pub fn cancel(&mut self) {
        self.owner = None;
        self.points.clear();
    }
    /// Close after the fourth click, or after three vertices when the
    /// click is within four paper millimetres of the first. The closing click
    /// is not an extra vertex. Staging neither allocates an ID nor edits paper.
    pub fn click(
        &mut self,
        stamp: &Stamp,
        point: [f64; 2],
        document: &DrawingDocumentDto,
    ) -> Result<Option<DrawingDocumentDto>, String> {
        if point.iter().any(|v| !v.is_finite()) {
            return Err("Invalid revision cloud paper point".into());
        }
        if self.owner.as_ref() != Some(stamp) {
            self.cancel();
            self.owner = Some(stamp.clone());
        }
        let close = self.points.len() >= 3
            && (point[0] - self.points[0][0]).hypot(point[1] - self.points[0][1]) <= 4.;
        if self.points.len() < 3 {
            self.points.push(point);
            return Ok(None);
        }
        let mut next = document.clone();
        let id = next.next_annotation_id;
        next.next_annotation_id = id.checked_add(1).ok_or("Annotation IDs are exhausted")?;
        let sheet = next
            .sheets
            .iter_mut()
            .find(|s| s.id == stamp.sheet_id)
            .ok_or("Drawing sheet changed")?;
        let mut points = self.points.clone();
        if !close {
            points.push(point);
        }
        sheet.annotations.push(DrawingAnnotationDto::RevisionCloud {
            id,
            revision: if sheet.title_block.revision.is_empty() {
                "A".into()
            } else {
                sheet.title_block.revision.clone()
            },
            points,
        });
        if sheet.release.status == DrawingReleaseStatus::Released {
            sheet.release.status = DrawingReleaseStatus::Draft;
        }
        next.validate()?;
        Ok(Some(next))
    }
}

/// One coarse control per polygon edge, with a narrow phase against the actual
/// clockwise scallops. Work is constant even for a very long loaded edge:
/// only the nearest scallop and its two neighbours can be closest. No mesh,
/// projection, per-scallop UI entities, or transient zoom-dependent work.
pub(super) fn edge_distance(point: [f64; 2], [a, b]: [[f64; 2]; 2]) -> Option<f64> {
    if point.iter().chain(&a).chain(&b).any(|v| !v.is_finite()) {
        return None;
    }
    let delta = [b[0] - a[0], b[1] - a[1]];
    let length = delta[0].hypot(delta[1]);
    if !length.is_finite() || length < 1e-8 {
        return None;
    }
    let direction = delta.map(|v| v / length);
    let offset = [point[0] - a[0], point[1] - a[1]];
    let x = offset[0] * direction[0] + offset[1] * direction[1];
    let y = -offset[0] * direction[1] + offset[1] * direction[0];
    let count = (length / 5.).ceil().max(1.);
    let step = length / count;
    let radius = (step * 0.58).max(1.4);
    let height = (radius * radius - step * step * 0.25).sqrt();
    let half_angle = (step / (2. * radius)).asin();
    let nearest = (x / step).floor().clamp(0., count - 1.);
    let mut distance = f64::INFINITY;
    for i in [-1., 0., 1.].map(|d| (nearest + d).clamp(0., count - 1.)) {
        let left = i * step;
        let dx = x - (left + step * 0.5);
        let dy = y - height;
        distance = distance
            .min((x - left).hypot(y))
            .min((x - left - step).hypot(y));
        let angle = dy.atan2(dx);
        if (angle + std::f64::consts::FRAC_PI_2).abs() <= half_angle {
            distance = distance.min((dx.hypot(dy) - radius).abs());
        }
    }
    distance.is_finite().then_some(distance)
}

pub(super) fn hit(sheet: &DrawingSheetDto, point: [f64; 2]) -> Option<u64> {
    let count: usize = sheet
        .annotations
        .iter()
        .filter_map(|a| match a {
            DrawingAnnotationDto::RevisionCloud { points, .. } => Some(points.len()),
            _ => None,
        })
        .sum();
    if count > 4096 {
        return None;
    }
    let mut best: Option<(u64, f64)> = None;
    for annotation in sheet.annotations.iter().rev() {
        let DrawingAnnotationDto::RevisionCloud { id, points, .. } = annotation else {
            continue;
        };
        for edge in 0..points.len() {
            if let Some(distance) =
                edge_distance(point, [points[edge], points[(edge + 1) % points.len()]])
            {
                if distance <= 3. && best.is_none_or(|(_, saved)| distance < saved) {
                    best = Some((*id, distance));
                }
            }
        }
    }
    best.map(|(id, _)| id)
}

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod tests;
