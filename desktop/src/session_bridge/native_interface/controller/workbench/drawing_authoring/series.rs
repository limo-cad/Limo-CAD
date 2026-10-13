//! The release drawing tools collect three endpoints for a series and two for
//! an ordinate. Only the existing shared document is committed.
use super::{anchors, Stamp};
use limo_cad_sketch::*;

#[derive(Default)]
pub(super) struct Placement {
    owner: Option<(Stamp, u64)>,
    pub picks: Vec<DrawingTopologyAnchorRefDto>,
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
            && self.picks.iter().any(|a| anchors::same_anchor(a, anchor))
    }
    pub fn click(
        &mut self,
        stamp: &Stamp,
        view: u64,
        anchor: DrawingTopologyAnchorRefDto,
        layout: Option<DrawingChainDimensionLayout>,
        document: &DrawingDocumentDto,
    ) -> Result<Option<DrawingDocumentDto>, String> {
        self.observe(stamp);
        if self.owner.as_ref().is_none_or(|(_, id)| *id != view) {
            self.cancel();
            self.owner = Some((stamp.clone(), view));
        }
        if anchor.circle_center {
            return Err("Choose a projected endpoint".into());
        }
        if self.selected(view, &anchor) {
            return Ok(None);
        }
        let count = if layout.is_some() { 3 } else { 2 };
        if self.picks.len() + 1 == count {
            let mut picks = self.picks.clone();
            picks.push(anchor);
            let next = create(document, stamp.sheet_id, view, picks, layout)?;
            self.cancel();
            return Ok(Some(next));
        }
        self.picks.push(anchor);
        Ok(None)
    }
}

pub(super) fn create(
    document: &DrawingDocumentDto,
    sheet_id: u64,
    view_id: u64,
    anchors: Vec<DrawingTopologyAnchorRefDto>,
    layout: Option<DrawingChainDimensionLayout>,
) -> Result<DrawingDocumentDto, String> {
    if anchors.len() != if layout.is_some() { 3 } else { 2 } {
        return Err("Choose all dimension endpoints first".into());
    }
    let mut next = document.clone();
    let id = next.next_annotation_id;
    next.next_annotation_id = id.checked_add(1).ok_or("Annotation IDs are exhausted")?;
    let sheet = next
        .sheets
        .iter_mut()
        .find(|s| s.id == sheet_id)
        .ok_or("Drawing sheet changed")?;
    let annotation = if let Some(layout) = layout {
        DrawingAnnotationDto::ChainDimension {
            id,
            view_id,
            anchors,
            mode: DrawingLinearDimensionMode::Aligned,
            layout,
            offset: 12.,
            spacing: 7.,
            prefix: String::new(),
            suffix: String::new(),
            precision: 2,
            presentation: Default::default(),
        }
    } else {
        DrawingAnnotationDto::OrdinateDimension {
            id,
            view_id,
            origin: anchors[0].clone(),
            target: anchors[1].clone(),
            axis: DrawingOrdinateAxis::Both,
            offset: 10.,
            precision: 2,
            presentation: Default::default(),
        }
    };
    sheet.annotations.push(annotation);
    if sheet.release.status == DrawingReleaseStatus::Released {
        sheet.release.status = DrawingReleaseStatus::Draft;
    }
    next.validate()?;
    Ok(next)
}

#[cfg(test)]
pub(super) mod tests;
