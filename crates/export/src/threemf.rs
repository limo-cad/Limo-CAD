//! 3MF package writer: portable model, plus Prusa and Cura metadata.

use std::io::{Cursor, Write};

use limo_cad_core::{BodyAppearance, BodyId};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;
use zip::ZipWriter;

use crate::mesh_weld::{validate_3mf_model_mesh, weld_triangle_mesh, DEFAULT_WELD_EPSILON};
use crate::slicer::SlicerTarget;
use crate::{ExportError, TriangleMesh};

pub fn write_3mf(
    meshes: &[TriangleMesh],
    appearances: &[BodyAppearance],
    include_appearance: bool,
    target: SlicerTarget,
) -> Result<Vec<u8>, ExportError> {
    write_package(meshes, appearances, include_appearance, target, None)
}

pub(crate) fn write_package(
    meshes: &[TriangleMesh],
    appearances: &[BodyAppearance],
    include_appearance: bool,
    target: SlicerTarget,
    scene: Option<(
        &limo_cad_assembly::ComponentStructureDto,
        &limo_cad_assembly::AssemblySolutionDto,
    )>,
) -> Result<Vec<u8>, ExportError> {
    if meshes.is_empty() {
        return Err(ExportError("There are no active bodies to export.".into()));
    }

    let welded: Vec<TriangleMesh> = meshes
        .iter()
        .map(|mesh| weld_triangle_mesh(mesh, DEFAULT_WELD_EPSILON))
        .collect::<Result<_, _>>()?;
    for mesh in &welded {
        validate_3mf_model_mesh(mesh)?;
    }

    let model_xml = match scene {
        Some((structure, solution)) => crate::scene::build_scene_xml(
            &welded,
            appearances,
            include_appearance,
            target,
            structure,
            solution,
        )?,
        None => build_3mf_model_xml(&welded, appearances, include_appearance, target)?,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", options)
            .map_err(zip_err)?;
        zip.write_all(content_types_xml(target).as_bytes())
            .map_err(io_err)?;

        zip.start_file("_rels/.rels", options).map_err(zip_err)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#,
        )
        .map_err(io_err)?;

        zip.start_file("3D/3dmodel.model", options)
            .map_err(zip_err)?;
        zip.write_all(model_xml.as_bytes()).map_err(io_err)?;

        if include_appearance {
            match target {
                SlicerTarget::Standard | SlicerTarget::BambuStudio | SlicerTarget::OrcaSlicer => {}
                SlicerTarget::PrusaSlicer => {
                    write_prusa_metadata(&mut zip, options, &welded, appearances)?;
                }
                SlicerTarget::Cura => {
                    write_cura_metadata(&mut zip, options, &welded, appearances)?;
                }
            }
        }

        zip.finish().map_err(zip_err)?;
    }
    Ok(cursor.into_inner())
}

fn content_types_xml(target: SlicerTarget) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
"#,
    );
    if matches!(target, SlicerTarget::PrusaSlicer | SlicerTarget::Cura) {
        xml.push_str(
            r#"  <Default Extension="config" ContentType="application/octet-stream"/>
  <Default Extension="json" ContentType="application/json"/>
"#,
        );
    }
    xml.push_str("</Types>\n");
    xml
}

/// Cura primarily consumes consortium `basematerials` displaycolor.
/// We also embed a compact materials JSON for tooling / round-trip hints.
fn write_cura_metadata(
    zip: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    options: SimpleFileOptions,
    meshes: &[TriangleMesh],
    appearances: &[BodyAppearance],
) -> Result<(), ExportError> {
    let materials: Vec<serde_json::Value> = meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| {
            let appearance = appearance_for(appearances, mesh.body_id);
            serde_json::json!({
                "extruder": index + 1,
                "body_id": mesh.body_id.0,
                "name": appearance.display_label(),
                "brand": appearance.brand,
                "material": appearance.filament_type,
                "color": appearance.color.opaque_rgb().to_hex_rgb(),
                "diameter_mm": appearance.diameter_mm,
                "density_g_cm3": appearance.density_g_cm3,
            })
        })
        .collect();
    let payload = serde_json::json!({
        "generator": "Limo CAD",
        "note": "Cura reads per-body colors from 3MF basematerials; this file is a material hint list.",
        "materials": materials,
    });
    zip.start_file("Metadata/cura_materials.json", options)
        .map_err(zip_err)?;
    zip.write_all(
        serde_json::to_string_pretty(&payload)
            .map_err(|e| ExportError(format!("cura_materials: {e}")))?
            .as_bytes(),
    )
    .map_err(io_err)?;
    Ok(())
}

