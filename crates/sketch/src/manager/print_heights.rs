//! Height intent uses the same hierarchy and layout resolver as display and export.

use super::*;
use limo_cad_core::{
    PrintHeightBindingDto, PrintHeightLayoutDto, PrintHeightOccurrenceDto,
    PrintHeightRangeDraftDto, PrintHeightRangeDto, PrintIntentDocumentDto,
    PrintLayerHeightProfileDraftDto, PrintLayerHeightProfileDto, PrintLocalPoseDto,
    PrintSettingsDto,
};

impl SketchManager {
    /// Check persistent layout identity while the owning host still holds its model lock.
    pub fn validate_print_height_export_view(
        &self,
        name: Option<&str>,
    ) -> Result<(), SessionError> {
        if !self
            .print_intent
            .height_ranges
            .iter()
            .any(|range| range.enabled)
            && !self
                .print_intent
                .layer_height_profiles
                .iter()
                .any(|profile| profile.enabled)
        {
            return Ok(());
        }
        let selected_name = name
            .or(self.active_named_view.as_deref())
            .filter(|name| !name.is_empty());
        let selected = match selected_name {
            None => PrintHeightLayoutDto::Assembly,
            Some(name) => {
                let view = self
                    .named_views()
                    .views
                    .into_iter()
                    .find(|view| view.name == name)
                    .ok_or_else(|| {
                        SessionError::Solid("Selected height layout was removed".into())
                    })?;
                PrintHeightLayoutDto::NamedLayout { id: view.id.ok_or_else(|| SessionError::Solid("Save the selected legacy layout before binding/exporting height intent".into()))? }
            }
        };
        for layout in self
            .print_intent
            .height_ranges
            .iter()
            .filter(|range| range.enabled)
            .map(|range| &range.binding.layout)
            .chain(
                self.print_intent
                    .layer_height_profiles
                    .iter()
                    .filter(|profile| profile.enabled)
                    .map(|profile| &profile.binding.layout),
            )
        {
            if layout != &selected {
                return Err(SessionError::Solid("Enabled height intent is bound to another assembled/saved layout; select its layout or explicitly review and rebind the intent".into()));
            }
        }
        Ok(())
    }
    pub(super) fn effective_print_heights(
        &self,
        body_ids: &BTreeSet<BodyId>,
        target: Option<limo_cad_core::PrintIntentTargetDto>,
    ) -> (
        Vec<limo_cad_core::PrintHeightRangeEffectiveDto>,
        Vec<limo_cad_core::PrintLayerHeightProfileEffectiveDto>,
    ) {
        use limo_cad_core::{configured_print_fields, PrintHeightSpeedFieldDto};
        let live: BTreeSet<_> = self
            .solid_scene()
            .bodies
            .iter()
            .map(|body| body.id)
            .collect();
        let retained = self.retained_presentation_body_ids();
        let source_binding = |body_id| {
            if live.contains(&body_id) {
                limo_cad_core::PrintPartBindingDto::Live
            } else if retained.contains(&body_id) {
                limo_cad_core::PrintPartBindingDto::Retained
            } else {
                limo_cad_core::PrintPartBindingDto::Orphan
            }
        };
        let caps = target.map(|target| {
            limo_cad_core::print_setting_capabilities(
                target,
                limo_cad_core::PrintIntentScopeDto::HeightRange,
            )
        });
        let ranges = self
            .print_intent
            .height_ranges
            .iter()
            .filter(|range| body_ids.contains(&range.body_id))
            .map(|range| {
                let (binding_current, mut issues) =
                    self.print_height_binding_status(range.body_id, &range.binding);
                if !range.enabled {
                    issues.push("Height range is disabled and omitted from export".into());
                }
                let part = self
                    .print_intent
                    .parts
                    .iter()
                    .find(|part| part.body_id == range.body_id)
                    .map(|part| part.settings.clone())
                    .unwrap_or_default();
                let binding = source_binding(range.body_id);
                let (settings, sources) = if binding == limo_cad_core::PrintPartBindingDto::Orphan {
                    (PrintSettingsDto::default(), Default::default())
                } else {
                    limo_cad_core::resolve_print_height_settings(
                        &self.print_intent,
                        &part,
                        &range.settings,
                    )
                };
                let configured = configured_print_fields(&range.settings);
                let unsupported = caps
                    .iter()
                    .flatten()
                    .filter(|cap| !cap.supported && configured.contains(&cap.field))
                    .map(|cap| cap.field)
                    .collect();
                let unsupported_speeds = [
                    (
                        range.speeds.outer_wall_mm_s.is_some(),
                        PrintHeightSpeedFieldDto::OuterWallMmS,
                    ),
                    (
                        range.speeds.inner_wall_mm_s.is_some(),
                        PrintHeightSpeedFieldDto::InnerWallMmS,
                    ),
                    (
                        range.speeds.infill_mm_s.is_some(),
                        PrintHeightSpeedFieldDto::InfillMmS,
                    ),
                ]
                .into_iter()
                .filter_map(|(set, field)| {
                    (set && !target.is_some_and(limo_cad_core::print_height_target_supported))
                        .then_some(field)
                })
                .collect();
                limo_cad_core::PrintHeightRangeEffectiveDto {
                    range: range.clone(),
                    binding,
                    binding_current,
                    issues,
                    settings,
                    sources,
                    unsupported,
                    unsupported_speeds,
                }
            })
            .collect();
        let profiles = self
            .print_intent
            .layer_height_profiles
            .iter()
            .filter(|profile| body_ids.contains(&profile.body_id))
            .map(|profile| {
                let (binding_current, mut issues) =
                    self.print_height_binding_status(profile.body_id, &profile.binding);
                if !profile.enabled {
                    issues
                        .push("Variable layer profile is disabled and omitted from export".into());
                }
                limo_cad_core::PrintLayerHeightProfileEffectiveDto {
                    profile: profile.clone(),
                    binding: source_binding(profile.body_id),
                    binding_current,
                    issues,
                    target_supported: target
                        .is_some_and(limo_cad_core::print_height_target_supported),
                }
            })
            .collect();
        (ranges, profiles)
    }

