//! Real retained controls qualify an explicitly bound, saved-template handoff.
mod verification;
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, owned_config, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use limo_cad_core::PrintSettingsDto;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command, time::Duration};

fn field(c: &mut Client, label: &str, value: Option<&str>, up: &str, down: &str) -> Result<Value> {
    for pass in 0..2 {
        if pass == 1 {
            for _ in 0..180 {
                let state = ui(c, json!({"action":"inspect"}))?;
                if !controls(&state).any(|v| v["label"] == up && v["disabled"] == false) {
                    break;
                }
                control(c, up, None)?;
            }
        }
        for _ in 0..180 {
            let state = ui(c, json!({"action":"inspect"}))?;
            let found: Vec<_> = controls(&state)
                .filter(|v| {
                    v["disabled"] == false
                        && v["label"]
                            .as_str()
                            .is_some_and(|s| s == label || s.starts_with(&format!("{label} (")))
                })
                .collect();
            ensure!(found.len() <= 1, "Ambiguous retained field {label}");
            if let Some(found) = found.first() {
                return control(
                    c,
                    found["label"].as_str().context("Retained field label")?,
                    value,
                );
            }
            if !controls(&state).any(|v| v["label"] == down && v["disabled"] == false) {
                break;
            }
            control(c, down, None)?;
        }
    }
    anyhow::bail!("Missing enabled retained field {label}")
}
fn bambu(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    field(
        c,
        label,
        value,
        "Previous Bambu project fields",
        "More Bambu project fields",
    )
}
fn print(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    field(
        c,
        label,
        value,
        "Previous print settings fields",
        "More print settings fields",
    )
}
fn view(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    field(
        c,
        label,
        value,
        "Previous named-view fields",
        "More named-view fields",
    )
}
fn verify_template_z(
    report: &Value,
    expected: &[limo_cad_export::bambu_project::BambuVolumeGeometry],
    evidence: &std::path::Path,
) -> Result<()> {
    fs::write(evidence, serde_json::to_vec_pretty(report)?)?;
    let groups: Vec<limo_cad_export::bambu_project::BambuGroupZPreflight> =
        serde_json::from_value(report["z_preflight"].clone())?;
    let normal: Vec<_> = expected
        .iter()
        .filter(|volume| volume.subtype == "normal_part")
        .collect();
    ensure!(
        groups.len() == 5 && normal.len() == 5,
        "Fixture must retain five separate normal objects"
    );
    for group in groups {
        let baseline = normal
            .iter()
            .find(|volume| {
                volume.object_id == group.object_id
                    && volume.instance_id == group.instance_id
                    && volume.plate_index == group.plate_index
            })
            .context("Unexpected target group")?;
        ensure!(group.source_bindings.len() == 1, "Fixture grouping changed");
        for axis in 0..3 {
            ensure!(
                (group.world_bounds.min_mm[axis] - baseline.world_bounds.min_mm[axis]).abs()
                    < 0.001
                    && (group.world_bounds.max_mm[axis] - baseline.world_bounds.max_mm[axis]).abs()
                        < 0.001,
                "Synthetic source bounds differ from reviewed template"
            );
        }
        ensure!(
            group.issues.is_empty(),
            "Positive fixture has unexpected layout issues: {:?}",
            group.issues
        );
    }
    Ok(())
}

