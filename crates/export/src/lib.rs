//! Manufacturing mesh export: precision-preserving STL and 3MF with slicer metadata.
//!
//! Tessellation is owned by the OCCT (or browser) kernel. This crate turns
//! triangle soups + [`BodyAppearance`] into file bytes so UI and MCP share one
//! writer ([`ExportFacade`]).

pub mod bambu_project;
mod facade;
#[cfg(not(target_arch = "wasm32"))]
pub mod manufacturing_report;
mod manufacturing_request;
#[cfg(not(target_arch = "wasm32"))]
pub mod slicer_verification;
pub use manufacturing_request::BambuExportRequest;
mod print_layout;
mod scene;
pub use print_layout::{analyze_print_layout, LayoutIssue, LayoutTranslation, PrintLayoutReport};
pub use scene::write_3mf_scene;
mod instances;
mod materials;
mod mesh_weld;
mod pip_demo;
pub mod profile_dxf;
mod slicer;
mod stl;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_reader;
mod threemf;

use limo_cad_core::BodyId;
use limo_cad_solid::KernelBodyDto;
use serde::{Deserialize, Serialize};

pub use facade::ExportFacade;
pub use instances::{prepare_export_meshes, MeshInstance};
pub use materials::{
    brands, catalog_json, find_preset, material_catalog, presets_for_brand,
    resolve_body_appearance, MaterialPreset,
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
    /// Saved named view used for occurrence placement and visibility.
    #[serde(default)]
    pub named_view: Option<String>,
    #[serde(default)]
    pub print_bed: Option<limo_cad_core::PrintBedDto>,
    #[serde(default = "default_linear")]
    pub linear_deflection: f64,
    #[serde(default = "default_angular")]
    pub angular_deflection: f64,
    /// 3MF only; STL ignores appearance.
    #[serde(default = "default_true")]
    pub include_appearance: bool,
    /// `standard`, `bambu_studio`, and `orca_slicer` write the portable model.
    /// `prusa_slicer` and `cura` add that slicer's metadata.
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
    SlicerTarget::Standard
}

impl Default for MeshExportRequest {
    fn default() -> Self {
        Self {
            expected_model_json: None,
            body_ids: Vec::new(),
            scope: MeshExportScope::Assembly,
            named_view: None,
            print_bed: None,
            linear_deflection: DEFAULT_LINEAR_DEFLECTION,
            angular_deflection: DEFAULT_ANGULAR_DEFLECTION,
            include_appearance: true,
            slicer_target: SlicerTarget::Standard,
        }
    }
}

