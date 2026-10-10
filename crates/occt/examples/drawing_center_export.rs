//! Reproducible synthetic center graphics; never starts a host or sends input.
#[path = "../tests/support/center_export.rs"]
mod fixture;
use limo_cad_occt as occt;
use occt::drawing_export::{export_sheet, DrawingExportFormat, DrawingExportRequest};
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
    let mut cases = Vec::new();
    for kind in ["mark", "line"] {
        for scale in [1., 2.] {
            for custom in [false, true] {
                let (drawing, scene, projection) = fixture::fixture(kind, scale, custom);
                let stem = format!(
                    "center-{kind}-{scale}-{}",
                    if custom { "custom" } else { "default" }
                );
                fs::write(
                    out.join(format!("{stem}.json")),
                    serde_json::to_vec_pretty(&json!({
                    "source":"synthetic circles; no OCC, host or physical input", "drawing":drawing,"scene":scene,"projection":projection}))?,
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
        serde_json::to_vec_pretty(
            &json!({"source":"synthetic projection fixture","cases":cases,
        "pixel_review":"required","not_proven":["OCC projection","native authoring or File UI","physical input","printing"]}),
        )?,
    )?;
    println!(
        "Retained 8 paired center SVG/DXF cases in {}",
        out.display()
    );
    Ok(())
}