    fn print_height_binding_status(
        &self,
        body_id: BodyId,
        binding: &PrintHeightBindingDto,
    ) -> (bool, Vec<String>) {
        match self.print_height_binding(body_id, binding.layout.clone()) {
            Ok(current) if same_height_binding(&current, binding) => (true, Vec::new()),
            Ok(_) => (false, vec!["Resolved print orientation, placement, visibility, group membership or bounds changed; explicitly review and rebind height intent".into()]),
            Err(error) => (false, vec![error.to_string()]),
        }
    }

    fn print_height_view_name(
        &self,
        layout: &PrintHeightLayoutDto,
    ) -> Result<String, SessionError> {
        match layout {
            PrintHeightLayoutDto::Assembly => Ok(String::new()),
            PrintHeightLayoutDto::NamedLayout { id } => self
                .named_views()
                .views
                .into_iter()
                .find(|view| view.id.as_ref() == Some(id))
                .map(|view| view.name)
                .ok_or_else(|| {
                    SessionError::Solid(
                        "The bound saved layout was removed; select a layout and explicitly rebind"
                            .into(),
                    )
                }),
        }
    }

    /// Capture all visible intentional repeats and their multipart group bounds without mutation.
    pub fn print_height_binding(
        &self,
        body_id: BodyId,
        layout: PrintHeightLayoutDto,
    ) -> Result<PrintHeightBindingDto, SessionError> {
        self.ensure_no_active_sketch("binding print height intent")?;
        self.solids
            .ensure_metadata_editable()
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        self.require_print_part(body_id)?;
        let name = self.print_height_view_name(&layout)?;
        let solution = self.export_view_solution(Some(&name))?;
        if !solution.solved {
            return Err(SessionError::Solid(
                "Resolve assembly errors before binding print heights".into(),
            ));
        }
        self.assembly
            .component_structure
            .validate()
            .map_err(SessionError::Solid)?;
        let roots = self
            .assembly
            .component_structure
            .occurrences
            .iter()
            .map(|occurrence| {
                let mut current = occurrence;
                while let Some(parent) = current.parent_occurrence_id {
                    // validate() established an acyclic, complete hierarchy above.
                    current = self
                        .assembly
                        .component_structure
                        .occurrences
                        .iter()
                        .find(|row| row.id == parent)
                        .expect("validated occurrence parent");
                }
                (occurrence.id, current.id)
            })
            .collect::<std::collections::HashMap<_, _>>();
        let scene = self.solid_scene();
        let meshes = scene
            .bodies
            .iter()
            .map(|body| (body.id, &body.mesh))
            .collect::<std::collections::HashMap<_, _>>();
        let mut group_bounds = std::collections::HashMap::<_, [f64; 2]>::new();
        for pose in solution
            .instance_body_poses
            .iter()
            .filter(|pose| pose.visible)
        {
            let root = roots.get(&pose.occurrence_id).ok_or_else(|| {
                SessionError::Solid("Resolved print pose omitted its hierarchy occurrence".into())
            })?;
            let mesh = meshes.get(&pose.body_id).ok_or_else(|| {
                SessionError::Solid("Resolved print group contains a missing source mesh".into())
            })?;
            if mesh.positions.len() % 3 != 0 {
                return Err(SessionError::Solid(
                    "Resolved print mesh has incomplete coordinates".into(),
                ));
            }
            let transform = limo_cad_assembly::AssemblyTransformDto {
                translation: pose.translation,
                rotation: pose.rotation,
            };
            let bounds = group_bounds
                .entry(*root)
                .or_insert([f64::INFINITY, f64::NEG_INFINITY]);
            for point in mesh.positions.as_chunks::<3>().0 {
                let z =
                    transform.transform_point([point[0] as f64, point[1] as f64, point[2] as f64])
                        [2];
                if !z.is_finite() {
                    return Err(SessionError::Solid(
                        "Resolved print group has nonfinite mesh coordinates".into(),
                    ));
                }
                bounds[0] = bounds[0].min(z);
                bounds[1] = bounds[1].max(z);
            }
        }
        let mut occurrences = Vec::new();
        for pose in solution
            .instance_body_poses
            .iter()
            .filter(|pose| pose.visible && pose.body_id == body_id)
        {
            let root = roots[&pose.occurrence_id];
            let bounds = group_bounds[&root];
            occurrences.push(PrintHeightOccurrenceDto {
                body_id,
                occurrence_id: pose.occurrence_id.0,
                root_occurrence_id: root.0,
                pose: PrintLocalPoseDto {
                    translation_mm: pose.translation,
                    rotation: pose.rotation,
                },
                min_z_mm: bounds[0],
                max_z_mm: bounds[1],
            });
        }
        occurrences.sort_by_key(|occurrence| occurrence.occurrence_id);
        let wanted_roots: BTreeSet<_> = occurrences
            .iter()
            .map(|occurrence| occurrence.root_occurrence_id)
            .collect();
        let groups = wanted_roots
            .into_iter()
            .map(|root| {
                let id = limo_cad_assembly::OccurrenceId(root);
                let bounds = group_bounds[&id];
                let mut members: Vec<_> = solution
                    .instance_body_poses
                    .iter()
                    .filter(|pose| pose.visible && roots[&pose.occurrence_id] == id)
                    .map(|pose| limo_cad_core::PrintSourceOccurrenceDto {
                        body_id: pose.body_id,
                        occurrence_id: pose.occurrence_id.0,
                    })
                    .collect();
                members.sort_unstable();
                limo_cad_core::PrintHeightGroupDto {
                    root_occurrence_id: root,
                    members,
                    min_z_mm: bounds[0],
                    max_z_mm: bounds[1],
                }
            })
            .collect();
        let binding = PrintHeightBindingDto {
            layout,
            occurrences,
            groups,
        };
        binding.validate(body_id).map_err(SessionError::Solid)?;
        Ok(binding)
    }

