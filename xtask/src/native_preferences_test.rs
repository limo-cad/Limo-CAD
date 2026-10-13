//! Shared application Settings on an owned native host with a real CAD solid.
//! The fixture writes only its isolated config folder, never a user's settings.
use crate::{
    native_fixture::{
        begin_sketch, capture, control, control_in, controls, owned_config, start, ui,
    },
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

const SURFACE: &str = "document/appearance";

fn dictionary(locale: &str) -> Value {
    serde_json::from_str(match locale {
        "en" => include_str!("../../assets/i18n/en.json"),
        "zh-CN" => include_str!("../../assets/i18n/zh-CN.json"),
        "es" => include_str!("../../assets/i18n/es.json"),
        "de" => include_str!("../../assets/i18n/de.json"),
        _ => unreachable!(),
    })
    .unwrap()
}
fn label(locale: &str, key: &str) -> String {
    let document = dictionary(locale);
    key.split('.')
        .fold(&document, |value, segment| &value[segment])
        .as_str()
        .unwrap_or(key)
        .to_owned()
}
fn inspect(c: &mut Client) -> Result<Value> {
    ui(c, json!({"action":"inspect"}))
}
fn idle_device_control(c: &mut Client) -> Result<Value> {
    let state = inspect(c)?;
    let matches: Vec<_> = controls(&state)
        .filter(|v| v["label"] == "Connect 3D mouse")
        .collect();
    ensure!(
        matches.len() == 1,
        "Expected one explicit 3D mouse Connect control"
    );
    let button = matches[0];
    ensure!(
        button["disabled"] == false && button["selected"] == false,
        "Native host must keep device connection idle until explicit activation: {button}"
    );
    Ok(button.clone())
}
fn settings(state: &Value) -> Result<Value> {
    let text = state["ui"]["surfaces"]
        .as_array()
        .and_then(|items| items.iter().find(|s| s["name"] == SURFACE))
        .and_then(|surface| surface["text"].as_str())
        .context("Native Settings diagnostic snapshot missing")?;
    serde_json::from_str(text).context("Native Settings diagnostic snapshot is not JSON")
}
fn open(c: &mut Client) -> Result<Value> {
    let mut state = inspect(c)?;
    let current = settings(&state)?;
    if current["open"] == true {
        return Ok(state);
    }
    let locale = current["effective"]["locale"]
        .as_str()
        .context("Settings locale")?;
    let caption = label(locale, "topbar.settings");
    if !controls(&state)
        .any(|v| v["surface"] == "file-menu" && v["disabled"] == false && v["label"] == caption)
    {
        control_in(c, "document/session", &label(locale, "file.menu"), None)?;
        state = inspect(c)?;
    }
    let found: Vec<_> = controls(&state)
        .filter(|v| v["surface"] == "file-menu" && v["disabled"] == false && v["label"] == caption)
        .collect();
    ensure!(
        found.len() == 1,
        "Expected one visible File > Settings control: {}; inspection: {state}",
        found.len()
    );
    ui(c, json!({"action":"click","target":found[0]["id"]}))?;
    let state = inspect(c)?;
    ensure!(settings(&state)?["open"] == true, "Settings did not open");
    Ok(state)
}
fn escape(c: &mut Client) -> Result<()> {
    let state = inspect(c)?;
    let current = settings(&state)?;
    let locale = current["effective"]["locale"]
        .as_str()
        .context("Settings locale")?;
    let close = label(locale, "appearance.close");
    let target = controls(&state)
        .find(|v| v["surface"] == SURFACE && v["label"] == close && v["disabled"] == false)
        .context("Settings close control")?;
    ui(
        c,
        json!({"action":"key","target":target["id"],"key":"Escape"}),
    )?;
    ensure!(
        settings(&inspect(c)?)?["open"] == false,
        "Escape did not close Settings"
    );
    Ok(())
}
fn model(c: &mut Client) -> Result<Value> {
    let text = c.call("cad_project_model", json!({}))?;
    Ok(serde_json::from_str(
        text.as_str().context("Project model text")?,
    )?)
}
fn differences(expected: &Value, actual: &Value, path: &str, out: &mut Vec<Value>) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let keys: std::collections::BTreeSet<_> =
                expected.keys().chain(actual.keys()).collect();
            for key in keys {
                differences(
                    &expected.get(key).cloned().unwrap_or(Value::Null),
                    &actual.get(key).cloned().unwrap_or(Value::Null),
                    &format!("{path}/{key}"),
                    out,
                );
            }
        }
        (Value::Array(expected), Value::Array(actual)) if expected.len() == actual.len() => {
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                differences(expected, actual, &format!("{path}/{index}"), out);
            }
        }
        _ if expected != actual => {
            out.push(json!({"path":path,"expected":expected,"actual":actual}))
        }
        _ => {}
    }
}
fn model_evidence(out: &Path, name: &str, value: &Value) -> Result<()> {
    fs::write(
        out.join(format!("{name}.json")),
        serde_json::to_vec_pretty(value)?,
    )?;
    Ok(())
}
fn normalized(path: &str) -> String {
    let value = path.replace('/', "\\").replace("\\\\?\\", "");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}