fn model(c: &mut Client) -> Result<Value> {
    let value = c.call("cad_project_model", json!({}))?;
    Ok(serde_json::from_str(
        value.as_str().context("Owned model JSON")?,
    )?)
}
fn attach(c: &mut Client, value: &Value) -> Result<()> {
    c.call("cad_attach",json!({"session_id":value["active_session_id"].as_str().context("Owned transitioned session")?}))?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipes {
    version: u32,
    parts: Vec<Recipe>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    target_uuid: String,
    role: String,
    settings: PrintSettingsDto,
}

fn request_fields(settings: &PrintSettingsDto) -> Vec<(&'static str, String)> {
    vec![
        (
            "Requested walls",
            settings
                .wall_count
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ),
        (
            "Requested infill (%)",
            settings
                .infill_density_percent
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ),
        (
            "Requested infill pattern",
            settings
                .infill_pattern
                .as_ref()
                .map(|v| {
                    serde_json::to_value(v)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
                .unwrap_or_default(),
        ),
        (
            "Requested top shell layers",
            settings
                .top_shell_layers
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ),
        (
            "Requested bottom shell layers",
            settings
                .bottom_shell_layers
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ),
    ]
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let input=PathBuf::from(std::env::var_os("LIMO_CAD_BAMBU_TEMPLATE").context("Set LIMO_CAD_BAMBU_TEMPLATE to an operator-owned synthetic five-body normal-volume template")?);
    let bindings_path=PathBuf::from(std::env::var_os("LIMO_CAD_BAMBU_BINDINGS").context("Set LIMO_CAD_BAMBU_BINDINGS to the explicit qualification report with report.parts[].binding")?);
    let recipes_path = PathBuf::from(std::env::var_os("LIMO_CAD_BAMBU_RECIPES").context(
        "Set LIMO_CAD_BAMBU_RECIPES to operator-reviewed recipes keyed by native target UUID",
    )?);
    let bytes = fs::read(&input)?;
    let input_sha = crate::hash::hex(&Sha256::digest(&bytes));
    let supplied: Value = serde_json::from_slice(&fs::read(&bindings_path)?)?;
    let recipes_json: Value = serde_json::from_slice(&fs::read(&recipes_path)?)?;
    let recipes: Recipes = serde_json::from_value(recipes_json.clone())?;
    ensure!(
        recipes.version == 1 && recipes.parts.len() == 5,
        "Provide exactly five version-1 operator recipes"
    );
    let inspected = limo_cad_export::bambu_project::inspect_bambu_template(&bytes)?;
    let supplied_parts = supplied["report"]["parts"]
        .as_array()
        .context("Explicit report parts")?;
    let mut seen = std::collections::BTreeSet::new();
    for part in supplied_parts {
        let uuid = part["target_uuid"]
            .as_str()
            .context("Explicit target UUID")?;
        ensure!(
            seen.insert(uuid),
            "Each target UUID must have exactly one explicit recipe"
        );
        let recipe = recipes
            .parts
            .iter()
            .find(|r| r.target_uuid == uuid)
            .context("Missing operator recipe for explicit target UUID")?;
        ensure!(
            !recipe.role.trim().is_empty(),
            "Operator recipe needs a role label"
        );
        recipe.settings.validate().map_err(anyhow::Error::msg)?;
        let binding = &part["binding"];
        let object = inspected
            .objects
            .iter()
            .find(|o| Some(o.object_id as u64) == binding["object_id"].as_u64())
            .context("Explicit native object not found")?;
        let volume = object
            .parts
            .iter()
            .find(|p| Some(p.part_id as u64) == binding["part_id"].as_u64())
            .context("Explicit native part not found")?;
        ensure!(
            volume.uuid.as_deref() == Some(uuid),
            "Operator recipe UUID does not match the inspected explicit native target"
        );
    }
    ensure!(
        recipes
            .parts
            .iter()
            .all(|r| seen.contains(r.target_uuid.as_str())),
        "Recipe contains an unbound target UUID"
    );
    let bindings: Vec<Value> = supplied["report"]["parts"]
        .as_array()
        .context("Explicit report parts")?
        .iter()
        .map(|p| p["binding"].clone())
        .collect();
    ensure!(
        bindings.len() == 5,
        "This fixture requires five explicit normal-volume targets"
    );
    let native_geometry = limo_cad_export::bambu_project::read_bambu_volume_geometry(&bytes)?;
    let source_dimensions: Vec<[f64; 3]> = bindings
        .iter()
        .map(|binding| {
            let volume = native_geometry
                .iter()
                .find(|volume| {
                    volume.subtype == "normal_part"
                        && binding["object_id"] == volume.object_id
                        && binding["instance_id"] == volume.instance_id
                        && binding["part_id"] == volume.part_id
                })
                .context("Explicit fixture binding has no normal template geometry")?;
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for point in volume.positions.as_chunks::<3>().0 {
                for axis in 0..3 {
                    min[axis] = min[axis].min(point[axis]);
                    max[axis] = max[axis].max(point[axis]);
                }
            }
            let dimensions = std::array::from_fn(|axis| max[axis] - min[axis]);
            ensure!(
                dimensions
                    .iter()
                    .all(|v| v.is_finite() && *v > 0. && *v <= 1_000_000.),
                "Invalid synthetic fixture dimensions"
            );
            Ok(dimensions)
        })
        .collect::<Result<_>>()?;
    let mut fixture = start(args, "native-bambu-project")?;
    owned_config(&fixture.out)?;
    let template = fixture.out.join("owned-input-template.3mf");
    fs::write(&template, &bytes)?;
    let output = fixture.out.join("reviewed-bambu-project.3mf");
    let refreshed = fixture.out.join("refreshed-bambu-project.3mf");
    let c = &mut fixture.client;
    let new = control(c, "New design", None)?;
    attach(c, &new)?;
    for (i, dimensions) in source_dimensions.iter().enumerate() {
        begin_sketch(c, "XY")?;
        c.call("sketch_set_grid_snap", json!({"enabled":false}))?;
        let x = i as f64 * 30.;
        c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+dimensions[0],"y":dimensions[1]},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":dimensions[2]}}))?;
    }
    let scene = c.call("solid_scene", json!({}))?;
    let ids: Vec<_> = scene["bodies"]
        .as_array()
        .context("Five fixture source bodies")?
        .iter()
        .map(|b| b["id"].clone())
        .collect();
    ensure!(
        ids.len() == 5,
        "Fixture must contain five independent source definitions"
    );
    for (id, binding) in ids.iter().zip(&bindings) {
        ensure!(
            binding["body_id"] == *id,
            "Supplied fixture binding must explicitly name this source body; no auto matching"
        );
    }
    control(c, "Named Views", None)?;
    view(
        c,
        "CAD occurrence or body",
        Some(&format!("body:{}", ids[0])),
    )?;
    view(c, "Part print settings", None)?;
    for id in &ids {
        let target = supplied_parts
            .iter()
            .find(|p| p["binding"]["body_id"] == *id)
            .context("Explicit source target")?;
        let recipe = recipes
            .parts
            .iter()
            .find(|r| target["target_uuid"] == r.target_uuid)
            .context("Explicit target recipe")?;
        print(c, "Print settings part", Some(&id.to_string()))?;
        for (label, value) in request_fields(&recipe.settings) {
            print(c, label, Some(&value))?;
        }
        print(c, "Apply print settings", None)?;
    }
    print(c, "Close print settings", None)?;
    view(c, "Close named views", None)?;
    let source_geometry = model(c)?;
    control(c, "File", None)?;
    control(c, "Export All Bodies as 3MF…", None)?;
    control(c, "3MF file mode", Some("bambu_project"))?;
    bambu(
        c,
        "Saved Bambu template path",
        Some(&template.to_string_lossy()),
    )?;
    bambu(c, "Inspect saved Bambu template", None)?;
    bambu(c, "Template printer and process", None)?;
    capture(c, &fixture.out, "bambu-template-inspection")?;
    bambu(c, "Apply template process defaults", None)?;
    let intent = c.call("print_intent_get", json!({}))?;
    ensure!(
        intent["selected_process"]["source"]["kind"] == "saved_template",
        "Explicit template default application must persist provenance"
    );
    ensure!(
        intent["selected_process"]["source"]["sha256"] == input_sha,
        "Selected process must capture exact operator template hash"
    );
    bambu(c, "Bambu placement source", Some("template"))?;
    for binding in &bindings {
        bambu(
            c,
            "Bambu source occurrence",
            Some(&format!(
                "body:{}:occurrence:{}",
                binding["body_id"], binding["occurrence_id"]
            )),
        )?;
        bambu(
            c,
            "Bambu target volume instance",
            Some(&format!(
                "object:{}:instance:{}:part:{}",
                binding["object_id"], binding["instance_id"], binding["part_id"]
            )),
        )?;
        bambu(c, "Bind selected source and target", None)?;
    }
    bambu(c, "Keep template material and color", None)?;
    let preview = bambu(c, "Preview Bambu project", None)?;
    let report = &preview["value"]["report"];
    verify_template_z(
        report,
        &native_geometry,
        &fixture.out.join("bambu-initial-preflight.json"),
    )?;
    ensure!(
        report["parts"].as_array().is_some_and(|p| p.len() == 5),
        "Preview must include every explicitly bound visible source"
    );
    ensure!(
        report["template"]["plate_count"] == 4,
        "Template placement must preserve four native plates"
    );
    ensure!(
        report["metadata_readback_verified"] == true
            && report["installed_slicer_imported"] == false
            && report["toolpaths_generated"] == false,
        "Preview must distinguish metadata from slicing evidence"
    );
    let parts = report["parts"].as_array().context("Preview parts")?;
    for p in parts {
        let target = supplied_parts
            .iter()
            .find(|target| target["binding"] == p["binding"])
            .context("Unexpected preview binding")?;
        let recipe = recipes
            .parts
            .iter()
            .find(|r| target["target_uuid"] == r.target_uuid)
            .context("Preview recipe")?;
        ensure!(
            p["target_uuid"] == target["target_uuid"],
            "Preview may not replace intentional bindings"
        );
        let inherited = &p["inherited_settings"];
        for (field, requested) in [
            (
                "wall_loops",
                recipe.settings.wall_count.map(|v| v.to_string()),
            ),
            (
                "sparse_infill_density",
                recipe
                    .settings
                    .infill_density_percent
                    .map(|v| format!("{v}%")),
            ),
            (
                "sparse_infill_pattern",
                recipe.settings.infill_pattern.as_ref().map(|v| {
                    if *v == limo_cad_core::InfillPatternDto::Rectilinear {
                        "rectilinear".into()
                    } else {
                        serde_json::to_value(v).unwrap().as_str().unwrap().into()
                    }
                }),
            ),
            (
                "top_shell_layers",
                recipe.settings.top_shell_layers.map(|v| v.to_string()),
            ),
            (
                "bottom_shell_layers",
                recipe.settings.bottom_shell_layers.map(|v| v.to_string()),
            ),
        ] {
            let expected = requested
                .as_ref()
                .map(|v| Value::String(v.clone()))
                .unwrap_or_else(|| inherited[field].clone());
            // Bambu's native spelling for rectilinear is zig-zag.
            let expected = if field == "sparse_infill_pattern" && expected == "rectilinear" {
                json!("zig-zag")
            } else {
                expected
            };
            ensure!(
                p["effective_settings"][field] == expected,
                "Operator recipe {} has incorrect {field}: {} instead of {expected}",
                recipe.role,
                p["effective_settings"][field]
            );
        }
    }
    bambu(c, "Bambu effective part 1 effective settings", None)?;
    capture(c, &fixture.out, "bambu-effective-preview")?;
    bambu(
        c,
        "Bambu output project path",
        Some(&output.to_string_lossy()),
    )?;
    let written = bambu(c, "Write reviewed Bambu project", None)?;
    ensure!(
        written["value"]["exported"] == true && output.exists(),
        "Actual reviewed file was not written"
    );
    ensure!(
        written["value"]["requires_reslicing"] == true,
        "Written native project must ask for reslicing"
    );
    bambu(c, "Bambu handoff name", Some("Four-plate X2D fixture"))?;
    bambu(c, "Save written Bambu handoff", None)?;
    let saved = c.call("print_intent_get", json!({}))?;
    ensure!(
        saved["target_handoffs"]
            .as_array()
            .is_some_and(|v| v.len() == 1),
        "Explicit written lineage was not persisted"
    );
    let refreshed_preview = bambu(c, "Preview Bambu project", None)?;
    verify_template_z(
        &refreshed_preview["value"]["report"],
        &native_geometry,
        &fixture.out.join("bambu-refresh-preflight.json"),
    )?;
    ensure!(
        refreshed_preview["value"]["report"]["parts"]
            .as_array()
            .is_some_and(|p| p.len() == 5),
        "Written template lineage must preview without fake native-edit acceptance"
    );
    bambu(
        c,
        "Bambu output project path",
        Some(&refreshed.to_string_lossy()),
    )?;
    bambu(c, "Write reviewed Bambu project", None)?;
    ensure!(refreshed.exists(), "Refresh output missing");
    control(c, "Cancel", None)?;
    let expected = model(c)?;
    let mut before = source_geometry;
    before.as_object_mut().unwrap().remove("print_intent");
    let mut after = expected.clone();
    after.as_object_mut().unwrap().remove("print_intent");
    ensure!(
        before == after,
        "Manufacturing UI must preserve source geometry and assembly"
    );
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let archived = crate::project_archive::model(&fs::read(&fixture.project)?)?;
    let closed = ui(c, json!({"action":"file","command":"close"}))?;
    attach(c, &closed)?;
    let opened = ui(
        c,
        json!({"action":"file","command":"open","path":fixture.project}),
    )?;
    attach(c, &opened)?;
    ensure!(
        c.call("print_intent_get", json!({}))? == saved,
        "Reopen must retain explicit source/bindings/baselines"
    );
    let mut command = Command::new(&fixture.server);
    command.arg("--headless");
    let mut cold = Client::start_command(command, Some(Duration::from_secs(45)))?;
    cold.call("cad_load_project_model", json!({"model_json":archived}))?;
    ensure!(
        cold.call("print_intent_get", json!({}))? == saved,
        "Cold load lost saved handoff lineage"
    );
    cold.finish(Duration::from_secs(10))?;
    ensure!(
        fs::read(&input)? == bytes && fs::read(&template)? == bytes,
        "Original and copied input templates must remain byte-for-byte unchanged"
    );
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"input_sha256":input_sha,"explicit_bindings":bindings,"operator_recipes":recipes_json,"source_dimensions_mm":source_dimensions,"preview":report,"written":written,"saved_intent":saved,"project":fixture.project,"output":output,"refreshed":refreshed,"cold_load":true,"not_proven":["physical strength","OS file chooser","installed slicer import","toolpaths"]}),
        )?,
    )?;
    verification::run(c, &fixture.out, &refreshed, &ids[0])?;
    println!(
        "PASS native Bambu saved-template controls, explicit five-part binding, four plates, requested settings, write/refresh and persistent lineage: {}",
        fixture.report.display()
    );
    Ok(())
}

