use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BambuZCorrectionTarget {
    SavedTemplate,
    CadPrintLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuGroupZPreflight {
    pub object_id: u32,
    pub instance_id: u32,
    pub plate_index: u32,
    pub source_bindings: Vec<BambuPartBinding>,
    pub world_bounds: limo_cad_core::PrintModifierBoundsDto,
    pub issues: Vec<crate::print_layout::LayoutIssue>,
    /// Proposed world-axis translation for the whole normal multipart group; never applied here.
    pub proposed_translation_mm: Option<[f64; 3]>,
    pub correction_target: BambuZCorrectionTarget,
}

pub(super) fn populate(result: &mut BambuProjectExport) -> Result<(), ExportError> {
    let mut groups = BTreeMap::<(u32, u32, u32), BambuGroupZPreflight>::new();
    for volume in read_bambu_volume_geometry(&result.bytes)? {
        if volume.subtype != "normal_part" {
            continue;
        }
        let binding = result
            .report
            .parts
            .iter()
            .find(|part| {
                part.binding.object_id == volume.object_id
                    && part.binding.instance_id == volume.instance_id
                    && part.binding.part_id == volume.part_id
            })
            .ok_or_else(|| err("Normal group preflight lost its reviewed source binding"))?
            .binding
            .clone();
        let group = groups
            .entry((volume.object_id, volume.instance_id, volume.plate_index))
            .or_insert_with(|| BambuGroupZPreflight {
                object_id: volume.object_id,
                instance_id: volume.instance_id,
                plate_index: volume.plate_index,
                source_bindings: Vec::new(),
                world_bounds: volume.world_bounds,
                issues: Vec::new(),
                proposed_translation_mm: None,
                correction_target: match result.report.placement {
                    BambuPlacementMode::Template => BambuZCorrectionTarget::SavedTemplate,
                    BambuPlacementMode::ResolvedScene => BambuZCorrectionTarget::CadPrintLayout,
                },
            });
        for axis in 0..3 {
            group.world_bounds.min_mm[axis] =
                group.world_bounds.min_mm[axis].min(volume.world_bounds.min_mm[axis]);
            group.world_bounds.max_mm[axis] =
                group.world_bounds.max_mm[axis].max(volume.world_bounds.max_mm[axis]);
        }
        group.source_bindings.push(binding);
    }
    for group in groups.values_mut() {
        group.source_bindings.sort();
        let ids = group
            .source_bindings
            .iter()
            .map(|binding| binding.occurrence_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let label = format!(
            "Exported object {} / instance {} / plate {}",
            group.object_id, group.instance_id, group.plate_index
        );
        if let Some(issue) =
            crate::print_layout::bed_z_issue(group.world_bounds.min_mm[2], &label, ids)
        {
            let target = match group.correction_target {
                BambuZCorrectionTarget::SavedTemplate => "the saved slicer template; CAD offsets do not change explicit template placement",
                BambuZCorrectionTarget::CadPrintLayout => "the selected CAD print layout",
            };
            let dz = -group.world_bounds.min_mm[2];
            result.report.warnings.push(format!("{} Review support intent or translate the entire group by Z {dz:.4} mm in {target}, then preview again. This proposal is not applied and does not qualify XY clearance or supports.", issue.message));
            group.issues.push(issue);
            group.proposed_translation_mm = Some([0., 0., dz]);
        }
    }
    result.report.z_preflight = groups.into_values().collect();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_template_groups_report_float_and_submerge_without_moving_repeats() {
        let (template, meshes, appearances, instances, structure, intent, mut request) =
            super::super::tests::fixture();
        let mut entries = archive(&template).unwrap();
        let root = text(&entries, ROOT)
            .unwrap()
            .replace("20 20 0\"", "20 20 13.6\"")
            .replace("80 20 0\"", "80 20 2\"");
        entries.insert(ROOT.into(), root.into_bytes());
        // Native saved templates center their mesh coordinates around the volume origin.
        for (path, id) in [("3D/Objects/a.model", 7), ("3D/Objects/b.model", 9)] {
            entries.insert(path.into(), format!(r#"<model xmlns="{CORE_NS}" unit="millimeter"><resources><object id="{id}" type="model">{}</object></resources></model>"#, mesh_xml(&super::super::tests::cube(1), [5.; 3])).into_bytes());
        }
        request.placement = BambuPlacementMode::Template;
        let source = write_archive(&entries).unwrap();
        let before = read_bambu_volume_geometry(&source).unwrap();
        let result = write_bambu_project(
            &source,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(result.report.z_preflight.len(), 2);
        assert!((result.report.z_preflight[0].world_bounds.min_mm[2] - 8.6).abs() < 1e-6);
        assert!((result.report.z_preflight[1].world_bounds.min_mm[2] + 3.).abs() < 1e-6);
        assert_eq!(
            result.report.z_preflight[0].issues[0].occurrence_ids,
            [11, 12]
        );
        assert_eq!(
            result.report.z_preflight[1].issues[0].occurrence_ids,
            [21, 22]
        );
        for (group, code) in result
            .report
            .z_preflight
            .iter()
            .zip(["above_bed", "below_bed"])
        {
            assert_eq!(group.source_bindings.len(), 2);
            assert_eq!(group.issues[0].code, code);
            assert_eq!(
                group.correction_target,
                BambuZCorrectionTarget::SavedTemplate
            );
            assert_eq!(
                group.proposed_translation_mm.unwrap(),
                [0., 0., -group.world_bounds.min_mm[2]]
            );
            assert_eq!(group.issues[0].occurrence_ids.len(), 2);
        }
        assert!(result
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("CAD offsets do not change")));
        assert_eq!(result.report.parts.len(), 4);
        let after = read_bambu_volume_geometry(&result.bytes).unwrap();
        for (a, b) in before.iter().zip(after.iter()) {
            assert_eq!(a.world_bounds, b.world_bounds);
            assert_eq!(a.object_id, b.object_id);
            assert_eq!(a.instance_id, b.instance_id);
            assert_eq!(a.part_id, b.part_id);
        }
        let mut old = serde_json::to_value(&result.report).unwrap();
        old.as_object_mut().unwrap().remove("z_preflight");
        assert!(serde_json::from_value::<BambuProjectReport>(old)
            .unwrap()
            .z_preflight
            .is_empty());
    }

    #[test]
    fn grounded_normal_groups_ignore_print_only_geometry_below_the_plate() {
        use limo_cad_core::{PrintLocalPoseDto, PrintModifierDto, PrintModifierPrimitiveDto};
        let (template, meshes, appearances, instances, structure, mut intent, request) =
            super::super::tests::fixture();
        intent.modifiers.push(PrintModifierDto {
            id: "d7c2d9c0-d04a-40ed-93d4-1dbe0ec56320".into(),
            name: "Bottom zone".into(),
            body_id: BodyId(1),
            enabled: true,
            local_pose: PrintLocalPoseDto {
                translation_mm: [5., 5., 0.],
                ..Default::default()
            },
            primitive: PrintModifierPrimitiveDto::Box {
                size_mm: [4., 4., 4.],
            },
            settings: PrintSettingsDto {
                wall_count: Some(6),
                ..Default::default()
            },
        });
        let result = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(result.report.modifiers[0].instances.len(), 2);
        assert!(result.report.modifiers[0]
            .instances
            .iter()
            .all(|instance| instance.world_bounds.min_mm[2] < 0.));
        assert_eq!(result.report.z_preflight.len(), 2);
        for group in &result.report.z_preflight {
            assert_eq!(group.source_bindings.len(), 2);
            assert!(group.issues.is_empty());
            assert!(group.proposed_translation_mm.is_none());
            assert_eq!(
                group.correction_target,
                BambuZCorrectionTarget::CadPrintLayout
            );
        }
    }
}