    pub fn upsert_print_height_range(
        &mut self,
        draft: PrintHeightRangeDraftDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        let old = draft
            .id
            .as_ref()
            .and_then(|id| document.height_ranges.iter().find(|range| &range.id == id));
        if draft.id.is_some() && old.is_none() {
            return Err(SessionError::Solid(
                "Print height range was not found; omit id to create one".into(),
            ));
        }
        let (id, binding) = if let Some(old) = old {
            if old.body_id != draft.body_id || old.binding.layout != draft.layout {
                return Err(SessionError::Solid("Height attachment/layout cannot change during an update; explicitly rebind its layout".into()));
            }
            (old.id.clone(), old.binding.clone())
        } else {
            (
                uuid::Uuid::new_v4().to_string(),
                self.print_height_binding(draft.body_id, draft.layout)?,
            )
        };
        let range = PrintHeightRangeDto {
            id,
            name: draft.name,
            body_id: draft.body_id,
            enabled: draft.enabled,
            coordinate: draft.coordinate,
            min_z_mm: draft.min_z_mm,
            max_z_mm: draft.max_z_mm,
            binding,
            settings: draft.settings,
            speeds: draft.speeds,
        };
        document.height_ranges.retain(|old| old.id != range.id);
        document.height_ranges.push(range);
        self.set_print_intent_document(document)
    }

