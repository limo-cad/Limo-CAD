use cxx::UniquePtr;
use nbcad_core::BodyAppearance;
use nbcad_export::{self, MeshExportRequest, TriangleMesh};
#[cfg(test)]
use nbcad_solid::StepOccurrencePlacementDto;
use nbcad_solid::{
    iso_metric_thread_envelope, CombineOperation, ExtrudeOperation, HoleBottomStyle, HoleExtent,
    HoleStyle, HoleThreadHand, HoleThreadRepresentation, KernelBodyDto, KernelCurveDto,
    KernelEdgeDto, KernelFaceDto, KernelFeatureErrorDto, KernelJobDto, KernelProfileDto,
    KernelSceneDto, KernelTransformDto, LoftContinuity, Point3Dto, RecomputePlanDto,
    StepExportRequest, SweepOrientation, SweepTransition, ThreadFit,
};
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Mutex;

use crate::OcctError;
use crate::{
    DrawingPolylineDto, DrawingProjectionDto, DrawingProjectionRequest, ExactInterferenceResultDto,
    PlacedBodyQueryDto,
};

#[cxx::bridge(namespace = "nbcad_occt")]
mod ffi {
    struct FfiDrawingOccurrence {
        body_id: u64,
        translation: [f64; 3],
        rotation: [f64; 4],
    }
    struct FfiJob {
        feature_id: u64,
        kind: u8,
        operation: u8,
        points: Vec<f64>,
        profile_offsets: Vec<u32>,
        /// Exact planar-face source. Zero / u32::MAX means sketch profiles.
        source_body_id: u64,
        source_face_index: u32,
        /// centroid xyz, oriented normal xyz, area, perimeter, wires, edges.
        source_face_signature: Vec<f64>,
        /// Prefix offsets into profile wires. Each region starts with its outer
        /// wire followed by zero or more inner (hole) wires.
        region_offsets: Vec<u32>,
        curve_kinds: Vec<u8>,
        curve_profile_offsets: Vec<u32>,
        curve_point_offsets: Vec<u32>,
        curve_points: Vec<f64>,
        normal_x: f64,
        normal_y: f64,
        normal_z: f64,
        start_offset: f64,
        end_offset: f64,
        taper_angle_deg: f64,
        axis_origin_x: f64,
        axis_origin_y: f64,
        axis_origin_z: f64,
        axis_direction_x: f64,
        axis_direction_y: f64,
        axis_direction_z: f64,
        angle_rad: f64,
        path_curve_kinds: Vec<u8>,
        path_curve_point_offsets: Vec<u32>,
        path_curve_points: Vec<f64>,
        guide_curve_kinds: Vec<u8>,
        guide_curve_point_offsets: Vec<u32>,
        guide_curve_points: Vec<f64>,
        orientation: u8,
        transition: u8,
        continuity: u8,
        force_c1: bool,
        ruled: bool,
        edge_indices: Vec<u32>,
        face_indices: Vec<u32>,
        transform_kinds: Vec<u8>,
        transform_values: Vec<f64>,
        radius: f64,
        diameter: f64,
        secondary_diameter: f64,
        secondary_depth: f64,
        hole_angle_deg: f64,
        hole_style: u8,
        drill_point_angle_deg: f64,
        hole_bottom_style: u8,
        thread_mode: u8,
        thread_nominal_diameter: f64,
        /// Finished ISO class diameters used by the B-rep, not tap-drill data.
        thread_major_diameter: f64,
        thread_pitch_diameter: f64,
        thread_minor_diameter: f64,
        thread_pitch: f64,
        thread_depth: f64,
        thread_left_hand: bool,
        through_all: bool,
        inward: bool,
        keep_tools: bool,
        step_data: Vec<u8>,
        target_body_ids: Vec<u64>,
        result_body_ids: Vec<u64>,
    }

    struct FfiMesh {
        body_id: u64,
        topology_signature: String,
        positions: Vec<f32>,
        normals: Vec<f32>,
        indices: Vec<u32>,
        face_first_indices: Vec<u32>,
        face_index_counts: Vec<u32>,
        /// Per face: valid flag then origin/u/v/normal (13 f64 values).
        face_plane_data: Vec<f64>,
        /// Per face: valid, centroid xyz, area, perimeter, wire count, edge count.
        face_signature_data: Vec<f64>,
        /// Per face: valid, axis origin xyz, axis direction xyz,
        /// reference direction xyz, radius (11 f64 values).
        face_cylinder_data: Vec<f64>,
        /// Prefix offsets into `edge_points`, measured in 3D points.
        edge_point_offsets: Vec<u32>,
        /// Flat xyz edge polyline coordinates.
        edge_points: Vec<f64>,
        /// Per-edge topology classification for refinement tools.
        edge_refinable: Vec<u8>,
        /// Per edge: valid, center xyz, normal xyz, reference xyz, radius,
        /// closed flag (12 f64 values).
        edge_circle_data: Vec<f64>,
    }

    struct FfiDrawingProjection {
        visible_offsets: Vec<u32>,
        visible_points: Vec<f64>,
        hidden_offsets: Vec<u32>,
        hidden_points: Vec<f64>,
        section_offsets: Vec<u32>,
        section_points: Vec<f64>,
    }

    struct FfiInterferenceResult {
        minimum_clearance_mm: f64,
        overlap_volume_mm3: f64,
        closest_point_a_x: f64,
        closest_point_a_y: f64,
        closest_point_a_z: f64,
        closest_point_b_x: f64,
        closest_point_b_y: f64,
        closest_point_b_z: f64,
    }

    unsafe extern "C++" {
        include!("shim.hpp");

        type Kernel;
        fn new_kernel() -> UniquePtr<Kernel>;
        fn reset(self: Pin<&mut Kernel>);
        fn apply_job(self: Pin<&mut Kernel>, job: &FfiJob) -> Result<()>;
        fn body_ids(self: &Kernel) -> Vec<u64>;
        fn mesh(self: &Kernel, body_id: u64) -> Result<FfiMesh>;
        fn mesh_with_deflection(
            self: &Kernel,
            body_id: u64,
            linear_deflection: f64,
            angular_deflection: f64,
        ) -> Result<FfiMesh>;
        fn export_step(
            self: &Kernel,
            body_ids: &Vec<u64>,
            thread_metadata_hex: &str,
            occurrence_placements_hex: &str,
        ) -> Result<Vec<u8>>;
        fn drawing_projection(
            self: &Kernel,
            body_ids: &Vec<u64>,
            occurrences: &Vec<FfiDrawingOccurrence>,
            assembly_scope: bool,
            direction_x: f64,
            direction_y: f64,
            direction_z: f64,
            up_x: f64,
            up_y: f64,
            up_z: f64,
            include_hidden: bool,
            include_tangent_edges: bool,
            deflection: f64,
            has_section_plane: bool,
            section_point_x: f64,
            section_point_y: f64,
            section_point_z: f64,
            section_normal_x: f64,
            section_normal_y: f64,
            section_normal_z: f64,
            has_section_depth: bool,
            section_depth: f64,
        ) -> Result<FfiDrawingProjection>;
        fn exact_interference(
            self: &Kernel,
            body_a: u64,
            translation_a_x: f64,
            translation_a_y: f64,
            translation_a_z: f64,
            rotation_a_x: f64,
            rotation_a_y: f64,
            rotation_a_z: f64,
            rotation_a_w: f64,
            body_b: u64,
            translation_b_x: f64,
            translation_b_y: f64,
            translation_b_z: f64,
            rotation_b_x: f64,
            rotation_b_y: f64,
            rotation_b_z: f64,
            rotation_b_w: f64,
        ) -> Result<FfiInterferenceResult>;
    }
}

// The C++ Kernel is never accessed concurrently: both the native shell and
// tests serialize all calls through `&mut OcctKernel` (the shell additionally
// holds it inside a Mutex). OCCT objects owned by this instance never escape
// the bridge, so moving the opaque pointer between command threads is safe.
unsafe impl Send for ffi::Kernel {}

pub struct OcctKernel {
    inner: UniquePtr<ffi::Kernel>,
    /// Only a fully successful replay may seed the next append. The jobs
    /// include all resolved geometry inputs, not just feature IDs/revisions.
    successful_jobs: Option<Vec<KernelJobDto>>,
    /// Exact projection is independent of paper styling and export format.
    /// Full requests include authoritative occurrence poses and section intent.
    projection_cache: Mutex<VecDeque<(DrawingProjectionRequest, DrawingProjectionDto)>>,
    #[cfg(test)]
    last_applied_jobs: usize,
    #[cfg(test)]
    projection_calculations: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for OcctKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcctKernel").finish_non_exhaustive()
    }
}

