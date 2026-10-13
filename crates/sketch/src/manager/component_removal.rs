use super::*;

impl SketchManager {
    /// Remove one leaf instance without discarding any dependent authored intent.
    /// Native and browser hosts share this operation and its rejection rules.
    pub fn remove_occurrence(
        &mut self,
        request: RemoveOccurrenceRequestDto,
    ) -> Result<ComponentOccurrenceDto, SessionError> {
        self.ensure_no_component_edit()?;
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "Finish the active sketch before removing a component instance".into(),
            ));
        }
        let id = request.occurrence_id;
        for view in &self.named_views {
            if view
                .occurrence_offsets
                .iter()
                .any(|offset| offset.occurrence_id == id)
            {
                return Err(SessionError::Solid(format!(
                    "Change or remove saved view '{}' before removing occurrence {}",
                    view.name, id.0
                )));
            }
        }
        for sheet in &self.drawings.sheets {
            let (_, occurrences) =
                crate::drawing_topology::drawing_sheet_component_references(sheet)
                    .map_err(SessionError::Solid)?;
            if occurrences.contains(&id) {
                return Err(SessionError::Solid(format!(
                    "Reassociate or remove occurrence references in drawing sheet '{}' before removing occurrence {}",
                    sheet.name, id.0
                )));
            }
        }
        for handoff in &self.print_intent.target_handoffs {
            if handoff
                .reference()
                .parts
                .iter()
                .any(|part| part.binding.occurrence_id == id.0)
                || handoff.reference().height_objects.iter().any(|object| {
                    object
                        .source_bindings
                        .iter()
                        .any(|binding| binding.occurrence_id == id.0)
                })
            {
                return Err(SessionError::Solid(format!(
                    "Remove print target handoff '{}' before removing occurrence {}",
                    handoff.name(),
                    id.0
                )));
            }
        }
        for (name, binding) in self
            .print_intent
            .height_ranges
            .iter()
            .map(|range| (range.name.as_str(), &range.binding))
            .chain(
                self.print_intent
                    .layer_height_profiles
                    .iter()
                    .map(|profile| (profile.name.as_str(), &profile.binding)),
            )
        {
            if binding.occurrences.iter().any(|occurrence| {
                occurrence.occurrence_id == id.0 || occurrence.root_occurrence_id == id.0
            }) || binding.groups.iter().any(|group| {
                group.root_occurrence_id == id.0
                    || group
                        .members
                        .iter()
                        .any(|member| member.occurrence_id == id.0)
            }) {
                return Err(SessionError::Solid(format!(
                    "Rebind or remove print height setting '{name}' before removing occurrence {}",
                    id.0
                )));
            }
        }
        let removed = self
            .assembly
            .remove_occurrence(request)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(removed)
    }
}
