//! Synthetic export evidence; never starts the CAD host or touches the desktop.
#[path = "../tests/support/series_export.rs"]
mod fixture;
use limo_cad_core::UnitSystem;
use limo_cad_occt as occt;
#[path = "../tests/support/straight_export.rs"]
pub mod rectangle;
use limo_cad_sketch::AssemblyDocumentDto;
use occt::drawing_export::{export_sheet_with_units, DrawingExportFormat, DrawingExportRequest};
use serde_json::json;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Pass a fresh absolute evidence directory")?,
    );
    if !out.is_absolute() || (out.exists() && fs::read_dir(&out)?.next().is_some()) {
        return Err("Use a fresh absolute directory; existing evidence is preserved".into());
    }
    fs::create_dir_all(&out)?;
    let mut cases = vec![];
    for kind in ["chain", "baseline", "continued", "ordinate"] {
        for full in [false, true] {
            let (mut document, scene, projection) =
                fixture::fixture(if kind == "ordinate" { "chain" } else { kind });
            if kind == "ordinate" {
                let data = serde_json::to_value(&document.sheets[0].annotations[0])?;
                document.sheets[0].annotations = vec![serde_json::from_value(json!({
                    "id":1,"kind":"ordinate_dimension","view_id":1,
                    "origin":data["anchors"][0],"target":data["anchors"][2],
                    "axis":"both","offset":-12.,"precision":2
                }))?];
            }
            if full {
                fixture::full_presentation(&mut document);
                document.sheets[0].views[0].position[1] = 180.;
            }
            document.sheets[0].title_block.title = format!("{kind} export / synthetic projection");
            let stem = format!("{kind}-{}", if full { "full-in" } else { "default-mm" });
            let units = if full { UnitSystem::In } else { UnitSystem::Mm };
            let before = serde_json::to_value((&document, &scene, &projection))?;
            fs::write(
                out.join(format!("{stem}.json")),
                serde_json::to_vec_pretty(&json!({
                    "source":"synthetic projection; no OCCT/live input", "drawing":document,
                    "scene":scene,"projection":projection,"units":units
                }))?,
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
            assert_eq!(
                before,
                serde_json::to_value((&document, &scene, &projection))?
            );
            cases.push(stem);
        }
    }
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "source":"synthetic exact-anchor projection", "cases":cases,
            "pixel_review":"required", "not_proven":["OCCT projection","live input","printing"]
        }))?,
    )?;
    println!(
        "Retained eight paired series/ordinate SVG/DXF cases in {}",
        out.display()
    );
    Ok(())
}