pub(super) fn run_repeated(args: impl Iterator<Item = String>) -> Result<()> {
    let input = PathBuf::from(
        std::env::var_os("LIMO_CAD_BAMBU_TEMPLATE")
            .context("Set LIMO_CAD_BAMBU_TEMPLATE to the owned repeated multipart template")?,
    );
    let bindings_path = PathBuf::from(
        std::env::var_os("LIMO_CAD_BAMBU_BINDINGS")
            .context("Set LIMO_CAD_BAMBU_BINDINGS to its explicit target report")?,
    );
    let bytes = fs::read(&input)?;
    let supplied: Value = serde_json::from_slice(&fs::read(&bindings_path)?)?;
    let targets = supplied["report"]["parts"]
        .as_array()
        .context("Explicit multipart targets")?;
    ensure!(
        targets.len() == 4,
        "Choose two explicit two-part native instances"
    );
    let mut fixture = start(args, "native-bambu-repeated")?;
    owned_config(&fixture.out)?;
    let template = fixture.out.join("owned-input-template.3mf");
    fs::write(&template, &bytes)?;
    let output = fixture.out.join("reviewed-repeated-project.3mf");
    let c = &mut fixture.client;
    let new = control(c, "New design", None)?;
    attach(c, &new)?;
    for i in 0..2 {
        begin_sketch(c, "XY")?;
        let x = i as f64 * 25.;
        c.call("sketch_add_rectangle", json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+15.,"y":10.},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude", json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}))?;
    }
    let scene = c.call("solid_scene", json!({}))?;
    let ids: Vec<_> = scene["bodies"]
        .as_array()
        .context("Two source definitions")?
        .iter()
        .map(|b| b["id"].clone())
        .collect();
    ensure!(
        ids.len() == 2,
        "Multipart fixture must have exactly two definitions"
    );
    let component = c.call(
        "assembly_create_component",
        json!({"name":"Repeated two-part print","body_ids":ids,"absorb_promoted_bodies":true}),
    )?;
    let assembly = c.call("assembly_document", json!({}))?;
    let root = assembly["component_structure"]["occurrences"]
        .as_array()
        .context("CAD hierarchy")?
        .iter()
        .find(|o| o["component_id"] == component["id"])
        .context("Original component occurrence")?["id"]
        .clone();
    let rotation = [
        0.,
        0.,
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ];
    c.call(
        "assembly_set_occurrence_pose",
        json!({"occurrence_id":root,"local_pose":{"translation":[40.,40.,0.],"rotation":rotation}}),
    )?;
    let repeated = c.call("assembly_create_occurrence", json!({"component_id":component["id"],"name":"Intentional second print","local_pose":{"translation":[130.,40.,0.],"rotation":rotation}}))?;
    let second = repeated["id"].clone();
    control(c, "Named Views", None)?;
    view(c, "Create named view", None)?;
    view(c, "View name", Some("Repeated manufacturing layout"))?;
    view(c, "Capture current camera and visibility", None)?;
    view(
        c,
        "CAD occurrence or body",
        Some(&format!("occurrence:{root}")),
    )?;
    view(c, "View offset X", Some("5 mm"))?;
    view(c, "View offset Y", Some("10 mm"))?;
    view(c, "View rotation Z", Some("45 deg"))?;
    view(c, "Save view", None)?;
    view(c, "Recall saved view", None)?;
    view(c, "Close named views", None)?;
    let solution = c.call(
        "named_view_solution",
        json!({"name":"Repeated manufacturing layout"}),
    )?;
    let poses = solution["instance_body_poses"]
        .as_array()
        .context("Repeated resolved body poses")?;
    ensure!(
        poses.len() == 4,
        "CAD hierarchy must retain both parts of both intentional repeats"
    );
    let bindings: Vec<Value> = targets
        .iter()
        .map(|part| {
            let mut binding = part["binding"].clone();
            binding["occurrence_id"] = if binding["instance_id"] == 0 {
                root.clone()
            } else {
                second.clone()
            };
            binding
        })
        .collect();
    for binding in &bindings {
        ensure!(
            binding["instance_id"] == 0 || binding["instance_id"] == 1,
            "Only the operator's two explicit native instances are supported"
        );
        ensure!(
            poses.iter().any(|p| p["body_id"] == binding["body_id"]
                && p["occurrence_id"] == binding["occurrence_id"]),
            "Explicit target report must name this authored source definition and occurrence"
        );
    }
    let original = model(c)?;
    control(c, "File", None)?;
    control(c, "Export All Bodies as 3MF…", None)?;
    control(c, "3MF file mode", Some("bambu_project"))?;
    bambu(
        c,
        "Saved Bambu template path",
        Some(&template.to_string_lossy()),
    )?;
    bambu(c, "Inspect saved Bambu template", None)?;
    bambu(c, "Apply template process defaults", None)?;
    bambu(c, "Bambu placement source", Some("resolved_scene"))?;
    bambu(
        c,
        "Bambu CAD view",
        Some("saved:Repeated manufacturing layout"),
    )?;
    bambu(c, "Keep template material and color", None)?;
    for (index, binding) in bindings.iter().enumerate() {
        if index == 2 {
            let before = model(c)?;
            ensure!(
                bambu(c, "Preview Bambu project", None).is_err(),
                "Incomplete repeat bindings must fail rather than silently dropping intended copies"
            );
            ensure!(
                model(c)? == before && !output.exists(),
                "Rejected incomplete bindings must preserve source and output"
            );
        }
        bambu(
            c,
            "Bambu source occurrence",
            Some(&format!(
                "body:{}:occurrence:{}",
                binding["body_id"], binding["occurrence_id"]
            )),
        )?;
        bambu(
            c,
            "Bambu target volume instance",
            Some(&format!(
                "object:{}:instance:{}:part:{}",
                binding["object_id"], binding["instance_id"], binding["part_id"]
            )),
        )?;
        bambu(c, "Bind selected source and target", None)?;
    }
    let preview = bambu(c, "Preview Bambu project", None)?;
    let report = &preview["value"]["report"];
    let parts = report["parts"]
        .as_array()
        .context("Repeated preview parts")?;
    ensure!(
        parts.len() == 4,
        "Native project must retain four printable parts across two intended objects"
    );
    for part in parts {
        let binding = &part["binding"];
        let pose = poses
            .iter()
            .find(|p| {
                p["body_id"] == binding["body_id"] && p["occurrence_id"] == binding["occurrence_id"]
            })
            .context("Matching resolved saved-layout pose")?;
        let translation = part["world_transform"]
            .as_array()
            .context("Actual native transform")?;
        let angle = 2.
            * pose["rotation"][2]
                .as_f64()
                .context("Saved Z rotation")?
                .atan2(
                    pose["rotation"][3]
                        .as_f64()
                        .context("Saved quaternion scalar")?,
                );
        let (sin, cos) = angle.sin_cos();
        for (index, expected) in [cos, sin, 0., -sin, cos, 0., 0., 0., 1.]
            .into_iter()
            .enumerate()
        {
            ensure!(
                (translation[index]
                    .as_f64()
                    .context("Written native basis")?
                    - expected)
                    .abs()
                    < 1e-7,
                "Native multipart orientation must match the saved-layout rotation"
            );
        }
        for axis in 0..3 {
            ensure!(
                (translation[9 + axis]
                    .as_f64()
                    .context("Written native translation")?
                    - pose["translation"][axis]
                        .as_f64()
                        .context("Saved CAD translation")?)
                .abs()
                    < 1e-7,
                "Native translation must match resolved saved-view hierarchy"
            );
        }
    }
    bambu(c, "Bambu source binding coverage", None)?;
    capture(c, &fixture.out, "repeated-multipart-coverage")?;
    bambu(c, "Bambu effective part 1 translation mm", None)?;
    capture(c, &fixture.out, "repeated-saved-layout-preview")?;
    bambu(
        c,
        "Bambu output project path",
        Some(&output.to_string_lossy()),
    )?;
    let written = bambu(c, "Write reviewed Bambu project", None)?;
    ensure!(
        written["value"]["exported"] == true && output.exists(),
        "Reviewed repeated multipart native project was not written"
    );
    control(c, "Cancel", None)?;
    let mut before = original;
    let mut after = model(c)?;
    before.as_object_mut().unwrap().remove("print_intent");
    after.as_object_mut().unwrap().remove("print_intent");
    ensure!(
        before == after && fs::read(&input)? == bytes && fs::read(&template)? == bytes,
        "Manufacturing handoff must preserve CAD hierarchy, saved layouts and input bytes"
    );
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"source_solution":solution,"explicit_bindings":bindings,"preview":report,"written":written,"output":output,"incomplete_bindings_rejected":true,"not_proven":["physical strength","installed slicer import","toolpaths"]}),
        )?,
    )?;
    println!(
        "PASS native Bambu explicit repeated multipart controls and saved-layout poses: {}",
        fixture.report.display()
    );
    Ok(())
}
