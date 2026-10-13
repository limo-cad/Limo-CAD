//! Definition-level print metadata shares project persistence and document ownership.
use super::*;
use limo_cad_core::{
    configured_print_fields, print_setting_capabilities, resolve_print_settings,
    PartPrintIntentDto, PartPrintIntentEffectiveDto, PrintIntentDocumentDto,
    PrintIntentEffectiveReportDto, PrintIntentPresetDto, PrintIntentScopeDto, PrintIntentTargetDto,
    PrintPartBindingDto, PrintSettingsDto, PrintTargetHandoffDto, ProcessProfileStatusDto,
};

impl SketchManager {
    pub fn print_intent(&self) -> PrintIntentDocumentDto {
        self.print_intent.clone()
    }

    /// Replace metadata atomically. Missing source parts remain reportable orphans.
    pub fn set_print_intent_document(
        &mut self,
        mut document: PrintIntentDocumentDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        if self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "Print intent cannot change during project replacement".into(),
            ));
        }
        self.solids
            .ensure_metadata_editable()
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        document.validate().map_err(SessionError::Solid)?;
        self.validate_height_document_edit(&document)?;
        let retained_bodies = self.retained_presentation_body_ids();
        for modifier in &document.modifiers {
            let old = self
                .print_intent
                .modifiers
                .iter()
                .find(|old| old.id.eq_ignore_ascii_case(&modifier.id));
            if old.is_some_and(|old| old.body_id != modifier.body_id) {
                return Err(SessionError::Solid(
                    "Modifier attachment cannot change; copy it explicitly to another source body"
                        .into(),
                ));
            }
            if !retained_bodies.contains(&modifier.body_id) && old.is_none() {
                return Err(SessionError::Solid(
                    "A new print modifier cannot attach to an unknown source body".into(),
                ));
            }
        }
        let mut known_bodies = self.retained_presentation_body_ids();
        known_bodies.extend(self.print_intent.parts.iter().map(|part| part.body_id));
        known_bodies.extend(
            self.print_intent
                .modifiers
                .iter()
                .map(|modifier| modifier.body_id),
        );
        known_bodies.extend(
            self.print_intent
                .height_ranges
                .iter()
                .map(|range| range.body_id),
        );
        known_bodies.extend(
            self.print_intent
                .layer_height_profiles
                .iter()
                .map(|profile| profile.body_id),
        );
        let known_pairs = self
            .print_intent
            .target_handoffs
            .iter()
            .flat_map(|handoff| {
                handoff
                    .reference()
                    .parts
                    .iter()
                    .map(|part| (part.binding.body_id, part.binding.occurrence_id))
            })
            .collect::<BTreeSet<_>>();
        known_bodies.extend(known_pairs.iter().map(|(body, _)| *body));
        if document
            .parts
            .iter()
            .any(|part| !known_bodies.contains(&part.body_id))
        {
            return Err(SessionError::Solid(
                "Print settings cannot introduce an unknown source body".into(),
            ));
        }
        for binding in document
            .target_handoffs
            .iter()
            .flat_map(|handoff| handoff.reference().parts.iter().map(|part| &part.binding))
        {
            let existing = known_pairs.contains(&(binding.body_id, binding.occurrence_id));
            let current = known_bodies.contains(&binding.body_id)
                && self
                    .assembly
                    .component_structure
                    .occurrences
                    .iter()
                    .find(|occurrence| occurrence.id.0 == binding.occurrence_id)
                    .and_then(|occurrence| {
                        self.assembly
                            .component_structure
                            .definitions
                            .iter()
                            .find(|definition| definition.id == occurrence.component_id)
                    })
                    .is_some_and(|definition| definition.body_ids.contains(&binding.body_id));
            if !existing && !current {
                return Err(SessionError::Solid(
                    "Target handoff cannot introduce an unknown body/occurrence binding".into(),
                ));
            }
        }
        if document.source_document_id != self.print_intent.source_document_id {
            return Err(SessionError::Solid(
                "Print source document identity cannot be changed by an edit".into(),
            ));
        }
        if document.source_document_id.is_none() {
            document.source_document_id = Some(uuid::Uuid::new_v4().to_string());
        }
        document.parts.sort_by_key(|part| part.body_id);
        document.presets.sort_by(|a, b| a.name.cmp(&b.name));
        document
            .modifiers
            .iter_mut()
            .for_each(|modifier| modifier.id.make_ascii_lowercase());
        document.modifiers.sort_by(|a, b| a.id.cmp(&b.id));
        document.height_ranges.sort_by(|a, b| a.id.cmp(&b.id));
        document
            .layer_height_profiles
            .sort_by(|a, b| a.id.cmp(&b.id));
        document
            .target_handoffs
            .sort_by(|a, b| a.name().cmp(b.name()));
        if let Some(body_id) = print_intent_body_floor(&document) {
            self.solids
                .reserve_body_ids_through(body_id)
                .map_err(|error| SessionError::Solid(error.to_string()))?;
        }
        self.assembly.component_structure.next_occurrence_id = self
            .assembly
            .component_structure
            .next_occurrence_id
            .max(print_intent_occurrence_floor(&document));
        self.print_intent = document;
        Ok(self.print_intent())
    }

    pub(super) fn require_print_part(&self, body_id: BodyId) -> Result<(), SessionError> {
        if !self.retained_presentation_body_ids().contains(&body_id) {
            return Err(SessionError::Solid(format!(
                "Print source body {} was not found",
                body_id.0
            )));
        }
        Ok(())
    }

    pub fn set_part_print_intent(
        &mut self,
        body_id: BodyId,
        settings: PrintSettingsDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        self.require_print_part(body_id)?;
        settings.validate().map_err(SessionError::Solid)?;
        let mut document = self.print_intent();
        document.parts.retain(|part| part.body_id != body_id);
        document
            .parts
            .push(PartPrintIntentDto { body_id, settings });
        self.set_print_intent_document(document)
    }

    pub fn reset_part_print_intent(
        &mut self,
        body_id: BodyId,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        if !self
            .print_intent
            .parts
            .iter()
            .any(|part| part.body_id == body_id)
        {
            self.require_print_part(body_id)?;
        }
        let mut document = self.print_intent();
        document.parts.retain(|part| part.body_id != body_id);
        self.set_print_intent_document(document)
    }

    /// Copy explicit overrides. Fresh CAD copies inherit defaults until this deliberate edit.
    pub fn copy_part_print_intent(
        &mut self,
        source_body_id: BodyId,
        targets: Vec<BodyId>,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        if targets.is_empty()
            || targets.len() > 4096
            || targets.iter().collect::<BTreeSet<_>>().len() != targets.len()
        {
            return Err(SessionError::Solid(
                "Copy print intent needs 1..=4096 unique target bodies".into(),
            ));
        }
        let settings = match self
            .print_intent
            .parts
            .iter()
            .find(|part| part.body_id == source_body_id)
        {
            Some(part) => part.settings.clone(),
            None => {
                self.require_print_part(source_body_id)?;
                PrintSettingsDto::default()
            }
        };
        for &body_id in &targets {
            self.require_print_part(body_id)?;
        }
        let mut document = self.print_intent();
        document
            .parts
            .retain(|part| !targets.contains(&part.body_id));
        document
            .parts
            .extend(targets.into_iter().map(|body_id| PartPrintIntentDto {
                body_id,
                settings: settings.clone(),
            }));
        self.set_print_intent_document(document)
    }

    pub fn upsert_print_intent_preset(
        &mut self,
        preset: PrintIntentPresetDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        document.presets.retain(|entry| entry.name != preset.name);
        document.presets.push(preset);
        self.set_print_intent_document(document)
    }

    pub fn upsert_print_intent_handoff(
        &mut self,
        handoff: PrintTargetHandoffDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        document
            .target_handoffs
            .retain(|entry| entry.name() != handoff.name());
        document.target_handoffs.push(handoff);
        self.set_print_intent_document(document)
    }

    pub fn remove_print_intent_handoff(
        &mut self,
        name: &str,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        if !document
            .target_handoffs
            .iter()
            .any(|entry| entry.name() == name)
        {
            return Err(SessionError::Solid(format!(
                "Target handoff '{name}' was not found"
            )));
        }
        document
            .target_handoffs
            .retain(|entry| entry.name() != name);
        self.set_print_intent_document(document)
    }

    pub fn remove_print_intent_preset(
        &mut self,
        name: &str,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        if !document.presets.iter().any(|entry| entry.name == name) {
            return Err(SessionError::Solid(format!(
                "Print preset '{name}' was not found"
            )));
        }
        document.presets.retain(|entry| entry.name != name);
        self.set_print_intent_document(document)
    }

    pub fn effective_print_intent(
        &self,
        body_ids: Vec<BodyId>,
        target: Option<PrintIntentTargetDto>,
    ) -> Result<PrintIntentEffectiveReportDto, SessionError> {
        if body_ids.len() > 4096 || body_ids.iter().collect::<BTreeSet<_>>().len() != body_ids.len()
        {
            return Err(SessionError::Solid(
                "Print settings query needs at most 4096 unique bodies".into(),
            ));
        }
        let retained = self.retained_presentation_body_ids();
        let live: BTreeSet<_> = self
            .solids
            .scene()
            .bodies
            .iter()
            .map(|body| body.id)
            .collect();
        let recorded: BTreeSet<_> = self
            .print_intent
            .parts
            .iter()
            .map(|part| part.body_id)
            .chain(
                self.print_intent
                    .modifiers
                    .iter()
                    .map(|modifier| modifier.body_id),
            )
            .chain(
                self.print_intent
                    .height_ranges
                    .iter()
                    .map(|range| range.body_id),
            )
            .chain(
                self.print_intent
                    .layer_height_profiles
                    .iter()
                    .map(|profile| profile.body_id),
            )
            .collect();
        let orphan_body_ids: Vec<_> = recorded.difference(&retained).copied().collect();
        let selected: BTreeSet<_> = if body_ids.is_empty() {
            live.union(&recorded).copied().collect()
        } else {
            body_ids.into_iter().collect()
        };
        let capabilities = target
            .map(|target| print_setting_capabilities(target, PrintIntentScopeDto::Part))
            .unwrap_or_default();
        let parts = selected
            .into_iter()
            .map(|body_id| {
                if !retained.contains(&body_id) && !recorded.contains(&body_id) {
                    return Err(SessionError::Solid(format!(
                        "Print source body {} was not found",
                        body_id.0
                    )));
                }
                let binding = if live.contains(&body_id) {
                    PrintPartBindingDto::Live
                } else if retained.contains(&body_id) {
                    PrintPartBindingDto::Retained
                } else {
                    PrintPartBindingDto::Orphan
                };
                let requested = self
                    .print_intent
                    .parts
                    .iter()
                    .find(|part| part.body_id == body_id)
                    .map(|part| part.settings.clone())
                    .unwrap_or_default();
                let (settings, sources) = if binding == PrintPartBindingDto::Orphan {
                    (PrintSettingsDto::default(), Default::default())
                } else {
                    resolve_print_settings(&self.print_intent, &requested)
                };
                let configured = configured_print_fields(&settings);
                let unsupported = capabilities
                    .iter()
                    .filter(|cap| !cap.supported && configured.contains(&cap.field))
                    .map(|cap| cap.field)
                    .collect();
                Ok(PartPrintIntentEffectiveDto {
                    body_id,
                    binding,
                    requested,
                    settings,
                    sources,
                    unsupported,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let profile_status = self
            .print_intent
            .selected_process
            .as_ref()
            .map(|profile| profile.status);
        let mut warnings = Vec::new();
        if profile_status == Some(ProcessProfileStatusDto::Unresolved) {
            warnings.push(
                "Selected process profile is unresolved; its inherited defaults are unavailable"
                    .into(),
            );
        }
        if !orphan_body_ids.is_empty() {
            warnings.push("Orphan print overrides are retained for deliberate recovery and do not apply to geometry".into());
        }
        let selected_bodies: BTreeSet<_> = parts.iter().map(|part| part.body_id).collect();
        let modifier_caps = target
            .map(|target| print_setting_capabilities(target, PrintIntentScopeDto::Modifier))
            .unwrap_or_default();
        let modifiers = self.print_intent.modifiers.iter().filter(|modifier| selected_bodies.contains(&modifier.body_id))
            .map(|modifier| {
                let part = parts.iter().find(|part| part.body_id == modifier.body_id).unwrap();
                let (settings, sources) = if part.binding == PrintPartBindingDto::Orphan { (PrintSettingsDto::default(), Default::default()) } else {
                    limo_cad_core::resolve_print_setting_layers([
                        (&part.settings, limo_cad_core::PrintSettingSourceDto::Part),
                        (&modifier.settings, limo_cad_core::PrintSettingSourceDto::Modifier),
                    ])
                };
                let mut sources = sources;
                macro_rules! inherit_sources { ($($field:ident),*) => { $(if modifier.settings.$field.is_none() { sources.$field = part.sources.$field; })* }; }
                inherit_sources!(wall_count, infill_density_percent, infill_pattern, top_shell_layers, bottom_shell_layers);
                let mut occurrence_ids: Vec<_> = self.assembly.component_structure.occurrences.iter().filter(|occurrence| {
                    self.assembly.component_structure.definitions.iter().any(|definition| definition.id == occurrence.component_id && definition.body_ids.contains(&modifier.body_id))
                }).map(|occurrence| occurrence.id.0).collect();
                occurrence_ids.sort_unstable();
                let configured = configured_print_fields(&modifier.settings);
                let unsupported = modifier_caps.iter().filter(|cap| !cap.supported && configured.contains(&cap.field)).map(|cap| cap.field).collect();
                let mut warnings = Vec::new();
                if part.binding == PrintPartBindingDto::Orphan { warnings.push("Orphan modifier is retained for explicit recovery and cannot target new geometry".into()); }
                if !modifier.enabled { warnings.push("Modifier is disabled and omitted from export".into()); }
                if configured.is_empty() { warnings.push("Modifier inherits all settings and has no effect; its shape is retained but omitted from native export".into()); }
                if target.is_some_and(|target| target != PrintIntentTargetDto::BambuStudio) && modifier.enabled && !configured.is_empty() {
                    warnings.push("This target does not carry qualified modifier metadata; print-only geometry is omitted".into());
                }
                Ok(limo_cad_core::PrintModifierEffectiveDto { modifier: modifier.clone(), binding: part.binding, occurrence_ids,
                    local_bounds: limo_cad_core::print_modifier_local_bounds(modifier).map_err(SessionError::Solid)?, settings, sources, unsupported, warnings })
            }).collect::<Result<Vec<_>, SessionError>>()?;
        let (height_ranges, layer_height_profiles) =
            self.effective_print_heights(&selected_bodies, target);
        Ok(PrintIntentEffectiveReportDto {
            selected_process: self.print_intent.selected_process.clone(),
            project_defaults: self.print_intent.defaults.clone(),
            parts,
            orphan_body_ids,
            profile_status,
            warnings,
            capabilities,
            modifiers,
            height_ranges,
            layer_height_profiles,
        })
    }
}

pub(super) fn print_intent_body_floor(document: &PrintIntentDocumentDto) -> Option<BodyId> {
    document
        .parts
        .iter()
        .map(|part| part.body_id)
        .chain(document.modifiers.iter().map(|modifier| modifier.body_id))
        .chain(document.height_ranges.iter().map(|range| range.body_id))
        .chain(
            document
                .layer_height_profiles
                .iter()
                .map(|profile| profile.body_id),
        )
        .chain(
            document
                .height_ranges
                .iter()
                .map(|range| &range.binding)
                .chain(
                    document
                        .layer_height_profiles
                        .iter()
                        .map(|profile| &profile.binding),
                )
                .flat_map(|binding| {
                    binding
                        .groups
                        .iter()
                        .flat_map(|group| group.members.iter().map(|member| member.body_id))
                }),
        )
        .chain(document.target_handoffs.iter().flat_map(|handoff| {
            handoff
                .reference()
                .parts
                .iter()
                .map(|part| part.binding.body_id)
        }))
        .max()
}

pub(super) fn print_intent_occurrence_floor(document: &PrintIntentDocumentDto) -> u64 {
    document
        .target_handoffs
        .iter()
        .flat_map(|handoff| {
            handoff
                .reference()
                .parts
                .iter()
                .map(|part| part.binding.occurrence_id)
        })
        .chain(
            document
                .height_ranges
                .iter()
                .map(|range| &range.binding)
                .chain(
                    document
                        .layer_height_profiles
                        .iter()
                        .map(|profile| &profile.binding),
                )
                .flat_map(|binding| {
                    binding.groups.iter().flat_map(|group| {
                        std::iter::once(group.root_occurrence_id)
                            .chain(group.members.iter().map(|member| member.occurrence_id))
                    })
                }),
        )
        .max()
        .map_or(1, |id| id + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host;
    use limo_cad_core::{InfillPatternDto, PrintSettingSourceDto};
    use limo_cad_solid::{ImportStepRequest, KernelBodyDto, KernelJobDto, KernelSceneDto};
    use serde_json::{json, Value};

    fn raw(body_id: BodyId) -> KernelBodyDto {
        KernelBodyDto {
            body_id,
            topology_signature: String::new(),
            display_warnings: Vec::new(),
            positions: vec![0., 0., 0., 10., 0., 0., 0., 10., 0.],
            normals: [0., 0., 1.].repeat(3),
            indices: vec![0, 1, 2],
            faces: Vec::new(),
            edges: Vec::new(),
        }
    }
    fn commit(manager: &mut SketchManager, plan: RecomputePlanDto, bodies: &[KernelBodyDto]) {
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: bodies.to_vec(),
                    errors: Vec::new(),
                },
            })
            .unwrap();
    }
    fn import(manager: &mut SketchManager, bodies: &mut Vec<KernelBodyDto>) -> (BodyId, FeatureId) {
        let plan = manager
            .prepare_body_feature(BodyFeatureRequestDto::ImportStep(ImportStepRequest {
                file_name: "part.step".into(),
                data_base64: "U1RFUA==".into(),
            }))
            .unwrap();
        let KernelJobDto::ImportStep(job) = plan.jobs.last().unwrap() else {
            panic!("import job")
        };
        let result = (job.result_body_id, job.feature_id);
        bodies.push(raw(result.0));
        commit(manager, plan, bodies);
        result
    }
    fn response(manager: &mut SketchManager, method: &str, payload: Value) -> Value {
        let result: Value =
            serde_json::from_str(&host::handle(manager, method, &payload.to_string())).unwrap();
        assert_eq!(result["ok"], true, "{result}");
        result["value"].clone()
    }
    fn geometry(manager: &SketchManager) -> Value {
        let mut value: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("print_intent");
        value
    }

    #[test]
    fn print_intent_rejects_pending_project_and_geometry_before_assigning_identity() {
        let mut manager = SketchManager::new();
        let mut bodies = Vec::new();
        import(&mut manager, &mut bodies);
        let before = manager.export_project_model().unwrap();
        let plan = manager.prepare_load_project(before.clone()).unwrap();
        let request = json!({"preset":{"name":"Would be lost","settings":{"wall_count":6}},
            "expected_model_json":before});
        let rejected: Value = serde_json::from_str(&host::handle(
            &mut manager,
            "print_intent_upsert_preset",
            &request.to_string(),
        ))
        .unwrap();
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert_eq!(manager.export_project_model().unwrap(), before);
        assert_eq!(manager.print_intent.source_document_id, None);
        commit(&mut manager, plan, &bodies);
        assert_eq!(manager.export_project_model().unwrap(), before);
        let plan = manager.prepare_recompute().unwrap();
        let rejected: Value = serde_json::from_str(&host::handle(
            &mut manager,
            "print_intent_upsert_preset",
            &request.to_string(),
        ))
        .unwrap();
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert_eq!(manager.print_intent.source_document_id, None);
        assert_eq!(manager.export_project_model().unwrap(), before);
        manager.cancel_solid_recompute(plan.transaction_id);
        response(&mut manager, "print_intent_upsert_preset", request);
        assert_eq!(manager.print_intent.presets.len(), 1);
        assert!(manager.print_intent.source_document_id.is_some());
    }

    #[test]
    fn print_intent_guarded_edits_preserve_geometry_repeats_and_namespace() {
        let mut manager = SketchManager::new();
        let mut bodies = Vec::new();
        let (first, _) = import(&mut manager, &mut bodies);
        let (second, _) = import(&mut manager, &mut bodies);
        let occurrence = manager.assembly.component_structure.occurrences[0].clone();
        manager
            .duplicate_occurrence(DuplicateOccurrenceRequestDto {
                occurrence_id: occurrence.id,
                parent_occurrence_id: None,
                local_pose: None,
            })
            .unwrap();
        let before = manager.export_project_model().unwrap();
        let original_geometry = geometry(&manager);
        assert_eq!(manager.print_intent.source_document_id, None);
        response(&mut manager, "print_intent_get", json!({}));
        response(&mut manager, "print_intent_effective", json!({}));
        assert_eq!(manager.export_project_model().unwrap(), before);
        let settings = PrintSettingsDto {
            wall_count: Some(6),
            infill_density_percent: Some(40.),
            infill_pattern: Some(InfillPatternDto::Gyroid),
            top_shell_layers: Some(6),
            bottom_shell_layers: Some(6),
        };
        response(
            &mut manager,
            "print_intent_set_part",
            json!({"body_id":first,"settings":settings,"expected_model_json":before}),
        );
        let document = manager.print_intent();
        let namespace = document.source_document_id.clone().unwrap();
        assert_eq!(namespace.len(), 36);
        assert_eq!(geometry(&manager), original_geometry);
        assert_eq!(
            manager
                .assembly_solution()
                .instance_body_poses
                .iter()
                .filter(|pose| pose.body_id == first)
                .count(),
            2
        );
        assert_eq!(
            manager
                .effective_print_intent(vec![first], None)
                .unwrap()
                .parts[0]
                .settings
                .wall_count,
            Some(6)
        );
        let after = manager.export_project_model().unwrap();
        let failed: Value = serde_json::from_str(&host::handle(
            &mut manager,
            "print_intent_reset_part",
            &json!({"body_id":first,"expected_model_json":before}).to_string(),
        ))
        .unwrap();
        assert_eq!(failed["ok"], false);
        assert_eq!(manager.export_project_model().unwrap(), after);
        let missing: Value = serde_json::from_str(&host::handle(
            &mut manager,
            "print_intent_set_part",
            &json!({"body_id":first,"settings":{}}).to_string(),
        ))
        .unwrap();
        assert_eq!(missing["ok"], false);
        assert_eq!(manager.export_project_model().unwrap(), after);
        assert!(manager
            .copy_part_print_intent(first, vec![second, BodyId(999)])
            .is_err());
        assert_eq!(manager.export_project_model().unwrap(), after);
        response(
            &mut manager,
            "print_intent_copy_part",
            json!({"source_body_id":first,"target_body_ids":[second],"expected_model_json":after}),
        );
        assert_eq!(
            manager.print_intent.parts[1].settings,
            document.parts[0].settings
        );
        assert_eq!(geometry(&manager), original_geometry);
        let mut replacement = manager.print_intent();
        replacement.source_document_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(manager.set_print_intent_document(replacement).is_err());
        manager.reset_part_print_intent(first).unwrap();
        assert_eq!(manager.print_intent.source_document_id, Some(namespace));
        assert_eq!(geometry(&manager), original_geometry);
    }

    #[test]
    fn print_intent_unknown_part_edits_cannot_poison_identity_allocation() {
        let mut manager = SketchManager::new();
        let mut bodies = Vec::new();
        let (body, _) = import(&mut manager, &mut bodies);
        let before = manager.export_project_model().unwrap();
        for unknown in [900, 9_007_199_254_740_991] {
            let mut document = manager.print_intent();
            document.parts.push(PartPrintIntentDto {
                body_id: BodyId(unknown),
                settings: Default::default(),
            });
            assert!(manager.set_print_intent_document(document).is_err());
            assert_eq!(manager.export_project_model().unwrap(), before);
            assert!(manager.print_intent.source_document_id.is_none());
        }
        let (next, _) = import(&mut manager, &mut bodies);
        assert_eq!(next.0, body.0 + 1);
    }

    #[test]
    fn print_intent_preserves_orphans_without_reusing_their_ids_after_reload() {
        let mut manager = SketchManager::new();
        let mut bodies = Vec::new();
        let (first, _) = import(&mut manager, &mut bodies);
        let (second, creator) = import(&mut manager, &mut bodies);
        manager
            .set_part_print_intent(
                second,
                PrintSettingsDto {
                    wall_count: Some(8),
                    ..Default::default()
                },
            )
            .unwrap();
        let intent = manager.print_intent();
        let plan = manager
            .prepare_set_rollback(SetRollbackRequest { rollback_index: 0 })
            .unwrap();
        commit(&mut manager, plan, &[]);
        let report = manager.effective_print_intent(vec![second], None).unwrap();
        assert_eq!(report.parts[0].binding, PrintPartBindingDto::Retained);
        assert_eq!(report.parts[0].settings.wall_count, Some(8));
        assert!(report.orphan_body_ids.is_empty());
        let plan = manager
            .prepare_set_rollback(SetRollbackRequest { rollback_index: 2 })
            .unwrap();
        commit(&mut manager, plan, &bodies);
        let plan = manager
            .prepare_delete_feature(DeleteFeatureRequest {
                feature_id: creator,
            })
            .unwrap();
        bodies.retain(|body| body.body_id != second);
        commit(&mut manager, plan, &bodies);
        let report = manager.effective_print_intent(Vec::new(), None).unwrap();
        assert_eq!(report.orphan_body_ids, vec![second]);
        let orphan = report
            .parts
            .iter()
            .find(|part| part.body_id == second)
            .unwrap();
        assert_eq!(orphan.binding, PrintPartBindingDto::Orphan);
        assert_eq!(orphan.requested.wall_count, Some(8));
        assert_eq!(orphan.settings.wall_count, None);
        let saved = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let plan = loaded.prepare_load_project(saved).unwrap();
        commit(&mut loaded, plan, &bodies);
        assert_eq!(loaded.print_intent(), intent);
        loaded.set_print_intent_document(intent.clone()).unwrap();
        let (replacement, _) = import(&mut loaded, &mut bodies);
        assert!(replacement.0 > second.0);
        assert_eq!(
            loaded
                .effective_print_intent(vec![replacement], None)
                .unwrap()
                .parts[0]
                .settings
                .wall_count,
            None
        );
        assert_eq!(
            loaded
                .effective_print_intent(vec![first], None)
                .unwrap()
                .parts[0]
                .binding,
            PrintPartBindingDto::Live
        );
    }

    #[test]
    fn print_intent_schema_ten_migrates_without_settings_and_presets_are_atomic() {
        let mut manager = SketchManager::new();
        let mut old: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        old["schema_version"] = json!(10);
        old.as_object_mut().unwrap().remove("print_intent");
        let plan = manager.prepare_load_project(old.to_string()).unwrap();
        commit(&mut manager, plan, &[]);
        assert_eq!(manager.print_intent(), PrintIntentDocumentDto::default());
        let original = geometry(&manager);
        manager
            .upsert_print_intent_preset(PrintIntentPresetDto {
                name: "Strong requested shell".into(),
                settings: PrintSettingsDto {
                    wall_count: Some(6),
                    ..Default::default()
                },
            })
            .unwrap();
        let namespace = manager.print_intent.source_document_id.clone();
        let before = manager.export_project_model().unwrap();
        assert!(manager
            .upsert_print_intent_preset(PrintIntentPresetDto {
                name: "Invalid".into(),
                settings: PrintSettingsDto {
                    infill_density_percent: Some(101.),
                    ..Default::default()
                }
            })
            .is_err());
        assert_eq!(manager.export_project_model().unwrap(), before);
        manager
            .remove_print_intent_preset("Strong requested shell")
            .unwrap();
        assert_eq!(manager.print_intent.source_document_id, namespace);
        assert_eq!(geometry(&manager), original);
        let (settings, sources) = limo_cad_core::resolve_print_settings(
            &PrintIntentDocumentDto {
                defaults: PrintSettingsDto {
                    wall_count: Some(2),
                    ..Default::default()
                },
                ..Default::default()
            },
            &PrintSettingsDto {
                wall_count: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(settings.wall_count, Some(0));
        assert_eq!(sources.wall_count, Some(PrintSettingSourceDto::Part));
    }

    #[test]
    fn print_intent_handoffs_roundtrip_guard_namespace_and_reserve_orphan_identities() {
        let mut manager = SketchManager::new();
        let mut bodies = Vec::new();
        let (body, _) = import(&mut manager, &mut bodies);
        manager.assembly.component_structure.next_occurrence_id = 901;
        let (second, creator) = import(&mut manager, &mut bodies);
        manager
            .upsert_print_intent_preset(PrintIntentPresetDto {
                name: "Assign source identity".into(),
                settings: Default::default(),
            })
            .unwrap();
        let namespace = manager.print_intent.source_document_id.clone().unwrap();
        let defaults = json!({"wall_loops":"2","sparse_infill_density":"15%","sparse_infill_pattern":"gyroid","top_shell_layers":"5","bottom_shell_layers":"3"});
        let handoff = json!({"kind":"bambu_studio","name":"Production","source_label":"private template.3mf","reference":{
            "version":1,"source_document_id":namespace,"original_template_sha256":"a".repeat(64),"profile_sha256":"b".repeat(64),"profile_identity_sha256":"c".repeat(64),
            "baseline_project_settings":defaults,"written_project_settings":defaults,
            "parts":[{"binding":{"body_id":body.0,"occurrence_id":1,"object_id":1,"instance_id":0,"part_id":2},"target_uuid":"volume","instance_identify_id":1,"baseline_part_settings":{},"written_part_settings":{}},
            {"binding":{"body_id":second.0,"occurrence_id":901,"object_id":1,"instance_id":1,"part_id":2},"target_uuid":"volume","instance_identify_id":2,"baseline_part_settings":{},"written_part_settings":{}}]}});
        let before = manager.export_project_model().unwrap();
        let mut unknown = manager.print_intent();
        unknown.parts.push(PartPrintIntentDto {
            body_id: BodyId(900),
            settings: Default::default(),
        });
        assert!(manager.set_print_intent_document(unknown).is_err());
        assert_eq!(manager.export_project_model().unwrap(), before);
        let mut unknown: PrintTargetHandoffDto = serde_json::from_value(handoff.clone()).unwrap();
        let PrintTargetHandoffDto::BambuStudio { reference, .. } = &mut unknown;
        reference.parts[0].binding.occurrence_id = 902;
        assert!(manager.upsert_print_intent_handoff(unknown).is_err());
        assert_eq!(manager.export_project_model().unwrap(), before);
        response(
            &mut manager,
            "print_intent_upsert_handoff",
            json!({"handoff":handoff,"expected_model_json":before}),
        );
        let saved = manager.export_project_model().unwrap();
        let stale: Value = serde_json::from_str(&host::handle(
            &mut manager,
            "print_intent_remove_handoff",
            &json!({"name":"Production","expected_model_json":before}).to_string(),
        ))
        .unwrap();
        assert_eq!(stale["ok"], false);
        assert_eq!(manager.export_project_model().unwrap(), saved);
        assert!(manager.assembly.component_structure.next_occurrence_id > 901);
        let mut replacement = manager.print_intent();
        let limo_cad_core::PrintTargetHandoffDto::BambuStudio { reference, .. } =
            &mut replacement.target_handoffs[0];
        reference.source_document_id = "22222222-2222-4222-8222-222222222222".into();
        assert!(manager.set_print_intent_document(replacement).is_err());
        assert_eq!(manager.export_project_model().unwrap(), saved);
        let mut loaded = SketchManager::new();
        let plan = manager
            .prepare_delete_feature(DeleteFeatureRequest {
                feature_id: creator,
            })
            .unwrap();
        bodies.retain(|body| body.body_id != second);
        commit(&mut manager, plan, &bodies);
        let mut saved: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        saved["assembly"]["component_structure"]["next_occurrence_id"] = 2.into();
        let plan = loaded.prepare_load_project(saved.to_string()).unwrap();
        commit(&mut loaded, plan, &bodies);
        assert_eq!(loaded.print_intent(), manager.print_intent());
        let (new_body, _) = import(&mut loaded, &mut bodies);
        assert!(new_body.0 > second.0);
        assert!(
            loaded
                .assembly
                .component_structure
                .occurrences
                .last()
                .unwrap()
                .id
                .0
                > 901
        );
        let loaded_model = loaded.export_project_model().unwrap();
        response(
            &mut loaded,
            "print_intent_remove_handoff",
            json!({"name":"Production","expected_model_json":loaded_model}),
        );
        assert!(loaded.print_intent.target_handoffs.is_empty());
        assert_eq!(
            loaded.print_intent.source_document_id,
            manager.print_intent.source_document_id
        );
    }

    #[test]
    fn print_intent_schema_eleven_migrates_without_losing_settings_and_rejects_future_versions() {
        let mut manager = SketchManager::new();
        manager
            .upsert_print_intent_preset(PrintIntentPresetDto {
                name: "Retained preset".into(),
                settings: PrintSettingsDto {
                    wall_count: Some(0),
                    ..Default::default()
                },
            })
            .unwrap();
        let mut old: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        old["schema_version"] = 11.into();
        old["print_intent"]["version"] = 1.into();
        old["print_intent"]
            .as_object_mut()
            .unwrap()
            .remove("modifiers");
        old["print_intent"]
            .as_object_mut()
            .unwrap()
            .remove("height_ranges");
        old["print_intent"]
            .as_object_mut()
            .unwrap()
            .remove("layer_height_profiles");
        old["print_intent"]
            .as_object_mut()
            .unwrap()
            .remove("target_handoffs");
        let mut loaded = SketchManager::new();
        let plan = loaded.prepare_load_project(old.to_string()).unwrap();
        commit(&mut loaded, plan, &[]);
        assert_eq!(loaded.print_intent(), manager.print_intent());
        let before = loaded.export_project_model().unwrap();
        let mut future: Value = serde_json::from_str(&before).unwrap();
        future["print_intent"]["version"] = 5.into();
        assert!(loaded.prepare_load_project(future.to_string()).is_err());
        assert_eq!(loaded.export_project_model().unwrap(), before);
        old["print_intent"]["target_handoffs"] = json!([]);
        assert!(loaded.prepare_load_project(old.to_string()).is_err());
        assert_eq!(loaded.export_project_model().unwrap(), before);
    }
}
