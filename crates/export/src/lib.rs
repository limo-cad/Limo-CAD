//! Manufacturing mesh export: binary STL and 3MF with materials + slicer metadata.
//!
//! Tessellation is owned by the OCCT (or browser) kernel. This crate turns
//! triangle soups + [`BodyAppearance`] into file bytes so UI and MCP share one
//! writer ([`ExportFacade`]).

mod facade;
mod instances;
mod materials;
mod mesh_weld;
mod pip_demo;
mod slicer;
mod stl;
mod threemf;

use nbcad_core::BodyId;
use nbcad_solid::KernelBodyDto;
use serde::{Deserialize, Serialize};

pub use facade::ExportFacade;
pub use instances::{prepare_export_meshes, MeshInstance};
pub use materials::{
    brands, catalog_json, find_preset, material_catalog, presets_for_brand, MaterialPreset,
};
pub use mesh_weld::{
    boundary_edge_count, invalid_model_edge_count, validate_3mf_model_mesh, weld_triangle_mesh,
    DEFAULT_WELD_EPSILON,
};
pub use pip_demo::{
    assert_box_clearances, print_in_place_cam_bolt, print_in_place_clip, print_in_place_latch,
    CLEAR_MM,
};
pub use slicer::SlicerTarget;
pub use stl::write_stl;
pub use threemf::write_3mf;

pub const DEFAULT_LINEAR_DEFLECTION: f64 = 0.15;
pub const DEFAULT_ANGULAR_DEFLECTION: f64 = 0.35;

/// Mesh export selection. An empty body list means every tessellated body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshExportScope {
    /// Visible solved occurrences, preserving repeats and assembly placement.
    #[default]
    Assembly,
    /// Each selected retained body once, in its original part coordinates.
    Definition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshExportRequest {
    /// Optional optimistic precondition captured before interactive choices.
    /// Hosts compare it while holding the same ownership lock as mesh export.
    #[serde(default)]
    pub expected_model_json: Option<String>,
    #[serde(default)]
    pub body_ids: Vec<BodyId>,
    #[serde(default)]
    pub scope: MeshExportScope,
    #[serde(default = "default_linear")]
    pub linear_deflection: f64,
    #[serde(default = "default_angular")]
    pub angular_deflection: f64,
    /// 3MF only; STL ignores appearance.
    #[serde(default = "default_true")]
    pub include_appearance: bool,
    /// Slicer ecosystem metadata to embed alongside consortium 3MF materials.
    #[serde(default = "default_slicer_target")]
    pub slicer_target: SlicerTarget,
}

fn default_linear() -> f64 {
    DEFAULT_LINEAR_DEFLECTION
}

fn default_angular() -> f64 {
    DEFAULT_ANGULAR_DEFLECTION
}

fn default_true() -> bool {
    true
}

fn default_slicer_target() -> SlicerTarget {
    SlicerTarget::BambuStudio
}

impl Default for MeshExportRequest {
    fn default() -> Self {
        Self {
            expected_model_json: None,
            body_ids: Vec::new(),
            scope: MeshExportScope::Assembly,
            linear_deflection: DEFAULT_LINEAR_DEFLECTION,
            angular_deflection: DEFAULT_ANGULAR_DEFLECTION,
            include_appearance: true,
            slicer_target: SlicerTarget::BambuStudio,
        }
    }
}

