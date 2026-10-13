//! Existing body metadata and 3MF target controls on a blank owned native host.
use crate::native_fixture::{
    begin_sketch, browser_select, capture, control, controls, panel_field, start, ui,
};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

fn model(c: &mut Client) -> Result<Value> {
    let text = c.call("cad_project_model", json!({}))?;
    Ok(serde_json::from_str(
        text.as_str().context("Project JSON text")?,
    )?)
}

fn field(c: &mut Client, label: &str, text: &str) -> Result<()> {
    panel_field(
        c,
        label,
        Some(text),
        "Previous appearance fields",
        "More appearance fields",
    )?;
    Ok(())
}

fn history(c: &mut Client, before: &Value, after: &Value) -> Result<()> {
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == before,
        "Appearance Undo did not restore the complete model"
    );
    control(c, "Redo", None)?;
    ensure!(
        &model(c)? == after,
        "Appearance Redo did not restore the complete model"
    );
    browser_select(c, "Bodies", "Body1")?;
    Ok(())
}

fn reject_apply(c: &mut Client) -> Result<()> {
    let view = ui(c, json!({"action":"inspect"}))?;
    let button = controls(&view)
        .find(|v| v["label"] == "Apply appearance" && v["disabled"] == false)
        .context("Appearance Apply must accept a click after a field blur")?;
    let result = c.call(
        "cad_interface",
        json!({"action":"click","target":button["id"]}),
    );
    ensure!(
        result.is_err() || result.as_ref().is_ok_and(|v| v["status"] == "failed"),
        "Invalid appearance unexpectedly committed: {result:?}"
    );
    Ok(())
}

