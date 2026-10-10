//! The existing three-anchor angular command, stamped to one live projection.
use super::{anchors, Stamp};
use limo_cad_sketch::{drawing_commands::AddAngularDimension, DrawingTopologyAnchorRefDto};

#[derive(Default)]
pub(super) struct Placement {
    owner: Option<(Stamp, u64)>,
    pub picks: Vec<(DrawingTopologyAnchorRefDto, [f64; 2])>,
}
impl Placement {
    pub fn cancel(&mut self) {
        self.owner = None;
        self.picks.clear();
    }
    pub fn observe(&mut self, stamp: &Stamp) {
        if self.owner.as_ref().is_some_and(|(saved, _)| saved != stamp) {
            self.cancel();
        }
    }
    pub fn selected(&self, view: u64, anchor: &DrawingTopologyAnchorRefDto) -> bool {
        self.owner.as_ref().is_some_and(|(_, id)| *id == view)
            && self
                .picks
                .iter()
                .any(|(a, _)| anchors::same_anchor(a, anchor))
    }
    pub fn click(
        &mut self,
        stamp: &Stamp,
        view: u64,
        anchor: DrawingTopologyAnchorRefDto,
        point: [f64; 2],
    ) -> Result<Option<AddAngularDimension>, String> {
        self.observe(stamp);
        if self.owner.as_ref().is_none_or(|(_, id)| *id != view) {
            self.cancel();
            self.owner = Some((stamp.clone(), view));
        }
        if self.selected(view, &anchor) {
            return Ok(None);
        }
        if point.iter().any(|n| !n.is_finite()) {
            return Err("Projected angular anchor is not finite".into());
        }
        if self.picks.len() == 2 {
            valid_angle(self.picks[0].1, self.picks[1].1, point)?;
            let request = AddAngularDimension {
                sheet_id: stamp.sheet_id,
                view_id: view,
                vertex: self.picks[0].0.clone(),
                first: self.picks[1].0.clone(),
                second: anchor,
                radius: 12.,
                precision: 1,
                prefix: String::new(),
                suffix: String::new(),
                presentation: Default::default(),
            };
            self.cancel();
            return Ok(Some(request));
        }
        if self
            .picks
            .first()
            .is_some_and(|(_, p)| (point[0] - p[0]).hypot(point[1] - p[1]) < 1e-7)
        {
            return Err("Choose a ray endpoint away from the vertex".into());
        }
        self.picks.push((anchor, point));
        Ok(None)
    }
}
fn valid_angle(vertex: [f64; 2], first: [f64; 2], second: [f64; 2]) -> Result<(), String> {
    let a = [first[0] - vertex[0], first[1] - vertex[1]];
    let b = [second[0] - vertex[0], second[1] - vertex[1]];
    let lengths = [a[0].hypot(a[1]), b[0].hypot(b[1])];
    if lengths.iter().any(|l| !l.is_finite() || *l < 1e-7) {
        return Err("Choose ray endpoints away from the angular vertex".into());
    }
    let angle = ((a[0] / lengths[0]) * (b[0] / lengths[1])
        + (a[1] / lengths[0]) * (b[1] / lengths[1]))
        .clamp(-1., 1.)
        .acos();
    if !angle.is_finite() || angle < 1e-7 {
        return Err("Choose two different angular ray directions".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