impl OcctKernel {
    pub fn new() -> Result<Self, OcctError> {
        let inner = ffi::new_kernel();
        if inner.is_null() {
            return Err(OcctError("OCCT kernel allocation failed".to_string()));
        }
        Ok(Self {
            inner,
            successful_jobs: None,
            projection_cache: Mutex::new(VecDeque::new()),
            #[cfg(test)]
            last_applied_jobs: 0,
            #[cfg(test)]
            projection_calculations: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn recompute(&mut self, plan: &RecomputePlanDto) -> Result<KernelSceneDto, OcctError> {
        // Invalidate before touching the kernel. A conversion, native Boolean,
        // or tessellation failure can leave a partial native state; the next
        // request must rebuild it even when that request matches an old plan.
        let previous = self.successful_jobs.take();
        if !plan.errors.is_empty() || previous.as_ref() != Some(&plan.jobs) {
            self.projection_cache.get_mut().unwrap().clear();
        }
        let reused = previous
            .as_ref()
            .filter(|jobs| plan.errors.is_empty() && plan.jobs.starts_with(jobs))
            .map_or(0, Vec::len);
        let mut pinned = self.inner.pin_mut();
        if reused == 0 {
            pinned.as_mut().reset();
        }
        #[cfg(test)]
        {
            self.last_applied_jobs = 0;
        }
        let mut errors = plan.errors.clone();
        for job in &plan.jobs[reused..] {
            let ffi_job = match to_ffi_job(job) {
                Ok(job) => job,
                Err(error) => {
                    errors.push(KernelFeatureErrorDto {
                        feature_id: job.feature_id(),
                        message: error.to_string(),
                    });
                    break;
                }
            };
            #[cfg(test)]
            {
                self.last_applied_jobs += 1;
            }
            if let Err(error) = pinned.as_mut().apply_job(&ffi_job) {
                errors.push(KernelFeatureErrorDto {
                    feature_id: job.feature_id(),
                    message: error.to_string(),
                });
                break;
            }
        }

        let body_ids = self
            .inner
            .as_ref()
            .ok_or_else(|| OcctError("OCCT kernel was released".to_string()))?
            .body_ids();
        let mut bodies = Vec::with_capacity(body_ids.len());
        for body_id in body_ids {
            let raw = self
                .inner
                .as_ref()
                .ok_or_else(|| OcctError("OCCT kernel was released".to_string()))?
                .mesh(body_id)
                .map_err(|error| OcctError(error.to_string()))?;
            bodies.push(from_ffi_mesh(raw)?);
        }
        if errors.is_empty() {
            self.successful_jobs = Some(plan.jobs.clone());
        }
        Ok(KernelSceneDto { bodies, errors })
    }

    /// Serialize selected (or all) live B-reps as an AP242 STEP exchange
    /// file. Tessellated meshes are never used for export.
    pub fn export_step(&self, request: &StepExportRequest) -> Result<Vec<u8>, OcctError> {
        let body_ids = request.body_ids.iter().map(|id| id.0).collect::<Vec<_>>();
        let thread_metadata_json =
            serde_json::to_string(&request.thread_metadata).map_err(|error| {
                OcctError(format!("could not serialize STEP thread metadata: {error}"))
            })?;
        let mut thread_metadata_hex = String::with_capacity(thread_metadata_json.len() * 2);
        for byte in thread_metadata_json.bytes() {
            write!(&mut thread_metadata_hex, "{byte:02x}")
                .map_err(|error| OcctError(format!("could not encode STEP metadata: {error}")))?;
        }
        let mut occurrence_placements_hex = String::with_capacity(request.occurrences.len() * 160);
        for occurrence in &request.occurrences {
            let values = occurrence
                .translation
                .iter()
                .chain(occurrence.rotation.iter())
                .copied()
                .collect::<Vec<_>>();
            if values.iter().any(|value| !value.is_finite()) {
                return Err(OcctError(format!(
                    "STEP occurrence {} has a non-finite placement",
                    occurrence.occurrence_id
                )));
            }
            let magnitude = occurrence
                .rotation
                .iter()
                .map(|value| value * value)
                .sum::<f64>();
            if magnitude <= 1.0e-24 {
                return Err(OcctError(format!(
                    "STEP occurrence {} has a degenerate rotation",
                    occurrence.occurrence_id
                )));
            }
            for value in [
                occurrence.body_id.0,
                occurrence.occurrence_id,
                occurrence.component_id,
            ] {
                for byte in value.to_le_bytes() {
                    write!(&mut occurrence_placements_hex, "{byte:02x}").map_err(|error| {
                        OcctError(format!("could not encode STEP occurrence id: {error}"))
                    })?;
                }
            }
            for value in occurrence
                .translation
                .iter()
                .chain(occurrence.rotation.iter())
            {
                for byte in value.to_le_bytes() {
                    write!(&mut occurrence_placements_hex, "{byte:02x}").map_err(|error| {
                        OcctError(format!("could not encode STEP occurrence pose: {error}"))
                    })?;
                }
            }
        }
        self.inner
            .as_ref()
            .ok_or_else(|| OcctError("OCCT kernel was released".to_string()))?
            .export_step(&body_ids, &thread_metadata_hex, &occurrence_placements_hex)
            .map_err(|error| OcctError(error.to_string()))
    }

    /// Generate exact OCCT hidden-line projection curves from the active
    /// B-reps. This deliberately bypasses viewport tessellation.
    pub fn drawing_projection(
        &self,
        request: &DrawingProjectionRequest,
    ) -> Result<DrawingProjectionDto, OcctError> {
        validate_projection_basis(request.direction, request.up)?;
        if !request.deflection.is_finite() || request.deflection <= 0.0 {
            return Err(OcctError(
                "drawing projection deflection must be positive and finite".to_string(),
            ));
        }
        if let Some(section) = &request.section_plane {
            if section
                .point
                .iter()
                .chain(section.normal.iter())
                .any(|value| !value.is_finite())
                || section
                    .normal
                    .iter()
                    .map(|value| value * value)
                    .sum::<f64>()
                    < 1.0e-12
            {
                return Err(OcctError(
                    "drawing section plane must contain a finite point and non-zero normal"
                        .to_string(),
                ));
            }
            if section
                .depth
                .is_some_and(|depth| !depth.is_finite() || depth <= 0.0)
            {
                return Err(OcctError(
                    "drawing section depth must be a positive finite model distance".to_string(),
                ));
            }
        }
        let assembly_scope = request.scope == nbcad_sketch::DrawingViewScope::Assembly;
        if assembly_scope
            && request
                .resolved_occurrences
                .as_ref()
                .is_none_or(Vec::is_empty)
        {
            return Err(OcctError(
                "Assembly drawing requires nonempty host-resolved occurrences".into(),
            ));
        }
        let occurrences = request
            .resolved_occurrences
            .iter()
            .flatten()
            .map(|pose| ffi::FfiDrawingOccurrence {
                body_id: pose.body_id.0,
                translation: pose.translation,
                rotation: pose.rotation,
            })
            .collect::<Vec<_>>();
        let body_ids = request.body_ids.iter().map(|id| id.0).collect::<Vec<_>>();
        if let Some((_, projection)) = self
            .projection_cache
            .lock()
            .unwrap()
            .iter()
            .find(|(key, _)| key == request)
        {
            return Ok(projection.clone());
        }
        #[cfg(test)]
        self.projection_calculations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let raw = self
            .inner
            .as_ref()
            .ok_or_else(|| OcctError("OCCT kernel was released".to_string()))?
            .drawing_projection(
                &body_ids,
                &occurrences,
                assembly_scope,
                request.direction[0],
                request.direction[1],
                request.direction[2],
                request.up[0],
                request.up[1],
                request.up[2],
                request.include_hidden,
                request.include_tangent_edges,
                request.deflection.clamp(1.0e-4, 10.0),
                request.section_plane.is_some(),
                request
                    .section_plane
                    .as_ref()
                    .map_or(0.0, |plane| plane.point[0]),
                request
                    .section_plane
                    .as_ref()
                    .map_or(0.0, |plane| plane.point[1]),
                request
                    .section_plane
                    .as_ref()
                    .map_or(0.0, |plane| plane.point[2]),
                request
                    .section_plane
                    .as_ref()
                    .map_or(0.0, |plane| plane.normal[0]),
                request
                    .section_plane
                    .as_ref()
                    .map_or(0.0, |plane| plane.normal[1]),
                request
                    .section_plane
                    .as_ref()
                    .map_or(1.0, |plane| plane.normal[2]),
                request
                    .section_plane
                    .as_ref()
                    .and_then(|plane| plane.depth)
                    .is_some(),
                request
                    .section_plane
                    .as_ref()
                    .and_then(|plane| plane.depth)
                    .unwrap_or(0.0),
            )
            .map_err(|error| OcctError(error.to_string()))?;
        let projection = projection_from_ffi(raw)?;
        // Bound retained work both by view count and actual linework size.
        let weight = |projection: &DrawingProjectionDto| {
            projection
                .visible
                .iter()
                .chain(&projection.hidden)
                .chain(&projection.section)
                .map(|line| line.points.len())
                .sum::<usize>()
        };
        let points = weight(&projection);
        const MAX_POINTS: usize = 500_000;
        if points <= MAX_POINTS {
            let mut cache = self.projection_cache.lock().unwrap();
            let mut retained = cache.iter().map(|(_, value)| weight(value)).sum::<usize>();
            while cache.len() >= 16 || retained + points > MAX_POINTS {
                if let Some((_, oldest)) = cache.pop_front() {
                    retained -= weight(&oldest);
                } else {
                    break;
                }
            }
            cache.push_back((request.clone(), projection.clone()));
        }
        Ok(projection)
    }

    pub fn exact_interference(
        &self,
        a: PlacedBodyQueryDto,
        b: PlacedBodyQueryDto,
    ) -> Result<ExactInterferenceResultDto, OcctError> {
        let raw = self
            .inner
            .as_ref()
            .ok_or_else(|| OcctError("OCCT kernel was released".to_string()))?
            .exact_interference(
                a.body_id.0,
                a.translation[0],
                a.translation[1],
                a.translation[2],
                a.rotation[0],
                a.rotation[1],
                a.rotation[2],
                a.rotation[3],
                b.body_id.0,
                b.translation[0],
                b.translation[1],
                b.translation[2],
                b.rotation[0],
                b.rotation[1],
                b.rotation[2],
                b.rotation[3],
            )
            .map_err(|error| OcctError(error.to_string()))?;
        Ok(ExactInterferenceResultDto {
            minimum_clearance_mm: raw.minimum_clearance_mm,
            overlap_volume_mm3: raw.overlap_volume_mm3,
            closest_point_a: [
                raw.closest_point_a_x,
                raw.closest_point_a_y,
                raw.closest_point_a_z,
            ],
            closest_point_b: [
                raw.closest_point_b_x,
                raw.closest_point_b_y,
                raw.closest_point_b_z,
            ],
        })
    }

    /// Tessellate selected (or all) live bodies with configurable deflection.
    pub fn tessellate_bodies(
        &self,
        request: &MeshExportRequest,
    ) -> Result<Vec<TriangleMesh>, OcctError> {
        let kernel = self
            .inner
            .as_ref()
            .ok_or_else(|| OcctError("OCCT kernel was released".to_string()))?;
        let available = kernel.body_ids();
        if available.is_empty() {
            return Err(OcctError(
                "There are no active bodies to export.".to_string(),
            ));
        }
        let selected: Vec<u64> = if request.body_ids.is_empty() {
            available
        } else {
            let mut ids = request.body_ids.iter().map(|id| id.0).collect::<Vec<_>>();
            ids.sort_unstable();
            ids.dedup();
            for body_id in &ids {
                if !available.contains(body_id) {
                    return Err(OcctError(format!("Selected body {body_id} is not active.")));
                }
            }
            ids
        };
        let mut meshes = Vec::with_capacity(selected.len());
        for body_id in selected {
            let raw = kernel
                .mesh_with_deflection(
                    body_id,
                    request.linear_deflection,
                    request.angular_deflection,
                )
                .map_err(|error| OcctError(error.to_string()))?;
            let body = from_ffi_mesh(raw)?;
            meshes.push(TriangleMesh::from_kernel_body(
                &body,
                format!("Body{}", body_id),
            ));
        }
        Ok(meshes)
    }

    pub fn export_stl(&self, request: &MeshExportRequest) -> Result<Vec<u8>, OcctError> {
        let meshes = self.tessellate_bodies(request)?;
        nbcad_export::write_stl(&meshes).map_err(|error| OcctError(error.to_string()))
    }

    pub fn export_3mf(
        &self,
        request: &MeshExportRequest,
        appearances: &[BodyAppearance],
    ) -> Result<Vec<u8>, OcctError> {
        let meshes = self.tessellate_bodies(request)?;
        nbcad_export::ExportFacade::export_3mf(&meshes, appearances, request)
            .map_err(|error| OcctError(error.to_string()))
    }
}

fn validate_projection_basis(direction: [f64; 3], up: [f64; 3]) -> Result<(), OcctError> {
    if direction
        .iter()
        .chain(up.iter())
        .any(|value| !value.is_finite())
    {
        return Err(OcctError(
            "drawing projection basis contains non-finite values".to_string(),
        ));
    }
    let length_sq = |value: [f64; 3]| {
        value
            .iter()
            .map(|component| component * component)
            .sum::<f64>()
    };
    let cross = [
        up[1] * direction[2] - up[2] * direction[1],
        up[2] * direction[0] - up[0] * direction[2],
        up[0] * direction[1] - up[1] * direction[0],
    ];
    if length_sq(direction) < 1.0e-12
        || length_sq(up) < 1.0e-12
        || length_sq(cross) < length_sq(direction) * length_sq(up) * 1.0e-12
    {
        return Err(OcctError(
            "drawing projection direction and up vectors are degenerate".to_string(),
        ));
    }
    Ok(())
}

fn projection_from_ffi(raw: ffi::FfiDrawingProjection) -> Result<DrawingProjectionDto, OcctError> {
    let visible = projection_polylines(&raw.visible_offsets, &raw.visible_points)?;
    let hidden = projection_polylines(&raw.hidden_offsets, &raw.hidden_points)?;
    let section = projection_polylines(&raw.section_offsets, &raw.section_points)?;
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for point in visible
        .iter()
        .chain(hidden.iter())
        .chain(section.iter())
        .flat_map(|polyline| polyline.points.iter())
    {
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    }
    if !bounds.iter().all(|value| value.is_finite()) {
        bounds = [0.0; 4];
    }
    Ok(DrawingProjectionDto {
        topology_signatures: Default::default(),
        visible,
        hidden,
        anchors: Vec::new(),
        circles: Vec::new(),
        section,
        bounds,
    })
}

fn projection_polylines(
    offsets: &[u32],
    points: &[f64],
) -> Result<Vec<DrawingPolylineDto>, OcctError> {
    if offsets.is_empty()
        || offsets[0] != 0
        || offsets.last().copied().unwrap_or(0) as usize * 2 != points.len()
    {
        return Err(OcctError(
            "OCCT returned malformed drawing projection buffers".to_string(),
        ));
    }
    let mut result = Vec::with_capacity(offsets.len().saturating_sub(1));
    for window in offsets.windows(2) {
        let begin = window[0] as usize;
        let end = window[1] as usize;
        if end < begin || end * 2 > points.len() || end - begin < 2 {
            return Err(OcctError(
                "OCCT returned a malformed drawing polyline".to_string(),
            ));
        }
        result.push(DrawingPolylineDto {
            points: (begin..end)
                .map(|index| [points[index * 2], points[index * 2 + 1]])
                .collect(),
        });
    }
    Ok(result)
}

struct ProfileBuffers {
    points: Vec<f64>,
    profile_offsets: Vec<u32>,
    region_offsets: Vec<u32>,
    curve_kinds: Vec<u8>,
    curve_profile_offsets: Vec<u32>,
    curve_point_offsets: Vec<u32>,
    curve_points: Vec<f64>,
}

fn profile_buffers(profiles: &[KernelProfileDto]) -> ProfileBuffers {
    let mut buffers = ProfileBuffers {
        points: Vec::new(),
        profile_offsets: Vec::with_capacity(profiles.len() + 1),
        region_offsets: Vec::with_capacity(profiles.len() + 1),
        curve_kinds: Vec::new(),
        curve_profile_offsets: Vec::with_capacity(profiles.len() + 1),
        curve_point_offsets: vec![0],
        curve_points: Vec::new(),
    };
    buffers.profile_offsets.push(0);
    buffers.region_offsets.push(0);
    buffers.curve_profile_offsets.push(0);
    fn append_profile(buffers: &mut ProfileBuffers, profile: &KernelProfileDto) {
        for point in &profile.points {
            buffers.points.extend([point.x, point.y, point.z]);
        }
        buffers
            .profile_offsets
            .push((buffers.points.len() / 3) as u32);

        if profile.curves.is_empty() {
            for (start, end) in profile
                .points
                .iter()
                .zip(profile.points.iter().cycle().skip(1))
                .take(profile.points.len())
            {
                push_curve(buffers, 0, &[*start, *end]);
            }
        } else {
            for curve in &profile.curves {
                match curve {
                    KernelCurveDto::Line { start, end, .. } => {
                        push_curve(buffers, 0, &[*start, *end]);
                    }
                    KernelCurveDto::Arc {
                        start, mid, end, ..
                    } => {
                        push_curve(buffers, 1, &[*start, *mid, *end]);
                    }
                    KernelCurveDto::Circle {
                        center,
                        axis_point,
                        normal,
                        ..
                    } => {
                        push_curve(buffers, 2, &[*center, *axis_point, *normal]);
                    }
                    KernelCurveDto::Polyline { points, .. } => {
                        push_curve(buffers, 3, points);
                    }
                }
            }
        }
        buffers
            .curve_profile_offsets
            .push(buffers.curve_kinds.len() as u32);
    }
    for profile in profiles {
        append_profile(&mut buffers, profile);
        for hole in &profile.holes {
            append_profile(&mut buffers, hole);
        }
        buffers
            .region_offsets
            .push((buffers.profile_offsets.len() - 1) as u32);
    }
    buffers
}

fn push_curve(buffers: &mut ProfileBuffers, kind: u8, points: &[Point3Dto]) {
    buffers.curve_kinds.push(kind);
    for point in points {
        buffers.curve_points.extend([point.x, point.y, point.z]);
    }
    buffers
        .curve_point_offsets
        .push((buffers.curve_points.len() / 3) as u32);
}

struct CurveBuffers {
    kinds: Vec<u8>,
    point_offsets: Vec<u32>,
    points: Vec<f64>,
}

fn curve_buffers(curves: &[KernelCurveDto]) -> CurveBuffers {
    let mut buffers = CurveBuffers {
        kinds: Vec::with_capacity(curves.len()),
        point_offsets: vec![0],
        points: Vec::new(),
    };
    for curve in curves {
        let (kind, points): (u8, Vec<Point3Dto>) = match curve {
            KernelCurveDto::Line { start, end, .. } => (0, vec![*start, *end]),
            KernelCurveDto::Arc {
                start, mid, end, ..
            } => (1, vec![*start, *mid, *end]),
            KernelCurveDto::Circle {
                center,
                axis_point,
                normal,
                ..
            } => (2, vec![*center, *axis_point, *normal]),
            KernelCurveDto::Polyline { points, .. } => (3, points.clone()),
        };
        buffers.kinds.push(kind);
        for point in points {
            buffers.points.extend([point.x, point.y, point.z]);
        }
        buffers
            .point_offsets
            .push((buffers.points.len() / 3) as u32);
    }
    buffers
}

fn empty_ffi_job(feature_id: u64, kind: u8) -> ffi::FfiJob {
    ffi::FfiJob {
        feature_id,
        kind,
        operation: 0,
        points: Vec::new(),
        profile_offsets: vec![0],
        source_body_id: 0,
        source_face_index: u32::MAX,
        source_face_signature: Vec::new(),
        region_offsets: vec![0],
        curve_kinds: Vec::new(),
        curve_profile_offsets: vec![0],
        curve_point_offsets: vec![0],
        curve_points: Vec::new(),
        normal_x: 0.0,
        normal_y: 0.0,
        normal_z: 0.0,
        start_offset: 0.0,
        end_offset: 0.0,
        taper_angle_deg: 0.0,
        axis_origin_x: 0.0,
        axis_origin_y: 0.0,
        axis_origin_z: 0.0,
        axis_direction_x: 0.0,
        axis_direction_y: 0.0,
        axis_direction_z: 0.0,
        angle_rad: 0.0,
        path_curve_kinds: Vec::new(),
        path_curve_point_offsets: vec![0],
        path_curve_points: Vec::new(),
        guide_curve_kinds: Vec::new(),
        guide_curve_point_offsets: vec![0],
        guide_curve_points: Vec::new(),
        orientation: 0,
        transition: 0,
        continuity: 1,
        force_c1: false,
        ruled: false,
        edge_indices: Vec::new(),
        face_indices: Vec::new(),
        transform_kinds: Vec::new(),
        transform_values: Vec::new(),
        radius: 0.0,
        diameter: 0.0,
        secondary_diameter: 0.0,
        secondary_depth: 0.0,
        hole_angle_deg: 0.0,
        hole_style: 0,
        drill_point_angle_deg: 0.0,
        hole_bottom_style: 0,
        thread_mode: 0,
        thread_nominal_diameter: 0.0,
        thread_major_diameter: 0.0,
        thread_pitch_diameter: 0.0,
        thread_minor_diameter: 0.0,
        thread_pitch: 0.0,
        thread_depth: 0.0,
        thread_left_hand: false,
        through_all: false,
        inward: false,
        keep_tools: false,
        step_data: Vec::new(),
        target_body_ids: Vec::new(),
        result_body_ids: Vec::new(),
    }
}

fn set_profiles(job: &mut ffi::FfiJob, buffers: ProfileBuffers) {
    job.points = buffers.points;
    job.profile_offsets = buffers.profile_offsets;
    job.region_offsets = buffers.region_offsets;
    job.curve_kinds = buffers.curve_kinds;
    job.curve_profile_offsets = buffers.curve_profile_offsets;
    job.curve_point_offsets = buffers.curve_point_offsets;
    job.curve_points = buffers.curve_points;
}

fn set_path(job: &mut ffi::FfiJob, curves: &[KernelCurveDto]) {
    let buffers = curve_buffers(curves);
    job.path_curve_kinds = buffers.kinds;
    job.path_curve_point_offsets = buffers.point_offsets;
    job.path_curve_points = buffers.points;
}

fn set_guide(job: &mut ffi::FfiJob, curves: &[KernelCurveDto]) {
    let buffers = curve_buffers(curves);
    job.guide_curve_kinds = buffers.kinds;
    job.guide_curve_point_offsets = buffers.point_offsets;
    job.guide_curve_points = buffers.points;
}

fn to_ffi_job(job: &KernelJobDto) -> Result<ffi::FfiJob, OcctError> {
    Ok(match job {
        KernelJobDto::Extrude(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 0);
            job.operation = operation_code(source.operation);
            set_profiles(&mut job, profile_buffers(&source.profiles));
            if let Some(face) = &source.source_face {
                job.source_body_id = face.body_id.0;
                job.source_face_index = face
                    .face_key
                    .strip_prefix("face:")
                    .and_then(|value| value.parse::<u32>().ok())
                    .ok_or_else(|| {
                        OcctError(format!(
                            "invalid planar-face topology key '{}'",
                            face.face_key
                        ))
                    })?;
                job.source_face_signature = vec![
                    face.signature.centroid.x,
                    face.signature.centroid.y,
                    face.signature.centroid.z,
                    face.signature.normal.x,
                    face.signature.normal.y,
                    face.signature.normal.z,
                    face.signature.area,
                    face.signature.perimeter,
                    f64::from(face.signature.wire_count),
                    f64::from(face.signature.edge_count),
                ];
            }
            job.normal_x = source.normal.x;
            job.normal_y = source.normal.y;
            job.normal_z = source.normal.z;
            job.start_offset = source.start_offset;
            job.end_offset = source.end_offset;
            job.taper_angle_deg = source.taper_angle_deg;
            job.target_body_ids = source.target_body_ids.iter().map(|id| id.0).collect();
            job.result_body_ids = source.result_body_ids.iter().map(|id| id.0).collect();
            job
        }
        KernelJobDto::Revolve(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 1);
            job.operation = operation_code(source.operation);
            set_profiles(&mut job, profile_buffers(&source.profiles));
            job.axis_origin_x = source.axis_origin.x;
            job.axis_origin_y = source.axis_origin.y;
            job.axis_origin_z = source.axis_origin.z;
            job.axis_direction_x = source.axis_direction.x;
            job.axis_direction_y = source.axis_direction.y;
            job.axis_direction_z = source.axis_direction.z;
            job.angle_rad = source.angle_rad;
            job.target_body_ids = source.target_body_ids.iter().map(|id| id.0).collect();
            job.result_body_ids = source.result_body_ids.iter().map(|id| id.0).collect();
            job
        }
        KernelJobDto::Sweep(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 2);
            job.operation = operation_code(source.operation);
            set_profiles(
                &mut job,
                profile_buffers(std::slice::from_ref(&source.profile)),
            );
            set_path(&mut job, &source.path);
            set_guide(&mut job, &source.guide_rail);
            job.orientation = match source.orientation {
                SweepOrientation::CorrectedFrenet => 0,
                SweepOrientation::Frenet => 1,
                SweepOrientation::Fixed => 2,
            };
            job.transition = match source.transition {
                SweepTransition::Transformed => 0,
                SweepTransition::RightCorner => 1,
                SweepTransition::RoundCorner => 2,
            };
            job.force_c1 = source.force_c1;
            job.target_body_ids = source.target_body_ids.iter().map(|id| id.0).collect();
            job.result_body_ids = source.result_body_ids.iter().map(|id| id.0).collect();
            job
        }
        KernelJobDto::Loft(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 3);
            job.operation = operation_code(source.operation);
            set_profiles(&mut job, profile_buffers(&source.sections));
            set_path(&mut job, &source.centerline);
            set_guide(&mut job, &source.guide_rail);
            job.ruled = source.ruled;
            job.continuity = match source.continuity {
                LoftContinuity::G0 => 0,
                LoftContinuity::G1 => 1,
                LoftContinuity::G2 => 2,
            };
            job.target_body_ids = source.target_body_ids.iter().map(|id| id.0).collect();
            job.result_body_ids = source.result_body_ids.iter().map(|id| id.0).collect();
            job
        }
        KernelJobDto::Rib(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 4);
            job.operation = operation_code(source.operation);
            set_profiles(&mut job, profile_buffers(&source.profiles));
            job.normal_x = source.normal.x;
            job.normal_y = source.normal.y;
            job.normal_z = source.normal.z;
            job.start_offset = source.start_offset;
            job.end_offset = source.end_offset;
            job.target_body_ids = source.target_body_ids.iter().map(|id| id.0).collect();
            job.result_body_ids = source.result_body_ids.iter().map(|id| id.0).collect();
            job
        }
        KernelJobDto::Fillet(source) => refinement_ffi_job(
            source.feature_id.0,
            5,
            source.target_body_id.0,
            edge_indices(&source.edge_keys),
            source.radius,
        ),
        KernelJobDto::Chamfer(source) => refinement_ffi_job(
            source.feature_id.0,
            6,
            source.target_body_id.0,
            edge_indices(&source.edge_keys),
            source.distance,
        ),
        KernelJobDto::Hole(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 7);
            job.operation = 2;
            job.end_offset = match source.extent {
                HoleExtent::Distance { depth } => depth,
                HoleExtent::ThroughAll => 1_000_000.0,
            };
            job.axis_origin_x = source.center.x;
            job.axis_origin_y = source.center.y;
            job.axis_origin_z = source.center.z;
            job.axis_direction_x = source.direction.x;
            job.axis_direction_y = source.direction.y;
            job.axis_direction_z = source.direction.z;
            job.diameter = source.diameter;
            match source.style {
                HoleStyle::Simple => {}
                HoleStyle::Counterbore => {
                    job.hole_style = 1;
                    job.secondary_diameter = source.counterbore_diameter;
                    job.secondary_depth = source.counterbore_depth;
                }
                HoleStyle::Countersink => {
                    job.hole_style = 2;
                    job.secondary_diameter = source.countersink_diameter;
                    job.hole_angle_deg = source.countersink_angle_deg;
                }
            }
            job.hole_bottom_style = match source.bottom_style {
                HoleBottomStyle::Flat => 0,
                HoleBottomStyle::DrillPoint => 1,
            };
            job.drill_point_angle_deg = source.drill_point_angle_deg;
            if let Some(thread) = &source.thread {
                job.thread_mode = match thread.representation {
                    HoleThreadRepresentation::Simplified => 1,
                    HoleThreadRepresentation::Modeled => 2,
                };
                job.thread_nominal_diameter = thread.nominal_diameter;
                if let Some(limits) =
                    iso_metric_thread_envelope(thread, ThreadFit::Internal).map_err(OcctError)?
                {
                    job.thread_major_diameter = limits.modeled_major;
                    job.thread_pitch_diameter = limits.modeled_pitch;
                    job.thread_minor_diameter = limits.modeled_minor;
                } else {
                    job.thread_major_diameter = thread.nominal_diameter;
                    job.thread_pitch_diameter =
                        thread.nominal_diameter - 0.649_519_052_838_329 * thread.pitch;
                    job.thread_minor_diameter = source.diameter;
                }
                job.thread_pitch = thread.pitch;
                job.thread_depth = thread.depth.unwrap_or(0.0);
                job.thread_left_hand = thread.hand == HoleThreadHand::Left;
            }
            job.through_all = matches!(source.extent, HoleExtent::ThroughAll);
            job.target_body_ids = vec![source.target_body_id.0];
            job.result_body_ids = vec![source.target_body_id.0];
            job
        }
        KernelJobDto::ExternalThread(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 13);
            job.operation = 2;
            job.face_indices = face_indices(std::slice::from_ref(&source.face_key));
            job.axis_origin_x = source.cylinder.origin.x;
            job.axis_origin_y = source.cylinder.origin.y;
            job.axis_origin_z = source.cylinder.origin.z;
            job.axis_direction_x = source.cylinder.axis.x;
            job.axis_direction_y = source.cylinder.axis.y;
            job.axis_direction_z = source.cylinder.axis.z;
            job.diameter = source.cylinder.radius * 2.0;
            job.thread_mode = match source.thread.representation {
                HoleThreadRepresentation::Simplified => 1,
                HoleThreadRepresentation::Modeled => 2,
            };
            job.thread_nominal_diameter = source.thread.nominal_diameter;
            if let Some(limits) = iso_metric_thread_envelope(&source.thread, ThreadFit::External)
                .map_err(OcctError)?
            {
                job.thread_major_diameter = limits.modeled_major;
                job.thread_pitch_diameter = limits.modeled_pitch;
                job.thread_minor_diameter = limits.modeled_minor;
            } else {
                job.thread_major_diameter = source.thread.nominal_diameter;
                job.thread_pitch_diameter =
                    source.thread.nominal_diameter - 0.649_519_052_838_329 * source.thread.pitch;
                job.thread_minor_diameter =
                    source.thread.nominal_diameter - 1.226_869_322_027_954 * source.thread.pitch;
            }
            job.thread_pitch = source.thread.pitch;
            job.thread_depth = source.thread.depth.unwrap_or(0.0);
            job.thread_left_hand = source.thread.hand == HoleThreadHand::Left;
            job.through_all = source.thread.depth.is_none();
            job.inward = source.flip;
            job.target_body_ids = vec![source.target_body_id.0];
            job.result_body_ids = vec![source.target_body_id.0];
            job
        }
        KernelJobDto::Shell(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 8);
            job.target_body_ids = vec![source.target_body_id.0];
            job.result_body_ids = vec![source.target_body_id.0];
            job.face_indices = face_indices(&source.face_keys);
            job.radius = source.thickness;
            job.inward = source.inward;
            job
        }
        KernelJobDto::Transform(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 9);
            job.target_body_ids = source.source_body_ids.iter().map(|id| id.0).collect();
            job.result_body_ids = source.result_body_ids.iter().map(|id| id.0).collect();
            for transform in &source.transforms {
                match transform {
                    KernelTransformDto::Mirror { origin, normal } => {
                        job.transform_kinds.push(0);
                        job.transform_values.extend([
                            origin.x, origin.y, origin.z, normal.x, normal.y, normal.z, 0.0, 0.0,
                            0.0, 0.0,
                        ]);
                    }
                    KernelTransformDto::Translate { vector } => {
                        job.transform_kinds.push(1);
                        job.transform_values.extend([
                            vector.x, vector.y, vector.z, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        ]);
                    }
                    KernelTransformDto::Rotate {
                        origin,
                        axis,
                        angle_rad,
                    } => {
                        job.transform_kinds.push(2);
                        job.transform_values.extend([
                            origin.x, origin.y, origin.z, axis.x, axis.y, axis.z, *angle_rad, 0.0,
                            0.0, 0.0,
                        ]);
                    }
                    KernelTransformDto::Rigid {
                        translation,
                        rotation,
                        pivot,
                    } => {
                        job.transform_kinds.push(3);
                        job.transform_values.extend([
                            translation.x,
                            translation.y,
                            translation.z,
                            rotation[0],
                            rotation[1],
                            rotation[2],
                            rotation[3],
                            pivot.x,
                            pivot.y,
                            pivot.z,
                        ]);
                    }
                }
            }
            job
        }
        KernelJobDto::Combine(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 10);
            job.operation = combine_operation_code(source.operation);
            job.target_body_ids = std::iter::once(source.target_body_id.0)
                .chain(source.tool_body_ids.iter().map(|id| id.0))
                .collect();
            job.result_body_ids = vec![source.target_body_id.0];
            job.keep_tools = source.keep_tools;
            job
        }
        KernelJobDto::SplitBody(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 11);
            job.target_body_ids = vec![source.target_body_id.0];
            job.result_body_ids = vec![source.target_body_id.0, source.new_body_id.0];
            job.axis_origin_x = source.plane_origin.x;
            job.axis_origin_y = source.plane_origin.y;
            job.axis_origin_z = source.plane_origin.z;
            job.axis_direction_x = source.plane_normal.x;
            job.axis_direction_y = source.plane_normal.y;
            job.axis_direction_z = source.plane_normal.z;
            job
        }
        KernelJobDto::ImportStep(source) => {
            let mut job = empty_ffi_job(source.feature_id.0, 12);
            job.step_data = decode_base64(&source.data_base64)?;
            job.result_body_ids = vec![source.result_body_id.0];
            job
        }
    })
}

