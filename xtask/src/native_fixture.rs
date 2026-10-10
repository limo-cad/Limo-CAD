//! Shared MCP transport and blank-document guards for native live fixtures.
mod settle;
pub(crate) use settle::inspect_after_gesture;

use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, path::PathBuf, process::Command, time::Duration};

/// Storage fixtures must use the owned evidence root's dedicated config folder.
/// This checks our path boundary; each fixture also verifies the host's exposed
/// configuration path before asking it to write preferences or private data.
pub(super) fn owned_config(out: &std::path::Path) -> Result<PathBuf> {
    let path = PathBuf::from(
        std::env::var_os("LIMO_CAD_CONFIG_DIR")
            .context("Native storage QA requires an isolated LIMO_CAD_CONFIG_DIR")?,
    );
    ensure!(
        path.is_absolute(),
        "Native storage QA config must be absolute"
    );
    let root = out
        .parent()
        .context("Fixture evidence needs an owned parent")?
        .canonicalize()?;
    ensure!(
        path.file_name().is_some_and(|name| name == "config")
            && path.parent().context("Config parent")?.canonicalize()? == root,
        "Native storage QA config must be the evidence folder's sibling named config"
    );
    if path.exists() {
        let resolved = path.canonicalize()?;
        ensure!(
            resolved.parent() == Some(root.as_path())
                && resolved.file_name().is_some_and(|name| name == "config"),
            "Native storage QA config redirects outside its owned evidence root"
        );
    }
    Ok(path)
}
pub(super) fn controls(value: &Value) -> impl Iterator<Item = &Value> {
    value["ui"]["surfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["controls"].as_array().into_iter().flatten())
}
pub(super) fn ui(client: &mut Client, request: Value) -> Result<Value> {
    if request == json!({"action":"inspect"}) {
        return settle::inspect_ready(client);
    }
    let result = client
        .call("cad_interface", request.clone())
        .with_context(|| format!("Native interface request: {request}"))?;
    ensure!(result["status"] == "applied", "Interface failed: {result}");
    Ok(result)
}
pub(super) fn control(client: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    control_matching(client, None, label, value)
}
pub(super) fn control_in(
    client: &mut Client,
    surface: &str,
    label: &str,
    value: Option<&str>,
) -> Result<Value> {
    control_matching(client, Some(surface), label, value)
}
fn control_matching(
    client: &mut Client,
    surface: Option<&str>,
    label: &str,
    value: Option<&str>,
) -> Result<Value> {
    let inspected = ui(client, json!({"action":"inspect"}))?;
    let found: Vec<_> = controls(&inspected)
        .filter(|c| c["label"] == label && c["disabled"] == false)
        .filter(|c| surface.is_none_or(|surface| c["surface"] == surface))
        .collect();
    ensure!(
        found.len() == 1,
        "Expected one enabled {label}, got {found:?}"
    );
    ui(
        client,
        if let Some(value) = value {
            json!({"action":"set_value","target":found[0]["id"],"value":value})
        } else {
            json!({"action":"click","target":found[0]["id"]})
        },
    )
}
pub(super) fn sketch(client: &mut Client) -> Result<Value> {
    client.call("sketch_active", json!({}))
}
pub(super) fn begin_sketch(client: &mut Client, plane: &str) -> Result<Value> {
    control(client, "Create Sketch", None)?;
    browser_select(client, "Origin", plane)
}
pub(super) fn browser_select(client: &mut Client, folder: &str, name: &str) -> Result<Value> {
    let mut inspected = ui(client, json!({"action":"inspect"}))?;
    let matches = |c: &&Value| {
        c["label"] == name
            && c["surface"] == "solid/selection"
            && c["role"] == "treeitem"
            && c["disabled"] == false
    };
    if controls(&inspected).find(matches).is_none() {
        control(client, &format!("Expand {folder}"), None)?;
        inspected = ui(client, json!({"action":"inspect"}))?;
    }
    let found: Vec<_> = controls(&inspected).filter(matches).collect();
    ensure!(found.len() == 1, "Expected one visible browser row {name}");
    ui(client, json!({"action":"click","target":found[0]["id"]}))
}
pub(super) fn click(client: &mut Client, point: [f64; 2], shift: bool) -> Result<Value> {
    ui(
        client,
        json!({"action":"viewport","gesture":"click","world":[point[0],point[1],0.],"shift":shift}),
    )
}

pub(super) struct Fixture {
    pub client: Client,
    pub server: String,
    pub session: String,
    pub out: PathBuf,
    pub project: PathBuf,
    pub capture: PathBuf,
    pub report: PathBuf,
}
pub(super) fn start(mut args: impl Iterator<Item = String>, name: &str) -> Result<Fixture> {
    let mut options = HashMap::new();
    while let Some(key) = args.next() {
        ensure!(
            ["--server", "--session", "--out"].contains(&key.as_str()),
            "Unknown option {key}"
        );
        let value = args
            .next()
            .with_context(|| format!("Missing value for {key}"))?;
        ensure!(
            options.insert(key.clone(), value).is_none(),
            "Duplicate option {key}"
        );
    }
    let server = options
        .get("--server")
        .context("Use --server PATH for the rebuilt CAD binary")?;
    let session = options
        .get("--session")
        .context("Use --session UUID for an existing blank native document")?;
    let out = PathBuf::from(
        options
            .get("--out")
            .context("Use --out PATH for local evidence")?,
    );
    ensure!(out.is_absolute(), "The evidence directory must be absolute");
    ensure!(
        !out.exists() || fs::read_dir(&out)?.next().is_none(),
        "Choose an empty evidence directory; preserve partial runs as well as completed results"
    );
    let project = out.join(format!("{name}.limo"));
    let capture = out.join(format!("{name}.png"));
    let report = out.join(format!("{name}.json"));
    ensure!(
        !project.exists() && !capture.exists() && !report.exists(),
        "Choose a fresh evidence directory; existing results are preserved"
    );
    fs::create_dir_all(&out)?;
    let mut command = Command::new(server);
    command.arg("--headless");
    let mut client = Client::start_command(command, Some(Duration::from_secs(45)))?;
    client.call("cad_attach", json!({"session_id":session}))?;
    let document = client.call("cad_document", json!({}))?;
    ensure!(
        document["features"].as_array().is_some_and(Vec::is_empty),
        "The selected document contains work; choose a blank document"
    );
    ensure!(
        sketch(&mut client)?.is_null(),
        "Finish the active sketch or choose a blank document"
    );

    Ok(Fixture {
        client,
        server: server.clone(),
        session: session.clone(),
        out,
        project,
        capture,
        report,
    })
}

pub(super) fn edit_feature(client: &mut Client, name: &str, context_menu: bool) -> Result<()> {
    let state = ui(client, json!({"action":"inspect"}))?;
    let target = controls(&state)
        .find(|c| c["label"] == name && c["surface"] == "document/history")
        .context("History feature missing")?;
    ui(
        client,
        json!({"action":if context_menu {"context_menu"} else {"double_click"},"target":target["id"]}),
    )?;
    if context_menu {
        control(client, "Edit feature", None)?;
    }
    Ok(())
}

pub(super) fn field(client: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    panel_field(
        client,
        label,
        value,
        "Scroll feature up",
        "Scroll feature down",
    )
}
pub(super) fn panel_field(
    client: &mut Client,
    label: &str,
    value: Option<&str>,
    up: &str,
    down: &str,
) -> Result<Value> {
    for pass in 0..2 {
        if pass == 1 {
            for _ in 0..12 {
                let state = ui(client, json!({"action":"inspect"}))?;
                if !controls(&state).any(|c| c["label"] == up && c["disabled"] == false) {
                    break;
                }
                control(client, up, None)?;
            }
        }
        for _ in 0..12 {
            let state = ui(client, json!({"action":"inspect"}))?;
            let found: Vec<_> = controls(&state)
                .filter(|c| {
                    c["disabled"] == false
                        && c["label"]
                            .as_str()
                            .is_some_and(|s| s == label || s.starts_with(&format!("{label}: ")))
                })
                .collect();
            if found.len() == 1 {
                return match panel_field_action(found[0], value)? {
                    Some(request) => ui(client, request),
                    None => Ok(state),
                };
            }
            ensure!(found.is_empty(), "Ambiguous feature control {label}");
            if !controls(&state).any(|c| c["label"] == down && c["disabled"] == false) {
                break;
            }
            control(client, down, None)?;
        }
    }
    anyhow::bail!("No visible, enabled feature control {label}")
}
fn panel_field_action(control: &Value, value: Option<&str>) -> Result<Option<Value>> {
    if let Some(value) = value {
        if control["role"] == "checkbox" {
            let desired = value
                .parse::<bool>()
                .context("Checkbox fixture values must be exactly true or false")?;
            let current = control["value"]
                .as_bool()
                .context("Published checkbox must expose a Boolean value")?;
            return Ok(
                (current != desired).then(|| json!({"action":"click","target":control["id"]}))
            );
        }
        return Ok(Some(
            json!({"action":"set_value","target":control["id"],"value":value}),
        ));
    }
    Ok(Some(json!({"action":"click","target":control["id"]})))
}
pub(super) fn capture(client: &mut Client, out: &std::path::Path, name: &str) -> Result<()> {
    ui(
        client,
        json!({"action":"capture","path":out.join(format!("{name}.png"))}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_checkbox_uses_click_only_when_requested_boolean_differs() {
        for current in [false, true] {
            let control = json!({"id":"current-control", "role":"checkbox", "value":current});
            assert!(panel_field_action(&control, Some(&current.to_string()))
                .unwrap()
                .is_none());
            assert_eq!(
                panel_field_action(&control, Some(&(!current).to_string())).unwrap(),
                Some(json!({"action":"click","target":"current-control"}))
            );
            for invalid in ["TRUE", "1", " false ", ""] {
                assert!(panel_field_action(&control, Some(invalid)).is_err());
            }
        }
        let malformed = json!({"id":"current-control", "role":"checkbox", "value":"false"});
        assert!(panel_field_action(&malformed, Some("true")).is_err());
        let text = json!({"id":"text-control", "role":"textbox", "value":"false"});
        assert_eq!(
            panel_field_action(&text, Some("true")).unwrap(),
            Some(json!({"action":"set_value","target":"text-control","value":"true"}))
        );
    }
}
