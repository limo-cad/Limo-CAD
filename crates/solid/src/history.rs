use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use nbcad_core::{BodyId, EdgeId, FaceId, FeatureId, PlaneBasis};

use crate::dto::*;
use crate::stable;
use crate::thread::{iso_metric_thread_envelope, ThreadFit};

const MAX_EXTENT_MM: f64 = 1_000_000.0;
const MAX_STEP_BASE64_LENGTH: usize = 128 * 1024 * 1024;
const EPS: f64 = 1e-7;

#[derive(Debug, Clone, PartialEq)]
pub enum SolidError {
    PendingTransaction,
    NoPendingTransaction,
    WrongTransaction,
    SketchNotFound(String),
    ProfileNotFound { sketch: String, index: u32 },
    EmptySelection,
    InvalidExtent(String),
    InvalidTaper,
    InvalidAxis(String),
    InvalidPath(String),
    InvalidAngle,
    MissingTarget(BodyId),
    MissingFace(FaceId),
    MissingEdge(EdgeId),
    KernelContract(String),
    FeatureNotFound(FeatureId),
    InvalidHistory(String),
}

impl fmt::Display for SolidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolidError::PendingTransaction => write!(f, "a solid recompute is already pending"),
            SolidError::NoPendingTransaction => write!(f, "no solid recompute is pending"),
            SolidError::WrongTransaction => write!(f, "stale solid recompute result"),
            SolidError::SketchNotFound(name) => write!(f, "sketch '{name}' was not found"),
            SolidError::ProfileNotFound { sketch, index } => {
                write!(f, "profile {index} was not found in '{sketch}'")
            }
            SolidError::EmptySelection => write!(f, "select at least one closed profile"),
            SolidError::InvalidExtent(message) => write!(f, "{message}"),
            SolidError::InvalidTaper => write!(f, "taper angle must be between -89° and 89°"),
            SolidError::InvalidAxis(message) => write!(f, "{message}"),
            SolidError::InvalidPath(message) => write!(f, "{message}"),
            SolidError::InvalidAngle => {
                write!(f, "revolve angle must be non-zero and no greater than 360°")
            }
            SolidError::MissingTarget(id) => write!(f, "target body {} is not available", id.0),
            SolidError::MissingFace(id) => write!(f, "referenced face {} is not available", id.0),
            SolidError::MissingEdge(id) => write!(f, "referenced edge {} is not available", id.0),
            SolidError::KernelContract(message) => write!(f, "kernel result is invalid: {message}"),
            SolidError::FeatureNotFound(id) => write!(f, "feature {} was not found", id.0),
            SolidError::InvalidHistory(message) => {
                write!(f, "invalid solid feature history: {message}")
            }
        }
    }
}

impl std::error::Error for SolidError {}

#[derive(Debug, Clone)]
struct Pending {
    transaction_id: u64,
    extrudes: Vec<ExtrudeDefinitionDto>,
    revolves: Vec<RevolveDefinitionDto>,
    sweeps: Vec<SweepDefinitionDto>,
    lofts: Vec<LoftDefinitionDto>,
    ribs: Vec<RibDefinitionDto>,
    fillets: Vec<SolidFilletDefinitionDto>,
    chamfers: Vec<SolidChamferDefinitionDto>,
    holes: Vec<HoleDefinitionDto>,
    body_features: Vec<BodyFeatureDefinitionDto>,
    feature_order: BTreeMap<FeatureId, usize>,
}

#[derive(Debug, Clone)]
pub struct SolidDocument {
    extrudes: Vec<ExtrudeDefinitionDto>,
    revolves: Vec<RevolveDefinitionDto>,
    sweeps: Vec<SweepDefinitionDto>,
    lofts: Vec<LoftDefinitionDto>,
    ribs: Vec<RibDefinitionDto>,
    fillets: Vec<SolidFilletDefinitionDto>,
    chamfers: Vec<SolidChamferDefinitionDto>,
    holes: Vec<HoleDefinitionDto>,
    body_features: Vec<BodyFeatureDefinitionDto>,
    /// Authoritative document timeline order. Stable FeatureIds identify
    /// definitions; their numeric allocation order is not their build order
    /// after a dependency-safe drag reorder.
    feature_order: BTreeMap<FeatureId, usize>,
    scene: SolidSceneDto,
    next_body_id: u64,
    next_transaction_id: u64,
    pending: Option<Pending>,
}

impl Default for SolidDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl SolidDocument {
    pub fn new() -> Self {
        Self {
            extrudes: Vec::new(),
            revolves: Vec::new(),
            sweeps: Vec::new(),
            lofts: Vec::new(),
            ribs: Vec::new(),
            fillets: Vec::new(),
            chamfers: Vec::new(),
            holes: Vec::new(),
            body_features: Vec::new(),
            feature_order: BTreeMap::new(),
            scene: SolidSceneDto::default(),
            next_body_id: 1,
            next_transaction_id: 1,
            pending: None,
        }
    }

    pub fn definitions(&self) -> &[ExtrudeDefinitionDto] {
        &self.extrudes
    }

    pub fn set_feature_order(&mut self, order: &[FeatureId]) -> Result<(), SolidError> {
        self.ensure_idle()?;
        self.feature_order = order
            .iter()
            .enumerate()
            .map(|(index, feature_id)| (*feature_id, index))
            .collect();
        Ok(())
    }

    pub fn revolve_definitions(&self) -> &[RevolveDefinitionDto] {
        &self.revolves
    }

    pub fn sweep_definitions(&self) -> &[SweepDefinitionDto] {
        &self.sweeps
    }

    pub fn loft_definitions(&self) -> &[LoftDefinitionDto] {
        &self.lofts
    }

    pub fn rib_definitions(&self) -> &[RibDefinitionDto] {
        &self.ribs
    }

    pub fn fillet_definitions(&self) -> &[SolidFilletDefinitionDto] {
        &self.fillets
    }

    pub fn chamfer_definitions(&self) -> &[SolidChamferDefinitionDto] {
        &self.chamfers
    }

    pub fn hole_definitions(&self) -> &[HoleDefinitionDto] {
        &self.holes
    }

    pub fn body_feature_definitions(&self) -> &[BodyFeatureDefinitionDto] {
        &self.body_features
    }

    /// Stable bodies owned by one history feature, including identities that
    /// are currently rolled back. Assembly cleanup calls this only for an
    /// explicit deletion; moving the build cursor must preserve references.
    pub fn owned_body_ids_for_feature(&self, feature_id: FeatureId) -> Vec<BodyId> {
        body_owners(
            &self.extrudes,
            &self.revolves,
            &self.sweeps,
            &self.lofts,
            &self.ribs,
            &self.fillets,
            &self.chamfers,
            &self.holes,
            &self.body_features,
            &self.feature_order,
        )
        .into_iter()
        .filter_map(|(body_id, owner)| (owner == feature_id).then_some(body_id))
        .collect()
    }

    /// Bodies produced by retained history, including outputs outside the
    /// current rollback stage or consumed by a later feature. Mere references
    /// from modifiers cannot keep a deleted creator's metadata alive.
    pub fn retained_body_ids(&self) -> BTreeSet<BodyId> {
        let mut retained = BTreeSet::new();
        let mut add_outputs = |operation, outputs: &[BodyId], targets: &[BodyId]| match operation {
            ExtrudeOperation::NewBody => retained.extend(outputs.iter().copied()),
            ExtrudeOperation::Join if targets.is_empty() => {
                retained.extend(outputs.first().copied());
            }
            ExtrudeOperation::Join | ExtrudeOperation::Cut | ExtrudeOperation::Intersect => {}
        };
        for definition in &self.extrudes {
            let count = definition
                .source_face
                .map(|_| 1)
                .unwrap_or(definition.profile_indices.len());
            add_outputs(
                definition.operation,
                &definition.new_body_ids[..count.min(definition.new_body_ids.len())],
                &definition.target_body_ids,
            );
        }
        for definition in &self.revolves {
            add_outputs(
                definition.operation,
                &definition.new_body_ids[..definition
                    .profile_indices
                    .len()
                    .min(definition.new_body_ids.len())],
                &definition.target_body_ids,
            );
        }
        for definition in &self.sweeps {
            add_outputs(
                definition.operation,
                std::slice::from_ref(&definition.new_body_id),
                &definition.target_body_ids,
            );
        }
        for definition in &self.lofts {
            add_outputs(
                definition.operation,
                std::slice::from_ref(&definition.new_body_id),
                &definition.target_body_ids,
            );
        }
        for definition in &self.ribs {
            add_outputs(
                definition.operation,
                &definition.new_body_ids[..definition
                    .line_entity_ids
                    .len()
                    .min(definition.new_body_ids.len())],
                &definition.target_body_ids,
            );
        }
        for definition in &self.body_features {
            match definition {
                BodyFeatureDefinitionDto::MoveCopy {
                    copy,
                    result_body_ids,
                    ..
                } => {
                    if *copy {
                        retained.extend(result_body_ids.iter().copied());
                    }
                }
                BodyFeatureDefinitionDto::Mirror { new_body_ids, .. }
                | BodyFeatureDefinitionDto::RectangularPattern { new_body_ids, .. }
                | BodyFeatureDefinitionDto::CircularPattern { new_body_ids, .. } => {
                    retained.extend(new_body_ids.iter().copied());
                }
                BodyFeatureDefinitionDto::SplitBody { new_body_id, .. } => {
                    retained.insert(*new_body_id);
                }
                BodyFeatureDefinitionDto::ImportStep { body_id, .. } => {
                    retained.insert(*body_id);
                }
                BodyFeatureDefinitionDto::ExternalThread { .. }
                | BodyFeatureDefinitionDto::Shell { .. }
                | BodyFeatureDefinitionDto::Combine { .. } => {}
            }
        }
        retained
    }

    pub fn scene(&self) -> &SolidSceneDto {
        &self.scene
    }