    pub fn upsert_print_layer_profile(
        &mut self,
        draft: PrintLayerHeightProfileDraftDto,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        let old = draft.id.as_ref().and_then(|id| {
            document
                .layer_height_profiles
                .iter()
                .find(|profile| &profile.id == id)
        });
        if draft.id.is_some() && old.is_none() {
            return Err(SessionError::Solid(
                "Variable layer profile was not found; omit id to create one".into(),
            ));
        }
        let (id, binding) = if let Some(old) = old {
            if old.body_id != draft.body_id || old.binding.layout != draft.layout {
                return Err(SessionError::Solid("Variable profile attachment/layout cannot change during an update; explicitly rebind its layout".into()));
            }
            (old.id.clone(), old.binding.clone())
        } else {
            (
                uuid::Uuid::new_v4().to_string(),
                self.print_height_binding(draft.body_id, draft.layout)?,
            )
        };
        let profile = PrintLayerHeightProfileDto {
            id,
            name: draft.name,
            body_id: draft.body_id,
            enabled: draft.enabled,
            binding,
            points: draft.points,
        };
        document
            .layer_height_profiles
            .retain(|old| old.id != profile.id);
        document.layer_height_profiles.push(profile);
        self.set_print_intent_document(document)
    }

    pub fn remove_print_height(
        &mut self,
        id: &str,
    ) -> Result<PrintIntentDocumentDto, SessionError> {
        let mut document = self.print_intent();
        let old = document.height_ranges.len() + document.layer_height_profiles.len();
        document.height_ranges.retain(|range| range.id != id);
        document
            .layer_height_profiles
            .retain(|profile| profile.id != id);
        if old == document.height_ranges.len() + document.layer_height_profiles.len() {
            return Err(SessionError::Solid(
                "Print height intent was not found".into(),
            ));
        }
        self.set_print_intent_document(document)
    }

