use super::*;
use limo_cad_core::{PrintIntentDocumentDto, PrintModifierDto, PrintSettingsDto};

impl SketchManager {
    pub fn create_print_modifier(
        &mut self,
        mut modifier: PrintModifierDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        self.require_print_part(modifier.body_id)?;
        modifier.validate().map_err(SessionError::Solid)?;
        modifier.id.make_ascii_lowercase();
        let mut document = self.print_intent();
        if document
            .modifiers
            .iter()
            .any(|old| old.id.eq_ignore_ascii_case(&modifier.id))
        {
            return Err(SessionError::Solid(
                "Print modifier identity already exists".into(),
            ));
        }
        document.modifiers.push(modifier);
        self.set_print_intent_document(document)
    }

    pub fn update_print_modifier(
        &mut self,
        mut modifier: PrintModifierDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        modifier.validate().map_err(SessionError::Solid)?;
        modifier.id.make_ascii_lowercase();
        let mut document = self.print_intent();
        let entry = document
            .modifiers
            .iter_mut()
            .find(|old| old.id.eq_ignore_ascii_case(&modifier.id))
            .ok_or_else(|| SessionError::Solid("Print modifier was not found".into()))?;
        if entry.body_id != modifier.body_id {
            return Err(SessionError::Solid(
                "Modifier attachment cannot change; copy it explicitly to another source body"
                    .into(),
            ));
        }
        *entry = modifier;
        self.set_print_intent_document(document)
    }

    pub fn remove_print_modifier(
        &mut self,
        id: &str,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        let old_len = document.modifiers.len();
        document
            .modifiers
            .retain(|entry| !entry.id.eq_ignore_ascii_case(id));
        if old_len == document.modifiers.len() {
            return Err(SessionError::Solid("Print modifier was not found".into()));
        }
        self.set_print_intent_document(document)
    }

    /// A reset inherits settings while keeping the explicit print-only zone and its attachment.
    pub fn reset_print_modifier(
        &mut self,
        id: &str,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        let entry = document
            .modifiers
            .iter_mut()
            .find(|entry| entry.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| SessionError::Solid("Print modifier was not found".into()))?;
        entry.settings = PrintSettingsDto::default();
        self.set_print_intent_document(document)
    }

