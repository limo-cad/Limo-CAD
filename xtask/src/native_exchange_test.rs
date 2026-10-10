//! Native File exchange over the same owned commands used by its OS pickers.
//! No desktop-dialog automation: path-bearing requests select fixture outputs.
use crate::native_fixture::{begin_sketch, browser_select, capture, control, controls, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

fn scene(client: &mut Client) -> Result<Value> {
    let mut scene = client.call("solid_scene", json!({}))?;
    if let Some(object) = scene.as_object_mut() {
        object.remove("_disclosure");
    }
    Ok(scene)
}

fn box_solid(
    client: &mut Client,
    sketch_name: &str,
    min: [f64; 2],
    max: [f64; 2],
    height: f64,
) -> Result<()> {
    begin_sketch(client, "XY")?;
    client.call("sketch_add_rectangle", json!({"mode":"two_point","p1":{"x":min[0],"y":min[1]},"p2":{"x":max[0],"y":max[1]},"ctrl_held":true}))?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude", json!({"sketch_name":sketch_name,"profile_indices":[0],"extent":{"type":"distance","distance":height}}))?;
    Ok(())
}

fn bounds(positions: impl Iterator<Item = f64>) -> Result<[[f64; 3]; 2]> {
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    let mut count = 0;
    for (i, position) in positions.enumerate() {
        ensure!(position.is_finite(), "Mesh coordinate is not finite");
        minimum[i % 3] = minimum[i % 3].min(position);
        maximum[i % 3] = maximum[i % 3].max(position);
        count += 1;
    }
    ensure!(
        count > 0 && count % 3 == 0,
        "Mesh positions are empty or incomplete"
    );
    Ok([minimum, maximum])
}

fn body_bounds(body: &Value) -> Result<[[f64; 3]; 2]> {
    bounds(
        body["mesh"]["positions"]
            .as_array()
            .context("Body mesh missing")?
            .iter()
            .map(|n| n.as_f64().unwrap_or(f64::NAN)),
    )
}

fn same_bounds(actual: [[f64; 3]; 2], expected: [[f64; 3]; 2]) -> Result<()> {
    ensure!(
        actual
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 1e-5),
        "Exchange changed geometry or units: {actual:?} != {expected:?}"
    );
    Ok(())
}

