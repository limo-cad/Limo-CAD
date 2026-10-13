//! Persistent target identity and baselines; realized slicer evidence remains separate.
use crate::BodyId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Bind an intentional CAD occurrence to one existing Bambu normal volume instance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuPartBinding {
    pub body_id: BodyId,
    pub occurrence_id: u64,
    pub object_id: u32,
    pub instance_id: u32,
    pub part_id: u32,
}
impl BambuPartBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.body_id.0 == 0
            || self.body_id.0 >= 9_007_199_254_740_991
            || self.occurrence_id == 0
            || self.occurrence_id >= 9_007_199_254_740_991
            || self.object_id == 0
            || self.part_id == 0
        {
            return Err(
                "Bambu binding requires allocatable source IDs and positive object/part IDs".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuRefreshPart {
    pub binding: BambuPartBinding,
    pub target_uuid: String,
    pub instance_identify_id: u32,
    pub baseline_part_settings: BTreeMap<String, String>,
    pub written_part_settings: BTreeMap<String, String>,
    #[serde(default)]
    pub baseline_object_settings: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub written_object_settings: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuRefreshReference {
    pub version: u32,
    pub source_document_id: String,
    pub original_template_sha256: String,
    pub profile_sha256: String,
    pub profile_identity_sha256: String,
    pub baseline_project_settings: BTreeMap<String, String>,
    pub written_project_settings: BTreeMap<String, String>,
    pub parts: Vec<BambuRefreshPart>,
    #[serde(default)]
    pub modifiers: Vec<BambuRefreshModifier>,
    #[serde(default)]
    pub height_objects: Vec<crate::BambuRefreshHeightObjectDto>,
}

/// Snapshot of a generated print-only volume, checked before replacing it on refresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuRefreshModifier {
    pub modifier: crate::PrintModifierDto,
    pub parent_volume_uuid: String,
    pub target_uuid: String,
    pub source_mesh_center_mm: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintTargetHandoffDto {
    BambuStudio {
        name: String,
        source_label: String,
        reference: BambuRefreshReference,
    },
}
impl PrintTargetHandoffDto {
    pub fn name(&self) -> &str {
        match self {
            Self::BambuStudio { name, .. } => name,
        }
    }
    pub fn reference(&self) -> &BambuRefreshReference {
        match self {
            Self::BambuStudio { reference, .. } => reference,
        }
    }
    pub fn validate(&self, source_document_id: Option<&str>) -> Result<(), String> {
        let Self::BambuStudio {
            name,
            source_label,
            reference,
        } = self;
        for label in [name, source_label] {
            if label.trim().is_empty() || label.len() > 256 || label.chars().any(char::is_control) {
                return Err(
                    "Target handoff names and source labels require 1..=256 printable bytes".into(),
                );
            }
        }
        if source_document_id != Some(reference.source_document_id.as_str()) {
            return Err("Target handoff belongs to a different source document".into());
        }
        reference.validate()
    }
}

impl BambuRefreshReference {
    /// Bound stored identity and the five supported requested-setting fields.
    /// The adapter additionally validates complete profiles and native compatibility.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.parts.is_empty() || self.parts.len() > 4096 {
            return Err("Unsupported Bambu refresh version or excessive/empty bindings".into());
        }
        if !crate::print_intent::valid_source_document_id(&self.source_document_id) {
            return Err("Bambu refresh source identity must be a UUID".into());
        }
        for hash in [
            &self.original_template_sha256,
            &self.profile_sha256,
            &self.profile_identity_sha256,
        ] {
            if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err("Bambu refresh hashes must be SHA-256 values".into());
            }
        }
        validate_settings(&self.baseline_project_settings, true)?;
        validate_settings(&self.written_project_settings, true)?;
        let mut sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut object_snapshots = BTreeMap::new();
        for part in &self.parts {
            let binding = &part.binding;
            binding.validate()?;
            if part.instance_identify_id == 0
                || !sources.insert((binding.body_id, binding.occurrence_id))
                || !targets.insert((&part.target_uuid, part.instance_identify_id))
                || part.target_uuid.trim().is_empty()
                || part.target_uuid.len() > 256
                || part.target_uuid.chars().any(char::is_control)
            {
                return Err("Bambu refresh requires unique, allocatable source and target instance bindings".into());
            }
            validate_settings(&part.baseline_part_settings, false)?;
            validate_settings(&part.written_part_settings, false)?;
            let snapshot = (
                &part.baseline_object_settings,
                &part.written_object_settings,
            );
            if object_snapshots
                .insert(&part.target_uuid, snapshot)
                .is_some_and(|previous| previous != snapshot)
            {
                return Err(
                    "Repeated volume bindings require consistent object refresh settings".into(),
                );
            }
            match (
                &part.baseline_object_settings,
                &part.written_object_settings,
            ) {
                (Some(baseline), Some(written)) => {
                    validate_settings(baseline, false)?;
                    validate_settings(written, false)?;
                }
                (None, None) => {}
                _ => {
                    return Err(
                        "Bambu object refresh requires both baseline and written settings".into(),
                    )
                }
            }
        }
        if self.modifiers.len() > 1024 {
            return Err("Excessive native modifier refresh volumes".into());
        }
        let mut modifier_targets = BTreeSet::new();
        let mut modifier_sources = BTreeSet::new();
        for reference in &self.modifiers {
            reference.modifier.validate()?;
            if !reference.modifier.enabled
                || crate::configured_print_fields(&reference.modifier.settings).is_empty()
                || reference
                    .source_mesh_center_mm
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() > 10_000_000.)
                || reference.target_uuid.is_empty()
                || reference.target_uuid.len() > 256
                || reference.target_uuid.chars().any(char::is_control)
                || !modifier_targets.insert(reference.target_uuid.to_ascii_lowercase())
                || !modifier_sources.insert((
                    reference.modifier.id.to_ascii_lowercase(),
                    reference.parent_volume_uuid.to_ascii_lowercase(),
                ))
                || self.parts.iter().any(|part| {
                    part.target_uuid
                        .eq_ignore_ascii_case(&reference.target_uuid)
                })
                || !self.parts.iter().any(|part| {
                    part.target_uuid == reference.parent_volume_uuid
                        && part.binding.body_id == reference.modifier.body_id
                })
            {
                return Err(
                    "Invalid or ambiguous print modifier refresh identity/attachment".into(),
                );
            }
        }
        if self.height_objects.len() > 4096 {
            return Err("Excessive native height refresh records".into());
        }
        let mut height_sources = BTreeSet::new();
        let mut height_targets = BTreeSet::new();
        for record in &self.height_objects {
            record.validate()?;
            let mut object_ids = BTreeSet::new();
            for source in &record.source_bindings {
                let part = self
                    .parts
                    .iter()
                    .find(|part| {
                        part.binding.body_id == source.body_id
                            && part.binding.occurrence_id == source.occurrence_id
                    })
                    .ok_or("Height refresh record references an unbound normal volume")?;
                if !height_sources.insert(*source) {
                    return Err("Height refresh source belongs to multiple objects".into());
                }
                object_ids.insert(part.binding.object_id);
            }
            if object_ids.len() != 1 {
                return Err("Height refresh must own one complete native multipart object".into());
            }
            let object_id = *object_ids.first().unwrap();
            if !height_targets.insert(object_id)
                || self
                    .parts
                    .iter()
                    .filter(|part| part.binding.object_id == object_id)
                    .count()
                    != record.source_bindings.len()
            {
                return Err(
                    "Height refresh omitted a native multipart sibling or repeated instance".into(),
                );
            }
        }
        Ok(())
    }
}