fn normalized(text: &str) -> String {
    let text = text.replace('/', "\\").replace("\\\\?\\", "");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

fn check_3mf(path: &Path, application: &str, metadata: Option<&str>) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path)?)?;
    let mut model = String::new();
    archive
        .by_name("3D/3dmodel.model")?
        .read_to_string(&mut model)?;
    ensure!(
        model.contains(&format!(
            "<metadata name=\"Application\">{application}</metadata>"
        )),
        "3MF did not use the chosen shared slicer target"
    );
    ensure!(
        model.contains("displaycolor=\"#12ABEF\"") && model.contains("name=\"PLA, Café 零件\""),
        "3MF lost the selected body's custom color or consortium material label"
    );
    if let Some(metadata) = metadata {
        ensure!(
            archive.by_name(metadata).is_ok(),
            "Missing shared slicer metadata {metadata}"
        );
    } else {
        ensure!(
            !archive
                .file_names()
                .any(|name| name.starts_with("Metadata/")),
            "Standard 3MF unexpectedly contains slicer-specific metadata"
        );
    }
    Ok(())
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-body-appearance")?;
    let c = &mut fixture.client;
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":false}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}))?;
    control(c, "Fit", None)?;
    browser_select(c, "Bodies", "Body1")?;

    let config = crate::native_fixture::owned_config(&fixture.out)?;
    let preference = config.join("slicer-target.json");
    let inspected = ui(c, json!({"action":"inspect"}))?;
    let caption = inspected["ui"]["surfaces"]
        .as_array()
        .context("Interface surfaces")?
        .iter()
        .find(|s| s["name"] == "body/appearance")
        .and_then(|s| s["text"].as_str())
        .context("Body appearance inspection text")?;
    ensure!(
        normalized(caption).contains(&normalized(&preference.to_string_lossy())),
        "Attached host does not use the isolated slicer preference: {caption}"
    );

    let before = model(c)?;
    field(c, "Material brand", "Generic")?;
    field(c, "Material preset", "material.aluminum-6061-t6")?;
    control(c, "Material properties", None)?;
    let details = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&details).any(|v| v["label"] == "Material category"),
        "Unified metal properties were not exposed by the Bevy panel"
    );
    capture(c, &fixture.out, "appearance-metal-properties")?;
    control(c, "Material properties", None)?;
    ensure!(
        model(c)? == before,
        "Inspecting material properties changed the canonical document"
    );
    field(c, "Material brand", "Bambu Lab")?;
    field(c, "Material preset", "bambu.pla.basic.red")?;
    control(c, "Apply appearance", None)?;
    let preset = model(c)?;
    let body_id = c.call("solid_scene", json!({}))?["bodies"][0]["id"].clone();
    let mut appearance = json!({"body_id":body_id,"color":{"r":200,"g":40,"b":40,"a":255},
        "material_name":"Bambu PLA Basic","filament_type":"PLA","brand":"Bambu Lab",
        "color_name":"Red","filament_id":"GFA00","preset_id":"bambu.pla.basic.red",
        "density_g_cm3":1.24,"diameter_mm":1.75});
    let catalog: Value = serde_json::from_slice(&fs::read(
        crate::release_tooling::root().join("crates/export/presets/catalog.json"),
    )?)?;
    let record = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "bambu.pla.basic.red")
        .context("Bambu material catalog record")?;
    appearance["material"] = record["material"].clone();
    appearance["density_g_cm3"] = record["density_g_cm3"].clone();
    ensure!(
        !appearance["material"]["sources"]
            .as_array()
            .context("Resolved material sources")?
            .is_empty(),
        "Catalog preset did not include sourced print properties"
    );
    let mut expected = before.clone();
    expected["body_appearances"] = json!([appearance]);
    ensure!(
        preset == expected,
        "Catalog selection changed unrelated project data"
    );
    history(c, &before, &preset)?;
    capture(c, &fixture.out, "appearance-catalog")?;

    field(c, "Body color (hex)", "#12abEF")?;
    field(c, "Material name", "")?;
    field(c, "Material preset", "")?;
    reject_apply(c)?;
    ensure!(
        model(c)? == preset,
        "Invalid material changed canonical appearance"
    );
    capture(c, &fixture.out, "appearance-invalid")?;
    field(c, "Material name", "Custom blend")?;
    field(c, "Color name", "Café 零件")?;
    control(c, "Apply appearance", None)?;
    let custom = model(c)?;
    expected["body_appearances"][0]["color"] = json!({"r":18,"g":171,"b":239,"a":255});
    expected["body_appearances"][0]["preset_id"] = Value::Null;
    expected["body_appearances"][0]["material_name"] = json!("Custom blend");
    expected["body_appearances"][0]["color_name"] = json!("Café 零件");
    ensure!(
        custom == expected,
        "Custom edit lost manufacturing metadata or unrelated project state"
    );
    history(c, &preset, &custom)?;
    capture(c, &fixture.out, "appearance-custom")?;
    capture(c, &fixture.out, "appearance-body-color")?;
    browser_select(c, "Bodies", "Body1")?;

    field(c, "Body color (hex)", "#GG0000")?;
    reject_apply(c)?;
    ensure!(model(c)? == custom, "Invalid hex committed");
    control(c, "Reset appearance", None)?;
    control(c, "Apply appearance", None)?;
    ensure!(
        model(c)? == custom,
        "Reset did not retain the canonical appearance"
    );

    let mut exports = Vec::new();
    for (target, application, metadata) in [
        ("standard", "Limo CAD", None),
        (
            "prusa_slicer",
            "Limo CAD (PrusaSlicer-compatible)",
            Some("Metadata/Slic3r_PE.config"),
        ),
        (
            "cura",
            "Limo CAD (Cura-compatible)",
            Some("Metadata/cura_materials.json"),
        ),
    ] {
        field(c, "3MF slicer target", target)?;
        ensure!(
            serde_json::from_slice::<Value>(&fs::read(&preference)?)? == target,
            "Slicer choice was not persisted to the isolated application config"
        );
        let path = fixture.out.join(format!("appearance-{target}.3mf"));
        ui(
            c,
            json!({"action":"file","command":"export_3mf","path":path,"scope":"definition","selected_only":true}),
        )?;
        check_3mf(&path, application, metadata)?;
        ensure!(
            model(c)? == custom,
            "Slicer preference or export mutated the project"
        );
        exports.push(path);
    }
    capture(c, &fixture.out, "appearance-slicer")?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&fixture.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(saved == custom, "Saved project changed appearance intent");
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(&json!({
            "checks":["unified-metal-properties","existing-catalog","custom-metadata-preservation","invalid-field-rejection",
                "custom-preset-retains-invalid-buffer","reset","exact-history","isolated-slicer-preference",
                "shared-native-3mf-targets","no-export-model-mutation","exact-project-archive"],
            "exports":exports,"pixel_review":"required",
        "captures":["appearance-metal-properties.png","appearance-catalog.png","appearance-invalid.png","appearance-custom.png","appearance-body-color.png","appearance-slicer.png"]
        }))?,
    )?;
    println!("PASS native body appearance, exact history/archive and all shared 3MF targets; review captures");
    Ok(())
}
