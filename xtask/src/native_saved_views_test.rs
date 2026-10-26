//! Finite saved-view acceptance on a fresh owned native host, without OS input.
use crate::{
    native_fixture::{begin_sketch, browser_select, capture, control, controls, panel_field, ui},
    native_platform_test::{wait_for_interface, wait_for_owned_window},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

fn field(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    panel_field(
        c,
        label,
        value,
        "Previous named-view fields",
        "More named-view fields",
    )
}

fn camera(c: &mut Client) -> Result<Value> {
    Ok(ui(
        c,
        json!({"action":"view","view":"current","fit":false,"duration_ms":0}),
    )?["value"]["camera"]
        .clone())
}

fn expected_camera(c: &mut Client, name: &str) -> Result<Value> {
    let views = c.call("named_views", json!({}))?;
    views["views"]
        .as_array()
        .context("Saved views")?
        .iter()
        .find(|v| v["name"] == name)
        .map(|v| v["camera"].clone())
        .context("Saved camera")
}

fn assert_camera(c: &mut Client, expected: &Value) -> Result<()> {
    let actual = camera(c)?;
    for key in ["position", "target", "up"] {
        for axis in 0..3 {
            let a = actual[key][axis]
                .as_f64()
                .context("Native camera coordinate")?;
            let b = expected[key][axis]
                .as_f64()
                .context("Saved camera coordinate")?;
            ensure!(
                (a - b).abs() < 1e-3,
                "Saved-view camera was not restored: {actual} != {expected}"
            );
        }
    }
    Ok(())
}

fn preferences(config: &Path, locale: &str, scale: f64) -> Result<()> {
    fs::write(
        config.join("app-preferences.json"),
        serde_json::to_vec_pretty(
            &json!({"schema_version":1,"theme":"light","locale":locale,"ui_scale":scale}),
        )?,
    )?;
    Ok(())
}

fn minimum_window(c: &Client) -> Result<()> {
    crate::linux_fixture::verify_private_display()?;
    let read =
        |command: &mut Command| crate::linux_fixture::output(command, Duration::from_secs(5));
    let windows = read(Command::new("xdotool").args([
        "search",
        "--onlyvisible",
        "--pid",
        &c.process_id().to_string(),
    ]))?;
    let windows: Vec<_> = windows.lines().collect();
    ensure!(
        windows.len() == 1 && windows[0].bytes().all(|b| b.is_ascii_digit()),
        "Expected one owned native window"
    );
    let window = windows[0];
    let pid = read(Command::new("xdotool").args(["getwindowpid", window]))?;
    ensure!(
        pid.trim().parse::<u32>()? == c.process_id(),
        "Window PID changed"
    );
    // This resize targets only the fixture's verified child on its private Xvfb.
    let status = Command::new("xdotool")
        .args(["windowsize", window, "1200", "760"])
        .status()?;
    ensure!(status.success(), "Owned minimum-window resize failed");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let geometry =
            read(Command::new("xdotool").args(["getwindowgeometry", "--shell", window]))?;
        if geometry.lines().any(|v| v == "WIDTH=1200")
            && geometry.lines().any(|v| v == "HEIGHT=760")
        {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Minimum window was not applied: {geometry}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn exercise(c: &mut Client, out: &Path, config: &Path) -> Result<Value> {
    ensure!(
        c.call("cad_document", json!({}))?["features"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Owned document is not blank"
    );
    begin_sketch(c, "XY")?;
    c.call("sketch_add_rectangle", json!({"mode":"two_point","p1":{"x":-30.,"y":-15.},"p2":{"x":30.,"y":15.},"ctrl_held":true}))?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":20.}}))?;
    let source = c.call("solid_scene", json!({}))?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    control(c, "Named Views", None)?;
    field(c, "View name", Some("Overview"))?;
    field(c, "Save view", None)?;
    control(c, "Close named views", None)?;
    ui(
        c,
        json!({"action":"view","view":"front","fit":true,"duration_ms":0}),
    )?;
    control(c, "Named Views", None)?;
    field(c, "Create named view", None)?;
    field(c, "View name", Some("Front"))?;
    field(c, "Save view", None)?;
    control(c, "Close named views", None)?;
    let overview = expected_camera(c, "Overview")?;
    let front = expected_camera(c, "Front")?;
    ensure!(overview != front, "Fixture views must be visibly different");
    browser_select(c, "Named Views", "Overview")?;
    assert_camera(c, &overview)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        !controls(&state).any(|v| v["label"] == "Close named views"),
        "Browser activation opened the editor"
    );
    capture(c, out, "01-overview")?;
    browser_select(c, "Named Views", "Front")?;
    assert_camera(c, &front)?;
    capture(c, out, "02-front-click")?;
    browser_select(c, "Named Views", "Overview")?;
    assert_camera(c, &overview)?;
    capture(c, out, "03-overview-click")?;
    control(c, "Edit saved view Overview", None)?;
    assert_camera(c, &overview)?;
    let first = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&first).any(|v| v["label"] == "Activate view" && v["disabled"] == false),
        "Activation is missing from the first editor page"
    );
    capture(c, out, "04-edit-separate")?;
    field(c, "View name", Some("Unfinished name"))?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let front_row = controls(&state)
        .find(|v| v["label"] == "Front" && v["role"] == "treeitem")
        .context("Front browser row")?;
    let rejected = c.call(
        "cad_interface",
        json!({"action":"click","target":front_row["id"]}),
    );
    ensure!(
        rejected.is_err() || rejected.is_ok_and(|v| v["status"] == "failed"),
        "Switch discarded unfinished view edits"
    );
    assert_camera(c, &overview)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&state).any(|v| v["label"] == "View name" && v["value"] == "Unfinished name"),
        "Draft name was lost after rejected activation"
    );
    capture(c, out, "05-edits-preserved")?;
    control(c, "Discard edits", None)?;
    browser_select(c, "Named Views", "Front")?;
    assert_camera(c, &front)?;
    capture(c, out, "06-front-after-discard")?;

    minimum_window(c)?;
    let mut locales = Vec::new();
    for (locale, scale, edit, discard) in [
        ("en", 1., "Edit saved view Front", "Discard edits"),
        ("es", 1., "Editar vista guardada Front", "Descartar cambios"),
        (
            "de",
            1.75,
            "Gespeicherte Ansicht Front bearbeiten",
            "Änderungen verwerfen",
        ),
        ("zh-CN", 1.75, "编辑已保存的视图 Front", "放弃更改"),
    ] {
        preferences(config, locale, scale)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = ui(c, json!({"action":"inspect"}))?;
            if controls(&state).any(|v| v["label"] == edit) {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Host did not apply {locale} preferences"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        capture(c, out, &format!("locale-{locale}"))?;
        field(c, "View name", Some("Unfinished Front"))?;
        let state = ui(c, json!({"action":"inspect"}))?;
        let overview = controls(&state)
            .find(|v| v["label"] == "Overview" && v["role"] == "treeitem")
            .context("Overview browser row")?;
        let rejected = c.call(
            "cad_interface",
            json!({"action":"click","target":overview["id"]}),
        );
        ensure!(
            rejected.is_err() || rejected.is_ok_and(|v| v["status"] == "failed"),
            "Localized dirty editor lost its draft"
        );
        capture(c, out, &format!("locale-{locale}-unsaved"))?;
        let png = fs::read(out.join(format!("locale-{locale}-unsaved.png")))?;
        ensure!(
            png.len() >= 24
                && &png[..8] == b"\x89PNG\r\n\x1a\n"
                && u32::from_be_bytes(png[16..20].try_into()?) == 1200
                && u32::from_be_bytes(png[20..24].try_into()?) == 760,
            "Native capture does not cover the supported 1200x760 minimum window"
        );
        control(c, discard, None)?;
        locales.push(json!({"locale":locale,"ui_scale":scale}));
    }
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "View activation changed source geometry"
    );
    Ok(
        json!({"passed":true,"native_camera_restore":true,"separate_edit":true,"draft_preserved":true,
        "locales":locales,"minimum_window":[1200,760],"not_proven":["physical keyboard","OS pointer delivery","visual correctness before inspecting captures"]}),
    )
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    crate::linux_fixture::verify_private_display()?;
    let mut server = None;
    let mut out = None;
    while let Some(arg) = args.next() {
        let value = args.next().context("Missing option value")?;
        match arg.as_str() {
            "--server" if server.is_none() => server = Some(PathBuf::from(value)),
            "--out" if out.is_none() => out = Some(PathBuf::from(value)),
            _ => anyhow::bail!("Unknown or duplicate saved-view option {arg}"),
        }
    }
    let server = server.context("Use --server")?.canonicalize()?;
    let out = out.context("Use --out")?;
    ensure!(
        out.is_absolute() && (!out.exists() || fs::read_dir(&out)?.next().is_none()),
        "Choose a fresh absolute evidence directory"
    );
    fs::create_dir_all(&out)?;
    let config = out.join("config");
    fs::create_dir(&config)?;
    preferences(&config, "en", 1.)?;
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
        exercise(&mut client, &out, &config)
    })();
    let report = match &checked {
        Ok(value) => value.clone(),
        Err(error) => json!({"passed":false,"error":format!("{error:#}")}),
    };
    fs::write(
        out.join("saved-views.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    checked.map(|_| ())
}
