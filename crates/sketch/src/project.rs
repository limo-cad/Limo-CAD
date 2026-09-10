//! Versioned, host-neutral noBS CAD project model.
//!
//! The outer `.nbcad` ZIP container is owned by the frontend file layer.
//! This module owns `model.json`, its validation, and the migration entry
//! point so native and browser hosts cannot disagree about project meaning.

use std::collections::{BTreeSet, HashSet};

use nbcad_core::{
    BodyAppearance, DimensionStyle, DocumentSettings, FeatureId, FeatureKind, FeatureTree,
    PlaneBasis, PlaneRef,
};
use nbcad_solid::{
    BodyFeatureDefinitionDto, DatumPlaneDefinitionDto, ExtrudeDefinitionDto, HoleDefinitionDto,
    LoftDefinitionDto, RevolveDefinitionDto, RibDefinitionDto, SolidChamferDefinitionDto,
    SolidFilletDefinitionDto, SweepDefinitionDto,
};
use serde::{Deserialize, Serialize};

use crate::sketch::SketchSnapshot;
use crate::{AssemblyDocumentDto, DrawingDocumentDto, ProjectVisibilityDto};

pub const PROJECT_FORMAT: &str = "nbcad-project";
pub const LEGACY_PROJECT_FORMAT: &str = "tfcad-project";
// Reader boundaries protect model semantics: schema 3 introduced reference
// dimensions; schema 4 introduced gear coupling; schema 5 protects placed
// drawing references from readers that would silently treat them as definitions.
// Schema 6 protects structural drawing guards from readers that would drop them.
pub const PROJECT_SCHEMA_VERSION: u32 = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectModelV6 {
    pub format: String,
    pub schema_version: u32,
    pub document: ProjectDocumentV2,
    pub sketches: Vec<ProjectSketchV2>,
    pub extrudes: Vec<ExtrudeDefinitionDto>,
    #[serde(default)]
    pub revolves: Vec<RevolveDefinitionDto>,
    #[serde(default)]
    pub sweeps: Vec<SweepDefinitionDto>,
    #[serde(default)]
    pub lofts: Vec<LoftDefinitionDto>,
    #[serde(default)]
    pub ribs: Vec<RibDefinitionDto>,
    #[serde(default)]
    pub fillets: Vec<SolidFilletDefinitionDto>,
    #[serde(default)]
    pub chamfers: Vec<SolidChamferDefinitionDto>,
    #[serde(default)]
    pub holes: Vec<HoleDefinitionDto>,
    #[serde(default)]
    pub datum_planes: Vec<DatumPlaneDefinitionDto>,
    #[serde(default)]
    pub body_features: Vec<BodyFeatureDefinitionDto>,
    /// Per-body viewport / manufacturing appearance. Additive; missing on
    /// older projects deserializes as empty.
    #[serde(default)]
    pub body_appearances: Vec<BodyAppearance>,
    /// Technical drawing intent. Generated projection curves are deliberately
    /// omitted and rebuilt from the current solid model.
    #[serde(default)]
    pub drawings: DrawingDocumentDto,
    /// Assembly intent references stable body/face topology. Display poses and
    /// solver caches are rebuilt and deliberately excluded.
    #[serde(default)]
    pub assembly: AssemblyDocumentDto,
    /// Browser eye-toggle choices. Additive so older projects remain valid.
    #[serde(default)]
    pub visibility: ProjectVisibilityDto,
    pub counters: ProjectCountersV2,
    pub preferences: ProjectPreferencesV2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectDocumentV2 {
    pub name: String,
    pub settings: DocumentSettings,
    pub history: FeatureTree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectSketchV2 {
    pub feature_id: FeatureId,
    pub name: String,
    pub plane: PlaneRef,
    pub basis: PlaneBasis,
    pub dimension_style: DimensionStyle,
    pub grid_snap: bool,
    pub snapshot: SketchSnapshot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ProjectCountersV2 {
    pub sketch: u32,
    pub extrude: u32,
    #[serde(default)]
    pub revolve: u32,
    #[serde(default)]
    pub sweep: u32,
    #[serde(default)]
    pub loft: u32,
    #[serde(default)]
    pub rib: u32,
    #[serde(default)]
    pub fillet: u32,
    #[serde(default)]
    pub chamfer: u32,
    #[serde(default)]
    pub hole: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ProjectPreferencesV2 {
    pub grid_snap: bool,
}

pub(crate) fn decode_project(json: &str) -> Result<ProjectModelV6, String> {
    let mut header: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("model.json is not valid JSON: {error}"))?;
    let format = header
        .get("format")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "model.json is missing its format identifier".to_string())?
        .to_string();
    let schema_version = header
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "model.json is missing its schema version".to_string())?;
    if format == LEGACY_PROJECT_FORMAT {
        header["format"] = serde_json::Value::String(PROJECT_FORMAT.to_string());
    } else if format != PROJECT_FORMAT {
        return Err(format!("unsupported project format '{format}'"));
    }
    match schema_version {
        1 => {
            migrate_v1_to_v2(&mut header)?;
            migrate_v2_to_v3(&mut header);
        }
        2 => migrate_v2_to_v3(&mut header),
        3 | 4 | 5 => {}
        version if version == u64::from(PROJECT_SCHEMA_VERSION) => {}
        _ => {
            return Err(format!(
                "project schema {schema_version} is not supported by this build (latest: {PROJECT_SCHEMA_VERSION})"
            ));
        }
    }

    // New relation fields default to empty for schema 1-3 projects; drawing
    // occurrence selection defaults to definition space for schema 1-4.
    // Schema 1-5 drawing guards remain absent: loading cannot establish historical
    // association. Users must explicitly reassociate unverified references.
    // Raising the version prevents old readers from saving away model intent.
    header["schema_version"] = serde_json::Value::from(PROJECT_SCHEMA_VERSION);
    let model: ProjectModelV6 = serde_json::from_value(header)
        .map_err(|error| format!("invalid project model: {error}"))?;
    validate_project(&model)?;
    Ok(model)
}

fn migrate_v1_to_v2(model: &mut serde_json::Value) -> Result<(), String> {
    let root = model
        .as_object_mut()
        .ok_or_else(|| "project model must be a JSON object".to_string())?;

    if let Some(style) = root
        .get_mut("document")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|document| document.get_mut("settings"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|settings| settings.get_mut("dimension_style"))
    {
        if style.as_str() != Some("iso") {
            *style = serde_json::Value::String("aligned".to_string());
        }
    }

    if let Some(sketches) = root
        .get_mut("sketches")
        .and_then(serde_json::Value::as_array_mut)
    {
        for sketch in sketches {
            if let Some(style) = sketch
                .as_object_mut()
                .and_then(|sketch| sketch.get_mut("dimension_style"))
            {
                if style.as_str() != Some("iso") {
                    *style = serde_json::Value::String("aligned".to_string());
                }
            }
        }
    }

    root.insert("schema_version".to_string(), serde_json::Value::from(2));
    Ok(())
}

fn migrate_v2_to_v3(model: &mut serde_json::Value) {
    // SketchSnapshot defaults missing dim_modes entries to Driving. Preserve
    // any explicit modes written by pre-release schema-2 reference builds.
    model["schema_version"] = serde_json::Value::from(3);
}

pub(crate) fn validate_project(model: &ProjectModelV6) -> Result<(), String> {
    if model.format != PROJECT_FORMAT || model.schema_version != PROJECT_SCHEMA_VERSION {
        return Err("project header does not match the supported schema".to_string());
    }
    if model.document.name.trim().is_empty() {
        return Err("document name cannot be empty".to_string());
    }
    if model.document.history.rollback_index > model.document.history.features.len() {
        return Err("rollback index is beyond the feature history".to_string());
    }
    model.drawings.validate()?;
    model.assembly.validate()?;

    let mut feature_ids = HashSet::new();
    for feature in &model.document.history.features {
        if feature.id.0 == 0 || !feature_ids.insert(feature.id) {
            return Err(format!("duplicate or zero feature id {}", feature.id.0));
        }
    }

    let mut sketch_ids = HashSet::new();
    let mut sketch_names = HashSet::new();
    for sketch in &model.sketches {
        if !sketch_ids.insert(sketch.feature_id) {
            return Err(format!(
                "duplicate saved sketch feature {}",
                sketch.feature_id.0
            ));
        }
        if sketch.name.trim().is_empty() || !sketch_names.insert(sketch.name.as_str()) {
            return Err(format!("duplicate or empty sketch name '{}'", sketch.name));
        }
        let feature = model
            .document
            .history
            .features
            .iter()
            .find(|feature| feature.id == sketch.feature_id)
            .ok_or_else(|| {
                format!(
                    "saved sketch '{}' has no feature-history entry",
                    sketch.name
                )
            })?;
        if feature.kind != FeatureKind::Sketch || feature.name != sketch.name {
            return Err(format!(
                "saved sketch '{}' does not match feature {}",
                sketch.name, sketch.feature_id.0
            ));
        }
        sketch
            .snapshot
            .validate()
            .map_err(|error| format!("{}: {error}", sketch.name))?;
    }

    let mut extrude_ids = HashSet::new();
    let mut revolve_ids = HashSet::new();
    let mut sweep_ids = HashSet::new();
    let mut loft_ids = HashSet::new();
    let mut rib_ids = HashSet::new();
    let mut fillet_ids = HashSet::new();
    let mut chamfer_ids = HashSet::new();
    let mut hole_ids = HashSet::new();
    let mut datum_plane_ids = HashSet::new();
    let mut datum_ids = HashSet::new();
    let mut body_feature_ids = HashSet::new();
    let mut reserved_body_ids = BTreeSet::new();
    for extrude in &model.extrudes {
        if !extrude_ids.insert(extrude.feature_id) {
            return Err(format!(
                "duplicate saved Extrude feature {}",
                extrude.feature_id.0
            ));
        }
        let feature = model
            .document
            .history
            .features
            .iter()
            .find(|feature| feature.id == extrude.feature_id)
            .ok_or_else(|| {
                format!(
                    "saved Extrude '{}' has no feature-history entry",
                    extrude.name
                )
            })?;
        if feature.kind != FeatureKind::Extrude || feature.name != extrude.name {
            return Err(format!(
                "saved Extrude '{}' does not match feature {}",
                extrude.name, extrude.feature_id.0
            ));
        }
        for id in &extrude.new_body_ids {
            if id.0 == 0 || !reserved_body_ids.insert(*id) {
                return Err(format!("duplicate or zero reserved body id {}", id.0));
            }
        }
    }

    for revolve in &model.revolves {
        if !revolve_ids.insert(revolve.feature_id) {
            return Err(format!(
                "duplicate saved Revolve feature {}",
                revolve.feature_id.0
            ));
        }
        let feature = model
            .document
            .history
            .features
            .iter()
            .find(|feature| feature.id == revolve.feature_id)
            .ok_or_else(|| {
                format!(
                    "saved Revolve '{}' has no feature-history entry",
                    revolve.name
                )
            })?;
        if feature.kind != FeatureKind::Revolve || feature.name != revolve.name {
            return Err(format!(
                "saved Revolve '{}' does not match feature {}",
                revolve.name, revolve.feature_id.0
            ));
        }
        for id in &revolve.new_body_ids {
            if id.0 == 0 || !reserved_body_ids.insert(*id) {
                return Err(format!("duplicate or zero reserved body id {}", id.0));
            }
        }
    }

    for sweep in &model.sweeps {
        if !sweep_ids.insert(sweep.feature_id) {
            return Err(format!(
                "duplicate saved Sweep feature {}",
                sweep.feature_id.0
            ));
        }
        validate_feature_entry(
            model,
            sweep.feature_id,
            &sweep.name,
            FeatureKind::Sweep,
            "Sweep",
        )?;
        if sweep.new_body_id.0 == 0 || !reserved_body_ids.insert(sweep.new_body_id) {
            return Err(format!(
                "duplicate or zero reserved body id {}",
                sweep.new_body_id.0
            ));
        }
    }

    for loft in &model.lofts {
        if !loft_ids.insert(loft.feature_id) {
            return Err(format!(
                "duplicate saved Loft feature {}",
                loft.feature_id.0
            ));
        }
        validate_feature_entry(
            model,
            loft.feature_id,
            &loft.name,
            FeatureKind::Loft,
            "Loft",
        )?;
        if loft.new_body_id.0 == 0 || !reserved_body_ids.insert(loft.new_body_id) {
            return Err(format!(
                "duplicate or zero reserved body id {}",
                loft.new_body_id.0
            ));
        }
    }

    for rib in &model.ribs {
        if !rib_ids.insert(rib.feature_id) {
            return Err(format!("duplicate saved Rib feature {}", rib.feature_id.0));
        }
        validate_feature_entry(model, rib.feature_id, &rib.name, FeatureKind::Rib, "Rib")?;
        for id in &rib.new_body_ids {
            if id.0 == 0 || !reserved_body_ids.insert(*id) {
                return Err(format!("duplicate or zero reserved body id {}", id.0));
            }
        }
    }

    for fillet in &model.fillets {
        if !fillet_ids.insert(fillet.feature_id) {
            return Err(format!(
                "duplicate saved Fillet feature {}",
                fillet.feature_id.0
            ));
        }
        validate_feature_entry(
            model,
            fillet.feature_id,
            &fillet.name,
            FeatureKind::Fillet,
            "Fillet",
        )?;
    }

    for chamfer in &model.chamfers {
        if !chamfer_ids.insert(chamfer.feature_id) {
            return Err(format!(
                "duplicate saved Chamfer feature {}",
                chamfer.feature_id.0
            ));
        }
        validate_feature_entry(
            model,
            chamfer.feature_id,
            &chamfer.name,
            FeatureKind::Chamfer,
            "Chamfer",
        )?;
    }

    for hole in &model.holes {
        if !hole_ids.insert(hole.feature_id) {
            return Err(format!(
                "duplicate saved Hole feature {}",
                hole.feature_id.0
            ));
        }
        validate_feature_entry(
            model,
            hole.feature_id,
            &hole.name,
            FeatureKind::Hole,
            "Hole",
        )?;
    }

    for plane in &model.datum_planes {
        if !datum_plane_ids.insert(plane.feature_id) {
            return Err(format!(
                "duplicate saved Construction Plane feature {}",
                plane.feature_id.0
            ));
        }
        if plane.datum_id.0 == 0 || !datum_ids.insert(plane.datum_id) {
            return Err(format!(
                "duplicate or zero construction plane id {}",
                plane.datum_id.0
            ));
        }
        validate_feature_entry(
            model,
            plane.feature_id,
            &plane.name,
            FeatureKind::ConstructionPlane,
            "Construction Plane",
        )?;
    }

    for definition in &model.body_features {
        let feature_id = definition.feature_id();
        if !body_feature_ids.insert(feature_id) {
            return Err(format!("duplicate saved body feature {}", feature_id.0));
        }
        let (kind, label) = match definition {
            BodyFeatureDefinitionDto::ExternalThread { .. } => {
                (FeatureKind::ExternalThread, "External Thread")
            }
            BodyFeatureDefinitionDto::MoveCopy { .. } => (FeatureKind::MoveCopy, "Move/Copy"),
            BodyFeatureDefinitionDto::Shell { .. } => (FeatureKind::Shell, "Shell"),
            BodyFeatureDefinitionDto::Mirror { .. } => (FeatureKind::Mirror, "Mirror"),
            BodyFeatureDefinitionDto::RectangularPattern { .. } => {
                (FeatureKind::RectangularPattern, "Rectangular Pattern")
            }
            BodyFeatureDefinitionDto::CircularPattern { .. } => {
                (FeatureKind::CircularPattern, "Circular Pattern")
            }
            BodyFeatureDefinitionDto::Combine { .. } => (FeatureKind::Combine, "Combine"),
            BodyFeatureDefinitionDto::SplitBody { .. } => (FeatureKind::SplitBody, "Split Body"),
            BodyFeatureDefinitionDto::ImportStep { .. } => (FeatureKind::ImportStep, "STEP Import"),
        };
        validate_feature_entry(model, feature_id, definition.name(), kind, label)?;
        let reserved: &[nbcad_core::BodyId] = match definition {
            BodyFeatureDefinitionDto::MoveCopy {
                copy: true,
                result_body_ids,
                ..
            } => result_body_ids,
            BodyFeatureDefinitionDto::MoveCopy { .. } => &[],
            BodyFeatureDefinitionDto::Mirror { new_body_ids, .. }
            | BodyFeatureDefinitionDto::RectangularPattern { new_body_ids, .. }
            | BodyFeatureDefinitionDto::CircularPattern { new_body_ids, .. } => new_body_ids,
            BodyFeatureDefinitionDto::SplitBody { new_body_id, .. } => {
                std::slice::from_ref(new_body_id)
            }
            BodyFeatureDefinitionDto::ImportStep { body_id, .. } => std::slice::from_ref(body_id),
            BodyFeatureDefinitionDto::ExternalThread { .. }
            | BodyFeatureDefinitionDto::Shell { .. }
            | BodyFeatureDefinitionDto::Combine { .. } => &[],
        };
        for id in reserved {
            if id.0 == 0 || !reserved_body_ids.insert(*id) {
                return Err(format!("duplicate or zero reserved body id {}", id.0));
            }
        }
    }

    for feature in &model.document.history.features {
        match feature.kind {
            FeatureKind::Sketch if !sketch_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved sketch", feature.name));
            }
            FeatureKind::Extrude if !extrude_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Extrude", feature.name));
            }
            FeatureKind::Revolve if !revolve_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Revolve", feature.name));
            }
            FeatureKind::Sweep if !sweep_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Sweep", feature.name));
            }
            FeatureKind::Loft if !loft_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Loft", feature.name));
            }
            FeatureKind::Rib if !rib_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Rib", feature.name));
            }
            FeatureKind::Fillet if !fillet_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Fillet", feature.name));
            }
            FeatureKind::Chamfer if !chamfer_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Chamfer", feature.name));
            }
            FeatureKind::Hole if !hole_ids.contains(&feature.id) => {
                return Err(format!("feature '{}' has no saved Hole", feature.name));
            }
            FeatureKind::ConstructionPlane if !datum_plane_ids.contains(&feature.id) => {
                return Err(format!(
                    "feature '{}' has no saved Construction Plane",
                    feature.name
                ));
            }
            FeatureKind::ExternalThread
            | FeatureKind::Shell
            | FeatureKind::Mirror
            | FeatureKind::RectangularPattern
            | FeatureKind::CircularPattern
            | FeatureKind::Combine
            | FeatureKind::SplitBody
            | FeatureKind::ImportStep
                if !body_feature_ids.contains(&feature.id) =>
            {
                return Err(format!(
                    "feature '{}' has no saved body operation",
                    feature.name
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_feature_entry(
    model: &ProjectModelV6,
    feature_id: FeatureId,
    name: &str,
    kind: FeatureKind,
    label: &str,
) -> Result<(), String> {
    let feature = model
        .document
        .history
        .features
        .iter()
        .find(|feature| feature.id == feature_id)
        .ok_or_else(|| format!("saved {label} '{name}' has no feature-history entry"))?;
    if feature.kind != kind || feature.name != name {
        return Err(format!(
            "saved {label} '{name}' does not match feature {}",
            feature_id.0
        ));
    }
    Ok(())
}