impl MeshExportRequest {
    pub fn check_model_snapshot(&self, current: &str) -> Result<(), ExportError> {
        limo_cad_solid::check_export_model_snapshot(self.expected_model_json.as_deref(), current)
            .map_err(|message| ExportError(message.into()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TriangleMesh {
    pub body_id: BodyId,
    pub name: String,
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
}

impl TriangleMesh {
    pub fn from_kernel_body(body: &KernelBodyDto, name: impl Into<String>) -> Self {
        Self {
            body_id: body.body_id,
            name: name.into(),
            positions: body.positions.iter().copied().map(f64::from).collect(),
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
    appearances: &[limo_cad_core::BodyAppearance],
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
    use limo_cad_core::{BodyAppearance, BodyId, Rgba8};
    use std::io::Cursor;

    /// Closed 20 mm cube (watertight). A single quad is rejected by slicers as
    /// zero volume / no geometry.
    fn unit_cube(body_id: u64) -> TriangleMesh {
        let s = 20.0_f64;
        TriangleMesh {
            body_id: BodyId(body_id),
            name: format!("Body{body_id}"),

            positions: vec![
                0.0, 0.0, 0.0, s, 0.0, 0.0, s, s, 0.0, 0.0, s, 0.0, 0.0, 0.0, s, s, 0.0, s, s, s,
                s, 0.0, s, s,
            ],

            indices: vec![
                0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0,
                7, 3, 1, 2, 6, 1, 6, 5,
            ],
        }
    }

    /// OCCT-style cube: 12 triangles × 3 unique positions each (36 verts, no shared indices).
    fn unwelded_unit_cube(body_id: u64) -> TriangleMesh {
        let s = 20.0_f64;
        let corners: [[f64; 3]; 8] = [
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
        let (mut min_z, mut max_z) = (f64::MAX, f64::MIN);
        for chunk in mesh.positions.as_chunks::<3>().0 {
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
    fn threemf_standard_names_material_and_skips_slicer_profile() {
        let bytes =
            write_3mf(&[unit_cube(1)], &[red_pla(1)], true, SlicerTarget::Standard).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name("Metadata/project_settings.config").is_err());
        let mut model = archive.by_name("3D/3dmodel.model").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
        assert!(
            xml.contains(r#"<metadata name="Application">Limo CAD</metadata>"#),
            "standard Application metadata must be exact: {xml}"
        );
        assert!(!xml.contains("BambuStudio"));
        assert!(xml.contains("PLA"));
        assert!(xml.contains("#C82828"));
    }

    #[test]
    fn threemf_bambu_target_is_a_standard_model() {
        let blue = BodyAppearance {
            material: None,
            body_id: BodyId(2),
            color: limo_cad_core::Rgba8::opaque(40, 90, 200),
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
        assert!(archive.by_name("Metadata/project_settings.config").is_err());
        let mut model = archive.by_name("3D/3dmodel.model").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
        assert!(xml.contains(r#"pindex="0""#));
        assert!(xml.contains(r#"pindex="1""#));
        assert!(xml.contains("#C82828"));
        assert!(xml.contains("#285AC8"));
        assert!(xml.contains("PLA, Blue"));
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
    fn threemf_orca_target_is_a_standard_model() {
        let bytes = write_3mf(
            &[unit_cube(1)],
            &[red_pla(1)],
            true,
            SlicerTarget::OrcaSlicer,
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name("Metadata/project_settings.config").is_err());
        let mut model = archive.by_name("3D/3dmodel.model").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
        assert!(!xml.contains("OrcaSlicer"));
        assert!(!xml.contains("BambuStudio"));
    }

    #[test]
    fn threemf_base_name_uses_filament_and_color_not_a_printer_preset() {
        let mut appearance = red_pla(1);
        appearance.filament_type = "PETG".into();
        appearance.preset_id = Some("Generic PETG @BBL X1 Carbon 0.4 nozzle".into());
        appearance.color_name = "Jade White".into();
        let bytes =
            write_3mf(&[unit_cube(1)], &[appearance], true, SlicerTarget::Standard).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut model = archive.by_name("3D/3dmodel.model").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
        assert!(xml.contains("PETG, Jade White"));
        assert!(!xml.contains("@BBL"));
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
    /// Run explicitly: `cargo test -p limo-cad-export --lib tests::regen_manual_smoke_fixtures -- --ignored --exact`
    #[test]
    #[ignore]
    fn regen_manual_smoke_fixtures() {
        use std::path::PathBuf;
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/smoke");
        std::fs::create_dir_all(&dir).unwrap();

        let cube_meshes = [unit_cube(1)];
        let cube_apps = [red_pla(1)];
        for (name, target) in [
            ("cube_bambu_studio.3mf", SlicerTarget::BambuStudio),
            ("cube_orca_slicer.3mf", SlicerTarget::OrcaSlicer),
            ("cube_prusa_slicer.3mf", SlicerTarget::PrusaSlicer),
            ("cube_cura.3mf", SlicerTarget::Cura),
            ("cube_standard.3mf", SlicerTarget::Standard),
        ] {
            let bytes = write_3mf(&cube_meshes, &cube_apps, true, target).unwrap();
            std::fs::write(dir.join(name), bytes).unwrap();
        }

        let (pip_meshes, pip_apps) = print_in_place_clip();
        assert_eq!(pip_meshes.len(), 3);
        for (name, target) in [
            ("print_in_place_clip_bambu.3mf", SlicerTarget::BambuStudio),
            ("print_in_place_clip_orca.3mf", SlicerTarget::OrcaSlicer),
            ("print_in_place_clip_prusa.3mf", SlicerTarget::PrusaSlicer),
            ("print_in_place_clip_cura.3mf", SlicerTarget::Cura),
            ("print_in_place_latch_bambu.3mf", SlicerTarget::BambuStudio),
            ("print_in_place_latch_orca.3mf", SlicerTarget::OrcaSlicer),
            ("print_in_place_latch_prusa.3mf", SlicerTarget::PrusaSlicer),
            ("print_in_place_latch_cura.3mf", SlicerTarget::Cura),
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
