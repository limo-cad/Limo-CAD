//! Native note and dimension authoring over the shared drawing document.
//! Creation uses existing drawing commands; edits replace the shared DTO under
//! the controller's normal owner/revision/history transaction.
mod anchors;
mod angular;
mod center;
mod center_panel;
mod chamfer;
mod cloud;
mod cloud_panel;
mod draft;
mod fields;
mod hole;
mod input;
mod panel;
mod radial;
mod repair;
mod runtime;
mod series;
mod straight;
mod technical;
mod technical_runtime;
pub(super) use input::process;
pub(super) use runtime::{
    cancel_input, guard, guard_document_switch, native, owns_panel, pointer_active, preview,
    reduce, repair_view, synchronize,
};
pub(crate) use runtime::{Command, Tool};
pub(crate) use technical::Tool as TechnicalTool;

use limo_cad_interface::DocumentContext;
use limo_cad_sketch::{
    drawing_commands::{AddLinearDimension, AddNote},
    DrawingLinearDimensionMode, DrawingTopologyAnchorRefDto,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Stamp {
    pub owner: DocumentContext,
    pub revision: u64,
    pub sheet_id: u64,
}

#[derive(Default)]
pub(super) struct LinearPlacement {
    first: Option<(Stamp, u64, DrawingTopologyAnchorRefDto)>,
}
impl LinearPlacement {
    pub fn cancel(&mut self) {
        self.first = None;
    }
    pub fn observe(&mut self, stamp: &Stamp) {
        if self
            .first
            .as_ref()
            .is_some_and(|(saved, _, _)| saved != stamp)
        {
            self.cancel();
        }
    }
    /// Changing views starts a new pair, matching DrawingWorkspace. Duplicate
    /// clicks never create a zero-span dimension from the same topology anchor.
    pub fn click(
        &mut self,
        stamp: &Stamp,
        view_id: u64,
        anchor: DrawingTopologyAnchorRefDto,
    ) -> Option<AddLinearDimension> {
        self.observe(stamp);
        if let Some((_, saved_view, first)) = &self.first {
            if *saved_view == view_id {
                if anchors::same_anchor(first, &anchor) {
                    return None;
                }
                let request = AddLinearDimension {
                    sheet_id: stamp.sheet_id,
                    view_id,
                    first: first.clone(),
                    second: anchor,
                    mode: DrawingLinearDimensionMode::Aligned,
                    offset: 12.,
                    prefix: String::new(),
                    suffix: String::new(),
                    precision: 2,
                    presentation: Default::default(),
                };
                self.cancel();
                return Some(request);
            }
        }
        self.first = Some((stamp.clone(), view_id, anchor));
        None
    }
}

#[cfg(test)]
pub(super) fn place_note(sheet_id: u64, position: [f64; 2]) -> AddNote {
    AddNote {
        sheet_id,
        text: "NOTE".into(),
        position,
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod mcp_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod transition_tests;

#[cfg(test)]
#[path = "drawing_authoring/basis_tests.rs"]
mod basis_tests;
