//! Chamfer notes retain the existing exact endpoint references and setback DTO.
use super::{anchors, straight, Stamp};
use limo_cad_sketch::*;
mod candidates;
pub(super) use candidates::targets;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    pub line: straight::LineTarget,
    pub first: DrawingTopologyAnchorRefDto,
    pub second: DrawingTopologyAnchorRefDto,
    pub length: f64,
    pub angle: f64,
}
#[derive(Default)]
pub(super) struct Placement {
    owner: Option<Stamp>,
    target: Option<Target>,
    position: [f64; 2],
}
impl Placement {
    pub fn cancel(&mut self) {
        self.owner = None;
        self.target = None;
    }
    pub fn active(&self) -> bool {
        self.target.is_some()
    }
    pub fn selected(&self, target: &Target) -> bool {
        self.target.as_ref().is_some_and(|t| {
            t.line.view_id == target.line.view_id && anchors::same_anchor(&t.first, &target.first)
        })
    }
    pub fn pick(&mut self, stamp: &Stamp, target: Target) {
        let [a, b] = target.line.paper;
        let d = [b[0] - a[0], b[1] - a[1]];
        let n = d[0].hypot(d[1]);
        let mut normal = [-d[1] / n, d[0] / n];
        if normal[1] > 0. {
            normal = normal.map(|v| -v);
        }
        self.position = [
            (a[0] + b[0]) * 0.5 + normal[0] * 14. + 5.,
            (a[1] + b[1]) * 0.5 + normal[1] * 14.,
        ];
        self.owner = Some(stamp.clone());
        self.target = Some(target);
    }
    pub fn move_to(&mut self, point: [f64; 2], sheet: [f64; 2]) -> Result<(), String> {
        if point.iter().any(|v| !v.is_finite()) || sheet.iter().any(|v| !v.is_finite() || *v < 10.)
        {
            return Err("Invalid chamfer note paper position".into());
        }
        self.position = std::array::from_fn(|i| point[i].clamp(5., sheet[i] - 5.));
        Ok(())
    }
    pub fn annotation(&self, id: u64) -> Option<DrawingAnnotationDto> {
        let t = self.target.as_ref()?;
        Some(DrawingAnnotationDto::ChamferNote {
            id,
            view_id: t.line.view_id,
            first: t.first.clone(),
            second: t.second.clone(),
            position: self.position,
            length: t.length,
            angle_deg: t.angle,
            prefix: String::new(),
        })
    }
    pub fn create(
        &self,
        document: &DrawingDocumentDto,
        stamp: &Stamp,
    ) -> Result<DrawingDocumentDto, String> {
        if self.owner.as_ref() != Some(stamp) {
            return Err("Chamfer selection changed".into());
        }
        let mut next = document.clone();
        let id = next.next_annotation_id;
        let a = self.annotation(id).ok_or("Choose a chamfer edge first")?;
        next.next_annotation_id = id.checked_add(1).ok_or("Annotation IDs are exhausted")?;
        let sheet = next
            .sheets
            .iter_mut()
            .find(|s| s.id == stamp.sheet_id)
            .ok_or("Drawing sheet changed")?;
        sheet.annotations.push(a);
        if sheet.release.status == DrawingReleaseStatus::Released {
            sheet.release.status = DrawingReleaseStatus::Draft;
        }
        next.validate()?;
        Ok(next)
    }
}
pub(super) fn hit(targets: &[Target], point: [f64; 2], tolerance: f64) -> Option<usize> {
    targets
        .iter()
        .enumerate()
        .flat_map(|(i, t)| {
            t.line
                .pick_segments
                .iter()
                .map(move |s| (i, straight::candidates::distance(point, *s)))
        })
        .filter(|(_, d)| *d <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod tests;
