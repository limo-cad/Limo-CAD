//! Bounded advanced annotation export QA; no CAD host or desktop input.
#[path = "../tests/support/advanced_export.rs"]
mod fixture;
use limo_cad_occt as occt;
#[path = "../tests/support/straight_export.rs"]
pub mod rectangle;
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
        return Err("Preserve existing evidence; choose a fresh absolute directory".into());
    }
    fs::create_dir_all(&out)?;
    let (drawing, scene, projection) = fixture::fixture();
    let before = serde_json::to_value((&drawing, &scene, &projection))?;
    let mut cases = vec![];
    for (stem, units) in [
        ("advanced-mm", limo_cad_core::UnitSystem::Mm),
        ("advanced-in", limo_cad_core::UnitSystem::In),
        ("advanced-unicode", limo_cad_core::UnitSystem::Mm),
    ] {
        let mut drawing = drawing.clone();
        if stem == "advanced-unicode" {
            drawing.sheets[0]
                .annotations
                .push(serde_json::from_value(json!({
                    "kind":"note", "id":drawing.next_annotation_id,
                    "text":"Café 零件 ⌀ Ø Ω Ⓜ\u{fe0e}", "position":[30., 175.]
                }))?);
            drawing.next_annotation_id += 1;
        }
        fs::write(
            out.join(format!("{stem}.json")),
            serde_json::to_vec_pretty(&json!({
                "source":"synthetic exact-key projection; not OCCT or live-input proof",
                "drawing":drawing,"scene":scene,"projection":projection,"units":units
            }))?,
        )?;
        for (extension, format) in [
            ("svg", DrawingExportFormat::Svg),
            ("dxf", DrawingExportFormat::Dxf),
        ] {
            let content = export_sheet_with_units(
                &drawing,
                &scene,
                &Default::default(),
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
    assert_eq!(
        before,
        serde_json::to_value((&drawing, &scene, &projection))?
    );
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "source":"synthetic exact-key projection","cases":cases,"families":fixture::KINDS,
            "pixel_review":"required","not_proven":["OCCT projection","native authoring UI","physical input","printing"]
        }))?,
    )?;
    println!(
        "Retained three mixed advanced SVG/DXF pairs in {}",
        out.display()
    );
    Ok(())
}
