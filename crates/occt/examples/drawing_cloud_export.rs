//! Reproducible cloud SVG/DXF artifacts without a host, kernel or OS input.
#[path = "../tests/support/cloud_export.rs"]
mod fixture;
use limo_cad_occt::drawing_export::{export_sheet, DrawingExportFormat, DrawingExportRequest};
use serde_json::json;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Pass a fresh absolute evidence directory")?,
    );
    let font_family = std::env::args().nth(2);
    if !out.is_absolute() || out.exists() && fs::read_dir(&out)?.next().is_some() {
        return Err("Preserve previous evidence; use a fresh absolute directory".into());
    }
    fs::create_dir_all(&out)?;
    let mut cases = Vec::new();
    for kind in ["triangle", "quad", "loaded-seven", "clipped-right"] {
        let (mut drawing, scene) = fixture::fixture(kind);
        if let Some(family) = &font_family {
            drawing.sheets[0].style.font_family.clone_from(family);
        }
        let stem = format!("cloud-{kind}");
        fs::write(
            out.join(format!("{stem}.json")),
            serde_json::to_vec_pretty(
                &json!({"source":"Synthetic saved paper vertices, no projection","drawing":drawing,"scene":scene}),
            )?,
        )?;
        for (extension, format) in [
            ("svg", DrawingExportFormat::Svg),
            ("dxf", DrawingExportFormat::Dxf),
        ] {
            let content = export_sheet(
                &drawing,
                &scene,
                &Default::default(),
                &DrawingExportRequest {
                    sheet_id: 1,
                    format,
                },
                |_| Err("Cloud fixture must not request projection".into()),
            )
            .map_err(std::io::Error::other)?;
            fs::write(out.join(format!("{stem}.{extension}")), content)?;
        }
        cases.push(stem);
    }
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(
            &json!({"source":"synthetic saved paper cloud fixture","cases":cases,"pixel_review":"required","not_proven":["OCC projection","native authoring or File UI","physical input","printing"]}),
        )?,
    )?;
    println!("Retained 4 paired cloud SVG/DXF cases in {}", out.display());
    Ok(())
}