fn wait(c: &mut Client, predicate: impl Fn(&Value) -> bool) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = inspect(c)?;
        let prefs = settings(&state)?;
        if predicate(&prefs) {
            return Ok(state);
        }
        ensure!(
            Instant::now() < deadline,
            "Shared Settings did not reach expected state: {prefs}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn selected(state: &Value, name: &str) -> bool {
    controls(state).any(|v| {
        v["surface"] == SURFACE
            && v["role"] == "radio"
            && v["label"] == name
            && v["selected"] == true
    })
}
fn assert_controls(state: &Value, locale: &str, name: &str, theme: &str, speed: f64) -> Result<()> {
    ensure!(
        selected(state, name),
        "Selected native-name language is missing: {name}"
    );
    ensure!(
        selected(state, &label(locale, &format!("appearance.{theme}"))),
        "Selected translated theme is missing"
    );
    let name = label(locale, "appearance.sixDofSpeed");
    let slider = controls(state)
        .find(|v| v["surface"] == SURFACE && v["role"] == "slider" && v["label"] == name)
        .context("Translated navigation speed slider")?;
    let value = slider["value"]
        .as_f64()
        .or_else(|| slider["value"].as_str()?.parse().ok());
    ensure!(
        value.is_some_and(|value| (value - speed).abs() < 1e-9),
        "Speed slider value {value:?} did not match {speed}"
    );
    ensure!(
        slider["min"] == 0.25 && slider["max"] == 3.0 && slider["step"] == 0.05,
        "Navigation slider changed its existing range"
    );
    Ok(())
}
fn disk(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn assert_disk(path: &Path, theme: &str, locale: &str, speed: f64) -> Result<Value> {
    let data = disk(path)?;
    ensure!(
        data["schema_version"] == 1
            && data["theme"] == theme
            && data["locale"] == locale
            && data["six_dof_speed"]
                .as_f64()
                .is_some_and(|value| (value - speed).abs() < 1e-9),
        "Settings did not persist the shared fields: {data}"
    );
    Ok(data)
}
fn rejected(c: &mut Client, name: &str, value: Option<&str>) -> Result<String> {
    let state = inspect(c)?;
    let target = controls(&state)
        .find(|v| v["surface"] == SURFACE && v["label"] == name && v["disabled"] == false)
        .context("Settings control for expected rejection")?;
    let request = value.map_or_else(
        || json!({"action":"click","target":target["id"]}),
        |value| json!({"action":"set_value","target":target["id"],"value":value}),
    );
    let result = c.call("cad_interface", request);
    match result {
        Err(error) => Ok(error.to_string()),
        Ok(result) => {
            ensure!(
                result["status"] == "failed",
                "Invalid preference succeeded: {result}"
            );
            Ok(result.to_string())
        }
    }
}
fn external_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path.parent().context("Config parent")?.canonicalize()?;
    ensure!(
        path.file_name()
            .is_some_and(|name| name == "app-preferences.json"),
        "Unexpected preference filename"
    );
    if path.exists() {
        ensure!(
            !fs::symlink_metadata(path)?.file_type().is_symlink()
                && path.canonicalize()?.parent() == Some(directory.as_path()),
            "Preference file redirects outside owned config"
        );
    }
    let pending = directory.join(format!(".qa-app-preferences-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&pending, path)?;
    Ok(())
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-preferences")?;
    let c = &mut fixture.client;
    let config = owned_config(&fixture.out)?;
    let path = config.join("app-preferences.json");
    let empty = model(c)?;
    let initial_device = idle_device_control(c)?;
    let initial = open(c)?;
    let diagnostics = settings(&initial)?;
    ensure!(
        diagnostics["preferences_path"]
            .as_str()
            .is_some_and(|actual| normalized(actual) == normalized(&path.to_string_lossy())),
        "Attached host does not use the owned preference file: {diagnostics}"
    );
    control_in(c, SURFACE, "English", None)?;
    escape(c)?;
    ensure!(
        model(c)? == empty,
        "Initial locale normalization changed the blank document"
    );

    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    let before_creation = model(c)?;
    model_evidence(&fixture.out, "preferences-before-extrude", &before_creation)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}))?;
    control(c, "Fit", None)?;
    control(c, "Undo", None)?;
    let before_extrude = model(c)?;
    model_evidence(&fixture.out, "preferences-undo-baseline", &before_extrude)?;
    let mut baseline_diff = Vec::new();
    differences(&before_creation, &before_extrude, "", &mut baseline_diff);
    model_evidence(
        &fixture.out,
        "preferences-initial-undo-diff",
        &json!(baseline_diff),
    )?;
    control(c, "Redo", None)?;
    let expected = model(c)?;
    model_evidence(&fixture.out, "preferences-solid-expected", &expected)?;
    let mut captures = Vec::new();
    for theme in ["dark", "light"] {
        open(c)?;
        control_in(
            c,
            SURFACE,
            &label("en", &format!("appearance.{theme}")),
            None,
        )?;
        control_in(
            c,
            SURFACE,
            &label("en", "appearance.sixDofSpeed"),
            Some("2.35"),
        )?;
        assert_disk(&path, theme, "en", 2.35)?;
        assert_controls(&inspect(c)?, "en", "English", theme, 2.35)?;
        let name = format!("preferences-settings-{theme}");
        capture(c, &fixture.out, &name)?;
        captures.push(name);
        escape(c)?;
        idle_device_control(c)?;
        let name = format!("preferences-workspace-{theme}");
        capture(c, &fixture.out, &name)?;
        captures.push(name);
        ensure!(model(c)? == expected, "Theme changed document intent");
    }
    open(c)?;
    let before_invalid = fs::read(&path)?;
    let invalid = rejected(c, &label("en", "appearance.sixDofSpeed"), Some("3.25"))?;
    ensure!(
        fs::read(&path)? == before_invalid,
        "Invalid speed overwrote shared preferences"
    );
    control_in(c, SURFACE, &label("en", "appearance.reset"), None)?;
    assert_disk(&path, "light", "en", 1.5)?;

    for (locale, native_name) in [
        ("es", "Español"),
        ("de", "Deutsch"),
        ("zh-CN", "简体中文"),
        ("en", "English"),
    ] {
        control_in(c, SURFACE, native_name, None)?;
        assert_disk(&path, "light", locale, 1.5)?;
        assert_controls(&inspect(c)?, locale, native_name, "light", 1.5)?;
        let name = format!("preferences-locale-{locale}");
        capture(c, &fixture.out, &name)?;
        captures.push(name);
        escape(c)?;
        let state = inspect(c)?;
        ensure!(
            controls(&state).any(|v| v["label"] == label(locale, "ribbon.solid.extrude")),
            "Ribbon did not use canonical {locale} Extrude caption"
        );
        ensure!(
            model(c)? == expected,
            "Locale changed document names or geometry"
        );
        open(c)?;
        assert_controls(&inspect(c)?, locale, native_name, "light", 1.5)?;
    }

    let mut external = disk(&path)?;
    external["theme"] = json!("dark");
    external["locale"] = json!("de");
    external["six_dof_speed"] = json!(2.75);
    external["qa_future"] = json!({"preserve":["unknown metadata",17]});
    external_write(&path, &serde_json::to_vec_pretty(&external)?)?;
    let external_state = wait(c, |p| {
        p["effective"]["theme"] == "dark"
            && p["effective"]["locale"] == "de"
            && p["effective"]["six_dof_speed"] == 2.75
            && p["error"].is_null()
    })?;
    assert_controls(&external_state, "de", "Deutsch", "dark", 2.75)?;
    capture(c, &fixture.out, "preferences-external-refresh")?;
    captures.push("preferences-external-refresh".into());
    control_in(c, SURFACE, &label("de", "appearance.light"), None)?;
    let latest = assert_disk(&path, "light", "de", 2.75)?;
    ensure!(
        latest["qa_future"] == external["qa_future"],
        "Native field patch discarded unrelated shared metadata"
    );

    let corrupt = b"invalid owned QA preferences";
    external_write(&path, corrupt)?;
    let corrupt_state = wait(c, |p| {
        p["error"]
            .as_str()
            .is_some_and(|error| error.contains("parse"))
    })?;
    assert_controls(&corrupt_state, "de", "Deutsch", "light", 2.75)?;
    let failed_save = control_in(c, SURFACE, &label("de", "appearance.dark"), None)?;
    ensure!(
        fs::read(&path)? == corrupt,
        "Failed native save replaced corrupt preference data"
    );
    let pending_state = inspect(c)?;
    let pending = settings(&pending_state)?;
    ensure!(
        pending["pending"]["theme"] == "dark"
            && pending["explicit"]["theme"] == "light"
            && !pending["error"].is_null(),
        "A failed save must retain the live choice and explicit pending state: {pending}"
    );
    assert_controls(&pending_state, "de", "Deutsch", "dark", 2.75)?;
    capture(c, &fixture.out, "preferences-corrupt-pending")?;
    captures.push("preferences-corrupt-pending".into());
    let mut recovered = latest;
    recovered["six_dof_speed"] = json!(2.6);
    external_write(&path, &serde_json::to_vec_pretty(&recovered)?)?;
    let awaiting_retry = wait(c, |p| {
        p["explicit"]["six_dof_speed"] == 2.6
            && p["effective"]["theme"] == "dark"
            && p["pending"]["theme"] == "dark"
            && !p["error"].is_null()
    })?;
    assert_controls(&awaiting_retry, "de", "Deutsch", "dark", 2.6)?;
    control_in(
        c,
        SURFACE,
        &label("de", "appearance.retryPreferences"),
        None,
    )?;
    let retried = wait(c, |p| {
        p["error"].is_null() && p["pending"]["theme"].is_null()
    })?;
    assert_controls(&retried, "de", "Deutsch", "dark", 2.6)?;
    ensure!(
        assert_disk(&path, "dark", "de", 2.6)?["qa_future"] == external["qa_future"],
        "Retry discarded unrelated shared metadata"
    );
    control_in(c, SURFACE, "English", None)?;
    control_in(c, SURFACE, &label("en", "appearance.reset"), None)?;
    control_in(c, SURFACE, &label("en", "appearance.dark"), None)?;
    let final_preferences = assert_disk(&path, "dark", "en", 1.5)?;
    let final_settings = inspect(c)?;
    capture(c, &fixture.out, "preferences-recovered")?;
    captures.push("preferences-recovered".into());
    escape(c)?;
    ensure!(
        model(c)? == expected,
        "Settings changed the complete project model"
    );
    control(c, "Undo", None)?;
    let after_undo = model(c)?;
    model_evidence(&fixture.out, "preferences-after-undo", &after_undo)?;
    let mut history_diff = Vec::new();
    differences(&before_extrude, &after_undo, "", &mut history_diff);
    model_evidence(&fixture.out, "preferences-undo-diff", &json!(history_diff))?;
    ensure!(
        after_undo == before_extrude,
        "Application preferences polluted Undo history: {}",
        serde_json::to_string(&history_diff)?
    );
    control(c, "Redo", None)?;
    ensure!(
        model(c)? == expected,
        "Redo did not restore exact real solid intent"
    );
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&fixture.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        saved == expected,
        "Save persisted application settings into CAD intent"
    );
    capture(c, &fixture.out, "native-preferences")?;
    captures.push("native-preferences".into());
    let final_device = idle_device_control(c)?;
    for name in &captures {
        ensure!(
            fs::read(fixture.out.join(format!("{name}.png")))?.starts_with(b"\x89PNG\r\n\x1a\n"),
            "Invalid live capture {name}"
        );
    }
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required","server":fixture.server,"session":fixture.session,
            "preferences_path":path,"final_preferences":final_preferences,"initial":initial,"external":external_state,
            "initial_device_control":initial_device,"final_device_control":final_device,
            "corrupt":corrupt_state,"pending":pending_state,"awaiting_retry":awaiting_retry,
            "final_settings":final_settings,"invalid_speed":invalid,"failed_save":failed_save,
            "captures":captures,"final_model":expected,
            "checks":["owned-config-and-host-path","shared-theme-light-dark","four-canonical-locales","speed-range-reset",
                "explicit-idle-device-connection-control",
                "escape-reopen","external-process-refresh","unknown-metadata-preserved","failed-save-live-choice-explicit-retry",
                "exact-model-history-and-archive"],
            "limits":"Interface Escape uses the product key action; this fixture does not claim physical 6DoF hardware or OS keyboard coverage."
        }))?,
    )?;
    fixture.client.finish(Duration::from_secs(5))?;
    println!("PASS native application preferences, shared persistence and exact document/history/archive; review captures");
    Ok(())
}