    /// Restore persistent feature definitions without restoring tessellation
    /// or B-reps. The caller immediately requests a full kernel recompute.
    pub fn restore_definitions(definitions: Vec<ExtrudeDefinitionDto>) -> Result<Self, SolidError> {
        Self::restore_feature_definitions(
            definitions,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn restore_feature_definitions(
        extrudes: Vec<ExtrudeDefinitionDto>,
        revolves: Vec<RevolveDefinitionDto>,
        sweeps: Vec<SweepDefinitionDto>,
        lofts: Vec<LoftDefinitionDto>,
        ribs: Vec<RibDefinitionDto>,
        fillets: Vec<SolidFilletDefinitionDto>,
        chamfers: Vec<SolidChamferDefinitionDto>,
        holes: Vec<HoleDefinitionDto>,
        body_features: Vec<BodyFeatureDefinitionDto>,
    ) -> Result<Self, SolidError> {
        let mut feature_ids = BTreeSet::new();
        let mut body_ids = BTreeSet::new();
        let mut max_body_id = 0u64;
        for definition in &extrudes {
            if definition.feature_id.0 == 0 || !feature_ids.insert(definition.feature_id) {
                return Err(SolidError::InvalidHistory(format!(
                    "duplicate or zero feature id {}",
                    definition.feature_id.0
                )));
            }
            for body_id in &definition.new_body_ids {
                if body_id.0 == 0 || !body_ids.insert(*body_id) {
                    return Err(SolidError::InvalidHistory(format!(
                        "duplicate or zero reserved body id {}",
                        body_id.0
                    )));
                }
                max_body_id = max_body_id.max(body_id.0);
            }
        }
        for definition in &revolves {
            if definition.feature_id.0 == 0 || !feature_ids.insert(definition.feature_id) {
                return Err(SolidError::InvalidHistory(format!(
                    "duplicate or zero feature id {}",
                    definition.feature_id.0
                )));
            }
            for body_id in &definition.new_body_ids {
                if body_id.0 == 0 || !body_ids.insert(*body_id) {
                    return Err(SolidError::InvalidHistory(format!(
                        "duplicate or zero reserved body id {}",
                        body_id.0
                    )));
                }
                max_body_id = max_body_id.max(body_id.0);
            }
        }
        for (feature_id, reserved_ids) in sweeps
            .iter()
            .map(|definition| (definition.feature_id, vec![definition.new_body_id]))
            .chain(
                lofts
                    .iter()
                    .map(|definition| (definition.feature_id, vec![definition.new_body_id])),
            )
            .chain(
                ribs.iter()
                    .map(|definition| (definition.feature_id, definition.new_body_ids.clone())),
            )
        {
            if feature_id.0 == 0 || !feature_ids.insert(feature_id) {
                return Err(SolidError::InvalidHistory(format!(
                    "duplicate or zero feature id {}",
                    feature_id.0
                )));
            }
            for body_id in reserved_ids {
                if body_id.0 == 0 || !body_ids.insert(body_id) {
                    return Err(SolidError::InvalidHistory(format!(
                        "duplicate or zero reserved body id {}",
                        body_id.0
                    )));
                }
                max_body_id = max_body_id.max(body_id.0);
            }
        }
        for (feature_id, body_id) in fillets
            .iter()
            .map(|definition| (definition.feature_id, definition.body_id))
            .chain(
                chamfers
                    .iter()
                    .map(|definition| (definition.feature_id, definition.body_id)),
            )
            .chain(
                holes
                    .iter()
                    .map(|definition| (definition.feature_id, definition.body_id)),
            )
        {
            if feature_id.0 == 0 || !feature_ids.insert(feature_id) {
                return Err(SolidError::InvalidHistory(format!(
                    "duplicate or zero feature id {}",
                    feature_id.0
                )));
            }
            if body_id.0 == 0 {
                return Err(SolidError::InvalidHistory(
                    "a refinement feature references body id 0".to_string(),
                ));
            }
        }
        for definition in &body_features {
            let feature_id = definition.feature_id();
            if feature_id.0 == 0 || !feature_ids.insert(feature_id) {
                return Err(SolidError::InvalidHistory(format!(
                    "duplicate or zero feature id {}",
                    feature_id.0
                )));
            }
            let reserved: &[BodyId] = match definition {
                BodyFeatureDefinitionDto::MoveCopy {
                    copy,
                    result_body_ids,
                    ..
                } if *copy => result_body_ids,
                BodyFeatureDefinitionDto::MoveCopy { .. } => &[],
                BodyFeatureDefinitionDto::Mirror { new_body_ids, .. }
                | BodyFeatureDefinitionDto::RectangularPattern { new_body_ids, .. }
                | BodyFeatureDefinitionDto::CircularPattern { new_body_ids, .. } => new_body_ids,
                BodyFeatureDefinitionDto::SplitBody { new_body_id, .. } => {
                    std::slice::from_ref(new_body_id)
                }
                BodyFeatureDefinitionDto::ImportStep { body_id, .. } => {
                    std::slice::from_ref(body_id)
                }
                BodyFeatureDefinitionDto::ExternalThread { .. }
                | BodyFeatureDefinitionDto::Shell { .. }
                | BodyFeatureDefinitionDto::Combine { .. } => &[],
            };
            for body_id in reserved {
                if body_id.0 == 0 || !body_ids.insert(*body_id) {
                    return Err(SolidError::InvalidHistory(format!(
                        "duplicate or zero reserved body id {}",
                        body_id.0
                    )));
                }
                max_body_id = max_body_id.max(body_id.0);
            }
        }
        Ok(Self {
            extrudes,
            revolves,
            sweeps,
            lofts,
            ribs,
            fillets,
            chamfers,
            holes,
            body_features,
            feature_order: BTreeMap::new(),
            scene: SolidSceneDto::default(),
            next_body_id: max_body_id.saturating_add(1).max(1),
            next_transaction_id: 1,
            pending: None,
        })
    }

    pub fn face_basis(&self, id: FaceId) -> Option<PlaneBasis> {
        self.scene
            .bodies
            .iter()
            .flat_map(|body| &body.faces)
            .find(|face| face.id == id)
            .and_then(|face| face.plane)
    }

    /// Area-weighted center of the tessellated face. The result lies on the
    /// support plane for planar faces and gives face-hosted sketches a useful
    /// local zero instead of inheriting an arbitrary kernel surface origin.
    pub fn face_center(&self, id: FaceId) -> Option<[f64; 3]> {
        let (body, face) = self.scene.bodies.iter().find_map(|body| {
            body.faces
                .iter()
                .find(|face| face.id == id)
                .map(|face| (body, face))
        })?;
        let end = face
            .first_index
            .saturating_add(face.index_count)
            .min(body.mesh.indices.len() as u32) as usize;
        let mut weighted = [0.0; 3];
        let mut total_weight = 0.0;
        for offset in (face.first_index as usize..end).step_by(3) {
            if offset + 2 >= end {
                break;
            }
            let mut points = [[0.0; 3]; 3];
            let mut valid = true;
            for corner in 0..3 {
                let vertex = body.mesh.indices[offset + corner] as usize;
                let base = vertex.saturating_mul(3);
                if base + 2 >= body.mesh.positions.len() {
                    valid = false;
                    break;
                }
                points[corner] = [
                    body.mesh.positions[base] as f64,
                    body.mesh.positions[base + 1] as f64,
                    body.mesh.positions[base + 2] as f64,
                ];
            }
            if !valid {
                continue;
            }
            let ab = [
                points[1][0] - points[0][0],
                points[1][1] - points[0][1],
                points[1][2] - points[0][2],
            ];
            let ac = [
                points[2][0] - points[0][0],
                points[2][1] - points[0][1],
                points[2][2] - points[0][2],
            ];
            // Twice the triangle area is a sufficient positive weight.
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let weight = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            if weight <= EPS {
                continue;
            }
            for axis in 0..3 {
                weighted[axis] +=
                    weight * (points[0][axis] + points[1][axis] + points[2][axis]) / 3.0;
            }
            total_weight += weight;
        }
        (total_weight > EPS).then(|| {
            [
                weighted[0] / total_weight,
                weighted[1] / total_weight,
                weighted[2] / total_weight,
            ]
        })
    }

    pub fn has_face(&self, id: FaceId) -> bool {
        self.scene
            .bodies
            .iter()
            .any(|body| body.faces.iter().any(|face| face.id == id))
    }

    pub fn edge_points(&self, body_id: BodyId, edge_id: EdgeId) -> Option<Vec<Point3Dto>> {
        self.scene
            .bodies
            .iter()
            .find(|body| body.id == body_id)?
            .edges
            .iter()
            .find(|edge| edge.id == edge_id)
            .map(|edge| edge.points.clone())
    }

    pub fn resolve_plane_ref(&self, reference: nbcad_core::PlaneRef) -> Option<PlaneBasis> {
        match reference {
            nbcad_core::PlaneRef::OriginPlane { .. } => reference.origin_basis().ok(),
            nbcad_core::PlaneRef::PlanarFace { face_id } => self.face_basis(face_id),
            nbcad_core::PlaneRef::DatumPlane { .. } => None,
        }
    }

    /// Refresh cached construction-plane placements used by body Mirror and
    /// Split Body definitions.
    pub fn refresh_datum_plane_basis(&mut self, datum_id: FaceId, basis: PlaneBasis) {
        for definition in &mut self.body_features {
            match definition {
                BodyFeatureDefinitionDto::Mirror {
                    plane:
                        nbcad_core::PlaneRef::DatumPlane {
                            datum_id: referenced,
                        },
                    plane_basis,
                    ..
                }
                | BodyFeatureDefinitionDto::SplitBody {
                    plane:
                        nbcad_core::PlaneRef::DatumPlane {
                            datum_id: referenced,
                        },
                    plane_basis,
                    ..
                } if *referenced == datum_id => *plane_basis = basis,
                _ => {}
            }
        }
    }

    pub fn prepare_add(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: ExtrudeRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut definitions = self.extrudes.clone();
        if request.source_face.is_some() && !request.profile_indices.is_empty() {
            return Err(SolidError::InvalidHistory(
                "Extrude cannot mix a planar face with sketch profiles".to_string(),
            ));
        }
        let (source_face_key, source_face_basis, source_face_signature) = match request.source_face
        {
            Some(source) => {
                let (key, basis, signature) = resolve_planar_face_source(&self.scene, source)?;
                (Some(key), Some(basis), Some(signature))
            }
            None => (None, None, None),
        };
        let count = if request.source_face.is_some() {
            1
        } else {
            request.profile_indices.len()
        };
        if count == 0 {
            return Err(SolidError::EmptySelection);
        }
        let mut new_body_ids = Vec::with_capacity(count);
        for _ in 0..count {
            new_body_ids.push(self.alloc_body_id());
        }
        definitions.push(ExtrudeDefinitionDto {
            feature_id,
            name: name.into(),
            source_face: request.source_face,
            source_face_key,
            source_face_signature,
            source_face_basis,
            sketch_name: if request.source_face.is_some() {
                String::new()
            } else {
                request.sketch_name
            },
            profile_indices: if request.source_face.is_some() {
                Vec::new()
            } else {
                request.profile_indices
            },
            operation: request.operation,
            extent: request.extent,
            taper_angle_deg: request.taper_angle_deg,
            flip: request.flip,
            target_body_ids: request.target_body_ids,
            to_face_basis: extent_face_basis(request.extent, &self.scene),
            new_body_ids,
        });
        self.prepare(
            definitions,
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit(
        &mut self,
        feature_id: FeatureId,
        request: ExtrudeRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut definitions = self.extrudes.clone();
        if request.source_face.is_some() && !request.profile_indices.is_empty() {
            return Err(SolidError::InvalidHistory(
                "Extrude cannot mix a planar face with sketch profiles".to_string(),
            ));
        }
        let to_face_basis = extent_face_basis(request.extent, &self.scene);
        let existing = definitions
            .iter()
            .find(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        let (source_face_key, source_face_basis, source_face_signature) = match request.source_face
        {
            Some(source) if existing.source_face == Some(source) => (
                existing.source_face_key.clone().or_else(|| {
                    resolve_planar_face_source(&self.scene, source)
                        .ok()
                        .map(|resolved| resolved.0)
                }),
                existing.source_face_basis.or_else(|| {
                    resolve_planar_face_source(&self.scene, source)
                        .ok()
                        .map(|resolved| resolved.1)
                }),
                existing.source_face_signature.or_else(|| {
                    resolve_planar_face_source(&self.scene, source)
                        .ok()
                        .map(|resolved| resolved.2)
                }),
            ),
            Some(source) => {
                let (key, basis, signature) = resolve_planar_face_source(&self.scene, source)?;
                (Some(key), Some(basis), Some(signature))
            }
            None => (None, None, None),
        };
        let source_count = if request.source_face.is_some() {
            1
        } else {
            request.profile_indices.len()
        };
        if source_count == 0 {
            return Err(SolidError::EmptySelection);
        }
        let definition = definitions
            .iter_mut()
            .find(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        while definition.new_body_ids.len() < source_count {
            definition.new_body_ids.push(self.alloc_body_id());
        }
        definition.source_face = request.source_face;
        definition.source_face_key = source_face_key;
        definition.source_face_basis = source_face_basis;
        definition.source_face_signature = source_face_signature;
        definition.sketch_name = if request.source_face.is_some() {
            String::new()
        } else {
            request.sketch_name
        };
        definition.profile_indices = if request.source_face.is_some() {
            Vec::new()
        } else {
            request.profile_indices
        };
        definition.operation = request.operation;
        definition.extent = request.extent;
        definition.taper_angle_deg = request.taper_angle_deg;
        definition.flip = request.flip;
        definition.target_body_ids = request.target_body_ids;
        definition.to_face_basis = to_face_basis;
        self.prepare(
            definitions,
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_revolve(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: RevolveRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        if request.profile_indices.is_empty() {
            return Err(SolidError::EmptySelection);
        }
        let mut new_body_ids = Vec::with_capacity(request.profile_indices.len());
        for _ in &request.profile_indices {
            new_body_ids.push(self.alloc_body_id());
        }
        let mut revolves = self.revolves.clone();
        revolves.push(RevolveDefinitionDto {
            feature_id,
            name: name.into(),
            sketch_name: request.sketch_name,
            profile_indices: request.profile_indices,
            axis_origin: request.axis_origin,
            axis_direction: request.axis_direction,
            axis_line_sketch_name: request.axis_line_sketch_name,
            axis_line_entity_id: request.axis_line_entity_id,
            angle_deg: request.angle_deg,
            flip: request.flip,
            operation: request.operation,
            target_body_ids: request.target_body_ids,
            new_body_ids,
        });
        self.prepare(
            self.extrudes.clone(),
            revolves,
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_revolve(
        &mut self,
        feature_id: FeatureId,
        request: RevolveRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut revolves = self.revolves.clone();
        let definition = revolves
            .iter_mut()
            .find(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        while definition.new_body_ids.len() < request.profile_indices.len() {
            definition.new_body_ids.push(self.alloc_body_id());
        }
        definition.sketch_name = request.sketch_name;
        definition.profile_indices = request.profile_indices;
        definition.axis_origin = request.axis_origin;
        definition.axis_direction = request.axis_direction;
        definition.axis_line_sketch_name = request.axis_line_sketch_name;
        definition.axis_line_entity_id = request.axis_line_entity_id;
        definition.angle_deg = request.angle_deg;
        definition.flip = request.flip;
        definition.operation = request.operation;
        definition.target_body_ids = request.target_body_ids;
        self.prepare(
            self.extrudes.clone(),
            revolves,
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_sweep(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: SweepRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut sweeps = self.sweeps.clone();
        sweeps.push(SweepDefinitionDto {
            feature_id,
            name: name.into(),
            profile: request.profile,
            path_sketch_name: request.path_sketch_name,
            path_entity_ids: request.path_entity_ids,
            operation: request.operation,
            target_body_ids: request.target_body_ids,
            new_body_id: self.alloc_body_id(),
            guide_rail: request.guide_rail,
            orientation: request.orientation,
            transition: request.transition,
            force_c1: request.force_c1,
        });
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            sweeps,
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_sweep(
        &mut self,
        feature_id: FeatureId,
        request: SweepRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut sweeps = self.sweeps.clone();
        let definition = sweeps
            .iter_mut()
            .find(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        definition.profile = request.profile;
        definition.path_sketch_name = request.path_sketch_name;
        definition.path_entity_ids = request.path_entity_ids;
        definition.operation = request.operation;
        definition.target_body_ids = request.target_body_ids;
        definition.guide_rail = request.guide_rail;
        definition.orientation = request.orientation;
        definition.transition = request.transition;
        definition.force_c1 = request.force_c1;
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            sweeps,
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_loft(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: LoftRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut lofts = self.lofts.clone();
        lofts.push(LoftDefinitionDto {
            feature_id,
            name: name.into(),
            sections: request.sections,
            ruled: request.ruled,
            operation: request.operation,
            target_body_ids: request.target_body_ids,
            new_body_id: self.alloc_body_id(),
            continuity: request.continuity,
            centerline: request.centerline,
            guide_rail: request.guide_rail,
        });
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            lofts,
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_loft(
        &mut self,
        feature_id: FeatureId,
        request: LoftRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut lofts = self.lofts.clone();
        let definition = lofts
            .iter_mut()
            .find(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        definition.sections = request.sections;
        definition.ruled = request.ruled;
        definition.operation = request.operation;
        definition.target_body_ids = request.target_body_ids;
        definition.continuity = request.continuity;
        definition.centerline = request.centerline;
        definition.guide_rail = request.guide_rail;
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            lofts,
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_rib(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: RibRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        if request.line_entity_ids.is_empty() {
            return Err(SolidError::EmptySelection);
        }
        let mut new_body_ids = Vec::with_capacity(request.line_entity_ids.len());
        for _ in &request.line_entity_ids {
            new_body_ids.push(self.alloc_body_id());
        }
        let mut ribs = self.ribs.clone();
        ribs.push(RibDefinitionDto {
            feature_id,
            name: name.into(),
            sketch_name: request.sketch_name,
            line_entity_ids: request.line_entity_ids,
            thickness: request.thickness,
            depth: request.depth,
            symmetric: request.symmetric,
            flip: request.flip,
            operation: request.operation,
            target_body_ids: request.target_body_ids,
            new_body_ids,
            extent: request.extent,
            to_face_basis: rib_extent_face_basis(request.extent, &self.scene),
        });
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            ribs,
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_rib(
        &mut self,
        feature_id: FeatureId,
        request: RibRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut ribs = self.ribs.clone();
        let index = ribs
            .iter()
            .position(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        while ribs[index].new_body_ids.len() < request.line_entity_ids.len() {
            let body_id = self.alloc_body_id();
            ribs[index].new_body_ids.push(body_id);
        }
        let definition = &mut ribs[index];
        definition.sketch_name = request.sketch_name;
        definition.line_entity_ids = request.line_entity_ids;
        definition.thickness = request.thickness;
        definition.depth = request.depth;
        definition.symmetric = request.symmetric;
        definition.flip = request.flip;
        definition.operation = request.operation;
        definition.target_body_ids = request.target_body_ids;
        definition.extent = request.extent;
        definition.to_face_basis = rib_extent_face_basis(request.extent, &self.scene);
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            ribs,
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_fillet(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: SolidFilletRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        validate_positive(request.radius, "fillet radius")?;
        let edge_keys = edge_keys_for(&self.scene, request.body_id, &request.edge_ids)?;
        let mut fillets = self.fillets.clone();
        fillets.push(SolidFilletDefinitionDto {
            feature_id,
            name: name.into(),
            body_id: request.body_id,
            edge_ids: request.edge_ids,
            edge_keys,
            radius: request.radius,
            tangent_chain: request.tangent_chain,
        });
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            fillets,
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_fillet(
        &mut self,
        feature_id: FeatureId,
        request: SolidFilletRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        validate_positive(request.radius, "fillet radius")?;
        let edge_keys = edge_keys_for(&self.scene, request.body_id, &request.edge_ids)?;
        let mut fillets = self.fillets.clone();
        let definition = fillets
            .iter_mut()
            .find(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        definition.body_id = request.body_id;
        definition.edge_ids = request.edge_ids;
        definition.edge_keys = edge_keys;
        definition.radius = request.radius;
        definition.tangent_chain = request.tangent_chain;
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            fillets,
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_chamfer(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: SolidChamferRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        validate_positive(request.distance, "chamfer distance")?;
        let edge_keys = edge_keys_for(&self.scene, request.body_id, &request.edge_ids)?;
        let mut chamfers = self.chamfers.clone();
        chamfers.push(SolidChamferDefinitionDto {
            feature_id,
            name: name.into(),
            body_id: request.body_id,
            edge_ids: request.edge_ids,
            edge_keys,
            distance: request.distance,
            tangent_chain: request.tangent_chain,
        });
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            chamfers,
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_chamfer(
        &mut self,
        feature_id: FeatureId,
        request: SolidChamferRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        validate_positive(request.distance, "chamfer distance")?;
        let edge_keys = edge_keys_for(&self.scene, request.body_id, &request.edge_ids)?;
        let mut chamfers = self.chamfers.clone();
        let definition = chamfers
            .iter_mut()
            .find(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        definition.body_id = request.body_id;
        definition.edge_ids = request.edge_ids;
        definition.edge_keys = edge_keys;
        definition.distance = request.distance;
        definition.tangent_chain = request.tangent_chain;
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            chamfers,
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_hole(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: HoleRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        validate_hole(&request)?;
        let basis = support_face_basis(&self.scene, request.body_id, request.face_id)?;
        let mut holes = self.holes.clone();
        holes.push(hole_definition(feature_id, name.into(), request, basis));
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            holes,
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_hole(
        &mut self,
        feature_id: FeatureId,
        request: HoleRequest,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        validate_hole(&request)?;
        let basis = support_face_basis(&self.scene, request.body_id, request.face_id)?;
        let mut holes = self.holes.clone();
        let index = holes
            .iter()
            .position(|definition| definition.feature_id == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        let name = holes[index].name.clone();
        holes[index] = hole_definition(feature_id, name, request, basis);
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            holes,
            catalog,
            active_features,
        )
    }

    pub fn prepare_add_body_feature(
        &mut self,
        feature_id: FeatureId,
        name: impl Into<String>,
        request: BodyFeatureRequestDto,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let definition =
            self.make_body_feature_definition(feature_id, name.into(), request, None)?;
        let mut body_features = self.body_features.clone();
        body_features.push(definition);
        self.prepare_with_body_features(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            body_features,
            catalog,
            active_features,
        )
    }

    pub fn prepare_edit_body_feature(
        &mut self,
        feature_id: FeatureId,
        request: BodyFeatureRequestDto,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut body_features = self.body_features.clone();
        let index = body_features
            .iter()
            .position(|definition| definition.feature_id() == feature_id)
            .ok_or(SolidError::FeatureNotFound(feature_id))?;
        let name = body_features[index].name().to_string();
        let replacement = self.make_body_feature_definition(
            feature_id,
            name,
            request,
            Some(&body_features[index]),
        )?;
        body_features[index] = replacement;
        self.prepare_with_body_features(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            body_features,
            catalog,
            active_features,
        )
    }

    fn make_body_feature_definition(
        &mut self,
        feature_id: FeatureId,
        name: String,
        request: BodyFeatureRequestDto,
        existing: Option<&BodyFeatureDefinitionDto>,
    ) -> Result<BodyFeatureDefinitionDto, SolidError> {
        let mut reuse_or_allocate = |count: usize| {
            let mut ids = match existing {
                Some(BodyFeatureDefinitionDto::MoveCopy {
                    result_body_ids, ..
                }) => result_body_ids.clone(),
                Some(BodyFeatureDefinitionDto::Mirror { new_body_ids, .. })
                | Some(BodyFeatureDefinitionDto::RectangularPattern { new_body_ids, .. })
                | Some(BodyFeatureDefinitionDto::CircularPattern { new_body_ids, .. }) => {
                    new_body_ids.clone()
                }
                Some(BodyFeatureDefinitionDto::SplitBody { new_body_id, .. }) => {
                    vec![*new_body_id]
                }
                Some(BodyFeatureDefinitionDto::ImportStep { body_id, .. }) => {
                    vec![*body_id]
                }
                _ => Vec::new(),
            };
            while ids.len() < count {
                ids.push(self.alloc_body_id());
            }
            ids.truncate(count);
            ids
        };

        Ok(match request {
            BodyFeatureRequestDto::ExternalThread(request) => {
                let (face_key, cylinder) = match existing {
                    Some(BodyFeatureDefinitionDto::ExternalThread {
                        body_id,
                        face_id,
                        face_key,
                        cylinder,
                        ..
                    }) if *body_id == request.body_id && *face_id == request.face_id => {
                        (face_key.clone(), *cylinder)
                    }
                    _ => resolve_cylindrical_face_source(
                        &self.scene,
                        request.body_id,
                        request.face_id,
                    )?,
                };
                validate_external_thread(&request.thread, cylinder.radius * 2.0)?;
                BodyFeatureDefinitionDto::ExternalThread {
                    feature_id,
                    name,
                    body_id: request.body_id,
                    face_id: request.face_id,
                    face_key,
                    cylinder,
                    thread: request.thread,
                    flip: request.flip,
                }
            }
            BodyFeatureRequestDto::Shell(request) => {
                validate_positive(request.thickness.abs(), "shell thickness")?;
                if request.face_ids.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                let face_keys = face_keys_for(&self.scene, request.body_id, &request.face_ids)?;
                BodyFeatureDefinitionDto::Shell {
                    feature_id,
                    name,
                    body_id: request.body_id,
                    face_ids: request.face_ids,
                    face_keys,
                    thickness: request.thickness.abs(),
                    inward: request.inward,
                }
            }
            BodyFeatureRequestDto::MoveCopy(request) => {
                if request.body_ids.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                validate_vector_finite(request.translation, "move translation")?;
                validate_vector_finite(request.pivot, "move pivot")?;
                let rotation = normalized_quaternion(request.rotation)?;
                // A move is a placement feature on the selected body's stable
                // identity. Only Copy creates new body identities. Keeping the
                // IDs stable is essential for components, joints, visibility,
                // and downstream feature references.
                let result_body_ids = if request.copy {
                    match existing {
                        Some(BodyFeatureDefinitionDto::MoveCopy {
                            copy: true,
                            result_body_ids,
                            ..
                        }) if result_body_ids.len() == request.body_ids.len()
                            && result_body_ids
                                .iter()
                                .all(|id| !request.body_ids.contains(id)) =>
                        {
                            result_body_ids.clone()
                        }
                        _ => (0..request.body_ids.len())
                            .map(|_| self.alloc_body_id())
                            .collect(),
                    }
                } else {
                    request.body_ids.clone()
                };
                BodyFeatureDefinitionDto::MoveCopy {
                    feature_id,
                    name,
                    body_ids: request.body_ids,
                    translation: request.translation,
                    rotation,
                    pivot: request.pivot,
                    copy: request.copy,
                    result_body_ids,
                }
            }
            BodyFeatureRequestDto::Mirror(request) => {
                if request.body_ids.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                let plane_basis = request.plane_basis.ok_or_else(|| {
                    SolidError::InvalidAxis("mirror plane could not be resolved".to_string())
                })?;
                let new_body_ids = reuse_or_allocate(request.body_ids.len());
                BodyFeatureDefinitionDto::Mirror {
                    feature_id,
                    name,
                    body_ids: request.body_ids,
                    plane: request.plane,
                    plane_basis,
                    new_body_ids,
                }
            }
            BodyFeatureRequestDto::RectangularPattern(request) => {
                if request.body_ids.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                validate_pattern_count(request.count, "first direction")?;
                validate_positive(request.spacing.abs(), "pattern spacing")?;
                validate_vector(request.direction, "pattern direction")?;
                let second_count = request.second_count.max(1);
                if second_count > 1 {
                    validate_positive(request.second_spacing.abs(), "second pattern spacing")?;
                    validate_vector(
                        request.second_direction.ok_or_else(|| {
                            SolidError::InvalidAxis(
                                "second pattern direction is required".to_string(),
                            )
                        })?,
                        "second pattern direction",
                    )?;
                }
                let copies = (request.count as usize * second_count as usize)
                    .saturating_sub(1)
                    .saturating_mul(request.body_ids.len());
                let new_body_ids = reuse_or_allocate(copies);
                BodyFeatureDefinitionDto::RectangularPattern {
                    feature_id,
                    name,
                    body_ids: request.body_ids,
                    direction: request.direction,
                    spacing: request.spacing,
                    count: request.count,
                    second_direction: request.second_direction,
                    second_spacing: request.second_spacing,
                    second_count,
                    new_body_ids,
                }
            }
            BodyFeatureRequestDto::CircularPattern(request) => {
                if request.body_ids.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                validate_pattern_count(request.count, "circular")?;
                validate_vector(request.axis_direction, "pattern axis")?;
                if !request.total_angle_deg.is_finite()
                    || request.total_angle_deg.abs() <= EPS
                    || request.total_angle_deg.abs() > 360.0 + EPS
                {
                    return Err(SolidError::InvalidAngle);
                }
                let copies = (request.count as usize - 1) * request.body_ids.len();
                let new_body_ids = reuse_or_allocate(copies);
                BodyFeatureDefinitionDto::CircularPattern {
                    feature_id,
                    name,
                    body_ids: request.body_ids,
                    axis_origin: request.axis_origin,
                    axis_direction: request.axis_direction,
                    count: request.count,
                    total_angle_deg: request.total_angle_deg,
                    new_body_ids,
                }
            }
            BodyFeatureRequestDto::Combine(request) => {
                if request.tool_body_ids.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                if request
                    .tool_body_ids
                    .iter()
                    .any(|body_id| *body_id == request.target_body_id)
                {
                    return Err(SolidError::InvalidHistory(
                        "Combine target cannot also be a tool body".to_string(),
                    ));
                }
                BodyFeatureDefinitionDto::Combine {
                    feature_id,
                    name,
                    target_body_id: request.target_body_id,
                    tool_body_ids: request.tool_body_ids,
                    operation: request.operation,
                    keep_tools: request.keep_tools,
                }
            }
            BodyFeatureRequestDto::SplitBody(request) => {
                let plane_basis = request.plane_basis.ok_or_else(|| {
                    SolidError::InvalidAxis("split plane could not be resolved".to_string())
                })?;
                let new_body_id = reuse_or_allocate(1)[0];
                BodyFeatureDefinitionDto::SplitBody {
                    feature_id,
                    name,
                    body_id: request.body_id,
                    plane: request.plane,
                    plane_basis,
                    new_body_id,
                }
            }
            BodyFeatureRequestDto::ImportStep(request) => {
                let file_name = request.file_name.trim();
                if file_name.is_empty()
                    || !(file_name.to_ascii_lowercase().ends_with(".step")
                        || file_name.to_ascii_lowercase().ends_with(".stp"))
                {
                    return Err(SolidError::InvalidHistory(
                        "STEP import needs a .step or .stp file name".to_string(),
                    ));
                }
                if request.data_base64.is_empty()
                    || request.data_base64.len() > MAX_STEP_BASE64_LENGTH
                    || request.data_base64.len() % 4 != 0
                    || !request.data_base64.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/' || byte == b'='
                    })
                {
                    return Err(SolidError::InvalidHistory(
                        "STEP import data is empty, too large, or not valid base64".to_string(),
                    ));
                }
                let body_id = reuse_or_allocate(1)[0];
                BodyFeatureDefinitionDto::ImportStep {
                    feature_id,
                    name,
                    file_name: file_name.to_string(),
                    data_base64: request.data_base64,
                    body_id,
                }
            }
        })
    }

    pub fn prepare_recompute(
        &mut self,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        self.prepare(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            catalog,
            active_features,
        )
    }

    /// Recompute every valid feature while retaining a timeline error for
    /// each feature whose references no longer resolve. This is used by
    /// history navigation and deletion: one broken branch must not prevent
    /// independent later work from rebuilding.
    pub fn prepare_recompute_resilient(
        &mut self,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        self.prepare_with_body_features_resilient(
            self.extrudes.clone(),
            self.revolves.clone(),
            self.sweeps.clone(),
            self.lofts.clone(),
            self.ribs.clone(),
            self.fillets.clone(),
            self.chamfers.clone(),
            self.holes.clone(),
            self.body_features.clone(),
            catalog,
            active_features,
        )
    }

    /// Remove the persistent definition owned by one timeline feature, then
    /// recompute the surviving graph. Definitions that depended on the
    /// removed feature remain in history and receive planning errors.
    pub fn prepare_delete_feature(
        &mut self,
        feature_id: FeatureId,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        self.ensure_idle()?;
        let mut extrudes = self.extrudes.clone();
        let mut revolves = self.revolves.clone();
        let mut sweeps = self.sweeps.clone();
        let mut lofts = self.lofts.clone();
        let mut ribs = self.ribs.clone();
        let mut fillets = self.fillets.clone();
        let mut chamfers = self.chamfers.clone();
        let mut holes = self.holes.clone();
        let mut body_features = self.body_features.clone();
        extrudes.retain(|definition| definition.feature_id != feature_id);
        revolves.retain(|definition| definition.feature_id != feature_id);
        sweeps.retain(|definition| definition.feature_id != feature_id);
        lofts.retain(|definition| definition.feature_id != feature_id);
        ribs.retain(|definition| definition.feature_id != feature_id);
        fillets.retain(|definition| definition.feature_id != feature_id);
        chamfers.retain(|definition| definition.feature_id != feature_id);
        holes.retain(|definition| definition.feature_id != feature_id);
        body_features.retain(|definition| definition.feature_id() != feature_id);
        self.prepare_with_body_features_resilient(
            extrudes,
            revolves,
            sweeps,
            lofts,
            ribs,
            fillets,
            chamfers,
            holes,
            body_features,
            catalog,
            active_features,
        )
    }

    pub fn cancel_pending(&mut self, transaction_id: u64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.transaction_id == transaction_id)
        {
            self.pending = None;
        }
    }

    pub fn commit(
        &mut self,
        transaction_id: u64,
        kernel: KernelSceneDto,
    ) -> Result<&SolidSceneDto, SolidError> {
        let pending = self
            .pending
            .take()
            .ok_or(SolidError::NoPendingTransaction)?;
        if pending.transaction_id != transaction_id {
            self.pending = Some(pending);
            return Err(SolidError::WrongTransaction);
        }

        let owners = body_owners(
            &pending.extrudes,
            &pending.revolves,
            &pending.sweeps,
            &pending.lofts,
            &pending.ribs,
            &pending.fillets,
            &pending.chamfers,
            &pending.holes,
            &pending.body_features,
            &pending.feature_order,
        );
        let mut seen = BTreeSet::new();
        let mut bodies = Vec::with_capacity(kernel.bodies.len());
        for raw in kernel.bodies {
            if !seen.insert(raw.body_id) {
                return Err(SolidError::KernelContract(format!(
                    "duplicate body id {}",
                    raw.body_id.0
                )));
            }
            if raw.positions.len() % 3 != 0 || raw.normals.len() != raw.positions.len() {
                return Err(SolidError::KernelContract(format!(
                    "body {} has malformed vertex buffers",
                    raw.body_id.0
                )));
            }
            if raw
                .indices
                .iter()
                .any(|index| (*index as usize) * 3 >= raw.positions.len())
            {
                return Err(SolidError::KernelContract(format!(
                    "body {} has an out-of-range mesh index",
                    raw.body_id.0
                )));
            }
            let feature_id = owners.get(&raw.body_id).copied().ok_or_else(|| {
                SolidError::KernelContract(format!("unknown body id {}", raw.body_id.0))
            })?;
            let name = format!("Body{}", raw.body_id.0);
            let faces = raw
                .faces
                .into_iter()
                .map(|face| FaceDto {
                    id: stable::face_id(raw.body_id, &face.key),
                    key: face.key,
                    first_index: face.first_index,
                    index_count: face.index_count,
                    plane: face.plane,
                    signature: face.signature,
                    cylinder: face.cylinder,
                })
                .collect();
            let edges = raw
                .edges
                .into_iter()
                .map(|edge| EdgeDto {
                    id: stable::edge_id(raw.body_id, &edge.key),
                    key: edge.key,
                    points: edge.points,
                    circle: edge.circle,
                    refinable: edge.refinable,
                })
                .collect();
            bodies.push(BodyDto {
                id: raw.body_id,
                topology_signature: raw.topology_signature,
                name,
                feature_id,
                mesh: MeshDto {
                    positions: raw.positions,
                    normals: raw.normals,
                    indices: raw.indices,
                },
                faces,
                edges,
            });
        }
        bodies.sort_by_key(|body| body.id);
        self.extrudes = pending.extrudes;
        self.revolves = pending.revolves;
        self.sweeps = pending.sweeps;
        self.lofts = pending.lofts;
        self.ribs = pending.ribs;
        self.fillets = pending.fillets;
        self.chamfers = pending.chamfers;
        self.holes = pending.holes;
        self.body_features = pending.body_features;
        self.feature_order = pending.feature_order;
        self.scene = SolidSceneDto {
            bodies,
            errors: kernel.errors,
        };
        Ok(&self.scene)
    }

    fn ensure_idle(&self) -> Result<(), SolidError> {
        if self.pending.is_some() {
            Err(SolidError::PendingTransaction)
        } else {
            Ok(())
        }
    }

    fn alloc_body_id(&mut self) -> BodyId {
        let id = BodyId(self.next_body_id);
        self.next_body_id += 1;
        id
    }

    fn prepare(
        &mut self,
        extrudes: Vec<ExtrudeDefinitionDto>,
        revolves: Vec<RevolveDefinitionDto>,
        sweeps: Vec<SweepDefinitionDto>,
        lofts: Vec<LoftDefinitionDto>,
        ribs: Vec<RibDefinitionDto>,
        fillets: Vec<SolidFilletDefinitionDto>,
        chamfers: Vec<SolidChamferDefinitionDto>,
        holes: Vec<HoleDefinitionDto>,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        let body_features = self.body_features.clone();
        self.prepare_with_body_features(
            extrudes,
            revolves,
            sweeps,
            lofts,
            ribs,
            fillets,
            chamfers,
            holes,
            body_features,
            catalog,
            active_features,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_with_body_features_resilient(
        &mut self,
        mut extrudes: Vec<ExtrudeDefinitionDto>,
        revolves: Vec<RevolveDefinitionDto>,
        sweeps: Vec<SweepDefinitionDto>,
        lofts: Vec<LoftDefinitionDto>,
        mut ribs: Vec<RibDefinitionDto>,
        mut fillets: Vec<SolidFilletDefinitionDto>,
        mut chamfers: Vec<SolidChamferDefinitionDto>,
        holes: Vec<HoleDefinitionDto>,
        mut body_features: Vec<BodyFeatureDefinitionDto>,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        let transaction_id = self.next_transaction_id;
        self.next_transaction_id += 1;
        refresh_extent_references(&mut extrudes, &self.scene);
        refresh_rib_extent_references(&mut ribs, &self.scene);
        refresh_refinement_references(&mut fillets, &mut chamfers, &self.scene);
        refresh_body_feature_references(&mut body_features, &self.scene);

        let mut solid_feature_ids = extrudes
            .iter()
            .map(|definition| definition.feature_id)
            .chain(revolves.iter().map(|definition| definition.feature_id))
            .chain(sweeps.iter().map(|definition| definition.feature_id))
            .chain(lofts.iter().map(|definition| definition.feature_id))
            .chain(ribs.iter().map(|definition| definition.feature_id))
            .chain(fillets.iter().map(|definition| definition.feature_id))
            .chain(chamfers.iter().map(|definition| definition.feature_id))
            .chain(holes.iter().map(|definition| definition.feature_id))
            .chain(
                body_features
                    .iter()
                    .map(BodyFeatureDefinitionDto::feature_id),
            )
            .collect::<Vec<_>>();
        solid_feature_ids
            .sort_by_key(|feature_id| feature_order_key(&self.feature_order, *feature_id));
        solid_feature_ids.dedup();

        // Sketches and datum planes remain active inputs. Solid features are
        // admitted one at a time in timeline order; a feature that cannot be
        // planned is skipped and reported without blocking independent work.
        let solid_feature_set = solid_feature_ids.iter().copied().collect::<BTreeSet<_>>();
        let mut viable_active = active_features
            .difference(&solid_feature_set)
            .copied()
            .collect::<BTreeSet<_>>();
        let mut jobs = Vec::new();
        let mut errors = Vec::new();
        for feature_id in solid_feature_ids {
            if !active_features.contains(&feature_id) {
                continue;
            }
            viable_active.insert(feature_id);
            match make_jobs(
                &extrudes,
                &revolves,
                &sweeps,
                &lofts,
                &ribs,
                &fillets,
                &chamfers,
                &holes,
                &body_features,
                catalog,
                &viable_active,
                &self.scene,
                &self.feature_order,
            ) {
                Ok(next_jobs) => jobs = next_jobs,
                Err(error) => {
                    viable_active.remove(&feature_id);
                    errors.push(KernelFeatureErrorDto {
                        feature_id,
                        message: format!("Broken history reference: {error}"),
                    });
                }
            }
        }

        self.pending = Some(Pending {
            transaction_id,
            extrudes,
            revolves,
            sweeps,
            lofts,
            ribs,
            fillets,
            chamfers,
            holes,
            body_features,
            feature_order: self.feature_order.clone(),
        });
        Ok(RecomputePlanDto {
            transaction_id,
            jobs,
            errors,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_with_body_features(
        &mut self,
        mut extrudes: Vec<ExtrudeDefinitionDto>,
        revolves: Vec<RevolveDefinitionDto>,
        sweeps: Vec<SweepDefinitionDto>,
        lofts: Vec<LoftDefinitionDto>,
        mut ribs: Vec<RibDefinitionDto>,
        mut fillets: Vec<SolidFilletDefinitionDto>,
        mut chamfers: Vec<SolidChamferDefinitionDto>,
        holes: Vec<HoleDefinitionDto>,
        mut body_features: Vec<BodyFeatureDefinitionDto>,
        catalog: &[ProfileCatalogItemDto],
        active_features: &BTreeSet<FeatureId>,
    ) -> Result<RecomputePlanDto, SolidError> {
        let transaction_id = self.next_transaction_id;
        self.next_transaction_id += 1;
        refresh_extent_references(&mut extrudes, &self.scene);
        refresh_rib_extent_references(&mut ribs, &self.scene);
        refresh_refinement_references(&mut fillets, &mut chamfers, &self.scene);
        refresh_body_feature_references(&mut body_features, &self.scene);
        let jobs = make_jobs(
            &extrudes,
            &revolves,
            &sweeps,
            &lofts,
            &ribs,
            &fillets,
            &chamfers,
            &holes,
            &body_features,
            catalog,
            active_features,
            &self.scene,
            &self.feature_order,
        )?;
        self.pending = Some(Pending {
            transaction_id,
            extrudes,
            revolves,
            sweeps,
            lofts,
            ribs,
            fillets,
            chamfers,
            holes,
            body_features,
            feature_order: self.feature_order.clone(),
        });
        Ok(RecomputePlanDto {
            transaction_id,
            jobs,
            errors: Vec::new(),
        })
    }
}

fn body_owners(
    extrudes: &[ExtrudeDefinitionDto],
    revolves: &[RevolveDefinitionDto],
    sweeps: &[SweepDefinitionDto],
    lofts: &[LoftDefinitionDto],
    ribs: &[RibDefinitionDto],
    fillets: &[SolidFilletDefinitionDto],
    chamfers: &[SolidChamferDefinitionDto],
    holes: &[HoleDefinitionDto],
    body_features: &[BodyFeatureDefinitionDto],
    feature_order: &BTreeMap<FeatureId, usize>,
) -> BTreeMap<BodyId, FeatureId> {
    let mut owners = BTreeMap::new();
    let mut features = extrudes
        .iter()
        .map(FeatureDefinitionRef::Extrude)
        .chain(revolves.iter().map(FeatureDefinitionRef::Revolve))
        .chain(sweeps.iter().map(FeatureDefinitionRef::Sweep))
        .chain(lofts.iter().map(FeatureDefinitionRef::Loft))
        .chain(ribs.iter().map(FeatureDefinitionRef::Rib))
        .chain(fillets.iter().map(FeatureDefinitionRef::Fillet))
        .chain(chamfers.iter().map(FeatureDefinitionRef::Chamfer))
        .chain(holes.iter().map(FeatureDefinitionRef::Hole))
        .chain(body_features.iter().map(FeatureDefinitionRef::BodyFeature))
        .collect::<Vec<_>>();
    features.sort_by_key(|feature| feature_order_key(feature_order, feature.feature_id()));
    for feature in features {
        match feature {
            FeatureDefinitionRef::Extrude(definition) => assign_owners(
                &mut owners,
                definition.feature_id,
                definition.operation,
                &definition.new_body_ids[..definition
                    .source_face
                    .map(|_| 1)
                    .unwrap_or(definition.profile_indices.len())
                    .min(definition.new_body_ids.len())],
                &definition.target_body_ids,
            ),
            FeatureDefinitionRef::Revolve(definition) => assign_owners(
                &mut owners,
                definition.feature_id,
                definition.operation,
                &definition.new_body_ids[..definition
                    .profile_indices
                    .len()
                    .min(definition.new_body_ids.len())],
                &definition.target_body_ids,
            ),
            FeatureDefinitionRef::Sweep(definition) => assign_owners(
                &mut owners,
                definition.feature_id,
                definition.operation,
                &[definition.new_body_id],
                &definition.target_body_ids,
            ),
            FeatureDefinitionRef::Loft(definition) => assign_owners(
                &mut owners,
                definition.feature_id,
                definition.operation,
                &[definition.new_body_id],
                &definition.target_body_ids,
            ),
            FeatureDefinitionRef::Rib(definition) => assign_owners(
                &mut owners,
                definition.feature_id,
                definition.operation,
                &definition.new_body_ids[..definition
                    .line_entity_ids
                    .len()
                    .min(definition.new_body_ids.len())],
                &definition.target_body_ids,
            ),
            FeatureDefinitionRef::Fillet(definition) => {
                owners
                    .entry(definition.body_id)
                    .or_insert(definition.feature_id);
            }
            FeatureDefinitionRef::Chamfer(definition) => {
                owners
                    .entry(definition.body_id)
                    .or_insert(definition.feature_id);
            }
            FeatureDefinitionRef::Hole(definition) => {
                owners
                    .entry(definition.body_id)
                    .or_insert(definition.feature_id);
            }
            FeatureDefinitionRef::BodyFeature(definition) => match definition {
                BodyFeatureDefinitionDto::ExternalThread {
                    feature_id,
                    body_id,
                    ..
                } => {
                    owners.entry(*body_id).or_insert(*feature_id);
                }
                BodyFeatureDefinitionDto::Shell {
                    feature_id,
                    body_id,
                    ..
                } => {
                    owners.entry(*body_id).or_insert(*feature_id);
                }
                BodyFeatureDefinitionDto::MoveCopy {
                    feature_id,
                    copy,
                    result_body_ids,
                    ..
                } => {
                    if *copy {
                        for body_id in result_body_ids {
                            owners.insert(*body_id, *feature_id);
                        }
                    }
                }
                BodyFeatureDefinitionDto::Mirror {
                    feature_id,
                    new_body_ids,
                    ..
                }
                | BodyFeatureDefinitionDto::RectangularPattern {
                    feature_id,
                    new_body_ids,
                    ..
                }
                | BodyFeatureDefinitionDto::CircularPattern {
                    feature_id,
                    new_body_ids,
                    ..
                } => {
                    for body_id in new_body_ids {
                        owners.insert(*body_id, *feature_id);
                    }
                }
                BodyFeatureDefinitionDto::Combine {
                    feature_id,
                    target_body_id,
                    ..
                } => {
                    owners.entry(*target_body_id).or_insert(*feature_id);
                }
                BodyFeatureDefinitionDto::SplitBody {
                    feature_id,
                    body_id,
                    new_body_id,
                    ..
                } => {
                    owners.entry(*body_id).or_insert(*feature_id);
                    owners.insert(*new_body_id, *feature_id);
                }
                BodyFeatureDefinitionDto::ImportStep {
                    feature_id,
                    body_id,
                    ..
                } => {
                    owners.insert(*body_id, *feature_id);
                }
            },
        }
    }
    owners
}

fn assign_owners(
    owners: &mut BTreeMap<BodyId, FeatureId>,
    feature_id: FeatureId,
    operation: ExtrudeOperation,
    new_body_ids: &[BodyId],
    target_body_ids: &[BodyId],
) {
    match operation {
        ExtrudeOperation::NewBody => {
            for body_id in new_body_ids {
                owners.insert(*body_id, feature_id);
            }
        }
        ExtrudeOperation::Join if target_body_ids.is_empty() => {
            if let Some(body_id) = new_body_ids.first() {
                owners.insert(*body_id, feature_id);
            }
        }
        ExtrudeOperation::Join | ExtrudeOperation::Cut | ExtrudeOperation::Intersect => {
            for body_id in target_body_ids {
                owners.entry(*body_id).or_insert(feature_id);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum FeatureDefinitionRef<'a> {
    Extrude(&'a ExtrudeDefinitionDto),
    Revolve(&'a RevolveDefinitionDto),
    Sweep(&'a SweepDefinitionDto),
    Loft(&'a LoftDefinitionDto),
    Rib(&'a RibDefinitionDto),
    Fillet(&'a SolidFilletDefinitionDto),
    Chamfer(&'a SolidChamferDefinitionDto),
    Hole(&'a HoleDefinitionDto),
    BodyFeature(&'a BodyFeatureDefinitionDto),
}

impl FeatureDefinitionRef<'_> {
    fn feature_id(&self) -> FeatureId {
        match self {
            Self::Extrude(definition) => definition.feature_id,
            Self::Revolve(definition) => definition.feature_id,
            Self::Sweep(definition) => definition.feature_id,
            Self::Loft(definition) => definition.feature_id,
            Self::Rib(definition) => definition.feature_id,
            Self::Fillet(definition) => definition.feature_id,
            Self::Chamfer(definition) => definition.feature_id,
            Self::Hole(definition) => definition.feature_id,
            Self::BodyFeature(definition) => definition.feature_id(),
        }
    }
}

fn feature_order_key(
    feature_order: &BTreeMap<FeatureId, usize>,
    feature_id: FeatureId,
) -> (usize, u64) {
    (
        feature_order
            .get(&feature_id)
            .copied()
            .unwrap_or(usize::MAX),
        feature_id.0,
    )
}

fn make_jobs(
    extrudes: &[ExtrudeDefinitionDto],
    revolves: &[RevolveDefinitionDto],
    sweeps: &[SweepDefinitionDto],
    lofts: &[LoftDefinitionDto],
    ribs: &[RibDefinitionDto],
    fillets: &[SolidFilletDefinitionDto],
    chamfers: &[SolidChamferDefinitionDto],
    holes: &[HoleDefinitionDto],
    body_features: &[BodyFeatureDefinitionDto],
    catalog: &[ProfileCatalogItemDto],
    active_features: &BTreeSet<FeatureId>,
    previous_scene: &SolidSceneDto,
    feature_order: &BTreeMap<FeatureId, usize>,
) -> Result<Vec<KernelJobDto>, SolidError> {
    let mut jobs = Vec::new();
    let mut available_bodies = BTreeSet::new();
    let mut features = extrudes
        .iter()
        .map(FeatureDefinitionRef::Extrude)
        .chain(revolves.iter().map(FeatureDefinitionRef::Revolve))
        .chain(sweeps.iter().map(FeatureDefinitionRef::Sweep))
        .chain(lofts.iter().map(FeatureDefinitionRef::Loft))
        .chain(ribs.iter().map(FeatureDefinitionRef::Rib))
        .chain(fillets.iter().map(FeatureDefinitionRef::Fillet))
        .chain(chamfers.iter().map(FeatureDefinitionRef::Chamfer))
        .chain(holes.iter().map(FeatureDefinitionRef::Hole))
        .chain(body_features.iter().map(FeatureDefinitionRef::BodyFeature))
        .collect::<Vec<_>>();
    features.sort_by_key(|feature| feature_order_key(feature_order, feature.feature_id()));

    for feature in features {
        if !active_features.contains(&feature.feature_id()) {
            continue;
        }
        match feature {
            FeatureDefinitionRef::Extrude(definition) => {
                if definition.taper_angle_deg.abs() >= 89.0
                    || !definition.taper_angle_deg.is_finite()
                {
                    return Err(SolidError::InvalidTaper);
                }
                let (profiles, source_face, source_basis) = if let Some(source) =
                    definition.source_face
                {
                    if !definition.profile_indices.is_empty() {
                        return Err(SolidError::InvalidHistory(format!(
                            "{} mixes a planar face with sketch profiles",
                            definition.name
                        )));
                    }
                    if !available_bodies.contains(&source.body_id) {
                        return Err(SolidError::MissingTarget(source.body_id));
                    }
                    let face_key = definition.source_face_key.clone().ok_or_else(|| {
                        SolidError::InvalidHistory(format!(
                            "{} has no saved OCCT face key",
                            definition.name
                        ))
                    })?;
                    let signature = definition.source_face_signature.ok_or_else(|| {
                        SolidError::InvalidHistory(format!(
                            "{} has an unvalidated legacy face reference; reselect its Extrude source",
                            definition.name
                        ))
                    })?;
                    let basis = definition
                        .source_face_basis
                        .or_else(|| {
                            support_face_basis(previous_scene, source.body_id, source.face_id).ok()
                        })
                        .ok_or(SolidError::MissingFace(source.face_id))?;
                    (
                        Vec::new(),
                        Some(KernelPlanarFaceSourceDto {
                            body_id: source.body_id,
                            face_id: source.face_id,
                            face_key,
                            signature,
                        }),
                        basis,
                    )
                } else {
                    let sketch = find_input_sketch(
                        catalog,
                        active_features,
                        &definition.sketch_name,
                        definition.feature_id,
                        feature_order,
                    )?;
                    if definition.profile_indices.is_empty() {
                        return Err(SolidError::EmptySelection);
                    }
                    (
                        kernel_profiles(sketch, &definition.profile_indices, |_| Ok(()))?,
                        None,
                        sketch.basis,
                    )
                };
                let source_count = if source_face.is_some() {
                    1
                } else {
                    profiles.len()
                };
                let (mut start_offset, mut end_offset) = offsets(
                    definition.extent,
                    source_basis,
                    previous_scene,
                    definition.to_face_basis,
                )?;
                if definition.flip {
                    (start_offset, end_offset) = (-end_offset, -start_offset);
                }
                if (end_offset - start_offset).abs() <= EPS {
                    return Err(SolidError::InvalidExtent(
                        "extrude extent must have non-zero depth".to_string(),
                    ));
                }

                let result_body_ids = match definition.operation {
                    ExtrudeOperation::NewBody => definition
                        .new_body_ids
                        .iter()
                        .copied()
                        .take(source_count)
                        .collect::<Vec<_>>(),
                    ExtrudeOperation::Join if definition.target_body_ids.is_empty() => {
                        if source_count < 2 {
                            return Err(SolidError::InvalidExtent(
                                "Join without a target needs at least two profiles".to_string(),
                            ));
                        }
                        vec![*definition.new_body_ids.first().ok_or_else(|| {
                            SolidError::InvalidHistory(format!(
                                "{} has no reserved body id",
                                definition.name
                            ))
                        })?]
                    }
                    ExtrudeOperation::Join
                    | ExtrudeOperation::Cut
                    | ExtrudeOperation::Intersect => {
                        if definition.target_body_ids.is_empty() {
                            return Err(SolidError::InvalidExtent(
                                "a boolean extrude needs at least one target body".to_string(),
                            ));
                        }
                        for target in &definition.target_body_ids {
                            if !available_bodies.contains(target) {
                                return Err(SolidError::MissingTarget(*target));
                            }
                        }
                        definition.target_body_ids.clone()
                    }
                };
                if result_body_ids.len() != source_count
                    && definition.operation == ExtrudeOperation::NewBody
                {
                    return Err(SolidError::InvalidHistory(format!(
                        "{} has too few reserved body ids",
                        definition.name
                    )));
                }
                match definition.operation {
                    ExtrudeOperation::NewBody | ExtrudeOperation::Join => {
                        available_bodies.extend(result_body_ids.iter().copied())
                    }
                    ExtrudeOperation::Cut | ExtrudeOperation::Intersect => {}
                }
                jobs.push(KernelJobDto::Extrude(KernelExtrudeJobDto {
                    feature_id: definition.feature_id,
                    operation: definition.operation,
                    source_face,
                    profiles,
                    normal: source_basis.normal.into(),
                    start_offset,
                    end_offset,
                    taper_angle_deg: definition.taper_angle_deg,
                    target_body_ids: definition.target_body_ids.clone(),
                    result_body_ids,
                }));
            }
            FeatureDefinitionRef::Revolve(definition) => {
                let sketch = find_input_sketch(
                    catalog,
                    active_features,
                    &definition.sketch_name,
                    definition.feature_id,
                    feature_order,
                )?;
                if definition.profile_indices.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                let (axis_origin_2d, axis_direction_2d) =
                    if let Some(entity_id) = definition.axis_line_entity_id {
                        let axis_sketch_name = definition
                            .axis_line_sketch_name
                            .as_deref()
                            .filter(|name| !name.is_empty())
                            .unwrap_or(&definition.sketch_name);
                        let axis_sketch = if axis_sketch_name == definition.sketch_name {
                            sketch
                        } else {
                            find_input_sketch(
                                catalog,
                                active_features,
                                axis_sketch_name,
                                definition.feature_id,
                                feature_order,
                            )?
                        };
                        if !plane_bases_coplanar(sketch.basis, axis_sketch.basis) {
                            return Err(SolidError::InvalidAxis(format!(
                                "Revolve axis sketch '{}' is not coplanar with profile sketch '{}'",
                                axis_sketch.sketch_name, sketch.sketch_name
                            )));
                        }
                        let line = find_line(axis_sketch, entity_id).ok_or_else(|| {
                            SolidError::InvalidAxis(format!(
                                "Revolve axis line {entity_id} no longer exists in '{}'",
                                axis_sketch.sketch_name
                            ))
                        })?;
                        let start_world = axis_sketch.basis.to_3d([line.start.x, line.start.y]);
                        let end_world = axis_sketch.basis.to_3d([line.end.x, line.end.y]);
                        let start = sketch.basis.to_2d(start_world);
                        let end = sketch.basis.to_2d(end_world);
                        (
                            Point2Dto::new(start[0], start[1]),
                            Point2Dto::new(end[0] - start[0], end[1] - start[1]),
                        )
                    } else {
                        (definition.axis_origin, definition.axis_direction)
                    };
                let axis_length = axis_direction_2d.x.hypot(axis_direction_2d.y);
                if !axis_length.is_finite() || axis_length <= EPS {
                    return Err(SolidError::InvalidAxis(
                        "revolve axis direction must be non-zero".to_string(),
                    ));
                }
                if !definition.angle_deg.is_finite()
                    || definition.angle_deg.abs() <= EPS
                    || definition.angle_deg.abs() > 360.0 + EPS
                {
                    return Err(SolidError::InvalidAngle);
                }
                let dx = axis_direction_2d.x / axis_length;
                let dy = axis_direction_2d.y / axis_length;
                let profiles = kernel_profiles(sketch, &definition.profile_indices, |profile| {
                    let mut has_positive = false;
                    let mut has_negative = false;
                    for point in &profile.points {
                        let signed =
                            dx * (point.y - axis_origin_2d.y) - dy * (point.x - axis_origin_2d.x);
                        has_positive |= signed > EPS;
                        has_negative |= signed < -EPS;
                    }
                    if has_positive && has_negative {
                        return Err(SolidError::InvalidAxis(format!(
                            "profile {} crosses the revolve axis",
                            profile.index
                        )));
                    }
                    Ok(())
                })?;
                let result_body_ids = resolve_outputs(
                    definition.operation,
                    &definition.new_body_ids,
                    profiles.len(),
                    &definition.target_body_ids,
                    &available_bodies,
                    &definition.name,
                )?;
                record_new_bodies(
                    definition.operation,
                    &result_body_ids,
                    &mut available_bodies,
                );
                let axis_origin = sketch.basis.to_3d([axis_origin_2d.x, axis_origin_2d.y]);
                let axis_direction = [
                    sketch.basis.u[0] * dx + sketch.basis.v[0] * dy,
                    sketch.basis.u[1] * dx + sketch.basis.v[1] * dy,
                    sketch.basis.u[2] * dx + sketch.basis.v[2] * dy,
                ];
                let angle_rad =
                    definition.angle_deg.to_radians() * if definition.flip { -1.0 } else { 1.0 };
                jobs.push(KernelJobDto::Revolve(KernelRevolveJobDto {
                    feature_id: definition.feature_id,
                    operation: definition.operation,
                    profiles,
                    axis_origin: axis_origin.into(),
                    axis_direction: axis_direction.into(),
                    angle_rad,
                    target_body_ids: definition.target_body_ids.clone(),
                    result_body_ids,
                }));
            }
            FeatureDefinitionRef::Sweep(definition) => {
                let profile_sketch = find_input_sketch(
                    catalog,
                    active_features,
                    &definition.profile.sketch_name,
                    definition.feature_id,
                    feature_order,
                )?;
                let profile =
                    kernel_profiles(profile_sketch, &[definition.profile.profile_index], |_| {
                        Ok(())
                    })?
                    .into_iter()
                    .next()
                    .ok_or(SolidError::EmptySelection)?;
                let path_sketch = find_input_sketch(
                    catalog,
                    active_features,
                    &definition.path_sketch_name,
                    definition.feature_id,
                    feature_order,
                )?;
                let path = ordered_path(path_sketch, &definition.path_entity_ids)?;
                let guide_rail = if let Some(reference) = &definition.guide_rail {
                    let sketch = find_input_sketch(
                        catalog,
                        active_features,
                        &reference.sketch_name,
                        definition.feature_id,
                        feature_order,
                    )?;
                    ordered_path(sketch, &reference.entity_ids)?
                } else {
                    Vec::new()
                };
                let result_body_ids = resolve_outputs(
                    definition.operation,
                    &[definition.new_body_id],
                    1,
                    &definition.target_body_ids,
                    &available_bodies,
                    &definition.name,
                )?;
                record_new_bodies(
                    definition.operation,
                    &result_body_ids,
                    &mut available_bodies,
                );
                jobs.push(KernelJobDto::Sweep(KernelSweepJobDto {
                    feature_id: definition.feature_id,
                    operation: definition.operation,
                    profile,
                    path,
                    guide_rail,
                    orientation: definition.orientation,
                    transition: definition.transition,
                    force_c1: definition.force_c1,
                    target_body_ids: definition.target_body_ids.clone(),
                    result_body_ids,
                }));
            }
            FeatureDefinitionRef::Loft(definition) => {
                if definition.sections.len() < 2 {
                    return Err(SolidError::InvalidPath(
                        "Loft needs at least two closed profile sections".to_string(),
                    ));
                }
                let mut sections = Vec::with_capacity(definition.sections.len());
                for section in &definition.sections {
                    let sketch = find_input_sketch(
                        catalog,
                        active_features,
                        &section.sketch_name,
                        definition.feature_id,
                        feature_order,
                    )?;
                    let profile = kernel_profiles(sketch, &[section.profile_index], |_| Ok(()))?
                        .into_iter()
                        .next()
                        .ok_or(SolidError::EmptySelection)?;
                    sections.push(profile);
                }
                let centerline = if let Some(reference) = &definition.centerline {
                    let sketch = find_input_sketch(
                        catalog,
                        active_features,
                        &reference.sketch_name,
                        definition.feature_id,
                        feature_order,
                    )?;
                    ordered_path(sketch, &reference.entity_ids)?
                } else {
                    Vec::new()
                };
                let guide_rail = if let Some(reference) = &definition.guide_rail {
                    let sketch = find_input_sketch(
                        catalog,
                        active_features,
                        &reference.sketch_name,
                        definition.feature_id,
                        feature_order,
                    )?;
                    ordered_path(sketch, &reference.entity_ids)?
                } else {
                    Vec::new()
                };
                let result_body_ids = resolve_outputs(
                    definition.operation,
                    &[definition.new_body_id],
                    1,
                    &definition.target_body_ids,
                    &available_bodies,
                    &definition.name,
                )?;
                record_new_bodies(
                    definition.operation,
                    &result_body_ids,
                    &mut available_bodies,
                );
                jobs.push(KernelJobDto::Loft(KernelLoftJobDto {
                    feature_id: definition.feature_id,
                    operation: definition.operation,
                    sections,
                    ruled: definition.ruled,
                    continuity: definition.continuity,
                    centerline,
                    guide_rail,
                    target_body_ids: definition.target_body_ids.clone(),
                    result_body_ids,
                }));
            }
            FeatureDefinitionRef::Rib(definition) => {
                if !definition.thickness.is_finite() || definition.thickness <= EPS {
                    return Err(SolidError::InvalidExtent(
                        "rib thickness must be greater than zero".to_string(),
                    ));
                }
                let sketch = find_input_sketch(
                    catalog,
                    active_features,
                    &definition.sketch_name,
                    definition.feature_id,
                    feature_order,
                )?;
                if definition.line_entity_ids.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                let profiles =
                    rib_profiles(sketch, &definition.line_entity_ids, definition.thickness)?;
                let (mut start_offset, mut end_offset) = match definition.extent {
                    None | Some(RibExtent::Distance { .. }) => {
                        let depth = match definition.extent {
                            Some(RibExtent::Distance { depth }) => depth,
                            _ => definition.depth,
                        };
                        if !depth.is_finite() || depth <= EPS {
                            return Err(SolidError::InvalidExtent(
                                "rib depth must be greater than zero".to_string(),
                            ));
                        }
                        if definition.symmetric {
                            (-depth * 0.5, depth * 0.5)
                        } else {
                            (0.0, depth)
                        }
                    }
                    Some(RibExtent::ThroughAll) => (-MAX_EXTENT_MM, MAX_EXTENT_MM),
                    Some(RibExtent::ToFace { face_id }) => offsets(
                        ExtrudeExtent::ToFace { face_id },
                        sketch.basis,
                        previous_scene,
                        definition.to_face_basis,
                    )?,
                    Some(RibExtent::ToNext) => (
                        0.0,
                        nearest_target_offset(
                            sketch.basis,
                            &definition.target_body_ids,
                            previous_scene,
                        )?,
                    ),
                };
                if definition.flip {
                    (start_offset, end_offset) = (-end_offset, -start_offset);
                }
                let result_body_ids = resolve_outputs(
                    definition.operation,
                    &definition.new_body_ids,
                    profiles.len(),
                    &definition.target_body_ids,
                    &available_bodies,
                    &definition.name,
                )?;
                record_new_bodies(
                    definition.operation,
                    &result_body_ids,
                    &mut available_bodies,
                );
                jobs.push(KernelJobDto::Rib(KernelRibJobDto {
                    feature_id: definition.feature_id,
                    operation: definition.operation,
                    profiles,
                    normal: sketch.basis.normal.into(),
                    start_offset,
                    end_offset,
                    target_body_ids: definition.target_body_ids.clone(),
                    result_body_ids,
                }));
            }
            FeatureDefinitionRef::Fillet(definition) => {
                validate_positive(definition.radius, "fillet radius")?;
                ensure_refinement_target(&available_bodies, definition.body_id)?;
                if definition.edge_keys.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                jobs.push(KernelJobDto::Fillet(KernelFilletJobDto {
                    feature_id: definition.feature_id,
                    target_body_id: definition.body_id,
                    edge_keys: definition.edge_keys.clone(),
                    radius: definition.radius,
                    tangent_chain: definition.tangent_chain,
                }));
            }
            FeatureDefinitionRef::Chamfer(definition) => {
                validate_positive(definition.distance, "chamfer distance")?;
                ensure_refinement_target(&available_bodies, definition.body_id)?;
                if definition.edge_keys.is_empty() {
                    return Err(SolidError::EmptySelection);
                }
                jobs.push(KernelJobDto::Chamfer(KernelChamferJobDto {
                    feature_id: definition.feature_id,
                    target_body_id: definition.body_id,
                    edge_keys: definition.edge_keys.clone(),
                    distance: definition.distance,
                    tangent_chain: definition.tangent_chain,
                }));
            }
            FeatureDefinitionRef::Hole(definition) => {
                ensure_refinement_target(&available_bodies, definition.body_id)?;
                validate_hole_definition(definition)?;
                // The selected solid face governs the cut direction even
                // when a center comes from a sketch on a parallel base or
                // offset plane. The cached basis also avoids retargeting a
                // pre-hole face id after the boolean changes topology.
                let mut basis = if let Some(cached) = definition.face_basis {
                    cached
                } else if previous_scene.bodies.is_empty() {
                    return Err(SolidError::MissingFace(definition.face_id));
                } else {
                    support_face_basis(previous_scene, definition.body_id, definition.face_id)?
                };
                // Recover legacy definitions whose cached face slot was
                // poisoned by post-boolean face-id reuse. Associative hole
                // sketches are expected to be coplanar or parallel to the
                // intended support; a perpendicular cached normal is not a
                // credible support and the sketch plane is the safer basis.
                if let Some(reference) = hole_positions(definition)
                    .iter()
                    .find_map(|position| position.position_reference.as_ref())
                {
                    let sketch = find_input_sketch(
                        catalog,
                        active_features,
                        &reference.sketch_name,
                        definition.feature_id,
                        feature_order,
                    )?;
                    if dot(basis.normal, sketch.basis.normal).abs() < 1.0 - 1e-6 {
                        basis = sketch.basis;
                    }
                }
                let sign = if definition.flip { 1.0 } else { -1.0 };
                for position in hole_positions(definition) {
                    let center = match &position.position_reference {
                        Some(reference) => {
                            find_input_sketch(
                                catalog,
                                active_features,
                                &reference.sketch_name,
                                definition.feature_id,
                                feature_order,
                            )?;
                            hole_reference_center(reference, catalog, active_features, basis)?
                        }
                        None => basis.to_3d([position.position.x, position.position.y]),
                    };
                    jobs.push(KernelJobDto::Hole(KernelHoleJobDto {
                        feature_id: definition.feature_id,
                        target_body_id: definition.body_id,
                        center: center.into(),
                        direction: Point3Dto::from([
                            basis.normal[0] * sign,
                            basis.normal[1] * sign,
                            basis.normal[2] * sign,
                        ]),
                        diameter: definition.diameter,
                        extent: definition.extent,
                        style: definition.style,
                        counterbore_diameter: definition.counterbore_diameter,
                        counterbore_depth: definition.counterbore_depth,
                        countersink_diameter: definition.countersink_diameter,
                        countersink_angle_deg: definition.countersink_angle_deg,
                        bottom_style: definition.bottom_style,
                        drill_point_angle_deg: definition.drill_point_angle_deg,
                        thread: definition.thread.clone(),
                    }));
                }
            }
            FeatureDefinitionRef::BodyFeature(definition) => match definition {
                BodyFeatureDefinitionDto::ExternalThread {
                    feature_id,
                    body_id,
                    face_key,
                    cylinder,
                    thread,
                    flip,
                    ..
                } => {
                    ensure_refinement_target(&available_bodies, *body_id)?;
                    validate_external_thread(thread, cylinder.radius * 2.0)?;
                    jobs.push(KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
                        feature_id: *feature_id,
                        target_body_id: *body_id,
                        face_key: face_key.clone(),
                        cylinder: *cylinder,
                        thread: thread.clone(),
                        flip: *flip,
                    }));
                }
                BodyFeatureDefinitionDto::Shell {
                    feature_id,
                    body_id,
                    face_keys,
                    thickness,
                    inward,
                    ..
                } => {
                    ensure_refinement_target(&available_bodies, *body_id)?;
                    if face_keys.is_empty() {
                        return Err(SolidError::EmptySelection);
                    }
                    validate_positive(*thickness, "shell thickness")?;
                    jobs.push(KernelJobDto::Shell(KernelShellJobDto {
                        feature_id: *feature_id,
                        target_body_id: *body_id,
                        face_keys: face_keys.clone(),
                        thickness: *thickness,
                        inward: *inward,
                    }));
                }
                BodyFeatureDefinitionDto::MoveCopy {
                    feature_id,
                    body_ids,
                    translation,
                    rotation,
                    pivot,
                    copy: _,
                    result_body_ids,
                    ..
                } => {
                    ensure_body_inputs(&available_bodies, body_ids)?;
                    if result_body_ids.len() != body_ids.len() {
                        return Err(SolidError::InvalidHistory(
                            "Move/Copy output count does not match its source bodies".to_string(),
                        ));
                    }
                    jobs.push(KernelJobDto::Transform(KernelTransformJobDto {
                        feature_id: *feature_id,
                        source_body_ids: body_ids.clone(),
                        transforms: vec![KernelTransformDto::Rigid {
                            translation: *translation,
                            rotation: *rotation,
                            pivot: *pivot,
                        }],
                        result_body_ids: result_body_ids.clone(),
                    }));
                    // Move overwrites each stable source id; Copy writes new ids.
                    available_bodies.extend(result_body_ids.iter().copied());
                }
                BodyFeatureDefinitionDto::Mirror {
                    feature_id,
                    body_ids,
                    plane_basis,
                    new_body_ids,
                    ..
                } => {
                    ensure_body_inputs(&available_bodies, body_ids)?;
                    if new_body_ids.len() != body_ids.len() {
                        return Err(SolidError::InvalidHistory(
                            "Mirror output count does not match its source bodies".to_string(),
                        ));
                    }
                    jobs.push(KernelJobDto::Transform(KernelTransformJobDto {
                        feature_id: *feature_id,
                        source_body_ids: body_ids.clone(),
                        transforms: vec![KernelTransformDto::Mirror {
                            origin: plane_basis.origin.into(),
                            normal: plane_basis.normal.into(),
                        }],
                        result_body_ids: new_body_ids.clone(),
                    }));
                    available_bodies.extend(new_body_ids.iter().copied());
                }
                BodyFeatureDefinitionDto::RectangularPattern {
                    feature_id,
                    body_ids,
                    direction,
                    spacing,
                    count,
                    second_direction,
                    second_spacing,
                    second_count,
                    new_body_ids,
                    ..
                } => {
                    ensure_body_inputs(&available_bodies, body_ids)?;
                    let first = unit_point(*direction, "pattern direction")?;
                    let second = if *second_count > 1 {
                        Some(unit_point(
                            second_direction.ok_or_else(|| {
                                SolidError::InvalidAxis(
                                    "second pattern direction is missing".to_string(),
                                )
                            })?,
                            "second pattern direction",
                        )?)
                    } else {
                        None
                    };
                    let mut transforms = Vec::new();
                    for second_index in 0..*second_count {
                        for first_index in 0..*count {
                            if first_index == 0 && second_index == 0 {
                                continue;
                            }
                            let second_vector = second.unwrap_or([0.0; 3]);
                            transforms.push(KernelTransformDto::Translate {
                                vector: Point3Dto::from([
                                    first[0] * *spacing * f64::from(first_index)
                                        + second_vector[0]
                                            * *second_spacing
                                            * f64::from(second_index),
                                    first[1] * *spacing * f64::from(first_index)
                                        + second_vector[1]
                                            * *second_spacing
                                            * f64::from(second_index),
                                    first[2] * *spacing * f64::from(first_index)
                                        + second_vector[2]
                                            * *second_spacing
                                            * f64::from(second_index),
                                ]),
                            });
                        }
                    }
                    if new_body_ids.len() != transforms.len() * body_ids.len() {
                        return Err(SolidError::InvalidHistory(
                            "Rectangular Pattern output count is invalid".to_string(),
                        ));
                    }
                    jobs.push(KernelJobDto::Transform(KernelTransformJobDto {
                        feature_id: *feature_id,
                        source_body_ids: body_ids.clone(),
                        transforms,
                        result_body_ids: new_body_ids.clone(),
                    }));
                    available_bodies.extend(new_body_ids.iter().copied());
                }
                BodyFeatureDefinitionDto::CircularPattern {
                    feature_id,
                    body_ids,
                    axis_origin,
                    axis_direction,
                    count,
                    total_angle_deg,
                    new_body_ids,
                    ..
                } => {
                    ensure_body_inputs(&available_bodies, body_ids)?;
                    let full_circle = (total_angle_deg.abs() - 360.0).abs() <= EPS;
                    let divisor = if full_circle {
                        f64::from(*count)
                    } else {
                        f64::from(count.saturating_sub(1))
                    };
                    let step = total_angle_deg.to_radians() / divisor;
                    let transforms = (1..*count)
                        .map(|index| KernelTransformDto::Rotate {
                            origin: *axis_origin,
                            axis: *axis_direction,
                            angle_rad: step * f64::from(index),
                        })
                        .collect::<Vec<_>>();
                    if new_body_ids.len() != transforms.len() * body_ids.len() {
                        return Err(SolidError::InvalidHistory(
                            "Circular Pattern output count is invalid".to_string(),
                        ));
                    }
                    jobs.push(KernelJobDto::Transform(KernelTransformJobDto {
                        feature_id: *feature_id,
                        source_body_ids: body_ids.clone(),
                        transforms,
                        result_body_ids: new_body_ids.clone(),
                    }));
                    available_bodies.extend(new_body_ids.iter().copied());
                }
                BodyFeatureDefinitionDto::Combine {
                    feature_id,
                    target_body_id,
                    tool_body_ids,
                    operation,
                    keep_tools,
                    ..
                } => {
                    ensure_refinement_target(&available_bodies, *target_body_id)?;
                    ensure_body_inputs(&available_bodies, tool_body_ids)?;
                    jobs.push(KernelJobDto::Combine(KernelCombineJobDto {
                        feature_id: *feature_id,
                        target_body_id: *target_body_id,
                        tool_body_ids: tool_body_ids.clone(),
                        operation: *operation,
                        keep_tools: *keep_tools,
                    }));
                    if !keep_tools {
                        for tool in tool_body_ids {
                            available_bodies.remove(tool);
                        }
                    }
                }
                BodyFeatureDefinitionDto::SplitBody {
                    feature_id,
                    body_id,
                    plane_basis,
                    new_body_id,
                    ..
                } => {
                    ensure_refinement_target(&available_bodies, *body_id)?;
                    jobs.push(KernelJobDto::SplitBody(KernelSplitBodyJobDto {
                        feature_id: *feature_id,
                        target_body_id: *body_id,
                        plane_origin: plane_basis.origin.into(),
                        plane_normal: plane_basis.normal.into(),
                        new_body_id: *new_body_id,
                    }));
                    available_bodies.insert(*new_body_id);
                }
                BodyFeatureDefinitionDto::ImportStep {
                    feature_id,
                    data_base64,
                    body_id,
                    ..
                } => {
                    jobs.push(KernelJobDto::ImportStep(KernelImportStepJobDto {
                        feature_id: *feature_id,
                        result_body_id: *body_id,
                        data_base64: data_base64.clone(),
                    }));
                    available_bodies.insert(*body_id);
                }
            },
        }
    }
    Ok(jobs)
}

fn resolve_outputs(
    operation: ExtrudeOperation,
    reserved_body_ids: &[BodyId],
    new_body_count: usize,
    target_body_ids: &[BodyId],
    available_bodies: &BTreeSet<BodyId>,
    feature_name: &str,
) -> Result<Vec<BodyId>, SolidError> {
    match operation {
        ExtrudeOperation::NewBody => {
            if reserved_body_ids.len() < new_body_count {
                return Err(SolidError::InvalidHistory(format!(
                    "{feature_name} has too few reserved body ids"
                )));
            }
            Ok(reserved_body_ids
                .iter()
                .copied()
                .take(new_body_count)
                .collect())
        }
        ExtrudeOperation::Join | ExtrudeOperation::Cut | ExtrudeOperation::Intersect => {
            if target_body_ids.is_empty() {
                return Err(SolidError::InvalidExtent(format!(
                    "{feature_name} needs at least one boolean target body"
                )));
            }
            for target in target_body_ids {
                if !available_bodies.contains(target) {
                    return Err(SolidError::MissingTarget(*target));
                }
            }
            Ok(target_body_ids.to_vec())
        }
    }
}

fn record_new_bodies(
    operation: ExtrudeOperation,
    result_body_ids: &[BodyId],
    available_bodies: &mut BTreeSet<BodyId>,
) {
    if operation == ExtrudeOperation::NewBody {
        available_bodies.extend(result_body_ids.iter().copied());
    }
}

fn find_line(sketch: &ProfileCatalogItemDto, entity_id: u64) -> Option<&SketchLineDto> {
    sketch.lines.iter().find(|line| line.entity_id == entity_id)
}

fn near(a: Point2Dto, b: Point2Dto) -> bool {
    (a.x - b.x).hypot(a.y - b.y) <= 1e-5
}

fn ordered_path(
    sketch: &ProfileCatalogItemDto,
    entity_ids: &[u64],
) -> Result<Vec<KernelCurveDto>, SolidError> {
    if entity_ids.is_empty() {
        return Err(SolidError::InvalidPath(
            "A path needs at least one sketch curve".to_string(),
        ));
    }
    let find_curve = |entity_id: u64| {
        sketch
            .path_curves
            .iter()
            .find(|curve| curve.entity_id() == entity_id)
            .cloned()
            .or_else(|| {
                find_line(sketch, entity_id).map(|line| SketchPathCurveDto::Line {
                    entity_id,
                    start: line.start,
                    end: line.end,
                })
            })
    };
    let first = find_curve(entity_ids[0]).ok_or_else(|| {
        SolidError::InvalidPath(format!(
            "Path curve {} no longer exists in '{}'",
            entity_ids[0], sketch.sketch_name
        ))
    })?;
    if matches!(first, SketchPathCurveDto::Circle { .. }) {
        if entity_ids.len() != 1 {
            return Err(SolidError::InvalidPath(
                "A closed circle path cannot be chained with another curve".to_string(),
            ));
        }
        return Ok(vec![sketch_curve_to_kernel(&first, sketch.basis)]);
    }
    let (_, mut tail) = path_endpoints(&first).ok_or_else(|| {
        SolidError::InvalidPath("The first path curve has no usable endpoints".to_string())
    })?;
    let mut ordered = vec![first];
    let mut used = BTreeSet::from([entity_ids[0]]);
    for entity_id in &entity_ids[1..] {
        if !used.insert(*entity_id) {
            return Err(SolidError::InvalidPath(format!(
                "Path curve {entity_id} is selected more than once"
            )));
        }
        let curve = find_curve(*entity_id).ok_or_else(|| {
            SolidError::InvalidPath(format!(
                "Path curve {entity_id} no longer exists in '{}'",
                sketch.sketch_name
            ))
        })?;
        let (start, end) = path_endpoints(&curve).ok_or_else(|| {
            SolidError::InvalidPath(
                "A closed circle path cannot be part of a curve chain".to_string(),
            )
        })?;
        if near(tail, start) {
            tail = end;
            ordered.push(curve);
        } else if near(tail, end) {
            tail = start;
            ordered.push(reverse_path_curve(curve));
        } else {
            return Err(SolidError::InvalidPath(format!(
                "Path curve {entity_id} is not connected to the previous curve"
            )));
        }
    }
    Ok(ordered
        .iter()
        .map(|curve| sketch_curve_to_kernel(curve, sketch.basis))
        .collect())
}

fn path_endpoints(curve: &SketchPathCurveDto) -> Option<(Point2Dto, Point2Dto)> {
    match curve {
        SketchPathCurveDto::Line { start, end, .. }
        | SketchPathCurveDto::Arc { start, end, .. } => Some((*start, *end)),
        SketchPathCurveDto::Spline { points, .. } => Some((*points.first()?, *points.last()?)),
        SketchPathCurveDto::Circle { .. } => None,
    }
}

fn reverse_path_curve(curve: SketchPathCurveDto) -> SketchPathCurveDto {
    match curve {
        SketchPathCurveDto::Line {
            entity_id,
            start,
            end,
        } => SketchPathCurveDto::Line {
            entity_id,
            start: end,
            end: start,
        },
        SketchPathCurveDto::Arc {
            entity_id,
            start,
            mid,
            end,
        } => SketchPathCurveDto::Arc {
            entity_id,
            start: end,
            mid,
            end: start,
        },
        SketchPathCurveDto::Spline {
            entity_id,
            mut points,
        } => {
            points.reverse();
            SketchPathCurveDto::Spline { entity_id, points }
        }
        circle @ SketchPathCurveDto::Circle { .. } => circle,
    }
}

fn sketch_curve_to_kernel(curve: &SketchPathCurveDto, basis: PlaneBasis) -> KernelCurveDto {
    let world = |point: Point2Dto| Point3Dto::from(basis.to_3d([point.x, point.y]));
    match curve {
        SketchPathCurveDto::Line {
            entity_id,
            start,
            end,
        } => KernelCurveDto::Line {
            entity_id: *entity_id,
            start: world(*start),
            end: world(*end),
        },
        SketchPathCurveDto::Arc {
            entity_id,
            start,
            mid,
            end,
        } => KernelCurveDto::Arc {
            entity_id: *entity_id,
            start: world(*start),
            mid: world(*mid),
            end: world(*end),
        },
        SketchPathCurveDto::Circle {
            entity_id,
            center,
            radius,
        } => KernelCurveDto::Circle {
            entity_id: *entity_id,
            center: world(*center),
            axis_point: world(Point2Dto::new(center.x + radius, center.y)),
            normal: basis.normal.into(),
        },
        SketchPathCurveDto::Spline { entity_id, points } => KernelCurveDto::Polyline {
            entity_id: *entity_id,
            points: points.iter().copied().map(world).collect(),
        },
    }
}

fn rib_profiles(
    sketch: &ProfileCatalogItemDto,
    entity_ids: &[u64],
    thickness: f64,
) -> Result<Vec<KernelProfileDto>, SolidError> {
    let half = thickness * 0.5;
    entity_ids
        .iter()
        .enumerate()
        .map(|(profile_index, entity_id)| {
            let curve = sketch
                .path_curves
                .iter()
                .find(|curve| curve.entity_id() == *entity_id)
                .cloned()
                .or_else(|| {
                    find_line(sketch, *entity_id).map(|line| SketchPathCurveDto::Line {
                        entity_id: *entity_id,
                        start: line.start,
                        end: line.end,
                    })
                })
                .ok_or_else(|| {
                    SolidError::InvalidPath(format!(
                        "Rib centerline {entity_id} no longer exists in '{}'",
                        sketch.sketch_name
                    ))
                })?;
            rib_profile_for_curve(sketch, &curve, half, profile_index as u32, *entity_id)
        })
        .collect()
}

fn rib_profile_for_curve(
    sketch: &ProfileCatalogItemDto,
    curve: &SketchPathCurveDto,
    half: f64,
    profile_index: u32,
    entity_id: u64,
) -> Result<KernelProfileDto, SolidError> {
    match curve {
        SketchPathCurveDto::Line { start, end, .. } => {
            polyline_rib_profile(sketch, &[*start, *end], half, profile_index, entity_id)
        }
        SketchPathCurveDto::Spline { points, .. } => {
            polyline_rib_profile(sketch, points, half, profile_index, entity_id)
        }
        SketchPathCurveDto::Arc {
            start, mid, end, ..
        } => arc_rib_profile(sketch, *start, *mid, *end, half, profile_index, entity_id),
        SketchPathCurveDto::Circle { center, radius, .. } => {
            circle_rib_profile(sketch, *center, *radius, half, profile_index, entity_id)
        }
    }
}

fn polyline_rib_profile(
    sketch: &ProfileCatalogItemDto,
    points: &[Point2Dto],
    half: f64,
    profile_index: u32,
    entity_id: u64,
) -> Result<KernelProfileDto, SolidError> {
    if points.len() < 2 {
        return Err(SolidError::InvalidPath(format!(
            "Rib centerline {entity_id} needs at least two points"
        )));
    }
    let mut normals = Vec::with_capacity(points.len());
    for index in 0..points.len() {
        let direction = if index == 0 {
            [points[1].x - points[0].x, points[1].y - points[0].y]
        } else if index + 1 == points.len() {
            [
                points[index].x - points[index - 1].x,
                points[index].y - points[index - 1].y,
            ]
        } else {
            let before = [
                points[index].x - points[index - 1].x,
                points[index].y - points[index - 1].y,
            ];
            let after = [
                points[index + 1].x - points[index].x,
                points[index + 1].y - points[index].y,
            ];
            let before_length = before[0].hypot(before[1]);
            let after_length = after[0].hypot(after[1]);
            if before_length <= EPS || after_length <= EPS {
                return Err(SolidError::InvalidPath(format!(
                    "Rib centerline {entity_id} contains duplicate points"
                )));
            }
            [
                before[0] / before_length + after[0] / after_length,
                before[1] / before_length + after[1] / after_length,
            ]
        };
        let length = direction[0].hypot(direction[1]);
        if length <= EPS {
            return Err(SolidError::InvalidPath(format!(
                "Rib centerline {entity_id} reverses direction at a point"
            )));
        }
        normals.push([-direction[1] / length, direction[0] / length]);
    }
    let left = points
        .iter()
        .zip(&normals)
        .map(|(point, normal)| {
            Point2Dto::new(point.x + normal[0] * half, point.y + normal[1] * half)
        })
        .collect::<Vec<_>>();
    let right = points
        .iter()
        .zip(&normals)
        .map(|(point, normal)| {
            Point2Dto::new(point.x - normal[0] * half, point.y - normal[1] * half)
        })
        .collect::<Vec<_>>();
    let local = left
        .into_iter()
        .chain(right.into_iter().rev())
        .collect::<Vec<_>>();
    polygon_rib_profile(sketch, &local, profile_index, entity_id)
}

fn polygon_rib_profile(
    sketch: &ProfileCatalogItemDto,
    local: &[Point2Dto],
    profile_index: u32,
    entity_id: u64,
) -> Result<KernelProfileDto, SolidError> {
    if local.len() < 3 {
        return Err(SolidError::InvalidPath(
            "Rib profile collapsed to fewer than three points".to_string(),
        ));
    }
    let points = local
        .iter()
        .map(|point| Point3Dto::from(sketch.basis.to_3d([point.x, point.y])))
        .collect::<Vec<_>>();
    let curves = (0..points.len())
        .map(|index| KernelCurveDto::Line {
            entity_id,
            start: points[index],
            end: points[(index + 1) % points.len()],
        })
        .collect();
    Ok(KernelProfileDto {
        profile_index,
        points,
        curves,
        holes: Vec::new(),
    })
}

fn arc_rib_profile(
    sketch: &ProfileCatalogItemDto,
    start: Point2Dto,
    mid: Point2Dto,
    end: Point2Dto,
    half: f64,
    profile_index: u32,
    entity_id: u64,
) -> Result<KernelProfileDto, SolidError> {
    let denominator =
        2.0 * (start.x * (mid.y - end.y) + mid.x * (end.y - start.y) + end.x * (start.y - mid.y));
    if denominator.abs() <= EPS {
        return Err(SolidError::InvalidPath(format!(
            "Rib arc {entity_id} is degenerate"
        )));
    }
    let start_sq = start.x * start.x + start.y * start.y;
    let mid_sq = mid.x * mid.x + mid.y * mid.y;
    let end_sq = end.x * end.x + end.y * end.y;
    let center = Point2Dto::new(
        (start_sq * (mid.y - end.y) + mid_sq * (end.y - start.y) + end_sq * (start.y - mid.y))
            / denominator,
        (start_sq * (end.x - mid.x) + mid_sq * (start.x - end.x) + end_sq * (mid.x - start.x))
            / denominator,
    );
    let radius = (start.x - center.x).hypot(start.y - center.y);
    if radius <= half + EPS {
        return Err(SolidError::InvalidPath(format!(
            "Rib thickness is too large for arc {entity_id}"
        )));
    }
    let radial = |point: Point2Dto, target_radius: f64| {
        let dx = point.x - center.x;
        let dy = point.y - center.y;
        let length = dx.hypot(dy);
        Point2Dto::new(
            center.x + dx * target_radius / length,
            center.y + dy * target_radius / length,
        )
    };
    let outer = [
        radial(start, radius + half),
        radial(mid, radius + half),
        radial(end, radius + half),
    ];
    let inner = [
        radial(start, radius - half),
        radial(mid, radius - half),
        radial(end, radius - half),
    ];
    let world = |point: Point2Dto| Point3Dto::from(sketch.basis.to_3d([point.x, point.y]));
    let points = vec![
        world(outer[0]),
        world(outer[1]),
        world(outer[2]),
        world(inner[2]),
        world(inner[1]),
        world(inner[0]),
    ];
    let curves = vec![
        KernelCurveDto::Arc {
            entity_id,
            start: world(outer[0]),
            mid: world(outer[1]),
            end: world(outer[2]),
        },
        KernelCurveDto::Line {
            entity_id,
            start: world(outer[2]),
            end: world(inner[2]),
        },
        KernelCurveDto::Arc {
            entity_id,
            start: world(inner[2]),
            mid: world(inner[1]),
            end: world(inner[0]),
        },
        KernelCurveDto::Line {
            entity_id,
            start: world(inner[0]),
            end: world(outer[0]),
        },
    ];
    Ok(KernelProfileDto {
        profile_index,
        points,
        curves,
        holes: Vec::new(),
    })
}

fn circle_rib_profile(
    sketch: &ProfileCatalogItemDto,
    center: Point2Dto,
    radius: f64,
    half: f64,
    profile_index: u32,
    entity_id: u64,
) -> Result<KernelProfileDto, SolidError> {
    if !radius.is_finite() || radius <= half + EPS {
        return Err(SolidError::InvalidPath(format!(
            "Rib thickness is too large for circle {entity_id}"
        )));
    }
    let make_circle = |circle_radius: f64, index: u32| {
        let local_points = (0..8)
            .map(|step| {
                let angle = std::f64::consts::TAU * step as f64 / 8.0;
                Point2Dto::new(
                    center.x + circle_radius * angle.cos(),
                    center.y + circle_radius * angle.sin(),
                )
            })
            .collect::<Vec<_>>();
        let points = local_points
            .iter()
            .map(|point| Point3Dto::from(sketch.basis.to_3d([point.x, point.y])))
            .collect::<Vec<_>>();
        KernelProfileDto {
            profile_index: index,
            points,
            curves: vec![KernelCurveDto::Circle {
                entity_id,
                center: Point3Dto::from(sketch.basis.to_3d([center.x, center.y])),
                axis_point: Point3Dto::from(
                    sketch.basis.to_3d([center.x + circle_radius, center.y]),
                ),
                normal: sketch.basis.normal.into(),
            }],
            holes: Vec::new(),
        }
    };
    let mut outer = make_circle(radius + half, profile_index);
    outer.holes.push(make_circle(radius - half, 0));
    Ok(outer)
}

fn find_sketch<'a>(
    catalog: &'a [ProfileCatalogItemDto],
    active_features: &BTreeSet<FeatureId>,
    sketch_name: &str,
) -> Result<&'a ProfileCatalogItemDto, SolidError> {
    let sketch = catalog
        .iter()
        .find(|item| item.sketch_name == sketch_name)
        .ok_or_else(|| SolidError::SketchNotFound(sketch_name.to_string()))?;
    if !active_features.contains(&sketch.feature_id) {
        return Err(SolidError::SketchNotFound(sketch_name.to_string()));
    }
    Ok(sketch)
}

fn find_input_sketch<'a>(
    catalog: &'a [ProfileCatalogItemDto],
    active_features: &BTreeSet<FeatureId>,
    sketch_name: &str,
    consumer: FeatureId,
    feature_order: &BTreeMap<FeatureId, usize>,
) -> Result<&'a ProfileCatalogItemDto, SolidError> {
    let sketch = find_sketch(catalog, active_features, sketch_name)?;
    if feature_order_key(feature_order, sketch.feature_id)
        >= feature_order_key(feature_order, consumer)
    {
        return Err(SolidError::InvalidHistory(format!(
            "feature {} cannot use later sketch '{}'",
            consumer.0, sketch_name,
        )));
    }
    Ok(sketch)
}

fn kernel_profiles(
    sketch: &ProfileCatalogItemDto,
    indices: &[u32],
    mut validate: impl FnMut(&ProfileLoopDto) -> Result<(), SolidError>,
) -> Result<Vec<KernelProfileDto>, SolidError> {
    let mut selected = BTreeSet::new();
    let mut roots = Vec::new();
    for requested_index in indices {
        let mut profile = sketch
            .profiles
            .iter()
            .find(|profile| profile.index == *requested_index)
            .ok_or_else(|| SolidError::ProfileNotFound {
                sketch: sketch.sketch_name.clone(),
                index: *requested_index,
            })?;
        while profile.nesting_depth % 2 == 1 {
            let parent_index = profile.parent_index.ok_or_else(|| {
                SolidError::InvalidHistory(format!(
                    "profile {} in '{}' has an invalid nesting parent",
                    profile.index, sketch.sketch_name
                ))
            })?;
            profile = sketch
                .profiles
                .iter()
                .find(|candidate| candidate.index == parent_index)
                .ok_or_else(|| SolidError::ProfileNotFound {
                    sketch: sketch.sketch_name.clone(),
                    index: parent_index,
                })?;
        }
        if selected.insert(profile.index) {
            roots.push(profile);
        }
    }

    roots
        .into_iter()
        .map(|profile| {
            validate(profile)?;
            let holes = sketch
                .profiles
                .iter()
                .filter(|candidate| candidate.parent_index == Some(profile.index))
                .map(|hole| {
                    validate(hole)?;
                    Ok(kernel_profile(sketch, hole, Vec::new()))
                })
                .collect::<Result<Vec<_>, SolidError>>()?;
            Ok(kernel_profile(sketch, profile, holes))
        })
        .collect()
}

fn kernel_profile(
    sketch: &ProfileCatalogItemDto,
    profile: &ProfileLoopDto,
    holes: Vec<KernelProfileDto>,
) -> KernelProfileDto {
    KernelProfileDto {
        profile_index: profile.index,
        points: profile
            .points
            .iter()
            .map(|point| sketch.basis.to_3d([point.x, point.y]).into())
            .collect(),
        curves: profile
            .curves
            .iter()
            .map(|curve| kernel_curve(curve, sketch.basis))
            .collect(),
        holes,
    }
}

fn kernel_curve(curve: &ProfileCurveDto, basis: PlaneBasis) -> KernelCurveDto {
    let point = |value: Point2Dto| Point3Dto::from(basis.to_3d([value.x, value.y]));
    match curve {
        ProfileCurveDto::Line {
            entity_id,
            start,
            end,
            ..
        } => KernelCurveDto::Line {
            entity_id: *entity_id,
            start: point(*start),
            end: point(*end),
        },
        ProfileCurveDto::Arc {
            entity_id,
            start,
            mid,
            end,
            ..
        } => KernelCurveDto::Arc {
            entity_id: *entity_id,
            start: point(*start),
            mid: point(*mid),
            end: point(*end),
        },
        ProfileCurveDto::Circle {
            entity_id,
            center,
            radius,
            ..
        } => KernelCurveDto::Circle {
            entity_id: *entity_id,
            center: point(*center),
            axis_point: point(Point2Dto::new(center.x + radius, center.y)),
            normal: basis.normal.into(),
        },
        ProfileCurveDto::Polyline {
            entity_id, points, ..
        } => KernelCurveDto::Polyline {
            entity_id: *entity_id,
            points: points.iter().copied().map(point).collect(),
        },
    }
}

fn offsets(
    extent: ExtrudeExtent,
    sketch_basis: PlaneBasis,
    previous_scene: &SolidSceneDto,
    cached_to_face_basis: Option<PlaneBasis>,
) -> Result<(f64, f64), SolidError> {
    let finite_nonzero = |value: f64, label: &str| {
        if value.is_finite() && value.abs() > EPS {
            Ok(value)
        } else {
            Err(SolidError::InvalidExtent(format!(
                "{label} must be non-zero"
            )))
        }
    };
    let finite_positive = |value: f64, label: &str| {
        if value.is_finite() && value > EPS {
            Ok(value)
        } else {
            Err(SolidError::InvalidExtent(format!(
                "{label} must be greater than zero"
            )))
        }
    };
    match extent {
        ExtrudeExtent::Distance { distance } => Ok((0.0, finite_nonzero(distance, "distance")?)),
        ExtrudeExtent::TwoSides {
            distance,
            second_distance,
        } => Ok((
            -finite_positive(second_distance, "second distance")?,
            finite_positive(distance, "distance")?,
        )),
        ExtrudeExtent::Symmetric { distance } => {
            let half = finite_positive(distance, "distance")? * 0.5;
            Ok((-half, half))
        }
        ExtrudeExtent::ThroughAll => Ok((-MAX_EXTENT_MM, MAX_EXTENT_MM)),
        ExtrudeExtent::ToFace { face_id } => {
            let plane = face_basis(previous_scene, face_id)
                .or(cached_to_face_basis)
                .ok_or(SolidError::MissingFace(face_id))?;
            let alignment = dot(plane.normal, sketch_basis.normal).abs();
            if alignment < 1.0 - 1e-6 {
                return Err(SolidError::InvalidExtent(
                    "To Face currently requires a parallel planar face".to_string(),
                ));
            }
            let delta = [
                plane.origin[0] - sketch_basis.origin[0],
                plane.origin[1] - sketch_basis.origin[1],
                plane.origin[2] - sketch_basis.origin[2],
            ];
            let distance = dot(delta, sketch_basis.normal);
            if distance.abs() <= EPS {
                return Err(SolidError::InvalidExtent(
                    "target face lies on the sketch plane".to_string(),
                ));
            }
            Ok((0.0, distance))
        }
    }
}

fn face_basis(scene: &SolidSceneDto, face_id: FaceId) -> Option<PlaneBasis> {
    scene
        .bodies
        .iter()
        .flat_map(|body| &body.faces)
        .find(|face| face.id == face_id)
        .and_then(|face| face.plane)
}

fn extent_face_basis(extent: ExtrudeExtent, scene: &SolidSceneDto) -> Option<PlaneBasis> {
    match extent {
        ExtrudeExtent::ToFace { face_id } => face_basis(scene, face_id),
        _ => None,
    }
}

fn rib_extent_face_basis(extent: Option<RibExtent>, scene: &SolidSceneDto) -> Option<PlaneBasis> {
    match extent {
        Some(RibExtent::ToFace { face_id }) => face_basis(scene, face_id),
        _ => None,
    }
}

fn refresh_extent_references(definitions: &mut [ExtrudeDefinitionDto], scene: &SolidSceneDto) {
    for definition in definitions {
        if let ExtrudeExtent::ToFace { face_id } = definition.extent {
            if let Some(basis) = face_basis(scene, face_id) {
                definition.to_face_basis = Some(basis);
            }
        } else {
            definition.to_face_basis = None;
        }
    }
}

fn refresh_rib_extent_references(definitions: &mut [RibDefinitionDto], scene: &SolidSceneDto) {
    for definition in definitions {
        if let Some(RibExtent::ToFace { face_id }) = definition.extent {
            if let Some(basis) = face_basis(scene, face_id) {
                definition.to_face_basis = Some(basis);
            }
        } else {
            definition.to_face_basis = None;
        }
    }
}

fn validate_positive(value: f64, label: &str) -> Result<(), SolidError> {
    if value.is_finite() && value > EPS {
        Ok(())
    } else {
        Err(SolidError::InvalidExtent(format!(
            "{label} must be greater than zero"
        )))
    }
}

fn validate_pattern_count(count: u32, label: &str) -> Result<(), SolidError> {
    if (2..=10_000).contains(&count) {
        Ok(())
    } else {
        Err(SolidError::InvalidExtent(format!(
            "{label} pattern count must be between 2 and 10000"
        )))
    }
}

fn validate_vector(value: Point3Dto, label: &str) -> Result<(), SolidError> {
    unit_point(value, label).map(|_| ())
}

fn validate_vector_finite(value: Point3Dto, label: &str) -> Result<(), SolidError> {
    if value.x.is_finite() && value.y.is_finite() && value.z.is_finite() {
        Ok(())
    } else {
        Err(SolidError::InvalidAxis(format!("{label} must be finite")))
    }
}

fn normalized_quaternion(value: [f64; 4]) -> Result<[f64; 4], SolidError> {
    let length = value.iter().map(|entry| entry * entry).sum::<f64>().sqrt();
    if !length.is_finite() || length <= EPS {
        return Err(SolidError::InvalidAxis(
            "move rotation must be a finite, non-zero quaternion".to_string(),
        ));
    }
    Ok(value.map(|entry| entry / length))
}

fn unit_point(value: Point3Dto, label: &str) -> Result<[f64; 3], SolidError> {
    let length = (value.x * value.x + value.y * value.y + value.z * value.z).sqrt();
    if !length.is_finite() || length <= EPS {
        return Err(SolidError::InvalidAxis(format!(
            "{label} must be a non-zero vector"
        )));
    }
    Ok([value.x / length, value.y / length, value.z / length])
}

fn ensure_refinement_target(
    available_bodies: &BTreeSet<BodyId>,
    body_id: BodyId,
) -> Result<(), SolidError> {
    if available_bodies.contains(&body_id) {
        Ok(())
    } else {
        Err(SolidError::MissingTarget(body_id))
    }
}

fn ensure_body_inputs(
    available_bodies: &BTreeSet<BodyId>,
    body_ids: &[BodyId],
) -> Result<(), SolidError> {
    if body_ids.is_empty() {
        return Err(SolidError::EmptySelection);
    }
    for body_id in body_ids {
        ensure_refinement_target(available_bodies, *body_id)?;
    }
    Ok(())
}

fn edge_keys_for(
    scene: &SolidSceneDto,
    body_id: BodyId,
    edge_ids: &[EdgeId],
) -> Result<Vec<String>, SolidError> {
    if edge_ids.is_empty() {
        return Err(SolidError::EmptySelection);
    }
    let body = scene
        .bodies
        .iter()
        .find(|body| body.id == body_id)
        .ok_or(SolidError::MissingTarget(body_id))?;
    edge_ids
        .iter()
        .map(|edge_id| {
            body.edges
                .iter()
                .find(|edge| edge.id == *edge_id)
                .map(|edge| edge.key.clone())
                .ok_or(SolidError::MissingEdge(*edge_id))
        })
        .collect()
}

fn face_keys_for(
    scene: &SolidSceneDto,
    body_id: BodyId,
    face_ids: &[FaceId],
) -> Result<Vec<String>, SolidError> {
    if face_ids.is_empty() {
        return Err(SolidError::EmptySelection);
    }
    let body = scene
        .bodies
        .iter()
        .find(|body| body.id == body_id)
        .ok_or(SolidError::MissingTarget(body_id))?;
    face_ids
        .iter()
        .map(|face_id| {
            body.faces
                .iter()
                .find(|face| face.id == *face_id)
                .map(|face| face.key.clone())
                .ok_or(SolidError::MissingFace(*face_id))
        })
        .collect()
}

fn resolve_cylindrical_face_source(
    scene: &SolidSceneDto,
    body_id: BodyId,
    face_id: FaceId,
) -> Result<(String, CylindricalSurfaceDto), SolidError> {
    let body = scene
        .bodies
        .iter()
        .find(|body| body.id == body_id)
        .ok_or(SolidError::MissingTarget(body_id))?;
    let face = body
        .faces
        .iter()
        .find(|face| face.id == face_id)
        .ok_or(SolidError::MissingFace(face_id))?;
    let cylinder = face.cylinder.ok_or_else(|| {
        SolidError::InvalidExtent(
            "External Thread requires an analytic cylindrical face".to_string(),
        )
    })?;
    validate_vector(cylinder.axis, "thread cylinder axis")?;
    validate_vector(cylinder.reference, "thread cylinder reference")?;
    validate_positive(cylinder.radius, "thread cylinder radius")?;
    Ok((face.key.clone(), cylinder))
}

fn resolve_planar_face_source(
    scene: &SolidSceneDto,
    source: PlanarFaceSourceDto,
) -> Result<(String, PlaneBasis, PlanarFaceSignatureDto), SolidError> {
    let body = scene
        .bodies
        .iter()
        .find(|body| body.id == source.body_id)
        .ok_or(SolidError::MissingTarget(source.body_id))?;
    let face = body
        .faces
        .iter()
        .find(|face| face.id == source.face_id)
        .ok_or(SolidError::MissingFace(source.face_id))?;
    let basis = face.plane.ok_or_else(|| {
        SolidError::InvalidExtent("Extrude source face must be planar".to_string())
    })?;
    let signature = face.signature.ok_or_else(|| {
        SolidError::KernelContract("planar face is missing its OCCT signature".to_string())
    })?;
    Ok((face.key.clone(), basis, signature))
}

fn support_face_basis(
    scene: &SolidSceneDto,
    body_id: BodyId,
    face_id: FaceId,
) -> Result<PlaneBasis, SolidError> {
    let body = scene
        .bodies
        .iter()
        .find(|body| body.id == body_id)
        .ok_or(SolidError::MissingTarget(body_id))?;
    body.faces
        .iter()
        .find(|face| face.id == face_id)
        .ok_or(SolidError::MissingFace(face_id))?
        .plane
        .ok_or_else(|| SolidError::InvalidExtent("Hole support face must be planar".to_string()))
}

fn validate_hole(request: &HoleRequest) -> Result<(), SolidError> {
    validate_positive(request.diameter, "hole diameter")?;
    let legacy_position;
    let positions = if request.positions.is_empty() {
        legacy_position = HolePositionDto {
            position: request.position,
            position_reference: request.position_reference.clone(),
        };
        std::slice::from_ref(&legacy_position)
    } else {
        &request.positions
    };
    validate_hole_positions(positions)?;
    validate_hole_values(
        request.extent,
        request.style,
        request.diameter,
        request.counterbore_diameter,
        request.counterbore_depth,
        request.countersink_diameter,
        request.countersink_angle_deg,
        request.bottom_style,
        request.drill_point_angle_deg,
        request.thread.as_ref(),
    )
}

fn validate_hole_definition(definition: &HoleDefinitionDto) -> Result<(), SolidError> {
    validate_positive(definition.diameter, "hole diameter")?;
    let positions = hole_positions(definition);
    validate_hole_positions(&positions)?;
    validate_hole_values(
        definition.extent,
        definition.style,
        definition.diameter,
        definition.counterbore_diameter,
        definition.counterbore_depth,
        definition.countersink_diameter,
        definition.countersink_angle_deg,
        definition.bottom_style,
        definition.drill_point_angle_deg,
        definition.thread.as_ref(),
    )
}

fn validate_hole_positions(positions: &[HolePositionDto]) -> Result<(), SolidError> {
    if positions.is_empty() {
        return Err(SolidError::EmptySelection);
    }
    if positions
        .iter()
        .any(|position| !position.position.x.is_finite() || !position.position.y.is_finite())
    {
        return Err(SolidError::InvalidExtent(
            "hole positions must be finite".to_string(),
        ));
    }
    Ok(())
}

fn validate_hole_values(
    extent: HoleExtent,
    style: HoleStyle,
    diameter: f64,
    counterbore_diameter: f64,
    counterbore_depth: f64,
    countersink_diameter: f64,
    countersink_angle_deg: f64,
    bottom_style: HoleBottomStyle,
    drill_point_angle_deg: f64,
    thread: Option<&HoleThreadDto>,
) -> Result<(), SolidError> {
    if let HoleExtent::Distance { depth } = extent {
        validate_positive(depth, "hole depth")?;
    }
    match style {
        HoleStyle::Simple => {}
        HoleStyle::Counterbore => {
            validate_positive(counterbore_diameter, "counterbore diameter")?;
            validate_positive(counterbore_depth, "counterbore depth")?;
            if counterbore_diameter <= diameter + EPS {
                return Err(SolidError::InvalidExtent(
                    "counterbore diameter must exceed the hole diameter".to_string(),
                ));
            }
        }
        HoleStyle::Countersink => {
            validate_positive(countersink_diameter, "countersink diameter")?;
            if countersink_diameter <= diameter + EPS {
                return Err(SolidError::InvalidExtent(
                    "countersink diameter must exceed the hole diameter".to_string(),
                ));
            }
            if !countersink_angle_deg.is_finite()
                || countersink_angle_deg <= EPS
                || countersink_angle_deg >= 180.0 - EPS
            {
                return Err(SolidError::InvalidExtent(
                    "countersink angle must be between 0° and 180°".to_string(),
                ));
            }
        }
    }
    if bottom_style == HoleBottomStyle::DrillPoint
        && (!drill_point_angle_deg.is_finite()
            || drill_point_angle_deg <= EPS
            || drill_point_angle_deg >= 180.0 - EPS)
    {
        return Err(SolidError::InvalidExtent(
            "drill point angle must be between 0° and 180°".to_string(),
        ));
    }
    if let Some(thread) = thread {
        validate_hole_thread(thread, diameter, extent)?;
    }
    Ok(())
}

fn validate_hole_thread(
    thread: &HoleThreadDto,
    predrill_diameter: f64,
    hole_extent: HoleExtent,
) -> Result<(), SolidError> {
    validate_thread_spec(thread)?;
    if thread.nominal_diameter <= predrill_diameter + EPS {
        return Err(SolidError::InvalidExtent(
            "thread nominal diameter must exceed the predrill diameter".to_string(),
        ));
    }
    let iso_limits = iso_metric_thread_envelope(thread, ThreadFit::Internal)
        .map_err(SolidError::InvalidExtent)?;
    if let Some(limits) = iso_limits {
        if limits.modeled_major <= limits.modeled_pitch + EPS
            || limits.modeled_pitch <= limits.modeled_minor + EPS
        {
            return Err(SolidError::InvalidExtent(
                "ISO thread tolerance limits do not form a valid 60° internal profile".to_string(),
            ));
        }
    }
    if let Some(depth) = thread.depth {
        if let HoleExtent::Distance { depth: hole_depth } = hole_extent {
            if depth > hole_depth + EPS {
                return Err(SolidError::InvalidExtent(
                    "thread depth cannot exceed the cylindrical hole depth".to_string(),
                ));
            }
        }
    }
    if thread.representation == HoleThreadRepresentation::Modeled && iso_limits.is_none() {
        // Start with the P/8 basic root flat at the major diameter, then
        // widen toward the actual predrill along 60° flanks. An excessively
        // small custom predrill would make adjacent turns overlap.
        let radial_depth = (thread.nominal_diameter - predrill_diameter) * 0.5;
        let inner_half_width = thread.pitch * 0.0625 + radial_depth * (30.0_f64.to_radians().tan());
        if inner_half_width >= thread.pitch * 0.499 {
            return Err(SolidError::InvalidExtent(
                "predrill diameter is too small for a non-overlapping 60° modeled thread"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_external_thread(
    thread: &HoleThreadDto,
    surface_diameter: f64,
) -> Result<(), SolidError> {
    validate_thread_spec(thread)?;
    validate_positive(surface_diameter, "thread cylinder diameter")?;
    if let Some(limits) = iso_metric_thread_envelope(thread, ThreadFit::External)
        .map_err(SolidError::InvalidExtent)?
    {
        if limits.modeled_major <= limits.modeled_pitch + EPS
            || limits.modeled_pitch <= limits.modeled_minor + EPS
        {
            return Err(SolidError::InvalidExtent(
                "ISO thread tolerance limits do not form a valid 60° external profile".to_string(),
            ));
        }
    }
    let tolerance = (thread.nominal_diameter.abs() * 0.002).max(0.01);
    if (thread.nominal_diameter - surface_diameter).abs() > tolerance {
        return Err(SolidError::InvalidExtent(format!(
            "selected cylinder diameter ({surface_diameter:.3} mm) must match the thread major diameter ({:.3} mm)",
            thread.nominal_diameter
        )));
    }
    Ok(())
}

fn validate_thread_spec(thread: &HoleThreadDto) -> Result<(), SolidError> {
    validate_positive(thread.nominal_diameter, "thread nominal diameter")?;
    validate_positive(thread.pitch, "thread pitch")?;
    if thread.designation.trim().is_empty() {
        return Err(SolidError::InvalidExtent(
            "thread designation cannot be empty".to_string(),
        ));
    }
    if thread.class.trim().is_empty() {
        return Err(SolidError::InvalidExtent(
            "thread tolerance class cannot be empty".to_string(),
        ));
    }
    match (thread.standard, thread.series) {
        (
            HoleThreadStandard::IsoMetric,
            HoleThreadSeries::MetricCoarse | HoleThreadSeries::MetricFine,
        )
        | (HoleThreadStandard::UnifiedInch, HoleThreadSeries::Unc | HoleThreadSeries::Unf) => {}
        _ => {
            return Err(SolidError::InvalidExtent(
                "thread series does not match its standard".to_string(),
            ));
        }
    }
    match thread.standard {
        HoleThreadStandard::IsoMetric => {
            if thread.threads_per_inch.is_some() {
                return Err(SolidError::InvalidExtent(
                    "ISO metric threads must not specify threads per inch".to_string(),
                ));
            }
        }
        HoleThreadStandard::UnifiedInch => {
            let tpi = thread.threads_per_inch.ok_or_else(|| {
                SolidError::InvalidExtent(
                    "Unified threads must specify threads per inch".to_string(),
                )
            })?;
            validate_positive(tpi, "threads per inch")?;
            let expected_pitch = 25.4 / tpi;
            if (thread.pitch - expected_pitch).abs() > expected_pitch * 1e-6 {
                return Err(SolidError::InvalidExtent(
                    "Unified thread pitch must equal 25.4 / threads per inch".to_string(),
                ));
            }
        }
    }
    if let Some(depth) = thread.depth {
        validate_positive(depth, "thread depth")?;
    }
    Ok(())
}

fn hole_definition(
    feature_id: FeatureId,
    name: String,
    request: HoleRequest,
    face_basis: PlaneBasis,
) -> HoleDefinitionDto {
    HoleDefinitionDto {
        feature_id,
        name,
        body_id: request.body_id,
        face_id: request.face_id,
        position: request.position,
        position_reference: request.position_reference,
        positions: request.positions,
        diameter: request.diameter,
        extent: request.extent,
        style: request.style,
        counterbore_diameter: request.counterbore_diameter,
        counterbore_depth: request.counterbore_depth,
        countersink_diameter: request.countersink_diameter,
        countersink_angle_deg: request.countersink_angle_deg,
        bottom_style: request.bottom_style,
        drill_point_angle_deg: request.drill_point_angle_deg,
        thread: request.thread,
        flip: request.flip,
        face_basis: Some(face_basis),
    }
}

fn hole_positions(definition: &HoleDefinitionDto) -> Vec<HolePositionDto> {
    if definition.positions.is_empty() {
        vec![HolePositionDto {
            position: definition.position,
            position_reference: definition.position_reference.clone(),
        }]
    } else {
        definition.positions.clone()
    }
}

fn hole_reference_center(
    reference: &SketchPointRefDto,
    catalog: &[ProfileCatalogItemDto],
    active_features: &BTreeSet<FeatureId>,
    support_basis: PlaneBasis,
) -> Result<[f64; 3], SolidError> {
    let sketch = find_sketch(catalog, active_features, &reference.sketch_name)?;
    let point = sketch
        .reference_points
        .iter()
        .find(|candidate| {
            candidate.entity_id == reference.entity_id && candidate.point == reference.point
        })
        .ok_or_else(|| {
            SolidError::InvalidHistory(format!(
                "point reference {} on entity {} in '{}' is not available",
                sketch_point_label(&reference.point),
                reference.entity_id,
                reference.sketch_name,
            ))
        })?;
    let world = sketch.basis.to_3d([point.position.x, point.position.y]);
    let offset = [
        world[0] - support_basis.origin[0],
        world[1] - support_basis.origin[1],
        world[2] - support_basis.origin[2],
    ];
    let normal_offset = dot(offset, support_basis.normal);
    Ok([
        world[0] - support_basis.normal[0] * normal_offset,
        world[1] - support_basis.normal[1] * normal_offset,
        world[2] - support_basis.normal[2] * normal_offset,
    ])
}

fn sketch_point_label(point: &SketchPointKindDto) -> String {
    match point {
        SketchPointKindDto::Point => "point".to_string(),
        SketchPointKindDto::Start => "start".to_string(),
        SketchPointKindDto::End => "end".to_string(),
        SketchPointKindDto::Center => "center".to_string(),
        SketchPointKindDto::FitPoint { index } => format!("fit point {index}"),
    }
}

fn refresh_refinement_references(
    fillets: &mut [SolidFilletDefinitionDto],
    chamfers: &mut [SolidChamferDefinitionDto],
    scene: &SolidSceneDto,
) {
    for definition in fillets {
        if let Ok(keys) = edge_keys_for(scene, definition.body_id, &definition.edge_ids) {
            definition.edge_keys = keys;
        }
    }
    for definition in chamfers {
        if let Ok(keys) = edge_keys_for(scene, definition.body_id, &definition.edge_ids) {
            definition.edge_keys = keys;
        }
    }
    // Hole support planes are intentionally not refreshed from the current
    // result scene. Their selected face has already been consumed by the hole
    // boolean, and its stable-id slot may now describe an unrelated face.
    // Creation and explicit edit capture the authoritative support basis.
}

fn refresh_body_feature_references(
    definitions: &mut [BodyFeatureDefinitionDto],
    scene: &SolidSceneDto,
) {
    for definition in definitions {
        match definition {
            BodyFeatureDefinitionDto::Shell {
                body_id,
                face_ids,
                face_keys,
                ..
            } => {
                if let Ok(keys) = face_keys_for(scene, *body_id, face_ids) {
                    *face_keys = keys;
                }
            }
            BodyFeatureDefinitionDto::Mirror {
                plane, plane_basis, ..
            }
            | BodyFeatureDefinitionDto::SplitBody {
                plane, plane_basis, ..
            } => match *plane {
                nbcad_core::PlaneRef::OriginPlane { .. } => {
                    if let Ok(basis) = plane.origin_basis() {
                        *plane_basis = basis;
                    }
                }
                nbcad_core::PlaneRef::PlanarFace { face_id } => {
                    if let Some(basis) = face_basis(scene, face_id) {
                        *plane_basis = basis;
                    }
                }
                nbcad_core::PlaneRef::DatumPlane { .. } => {}
            },
            _ => {}
        }
    }
}

fn nearest_target_offset(
    sketch_basis: PlaneBasis,
    target_body_ids: &[BodyId],
    scene: &SolidSceneDto,
) -> Result<f64, SolidError> {
    if target_body_ids.is_empty() {
        return Err(SolidError::InvalidExtent(
            "To Next requires at least one target body".to_string(),
        ));
    }
    let targets = target_body_ids.iter().copied().collect::<BTreeSet<_>>();
    let nearest = scene
        .bodies
        .iter()
        .filter(|body| targets.contains(&body.id))
        .flat_map(|body| &body.faces)
        .filter_map(|face| face.plane)
        .filter(|plane| dot(plane.normal, sketch_basis.normal).abs() >= 1.0 - 1e-6)
        .map(|plane| {
            dot(
                [
                    plane.origin[0] - sketch_basis.origin[0],
                    plane.origin[1] - sketch_basis.origin[1],
                    plane.origin[2] - sketch_basis.origin[2],
                ],
                sketch_basis.normal,
            )
        })
        .filter(|distance| *distance > EPS)
        .min_by(|a, b| a.total_cmp(b));
    nearest.ok_or_else(|| {
        SolidError::InvalidExtent("To Next found no forward parallel target face".to_string())
    })
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn plane_bases_coplanar(first: PlaneBasis, second: PlaneBasis) -> bool {
    const NORMAL_TOLERANCE: f64 = 1e-6;
    const PLANE_DISTANCE_TOLERANCE_MM: f64 = 1e-5;
    let first_normal_length = dot(first.normal, first.normal).sqrt();
    let second_normal_length = dot(second.normal, second.normal).sqrt();
    if first_normal_length <= EPS || second_normal_length <= EPS {
        return false;
    }
    let alignment =
        dot(first.normal, second.normal).abs() / (first_normal_length * second_normal_length);
    if alignment < 1.0 - NORMAL_TOLERANCE {
        return false;
    }
    let origin_delta = [
        second.origin[0] - first.origin[0],
        second.origin[1] - first.origin[1],
        second.origin[2] - first.origin[2],
    ];
    dot(origin_delta, first.normal).abs() / first_normal_length <= PLANE_DISTANCE_TOLERANCE_MM
}

#[cfg(test)]
mod tests {
    use super::*;
    use nbcad_core::OriginPlane;

    fn catalog() -> Vec<ProfileCatalogItemDto> {
        vec![ProfileCatalogItemDto {
            sketch_name: "Sketch1".to_string(),
            feature_id: FeatureId(1),
            basis: nbcad_core::PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            }
            .origin_basis()
            .unwrap(),
            profiles: vec![
                ProfileLoopDto {
                    index: 0,
                    parent_index: None,
                    nesting_depth: 0,
                    points: vec![
                        Point2Dto::new(0.0, 0.0),
                        Point2Dto::new(20.0, 0.0),
                        Point2Dto::new(20.0, 10.0),
                        Point2Dto::new(0.0, 10.0),
                    ],
                    area: 200.0,
                    curves: Vec::new(),
                },
                ProfileLoopDto {
                    index: 1,
                    parent_index: None,
                    nesting_depth: 0,
                    points: vec![
                        Point2Dto::new(30.0, 0.0),
                        Point2Dto::new(40.0, 0.0),
                        Point2Dto::new(40.0, 10.0),
                        Point2Dto::new(30.0, 10.0),
                    ],
                    area: 100.0,
                    curves: Vec::new(),
                },
            ],
            profile_error: None,
            lines: vec![SketchLineDto {
                entity_id: 99,
                start: Point2Dto::new(0.0, 0.0),
                end: Point2Dto::new(0.0, 20.0),
            }],
            path_curves: vec![SketchPathCurveDto::Line {
                entity_id: 99,
                start: Point2Dto::new(0.0, 0.0),
                end: Point2Dto::new(0.0, 20.0),
            }],
            reference_points: vec![
                SketchReferencePointDto {
                    entity_id: 99,
                    point: SketchPointKindDto::Start,
                    position: Point2Dto::new(0.0, 0.0),
                },
                SketchReferencePointDto {
                    entity_id: 99,
                    point: SketchPointKindDto::End,
                    position: Point2Dto::new(0.0, 20.0),
                },
            ],
        }]
    }

    fn catalog_with_second_section() -> Vec<ProfileCatalogItemDto> {
        let mut catalog = catalog();
        let mut second = catalog[0].clone();
        second.sketch_name = "Sketch2".to_string();
        second.feature_id = FeatureId(2);
        second.basis.origin[2] = 20.0;
        catalog.push(second);
        catalog
    }

    fn catalog_with_coplanar_axis_sketch() -> Vec<ProfileCatalogItemDto> {
        let mut catalog = catalog();
        let mut axis = catalog[0].clone();
        axis.sketch_name = "AxisSketch".to_string();
        axis.feature_id = FeatureId(2);
        axis.profiles.clear();
        // Same XY plane, but with a translated and quarter-turned local basis.
        // The local line below maps to the world-space X=0 profile boundary.
        axis.basis.origin = [5.0, 5.0, 0.0];
        axis.basis.u = [0.0, 1.0, 0.0];
        axis.basis.v = [-1.0, 0.0, 0.0];
        axis.basis.normal = [0.0, 0.0, 1.0];
        axis.lines = vec![SketchLineDto {
            entity_id: 199,
            start: Point2Dto::new(-5.0, 5.0),
            end: Point2Dto::new(15.0, 5.0),
        }];
        axis.path_curves.clear();
        axis.reference_points.clear();
        catalog.push(axis);
        catalog
    }

    #[test]
    fn associative_hole_point_follows_its_finished_sketch_reference() {
        let mut catalog = catalog();
        let active = BTreeSet::from([FeatureId(1)]);
        let reference = SketchPointRefDto {
            sketch_name: "Sketch1".to_string(),
            entity_id: 99,
            point: SketchPointKindDto::End,
        };
        let basis = catalog[0].basis;
        assert_eq!(
            hole_reference_center(&reference, &catalog, &active, basis).unwrap(),
            [0.0, 20.0, 0.0],
        );

        catalog[0]
            .reference_points
            .iter_mut()
            .find(|point| point.entity_id == 99 && point.point == SketchPointKindDto::End)
            .unwrap()
            .position = Point2Dto::new(0.0, 25.0);
        assert_eq!(
            hole_reference_center(&reference, &catalog, &active, basis).unwrap(),
            [0.0, 25.0, 0.0],
        );

        catalog[0].reference_points.clear();
        assert!(matches!(
            hole_reference_center(&reference, &catalog, &active, basis),
            Err(SolidError::InvalidHistory(_)),
        ));
    }

    #[test]
    fn associative_hole_point_projects_from_a_parallel_base_sketch() {
        let mut catalog = catalog();
        catalog[0].basis.origin[2] = -10.0;
        let support = nbcad_core::PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .origin_basis()
        .unwrap();
        let reference = SketchPointRefDto {
            sketch_name: "Sketch1".to_string(),
            entity_id: 99,
            point: SketchPointKindDto::End,
        };

        assert_eq!(
            hole_reference_center(
                &reference,
                &catalog,
                &BTreeSet::from([FeatureId(1)]),
                support,
            )
            .unwrap(),
            [0.0, 20.0, 0.0],
        );
    }

    fn metric_thread() -> HoleThreadDto {
        HoleThreadDto {
            standard: HoleThreadStandard::IsoMetric,
            series: HoleThreadSeries::MetricCoarse,
            designation: "M6 x 1 - 6H".to_string(),
            class: "6H".to_string(),
            nominal_diameter: 6.0,
            pitch: 1.0,
            threads_per_inch: None,
            hand: HoleThreadHand::Right,
            depth: Some(7.0),
            representation: HoleThreadRepresentation::Modeled,
            tap_drill_designation: Some("5 mm".to_string()),
        }
    }

    #[test]
    fn threaded_hole_validation_enforces_standard_pitch_depth_and_profile() {
        let metric = metric_thread();
        validate_hole_thread(&metric, 5.0, HoleExtent::Distance { depth: 8.0 }).unwrap();

        let mut wrong_series = metric.clone();
        wrong_series.series = HoleThreadSeries::Unc;
        assert!(matches!(
            validate_hole_thread(
                &wrong_series,
                5.0,
                HoleExtent::Distance { depth: 8.0 }
            ),
            Err(SolidError::InvalidExtent(message))
                if message.contains("series does not match")
        ));

        let mut too_deep = metric.clone();
        too_deep.depth = Some(9.0);
        assert!(matches!(
            validate_hole_thread(
                &too_deep,
                5.0,
                HoleExtent::Distance { depth: 8.0 }
            ),
            Err(SolidError::InvalidExtent(message))
                if message.contains("cannot exceed")
        ));

        // The tap drill is manufacturing guidance, not the finished 6H minor
        // diameter. A smaller custom drill must not distort or reject the
        // standards-derived finished B-rep.
        validate_hole_thread(&metric, 4.0, HoleExtent::Distance { depth: 8.0 }).unwrap();

        let mut unsupported_class = metric.clone();
        unsupported_class.class = "5H".to_string();
        assert!(matches!(
            validate_hole_thread(
                &unsupported_class,
                5.0,
                HoleExtent::Distance { depth: 8.0 }
            ),
            Err(SolidError::InvalidExtent(message))
                if message.contains("supports tolerance class 6H")
        ));

        let mut unified = metric;
        unified.standard = HoleThreadStandard::UnifiedInch;
        unified.series = HoleThreadSeries::Unc;
        unified.designation = "1/4-20 UNC-2B".to_string();
        unified.class = "2B".to_string();
        unified.nominal_diameter = 6.35;
        unified.pitch = 1.27;
        unified.threads_per_inch = Some(20.0);
        validate_hole_thread(&unified, 5.1054, HoleExtent::Distance { depth: 8.0 }).unwrap();
        unified.pitch = 1.2;
        assert!(matches!(
            validate_hole_thread(
                &unified,
                5.1054,
                HoleExtent::Distance { depth: 8.0 }
            ),
            Err(SolidError::InvalidExtent(message))
                if message.contains("25.4 / threads per inch")
        ));
    }

    #[test]
    fn one_hole_feature_plans_every_selected_position_with_shared_parameters() {
        let mut document = SolidDocument::new();
        let extrude = document
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let body_id = extrude_job(&extrude.jobs[0]).result_body_ids[0];
        document
            .commit(
                extrude.transaction_id,
                KernelSceneDto {
                    bodies: vec![raw_body(body_id)],
                    errors: Vec::new(),
                },
            )
            .unwrap();
        let face_id = document.scene().bodies[0].faces[0].id;
        let references = [SketchPointKindDto::Start, SketchPointKindDto::End]
            .into_iter()
            .map(|point| HolePositionDto {
                position: Point2Dto::new(0.0, 0.0),
                position_reference: Some(SketchPointRefDto {
                    sketch_name: "Sketch1".to_string(),
                    entity_id: 99,
                    point,
                }),
            })
            .collect::<Vec<_>>();

        let plan = document
            .prepare_add_hole(
                FeatureId(3),
                "Hole1",
                HoleRequest {
                    body_id,
                    face_id,
                    position: references[0].position,
                    position_reference: references[0].position_reference.clone(),
                    positions: references,
                    diameter: 4.0,
                    extent: HoleExtent::Distance { depth: 6.0 },
                    style: HoleStyle::Simple,
                    counterbore_diameter: 0.0,
                    counterbore_depth: 0.0,
                    countersink_diameter: 0.0,
                    countersink_angle_deg: 90.0,
                    bottom_style: HoleBottomStyle::DrillPoint,
                    drill_point_angle_deg: 118.0,
                    thread: None,
                    flip: false,
                },
                &catalog(),
                &active(&[1, 2, 3]),
            )
            .unwrap();
        let jobs = plan
            .jobs
            .iter()
            .filter_map(|job| match job {
                KernelJobDto::Hole(job) => Some(job),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].center, Point3Dto::from([0.0, 0.0, 0.0]));
        assert_eq!(jobs[1].center, Point3Dto::from([0.0, 20.0, 0.0]));
        assert!(jobs.iter().all(|job| {
            job.bottom_style == HoleBottomStyle::DrillPoint
                && (job.drill_point_angle_deg - 118.0).abs() < EPS
        }));
    }

    #[test]
    fn associative_hole_does_not_retarget_to_a_reused_post_boolean_face_id() {
        let mut document = SolidDocument::new();
        let extrude = document
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let body_id = extrude_job(&extrude.jobs[0]).result_body_ids[0];
        document
            .commit(
                extrude.transaction_id,
                KernelSceneDto {
                    bodies: vec![raw_body(body_id)],
                    errors: Vec::new(),
                },
            )
            .unwrap();
        let face_id = document.scene().bodies[0].faces[0].id;
        let hole = document
            .prepare_add_hole(
                FeatureId(3),
                "Hole1",
                HoleRequest {
                    body_id,
                    face_id,
                    position: Point2Dto::new(0.0, 20.0),
                    position_reference: Some(SketchPointRefDto {
                        sketch_name: "Sketch1".to_string(),
                        entity_id: 99,
                        point: SketchPointKindDto::End,
                    }),
                    positions: Vec::new(),
                    diameter: 2.0,
                    extent: HoleExtent::ThroughAll,
                    style: HoleStyle::Simple,
                    counterbore_diameter: 0.0,
                    counterbore_depth: 0.0,
                    countersink_diameter: 0.0,
                    countersink_angle_deg: 90.0,
                    bottom_style: HoleBottomStyle::Flat,
                    drill_point_angle_deg: 118.0,
                    thread: None,
                    flip: false,
                },
                &catalog(),
                &active(&[1, 2, 3]),
            )
            .unwrap();

        // Model the common post-boolean topology case: the face key/id slot
        // still exists but now describes an unrelated, perpendicular face.
        let reused_face_basis = nbcad_core::PlaneRef::OriginPlane {
            plane: OriginPlane::Xz,
        }
        .origin_basis()
        .unwrap();
        let mut post_boolean_body = raw_body(body_id);
        post_boolean_body.faces[0].plane = Some(reused_face_basis);
        document
            .commit(
                hole.transaction_id,
                KernelSceneDto {
                    bodies: vec![post_boolean_body],
                    errors: Vec::new(),
                },
            )
            .unwrap();
        // Older saves may already contain a poisoned cached support basis
        // from the pre-fix replay path. The sketch association must recover
        // without requiring users to recreate the feature.
        document.holes[0].face_basis = Some(reused_face_basis);

        let replay = document
            .prepare_recompute(&catalog(), &active(&[1, 2, 3]))
            .unwrap();
        let hole_job = replay
            .jobs
            .iter()
            .find_map(|job| match job {
                KernelJobDto::Hole(job) => Some(job),
                _ => None,
            })
            .expect("associative hole should remain plannable");
        assert_eq!(hole_job.center, Point3Dto::from([0.0, 20.0, 0.0]));
        assert_eq!(hole_job.direction, Point3Dto::from([0.0, 0.0, -1.0]));
    }

    #[test]
    fn direct_hole_keeps_its_creation_plane_when_the_result_reuses_the_face_id() {
        let mut document = SolidDocument::new();
        let extrude = document
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let body_id = extrude_job(&extrude.jobs[0]).result_body_ids[0];
        document
            .commit(
                extrude.transaction_id,
                KernelSceneDto {
                    bodies: vec![raw_body(body_id)],
                    errors: Vec::new(),
                },
            )
            .unwrap();
        let face_id = document.scene().bodies[0].faces[0].id;
        let creation_basis = document.scene().bodies[0].faces[0].plane.unwrap();
        let hole = document
            .prepare_add_hole(
                FeatureId(3),
                "Hole1",
                HoleRequest {
                    body_id,
                    face_id,
                    position: Point2Dto::new(2.0, 3.0),
                    position_reference: None,
                    positions: Vec::new(),
                    diameter: 2.0,
                    extent: HoleExtent::ThroughAll,
                    style: HoleStyle::Simple,
                    counterbore_diameter: 0.0,
                    counterbore_depth: 0.0,
                    countersink_diameter: 0.0,
                    countersink_angle_deg: 90.0,
                    bottom_style: HoleBottomStyle::Flat,
                    drill_point_angle_deg: 118.0,
                    thread: None,
                    flip: false,
                },
                &catalog(),
                &active(&[1, 2, 3]),
            )
            .unwrap();

        let reused_face_basis = nbcad_core::PlaneRef::OriginPlane {
            plane: OriginPlane::Xz,
        }
        .origin_basis()
        .unwrap();
        let mut post_boolean_body = raw_body(body_id);
        post_boolean_body.faces[0].plane = Some(reused_face_basis);
        document
            .commit(
                hole.transaction_id,
                KernelSceneDto {
                    bodies: vec![post_boolean_body],
                    errors: Vec::new(),
                },
            )
            .unwrap();

        let replay = document
            .prepare_recompute(&catalog(), &active(&[1, 2, 3]))
            .unwrap();
        let hole_job = replay
            .jobs
            .iter()
            .find_map(|job| match job {
                KernelJobDto::Hole(job) => Some(job),
                _ => None,
            })
            .expect("direct hole should remain plannable");
        assert_eq!(
            document.pending.as_ref().unwrap().holes[0].face_basis,
            Some(creation_basis)
        );
        assert_eq!(
            hole_job.center,
            Point3Dto::from(creation_basis.to_3d([2.0, 3.0]))
        );
        assert_eq!(
            hole_job.direction,
            Point3Dto::from([
                -creation_basis.normal[0],
                -creation_basis.normal[1],
                -creation_basis.normal[2],
            ])
        );
    }

    fn request(indices: Vec<u32>) -> ExtrudeRequest {
        ExtrudeRequest {
            source_face: None,
            sketch_name: "Sketch1".to_string(),
            profile_indices: indices,
            operation: ExtrudeOperation::NewBody,
            extent: ExtrudeExtent::Distance { distance: 15.0 },
            taper_angle_deg: 0.0,
            flip: false,
            target_body_ids: Vec::new(),
        }
    }

    #[test]
    fn to_face_extent_can_bootstrap_from_its_saved_reference_plane() {
        let sketch_basis = catalog()[0].basis;
        let mut target_basis = sketch_basis;
        target_basis.origin[2] = 25.0;
        let offsets = super::offsets(
            ExtrudeExtent::ToFace {
                face_id: FaceId(99),
            },
            sketch_basis,
            &SolidSceneDto::default(),
            Some(target_basis),
        )
        .unwrap();
        assert_eq!(offsets, (0.0, 25.0));
    }

    fn active(ids: &[u64]) -> BTreeSet<FeatureId> {
        ids.iter().copied().map(FeatureId).collect()
    }

    fn raw_body(id: BodyId) -> KernelBodyDto {
        KernelBodyDto {
            topology_signature: String::new(),
            body_id: id,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![KernelFaceDto {
                key: "face:0".to_string(),
                first_index: 0,
                index_count: 3,
                plane: Some(catalog()[0].basis),
                signature: Some(PlanarFaceSignatureDto {
                    centroid: Point3Dto {
                        x: 1.0 / 3.0,
                        y: 1.0 / 3.0,
                        z: 0.0,
                    },
                    normal: Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    area: 0.5,
                    perimeter: 2.0 + 2.0_f64.sqrt(),
                    wire_count: 1,
                    edge_count: 3,
                }),
                cylinder: None,
            }],
            edges: vec![],
        }
    }

    fn cylindrical_body(id: BodyId, radius: f64) -> KernelBodyDto {
        let mut body = raw_body(id);
        let face = &mut body.faces[0];
        face.key = "cylinder:0".to_string();
        face.plane = None;
        face.signature = None;
        face.cylinder = Some(CylindricalSurfaceDto {
            origin: Point3Dto::from([0.0, 0.0, 0.0]),
            axis: Point3Dto::from([0.0, 0.0, 1.0]),
            reference: Point3Dto::from([1.0, 0.0, 0.0]),
            radius,
        });
        body
    }

    fn extrude_job(job: &KernelJobDto) -> &KernelExtrudeJobDto {
        match job {
            KernelJobDto::Extrude(job) => job,
            _ => panic!("expected an Extrude job"),
        }
    }

    #[test]
    fn rectangular_new_body_plan_and_commit_have_stable_ids() {
        let mut doc = SolidDocument::new();
        let plan = doc
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        assert_eq!(plan.jobs.len(), 1);
        assert_eq!(extrude_job(&plan.jobs[0]).start_offset, 0.0);
        assert_eq!(extrude_job(&plan.jobs[0]).end_offset, 15.0);
        let body_id = extrude_job(&plan.jobs[0]).result_body_ids[0];
        doc.commit(
            plan.transaction_id,
            KernelSceneDto {
                bodies: vec![raw_body(body_id)],
                errors: vec![],
            },
        )
        .unwrap();
        let face_id = doc.scene().bodies[0].faces[0].id;

        let plan2 = doc
            .prepare_edit(
                FeatureId(2),
                ExtrudeRequest {
                    extent: ExtrudeExtent::Distance { distance: 25.0 },
                    ..request(vec![0])
                },
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        assert_eq!(extrude_job(&plan2.jobs[0]).result_body_ids[0], body_id);
        doc.commit(
            plan2.transaction_id,
            KernelSceneDto {
                bodies: vec![raw_body(body_id)],
                errors: vec![],
            },
        )
        .unwrap();
        assert_eq!(doc.scene().bodies[0].faces[0].id, face_id);
    }

    #[test]
    fn planar_face_extrude_saves_stable_id_and_validated_occt_signature() {
        let mut doc = SolidDocument::new();
        let base = doc
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let source_body_id = extrude_job(&base.jobs[0]).result_body_ids[0];
        doc.commit(
            base.transaction_id,
            KernelSceneDto {
                bodies: vec![raw_body(source_body_id)],
                errors: Vec::new(),
            },
        )
        .unwrap();
        let source_face_id = doc.scene().bodies[0].faces[0].id;
        let source_signature = doc.scene().bodies[0].faces[0].signature.unwrap();
        let face_request = ExtrudeRequest {
            source_face: Some(PlanarFaceSourceDto {
                body_id: source_body_id,
                face_id: source_face_id,
            }),
            sketch_name: String::new(),
            profile_indices: Vec::new(),
            operation: ExtrudeOperation::NewBody,
            extent: ExtrudeExtent::Distance { distance: 8.0 },
            taper_angle_deg: 0.0,
            flip: false,
            target_body_ids: Vec::new(),
        };
        let plan = doc
            .prepare_add(
                FeatureId(3),
                "Extrude2",
                face_request,
                &catalog(),
                &active(&[1, 2, 3]),
            )
            .unwrap();
        let job = extrude_job(&plan.jobs[1]);
        assert!(job.profiles.is_empty());
        assert_eq!(job.normal, Point3Dto::from(catalog()[0].basis.normal));
        assert_eq!(
            job.source_face,
            Some(KernelPlanarFaceSourceDto {
                body_id: source_body_id,
                face_id: source_face_id,
                face_key: "face:0".to_string(),
                signature: source_signature,
            })
        );
        let definition = doc
            .pending
            .as_ref()
            .unwrap()
            .extrudes
            .iter()
            .find(|definition| definition.feature_id == FeatureId(3))
            .unwrap();
        assert_eq!(definition.source_face.unwrap().face_id, source_face_id);
        assert_eq!(definition.source_face_key.as_deref(), Some("face:0"));
        assert_eq!(definition.source_face_signature, Some(source_signature));
        assert_eq!(definition.source_face_basis, Some(catalog()[0].basis));

        let saved = serde_json::to_string(&doc.pending.as_ref().unwrap().extrudes).unwrap();
        let definitions: Vec<ExtrudeDefinitionDto> = serde_json::from_str(&saved).unwrap();
        let mut restored = SolidDocument::restore_definitions(definitions).unwrap();
        let replay = restored
            .prepare_recompute(&catalog(), &active(&[1, 2, 3]))
            .unwrap();
        let replay_face = extrude_job(&replay.jobs[1]).source_face.as_ref().unwrap();
        assert_eq!(replay_face.face_id, source_face_id);
        assert_eq!(replay_face.face_key, "face:0");
        assert_eq!(replay_face.signature, source_signature);
    }

    #[test]
    fn multi_profile_and_symmetric_extent_are_planned() {
        let mut doc = SolidDocument::new();
        let mut request = request(vec![0, 1]);
        request.extent = ExtrudeExtent::Symmetric { distance: 20.0 };
        let plan = doc
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request,
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        assert_eq!(extrude_job(&plan.jobs[0]).profiles.len(), 2);
        assert_eq!(extrude_job(&plan.jobs[0]).result_body_ids.len(), 2);
        assert_eq!(
            (
                extrude_job(&plan.jobs[0]).start_offset,
                extrude_job(&plan.jobs[0]).end_offset
            ),
            (-10.0, 10.0)
        );
    }

    #[test]
    fn retained_body_ids_drop_removed_outputs_after_profile_edit() {
        let mut doc = SolidDocument::new();
        let plan = doc
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0, 1]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let bodies = extrude_job(&plan.jobs[0]).result_body_ids.clone();
        doc.commit(
            plan.transaction_id,
            KernelSceneDto {
                bodies: bodies.iter().copied().map(raw_body).collect(),
                errors: vec![],
            },
        )
        .unwrap();
        assert_eq!(doc.retained_body_ids(), bodies.iter().copied().collect());

        let plan = doc.prepare_recompute(&catalog(), &active(&[1])).unwrap();
        doc.commit(
            plan.transaction_id,
            KernelSceneDto {
                bodies: vec![],
                errors: vec![],
            },
        )
        .unwrap();
        assert_eq!(
            doc.retained_body_ids(),
            bodies.iter().copied().collect(),
            "rollback retains both outputs"
        );

        let plan = doc
            .prepare_edit(FeatureId(2), request(vec![0]), &catalog(), &active(&[1, 2]))
            .unwrap();
        doc.commit(
            plan.transaction_id,
            KernelSceneDto {
                bodies: vec![raw_body(bodies[0])],
                errors: vec![],
            },
        )
        .unwrap();
        assert_eq!(
            doc.definitions()[0].new_body_ids.len(),
            2,
            "the second ID remains reserved for later editing"
        );
        assert_eq!(
            doc.retained_body_ids(),
            [bodies[0]].into_iter().collect(),
            "reserved IDs no longer produced by the feature are not retained bodies"
        );
    }

    #[test]
    fn signed_distance_reverses_a_one_sided_extrude() {
        let mut doc = SolidDocument::new();
        let plan = doc
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                ExtrudeRequest {
                    extent: ExtrudeExtent::Distance { distance: -10.0 },
                    ..request(vec![0])
                },
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        assert_eq!(extrude_job(&plan.jobs[0]).start_offset, 0.0);
        assert_eq!(extrude_job(&plan.jobs[0]).end_offset, -10.0);
    }

    #[test]
    fn join_without_a_target_combines_multiple_profiles_into_one_new_body() {
        let mut doc = SolidDocument::new();
        let plan = doc
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                ExtrudeRequest {
                    operation: ExtrudeOperation::Join,
                    ..request(vec![0, 1])
                },
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let job = extrude_job(&plan.jobs[0]);
        assert_eq!(job.operation, ExtrudeOperation::Join);
        assert_eq!(job.profiles.len(), 2);
        assert!(job.target_body_ids.is_empty());
        assert_eq!(job.result_body_ids.len(), 1);
    }

    #[test]
    fn revolve_plan_maps_the_sketch_axis_and_rejects_crossing_profiles() {
        let mut doc = SolidDocument::new();
        let request = RevolveRequest {
            sketch_name: "Sketch1".to_string(),
            profile_indices: vec![0],
            axis_origin: Point2Dto::new(0.0, 0.0),
            axis_direction: Point2Dto::new(0.0, 1.0),
            axis_line_sketch_name: None,
            axis_line_entity_id: Some(99),
            angle_deg: 360.0,
            flip: false,
            operation: ExtrudeOperation::NewBody,
            target_body_ids: Vec::new(),
        };
        let plan = doc
            .prepare_add_revolve(
                FeatureId(2),
                "Revolve1",
                request,
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let KernelJobDto::Revolve(job) = &plan.jobs[0] else {
            panic!("expected a Revolve job")
        };
        assert_eq!(job.result_body_ids.len(), 1);
        assert_eq!(job.axis_origin, Point3Dto::from([0.0, 0.0, 0.0]));
        assert_eq!(job.axis_direction, Point3Dto::from([0.0, 1.0, 0.0]));
        assert!((job.angle_rad - std::f64::consts::TAU).abs() < 1e-12);

        doc.cancel_pending(plan.transaction_id);
        let crossing = RevolveRequest {
            sketch_name: "Sketch1".to_string(),
            profile_indices: vec![0],
            axis_origin: Point2Dto::new(10.0, 0.0),
            axis_direction: Point2Dto::new(0.0, 1.0),
            axis_line_sketch_name: None,
            axis_line_entity_id: None,
            angle_deg: 180.0,
            flip: false,
            operation: ExtrudeOperation::NewBody,
            target_body_ids: Vec::new(),
        };
        assert!(matches!(
            doc.prepare_add_revolve(
                FeatureId(2),
                "Revolve1",
                crossing,
                &catalog(),
                &active(&[1, 2]),
            ),
            Err(SolidError::InvalidAxis(_))
        ));
    }

    #[test]
    fn revolve_accepts_a_stable_line_from_any_coplanar_earlier_sketch() {
        let mut doc = SolidDocument::new();
        let plan = doc
            .prepare_add_revolve(
                FeatureId(3),
                "Revolve1",
                RevolveRequest {
                    sketch_name: "Sketch1".to_string(),
                    profile_indices: vec![0],
                    axis_origin: Point2Dto::new(123.0, 456.0),
                    axis_direction: Point2Dto::new(1.0, 0.0),
                    axis_line_sketch_name: Some("AxisSketch".to_string()),
                    axis_line_entity_id: Some(199),
                    angle_deg: 180.0,
                    flip: false,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: Vec::new(),
                },
                &catalog_with_coplanar_axis_sketch(),
                &active(&[1, 2, 3]),
            )
            .unwrap();
        let KernelJobDto::Revolve(job) = &plan.jobs[0] else {
            panic!("expected a Revolve job")
        };
        assert_eq!(job.axis_origin, Point3Dto::from([0.0, 0.0, 0.0]));
        assert_eq!(job.axis_direction, Point3Dto::from([0.0, 1.0, 0.0]));
    }

    #[test]
    fn revolve_rejects_a_stable_line_from_a_parallel_offset_plane() {
        let mut doc = SolidDocument::new();
        assert!(matches!(
            doc.prepare_add_revolve(
                FeatureId(3),
                "Revolve1",
                RevolveRequest {
                    sketch_name: "Sketch1".to_string(),
                    profile_indices: vec![0],
                    axis_origin: Point2Dto::new(0.0, 0.0),
                    axis_direction: Point2Dto::new(0.0, 1.0),
                    axis_line_sketch_name: Some("Sketch2".to_string()),
                    axis_line_entity_id: Some(99),
                    angle_deg: 180.0,
                    flip: false,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: Vec::new(),
                },
                &catalog_with_second_section(),
                &active(&[1, 2, 3]),
            ),
            Err(SolidError::InvalidAxis(message)) if message.contains("not coplanar")
        ));
    }

    #[test]
    fn stable_line_references_drive_sweep_loft_and_rib_jobs() {
        let mut sweep_doc = SolidDocument::new();
        let sweep = sweep_doc
            .prepare_add_sweep(
                FeatureId(2),
                "Sweep1",
                SweepRequest {
                    profile: ProfileRefDto {
                        sketch_name: "Sketch1".to_string(),
                        profile_index: 0,
                    },
                    path_sketch_name: "Sketch1".to_string(),
                    path_entity_ids: vec![99],
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: Vec::new(),
                    guide_rail: None,
                    orientation: SweepOrientation::CorrectedFrenet,
                    transition: SweepTransition::Transformed,
                    force_c1: false,
                },
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let KernelJobDto::Sweep(sweep_job) = &sweep.jobs[0] else {
            panic!("expected a Sweep job")
        };
        assert_eq!(sweep_job.path.len(), 1);
        assert_eq!(sweep_job.result_body_ids.len(), 1);

        let mut loft_doc = SolidDocument::new();
        let loft = loft_doc
            .prepare_add_loft(
                FeatureId(3),
                "Loft1",
                LoftRequest {
                    sections: vec![
                        ProfileRefDto {
                            sketch_name: "Sketch1".to_string(),
                            profile_index: 0,
                        },
                        ProfileRefDto {
                            sketch_name: "Sketch2".to_string(),
                            profile_index: 0,
                        },
                    ],
                    ruled: true,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: Vec::new(),
                    continuity: LoftContinuity::G1,
                    centerline: None,
                    guide_rail: None,
                },
                &catalog_with_second_section(),
                &active(&[1, 2, 3]),
            )
            .unwrap();
        let KernelJobDto::Loft(loft_job) = &loft.jobs[0] else {
            panic!("expected a Loft job")
        };
        assert_eq!(loft_job.sections.len(), 2);
        assert!(loft_job.ruled);

        let mut rib_doc = SolidDocument::new();
        let rib = rib_doc
            .prepare_add_rib(
                FeatureId(2),
                "Rib1",
                RibRequest {
                    sketch_name: "Sketch1".to_string(),
                    line_entity_ids: vec![99],
                    thickness: 2.0,
                    depth: 10.0,
                    symmetric: true,
                    flip: false,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: Vec::new(),
                    extent: None,
                },
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let KernelJobDto::Rib(rib_job) = &rib.jobs[0] else {
            panic!("expected a Rib job")
        };
        assert_eq!((rib_job.start_offset, rib_job.end_offset), (-5.0, 5.0));
        assert_eq!(rib_job.profiles.len(), 1);
    }

    #[test]
    fn missing_stable_line_references_fail_recompute() {
        let mut revolve_doc = SolidDocument::new();
        let missing_axis = RevolveRequest {
            sketch_name: "Sketch1".to_string(),
            profile_indices: vec![0],
            axis_origin: Point2Dto::new(0.0, 0.0),
            axis_direction: Point2Dto::new(0.0, 1.0),
            axis_line_sketch_name: None,
            axis_line_entity_id: Some(404),
            angle_deg: 180.0,
            flip: false,
            operation: ExtrudeOperation::NewBody,
            target_body_ids: Vec::new(),
        };
        assert!(matches!(
            revolve_doc.prepare_add_revolve(
                FeatureId(2),
                "Revolve1",
                missing_axis,
                &catalog(),
                &active(&[1, 2]),
            ),
            Err(SolidError::InvalidAxis(message)) if message.contains("404")
        ));

        let mut sweep_doc = SolidDocument::new();
        assert!(matches!(
            sweep_doc.prepare_add_sweep(
                FeatureId(2),
                "Sweep1",
                SweepRequest {
                    profile: ProfileRefDto {
                        sketch_name: "Sketch1".to_string(),
                        profile_index: 0,
                    },
                    path_sketch_name: "Sketch1".to_string(),
                    path_entity_ids: vec![404],
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: Vec::new(),
                    guide_rail: None,
                    orientation: SweepOrientation::CorrectedFrenet,
                    transition: SweepTransition::Transformed,
                    force_c1: false,
                },
                &catalog(),
                &active(&[1, 2]),
            ),
            Err(SolidError::InvalidPath(message)) if message.contains("404")
        ));
    }

    #[test]
    fn rollback_omits_features_and_bad_targets_are_rejected() {
        let mut doc = SolidDocument::new();
        let plan = doc
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1]),
            )
            .unwrap();
        assert!(plan.jobs.is_empty());
        doc.commit(plan.transaction_id, KernelSceneDto::default())
            .unwrap();

        let mut bad = request(vec![0]);
        bad.operation = ExtrudeOperation::Cut;
        bad.target_body_ids = vec![BodyId(999)];
        assert!(matches!(
            doc.prepare_add(
                FeatureId(3),
                "Extrude2",
                bad,
                &catalog(),
                &active(&[1, 2, 3]),
            ),
            Err(SolidError::MissingTarget(BodyId(999)))
        ));
    }

    fn two_body_document() -> (SolidDocument, Vec<BodyId>) {
        let mut document = SolidDocument::new();
        let plan = document
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0, 1]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("expected base Extrude job");
        };
        let body_ids = job.result_body_ids.clone();
        document
            .commit(
                plan.transaction_id,
                KernelSceneDto {
                    bodies: body_ids.iter().copied().map(raw_body).collect(),
                    errors: Vec::new(),
                },
            )
            .unwrap();
        (document, body_ids)
    }

    #[test]
    fn curved_rib_centerlines_preserve_analytic_arc_and_circle_boundaries() {
        let mut path_catalog = catalog();
        path_catalog[0].lines.clear();
        path_catalog[0].path_curves = vec![
            SketchPathCurveDto::Arc {
                entity_id: 100,
                start: Point2Dto::new(10.0, 0.0),
                mid: Point2Dto::new(0.0, 10.0),
                end: Point2Dto::new(-10.0, 0.0),
            },
            SketchPathCurveDto::Circle {
                entity_id: 101,
                center: Point2Dto::new(30.0, 0.0),
                radius: 8.0,
            },
        ];
        let mut document = SolidDocument::new();
        let plan = document
            .prepare_add_rib(
                FeatureId(2),
                "Rib1",
                RibRequest {
                    sketch_name: "Sketch1".to_string(),
                    line_entity_ids: vec![100, 101],
                    thickness: 2.0,
                    depth: 10.0,
                    symmetric: false,
                    flip: false,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: Vec::new(),
                    extent: Some(RibExtent::Distance { depth: 10.0 }),
                },
                &path_catalog,
                &active(&[1, 2]),
            )
            .unwrap();
        let KernelJobDto::Rib(job) = &plan.jobs[0] else {
            panic!("expected Rib job");
        };
        assert_eq!(job.profiles.len(), 2);
        assert_eq!(
            job.profiles[0]
                .curves
                .iter()
                .filter(|curve| matches!(curve, KernelCurveDto::Arc { .. }))
                .count(),
            2
        );
        assert_eq!(
            job.profiles[1]
                .curves
                .iter()
                .filter(|curve| matches!(curve, KernelCurveDto::Circle { .. }))
                .count(),
            1
        );
        assert_eq!(job.profiles[1].holes.len(), 1);
        assert!(matches!(
            job.profiles[1].holes[0].curves[0],
            KernelCurveDto::Circle { .. }
        ));
    }

    #[test]
    fn body_level_features_plan_shell_transform_combine_and_split_jobs() {
        let feature_id = FeatureId(3);
        let active_features = active(&[1, 2, 3]);

        let (mut shell_document, shell_bodies) = two_body_document();
        let shell_face = shell_document.scene().bodies[0].faces[0].id;
        let shell = shell_document
            .prepare_add_body_feature(
                feature_id,
                "Shell1",
                BodyFeatureRequestDto::Shell(ShellRequest {
                    body_id: shell_bodies[0],
                    face_ids: vec![shell_face],
                    thickness: 1.5,
                    inward: true,
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            shell.jobs.last(),
            Some(KernelJobDto::Shell(KernelShellJobDto {
                target_body_id,
                inward: true,
                ..
            })) if *target_body_id == shell_bodies[0]
        ));

        let (mut move_document, move_bodies) = two_body_document();
        let moved = move_document
            .prepare_add_body_feature(
                feature_id,
                "MoveCopy1",
                BodyFeatureRequestDto::MoveCopy(MoveCopyBodyRequest {
                    body_ids: vec![move_bodies[0]],
                    translation: Point3Dto::from([12.0, -4.0, 2.0]),
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    pivot: Point3Dto::from([5.0, 5.0, 5.0]),
                    copy: false,
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            move_document.pending.as_ref().unwrap().body_features.last(),
            Some(BodyFeatureDefinitionDto::MoveCopy {
                body_ids,
                copy: false,
                result_body_ids,
                ..
            }) if body_ids == &[move_bodies[0]] && result_body_ids == body_ids
        ));
        assert!(matches!(
            moved.jobs.last(),
            Some(KernelJobDto::Transform(KernelTransformJobDto {
                source_body_ids,
                transforms,
                result_body_ids,
                ..
            })) if source_body_ids == &[move_bodies[0]]
                && result_body_ids == source_body_ids
                && matches!(transforms.as_slice(), [KernelTransformDto::Rigid { .. }])
        ));
        move_document
            .commit(
                moved.transaction_id,
                KernelSceneDto {
                    bodies: move_bodies.iter().copied().map(raw_body).collect(),
                    errors: Vec::new(),
                },
            )
            .unwrap();
        let saved_move = serde_json::to_string(move_document.body_feature_definitions()).unwrap();
        let restored_move_features: Vec<BodyFeatureDefinitionDto> =
            serde_json::from_str(&saved_move).unwrap();
        let mut restored_move = SolidDocument::restore_feature_definitions(
            move_document.definitions().to_vec(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            restored_move_features,
        )
        .unwrap();
        let replayed_move = restored_move
            .prepare_recompute(&catalog(), &active_features)
            .unwrap();
        assert!(matches!(
            replayed_move.jobs.last(),
            Some(KernelJobDto::Transform(KernelTransformJobDto {
                source_body_ids,
                result_body_ids,
                ..
            })) if source_body_ids == result_body_ids && source_body_ids == &[move_bodies[0]]
        ));

        let (mut copy_document, copy_bodies) = two_body_document();
        let copied = copy_document
            .prepare_add_body_feature(
                feature_id,
                "MoveCopy1",
                BodyFeatureRequestDto::MoveCopy(MoveCopyBodyRequest {
                    body_ids: vec![copy_bodies[0]],
                    translation: Point3Dto::from([12.0, 0.0, 0.0]),
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    pivot: Point3Dto::from([0.0, 0.0, 0.0]),
                    copy: true,
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            copied.jobs.last(),
            Some(KernelJobDto::Transform(KernelTransformJobDto {
                source_body_ids,
                result_body_ids,
                ..
            })) if source_body_ids == &[copy_bodies[0]]
                && result_body_ids.len() == 1
                && result_body_ids[0] != copy_bodies[0]
        ));

        let (mut mirror_document, mirror_bodies) = two_body_document();
        let basis = catalog()[0].basis;
        let mirror = mirror_document
            .prepare_add_body_feature(
                feature_id,
                "Mirror1",
                BodyFeatureRequestDto::Mirror(SolidMirrorRequest {
                    body_ids: vec![mirror_bodies[0]],
                    plane: nbcad_core::PlaneRef::OriginPlane {
                        plane: OriginPlane::Yz,
                    },
                    plane_basis: Some(
                        nbcad_core::PlaneRef::OriginPlane {
                            plane: OriginPlane::Yz,
                        }
                        .origin_basis()
                        .unwrap(),
                    ),
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            mirror.jobs.last(),
            Some(KernelJobDto::Transform(KernelTransformJobDto {
                source_body_ids,
                transforms,
                result_body_ids,
                ..
            })) if source_body_ids == &[mirror_bodies[0]]
                && transforms.len() == 1
                && result_body_ids.len() == 1
        ));

        let (mut rectangular_document, rectangular_bodies) = two_body_document();
        let rectangular = rectangular_document
            .prepare_add_body_feature(
                feature_id,
                "RectangularPattern1",
                BodyFeatureRequestDto::RectangularPattern(RectangularPatternRequest {
                    body_ids: vec![rectangular_bodies[0]],
                    direction: Point3Dto::from([1.0, 0.0, 0.0]),
                    spacing: 12.0,
                    count: 2,
                    second_direction: Some(Point3Dto::from([0.0, 1.0, 0.0])),
                    second_spacing: 8.0,
                    second_count: 2,
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            rectangular.jobs.last(),
            Some(KernelJobDto::Transform(KernelTransformJobDto {
                transforms,
                result_body_ids,
                ..
            })) if transforms.len() == 3 && result_body_ids.len() == 3
        ));

        let (mut circular_document, circular_bodies) = two_body_document();
        let circular = circular_document
            .prepare_add_body_feature(
                feature_id,
                "CircularPattern1",
                BodyFeatureRequestDto::CircularPattern(CircularPatternRequest {
                    body_ids: vec![circular_bodies[0]],
                    axis_origin: Point3Dto::from([0.0, 0.0, 0.0]),
                    axis_direction: Point3Dto::from([0.0, 0.0, 1.0]),
                    count: 4,
                    total_angle_deg: 360.0,
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            circular.jobs.last(),
            Some(KernelJobDto::Transform(KernelTransformJobDto {
                transforms,
                result_body_ids,
                ..
            })) if transforms.len() == 3 && result_body_ids.len() == 3
        ));

        let (mut combine_document, combine_bodies) = two_body_document();
        let combine = combine_document
            .prepare_add_body_feature(
                feature_id,
                "Combine1",
                BodyFeatureRequestDto::Combine(CombineRequest {
                    target_body_id: combine_bodies[0],
                    tool_body_ids: vec![combine_bodies[1]],
                    operation: CombineOperation::Join,
                    keep_tools: false,
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            combine.jobs.last(),
            Some(KernelJobDto::Combine(KernelCombineJobDto {
                target_body_id,
                tool_body_ids,
                ..
            })) if *target_body_id == combine_bodies[0]
                && tool_body_ids == &[combine_bodies[1]]
        ));

        let (mut split_document, split_bodies) = two_body_document();
        let split = split_document
            .prepare_add_body_feature(
                feature_id,
                "SplitBody1",
                BodyFeatureRequestDto::SplitBody(SplitBodyRequest {
                    body_id: split_bodies[0],
                    plane: nbcad_core::PlaneRef::OriginPlane {
                        plane: OriginPlane::Xy,
                    },
                    plane_basis: Some(basis),
                }),
                &catalog(),
                &active_features,
            )
            .unwrap();
        assert!(matches!(
            split.jobs.last(),
            Some(KernelJobDto::SplitBody(KernelSplitBodyJobDto {
                target_body_id,
                new_body_id,
                ..
            })) if *target_body_id == split_bodies[0]
                && !split_bodies.contains(new_body_id)
        ));
    }

    #[test]
    fn external_thread_requires_and_preserves_an_exact_cylindrical_face() {
        let mut document = SolidDocument::new();
        let base = document
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let body_id = extrude_job(&base.jobs[0]).result_body_ids[0];
        document
            .commit(
                base.transaction_id,
                KernelSceneDto {
                    bodies: vec![cylindrical_body(body_id, 3.0)],
                    errors: Vec::new(),
                },
            )
            .unwrap();
        let face_id = document.scene().bodies[0].faces[0].id;
        let mut thread = metric_thread();
        thread.designation = "M6 x 1 - 6g".to_string();
        thread.class = "6g".to_string();
        thread.tap_drill_designation = None;
        thread.representation = HoleThreadRepresentation::Simplified;

        let plan = document
            .prepare_add_body_feature(
                FeatureId(3),
                "ExternalThread1",
                BodyFeatureRequestDto::ExternalThread(ExternalThreadRequest {
                    body_id,
                    face_id,
                    thread: thread.clone(),
                    flip: true,
                }),
                &catalog(),
                &active(&[1, 2, 3]),
            )
            .unwrap();
        assert!(matches!(
            plan.jobs.last(),
            Some(KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
                target_body_id,
                face_key,
                cylinder: CylindricalSurfaceDto { radius, .. },
                thread: planned_thread,
                flip: true,
                ..
            })) if *target_body_id == body_id
                && face_key == "cylinder:0"
                && (*radius - 3.0).abs() < EPS
                && planned_thread == &thread
        ));
        let definition = document
            .pending
            .as_ref()
            .unwrap()
            .body_features
            .last()
            .unwrap();
        assert!(matches!(
            definition,
            BodyFeatureDefinitionDto::ExternalThread {
                face_key,
                cylinder: CylindricalSurfaceDto { radius, .. },
                thread: saved_thread,
                flip: true,
                ..
            } if face_key == "cylinder:0"
                && (*radius - 3.0).abs() < EPS
                && saved_thread == &thread
        ));
        let serialized = serde_json::to_string(definition).unwrap();
        let restored: BodyFeatureDefinitionDto = serde_json::from_str(&serialized).unwrap();
        assert_eq!(&restored, definition);

        let mut planar_document = SolidDocument::new();
        let planar_base = planar_document
            .prepare_add(
                FeatureId(2),
                "Extrude1",
                request(vec![0]),
                &catalog(),
                &active(&[1, 2]),
            )
            .unwrap();
        let planar_body_id = extrude_job(&planar_base.jobs[0]).result_body_ids[0];
        planar_document
            .commit(
                planar_base.transaction_id,
                KernelSceneDto {
                    bodies: vec![raw_body(planar_body_id)],
                    errors: Vec::new(),
                },
            )
            .unwrap();
        let planar_face_id = planar_document.scene().bodies[0].faces[0].id;
        assert!(matches!(
            planar_document.prepare_add_body_feature(
                FeatureId(3),
                "ExternalThread1",
                BodyFeatureRequestDto::ExternalThread(ExternalThreadRequest {
                    body_id: planar_body_id,
                    face_id: planar_face_id,
                    thread,
                    flip: false,
                }),
                &catalog(),
                &active(&[1, 2, 3]),
            ),
            Err(SolidError::InvalidExtent(message))
                if message.contains("analytic cylindrical face")
        ));
    }
}
