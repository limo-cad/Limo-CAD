//! Retain deterministic synthetic SVG/DXF artifacts without starting a host/GUI.
#[path = "../tests/support/center_export.rs"]
mod circular_fixture;
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
    let mode = match std::env::args().nth(2).as_deref() {
        None => "all",
        Some("--radial-only") => "radial",
        Some("--presentation-only") => "presentation",
        _ => {
            return Err(
                "Optional second argument must be --radial-only or --presentation-only".into(),
            )
        }
    };
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
    for kind in [
        "length",
        "distance",
        "angle",
        "point-line",
        "angle-three-point",
        "radius",
        "diameter",
    ] {
        if mode == "radial" && !matches!(kind, "radius" | "diameter") {
            continue;
        }
        for (unit_name, units) in [
            ("mm", UnitSystem::Mm),
            ("cm", UnitSystem::Cm),
            ("in", UnitSystem::In),
        ] {
            for variant in if mode == "presentation" {
                &["reference", "fit", "reference-fit"][..]
            } else {
                &["default", "full"][..]
            } {
                let (mut document, scene, projection) = if matches!(kind, "radius" | "diameter") {
                    circular_fixture::fixture(kind, 2., false)
                } else {
                    fixture::fixture(kind, 40.)
                };
                if *variant == "full" {
                    fixture::full_presentation(&mut document);
                }
                if mode == "presentation" {
                    let mut value = serde_json::to_value(&document.sheets[0].annotations[0])?;
                    value["prefix"] = json!("QA ");
                    value["suffix"] = json!(" exact");
                    value["presentation"] = json!({"reference":variant.contains("reference"),
                        "fit_class":if variant.contains("fit") { " H7 " } else { "" }});
                    document.sheets[0].annotations[0] = serde_json::from_value(value)?;
                }
                let stem = format!("{kind}-{unit_name}-{variant}");
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
            "formats":["svg","dxf"],"pixel_review":"required",
            "semantic_dxf":"Default linear, angular, radius and diameter cases use editable DIMENSION entities; full presentation remains graphical",
            "not_proven":["OCC projection","physical input","model topology association in DXF","printing"]
        }))?,
    )?;
    println!(
        "Saved {} paired SVG/DXF cases and source records in {}",
        cases.len(),
        out.display()
    );
    Ok(())
}
