//! Exact projected-edge picks and disposable smart-dimension placement.
//! Only the existing drawing document is committed, after a separate placement.
use super::super::drawing_paper;
use super::{anchors, runtime::Target, Stamp};
use limo_cad_sketch::*;
pub(super) mod candidates;
pub(super) use candidates::targets;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LineTarget {
    pub view_id: u64,
    pub reference: DrawingLineRefDto,
    pub paper: [[f64; 2]; 2],
    pub pick_segments: Vec<[[f64; 2]; 2]>,
    pub scale: f64,
}
pub(super) fn same_line(a: &DrawingLineRefDto, b: &DrawingLineRefDto) -> bool {
    a.occurrence_id == b.occurrence_id
        && a.body_id == b.body_id
        && a.edge_id == b.edge_id
        && a.edge_key == b.edge_key
}
fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn unit(a: [f64; 2]) -> [f64; 2] {
    let l = a[0].hypot(a[1]);
    [a[0] / l, a[1] / l]
}
pub(super) fn mode(first: &LineTarget, second: Option<&LineTarget>) -> DrawingLineDimensionMode {
    let Some(second) = second else {
        return DrawingLineDimensionMode::Length;
    };
    let a = unit(sub(first.paper[1], first.paper[0]));
    let b = unit(sub(second.paper[1], second.paper[0]));
    if (a[0] * b[1] - a[1] * b[0]).abs() <= 1_f64.to_radians().sin() {
        DrawingLineDimensionMode::Distance
    } else {
        DrawingLineDimensionMode::Angle
    }
}
enum Stage {
    Line {
        first: LineTarget,
        second: Option<LineTarget>,
    },
    PointLine {
        point: Target,
        line: LineTarget,
    },
}
#[derive(Default)]
pub(super) struct Placement {
    owner: Option<(Stamp, u64)>,
    stage: Option<Stage>,
    position: [f64; 2],
}
impl Placement {
    pub fn cancel(&mut self) {
        self.owner = None;
        self.stage = None;
    }
    pub fn active(&self) -> bool {
        self.stage.is_some()
    }
    fn observe(&mut self, stamp: &Stamp, view: u64) {
        if self
            .owner
            .as_ref()
            .is_none_or(|(saved, id)| saved != stamp || *id != view)
        {
            self.cancel();
            self.owner = Some((stamp.clone(), view));
        }
    }
    pub fn selected(&self, target: &LineTarget) -> bool {
        match &self.stage {
            Some(Stage::Line { first, second }) => {
                same_line(&first.reference, &target.reference)
                    || second
                        .as_ref()
                        .is_some_and(|s| same_line(&s.reference, &target.reference))
            }
            Some(Stage::PointLine { line, .. }) => same_line(&line.reference, &target.reference),
            None => false,
        }
    }
    pub fn selected_anchor(&self, view: u64, reference: &DrawingTopologyAnchorRefDto) -> bool {
        matches!(&self.stage, Some(Stage::PointLine {point,..})
            if point.view_id == view && anchors::same_anchor(&point.reference, reference))
    }
    pub fn anchor(&mut self, stamp: &Stamp, point: Target) -> bool {
        self.observe(stamp, point.view_id);
        let line = match &self.stage {
            Some(Stage::Line {
                first,
                second: None,
            }) => first.clone(),
            Some(Stage::PointLine { line, .. }) => line.clone(),
            _ => {
                self.cancel();
                return false;
            }
        };
        let u = unit(sub(line.paper[1], line.paper[0]));
        self.position = [point.paper[0] + 12. * u[0], point.paper[1] + 12. * u[1]];
        self.stage = Some(Stage::PointLine { point, line });
        true
    }
    pub fn edge(&mut self, stamp: &Stamp, line: LineTarget, point: Option<Target>) {
        self.observe(stamp, line.view_id);
        if let Some(point) = point.filter(|p| p.view_id == line.view_id) {
            let u = unit(sub(line.paper[1], line.paper[0]));
            self.position = [point.paper[0] + 12. * u[0], point.paper[1] + 12. * u[1]];
            self.stage = Some(Stage::PointLine { point, line });
            return;
        }
        match &mut self.stage {
            Some(Stage::PointLine { line: selected, .. }) => *selected = line,
            Some(Stage::Line { first, second }) => {
                if same_line(&first.reference, &line.reference)
                    || second
                        .as_ref()
                        .is_some_and(|s| same_line(&s.reference, &line.reference))
                {
                    *second = None;
                } else {
                    *second = Some(line);
                }
            }
            None => {
                let u = unit(sub(line.paper[1], line.paper[0]));
                self.position = [
                    (line.paper[0][0] + line.paper[1][0]) / 2. - 12. * u[1],
                    (line.paper[0][1] + line.paper[1][1]) / 2. + 12. * u[0],
                ];
                self.stage = Some(Stage::Line {
                    first: line,
                    second: None,
                });
            }
        }
    }
    pub fn move_to(&mut self, point: [f64; 2], sheet_mm: [f64; 2]) -> Result<(), String> {
        if point.iter().any(|v| !v.is_finite())
            || sheet_mm.iter().any(|v| !v.is_finite() || *v < 10.)
        {
            return Err("Invalid dimension paper position".into());
        }
        self.position = std::array::from_fn(|i| point[i].clamp(5., sheet_mm[i] - 5.));
        Ok(())
    }
    pub fn annotation(&self, id: u64) -> Option<DrawingAnnotationDto> {
        let (_, view_id) = self.owner.as_ref()?;
        Some(match self.stage.as_ref()? {
            Stage::Line { first, second } => DrawingAnnotationDto::LineDimension {
                id,
                view_id: *view_id,
                first: first.reference.clone(),
                second: second.as_ref().map(|s| s.reference.clone()),
                mode: mode(first, second.as_ref()),
                position: self.position,
                prefix: String::new(),
                suffix: String::new(),
                precision: if mode(first, second.as_ref()) == DrawingLineDimensionMode::Angle {
                    1
                } else {
                    2
                },
                presentation: Default::default(),
            },
            Stage::PointLine { point, line } => DrawingAnnotationDto::PointLineDimension {
                id,
                view_id: *view_id,
                point: point.reference.clone(),
                line: line.reference.clone(),
                position: self.position,
                prefix: String::new(),
                suffix: String::new(),
                precision: 2,
                presentation: Default::default(),
            },
        })
    }
    pub fn valid(&self) -> bool {
        match &self.stage {
            Some(Stage::Line { first, second }) => drawing_paper::valid_line_dimension(
                first.paper,
                second.as_ref().map(|s| s.paper),
                mode(first, second.as_ref()),
                self.position,
                first.scale,
            ),
            Some(Stage::PointLine { point, line }) => {
                drawing_paper::valid_point_line(point.paper, line.paper, self.position, line.scale)
            }
            None => false,
        }
    }
    pub fn create(
        &self,
        document: &DrawingDocumentDto,
        stamp: &Stamp,
    ) -> Result<DrawingDocumentDto, String> {
        if self.owner.as_ref().is_none_or(|(saved, _)| saved != stamp) {
            return Err("Dimension selection changed".into());
        }
        if !self.valid() {
            return Err("Choose edges or a point with a nonzero projected dimension".into());
        }
        let mut next = document.clone();
        let id = next.next_annotation_id;
        let annotation = self
            .annotation(id)
            .ok_or("Choose dimension geometry first")?;
        next.next_annotation_id = id.checked_add(1).ok_or("Annotation IDs are exhausted")?;
        let sheet = next
            .sheets
            .iter_mut()
            .find(|s| s.id == stamp.sheet_id)
            .ok_or("Drawing sheet changed")?;
        sheet.annotations.push(annotation);
        if sheet.release.status == DrawingReleaseStatus::Released {
            sheet.release.status = DrawingReleaseStatus::Draft;
        }
        next.validate()?;
        Ok(next)
    }
}

pub(super) fn hit(targets: &[LineTarget], point: [f64; 2], tolerance: f64) -> Option<usize> {
    targets
        .iter()
        .enumerate()
        .flat_map(|(i, t)| {
            t.pick_segments
                .iter()
                .map(move |segment| (i, candidates::distance(point, *segment)))
        })
        .filter(|(_, d)| *d <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod tests;
