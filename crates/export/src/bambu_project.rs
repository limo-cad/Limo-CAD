//! Explicit saved-project refresh. Portable export remains independent of Bambu profiles.
use crate::{
    validate_3mf_model_mesh, weld_triangle_mesh, ExportError, MeshInstance, TriangleMesh,
    DEFAULT_WELD_EPSILON,
};
pub use limo_cad_core::{BambuPartBinding, BambuRefreshPart, BambuRefreshReference};
use limo_cad_core::{
    BodyAppearance, BodyId, InfillPatternDto, PrintIntentDocumentDto, PrintSettingsDto,
    ProcessProfileSourceDto, ProcessProfileStatusDto,
};
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
    ops::Range,
};

const ROOT: &str = "3D/3dmodel.model";
const CONFIG: &str = "Metadata/model_settings.config";
const PROFILE: &str = "Metadata/project_settings.config";
const MANIFEST: &str = "Metadata/limo_cad_project.json";
const MAX_INPUT: usize = 128 * 1024 * 1024;
const MAX_EXPANDED: u64 = 512 * 1024 * 1024;
const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BambuPlacementMode {
    /// Use the caller's resolved assembly/named-view poses, preserving target object grouping.
    #[default]
    ResolvedScene,
    /// Preserve the explicitly selected template's centered volume placements and plate layout.
    Template,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BambuProjectRequest {
    pub source_document_id: String,
    /// Empty uses this document's previously written manifest; foreign templates require explicit bindings.
    #[serde(default)]
    pub bindings: Vec<BambuPartBinding>,
    #[serde(default)]
    pub placement: BambuPlacementMode,
    /// Explicitly keep the template's filament chemistry/colors when they differ from CAD appearance.
    #[serde(default)]
    pub allow_template_appearance: bool,
    /// Explicit reference retained from an earlier report, including through native slicer saves.
    #[serde(default)]
    pub refresh_reference: Option<BambuRefreshReference>,
    /// Adopt explicitly reviewed native changes as a new inherited baseline before applying CAD overrides.
    #[serde(default)]
    pub accept_native_setting_changes: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuTemplatePart {
    pub part_id: u32,
    pub uuid: Option<String>,
    pub name: String,
    pub mesh_path: String,
    pub subtype: String,
    pub settings: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuTemplateObject {
    pub object_id: u32,
    pub object_ordinal: u32,
    pub name: String,
    pub instance_count: u32,
    pub parts: Vec<BambuTemplatePart>,
    pub settings: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuTemplateSummary {
    pub template_sha256: String,
    pub version: String,
    pub printer_settings_id: String,
    pub printer_model: String,
    pub printer_variant: String,
    pub process_settings_id: String,
    pub process_defaults: PrintSettingsDto,
    /// Read-only native process values. Missing fields remain absent, including values
    /// outside the typed CAD editing capability; presence does not prove native import.
    #[serde(default)]
    pub native_process_settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub process_capability_warnings: Vec<String>,
    pub nozzle_diameter_mm: Vec<f64>,
    pub filament_settings_ids: Vec<String>,
    pub filament_types: Vec<String>,
    pub filament_colors: Vec<String>,
    pub support_filament: u32,
    pub support_interface_filament: u32,
    /// Logical mappings preserved from the template; these are not physical AMS tray assignments.
    pub filament_map: Value,
    pub filament_nozzle_map: Value,
    pub plate_count: usize,
    pub objects: Vec<BambuTemplateObject>,
    pub has_identity_manifest: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BambuSettingOrigin {
    TemplateProcess,
    TemplateObject,
    TemplateVolume,
    SelectedProcessSnapshot,
    CadProjectDefault,
    CadPart,
    CadModifier,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BambuModifierDisposition {
    Written,
    Disabled,
    NoEffect,
    Excluded,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuModifierInstanceReport {
    pub parent_binding: BambuPartBinding,
    pub target_uuid: String,
    pub plate_index: u32,
    pub world_transform: [f64; 12],
    pub world_bounds: limo_cad_core::PrintModifierBoundsDto,
    pub effective_settings: BTreeMap<String, String>,
    pub effective_sources: BTreeMap<String, BambuSettingOrigin>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuModifierReport {
    pub id: String,
    pub name: String,
    pub body_id: BodyId,
    pub disposition: BambuModifierDisposition,
    pub instances: Vec<BambuModifierInstanceReport>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuPartReport {
    pub binding: BambuPartBinding,
    pub target_uuid: Option<String>,
    pub geometry_sha256: String,
    pub triangle_count: usize,
    pub filament_index: u32,
    pub filament_type: String,
    pub filament_color: String,
    /// Actual exported world affine transform, in standard 3MF column-grouped order.
    pub world_transform: [f64; 12],
    pub plate_index: u32,
    pub effective_sources: BTreeMap<String, BambuSettingOrigin>,
    pub inherited_settings: BTreeMap<String, String>,
    pub written_overrides: BTreeMap<String, String>,
    pub effective_settings: BTreeMap<String, String>,
    /// Native metadata intent, including read-only layer, width, shell and support
    /// fields. These are not measured extrusion widths, support usage or toolpaths.
    #[serde(default)]
    pub native_inherited_settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub native_effective_settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub native_effective_sources: BTreeMap<String, BambuSettingOrigin>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuProjectReport {
    pub template: BambuTemplateSummary,
    pub source_document_id: String,
    pub output_sha256: String,
    pub placement: BambuPlacementMode,
    pub parts: Vec<BambuPartReport>,
    pub modifiers: Vec<BambuModifierReport>,
    /// Actual normal-group Z diagnostics only; no XY envelope or support qualification.
    #[serde(default)]
    pub z_preflight: Vec<BambuGroupZPreflight>,
    pub invalidated_entries: Vec<String>,
    pub warnings: Vec<String>,
    /// Metadata has been independently parsed and compared; this is not installed-slicer evidence.
    pub metadata_readback_verified: bool,
    pub installed_slicer_imported: bool,
    pub toolpaths_generated: bool,
    pub refresh_reference: BambuRefreshReference,
}
pub struct BambuProjectExport {
    pub bytes: Vec<u8>,
    pub report: BambuProjectReport,
}

struct PartTarget {
    path: String,
    mesh_range: Range<usize>,
    component_range: Range<usize>,
    component_transform: Matrix,
}
struct Template {
    entries: BTreeMap<String, Vec<u8>>,
    summary: BambuTemplateSummary,
    targets: BTreeMap<(u32, u32), PartTarget>,
    build_ranges: BTreeMap<(u32, u32), Range<usize>>,
    profile: Value,
    instance_identities: BTreeMap<(u32, u32), u32>,
    plate_indices: BTreeMap<(u32, u32), u32>,
}

/// Validate and inspect a complete saved Bambu project without resolving CAD names or mutating it.
pub fn inspect_bambu_template(bytes: &[u8]) -> Result<BambuTemplateSummary, ExportError> {
    Ok(parse_template(bytes)?.summary)
}

/// Expected object quantities for one explicitly selected plate in the qualified native CLI lane.
pub fn inspect_bambu_plate(
    bytes: &[u8],
    plate_index: u32,
) -> Result<BambuTemplateSummary, ExportError> {
    let mut template = parse_template(bytes)?;
    if plate_index == 0 || plate_index as usize > template.summary.plate_count {
        return fail("Requested validation plate is outside the saved project");
    }
    template.summary.objects.retain_mut(|object| {
        let count = template
            .plate_indices
            .iter()
            .filter(|((object_id, _), plate)| {
                *object_id == object.object_id && **plate == plate_index
            })
            .count();
        object.instance_count = count as u32;
        count > 0
    });
    Ok(template.summary)
}

/// Actual package geometry for diagnostics. Native IDs here are target identities, not CAD IDs.
/// Callers can feed the existing layout analyzer after explicitly mapping reviewed bindings;
/// print-only volumes must never become physical printable bodies in that analysis.
pub struct BambuVolumeGeometry {
    pub object_id: u32,
    pub instance_id: u32,
    pub instance_identify_id: u32,
    pub plate_index: u32,
    pub part_id: u32,
    pub target_uuid: Option<String>,
    pub name: String,
    pub subtype: String,
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    pub world_transform: [f64; 12],
    pub world_bounds: limo_cad_core::PrintModifierBoundsDto,
}

pub fn read_bambu_volume_geometry(bytes: &[u8]) -> Result<Vec<BambuVolumeGeometry>, ExportError> {
    let template = parse_template(bytes)?;
    let root = xml(text(&template.entries, ROOT)?)?;
    let mut geometry = Vec::new();
    let mut expanded_geometry_bytes = 0u64;
    for object in &template.summary.objects {
        for part in &object.parts {
            let mesh = modifiers::read_mesh(&template, object.object_id, part.part_id, BodyId(1))?;
            let component = template.targets[&(object.object_id, part.part_id)].component_transform;
            for instance in 0..object.instance_count {
                expanded_geometry_bytes = expanded_geometry_bytes
                    .checked_add(mesh.positions.len() as u64 * 8 + mesh.indices.len() as u64 * 4)
                    .ok_or_else(|| err("Native geometry readback size overflow"))?;
                if geometry.len() >= 4096 || expanded_geometry_bytes > MAX_EXPANDED {
                    return fail("Native geometry readback exceeds 4096 volume instances or 512 MiB; reduce the project before diagnostics");
                }
                let range = &template.build_ranges[&(object.object_id, instance)];
                let item = root
                    .descendants()
                    .find(|node| node.range() == *range)
                    .ok_or_else(|| err("Native geometry readback lost its build instance"))?;
                let world = Matrix::parse(item.attribute("transform"))?.compose(component);
                let mut min_mm = [f64::INFINITY; 3];
                let mut max_mm = [f64::NEG_INFINITY; 3];
                for point in mesh.positions.as_chunks::<3>().0 {
                    for axis in 0..3 {
                        let coordinate = (0..3)
                            .map(|index| world.0[axis * 4 + index] * point[index])
                            .sum::<f64>()
                            + world.0[axis * 4 + 3];
                        if !coordinate.is_finite() {
                            return fail("Native world geometry transform overflow");
                        }
                        min_mm[axis] = min_mm[axis].min(coordinate);
                        max_mm[axis] = max_mm[axis].max(coordinate);
                    }
                }
                geometry.push(BambuVolumeGeometry {
                    object_id: object.object_id,
                    instance_id: instance,
                    instance_identify_id: template.instance_identities
                        [&(object.object_id, instance)],
                    plate_index: template.plate_indices[&(object.object_id, instance)],
                    part_id: part.part_id,
                    target_uuid: part.uuid.clone(),
                    name: part.name.clone(),
                    subtype: part.subtype.clone(),
                    positions: mesh.positions.clone(),
                    indices: mesh.indices.clone(),
                    world_transform: world.standard_values(),
                    world_bounds: limo_cad_core::PrintModifierBoundsDto { min_mm, max_mm },
                });
            }
        }
    }
    Ok(geometry)
}

/// Compare actual world triangles with native recentering/reordering tolerance of 0.001 mm.
/// Identity/group/plate matching is the caller's responsibility; this does not check layout or strength.
pub fn equivalent_bambu_world_geometry(
    left: &BambuVolumeGeometry,
    right: &BambuVolumeGeometry,
) -> Result<bool, ExportError> {
    let pose = |values: [f64; 12]| {
        Matrix::parse(Some(
            &values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        ))
    };
    modifiers::same_world_geometry(
        &left.positions,
        &left.indices,
        pose(left.world_transform)?,
        &right.positions,
        &right.indices,
        pose(right.world_transform)?,
    )
}

/// Verify generated modifier identity, geometry and supported overrides
/// against a prior report after an installed slicer save. No native toolpaths are inferred.
pub fn verify_bambu_modifier_reference(
    bytes: &[u8],
    reference: &BambuRefreshReference,
) -> Result<(), ExportError> {
    reference.validate().map_err(ExportError)?;
    let mut template = parse_template(bytes)?;
    modifiers::strip_managed(&mut template, Some(reference))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BambuHeightObjectReadback {
    pub object_id: u32,
    pub object_ordinal: u32,
    pub ranges: Vec<limo_cad_core::BambuHeightRangeSnapshotDto>,
    pub profile: Option<Vec<limo_cad_core::PrintLayerHeightPointDto>>,
}

/// Read and verify native height metadata through stable normal-volume bindings.
/// This never strips/restores settings or infers generated toolpath behavior.
pub fn inspect_bambu_height_reference(
    bytes: &[u8],
    reference: &BambuRefreshReference,
) -> Result<Vec<BambuHeightObjectReadback>, ExportError> {
    reference.validate().map_err(ExportError)?;
    heights::verify_reference(&parse_template(bytes)?, reference)
}

pub fn verify_bambu_height_reference(
    bytes: &[u8],
    reference: &BambuRefreshReference,
) -> Result<(), ExportError> {
    inspect_bambu_height_reference(bytes, reference).map(|_| ())
}

/// Refresh an explicitly bound saved project using the same validated source meshes and solved poses as portable export.
pub fn write_bambu_project(
    template_bytes: &[u8],
    meshes: &[TriangleMesh],
    appearances: &[BodyAppearance],
    instances: &[MeshInstance],
    structure: &limo_cad_assembly::ComponentStructureDto,
    intent: &PrintIntentDocumentDto,
    request: &BambuProjectRequest,
) -> Result<BambuProjectExport, ExportError> {
    intent.validate().map_err(ExportError)?;
    for binding in &request.bindings {
        binding.validate().map_err(ExportError)?;
    }
    if let Some(reference) = &request.refresh_reference {
        reference.validate().map_err(ExportError)?;
    }
    if intent.source_document_id.as_deref() != Some(request.source_document_id.as_str()) {
        return fail(
            "Project refresh requires the current document's persistent source_document_id",
        );
    }
    if let Some(profile) = &intent.selected_process {
        if profile.status != ProcessProfileStatusDto::Resolved {
            return fail("Resolve the selected process profile before Bambu project export");
        }
    }
    let mut template = parse_template(template_bytes)?;
    // Inspection retains native options that CAD cannot author. Managed export
    // remains explicit about that boundary rather than silently inventing defaults.
    typed_profile_defaults(&template.profile)?;
    let reference = load_reference(&template, request)?;
    heights::strip_managed(&mut template, reference.as_ref())?;
    modifiers::strip_managed(&mut template, reference.as_ref())?;
    let (native_warnings, native_profile_changed) = check_reference_settings(
        &template,
        reference.as_ref(),
        request.accept_native_setting_changes,
    )?;
    if request.placement == BambuPlacementMode::ResolvedScene && template.summary.plate_count > 1 {
        return fail("Resolved named/CAD placement currently requires a single-plate template; choose explicit template placement to preserve a saved multi-plate layout, or a matching single-plate template");
    }
    if template
        .summary
        .objects
        .iter()
        .any(|o| o.parts.iter().any(|p| p.subtype != "normal_part"))
    {
        return fail("Existing print-only modifier/support/negative volumes require the coordinated modifier adapter before mesh refresh; use a normal-volume template for this export");
    }
    if let Some(profile) = &intent.selected_process {
        let supplied = settings_map(&profile.defaults);
        let actual = reference
            .as_ref()
            .filter(|_| !native_profile_changed)
            .map(|r| r.baseline_project_settings.clone())
            .unwrap_or_else(|| settings_map(&template.summary.process_defaults));
        if supplied
            .iter()
            .any(|(key, value)| actual.get(key) != Some(value))
        {
            return fail("Selected process snapshot defaults differ from the complete saved template; reselect its sourced defaults or put deliberate changes in project/part overrides");
        }
        if profile.name != template.summary.process_settings_id
            && profile.profile_id != template.summary.process_settings_id
        {
            return fail("Selected process profile does not match the complete saved template; choose a matching template");
        }
        if let Some(ProcessProfileSourceDto::SavedTemplate { sha256, .. }) = &profile.source {
            if sha256 != &template.summary.template_sha256
                && reference.as_ref().is_none_or(|reference| {
                    reference.original_template_sha256 != *sha256 || native_profile_changed
                })
            {
                return fail("Selected saved-template profile hash does not match this template or its verified unchanged profile lineage");
            }
        }
    }
    let bindings = resolve_bindings(&template, request, reference.as_ref())?;
    validate_grouping(structure, &bindings)?;
    let appearance_warnings = check_appearance(
        &template,
        &bindings,
        appearances,
        request.allow_template_appearance,
    )?;
    let source: BTreeMap<_, _> = meshes.iter().map(|m| (m.body_id, m)).collect();
    if source.len() != meshes.len() {
        return fail("Bambu export requires one source mesh per body");
    }
    let poses: BTreeMap<_, _> = instances
        .iter()
        .filter(|p| p.visible && source.contains_key(&p.body_id))
        .map(|p| Ok(((p.body_id, p.occurrence_id), Matrix::pose(p)?)))
        .collect::<Result<_, ExportError>>()?;
    if poses.len()
        != instances
            .iter()
            .filter(|p| p.visible && source.contains_key(&p.body_id))
            .count()
    {
        return fail("Ambiguous repeated CAD body/occurrence identity");
    }
    let supplied: BTreeSet<_> = bindings
        .iter()
        .map(|b| (b.body_id, b.occurrence_id))
        .collect();
    if supplied.len() != bindings.len() || supplied != poses.keys().copied().collect() {
        return fail("Bindings must cover every visible CAD body occurrence exactly once, including intentional repeats");
    }
    let expected: BTreeSet<_> = template
        .summary
        .objects
        .iter()
        .flat_map(|o| {
            (0..o.instance_count).flat_map(move |i| {
                o.parts
                    .iter()
                    .filter(|p| p.subtype == "normal_part")
                    .map(move |p| (o.object_id, i, p.part_id))
            })
        })
        .collect();
    let bound: BTreeSet<_> = bindings
        .iter()
        .map(|b| (b.object_id, b.instance_id, b.part_id))
        .collect();
    if bound.len() != bindings.len() || bound != expected {
        return fail("Bindings must cover each existing printable object-instance-volume exactly once; explicit regrouping requires a different template");
    }
    let mut welded = BTreeMap::new();
    for body in supplied
        .iter()
        .map(|(body, _)| *body)
        .collect::<BTreeSet<_>>()
    {
        let mesh = weld_triangle_mesh(source[&body], DEFAULT_WELD_EPSILON)?;
        validate_3mf_model_mesh(&mesh)?;
        welded.insert(body, mesh);
    }
    let mut edits: BTreeMap<String, Vec<(Range<usize>, String)>> = BTreeMap::new();
    let mut target_body = BTreeMap::new();
    let mut target_local = BTreeMap::new();
    let mut group_world = BTreeMap::new();
    for binding in &bindings {
        let key = (binding.object_id, binding.part_id);
        if target_body
            .insert(key, binding.body_id)
            .is_some_and(|previous| previous != binding.body_id)
        {
            return fail("A shared template volume cannot bind different source bodies across its repeated instances");
        }
        if request.placement == BambuPlacementMode::ResolvedScene {
            let world = poses[&(binding.body_id, binding.occurrence_id)];
            let group = *group_world
                .entry((binding.object_id, binding.instance_id))
                .or_insert(world);
            let local = group.inverse()?.compose(world);
            if target_local
                .insert(key, local)
                .is_some_and(|previous: Matrix| !previous.near(local))
            {
                return fail("Repeated template object has different relative CAD volume poses; choose a template with separate objects instead of silently changing grouping");
            }
        }
    }
    for ((object, part), body) in &target_body {
        let target = &template.targets[&(*object, *part)];
        let mesh = &welded[body];
        let offset = if request.placement == BambuPlacementMode::Template {
            mesh_center(mesh)
        } else {
            [0.; 3]
        };
        edits
            .entry(target.path.clone())
            .or_default()
            .push((target.mesh_range.clone(), mesh_xml(mesh, offset)));
        if request.placement == BambuPlacementMode::ResolvedScene {
            let local = target_local[&(*object, *part)];
            let root = text(&template.entries, ROOT)?;
            edits.entry(ROOT.into()).or_default().push((
                target.component_range.clone(),
                set_tag_attribute(
                    &root[target.component_range.clone()],
                    "transform",
                    &local.standard(),
                ),
            ));
        }
    }
    if request.placement == BambuPlacementMode::ResolvedScene {
        let root = text(&template.entries, ROOT)?;
        for (key, world) in &group_world {
            let range = template.build_ranges[key].clone();
            edits.entry(ROOT.into()).or_default().push((
                range.clone(),
                set_tag_attribute(&root[range], "transform", &world.standard()),
            ));
        }
    }
    apply_file_edits(&mut template.entries, edits)?;
    let (mut reports, baseline_project_settings, baseline_part_settings, baseline_object_settings) =
        update_config(
            &mut template,
            &bindings,
            &target_body,
            &welded,
            intent,
            reference.as_ref(),
            request.accept_native_setting_changes,
        )?;
    reports.sort_by_key(|p| p.binding.clone());
    let invalidated_entries = invalidate_derived(&mut template.entries)?;
    let original_template_sha256 = if native_profile_changed {
        template.summary.template_sha256.clone()
    } else {
        reference
            .as_ref()
            .map(|r| r.original_template_sha256.clone())
            .unwrap_or_else(|| template.summary.template_sha256.clone())
    };
    let refresh_reference = BambuRefreshReference {
        version: 1,
        source_document_id: request.source_document_id.clone(),
        original_template_sha256: original_template_sha256.clone(),
        profile_sha256: profile_hash(&template.profile),
        profile_identity_sha256: profile_identity_hash(&template.profile),
        baseline_project_settings: baseline_project_settings.clone(),
        written_project_settings: profile_settings(&template.profile),
        parts: bindings
            .iter()
            .map(|binding| {
                let part = template
                    .summary
                    .objects
                    .iter()
                    .find(|o| o.object_id == binding.object_id)
                    .unwrap()
                    .parts
                    .iter()
                    .find(|p| p.part_id == binding.part_id)
                    .unwrap();
                let uuid = part.uuid.clone().filter(|v| !v.is_empty()).ok_or_else(|| {
                    err("Saved target volume needs its persistent UUID for safe refresh")
                })?;
                let key = format!("{}:{}", binding.object_id, binding.part_id);
                let mut written = baseline_part_settings[&key].clone();
                let overrides = &reports
                    .iter()
                    .find(|p| p.binding == *binding)
                    .unwrap()
                    .written_overrides;
                if baseline_object_settings.contains_key(&binding.object_id) {
                    for key in overrides.keys() {
                        written.remove(key);
                    }
                } else {
                    written.extend(overrides.clone());
                }
                Ok(BambuRefreshPart {
                    binding: binding.clone(),
                    target_uuid: uuid,
                    instance_identify_id: template.instance_identities
                        [&(binding.object_id, binding.instance_id)],
                    baseline_part_settings: baseline_part_settings[&key].clone(),
                    written_part_settings: written,
                    baseline_object_settings: baseline_object_settings
                        .get(&binding.object_id)
                        .cloned(),
                    written_object_settings: baseline_object_settings.get(&binding.object_id).map(
                        |baseline| {
                            let mut written = baseline.clone();
                            written.extend(
                                reports
                                    .iter()
                                    .find(|p| p.binding == *binding)
                                    .unwrap()
                                    .written_overrides
                                    .clone(),
                            );
                            written
                        },
                    ),
                })
            })
            .collect::<Result<_, ExportError>>()?,
        modifiers: Vec::new(),
        height_objects: Vec::new(),
    };
    refresh_reference.validate().map_err(ExportError)?;
    template.entries.insert(
        MANIFEST.into(),
        serde_json::to_vec_pretty(&refresh_reference).map_err(err)?,
    );
    let bytes = write_archive(&template.entries)?;
    let parsed = parse_template(&bytes)?;
    populate_actual_report(&parsed, &mut reports)?;
    verify_readback(&parsed, &reports, &welded, &poses, request.placement)?;
    let mut warnings = vec!["Metadata readback is verified; installed slicer import, toolpaths and physical performance are not verified by this export.".into()];
    warnings.push("Replaced meshes have their stale external reload source and source transform metadata cleared; CAD geometry and the selected component/build placement are authoritative.".into());
    if parsed.summary.objects.iter().any(|object| {
        object
            .parts
            .iter()
            .filter(|part| part.subtype == "normal_part")
            .count()
            > 1
    }) {
        warnings.push("Multipart overrides remain native volume settings; inspect each volume's settings in Objects. Single-normal-volume CAD part requests are written at object controls without changing CAD grouping.".into());
    }
    warnings.extend(appearance_warnings);
    warnings.extend(native_warnings);
    let mut same_plate_instances = BTreeMap::new();
    for ((object, _), plate) in &parsed.plate_indices {
        *same_plate_instances
            .entry((*object, *plate))
            .or_insert(0usize) += 1;
    }
    if reference.is_none() && template.summary.has_identity_manifest {
        warnings.push("Explicitly reviewed bindings start a new CAD project lineage from a foreign authored template; current native settings are retained as the new baseline".into());
    }
    if same_plate_instances.values().any(|count| *count > 1) {
        warnings.push("Bambu Studio 2.8.2.61 rewrites later identities for repeated instances of one object on the same plate during native save; quantity and grouping are retained, but a later missing identify_id requires an explicitly reviewed rebind and a new inherited baseline".into());
    }
    if request.placement == BambuPlacementMode::Template {
        warnings.push("Explicit template placement keeps centered volumes and plate positions; current CAD/named-view poses are not used.".into());
    }
    let mut result = BambuProjectExport {
        report: BambuProjectReport {
            template: template.summary,
            source_document_id: request.source_document_id.clone(),
            output_sha256: hash(&bytes),
            placement: request.placement,
            parts: reports,
            modifiers: Vec::new(),
            z_preflight: Vec::new(),
            invalidated_entries,
            warnings,
            metadata_readback_verified: true,
            installed_slicer_imported: false,
            toolpaths_generated: false,
            refresh_reference,
        },
        bytes,
    };
    modifiers::append(&mut result, meshes, structure, intent)?;
    heights::append(&mut result, meshes, intent)?;
    z_preflight::populate(&mut result)?;
    Ok(result)
}

fn fail<T>(message: &str) -> Result<T, ExportError> {
    Err(ExportError(message.into()))
}
fn err(error: impl std::fmt::Display) -> ExportError {
    ExportError(format!("Bambu project: {error}"))
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn archive(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, ExportError> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT {
        return fail("Template must be a 3MF ZIP no larger than 128 MiB");
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(err)?;
    if zip.len() > 4096 {
        return fail("Template has too many archive entries");
    }
    let mut total = 0u64;
    let mut entries = BTreeMap::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(err)?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        safe_path(&name)?;
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| err("Archive size overflow"))?;
        if total > MAX_EXPANDED || entry.size() > MAX_INPUT as u64 {
            return fail("Template expanded data exceeds the archive limit");
        }
        if entries.contains_key(&name) {
            return fail("Duplicate template ZIP entry is ambiguous");
        }
        let mut value = Vec::new();
        entry
            .take(MAX_INPUT as u64 + 1)
            .read_to_end(&mut value)
            .map_err(err)?;
        if value.len() > MAX_INPUT {
            return fail("Template entry exceeds 128 MiB");
        }
        entries.insert(name, value);
    }
    Ok(entries)
}
fn safe_path(path: &str) -> Result<(), ExportError> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains(['\\', ':', '\0'])
        || path.starts_with('/')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return fail("Unsafe or ambiguous project archive path");
    }
    Ok(())
}
fn text<'a>(entries: &'a BTreeMap<String, Vec<u8>>, path: &str) -> Result<&'a str, ExportError> {
    std::str::from_utf8(
        entries
            .get(path)
            .ok_or_else(|| err(format!("Required saved-project entry missing: {path}")))?,
    )
    .map_err(err)
}
fn xml(value: &str) -> Result<Document<'_>, ExportError> {
    Document::parse(value).map_err(err)
}
fn node_id(node: Node<'_, '_>, name: &str) -> Result<u32, ExportError> {
    let id = node
        .attribute(name)
        .ok_or_else(|| err(format!("Missing {name}")))?
        .parse::<u32>()
        .map_err(err)?;
    if id == 0 {
        return fail("3MF resource IDs must be positive");
    }
    Ok(id)
}
fn metadata(node: Node<'_, '_>) -> Result<BTreeMap<String, String>, ExportError> {
    let mut values = BTreeMap::new();
    for child in node.children().filter(|n| n.has_tag_name("metadata")) {
        if let (Some(key), Some(value)) = (child.attribute("key"), child.attribute("value")) {
            if values.insert(key.into(), value.into()).is_some() {
                return fail("Duplicate Bambu settings metadata key");
            }
        }
    }
    Ok(values)
}
fn profile_string(profile: &Value, key: &str) -> Result<String, ExportError> {
    profile
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty() && v.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| err(format!("Complete saved template requires '{key}'")))
}
fn profile_strings(profile: &Value, key: &str) -> Result<Vec<String>, ExportError> {
    let array = profile
        .get(key)
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty() && a.len() <= 64)
        .ok_or_else(|| err(format!("Complete saved template requires '{key}' array")))?;
    array
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 4096)
                .map(str::to_owned)
                .ok_or_else(|| err(format!("Invalid '{key}' array value")))
        })
        .collect()
}
fn profile_u32(profile: &Value, key: &str) -> Result<u32, ExportError> {
    profile_string(profile, key)?.parse().map_err(err)
}

fn model_relationship_targets(
    entries: &BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Result<BTreeSet<String>, ExportError> {
    let Some(bytes) = entries.get(path) else {
        return Ok(BTreeSet::new());
    };
    let source = std::str::from_utf8(bytes).map_err(err)?;
    let document = xml(source)?;
    let mut targets = BTreeSet::new();
    for relationship in document.root_element().children().filter(|n| {
        n.has_tag_name((
            "http://schemas.openxmlformats.org/package/2006/relationships",
            "Relationship",
        ))
    }) {
        let kind = relationship.attribute("Type").unwrap_or_default();
        if kind.starts_with("http://schemas.microsoft.com/3dmanufacturing/")
            && kind.ends_with("3dmodel")
        {
            if relationship.attribute("TargetMode") == Some("External") {
                return fail(
                    "Saved template 3D model relationships must remain inside the package",
                );
            }
            let target = resolve_relationship(
                path,
                relationship
                    .attribute("Target")
                    .ok_or_else(|| err("3D model relationship missing target"))?,
            )?;
            if !entries.contains_key(&target) || !targets.insert(target) {
                return fail("Missing or duplicate saved-template 3D model relationship target");
            }
        }
    }
    Ok(targets)
}
fn parse_template(bytes: &[u8]) -> Result<Template, ExportError> {
    let entries = archive(bytes)?;
    let root_text = text(&entries, ROOT)?;
    let root = xml(root_text)?;
    if !root.root_element().has_tag_name((CORE_NS, "model"))
        || root.root_element().attribute("unit") != Some("millimeter")
    {
        return fail("Bambu template needs a millimeter 3MF model");
    }
    if !root.root_element().children().any(|n| {
        n.has_tag_name((CORE_NS, "metadata"))
            && n.attribute("name") == Some("Application")
            && n.text().is_some_and(|s| s.starts_with("BambuStudio"))
    }) {
        return fail("Choose a complete saved Bambu project, not a geometry-only 3MF");
    }
    let profile: Value = serde_json::from_str(text(&entries, PROFILE)?).map_err(err)?;
    if profile.as_object().is_none() {
        return fail("Project settings must be a complete JSON object");
    }
    let version = profile_string(&profile, "version")?;
    if !["02.08.02.61", "2.8.2.61"].contains(&version.as_str()) {
        return fail("Bambu project adapter is qualified for saved-project version 2.8.2.61; save the template with this supported version");
    }
    for key in [
        "printer_technology",
        "gcode_flavor",
        "machine_start_gcode",
        "machine_end_gcode",
        "printable_height",
        "layer_height",
        "initial_layer_print_height",
        "wall_loops",
        "sparse_infill_density",
        "sparse_infill_pattern",
        "top_shell_layers",
        "bottom_shell_layers",
    ] {
        if profile.get(key).and_then(Value::as_str).is_none() {
            return Err(err(format!("Complete saved template is missing '{key}'; bed envelopes and thin profiles are not full slicer profiles")));
        }
    }
    if profile_string(&profile, "printer_technology")? != "FFF" {
        return fail("Only saved FFF Bambu projects are supported");
    }
    let nozzle_diameter_mm = profile_strings(&profile, "nozzle_diameter")?
        .iter()
        .map(|s| s.parse::<f64>().map_err(err))
        .collect::<Result<Vec<_>, _>>()?;
    if nozzle_diameter_mm
        .iter()
        .any(|v| !v.is_finite() || *v <= 0. || *v > 10.)
    {
        return fail("Invalid selected printer nozzle diameters");
    }
    let filament_settings_ids = profile_strings(&profile, "filament_settings_id")?;
    let filament_types = profile_strings(&profile, "filament_type")?;
    let filament_colors = profile_strings(&profile, "filament_colour")?;
    let filament_count = filament_settings_ids.len();
    if profile_strings(&profile, "filament_diameter")?.len() != filament_count {
        return fail("Incomplete filament diameter mapping");
    }
    let temperature_count = profile_strings(&profile, "nozzle_temperature")?.len();
    if profile_strings(&profile, "nozzle_temperature_initial_layer")?.len() != temperature_count {
        return fail("Initial/normal nozzle temperature variant mappings differ");
    }
    if temperature_count != filament_count {
        let self_indices = profile_strings(&profile, "filament_self_index")?;
        let variants = profile_strings(&profile, "filament_extruder_variant")?;
        if self_indices.len() != temperature_count || variants.len() != temperature_count {
            return fail("Expanded filament temperature settings require complete self-index/extruder-variant mappings");
        }
        let ids = self_indices
            .iter()
            .map(|v| v.parse::<usize>().map_err(err))
            .collect::<Result<Vec<_>, _>>()?;
        if ids.iter().any(|id| *id == 0 || *id > filament_count)
            || ids.into_iter().collect::<BTreeSet<_>>() != (1..=filament_count).collect()
        {
            return fail("Filament variant self-index mapping is incomplete or outside the selected profile slots");
        }
    }
    if filament_types.len() != filament_count || filament_colors.len() != filament_count {
        return fail("Filament settings/type/color mappings have different lengths");
    }
    let support_filament = profile_u32(&profile, "support_filament")?;
    let support_interface_filament = profile_u32(&profile, "support_interface_filament")?;
    if support_filament as usize > filament_count
        || support_interface_filament as usize > filament_count
    {
        return fail("Support filament mapping refers to an unavailable filament profile");
    }
    let filament_map = profile
        .get("filament_map")
        .cloned()
        .ok_or_else(|| err("Template missing filament_map"))?;
    let filament_nozzle_map = profile
        .get("filament_nozzle_map")
        .cloned()
        .ok_or_else(|| err("Template missing filament_nozzle_map"))?;
    if filament_map
        .as_array()
        .is_none_or(|a| a.len() != filament_count)
        || filament_nozzle_map
            .as_array()
            .is_none_or(|a| a.len() != filament_count)
    {
        return fail("Filament/nozzle map lengths must match complete filament profiles");
    }
    for value in filament_nozzle_map.as_array().unwrap() {
        let index = value
            .as_str()
            .and_then(|v| v.parse::<usize>().ok())
            .or_else(|| value.as_u64().and_then(|v| usize::try_from(v).ok()))
            .ok_or_else(|| err("Invalid filament_nozzle_map index"))?;
        if index >= nozzle_diameter_mm.len() {
            return fail("Filament references an unavailable physical nozzle");
        }
    }
    let root_relationships = model_relationship_targets(&entries, "_rels/.rels")?;
    if root_relationships != BTreeSet::from([ROOT.to_owned()]) {
        return fail("Saved template package must identify its one root 3D model");
    }
    let sub_models = model_relationship_targets(&entries, "3D/_rels/3dmodel.model.rels")?;
    let config_text = text(&entries, CONFIG)?;
    let config = xml(config_text)?;
    if !config.root_element().has_tag_name("config") {
        return fail("Invalid saved-project model settings root");
    }
    let resource = root
        .root_element()
        .children()
        .find(|n| n.has_tag_name((CORE_NS, "resources")))
        .ok_or_else(|| err("No model resources"))?;
    let mut objects = BTreeMap::new();
    for object in resource
        .children()
        .filter(|n| n.has_tag_name((CORE_NS, "object")))
    {
        if objects.insert(node_id(object, "id")?, object).is_some() {
            return fail("Duplicate root model resource ID");
        }
    }
    let build = root
        .root_element()
        .children()
        .find(|n| n.has_tag_name((CORE_NS, "build")))
        .ok_or_else(|| err("No model build"))?;
    let mut build_ranges = BTreeMap::new();
    let mut object_order = Vec::new();
    let mut counts = BTreeMap::<u32, u32>::new();
    for item in build
        .children()
        .filter(|n| n.has_tag_name((CORE_NS, "item")))
    {
        if !matches!(item.attribute("printable"), None | Some("1")) {
            return fail("Saved Bambu build instances must have absent or literal 1 printable attributes; remove non-printable instances or review ambiguous boolean values before refresh");
        }
        if item.attributes().any(|a| a.name() == "path") {
            return fail("External builditem objects are not supported; use a saved Bambu root object template");
        }
        let id = node_id(item, "objectid")?;
        if !objects.contains_key(&id) {
            return fail("Build references missing root resource");
        }
        Matrix::parse(item.attribute("transform"))?;
        let count = counts.entry(id).or_default();
        if *count == 0 {
            object_order.push(id);
        }
        build_ranges.insert((id, *count), item.range());
        *count += 1;
    }
    if object_order.is_empty() {
        return fail("No printable Bambu objects");
    }
    let mut config_objects = BTreeMap::new();
    for object in config
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("object"))
    {
        if config_objects
            .insert(node_id(object, "id")?, object)
            .is_some()
        {
            return fail("Duplicate model_settings object ID");
        }
    }
    if config_objects.keys().copied().collect::<BTreeSet<_>>() != counts.keys().copied().collect() {
        return fail("Model settings and printable root objects do not match");
    }
    let mut targets = BTreeMap::new();
    let mut summary_objects = Vec::new();
    let mut used_meshes = BTreeSet::new();
    for (ordinal, id) in object_order.iter().enumerate() {
        let model = objects[id];
        let components = model
            .children()
            .find(|n| n.has_tag_name((CORE_NS, "components")))
            .ok_or_else(|| err("Saved Bambu object must contain explicit component volumes"))?;
        let settings = metadata(config_objects[id])?;
        let mut parts = Vec::new();
        let part_config: BTreeMap<_, _> = config_objects[id]
            .children()
            .filter(|n| n.has_tag_name("part"))
            .map(|n| Ok((node_id(n, "id")?, n)))
            .collect::<Result<_, ExportError>>()?;
        if part_config.len()
            != config_objects[id]
                .children()
                .filter(|n| n.has_tag_name("part"))
                .count()
        {
            return fail("Duplicate template part metadata IDs");
        }
        for component in components
            .children()
            .filter(|n| n.has_tag_name((CORE_NS, "component")))
        {
            let part_id = node_id(component, "objectid")?;
            let path = component
                .attributes()
                .find(|a| a.name() == "path")
                .map(|a| a.value().strip_prefix('/').unwrap_or(a.value()))
                .unwrap_or(ROOT)
                .to_string();
            safe_path(&path)?;
            if path != ROOT && !sub_models.contains(&path) {
                return fail("Saved template component model is missing its 3D model relationship; resave a complete Bambu project");
            }
            let model_doc = xml(text(&entries, &path)?)?;
            let mesh_objects: Vec<_> = model_doc
                .descendants()
                .filter(|n| {
                    n.has_tag_name((CORE_NS, "object"))
                        && n.attribute("id") == Some(part_id.to_string().as_str())
                })
                .collect();
            if mesh_objects.len() != 1 {
                return fail("Missing or ambiguous Bambu component mesh resource");
            }
            let mesh=mesh_objects[0].children().find(|n|n.has_tag_name((CORE_NS,"mesh"))).ok_or_else(||err("Nested template components are not supported by this qualified adapter; save flattened normal-volume Bambu objects without changing multipart grouping"))?;
            let part = *part_config
                .get(&part_id)
                .ok_or_else(|| err("Model settings missing component part"))?;
            let subtype = part.attribute("subtype").unwrap_or("normal_part");
            if ![
                "normal_part",
                "modifier_part",
                "negative_part",
                "support_enforcer",
                "support_blocker",
            ]
            .contains(&subtype)
            {
                return fail("Unknown Bambu volume subtype");
            }
            if !used_meshes.insert((path.clone(), part_id)) {
                return fail("Template shares a mesh resource across objects/volumes; explicit separate volume resources are required for safe independent refresh");
            }
            let part_settings = metadata(part)?;
            parts.push(BambuTemplatePart {
                part_id,
                uuid: part.attribute("uuid").map(str::to_owned),
                name: part_settings.get("name").cloned().unwrap_or_default(),
                mesh_path: path.clone(),
                subtype: subtype.into(),
                settings: part_settings,
            });
            if targets
                .insert(
                    (*id, part_id),
                    PartTarget {
                        path,
                        mesh_range: mesh.range(),
                        component_range: component.range(),
                        component_transform: Matrix::parse(component.attribute("transform"))?,
                    },
                )
                .is_some()
            {
                return fail("Ambiguous repeated part IDs in a template object");
            }
        }
        if parts.len() != part_config.len() || !parts.iter().any(|p| p.subtype == "normal_part") {
            return fail("Part metadata does not match the object's printable volumes");
        }
        summary_objects.push(BambuTemplateObject {
            object_id: *id,
            object_ordinal: ordinal as u32 + 1,
            name: settings.get("name").cloned().unwrap_or_default(),
            instance_count: counts[id],
            parts,
            settings,
        });
    }
    let plates: Vec<_> = config
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("plate"))
        .collect();
    if plates.is_empty() {
        return fail("Saved project has no explicit Bambu plate metadata");
    }
    let mut plated = BTreeSet::new();
    let mut instance_identities = BTreeMap::new();
    let mut plate_indices = BTreeMap::new();
    let mut plate_ids = BTreeSet::new();
    for plate in &plates {
        let values = metadata(*plate)?;
        let plate_id = values
            .get("plater_id")
            .ok_or_else(|| err("Plate metadata needs plater_id"))?
            .parse::<usize>()
            .map_err(err)?;
        if !plate_ids.insert(plate_id) {
            return fail("Duplicate plate identity");
        }
        for instance in plate
            .children()
            .filter(|n| n.has_tag_name("model_instance"))
        {
            let data = metadata(instance)?;
            let obj = data
                .get("object_id")
                .ok_or_else(|| err("Plate instance has no object_id"))?
                .parse::<u32>()
                .map_err(err)?;
            let idx = data
                .get("instance_id")
                .ok_or_else(|| err("Plate instance has no instance_id"))?
                .parse::<u32>()
                .map_err(err)?;
            if !plated.insert((obj, idx)) || !build_ranges.contains_key(&(obj, idx)) {
                return fail("Plate references duplicate or missing printable instances");
            }
            let identify = data
                .get("identify_id")
                .ok_or_else(|| err("Saved plate instance needs its stable identify_id"))?
                .parse::<u32>()
                .map_err(err)?;
            if identify == 0 {
                return fail("Saved plate instance identify_id must be positive for safe refresh");
            }
            instance_identities.insert((obj, idx), identify);
            plate_indices.insert((obj, idx), plate_id as u32);
        }
    }
    if plated != build_ranges.keys().copied().collect() {
        return fail("Every template instance must belong to exactly one plate");
    }
    if plate_ids != (1..=plates.len()).collect() {
        return fail("Saved Bambu plate IDs must be contiguous starting at one");
    }
    let (process_defaults, process_capability_warnings) = inspect_profile_defaults(&profile)?;
    let summary = BambuTemplateSummary {
        template_sha256: hash(bytes),
        version,
        printer_settings_id: profile_string(&profile, "printer_settings_id")?,
        printer_model: profile_string(&profile, "printer_model")?,
        printer_variant: profile_string(&profile, "printer_variant")?,
        process_settings_id: profile_string(&profile, "print_settings_id")?,
        process_defaults,
        native_process_settings: native_process_settings(&profile)?,
        process_capability_warnings,
        nozzle_diameter_mm,
        filament_settings_ids,
        filament_types,
        filament_colors,
        support_filament,
        support_interface_filament,
        filament_map,
        filament_nozzle_map,
        plate_count: plates.len(),
        objects: summary_objects,
        has_identity_manifest: entries.contains_key(MANIFEST),
    };
    Ok(Template {
        entries,
        summary,
        targets,
        build_ranges,
        profile,
        instance_identities,
        plate_indices,
    })
}

