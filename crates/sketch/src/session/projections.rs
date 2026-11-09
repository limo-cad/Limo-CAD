use super::*;
use crate::projection::ProjectionBinding;
use std::collections::BTreeMap;

impl SketchSession {
    pub(super) fn reference_edges_iter(&self) -> impl Iterator<Item = &ProjectedEdgeDto> {
        self.projected_edges
            .iter()
            .chain(self.sketch.projection_edges_iter())
    }
    pub(crate) fn support_has_edge(&self, edge: EdgeId) -> bool {
        self.projected_edges
            .iter()
            .any(|reference| reference.edge_id == edge)
    }
    pub(crate) fn projection_bindings(&self) -> &[ProjectionBinding] {
        &self.sketch.projection_bindings
    }

    pub(crate) fn all_projection_bindings(&self) -> Vec<ProjectionBinding> {
        let mut bindings = BTreeMap::new();
        for binding in
            self.projection_bindings()
                .iter()
                .chain(self.undo.iter().chain(&self.redo).flat_map(|command| {
                    command
                        .before
                        .projection_bindings
                        .iter()
                        .chain(&command.after.projection_bindings)
                }))
        {
            bindings.insert(binding.id, binding.clone());
        }
        bindings.into_values().collect()
    }

    pub(crate) fn next_projection_id(&self) -> u32 {
        self.sketch.projection_id_high_water + 1
    }

    pub(crate) fn install_projections(
        &mut self,
        bindings: Vec<ProjectionBinding>,
        resolved: &[ProjectedEdgeDto],
    ) -> Result<(), SessionError> {
        if self.pending_drag.is_some() {
            return Err(SessionError::Solid(
                "Finish the current drag before changing projections".into(),
            ));
        }
        let before = self.sketch.snapshot();
        if let Some(id) = bindings.iter().map(|b| b.id).max() {
            self.sketch.projection_id_high_water = self.sketch.projection_id_high_water.max(id);
        }
        self.sketch.projection_bindings = bindings;
        self.sketch.replace_projection_edges(resolved);
        self.recompute_projection_midpoints();
        self.refresh_profile_identities();
        self.push_command(before);
        Ok(())
    }

    pub(crate) fn projection_has_constraints(&self, id: u32) -> bool {
        let Some(binding) = self
            .projection_bindings()
            .iter()
            .find(|binding| binding.id == id)
        else {
            return false;
        };
        self.sketch.constraints().any(|(_, constraint)| {
            matches!(constraint,
            Constraint::ReferenceMidpoint {edge, ..} | Constraint::ReferenceOnEdge {edge, ..}
            | Constraint::ReferenceDistance {edge, ..} | Constraint::ReferenceAngle {edge, ..}
            if *edge == binding.edge_id())
        })
    }

    pub(crate) fn refresh_projection_geometry(
        &mut self,
        resolved: &BTreeMap<u32, ProjectedEdgeDto>,
    ) {
        let current: Vec<_> = self
            .projection_bindings()
            .iter()
            .filter_map(|binding| resolved.get(&binding.id))
            .cloned()
            .collect();
        self.sketch.replace_projection_edges(&current);
        for command in self.undo.iter_mut().chain(&mut self.redo) {
            command.before.refresh_projection_edges(resolved);
            command.after.refresh_projection_edges(resolved);
        }
        if let Some(snapshot) = &mut self.pending_drag {
            snapshot.refresh_projection_edges(resolved);
        }
        if let Some(snapshot) = &mut self.last_good_drag {
            snapshot.refresh_projection_edges(resolved);
        }
        self.recompute_projection_midpoints();
        self.recompute();
        self.refresh_profile_identities();
    }

    fn recompute_projection_midpoints(&mut self) {
        let midpoints = self
            .projected_edges
            .iter()
            .chain(&self.sketch.projection_edges())
            .filter_map(|edge| edge.midpoint().map(|point| (edge.edge_id, point)))
            .collect();
        self.set_reference_midpoints(midpoints);
    }
}
