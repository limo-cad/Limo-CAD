//! Finite Projection acceptance on one fresh child in this process's private Xvfb.
//! Modeling setup uses MCP; keyboard assertions use real XTEST events.
use crate::{
    native_fixture::{capture, control, controls, ui},
    native_platform_test::{wait_for_interface, wait_for_owned_window},
    native_saved_views_test::{minimum_window, recording::Recording},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn read(command: &mut Command) -> Result<String> {
    crate::linux_fixture::output(command, Duration::from_secs(5))
}

fn owned_window(c: &Client) -> Result<String> {
    crate::linux_fixture::verify_private_display()?;
    let windows = read(Command::new("xdotool").args([
        "search",
        "--onlyvisible",
        "--pid",
        &c.process_id().to_string(),
    ]))?;
    let windows: Vec<_> = windows.lines().collect();
    ensure!(
        windows.len() == 1
            && !windows[0].is_empty()
            && windows[0].bytes().all(|b| b.is_ascii_digit()),
        "Expected one owned Projection window"
    );
    let window = windows[0];
    ensure!(
        read(Command::new("xdotool").args(["getwindowpid", window]))?.parse::<u32>()?
            == c.process_id(),
        "Projection window owner changed"
    );
    Ok(window.to_owned())
}

fn key(c: &mut Client, name: &str) -> Result<Value> {
    ensure!(
        ["Tab", "ISO_Left_Tab", "Return", "Escape"].contains(&name),
        "Unsupported fixture key"
    );
    let window = owned_window(c)?;
    read(Command::new("xdotool").args(["windowfocus", "--sync", &window]))?;
    ensure!(
        owned_window(c)? == window
            && read(Command::new("xdotool").arg("getwindowfocus"))? == window,
        "Projection window lost focus; no key sent"
    );
    read(Command::new("xdotool").args(["key", "--clearmodifiers", name]))?;
    ui(c, json!({"action":"inspect"}))
}

fn focused(state: &Value, label: &str) -> bool {
    controls(state).any(|v| v["id"] == state["ui"]["focused_control"] && v["label"] == label)
}

fn scaled_minimum_client(state: &Value) -> bool {
    [("width", 1200.), ("height", 760.)]
        .into_iter()
        .all(|(axis, physical)| {
            state["ui"]["client"][axis]
                .as_f64()
                .is_some_and(|actual| actual.is_finite() && (actual * 1.75 - physical).abs() < 1.)
        })
}

fn minimum_capture(c: &mut Client, out: &Path, name: &str) -> Result<()> {
    let captured = ui(
        c,
        json!({"action":"capture","path":out.join(format!("{name}.png"))}),
    )?;
    ensure!(
        captured["value"]["width"].as_u64() == Some(1200)
            && captured["value"]["height"].as_u64() == Some(760),
        "Native minimum PNG dimensions disagree: {}",
        captured["value"]
    );
    Ok(())
}

fn tab_to(c: &mut Client, label: &str) -> Result<()> {
    // Traverse actual retained ribbon/menu/picker controls, never a fabricated pair.
    let mut visited = Vec::new();
    for _ in 0..96 {
        let state = key(c, "Tab")?;
        if let Some(v) = controls(&state).find(|v| v["id"] == state["ui"]["focused_control"]) {
            visited.push(v["label"].clone());
        }
        if focused(&state, label) {
            return Ok(());
        }
    }
    anyhow::bail!("OS Tab did not reach {label}; visited {visited:?}")
}

fn active(c: &mut Client) -> Result<Value> {
    c.call("sketch_active", json!({}))
}
fn count(c: &mut Client) -> Result<usize> {
    c.call("sketch_projections", json!({}))?["projections"]
        .as_array()
        .map(Vec::len)
        .context("Projection list missing")
}
fn open(c: &mut Client) -> Result<()> {
    control(c, "EDIT tools", None)?;
    tab_to(c, "Project")?;
    let state = key(c, "Return")?;
    ensure!(
        controls(&state).any(|v| v["surface"] == "sketch/projection"),
        "OS Enter did not open Project"
    );
    Ok(())
}
fn shot(c: &mut Client, out: &Path, name: &str, recording: &mut Recording) -> Result<()> {
    capture(c, out, name)?;
    recording.hold(name)
}

fn exercise(c: &mut Client, out: &Path, config: &Path) -> Result<Value> {
    ensure!(
        c.call("cad_document", json!({}))?["features"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Owned Projection document is not blank"
    );
    c.call(
        "sketch_begin",
        json!({"name":"Source curves","plane":{"type":"origin_plane","plane":"xy"}}),
    )?;
    c.call("sketch_set_grid_snap", json!({"enabled":false}))?;
    c.call("sketch_add_circle", json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":8.,"y":0.},"ctrl_held":true}))?;
    for index in 0..7 {
        c.call(
            "sketch_add_line",
            json!({"from":{"x":-20.+f64::from(index)*6.,"y":-12.},
            "to_raw":{"x":-16.+f64::from(index)*6.,"y":-8.},"ctrl_held":true}),
        )?;
    }
    control(c, "Finish sketch", None)?;
    c.call(
        "solid_extrude",
        json!({"sketch_name":"Source curves","profile_indices":[0],
        "operation":"new_body","extent":{"type":"distance","distance":5.},"target_body_ids":[]}),
    )?;
    let plane = c.call(
        "construction_plane_offset",
        json!({"name":"Projection target plane",
        "reference":{"type":"origin_plane","plane":"xy"},"distance":25.}),
    )?;
    ensure!(
        plane["planes"][0]["datum_id"].is_u64(),
        "Missing target plane ID"
    );
    c.call(
        "sketch_begin",
        json!({"name":"Projected references","plane":{"type":"datum_plane",
        "datum_id":plane["planes"][0]["datum_id"]}}),
    )?;
    let sources = c.call("sketch_projection_sources", json!({}))?;
    ensure!(
        sources["sources"].as_array().is_some_and(|v| v.len() == 8),
        "Fixture sources missing"
    );
    let before = active(c)?;
    let mut recording = Recording::start(out)?;
    shot(c, out, "00-target", &mut recording)?;
    open(c)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let row = controls(&state)
        .find(|v| {
            v["surface"] == "sketch/projection"
                && v["label"].as_str().is_some_and(|s| s.contains("Circle"))
        })
        .context("Circle source row missing")?["label"]
        .as_str()
        .unwrap()
        .to_owned();
    tab_to(c, &row)?;
    key(c, "Return")?;
    ensure!(
        active(c)? == before && count(c)? == 0,
        "Preview changed authored sketch"
    );
    shot(c, out, "01-preview", &mut recording)?;
    let cancelled = key(c, "Escape")?;
    ensure!(
        !controls(&cancelled).any(|v| v["surface"] == "sketch/projection"),
        "OS Escape did not close the Projection picker"
    );
    ensure!(
        active(c)? == before && count(c)? == 0,
        "OS Escape changed sketch"
    );
    shot(c, out, "02-cancel", &mut recording)?;
    open(c)?;
    tab_to(c, &row)?;
    key(c, "Return")?;
    tab_to(c, "Confirm")?;
    let after_reverse = key(c, "ISO_Left_Tab")?;
    ensure!(
        !focused(&after_reverse, "Confirm"),
        "OS reverse Tab did not leave Confirm"
    );
    tab_to(c, "Confirm")?;
    key(c, "Return")?;
    ensure!(
        count(c)? == 1,
        "OS Confirm did not add exactly one reference"
    );
    shot(c, out, "03-confirmed", &mut recording)?;
    ui(c, json!({"action":"history","command":"undo"}))?;
    ensure!(count(c)? == 0, "Undo did not remove reference");
    shot(c, out, "04-undo", &mut recording)?;
    ui(c, json!({"action":"history","command":"redo"}))?;
    ensure!(count(c)? == 1, "Redo did not restore reference");
    shot(c, out, "05-redo", &mut recording)?;
    open(c)?;
    control(c, "References", None)?;
    shot(c, out, "06-references", &mut recording)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let remove = controls(&state)
        .find(|v| {
            v["surface"] == "sketch/projection"
                && v["label"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("Remove "))
        })
        .context("Removal control missing")?["label"]
        .as_str()
        .unwrap()
        .to_owned();
    control(c, &remove, None)?;
    ensure!(count(c)? == 0, "Removal did not remove reference");
    shot(c, out, "07-removed", &mut recording)?;
    recording.finish()?;
    // Reused recorder retains its established filenames and conservative scope.
    // projection.json records precisely which real OS events were qualified.
    minimum_window(c)?;
    open(c)?;
    let mut layouts = Vec::new();
    for (locale, confirm, next, previous) in [
        ("en", "Confirm", "Next", "Previous"),
        ("es", "Confirmar", "Siguiente", "Anterior"),
        ("de", "Bestätigen", "Weiter", "Zurück"),
        ("zh-CN", "确认", "下一页", "上一页"),
    ] {
        fs::write(
            config.join("app-preferences.json"),
            serde_json::to_vec(&json!({
            "schema_version":1,"theme":"light","locale":locale,"ui_scale":1.75}))?,
        )?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let state = loop {
            let state = ui(c, json!({"action":"inspect"}))?;
            if scaled_minimum_client(&state)
                && controls(&state)
                    .any(|v| v["surface"] == "sketch/projection" && v["label"] == confirm)
            {
                break state;
            }
            ensure!(Instant::now() < deadline, "Locale did not settle: {locale}");
            std::thread::sleep(Duration::from_millis(50));
        };
        minimum_capture(c, out, &format!("locale-{locale}-minimum"))?;
        control(c, next, None)?;
        minimum_capture(c, out, &format!("locale-{locale}-minimum-next"))?;
        control(c, previous, None)?;
        layouts.push(json!({"locale":locale,"client":state["ui"]["client"],"scale":1.75}));
    }
    Ok(
        json!({"passed":true,"source_revision":std::env::var("GITHUB_SHA").ok(),
        "os_input":"X11 XTEST on the exact owned child in private Xvfb",
        "qualified_os_keys":["Tab","Shift+Tab","Enter","Escape"],
        "model_setup_and_reference_removal":"native control API and shared MCP",
        "preview_and_cancel_preserve_sketch":true,"confirm_undo_redo_remove":true,
        "layouts":layouts,"recording":"saved-views.gif",
        "not_proven":["OS pointer delivery","Windows/macOS OS input","performance","solid-body edge projection"]}),
    )
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    crate::linux_fixture::verify_private_display()?;
    let (mut server, mut out) = (None, None);
    while let Some(arg) = args.next() {
        let value = args.next().context("Missing Projection option value")?;
        match arg.as_str() {
            "--server" if server.is_none() => server = Some(std::path::PathBuf::from(value)),
            "--out" if out.is_none() => out = Some(std::path::PathBuf::from(value)),
            _ => anyhow::bail!("Unknown or duplicate Projection option {arg}"),
        }
    }
    let server = server.context("Use --server")?.canonicalize()?;
    let out = out.context("Use --out")?;
    ensure!(
        out.is_absolute() && (!out.exists() || fs::read_dir(&out)?.next().is_none()),
        "Choose a fresh absolute Projection evidence root"
    );
    fs::create_dir_all(&out)?;
    let config = out.join("config");
    fs::create_dir(&config)?;
    fs::write(
        config.join("app-preferences.json"),
        br#"{"schema_version":1,"theme":"light","locale":"en","ui_scale":1}"#,
    )?;
    let sessions = out.join("sessions");
    limo_cad_session_storage::create_registry(&sessions)?;
    let mut command = Command::new(server);
    command
        .current_dir(&out)
        .env("LIMO_CAD_SESSION_DIR", &sessions)
        .env("LIMO_CAD_CONFIG_DIR", &config);
    let mut client = Client::start_command_logged(command, Some(Duration::from_secs(45)), &out)?;
    let checked = (|| {
        let session = wait_for_owned_window(&mut client, &sessions)?;
        client.call("cad_attach", json!({"session_id":session}))?;
        wait_for_interface(&mut client, &session)?;
        let identity = ui(&mut client, json!({"action":"inspect"}))?;
        ensure!(
            identity["build_pair"]["status"] == "matched",
            "Projection build pair is not matched"
        );
        let expected =
            std::env::var("GITHUB_SHA").context("Use exact GITHUB_SHA on the disposable runner")?;
        for build in [
            &identity["desktop_build"],
            &identity["build_pair"]["desktop"],
            &identity["build_pair"]["mcp"],
        ] {
            ensure!(
                build["revision"] == expected && build["modified"] == false,
                "Projection requires the exact clean source build: {build}"
            );
        }
        fs::write(
            out.join("identity.json"),
            serde_json::to_vec_pretty(&identity)?,
        )?;
        exercise(&mut client, &out, &config)
    })();
    let report = match &checked {
        Ok(report) => report.clone(),
        Err(error) => json!({"passed":false,"error":format!("{error:#}")}),
    };
    fs::write(
        out.join("projection.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    checked.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_layout_waits_for_real_scaled_client_coordinates() {
        assert!(scaled_minimum_client(
            &json!({"ui":{"client":{"width":1200./1.75,"height":760./1.75}}})
        ));
        for client in [
            json!({"width":1200.,"height":760.}),
            json!({"width":1360./1.75,"height":860./1.75}),
            json!({"width":1200./1.75}),
            json!({"width":null,"height":760./1.75}),
        ] {
            assert!(!scaled_minimum_client(&json!({"ui":{"client":client}})));
        }
    }
}
