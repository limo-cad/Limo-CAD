//! Retain deterministic synthetic SVG/DXF artifacts without starting a host/GUI.
#[path = "../tests/support/straight_export.rs"]
mod fixture;
use limo_cad_core::UnitSystem;
use limo_cad_occt as occt;
use limo_cad_occt::drawing_export::{
    export_sheet_with_units, DrawingExportFormat, DrawingExportRequest,
};
use limo_cad_sketch::AssemblyDocumentDto;
use serde_json::json;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Pass a fresh absolute evidence directory")?,
    );
    if !out.is_absolute() || (out.exists() && fs::read_dir(&out)?.next().is_some()) {
        return Err(
            "Use a fresh absolute evidence directory; existing artifacts are preserved".into(),
        );
    }
    fs::create_dir_all(&out)?;
    let mut cases = vec![];
    for kind in ["length", "distance", "angle", "point-line"] {
        for (unit_name, units) in [
            ("mm", UnitSystem::Mm),
            ("cm", UnitSystem::Cm),
            ("in", UnitSystem::In),
        ] {
            for full in [false, true] {
                let (mut document, scene, projection) = fixture::fixture(kind, 40.);
                if full {
                    fixture::full_presentation(&mut document);
                }
                let stem = format!(
                    "{kind}-{unit_name}-{}",
                    if full { "full" } else { "default" }
                );
                let source = json!({"source":"synthetic projection fixture; no OCC, host, or physical input",
                    "document_units":units,"drawing":document,"scene":scene,"projection":projection});
                fs::write(
                    out.join(format!("{stem}.json")),
                    serde_json::to_vec_pretty(&source)?,
                )?;
                for (extension, format) in [
                    ("svg", DrawingExportFormat::Svg),
                    ("dxf", DrawingExportFormat::Dxf),
                ] {
                    let content = export_sheet_with_units(
                        &document,
                        &scene,
                        &AssemblyDocumentDto::default(),
                        &DrawingExportRequest {
                            sheet_id: 1,
                            format,
                        },
                        units,
                        |_| Ok(projection.clone()),
                    )
                    .map_err(std::io::Error::other)?;
                    fs::write(out.join(format!("{stem}.{extension}")), content)?;
                }
                cases.push(stem);
            }
        }
    }
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "source":"synthetic projection fixture", "cases":cases,
            "formats":["svg","graphical_dxf"],"pixel_review":"required",
            "not_proven":["OCC projection","physical input","editable DXF DIMENSION entities","printing"]
        }))?,
    )?;
    println!(
        "Saved 24 paired SVG/DXF cases and source records in {}",
        out.display()
    );
    Ok(())
}