    /// Review and deliberately replace placement evidence; interval values are never clamped.
    pub fn rebind_print_height(
        &mut self,
        id: &str,
        layout: PrintHeightLayoutDto,
        interval: Option<limo_cad_core::PrintHeightIntervalDto>,
        points: Option<Vec<limo_cad_core::PrintLayerHeightPointDto>>,
    ) -> Result<limo_cad_core::PrintHeightRebindReportDto, SessionError> {
        let mut document = self.print_intent();
        let (
            previous_binding,
            binding,
            previous_interval,
            new_interval,
            previous_points,
            new_points,
        ) = if let Some(range) = document
            .height_ranges
            .iter_mut()
            .find(|range| range.id == id)
        {
            if points.is_some() {
                return Err(SessionError::Solid(
                    "Layer samples cannot be supplied when rebinding a settings interval".into(),
                ));
            }
            let previous_binding = range.binding.clone();
            let previous_interval = limo_cad_core::PrintHeightIntervalDto {
                coordinate: range.coordinate,
                min_z_mm: range.min_z_mm,
                max_z_mm: range.max_z_mm,
            };
            range.binding = self.print_height_binding(range.body_id, layout)?;
            if let Some(interval) = interval {
                range.coordinate = interval.coordinate;
                range.min_z_mm = interval.min_z_mm;
                range.max_z_mm = interval.max_z_mm;
            }
            let new_interval = limo_cad_core::PrintHeightIntervalDto {
                coordinate: range.coordinate,
                min_z_mm: range.min_z_mm,
                max_z_mm: range.max_z_mm,
            };
            (
                previous_binding,
                range.binding.clone(),
                Some(previous_interval),
                Some(new_interval),
                None,
                None,
            )
        } else if let Some(profile) = document
            .layer_height_profiles
            .iter_mut()
            .find(|profile| profile.id == id)
        {
            if interval.is_some() {
                return Err(SessionError::Solid(
                    "An interval cannot be supplied when rebinding a variable layer profile".into(),
                ));
            }
            let previous_binding = profile.binding.clone();
            let previous_points = profile.points.clone();
            profile.binding = self.print_height_binding(profile.body_id, layout)?;
            if let Some(points) = points {
                profile.points = points;
            }
            (
                previous_binding,
                profile.binding.clone(),
                None,
                None,
                Some(previous_points),
                Some(profile.points.clone()),
            )
        } else {
            return Err(SessionError::Solid(
                "Print height intent was not found".into(),
            ));
        };
        let document = self.set_print_intent_document(document)?;
        Ok(limo_cad_core::PrintHeightRebindReportDto {
            document,
            previous_binding,
            binding,
            previous_interval,
            interval: new_interval,
            previous_points,
            points: new_points,
        })
    }

    pub(super) fn validate_height_document_edit(
        &self,
        document: &PrintIntentDocumentDto,
    ) -> Result<(), SessionError> {
        for range in &document.height_ranges {
            if self
                .print_intent
                .layer_height_profiles
                .iter()
                .any(|old| old.id == range.id)
            {
                return Err(SessionError::Solid("A variable-profile identity cannot be reused for a different height-intent kind".into()));
            }
            let previous = self
                .print_intent
                .height_ranges
                .iter()
                .find(|old| old.id == range.id);
            if let Some(previous) = previous {
                if previous.body_id != range.body_id {
                    return Err(SessionError::Solid(
                        "Height range attachment cannot be rebound to another source body".into(),
                    ));
                }
                if previous.binding == range.binding {
                    continue; // Retained stale/orphan intent stays readable and removable.
                }
            }
            let current = self.print_height_binding(range.body_id, range.binding.layout.clone())?;
            if !same_height_binding(&current, &range.binding) {
                return Err(SessionError::Solid(
                    "Height range binding must match the owning engine's resolved layout".into(),
                ));
            }
        }
        for profile in &document.layer_height_profiles {
            if self
                .print_intent
                .height_ranges
                .iter()
                .any(|old| old.id == profile.id)
            {
                return Err(SessionError::Solid(
                    "A height-range identity cannot be reused for a different height-intent kind"
                        .into(),
                ));
            }
            let previous = self
                .print_intent
                .layer_height_profiles
                .iter()
                .find(|old| old.id == profile.id);
            if let Some(previous) = previous {
                if previous.body_id != profile.body_id {
                    return Err(SessionError::Solid(
                        "Variable layer attachment cannot be rebound to another source body".into(),
                    ));
                }
                if previous.binding == profile.binding {
                    continue;
                }
            }
            let current =
                self.print_height_binding(profile.body_id, profile.binding.layout.clone())?;
            if !same_height_binding(&current, &profile.binding) {
                return Err(SessionError::Solid(
                    "Variable layer binding must match the owning engine's resolved layout".into(),
                ));
            }
        }
        Ok(())
    }
}