fn decode_base64(value: &str) -> Result<Vec<u8>, OcctError> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return Err(OcctError(
            "STEP import contains invalid base64 data".to_string(),
        ));
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for (chunk_index, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = chunk_index + 1 == bytes.len() / 4;
        let padding = usize::from(chunk[3] == b'=') + usize::from(chunk[2] == b'=');
        if padding > 0 && !last || chunk[2] == b'=' && chunk[3] != b'=' {
            return Err(OcctError(
                "STEP import contains invalid base64 padding".to_string(),
            ));
        }
        let a = digit(chunk[0]);
        let b = digit(chunk[1]);
        let c = if chunk[2] == b'=' {
            Some(0)
        } else {
            digit(chunk[2])
        };
        let d = if chunk[3] == b'=' {
            Some(0)
        } else {
            digit(chunk[3])
        };
        let [Some(a), Some(b), Some(c), Some(d)] = [a, b, c, d] else {
            return Err(OcctError(
                "STEP import contains invalid base64 data".to_string(),
            ));
        };
        let bits = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        output.push((bits >> 16) as u8);
        if padding < 2 {
            output.push((bits >> 8) as u8);
        }
        if padding == 0 {
            output.push(bits as u8);
        }
    }
    Ok(output)
}

fn edge_indices(keys: &[String]) -> Vec<u32> {
    keys.iter()
        .filter_map(|key| key.strip_prefix("edge:")?.parse::<u32>().ok())
        .collect()
}

fn face_indices(keys: &[String]) -> Vec<u32> {
    keys.iter()
        .filter_map(|key| key.strip_prefix("face:")?.parse::<u32>().ok())
        .collect()
}

fn refinement_ffi_job(
    feature_id: u64,
    kind: u8,
    body_id: u64,
    edge_indices: Vec<u32>,
    radius: f64,
) -> ffi::FfiJob {
    let mut job = empty_ffi_job(feature_id, kind);
    job.edge_indices = edge_indices;
    job.radius = radius;
    job.target_body_ids = vec![body_id];
    job.result_body_ids = vec![body_id];
    job
}

