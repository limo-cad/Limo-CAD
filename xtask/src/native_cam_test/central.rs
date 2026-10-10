//! The real library UI against a deliberately isolated, owned config folder.
//! Never allow an attached fixture to write the operator's ordinary library.
use super::*;
pub(super) use crate::native_fixture::owned_config;
use std::{fs, path::Path};

fn normalized(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.into()
    }
}

fn contains_text(value: &Value, text: &str) -> bool {
    match value {
        Value::String(value) => normalized(Path::new(value)).contains(text),
        Value::Array(values) => values.iter().any(|value| contains_text(value, text)),
        Value::Object(values) => values.values().any(|value| contains_text(value, text)),
        _ => false,
    }
}

/// Call before the first project-tool Create, which also creates its central
/// definition. Confirm the *host's* displayed path, not only our environment.
pub(super) fn verify_isolation(c: &mut Client, out: &Path) -> Result<()> {
    let config = owned_config(out)?;
    control(c, "Central library", None)?;
    let view = ui(c, json!({"action":"inspect"}))?;
    let expected = normalized(&config.join("cam-tool-library.json"));
    ensure!(
        contains_text(&view, &expected),
        "The attached host did not expose the isolated library path {expected}: {view}"
    );
    control(c, "Close library", None)?;
    Ok(())
}

fn library(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).context("Read the isolated tool library")?)
        .context("Decode the isolated tool library")
}

fn central_field(c: &mut Client, label: &str, value: &str) -> Result<()> {
    paged(
        c,
        label,
        Some(value),
        "Previous library fields",
        "More library fields",
    )?;
    Ok(())
}

