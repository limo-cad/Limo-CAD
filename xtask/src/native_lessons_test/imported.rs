//! General-file Scripts controls must inspect without modifying a nonblank
//! design, run the inspected source in another tab, and preserve both models.
use super::*;
use crate::replay::Client;
use std::path::Path;
mod os_input;
mod preview;

pub(super) fn exercise(c: &mut Client, out: &Path) -> Result<Value> {
    let renamed = ui(
        c,
        json!({"action":"file","command":"rename","name":"Retained lesson"}),
    )?;
    let original = c.call("cad_project_model", json!({}))?;
    let original_json = original.as_str().context("Retained lesson model export")?;
    std::fs::write(out.join("scripts-retained-before.json"), original_json)?;
    std::fs::write(
        out.join("scripts-retained-rename-receipt.json"),
        serde_json::to_vec_pretty(&renamed)?,
    )?;
    let original_model: Value = serde_json::from_str(original_json)?;
    ensure!(
        original_model["document"]["name"] == "Retained lesson",
        "Original model was captured before the rename completed"
    );
    let original_ui = ui(c, json!({"action":"inspect"}))?;
    let original_session = original_ui["active_session_id"].clone();
    let source_path = out.join("opened-script.limo.jsonc");
    let fragment_path = out.join("opened-profile.collection.jsonc");
    let fragment = json!({"steps":[
        {"call":{"group":"sketch/draw","operation":"sketch_begin","arguments":{"name":"Imported profile","plane":{"type":"origin_plane","plane":"xy"}}}},
        {"call":{"group":"sketch/draw","operation":"sketch_add_rectangle_locked","arguments":{"mode":"two_point","anchor":{"x":0.,"y":0.},"corner_hint":{"x":16.,"y":9.},"width_mm":16.,"height_mm":9.,"ctrl_held":true}}},
        {"call":{"group":"sketch/draw","operation":"sketch_finish","arguments":{}}},
        {"call":{"group":"solid/build","operation":"solid_extrude","arguments":{"sketch_name":"Imported profile","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":5.}}}}
    ]});
    let source = json!({"version":1,"name":"Opened file fixture","includes":["opened-profile.collection.jsonc"],"steps":[
        {"view":"isometric","fit":true,"duration_ms":1},
        {"chapter":"Imported source complete","note":"The inspected source ran in its own design","duration_ms":1}
    ],"checks":[{"call":{"group":"solid/check","operation":"solid_scene","arguments":{}}}]});
    let authored = format!(
        "// Retain authored comments and relative includes.\n{}",
        serde_json::to_string_pretty(&source)?
    );
    std::fs::write(&source_path, &authored)?;
    std::fs::write(&fragment_path, serde_json::to_vec_pretty(&fragment)?)?;
    std::fs::write(
        out.join("opened-script-inspected-source.json"),
        serde_json::to_vec_pretty(&json!({"root":source,"fragment":fragment}))?,
    )?;
    control(c, "Scripts", None)?;
    if os_input::enabled() {
        let opened = control(c, "Open script...", None)?;
        ensure!(
            opened["value"]["awaiting_input"] == true,
            "Open script did not start the OS chooser: {opened}"
        );
        os_input::complete_dialog(
            c,
            out,
            "Open Limo CAD script",
            source_path.to_str().context("Fixture path Unicode")?,
        )?;
    } else {
        control(
            c,
            "Script path",
            Some(source_path.to_str().context("Fixture path Unicode")?),
        )?;
        control(c, "Load script", None)?;
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let loaded = loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state)
            .any(|row| row["label"] == "Run in new design" && row["disabled"] == false)
        {
            break state;
        }
        ensure!(
            Instant::now() < deadline,
            "Imported script did not become runnable: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    ensure!(
        controls(&loaded).any(
            |row| row["label"] == "Script source path and include directory"
                && row["value"] == source_path.to_string_lossy().as_ref()
                && row["read_only"] == true
        ),
        "Loaded script lost its inspected file identity: {loaded}"
    );
    ensure!(
        c.call("cad_project_model", json!({}))? == original,
        "Inspecting a script changed the nonblank design"
    );
    capture(c, out, "scripts-imported-ready")?;
    let editor = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&editor).any(|row| row["label"] == "Authored script source"
            && row["role"] == "multiline_textbox"
            && row["value"] == authored),
        "Editor must show the authored source from the same inspected snapshot: {editor}"
    );
    control(c, "Authored script source", Some("{ unfinished draft"))?;
    let invalid = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&invalid)
            .any(|row| row["label"] == "Run in new design" && row["disabled"] == true),
        "Unvalidated edits left the previous source runnable: {invalid}"
    );
    control(c, "Validate source", None)?;
    let invalid = wait_validation(c, false)?;
    capture(c, out, "scripts-source-invalid")?;
    let edited = authored.replace("Opened file fixture", "Edited file fixture");
    control(c, "Authored script source", Some(&edited))?;
    control(c, "Validate source", None)?;
    let validated = wait_validation(c, true)?;
    ensure!(
        controls(&validated).any(|row| row["label"] == "Script validation and save status"
            && row["value"]
                .as_str()
                .is_some_and(|text| text.contains("unsaved"))),
        "Validation incorrectly marked edited source saved: {validated}"
    );
    ensure!(
        c.call("cad_project_model", json!({}))? == original,
        "Source editing or validation changed the nonblank design"
    );
    ensure!(
        std::fs::read_to_string(&source_path)? == authored,
        "Editing or validation silently wrote the source file"
    );
    capture(c, out, "scripts-source-validated")?;
    if os_input::enabled() {
        let saved_path = out.join("saved-script.limo.jsonc");
        let saving = control(c, "Save script as...", None)?;
        ensure!(
            saving["value"]["awaiting_input"] == true,
            "Save script as did not start the OS chooser: {saving}"
        );
        os_input::complete_dialog(
            c,
            out,
            "Save Limo CAD script",
            saved_path.to_str().context("Save path Unicode")?,
        )?;
        let save_deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let _ = ui(c, json!({"action":"inspect"}))?;
            if saved_path.is_file()
                && std::fs::read_to_string(&saved_path).ok().as_deref() == Some(edited.as_str())
            {
                break;
            }
            ensure!(
                Instant::now() < save_deadline,
                "Saved script file did not appear"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        control(c, "Validate source", None)?;
        let _revalidated = wait_validation(c, true)?;
    }
    std::fs::write(&source_path, "invalid root changed after inspection")?;
    std::fs::write(&fragment_path, "invalid include changed after inspection")?;
    let started = control(c, "Run in new design", None)?;
    let expected_path = if os_input::enabled() {
        out.join("saved-script.limo.jsonc")
    } else {
        source_path.clone()
    };
    ensure!(
        started["value"]["script_started"]["path"] == expected_path.to_string_lossy().as_ref()
            && started["value"]["script_error"].is_null(),
        "Imported script did not start: {started}"
    );
    let session = started["active_session_id"]
        .as_str()
        .context("Script new tab session")?;
    ensure!(
        started["active_session_id"] != original_session,
        "Script reused the original document session"
    );
    c.call("cad_attach", json!({"session_id":session}))?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let presentation = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(state["stopped"] != true, "Imported script stopped: {state}");
        if state["finished"] == true && state["chapter"] == "Imported source complete" {
            break state.clone();
        }
        ensure!(
            Instant::now() < deadline,
            "Imported script did not finish: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let exported = c.call("cad_project_model", json!({}))?;
    let model: Value = serde_json::from_str(exported.as_str().context("Imported model export")?)?;
    ensure!(
        model["extrudes"]
            .as_array()
            .is_some_and(|rows| rows.len() == 1)
            && model["extrudes"][0]["extent"]["distance"] == 5.
            && model["fillets"].as_array().is_some_and(Vec::is_empty),
        "Imported source did not create its own editable extrusion: {model}"
    );
    let scene = c.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"].as_array().is_some_and(Vec::is_empty)
            && scene["bodies"]
                .as_array()
                .is_some_and(|rows| rows.len() == 1),
        "Imported source did not produce one valid solid: {scene}"
    );
    capture(c, out, "scripts-imported-complete")?;
    let archive_path = out.join("opened-script-result.limo");
    ui(
        c,
        json!({"action":"file","command":"save","path":archive_path}),
    )?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&archive_path)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        saved == model,
        "Imported source archive lost editable intent"
    );
    let restored = control(c, "Retained lesson", None)?;
    ensure!(
        restored["active_session_id"] == original_session,
        "Switching back did not restore the original lesson session: {restored}"
    );
    c.call(
        "cad_attach",
        json!({"session_id":restored["active_session_id"]}),
    )?;
    let restored_model = c.call("cad_project_model", json!({}))?;
    std::fs::write(
        out.join("scripts-retained-after.json"),
        restored_model
            .as_str()
            .context("Restored lesson model export")?,
    )?;
    ensure!(
        restored_model == original,
        "Imported source modified the retained original lesson"
    );
    capture(c, out, "scripts-retained-original")?;
    let catalog = exercise_catalog(c, out, &original, &edited)?;
    Ok(
        json!({"state_checks_passed":true,"pixel_review":"required","loaded_ui":loaded,
        "source_editor":editor,"invalid_draft":invalid,"validated_draft":validated,
        "catalog":catalog,
        "started":started,"presentation":presentation,"model":model,"original":original,
        "checks":["inspect-does-not-run","authored-multiline-editor","invalid-draft-blocks-run",
            "shared-source-validation","validation-keeps-unsaved-state","no-implicit-source-write",
            "frozen-source-and-includes","explicit-retained-new-tab",
            "real-shared-runner-solid","exact-saved-model","original-tab-unchanged"],
        "not_proven": if os_input::enabled() {
            json!(["Physical source editing and IME", "Physical keyboard path entry"])
        } else {
            json!(["OS script open/save chooser interaction", "Physical source editing and IME", "Physical keyboard path entry"])
        }}),
    )
}