fn write_prusa_metadata(
    zip: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    options: SimpleFileOptions,
    meshes: &[TriangleMesh],
    appearances: &[BodyAppearance],
) -> Result<(), ExportError> {
    let mut config = String::from(
        "; Limo CAD → PrusaSlicer-compatible filament hints\n\
         ; generated for multi-material plate import\n",
    );
    let colours: Vec<String> = meshes
        .iter()
        .map(|mesh| {
            appearance_for(appearances, mesh.body_id)
                .color
                .opaque_rgb()
                .to_hex_rgb()
        })
        .collect();
    let types: Vec<String> = meshes
        .iter()
        .map(|mesh| {
            nonempty(
                &appearance_for(appearances, mesh.body_id).filament_type,
                "PLA",
            )
        })
        .collect();
    let settings: Vec<String> = meshes
        .iter()
        .map(|mesh| {
            appearance_for(appearances, mesh.body_id)
                .material_name
                .clone()
        })
        .collect();

    config.push_str(&format!("filament_colour = {}\n", colours.join(";")));
    config.push_str(&format!("filament_type = {}\n", types.join(";")));
    config.push_str(&format!(
        "filament_settings_id = {}\n",
        settings
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(";")
    ));
    let diameters: Vec<String> = meshes
        .iter()
        .map(|mesh| {
            format!(
                "{:.2}",
                appearance_for(appearances, mesh.body_id).diameter_mm
            )
        })
        .collect();
    config.push_str(&format!("filament_diameter = {}\n", diameters.join(";")));

    zip.start_file("Metadata/Slic3r_PE.config", options)
        .map_err(zip_err)?;
    zip.write_all(config.as_bytes()).map_err(io_err)?;

    let mut model_config = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
"#,
    );
    for (index, mesh) in meshes.iter().enumerate() {
        let object_id = index + 2;
        let extruder = index + 1;
        let tri_count = mesh.indices.len() / 3;
        let last_tri = if tri_count == 0 { 0 } else { tri_count - 1 };
        let fallback_name = format!("Body{}", mesh.body_id.0);
        let display_name = if mesh.name.trim().is_empty() {
            fallback_name.as_str()
        } else {
            mesh.name.as_str()
        };
        let name = xml_escape(display_name);
        model_config.push_str(&format!(
            r#"  <object id="{object_id}" instancescount="1">
    <metadata type="object" key="name" value="{name}"/>
    <metadata type="object" key="extruder" value="{extruder}"/>
    <volume firstid="0" lastid="{last_tri}">
      <metadata type="volume" key="name" value="{name}"/>
      <metadata type="volume" key="volume_type" value="ModelPart"/>
      <metadata type="volume" key="extruder" value="{extruder}"/>
    </volume>
  </object>
"#
        ));
    }
    model_config.push_str("</config>\n");

    zip.start_file("Metadata/Slic3r_PE_model.config", options)
        .map_err(zip_err)?;
    zip.write_all(model_config.as_bytes()).map_err(io_err)?;
    Ok(())
}