fn load_reference(
    template: &Template,
    request: &BambuProjectRequest,
) -> Result<Option<BambuRefreshReference>, ExportError> {
    let reference = if let Some(reference) = &request.refresh_reference {
        Some(reference.clone())
    } else {
        template
            .entries
            .get(MANIFEST)
            .map(|bytes| serde_json::from_slice::<BambuRefreshReference>(bytes).map_err(err))
            .transpose()?
    };
    if let Some(reference) = &reference {
        reference.validate().map_err(ExportError)?;
        if request.refresh_reference.is_none()
            && !request.bindings.is_empty()
            && reference.source_document_id != request.source_document_id
        {
            // Explicit bindings start a new CAD lineage; current native settings become its baseline.
            return Ok(None);
        }
        if reference.version != 1
            || reference.source_document_id != request.source_document_id
            || reference.parts.is_empty()
            || reference.parts.len() > 4096
        {
            return fail("Refresh reference belongs to another CAD document or is unsupported; explicitly review bindings");
        }
        for digest in [
            &reference.original_template_sha256,
            &reference.profile_sha256,
            &reference.profile_identity_sha256,
        ] {
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return fail("Malformed refresh reference source hash");
            }
        }
        validate_reference_settings(&reference.baseline_project_settings, true)?;
        validate_reference_settings(&reference.written_project_settings, true)?;
        let mut sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for part in &reference.parts {
            if part.target_uuid.is_empty()
                || part.target_uuid.len() > 256
                || part.instance_identify_id == 0
                || !sources.insert((part.binding.body_id, part.binding.occurrence_id))
                || !targets.insert((part.target_uuid.clone(), part.instance_identify_id))
            {
                return fail(
                    "Ambiguous or missing stable volume/instance identity in refresh reference",
                );
            }
            validate_reference_settings(&part.baseline_part_settings, false)?;
            validate_reference_settings(&part.written_part_settings, false)?;
        }
        if reference.profile_identity_sha256 != profile_identity_hash(&template.profile) {
            return fail("Saved template printer/process/material mapping changed; select and review a matching complete template again");
        }
    }
    Ok(reference)
}
fn validate_reference_settings(settings: &Settings, complete: bool) -> Result<(), ExportError> {
    if settings.len() > 5
        || settings
            .keys()
            .any(|key| !SETTING_KEYS.contains(&key.as_str()))
    {
        return fail(
            "Refresh reference may contain only the five supported manufacturing settings",
        );
    }
    let mut full = BTreeMap::from([
        ("wall_loops".into(), "2".into()),
        ("sparse_infill_density".into(), "15%".into()),
        ("sparse_infill_pattern".into(), "gyroid".into()),
        ("top_shell_layers".into(), "5".into()),
        ("bottom_shell_layers".into(), "3".into()),
    ]);
    if complete && settings.len() != 5 {
        return fail("Refresh reference missing process defaults");
    }
    for (key, value) in settings {
        if value.len() > 64 {
            return fail("Refresh reference setting is too long");
        }
        full.insert(key.clone(), value.clone());
    }
    if !complete
        && settings.contains_key("sparse_infill_density")
        && !settings.contains_key("sparse_infill_pattern")
    {
        full.insert("sparse_infill_pattern".into(), "zig-zag".into());
    }
    let value = serde_json::to_value(full).map_err(err)?;
    typed_profile_defaults(&value)?;
    Ok(())
}
fn profile_identity_hash(profile: &Value) -> String {
    let keys = [
        "printer_settings_id",
        "printer_model",
        "printer_variant",
        "print_settings_id",
        "nozzle_diameter",
        "nozzle_type",
        "nozzle_volume",
        "filament_settings_id",
        "filament_type",
        "filament_colour",
        "filament_diameter",
        "filament_self_index",
        "filament_extruder_variant",
        "support_filament",
        "support_interface_filament",
        "filament_map",
        "filament_nozzle_map",
    ];
    let selected: BTreeMap<_, _> = keys
        .into_iter()
        .filter_map(|key| profile.get(key).map(|v| (key, v.clone())))
        .collect();
    hash(&serde_json::to_vec(&selected).expect("JSON Value serialization"))
}
fn settings_equal(left: &Settings, right: &Settings) -> bool {
    if left.keys().ne(right.keys()) {
        return false;
    }
    left.iter().all(|(key, value)| {
        let other = &right[key];
        if key == "sparse_infill_density" {
            value.trim_end_matches('%').parse::<f64>().ok()
                == other.trim_end_matches('%').parse::<f64>().ok()
        } else if key == "sparse_infill_pattern" {
            value == other
        } else {
            value.parse::<u32>().ok() == other.parse::<u32>().ok()
        }
    })
}
fn resolved_reference_binding(
    template: &Template,
    part: &BambuRefreshPart,
) -> Result<BambuPartBinding, ExportError> {
    let mut matches = Vec::new();
    for object in &template.summary.objects {
        for volume in &object.parts {
            if volume.uuid.as_deref() == Some(part.target_uuid.as_str())
                && volume.subtype == "normal_part"
            {
                for ((object_id, instance_id), identify_id) in &template.instance_identities {
                    if *object_id == object.object_id && *identify_id == part.instance_identify_id {
                        matches.push(BambuPartBinding {
                            object_id: *object_id,
                            instance_id: *instance_id,
                            part_id: volume.part_id,
                            ..part.binding.clone()
                        });
                    }
                }
            }
        }
    }
    if matches.len() != 1 {
        let candidates: Vec<_> = template
            .summary
            .objects
            .iter()
            .flat_map(|object| {
                object
                    .parts
                    .iter()
                    .filter(|volume| volume.uuid.as_deref() == Some(part.target_uuid.as_str()))
                    .flat_map(move |_| {
                        template
                            .instance_identities
                            .iter()
                            .filter(move |((id, _), _)| *id == object.object_id)
                            .map(|((object, instance), identify)| {
                                format!(
                                    "object={object}, instance={instance}, identify_id={identify}"
                                )
                            })
                    })
            })
            .collect();
        return Err(err(format!("CAD body {} occurrence {} expected volume UUID {} with identify_id {}; target identity is missing or ambiguous after native save. Native candidate bindings: [{}]. Inspect and explicitly review each binding; clearing the old reference adopts a new inherited baseline, without name/order matching",part.binding.body_id.0,part.binding.occurrence_id,part.target_uuid,part.instance_identify_id,candidates.join("; "))));
    }
    Ok(matches.remove(0))
}
fn check_reference_settings(
    template: &Template,
    reference: Option<&BambuRefreshReference>,
    accept: bool,
) -> Result<(Vec<String>, bool), ExportError> {
    let Some(reference) = reference else {
        return Ok((Vec::new(), false));
    };
    let mut warnings = Vec::new();
    let global_changed = !settings_equal(
        &profile_settings(&template.profile),
        &reference.written_project_settings,
    );
    if global_changed {
        warnings.push("Native process defaults changed outside CAD; reviewed values become the new inherited baseline and require a matching sourced process snapshot".into());
    }
    for part in &reference.parts {
        let binding = resolved_reference_binding(template, part)?;
        let volume = template
            .summary
            .objects
            .iter()
            .find(|o| o.object_id == binding.object_id)
            .unwrap()
            .parts
            .iter()
            .find(|p| p.part_id == binding.part_id)
            .unwrap();
        if let Some(written) = &part.written_object_settings {
            let object = template
                .summary
                .objects
                .iter()
                .find(|o| o.object_id == binding.object_id)
                .unwrap();
            if object
                .parts
                .iter()
                .filter(|p| p.subtype == "normal_part")
                .count()
                != 1
            {
                return fail("A promoted single-volume object now has different grouping; review its target bindings before refresh");
            }
            if !settings_equal(&select_settings(&object.settings), written) {
                warnings.push(format!("Native object settings changed for CAD body {} occurrence {}; inspect and review the new inherited baseline", binding.body_id.0, binding.occurrence_id));
            }
        }
        if !settings_equal(
            &select_settings(&volume.settings),
            &part.written_part_settings,
        ) {
            warnings.push(format!("Native volume settings changed for CAD body {} occurrence {}; reviewed values become its inherited baseline",binding.body_id.0,binding.occurrence_id));
        }
    }
    if !warnings.is_empty() && !accept {
        return fail("Native settings differ from the previous CAD handoff; inspect and explicitly accept native setting changes before refresh");
    }
    let current_hash = profile_hash(&template.profile);
    if current_hash != reference.profile_sha256 {
        warnings.push(format!("Complete template profile changed (previous {}, current {}); unknown native fields are preserved, and this metadata check does not qualify their toolpaths",reference.profile_sha256,current_hash));
    }
    Ok((warnings, global_changed))
}
fn resolve_bindings(
    template: &Template,
    request: &BambuProjectRequest,
    reference: Option<&BambuRefreshReference>,
) -> Result<Vec<BambuPartBinding>, ExportError> {
    if let Some(reference) = reference {
        let resolved = reference
            .parts
            .iter()
            .map(|part| resolved_reference_binding(template, part))
            .collect::<Result<Vec<_>, _>>()?;
        if !request.bindings.is_empty() {
            let a: BTreeSet<_> = request.bindings.iter().cloned().collect();
            let b: BTreeSet<_> = resolved.iter().cloned().collect();
            if a != b || a.len() != request.bindings.len() {
                return fail("Explicit bindings conflict with stable refresh reference; review the target mapping");
            }
        }
        return Ok(resolved);
    }
    if request.bindings.is_empty() {
        return fail("Foreign saved template requires explicit CAD-to-object/instance/volume bindings; names are never matched automatically");
    }
    if request.bindings.len() > 4096 {
        return fail("Too many template bindings");
    }
    Ok(request.bindings.clone())
}

