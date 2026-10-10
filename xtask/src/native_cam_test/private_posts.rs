//! Real profile Save/Refresh/catalog controls, using the isolated fixture root.
//! File chooser import is separately covered by native IO tests and OS review.
use super::*;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn text_contains(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.contains(needle),
        Value::Array(values) => values.iter().any(|value| text_contains(value, needle)),
        Value::Object(values) => values.values().any(|value| text_contains(value, needle)),
        _ => false,
    }
}

/// Call after the native machine editor applied a valid profile, with the
/// machine section retained. This flow must never change the pinned setup.
pub(super) fn check(c: &mut Client, out: &Path) -> Result<()> {
    let config = super::central::owned_config(out)?;
    let baseline = document(c)?;
    let machine = baseline["setups"][0]["machine"].clone();
    let mut saved_machine = machine.clone();
    saved_machine["tool_calls"] = json!([]);
    ensure!(
        !machine.is_null(),
        "Apply a machine before profile storage QA"
    );
    let before = c.call("cad_project_model", json!({}))?;
    control(c, "Save profile", None)?;
    let directory = config.join("cam-posts");
    let deadline = Instant::now() + Duration::from_secs(15);
    let saved = loop {
        let _ = ui(c, json!({"action":"inspect"}))?;
        let saved = fs::read_dir(&directory)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "nbpost")
            })
            .filter_map(|entry| {
                fs::read(entry.path()).ok().and_then(|bytes| {
                    serde_json::from_slice::<Value>(&bytes)
                        .ok()
                        .map(|value| (entry.path(), value))
                })
            })
            .find(|(_, value)| value["machine"] == saved_machine);
        if let Some(saved) = saved {
            break saved;
        }
        ensure!(
            Instant::now() < deadline,
            "Save profile did not persist the exact applied machine"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    let invalid = directory.join("fixture-invalid.nbpost");
    let reference = directory.join("fixture-reference.cps");
    ensure!(
        !invalid.exists() && !reference.exists(),
        "Private post QA filenames already exist"
    );
    let invalid_source = b"{\"format\":\"invalid-native-format\"}";
    fs::write(&invalid, invalid_source)?;
    let reference_source = b"// Private reference only; this source must never execute.\nthrow new Error('Do not execute fixture');\n";
    fs::write(&reference, reference_source)?;
    field(c, "Setup section", "private_posts")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let view = loop {
        let view = ui(c, json!({"action":"inspect"}))?;
        if text_contains(&view, "fixture-invalid.nbpost")
            && text_contains(&view, "fixture-reference.cps")
        {
            break view;
        }
        ensure!(
            Instant::now() < deadline,
            "Private post catalog did not expose both source diagnostics: {view}"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    ensure!(text_contains(&view, "Reference only") && text_contains(&view, "Needs attention")
        && text_contains(&view, "Machine profile"),
        "Private post catalog did not distinguish runnable, reference-only, and invalid files: {view}");
    capture(c, out, "cam-private-posts")?;
    control(c, "Refresh posts", None)?;
    ensure!(
        document(c)? == baseline && c.call("cad_project_model", json!({}))? == before,
        "Per-user profile/catalog actions changed the project machine or CAD document"
    );
    ensure!(
        fs::read(&reference)? == reference_source && fs::read(&invalid)? == invalid_source,
        "Refreshing source diagnostics changed private post bytes"
    );
    field(c, "Setup section", "machine")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        panel_field(
            c,
            "Machine / controller",
            Some("snapshot"),
            "Previous fields",
            "More fields",
        )?;
        let view = ui(c, json!({"action":"inspect"}))?;
        let offers_saved = controls(&view)
            .filter(|control| control["label"] == "Machine / controller")
            .any(|control| {
                control["options"].as_array().is_some_and(|options| {
                    options.iter().any(|option| {
                        option["value"]
                            .as_str()
                            .is_some_and(|value| value.starts_with("private:"))
                            && option["label"].as_str().is_some_and(|label| {
                                label.contains(machine["profile"]["name"].as_str().unwrap())
                            })
                    })
                })
            });
        if offers_saved {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Saved private profile did not become a named choice"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    ensure!(
        document(c)? == baseline,
        "Listing a saved profile silently assigned it to the setup"
    );
    fs::write(
        out.join("cam-private-posts.json"),
        serde_json::to_vec_pretty(&json!({
            "directory": directory, "saved_profile": saved.0,
            "verified":["exact applied profile saved", "invalid and reference-only diagnostics", "refresh preserves source bytes", "project snapshot unchanged"]
        }))?,
    )?;
    Ok(())
}