fn validate_settings(settings: &BTreeMap<String, String>, complete: bool) -> Result<(), String> {
    if settings.len() > 5 || (complete && settings.len() != 5) {
        return Err(
            "Bambu refresh stores only five supported settings, with complete project defaults"
                .into(),
        );
    }
    for (key, value) in settings {
        let valid = match key.as_str() {
            "wall_loops" | "top_shell_layers" | "bottom_shell_layers" => {
                value.parse::<u32>().is_ok_and(|v| v <= 1000)
            }
            "sparse_infill_density" => value
                .strip_suffix('%')
                .and_then(|v| v.parse::<f64>().ok())
                .is_some_and(|v| v.is_finite() && (0.0..=100.0).contains(&v)),
            "sparse_infill_pattern" => matches!(
                value.as_str(),
                "grid" | "gyroid" | "zig-zag" | "concentric" | "cubic" | "honeycomb" | "lightning"
            ),
            _ => false,
        };
        if value.len() > 64 || !valid {
            return Err(format!(
                "Unsupported or invalid Bambu refresh setting '{key}'"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample_reference() -> BambuRefreshReference {
        let settings = BTreeMap::from([
            ("wall_loops".into(), "2".into()),
            ("sparse_infill_density".into(), "15%".into()),
            ("sparse_infill_pattern".into(), "gyroid".into()),
            ("top_shell_layers".into(), "5".into()),
            ("bottom_shell_layers".into(), "3".into()),
        ]);
        BambuRefreshReference {
            version: 1,
            source_document_id: "11111111-1111-4111-8111-111111111111".into(),
            original_template_sha256: "a".repeat(64),
            profile_sha256: "b".repeat(64),
            profile_identity_sha256: "c".repeat(64),
            baseline_project_settings: settings.clone(),
            written_project_settings: settings,
            modifiers: Vec::new(),
            height_objects: Vec::new(),
            parts: vec![BambuRefreshPart {
                binding: BambuPartBinding {
                    body_id: BodyId(1),
                    occurrence_id: 1,
                    object_id: 1,
                    instance_id: 0,
                    part_id: 2,
                },
                target_uuid: "volume-uuid".into(),
                instance_identify_id: 1,
                baseline_part_settings: Default::default(),
                written_part_settings: Default::default(),
                baseline_object_settings: None,
                written_object_settings: None,
            }],
        }
    }
    #[test]
    fn handoff_identity_allows_intentional_repeats_but_rejects_duplicate_pairs_and_poisoned_ids() {
        let mut reference = sample_reference();
        let mut repeat = reference.parts[0].clone();
        repeat.binding.occurrence_id = 2;
        repeat.binding.instance_id = 1;
        repeat.instance_identify_id = 2;
        reference.parts.push(repeat);
        reference.validate().unwrap();
        let valid = reference.clone();
        reference.parts[1].instance_identify_id = 1;
        assert!(reference.validate().is_err());
        reference = valid.clone();
        reference.parts[1].binding.occurrence_id = 1;
        assert!(reference.validate().is_err());
        reference = valid.clone();
        reference.parts[0].binding.body_id = BodyId(u64::MAX);
        assert!(reference.validate().is_err());
        reference = valid;
        reference.parts[0]
            .written_part_settings
            .insert("arbitrary_gcode".into(), "G28".into());
        assert!(reference.validate().is_err());
    }
    #[test]
    fn stored_handoff_rejects_foreign_namespace_future_versions_and_invalid_defaults() {
        let mut reference = sample_reference();
        reference.version = 2;
        assert!(reference.validate().is_err());
        reference.version = 1;
        reference
            .baseline_project_settings
            .insert("sparse_infill_density".into(), "NaN%".into());
        assert!(reference.validate().is_err());
        let handoff = PrintTargetHandoffDto::BambuStudio {
            name: "Printer project".into(),
            source_label: "template.3mf".into(),
            reference: sample_reference(),
        };
        assert!(handoff
            .validate(Some("22222222-2222-4222-8222-222222222222"))
            .is_err());
        handoff
            .validate(Some(handoff.reference().source_document_id.as_str()))
            .unwrap();
    }
    #[test]
    fn legacy_object_scope_reference_defaults_and_new_snapshots_are_bounded() {
        let reference = sample_reference();
        let mut old = serde_json::to_value(&reference).unwrap();
        for part in old["parts"].as_array_mut().unwrap() {
            part.as_object_mut()
                .unwrap()
                .remove("baseline_object_settings");
            part.as_object_mut()
                .unwrap()
                .remove("written_object_settings");
        }
        let restored: BambuRefreshReference = serde_json::from_value(old).unwrap();
        assert_eq!(restored, reference);
        let mut invalid = reference.clone();
        invalid.parts[0].baseline_object_settings = Some(BTreeMap::new());
        assert!(invalid.validate().unwrap_err().contains("both baseline"));
        invalid.parts[0].written_object_settings =
            Some(BTreeMap::from([("temperature".into(), "260".into())]));
        assert!(invalid.validate().unwrap_err().contains("Unsupported"));
        invalid.parts[0].written_object_settings = Some(BTreeMap::new());
        assert!(invalid.validate().is_ok());
    }
}