pub(super) fn same_height_binding(a: &PrintHeightBindingDto, b: &PrintHeightBindingDto) -> bool {
    if a.layout != b.layout
        || a.occurrences.len() != b.occurrences.len()
        || a.groups.len() != b.groups.len()
    {
        return false;
    }
    if a.groups.iter().any(|left| {
        !b.groups
            .iter()
            .find(|right| right.root_occurrence_id == left.root_occurrence_id)
            .is_some_and(|right| {
                left.members.len() == right.members.len()
                    && left
                        .members
                        .iter()
                        .all(|member| right.members.contains(member))
                    && (left.min_z_mm - right.min_z_mm).abs() <= 1e-6
                    && (left.max_z_mm - right.max_z_mm).abs() <= 1e-6
            })
    }) {
        return false;
    }
    a.occurrences.iter().all(|left| {
        b.occurrences
            .iter()
            .find(|right| right.occurrence_id == left.occurrence_id)
            .is_some_and(|right| {
                left.body_id == right.body_id
                    && left.root_occurrence_id == right.root_occurrence_id
                    && (left.min_z_mm - right.min_z_mm).abs() <= 1e-6
                    && (left.max_z_mm - right.max_z_mm).abs() <= 1e-6
                    && left
                        .pose
                        .translation_mm
                        .iter()
                        .zip(right.pose.translation_mm)
                        .all(|(l, r)| (l - r).abs() <= 1e-6)
                    && {
                        let same = left
                            .pose
                            .rotation
                            .iter()
                            .zip(right.pose.rotation)
                            .all(|(l, r)| (l - r).abs() <= 1e-8);
                        let opposite = left
                            .pose
                            .rotation
                            .iter()
                            .zip(right.pose.rotation)
                            .all(|(l, r)| (l + r).abs() <= 1e-8);
                        same || opposite
                    }
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host;
    use limo_cad_core::{PrintHeightCoordinateDto, PrintIntentTargetDto, PrintLayerHeightPointDto};
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
    fn fixture() -> (SketchManager, Vec<KernelBodyDto>, BodyId) {
        let mut manager = SketchManager::new();
        let plan = manager
            .prepare_body_feature(BodyFeatureRequestDto::ImportStep(ImportStepRequest {
                file_name: "height-source.step".into(),
                data_base64: "U1RFUA==".into(),
            }))
            .unwrap();
        let KernelJobDto::ImportStep(job) = plan.jobs.last().unwrap() else {
            panic!("import");
        };
        let body = job.result_body_id;
        let bodies = vec![KernelBodyDto {
            body_id: body,
            topology_signature: String::new(),
            display_warnings: Vec::new(),
            positions: vec![0., 0., 0., 10., 0., 0., 0., 10., 20.],
            normals: [0., 0., 1.].repeat(3),
            indices: vec![0, 1, 2],
            faces: Vec::new(),
            edges: Vec::new(),
        }];
        commit(&mut manager, plan, &bodies);
        let occurrence = manager.assembly.component_structure.occurrences[0].id;
        manager
            .duplicate_occurrence(DuplicateOccurrenceRequestDto {
                occurrence_id: occurrence,
                parent_occurrence_id: None,
                local_pose: None,
            })
            .unwrap();
        (manager, bodies, body)
    }
    fn call(manager: &mut SketchManager, method: &str, payload: Value) -> Value {
        serde_json::from_str(&host::handle(manager, method, &payload.to_string())).unwrap()
    }
    fn mechanical(manager: &SketchManager) -> Value {
        let mut model: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        model.as_object_mut().unwrap().remove("print_intent");
        model
    }
    fn range(body_id: BodyId) -> PrintHeightRangeDraftDto {
        PrintHeightRangeDraftDto {
            id: None,
            name: "High shell band".into(),
            body_id,
            enabled: true,
            coordinate: PrintHeightCoordinateDto::ObjectBottom,
            min_z_mm: 5.,
            max_z_mm: 10.,
            layout: PrintHeightLayoutDto::Assembly,
            settings: PrintSettingsDto {
                wall_count: Some(6),
                ..Default::default()
            },
            speeds: Default::default(),
        }
    }
    #[test]
    fn height_export_layout_identity_is_owned_and_live_visibility_stays_authoritative() {
        let (mut manager, _, body) = fixture();
        let views = manager
            .upsert_named_view(NamedViewConfigurationDto {
                id: None,
                name: "Print layout".into(),
                camera: crate::dto::ViewCameraDto {
                    position: [30., -40., 20.],
                    target: [0.; 3],
                    up: [0., 0., 1.],
                },
                visible_body_ids: vec![body.0],
                part_offsets: Vec::new(),
                occurrence_offsets: Vec::new(),
                print_layout: true,
                print_bed: Default::default(),
            })
            .unwrap();
        let id = views.views[0].id.clone().unwrap();
        let mut draft = range(body);
        draft.layout = PrintHeightLayoutDto::NamedLayout { id };
        manager.upsert_print_height_range(draft).unwrap();
        assert!(manager.validate_print_height_export_view(None).is_err());
        assert!(manager.validate_print_height_export_view(Some("")).is_err());
        manager.recall_named_view("Print layout".into()).unwrap();
        manager.validate_print_height_export_view(None).unwrap();
        manager
            .validate_print_height_export_view(Some("Print layout"))
            .unwrap();
        manager
            .rename_named_view("Print layout".into(), "Horizontal layout".into())
            .unwrap();
        manager
            .validate_print_height_export_view(Some("Horizontal layout"))
            .unwrap();
        manager
            .set_project_visibility(ProjectVisibilityDto {
                hidden_body_ids: vec![body.0],
                ..Default::default()
            })
            .unwrap();
        assert!(manager
            .export_view_solution(None)
            .unwrap()
            .instance_body_poses
            .iter()
            .all(|pose| !pose.visible));
        assert!(manager
            .export_view_solution(Some("Horizontal layout"))
            .unwrap()
            .instance_body_poses
            .iter()
            .all(|pose| pose.visible));
        manager.clear_named_view();
        assert!(manager.validate_print_height_export_view(None).is_err());
    }

    #[test]
    fn height_intent_guarded_repeats_roundtrip_and_rebind_numeric_correction_preserve_geometry() {
        let (mut manager, bodies, body) = fixture();
        let before = manager.export_project_model().unwrap();
        let physical = mechanical(&manager);
        let binding = manager
            .print_height_binding(body, PrintHeightLayoutDto::Assembly)
            .unwrap();
        assert_eq!(binding.occurrences.len(), 2);
        assert!(binding
            .occurrences
            .iter()
            .all(|pose| pose.min_z_mm == 0. && pose.max_z_mm == 20.));
        assert_eq!(
            call(
                &mut manager,
                "print_intent_upsert_height_range",
                json!({"range":range(body),"expected_model_json":before})
            )["ok"],
            true
        );
        let created = manager.export_project_model().unwrap();
        let id = manager.print_intent.height_ranges[0].id.clone();
        assert_eq!(mechanical(&manager), physical);
        assert_eq!(
            call(
                &mut manager,
                "print_intent_remove_height",
                json!({"id":id,"expected_model_json":before})
            )["ok"],
            false
        );
        assert_eq!(manager.export_project_model().unwrap(), created);
        let mut changed = manager.assembly_document();
        changed.component_structure.occurrences[0]
            .local_pose
            .translation[2] = 30.;
        manager.set_assembly_document(changed).unwrap();
        let report = manager
            .effective_print_intent(vec![body], Some(PrintIntentTargetDto::BambuStudio))
            .unwrap();
        assert!(!report.height_ranges[0].binding_current);
        assert_eq!(report.height_ranges[0].settings.wall_count, Some(6));
        let corrected = manager
            .rebind_print_height(
                &id,
                PrintHeightLayoutDto::Assembly,
                Some(limo_cad_core::PrintHeightIntervalDto {
                    coordinate: PrintHeightCoordinateDto::ObjectBottom,
                    min_z_mm: 2.,
                    max_z_mm: 12.,
                }),
                None,
            )
            .unwrap();
        assert_ne!(corrected.previous_binding, corrected.binding);
        assert_eq!(corrected.previous_interval.unwrap().max_z_mm, 10.);
        assert_eq!(corrected.interval.unwrap().max_z_mm, 12.);
        let saved = manager.export_project_model().unwrap();
        let intent = manager.print_intent();
        let mut loaded = SketchManager::new();
        let plan = loaded.prepare_load_project(saved.clone()).unwrap();
        commit(&mut loaded, plan, &bodies);
        assert_eq!(loaded.print_intent(), intent);
        assert!(
            loaded
                .effective_print_intent(vec![body], None)
                .unwrap()
                .height_ranges[0]
                .binding_current
        );
        let pending = loaded.prepare_load_project(saved).unwrap();
        assert!(loaded.remove_print_height(&id).is_err());
        commit(&mut loaded, pending, &bodies);
        assert_eq!(loaded.print_intent(), intent);
    }
    #[test]
    fn variable_layer_rebind_can_fix_changed_height_atomically_and_rejects_cross_kind_values() {
        let (mut manager, mut bodies, body) = fixture();
        manager
            .upsert_print_layer_profile(PrintLayerHeightProfileDraftDto {
                id: None,
                name: "Fine upper layers".into(),
                body_id: body,
                enabled: true,
                layout: PrintHeightLayoutDto::Assembly,
                points: vec![
                    PrintLayerHeightPointDto {
                        z_mm: 0.,
                        height_mm: 0.2,
                    },
                    PrintLayerHeightPointDto {
                        z_mm: 10.,
                        height_mm: 0.16,
                    },
                    PrintLayerHeightPointDto {
                        z_mm: 20.,
                        height_mm: 0.12,
                    },
                ],
            })
            .unwrap();
        let id = manager.print_intent.layer_height_profiles[0].id.clone();
        // The retained creator and occurrences are unchanged; native recompute grows the source.
        bodies[0].positions[8] = 40.;
        let saved = manager.export_project_model().unwrap();
        let plan = manager.prepare_load_project(saved).unwrap();
        commit(&mut manager, plan, &bodies);
        assert!(
            !manager
                .effective_print_intent(vec![body], None)
                .unwrap()
                .layer_height_profiles[0]
                .binding_current
        );
        let before = manager.export_project_model().unwrap();
        assert!(manager
            .rebind_print_height(&id, PrintHeightLayoutDto::Assembly, None, None)
            .is_err());
        assert_eq!(manager.export_project_model().unwrap(), before);
        let points = vec![
            PrintLayerHeightPointDto {
                z_mm: 0.,
                height_mm: 0.2,
            },
            PrintLayerHeightPointDto {
                z_mm: 20.,
                height_mm: 0.16,
            },
            PrintLayerHeightPointDto {
                z_mm: 40.,
                height_mm: 0.12,
            },
        ];
        let repaired = manager
            .rebind_print_height(&id, PrintHeightLayoutDto::Assembly, None, Some(points))
            .unwrap();
        assert_eq!(repaired.previous_points.unwrap().last().unwrap().z_mm, 20.);
        assert_eq!(repaired.points.unwrap().last().unwrap().z_mm, 40.);
        let before = manager.export_project_model().unwrap();
        assert!(manager
            .rebind_print_height(
                &id,
                PrintHeightLayoutDto::Assembly,
                Some(limo_cad_core::PrintHeightIntervalDto {
                    coordinate: PrintHeightCoordinateDto::ObjectBottom,
                    min_z_mm: 0.,
                    max_z_mm: 2.
                }),
                None
            )
            .is_err());
        assert_eq!(manager.export_project_model().unwrap(), before);
    }
}