fn operation_code(operation: ExtrudeOperation) -> u8 {
    match operation {
        ExtrudeOperation::NewBody => 0,
        ExtrudeOperation::Join => 1,
        ExtrudeOperation::Cut => 2,
        ExtrudeOperation::Intersect => 3,
    }
}

fn combine_operation_code(operation: CombineOperation) -> u8 {
    match operation {
        CombineOperation::Join => 1,
        CombineOperation::Cut => 2,
        CombineOperation::Intersect => 3,
    }
}

fn from_ffi_mesh(raw: ffi::FfiMesh) -> Result<KernelBodyDto, OcctError> {
    if raw.face_first_indices.len() != raw.face_index_counts.len()
        || raw.face_plane_data.len() != raw.face_first_indices.len() * 13
        || raw.face_signature_data.len() != raw.face_first_indices.len() * 8
        || raw.face_cylinder_data.len() != raw.face_first_indices.len() * 11
    {
        return Err(OcctError(
            "OCCT bridge returned malformed face metadata".to_string(),
        ));
    }
    let faces = raw
        .face_first_indices
        .iter()
        .zip(&raw.face_index_counts)
        .enumerate()
        .map(|(index, (first_index, index_count))| {
            let data = &raw.face_plane_data[index * 13..(index + 1) * 13];
            let signature = &raw.face_signature_data[index * 8..(index + 1) * 8];
            let cylinder = &raw.face_cylinder_data[index * 11..(index + 1) * 11];
            let point = |offset: usize| [data[offset], data[offset + 1], data[offset + 2]];
            let signature_point = |offset: usize| Point3Dto {
                x: signature[offset],
                y: signature[offset + 1],
                z: signature[offset + 2],
            };
            let plane = (data[0] != 0.0).then(|| nbcad_core::PlaneBasis {
                origin: point(1),
                u: point(4),
                v: point(7),
                normal: point(10),
            });
            KernelFaceDto {
                key: format!("face:{index}"),
                first_index: *first_index,
                index_count: *index_count,
                plane,
                signature: (signature[0] != 0.0).then(|| nbcad_solid::PlanarFaceSignatureDto {
                    centroid: signature_point(1),
                    normal: Point3Dto::from(plane.expect("signature requires plane").normal),
                    area: signature[4],
                    perimeter: signature[5],
                    wire_count: signature[6].round().max(0.0) as u32,
                    edge_count: signature[7].round().max(0.0) as u32,
                }),
                cylinder: (cylinder[0] != 0.0).then(|| nbcad_solid::CylindricalSurfaceDto {
                    origin: Point3Dto {
                        x: cylinder[1],
                        y: cylinder[2],
                        z: cylinder[3],
                    },
                    axis: Point3Dto {
                        x: cylinder[4],
                        y: cylinder[5],
                        z: cylinder[6],
                    },
                    reference: Point3Dto {
                        x: cylinder[7],
                        y: cylinder[8],
                        z: cylinder[9],
                    },
                    radius: cylinder[10],
                }),
            }
        })
        .collect();

    if raw.edge_point_offsets.is_empty()
        || raw.edge_point_offsets[0] != 0
        || raw
            .edge_point_offsets
            .last()
            .is_none_or(|offset| *offset as usize * 3 != raw.edge_points.len())
        || raw.edge_refinable.len() + 1 != raw.edge_point_offsets.len()
        || raw.edge_circle_data.len() != raw.edge_refinable.len() * 12
    {
        return Err(OcctError(
            "OCCT bridge returned malformed edge metadata".to_string(),
        ));
    }
    let edges = raw
        .edge_point_offsets
        .windows(2)
        .enumerate()
        .map(|(index, offsets)| {
            let circle = &raw.edge_circle_data[index * 12..(index + 1) * 12];
            let points = (offsets[0] as usize..offsets[1] as usize)
                .map(|point_index| {
                    let offset = point_index * 3;
                    Point3Dto {
                        x: raw.edge_points[offset],
                        y: raw.edge_points[offset + 1],
                        z: raw.edge_points[offset + 2],
                    }
                })
                .collect();
            KernelEdgeDto {
                key: format!("edge:{index}"),
                points,
                circle: (circle[0] != 0.0).then(|| nbcad_solid::CircularCurveDto {
                    center: Point3Dto {
                        x: circle[1],
                        y: circle[2],
                        z: circle[3],
                    },
                    normal: Point3Dto {
                        x: circle[4],
                        y: circle[5],
                        z: circle[6],
                    },
                    reference: Point3Dto {
                        x: circle[7],
                        y: circle[8],
                        z: circle[9],
                    },
                    radius: circle[10],
                    closed: circle[11] != 0.0,
                }),
                refinable: raw.edge_refinable[index] != 0,
            }
        })
        .collect();

    Ok(KernelBodyDto {
        body_id: nbcad_core::BodyId(raw.body_id),
        topology_signature: raw.topology_signature,
        positions: raw.positions,
        normals: raw.normals,
        indices: raw.indices,
        faces,
        edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nbcad_core::{BodyId, FaceId, FeatureId};
    use nbcad_solid::{
        iso_metric_grade6_envelope, CylindricalSurfaceDto, HoleBottomStyle, HoleExtent, HoleStyle,
        HoleThreadDto, HoleThreadHand, HoleThreadRepresentation, HoleThreadSeries,
        HoleThreadStandard, KernelChamferJobDto, KernelCombineJobDto, KernelCurveDto,
        KernelExternalThreadJobDto, KernelExtrudeJobDto, KernelFilletJobDto, KernelHoleJobDto,
        KernelImportStepJobDto, KernelJobDto, KernelLoftJobDto, KernelPlanarFaceSourceDto,
        KernelProfileDto, KernelRevolveJobDto, KernelRibJobDto, KernelSplitBodyJobDto,
        KernelSweepJobDto, LoftContinuity, Point3Dto, RecomputePlanDto, StepThreadMetadataDto,
        SweepOrientation, SweepTransition,
    };

    fn square(z: f64, half: f64) -> KernelProfileDto {
        KernelProfileDto {
            profile_index: 0,
            points: vec![
                Point3Dto {
                    x: -half,
                    y: -half,
                    z,
                },
                Point3Dto {
                    x: half,
                    y: -half,
                    z,
                },
                Point3Dto {
                    x: half,
                    y: half,
                    z,
                },
                Point3Dto {
                    x: -half,
                    y: half,
                    z,
                },
            ],
            curves: Vec::new(),
            holes: Vec::new(),
        }
    }

    fn rectangle_profile(
        profile_index: u32,
        min_x: f64,
        max_x: f64,
        min_y: f64,
        max_y: f64,
    ) -> KernelProfileDto {
        KernelProfileDto {
            profile_index,
            points: vec![
                Point3Dto {
                    x: min_x,
                    y: min_y,
                    z: 0.0,
                },
                Point3Dto {
                    x: max_x,
                    y: min_y,
                    z: 0.0,
                },
                Point3Dto {
                    x: max_x,
                    y: max_y,
                    z: 0.0,
                },
                Point3Dto {
                    x: min_x,
                    y: max_y,
                    z: 0.0,
                },
            ],
            curves: Vec::new(),
            holes: Vec::new(),
        }
    }

    fn box_job(feature_id: u64, body_id: u64) -> KernelJobDto {
        KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(feature_id),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![square(0.0, 10.0)],
            normal: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            start_offset: 0.0,
            end_offset: 10.0,
            taper_angle_deg: 0.0,
            target_body_ids: Vec::new(),
            result_body_ids: vec![BodyId(body_id)],
        })
    }

    fn assert_m6_6h_modeled_thread_go_no_go_envelope(scene: &KernelSceneDto, stage: &str) {
        let limits = iso_metric_grade6_envelope(6.0, 1.0, ThreadFit::Internal).unwrap();
        assert_eq!(limits.modeled_major, limits.major_min);
        assert_eq!(limits.modeled_pitch, limits.pitch_min);
        assert_eq!(limits.modeled_minor, limits.minor_min);
        assert!(limits.modeled_pitch <= limits.pitch_max);
        assert!(limits.modeled_minor <= limits.minor_max);

        let wall_samples = scene
            .bodies
            .iter()
            .flat_map(|body| body.positions.chunks_exact(3))
            .filter_map(|point| {
                let radius = (f64::from(point[0]).powi(2) + f64::from(point[1]).powi(2)).sqrt();
                (point[2] > 0.1 && point[2] < 9.9 && radius > 1.0 && radius < 4.0).then_some((
                    radius,
                    f64::from(point[0]),
                    f64::from(point[1]),
                    f64::from(point[2]),
                ))
            })
            .collect::<Vec<_>>();
        let minimum_wall_radius = wall_samples
            .iter()
            .map(|sample| sample.0)
            .fold(f64::INFINITY, f64::min);
        let maximum_wall_sample = wall_samples
            .iter()
            .copied()
            .max_by(|left, right| left.0.total_cmp(&right.0))
            .unwrap();
        let maximum_wall_radius = maximum_wall_sample.0;
        let mesh_tolerance = 0.03;
        assert!(
            minimum_wall_radius >= limits.modeled_minor * 0.5 - mesh_tolerance
                && minimum_wall_radius <= limits.modeled_minor * 0.5 + mesh_tolerance,
            "{stage}: internal 6H minor radius {minimum_wall_radius} does not follow the GO boundary {} (NO-GO maximum {})",
            limits.modeled_minor * 0.5,
            limits.minor_max * 0.5,
        );
        assert!(
            maximum_wall_radius >= limits.modeled_major * 0.5 - mesh_tolerance
                && maximum_wall_radius <= limits.modeled_major * 0.5 + mesh_tolerance,
            "{stage}: internal 6H major radius {maximum_wall_radius} at ({}, {}, {}) does not reach the GO boundary {}",
            maximum_wall_sample.1,
            maximum_wall_sample.2,
            maximum_wall_sample.3,
            limits.modeled_major * 0.5,
        );

        let mut minimum_hole_edge_chord_radius = f64::INFINITY;
        for body in &scene.bodies {
            for edge in &body.edges {
                for segment in edge.points.windows(2) {
                    let a = &segment[0];
                    let b = &segment[1];
                    if a.x.hypot(a.y) >= 4.0 || b.x.hypot(b.y) >= 4.0 {
                        continue;
                    }
                    let dx = b.x - a.x;
                    let dy = b.y - a.y;
                    let denominator = dx * dx + dy * dy;
                    let parameter = if denominator > 1e-12 {
                        (-(a.x * dx + a.y * dy) / denominator).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    minimum_hole_edge_chord_radius = minimum_hole_edge_chord_radius
                        .min((a.x + dx * parameter).hypot(a.y + dy * parameter));
                }
            }
        }
        assert!(
            minimum_hole_edge_chord_radius > limits.modeled_minor * 0.5 - 0.06,
            "{stage}: displayed thread edge chords cross the cavity at radius {minimum_hole_edge_chord_radius}"
        );

        let axis_cover_count = scene
            .bodies
            .iter()
            .map(|body| {
                body.indices
                    .chunks_exact(3)
                    .filter(|triangle| {
                        let point = |index: u32| {
                            let offset = index as usize * 3;
                            [
                                body.positions[offset] as f64,
                                body.positions[offset + 1] as f64,
                            ]
                        };
                        let a = point(triangle[0]);
                        let b = point(triangle[1]);
                        let c = point(triangle[2]);
                        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                        if area.abs() < 1e-10 {
                            return false;
                        }
                        let first = (b[0] * c[1] - b[1] * c[0]) / area;
                        let second = (c[0] * a[1] - c[1] * a[0]) / area;
                        let third = 1.0 - first - second;
                        first >= -1e-8 && second >= -1e-8 && third >= -1e-8
                    })
                    .count()
            })
            .sum::<usize>();
        assert_eq!(
            axis_cover_count, 0,
            "{stage}: modeled through-thread left triangular material across its center axis"
        );

        let internal_thread_cap_faces = scene
            .bodies
            .iter()
            .flat_map(|body| body.faces.iter().map(move |face| (body, face)))
            .filter(|(_, face)| face.plane.is_some())
            .filter(|(body, face)| {
                let begin = face.first_index as usize;
                let end = begin + face.index_count as usize;
                let mut minimum_radius = f64::INFINITY;
                let mut maximum_radius = f64::NEG_INFINITY;
                let mut minimum_z = f64::INFINITY;
                let mut maximum_z = f64::NEG_INFINITY;
                for index in &body.indices[begin..end] {
                    let offset = *index as usize * 3;
                    let x = f64::from(body.positions[offset]);
                    let y = f64::from(body.positions[offset + 1]);
                    let z = f64::from(body.positions[offset + 2]);
                    let radius = x.hypot(y);
                    minimum_radius = minimum_radius.min(radius);
                    maximum_radius = maximum_radius.max(radius);
                    minimum_z = minimum_z.min(z);
                    maximum_z = maximum_z.max(z);
                }
                minimum_radius > 2.2 && maximum_radius < 3.2 && minimum_z > 0.2 && maximum_z < 9.8
            })
            .count();
        assert_eq!(
            internal_thread_cap_faces, 0,
            "{stage}: modeled thread left planar cutter caps inside the hole"
        );
    }

    fn thread_handedness_score(body: &KernelBodyDto) -> f64 {
        body.edges
            .iter()
            .flat_map(|edge| edge.points.windows(2))
            .filter_map(|segment| {
                let first = &segment[0];
                let second = &segment[1];
                let first_radius = first.x.hypot(first.y);
                let second_radius = second.x.hypot(second.y);
                if first_radius < 2.2
                    || first_radius > 3.1
                    || second_radius < 2.2
                    || second_radius > 3.1
                    || (second.z - first.z).abs() < 1e-6
                {
                    return None;
                }
                let mut angular_delta = second.y.atan2(second.x) - first.y.atan2(first.x);
                if angular_delta > std::f64::consts::PI {
                    angular_delta -= std::f64::consts::TAU;
                } else if angular_delta < -std::f64::consts::PI {
                    angular_delta += std::f64::consts::TAU;
                }
                Some(angular_delta * (second.z - first.z))
            })
            .sum()
    }

    fn encode_base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let a = u32::from(chunk[0]);
            let b = u32::from(*chunk.get(1).unwrap_or(&0));
            let c = u32::from(*chunk.get(2).unwrap_or(&0));
            let bits = (a << 16) | (b << 8) | c;
            encoded.push(ALPHABET[((bits >> 18) & 63) as usize] as char);
            encoded.push(ALPHABET[((bits >> 12) & 63) as usize] as char);
            encoded.push(if chunk.len() > 1 {
                ALPHABET[((bits >> 6) & 63) as usize] as char
            } else {
                '='
            });
            encoded.push(if chunk.len() > 2 {
                ALPHABET[(bits & 63) as usize] as char
            } else {
                '='
            });
        }
        encoded
    }

    #[test]
    fn occt_extrudes_and_meshes_a_rectangle() {
        let mut kernel = OcctKernel::new().unwrap();
        let plan = RecomputePlanDto {
            transaction_id: 1,
            errors: Vec::new(),
            jobs: vec![KernelJobDto::Extrude(KernelExtrudeJobDto {
                feature_id: FeatureId(2),
                operation: ExtrudeOperation::NewBody,
                source_face: None,
                profiles: vec![KernelProfileDto {
                    profile_index: 0,
                    points: vec![
                        Point3Dto {
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 40.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 40.0,
                            y: 30.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 0.0,
                            y: 30.0,
                            z: 0.0,
                        },
                    ],
                    curves: Vec::new(),
                    holes: Vec::new(),
                }],
                normal: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                start_offset: 0.0,
                end_offset: 20.0,
                taper_angle_deg: 0.0,
                target_body_ids: vec![],
                result_body_ids: vec![BodyId(1)],
            })],
        };
        let scene = kernel.recompute(&plan).unwrap();
        assert!(scene.errors.is_empty());
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(scene.bodies[0].faces.len(), 6);
        assert_eq!(scene.bodies[0].edges.len(), 12);
        assert_eq!(scene.bodies[0].indices.len(), 36);
        assert!(scene.bodies[0]
            .faces
            .iter()
            .all(|face| face.plane.is_some()));

        let step = kernel.export_step(&StepExportRequest::default()).unwrap();
        let text = String::from_utf8(step).unwrap();
        assert!(text.starts_with("ISO-10303-21;"));
        assert!(
            text.to_ascii_uppercase().contains("AP242"),
            "{}",
            &text[..text.len().min(2_000)]
        );
        assert!(text.contains("MANIFOLD_SOLID_BREP"));
        assert!(text.ends_with("END-ISO-10303-21;\n"));

        let placed_step = kernel
            .export_step(&StepExportRequest {
                body_ids: Vec::new(),
                thread_metadata: Vec::new(),
                occurrences: vec![StepOccurrencePlacementDto {
                    occurrence_id: 7,
                    component_id: 3,
                    body_id: BodyId(1),
                    name: "Placed box".to_string(),
                    translation: [125.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                }],
            })
            .unwrap();
        let mut placed_kernel = OcctKernel::new().unwrap();
        let placed_scene = placed_kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 99,
                errors: Vec::new(),
                jobs: vec![KernelJobDto::ImportStep(KernelImportStepJobDto {
                    feature_id: FeatureId(99),
                    data_base64: encode_base64(&placed_step),
                    result_body_id: BodyId(99),
                })],
            })
            .unwrap();
        let minimum_x = placed_scene.bodies[0]
            .positions
            .chunks_exact(3)
            .map(|point| point[0])
            .fold(f32::INFINITY, f32::min);
        assert!((minimum_x - 125.0).abs() < 1.0e-3);

        let metadata_step = kernel
            .export_step(&StepExportRequest {
                body_ids: Vec::new(),
                thread_metadata: vec![StepThreadMetadataDto {
                    body_id: BodyId(1),
                    feature_id: FeatureId(3),
                    feature_name: "Hole1".to_string(),
                    position_count: 1,
                    external: false,
                    predrill_diameter: 5.0,
                    thread: HoleThreadDto {
                        standard: HoleThreadStandard::IsoMetric,
                        series: HoleThreadSeries::MetricCoarse,
                        designation: "M6 x 1 - 6H".to_string(),
                        class: "6H".to_string(),
                        nominal_diameter: 6.0,
                        pitch: 1.0,
                        threads_per_inch: None,
                        hand: HoleThreadHand::Right,
                        depth: None,
                        representation: HoleThreadRepresentation::Modeled,
                        tap_drill_designation: Some("5 mm".to_string()),
                    },
                }],
                occurrences: Vec::new(),
            })
            .unwrap();
        let metadata_text = String::from_utf8(metadata_step.clone()).unwrap();
        let compact_metadata = metadata_text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        assert!(compact_metadata.contains("NBCAD_THREAD_METADATA_V1_HEX="));
        assert!(compact_metadata.contains("4d3620782031202d203648"));

        let mut roundtrip_kernel = OcctKernel::new().unwrap();
        let roundtrip = roundtrip_kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 2,
                errors: Vec::new(),
                jobs: vec![KernelJobDto::ImportStep(KernelImportStepJobDto {
                    feature_id: FeatureId(4),
                    result_body_id: BodyId(8),
                    data_base64: encode_base64(&metadata_step),
                })],
            })
            .unwrap();
        assert!(roundtrip.errors.is_empty());
        assert_eq!(roundtrip.bodies.len(), 1);
    }

    #[test]
    fn export_quality_does_not_change_retained_mesh_or_later_exports() {
        let point = |x, y| Point3Dto { x, y, z: 0. };
        let mut cylinder = box_job(1, 1);
        if let KernelJobDto::Extrude(job) = &mut cylinder {
            job.profiles = vec![KernelProfileDto {
                profile_index: 0,
                points: (0..64)
                    .map(|i| {
                        let angle = std::f64::consts::TAU * i as f64 / 64.;
                        point(10. * angle.cos(), 10. * angle.sin())
                    })
                    .collect(),
                curves: vec![KernelCurveDto::Circle {
                    entity_id: 1,
                    center: point(0., 0.),
                    axis_point: point(10., 0.),
                    normal: Point3Dto {
                        x: 0.,
                        y: 0.,
                        z: 1.,
                    },
                }],
                holes: vec![],
            }];
        }
        let mut plan = RecomputePlanDto {
            transaction_id: 1,
            jobs: vec![cylinder],
            errors: vec![],
        };
        let mut kernel = OcctKernel::new().unwrap();
        let initial = kernel.recompute(&plan).unwrap();
        assert!(initial.errors.is_empty());
        let fine_request = MeshExportRequest {
            linear_deflection: 0.01,
            angular_deflection: 0.05,
            ..Default::default()
        };
        let coarse_request = MeshExportRequest {
            linear_deflection: 0.6,
            angular_deflection: 0.8,
            ..Default::default()
        };
        let fine = kernel.tessellate_bodies(&fine_request).unwrap();
        assert!(fine[0].indices.len() > initial.bodies[0].indices.len());
        let repeated = kernel.recompute(&plan).unwrap();
        assert_eq!(
            repeated.bodies[0].indices.len(),
            initial.bodies[0].indices.len(),
            "manufacturing export must not replace the retained render triangulation"
        );
        assert_eq!(repeated, initial);
        assert_eq!(
            kernel.last_applied_jobs, 0,
            "export must preserve native prefix reuse"
        );
        let coarse = kernel.tessellate_bodies(&coarse_request).unwrap();
        assert!(coarse[0].indices.len() < fine[0].indices.len());
        let mut cold = OcctKernel::new().unwrap();
        cold.recompute(&plan).unwrap();
        assert_eq!(
            coarse,
            cold.tessellate_bodies(&coarse_request).unwrap(),
            "requested export quality cannot depend on earlier exports"
        );
        assert_eq!(fine, kernel.tessellate_bodies(&fine_request).unwrap());
        plan.jobs.push(box_job(2, 2));
        assert_eq!(
            kernel.recompute(&plan).unwrap(),
            OcctKernel::new().unwrap().recompute(&plan).unwrap()
        );
        assert_eq!(kernel.last_applied_jobs, 1);
    }

    #[test]
    fn append_replay_matches_cold_geometry_and_rebuilds_on_edit_rollback_or_failure() {
        let mut kernel = OcctKernel::new().unwrap();
        let mut plan = RecomputePlanDto {
            transaction_id: 1,
            jobs: vec![box_job(1, 1)],
            errors: vec![],
        };
        let original = kernel.recompute(&plan).unwrap();
        assert_eq!(kernel.last_applied_jobs, 1);
        plan.transaction_id += 1;
        assert_eq!(kernel.recompute(&plan).unwrap(), original);
        assert_eq!(
            kernel.last_applied_jobs, 0,
            "transaction IDs alone do not invalidate geometry"
        );
        let mut second = box_job(2, 2);
        if let KernelJobDto::Extrude(job) = &mut second {
            for profile in &mut job.profiles {
                for p in &mut profile.points {
                    p.x += 5.;
                }
            }
        }
        plan.jobs.push(second);
        assert_eq!(
            kernel.recompute(&plan).unwrap(),
            OcctKernel::new().unwrap().recompute(&plan).unwrap()
        );
        assert_eq!(kernel.last_applied_jobs, 1);
        plan.jobs
            .push(KernelJobDto::Combine(nbcad_solid::KernelCombineJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                tool_body_ids: vec![BodyId(2)],
                operation: CombineOperation::Join,
                keep_tools: false,
            }));
        let combined = kernel.recompute(&plan).unwrap();
        assert_eq!(combined.bodies.len(), 1);
        assert_eq!(
            combined,
            OcctKernel::new().unwrap().recompute(&plan).unwrap()
        );
        assert_eq!(kernel.last_applied_jobs, 1);
        let good = plan.clone();
        // A failed append must never mark its partially changed native state as reusable.
        plan.jobs
            .push(KernelJobDto::Combine(nbcad_solid::KernelCombineJobDto {
                feature_id: FeatureId(4),
                target_body_id: BodyId(1),
                tool_body_ids: vec![BodyId(999)],
                operation: CombineOperation::Join,
                keep_tools: false,
            }));
        assert!(!kernel.recompute(&plan).unwrap().errors.is_empty());
        assert!(kernel.successful_jobs.is_none());
        assert_eq!(kernel.recompute(&good).unwrap(), combined);
        assert_eq!(kernel.last_applied_jobs, 3);
        plan = good;
        if let KernelJobDto::Extrude(job) = &mut plan.jobs[0] {
            job.end_offset += 2.;
        }
        let edited = kernel.recompute(&plan).unwrap();
        assert_eq!(edited, OcctKernel::new().unwrap().recompute(&plan).unwrap());
        assert_ne!(edited, combined);
        assert_eq!(kernel.last_applied_jobs, 3);
        plan.jobs.truncate(1);
        assert_eq!(
            kernel.recompute(&plan).unwrap(),
            OcctKernel::new().unwrap().recompute(&plan).unwrap()
        );
        assert_eq!(kernel.last_applied_jobs, 1);
        plan.errors.push(KernelFeatureErrorDto {
            feature_id: FeatureId(9),
            message: "missing sketch".into(),
        });
        assert!(!kernel.recompute(&plan).unwrap().errors.is_empty());
        assert!(kernel.successful_jobs.is_none());
        plan.errors.clear();
        kernel.recompute(&plan).unwrap();
        assert_eq!(kernel.last_applied_jobs, 1);
    }

    #[test]
    fn exact_projection_cache_tracks_geometry_sections_and_solved_placements() {
        use nbcad_assembly::{ComponentId, InstanceBodyPoseDto, OccurrenceId};
        use nbcad_sketch::DrawingViewScope;
        let mut kernel = OcctKernel::new().unwrap();
        let mut plan = RecomputePlanDto {
            transaction_id: 1,
            errors: vec![],
            jobs: vec![box_job(1, 1)],
        };
        kernel.recompute(&plan).unwrap();
        let mut request = DrawingProjectionRequest {
            scope: DrawingViewScope::Definition,
            occurrence_ids: vec![],
            resolved_occurrences: None,
            body_ids: vec![BodyId(1)],
            direction: [0., -1., 0.],
            up: [0., 0., 1.],
            include_hidden: false,
            include_tangent_edges: false,
            deflection: 0.05,
            section_plane: None,
        };
        let first = kernel.drawing_projection(&request).unwrap();
        let value = |projection: DrawingProjectionDto| serde_json::to_value(projection).unwrap();
        assert_eq!(
            value(first.clone()),
            value(kernel.drawing_projection(&request).unwrap())
        );
        kernel.recompute(&plan).unwrap();
        assert_eq!(
            value(first.clone()),
            value(kernel.drawing_projection(&request).unwrap())
        );
        assert_eq!(
            kernel
                .projection_calculations
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        request.section_plane = Some(crate::DrawingSectionPlaneDto {
            point: [0., 0., 0.],
            normal: [0., -1., 0.],
            depth: Some(5.),
        });
        kernel.drawing_projection(&request).unwrap();
        assert_eq!(
            kernel
                .projection_calculations
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        request.section_plane = None;
        request.scope = DrawingViewScope::Assembly;
        request.resolved_occurrences = Some(vec![InstanceBodyPoseDto {
            occurrence_id: OccurrenceId(1),
            component_id: ComponentId(1),
            body_id: BodyId(1),
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            visible: true,
        }]);
        let home = kernel.drawing_projection(&request).unwrap();
        request.resolved_occurrences.as_mut().unwrap()[0].translation[0] = 40.;
        let moved = kernel.drawing_projection(&request).unwrap();
        assert!((moved.bounds[0] - home.bounds[0]).abs() > 39.);
        assert_eq!(
            kernel
                .projection_calculations
                .load(std::sync::atomic::Ordering::Relaxed),
            4
        );
        if let KernelJobDto::Extrude(job) = &mut plan.jobs[0] {
            job.end_offset += 7.;
        }
        kernel.recompute(&plan).unwrap();
        assert!(kernel.projection_cache.lock().unwrap().is_empty());
        let edited = kernel.drawing_projection(&request).unwrap();
        let mut cold = OcctKernel::new().unwrap();
        cold.recompute(&plan).unwrap();
        assert_eq!(
            value(edited),
            value(cold.drawing_projection(&request).unwrap())
        );
        for index in 0..20 {
            request.deflection = 0.01 + index as f64 * 0.01;
            kernel.drawing_projection(&request).unwrap();
        }
        assert!(kernel.projection_cache.lock().unwrap().len() <= 16);
        plan.errors.push(KernelFeatureErrorDto {
            feature_id: nbcad_core::FeatureId(2),
            message: "missing sketch".into(),
        });
        kernel.recompute(&plan).unwrap();
        assert!(kernel.projection_cache.lock().unwrap().is_empty());
    }

    #[test]
    fn exact_hlr_projects_a_box_to_vector_edges() {
        let mut kernel = OcctKernel::new().unwrap();
        kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![box_job(1, 1)],
            })
            .unwrap();
        let projection = kernel
            .drawing_projection(&DrawingProjectionRequest {
                scope: Default::default(),
                occurrence_ids: vec![],
                resolved_occurrences: None,
                body_ids: vec![BodyId(1)],
                direction: [0.0, 0.0, 1.0],
                up: [0.0, 1.0, 0.0],
                include_hidden: true,
                include_tangent_edges: false,
                deflection: 0.05,
                section_plane: None,
            })
            .unwrap();
        assert!(projection.visible.len() >= 4);
        assert!((projection.bounds[0] + 10.0).abs() < 1.0e-6);
        assert!((projection.bounds[1] + 10.0).abs() < 1.0e-6);
        assert!((projection.bounds[2] - 10.0).abs() < 1.0e-6);
        assert!((projection.bounds[3] - 10.0).abs() < 1.0e-6);
    }

    #[test]
    fn exact_interference_uses_placed_retained_breps() {
        let mut kernel = OcctKernel::new().unwrap();
        kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![box_job(1, 1), box_job(2, 2)],
            })
            .unwrap();
        let identity = |body_id| PlacedBodyQueryDto {
            body_id: BodyId(body_id),
            translation: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };

        let separated = kernel
            .exact_interference(
                identity(1),
                PlacedBodyQueryDto {
                    translation: [22.0, 0.0, 0.0],
                    ..identity(2)
                },
            )
            .unwrap();
        assert!((separated.minimum_clearance_mm - 2.0).abs() < 1.0e-7);
        assert!(separated.overlap_volume_mm3.abs() < 1.0e-7);

        let overlapping = kernel
            .exact_interference(
                identity(1),
                PlacedBodyQueryDto {
                    translation: [15.0, 0.0, 0.0],
                    ..identity(2)
                },
            )
            .unwrap();
        assert!(overlapping.minimum_clearance_mm.abs() < 1.0e-7);
        assert!((overlapping.overlap_volume_mm3 - 1_000.0).abs() < 1.0e-5);
    }

    #[test]
    fn exact_section_hlr_clips_the_retained_half_space_and_depth_slab() {
        let mut kernel = OcctKernel::new().unwrap();
        kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![box_job(1, 1)],
            })
            .unwrap();

        let request = |depth| DrawingProjectionRequest {
            scope: Default::default(),
            occurrence_ids: vec![],
            resolved_occurrences: None,
            body_ids: vec![BodyId(1)],
            direction: [0.0, 0.0, 1.0],
            up: [0.0, 1.0, 0.0],
            include_hidden: true,
            include_tangent_edges: false,
            deflection: 0.05,
            section_plane: Some(crate::DrawingSectionPlaneDto {
                point: [0.0, 0.0, 5.0],
                normal: [1.0, 0.0, 0.0],
                depth,
            }),
        };

        let full = kernel.drawing_projection(&request(None)).unwrap();
        assert!(!full.visible.is_empty());
        assert!(!full.section.is_empty());
        assert!((full.bounds[0] + 10.0).abs() < 1.0e-6);
        assert!(full.bounds[2].abs() < 1.0e-6);

        let depth = kernel.drawing_projection(&request(Some(4.0))).unwrap();
        assert!(!depth.visible.is_empty());
        assert!(!depth.section.is_empty());
        assert!((depth.bounds[0] + 4.0).abs() < 1.0e-6);
        assert!(depth.bounds[2].abs() < 1.0e-6);

        let error = kernel.drawing_projection(&request(Some(0.0))).unwrap_err();
        assert!(error.to_string().contains("section depth"));

        let mut invalid = request(None);
        invalid.deflection = f64::NAN;
        let error = kernel.drawing_projection(&invalid).unwrap_err();
        assert!(error.to_string().contains("deflection"));

        let mut invalid = request(None);
        invalid.section_plane.as_mut().unwrap().normal = [0.0; 3];
        let error = kernel.drawing_projection(&invalid).unwrap_err();
        assert!(error.to_string().contains("section plane"));
    }

    #[test]
    fn occt_imports_an_exported_step_as_a_recomputable_body() {
        let mut source_kernel = OcctKernel::new().unwrap();
        let source_scene = source_kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![box_job(1, 1)],
            })
            .unwrap();
        assert!(source_scene.errors.is_empty());
        let step = source_kernel
            .export_step(&StepExportRequest::default())
            .unwrap();

        let mut imported_kernel = OcctKernel::new().unwrap();
        let imported_scene = imported_kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 2,
                errors: Vec::new(),
                jobs: vec![KernelJobDto::ImportStep(KernelImportStepJobDto {
                    feature_id: FeatureId(2),
                    result_body_id: BodyId(7),
                    data_base64: encode_base64(&step),
                })],
            })
            .unwrap();

        assert!(imported_scene.errors.is_empty());
        assert_eq!(imported_scene.bodies.len(), 1);
        assert_eq!(imported_scene.bodies[0].body_id, BodyId(7));
        assert!(!imported_scene.bodies[0].positions.is_empty());
        let reexported = imported_kernel
            .export_step(&StepExportRequest::default())
            .unwrap();
        assert!(reexported.starts_with(b"ISO-10303-21;"));
    }

    #[test]
    fn occt_joins_adjacent_extrude_profiles_without_an_existing_target() {
        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![KernelJobDto::Extrude(KernelExtrudeJobDto {
                    feature_id: FeatureId(2),
                    operation: ExtrudeOperation::Join,
                    source_face: None,
                    profiles: vec![
                        rectangle_profile(0, -10.0, 0.0, -5.0, 5.0),
                        rectangle_profile(1, 0.0, 10.0, -5.0, 5.0),
                    ],
                    normal: Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    start_offset: 0.0,
                    end_offset: 10.0,
                    taper_angle_deg: 0.0,
                    target_body_ids: Vec::new(),
                    result_body_ids: vec![BodyId(1)],
                })],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(
            scene.bodies[0].faces.len(),
            6,
            "same-domain cap and wall faces should be unified"
        );
        assert_eq!(
            scene.bodies[0].edges.len(),
            12,
            "the shared profile boundary must not survive as a seam"
        );
        assert!(scene.bodies[0].edges.iter().all(|edge| edge.refinable));
    }

    #[test]
    fn occt_combine_join_unifies_coplanar_faces_and_removes_the_seam() {
        let new_body = |feature_id, body_id, profile| {
            KernelJobDto::Extrude(KernelExtrudeJobDto {
                feature_id: FeatureId(feature_id),
                operation: ExtrudeOperation::NewBody,
                source_face: None,
                profiles: vec![profile],
                normal: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                start_offset: 0.0,
                end_offset: 10.0,
                taper_angle_deg: 0.0,
                target_body_ids: Vec::new(),
                result_body_ids: vec![BodyId(body_id)],
            })
        };
        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![
                    new_body(1, 1, rectangle_profile(0, -10.0, 0.0, -5.0, 5.0)),
                    new_body(2, 2, rectangle_profile(0, 0.0, 10.0, -5.0, 5.0)),
                    KernelJobDto::Combine(KernelCombineJobDto {
                        feature_id: FeatureId(3),
                        target_body_id: BodyId(1),
                        tool_body_ids: vec![BodyId(2)],
                        operation: CombineOperation::Join,
                        keep_tools: false,
                    }),
                ],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(scene.bodies[0].body_id, BodyId(1));
        assert_eq!(scene.bodies[0].faces.len(), 6);
        assert_eq!(scene.bodies[0].edges.len(), 12);
        assert!(scene.bodies[0].edges.iter().all(|edge| edge.refinable));
    }

    #[test]
    fn occt_extrudes_one_analytic_arc_into_one_curved_face() {
        let p = |x, y| Point3Dto { x, y, z: 0.0 };
        let profile = KernelProfileDto {
            profile_index: 0,
            // Discovery/preview tessellation deliberately contains several
            // samples; the analytic curve list must determine B-rep topology.
            points: vec![
                p(-10.0, 0.0),
                p(10.0, 0.0),
                p(7.071, 7.071),
                p(0.0, 10.0),
                p(-7.071, 7.071),
            ],
            curves: vec![
                KernelCurveDto::Line {
                    entity_id: 1,
                    start: p(-10.0, 0.0),
                    end: p(10.0, 0.0),
                },
                KernelCurveDto::Arc {
                    entity_id: 2,
                    start: p(10.0, 0.0),
                    mid: p(0.0, 10.0),
                    end: p(-10.0, 0.0),
                },
            ],
            holes: Vec::new(),
        };
        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![KernelJobDto::Extrude(KernelExtrudeJobDto {
                    feature_id: FeatureId(2),
                    operation: ExtrudeOperation::NewBody,
                    source_face: None,
                    profiles: vec![profile],
                    normal: Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    start_offset: 0.0,
                    end_offset: 5.0,
                    taper_angle_deg: 0.0,
                    target_body_ids: Vec::new(),
                    result_body_ids: vec![BodyId(1)],
                })],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        let body = &scene.bodies[0];
        assert_eq!(body.faces.len(), 4, "caps + line side + one arc side");
        assert_eq!(
            body.faces
                .iter()
                .filter(|face| face.plane.is_none())
                .count(),
            1,
            "the analytic arc must create one cylindrical face"
        );
        assert_eq!(
            body.edges.len(),
            6,
            "two profile edges at each cap + uprights"
        );

        let curved_face = body.faces.iter().find(|face| face.plane.is_none()).unwrap();
        let first = curved_face.first_index as usize;
        let last = first + curved_face.index_count as usize;
        let mut curved_normals = body.indices[first..last]
            .iter()
            .map(|index| {
                let offset = *index as usize * 3;
                (
                    (body.normals[offset] * 100.0).round() as i32,
                    (body.normals[offset + 1] * 100.0).round() as i32,
                    (body.normals[offset + 2] * 100.0).round() as i32,
                )
            })
            .collect::<Vec<_>>();
        curved_normals.sort_unstable();
        curved_normals.dedup();
        assert!(
            curved_normals.len() > 3,
            "the curved face should use varying vertex normals instead of flat facets"
        );

        let step =
            String::from_utf8(kernel.export_step(&StepExportRequest::default()).unwrap()).unwrap();
        assert!(step.contains("CYLINDRICAL_SURFACE"));
    }

    #[test]
    fn occt_extrudes_nested_profile_as_one_body_with_a_real_hole() {
        let mut outer = square(0.0, 20.0);
        let mut inner = square(0.0, 6.0);
        inner.profile_index = 1;
        outer.holes.push(inner);

        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![KernelJobDto::Extrude(KernelExtrudeJobDto {
                    feature_id: FeatureId(2),
                    operation: ExtrudeOperation::NewBody,
                    source_face: None,
                    profiles: vec![outer],
                    normal: Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    start_offset: 0.0,
                    end_offset: 10.0,
                    taper_angle_deg: 0.0,
                    target_body_ids: Vec::new(),
                    result_body_ids: vec![BodyId(1)],
                })],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(scene.bodies[0].faces.len(), 10, "two caps plus eight walls");
        assert_eq!(scene.bodies[0].edges.len(), 24);
        let body = &scene.bodies[0];
        for face in body
            .faces
            .iter()
            .filter(|face| face.plane.is_some_and(|basis| basis.normal[2].abs() > 0.9))
        {
            let begin = face.first_index as usize;
            let end = begin + face.index_count as usize;
            for triangle in body.indices[begin..end].chunks_exact(3) {
                let centroid = triangle.iter().fold([0.0f64; 3], |mut sum, index| {
                    let offset = *index as usize * 3;
                    sum[0] += body.positions[offset] as f64 / 3.0;
                    sum[1] += body.positions[offset + 1] as f64 / 3.0;
                    sum[2] += body.positions[offset + 2] as f64 / 3.0;
                    sum
                });
                assert!(
                    centroid[0].abs() >= 5.5 || centroid[1].abs() >= 5.5,
                    "a cap triangle filled the intended inner void at {centroid:?}"
                );
            }
        }
    }

    #[test]
    fn occt_extrudes_an_arch_profile_with_an_analytic_circular_hole() {
        let p = |x, y| Point3Dto { x, y, z: 0.0 };
        let mut outer = KernelProfileDto {
            profile_index: 0,
            points: vec![
                p(-10.0, 0.0),
                p(10.0, 0.0),
                p(10.0, 30.0),
                p(0.0, 40.0),
                p(-10.0, 30.0),
            ],
            curves: vec![
                KernelCurveDto::Line {
                    entity_id: 1,
                    start: p(-10.0, 0.0),
                    end: p(10.0, 0.0),
                },
                KernelCurveDto::Line {
                    entity_id: 2,
                    start: p(10.0, 0.0),
                    end: p(10.0, 30.0),
                },
                KernelCurveDto::Arc {
                    entity_id: 3,
                    start: p(10.0, 30.0),
                    mid: p(0.0, 40.0),
                    end: p(-10.0, 30.0),
                },
                KernelCurveDto::Line {
                    entity_id: 4,
                    start: p(-10.0, 30.0),
                    end: p(-10.0, 0.0),
                },
            ],
            holes: Vec::new(),
        };
        outer.holes.push(KernelProfileDto {
            profile_index: 1,
            points: (0..64)
                .map(|index| {
                    let angle = std::f64::consts::TAU * index as f64 / 64.0;
                    p(5.0 * angle.cos(), 30.0 + 5.0 * angle.sin())
                })
                .collect(),
            curves: vec![KernelCurveDto::Circle {
                entity_id: 5,
                center: p(0.0, 30.0),
                axis_point: p(5.0, 30.0),
                normal: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
            }],
            holes: Vec::new(),
        });

        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![KernelJobDto::Extrude(KernelExtrudeJobDto {
                    feature_id: FeatureId(2),
                    operation: ExtrudeOperation::NewBody,
                    source_face: None,
                    profiles: vec![outer],
                    normal: Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    start_offset: 0.0,
                    end_offset: 10.0,
                    taper_angle_deg: 0.0,
                    target_body_ids: Vec::new(),
                    result_body_ids: vec![BodyId(1)],
                })],
            })
            .unwrap();

        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_eq!(scene.bodies.len(), 1);
        assert!(scene.bodies[0].faces.iter().any(|face| face
            .cylinder
            .is_some_and(|cylinder| (cylinder.radius - 5.0).abs() < 1e-6)));
    }

    #[test]
    fn occt_exposes_exact_cylinder_and_both_countersink_rims() {
        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 2,
                errors: Vec::new(),
                jobs: vec![
                    box_job(2, 1),
                    KernelJobDto::Hole(KernelHoleJobDto {
                        feature_id: FeatureId(3),
                        target_body_id: BodyId(1),
                        center: Point3Dto {
                            x: 0.0,
                            y: 0.0,
                            z: 10.0,
                        },
                        direction: Point3Dto {
                            x: 0.0,
                            y: 0.0,
                            z: -1.0,
                        },
                        diameter: 4.0,
                        extent: HoleExtent::ThroughAll,
                        style: HoleStyle::Countersink,
                        counterbore_diameter: 0.0,
                        counterbore_depth: 0.0,
                        countersink_diameter: 8.0,
                        countersink_angle_deg: 90.0,
                        bottom_style: HoleBottomStyle::Flat,
                        drill_point_angle_deg: 118.0,
                        thread: None,
                    }),
                ],
            })
            .unwrap();

        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        let cylinder = scene.bodies[0]
            .faces
            .iter()
            .find_map(|face| face.cylinder.as_ref())
            .expect("the through-hole wall must retain its exact OCCT cylinder");
        assert!((cylinder.radius - 2.0).abs() < 1e-8);
        assert!((cylinder.axis.x.abs() + cylinder.axis.y.abs()) < 1e-8);
        assert!((cylinder.axis.z.abs() - 1.0).abs() < 1e-8);
        assert!((cylinder.reference.x.hypot(cylinder.reference.y) - 1.0).abs() < 1e-8);

        let mut closed_radii = scene.bodies[0]
            .edges
            .iter()
            .filter_map(|edge| edge.circle.filter(|circle| circle.closed))
            .map(|circle| circle.radius)
            .collect::<Vec<_>>();
        closed_radii.sort_by(f64::total_cmp);
        assert!(
            closed_radii
                .iter()
                .any(|radius| (*radius - 2.0).abs() < 1e-8),
            "the inner hole rim must remain an exact selectable OCCT circle: {closed_radii:?}"
        );
        assert!(
            closed_radii.iter().any(|radius| (*radius - 4.0).abs() < 2e-4),
            "the outer countersink rim must remain an exact selectable OCCT circle: {closed_radii:?}"
        );
    }

    #[test]
    fn occt_exact_planar_face_extrude_preserves_inner_boundary_wires() {
        let mut outer = square(0.0, 20.0);
        let mut inner = square(0.0, 6.0);
        inner.profile_index = 1;
        outer.holes.push(inner);
        let base_job = KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(2),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![outer],
            normal: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            start_offset: 0.0,
            end_offset: 10.0,
            taper_angle_deg: 0.0,
            target_body_ids: Vec::new(),
            result_body_ids: vec![BodyId(1)],
        });

        let mut kernel = OcctKernel::new().unwrap();
        let base_scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![base_job.clone()],
            })
            .unwrap();
        let source = base_scene.bodies[0]
            .faces
            .iter()
            .find(|face| {
                face.plane
                    .is_some_and(|basis| basis.normal[2] > 0.9 && basis.origin[2] > 9.0)
            })
            .expect("hollow body should expose a planar top face");
        let basis = source.plane.unwrap();
        let signature = source.signature.expect("planar face signature");
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 2,
                errors: Vec::new(),
                jobs: vec![
                    base_job.clone(),
                    KernelJobDto::Extrude(KernelExtrudeJobDto {
                        feature_id: FeatureId(3),
                        operation: ExtrudeOperation::NewBody,
                        source_face: Some(KernelPlanarFaceSourceDto {
                            body_id: BodyId(1),
                            face_id: FaceId(42),
                            // The ordinal is deliberately wrong. It is only a
                            // diagnostic hint; the exact BRep signature must
                            // resolve the intended face after face reordering.
                            face_key: "face:999".to_string(),
                            signature,
                        }),
                        profiles: Vec::new(),
                        normal: basis.normal.into(),
                        start_offset: 0.0,
                        end_offset: 5.0,
                        taper_angle_deg: 0.0,
                        target_body_ids: Vec::new(),
                        result_body_ids: vec![BodyId(2)],
                    }),
                ],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        let body = scene
            .bodies
            .iter()
            .find(|body| body.body_id == BodyId(2))
            .expect("exact-face Extrude should create its reserved body");
        assert_eq!(body.faces.len(), 10, "two annular caps plus eight walls");
        for face in body
            .faces
            .iter()
            .filter(|face| face.plane.is_some_and(|plane| plane.normal[2].abs() > 0.9))
        {
            let begin = face.first_index as usize;
            let end = begin + face.index_count as usize;
            for triangle in body.indices[begin..end].chunks_exact(3) {
                let centroid = triangle.iter().fold([0.0f64; 2], |mut sum, index| {
                    let offset = *index as usize * 3;
                    sum[0] += body.positions[offset] as f64 / 3.0;
                    sum[1] += body.positions[offset + 1] as f64 / 3.0;
                    sum
                });
                assert!(
                    centroid[0].abs() >= 5.5 || centroid[1].abs() >= 5.5,
                    "exact-face cap filled its inner wire at {centroid:?}"
                );
            }
        }

        let mut changed_signature = signature;
        changed_signature.centroid.x += 1.0;
        let broken_reference = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 3,
                errors: Vec::new(),
                jobs: vec![
                    base_job,
                    KernelJobDto::Extrude(KernelExtrudeJobDto {
                        feature_id: FeatureId(3),
                        operation: ExtrudeOperation::NewBody,
                        source_face: Some(KernelPlanarFaceSourceDto {
                            body_id: BodyId(1),
                            face_id: FaceId(42),
                            face_key: source.key.clone(),
                            signature: changed_signature,
                        }),
                        profiles: Vec::new(),
                        normal: basis.normal.into(),
                        start_offset: 0.0,
                        end_offset: 5.0,
                        taper_angle_deg: 0.0,
                        target_body_ids: Vec::new(),
                        result_body_ids: vec![BodyId(2)],
                    }),
                ],
            })
            .unwrap();
        assert_eq!(broken_reference.errors.len(), 1);
        assert_eq!(broken_reference.errors[0].feature_id, FeatureId(3));
        assert!(
            broken_reference.errors[0]
                .message
                .contains("source face changed or no longer exists"),
            "unexpected broken-reference diagnostic: {:?}",
            broken_reference.errors
        );
    }

    #[test]
    fn occt_modeled_through_thread_removes_the_predrill_core() {
        let mut kernel = OcctKernel::new().unwrap();
        let base_job = box_job(2, 1);
        let hole_job = KernelJobDto::Hole(KernelHoleJobDto {
            feature_id: FeatureId(3),
            target_body_id: BodyId(1),
            center: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: 10.0,
            },
            direction: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
            diameter: 5.0,
            extent: HoleExtent::ThroughAll,
            style: HoleStyle::Simple,
            counterbore_diameter: 0.0,
            counterbore_depth: 0.0,
            countersink_diameter: 0.0,
            countersink_angle_deg: 90.0,
            bottom_style: HoleBottomStyle::Flat,
            drill_point_angle_deg: 118.0,
            thread: Some(HoleThreadDto {
                standard: HoleThreadStandard::IsoMetric,
                series: HoleThreadSeries::MetricCoarse,
                designation: "M6 x 1 - 6H".to_string(),
                class: "6H".to_string(),
                nominal_diameter: 6.0,
                pitch: 1.0,
                threads_per_inch: None,
                hand: HoleThreadHand::Right,
                depth: None,
                representation: HoleThreadRepresentation::Modeled,
                tap_drill_designation: Some("5 mm".to_string()),
            }),
        });
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 3,
                errors: Vec::new(),
                jobs: vec![base_job.clone(), hole_job.clone()],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_m6_6h_modeled_thread_go_no_go_envelope(&scene, "before Split Body");
        let step =
            String::from_utf8(kernel.export_step(&StepExportRequest::default()).unwrap()).unwrap();
        assert_eq!(
            step.matches("MANIFOLD_SOLID_BREP").count(),
            1,
            "threaded-hole STEP must contain one connected solid"
        );
        assert!(
            step.contains("B_SPLINE_CURVE_WITH_KNOTS") || step.contains("SURFACE_CURVE"),
            "threaded-hole STEP must contain helical B-rep geometry"
        );

        let split_scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 4,
                errors: Vec::new(),
                jobs: vec![
                    base_job,
                    hole_job,
                    KernelJobDto::SplitBody(KernelSplitBodyJobDto {
                        feature_id: FeatureId(4),
                        target_body_id: BodyId(1),
                        plane_origin: Point3Dto {
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        plane_normal: Point3Dto {
                            x: 1.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        new_body_id: BodyId(2),
                    }),
                ],
            })
            .unwrap();
        assert!(split_scene.errors.is_empty(), "{:?}", split_scene.errors);
        assert_eq!(
            split_scene.bodies.len(),
            2,
            "Split Body must retain both halves"
        );
        assert_m6_6h_modeled_thread_go_no_go_envelope(&split_scene, "after Split Body");
        let split_step =
            String::from_utf8(kernel.export_step(&StepExportRequest::default()).unwrap()).unwrap();
        assert_eq!(
            split_step.matches("MANIFOLD_SOLID_BREP").count(),
            2,
            "split threaded-hole STEP must contain two connected solids"
        );
    }

    #[test]
    fn occt_models_an_external_thread_on_an_exact_cylindrical_face() {
        let limits = iso_metric_grade6_envelope(6.0, 1.0, ThreadFit::External).unwrap();
        assert_eq!(limits.modeled_major, limits.major_max);
        assert_eq!(limits.modeled_pitch, limits.pitch_max);
        assert_eq!(limits.modeled_minor, limits.minor_max);
        assert!(limits.modeled_major >= limits.major_min);
        assert!(limits.modeled_pitch >= limits.pitch_min);
        assert!(limits.modeled_minor >= limits.minor_min);

        let circle_profile = KernelProfileDto {
            profile_index: 0,
            points: (0..64)
                .map(|index| {
                    let angle = std::f64::consts::TAU * index as f64 / 64.0;
                    Point3Dto {
                        x: 3.0 * angle.cos(),
                        y: 3.0 * angle.sin(),
                        z: 0.0,
                    }
                })
                .collect(),
            curves: vec![KernelCurveDto::Circle {
                entity_id: 1,
                center: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                axis_point: Point3Dto {
                    x: 3.0,
                    y: 0.0,
                    z: 0.0,
                },
                normal: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
            }],
            holes: Vec::new(),
        };
        let base_job = KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(2),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![circle_profile],
            normal: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            start_offset: 0.0,
            end_offset: 10.0,
            taper_angle_deg: 0.0,
            target_body_ids: Vec::new(),
            result_body_ids: vec![BodyId(1)],
        });
        let mut kernel = OcctKernel::new().unwrap();
        let base_scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![base_job.clone()],
            })
            .unwrap();
        let shaft_face = base_scene.bodies[0]
            .faces
            .iter()
            .find(|face| {
                face.cylinder
                    .as_ref()
                    .is_some_and(|cylinder| (cylinder.radius - 3.0).abs() < 1e-6)
            })
            .expect("extruded shaft must expose an analytic cylinder");
        let face_key = shaft_face.key.clone();
        let cylinder: CylindricalSurfaceDto = shaft_face.cylinder.clone().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 2,
                errors: Vec::new(),
                jobs: vec![
                    base_job.clone(),
                    KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
                        feature_id: FeatureId(3),
                        target_body_id: BodyId(1),
                        face_key: face_key.clone(),
                        cylinder: cylinder.clone(),
                        thread: HoleThreadDto {
                            standard: HoleThreadStandard::IsoMetric,
                            series: HoleThreadSeries::MetricCoarse,
                            designation: "M6 x 1 - 6g".to_string(),
                            class: "6g".to_string(),
                            nominal_diameter: 6.0,
                            pitch: 1.0,
                            threads_per_inch: None,
                            hand: HoleThreadHand::Right,
                            depth: None,
                            representation: HoleThreadRepresentation::Modeled,
                            tap_drill_designation: None,
                        },
                        flip: false,
                    }),
                ],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        let body = &scene.bodies[0];
        assert!(
            body.faces.len() > 3,
            "modeled thread must add helical faces"
        );
        assert!(
            body.edges.len() > 3,
            "modeled thread must add helical edges"
        );
        let wall_radii = body
            .positions
            .chunks_exact(3)
            .filter_map(|point| {
                let radius = f64::from(point[0]).hypot(f64::from(point[1]));
                (point[2] > 0.1 && point[2] < 9.9).then_some(radius)
            })
            .collect::<Vec<_>>();
        let minimum_wall_radius = wall_radii.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum_wall_radius = wall_radii.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let edge_radii = body
            .edges
            .iter()
            .flat_map(|edge| edge.points.iter())
            .filter_map(|point| {
                let radius = point.x.hypot(point.y);
                (point.z > 0.1 && point.z < 9.9).then_some(radius)
            })
            .collect::<Vec<_>>();
        let minimum_edge_radius = edge_radii.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum_edge_radius = edge_radii.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mesh_tolerance = 0.03;
        assert!(
            minimum_wall_radius >= limits.minor_min * 0.5 - mesh_tolerance
                && minimum_wall_radius <= limits.modeled_minor * 0.5 + mesh_tolerance,
            "external 6g root radius {minimum_wall_radius} falls outside the GO/NO-GO envelope {}..{}; edge radius {minimum_edge_radius}..{maximum_edge_radius}",
            limits.minor_min * 0.5,
            limits.modeled_minor * 0.5,
        );
        assert!(
            (minimum_wall_radius - limits.modeled_minor * 0.5).abs() <= mesh_tolerance,
            "external 6g root radius {minimum_wall_radius} does not reach the maximum-material GO boundary {}",
            limits.modeled_minor * 0.5,
        );
        assert!(
            maximum_wall_radius >= limits.major_min * 0.5 - mesh_tolerance
                && maximum_wall_radius <= limits.modeled_major * 0.5 + mesh_tolerance,
            "external 6g crest radius {maximum_wall_radius} falls outside the GO/NO-GO envelope {}..{}",
            limits.major_min * 0.5,
            limits.modeled_major * 0.5,
        );
        assert!(
            (maximum_wall_radius - limits.modeled_major * 0.5).abs() <= mesh_tolerance,
            "external 6g crest radius {maximum_wall_radius} does not reach the maximum-material GO boundary {}",
            limits.modeled_major * 0.5,
        );
        let right_hand_score = thread_handedness_score(body);
        assert!(
            right_hand_score > 0.1,
            "right-hand external thread has the wrong helical direction: {right_hand_score}"
        );
        let step =
            String::from_utf8(kernel.export_step(&StepExportRequest::default()).unwrap()).unwrap();
        assert_eq!(
            step.matches("MANIFOLD_SOLID_BREP").count(),
            1,
            "external thread STEP must contain one connected solid"
        );
        assert!(
            step.contains("B_SPLINE_CURVE_WITH_KNOTS") || step.contains("SURFACE_CURVE"),
            "external thread STEP must contain helical B-rep geometry"
        );

        let left_scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 3,
                errors: Vec::new(),
                jobs: vec![
                    base_job,
                    KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
                        feature_id: FeatureId(3),
                        target_body_id: BodyId(1),
                        face_key,
                        cylinder,
                        thread: HoleThreadDto {
                            standard: HoleThreadStandard::IsoMetric,
                            series: HoleThreadSeries::MetricCoarse,
                            designation: "M6 x 1 - 6g LH".to_string(),
                            class: "6g".to_string(),
                            nominal_diameter: 6.0,
                            pitch: 1.0,
                            threads_per_inch: None,
                            hand: HoleThreadHand::Left,
                            depth: None,
                            representation: HoleThreadRepresentation::Modeled,
                            tap_drill_designation: None,
                        },
                        flip: false,
                    }),
                ],
            })
            .unwrap();
        assert!(left_scene.errors.is_empty(), "{:?}", left_scene.errors);
        let left_body = &left_scene.bodies[0];
        let left_hand_score = thread_handedness_score(left_body);
        assert!(
            left_hand_score < -0.1,
            "left-hand external thread has the wrong helical direction: {left_hand_score}"
        );
        let left_wall_radii = left_body
            .positions
            .chunks_exact(3)
            .filter_map(|point| {
                let radius = f64::from(point[0]).hypot(f64::from(point[1]));
                (point[2] > 0.1 && point[2] < 9.9).then_some(radius)
            })
            .collect::<Vec<_>>();
        let left_minimum_radius = left_wall_radii
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        let left_maximum_radius = left_wall_radii
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (left_minimum_radius - limits.modeled_minor * 0.5).abs() <= mesh_tolerance,
            "left-hand external 6g root {left_minimum_radius} misses the GO boundary {}",
            limits.modeled_minor * 0.5,
        );
        assert!(
            (left_maximum_radius - limits.modeled_major * 0.5).abs() <= mesh_tolerance,
            "left-hand external 6g crest {left_maximum_radius} misses the GO boundary {}",
            limits.modeled_major * 0.5,
        );
    }

    #[test]
    fn occt_models_a_full_length_m10_thread_on_a_reversed_axis_cylinder() {
        let limits = iso_metric_grade6_envelope(10.0, 1.5, ThreadFit::External).unwrap();
        assert_eq!(limits.modeled_major, limits.major_max);
        assert_eq!(limits.modeled_pitch, limits.pitch_max);
        assert_eq!(limits.modeled_minor, limits.minor_max);

        let circle_profile = KernelProfileDto {
            profile_index: 0,
            points: (0..64)
                .map(|index| {
                    let angle = std::f64::consts::TAU * index as f64 / 64.0;
                    Point3Dto {
                        x: 5.0 * angle.cos(),
                        y: 5.0 * angle.sin(),
                        z: 0.0,
                    }
                })
                .collect(),
            curves: vec![KernelCurveDto::Circle {
                entity_id: 1,
                center: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                axis_point: Point3Dto {
                    x: 5.0,
                    y: 0.0,
                    z: 0.0,
                },
                normal: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
            }],
            holes: Vec::new(),
        };
        let base_job = KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(2),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![circle_profile],
            normal: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            start_offset: 0.0,
            end_offset: 20.0,
            taper_angle_deg: 0.0,
            target_body_ids: Vec::new(),
            result_body_ids: vec![BodyId(1)],
        });
        let mut kernel = OcctKernel::new().unwrap();
        let base_scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![base_job.clone()],
            })
            .unwrap();
        let shaft_face = base_scene.bodies[0]
            .faces
            .iter()
            .find(|face| {
                face.cylinder
                    .as_ref()
                    .is_some_and(|cylinder| (cylinder.radius - 5.0).abs() < 1e-6)
            })
            .expect("extruded M10 shaft must expose an analytic cylinder");
        let cylinder = shaft_face.cylinder.clone().unwrap();
        assert!(
            cylinder.axis.z < -0.99,
            "regression requires the OCCT cylinder's reversed -Z axis, got {:?}",
            cylinder.axis
        );

        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 2,
                errors: Vec::new(),
                jobs: vec![
                    base_job,
                    KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
                        feature_id: FeatureId(3),
                        target_body_id: BodyId(1),
                        face_key: shaft_face.key.clone(),
                        cylinder,
                        thread: HoleThreadDto {
                            standard: HoleThreadStandard::IsoMetric,
                            series: HoleThreadSeries::MetricCoarse,
                            designation: "M10 x 1.5 - 6g".to_string(),
                            class: "6g".to_string(),
                            nominal_diameter: 10.0,
                            pitch: 1.5,
                            threads_per_inch: None,
                            hand: HoleThreadHand::Right,
                            depth: None,
                            representation: HoleThreadRepresentation::Modeled,
                            tap_drill_designation: None,
                        },
                        flip: false,
                    }),
                ],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        let body = &scene.bodies[0];
        assert!(
            body.faces.len() > 3,
            "modeled M10 thread adds helical faces"
        );
        assert!(
            body.edges.len() > 3,
            "modeled M10 thread adds helical edges"
        );
        let wall_radii = body
            .positions
            .chunks_exact(3)
            .filter_map(|point| {
                let radius = f64::from(point[0]).hypot(f64::from(point[1]));
                (point[2] > 0.1 && point[2] < 19.9).then_some(radius)
            })
            .collect::<Vec<_>>();
        let minimum_wall_radius = wall_radii.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum_wall_radius = wall_radii.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mesh_tolerance = 0.04;
        assert!(
            minimum_wall_radius >= limits.minor_min * 0.5 - mesh_tolerance
                && minimum_wall_radius <= limits.modeled_minor * 0.5 + mesh_tolerance,
            "reversed-axis M10 6g root radius {minimum_wall_radius} falls outside the GO/NO-GO envelope {}..{}",
            limits.minor_min * 0.5,
            limits.modeled_minor * 0.5,
        );
        assert!(
            (minimum_wall_radius - limits.modeled_minor * 0.5).abs() <= mesh_tolerance,
            "reversed-axis M10 6g root radius {minimum_wall_radius} does not reach the maximum-material GO boundary {}",
            limits.modeled_minor * 0.5,
        );
        assert!(
            maximum_wall_radius >= limits.major_min * 0.5 - mesh_tolerance
                && maximum_wall_radius <= limits.modeled_major * 0.5 + mesh_tolerance,
            "reversed-axis M10 6g crest radius {maximum_wall_radius} falls outside the GO/NO-GO envelope {}..{}",
            limits.major_min * 0.5,
            limits.modeled_major * 0.5,
        );
        assert!(
            (maximum_wall_radius - limits.modeled_major * 0.5).abs() <= mesh_tolerance,
            "reversed-axis M10 6g crest radius {maximum_wall_radius} does not reach the maximum-material GO boundary {}",
            limits.modeled_major * 0.5,
        );

        let minimum_material_cap_area = std::f64::consts::PI * (limits.modeled_minor * 0.5).powi(2);
        let maximum_material_cap_area = std::f64::consts::PI * (limits.modeled_major * 0.5).powi(2);
        for expected_z in [0.0, 20.0] {
            let preserved_area = body
                .faces
                .iter()
                .filter_map(|face| face.signature.as_ref())
                .filter(|signature| {
                    signature.normal.z.abs() > 0.99
                        && (signature.centroid.z - expected_z).abs() < 1e-5
                })
                .map(|signature| signature.area)
                .sum::<f64>();
            assert!(
                preserved_area > minimum_material_cap_area * 0.98
                    && preserved_area < maximum_material_cap_area * 1.02,
                "modeled M10 thread end at z={expected_z} must remain inside its ISO 6g radial envelope; cap area {preserved_area} vs {minimum_material_cap_area}..{maximum_material_cap_area}"
            );
        }
        let step =
            String::from_utf8(kernel.export_step(&StepExportRequest::default()).unwrap()).unwrap();
        assert_eq!(
            step.matches("MANIFOLD_SOLID_BREP").count(),
            1,
            "full-length M10 thread must remain one connected solid"
        );
    }

    #[test]
    fn occt_builds_multi_edge_refinements_and_all_first_hole_styles() {
        let refinements = vec![
            KernelJobDto::Fillet(KernelFilletJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                edge_keys: vec!["edge:0".to_string(), "edge:1".to_string()],
                radius: 1.0,
                tangent_chain: false,
            }),
            KernelJobDto::Chamfer(KernelChamferJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                edge_keys: vec!["edge:0".to_string(), "edge:1".to_string()],
                distance: 1.0,
                tangent_chain: false,
            }),
            KernelJobDto::Hole(KernelHoleJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                center: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 10.0,
                },
                direction: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: -1.0,
                },
                diameter: 4.0,
                extent: HoleExtent::Distance { depth: 5.0 },
                style: HoleStyle::Simple,
                counterbore_diameter: 0.0,
                counterbore_depth: 0.0,
                countersink_diameter: 0.0,
                countersink_angle_deg: 90.0,
                bottom_style: HoleBottomStyle::DrillPoint,
                drill_point_angle_deg: 118.0,
                thread: None,
            }),
            KernelJobDto::Hole(KernelHoleJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                center: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 10.0,
                },
                direction: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: -1.0,
                },
                diameter: 4.0,
                extent: HoleExtent::ThroughAll,
                style: HoleStyle::Simple,
                counterbore_diameter: 0.0,
                counterbore_depth: 0.0,
                countersink_diameter: 0.0,
                countersink_angle_deg: 90.0,
                bottom_style: HoleBottomStyle::Flat,
                drill_point_angle_deg: 118.0,
                thread: None,
            }),
            KernelJobDto::Hole(KernelHoleJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                center: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 10.0,
                },
                direction: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: -1.0,
                },
                diameter: 4.0,
                extent: HoleExtent::Distance { depth: 8.0 },
                style: HoleStyle::Counterbore,
                counterbore_diameter: 8.0,
                counterbore_depth: 2.0,
                countersink_diameter: 0.0,
                countersink_angle_deg: 90.0,
                bottom_style: HoleBottomStyle::Flat,
                drill_point_angle_deg: 118.0,
                thread: None,
            }),
            KernelJobDto::Hole(KernelHoleJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                center: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 10.0,
                },
                direction: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: -1.0,
                },
                diameter: 4.0,
                extent: HoleExtent::Distance { depth: 8.0 },
                style: HoleStyle::Countersink,
                counterbore_diameter: 0.0,
                counterbore_depth: 0.0,
                countersink_diameter: 8.0,
                countersink_angle_deg: 90.0,
                bottom_style: HoleBottomStyle::Flat,
                drill_point_angle_deg: 118.0,
                thread: None,
            }),
            KernelJobDto::Hole(KernelHoleJobDto {
                feature_id: FeatureId(3),
                target_body_id: BodyId(1),
                center: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 10.0,
                },
                direction: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: -1.0,
                },
                diameter: 5.0,
                extent: HoleExtent::Distance { depth: 8.0 },
                style: HoleStyle::Simple,
                counterbore_diameter: 0.0,
                counterbore_depth: 0.0,
                countersink_diameter: 0.0,
                countersink_angle_deg: 90.0,
                bottom_style: HoleBottomStyle::DrillPoint,
                drill_point_angle_deg: 118.0,
                thread: Some(HoleThreadDto {
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
                }),
            }),
        ];

        for refinement in refinements {
            let feature_id = refinement.feature_id();
            let mut kernel = OcctKernel::new().unwrap();
            let scene = kernel
                .recompute(&RecomputePlanDto {
                    transaction_id: feature_id.0,
                    errors: Vec::new(),
                    jobs: vec![box_job(2, 1), refinement],
                })
                .unwrap();
            assert!(
                scene.errors.is_empty(),
                "feature {feature_id:?} failed: {:?}",
                scene.errors
            );
            assert_eq!(scene.bodies.len(), 1);
            assert!(!scene.bodies[0].indices.is_empty());
            assert_ne!(scene.bodies[0].edges.len(), 12);
        }
    }

    #[test]
    fn occt_revolves_and_meshes_a_profile() {
        let mut kernel = OcctKernel::new().unwrap();
        let plan = RecomputePlanDto {
            transaction_id: 1,
            errors: Vec::new(),
            jobs: vec![KernelJobDto::Revolve(KernelRevolveJobDto {
                feature_id: FeatureId(2),
                operation: ExtrudeOperation::NewBody,
                profiles: vec![KernelProfileDto {
                    profile_index: 0,
                    points: vec![
                        Point3Dto {
                            x: 10.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 20.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 20.0,
                            y: 15.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 10.0,
                            y: 15.0,
                            z: 0.0,
                        },
                    ],
                    curves: Vec::new(),
                    holes: Vec::new(),
                }],
                axis_origin: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                axis_direction: Point3Dto {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
                angle_rad: std::f64::consts::TAU,
                target_body_ids: Vec::new(),
                result_body_ids: vec![BodyId(1)],
            })],
        };
        let scene = kernel.recompute(&plan).unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_eq!(scene.bodies.len(), 1);
        assert!(!scene.bodies[0].indices.is_empty());
        assert!(scene.bodies[0]
            .faces
            .iter()
            .any(|face| face.plane.is_none()));
    }

    #[test]
    fn occt_sweeps_lofts_and_builds_ribs() {
        let cases = vec![
            KernelJobDto::Sweep(KernelSweepJobDto {
                feature_id: FeatureId(3),
                operation: ExtrudeOperation::NewBody,
                profile: square(0.0, 3.0),
                path: vec![KernelCurveDto::Polyline {
                    entity_id: 1,
                    points: vec![
                        Point3Dto {
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 0.0,
                            y: 0.0,
                            z: 20.0,
                        },
                        Point3Dto {
                            x: 10.0,
                            y: 0.0,
                            z: 30.0,
                        },
                    ],
                }],
                guide_rail: Vec::new(),
                orientation: SweepOrientation::CorrectedFrenet,
                transition: SweepTransition::Transformed,
                force_c1: false,
                target_body_ids: Vec::new(),
                result_body_ids: vec![BodyId(1)],
            }),
            KernelJobDto::Loft(KernelLoftJobDto {
                feature_id: FeatureId(4),
                operation: ExtrudeOperation::NewBody,
                sections: vec![square(0.0, 3.0), square(20.0, 7.0)],
                ruled: false,
                continuity: LoftContinuity::G1,
                centerline: Vec::new(),
                guide_rail: Vec::new(),
                target_body_ids: Vec::new(),
                result_body_ids: vec![BodyId(1)],
            }),
            KernelJobDto::Rib(KernelRibJobDto {
                feature_id: FeatureId(5),
                operation: ExtrudeOperation::NewBody,
                profiles: vec![KernelProfileDto {
                    profile_index: 0,
                    points: vec![
                        Point3Dto {
                            x: -10.0,
                            y: -1.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 10.0,
                            y: -1.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: 10.0,
                            y: 1.0,
                            z: 0.0,
                        },
                        Point3Dto {
                            x: -10.0,
                            y: 1.0,
                            z: 0.0,
                        },
                    ],
                    curves: Vec::new(),
                    holes: Vec::new(),
                }],
                normal: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                start_offset: 0.0,
                end_offset: 12.0,
                target_body_ids: Vec::new(),
                result_body_ids: vec![BodyId(1)],
            }),
        ];
        for job in cases {
            let mut kernel = OcctKernel::new().unwrap();
            let scene = kernel
                .recompute(&RecomputePlanDto {
                    transaction_id: 1,
                    errors: Vec::new(),
                    jobs: vec![job],
                })
                .unwrap();
            assert!(scene.errors.is_empty(), "{:?}", scene.errors);
            assert_eq!(scene.bodies.len(), 1);
            assert!(!scene.bodies[0].indices.is_empty());
        }
    }

    #[test]
    fn occt_applies_revolve_boolean_to_an_existing_body() {
        let base = KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(2),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![KernelProfileDto {
                profile_index: 0,
                points: vec![
                    Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Point3Dto {
                        x: 20.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Point3Dto {
                        x: 20.0,
                        y: 20.0,
                        z: 0.0,
                    },
                    Point3Dto {
                        x: 0.0,
                        y: 20.0,
                        z: 0.0,
                    },
                ],
                curves: Vec::new(),
                holes: Vec::new(),
            }],
            normal: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            start_offset: -10.0,
            end_offset: 10.0,
            taper_angle_deg: 0.0,
            target_body_ids: Vec::new(),
            result_body_ids: vec![BodyId(1)],
        });
        let cut = KernelJobDto::Revolve(KernelRevolveJobDto {
            feature_id: FeatureId(3),
            operation: ExtrudeOperation::Cut,
            profiles: vec![KernelProfileDto {
                profile_index: 0,
                points: vec![
                    Point3Dto {
                        x: 4.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Point3Dto {
                        x: 8.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Point3Dto {
                        x: 8.0,
                        y: 20.0,
                        z: 0.0,
                    },
                    Point3Dto {
                        x: 4.0,
                        y: 20.0,
                        z: 0.0,
                    },
                ],
                curves: Vec::new(),
                holes: Vec::new(),
            }],
            axis_origin: Point3Dto {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            axis_direction: Point3Dto {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            angle_rad: std::f64::consts::TAU,
            target_body_ids: vec![BodyId(1)],
            result_body_ids: vec![BodyId(1)],
        });
        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                errors: Vec::new(),
                jobs: vec![base, cut],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_eq!(
            scene
                .bodies
                .iter()
                .map(|body| body.body_id)
                .collect::<Vec<_>>(),
            vec![BodyId(1)]
        );
        assert!(!scene.bodies[0].indices.is_empty());
    }
}