pub(crate) const SETTING_KEYS: [&str; 5] = [
    "wall_loops",
    "sparse_infill_density",
    "sparse_infill_pattern",
    "top_shell_layers",
    "bottom_shell_layers",
];
/// A bounded read-only handoff view, not an extensible CAD mutation registry.
/// Native scalar/vector spelling is retained so extruder variants and percent
/// widths are not collapsed into guessed physical values.
pub(crate) const NATIVE_PROCESS_KEYS: [&str; 23] = [
    "wall_loops",
    "sparse_infill_density",
    "sparse_infill_pattern",
    "top_shell_layers",
    "bottom_shell_layers",
    "layer_height",
    "initial_layer_print_height",
    "line_width",
    "initial_layer_line_width",
    "outer_wall_line_width",
    "inner_wall_line_width",
    "top_surface_line_width",
    "sparse_infill_line_width",
    "internal_solid_infill_line_width",
    "support_line_width",
    "top_shell_thickness",
    "bottom_shell_thickness",
    "enable_support",
    "support_type",
    "support_style",
    "support_threshold_angle",
    "support_on_build_plate_only",
    "enable_prime_tower",
];

fn validate_native_report_value(key: &str, value: &Value) -> Result<usize, ExportError> {
    let scalar = |value: &Value| match value {
        Value::String(value) => value.len() <= 256,
        Value::Bool(_) | Value::Number(_) | Value::Null => true,
        _ => false,
    };
    let valid = match value {
        Value::Array(values) => values.len() <= 128 && values.iter().all(scalar),
        value => scalar(value),
    };
    if !valid {
        return Err(err(format!(
            "Native read-only setting '{key}' exceeds the bounded scalar/vector report capability"
        )));
    }
    // Serialization is bounded above by the scalar/vector limits before it runs.
    Ok(serde_json::to_vec(value).map_err(err)?.len() + key.len() + 4)
}