fn paged(
    c: &mut Client,
    label: &str,
    value: Option<&str>,
    previous: &str,
    next: &str,
) -> Result<()> {
    for pass in 0..2 {
        if pass == 1 {
            for _ in 0..16 {
                let view = ui(c, json!({"action":"inspect"}))?;
                if !controls(&view).any(|control| {
                    control["surface"] == "cam-library"
                        && control["label"] == previous
                        && control["disabled"] == false
                }) {
                    break;
                }
                crate::native_fixture::control_in(c, "cam-library", previous, None)?;
            }
        }
        for _ in 0..16 {
            let view = ui(c, json!({"action":"inspect"}))?;
            let matches = controls(&view)
                .filter(|control| {
                    control["surface"] == "cam-library"
                        && control["label"] == label
                        && control["disabled"] == false
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() <= 1,
                "Ambiguous central library control {label}"
            );
            if let Some(control) = matches.first() {
                ui(
                    c,
                    if let Some(value) = value {
                        json!({"action":"set_value","target":control["id"],"value":value})
                    } else {
                        json!({"action":"click","target":control["id"]})
                    },
                )?;
                return Ok(());
            }
            if !controls(&view).any(|control| {
                control["surface"] == "cam-library"
                    && control["label"] == next
                    && control["disabled"] == false
            }) {
                break;
            }
            crate::native_fixture::control_in(c, "cam-library", next, None)?;
        }
    }
    anyhow::bail!("No visible, enabled central library control {label}")
}

fn select(c: &mut Client, id: u64, path: &Path) -> Result<()> {
    let library = library(path)?;
    let tool = library["tools"]
        .as_array()
        .context("Central rows")?
        .iter()
        .find(|tool| tool["id"] == id)
        .context("Central source tool")?;
    let name = tool["name"].as_str().context("Central tool name")?;
    let label = tool["number"]
        .as_u64()
        .map_or_else(|| name.to_owned(), |number| format!("T{number} · {name}"));
    paged(c, &label, None, "Previous tools", "More tools")?;
    Ok(())
}

/// Call with Project tools selected, after ordinary edits/presets are checked.
/// Project imports have one undo step; library-only work has no project delta.
pub(super) fn check(c: &mut Client, out: &Path) -> Result<()> {
    let config = owned_config(out)?;
    let path = config.join("cam-tool-library.json");
    let baseline = document(c)?;
    let original_model = c.call("cad_project_model", json!({}))?;
    let project_tool = baseline["tools"][0].clone();
    let id = project_tool["id"].as_u64().context("Project tool ID")?;
    let central_before = library(&path)?;
    ensure!(
        central_before["tools"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|tool| tool["id"] == id)),
        "Native project creation did not register its central tool definition"
    );

    control(c, "Central library", None)?;
    select(c, id, &path)?;
    central_field(c, "Name", "Central roughing mill")?;
    control(c, "Apply central tool", None)?;
    ensure!(
        document(c)? == baseline,
        "Central edit silently updated a project snapshot"
    );
    ensure!(
        library(&path)?["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["id"] == id && tool["name"] == "Central roughing mill"),
        "Central edit was not persisted"
    );
    central_field(c, "Diameter (mm)", "-1")?;
    let valid_bytes = fs::read(&path)?;
    rejected(c, "Apply central tool")?;
    ensure!(
        fs::read(&path)? == valid_bytes && document(c)? == baseline,
        "Invalid central cutter changed library or project"
    );
    control(c, "Reset central tool", None)?;

    central_field(c, "Name", "Should not overwrite concurrent edit")?;
    let mut external = library(&path)?;
    external["fixture_metadata"] = json!({"writer":"concurrent","version":1});
    let external_bytes = serde_json::to_vec(&external)?;
    fs::write(&path, &external_bytes)?;
    rejected(c, "Apply central tool")?;
    ensure!(
        fs::read(&path)? == external_bytes,
        "Stale library receipt overwrote another writer"
    );
    control(c, "Reset central tool", None)?;
    control(c, "Refresh central library", None)?;
    select(c, id, &path)?;
    central_field(c, "Name", "Central finishing mill")?;
    control(c, "Apply central tool", None)?;
    let saved = library(&path)?;
    ensure!(
        saved["fixture_metadata"] == external["fixture_metadata"],
        "Central editing dropped untouched collection metadata"
    );
    capture(c, out, "cam-central-library")?;

    control(c, "Copy tool", None)?;
    central_field(c, "Name", "Central finishing copy")?;
    let pre_create = fs::read(&path)?;
    ensure!(
        document(c)? == baseline,
        "Preparing a central copy changed the project"
    );
    control(c, "Create central tool", None)?;
    ensure!(fs::read(&path)? != pre_create, "Central copy was not saved");
    let copied = library(&path)?;
    let copy_id = copied["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "Central finishing copy")
        .context("Created central copy")?["id"]
        .as_u64()
        .unwrap();
    ensure!(copy_id != id, "Copy reused source central identity");
    control(c, "Delete tool", None)?;
    ensure!(
        !library(&path)?["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["id"] == copy_id),
        "Central delete retained its selected row"
    );
    ensure!(
        document(c)? == baseline,
        "Central deletion changed a project tool"
    );

    control(c, "New tool", None)?;
    for (label, value) in [
        ("Name", "Independent central mill"),
        ("Tool number (optional)", "21"),
        ("Diameter (mm)", "4"),
        ("Flute length (mm)", "14"),
        ("Overall length (mm)", "45"),
        ("Default spindle (rpm)", "12000"),
        ("Default cutting feed (mm/min)", "600"),
        ("Default plunge feed (mm/min)", "100"),
    ] {
        central_field(c, label, value)?;
    }
    let before_new = fs::read(&path)?;
    control(c, "Create central tool", None)?;
    ensure!(
        fs::read(&path)? != before_new && document(c)? == baseline,
        "Central New did not save independently of the project"
    );
    ensure!(
        library(&path)?["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "Independent central mill" && tool["diameter"] == 4.),
        "Central New did not retain the entered cutter"
    );
    control(c, "Delete tool", None)?;

    select(c, id, &path)?;
    let imported_tool = library(&path)?["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["id"] == id)
        .unwrap()
        .clone();
    control(c, "Import central tool", None)?;
    let imported = document(c)?;
    let mut expected = baseline.clone();
    expected["tools"][0] = imported_tool;
    ensure!(
        imported == expected,
        "Import changed more than the explicit project tool snapshot (or reset operation feeds)"
    );
    let imported_model = c.call("cad_project_model", json!({}))?;
    history(c, &original_model, &imported_model)?;
    control(c, "Undo", None)?;
    ensure!(
        document(c)? == baseline,
        "Undo did not restore project snapshot after import"
    );

    control(c, "Central library", None)?;
    let publish_label = format!(
        "Publish project tool: {}",
        project_tool["name"].as_str().unwrap()
    );
    control(c, &publish_label, None)?;
    ensure!(
        library(&path)?["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["id"] == id)
            == Some(&project_tool),
        "Publish did not copy the complete selected project snapshot"
    );
    ensure!(
        document(c)? == baseline,
        "Publish changed its project source"
    );

    let alternate = config.with_file_name("library-copy");
    fs::create_dir(&alternate).context("Create fresh isolated alternate library")?;
    let original_bytes = fs::read(&path)?;
    control(c, "Library storage", None)?;
    control(c, "Library folder", Some(&alternate.to_string_lossy()))?;
    control(c, "Inspect library folder", None)?;
    capture(c, out, "cam-library-storage")?;
    control(c, "Copy current here", None)?;
    ensure!(
        fs::read(alternate.join("cam-tool-library.json"))? == original_bytes,
        "Storage copy did not preserve the exact reviewed library bytes"
    );
    ensure!(
        fs::read(&path)? == original_bytes && document(c)? == baseline,
        "Storage copy changed source library or project"
    );
    control(c, "Close library", None)?;

    let moved = alternate.with_file_name("library-copy-disconnected");
    ensure!(
        alternate.parent() == config.parent() && !moved.exists(),
        "Isolated recovery fixture directory mismatch"
    );
    fs::rename(&alternate, &moved)?;
    let opened = control(c, "Central library", None);
    ensure!(opened.is_err(), "Disconnected library unexpectedly loaded");
    control(c, "Library storage", None).ok();
    control(c, "Default library folder", None)?;
    control(c, "Use this library", None)?;
    let inspect = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        contains_text(&inspect, &normalized(&path)),
        "Use existing did not recover to the default library after disconnect"
    );
    control(c, "Close library", None)?;
    ensure!(
        document(c)? == baseline && c.call("cad_project_model", json!({}))? == original_model,
        "Library workflow left an unexpected project/history mutation"
    );
    fs::write(
        out.join("cam-central-library.json"),
        serde_json::to_vec_pretty(&json!({
            "config": config, "project_tool_id": id, "central_copy_id": copy_id,
            "verified":["project creation registration","central edit and validation","CAS rejection",
                "explicit import history","explicit publish","copy/delete","storage copy","disconnected recovery"]
        }))?,
    )?;
    Ok(())
}
