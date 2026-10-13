//! Requested settings in resolved print Z. Bindings record existing layout poses.

use crate::{BodyId, PrintLocalPoseDto, PrintSettingsDto};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_SAFE_ID: u64 = 9_007_199_254_740_991;
const MAX_PRINT_MM: f64 = 1.0e6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintHeightLayoutDto {
    Assembly,
    NamedLayout { id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintHeightCoordinateDto {
    /// Zero is the resolved printable object's lowest point, excluding raft.
    ObjectBottom,
    /// Zero is the build plate, before slicer raft or lift adjustments.
    BuildPlate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintHeightIntervalDto {
    pub coordinate: PrintHeightCoordinateDto,
    pub min_z_mm: f64,
    pub max_z_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintHeightOccurrenceDto {
    pub body_id: BodyId,
    pub occurrence_id: u64,
    pub root_occurrence_id: u64,
    pub pose: PrintLocalPoseDto,
    /// Bounds of the resolved multipart printable group, including its visible siblings.
    pub min_z_mm: f64,
    pub max_z_mm: f64,
}

/// Engine-captured placements of every visible intentional occurrence of a definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintHeightBindingDto {
    pub layout: PrintHeightLayoutDto,
    pub occurrences: Vec<PrintHeightOccurrenceDto>,
    pub groups: Vec<PrintHeightGroupDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintHeightGroupDto {
    pub root_occurrence_id: u64,
    pub members: Vec<PrintSourceOccurrenceDto>,
    pub min_z_mm: f64,
    pub max_z_mm: f64,
}

/// Closed requested speed fields. Adapter qualification determines support.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintHeightSpeedsDto {
    #[serde(default)]
    pub outer_wall_mm_s: Option<f64>,
    #[serde(default)]
    pub inner_wall_mm_s: Option<f64>,
    #[serde(default)]
    pub infill_mm_s: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintHeightRangeDto {
    pub id: String,
    pub name: String,
    pub body_id: BodyId,
    pub enabled: bool,
    pub coordinate: PrintHeightCoordinateDto,
    pub min_z_mm: f64,
    pub max_z_mm: f64,
    pub binding: PrintHeightBindingDto,
    pub settings: PrintSettingsDto,
    #[serde(default)]
    pub speeds: PrintHeightSpeedsDto,
}

/// Write input. Placement evidence is captured by the owning engine, never supplied here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintHeightRangeDraftDto {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub body_id: BodyId,
    pub enabled: bool,
    pub coordinate: PrintHeightCoordinateDto,
    pub min_z_mm: f64,
    pub max_z_mm: f64,
    pub layout: PrintHeightLayoutDto,
    pub settings: PrintSettingsDto,
    #[serde(default)]
    pub speeds: PrintHeightSpeedsDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintLayerHeightPointDto {
    pub z_mm: f64,
    pub height_mm: f64,
}

/// Variable layer heights are a separate opt-in from settings-only intervals.
/// The selected saved slicer template supplies authoritative nozzle constraints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintLayerHeightProfileDto {
    pub id: String,
    pub name: String,
    pub body_id: BodyId,
    pub enabled: bool,
    pub binding: PrintHeightBindingDto,
    /// Object-bottom coordinates, including zero and the exact resolved object height.
    pub points: Vec<PrintLayerHeightPointDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintLayerHeightProfileDraftDto {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub body_id: BodyId,
    pub enabled: bool,
    pub layout: PrintHeightLayoutDto,
    pub points: Vec<PrintLayerHeightPointDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintHeightRebindReportDto {
    pub document: crate::PrintIntentDocumentDto,
    pub previous_binding: PrintHeightBindingDto,
    pub binding: PrintHeightBindingDto,
    pub previous_interval: Option<PrintHeightIntervalDto>,
    pub interval: Option<PrintHeightIntervalDto>,
    pub previous_points: Option<Vec<PrintLayerHeightPointDto>>,
    pub points: Option<Vec<PrintLayerHeightPointDto>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintHeightRangeEffectiveDto {
    pub range: PrintHeightRangeDto,
    pub binding: crate::PrintPartBindingDto,
    pub binding_current: bool,
    pub issues: Vec<String>,
    pub settings: PrintSettingsDto,
    pub sources: crate::PrintSettingSourcesDto,
    pub unsupported: Vec<crate::PrintSettingFieldDto>,
    pub unsupported_speeds: Vec<PrintHeightSpeedFieldDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintHeightSpeedFieldDto {
    OuterWallMmS,
    InnerWallMmS,
    InfillMmS,
}

/// Installed Bambu 2.8.2.61 qualification covers uniform speed variants and
/// variable schedules. This is representation support, not per-project evidence.
pub fn print_height_target_supported(target: crate::PrintIntentTargetDto) -> bool {
    matches!(target, crate::PrintIntentTargetDto::BambuStudio)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintLayerHeightProfileEffectiveDto {
    pub profile: PrintLayerHeightProfileDto,
    pub binding: crate::PrintPartBindingDto,
    pub binding_current: bool,
    pub issues: Vec<String>,
    /// True only when the target adapter has qualified its variable-layer path.
    pub target_supported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintSourceOccurrenceDto {
    pub body_id: BodyId,
    pub occurrence_id: u64,
}

/// Known native range keys only. `layer_height_mm` is the unchanged effective native value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuHeightRangeSnapshotDto {
    pub min_z_mm: f64,
    pub max_z_mm: f64,
    pub layer_height_mm: f64,
    pub settings: PrintSettingsDto,
    #[serde(default)]
    pub speeds: PrintHeightSpeedsDto,
}

/// Retained managed native metadata, resolved through existing source/UUID/instance bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuRefreshHeightObjectDto {
    pub source_bindings: Vec<PrintSourceOccurrenceDto>,
    pub baseline_ranges: Vec<BambuHeightRangeSnapshotDto>,
    pub written_ranges: Vec<BambuHeightRangeSnapshotDto>,
    pub baseline_profile: Option<Vec<PrintLayerHeightPointDto>>,
    pub written_profile: Option<Vec<PrintLayerHeightPointDto>>,
}

impl BambuRefreshHeightObjectDto {
    pub fn validate(&self) -> Result<(), String> {
        if self.source_bindings.is_empty()
            || self.source_bindings.len() > 4096
            || self.baseline_ranges.len() > 256
            || self.written_ranges.len() > 256
        {
            return Err("Excessive or empty managed native height metadata".into());
        }
        let mut ids = BTreeSet::new();
        for pair in &self.source_bindings {
            if pair.body_id.0 == 0
                || pair.body_id.0 >= MAX_SAFE_ID
                || pair.occurrence_id == 0
                || pair.occurrence_id >= MAX_SAFE_ID
                || !ids.insert(*pair)
            {
                return Err(
                    "Managed native height object requires unique safe source bindings".into(),
                );
            }
        }
        for range in self.baseline_ranges.iter().chain(&self.written_ranges) {
            if !finite_z(range.min_z_mm)
                || !finite_z(range.max_z_mm)
                || range.min_z_mm < 0.
                || range.min_z_mm >= range.max_z_mm
                || !range.layer_height_mm.is_finite()
                || range.layer_height_mm <= 0.
                || range.layer_height_mm > MAX_PRINT_MM
            {
                return Err("Managed native height intervals require finite ordered bounds and positive layer height".into());
            }
            range.settings.validate()?;
            range.speeds.validate()?;
        }
        for points in self.baseline_profile.iter().chain(&self.written_profile) {
            if points.is_empty() || points.len() > 4096 {
                return Err("Managed native layer profile exceeds its sample bound".into());
            }
            let mut previous = -1.;
            for point in points {
                if !finite_z(point.z_mm)
                    || point.z_mm <= previous
                    || !point.height_mm.is_finite()
                    || point.height_mm <= 0.
                    || point.height_mm > MAX_PRINT_MM
                {
                    return Err("Managed native layer samples need increasing Z and positive finite heights".into());
                }
                previous = point.z_mm;
            }
        }
        Ok(())
    }
}

/// Height scope follows the same process/project/part inheritance as every other request.
pub fn resolve_print_height_settings(
    document: &crate::PrintIntentDocumentDto,
    part: &PrintSettingsDto,
    range: &PrintSettingsDto,
) -> (PrintSettingsDto, crate::PrintSettingSourcesDto) {
    let profile = document
        .selected_process
        .as_ref()
        .filter(|profile| profile.status == crate::ProcessProfileStatusDto::Resolved);
    crate::resolve_print_setting_layers(
        profile
            .map(|profile| (&profile.defaults, crate::PrintSettingSourceDto::Profile))
            .into_iter()
            .chain([
                (
                    &document.defaults,
                    crate::PrintSettingSourceDto::ProjectDefault,
                ),
                (part, crate::PrintSettingSourceDto::Part),
                (range, crate::PrintSettingSourceDto::HeightRange),
            ]),
    )
}

fn identity(id: &str) -> Result<(), String> {
    if id.len() != 36
        || !id.bytes().enumerate().all(|(index, value)| {
            if [8, 13, 18, 23].contains(&index) {
                value == b'-'
            } else {
                value.is_ascii_digit() || (b'a'..=b'f').contains(&value)
            }
        })
    {
        return Err("Print height identity must be a canonical UUID".into());
    }
    Ok(())
}

fn attachment(body_id: BodyId, name: &str) -> Result<(), String> {
    if body_id.0 == 0 || body_id.0 >= MAX_SAFE_ID {
        return Err("Print height intent requires an allocatable source body ID".into());
    }
    if name.trim() != name
        || name.is_empty()
        || name.chars().count() > 200
        || name.chars().any(char::is_control)
    {
        return Err("Print height name must contain 1..=200 printable characters".into());
    }
    Ok(())
}

fn finite_z(z: f64) -> bool {
    z.is_finite() && z.abs() <= MAX_PRINT_MM
}

impl PrintHeightBindingDto {
    pub fn validate(&self, body_id: BodyId) -> Result<(), String> {
        if let PrintHeightLayoutDto::NamedLayout { id } = &self.layout {
            identity(id)?;
        }
        if self.occurrences.is_empty() || self.occurrences.len() > 4096 {
            return Err("Print height binding requires 1..=4096 intentional occurrences".into());
        }
        if self.groups.is_empty()
            || self.groups.len() > 4096
            || self
                .groups
                .iter()
                .map(|group| group.members.len())
                .sum::<usize>()
                > 4096
        {
            return Err("Print height binding requires bounded visible multipart groups".into());
        }
        let mut roots = BTreeSet::new();
        let mut members = BTreeSet::new();
        for group in &self.groups {
            if group.root_occurrence_id == 0
                || group.root_occurrence_id >= MAX_SAFE_ID
                || !roots.insert(group.root_occurrence_id)
                || group.members.is_empty()
                || !finite_z(group.min_z_mm)
                || !finite_z(group.max_z_mm)
                || group.min_z_mm >= group.max_z_mm
            {
                return Err(
                    "Print height groups need unique safe roots and ordered finite bounds".into(),
                );
            }
            for member in &group.members {
                if member.body_id.0 == 0
                    || member.body_id.0 >= MAX_SAFE_ID
                    || member.occurrence_id == 0
                    || member.occurrence_id >= MAX_SAFE_ID
                    || !members.insert(*member)
                {
                    return Err(
                        "Visible print group members require unique safe source identities".into(),
                    );
                }
            }
        }
        let mut ids = BTreeSet::new();
        for occurrence in &self.occurrences {
            if occurrence.body_id != body_id
                || occurrence.occurrence_id == 0
                || occurrence.occurrence_id >= MAX_SAFE_ID
                || occurrence.root_occurrence_id == 0
                || occurrence.root_occurrence_id >= MAX_SAFE_ID
                || !ids.insert(occurrence.occurrence_id)
            {
                return Err(
                    "Print height binding must contain unique owned body occurrences".into(),
                );
            }
            occurrence.pose.validate()?;
            if !finite_z(occurrence.min_z_mm)
                || !finite_z(occurrence.max_z_mm)
                || occurrence.min_z_mm >= occurrence.max_z_mm
            {
                return Err("Print height binding needs finite ordered resolved Z bounds".into());
            }
            let group = self
                .groups
                .iter()
                .find(|group| group.root_occurrence_id == occurrence.root_occurrence_id)
                .ok_or("Print height occurrence omitted its captured group")?;
            if group.min_z_mm != occurrence.min_z_mm
                || group.max_z_mm != occurrence.max_z_mm
                || !group.members.contains(&PrintSourceOccurrenceDto {
                    body_id,
                    occurrence_id: occurrence.occurrence_id,
                })
            {
                return Err("Print height occurrence must match its captured visible group membership and bounds".into());
            }
        }
        if self.groups.iter().any(|group| {
            !self
                .occurrences
                .iter()
                .any(|occurrence| occurrence.root_occurrence_id == group.root_occurrence_id)
        }) {
            return Err("Print height binding contains an unrelated printable group".into());
        }
        Ok(())
    }
}

impl PrintHeightSpeedsDto {
    pub fn validate(&self) -> Result<(), String> {
        if [self.outer_wall_mm_s, self.inner_wall_mm_s, self.infill_mm_s]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=MAX_PRINT_MM).contains(&v))
        {
            return Err("Requested print speeds must be finite and in 0..=1000000 mm/s".into());
        }
        Ok(())
    }
}

impl PrintHeightRangeDto {
    pub fn absolute_interval(&self, occurrence: &PrintHeightOccurrenceDto) -> [f64; 2] {
        let origin = match self.coordinate {
            PrintHeightCoordinateDto::ObjectBottom => occurrence.min_z_mm,
            PrintHeightCoordinateDto::BuildPlate => 0.0,
        };
        [origin + self.min_z_mm, origin + self.max_z_mm]
    }

    pub fn validate(&self) -> Result<(), String> {
        identity(&self.id)?;
        attachment(self.body_id, &self.name)?;
        self.binding.validate(self.body_id)?;
        self.settings.validate()?;
        self.speeds.validate()?;
        if !finite_z(self.min_z_mm)
            || !finite_z(self.max_z_mm)
            || self.min_z_mm < 0.0
            || self.min_z_mm >= self.max_z_mm
        {
            return Err(
                "Print height interval must have finite ordered nonnegative endpoints".into(),
            );
        }
        for occurrence in &self.binding.occurrences {
            let [min, max] = self.absolute_interval(occurrence);
            if min < occurrence.min_z_mm - 0.001 || max > occurrence.max_z_mm + 0.001 {
                return Err(
                    "Print height interval is outside its captured printable group bounds".into(),
                );
            }
        }
        Ok(())
    }
}

impl PrintLayerHeightProfileDto {
    pub fn validate(&self) -> Result<(), String> {
        identity(&self.id)?;
        attachment(self.body_id, &self.name)?;
        self.binding.validate(self.body_id)?;
        if !(3..=4096).contains(&self.points.len()) || self.points[0].z_mm != 0.0 {
            return Err(
                "Variable layer profile requires 3..=4096 samples starting at object Z zero".into(),
            );
        }
        let mut previous = -1.0;
        for point in &self.points {
            if !finite_z(point.z_mm)
                || point.z_mm <= previous
                || !point.height_mm.is_finite()
                || point.height_mm <= 0.0
                || point.height_mm > MAX_PRINT_MM
            {
                return Err(
                    "Variable layer samples require increasing Z and finite positive heights"
                        .into(),
                );
            }
            previous = point.z_mm;
        }
        if self.binding.occurrences.iter().any(|occurrence| {
            (previous - (occurrence.max_z_mm - occurrence.min_z_mm)).abs() > 0.001
        }) {
            return Err(
                "Variable layer profile must end at every captured printable group height".into(),
            );
        }
        Ok(())
    }
}

/// Every identity is unique across range and variable-profile intent in one document.
pub fn validate_print_heights(
    ranges: &[PrintHeightRangeDto],
    profiles: &[PrintLayerHeightProfileDto],
) -> Result<(), String> {
    if ranges.len() > 256 || profiles.len() > 128 {
        return Err(
            "Excessive print height intent: at most 256 ranges and 128 variable profiles".into(),
        );
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for range in ranges {
        range.validate()?;
        if !ids.insert(&range.id) || !names.insert((range.body_id, &range.name)) {
            return Err("Duplicate print height identity or source name".into());
        }
    }
    for (index, left) in ranges.iter().enumerate().filter(|(_, range)| range.enabled) {
        for right in ranges[index + 1..].iter().filter(|range| range.enabled) {
            if left.body_id != right.body_id || left.binding.layout != right.binding.layout {
                continue;
            }
            for occurrence in &left.binding.occurrences {
                if let Some(other) = right
                    .binding
                    .occurrences
                    .iter()
                    .find(|other| other.occurrence_id == occurrence.occurrence_id)
                {
                    let [a, b] = left.absolute_interval(occurrence);
                    let [c, d] = right.absolute_interval(other);
                    if a < d && c < b {
                        return Err("Enabled print height intervals overlap; split or disable one explicitly".into());
                    }
                }
            }
        }
    }
    let mut enabled_profiles = Vec::new();
    for profile in profiles {
        profile.validate()?;
        if !ids.insert(&profile.id) || !names.insert((profile.body_id, &profile.name)) {
            return Err("Duplicate print height identity or source name".into());
        }
        if profile.enabled {
            if enabled_profiles.contains(&(profile.body_id, &profile.binding.layout)) {
                return Err(
                    "Only one enabled variable layer profile can own a source body in a layout"
                        .into(),
                );
            }
            enabled_profiles.push((profile.body_id, &profile.binding.layout));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_capabilities_are_qualified_only_for_bambu() {
        use crate::{print_setting_capabilities, PrintIntentScopeDto, PrintIntentTargetDto};
        for target in [
            PrintIntentTargetDto::BambuStudio,
            PrintIntentTargetDto::OrcaSlicer,
            PrintIntentTargetDto::Portable,
            PrintIntentTargetDto::PrusaSlicer,
        ] {
            let supported = target == PrintIntentTargetDto::BambuStudio;
            assert_eq!(print_height_target_supported(target), supported);
            assert!(
                print_setting_capabilities(target, PrintIntentScopeDto::HeightRange)
                    .iter()
                    .all(|field| field.supported == supported)
            );
        }
    }

    fn range() -> PrintHeightRangeDto {
        PrintHeightRangeDto {
            id: "a851d1cd-70be-4524-b907-ae62d18e9f83".into(),
            name: "Dense lower section".into(),
            body_id: BodyId(10),
            enabled: true,
            coordinate: PrintHeightCoordinateDto::ObjectBottom,
            min_z_mm: 2.,
            max_z_mm: 8.,
            binding: PrintHeightBindingDto {
                layout: PrintHeightLayoutDto::Assembly,
                groups: [11, 12]
                    .into_iter()
                    .map(|id| PrintHeightGroupDto {
                        root_occurrence_id: id,
                        members: vec![PrintSourceOccurrenceDto {
                            body_id: BodyId(10),
                            occurrence_id: id,
                        }],
                        min_z_mm: 5.,
                        max_z_mm: 25.,
                    })
                    .collect(),
                occurrences: [11, 12]
                    .into_iter()
                    .map(|id| PrintHeightOccurrenceDto {
                        body_id: BodyId(10),
                        occurrence_id: id,
                        root_occurrence_id: id,
                        pose: PrintLocalPoseDto::default(),
                        min_z_mm: 5.,
                        max_z_mm: 25.,
                    })
                    .collect(),
            },
            settings: PrintSettingsDto {
                infill_density_percent: Some(70.),
                ..Default::default()
            },
            speeds: Default::default(),
        }
    }

    #[test]
    fn ranges_validate_resolved_z_repeats_and_explicit_coordinate_overlap() {
        let range = range();
        range.validate().unwrap();
        assert_eq!(
            range.absolute_interval(&range.binding.occurrences[0]),
            [7., 13.]
        );
        let mut other = range.clone();
        other.id = "c024d5b7-57cf-423a-bb2b-a1791d8144ca".into();
        other.name = "Plate absolute".into();
        other.coordinate = PrintHeightCoordinateDto::BuildPlate;
        other.min_z_mm = 12.;
        other.max_z_mm = 16.;
        assert!(validate_print_heights(&[range.clone(), other.clone()], &[]).is_err());
        other.min_z_mm = 13.;
        validate_print_heights(&[range.clone(), other], &[]).unwrap();
        for invalid in [f64::NAN, f64::INFINITY, -1., 30.] {
            let mut bad = range.clone();
            bad.max_z_mm = invalid;
            assert!(bad.validate().is_err());
        }
        let mut bad = range;
        bad.binding.occurrences[1].occurrence_id = 11;
        assert!(bad.validate().is_err());
        bad.binding.occurrences[1].occurrence_id = MAX_SAFE_ID;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn variable_profile_requires_real_end_height_and_does_not_invent_nozzle_limits() {
        let range = range();
        let mut profile = PrintLayerHeightProfileDto {
            id: range.id,
            name: "Variable layers".into(),
            body_id: range.body_id,
            enabled: true,
            binding: range.binding,
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
        };
        profile.validate().unwrap();
        profile.points[2].z_mm = 19.;
        assert!(profile.validate().is_err());
        profile.points[2].z_mm = 20.;
        profile.points[1].height_mm = 0.;
        assert!(profile.validate().is_err());
        profile.points[1].height_mm = 0.02;
        profile.validate().unwrap(); // Saved-template nozzle constraints are an adapter gate.
        profile.points[1].z_mm = 0.;
        assert!(profile.validate().is_err());
    }
}