fn wait_source(c: &mut Client, contains: &str) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state).any(|row| {
            row["label"] == "Script source path and include directory"
                && row["value"]
                    .as_str()
                    .is_some_and(|value| value.contains(contains))
        }) && controls(&state)
            .any(|row| row["label"] == "Authored script source" && row["disabled"] == false)
        {
            return Ok(state);
        }
        ensure!(
            Instant::now() < deadline,
            "Recipe source did not appear: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn exercise_catalog(c: &mut Client, out: &Path, model: &Value, edited: &str) -> Result<Value> {
    let queued = ui(c, json!({"action":"open_recipe","recipe":"garden-bench"}))?;
    ensure!(
        queued["recipe"]["status"] == "queued",
        "Recipe delivery lacked its queued receipt: {queued}"
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let pending = loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state).any(|row| row["label"] == "Cancel opening example") {
            break state;
        }
        ensure!(
            Instant::now() < deadline,
            "Unsaved-source decision did not appear: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    ensure!(
        controls(&pending)
            .any(|row| row["label"] == "Authored script source" && row["value"] == edited),
        "Queued recipe replaced unsaved authored source: {pending}"
    );
    control(c, "Cancel opening example", None)?;
    let cancelled = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&cancelled)
            .any(|row| row["label"] == "Authored script source" && row["value"] == edited),
        "Cancel changed the source draft: {cancelled}"
    );
    ui(c, json!({"action":"open_recipe","recipe":"garden-bench"}))?;
    control(c, "Discard edits", None)?;
    let recipe = wait_source(c, "(garden-bench)")?;
    ensure!(
        c.call("cad_project_model", json!({}))? == *model,
        "Opening a flagship recipe executed modeling commands"
    );
    capture(c, out, "scripts-recipe-source")?;
    control(c, "Back to Scripts", None)?;
    control(c, "Browse examples", None)?;
    let catalog = c.call("cad_interface", json!({"action":"recipes"}))?;
    let expected: std::collections::BTreeSet<_> = catalog
        .as_array()
        .context("Shared recipe catalog")?
        .iter()
        .filter_map(|row| row["name"].as_str().map(str::to_owned))
        .collect();
    let lesson = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "fillet-basics")
        .and_then(|row| row["name"].as_str())
        .context("Shared first lesson")?
        .to_owned();
    let mut visible = std::collections::BTreeSet::new();
    let mut pages = Vec::new();
    for _ in 0..32 {
        let state = ui(c, json!({"action":"inspect"}))?;
        for row in controls(&state) {
            if let Some(label) = row["label"]
                .as_str()
                .filter(|label| expected.contains(*label))
            {
                visible.insert(label.to_owned());
            }
        }
        let next = controls(&state)
            .find(|row| {
                row["label"]
                    .as_str()
                    .is_some_and(|label| label.starts_with("Next examples ("))
                    && row["disabled"] == false
            })
            .and_then(|row| row["label"].as_str())
            .map(str::to_owned);
        pages.push(state);
        if let Some(next) = next {
            control(c, &next, None)?;
        } else {
            break;
        }
    }
    ensure!(
        visible == expected,
        "Native catalog missed installed examples: {visible:?} vs {expected:?}"
    );
    capture(c, out, "scripts-example-library")?;
    for _ in 0..32 {
        let state = ui(c, json!({"action":"inspect"}))?;
        if !controls(&state)
            .any(|row| row["label"] == "Previous examples" && row["disabled"] == false)
        {
            break;
        }
        control(c, "Previous examples", None)?;
    }
    control(c, &lesson, None)?;
    let selected = wait_source(c, "(fillet-basics)")?;
    ensure!(
        c.call("cad_project_model", json!({}))? == *model,
        "Browsing examples changed the retained design"
    );
    let preview = preview::exercise(c, out, model)?;
    Ok(
        json!({"queued":queued,"unsaved_guard":pending,"cancelled":cancelled,"recipe_source":recipe,
        "catalog_pages":pages,"selected_example":selected,"preview":preview,"state_checks_passed":true,"pixel_review":"required",
        "not_proven":["Cold command-line recipe URL","OS registered protocol dispatch"]}),
    )
}

fn wait_validation(c: &mut Client, valid: bool) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        let status = controls(&state)
            .find(|row| row["label"] == "Script validation and save status")
            .and_then(|row| row["value"].as_str())
            .unwrap_or("");
        let finished = if valid {
            status.starts_with("Valid script;")
        } else {
            status.starts_with("Script: ")
        };
        if finished {
            ensure!(
                controls(&state)
                    .any(|row| row["label"] == "Run in new design" && row["disabled"] == !valid),
                "Validation result and Run availability disagree: {state}"
            );
            return Ok(state);
        }
        ensure!(
            Instant::now() < deadline,
            "Source validation did not finish: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