pub(crate) fn build_3mf_model_xml(
    meshes: &[TriangleMesh],
    appearances: &[BodyAppearance],
    include_appearance: bool,
    target: SlicerTarget,
) -> Result<String, ExportError> {
    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US"
  xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <metadata name="Application">{}</metadata>
  <metadata name="Title">Limo CAD export</metadata>
  <resources>
"#,
        xml_escape(target.application_metadata())
    );

    if include_appearance {
        xml.push_str(r#"    <basematerials id="1">"#);
        xml.push('\n');
        for mesh in meshes {
            let appearance = appearance_for(appearances, mesh.body_id);
            let color = appearance.color.opaque_rgb();
            let name = xml_escape(&portable_material_name(&appearance));
            xml.push_str(&format!(
                r#"      <base name="{name}" displaycolor="{}"/>"#,
                color.to_hex_rgb()
            ));
            xml.push('\n');
        }
        xml.push_str("    </basematerials>\n");
    }

    for (index, mesh) in meshes.iter().enumerate() {
        let object_id = index + 2;
        let fallback_name = format!("Body{}", mesh.body_id.0);
        let display_name = if mesh.name.trim().is_empty() {
            fallback_name.as_str()
        } else {
            mesh.name.as_str()
        };
        let name = xml_escape(display_name);
        if include_appearance {
            xml.push_str(&format!(
                r#"    <object id="{object_id}" type="model" name="{name}" pid="1" pindex="{pindex}">"#,
                pindex = index,
            ));
        } else {
            xml.push_str(&format!(
                r#"    <object id="{object_id}" type="model" name="{name}">"#
            ));
        }
        xml.push('\n');
        xml.push_str("      <mesh>\n        <vertices>\n");
        if mesh.positions.len() % 3 != 0 {
            return Err(ExportError(format!(
                "body {} has a malformed position buffer",
                mesh.body_id.0
            )));
        }
        for chunk in mesh.positions.as_chunks::<3>().0 {
            xml.push_str(&format!(
                r#"          <vertex x="{}" y="{}" z="{}"/>"#,
                chunk[0], chunk[1], chunk[2]
            ));
            xml.push('\n');
        }
        xml.push_str("        </vertices>\n        <triangles>\n");
        if mesh.indices.len() % 3 != 0 {
            return Err(ExportError(format!(
                "body {} has a malformed index buffer",
                mesh.body_id.0
            )));
        }
        let vertex_count = mesh.positions.len() / 3;
        for tri in mesh.indices.as_chunks::<3>().0 {
            let (v1, v2, v3) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            if v1 >= vertex_count || v2 >= vertex_count || v3 >= vertex_count {
                return Err(ExportError(format!(
                    "body {} has an out-of-range triangle index",
                    mesh.body_id.0
                )));
            }
            xml.push_str(&format!(
                r#"          <triangle v1="{v1}" v2="{v2}" v3="{v3}"/>"#
            ));
            xml.push('\n');
        }
        xml.push_str("        </triangles>\n      </mesh>\n    </object>\n");
    }

    xml.push_str("  </resources>\n  <build>\n");
    for index in 0..meshes.len() {
        let object_id = index + 2;
        xml.push_str(&format!(r#"    <item objectid="{object_id}"/>"#));
        xml.push('\n');
    }
    xml.push_str("  </build>\n</model>\n");
    Ok(xml)
}

/// Core 3MF base-material name: chemistry and color, not a printer preset.
fn portable_material_name(appearance: &BodyAppearance) -> String {
    let kind = appearance.filament_type.trim();
    let kind = if kind.is_empty() { "Material" } else { kind };
    let color = appearance.color_name.trim();
    if color.is_empty() {
        kind.to_string()
    } else {
        format!("{kind}, {color}")
    }
}

pub(crate) fn appearance_for(appearances: &[BodyAppearance], body_id: BodyId) -> BodyAppearance {
    appearances
        .iter()
        .find(|entry| entry.body_id == body_id)
        .cloned()
        .unwrap_or_else(|| BodyAppearance::default_for(body_id))
}

fn nonempty(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn zip_err(error: zip::result::ZipError) -> ExportError {
    ExportError(format!("could not write 3MF package: {error}"))
}

fn io_err(error: std::io::Error) -> ExportError {
    ExportError(format!("could not write 3MF package: {error}"))
}