fn check_export(path: &Path, format: &str, selected: bool) -> Result<Value> {
    let bytes = fs::read(path)?;
    match format {
        "step" => {
            ensure!(bytes.starts_with(b"ISO-10303-21"), "STEP header missing");
            ensure!(
                String::from_utf8_lossy(&bytes).contains("END-ISO-10303-21;"),
                "STEP file truncated"
            );
        }
        "3mf" => {
            ensure!(bytes.starts_with(b"PK"), "3MF archive header missing");
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
            let mut model = String::new();
            archive
                .by_name("3D/3dmodel.model")?
                .read_to_string(&mut model)?;
            ensure!(model.contains("unit=\"millimeter\""), "3MF units changed");
            ensure!(
                model.matches("<mesh>").count() == if selected { 1 } else { 2 },
                "3MF exported the wrong selected definitions"
            );
        }
        "stl" => {
            ensure!(bytes.len() >= 84, "STL header truncated");
            let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
            ensure!(
                count > 0 && bytes.len() == 84 + 50 * count,
                "STL triangle table is invalid"
            );
            let positions = bytes[84..].as_chunks::<50>().0.iter().flat_map(|triangle| {
                triangle[12..48]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|v| f32::from_le_bytes(*v) as f64)
            });
            same_bounds(
                bounds(positions)?,
                if selected {
                    [[0., 0., 0.], [40., 25., 6.]]
                } else {
                    [[0., 0., 0.], [80., 25., 6.]]
                },
            )?;
        }
        _ => unreachable!(),
    }
    Ok(json!({"path":path,"format":format,"selected_only":selected,"bytes":bytes.len()}))
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-exchange")?;
    let client = &mut fixture.client;
    box_solid(client, "Sketch1", [0., 0.], [40., 25.], 6.)?;
    box_solid(client, "Sketch2", [60., 0.], [80., 15.], 4.)?;
    let initial_scene = scene(client)?;
    ensure!(
        initial_scene["bodies"]
            .as_array()
            .is_some_and(|b| b.len() == 2),
        "Two exchange solids missing"
    );
    ui(
        client,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let before = client.call("cad_project_model", json!({}))?;
    let initial_archive = fs::read(&fixture.project)?;
    control(client, "Clear selection", None)?;
    control(client, "File", None)?;
    let menu = ui(client, json!({"action":"inspect"}))?;
    for format in ["STEP", "3MF", "STL"] {
        ensure!(
            controls(&menu).any(|c| c["label"] == format!("Export All Bodies as {format}…")
                && c["disabled"] == false),
            "All {format} export is disabled"
        );
        ensure!(
            controls(&menu).any(
                |c| c["label"] == format!("Export Selected Body as {format}…")
                    && c["disabled"] == true
            ),
            "Selected {format} export must require selection"
        );
    }
    ensure!(
        controls(&menu).any(|c| c["label"] == "Import STEP/STP…" && c["disabled"] == false),
        "STEP import disabled"
    );
    capture(client, &fixture.out, "exchange-file-menu")?;
    control(client, "Close File menu", None)?;
    let body_name = initial_scene["bodies"][0]["name"]
        .as_str()
        .context("First body name missing")?;
    browser_select(client, "Bodies", body_name)?;
    control(client, "File", None)?;
    control(client, "Export Selected Body as 3MF…", None)?;
    let options = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&options).any(|c| c["label"] == "Assembled placement" && c["selected"] == true),
        "Mesh export lost its assembly default"
    );
    control(client, "Part coordinates", None)?;
    let options = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&options).any(|c| c["label"] == "Part coordinates" && c["selected"] == true),
        "Mesh scope choice was not retained"
    );
    capture(client, &fixture.out, "exchange-mesh-options")?;
    control(client, "Cancel", None)?;
    ensure!(
        client.call("cad_project_model", json!({}))? == before,
        "Cancelled mesh export changed the model"
    );
    let mut exports = Vec::new();
    for format in ["step", "3mf", "stl"] {
        for selected in [false, true] {
            let path = fixture.out.join(format!(
                "{}.{format}",
                if selected { "selected" } else { "all" }
            ));
            ui(
                client,
                json!({"action":"file","command":format!("export_{format}"),"path":path,"selected_only":selected,"scope":"definition"}),
            )?;
            exports.push(check_export(&path, format, selected)?);
            ensure!(
                client.call("cad_project_model", json!({}))? == before,
                "Export changed the authoritative model"
            );
            ensure!(
                fs::read(&fixture.project)? == initial_archive,
                "Export overwrote the saved project"
            );
        }
    }
    let source = fixture.out.join("selected.step");
    let source_bytes = fs::read(&source)?;
    ui(
        client,
        json!({"action":"file","command":"import_step","path":source}),
    )?;
    let imported = client.call("cad_project_model", json!({}))?;
    let imported_scene = scene(client)?;
    let bodies = imported_scene["bodies"]
        .as_array()
        .context("Imported scene missing")?;
    ensure!(bodies.len() == 3, "STEP did not add one retained body");
    let imported_body = bodies
        .iter()
        .find(|body| {
            !initial_scene["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|before| before["id"] == body["id"])
        })
        .context("Imported body missing")?;
    same_bounds(body_bounds(imported_body)?, [[0., 0., 0.], [40., 25., 6.]])?;
    let definitions = client.call("solid_body_feature_definitions", json!({}))?;
    let embedded = definitions
        .as_array()
        .context("Body feature definitions missing")?
        .iter()
        .find(|f| f["type"] == "import_step")
        .context("Import has no history definition")?;
    ensure!(
        embedded["file_name"] == "selected.step",
        "Import did not retain its source name"
    );
    ensure!(
        STANDARD.decode(
            embedded["data_base64"]
                .as_str()
                .context("Embedded STEP bytes missing")?
        )? == source_bytes,
        "Saved import source differs from the exported STEP"
    );
    capture(client, &fixture.out, "exchange-imported-step")?;
    control(client, "Undo", None)?;
    ensure!(
        client.call("cad_project_model", json!({}))? == before,
        "Import Undo changed prior work or left imported history"
    );
    ensure!(
        scene(client)? == initial_scene,
        "Import Undo did not preserve the two source solids"
    );
    control(client, "Redo", None)?;
    ensure!(
        client.call("cad_project_model", json!({}))? == imported,
        "Import Redo was not exact"
    );
    control(client, "File", None)?;
    control(client, "Save", None)?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&fixture.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    let expected: Value =
        serde_json::from_str(imported.as_str().context("Project export was not text")?)?;
    ensure!(
        saved == expected,
        "Save no longer targets the original project or lost imported geometry"
    );
    ensure!(
        fs::read(source)? == source_bytes,
        "Save overwrote an exchange destination"
    );
    ui(client, json!({"action":"capture","path":fixture.capture}))?;
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required","session":fixture.session,
            "exports":exports,"source_scene":initial_scene,"imported_scene":imported_scene,"model":saved,
            "checks":["native-file-controls","selected-requires-body","native-mesh-scope-choice-and-cancel",
                "all-and-selected-step-3mf-stl","stl-geometry-and-millimetres","3mf-body-count-and-millimetres",
                "step-import-geometry-and-embedded-source","exact-import-undo-redo","export-does-not-mutate-model",
                "save-retains-original-project-destination","live-window-captures"]
        }))?,
    )?;
    println!("PASS native File exchange, embedded STEP import, exact Undo/Redo, original Save destination and capture");
    Ok(())
}
