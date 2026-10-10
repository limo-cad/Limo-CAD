//! Requested manufacturing settings. They never claim realized toolpaths or strength.
use crate::{BodyId, PrintProfileSource, PrintTargetHandoffDto};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfillPatternDto {
    Grid,
    Gyroid,
    Rectilinear,
    Concentric,
    Cubic,
    Honeycomb,
    Lightning,
}

/// An absent value inherits. Zero is an explicit request, including zero walls.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintSettingsDto {
    #[serde(default)]
    pub wall_count: Option<u32>,
    #[serde(default)]
    pub infill_density_percent: Option<f64>,
    #[serde(default)]
    pub infill_pattern: Option<InfillPatternDto>,
    #[serde(default)]
    pub top_shell_layers: Option<u32>,
    #[serde(default)]
    pub bottom_shell_layers: Option<u32>,
}
impl PrintSettingsDto {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.wall_count,
            self.top_shell_layers,
            self.bottom_shell_layers,
        ]
        .into_iter()
        .flatten()
        .any(|count| count > 1000)
        {
            return Err("Print wall and shell counts must be in 0..=1000".into());
        }
        if self
            .infill_density_percent
            .is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
        {
            return Err("Print infill density must be finite and in 0..=100 percent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessProfileStatusDto {
    /// Typed requested defaults and provenance are available; no slicer evidence is implied.
    Resolved,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessProfileSourceDto {
    PinnedRepository {
        source: PrintProfileSource,
    },
    SavedTemplate {
        sha256: String,
        source_label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessProfileSnapshotDto {
    pub profile_id: String,
    pub name: String,
    #[serde(default)]
    pub source: Option<ProcessProfileSourceDto>,
    pub status: ProcessProfileStatusDto,
    #[serde(default)]
    pub defaults: PrintSettingsDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartPrintIntentDto {
    pub body_id: BodyId,
    pub settings: PrintSettingsDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintIntentPresetDto {
    pub name: String,
    pub settings: PrintSettingsDto,
}

/// Definition-level intent is shared by every intentional occurrence of a source body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintIntentDocumentDto {
    pub version: u32,
    /// Persistent namespace for target-project refresh, assigned on the first successful edit.
    #[serde(default)]
    pub source_document_id: Option<String>,
    #[serde(default)]
    pub selected_process: Option<ProcessProfileSnapshotDto>,
    #[serde(default)]
    pub defaults: PrintSettingsDto,
    #[serde(default)]
    pub parts: Vec<PartPrintIntentDto>,
    #[serde(default)]
    pub presets: Vec<PrintIntentPresetDto>,
    #[serde(default)]
    pub target_handoffs: Vec<PrintTargetHandoffDto>,
    #[serde(default)]
    pub modifiers: Vec<crate::PrintModifierDto>,
    #[serde(default)]
    pub height_ranges: Vec<crate::PrintHeightRangeDto>,
    #[serde(default)]
    pub layer_height_profiles: Vec<crate::PrintLayerHeightProfileDto>,
}
impl Default for PrintIntentDocumentDto {
    fn default() -> Self {
        Self {
            version: 4,
            source_document_id: None,
            selected_process: None,
            defaults: PrintSettingsDto::default(),
            parts: Vec::new(),
            presets: Vec::new(),
            target_handoffs: Vec::new(),
            modifiers: Vec::new(),
            height_ranges: Vec::new(),
            layer_height_profiles: Vec::new(),
        }
    }
}
impl PrintIntentDocumentDto {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 4
            || self.parts.len() > 4096
            || self.presets.len() > 128
            || self.target_handoffs.len() > 16
        {
            return Err("Unsupported print-intent version or excessive part/preset data".into());
        }
        self.defaults.validate()?;
        crate::validate_print_heights(&self.height_ranges, &self.layer_height_profiles)?;
        crate::validate_print_modifiers(&self.modifiers)?;
        if self
            .source_document_id
            .as_ref()
            .is_some_and(|id| !valid_source_document_id(id))
        {
            return Err("Print source document identity must be a UUID".into());
        }
        let mut ids = BTreeSet::new();
        let mut handoff_names = BTreeSet::new();
        for handoff in &self.target_handoffs {
            handoff.validate(self.source_document_id.as_deref())?;
            if !handoff_names.insert(handoff.name()) {
                return Err("Duplicate target handoff name".into());
            }
        }
        for part in &self.parts {
            if part.body_id.0 == 0
                || part.body_id.0 >= 9_007_199_254_740_991
                || !ids.insert(part.body_id)
            {
                return Err("Print intent requires unique, non-zero, allocatable body IDs".into());
            }
            part.settings.validate()?;
        }
        let mut names = BTreeSet::new();
        for preset in &self.presets {
            validate_label(&preset.name)?;
            if !names.insert(&preset.name) {
                return Err(format!("Duplicate print preset '{}'", preset.name));
            }
            preset.settings.validate()?;
        }
        if let Some(profile) = &self.selected_process {
            validate_label(&profile.profile_id)?;
            validate_label(&profile.name)?;
            profile.defaults.validate()?;
            if profile.status == ProcessProfileStatusDto::Resolved && profile.source.is_none() {
                return Err("Resolved process defaults require pinned source provenance".into());
            }
            if let Some(ProcessProfileSourceDto::PinnedRepository { source }) = &profile.source {
                if source.repository.trim().is_empty()
                    || source.repository.len() > 512
                    || source.profile.trim().is_empty()
                    || source.profile.len() > 4096
                    || source.revision.len() != 40
                    || !source.revision.bytes().all(|c| c.is_ascii_hexdigit())
                    || source.files.is_empty()
                    || source.files.len() > 128
                    || source.files.iter().any(|(path, hash)| {
                        path.trim().is_empty()
                            || path.len() > 4096
                            || hash.len() != 64
                            || !hash.bytes().all(|c| c.is_ascii_hexdigit())
                    })
                {
                    return Err("Invalid process-profile source provenance".into());
                }
            }
            if let Some(ProcessProfileSourceDto::SavedTemplate {
                sha256,
                source_label,
            }) = &profile.source
            {
                validate_label(source_label)?;
                if sha256.len() != 64 || !sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err("Saved process template needs its exact SHA-256".into());
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn valid_source_document_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, value)| {
            if [8, 13, 18, 23].contains(&index) {
                value == b'-'
            } else {
                value.is_ascii_hexdigit()
            }
        })
}

fn validate_label(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err("Print profile and preset names require 1..=256 printable bytes".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintSettingFieldDto {
    WallCount,
    InfillDensityPercent,
    InfillPattern,
    TopShellLayers,
    BottomShellLayers,
}
impl PrintSettingFieldDto {
    pub const ALL: [Self; 5] = [
        Self::WallCount,
        Self::InfillDensityPercent,
        Self::InfillPattern,
        Self::TopShellLayers,
        Self::BottomShellLayers,
    ];
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintIntentTargetDto {
    #[default]
    Portable,
    BambuStudio,
    OrcaSlicer,
    PrusaSlicer,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintIntentScopeDto {
    Project,
    Part,
    Occurrence,
    Layout,
    Modifier,
    HeightRange,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintSettingCapabilityDto {
    pub field: PrintSettingFieldDto,
    pub target: PrintIntentTargetDto,
    pub scope: PrintIntentScopeDto,
    /// Known representation capability; it does not say metadata has been written or applied.
    pub supported: bool,
}
pub fn print_setting_capabilities(
    target: PrintIntentTargetDto,
    scope: PrintIntentScopeDto,
) -> Vec<PrintSettingCapabilityDto> {
    let supported = matches!(target, PrintIntentTargetDto::BambuStudio)
        && matches!(
            scope,
            PrintIntentScopeDto::Project
                | PrintIntentScopeDto::Part
                | PrintIntentScopeDto::Modifier
                | PrintIntentScopeDto::HeightRange
        );
    PrintSettingFieldDto::ALL
        .into_iter()
        .map(|field| PrintSettingCapabilityDto {
            field,
            target,
            scope,
            supported,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintSettingSourceDto {
    Profile,
    ProjectDefault,
    Part,
    Modifier,
    HeightRange,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrintSettingSourcesDto {
    pub wall_count: Option<PrintSettingSourceDto>,
    pub infill_density_percent: Option<PrintSettingSourceDto>,
    pub infill_pattern: Option<PrintSettingSourceDto>,
    pub top_shell_layers: Option<PrintSettingSourceDto>,
    pub bottom_shell_layers: Option<PrintSettingSourceDto>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintPartBindingDto {
    Live,
    Retained,
    Orphan,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartPrintIntentEffectiveDto {
    pub body_id: BodyId,
    pub binding: PrintPartBindingDto,
    pub requested: PrintSettingsDto,
    pub settings: PrintSettingsDto,
    pub sources: PrintSettingSourcesDto,
    pub unsupported: Vec<PrintSettingFieldDto>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintIntentEffectiveReportDto {
    pub selected_process: Option<ProcessProfileSnapshotDto>,
    pub project_defaults: PrintSettingsDto,
    pub parts: Vec<PartPrintIntentEffectiveDto>,
    pub orphan_body_ids: Vec<BodyId>,
    pub profile_status: Option<ProcessProfileStatusDto>,
    pub warnings: Vec<String>,
    pub capabilities: Vec<PrintSettingCapabilityDto>,
    pub modifiers: Vec<crate::PrintModifierEffectiveDto>,
    pub height_ranges: Vec<crate::PrintHeightRangeEffectiveDto>,
    pub layer_height_profiles: Vec<crate::PrintLayerHeightProfileEffectiveDto>,
}

/// Resolve requested settings once; exporters and reports consume the same values and origins.
pub fn resolve_print_settings(
    document: &PrintIntentDocumentDto,
    part: &PrintSettingsDto,
) -> (PrintSettingsDto, PrintSettingSourcesDto) {
    let profile = document
        .selected_process
        .as_ref()
        .filter(|profile| profile.status == ProcessProfileStatusDto::Resolved);
    resolve_print_setting_layers(
        profile
            .map(|profile| (&profile.defaults, PrintSettingSourceDto::Profile))
            .into_iter()
            .chain([
                (&document.defaults, PrintSettingSourceDto::ProjectDefault),
                (part, PrintSettingSourceDto::Part),
            ]),
    )
}

/// Apply typed settings once in explicit scope order, preserving each field's actual source.
pub fn resolve_print_setting_layers<'a>(
    layers: impl IntoIterator<Item = (&'a PrintSettingsDto, PrintSettingSourceDto)>,
) -> (PrintSettingsDto, PrintSettingSourcesDto) {
    let mut settings = PrintSettingsDto::default();
    let mut sources = PrintSettingSourcesDto::default();
    for (next, source) in layers {
        macro_rules! overlay {
            ($($field:ident),*) => { $(if next.$field.is_some() {
                settings.$field = next.$field;
                sources.$field = Some(source);
            })* };
        }
        overlay!(
            wall_count,
            infill_density_percent,
            infill_pattern,
            top_shell_layers,
            bottom_shell_layers
        );
    }
    (settings, sources)
}

pub fn configured_print_fields(settings: &PrintSettingsDto) -> Vec<PrintSettingFieldDto> {
    [
        (
            settings.wall_count.is_some(),
            PrintSettingFieldDto::WallCount,
        ),
        (
            settings.infill_density_percent.is_some(),
            PrintSettingFieldDto::InfillDensityPercent,
        ),
        (
            settings.infill_pattern.is_some(),
            PrintSettingFieldDto::InfillPattern,
        ),
        (
            settings.top_shell_layers.is_some(),
            PrintSettingFieldDto::TopShellLayers,
        ),
        (
            settings.bottom_shell_layers.is_some(),
            PrintSettingFieldDto::BottomShellLayers,
        ),
    ]
    .into_iter()
    .filter_map(|(configured, field)| configured.then_some(field))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_is_an_override_and_unresolved_profiles_do_not_invent_defaults() {
        let mut document = PrintIntentDocumentDto::default();
        document.defaults.wall_count = Some(6);
        let (settings, sources) = resolve_print_settings(
            &document,
            &PrintSettingsDto {
                wall_count: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(settings.wall_count, Some(0));
        assert_eq!(sources.wall_count, Some(PrintSettingSourceDto::Part));
        document.selected_process = Some(ProcessProfileSnapshotDto {
            profile_id: "missing".into(),
            name: "Missing profile".into(),
            source: None,
            status: ProcessProfileStatusDto::Unresolved,
            defaults: PrintSettingsDto {
                infill_density_percent: Some(20.),
                ..Default::default()
            },
        });
        document.validate().unwrap();
        assert_eq!(
            resolve_print_settings(&document, &Default::default())
                .0
                .infill_density_percent,
            None
        );
    }
    #[test]
    fn invalid_and_unknown_print_settings_are_rejected() {
        for value in [-1., 101., f64::NAN, f64::INFINITY] {
            assert!(PrintSettingsDto {
                infill_density_percent: Some(value),
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(serde_json::from_str::<PrintSettingsDto>(r#"{"wall_count":-1}"#).is_err());
        assert!(
            serde_json::from_str::<PrintSettingsDto>(r#"{"infill_pattern":"made_up"}"#).is_err()
        );
        assert!(serde_json::from_str::<PrintSettingsDto>(r#"{"bed_temperature":100}"#).is_err());
        assert!(print_setting_capabilities(
            PrintIntentTargetDto::Portable,
            PrintIntentScopeDto::Part
        )
        .iter()
        .all(|c| !c.supported));
        assert!(print_setting_capabilities(
            PrintIntentTargetDto::BambuStudio,
            PrintIntentScopeDto::Occurrence
        )
        .iter()
        .all(|c| !c.supported));
    }
}