fn native_process_settings(profile: &Value) -> Result<BTreeMap<String, Value>, ExportError> {
    let mut values = BTreeMap::new();
    let mut report_bytes = 0;
    for key in NATIVE_PROCESS_KEYS {
        if let Some(value) = profile.get(key) {
            report_bytes += validate_native_report_value(key, value)?;
            if report_bytes > 4096 {
                return fail("Native read-only process settings exceed the 4 KiB report capability; the saved project remains unchanged");
            }
            values.insert(key.into(), value.clone());
        }
    }
    Ok(values)
}

type NativeScopedSettings = (
    BTreeMap<String, Value>,
    BTreeMap<String, BambuSettingOrigin>,
);

pub(crate) fn native_effective_settings(
    process: &BTreeMap<String, Value>,
    object: &BTreeMap<String, String>,
    volume: &BTreeMap<String, String>,
) -> Result<NativeScopedSettings, ExportError> {
    let mut values = process.clone();
    let mut sources = values
        .keys()
        .map(|key| (key.clone(), BambuSettingOrigin::TemplateProcess))
        .collect::<BTreeMap<_, _>>();
    for (scope, origin) in [
        (object, BambuSettingOrigin::TemplateObject),
        (volume, BambuSettingOrigin::TemplateVolume),
    ] {
        for key in NATIVE_PROCESS_KEYS {
            if let Some(value) = scope.get(key) {
                if value.len() > 256 {
                    return Err(err(format!(
                        "Native read-only setting '{key}' exceeds the bounded scalar report capability"
                    )));
                }
                values.insert(key.into(), Value::String(value.clone()));
                sources.insert(key.into(), origin);
            }
        }
    }
    Ok((values, sources))
}
pub(crate) fn settings_map(settings: &PrintSettingsDto) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    if let Some(v) = settings.wall_count {
        values.insert("wall_loops".into(), v.to_string());
    }
    if let Some(v) = settings.infill_density_percent {
        values.insert("sparse_infill_density".into(), format!("{v}%"));
    }
    if let Some(v) = settings.top_shell_layers {
        values.insert("top_shell_layers".into(), v.to_string());
    }
    if let Some(v) = settings.bottom_shell_layers {
        values.insert("bottom_shell_layers".into(), v.to_string());
    }
    if let Some(v) = settings.infill_pattern {
        values.insert(
            "sparse_infill_pattern".into(),
            match v {
                InfillPatternDto::Grid => "grid",
                InfillPatternDto::Gyroid => "gyroid",
                InfillPatternDto::Rectilinear => "zig-zag",
                InfillPatternDto::Concentric => "concentric",
                InfillPatternDto::Cubic => "cubic",
                InfillPatternDto::Honeycomb => "honeycomb",
                InfillPatternDto::Lightning => "lightning",
            }
            .into(),
        );
    }
    values
}
fn select_settings(values: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    values
        .iter()
        .filter(|(key, _)| SETTING_KEYS.contains(&key.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
fn profile_settings(profile: &Value) -> BTreeMap<String, String> {
    SETTING_KEYS
        .iter()
        .filter_map(|key| {
            profile
                .get(*key)
                .and_then(Value::as_str)
                .map(|v| ((*key).into(), v.into()))
        })
        .collect()
}
fn inspect_profile_defaults(
    profile: &Value,
) -> Result<(PrintSettingsDto, Vec<String>), ExportError> {
    let density = profile_string(profile, "sparse_infill_density")?;
    let pattern = profile_string(profile, "sparse_infill_pattern")?;
    if pattern.is_empty() || pattern.len() > 64 || pattern.chars().any(char::is_control) {
        return fail("Native infill pattern must be a nonempty bounded option name");
    }
    let infill_pattern = match pattern.as_str() {
        "grid" => Some(InfillPatternDto::Grid),
        "gyroid" => Some(InfillPatternDto::Gyroid),
        "zig-zag" => Some(InfillPatternDto::Rectilinear),
        "concentric" => Some(InfillPatternDto::Concentric),
        "cubic" => Some(InfillPatternDto::Cubic),
        "honeycomb" => Some(InfillPatternDto::Honeycomb),
        "lightning" => Some(InfillPatternDto::Lightning),
        _ => None,
    };
    let defaults = PrintSettingsDto {
        wall_count: Some(profile_u32(profile, "wall_loops")?),
        infill_density_percent: Some(
            density
                .strip_suffix('%')
                .ok_or_else(|| err("Template infill density needs a percentage"))?
                .parse()
                .map_err(err)?,
        ),
        infill_pattern,
        top_shell_layers: Some(profile_u32(profile, "top_shell_layers")?),
        bottom_shell_layers: Some(profile_u32(profile, "bottom_shell_layers")?),
    };
    defaults.validate().map_err(ExportError)?;
    let warnings = if infill_pattern.is_none() {
        vec![format!(
            "Native sparse_infill_pattern {pattern:?} is retained for read-only inspection; typed CAD process editing and managed export do not support this value. Native project import and toolpaths are not verified by inspection."
        )]
    } else {
        Vec::new()
    };
    Ok((defaults, warnings))
}
fn typed_profile_defaults(profile: &Value) -> Result<PrintSettingsDto, ExportError> {
    let (defaults, warnings) = inspect_profile_defaults(profile)?;
    if let Some(warning) = warnings.first() {
        return Err(err(format!(
            "Managed Bambu export cannot represent this process: {warning}"
        )));
    }
    Ok(defaults)
}
fn profile_hash(profile: &Value) -> String {
    hash(&serde_json::to_vec(profile).expect("JSON Value serialization"))
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn close_position(source: &str, node: Node<'_, '_>) -> Result<usize, ExportError> {
    let range = node.range();
    source[range.clone()]
        .rfind("</")
        .map(|v| range.start + v)
        .ok_or_else(|| err("Target XML node cannot be self-closing"))
}
fn edit_metadata(
    source: &str,
    node: Node<'_, '_>,
    updates: &BTreeMap<String, Option<String>>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), ExportError> {
    let mut missing = updates.clone();
    for child in node.children().filter(|n| n.has_tag_name("metadata")) {
        if let Some(key) = child.attribute("key") {
            if let Some(value) = missing.remove(key) {
                edits.push((
                    child.range(),
                    value
                        .map(|value| {
                            format!(
                                "<metadata key=\"{}\" value=\"{}\"/>",
                                escape(key),
                                escape(&value)
                            )
                        })
                        .unwrap_or_default(),
                ));
            }
        }
    }
    let addition = missing
        .into_iter()
        .filter_map(|(key, value)| {
            value.map(|value| {
                format!(
                    "<metadata key=\"{}\" value=\"{}\"/>",
                    escape(&key),
                    escape(&value)
                )
            })
        })
        .collect::<String>();
    if !addition.is_empty() {
        let offset = close_position(source, node)?;
        edits.push((offset..offset, addition));
    }
    Ok(())
}
fn adopt_changed_settings(
    baseline: &mut Settings,
    current: &Settings,
    written: &Settings,
    accept: bool,
) {
    if !accept {
        return;
    }
    for key in SETTING_KEYS {
        let left = current
            .get(key)
            .map(|v| BTreeMap::from([(key.to_owned(), v.clone())]))
            .unwrap_or_default();
        let right = written
            .get(key)
            .map(|v| BTreeMap::from([(key.to_owned(), v.clone())]))
            .unwrap_or_default();
        if !settings_equal(&left, &right) {
            if let Some(value) = current.get(key) {
                baseline.insert(key.to_owned(), value.clone());
            } else {
                baseline.remove(key);
            }
        }
    }
}
type Settings = BTreeMap<String, String>;
type PartBaselines = BTreeMap<String, Settings>;
type ConfigUpdate = (
    Vec<BambuPartReport>,
    Settings,
    PartBaselines,
    BTreeMap<u32, Settings>,
);
fn update_config(
    template: &mut Template,
    bindings: &[BambuPartBinding],
    target_body: &BTreeMap<(u32, u32), BodyId>,
    meshes: &BTreeMap<BodyId, TriangleMesh>,
    intent: &PrintIntentDocumentDto,
    reference: Option<&BambuRefreshReference>,
    accept_native_changes: bool,
) -> Result<ConfigUpdate, ExportError> {
    let current_project = profile_settings(&template.profile);
    let mut baseline_project = reference
        .map(|r| r.baseline_project_settings.clone())
        .unwrap_or_else(|| current_project.clone());
    if let Some(reference) = reference {
        adopt_changed_settings(
            &mut baseline_project,
            &current_project,
            &reference.written_project_settings,
            accept_native_changes,
        );
    }
    let mut baseline_parts = PartBaselines::new();
    let mut baseline_objects = BTreeMap::new();
    for object in &template.summary.objects {
        if object
            .parts
            .iter()
            .filter(|p| p.subtype == "normal_part")
            .count()
            == 1
        {
            let previous = reference.and_then(|r| {
                r.parts.iter().find(|p| {
                    resolved_reference_binding(template, p)
                        .is_ok_and(|b| b.object_id == object.object_id)
                })
            });
            let current = select_settings(&object.settings);
            let mut baseline = previous
                .and_then(|p| p.baseline_object_settings.clone())
                .unwrap_or_else(|| current.clone());
            if let Some(written) = previous.and_then(|p| p.written_object_settings.as_ref()) {
                adopt_changed_settings(&mut baseline, &current, written, accept_native_changes);
            }
            baseline_objects.insert(object.object_id, baseline);
        }
        for part in object.parts.iter().filter(|p| p.subtype == "normal_part") {
            let current = select_settings(&part.settings);
            let previous = reference.and_then(|r| {
                r.parts.iter().find(|p| {
                    resolved_reference_binding(template, p)
                        .is_ok_and(|b| b.object_id == object.object_id && b.part_id == part.part_id)
                })
            });
            let mut baseline = previous
                .map(|p| p.baseline_part_settings.clone())
                .unwrap_or_else(|| current.clone());
            if let Some(previous) = previous {
                adopt_changed_settings(
                    &mut baseline,
                    &current,
                    &previous.written_part_settings,
                    accept_native_changes,
                );
            }
            baseline_parts.insert(format!("{}:{}", object.object_id, part.part_id), baseline);
        }
    }
    for (key, value) in &baseline_project {
        template.profile[key] = Value::String(value.clone());
    }
    let (project_defaults, _) =
        limo_cad_core::resolve_print_settings(intent, &PrintSettingsDto::default());
    for (key, value) in settings_map(&project_defaults) {
        template.profile[&key] = Value::String(value);
    }
    let global = profile_settings(&template.profile);
    let native_global = native_process_settings(&template.profile)?;
    let config_source = text(&template.entries, CONFIG)?.to_string();
    let config = xml(&config_source)?;
    let mut edits = Vec::new();
    let mut reports = Vec::new();
    for object in config
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("object"))
    {
        let object_id = node_id(object, "id")?;
        let object_settings = metadata(object)?;
        let inherited_object_settings = baseline_objects
            .get(&object_id)
            .cloned()
            .unwrap_or_else(|| select_settings(&object_settings));
        let mut final_object_settings = baseline_objects.get(&object_id).cloned();
        let mut total_faces = 0usize;
        for part in object.children().filter(|n| n.has_tag_name("part")) {
            let part_id = node_id(part, "id")?;
            let Some(body) = target_body.get(&(object_id, part_id)) else {
                continue;
            };
            let mesh = &meshes[body];
            total_faces += mesh.triangle_count();
            let baseline = baseline_parts
                .get(&format!("{object_id}:{part_id}"))
                .ok_or_else(|| err("Refresh manifest missing original volume settings"))?;
            let requested = intent
                .parts
                .iter()
                .find(|p| p.body_id == *body)
                .map(|p| p.settings.clone())
                .unwrap_or_default();
            let overrides = settings_map(&requested);
            if let Some(object_settings) = &mut final_object_settings {
                object_settings.extend(overrides.clone());
            }
            let mut final_part = baseline.clone();
            if final_object_settings.is_some() {
                for key in overrides.keys() {
                    final_part.remove(key);
                }
            } else {
                final_part.extend(overrides.clone());
            }
            let mut inherited = global.clone();
            inherited.extend(inherited_object_settings.clone());
            inherited.extend(baseline.clone());
            let mut effective = inherited.clone();
            effective.extend(overrides.clone());
            let mut effective_sources: BTreeMap<_, _> = global
                .keys()
                .map(|key| (key.clone(), BambuSettingOrigin::TemplateProcess))
                .collect();
            if let Some(profile) = &intent.selected_process {
                for key in settings_map(&profile.defaults).keys() {
                    effective_sources
                        .insert(key.clone(), BambuSettingOrigin::SelectedProcessSnapshot);
                }
            }
            for key in settings_map(&intent.defaults).keys() {
                effective_sources.insert(key.clone(), BambuSettingOrigin::CadProjectDefault);
            }
            for key in inherited_object_settings.keys() {
                effective_sources.insert(key.clone(), BambuSettingOrigin::TemplateObject);
            }
            for key in baseline.keys() {
                effective_sources.insert(key.clone(), BambuSettingOrigin::TemplateVolume);
            }
            for key in overrides.keys() {
                effective_sources.insert(key.clone(), BambuSettingOrigin::CadPart);
            }
            validate_effective(&effective)?;
            let old = metadata(part)?;
            let (mut native_inherited, mut native_sources) =
                native_effective_settings(&native_global, &object_settings, &old)?;
            // A refresh restores reviewed managed baselines before applying CAD
            // overrides. Native read-only fields retain their untouched scopes.
            for (key, value) in &inherited {
                native_inherited.insert(key.clone(), Value::String(value.clone()));
            }
            let mut native_effective = native_inherited.clone();
            for (key, value) in &effective {
                native_effective.insert(key.clone(), Value::String(value.clone()));
            }
            native_sources.extend(effective_sources.clone());
            let filament = old
                .get("extruder")
                .or_else(|| object_settings.get("extruder"))
                .map(|v| v.parse::<u32>().map_err(err))
                .transpose()?
                .unwrap_or(1);
            if filament == 0 || filament as usize > template.summary.filament_settings_ids.len() {
                return fail("Object/volume filament index is outside the complete selected filament mapping");
            }
            let mut updates: BTreeMap<String, Option<String>> = SETTING_KEYS
                .iter()
                .map(|key| ((*key).into(), final_part.get(*key).cloned()))
                .collect();
            // These fields describe the previous mesh's disk-reload source, not
            // placement. Actual placement remains in 3MF components/build items.
            for key in [
                "matrix",
                "source_file",
                "source_object_id",
                "source_volume_id",
                "source_offset_x",
                "source_offset_y",
                "source_offset_z",
                "source_in_inches",
                "source_in_meters",
            ] {
                updates.insert(key.into(), None);
            }
            edit_metadata(&config_source, part, &updates, &mut edits)?;
            for stat in part.children().filter(|n| n.has_tag_name("mesh_stat")) {
                edits.push((stat.range(),format!("<mesh_stat face_count=\"{}\" edges_fixed=\"0\" degenerate_facets=\"0\" facets_removed=\"0\" facets_reversed=\"0\" backwards_edges=\"0\"/>",mesh.triangle_count())));
            }
            for binding in bindings
                .iter()
                .filter(|b| b.object_id == object_id && b.part_id == part_id)
            {
                reports.push(BambuPartReport {
                    binding: binding.clone(),
                    target_uuid: part.attribute("uuid").map(str::to_owned),
                    geometry_sha256: geometry_hash(mesh),
                    triangle_count: mesh.triangle_count(),
                    filament_index: filament,
                    filament_type: template.summary.filament_types[(filament - 1) as usize].clone(),
                    filament_color: template.summary.filament_colors[(filament - 1) as usize]
                        .clone(),
                    world_transform: Matrix::IDENTITY.standard_values(),
                    plate_index: template.plate_indices[&(binding.object_id, binding.instance_id)],
                    effective_sources: effective_sources.clone(),
                    inherited_settings: inherited.clone(),
                    written_overrides: overrides.clone(),
                    effective_settings: effective.clone(),
                    native_inherited_settings: native_inherited.clone(),
                    native_effective_settings: native_effective.clone(),
                    native_effective_sources: native_sources.clone(),
                });
            }
        }
        if let Some(settings) = final_object_settings {
            let updates = SETTING_KEYS
                .iter()
                .map(|key| ((*key).to_owned(), settings.get(*key).cloned()))
                .collect();
            edit_metadata(&config_source, object, &updates, &mut edits)?;
        }
        if let Some(face) = object
            .children()
            .find(|n| n.has_tag_name("metadata") && n.attribute("face_count").is_some())
        {
            edits.push((
                face.range(),
                format!("<metadata face_count=\"{total_faces}\"/>"),
            ));
        }
    }
    for assemble in config
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("assemble"))
    {
        edits.push((assemble.range(), String::new()));
    }
    template.entries.insert(
        CONFIG.into(),
        apply_edits(&config_source, edits)?.into_bytes(),
    );
    template.entries.insert(
        PROFILE.into(),
        serde_json::to_vec(&template.profile).map_err(err)?,
    );
    Ok((reports, baseline_project, baseline_parts, baseline_objects))
}
fn validate_effective(values: &Settings) -> Result<(), ExportError> {
    for key in ["wall_loops", "top_shell_layers", "bottom_shell_layers"] {
        if values
            .get(key)
            .and_then(|v| v.parse::<u32>().ok())
            .is_none_or(|v| v > 1000)
        {
            return Err(err(format!("Invalid effective '{key}'")));
        }
    }
    let density = values
        .get("sparse_infill_density")
        .and_then(|v| v.strip_suffix('%'))
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
        .ok_or_else(|| err("Invalid effective infill percentage"))?;
    let pattern = values
        .get("sparse_infill_pattern")
        .ok_or_else(|| err("No effective infill pattern"))?;
    if (density - 100.).abs() < 1e-6
        && ![
            "zig-zag",
            "concentric",
            "alignedrectilinear",
            "hilbertcurve",
            "archimedeanchords",
            "octagramspiral",
        ]
        .contains(&pattern.as_str())
    {
        return fail("Selected infill pattern is incompatible with 100% density in Bambu; explicitly choose Rectilinear/zig-zag or another supported solid pattern");
    }
    Ok(())
}
fn check_appearance(
    template: &Template,
    bindings: &[BambuPartBinding],
    appearances: &[BodyAppearance],
    allow: bool,
) -> Result<Vec<String>, ExportError> {
    let mut warnings = BTreeSet::new();
    for binding in bindings {
        let object = template
            .summary
            .objects
            .iter()
            .find(|o| o.object_id == binding.object_id)
            .ok_or_else(|| err("Binding target object not found"))?;
        let part = object
            .parts
            .iter()
            .find(|p| p.part_id == binding.part_id)
            .ok_or_else(|| err("Binding target part not found"))?;
        let filament = part
            .settings
            .get("extruder")
            .or_else(|| object.settings.get("extruder"))
            .map(|v| v.parse::<usize>().map_err(err))
            .transpose()?
            .unwrap_or(1);
        if filament == 0 || filament > template.summary.filament_types.len() {
            return fail("Invalid appearance filament mapping");
        }
        let Some(appearance) = appearances.iter().find(|a| a.body_id == binding.body_id) else {
            let message = format!("Body {} has no authored CAD appearance; template filament {} {}/{} is used only after explicit review", binding.body_id.0, filament, template.summary.filament_types[filament - 1], template.summary.filament_colors[filament - 1]);
            if !allow {
                return Err(err(format!(
                    "{message}; assign CAD appearance or explicitly accept template appearance"
                )));
            }
            warnings.insert(message);
            continue;
        };
        let color = appearance.color.opaque_rgb().to_hex_rgb();
        if !appearance
            .filament_type
            .eq_ignore_ascii_case(&template.summary.filament_types[filament - 1])
            || !color.eq_ignore_ascii_case(&template.summary.filament_colors[filament - 1])
        {
            let message=format!("Body {} CAD appearance {}/{} differs from template filament {} {}/{}; explicitly accept template appearance or select a matching template",binding.body_id.0,appearance.filament_type,color,filament,template.summary.filament_types[filament-1],template.summary.filament_colors[filament-1]);
            if !allow {
                return Err(err(message));
            }
            warnings.insert(message);
        }
    }
    Ok(warnings.into_iter().collect())
}

fn mesh_center(mesh: &TriangleMesh) -> [f64; 3] {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in mesh.positions.as_chunks::<3>().0 {
        for (axis, value) in point.iter().enumerate() {
            low[axis] = low[axis].min(*value);
            high[axis] = high[axis].max(*value);
        }
    }
    std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5)
}
fn geometry_hash(mesh: &TriangleMesh) -> String {
    let mut digest = Sha256::new();
    for value in &mesh.positions {
        digest.update(value.to_le_bytes());
    }
    for value in &mesh.indices {
        digest.update(value.to_le_bytes());
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn mesh_xml(mesh: &TriangleMesh, offset: [f64; 3]) -> String {
    let mut out = String::from("<mesh><vertices>");
    for [x, y, z] in mesh.positions.as_chunks::<3>().0 {
        out.push_str(&format!(
            "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>",
            *x - offset[0],
            *y - offset[1],
            *z - offset[2]
        ));
    }
    out.push_str("</vertices><triangles>");
    for [a, b, c] in mesh.indices.as_chunks::<3>().0 {
        out.push_str(&format!("<triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\"/>"));
    }
    out.push_str("</triangles></mesh>");
    out
}
fn set_tag_attribute(tag: &str, key: &str, value: &str) -> String {
    let bytes = tag.as_bytes();
    let mut at = 1;
    while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>' {
        at += 1;
    }
    while at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at == bytes.len() || matches!(bytes[at], b'/' | b'>') {
            break;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'=' {
            at += 1;
        }
        let name = &tag[start..at];
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'=') {
            at += 1;
        }
        let quote = bytes[at];
        at += 1;
        let start = at;
        while at < bytes.len() && bytes[at] != quote {
            at += 1;
        }
        if name == key {
            let mut output = tag.to_string();
            output.replace_range(start..at, &escape(value));
            return output;
        }
        at += 1;
    }
    let end = at;
    let offset = if tag.as_bytes()[end - 1] == b'/' {
        end - 1
    } else {
        end
    };
    let mut output = tag.to_string();
    output.insert_str(offset, &format!(" {key}=\"{}\"", escape(value)));
    output
}
fn apply_edits(
    source: &str,
    mut edits: Vec<(Range<usize>, String)>,
) -> Result<String, ExportError> {
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    let mut cursor = 0;
    let mut output = String::new();
    for (range, value) in edits {
        if range.start < cursor || range.end > source.len() {
            return fail("Overlapping XML edits are ambiguous");
        }
        output.push_str(&source[cursor..range.start]);
        output.push_str(&value);
        cursor = range.end;
    }
    output.push_str(&source[cursor..]);
    xml(&output)?;
    Ok(output)
}
fn apply_file_edits(
    entries: &mut BTreeMap<String, Vec<u8>>,
    edits: BTreeMap<String, Vec<(Range<usize>, String)>>,
) -> Result<(), ExportError> {
    for (path, patches) in edits {
        entries.insert(
            path.clone(),
            apply_edits(text(entries, &path)?, patches)?.into_bytes(),
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct Matrix([f64; 16]);
impl Matrix {
    const IDENTITY: Self = Self([
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]);
    fn parse(text: Option<&str>) -> Result<Self, ExportError> {
        let Some(text) = text else {
            return Ok(Self::IDENTITY);
        };
        let values = text
            .split_whitespace()
            .map(|v| v.parse::<f64>().map_err(err))
            .collect::<Result<Vec<_>, _>>()?;
        if values.len() != 12 || values.iter().any(|v| !v.is_finite()) {
            return fail("Invalid template transform");
        }
        let mut out = Self::IDENTITY;
        for c in 0..4 {
            for r in 0..3 {
                out.0[r * 4 + c] = values[c * 3 + r];
            }
        }
        out.inverse()?;
        Ok(out)
    }
    fn pose(pose: &MeshInstance) -> Result<Self, ExportError> {
        let norm = pose.rotation.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !norm.is_finite() || norm < 1e-12 || pose.translation.iter().any(|v| !v.is_finite()) {
            return fail("Invalid resolved CAD export pose");
        }
        let [x, y, z, w] = pose.rotation.map(|v| v / norm);
        let [a, b, c] = pose.translation;
        Ok(Self([
            1. - 2. * (y * y + z * z),
            2. * (x * y - z * w),
            2. * (x * z + y * w),
            a,
            2. * (x * y + z * w),
            1. - 2. * (x * x + z * z),
            2. * (y * z - x * w),
            b,
            2. * (x * z - y * w),
            2. * (y * z + x * w),
            1. - 2. * (x * x + y * y),
            c,
            0.,
            0.,
            0.,
            1.,
        ]))
    }
    fn compose(self, right: Self) -> Self {
        Self(std::array::from_fn(|i| {
            (0..4)
                .map(|k| self.0[i / 4 * 4 + k] * right.0[k * 4 + i % 4])
                .sum()
        }))
    }
    fn inverse(self) -> Result<Self, ExportError> {
        let a = self.0;
        let determinant = a[0] * (a[5] * a[10] - a[6] * a[9]) - a[1] * (a[4] * a[10] - a[6] * a[8])
            + a[2] * (a[4] * a[9] - a[5] * a[8]);
        if !determinant.is_finite() || determinant.abs() < 1e-12 {
            return fail("Noninvertible or overflowing template transform");
        }
        let mut result = Self::IDENTITY;
        result.0[0] = (a[5] * a[10] - a[6] * a[9]) / determinant;
        result.0[1] = (a[2] * a[9] - a[1] * a[10]) / determinant;
        result.0[2] = (a[1] * a[6] - a[2] * a[5]) / determinant;
        result.0[4] = (a[6] * a[8] - a[4] * a[10]) / determinant;
        result.0[5] = (a[0] * a[10] - a[2] * a[8]) / determinant;
        result.0[6] = (a[2] * a[4] - a[0] * a[6]) / determinant;
        result.0[8] = (a[4] * a[9] - a[5] * a[8]) / determinant;
        result.0[9] = (a[1] * a[8] - a[0] * a[9]) / determinant;
        result.0[10] = (a[0] * a[5] - a[1] * a[4]) / determinant;
        for row in 0..3 {
            result.0[row * 4 + 3] = -(0..3)
                .map(|col| result.0[row * 4 + col] * a[col * 4 + 3])
                .sum::<f64>();
        }
        if result.0.iter().any(|v| !v.is_finite()) {
            return fail("Transform inverse overflow");
        }
        Ok(result)
    }
    fn standard_values(self) -> [f64; 12] {
        std::array::from_fn(|index| self.0[(index % 3) * 4 + index / 3])
    }
    fn standard(self) -> String {
        (0..4)
            .flat_map(|c| (0..3).map(move |r| self.0[r * 4 + c].to_string()))
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn near(self, right: Self) -> bool {
        self.0
            .iter()
            .zip(right.0)
            .all(|(a, b)| (a - b).abs() < 1e-7)
    }
}

fn validate_grouping(
    structure: &limo_cad_assembly::ComponentStructureDto,
    bindings: &[BambuPartBinding],
) -> Result<(), ExportError> {
    structure.validate().map_err(ExportError)?;
    if structure.occurrences.is_empty() {
        return Ok(());
    }
    let parents: BTreeMap<_, _> = structure
        .occurrences
        .iter()
        .map(|o| (o.id.0, o.parent_occurrence_id.map(|p| p.0)))
        .collect();
    let mut cad_to_target = BTreeMap::new();
    let mut target_to_cad = BTreeMap::new();
    for binding in bindings {
        let mut root = binding.occurrence_id;
        loop {
            match parents.get(&root) {
                Some(Some(parent)) => root = *parent,
                Some(None) => break,
                None => return fail("Bound occurrence is missing from the CAD hierarchy"),
            }
        }
        let target = (binding.object_id, binding.instance_id);
        if cad_to_target
            .insert(root, target)
            .is_some_and(|old| old != target)
            || target_to_cad
                .insert(target, root)
                .is_some_and(|old| old != root)
        {
            return fail("Template grouping differs from the existing CAD component hierarchy; select a matching template or explicitly change the shared CAD/view organization");
        }
    }
    Ok(())
}

fn invalidate_derived(entries: &mut BTreeMap<String, Vec<u8>>) -> Result<Vec<String>, ExportError> {
    let mut removed = BTreeSet::new();
    let image_prefixes = [
        "plate_",
        "plate_no_light_",
        "top_",
        "pick_",
        "pattern_",
        "pattern_bbox_",
    ];
    for name in entries.keys() {
        let lower = name.to_ascii_lowercase();
        let metadata = lower.strip_prefix("metadata/");
        if lower.ends_with(".gcode")
            || lower.ends_with(".gcode.md5")
            || lower.ends_with(".bgcode")
            || lower.starts_with("metadata/slice_cache")
            || lower.starts_with("metadata/slicedata")
            || metadata.is_some_and(|tail| {
                image_prefixes.iter().any(|p| tail.starts_with(p))
                    && (tail.ends_with(".png")
                        || tail.ends_with(".jpg")
                        || tail.ends_with(".jpeg")
                        || tail.ends_with(".json"))
            })
            || lower == "metadata/print_profile.config"
        {
            removed.insert(name.clone());
        }
    }
    let config_source = text(entries, CONFIG)?.to_owned();
    let config = xml(&config_source)?;
    let mut patches = Vec::new();
    let stale_keys = [
        "gcode_file",
        "thumbnail_file",
        "thumbnail_no_light_file",
        "top_file",
        "pick_file",
        "pattern_file",
        "pattern_bbox_file",
        "prediction",
        "weight",
        "outside",
        "support_used",
        "first_layer_time",
        "label_object_enabled",
    ];
    for node in config.descendants().filter(|n| n.has_tag_name("metadata")) {
        if node
            .attribute("key")
            .is_some_and(|key| stale_keys.contains(&key))
        {
            if let Some(value) = node.attribute("value") {
                let path = value.strip_prefix('/').unwrap_or(value);
                if entries.contains_key(path) {
                    removed.insert(path.to_string());
                }
            }
            patches.push((node.range(), String::new()));
        }
    }
    entries.insert(
        CONFIG.into(),
        apply_edits(&config_source, patches)?.into_bytes(),
    );
    let root_source = text(entries, ROOT)?.to_string();
    let root = xml(&root_source)?;
    let patches = root
        .root_element()
        .children()
        .filter(|n| {
            n.has_tag_name((CORE_NS, "metadata"))
                && n.attribute("name")
                    .is_some_and(|name| name.starts_with("Thumbnail_"))
        })
        .map(|n| (n.range(), String::new()))
        .collect();
    entries.insert(
        ROOT.into(),
        apply_edits(&root_source, patches)?.into_bytes(),
    );
    if entries.contains_key("Metadata/slice_info.config") {
        entries.insert(
            "Metadata/slice_info.config".into(),
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><config><header/></config>".to_vec(),
        );
        removed.insert("Metadata/slice_info.config (derived contents)".into());
    }
    if entries.contains_key("Metadata/filament_sequence.json") {
        entries.insert("Metadata/filament_sequence.json".into(), b"{}".to_vec());
        removed.insert("Metadata/filament_sequence.json (derived contents)".into());
    }
    for path in removed
        .iter()
        .filter(|p| !p.ends_with(" (derived contents)"))
    {
        entries.remove(path);
    }
    let relationships: Vec<_> = entries
        .keys()
        .filter(|p| p.ends_with(".rels"))
        .cloned()
        .collect();
    for path in relationships {
        let source = text(entries, &path)?.to_string();
        let document = xml(&source)?;
        let mut patches = Vec::new();
        for node in document
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        {
            if node.attribute("TargetMode") == Some("External") {
                return fail(
                    "External package relationships are not supported in a manufacturing template",
                );
            }
            let target = node
                .attribute("Target")
                .ok_or_else(|| err("Relationship has no Target"))?;
            let target = resolve_relationship(&path, target)?;
            let target_lower = target.to_ascii_lowercase();
            let known_derived = target_lower.ends_with(".gcode")
                || target_lower.ends_with(".gcode.md5")
                || target_lower.strip_prefix("metadata/").is_some_and(|tail| {
                    image_prefixes.iter().any(|prefix| tail.starts_with(prefix))
                        && (tail.ends_with(".png")
                            || tail.ends_with(".jpg")
                            || tail.ends_with(".json"))
                });
            if removed.contains(&target) || known_derived {
                removed.insert(target);
                patches.push((node.range(), String::new()));
            } else if !entries.contains_key(&target) {
                return Err(err(format!(
                    "Dangling package relationship: {path} -> {target}"
                )));
            }
        }
        entries.insert(path, apply_edits(&source, patches)?.into_bytes());
    }
    if let Some(content) = entries.get("[Content_Types].xml") {
        let source = std::str::from_utf8(content).map_err(err)?.to_string();
        let doc = xml(&source)?;
        let patches = doc
            .descendants()
            .filter(|n| {
                n.is_element()
                    && n.tag_name().name() == "Override"
                    && n.attribute("PartName")
                        .is_some_and(|p| removed.contains(p.strip_prefix('/').unwrap_or(p)))
            })
            .map(|n| (n.range(), String::new()))
            .collect();
        entries.insert(
            "[Content_Types].xml".into(),
            apply_edits(&source, patches)?.into_bytes(),
        );
    }
    Ok(removed.into_iter().collect())
}
fn resolve_relationship(rels: &str, target: &str) -> Result<String, ExportError> {
    let mut parts = if target.starts_with('/') {
        Vec::new()
    } else {
        let parent = rels
            .rsplit_once("/_rels/")
            .map(|(parent, _)| parent)
            .unwrap_or("");
        parent
            .split('/')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    for part in target.strip_prefix('/').unwrap_or(target).split('/') {
        match part {
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return fail("Package relationship escapes the archive");
                }
            }
            "" => return fail("Malformed package relationship"),
            value => parts.push(value.into()),
        }
    }
    let path = parts.join("/");
    safe_path(&path)?;
    Ok(path)
}
fn write_archive(entries: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, ExportError> {
    if entries.len() > 4096
        || entries.values().any(|entry| entry.len() > MAX_INPUT)
        || entries
            .values()
            .map(|entry| entry.len() as u64)
            .sum::<u64>()
            > MAX_EXPANDED
    {
        return fail("Refreshed project exceeds the bounded 3MF package budget; reduce exported geometry or unused template assets");
    }
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            writer.start_file(name, options).map_err(err)?;
            writer.write_all(data).map_err(err)?;
        }
        writer.finish().map_err(err)?;
    }
    let bytes = cursor.into_inner();
    if bytes.len() > MAX_INPUT {
        return fail("Refreshed 3MF exceeds the supported 128 MiB archive size");
    }
    Ok(bytes)
}
fn populate_actual_report(
    template: &Template,
    reports: &mut [BambuPartReport],
) -> Result<(), ExportError> {
    let root_source = text(&template.entries, ROOT)?;
    let root = xml(root_source)?;
    for report in reports {
        let range = &template.build_ranges[&(report.binding.object_id, report.binding.instance_id)];
        let item = root
            .descendants()
            .find(|node| node.range() == *range)
            .ok_or_else(|| err("Actual report lost build item"))?;
        let target = &template.targets[&(report.binding.object_id, report.binding.part_id)];
        report.world_transform = Matrix::parse(item.attribute("transform"))?
            .compose(target.component_transform)
            .standard_values();
    }
    Ok(())
}
fn verify_readback(
    template: &Template,
    reports: &[BambuPartReport],
    sources: &BTreeMap<BodyId, TriangleMesh>,
    poses: &BTreeMap<(BodyId, u64), Matrix>,
    placement: BambuPlacementMode,
) -> Result<(), ExportError> {
    for report in reports {
        let object = template
            .summary
            .objects
            .iter()
            .find(|o| o.object_id == report.binding.object_id)
            .ok_or_else(|| err("Readback lost object identity"))?;
        let part = object
            .parts
            .iter()
            .find(|p| p.part_id == report.binding.part_id)
            .ok_or_else(|| err("Readback lost part identity"))?;
        let mut effective = profile_settings(&template.profile);
        effective.extend(select_settings(&object.settings));
        effective.extend(select_settings(&part.settings));
        if effective != report.effective_settings || part.uuid != report.target_uuid {
            return fail("Written settings/UUID failed independent metadata readback");
        }
        let target = &template.targets[&(object.object_id, part.part_id)];
        if placement == BambuPlacementMode::ResolvedScene {
            let root_source = text(&template.entries, ROOT)?;
            let root = xml(root_source)?;
            let range =
                &template.build_ranges[&(report.binding.object_id, report.binding.instance_id)];
            let item = root
                .descendants()
                .find(|n| n.range() == *range)
                .ok_or_else(|| err("Readback lost build instance"))?;
            let world =
                Matrix::parse(item.attribute("transform"))?.compose(target.component_transform);
            if !world.near(poses[&(report.binding.body_id, report.binding.occurrence_id)]) {
                return fail("Written multipart instance transforms differ from resolved CAD/named-view poses");
            }
        }
        let source = text(&template.entries, &target.path)?;
        let doc = xml(source)?;
        let mesh = doc
            .descendants()
            .find(|n| n.has_tag_name((CORE_NS, "mesh")) && n.range() == target.mesh_range)
            .ok_or_else(|| err("Readback mesh not found"))?;
        let input = &sources[&report.binding.body_id];
        let offset = if placement == BambuPlacementMode::Template {
            mesh_center(input)
        } else {
            [0.; 3]
        };
        let vertices: Vec<_> = mesh
            .descendants()
            .filter(|n| n.has_tag_name((CORE_NS, "vertex")))
            .collect();
        let triangles: Vec<_> = mesh
            .descendants()
            .filter(|n| n.has_tag_name((CORE_NS, "triangle")))
            .collect();
        if vertices.len() != input.positions.len() / 3 || triangles.len() != input.indices.len() / 3
        {
            return fail("Written geometry count failed independent readback");
        }
        for (node, point) in vertices.iter().zip(input.positions.as_chunks::<3>().0) {
            for (axis, key) in ["x", "y", "z"].iter().enumerate() {
                let actual = node
                    .attribute(*key)
                    .ok_or_else(|| err("Missing readback coordinate"))?
                    .parse::<f64>()
                    .map_err(err)?;
                if (actual + offset[axis] - point[axis]).abs() > 1e-5 {
                    return fail("Written mesh geometry differs from canonical source");
                }
            }
        }
        for (node, triangle) in triangles.iter().zip(input.indices.as_chunks::<3>().0) {
            for (axis, key) in ["v1", "v2", "v3"].iter().enumerate() {
                if node
                    .attribute(*key)
                    .ok_or_else(|| err("Missing readback triangle index"))?
                    .parse::<u32>()
                    .map_err(err)?
                    != triangle[axis]
                {
                    return fail("Written mesh topology differs from canonical source");
                }
            }
        }
    }
    Ok(())
}

pub use z_preflight::{BambuGroupZPreflight, BambuZCorrectionTarget};
#[path = "bambu_heights.rs"]
mod heights;
#[path = "bambu_modifiers.rs"]
mod modifiers;
mod z_preflight;

#[cfg(test)]
#[path = "bambu_qualification.rs"]
mod qualification;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    const NAMESPACE: &str = "8bb7fb96-d9d2-4b3c-81db-80ee3344f5de";
    pub(super) fn cube(body: u64) -> TriangleMesh {
        TriangleMesh {
            body_id: BodyId(body),
            name: format!("Part{body}"),
            positions: vec![
                0., 0., 0., 10., 0., 0., 10., 10., 0., 0., 10., 0., 0., 0., 10., 10., 0., 10., 10.,
                10., 10., 0., 10., 10.,
            ],
            indices: vec![
                0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0,
                7, 3, 1, 2, 6, 1, 6, 5,
            ],
        }
    }
    fn appearance(body: u64) -> BodyAppearance {
        let mut a = BodyAppearance::default_for(BodyId(body));
        a.filament_type = "PETG".into();
        a.color = limo_cad_core::Rgba8::opaque(3, 70, 56);
        a
    }
    fn profile() -> Value {
        json!({"version":"02.08.02.61","from":"project","printer_technology":"FFF","printer_settings_id":"Bambu Lab X2D 0.4 nozzle","printer_model":"Bambu Lab X2D","printer_variant":"0.4","print_settings_id":"0.20mm High Quality @BBL X2D","nozzle_diameter":["0.4","0.4"],"filament_settings_id":["Bambu PETG Basic @BBL X2D 0.4 nozzle"],"filament_type":["PETG"],"filament_colour":["#034638"],"filament_diameter":["1.75"],"nozzle_temperature":["255"],"nozzle_temperature_initial_layer":["255"],"filament_map":["1"],"filament_nozzle_map":["0"],"support_filament":"0","support_interface_filament":"1","machine_start_gcode":"G28","machine_end_gcode":"M400","gcode_flavor":"marlin","printable_height":"256","printable_area":["0x0","256x0","256x256","0x256"],"layer_height":"0.2","initial_layer_print_height":"0.2","wall_loops":"2","sparse_infill_density":"15%","sparse_infill_pattern":"gyroid","top_shell_layers":"5","bottom_shell_layers":"3"})
    }
    type TemplateFixture = (
        Vec<u8>,
        Vec<TriangleMesh>,
        Vec<BodyAppearance>,
        Vec<MeshInstance>,
        limo_cad_assembly::ComponentStructureDto,
        PrintIntentDocumentDto,
        BambuProjectRequest,
    );

    pub(crate) fn fixture() -> TemplateFixture {
        let mut entries = BTreeMap::new();
        entries.insert(ROOT.into(),format!(r#"<model xmlns="{CORE_NS}" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" unit="millimeter"><metadata name="Application">BambuStudio-02.08.02.61</metadata><resources><object id="20" type="model"><components><component objectid="7" p:path="/3D/Objects/a.model" transform="1 0 0 0 1 0 0 0 1 0 0 0"/><component objectid="9" p:path="/3D/Objects/b.model" transform="1 0 0 0 1 0 0 0 1 20 0 0"/></components></object></resources><build><item objectid="20" transform="1 0 0 0 1 0 0 0 1 20 20 0"/><item objectid="20" transform="1 0 0 0 1 0 0 0 1 80 20 0"/></build></model>"#).into_bytes());
        for (path, id) in [("3D/Objects/a.model", 7), ("3D/Objects/b.model", 9)] {
            entries.insert(path.into(),format!(r#"<model xmlns="{CORE_NS}" unit="millimeter"><resources><object id="{id}" type="model">{}</object></resources></model>"#,mesh_xml(&cube(1),[0.;3])).into_bytes());
        }
        entries.insert(CONFIG.into(),br#"<config><object id="20"><metadata key="name" value="Two &amp; parts"/><metadata key="extruder" value="1"/><metadata face_count="24"/><part id="7" subtype="normal_part" uuid="first-volume"><metadata key="name" value="First"/><metadata key="matrix" value="1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"/><mesh_stat face_count="12"/></part><part id="9" subtype="normal_part" uuid="second-volume"><metadata key="name" value="Second"/><mesh_stat face_count="12"/></part></object><plate><metadata key="plater_id" value="1"/><metadata key="gcode_file" value="Metadata/plate_1.gcode"/><metadata key="prediction" value="1234"/><model_instance><metadata key="object_id" value="20"/><metadata key="instance_id" value="0"/><metadata key="identify_id" value="120"/></model_instance><model_instance><metadata key="object_id" value="20"/><metadata key="instance_id" value="1"/><metadata key="identify_id" value="121"/></model_instance></plate><assemble><assemble_item object_id="20"/></assemble></config>"#.to_vec());
        entries.insert(PROFILE.into(), serde_json::to_vec(&profile()).unwrap());
        entries.insert("[Content_Types].xml".into(),br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#.to_vec());
        entries.insert("_rels/.rels".into(),br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="model" Target="/3D/3dmodel.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#.to_vec());
        entries.insert("3D/_rels/3dmodel.model.rels".into(),br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="part-a" Target="/3D/Objects/a.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/><Relationship Id="part-b" Target="/3D/Objects/b.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#.to_vec());
        entries.insert("Metadata/_rels/model_settings.config.rels".into(),br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="gcode" Target="plate_1.gcode" Type="http://schemas.bambulab.com/package/2021/gcode"/></Relationships>"#.to_vec());
        entries.insert("Metadata/plate_1.gcode".into(), b"STALE TOOLPATH".to_vec());
        entries.insert("Metadata/plate_1.gcode.md5".into(), b"stale".to_vec());
        entries.insert(
            "Metadata/plate_1_small.png".into(),
            b"stale preview".to_vec(),
        );
        entries.insert(
            "Metadata/slice_info.config".into(),
            b"<config><plate><metadata key=\"prediction\" value=\"1234\"/></plate></config>"
                .to_vec(),
        );
        entries.insert(
            "Metadata/filament_sequence.json".into(),
            b"{\"plate_1\":{\"sequence\":[1,2]}}".to_vec(),
        );
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let instances = vec![
            MeshInstance {
                body_id: BodyId(1),
                occurrence_id: 11,
                translation: [30., 40., 0.],
                rotation: [0., 0., half, half],
                visible: true,
            },
            MeshInstance {
                body_id: BodyId(2),
                occurrence_id: 12,
                translation: [30., 60., 0.],
                rotation: [0., 0., half, half],
                visible: true,
            },
            MeshInstance {
                body_id: BodyId(1),
                occurrence_id: 21,
                translation: [80., 70., 0.],
                rotation: [0., 0., 0., 1.],
                visible: true,
            },
            MeshInstance {
                body_id: BodyId(2),
                occurrence_id: 22,
                translation: [100., 70., 0.],
                rotation: [0., 0., 0., 1.],
                visible: true,
            },
        ];
        let structure=serde_json::from_value(json!({"definitions":[{"id":1,"name":"Group"},{"id":2,"name":"Part1","body_ids":[1]},{"id":3,"name":"Part2","body_ids":[2]}],"occurrences":[{"id":100,"name":"Firstgroup","component_id":1},{"id":11,"name":"Part1","component_id":2,"parent_occurrence_id":100},{"id":12,"name":"Part2","component_id":3,"parent_occurrence_id":100},{"id":200,"name":"Secondgroup","component_id":1},{"id":21,"name":"Part1repeat","component_id":2,"parent_occurrence_id":200},{"id":22,"name":"Part2repeat","component_id":3,"parent_occurrence_id":200}],"next_component_id":4,"next_occurrence_id":201})).unwrap();
        let intent = PrintIntentDocumentDto {
            source_document_id: Some(NAMESPACE.into()),
            parts: vec![limo_cad_core::PartPrintIntentDto {
                body_id: BodyId(1),
                settings: PrintSettingsDto {
                    wall_count: Some(6),
                    infill_density_percent: Some(40.),
                    infill_pattern: Some(InfillPatternDto::Gyroid),
                    top_shell_layers: Some(6),
                    bottom_shell_layers: Some(6),
                },
            }],
            ..Default::default()
        };
        let request = BambuProjectRequest {
            source_document_id: NAMESPACE.into(),
            bindings: vec![
                BambuPartBinding {
                    body_id: BodyId(1),
                    occurrence_id: 11,
                    object_id: 20,
                    instance_id: 0,
                    part_id: 7,
                },
                BambuPartBinding {
                    body_id: BodyId(2),
                    occurrence_id: 12,
                    object_id: 20,
                    instance_id: 0,
                    part_id: 9,
                },
                BambuPartBinding {
                    body_id: BodyId(1),
                    occurrence_id: 21,
                    object_id: 20,
                    instance_id: 1,
                    part_id: 7,
                },
                BambuPartBinding {
                    body_id: BodyId(2),
                    occurrence_id: 22,
                    object_id: 20,
                    instance_id: 1,
                    part_id: 9,
                },
            ],
            ..Default::default()
        };
        (
            write_archive(&entries).unwrap(),
            vec![cube(1), cube(2)],
            vec![appearance(1), appearance(2)],
            instances,
            structure,
            intent,
            request,
        )
    }

    #[test]
    fn single_volume_object_controls_track_overrides_reset_and_reviewed_native_changes() {
        let (
            template,
            mut meshes,
            mut appearances,
            mut instances,
            mut structure,
            mut intent,
            mut request,
        ) = fixture();
        let mut entries = archive(&template).unwrap();
        for (path, kind, attribute, id) in [
            (ROOT, "component", "objectid", "9"),
            (CONFIG, "part", "id", "9"),
        ] {
            let source = text(&entries, path).unwrap().to_owned();
            let document = xml(&source).unwrap();
            let node = document
                .descendants()
                .find(|node| node.has_tag_name(kind) && node.attribute(attribute) == Some(id))
                .unwrap();
            entries.insert(
                path.into(),
                apply_edits(&source, vec![(node.range(), String::new())])
                    .unwrap()
                    .into_bytes(),
            );
        }
        let source = text(&entries, CONFIG).unwrap().to_owned();
        let document = xml(&source).unwrap();
        let object = document
            .descendants()
            .find(|node| node.has_tag_name("object"))
            .unwrap();
        let updates = BTreeMap::from([
            ("wall_loops".into(), Some("3".into())),
            ("sparse_infill_density".into(), Some("22%".into())),
        ]);
        let mut edits = Vec::new();
        edit_metadata(&source, object, &updates, &mut edits).unwrap();
        let part = object
            .children()
            .find(|node| node.has_tag_name("part"))
            .unwrap();
        edit_metadata(
            &source,
            part,
            &BTreeMap::from([("top_shell_layers".into(), Some("4".into()))]),
            &mut edits,
        )
        .unwrap();
        entries.insert(
            CONFIG.into(),
            apply_edits(&source, edits).unwrap().into_bytes(),
        );
        meshes.retain(|mesh| mesh.body_id == BodyId(1));
        appearances.retain(|appearance| appearance.body_id == BodyId(1));
        instances.retain(|instance| instance.body_id == BodyId(1));
        structure
            .occurrences
            .retain(|occurrence| occurrence.id.0 != 12 && occurrence.id.0 != 22);
        request
            .bindings
            .retain(|binding| binding.body_id == BodyId(1));
        let first = write_bambu_project(
            &write_archive(&entries).unwrap(),
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let parsed = parse_template(&first.bytes).unwrap();
        let object = &parsed.summary.objects[0];
        assert_eq!(object.parts.len(), 1);
        assert_eq!(object.instance_count, 2);
        for key in SETTING_KEYS {
            assert_eq!(
                object.settings[key],
                first.report.parts[0].effective_settings[key]
            );
            assert!(!object.parts[0].settings.contains_key(key));
        }
        assert_eq!(
            first.report.parts[0].effective_sources["wall_loops"],
            BambuSettingOrigin::CadPart
        );
        assert_eq!(
            first.report.refresh_reference.parts[0]
                .baseline_object_settings
                .as_ref()
                .unwrap()["wall_loops"],
            "3"
        );
        assert_eq!(
            first.report.refresh_reference.parts[0]
                .written_object_settings
                .as_ref()
                .unwrap()["wall_loops"],
            "6"
        );
        assert!(first.report.refresh_reference.parts[0]
            .written_part_settings
            .is_empty());
        assert_eq!(
            first.report.refresh_reference.parts[0].baseline_part_settings["top_shell_layers"],
            "4"
        );
        request.bindings.clear();
        request.refresh_reference = Some(first.report.refresh_reference.clone());
        intent.parts.clear();
        let reset = write_bambu_project(
            &first.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let parsed = parse_template(&reset.bytes).unwrap();
        assert_eq!(parsed.summary.objects[0].settings["wall_loops"], "3");
        assert_eq!(
            parsed.summary.objects[0].settings["sparse_infill_density"],
            "22%"
        );
        assert!(!parsed.summary.objects[0]
            .settings
            .contains_key("top_shell_layers"));
        assert!(!parsed.summary.objects[0].parts[0]
            .settings
            .contains_key("wall_loops"));
        assert_eq!(
            parsed.summary.objects[0].parts[0].settings["top_shell_layers"],
            "4"
        );
        assert_eq!(
            reset.report.parts[0].effective_settings["top_shell_layers"],
            "4"
        );
        assert_eq!(reset.report.parts[0].effective_settings["wall_loops"], "3");
        assert_eq!(
            reset.report.parts[0].effective_sources["wall_loops"],
            BambuSettingOrigin::TemplateObject
        );
        let mut changed_entries = archive(&first.bytes).unwrap();
        let source = text(&changed_entries, CONFIG).unwrap().to_owned();
        let document = xml(&source).unwrap();
        let object = document
            .descendants()
            .find(|node| node.has_tag_name("object"))
            .unwrap();
        let mut edits = Vec::new();
        edit_metadata(
            &source,
            object,
            &BTreeMap::from([("wall_loops".into(), Some("8".into()))]),
            &mut edits,
        )
        .unwrap();
        changed_entries.insert(
            CONFIG.into(),
            apply_edits(&source, edits).unwrap().into_bytes(),
        );
        let changed = write_archive(&changed_entries).unwrap();
        assert!(write_bambu_project(
            &changed,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .to_string()
        .contains("Native settings differ"));
        request.accept_native_setting_changes = true;
        let accepted = write_bambu_project(
            &changed,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let parsed = parse_template(&accepted.bytes).unwrap();
        assert_eq!(parsed.summary.objects[0].settings["wall_loops"], "8");
        assert_eq!(
            accepted.report.parts[0].effective_settings["wall_loops"],
            "8"
        );
        assert!(accepted
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("Native object settings changed")));
    }

    #[test]
    fn saved_bambu_printable_flags_cannot_silently_drop_intentional_instances() {
        let (template, meshes, appearances, instances, structure, intent, request) = fixture();
        for value in ["0", "false", "true", "bad", "2", "1"] {
            let mut entries = archive(&template).unwrap();
            let source = text(&entries, ROOT).unwrap().replace(
                "<item objectid=\"20\" ",
                &format!("<item objectid=\"20\" printable=\"{value}\" "),
            );
            entries.insert(ROOT.into(), source.into_bytes());
            let result = write_bambu_project(
                &write_archive(&entries).unwrap(),
                &meshes,
                &appearances,
                &instances,
                &structure,
                &intent,
                &request,
            );
            if value == "1" {
                assert_eq!(result.unwrap().report.parts.len(), instances.len());
            } else {
                assert!(result.err().unwrap().to_string().contains("printable"));
            }
        }
    }

    #[test]
    fn rotated_repeated_multipart_export_preserves_identity_geometry_settings_and_invalidates_toolpaths(
    ) {
        let (template, meshes, appearances, instances, structure, intent, request) = fixture();
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
        assert!(result.report.metadata_readback_verified);
        assert!(!result.report.installed_slicer_imported && !result.report.toolpaths_generated);
        assert_eq!(result.report.parts.len(), 4);
        let parsed = parse_template(&result.bytes).unwrap();
        assert_eq!(parsed.summary.objects.len(), 1);
        assert_eq!(parsed.summary.objects[0].instance_count, 2);
        assert_eq!(parsed.summary.objects[0].object_ordinal, 1);
        assert_eq!(parsed.summary.objects[0].object_id, 20);
        for part in result.report.parts {
            assert_eq!(
                part.effective_settings["wall_loops"],
                if part.binding.body_id == BodyId(1) {
                    "6"
                } else {
                    "2"
                }
            );
        }
        let model = xml(text(&parsed.entries, ROOT).unwrap()).unwrap();
        let transforms: Vec<_> = model
            .descendants()
            .filter(|n| n.has_tag_name((CORE_NS, "item")))
            .map(|n| Matrix::parse(n.attribute("transform")).unwrap())
            .collect();
        assert!(transforms[0].near(Matrix::pose(&instances[0]).unwrap()));
        assert!(transforms[1].near(Matrix::pose(&instances[2]).unwrap()));
        assert!(parsed.targets[&(20, 9)]
            .component_transform
            .near(Matrix::parse(Some("1 0 0 0 1 0 0 0 1 20 0 0")).unwrap()));
        assert!(!parsed
            .entries
            .keys()
            .any(|p| p.ends_with(".gcode") || p.ends_with(".md5") || p.ends_with(".png")));
        assert!(!text(&parsed.entries, CONFIG)
            .unwrap()
            .contains("prediction"));
        assert!(
            !text(&parsed.entries, "Metadata/_rels/model_settings.config.rels")
                .unwrap()
                .contains("gcode")
        );
    }
    #[test]
    fn manifest_refresh_never_matches_names_and_reset_restores_original_inherited_defaults() {
        let (template, meshes, appearances, instances, structure, mut intent, mut request) =
            fixture();
        let first = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        request.bindings.clear();
        intent.parts.clear();
        let second = write_bambu_project(
            &first.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert!(second
            .report
            .parts
            .iter()
            .all(|p| p.effective_settings["wall_loops"] == "2"));
        let error = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .err()
        .unwrap();
        assert!(error.0.contains("Foreign saved template"));
        let mut alien = intent.clone();
        alien.source_document_id = Some("8bb7fb96-d9d2-4b3c-81db-80ee3344f5df".into());
        request.source_document_id = alien.source_document_id.clone().unwrap();
        assert!(write_bambu_project(
            &first.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &alien,
            &request
        )
        .is_err());
    }
    #[test]
    fn missing_repeat_ambiguous_binding_and_changed_relative_pose_are_rejected() {
        let (template, meshes, appearances, mut instances, structure, intent, request) = fixture();
        let mut missing = request.clone();
        missing.bindings.pop();
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &missing
        )
        .is_err());
        let mut duplicate = request.clone();
        duplicate.bindings[1] = duplicate.bindings[0].clone();
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &duplicate
        )
        .is_err());
        instances[3].translation[0] += 1.;
        let error = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .err()
        .unwrap();
        assert!(error.0.contains("different relative CAD"));
    }
    #[test]
    fn no_silent_appearance_or_solid_infill_substitution() {
        let (template, meshes, mut appearances, instances, structure, mut intent, mut request) =
            fixture();
        appearances[0].filament_type = "PLA".into();
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .is_err());
        request.allow_template_appearance = true;
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
        assert!(result
            .report
            .warnings
            .iter()
            .any(|w| w.contains("differs from template")));
        intent.parts[0].settings.infill_density_percent = Some(100.);
        assert!(write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .is_err());
        intent.parts[0].settings.infill_pattern = Some(InfillPatternDto::Rectilinear);
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
        assert_eq!(
            result.report.parts[0].effective_settings["sparse_infill_pattern"],
            "zig-zag"
        );
    }
    #[test]
    fn cad_groups_cannot_silently_merge_into_foreign_template_object() {
        let (template, meshes, appearances, instances, mut structure, intent, request) = fixture();
        structure
            .occurrences
            .iter_mut()
            .find(|o| o.id.0 == 12)
            .unwrap()
            .parent_occurrence_id = None;
        let error = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .err()
        .unwrap();
        assert!(error.0.contains("grouping differs"));
    }
    #[test]
    fn complete_template_required_and_unqualified_versions_rejected() {
        let (template, ..) = fixture();
        let mut entries = archive(&template).unwrap();
        let mut settings = profile();
        settings
            .as_object_mut()
            .unwrap()
            .remove("machine_start_gcode");
        entries.insert(PROFILE.into(), serde_json::to_vec(&settings).unwrap());
        assert!(inspect_bambu_template(&write_archive(&entries).unwrap()).is_err());
        settings = profile();
        settings["version"] = json!("02.05.00.66");
        entries.insert(PROFILE.into(), serde_json::to_vec(&settings).unwrap());
        assert!(inspect_bambu_template(&write_archive(&entries).unwrap()).is_err());
        entries.insert(PROFILE.into(), serde_json::to_vec(&profile()).unwrap());
        let graph = entries.remove("3D/_rels/3dmodel.model.rels").unwrap();
        assert!(inspect_bambu_template(&write_archive(&entries).unwrap())
            .err()
            .unwrap()
            .0
            .contains("missing its 3D model relationship"));
        entries.insert("3D/_rels/3dmodel.model.rels".into(), graph);
        entries.insert("../escape".into(), vec![]);
        assert!(inspect_bambu_template(&write_archive(&entries).unwrap()).is_err());
    }
    #[test]
    fn native_patterns_outside_typed_capability_are_inspected_without_becoming_editable() {
        let (template, meshes, appearances, instances, structure, intent, request) = fixture();
        let mut entries = archive(&template).unwrap();
        for pattern in ["rectilinear", "adaptivecubic", "future_native_pattern"] {
            let mut settings = profile();
            settings["sparse_infill_pattern"] = json!(pattern);
            settings["layer_height"] = json!("0.28");
            settings["line_width"] = json!(["0.50", "0.45"]);
            settings["top_shell_thickness"] = json!("1.2");
            settings["enable_support"] = json!("0");
            settings["enable_prime_tower"] = json!("0");
            entries.insert(PROFILE.into(), serde_json::to_vec(&settings).unwrap());
            let bytes = write_archive(&entries).unwrap();
            let summary = inspect_bambu_template(&bytes).unwrap();
            assert_eq!(summary.process_defaults.infill_pattern, None);
            assert_eq!(summary.process_defaults.wall_count, Some(2));
            assert_eq!(
                summary.native_process_settings["sparse_infill_pattern"],
                pattern
            );
            assert_eq!(
                summary.native_process_settings["line_width"],
                json!(["0.50", "0.45"])
            );
            assert_eq!(summary.native_process_settings["layer_height"], "0.28");
            assert_eq!(
                summary.native_process_settings["top_shell_thickness"],
                "1.2"
            );
            assert_eq!(summary.native_process_settings["enable_support"], "0");
            assert_eq!(summary.native_process_settings["enable_prime_tower"], "0");
            assert!(!summary
                .native_process_settings
                .contains_key("machine_start_gcode"));
            assert_eq!(summary.process_capability_warnings.len(), 1);
            let error = write_bambu_project(
                &bytes,
                &meshes,
                &appearances,
                &instances,
                &structure,
                &intent,
                &request,
            )
            .err()
            .unwrap();
            assert!(error
                .0
                .contains("Managed Bambu export cannot represent this process"));
            assert!(error.0.contains("read-only inspection"));
        }
        let mut settings = profile();
        settings["sparse_infill_pattern"] = json!("rectilinear");
        settings["sparse_infill_density"] = json!("NaN%");
        entries.insert(PROFILE.into(), serde_json::to_vec(&settings).unwrap());
        assert!(inspect_bambu_template(&write_archive(&entries).unwrap()).is_err());
        for oversized in [
            json!(vec!["0.5"; 129]),
            json!("0".repeat(257)),
            json!({"unexpected": "object"}),
        ] {
            let mut settings = profile();
            settings["line_width"] = oversized;
            entries.insert(PROFILE.into(), serde_json::to_vec(&settings).unwrap());
            assert!(inspect_bambu_template(&write_archive(&entries).unwrap())
                .err()
                .unwrap()
                .0
                .contains("bounded scalar/vector report capability"));
        }
    }
    #[test]
    fn native_read_only_fields_keep_effective_scope_and_survive_mesh_refresh() {
        let (template, meshes, appearances, instances, structure, intent, request) = fixture();
        let mut entries = archive(&template).unwrap();
        let mut settings = profile();
        settings["sparse_infill_pattern"] = json!("zig-zag");
        settings["layer_height"] = json!("0.28");
        settings["line_width"] = json!(["0.50", "0.45"]);
        settings["top_shell_thickness"] = json!("1.2");
        settings["enable_support"] = json!("0");
        settings["enable_prime_tower"] = json!("0");
        entries.insert(PROFILE.into(), serde_json::to_vec(&settings).unwrap());
        let config = text(&entries, CONFIG).unwrap().replace(
            "<object id=\"20\">",
            "<object id=\"20\"><metadata key=\"outer_wall_line_width\" value=\"0.48\"/>",
        ).replace(
            "<metadata key=\"name\" value=\"First\"/>",
            "<metadata key=\"name\" value=\"First\"/><metadata key=\"top_shell_thickness\" value=\"1.6\"/>",
        );
        entries.insert(CONFIG.into(), config.into_bytes());
        let bytes = write_archive(&entries).unwrap();
        let summary = inspect_bambu_template(&bytes).unwrap();
        assert_eq!(
            summary.process_defaults.infill_pattern,
            Some(InfillPatternDto::Rectilinear)
        );
        assert!(summary.process_capability_warnings.is_empty());
        let result = write_bambu_project(
            &bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let first = result
            .report
            .parts
            .iter()
            .find(|part| part.binding.body_id == BodyId(1))
            .unwrap();
        assert_eq!(
            first.native_effective_settings["line_width"],
            json!(["0.50", "0.45"])
        );
        assert_eq!(first.native_effective_settings["layer_height"], "0.28");
        assert_eq!(
            first.native_effective_settings["outer_wall_line_width"],
            "0.48"
        );
        assert_eq!(
            first.native_effective_settings["top_shell_thickness"],
            "1.6"
        );
        assert_eq!(first.native_effective_settings["wall_loops"], "6");
        assert_eq!(first.native_inherited_settings["wall_loops"], "2");
        assert_eq!(
            first.native_effective_sources["line_width"],
            BambuSettingOrigin::TemplateProcess
        );
        assert_eq!(
            first.native_effective_sources["outer_wall_line_width"],
            BambuSettingOrigin::TemplateObject
        );
        assert_eq!(
            first.native_effective_sources["top_shell_thickness"],
            BambuSettingOrigin::TemplateVolume
        );
        assert_eq!(
            first.native_effective_sources["wall_loops"],
            BambuSettingOrigin::CadPart
        );
        assert!(!result.report.installed_slicer_imported && !result.report.toolpaths_generated);
        let refreshed = inspect_bambu_template(&result.bytes).unwrap();
        assert_eq!(
            refreshed.native_process_settings,
            summary.native_process_settings
        );
        assert!(!refreshed
            .native_process_settings
            .contains_key("support_style"));
    }
    #[test]
    fn native_resave_renumbering_and_instance_reordering_use_uuid_and_identify_id() {
        let (template, meshes, appearances, instances, structure, intent, mut request) = fixture();
        let first = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let mut entries = archive(&first.bytes).unwrap();
        entries.remove(MANIFEST);
        let source = text(&entries, ROOT).unwrap().to_owned();
        let doc = xml(&source).unwrap();
        let items: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name((CORE_NS, "item")))
            .map(|n| n.range())
            .collect();
        let swapped = apply_edits(
            &source,
            vec![
                (items[0].clone(), source[items[1].clone()].to_owned()),
                (items[1].clone(), source[items[0].clone()].to_owned()),
            ],
        )
        .unwrap();
        entries.insert(ROOT.into(), swapped.into_bytes());
        for (path, bytes) in entries
            .iter_mut()
            .filter(|(p, _)| p.ends_with(".model") || p.as_str() == CONFIG)
        {
            let mut source = String::from_utf8(bytes.clone()).unwrap();
            for (old, new) in [(20, 200), (7, 70), (9, 90)] {
                for attribute in ["id", "objectid"] {
                    source = source.replace(
                        &format!("{attribute}=\"{old}\""),
                        &format!("{attribute}=\"{new}\""),
                    );
                }
            }
            if path == CONFIG {
                source = source
                    .replace(
                        "key=\"object_id\" value=\"20\"",
                        "key=\"object_id\" value=\"200\"",
                    )
                    .replace(
                        "key=\"instance_id\" value=\"0\"",
                        "key=\"instance_id\" value=\"swap\"",
                    )
                    .replace(
                        "key=\"instance_id\" value=\"1\"",
                        "key=\"instance_id\" value=\"0\"",
                    )
                    .replace(
                        "key=\"instance_id\" value=\"swap\"",
                        "key=\"instance_id\" value=\"1\"",
                    );
            }
            *bytes = source.into_bytes();
        }
        request.bindings.clear();
        request.refresh_reference = Some(first.report.refresh_reference);
        let refreshed = write_bambu_project(
            &write_archive(&entries).unwrap(),
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(
            refreshed
                .report
                .parts
                .iter()
                .find(|p| p.binding.occurrence_id == 11)
                .unwrap()
                .binding,
            BambuPartBinding {
                body_id: BodyId(1),
                occurrence_id: 11,
                object_id: 200,
                instance_id: 1,
                part_id: 70
            }
        );
        assert_eq!(
            refreshed
                .report
                .parts
                .iter()
                .find(|p| p.binding.occurrence_id == 21)
                .unwrap()
                .binding
                .instance_id,
            0
        );
        request.refresh_reference.as_mut().unwrap().parts[0].instance_identify_id = 999;
        assert!(write_bambu_project(
            &write_archive(&entries).unwrap(),
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("missing or ambiguous"));
    }
    #[test]
    fn reviewed_native_override_becomes_baseline_and_reference_cannot_inject_settings() {
        let (template, meshes, appearances, instances, structure, mut intent, mut request) =
            fixture();
        let first = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let mut entries = archive(&first.bytes).unwrap();
        entries.remove(MANIFEST);
        let config = text(&entries, CONFIG).unwrap().replace(
            "key=\"wall_loops\" value=\"6\"",
            "key=\"wall_loops\" value=\"8\"",
        );
        entries.insert(CONFIG.into(), config.into_bytes());
        request.bindings.clear();
        request.refresh_reference = Some(first.report.refresh_reference);
        let native = write_archive(&entries).unwrap();
        assert!(write_bambu_project(
            &native,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("explicitly accept"));
        request.accept_native_setting_changes = true;
        intent.parts.clear();
        let reviewed = write_bambu_project(
            &native,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(
            reviewed
                .report
                .parts
                .iter()
                .find(|p| p.binding.body_id == BodyId(1))
                .unwrap()
                .effective_settings["wall_loops"],
            "8"
        );
        request.refresh_reference = Some(reviewed.report.refresh_reference);
        request.accept_native_setting_changes = false;
        let next = write_bambu_project(
            &reviewed.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(next.report.parts[0].effective_settings["wall_loops"], "8");
        request
            .refresh_reference
            .as_mut()
            .unwrap()
            .baseline_project_settings
            .insert("machine_start_gcode".into(), "malicious".into());
        assert!(write_bambu_project(
            &reviewed.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .err()
        .unwrap()
        .0
        .contains("five supported settings"));
    }
    #[test]
    fn fresh_foreign_template_needs_explicit_complete_bindings_and_starts_new_baseline() {
        let (template, meshes, appearances, instances, structure, mut intent, mut request) =
            fixture();
        let first = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let old_reference = first.report.refresh_reference;
        request.source_document_id = "77777777-7777-4777-8777-777777777777".into();
        intent.source_document_id = Some(request.source_document_id.clone());
        let bindings = std::mem::take(&mut request.bindings);
        assert!(write_bambu_project(
            &first.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .is_err());
        request.bindings = bindings;
        let new = write_bambu_project(
            &first.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert_eq!(
            new.report.refresh_reference.source_document_id,
            request.source_document_id
        );
        assert!(new
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("new CAD project lineage")));
        assert_eq!(
            new.report.refresh_reference.original_template_sha256,
            hash(&first.bytes)
        );
        assert_eq!(new.report.parts[0].effective_settings["wall_loops"], "6");
        request.refresh_reference = Some(old_reference);
        assert!(write_bambu_project(
            &first.bytes,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request
        )
        .is_err());
    }

    #[test]
    fn unauthored_cad_appearance_requires_review_and_retains_native_materials() {
        let (template, meshes, _, instances, structure, intent, mut request) = fixture();
        assert!(write_bambu_project(
            &template,
            &meshes,
            &[],
            &instances,
            &structure,
            &intent,
            &request
        )
        .is_err());
        request.allow_template_appearance = true;
        let output = write_bambu_project(
            &template,
            &meshes,
            &[],
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        assert!(output
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("no authored CAD appearance")));
        assert!(output
            .report
            .parts
            .iter()
            .all(|part| part.filament_type == "PETG" && part.filament_color == "#034638"));
        assert_eq!(output.report.parts.len(), instances.len());
    }

    #[test]
    fn replaced_meshes_clear_external_reload_provenance_without_changing_placement_or_process() {
        for placement in [
            BambuPlacementMode::Template,
            BambuPlacementMode::ResolvedScene,
        ] {
            let (bytes, meshes, appearances, instances, structure, intent, mut request) = fixture();
            let mut entries = archive(&bytes).unwrap();
            let original_root = text(&entries, ROOT).unwrap().to_owned();
            let original_profile = entries[PROFILE].clone();
            let marker = r#"<part id="9" subtype="normal_part" uuid="second-volume">"#;
            let legacy_source = r#"<metadata key="matrix" value="1 0 0 500 0 1 0 600 0 0 1 700 0 0 0 1"/><metadata key="source_file" value="old-mechanical-source.step"/><metadata key="source_object_id" value="77"/><metadata key="source_volume_id" value="88"/><metadata key="source_offset_x" value="123"/><metadata key="source_offset_y" value="234"/><metadata key="source_offset_z" value="345"/><metadata key="source_in_inches" value="1"/><metadata key="source_in_meters" value="0"/><metadata key="ironing_type" value="all"/>"#;
            entries.insert(
                CONFIG.into(),
                text(&entries, CONFIG)
                    .unwrap()
                    .replace(marker, &format!("{marker}{legacy_source}"))
                    .into_bytes(),
            );
            request.placement = placement;
            let output = write_bambu_project(
                &write_archive(&entries).unwrap(),
                &meshes,
                &appearances,
                &instances,
                &structure,
                &intent,
                &request,
            )
            .unwrap();
            let parsed = parse_template(&output.bytes).unwrap();
            assert_eq!(parsed.entries[PROFILE], original_profile);
            for part in &parsed.summary.objects[0].parts {
                for key in [
                    "matrix",
                    "source_file",
                    "source_object_id",
                    "source_volume_id",
                    "source_offset_x",
                    "source_offset_y",
                    "source_offset_z",
                    "source_in_inches",
                    "source_in_meters",
                ] {
                    assert!(
                        !part.settings.contains_key(key),
                        "stale source key {key} retained"
                    );
                }
            }
            assert_eq!(
                parsed.summary.objects[0]
                    .parts
                    .iter()
                    .find(|part| part.part_id == 9)
                    .unwrap()
                    .settings["ironing_type"],
                "all"
            );
            if placement == BambuPlacementMode::Template {
                assert_eq!(text(&parsed.entries, ROOT).unwrap(), original_root);
            } else {
                for report in &output.report.parts {
                    let pose = instances
                        .iter()
                        .find(|pose| {
                            pose.body_id == report.binding.body_id
                                && pose.occurrence_id == report.binding.occurrence_id
                        })
                        .unwrap();
                    for (actual, expected) in report
                        .world_transform
                        .into_iter()
                        .zip(Matrix::pose(pose).unwrap().standard_values())
                    {
                        assert!((actual - expected).abs() < 1e-7);
                    }
                }
            }
            assert!(output
                .report
                .warnings
                .iter()
                .any(|warning| warning.contains("stale external reload")));
        }
    }
    #[test]
    fn report_tracks_actual_template_placement_material_and_setting_origins() {
        let (template, meshes, appearances, instances, structure, mut intent, mut request) =
            fixture();
        request.placement = BambuPlacementMode::Template;
        intent.defaults.wall_count = Some(4);
        let output = write_bambu_project(
            &template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        let first = output
            .report
            .parts
            .iter()
            .find(|part| part.binding.occurrence_id == 11)
            .unwrap();
        assert_eq!(&first.world_transform[9..], [20., 20., 0.]);
        assert_ne!(&first.world_transform[9..], instances[0].translation);
        assert_eq!(first.plate_index, 1);
        assert_eq!(first.filament_type, "PETG");
        assert_eq!(first.filament_color, "#034638");
        assert_eq!(
            first.effective_sources["wall_loops"],
            BambuSettingOrigin::CadPart
        );
        let second = output
            .report
            .parts
            .iter()
            .find(|part| part.binding.occurrence_id == 12)
            .unwrap();
        assert_eq!(&second.world_transform[9..], [40., 20., 0.]);
        assert_eq!(
            second.effective_sources["wall_loops"],
            BambuSettingOrigin::CadProjectDefault
        );
        assert_eq!(
            second.effective_sources["sparse_infill_density"],
            BambuSettingOrigin::TemplateProcess
        );
        assert!(output
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("later identities")));
    }
}