    /// Copy preserves local coordinates; its new stable identity never allocates a mechanical body.
    pub fn copy_print_modifier(
        &mut self,
        source_id: &str,
        target_body_id: BodyId,
        name: Option<String>,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        self.require_print_part(target_body_id)?;
        let mut modifier = self
            .print_intent
            .modifiers
            .iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(source_id))
            .cloned()
            .ok_or_else(|| SessionError::Solid("Print modifier was not found".into()))?;
        modifier.id = uuid::Uuid::new_v4().to_string();
        modifier.body_id = target_body_id;
        if let Some(name) = name {
            modifier.name = name;
        }
        self.create_print_modifier(modifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host;
    use limo_cad_core::{
        PrintIntentTargetDto, PrintLocalPoseDto, PrintModifierPrimitiveDto, PrintPartBindingDto,
        PrintSettingSourceDto,
    };
    use limo_cad_solid::{ImportStepRequest, KernelBodyDto, KernelJobDto, KernelSceneDto};
    use serde_json::{json, Value};

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
                file_name: "zone-parent.step".into(),
                data_base64: "U1RFUA==".into(),
            }))
            .unwrap();
        let KernelJobDto::ImportStep(job) = plan.jobs.last().unwrap() else {
            panic!("import")
        };
        let pair = (job.result_body_id, job.feature_id);
        bodies.push(KernelBodyDto {
            body_id: pair.0,
            topology_signature: String::new(),
            display_warnings: Vec::new(),
            positions: vec![0., 0., 0., 10., 0., 0., 0., 10., 0.],
            normals: [0., 0., 1.].repeat(3),
            indices: vec![0, 1, 2],
            faces: Vec::new(),
            edges: Vec::new(),
        });
        commit(manager, plan, bodies);
        pair
    }
    fn zone(body_id: BodyId) -> PrintModifierDto {
        PrintModifierDto {
            id: "01234567-89ab-4cde-8123-456789abcdef".into(),
            name: "Boss zone".into(),
            body_id,
            enabled: true,
            local_pose: PrintLocalPoseDto {
                translation_mm: [5., 5., 5.],
                ..Default::default()
            },
            primitive: PrintModifierPrimitiveDto::Box {
                size_mm: [3., 4., 5.],
            },
            settings: PrintSettingsDto {
                wall_count: Some(6),
                ..Default::default()
            },
        }
    }
    fn call(manager: &mut SketchManager, method: &str, payload: Value) -> Value {
        serde_json::from_str(&host::handle(manager, method, &payload.to_string())).unwrap()
    }
    fn mechanical(manager: &SketchManager) -> Value {
        let mut value: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("print_intent");
        value
    }

    #[test]
    fn print_modifier_guarded_crud_repeats_reset_and_snapshot_history_preserve_mechanics() {
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
        manager
            .set_part_print_intent(
                first,
                PrintSettingsDto {
                    infill_density_percent: Some(30.),
                    ..Default::default()
                },
            )
            .unwrap();
        let before = manager.export_project_model().unwrap();
        let physical = mechanical(&manager);
        let modifier = zone(first);
        assert_eq!(
            call(
                &mut manager,
                "print_modifier_create",
                json!({"modifier":modifier,"expected_model_json":before})
            )["ok"],
            true
        );
        let created = manager.export_project_model().unwrap();
        assert_eq!(mechanical(&manager), physical);
        let effective = manager
            .effective_print_intent(vec![first], Some(PrintIntentTargetDto::BambuStudio))
            .unwrap();
        assert_eq!(effective.modifiers[0].occurrence_ids.len(), 2);
        assert_eq!(effective.modifiers[0].settings.wall_count, Some(6));
        assert_eq!(
            effective.modifiers[0].sources.wall_count,
            Some(PrintSettingSourceDto::Modifier)
        );
        assert_eq!(
            effective.modifiers[0].settings.infill_density_percent,
            Some(30.)
        );
        assert_eq!(
            effective.modifiers[0].sources.infill_density_percent,
            Some(PrintSettingSourceDto::Part)
        );
        assert_eq!(effective.modifiers[0].binding, PrintPartBindingDto::Live);
        let portable = manager
            .effective_print_intent(vec![first], Some(PrintIntentTargetDto::Portable))
            .unwrap();
        assert_eq!(portable.modifiers[0].unsupported.len(), 1);
        assert!(portable.modifiers[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("omitted")));
        assert_eq!(
            call(
                &mut manager,
                "print_modifier_reset",
                json!({"id":modifier.id,"expected_model_json":before})
            )["ok"],
            false
        );
        let mut rebound = modifier.clone();
        rebound.body_id = second;
        assert_eq!(
            call(
                &mut manager,
                "print_modifier_update",
                json!({"modifier":rebound,"expected_model_json":created})
            )["ok"],
            false
        );
        let mut document = manager.print_intent();
        document.modifiers[0] = rebound;
        assert!(manager.set_print_intent_document(document).is_err());
        assert_eq!(manager.export_project_model().unwrap(), created);
        assert_eq!(
            call(
                &mut manager,
                "print_modifier_copy",
                json!({"source_id":modifier.id,"target_body_id":second,"name":"Copied zone","expected_model_json":created})
            )["ok"],
            true
        );
        let copied = manager
            .print_intent()
            .modifiers
            .iter()
            .find(|entry| entry.body_id == second)
            .unwrap()
            .clone();
        assert_ne!(copied.id, modifier.id);
        assert_eq!(copied.local_pose, modifier.local_pose);
        manager.reset_print_modifier(&modifier.id).unwrap();
        let reset = manager
            .print_intent
            .modifiers
            .iter()
            .find(|entry| entry.id == modifier.id)
            .unwrap();
        assert_eq!(reset.settings, Default::default());
        assert_eq!(reset.primitive, modifier.primitive);
        assert_eq!(reset.local_pose, modifier.local_pose);
        assert_eq!(mechanical(&manager), physical);
        let after = manager.export_project_model().unwrap();
        // The owning history restores exact completed models; these round trips exercise
        // both directions with real retained bodies rather than cloning the DTO only.
        for snapshot in [&created, &after, &created, &after] {
            let plan = manager.prepare_load_project(snapshot.clone()).unwrap();
            commit(&mut manager, plan, &bodies);
            assert_eq!(&manager.export_project_model().unwrap(), snapshot);
            assert_eq!(mechanical(&manager), physical);
        }
        manager.remove_print_modifier(&copied.id).unwrap();
        assert_eq!(manager.print_intent.modifiers.len(), 1);
    }

    #[test]
    fn print_modifier_orphans_are_reported_reserved_and_cannot_be_novel_attachments() {
        let mut manager = SketchManager::new();
        let mut bodies = Vec::new();
        let (first, _) = import(&mut manager, &mut bodies);
        let (second, creator) = import(&mut manager, &mut bodies);
        manager.create_print_modifier(zone(second)).unwrap();
        let plan = manager
            .prepare_delete_feature(DeleteFeatureRequest {
                feature_id: creator,
            })
            .unwrap();
        bodies.retain(|body| body.body_id != second);
        commit(&mut manager, plan, &bodies);
        let report = manager.effective_print_intent(Vec::new(), None).unwrap();
        assert_eq!(report.modifiers[0].binding, PrintPartBindingDto::Orphan);
        assert_eq!(report.modifiers[0].settings, Default::default());
        assert!(report.modifiers[0].occurrence_ids.is_empty());
        let saved = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let plan = loaded.prepare_load_project(saved.clone()).unwrap();
        commit(&mut loaded, plan, &bodies);
        let (third, _) = import(&mut loaded, &mut bodies);
        assert!(third.0 > second.0);
        assert_eq!(loaded.print_intent.modifiers[0].body_id, second);
        let mut unknown = zone(BodyId(999));
        unknown.id = uuid::Uuid::new_v4().to_string();
        assert!(loaded.create_print_modifier(unknown.clone()).is_err());
        let mut document = loaded.print_intent();
        document.modifiers.push(unknown);
        assert!(loaded.set_print_intent_document(document).is_err());
        assert!(loaded
            .copy_print_modifier(&zone(second).id, BodyId(999), None)
            .is_err());
        loaded
            .copy_print_modifier(&zone(second).id, first, Some("Recovered zone".into()))
            .unwrap();
        assert_eq!(loaded.print_intent.modifiers.len(), 2);
        loaded.reset_print_modifier(&zone(second).id).unwrap();
        assert_eq!(
            loaded
                .print_intent
                .modifiers
                .iter()
                .find(|entry| entry.body_id == second)
                .unwrap()
                .settings,
            Default::default()
        );
    }

    #[test]
    fn print_modifier_schema_twelve_migrates_and_pending_load_rejects_edits_atomically() {
        let mut manager = SketchManager::new();
        let mut bodies = Vec::new();
        let (body, _) = import(&mut manager, &mut bodies);
        manager
            .upsert_print_intent_preset(limo_cad_core::PrintIntentPresetDto {
                name: "Preserved process request".into(),
                settings: PrintSettingsDto {
                    wall_count: Some(0),
                    ..Default::default()
                },
            })
            .unwrap();
        let original = manager.export_project_model().unwrap();
        let mut old: Value = serde_json::from_str(&original).unwrap();
        old["schema_version"] = 12.into();
        old["print_intent"]["version"] = 2.into();
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
        let mut loaded = SketchManager::new();
        let plan = loaded.prepare_load_project(old.to_string()).unwrap();
        commit(&mut loaded, plan, &bodies);
        assert_eq!(loaded.export_project_model().unwrap(), original);
        let plan = loaded.prepare_load_project(original.clone()).unwrap();
        assert!(loaded.create_print_modifier(zone(body)).is_err());
        assert_eq!(loaded.export_project_model().unwrap(), original);
        commit(&mut loaded, plan, &bodies);
        assert!(loaded.print_intent.modifiers.is_empty());
        old["print_intent"]["modifiers"] = json!([zone(body)]);
        assert!(loaded.prepare_load_project(old.to_string()).is_err());
        assert_eq!(loaded.export_project_model().unwrap(), original);
        loaded.create_print_modifier(zone(body)).unwrap();
        let saved = loaded.export_project_model().unwrap();
        let mut future: Value = serde_json::from_str(&saved).unwrap();
        future["print_intent"]["version"] = 5.into();
        assert!(loaded.prepare_load_project(future.to_string()).is_err());
        assert_eq!(loaded.export_project_model().unwrap(), saved);
    }
}