impl MeshExportRequest {
    pub fn check_model_snapshot(&self, current: &str) -> Result<(), ExportError> {
        nbcad_solid::check_export_model_snapshot(self.expected_model_json.as_deref(), current)
            .map_err(|message| ExportError(message.into()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TriangleMesh {
    pub body_id: BodyId,
    pub name: String,
    pub positions: Vec<f32>,
    pub indices: Vec<u32>,
}

impl TriangleMesh {
    pub fn from_kernel_body(body: &KernelBodyDto, name: impl Into<String>) -> Self {
        Self {
            body_id: body.body_id,
            name: name.into(),
            positions: body.positions.clone(),
            indices: body.indices.clone(),
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError(pub String);

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ExportError {}

/// Convenience: write 3MF with default standard target (backward compatible).
pub fn write_3mf_standard(
    meshes: &[TriangleMesh],
    appearances: &[nbcad_core::BodyAppearance],
    include_appearance: bool,
) -> Result<Vec<u8>, ExportError> {
    write_3mf(
        meshes,
        appearances,
        include_appearance,
        SlicerTarget::Standard,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nbcad_core::{BodyAppearance, BodyId, Rgba8};
    use std::io::Cursor;

    /// Closed 20 mm cube (watertight). A single quad is rejected by slicers as
    /// zero volume / no geometry.
    fn unit_cube(body_id: u64) -> TriangleMesh {
        let s = 20.0_f32;
        TriangleMesh {
            body_id: BodyId(body_id),
            name: format!("Body{body_id}"),
            // 8 corners: bottom z=0, top z=s
            positions: vec![
                0.0, 0.0, 0.0, // 0
                s, 0.0, 0.0, // 1
                s, s, 0.0, // 2
                0.0, s, 0.0, // 3
                0.0, 0.0, s, // 4
                s, 0.0, s, // 5
                s, s, s, // 6
                0.0, s, s, // 7
            ],
            // Outward CCW winding when viewed from outside.
            indices: vec![
                // bottom (z=0, normal -Z)
                0, 2, 1, 0, 3, 2, // top (z=s, normal +Z)
                4, 5, 6, 4, 6, 7, // front (y=0, normal -Y)
                0, 1, 5, 0, 5, 4, // back (y=s, normal +Y)
                3, 7, 6, 3, 6, 2, // left (x=0, normal -X)
                0, 4, 7, 0, 7, 3, // right (x=s, normal +X)
                1, 2, 6, 1, 6, 5,
            ],
        }
    }

    /// OCCT-style cube: 12 triangles × 3 unique positions each (36 verts, no shared indices).
    fn unwelded_unit_cube(body_id: u64) -> TriangleMesh {
        let s = 20.0_f32;
        let corners: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [s, 0.0, 0.0],
            [s, s, 0.0],
            [0.0, s, 0.0],
            [0.0, 0.0, s],
            [s, 0.0, s],
            [s, s, s],
            [0.0, s, s],
        ];
        let tri_corners: [[usize; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [3, 7, 6],
            [3, 6, 2],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ];
        let mut positions = Vec::with_capacity(36 * 3);
        let mut indices = Vec::with_capacity(36);
        for tri in tri_corners {
            let base = (positions.len() / 3) as u32;
            for corner_idx in tri {
                positions.extend_from_slice(&corners[corner_idx]);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
        TriangleMesh {
            body_id: BodyId(body_id),
            name: format!("Body{body_id}"),
            positions,
            indices,
        }
    }

    fn count_3mf_vertices(xml: &str) -> usize {
        xml.matches("<vertex ").count()
    }

    fn count_3mf_triangles(xml: &str) -> usize {
        xml.matches("<triangle ").count()
    }

    fn red_pla(body_id: u64) -> BodyAppearance {
        let mut appearance = find_preset("bambu.pla.basic.red")
            .unwrap()
            .to_appearance(BodyId(body_id));
        appearance.color = Rgba8::opaque(200, 40, 40);
        appearance
    }

    #[test]
    fn binary_stl_has_header_and_triangle_count() {
        let bytes = write_stl(&[unit_cube(1)]).unwrap();
        assert!(bytes.len() >= 84);
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap());
        assert_eq!(count, 12);
        assert_eq!(bytes.len(), 84 + 12 * 50);
    }

    #[test]
    fn unit_cube_mesh_is_closed_volume() {
        let mesh = unit_cube(1);
        assert_eq!(mesh.positions.len(), 8 * 3);
        assert_eq!(mesh.indices.len(), 12 * 3);
        let (mut min_z, mut max_z) = (f32::MAX, f32::MIN);
        for chunk in mesh.positions.chunks_exact(3) {
            min_z = min_z.min(chunk[2]);
            max_z = max_z.max(chunk[2]);
        }
        assert!((max_z - min_z - 20.0).abs() < 1e-3);
    }

    #[test]
    fn threemf_standard_includes_millimeter_and_basematerial() {
        let bytes =
            write_3mf(&[unit_cube(1)], &[red_pla(1)], true, SlicerTarget::Standard).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        {
            let mut model = archive.by_name("3D/3dmodel.model").unwrap();
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
            assert!(xml.contains(r#"unit="millimeter""#));
            assert!(xml.contains("<basematerials"));
            assert!(!xml.contains("<m:basematerials"));
            assert!(xml.contains("<base "));
            assert!(xml.contains("#C82828"));
        }
        assert!(archive.by_name("Metadata/project_settings.config").is_err());
    }

    #[test]
    fn unwelded_cube_welds_to_eight_vertices_on_3mf_export() {
        let raw = unwelded_unit_cube(1);
        assert_eq!(raw.positions.len() / 3, 36);
        assert_eq!(raw.triangle_count(), 12);
        assert!(
            boundary_edge_count(&raw) > 0,
            "unwelded OCCT-style soup should have boundary edges"
        );

        let welded = weld_triangle_mesh(&raw, DEFAULT_WELD_EPSILON).unwrap();
        assert_eq!(welded.positions.len() / 3, 8);
        assert_eq!(boundary_edge_count(&welded), 0);
        assert_eq!(invalid_model_edge_count(&welded), 0);

        let bytes = write_3mf(&[raw], &[red_pla(1)], true, SlicerTarget::Standard).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut model = archive.by_name("3D/3dmodel.model").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
        assert_eq!(count_3mf_vertices(&xml), 8);
        assert_eq!(count_3mf_triangles(&xml), 12);
    }

    #[test]
    fn threemf_bambu_embeds_filament_project_settings() {
        let bytes = ExportFacade::export_3mf_for_target(
            &[unit_cube(1)],
            &[red_pla(1)],
            true,
            SlicerTarget::BambuStudio,
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        {
            let mut settings = archive.by_name("Metadata/project_settings.config").unwrap();
            let mut json = String::new();
            std::io::Read::read_to_string(&mut settings, &mut json).unwrap();
            assert!(json.contains("filament_colour"));
            assert!(json.contains("filament_vendor"));
            assert!(json.contains("#C82828"));
            assert!(json.contains("\"PLA\""));
            assert!(json.contains("Bambu Lab X1 Carbon"));
        }
        assert!(archive.by_name("Metadata/model_settings.config").is_ok());
    }

    #[test]
    fn threemf_bambu_maps_each_body_to_extruder_slot() {
        let blue = BodyAppearance {
            body_id: BodyId(2),
            color: nbcad_core::Rgba8::opaque(40, 90, 200),
            material_name: "Bambu PLA Basic".into(),
            filament_type: "PLA".into(),
            brand: "Bambu Lab".into(),
            color_name: "Blue".into(),
            filament_id: Some("GFA00".into()),
            preset_id: Some("bambu.pla.basic.blue".into()),
            density_g_cm3: Some(1.24),
            diameter_mm: 1.75,
        };
        let bytes = write_3mf(
            &[unit_cube(1), unit_cube(2)],
            &[red_pla(1), blue],
            true,
            SlicerTarget::BambuStudio,
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        {
            let mut settings = archive.by_name("Metadata/model_settings.config").unwrap();
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut settings, &mut xml).unwrap();
            assert!(xml.contains(r#"key="extruder" value="1""#));
            assert!(xml.contains(r#"key="extruder" value="2""#));
        }
        {
            let mut project = archive.by_name("Metadata/project_settings.config").unwrap();
            let mut json = String::new();
            std::io::Read::read_to_string(&mut project, &mut json).unwrap();
            assert!(json.contains("#C82828"));
            assert!(json.contains("#285AC8"));
        }
    }

    #[test]
    fn threemf_prusa_embeds_slic3r_config() {
        let bytes = write_3mf(
            &[unit_cube(1), unit_cube(2)],
            &[red_pla(1), red_pla(2)],
            true,
            SlicerTarget::PrusaSlicer,
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        {
            let mut config = archive.by_name("Metadata/Slic3r_PE.config").unwrap();
            let mut text = String::new();
            std::io::Read::read_to_string(&mut config, &mut text).unwrap();
            assert!(text.contains("filament_colour"));
            assert!(text.contains("#C82828"));
            assert!(text.contains("filament_diameter"));
        }
        {
            let mut model = archive.by_name("Metadata/Slic3r_PE_model.config").unwrap();
            let mut text = String::new();
            std::io::Read::read_to_string(&mut model, &mut text).unwrap();
            assert!(text.contains(r#"key="extruder""#));
            assert!(text.contains(r#"type="object""#));
            assert!(text.contains(r#"type="volume""#));
            assert!(text.contains("volume_type"));
        }
    }

    #[test]
    fn threemf_orca_mirrors_bambu_metadata_shape() {
        let bytes = write_3mf(
            &[unit_cube(1)],
            &[red_pla(1)],
            true,
            SlicerTarget::OrcaSlicer,
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name("Metadata/project_settings.config").is_ok());
    }

    #[test]
    fn threemf_cura_embeds_materials_hint_json() {
        let bytes = write_3mf(&[unit_cube(1)], &[red_pla(1)], true, SlicerTarget::Cura).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        {
            let mut model = archive.by_name("3D/3dmodel.model").unwrap();
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
            assert!(xml.contains("basematerials"));
            assert!(xml.contains("#C82828"));
        }
        {
            let mut hints = archive.by_name("Metadata/cura_materials.json").unwrap();
            let mut json = String::new();
            std::io::Read::read_to_string(&mut hints, &mut json).unwrap();
            assert!(json.contains("materials"));
            assert!(json.contains("#C82828"));
        }
    }

    #[test]
    fn facade_matches_direct_writers() {
        let meshes = [unit_cube(1)];
        let appearances = [red_pla(1)];
        let request = MeshExportRequest {
            slicer_target: SlicerTarget::Standard,
            ..Default::default()
        };
        let via_facade = ExportFacade::export_3mf(&meshes, &appearances, &request).unwrap();
        let direct = write_3mf(&meshes, &appearances, true, SlicerTarget::Standard).unwrap();
        assert_eq!(via_facade, direct);
    }

    #[test]
    fn print_in_place_demo_meshes_are_well_formed() {
        use std::io::Cursor;

        // Print-in-place drawer clip (housing + drawer + latch), AABB clearance smoke.
        let (pip_meshes, pip_apps) = print_in_place_clip();
        assert_eq!(pip_meshes.len(), 3);
        for target in [
            SlicerTarget::BambuStudio,
            SlicerTarget::OrcaSlicer,
            SlicerTarget::PrusaSlicer,
            SlicerTarget::Cura,
        ] {
            let bytes = write_3mf(&pip_meshes, &pip_apps, true, target).unwrap();
            let tri_floats: usize = pip_meshes.iter().map(|m| m.indices.len()).sum();
            assert!(
                bytes.len() > 2_500 && tri_floats >= 20 * 36,
                "clip mesh under-built for {target:?} ({} bytes, {} index floats)",
                bytes.len(),
                tri_floats
            );
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            assert!(archive.by_name("3D/3dmodel.model").is_ok());
        }

        // Four-body cam bolt (wedge drive + dial lock), AABB clearance smoke.
        let (cam_meshes, cam_apps) = print_in_place_cam_bolt();
        assert_eq!(cam_meshes.len(), 4);
        for target in [
            SlicerTarget::BambuStudio,
            SlicerTarget::OrcaSlicer,
            SlicerTarget::PrusaSlicer,
            SlicerTarget::Cura,
        ] {
            let bytes = write_3mf(&cam_meshes, &cam_apps, true, target).unwrap();
            let tri_floats: usize = cam_meshes.iter().map(|m| m.indices.len()).sum();
            assert!(
                bytes.len() > 3_000 && tri_floats >= 28 * 36,
                "cam-bolt mesh under-built for {target:?} ({} bytes, {} index floats)",
                bytes.len(),
                tri_floats
            );
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            assert!(archive.by_name("3D/3dmodel.model").is_ok());
        }
    }

    /// Regenerates `fixtures/smoke/*.3mf` for manual KR3.6 slicer open checks.
    /// Run explicitly: `cargo test -p nbcad-export regen_manual_smoke_fixtures -- --ignored --exact`
    #[test]
    #[ignore]
    fn regen_manual_smoke_fixtures() {
        use std::path::PathBuf;
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/smoke");
        std::fs::create_dir_all(&dir).unwrap();

        // Simple closed cube (geometry sanity).
        let cube_meshes = [unit_cube(1)];
        let cube_apps = [red_pla(1)];
        for (name, target) in [
            ("cube_bambu_studio.3mf", SlicerTarget::BambuStudio),
            ("cube_prusa_slicer.3mf", SlicerTarget::PrusaSlicer),
            ("cube_standard.3mf", SlicerTarget::Standard),
        ] {
            let bytes = write_3mf(&cube_meshes, &cube_apps, true, target).unwrap();
            std::fs::write(dir.join(name), bytes).unwrap();
        }

        // Print-in-place drawer clip (housing + drawer + latch), AABB clearance smoke.
        let (pip_meshes, pip_apps) = print_in_place_clip();
        assert_eq!(pip_meshes.len(), 3);
        for (name, target) in [
            ("print_in_place_clip_bambu.3mf", SlicerTarget::BambuStudio),
            ("print_in_place_clip_orca.3mf", SlicerTarget::OrcaSlicer),
            ("print_in_place_clip_prusa.3mf", SlicerTarget::PrusaSlicer),
            ("print_in_place_clip_cura.3mf", SlicerTarget::Cura),
            // Alias names kept for older smoke paths / docs links.
            ("print_in_place_latch_bambu.3mf", SlicerTarget::BambuStudio),
            ("print_in_place_latch_prusa.3mf", SlicerTarget::PrusaSlicer),
        ] {
            let bytes = write_3mf(&pip_meshes, &pip_apps, true, target).unwrap();
            let tri_floats: usize = pip_meshes.iter().map(|m| m.indices.len()).sum();
            assert!(
                bytes.len() > 2_500 && tri_floats >= 20 * 36,
                "{name} clip mesh under-built ({} bytes, {} index floats)",
                bytes.len(),
                tri_floats
            );
            std::fs::write(dir.join(name), &bytes).unwrap();
        }
        assert!(dir.join("print_in_place_clip_bambu.3mf").is_file());

        // Four-body cam bolt (wedge drive + dial lock), AABB clearance smoke.
        let (cam_meshes, cam_apps) = print_in_place_cam_bolt();
        assert_eq!(cam_meshes.len(), 4);
        for (name, target) in [
            (
                "print_in_place_cam_bolt_bambu.3mf",
                SlicerTarget::BambuStudio,
            ),
            ("print_in_place_cam_bolt_orca.3mf", SlicerTarget::OrcaSlicer),
            (
                "print_in_place_cam_bolt_prusa.3mf",
                SlicerTarget::PrusaSlicer,
            ),
            ("print_in_place_cam_bolt_cura.3mf", SlicerTarget::Cura),
        ] {
            let bytes = write_3mf(&cam_meshes, &cam_apps, true, target).unwrap();
            let tri_floats: usize = cam_meshes.iter().map(|m| m.indices.len()).sum();
            assert!(
                bytes.len() > 3_000 && tri_floats >= 28 * 36,
                "{name} cam-bolt mesh under-built ({} bytes, {} index floats)",
                bytes.len(),
                tri_floats
            );
            std::fs::write(dir.join(name), &bytes).unwrap();
        }
        assert!(dir.join("print_in_place_cam_bolt_bambu.3mf").is_file());
    }
}
